"""Compatibility source SVG reproduces the original's house format (tools/figkit/emit.py) exactly.

Every bundled oracle source SVG is read back into a scene (its comment, title,
desc, chart defs, group tree with classes, ids and attributes in order, marks
with their data-* identities, symbol uses, path data and label code), rendered
by the compatibility renderer through the protocol, and compared byte for byte
with the original's file.
"""
import json
import re
import xml.etree.ElementTree as ET
from pathlib import Path

import chartgen

ORACLE = Path(__file__).resolve().parents[2] / "tests" / "conformance" / "oracle" / "plain"
SVG = "{http://www.w3.org/2000/svg}"
PAINT = {"fill": "#000000", "stroke": None, "stroke_width": 0.0, "opacity": 1.0}
NUMBER = r"-?\d+(?:\.\d+)?"


def meta(el, attrs=()):
    return {"id": el.get("id"), "role": "", "classes": (el.get("class") or "").split(),
            "data": {k[5:]: v for k, v in el.attrib.items() if k.startswith("data-")},
            "attrs": {k: el.get(k) for k in attrs}, "joint": None}


def point(x, y):
    return {"x": float(x), "y": float(y)}


def commands(d):
    out = []
    for op, args in re.findall(r"([MLCAZ])([^MLCAZ]*)", d):
        nums = re.findall(NUMBER, args)
        if op in "ML":
            out.append({"op": "move_to" if op == "M" else "line_to", "to": point(*nums)})
        elif op == "C":
            out.append({"op": "cubic_to", "c1": point(*nums[0:2]), "c2": point(*nums[2:4]), "to": point(*nums[4:6])})
        elif op == "A":
            rx, ry, rot, large, sweep, x, y = nums
            out.append({"op": "arc_to", "rx": float(rx), "ry": float(ry), "rotation": float(rot),
                        "large_arc": large == "1", "sweep": sweep == "1", "to": point(x, y)})
        else:
            out.append({"op": "close"})
    return out


def primitive(el):
    tag, f = el.tag.replace(SVG, ""), lambda k: float(el.get(k))
    if tag == "g":
        # figkit Group.attrs keep their order, data-* among them
        rest = [k for k in el.attrib if k not in ("class", "id")]
        m = meta(el, rest)
        return {"kind": "group", "children": [primitive(c) for c in el], "translate": {"x": 0, "y": 0},
                "clip": None, "meta": m}
    if tag == "rect":
        return {"kind": "rect", "bounds": {"x": f("x"), "y": f("y"), "width": f("width"), "height": f("height")},
                "paint": PAINT, "meta": meta(el, ["rx"] if el.get("rx") else [])}
    if tag == "line":
        return {"kind": "line", "from": point(el.get("x1"), el.get("y1")), "to": point(el.get("x2"), el.get("y2")),
                "paint": PAINT, "meta": meta(el)}
    if tag == "path":
        return {"kind": "shape", "commands": commands(el.get("d")), "paint": PAINT, "meta": meta(el)}
    if tag == "use":
        size = f("width")
        return {"kind": "use", "center": point(f("x") + size / 2, f("y") + size / 2), "size": size,
                "href": el.get("href").lstrip("#"), "paint": PAINT, "meta": meta(el)}
    assert tag == "text", tag
    request = {"id": "t", "content": {"kind": "native", "host": "typst", "key": el.text or "", "alternative": None},
               "role": "label", "font_family": "New Computer Modern", "font_size": 10.0,
               "style": {"fill": None, "weight": "normal", "font_style": "normal"}, "max_width": None, "context": "c"}
    return {"kind": "text", "position": {"x": f("x"), "y": f("y") - 7.0}, "request": request,
            "metrics": {"width": 20.0, "height": 9.0, "baseline": 7.0}, "anchor": el.get("text-anchor", "start"),
            "meta": meta(el)}


def scene(source):
    root = ET.fromstring(source)
    _, _, w, h = (float(v) for v in root.get("viewBox").split())
    title, desc = root.find(SVG + "title"), root.find(SVG + "desc")
    comment = re.search(r"^  <!-- (.*) -->$", source, re.M)
    defs = re.search(r'^  <defs id="chart-defs">\n(.*?)\n  </defs>$', source, re.M | re.S)
    return {"version": 1, "width": w, "height": h, "title": None if title is None else title.text,
            "description": None if desc is None else desc.text, "background": "#ffffff",
            "comment": comment and comment.group(1),
            "defs": [l[4:] for l in defs.group(1).split("\n")] if defs else [],
            "primitives": [primitive(c) for c in root if c.tag == SVG + "g"]}


def test_compatibility_svg_reproduces_every_original_source_exactly():
    checked, unrepresentable = 0, []
    for path in sorted(ORACLE.glob("*.svg")):
        source = path.read_text(encoding="utf-8")
        try:
            data = scene(source)
        except AssertionError as error:
            unrepresentable.append((path.name, str(error)))
            continue
        out = chartgen.dispatch({"operation": "render_scene", "backend": "compatibility_svg", "scene": data})["output"]
        assert out == source, path.name
        checked += 1
    # Texts with tspans (none in the bundled sources) are the only element the reader leaves out.
    assert not unrepresentable, unrepresentable[:5]
    assert checked == 352


def test_render_writes_the_original_header_comment():
    spec = json.loads((Path(__file__).resolve().parents[2] / "examples" / "basic.json").read_text())
    from chartgen.measure import ApproximateMeasurer
    out, report = chartgen.render(spec, "plain", None, None, ApproximateMeasurer(), source_name="specs/basic.json")
    # charts/chartgen/__init__.py: f"{spec.id}: GENERATED by chartgen from {source_name} under style {style.name};
    # do not edit. type {choice.type} ({R9-10 and 02 D5 decisions})"
    comment = re.search(r"<!-- (.*) -->", out).group(1)
    assert comment.startswith("headcount: GENERATED by chartgen from specs/basic.json under style plain; "
                              "do not edit. type bar (")
