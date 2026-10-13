"""The default measurer measures exactly as the original's figkit measurer.

Evidence: python/tests/fixtures/oracle_measure.json holds labels and extents from
the original's measurement cache (charts/.cache/measure.json, written by
tools/figkit/measure.py through tools/bake.py with the Typst 0.15.1 CLI). The
rewrite measures with the typst Python package (0.15.0 is the newest release),
and must reproduce every extent to the original's four-decimal rounding.
"""
import json
from pathlib import Path

import pytest

import chartgen
from chartgen.measure import Extent, Measurer, label_style
from chartgen.style import Style

FIXTURE = json.loads((Path(__file__).parent / "fixtures" / "oracle_measure.json").read_text(encoding="utf-8"))


def _request(i, label):
    return {"id": f"l{i}", "content": {"kind": "native", "host": "typst", "key": label["code"]},
            "role": "label", "font_family": "New Computer Modern", "font_size": label["size"],
            "style": {"fill": "#595959", "weight": label["weight"] or "normal",
                      "font_style": label["slant"] or "normal"},
            "max_width": None, "context": "test"}


def test_fixture_covers_the_original_typography():
    groups = {(l["size"], l["weight"], l["slant"]) for l in FIXTURE["labels"]}
    assert len(FIXTURE["labels"]) >= 400
    assert {(10.0, None, None), (12.0, "bold", None), (10.0, None, "italic"), (8.0, None, "italic")} <= groups


def test_requests_measure_exactly_as_the_original():
    labels = [l for l in FIXTURE["labels"] if l["code"]]
    got = Measurer().measure_requests([_request(i, l) for i, l in enumerate(labels)])
    wrong = []
    for i, label in enumerate(labels):
        width, ascent, descent = label["extent"]
        expected = {"width": width, "height": round(ascent + descent, 4), "baseline": ascent}
        if got[f"l{i}"] != expected:
            wrong.append((label, got[f"l{i}"]))
    assert not wrong, f"{len(wrong)} labels differ, e.g. {wrong[:3]}"


def test_original_interface_measures_by_class_typography():
    # figkit.Measurer.measure([(classes, code)]): typography from bake.label_style on the stylesheet.
    css = Style("plain").css
    assert label_style(css, ["prose", "title"])[::2] == (12.0, "bold")
    assert label_style(css, ["prose", "footnote"])[0::3] == (8.0, "italic")
    by = {(l["code"], l["size"], l["weight"], l["slant"]): l["extent"] for l in FIXTURE["labels"]}
    picks = [(cls, key) for cls, typo in ((("title",), (12.0, "bold", None)), ((), (10.0, None, None)),
                                          (("footnote",), (8.0, None, "italic")))
             for key in [k for k in by if k[1:] == typo][:5]]
    extents = Measurer(css).measure([(cls, key[0]) for cls, key in picks])
    assert extents == [Extent(*by[key]) for _, key in picks]


def test_wrapped_label_reports_widest_line_without_single_line_baseline():
    text = "A long title that cannot fit on one line within a narrow chart"
    request = {**_request(0, {"code": text, "size": 12.0, "weight": "bold", "slant": None}), "max_width": 120.0}
    single = Measurer().measure_requests([{**request, "max_width": None}])["l0"]
    wrapped = Measurer().measure_requests([request])["l0"]
    assert single["width"] > 120 and wrapped["width"] <= 120
    assert wrapped["height"] > 2 * single["height"] and wrapped["baseline"] is None
    fits = Measurer().measure_requests([{**request, "max_width": single["width"] + 1}])["l0"]
    assert fits == single


def test_legacy_measurer_receives_classes_giving_requested_typography():
    css = Style("plain").css
    seen = []

    class Legacy:  # the original interface only: no measure_requests
        def __init__(self, css):
            self.css, self.inner = css, Measurer(css)

        def style_of(self, classes):
            return self.inner.style_of(classes)

        def measure(self, items):
            seen.extend(items)
            return self.inner.measure(items)

    requests = [{**_request(0, {"code": "x", "size": 12.0, "weight": "bold", "slant": None}), "role": "title",
                 "content": {"kind": "plain", "text": "Int'l $5"}},
                {**_request(1, {"code": "x", "size": 8.0, "weight": None, "slant": "italic"}), "role": "footnote",
                 "content": {"kind": "plain", "text": "a note"}},
                {**_request(2, {"code": "x", "size": 10.0, "weight": None, "slant": None}), "role": "category-label",
                 "content": {"kind": "plain", "text": "East"}}]
    legacy = chartgen._measure(Legacy(css), requests)
    assert seen == [(("prose", "title"), "Int'l \\$5"), (("prose", "footnote"), "a note"), (("prose",), "East")]
    assert legacy == Measurer().measure_requests(requests)


def test_legacy_measurer_cannot_silently_change_typography():
    css = Style("plain").css

    class Legacy:
        def __init__(self):
            self.css = css

        def measure(self, items):
            pytest.fail("typography mismatch must be reported before measuring")

    request = {**_request(0, {"code": "x", "size": 9.5, "weight": "bold", "slant": "italic"}), "role": "title"}
    with pytest.raises(ValueError, match="requested typography"):
        chartgen._measure(Legacy(), [request])
    with pytest.raises(ValueError, match="style_of"):
        chartgen._measure(type("Bare", (), {"measure": lambda self, items: []})(), [request])


FIGKIT = json.loads((Path(__file__).resolve().parents[2] / "tests" / "conformance" / "figkit" /
                     "fixtures.json").read_text(encoding="utf-8"))


def test_figkit_measure_fixtures_through_the_original_interface():
    # tests/conformance/figkit/fixtures.json "measure": figkit.measure.Measurer under the plain style.
    cases = FIGKIT["measure"]["cases"]
    assert len(cases) == 9
    css = Style("plain").css
    m = Measurer(css)
    for case in cases:
        assert list(m.style_of(case["classes"])) == case["style"], case
    got = m.measure([(c["classes"], c["code"]) for c in cases])
    assert [list(e.__dict__.values()) for e in got] == [c["expect"] for c in cases]
    assert m.one(cases[0]["classes"], cases[0]["code"]) == Extent(*cases[0]["expect"])
    assert m.runs == 1                                   # one batch; the repeat came from the cache


def test_cache_keys_follow_the_original_and_persist(tmp_path):
    path = tmp_path / "measure.json"
    m = Measurer(Style("plain").css, cache_path=path)
    m.measure([(("title",), "Revenue")])
    keys = [json.loads(k) for k in json.loads(path.read_text())]
    # figkit: [mode, code, size, weight, slant, Typst version]
    assert keys == [["prose", "Revenue", 12.0, "bold", None, m.version]]
    again = Measurer(Style("plain").css, cache_path=path)
    again.measure([(("title",), "Revenue")])
    assert again.runs == 0
