// SPDX-License-Identifier: GPL-3.0-only

//! Driving `codex app-server`: JSON-RPC over stdio, one message a line.
//!
//! The server is started with the user's ChatGPT login forced and the API
//! keys unset, so turns count against their subscription. Its messages
//! carry no `"jsonrpc"` field. After the `initialize` handshake a thread is
//! started, or resumed by its id, and each user turn is a `turn/start`.
//!
//! A Codex thread is a list of items — messages, reasoning, commands, file
//! changes, tool calls — each started, streamed and completed. They become
//! the [`Event`]s Claude's CLI gives: each streamed item is a block of one
//! message per turn, commands are `Bash` tool calls, file changes `Edit` or
//! `Write` calls whose input holds the patch ([`super::model::ToolCall::patch`])
//! and plans `TodoWrite`s. Approvals are server requests, answered by their
//! JSON-RPC id. The composer's permission modes map onto an approval
//! policy, a sandbox and who reviews approvals ([`policy`]).
//!
//! A rollout file, `~/.codex/sessions/YYYY/MM/DD/rollout-…-<id>.jsonl`,
//! records the same items, differently spelt: [`Rollout`] reads them back.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::claude::{Decision, Launch};
use super::model::{Event, PermissionRequest};
use super::protocol::Command;

/// A conversation with `codex app-server`
pub struct Codex {
    cwd: String,
    resume: Option<String>,
    model: Option<String>,
    /// The permission mode, by the composer's name for it
    mode: String,
    instructions: String,
    /// Whether the view can show images from this computer's files
    local: bool,
    next_id: u64,
    /// Requests sent and not yet answered, by id, to their method
    requests: HashMap<u64, &'static str>,
    thread: Option<String>,
    turn: Option<String>,
    /// Stop asked for before the turn's id was known
    interrupt: bool,
    /// Commands that came before the thread started
    queued: Vec<Command>,
    /// Whether plan mode is on in the thread
    planning: bool,
    effort: Option<String>,
    /// Settings `thread/settings/update` refused, for the next turn instead
    unsettled: bool,
    /// Approvals asked, by the request id given the view, to their JSON-RPC id
    approvals: HashMap<String, Value>,
    /// Streamed items to their block index
    blocks: HashMap<String, usize>,
    next_block: usize,
    /// Tool calls seen, to their name and input, for their approvals
    tools: HashMap<String, (String, Value)>,
    context_tokens: Option<u64>,
    /// A failed turn's error, given before the turn ends
    error: Option<String>,
}

impl Codex {
    /// Joining an app-server already started: the thread is the one
    /// resumed, and request ids keep clear of those in its replayed log
    pub fn attached(&mut self) {
        if self.thread.is_none() {
            self.thread = self.resume.clone();
        }
        self.next_id = self.next_id.max(1 << 40) + u64::from(rand_offset());
    }

    pub fn new(launch: &Launch) -> Self {
        let args = shlex::split(&launch.args).unwrap_or_default();
        let bypass = args.iter().any(|arg| matches!(arg.as_str(), "--dangerously-bypass-approvals-and-sandbox" | "--yolo"));
        let mode = if bypass { "bypassPermissions".to_string() } else { launch.permission_mode.clone().unwrap_or_default() };
        let model = launch.model.clone().or_else(|| {
            args.windows(2).find(|pair| matches!(pair[0].as_str(), "-m" | "--model")).map(|pair| pair[1].clone())
        });
        Self {
            cwd: launch.cwd.clone(),
            resume: launch.resume.clone(),
            model,
            mode,
            instructions: launch.system_prompt(),
            local: launch.host.is_empty(),
            next_id: 0,
            requests: HashMap::new(),
            thread: None,
            turn: None,
            interrupt: false,
            queued: Vec::new(),
            planning: false,
            effort: None,
            unsettled: false,
            approvals: HashMap::new(),
            blocks: HashMap::new(),
            next_block: 0,
            tools: HashMap::new(),
            context_tokens: None,
            error: None,
        }
    }

    pub fn start(&mut self) -> Vec<String> {
        vec![self.request(
            "initialize",
            json!({
                "clientInfo": {"name": "cosmic_term", "title": "COSMIC Terminal", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": true},
            }),
        )]
    }

    pub fn command(&mut self, command: Command) -> Vec<String> {
        let Some(thread) = self.thread.clone() else {
            match command {
                Command::Interrupt => self.interrupt = self.queued.iter().any(|c| matches!(c, Command::User { .. })),
                Command::SetModel(model) => self.model = Some(model),
                Command::SetMode(mode) => self.mode = mode,
                command => self.queued.push(command),
            }
            return Vec::new();
        };
        match command {
            Command::User { text, images } => {
                let mut input: Vec<Value> = images
                    .iter()
                    .map(|(media_type, data)| json!({"type": "image", "url": format!("data:{media_type};base64,{data}")}))
                    .collect();
                if !text.is_empty() {
                    input.push(json!({"type": "text", "text": text, "text_elements": []}));
                }
                let mut params = json!({"threadId": thread, "input": input, "summary": "auto"});
                if self.unsettled {
                    self.unsettled = false;
                    let (approval, reviewer, sandbox, _) = policy(&self.mode);
                    extend(
                        &mut params,
                        json!({"approvalPolicy": approval, "approvalsReviewer": reviewer, "sandboxPolicy": sandbox_policy(sandbox)}),
                    );
                    if let Some(mode) = self.collaboration() {
                        extend(&mut params, json!({"model": self.model, "collaborationMode": mode}));
                    }
                }
                vec![self.request("turn/start", params)]
            }
            Command::Interrupt => match self.turn.clone() {
                Some(turn) => vec![self.request("turn/interrupt", json!({"threadId": thread, "turnId": turn}))],
                None => {
                    self.interrupt = self.requests.values().any(|method| *method == "turn/start");
                    Vec::new()
                }
            },
            // Questions dismissed are answered with nothing
            Command::Answer { request, .. } if !request.questions.is_empty() => {
                let Some(id) = self.approvals.remove(&request.request_id) else { return Vec::new() };
                vec![json!({"id": id, "result": {"answers": {}}}).to_string()]
            }
            Command::Answers { request, answers } => {
                let Some(id) = self.approvals.remove(&request.request_id) else { return Vec::new() };
                let answers: serde_json::Map<String, Value> =
                    answers.into_iter().map(|(key, answer)| (key, json!({"answers": [answer]}))).collect();
                vec![json!({"id": id, "result": {"answers": answers}}).to_string()]
            }
            Command::Answer { request, decision } => {
                let Some(id) = self.approvals.remove(&request.request_id) else { return Vec::new() };
                let decision = match decision {
                    Decision::Allow => "accept",
                    Decision::AllowAlways => "acceptForSession",
                    Decision::Deny => "decline",
                };
                vec![json!({"id": id, "result": {"decision": decision}}).to_string()]
            }
            Command::SetModel(model) => {
                self.model = Some(model.clone());
                vec![self.request("thread/settings/update", json!({"threadId": thread, "model": model}))]
            }
            Command::SetMode(mode) => {
                self.mode = mode;
                vec![self.settings(&thread)]
            }
        }
    }

    /// The events in one line from the server, and the lines it needs sent
    pub fn parse(&mut self, line: &str, events: &mut Vec<Event>) -> Vec<String> {
        let Ok(mut message) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        let method = message.get("method").and_then(Value::as_str).map(str::to_string);
        let id = message.get("id").cloned();
        match (method, id) {
            (Some(method), Some(id)) => self.server_request(&method, id, message.get("params"), events),
            (Some(method), None) => {
                let params = message.get_mut("params").map(Value::take).unwrap_or(Value::Null);
                self.notification(&method, params, events)
            }
            (None, Some(id)) => self.response(id.as_u64().unwrap_or(u64::MAX), &message, events),
            (None, None) => Vec::new(),
        }
    }

    fn request(&mut self, method: &'static str, params: Value) -> String {
        self.next_id += 1;
        self.requests.insert(self.next_id, method);
        json!({"id": self.next_id, "method": method, "params": params}).to_string()
    }

    /// The thread's permission settings for the current mode
    fn settings(&mut self, thread: &str) -> String {
        let (approval, reviewer, _, profile) = policy(&self.mode);
        let mut params = json!({
            "threadId": thread,
            "approvalPolicy": approval,
            "approvalsReviewer": reviewer,
            "permissions": profile,
        });
        if self.planning != (self.mode == "plan")
            && let Some(mode) = self.collaboration()
        {
            extend(&mut params, json!({"collaborationMode": mode}));
        }
        self.request("thread/settings/update", params)
    }

    /// Plan mode or the default, with the thread's model and effort, once
    /// the model is known
    fn collaboration(&mut self) -> Option<Value> {
        let model = self.model.clone()?;
        self.planning = self.mode == "plan";
        Some(json!({
            "mode": if self.planning { "plan" } else { "default" },
            "settings": {"model": model, "reasoning_effort": self.effort, "developer_instructions": null},
        }))
    }

    fn thread_params(&self) -> Value {
        let (approval, reviewer, sandbox, _) = policy(&self.mode);
        let mut params = json!({
            "approvalPolicy": approval,
            // Always given: the user's own config may route approvals elsewhere
            "approvalsReviewer": reviewer,
            "sandbox": sandbox,
            "developerInstructions": self.instructions,
        });
        if !self.cwd.is_empty() {
            extend(&mut params, json!({"cwd": self.cwd}));
        }
        if let Some(model) = &self.model {
            extend(&mut params, json!({"model": model}));
        }
        params
    }

    fn response(&mut self, id: u64, message: &Value, events: &mut Vec<Event>) -> Vec<String> {
        let method = self.requests.remove(&id).unwrap_or_default();
        let result = message.get("result").unwrap_or(&Value::Null);
        let error = message.pointer("/error/message").and_then(Value::as_str).map(str::to_string);
        let mut out = Vec::new();
        match (method, error) {
            ("initialize", None) => {
                out.push(json!({"method": "initialized"}).to_string());
                let mut params = self.thread_params();
                match self.resume.clone() {
                    Some(thread) => {
                        extend(&mut params, json!({"threadId": thread, "excludeTurns": true}));
                        out.push(self.request("thread/resume", params));
                    }
                    None => out.push(self.request("thread/start", params)),
                }
                out.push(self.request("model/list", json!({})));
            }
            ("thread/start" | "thread/resume", None) => {
                let Some(thread) = result.pointer("/thread/id").and_then(Value::as_str).map(str::to_string) else {
                    return out;
                };
                let model = str_field(result, "model");
                if !model.is_empty() {
                    self.model = Some(model.clone());
                }
                self.effort = result.get("reasoningEffort").and_then(Value::as_str).map(str::to_string);
                events.push(Event::Started {
                    session_id: thread.clone(),
                    model,
                    cwd: str_field(result, "cwd"),
                    permission_mode: self.mode.clone(),
                });
                self.thread = Some(thread.clone());
                if self.mode == "plan" {
                    out.push(self.settings(&thread));
                }
                for command in std::mem::take(&mut self.queued) {
                    out.extend(self.command(command));
                }
            }
            ("thread/resume", Some(error)) => {
                events.push(Event::Error(format!("The conversation could not be continued, so a new one began: {error}")));
                self.resume = None;
                let params = self.thread_params();
                out.push(self.request("thread/start", params));
            }
            ("model/list", None) => events.push(Event::Models(
                result
                    .get("data")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|model| !model.get("hidden").and_then(Value::as_bool).unwrap_or(false))
                    .filter_map(|model| {
                        let id = model.get("model").or_else(|| model.get("id")).and_then(Value::as_str)?;
                        let name = model.get("displayName").and_then(Value::as_str).unwrap_or(id);
                        Some((id.to_string(), name.to_string()))
                    })
                    .collect(),
            )),
            ("turn/start", None) => {
                if let Some(turn) = result.pointer("/turn/id").and_then(Value::as_str) {
                    out.extend(self.turn_started(turn));
                }
            }
            ("turn/start", Some(error)) => events.push(Event::TurnDone {
                error: true,
                message: error,
                duration_ms: 0,
                context_tokens: None,
            }),
            ("thread/settings/update", Some(_)) => self.unsettled = true,
            ("turn/interrupt" | "model/list", Some(_)) => {}
            (_, Some(error)) => events.push(Event::Error(error)),
            _ => {}
        }
        out
    }

    /// The turn in progress is known: a stop asked for meanwhile is sent
    fn turn_started(&mut self, turn: &str) -> Vec<String> {
        self.turn = Some(turn.to_string());
        match (std::mem::take(&mut self.interrupt), self.thread.clone()) {
            (true, Some(thread)) => vec![self.request("turn/interrupt", json!({"threadId": thread, "turnId": turn}))],
            _ => Vec::new(),
        }
    }

    fn server_request(&mut self, method: &str, id: Value, params: Option<&Value>, events: &mut Vec<Event>) -> Vec<String> {
        let params = params.unwrap_or(&Value::Null);
        // Questions for the user, asked as a prompt of their own
        if method == "item/tool/requestUserInput" && self.ours(params) {
            let questions = questions(params);
            if !questions.is_empty() {
                let request_id = match &id {
                    Value::String(id) => id.clone(),
                    id => id.to_string(),
                };
                self.approvals.insert(request_id.clone(), id);
                events.push(Event::Permission(PermissionRequest {
                    request_id,
                    tool_name: "AskUserQuestion".to_string(),
                    tool_use_id: str_field(params, "itemId"),
                    title: String::new(),
                    input: Value::Null,
                    suggestions: Value::Null,
                    always_allowed: false,
                    questions,
                }));
                return Vec::new();
            }
        }
        if !matches!(method, "item/commandExecution/requestApproval" | "item/fileChange/requestApproval")
            || !self.ours(params)
        {
            return vec![json!({"id": id, "error": {"code": -32601, "message": "not supported by this client"}}).to_string()];
        }
        let item = str_field(params, "itemId");
        let request_id = match &id {
            Value::String(id) => id.clone(),
            id => id.to_string(),
        };
        let (tool_name, input) = match self.tools.get(&item) {
            Some((name, input)) => (name.clone(), input.clone()),
            None if method == "item/fileChange/requestApproval" => ("Edit".to_string(), json!({})),
            None => {
                // Asked before the item started: its card is made now
                let input = json!({"command": shell_script(params.get("command").unwrap_or(&Value::Null))});
                let block = json!({"type": "tool_use", "id": item, "name": "Bash", "input": input});
                events.push(Event::Assistant { message_id: item.clone(), content: json!([block]), parent: None });
                self.tools.insert(item.clone(), ("Bash".to_string(), input.clone()));
                ("Bash".to_string(), input)
            }
        };
        self.approvals.insert(request_id.clone(), id);
        events.push(Event::Permission(PermissionRequest {
            request_id,
            tool_name,
            tool_use_id: item,
            title: str_field(params, "reason"),
            input,
            suggestions: json!(["acceptForSession"]),
            always_allowed: true,
            questions: Vec::new(),
        }));
        Vec::new()
    }

    /// Whether a message is about this thread, not another, such as a
    /// subagent's
    fn ours(&self, params: &Value) -> bool {
        match params.get("threadId").and_then(Value::as_str) {
            Some(thread) => self.thread.as_deref() == Some(thread),
            None => true,
        }
    }

    fn notification(&mut self, method: &str, params: Value, events: &mut Vec<Event>) -> Vec<String> {
        if !self.ours(&params) {
            return Vec::new();
        }
        let field = |name: &str| str_field(&params, name);
        match method {
            "turn/started" => {
                let turn = params.pointer("/turn/id").and_then(Value::as_str).unwrap_or_default().to_string();
                events.push(Event::MessageStart { message_id: turn.clone(), parent: None });
                if self.turn.as_deref() != Some(turn.as_str()) {
                    return self.turn_started(&turn);
                }
            }
            "item/started" => {
                let item = &params["item"];
                let id = str_field(item, "id");
                match item.get("type").and_then(Value::as_str).unwrap_or_default() {
                    "agentMessage" | "plan" => {
                        let index = self.block(&id);
                        events.push(Event::BlockStart { index, block: json!({"type": "text", "text": ""}), parent: None });
                    }
                    _ => {
                        if let Some(block) = self.tool_use(item) {
                            events.push(Event::Assistant { message_id: id, content: json!([block]), parent: None });
                        }
                    }
                }
            }
            "item/agentMessage/delta" | "item/plan/delta" => {
                if let Some(&index) = self.blocks.get(&field("itemId")) {
                    events.push(Event::TextDelta { index, text: field("delta"), parent: None });
                }
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                let id = field("itemId");
                let index = match self.blocks.get(&id) {
                    Some(&index) => index,
                    None => {
                        let index = self.block(&id);
                        events.push(Event::BlockStart { index, block: json!({"type": "thinking"}), parent: None });
                        index
                    }
                };
                events.push(Event::ThinkingDelta { index, text: field("delta"), tokens: None, parent: None });
            }
            "item/reasoning/summaryPartAdded" => {
                let index = params.get("summaryIndex").and_then(Value::as_u64).unwrap_or(0);
                if let Some(&block) = self.blocks.get(&field("itemId"))
                    && index > 0
                {
                    events.push(Event::ThinkingDelta { index: block, text: "\n\n".into(), tokens: None, parent: None });
                }
            }
            "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
                events.push(Event::ToolOutput { id: field("itemId"), text: field("delta") });
            }
            "item/completed" => {
                let item = &params["item"];
                let id = str_field(item, "id");
                match self.blocks.get(&id) {
                    Some(&index) => events.push(Event::BlockStop { index, parent: None }),
                    // The user's own words are shown as they are sent
                    None if item.get("type").and_then(Value::as_str) == Some("userMessage") => {}
                    None => completed(item, self.local, events),
                }
                self.tools.remove(&id);
            }
            "turn/plan/updated" => {
                let id = format!("plan-{}", field("turnId"));
                plan(&id, &params["plan"], events);
            }
            "thread/tokenUsage/updated" => {
                self.context_tokens = params.pointer("/tokenUsage/last/totalTokens").and_then(Value::as_u64);
            }
            "thread/settings/updated" => {
                let settings = &params["threadSettings"];
                if let Some(effort) = settings.get("effort").and_then(Value::as_str) {
                    self.effort = Some(effort.to_string());
                }
            }
            "error" => {
                let message = params.pointer("/error/message").and_then(Value::as_str).unwrap_or_default().to_string();
                if params.get("willRetry").and_then(Value::as_bool).unwrap_or(false) {
                    events.push(Event::Status(message));
                } else if self.turn.is_some() {
                    self.error = Some(message);
                } else {
                    events.push(Event::Error(message));
                }
            }
            "serverRequest/resolved" => {
                let id = match params.get("requestId") {
                    Some(Value::String(id)) => id.clone(),
                    Some(id) => id.to_string(),
                    None => return Vec::new(),
                };
                self.approvals.remove(&id);
                events.push(Event::PermissionCancelled(id));
            }
            "warning" | "configWarning" => {
                let text = ["message", "summary"].iter().map(|name| field(name)).find(|text| !text.is_empty());
                events.extend(text.map(Event::Notice));
            }
            "turn/completed" => {
                let turn = &params["turn"];
                let failed = turn.get("status").and_then(Value::as_str) == Some("failed");
                let message = turn.pointer("/error/message").and_then(Value::as_str).map(str::to_string);
                let error = self.error.take();
                events.push(Event::TurnDone {
                    error: failed,
                    message: if failed { message.or(error).unwrap_or_default() } else { String::new() },
                    duration_ms: turn.get("durationMs").and_then(Value::as_u64).unwrap_or(0),
                    context_tokens: self.context_tokens,
                });
                self.turn = None;
                self.interrupt = false;
                self.blocks.clear();
            }
            _ => {}
        }
        Vec::new()
    }

    /// A new block in the turn's message, for item `id`
    fn block(&mut self, id: &str) -> usize {
        self.next_block += 1;
        self.blocks.insert(id.to_string(), self.next_block);
        self.next_block
    }

    fn tool_use(&mut self, item: &Value) -> Option<Value> {
        let block = tool_use(item, self.local)?;
        self.tools.insert(str_field(&block, "id"), (str_field(&block, "name"), block["input"].clone()));
        Some(block)
    }
}

/// The approval policy, who reviews approvals, the sandbox and its
/// permission profile, for a composer's permission mode
fn policy(mode: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match mode {
        "acceptEdits" => ("on-request", "user", "workspace-write", ":workspace"),
        // Codex's reviewer approves what is safe and asks about the rest
        "auto" => ("on-request", "auto_review", "workspace-write", ":workspace"),
        "plan" => ("on-request", "user", "read-only", ":read-only"),
        "bypassPermissions" => ("never", "user", "danger-full-access", ":danger-full-access"),
        _ => ("untrusted", "user", "workspace-write", ":workspace"),
    }
}

fn sandbox_policy(sandbox: &str) -> Value {
    match sandbox {
        "read-only" => json!({"type": "readOnly", "networkAccess": false}),
        "danger-full-access" => json!({"type": "dangerFullAccess"}),
        _ => json!({
            "type": "workspaceWrite",
            "writableRoots": [],
            "networkAccess": false,
            "excludeTmpdirEnvVar": false,
            "excludeSlashTmp": false,
        }),
    }
}

/// An item's tool call, as a `tool_use` block, if it is one
fn tool_use(item: &Value, local: bool) -> Option<Value> {
    let field = |name: &str| str_field(item, name);
    let (name, input) = match item.get("type").and_then(Value::as_str)? {
        "commandExecution" => ("Bash".to_string(), json!({"command": shell_script(&item["command"])})),
        "fileChange" => {
            let changes = item.get("changes").cloned().unwrap_or(json!([]));
            let paths: Vec<&str> = changes
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|change| change.get("path").and_then(Value::as_str))
                .collect();
            let adds = changes.as_array().is_some_and(|changes| {
                changes.iter().all(|change| change.pointer("/kind/type").and_then(Value::as_str) == Some("add"))
            });
            let name = if adds { "Write" } else { "Edit" };
            (name.to_string(), json!({"file_path": paths.join(", "), "changes": changes}))
        }
        "mcpToolCall" => (
            format!("mcp__{}__{}", field("server"), field("tool")),
            item.get("arguments").cloned().unwrap_or(Value::Null),
        ),
        "dynamicToolCall" => (field("tool"), item.get("arguments").cloned().unwrap_or(Value::Null)),
        "webSearch" => ("WebSearch".to_string(), json!({"query": field("query")})),
        "imageView" if local => ("Read".to_string(), json!({"file_path": field("path")})),
        "collabAgentToolCall" => {
            let tool = field("tool");
            if !matches!(tool.as_str(), "spawnAgent" | "sendInput" | "sendMessage" | "followupTask") {
                return None;
            }
            ("Agent".to_string(), json!({"description": tool, "prompt": field("prompt")}))
        }
        _ => return None,
    };
    Some(json!({"type": "tool_use", "id": field("id"), "name": name, "input": input}))
}

/// The events of a completed item, without its streaming
fn completed(item: &Value, local: bool, events: &mut Vec<Event>) {
    let field = |name: &str| str_field(item, name);
    let id = field("id");
    let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
    let text_block = |kind: &str, text: String| match kind {
        "thinking" => json!({"type": "thinking", "thinking": text}),
        _ => json!({"type": "text", "text": text}),
    };
    match kind {
        "agentMessage" | "plan" => {
            events.push(Event::Assistant { message_id: id, content: json!([text_block("text", field("text"))]), parent: None });
            return;
        }
        "reasoning" => {
            let text = ["summary", "content"]
                .iter()
                .filter_map(|name| item.get(*name).and_then(Value::as_array))
                .find(|parts| !parts.is_empty())
                .map(|parts| parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n"))
                .unwrap_or_default();
            if !text.trim().is_empty() {
                events.push(Event::Assistant { message_id: id, content: json!([text_block("thinking", text)]), parent: None });
            }
            return;
        }
        "userMessage" => {
            let content: Vec<Value> = item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| match part.get("type").and_then(Value::as_str)? {
                    "text" => Some(json!({"type": "text", "text": str_field(part, "text")})),
                    "image" => {
                        let url = str_field(part, "url");
                        let (media_type, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
                        let mut image = json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}});
                        super::images::spill(&mut image);
                        Some(image)
                    }
                    "localImage" if local => Some(json!({"type": "image", "source": {"path": str_field(part, "path")}})),
                    _ => None,
                })
                .collect();
            if !content.is_empty() {
                events.push(Event::User { content: Value::Array(content), parent: None });
            }
            return;
        }
        "contextCompaction" => {
            events.push(Event::Notice("Conversation compacted".into()));
            return;
        }
        _ => {}
    }
    let Some(block) = tool_use(item, local) else { return };
    events.push(Event::Assistant { message_id: id.clone(), content: json!([block]), parent: None });
    let status = field("status");
    let mut error = matches!(status.as_str(), "failed" | "declined");
    let mut content = Vec::new();
    match kind {
        "commandExecution" => {
            let mut output = field("aggregatedOutput");
            if let Some(code) = item.get("exitCode").and_then(Value::as_i64)
                && code != 0
            {
                error = true;
                if output.is_empty() {
                    output = format!("Exit status {code}");
                }
            }
            if status == "declined" {
                output = "The user declined this command.".into();
            }
            content.push(json!({"type": "text", "text": output}));
        }
        "fileChange" if status == "declined" => {
            content.push(json!({"type": "text", "text": "The user declined this change."}));
        }
        "mcpToolCall" => {
            if let Some(message) = item.pointer("/error/message").and_then(Value::as_str) {
                error = true;
                content.push(json!({"type": "text", "text": message}));
            }
            for part in item.pointer("/result/content").and_then(Value::as_array).into_iter().flatten() {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => content.push(json!({"type": "text", "text": str_field(part, "text")})),
                    Some("image") => {
                        let mut image = json!({"type": "image", "source": {
                            "type": "base64",
                            "media_type": str_field(part, "mimeType"),
                            "data": str_field(part, "data"),
                        }});
                        super::images::spill(&mut image);
                        content.push(image);
                    }
                    _ => {}
                }
            }
        }
        "dynamicToolCall" => {
            error |= item.get("success").and_then(Value::as_bool) == Some(false);
            for part in item.get("contentItems").and_then(Value::as_array).into_iter().flatten() {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    content.push(json!({"type": "text", "text": text}));
                }
            }
        }
        "imageView" => content.push(json!({"type": "image", "source": {"path": field("path")}})),
        _ => {}
    }
    for part in &mut content {
        if let Some(Value::String(text)) = part.get_mut("text") {
            *text = super::model::cut(text, super::model::TOOL_TEXT_LIMIT);
        }
    }
    events.push(Event::User {
        content: json!([{"type": "tool_result", "tool_use_id": id, "content": content, "is_error": error}]),
        parent: None,
    });
}

/// A plan's steps as a `TodoWrite`, updated in place under `id`
fn plan(id: &str, steps: &Value, events: &mut Vec<Event>) {
    let todos: Vec<Value> = steps
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            let status = match step.get("status").and_then(Value::as_str).unwrap_or_default() {
                "inProgress" | "in_progress" => "in_progress",
                "completed" => "completed",
                _ => "pending",
            };
            json!({"content": str_field(step, "step"), "status": status})
        })
        .collect();
    let block = json!({"type": "tool_use", "id": id, "name": "TodoWrite", "input": {"todos": todos}});
    events.push(Event::Assistant { message_id: id.to_string(), content: json!([block]), parent: None });
    events.push(Event::User {
        content: json!([{"type": "tool_result", "tool_use_id": id, "content": "", "is_error": false}]),
        parent: None,
    });
}

/// A command as the user would type it: a shell's `-c` script unwrapped
fn shell_script(command: &Value) -> String {
    let words: Vec<String> = match command {
        Value::String(command) => shlex::split(command).unwrap_or_else(|| vec![command.clone()]),
        Value::Array(words) => words.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => return String::new(),
    };
    match words.as_slice() {
        [shell, flag, script]
            if matches!(shell.rsplit('/').next(), Some("bash" | "sh" | "zsh"))
                && matches!(flag.as_str(), "-c" | "-lc") =>
        {
            script.clone()
        }
        _ => match command {
            Value::String(command) => command.clone(),
            _ => shlex::try_join(words.iter().map(String::as_str)).unwrap_or_else(|_| words.join(" ")),
        },
    }
}

/// Reads a rollout file back, a line at a time, as the events of its
/// items
#[derive(Default)]
pub struct Rollout {
    /// Whether the view can show images from this computer's files
    pub local: bool,
    /// Whether the file records completed items; older ones record events
    /// and model calls instead
    items: bool,
    /// Legacy tool calls not yet answered, by call id
    calls: HashMap<String, Value>,
}

impl Rollout {
    pub fn new(local: bool) -> Self {
        Self { local, ..Self::default() }
    }

    /// The events of one line, and whether it starts a user turn, or `None`
    /// for a line that shows nothing
    pub fn line(&mut self, value: &Value, events: &mut Vec<Event>) -> Option<bool> {
        let payload = value.get("payload")?;
        let kind = payload.get("type").and_then(Value::as_str).unwrap_or_default();
        let field = |name: &str| str_field(payload, name);
        match (value.get("type").and_then(Value::as_str)?, kind) {
            ("event_msg", "item_completed") => {
                self.items = true;
                let item = normalize(payload.get("item")?)?;
                let user = item.get("type").and_then(Value::as_str) == Some("userMessage");
                completed(&item, self.local, events);
                Some(user)
            }
            ("event_msg", "user_message") => {
                let mut content = vec![json!({"type": "text", "text": field("message")})];
                if self.local {
                    for path in payload.get("local_images").and_then(Value::as_array).into_iter().flatten() {
                        content.push(json!({"type": "image", "source": {"path": path}}));
                    }
                }
                events.push(Event::User { content: Value::Array(content), parent: None });
                Some(true)
            }
            ("event_msg", "agent_message") => {
                let item = json!({"type": "agentMessage", "id": field("id"), "text": field("message")});
                completed(&item, self.local, events);
                Some(false)
            }
            ("event_msg", "agent_reasoning") => {
                let item = json!({"type": "reasoning", "summary": [field("text")]});
                completed(&item, self.local, events);
                Some(false)
            }
            ("event_msg", "exec_command_end") => {
                let item = json!({
                    "type": "commandExecution",
                    "id": field("call_id"),
                    "command": payload.get("command"),
                    "aggregatedOutput": field("aggregated_output"),
                    "exitCode": payload.get("exit_code"),
                });
                completed(&item, self.local, events);
                Some(false)
            }
            ("event_msg", "patch_apply_end") => {
                let failed = payload.get("success").and_then(Value::as_bool) == Some(false);
                let item = json!({
                    "type": "fileChange",
                    "id": field("call_id"),
                    "changes": changes(payload.get("changes")?),
                    "status": if failed { "failed" } else { "completed" },
                });
                completed(&item, self.local, events);
                Some(false)
            }
            ("event_msg", "web_search_end") => {
                completed(&json!({"type": "webSearch", "id": field("call_id"), "query": field("query")}), self.local, events);
                Some(false)
            }
            ("event_msg", "context_compacted") => {
                events.push(Event::Notice("Conversation compacted".into()));
                Some(false)
            }
            ("event_msg", "error") => {
                events.push(Event::Error(field("message")));
                Some(false)
            }
            ("response_item", "function_call") if field("name") == "update_plan" => {
                let arguments: Value = serde_json::from_str(&field("arguments")).ok()?;
                plan(&field("call_id"), &arguments["plan"], events);
                Some(false)
            }
            // Before items were recorded, commands were known only as the
            // model's calls
            ("response_item", "function_call" | "custom_tool_call") if !self.items => {
                let input = match field("name").as_str() {
                    "exec_command" => {
                        let arguments: Value = serde_json::from_str(&field("arguments")).unwrap_or_default();
                        json!({"name": "Bash", "input": {"command": str_field(&arguments, "cmd")}})
                    }
                    "exec" => json!({"name": "exec", "input": {"code": field("input")}}),
                    _ => return None,
                };
                let block = json!({"type": "tool_use", "id": field("call_id"), "name": input["name"], "input": input["input"]});
                events.push(Event::Assistant { message_id: field("call_id"), content: json!([block]), parent: None });
                self.calls.insert(field("call_id"), Value::Null);
                Some(false)
            }
            ("response_item", "function_call_output" | "custom_tool_call_output") if !self.items => {
                self.calls.remove(&field("call_id"))?;
                let output = match payload.get("output") {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Array(parts)) => {
                        parts.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")
                    }
                    _ => String::new(),
                };
                let output = super::model::cut(&output, super::model::TOOL_TEXT_LIMIT);
                events.push(Event::User {
                    content: json!([{"type": "tool_result", "tool_use_id": field("call_id"), "content": output}]),
                    parent: None,
                });
                Some(false)
            }
            _ => None,
        }
    }

    /// The conversation's id, if `value` is the line naming it
    pub fn session_id(value: &Value) -> Option<String> {
        (value.get("type").and_then(Value::as_str) == Some("session_meta"))
            .then(|| value.pointer("/payload/id").and_then(Value::as_str).map(str::to_string))
            .flatten()
    }
}

/// A recorded item in the shape the server sends it
fn normalize(item: &Value) -> Option<Value> {
    let field = |name: &str| item.get(name).cloned().unwrap_or(Value::Null);
    let id = field("id");
    Some(match item.get("type").and_then(Value::as_str)? {
        "UserMessage" => json!({"type": "userMessage", "id": id, "content": field("content")}),
        "AgentMessage" | "Plan" => {
            let text = match item.get("content") {
                Some(Value::Array(parts)) => {
                    parts.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("")
                }
                _ => str_field(item, "text"),
            };
            json!({"type": "agentMessage", "id": id, "text": text})
        }
        "Reasoning" => json!({"type": "reasoning", "id": id, "summary": field("summary_text"), "content": field("raw_content")}),
        "CommandExecution" => json!({
            "type": "commandExecution",
            "id": id,
            "command": field("command"),
            "status": field("status"),
            "aggregatedOutput": cut_text(item.get("aggregated_output")),
            "exitCode": field("exit_code"),
        }),
        "FileChange" => json!({"type": "fileChange", "id": id, "changes": changes(item.get("changes")?), "status": field("status")}),
        "McpToolCall" => {
            let result = field("result");
            let error = match (result.get("isError").and_then(Value::as_bool), item.get("error")) {
                (_, Some(Value::Object(error))) => json!(error),
                (Some(true), _) => json!({"message": "The tool failed."}),
                _ => Value::Null,
            };
            json!({
                "type": "mcpToolCall",
                "id": id,
                "server": field("server"),
                "tool": field("tool"),
                "arguments": field("arguments"),
                "status": field("status"),
                "result": result,
                "error": error,
            })
        }
        "Extension" if str_field(item, "kind") == "web.search" => json!({"type": "webSearch", "id": id, "query": field("query")}),
        "ImageView" => json!({"type": "imageView", "id": id, "path": field("path")}),
        "CollabAgentToolCall" => json!({"type": "collabAgentToolCall", "id": id, "tool": field("tool"), "prompt": field("prompt")}),
        "ContextCompaction" => json!({"type": "contextCompaction", "id": id}),
        _ => return None,
    })
}

/// A recorded patch's changes, a map by path, as the server's list
fn changes(map: &Value) -> Value {
    let changes: Vec<Value> = map
        .as_object()
        .into_iter()
        .flatten()
        .map(|(path, change)| {
            let kind = str_field(change, "type");
            let diff = change.get("unified_diff").or_else(|| change.get("content"));
            json!({
                "path": path,
                "kind": {"type": kind, "move_path": change.get("move_path")},
                "diff": cut_text(diff),
            })
        })
        .collect();
    Value::Array(changes)
}

fn cut_text(text: Option<&Value>) -> String {
    super::model::cut(text.and_then(Value::as_str).unwrap_or_default(), super::model::TOOL_TEXT_LIMIT)
}

/// `fields` added to the object `value`
fn extend(value: &mut Value, fields: Value) {
    if let (Some(map), Value::Object(fields)) = (value.as_object_mut(), fields) {
        map.extend(fields);
    }
}

fn str_field(value: &Value, name: &str) -> String {
    value.get(name).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// The `sh` script that starts the app-server in the launch's directory.
/// Of the profile's arguments only configuration passes to it: the rest
/// are the interactive CLI's
pub fn script(launch: &Launch) -> String {
    let quote = crate::agent::sh_quote;
    let cwd = if launch.cwd.is_empty() { "$HOME".to_string() } else { quote(&launch.cwd) };
    let mut command = format!("codex app-server -c {}", quote("forced_login_method=\"chatgpt\""));
    let args = shlex::split(&launch.args).unwrap_or_default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let passed = match arg.as_str() {
            "-c" | "--config" | "--enable" | "--disable" => args.next().map(|value| vec![arg.clone(), value.clone()]),
            _ if ["--config=", "--enable=", "--disable="].iter().any(|flag| arg.starts_with(flag)) => Some(vec![arg.clone()]),
            _ if arg.starts_with("-c") && arg.len() > 2 => Some(vec![arg.clone()]),
            _ => None,
        };
        for word in passed.into_iter().flatten() {
            command.push(' ');
            command.push_str(&quote(&word));
        }
    }
    // The subscription, never the API: a key in the environment would be used
    // over the login
    format!(
        "cd {cwd} 2>/dev/null || cd\n\
         unset OPENAI_API_KEY CODEX_API_KEY OPENAI_BASE_URL\n\
         exec {command}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::model::{Activity, Entries, ItemKind, ToolStatus};

    const THREAD: &str = "01a1239d-6547-76f3-b03e-e6175b833eaf";

    fn launch() -> Launch {
        Launch {
            agent: crate::config::Agent::Codex,
            cwd: "/tmp/work".into(),
            permission_mode: Some("default".into()),
            ..Launch::default()
        }
    }

    fn json_lines(lines: &[String]) -> Vec<Value> {
        lines.iter().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    fn user(text: &str) -> Command {
        Command::User { text: text.to_string(), images: Vec::new() }
    }

    /// Feed `lines` to `codex`, applying the events to `entries`; the
    /// replies it sent
    fn feed(codex: &mut Codex, entries: &mut Entries, lines: &str) -> Vec<Value> {
        let mut replies = Vec::new();
        for line in lines.lines().filter(|line| !line.trim().is_empty()) {
            let mut events = Vec::new();
            replies.extend(codex.parse(line, &mut events));
            events.into_iter().for_each(|event| entries.apply(event));
        }
        json_lines(&replies)
    }

    /// A Codex whose thread has started
    fn started(launch: &Launch) -> (Codex, Entries) {
        let mut codex = Codex::new(launch);
        let mut entries = Entries::default();
        codex.start();
        feed(&mut codex, &mut entries, r#"{"id":1,"result":{"userAgent":"x","codexHome":"/home/u/.codex"}}"#);
        let line = format!(
            r#"{{"id":2,"result":{{"thread":{{"id":"{THREAD}","turns":[]}},"model":"gpt-5.6-luna","cwd":"/tmp/work","reasoningEffort":"high"}}}}"#
        );
        feed(&mut codex, &mut entries, &line);
        (codex, entries)
    }

    #[test]
    fn handshakes_then_starts_or_resumes_a_thread() {
        let mut codex = Codex::new(&launch());
        let start = json_lines(&codex.start());
        assert_eq!(start[0]["method"], "initialize");
        assert_eq!(start[0]["params"]["capabilities"]["experimentalApi"], true);
        assert!(start[0].get("jsonrpc").is_none());
        // A turn asked for before the thread exists waits for it
        assert!(codex.command(user("hi")).is_empty());

        let mut entries = Entries::default();
        let replies = feed(&mut codex, &mut entries, r#"{"id":1,"result":{"userAgent":"x"}}"#);
        assert_eq!(replies[0], json!({"method": "initialized"}));
        let thread = &replies[1];
        assert_eq!(thread["method"], "thread/start");
        assert_eq!(thread["params"]["cwd"], "/tmp/work");
        assert_eq!(thread["params"]["approvalPolicy"], "untrusted");
        assert_eq!(thread["params"]["approvalsReviewer"], "user");
        assert_eq!(thread["params"]["sandbox"], "workspace-write");
        assert!(thread["params"]["developerInstructions"].as_str().unwrap().contains("rich-text thread view"));
        assert_eq!(replies[2]["method"], "model/list");

        let line = format!(r#"{{"id":2,"result":{{"thread":{{"id":"{THREAD}"}},"model":"gpt-5.6-luna","cwd":"/tmp/work"}}}}"#);
        let replies = feed(&mut codex, &mut entries, &line);
        assert_eq!(entries.session_id.as_deref(), Some(THREAD));
        assert_eq!(entries.model, "gpt-5.6-luna");
        assert_eq!(entries.permission_mode, "default");
        // Then the turn that waited
        assert_eq!(replies[0]["method"], "turn/start");
        assert_eq!(replies[0]["params"]["threadId"], THREAD);
        assert_eq!(replies[0]["params"]["input"][0], json!({"type": "text", "text": "hi", "text_elements": []}));

        feed(
            &mut codex,
            &mut entries,
            r#"{"id":3,"result":{"data":[{"id":"gpt-6.1-sol","model":"gpt-6.1-sol","displayName":"GPT-6.1-Sol","hidden":false},{"id":"old","model":"old","displayName":"Old","hidden":true}]}}"#,
        );
        assert_eq!(entries.models, vec![("gpt-6.1-sol".to_string(), "GPT-6.1-Sol".to_string())]);

        // Resuming names the thread
        let mut codex = Codex::new(&Launch { resume: Some(THREAD.into()), ..launch() });
        codex.start();
        let replies = feed(&mut codex, &mut entries, r#"{"id":1,"result":{}}"#);
        assert_eq!(replies[1]["method"], "thread/resume");
        assert_eq!(replies[1]["params"]["threadId"], THREAD);
        assert_eq!(replies[1]["params"]["excludeTurns"], true);
        // A thread that can't be resumed is begun again
        let replies = feed(&mut codex, &mut entries, r#"{"id":2,"error":{"code":-32600,"message":"no rollout found"}}"#);
        assert_eq!(replies[0]["method"], "thread/start");
        assert!(matches!(&entries.items.last().unwrap().kind, ItemKind::Notice { error: true, .. }));
    }

    #[test]
    fn sends_images_as_data_urls() {
        let (mut codex, _) = started(&launch());
        let lines = codex.command(Command::User { text: "look".into(), images: vec![("image/png".into(), "AAAA".into())] });
        let turn = &json_lines(&lines)[0];
        assert_eq!(turn["params"]["input"][0], json!({"type": "image", "url": "data:image/png;base64,AAAA"}));
        assert_eq!(turn["params"]["input"][1]["text"], "look");
        assert_eq!(turn["params"]["summary"], "auto");
    }

    /// A turn as `codex app-server` 0.162 streams it, up to a permission
    /// prompt
    const TURN: &str = r#"
{"id":4,"result":{"turn":{"id":"turn-1","items":[],"status":"inProgress"}}}
{"method":"turn/started","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turn":{"id":"turn-1","items":[],"status":"inProgress"}}}
{"method":"item/started","params":{"item":{"type":"userMessage","id":"u1","content":[{"type":"text","text":"Make hello.txt","text_elements":[]}]},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/completed","params":{"item":{"type":"userMessage","id":"u1","content":[{"type":"text","text":"Make hello.txt","text_elements":[]}]},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/started","params":{"item":{"type":"reasoning","id":"rs_1","summary":[],"content":[]},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/reasoning/summaryPartAdded","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1","itemId":"rs_1","summaryIndex":0}}
{"method":"item/reasoning/summaryTextDelta","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1","itemId":"rs_1","delta":"**Planning concise step**","summaryIndex":0}}
{"method":"item/completed","params":{"item":{"type":"reasoning","id":"rs_1","summary":["**Planning concise step**"],"content":[]},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/started","params":{"item":{"type":"reasoning","id":"rs_2","summary":[],"content":[]},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/completed","params":{"item":{"type":"reasoning","id":"rs_2","summary":[],"content":[]},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/started","params":{"item":{"type":"agentMessage","id":"msg_1","text":"","phase":"commentary"},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/agentMessage/delta","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1","itemId":"msg_1","delta":"I’ll create `hello.txt` with $x"}}
{"method":"item/agentMessage/delta","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1","itemId":"msg_1","delta":"^2$."}}
{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"msg_1","text":"I’ll create `hello.txt` with $x^2$.","phase":"commentary"},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"turn/plan/updated","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1","explanation":null,"plan":[{"step":"Write the file","status":"inProgress"},{"step":"Check it","status":"pending"}]}}
{"method":"item/started","params":{"item":{"type":"fileChange","id":"exec-1","changes":[{"path":"/tmp/work/hello.txt","kind":{"type":"add"},"diff":"hi\n"}],"status":"inProgress"},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"thread/status/changed","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","status":{"type":"active","activeFlags":["waitingOnApproval"]}}}
{"method":"item/fileChange/requestApproval","id":0,"params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1","itemId":"exec-1","startedAtMs":1791598884702,"reason":null,"grantRoot":null}}
"#;

    /// The rest of it, once the prompt is answered
    const TURN_END: &str = r#"
{"method":"serverRequest/resolved","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","requestId":0}}
{"method":"item/completed","params":{"item":{"type":"fileChange","id":"exec-1","changes":[{"path":"/tmp/work/hello.txt","kind":{"type":"add"},"diff":"hi\n"}],"status":"completed"},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/started","params":{"item":{"type":"commandExecution","id":"call_2","command":"/usr/bin/bash -lc 'cat hello.txt'","cwd":"/tmp/work","processId":null,"source":"agent","status":"inProgress","commandActions":[],"aggregatedOutput":null,"exitCode":null,"durationMs":null},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"item/commandExecution/outputDelta","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1","itemId":"call_2","delta":"h"}}
{"method":"item/started","params":{"item":{"type":"agentMessage","id":"sub","text":""},"threadId":"another-thread","turnId":"t9"}}
{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"call_2","command":"/usr/bin/bash -lc 'cat hello.txt'","cwd":"/tmp/work","processId":null,"source":"agent","status":"completed","commandActions":[],"aggregatedOutput":"hi\n","exitCode":0,"durationMs":3},"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1"}}
{"method":"thread/tokenUsage/updated","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turnId":"turn-1","tokenUsage":{"total":{"totalTokens":64023},"last":{"totalTokens":23407,"inputTokens":23402,"outputTokens":5},"modelContextWindow":258400}}}
{"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"codex"}}}
{"method":"turn/completed","params":{"threadId":"01a1239d-6547-76f3-b03e-e6175b833eaf","turn":{"id":"turn-1","items":[],"status":"completed","error":null,"durationMs":2237}}}
"#;

    #[test]
    fn a_streamed_turn_builds_text_thinking_tools_and_a_permission_prompt() {
        let (mut codex, mut entries) = started(&launch());
        codex.command(user("Make hello.txt"));
        entries.user("Make hello.txt".into(), Vec::new());
        let replies = feed(&mut codex, &mut entries, TURN);
        assert!(replies.is_empty(), "{replies:?}");

        // The prompt for the file change, on its card
        let request = entries.permissions[0].clone();
        assert_eq!(request.tool_name, "Write");
        assert_eq!(request.tool_use_id, "exec-1");
        assert!(request.always_allowed && !request.suggestions.is_null());
        let ItemKind::Tool(tool) = &entries.items.last().unwrap().kind else { panic!("{:#?}", entries.items) };
        assert_eq!(tool.status, ToolStatus::Asking);
        assert_eq!(tool.title(), "Write /tmp/work/hello.txt");
        assert_eq!(tool.patch().as_deref(), Some("+hi"));
        // Answered with the server's own id, once
        entries.answered(&request.request_id, true);
        let reply = codex.command(Command::Answer { request: request.clone(), decision: Decision::AllowAlways });
        assert_eq!(json_lines(&reply)[0], json!({"id": 0, "result": {"decision": "acceptForSession"}}));
        assert!(codex.command(Command::Answer { request, decision: Decision::Deny }).is_empty());

        feed(&mut codex, &mut entries, TURN_END);
        let kinds: Vec<&ItemKind> = entries.items.iter().map(|item| &item.kind).collect();
        assert_eq!(kinds.len(), 6, "{kinds:#?}");
        assert!(matches!(kinds[0], ItemKind::User { text, .. } if text == "Make hello.txt"));
        // Reasoning with nothing to show leaves no item
        assert!(matches!(kinds[1], ItemKind::Thinking { text, done: true, .. } if text == "**Planning concise step**"));
        let ItemKind::Text { text, doc } = kinds[2] else { panic!() };
        assert_eq!(text, "I’ll create `hello.txt` with $x^2$.");
        assert!(doc.formulas().next().is_some());
        let ItemKind::Tool(todo) = kinds[3] else { panic!() };
        assert_eq!(todo.name, "TodoWrite");
        assert_eq!(todo.todos()[0], ("Write the file".to_string(), "in_progress".to_string()));
        let ItemKind::Tool(write) = kinds[4] else { panic!() };
        assert_eq!(write.status, ToolStatus::Done);
        let ItemKind::Tool(bash) = kinds[5] else { panic!() };
        assert_eq!(bash.title(), "Bash cat hello.txt");
        assert_eq!(bash.output, "hi\n");
        assert_eq!(bash.status, ToolStatus::Done);
        assert!(entries.permissions.is_empty());
        assert_eq!(entries.activity, Activity::Idle);
        assert_eq!(entries.context_tokens, Some(23407));
    }

    #[test]
    fn commands_stream_output_and_report_failure() {
        let (mut codex, mut entries) = started(&launch());
        let item = |status: &str, output: &str, code: &str| {
            format!(
                r#"{{"method":"item/{status}","params":{{"item":{{"type":"commandExecution","id":"c1","command":"false","status":"x","aggregatedOutput":{output},"exitCode":{code}}},"threadId":"{THREAD}","turnId":"t"}}}}"#
            )
        };
        feed(&mut codex, &mut entries, &item("started", "null", "null"));
        let delta = format!(
            r#"{{"method":"item/commandExecution/outputDelta","params":{{"threadId":"{THREAD}","turnId":"t","itemId":"c1","delta":"partial"}}}}"#
        );
        feed(&mut codex, &mut entries, &delta);
        let ItemKind::Tool(tool) = &entries.items[0].kind else { panic!() };
        assert_eq!((tool.output.as_str(), tool.status), ("partial", ToolStatus::Running));
        feed(&mut codex, &mut entries, &item("completed", "\"\"", "1"));
        let ItemKind::Tool(tool) = &entries.items[0].kind else { panic!() };
        assert_eq!((tool.output.as_str(), tool.status), ("Exit status 1", ToolStatus::Failed));
    }

    #[test]
    fn interrupts_the_turn_once_its_id_is_known() {
        let (mut codex, mut entries) = started(&launch());
        codex.command(user("go"));
        // Stopped before the server named the turn
        assert!(codex.command(Command::Interrupt).is_empty());
        let replies = feed(&mut codex, &mut entries, r#"{"id":4,"result":{"turn":{"id":"turn-9"}}}"#);
        assert_eq!(replies[0]["method"], "turn/interrupt");
        assert_eq!(replies[0]["params"], json!({"threadId": THREAD, "turnId": "turn-9"}));
        let line = format!(r#"{{"method":"turn/started","params":{{"threadId":"{THREAD}","turn":{{"id":"turn-9"}}}}}}"#);
        assert!(feed(&mut codex, &mut entries, &line).is_empty(), "sent once");
        // Once known, at once
        let stop = json_lines(&codex.command(Command::Interrupt));
        assert_eq!(stop[0]["params"]["turnId"], "turn-9");
        let line = format!(
            r#"{{"method":"turn/completed","params":{{"threadId":"{THREAD}","turn":{{"id":"turn-9","status":"interrupted","error":null}}}}}}"#
        );
        feed(&mut codex, &mut entries, &line);
        assert_eq!(entries.activity, Activity::Idle);
        assert!(entries.items.is_empty());
        // Idle, a stop is nothing to the next turn
        assert!(codex.command(Command::Interrupt).is_empty());
        assert!(!codex.interrupt);
    }

    #[test]
    fn a_failed_turn_says_why() {
        let (mut codex, mut entries) = started(&launch());
        let lines = format!(
            r#"{{"method":"turn/started","params":{{"threadId":"{THREAD}","turn":{{"id":"t1"}}}}}}
{{"method":"error","params":{{"threadId":"{THREAD}","turnId":"t1","error":{{"message":"Reconnecting 1/5"}},"willRetry":true}}}}
{{"method":"error","params":{{"threadId":"{THREAD}","turnId":"t1","error":{{"message":"usage limit reached"}},"willRetry":false}}}}
{{"method":"turn/completed","params":{{"threadId":"{THREAD}","turn":{{"id":"t1","status":"failed","error":null}}}}}}"#
        );
        feed(&mut codex, &mut entries, &lines);
        assert_eq!(entries.items.len(), 1, "{:#?}", entries.items);
        assert!(matches!(&entries.items[0].kind, ItemKind::Notice { text, error: true } if text == "usage limit reached"));
    }

    #[test]
    fn a_turn_sent_before_its_thread_starts_is_told_as_a_turn() {
        let protocol = super::super::protocol::Protocol::new(&launch());
        let mut codex = Codex::new(&launch());
        codex.start();
        let sent = codex.parse(r#"{"id":1,"result":{"userAgent":"x","codexHome":"/home/u/.codex"}}"#, &mut Vec::new());
        assert!(sent.iter().any(|line| line.contains("thread/start")));
        assert!(codex.command(user("hi")).is_empty());
        let replies = codex.parse(
            &format!(r#"{{"id":2,"result":{{"thread":{{"id":"{THREAD}","cwd":"/tmp/work"}},"model":"m"}}}}"#),
            &mut Vec::new(),
        );
        let turn = replies.iter().find(|line| line.contains("turn/start")).expect("the waiting turn");
        assert!(protocol.starts_turn(turn));
        assert!(replies.iter().filter(|line| !line.contains("turn/start")).all(|line| !protocol.starts_turn(line)));
    }

    #[test]
    fn questions_are_asked_and_answered_by_key() {
        let (mut codex, mut entries) = started(&launch());
        let line = format!(
            r#"{{"method":"item/tool/requestUserInput","id":9,"params":{{"threadId":"{THREAD}","turnId":"t1","itemId":"q1","isBlocking":true,"questions":[{{"id":"db","header":"Store","question":"Which database?","isOther":true,"isSecret":false,"options":[{{"label":"SQLite","description":"one file"}},{{"label":"Postgres","description":""}}]}}]}}}}"#
        );
        assert!(feed(&mut codex, &mut entries, &line).is_empty());
        let request = entries.permissions[0].clone();
        assert_eq!(request.questions.len(), 1);
        assert_eq!(request.questions[0].key, "db");
        assert_eq!(request.questions[0].options[0], ("SQLite".to_string(), "one file".to_string()));
        assert!(request.questions[0].free);
        let reply = json_lines(&codex.command(Command::Answers {
            request,
            answers: vec![("db".to_string(), "Postgres".to_string())],
        }));
        assert_eq!(reply[0]["id"], 9);
        assert_eq!(reply[0]["result"]["answers"]["db"]["answers"][0], "Postgres");
    }

    #[test]
    fn requests_it_cannot_answer_get_an_error() {
        let (mut codex, mut entries) = started(&launch());
        let line = format!(r#"{{"method":"item/tool/requestUserInput","id":7,"params":{{"threadId":"{THREAD}","questions":[]}}}}"#);
        let replies = feed(&mut codex, &mut entries, &line);
        assert_eq!(replies[0]["id"], 7);
        assert!(replies[0]["error"]["message"].is_string());
        // Another thread's approval is not this one's to give
        let line = r#"{"method":"item/commandExecution/requestApproval","id":"r8","params":{"threadId":"sub","itemId":"c"}}"#;
        let replies = feed(&mut codex, &mut entries, line);
        assert_eq!(replies[0]["id"], "r8");
        assert!(entries.permissions.is_empty());
        // A command asked about before it started gets its card
        let line = format!(
            r#"{{"method":"item/commandExecution/requestApproval","id":"r9","params":{{"threadId":"{THREAD}","itemId":"c2","command":"rm -rf build","reason":"needs write access"}}}}"#
        );
        assert!(feed(&mut codex, &mut entries, &line).is_empty());
        assert_eq!(entries.permissions[0].title, "needs write access");
        let ItemKind::Tool(tool) = &entries.items[0].kind else { panic!() };
        assert_eq!((tool.title().as_str(), tool.status), ("Bash rm -rf build", ToolStatus::Asking));
        let reply = codex.command(Command::Answer { request: entries.permissions[0].clone(), decision: Decision::Deny });
        assert_eq!(json_lines(&reply)[0], json!({"id": "r9", "result": {"decision": "decline"}}));
    }

    #[test]
    fn modes_map_to_policies_and_settings() {
        assert_eq!(policy("default"), ("untrusted", "user", "workspace-write", ":workspace"));
        assert_eq!(policy("acceptEdits").0, "on-request");
        assert_eq!(policy("auto"), ("on-request", "auto_review", "workspace-write", ":workspace"));
        assert_eq!(policy("plan").2, "read-only");
        assert_eq!(policy("bypassPermissions"), ("never", "user", "danger-full-access", ":danger-full-access"));

        let (mut codex, _) = started(&launch());
        let plan = &json_lines(&codex.command(Command::SetMode("plan".into())))[0];
        assert_eq!(plan["method"], "thread/settings/update");
        assert_eq!(plan["params"]["permissions"], ":read-only");
        assert_eq!(plan["params"]["collaborationMode"]["mode"], "plan");
        assert_eq!(plan["params"]["collaborationMode"]["settings"]["model"], "gpt-5.6-luna");
        assert_eq!(plan["params"]["collaborationMode"]["settings"]["reasoning_effort"], "high");
        let auto = &json_lines(&codex.command(Command::SetMode("auto".into())))[0];
        assert_eq!(auto["params"]["approvalsReviewer"], "auto_review");
        assert_eq!(auto["params"]["collaborationMode"]["mode"], "default");
        let edits = &json_lines(&codex.command(Command::SetMode("acceptEdits".into())))[0];
        assert!(edits["params"].get("collaborationMode").is_none(), "plan mode was already off");
        let model = &json_lines(&codex.command(Command::SetModel("gpt-6-sol".into())))[0];
        assert_eq!(model["params"], json!({"threadId": THREAD, "model": "gpt-6-sol"}));

        // Refused, the settings go with the next turn
        let mut entries = Entries::default();
        let refused = format!(r#"{{"id":{},"error":{{"message":"unknown method"}}}}"#, codex.next_id);
        feed(&mut codex, &mut entries, &refused);
        assert!(entries.items.is_empty());
        let turn = &json_lines(&codex.command(user("x")))[0];
        assert_eq!(turn["params"]["approvalPolicy"], "on-request");
        assert_eq!(turn["params"]["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(turn["params"]["model"], "gpt-6-sol");

        // Started in plan mode, it is set on the new thread
        let mut codex = Codex::new(&Launch { permission_mode: Some("plan".into()), ..launch() });
        codex.start();
        feed(&mut codex, &mut entries, r#"{"id":1,"result":{}}"#);
        let line = format!(r#"{{"id":2,"result":{{"thread":{{"id":"{THREAD}"}},"model":"m"}}}}"#);
        let replies = feed(&mut codex, &mut entries, &line);
        assert_eq!(replies[0]["params"]["collaborationMode"]["mode"], "plan");

        // The profile's bypass flag
        let codex = Codex::new(&Launch { args: "--dangerously-bypass-approvals-and-sandbox".into(), ..launch() });
        assert_eq!(codex.thread_params()["sandbox"], "danger-full-access");
        assert_eq!(codex.thread_params()["approvalPolicy"], "never");
    }

    #[test]
    fn script_passes_only_configuration_through() {
        let launch = Launch {
            cwd: "/home/u/my project".into(),
            args: "--dangerously-bypass-approvals-and-sandbox -c 'model_reasoning_effort=\"low\"' --enable web_search \
                   --search -m gpt-6 --config=a.b=1"
                .into(),
            ..launch()
        };
        let script = launch.script();
        assert!(script.contains("unset OPENAI_API_KEY CODEX_API_KEY OPENAI_BASE_URL"));
        assert!(script.contains("cd '/home/u/my project'"));
        let words = shlex::split(script.lines().last().unwrap().trim_start_matches("exec ")).unwrap();
        assert_eq!(
            words,
            [
                "codex",
                "app-server",
                "-c",
                "forced_login_method=\"chatgpt\"",
                "-c",
                "model_reasoning_effort=\"low\"",
                "--enable",
                "web_search",
                "--config=a.b=1"
            ]
        );
        assert_eq!(Codex::new(&launch).model.as_deref(), Some("gpt-6"));
    }

    #[test]
    fn unwraps_shell_commands() {
        assert_eq!(shell_script(&json!("/usr/bin/bash -lc 'ls -la | head'")), "ls -la | head");
        assert_eq!(shell_script(&json!(["/usr/bin/bash", "-lc", "echo hi"])), "echo hi");
        assert_eq!(shell_script(&json!(["git", "status", "a b"])), "git status 'a b'");
        assert_eq!(shell_script(&json!("cargo test")), "cargo test");
    }

    #[test]
    fn patches_render_as_one_diff() {
        let item = json!({"type": "fileChange", "id": "p", "status": "completed", "changes": [
            {"path": "/w/a.rs", "kind": {"type": "update", "move_path": null}, "diff": "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n"},
            {"path": "/w/b.rs", "kind": {"type": "delete"}, "diff": "gone\n"},
        ]});
        let mut entries = Entries::default();
        let mut events = Vec::new();
        completed(&item, true, &mut events);
        events.into_iter().for_each(|event| entries.apply(event));
        let ItemKind::Tool(tool) = &entries.items[0].kind else { panic!() };
        assert_eq!(tool.title(), "Edit /w/a.rs, /w/b.rs");
        assert_eq!(tool.patch().unwrap(), "@@ /w/a.rs\n@@ -1 +1 @@\n-old\n+new\n@@ /w/b.rs\n-gone");
    }
}

/// The questions of a `requestUserInput`; a secret's answer is typed as any
/// other's
fn questions(params: &Value) -> Vec<super::model::Question> {
    params
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|q| super::model::Question {
            key: str_field(q, "id"),
            header: str_field(q, "header"),
            question: str_field(q, "question"),
            options: q
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|o| (str_field(o, "label"), str_field(o, "description")))
                .collect(),
            free: q.get("isOther").and_then(Value::as_bool).unwrap_or(false)
                || q.get("options").and_then(Value::as_array).is_none_or(|o| o.is_empty()),
        })
        .collect()
}

/// Something different for each client, from the clock
fn rand_offset() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos())
}
