"""The original spec API (chartgen.spec) and host defaults.

Expected (path, rule) pairs below come from running the original
(charts/chartgen/spec.py check_structure) on the same specs; the engine's
messages are worded differently.
"""
import copy
import json
from pathlib import Path

import chartgen
from chartgen.spec import check_structure, load, read_file

SPECS = Path(__file__).resolve().parents[2] / "tests" / "conformance" / "examples" / "specs"


def base():
    return copy.deepcopy(read_file(SPECS / "p1-01-headcount-by-region.json")[0])


def paths(problems):
    return [(p, rule) for p, _, rule in problems]


def test_irregular_points_week_grids_and_events_are_checked():
    d = base()
    d["data"]["x"] = {"kind": "time", "grid": "none"}
    d["data"]["series"][0].pop("values")
    d["data"]["series"][0]["points"] = [["2022-03-01", 5], ["2022-01-01", 4]]
    assert paths(check_structure(load(d))) == [("data.series[0].points[1]", "CG-4")]
    d = base()
    d["data"]["x"] = {"kind": "time", "grid": "week",
                      "values": ["2022-06-20", "2022-06-28", "2022-07-04", "2022-07-11", "2022-07-18"]}
    assert ("data.x.values", "CG-4") in paths(check_structure(load(d)))   # original: 2022-06-28 is a Tuesday
    d = base()
    d["data"]["x"] = {"kind": "time", "grid": "month", "values": ["2022-01", "2022-02", "2022-03", "2022-04", "2022-05"],
                      "events": [{"at": "soon", "label": "x"}]}
    assert paths(check_structure(load(d))) == [("data.x.events[0].at", "CG-4")]
    assert check_structure(load(base())) == []


def test_default_width_is_the_originals_and_layout_height_is_passed(monkeypatch):
    # charts/chartgen/spec.py LayoutOpts: width 368 (the paper's text width), height none.
    sent = []
    monkeypatch.setattr(chartgen, "dispatch", lambda op: sent.append(op) or {"result": {
        "status": "stopped", "report": {"chart": "x", "style": "plain", "variant": None, "type": None,
                                        "status": "refused", "decisions": [], "findings": [], "overrides": [],
                                        "expect": {}, "expect_met": True, "source": {}, "summary": {}, "path": [],
                                        "copies": {}}}})
    spec = base()
    spec.pop("layout", None)
    chartgen.render(spec, "plain", measurer=object())
    assert sent[0]["request"]["constraints"]["width"] == 368
    assert sent[0]["request"]["constraints"]["height"] is None
    spec["layout"] = {"width": 300, "height": 200}
    chartgen.render(spec, "plain", measurer=object())
    assert (sent[1]["request"]["constraints"]["width"], sent[1]["request"]["constraints"]["height"]) == (300, 200)
