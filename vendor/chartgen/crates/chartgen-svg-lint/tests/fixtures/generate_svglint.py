"""SVG-lint fixtures from the original (charts/chartgen/svglint.py), run in the oracle environment:

    venv-oracle/bin/python generate_svglint.py ORACLE_ROOT GOLDEN_DIR CACHE OUT_DIR

writes OUT_DIR/svglint_cases.json: the original plain style's flattened CSS; for every golden source
SVG (by key) and every SVG the original's test_svglint lints (its fixtures and faulty copies of
generated charts, kept whole), the text extents the original measured (Typst, in document order)
and the findings of lint_svg with them (rule, severity, level, message, elements)."""
import json, sys
from pathlib import Path
root, golden, cache, out = (Path(a) for a in sys.argv[1:5])
sys.path.insert(0, str(root / "tools")); sys.path.insert(0, str(root / "charts")); sys.path.insert(0, str(root / "charts" / "tests"))
import chartgen.measure as cm
cm.Measurer.__init__.__defaults__ = (cache,)          # never the original's own cache file
from chartgen.style import Style
import chartgen.svglint as SL

STYLE = Style("plain")
MEASURER = cm.Measurer(STYLE.css, cache)
_lint = SL.lint_svg

def extents_of(svg):
    d = SL.Drawing(svg, STYLE, None)
    d.measure(MEASURER)
    return [[t.extent.width, t.extent.ascent, t.extent.descent] if t.extent else None for t in d.texts]

def findings(fs):
    return [{"rule": f.rule, "severity": f.severity, "level": f.level, "message": f.message,
             "elements": list(f.elements)} for f in fs]

cases = []
def recording(svg_text, style=None, measurer=None, extents=None, generated=False, final_width=None,
              cvd="assume", date_format=None):
    style_, measurer_ = style, measurer
    if extents is None and measurer_ is not None and style_ is not None and style_.name == "plain":
        ext = extents_of(svg_text)
        fs = _lint(svg_text, style_, extents=[cm.Extent(*e) if e else None for e in ext], generated=generated,
                   final_width=final_width, cvd=cvd, date_format=date_format)
        cases.append({"source": current[0], "svg": svg_text, "extents": ext, "generated": generated,
                      "final_width": final_width, "cvd": cvd, "date_format": date_format, "findings": findings(fs)})
        return fs
    return _lint(svg_text, style_, measurer_, extents, generated, final_width, cvd, date_format)

current = ["?"]
SL.lint_svg = recording
import unittest
import test_svglint
test_svglint.lint_svg = recording
class R(unittest.TextTestResult):
    def startTest(self, test):
        current[0] = test.id(); super().startTest(test)
suite = unittest.defaultTestLoader.loadTestsFromModule(test_svglint)
res = unittest.TextTestRunner(resultclass=R, verbosity=0, stream=open("/dev/null", "w")).run(suite)
print(f"test_svglint: {res.testsRun} tests, {len(res.failures)} failures, {len(res.errors)} errors", file=sys.stderr)
tests_cases = [c for c in cases if "Generated" not in c["source"]]     # every example is in the golden set below
gold = []
for svg in sorted(golden.glob("*.svg")):
    text = svg.read_text(encoding="utf-8")
    ext = extents_of(text)
    gold.append({"key": svg.stem, "extents": ext,
                 "findings": findings(_lint(text, STYLE, extents=[cm.Extent(*e) if e else None for e in ext]))})
MEASURER.save()
(out / "svglint_cases.json").write_text(json.dumps({"style_css": STYLE.css, "golden": gold, "tests": tests_cases},
                                                   ensure_ascii=False) + "\n", encoding="utf-8")
print(f"{len(gold)} golden, {len(tests_cases)} test cases", file=sys.stderr)
