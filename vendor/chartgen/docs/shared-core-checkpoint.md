# Shared core implementation checkpoint

2026-10-03. This is a usable implementation checkpoint, with final validation
and conformance closure still pending. The reviewed architecture authority is
`/home/twright/chartgen-continue/design/shared-figure-core.md`, including I1–I5.

## Available now

Standalone Rust model, geometry, colour, style, scene preparation, rendering,
placement and protocol crates sit beneath chartgen. Chart interpretation and
annotation policy remain in chartgen. Version 1 compatibility uses explicit DTO
adapters; the portable protocol is version 2. Python `figkit` is independently
packaged, with `chartgen.figkit` aliases preserving identity. Browser, native
LaTeX and Typst peers use the shared protocol directly.

[Runnable document examples](../examples/figkit/README.md) produce native LaTeX
and Typst PDFs. Both examples were compiled from this checkpoint's source using
the CLI and WASM plugin; `pdftotext` recovered their labels and equations. The
LaTeX helper verifies final native box dimensions against measured metrics.
Keep measurement and composition contexts consistent when embedding them.

## Evidence

The independent implementer's [execution log](../tests/evidence/shared-figure-contract.txt)
records commands, revisions, failures, fixes and reruns. Passed slices include:

- 13 portable contract tests and seven placement tests.
- 67 retained Python tests, zero skips; changed final packaging paths separately checked.
- Standalone source staging, Rust consumer, compiler-negative APIs and installed wheel.
- Sequential figkit → chartgen → figkit builds in the same Cargo target directory.
- Genuine browser WASM, native TeX saved-box and whitespace PDFs, and Typst rich native composition.

An earlier integrated workspace run passed 216 Rust tests; it preceded the final
repair and test commits. It is not a frozen-checkpoint full workspace result.
Four initial independent review findings were repaired: transactional measurement
rounds, Typst named paint, connector profile selection, and portable text spaces.
The same-target native wheel artifact collision was also repaired and rerun.

## Resume here

1. Run the full workspace and retained host suites against one frozen revision.
2. Complete coverage for all configured counter boundaries, both chart annotators'
   candidate/tie/fallback/report traces, legacy conversion and multiline groups,
   the public Python symbol inventory, and the peer negative/capability matrix.
3. Finish the independent guidance/source audit and conformance review against
   the repaired implementation and concrete coverage. No final conformance claim
   has been made.

Detailed uncommitted workflow artifacts are retained under
`/home/twright/chartgen-continue/wt/shared-core/.architecture-work/`: architecture,
test plan, scenarios, coverage, API notes and integration history. The independent
review is in `wt/shared-review/.architecture-work/review.md`. Preserve the
immutable legacy oracle at `ff0a238`; do not regenerate expectations from new output.
Use `/home/twright/chartgen-continue/bin/limited` for expensive work, with
`LIMIT_MEM=3G LIMIT_CPU=400% CARGO_BUILD_JOBS=3`.

The original broader chart rewrite still has unresolved baseline parity findings;
this checkpoint does not close those. Release publication, deprecation and future
algorithm diagram products remain outside this implementation scope.
