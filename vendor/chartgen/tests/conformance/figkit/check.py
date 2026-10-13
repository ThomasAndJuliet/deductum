"""Replay the original figkit/okhsl/bake fixtures against the rewrite (differential checks).

    python tests/conformance/figkit/check.py [--target facade|host-copy|all] [--out results.json --md RESULTS.md]

Targets:

``facade``     the migration target of agents/FIGKIT.md: the Python package ``chartgen.figkit``
               (``typst``, ``style`` with the colour functions, ``model``, ``emit``, ``measure``)
               and ``chartgen.figkit.okhsl``, backed by the Rust crates. Until those land every
               family is reported ``unavailable``: a known failure, never a skip.
``oracle``     a control: the original tools themselves (``--oracle-tools``), which must pass every family.
``host-copy``  what the rewrite's package holds today: copies of the original Python
               (``chartgen.typst.escape``, ``chartgen._support.style``, ``chartgen._support.okhsl``,
               ``chartgen.measure.Measurer``). A pass here is NOT evidence for the Rust layers; the
               report says how many calls reached ``chartgen._native``.

Each family is: escape, colour (13 functions), okhsl, style (flattening, tokens, metric,
defs, errors), label_style, geometry (Text/Rect/Path/Use/Curve/Wedge boxes, wedge shape/path,
overlap), emit (whole figures), measure (Typst extents, tolerance 0.01 pt). Fixtures come
from the original through generate_fixtures.py; the oracle is not needed here. Exit status is
0; the result records every failure.
"""
import argparse
import importlib
import json
import math
import tempfile
import traceback
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
CALLS = [0]

FACADE = {"typst": "chartgen.figkit.typst", "style": "chartgen.figkit.style", "okhsl": "chartgen.figkit.okhsl",
          "model": "chartgen.figkit.model", "emit": "chartgen.figkit.emit", "measure": "chartgen.figkit.measure",
          "label_style": "chartgen.figkit.style"}
ORACLE = {"typst": "figkit.typst", "style": "figkit.style", "okhsl": "okhsl", "model": "figkit.model",
          "emit": "figkit.emit", "measure": "figkit.measure", "label_style": "bake"}
HOST = {"typst": "chartgen.typst", "style": "chartgen._support.style", "okhsl": "chartgen._support.okhsl",
        "model": None, "emit": None, "measure": "chartgen.measure", "label_style": "chartgen.style"}


class Unavailable(Exception):
    pass


def module(target, key):
    name = {"facade": FACADE, "host-copy": HOST, "oracle": ORACLE}[target][key]
    if name is None:
        raise Unavailable(f"the rewrite has no {key} module in its {target}")
    try:
        return importlib.import_module(name)
    except ImportError as error:
        raise Unavailable(f"{name}: {error}") from error


def attr(mod, name):
    if not hasattr(mod, name):
        raise Unavailable(f"{mod.__name__}.{name} is absent")
    return getattr(mod, name)


def close(a, b, tol):
    if isinstance(a, (list, tuple)) and isinstance(b, (list, tuple)):
        return len(a) == len(b) and all(close(x, y, tol) for x, y in zip(a, b))
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(close(a[k], b[k], tol) for k in a)
    if isinstance(a, bool) or isinstance(b, bool) or a is None or b is None:
        return a == b
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return math.isclose(a, b, rel_tol=tol, abs_tol=tol)
    return a == b


def plain(v):
    return json.loads(json.dumps(v, default=lambda o: list(o) if isinstance(o, tuple) else str(o)))


class Family:
    def __init__(self, name):
        self.name, self.counts, self.failures, self.unavailable = name, Counter(), [], None

    def case(self, label, fn, expect, tol=1e-9):
        try:
            got = plain(fn())
        except Unavailable:
            raise
        except Exception as error:
            got = {"raised": f"{type(error).__name__}: {error}"}
        ok = close(got, expect, tol)
        self.counts["pass" if ok else "fail"] += 1
        if not ok and len(self.failures) < 8:
            self.failures.append({"case": label, "expect": str(expect)[:300], "got": str(got)[:300]})


def run_escape(fx, target, fam):
    escape = attr(module(target, "typst"), "escape")
    for c in fx["escape"]:
        fam.case(repr(c["args"][0]), lambda: escape(*c["args"]), c["expect"])


def run_colour(fx, target, fam):
    st = module(target, "style")
    for fname, cases in fx["colour"].items():
        if fname in ("okhsl", "srgb_to_okhsl", "okhsl_to_srgb"):
            continue
        fn = attr(st, fname)
        for c in cases:
            fam.case(f"{fname}{tuple(c['args'])}", lambda: fn(*c["args"]), c["expect"])


def run_okhsl(fx, target, fam):
    ok = module(target, "okhsl")
    for fname in ("okhsl", "srgb_to_okhsl", "okhsl_to_srgb"):
        fn = attr(ok, fname)
        for c in fx["colour"][fname]:
            fam.case(f"{fname}{tuple(c['args'])}", lambda: fn(*c["args"]), c["expect"])


def run_style(fx, target, fam):
    Style = attr(module(target, "style"), "Style")
    for c in fx["style"]:
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            for rel, text in c["files"].items():
                (root / rel).parent.mkdir(parents=True, exist_ok=True)
                (root / rel).write_text(text, encoding="utf-8")

            def snap():
                try:
                    s = Style(root / c["entry"])
                except Exception as error:
                    return {"error": {"type": type(error).__name__, "message": str(error).replace(str(root), "<root>")}}
                metric = {}
                for k in s.tokens:
                    try:
                        metric[k] = s.metric(k)
                    except ValueError as error:
                        metric[k] = {"error": str(error)}
                css = s.css.replace("chartgen-rs", "figkit").replace("chartgen", "figkit")
                return {"expect": {"name": s.name, "tokens": s.tokens, "css": css,
                                   "defs": None if s.defs is None else str(Path(s.defs).relative_to(root.resolve())),
                                   "metric": metric, "rules": plain(s.rules)}}
            want = {"error": c["error"]} if "error" in c else {"expect": c["expect"]}
            fam.case(c["case"], snap, want)


def run_label_style(fx, target, fam):
    mod = module(target, "label_style")
    label_style = attr(mod, "label_style")
    Style = attr(module(target, "style"), "Style")
    css = {}
    styles = HERE.parent.parent.parent / "python" / "chartgen" / "styles"
    for c in fx["label_style"]:
        if c["style"] not in css:
            css[c["style"]] = Style(styles / f"{c['style']}.css").css
        fam.case(f"{c['style']}:{' '.join(c['classes'])}", lambda: label_style(css[c["style"]], c["classes"]), c["expect"])


def build(model, d):
    cls = attr(model, d["type"])
    args = dict(d["args"])
    for k in ("classes", "modifiers"):
        if k in args:
            args[k] = tuple(args[k])
    if "start" in args and args["start"] is not None:
        args["start"] = tuple(args["start"])
    for k in ("points", "head"):
        if args.get(k) is not None:
            args[k] = [tuple(p) for p in args[k]]
    if args.get("segments") is not None:
        args["segments"] = [tuple(tuple(p) for p in s) for s in args["segments"]]
    el = cls(**args)
    for child in d.get("children", []):
        el.children.append(build(model, child))
    return el


def run_geometry(fx, target, fam):
    model = module(target, "model")
    n = attr(module(target, "emit"), "_n")
    g = fx["geometry"]
    for c in g["text_box"]:
        ext = type("Extent", (), c["extent"])()
        t = build(model, {"type": "Text", "args": {**c["text"], "extent": ext}})
        fam.case(f"Text.box {c['text']['anchor']}", lambda: t.box(c["pad"]), c["expect"])
        fam.case("Text.classes", lambda: t.classes, c["classes"])
    for c in g["box"]:
        el = build(model, {"type": c["kind"], "args": c["args"]})
        fam.case(f"{c['kind']}.box", lambda: el.box(c["pad"]), c["expect"])
    for c in g["curve"]:
        el = build(model, {"type": "Curve", "args": c["args"]})
        fam.case("Curve.points", lambda: el.points(), c["points"])
        fam.case("Curve.head_classes", lambda: el.head_classes, c["head_classes"])
        fam.case("Curve.box", lambda: el.box(1.0), c["box"])
    for c in g["wedge"]:
        w = build(model, {"type": "Wedge", "args": c["args"]})
        a = c["args"]
        label = f"Wedge a0={a['a0']:.2f} a1={a['a1']:.2f} r0={a['r0']} inset={a['inset']}"
        fam.case(label + " shape", lambda: w.shape(), c["shape"])
        fam.case(label + " path", lambda: w.path(n), c["path"])
        fam.case(label + " box", lambda: w.box(0.5), c["box"])
        fam.case(label + " point", lambda: [w.point(a["a0"]), w.point(a["a1"], 10.0)], c["point"])
    overlap = attr(model, "overlap")
    for c in g["overlap"]:
        fam.case(f"overlap{c['args']}", lambda: overlap(*c["args"]), c["expect"])


def run_emit(fx, target, fam):
    model, emit = module(target, "model"), module(target, "emit")
    svg = attr(emit, "svg")
    Figure = attr(model, "Figure")
    for i, c in enumerate(fx["emit"]):
        f = c["figure"]
        fig = lambda: Figure(f["width"], f["height"], build(model, f["root"]), title=f.get("title"),
                             desc=f.get("desc"), comment=f.get("comment"), defs=list(f.get("defs", [])))
        fam.case(f"figure {i}", lambda: svg(fig()), c["expect"])


def run_measure(fx, target, fam):
    if "error" in fx["measure"]:
        raise Unavailable(f"the fixture could not be generated: {fx['measure']['error']}")
    Measurer = attr(module(target, "measure"), "Measurer")
    Style = attr(module(target, "style"), "Style")
    styles = HERE.parent.parent.parent / "python" / "chartgen" / "styles"
    m = Measurer(Style(styles / "plain.css").css)
    if not hasattr(m, "measure"):
        raise Unavailable(f"{type(m).__module__}.Measurer has no original batched measure(items) API")
    items = [(tuple(c["classes"]), c["code"]) for c in fx["measure"]["cases"]]
    ext = m.measure(items)
    for c, e in zip(fx["measure"]["cases"], ext):
        fam.case(f"{c['classes']} {c['code']}", lambda: [e.width, e.ascent, e.descent], c["expect"], tol=0.01)


FAMILIES = [("escape", run_escape), ("colour", run_colour), ("okhsl", run_okhsl), ("style", run_style),
            ("label_style", run_label_style), ("geometry", run_geometry), ("emit", run_emit), ("measure", run_measure)]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fixtures", type=Path, default=HERE / "fixtures.json")
    parser.add_argument("--target", choices=("facade", "host-copy", "all", "oracle"), default="all")
    parser.add_argument("--oracle-tools", type=Path, help="with --target oracle: the original tools/ directory "
                                                           "(a control: the fixtures replayed against their source)")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--md", type=Path)
    args = parser.parse_args()
    fx = json.loads(args.fixtures.read_text(encoding="utf-8"))
    if args.target == "oracle":
        import sys
        sys.path.insert(0, str(args.oracle_tools.resolve()))
    try:
        native = importlib.import_module("chartgen._native")
        dispatch = native.dispatch

        def counted(*a, **k):
            CALLS[0] += 1
            return dispatch(*a, **k)
        native.dispatch = counted
    except ImportError:
        pass
    results = {}
    for target in (("facade", "host-copy") if args.target == "all" else (args.target,)):
        rows = {}
        for name, fn in FAMILIES:
            fam = Family(name)
            CALLS[0] = 0
            try:
                fn(fx, target, fam)
                verdict = "pass" if not fam.counts["fail"] else "fail"
            except Unavailable as error:
                verdict, fam.unavailable = "unavailable", str(error)
            except Exception as error:  # recorded, never hidden
                verdict, fam.unavailable = "harness-error", f"{type(error).__name__}: {error}\n{traceback.format_exc(limit=-3)}"
            rows[name] = {"verdict": verdict, "cases": dict(fam.counts), "native_calls": CALLS[0],
                          "unavailable": fam.unavailable, "first_failures": fam.failures}
        results[target] = rows
    known = [f"{t}:{f}" for t, rows in results.items() for f, r in rows.items() if r["verdict"] != "pass"]
    out = {"fixtures": str(args.fixtures.name), "results": results, "known_failures": known}
    text = json.dumps(out, indent=1, ensure_ascii=False)
    if args.out:
        args.out.write_text(text + "\n", encoding="utf-8")
    if args.md:
        lines = ["# figkit differential checks against the rewrite", "",
                 "Generated by `tests/conformance/figkit/check.py` from `fixtures.json` (expected values produced",
                 "by the original figkit/okhsl/bake via `generate_fixtures.py`). `facade` is the FIGKIT.md target",
                 "(`chartgen.figkit`, Rust-backed); `host-copy` is the original Python the rewrite package copies",
                 "today: a pass there with 0 native calls is not evidence for the Rust layers.", "",
                 "| target | family | verdict | cases pass/fail | native calls | note |", "|---|---|---|---|---|---|"]
        for t, rows in results.items():
            for f, r in rows.items():
                note = (r["unavailable"] or (r["first_failures"][0]["case"] + ": expected " + r["first_failures"][0]["expect"][:80]
                                             + " got " + r["first_failures"][0]["got"][:80] if r["first_failures"] else ""))
                lines.append(f"| {t} | {f} | {r['verdict']} | {r['cases'].get('pass', 0)}/{r['cases'].get('fail', 0)} | "
                             f"{r['native_calls']} | {note.splitlines()[0][:220].replace('|', '/') if note else ''} |")
        lines += ["", "Known failures (until the figkit layers land): " + ", ".join(f"`{k}`" for k in known)]
        args.md.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(json.dumps({t: {f: r["verdict"] for f, r in rows.items()} for t, rows in results.items()}, indent=1))


if __name__ == "__main__":
    main()
