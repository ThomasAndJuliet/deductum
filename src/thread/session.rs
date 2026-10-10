// SPDX-License-Identifier: GPL-3.0-only

//! A thread's history, read from the CLI's transcript, and its live CLI.
//!
//! A Claude transcript (`~/.claude/projects/<dir>/<id>.jsonl`) can run to
//! hundreds of megabytes, mostly tool output. It is read a line at a time,
//! each line cut down to what the view keeps, and only its last
//! `turns` user turns are kept, as t3code pages its history: the thread
//! offers the earlier ones on request. A Codex rollout file
//! (`~/.codex/sessions/YYYY/MM/DD/rollout-…-<id>.jsonl`) is read the same
//! way.

use std::collections::VecDeque;
use std::io::BufRead;

use cosmic::iced::futures::{SinkExt, Stream};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use super::model::{Entries, Event, TOOL_TEXT_LIMIT, cut};
use super::{claude, codex};
use crate::config::Agent;

/// A conversation's history, as far back as was asked
#[derive(Debug)]
pub struct History {
    pub entries: Entries,
    /// User turns not loaded
    pub earlier: usize,
}

/// The last `turns` user turns of `agent`'s conversation `id` on `host`
pub fn load(agent: Agent, host: &str, id: &str, turns: usize) -> Result<History, String> {
    if !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || id.is_empty() {
        return Err(format!("bad conversation id: {id:?}"));
    }
    let codex = agent == Agent::Codex;
    let history = |reader: &mut dyn BufRead| {
        if codex { read_codex(reader, turns, host.is_empty()) } else { read(reader, turns) }
    };
    if host.is_empty() {
        let env = cosmic_term::Env::from_process();
        let kind = if codex { cosmic_term::provider::Kind::Codex } else { cosmic_term::provider::Kind::Claude };
        let path = cosmic_term::provider::find(&env, kind, id)?;
        let file = std::fs::File::open(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        return Ok(history(&mut std::io::BufReader::with_capacity(256 * 1024, file)));
    }
    let files = if codex {
        format!("\"$HOME\"/.codex/sessions/*/*/*/rollout-*-{id}.jsonl")
    } else {
        format!("\"$HOME\"/.claude/projects/*/{id}.jsonl")
    };
    let script = format!(
        "for f in {files}; do [ -f \"$f\" ] && exec cat \"$f\"; done\n\
         echo 'conversation not found' >&2; exit 1\n"
    );
    let mut child = crate::agent::session_exec_std(host, &script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("ssh: {err}"))?;
    let stdout = child.stdout.take().ok_or("no output")?;
    let history = history(&mut std::io::BufReader::with_capacity(256 * 1024, stdout));
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    if !output.status.success() && history.entries.items.is_empty() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(history)
}

/// Read a Claude transcript, keeping its last `turns` user turns
pub fn read(reader: impl BufRead, turns: usize) -> History {
    let mut session = None;
    let mut history = keep(reader, turns, |mut value, events| {
        if session.is_none() {
            session = value.get("sessionId").and_then(Value::as_str).map(str::to_string);
        }
        let kind = value.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
        let kind = kind.as_str();
        if !matches!(kind, "user" | "assistant") {
            return None;
        }
        // Subagents' own transcripts, and the CLI's notes to itself
        if value.get("isSidechain").and_then(Value::as_bool).unwrap_or(false)
            || value.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
        {
            return None;
        }
        if kind == "user" && is_command_noise(&value) {
            return None;
        }
        slim(&mut value);
        let starts_turn = kind == "user" && is_user_text(&value);
        claude::parse_line(&value.to_string(), events);
        Some(starts_turn)
    });
    history.entries.session_id = session;
    history
}

/// Read a Codex rollout file, keeping its last `turns` user turns. Images
/// are shown from their files when `local`
pub fn read_codex(reader: impl BufRead, turns: usize, local: bool) -> History {
    let mut session = None;
    let mut rollout = codex::Rollout::new(local);
    let mut history = keep(reader, turns, |value, events| {
        if session.is_none() {
            session = codex::Rollout::session_id(&value);
        }
        rollout.line(&value, events)
    });
    history.entries.session_id = session;
    history
}

/// The last `turns` user turns of a transcript, each line's events given by
/// `line`, with whether it starts a turn, or `None` for a line left out
fn keep(reader: impl BufRead, turns: usize, mut line: impl FnMut(Value, &mut Vec<Event>) -> Option<bool>) -> History {
    let turns = turns.max(1);
    // Each turn's events, cut down; whole turns drop off the front
    let mut kept: VecDeque<Vec<Event>> = VecDeque::new();
    let mut earlier = 0;
    for bytes in reader.split(b'\n') {
        let Ok(bytes) = bytes else { break };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
        drop(bytes);
        let mut events = Vec::new();
        let Some(starts_turn) = line(value, &mut events) else { continue };
        if starts_turn || kept.is_empty() {
            kept.push_back(Vec::new());
            if kept.len() > turns {
                kept.pop_front();
                earlier += 1;
            }
        }
        if let Some(turn) = kept.back_mut() {
            turn.extend(events);
        }
    }
    let mut entries = Entries::default();
    for event in kept.into_iter().flatten() {
        entries.apply(event);
    }
    entries.stopped();
    History { entries, earlier }
}

/// Whether a user line is the user's own words, not tool results
fn is_user_text(value: &Value) -> bool {
    match value.pointer("/message/content") {
        Some(Value::String(_)) => true,
        Some(Value::Array(blocks)) => blocks
            .iter()
            .any(|block| matches!(block.get("type").and_then(Value::as_str), Some("text") | Some("image"))),
        _ => false,
    }
}

/// Slash commands and their output, which the CLI records as user text
fn is_command_noise(value: &Value) -> bool {
    let text = match value.pointer("/message/content") {
        Some(Value::String(text)) => text.as_str(),
        Some(Value::Array(blocks)) => blocks
            .first()
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        _ => return false,
    };
    let text = text.trim_start();
    text.starts_with("<command-name>")
        || text.starts_with("<local-command-")
        || text.starts_with("<command-message>")
        || text.starts_with("Caveat: The messages below were generated")
}

/// Cut every long string but the assistant's own prose, which is shown in
/// full
fn slim(value: &mut Value) {
    // Duplicates of the tool results, with more detail than is shown
    if let Some(map) = value.as_object_mut() {
        map.remove("toolUseResult");
        map.remove("snapshot");
    }
    if let Some(content) = value.pointer_mut("/message/content").and_then(Value::as_array_mut) {
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_result") | Some("tool_use") => cut_strings(block),
                _ => {}
            }
        }
    }
}

fn cut_strings(value: &mut Value) {
    match value {
        Value::String(text) if text.len() > TOOL_TEXT_LIMIT => *text = cut(text, TOOL_TEXT_LIMIT),
        Value::Array(values) => values.iter_mut().for_each(cut_strings),
        Value::Object(map) => {
            // Image data is spilled, not cut
            if map.get("type").and_then(Value::as_str) == Some("base64") {
                return;
            }
            map.values_mut().for_each(cut_strings);
        }
        _ => {}
    }
}

/// What a running CLI says
#[derive(Clone, Debug)]
pub enum Output {
    /// Lines for it go here; dropping the sender ends it
    Ready(mpsc::UnboundedSender<super::protocol::Command>),
    Events(Vec<Event>),
    /// It exited, with the end of what it wrote to stderr
    Exited(Option<i32>, String),
}

/// Run the CLI, as `launch` says. The stream ends when it exits
pub fn run(launch: claude::Launch) -> impl Stream<Item = Output> {
    cosmic::iced::stream::channel(64, move |mut output: cosmic::iced::futures::channel::mpsc::Sender<Output>| async move {
        // The agent checks its charts with it; a thread works without it
        if let Err(err) = crate::agent::ensure_helper(launch.host.clone(), crate::agent::Helper::Chartgen).await {
            log::warn!("installing ct-chartgen: {err}");
        }
        let mut command = crate::agent::session_exec(&launch.host, &launch.script());
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let _ = output.send(Output::Exited(None, err.to_string())).await;
                return;
            }
        };
        let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return;
        };
        let mut protocol = super::protocol::Protocol::new(&launch);
        let (to_cli, mut lines_in) = mpsc::unbounded_channel::<super::protocol::Command>();
        let _ = output.send(Output::Ready(to_cli)).await;
        // The thread holds the only sender: when it lets go, stdin closes and
        // the CLI ends
        let write = async |stdin: &mut tokio::process::ChildStdin, mut line: String| {
            line.push('\n');
            stdin.write_all(line.as_bytes()).await.is_ok() && stdin.flush().await.is_ok()
        };
        let mut stdin_open = true;
        for line in protocol.start() {
            if stdin_open {
                stdin_open = write(&mut stdin, line).await;
            }
        }
        // stderr's end, for when it fails
        let stderr_task = tokio::spawn(async move {
            let mut tail = VecDeque::new();
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tail.push_back(line);
                if tail.len() > 20 {
                    tail.pop_front();
                }
            }
            tail.into_iter().collect::<Vec<_>>().join("\n")
        });
        let mut lines = tokio::io::BufReader::with_capacity(256 * 1024, stdout).lines();
        let mut replies = Vec::new();
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Ok(Some(line)) = line else { break };
                    let mut events = Vec::new();
                    replies.extend(protocol.parse(&line, &mut events));
                    // Deltas come in bursts; send what is already here together
                    while let Ok(Ok(Some(line))) =
                        tokio::time::timeout(std::time::Duration::from_millis(4), lines.next_line()).await
                    {
                        replies.extend(protocol.parse(&line, &mut events));
                        if events.len() > 256 {
                            break;
                        }
                    }
                    for reply in replies.drain(..) {
                        if stdin_open {
                            stdin_open = write(&mut stdin, reply).await;
                        }
                    }
                    if !events.is_empty() && output.send(Output::Events(events)).await.is_err() {
                        break;
                    }
                }
                command = lines_in.recv(), if stdin_open => match command {
                    Some(command) => {
                        for line in protocol.command(command) {
                            if stdin_open {
                                stdin_open = write(&mut stdin, line).await;
                            }
                        }
                    }
                    None => {
                        let _ = stdin.shutdown().await;
                        // Give it a moment to finish its transcript, then stop it
                        let deadline = tokio::time::sleep(std::time::Duration::from_secs(5));
                        tokio::pin!(deadline);
                        loop {
                            tokio::select! {
                                line = lines.next_line() => if !matches!(line, Ok(Some(_))) { break },
                                _ = &mut deadline => { let _ = child.start_kill(); break }
                            }
                        }
                        break;
                    }
                },
            }
        }
        let status = child.wait().await.ok().and_then(|status| status.code());
        let tail = stderr_task.await.unwrap_or_default();
        let _ = output.send(Output::Exited(status, tail)).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::model::ItemKind;

    fn transcript(turns: usize) -> String {
        let mut out = String::from("{\"type\":\"summary\",\"summary\":\"x\"}\n");
        for n in 0..turns {
            out.push_str(&format!(
                "{{\"type\":\"user\",\"sessionId\":\"s1\",\"message\":{{\"role\":\"user\",\"content\":\"question {n}\"}}}}\n\
                 {{\"type\":\"assistant\",\"message\":{{\"id\":\"m{n}\",\"content\":[{{\"type\":\"tool_use\",\"id\":\"t{n}\",\"name\":\"Bash\",\"input\":{{\"command\":\"ls\"}}}}]}}}}\n\
                 {{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"t{n}\",\"content\":\"{}\"}}]}}}}\n\
                 {{\"type\":\"assistant\",\"message\":{{\"id\":\"m{n}b\",\"content\":[{{\"type\":\"text\",\"text\":\"answer {n}\"}}]}}}}\n",
                "y".repeat(100_000)
            ));
        }
        out.push_str("{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"meta\"}}\n");
        out
    }

    #[test]
    fn keeps_only_the_last_turns_and_cuts_tool_output() {
        let history = read(std::io::Cursor::new(transcript(50)), 3);
        assert_eq!(history.earlier, 47);
        assert_eq!(history.entries.session_id.as_deref(), Some("s1"));
        let items = &history.entries.items;
        assert_eq!(items.len(), 9, "{items:#?}");
        assert!(matches!(&items[0].kind, ItemKind::User { text, .. } if text == "question 47"));
        assert!(matches!(&items[8].kind, ItemKind::Text { text, .. } if text == "answer 49"));
        // 3 turns of 100 kB results, cut to a few kB each
        assert!(history.entries.weight() < 3 * 2 * TOOL_TEXT_LIMIT, "{}", history.entries.weight());
    }

    /// A rollout file of `turns` turns, as codex-cli 0.162 writes them
    fn rollout(turns: usize) -> String {
        let mut out = String::from(
            "{\"timestamp\":\"t\",\"type\":\"session_meta\",\"payload\":{\"id\":\"01a1-thread\",\"cwd\":\"/w\"}}\n\
             {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"developer\",\"content\":[{\"type\":\"input_text\",\"text\":\"rules\"}]}}\n",
        );
        let item = |item: String| {
            format!("{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"item_completed\",\"thread_id\":\"01a1-thread\",\"turn_id\":\"t\",\"item\":{item}}}}}\n")
        };
        for n in 0..turns {
            out.push_str(&item(format!(
                "{{\"type\":\"UserMessage\",\"id\":\"u{n}\",\"content\":[{{\"type\":\"text\",\"text\":\"question {n}\",\"text_elements\":[]}}]}}"
            )));
            out.push_str(&item(format!("{{\"type\":\"Reasoning\",\"id\":\"r{n}\",\"summary_text\":[],\"raw_content\":[]}}")));
            out.push_str(&format!(
                "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"custom_tool_call\",\"call_id\":\"x{n}\",\"name\":\"exec\",\"input\":\"code\"}}}}\n"
            ));
            out.push_str(&item(format!(
                "{{\"type\":\"CommandExecution\",\"id\":\"c{n}\",\"command\":[\"/usr/bin/bash\",\"-lc\",\"ls\"],\"status\":\"completed\",\"stdout\":\"{0}\",\"aggregated_output\":\"{0}\",\"exit_code\":0}}",
                "y".repeat(100_000)
            )));
            out.push_str(&item(format!(
                "{{\"type\":\"FileChange\",\"id\":\"f{n}\",\"changes\":{{\"/w/a.rs\":{{\"type\":\"update\",\"unified_diff\":\"@@ -1 +1 @@\\n-a\\n+b\\n\",\"move_path\":null}}}},\"status\":\"completed\"}}"
            )));
            out.push_str(&format!(
                "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call\",\"name\":\"update_plan\",\"call_id\":\"p{n}\",\"arguments\":\"{{\\\"plan\\\":[{{\\\"step\\\":\\\"Look\\\",\\\"status\\\":\\\"completed\\\"}}]}}\"}}}}\n"
            ));
            out.push_str(&item(format!(
                "{{\"type\":\"McpToolCall\",\"id\":\"m{n}\",\"server\":\"docs\",\"tool\":\"search\",\"arguments\":{{\"q\":\"x\"}},\"status\":\"completed\",\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"found\"}}],\"isError\":false}}}}"
            )));
            out.push_str(&item(format!(
                "{{\"type\":\"AgentMessage\",\"id\":\"a{n}\",\"content\":[{{\"type\":\"Text\",\"text\":\"answer {n}\"}}],\"phase\":\"final_answer\"}}"
            )));
            out.push_str("{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"t\"}}\n");
        }
        out
    }

    #[test]
    fn reads_a_codex_rollout_keeping_the_last_turns() {
        let history = read_codex(std::io::Cursor::new(rollout(30)), 2, true);
        assert_eq!(history.earlier, 28);
        assert_eq!(history.entries.session_id.as_deref(), Some("01a1-thread"));
        let items = &history.entries.items;
        let titles: Vec<String> = items
            .iter()
            .map(|item| match &item.kind {
                ItemKind::User { text, .. } | ItemKind::Text { text, .. } => text.clone(),
                ItemKind::Tool(tool) => tool.title(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            titles[..6],
            ["question 28", "Bash ls", "Edit /w/a.rs", "TodoWrite", "docs: search x", "answer 28"],
            "{titles:#?}"
        );
        assert_eq!(items.len(), 12);
        let ItemKind::Tool(edit) = &items[2].kind else { panic!() };
        assert_eq!(edit.patch().as_deref(), Some("@@ -1 +1 @@\n-a\n+b"));
        let ItemKind::Tool(mcp) = &items[4].kind else { panic!() };
        assert_eq!(mcp.output, "found");
        // 2 turns of 100 kB output, cut to a few kB each
        assert!(history.entries.weight() < 2 * 2 * TOOL_TEXT_LIMIT, "{}", history.entries.weight());
    }

    #[test]
    fn reads_older_codex_rollouts() {
        let lines = r#"{"type":"session_meta","payload":{"id":"old-1"}}
{"type":"event_msg","payload":{"type":"user_message","message":"hello","images":[],"local_images":[]}}
{"type":"event_msg","payload":{"type":"agent_reasoning","text":"**Thinking**"}}
{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","call_id":"c1"}}
{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"a.txt"}}
{"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch","call_id":"c2"}}
{"type":"event_msg","payload":{"type":"patch_apply_end","call_id":"c2","success":true,"changes":{"/w/n.txt":{"type":"add","content":"new\n"}}}}
{"type":"event_msg","payload":{"type":"agent_message","message":"done","phase":"final_answer"}}
"#;
        let history = read_codex(std::io::Cursor::new(lines), 5, true);
        let items = &history.entries.items;
        assert_eq!(items.len(), 5, "{items:#?}");
        assert!(matches!(&items[0].kind, ItemKind::User { text, .. } if text == "hello"));
        assert!(matches!(&items[1].kind, ItemKind::Thinking { text, .. } if text == "**Thinking**"));
        let ItemKind::Tool(bash) = &items[2].kind else { panic!() };
        assert_eq!((bash.title().as_str(), bash.output.as_str()), ("Bash ls", "a.txt"));
        let ItemKind::Tool(write) = &items[3].kind else { panic!() };
        assert_eq!(write.title(), "Write /w/n.txt");
        assert!(matches!(&items[4].kind, ItemKind::Text { text, .. } if text == "done"));
    }

    #[test]
    fn command_noise_is_left_out() {
        let lines = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/clear</command-name>\"}}\n";
        let history = read(std::io::Cursor::new(lines), 5);
        assert!(history.entries.items.is_empty());
    }
}

/// The real CLI, on this computer, using the user's own login. Run with
/// `cargo test -- --ignored live_` where the agent is installed and logged in
#[cfg(test)]
mod live {
    use super::*;
    use crate::thread::claude::{Decision, Launch};
    use crate::thread::protocol::Command;

    fn user(text: &str) -> Command {
        Command::User { text: text.to_string(), images: Vec::new() }
    }
    use crate::thread::model::{Activity, ItemKind};
    use cosmic::iced::futures::StreamExt;

    /// Feed the CLI's output to `entries` until its turn ends, answering
    /// permission prompts with `decision`; how many it answered
    async fn turn(
        stream: &mut (impl Stream<Item = Output> + Unpin),
        tx: &mpsc::UnboundedSender<Command>,
        entries: &mut Entries,
        decision: Decision,
    ) -> usize {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
        let mut answered = 0;
        loop {
            let output = tokio::time::timeout_at(deadline, stream.next()).await.expect("a turn within 3 minutes");
            match output {
                Some(Output::Events(events)) => {
                    let done = events.iter().any(|e| matches!(e, Event::TurnDone { .. }));
                    for event in events {
                        entries.apply(event);
                    }
                    for request in entries.permissions.clone() {
                        tx.send(Command::Answer { request: request.clone(), decision: decision.clone() }).unwrap();
                        entries.answered(&request.request_id, decision != Decision::Deny);
                        answered += 1;
                    }
                    if done {
                        return answered;
                    }
                }
                Some(Output::Exited(code, stderr)) => panic!("the CLI exited ({code:?}): {stderr}"),
                Some(Output::Ready(_)) => {}
                None => panic!("the CLI's output ended"),
            }
        }
    }

    #[test]
    #[ignore]
    fn live_claude_streams_math_and_asks_permission() {
        let dir = std::env::temp_dir().join(format!("ct-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let launch = Launch {
                cwd: dir.display().to_string(),
                model: Some("haiku".into()),
                permission_mode: Some("default".into()),
                ..Launch::default()
            };
            let mut stream = Box::pin(run(launch));
            let Some(Output::Ready(tx)) = stream.next().await else { panic!("no CLI") };
            let mut entries = Entries::default();

            tx.send(user("Reply with exactly this and nothing else: $e^{i\\pi}+1=0$")).unwrap();
            turn(&mut stream, &tx, &mut entries, Decision::Allow).await;
            let text: String = entries
                .items
                .iter()
                .filter_map(|item| match &item.kind {
                    ItemKind::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(text.contains("e^{i\\pi}"), "{text:?}");
            assert!(entries.session_id.is_some());
            assert_eq!(entries.activity, Activity::Idle);
            // The formula was streamed into a parsed document with math in it
            let doc = entries.items.iter().find_map(|item| match &item.kind {
                ItemKind::Text { doc, .. } => Some(doc.clone()),
                _ => None,
            });
            assert!(doc.unwrap().formulas().next().is_some());

            tx.send(user(
                "Use the Write tool to create the file hello.txt here containing the word hi. Do not use Bash.",
            ))
            .unwrap();
            turn(&mut stream, &tx, &mut entries, Decision::Allow).await;
            let written = std::fs::read_to_string(dir.join("hello.txt")).unwrap_or_default();
            assert!(written.contains("hi"), "{:#?}", entries.items);
            drop(tx);
            // Letting go of the CLI ends it
            let mut exited = false;
            while let Some(output) = stream.next().await {
                if let Output::Exited(..) = output {
                    exited = true;
                }
            }
            assert!(exited);
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    fn text_of(entries: &Entries) -> String {
        entries
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Run where `codex` is installed and logged in to ChatGPT
    #[test]
    #[ignore]
    fn live_codex_streams_math_asks_permission_and_resumes() {
        let dir = std::env::temp_dir().join(format!("ct-live-codex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let launch = Launch {
            agent: Agent::Codex,
            cwd: dir.display().to_string(),
            model: Some("gpt-5.6-luna".into()),
            permission_mode: Some("default".into()),
            args: "-c 'model_reasoning_effort=\"low\"'".into(),
            ..Launch::default()
        };
        let session = runtime.block_on(async {
            let mut stream = Box::pin(run(launch.clone()));
            let Some(Output::Ready(tx)) = stream.next().await else { panic!("no CLI") };
            let mut entries = Entries::default();

            tx.send(user("Reply with exactly this and nothing else: $e^{i\\pi}+1=0$")).unwrap();
            entries.user(String::new(), Vec::new());
            turn(&mut stream, &tx, &mut entries, Decision::Allow).await;
            let text = text_of(&entries);
            assert!(text.contains("e^{i\\pi}"), "{:#?}", entries.items);
            assert_eq!(entries.activity, Activity::Idle);
            assert!(!entries.models.is_empty());
            assert!(entries.context_tokens.is_some());

            // A command outside the trusted few is asked about
            tx.send(user("Run the shell command `touch hello.txt` in the current directory, then reply done.")).unwrap();
            entries.user(String::new(), Vec::new());
            let asked = turn(&mut stream, &tx, &mut entries, Decision::Allow).await;
            assert!(asked > 0 && dir.join("hello.txt").exists(), "{:#?}", entries.items);
            assert!(
                entries.items.iter().any(|item| matches!(&item.kind, ItemKind::Tool(tool) if tool.name == "Bash")),
                "{:#?}",
                entries.items
            );
            drop(tx);
            while stream.next().await.is_some() {}
            entries.session_id.expect("the thread's id")
        });

        // Its history reads back from the rollout file
        let history = load(Agent::Codex, "", &session, 5).unwrap();
        assert_eq!(history.entries.session_id.as_deref(), Some(session.as_str()));
        assert!(text_of(&history.entries).contains("e^{i\\pi}"), "{:#?}", history.entries.items);
        assert!(history.entries.items.iter().any(|item| matches!(&item.kind, ItemKind::User { text, .. } if text.contains("touch"))));

        // And the thread resumes
        runtime.block_on(async {
            let mut stream = Box::pin(run(Launch { resume: Some(session.clone()), ..launch }));
            let Some(Output::Ready(tx)) = stream.next().await else { panic!("no CLI") };
            let mut entries = Entries::default();
            while entries.session_id.is_none() {
                match tokio::time::timeout(std::time::Duration::from_secs(60), stream.next()).await.expect("resumed") {
                    Some(Output::Events(events)) => events.into_iter().for_each(|event| entries.apply(event)),
                    Some(Output::Exited(code, stderr)) => panic!("the CLI exited ({code:?}): {stderr}"),
                    _ => {}
                }
            }
            assert_eq!(entries.session_id, Some(session));
            assert!(!entries.items.iter().any(|item| matches!(item.kind, ItemKind::Notice { .. })), "{:#?}", entries.items);
            drop(tx);
            while stream.next().await.is_some() {}
        });
        let _ = std::fs::remove_dir_all(dir);
    }
}
