// SPDX-License-Identifier: GPL-3.0-only

//! Coding agents' saved conversations, read and written in their own formats.
//!
//! Claude keeps a conversation in `~/.claude/projects/<cwd, encoded>/<id>.jsonl`,
//! and every record carries its absolute `cwd`. A conversation moves to
//! another computer by copying that file into the destination directory's
//! folder, changing only the `cwd` fields (through a [`PathMap`]) and the
//! conversation's id, so the copy never shares an id with the original.
//! Everything else, the signed thinking blocks above all, is copied byte for
//! byte; the resumed agent is told how paths moved instead
//! ([`moved_note`]).
//!
//! Codex keeps its own index of conversations besides the files, so a copied
//! Codex conversation is not resumed natively; it, and any conversation
//! continued with a different agent, starts a new conversation from a short
//! [`handoff`] that points at the whole conversation as a readable
//! [`transcript`].

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::Env;

/// The agents whose conversations can move, named as in the terminal's
/// profiles
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Kind {
    Claude,
    Codex,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// Whether a conversation can be resumed as itself on another computer
    pub fn native_import(self) -> bool {
        self == Self::Claude
    }
}

/// A saved conversation, as its file
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConversationFile {
    pub kind: Kind,
    pub id: String,
    /// The working directory it was started in
    pub cwd: String,
    #[serde(with = "crate::b64::serde_bytes")]
    pub data: Vec<u8>,
}

/// Prefix replacements for paths, most specific first
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PathMap(pub Vec<(String, String)>);

impl PathMap {
    /// `path` under the first prefix it is in, or `None`. A prefix matches
    /// whole components only: `/home/a` is not a prefix of `/home/ab`
    pub fn map(&self, path: &str) -> Option<String> {
        self.0.iter().find_map(|(from, to)| {
            let from = from.trim_end_matches('/');
            let rest = path.strip_prefix(from)?;
            (rest.is_empty() || rest.starts_with('/'))
                .then(|| format!("{}{rest}", to.trim_end_matches('/')))
        })
    }
}

/// The name of Claude's folder for a working directory
pub fn claude_folder(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn check_id(id: &str) -> Result<(), String> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        Ok(())
    } else {
        Err(format!("bad conversation id: {id:?}"))
    }
}

/// The file of a conversation on this computer
pub fn find(env: &Env, kind: Kind, id: &str) -> Result<PathBuf, String> {
    check_id(id)?;
    let found = match kind {
        Kind::Claude => {
            let name = format!("{id}.jsonl");
            fs::read_dir(env.home.join(".claude/projects"))
                .into_iter()
                .flatten()
                .flatten()
                .map(|dir| dir.path().join(&name))
                .find(|path| path.is_file())
        }
        Kind::Codex => {
            let suffix = format!("-{id}.jsonl");
            let mut found = None;
            // sessions/YYYY/MM/DD/rollout-…-<id>.jsonl
            let mut dirs = vec![(env.home.join(".codex/sessions"), 0)];
            while let Some((dir, depth)) = dirs.pop() {
                for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
                    let path = entry.path();
                    if depth < 3 && path.is_dir() {
                        dirs.push((path, depth + 1));
                    } else if path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(&suffix))
                    {
                        found = Some(path);
                    }
                }
            }
            found
        }
    };
    found.ok_or_else(|| format!("no {} conversation {id} on this computer", kind.label()))
}

pub fn export(env: &Env, kind: Kind, id: &str) -> Result<ConversationFile, String> {
    let path = find(env, kind, id)?;
    let data = fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    let cwd = recorded_cwd(kind, &data).unwrap_or_default();
    Ok(ConversationFile {
        kind,
        id: id.to_string(),
        cwd,
        data,
    })
}

fn recorded_cwd(kind: Kind, data: &[u8]) -> Option<String> {
    data.split(|b| *b == b'\n').find_map(|line| {
        let value: serde_json::Value = serde_json::from_slice(line).ok()?;
        let cwd = match kind {
            Kind::Claude => value.get("cwd"),
            Kind::Codex => value.get("payload")?.get("cwd"),
        };
        cwd?.as_str().map(str::to_string)
    })
}

/// Replace the string values of `"key":"…"` members in a line of compact
/// JSON, leaving every other byte as it was. Inside a JSON string a quote
/// is escaped, so the unescaped pattern only ever matches a member name
fn rewrite_member(line: &str, key: &str, change: &dyn Fn(&str) -> Option<String>) -> String {
    let pattern = format!("\"{key}\":\"");
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(&pattern) {
        let start = at + pattern.len() - 1; // the value's opening quote
        out.push_str(&rest[..start]);
        let value_text = &rest[start..];
        // The string literal's end: the first unescaped quote
        let mut end = None;
        let mut escaped = false;
        for (i, c) in value_text.char_indices().skip(1) {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => {
                    end = Some(i + 1);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push_str(value_text);
            return out;
        };
        let literal = &value_text[..end];
        let replaced = serde_json::from_str::<String>(literal)
            .ok()
            .and_then(|value| change(&value))
            .and_then(|value| serde_json::to_string(&value).ok());
        out.push_str(replaced.as_deref().unwrap_or(literal));
        rest = &value_text[end..];
    }
    out.push_str(rest);
    out
}

/// A Claude conversation with its `cwd` fields moved through `map` and
/// its id changed to `new_id`
pub fn rewrite_claude(data: &[u8], old_id: &str, new_id: &str, map: &PathMap) -> Vec<u8> {
    let text = String::from_utf8_lossy(data);
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = rewrite_member(line, "cwd", &|cwd| map.map(cwd));
        let line = rewrite_member(&line, "sessionId", &|id| {
            (id == old_id).then(|| new_id.to_string())
        });
        out.push_str(&line);
    }
    out.into_bytes()
}

/// Where an imported conversation went
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Imported {
    pub id: String,
    pub path: PathBuf,
}

/// Save a conversation from another computer here, under a new id, for
/// resuming in `cwd` (the destination's path for its working directory)
pub fn import(
    env: &Env,
    file: &ConversationFile,
    map: &PathMap,
    cwd: &str,
) -> Result<Imported, String> {
    match file.kind {
        Kind::Claude => {
            check_id(&file.id)?;
            let id = crate::random_uuid();
            let dir = env.home.join(".claude/projects").join(claude_folder(cwd));
            fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
            let path = dir.join(format!("{id}.jsonl"));
            let data = rewrite_claude(&file.data, &file.id, &id, map);
            fs::write(&path, data).map_err(|err| format!("{}: {err}", path.display()))?;
            Ok(Imported { id, path })
        }
        Kind::Codex => Err("Codex conversations continue as new conversations".into()),
    }
}

/// Text appended to a resumed conversation's system prompt, saying where
/// its paths went
pub fn moved_note(source: &str, destination: &str, map: &PathMap) -> String {
    let mut note = format!(
        "This conversation was moved from the computer {source} to {destination}. \
         Paths in earlier messages refer to {source}; on this computer they are:"
    );
    for (from, to) in &map.0 {
        note.push_str(&format!("\n- {from} is now {to}"));
    }
    note
}

/// Messages typed by the user, not added by the agent's harness
const WRAPPERS: [&str; 15] = [
    "<command-",
    "<local-command",
    "<task-notification",
    "<system-reminder",
    "<user-prompt-submit-hook",
    "<bash-",
    "Caveat:",
    "<environment_context",
    "<user_instructions",
    "<recommended_plugins",
    "<turn_aborted",
    "<permissions",
    "<skill",
    "# AGENTS.md",
    "# Context from my IDE",
];

/// One entry of a readable transcript
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    User(String),
    Assistant(String),
    Tool { name: String, input: String },
    Output(String),
}

fn texts(content: &serde_json::Value, kinds: &[&str]) -> Option<String> {
    match content {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(blocks) => {
            let parts: Vec<&str> = blocks
                .iter()
                .filter(|b| {
                    b.get("type")
                        .and_then(|t| t.as_str())
                        .is_some_and(|t| kinds.contains(&t))
                })
                .filter_map(|b| b.get("text")?.as_str())
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

fn genuine(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && !WRAPPERS.iter().any(|w| text.starts_with(w))
}

/// A conversation's messages and tool use, without reasoning
pub fn entries(file: &ConversationFile) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in file.data.split(|b| *b == b'\n') {
        let Ok(d) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        match file.kind {
            Kind::Claude => {
                if d.get("isSidechain").and_then(|v| v.as_bool()) == Some(true)
                    || d.get("isMeta").and_then(|v| v.as_bool()) == Some(true)
                {
                    continue;
                }
                let Some(content) = d.get("message").and_then(|m| m.get("content")) else {
                    continue;
                };
                match d.get("type").and_then(|t| t.as_str()) {
                    Some("user") => {
                        if let Some(text) = texts(content, &["text"]).filter(|t| genuine(t)) {
                            out.push(Entry::User(text));
                        }
                        for block in content.as_array().into_iter().flatten() {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                                && let Some(text) =
                                    block.get("content").and_then(|c| texts(c, &["text"]))
                            {
                                out.push(Entry::Output(text));
                            }
                        }
                    }
                    Some("assistant") => {
                        if let Some(text) = texts(content, &["text"]) {
                            out.push(Entry::Assistant(text));
                        }
                        for block in content.as_array().into_iter().flatten() {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                out.push(Entry::Tool {
                                    name: block
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("tool")
                                        .to_string(),
                                    input: block
                                        .get("input")
                                        .map(|i| i.to_string())
                                        .unwrap_or_default(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Kind::Codex => {
                if d.get("type").and_then(|t| t.as_str()) != Some("response_item") {
                    continue;
                }
                let Some(p) = d.get("payload") else { continue };
                let field = |name: &str| {
                    p.get(name)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                match p.get("type").and_then(|t| t.as_str()) {
                    Some("message") => {
                        let Some(content) = p.get("content") else {
                            continue;
                        };
                        match p.get("role").and_then(|r| r.as_str()) {
                            Some("user") => {
                                if let Some(text) =
                                    texts(content, &["input_text"]).filter(|t| genuine(t))
                                {
                                    out.push(Entry::User(text));
                                }
                            }
                            Some("assistant") => {
                                if let Some(text) = texts(content, &["output_text"]) {
                                    out.push(Entry::Assistant(text));
                                }
                            }
                            _ => {}
                        }
                    }
                    Some("function_call") => out.push(Entry::Tool {
                        name: field("name"),
                        input: field("arguments"),
                    }),
                    Some("custom_tool_call") => out.push(Entry::Tool {
                        name: field("name"),
                        input: field("input"),
                    }),
                    Some("function_call_output" | "custom_tool_call_output") => {
                        let output = match p.get("output") {
                            Some(serde_json::Value::String(s)) => s.clone(),
                            Some(other) => other.to_string(),
                            None => String::new(),
                        };
                        out.push(Entry::Output(output));
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// Long tool output is cut to this many characters in a transcript
const OUTPUT_CHARS: usize = 20_000;

fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!(
            "{}\n… ({} more characters)",
            &text[..at],
            text[at..].chars().count()
        ),
        None => text.to_string(),
    }
}

/// The whole conversation as Markdown
pub fn transcript(file: &ConversationFile, source_host: &str) -> String {
    let mut out = format!(
        "# {} conversation {}\n\nFrom {source_host}, started in `{}`.\n",
        file.kind.label(),
        file.id,
        file.cwd
    );
    for entry in entries(file) {
        match entry {
            Entry::User(text) => out.push_str(&format!("\n## User\n\n{text}\n")),
            Entry::Assistant(text) => {
                out.push_str(&format!("\n## {}\n\n{text}\n", file.kind.label()))
            }
            Entry::Tool { name, input } => {
                out.push_str(&format!(
                    "\n**Tool call `{name}`**\n\n```\n{}\n```\n",
                    cut(&input, OUTPUT_CHARS)
                ));
            }
            Entry::Output(text) => {
                out.push_str(&format!(
                    "\n**Result**\n\n```\n{}\n```\n",
                    cut(&text, OUTPUT_CHARS)
                ));
            }
        }
    }
    out
}

/// The short first message of a conversation continuing another one
pub fn handoff(
    file: &ConversationFile,
    source_host: &str,
    map: &PathMap,
    transcript_path: &Path,
) -> String {
    let last_request = entries(file)
        .into_iter()
        .rev()
        .find_map(|e| match e {
            Entry::User(text) => Some(text),
            _ => None,
        })
        .unwrap_or_default();
    let mut text = format!(
        "This continues a {} conversation from the computer {source_host}. The complete \
         conversation is in {}; read it before doing anything else, then carry on from where \
         it ended.",
        file.kind.label(),
        transcript_path.display()
    );
    if !map.0.is_empty() {
        text.push_str(" Paths in it refer to the other computer; here:");
        for (from, to) in &map.0 {
            text.push_str(&format!(" {from} is now {to};"));
        }
        text.pop();
        text.push('.');
    }
    if !last_request.is_empty() {
        text.push_str(&format!(
            "\n\nThe user's last message there was:\n\n{}",
            cut(&last_request, 2000)
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tempdir::TempDir;

    const SIGNED: &str = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"look in /home/a/x and \"cwd\":\"/home/a\"","signature":"CAQS9QoKEAgS"}]},"cwd":"/home/a/x","sessionId":"old-1"}"#;

    fn map() -> PathMap {
        PathMap(vec![
            ("/home/a/x".into(), "/home/b/worktrees/x-main".into()),
            ("/home/a".into(), "/home/b".into()),
        ])
    }

    #[test]
    fn path_maps_match_whole_components_most_specific_first() {
        let map = map();
        assert_eq!(
            map.map("/home/a/x/src").as_deref(),
            Some("/home/b/worktrees/x-main/src")
        );
        assert_eq!(map.map("/home/a/y").as_deref(), Some("/home/b/y"));
        assert_eq!(map.map("/home/a").as_deref(), Some("/home/b"));
        assert_eq!(map.map("/home/ab"), None);
        assert_eq!(map.map("/tmp"), None);
    }

    #[test]
    fn rewriting_changes_only_cwd_and_the_id() {
        let line = rewrite_claude(SIGNED.as_bytes(), "old-1", "new-2", &map());
        let expected = SIGNED
            .replace(
                r#""cwd":"/home/a/x""#,
                r#""cwd":"/home/b/worktrees/x-main""#,
            )
            .replace(r#""sessionId":"old-1""#, r#""sessionId":"new-2""#);
        assert_eq!(String::from_utf8(line).unwrap(), expected);
        // The thinking block, escaped quotes and all, is byte for byte the same
        assert!(expected.contains(
            r#""thinking":"look in /home/a/x and \"cwd\":\"/home/a\"","signature":"CAQS9QoKEAgS""#
        ));
    }

    #[test]
    fn rewriting_keeps_unmapped_values_and_other_ids() {
        let line = r#"{"cwd":"/elsewhere","sessionId":"other","x":"\"cwd\":\"/home/a\""}"#;
        assert_eq!(
            rewrite_claude(line.as_bytes(), "old-1", "new", &map()),
            line.as_bytes()
        );
        let escaped = r#"{"cwd":"/home/a/café \"q\""}"#;
        let out = String::from_utf8(rewrite_claude(escaped.as_bytes(), "", "", &map())).unwrap();
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["cwd"], "/home/b/café \"q\"");
    }

    #[test]
    fn claude_conversations_import_under_a_new_id_in_the_new_folder() {
        let tmp = TempDir::new();
        let env = Env::at(tmp.path());
        let file = ConversationFile {
            kind: Kind::Claude,
            id: "old-1".into(),
            cwd: "/home/a/x".into(),
            data: format!("{SIGNED}\n").into_bytes(),
        };
        let imported = import(&env, &file, &map(), "/home/b/worktrees/x-main").unwrap();
        assert_ne!(imported.id, "old-1");
        assert_eq!(
            imported.path,
            tmp.path()
                .join(".claude/projects/-home-b-worktrees-x-main")
                .join(format!("{}.jsonl", imported.id))
        );
        let back = export(&env, Kind::Claude, &imported.id).unwrap();
        assert_eq!(back.cwd, "/home/b/worktrees/x-main");
        assert!(
            String::from_utf8(back.data)
                .unwrap()
                .contains(&format!("\"sessionId\":\"{}\"", imported.id))
        );
        assert!(
            import(
                &env,
                &ConversationFile {
                    kind: Kind::Codex,
                    ..file
                },
                &map(),
                "/x"
            )
            .is_err()
        );
    }

    #[test]
    fn finds_codex_rollouts_by_id() {
        let tmp = TempDir::new();
        let env = Env::at(tmp.path());
        let dir = tmp.path().join(".codex/sessions/2026/10/02");
        fs::create_dir_all(&dir).unwrap();
        let line = r#"{"type":"session_meta","payload":{"id":"abc-1","cwd":"/home/a/x"}}"#;
        fs::write(dir.join("rollout-2026-10-02T15-46-37-abc-1.jsonl"), line).unwrap();
        let file = export(&env, Kind::Codex, "abc-1").unwrap();
        assert_eq!(file.cwd, "/home/a/x");
        assert!(export(&env, Kind::Codex, "abc").is_err());
        assert!(find(&env, Kind::Claude, "../etc").is_err());
    }

    #[test]
    fn transcripts_keep_messages_and_tools_but_not_reasoning() {
        let claude = ConversationFile {
            kind: Kind::Claude,
            id: "c".into(),
            cwd: "/home/a/x".into(),
            data: [
                r#"{"type":"user","message":{"content":"<command-name>/x</command-name>"}}"#,
                r#"{"type":"user","message":{"content":"fix the bug"}}"#,
                SIGNED,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Looking."},{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"a.txt"}]}}"#,
                r#"{"type":"user","isSidechain":true,"message":{"content":"subagent"}}"#,
            ]
            .join("\n")
            .into_bytes(),
        };
        assert_eq!(
            entries(&claude),
            vec![
                Entry::User("fix the bug".into()),
                Entry::Assistant("Looking.".into()),
                Entry::Tool {
                    name: "Bash".into(),
                    input: r#"{"command":"ls"}"#.into()
                },
                Entry::Output("a.txt".into()),
            ]
        );
        let text = transcript(&claude, "au-laptop");
        assert!(text.contains("fix the bug") && !text.contains("look in"));

        let codex = ConversationFile {
            kind: Kind::Codex,
            id: "x".into(),
            cwd: "/home/a/x".into(),
            data: [
                r##"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md stuff"}]}}"##,
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add tests"}]}}"#,
                r#"{"type":"response_item","payload":{"type":"reasoning","encrypted_content":"zzz"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"ls\"}"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call_output","output":"ok"}}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done."}]}}"#,
            ]
            .join("\n")
            .into_bytes(),
        };
        assert_eq!(
            entries(&codex),
            vec![
                Entry::User("add tests".into()),
                Entry::Tool {
                    name: "shell".into(),
                    input: r#"{"cmd":"ls"}"#.into()
                },
                Entry::Output("ok".into()),
                Entry::Assistant("Done.".into()),
            ]
        );
        let note = handoff(&codex, "neo", &map(), Path::new("/t/transcript.md"));
        assert!(note.contains("/t/transcript.md") && note.contains("add tests"));
        assert!(note.contains("/home/a is now /home/b."));
    }
}
