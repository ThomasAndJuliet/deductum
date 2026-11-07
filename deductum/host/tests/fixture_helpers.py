"""Portable external CLI stand-ins; the real prover still checks their output."""
import json
import os
from pathlib import Path
import sys


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
        executable.write_text(f'@"{sys.executable}" "{script}"\n', encoding="utf-8")
    else:
        executable = root / name
        # The test interpreter path is fixed by its environment; no shell or
        # query interpolation is involved.
        executable.write_text(f"#!{sys.executable}\n" + script.read_text(encoding="utf-8"), encoding="utf-8")
        executable.chmod(0o755)
    return executable
