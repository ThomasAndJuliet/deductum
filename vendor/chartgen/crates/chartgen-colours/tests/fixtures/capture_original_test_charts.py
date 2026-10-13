"""Run original test modules with chartgen.render wrapped; record every drawn chart's spec and the
colour classes of its data marks, decisions and findings (oracle evidence for the colour plan)."""
import copy, json, sys, unittest, hashlib
from pathlib import Path
ORACLE = Path.home() / "chartgen-continue/oracle/charts"
sys.path.insert(0, str(ORACLE)); sys.path.insert(0, str(ORACLE / "tests")); sys.path.insert(0, str(ORACLE.parent / "tools"))
import chartgen
import xml.etree.ElementTree as ET

MARKS = ("bar", "dot", "segment", "slice", "tile", "cell", "line", "projection", "area", "change-shaft",
         "step-line", "event-bar", "target-mark")
records, seen = [], set()
_render = chartgen.render

def colour_marks(svg):
    root = ET.fromstring(svg)
    parent = {c: p for p in root.iter() for c in p}
    out = []
    for el in root.iter():
        cls = (el.get("class") or "").split()
        kind = next((m for m in MARKS if m in cls), None)
        if kind is None or el.tag.endswith("}g"):
            continue
        n, series = el, None
        while n is not None and series is None:
            series = n.get("data-series"); n = parent.get(n)
        out.append([kind, series, el.get("data-x"), cls])
    return out

def wrapped(spec_json, style, variant=None, expect=None, measurer=None, source_name=None, base_dir=None):
    spec_copy = copy.deepcopy(spec_json)
    svg, report = _render(spec_json, style, variant, expect, measurer, source_name, base_dir)
    if style.name == "plain" and isinstance(spec_copy, dict):
        key = hashlib.sha256(json.dumps(spec_copy, sort_keys=True, default=str).encode()).hexdigest()
        if key not in seen:
            seen.add(key)
            rep = report.to_json()
            records.append({"test": current[0], "spec": spec_copy, "type": rep.get("type"), "status": rep.get("status"),
                            "marks": colour_marks(svg) if svg else [],
                            "decisions": [{k: d.get(k) for k in ("rule", "level", "text")} for d in rep.get("decisions", [])],
                            "findings": [{k: f.get(k) for k in ("rule", "severity", "level", "path")} for f in rep.get("findings", [])]})
    return svg, report

chartgen.render = wrapped
current = [None]

class Recorder(unittest.TextTestResult):
    def startTest(self, test):
        current[0] = test.id()
        super().startTest(test)

modules = sys.argv[2:] or ["test_breakdown", "test_stacks", "test_lines", "test_clustered", "test_dots", "test_multiples"]
suite = unittest.TestSuite()
for name in modules:
    suite.addTests(unittest.defaultTestLoader.loadTestsFromName(name))
result = unittest.TextTestRunner(resultclass=Recorder, verbosity=0, stream=open("/dev/null", "w")).run(suite)
print(f"{result.testsRun} tests, {len(result.failures)} failures, {len(result.errors)} errors; {len(records)} charts", file=sys.stderr)
Path(sys.argv[1]).write_text(json.dumps(records, ensure_ascii=False, default=str) + "\n")
