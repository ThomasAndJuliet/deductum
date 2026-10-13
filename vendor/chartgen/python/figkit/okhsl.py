"""OKHSL <-> sRGB (the original tools/okhsl.py), computed in Rust. Hue in degrees; saturation
and lightness in percent."""
from ._call import number, result


def _c(function, *args):
    return result("colour", function=function, args=list(args))


def okhsl(h, s, l):
    """Hex code of an OKHSL colour."""
    return _c("okhsl", h, s, l)


def okhsl_to_srgb(h, s, l):
    """h in degrees, s and l in percent; returns gamma-encoded sRGB in [0, 1]."""
    return tuple(number(v) for v in _c("okhsl_to_srgb", h, s, l))


def srgb_to_okhsl(r, g, b):
    """Gamma-encoded sRGB in [0, 1] to (h degrees, s percent, l percent)."""
    return tuple(number(v) for v in _c("srgb_to_okhsl", r, g, b))


def to_gamma(x):
    return number(_c("to_gamma", x))


def from_gamma(x):
    return number(_c("from_gamma", x))


def oklab_to_linear_srgb(L, a, b):
    return tuple(number(v) for v in _c("oklab_to_linear_srgb", L, a, b))


def linear_srgb_to_oklab(r, g, b):
    return tuple(number(v) for v in _c("linear_srgb_to_oklab", r, g, b))
