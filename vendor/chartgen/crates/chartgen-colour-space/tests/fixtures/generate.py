"""Differential fixtures for chartgen-colour-space, computed by the original figkit style.py and
okhsl.py (and svglint._cvd_distance). Run with the oracle environment:
    venv-oracle/bin/python generate.py ORACLE_ROOT > colour_space.json"""
import json, random, sys
from pathlib import Path
root = Path(sys.argv[1])
sys.path.insert(0, str(root / "tools")); sys.path.insert(0, str(root / "charts"))
import okhsl
from figkit import style as S
from chartgen.svglint import CVD, _cvd_distance

rng = random.Random(20261002)
hexes = ["#000000", "#ffffff", "#808080", "#fff", "#09c", "#595959", "#95b7d3", "#558bb9", "#c5392a",
         "#6eae44", "#f3f3f3", "#fef200", "#00adef", "#ed008c", "#7f7f7f", "#010101", "#fefefe"]
hexes += ["#%06x" % rng.randrange(0, 1 << 24) for _ in range(120)]
out = {"hex": [], "pairs": [], "oklch": [], "okhsl": [], "rgb_hex": []}
for h in hexes:
    out["hex"].append({"h": h, "rgb": list(S.hex_rgb(h)), "oklch": list(S.oklch(h)), "luminance": S.luminance(h),
                       "shift": [[dl, dc, S.oklch_shift(h, dl, dc)] for dl, dc in ((-0.12, 0), (0.1, -0.02), (0.3, 0.1))],
                       "light": [[L, c, S.oklch_light(h, L, c)] for L, c in ((0.9, 0.05), (0.5, None), (0.97, 0.02))],
                       "okhsl": list(okhsl.srgb_to_okhsl(*S.hex_rgb(h)))})
for _ in range(150):
    a, b = rng.choice(hexes), rng.choice(hexes)
    t = round(rng.random(), 3)
    out["pairs"].append({"a": a, "b": b, "t": t, "mix": S.mix(a, b, t), "contrast": S.contrast(a, b),
                         "tone": list(S.text_tone([a, b])),
                         "cvd": {k: _cvd_distance(a, b, m) for k, m in CVD.items()}})
for _ in range(150):
    L, C, h = round(rng.random(), 4), round(rng.random() * 0.4, 4), round(rng.random() * 360, 2)
    out["oklch"].append({"l": L, "c": C, "h": h, "hex": S.from_oklch(L, C, h)})
for _ in range(150):
    h, s, l = round(rng.random() * 360, 2), round(rng.random() * 100, 2), round(rng.random() * 100, 2)
    out["okhsl"].append({"h": h, "s": s, "l": l, "hex": okhsl.okhsl(h, s, l), "rgb": list(okhsl.okhsl_to_srgb(h, s, l))})
for v in (0.0, 1.0, 0.5 / 255, 1.5 / 255, 2.5 / 255, 127.5 / 255, 128.5 / 255, -0.2, 1.3, 0.3333):
    out["rgb_hex"].append({"v": v, "hex": S.rgb_hex([v, v, v])})
print(json.dumps(out))
