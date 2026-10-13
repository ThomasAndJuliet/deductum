"""Per-rule recall of the rewrite against the oracle, over runs both completed (no exception).

    python tests/conformance/inventory/rule_recall.py ORACLE_DIR REWRITE_DIR [PROBE_DIR] --out rules.json

For every rule ID the original emits (findings by severity, decisions, chooser
path) it counts the runs where the original emits it and, of those, the runs
where the rewrite emits the same rule (with the same severity for findings).
With PROBE_DIR (tools/conformance/probes.py run outputs: oracle-*.jsonl and
rewrite-*.jsonl) the probe runs are counted too. Rules the rewrite emits that the
original never does in the same run are listed as extra.
"""
import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path


def items(report):
    out = set()
    for f in report.get("findings", []):
        out.add(("finding", f"{f.get('rule')}/{f.get('severity')}"))
    for d in report.get("decisions", []):
        out.add(("decision", d.get("rule")))
    for p in report.get("path", []):
        if isinstance(p, dict):
            out.add(("path", p.get("rule")))
    return out


def runs_from_dirs(o_dir, r_dir):
    pairs = []
    for path in sorted(Path(o_dir).glob("*.json")):
        if path.name.startswith("_"):
            continue
        o = json.loads(path.read_text())
        rp = Path(r_dir) / path.name
        if not rp.exists():
            continue
        r = json.loads(rp.read_text())
        if "exception" in o or "exception" in r:
            continue
        pairs.append((o["key"], o["report"], r["report"]))
    return pairs


def runs_from_probes(directory):
    def load(prefix):
        out = {}
        for p in Path(directory).glob(f"{prefix}-*.jsonl"):
            for line in p.read_text().splitlines():
                e = json.loads(line)
                out[e["id"]] = e
        return out
    o, r = load("oracle"), load("rewrite")
    return [(k, o[k]["report"], r[k]["report"]) for k in sorted(o.keys() & r.keys())
            if "exception" not in o[k] and "exception" not in r[k]]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("oracle", type=Path)
    parser.add_argument("rewrite", type=Path)
    parser.add_argument("probes", type=Path, nargs="?")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    pairs = runs_from_dirs(args.oracle, args.rewrite)
    if args.probes:
        pairs += runs_from_probes(args.probes)
    emitted, recalled, extra, examples = Counter(), Counter(), Counter(), defaultdict(list)
    for key, o, r in pairs:
        io, ir = items(o), items(r)
        for it in io:
            emitted[it] += 1
            if it in ir:
                recalled[it] += 1
            elif len(examples[it]) < 3:
                examples[it].append(key)
        for it in ir - io:
            extra[it] += 1
    rows = [{"kind": k, "rule": rule, "oracle_runs": n, "rewrite_also": recalled[(k, rule)],
             "missed_examples": examples[(k, rule)]} for (k, rule), n in sorted(emitted.items())]
    result = {"runs": len(pairs), "rules": rows,
              "rewrite_extra": [{"kind": k, "rule": rule, "runs": n} for (k, rule), n in sorted(extra.items())]}
    args.out.write_text(json.dumps(result, indent=1) + "\n")
    zero = [r for r in rows if r["rewrite_also"] == 0]
    print(json.dumps({"runs": len(pairs), "rule_kinds": len(rows), "never_recalled": len(zero),
                      "fully_recalled": sum(r["rewrite_also"] == r["oracle_runs"] for r in rows)}, indent=1))


if __name__ == "__main__":
    main()
