"""Familiar Python API backed by the layered Rust engine."""
import json
import copy
from pathlib import Path
from dataclasses import dataclass, field
from decimal import Decimal
from . import _native
from .rules import Report
from .style import Style
from .measure import Measurer
from .typst import escape

__all__ = ["render", "lint_scene", "Context", "ChartgenError", "ProtocolError"]


class ChartgenError(RuntimeError):
    """An error the Rust engine reported: ``kind`` (its layer), ``message`` and the spec ``path``."""

    def __init__(self, kind, message, path=None):
        super().__init__(f"{kind}: {message}")
        self.kind, self.message, self.path = kind, message, path


class ProtocolError(ChartgenError):
    """A malformed request or response at the host/engine boundary."""

@dataclass
class Context:
    spec: object
    style: object
    measurer: object
    report: object
    choice: object = None
    scale: object = None
    labels: object = None
    batch: object = None
    geometry: dict = field(default_factory=dict)
    hooks: dict = field(default_factory=dict)

def _json(value):
    """Serialize Decimal as exact JSON numbers, avoiding binary float conversion."""
    if isinstance(value, Decimal):
        if not value.is_finite():
            raise ValueError("Non-finite decimal")
        return str(value)
    if isinstance(value, dict):
        return "{" + ",".join(json.dumps(str(k)) + ":" + _json(v) for k, v in value.items()) + "}"
    if isinstance(value, (list, tuple)):
        return "[" + ",".join(_json(v) for v in value) + "]"
    return json.dumps(value, ensure_ascii=False, allow_nan=False)

_OUTCOME_FIELDS = {"render": "result", "output": "output", "data": "value", "error": "error"}


def dispatch(operation):
    response = json.loads(_native.dispatch(_json({"version": 1, **operation})))
    if not isinstance(response, dict) or response.get("version") != 1 \
            or _OUTCOME_FIELDS.get(response.get("outcome")) not in response:
        raise ProtocolError("protocol", f"malformed engine response: {str(response)[:200]}")
    if response["outcome"] == "error":
        error = response["error"]
        cls = ProtocolError if error.get("kind") == "protocol" else ChartgenError
        raise cls(error.get("kind", "?"), error.get("message", ""), error.get("path"))
    return response

def lint_scene(scene):
    """Run Rust's placed-scene checks on a serialized scene object."""
    return dispatch({"operation": "lint_scene", "scene": scene})["value"]

def _resolve_registry(spec, base_dir):
    colours=spec.get("colours", {})
    if not isinstance(colours, dict) or not colours.get("registry"):
        return spec
    path=Path(colours["registry"])
    if not path.is_absolute():
        path=Path(base_dir or ".") / path
    try:
        registry=json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise ValueError(f"the colour registry {colours['registry']} cannot be read ({error})") from error
    if not isinstance(registry, dict) or any(not isinstance(slot, int) or isinstance(slot, bool) or not 1 <= slot <= 16 for slot in registry.values()):
        raise ValueError("the colour registry must be an object of names and palette slots (1–16)")
    resolved=copy.deepcopy(spec)
    resolved["colours"]["slots"]={**registry, **(colours.get("slots") or {})}
    resolved["colours"].pop("registry",None)
    return resolved

# The original's label classes (style/base.css, style/message.css) by text request role. A legacy
# measurer derives size, weight and slant from a label's classes (bake.label_style), so the
# classes passed must give exactly the typography the request asks for.
_ROLE_CLASSES = {role: (role,) for role in (
    "title", "panel-title", "text-strong", "comment", "missing-note", "incomplete-note", "chart-note",
    "footnote", "note-mark", "event-label", "arrow-key", "inset-text")}


def _legacy_style(measurer, classes):
    if hasattr(measurer, "style_of"):
        return measurer.style_of(classes)
    if getattr(measurer, "css", None) is not None:
        from .measure import label_style
        return label_style(measurer.css, ["prose"] + [c for c in classes if c != "prose"])
    raise ValueError("A legacy measurer must expose style_of(classes) or css, so that the requested "
                     "text size, weight and slant can be preserved")


def _legacy_classes(measurer, request):
    """Classes under which a legacy measurer sets exactly the request's size, weight and slant."""
    import re
    from .measure import _typography
    size, weight, slant = _typography(request)
    wanted = (size, weight, slant)
    candidates = [_ROLE_CLASSES.get(request["role"], ()), ()]
    css = getattr(measurer, "css", None) or ""
    candidates += [(name,) for name in dict.fromkeys(re.findall(r"\.([A-Za-z][\w-]*)", css))]
    for classes in candidates:
        found, _, w, s = _legacy_style(measurer, ("prose",) + tuple(classes))
        if (float(found), w, s) == wanted:
            return ("prose",) + tuple(classes)
    found = _legacy_style(measurer, ("prose",) + tuple(candidates[0]))
    raise ValueError(f"The legacy measurer's stylesheet has no label class giving the {request['role']!r} text "
                     f"its requested typography (size {size}, weight {weight or 'regular'}, slant "
                     f"{slant or 'upright'}); its {'/'.join(candidates[0]) or 'prose'} labels are set at "
                     f"size {found[0]}, weight {found[2] or 'regular'}, slant {found[3] or 'upright'}")


def _measure(measurer, requests):
    if hasattr(measurer, "measure_requests"):
        return measurer.measure_requests(requests)
    if not hasattr(measurer, "measure"):
        raise TypeError("A measurer needs measure_requests(requests) or the original measure(items)")
    from .measure import metrics
    # Adapt the original batched measurer interface: [(classes, code)], code as Typst prose markup.
    items = []
    for request in requests:
        content = request["content"]
        if content["kind"] == "plain":
            code = escape(content["text"])
        elif content["host"] == "typst":
            code = content["key"]
        elif content.get("alternative") is not None:
            code = escape(content["alternative"])
        else:
            raise ValueError("Legacy measurers need a text alternative for foreign native content")
        if request.get("font_family") not in (None, "New Computer Modern"):
            raise ValueError(f"A legacy measurer sets New Computer Modern, not {request['font_family']!r}")
        items.append((_legacy_classes(measurer, request), code))
    extents = measurer.measure(items)
    if len(extents) != len(requests):
        raise ValueError("Measurer returned an incorrect number of results")
    return {r["id"]: metrics((e.width, e.ascent, e.descent)) for r, e in zip(requests, extents)}

# The original's figure width when the spec gives none (spec.py LayoutOpts.width: the paper's text width).
DEFAULT_WIDTH = 368


def render(spec_json, style, variant=None, expect=None, measurer=None,
           source_name=None, base_dir=None, *, backend="compatibility_svg",
           width=None, height=None, latex_preamble="", native=None):
    """Render using Rust, servicing measured text batches in Python.

    Original return shape is retained. Unsupported Rust features raise an
    explicit error; they are never delegated to an unreported Python renderer.
    """
    if isinstance(spec_json, dict):
        try:
            spec_json=_resolve_registry(spec_json,base_dir)
        except ValueError as error:
            expected=expect or {}
            found={"CG-4"}
            missing=[rule for rule in (*expected.get("refused",[]),*expected.get("findings",[])) if rule not in found]
            unexpected=[] if "CG-4" in expected.get("refused",[]) else ["CG-4"]
            return None,Report(chart=spec_json.get("id","?"),style=getattr(style,"name",style if isinstance(style,str) else "plain"),variant=variant,status="invalid",expect=expected,expect_met=not missing and not unexpected,source=spec_json.get("source",{}),summary={"expect_missing":missing,"unexpected_errors":unexpected},findings=[{"rule":"CG-4","severity":"error","message":str(error),"path":"colours.registry","title":"the spec is well formed"}])
    if isinstance(style, (str, bytes)):
        style = Style(style)
    if hasattr(style, "to_theme"):
        theme = style.to_theme()
    elif hasattr(style, "tokens"):
        theme = {"name": style.name, "font_family": "New Computer Modern", "font_size": style.metric("--label-size"), "text": style.paint("--text"), "grid": style.paint("--grid"), "background": style.paint("--page"), "palette": [style.paint(f"--series-{i}") or "" for i in range(1, 17)], "tokens": dict(style.tokens)}
    else:
        theme = dict(style)
    if measurer is None and backend in ("latex", "pgf"):
        from .latex import TeXMeasurer
        measurer = TeXMeasurer(latex_preamble, native)
    measurer = measurer or Measurer(getattr(style, "css", None))
    context = _json({"theme": theme, "host": getattr(measurer, "context_signature", type(measurer).__name__)})
    layout = spec_json.get("layout", {}) if isinstance(spec_json, dict) else {}
    request = {"spec": spec_json, "theme": theme, "backend": backend, "variant": variant, "expect": expect or {}, "source_name": None if source_name is None else str(source_name), "constraints": {"width": width if width is not None else layout.get("width", DEFAULT_WIDTH), "height": height if height is not None else layout.get("height"), "measurement_context": context, "max_rounds": 8}, "measurements": {"context": context, "entries": {}}}
    for _ in range(request["constraints"]["max_rounds"]):
        result = dispatch({"operation": "render", "request": request})["result"]
        if result["status"] == "need_measurements":
            entries = _measure(measurer, result["requests"])
            if not entries or any(r["id"] not in entries for r in result["requests"]):
                raise ValueError("Measurer did not supply all requested metrics")
            request["measurements"]["entries"].update(entries)
        elif result["status"] in ("complete", "stopped"):
            return result.get("output"), Report.from_json(result["report"])
        else:
            raise ProtocolError("protocol", f"Unexpected render status {result['status']!r}")
    raise RuntimeError("Layout did not converge within the measurement round limit")
