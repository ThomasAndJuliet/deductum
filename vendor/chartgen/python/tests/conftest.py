"""Shared test helpers. External tools are required: a missing tool fails its test,
unless CHARTGEN_ALLOW_MISSING_TOOLS=1 turns the failure into an explicit skip."""
import os
import shutil
from pathlib import Path

import pytest


def require(path_or_tool, how):
    found = Path(path_or_tool).exists() if os.sep in str(path_or_tool) else shutil.which(path_or_tool)
    if found:
        return
    message = f"{path_or_tool} is required ({how})"
    if os.environ.get("CHARTGEN_ALLOW_MISSING_TOOLS") == "1":
        pytest.skip(message + "; skipped because CHARTGEN_ALLOW_MISSING_TOOLS=1")
    pytest.fail(message + "; set CHARTGEN_ALLOW_MISSING_TOOLS=1 to skip explicitly")
