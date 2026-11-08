"""Portable external CLI stand-ins; the real prover still checks their output."""
import json
import os
from pathlib import Path
import sys


def _windows_ascii_file(path):
    """An existing same-file name that is safe in an OEM-decoded batch file."""
    import ctypes
    from ctypes import wintypes
    path = str(Path(path).resolve(strict=True))
    alias = path
    if not path.isascii():
        get_short = ctypes.WinDLL("kernel32", use_last_error=True).GetShortPathNameW
        get_short.argtypes = [wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD]
        get_short.restype = wintypes.DWORD
        output = ctypes.create_unicode_buffer(32768)
        size = get_short(path, output, len(output))
        if not size or size >= len(output) or not output.value.isascii():
            raise OSError("The Windows command fixture requires an existing ASCII NTFS short name: " + path)
        alias = output.value
    if not os.path.samefile(path, alias):
        raise OSError("The command fixture's ASCII alias identifies a different file: " + path)
    return alias


def json_agent(root, answer):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    script = root / "agent-answer.py"
    script.write_text("import json\nprint(json.dumps(" + repr(answer) + "))\n", encoding="utf-8")
    config = root / "config"
    config.mkdir(exist_ok=True)
    (config / "host.json").write_text(json.dumps({"agent_commands": {"claude": [sys.executable, str(script)]}}),
                                      encoding="utf-8")
    os.environ["DEDUCTUM_CONFIG_DIR"] = str(config)


def json_executable(root, name, answer):
    """A small real command with fixed output, ignoring all query arguments."""
    root = Path(root)
    script = root / (name + "-answer.py")
    script.write_text("import json\nprint(json.dumps(" + repr(answer) + "))\n", encoding="utf-8")
    if os.name == "nt":
        executable = root / (name + ".cmd")
        python = _windows_ascii_file(sys.executable).replace("%", "%%")
        source = _windows_ascii_file(script).replace("%", "%%")
        executable.write_bytes(f'@setlocal DisableDelayedExpansion\r\n@"{python}" "{source}"\r\n'.encode("ascii"))
    else:
        executable = root / name
        # The test interpreter path is fixed by its environment; no shell or
        # query interpolation is involved.
        executable.write_text(f"#!{sys.executable}\n" + script.read_text(encoding="utf-8"), encoding="utf-8")
        executable.chmod(0o755)
    return executable
