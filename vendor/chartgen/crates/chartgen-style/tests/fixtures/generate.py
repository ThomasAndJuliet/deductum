"""Stylesheet fixtures from the ORIGINAL figkit style and bake.label_style, supplementing the
independent review's tests/conformance/figkit/fixtures.json with edge cases: the three @import
syntaxes, import cycles and repeats, defs precedence, token order, var() fallbacks, em
boundaries and rounding, colour-function errors, literal rules, metric parsing, and
label_style with own values, fall-through and errors.

    ORACLE_PY crates/chartgen-style/tests/fixtures/generate.py ORIGINAL/tools > style.json
"""
import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, sys.argv[1])
import bake  # noqa: E402
from figkit import style as fk  # noqa: E402

CASES = {
    "import-syntaxes": {
        "main.css": '@import "a b.css";\n@import \'single.css\';\n@import url(bare.css);\n@import url("q q.css") ;\n'
                    "@import url( 'sp.css' );\n:root { --m: 1; }\n",
        "a b.css": ":root { --a: 1; }\n", "single.css": ":root { --s: 2; }\n", "bare.css": ":root { --b: 3; }\n",
        "q q.css": ":root { --q: 4; }\n", "sp.css": ":root { --sp: 5; }\n"},
    "import-cycle-and-repeat": {
        "main.css": '@import "b.css";\n@import "b.css";\n:root { --main: 1; }\n',
        "b.css": '@import "main.css";\n@import "sub/c.css";\n:root { --b: 2; }\n',
        "sub/c.css": '@import "../b.css";\n/* @import "d.css"; */\n:root { --c: 3; }\n.c { fill: var(--d); }\n',
        "sub/d.css": ":root { --d: #123; }\n"},
    "import-cycle-ok": {
        "main.css": '@import "b.css";\n:root { --main: 1; }\n',
        "b.css": '@import "main.css";\n@import "sub/c.css";\n@import "./b.css";\n:root { --b: 2; }\n',
        "sub/c.css": '@import "../b.css";\n@import "../main.css";\n:root { --c: 3; }\n'},
    "defs-own-wins": {
        "main.css": '@import "x.css";\n/* @defs own.svg */\n', "x.css": "/* @defs x.svg */\n"},
    "defs-first-import": {
        "main.css": '@import "x.css";\n@import "y.css";\n', "x.css": ":root{--x:1}\n",
        "y.css": "/*   @defs   sub/y-defs.svg   */\n"},
    "token-order-and-roots": {
        "main.css": ":root { --a: var(--c) var(--b); --b: 2; color: red; }\n.x { stroke: var(--b); }\n"
                    ":root { --a: var(--b); --c: 3; --d: var(--c); }\n.y {}\n.z { nocolon; fill : #fff ; ; }\n"
                    "p , .q\n  { background: url(a:b) ; }\n"},
    "fallbacks": {
        "main.css": ":root { --a: var( --none , 12px ); --b: var(--none, var(--a)); --c: var(--none,); "
                    "--d: var(--none, mix(#000000, #ffffff, 0.5)); --e: [var(--a)] [var(--none, x y)]; }\n"},
    "em-boundaries": {
        "main.css": ":root { --label-size: 8px; --a: 1em 1.5em -2em .5em; --b: #1em a1em x-1em 1emx 1em-x; "
                    "--c: calc(2em + 3em); --d: 0.03125em 0.09375em 0.00005em 1.000049em; --e: 1.em 1..5em; "
                    "--f: 2em2em 3em; }\n.t { font-size: 1.25em; margin: 1em; }\n"},
    "em-label-size-from-var": {
        "main.css": ":root { --base: 12; --label-size: var(--base); --x: 2em; }\n"},
    "em-label-size-empty": {
        "main.css": ":root { --label-size: ; --x: 2em; }\n"},
    "em-label-size-not-number": {
        "main.css": ":root { --label-size: big; --x: 2em; }\n"},
    "em-label-size-self": {
        "main.css": ":root { --label-size: 2em; }\n"},
    "em-in-rule-without-label-size": {
        "main.css": ".t { font-size: 1.2em; }\n"},
    "functions": {
        "main.css": ":root { --a: remix(#ffffff, #000000, 0.25); --b: mix(#fff,#000,1e-1); --c: oklch-light(#336699, 2); "
                    "--d: oklch-shift(#336699, -1, -1); --e: mix(  #FFF , #000 , .5 , extra ); "
                    "--f: oklch-light(#336699, 0.5, 0); --g: mix(##fff, #000, 0.5); }\n"},
    "mix-too-few": {"main.css": ":root { --a: mix(#fff, #000); }\n"},
    "mix-bad-colour": {"main.css": ":root { --a: mix(red, #000, 0.5); }\n"},
    "mix-bad-second": {"main.css": ":root { --a: mix(#000, #12, 0.5); }\n"},
    "mix-bad-number": {"main.css": ":root { --a: mix(#fff, #000, half); }\n"},
    "light-too-few": {"main.css": ":root { --a: oklch-light(#fff); }\n"},
    "shift-bad": {"main.css": ":root { --a: oklch-shift(#fff, 0.1, x); }\n"},
    "font-size-px": {
        "main.css": ":root { --s: 12; }\n.a { font-size: var(--s); }\n.b { font-size: 12.5; }\n.c { font-size: 12px; }\n"
                    ".d { font-size: -.5; }\n.e { font-size: 1e3; }\n.f { FONT-SIZE: 9; }\n"},
    "metrics": {
        "main.css": ":root { --a: 12px; --b:  -3.5pt; --c: .25; --d: 1e3; --e: abc; --f: 'q'; --g: \"x'y\"; "
                    "--h: -; --i: 1_000; }\n"},
    "whitespace": {
        "main.css": ":root {\n  --a:\u00a0two\u00a0\u00a0words\x1c;\n\t--b :\n x \n y ;\n}\n.r\n.s\t{ fill:  #000 ; }\n"},
    "self-direct": {"main.css": ":root { --a: var(--a); }\n"},
    "self-long": {"main.css": ":root { --x: 1; --a: var(--b); --b: var(--c, 2); --c: var(--a); }\n"},
}


def snapshot(case, files, work):
    root = work / case
    for rel, text in files.items():
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        (root / rel).write_text(text, encoding="utf-8")
    rec = {"case": case, "files": files, "entry": "main.css"}
    try:
        s = fk.Style(root / "main.css")
    except Exception as e:
        rec["error"] = {"type": type(e).__name__, "message": str(e).replace(str(root.resolve()), "<root>")}
        return rec
    metric = {}
    for k in s.tokens:
        try:
            metric[k] = s.metric(k)
        except ValueError as e:
            metric[k] = {"error": str(e)}
    rec["expect"] = {"name": s.name, "tokens": list(s.tokens.items()), "css": s.css, "rules": s.rules,
                     "defs": None if s.defs is None else str(Path(s.defs).relative_to(root.resolve())),
                     "metric": metric}
    return rec


LABEL_CSS = """/* first line .title */
:root { --x: 1; }
.title, .big { font-size: 14px; fill: #ABC; font-weight: bold; }
.title { font-size: 99; font-style: italic; }
.zero { font-size: 0; fill: none; font-weight: 400; font-style: normal; }
.pt { font-size: 9pt; fill: #123456; font-weight: 700; }
.light { font-weight: lighter; font-style: OBLIQUE; }
.bolder { font-weight: Bolder; }
.prose { font-size: 10px; fill: #595959; }
.math { font-size: 11px; fill: #000000; }
.title-x { font-size: 50; }
.dashed { stroke-fill: #ff0000; my-font-size: 3; fill:#0f0 }
.empty { fill: ; font-size:  ; }
.bad { font-size: large; }
.badnum { font-size: 1e; }
.sign { font-size: +12; }
.weird { font-weight: 950; font-style: italic oblique; }
"""
LABEL = [
    (["prose", "title"], None), (["title", "prose"], None), (["math", "title"], None), (["prose"], None),
    (["math"], None), ([], None), (["unknown"], None), (["prose", "zero"], None), (["prose", "pt"], None),
    (["prose", "light"], None), (["prose", "bolder"], None), (["prose", "title-x"], None),
    (["prose", "dashed"], None), (["prose", "empty"], None), (["prose", "big", "pt"], None),
    (["prose", "sign"], None), (["prose", "weird"], None), (["prose", "math"], None),
    (["prose", "title", "bad"], None), (["prose", "bad"], None), (["prose", "badnum"], None),
    (["prose", "title"], {"font-size": "7", "fill": "#fff", "font-weight": "600", "font-style": "italic"}),
    (["prose", "title"], {"font-size": "0", "fill": "red", "font-weight": "normal"}),
    (["prose", "bad"], {"font-size": "8"}),
    (["prose"], {"font-size": "6pt", "fill": " #ABCDEF "}),
]


def label_cases():
    out = []
    for classes, own in LABEL:
        try:
            v = {"value": list(bake.label_style(LABEL_CSS, classes, own))}
        except Exception as e:
            v = {"error": {"type": type(e).__name__, "message": str(e)}}
        out.append({"classes": classes, "own": own, **v})
    return out


def main():
    with tempfile.TemporaryDirectory() as tmp:
        styles = [snapshot(c, f, Path(tmp)) for c, f in CASES.items()]
    nums = [0, -0.0, 1, 0.5, 1 / 3, -2 / 3, 0.00005, 0.00015, 0.03125, 0.09375, 1e-9, -1e-9, 123456.78905,
            12.0, 1e16, float("inf"), float("-inf")]
    texts = ["a /* x */ b /* y\n z */ c", "/* unterminated", "/**/x/***/", ".a{b:c}\n.d { e : f g ; h }\n:root{--x:1;--y : 2 }"]
    print(json.dumps({"styles": styles, "label_css": LABEL_CSS, "label_style": label_cases(),
                      "num": [[x if x == x and abs(x) != float("inf") else str(x), fk._num(x)] for x in nums],
                      "strip_comments": [[t, fk.strip_comments(t)] for t in texts],
                      "parse": [[fk.strip_comments(t), fk.parse(fk.strip_comments(t))] for t in texts]},
                     indent=1, ensure_ascii=False))


main()
