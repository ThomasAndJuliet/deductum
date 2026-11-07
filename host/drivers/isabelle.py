"""The Isabelle driver: ``isabelle deductum_backend`` (backend/Isabelle2025/src/backend.scala),
the Scala PIDE process the host started directly before drivers existed (design C12, and
docs/deductum-provers.md §4). The backend protocol is its own, so this driver only starts the
process, passes JSON lines both ways and reads what is Isabelle's to know: runtimes, session
ROOTs, the base logic, symbols, and the proof tools.
"""

from __future__ import annotations

import asyncio
import base64
import contextlib
import errno
import hashlib
import json
import os
import time
import threading
import re
import shutil
import subprocess
import ntpath
import tempfile
import platform_support as platform
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional

from . import (SHARE_DIR, Driver, HostError, log, read_config, resource_limit, stop_process)


class RuntimeAdapter:
    """The official distribution's Bash boundary; JVM protocol paths stay native.

    Shell tools consume Cygwin paths, while backend.scala uses java.io.File and
    must receive Windows paths in protocol objects. Path conversion belongs here,
    never in the common host or Lean driver.
    """

    def __init__(self, runtime: str, cancel_event=None):
        self.runtime = str(runtime)
        self.home = Path(runtime).resolve().parent.parent
        self.bash = self.home / "contrib" / "cygwin" / "bin" / "bash.exe"
        self.cygpath = self.bash.with_name("cygpath.exe")
        if platform.WINDOWS and not self.bash.is_file():
            candidates = sorted((self.home / "contrib").glob("cygwin*/bin/bash.exe"))
            if candidates:
                self.bash = candidates[0]
                self.cygpath = self.bash.with_name("cygpath.exe")
        self._native = {}
        self._posix = {}
        self.cancel_event = cancel_event

    def _convert(self, path: str, flag: str) -> str:
        args = [str(self.cygpath), flag, "-a"]
        if flag == "-w":
            args += ["-C", "UTF8"]  # matches Isabelle's own platform_path function
        result = self._run_command([*args, "--", str(path)],
                              capture_output=True, text=True, encoding="utf-8", timeout=30,
                              env=self.environment())
        if result.returncode or not result.stdout.strip():
            raise HostError("Isabelle's bundled Cygwin could not convert a path: " + result.stderr[-2000:])
        return result.stdout.strip()

    def to_posix(self, path: str) -> str:
        if not platform.WINDOWS or path.startswith("/"):
            return path
        if path not in self._posix:
            self._posix[path] = self._convert(path, "-u")
        return self._posix[path]

    def to_native(self, path: str) -> str:
        if not platform.WINDOWS or not path.startswith("/"):
            return os.path.normpath(path) if platform.WINDOWS else path
        if path not in self._native:
            self._native[path] = os.path.realpath(self._convert(path, "-w"))
        return self._native[path]

    def cli_path(self, path: str) -> str:
        """Only native Java CLI paths use existing ASCII filesystem aliases.

        The bundled Windows JDK narrows command-line arguments to its ANSI code
        page even with file.encoding=UTF-8. JSON document paths remain native.
        """
        if not platform.WINDOWS:
            return path
        from _windows import ascii_short_path
        native = self.to_native(path) if path.startswith("/") else path
        try:
            return self.to_posix(ascii_short_path(native))
        except OSError as error:
            raise HostError(f"Isabelle command-line path {native!r}: {error}", "unsupported_path") from error

    def command(self, args: List[str]) -> List[str]:
        if not platform.WINDOWS:
            return [self.runtime, *args]
        if not self.bash.is_file() or not self.cygpath.is_file():
            raise HostError("Use the official Isabelle Windows distribution with its bundled Cygwin")
        if any("\0" in arg for arg in args):
            raise HostError("Windows Isabelle arguments cannot contain NUL characters", "unsupported_path")
        if any(not arg.isascii() for arg in args):
            raise HostError("The Windows Isabelle launcher cannot carry a non-ASCII command-line value. "
                            "Use an ASCII logic name and option values; project and application paths "
                            "need an existing ASCII NTFS short name.", "unsupported_path")
        # The bundled Cygwin native-argv parser does not implement MSCRT's
        # embedded-quote/backslash rules, even with noglob. Only safe base64
        # data crosses that parser; the immutable launcher reconstructs argv.
        values = [self.cli_path(self.runtime), *args]
        raw = ("\0".join([str(len(values)), *values]) + "\0").encode("utf-8")
        payload = base64.b64encode(raw).decode("ascii")
        if len(payload) > 16384:
            raise HostError("Windows Isabelle arguments exceed the 16384-byte encoded limit; "
                            "reduce the number or length of prover options", "unsupported_path")
        launcher = self.cli_path(str(Path(__file__).with_name("isabelle_windows.sh")))
        return [str(self.bash), "--noprofile", "--norc", launcher, payload]

    def environment(self, env=None):
        result = dict(os.environ if env is None else env)
        if platform.WINDOWS:
            result["LANG"] = result["LC_ALL"] = "C.UTF-8"
            path_keys = [key for key in result if key.upper() == "PATH"]
            path = result[path_keys[0]] if path_keys else ""
            for key in path_keys:
                del result[key]
            result["PATH"] = str(self.bash.parent) + ";" + path
            # Native Python already supplies argv as data. Cygwin's startup
            # globber otherwise removes apostrophes from valid NTFS short
            # names before cygpath or Bash receives them.
            result["CYGWIN"] = (result.get("CYGWIN", "") + " noglob").strip()
        return result

    def run(self, args: List[str], **kwargs):
        kwargs["env"] = self.environment(kwargs.get("env"))
        return self._run_command(self.command(args), **kwargs)

    def _run_command(self, argv, **kwargs):
        try:
            return platform.run(argv, cancel_event=self.cancel_event, **kwargs)
        except UnicodeDecodeError as error:
            diagnostic = getattr(error, "stderr", None) or b""
            raise HostError("Isabelle command returned non-UTF-8 " + getattr(error, "stream", "output") +
                            f" (exit {getattr(error, 'returncode', 'unknown')}); " +
                            "raw stderr: " + repr(diagnostic[-2000:]), "bad_encoding") from error

    def protocol(self, obj: Any) -> Any:
        """Normalize only structured file fields, preserving text and offsets."""
        if not platform.WINDOWS:
            return obj
        if isinstance(obj, list):
            return [self.protocol(v) for v in obj]
        if not isinstance(obj, dict):
            return obj
        result = {}
        for key, value in obj.items():
            if key in ("file", "home") and isinstance(value, str):
                result[key] = self.to_native(value)
            elif key in ("files", "imports") and isinstance(value, list):
                result[key] = [self.to_native(v) if isinstance(v, str) and
                               (v.startswith("/") or ntpath.isabs(v)) else self.protocol(v) for v in value]
            else:
                result[key] = self.protocol(value)
        return result


# ---------------------------------------------------------------------------
# Isabelle runtimes and session ROOTs


def runtime_identifier(isabelle: Path) -> Optional[str]:
    home = isabelle.resolve().parent.parent
    try:
        return (home / "etc" / "ISABELLE_IDENTIFIER").read_text(encoding="utf-8").strip() or None
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
    if platform.WINDOWS:
        for root in (os.environ.get("ProgramFiles"), os.environ.get("ProgramFiles(x86)"), Path.home().anchor):
            if root:
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
        text = path.read_text(encoding="utf-8", errors="replace")
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
            r = RuntimeAdapter(runtime).run(["sessions", "-a"], capture_output=True, text=True,
                                             encoding="utf-8", timeout=300)
            _known_sessions[runtime] = [ln.strip() for ln in r.stdout.splitlines() if ln.strip()]
        except (OSError, subprocess.TimeoutExpired):
            _known_sessions[runtime] = []
    return _known_sessions[runtime]


#: How long detection looks through a folder for theories
DETECT_SECONDS = 1.5


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
    adapter = RuntimeAdapter(runtime)
    r = adapter.run(["getenv", "-b", "ISABELLE_SYMBOLS"], capture_output=True, text=True,
                    encoding="utf-8", timeout=120)
    entries: Dict[str, Dict[str, Any]] = {}
    for path in r.stdout.strip().split(":"):
        try:
            text = Path(adapter.to_native(path)).read_text(encoding="utf-8", errors="replace")
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


_component_locks: Dict[str, threading.Lock] = {}
_component_locks_guard = threading.Lock()
_COMPONENT_MARKER = "_deductum-component.json"


def _component_canceled(cancel_event):
    if cancel_event is not None and cancel_event.is_set():
        raise InterruptedError("Prover command canceled")


def _component_jvm_path(path, guidance):
    if any(ord(char) >= 0x10000 or 0xd800 <= ord(char) <= 0xdfff for char in str(Path(path).resolve())):
        raise HostError("The official Isabelle Windows JVM cannot load components from a canonical "
                        "path containing supplementary Unicode characters. " + guidance, "unsupported_path")


def _component_tree(source: Path, ident: str):
    """Exact immutable build inputs, excluding generated compiler output."""
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", ident) or ident in (".", ".."):
        raise HostError("Invalid Isabelle component identifier")
    files = {}
    for path in sorted(source.rglob("*")):
        relative = path.relative_to(source).as_posix()
        if path.is_symlink() or (hasattr(path, "is_junction") and path.is_junction()):
            raise HostError("Backend component inputs cannot contain symbolic links")
        if relative.split("/")[0] == "lib" or relative == _COMPONENT_MARKER:
            continue
        if path.is_file():
            files[relative] = path.read_bytes()
    if not all(name in files for name in ("etc/settings", "etc/build.props")):
        raise HostError("The Deductum backend component is incomplete")
    digest = hashlib.sha256(b"deductum-isabelle-component-v1\0" + ident.encode("utf-8") + b"\0")
    for name, data in files.items():
        digest.update(name.encode("utf-8") + b"\0" + str(len(data)).encode("ascii") + b"\0" + data)
    return digest.hexdigest(), files


def _component_secure_directory(path):
    platform.ensure_state_dir(path)


def _component_secure_tree(path, *, harden=True):
    """Check preexisting exposure before tightening any cache child ACL."""
    from _windows import check_private_dacl, reject_reparse, secure_file
    if not path.exists():
        return
    paths = []

    def inspect(current):
        reject_reparse(current)
        check_private_dacl(current)
        paths.append(current)
        if current.is_dir():
            for child in current.iterdir():
                inspect(child)

    try:
        if not harden:
            # The stable root must block ancestor ACL propagation. Inspect
            # compiler outputs only after obtaining the content build lock.
            check_private_dacl(path, protected=True)
            return
        inspect(path)
        for current in paths:
            if current.is_dir():
                _component_secure_directory(current)
            else:
                secure_file(current)
    except OSError as error:
        raise HostError("The private Isabelle component cache has unsafe ownership or permissions: " +
                        str(path) + ". Stop its hosts and remove the damaged cache, or choose another "
                        "private DEDUCTUM_STATE_DIR.") from error


def _component_cache_root(ident):
    state = platform.state_dir()
    _component_jvm_path(state, "Set DEDUCTUM_STATE_DIR to a dedicated private directory without emoji.")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", ident) or ident in (".", ".."):
        raise HostError("Invalid Isabelle component identifier")
    root = state / "isabelle-components" / ident
    for directory in (state, root.parent, root):
        _component_secure_directory(directory)
    return root


def _windows_component(source, ident, cancel_event=None, *, tree=None):
    _component_canceled(cancel_event)
    digest, files = _component_tree(source, ident) if tree is None else tree
    # Inspect before changing ancestor ACLs, which could propagate to children.
    _component_secure_tree(platform.state_dir() / "isabelle-components" / ident / digest)
    root = _component_cache_root(ident)
    destination = root / digest
    marker = {"kind": "deductum-isabelle-component", "version": 1, "identifier": ident,
              "digest": digest,
              "files": [[name, hashlib.sha256(data).hexdigest()] for name, data in files.items()]}

    def validate():
        try:
            _component_secure_tree(destination)
            actual_marker = json.loads((destination / _COMPONENT_MARKER).read_text(encoding="utf-8"))
            actual_digest, actual_files = _component_tree(destination, ident)
            if actual_marker != marker or actual_digest != digest or actual_files != files:
                raise ValueError("component bytes or marker differ")
        except (OSError, ValueError) as error:
            raise HostError("The private Isabelle component cache is invalid: " + str(destination)) from error

    if destination.exists():
        validate()
        return destination
    staging = Path(tempfile.mkdtemp(prefix=".prepare-", dir=root))
    try:
        _component_secure_directory(staging)
        for name, data in files.items():
            _component_canceled(cancel_event)
            target = staging / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        (staging / _COMPONENT_MARKER).write_text(json.dumps(marker, sort_keys=True), encoding="utf-8")
        _component_secure_tree(staging)
        _component_canceled(cancel_event)
        try:
            staging.rename(destination)
        except FileExistsError:
            validate()
        return destination
    finally:
        if staging.exists():
            shutil.rmtree(staging)


@contextlib.contextmanager
def _component_file_lock(path, cancel_event):
    import msvcrt
    from _windows import reject_reparse, secure_file, check_owner
    reject_reparse(path)
    if path.name == "deductum-component.lock":
        registry = path.with_name("components")
        reject_reparse(registry)
        if registry.exists():
            check_owner(registry)
    with open(path, "a+b") as handle:
        secure_file(path)
        if handle.seek(0, os.SEEK_END) == 0:
            handle.write(b"\0")
            handle.flush()
        deadline = time.monotonic() + 1800
        while True:
            _component_canceled(cancel_event)
            try:
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                break
            except OSError as error:
                if error.errno not in (errno.EACCES, errno.EAGAIN, errno.EDEADLK):
                    raise
                if time.monotonic() >= deadline:
                    raise HostError("Timed out waiting for Isabelle component registration")
                time.sleep(0.05)
        try:
            yield
        finally:
            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)


@contextlib.contextmanager
def _component_lock(path, cancel_event):
    key = os.path.normcase(str(path.resolve()))
    with _component_locks_guard:
        lock = _component_locks.setdefault(key, threading.Lock())
    deadline = time.monotonic() + 1800
    while not lock.acquire(timeout=0.05):
        _component_canceled(cancel_event)
        if time.monotonic() >= deadline:
            raise HostError("Timed out waiting for Isabelle component registration")
    try:
        _component_canceled(cancel_event)
        path.parent.mkdir(parents=True, exist_ok=True)
        with _component_file_lock(path, cancel_event):
            yield
    finally:
        lock.release()


def _component_registry_lock(registry, cancel_event):
    return _component_lock(registry.with_name("deductum-component.lock"), cancel_event)


def _component_replace_registry(staged, registry):
    from _windows import replace_preserving_dacl
    replace_preserving_dacl(staged, registry)


def _is_deductum_component(path):
    try:
        settings = (path / "etc" / "settings").read_text(encoding="utf-8")
        props = (path / "etc" / "build.props").read_text(encoding="utf-8").replace("\\\n", "")
        return bool(re.search(r'^\s*DEDUCTUM_BACKEND_HOME="\$COMPONENT"\s*$', settings, re.M) and
                    re.search(r'^\s*services\s*=\s*deductum\.Tools\s*$', props, re.M))
    except (OSError, UnicodeError):
        return False


def _owned_missing_component(path):
    root = (platform.state_dir() / "isabelle-components").resolve()
    try:
        parts = path.resolve().relative_to(root).parts
        return len(parts) == 2 and bool(re.fullmatch(r"[A-Za-z0-9_.-]+", parts[0])) and bool(
            re.fullmatch(r"[0-9a-f]{64}", parts[1]))
    except (OSError, ValueError):
        return False


def _register_windows_component(adapter, registry, comp, progress, cancel_event):
    original = registry.read_bytes() if registry.exists() else b""
    lines = original.decode("utf-8").splitlines(keepends=True)
    selected = adapter.cli_path(str(comp))
    output, present = [], False
    for line in lines:
        entry = line.strip()
        if not entry or entry.startswith("#"):
            output.append(line)
            continue
        if "$" in entry:
            output.append(line)  # Unexpanded upstream variables are not ownership evidence.
            continue
        native = Path(adapter.to_native(entry if entry.startswith("/") or ntpath.isabs(entry)
                                        else str(registry.parent.parent / entry)))
        if native.resolve() == comp.resolve():
            if not present:
                output.append(selected + ("\r\n" if line.endswith("\r\n") else "\n"))
                present = True
        elif _is_deductum_component(native) or (not native.exists() and _owned_missing_component(native)):
            progress("Unregistering a stale Deductum backend: " + entry)
        else:
            output.append(line)
    if not present:
        progress("Registering the Deductum backend with " + comp.parent.name)
        if output and not output[-1].endswith(("\n", "\r")):
            output.append("\n")
        output.append(selected + "\n")
    data = "".join(output).encode("utf-8")
    if data != original:
        _component_canceled(cancel_event)
        fd, temporary = tempfile.mkstemp(prefix=".deductum-components-", dir=registry.parent)
        staged = Path(temporary)
        try:
            with os.fdopen(fd, "wb") as handle:
                handle.write(data)
                handle.flush()
                os.fsync(handle.fileno())
            _component_canceled(cancel_event)
            _component_replace_registry(staged, registry)
        finally:
            staged.unlink(missing_ok=True)


# Either separator: on Windows the components listing holds native paths
_STALE_BACKEND_PATH = re.compile(r"deductum[^/\\]*[/\\]backend[/\\][^/\\]+[/\\]?$")


def stale_backend_components(adapter, lines: List[str], comp_path: str) -> List[str]:
    """Registered components other than comp_path that provide a Deductum backend.

    Used where the Isabelle CLI manages the registry (Linux and macOS); Windows
    registration is _register_windows_component's. A component is recognised by its
    content (_is_deductum_component), so a source checkout is found whatever its
    directory is called. A registration whose directory is gone cannot be inspected;
    for those the old path pattern still applies, so that a deleted install is
    unregistered too (a stale entry would break Isabelle's startup).
    """
    stale = []
    for ln in lines:
        if not ln or ln == comp_path:
            continue
        try:
            native = Path(adapter.to_native(ln))
            exists = native.is_dir()
            marked = exists and _is_deductum_component(native)
        except OSError:
            exists = marked = False
        if marked or (not exists and _STALE_BACKEND_PATH.search(ln)):
            stale.append(ln)
    return stale


def ensure_component(runtime: str, progress, cancel_event=None) -> None:
    """Register the backend component with the runtime and build its jar"""
    ident = runtime_identifier(Path(runtime))
    comp = SHARE_DIR / "backend" / (ident or "")
    if not ident or not comp.is_dir():
        raise HostError(f"No Deductum backend for {ident or runtime}", "unsupported_runtime")
    env = dict(os.environ)
    adapter = RuntimeAdapter(runtime, cancel_event=cancel_event)
    if platform.WINDOWS:
        _component_canceled(cancel_event)
        _component_jvm_path(Path(runtime).resolve().parent.parent,
                            "Install the official Isabelle runtime in a location without emoji.")
    home_user = _component_ready.get(runtime)
    if not home_user:
        settings = adapter.run(["getenv", "-b", "ISABELLE_HOME_USER"], capture_output=True,
                               text=True, encoding="utf-8", timeout=120, env=env)
        if settings.returncode:
            raise HostError("Could not read Isabelle settings: " + settings.stderr[-2000:])
        home_user = settings.stdout.strip()
    if not home_user:
        raise HostError("Isabelle did not report its per-user component directory")
    registry = Path(adapter.to_native(home_user)) / "etc" / "components"
    if platform.WINDOWS:
        with _component_registry_lock(registry, cancel_event):
            tree = _component_tree(comp, ident)
            digest, _ = tree
            candidate = platform.state_dir() / "isabelle-components" / ident / digest
            _component_secure_tree(candidate, harden=False)
            cache_root = _component_cache_root(ident)
            # Registry before content lock: all callers use the same lock order.
            with _component_lock(cache_root / ("." + digest + ".lock"), cancel_event):
                comp = _windows_component(comp, ident, cancel_event, tree=tree)
                jar = comp / "lib" / "deductum_backend.jar"
                receipt = jar.with_name("_deductum-build.sha256")
                valid_build = False
                if receipt.exists():
                    try:
                        valid_build = receipt.read_text(encoding="ascii") == hashlib.sha256(jar.read_bytes()).hexdigest()
                    except (OSError, UnicodeError):
                        pass
                    if not valid_build:
                        raise HostError("The private Isabelle component jar differs from its accepted build: " +
                                        str(comp) + ". Stop its hosts and remove the damaged cache, or choose "
                                        "another private DEDUCTUM_STATE_DIR.")
                elif jar.exists():
                    # Unaccepted output may contain a valid source shasum but
                    # corrupt classes, which incremental scala_build trusts.
                    jar.unlink()
                _register_windows_component(adapter, registry, comp, progress, cancel_event)
                if not valid_build:
                    progress("Building the Deductum backend")
                    result = adapter.run(["scala_build"], capture_output=True, text=True,
                                         encoding="utf-8", timeout=1800, env=env)
                    if result.returncode or not jar.is_file():
                        raise HostError("Backend build failed: " + (result.stdout + result.stderr)[-2000:])
                    _component_canceled(cancel_event)
                    _component_secure_tree(comp)
                    receipt.write_text(hashlib.sha256(jar.read_bytes()).hexdigest(), encoding="ascii")
                    _component_secure_tree(comp)
            _component_ready[runtime] = home_user
        return
    comp_path = adapter.cli_path(str(comp))
    lines = []
    with contextlib.suppress(OSError):
        lines = [ln.strip() for ln in registry.read_text(encoding="utf-8").splitlines()]
    # (ready before, and still registered: another host, a test, may have registered its own since)
    if _component_ready.get(runtime) and comp_path in lines:
        return
    # Other copies of the backend (an old install, a test) would provide the same tool
    for stale in stale_backend_components(adapter, lines, comp_path):
        progress(f"Unregistering a stale Deductum backend: {stale}")
        adapter.run(["components", "-x", stale], capture_output=True, text=True, encoding="utf-8", timeout=300, env=env)
    if comp_path not in lines:
        progress(f"Registering the Deductum backend with {ident}")
        r = adapter.run(["components", "-u", comp_path], capture_output=True,
                        text=True, encoding="utf-8", timeout=300, env=env)
        if r.returncode != 0:
            raise HostError("Could not register backend: " + r.stderr.strip())
    jar = comp / "lib" / "deductum_backend.jar"
    sources = list((comp / "src").glob("*.scala")) + [comp / "etc" / "build.props"]
    if not jar.is_file() or any(s.stat().st_mtime > jar.stat().st_mtime for s in sources):
        progress("Building the Deductum backend")
        r = adapter.run(["scala_build"], capture_output=True, text=True, encoding="utf-8", timeout=1800,
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
    # Only the theories someone views (or an agent asked about lately) are required, as
    # Isabelle/jEdit requires its open buffers: a theory once looked at but kept required would
    # be checked again, with everything importing it, at every keystroke in a theory it imports
    open_policy = "viewed"
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
        # (a huge folder, a home directory, is not walked for long: what it holds is "many")
        deadline = time.monotonic() + DETECT_SECONDS
        for dirpath, dirnames, filenames in os.walk(f):
            if time.monotonic() > deadline:
                thys = max(thys, 50) if thys else thys
                break
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
        self.runtime_adapter = RuntimeAdapter(runtime)
        proj = meta.get("project") or {}
        loop = asyncio.get_running_loop()
        canceled = threading.Event()
        build = asyncio.create_task(asyncio.to_thread(
            ensure_component, runtime, lambda m: loop.call_soon_threadsafe(s.session_log, m), canceled))
        try:
            await asyncio.shield(build)
        except asyncio.CancelledError:
            canceled.set()
            await asyncio.gather(build, return_exceptions=True)
            raise
        cmd = ["deductum_backend", "-l", proj.get("logic") or "HOL"]
        for d in proj.get("dirs", []):
            cmd += ["-d", await asyncio.to_thread(self.runtime_adapter.cli_path, d)]
        for name in proj.get("include_sessions", []):
            cmd += ["-i", name]
        threads = resource_limit(s, "prover_threads")
        if threads:
            cmd += ["-o", f"threads={threads}"]
        for opt in meta.get("options", []):
            cmd += ["-o", opt]
        log("starting backend", s.id, " ".join(cmd))
        self.proc = await platform.create_subprocess_exec(
            *self.runtime_adapter.command(cmd), stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE, start_new_session=True, limit=64 * 1024 * 1024,
            env=self.runtime_adapter.environment())
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
                if platform.WINDOWS:
                    msg = await asyncio.to_thread(self.runtime_adapter.protocol, msg)
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
        obj = self.runtime_adapter.protocol(obj)
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
