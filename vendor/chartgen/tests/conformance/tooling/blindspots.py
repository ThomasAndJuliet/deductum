"""Demonstrate what compare.py lets through: mutate oracle golden runs and compare them with the originals.

    python tests/conformance/tooling/blindspots.py [--out tests/conformance/tooling/BLINDSPOTS.md]

Each mutation alters one golden output or report in a way a reader would see or
a consumer would depend on. compare.py's verdict for the mutant against the
unmutated golden run is recorded (``match`` means compare.py, and so the ratchet,
cannot see the difference), next to the verdict of compare_strict.py. Nothing
here changes compare.py; it documents its blind spots with reproducible cases.
"""
import argparse
import copy
import json
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools" / "conformance"))
import compare  # noqa: E402
import compare_strict  # noqa: E402

GOLDEN = ROOT / "tests" / "conformance" / "oracle" / "plain"


def svg_mutation(pattern, repl, count=1, flags=0):
    def apply(text):
        out, n = re.subn(pattern, repl, text, count=count, flags=flags)
        return out if n else None
    return apply


def shift_numbers(attr, delta):
    def apply(text):
        def bump(m):
            nums = re.sub(r"[-+]?\d+\.?\d*", lambda n: f"{float(n.group(0)) + delta:.2f}", m.group(2))
            return f'{m.group(1)}"{nums}"'
        out, n = re.subn(rf'(\s{attr}=)"([^"]*)"', bump, text, count=1)
        return out if n else None
    return apply


def move_group(first, after):
    def apply(text):
        m = re.search(rf'(\n\s*<g class="{first}">.*?\n\s*</g>)', text, re.S)
        if not m or f'<g class="{after}">' not in text[m.end():]:
            return None
        rest = text[:m.start()] + text[m.end():]
        n = re.search(rf'\n\s*<g class="{after}">.*?\n\s*</g>', rest, re.S)
        return rest[:n.end()] + m.group(1) + rest[n.end():] if n else None
    return apply


def swap_texts(cls):
    def apply(text):
        found = re.findall(rf'(<text class="{cls}"[^>]*>)([^<]+)(</text>)', text)
        if len(found) < 2 or found[0][1] == found[1][1]:
            return None
        a, b = found[0], found[1]
        text = text.replace("".join(a), "\0A", 1).replace("".join(b), "\0B", 1)
        return text.replace("\0A", a[0] + b[1] + a[2], 1).replace("\0B", b[0] + a[1] + b[2], 1)
    return apply


def all_bars_shift(delta):
    def apply(text):
        out, n = re.subn(r'(<rect class="bar[^"]*" x=")([\d.]+)"', lambda m: f'{m.group(1)}{float(m.group(2)) + delta:.2f}"', text)
        return out if n else None
    return apply


def report_mutation(fn):
    def apply(report):
        report = copy.deepcopy(report)
        return report if fn(report) is not False else None
    return apply


def reverse_labels(rep):
    labels = (rep.get("summary") or {}).get("labels")
    if not labels or len(labels) < 2:
        return False
    rep["summary"]["labels"] = list(reversed(labels))


def change_message(rep):
    if not rep.get("findings"):
        return False
    rep["findings"][0]["message"] = "a different explanation"
    rep["findings"][0]["supply"] = "something else"


def move_finding_path(rep):
    fs = [f for f in rep.get("findings", []) if f.get("path")]
    if len(fs) < 1:
        return False
    dup = dict(fs[0])
    dup["severity"] = "info" if fs[0]["severity"] != "info" else "warn"
    rep["findings"].append(dup)
    rep["findings"].append(dict(fs[0]))
    rep["findings"].remove(fs[0])


MUTATIONS = [
    ("line/area/wedge geometry: every coordinate of the first <path d> moved 20 units",
     "svg", shift_numbers("d", 20.0), ("line", "pie", "area")),
    ("figure size: root width/viewBox changed from the original's", "svg",
     svg_mutation(r'viewBox="0 0 ([\d.]+) ([\d.]+)" width="[\d.]+"', r'viewBox="0 0 480 \2" width="480"'), ("bar",)),
    ("mark symbol: a dot drawn with another symbol (use href)", "svg", svg_mutation(r'href="#mark-dot"', 'href="#mark-square"'), ("dot",)),
    ("z-order: gridlines drawn over the marks", "svg", move_group("grid", "marks"), ("bar",)),
    ("text anchor: a label anchored at its other end", "svg",
     svg_mutation(r'(<text class="prose category-label"[^>]*?)( text-anchor="middle")', r'\1 text-anchor="end"'), ("bar",)),
    ("inline colour: a texture pattern's ink changed", "svg", svg_mutation(r'(<pattern id="texture-bad"[^>]*><rect[^>]*fill=")#[0-9a-f]{6}', r'\1#00ff00'), ("bar", "dot", "line")),
    ("swapped labels: two category names exchanged (same multiset)", "svg", swap_texts(r"prose category-label"), ("bar",)),
    ("sub-tolerance drift: every bar moved 0.45 units", "svg", all_bars_shift(0.45), ("bar",)),
    ("unit-bearing geometry: a bar width written as '12pt' (unparseable, skipped)", "svg",
     svg_mutation(r'(<rect class="bar[^"]*" x="[\d.]+" y="[\d.]+" width=")[\d.]+"', r'\g<1>12pt"'), ("bar",)),
    ("<title> text changed", "svg", svg_mutation(r"<title>[^<]*</title>", "<title>Another title</title>"), ("bar",)),
    ("report: scale labels in reverse order (summary lists are sorted before comparing)", "report",
     report_mutation(reverse_labels), ("bar",)),
    ("report: a finding's message and supply text changed (prose not compared, by design)", "report",
     report_mutation(change_message), ("bar", "line", "dot")),
    ("report: one finding's path moved to another severity (paths compared as a set)", "report",
     report_mutation(move_finding_path), ("bar", "line", "dot", "table", "pie")),
]


def find_run(kind_prefixes, mutate, target):
    for path in sorted(GOLDEN.glob("*.json")):
        entry = json.loads(path.read_text())
        rep = entry.get("report") or {}
        svg = path.with_suffix(".svg")
        if not svg.exists() or (rep.get("type") or "").split("_")[0] not in kind_prefixes and rep.get("type") not in kind_prefixes:
            continue
        if target == "svg":
            out = mutate(svg.read_text())
            if out is not None and out != svg.read_text():
                return entry, svg.read_text(), out, rep
        else:
            out = mutate(rep)
            if out is not None:
                return entry, svg.read_text(), svg.read_text(), out
    return None


def verdicts(entry, svg_a, svg_b, rep_b):
    with tempfile.TemporaryDirectory() as d:
        a, b = Path(d, "a"), Path(d, "b")
        a.mkdir(), b.mkdir()
        key = entry["key"]
        (a / f"{key}.svg").write_text(svg_a)
        (b / f"{key}.svg").write_text(svg_b)
        (a / f"{key}.json").write_text(json.dumps(entry))
        (b / f"{key}.json").write_text(json.dumps({**entry, "report": rep_b}))
        ra, rb = compare.load_runs(a)[key], compare.load_runs(b)[key]
        lenient = compare.judge(key, ra, rb, a, b)["verdict"]
        strict = compare_strict.strict_report(ra["report"], rb["report"])
        va, vb = compare_strict.view(compare_strict.parse(ra["_svg"])), compare_strict.view(compare_strict.parse(rb["_svg"]))
        strict.update(compare_strict.compare_views(va, vb, 0.02))
        caught = sorted(k for k, v in strict.items() if v != "agree")
        return lenient, caught


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, default=Path(__file__).resolve().parent / "BLINDSPOTS.md")
    args = parser.parse_args()
    rows = []
    for name, target, mutate, kinds in MUTATIONS:
        found = find_run(kinds, mutate, target)
        if found is None:
            rows.append((name, "-", "no golden run has the element", "-"))
            continue
        entry, svg_a, svg_b, rep_b = found
        lenient, caught = verdicts(entry, svg_a, svg_b, rep_b)
        rows.append((name, entry["key"], lenient, ", ".join(caught) or "not caught"))
    lines = ["# compare.py blind spots, demonstrated", "",
             "Generated by `tests/conformance/tooling/blindspots.py`: each row mutates one oracle golden run and compares",
             "the mutant with the unmutated run. `compare.py` = its verdict (`match` means the ratchet cannot see the",
             "change); `compare_strict.py` = the strict checks that differ.", "",
             "| mutation | golden run | compare.py | compare_strict.py |", "|---|---|---|---|"]
    lines += [f"| {n} | `{k}` | {v} | {s} |" for n, k, v, s in rows]
    args.out.write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    missed = [r for r in rows if r[2] == "match"]
    print(f"\n{len(missed)} of {len(rows)} mutations pass compare.py as a match")


if __name__ == "__main__":
    main()
