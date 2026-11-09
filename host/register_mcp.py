"""Register the deductum MCP server for Claude and Codex, at user level, when it is missing.

Deductum runs this on a machine after it has installed its host there (`deductum_host.py
register-mcp`), so that agents started on that machine find the proof tools. It only adds: an entry
or a table that is already there, whatever it says, is left exactly as it is, and so is a
configuration file this cannot read. The one repair: Deductum's own entry for this root (its arguments
are exactly this root's `host/deductum_host.py mcp`) whose `command` cannot run (an absolute path that
does not exist, or a bare name that is not on PATH) gets only its `command` replaced, and is reported as
"updated". For Codex that is done on the `command = "..."` line of the `[mcp_servers.deductum]` table
alone, when that table is a plain one-line `command` and one-line `args`; otherwise it stays as it is.
On macOS the command is the host's own Python (`sys.executable`), as `python3` there is missing or the
system one; elsewhere it is `python3`. A tool that is not in use on the machine (no `~/.claude.json`
or `~/.claude`, no `~/.codex`) is not given a file.

Claude: `mcpServers.deductum` in `~/.claude.json`. Codex: a `[mcp_servers.deductum]` table in
`$CODEX_HOME/config.toml` (`~/.codex/config.toml`).
"""
from __future__ import annotations

import json
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Dict, List, Optional

NAME = "deductum"


def command_for(root: Path) -> List[str]:
    """What an agent runs to talk to the host installed in `root`"""
    python = sys.executable if sys.platform == "darwin" and sys.executable else "python3"
    return [python, str(root / "host" / "deductum_host.py"), "mcp"]


def _runs(command: object) -> bool:
    """Whether `command` can be started: an existing absolute path, or a name found on PATH"""
    if not isinstance(command, str) or not command:
        return False
    if os.path.isabs(command):
        return os.path.exists(command)
    return shutil.which(command) is not None


def _write_atomic(path: Path, text: str) -> None:
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=path.name + ".")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as f:
            f.write(text)
        if path.exists():
            os.chmod(tmp, path.stat().st_mode & 0o777)
        os.replace(tmp, path)
    except BaseException:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


def register_claude(home: Path, root: Path) -> str:
    """"added", "updated" (a stale command of ours replaced), "present", "skipped" (Claude is not used here) or "unreadable" (left alone)"""
    path = home / ".claude.json"
    if not path.exists() and not (home / ".claude").is_dir():
        return "skipped"
    data: Dict = {}
    if path.exists():
        try:
            data = json.loads(path.read_text(encoding="utf-8") or "{}")
        except (OSError, ValueError):
            return "unreadable"
        if not isinstance(data, dict):
            return "unreadable"
    servers = data.get("mcpServers")
    if servers is None:
        servers = data["mcpServers"] = {}
    if not isinstance(servers, dict):
        return "unreadable"
    cmd = command_for(root)
    if NAME in servers:
        mine = servers[NAME]
        if isinstance(mine, dict) and mine.get("args") == cmd[1:] and not _runs(mine.get("command")) and mine.get("command") != cmd[0]:
            mine["command"] = cmd[0]
            _write_atomic(path, json.dumps(data, indent=2, ensure_ascii=False) + "\n")
            return "updated"
        return "present"
    servers[NAME] = {"type": "stdio", "command": cmd[0], "args": cmd[1:], "env": {}}
    _write_atomic(path, json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    return "added"


_TABLE = re.compile(r'^\s*\[\s*mcp_servers\s*\.\s*"?deductum"?\s*(\.[^\]]*)?\]', re.M)


def codex_home(home: Path) -> Path:
    return Path(os.environ["CODEX_HOME"]) if os.environ.get("CODEX_HOME") else home / ".codex"


_COMMAND_LINE = re.compile(r'^(command[ \t]*=[ \t]*)"((?:[^"\\\n]|\\.)*)"([ \t]*(?:#.*)?)$')
_ARGS_LINE = re.compile(r'^args[ \t]*=[ \t]*(\[.*\])[ \t]*(?:#.*)?$')


def _repair_codex(text: str, cmd: List[str]) -> Optional[str]:
    """`text` with the command line of our own `[mcp_servers.deductum]` table replaced, when that
    table is ours (its args are `cmd[1:]`) and its command cannot run; None when nothing is to change"""
    lines = text.splitlines(keepends=True)
    start = [i for i, l in enumerate(lines) if re.match(r'^[ \t]*\[[ \t]*mcp_servers[ \t]*\.[ \t]*(deductum|"deductum")[ \t]*\][ \t]*(#.*)?$', l.rstrip("\r\n"))]
    if len(start) != 1:
        return None
    end = next((j for j in range(start[0] + 1, len(lines)) if lines[j].lstrip().startswith("[")), len(lines))
    found = {"command": [], "args": []}
    for j in range(start[0] + 1, end):
        body = lines[j].rstrip("\r\n")
        for key in found:
            if re.match(key + r'[ \t]*=', body):
                found[key].append(j)
    if len(found["command"]) != 1 or len(found["args"]) != 1:
        return None
    ending = lines[found["command"][0]][len(lines[found["command"][0]].rstrip("\r\n")):]
    m = _COMMAND_LINE.match(lines[found["command"][0]].rstrip("\r\n"))
    a = _ARGS_LINE.match(lines[found["args"][0]].rstrip("\r\n"))
    if not m or not a:
        return None
    try:
        command = json.loads('"' + m.group(2) + '"')
        args = json.loads(a.group(1))
    except ValueError:
        return None
    if args != cmd[1:] or _runs(command) or command == cmd[0]:
        return None
    lines[found["command"][0]] = m.group(1) + json.dumps(cmd[0], ensure_ascii=False) + m.group(3) + ending
    return "".join(lines)


def register_codex(home: Path, root: Path) -> str:
    """As [`register_claude`], for Codex's config.toml"""
    folder = codex_home(home)
    path = folder / "config.toml"
    if not path.exists() and not folder.is_dir():
        return "skipped"
    try:
        if path.exists():
            with open(path, encoding="utf-8", newline="") as f:  # (line endings kept as they are)
                text = f.read()
        else:
            text = ""
    except (OSError, ValueError):
        return "unreadable"
    cmd = command_for(root)
    if _TABLE.search(text) or "deductum_host.py" in text:
        fixed = _repair_codex(text, cmd)
        if fixed is None:
            return "present"
        _write_atomic(path, fixed)
        return "updated"
    quote = lambda s: json.dumps(s, ensure_ascii=False)  # a JSON string is a TOML basic string
    table = f"[mcp_servers.{NAME}]\ncommand = {quote(cmd[0])}\nargs = [{', '.join(quote(a) for a in cmd[1:])}]\n"
    if text and not text.endswith("\n"):
        text += "\n"
    _write_atomic(path, text + ("\n" if text else "") + table)
    return "added"


def register(home: Optional[Path] = None, root: Optional[Path] = None) -> Dict[str, str]:
    """Register for both tools; what happened to each. `root` is where the host is installed"""
    home = Path(home) if home else Path.home()
    root = Path(root) if root else Path(__file__).resolve().parent.parent
    out = {}
    for tool, fn in (("claude", register_claude), ("codex", register_codex)):
        try:
            out[tool] = fn(home, root)
        except OSError as e:
            out[tool] = f"failed: {e}"
    return out
