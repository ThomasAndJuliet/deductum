# Native LaTeX and Typst figures

Run from the repository root. These examples measure native mathematical content,
place it through the standalone Rust core, and produce PDFs containing native text.
They require Rust, Node, `pdflatex` with PGF, and Typst with plugin support. Python
and chartgen assets are not required.

```sh
cargo build -p figkit-cli
node examples/figkit/native-latex.mjs
rustup target add wasm32-unknown-unknown
cargo build -p figkit-typst --target wasm32-unknown-unknown
typst compile --root . examples/figkit/native.typ figkit-typst.pdf
```

The outputs are `figkit-latex.pdf` and `figkit-typst.pdf`. To use another Cargo
target directory, pass the dispatcher and output paths as the two Node arguments.
Typst's plugin must be inside its project root; copy it there and pass
`--input wasm=/target/wasm32-unknown-unknown/debug/figkit_typst.wasm` to select
that path relative to the project root.

The LaTeX helper can also return PGF using `session.render(figure, 'pgf')` for
inclusion in an existing document. Declare native boxes with `figkit.sty` using
the same fonts, preamble, registry and measurement context. The Typst example's
`measure`/`render` block can be included in another document with its registry.
Change the context identity when changing the font environment or registry.

See the [LaTeX peer](../../packages/figkit-latex/README.md),
[Typst peer](../../packages/figkit-typst/README.md), and
[output guidance](../../docs/figkit-guidance.md) for supported capabilities.
