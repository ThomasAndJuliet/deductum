"""The Isabelle driver: ``isabelle deductum_backend`` (backend/Isabelle2025/src/backend.scala),
the Scala PIDE process the host started directly before drivers existed (design C12, and
docs/deductum-provers.md §4). The backend protocol is its own, so this driver only starts the
process, passes JSON lines both ways and reads what is Isabelle's to know: runtimes, session
ROOTs, the base logic, symbols, and the proof tools.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import os
import re
import shutil
import subprocess
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional

from . import (SHARE_DIR, Driver, HostError, log, read_config, stop_process)


# ---------------------------------------------------------------------------
# Isabelle runtimes and session ROOTs


def runtime_identifier(isabelle: Path) -> Optional[str]:
    home = isabelle.resolve().parent.parent
    try:
        return (home / "etc" / "ISABELLE_IDENTIFIER").read_text().strip() or None
    except OSError:
        return None


def find_runtimes() -> List[Dict[str, Any]]:
    candidates: List[Path] = []
    config = read_config()
    # (`isabelle` lists runtimes as {"path": ...} or plain paths; `runtimes` is the older name)
    for entry in list(config.get("isabelle") or []) + list(config.get("runtimes") or []):
        p = entry.get("path") if isinstance(entry, dict) else entry
        if p:
            candidates.append(Path(os.path.expanduser(p)))
    if os.environ.get("DEDUCTUM_ISABELLE"):
        candidates.append(Path(os.environ["DEDUCTUM_ISABELLE"]))
    for pattern in ("Documents/Software/Isabelle*/bin/isabelle", "Software/Isabelle*/bin/isabelle",
                    "Isabelle*/bin/isabelle", "opt/Isabelle*/bin/isabelle"):
        candidates.extend(sorted(Path.home().glob(pattern)))
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
        if d == Path.home() or d == d.parent:
            break
    return {"logic": "HOL", "dirs": [], "include_sessions": [], "session": None, "sessions": [], "note": None}


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
# The driver


class IsabelleDriver(Driver):
    backend = "isabelle"
    label = "Isabelle"
    documents = (".thy",)
    project_files = ("ROOT", "ROOTS")
    open_policy = "all"
    placeholder = "sorry"
    placeholder_commands = ("sorry", "oops")
    goal_commands = ("lemma", "theorem", "corollary", "proposition", "schematic_goal")
    theory_end_commands = ("end", "theory")
    # Proof tools typed in the text: their findings come with Insert, and they stop when deleted
    TOOLS = ("sledgehammer", "try", "try0", "nitpick", "quickcheck", "solve_direct", "nunchaku")
    tool_names = TOOLS
    option_allowlist = ("sledgehammer_timeout", "sledgehammer_provers")
    statement_regex = re.compile(
        r"\s*(lemma|theorem|corollary|proposition|schematic_goal)\b\s*(?:([^\s:\"\[]+)(?:\s*\[[^\]]*\])?\s*:)?")
    proof_start_regex = re.compile(r"\n\s*(proof|by|apply|using|unfolding|sorry|oops|including|supply)\b")
    facts_line = "using {facts}\nsledgehammer"
    facts_separator = " "

    def __init__(self, session: Any):
        super().__init__(session)
        self.next_id = 1
        self.pending: Dict[int, asyncio.Future] = {}
        self.reader_task: Optional[asyncio.Task] = None
        self.stderr_task: Optional[asyncio.Task] = None

    # ----- discovery

    @classmethod
    def toolchains(cls) -> List[Dict[str, Any]]:
        out = []
        for r in find_runtimes():
            ident = r["identifier"]
            problem = None
            if not r["supported"]:
                problem = (f"No Deductum backend for {ident}" if ident else "Not an Isabelle installation")
            out.append({"id": r["path"], "label": Path(r["home"]).name, "version": ident, "path": r["path"],
                        "supported": r["supported"], "ok": r["supported"], "problem": problem,
                        "identifier": ident, "home": r["home"]})
        return out

    @classmethod
    def detect(cls, folder: str) -> Optional[Dict[str, Any]]:
        f = Path(folder).resolve()
        evidence: List[str] = []
        for d in [f] + list(f.parents):
            for name in ("ROOT", "ROOTS"):
                if (d / name).is_file():
                    evidence.append(f"{name} in {d}")
            if evidence or d == Path.home() or d == d.parent:
                break
        thys = 0
        for dirpath, dirnames, filenames in os.walk(f):
            dirnames[:] = [x for x in dirnames if not x.startswith(".") and x not in ("output", "node_modules")]
            if len(Path(dirpath).relative_to(f).parts) >= 3:
                dirnames[:] = []
            thys += sum(1 for n in filenames if n.endswith(".thy"))
            if thys >= 50:
                break
        if thys:
            evidence.append(f"{thys}{'+' if thys >= 50 else ''} .thy file{'s' if thys != 1 else ''}")
        if not evidence:
            return None
        supported = [t for t in cls.toolchains() if t["supported"]]
        runtime = supported[0]["id"] if supported else None
        sug = suggest_logic(str(f), runtime)
        project = {k: sug[k] for k in ("logic", "dirs", "include_sessions", "session", "sessions")}
        return {"backend": cls.backend, "evidence": evidence, "project": project, "toolchain": runtime,
                "note": sug.get("note"), "documents": list(cls.documents)}

    @classmethod
    def resolve_session(cls, params: Dict[str, Any], folder: str) -> Dict[str, Any]:
        # (the project may come inside `project`, or at the top level as older apps send it)
        proj = dict(params.get("project") or {})
        runtime = params.get("toolchain") or params.get("runtime")
        if runtime:
            runtime = os.path.realpath(os.path.expanduser(runtime))
        else:
            runtimes = find_runtimes()
            supported = [r for r in runtimes if r["supported"]]
            if not supported:
                raise HostError("No supported Isabelle runtime on this machine; Deductum needs "
                                "Isabelle2025 (CyPhyAssure). Found: " +
                                ", ".join(f"{r['identifier']} ({r['path']})" for r in runtimes),
                                "unsupported_runtime")
            runtime = supported[0]["path"]
        suggestion = suggest_logic(folder, runtime)
        logic = proj.get("logic") or params.get("logic") or suggestion["logic"]
        dirs = proj.get("dirs") if proj.get("dirs") is not None else params.get("dirs")
        if dirs is None:
            dirs = suggestion["dirs"]
        include = proj.get("include_sessions") if proj.get("include_sessions") is not None \
            else params.get("include_sessions")
        if include is None:
            include = suggestion["include_sessions"] if logic != suggestion.get("session") else []
        dirs = [os.path.realpath(d) for d in dirs]
        project = {"logic": logic, "dirs": dirs, "include_sessions": include}
        return {"toolchain": runtime, "project": project,
                "meta": {"runtime": runtime, "identifier": runtime_identifier(Path(runtime)),
                         "logic": logic, "dirs": dirs, "include_sessions": include}}

    @classmethod
    def migrate_meta(cls, meta: Dict[str, Any]) -> None:
        meta.setdefault("project", {"logic": meta.get("logic", "HOL"), "dirs": meta.get("dirs", []),
                                    "include_sessions": meta.get("include_sessions", [])})
        meta.setdefault("toolchain", meta.get("runtime", ""))

    @classmethod
    def sync_meta(cls, meta: Dict[str, Any], changed: Dict[str, Any]) -> None:
        """After `configure`: the legacy top-level keys and `project`/`toolchain` agree again"""
        proj = meta.setdefault("project", {})
        given = changed.get("project") or {}
        for k in ("logic", "dirs", "include_sessions"):
            if k in changed:
                proj[k] = changed[k]
            elif k in given:
                meta[k] = given[k]
        if "runtime" in changed:
            meta["toolchain"] = changed["runtime"]
        elif "toolchain" in changed:
            meta["runtime"] = changed["toolchain"]

    @classmethod
    def project_key(cls, project: Dict[str, Any]) -> str:
        return str(project.get("logic"))

    @classmethod
    def relocate_project(cls, project: Dict[str, Any], moved: Callable[[str], str]) -> Dict[str, Any]:
        out = dict(project)
        out["dirs"] = [moved(d) for d in project.get("dirs", [])]
        return out

    @classmethod
    def toolchain_label(cls, toolchain: str, meta: Dict[str, Any]) -> str:
        return Path(toolchain).parent.parent.name or toolchain

    @classmethod
    def symbol_table(cls, toolchain: Optional[str]) -> List[Dict[str, Any]]:
        if not toolchain:
            supported = [r for r in find_runtimes() if r["supported"]] or find_runtimes()
            if not supported:
                return []
            toolchain = supported[0]["path"]
        return symbol_table(toolchain)

    @classmethod
    def fact_check_text(cls, facts: List[str]) -> Optional[str]:
        return "thm " + " ".join(facts)

    @classmethod
    def header_of(cls, text: str) -> str:
        end = text.find("begin")
        return text[:end + 5] if end >= 0 else text[:400]

    def building_message(self) -> str:
        return "Building the session image for " + str(self.session.meta["project"].get("logic"))

    # ----- what the app may show

    def capabilities(self) -> Dict[str, Any]:
        def tool(name: str, kind: str, stop: bool = True, **extra: Any) -> Dict[str, Any]:
            return dict({"name": name, "typed": name, "kind": kind, "stop": stop, "settings": {}}, **extra)
        sledgehammer = tool("sledgehammer", "prove", settings={
            "sledgehammer_timeout": {"label": "Timeout", "type": "seconds", "default": 30},
            "sledgehammer_provers": {"label": "Provers", "type": "string", "default": ""}})
        return {
            "checking": "continuous", "pause": "perspective", "interrupt": True,
            "goals": {"structured": True, "term_goal": False, "facts": True, "shelved": False},
            "hover": True, "definition": True, "spans": "prover",
            "tools": [sledgehammer, tool("try", "prove"), tool("try0", "prove"),
                      tool("solve_direct", "prove"), tool("nitpick", "counterexample"),
                      tool("quickcheck", "counterexample"), tool("nunchaku", "counterexample")],
            "counterexamples": ["quickcheck", "nitpick"],
            "search": {"label": "Find theorems", "syntax": "isabelle-find"},
            "command": ["value", "term", "thm", "typ", "prop", "find_consts", "find_theorems"],
            "check_candidates": "query",
            "project_actions": [{"name": "build", "label": "Build heap"}],
            "semantic_colours": True,
            "documents": list(self.documents), "project_files": ["ROOT"],
            "placeholder": self.placeholder, "keyword_line_suffix": "",
            "facts_line": self.facts_line, "facts_separator": self.facts_separator,
            "open_policy": self.open_policy,
        }

    # ----- the process

    async def start(self) -> None:
        s = self.session
        meta = s.meta
        runtime = meta.get("toolchain") or meta["runtime"]
        proj = meta.get("project") or {}
        await asyncio.to_thread(ensure_component, runtime, lambda m: s.session_log(m))
        cmd = [runtime, "deductum_backend", "-l", proj.get("logic") or "HOL"]
        for d in proj.get("dirs", []):
            cmd += ["-d", d]
        for name in proj.get("include_sessions", []):
            cmd += ["-i", name]
        for opt in meta.get("options", []):
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
                    self.emit(msg)
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
        await stop_process(self.proc)
