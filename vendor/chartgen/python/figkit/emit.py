"""The SVG writer (figkit ``emit``): the figure converted to the rewrite's scene and rendered by
the Rust compatibility SVG renderer (``render_scene``, backend ``compatibility_svg``).

No SVG is written here. The scene carries everything a figkit figure holds: the GENERATED
comment and chart defs, group attrs in order, rounded rects (``rx``), library marks (``Use``),
arrows with their heads (``Curve``), and pie slices and flat bumps curves as path commands from
chartgen-geometry (``Wedge.commands``, ``Path.commands``). A wedge with nothing to draw is left
out, as emit.py leaves it out.
"""
import json

from . import _native
from .model import Curve, Group, Line, Path, Rect, Text, Use, Wedge


def _n(x):
    """A coordinate as emit writes it: rounded to 0.01, no trailing zeros, never "-0"."""
    s = f"{x:.2f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


class Unsupported(NotImplementedError):
    pass


def _classes(classes):
    return classes.split() if isinstance(classes, str) else [str(c) for c in classes]


def _meta(el, classes, role="", attrs=None):
    data = {str(k): str(v) for k, v in (getattr(el, "data", None) or {}).items() if v is not None}
    return {"id": el.id, "role": role, "classes": classes, "data": data, "attrs": attrs or {}, "joint": None}


_PAINT = {"fill": None, "stroke": None, "stroke_width": 0.0, "opacity": 1.0}
_INK = {"fill": "#000000", "stroke": None, "stroke_width": 0.0, "opacity": 1.0}
_STROKE = {"fill": None, "stroke": "#000000", "stroke_width": 1.0, "opacity": 1.0}


def _pt(p):
    return {"x": float(p[0]), "y": float(p[1])}


def _scene_commands(commands):
    """chartgen-geometry path commands as scene path commands."""
    out = []
    for c in commands:
        op = c["op"]
        if op in ("move_to", "line_to"):
            out.append({"op": op, "to": _pt((c["x"], c["y"]))})
        elif op == "cubic_to":
            out.append({"op": op, "c1": _pt((c["x1"], c["y1"])), "c2": _pt((c["x2"], c["y2"])),
                        "to": _pt((c["x"], c["y"]))})
        elif op == "arc_to":
            out.append({"op": op, "rx": c["rx"], "ry": c["ry"], "rotation": c["rotation"],
                        "large_arc": bool(c["large_arc"]), "sweep": bool(c["sweep"]), "to": _pt((c["x"], c["y"]))})
        elif op == "close":
            out.append({"op": "close"})
        else:
            raise ValueError(f"unknown path command {op!r}")
    return out


def _primitive(el):
    if isinstance(el, Group):
        children = [p for p in (_primitive(c) for c in el.children) if p is not None]
        return {"kind": "group", "children": children, "translate": {"x": 0.0, "y": 0.0}, "clip": None,
                "meta": _meta(el, _classes(el.classes), attrs={str(k): str(v) for k, v in el.attrs.items()})}
    if isinstance(el, Rect):
        return {"kind": "rect", "bounds": {"x": el.x, "y": el.y, "width": el.w, "height": el.h},
                "paint": _INK, "meta": _meta(el, _classes(el.classes), attrs={"rx": repr(float(el.rx))} if el.rx else None)}
    if isinstance(el, Line):
        return {"kind": "line", "from": {"x": el.x1, "y": el.y1}, "to": {"x": el.x2, "y": el.y2},
                "paint": _STROKE, "meta": _meta(el, _classes(el.classes))}
    if isinstance(el, Path):
        if el.curve == "flat":
            return {"kind": "shape", "commands": _scene_commands(el.commands()), "paint": _STROKE,
                    "meta": _meta(el, _classes(el.classes))}
        return {"kind": "path", "points": [{"x": x, "y": y} for x, y in el.points], "closed": False,
                "paint": _STROKE, "meta": _meta(el, _classes(el.classes))}
    if isinstance(el, Use):
        return {"kind": "use", "center": _pt((el.x, el.y)), "size": float(el.size), "href": el.href,
                "paint": _INK, "meta": _meta(el, _classes(el.classes))}
    if isinstance(el, Curve):
        return {"kind": "curve", "start": _pt(el.start),
                "segments": [{"c1": _pt(c1), "c2": _pt(c2), "end": _pt(e)} for c1, c2, e in el.segments],
                "head": [_pt(p) for p in el.head] if el.head else None, "paint": _STROKE,
                "meta": _meta(el, _classes(el.classes))}
    if isinstance(el, Wedge):
        commands = el.commands()
        if not commands:
            return None                                  # nothing drawn: emit.py writes no element
        return {"kind": "shape", "commands": _scene_commands(commands), "paint": _INK,
                "meta": _meta(el, _classes(el.classes))}
    if isinstance(el, Text):
        e = el.extent
        # A figure built without measurement has no extent: emit places a label by its baseline only.
        width, ascent, descent = (e.width, e.ascent, e.descent) if e is not None else (0.0, 0.0, 1.0)
        classes = el.classes.split()
        request = {"id": el.id or el.code, "content": {"kind": "native", "host": "typst", "key": el.code,
                                                      "alternative": el.plain},
                   "role": el.role, "font_family": "New Computer Modern", "font_size": 10.0,
                   "max_width": None, "context": "figkit"}
        meta = _meta(el, classes)
        meta["joint"] = el.joint
        return {"kind": "text", "position": {"x": el.x, "y": el.y - ascent}, "request": request,
                "metrics": {"width": width, "height": ascent + descent, "baseline": ascent},
                "anchor": el.anchor, "meta": meta}
    raise TypeError(el)


def scene(fig):
    """The figure as the rewrite's scene (version 1)."""
    return {"version": 1, "width": fig.width, "height": fig.height, "title": fig.title,
            "description": fig.desc, "background": "none", "comment": fig.comment,
            "defs": [str(d) for d in fig.defs or []], "primitives": [_primitive(fig.root)]}


def svg(fig):
    """The SVG source text of a figure, from the Rust compatibility renderer."""
    request = {"version": 1, "operation": "render_scene", "scene": scene(fig), "backend": "compatibility_svg"}
    response = json.loads(_native.dispatch(json.dumps(request, ensure_ascii=False, allow_nan=False)))
    if response["outcome"] == "error":
        error = response["error"]
        raise RuntimeError(f"{error['kind']}: {error['message']}")
    return response["output"]
