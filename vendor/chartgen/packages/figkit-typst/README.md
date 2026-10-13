# Typst peer

Build `figkit-typst` for `wasm32-unknown-unknown`, load it with `plugin`, and import
`figkit.typ`. `call` dispatches version 2 operations. `measure` negotiates requests
with the Rust session; `measure-batch` measures registered native content or plain
text in the current document. `render` consumes an already measured figure and
composes the ordered placement plan. Native keys must exist in the supplied
registry and are never evaluated as Typst source. Keep the registry and context
unchanged from measurement through composition. All lengths are physical points.
