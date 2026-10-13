# Chartgen architecture

## Requirements

One Rust engine supports native Python and Rust, browser WebAssembly, Typst WebAssembly and LaTeX build integration. The original Python chart catalogue, semantic rules, diagnostics, style workflows and CLI are the compatibility target. Native document text is measured and composed by its host.

## Layers

`chartgen-model` defines chart contracts and preserves the version-1 scene DTOs. `chartgen-data` parses and transforms data. `chartgen-policy` owns chart selection, semantic refusals and authorized alternatives. `chartgen-grammar` compiles explicit semantic plans. `chartgen-scales` owns decimal scale training and mappings. `chartgen-layout` owns guide planning and physical arrangement using supplied native text metrics. `chartgen-scene` supplies chart metadata and delegates generic validation. `chartgen-render` adapts chart scenes to shared renderers without policy or layout. The root `chartgen` package orchestrates these APIs.

Hosts and bindings depend inward. Computational crates never import Python, browsers, document engines, filesystems or processes. Policy does not depend on layout; renderers do not depend on policy or layout. Guides and layout may negotiate within a bounded host measurement protocol. Layout must never substitute a different chart family or silently waive a semantic rule.

## Standalone figures

The `figkit` Rust umbrella exposes independently layered model, geometry, colour, style, scene and render crates. These crates have no chartgen dependency. Chartgen retains chart data, scales, palette allocation, bundled chart styles, semantic targets and rule interpretation. Its low-level compatibility crates delegate to the shared owners.

The shared placement service accepts measured alternatives, allowed regions, preferred positions, explicit obstacles and concrete target groups. It returns selected alternatives, translations, connectors and unresolved reasons within configured allocation and work budgets. Both chart annotators use its occupancy, ranking and routing services through distinct legacy profiles. Chartgen still chooses semantic targets, comment wrapping, chart remedies and fallback layout. The library has no algorithm-diagram product layer.

A portable figure carries already placed geometry and measured content. Preparation checks structure, resources and configured limits, assigns local element references, and returns a `PreparedScene` with private internals. A renderer validates its backend capabilities before encoding the scene. Unsupported native content or resources fail explicitly. Generic bounds observations carry element references; chartgen translates them into chart findings.

Scene version 2 carries resolved typography and paint, the complete measured request and context, typed rectangle radii, clips and supplied symbol geometry. Scene version 1 remains a separate compatibility representation for raw SVG attributes, definitions and baseline placeholders. Chart datum associations remain in chartgen. Converting an old placeholder into portable text requires a new measurement.

The independent Python `figkit` distribution uses its own native extension. `chartgen.figkit` submodules alias the same modules and classes. Browser and Typst WASM bindings and the LaTeX helper are peer integrations over the shared protocol; they do not require the Python facade. Hosts retain native content under opaque registry keys and own measurement, caching and final composition. The version-2 request/reply session checks full request identity, context, progress and round limits; it invokes no document engine.

See [standalone packaging](packaging/figkit/README.md), [figure authoring guidance](docs/figkit-guidance.md), and the peer [browser](packages/figkit-web/README.md), [Typst](packages/figkit-typst/README.md) and [LaTeX](packages/figkit-latex/README.md) integrations.

## Representations

The original JSON is retained losslessly in `Spec`; typed prepared data, decisions, semantic plans, trained scales, placed charts and scenes define the layer boundaries. Decimal values remain decimal until geometry mapping. Physical coordinates use a top-left origin, downward Y and 1/72-inch units. Document backends convert to TeX bp, Typst pt and SVG units explicitly.

Text requests have stable IDs and a host measurement context. Native TeX/Typst content remains in the host and is referenced by keys. The core cannot reinterpret arbitrary markup as portable text. Native text metrics must drive layout; no backend performs relayout. Unsupported host content or primitives produce explicit errors.

## Hosts

Python has a native PyO3 extension and convenience package preserving the existing `render` signature and report shape. Browser WASM and Typst plugin WASM use distinct interfaces. Typst measures and builds content in its wrapper; plugins exchange byte buffers only. LaTeX uses PGF geometry and native TeX boxes through an external build helper. Compatibility source SVG is separate from portable web SVG.

## Verification and migration

Tests target independent policy, scale, measurement/layout and backend boundaries. Existing tests and consuming scripts define parity; chart-family registration alone is not a parity claim. Unsupported features must be reported rather than silently omitted. Implementation status and executable validation belong in README and the parity inventory. No Python delegation may be described as a Rust port.
