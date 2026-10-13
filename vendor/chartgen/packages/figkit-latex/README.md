# LaTeX peer

Build the native `figkit-dispatch` binary in `figkit-cli`. The Node helper
`TeXSession` invokes that executable and the selected TeX engine directly. It
retains native TeX content in its registry; measurements return complete request
identities and convert TeX scaled points to physical 1/72-inch points.

Load `figkit.sty`, declare each native box with `\figkitDeclareNative{key}{content}`
in the same typography/preamble context, and include the PGF string returned by
`session.render(figure, 'pgf')`. The renderer emits `\figkitUseNative{key}` in paint
order; the package reuses saved boxes. Keys are opaque alphanumeric identifiers
(with `_`, `-`, `:` allowed), never interpreted as source. Keep the registry and
context unchanged through composition. No Python facade or chartgen assets are
required. `max_width` uses a TeX paragraph box and therefore reports no baseline.

`session.compose(figure)` compiles the PGF with the registered native saved boxes,
checks their final dimensions against the supplied scene metrics (0.001 pt tolerance
for TeX arithmetic), and returns PDF bytes only if they match. The final document
uses the measured saved boxes directly. A changed font/preamble must use a new context.
