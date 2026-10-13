"""Geometry fixtures from the ORIGINAL figkit model/emit (supplementing
tests/conformance/figkit/fixtures.json): wedges with rings, insets, slivers, whole circles and
large arcs; flat bumps curves and polylines as emit.py writes them; curves with heads.

    ORACLE_PY crates/chartgen-geometry/tests/fixtures/generate.py ORIGINAL/tools > geometry.json
"""
import json
import random
import sys

sys.path.insert(0, sys.argv[1])
from figkit import emit, model  # noqa: E402

rng = random.Random(20261002)


def shape(sh):
    return None if sh is None else json.loads(json.dumps(sh))


def capture(fn):
    try:
        return {"value": fn()}
    except Exception as e:  # recorded as the original raises it
        return {"error": type(e).__name__}


wedges = []
spans = [(0, 0), (0, 1e-10), (0, 0.5), (0, 3), (10, 50), (350, 370), (0, 179.9), (0, 180), (0, 180.1),
         (-30, 200), (45, 315), (90, 450), (0, 360), (0, 359.9999999995), (12.5, 372.5), (270, 271),
         (100, 99), (-90, -10)]
for a0, a1 in spans:
    for r0, inset in [(0, 0), (0, 0.75), (0, 3), (0, 100), (30, 0), (30, 0.75), (30, 4), (30, 35),
                      (2, 1.5), (0, -1), (79.9, 0.2)]:
        wedges.append((100, 100, 80, a0, a1, r0, inset))
for _ in range(80):
    a0 = rng.uniform(-400, 400)
    a1 = a0 + rng.choice([rng.uniform(0, 360), rng.uniform(0, 2), rng.uniform(170, 190), 360])
    wedges.append((rng.uniform(-50, 50), rng.uniform(-50, 50), rng.uniform(0, 120), a0, a1,
                   rng.choice([0, 0, rng.uniform(0, 60)]), rng.choice([0, rng.uniform(0, 5), rng.uniform(0, 50)])))
wedges.append((0, 0, 0, 0, 90, 0, 0))
wedge_out = []
for x, y, r, a0, a1, r0, inset in wedges:
    w = model.Wedge(x, y, r, a0, a1, ("slice",), r0=r0, inset=inset)
    wedge_out.append({"args": [x, y, r, a0, a1, r0, inset], "shape": capture(lambda: shape(w.shape())),
                      "path": capture(lambda: w.path(emit._n)), "box": capture(lambda: list(w.box(0.25))),
                      "points": [list(w.point(a)) for a in (a0, a1, 0, 90, 123.4)]})

paths = []
for n in [1, 2, 3, 5, 8]:
    pts = [(round(rng.uniform(0, 300), 3), round(rng.uniform(0, 200), 3)) for _ in range(n)]
    pts.sort()
    for curve in (None, "flat"):
        p = model.Path(pts, ("line",), curve=curve)
        line = emit._item(p, 0)[0]
        paths.append({"points": pts, "curve": curve, "d": line.split(' d="')[1].split('"')[0],
                      "box": list(p.box(1.0))})
paths.append({"points": [], "curve": None, "d": "", "box": None})

curves = []
for k in range(6):
    segs = [tuple((rng.uniform(0, 100), rng.uniform(0, 100)) for _ in range(3)) for _ in range(rng.randrange(1, 4))]
    head = [(rng.uniform(0, 100), rng.uniform(0, 100)) for _ in range(rng.choice([0, 3, 5]))] if k % 3 else None
    c = model.Curve((rng.uniform(0, 100), rng.uniform(0, 100)), segs, ("comment-arrow", "x") if k % 2 else ("connector",),
                    head=head)
    lines = emit._item(c, 0)
    curves.append({"start": list(c.start), "segments": [[list(p) for p in s] for s in segs], "head": head,
                   "classes": list(c.classes), "head_classes": list(c.head_classes), "points": [list(p) for p in c.points()],
                   "box": list(c.box(0.5)), "lines": lines})

numbers = [0, -0.0, 0.004, -0.004, 0.005, 0.015, 0.025, 1.005, 2.675, -1.234567, 1e-7, 123456.789, 0.1 + 0.2,
           12.345, 99.995, -99.995] + [rng.uniform(-1000, 1000) for _ in range(40)]
print(json.dumps({"wedges": wedge_out, "paths": paths, "curves": curves,
                  "numbers": [[x, emit._n(x)] for x in numbers]}, indent=0))
