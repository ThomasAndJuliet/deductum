"""chartgen.figkit and chartgen.style against the ORIGINAL figkit/okhsl/bake.

Expected values come from the original implementation: tests/conformance/figkit/fixtures.json
(the independent review's generate_fixtures.py) and the crates' fixtures
(crates/chartgen-style/tests/fixtures/style.json, crates/chartgen-geometry/tests/fixtures/
geometry.json, from their generate.py). Every computing call must cross the native protocol.
"""
import json
import math
from pathlib import Path

import pytest

import figkit._native as native
from chartgen import figkit  # noqa: F401
from chartgen.figkit import emit, measure, model, okhsl, style, typst
from chartgen.style import STYLE_DIR
from chartgen.style import Style as ChartStyle

ROOT = Path(__file__).resolve().parents[2]
FX = json.loads((ROOT / "tests/conformance/figkit/fixtures.json").read_text(encoding="utf-8"))
STYLE_FX = json.loads((ROOT / "crates/chartgen-style/tests/fixtures/style.json").read_text(encoding="utf-8"))
GEO_FX = json.loads((ROOT / "crates/chartgen-geometry/tests/fixtures/geometry.json").read_text(encoding="utf-8"))


def plain(v):
    return json.loads(json.dumps(v, default=lambda o: list(o) if isinstance(o, tuple) else str(o)))


def close(a, b, tol=1e-9):
    if isinstance(a, (list, tuple)) and isinstance(b, (list, tuple)):
        return len(a) == len(b) and all(close(x, y, tol) for x, y in zip(a, b))
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(close(a[k], b[k], tol) for k in a)
    if isinstance(a, bool) or isinstance(b, bool) or a is None or b is None:
        return a == b
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return math.isclose(a, b, rel_tol=tol, abs_tol=tol)
    return a == b


@pytest.fixture
def calls(monkeypatch):
    count = [0]
    dispatch = native.dispatch

    def counted(*a):
        count[0] += 1
        return dispatch(*a)
    monkeypatch.setattr(native, "dispatch", counted)
    return count


def test_escape_matches_figkit_typst(calls):
    for c in FX["escape"]:
        assert typst.escape(*c["args"]) == c["expect"], c["args"]
    assert typst.markup(None) is None
    assert calls[0] == len(FX["escape"])


def test_colour_functions_and_okhsl(calls):
    for name, cases in FX["colour"].items():
        fn = getattr(okhsl if name in ("okhsl", "srgb_to_okhsl", "okhsl_to_srgb") else style, name)
        for c in cases:
            assert close(plain(fn(*c["args"])), c["expect"]), (name, c["args"])
    assert calls[0] > 500


def _write(root, files):
    for rel, text in files.items():
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        (root / rel).write_text(text, encoding="utf-8")


def _snapshot(root, entry):
    try:
        s = style.Style(root / entry)
    except Exception as error:
        return {"error": {"type": type(error).__name__, "message": str(error).replace(str(root.resolve()), "<root>")}}
    metric = {}
    for k in s.tokens:
        try:
            metric[k] = s.metric(k)
        except ValueError as error:
            metric[k] = {"error": str(error)}
    return {"expect": {"name": s.name, "tokens": list(s.tokens.items()), "css": s.css, "rules": plain(s.rules),
                       "defs": None if s.defs is None else str(Path(s.defs).relative_to(root.resolve())),
                       "metric": metric}}


def test_stylesheets_of_the_independent_fixtures(tmp_path, calls):
    for i, c in enumerate(FX["style"]):
        root = tmp_path / f"s{i}"
        _write(root, c["files"])
        got = _snapshot(root, c["entry"])
        if "error" in c:
            assert got == {"error": c["error"]}, c["case"]
        else:
            got["expect"]["tokens"] = dict(got["expect"]["tokens"])
            assert close(got, {"expect": c["expect"]}), c["case"]
    assert calls[0] >= len(FX["style"])


def test_stylesheet_edge_cases(tmp_path):
    for c in STYLE_FX["styles"]:
        root = tmp_path / c["case"]
        _write(root, c["files"])
        want = {"error": c["error"]} if "error" in c else {"expect": c["expect"]}
        assert close(_snapshot(root, c["entry"]), want), c["case"]


def test_missing_import_is_the_hosts_file_error(tmp_path):
    (tmp_path / "a.css").write_text('@import "gone.css";\n', encoding="utf-8")
    with pytest.raises(FileNotFoundError):
        style.Style(tmp_path / "a.css")
    with pytest.raises(ValueError, match="a name needs a style_dir"):
        style.Style("plain")


def test_label_style(calls):
    css = {}
    for c in FX["label_style"]:
        if c["style"] not in css:
            css[c["style"]] = style.Style(STYLE_DIR / f"{c['style']}.css").css
        assert close(plain(style.label_style(css[c["style"]], c["classes"])), c["expect"]), c
    for c in STYLE_FX["label_style"]:
        try:
            got = {"value": plain(style.label_style(STYLE_FX["label_css"], c["classes"], c["own"]))}
        except Exception as e:
            got = {"error": {"type": type(e).__name__, "message": str(e)}}
        want = {k: v for k, v in c.items() if k in ("value", "error")}
        assert close(got, want), c


def test_chartgen_style_keeps_the_original_additions():
    s = ChartStyle("plain")
    assert s.name == "plain" and s.path == (STYLE_DIR / "plain.css").resolve()
    assert s.css.startswith("/* Flattened by chartgen from the plain stylesheet: literal values only */\n")
    assert s.defs == (STYLE_DIR / "plain-defs.svg").resolve()
    assert s.metric("--label-size") == 10.0 and s.has("--text") and s.paint("--nope") is None
    with pytest.raises(KeyError):
        s.metric("--nope")
    theme = s.to_theme()
    assert theme["font_size"] == 10.0 and len(theme["palette"]) == 16 and theme["text"] == s.paint("--text")


def _build(d):
    cls = getattr(model, d["type"])
    args = dict(d["args"])
    for k in ("classes", "modifiers"):
        if k in args:
            args[k] = tuple(args[k])
    el = cls(**args)
    for child in d.get("children", []):
        el.children.append(_build(child))
    return el


def test_geometry(calls):
    g = FX["geometry"]
    for c in g["text_box"]:
        t = _build({"type": "Text", "args": {**c["text"], "extent": measure.Extent(**c["extent"])}})
        assert close(plain(t.box(c["pad"])), c["expect"]) and t.classes == c["classes"]
    for c in g["box"]:
        assert close(plain(_build({"type": c["kind"], "args": c["args"]}).box(c["pad"])), c["expect"])
    for c in g["curve"]:
        cv = _build({"type": "Curve", "args": c["args"]})
        assert close(plain(cv.points()), c["points"]) and list(cv.head_classes) == c["head_classes"]
        assert close(plain(cv.box(1.0)), c["box"])
    for c in g["wedge"]:
        w = _build({"type": "Wedge", "args": c["args"]})
        assert close(plain(w.shape()), c["shape"]) and w.path(emit._n) == c["path"]
        assert close(plain(w.box(0.5)), c["box"])
        assert close(plain([w.point(c["args"]["a0"]), w.point(c["args"]["a1"], 10.0)]), c["point"])
    for c in g["overlap"]:
        assert model.overlap(*c["args"]) == c["expect"]
    for c in GEO_FX["paths"]:
        if not c["points"]:
            continue
        p = model.Path([tuple(q) for q in c["points"]], ("line",), curve=c["curve"])
        from chartgen.figkit._call import path_data
        assert path_data(p.commands(), emit._n) == c["d"]
    assert calls[0] > 1000


@pytest.mark.parametrize("case", range(len(FX["emit"])))
def test_emit_matches_the_original_figure(case):
    f = FX["emit"][case]["figure"]
    fig = model.Figure(f["width"], f["height"], _build(f["root"]), title=f.get("title"), desc=f.get("desc"),
                       comment=f.get("comment"), defs=f.get("defs") or [])
    assert emit.svg(fig) == FX["emit"][case]["expect"]


def test_measure_matches_figkit_extents():
    m = measure.Measurer(ChartStyle(STYLE_DIR / "plain.css").css)
    cases = FX["measure"]["cases"]
    got = m.measure([(tuple(c["classes"]), c["code"]) for c in cases])
    for c, e in zip(cases, got):
        assert close([e.width, e.ascent, e.descent], c["expect"], 0.01)


def test_measure_uses_label_style_typography():
    seen = []

    class Host:
        def measure_requests(self, requests):
            seen.extend(requests)
            return {r["id"]: {"width": 5.0, "height": 9.0, "baseline": 7.0} for r in requests}
    css = ChartStyle("plain").css
    m = measure.Measurer(css, host=Host())
    e = m.one(("title",), "Hi")
    assert (e.width, e.ascent, e.descent, e.height) == (5.0, 7.0, 2.0, 9.0)
    size, fill, weight, slant = style.label_style(css, ["prose", "title"])
    assert seen[0]["font_size"] == size and seen[0]["style"] == {"fill": fill, "weight": weight or "normal",
                                                                  "font_style": slant or "normal"}
