"""Register the deductum MCP server for Claude and Codex, at user level, when it is missing.

Deductum runs this on a machine after it has installed its host there (`deductum_host.py
register-mcp`), so that agents started on that machine find the proof tools. It only adds: an entry
or a table that is already there, whatever it says, is left exactly as it is, and so is a
configuration file this cannot read. A tool that is not in use on the machine (no `~/.claude.json`
or `~/.claude`, no `~/.codex`) is not given a file.

Claude: `mcpServers.deductum` in `~/.claude.json`. Codex: a `[mcp_servers.deductum]` table in
`$CODEX_HOME/config.toml` (`~/.codex/config.toml`).
"""
from __future__ import annotations

import json
import os
import re
import tempfile
from pathlib import Path
from typing import Dict, List, Optional

NAME = "deductum"


def command_for(root: Path) -> List[str]:
    """What an agent runs to talk to the host installed in `root`"""
    return ["python3", str(root / "host" / "deductum_host.py"), "mcp"]


def _write_atomic(path: Path, text: str) -> None:
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=path.name + ".")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
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
    """"added", "present", "skipped" (Claude is not used here) or "unreadable" (left alone)"""
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
    if NAME in servers:
        return "present"
    cmd = command_for(root)
    servers[NAME] = {"type": "stdio", "command": cmd[0], "args": cmd[1:], "env": {}}
    _write_atomic(path, json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    return "added"


_TABLE = re.compile(r'^\s*\[\s*mcp_servers\s*\.\s*"?deductum"?\s*(\.[^\]]*)?\]', re.M)


def codex_home(home: Path) -> Path:
    return Path(os.environ["CODEX_HOME"]) if os.environ.get("CODEX_HOME") else home / ".codex"


def register_codex(home: Path, root: Path) -> str:
    """As [`register_claude`], for Codex's config.toml"""
    folder = codex_home(home)
    path = folder / "config.toml"
    if not path.exists() and not folder.is_dir():
        return "skipped"
    try:
        text = path.read_text(encoding="utf-8") if path.exists() else ""
    except (OSError, ValueError):
        return "unreadable"
    if _TABLE.search(text) or "deductum_host.py" in text:
        return "present"
    cmd = command_for(root)
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
