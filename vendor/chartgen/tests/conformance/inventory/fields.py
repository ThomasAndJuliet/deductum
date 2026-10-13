"""Per-option status of the rewrite, from the silent-disappearance probes, written into INVENTORY.md.

    python tests/conformance/inventory/fields.py [--silent tests/conformance/silent/result.json]

For every option of the original schema it combines the probe verdicts
(tools/conformance/probes.py) with references to the option in the rewrite's
Rust source, and assigns a conservative status:

``ignored``        the original reacts and the rewrite never does: every informative probe is SILENT or
                   OUTPUT-IGNORED
``partly ignored`` some informative probes SILENT/OUTPUT-IGNORED, others react or raise
``refusal missed`` the original refuses (or marks invalid/unsupported) and the rewrite does not
``diagnosed``      the rewrite raises an explicit error whenever the option matters, never silent
``responds``       the rewrite's output or report changes whenever the original's does; equality with the
                   original is NOT established (compare.py has almost no matching runs), so this is not "ported"
``refusals agree``  as ``responds``, and on every probe where the original's status or refusals changed the
                   rewrite reaches the same status, type and refusal set
``no evidence``    only probes on which the original does not react or the rewrite fails on the base too
``not probed``     no probe reached it

Writes tests/conformance/inventory/fields.json and replaces the block between
the ``fields:begin``/``fields:end`` markers of tests/conformance/INVENTORY.md.
"""
import argparse
import json
import re
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
RUST = sorted(ROOT.glob("crates/*/src/**/*.rs")) + [ROOT / "src" / "lib.rs"]


def rust_refs(path):
    """Up to three source lines naming the option: its JSON pointer first, else its key quoted."""
    plain = path.replace("[]", "").replace("{}", "")
    pointer = "/" + plain.replace(".", "/")
    key = '"' + plain.split(".")[-1] + '"'
    hits, weak = [], []
    for f in RUST:
        if "/tests/" in str(f):
            continue
        lines = f.read_text(encoding="utf-8").splitlines()
        in_tests = False
        for i, line in enumerate(lines, 1):
            if "#[cfg(test)]" in line:
                in_tests = True
            if in_tests:
                continue
            rel = f"{f.relative_to(ROOT)}:{i}"
            if f'"{pointer}"' in line or f'"{pointer}/' in line:
                hits.append(rel)
            elif key in line and len(plain.split(".")[-1]) > 3:
                weak.append(rel)
    return hits[:3], weak[:3]


def classify(c, agree_on_policy, policy_probes):
    silent = c.get("SILENT", 0) + c.get("OUTPUT-IGNORED", 0)
    reacted, diagnosed = c.get("rewrite-reacted", 0), c.get("rewrite-diagnosed", 0)
    if not c:
        return "not probed"
    if c.get("_refusal_missed"):
        return "refusal missed"
    if silent and not (reacted or diagnosed):
        return "ignored"
    if silent:
        return "partly ignored"
    if diagnosed and not reacted:
        return "diagnosed"
    if reacted:
        if policy_probes and agree_on_policy == policy_probes:
            return "refusals agree"
        return "responds"
    return "no evidence"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--silent", type=Path, default=ROOT / "tests" / "conformance" / "silent" / "result.json")
    parser.add_argument("--inventory", type=Path, default=ROOT / "tests" / "conformance" / "INVENTORY.md")
    args = parser.parse_args()
    result = json.loads(args.silent.read_text())
    sev = {0: "status", 1: "findings", 2: "type", 3: "output", 4: "decisions/summary", 5: "path", 6: "finding detail"}
    worst = {f["path"]: sev.get(f["worst"], f["worst"]) for f in result.get("flagged_options", [])}
    per = defaultdict(Counter)
    agree, policy = Counter(), Counter()
    for row in result["probes"]:
        path = row.get("path")
        if path is None:
            continue
        if path.startswith("data.series[0].values"):
            path = f"data value: {row['value']}"
        per[path][row["verdict"]] += 1
        if row.get("refusal_missed"):
            per[path]["_refusal_missed"] += 1
        if row["verdict"] == "rewrite-reacted" and set(row.get("oracle_changed", [])) & {"status", "findings"}:
            pol = row.get("policy") or {}
            policy[path] += 1
            agree[path] += pol.get("oracle") == pol.get("rewrite")
    for n in result.get("not_probed", []):
        per.setdefault(n["path"], Counter())
    rows = []
    for path in sorted(per, key=lambda p: (p.startswith("data value"), p)):
        c = per[path]
        status = classify(c, agree[path], policy[path])
        hits, weak = rust_refs(path) if not path.startswith("data value") else ([], [])
        rows.append({"option": path, "status": status,
                     "verdicts": {k: v for k, v in sorted(c.items()) if not k.startswith("_")},
                     "refusal_missed": c.get("_refusal_missed", 0),
                     "refusals_agree": f"{agree[path]}/{policy[path]}" if policy[path] else "",
                     "flagged_original_change": worst.get(path, ""),
                     "rust_pointer_refs": hits, "rust_key_refs": weak})
    out = ROOT / "tests" / "conformance" / "inventory" / "fields.json"
    counts = Counter(r["status"] for r in rows)
    out.write_text(json.dumps({"statuses": dict(counts), "options": rows}, indent=1) + "\n")
    lines = ["Generated by `tests/conformance/inventory/fields.py` from `tests/conformance/silent/result.json`;",
             "JSON form: `tests/conformance/inventory/fields.json`.", "",
             "Status counts: " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())) + ".", "",
             "| option | status | probe verdicts | Rust references (pointer, else key name) |", "|---|---|---|---|"]
    abbreviations = {"rewrite-reacted": "reacted", "rewrite-diagnosed": "diagnosed", "rewrite-blocked": "blocked",
                     "oracle-unchanged": "orig-unchanged", "OUTPUT-IGNORED": "OUT-IGN"}
    for r in rows:
        v = ", ".join(f"{abbreviations.get(k, k)} {n}" for k, n in r["verdicts"].items())
        if r["refusal_missed"]:
            v += f"; refusal missed {r['refusal_missed']}"
        if r["refusals_agree"]:
            v += f"; refusals agree {r['refusals_agree']}"
        if r["flagged_original_change"]:
            v += f"; worst original change where flagged: {r['flagged_original_change']}"
        refs = ", ".join(f"`{h}`" for h in r["rust_pointer_refs"]) or (
            ", ".join(f"`{h}`?" for h in r["rust_key_refs"]) or "none found")
        lines.append(f"| `{r['option']}` | {r['status']} | {v or '-'} | {refs} |")
    text = args.inventory.read_text(encoding="utf-8")
    begin, end = "<!-- fields:begin -->", "<!-- fields:end -->"
    if begin not in text or end not in text:
        raise SystemExit(f"{args.inventory} lacks the {begin} … {end} markers")
    head, rest = text.split(begin, 1)
    _, tail = rest.split(end, 1)
    args.inventory.write_text(head + begin + "\n" + "\n".join(lines) + "\n" + end + tail, encoding="utf-8")
    print(json.dumps(dict(counts), indent=1))


if __name__ == "__main__":
    main()
