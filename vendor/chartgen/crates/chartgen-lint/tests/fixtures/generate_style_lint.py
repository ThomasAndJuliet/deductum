"""Style-lint fixtures: the original's lint_style (charts/chartgen/style.py) on the original plain
style and on faulty stylesheets importing it, with each style's flattened tokens and rules as the
original figkit Style produces them. Run with the oracle environment:
    venv-oracle/bin/python generate_style_lint.py ORACLE_ROOT WORKDIR > style_lint.json"""
import json, sys
from pathlib import Path
root, work = Path(sys.argv[1]), Path(sys.argv[2])
sys.path.insert(0, str(root / "tools")); sys.path.insert(0, str(root / "charts"))
from chartgen.style import Style, lint_style
plain = root / "charts" / "style" / "plain.css"
FAULTS = {
    "loud-palette": ":root { --series-1: #00adef; --series-2: #ed008c; --series-3: #fef200; --single: #00adef; }",
    "grey-slot": ":root { --series-4: #8a8a8a; }",
    "light-text": ":root { --text: #b0b0b0; --note: #cccccc; --comment: #9ab; }",
    "dark-grid": ":root { --grid: #000000; --zero-line: #333333; --divider: #a0a0a0; }",
    "grid-darker-than-data": ":root { --grid: #c0c0c0; --single: #e8eef3; }",
    "small-notes": ":root { --note-size: 6px; --note-mark-size: 0.5em; --note-marks: roman; }",
    "flat-ramp": ":root { --seq-3: #ffffff; --seq-7: #e0e0e0; }",
    "loud-neutral": ":root { --div-0: #ffd000; --div-m1: #f5c400; --good-1: #e8b800; }",
    "dark-cells": ".labelled.seq-9 { fill: #303030; } .cell-bar { fill: #505050; }",
    "pale-group-text": ":root { --series-1-text: #dddddd; --series-2-text: #c0c0ff; }",
    "spread-lightness": ":root { --series-2: #2a2a6a; --series-5: #f0e0a0; }",
    "good-bad-text": ":root { --good-text: #a0d0a0; --bad-cvd-text: #f0b0a0; --note-mark-ink: #dddddd; }",
}
out = []
def record(name, path):
    st = Style(path)
    out.append({"name": name, "tokens": st.tokens, "rules": st.rules,
                "findings": lint_style(st)})
record("plain", "plain")
for name, css in FAULTS.items():
    p = work / f"{name}.css"
    p.write_text(f'@import "{plain}";\n{css}\n', encoding="utf-8")
    record(name, p)
print(json.dumps(out, ensure_ascii=False))
