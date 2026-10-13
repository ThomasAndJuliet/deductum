# Diagrams

Diagram tools for papers, presentations and recreational things, shared
between Juliet and Tom. chartgen (with figkit) is the first: its README
follows. Read [AGENTS.md](AGENTS.md) before changing anything: every change
goes through a pull request, and changes to the rulings or the architecture
need Juliet's approval.

- `crates/` — stable crates; `experimental/` — usable, shape still settling
- `docs/` — guidance; `docs/chartgen/` the rules and rulings for charts
- `tests/rulings/` — the rulings as tests

## Chartgen Rust rewrite

A layered Rust chart engine with native Python bindings, browser WebAssembly,
Typst plugin WebAssembly, and LaTeX/PGF output. This repository is a rewrite in
progress, **not a feature-complete replacement** for the original Python library.

## Layer boundaries

The `chartgen` crate (`crates/chartgen`) orchestrates independently usable crates for data,
decision policy, grammar, decimal scales, formatting, measured layout, scenes and
rendering. Policy has no font or rendering dependency. Layout accepts native text
measurements and implements a replaceable `LayoutEngine`. Renderers consume
completed scenes and cannot select chart types or perform chart layout.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the agreed contracts.

The shared figure stack is available independently through the Rust `figkit`
umbrella and the Python `figkit` distribution. It owns geometry, colour mathematics,
stylesheet evaluation, checked scenes and rendering. Chartgen owns chart semantics,
layout policy and reports. Historical `chartgen.figkit` submodules alias the
standalone Python modules. See [standalone packaging](packaging/figkit/README.md)
and [figure authoring guidance](docs/figkit-guidance.md).
The shared placement service provides bounded label and connector search; chartgen
supplies chart targets and chooses fallback layout.

## Build and use

```sh
cargo test --workspace
python3 -m venv .venv
.venv/bin/pip install maturin pytest typst
source .venv/bin/activate
python packaging/figkit/stage.py ../figkit-source
(cd ../figkit-source && python -m maturin build)
python -m pip install ../figkit-source/target/wheels/figkit-*.whl
.venv/bin/maturin develop
.venv/bin/python examples/python/render.py
```

The familiar interface is available:

```python
from chartgen import render
from chartgen.style import Style

svg, report = render(spec, Style("plain"))
web_svg, report = render(spec, Style("plain"), backend="svg")
pgf, report = render(spec, Style("plain"), backend="pgf")
```

`render` retains the original positional parameters and `(output_or_none,
Report)` result. The default backend emits compatibility source SVG with Typst
text placeholders for the existing bake workflow; `svg` emits ordinary SVG.
Both historical output equivalence and full diagnostic parity remain unverified.
Python loads stylesheet files; Rust evaluates imports, tokens and typography.
Chart decisions and geometry run in Rust. There is no hidden Python rendering
fallback. `Decimal` values cross the native binding as exact JSON numbers.

Python measures text using Typst by default. Pass an existing batched measurer or
a provider exposing `measure_requests(requests)` to use another engine.
`ApproximateMeasurer` is available only as an explicit preview choice.

For PGF output, Python uses actual TeX measurement. Supply the same
`latex_preamble` used in the document. Explicit `{ "latex": "key" }` labels
refer to content in the `native` dictionary; declare matching saved boxes in the
document with `\chartgenDeclareNative`. No shell escape is required.

## WebAssembly and documents

```sh
cargo build -p chartgen-web -p chartgen-typst --target wasm32-unknown-unknown
```

The browser build uses wasm-bindgen; generate its JavaScript glue with the
matching wasm-bindgen CLI. The host helper is `packages/web/chartgen.mjs`.
The Typst build uses Typst's byte-buffer ABI, independently of browser glue.
Compile `examples/typst/smoke.typ` from the repository root after building.
Its document package measures and places native text around SVG geometry layers.
See [Typst integration](packages/typst/README.md) and
[LaTeX integration](packages/latex/README.md).

The versioned JSON protocol provides `render`, `render_scene`, `choose` and
`schema`. Rendering returns measurement requests until layout completes. Hosts
own fonts, resources, measurements and output files; the computational crates
have no filesystem/process/DOM dependency.

## Compatibility status

All 21 original chart-family names have layout implementations, but this is not
a parity claim. Family implementations are bounded subsets of the original
algorithms. The bundled schema comes from the original source; validation is
implemented in Rust. Unsupported semantic features produce explicit failures.

Remaining migration work includes exact chooser/report equivalence, annotation
and highlight behavior, footnotes, insets, small multiples and merged measures,
comparison marks, time-axis presentation, colour registries, full style and SVG
lint, and geometry/style fidelity across the original examples. Irregular time
support is being ported separately. Document native text support is implemented;
all host-specific clipping, wrapping and font combinations need conformance
coverage before claiming document output parity.

`python -m chartgen` exposes `render`, `build`, `choose`, `schema`, `style` and
`lint`. Render/choose output is not yet an exact legacy CLI replacement. `style`
evaluates and checks styles in Rust. SVG lint remains partial: `CG-SVG-PARTIAL`
and CLI exit code 2 identify an incomplete check.

Run the original example audit with the installed binding:

```sh
.venv/bin/python tools/audit_examples.py ../charts/examples/specs \
  --out /tmp/chartgen-example-outcomes.json
```

The audit records statuses and embedded expectations. It does not establish
report or geometry equivalence. The original share is source material and is
not required to build this repository.
