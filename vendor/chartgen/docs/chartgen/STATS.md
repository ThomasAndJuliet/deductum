# Statistical chart types

Status: **implemented in the main pipeline; the rules are proposed, not rulings.** Every rule
below is tagged `guided` and numbered `XS-…`. They follow the library's stance (refuse what
misrepresents the data, state what was computed, never infer a missing fact), but nothing in the
book behind the rest of `GUIDE.md` covers distributions, intervals or scatter plots. Each needs an
owner decision and a source before it is locked (see "Decisions for the owner"). Nothing here
changes a ruling in `tests/rulings/`, `RULINGS.md` or `ARCHITECTURE.md`.

## Types and their input

`chart.type` is one of the following, or `auto`, which routes by the data (below).

| `chart.type` | Shows | Series carry |
|---|---|---|
| `box` | quartiles, whiskers and outliers per category | `samples` (one array per `x` value), or `summaries` |
| `strip` | every measurement, with its median | `samples` |
| `ecdf` | the share of measurements at or below each value | `samples` |
| `scatter` | two numeric measures, one mark per case | `xy` pairs; `data.x_measure` names the x axis |
| `dumbbell` | two conditions on the same categories, joined (`view: pair`) or their difference from zero (`view: difference`) | `values` (exactly two series) |
| `interval_dot`, `interval_bar` | an estimate with an interval that says what it is | `samples`, or `successes` with `trials`, or `values` with `intervals` |
| `state_matrix` | a declared state in every cell | `states` (one id per `x` value); `data.states` declares them |
| `contingency` | counts of an exhaustive two-way partition, with marginals | `counts`; `data.parts` {all_parts, overlap}; `x.title` and `data.series_name` name the dimensions |

The input follows the library's conventions: `measure.kind: percent` means percent units (45 for
45 %), `measure.min`/`max` give theoretical limits (R2-28), `measure.good` (`higher`|`lower`)
colours a paired contrast good or bad (R3-17), `chart.orientation` and `chart.sort`/`sort_by` as
for any chart (`sort_by: difference` sorts a dumbbell by its two series' difference). Chart
options are in the schema (`crates/chartgen-data/schema.json`): `whiskers`, `quantiles`,
`show_points`, `mean`, `notch`, `n`, `interval`, `view`, `difference`, `reference_quantiles`, `fit`.

`chart.type: auto` picks from the series: `xy` is a scatter plot, `counts` a contingency table,
`states` a state matrix, `successes` or `intervals` an interval dot, `samples` a box (a strip when a
group has fewer than 5 measurements; an ECDF for `facts.message: distribution_shape`; an interval dot
for `estimate_uncertainty`), and two `values` series with `facts.message: paired_change` a dumbbell.

## Where each piece lives

| Stage | Crate | What it does for these charts |
|---|---|---|
| input | `chartgen-statistics` (new, no geometry or I/O) | `parse` reads a spec into a typed model, checks it against the rules, computes the statistics (`stats.rs`), and returns issues and defaulted choices. Policy, scales and layout all call it, so they cannot disagree about what the chart contains |
| data | `chartgen-data` | schema fields; the generic stages see one value per category and series: the estimate the chart is about (median, interval estimate, paired value, a state's index, a count) |
| policy | `chartgen-policy` `statistical.rs` | routes the type (procedure step "variables"), turns the issues into refusals, warnings and infos with their rule, path and what to supply, and records defaulted choices as decisions |
| scales | `chartgen-scales` | trains the value axis on what is drawn (whiskers and outliers, intervals, both conditions, curves), by the library's own rules; `interval_bar` is length-encoded, so R2-24 puts it on zero |
| colours | `chartgen-colours` | series take colours as clustered bars do; states take palette slots in declaration order |
| layout | `chartgen-layout` `families/statistical*.rs` | one family, text measured by the host through the `Kit`; row charts (box, strip, interval, dumbbell) share a frame in either orientation; ECDF and scatter have two numeric axes; the grids are tables |
| lint | `chartgen-lint` | R2-32 (values against scale labels) is not applied to summaries and samples; the scene lint (text overlap, text outside the chart) applies to them as to any chart |

Features the family does not place (`highlight`, facets, insets, annotations, panels, bands,
reference lines, comments) are refused with an explicit error, never silently dropped.

## Statistics

Computed in `chartgen-statistics/src/stats.rs`, as pandas and Altair compute them: count, mean,
stdev (n−1), quartiles, quantile methods `linear` (R type 7, the default), `weibull`, `lower`,
`higher`, `nearest`, `midpoint`; Tukey (`k`×IQR, default 1.5), min–max and percentile whiskers; the
ECDF with merged ties; histogram on a 1-2-5 step; a percentile bootstrap of the mean or median with
an explicit seed; Wilson score and normal-approximation intervals; least squares with Pearson's r;
Sarle's bimodality coefficient. Everything is deterministic. What the generator defaulted (whisker
rule, quantile method, the bootstrap's resamples and seed) is a decision in the report and a
sentence in the chart's note, so the reader and the provenance both see it. The numbers behind each
mark are in the report's `summary.statistics`.

## Proposed rules

`error` refuses the chart and says what to supply (these are not overridable: there is no form of
the chart to draw without the fact). `warn` and `info` accompany a drawn chart.

| Rule | | |
|---|---|
| XS-GEN-01 | error: more than six series cannot be told apart (eight curves for an ECDF, XS-ECDF-01) |
| XS-ROUTE | the type asked for or implied, recorded in the report's path |
| XS-BOX-01 | info: no whisker rule, so Tukey 1.5×IQR is used and stated; error for an unknown rule |
| XS-BOX-02 | error: `n: hide` (n is always shown); an unknown quantile method |
| XS-BOX-03 | error: n below 5 in a box (use `strip`) |
| XS-BOX-04 | warn: n of 5 to 8, every point is drawn over the box |
| XS-BOX-05 | error: an empty category |
| XS-BOX-06 | error: `notch` implies a confidence-interval comparison the chart does not compute |
| XS-BOX-07 | info: a sample that may be multimodal (coefficient above 5/9, n of 20 or more) |
| XS-BOX-08..10 | error: summarised input states its whisker rule, carries n, and supplies its outliers or says `none` |
| XS-STRIP-01 / 02 | warn: over 300 points in a row overplot (use `ecdf`); error: an empty category |
| XS-IV-01 | error: an interval declares its kind: `confidence`, `bound`, `range`, `quantile_band` |
| XS-IV-02 | error: a confidence interval states its level and method |
| XS-IV-03 | error: a bound or range states what it bounds; a quantile band names its quantiles |
| XS-IV-04 | error: a bar needs a zero that is meaningful |
| XS-IV-05 | warn: bootstrap with n below 10, normal approximation with n below 30 |
| XS-IV-06 | error: counts need a Wilson interval; a method that cannot be computed from the input; an estimate outside its interval; n below 2 |
| XS-DB-01..04 | error: exactly two conditions; paired (equal lengths); a difference interval that includes its difference; known `view` and `difference` |
| XS-SC-01 | error: `size` encoding is not offered |
| XS-SC-02 | error: only `fit: linear` is offered |
| XS-SC-03 | error: two named measures |
| XS-SC-04 / 05 | info above 500 points (transparent markers), warn above 2,000 (aggregate); warn: too few points for a fit |
| XS-MX-01 / 02 | error: every cell state is declared (a blank is not a state); at most six states |
| XS-CT-01 / 02 | error: the table asserts a partition (`all_parts: true`, `overlap: false`); at least one case |
| XS-DEFAULT, XS-LAYOUT | decisions: a defaulted choice; how the chart was laid out |

Existing rules apply unchanged: R0-07 (job), R2-12 (title), R2-11 (a percent measure needs a unit
phrase), R2-24 (bars start at zero), the scale rules R2-2x, and the scene lint.

## Not built

- **Annotations on these charts**: highlight, comments, insets, panels (refused, above).
- **Log axes**: the library has none; skewed latency is drawn on a linear axis, and the ECDF is the
  chart for tails.
- **Vertical orientation** for the ECDF, scatter, state matrix and contingency table (the ECDF and
  scatter are always x over y; the matrix puts categories down the rows unless `orientation:
  vertical`, which needs labels that fit).
- **UpSet** and other multi-set views; a 2×2 table covers two configurations.
- **Python `ChartSpec` dataclasses**: the original's spec classes live in the private sandbox; the
  schema here is the contract.
- **The Python conformance ratchet** (`tools/conformance`) is not in this checkout, so it was not
  run. The Rust suite, including the oracle-comparison tests, passes.

## Decisions for the owner

1. Do the proposed `XS-` rules become rulings, with sources? (Cleveland and McGill on position and
   length; Cumming and Finch on interval display; Tukey for the box.)
2. Whisker rule and quantile method **default** (Tukey 1.5, linear) rather than being refused, on
   the argument that the chart's own note states them. The alternative is to require them.
3. The library computes statistics from samples (a change from "draws what it is given"). Summarised
   input is accepted only with its whisker rule, n and outliers.
4. Whether `state_matrix` should take its palette slots from `colour_slots` (it does not: states take
   slots in declaration order).

## Run

```sh
cargo test -p chartgen-statistics                   # statistics and parsing
cargo test -p chartgen --test statistical           # the charts through the whole pipeline
CHARTGEN_STATS_OUT=out cargo test -p chartgen --test statistical   # also writes each SVG
```

The tests render with a stand-in text measurer. In LaTeX, `\chartgen{spec.json}` draws them with the
document's own fonts; the specs are in `crates/chartgen/tests/statistical/`.
