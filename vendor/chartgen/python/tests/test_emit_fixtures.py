"""Differential check of the compatibility renderer against the original figkit emit.svg.

tests/conformance/figkit/fixtures.json (generated from the original by
generate_fixtures.py) holds whole figkit figures and the source text emit.svg
wrote for them. Each figure is turned into a scene here and rendered through the
protocol; the output must equal the original's text, wedges (outlined by
chartgen-geometry in the renderer) included. A flat (bumps) path is
given as the cubic commands emit.py writes for it (h = (x1 - x0) / 2).
"""
import json
from pathlib import Path

import pytest

import chartgen

FIXTURES = json.loads((Path(__file__).resolve().parents[2] / "tests" / "conformance" / "figkit" /
                       "fixtures.json").read_text(encoding="utf-8"))
PAINT = {"fill": "#000000", "stroke": None, "stroke_width": 0.0, "opacity": 1.0}


def pt(p):
    return {"x": float(p[0]), "y": float(p[1])}


def meta(a, extra=None):
    return {"id": a.get("id"), "role": "", "classes": list(a.get("classes") or ()),
            "data": {k: str(v) for k, v in (a.get("data") or {}).items() if v is not None},
            "attrs": extra or {}, "joint": a.get("joint")}


def element(item):
    t, a = item["type"], item["args"]
    if t == "Group":
        return {"kind": "group", "children": [element(c) for c in item.get("children", [])],
                "translate": {"x": 0, "y": 0}, "clip": None,
                "meta": meta(a, {k: str(v) for k, v in (a.get("attrs") or {}).items()})}
    if t == "Rect":
        return {"kind": "rect", "bounds": {"x": a["x"], "y": a["y"], "width": a["w"], "height": a["h"]}, "paint": PAINT,
                "meta": meta(a, {"rx": str(a["rx"])} if a.get("rx") else None)}
    if t == "Line":
        return {"kind": "line", "from": pt((a["x1"], a["y1"])), "to": pt((a["x2"], a["y2"])), "paint": PAINT,
                "meta": meta(a)}
    if t == "Path" and a.get("curve") == "flat":
        (x0, y0), commands = a["points"][0], [{"op": "move_to", "to": pt(a["points"][0])}]
        for x1, y1 in a["points"][1:]:
            h = (x1 - x0) / 2
            commands.append({"op": "cubic_to", "c1": pt((x0 + h, y0)), "c2": pt((x1 - h, y1)), "to": pt((x1, y1))})
            x0, y0 = x1, y1
        return {"kind": "shape", "commands": commands, "paint": PAINT, "meta": meta(a)}
    if t == "Path":
        return {"kind": "path", "points": [pt(p) for p in a["points"]], "closed": False, "paint": PAINT, "meta": meta(a)}
    if t == "Wedge":
        return {"kind": "wedge", "center": pt((a["x"], a["y"])), "radius": float(a["r"]),
                "inner_radius": float(a.get("r0", 0.0)), "start_angle": float(a["a0"]), "end_angle": float(a["a1"]),
                "inset": float(a.get("inset", 0.0)), "paint": PAINT, "meta": meta(a)}
    if t == "Use":
        return {"kind": "use", "center": pt((a["x"], a["y"])), "size": a["size"], "href": a["href"], "paint": PAINT, "meta": meta(a)}
    if t == "Curve":
        return {"kind": "curve", "start": pt(a["start"]),
                "segments": [{"c1": pt(c1), "c2": pt(c2), "end": pt(e)} for c1, c2, e in a["segments"]],
                "head": [pt(p) for p in a["head"]] if a.get("head") else None,
                "paint": {**PAINT, "fill": None, "stroke": "#000000", "stroke_width": 1.0}, "meta": meta(a)}
    assert t == "Text", t
    classes = list(a.get("modifiers") or ()) + [a["role"]]
    request = {"id": "t", "content": {"kind": "native", "host": "typst", "key": a["code"], "alternative": None},
               "role": a["role"], "font_family": "New Computer Modern", "font_size": 10.0,
               "style": {"fill": None, "weight": "normal", "font_style": "normal"}, "max_width": None, "context": "c"}
    return {"kind": "text", "position": {"x": float(a["x"]), "y": float(a["y"]) - 7.0}, "request": request,
            "metrics": {"width": 20.0, "height": 9.0, "baseline": 7.0}, "anchor": a.get("anchor", "start"),
            "meta": {**meta(a), "classes": classes}}


def scene(fig):
    return {"version": 1, "width": fig["width"], "height": fig["height"], "title": fig.get("title"),
            "description": fig.get("desc"), "background": "#ffffff", "comment": fig.get("comment"),
            "defs": fig.get("defs") or [], "primitives": [element(fig["root"])]}


@pytest.mark.parametrize("case", range(len(FIXTURES["emit"])))
def test_renderer_writes_what_emit_wrote(case):
    fixture = FIXTURES["emit"][case]
    expected = fixture["expect"]
    out = chartgen.dispatch({"operation": "render_scene", "backend": "compatibility_svg",
                             "scene": scene(fixture["figure"])})["output"]
    assert out == expected
