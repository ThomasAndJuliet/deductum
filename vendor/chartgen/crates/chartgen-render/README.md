# Renderers

`Renderer::render(&Scene) -> Result<String>` consumes the ordered, placed scene
without layout or policy. `SvgRenderer` creates standalone browser SVG with
native SVG text. `CompatibilitySvgRenderer` writes the original's house source
format for `bake.py` exactly as figkit's `emit.svg` does: the `<svg>` root with
`viewBox`, `width` and `height`, the scene's `comment` (`--` written as `–`),
`<title>`, `<desc>`, empty `style#house` and `defs#house-defs`,
`defs#chart-defs` with the scene's `defs` (then any group clip paths), the
group tree with classes (the role appended as the last class), ids, ordered
attributes (figkit `Group.attrs`, `Rect.rx`) and ordered `data-*` values
(quoted as `quoteattr` does), `use` symbol marks, curves with their `-head`
paths, shape path data, coordinates rounded to 0.01, `text-anchor` omitted for
`start`, and `<text class="prose …">` Typst placeholders placed by baseline.
As in the original, the source carries no paint or typography: the house
stylesheet styles it by class when baked, so layouts must request measurement
with the typography the classes give. A scene whose only top-level primitive is
a group renders that group as the root; others are wrapped in
`<g class="chart" id="chart">`. Wedges must be lowered to path commands
(chartgen-geometry) first; the renderer refuses them. Evidence:
`python/tests/test_compat_svg.py` (all 352 bundled oracle sources reproduced
byte for byte from scenes) and `python/tests/test_emit_fixtures.py` (the
original emit.svg on figkit fixture figures, wedges excluded). Both SVG forms
reject multiline plain text until a measured line layout is available.

`typst::escape` is the single implementation of the original's
`typst.escape`; hosts reach it through the protocol's `escape` operation.

`PgfRenderer` returns a standalone `pgfpicture`; use the LaTeX adapter in
`packages/latex`. It accepts six-digit hex and common named colors. Plain
labels are escaped, set at the requested font size, and conditionally wrapped
in a top-aligned TeX parbox when their natural width exceeds `max_width`,
matching the Python TeX measurer. Native LaTeX labels use saved box keys; a
box wider than its measured bound raises a LaTeX error. Unsupported colors
return render errors.

`TypstRenderer` returns a JSON render plan. It emits geometry as SVG layers and
text as native host layers in scene order, with group clipping on both.
Foreign native text uses its explicit plain-text alternative; lacking one
returns a render error. Use
`packages/typst/chartgen.typ` to measure and compose the plan. The plan is
not a Typst source string: this keeps arbitrary host markup out of Rust and
avoids converting native document text to SVG.
