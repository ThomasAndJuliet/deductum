# Rulings

Juliet's decided rulings (listed, with their status, in
`docs/chartgen/RULINGS.md`) encoded as tests, so that a change which breaks a
ruling fails CI, whoever makes it. Changing anything here needs Juliet's
approval: CI labels such pull requests `needs-juliet` (see `AGENTS.md`).

The crate `chartgen-rulings` is a workspace member. `src/lib.rs` is the
harness: it loads Juliet's example specs from `tests/conformance/examples/specs`
(variants merged as the original does) and runs them through the chooser or
renders them end to end in the plain style, with a stand-in text measurer.
`tests/` holds one file per topic; each test is named after its ruling
(`d18_…` for D-18, `o4_…` for O-4).

    cargo test -p chartgen-rulings
