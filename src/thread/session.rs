// SPDX-License-Identifier: GPL-3.0-only

//! A thread's history, read from the CLI's transcript, and its live CLI.
//!
//! A Claude transcript (`~/.claude/projects/<dir>/<id>.jsonl`) can run to
//! hundreds of megabytes, mostly tool output. It is read a line at a time,
//! each line cut down to what the view keeps, and only its last
//! `turns` user turns are kept, as t3code pages its history: the thread
//! offers the earlier ones on request.

use std::collections::VecDeque;
use std::io::BufRead;

use cosmic::iced::futures::{SinkExt, Stream};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use super::claude;
use super::model::{Entries, Event, TOOL_TEXT_LIMIT, cut};

/// A conversation's history, as far back as was asked
#[derive(Debug)]
pub struct History {
    pub entries: Entries,
    /// User turns not loaded
    pub earlier: usize,
}

/// The last `turns` user turns of conversation `id` on `host`
pub fn load(host: &str, id: &str, turns: usize) -> Result<History, String> {
    if !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || id.is_empty() {
        return Err(format!("bad conversation id: {id:?}"));
    }
    if host.is_empty() {
        let env = cosmic_term::Env::from_process();
        let path = cosmic_term::provider::find(&env, cosmic_term::provider::Kind::Claude, id)?;
        let file = std::fs::File::open(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        return Ok(read(std::io::BufReader::with_capacity(256 * 1024, file), turns));
    }
    let script = format!(
        "for f in \"$HOME\"/.claude/projects/*/{id}.jsonl; do [ -f \"$f\" ] && exec cat \"$f\"; done\n\
         echo 'conversation not found' >&2; exit 1\n"
    );
    let mut child = crate::agent::session_exec_std(host, &script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("ssh: {err}"))?;
    let stdout = child.stdout.take().ok_or("no output")?;
    let history = read(std::io::BufReader::with_capacity(256 * 1024, stdout), turns);
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    if !output.status.success() && history.entries.items.is_empty() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(history)
}

/// Read a transcript, keeping its last `turns` user turns
pub fn read(reader: impl BufRead, turns: usize) -> History {
    let turns = turns.max(1);
    // Each turn's events, cut down; whole turns drop off the front
    let mut kept: VecDeque<Vec<Event>> = VecDeque::new();
    let mut earlier = 0;
    let mut session = None;
    for line in reader.split(b'\n') {
        let Ok(line) = line else { break };
        let Ok(mut value) = serde_json::from_slice::<Value>(&line) else { continue };
        drop(line);
        if session.is_none() {
            session = value.get("sessionId").and_then(Value::as_str).map(str::to_string);
        }
        let kind = value.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
        let kind = kind.as_str();
        if !matches!(kind, "user" | "assistant") {
            continue;
        }
        // Subagents' own transcripts, and the CLI's notes to itself
        if value.get("isSidechain").and_then(Value::as_bool).unwrap_or(false)
            || value.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
        {
            continue;
        }
        if kind == "user" && is_command_noise(&value) {
            continue;
        }
        slim(&mut value);
        let starts_turn = kind == "user" && is_user_text(&value);
        let line = value.to_string();
        let mut events = Vec::new();
        claude::parse_line(&line, &mut events);
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
    entries.session_id = session;
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

    #[test]
    fn command_noise_is_left_out() {
        let lines = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/clear</command-name>\"}}\n";
        let history = read(std::io::Cursor::new(lines), 5);
        assert!(history.entries.items.is_empty());
    }
}

/// The real CLI, on this computer, using the user's own login. Run with
/// `cargo test -- --ignored live_` where `claude` is installed and logged in
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
    /// permission prompts with `decision`
    async fn turn(
        stream: &mut (impl Stream<Item = Output> + Unpin),
        tx: &mpsc::UnboundedSender<Command>,
        entries: &mut Entries,
        decision: Decision,
    ) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
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
                    }
                    if done {
                        return;
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
}
