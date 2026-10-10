# Print the project directories of a terminal tab, one per line, or describe
# the coding agents running in it.
#
# Usage: project-dirs.py env TOKEN | project-dirs.py pid PID
#        | project-dirs.py zellij SESSION
#        (then optionally --cwd or --json)
#        | project-dirs.py sessions
#
# The tab's processes are those whose environment has COSMIC_TERM_TAB=TOKEN,
# or the given PID, or the children of the Zellij server for SESSION, together
# with all their descendants.
#
# A coding agent rarely stays where it started: asked to make a worktree, or
# started in ~ and pointed at a project, it carries on elsewhere while its
# process's working directory stays put. So each agent's *focus* is read from
# what it has been doing lately: the directories of its recent commands (`cd`,
# `git -C`, Codex's `workdir`) and the files it read, wrote or patched, each
# replaced by its git repository root and ranked by how often and how recently
# it was used. A Claude process is found by its ~/.claude/sessions/PID.json,
# which names its conversation; a Codex process by the thread lock it holds in
# ~/.codex/thread-writer-locks.
#
# By default: each agent's leading projects (up to three, each used at least a
# quarter as much as its first), then the directories a Claude process was
# given with --add-dir (from the latest environment snapshot in its
# transcript) or /add-dir (from the command's confirmations). Where an agent
# merely started, such as ~, is not a project. With no project found, an
# agent's directory, else the working directory of the deepest process. Each
# is replaced by its git repository root. With --cwd, only the directory the tab's relative paths
# are relative to is printed, as it is. With --json, one object:
#   {"cwd": deepest process's directory,
#    "agents": [{"kind": "claude"|"codex", "pid", "conversation": id or null,
#                "started_in", "cwd": its current directory, "focus": [roots],
#                "projects": its leading roots,
#                "status": Claude's status, or "running"}]}
#
# With sessions, a JSON list describing every running Zellij session this
# terminal made (named ct-…), for its sidebar:
#   [{"name", "clients": attached clients, "cwd", "project": git root,
#     "repository": the main worktree of its repository, "branch",
#     "command": a terminal's foreground command, "screen": its last lines,
#     "since": when that command started,
#     "agents": as above, each with "title", "state": "working"|"idle",
#               "last": its latest message, "updated": transcript mtime}]
# and for each rich-text thread whose relay is running (named ct-thread-…,
# with or without Zellij), a row of the same shape with
#     "thread": {"agent", "busy", "conversation", "meta": its meta.json,
#                "turn_start", "end": offsets into its log, "created"}
# Threads' directories whose relay has been gone a day are removed.
#
# Runs locally and on remote hosts, so it uses only the standard library.

import glob
import json
import os
import re
import shlex
import subprocess
import sys

HOME = os.path.expanduser("~")
TAIL_BYTES = 4 << 20


def read(path):
    try:
        with open(path, "rb") as f:
            return f.read()
    except OSError:
        return None


def parents():
    """Map each PID to its parent"""
    ppid = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        stat = read(f"/proc/{entry}/stat")
        if stat is None:
            continue
        # The command name is in parentheses and may contain spaces
        fields = stat[stat.rfind(b")") + 2 :].split()
        ppid[int(entry)] = int(fields[1])
    return ppid


def zellij_server(session):
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        cmdline = read(f"/proc/{entry}/cmdline") or b""
        argv = cmdline.split(b"\0")
        if b"--server" in argv and any(
            arg.endswith(b"/" + session.encode()) for arg in argv
        ):
            return int(entry)
    return None


def descendants(root, ppid):
    """PIDs under root, breadth first, with their depth"""
    children = {}
    for pid, parent in ppid.items():
        children.setdefault(parent, []).append(pid)
    order = [(root, 0)]
    for pid, depth in order:
        order.extend((child, depth + 1) for child in sorted(children.get(pid, [])))
    return order


def cwd(pid):
    try:
        return os.readlink(f"/proc/{pid}/cwd")
    except OSError:
        return None


def claude_dirs(pid):
    """The directories of a Claude session, or None if pid isn't Claude"""
    session = read(os.path.join(HOME, ".claude", "sessions", f"{pid}.json"))
    if session is None:
        return None
    try:
        session = json.loads(session)
    except ValueError:
        return None
    dirs = [session.get("cwd")]
    transcripts = glob.glob(
        os.path.join(HOME, ".claude", "projects", "*", f"{session.get('sessionId')}.jsonl")
    )
    for transcript in transcripts:
        try:
            with open(transcript, "rb") as f:
                f.seek(0, os.SEEK_END)
                f.seek(max(0, f.tell() - TAIL_BYTES))
                lines = f.read().splitlines()
        except OSError:
            continue
        for line in reversed(lines):
            if b'"environment"' not in line:
                continue
            try:
                attachment = json.loads(line).get("attachment") or {}
            except ValueError:
                continue
            if attachment.get("type") != "environment":
                continue
            snapshot = attachment.get("snapshot") or {}
            dirs = [snapshot.get("workingDirectory")] + list(
                snapshot.get("additionalWorkingDirectories") or []
            )
            break
        # The snapshots list only the --add-dir directories given at launch;
        # those added later with /add-dir appear only in its confirmation
        dirs.extend(added_dirs(transcript))
    return [d for d in dirs if d]


ADDED_DIR = re.compile(r"Added (.+?) as a working directory")
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def added_dirs(transcript):
    """Directories added to a Claude session with /add-dir"""
    dirs = []
    try:
        with open(transcript, "rb") as f:
            for line in f:
                if b"as a working directory" not in line:
                    continue
                try:
                    content = (json.loads(line).get("message") or {}).get("content")
                except ValueError:
                    continue
                if not isinstance(content, str) or "<local-command-stdout>" not in content:
                    continue
                match = ADDED_DIR.search(ANSI.sub("", content))
                if match:
                    dirs.append(match.group(1))
    except OSError:
        pass
    return dirs


def git_root(path):
    try:
        result = subprocess.run(
            ["git", "-C", path, "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return path


# Recent activity read for an agent's focus
FOCUS_EVENTS = 40
# Directories that are never a project
NOT_PROJECTS = ("/tmp", "/var/tmp", "/proc", "/dev", "/run")


# How far back a transcript is read, at most, to find enough tool calls:
# screenshots and long outputs make some lines megabytes long
TAIL_MAX_BYTES = 64 << 20


def tail_lines(path, size=TAIL_BYTES, enough=None):
    """The complete lines at the end of a file: the last size bytes, or, given
    (needle, count), as far back as needed for count lines containing
    needle, within TAIL_MAX_BYTES"""
    try:
        with open(path, "rb") as f:
            f.seek(0, os.SEEK_END)
            end = f.tell()
            start = max(0, end - size)
            while True:
                f.seek(start)
                data = f.read(end - start)
                if (enough is None or start == 0 or end - start >= TAIL_MAX_BYTES
                        or data.count(enough[0]) >= enough[1]):
                    break
                start = max(0, end - 2 * (end - start))
            lines = data.splitlines()
    except OSError:
        return []
    return lines[1:] if start > 0 else lines


def expand(path, base):
    path = path.strip().strip("'\"")
    if path in ("~", "$HOME", "${HOME}"):
        return HOME
    for prefix in ("~/", "$HOME/", "${HOME}/"):
        if path.startswith(prefix):
            return os.path.join(HOME, path[len(prefix):])
    if not path or "$" in path or "`" in path:
        return None
    return os.path.normpath(os.path.join(base or HOME, path))


COMMAND_DIR = re.compile(
    r"""(?:^|&&|;|\n|\|\||\()\s*(?:cd|pushd)\s+("[^"]+"|'[^']+'|[^\s;&|)]+)"""
    r"""|\bgit\s+-C\s+("[^"]+"|'[^']+'|[^\s;&|)]+)"""
)


def command_dirs(command, base):
    """Directories a shell command works in: its cd and git -C targets"""
    found = []
    for match in COMMAND_DIR.finditer(command or ""):
        path = expand(match.group(1) or match.group(2), base)
        if path:
            found.append(path)
    return found


def file_dir(path, base):
    path = expand(path, base) if isinstance(path, str) else None
    return os.path.dirname(path) if path else None


def claude_events(lines):
    """Directories a Claude transcript's recent tool use worked in, oldest first"""
    events, cwd = [], None
    for line in lines:
        if b'"tool_use"' not in line and b'"cwd"' not in line:
            continue
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get("cwd") and d["cwd"] != cwd:
            cwd = d["cwd"]
            events.append(cwd)
        content = (d.get("message") or {}).get("content")
        for block in content if isinstance(content, list) else []:
            if not isinstance(block, dict) or block.get("type") != "tool_use":
                continue
            given = block.get("input") or {}
            if not isinstance(given, dict):
                continue
            if isinstance(given.get("command"), str):
                events.extend(command_dirs(given["command"], cwd))
            for key in ("file_path", "notebook_path"):
                if given.get(key):
                    events.append(file_dir(given[key], cwd))
            if isinstance(given.get("path"), str):
                events.append(expand(given["path"], cwd))
    return [e for e in events if e]


PATCH_FILE = re.compile(r"^\*\*\* (?:Update|Add|Delete) File: (.+)$", re.M)


def codex_events(lines):
    """Directories a Codex rollout's recent tool use worked in, oldest first"""
    events, cwd = [], None
    for line in lines:
        try:
            d = json.loads(line)
        except ValueError:
            continue
        payload = d.get("payload") or {}
        if d.get("type") == "turn_context" and payload.get("cwd"):
            if payload["cwd"] != cwd:
                cwd = payload["cwd"]
                events.append(cwd)
            continue
        if d.get("type") != "response_item":
            continue
        kind = payload.get("type")
        if kind == "function_call":
            try:
                given = json.loads(payload.get("arguments") or "{}")
            except ValueError:
                continue
            if not isinstance(given, dict):
                continue
            workdir = given.get("workdir") or given.get("cwd")
            base = expand(workdir, cwd) if isinstance(workdir, str) else cwd
            if base:
                events.append(base)
            command = given.get("cmd") or given.get("command")
            if isinstance(command, list):
                command = " ".join(str(c) for c in command)
            if isinstance(command, str):
                events.extend(command_dirs(command, base))
        elif kind == "custom_tool_call" and isinstance(payload.get("input"), str):
            for path in PATCH_FILE.findall(payload["input"]):
                events.append(file_dir(path.strip(), cwd))
    return [e for e in events if e]


ROOTS = {}


def project_root(path):
    """The git root of a directory, or the directory; None if not a project"""
    while path and not os.path.isdir(path):
        path = os.path.dirname(path)
    if path and path not in ROOTS:
        ROOTS[path] = git_root(path)
    # Scratch space counts only when it holds a repository
    if path and any(path == p or path.startswith(p + "/") for p in NOT_PROJECTS) and not os.path.exists(
        os.path.join(ROOTS[path], ".git")
    ):
        return None
    if not path or path == HOME or path == "/" or any(
        path == os.path.join(HOME, d) or path.startswith(os.path.join(HOME, d) + "/")
        for d in (".claude", ".codex", ".cache", ".config", ".local")
    ):
        return None
    if path not in ROOTS:
        ROOTS[path] = git_root(path)
    root = ROOTS[path]
    return None if root == HOME else root


def ranked(events):
    """Projects of recent events with their scores, most used first; recent
    use weighs more"""
    recent = events[-FOCUS_EVENTS:]
    scores, last = {}, {}
    for i, path in enumerate(recent):
        root = project_root(path)
        if root:
            # A repository is a project; a plain directory only perhaps
            weight = 1 if os.path.exists(os.path.join(root, ".git")) else 0.25
            scores[root] = scores.get(root, 0) + weight * (1 + i / len(recent))
            last[root] = i
    order = sorted(scores, key=lambda r: (-scores[r], -last[r]))
    return [(root, scores[root]) for root in order]


def rank(events):
    return [root for root, _score in ranked(events)]


# The projects an editor opens for an agent: its leading ones, each used at
# least this share as much as the first
EDITOR_SHARE = 0.25
EDITOR_PROJECTS = 3


def leading(events):
    found = ranked(events)
    if not found:
        return []
    top = found[0][1]
    return [root for root, score in found[:EDITOR_PROJECTS] if score >= EDITOR_SHARE * top]


def claude_agent(pid):
    session = read(os.path.join(HOME, ".claude", "sessions", f"{pid}.json"))
    if session is None:
        return None
    try:
        session = json.loads(session)
    except ValueError:
        return None
    conversation = session.get("sessionId")
    transcripts = glob.glob(os.path.join(HOME, ".claude", "projects", "*", f"{conversation}.jsonl"))
    lines = tail_lines(transcripts[0], enough=(b'"tool_use"', FOCUS_EVENTS)) if transcripts else []
    events = claude_events(lines)
    summary = claude_summary(transcripts[0], lines) if transcripts else {}
    current = next((json.loads(l).get("cwd") for l in reversed(lines) if b'"cwd"' in l and json.loads(l).get("cwd")), None) if lines else None
    return {"kind": "claude", "pid": pid, "conversation": conversation,
            "started_in": session.get("cwd"), "cwd": current or session.get("cwd"),
            "focus": rank(events), "projects": leading(events),
            "status": session.get("status") or "running",
            "state": "working" if session.get("status") == "busy" else "idle", **summary}


LOCK = re.compile(r"/thread-writer-locks/([0-9a-fA-F-]+)\.lock$")


def codex_daemon(pid):
    """Whether a process is Codex's shared app server, which holds the locks
    of every thread its clients have open, so not those of any one agent"""
    return "app-server" in (read(f"/proc/{pid}/cmdline") or b"").decode(errors="replace").split("\0")


def codex_agent(pid, guess=True, screen=None):
    """A Codex process and its conversation: certain when it holds the
    thread's lock or was resumed by id. Otherwise guess is False to give up,
    "screen" to settle only for a conversation shown on screen (see
    codex_guess), or True to fall back on the newest one begun there"""
    if codex_daemon(pid):
        return None
    try:
        fds = os.listdir(f"/proc/{pid}/fd")
    except OSError:
        return None
    thread = None
    for fd in fds:
        try:
            match = LOCK.search(os.readlink(f"/proc/{pid}/fd/{fd}"))
        except OSError:
            continue
        if match:
            thread = match.group(1)
            break
    guessed = False
    if thread is None:
        argv = (read(f"/proc/{pid}/cmdline") or b"").decode(errors="replace").split("\0")
        if not any(os.path.basename(a) in ("codex", "codex.js") for a in argv[:2]):
            return None
        # Resumed by id, or the newest conversation begun here since it started
        if "resume" in argv and argv.index("resume") + 1 < len(argv) and UUID.fullmatch(argv[argv.index("resume") + 1]):
            thread = argv[argv.index("resume") + 1]
        elif not guess:
            return None
        else:
            thread = codex_guess(cwd(pid), started_at(pid), codex_prompt(argv), screen,
                                 fallback=guess is True)
            guessed = True
        if thread is None:
            return None
    rollouts = glob.glob(os.path.join(HOME, ".codex", "sessions", "*", "*", "*", f"rollout-*-{thread}.jsonl"))
    lines = tail_lines(rollouts[0], enough=(b'_call"', FOCUS_EVENTS)) if rollouts else []
    started = None
    for line in (read(rollouts[0]) or b"").split(b"\n", 1)[:1] if rollouts else []:
        try:
            started = (json.loads(line).get("payload") or {}).get("cwd")
        except ValueError:
            pass
    events = codex_events(lines)
    summary = codex_summary(rollouts[0], thread, lines) if rollouts else {}
    return {"kind": "codex", "pid": pid, "conversation": thread, "guessed": guessed,
            "started_in": started, "cwd": events[-1] if events else started,
            "focus": rank(events), "projects": leading(events), "status": "running", **summary}


# What the sidebar shows of a conversation

SNIPPET_CHARS = 240
TITLE_CHARS = 80
HEAD_BYTES = 256 << 10
# Injected user messages that are not something the user typed
CLAUDE_WRAPPERS = ("<command-", "<local-command", "<task-notification", "<system-reminder",
                   "<user-prompt-submit-hook", "<bash-", "Caveat:", "[Request interrupted")
CODEX_WRAPPERS = ("<environment_context", "<user_instructions", "<recommended_plugins",
                  "<turn_aborted", "<permissions", "<skill", "# AGENTS.md", "# Context from my IDE")


def one_line(text, limit):
    text = " ".join((text or "").split())
    return text if len(text) <= limit else text[: limit - 1].rstrip() + "…"


def text_of(content):
    """The text of a message's content, or None if it has none (tool results)"""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = [b.get("text", "") for b in content
                 if isinstance(b, dict) and b.get("type") in ("text", "input_text", "output_text")]
        return "\n".join(parts) if parts else None
    return None


def genuine(text, wrappers):
    text = (text or "").strip()
    return bool(text) and not text.startswith(wrappers)


def head_lines(path):
    data = (read(path) or b"")[:HEAD_BYTES]
    return data.split(b"\n")


def mtime(path):
    try:
        return os.path.getmtime(path)
    except OSError:
        return 0


def claude_summary(transcript, lines):
    """A Claude conversation's title and latest message"""
    title = first_user = last = None
    for line in head_lines(transcript):
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get("type") == "user" and not d.get("isMeta") and not d.get("isSidechain"):
            text = text_of((d.get("message") or {}).get("content"))
            if genuine(text, CLAUDE_WRAPPERS):
                first_user = text
                break
    named = {}
    for line in lines:
        if b'"ai-title"' in line or b'"custom-title"' in line:
            try:
                d = json.loads(line)
            except ValueError:
                continue
            for kind, key in (("ai-title", "aiTitle"), ("custom-title", "customTitle")):
                if d.get("type") == kind and d.get(key):
                    named[kind] = d[key]
    for line in reversed(lines):
        if b'"assistant"' not in line:
            continue
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get("type") == "assistant" and not d.get("isSidechain"):
            text = text_of((d.get("message") or {}).get("content"))
            if text and text.strip():
                last = text
                break
    title = named.get("custom-title") or named.get("ai-title") or first_user
    return {"title": one_line(title, TITLE_CHARS), "last": one_line(last, SNIPPET_CHARS),
            "updated": mtime(transcript)}


def codex_summary(rollout, thread, lines):
    """A Codex conversation's title, latest message and whether it is working"""
    title = None
    index =read(os.path.join(HOME, ".codex", "session_index.jsonl")) or b""
    for line in index.split(b"\n"):
        if thread.encode() not in line:
            continue
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get("id") == thread and d.get("thread_name"):
            title = d["thread_name"]
    if not title:
        for line in head_lines(rollout):
            try:
                d = json.loads(line)
            except ValueError:
                continue
            payload = d.get("payload") or {}
            if (d.get("type") == "response_item" and payload.get("type") == "message"
                    and payload.get("role") == "user"):
                text = text_of(payload.get("content"))
                if genuine(text, CODEX_WRAPPERS):
                    title = text
                    break
    state, last = "idle", None
    for line in lines:
        if b'"event_msg"' not in line:
            continue
        try:
            d = json.loads(line)
        except ValueError:
            continue
        payload = d.get("payload") or {}
        kind = payload.get("type")
        if kind == "task_started":
            state = "working"
        elif kind in ("task_complete", "turn_aborted"):
            state = "idle"
            last = payload.get("last_agent_message") or last
        elif kind == "agent_message" and payload.get("message"):
            last = payload["message"]
    return {"title": one_line(title, TITLE_CHARS), "state": state,
            "last": one_line(last, SNIPPET_CHARS), "updated": mtime(rollout)}


UUID = re.compile(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")


def started_at(pid):
    """When a process started, in seconds since the epoch"""
    try:
        boot = next(int(l.split()[1]) for l in open("/proc/stat") if l.startswith("btime "))
        stat = read(f"/proc/{pid}/stat") or b""
        ticks = int(stat[stat.rfind(b")") + 2:].split()[19])
        return boot + ticks / os.sysconf("SC_CLK_TCK")
    except (OSError, ValueError, StopIteration, IndexError):
        return 0


# Conversations already given to an agent, so that two agents started in the
# same directory are not given the same guess
CLAIMED = set()


def codex_prompt(argv):
    """The prompt a Codex command line starts with, if any"""
    argv = [a for a in argv if a]
    if len(argv) >= 2 and not argv[-1].startswith("-") and "/" not in argv[-1].split()[0]:
        name = os.path.basename(argv[-1])
        if name not in ("codex", "codex.js"):
            return argv[-1]
    return None


def codex_first_user(path):
    for line in head_lines(path):
        try:
            d = json.loads(line)
        except ValueError:
            continue
        payload = d.get("payload") or {}
        if (d.get("type") == "response_item" and payload.get("type") == "message"
                and payload.get("role") == "user"):
            text = text_of(payload.get("content"))
            if genuine(text, CODEX_WRAPPERS):
                return text
    return None


def codex_candidates(directory, since):
    """The unclaimed Codex conversations begun in a directory since a time,
    newest first, as (modified, id, path)"""
    found = []
    for path in glob.glob(os.path.join(HOME, ".codex", "sessions", "*", "*", "*", "rollout-*.jsonl")):
        try:
            modified = os.path.getmtime(path)
        except OSError:
            continue
        if modified < since:
            continue
        try:
            with open(path, "rb") as f:
                meta = (json.loads(f.readline()).get("payload") or {})
        except (OSError, ValueError):
            continue
        if (meta.get("cwd") == directory and meta.get("id") and meta["id"] not in CLAIMED
                and meta.get("thread_source") not in ("subagent",)):
            found.append((modified, meta["id"], path))
    return sorted(found, reverse=True)


WORD = re.compile(r"[a-z0-9]+")
# Runs of this many words, of a conversation's messages and of a screen,
# that match tell which conversation a screen shows
SHINGLE = 6
SHOWN_MESSAGES = 8


def shingles(text):
    words = WORD.findall(text.lower())
    return {tuple(words[i:i + SHINGLE]) for i in range(len(words) - SHINGLE + 1)}


def codex_recent_messages(path):
    """The last messages of a Codex conversation, the user's and Codex's"""
    texts = []
    for line in tail_lines(path):
        try:
            d = json.loads(line)
        except ValueError:
            continue
        payload = d.get("payload") or {}
        if d.get("type") == "event_msg" and payload.get("type") == "agent_message":
            texts.append(payload.get("message") or "")
        elif (d.get("type") == "response_item" and payload.get("type") == "message"
                and payload.get("role") == "user"):
            text = text_of(payload.get("content"))
            if genuine(text, CODEX_WRAPPERS):
                texts.append(text)
    return texts[-SHOWN_MESSAGES:]


def codex_guess(directory, since, prompt=None, screen=None, fallback=True):
    """The conversation of a Codex process that holds no lock, as Codex's
    shared app server holds them all: among those begun where it started
    since it did, the one it was started with a prompt for, else the one
    whose latest messages are on its screen, else, with fallback, the newest"""
    candidates = codex_candidates(directory, since)
    if prompt:
        wanted = " ".join(prompt.split())[:200]
        for _modified, thread, path in candidates:
            if " ".join((codex_first_user(path) or "").split()).startswith(wanted):
                return thread
    if screen:
        shown = shingles(screen)
        scored = []
        for modified, thread, path in candidates:
            mine = set().union(*map(shingles, codex_recent_messages(path)), set())
            score = len(mine & shown)
            if score:
                scored.append((score, modified, thread))
        if scored:
            return max(scored)[2]
    if fallback and candidates:
        return candidates[0][1]
    return None


def known_agents(processes):
    """The agents among processes whose conversations are certain"""
    found = []
    for pid, _depth in processes:
        agent = claude_agent(pid) or codex_agent(pid, guess=False)
        if agent:
            CLAIMED.add(agent["conversation"])
            found.append(agent)
    return found


def related(pid, others, ppid):
    """Whether pid is above or below one of others in the process tree"""
    def ancestors(p):
        seen = set()
        while p in ppid and p not in seen and p > 1:
            seen.add(p)
            p = ppid[p]
            yield p
    return any(a in others for a in ancestors(pid)) or any(
        pid in set(ancestors(o)) for o in others)


def agents(processes, ppid, known=None, screen=None, guess=True):
    """The coding agents among a tab's processes. A Codex process whose
    conversation is only guessed is skipped when an agent process is above or
    below it: one that knows its conversation, or the same agent's wrapper.
    screen is what the tab shows, and guess as for codex_agent"""
    found = known_agents(processes) if known is None else list(known)
    pids = {a["pid"] for a in found}
    for pid, _depth in processes:
        if pid in pids or related(pid, pids, ppid):
            continue
        agent = codex_agent(pid, guess, screen)
        if agent:
            CLAIMED.add(agent["conversation"])
            pids.add(pid)
            found.append(agent)
    unique, seen = [], set()
    for agent in found:
        if agent["conversation"] not in seen:
            seen.add(agent["conversation"])
            unique.append(agent)
    return unique


SHELLS = ("sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "nu", "xonsh")
SUPERVISOR = "ct-supervise"
SCREEN_LINES = 3


def zellij_servers():
    """The running Zellij servers, by session name"""
    found = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        argv = (read(f"/proc/{entry}/cmdline") or b"").split(b"\0")
        if b"--server" in argv and argv.index(b"--server") + 1 < len(argv):
            socket = argv[argv.index(b"--server") + 1].decode(errors="replace")
            found[os.path.basename(socket)] = int(entry)
    return found


def argv_of(pid):
    return [a.decode(errors="replace") for a in (read(f"/proc/{pid}/cmdline") or b"").split(b"\0") if a]


def foreground(processes):
    """The command a terminal is running, unless it is just at its shell"""
    pid, _depth = max(processes, key=lambda p: p[1])
    argv = argv_of(pid)
    if not argv:
        return None, pid
    name = os.path.basename(argv[0]).lstrip("-")
    if name in SHELLS or name.startswith(SUPERVISOR) or name == "zellij":
        return None, pid
    return one_line(" ".join(argv), SNIPPET_CHARS), pid


def zellij_output(session, *args):
    try:
        result = subprocess.run(["zellij", "--session", session, "action", *args],
                                capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.SubprocessError):
        return ""
    return result.stdout if result.returncode == 0 else ""


def screen_text(session):
    return zellij_output(session, "dump-screen", "-p", "terminal_0")


def last_lines(text):
    """The last non-blank lines of a screen"""
    lines = [l.rstrip() for l in text.splitlines()]
    return [one_line(l, 160) for l in lines if l.strip()][-SCREEN_LINES:]


def repository_of(path):
    """The main worktree of the repository holding path, and its branch"""
    try:
        common = subprocess.run(
            ["git", "-C", path, "rev-parse", "--path-format=absolute", "--git-common-dir"],
            capture_output=True, text=True, timeout=5)
        branch = subprocess.run(["git", "-C", path, "symbolic-ref", "--short", "-q", "HEAD"],
                                capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.SubprocessError):
        return None, None
    if common.returncode != 0:
        return None, None
    common = common.stdout.strip()
    repo = os.path.dirname(common) if os.path.basename(common) == ".git" else common
    return repo, branch.stdout.strip() or None


THREADS = os.path.join(os.environ.get("XDG_STATE_HOME") or os.path.join(HOME, ".local", "state"),
                       "cosmic-term", "threads")
THREAD_GC_AGE = 24 * 3600


def relay_live(directory):
    """Whether a relay holds the thread directory's lock"""
    import fcntl
    try:
        fd = os.open(os.path.join(directory, "lock"), os.O_RDONLY)
    except OSError:
        return False
    try:
        fcntl.flock(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
        fcntl.flock(fd, fcntl.LOCK_UN)
        return False
    except OSError:
        return True
    finally:
        os.close(fd)


def read_json(path):
    try:
        value = json.loads(read(path) or b"null")
    except ValueError:
        return {}
    return value if isinstance(value, dict) else {}


def thread_rows():
    """The live relays of rich-text threads (see relay.rs), as session rows
    with a "thread" object; and the removal of those gone a day"""
    import shutil
    import time
    rows = []
    try:
        names = sorted(os.listdir(THREADS))
    except OSError:
        return rows
    for name in names:
        directory = os.path.join(THREADS, name)
        if not name.startswith("ct-thread-") or not os.path.isdir(directory):
            continue
        if not relay_live(directory):
            if time.time() - max(mtime(os.path.join(directory, "state")), mtime(directory)) > THREAD_GC_AGE:
                shutil.rmtree(directory, ignore_errors=True)
            continue
        meta = read_json(os.path.join(directory, "meta.json"))
        state = read_json(os.path.join(directory, "state"))
        kind = meta.get("agent") or "claude"
        conversation = meta.get("conversation")
        where = os.path.expanduser(meta.get("cwd") or "~")
        summary = {}
        if conversation and kind == "claude":
            transcripts = glob.glob(os.path.join(HOME, ".claude", "projects", "*", f"{conversation}.jsonl"))
            if transcripts:
                summary = claude_summary(transcripts[0], tail_lines(transcripts[0]))
        elif conversation and kind == "codex":
            rollouts = glob.glob(os.path.join(HOME, ".codex", "sessions", "*", "*", "*", f"rollout-*-{conversation}.jsonl"))
            if rollouts:
                summary = codex_summary(rollouts[0], conversation, tail_lines(rollouts[0]))
        busy = bool(state.get("busy"))
        agent = {"kind": kind, "pid": state.get("cli") or 0, "conversation": conversation,
                 "started_in": where, "cwd": where, "focus": [], "projects": [], "status": "running",
                 **summary, "state": "working" if busy else "idle"}
        repository, branch = repository_of(where) if os.path.isdir(where) else (None, None)
        rows.append({"name": name, "clients": state.get("clients") or 0, "cwd": where,
                     "project": project_root(where) if os.path.isdir(where) else None,
                     "repository": repository, "branch": branch, "command": None,
                     "since": meta.get("created"), "screen": [], "agents": [agent],
                     "thread": {"agent": kind, "busy": busy, "conversation": conversation,
                                "meta": meta, "turn_start": state.get("turn_start") or 0,
                                "end": state.get("end") or 0, "created": meta.get("created") or 0}})
    claims = os.path.join(THREADS, "by-conversation")
    try:
        for claim in os.listdir(claims):
            path = os.path.join(claims, claim)
            if os.path.islink(path) and not os.path.exists(path):
                os.unlink(path)
    except OSError:
        pass
    return rows


def sessions():
    ppid = parents()
    found = thread_rows()
    # The newest first, since an agent's guessed conversation is the newest
    # one begun where it started that no newer agent has. A thread's session
    # runs its relay, which is described above
    servers = sorted(
        ((name, server) for name, server in zellij_servers().items()
         if name.startswith("ct-") and not name.startswith("ct-thread-")),
        key=lambda s: -started_at(s[1]))
    trees = {}
    # Conversations that are certain are claimed before any is guessed
    for name, server in servers:
        roots = [pid for pid, parent in ppid.items() if parent == server]
        processes = [p for root in roots for p in descendants(root, ppid)]
        trees[name] = (processes, known_agents(processes))
    # Then those a session shows on its screen, before any is guessed
    screens = {name: screen_text(name) for name, _server in servers}
    for name, _server in servers:
        processes, known = trees[name]
        trees[name] = (processes, agents(processes, ppid, known, screens[name], guess="screen"))
    for name, server in servers:
        processes, known = trees[name]
        running = agents(processes, ppid, known, screens[name])
        deepest_cwd = None
        command = since = None
        lines = []
        if processes:
            command, pid = foreground(processes)
            deepest_cwd = cwd(pid)
            since = started_at(pid) if command else None
        if not running:
            lines = last_lines(screens[name])
        where = None
        if running:
            agent = running[0]
            repositories = [f for f in agent["focus"] if os.path.exists(os.path.join(f, ".git"))]
            where = (repositories or [None])[0] or agent.get("cwd") or agent.get("started_in")
        where = where or deepest_cwd
        project = project_root(where) if where else None
        repository, branch = repository_of(where) if where and os.path.isdir(where) else (None, None)
        clients = zellij_output(name, "list-clients").splitlines()[1:]
        found.append({"name": name, "clients": len([c for c in clients if c.strip()]),
                      "cwd": where, "project": project, "repository": repository,
                      "branch": branch, "command": command, "since": since,
                      "screen": lines, "agents": running})
    print(json.dumps(sorted(found, key=lambda s: s["name"])))


def main():
    if sys.argv[1] == "sessions":
        sessions()
        return
    kind, value = sys.argv[1], sys.argv[2]
    ppid = parents()
    if kind == "env":
        marker = f"COSMIC_TERM_TAB={value}".encode()
        tagged = {
            pid
            for pid in ppid
            if marker in (read(f"/proc/{pid}/environ") or b"").split(b"\0")
        }
        # Descendants inherit the variable, so the roots are the tagged
        # processes whose parent is not
        roots = sorted(pid for pid in tagged if ppid[pid] not in tagged)
        if not roots:
            sys.exit(f"no process for tab {value}")
    elif kind == "pid":
        roots = [int(value)]
    else:
        server = zellij_server(value)
        if server is None:
            sys.exit(f"no Zellij session {value}")
        roots = [pid for pid, parent in ppid.items() if parent == server]

    processes = [p for root in roots for p in descendants(root, ppid)]
    running = agents(processes, ppid, screen=screen_text(value) if kind == "zellij" else None)
    deepest = max(processes, key=lambda p: p[1], default=None)
    deepest_cwd = cwd(deepest[0]) if deepest else None

    if "--json" in sys.argv[3:]:
        print(json.dumps({"cwd": deepest_cwd, "agents": running}))
        return

    # An agent's leading projects, then the directories it was given with
    # --add-dir or /add-dir; where it merely started, such as ~, is left out
    dirs = []
    for agent in running:
        dirs.extend(agent["projects"])
    # Directories the user added are kept whatever they are
    for pid, _depth in processes:
        found = claude_dirs(pid)
        if found:
            dirs.extend(found[1:])
    if not dirs:
        dirs = [root for root in (project_root(a["cwd"]) for a in running if a.get("cwd")) if root]
    if not dirs and deepest_cwd:
        dirs.append(deepest_cwd)

    if "--cwd" in sys.argv[3:]:
        current = next((a["cwd"] for a in running if a.get("cwd")), None) or (dirs[0] if dirs else None)
        if current:
            print(current)
        return

    seen = set()
    for d in dirs:
        root = git_root(d)
        if os.path.isdir(root) and root not in seen:
            seen.add(root)
            print(root)

main()
