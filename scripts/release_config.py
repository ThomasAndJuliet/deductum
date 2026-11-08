"""The one place this source tree names the repository it publishes from."""
import json
from pathlib import Path
import re

CONFIG = Path(__file__).resolve().parents[1] / "release-config.json"


def load():
    return json.loads(CONFIG.read_text(encoding="utf-8"))


def repository():
    value = load().get("repository", "")
    if not re.fullmatch(r"[\w.-]+/[\w.-]+", value):
        raise ValueError("release-config.json must name the publishing repository as OWNER/NAME")
    return value
