"""Run with the installed Python extension: python examples/python/render.py."""
from pathlib import Path
import json
from chartgen import render
from chartgen.style import Style

spec = json.loads((Path(__file__).resolve().parents[1] / "basic.json").read_text())
output, report = render(spec, Style("plain"), backend="svg")
print(output)
print(report.status)
