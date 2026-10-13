# Typst adapter

Import `chartgen.typ`. `chartgen-dispatch` sends JSON to a plugin export named
`dispatch` as bytes and decodes its byte response. Pass the resulting render
plan (or its `result` field if the protocol wraps it) to
`chartgen-compose(plan, native: (key: [Typst content]))`. Native content stays
in Typst; Rust only sees keys and supplied dimensions. Call
`chartgen-measure(requests, native: ...)` inside the chart's final document
context and send the returned dictionary as the `entries` of `Measurements`.
Use the same font context for measurement and composition.

A plan has `version`, `width`, `height`, and ordered `layers`. SVG layers are
geometry-only full-size transparent canvases. Text layers carry top-left box
position, measured dimensions, optional group clip, and plain or native content.
Each SVG and native layer is placed in order. Chartgen coordinates convert
explicitly to Typst pt. The adapter clips native content with a box for group
clips. It does not interpret native markup or try to reflow after rendering.

API basis: [Typst plugin](https://typst.app/docs/reference/foundations/plugin/),
[place](https://typst.app/docs/reference/layout/place/),
[box clip](https://typst.app/docs/reference/layout/box/), and
[measure](https://typst.app/docs/reference/layout/measure/).

`chart(spec, module, ...)` defaults chart width to `spec.layout.width` when
provided, then to the original's 368 pt, and its height to `spec.layout.height`. An explicit `constraints` dictionary takes precedence.

`chart` also accepts the specification's raw JSON source as a string
(`chart(read("chart.json"), engine)`). Typst decodes JSON numbers as 64-bit
floats, so only the raw form carries long decimals to the engine unchanged; it
is spliced into the protocol request without being decoded.

Measurement follows the original's typesetting: text is set with its top and
bottom edges at the glyph bounds, so a label's box is its ink, its width the
advance, and `baseline` the ink ascent. A label wider than its `max_width` is
broken within it and reports its widest line without a baseline. Entries
accumulate across measurement rounds. Responses are validated: a malformed
response, an `error` outcome or a stopped render ends compilation with an
explicit `chartgen` message. The final plan is also emitted as
`metadata` labelled `<chartgen-plan>`, so `typst query DOC "<chartgen-plan>"`
can check placement.
