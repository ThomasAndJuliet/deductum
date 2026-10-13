"""A laid-out figure: texts, marks and groups in points (figkit ``model``).

The dataclasses keep the original's fields and methods. Every method that computes geometry
(boxes, wedge shapes, paths and points, curve points, overlap) is evaluated by the Rust crate
``chartgen-geometry`` through the native protocol; ``Wedge.path(n)`` formats the Rust path
commands with the caller's number writer, as the original does.
"""
from dataclasses import dataclass, field

from ._call import number, path_data, result


def _geometry(**item):
    (value,) = result("geometry", items=[item])
    from ._call import unwrap
    return unwrap(value)


def _box(value):
    return tuple(number(v) for v in value)


def _pt(p):
    return [float(p[0]), float(p[1])]


@dataclass
class Text:
    """A label: Typst markup placed by its baseline (y) and anchor."""
    role: str
    code: str
    x: float
    y: float                        # the baseline
    anchor: str = "start"           # start, middle, end
    extent: object = None           # measure.Extent
    modifiers: tuple = ()
    id: str = None
    plain: str = None
    joint: str = None

    @property
    def classes(self):
        return " ".join(("prose",) + tuple(self.modifiers) + (self.role,))

    def box(self, pad=0.0):
        """(x0, y0, x1, y1) of the typeset line: advance width, ink height."""
        e = self.extent
        return _box(_geometry(op="text_box", x=self.x, y=self.y, anchor=self.anchor, pad=pad,
                              extent={"width": e.width, "ascent": e.ascent, "descent": e.descent}))


@dataclass
class Rect:
    x: float
    y: float
    w: float
    h: float
    classes: tuple
    data: dict = field(default_factory=dict)
    id: str = None
    rx: float = None

    def box(self, pad=0.0):
        return _box(_geometry(op="rect_box", x=self.x, y=self.y, w=self.w, h=self.h, pad=pad))


@dataclass
class Line:
    x1: float
    y1: float
    x2: float
    y2: float
    classes: tuple
    data: dict = field(default_factory=dict)
    id: str = None


@dataclass
class Path:
    """A polyline through points; with curve "flat", a bumps chart's curve (R6-40)."""
    points: list
    classes: tuple
    data: dict = field(default_factory=dict)
    id: str = None
    curve: str = None

    def box(self, pad=0.0):
        return _box(_geometry(op="points_box", points=[_pt(p) for p in self.points], pad=pad))

    def commands(self):
        """The path commands emit writes: a polyline, or the flat curve's cubic segments."""
        op = "flat_curve" if self.curve == "flat" else "polyline"
        return _geometry(op=op, points=[_pt(p) for p in self.points])


@dataclass
class Use:
    """A mark of the library, centred on (x, y) with the given diameter."""
    href: str
    x: float
    y: float
    size: float
    classes: tuple
    data: dict = field(default_factory=dict)
    id: str = None

    def box(self, pad=0.0):
        return _box(_geometry(op="use_box", x=self.x, y=self.y, size=self.size, pad=pad))


@dataclass
class Curve:
    """A stroke that is not data: a smooth shaft of cubic segments from start, and its head."""
    start: tuple
    segments: list
    classes: tuple
    head: list = None
    data: dict = field(default_factory=dict)
    id: str = None

    def _rust(self, pad=0.0):
        return _geometry(op="curve", start=_pt(self.start), pad=pad,
                         segments=[[_pt(p) for p in s] for s in self.segments],
                         head=None if self.head is None else [_pt(p) for p in self.head])

    @property
    def head_classes(self):
        return tuple(_geometry(op="head_classes", classes=list(self.classes)))

    def points(self):
        return [tuple(number(v) for v in p) for p in self._rust()["points"]]

    def box(self, pad=0.0):
        from ._call import unwrap
        return _box(unwrap(self._rust(pad)["box"]))


@dataclass
class Wedge:
    """A pie slice: the sector about (x, y) of radius r from a0 to a1 (degrees clockwise from
    12 o'clock); with r0 > 0 the sector of a ring; inset moves both straight edges inward."""
    x: float
    y: float
    r: float
    a0: float
    a1: float
    classes: tuple
    data: dict = field(default_factory=dict)
    id: str = None
    r0: float = 0.0
    inset: float = 0.0

    def _w(self):
        return {"x": self.x, "y": self.y, "r": self.r, "a0": self.a0, "a1": self.a1,
                "r0": self.r0, "inset": self.inset}

    def point(self, a, r=None):
        """The point at angle a (degrees clockwise from 12 o'clock), r from the centre."""
        return tuple(number(v) for v in _geometry(op="wedge_point", wedge=self._w(), a=a, r=r))

    def shape(self):
        """None, ("circle", r, r0), or ("sector", apex, (b0, b1), inner)."""
        sh = _geometry(op="wedge_shape", wedge=self._w())
        if sh is None:
            return None
        if sh[0] == "circle":
            return ("circle", number(sh[1]), number(sh[2]))
        inner = None if sh[3] is None else tuple(number(v) for v in sh[3])
        return ("sector", tuple(number(v) for v in sh[1]), tuple(number(v) for v in sh[2]), inner)

    def commands(self):
        """The outline as path commands (empty when nothing is drawn)."""
        return _geometry(op="wedge_path", wedge=self._w())

    def path(self, n):
        """The SVG path data, numbers written by n."""
        return path_data(self.commands(), n)

    def box(self, pad=0.0):
        """The bounding box of the drawn sector."""
        return _box(_geometry(op="wedge_box", wedge=self._w(), pad=pad))


@dataclass
class Group:
    classes: tuple = ()
    id: str = None
    children: list = field(default_factory=list)
    attrs: dict = field(default_factory=dict)

    def add(self, *items):
        self.children.extend(items)
        return items[0] if len(items) == 1 else items

    def walk(self):
        for c in self.children:
            yield c
            if isinstance(c, Group):
                yield from c.walk()


@dataclass
class Figure:
    width: float
    height: float
    root: Group
    title: str = None
    desc: str = None
    comment: str = None
    defs: list = field(default_factory=list)

    def texts(self):
        return [c for c in self.root.walk() if isinstance(c, Text)]

    def marks(self):
        return [c for c in self.root.walk() if isinstance(c, Rect) and "bar" in c.classes]


def overlap(a, b):
    """Whether two boxes (x0, y0, x1, y1) overlap with positive area."""
    return _geometry(op="overlap", a=[float(v) for v in a], b=[float(v) for v in b])
