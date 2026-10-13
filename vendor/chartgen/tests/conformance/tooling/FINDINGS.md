# Conformance tooling findings (independent review, 2026-10-02)

Findings of the independent reviewer about `tools/conformance/` at `0fee67a`, and their resolution in
`32a7469`. `blindspots.py` reproduces the mutation cases; `BLINDSPOTS.md` is its current output.

| Finding | Resolution |
| --- | --- |
| Path data `d`, `points`, `transform`, `use href` not compared (a line moved 20 units passed) | Strict attribute check is part of the verdict |
| Inline fill/stroke/opacity/`style` and `<defs>` ignored | Strict paint check (all elements, including defs) is part of the verdict |
| Root `width`/`height`/`viewBox` not compared | Strict figure check |
| Z-order not compared | Strict order check of classed groups |
| Text compared as a multiset (swapped labels, anchors, `<title>` passed) | Strict text-place and title checks |
| Unparseable numbers (`12pt`) skipped | Compared exactly as strings |
| Tolerance 0.5 too loose for two-decimal output | Default 0.02 |
| Copies compared by name only | Copy contents compared with the strict checks |
| Summary lists sorted before comparison | Order kept |
| Finding paths compared as a set | (rule, severity, path) multiset |
| Ratchet blind within `report-only`/`rewrite-error`; missing/new runs skipped | Every disagreeing check is tracked for all verdicts; missing baseline runs regress; new runs listed |
| `run_reports.py --oracle-root` wrote the measurement cache into the original's tree | Cache kept beside the outputs, seeded from the original's cache |
| GUIDE.md absent: all golden `guide` lists are empty and levels are `rules.py` fallbacks; README said `None` | README corrected; the limitation stands — the oracle can only be run without GUIDE.md |
| Only style `plain` has golden evidence | Open: other styles (`message`, `palette-muted`) are not covered |
| Finding prose and `elements` lists are not compared | By design for prose; `elements` remains uncompared (open) |
