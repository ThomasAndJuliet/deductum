// SPDX-License-Identifier: GPL-3.0-only

//! A thread: the entries of a conversation as the view shows them.
//!
//! As in Zed's agent panel, a thread is a flat list of entries — user
//! messages, assistant text, thinking, tool calls and notices — and a tool
//! call is patched in place as its input streams in, its permission is asked
//! and its result arrives. The same [`Event`]s build a thread from the CLI's
//! live output and from a saved transcript.
//!
//! Only what the view needs is kept. A tool's input and output are cut to
//! [`TOOL_TEXT_LIMIT`], keeping both ends, and images live in files (see
//! [`super::images`]), so a long thread costs little more than its prose.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use super::markdown::{self, Doc};

/// The most of a tool's input or output that is kept, in bytes
pub const TOOL_TEXT_LIMIT: usize = 12 * 1024;
/// The most lines of a subagent's activity kept under its tool call
const CHILD_LINES: usize = 40;

/// What the CLI said, already parsed off the UI thread
#[derive(Clone, Debug)]
pub enum Event {
    Started { session_id: String, model: String, cwd: String, permission_mode: String },
    Models(Vec<(String, String)>),
    Status(String),
    MessageStart { message_id: String, parent: Option<String> },
    BlockStart { index: usize, block: Value, parent: Option<String> },
    TextDelta { index: usize, text: String, parent: Option<String> },
    ThinkingDelta { index: usize, text: String, tokens: Option<u64>, parent: Option<String> },
    InputDelta { index: usize, json: String, parent: Option<String> },
    BlockStop { index: usize, parent: Option<String> },
    /// A complete assistant message, or one block of one
    Assistant { message_id: String, content: Value, parent: Option<String> },
    /// A user message: text from the user, or tool results
    User { content: Value, parent: Option<String> },
    /// More of a running tool's output
    ToolOutput { id: String, text: String },
    Permission(PermissionRequest),
    PermissionCancelled(String),
    TurnDone { error: bool, message: String, duration_ms: u64, context_tokens: Option<u64> },
    Notice(String),
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub title: String,
    pub input: Value,
    /// The rules the CLI suggests for "always allow"
    pub suggestions: Value,
    /// Whether "always allow" may be offered
    pub always_allowed: bool,
    /// What the agent asks the user, when the prompt is its questions
    /// rather than a tool to allow
    pub questions: Vec<Question>,
}

/// A question the agent asks, with the answers it offers
#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    /// What its answer is filed under: Codex's id, Claude's question
    pub key: String,
    pub header: String,
    pub question: String,
    /// Each answer offered, and what it means
    pub options: Vec<(String, String)>,
    /// Whether an answer of the user's own is taken
    pub free: bool,
}

/// An image, kept in a file
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageRef {
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    /// Its input is still streaming in
    Pending,
    /// Waiting for the user's permission
    Asking,
    Running,
    Done,
    Failed,
    Denied,
}

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The input as streamed, until it is complete
    pub partial_input: String,
    /// The input, with long strings cut
    pub input: Value,
    pub status: ToolStatus,
    /// The result, cut to [`TOOL_TEXT_LIMIT`]
    pub output: String,
    pub images: Vec<ImageRef>,
    /// A subagent's activity, one line per step
    pub children: Vec<String>,
    pub child_count: usize,
}

impl ToolCall {
    /// A one-line title, as Zed and Claude Code show: the tool and what it
    /// acts on
    pub fn title(&self) -> String {
        let field = |name: &str| self.input.get(name).and_then(Value::as_str).unwrap_or_default();
        let detail = match self.name.as_str() {
            "Bash" => field("command").lines().next().unwrap_or_default().to_string(),
            "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
                let path = field("file_path");
                if path.is_empty() { field("notebook_path").to_string() } else { path.to_string() }
            }
            "Glob" | "Grep" => field("pattern").to_string(),
            "WebFetch" => field("url").to_string(),
            "WebSearch" => field("query").to_string(),
            "Task" | "Agent" => field("description").to_string(),
            "TodoWrite" => String::new(),
            _ => self
                .input
                .as_object()
                .and_then(|map| map.values().find_map(Value::as_str))
                .map(|text| text.lines().next().unwrap_or_default().to_string())
                .unwrap_or_default(),
        };
        let name = self.name.strip_prefix("mcp__").map_or(self.name.clone(), |rest| rest.replace("__", ": "));
        if detail.is_empty() { name } else { format!("{name} {detail}") }
    }

    /// An edit's old and new text, for a diff
    pub fn edits(&self) -> Vec<(String, String)> {
        let pair = |value: &Value| {
            Some((
                value.get("old_string")?.as_str()?.to_string(),
                value.get("new_string")?.as_str()?.to_string(),
            ))
        };
        match self.name.as_str() {
            "Edit" => pair(&self.input).into_iter().collect(),
            "MultiEdit" => self
                .input
                .get("edits")
                .and_then(Value::as_array)
                .map(|edits| edits.iter().filter_map(pair).collect())
                .unwrap_or_default(),
            "Write" => self
                .input
                .get("content")
                .and_then(Value::as_str)
                .map(|content| vec![(String::new(), content.to_string())])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// A Codex patch's changes as one diff, each file under an `@@` line of
    /// its name when there are several
    pub fn patch(&self) -> Option<String> {
        let changes = self.input.get("changes")?.as_array()?;
        let mut out = String::new();
        for change in changes {
            let field = |name: &str| change.get(name).and_then(Value::as_str).unwrap_or_default();
            if changes.len() > 1 {
                out.push_str(&format!("@@ {}\n", field("path")));
            }
            let diff = field("diff");
            match change.pointer("/kind/type").and_then(Value::as_str) {
                Some("add") => diff.lines().for_each(|line| out.push_str(&format!("+{line}\n"))),
                Some("delete") => diff.lines().for_each(|line| out.push_str(&format!("-{line}\n"))),
                _ => {
                    // Hunks only: file headers would read as removed and added lines
                    let start = diff.find("@@").unwrap_or(0);
                    out.push_str(&diff[start..]);
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
            }
        }
        Some(out.trim_end().to_string())
    }

    /// The todo list a `TodoWrite` sets: each item's text and status
    pub fn todos(&self) -> Vec<(String, String)> {
        self.input
            .get("todos")
            .and_then(Value::as_array)
            .map(|todos| {
                todos
                    .iter()
                    .map(|todo| {
                        let text = todo.get("content").and_then(Value::as_str).unwrap_or_default();
                        let status = todo.get("status").and_then(Value::as_str).unwrap_or_default();
                        (text.to_string(), status.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    User { text: String, images: Vec<ImageRef> },
    /// Assistant prose, as markdown and as parsed
    Text { text: String, doc: Arc<Doc> },
    Thinking { text: String, tokens: u64, done: bool },
    Tool(Box<ToolCall>),
    Notice { text: String, error: bool },
}

#[derive(Clone, Debug)]
pub struct Item {
    /// Unique within the thread, and never reused, so view state can be
    /// keyed by it
    pub id: u64,
    pub kind: ItemKind,
}

impl Item {
    /// The text a copy of the whole item gives
    pub fn copy_text(&self) -> String {
        match &self.kind {
            ItemKind::User { text, .. } | ItemKind::Text { text, .. } => text.clone(),
            ItemKind::Thinking { text, .. } | ItemKind::Notice { text, .. } => text.clone(),
            ItemKind::Tool(tool) => {
                let mut out = tool.title();
                if !tool.output.is_empty() {
                    out.push_str("\n\n");
                    out.push_str(&tool.output);
                }
                out
            }
        }
    }

    /// Roughly how many bytes it holds, for memory accounting
    pub fn weight(&self) -> usize {
        match &self.kind {
            ItemKind::User { text, .. } => text.len(),
            ItemKind::Text { text, .. } => text.len() * 3,
            ItemKind::Thinking { text, .. } | ItemKind::Notice { text, .. } => text.len(),
            ItemKind::Tool(tool) => {
                tool.output.len() + tool.partial_input.len() + tool.children.iter().map(String::len).sum::<usize>() + 512
            }
        }
    }
}

/// Whether the thread's agent is busy
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Activity {
    #[default]
    Idle,
    Working,
}

/// The entries of a conversation, and what streaming needs to place what
/// comes next
#[derive(Debug, Default)]
pub struct Entries {
    pub items: Vec<Item>,
    next_id: u64,
    /// The current message's blocks by stream index, to their items
    streaming: HashMap<usize, usize>,
    current_message: String,
    /// How many blocks of each message complete `assistant` messages have
    /// delivered, as each such message carries one block
    delivered: HashMap<String, usize>,
    /// Items of each streamed message, in block order
    message_items: HashMap<String, Vec<usize>>,
    /// Tool calls by id, to their items
    tools: HashMap<String, usize>,
    pub activity: Activity,
    pub session_id: Option<String>,
    pub model: String,
    pub permission_mode: String,
    pub models: Vec<(String, String)>,
    pub context_tokens: Option<u64>,
    pub permissions: Vec<PermissionRequest>,
    /// Bumped by every change, so views can tell when to look again
    pub revision: u64,
}

impl Entries {
    pub fn push(&mut self, kind: ItemKind) -> usize {
        self.next_id += 1;
        self.items.push(Item { id: self.next_id, kind });
        self.items.len() - 1
    }

    pub fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.push(ItemKind::Notice { text: text.into(), error });
        self.revision += 1;
    }

    /// The user's own message, shown as soon as it is sent
    pub fn user(&mut self, text: String, images: Vec<ImageRef>) {
        self.push(ItemKind::User { text, images });
        self.activity = Activity::Working;
        self.revision += 1;
    }

    /// Bytes held, roughly
    pub fn weight(&self) -> usize {
        self.items.iter().map(Item::weight).sum()
    }

    pub fn apply(&mut self, event: Event) {
        self.revision += 1;
        match event {
            Event::Started { session_id, model, permission_mode, .. } => {
                self.session_id = Some(session_id);
                if !model.is_empty() {
                    self.model = model;
                }
                if !permission_mode.is_empty() {
                    self.permission_mode = permission_mode;
                }
            }
            Event::Models(models) => self.models = models,
            Event::Status(_) => {}
            Event::MessageStart { message_id, parent } => {
                if parent.is_none() {
                    self.current_message = message_id;
                    self.streaming.clear();
                    self.activity = Activity::Working;
                }
            }
            Event::BlockStart { index, block, parent } => {
                if let Some(parent) = parent {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let name = block.get("name").and_then(Value::as_str).unwrap_or_default();
                        self.child_line(&parent, name.to_string());
                    }
                    return;
                }
                let kind = match block.get("type").and_then(Value::as_str) {
                    Some("text") => ItemKind::Text {
                        text: String::new(),
                        doc: Arc::new(Doc::default()),
                    },
                    Some("thinking") | Some("redacted_thinking") => {
                        ItemKind::Thinking { text: String::new(), tokens: 0, done: false }
                    }
                    Some("tool_use") | Some("server_tool_use") => {
                        let id = block.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                        let at = self.push(ItemKind::Tool(Box::new(ToolCall {
                            id: id.clone(),
                            name: block.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                            partial_input: String::new(),
                            input: Value::Null,
                            status: ToolStatus::Pending,
                            output: String::new(),
                            images: Vec::new(),
                            children: Vec::new(),
                            child_count: 0,
                        })));
                        self.tools.insert(id, at);
                        self.streamed(index, at);
                        return;
                    }
                    _ => return,
                };
                let at = self.push(kind);
                self.streamed(index, at);
            }
            Event::TextDelta { index, text, parent } => {
                if parent.is_some() {
                    return;
                }
                if let Some(&at) = self.streaming.get(&index)
                    && let ItemKind::Text { text: current, doc } = &mut self.items[at].kind
                {
                    current.push_str(&text);
                    *doc = Arc::new(markdown::parse(current));
                }
            }
            Event::ThinkingDelta { index, text, tokens, parent } => {
                if parent.is_some() {
                    return;
                }
                if let Some(&at) = self.streaming.get(&index)
                    && let ItemKind::Thinking { text: current, tokens: total, .. } = &mut self.items[at].kind
                {
                    current.push_str(&text);
                    if let Some(tokens) = tokens {
                        *total = (*total).max(tokens);
                    }
                }
            }
            Event::InputDelta { index, json, parent } => {
                if parent.is_some() {
                    return;
                }
                if let Some(&at) = self.streaming.get(&index)
                    && let ItemKind::Tool(tool) = &mut self.items[at].kind
                    && tool.partial_input.len() < TOOL_TEXT_LIMIT * 4
                {
                    tool.partial_input.push_str(&json);
                }
            }
            Event::BlockStop { index, parent } => {
                if parent.is_some() {
                    return;
                }
                if let Some(&at) = self.streaming.get(&index) {
                    match &mut self.items[at].kind {
                        ItemKind::Thinking { done, .. } => *done = true,
                        ItemKind::Tool(tool) => {
                            if let Ok(input) = serde_json::from_str::<Value>(&tool.partial_input) {
                                tool.input = cut_value(input);
                                tool.partial_input = String::new();
                            }
                            if tool.status == ToolStatus::Pending {
                                tool.status = ToolStatus::Running;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Assistant { message_id, content, parent } => {
                if let Some(parent) = parent {
                    for block in content.as_array().into_iter().flatten() {
                        let line = match block.get("type").and_then(Value::as_str) {
                            Some("text") => first_line(block.get("text").and_then(Value::as_str).unwrap_or_default()),
                            Some("tool_use") => {
                                let tool = tool_from_block(block);
                                tool.title()
                            }
                            _ => continue,
                        };
                        if !line.is_empty() {
                            self.replace_child_line(&parent, line);
                        }
                    }
                    return;
                }
                for block in content.as_array().into_iter().flatten() {
                    self.assistant_block(&message_id, block);
                }
            }
            Event::User { content, parent } => {
                if let Some(text) = content.as_str() {
                    if parent.is_none() {
                        self.push(ItemKind::User { text: text.to_string(), images: Vec::new() });
                    }
                    return;
                }
                let mut text = String::new();
                let mut images = Vec::new();
                for block in content.as_array().into_iter().flatten() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_result") => {
                            let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or_default();
                            let error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                            if parent.is_some() {
                                continue;
                            }
                            self.tool_result(id, block.get("content"), error);
                        }
                        Some("text") if parent.is_none() => {
                            if !text.is_empty() {
                                text.push_str("\n\n");
                            }
                            text.push_str(block.get("text").and_then(Value::as_str).unwrap_or_default());
                        }
                        Some("image") if parent.is_none() => {
                            if let Some(image) = image_ref(block) {
                                images.push(image);
                            }
                        }
                        _ => {}
                    }
                }
                if !text.is_empty() || !images.is_empty() {
                    self.push(ItemKind::User { text, images });
                }
            }
            Event::ToolOutput { id, text } => {
                if let Some(&at) = self.tools.get(&id)
                    && let ItemKind::Tool(tool) = &mut self.items[at].kind
                {
                    tool.output.push_str(&text);
                    if tool.output.len() > TOOL_TEXT_LIMIT * 2 {
                        tool.output = cut(&tool.output, TOOL_TEXT_LIMIT);
                    }
                }
            }
            Event::Permission(request) => {
                if let Some(&at) = self.tools.get(&request.tool_use_id)
                    && let ItemKind::Tool(tool) = &mut self.items[at].kind
                {
                    tool.status = ToolStatus::Asking;
                    if tool.input.is_null() {
                        tool.input = cut_value(request.input.clone());
                    }
                }
                self.permissions.push(request);
            }
            Event::PermissionCancelled(id) => self.permissions.retain(|request| request.request_id != id),
            Event::TurnDone { error, message, context_tokens, .. } => {
                self.activity = Activity::Idle;
                self.streaming.clear();
                self.permissions.clear();
                if context_tokens.is_some() {
                    self.context_tokens = context_tokens;
                }
                // Tools that never reported are over too
                for item in &mut self.items {
                    if let ItemKind::Tool(tool) = &mut item.kind
                        && matches!(tool.status, ToolStatus::Pending | ToolStatus::Running | ToolStatus::Asking)
                    {
                        tool.status = ToolStatus::Done;
                    }
                    if let ItemKind::Thinking { done, .. } = &mut item.kind {
                        *done = true;
                    }
                }
                if error && !message.is_empty() {
                    self.push(ItemKind::Notice { text: message, error: true });
                }
            }
            Event::Notice(text) => {
                self.push(ItemKind::Notice { text, error: false });
            }
            Event::Error(text) => {
                self.push(ItemKind::Notice { text, error: true });
            }
        }
    }

    /// Answered, so the prompt goes and the tool runs or is refused
    pub fn answered(&mut self, request_id: &str, allowed: bool) {
        let Some(at) = self.permissions.iter().position(|r| r.request_id == request_id) else {
            return;
        };
        let request = self.permissions.remove(at);
        if let Some(&at) = self.tools.get(&request.tool_use_id)
            && let ItemKind::Tool(tool) = &mut self.items[at].kind
        {
            tool.status = if allowed { ToolStatus::Running } else { ToolStatus::Denied };
        }
        self.revision += 1;
    }

    /// The agent stopped: nothing more will come of the current turn
    pub fn stopped(&mut self) {
        if self.activity == Activity::Working {
            self.apply(Event::TurnDone { error: false, message: String::new(), duration_ms: 0, context_tokens: None });
        }
        self.permissions.clear();
    }

    fn streamed(&mut self, index: usize, at: usize) {
        self.streaming.insert(index, at);
        self.message_items.entry(self.current_message.clone()).or_default().push(at);
    }

    fn assistant_block(&mut self, message_id: &str, block: &Value) {
        let n = self.delivered.entry(message_id.to_string()).or_default();
        let nth = *n;
        *n += 1;
        let existing = self.message_items.get(message_id).and_then(|items| items.get(nth)).copied();
        let kind = block.get("type").and_then(Value::as_str).unwrap_or_default();
        match (kind, existing) {
            ("text", Some(at)) => {
                if let ItemKind::Text { text, doc } = &mut self.items[at].kind {
                    let complete = block.get("text").and_then(Value::as_str).unwrap_or_default();
                    if text != complete {
                        *text = complete.to_string();
                        *doc = Arc::new(markdown::parse(text));
                    }
                }
            }
            ("text", None) => {
                let text = block.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
                if !text.trim().is_empty() {
                    let doc = Arc::new(markdown::parse(&text));
                    self.push(ItemKind::Text { text, doc });
                }
            }
            ("thinking" | "redacted_thinking", Some(at)) => {
                if let ItemKind::Thinking { text, done, .. } = &mut self.items[at].kind {
                    let complete = block.get("thinking").and_then(Value::as_str).unwrap_or_default();
                    if !complete.is_empty() {
                        *text = complete.to_string();
                    }
                    *done = true;
                }
            }
            ("thinking" | "redacted_thinking", None) => {
                let text = block.get("thinking").and_then(Value::as_str).unwrap_or_default().to_string();
                self.push(ItemKind::Thinking { text, tokens: 0, done: true });
            }
            ("tool_use" | "server_tool_use", existing) => {
                let complete = tool_from_block(block);
                match existing.or_else(|| self.tools.get(&complete.id).copied()) {
                    Some(at) => {
                        if let ItemKind::Tool(tool) = &mut self.items[at].kind {
                            tool.input = complete.input;
                            tool.partial_input = String::new();
                            if tool.status == ToolStatus::Pending {
                                tool.status = ToolStatus::Running;
                            }
                        }
                    }
                    None => {
                        let id = complete.id.clone();
                        let at = self.push(ItemKind::Tool(Box::new(complete)));
                        self.tools.insert(id, at);
                    }
                }
            }
            _ => {}
        }
    }

    fn tool_result(&mut self, id: &str, content: Option<&Value>, error: bool) {
        let Some(&at) = self.tools.get(id) else { return };
        let ItemKind::Tool(tool) = &mut self.items[at].kind else { return };
        let mut output = String::new();
        match content {
            Some(Value::String(text)) => output.push_str(text),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if !output.is_empty() {
                                output.push('\n');
                            }
                            output.push_str(block.get("text").and_then(Value::as_str).unwrap_or_default());
                        }
                        Some("image") => tool.images.extend(image_ref(block)),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        tool.output = cut(&output, TOOL_TEXT_LIMIT);
        tool.status = if tool.status == ToolStatus::Denied {
            ToolStatus::Denied
        } else if error {
            ToolStatus::Failed
        } else {
            ToolStatus::Done
        };
        if tool.status == ToolStatus::Failed && output.contains("declined") {
            tool.status = ToolStatus::Denied;
        }
    }

    fn child_line(&mut self, parent: &str, line: String) {
        let Some(&at) = self.tools.get(parent) else { return };
        if let ItemKind::Tool(tool) = &mut self.items[at].kind {
            tool.child_count += 1;
            tool.children.push(line);
            if tool.children.len() > CHILD_LINES {
                tool.children.remove(0);
            }
        }
    }

    /// A step's full description replaces the bare tool name its start gave
    fn replace_child_line(&mut self, parent: &str, line: String) {
        let Some(&at) = self.tools.get(parent) else { return };
        if let ItemKind::Tool(tool) = &mut self.items[at].kind {
            match tool.children.last_mut() {
                Some(last) if line.starts_with(last.as_str()) => *last = line,
                _ => {
                    tool.child_count += 1;
                    tool.children.push(line);
                    if tool.children.len() > CHILD_LINES {
                        tool.children.remove(0);
                    }
                }
            }
        }
    }
}

fn tool_from_block(block: &Value) -> ToolCall {
    ToolCall {
        id: block.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
        name: block.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        partial_input: String::new(),
        input: cut_value(block.get("input").cloned().unwrap_or(Value::Null)),
        status: ToolStatus::Running,
        output: String::new(),
        images: Vec::new(),
        children: Vec::new(),
        child_count: 0,
    }
}

/// An image block whose data [`super::images::spill`] moved to a file
pub fn image_ref(block: &Value) -> Option<ImageRef> {
    let path = block.pointer("/source/path").and_then(Value::as_str)?;
    Some(ImageRef { path: PathBuf::from(path) })
}

fn first_line(text: &str) -> String {
    text.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim().to_string()
}

/// `text` cut to about `limit` bytes, keeping its beginning and end
pub fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let head_end = floor_char(text, limit * 2 / 3);
    let tail_start = ceil_char(text, text.len() - limit / 3);
    let omitted = text[head_end..tail_start].lines().count();
    format!("{}\n… {omitted} lines not kept …\n{}", &text[..head_end], &text[tail_start..])
}

fn floor_char(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn ceil_char(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// A tool's input with every long string cut
fn cut_value(value: Value) -> Value {
    match value {
        Value::String(text) if text.len() > TOOL_TEXT_LIMIT => Value::String(cut(&text, TOOL_TEXT_LIMIT)),
        Value::Array(values) => Value::Array(values.into_iter().map(cut_value).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, cut_value(v))).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::claude::parse_line;

    fn feed(entries: &mut Entries, lines: &str) {
        for line in lines.lines() {
            let mut events = Vec::new();
            parse_line(line, &mut events);
            for event in events {
                entries.apply(event);
            }
        }
    }

    #[test]
    fn streams_text_then_reconciles_with_the_complete_message() {
        let mut entries = Entries::default();
        feed(
            &mut entries,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m1"}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_stop","index":0},"parent_tool_use_id":null}
{"type":"assistant","message":{"id":"m1","content":[{"type":"thinking","thinking":""}]},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hello $x"}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"^2$"}},"parent_tool_use_id":null}
{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Hello $x^2$"}]},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"t1","name":"Bash","input":{}}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"command\": \"echo hi\"}"}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_stop","index":2},"parent_tool_use_id":null}
{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"echo hi"}}]},"parent_tool_use_id":null}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"t1","type":"tool_result","content":"hi","is_error":false}]},"parent_tool_use_id":null}
{"type":"result","is_error":false,"duration_ms":5,"usage":{"input_tokens":1}}"#,
        );
        assert_eq!(entries.items.len(), 3, "{:#?}", entries.items);
        let ItemKind::Text { text, .. } = &entries.items[1].kind else { panic!() };
        assert_eq!(text, "Hello $x^2$");
        let ItemKind::Tool(tool) = &entries.items[2].kind else { panic!() };
        assert_eq!(tool.title(), "Bash echo hi");
        assert_eq!(tool.output, "hi");
        assert_eq!(tool.status, ToolStatus::Done);
        assert_eq!(entries.activity, Activity::Idle);
    }

    #[test]
    fn a_transcript_without_streaming_builds_the_same_items() {
        let mut entries = Entries::default();
        feed(
            &mut entries,
            r#"{"type":"user","message":{"role":"user","content":"What is $\\pi$?"}}
{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"About 3.14"}]}}
{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/a"}}]}}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"t1","type":"tool_result","content":[{"type":"text","text":"body"}],"is_error":true}]}}"#,
        );
        assert_eq!(entries.items.len(), 3);
        assert!(matches!(&entries.items[0].kind, ItemKind::User { text, .. } if text == "What is $\\pi$?"));
        let ItemKind::Tool(tool) = &entries.items[2].kind else { panic!() };
        assert_eq!(tool.status, ToolStatus::Failed);
    }

    #[test]
    fn cuts_long_text_keeping_both_ends() {
        let text: String = (0..10_000).map(|i| format!("line {i}\n")).collect();
        let kept = cut(&text, 1000);
        assert!(kept.len() < 1200);
        assert!(kept.starts_with("line 0\n"));
        assert!(kept.trim_end().ends_with("line 9999"));
        assert!(kept.contains("lines not kept"));
    }

    #[test]
    fn subagent_steps_go_under_their_task() {
        let mut entries = Entries::default();
        feed(
            &mut entries,
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"task","name":"Task","input":{"description":"Look"}}]}}
{"type":"assistant","message":{"id":"s1","content":[{"type":"tool_use","id":"x","name":"Grep","input":{"pattern":"foo"}}]},"parent_tool_use_id":"task"}"#,
        );
        assert_eq!(entries.items.len(), 1);
        let ItemKind::Tool(tool) = &entries.items[0].kind else { panic!() };
        assert_eq!(tool.children, vec!["Grep foo".to_string()]);
    }
}
