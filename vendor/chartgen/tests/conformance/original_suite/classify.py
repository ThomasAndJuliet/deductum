"""Classify every original test from the raw results of run_suite.py (rewrite and oracle control).

    python classify.py --work SCRATCH --out tests/conformance/original_suite/results

Writes ``results.json`` (one row per original test method, machine-readable),
``results.tsv`` and ``SUMMARY.md``. The canonical list of tests is the oracle
control's (every method the original modules define); a test the rewrite run
never reached (its class fixture or module import failed) is still listed, with
that failure.

Categories, decided in this order:

``inapplicable:environment``
    The oracle control itself cannot pass it here, for a documented reason in
    ``reasons.json`` (``GUIDE.md``/``eval/`` absent, Julia not installed). The
    rewrite outcome is still recorded.
``applicable:public-api-absent``
    The test calls a function of the original's public surface (listed in
    ``reasons.json``: the SVG lint, the style lint, the chooser, the
    measurer's batched API) that the rewrite's package does not provide.
    Counted as an applicable **failure**.
``inapplicable:original-internal``
    The test used an original Python internal (layout modules, scales, fmt,
    transform, timeaxis, rule tables, figure model, ...) absent from the
    rewrite; the harness's placeholder recorded the use.
``applicable``
    Everything else: run against the rewrite; outcome pass, fail or error.

For applicable tests the ``engine`` column says whether the rewrite run
called the Rust engine (``rust``: the test or its class fixture dispatched to
``chartgen._native``) or only host Python (``host-python``: e.g. the original
``spec.py`` reader the rewrite ships unchanged). A host-python pass is not
evidence about the Rust port.
"""
import argparse
import ast
import json
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(work, impl):
    out = {}
    for path in sorted((work / impl / "raw").glob("*.json")):
        out[path.stem] = json.loads(path.read_text())
    return out


def static_refs(test_file, injected):
    """{(class, method): original names absent from the rewrite that the method (or a helper it
    calls, transitively within the module) references}. Used only for tests the rewrite never ran."""
    tree = ast.parse(Path(test_file).read_text(encoding="utf-8"))
    alias = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom) and node.module and node.module.split(".")[0] == "chartgen":
            for a in node.names:
                alias[a.asname or a.name] = f"{node.module}.{a.name}"
        elif isinstance(node, ast.Import):
            for a in node.names:
                if a.name.split(".")[0] == "chartgen":
                    alias[a.asname or a.name.split(".")[0]] = a.asname and a.name or a.name.split(".")[0]
    funcs = {n.name: n for n in tree.body if isinstance(n, ast.FunctionDef)}
    classes = {n.name: n for n in tree.body if isinstance(n, ast.ClassDef)}

    def methods(cls):
        out, node = {}, classes.get(cls)
        while node is not None:
            for m in node.body:
                if isinstance(m, ast.FunctionDef):
                    out.setdefault(m.name, m)
            base = node.bases[0].id if node.bases and isinstance(node.bases[0], ast.Name) else None
            node = classes.get(base)
        return out

    def refs(fn, cls, seen):
        found = set()
        for n in ast.walk(fn):
            if isinstance(n, ast.Name) and n.id in alias:
                q = alias[n.id]
                found |= {k for k in injected if k == q or k.startswith(q + ".")}
            if isinstance(n, ast.Attribute):
                chain, cur = [n.attr], n.value
                while isinstance(cur, ast.Attribute):
                    chain.insert(0, cur.attr)
                    cur = cur.value
                if isinstance(cur, ast.Name) and cur.id in alias:
                    q = ".".join([alias[cur.id], *chain])
                    found |= {k for k in injected if q == k or q.startswith(k + ".")}
                if isinstance(cur, ast.Name) and cur.id in ("self", "cls"):
                    target = methods(cls).get(n.attr)
                    if target is not None and (cls, n.attr) not in seen:
                        seen.add((cls, n.attr))
                        found |= refs(target, cls, seen)
            if isinstance(n, ast.Call) and isinstance(n.func, ast.Name) and n.func.id in funcs \
                    and n.func.id not in seen:
                seen.add(n.func.id)
                found |= refs(funcs[n.func.id], cls, seen)
        return found

    return {(c, m): refs(fn, c, {(c, m)}) for c in classes for m, fn in methods(c).items() if m.startswith("test")}


def first_detail(test):
    for d in test.get("details", []) + [s["detail"] for s in test.get("subtests", [])]:
        if d.get("type") != "skip":
            return f"{d['type']}: {d['message']}"[:300]
    for d in test.get("details", []):
        return f"skip: {d['message']}"[:300]
    return ""


def failure_kind(row):
    d = row.get("rewrite_detail", "")
    if row["category"] == "applicable:public-api-absent":
        return "public API absent"
    if d.startswith("class fixture"):
        return "class fixture failed (render raised on an input the class needs)"
    if d.startswith("RuntimeError:"):
        return "render raised an explicit RuntimeError (no Report returned)"
    if d.startswith("AssertionError"):
        return "assertion failed (output or report differs)"
    return "other exception in the test (missing element/key in output or report, or host bug)"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=HERE / "results")
    args = parser.parse_args()
    reasons = json.loads((HERE / "reasons.json").read_text())
    oracle, rewrite = load(args.work, "oracle"), load(args.work, "rewrite")
    if set(oracle) != set(rewrite):
        raise SystemExit(f"module sets differ: {sorted(set(oracle) ^ set(rewrite))}")
    rows = []
    for module in sorted(oracle):
        o, r = oracle[module], rewrite[module]
        for side, data in (("oracle", o), ("rewrite", r)):
            if data.get("harness_failure") or data.get("import_error"):
                raise SystemExit(f"{side} {module}: the harness or the module import failed; see the raw result")
        rtests = {t["id"]: t for t in r["tests"]}
        fixtures = {t["id"]: t for t in r["tests"] if t["id"].startswith("<fixture>")}
        class_calls = r.get("class_native_calls", {})
        static = static_refs(args.work / "rewrite" / "stage" / "charts" / "tests" / f"{module}.py",
                             set(r.get("injected", {})))
        for ot in o["tests"]:
            tid = ot["id"]
            if tid.startswith("<fixture>"):
                raise SystemExit(f"oracle fixture failure {tid}: the control must define every test")
            cls = tid.split(".")[-2]
            rt = rtests.get(tid)
            fixture = next((f for k, f in fixtures.items() if k.endswith(f".{cls})")), None)
            touched = set(r.get("module_touches", []))
            touched |= {t.split(": ", 1)[1] for t in r.get("fixture_touches", []) if t.split(": ", 1)[0] in (cls,)
                        or t.split(": ", 1)[0].endswith(f".{cls})")}
            if rt is not None:
                touched |= set(rt.get("touched", []))
                r_outcome, r_detail = rt["outcome"], first_detail(rt)
                calls = rt.get("native_calls", 0) + class_calls.get(cls, 0)
            elif fixture is not None:
                touched |= set(fixture.get("touched", []))
                # never ran: what its body would use is known only statically
                touched |= {f"{n} (static: in the test body, not run)" for n in static.get((cls, tid.split(".")[-1]), ())}
                r_outcome = "error"
                r_detail = "class fixture (setUpClass) failed: " + first_detail(fixture)
                calls = None
            else:
                raise SystemExit(f"{tid}: not run by the rewrite and no fixture failure explains it")
            row = {"id": tid, "module": module, "class": cls,
                   "oracle": ot["outcome"], "oracle_detail": first_detail(ot) if ot["outcome"] != "pass" else "",
                   "rewrite": r_outcome, "rewrite_detail": r_detail if r_outcome != "pass" else "",
                   "touched": sorted(touched)}
            env = reasons["environment"].get(tid)
            public = sorted(n.split(" ")[0] for n in touched if n.split(" ")[0] in reasons["public_api"])
            detail_public = [k for k, why in reasons["public_api_attributes"].items() if k in r_detail]
            if env:
                row["category"], row["reason"] = "inapplicable:environment", env
            elif public or detail_public:
                row["category"] = "applicable:public-api-absent"
                row["reason"] = "uses public original API the rewrite package lacks: " + ", ".join(
                    [f"{n} ({reasons['public_api'][n]})" for n in public]
                    + [f"{k} ({reasons['public_api_attributes'][k]})" for k in detail_public])
                row["result"] = "fail"
            elif touched:
                row["category"] = "inapplicable:original-internal"
                row["reason"] = "exercises original Python internal(s) absent from the rewrite: " + ", ".join(sorted(touched))
            else:
                row["category"], row["reason"] = "applicable", ""
                row["result"] = "pass" if r_outcome == "pass" else ("skip" if r_outcome == "skip" else "fail")
                row["engine"] = "unknown (fixture failed)" if calls is None else ("rust" if calls else "host-python")
            if row["category"] != "applicable" and "result" not in row:
                row["result"] = "n/a"
            rows.append(row)
    args.out.mkdir(parents=True, exist_ok=True)
    cats = Counter(r["category"] for r in rows)
    app = [r for r in rows if r["category"].startswith("applicable")]
    summary = {
        "tests": len(rows), "categories": dict(sorted(cats.items())),
        "oracle_control": dict(Counter(r["oracle"] for r in rows)),
        "rewrite_raw": dict(Counter(r["rewrite"] for r in rows)),
        "applicable": {"total": len(app), "rewrite": dict(Counter(r["result"] for r in app)),
                       "oracle_control": dict(Counter(r["oracle"] for r in app)),
                       "rewrite_pass_by_engine": dict(Counter(r.get("engine") for r in app if r["result"] == "pass")),
                       "rewrite_failure_kinds": dict(Counter(failure_kind(r) for r in app if r["result"] != "pass"))},
    }
    (args.out / "results.json").write_text(json.dumps({"summary": summary, "tests": rows}, indent=1,
                                                      ensure_ascii=False) + "\n")
    cols = ["id", "category", "result", "engine", "oracle", "rewrite", "reason", "rewrite_detail"]
    with open(args.out / "results.tsv", "w", encoding="utf-8") as fh:
        fh.write("\t".join(cols) + "\n")
        for r in rows:
            fh.write("\t".join(str(r.get(c, "")).replace("\t", " ").replace("\n", " | ") for c in cols) + "\n")
    write_summary(args.out / "SUMMARY.md", rows, summary)
    print(json.dumps(summary, indent=1))


def write_summary(path, rows, summary):
    by_module = defaultdict(Counter)
    for r in rows:
        key = r["category"] if not r["category"].startswith("applicable") else f"{r['category']}/{r['result']}"
        by_module[r["module"]][key] += 1
    keys = sorted({k for c in by_module.values() for k in c})
    lines = ["# Original test suite against the rewrite", "",
             "Generated by `tests/conformance/original_suite/classify.py` from `run_suite.py` raw results.",
             "Every original test method is listed in `results.json`/`results.tsv`; nothing is skipped silently.", "",
             "## Counts", "", "```json", json.dumps(summary, indent=1), "```", "",
             "## Per module", "", "| module | " + " | ".join(keys) + " |", "|---" * (len(keys) + 1) + "|"]
    for m, c in sorted(by_module.items()):
        lines.append(f"| {m} | " + " | ".join(str(c.get(k, "")) for k in keys) + " |")
    lines += ["", "## Applicable tests that fail on the rewrite (known failures)", "",
              "| test | oracle | rewrite | first failure |", "|---|---|---|---|"]
    for r in rows:
        if r["category"].startswith("applicable") and r["result"] != "pass":
            why = r["reason"] if r["category"] == "applicable:public-api-absent" else r["rewrite_detail"]
            lines.append(f"| `{r['id'].removeprefix('tests.')}` | {r['oracle']} | {r['rewrite']} | "
                         f"{why[:220].replace('|', '/').replace(chr(10), ' ')} |")
    lines += ["", "## Applicable tests that pass on the rewrite", "", "| test | engine |", "|---|---|"]
    for r in rows:
        if r["category"] == "applicable" and r["result"] == "pass":
            lines.append(f"| `{r['id'].removeprefix('tests.')}` | {r['engine']} |")
    lines += ["", "## Inapplicable tests", "", "| test | category | reason | rewrite outcome |", "|---|---|---|---|"]
    for r in rows:
        if r["category"].startswith("inapplicable"):
            lines.append(f"| `{r['id'].removeprefix('tests.')}` | {r['category'].split(':')[1]} | "
                         f"{r['reason'][:200]} | {r['rewrite']} |")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
