"""Stylesheets with tokens: reading, flattening, metrics, and colour (figkit ``style``).

The semantics live in the Rust crate ``chartgen-style`` (flattening, tokens, ``var()``, ``em``,
colour functions, literal rules, ``metric``, ``label_style``); the colour maths in its interim
colour-space module. This module only reads files: the Rust loader asks, round by round, for
each ``@import`` or ``@defs`` target to be resolved against its sheet
(``(Path(sheet).parent / target).resolve()``, as the original) and for each import to be read.
"""
from pathlib import Path

from ._call import number, result
from ._call import call as _call
from ._call import unwrap as _unwrap


def _load(name, maker, root):
    sheets, links = {}, {}
    for _round in range(256):
        before = (len(sheets), sum(len(v) for v in links.values()))
        value = _call("style", name=name, maker=maker, root=root, sheets=sheets, links=links)
        if "need" not in value:
            return _unwrap(value)
        for need in value["need"]:
            if "read" in need:
                sheets[need["read"]] = Path(need["read"]).read_text(encoding="utf-8")
                continue
            target = str((Path(need["from"]).parent / need["resolve"]).resolve())
            links.setdefault(need["from"], {})[need["resolve"]] = target
            if need["link"] == "import" and target not in sheets:
                sheets[target] = Path(target).read_text(encoding="utf-8")

        if before == (len(sheets), sum(len(v) for v in links.values())):
            raise RuntimeError("style negotiation made no progress")
    raise RuntimeError("style negotiation round limit exceeded")


class Style:
    """A stylesheet, flattened; its tokens as literal values. A name that is not a .css path is
    looked up in style_dir; maker names the generator in the flattened file's first line."""

    maker = "figkit"

    def __init__(self, name_or_path, style_dir=None):
        p = Path(name_or_path)
        if p.suffix != ".css" and style_dir is None:
            raise ValueError(f"style {name_or_path!r}: a name needs a style_dir, or give the .css path")
        path = p if p.suffix == ".css" else Path(style_dir) / f"{name_or_path}.css"
        self.name = path.stem
        self.path = path.resolve()
        data = _load(self.name, self.maker, str(self.path))
        self.defs = None if data["defs"] is None else Path(data["defs"])
        self.tokens = {k: v for k, v in data["tokens"]}
        self.rules = [(sel, [(p, v) for p, v in decls]) for sel, decls in data["rules"]]
        self.css = data["css"]
        self._metrics = {k: (seen, outcome) for k, (seen, outcome) in data["metrics"].items()}
        self._theme = data["theme"]

    # ---- access for the layout ----
    def metric(self, name):
        """A metric token in points (or a plain fraction)."""
        v = self.tokens.get(name)
        if v is None:
            raise KeyError(f"style {self.name} has no token {name}")
        seen, outcome = self._metrics.get(name, (None, None))
        if seen != v:
            return number(result("metric", style=self.name, token=name, value=v))
        return number(_unwrap(outcome))

    def paint(self, name):
        return self.tokens.get(name)

    def has(self, name):
        return name in self.tokens

    def theme(self):
        """The engine's typed theme (chartgen-style ``Style::theme``) as a dict."""
        return _unwrap(self._theme)


# ---- Colour ------------------------------------------------------------------

def _colour(function, *args, **extra):
    return result("colour", function=function, args=list(args), **extra)


def hex_rgb(h):
    return tuple(number(c) for c in _colour("hex_rgb", h))


def rgb_hex(rgb):
    return _colour("rgb_hex", [float(c) for c in rgb])


def oklch(h):
    """(L, C, hue in degrees) of a hex colour."""
    return tuple(number(c) for c in _colour("oklch", h))


def from_oklch(L, C, hue):
    """A hex colour from OKLCH, chroma reduced until it is inside sRGB."""
    return _colour("from_oklch", L, C, hue)


def mix(a, b, t):
    return _colour("mix", a, b, t)


def oklch_shift(c, dL, dC):
    return _colour("oklch_shift", c, dL, dC)


def oklch_light(c, L, Cmax=None):
    """c at OKLCH lightness L, its hue kept and its chroma, at most Cmax, as far as sRGB holds it."""
    return _colour("oklch_light", c, L, Cmax)


def luminance(h):
    return number(_colour("luminance", h))


def contrast(a, b):
    return number(_colour("contrast", a, b))


def text_tone(fills, dark="#000000", light="#ffffff"):
    """("dark" or "light", its contrast): the text colour with more contrast on every one of these
    fills, judged by the lowest (WCAG contrast). A fill may be given alone."""
    fills = [fills] if isinstance(fills, str) else list(fills)
    tone, k = _colour("text_tone", dark, light, fills=fills)
    return tone, number(k)


# ---- Stylesheet text ---------------------------------------------------------

def strip_comments(css):
    return result("strip_comments", css=css)


def parse(css):
    """[(selector, [(property, value)])] of a comment-free stylesheet."""
    return [(sel, [(p, v) for p, v in decls]) for sel, decls in result("parse", css=css)]


def _num(x):
    """A number as CSS writes it: at most four decimals, no trailing zeros."""
    return result("num", x=x)


def label_style(css, classes, own=None):
    """bake.label_style: the (size, fill, weight, slant) a label with these classes is typeset with."""
    size, fill, weight, slant = result("label_style", css=css, classes=list(classes), own=own or None)
    return number(size), fill, weight, slant
