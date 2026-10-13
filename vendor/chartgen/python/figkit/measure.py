"""Typeset sizes of labels (figkit ``measure``), through the rewrite's host measurer.

``Measurer(css, cache_path)`` keeps the original's interface: ``measure([(classes, code[,
mode])])`` returns :class:`Extent` values (advance width, ink ascent and descent) for the
typography that ``label_style`` (Rust ``chartgen-style``) gives the classes under ``css``;
``one(classes, code)`` measures one label. Typesetting is the host's: the labels are passed to
``chartgen.measure.Measurer.measure_requests`` as measured-text requests. A host measurer that
reports no baseline (no ink ascent) cannot give an extent, and the call raises
``NotImplementedError`` rather than guessing one.
"""
import hashlib
import json
from dataclasses import dataclass

from . import host as _host
from .style import label_style


@dataclass(frozen=True)
class Extent:
    """A typeset line: advance width, ink above and below the baseline (points)."""
    width: float
    ascent: float
    descent: float

    @property
    def height(self):
        return self.ascent + self.descent


class Measurer:
    """Measures labels in the classes they will carry, under one flattened style."""

    def __init__(self, css, cache_path=None, host=None):
        """css: the flattened stylesheet; cache_path: the host measurer's cache file (None: none);
        host: a measurer with measure_requests (default: chartgen.measure.Measurer)."""
        self.css = css
        self.cache_path = cache_path
        self.host = host if host is not None else _host.Measurer(css, cache_path)
        self._styles = {}

    @property
    def runs(self):
        return getattr(self.host, "runs", 0)

    def style_of(self, classes):
        """(size, fill, weight, slant) of a label with these classes (bake.label_style, memoised)."""
        key = tuple(classes)
        if key not in self._styles:
            self._styles[key] = label_style(self.css, ["prose"] + [c for c in classes if c != "prose"])
        return self._styles[key]

    def _request(self, classes, code, mode):
        size, fill, weight, slant = self.style_of(classes)
        key = f"${code}$" if mode == "math" else code
        style = {"fill": fill, "weight": "normal" if weight is None else str(weight),
                 "font_style": slant or "normal"}
        ident = hashlib.sha256(json.dumps([mode, code, size, style], ensure_ascii=False).encode()).hexdigest()[:24]
        return {"id": ident, "content": {"kind": "native", "host": "typst", "key": key, "alternative": None},
                "role": classes[-1] if classes else "prose", "font_family": "New Computer Modern",
                "font_size": size, "style": style, "max_width": None, "context": "figkit"}

    def measure(self, items):
        """Extents of [(classes, code)] or [(classes, code, mode)], measured in one batch."""
        requests = [self._request(tuple(i[0]), i[1], i[2] if len(i) > 2 else "prose") for i in items]
        unique = list({r["id"]: r for r in requests}.values())
        got = self.host.measure_requests(unique) if unique else {}
        out = []
        for r in requests:
            m = got[r["id"]]
            if m.get("baseline") is None:
                raise NotImplementedError(
                    f"{type(self.host).__name__}.measure_requests reports no baseline, so the ink ascent "
                    "and descent of a figkit Extent are unknown")
            out.append(Extent(m["width"], m["baseline"], m["height"] - m["baseline"]))
        return out

    def one(self, classes, code):
        return self.measure([(classes, code)])[0]

    def save(self):
        if hasattr(self.host, "save"):
            self.host.save()
