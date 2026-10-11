#!/usr/bin/env python3
"""Deductum session host.

One host per user per machine owns Deductum's shared Isabelle sessions there
(design C2/C3/C11 in .local-design/isabelle-app.md, docs/deductum-decisions.md):
the authoritative unsaved text of each theory, its author-attributed edit log,
undo and revert (C5/C7), editing grants (C6/C9), conflict notices for agents (C7),
autosave and external changes (C13), independent agent sessions (C8), and the
prover process for each session (``isabelle deductum_backend``, C12).

Clients (the Deductum app, agents' MCP adapters, the command line) talk JSON lines
over a Unix socket. Remote clients reach it through ``ssh MACHINE python3
.../deductum_host.py connect``, which bridges stdio to the socket and starts the
host if it isn't running. The host outlives its clients: closing the app detaches,
and only an explicit Stop ends a prover.

Standard library only; Python 3.9 or newer.
"""

from __future__ import annotations

import asyncio
import contextlib
import difflib
import errno
import fcntl
import hashlib
import json
import os
import re
import shutil
import signal
import shlex
import socket
import subprocess
import sys
import time
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

sys.path.insert(0, str(Path(__file__).resolve().parent))
import deductum_ot as ot  # noqa: E402

PROTOCOL = 1
VERSION = "0.1.0"
HOME = Path.home()
STATE_DIR = Path(os.environ.get("DEDUCTUM_STATE_DIR")
                 or Path(os.environ.get("XDG_STATE_HOME") or HOME / ".local/state") / "deductum")
SOCKET_PATH = STATE_DIR / "host.sock"
LOCK_PATH = STATE_DIR / "host.lock"
LOG_PATH = STATE_DIR / "host.log"
SHARE_DIR = Path(__file__).resolve().parent.parent
CONFIG_PATH = Path(os.environ.get("XDG_CONFIG_HOME") or HOME / ".config") / "deductum" / "host.json"

AUTOSAVE_DELAY = 2.0
WATCH_INTERVAL = 1.0
MEMORY_ENTRIES = 5000
LIFECYCLES = ("Starting", "Running", "Paused", "Stopping", "Stopped", "Failed")


def log(*args: Any) -> None:
    print(time.strftime("%H:%M:%S"), *args, file=sys.stderr, flush=True)


def now() -> float:
    return time.time()


def write_json_atomic(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name("." + path.name + ".tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False)
    os.replace(tmp, path)


def read_json(path: Path, default: Any = None) -> Any:
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def file_key(path: str) -> str:
    return hashlib.sha1(path.encode()).hexdigest()[:16]


def stat_sig(path: str) -> Optional[Tuple[int, int]]:
    try:
        st = os.stat(path)
        return (st.st_mtime_ns, st.st_size)
    except OSError:
        return None


class HostError(Exception):
    def __init__(self, message: str, code: str = "error", **data: Any):
        super().__init__(message)
        self.code = code
        self.data = data


# ---------------------------------------------------------------------------
# Files on disk


def decode_file(raw: bytes) -> Tuple[str, bool, bool]:
    """Text with LF line ends, whether it had CRLF line ends, whether it had a BOM"""
    bom = raw.startswith(b"\xef\xbb\xbf")
    if bom:
        raw = raw[3:]
    text = raw.decode("utf-8", errors="surrogateescape")
    crlf = "\r\n" in text
    return text.replace("\r\n", "\n"), crlf, bom


def encode_file(text: str, crlf: bool, bom: bool) -> bytes:
    if crlf:
        text = text.replace("\n", "\r\n")
    raw = text.encode("utf-8", errors="surrogateescape")
    return (b"\xef\xbb\xbf" + raw) if bom else raw


def read_file(path: str) -> Optional[Tuple[str, bool, bool]]:
    try:
        with open(path, "rb") as f:
            return decode_file(f.read())
    except OSError:
        return None


def write_file_atomic(path: str, data: bytes) -> None:
    """Write via a temporary file and rename, keeping the file's permissions"""
    target = Path(path)
    tmp = target.with_name("." + target.name + ".deductum-tmp")
    mode = None
    with contextlib.suppress(OSError):
        mode = os.stat(path).st_mode & 0o7777
    with open(tmp, "wb") as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    if mode is not None:
        os.chmod(tmp, mode)
    os.replace(tmp, target)


# ---------------------------------------------------------------------------
# Isabelle runtimes and session ROOTs (design C12)


def runtime_identifier(isabelle: Path) -> Optional[str]:
    home = isabelle.resolve().parent.parent
    try:
        return (home / "etc" / "ISABELLE_IDENTIFIER").read_text().strip() or None
    except OSError:
        return None


def find_runtimes() -> List[Dict[str, Any]]:
    candidates: List[Path] = []
    config = read_json(CONFIG_PATH, {}) or {}
    for p in config.get("runtimes", []):
        candidates.append(Path(os.path.expanduser(p)))
    if os.environ.get("DEDUCTUM_ISABELLE"):
        candidates.append(Path(os.environ["DEDUCTUM_ISABELLE"]))
    for pattern in ("Documents/Software/Isabelle*/bin/isabelle", "Software/Isabelle*/bin/isabelle",
                    "Isabelle*/bin/isabelle", "opt/Isabelle*/bin/isabelle"):
        candidates.extend(sorted(HOME.glob(pattern)))
    for root in ("/opt", "/usr/local"):
        candidates.extend(sorted(Path(root).glob("Isabelle*/bin/isabelle")))
    which = shutil.which("isabelle")
    if which:
        candidates.append(Path(which))
    seen = set()
    out = []
    for c in candidates:
        try:
            real = c.resolve()
        except OSError:
            continue
        if real in seen or not real.is_file():
            continue
        seen.add(real)
        ident = runtime_identifier(real)
        out.append({
            "path": str(real),
            "identifier": ident,
            "home": str(real.parent.parent),
            "supported": bool(ident) and (SHARE_DIR / "backend" / ident).is_dir(),
        })
    out.sort(key=lambda r: (not r["supported"], r["path"]))
    return out


SESSION_RE = re.compile(
    r'session\s+("[^"]+"|[\w\-.+]+)\s*(?:\([^)]*\))?\s*(?:in\s+("[^"]+"|[^\s=]+))?\s*='
    r'\s*("[^"]+"|[\w\-.+]+)?', re.S)


def parse_root(path: Path) -> List[Dict[str, str]]:
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return []
    text = re.sub(r"\(\*.*?\*\)", " ", text, flags=re.S)
    out = []
    for m in SESSION_RE.finditer(text):
        name = m.group(1).strip('"')
        sub = (m.group(2) or ".").strip('"')
        parent = (m.group(3) or "").strip('"')
        out.append({"name": name, "dir": str((path.parent / sub).resolve()), "parent": parent,
                    "root": str(path)})
    return out


_known_sessions: Dict[str, List[str]] = {}


def known_sessions(runtime: Optional[str]) -> List[str]:
    """The sessions the runtime knows without extra directories (`isabelle sessions -a`)"""
    if not runtime:
        return []
    if runtime not in _known_sessions:
        try:
            r = subprocess.run([runtime, "sessions", "-a"], capture_output=True, text=True, timeout=300)
            _known_sessions[runtime] = [ln.strip() for ln in r.stdout.splitlines() if ln.strip()]
        except (OSError, subprocess.TimeoutExpired):
            _known_sessions[runtime] = []
    return _known_sessions[runtime]


def suggest_logic(folder: str, runtime: Optional[str] = None) -> Dict[str, Any]:
    """The base logic and session directories for a theory folder.

    Normally the parent of the session whose directory contains the folder (its
    own heap would hold its theories as finished), with that session's ROOT
    directory included. When the runtime already defines a session of that name
    (an installed copy, as CyPhyAssure installs UTP2), adding the directory again
    would clash: the folder's theories then open as drafts on the parent logic."""
    known = known_sessions(runtime)

    def resolve(name: str) -> Optional[str]:
        if not known or name in known:
            return name
        for alt in (name.replace("-", "_"), name.replace("_", "-")):
            if alt in known:
                return alt
        return None

    f = Path(folder).resolve()
    for d in [f] + list(f.parents):
        root = d / "ROOT"
        if root.is_file():
            sessions = parse_root(root)
            best = None
            for s in sessions:
                sd = Path(s["dir"])
                if f == sd or sd in f.parents or (best is None and sd == d):
                    if best is None or len(s["dir"]) > len(best["dir"]):
                        best = s
            if best is None and sessions:
                best = sessions[0]
            if best:
                parent = resolve(best["parent"] or "HOL") or "HOL"
                if known and best["name"] in known:
                    return {"logic": parent, "dirs": [], "include_sessions": [], "session": best["name"],
                            "sessions": sessions,
                            "note": f"Session {best['name']} is also installed in this Isabelle, so this "
                                    f"folder's theories open as drafts on {parent}."}
                note = None
                if best["parent"] and parent != best["parent"]:
                    note = f"The parent session {best['parent']} is installed as {parent}."
                if best["parent"] and resolve(best["parent"]) is None:
                    note = (f"The parent session {best['parent']} is not installed; HOL is used instead. "
                            "Set the base logic if it should be built.")
                return {"logic": parent, "dirs": [str(d)], "include_sessions": [best["name"]],
                        "session": best["name"], "sessions": sessions, "note": note}
        if d == HOME or d == d.parent:
            break
    return {"logic": "HOL", "dirs": [], "include_sessions": [], "session": None, "sessions": [], "note": None}


def list_theories(folder: str, limit: int = 3000) -> List[str]:
    out: List[str] = []
    base = Path(folder)
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if not d.startswith(".")
                             and d not in ("output", "__pycache__", "node_modules"))
        depth = len(Path(dirpath).relative_to(base).parts)
        if depth > 6:
            dirnames[:] = []
        for name in sorted(filenames):
            if name.endswith(".thy") or name == "ROOT":
                out.append(os.path.join(dirpath, name))
                if len(out) >= limit:
                    return out
    return out


_component_ready: Dict[str, str] = {}  # runtime -> its ISABELLE_HOME_USER, once registered
_symbol_tables: Dict[str, List[Dict[str, Any]]] = {}


def symbol_table(runtime: str) -> List[Dict[str, Any]]:
    """The runtime's Isabelle symbols, from the files ISABELLE_SYMBOLS lists"""
    if runtime in _symbol_tables:
        return _symbol_tables[runtime]
    r = subprocess.run([runtime, "getenv", "-b", "ISABELLE_SYMBOLS"], capture_output=True, text=True,
                       timeout=120)
    entries: Dict[str, Dict[str, Any]] = {}
    for path in r.stdout.strip().split(":"):
        try:
            text = Path(path).read_text(errors="replace")
        except OSError:
            continue
        for line in text.splitlines():
            line = line.split("#", 1)[0].strip() if not line.lstrip().startswith("\\<") else line.strip()
            if not line.startswith("\\<"):
                continue
            parts = line.split()
            sym = parts[0]
            e = entries.setdefault(sym, {"symbol": sym, "code": 0, "abbrevs": [], "groups": []})
            i = 1
            while i + 1 < len(parts):
                key, val = parts[i].rstrip(":"), parts[i + 1]
                if key == "code":
                    try:
                        e["code"] = int(val, 16) if val.startswith("0x") else int(val)
                    except ValueError:
                        pass
                elif key == "abbrev":
                    e["abbrevs"].append(val)
                elif key == "group":
                    e["groups"].append(val)
                i += 2
    _symbol_tables[runtime] = list(entries.values())
    return _symbol_tables[runtime]


def ensure_component(runtime: str, progress) -> None:
    """Register the backend component with the runtime and build its jar"""
    ident = runtime_identifier(Path(runtime))
    comp = SHARE_DIR / "backend" / (ident or "")
    if not ident or not comp.is_dir():
        raise HostError(f"No Deductum backend for {ident or runtime}", "unsupported_runtime")
    env = dict(os.environ)
    home_user = _component_ready.get(runtime) or subprocess.run(
        [runtime, "getenv", "-b", "ISABELLE_HOME_USER"], capture_output=True, text=True, timeout=120,
        env=env).stdout.strip()
    registry = Path(home_user) / "etc" / "components"
    lines = []
    with contextlib.suppress(OSError):
        lines = [ln.strip() for ln in registry.read_text().splitlines()]
    # (ready before, and still registered: another host, a test, may have registered its own since)
    if _component_ready.get(runtime) and str(comp) in lines:
        return
    # Other copies of the backend (an old install, a test) would provide the same tool
    for stale in [ln for ln in lines if ln != str(comp) and re.search(r"deductum[^/]*/backend/[^/]+/?$", ln)]:
        progress(f"Unregistering a stale Deductum backend: {stale}")
        subprocess.run([runtime, "components", "-x", stale], capture_output=True, text=True, timeout=300, env=env)
    if str(comp) not in lines:
        progress(f"Registering the Deductum backend with {ident}")
        r = subprocess.run([runtime, "components", "-u", str(comp)], capture_output=True,
                           text=True, timeout=300, env=env)
        if r.returncode != 0:
            raise HostError("Could not register backend: " + r.stderr.strip())
    jar = comp / "lib" / "deductum_backend.jar"
    sources = list((comp / "src").glob("*.scala")) + [comp / "etc" / "build.props"]
    if not jar.is_file() or any(s.stat().st_mtime > jar.stat().st_mtime for s in sources):
        progress("Building the Deductum backend")
        r = subprocess.run([runtime, "scala_build"], capture_output=True, text=True, timeout=1800,
                           env=env)
        if r.returncode != 0 or not jar.is_file():
            raise HostError("Backend build failed: " + (r.stdout + r.stderr)[-2000:])
    _component_ready[runtime] = home_user


# ---------------------------------------------------------------------------
# Documents (design C5, C7, C11, C13)


class Doc:
    def __init__(self, session: "Session", path: str):
        self.session = session
        self.path = path
        self.key = file_key(path)
        self.text = ""
        self.revision = 0
        self.disk_text: Optional[str] = None
        self.disk_sig: Optional[Tuple[int, int]] = None
        self.crlf = False
        self.bom = False
        self.entries: List[Dict[str, Any]] = []
        self.next_entry = 1
        self.conflict: Optional[Dict[str, Any]] = None
        self.opened = False
        self.prover: Dict[str, Any] = {}
        self.trackers: List[Dict[str, Any]] = []
        self.undone: Dict[int, int] = {}  # entry id -> id of the entry that undid it
        self.undo_stack: List[str] = []  # human transaction ids, oldest first
        self.redo_stack: List[str] = []
        self.alternatives: List[Dict[str, Any]] = []
        self.save_handle: Optional[asyncio.TimerHandle] = None
        self.save_error: Optional[str] = None
        self.last_edit = 0.0
        self.saved_revision = 0

    # persistence

    def state_path(self) -> Path:
        return self.session.dir / "docs" / (self.key + ".json")

    def log_path(self) -> Path:
        return self.session.dir / "logs" / (self.key + ".jsonl")

    def to_json(self) -> Dict[str, Any]:
        return {
            "path": self.path, "text": self.text, "revision": self.revision,
            "disk_text": self.disk_text, "disk_sig": self.disk_sig, "crlf": self.crlf, "bom": self.bom,
            "entries": self.entries[-500:], "next_entry": self.next_entry, "conflict": self.conflict,
            "opened": self.opened, "trackers": self.trackers[-500:],
            "undone": {str(k): v for k, v in self.undone.items()},
            "undo_stack": self.undo_stack[-500:], "redo_stack": self.redo_stack[-500:],
            "alternatives": self.alternatives[-100:], "saved_revision": self.saved_revision,
        }

    @classmethod
    def from_json(cls, session: "Session", data: Dict[str, Any]) -> "Doc":
        d = cls(session, data["path"])
        d.text = data.get("text", "")
        d.revision = data.get("revision", 0)
        d.disk_text = data.get("disk_text")
        sig = data.get("disk_sig")
        d.disk_sig = tuple(sig) if sig else None
        d.crlf = data.get("crlf", False)
        d.bom = data.get("bom", False)
        d.entries = data.get("entries", [])
        d.next_entry = data.get("next_entry", len(d.entries) + 1)
        d.conflict = data.get("conflict")
        d.opened = data.get("opened", False)
        d.trackers = data.get("trackers", [])
        d.undone = {int(k): v for k, v in data.get("undone", {}).items()}
        d.undo_stack = data.get("undo_stack", [])
        d.redo_stack = data.get("redo_stack", [])
        # Undo no longer leaves pieces to restore (it keeps later edits instead)
        d.alternatives = [a for a in data.get("alternatives", []) if a.get("kind") not in ("undo", "redo", "revert")]
        d.saved_revision = data.get("saved_revision", 0)
        return d

    def load_from_disk(self) -> None:
        got = read_file(self.path)
        if got is None:
            raise HostError(f"Cannot read {self.path}", "not_found")
        self.text, self.crlf, self.bom = got
        self.disk_text = self.text
        self.disk_sig = stat_sig(self.path)
        self.saved_revision = self.revision

    @property
    def dirty(self) -> bool:
        return self.disk_text is None or self.text != self.disk_text

    def summary(self) -> Dict[str, Any]:
        return {
            "path": self.path, "revision": self.revision, "dirty": self.dirty,
            "conflict": self.conflict_summary(), "opened": self.opened,
            "status": self.prover.get("status"), "save_error": self.save_error,
        }

    def conflict_summary(self) -> Optional[Dict[str, Any]]:
        if not self.conflict:
            return None
        return {"since": self.conflict.get("since"), "deleted": self.conflict.get("disk_text") is None}

    # history

    def entries_after(self, revision: int) -> List[Dict[str, Any]]:
        if revision >= self.revision:
            return []
        if not self.entries or self.entries[0]["rev"] > revision + 1:
            raise HostError("Base revision is too old; read the theory again", "stale_base",
                            revision=self.revision)
        return [e for e in self.entries if e["rev"] > revision]

    def entry(self, entry_id: int) -> Dict[str, Any]:
        for e in self.entries:
            if e["id"] == entry_id:
                return e
        raise HostError(f"No change {entry_id} in recent history", "not_found")

    def text_length_at(self, revision: int) -> int:
        n = len(self.text)
        for e in reversed(self.entries_after(revision)):
            n -= sum(len(t) - (b - a) for a, b, t in e["change"])
        return n

    def map_offset(self, offset: int, revision: Optional[int], right: bool = False) -> int:
        if revision is None or revision >= self.revision:
            return max(0, min(offset, len(self.text)))
        pos = offset
        for e in self.entries_after(revision):
            pos = ot.map_pos(pos, [tuple(r) for r in e["change"]], right=right)
        return max(0, min(pos, len(self.text)))


# ---------------------------------------------------------------------------
# Backend process (design C12)


class Backend:
    def __init__(self, session: "Session"):
        self.session = session
        self.proc: Optional[asyncio.subprocess.Process] = None
        self.next_id = 1
        self.pending: Dict[int, asyncio.Future] = {}
        self.ready = False
        self.reader_task: Optional[asyncio.Task] = None
        self.stderr_task: Optional[asyncio.Task] = None

    async def start(self) -> None:
        s = self.session
        cmd = [s.meta["runtime"], "deductum_backend", "-l", s.meta["logic"]]
        for d in s.meta.get("dirs", []):
            cmd += ["-d", d]
        for name in s.meta.get("include_sessions", []):
            cmd += ["-i", name]
        for opt in s.meta.get("options", []):
            cmd += ["-o", opt]
        log("starting backend", s.id, " ".join(cmd))
        self.proc = await asyncio.create_subprocess_exec(
            *cmd, stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE, start_new_session=True, limit=64 * 1024 * 1024)
        self.reader_task = asyncio.create_task(self.read_loop())
        self.stderr_task = asyncio.create_task(self.stderr_loop())

    async def read_loop(self) -> None:
        assert self.proc and self.proc.stdout
        try:
            while True:
                line = await self.proc.stdout.readline()
                if not line:
                    break
                try:
                    msg = json.loads(line)
                except ValueError:
                    continue
                if "reply" in msg:
                    fut = self.pending.pop(msg["reply"], None)
                    if fut and not fut.done():
                        fut.set_result(msg)
                else:
                    try:
                        self.session.on_backend_event(msg)
                    except Exception as exn:  # keep reading whatever an event does
                        log("backend event error", repr(exn))
        finally:
            code = await self.proc.wait()
            for fut in self.pending.values():
                if not fut.done():
                    fut.set_exception(HostError("Prover stopped", "stopped"))
            self.pending.clear()
            self.session.on_backend_exit(self, code)

    async def stderr_loop(self) -> None:
        assert self.proc and self.proc.stderr
        tail: List[str] = []
        while True:
            line = await self.proc.stderr.readline()
            if not line:
                break
            text = line.decode(errors="replace").rstrip()
            tail = (tail + [text])[-40:]
            self.session.stderr_tail = tail

    def send(self, obj: Dict[str, Any]) -> None:
        if not self.proc or not self.proc.stdin or self.proc.returncode is not None:
            raise HostError("Prover is not running", "stopped")
        self.proc.stdin.write((json.dumps(obj, ensure_ascii=False) + "\n").encode())

    async def request(self, obj: Dict[str, Any], timeout: float = 120.0) -> Dict[str, Any]:
        rid = self.next_id
        self.next_id += 1
        fut = asyncio.get_running_loop().create_future()
        self.pending[rid] = fut
        self.send(dict(obj, id=rid))
        try:
            return await asyncio.wait_for(fut, timeout)
        finally:
            self.pending.pop(rid, None)

    async def stop(self) -> None:
        if not self.proc or self.proc.returncode is not None:
            return
        with contextlib.suppress(Exception):
            assert self.proc.stdin
            self.proc.stdin.close()
        try:
            await asyncio.wait_for(self.proc.wait(), 20)
        except asyncio.TimeoutError:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(self.proc.pid, signal.SIGTERM)
            try:
                await asyncio.wait_for(self.proc.wait(), 10)
            except asyncio.TimeoutError:
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(self.proc.pid, signal.SIGKILL)


# ---------------------------------------------------------------------------
# Helpers for authors, paths and line diffs


HUMAN = {"kind": "human", "key": "human", "label": "You"}
DISK = {"kind": "disk", "key": "disk", "label": "Disk"}


KEYWORD_PATTERN = r"^((?:[^\"\n]*?[ \t])?)(claude|codex|@[A-Za-z][\w.\-]*)(?:\[([\w.:\-]*)\])?(?:[ \t]+([^\"\n]*?))?[ \t]*$"
KEYWORD_LINES = re.compile(KEYWORD_PATTERN, re.M)


def prover_text(text: str) -> str:
    """The text as the prover sees it: a line asking `claude` or `codex` for a proof reads as
    `sorry`, padded to the same length (so offsets are the same), and the goal there is the
    goal it asks about, as with `sledgehammer`; the human's text keeps the keyword"""
    if "claude" not in text and "codex" not in text and "@" not in text:
        return text
    def hole(m: "re.Match[str]") -> str:
        rest = m.group(0)[len(m.group(1)):]
        return m.group(1) + "sorry" + " " * (len(rest) - 5)
    return KEYWORD_LINES.sub(hole, text)


def path_inside(path: str, folder: str) -> bool:
    try:
        p = os.path.realpath(path)
        f = os.path.realpath(folder)
        return os.path.commonpath([p, f]) == f
    except ValueError:
        return False


def line_change(old: str, new: str) -> ot.Change:
    """Replacements turning ``old`` into ``new``, one per differing run of lines"""
    a = old.splitlines(keepends=True)
    b = new.splitlines(keepends=True)
    starts = [0]
    for ln in a:
        starts.append(starts[-1] + len(ln))
    out: ot.Change = []
    sm = difflib.SequenceMatcher(None, a, b, autojunk=False)
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag != "equal":
            out.append((starts[i1], starts[i2], "".join(b[j1:j2])))
    return ot.normalize(out, len(old))


def excerpt(text: str, start: int, end: int, context: int = 80) -> str:
    s = max(0, start - context)
    e = min(len(text), max(end, start) + context)
    return text[s:e]


def git(*args: str, cwd: Optional[str] = None) -> str:
    # (a folder of another machine, through its sshfs mount, is owned by another user here: git
    # would refuse it as of "dubious ownership")
    r = subprocess.run(["git", "-c", "safe.directory=*", *args], cwd=cwd, capture_output=True, text=True, timeout=300)
    if r.returncode != 0:
        raise HostError("git " + " ".join(args[:2]) + ": " + r.stderr.strip())
    return r.stdout


# ---------------------------------------------------------------------------
# Sessions


class Session:
    def __init__(self, host: "Host", sid: str, meta: Dict[str, Any]):
        self.host = host
        self.id = sid
        self.meta = meta
        self.dir = STATE_DIR / "sessions" / sid
        self.docs: Dict[str, Doc] = {}
        self.backend: Optional[Backend] = None
        self.stderr_tail: List[str] = []
        self.theory_status: Dict[str, Any] = {}
        self.queries: Dict[str, Dict[str, Any]] = {}
        self.logs: List[str] = []
        self.watch_sigs: Dict[str, Optional[Tuple[int, int]]] = {}
        self.watch_list: List[str] = []
        self.watch_listed = 0.0
        self.watch_listed_once = False
        self.persist_handle: Optional[asyncio.TimerHandle] = None
        self.changed_docs: set = set()
        self.syntax_kinds: Dict[str, str] = {}
        self.status_handle: Optional[asyncio.TimerHandle] = None
        self.start_task: Optional[asyncio.Task] = None
        self.running_since: Dict[Tuple[str, int, str], float] = {}
        self.fill_tasks: Dict[str, asyncio.Task] = {}
        # Lines asking `claude` or `codex` for a proof, by job: where they are, and who asked
        self.keyword_holes: Dict[str, Dict[str, Any]] = {}
        # Persistent helper conversations, one query at a time each
        self.helper_locks: Dict[str, asyncio.Lock] = {}
        # External agents listening for queries (isabelle_listen), by key, and their queries
        self.listeners: Dict[str, Dict[str, Any]] = {}
        self.agent_queries: Dict[str, Dict[str, Any]] = {}
        self.withdrawn: Dict[str, List[str]] = {}
        self.listen_event = asyncio.Event()
        self.cpu_sample: Optional[Tuple[float, float]] = None
        self.last_activity: Optional[Dict[str, Any]] = None
        meta.setdefault("grants", [])
        meta.setdefault("requests", [])
        meta.setdefault("notices", {})
        meta.setdefault("seen", {})
        meta.setdefault("notice_seq", 0)
        meta.setdefault("epoch", 0)

    # ----- persistence

    @property
    def lifecycle(self) -> str:
        return self.meta.get("lifecycle", "Stopped")

    def set_lifecycle(self, lifecycle: str, message: Optional[str] = None) -> None:
        self.meta["lifecycle"] = lifecycle
        self.meta["message"] = message
        self.persist()
        self.host.broadcast_session(self)

    def persist(self, doc: Optional[Doc] = None) -> None:
        if doc is not None:
            self.changed_docs.add(doc.path)
        if self.persist_handle is None:
            loop = asyncio.get_running_loop()
            self.persist_handle = loop.call_later(0.5, self.persist_now)

    def persist_now(self) -> None:
        self.persist_handle = None
        if self.meta.get("discarded"):
            return
        write_json_atomic(self.dir / "meta.json", self.meta)
        for path in list(self.changed_docs):
            doc = self.docs.get(path)
            if doc:
                write_json_atomic(doc.state_path(), doc.to_json())
        self.changed_docs.clear()

    def load_docs(self) -> None:
        for p in sorted((self.dir / "docs").glob("*.json")):
            data = read_json(p)
            if data and "path" in data:
                doc = Doc.from_json(self, data)
                self.docs[doc.path] = doc

    def append_log(self, doc: Doc, entry: Dict[str, Any]) -> None:
        path = doc.log_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, "a", encoding="utf-8") as f:
            f.write(json.dumps(entry, ensure_ascii=False) + "\n")

    # ----- descriptions

    def attached_clients(self) -> List["Client"]:
        return [c for c in self.host.clients if self.id in c.sessions]

    def summary(self) -> Dict[str, Any]:
        running = sum(1 for st in self.theory_status.values() if st and st.get("running"))
        unprocessed = sum(1 for st in self.theory_status.values() if st and st.get("unprocessed"))
        failed = sum(1 for st in self.theory_status.values() if st and st.get("failed"))
        return {
            "id": self.id, "name": self.meta.get("name") or Path(self.meta["folder"]).name,
            "folder": self.meta["folder"], "logic": self.meta["logic"],
            "runtime": self.meta["runtime"], "identifier": self.meta.get("identifier"),
            "dirs": self.meta.get("dirs", []), "include_sessions": self.meta.get("include_sessions", []),
            "lifecycle": self.lifecycle, "message": self.meta.get("message"),
            "epoch": self.meta.get("epoch", 0), "owner": self.meta.get("owner"),
            "parent": {k: v for k, v in (self.meta.get("parent") or {}).items() if k != "base"} or None,
            "created": self.meta.get("created"),
            "activity": {"running": running, "unprocessed": unprocessed, "failed": failed},
            "dirty": sum(1 for d in self.docs.values() if d.dirty),
            "conflicts": sum(1 for d in self.docs.values() if d.conflict),
            "clients": [c.describe(self.id) for c in self.attached_clients()],
            "grants": self.meta["grants"],
            "requests": self.meta.get("requests", []),
            "prover_options": self.meta.get("prover_options", {}),
            "machine": socket.gethostname(),
        }

    # ----- documents

    def get_doc(self, path: str) -> Doc:
        path = os.path.realpath(os.path.expanduser(path))
        doc = self.docs.get(path)
        if doc is None:
            if not os.path.isfile(path):
                raise HostError(f"No such theory: {path}", "not_found")
            doc = Doc(self, path)
            doc.load_from_disk()
            self.docs[path] = doc
            self.persist(doc)
        if not doc.opened:
            doc.opened = True
            self.persist(doc)
        if self.backend and self.backend.ready:
            if not getattr(doc, "_sent_open", False):
                self.backend_open(doc)
        return doc

    def backend_open(self, doc: Doc) -> None:
        if self.backend and self.backend.ready:
            with contextlib.suppress(HostError):
                doc._prover_text = prover_text(doc.text)  # type: ignore[attr-defined]
                self.backend.send({"op": "open", "file": doc.path, "text": doc._prover_text,  # type: ignore[attr-defined]
                                   "rev": doc.revision})
                doc._sent_open = True  # type: ignore[attr-defined]

    def doc_view(self, doc: Doc, with_conflict_texts: bool = False) -> Dict[str, Any]:
        view = doc.summary()
        view.update({
            "text": doc.text, "prover": doc.prover, "alternatives": doc.alternatives[-20:],
            "theory_status": self.theory_status.get(doc.path),
            "can_undo": bool(doc.undo_stack), "can_redo": bool(doc.redo_stack),
        })
        if with_conflict_texts and doc.conflict:
            view["conflict_texts"] = {"base": doc.conflict.get("base_text"),
                                      "disk": doc.conflict.get("disk_text"), "buffer": doc.text}
        return view

    # ----- the edit log (C5, C7)

    def commit(self, doc: Doc, author: Dict[str, Any], kind: str, change: ot.Change,
               target: Optional[int] = None, txn: Optional[str] = None,
               request: Optional[Any] = None, client: Optional["Client"] = None) -> Dict[str, Any]:
        change = ot.normalize(change, len(doc.text))
        old = doc.text
        new = ot.apply(old, change)
        doc.revision += 1
        entry = {
            "id": doc.next_entry, "rev": doc.revision, "author": author, "kind": kind,
            "change": [list(r) for r in change], "inverse": [list(r) for r in ot.invert(old, change)],
            "target": target, "txn": txn or f"e{doc.next_entry}", "time": now(),
        }
        doc.next_entry += 1
        doc.text = new
        doc.entries.append(entry)
        if len(doc.entries) > MEMORY_ENTRIES:
            doc.entries = doc.entries[-MEMORY_ENTRIES:]
        doc.last_edit = now()
        self.append_log(doc, entry)
        self.update_trackers(doc, entry, old)
        if author["kind"] == "agent":
            self.track_agent_change(doc, entry)
        if self.backend and self.backend.ready and getattr(doc, "_sent_open", False):
            # (as the prover sees the text: where a keyword line is, what changes there differs)
            seen = getattr(doc, "_prover_text", None)
            if seen is None:
                seen = prover_text(old)
            shown = prover_text(new)
            sent = change if seen == old and shown == new else ot.diff(seen, shown)
            doc._prover_text = shown  # type: ignore[attr-defined]
            with contextlib.suppress(HostError):
                self.backend.send({
                    "op": "edit", "file": doc.path, "rev": doc.revision,
                    "edits": [{"start": s, "end": e, "text": t} for s, e, t in ot.sequential(sent)],
                })
            if not sent and doc.prover:
                # Nothing changed for the prover (typing in a keyword line's hint): it reports
                # nothing new, so its report so far covers this revision too
                pr = doc.prover

                def mp(pos: int, right: bool = False) -> int:
                    return ot.map_pos(pos, change, right=right)
                pr["commands"] = [[mp(s0), mp(e0, True), n, st] for s0, e0, n, st in pr.get("commands") or []]
                pr["messages"] = [dict(m, start=mp(m["start"]), end=mp(m["end"], True)) for m in pr.get("messages") or []]
                pr["colours"] = [[mp(s0), mp(e0, True), k] for s0, e0, k in pr.get("colours") or []]
                if (pr.get("backend_revision") or 0) >= doc.revision - 1:
                    pr["backend_revision"] = doc.revision
                pr["revision"] = doc.revision
                self.host.broadcast_doc(self, doc, {"event": "doc_prover", "session": self.id, "path": doc.path,
                                                    "prover": pr})
        self.track_keyword_holes(doc, change)
        self.persist(doc)
        self.schedule_save(doc)
        self.host.broadcast_doc(self, doc, {
            "event": "doc_changed", "session": self.id, "path": doc.path, "revision": doc.revision,
            "entry": {k: entry[k] for k in ("id", "rev", "author", "kind", "change", "target", "txn")},
            "request": request, "client": client.id if client else None,
            "dirty": doc.dirty, "can_undo": bool(doc.undo_stack), "can_redo": bool(doc.redo_stack),
        })
        return entry

    def track_agent_change(self, doc: Doc, entry: Dict[str, Any]) -> None:
        delta = 0
        for s, e, t in entry["change"]:
            ns = s + delta
            doc.trackers.append({"kind": "change", "agent": entry["author"]["key"], "entry": entry["id"],
                                 "start": ns, "end": ns + len(t)})
            delta += len(t) - (e - s)
        changes = [t for t in doc.trackers if t["kind"] == "change"]
        if len(changes) > 400:
            drop = {id(t) for t in changes[:-400]}
            doc.trackers = [t for t in doc.trackers if id(t) not in drop]

    def update_trackers(self, doc: Doc, entry: Dict[str, Any], old_text: str) -> None:
        change = [tuple(r) for r in entry["change"]]
        author = entry["author"]
        notified = set()
        for tr in doc.trackers:
            if tr["kind"] == "change":
                touched = ot.touches(tr["start"], tr["end"], change)
                if touched and tr["agent"] != author.get("key"):
                    reverted = entry["kind"] in ("revert", "undo") and entry.get("target") == tr["entry"]
                    key = (tr["agent"], tr["entry"])
                    if key not in notified:
                        notified.add(key)
                        tr["pending_notice"] = "reverted" if reverted else "overlap"
                tr["start"], tr["end"] = ot.map_range(tr["start"], tr["end"], change, inclusive=False)
            elif tr["kind"] in ("grant", "note"):
                tr["start"], tr["end"] = ot.map_range(tr["start"], tr["end"], change, inclusive=True)
        for tr in doc.trackers:
            kind = tr.pop("pending_notice", None)
            if kind:
                line, _ = ot.lines_cols(doc.text, min(tr["start"], len(doc.text)))
                self.add_notice(tr["agent"], {
                    "kind": kind, "path": doc.path, "entry": tr["entry"],
                    "by": author.get("label") or author.get("key"), "by_kind": author["kind"],
                    "revision": entry["rev"], "line": line,
                    "region": [tr["start"], tr["end"]],
                    "text": excerpt(doc.text, tr["start"], tr["end"]),
                    "message": (f"Your change {tr['entry']} in {Path(doc.path).name} was reverted by "
                                f"{author.get('label')}" if kind == "reverted" else
                                f"{author.get('label')} edited text from your change {tr['entry']} "
                                f"in {Path(doc.path).name} around line {line}"),
                })

    def client_edit(self, client: "Client", path: str, base_rev: int, edits: List[Any],
                    policy: str, txn: Optional[str], request: Any) -> Dict[str, Any]:
        doc = self.get_doc(path)
        if base_rev > doc.revision:
            raise HostError("Base revision is ahead of the theory", "bad_revision")
        later_entries = doc.entries_after(base_rev)
        base_len = doc.text_length_at(base_rev)
        raw = [(e["start"], e["end"], e.get("text", "")) if isinstance(e, dict) else tuple(e)
               for e in edits]
        change = ot.normalize(raw, base_len)
        if not change:
            return {"revision": doc.revision, "change": []}
        try:
            rebased, _ = ot.rebase_through(change, [[tuple(r) for r in e["change"]] for e in later_entries],
                                           policy)
        except ot.Conflict as c:
            regions = []
            for (_, (bs, be, bt)) in c.overlaps:
                regions.append({"start": bs, "end": be, "text": bt})
            authors = sorted({e["author"].get("label", "?") for e in later_entries})
            raise HostError("Your edit overlaps text changed since revision %d by %s; read the region "
                            "again and retry" % (base_rev, ", ".join(authors)), "conflict",
                            revision=doc.revision, authors=authors,
                            current=[{"line": ot.lines_cols(doc.text, doc.map_offset(r["start"], base_rev))[0],
                                      "text": excerpt(doc.text, doc.map_offset(r["start"], base_rev),
                                                      doc.map_offset(r["end"], base_rev, right=True))}
                                     for r in regions[:5]])
        author = client.author()
        if author["kind"] == "agent" and not self.agent_may_edit(author["key"], doc, rebased):
            raise HostError("You have no editing rights here. Ask for them with isabelle_request_access (the "
                            "human answers in Deductum), or work in your own independent session.", "forbidden")
        if author["kind"] == "agent" and doc.conflict:
            raise HostError("This theory changed on disk while it had unsaved edits; resolve that first "
                            "(isabelle_resolve_conflict)", "disk_conflict")
        txn = txn or uuid.uuid4().hex[:12]
        # Deleting a proof tool typed in the text (sledgehammer, nitpick, …) stops it
        tools_hit = any(c[2] in TOOLS and any(s0 < c[1] and c[0] < e0 for s0, e0, _ in rebased)
                        for c in (doc.prover.get("commands") or []))
        entry = self.commit(doc, author, "edit", rebased, txn=txn, request=request, client=client)
        if tools_hit and self.backend and self.backend.ready:
            with contextlib.suppress(HostError):
                self.backend.send({"op": "cancel_tools"})
        if author["kind"] == "human":
            if not doc.undo_stack or doc.undo_stack[-1] != txn:
                doc.undo_stack.append(txn)
            doc.redo_stack.clear()
        client.note_position(self, doc, entry["change"][0][0] if entry["change"] else 0)
        reply = {"revision": doc.revision, "change": entry["change"], "entry": entry["id"]}
        if author["kind"] == "human" and client.kind == "app":
            self.keyword_fill(client, doc, entry, author, txn)
        return reply

    # `claude` or `codex` (a model in brackets, `codex[astra]`, and a hint after it) ending a line,
    # alone or after a proof's start (`show ?thesis unfolding foo_def claude`), not inside a string
    KEYWORD_LINE = re.compile(KEYWORD_PATTERN)

    def keyword_fill(self, client: "Client", doc: Doc, entry: Dict[str, Any], author: Dict[str, Any],
                     txn: str) -> None:
        """A line reading `claude` or `codex`, perhaps with a hint after it, that the human ends
        with Enter asks that LLM to fill it, like typing `sledgehammer`: the line becomes `sorry`
        (in the same undo step), and the LLM's checked proofs for it come as fill events"""
        if self.lifecycle != "Running" or not (self.backend and self.backend.ready):
            return
        shift = 0
        for s0, e0, t in entry["change"]:
            ns = s0 + shift
            shift += len(t) - (e0 - s0)
            # Each line this edit ends: Enter after it, or a pasted line
            ends = [ns + i for i, ch in enumerate(t) if ch == "\n"]
            found = None
            for nl in ends:
                line_start = doc.text.rfind("\n", 0, nl) + 1
                m = self.KEYWORD_LINE.match(doc.text[line_start:nl])
                if m:
                    found = (nl, line_start, m)
                    break
            if not found:
                continue
            nl, line_start, m = found
            self.keyword_line(client, doc, line_start, author, txn)
            return

    def keyword_line(self, client: "Client", doc: Doc, line_start: int, author: Dict[str, Any], txn: str) -> bool:
        """The line at `line_start`, if it reads `claude` or `codex` (and a hint): it becomes the
        hole that LLM is asked to fill"""
        nl = doc.text.find("\n", line_start)
        nl = len(doc.text) if nl < 0 else nl
        m = self.KEYWORD_LINE.match(doc.text[line_start:nl])
        if not m:
            return False
        start = line_start + len(m.group(1))
        # (asked once: a pause again on the same line does not ask again; a line changed since,
        # another model or hint, asks again instead)
        line = doc.text[line_start:nl]
        for job, h in list(self.keyword_holes.items()):
            if h["path"] == doc.path and h["start"] == start:
                if h.get("line") == line:
                    return False
                self.drop_keyword_job(job)
        hint = (m.group(4) or "").strip()
        if len(hint) >= 2 and hint[0] == hint[-1] and hint[0] in "\"'":
            hint = hint[1:-1]
        for a, b in (("\\<open>", "\\<close>"), ("\u2039", "\u203a")):
            if hint.startswith(a) and hint.endswith(b):
                hint = hint[len(a):len(hint) - len(b)]
        # The keyword stays (the prover sees a `sorry` there) until a proof is inserted for it;
        # `@name` asks the listening agent of that name
        kind = m.group(2)
        route = ("name:" + kind[1:]) if kind.startswith("@") else None
        r = self.fill_hole(client, doc.path, start, doc.revision, "agent" if route else kind, hint,
                           hole=[start, nl, kind], model=m.group(3) or None, route=route)
        self.keyword_holes[r["job"]] = {"path": doc.path, "start": start, "end": nl, "client": client, "line": line}
        return True

    def track_keyword_holes(self, doc: Doc, change: "ot.Change") -> None:
        """Keyword lines move with edits; one whose keyword is gone (deleted, or replaced by an
        inserted proof) stops its job, and the app drops its proofs"""
        for job, h in list(self.keyword_holes.items()):
            if h["path"] != doc.path:
                continue
            h["start"] = ot.map_pos(h["start"], change, right=False)
            h["end"] = ot.map_pos(h["end"], change, right=True)
            line_start = doc.text.rfind("\n", 0, h["start"]) + 1
            nl = doc.text.find("\n", h["start"])
            nl = len(doc.text) if nl < 0 else nl
            m = self.KEYWORD_LINE.match(doc.text[line_start:nl])
            if m and line_start + len(m.group(1)) == h["start"]:
                h["end"] = nl
                continue
            self.drop_keyword_job(job)

    def drop_keyword_job(self, job: str) -> None:
        """Stop a keyword line's job; the app drops its proofs"""
        h = self.keyword_holes.pop(job, None)
        task = self.fill_tasks.pop(job, None)
        if task:
            task.cancel()
        if h:
            with contextlib.suppress(Exception):
                self._fill_push(h["client"], job, status="canceled", candidates=[])

    def text_at(self, doc: Doc, revision: int) -> str:
        """The theory's text at an earlier revision, from the edit log"""
        text = doc.text
        for e in reversed(doc.entries_after(revision)):
            text = ot.apply(text, [tuple(r) for r in e["inverse"]])
        return text

    def reverse_entries(self, doc: Doc, entries: List[Dict[str, Any]], author: Dict[str, Any],
                        kind: str, txn: str, report: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
        """Undo a transaction by content (C7): put back the text it replaced. When the
        theory has not otherwise changed since, that is exact; edits made since are merged
        around it character by character, and where they changed the same characters the
        later edit stays (the undo never takes away someone else's work). `report` gets
        where the undo acted and whose later edits it kept"""
        if not entries:
            return []
        first = min(e["rev"] for e in entries) - 1
        last = max(e["rev"] for e in entries)
        before = self.text_at(doc, first)
        after = self.text_at(doc, last)
        current = doc.text
        wanted = ot.fine(after, before)
        if current == after:
            kept, dropped = wanted, []
        else:
            since = ot.fine(after, current)
            kept, dropped = ot.rebase_through(split_at(wanted, since), [since], "partial")
        if report is not None:
            later = [e for e in doc.entries if e["rev"] > last and e["author"].get("kind") != "human"]
            report["kept_by"] = sorted({e["author"].get("label", "?") for e in later}) if dropped else []
            report["dropped"] = len(dropped)
        target = max(entries, key=lambda e: e["rev"])
        made = []
        if kept:
            entry = self.commit(doc, author, kind, kept, target=target["id"], txn=txn)
            made.append(entry)
            if report is not None and entry["change"]:
                c = entry["change"][0]
                report["at"] = ot.map_pos(c[0], [tuple(r) for r in entry["change"]], right=True)
                report["line"] = ot.lines_cols(doc.text, min(report["at"], len(doc.text)))[0]
        for e in entries:
            doc.undone[e["id"]] = made[0]["id"] if made else -1
        return made

    def undo(self, client: "Client", path: str) -> Dict[str, Any]:
        doc = self.get_doc(path)
        author = client.author()
        if author["kind"] != "human":
            raise HostError("Agents revert their changes with isabelle_revert", "forbidden")
        # A step later edits have entirely replaced has nothing left to undo: it is passed over
        # (and said so) on the way to the next
        passed = 0
        while doc.undo_stack:
            txn = doc.undo_stack.pop()
            entries = [e for e in doc.entries if e["txn"] == txn and e["author"]["kind"] == "human"
                       and e["id"] not in doc.undone]
            if not entries:
                continue
            report: Dict[str, Any] = {}
            made = self.reverse_entries(doc, entries, HUMAN, "undo", "undo:" + txn, report)
            if not made:
                passed += 1
                continue
            doc.redo_stack.append(txn)
            self.persist(doc)
            return {"revision": doc.revision, "undone": [e["id"] for e in entries],
                    "entries": [m["id"] for m in made], "at": report.get("at"),
                    "note": self.undo_note("Undid", report, passed)}
        self.persist(doc)
        return {"revision": doc.revision, "undone": [],
                "note": self.undo_note("Nothing more to undo", {}, passed)}

    @staticmethod
    def undo_note(what: str, report: Dict[str, Any], passed: int) -> str:
        note = what + (f" at line {report['line']}" if report.get("line") else "")
        if report.get("dropped"):
            who = ", ".join(report.get("kept_by") or []) or "a later edit"
            note += f"; kept what {who} changed there since"
        if passed:
            note += f" ({passed} earlier step{'s' if passed > 1 else ''} had been overwritten since)"
        return note

    def redo(self, client: "Client", path: str) -> Dict[str, Any]:
        doc = self.get_doc(path)
        if client.author()["kind"] != "human":
            raise HostError("Only the human has a redo stack", "forbidden")
        while doc.redo_stack:
            txn = doc.redo_stack.pop()
            entries = [e for e in doc.entries if e["txn"] == "undo:" + txn and e["id"] not in doc.undone]
            if entries:
                report: Dict[str, Any] = {}
                self.reverse_entries(doc, entries, HUMAN, "redo", txn, report)
                doc.undo_stack.append(txn)
                self.persist(doc)
                return {"revision": doc.revision, "redone": [e["id"] for e in entries], "at": report.get("at"),
                        "note": self.undo_note("Redid", report, 0)}
        return {"revision": doc.revision, "redone": [], "note": "Nothing to redo"}

    def revert(self, client: "Client", path: str, entry_id: int) -> Dict[str, Any]:
        doc = self.get_doc(path)
        target = doc.entry(entry_id)
        author = client.author()
        if author["kind"] == "agent":
            if target["author"].get("key") != author["key"]:
                raise HostError("Agents can only revert their own changes", "forbidden")
        if entry_id in doc.undone:
            raise HostError(f"Change {entry_id} was already reverted", "already_reverted")
        group = [e for e in doc.entries if e["txn"] == target["txn"] and e["id"] not in doc.undone
                 and e["author"].get("key") == target["author"].get("key")]
        made = self.reverse_entries(doc, group, author, "revert", "revert:" + target["txn"])
        self.persist(doc)
        return {"revision": doc.revision, "reverted": [e["id"] for e in group],
                "entries": [m["id"] for m in made]}

    def history(self, path: str, limit: int = 100, author_key: Optional[str] = None) -> List[Dict[str, Any]]:
        doc = self.get_doc(path)
        out = []
        for e in reversed(doc.entries):
            if author_key and e["author"].get("key") != author_key:
                continue
            first = e["change"][0] if e["change"] else [0, 0, ""]
            out.append({
                "id": e["id"], "rev": e["rev"], "author": e["author"], "kind": e["kind"],
                "txn": e["txn"], "target": e.get("target"), "time": e["time"],
                "reverted_by": doc.undone.get(e["id"]),
                "line": ot.lines_cols(doc.text, min(doc.map_offset(first[0], e["rev"]), len(doc.text)))[0],
                "summary": " / ".join(
                    (("−%d " % (b - a)) if b > a else "") + (("+" + repr(t[:60])) if t else "")
                    for a, b, t in e["change"][:3]),
            })
            if len(out) >= limit:
                break
        return out

    # ----- grants (C6, C9)

    def agent_may_edit(self, key: str, doc: Doc, change: ot.Change) -> bool:
        owner = self.meta.get("owner")
        if owner and owner.get("key") == key:
            return True
        for g in self.meta["grants"]:
            if g["agent"] != key:
                continue
            if g["scope"] == "folder" and path_inside(doc.path, self.meta["folder"]):
                return True
            if g["scope"] == "theory" and os.path.realpath(g["path"]) == doc.path:
                return True
            if g["scope"] == "proof" and os.path.realpath(g["path"]) == doc.path:
                tr = next((t for t in doc.trackers if t["kind"] == "grant" and t["grant"] == g["id"]), None)
                if tr and all(tr["start"] <= s and e <= tr["end"] for s, e, _ in change):
                    return True
        return False

    def proof_range(self, doc: Doc, offset: int) -> Tuple[int, int]:
        commands = doc.prover.get("commands") or []
        kinds = self.syntax_kinds
        idx = None
        for i, (s, e, name, _st) in enumerate(commands):
            if s <= offset < max(e, s + 1):
                idx = i
                break
        if idx is None:
            raise HostError("No checked command at that position yet", "not_ready")

        def is_goal(name: str) -> bool:
            return kinds.get(name, "").startswith("thy_goal")

        def is_theory_level(name: str) -> bool:
            k = kinds.get(name, "")
            return k.startswith("thy_") or k.startswith("document_") or name in ("end", "theory")

        start_i = idx
        while start_i > 0 and not is_goal(commands[start_i][2]):
            if is_theory_level(commands[start_i][2]) and start_i != idx:
                break
            start_i -= 1
        if not is_goal(commands[start_i][2]):
            raise HostError("That position is not inside a proof", "not_a_proof")
        end_i = start_i + 1
        while end_i < len(commands) and not is_theory_level(commands[end_i][2]):
            end_i += 1
        end = commands[end_i - 1][1]
        return commands[start_i][0], end

    def grant(self, agent_key: str, label: str, scope: str, path: Optional[str],
              offset: Optional[int]) -> Dict[str, Any]:
        if scope not in ("folder", "theory", "proof"):
            raise HostError("Scope must be folder, theory or proof")
        g = {"id": uuid.uuid4().hex[:8], "agent": agent_key, "label": label, "scope": scope,
             "path": os.path.realpath(path) if path else None, "created": now()}
        if scope == "proof":
            if path is None or offset is None:
                raise HostError("A proof grant needs a theory and a position")
            doc = self.get_doc(path)
            start, end = self.proof_range(doc, offset)
            g["range"] = [start, end]
            g["line"] = ot.lines_cols(doc.text, start)[0]
            doc.trackers.append({"kind": "grant", "grant": g["id"], "start": start, "end": end})
            self.persist(doc)
        elif scope == "theory" and not path:
            raise HostError("A theory grant needs a theory")
        self.meta["grants"].append(g)
        self.add_notice(agent_key, {"kind": "granted", "message":
                                    f"You may now edit {self.describe_scope(g)} in session {self.summary()['name']}",
                                    "grant": g})
        self.persist()
        self.host.broadcast_session(self)
        return g

    def request_access(self, client: "Client", scope: str, path: Optional[str], offset: Optional[int],
                       reason: str) -> Dict[str, Any]:
        """An agent asks the human for editing rights: Deductum shows the request with Allow and
        Deny, and the agent gets a notice with the answer"""
        if scope not in ("folder", "theory", "proof"):
            raise HostError("Scope must be folder, theory or proof")
        author = client.author()
        key = author["key"]
        r = {"id": uuid.uuid4().hex[:8], "agent": key, "label": author.get("label") or key, "scope": scope,
             "path": os.path.realpath(path) if path else None, "offset": offset, "reason": reason.strip()[:600],
             "created": now()}
        if scope != "folder" and not path:
            raise HostError("A theory or proof request needs a theory")
        if scope == "proof":
            if offset is None:
                raise HostError("A proof request needs a position")
            doc = self.get_doc(path or "")
            start, _ = self.proof_range(doc, offset)
            r["line"] = ot.lines_cols(doc.text, start)[0]
        # One open request per agent and scope: a new one replaces it
        requests = self.meta.setdefault("requests", [])
        requests[:] = [q for q in requests if not (q["agent"] == key and q["scope"] == scope and q["path"] == r["path"])]
        requests.append(r)
        self.persist()
        self.host.broadcast_session(self)
        for c in self.host.clients:
            if c.kind == "app":
                c.push({"event": "access_request", "session": self.id, "request": r})
        return r

    def answer_request(self, request_id: str, allow: bool) -> Dict[str, Any]:
        requests = self.meta.setdefault("requests", [])
        r = next((q for q in requests if q["id"] == request_id), None)
        if not r:
            raise HostError("No such request (the agent may have withdrawn it)", "not_found")
        requests.remove(r)
        if allow:
            g = self.grant(r["agent"], r["label"], r["scope"], r["path"], r["offset"])
            return {"grant": g}
        self.add_notice(r["agent"], {"kind": "denied", "message":
                                     f"The human declined your request to edit {self.describe_scope(r)}",
                                     "request": r})
        self.persist()
        self.host.broadcast_session(self)
        return {"denied": r}

    # ----- collaboration: what the human sees, showing them places, notes on the text

    def human_view(self) -> Dict[str, Any]:
        """Where the human is in Deductum: the theory, the caret, the selection, and the
        command and proof around the caret"""
        views = [c.views.get(self.id) for c in self.host.clients if c.kind == "app" and c.views.get(self.id)]
        if not views:
            return {"open": False}
        v = dict(max(views, key=lambda v: v.get("time", 0)))
        doc = self.docs.get(v.get("path", ""))
        if doc is not None:
            off = min(int(v.get("offset", 0)), len(doc.text))
            v["line"], v["column"] = ot.lines_cols(doc.text, off)
            for at in (off, max(0, off - 1)):
                with contextlib.suppress(HostError):
                    start, end = self.proof_range(doc, at)
                    v["proof"] = {"line": ot.lines_cols(doc.text, start)[0], "text": doc.text[start:end][:4000]}
                    # The lemma: its name and statement, for "prove this lemma"
                    m = re.match(r"\s*(lemma|theorem|corollary|proposition|schematic_goal)\b\s*(?:([^\s:\"\[]+)(?:\s*\[[^\]]*\])?\s*:)?",
                                 doc.text[start:end])
                    if m:
                        statement = doc.text[start:end]
                        first = re.search(r"\n\s*(proof|by|apply|using|unfolding|sorry|oops|including|supply)\b", statement)
                        v["lemma"] = {"kind": m.group(1), "name": m.group(2), "line": v["proof"]["line"],
                                      "statement": (statement[:first.start()] if first else statement)[:2000]}
                    break
            # The construct the goal is for (the Isar statement around the caret, or its step if
            # that is long), as the human's editor tints it
            c = v.pop("construct", None)
            if isinstance(c, list) and len(c) == 2:
                cs, ce = max(0, min(int(c[0]), len(doc.text))), max(0, min(int(c[1]), len(doc.text)))
                v["construct"] = {"line": ot.lines_cols(doc.text, cs)[0], "end_line": ot.lines_cols(doc.text, max(cs, ce - 1))[0],
                                  "text": doc.text[cs:ce][:4000]}
        v["open"] = True
        return v

    def show_human(self, client: "Client", path: str, start: int, end: int, message: str) -> Dict[str, Any]:
        doc = self.get_doc(path)
        author = client.author()
        line = ot.lines_cols(doc.text, min(start, len(doc.text)))[0]
        event = {"event": "show", "session": self.id, "path": doc.path, "start": start, "end": max(start, end),
                 "line": line, "message": message.strip()[:600], "revision": doc.revision,
                 "from": {"key": author.get("key"), "label": author.get("label")}}
        apps = [c for c in self.host.clients if c.kind == "app"]
        for c in apps:
            c.push(event)
        return {"shown": bool(apps), "line": line}

    def add_note(self, client: "Client", path: str, start: int, end: int, text: str) -> Dict[str, Any]:
        doc = self.get_doc(path)
        author = client.author()
        note = {"kind": "note", "id": uuid.uuid4().hex[:8], "agent": author.get("key"), "label": author.get("label"),
                "start": start, "end": max(start, end), "text": text.strip()[:2000], "replies": [], "created": now()}
        doc.trackers.append(note)
        self.persist(doc)
        self.notes_changed(doc)
        return self.note_view(doc, note)

    def note_view(self, doc: Doc, n: Dict[str, Any]) -> Dict[str, Any]:
        return {"id": n["id"], "path": doc.path, "agent": n["agent"], "label": n["label"], "start": n["start"],
                "end": n["end"], "line": ot.lines_cols(doc.text, min(n["start"], len(doc.text)))[0],
                "text": n["text"], "replies": n["replies"], "excerpt": excerpt(doc.text, n["start"], n["end"]),
                "revision": doc.revision}

    def notes(self, path: Optional[str] = None) -> List[Dict[str, Any]]:
        out = []
        for doc in self.docs.values():
            if path and doc.path != os.path.realpath(path):
                continue
            out.extend(self.note_view(doc, n) for n in doc.trackers if n["kind"] == "note")
        return out

    def find_note(self, note_id: str) -> Tuple[Doc, Dict[str, Any]]:
        for doc in self.docs.values():
            for n in doc.trackers:
                if n["kind"] == "note" and n["id"] == note_id:
                    return doc, n
        raise HostError("No such note (it may have been resolved)", "not_found")

    def reply_note(self, client: "Client", note_id: str, text: str, resolve: bool) -> Dict[str, Any]:
        doc, n = self.find_note(note_id)
        author = client.author()
        if text.strip():
            n["replies"].append({"by": author.get("label") or author["kind"], "kind": author["kind"],
                                 "text": text.strip()[:2000], "time": now()})
        if resolve:
            doc.trackers.remove(n)
        if author["kind"] == "human" and n.get("agent"):
            what = "resolved" if resolve else "replied to"
            self.add_notice(n["agent"], {"kind": "note_" + ("resolved" if resolve else "reply"), "path": doc.path,
                                         "note": n["id"], "message": f"The human {what} your note at line "
                                         f"{ot.lines_cols(doc.text, min(n['start'], len(doc.text)))[0]}"
                                         + (f": {text.strip()}" if text.strip() else ""), "reply": text.strip()})
        self.persist(doc)
        self.notes_changed(doc)
        return {"ok": True}

    def notes_changed(self, doc: Doc) -> None:
        self.host.broadcast_doc(self, doc, {"event": "notes", "session": self.id, "path": doc.path,
                                            "notes": self.notes(doc.path)})

    def describe_scope(self, g: Dict[str, Any]) -> str:
        if g["scope"] == "folder":
            return "the theories in " + self.meta["folder"]
        if g["scope"] == "theory":
            return Path(g["path"]).name
        return f"the proof at line {g.get('line')} of {Path(g['path']).name}"

    def revoke(self, grant_id: str) -> Dict[str, Any]:
        grants = self.meta["grants"]
        g = next((g for g in grants if g["id"] == grant_id), None)
        if not g:
            raise HostError("No such grant", "not_found")
        grants.remove(g)
        for doc in self.docs.values():
            doc.trackers = [t for t in doc.trackers if not (t["kind"] == "grant" and t["grant"] == grant_id)]
            self.persist(doc)
        self.add_notice(g["agent"], {"kind": "revoked", "message":
                                     f"Your editing rights for {self.describe_scope(g)} were revoked", "grant": g})
        self.persist()
        self.host.broadcast_session(self)
        return g

    # ----- notices (C7)

    def add_notice(self, agent_key: str, notice: Dict[str, Any]) -> None:
        self.meta["notice_seq"] += 1
        notice = dict(notice, id=f"{self.id}-{self.meta['notice_seq']}", session=self.id,
                      session_name=self.meta.get("name") or Path(self.meta["folder"]).name, time=now())
        lst = self.meta["notices"].setdefault(agent_key, [])
        lst.append(notice)
        del lst[:-200]
        self.persist()
        for c in self.host.clients:
            if c.key == agent_key:
                c.push({"event": "notice", "notice": notice})

    def notify_agents_for(self, doc: Doc, notice: Dict[str, Any]) -> None:
        keys = set()
        owner = self.meta.get("owner")
        if owner:
            keys.add(owner["key"])
        for g in self.meta["grants"]:
            if g["scope"] == "folder" and path_inside(doc.path, self.meta["folder"]) or \
               g.get("path") and os.path.realpath(g["path"]) == doc.path:
                keys.add(g["agent"])
        for k in keys:
            self.add_notice(k, notice)

    # ----- saving and the disk (C13)

    def schedule_save(self, doc: Doc, delay: float = AUTOSAVE_DELAY) -> None:
        if doc.save_handle:
            doc.save_handle.cancel()
        doc.save_handle = asyncio.get_running_loop().call_later(delay, self.save_doc, doc)

    def save_doc(self, doc: Doc, force: bool = False) -> bool:
        doc.save_handle = None
        if not doc.dirty:
            return True
        if doc.conflict:
            return False
        if stat_sig(doc.path) != doc.disk_sig:
            self.check_disk(doc)
            if doc.conflict or not doc.dirty:
                return not doc.conflict
        try:
            write_file_atomic(doc.path, encode_file(doc.text, doc.crlf, doc.bom))
        except OSError as exn:
            doc.save_error = str(exn)
            self.host.broadcast_doc(self, doc, {"event": "doc_saved", "session": self.id, "path": doc.path,
                                                "error": doc.save_error, "revision": doc.revision,
                                                "dirty": True})
            self.notify_agents_for(doc, {"kind": "save_failed", "path": doc.path,
                                         "message": f"Saving {Path(doc.path).name} failed: {exn}"})
            return False
        doc.save_error = None
        doc.disk_text = doc.text
        doc.disk_sig = stat_sig(doc.path)
        doc.saved_revision = doc.revision
        self.persist(doc)
        self.host.broadcast_doc(self, doc, {"event": "doc_saved", "session": self.id, "path": doc.path,
                                            "revision": doc.revision, "dirty": False})
        self.backend_disk_changed([doc.path])
        return True

    def save_all(self) -> None:
        for doc in list(self.docs.values()):
            if doc.dirty and not doc.conflict:
                self.save_doc(doc)

    def check_disk(self, doc: Doc) -> None:
        sig = stat_sig(doc.path)
        if sig == doc.disk_sig:
            return
        got = read_file(doc.path)
        disk_text = got[0] if got else None
        doc.disk_sig = sig
        if disk_text is not None and disk_text == doc.disk_text:
            self.persist(doc)
            return
        if disk_text is not None and doc.conflict and doc.conflict.get("disk_text") == disk_text:
            return
        if disk_text is not None and not doc.dirty and not doc.conflict:
            # No unsaved edits: take the disk version, as a logged, revertable edit
            doc.crlf, doc.bom = got[1], got[2]
            change = line_change(doc.text, disk_text)
            doc.disk_text = disk_text
            self.commit(doc, DISK, "reload", change)
            doc.saved_revision = doc.revision
            self.persist(doc)
            return
        doc.conflict = {"disk_text": disk_text, "base_text": doc.disk_text, "since": now()}
        if doc.save_handle:
            doc.save_handle.cancel()
            doc.save_handle = None
        self.persist(doc)
        self.host.broadcast_doc(self, doc, {"event": "doc_conflict", "session": self.id, "path": doc.path,
                                            "conflict": doc.conflict_summary()})
        self.host.broadcast_session(self)
        self.notify_agents_for(doc, {
            "kind": "disk_conflict", "path": doc.path,
            "message": f"{Path(doc.path).name} changed on disk while it had unsaved edits. Read the base, "
                       "disk and buffer texts with isabelle_conflict and resolve with "
                       "isabelle_resolve_conflict, or leave it to the human."})

    def resolve(self, client: "Client", path: str, choice: str, text: Optional[str]) -> Dict[str, Any]:
        doc = self.get_doc(path)
        if not doc.conflict:
            raise HostError("No conflict to resolve", "no_conflict")
        author = client.author()
        if author["kind"] == "agent" and not self.agent_may_edit(author["key"], doc, [(0, len(doc.text), "")]):
            raise HostError("You have no editing rights for this theory", "forbidden")
        disk_text = doc.conflict.get("disk_text")
        if choice == "disk":
            if disk_text is None:
                raise HostError("The file was deleted; keep the buffer to write it again")
            target = disk_text
        elif choice == "buffer":
            target = doc.text
        elif choice == "merge":
            if text is None:
                raise HostError("A merge needs the merged text")
            target = text.replace("\r\n", "\n")
        else:
            raise HostError("Choice must be disk, buffer or merge")
        doc.conflict = None
        doc.disk_text = disk_text
        doc.disk_sig = stat_sig(doc.path)
        if target != doc.text:
            self.commit(doc, author, "resolve", line_change(doc.text, target), client=client)
        self.persist(doc)
        self.schedule_save(doc, 0.1)
        self.host.broadcast_doc(self, doc, {"event": "doc_conflict", "session": self.id, "path": doc.path,
                                            "conflict": None})
        self.host.broadcast_session(self)
        return {"revision": doc.revision}

    def watch(self) -> None:
        for doc in list(self.docs.values()):
            if stat_sig(doc.path) != doc.disk_sig:
                self.check_disk(doc)
        # New and deleted theories show in the app at once; changed ones not open here are
        # passed to the prover (it reads them from disk)
        t = now()
        if t - self.watch_listed > 5:
            listed = [p for p in list_theories(self.meta["folder"], 1500) if p.endswith(".thy")]
            self.watch_listed = t
            if self.watch_listed_once and set(listed) != set(self.watch_list):
                for c in self.attached_clients():
                    c.push({"event": "theories_changed", "session": self.id})
            self.watch_listed_once = True
            self.watch_list = listed
            for p in self.watch_list:
                self.watch_sigs.setdefault(p, stat_sig(p))
        if not (self.backend and self.backend.ready):
            return
        changed = []
        for p in self.watch_list:
            if p in self.docs:
                continue
            sig = stat_sig(p)
            if sig != self.watch_sigs.get(p):
                self.watch_sigs[p] = sig
                changed.append(p)
        if changed:
            self.backend_disk_changed(changed)

    def prover_stats(self) -> Optional[Dict[str, Any]]:
        """CPU and memory of the prover's process group (the JVM and the ML process)"""
        if not (self.backend and self.backend.proc and self.backend.proc.returncode is None):
            return None
        pgid = self.backend.proc.pid
        ticks = 0
        rss_pages = 0
        try:
            for entry in os.listdir("/proc"):
                if not entry.isdigit():
                    continue
                try:
                    with open(f"/proc/{entry}/stat") as f:
                        fields = f.read().rsplit(")", 1)[1].split()
                except OSError:
                    continue
                if int(fields[2]) != pgid:  # process group
                    continue
                ticks += int(fields[11]) + int(fields[12])
                rss_pages += int(fields[21])
        except OSError:
            return None
        hz = os.sysconf("SC_CLK_TCK")
        cpu_s = ticks / hz
        t = time.monotonic()
        cpu = None
        if self.cpu_sample:
            dt = t - self.cpu_sample[0]
            if dt > 0:
                cpu = round(100 * (cpu_s - self.cpu_sample[1]) / dt)
        self.cpu_sample = (t, cpu_s)
        return {"cpu": cpu, "rss_mb": round(rss_pages * os.sysconf("SC_PAGE_SIZE") / 1e6)}

    def activity(self) -> Dict[str, Any]:
        t = now()
        running = []
        for (path, start, name), since in sorted(self.running_since.items(), key=lambda kv: kv[1]):
            doc = self.docs.get(path)
            line = ot.lines_cols(doc.text, min(start, len(doc.text)))[0] if doc else 0
            running.append({"path": path, "line": line, "name": name, "seconds": round(t - since)})
        theories = []
        for path, st in self.theory_status.items():
            if not st:
                continue
            total = st.get("total") or 0
            theories.append({"path": path, "theory": st.get("theory"), "percentage": st.get("percentage", 0),
                             "running": st.get("running", 0), "failed": st.get("failed", 0),
                             "consolidated": st.get("consolidated", False), "total": total,
                             "header_errors": st.get("header_errors"), "imports": st.get("imports", [])})
        return {"lifecycle": self.lifecycle, "prover": self.prover_stats(), "running": running[:20],
                "theories": theories, "listeners": self.live_listeners(),
                "route": self.meta.get("query_route") or "helper"}

    def push_activity(self) -> None:
        apps = [c for c in self.attached_clients() if c.kind == "app"]
        if not apps or not (self.backend and self.backend.ready):
            return
        act = self.activity()
        for c in apps:
            c.push({"event": "activity", "session": self.id, "activity": act})

    def backend_disk_changed(self, paths: List[str]) -> None:
        if self.backend and self.backend.ready:
            with contextlib.suppress(HostError):
                self.backend.send({"op": "disk_changed", "files": paths})

    # ----- lifecycle (C3, C7)

    def start(self) -> None:
        if self.lifecycle in ("Starting", "Running", "Paused", "Stopping"):
            return
        self.meta["epoch"] = self.meta.get("epoch", 0) + 1
        self.meta["desired"] = "running"
        for doc in self.docs.values():
            doc._sent_open = False  # type: ignore[attr-defined]
        self.set_lifecycle("Starting", "Starting the prover")
        self.start_task = asyncio.create_task(self._start())

    async def _start(self) -> None:
        try:
            await asyncio.to_thread(ensure_component, self.meta["runtime"],
                                    lambda m: self.session_log(m))
            self.backend = Backend(self)
            await self.backend.start()
        except HostError as exn:
            self.backend = None
            self.set_lifecycle("Failed", str(exn))
        except Exception as exn:
            self.backend = None
            self.set_lifecycle("Failed", f"Could not start the prover: {exn}")

    def session_log(self, text: str) -> None:
        self.logs = (self.logs + [text])[-300:]
        for c in self.attached_clients():
            if c.kind == "app":
                c.push({"event": "session_log", "session": self.id, "text": text})

    def pause(self) -> None:
        if self.lifecycle != "Running":
            raise HostError(f"Cannot pause a session that is {self.lifecycle}", "bad_state")
        self.save_all()
        self.meta["desired"] = "paused"
        assert self.backend
        self.backend.send({"op": "checking", "on": False})
        for qid, q in list(self.queries.items()):
            if q.get("status") not in ("finished", "canceled", "failed"):
                with contextlib.suppress(HostError):
                    self.backend.send({"op": "cancel_query", "query": qid})
        self.set_lifecycle("Paused", None)

    def resume(self) -> None:
        if self.lifecycle == "Paused":
            self.meta["desired"] = "running"
            assert self.backend
            self.backend.send({"op": "checking", "on": True})
            self.set_lifecycle("Running", None)
        elif self.lifecycle in ("Stopped", "Failed"):
            self.start()
        else:
            raise HostError(f"Cannot resume a session that is {self.lifecycle}", "bad_state")

    async def stop(self) -> None:
        if self.lifecycle in ("Stopped", "Stopping"):
            return
        self.save_all()
        self.meta["desired"] = "stopped"
        self.set_lifecycle("Stopping", "Stopping the prover")
        if self.backend:
            await self.backend.stop()
        self.backend = None
        self.set_lifecycle("Stopped", None)

    def on_backend_exit(self, backend: Backend, code: int) -> None:
        if backend is not self.backend:
            return
        self.backend = None
        for doc in self.docs.values():
            doc._sent_open = False  # type: ignore[attr-defined]
        for qid, q in list(self.queries.items()):
            if q.get("status") not in ("finished", "canceled", "failed"):
                self.query_event({"event": "query", "query": qid, "status": "failed",
                                  "error": "The prover stopped", "messages": []})
        if self.lifecycle in ("Stopping", "Stopped"):
            return
        if self.lifecycle != "Failed":
            tail = "\n".join(self.stderr_tail[-8:])
            self.set_lifecycle("Failed", f"The prover exited (code {code})" + (": " + tail if tail else ""))

    def require_checking(self) -> Backend:
        if self.lifecycle == "Paused":
            raise HostError("The session is paused; checking and queries resume when the human resumes "
                            "it", "paused")
        if self.lifecycle in ("Stopped", "Stopping"):
            raise HostError("The session is stopped; the human can resume it", "stopped")
        if self.lifecycle == "Failed":
            raise HostError("The session failed: " + (self.meta.get("message") or ""), "failed")
        if not self.backend or not self.backend.ready:
            raise HostError("The prover is still starting", "starting")
        return self.backend

    # ----- backend events

    def on_backend_event(self, msg: Dict[str, Any]) -> None:
        ev = msg.get("event")
        if ev == "ready":
            assert self.backend
            self.backend.ready = True
            self.meta["identifier"] = msg.get("identifier") or self.meta.get("identifier")
            for doc in self.docs.values():
                if doc.opened:
                    self.backend_open(doc)
            if self.meta.get("prover_options"):
                self.backend.send({"op": "options", "set": [f"{k}={v}" for k, v in
                                                            self.meta["prover_options"].items()]})
            if self.meta.get("desired") == "paused":
                self.backend.send({"op": "checking", "on": False})
                self.set_lifecycle("Paused", None)
            else:
                self.set_lifecycle("Running", None)
            asyncio.get_running_loop().create_task(self.load_syntax())
        elif ev == "phase":
            phase = msg.get("phase")
            if phase == "failed":
                self.set_lifecycle("Failed", msg.get("message") or "The prover failed")
            elif phase == "building":
                self.meta["message"] = "Building the session image for " + self.meta["logic"]
                self.host.broadcast_session(self)
        elif ev in ("build", "log"):
            self.session_log(msg.get("text", ""))
        elif ev == "node":
            self.node_event(msg)
        elif ev == "query":
            self.query_event(msg)

    async def load_syntax(self) -> None:
        if not (self.backend and self.backend.ready):
            return
        with contextlib.suppress(Exception):
            r = await self.backend.request({"op": "syntax"})
            self.syntax_kinds = r.get("syntax", {}).get("kinds", {})

    def node_event(self, msg: Dict[str, Any]) -> None:
        path = msg.get("file", "")
        status = dict(msg.get("status") or {})
        if msg.get("header_errors"):
            status["header_errors"] = msg["header_errors"]
        if msg.get("imports"):
            status["imports"] = msg["imports"]
        status["theory"] = msg.get("theory")
        self.theory_status[path] = status
        doc = self.docs.get(path)
        if doc is not None and "commands" in msg:
            seq = int(msg.get("seq", doc.revision))
            commands = msg["commands"]
            messages = msg["messages"]
            colours = msg.get("colours", [])
            if seq < doc.revision:
                try:
                    laters = [[tuple(r) for r in e["change"]] for e in doc.entries_after(seq)]
                except HostError:
                    laters = None
                if laters is None:
                    commands, messages, colours = [], [], []
                else:
                    def mp(p: int, right: bool = False) -> int:
                        for ch in laters:
                            p = ot.map_pos(p, ch, right=right)
                        return p
                    commands = [[mp(s), mp(e, True), n, st] for s, e, n, st in commands]
                    messages = [dict(m, start=mp(m["start"]), end=mp(m["end"], True)) for m in messages]
                    colours = [[mp(s), mp(e, True), k] for s, e, k in colours]
            doc.prover = {"revision": doc.revision, "backend_revision": seq, "status": msg.get("status"),
                          "commands": commands, "messages": messages, "colours": colours}
            # When each running command was first seen running, for the activity view
            t = now()
            running = {(path, c[0], c[2]) for c in commands if c[3] == "running"}
            for key in list(self.running_since):
                if key[0] == path and key not in running:
                    del self.running_since[key]
            for key in running:
                self.running_since.setdefault(key, t)
            self.host.broadcast_doc(self, doc, {"event": "doc_prover", "session": self.id, "path": path,
                                                "prover": doc.prover})
        if self.status_handle is None:
            self.status_handle = asyncio.get_running_loop().call_later(0.5, self.flush_status)

    def flush_status(self) -> None:
        self.status_handle = None
        for c in self.attached_clients():
            if c.kind == "app":
                c.push({"event": "theory_status", "session": self.id, "status": self.theory_status,
                        "activity": self.summary()["activity"]})

    # ----- queries (state, Sledgehammer, find theorems)

    async def state_query(self, client: "Client", path: str, offset: int, revision: Optional[int],
                          margin: float) -> Dict[str, Any]:
        backend = self.require_checking()
        doc = self.get_doc(path)
        pos = doc.map_offset(offset, revision)
        client.note_position(self, doc, pos)
        r = await backend.request({"op": "state", "file": doc.path, "offset": pos, "margin": margin})
        if "error" in r:
            raise HostError(r["error"])
        r.pop("reply", None)
        r["revision"] = doc.revision
        return r

    def start_query(self, client: "Client", path: str, offset: int, revision: Optional[int],
                    operation: str, args: List[str]) -> Dict[str, Any]:
        backend = self.require_checking()
        doc = self.get_doc(path)
        pos = doc.map_offset(offset, revision)
        qid = uuid.uuid4().hex[:10]
        self.queries[qid] = {"id": qid, "client": client.id, "path": doc.path, "operation": operation,
                             "args": args, "revision": doc.revision, "status": "waiting", "messages": [],
                             "started": now()}
        client.note_position(self, doc, pos)
        backend.send({"op": "query", "query": qid, "file": doc.path, "offset": pos,
                      "operation": operation, "args": args})
        return {"query": qid}

    def query_event(self, msg: Dict[str, Any]) -> None:
        q = self.queries.get(msg.get("query"))
        if not q:
            return
        doc = self.docs.get(q["path"])

        def mp(p: int, right: bool = False) -> int:
            return doc.map_offset(p, q["revision"], right) if doc else p

        for k in ("start", "end"):
            if k in msg:
                q[k] = mp(msg[k], k == "end")
        if msg.get("replace"):
            q["replace"] = {"start": mp(msg["replace"]["start"]), "end": mp(msg["replace"]["end"], True)}
        if "command" in msg:
            q["command"] = msg["command"]
        q["status"] = msg.get("status", q["status"])
        q["messages"] = msg.get("messages", q["messages"])
        if msg.get("error"):
            q["error"] = msg["error"]
        q["revision"] = doc.revision if doc else q["revision"]
        update = {"event": "query", "session": self.id, **{k: v for k, v in q.items() if k != "client"}}
        for c in self.host.clients:
            if c.id == q["client"]:
                c.push(update)
        if q["status"] in ("finished", "canceled", "failed"):
            q["done"] = now()
            for old in [k for k, v in self.queries.items() if v.get("done") and now() - v["done"] > 600]:
                self.queries.pop(old, None)

    def cancel_query(self, qid: str) -> None:
        q = self.queries.get(qid)
        if not q:
            raise HostError("No such query", "not_found")
        if self.backend and self.backend.ready:
            self.backend.send({"op": "cancel_query", "query": qid})

    # ----- filling a hole with an LLM, checked by the prover

    FACTS_PROMPT = """You are helping to set up Sledgehammer for an Isabelle/HOL proof. The theory uses
Isabelle {identifier} with base logic {logic}. Below is the start of the theory, the text before the
gap, the lemma with the gap marked {marker}, and the proof state at the gap.

Name up to 8 existing facts (lemmas, definitions such as foo_def, simp and intro rules) that a
proof search would most need to close the goal at {marker}: they go in a "using" line in front of
Sledgehammer to guide it. Only facts that exist in this context.{hint}

Answer at once with only a JSON object and nothing else:
{{"using": ["fact_name", "..."]}}

=== theory start ===
{header}
=== text before the gap ===
{before}
=== lemma ({marker} marks the gap) ===
{lemma}
=== proof state at the gap ===
{state}
"""

    async def _ask_llm(self, agent: str, model: Optional[str], prompt: str, cwd: str,
                       tag: str, conversation: Optional[str] = None, start: bool = False) -> Tuple[str, str]:
        """Ask `claude` or `codex` (a model, if named) without tools; its answer and errors. Stopping
        the task stops the LLM. With `conversation`, in that conversation: started with that id
        (claude) when `start`, else resumed"""
        self._started_conversation = None
        if agent == "codex":
            out_file: Optional[Path] = STATE_DIR / f"fill-{tag}.txt"
            if conversation and not start:
                cmd = ["codex", "exec", "resume", "--skip-git-repo-check", "-c", 'sandbox_mode="read-only"',
                       "-o", str(out_file)] + (["-m", model] if model else []) + [conversation, prompt]
            else:
                cmd = ["codex", "exec", "--skip-git-repo-check", "-s", "read-only", "-o", str(out_file)]
                cmd += (["--json"] if start else []) + (["-m", model] if model else []) + [prompt]
        else:
            out_file = None
            cmd = ["claude", "-p", prompt, "--output-format", "text",
                   "--disallowedTools", "Bash,Edit,Write,NotebookEdit,WebFetch,WebSearch"]
            cmd += ["--model", model] if model else []
            if conversation:
                cmd += ["--session-id" if start else "--resume", conversation]
        env = dict(os.environ)
        env["PATH"] = env.get("PATH", "") + ":" + str(HOME / ".local/bin")
        proc = await asyncio.create_subprocess_exec(
            *cmd, stdin=asyncio.subprocess.DEVNULL, stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE, cwd=cwd, env=env)
        try:
            out, err = await asyncio.wait_for(proc.communicate(), 300)
        except asyncio.TimeoutError:
            proc.kill()
            raise HostError(f"{agent} did not answer within 5 minutes")
        except asyncio.CancelledError:
            # (stopped, or its keyword deleted: the LLM stops too)
            with contextlib.suppress(ProcessLookupError):
                proc.kill()
            raise
        answer = out.decode(errors="replace")
        if agent == "codex" and start:
            # (its new conversation's id, from the JSON events)
            for line in answer.splitlines():
                with contextlib.suppress(ValueError):
                    ev = json.loads(line)
                    tid = ev.get("thread_id") or ev.get("session_id") or (ev.get("msg") or {}).get("session_id")
                    if tid:
                        self._started_conversation = str(tid)
                        break
        if out_file and out_file.exists():
            answer = out_file.read_text(errors="replace")
            out_file.unlink()
        if proc.returncode and not answer.strip():
            raise HostError(f"{agent} failed: " + err.decode(errors="replace").strip()[-300:])
        return answer, err.decode(errors="replace")

    def helper_key(self, agent: str, model: Optional[str]) -> str:
        return f"{agent}:{model or ''}"

    def helper_view(self, agent: str, model: Optional[str]) -> Optional[Dict[str, Any]]:
        h = (self.meta.get("helpers") or {}).get(self.helper_key(agent, model))
        if not h:
            return None
        folder = self.meta["folder"]
        resume = (f"cd {shlex.quote(folder)} && claude --resume {h['id']}" if agent == "claude"
                  else f"cd {shlex.quote(folder)} && codex resume {h['id']}")
        return {"id": h["id"], "agent": agent, "model": model, "queries": h.get("queries", 0), "resume": resume,
                "machine": socket.gethostname()}

    async def _ask_helper(self, client: "Client", job: str, agent: str, model: Optional[str],
                          prompt: str) -> Tuple[str, str]:
        """In the session's persistent conversation with that LLM (and model): it remembers the
        theory and earlier attempts; the human can open it to talk to it"""
        key = self.helper_key(agent, model)
        async with self.helper_locks.setdefault(key, asyncio.Lock()):
            helpers = self.meta.setdefault("helpers", {})
            h = helpers.get(key)
            cwd = self.meta["folder"]
            if h:
                try:
                    answer, err = await self._ask_llm(agent, model, prompt, cwd, job, conversation=h["id"])
                    h["queries"] = h.get("queries", 0) + 1
                    self.persist()
                    self._fill_push(client, job, status="asking", candidates=[], conversation=self.helper_view(agent, model))
                    return answer, err
                except HostError:
                    helpers.pop(key, None)  # (gone or broken: a new one)
            new_id = str(uuid.uuid4()) if agent == "claude" else None
            answer, err = await self._ask_llm(agent, model, prompt, cwd, job, conversation=new_id or "", start=True)
            new_id = new_id or self._started_conversation
            if new_id:
                helpers[key] = {"id": new_id, "agent": agent, "model": model, "queries": 1, "started": now()}
                self.persist()
                self._fill_push(client, job, status="asking", candidates=[], conversation=self.helper_view(agent, model))
            return answer, err

    # ----- external agents answering queries

    def live_listeners(self) -> List[Dict[str, Any]]:
        t = now()
        return [{"key": k, "label": v["label"]} for k, v in self.listeners.items()
                if v.get("waiting") or t - v.get("seen", 0) < 180]

    def resolve_listener(self, route: str) -> str:
        kind, _, want = route.partition(":")
        live = self.live_listeners()
        if kind == "agent":
            if any(l["key"] == want for l in live):
                return want
            raise HostError("The chosen agent is not listening now (it listens with isabelle_listen)")
        norm = lambda s: re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")
        for match in (lambda l: norm(l["label"]) == norm(want), lambda l: norm(l["label"]).startswith(norm(want)),
                      lambda l: norm(want) in norm(l["key"])):
            found = [l for l in live if match(l)]
            if found:
                return found[0]["key"]
        names = ", ".join("@" + norm(l["label"]) for l in live) or "none"
        raise HostError(f"No listening agent called @{want} (listening: {names})")

    async def _ask_agent(self, key: str, job: str, info: Dict[str, Any]) -> Dict[str, Any]:
        fut: asyncio.Future = asyncio.get_running_loop().create_future()
        self.agent_queries[job] = {"agent": key, "query": info, "future": fut, "delivered": False}
        self.listen_event.set()
        try:
            return await asyncio.wait_for(fut, 3600)
        except asyncio.TimeoutError:
            raise HostError("The agent did not answer within an hour")
        finally:
            q = self.agent_queries.pop(job, None)
            if q and not fut.done():
                self.withdrawn.setdefault(key, []).append(job)

    async def listen(self, client: "Client", timeout: float, name: str = "") -> Dict[str, Any]:
        """An agent waits for the next query for it (the human types `@name …`, or chose it for
        claude/codex queries); `name` is what the human calls it"""
        key = client.author()["key"]
        entry = self.listeners.setdefault(key, {})
        entry.update(label=(name.strip() or entry.get("label") or client.label)[:40], seen=now(), waiting=True)
        deadline = time.monotonic() + max(1.0, min(timeout, 55.0))
        try:
            while True:
                withdrawn = self.withdrawn.pop(key, [])
                for job, q in self.agent_queries.items():
                    if q["agent"] == key and not q["delivered"]:
                        q["delivered"] = True
                        return {"query": q["query"], "withdrawn": withdrawn}
                if withdrawn:
                    return {"query": None, "withdrawn": withdrawn}
                left = deadline - time.monotonic()
                if left <= 0:
                    return {"query": None, "withdrawn": []}
                self.listen_event.clear()
                with contextlib.suppress(asyncio.TimeoutError):
                    await asyncio.wait_for(self.listen_event.wait(), left)
        finally:
            entry.update(seen=now(), waiting=False)

    def propose(self, client: "Client", job: str, proofs: List[str], using: List[str], message: str) -> Dict[str, Any]:
        q = self.agent_queries.get(job)
        if not q:
            raise HostError("No such query waiting (answered, or withdrawn by the human)", "not_found")
        if q["agent"] != client.author()["key"]:
            raise HostError("That query was asked of another agent", "forbidden")
        if not q["future"].done():
            q["future"].set_result({"proofs": proofs, "using": using, "message": message})
        return {"accepted": True}

    FILL_PROMPT = """You are helping to finish an Isabelle/HOL proof. The theory uses Isabelle {identifier}
with base logic {logic}. Below is the start of the theory, the text before the gap, the lemma
with the gap marked {marker}, and the proof state at the gap.

Propose up to {n} different replacements for {marker}: each must be complete proof text that
closes the goal at that point, for example "by (simp add: foo_def)", "by auto", or an Isar
proof block ("proof ... qed"), indented like the surrounding text. Prefer short, robust proofs
using facts that exist in this context.{hint}

Also name up to 8 existing facts (lemmas, definitions such as foo_def, simp rules) that a proof
search would most need here: they go in a "using" line in front of Sledgehammer to guide it.

Answer with only a JSON object and nothing else:
{{"proofs": ["best proof", "..."], "using": ["fact_name", "..."]}}

=== theory start ===
{header}
=== text before the gap ===
{before}
=== lemma ({marker} marks the gap) ===
{lemma}
=== proof state at the gap ===
{state}
"""

    @staticmethod
    def parse_fill_answer(answer: str, n: int) -> Tuple[List[str], List[str]]:
        """Proofs and facts from an LLM's answer: {"proofs": [...], "using": [...]}, or a bare
        list of proofs"""
        def strings(v: Any) -> List[str]:
            return [str(x).strip() for x in v if str(x).strip()] if isinstance(v, list) else []
        i, j = answer.find("{"), answer.rfind("}")
        if i >= 0 and j > i:
            with contextlib.suppress(ValueError):
                obj = json.loads(answer[i:j + 1])
                if isinstance(obj, dict):
                    facts = [f for f in strings(obj.get("using")) if re.fullmatch(r"[\w.'()\[\],:= ]+", f)]
                    return strings(obj.get("proofs"))[:n], facts[:8]
        i, j = answer.find("["), answer.rfind("]")
        if i >= 0 and j > i:
            with contextlib.suppress(ValueError):
                return strings(json.loads(answer[i:j + 1]))[:n], []
        return [], []

    async def run_command_at(self, doc: Doc, offset: int, text: str, timeout: float = 60) -> List[str]:
        """Errors from running a diagnostic command at a position (deductum_command)"""
        qid = uuid.uuid4().hex[:10]
        self.queries[qid] = {"id": qid, "client": None, "path": doc.path, "operation": "deductum_command",
                             "args": [text], "revision": doc.revision, "status": "waiting", "messages": [],
                             "started": now()}
        assert self.backend
        self.backend.send({"op": "query", "query": qid, "file": doc.path, "offset": offset,
                           "operation": "deductum_command", "args": [text]})
        q = await self.await_query(qid, timeout)
        errors = [m["text"] for m in q.get("messages", []) if m.get("kind") == "error"]
        if q.get("status") != "finished" and not errors:
            errors.append(q.get("error") or "did not finish")
        return errors

    async def existing_facts(self, doc: Doc, offset: int, facts: List[str]) -> List[str]:
        if not await self.run_command_at(doc, offset, "thm " + " ".join(facts)):
            return facts
        kept = []
        for f in facts:
            if not await self.run_command_at(doc, offset, "thm " + f, 30):
                kept.append(f)
        return kept

    async def await_query(self, qid: str, timeout: float) -> Dict[str, Any]:
        t0 = time.monotonic()
        while time.monotonic() - t0 < timeout:
            q = self.queries.get(qid)
            if q and q.get("status") in ("finished", "failed", "canceled"):
                return q
            await asyncio.sleep(0.3)
        if self.backend and self.backend.ready:
            with contextlib.suppress(HostError):
                self.backend.send({"op": "cancel_query", "query": qid})
        return {"status": "timeout", "messages": [], "error": "timed out"}

    def fill_hole(self, client: "Client", path: str, offset: int, revision: Optional[int], agent: str,
                  hint: str, n: int = 4, hole: Optional[List[Any]] = None,
                  model: Optional[str] = None, route: Optional[str] = None) -> Dict[str, Any]:
        self.require_checking()
        doc = self.get_doc(path)
        pos = doc.map_offset(offset, revision)
        commands = doc.prover.get("commands") or []
        for c in ([] if hole else commands):
            if c[2] in ("sorry", "oops") and c[1] >= pos:
                hole = c
                break
        if hole is None:
            for c in reversed(commands):
                if c[2] in ("sorry", "oops") and c[0] <= pos:
                    hole = c
                    break
        if hole is None:
            raise HostError("No sorry or oops at or after the caret", "no_hole")
        job = uuid.uuid4().hex[:10]
        task = asyncio.get_running_loop().create_task(
            self._fill(client, job, doc, hole, agent, hint, n, model, route))
        self.fill_tasks[job] = task
        # Its first event says what it fills, for an app that did not ask (a typed keyword)
        self._fill_push(client, job, status="asking", candidates=[], agent=agent, hint=hint, model=model,
                        replace={"start": hole[0], "end": hole[1]}, revision=doc.revision)
        return {"job": job, "replace": {"start": hole[0], "end": hole[1]}, "revision": doc.revision}

    def _fill_push(self, client: "Client", job: str, **fields: Any) -> None:
        client.push({"event": "fill", "session": self.id, "job": job, **fields})

    async def _fill(self, client: "Client", job: str, doc: Doc, hole: List[Any], agent: str, hint: str,
                    n: int, model: Optional[str] = None, route: Optional[str] = None) -> None:
        facts_task: Optional[asyncio.Task] = None
        try:
            start, end = hole[0], hole[1]
            hole_rev = doc.revision
            text = doc.text
            # The lemma: back from the hole to the goal command that opens it
            goal_start = start
            kinds = self.syntax_kinds
            for c in reversed([c for c in (doc.prover.get("commands") or []) if c[0] < start]):
                goal_start = c[0]
                if kinds.get(c[2], "").startswith("thy_goal") or c[2] in ("lemma", "theorem", "corollary",
                                                                          "proposition", "schematic_goal"):
                    break
            marker = "<<<GAP>>>"
            lemma = text[goal_start:start] + marker + text[end:end + 1]
            header_end = text.find("begin")
            header = text[:header_end + 5] if header_end >= 0 else text[:400]
            before = text[max(0, goal_start - 6000):goal_start]
            self._fill_push(client, job, status="asking", candidates=[])
            state = ""
            if self.backend and self.backend.ready:
                r = await self.backend.request({"op": "state", "file": doc.path, "offset": max(0, start - 1),
                                                "margin": 100})
                state = "\n".join(m.get("text", "") for m in r.get("messages", []))
            prompt = self.FILL_PROMPT.format(
                identifier=self.meta.get("identifier") or "", logic=self.meta["logic"], marker=marker, n=n,
                hint=("\nThe user adds: " + hint) if hint.strip() else "", header=header, before=before,
                lemma=lemma, state=state or "(not available)")
            # The facts to guide Sledgehammer, asked for on their own as well: they come sooner than
            # proofs, and matter most when the LLM cannot find a proof itself
            facts_prompt = self.FACTS_PROMPT.format(
                identifier=self.meta.get("identifier") or "", logic=self.meta["logic"], marker=marker,
                hint=("\nThe user adds: " + hint) if hint.strip() else "", header=header, before=before,
                lemma=lemma, state=state or "(not available)")
            shared: Dict[str, Any] = {"status": "asking", "results": [], "using": []}

            def offer(using: List[str]) -> None:
                merged = list(dict.fromkeys(shared["using"] + using))[:10]
                if merged != shared["using"]:
                    shared["using"] = merged
                    self._fill_push(client, job, status=shared["status"], candidates=shared["results"], using=merged)

            async def early_facts() -> None:
                with contextlib.suppress(HostError, ValueError, OSError):
                    answer, _ = await self._ask_llm(agent, model, facts_prompt, os.path.dirname(doc.path), job + "-facts")
                    _, using = self.parse_fill_answer(answer, n)
                    if using:
                        offer(await self.existing_facts(doc, start, using))

            # Where the query goes: a listening agent (`@name`, or chosen for these keywords), the
            # session's persistent helper conversation, or a fresh request
            route = route or self.meta.get("query_route") or "helper"
            if route.startswith(("name:", "agent:")):
                key = self.resolve_listener(route)
                info = {"job": job, "path": doc.path, "line": ot.lines_cols(text, start)[0],
                        "lemma": text[goal_start:start].rstrip()[-4000:], "goal": state[:6000], "hint": hint,
                        "asked": doc.text[start:end]}
                self._fill_push(client, job, status="asking", candidates=[], agent="agent",
                                asked=self.listeners.get(key, {}).get("label") or key)
                answer_obj = await self._ask_agent(key, job, info)
                candidates = [p for p in answer_obj.get("proofs", []) if isinstance(p, str) and p.strip()][:8]
                using = [f for f in answer_obj.get("using", []) if isinstance(f, str)][:10]
                if answer_obj.get("message"):
                    self._fill_push(client, job, status="asking", candidates=[], message=str(answer_obj["message"])[:2000])
                if not candidates and not using:
                    raise HostError("The agent sent no proofs or facts" + (": " + str(answer_obj.get("message"))[:300]
                                                                         if answer_obj.get("message") else ""))
            else:
                facts_task = asyncio.get_running_loop().create_task(early_facts())
                try:
                    if route == "helper":
                        answer, err = await self._ask_helper(client, job, agent, model, prompt)
                    else:
                        answer, err = await self._ask_llm(agent, model, prompt, os.path.dirname(doc.path), job)
                except BaseException:
                    facts_task.cancel()
                    raise
                candidates, using = self.parse_fill_answer(answer, n)
                if not candidates and not using:
                    raise HostError(f"{agent} gave no candidates: " + (answer.strip()[:300] or err[:300]))
            results = [{"text": c, "ok": None, "message": ""} for c in candidates]
            shared.update(status="checking", results=results)
            self._fill_push(client, job, status="checking", candidates=results, using=shared["using"])
            # Its facts at once, before its proofs are checked (which can take minutes)
            if using:
                offer(await self.existing_facts(doc, start, using))
            # Proofs are tried in place of the hole's command, so the prover must have it (a hole
            # just typed, or an answer that came quickly)
            for _ in range(600):
                if (doc.prover.get("backend_revision") or 0) >= hole_rev and any(
                        c[0] <= start < c[1] for c in doc.prover.get("commands") or []):
                    break
                await asyncio.sleep(0.1)
            for res in results:
                qid = uuid.uuid4().hex[:10]
                self.queries[qid] = {"id": qid, "client": None, "path": doc.path, "operation": "deductum_command",
                                     "args": [res["text"]], "revision": doc.revision, "status": "waiting",
                                     "messages": [], "started": now()}
                assert self.backend
                self.backend.send({"op": "query", "query": qid, "file": doc.path, "offset": start,
                                   "operation": "deductum_command", "args": [res["text"]]})
                q = await self.await_query(qid, 120)
                # (a duplicate fact comes after the proof succeeded: see the prelude)
                errors = [m["text"] for m in q.get("messages", []) if m.get("kind") == "error"
                          and "Duplicate fact declaration" not in m["text"]]
                res["ok"] = q.get("status") == "finished" and not errors and not q.get("error")
                res["message"] = (errors[0] if errors else q.get("error") or "")[:400]
                self._fill_push(client, job, status="checking", candidates=results, using=shared["using"])
            results.sort(key=lambda r: not r["ok"])
            if facts_task:
                facts_task.cancel()
            self._fill_push(client, job, status="done", candidates=results, using=shared["using"])
        except asyncio.CancelledError:
            if facts_task:
                facts_task.cancel()
            raise
        except HostError as exn:
            self._fill_push(client, job, status="failed", error=str(exn), candidates=[])
        except Exception as exn:
            self._fill_push(client, job, status="failed", error=f"{type(exn).__name__}: {exn}", candidates=[])
        finally:
            self.fill_tasks.pop(job, None)

    async def cancel_all(self) -> int:
        backend = self.require_checking()
        r = await backend.request({"op": "cancel"})
        return int(r.get("canceled", 0))


# ---------------------------------------------------------------------------
# Clients


class Client:
    def __init__(self, host: "Host", reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
        self.host = host
        self.reader = reader
        self.writer = writer
        self.id = uuid.uuid4().hex[:8]
        self.kind = "cli"
        self.label = "client"
        self.identity: Optional[Dict[str, Any]] = None
        self.key: Optional[str] = None
        self.location: Dict[str, Any] = {}
        self.sessions: set = set()
        self.open_docs: set = set()
        self.positions: Dict[str, Dict[str, Any]] = {}
        # Where the human is, by session, as the app last said (design: collaboration)
        self.views: Dict[str, Dict[str, Any]] = {}
        self.closed = False

    def author(self) -> Dict[str, Any]:
        if self.kind == "agent":
            if not self.key:
                return {"kind": "agent", "key": "anonymous:" + self.id, "label": self.label}
            return {"kind": "agent", "key": self.key, "label": self.label}
        return HUMAN

    def describe(self, session_id: Optional[str] = None) -> Dict[str, Any]:
        return {"id": self.id, "kind": self.kind, "label": self.label, "key": self.key,
                "location": self.location,
                "position": self.positions.get(session_id) if session_id else None}

    def push(self, obj: Dict[str, Any]) -> None:
        if self.closed:
            return
        try:
            self.writer.write((json.dumps(obj, ensure_ascii=False) + "\n").encode())
        except Exception:
            self.closed = True

    def note_position(self, session: Session, doc: Doc, offset: int) -> None:
        line, col = ot.lines_cols(doc.text, min(offset, len(doc.text)))
        pos = {"path": doc.path, "offset": offset, "line": line, "column": col, "time": now()}
        self.positions[session.id] = pos
        self.attach(session)
        if self.kind == "agent":
            seen = session.meta["seen"].setdefault(self.key or ("client:" + self.id), {})
            seen.update({"label": self.label, "identity": self.identity, "location": self.location,
                         "last_seen": now(), "position": pos})
            session.persist()
            for c in session.attached_clients():
                if c.kind == "app":
                    c.push({"event": "agent_position", "session": session.id, "client": self.describe(session.id)})

    def attach(self, session: Session) -> None:
        if session.id not in self.sessions:
            self.sessions.add(session.id)
            if self.kind == "agent":
                seen = session.meta["seen"].setdefault(self.key or ("client:" + self.id), {})
                seen.update({"label": self.label, "identity": self.identity, "location": self.location,
                             "last_seen": now()})
                session.persist()
            self.host.broadcast_session(session)


# ---------------------------------------------------------------------------
# The host


class Host:
    def __init__(self) -> None:
        self.sessions: Dict[str, Session] = {}
        self.clients: List[Client] = []
        self.server: Optional[asyncio.AbstractServer] = None
        self.stopping = asyncio.Event()
        # Sessions that were running when the host last stopped: started again as it comes back
        self.resume_after_load: List[str] = []

    # ----- sessions on disk

    def load(self) -> None:
        for d in sorted((STATE_DIR / "sessions").glob("*")):
            meta = read_json(d / "meta.json")
            if not meta or meta.get("discarded"):
                continue
            s = Session(self, d.name, meta)
            s.load_docs()
            # A prover doesn't survive the host. A session that was running starts a new prover
            # (a new epoch, C3) as the host comes back, so that a restart or an update of the host
            # does not leave the human's work stopped; a paused one waits to be resumed
            if meta.pop("resume_on_start", False):
                self.resume_after_load.append(s.id)
                meta["message"] = None
            elif s.lifecycle not in ("Stopped", "Failed"):
                was = s.lifecycle
                meta["lifecycle"] = "Stopped"
                if was in ("Running", "Starting"):
                    # (the host ended without stopping it: a crash or a kill)
                    self.resume_after_load.append(s.id)
                    meta["message"] = None
                else:
                    meta["message"] = f"The session host restarted while the session was {was}; resume to " \
                                      "start a new prover"
            self.sessions[s.id] = s

    def session(self, sid: Any) -> Session:
        s = self.sessions.get(str(sid))
        if not s:
            raise HostError(f"No session {sid}", "not_found")
        return s

    def broadcast_session(self, session: Session) -> None:
        summary = session.summary()
        for c in self.clients:
            if c.kind in ("app", "cli") or session.id in c.sessions:
                c.push({"event": "session", "session": summary})

    def broadcast_doc(self, session: Session, doc: Doc, event: Dict[str, Any]) -> None:
        for c in self.clients:
            if (session.id, doc.path) in c.open_docs:
                c.push(event)

    def find_session(self, folder: str, runtime: str, logic: str) -> Optional[Session]:
        for s in self.sessions.values():
            if (s.meta["folder"] == folder and s.meta["runtime"] == runtime and s.meta["logic"] == logic
                    and not s.meta.get("owner")):
                return s
        return None

    def create_session(self, params: Dict[str, Any], owner: Optional[Dict[str, Any]] = None,
                       parent: Optional[Dict[str, Any]] = None) -> Session:
        folder = os.path.realpath(os.path.expanduser(params.get("folder") or ""))
        if not os.path.isdir(folder):
            raise HostError(f"No such folder: {folder}", "not_found")
        runtimes = find_runtimes()
        runtime = params.get("runtime")
        if runtime:
            runtime = os.path.realpath(os.path.expanduser(runtime))
        else:
            supported = [r for r in runtimes if r["supported"]]
            if not supported:
                raise HostError("No supported Isabelle runtime on this machine; Deductum needs "
                                "Isabelle2025 (CyPhyAssure). Found: " +
                                ", ".join(f"{r['identifier']} ({r['path']})" for r in runtimes),
                                "unsupported_runtime")
            runtime = supported[0]["path"]
        suggestion = suggest_logic(folder, runtime)
        logic = params.get("logic") or suggestion["logic"]
        dirs = params.get("dirs")
        if dirs is None:
            dirs = suggestion["dirs"]
        include = params.get("include_sessions")
        if include is None:
            include = suggestion["include_sessions"] if logic != suggestion.get("session") else []
        if not owner and not params.get("new"):
            existing = self.find_session(folder, runtime, logic)
            if existing:
                if params.get("start", True) and existing.lifecycle in ("Stopped", "Failed") \
                        and existing.meta.get("lifecycle_auto", True) and params.get("resume"):
                    existing.start()
                return existing
        sid = uuid.uuid4().hex[:8]
        meta = {
            "id": sid, "name": params.get("name") or Path(folder).name, "folder": folder,
            "runtime": runtime, "identifier": runtime_identifier(Path(runtime)), "logic": logic,
            "dirs": [os.path.realpath(d) for d in dirs], "include_sessions": include,
            "options": params.get("options", []), "owner": owner, "parent": parent, "created": now(),
            "lifecycle": "Stopped", "desired": "stopped",
        }
        s = Session(self, sid, meta)
        s.dir.mkdir(parents=True, exist_ok=True)
        self.sessions[sid] = s
        s.persist_now()
        if params.get("start", True):
            s.start()
        else:
            self.broadcast_session(s)
        return s

    # ----- independent sessions (C8)

    def fork_session(self, client: Client, params: Dict[str, Any]) -> Session:
        if client.kind == "agent" and not client.key:
            raise HostError("Independent sessions need an agent identity (a conversation id)", "no_identity")
        if params.get("session"):
            source = self.session(params["session"])
            folder = source.meta["folder"]
            base_params = {k: source.meta[k] for k in ("runtime", "logic", "include_sessions", "options")}
            dirs = source.meta.get("dirs", [])
        else:
            source = None
            folder = os.path.realpath(os.path.expanduser(params.get("folder") or ""))
            base_params = {k: params[k] for k in ("runtime", "logic", "include_sessions") if params.get(k)}
            dirs = params.get("dirs") or suggest_logic(folder, params.get("runtime"))["dirs"]
        try:
            top = git("rev-parse", "--show-toplevel", cwd=folder).strip()
        except HostError:
            raise HostError("Independent sessions need the folder to be in a git repository", "no_git")
        head = git("rev-parse", "HEAD", cwd=top).strip()
        sid = uuid.uuid4().hex[:8]
        branch = f"deductum/{sid}"
        worktree = STATE_DIR / "worktrees" / f"{Path(top).name}-{sid}"
        worktree.parent.mkdir(parents=True, exist_ok=True)
        git("worktree", "add", "-b", branch, str(worktree), head, cwd=top)

        def moved(p: str) -> str:
            return str(worktree / os.path.relpath(p, top)) if path_inside(p, top) else p

        base: Dict[str, Any] = {}
        if source:
            # Start from the human's live buffers, unsaved edits included
            for doc in source.docs.values():
                if path_inside(doc.path, top):
                    rel = os.path.relpath(doc.path, top)
                    target = worktree / rel
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(encode_file(doc.text, doc.crlf, doc.bom))
                    base[rel] = {"text": doc.text, "revision": doc.revision}
        owner = client.author() if client.kind == "agent" else {"kind": "agent", "key": params.get("agent"),
                                                                  "label": params.get("label", "agent")}
        if not owner.get("key"):
            raise HostError("An independent session needs an owning agent")
        params2 = dict(base_params, folder=moved(folder), dirs=[moved(d) for d in dirs],
                       name=params.get("name") or f"{Path(folder).name} ({owner['label']})", new=True,
                       start=params.get("start", True))
        parent = {"session": source.id if source else None, "folder": folder, "repo": top,
                  "worktree": str(worktree), "branch": branch, "head": head, "base": base}
        s = self.create_session(params2, owner=owner, parent=parent)
        client.attach(s)
        return s

    def merge_preview(self, s: Session) -> List[Dict[str, Any]]:
        parent = s.meta.get("parent")
        if not parent:
            raise HostError("Not an independent session", "bad_state")
        wt, top = parent["worktree"], parent["repo"]
        out = []
        for p in list_theories(s.meta["folder"]):
            if not p.endswith(".thy"):
                continue
            rel = os.path.relpath(p, wt)
            doc = s.docs.get(os.path.realpath(p))
            theirs = doc.text if doc else (read_file(p) or ("", False, False))[0]
            if rel in parent["base"]:
                base = parent["base"][rel]["text"]
            else:
                try:
                    base = git("show", f"{parent['head']}:{rel}", cwd=top).replace("\r\n", "\n")
                except HostError:
                    base = ""
            if theirs != base:
                diff = "".join(difflib.unified_diff(base.splitlines(True), theirs.splitlines(True),
                                                    "a/" + rel, "b/" + rel, n=2))
                out.append({"rel": rel, "path": os.path.join(top, rel), "base": base, "theirs": theirs,
                            "diff": diff})
        return out

    def merge_apply(self, client: Client, s: Session, rels: Optional[List[str]]) -> Dict[str, Any]:
        parent = s.meta["parent"]
        target = self.sessions.get(parent.get("session") or "")
        if target is None:
            target = self.create_session({"folder": parent["folder"], "runtime": s.meta["runtime"],
                                          "logic": s.meta["logic"], "start": False})
            parent["session"] = target.id
        owner = s.meta["owner"]
        results = []
        for item in self.merge_preview(s):
            if rels is not None and item["rel"] not in rels:
                continue
            path = item["path"]
            if not os.path.exists(path):
                Path(path).parent.mkdir(parents=True, exist_ok=True)
                Path(path).write_text("")
            doc = target.get_doc(path)
            agent_change = line_change(item["base"], item["theirs"])
            human_change = line_change(item["base"], doc.text)
            kept, dropped = ot.rebase_through(agent_change, [human_change], "partial")
            entry = target.commit(doc, owner, "merge", kept, client=client) if kept else None
            if dropped:
                pieces = [{"at": ot.map_pos(a, human_change), "restore": t,
                           "line": ot.lines_cols(doc.text, min(ot.map_pos(a, human_change), len(doc.text)))[0],
                           "current": ""} for a, b, t in dropped]
                doc.alternatives.append({"id": uuid.uuid4().hex[:8], "path": doc.path, "kind": "merge",
                                         "target": entry["id"] if entry else None, "pieces": pieces,
                                         "time": now()})
            parent["base"][item["rel"]] = {"text": item["theirs"]}
            results.append({"rel": item["rel"], "entry": entry["id"] if entry else None,
                            "conflicts": len(dropped)})
        s.persist()
        return {"session": target.id, "files": results}

    async def discard_session(self, s: Session, force: bool) -> None:
        if s.meta.get("parent") and not force and self.merge_preview(s):
            raise HostError("This independent session has changes that were not merged", "unmerged")
        await s.stop()
        parent = s.meta.get("parent")
        if parent:
            with contextlib.suppress(HostError):
                git("worktree", "remove", "--force", parent["worktree"], cwd=parent["repo"])
            with contextlib.suppress(HostError):
                git("branch", "-D", parent["branch"], cwd=parent["repo"])
        s.meta["discarded"] = True
        write_json_atomic(s.dir / "meta.json", s.meta)
        shutil.rmtree(s.dir, ignore_errors=True)
        self.sessions.pop(s.id, None)
        for c in self.clients:
            c.sessions.discard(s.id)
            c.push({"event": "session_removed", "session": s.id})

    # ----- requests

    def check_lifecycle_rights(self, client: Client, s: Session) -> None:
        if client.kind == "agent":
            owner = s.meta.get("owner")
            if not owner or owner.get("key") != client.key:
                raise HostError("Only the human controls this session's prover; you can control your "
                                "own independent sessions", "forbidden")

    async def handle(self, client: Client, req: Dict[str, Any]) -> Dict[str, Any]:
        op = req.get("op")
        p = req
        if op == "hello":
            client.kind = p.get("client", "cli")
            client.label = p.get("label") or client.kind
            ident = p.get("identity") or None
            if ident and ident.get("provider") and ident.get("conversation"):
                client.identity = ident
                client.key = f"{ident['provider']}:{ident['conversation']}"
            client.location = p.get("location") or {}
            return {"protocol": PROTOCOL, "version": VERSION, "client": client.id, "key": client.key,
                    "machine": socket.gethostname(), "home": str(HOME), "runtimes": find_runtimes()}
        if op == "ping":
            return {"pong": True}
        # A client on another machine names files as that machine does; here they are reached
        # through COSMIC Terminal's sshfs mount of it
        for key in ("path", "folder"):
            if isinstance(p.get(key), str):
                p[key] = translate_path(p[key], client.location.get("machine"))
        if op == "runtimes":
            return {"runtimes": find_runtimes()}
        if op == "suggest":
            folder = os.path.realpath(os.path.expanduser(p["folder"]))
            runtimes = find_runtimes()
            runtime = p.get("runtime") or next((r["path"] for r in runtimes if r["supported"]), None)
            suggestion = await asyncio.to_thread(suggest_logic, folder, runtime)
            return {"suggestion": suggestion, "runtimes": runtimes}
        if op == "list_dir":
            path = os.path.realpath(os.path.expanduser(p.get("path") or str(HOME)))
            entries = []
            with contextlib.suppress(OSError):
                for e in sorted(os.scandir(path), key=lambda e: e.name.lower()):
                    if e.name.startswith("."):
                        continue
                    if e.is_dir():
                        entries.append({"name": e.name, "dir": True})
                    elif e.name.endswith(".thy") or e.name == "ROOT":
                        entries.append({"name": e.name, "dir": False})
            return {"path": path, "parent": str(Path(path).parent), "entries": entries}
        if op == "sessions":
            return {"sessions": [s.summary() for s in self.sessions.values()]}
        if op == "symbol_table":
            runtime = p.get("runtime")
            if not runtime and p.get("session") in self.sessions:
                runtime = self.sessions[p["session"]].meta["runtime"]
            if not runtime:
                supported = [r for r in find_runtimes() if r["supported"]] or find_runtimes()
                if not supported:
                    return {"symbols": []}
                runtime = supported[0]["path"]
            return {"symbols": await asyncio.to_thread(symbol_table, runtime)}
        if op == "session_create":
            # Warm the runtime's session list off the event loop; suggestions use it
            supported = [r["path"] for r in find_runtimes() if r["supported"]]
            runtime = p.get("runtime") or (supported[0] if supported else None)
            await asyncio.to_thread(known_sessions, runtime)
            if client.kind == "agent" and p.get("independent"):
                s = self.fork_session(client, p)
            else:
                s = self.create_session(p)
            client.attach(s)
            return {"session": s.summary()}
        if op == "session_fork":
            s = self.fork_session(client, p)
            return {"session": s.summary()}
        if op == "find_sessions":
            return {"sessions": self.find_sessions(p)}
        if op == "notices":
            return {"notices": self.take_notices(client, p.get("ack", True), p.get("session"))}
        if op == "shutdown":
            if client.kind == "agent":
                raise HostError("Agents cannot stop the host", "forbidden")
            for s in self.sessions.values():
                # (running when the host stops: running again when it comes back)
                s.meta["resume_on_start"] = s.lifecycle in ("Running", "Starting")
                await s.stop()
                s.persist_now()
            self.stopping.set()
            return {}

        s = self.session(p.get("session"))
        if op == "session_info":
            client.attach(s)
            info = s.summary()
            info["theories"] = [{"path": t, "status": s.theory_status.get(t),
                                 "dirty": s.docs[t].dirty if t in s.docs else False,
                                 "conflict": bool(s.docs[t].conflict) if t in s.docs else False}
                                for t in list_theories(s.meta["folder"])]
            info["docs"] = [d.summary() for d in s.docs.values()]
            info["logs"] = s.logs[-100:]
            info["seen"] = s.meta["seen"]
            info["alternatives"] = [a for d in s.docs.values() for a in d.alternatives[-10:]]
            return {"session": info}
        if op == "attach":
            client.attach(s)
            return {"session": s.summary()}
        if op == "detach":
            client.sessions.discard(s.id)
            client.open_docs = {k for k in client.open_docs if k[0] != s.id}
            s.save_all()
            self.broadcast_session(s)
            return {}
        if op in ("stop", "pause", "resume", "start"):
            self.check_lifecycle_rights(client, s)
            if op == "stop":
                await s.stop()
            elif op == "pause":
                s.pause()
            elif op == "resume" or op == "start":
                s.resume() if s.lifecycle != "Stopped" or op == "resume" else s.start()
            return {"session": s.summary()}
        if op == "discard":
            if client.kind == "agent":
                self.check_lifecycle_rights(client, s)
            await self.discard_session(s, bool(p.get("force")))
            return {}
        if op == "configure":
            if client.kind == "agent":
                raise HostError("Only the human configures sessions", "forbidden")
            for k in ("logic", "dirs", "include_sessions", "name", "options", "runtime"):
                if k in p:
                    s.meta[k] = p[k]
            s.persist()
            self.broadcast_session(s)
            return {"session": s.summary()}
        if op == "read":
            if p.get("peek"):
                # A theory to look at (a reference window): one outside the session's folder, such
                # as a library theory, is read from disk and not opened as a document of the session
                path = os.path.realpath(os.path.expanduser(p["path"]))
                if path not in s.docs and not path_inside(path, s.meta["folder"]):
                    try:
                        text = Path(path).read_text()
                    except OSError as e:
                        raise HostError(f"Cannot read {path}: {e}", "not_found")
                    return {"doc": {"path": path, "text": text, "revision": 0, "prover": None, "library": True}}
                doc = s.get_doc(path)
                client.attach(s)
                return {"doc": s.doc_view(doc)}
            doc = s.get_doc(p["path"])
            client.attach(s)
            if p.get("subscribe", client.kind == "app"):
                client.open_docs.add((s.id, doc.path))
            return {"doc": s.doc_view(doc, with_conflict_texts=bool(p.get("conflict_texts")))}
        if op == "close":
            client.open_docs.discard((s.id, os.path.realpath(p["path"])))
            return {}
        if op == "edit":
            return s.client_edit(client, p["path"], int(p.get("revision", 0)), p.get("edits", []),
                                 p.get("policy") or ("merge" if client.kind == "app" else "strict"),
                                 p.get("txn"), p.get("request"))
        if op == "undo":
            return s.undo(client, p["path"])
        if op == "redo":
            return s.redo(client, p["path"])
        if op == "revert":
            return s.revert(client, p["path"], int(p["entry"]))
        if op == "history":
            return {"history": s.history(p["path"], int(p.get("limit", 100)), p.get("author"))}
        if op == "save":
            paths = [os.path.realpath(p["path"])] if p.get("path") else list(s.docs)
            failed = []
            for path in paths:
                doc = s.docs.get(path)
                if doc is None:
                    continue
                if client.kind == "agent" and not s.agent_may_edit(client.author()["key"], doc, []):
                    raise HostError("You have no editing rights for " + Path(path).name, "forbidden")
                if not s.save_doc(doc, force=True):
                    failed.append({"path": path, "error": doc.save_error or
                                   ("changed on disk; resolve the conflict first" if doc.conflict else "")})
            return {"failed": failed}
        if op == "resolve":
            return s.resolve(client, p["path"], p["choice"], p.get("text"))
        if op == "dismiss_alternative":
            doc = s.get_doc(p["path"])
            doc.alternatives = [a for a in doc.alternatives if a["id"] != p["id"]]
            s.persist(doc)
            return {}
        if op == "info":
            # Hover information and the definition at a position
            backend = s.require_checking()
            doc = s.get_doc(p["path"])
            pos = doc.map_offset(int(p.get("offset", 0)), p.get("revision"))
            r = (await backend.request({"op": "info", "file": doc.path, "offset": pos})).get("info") or {}
            d = r.get("definition")
            if d and d.get("file"):
                # A place in a theory of the session: as an offset there too
                target = s.docs.get(os.path.realpath(d["file"]))
                text = target.text if target else None
                if text is None:
                    with contextlib.suppress(OSError):
                        text = Path(d["file"]).read_text()
                if text is not None:
                    d["offset"] = ot.offset_of(text, int(d.get("line", 1)), int(d.get("column", 0)))
            return {"info": r, "revision": doc.revision}
        if op == "state":
            return await s.state_query(client, p["path"], int(p.get("offset", 0)), p.get("revision"),
                                       float(p.get("margin", 100)))
        if op == "query":
            return s.start_query(client, p["path"], int(p.get("offset", 0)), p.get("revision"),
                                 p["operation"], [str(a) for a in p.get("args", [])])
        if op == "query_status":
            q = s.queries.get(p["query"])
            if not q:
                raise HostError("No such query", "not_found")
            return {"query": {k: v for k, v in q.items() if k != "client"}}
        if op == "cancel_query":
            s.cancel_query(p["query"])
            return {}
        if op == "cancel":
            return {"canceled": await s.cancel_all()}
        if op == "activity":
            return {"activity": s.activity()}
        if op == "fill_hole":
            return s.fill_hole(client, p["path"], int(p.get("offset", 0)), p.get("revision"),
                               p.get("agent", "claude"), p.get("hint", ""), int(p.get("n", 4)))
        if op == "listen":
            if client.kind != "agent":
                raise HostError("Agents listen for queries", "forbidden")
            return await s.listen(client, float(p.get("timeout", 50)), str(p.get("name") or ""))
        if op == "propose":
            return s.propose(client, p["query"], [str(x) for x in p.get("proofs") or []],
                             [str(x) for x in p.get("using") or []], str(p.get("message") or ""))
        if op == "query_route":
            if client.kind != "app":
                raise HostError("The human chooses where queries go", "forbidden")
            route = str(p.get("route") or "helper")
            if not (route in ("helper", "fresh") or route.startswith("agent:")):
                raise HostError("Unknown route: " + route)
            s.meta["query_route"] = route
            s.persist()
            s.push_activity()
            return {"route": route}
        if op == "helper_reset":
            if client.kind != "app":
                raise HostError("Only the human resets the helper", "forbidden")
            (s.meta.get("helpers") or {}).pop(s.helper_key(p.get("agent", "claude"), p.get("model") or None), None)
            s.persist()
            return {}
        if op == "cancel_fill":
            s.keyword_holes.pop(p.get("job", ""), None)
            task = s.fill_tasks.pop(p.get("job", ""), None)
            if task:
                task.cancel()
            return {}
        if op == "prover_options":
            # Options of the running prover, such as Sledgehammer's timeout and provers;
            # kept with the session and set again on each start
            allowed = ("sledgehammer_timeout", "sledgehammer_provers")
            settings = {k: str(v) for k, v in (p.get("set") or {}).items() if k in allowed}
            s.meta.setdefault("prover_options", {}).update(settings)
            s.persist()
            if s.backend and s.backend.ready:
                await s.backend.request({"op": "options", "set": [f"{k}={v}" for k, v in settings.items()]})
            return {"options": s.meta["prover_options"]}
        if op == "grant":
            if client.kind == "agent":
                raise HostError("Only the human grants editing rights", "forbidden")
            return {"grant": s.grant(p["agent"], p.get("label", p["agent"]), p.get("scope", "folder"),
                                     p.get("path"), p.get("offset"))}
        if op == "view":
            # The app says where the human is (sent as the caret moves)
            client.views[s.id] = {"path": os.path.realpath(p.get("path") or ""), "offset": int(p.get("offset", 0)),
                                  "selection": (p.get("selection") or "")[:4000], "mode": p.get("mode"),
                                  "construct": p.get("construct"), "time": now()}
            return {}
        if op == "human_view":
            return {"view": s.human_view()}
        if op == "show":
            doc = s.get_doc(p["path"])
            start = doc.map_offset(int(p.get("start", 0)), p.get("revision"))
            end = doc.map_offset(int(p.get("end", p.get("start", 0))), p.get("revision"), True)
            return s.show_human(client, doc.path, start, end, p.get("message", ""))
        if op == "add_note":
            doc = s.get_doc(p["path"])
            start = doc.map_offset(int(p.get("start", 0)), p.get("revision"))
            end = doc.map_offset(int(p.get("end", p.get("start", 0))), p.get("revision"), True)
            return {"note": s.add_note(client, doc.path, start, end, p.get("text", ""))}
        if op == "notes":
            return {"notes": s.notes(p.get("path"))}
        if op == "reply_note":
            return s.reply_note(client, p["note"], p.get("text", ""), bool(p.get("resolve")))
        if op == "keyword":
            # The human paused on a line reading `claude …` or `codex …`: as Enter would
            s.require_checking()
            if client.kind != "app":
                raise HostError("Only the human's app asks this", "forbidden")
            doc = s.get_doc(p["path"])
            pos = doc.map_offset(int(p.get("offset", 0)), p.get("revision"))
            line_start = doc.text.rfind("\n", 0, pos) + 1
            return {"started": s.keyword_line(client, doc, line_start, client.author(), uuid.uuid4().hex[:12])}
        if op == "cancel_tools":
            backend = s.require_checking()
            return await backend.request({"op": "cancel_tools"})
        if op == "request_access":
            if client.kind != "agent":
                raise HostError("Only agents ask for editing rights", "forbidden")
            return {"request": s.request_access(client, p.get("scope", "theory"), p.get("path"), p.get("offset"),
                                                p.get("reason", ""))}
        if op == "answer_request":
            if client.kind == "agent":
                raise HostError("Only the human answers requests for editing rights", "forbidden")
            return s.answer_request(p["request"], bool(p.get("allow")))
        if op == "revoke":
            if client.kind == "agent":
                raise HostError("Only the human revokes editing rights", "forbidden")
            return {"grant": s.revoke(p["grant"])}
        if op == "merge_preview":
            return {"files": [{k: v for k, v in f.items() if k in ("rel", "path", "diff")}
                              for f in self.merge_preview(s)]}
        if op == "merge_apply":
            if client.kind == "agent":
                raise HostError("The human reviews and merges independent work", "forbidden")
            return self.merge_apply(client, s, p.get("files"))
        if op == "symbols":
            backend = s.require_checking() if s.lifecycle != "Paused" else s.backend
            if backend is None:
                raise HostError("The prover is not running", "stopped")
            r = await backend.request({"op": "symbols"})
            return {"symbols": r.get("symbols", [])}
        if op == "syntax":
            return {"kinds": s.syntax_kinds}
        raise HostError(f"Unknown operation {op}", "unknown_op")

    def find_sessions(self, p: Dict[str, Any]) -> List[Dict[str, Any]]:
        out = []
        folder = os.path.realpath(os.path.expanduser(p["folder"])) if p.get("folder") else None
        for s in self.sessions.values():
            if folder and (path_inside(folder, s.meta["folder"]) or path_inside(s.meta["folder"], folder)):
                out.append({"session": s.summary(), "agent": None, "position": None,
                            "last_seen": s.meta.get("created"), "by": "folder"})
            for key, seen in s.meta["seen"].items():
                loc = seen.get("location") or {}
                match = (p.get("key") and key == p["key"]) or \
                        (p.get("conversation") and key.endswith(":" + str(p["conversation"]))) or \
                        (p.get("zellij") and loc.get("zellij") == p["zellij"]) or \
                        (p.get("tab") and loc.get("tab") == p["tab"])
                if match:
                    out.append({"session": s.summary(), "agent": key, "position": seen.get("position"),
                                "last_seen": seen.get("last_seen")})
        out.sort(key=lambda r: -(r["last_seen"] or 0))
        return out

    def take_notices(self, client: Client, ack: bool, session_id: Optional[str]) -> List[Dict[str, Any]]:
        if not client.key:
            return []
        out = []
        for s in self.sessions.values():
            if session_id and s.id != session_id:
                continue
            lst = s.meta["notices"].get(client.key, [])
            pending = [n for n in lst if not n.get("delivered")]
            out.extend(pending)
            if ack and pending:
                for n in pending:
                    n["delivered"] = True
                s.persist()
        return out

    # ----- connections

    async def serve_client(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        client = Client(self, reader, writer)
        self.clients.append(client)
        try:
            while True:
                line = await reader.readline()
                if not line:
                    break
                try:
                    req = json.loads(line)
                except ValueError:
                    client.push({"error": "bad json", "code": "bad_request"})
                    continue
                rid = req.get("id")
                try:
                    result = await self.handle(client, req)
                    client.push({"id": rid, "ok": True, **result})
                except HostError as exn:
                    client.push({"id": rid, "ok": False, "error": str(exn), "code": exn.code, **exn.data})
                except Exception as exn:
                    log("request failed", req.get("op"), repr(exn))
                    import traceback
                    traceback.print_exc()
                    client.push({"id": rid, "ok": False, "error": f"{type(exn).__name__}: {exn}",
                                 "code": "internal"})
                with contextlib.suppress(Exception):
                    await writer.drain()
        except (ConnectionError, asyncio.IncompleteReadError):
            pass
        finally:
            client.closed = True
            self.clients.remove(client)
            for sid in list(client.sessions):
                s = self.sessions.get(sid)
                if s:
                    if client.kind == "app":
                        s.save_all()
                    self.broadcast_session(s)
            with contextlib.suppress(Exception):
                writer.close()

    async def watch_loop(self) -> None:
        tick = 0
        while not self.stopping.is_set():
            tick += 1
            for s in list(self.sessions.values()):
                try:
                    s.watch()
                    if tick % 2 == 0:
                        s.push_activity()
                except Exception as exn:
                    log("watch failed", s.id, repr(exn))
            await asyncio.sleep(WATCH_INTERVAL)

    async def run(self) -> None:
        STATE_DIR.mkdir(parents=True, exist_ok=True)
        self.load()
        with contextlib.suppress(FileNotFoundError):
            SOCKET_PATH.unlink()
        self.server = await asyncio.start_unix_server(self.serve_client, path=str(SOCKET_PATH),
                                                      limit=64 * 1024 * 1024)
        os.chmod(SOCKET_PATH, 0o600)
        log("deductum host", VERSION, "listening on", SOCKET_PATH)
        loop = asyncio.get_running_loop()
        for sig in (signal.SIGTERM, signal.SIGINT):
            loop.add_signal_handler(sig, self.stopping.set)
        watcher = asyncio.create_task(self.watch_loop())
        for sid in self.resume_after_load:
            with contextlib.suppress(Exception):
                log("resuming", sid, "after the host restarted")
                self.sessions[sid].start()
        self.resume_after_load.clear()
        await self.stopping.wait()
        watcher.cancel()
        for s in list(self.sessions.values()):
            s.save_all()
            # (running when the host stops: running again when it comes back; a session the
            # shutdown request already stopped keeps what it said)
            if s.lifecycle in ("Running", "Starting"):
                s.meta["resume_on_start"] = True
            with contextlib.suppress(Exception):
                await s.stop()
            s.persist_now()
        self.server.close()
        with contextlib.suppress(FileNotFoundError):
            SOCKET_PATH.unlink()


# ---------------------------------------------------------------------------
# Entry points


def run_daemon(to_log: bool = False) -> int:
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    if to_log:
        logf = open(LOG_PATH, "ab", buffering=0)
        os.dup2(logf.fileno(), 1)
        os.dup2(logf.fileno(), 2)
    lock = open(LOCK_PATH, "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        print("deductum host already running", file=sys.stderr)
        return 0
    asyncio.run(Host().run())
    return 0


def host_running() -> bool:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        s.connect(str(SOCKET_PATH))
        return True
    except OSError:
        return False
    finally:
        s.close()


# Proof tools typed in the text: their findings come with Insert, and they stop when deleted
TOOLS = ("sledgehammer", "try", "try0", "nitpick", "quickcheck", "solve_direct", "nunchaku")


def translate_path(path: str, machine: Optional[str]) -> str:
    """A path as a client on `machine` gave it, as this machine reaches it: unchanged if it
    exists here, else through the mount of that machine (~/.cache/cosmic-term/mounts/NAME)"""
    p = os.path.expanduser(path)
    if not os.path.isabs(p) or os.path.exists(p) or not machine or machine == socket.gethostname():
        return p
    alt = HOME / ".cache" / "cosmic-term" / "mounts" / machine.split(".")[0] / p.lstrip("/")
    return str(alt) if alt.exists() else p


def split_at(change: ot.Change, others: ot.Change) -> ot.Change:
    """A change cut where another change of the same text starts and ends, so that rebasing
    it drops only the characters both changed: (s, e, t) is the insertion of t at s, then
    the deletion of [s, e) in pieces"""
    cuts = sorted({p for s, e, _ in others for p in (s, e)})
    out: ot.Change = []
    for s, e, t in change:
        inner = [p for p in cuts if s < p < e]
        if not inner:
            out.append((s, e, t))
            continue
        if t:
            out.append((s, s, t))
        points = [s] + inner + [e]
        out.extend((a, b, "") for a, b in zip(points, points[1:]))
    return out


def spawn_daemon() -> None:
    """Start the host outside the client's process group and, where systemd manages the
    user's processes, in its own unit: closing or restarting a client (whose unit may be
    stopped with everything in it) must never end the host or its provers (design C2/C3)"""
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    script = str(Path(__file__).resolve())
    started = False
    if shutil.which("systemd-run") and os.environ.get("XDG_RUNTIME_DIR"):
        env_args = [f"--setenv={k}={os.environ[k]}" for k in ("DEDUCTUM_STATE_DIR", "USER_HOME", "PATH",
                                                                "DEDUCTUM_ISABELLE") if os.environ.get(k)]
        for unit in ("deductum-host", f"deductum-host-{int(time.time())}"):
            r = subprocess.run(["systemd-run", "--user", "--collect", "--quiet", f"--unit={unit}", *env_args,
                                sys.executable, script, "daemon", "--log"],
                               capture_output=True, text=True, timeout=30)
            if r.returncode == 0:
                started = True
                break
    if not started:
        logf = open(LOG_PATH, "ab")
        subprocess.Popen([sys.executable, script, "daemon"], stdin=subprocess.DEVNULL,
                         stdout=logf, stderr=logf, start_new_session=True, close_fds=True)
    for _ in range(100):
        if host_running():
            return
        time.sleep(0.1)
    raise SystemExit("deductum host did not start; see " + str(LOG_PATH))


def connect_socket() -> socket.socket:
    if not host_running():
        spawn_daemon()
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(str(SOCKET_PATH))
    return s


def run_connect() -> int:
    """Bridge stdio to the host's socket, for clients on other machines (over ssh)"""
    import selectors
    sock = connect_socket()
    sel = selectors.DefaultSelector()
    stdin = sys.stdin.buffer.raw if hasattr(sys.stdin.buffer, "raw") else sys.stdin.buffer
    sel.register(stdin, selectors.EVENT_READ, "in")
    sel.register(sock, selectors.EVENT_READ, "sock")
    out = sys.stdout.buffer
    while True:
        for key, _ in sel.select():
            if key.data == "in":
                data = os.read(stdin.fileno(), 65536)
                if not data:
                    sock.shutdown(socket.SHUT_WR)
                    sel.unregister(stdin)
                    continue
                sock.sendall(data)
            else:
                data = sock.recv(1 << 20)
                if not data:
                    return 0
                out.write(data)
                out.flush()


class SyncClient:
    """A blocking client for the command line and the MCP adapter"""

    def __init__(self, sock: Optional[socket.socket] = None, proc: Optional[subprocess.Popen] = None):
        self.sock = sock
        self.proc = proc
        self.buf = b""
        self.next_id = 1
        self.events: List[Dict[str, Any]] = []

    @classmethod
    def local(cls) -> "SyncClient":
        return cls(sock=connect_socket())

    @classmethod
    def over_ssh(cls, destination: str, remote_python: str = "python3",
                 remote_script: str = "~/.local/share/deductum/host/deductum_host.py") -> "SyncClient":
        proc = subprocess.Popen(["ssh", "-o", "BatchMode=yes", "-T", destination,
                                 f"{remote_python} {remote_script} connect"],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        return cls(proc=proc)

    def _send(self, data: bytes) -> None:
        if self.sock:
            self.sock.sendall(data)
        else:
            assert self.proc and self.proc.stdin
            self.proc.stdin.write(data)
            self.proc.stdin.flush()

    def _recv(self) -> bytes:
        if self.sock:
            return self.sock.recv(1 << 20)
        assert self.proc and self.proc.stdout
        return self.proc.stdout.read1(1 << 20)  # type: ignore[attr-defined]

    def read_message(self) -> Dict[str, Any]:
        while b"\n" not in self.buf:
            data = self._recv()
            if not data:
                raise ConnectionError("deductum host closed the connection")
            self.buf += data
        line, self.buf = self.buf.split(b"\n", 1)
        return json.loads(line)

    def request(self, op: str, **params: Any) -> Dict[str, Any]:
        rid = self.next_id
        self.next_id += 1
        self._send((json.dumps(dict(params, op=op, id=rid)) + "\n").encode())
        while True:
            msg = self.read_message()
            if msg.get("id") == rid:
                if not msg.get("ok"):
                    raise HostError(msg.get("error", "error"), msg.get("code", "error"),
                                    **{k: v for k, v in msg.items() if k not in ("id", "ok", "error", "code")})
                return msg
            self.events.append(msg)
            del self.events[:-500]


def main(argv: List[str]) -> int:
    cmd = argv[1] if len(argv) > 1 else "help"
    if cmd == "daemon":
        return run_daemon("--log" in argv[2:])
    if cmd == "connect":
        return run_connect()
    if cmd == "mcp":
        import deductum_mcp
        return deductum_mcp.main(argv[2:])
    if cmd in ("status", "sessions"):
        c = SyncClient.local()
        c.request("hello", client="cli", label="deductum-host cli")
        print(json.dumps(c.request("sessions")["sessions"], indent=2))
        return 0
    if cmd == "find":
        # For Terminal Agents (design C10): the sessions an agent used, by its
        # Zellij session, tab or conversation, or by folder; as JSON
        opts = {}
        rest = argv[2:]
        for i in range(0, len(rest) - 1, 2):
            if rest[i].startswith("--"):
                opts[rest[i][2:]] = rest[i + 1]
        c = SyncClient.local()
        c.request("hello", client="cli", label="deductum find")
        print(json.dumps(c.request("find_sessions", **opts)["sessions"]))
        return 0
    if cmd == "stop-host":
        if host_running():
            c = SyncClient.local()
            c.request("hello", client="cli")
            c.request("shutdown")
        return 0
    if cmd == "runtimes":
        print(json.dumps(find_runtimes(), indent=2))
        return 0
    print("usage: deductum_host.py daemon|connect|mcp|status|runtimes|find|stop-host")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
