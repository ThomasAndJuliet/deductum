# Oracle conformance evidence

Portable inputs and outputs for comparing the rewrite with the original Python
implementation. Nothing here depends on the share or a particular machine.

| Path | Contents |
| --- | --- |
| `examples/specs/`, `examples/registry.json` | The original 191 source example files (bases, variants, embedded expectations), byte-identical copies of `charts/examples/` |
| `oracle/plain/` | The original implementation's report (`<key>.json`) and source SVG (`<key>.svg`) for each of the 414 base and variant runs under style `plain`. `<key>` is the spec stem, or `<stem>--<variant>` (so `--bw` files are variants, not copies). No run under `plain` produces report copies |
| `baseline.json` | Per-run verdicts of the rewrite against `oracle/plain`, maintained by `tools/conformance/ratchet.py` |

## Oracle provenance

- Source: share `Chart generator/charts/` and `Chart generator/tools/`, snapshot of 2026-10-02.
  The SHA-256 manifest of the 296 source files is `oracle-source.sha256`
  (manifest hash e4765987b58d8389615baca8fc12f0eaa64a49cc990c65e6e0e95ce95d7b4dca).
- Python 3.14.6; numpy 2.5.3, shapely 2.1.2, pikepdf 10.16.0 (the original's third-party imports; it declares no requirements file).
- Measurement: Typst CLI `typst 0.15.1 (9dfd3a08)`, the version recorded in the original's measurement cache.
- Original test suite in this environment: 633 tests, 257 failures, 2 errors, 3 skipped. 254 failures are `test_rules.Levels`/`test_svglint` assertions about guide levels, which the original reads from `charts/GUIDE.md`; that file is absent from the share, so every golden finding/decision/path entry has an empty `guide` list and rule levels fall back to `rules.py`'s defaults where GUIDE.md would supply them. The ratchet therefore locks in the GUIDE-less levels; that is the oracle as it can be run from the share, not necessarily the original's intended levels. The 2 errors need the absent `charts/eval/` and a CLI grouping that depends on those levels.
- All 414 oracle runs meet their embedded expectations (352 drawn, 60 refused, 1 invalid, 1 unsupported).

Regenerate with `tools/conformance/run_reports.py SPECS --oracle-root ORIGINAL/charts --out DIR` in an environment with the dependencies above.

## What the comparison checks

`tools/conformance/compare.py` judges each run. Report projection: status, chosen type, refusal
rule set, finding (rule, severity, level) multiset, finding (rule, severity, path) multiset,
decision rules/levels in order, overrides, chooser path rules/levels in order, `expect_met`, the
summary object with list order kept, and copy suffixes. Output: elements keyed by class and
`data-*` identity with matched geometry within 0.02 units (non-numeric values compared exactly),
the multiset of visible text, and every strict check of `compare_strict.py` (figure size,
`<title>`, all attributes of matched elements including path data, `href` and `text-anchor`,
paint and `<defs>`, text with class/position/anchor, z-order of classed groups, copy contents).
A run is `match` only if all of these agree. Finding prose is deliberately not compared.
`tests/conformance/tooling/blindspots.py` demonstrates the checks on 13 mutations of golden runs.

`tools/conformance/ratchet.py` fails when a run's verdict worsens, a check that agreed starts to
disagree, or a baseline run is missing.
