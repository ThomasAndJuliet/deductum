# List the projects and conversations of coding agents, as JSON.
#
# Usage: launcher.py claude|codex|all [LIMIT]
#
# For one agent, prints one JSON object to stdout:
#   {"home": "<$HOME>",
#    "projects": [{"path": str, "last_used": float}],
#    "conversations": [{"id": str, "cwd": str, "title": str, "last_used": float}]}
#
# For all, prints {"home", "hostname", "host_id", "agents": {"claude": {...},
# "codex": {...}}}, each agent's object holding its projects and
# conversations. host_id is this computer's id in the terminal's project
# registry, or "" if it has none yet; hostname tells computers apart without
# one.
#
# conversations are the newest LIMIT (default 40) conversations of the agent,
# by file modification time, newest first. The title is a single line of at
# most 80 characters: the name given to the conversation, else its first user
# message. projects are the existing directories the agent knows (Claude:
# ~/.claude.json; Codex: ~/.codex/config.toml), plus the working directories
# of the listed conversations, newest first by last_used, the modification
# time of the newest conversation there (0 if none). $HOME and transient
# directories are left out. Only the head (and for Claude the tail) of each
# transcript is read, and never any credential file.
#
# Runs locally and on remote hosts, so it uses only the standard library.

import glob
import json
import os
import re
import socket
import sys

HOME = os.path.expanduser("~")
HEAD_BYTES = 256 << 10
TAIL_BYTES = 64 << 10
TITLE_CHARS = 80
DEFAULT_LIMIT = 40
TRANSIENT = ("/tmp", "/var/tmp", "/run", "/dev", "/proc")
TRANSIENT_PARTS = ("/.delta/clones/", "/.claude/worktrees/",
    "/.local-state/", "/claude-opus-implementer/")
# Injected user messages that are not something the user typed
CLAUDE_WRAPPERS = (
    "<command-",
    "<local-command",
    "<task-notification",
    "<system-reminder",
    "<user-prompt-submit-hook",
    "<bash-",
    "Caveat:",
    "[Request interrupted",
)
CODEX_WRAPPERS = (
    "<environment_context",
    "<user_instructions",
    "<recommended_plugins",
    "<turn_aborted",
    "<permissions",
    "<skill",
    "# AGENTS.md",
    "# Context from my IDE",
)


def read_head(path, size=HEAD_BYTES):
    """Complete JSON lines from the first size bytes of a file"""
    try:
        with open(path, "rb") as f:
            data = f.read(size)
    except OSError:
        return []
    lines = data.split(b"\n")
    if len(data) == size:
        lines.pop()  # cut short
    return lines


def read_tail(path, size=TAIL_BYTES):
    """Complete lines from the last size bytes of a file"""
    try:
        with open(path, "rb") as f:
            f.seek(0, os.SEEK_END)
            start = max(0, f.tell() - size)
            f.seek(start)
            lines = f.read().split(b"\n")
    except OSError:
        return []
    if start > 0:
        lines.pop(0)  # cut short
    return lines


def parse(line):
    """The JSON object on a line, or {}"""
    try:
        value = json.loads(line)
    except ValueError:
        return {}
    return value if isinstance(value, dict) else {}


def clean_title(text):
    return " ".join(text.split())[:TITLE_CHARS]


def text_of(content):
    """The text of a message's content, or None if it has none (tool results)"""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = [
            b.get("text", "")
            for b in content
            if isinstance(b, dict) and b.get("type") in ("text", "input_text")
        ]
        return "\n".join(parts) if parts else None
    return None


def genuine(text, wrappers):
    text = (text or "").strip()
    return bool(text) and not text.startswith(wrappers)


def is_transient(path):
    return (
        any(path == t or path.startswith(t + "/") for t in TRANSIENT)
        or any(part in path + "/" for part in TRANSIENT_PARTS)
    )


def mtime(path):
    try:
        return os.stat(path).st_mtime
    except OSError:
        return None


def encode_claude(path):
    """The name of Claude's transcript directory for a working directory"""
    return re.sub(r"[^A-Za-z0-9]", "-", path)


# Claude


def claude_conversation(path):
    """Fields of a Claude transcript from its head and tail, or None"""
    cwd = first_user = ai_title = custom_title = None
    for line in read_head(path):
        d = parse(line)
        cwd = cwd or d.get("cwd")
        kind = d.get("type")
        if kind == "user" and first_user is None:
            if d.get("isMeta") or d.get("isSidechain"):
                continue
            message = d.get("message")
            text = text_of(message.get("content")) if isinstance(message, dict) else None
            if genuine(text, CLAUDE_WRAPPERS):
                first_user = text
        elif kind == "ai-title":
            ai_title = d.get("aiTitle") or ai_title
        elif kind == "custom-title":
            custom_title = d.get("customTitle") or custom_title
    # Renames come late in a long conversation
    for line in read_tail(path):
        d = parse(line)
        if d.get("type") == "ai-title":
            ai_title = d.get("aiTitle") or ai_title
        elif d.get("type") == "custom-title":
            custom_title = d.get("customTitle") or custom_title
    title = custom_title or first_user or ai_title
    if not cwd or not title:
        return None
    return {
        "id": os.path.basename(path)[: -len(".jsonl")],
        "cwd": cwd,
        "title": clean_title(title),
    }


def claude_files():
    # Subagent transcripts are in subdirectories, so only direct children
    pattern = os.path.join(HOME, ".claude", "projects", "*", "*.jsonl")
    return [(m, f) for f in glob.glob(pattern) if (m := mtime(f)) is not None]


def claude_projects():
    try:
        with open(os.path.join(HOME, ".claude.json"), "rb") as f:
            projects = json.load(f).get("projects")
    except (OSError, ValueError, AttributeError):
        return []
    return list(projects) if isinstance(projects, dict) else []


def claude(limit):
    files = sorted(claude_files(), reverse=True)
    newest = {}  # transcript directory -> newest mtime
    for m, f in files:
        newest.setdefault(os.path.basename(os.path.dirname(f)), m)
    conversations = []
    for m, f in files:
        if len(conversations) >= limit:
            break
        found = claude_conversation(f)
        if found:
            found["last_used"] = m
            conversations.append(found)
    used = {}
    for path in claude_projects() + [c["cwd"] for c in conversations]:
        used[path] = newest.get(encode_claude(path), 0)
    return used, conversations


# Codex


def codex_meta(path):
    """The session_meta payload of a rollout, or {}"""
    for line in read_head(path, 256 << 10)[:1]:
        d = parse(line)
        payload = d.get("payload")
        if d.get("type") == "session_meta" and isinstance(payload, dict):
            return payload
    return {}


def codex_interactive(meta):
    """Whether a rollout is a conversation of the user, not a subagent's"""
    source = meta.get("source")
    return (
        not isinstance(source, dict)
        and meta.get("thread_source") not in ("subagent", "guardian_review")
        and bool(meta.get("id"))
        and bool(meta.get("cwd"))
    )


def codex_names():
    """Conversation names by id, from the session index"""
    names = {}
    try:
        with open(os.path.join(HOME, ".codex", "session_index.jsonl"), "rb") as f:
            for line in f:
                d = parse(line)
                if d.get("id") and d.get("thread_name"):
                    names[d["id"]] = d["thread_name"]
    except OSError:
        pass
    return names


def codex_first_user(path):
    for line in read_head(path):
        d = parse(line)
        payload = d.get("payload")
        if d.get("type") != "response_item" or not isinstance(payload, dict):
            continue
        if payload.get("type") == "message" and payload.get("role") == "user":
            text = text_of(payload.get("content"))
            if genuine(text, CODEX_WRAPPERS):
                return text
    return None


def codex_files():
    pattern = os.path.join(HOME, ".codex", "sessions", "*", "*", "*", "rollout-*.jsonl")
    return [(m, f) for f in glob.glob(pattern) if (m := mtime(f)) is not None]


def codex_projects():
    try:
        with open(os.path.join(HOME, ".codex", "config.toml"), encoding="utf-8") as f:
            text = f.read()
    except (OSError, ValueError):
        return []
    paths = []
    for match in re.finditer(r'^\s*\[projects\.("(?:[^"\\]|\\.)*"|\'[^\']*\')\]', text, re.M):
        key = match.group(1)
        try:
            paths.append(json.loads(key) if key[0] == '"' else key[1:-1])
        except ValueError:
            pass
    return paths


def codex(limit):
    names = codex_names()
    newest = {}  # cwd -> newest mtime
    conversations = []
    for m, f in sorted(codex_files(), reverse=True):
        meta = codex_meta(f)
        if not codex_interactive(meta):
            continue
        newest.setdefault(meta["cwd"], m)
        if len(conversations) >= limit:
            continue
        title = names.get(meta["id"]) or codex_first_user(f)
        if title:
            conversations.append(
                {
                    "id": meta["id"],
                    "cwd": meta["cwd"],
                    "title": clean_title(title),
                    "last_used": m,
                }
            )
    used = {path: newest.get(path, 0) for path in codex_projects()}
    for path, m in newest.items():
        used[path] = m
    return used, conversations


def projects(used):
    result = []
    for path, m in used.items():
        path = os.path.normpath(path)
        if (
            os.path.isabs(path)
            and path != HOME
            and not is_transient(path)
            and os.path.isdir(path)
        ):
            result.append({"path": path, "last_used": m})
    # A normalised path may repeat
    best = {}
    for p in result:
        best[p["path"]] = max(p["last_used"], best.get(p["path"], 0))
    return sorted(
        ({"path": k, "last_used": v} for k, v in best.items()),
        key=lambda p: (-p["last_used"], p["path"]),
    )


def host_id():
    state = os.environ.get("XDG_STATE_HOME") or os.path.join(HOME, ".local", "state")
    try:
        with open(os.path.join(state, "cosmic-term", "projects.json"), "rb") as f:
            value = json.load(f).get("host_id")
    except (OSError, ValueError, AttributeError):
        return ""
    return value if isinstance(value, str) else ""


def listing(agent, limit):
    used, conversations = (claude if agent == "claude" else codex)(limit)
    return {"projects": projects(used), "conversations": conversations}


def main():
    usage = "usage: launcher.py claude|codex|all [LIMIT]"
    agent = sys.argv[1] if len(sys.argv) > 1 else ""
    if agent not in ("claude", "codex", "all"):
        sys.exit(usage)
    try:
        limit = int(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_LIMIT
    except ValueError:
        sys.exit(usage)
    if agent == "all":
        result = {
            "home": HOME,
            "hostname": socket.gethostname(),
            "host_id": host_id(),
            "agents": {name: listing(name, limit) for name in ("claude", "codex")},
        }
    else:
        result = dict(home=HOME, **listing(agent, limit))
    print(json.dumps(result))


main()
