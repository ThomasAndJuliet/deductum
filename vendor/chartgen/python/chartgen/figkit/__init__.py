"""figkit: what every generator of house-format SVG figures shares, backed by the Rust crates.

The original figkit's public modules, with their call signatures:

- model: the laid-out figure (texts, rects, lines, paths, marks, curves, wedges, groups); its
  geometry (boxes, wedge shapes and paths, curve points, overlap) is computed by
  ``chartgen-geometry``;
- emit: the figure as a scene, rendered by the rewrite's compatibility SVG renderer;
- measure: Typst sizes of labels through the rewrite's host measurer, typography from
  ``chartgen-style``'s ``label_style``;
- typst: plain text escaped into Typst markup by ``chartgen-render``;
- style: stylesheets flattened by ``chartgen-style`` (files are read here, in Python); colour,
  contrast and text_tone;
- okhsl: the OKHSL conversions.

Nothing here re-implements the original's computations in Python: every call that computes
crosses the native protocol (``operation: "figkit"``). Features the rewrite's scene and renderer
cannot yet express raise ``NotImplementedError`` naming them.
"""
