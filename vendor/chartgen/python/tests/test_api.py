import json
from decimal import Decimal
import pytest
import chartgen
from chartgen.style import Style
from chartgen.measure import Measurer, ApproximateMeasurer

def spec():
    return {"chartspec":1,"id":"example","purpose":"analysis","title":{"text":"Revenue"},"data":{"x":{"kind":"nominal","values":["East","West"]},"measure":{"name":"revenue","kind":"amount","zero":"meaningful"},"series":[{"id":"revenue","name":"Revenue","values":[10,20]}]},"chart":{"type":"bar"}}

def test_native_binding_produces_legacy_return_shape():
    svg,report=chartgen.render(spec(),Style("plain"),measurer=ApproximateMeasurer(),backend="svg")
    assert svg.startswith("<svg") or "<svg" in svg
    assert report.status=="drawn"
    assert report.type=="bar"
    assert report.to_json()["chart"]=="example"

def test_refusal_does_not_call_measurer():
    class NeverMeasure:
        def measure_requests(self,requests):
            pytest.fail("Refused chart must not request native text measurement")
    s=spec(); s["purpose"]="communication"
    svg,report=chartgen.render(s,Style("plain"),measurer=NeverMeasure())
    assert svg is None
    assert report.status=="refused"
    assert "R0-07" in report.rules_found()

def test_decimal_crosses_binding_without_binary_float_rounding():
    value=Decimal("0.1234567890123456789012345678")
    assert chartgen._json({"value":value})=='{"value":0.1234567890123456789012345678}'

def test_typst_native_measurement_and_rust_layout():
    svg,report=chartgen.render(spec(),Style("plain"),measurer=Measurer(),backend="typst")
    plan=json.loads(svg)
    assert report.status=="drawn"
    assert any(layer["kind"]=="text" for layer in plan["layers"])
    assert any(layer["kind"]=="svg" for layer in plan["layers"])

def test_old_batched_measurer_is_adapted():
    # The original's measurer interface: measure([(classes, code)]) -> [Extent], typography by class.
    css = Style("plain").css + "\n.extra-size { font-size: 9; }\n"
    seen = []
    class Legacy:  # the original interface only: no measure_requests
        def __init__(self, css):
            self.css, self.inner = css, Measurer(css)

        def style_of(self, classes):
            return self.inner.style_of(classes)

        def measure(self, items):
            seen.extend(items)
            return self.inner.measure(items)
    legacy, report = chartgen.render(spec(), Style("plain"), measurer=Legacy(css), backend="svg")
    direct, _ = chartgen.render(spec(), Style("plain"), measurer=Measurer(), backend="svg")
    assert report.status == "drawn" and seen
    assert all(classes[0] == "prose" for classes, _ in seen)
    assert legacy == direct

def test_registry_resolution_preserves_spec_and_own_slots(tmp_path):
    (tmp_path/"colours.json").write_text('{"Revenue":16,"Other":2}')
    source=spec();source["colours"]={"registry":"colours.json","slots":{"Revenue":3}}
    resolved=chartgen._resolve_registry(source,tmp_path)
    assert resolved["colours"]["slots"]=={"Revenue":3,"Other":2}
    assert "registry" not in resolved["colours"]
    assert source["colours"]["registry"]=="colours.json"
    assert len(Style("plain").to_theme()["palette"])==16

def test_invalid_registry_returns_report_before_measurement(tmp_path):
    (tmp_path/"colours.json").write_text('{"Revenue":true}')
    source=spec();source["colours"]={"registry":"colours.json"}
    output,report=chartgen.render(source,Style("plain"),expect={"refused":["CG-4"]},base_dir=tmp_path,measurer=object())
    assert output is None and report.status=="invalid" and report.expect_met
    assert report.errors()[0].path=="colours.registry"


def test_library_marks_render_on_every_backend():
    # paper/ISSUES.md #2: action dots (the original's <use href="#mark-dot">) on portable backends.
    from pathlib import Path
    from chartgen.spec import read_file
    spec = read_file(Path(__file__).resolve().parents[2] / "tests/conformance/examples/specs/p3-54-action-dots.json")[0]
    for backend in ("svg", "pgf", "typst", "compatibility_svg"):
        out, report = chartgen.render(spec, Style("plain"), backend=backend)
        assert report.status == "drawn", backend
        if backend == "compatibility_svg":
            assert 'href="#mark-dot"' in out
        else:
            assert "<use" not in out and ("<circle" in out or "pgfpathcircle" in out), backend
