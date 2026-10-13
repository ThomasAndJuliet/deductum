# Independent conformance inventory of the Rust rewrite

Reviewer's inventory of what the rewrite does for each module and function family
of the original Python implementation, and for each option of the original spec.
It was built from the original sources and tests and from runs of both
implementations. The rewrite was examined as found on `stream/review` at
`0fee67a` (integration main when this review began). It was **not** derived from
the rewrite's own tests or documentation. It is not a parity claim. The rewrite
is unfinished, and the evidence below shows large gaps.

Statuses are conservative:

- **ported** requires behavioural evidence of equal results: matching oracle
  outputs, passing original tests that call the Rust engine, or agreeing probe
  results. A similarly named function is not evidence.
- **partial** means some of the family's behaviour is present and measurably
  agrees, and some is absent or differs.
- **missing** means the behaviour is absent, either silently or because the
  original Python is merely copied into the host package.
- **explicit-diagnostic** means the rewrite stops with a precise error or finding
  instead of doing what the original does.

Several families mix statuses; the table gives the dominant one first.

## Evidence and how to regenerate it

| Source | Command (from the repository root) | Committed result |
|---|---|---|
| Original unittest suite against the rewrite, with the oracle as control | `tests/conformance/original_suite/run_suite.py --oracle-root ORIG/charts --work W --impl rewrite --impl oracle --oracle-python ORACLE_PY`, then `classify.py --work W` | `tests/conformance/original_suite/results/` |
| Option-mutation probes (silent-disappearance audit) | `tools/conformance/probes.py audit --oracle-root ORIG/charts --oracle-python ORACLE_PY --work W` | `tests/conformance/silent/` |
| Ratchet over the 414 bundled runs | `tools/conformance/ratchet.py --work W` (verdicts at `0fee67a`: 1 match, 248 report-diff, 165 rewrite-error) | `tests/conformance/baseline.json` |
| Per-rule recall (oracle rule IDs that the rewrite also emits in the same run), over the 414 runs and the probes | `tests/conformance/inventory/rule_recall.py tests/conformance/oracle/plain W/rewrite W/probes` | figures quoted below |
| Strict comparison, and demonstrations of `compare.py`'s blind spots | `tools/conformance/compare_strict.py`, `tests/conformance/tooling/blindspots.py` | `tests/conformance/tooling/` |
| figkit, okhsl and bake differential fixtures (original → fixtures; replayed on the rewrite, with the original as control) | `tests/conformance/figkit/generate_fixtures.py --oracle-root ORIG/charts` (oracle Python), then `tests/conformance/figkit/check.py` | `tests/conformance/figkit/results.json`, `RESULTS.md` |
| Per-option status | `tests/conformance/inventory/fields.py` | the [spec option table](#spec-options) and `inventory/fields.json` |

Headline numbers:

- **Original tests (633).** 430 are applicable: 373 through the public surface,
  plus 57 that use the original's public lints or measurer API, which the rewrite
  package lacks. Another 192 exercise original Python internals, and 11 cannot
  pass in this environment (the oracle control fails or skips them too).
  - The oracle control passes all 430 applicable tests.
  - The rewrite passes 32. Only 7 of those 32 call the Rust engine; the other 25
    exercise only host Python copied from the original.
  - The 398 rewrite failures break down as:
    - 119: `render` raised `RuntimeError`;
    - 100: a class fixture raised;
    - 62: assertion failures;
    - 60: other exceptions;
    - 57: public API absent.
- **Ratchet.** Over the 249 runs that both implementations complete:
  - status agrees on 242;
  - chosen type on 203;
  - refusal set on 218;
  - decisions on 38;
  - chooser path on 1;
  - findings (rule, severity, level) on 73;
  - summary on 30.
- **SVG.** None of the 4274 classed oracle SVG identities (class plus `data-*`)
  appears in the rewrite's compatibility SVG: 0 matched over 199 runs.
- **Rule recall.** The original emits 296 distinct rule kinds (finding
  rule/severity, decision rule, path rule) across 1375 completed runs and probes.
  The rewrite never emits 218 of them in the same run and always emits 63.
- **Probes.** 1637 probes over 262 options:
  - 143 `SILENT` and 14 `OUTPUT-IGNORED`;
  - 41 refusals missed;
  - 178 explicitly diagnosed;
  - 554 where the rewrite reacted;
  - 253 blocked, where the rewrite fails on the unmodified base too;
  - 495 where the original does not react.

## Headline gaps (most important first)

1. **The compatibility SVG does not use the original's vocabulary.** The original
   writes `g.chart`, `rect.bar.single[data-series][data-x][data-value]`,
   `line.gridline[data-value]`, `text.prose.category-label` and so on. Its consumers
   are `bake.py`, the SVG lint and the original tests. The rewrite writes
   `rect.data-mark[data-role]`, `line.grid`, `text.prose.scale-label-formatted`,
   with inline fills and fonts (`crates/chartgen-render/src/lib.rs:177`
   `svg_primitive`). No original identity is matched, 1104 visible texts are
   missing, and every original test that parses output fails.
2. **Wrong default figure size, and layout options ignored.**
   - The Python host defaults the width to 480 pt
     (`python/chartgen/__init__.py:121`; `crates/chartgen-model/src/lib.rs:462`).
     The original defaults to 368 (`oracle charts/chartgen/spec.py:702`), so every
     example without `layout.width` is drawn 480 wide (all 199 compared figures
     differ in `compare_strict.py`'s `figure` check).
   - `layout.height` and `layout.plot_aspect` are silently ignored (probes:
     `ignored`).
3. **Refusals the rewrite misses.** There are 41 probes over 16 options where the
   original refuses or marks the run invalid or unsupported, and the rewrite
   draws. Among them:
   - `chart.gridlines: false` (R1-06);
   - `job.kind` answer/make_aware/persuade without an insight title (R0-10);
   - removing `title` (R2-12);
   - `data.measure.min` above the data (R2-28);
   - `table.bold` (R10-22);
   - removing `data.parts.overlaps` (R7-05);
   - `data.measure.additive: false` on bins (R2-43);
   - `scale.max` past the theoretical maximum (R2-28);
   - `table.graphics` dots on a heat table (CG-1);
   - removing a stub column's values (CG-4 invalid);
   - map and reference-series requests the original marks unsupported (CG-3);
   - `chart.type` requests the original refuses (6 probes).

   Separately, in 13 of the 60 oracle-refused bundled runs the rewrite raises
   instead of refusing.
4. **Silently ignored options.** 33 options are `ignored` and 23 `partly
   ignored`; see [spec options](#spec-options). They include:
   - `audience.locale` (matrix family), `audience.bw_risk`,
     `audience.abbreviations`;
   - `chart.encoding`, `chart.cells`;
   - `data.comparisons[].kind`, `data.x.names`, `data.x.order`, `data.x.title`,
     `data.parts.groups`, `data.parts.kind`;
   - `facts.focus`, `facts.total_matters`, `facts.exact_values_needed`,
     `facts.small_values_matter`;
   - `insight.important`, `reasons`;
   - `scale.breathing`, `scale.magnitude_style`, `scale.unit_position`,
     `scale.steps`, `scale.zero`;
   - `table.legend`, `table.names_title`, `table.stub[].title`;
   - `transform.unit`.
5. **`render` raises where the original returns a report.** The original returns
   `(svg or None, Report)` for every bundled example: 352 drawn, 60 refused, 1
   invalid, 1 unsupported. The rewrite raises `RuntimeError` in 165 of 414 runs,
   so no report, findings or expectation check is returned.
   - Some of these are explicit unported-feature diagnostics: insets, panels,
     highlight methods other than the strong shade, comments on pie, step and
     thin-bar charts, `labels.notes` statuses, events.
   - Others are layout-capacity failures in cases the original draws:
     - "category label N overlaps its neighbour";
     - "series legend does not fit the chart width";
     - "pie labels cannot be packed without overlap";
     - "scale title is wider than the plot";
     - "panel grid is too narrow: 80 pt per column";
     - "value N lies outside trained scale";
     - `CG-4` "more than 16 palette slots required" (13 runs).

     These are divergences, not diagnostics of unported features.
6. **The report content is mostly absent.**
   - **Decisions.** Never recalled: 02 D5 scale (862 oracle runs and probes),
     R2-07 unit placement (730), R2-21 labels as a set (690), R2-24 (568),
     02 D1 sort (487), R9-10 (418) and 112 more decision rules.
   - **Chooser path.** The rewrite emits guide steps (`G1.14`, `G1.20`, …) where
     the original's path cites R-rules with their levels, so 248 of 249 paths
     disagree.
   - **Findings.** Never recalled: R2-32/info (175), R2-23/info (95),
     02 D1/warn (49), R9-10/info (37), R2-27/warn (35), CG-3/info (26),
     CG-1/error (17) and 50 more.
   - **Severity values.** Some rewrite findings carry a level as their severity
     ("open", "guided"). The source lines are
     `crates/chartgen-layout/src/timeaxis.rs:314-316`, `annotations.rs:271-273`,
     `comments.rs:330-331` and `comments.rs:389-390`, and
     `special/events.rs:226`. These appear in 298 + 158 + 64 + 16 runs and probes.
     The original's severities are error, warn, info and overridden.
7. **Lints.**
   - **SVG lint:** 4 of the original's 34 checks (frames, text orientation, flat,
     vocabulary), with an explicit `CG-SVG-PARTIAL` finding
     (`crates/chartgen-svg-lint/src/lib.rs:333`).
   - **Style lint:** missing. The CLI says so (`python/chartgen/__main__.py:131-133`).
   - **Model lint:** partial; R2-05, R2-11, R2-12, R2-34, R5-6 and CG-1 only
     (`crates/chartgen-lint/src/lib.rs:6`, `131`).
8. **Colour ramps and classing are missing.** Heatmaps and shaded tables use one
   continuous tint of palette slot 1 (`crates/chartgen-layout/src/families/matrix.rs:278`).
   The original has stepped classing (linear, natural, quantile, log, custom),
   diverging ramps, `good`/`bad` encodings and cell strength (`ramps.py`).
   Decisions G4.29 and G4.34 are never recalled.
9. **Host-package defects.**
   - `python/chartgen/spec.py` is the original reader copied verbatim except for
     `parse_float=Decimal`. It still imports `chartgen.timeaxis` and
     `chartgen.layout.events` (`spec.py:1011,1028,1073,1084`), which the rewrite
     package lacks. So `check_structure` raises `ModuleNotFoundError` for
     irregular points, week and bin grids, and events (5 original tests).
   - `chartgen.measure.Measurer` lacks the original `measure`/`one` API.
   - It measures layout boxes with the Typst Python package 0.15.0 (with no
     baseline), where the original measured ink ascent and descent through the
     Typst 0.15.1 CLI via `bake.typeset`.

## Module and function families

Oracle paths are relative to `oracle/charts/chartgen/` unless they name
`tools/`. "Suite" refers to `original_suite/results`; "probes" to
`silent/result.json`.

| Original family | Rewrite location | Status | Evidence |
|---|---|---|---|
| `__init__.render` pipeline (`__init__.py:427`), semantic refusals `_semantic` (`:45`), `colour_slots` (`:167`), `censored_refusal` (`:227`), `compute_scale` (`:294`) | `src/lib.rs:98` `render_step`, `:103` `render_with_layout`; Python `python/chartgen/__init__.py:90` | partial | 249 of 414 runs complete (165 raise); status agrees on 242 of 249. R0-07 is recalled 4 of 4; R0-10, R1-06, R2-28 and R2-12 (as a refusal) are missed (probes `refusal missed`). The render contract is broken: the rewrite raises instead of returning a Report (headline 5). |
| `choose.py`: `procedure` (`:271`), `checks` (`:393`), `choose` (`:1106`), `line_refusals` (`:894`), `part_refusals` (`:930`), `panel_path` (`:1002`) | `crates/chartgen-policy/src/lib.rs:292` `decide`, `:75` `recommended`, `:191` `guide_path` | partial | Type agrees on 203 of 249. Of the oracle-refused runs, the refusal set agrees on 218 of 249 completed and 13 raise. The path agrees on 1 of 249. R7-02/R7-03/R7-05 errors are recalled about 95%, and R12-2…R12-7 and R6-38…R6-42 fully. Waterfall (10 runs) and slope (1 run) choose a different type. Suite `test_chooser`: 30 inapplicable (use `choose.choose`), 2 fail (`spec.check_structure` import error). `crates/chartgen-policy/tests/original_choices.rs` checks chosen types for bundled bases only. |
| `rules.py`: `Report`, `Finding` (`:553`, `:585`), levels from GUIDE.md (`:488`), `refusal`/`guided` (`:663`, `:649`) | `python/chartgen/rules.py` (Report/Finding shells); levels assigned in Rust per finding | partial | Report shape is kept. Levels can only be compared against GUIDE-less oracle fallbacks (GUIDE.md absent). Some severities carry level strings (headline 6). Guide citations are empty on both sides. |
| `spec.py`: dataclasses, `load` (`:882`), `read_file` (`:906`), `merge_patch` (`:893`), `schema` (`:967`), `check_structure` (`:986`) | Python: `python/chartgen/spec.py` (copy). Rust: `crates/chartgen-data/src/schema.rs:171` `validate`, `:278` `validate_structure`; schema `crates/chartgen-data/schema.json` | partial (Rust); missing (Python: a copy) | `test_spec` 11 of 11 pass, but host-python only (0 engine calls); this is not evidence for Rust. The Python `check_structure` crashes for time grids, points and events (headline 9). The Rust schema is the original's with nullable fields and extra Text forms (`typst`+`alternative`, LaTeX). The one invalid bundled run matches. Removing `table.stub[].values` is invalid in the original but drawn by the rewrite (probe). |
| `transform.py`: `apply` (`:170`), `for_type` (`:381`), `scale_values` (`:531`) | `crates/chartgen-data/src/lib.rs:653` `transform`, `:549` `resolve_comparisons` | partial | The probes for `transform.*` react. Refusal views agree in 3 of 4 (`transform.kind`), 2 of 3 (`relative`) and 4 of 8 (`unit.given_in`). `transform.unit`, `.unit.prefix/what` and `exposure.noun` are (partly) ignored. Decisions R15-13, R15-14 and R15-16 are never recalled. `test_transform`: 16 inapplicable (internals). |
| `scales.py`: `nice_scale` (`:142`), `lower_limit_mode` (`:78`), `breathing_room` (`:299`) | `crates/chartgen-scales/src/lib.rs:83` `nice`, `:329` `train` | partial | `summary.scale` equals the oracle's in 107 of 162 runs, and in 140 ignoring the thousands separators the rewrite omits. It differs in 22: the rewrite writes a hyphen-minus for −, and waterfall ranges differ (`p3-24`: 0–8 against −4–6). `scale.breathing`, `steps`, `zero` and `target_intervals` are partly or wholly ignored in probes. `test_scales`: 13 inapplicable. |
| `fmt.py`: `number` (`:143`), `currency` (`:91`), `choose_magnitude` (`:158`), `unit_phrase` (`:269`), `scale_title` (`:288`), `format_scale` (`:325`), `format_values` (`:383`), dates (`:428-448`) | `crates/chartgen-format/src/lib.rs` (same names: `:86`, `:169`, `:200`, `:344`, `:352`, `:417`, `:549`, `:622-649`) | partial | R2-11/info recalled 103 of 103. The summary scale string omits thousands separators and the typographic minus. `audience.locale` is ignored in the matrix family (`families/matrix.rs:101` formats without the locale). Magnitude style and unit position are ignored. `test_fmt`: 24 inapplicable (internals). |
| `timeaxis.py`: rows (`row1`/`row2` `:106`, `:129`; `fit` `:162`), continuous axis (`:404-466`), `check_grid` (`:815`), `rebin` (`:861`), `as_of_note` (`:661`), halos (`:741`) | `crates/chartgen-layout/src/timeaxis.rs` (regular rows), `crates/chartgen-data/src/irregular.rs:28`, `crates/chartgen-layout/src/special.rs:1646-1753` (irregular), `chartgen-data/src/lib.rs:204` `incomplete_periods` | partial / explicit-diagnostic | `data.x.grid` probes: refusal views agree in 24 of 24 (week start, bins). There are explicit errors: "step chart requires data.as_of or a two-bound time window", "dated event markers require an irregular step or thin-bar time axis", "incomplete value … needs distinct mark styling". Time-axis findings carry severity "open" (bug). `test_time_charts`: 15 fail and 11 public-api-absent (both class fixtures raise), 11 inapplicable. Rebinning `additive: false` misses the R2-43 refusal. |
| `ramps.py`: `Ramp` (`:49`), `choose` (`:218`), `Classes`/`classify` (`:513`, `:809`), `treatment`/`cell_strength` (`:412`, `:361`), `both_files` (`:386`) | none; `families/matrix.rs:278` linear tint only | missing | `chart.encoding` and `chart.cells` are ignored. `chart.classing`, `chart.breaks` and `facts.ramp_focus` have no evidence (the rewrite is blocked on those bases, or the original does not react). G4.29 and G4.34 are never recalled. Diverging `div-*` and stepped `seq-*` classes are absent from the rewrite's output. |
| `style.py` `lint_style` (`:31`), `lint_ramps` (`:83`), `lint_notes` (`:140`); flattening in `tools/figkit/style.py` | flattening: `python/chartgen/_support/style.py` (copy of figkit's); lint: none | flattening missing (a Python copy); lint explicit-diagnostic | `test_style` Flatten 3 pass, host-python only. The 4 lint tests are public-api-absent failures. `python -m chartgen style` prints "Rust style lint is not yet implemented" and exits 1. |
| `svglint.py`: `lint_svg` (`:2208`), 34 checks (`:823-2170`) | `crates/chartgen-svg-lint/src/lib.rs:124` | partial + explicit-diagnostic | Present: frames R1-03/04/05, R2-08, RP6-1/R14-1, CG-5. Every run gets `CG-SVG-PARTIAL`. CLI `lint` exits 2 by design. `test_svglint`: 22 public-api-absent, 5 fail, 2 environment, 4 inapplicable. |
| `lint.py` `lint_model` (`:243`) and its checks (`:16-224`) | `crates/chartgen-lint/src/lib.rs:6` `semantic`, `:131` `scene` | partial | R2-05, R2-11, R2-12, R2-34, R5-6 and CG-1 are coded. CG-1/error is never recalled in the oracle's 17 runs, and R2-12/warn is fully recalled. |
| `notes.py`: `describe` (`:134`), `NoteBatch` (`:155`), `adopt` (`:273`), `wrap` (`:521`), `apply` (`:595`) | `crates/chartgen-notes/src/lib.rs:122` `prepare`, `:200` `mark`; `crates/chartgen-note-layout/src/lib.rs:305` `solve`, `:209` `wrap` | partial | `notes[].on[].*` probes: the refusal view agrees in every reacted probe (element 15 of 15, status 13 of 13). Errors are explicit: "note for … needs a visible footnote", "one note word exceeds the chart width", "labels.notes.missing has no placed status in this family". `test_notes`: 6 pass (host-python), 23 fail. |
| `annotate.py`: `Annotator` (`:295`; comments, highlights, callouts, reference lines, title connectors), `copies` (`:192`), `Occupancy` (`:212`), `PanelAnnotator` (`:2002`) | `crates/chartgen-layout/src/annotations.rs:164` (highlight: strong shade only), `comments.rs:213` `place`, `:354` title link | partial / explicit-diagnostic | Highlight methods glow, thickness, selective_label, background, bold_label, icon and grey_others are each rejected: "highlight method X is not placed by this cartesian layout". Highlights on pie, table, treemap and stacked charts are rejected. Comments on pie, step and thin_bar are rejected. `comments[].answers` is ignored. Red-green copies (`copies`) are not produced. `test_annotate`: 1 pass, 16 fail, 6 inapplicable. |
| `layout/bars.py` `layout_bars` (`:358`), `layout_stacked` (`:2523`), value labels, targets, sorting | `crates/chartgen-layout/src/lib.rs:259` `solve` (cartesian), `:819-822` stack and waterfall guards, `guides.rs:250` value labels | partial | Bar: 91 of 117 complete, type agrees in all 91, 26 raise. Stacked bar: 9 of 15 complete. Stacked bar 100: 4 of 12. `chart.sort` and `chart.value_labels` probes: refusal views agree 8 of 8. `data.x.order` and `data.parts.kind/groups/order` are ignored. `test_clustered` 1 pass of 46 applicable; `test_stacks` 1 of 47. |
| `layout/lines.py` `layout_lines` (`:325`), `segment_meets_box` (`:144`), stretches | `layout/src/lib.rs:1347` `flush_path` and the cartesian solve | partial | Line: 31 of 58 complete, 27 raise. Path `d` geometry is not compared by `compare.py` (see tooling). `test_lines`: 10 not run (fixture: "inset is not supported"), 27 inapplicable. |
| `layout/dots.py` `layout_dots` (`:135`) | cartesian solve | partial | 7 of 12 complete. `test_dots`: 1 pass of 8 applicable. |
| `layout/areas.py` `layout_areas` (`:120`), `layout_line_total` (`:137`) | `layout/src/lib.rs:1950` `draw_areas`, `special.rs:1341` `line_total` | partial | Stacked area: 2 of 4 complete. Line total: 1 of 2, and its type disagrees. `facts.focus` and `facts.total_matters` are ignored. |
| `layout/step.py` `layout_step`/`layout_thin_bars` (`:347`, `:352`) | `layout/src/special.rs:1753` `irregular`, `:1949` thin bars | partial / explicit-diagnostic | Step: 2 of 4 complete. Thin bar: 1 of 5. Explicit as-of and comment guards. |
| `layout/arrowchart.py` `layout_arrows` (`:232`) | `special.rs:1018` `arrow` | partial / explicit-diagnostic | 3 of 6 drawn runs complete. Others raise `/chart/change_panels is not supported by portable layout` or "labels.notes.missing has no placed status in this family". |
| `layout/bumps.py` `layout_bumps` (`:100`), `ranks` (`:41`) | `special.rs:1206` `bumps`, `:1174` `rank_values` | partial (no completed run) | Both bumps runs raise ("highlight targets need a dedicated bumps layout", "period labels overlap in bumps chart"). |
| `layout/funnel.py` `layout_funnel_bars` (`:39`) | `special.rs:668` `funnel` | partial (no completed run) | Both runs raise "funnel stage labels overlap", where the original draws. |
| `layout/wrapped.py` `layout_wrapped` (`:39`) | `special.rs:528` `wrapped` | partial (no completed run) | Both runs raise (CG-4 palette slots). |
| `layout/waterfall.py` `layout_waterfall` (`:250`) | cartesian solve (`lib.rs:822`, `:1033`) | partial | All 10 runs complete, but the chosen type disagrees in 10 of 10 and the scale differs (`p3-24`). |
| `layout/pie.py` `layout_pie` (`:385`), `pack` (`:115`); `layout/packing.py` `pack` (`:20`) | `families/circular.rs:334` `pie` | partial | Pie: 9 of 26 complete, 17 raise ("pie labels cannot be packed", highlights, comments, CG-4). Donut: 1 of 1 completes. |
| `layout/treemap.py` `layout_treemap` (`:88`), `squarify` (`:40`) | `circular.rs:623` `treemap`, `:546` `squarify` | partial | 2 of 6 complete. Highlight targets are rejected. |
| `layout/heatmap.py` `layout_heatmap` (`:44`), censored modes (`:378-387`) | `families/matrix.rs:576` `heatmap` | partial | 9 of 16 complete. No classing, diverging or `cells` (see `ramps.py` above). |
| `layout/table.py` `layout_table` (`:118`), graphics, stub columns, censored shading | `families/matrix.rs:349` `table`, `:811` action dots | partial | Table: 10 of 23 complete. Action dots: 2 of 2. `table.legend`, `table.names_title` and `table.stub[].title` are ignored. Missed refusals: `table.bold` (R10-22), `table.graphics` (CG-1). `zebra stripes require an explicit semantic override` is explicit. `test_tables`: 1 pass of 38 applicable. |
| `layout/legend.py` (`:114` `need`, `:121` `key_width`) | `layout/src/lib.rs:1291-1310` compact legend | partial / divergent | "series legend does not fit the chart width" raises in 5 runs that the original draws. |
| `layout/inset.py` `layout` (`:485`), `find_outliers` (`:126`), `hull_links` | none; `layout/src/lib.rs:176` "inset is not supported by portable layout" | explicit-diagnostic | 9 runs raise. `test_insets`: 13 fail (fixture), 7 public-api-absent, 3 inapplicable. |
| `layout/multiples.py` (`:73` `applies`, `:206` `_auto`), merged charts | `crates/chartgen-panels/src/lib.rs:247` `prepare`, `crates/chartgen-panel-layout/src/lib.rs`; `layout/src/lib.rs:235` `/data/dims` and `/data/measures` guard | partial / explicit-diagnostic | Multiples of line: 2 of 13 complete. Multiples of bar: 0 of 4. Merged: 2 of 7. "panel grid is too narrow: 80.0 pt per column" fails the `test_multiples` class fixture, so none of its 24 tests runs (17 fail, 3 public-api-absent, 4 inapplicable by static reference). `panels.arrangement.*` and `data.dims` are diagnosed. |
| `layout/events.py` `place` (`:114`), `check_event` (`:70`) | `layout/src/special/events.rs` | explicit-diagnostic / partial | Events are placed only on irregular step or thin-bar axes. Elsewhere: "dated event markers require …". Event findings carry severity "open". |
| `layout/incomplete.py` (as-of incomplete periods, R2-42) | `chartgen-data/src/lib.rs:204` | explicit-diagnostic | "incomplete value for … needs distinct mark styling" (4 runs). |
| `layout/common.py` (`splits`, `best_lines`, `title_block`, `Batch`) | layout helpers (`layout/src/lib.rs:20-146`), `guides.rs` | not separately testable | 5 original tests use `Batch` directly (inapplicable). |
| `__main__.py` `main` (`:365`), `choose_path` (`:229`) | `python/chartgen/__main__.py:65` | partial | `test_cli`: 0 of 5 applicable pass. `choose` prints no defaults or decisions ("02 D5" missing). A refusal exits 0 instead of 1 (`as-answer` variant). `--json` lacks `choice`. `render` output lacks the original classes. `lint` exits 2 (partial lint), which is not equivalent. |
| `tools/figkit/measure.py` `Measurer.measure` (`:75`), cached batches via `bake.typeset` (see the figkit section) | `python/chartgen/measure.py:14` (Typst Python package) | partial | The API differs (`measure_requests`; no `measure`/`one`). Box height is measured, not ink extents, with no baseline. Typst package 0.15.0, against the CLI 0.15.1 the oracle recorded. |
| `tools/figkit/emit.py`, `model.py` (house SVG source) | `crates/chartgen-render/src/lib.rs:384` `svg` (compatibility source) | missing (vocabulary) | 0 of 4274 identities matched (headline 1). |
| `tools/figkit/typst.py` `escape` / `markup` | `python/chartgen/typst.py` (copy); Rust `chartgen-render/src/lib.rs:64` `typst_escape` | Python copy; Rust partial | `test_typst` `test_forms` passes in host Python only. `test_typesets_as_the_literal_text` needs `Measurer.measure`. |

## figkit, okhsl and bake (migration target per `agents/FIGKIT.md`)

The original chartgen re-exports `tools/figkit` (model, emit, measure, typst,
style; 785 lines) and uses `tools/okhsl.py` and `tools/bake.py`'s
`label_style`/`typeset`. `agents/FIGKIT.md` assigns each piece to a Rust layer and
forbids a figkit monolith and any Python delegation.

The differential checks are independent evidence. `tests/conformance/figkit/generate_fixtures.py`
ran the original functions (staged copy, verified against `oracle-source.sha256`;
Typst CLI 0.15.1 for measurement) and committed `fixtures.json`. That file
holds:

- 92 escape strings;
- 452 colour-function cases and 120 okhsl cases;
- 13 stylesheets: the four original ones (`base` and `message` fail standalone in
  the original itself) plus faulty and edge-case sheets;
- 114 `label_style` lookups over every text class in the golden outputs;
- 1062 geometry cases: Text, Rect, Path, Use and Curve boxes, 240 wedge shape,
  path, box and point checks, and overlap;
- 2 whole figures for `emit.svg`;
- 9 Typst extents.

`tests/conformance/figkit/check.py` replays them. Two controls frame the
results:

- **Original.** `--target oracle --oracle-tools ORIG/tools` passes every family:
  92, 452, 120, 13, 114, 1062, 2 and 9 cases.
- **`facade`.** This is FIGKIT.md's `chartgen.figkit` package and is the target
  once the pieces land. Every family is `unavailable` (no `chartgen.figkit`),
  which is a **known failure**, never a skip.

Separately, `host-copy` runs the original Python that the rewrite package copies
today. Results are in `figkit/results.json` and `figkit/RESULTS.md`.

| figkit piece (oracle file:line) | Assigned layer (FIGKIT.md) | What the rewrite has | Status | In the assigned layer? | Evidence |
|---|---|---|---|---|---|
| `okhsl.py` (`okhsl` `:190`, `srgb_to_okhsl` `:164`, `okhsl_to_srgb` `:139`, gamut `:45-104`) | leaf crate `chartgen-colour-space` | `python/chartgen/_support/okhsl.py`, a verbatim copy; no Rust crate | missing (Python copy) | no: host Python | `host-copy` okhsl 120/120 with 0 native calls; `facade` unavailable |
| `figkit/style.py` colour functions `hex_rgb`, `rgb_hex`, `oklch`, `from_oklch`, `mix`, `oklch_shift`, `oklch_light`, `luminance`, `contrast`, `text_tone` (`:88-158`) | `chartgen-colour-space` | Python copy in `python/chartgen/_support/style.py`. Private Rust fragments: `chartgen-layout/src/families/matrix.rs:254` `hex`, `:267` `mix` (sRGB mix only), `colours.rs:97` `tint`, `circular.rs:534` `tint`, `annotations.rs:136` `darken`. No OKLCH, gamut reduction, contrast or text tone in Rust. | missing in Rust (Python copy; several private divergent Rust helpers) | no | `host-copy` colour 452/452, 0 native calls. The private Rust helpers are not reachable for differential testing; the layouts that use them do not produce the original's paints (see headline 8; `compare_strict` paint check differs in 199 of 199 runs). |
| `figkit/style.Style`: parsing (`:67`), `@import` (`:37-60`), `@defs` directive, `var()` with fallback and cycle errors (`:189-212`), `em` (`:208-210`), colour functions in tokens, literal rules and `font-size` px (`:225`), flattened text (`:231`), `metric`/`paint`/`has` (`:241-255`) | crate `chartgen-style`, with `@import` through a host callback | `python/chartgen/_support/style.py` (copy; `maker` "chartgen-rs"). The Python host flattens and passes `tokens` to Rust in `Theme.tokens` (`python/chartgen/style.py:13`). | missing in Rust (Python copy) | no | `host-copy` style 13/13 (all fixtures, including the cycle, undefined-token and import-with-spaces errors), 0 native calls. No Rust stylesheet semantics exist; the Typst and LaTeX hosts cannot flatten a style without Python. |
| `bake.label_style(css, classes)` (`tools/bake.py:117`): typography from the class cascade | `chartgen-style::label_style`; layout requests must carry it | none. `chartgen-layout/src/guides.rs:36` `style_for` hard-codes weight and slant per role and takes ink from a few tokens; each call site passes its own size. | missing | n/a | `host-copy` and `facade` label_style unavailable. Divergence on output: in `p1-01` the rewrite sets scale labels at 9 pt, where `label_style(plain, prose scale-label)` gives 10 (fixture); title 12 bold matches. |
| `bake.typeset` and `figkit/measure.Measurer` (`measure.py:45`; `measure` `:75`, `one` `:97`, cache key `:71`, `Extent` `:27`) | host measurers; `Extent` ↔ `TextMetrics` fixed in the model | `python/chartgen/measure.py:14` (Typst Python package 0.15.0, `measure()` of `text(...)`) | partial | yes (host), but the semantics differ | The `measure`/`one` API is absent (fixture family unavailable; 2 original tests). Widths of 8 fixture labels equal the oracle's to 0.001 pt. Heights differ: the rewrite returns the box height (6.83 for `Hg`, 5.46 for `(missing)`, 8.23 for the title) where the original has ink ascent plus descent (8.89, 8.0, 10.73). No ascent/descent split or baseline (`baseline: None`). No cache keyed by Typst version. |
| `figkit/typst.escape`/`markup` (`typst.py:23`, `:34`) | `chartgen-render` public text-encoding module, exposed through the protocol | `python/chartgen/typst.py` (copy); a private Rust duplicate `chartgen-render/src/lib.rs:64` `typst_escape`, not exposed by the protocol (operations: render, render_scene, choose, schema, lint_scene, lint_svg) | partial (Python copy and private Rust duplicate) | no: two implementations, neither public in the assigned module | `host-copy` escape 92/92 (0 native calls). The Rust duplicate cannot be called directly. It differs from the original's regex in its numbered-list rule (`split_once(". ")` and ASCII digits, against `^(\d+)\.(\s)` and Unicode digits). |
| `figkit/model` data types `Text`, `Rect` (`rx`), `Line`, `Path` (`curve="flat"`), `Use`, `Curve` (head), `Wedge` (r0, inset), `Group` (classes/id/attrs), `Figure` (title/desc/comment/defs) (`model.py:13-256`) | `chartgen-model` scene primitives | `chartgen-model/src/lib.rs:507` `Primitive`: Rect (no rx), Line, Path (points, closed; no flat curve, no cubic segments), Circle, Text (no modifiers, no joint, no plain), Group (translate, clip; no attrs). No Use, Curve, Wedge, figure title/desc/comment/defs. Metadata has role, classes and data. | partial | yes (the layer exists), incomplete | Fixture families geometry and emit are unavailable on both targets. Without Use, Curve or Wedge primitives the original's mark symbols (`use href`), arrow heads and wedge sectors cannot be expressed (compare: 0 of 4274 identities). |
| `figkit/model` geometry: `Text.box` (anchor, ink extent) `:31`, `box(pad)` for each kind, `Wedge.point/shape/path/box` (gap inset, rings, slivers) `:143-221`, `Curve.points/head_classes` `:112-123`, flat bumps curves (`emit.py:57-66`), `overlap` `:258` | crate `chartgen-geometry` (model only); layout, scene and lint adopt | No crate. Private copies: `chartgen-layout/src/lib.rs:92` `boxes_overlap` and `chartgen-lint/src/lib.rs:125` `overlap` (same strict inequalities as `overlap`); `circular.rs:184` `make_path` approximates sectors by polylines every 5° with no inset gap, no sliver rule and no arc commands. | missing (as a layer); private divergent copies | no | Geometry fixtures (1062 cases) unavailable on the rewrite. By inspection, `make_path` cannot reproduce `Wedge.path` (arcs `A…`, inset apex `inset/sin(half)`, slivers dropped, full circle drawn as two arcs). |
| `figkit/emit.svg` (`emit.py:92`): header, GENERATED comment `--`→`–`, `<title>`, `<desc>`, `style#house`, `defs#house-defs`, `defs#chart-defs`, group tree, attribute order, `_n` 0.01 rounding, start anchor omitted, `-head` paths, determinism | `chartgen-render` `CompatibilitySvgRenderer` | `chartgen-render/src/lib.rs:384` `svg` (compatibility source): six-decimal numbers, `pt` sizes on the root, inline paint and fonts, `data-role`. In the 205 ratchet outputs: no GENERATED comment (0), no `defs#chart-defs` (0), `<desc>` in 94, and groups only for panels (4 outputs); otherwise a flat element list | missing (format) | yes (the layer exists), format not ported | Emit fixtures unavailable. `blindspots`/`compare_strict` figure, attribute, order and paint checks all differ on 199 of 199 runs; 0 of 4274 identities matched. |
| figkit's public Python API for other generators | `chartgen.figkit` façade over the Rust crates | none (`ModuleNotFoundError: chartgen.figkit`) | missing | n/a | `facade`: all 8 families unavailable (known failures). |

## Spec options

Per option, from the probes. "responds" and "refusals agree" are not
equivalence: they mean the rewrite changes its output or report where the
original does, and in "refusals agree" it reaches the same status, type and
refusal set. "no evidence" means that no probe could show a difference: the
original did not react on the probed bases, or the rewrite already failed on
them.

<!-- fields:begin -->
Generated by `tests/conformance/inventory/fields.py` from `tests/conformance/silent/result.json`;
JSON form: `tests/conformance/inventory/fields.json`.

Status counts: diagnosed 20, ignored 33, no evidence 58, not probed 15, partly ignored 23, refusal missed 16, refusals agree 46, responds 68.

| option | status | probe verdicts | Rust references (pointer, else key name) |
|---|---|---|---|
| `audience` | partly ignored | SILENT 1, diagnosed 1, reacted 1; refusals agree 0/1; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:395`, `crates/chartgen-format/src/lib.rs:544`, `crates/chartgen-format/src/lib.rs:545` |
| `audience.abbreviations` | ignored | SILENT 1, orig-unchanged 1, blocked 2; worst original change where flagged: output | `crates/chartgen-format/src/lib.rs:544`, `crates/chartgen-format/src/lib.rs:560` |
| `audience.bw_risk` | ignored | SILENT 4, orig-unchanged 1; worst original change where flagged: output | none found |
| `audience.cvd` | no evidence | orig-unchanged 3 | none found |
| `audience.data_savvy` | responds | orig-unchanged 2, reacted 2; refusals agree 0/2 | `crates/chartgen-policy/src/lib.rs:175`, `crates/chartgen-policy/src/lib.rs:583` |
| `audience.date_format` | not probed | - | none found |
| `audience.knows_scale` | no evidence | orig-unchanged 3 | none found |
| `audience.locale` | ignored | SILENT 2, orig-unchanged 4; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:395`, `crates/chartgen-format/src/lib.rs:545`, `crates/chartgen-format/src/lib.rs:559` |
| `audience.pie_averse` | responds | orig-unchanged 1, diagnosed 3, reacted 2 | `crates/chartgen-policy/src/lib.rs:116`, `crates/chartgen-policy/src/lib.rs:136` |
| `chart` | refusals agree | reacted 3; refusals agree 2/2 | `crates/chartgen-data/src/schema.rs:627`, `crates/chartgen-layout/src/families/circular.rs:218`, `crates/chartgen-layout/src/families/matrix.rs:611` |
| `chart.breaks` | no evidence | orig-unchanged 3 | none found |
| `chart.cells` | ignored | SILENT 2, orig-unchanged 4; worst original change where flagged: findings | none found |
| `chart.change_panels` | diagnosed | orig-unchanged 7, diagnosed 2 | `crates/chartgen-layout/src/lib.rs:223` |
| `chart.classing` | no evidence | orig-unchanged 10, blocked 5 | none found |
| `chart.encoding` | refusal missed | SILENT 3, orig-unchanged 3, blocked 3; refusal missed 1; worst original change where flagged: status | none found |
| `chart.gridlines` | refusal missed | SILENT 3; refusal missed 3; worst original change where flagged: status | none found |
| `chart.labelling` | diagnosed | orig-unchanged 2, diagnosed 4 | `crates/chartgen-layout/src/lib.rs:221` |
| `chart.orientation` | partly ignored | SILENT 1, orig-unchanged 1, diagnosed 2, reacted 5; refusals agree 3/3; worst original change where flagged: decisions/summary | `crates/chartgen-layout/src/orientation.rs:13`, `crates/chartgen-policy/src/lib.rs:646` |
| `chart.sort` | refusals agree | orig-unchanged 2, reacted 9; refusals agree 8/8 | `crates/chartgen-layout/src/families/circular.rs:218`, `crates/chartgen-panels/src/lib.rs:177`, `crates/chartgen-panels/src/lib.rs:236` |
| `chart.sort_by` | no evidence | orig-unchanged 5 | `crates/chartgen-data/src/schema.rs:627`, `crates/chartgen-policy/src/lib.rs:707` |
| `chart.type` | refusal missed | diagnosed 9, reacted 87; refusal missed 6; refusals agree 11/81; worst original change where flagged: status | `crates/chartgen-lint/src/lib.rs:36`, `crates/chartgen-policy/src/lib.rs:264`, `crates/chartgen-policy/src/lib.rs:366` |
| `chart.value_labels` | refusals agree | orig-unchanged 1, reacted 8; refusals agree 8/8 | `crates/chartgen-layout/src/families/matrix.rs:611`, `crates/chartgen-layout/src/guides.rs:232`, `crates/chartgen-layout/src/lib.rs:222` |
| `colours` | no evidence | blocked 3 | `crates/chartgen-colours/src/lib.rs:105`, `crates/chartgen-colours/src/lib.rs:294`, `crates/chartgen-data/src/schema.rs:632` |
| `colours.registry` | no evidence | blocked 3 | none found |
| `colours.slots` | responds | orig-unchanged 3, blocked 1, reacted 2 | `crates/chartgen-colours/src/lib.rs:105`, `crates/chartgen-colours/src/lib.rs:294`, `crates/chartgen-data/src/schema.rs:632` |
| `comments` | responds | blocked 1, reacted 2 | `crates/chartgen-data/src/schema.rs:537`, `crates/chartgen-data/src/schema.rs:614`, `crates/chartgen-note-layout/src/lib.rs:101` |
| `comments[].anchors` | responds | blocked 1, reacted 2 | `crates/chartgen-data/src/schema.rs:539`?, `crates/chartgen-layout/src/comments.rs:49`?, `crates/chartgen-layout/src/comments.rs:110`? |
| `comments[].anchors[].category` | responds | blocked 2, diagnosed 2, reacted 1 | `crates/chartgen-data/src/schema.rs:248`?, `crates/chartgen-data/src/schema.rs:583`?, `crates/chartgen-data/src/schema.rs:592`? |
| `comments[].anchors[].panel` | responds | diagnosed 2, reacted 1 | `crates/chartgen-panel-layout/src/lib.rs:348`?, `crates/chartgen-panels/src/lib.rs:546`?, `crates/chartgen-panels/src/lib.rs:558`? |
| `comments[].anchors[].series` | responds | blocked 3, diagnosed 2, reacted 4 | `crates/chartgen-data/src/lib.rs:303`?, `crates/chartgen-data/src/lib.rs:341`?, `crates/chartgen-data/src/lib.rs:638`? |
| `comments[].anchors[].x` | responds | blocked 2, diagnosed 2, reacted 1 | none found |
| `comments[].answers` | ignored | SILENT 4, orig-unchanged 1, blocked 3; worst original change where flagged: decisions/summary | none found |
| `comments[].arrow` | responds | diagnosed 2, reacted 1 | `crates/chartgen-layout/src/comments.rs:309`?, `crates/chartgen-model/src/lib.rs:163`?, `crates/chartgen-model/src/lib.rs:188`? |
| `comments[].placement` | diagnosed | blocked 3, diagnosed 6 | `crates/chartgen-layout/src/comments.rs:42`?, `crates/chartgen-layout/src/comments.rs:236`? |
| `comments[].text` | responds | blocked 1, reacted 2 | `crates/chartgen-colours/src/lib.rs:202`?, `crates/chartgen-data/src/schema.rs:565`?, `crates/chartgen-data/src/schema.rs:570`? |
| `data.actions` | refusals agree | orig-unchanged 5, diagnosed 2, reacted 10; refusals agree 2/2 | `crates/chartgen-data/src/schema.rs:451`, `crates/chartgen-layout/src/families/matrix.rs:869` |
| `data.as_of` | refusals agree | blocked 1, diagnosed 1, reacted 4; refusals agree 3/3 | `crates/chartgen-data/src/lib.rs:205`, `crates/chartgen-data/src/schema.rs:462` |
| `data.comparisons` | refusals agree | reacted 3; refusals agree 2/2 | `crates/chartgen-data/src/lib.rs:257`, `crates/chartgen-data/src/lib.rs:555`, `crates/chartgen-data/src/lib.rs:673` |
| `data.comparisons[].compute` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/lib.rs:283`?, `crates/chartgen-data/src/lib.rs:585`?, `crates/chartgen-policy/src/lib.rs:414`? |
| `data.comparisons[].factor` | responds | diagnosed 1, reacted 2; refusals agree 1/2 | `crates/chartgen-data/src/lib.rs:313`?, `crates/chartgen-data/src/lib.rs:314`?, `crates/chartgen-policy/src/lib.rs:299`? |
| `data.comparisons[].id` | partly ignored | SILENT 1, reacted 2; refusals agree 2/2; worst original change where flagged: output | none found |
| `data.comparisons[].kind` | ignored | SILENT 13, orig-unchanged 5; worst original change where flagged: findings | `crates/chartgen-data/src/lib.rs:280`?, `crates/chartgen-data/src/lib.rs:412`?, `crates/chartgen-data/src/lib.rs:416`? |
| `data.comparisons[].label` | partly ignored | SILENT 2, orig-unchanged 3, reacted 1; worst original change where flagged: decisions/summary | `crates/chartgen-data/src/lib.rs:337`?, `crates/chartgen-data/src/lib.rs:785`?, `crates/chartgen-layout/src/guides.rs:196`? |
| `data.comparisons[].reason` | no evidence | orig-unchanged 6 | `crates/chartgen-policy/src/lib.rs:17`? |
| `data.comparisons[].render` | refusals agree | orig-unchanged 4, diagnosed 6, reacted 2; refusals agree 1/1 | `crates/chartgen-data/src/lib.rs:339`?, `crates/chartgen-policy/src/lib.rs:311`?, `crates/chartgen-policy/src/lib.rs:470`? |
| `data.comparisons[].series` | responds | diagnosed 3, reacted 1 | `crates/chartgen-data/src/lib.rs:303`?, `crates/chartgen-data/src/lib.rs:341`?, `crates/chartgen-data/src/lib.rs:638`? |
| `data.comparisons[].unit.fallback` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-format/src/lib.rs:164`? |
| `data.comparisons[].unit.given_in` | refusals agree | reacted 12; refusals agree 12/12 | `crates/chartgen-data/src/lib.rs:227`?, `crates/chartgen-format/src/lib.rs:147`? |
| `data.comparisons[].unit.magnitude` | not probed | - | `crates/chartgen-data/src/lib.rs:232`?, `crates/chartgen-data/src/lib.rs:794`?, `crates/chartgen-data/src/schema.rs:666`? |
| `data.comparisons[].unit.noun` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/lib.rs:239`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `data.comparisons[].unit.prefix` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/lib.rs:236`?, `crates/chartgen-data/src/lib.rs:794`?, `crates/chartgen-data/src/lib.rs:857`? |
| `data.comparisons[].unit.scale_title` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-format/src/lib.rs:165`? |
| `data.comparisons[].unit.suffix` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/lib.rs:237`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `data.comparisons[].unit.what` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/lib.rs:834`?, `crates/chartgen-data/src/lib.rs:924`?, `crates/chartgen-data/src/lib.rs:941`? |
| `data.comparisons[].valid` | refusals agree | diagnosed 2, reacted 1; refusals agree 1/1 | `crates/chartgen-data/src/lib.rs:340`?, `crates/chartgen-policy/src/lib.rs:310`?, `crates/chartgen-policy/src/lib.rs:469`? |
| `data.comparisons[].values` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/lib.rs:284`?, `crates/chartgen-data/src/lib.rs:422`?, `crates/chartgen-data/src/lib.rs:463`? |
| `data.dims` | diagnosed | blocked 1, diagnosed 2 | `crates/chartgen-colours/src/lib.rs:81`, `crates/chartgen-colours/src/lib.rs:176`, `crates/chartgen-layout/src/lib.rs:219` |
| `data.dims[].id` | diagnosed | diagnosed 3 | none found |
| `data.dims[].kind` | no evidence | orig-unchanged 4 | `crates/chartgen-data/src/lib.rs:280`?, `crates/chartgen-data/src/lib.rs:412`?, `crates/chartgen-data/src/lib.rs:416`? |
| `data.dims[].name` | no evidence | blocked 6 | `crates/chartgen-colours/src/lib.rs:123`?, `crates/chartgen-data/src/lib.rs:373`?, `crates/chartgen-data/src/lib.rs:375`? |
| `data.dims[].values` | diagnosed | diagnosed 3 | `crates/chartgen-data/src/lib.rs:284`?, `crates/chartgen-data/src/lib.rs:422`?, `crates/chartgen-data/src/lib.rs:463`? |
| `data.measure` | responds | reacted 3; refusals agree 1/3 | `crates/chartgen-data/src/lib.rs:316`, `crates/chartgen-data/src/lib.rs:319`, `crates/chartgen-data/src/lib.rs:402` |
| `data.measure.additive` | refusal missed | SILENT 2, orig-unchanged 3; refusal missed 2; worst original change where flagged: status | none found |
| `data.measure.cumulative` | no evidence | orig-unchanged 3 | none found |
| `data.measure.dividing_point` | partly ignored | SILENT 2, reacted 1; refusals agree 1/1; worst original change where flagged: output | `crates/chartgen-policy/src/lib.rs:476` |
| `data.measure.good` | partly ignored | SILENT 4, orig-unchanged 2, reacted 1; refusals agree 1/1; worst original change where flagged: output | `crates/chartgen-policy/src/lib.rs:475` |
| `data.measure.heading` | no evidence | orig-unchanged 3 | `crates/chartgen-panels/src/lib.rs:432`?, `crates/chartgen-panels/src/lib.rs:433`? |
| `data.measure.id` | no evidence | orig-unchanged 3 | none found |
| `data.measure.kind` | responds | orig-unchanged 15, reacted 9 | `crates/chartgen-data/src/lib.rs:280`?, `crates/chartgen-data/src/lib.rs:412`?, `crates/chartgen-data/src/lib.rs:416`? |
| `data.measure.max` | responds | orig-unchanged 5, diagnosed 2, reacted 2 | none found |
| `data.measure.min` | refusal missed | SILENT 3, orig-unchanged 5, reacted 1; refusal missed 2; refusals agree 0/1; worst original change where flagged: status | none found |
| `data.measure.name` | no evidence | orig-unchanged 6 | `crates/chartgen-colours/src/lib.rs:123`?, `crates/chartgen-data/src/lib.rs:373`?, `crates/chartgen-data/src/lib.rs:375`? |
| `data.measure.persistence` | refusals agree | orig-unchanged 6, blocked 1, reacted 2; refusals agree 2/2 | `crates/chartgen-policy/src/lib.rs:146`, `crates/chartgen-policy/src/lib.rs:344`, `crates/chartgen-policy/src/lib.rs:356` |
| `data.measure.precision` | ignored | SILENT 3, blocked 6; worst original change where flagged: output | `crates/chartgen-data/src/lib.rs:770`?, `crates/chartgen-data/src/lib.rs:867`?, `crates/chartgen-data/src/lib.rs:947`? |
| `data.measure.unit` | responds | reacted 3; refusals agree 1/3 | `crates/chartgen-data/src/lib.rs:316`, `crates/chartgen-data/src/lib.rs:319`, `crates/chartgen-data/src/schema.rs:654` |
| `data.measure.unit.fallback` | partly ignored | SILENT 2, blocked 2, reacted 2; worst original change where flagged: output | `crates/chartgen-format/src/lib.rs:164`? |
| `data.measure.unit.given_in` | refusals agree | reacted 12; refusals agree 4/4 | `crates/chartgen-data/src/lib.rs:227`?, `crates/chartgen-format/src/lib.rs:147`? |
| `data.measure.unit.magnitude` | responds | blocked 3, reacted 1 | `crates/chartgen-data/src/lib.rs:232`?, `crates/chartgen-data/src/lib.rs:794`?, `crates/chartgen-data/src/schema.rs:666`? |
| `data.measure.unit.noun` | partly ignored | SILENT 1, reacted 5; refusals agree 2/4; worst original change where flagged: output | `crates/chartgen-data/src/lib.rs:239`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `data.measure.unit.prefix` | responds | blocked 4, reacted 2 | `crates/chartgen-data/src/lib.rs:236`?, `crates/chartgen-data/src/lib.rs:794`?, `crates/chartgen-data/src/lib.rs:857`? |
| `data.measure.unit.scale_title` | no evidence | orig-unchanged 4, blocked 2 | `crates/chartgen-format/src/lib.rs:165`? |
| `data.measure.unit.suffix` | partly ignored | SILENT 2, blocked 2, diagnosed 2; worst original change where flagged: output | `crates/chartgen-data/src/lib.rs:237`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `data.measure.unit.what` | responds | blocked 2, reacted 4; refusals agree 0/1 | `crates/chartgen-data/src/lib.rs:834`?, `crates/chartgen-data/src/lib.rs:924`?, `crates/chartgen-data/src/lib.rs:941`? |
| `data.measure.zero` | refusals agree | orig-unchanged 5, reacted 1; refusals agree 1/1 | `crates/chartgen-policy/src/lib.rs:182` |
| `data.measures` | responds | orig-unchanged 1, reacted 2; refusals agree 0/2 | `crates/chartgen-data/src/schema.rs:657`, `crates/chartgen-layout/src/lib.rs:220`, `crates/chartgen-panels/src/lib.rs:62` |
| `data.measures[].additive` | not probed | - | none found |
| `data.measures[].cumulative` | not probed | - | none found |
| `data.measures[].dividing_point` | not probed | - | `crates/chartgen-data/src/lib.rs:771`? |
| `data.measures[].good` | not probed | - | `crates/chartgen-layout/src/colours.rs:27`?, `crates/chartgen-layout/src/families/matrix.rs:903`? |
| `data.measures[].heading` | no evidence | blocked 6 | `crates/chartgen-panels/src/lib.rs:432`?, `crates/chartgen-panels/src/lib.rs:433`? |
| `data.measures[].id` | diagnosed | orig-unchanged 2, diagnosed 4 | none found |
| `data.measures[].kind` | responds | orig-unchanged 18, blocked 3, reacted 3 | `crates/chartgen-data/src/lib.rs:280`?, `crates/chartgen-data/src/lib.rs:412`?, `crates/chartgen-data/src/lib.rs:416`? |
| `data.measures[].max` | no evidence | orig-unchanged 6 | none found |
| `data.measures[].min` | no evidence | orig-unchanged 6 | none found |
| `data.measures[].name` | responds | orig-unchanged 2, blocked 2, reacted 2 | `crates/chartgen-colours/src/lib.rs:123`?, `crates/chartgen-data/src/lib.rs:373`?, `crates/chartgen-data/src/lib.rs:375`? |
| `data.measures[].persistence` | refusals agree | orig-unchanged 6, blocked 2, reacted 1; refusals agree 1/1 | none found |
| `data.measures[].precision` | no evidence | orig-unchanged 1, blocked 3 | `crates/chartgen-data/src/lib.rs:770`?, `crates/chartgen-data/src/lib.rs:867`?, `crates/chartgen-data/src/lib.rs:947`? |
| `data.measures[].unit` | responds | orig-unchanged 1, blocked 1, reacted 1; refusals agree 0/1 | `crates/chartgen-data/src/lib.rs:315`?, `crates/chartgen-data/src/lib.rs:316`?, `crates/chartgen-data/src/lib.rs:319`? |
| `data.measures[].unit.fallback` | not probed | - | `crates/chartgen-format/src/lib.rs:164`? |
| `data.measures[].unit.given_in` | not probed | - | `crates/chartgen-data/src/lib.rs:227`?, `crates/chartgen-format/src/lib.rs:147`? |
| `data.measures[].unit.magnitude` | responds | orig-unchanged 2, blocked 3, reacted 2 | `crates/chartgen-data/src/lib.rs:232`?, `crates/chartgen-data/src/lib.rs:794`?, `crates/chartgen-data/src/schema.rs:666`? |
| `data.measures[].unit.noun` | partly ignored | SILENT 1, orig-unchanged 2, blocked 2, reacted 1; worst original change where flagged: findings | `crates/chartgen-data/src/lib.rs:239`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `data.measures[].unit.prefix` | responds | orig-unchanged 2, blocked 2, diagnosed 1, reacted 1 | `crates/chartgen-data/src/lib.rs:236`?, `crates/chartgen-data/src/lib.rs:794`?, `crates/chartgen-data/src/lib.rs:857`? |
| `data.measures[].unit.scale_title` | ignored | SILENT 2, orig-unchanged 1, blocked 1; worst original change where flagged: output | `crates/chartgen-format/src/lib.rs:165`? |
| `data.measures[].unit.suffix` | no evidence | blocked 3 | `crates/chartgen-data/src/lib.rs:237`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `data.measures[].unit.what` | refusals agree | orig-unchanged 1, blocked 3, reacted 2; refusals agree 1/1 | `crates/chartgen-data/src/lib.rs:834`?, `crates/chartgen-data/src/lib.rs:924`?, `crates/chartgen-data/src/lib.rs:941`? |
| `data.measures[].zero` | responds | orig-unchanged 4, blocked 1, reacted 1 | `crates/chartgen-data/src/lib.rs:767`?, `crates/chartgen-data/src/lib.rs:865`?, `crates/chartgen-data/src/lib.rs:944`? |
| `data.parts` | responds | reacted 3; refusals agree 0/2 | `crates/chartgen-colours/src/lib.rs:242`, `crates/chartgen-colours/src/lib.rs:384`, `crates/chartgen-data/src/schema.rs:452` |
| `data.parts.all_parts` | refusal missed | SILENT 1, orig-unchanged 1, reacted 4; refusal missed 1; refusals agree 0/4; worst original change where flagged: status | `crates/chartgen-policy/src/lib.rs:96`?, `crates/chartgen-policy/src/lib.rs:541`? |
| `data.parts.groups` | ignored | SILENT 2; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:452` |
| `data.parts.kind` | ignored | SILENT 2; worst original change where flagged: output | `crates/chartgen-data/src/lib.rs:280`?, `crates/chartgen-data/src/lib.rs:412`?, `crates/chartgen-data/src/lib.rs:416`? |
| `data.parts.meaningful_total` | responds | orig-unchanged 2, reacted 4; refusals agree 0/4 | `crates/chartgen-policy/src/lib.rs:95`?, `crates/chartgen-policy/src/lib.rs:540`? |
| `data.parts.order` | partly ignored | SILENT 1, reacted 1; worst original change where flagged: output | `crates/chartgen-policy/src/lib.rs:678` |
| `data.parts.overlap` | responds | orig-unchanged 2, reacted 4; refusals agree 0/4 | `crates/chartgen-policy/src/lib.rs:97`?, `crates/chartgen-policy/src/lib.rs:542`?, `crates/chartgen-policy/src/lib.rs:545`? |
| `data.parts.overlaps` | refusal missed | SILENT 1; refusal missed 1; worst original change where flagged: status | `crates/chartgen-policy/src/lib.rs:545`? |
| `data.parts.total` | no evidence | orig-unchanged 1 | `crates/chartgen-layout/src/colours.rs:24`?, `crates/chartgen-layout/src/families/matrix.rs:117`?, `crates/chartgen-layout/src/families/matrix.rs:137`? |
| `data.series` | not probed | - | `crates/chartgen-colours/src/lib.rs:99`, `crates/chartgen-data/src/lib.rs:263`, `crates/chartgen-data/src/lib.rs:347` |
| `data.series[].certainty` | no evidence | orig-unchanged 9 | `crates/chartgen-data/src/lib.rs:93`?, `crates/chartgen-data/src/lib.rs:100`?, `crates/chartgen-data/src/lib.rs:137`? |
| `data.series[].id` | responds | reacted 3 | none found |
| `data.series[].keys` | diagnosed | diagnosed 3 | `crates/chartgen-colours/src/lib.rs:129`?, `crates/chartgen-panels/src/lib.rs:356`?, `crates/chartgen-panels/src/lib.rs:390`? |
| `data.series[].measure` | diagnosed | orig-unchanged 2, diagnosed 4 | `crates/chartgen-panels/src/lib.rs:84`?, `crates/chartgen-panels/src/lib.rs:91`?, `crates/chartgen-panels/src/lib.rs:155`? |
| `data.series[].name` | no evidence | orig-unchanged 6 | `crates/chartgen-colours/src/lib.rs:123`?, `crates/chartgen-data/src/lib.rs:373`?, `crates/chartgen-data/src/lib.rs:375`? |
| `data.series[].points` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/irregular.rs:30`?, `crates/chartgen-data/src/lib.rs:354`?, `crates/chartgen-data/src/schema.rs:257`? |
| `data.series[].role` | refusal missed | orig-unchanged 4, blocked 6, reacted 8; refusal missed 2; refusals agree 2/4; worst original change where flagged: status | `crates/chartgen-colours/src/lib.rs:170`?, `crates/chartgen-data/src/lib.rs:597`?, `crates/chartgen-layout/src/colours.rs:42`? |
| `data.series[].slot` | partly ignored | SILENT 1, orig-unchanged 2, reacted 3; worst original change where flagged: decisions/summary | `crates/chartgen-colours/src/lib.rs:133`?, `crates/chartgen-data/src/schema.rs:357`?, `crates/chartgen-layout/src/colours.rs:35`? |
| `data.x.events` | diagnosed | blocked 2, diagnosed 1 | `crates/chartgen-data/src/schema.rs:502`, `crates/chartgen-layout/src/special/events.rs:6` |
| `data.x.events[].at` | refusals agree | blocked 7, reacted 2; refusals agree 2/2 | none found |
| `data.x.events[].label` | no evidence | blocked 3 | `crates/chartgen-data/src/lib.rs:337`?, `crates/chartgen-data/src/lib.rs:785`?, `crates/chartgen-layout/src/guides.rs:196`? |
| `data.x.grid` | refusals agree | reacted 24; refusals agree 24/24 | `crates/chartgen-data/src/lib.rs:213`, `crates/chartgen-data/src/schema.rs:250`, `crates/chartgen-layout/src/timeaxis.rs:95` |
| `data.x.groups` | responds | blocked 2, diagnosed 3, reacted 1 | `crates/chartgen-colours/src/lib.rs:215`, `crates/chartgen-data/src/schema.rs:450`, `crates/chartgen-layout/src/families/circular.rs:51` |
| `data.x.kind` | refusals agree | reacted 6; refusals agree 4/4 | `crates/chartgen-data/src/schema.rs:250`, `crates/chartgen-panels/src/lib.rs:82` |
| `data.x.levels` | not probed | - | none found |
| `data.x.names` | ignored | SILENT 2, orig-unchanged 2, blocked 2; worst original change where flagged: output | `crates/chartgen-colours/src/lib.rs:209` |
| `data.x.order` | ignored | SILENT 1, orig-unchanged 2; worst original change where flagged: decisions/summary | `crates/chartgen-policy/src/lib.rs:677` |
| `data.x.roles` | partly ignored | SILENT 6, blocked 5, reacted 4; worst original change where flagged: output | `crates/chartgen-colours/src/lib.rs:212`, `crates/chartgen-data/src/schema.rs:449`, `crates/chartgen-layout/src/families/circular.rs:45` |
| `data.x.title` | ignored | SILENT 1, orig-unchanged 2; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:602` |
| `data.x.values` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/lib.rs:284`?, `crates/chartgen-data/src/lib.rs:422`?, `crates/chartgen-data/src/lib.rs:463`? |
| `data.x.week_start` | refusals agree | orig-unchanged 4, reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/schema.rs:401` |
| `facts` | responds | blocked 1, reacted 2 | `crates/chartgen-colours/src/lib.rs:75`, `crates/chartgen-layout/src/families/matrix.rs:371`, `crates/chartgen-panels/src/lib.rs:97` |
| `facts.compare` | diagnosed | blocked 3, diagnosed 3 | `crates/chartgen-colours/src/lib.rs:75`, `crates/chartgen-panels/src/lib.rs:97` |
| `facts.compare_shapes` | refusals agree | orig-unchanged 2, reacted 1; refusals agree 1/1 | `crates/chartgen-panels/src/lib.rs:326` |
| `facts.distance_to_zero_irrelevant` | responds | orig-unchanged 1, blocked 2, reacted 2 | `crates/chartgen-scales/src/lib.rs:449` |
| `facts.exact_values_needed` | ignored | SILENT 3, blocked 2; worst original change where flagged: output | none found |
| `facts.focus` | ignored | SILENT 4, orig-unchanged 2; worst original change where flagged: type | none found |
| `facts.incomplete_value_needed` | no evidence | orig-unchanged 2, blocked 2 | none found |
| `facts.location_matters` | refusal missed | OUT-IGN 4; refusal missed 2; worst original change where flagged: status | `crates/chartgen-policy/src/lib.rs:229` |
| `facts.message` | responds | orig-unchanged 2, blocked 3, reacted 10 | `crates/chartgen-layout/src/families/matrix.rs:371`, `crates/chartgen-policy/src/lib.rs:80`, `crates/chartgen-policy/src/lib.rs:193` |
| `facts.ramp_focus` | no evidence | orig-unchanged 2, blocked 4 | none found |
| `facts.small_differences_matter` | no evidence | blocked 6 | none found |
| `facts.small_values_matter` | ignored | SILENT 2, orig-unchanged 2, blocked 2; worst original change where flagged: findings | none found |
| `facts.total_matters` | ignored | SILENT 6; worst original change where flagged: findings | none found |
| `highlight` | partly ignored | SILENT 1, blocked 1, diagnosed 1; worst original change where flagged: decisions/summary | `crates/chartgen-data/src/schema.rs:521` |
| `highlight[].dim_others` | refusals agree | blocked 4, reacted 1; refusals agree 1/1 | `crates/chartgen-layout/src/annotations.rs:86`? |
| `highlight[].emphasis` | responds | orig-unchanged 2, reacted 1 | none found |
| `highlight[].glow` | responds | orig-unchanged 2, diagnosed 2, reacted 2 | none found |
| `highlight[].method` | partly ignored | SILENT 2, orig-unchanged 7, blocked 1, diagnosed 23; worst original change where flagged: findings | `crates/chartgen-layout/src/annotations.rs:71`? |
| `highlight[].targets` | diagnosed | diagnosed 3 | `crates/chartgen-data/src/schema.rs:523`?, `crates/chartgen-layout/src/annotations.rs:67`?, `crates/chartgen-panels/src/lib.rs:552`? |
| `highlight[].targets[].category` | responds | blocked 3, reacted 3 | `crates/chartgen-data/src/schema.rs:248`?, `crates/chartgen-data/src/schema.rs:583`?, `crates/chartgen-data/src/schema.rs:592`? |
| `highlight[].targets[].panel` | responds | orig-unchanged 1, diagnosed 1, reacted 1 | `crates/chartgen-panel-layout/src/lib.rs:348`?, `crates/chartgen-panels/src/lib.rs:546`?, `crates/chartgen-panels/src/lib.rs:558`? |
| `highlight[].targets[].series` | responds | blocked 2, diagnosed 5, reacted 2 | `crates/chartgen-data/src/lib.rs:303`?, `crates/chartgen-data/src/lib.rs:341`?, `crates/chartgen-data/src/lib.rs:638`? |
| `highlight[].targets[].x` | diagnosed | orig-unchanged 2, blocked 2, diagnosed 1 | none found |
| `inset` | responds | orig-unchanged 2, reacted 1 | `src/lib.rs:302` |
| `inset.compact_main` | no evidence | orig-unchanged 1, blocked 4 | none found |
| `inset.kind` | refusals agree | orig-unchanged 6, blocked 3, reacted 3; refusals agree 3/3 | `src/lib.rs:302` |
| `inset.members` | no evidence | orig-unchanged 2 | none found |
| `inset.position` | no evidence | orig-unchanged 10, blocked 5 | `crates/chartgen-model/src/lib.rs:588`? |
| `inset.share_axis` | no evidence | orig-unchanged 6 | none found |
| `insight` | partly ignored | SILENT 1, blocked 1, reacted 1; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:516`, `crates/chartgen-layout/src/annotations.rs:25`, `crates/chartgen-policy/src/lib.rs:708` |
| `insight.action` | no evidence | orig-unchanged 5 | none found |
| `insight.explain` | no evidence | orig-unchanged 6 | none found |
| `insight.explain_anchor` | no evidence | orig-unchanged 2 | none found |
| `insight.explain_anchor.category` | no evidence | orig-unchanged 3 | `crates/chartgen-data/src/schema.rs:248`?, `crates/chartgen-data/src/schema.rs:583`?, `crates/chartgen-data/src/schema.rs:592`? |
| `insight.explain_anchor.panel` | no evidence | orig-unchanged 3 | `crates/chartgen-panel-layout/src/lib.rs:348`?, `crates/chartgen-panels/src/lib.rs:546`?, `crates/chartgen-panels/src/lib.rs:558`? |
| `insight.explain_anchor.series` | no evidence | orig-unchanged 7 | `crates/chartgen-data/src/lib.rs:303`?, `crates/chartgen-data/src/lib.rs:341`?, `crates/chartgen-data/src/lib.rs:638`? |
| `insight.explain_anchor.x` | no evidence | orig-unchanged 5 | none found |
| `insight.important` | ignored | SILENT 4, blocked 2; worst original change where flagged: output | none found |
| `insight.targets` | responds | orig-unchanged 2, reacted 1 | `crates/chartgen-data/src/schema.rs:516`, `crates/chartgen-layout/src/annotations.rs:25`, `crates/chartgen-policy/src/lib.rs:708` |
| `insight.targets[].category` | responds | diagnosed 3, reacted 3 | `crates/chartgen-data/src/schema.rs:248`?, `crates/chartgen-data/src/schema.rs:583`?, `crates/chartgen-data/src/schema.rs:592`? |
| `insight.targets[].panel` | diagnosed | orig-unchanged 1, blocked 2, diagnosed 1 | `crates/chartgen-panel-layout/src/lib.rs:348`?, `crates/chartgen-panels/src/lib.rs:546`?, `crates/chartgen-panels/src/lib.rs:558`? |
| `insight.targets[].series` | responds | orig-unchanged 6, reacted 3 | `crates/chartgen-data/src/lib.rs:303`?, `crates/chartgen-data/src/lib.rs:341`?, `crates/chartgen-data/src/lib.rs:638`? |
| `insight.targets[].x` | responds | orig-unchanged 2, reacted 2 | none found |
| `insight.why_care` | no evidence | orig-unchanged 4 | none found |
| `job` | responds | reacted 3; refusals agree 0/3 | `crates/chartgen-policy/src/lib.rs:203`, `crates/chartgen-policy/src/lib.rs:332`, `crates/chartgen-policy/src/lib.rs:479` |
| `job.kind` | refusal missed | OUT-IGN 9, reacted 6; refusal missed 9; worst original change where flagged: status | `crates/chartgen-policy/src/lib.rs:203`, `crates/chartgen-policy/src/lib.rs:479` |
| `job.text` | no evidence | orig-unchanged 3 | `crates/chartgen-colours/src/lib.rs:202`?, `crates/chartgen-data/src/schema.rs:565`?, `crates/chartgen-data/src/schema.rs:570`? |
| `labels` | responds | diagnosed 1, reacted 2 | `crates/chartgen-layout/src/guides.rs:204`, `crates/chartgen-layout/src/special.rs:9`, `crates/chartgen-note-layout/src/lib.rs:109` |
| `labels.notes` | responds | blocked 3, diagnosed 1, reacted 2 | `crates/chartgen-layout/src/guides.rs:204`, `crates/chartgen-layout/src/special.rs:9`, `crates/chartgen-note-layout/src/lib.rs:109` |
| `labels.parts` | responds | orig-unchanged 2, blocked 5, diagnosed 3, reacted 1 | none found |
| `layout` | ignored | SILENT 1, blocked 2; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:640`, `crates/chartgen-data/src/schema.rs:641` |
| `layout.height` | ignored | SILENT 3; worst original change where flagged: findings | none found |
| `layout.plot_aspect` | ignored | SILENT 3, orig-unchanged 2, blocked 4; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:641` |
| `layout.width` | partly ignored | SILENT 2, blocked 4, reacted 3; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:640` |
| `notes` | responds | blocked 2, reacted 1 | `crates/chartgen-data/src/schema.rs:563`, `crates/chartgen-notes/src/lib.rs:124` |
| `notes[].on` | responds | blocked 2, reacted 1 | none found |
| `notes[].on[].category` | refusals agree | blocked 1, reacted 5; refusals agree 4/4 | `crates/chartgen-data/src/schema.rs:248`?, `crates/chartgen-data/src/schema.rs:583`?, `crates/chartgen-data/src/schema.rs:592`? |
| `notes[].on[].comment` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-data/src/schema.rs:579`?, `crates/chartgen-data/src/schema.rs:588`?, `crates/chartgen-data/src/schema.rs:613`? |
| `notes[].on[].element` | refusals agree | blocked 2, reacted 15; refusals agree 15/15 | `crates/chartgen-data/src/schema.rs:579`?, `crates/chartgen-data/src/schema.rs:587`?, `crates/chartgen-data/src/schema.rs:597`? |
| `notes[].on[].series` | refusals agree | blocked 3, reacted 4; refusals agree 4/4 | `crates/chartgen-data/src/lib.rs:303`?, `crates/chartgen-data/src/lib.rs:341`?, `crates/chartgen-data/src/lib.rs:638`? |
| `notes[].on[].status` | refusals agree | blocked 5, reacted 13; refusals agree 13/13 | `crates/chartgen-data/src/lib.rs:78`?, `crates/chartgen-data/src/lib.rs:79`?, `crates/chartgen-data/src/schema.rs:579`? |
| `notes[].on[].stub` | not probed | - | `crates/chartgen-data/src/schema.rs:579`?, `crates/chartgen-data/src/schema.rs:589`?, `crates/chartgen-data/src/schema.rs:618`? |
| `notes[].text` | responds | blocked 2, reacted 1 | `crates/chartgen-colours/src/lib.rs:202`?, `crates/chartgen-data/src/schema.rs:565`?, `crates/chartgen-data/src/schema.rs:570`? |
| `overrides` | not probed | - | `crates/chartgen-policy/src/lib.rs:14` |
| `overrides[].reason` | no evidence | orig-unchanged 3 | `crates/chartgen-policy/src/lib.rs:17`? |
| `overrides[].rule` | no evidence | orig-unchanged 3 | `crates/chartgen-colours/src/lib.rs:202`?, `crates/chartgen-layout/src/families/matrix.rs:49`?, `crates/chartgen-policy/src/lib.rs:16`? |
| `panels` | no evidence | blocked 1 | `crates/chartgen-colours/src/lib.rs:66`, `crates/chartgen-panels/src/lib.rs:67`, `crates/chartgen-panels/src/lib.rs:306` |
| `panels.arrangement` | no evidence | blocked 1 | `crates/chartgen-colours/src/lib.rs:66`, `crates/chartgen-panels/src/lib.rs:67` |
| `panels.arrangement.columns` | diagnosed | diagnosed 4 | `crates/chartgen-layout/src/families/matrix.rs:561`?, `crates/chartgen-panels/src/lib.rs:71`? |
| `panels.arrangement.rows` | diagnosed | diagnosed 6 | `crates/chartgen-layout/src/families/matrix.rs:557`?, `crates/chartgen-panels/src/lib.rs:70`? |
| `panels.arrangement.within` | diagnosed | diagnosed 4 | `crates/chartgen-colours/src/lib.rs:66` |
| `panels.scale` | refusals agree | orig-unchanged 7, reacted 2; refusals agree 2/2 | `crates/chartgen-panels/src/lib.rs:306`, `crates/chartgen-panels/src/lib.rs:315` |
| `panels.series_labels` | no evidence | orig-unchanged 9 | none found |
| `purpose` | responds | orig-unchanged 4, reacted 2 | `crates/chartgen-policy/src/lib.rs:20`, `crates/chartgen-policy/src/lib.rs:202`, `crates/chartgen-policy/src/lib.rs:332` |
| `reasons` | ignored | SILENT 5, orig-unchanged 1; worst original change where flagged: findings | `crates/chartgen-policy/src/lib.rs:41` |
| `scale` | no evidence | orig-unchanged 2, blocked 1 | `crates/chartgen-format/src/lib.rs:530`, `crates/chartgen-format/src/lib.rs:533`, `crates/chartgen-format/src/lib.rs:543` |
| `scale.allow_2_5` | no evidence | orig-unchanged 3 | `crates/chartgen-scales/src/lib.rs:442`? |
| `scale.breathing` | ignored | SILENT 2, orig-unchanged 4; worst original change where flagged: decisions/summary | `crates/chartgen-scales/src/lib.rs:448`? |
| `scale.magnitude` | no evidence | blocked 1 | `crates/chartgen-format/src/lib.rs:530`, `crates/chartgen-format/src/lib.rs:533` |
| `scale.magnitude_style` | ignored | SILENT 2, orig-unchanged 4; worst original change where flagged: findings | `crates/chartgen-format/src/lib.rs:543` |
| `scale.max` | refusal missed | OUT-IGN 1, orig-unchanged 1, diagnosed 3, reacted 1; refusal missed 2; refusals agree 0/1; worst original change where flagged: status | none found |
| `scale.min` | partly ignored | SILENT 1, orig-unchanged 3, blocked 1, diagnosed 2, reacted 2; refusals agree 0/2; worst original change where flagged: decisions/summary | `crates/chartgen-policy/src/lib.rs:636` |
| `scale.precision_value` | not probed | - | `crates/chartgen-scales/src/lib.rs:447`? |
| `scale.step` | no evidence | orig-unchanged 3 | `crates/chartgen-model/src/lib.rs:161`?, `crates/chartgen-model/src/lib.rs:186`?, `crates/chartgen-policy/src/lib.rs:148`? |
| `scale.steps` | ignored | SILENT 1, orig-unchanged 2; worst original change where flagged: findings | `crates/chartgen-scales/src/lib.rs:417`? |
| `scale.target_intervals` | partly ignored | SILENT 1, orig-unchanged 3, reacted 2; refusals agree 1/1; worst original change where flagged: decisions/summary | `crates/chartgen-scales/src/lib.rs:443`? |
| `scale.unit_position` | ignored | SILENT 2, orig-unchanged 7; worst original change where flagged: findings | none found |
| `scale.zero` | ignored | SILENT 1, orig-unchanged 3, blocked 2; worst original change where flagged: decisions/summary | `crates/chartgen-policy/src/lib.rs:183`, `crates/chartgen-policy/src/lib.rs:635` |
| `source` | no evidence | orig-unchanged 3 | `src/lib.rs:117`, `src/lib.rs:252` |
| `source.book` | no evidence | orig-unchanged 6 | none found |
| `source.notes` | no evidence | orig-unchanged 6 | `crates/chartgen-layout/src/lib.rs:169`?, `crates/chartgen-layout/src/lib.rs:183`?, `crates/chartgen-layout/src/lib.rs:186`? |
| `source.shows` | no evidence | orig-unchanged 6 | none found |
| `source.status` | no evidence | orig-unchanged 9 | `crates/chartgen-data/src/lib.rs:78`?, `crates/chartgen-data/src/lib.rs:79`?, `crates/chartgen-data/src/schema.rs:579`? |
| `table` | ignored | SILENT 1, blocked 2; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:467`, `crates/chartgen-data/src/schema.rs:495`, `crates/chartgen-data/src/schema.rs:603` |
| `table.bold` | refusal missed | SILENT 3, orig-unchanged 3, blocked 3; refusal missed 3; worst original change where flagged: status | `crates/chartgen-layout/src/guides.rs:47`?, `crates/chartgen-layout/src/guides.rs:87`?, `crates/chartgen-render/src/lib.rs:49`? |
| `table.graphics` | refusal missed | diagnosed 2, reacted 13; refusal missed 2; refusals agree 0/2; worst original change where flagged: status | `crates/chartgen-layout/src/families/matrix.rs:340`, `crates/chartgen-layout/src/families/matrix.rs:365` |
| `table.graphics_columns` | no evidence | orig-unchanged 3 | `crates/chartgen-data/src/schema.rs:495` |
| `table.legend` | ignored | SILENT 2, orig-unchanged 4; worst original change where flagged: output | none found |
| `table.names_title` | ignored | SILENT 2, blocked 4; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:603` |
| `table.row_key` | no evidence | orig-unchanged 21 | none found |
| `table.stripes` | diagnosed | orig-unchanged 1, blocked 1, diagnosed 1 | `crates/chartgen-layout/src/families/matrix.rs:336` |
| `table.stub` | ignored | SILENT 1, blocked 2; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:467`, `crates/chartgen-data/src/schema.rs:619` |
| `table.stub[].kind` | partly ignored | SILENT 2, orig-unchanged 1, blocked 2, reacted 1; refusals agree 1/1; worst original change where flagged: findings | `crates/chartgen-data/src/lib.rs:280`?, `crates/chartgen-data/src/lib.rs:412`?, `crates/chartgen-data/src/lib.rs:416`? |
| `table.stub[].title` | ignored | SILENT 2, blocked 4; worst original change where flagged: output | `crates/chartgen-data/src/schema.rs:599`?, `crates/chartgen-layout/src/comments.rs:349`?, `crates/chartgen-layout/src/families/circular.rs:160`? |
| `table.stub[].values` | refusal missed | SILENT 1, blocked 2; refusal missed 1; worst original change where flagged: status | `crates/chartgen-data/src/lib.rs:284`?, `crates/chartgen-data/src/lib.rs:422`?, `crates/chartgen-data/src/lib.rs:463`? |
| `title` | refusal missed | reacted 3; refusal missed 3; refusals agree 0/3; worst original change where flagged: status | `crates/chartgen-data/src/schema.rs:553`, `crates/chartgen-data/src/schema.rs:556`, `crates/chartgen-data/src/schema.rs:599` |
| `title.descriptive` | refusals agree | blocked 2, reacted 4; refusals agree 3/3 | `crates/chartgen-data/src/schema.rs:600`, `crates/chartgen-scene/src/lib.rs:244` |
| `title.kind` | refusals agree | orig-unchanged 3, reacted 6; refusals agree 6/6 | `crates/chartgen-data/src/lib.rs:280`?, `crates/chartgen-data/src/lib.rs:412`?, `crates/chartgen-data/src/lib.rs:416`? |
| `title.link` | responds | reacted 3 | `crates/chartgen-data/src/schema.rs:553`, `crates/chartgen-data/src/schema.rs:556`, `crates/chartgen-layout/src/comments.rs:18` |
| `title.link.category` | responds | diagnosed 3, reacted 3 | `crates/chartgen-data/src/schema.rs:248`?, `crates/chartgen-data/src/schema.rs:583`?, `crates/chartgen-data/src/schema.rs:592`? |
| `title.link.panel` | responds | diagnosed 2, reacted 1 | `crates/chartgen-panel-layout/src/lib.rs:348`?, `crates/chartgen-panels/src/lib.rs:546`?, `crates/chartgen-panels/src/lib.rs:558`? |
| `title.link.series` | responds | diagnosed 3, reacted 1 | `crates/chartgen-data/src/lib.rs:303`?, `crates/chartgen-data/src/lib.rs:341`?, `crates/chartgen-data/src/lib.rs:638`? |
| `title.link.x` | responds | diagnosed 2, reacted 1 | none found |
| `title.text` | refusals agree | reacted 3; refusals agree 1/1 | `crates/chartgen-data/src/schema.rs:599`, `crates/chartgen-scene/src/lib.rs:234` |
| `transform` | responds | reacted 3; refusals agree 1/2 | `crates/chartgen-data/src/lib.rs:654`, `crates/chartgen-policy/src/lib.rs:471`, `crates/chartgen-policy/src/lib.rs:472` |
| `transform.at` | responds | orig-unchanged 2, diagnosed 1, reacted 2 | none found |
| `transform.base` | refusals agree | diagnosed 3, reacted 4; refusals agree 1/1 | `crates/chartgen-data/src/lib.rs:876`?, `crates/chartgen-data/src/lib.rs:879`? |
| `transform.comparison` | refusals agree | reacted 3; refusals agree 3/3 | `crates/chartgen-policy/src/lib.rs:472` |
| `transform.exposure` | refusals agree | reacted 1; refusals agree 1/1 | `crates/chartgen-data/src/lib.rs:798`?, `crates/chartgen-data/src/lib.rs:846`? |
| `transform.exposure.noun` | partly ignored | SILENT 1, reacted 2; refusals agree 0/1; worst original change where flagged: output | `crates/chartgen-data/src/lib.rs:239`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `transform.exposure.values` | not probed | - | `crates/chartgen-data/src/lib.rs:284`?, `crates/chartgen-data/src/lib.rs:422`?, `crates/chartgen-data/src/lib.rs:463`? |
| `transform.kind` | responds | reacted 6; refusals agree 3/4 | `crates/chartgen-policy/src/lib.rs:471` |
| `transform.name` | refusals agree | reacted 3; refusals agree 1/1 | `crates/chartgen-colours/src/lib.rs:123`?, `crates/chartgen-data/src/lib.rs:373`?, `crates/chartgen-data/src/lib.rs:375`? |
| `transform.per` | responds | reacted 4; refusals agree 0/1 | none found |
| `transform.relative` | responds | reacted 4; refusals agree 2/3 | `crates/chartgen-data/src/lib.rs:690`? |
| `transform.unit` | ignored | SILENT 1; worst original change where flagged: output | `crates/chartgen-data/src/lib.rs:315`?, `crates/chartgen-data/src/lib.rs:316`?, `crates/chartgen-data/src/lib.rs:319`? |
| `transform.unit.fallback` | responds | reacted 3; refusals agree 0/2 | `crates/chartgen-format/src/lib.rs:164`? |
| `transform.unit.given_in` | responds | reacted 12; refusals agree 4/8 | `crates/chartgen-data/src/lib.rs:227`?, `crates/chartgen-format/src/lib.rs:147`? |
| `transform.unit.magnitude` | not probed | - | `crates/chartgen-data/src/lib.rs:232`?, `crates/chartgen-data/src/lib.rs:794`?, `crates/chartgen-data/src/schema.rs:666`? |
| `transform.unit.noun` | responds | reacted 3; refusals agree 0/1 | `crates/chartgen-data/src/lib.rs:239`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `transform.unit.prefix` | partly ignored | SILENT 2, reacted 2; refusals agree 0/1; worst original change where flagged: output | `crates/chartgen-data/src/lib.rs:236`?, `crates/chartgen-data/src/lib.rs:794`?, `crates/chartgen-data/src/lib.rs:857`? |
| `transform.unit.scale_title` | responds | reacted 3; refusals agree 0/2 | `crates/chartgen-format/src/lib.rs:165`? |
| `transform.unit.suffix` | responds | reacted 3; refusals agree 0/1 | `crates/chartgen-data/src/lib.rs:237`?, `crates/chartgen-data/src/lib.rs:792`?, `crates/chartgen-data/src/lib.rs:794`? |
| `transform.unit.what` | partly ignored | SILENT 1, reacted 3; refusals agree 1/2; worst original change where flagged: output | `crates/chartgen-data/src/lib.rs:834`?, `crates/chartgen-data/src/lib.rs:924`?, `crates/chartgen-data/src/lib.rs:941`? |
| `data value: estimate` | no evidence | orig-unchanged 7, blocked 1 | none found |
| `data value: incomplete` | diagnosed | orig-unchanged 6, blocked 1, diagnosed 1 | none found |
| `data value: missing-null` | refusals agree | blocked 2, diagnosed 3, reacted 3; refusals agree 2/2 | none found |
| `data value: noted` | diagnosed | blocked 2, diagnosed 6 | none found |
| `data value: pending` | refusals agree | blocked 2, diagnosed 3, reacted 3; refusals agree 2/2 | none found |
| `data value: projection` | no evidence | orig-unchanged 7, blocked 1 | none found |
| `data value: zero` | refusal missed | blocked 3, diagnosed 1, reacted 4; refusal missed 1; refusals agree 1/3 | none found |
<!-- fields:end -->
