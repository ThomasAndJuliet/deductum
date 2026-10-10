// SPDX-License-Identifier: GPL-3.0-only

//! Driving the `claude` CLI headless, as the Claude Agent SDK does.
//!
//! The CLI runs with `--input-format stream-json --output-format stream-json`
//! and talks one JSON object per line. Running the CLI, rather than calling
//! the API, keeps the user's own login, so turns count against their
//! subscription and never against API credits. `ANTHROPIC_API_KEY` is
//! removed from its environment for the same reason.
//!
//! Permission prompts arrive as `control_request`s with the subtype
//! `can_use_tool`, because the CLI is started with
//! `--permission-prompt-tool stdio`, and are answered with a
//! `control_response`.

use serde_json::{Value, json};

use super::model::{Event, PermissionRequest};

/// How a thread's CLI is started
#[derive(Clone, Debug, Default)]
pub struct Launch {
    /// Whose CLI: Claude's or Codex's
    pub agent: crate::config::Agent,
    /// ssh destination, or empty for this computer
    pub host: String,
    pub cwd: String,
    /// The conversation to continue, if any
    pub resume: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    /// Extra arguments, as a shell would split them
    pub args: String,
    /// The view's colours, for the images the agent draws to match
    pub colours: Option<Colours>,
    /// More for the system prompt, as where a moved conversation's files
    /// went
    pub note: Option<String>,
}

/// A view's colour scheme, as CSS colours
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Colours {
    pub dark: bool,
    pub background: String,
    pub text: String,
    pub accent: String,
}

impl Launch {
    /// The `sh` script that starts the CLI in `cwd`
    pub fn script(&self) -> String {
        if self.agent == crate::config::Agent::Codex {
            return super::codex::script(self);
        }
        let quote = crate::agent::sh_quote;
        let mut command = String::from(
            "claude --output-format stream-json --verbose --input-format stream-json \
             --include-partial-messages --permission-prompt-tool stdio",
        );
        if let Some(id) = &self.resume {
            command.push(' ');
            command.push_str(&quote(&format!("--resume={id}")));
        }
        if let Some(model) = self.model.as_deref().filter(|m| !m.is_empty()) {
            command.push_str(" --model ");
            command.push_str(&quote(model));
        }
        if let Some(mode) = self.permission_mode.as_deref().filter(|m| !m.is_empty()) {
            command.push_str(" --permission-mode ");
            command.push_str(&quote(mode));
        }
        // Before the profile's arguments, so that a prompt of its own wins
        command.push_str(" --append-system-prompt ");
        command.push_str(&quote(&self.system_prompt()));
        for arg in shlex::split(&self.args).unwrap_or_default() {
            command.push(' ');
            command.push_str(&quote(&arg));
        }
        let cwd = if self.cwd.is_empty() { "$HOME".to_string() } else { quote(&self.cwd) };
        // The subscription, never the API: the CLI uses an API key over its
        // login when one is set. CLAUDECODE would make it think it is nested
        format!(
            "cd {cwd} 2>/dev/null || cd\n\
             unset ANTHROPIC_API_KEY CLAUDECODE\n\
             exec {command}\n"
        )
    }

    /// What the CLI is told about the view its replies appear in, which it
    /// can't otherwise know: it would write for a terminal
    pub fn system_prompt(&self) -> String {
        let mut prompt = String::from(
            "The user reads this conversation in COSMIC Terminal's rich-text thread view, not in a \
             terminal. Your replies are rendered as Markdown, with LaTeX maths between $...$ or \
             \\(...\\) inline and $$...$$ or \\[...\\] on their own lines. To show a chart of data, \
             write a code block whose language is chartgen holding one chartgen specification (JSON): \
             the view draws it natively, in its colours. Read `~/.config/cosmic-term-agents/ct-chartgen \
             guide` before writing your first, and check each with `~/.config/cosmic-term-agents/ct-chartgen \
             check FILE` before showing it.",
        );
        // The view finds images on its own computer: a remote thread's
        // paths would name files it doesn't have
        if self.host.is_empty() {
            prompt.push_str(
                " To show the user an image, such as a plot or a diagram you made or found, put \
                 it in your reply as a Markdown image of the file on disk, with an absolute path: \
                 ![A plot of the results](/home/user/project/plot.png). It is displayed inline, \
                 and the user can click it to open it at full size. PNG, JPEG, WebP, GIF and SVG \
                 files are displayed, but not web URLs: download those first.",
            );
            if let Some(colours) = &self.colours {
                let scheme = if colours.dark { "dark" } else { "light" };
                prompt.push_str(&format!(
                    " The view has a {scheme} colour scheme: make the images you draw for it match, \
                     on a {scheme} background like {} or a transparent one, with text and lines \
                     like {}, {} as the accent, and other colours that read well on that \
                     background.",
                    colours.background, colours.text, colours.accent,
                ));
            }
        }
        if let Some(note) = &self.note {
            prompt.push_str("\n\n");
            prompt.push_str(note);
        }
        prompt
    }
}

/// The first line sent: the SDK's handshake
pub fn initialize(request_id: &str) -> String {
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "initialize", "hooks": null},
    })
    .to_string()
}

/// A user turn. Images go first, as the API prefers
pub fn user_message(text: &str, images: &[(String, String)]) -> String {
    let mut content: Vec<Value> = images
        .iter()
        .map(|(media_type, data)| {
            json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}})
        })
        .collect();
    if !text.is_empty() {
        content.push(json!({"type": "text", "text": text}));
    }
    json!({
        "type": "user",
        "message": {"role": "user", "content": content},
        "parent_tool_use_id": null,
        "session_id": "default",
    })
    .to_string()
}

/// A control request of `subtype`, with `fields` beside it
pub fn control(request_id: &str, subtype: &str, fields: Value) -> String {
    let mut request = json!({"subtype": subtype});
    if let (Some(request), Value::Object(fields)) = (request.as_object_mut(), fields) {
        request.extend(fields);
    }
    json!({"type": "control_request", "request_id": request_id, "request": request}).to_string()
}

/// How the user answered a permission prompt
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    Allow,
    /// Allow, and add the CLI's suggested rule for the rest of the session
    AllowAlways,
    Deny,
}

pub fn permission_response(request: &PermissionRequest, decision: &Decision) -> String {
    let response = match decision {
        Decision::Allow => json!({"behavior": "allow", "updatedInput": request.input}),
        Decision::AllowAlways => json!({
            "behavior": "allow",
            "updatedInput": request.input,
            "updatedPermissions": request.suggestions,
        }),
        Decision::Deny => json!({
            "behavior": "deny",
            "message": "The user declined this tool use.",
            "interrupt": false,
        }),
    };
    json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": request.request_id, "response": response},
    })
    .to_string()
}

/// A reply to a control request the CLI sent that this client does not handle
fn control_error(request_id: &str, error: &str) -> String {
    json!({
        "type": "control_response",
        "response": {"subtype": "error", "request_id": request_id, "error": error},
    })
    .to_string()
}

/// Turn one line of the CLI's output into events. A control request this
/// client cannot answer gets an error reply, returned as the second value
pub fn parse_line(line: &str, out: &mut Vec<Event>) -> Option<String> {
    let Ok(mut value) = serde_json::from_str::<Value>(line) else {
        return None;
    };
    if line.contains("\"base64\"") {
        super::images::spill(&mut value);
    }
    let parent = value.get("parent_tool_use_id").and_then(Value::as_str).map(str::to_string);
    match value.get("type").and_then(Value::as_str).unwrap_or_default() {
        "system" => match value.get("subtype").and_then(Value::as_str) {
            Some("init") => out.push(Event::Started {
                session_id: str_field(&value, "session_id"),
                model: str_field(&value, "model"),
                cwd: str_field(&value, "cwd"),
                permission_mode: str_field(&value, "permissionMode"),
            }),
            Some("compact_boundary") => out.push(Event::Notice("Conversation compacted".into())),
            Some("status") => out.push(Event::Status(str_field(&value, "status"))),
            _ => {}
        },
        "stream_event" => {
            let Some(event) = value.get("event") else { return None };
            stream_event(event, parent, out);
        }
        "assistant" => {
            if let Some(message) = value.get("message") {
                out.push(Event::Assistant {
                    message_id: str_field(message, "id"),
                    content: message.get("content").cloned().unwrap_or(Value::Null),
                    parent,
                });
            }
        }
        "user" => {
            if let Some(message) = value.get("message") {
                out.push(Event::User { content: message.get("content").cloned().unwrap_or(Value::Null), parent });
            }
        }
        "result" => out.push(Event::TurnDone {
            error: value.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            message: value
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                .then(|| str_field(&value, "result"))
                .unwrap_or_default(),
            duration_ms: value.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
            context_tokens: context_tokens(value.get("usage")),
        }),
        "control_request" => {
            let request_id = str_field(&value, "request_id");
            let request = value.get("request").cloned().unwrap_or(Value::Null);
            match request.get("subtype").and_then(Value::as_str) {
                Some("can_use_tool") => out.push(Event::Permission(PermissionRequest {
                    request_id,
                    tool_name: str_field(&request, "tool_name"),
                    tool_use_id: str_field(&request, "tool_use_id"),
                    title: request
                        .get("title")
                        .or_else(|| request.get("description"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    input: request.get("input").cloned().unwrap_or(Value::Null),
                    suggestions: request
                        .get("permission_suggestions")
                        .cloned()
                        .filter(|value| value.as_array().is_some_and(|a| !a.is_empty()))
                        .unwrap_or(Value::Null),
                    always_allowed: !request
                        .get("suppress_always_allow_rule")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })),
                _ => return Some(control_error(&request_id, "not supported by this client")),
            }
        }
        "control_cancel_request" => out.push(Event::PermissionCancelled(str_field(&value, "request_id"))),
        "control_response" => {
            let response = value.get("response").cloned().unwrap_or(Value::Null);
            if let Some(models) = response.pointer("/response/models").and_then(Value::as_array) {
                out.push(Event::Models(
                    models
                        .iter()
                        .filter_map(|model| {
                            let value = model.get("value").and_then(Value::as_str)?;
                            let name = model
                                .get("displayName")
                                .and_then(Value::as_str)
                                .unwrap_or(value);
                            Some((value.to_string(), name.to_string()))
                        })
                        .collect(),
                ));
            }
            if response.get("subtype").and_then(Value::as_str) == Some("error") {
                out.push(Event::Error(str_field(&response, "error")));
            }
        }
        _ => {}
    }
    None
}

fn stream_event(event: &Value, parent: Option<String>, out: &mut Vec<Event>) {
    let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    match event.get("type").and_then(Value::as_str).unwrap_or_default() {
        "message_start" => out.push(Event::MessageStart {
            message_id: event.pointer("/message/id").and_then(Value::as_str).unwrap_or_default().to_string(),
            parent,
        }),
        "content_block_start" => {
            let block = event.get("content_block").cloned().unwrap_or(Value::Null);
            out.push(Event::BlockStart { index, block, parent });
        }
        "content_block_delta" => {
            let Some(delta) = event.get("delta") else { return };
            match delta.get("type").and_then(Value::as_str).unwrap_or_default() {
                "text_delta" => out.push(Event::TextDelta { index, text: str_field(delta, "text"), parent }),
                "thinking_delta" => out.push(Event::ThinkingDelta {
                    index,
                    text: str_field(delta, "thinking"),
                    tokens: delta.get("estimated_tokens").and_then(Value::as_u64),
                    parent,
                }),
                "input_json_delta" => out.push(Event::InputDelta {
                    index,
                    json: str_field(delta, "partial_json"),
                    parent,
                }),
                _ => {}
            }
        }
        "content_block_stop" => out.push(Event::BlockStop { index, parent }),
        _ => {}
    }
}

/// The tokens the last request put in the context window
fn context_tokens(usage: Option<&Value>) -> Option<u64> {
    let usage = usage?;
    let field = |name| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    // The last iteration is the context as it now stands
    if let Some(last) = usage.get("iterations").and_then(Value::as_array).and_then(|i| i.last()) {
        let field = |name| last.get(name).and_then(Value::as_u64).unwrap_or(0);
        return Some(
            field("input_tokens")
                + field("cache_read_input_tokens")
                + field("cache_creation_input_tokens")
                + field("output_tokens"),
        );
    }
    Some(field("input_tokens") + field("cache_read_input_tokens") + field("cache_creation_input_tokens"))
}

fn str_field(value: &Value, name: &str) -> String {
    value.get(name).and_then(Value::as_str).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_keeps_the_subscription_and_quotes_arguments() {
        let launch = Launch {
            cwd: "/home/u/my project".into(),
            resume: Some("abc-1".into()),
            model: Some("opus".into()),
            args: "--add-dir '/a b'".into(),
            ..Launch::default()
        };
        let script = launch.script();
        assert!(script.contains("unset ANTHROPIC_API_KEY"));
        assert!(script.contains("cd '/home/u/my project'"));
        assert!(script.contains("'--resume=abc-1'"));
        assert!(script.contains("--permission-prompt-tool stdio"));
        assert!(script.contains("'/a b'"));
        assert!(!script.contains(" -p "));
        // The view is described before the profile's own arguments
        let prompt = script.find("--append-system-prompt").expect("the view described");
        assert!(prompt < script.find("--add-dir").unwrap());
        assert!(script.contains("as a Markdown image"));
        // Shell-quoted whole, through its `$` and quotes
        let words = shlex::split(script.lines().last().unwrap().trim_start_matches("exec ")).unwrap();
        let at = words.iter().position(|w| w == "--append-system-prompt").unwrap();
        assert!(words[at + 1].contains("between $...$ or \\(...\\) inline"), "{}", words[at + 1]);
        let remote = Launch { host: "neo".into(), ..Launch::default() }.script();
        assert!(remote.contains("--append-system-prompt") && !remote.contains("Markdown image"));
        // The colours, when images should match them
        assert!(!script.contains("colour scheme"));
        let colours = Colours { dark: true, background: "#1b1b1b".into(), text: "#e7e7e7".into(), accent: "#63d0df".into() };
        let matched = Launch { colours: Some(colours), ..Launch::default() }.script();
        assert!(matched.contains("a dark colour scheme") && matched.contains("#1b1b1b") && matched.contains("#63d0df"));
    }

    #[test]
    fn parses_a_permission_request_and_answers_it() {
        let line = r#"{"type":"control_request","request_id":"r9","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"/x","content":"hi"},"tool_use_id":"toolu_1","description":"x","permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}]}}"#;
        let mut events = Vec::new();
        assert!(parse_line(line, &mut events).is_none());
        let Some(Event::Permission(request)) = events.pop() else { panic!("no permission") };
        assert_eq!(request.tool_name, "Write");
        assert_eq!(request.title, "x");
        let reply: Value = serde_json::from_str(&permission_response(&request, &Decision::AllowAlways)).unwrap();
        assert_eq!(reply.pointer("/response/request_id").unwrap(), "r9");
        assert_eq!(reply.pointer("/response/response/behavior").unwrap(), "allow");
        assert_eq!(reply.pointer("/response/response/updatedInput/content").unwrap(), "hi");
        assert_eq!(reply.pointer("/response/response/updatedPermissions/0/mode").unwrap(), "acceptEdits");
        let deny: Value = serde_json::from_str(&permission_response(&request, &Decision::Deny)).unwrap();
        assert_eq!(deny.pointer("/response/response/behavior").unwrap(), "deny");
    }

    #[test]
    fn unknown_control_requests_get_an_error() {
        let mut events = Vec::new();
        let reply = parse_line(r#"{"type":"control_request","request_id":"q","request":{"subtype":"hook_callback"}}"#, &mut events);
        assert!(reply.unwrap().contains("\"error\""));
    }

    #[test]
    fn parses_streamed_text() {
        let mut events = Vec::new();
        parse_line(
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"$x^2$"}},"parent_tool_use_id":null}"#,
            &mut events,
        );
        assert!(matches!(&events[0], Event::TextDelta { index: 1, text, parent: None } if text == "$x^2$"));
    }
}
