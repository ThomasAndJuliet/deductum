"""Generate differential fixtures for figkit, okhsl and bake.label_style from the ORIGINAL implementation.

Run with the oracle's Python (the original imports only the standard library here; measurement
needs the Typst 0.15.1 CLI on PATH):

    PATH=TYPST_DIR:$PATH ORACLE_PY tests/conformance/figkit/generate_fixtures.py \
        --oracle-root ORIGINAL/charts [--out tests/conformance/figkit/fixtures.json]

The original tools are copied to a temporary stage first (bake and figkit insert their own
directory into sys.path and figkit.measure writes caches), and the copied files are checked
against tests/conformance/oracle-source.sha256. Inputs are deterministic (seeded); every
expected value is what the original returns. check.py replays them against the rewrite.
"""
import argparse
import hashlib
import json
import math
import random
import shutil
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
MANIFEST = HERE.parent / "oracle-source.sha256"


def stage(oracle_root, into):
    share = oracle_root.resolve().parent
    sums = dict(reversed(l.split(None, 1)) for l in MANIFEST.read_text().splitlines())
    sums = {k.strip(): v for k, v in sums.items()}
    tools = into / "tools"
    shutil.copytree(share / "tools", tools, ignore=shutil.ignore_patterns("__pycache__"))
    shutil.copytree(oracle_root / "style", into / "style", ignore=shutil.ignore_patterns("__pycache__"))
    for rel in sums:
        if rel.startswith("tools/") and (share / rel).exists():
            if hashlib.sha256((share / rel).read_bytes()).hexdigest() != sums[rel]:
                raise SystemExit(f"{rel} differs from oracle-source.sha256")
    sys.path.insert(0, str(tools))
    return tools


def colour_cases(rng, fk_style, okhsl):
    hexes = ["#000000", "#ffffff", "#808080", "#ff0000", "#00ff00", "#0000ff", "#595959", "#d9d9d9",
             "#fff", "#abc", "  #95B7D3 ", "#767676", "#ff4f3c", "#8d2419"]
    hexes += ["#%06x" % rng.randrange(0, 1 << 24) for _ in range(40)]
    out = {"hex_rgb": [], "rgb_hex": [], "oklch": [], "from_oklch": [], "mix": [], "oklch_shift": [],
           "oklch_light": [], "luminance": [], "contrast": [], "text_tone": [], "okhsl": [],
           "srgb_to_okhsl": [], "okhsl_to_srgb": []}
    for h in hexes:
        out["hex_rgb"].append({"args": [h], "expect": list(fk_style.hex_rgb(h))})
        out["oklch"].append({"args": [h], "expect": list(fk_style.oklch(h))})
        out["luminance"].append({"args": [h], "expect": fk_style.luminance(h)})
    for _ in range(30):
        rgb = [rng.choice([rng.random(), -0.1, 1.2, 0.0, 1.0, 0.5]) for _ in range(3)]
        out["rgb_hex"].append({"args": [rgb], "expect": fk_style.rgb_hex(rgb)})
    for _ in range(60):
        L, C, h = rng.random(), rng.choice([rng.random() * 0.4, 0.0, 0.5]), rng.random() * 360
        out["from_oklch"].append({"args": [L, C, h], "expect": fk_style.from_oklch(L, C, h)})
    for _ in range(40):
        a, b = rng.choice(hexes), rng.choice(hexes)
        t = rng.choice([0.0, 1.0, 0.5, rng.random()])
        out["mix"].append({"args": [a, b, t], "expect": fk_style.mix(a, b, t)})
        dL, dC = rng.uniform(-0.3, 0.3), rng.uniform(-0.1, 0.1)
        out["oklch_shift"].append({"args": [a, dL, dC], "expect": fk_style.oklch_shift(a, dL, dC)})
        L, Cmax = rng.random(), rng.choice([None, 0.05, 0.12])
        out["oklch_light"].append({"args": [a, L] + ([] if Cmax is None else [Cmax]),
                                   "expect": fk_style.oklch_light(a, L, Cmax)})
        out["contrast"].append({"args": [a, b], "expect": fk_style.contrast(a, b)})
        fills = [rng.choice(hexes) for _ in range(rng.randrange(1, 4))]
        arg = fills[0] if len(fills) == 1 else fills
        out["text_tone"].append({"args": [arg], "expect": list(fk_style.text_tone(arg))})
    for _ in range(40):
        h, s, l = rng.random(), rng.random(), rng.random()
        out["okhsl"].append({"args": [h, s, l], "expect": okhsl.okhsl(h, s, l)})
        out["okhsl_to_srgb"].append({"args": [h, s, l], "expect": list(okhsl.okhsl_to_srgb(h, s, l))})
        r, g, b = rng.random(), rng.random(), rng.random()
        out["srgb_to_okhsl"].append({"args": [r, g, b], "expect": list(okhsl.srgb_to_okhsl(r, g, b))})
    return out


ESCAPES = ["$300,000", "a // b", "~x", "-5", "+5", "= x", "1. Low", "12. High", "1.5 m", "a--b", "a---b",
           "Int'l", "R&D", "x_y*z", "#1", "@ref", "<tag>", "[a]", "`code`", "back\\slash", "  many   spaces\n",
           "line\nbreak", "", "–dash", "100%", "a/b", "émigré", "日本", "1.", "1.x", "-", "--"]


def escape_cases(rng, typst):
    chars = list("abc 01.-+=#$*_@<>[]`~/\\'\"&%\n\t")
    strings = ESCAPES + ["".join(rng.choice(chars) for _ in range(rng.randrange(1, 12))) for _ in range(60)]
    return [{"args": [s], "expect": typst.escape(s)} for s in strings]


STYLE_CASES = {
    "fallback-and-em": {
        "main.css": ":root { --label-size: 9; --gap: 1.5em; --a: var(--missing, #ff0000); --b: var(--a); "
                    "--size: 0.8em; }\n.title { font-size: 12; fill: var(--b); }\n.x { font-size: var(--size); }\n"},
    "colour-functions": {
        "main.css": ":root { --c: #95b7d3; --m: mix(var(--c), #ffffff, 0.5); --s: oklch-shift(var(--c), -0.1, 0.02); "
                    "--l: oklch-light(var(--c), 0.9); --lc: oklch-light(var(--c), 0.6, 0.05); "
                    "--nested: mix(oklch-shift(#336699, 0.1, 0), #000000, 0.25); }\n.bar { fill: var(--m); stroke: var(--s); }\n"},
    "imports-and-defs": {
        "main.css": "/* @defs marks.svg */\n@import \"base dir/base.css\";\n@import url(more.css);\n@import 'base dir/base.css';\n"
                    ":root { --label-size: 11; }\n.title { font-weight: bold; }\n",
        "base dir/base.css": ":root { --text: #595959; --label-size: 10; }\n.prose { fill: var(--text); font-size: 1em; }\n",
        "more.css": "/* a comment { with: braces; } */\n:root { --grid: #dddddd; }\n.gridline { stroke: var(--grid); }\n"},
    "nested-defs": {
        "main.css": "@import \"inner.css\";\n:root { --x: 1; }\n",
        "inner.css": "/* @defs inner-defs.svg */\n:root { --y: 2; }\n"},
    "cycle": {"main.css": ":root { --a: var(--b); --b: var(--a); }\n"},
    "undefined": {"main.css": ":root { --a: var(--nope); }\n"},
    "undefined-in-rule": {"main.css": ":root { --a: 1; }\n.t { fill: var(--zzz); }\n"},
    "not-a-number": {"main.css": ":root { --label-size: big; --w: 2; }\n"},
    "negative-and-zero": {"main.css": ":root { --label-size: 10; --n: -0.5em; --z: 0em; --f: .25em; }\n"},
}


def style_cases(fk_style, style_dir, work):
    out = []
    names = sorted(p.stem for p in style_dir.glob("*.css"))
    for name in names:
        files = {p.name: p.read_text(encoding="utf-8") for p in style_dir.iterdir() if p.is_file()}
        out.append(snapshot(fk_style, f"original:{name}", files, f"{name}.css", work))
    for case, files in STYLE_CASES.items():
        out.append(snapshot(fk_style, case, files, "main.css", work))
    return out


def snapshot(fk_style, case, files, entry, work):
    root = work / f"style-{len(list(work.iterdir()))}"
    for rel, text in files.items():
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        (root / rel).write_text(text, encoding="utf-8")
    rec = {"case": case, "files": files, "entry": entry}
    try:
        s = fk_style.Style(root / entry)
    except Exception as error:
        rec["error"] = {"type": type(error).__name__, "message": str(error).replace(str(root), "<root>")}
        return rec
    metrics = {}
    for k in s.tokens:
        try:
            metrics[k] = s.metric(k)
        except ValueError as error:
            metrics[k] = {"error": str(error)}
    rec["expect"] = {"name": s.name, "tokens": s.tokens, "css": s.css,
                     "defs": None if s.defs is None else str(Path(s.defs).relative_to(root.resolve())),
                     "metric": metrics, "rules": s.rules}
    return rec


def label_style_cases(bake, fk_style, style_dir, golden):
    classes = set()
    for svg in sorted(golden.glob("*.svg"))[:400]:
        for m in __import__("re").finditer(r'<text class="([^"]+)"', svg.read_text(encoding="utf-8")):
            classes.add(m.group(1))
    classes |= {"prose", "math", "prose title", "prose missing-note", "prose text-strong category-label",
                "math scale-label", "prose unknown-role", "prose text-series-3 row-header"}
    out = []
    for name in sorted(p.stem for p in style_dir.glob("*.css")):
        try:
            css = fk_style.Style(style_dir / f"{name}.css").css
        except ValueError:      # base.css is only imported by the others; it does not stand alone
            continue
        for c in sorted(classes):
            out.append({"style": name, "classes": c.split(), "expect": list(bake.label_style(css, c.split()))})
    return out


def geometry_cases(rng, model, emit):
    n = emit._n
    out = {"text_box": [], "box": [], "curve": [], "wedge": [], "overlap": []}
    for anchor in ("start", "middle", "end"):
        for _ in range(4):
            ext = {"width": rng.uniform(0, 80), "ascent": rng.uniform(0, 10), "descent": rng.uniform(0, 3)}
            t = model.Text("category-label", "x", rng.uniform(-50, 300), rng.uniform(0, 300), anchor=anchor,
                           extent=type("E", (), ext)(), modifiers=("text-strong",))
            pad = rng.choice([0.0, 1.5])
            out["text_box"].append({"text": {"role": "category-label", "code": "x", "x": t.x, "y": t.y,
                                             "anchor": anchor, "modifiers": ["text-strong"]},
                                    "extent": ext, "pad": pad, "expect": list(t.box(pad)), "classes": t.classes})
    for _ in range(10):
        pad = rng.choice([0.0, 2.0])
        r = model.Rect(rng.uniform(0, 100), rng.uniform(0, 100), rng.uniform(0, 50), rng.uniform(0, 50), ("bar",))
        out["box"].append({"kind": "Rect", "args": {"x": r.x, "y": r.y, "w": r.w, "h": r.h, "classes": ["bar"]},
                           "pad": pad, "expect": list(r.box(pad))})
        pts = [(rng.uniform(0, 100), rng.uniform(0, 100)) for _ in range(rng.randrange(1, 6))]
        p = model.Path(pts, ("line",))
        out["box"].append({"kind": "Path", "args": {"points": pts, "classes": ["line"]}, "pad": pad,
                           "expect": list(p.box(pad))})
        u = model.Use("mark-dot", rng.uniform(0, 100), rng.uniform(0, 100), rng.uniform(1, 10), ("dot",))
        out["box"].append({"kind": "Use", "args": {"href": "mark-dot", "x": u.x, "y": u.y, "size": u.size,
                                                   "classes": ["dot"]}, "pad": pad, "expect": list(u.box(pad))})
    for _ in range(6):
        segs = [tuple((rng.uniform(0, 100), rng.uniform(0, 100)) for _ in range(3)) for _ in range(rng.randrange(1, 3))]
        head = [(rng.uniform(0, 100), rng.uniform(0, 100)) for _ in range(rng.choice([0, 4]))] or None
        c = model.Curve((1.0, 2.0), segs, ("comment-arrow", "x"), head=head)
        out["curve"].append({"args": {"start": [1.0, 2.0], "segments": segs, "classes": ["comment-arrow", "x"],
                                      "head": head}, "points": c.points(), "head_classes": list(c.head_classes),
                             "box": list(c.box(1.0))})
    spans = [(0, 0), (0, 360), (10, 370), (0, 90), (45, 300), (0, 0.5), (350, 359.9), (90, 270), (0, 180), (0, 181)]
    spans += [tuple(sorted((rng.uniform(0, 360), rng.uniform(0, 360)))) for _ in range(30)]
    for a0, a1 in spans:
        for r0, inset in ((0.0, 0.0), (0.0, 0.75), (40.0, 0.0), (40.0, 1.5), (5.0, 6.0), (0.0, 90.0)):
            w = model.Wedge(100.0, 100.0, 80.0, a0, a1, ("slice", "series-1"), r0=r0, inset=inset)
            sh = w.shape()
            out["wedge"].append({"args": {"x": 100.0, "y": 100.0, "r": 80.0, "a0": a0, "a1": a1,
                                          "classes": ["slice", "series-1"], "r0": r0, "inset": inset},
                                 "shape": json.loads(json.dumps(sh)), "path": w.path(n), "box": list(w.box(0.5)),
                                 "point": [list(w.point(a0)), list(w.point(a1, 10.0))]})
    for _ in range(30):
        a = sorted([rng.uniform(0, 10), rng.uniform(0, 10)])
        b = sorted([rng.uniform(0, 10), rng.uniform(0, 10)])
        A = (a[0], b[0], a[1], b[1])
        B = rng.choice([(A[2], A[1], A[2] + 3, A[3]), (A[0] + 1, A[1] + 1, A[2] + 1, A[3] + 1),
                        tuple(sorted([rng.uniform(0, 10), rng.uniform(0, 10)])[i % 2] for i in range(4))])
        out["overlap"].append({"args": [list(A), list(B)], "expect": model.overlap(A, B)})
    return out


def figure_cases(model, emit):
    """Whole figures as constructor descriptions; the expectation is emit.svg's text."""
    def text(role, code, x, y, **kw):
        return {"type": "Text", "args": {"role": role, "code": code, "x": x, "y": y, **kw}}

    figs = []
    root = {"type": "Group", "args": {"classes": ["chart", "bar", "vertical", "categorical"], "id": "chart"}, "children": [
        {"type": "Group", "args": {"classes": ["header"]}, "children": [
            text("title", "Sales \\$ & <more>", 0, 8.334), text("subtitle", "by region", 0, 20.0, anchor="middle",
                                                                    modifiers=["text-strong"])]},
        {"type": "Group", "args": {"classes": ["grid"]}, "children": [
            {"type": "Line", "args": {"x1": 19, "y1": 230.4, "x2": 368, "y2": 230.4, "classes": ["gridline", "zero"],
                                      "data": {"value": "0"}}},
            {"type": "Line", "args": {"x1": 19.004, "y1": -0.001, "x2": 368, "y2": 0, "classes": ["gridline"],
                                      "data": {"value": "5", "skip": None}}}]},
        {"type": "Group", "args": {"classes": ["marks"], "attrs": {"clip-path": "url(#c)", "data-n": "a\"b"}}, "children": [
            {"type": "Rect", "args": {"x": 25.456, "y": 44.93, "w": 16.17, "h": 185.475, "classes": ["bar", "single"],
                                      "data": {"series": "days", "x": "2026-01", "value": "31"}, "id": "b1", "rx": 1.5}},
            {"type": "Path", "args": {"points": [[1, 2], [3.333, 4.005], [5, 6]], "classes": ["line", "series-2"],
                                      "data": {"series": "s"}}},
            {"type": "Path", "args": {"points": [[0, 10], [10, 0], [20, 5]], "classes": ["bump"], "curve": "flat"}},
            {"type": "Use", "args": {"href": "mark-dot", "x": 50, "y": 60, "size": 7.5, "classes": ["dot", "single"],
                                     "data": {"x": "Jasmine"}}},
            {"type": "Curve", "args": {"start": [0, 0], "segments": [[[1, 1], [2, 2], [3, 3]]],
                                       "classes": ["comment-arrow"], "head": [[3, 3], [4, 2], [4, 4]]}},
            {"type": "Wedge", "args": {"x": 100, "y": 100, "r": 50, "a0": 0, "a1": 120, "classes": ["slice", "series-1"],
                                       "inset": 0.5}},
            {"type": "Wedge", "args": {"x": 100, "y": 100, "r": 50, "a0": 120, "a1": 120, "classes": ["slice"]}},
            {"type": "Wedge", "args": {"x": 100, "y": 100, "r": 50, "a0": 0, "a1": 360, "classes": ["slice"], "r0": 20}}]},
        {"type": "Group", "args": {"classes": ["empty"]}, "children": []},
        text("category-label", "East", 93.2, 193.06, anchor="end", id="t1")]}
    figs.append({"width": 368, "height": 248.29, "title": "Days & <nights>", "desc": "the insight -- here",
                 "comment": "x: GENERATED -- do not edit", "defs": ['<mask id="m"/>'], "root": root})
    figs.append({"width": 120.004, "height": 0.001, "root": {"type": "Group", "args": {}, "children": []}})

    def build(d):
        cls = getattr(model, d["type"])
        args = dict(d["args"])
        for k in ("classes", "modifiers"):
            if k in args:
                args[k] = tuple(args[k])
        for k in ("points", "segments", "head", "start"):
            if k in args and args[k] is not None:
                args[k] = json.loads(json.dumps(args[k]), object_hook=None)
                if k == "start":
                    args[k] = tuple(args[k])
                elif k == "points" or k == "head":
                    args[k] = [tuple(p) for p in args[k]]
                else:
                    args[k] = [tuple(tuple(p) for p in s) for s in args[k]]
        el = cls(**args)
        if d["type"] == "Group":
            for c in d.get("children", []):
                el.children.append(build(c))
        return el

    out = []
    for f in figs:
        fig = model.Figure(f["width"], f["height"], build(f["root"]), title=f.get("title"), desc=f.get("desc"),
                           comment=f.get("comment"), defs=list(f.get("defs", [])))
        out.append({"figure": f, "expect": emit.svg(fig)})
    return out


def measure_cases(measure, fk_style, style_dir):
    css = fk_style.Style(style_dir / "plain.css").css
    m = measure.Measurer(css)
    items = [(("title",), "Days in Each Month, 2026"), (("category-label",), "East"),
             (("scale-label",), "1,200"), (("text-strong", "category-label"), "Central"),
             (("missing-note",), "(missing)"), (("unit-phrase",), "K in donations"),
             (("category-label",), "Hg"), (("title",), "\\$300,000 for x\\_y"), (("subtitle",), "Q2 2022")]
    ext = m.measure(items)
    return {"typst": m.version, "cases": [{"classes": list(c), "code": code, "style": list(m.style_of(c)),
                                           "expect": [e.width, e.ascent, e.descent]} for (c, code), e in zip(items, ext)]}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--oracle-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=HERE / "fixtures.json")
    parser.add_argument("--golden", type=Path, default=HERE.parent / "oracle" / "plain")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        stage(args.oracle_root, tmp)
        import bake
        import okhsl
        from figkit import emit, measure, model, typst
        from figkit import style as fk_style
        rng = random.Random(20261002)
        work = tmp / "styles"
        work.mkdir()
        fixtures = {
            "generated_from": "original tools/figkit, tools/okhsl.py, tools/bake.py (oracle-source.sha256)",
            "escape": escape_cases(rng, typst),
            "colour": colour_cases(rng, fk_style, okhsl),
            "style": style_cases(fk_style, tmp / "style", work),
            "label_style": label_style_cases(bake, fk_style, tmp / "style", args.golden),
            "geometry": geometry_cases(rng, model, emit),
            "emit": figure_cases(model, emit),
        }
        try:
            fixtures["measure"] = measure_cases(measure, fk_style, tmp / "style")
        except Exception as error:  # recorded, never hidden
            fixtures["measure"] = {"error": f"{type(error).__name__}: {error}"}
    args.out.write_text(json.dumps(fixtures, indent=1, ensure_ascii=False, allow_nan=False) + "\n", encoding="utf-8")
    print({k: (len(v) if isinstance(v, list) else {kk: len(vv) for kk, vv in v.items()} if isinstance(v, dict) else v)
           for k, v in fixtures.items() if k != "generated_from"})


if __name__ == "__main__":
    main()
