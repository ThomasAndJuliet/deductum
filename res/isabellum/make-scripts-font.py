"""Isabellum Scripts: raised and lowered small glyphs for Isabelle's \\<^sup> and \\<^sub>.

jEdit draws the character after a sub- or superscript control symbol smaller and
shifted; iced's text can't, so these glyphs carry the shift and scale themselves.
In the private use area: superscript ASCII c at U+E000 + c, subscript at U+E100 + c;
superscript Greek c at U+E200 + (c - U+0380), subscript at U+E300 + (c - U+0380). Glyphs are taken from a monospace base font, scaled
to 70% and centred in its advance, so columns stay aligned.

    python make-scripts-font.py /usr/share/fonts/google-noto/NotoSansMono-Regular.ttf out.ttf
"""
import sys
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.recordingPen import DecomposingRecordingPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

base_path, out_path = sys.argv[1], sys.argv[2]
base = TTFont(base_path)
gs = base.getGlyphSet()
cmap = base.getBestCmap()
upm = base["head"].unitsPerEm
adv = base["hmtx"][cmap[ord("a")]][0]
ascent = base["hhea"].ascent
descent = base["hhea"].descent
chars = list(range(0x21, 0x7F)) + [c for c in range(0x391, 0x3CA) if c in cmap]
scale = 0.7
def code(kind, c):
    if c < 0x100:
        return (0xE000 if kind == "sup" else 0xE100) + c
    return (0xE200 if kind == "sup" else 0xE300) + (c - 0x380)


shifts = {"sup": 0.42 * upm, "sub": -0.14 * upm}

empty = TTGlyphPen(None).glyph()
glyphs = {".notdef": empty, "space": empty}
metrics = {".notdef": (adv, 0), "space": (adv, 0)}
mapping = {}
for c in chars:
    name = cmap.get(c)
    if not name:
        continue
    rec = DecomposingRecordingPen(gs)
    gs[name].draw(rec)
    width = gs[name].width
    for kind, dy in shifts.items():
        pen = TTGlyphPen(None)
        dx = (adv - width * scale) / 2
        rec.replay(TransformPen(pen, (scale, 0, 0, scale, dx, dy)))
        glyph = pen.glyph()
        gname = f"{kind}{c:04X}"
        glyphs[gname] = glyph
        glyph.recalcBounds(None)
        metrics[gname] = (adv, getattr(glyph, "xMin", 0) or 0)
        mapping[code(kind, c)] = gname

fb = FontBuilder(upm, isTTF=True)
fb.setupGlyphOrder(list(glyphs))
fb.setupCharacterMap(mapping)
fb.setupGlyf(glyphs)
fb.setupHorizontalMetrics(metrics)
fb.setupHorizontalHeader(ascent=ascent, descent=descent)
# fontdb skips faces without a PostScript name
fb.setupNameTable({"familyName": "Isabellum Scripts", "styleName": "Regular",
                   "fullName": "Isabellum Scripts Regular", "psName": "IsabellumScripts-Regular",
                   "uniqueFontIdentifier": "IsabellumScripts-Regular-1.0", "version": "Version 1.0",
                   "copyright": "Derived from Noto Sans Mono, SIL Open Font License 1.1"})
fb.setupOS2(sTypoAscender=ascent, sTypoDescender=descent, usWinAscent=ascent, usWinDescent=-descent)
fb.setupPost(isFixedPitch=1)
fb.save(out_path)
print(f"{len(mapping)} glyphs -> {out_path}")
