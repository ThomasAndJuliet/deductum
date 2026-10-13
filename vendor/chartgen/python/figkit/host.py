"""Typeset sizes of labels, from Typst, exactly as the original's figkit measurer takes them.

The original (tools/figkit/measure.py with tools/bake.py) typesets every label
on its own page of a Typst document, ``width: auto, height: auto, margin: 0pt``,
in New Computer Modern, twice: once with ``top-edge: "bounds", bottom-edge:
"baseline"`` (the page height is the ink ascent) and once with both edges at
``"bounds"`` (the page is the ink box; its width is the line's advance). The
extent is read from each page's SVG ``viewBox`` and rounded to four decimals.
This module builds the same Typst source, page by page, and compiles it with the
``typst`` Python package instead of the Typst CLI, so that what is measured is
what bake.py sets.

A measured label maps onto the protocol's text metrics as
``width`` = advance, ``baseline`` = ink ascent and ``height`` = ink ascent plus
descent (the original's ``Extent.height``). A label that the host breaks within
its ``max_width`` (the original never measures broken labels) reports its widest
line and its whole ink height, with no single-line baseline.

``ApproximateMeasurer`` is an explicit preview choice only.
"""
import json
import re
from dataclasses import dataclass
from pathlib import Path

from .typst import escape

FONT = "New Computer Modern"


@dataclass(frozen=True)
class Extent:
    """A typeset line: advance width, ink above and below the baseline (points)."""
    width: float
    ascent: float
    descent: float

    @property
    def height(self):
        return self.ascent + self.descent


def label_style(css, classes, own=None):
    """(size, fill, weight, slant) a label with these classes is typeset with: the original's
    bake.label_style, implemented once in Rust (chartgen-style, through the protocol)."""
    from .style import label_style as rust_label_style
    return rust_label_style(css, classes, own)


# ---- typesetting ------------------------------------------------------------------------

def _typst():
    try:
        import typst
    except ImportError as error:
        raise RuntimeError("Install figkit[typst], or supply a native measurer") from error
    return typst


def typst_version():
    """The Typst engine the measurements come from (part of every cache key and context)."""
    typst = _typst()
    from importlib.metadata import PackageNotFoundError, version
    try:
        return f"typst-py {version('typst')}"
    except PackageNotFoundError:
        return f"typst-py {getattr(typst, '__version__', '?')}"


def _family(family):
    return "DejaVu Sans" if family == "sans-serif" else family


def _source(labels, bottom, wrap=None):
    """The figkit/bake document: one page per label (mode, code, size, weight, slant, family)."""
    styled = any(w or s for _, _, _, w, s, _ in labels)
    families = any(f != FONT for *_, f in labels)

    def text_rule(size, weight, style, family):
        extra = ""
        if styled:
            w = f'"{weight}"' if isinstance(weight, str) else weight or '"regular"'
            extra = f', weight: {w}, style: "{style or "normal"}"'
        if families:
            extra += f", font: {json.dumps(family, ensure_ascii=False)}"
        return f'#set text(size: {size}pt, fill: rgb("#000000"){extra})\n'

    pages = [
        "#set page(width: auto, height: auto, margin: 0pt, fill: none)\n"
        f'#set text(font: "{FONT}", top-edge: "bounds", bottom-edge: "{bottom}")\n'
    ]
    for i, (mode, code, size, weight, style, family) in enumerate(labels):
        if i:
            pages.append("#pagebreak()\n")
        pages.append(text_rule(size, weight, style, family))
        body = f"${code}$" if mode == "math" else code
        if wrap is not None and wrap[i] is not None:
            # The widest line of the label broken within its maximum width.
            body = (f"#context {{ let b = [{body}]; "
                    f"block(width: measure(b, width: {wrap[i]}pt).width, b) }}")
        pages.append(body + "\n")
    return "".join(pages)


def _view_boxes(source, labels):
    typst = _typst()
    try:
        pages = typst.compile(source.encode(), format="svg")
    except Exception as error:  # report the offending label, as bake.py does
        for label in labels:
            try:
                typst.compile(_source([label], "bounds").encode(), format="svg")
            except Exception as one:
                raise ValueError(f"Typst rejects the {label[0]} label {label[1]!r}: {one}") from error
        raise ValueError(f"Typst measurement failed: {error}") from error
    if isinstance(pages, (bytes, str)):
        pages = [pages]
    if len(pages) != len(labels):
        raise RuntimeError(f"Typst produced {len(pages)} pages for {len(labels)} labels")
    boxes = []
    for page in pages:
        text = page.decode() if isinstance(page, bytes) else page
        m = re.search(r'viewBox="([^"]+)"', text)
        if m is None:
            raise RuntimeError("Typst SVG page has no viewBox")
        _, _, w, h = (float(v) for v in m.group(1).replace(",", " ").split())
        boxes.append((w, h))
    return boxes


def typeset(labels, wrap=None):
    """[(width, ascent, descent)] of labels (mode, code, size, weight, slant[, family]), as figkit rounds them."""
    labels = [tuple(l) + (FONT,) * (6 - len(l)) for l in labels]
    ascents = [h for _, h in _view_boxes(_source(labels, "baseline", wrap), labels)]
    boxes = _view_boxes(_source(labels, "bounds", wrap), labels)
    return [(round(w, 4), round(a, 4), round(h - a, 4)) for a, (w, h) in zip(ascents, boxes)]


def _code(content):
    if content["kind"] == "plain":
        return escape(content["text"])
    if content["host"] == "typst":
        return content["key"]
    if content.get("alternative") is not None:
        return escape(content["alternative"])
    raise ValueError(f"Cannot measure {content['host']} content in Typst without a text alternative")


def _typography(request):
    style = request.get("style") or {}
    weight = style.get("weight", "normal")
    slant = style.get("font_style", "normal")
    if weight not in ("normal", "bold") or slant not in ("normal", "italic"):
        raise ValueError(f"Unsupported Typst text weight or font style: {weight!r}, {slant!r}")
    size = request["font_size"]
    if not isinstance(size, (int, float)) or isinstance(size, bool) or not size > 0:
        raise ValueError(f"Invalid font size {size!r}")
    return float(size), ("bold" if weight == "bold" else None), ("italic" if slant == "italic" else None)


def metrics(extent, baseline=True):
    """Protocol text metrics of a (width, ascent, descent) extent."""
    width, ascent, descent = extent
    return {"width": width, "height": round(ascent + descent, 4), "baseline": ascent if baseline else None}


class Measurer:
    """Measures labels as the original's figkit Measurer does, with the typst Python package.

    The original's interface is kept: ``measure([(classes, code[, mode])])``
    returns :class:`Extent` values for the typography ``label_style`` gives those
    classes under ``css``. ``measure_requests`` serves the Rust layout's text
    requests with their own size, weight and slant.
    """

    def __init__(self, css=None, cache_path=None):
        self.css = css
        self.cache_path = Path(cache_path) if cache_path else None
        self.version = typst_version()
        self.cache = {}
        if self.cache_path and self.cache_path.exists():
            try:
                self.cache = json.loads(self.cache_path.read_text())
            except ValueError:
                self.cache = {}
        self._dirty = False
        self._styles = {}
        self.runs = 0                   # Typst batches run (for the report)

    @property
    def context_signature(self):
        return f"chartgen.measure.Measurer[{self.version}; figkit bounds/baseline]"

    def style_of(self, classes):
        """(size, fill, weight, slant) of a label with these classes."""
        key = tuple(classes)
        if key not in self._styles:
            self._styles[key] = label_style(self.css or "", ["prose"] + [c for c in classes if c != "prose"])
        return self._styles[key]

    def _key(self, mode, code, size, weight, slant, family, wrap):
        key = [mode, code, size, weight, slant, self.version]
        if family != FONT:
            key.append(family)
        if wrap is not None:
            key.append({"max_width": wrap})
        return json.dumps(key, ensure_ascii=False)

    def _extents(self, labels):
        """Extents of (mode, code, size, weight, slant, family, max_width), one batch for the uncached."""
        keys = [self._key(*label) for label in labels]
        todo = {}
        for key, label in zip(keys, labels):
            if key not in self.cache and key not in todo:
                todo[key] = label
        if todo:
            plain = {k: l for k, l in todo.items() if l[6] is None}
            wrapped = {k: l for k, l in todo.items() if l[6] is not None}
            if plain:
                for key, extent in zip(plain, typeset([l[:6] for l in plain.values()])):
                    self.cache[key] = list(extent)
            if wrapped:
                # Unbroken first: within its width a label is exactly the single line the original measures.
                lines = typeset([l[:6] for l in wrapped.values()])
                broken = {}
                for (key, label), extent in zip(wrapped.items(), lines):
                    if extent[0] <= label[6]:
                        self.cache[key] = list(extent)
                    else:
                        broken[key] = label
                if broken:
                    for key, extent in zip(broken, typeset([l[:6] for l in broken.values()],
                                                           [l[6] for l in broken.values()])):
                        self.cache[key] = list(extent) + ["wrapped"]
            self.runs += 1
            self._dirty = True
            self.save()
        return [self.cache[k] for k in keys]

    def measure(self, items):
        """Extents of [(classes, code)] or [(classes, code, mode)], measured in one batch."""
        labels = []
        for item in items:
            classes, code = tuple(item[0]), item[1]
            mode = item[2] if len(item) > 2 else "prose"
            size, _, weight, slant = self.style_of(classes)
            labels.append((mode, code, size, weight, slant, FONT, None))
        return [Extent(*e[:3]) for e in self._extents(labels)]

    def one(self, classes, code):
        return self.measure([(classes, code)])[0]

    def measure_requests(self, requests):
        labels = []
        for request in requests:
            size, weight, slant = _typography(request)
            width = request.get("max_width")
            labels.append(("prose", _code(request["content"]), size, weight, slant,
                           _family(request["font_family"]), width))
        out = {}
        for request, extent in zip(requests, self._extents(labels)):
            out[request["id"]] = metrics(extent[:3], baseline=len(extent) == 3)
        return out

    def save(self):
        if self.cache_path and self._dirty:
            self.cache_path.parent.mkdir(parents=True, exist_ok=True)
            self.cache_path.write_text(json.dumps(self.cache, ensure_ascii=False, indent=0, sort_keys=True))
            self._dirty = False


class ApproximateMeasurer:
    """Opt-in preview measurement; never a document typography guarantee."""
    def measure_requests(self, requests):
        return {r["id"]: {"width": len(r["content"].get("text", r["content"].get("alternative") or "")) * r["font_size"] * 0.6, "height": r["font_size"] * 1.2, "baseline": None} for r in requests}
