import json
from pathlib import Path
from decimal import Decimal

import pytest

import chartgen
from chartgen import __main__ as cli
from chartgen.measure import Extent
from chartgen.spec import read_file


def test_file_loader_and_native_dispatch_keep_decimal_lexeme(tmp_path, monkeypatch):
    path = tmp_path / "values.json"
    path.write_text('{"chartspec":1,"data":{"series":[{"values":[0.1234567890123456789012345678]}]}}')
    spec, _, _ = read_file(path)
    assert spec["data"]["series"][0]["values"][0] == Decimal("0.1234567890123456789012345678")
    captured = []
    monkeypatch.setattr(chartgen._native, "dispatch", lambda payload: captured.append(payload) or '{"version":1,"outcome":"data","value":null}')
    chartgen.dispatch({"operation": "choose", "spec": spec})
    assert "0.1234567890123456789012345678" in captured[0]


def test_old_measurer_uses_foreign_native_alternative():
    class Legacy:
        css = ".prose { font-size: 10; }"

        def measure(self, items):
            assert items == [(("prose",), "TeX\\_only")]
            return [Extent(30, 8, 2)]

    result = chartgen._measure(Legacy(), [{"id": "foreign", "role": "axis", "font_size": 10.0, "content":
                                         {"kind": "native", "host": "latex", "key": "label", "alternative": "TeX_only"}}])
    assert result["foreign"] == {"width": 30, "height": 10, "baseline": 8}


def test_unexpected_native_render_status_is_an_error(monkeypatch):
    monkeypatch.setattr(chartgen, "dispatch", lambda operation: {"result": {"status": "skipped"}})
    with pytest.raises(RuntimeError, match="Unexpected render status"):
        chartgen.render({"layout": {}}, {}, measurer=object())


def test_choose_cli_prints_path_and_refusal_fields(tmp_path, monkeypatch, capsys):
    path = tmp_path / "chart.json"
    path.write_text('{"id":"chart"}')
    monkeypatch.setattr(cli, "dispatch", lambda operation: {"value": {
        "chart": "chart", "status": "refused", "path": [{"step": 1, "name": "variables", "level": "guided", "rule": "G1.20", "text": "one series"}],
        "decisions": [], "findings": [{"severity": "error", "rule": "R0-07", "level": "locked", "message": "job required", "path": "job", "supply": "a job"}]}})
    assert cli.main(["choose", str(path)]) == 1
    output = capsys.readouterr().out
    assert "refused" in output and "variables" in output and "field: job" in output and "supply: a job" in output


def test_build_uses_backend_extension_and_reports_failure(tmp_path, monkeypatch, capsys):
    specs = tmp_path / "specs"
    specs.mkdir()
    (specs / "first.json").write_text('{"id":"first"}')
    seen = []
    monkeypatch.setattr(cli, "render_one", lambda args, path, variant, out, report: seen.append(out) or 1)
    assert cli.main(["build", str(specs), "--src", str(tmp_path / "src"), "--reports", str(tmp_path / "reports"), "--backend", "pgf"]) == 1
    assert seen[0].suffix == ".pgf"
    assert "1 chart(s)" in capsys.readouterr().err


def test_standalone_svg_lint_reports_partial_without_certifying_clean(tmp_path, monkeypatch, capsys):
    path = tmp_path / "chart.svg"
    path.write_text('<svg xmlns="http://www.w3.org/2000/svg"/>')
    calls = []
    monkeypatch.setattr(cli, "_svg_extents", lambda svg, style: None)   # texts not measured
    monkeypatch.setattr(cli, "dispatch", lambda operation: calls.append(operation) or {"value": [{
        "rule": "CG-SVG-PARTIAL", "severity": "info", "level": "locked", "message": "more checks pending", "elements": ["svg"]}]})
    output = tmp_path / "lint.json"
    assert cli.main(["lint", str(path), "--json", str(output)]) == 2
    assert calls[0]["operation"] == "lint_svg"
    assert json.loads(output.read_text())["files"][str(path)][0]["rule"] == "CG-SVG-PARTIAL"
    assert "more checks pending" in capsys.readouterr().out


def test_scene_lint_wrapper_uses_rust_protocol(monkeypatch):
    calls = []
    monkeypatch.setattr(chartgen, "dispatch", lambda operation: calls.append(operation) or {"value": []})
    assert chartgen.lint_scene({"width": 10, "height": 10, "primitives": []}) == []
    assert calls == [{"operation": "lint_scene", "scene": {"width": 10, "height": 10, "primitives": []}}]


SPECS = __import__("pathlib").Path(__file__).resolve().parents[2] / "tests" / "conformance" / "examples" / "specs"


def test_choose_prints_choice_and_defaults_as_the_original(tmp_path, monkeypatch, capsys):
    # Rows and lines as the original's __main__ prints them (tests/test_cli.py Choose cases).
    path = tmp_path / "chart.json"
    path.write_text('{"id":"chart"}')
    report = {"chart": "chart", "status": "ready", "type": "bar",
              "path": [{"step": 7, "name": "values across categories", "level": "guided", "rule": "R9-1",
                        "text": "group: bar charts", "group": "bar charts"}],
              "decisions": [{"level": "guided", "rule": "02 D5", "text": "scale 0–60 by 10"}], "findings": [],
              "choice": [{"part": "type", "value": "bar", "rule": "G1.10 (the path above)", "level": "guided"},
                         {"part": "sort", "value": "desc by headcount", "rule": "R2-01 (G2.1), R9-9", "level": "guided"}],
              "defaults": [{"path": "facts.small_differences_matter", "default": "false", "used_by": "an inset"}]}
    monkeypatch.setattr(cli, "dispatch", lambda operation: {"value": report})
    assert cli.main(["choose", str(path)]) == 0
    captured = capsys.readouterr()
    out = captured.out
    assert "The path (G1.10, in D-12 (a)'s order; step, level, rule, guide rules): group bar charts" in out
    assert "02 D5" in out
    assert __import__("re").search(r"sort\s+desc by headcount\s+\[R2-01 \(G2.1\), R9-9; guided\]", out)
    assert __import__("re").search(r"facts.small_differences_matter\s+false", out)
    assert captured.err == ""
    assert cli.main(["choose", str(path), "--json"]) == 0
    d = json.loads(capsys.readouterr().out)
    assert list(d) == ["chart", "variant", "status", "type", "path", "decisions", "choice", "findings", "defaults"]
    assert d["choice"][0]["part"] == "type"


def test_choose_names_what_the_engine_does_not_report(tmp_path, monkeypatch, capsys):
    path = tmp_path / "chart.json"
    path.write_text('{"id":"chart"}')
    monkeypatch.setattr(cli, "dispatch", lambda operation: {"value": {
        "chart": "chart", "status": "ready", "type": "bar", "path": [], "decisions": [], "findings": [],
        "choice": None, "breakdown": False}})
    assert cli.main(["choose", str(path), "--json"]) == 0
    d = json.loads(capsys.readouterr().out)
    assert d["choice"] == [] and d["unreported"]
    assert cli.main(["choose", str(path)]) == 0
    assert "does not yet report the choice" in capsys.readouterr().err


def test_choose_prints_exactly_what_the_original_prints(capsys):
    # python/tests/fixtures/oracle_choose.json: the original CLI's exit code and output.
    fixtures = json.loads((Path(__file__).parent / "fixtures" / "oracle_choose.json").read_text(encoding="utf-8"))
    for key, (code, out) in fixtures["runs"].items():
        stem, variant, as_json = key.split("--")
        argv = ["choose", str(SPECS / f"{stem}.json")] + (["--variant", variant] if variant != "None" else [])
        assert cli.main(argv + (["--json"] if as_json == "True" else [])) == code, key
        assert capsys.readouterr().out == out, key

def test_schema_and_style_commands_as_build_sh_uses_them(tmp_path, capsys):
    # tests/test_cli.py Others: style prints the mark library path and exits 0; schema prints the schema.
    assert cli.main(["style", "plain", "--out", str(tmp_path / "style.css"), "--report", str(tmp_path / "s.json")]) == 0
    captured = capsys.readouterr()
    assert captured.out.strip().endswith("plain-defs.svg")
    assert ":root" in (tmp_path / "style.css").read_text()
    report = json.loads((tmp_path / "s.json").read_text())
    # the original's test_style.Lint.test_plain_passes: the plain style has no findings
    assert report["style"] == "plain" and report["findings"] == []
    assert cli.main(["schema"]) == 0
    assert json.loads(capsys.readouterr().out)["title"] == "chartgen chart spec, version 1"


def test_render_writes_source_and_report_with_the_original_names(tmp_path, capsys):
    out, report = tmp_path / "p1-01.svg", tmp_path / "p1-01.json"
    code = cli.main(["render", str(SPECS / "p1-01-headcount-by-region.json"), "--style", "plain",
                     "--out", str(out), "--report", str(report)])
    assert code == 0 and capsys.readouterr().out.startswith("ok ")
    assert "GENERATED by chartgen from specs/p1-01-headcount-by-region.json under style plain" in out.read_text()
    assert list(json.loads(report.read_text())) == ["chart", "style", "variant", "type", "status", "decisions",
                                                    "findings", "overrides", "expect", "expect_met", "source",
                                                    "summary", "path"]


def test_build_reports_an_engine_error_and_goes_on(tmp_path, monkeypatch, capsys):
    specs = tmp_path / "specs"
    specs.mkdir()
    (specs / "a.json").write_text('{"id":"a"}')
    (specs / "b.json").write_text('{"id":"b"}')
    src = tmp_path / "src"
    src.mkdir()
    (src / "a.svg").write_text("stale")
    seen = []

    def fake(args, path, variant, out, report):
        seen.append(Path(out).name)
        if Path(out).name == "a.svg":
            raise chartgen.ChartgenError("layout", "unported")
        return 0

    monkeypatch.setattr(cli, "render_one", fake)
    assert cli.main(["build", str(specs), "--src", str(src), "--reports", str(tmp_path / "r")]) == 1
    assert seen == ["a.svg", "b.svg"]
    assert not (src / "a.svg").exists()
    out = capsys.readouterr()
    assert "ERR  a" in out.out and "layout: unported" in out.out and "1 chart(s)" in out.err


def test_style_lint_finds_a_saturated_unbalanced_palette(tmp_path):
    # the original's test_style.Lint.test_saturated_unbalanced_palette_fails (03-04 #1)
    plain = __import__("pathlib").Path(chartgen.__file__).resolve().parent / "styles" / "plain.css"
    css = tmp_path / "loud.css"
    css.write_text(f'@import "{plain}";\n:root {{ --series-1: #00adef; --series-2: #ed008c; '
                   '--series-3: #fef200; --single: #00adef; }\n')
    report = tmp_path / "s.json"
    assert cli.main(["style", str(css), "--out", str(tmp_path / "flat.css"), "--report", str(report)]) == 0
    rules = {f["rule"] for f in json.loads(report.read_text())["findings"]}
    assert {"R3-2", "R3-6"} <= rules


def test_svg_lint_measures_its_texts_and_finds_what_the_original_finds(tmp_path):
    # the original's lint_svg on its golden p2-25-medal-ranks--line source (crates/chartgen-svg-lint
    # fixture): nine colours labelled directly (R3-22 warn), two close under deuteranopia (R3-19 info)
    golden = SPECS.parents[1] / "oracle" / "plain" / "p2-25-medal-ranks--line.svg"
    out = tmp_path / "lint.json"
    assert cli.main(["lint", str(golden), "--json", str(out)]) == 0
    found = json.loads(out.read_text())["files"][str(golden)]
    assert [(f["rule"], f["severity"]) for f in found] == [("R3-22", "warn"), ("R3-19", "info")]
