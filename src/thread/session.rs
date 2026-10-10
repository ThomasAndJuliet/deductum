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
    /// The name the user, else the agent, gave the conversation
    pub title: Option<String>,
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
    let (mut custom, mut ai) = (None, None);
    let mut history = keep(reader, turns, |mut value, events| {
        if session.is_none() {
            session = value.get("sessionId").and_then(Value::as_str).map(str::to_string);
        }
        let kind = value.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
        let kind = kind.as_str();
        // Names, as `/rename` and the CLI's own titling record them
        match kind {
            "custom-title" => custom = value.get("customTitle").and_then(Value::as_str).map(str::to_string),
            "ai-title" => ai = value.get("aiTitle").and_then(Value::as_str).map(str::to_string),
            _ => {}
        }
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
    history.title = custom.or(ai).filter(|t| !t.trim().is_empty());
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
    History { entries, earlier, title: None }
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
    /// A relayed CLI's relay, on each connection: where the log stood
    /// before the first turn and at the latest turn, its end, and whether a
    /// turn is in progress. What follows comes from the log at the cursor
    Hello { head: u64, turn_start: u64, end: u64, busy: bool },
    /// An input the relay passed to the CLI: `kind` is `turn` for a user's
    /// turn, else `line`; `own` if this stream sent it, else another window
    /// did, or this one before it was restarted
    Input { id: String, kind: String, json: String, own: bool },
    /// The connection to the relay was lost; it is tried again, and inputs
    /// are kept until it has them
    Disconnected,
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

/// A relayed thread to show: its relay's session, where to start reading,
/// and, to start the relay, the CLI's launch
#[derive(Clone, Debug)]
pub struct Attach {
    pub host: String,
    /// The thread's session name, from [`crate::agent::new_thread_session`]
    pub session: String,
    pub from: cosmic_term::relay::Cursor,
    pub start: Option<claude::Launch>,
    /// Joining a running relay: the conversation it holds, if known
    pub conversation: Option<String>,
}

/// What a relay starting `launch` records of it, for the sidebar
fn thread_meta(session: &str, launch: &claude::Launch) -> Value {
    serde_json::json!({
        "session": session,
        "agent": launch.agent.session_kind(),
        "cwd": launch.cwd,
        "conversation": launch.resume,
        "model": launch.model,
        "permission_mode": launch.permission_mode,
        "created": cosmic_term::now(),
    })
}

/// An input for a relay, kept until the relay has echoed it
struct Pending {
    id: String,
    kind: &'static str,
    json: String,
}

impl Pending {
    fn line(&self) -> String {
        format!("{} {} {}\n", self.kind, self.id, self.json)
    }
}

/// What the relay's lines say, given to a thread: see [`run_relayed`]
struct Relayed {
    protocol: super::protocol::Protocol,
    /// Inputs sent, or to send, that the relay has not echoed
    pending: Vec<Pending>,
    /// Where to read the log from on the next connection
    from: cosmic_term::relay::Cursor,
    /// The log offset of the next line
    offset: u64,
    /// This connection's greeting
    hello: Option<cosmic_term::relay::Hello>,
    /// The conversation id recorded in the thread's meta.json, or to be
    conversation: Option<String>,
    meta_sent: Option<String>,
}

/// What one line from a relay comes to
enum Step {
    Out(Output),
    Events(Vec<Event>),
    /// The CLI has ended
    End(Output),
    Nothing,
}

impl Relayed {
    fn new(protocol: super::protocol::Protocol, from: cosmic_term::relay::Cursor) -> Self {
        Self { protocol, pending: Vec::new(), from, offset: 0, hello: None, conversation: None, meta_sent: None }
    }

    fn meta(&mut self, replies: &mut Vec<String>) {
        if let Some(id) = &self.conversation
            && self.meta_sent.as_ref() != Some(id)
        {
            replies.push(format!("meta {}\n", serde_json::json!({"conversation": id})));
            self.meta_sent = Some(id.clone());
        }
    }

    /// Take one line, with its newline, from the connection; gives what it
    /// means, and adds the lines for the relay it calls for to `replies`
    fn line(&mut self, line: &[u8], replies: &mut Vec<String>) -> Step {
        use cosmic_term::relay::{Cursor, Record};
        let text = String::from_utf8_lossy(line);
        let Some(hello) = self.hello else {
            // The greeting, which is not in the log
            let Some(Record::Hello(hello)) = Record::parse(&text) else { return Step::Nothing };
            self.hello = Some(hello);
            self.offset = match self.from {
                Cursor::Offset(n) => n.min(hello.end),
                Cursor::Live if hello.busy => hello.turn_start,
                Cursor::Live => hello.end,
            };
            self.from = Cursor::Offset(self.offset);
            for pending in &self.pending {
                replies.push(pending.line());
            }
            self.meta(replies);
            return Step::Out(Output::Hello {
                head: hello.head,
                turn_start: hello.turn_start,
                end: hello.end,
                busy: hello.busy,
            });
        };
        let at = self.offset;
        self.offset += line.len() as u64;
        self.from = Cursor::Offset(self.offset);
        match Record::parse(&text) {
            Some(Record::Input { id, kind, json }) => {
                let own = match self.pending.iter().position(|p| p.id == id) {
                    Some(index) => {
                        self.pending.remove(index);
                        true
                    }
                    None => false,
                };
                Step::Out(Output::Input { id, kind, json, own })
            }
            Some(Record::Exited { code, stderr }) => Step::End(Output::Exited(Some(code), stderr)),
            Some(Record::Lost) => Step::End(Output::Exited(None, "the thread's agent was lost".into())),
            Some(Record::Hello(_)) => Step::Nothing,
            None => {
                let mut events = Vec::new();
                let text = text.trim_end_matches(['\n', '\r']);
                let mut answers = self.protocol.parse(text, &mut events);
                // Answers to lines of turns before this one were given then.
                // Each is named by its line, so that two windows answering
                // the same line are heard once
                if at < hello.turn_start {
                    answers.clear();
                }
                for (n, json) in answers.into_iter().enumerate() {
                    // A turn that waited for its thread starts here
                    let kind = if self.protocol.starts_turn(&json) { "turn" } else { "line" };
                    let pending = Pending { id: format!("r{at}-{n}"), kind, json };
                    replies.push(pending.line());
                    self.pending.push(pending);
                }
                for event in &events {
                    if let Event::Started { session_id, .. } = event
                        && !session_id.is_empty()
                    {
                        self.conversation = Some(session_id.clone());
                    }
                }
                self.meta(replies);
                if events.is_empty() { Step::Nothing } else { Step::Events(events) }
            }
        }
    }

    /// The lines for a command from the thread, kept until echoed
    fn command(&mut self, command: super::protocol::Command) -> Vec<String> {
        let user = matches!(command, super::protocol::Command::User { .. });
        let mut lines = Vec::new();
        for (n, json) in self.protocol.command(command).into_iter().enumerate() {
            let pending = Pending {
                id: cosmic_term::random_hex(12),
                kind: if user && n == 0 { "turn" } else { "line" },
                json,
            };
            lines.push(pending.line());
            self.pending.push(pending);
        }
        lines
    }
}

/// Like [`run`], with the CLI kept by a relay on its host (see
/// `cosmic_term::relay`), so that it outlives this stream. With
/// `attach.start`, the relay is started first; without, the thread's
/// running relay is joined, or what its log holds is read once it has gone.
/// The log is read from `attach.from` on, with the relay's greeting, each
/// window's inputs and lost connections besides; a lost connection is tried
/// again, from where it was, until the CLI exits. Dropping the sender of
/// [`Output::Ready`] lets go of the relay, which keeps the CLI: end it with
/// [`crate::agent::thread_stop`]. Without the relay's supervisor, a started
/// thread's CLI is run directly, by [`run`]
pub fn run_relayed(attach: Attach) -> impl Stream<Item = Output> {
    cosmic::iced::stream::channel(64, move |mut output: cosmic::iced::futures::channel::mpsc::Sender<Output>| async move {
        use cosmic::iced::futures::StreamExt;
        let session = crate::agent::Session { host: attach.host.clone(), name: attach.session.clone() };
        let launch = attach.start.clone().unwrap_or_else(|| claude::Launch {
            agent: session.agent().unwrap_or_default(),
            host: attach.host.clone(),
            resume: attach.conversation.clone(),
            ..claude::Launch::default()
        });
        let mut protocol = super::protocol::Protocol::new(&launch);
        let start_lines = protocol.start();
        if attach.start.is_none() {
            protocol.attached();
        }
        let done = protocol.turn_done();
        let mut script = None;
        if let Some(start) = &attach.start {
            if let Err(err) = crate::agent::ensure_helper(start.host.clone(), crate::agent::Helper::Chartgen).await {
                log::warn!("installing ct-chartgen: {err}");
            }
            let installed = crate::agent::ensure_helper(start.host.clone(), crate::agent::Helper::Supervise).await;
            if let Err(err) = &installed {
                log::warn!("installing ct-supervise: {err}");
            }
            script = installed.ok().and_then(|()| {
                crate::agent::thread_start_script(
                    &session,
                    &start.script(),
                    done,
                    crate::agent::THREAD_LINGER,
                    &thread_meta(&attach.session, start),
                )
            });
            if script.is_none() {
                let mut direct = std::pin::pin!(run(start.clone()));
                while let Some(item) = direct.next().await {
                    if output.send(item).await.is_err() {
                        return;
                    }
                }
                return;
            }
        }
        let mut relayed = Relayed::new(protocol, attach.from);
        relayed.conversation = launch.resume.clone();
        // A started relay was told in its meta.json
        relayed.meta_sent = attach.start.as_ref().and_then(|start| start.resume.clone());
        // The opening lines go to a new CLI only, named so that resending
        // them is harmless
        if attach.start.is_some() {
            for (n, json) in start_lines.into_iter().enumerate() {
                relayed.pending.push(Pending { id: format!("s{n}"), kind: "line", json });
            }
        }
        relay_loop(&mut output, &session, relayed, script, crate::agent::session_exec).await;
    })
}

/// Connect to the relay with `script` (the start script, or else an attach
/// script), run by `exec` on the session's host, and again whenever the
/// connection is lost
async fn relay_loop(
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<Output>,
    session: &crate::agent::Session,
    mut relayed: Relayed,
    script: Option<String>,
    exec: impl Fn(&str, &str) -> tokio::process::Command,
) {
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    let (to_cli, mut commands) = mpsc::unbounded_channel::<super::protocol::Command>();
    if output.send(Output::Ready(to_cli)).await.is_err() {
        return;
    }
    let mut script = script.unwrap_or_else(|| crate::agent::thread_attach_script(session, relayed.from));
    let mut backoff = Duration::from_secs(1);
    loop {
        let mut command = exec(&session.host, &script);
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        relayed.hello = None;
        let mut ended = None;
        match command.spawn() {
            Ok(mut child) => {
                let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
                else {
                    return;
                };
                let stderr_task = tokio::spawn(async move {
                    let mut text = String::new();
                    let _ = tokio::io::BufReader::new(stderr).take(1 << 16).read_to_string(&mut text).await;
                    text
                });
                let mut reader = tokio::io::BufReader::with_capacity(256 * 1024, stdout);
                let mut stdin_open = true;
                let mut replies = Vec::new();
                let mut line = Vec::new();
                loop {
                    tokio::select! {
                        read = reader.read_until(b'\n', &mut line) => {
                            if !matches!(read, Ok(n) if n > 0) || !line.ends_with(b"\n") {
                                break;
                            }
                            let mut events = Vec::new();
                            let mut outs = Vec::new();
                            // Deltas come in bursts; send what is already here together
                            loop {
                                match relayed.line(&line, &mut replies) {
                                    Step::Events(more) => events.extend(more),
                                    Step::Out(out) => outs.push(out),
                                    Step::End(out) => ended = Some(out),
                                    Step::Nothing => {}
                                }
                                line.clear();
                                if ended.is_some() || events.len() > 256 || !outs.is_empty() {
                                    break;
                                }
                                match tokio::time::timeout(Duration::from_millis(4), reader.read_until(b'\n', &mut line)).await {
                                    Ok(Ok(n)) if n > 0 && line.ends_with(b"\n") => {}
                                    // A partial line stays for the next read
                                    Ok(Ok(n)) if n > 0 => break,
                                    Ok(_) => break,
                                    Err(_) => break,
                                }
                            }
                            for reply in replies.drain(..) {
                                if stdin_open {
                                    stdin_open = stdin.write_all(reply.as_bytes()).await.is_ok() && stdin.flush().await.is_ok();
                                }
                            }
                            if !events.is_empty() && output.send(Output::Events(events)).await.is_err() {
                                return;
                            }
                            for out in outs {
                                if matches!(out, Output::Hello { .. }) {
                                    backoff = Duration::from_secs(1);
                                }
                                if output.send(out).await.is_err() {
                                    return;
                                }
                            }
                            if ended.is_some() {
                                break;
                            }
                        }
                        command = commands.recv() => match command {
                            Some(command) => {
                                for line in relayed.command(command) {
                                    if stdin_open && relayed.hello.is_some() {
                                        stdin_open = stdin.write_all(line.as_bytes()).await.is_ok() && stdin.flush().await.is_ok();
                                    }
                                }
                            }
                            // Let go: the relay keeps the CLI
                            None => {
                                let _ = stdin.shutdown().await;
                                return;
                            }
                        },
                    }
                }
                drop(stdin);
                let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .and_then(|status| status.code());
                let stderr = tokio::time::timeout(Duration::from_secs(1), stderr_task)
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_default();
                // It failed before the relay said anything, and not by
                // losing the connection
                if ended.is_none()
                    && relayed.hello.is_none()
                    && status.is_some_and(|code| code != crate::agent::SSH_CONNECTION_LOST)
                {
                    ended = Some(Output::Exited(status, stderr.trim().to_string()));
                }
            }
            Err(err) => ended = Some(Output::Exited(None, err.to_string())),
        }
        if let Some(out) = ended {
            let _ = output.send(out).await;
            return;
        }
        if output.send(Output::Disconnected).await.is_err() {
            return;
        }
        // Again, from what has been read, keeping commands meanwhile
        script = crate::agent::thread_attach_script(session, relayed.from);
        let sleep = tokio::time::sleep(backoff);
        tokio::pin!(sleep);
        backoff = (backoff * 2).min(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                command = commands.recv() => match command {
                    Some(command) => {
                        relayed.command(command);
                    }
                    None => return,
                },
            }
        }
    }
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

    #[test]
    fn relayed_lines_become_outputs() {
        use cosmic_term::relay::Cursor;
        let mut relayed = Relayed::new(super::super::protocol::Protocol::Claude, Cursor::Live);
        relayed.pending.push(Pending { id: "s0".into(), kind: "line", json: "{\"init\":1}".into() });
        let mut replies = Vec::new();
        // Joined mid-turn: read from the turn's start, and what was not
        // echoed is sent again
        let step = relayed.line(b"#ct hello 1 pid=5 head=10 turn=40 end=200 busy=1\n", &mut replies);
        assert!(matches!(step, Step::Out(Output::Hello { head: 10, turn_start: 40, end: 200, busy: true })));
        assert_eq!(relayed.offset, 40);
        assert_eq!(replies, ["line s0 {\"init\":1}\n"]);
        replies.clear();
        let lines: [&[u8]; 5] = [
            b"#ct< s0 line {\"init\":1}\n",
            b"#ct< other turn {\"type\":\"user\"}\n",
            b"{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"c-1\",\"model\":\"m\",\"cwd\":\"/\",\"permissionMode\":\"default\"}\n",
            b"{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
            b"#ct exited 0 b29wcw==\n",
        ];
        let steps: Vec<Step> = lines.iter().map(|line| relayed.line(line, &mut replies)).collect();
        assert!(matches!(&steps[0], Step::Out(Output::Input { id, kind, own: true, .. }) if id == "s0" && kind == "line"));
        assert!(matches!(&steps[1], Step::Out(Output::Input { id, kind, own: false, .. }) if id == "other" && kind == "turn"));
        assert!(matches!(&steps[2], Step::Events(events) if matches!(&events[0], Event::Started { session_id, .. } if session_id == "c-1")));
        assert!(matches!(&steps[3], Step::Events(events) if matches!(&events[0], Event::Assistant { message_id, .. } if message_id == "m1")));
        assert!(matches!(&steps[4], Step::End(Output::Exited(Some(0), stderr)) if stderr == "oops"));
        assert!(relayed.pending.is_empty());
        // The conversation's id is recorded, once
        assert_eq!(replies, ["meta {\"conversation\":\"c-1\"}\n"]);
        assert_eq!(relayed.offset, 40 + lines.iter().map(|l| l.len() as u64).sum::<u64>());
        assert_eq!(relayed.from, Cursor::Offset(relayed.offset));

        // A user's turn is framed as one, the rest as lines
        let sent = relayed.command(super::super::protocol::Command::User { text: "x".into(), images: Vec::new() });
        assert!(sent[0].starts_with("turn ") && sent[0].ends_with("}\n"), "{sent:?}");
        let sent = relayed.command(super::super::protocol::Command::Interrupt);
        assert!(sent[0].starts_with("line "), "{sent:?}");
        assert_eq!(relayed.pending.len(), 2);
    }

    /// The stream through a real relay, with the `ct-supervise` built
    /// beside the tests and a stand-in Claude: a connection cut mid-turn is
    /// made again from where it was, a turn sent meanwhile arrives once, a
    /// second stream replays what the first saw, and a stopped relay's log
    /// ends the stream
    #[test]
    fn relayed_threads_reconnect_and_replay() {
        use cosmic::iced::futures::StreamExt;
        use cosmic_term::relay::{self, Cursor};
        let Some(supervisor) = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("ct-supervise")))
            .filter(|path| path.exists())
        else {
            eprintln!("no ct-supervise built beside the tests: skipped");
            return;
        };
        let home = std::env::temp_dir().join(format!("ct-relayed-test-{}", cosmic_term::random_hex(8)));
        let session = crate::agent::Session { host: String::new(), name: "ct-thread-claude-t-00000001".into() };
        let dir = home.join(".local/state/cosmic-term/threads").join(&session.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(relay::RELAY_BIN), format!("{}\n", supervisor.display())).unwrap();
        // Each line read is answered with a message and the turn's end
        let fake = "n=0\nwhile IFS= read -r line; do n=$((n + 1))\n\
                    printf '{\"type\":\"assistant\",\"message\":{\"id\":\"m%s\",\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\\n' $n\n\
                    printf '{\"type\":\"result\",\"is_error\":false,\"duration_ms\":1}\\n'\n\
                    done\n";
        let mut options = relay::Options::new(&dir, vec!["sh".into(), "-c".into(), fake.into()]);
        options.done = Some("\"type\":\"result\"".into());
        let relay_thread = std::thread::spawn(move || relay::relay(options, None));
        while !dir.join(relay::STATE).exists() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // The first connection is cut after the greeting and one line
        let cut = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let exec = {
            let (home, cut) = (home.clone(), cut.clone());
            move |_: &str, script: &str| {
                let script = if cut.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    format!("{{ {script}\n}} | {{ IFS= read -r a; printf '%s\\n' \"$a\"; IFS= read -r b; printf '%s\\n' \"$b\"; kill $$; }}")
                } else {
                    script.to_string()
                };
                let mut command = tokio::process::Command::new("sh");
                command.arg("-c").arg(script).env("HOME", &home).env_remove("XDG_STATE_HOME");
                command
            }
        };
        let stream = |from: Cursor, exec: Box<dyn Fn(&str, &str) -> tokio::process::Command + Send>| {
            let session = session.clone();
            cosmic::iced::stream::channel(64, move |mut output: cosmic::iced::futures::channel::mpsc::Sender<Output>| async move {
                let relayed = Relayed::new(super::super::protocol::Protocol::Claude, from);
                relay_loop(&mut output, &session, relayed, None, exec).await;
            })
        };
        let user = |text: &str| super::super::protocol::Command::User { text: text.into(), images: Vec::new() };
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let next = |stream: &mut (dyn cosmic::iced::futures::Stream<Item = Output> + Unpin)| {
            runtime
                .block_on(async { tokio::time::timeout(std::time::Duration::from_secs(20), stream.next()).await })
                .expect("output within 20 s")
        };
        let mut first = Box::pin(stream(Cursor::Offset(0), Box::new(exec)));
        let Some(Output::Ready(tx)) = next(&mut first) else { panic!("not ready") };
        assert!(matches!(next(&mut first), Some(Output::Hello { end: 0, .. })));
        tx.send(user("one")).unwrap();
        assert!(matches!(next(&mut first), Some(Output::Input { kind, own: true, .. }) if kind == "turn"));
        assert!(matches!(next(&mut first), Some(Output::Disconnected)));
        // Sent while the connection is down
        tx.send(user("two")).unwrap();
        let mut seen = Vec::new();
        let mut done = 0;
        while done < 2 {
            match next(&mut first) {
                Some(Output::Events(events)) => {
                    done += events.iter().filter(|e| matches!(e, Event::TurnDone { .. })).count();
                    seen.extend(events);
                }
                Some(Output::Input { kind, own, .. }) => {
                    assert!(own && kind == "turn");
                    seen.push(Event::Notice("input".into()));
                }
                Some(Output::Hello { .. }) => {}
                other => panic!("{other:?}"),
            }
        }
        let messages: Vec<&str> = seen
            .iter()
            .filter_map(|e| match e {
                Event::Assistant { message_id, .. } => Some(message_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(messages, ["m1", "m2"]);
        let log = std::fs::read_to_string(dir.join(relay::LOG)).unwrap();
        assert_eq!(log.matches("#ct< ").count(), 2, "{log}");
        // Letting go ends the stream, and leaves the relay
        drop(tx);
        assert!(next(&mut first).is_none());
        assert!(relay::is_live(&dir));

        // Another window reads it all, and sees the other's turns as theirs
        let exec = {
            let home = home.clone();
            move |_: &str, script: &str| {
                let mut command = tokio::process::Command::new("sh");
                command.arg("-c").arg(script).env("HOME", &home).env_remove("XDG_STATE_HOME");
                command
            }
        };
        let mut second = Box::pin(stream(Cursor::Offset(0), Box::new(exec.clone())));
        let Some(Output::Ready(_tx)) = next(&mut second) else { panic!("not ready") };
        assert!(matches!(next(&mut second), Some(Output::Hello { end, busy: false, .. }) if end == log.len() as u64));
        assert!(matches!(next(&mut second), Some(Output::Input { own: false, .. })));
        assert!(relay::stop(&dir, std::time::Duration::from_secs(10)));
        assert_eq!(relay_thread.join().unwrap(), Ok(0));
        let mut ended = None;
        while let Some(output) = next(&mut second) {
            if let Output::Exited(code, _) = output {
                ended = Some(code);
            }
        }
        assert_eq!(ended, Some(Some(0)));

        // Once it has gone, the log's end is still read
        let mut third = Box::pin(stream(Cursor::Live, Box::new(exec)));
        let Some(Output::Ready(_tx)) = next(&mut third) else { panic!("not ready") };
        assert!(matches!(next(&mut third), Some(Output::Hello { .. })));
        assert!(matches!(next(&mut third), Some(Output::Exited(Some(0), _))));
        assert!(next(&mut third).is_none());
        let _ = std::fs::remove_dir_all(home);
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
                Some(Output::Ready(_) | Output::Hello { .. } | Output::Input { .. } | Output::Disconnected) => {}
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
