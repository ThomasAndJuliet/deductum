# Chart generator: specification

Design of `chartgen`, the generator that draws charts by the rules of
*Practical Charts* as recorded in `notes/`, under the owner decisions in
`README.md` (which override the notes). This is a specification, not code.

Conventions of this document:

- **Rule IDs** are the notes' own (R2-24 is rule 24 of `notes/02-scales.md`;
  R0/R1 `00-01`, R2 `02`, R3/R4 `03-04`, R5/RP2 `05`, R6 `06`, R7 `07`,
  R8–R12 `08-12`, RP3/R13/R14/RP5/R15/RP6/RP7 `13-15`). Several files number
  their procedures alike, so procedures and test figures carry the file:
  02 D5, 06 DP6-4, 07 §4.1, 08-12 DP-5, 13-15 DP-5; 00-01 F1, 02 #9, 06 T10.
- **Provenance of values**: *M* measured from the book's figures by the
  note-takers; *N* a threshold or proposal of the notes themselves (marked
  "mine" there); *O* our choice, made here.
- All lengths are in points (one SVG unit is one point, as in the diagrams).
  `em` means the chart's label size (`--label-size`, 10 pt in the plain
  style, owner 30 Sep 2026), never the Typst or CSS em of a parent.
- **Status (30 Sep 2026).** Stage 1 is built: the foundation (spec, style,
  measurement, scales, formatting, layout helpers, emit, report, examples
  build) and bar charts. Where the build departs from what this document
  first said, the text below has been changed to match and the change is
  marked "(stage 1)"; the owner's decisions of 30 September (README.md;
  DECISIONS.md D-2, D-3 decided, the other recommendations provisional
  defaults) are applied throughout. Stage 2 adds dot plots of one series,
  over categories and over time (`layout/dots.py`, drawn by the bar layout
  with a dot for each bar); its changes are marked "(stage 2)". Stage 2
  also adds line charts of one or more series with end labels, markers,
  projections and highlighting (`layout/lines.py`, `layout/packing.py`),
  multi-level time axes (`timeaxis.py`) and the chooser for time series;
  those changes are marked "(stage 2, lines)". The inset stage adds inset
  charts to bars, dot plots and lines (`layout/inset.py`, with hooks in the
  bar and line layouts and checks in both lints); its changes are marked
  "(inset stage)", and §6.6 is rewritten as built. Phase 2 adds several
  panels: small multiples over `data.dims` and merged charts of several
  measures, instead of a dual axis (`layout/multiples.py`); its changes are
  marked "(panels)", and §6.10 is rewritten as built. Phase 2 also draws
  several series over categories: clustered bars with a legend, bars with
  flat reference markers, value labels instead of a scale, a table with the
  graph, and dot plots of several series (`layout/bars.py`, `layout/dots.py`,
  and the legend component shared by every type, `layout/legend.py`); those
  changes are marked "(series)", and §6.7 gains items 11–14. Phase 2's
  message layer makes charts obvious (the book's Part 5): derived measures
  and comparison values before layout (`transform.py`), then good and bad
  (red and green in one file, textured in black and white; D-18),
  comparison lines,
  the highlight methods other than colour and thickness, the title connector
  and comments, on the laid-out chart (`annotate.py`, `style/message.css`);
  its changes are marked "(message)", and §6.12 describes it as built.
  Phase 2's time charts: step charts and thin bars on a true time axis
  (`layout/step.py`, `timeaxis.continuous_fit`), arrow charts
  (`layout/arrowchart.py`), bumps charts (`layout/bumps.py`), incomplete
  periods drawn apart (`layout/lines.py`, `layout/incomplete.py`),
  black-and-white symbols, and lint checks for what was only a default; its
  changes are marked "(time)", and §6.13 describes it as built.
  Phase 3's tables, heatmaps and colour ramps (`ramps.py`, `layout/table.py`,
  `layout/heatmap.py`, `layout/funnel.py`, the ramp and grouped keys in
  `layout/legend.py`, the ramps' tokens and rules in the plain style): their
  changes are marked "(tables)", and §6.14–§6.17 describe them as built.
  Phase 3's full chooser (`choose.procedure`: G1.10's steps, the groups, the
  four checks, the avoid-list, the slope chart; weeks and bins in
  `timeaxis.py`): its changes are marked "(chooser)", in §3.2 and §6.13.10–11.
- The rule-level open questions of the notes are being collected into
  `DECISIONS.md`. Where this design needs one of them settled it assumes a
  default and says so (Appendix A); §9 asks only design questions.

Contents: 1 Architecture · 2 The chart spec · 3 Pipeline · 4 The SVG output
· 5 The plain stylesheet · 6 Algorithms · 7 Chart types and phases · 8 Test
cases · 9 Open design questions · Appendix A Assumed rule-level defaults.


## 1. Architecture

### 1.1 Language: Python

`chartgen` is a Python 3 package; it takes a declarative chart spec (JSON)
and a stylesheet and writes an SVG source in the house format, which
`tools/bake.py` then bakes. Reasons:

1. **Layout needs typeset sizes.** Every layout decision the book cares
   about (the unit phrase and where the top gridline starts, wrapping
   category names instead of rotating them, packing end labels, whether an
   inset fits, the width of the scale-label column) depends on the exact
   size of labels set by Typst in New Computer Modern. `bake.typeset` gives
   them in Python, in one Typst run for a whole batch of labels. Julia would
   have to shell out to Python for every chart or duplicate the Typst path.
2. **The rest of the tool chain is Python.** `bake.py`, and the house
   effects a later style will add (`shadows.py`, `labels.py`, textures), are
   Python; the charts can reuse bake's own stylesheet lookup so that what is
   measured is exactly what is baked (§1.6).
3. **The dependencies are already there.** `decimal` gives exact step
   arithmetic and half-away-from-zero rounding (R5-16); `json` and
   `dataclasses` give the spec and its schema; shapely (already required by
   bake) gives convex hulls for inset connectors and box tests for
   collisions.
4. **Specs come from several producers.** A JSON spec is what an AI writes
   most reliably, and what a Julia generator can write with a 40-line
   base-only writer (`tools/chartspec.jl`, the analogue of `svg.jl`). Julia
   generators keep computing data; they stop drawing charts by hand.

No plotting library is used: their defaults contradict the rules (pie
direction, legends, rotated labels, missing values drawn as zero), their SVG
is not the house format, and every chart here is a few hundred elements that
are simpler to write directly.

### 1.2 Components and files

```
diagrams/charts/
  README.md  SPEC.md  DECISIONS.md  notes/
  chartgen/                 the package (stdlib + shapely; imports ../../tools/bake.py)
    __init__.py             render(spec, style) -> (svg_text, report)
    __main__.py             CLI (below)
    spec.py                 dataclasses, JSON loading, variants, schema export, validation
    rules.py                the rule registry: ID -> title, stage, default severity, notes file
    choose.py               chart-type decision procedures (06 DP6-1, 02 D1, 07 §4, 08-12 DP-2 …)
    scales.py               zero decision, limits, interval (02 D2-D5), inset scales
    fmt.py                  numbers as a set, unit phrases, dates (Decimal throughout)
    timeaxis.py             multi-level time axes (stage 1: regular grids, for bars over periods)
    style.py                stylesheet loading: @import, var(), em, colour functions; tokens
    measure.py              batched, cached Typst measurement (bake.typeset + bake.label_style)
    typst.py                escaping plain text into Typst markup
    model.py                the chart model: figure, panels, scales, marks, texts, geometry
    layout/                 common.py (figure stacking, titles, text boxes), bars.py, lines.py,
                            dots.py, inset.py, packing.py, legend.py; later multiples.py, pie.py …
                            (stacks, 6.18: bars.py's stack mode, waterfall.py, areas.py)
    emit.py                 SVG writer (house format)
    lint.py                 model lint (a list of checks, CHECKS); the style lint is style.lint_style
    svglint.py              SVG lint: the locked rules read off any drawing in the vocabulary (stage 2)
    schema/chartspec-1.json generated JSON Schema, for AI producers and editors (by the build)
  style/
    base.css                roles, greys, lines, metrics (the plain style without data colours)
    palette-muted.css       data colours, muted family (default)
    palette-office.css      data colours, Office family
    plain.css               @import "base.css"; @import "palette-muted.css";
    plain-office.css        @import "base.css"; @import "palette-office.css";
    plain-defs.svg          mark library (dot, line markers, B&W symbols, highlight icons)
  examples/
    charts-examples.typ     the figures-only document of generated examples
    build.sh                ./build.sh [--pack] [style]
    specs/*.json            one spec (with variants) per example
    gen/*.jl                optional Julia generators of specs
    src/<style>/            generated sources (never edited)
    lint/<style>/           reports (JSON)
    build/<style>/          flattened style.css, baked SVGs
  tests/                    unittest: tables from the notes, fixtures for the lint
                            (python3 -m unittest discover -s tests, from diagrams/charts)
    fixtures/*.svg          hand-written SVGs in the vocabulary for the SVG lint (stage 2; §8.3)
  AUTHORING.md              how an AI producer writes a spec and reads the report (stage 2)
  .cache/measure.json       the measurement cache (not in the repository)
diagrams/tools/chartspec.jl minimal JSON writer for Julia generators (stage 2; §1.6)
```

CLI:

- `python3 -m chartgen style STYLE --out build/STYLE/style.css` flattens a
  stylesheet (§1.5) and runs the style lint.
- `python3 -m chartgen render SPEC.json --style STYLE --out SRC.svg
  --report REPORT.json [--variant NAME]` validates, chooses, lays out, emits
  and lints. On a validation error it writes no SVG and a report whose
  findings say why.
- `python3 -m chartgen build SPECS_DIR --style STYLE --src DIR --reports DIR`
  renders every spec and variant (`id.svg`, `id--variant.svg`) with a
  report each, and fails when one does not provoke what its spec expects
  (stage 1; the examples build uses it).
- `python3 -m chartgen choose SPEC.json [--variant NAME] [--json]` prints the
  decision path only (later stage; built in stage 2): each decision of the
  chooser and the scale with its rule, guide rules and level, the choice
  (type, orientation, sort, labelling) with its rule and level, the
  refusals and findings so far by level, and the facts the spec leaves to
  defaults with what reads each; nothing is laid out, so orientation and
  labelling are settled only by `render`. Exits non-zero when refused or
  invalid.
- `python3 -m chartgen lint FILE.svg --style STYLE` lints any SVG written in
  the vocabulary (for hand-made fixtures such as the heavy chart of 00-01 F1;
  later stage, with the full lint; built in stage 2, §3.6): findings grouped
  by level, the worst first; `--final-width PT` when the chart is printed
  scaled, `--cvd no`, `--json FILE`; exits non-zero on an error.
- `python3 -m chartgen schema [--out FILE]` prints or writes the JSON Schema.

### 1.3 Data flow

```
spec.json ─┐                                   ┌─ report.json (decisions, findings)
           ├─ validate → choose → scales → layout ─ emit ─ lint ┤
style.css ─┘        (measure: Typst, cached)                     └─ src/<style>/id.svg
                                                                        │
                        build/<style>/style.css + plain-defs.svg ─ bake.py
                                                                        │
                                               build/<style>/id.svg ─ Typst document
```

### 1.4 How the rules are enforced

**Levels (owner, 30 Sep 2026).** Every piece of guidance has a level, and the
generator carries it: each rule in `rules.py` has one, and every finding and
every decision in a report names its level.

- **Locked (chart):** rules every chart must follow (bars from zero, no
  rotated text, a missing value is a gap or a note and never zero, labels
  formatted as a set). They are defaults or refusals; the only way past one
  is `overrides: [{rule, reason}]`, which exists to rebuild the book's bad
  versions and is always reported.
- **Locked (style)** (level `style`): the values one style fixes (font,
  sizes, palette and its order, greys, weights, spacings). They live only in
  the stylesheet; a chart spec has **no field that sets a style value**, and
  another look is another style. The style lint checks each style against
  the locked chart rules (gridlines lighter than every data colour, text
  contrast, a muted and balanced palette).
- **Guided:** decisions the book's procedures make from facts (chart type,
  orientation, sort, zero or truncation, inset or not, direct labels or
  legend, number format, highlight method). The spec gives facts and `auto`
  applies the procedure. The spec may choose explicitly instead; when that
  departs from what the procedure recommends, the report records a guided
  finding (a warning naming the recommendation and its rule), or an info
  when the spec states its reason in `reasons`, keyed by the option's path
  (`"chart.orientation"`, `"highlight[0].dim_others"`). The examples document
  is where guided decisions are illustrated.
- **Open:** choices for the particular chart: the job, the insight and the
  title's wording, highlight targets, comments, category names, width and
  plot aspect within limits (120–600 pt; 0.3–1.2). Never flagged, except
  that the required ones (job, title) must be present.

**Levels come from the guide** (owner, 30 Sep 2026). `GUIDE.md` tags every
rule [L], [S], [G] or [O], and its index of sources lists, for every rule of
the notes, the guide rules that cite it. `rules.py` derives each registered
rule's level from its citers, so the guide and the generator cannot drift.
Where the citers disagree, the registry names the one the generator's use
follows (`via`), with the reason beside it: a refusal of truncated bars
follows G2.6 [L] although G1.57 and G2.8 [G] also cite R2-24; wrapping
follows G6.7 [L] (R5-26), orientation G2.4 [G] (R9-10), the sort G2.1 [G]
(R2-01, R9-9), the highlight's form G4.26 [L] (R3-7) and its method G8.9 [G]
(R15-5), the job's presence G0.10 [L] (R0-07), labelling G5.1 [G] (R4-1),
direct labels G2.18 [G] (R2-32), the magnitude's word G2.23 [G] (R2-06), the
unit phrase G2.19 [L] (R2-07). A split rule's *s* part [S] counts only when
named: the style lint takes it (R1-02 via G3.2s, R5-6 via G6.2s, R5-8 via
G6.5s, R9-8 via G7.1s). Procedures and the generator's own checks, which the
index does not list, carry a level letter. A test checks every derivation.
A stage may still set a finding's level itself: a guided departure is
guided even under a rule whose own level is locked (R2-24 for bars where
dots are recommended, G1.57).

Stage 1 records guided departures for the options it implements: type
(bars where the procedure gives dots, G1.57), orientation, sort, zero,
labelling, magnitude, magnitude style, unit placement, and the highlight
method and paling. Stage 2 adds dots where the procedure gives bars (or a
line), and a lower limit pinned on dots (or lines) against the procedure's
(`scale.min`: R2-24 when the procedure keeps zero, G2.8; R2-26 otherwise,
G2.11); the type decision itself is always recorded as guided (G1.10), and
so is a truncated scale's (G2.8), although R2-24 is locked for bars.

Every rule the generator implements is registered in `rules.py` with its ID,
the stage that enforces it, a default severity and its level. Enforcement
has five forms:

| Form | What it does | Examples |
|---|---|---|
| **Default** | The generator does the right thing unless told otherwise; the rule is implemented, not checked. | no frames, ticks or axis lines (R1-03–R1-05); light gridlines under the marks (R1-06, R1-09); horizontal text (R0-06, R2-08); the unit phrase on the top label (R2-07); nice equal steps (R2-09, R2-10); labels formatted as a set (R2-21, R5-17); nominal categories sorted (R2-01); gaps for missing values (R6-22); direct end labels (R4-1, R6-32) |
| **Refusal** (validation error) | A spec that would misrepresent the data, or that lacks a fact the book says cannot be inferred, is refused with the rule ID, the field path and what to supply. | bars not from zero (R2-24, R9-5); a line through irregular events (R6-9, R6-12) or through sparse data (R6-2); a line across unordered categories (R6-42); a scale beyond a theoretical limit (R2-28); two units on one scale (R2-13); time drawn vertically (R2-37, R6-38); a communication chart without a job (R0-07); a chart whose job is to answer, persuade or make aware whose title does not state the insight (R0-10, G8.2, D-55); a pie without `parts.meaningful_total` (R7-02) |
| **Lint** (report) | Checks on the style, the laid-out model and the SVG; findings carry rule ID, severity (error, warn, info) and element ids. | text collisions; end-label displacement (R6-32); breathing room (R2-27); more than 8 intervals (R2-34); fewer values than scale labels (R2-32, info); palette chroma and balance (R3-2, R3-6); text contrast (R5-8) |
| **Override** | `overrides: [{rule, reason}]` in the spec lets a refusal through deliberately, as R0-04 requires. The finding stays in the report with severity `overridden` and the reason. | the book's "bad" versions rebuilt for comparison (§8) |
| **Guided finding** | An explicit option that departs from the procedure's recommendation: a warning naming the recommendation (`recommended`) and its rule; an info with the spec's `reasons` entry. | sort given for nominal categories (R2-01); bars where dots are recommended (R2-24, G1.57); `magnitude: none` (R2-05) |

`purpose: "analysis"` (R0-02) turns every refusal into a warning. Errors
fail the examples build unless overridden or expected by the variant (2);
warnings and infos are printed under the figure.

### 1.5 Styles, tokens, and why sources are generated per style

A chart style is a stylesheet in the house sense (classes name roles; the
stylesheet gives the look) whose values are written as **tokens**: CSS custom
properties on `:root`, used by role rules through `var()`.

- **Typst's SVG renderer does not resolve `var()`** (checked with Typst
  0.15.1: `fill: var(--c)` renders black). It does support descendant and
  attribute selectors, masks with gradients, `stroke-dasharray` with round
  caps (round dots from zero-length dashes), and `<use>` of a symbol with a
  `viewBox` scaled by `width`/`height`; it does not support `:not()`. So
  `chartgen style` **flattens** a stylesheet before it is baked: it inlines
  `@import`, converts `em` tokens to points (× `--label-size`), evaluates
  two colour functions, `mix(a, b, t)` and `oklch-shift(c, dL, dC)`, and
  substitutes every `var(--x[, fallback])`. The flattened `style.css` has
  only literal values; it is what `bake.py --style` inserts, so bake's label
  lookup (which accepts only literal sizes and `#rrggbb` fills) keeps working.
- **Metric tokens** (sizes, gaps, band fractions, line widths that enter the
  geometry) are read by `chartgen` at layout time; **paint tokens** (colours,
  dash patterns) only by the renderer. Because layout reads the metrics, a
  chart source is generated **per style** (`src/<style>/`), unlike a diagram
  source, which is one file restyled many times. Chart sources are build
  products, regenerated on every build, never edited.
- A style names its mark library in a comment directive, `/* @defs
  plain-defs.svg */`, in itself or in a file it imports; the build passes it
  to `bake.py --defs`.

### 1.6 Changes to shared tools (made in phase 1)

- `tools/bake.py`: read `font-weight` and `font-style` from the stylesheet
  by the same class-order lookup as `font-size` and `fill`, and pass them to
  `typeset` (`#set text(weight: …, style: …)`); factor the lookup into
  `label_style(css, classes, own=None) -> (size, fill, weight, style)`
  (`own`: the element's own values by property; weight and style None when
  regular and upright). `chartgen.measure` calls `label_style` and `typeset`
  so that measurement and baking cannot disagree. Diagrams gain bold and
  italic labels by class. Done in stage 1: `typeset` accepts labels of four
  or six fields and writes weight and style only when a batch needs them,
  so the diagrams' Typst source is unchanged; every diameters source baked
  under `neutral` before and after the change is byte-identical.
- `tools/chartspec.jl` (new): write a Julia `Dict`/`Vector`/`String`/`Real`/
  `Bool`/`nothing` tree as JSON (base only), for generators that emit specs.
  Done in stage 2: `chartspec_json(x)` and `write_chartspec(path, x)`; a
  NamedTuple or a vector of Pairs is an object in its own order, a Dict an
  object with sorted keys (the output is deterministic), a Tuple an array,
  a Symbol a string, `missing` null like `nothing` (in a series, a missing
  value whose reason is unstated), a non-finite number an error. The
  example generator is `examples/gen/days-in-each-month.jl`, which writes
  `specs/gen-01-days-in-each-month.json`; `tests/test_chartspec_jl.py`
  checks the round trip and that the generated spec validates and renders.
- `tools/arrowheads.py` (new; message, 30 Sep 2026): the house arrowhead's
  geometry (`head_size`, `head_path`, `head_outline`, `HEADS`, `NOTCH`)
  moved out of `tools/arrows.py`, which imports it, so that the charts'
  comment arrows share it without the diagrams' ink and shadows (6.12.5).
  Every diameters source baked under `neutral` before and after the move is
  byte-identical.
- Nothing else in the diagram tools changes. The plain style uses no class
  that bake treats specially: the class tokens `card`, `arrow`, `brace` and
  `edge-label` are reserved by bake and never appear in chart sources, and
  `label` is avoided because bake adds it to every baked label group.


## 2. The chart spec

A spec is a JSON object (or the equivalent Python dict). A spec file holds
either one spec or `{"base": spec, "variants": [{"name", "patch",
"expect"}]}`, where each patch is a JSON Merge Patch (RFC 7386) on the base;
variants rebuild the book's alternative versions of one figure. `expect` =
{`refused`: [rule], `findings`: [rule]} states what the variant must
provoke, and the build fails if it does not, so the book's bad versions are
regression tests of the refusals and the lint.

### 2.1 Top level

No field sets a style value (1.4): sizes, colours, weights and spacings are
the stylesheet's. `layout.width` and `layout.plot_aspect` are open choices
within limits.

| Field | Type | Default | Notes |
|---|---|---|---|
| `chartspec` | 1 | required | schema version |
| `id` | string | required | file stem; `[a-z0-9-]+` |
| `purpose` | `communication` \| `analysis` | `communication` | R0-02 |
| `job` | {`kind`, `text`} | required for communication | `kind` ∈ answer, make_aware, persuade, lookup, explain, other (R0-07) |
| `insight` | object | for answer, persuade and make-aware jobs, stated in the title (G8.2, D-55); none for look-up charts | {`important`, `why_care`, `explain`, `action`, `targets`: [ref], `explain_anchor`: ref} (R0-10, RP5-1, R15-1) |
| `title` | {`text`, `kind`, `descriptive`, `link`} | text required | `kind` ∈ insight, question, descriptive (13-15 §5.2); `descriptive`: the line under an insight or question title (what, by what, period with year), in the label style (D-5 (a); stage 1), wrapped like the title onto at most two lines of the figure's width (G6.7, G6.9; round 1, sonnet-G B05): a line two lines cannot hold, or Typst markup wider than the figure (markup is never wrapped), is an error (CG-1); `link`: ref for a title connector (phase 2). The generator never writes prose; it only checks it (R2-12, R5-27). |
| `audience` | object | see 2.6 | |
| `data` | object | required | 2.2–2.5 |
| `facts` | object | {} | the message facts the chooser needs (2.6) |
| `chart` | object | {`type`: `auto`} | 2.7 |
| `scale`, `inset`, `highlight`, `comments`, `comparisons`, `labels`, `layout` | objects | 2.7 | |
| `colours` | {`slots`, `registry`} | {} | palette slots fixed by name, the same in every chart of a set: a map of names and slots, or the document's registry file (2.10; R3-24, G4.22) |
| `transform` | {`kind`, `comparison`, `relative`, `exposure`, `per`, `base`, `at`, `name`, `unit`} | none | a derived measure: deviation, ratio or index (6.12.1; message) |
| `notes` | [{`text`, `on`: [place]}] | [] | notes under the chart (6.19; notes): a footnote, its mark set on each place `on` names (`element`, `category`, `series`, both for a value, `status`, `comment`, `stub`), or a plain note (`on` empty); not `labels.notes`, the words written for values not known |
| `panels` | {`arrangement`, `scale`, `series_labels`} | all `auto` | small multiples and merged charts, guided (6.10; panels) |
| `overrides` | [{`rule`, `reason`}] | [] | 1.4: past a locked rule, always reported |
| `reasons` | {option path: text} | {} | 1.4: why a guided option departs from the procedure (stage 1) |
| `source` | {`book`, `notes`, `shows`} | {} | for the examples document: "p. 49 (s. 0049)", "00-01 F1" |

### 2.2 `data.x`: the dimension along which values are laid out

| Field | Type | Notes |
|---|---|---|
| `kind` | `nominal` \| `ordinal` \| `time` | **required** (R2-01, R2-02, R3-8, R6-42): the order of categories cannot be inferred |
| `values` | [string] | categories in their built-in order (ordinal) or any order (nominal); for time, period strings on the grid: `2022` year, `2022-Q1` quarter, `2022-01` month, `2022-W05` ISO week (or the week's first day, `2022-06-19`, as `week_start` says; phase 3), `2022-01-15` day, `2022-01-15T06` hour, `1960–1969` a bin of years (phase 3). With `grid: none` (time), optionally the window the series covers, [first day, last day] (or two instants), so that its empty stretches show (6.13; time) |
| `names` | {value: text} | the words for a value that is a code, where the chart names it in words: an arrow chart's key, `{"2022-Q1": "Before blitz"}` (6.13; time) |
| `grid` | `year` \| `quarter` \| `month` \| `week` \| `day` \| `hour` \| `bin` \| `none` | **required for time**: regular intervals, one value each (R6-3), or `none` for irregular timestamps, given per series as `points`; `bin` (phase 3), ranges of years, of one length or re-aggregated to one (R2-43, 6.13.11) |
| `week_start` | `auto` \| `monday` \| `sunday` | phase 3: a week grid's first day; `auto` by `audience.locale` (Sunday for a Sunday-first region, en-US; else Monday, ISO 8601) (6.13.11) |
| `title` | string \| null | categorical axis title; null when the names say what they are (R2-03). Ordinal x without a title gets a warning. |
| `roles` | {category: role} | `all_others`, `total`, `average`, `missing` (R3-16; `all_others` sorts last, 02 D1). `missing` (round 1, sonnet-G B04): a category that stands for what is not known, "No answer", "Not known", drawn in the missing grey (G4.15s, #A6A6A6) in every chart with roles (bars, pies, stacks, treemaps, wrapped bars; a table's bars and dots, paled under their numbers), placed after `all_others` and before a total (G0.18, G2.1) |
| `groups` | {category: group} | the group each part belongs to (breakdown, 6.11, 6.11a): a treemap's top level, coloured and labelled by group (R7-16); a pie's parts of a group together, the second in a tint of the first's colour (07 #10) |
| `levels` | `auto` \| [unit…] | time axis rows (6.3) |
| `order` | `auto` \| `fixed` | phase 3 (owner, 1 Oct 2026): `fixed` where the categories' given order must be kept (a table shared by several charts ordered differently): the chooser's sort gives `given` (a guided decision, "the data's order is fixed"), so that bars, stacks and the table under them follow it; `data.parts.order` says the same of the parts (6.18.3) |
| `events` | [{`at`, `label`}] | time: dates marked on the axis, each a thin line across the plot with its label (the storage tank finished, a competitor's opening): something outside the data that the message refers to; `at` a date or an instant (6.13.12) |

### 2.3 `data.measure`: what the values are

| Field | Type | Notes |
|---|---|---|
| `name` | string | what is measured, for reports and legends |
| `kind` | `count` \| `amount` \| `percent` \| `score` \| `quantity` \| `change` \| `index` \| `rank` | `percent` values are given in percent units (45 for 45 %); `change` and `index` get signed labels (6.2) |
| `unit` | object | the unit phrase parts (below) |
| `zero` | `meaningful` \| `arbitrary` | 02 D2 Q2 ("ratio scale"): counts and money are meaningful, °C, dates and rating anchors arbitrary. Default `meaningful` for count, amount, percent; **required** for score and quantity when the chart is not length-encoded. |
| `min`, `max` | number \| null | theoretical limits (R2-26, R2-28); `percent` defaults to 0 and, if `parts.meaningful_total`, 100 |
| `persistence` | `summary` \| `state` \| `event` | time only: each value summarises its interval, lasts until the next (step chart), or is a one-off event (R6-8–R6-12). Default `summary` on a regular grid (with an info); **required** when `grid` is `none`. |
| `cumulative` | bool | false; the unit phrase must say so (R2-11) |
| `good` | `higher` \| `lower` \| null | good direction, for red/green (R3-17, R15-11) and rank order |
| `dividing_point` | number \| {`comparison`: id} \| null | diverging midpoint as the audience sees it (R3-13); without it palettes stay sequential |
| `precision` | number \| null | the finest difference the message needs (R5-9): decides data-label decimals |
| `id`, `heading` | string; text | in `data.measures` of a merged chart: the id its series name, and its panel's heading (a short name, or the question the panel answers; 6.10; panels) |

`unit` parts, assembled into the unit phrase by 6.2 and 6.4:

| Part | Example | Placement |
|---|---|---|
| `prefix` | `"$"`, `"€"` | before the number |
| `suffix` | `"%"`, `"°C"`, `" lbs."`, `" mm"` | after the number (a leading space is part of the suffix) |
| `noun` | `"successful transactions per month"` | runs on after the number: "500 successful transactions per month" |
| `what` | `"weight of product"` | always in parentheses after the number: "0.25 lbs. (weight of product)", "60 (max employees during month)" (D-20, decided) |
| `given_in` | `"thousands"`, `"millions"`, `"billions"`, `"trillions"` | only when the numbers in the data are already written in that unit, as a source headed "£000" is: 1840 stands for 1,840,000. Never a request to scale the labels: the generator hides digits by itself (6.2, G6.11) and `scale.magnitude` overrides that. The opposite reading (sonnet-G B11: monthly sales in pounds, `magnitude: 3`) multiplied every value by a thousand ("£500 million"); the check R5-11 (locked, a warning; G6.12) reports values given in thousands or more that still reach five integer digits, a magnitude word in `noun`, `what` or `suffix` beside `given_in` (stated twice), and such a word beside values of five integer digits; a reason, `reasons["data.measure.unit.given_in"]`, confirms the fact and makes it an info. `given_in` and `magnitude` that disagree are a CG-4 error |
| `magnitude` | 3, 6, 9, 12 | the older spelling of `given_in`, as a power of ten; kept for existing specs |
| `fallback` | string | text of the unit line when it cannot sit inline (6.4); generated if absent |
| `scale_title` | string | the title under a horizontal scale, in place of the derived one ("Donations ($K)", 6.4; stage 1) |

At least one of `noun`, `what` or a non-`%` suffix is expected; a unit phrase
of one generic word gets a warning (R2-11).

### 2.4 `data.series` and values

| Field | Type | Notes |
|---|---|---|
| `id` | string | referenced by `highlight`, `comments`, `targets` |
| `name` | string | the text of its end label or legend entry |
| `values` | [value] | aligned with `x.values` |
| `points` | [[time, value]] | instead of `values` when `grid` is `none` |
| `role` | `regular` \| `average` \| `total` \| `all_others` \| `reference` \| `target` \| `missing` | special roles get special colours, not palette slots (R3-16, R6-34); `missing`: a series that stands for what is not known ("No answer" as a part of a stack, a line or a table row), in the missing grey, last (after `all_others`) |
| `certainty` | `actual` \| `estimate` \| `projection` \| `target` | lower certainty is drawn dotted, nothing else is (R6-24, R6-33) |
| `slot` | integer | fixed palette slot (1–16), for consistency across a document (R3-24); kept in small multiples too, the same in every panel (6.10); by name for a set of charts, `colours` (2.10) |
| `keys` | {dim: value} | small multiples: the series' value of each of `data.dims` (6.10; panels) |
| `measure` | string | a merged chart: the id of its measure in `data.measures` (6.10; panels) |

A **value** is one of:

- a number: a known value; **0 is a known zero** (R6-23);
- `null`: missing, reason unstated (drawn as a gap or a "(missing)" note);
- `{"status": s, "note": text?}` with s ∈ `missing`, `unknown`, `pending`,
  `tbd`, `error`: not known, for the stated reason (R0-12, R9-7); the note, if
  given, is a footnote on the words written for it (6.19; notes);
- `{"value": v, "certainty": c}`: a value of lower certainty (a projection);
- `{"value": v, "note": text}`: a known value with a footnote on its text, its
  value label or table cell (6.19; notes);
- `{"value": v, "complete": false}`: a value for an incomplete period (R2-42);
- a **censored** value (round 1, sonnet-G B10), a state of its own, neither a
  number nor not known: `{"below": L, "note": text?}` or the string `"<L"`, a
  result below a detection limit L ("<1"); `{"above": U}` or `">U"`, above
  the top of a test's range (">2420"). `Value.censored`, its `limit`; it is
  never drawn as a value not known (grey) nor as zero (R0-12, G0.16). A heatmap
  draws it as a step of its own beyond the ramp's end, with its own entry in
  the key (6.15), a table writes "<1" in the cell, aligned as a number (6.16);
  every other type refuses it (R0-12, locked: it would show as a gap or zero)
  until it is designed, the refusal naming the value and the remedy.

`data.as_of` (a date) marks every period ending after it incomplete.
`data.parts` = {`meaningful_total`, `all_parts`, `overlap` (phase 3), `overlaps`: [[part, parent]],
`total`} is required by part-to-whole chart types (R7-02–R7-05) and ignored by
the others. (Stacks, 6.18: `kind`, `nominal` or `ordinal`, the parts' own order when they
are series: ordinal parts are stacked in their order in steps of one hue, R2-02, G4.7; `groups`,
{series: group}, parts of several totals in groups, in shades of a hue per group, G4.9; `order`,
`auto` or `fixed`: fixed where the data's given order must be kept, a table shared by several
charts ordered differently (owner, 1 Oct 2026), the parts and the totals they make then stand in
it.) `data.dims` = [{`id`, `name`, `kind`, `values`}] (small multiples)
and `data.measures` = [Measure with `id`] (a merged chart) are 6.10's (panels).
`data.comparisons` = [{`id`, `kind`, `values`, `label`, `valid`}]
with `kind` ∈ target, budget, tolerance, forecast, average, previous_period,
peer (R15-7–R15-9; phase 2). As built (message): `values` holds one value or
one per x value; `compute: mean` (an average of the plotted data, no
values), `series` (the series compared), `reason` (where it comes from, or
why it is doubtful) and `render` (`auto`, `mark`, `line`, `series`, `none`;
guided) complete it (6.12.2). A comparison given in a unit of its own takes
`unit` (a unit object, as `measure.unit`: "targets in kWh" is `{"suffix": "
kWh"}` against use in MWh) and `factor`, the number each value is multiplied
by to give the measure's unit (0.001); one whose unit differs from the
measure's only in magnitude (£K against £M) is converted without a factor,
and one in another unit without a factor is refused (R2-13, 6.12.2).

### 2.5 The semantics that must be given

The book is explicit that these cannot be read from the numbers.

| Semantic | Field | When absent |
|---|---|---|
| missing versus zero | `null` / `{"status"}` versus `0` | nothing is inferred: a number is a known value |
| regular versus irregular intervals | `x.grid` | time x without `grid`: error |
| lasting versus one-off values | `measure.persistence` | regular grid: `summary`, with an info; irregular: error |
| ordered versus unordered categories | `x.kind` | error |
| parts make a meaningful total; all parts present; overlaps | `data.parts` | error for pie, stacked, treemap, waterfall; ignored otherwise (as built, phase 3: `meaningful_total`, `all_parts` and overlap, `overlap` false or the pairs in `overlaps` (owner, 1 Oct 2026), are required before any of them, refused under R7-02, R7-03, R7-05 when absent, D-1) |
| values that add up over periods | `measure.additive` (phase 3) | unequal time bins refused, not re-aggregated (R2-43) |
| the first day of a week | `x.week_start` (phase 3) | `auto`: by `audience.locale`, Sunday for a Sunday-first region (en-US), else Monday (ISO) |
| meaningful zero | `measure.zero` | see 2.3 |
| theoretical limits | `measure.min`, `measure.max` | none |
| diverging midpoint | `measure.dividing_point` | sequential palette (R3-13) |
| good direction | `measure.good` | no red/green |
| comparison values | `data.comparisons` (+ `valid`) | none drawn; a warning if the insight is about performance (R15-7) |
| highlight targets | `highlight[].targets`, `insight.targets` | nothing highlighted |
| the insight | `insight`, `title.kind` | by the job (D-55, owner 30 Sep 2026): `answer`, `persuade`, `make_aware` (and `explain`, `other` with an insight) must have an insight title, else refused (R0-10, G8.2, overridable); `lookup`, `analysis`, and `explain`, `other` without an insight may have a descriptive title, with no finding |
| lower certainty | `certainty` | actual |
| incomplete periods | `data.as_of`, `complete: false` | complete |
| special categories | `series.role`, `x.roles` | regular; names such as "Other" or "Average" raise an info hint only |
| what the message needs | `facts` (2.6) | the defaults in 2.6 |

### 2.6 `facts` and `audience`

`facts`: what the message needs, for the chooser and the scale.

| Field | Default | Used by |
|---|---|---|
| `message` | time: `pattern`; categories: `compare` | 06 DP6-1 (`pattern`, `periods`, `rank`, `individual_values`); 07 §4 (`compare_parts_precisely`, `fraction_of_total`, `subtotal_amount`, `subtotal_fraction`, `cumulative_subtotals`, `compare_totals`, `compare_parts_across_totals`, `fractions_within_each_total`); phase 3 (the full chooser, 3.2): `lookup_only`, `export_only`, `not_comparable` (a table alone, R10-11), `action_needed` (action dots, R12-4), `stage_conversion` (the funnel's replacement, R12-5), `profiles` (items across attributes, a radar's data: a heatmap table, R12-6) |
| `total_matters` | false, with an info (phase 3) | a total over time: its change must be visible, a total panel or absolute stacks (07 §4.3, R7-25, R7-27, R7-29) |
| `focus` | `periods` when `message` is `periods`, else `pattern` (phase 3) | a total over time: stacked areas for the pattern, stacked bars for particular periods (R7-28) |
| `small_differences_matter` | false | narrow-range inset (R2-23, R9-6) |
| `small_values_matter` | false | outlier inset (R2-20) |
| `distance_to_zero_irrelevant` | false | truncation allowed for non-length marks (R2-24) |
| `exact_values_needed` | false | table with the graph (R2-31, R10-7; phase 2) |
| `location_matters` | false | maps (phase 4): where the values are matters more to the message than reading them (R8-2, R8-3); the chooser routes to a map and reports it not drawn (3.2, step 3) |
| `compare` | [] | small multiples: the dims whose values the insight compares directly, drawn as series in one panel (RP3-4, G1.19; 6.10) |
| `compare_shapes` | false | small multiples: the shapes of series of very different sizes are the message, a scale per panel (R6-37, G7.48; 6.10) |
| `incomplete_value_needed` | false | an incomplete first or last period's value must be shown: drawn apart with an "As of …" note, else left out (R2-42, G2.40, 02 D10; 6.13; time) |

`audience`: {`data_savvy`: false (R0-03, R6-41), `knows_scale`: false (02 D2
Q7), `cvd`: `assume` \| `no` (R3-19), `bw_risk`: false (R6-30: black and
white likely, each line its symbol, 6.13.7, and good and bad textured,
6.12.3), `pie_averse`:
false (R7-40), `locale`: `en-GB` (separators, R5-15), `date_format`: null
(R5-19), `abbreviations`: null \| {`thousand`: "K", `million`: "M", …}
(R5-13)}.

### 2.7 Options

- `chart`: {`type`: `auto` \| `bar` \| `line` \| `dot` \| later types (§7);
  `orientation`: `auto` \| `vertical` \| `horizontal`; `sort`: `auto` \|
  `desc` \| `asc` \| `given`; `sort_by`: series id; `labelling`: `auto` \|
  `direct` \| `legend`; `value_labels`: `auto` \| `on` \| `off` (R2-32; phase
  2); `gridlines`: true}. (Series: `sort_by` defaults to the series the
  insight names, else the first series that is not a reference; `labelling`
  and `value_labels` are guided, 6.7.4, 6.7.12; bars draw value labels, dots
  report `on` as a later stage.) (Time: `type` also `step`, `thin_bar`,
  `arrow`, `bumps`; `change_panels`: `auto` \| `both` \| `absolute` \|
  `relative`, an arrow chart's panels, guided, the absolute panel alone
  refused under R6-17 unless overridden, 6.13.3; `facts.message` also
  `change` (an arrow chart's rows by the change in %) and `relative_change`
  (the % panel alone).) (Stacks, 6.18: `type` also `stacked_bar`,
  `stacked_bar_100`, `waterfall`, `stacked_area`, `stacked_area_100`,
  `line_total`; `labelling: legend` puts a legend column beside upright stacks
  in place of the names beside the last one, a guided finding; a waterfall's
  `orientation: horizontal` is drawn upright, a guided finding, D-43.)
- `table` (series): {`row_key`: `auto` \| `none` \| `swatch` \|
  `swatch_beside` \| `swatch_after` \| `text` \| `pill` \| `halo`;
  `legend`: `auto` \| `keep` \| `drop`}: how a table's rows name the series'
  colours, and whether the legend stays when they do (6.7.13). Guided
  defaults (owner, 1 Oct 2026): `row_key: auto` is `swatch_after` under
  several series (each name, then its square) and `none` under one;
  `legend: auto` drops the legend when the rows are keyed, the table then
  being the key; `keep` includes it.
  (Tables: a table alone, `chart.type: table`, 6.16, adds `names_title` (the heading over the
  series' names), `stub` (columns of text or ISO dates before the names, [{`title`, `kind`:
  `text` \| `date`, `values`: one per series}]), `graphics`: `none` \| `heat` \| `bars` \| `dots`
  \| `trend` (graphics built into the table, guided, R10-9, G7.47) with `graphics_columns`,
  and `stripes`, `bold` (the book's rejected versions, refused under R10-16 and R10-22 unless
  overridden).)
- `chart.encoding` (tables; the notes' name, 03-04 §5.6): `auto` \| `sequential` \|
  `diverging` \| `categorical`, the kind of ramp by which colour encodes a quantity (a heatmap,
  a shaded table), never its colours (the style's; a spec has no field naming a palette, 1.4),
  guided (G4.5, G4.10, G4.11, D-37; 6.14): auto is diverging when `data.measure.dividing_point` gives the value the
  audience sees as dividing, else sequential; diverging without one is refused (R3-13),
  categorical for a quantity too (R3-9), except through an override. `data.actions`
  (tables): {category: `good_strong` \| `good` \| `bad` \| `bad_strong` \| `none`}, each
  item's status for action dots (R12-4, D-1: the thresholds are the maker's).
- `scale`: {`zero`: `auto` \| `include` \| `truncate`; `min`, `max`, `step`:
  null; `steps`: [1, 2, 5]; `allow_2_5`: false; `target_intervals`: 5;
  `precision_value`: null (a value that must be a gridline, R2-33);
  `breathing`: 0.15 (0.10–0.20); `magnitude`: `auto` \| `none` \| 3 \| 6 \| 9;
  `magnitude_style`: `auto` \| `word_on_top` \| `suffix` (D-3: the top
  value in full is never produced, so `full_top_value` is gone; stage 1);
  `unit_position`: `auto` \| `inline` \| `above` \| `below`}. Every option
  with `auto` is guided (1.4). (The wording of the unit is not an option:
  D-20 is decided.)
- `inset`: {`kind`: `none` \| `auto` \| `outlier` \| `narrow_range`;
  `members`: [category] (outlier inset: default all but the outliers);
  `position`: `auto` \| `inside` \| `above` \| `below` \| `right`;
  `share_axis`: false (the main chart drops its category or time labels when
  an aligned inset repeats them, 02 §5.4 d.8; inset stage: `auto`, and the
  inset drops its own: the note that follows); `compact_main`: false}.
  (Inset stage: `kind` defaults to `auto`, like every guided option, so the
  facts decide (6.6); `none` where the procedure gives an inset, or a kind it
  does not give, is a guided finding. `position: auto` puts an outlier inset
  over categories inside the plot, a narrow range above a vertical chart (an
  outlier inset over time below it, its band being low) or beside horizontal
  bars; `above` and `below` are honoured for vertical charts; the others are
  reported (CG-3) and the default taken. `compact_main` halves the
  main plot's height under an inset above or below it. `share_axis` is
  `auto` (default), `true` or `false`, guided (D-56, owner 30 Sep 2026): an
  inset aligned above or below its main chart drops the labels it would
  repeat when space is short (6.6); `true` or `false` against the procedure
  is a guided finding, `true` where the inset is not aligned an info.)
- `highlight`: [{`targets`: [ref], `method`: `auto` \| `strong` \|
  `thickness` \| `bold_label` \| `border` (phase 2: `glow`, `background`,
  `icon`, `selective_label`); `dim_others`: false}] (R15-5, R15-6). The
  method `grey_others` exists only to rebuild the book's rejected version
  and needs an override of R6-28. (Message: `border`, `glow`, `background`,
  `icon` and `selective_label` are drawn by the message layer, 6.12.4; a
  line's stretch is `{series, x: [from, to]}`; `emphasis`: `normal` \|
  `strong` (open) and `glow`: `auto` \| `own` \| `complementary` (guided),
  the glow's hue, 6.12.4.)
- `comments`: [{`text`, `anchors`: [ref], `placement`: `auto`}] (phase 2).
  (Message: `placement` ∈ `auto`, `inside`, `right`, `below`; `answers`, a
  list of `important`, `why_care`, `explain`, `action`, the questions of
  R15-1 it answers; `arrow`: true. 6.12.5.)
- `transform` (message): a derived measure, 6.12.1.
- `labels`: {`notes`: {`missing`: "(missing)", `unknown`: "(unknown)",
  `pending`: "(pending)", `tbd`: "(TBD)", `error`: "(error)", `zero`:
  "(zero)"}}. (Breakdown: `notes.unnamed`, "({n} parts too small to
  name)", a treemap's note; `parts`: `auto` \| `value` \| `percent` \|
  `name`, open, what a pie slice's label gives under its name, 6.11. Also
  `chart.type` `pie`, `donut` (refused under R7-39 unless overridden),
  `treemap`, `wrapped_bar`, and `highlight[].method: explode`, which exists
  only to rebuild the book's rejected version, refused under R7-38.)
- `layout`: {`width`: 368 (the text width); `plot_aspect`: null (style
  default); `height`: null}.

### 2.8 Element references

`{"series": id}`, `{"category": name}`, `{"series": id, "x": value}` (a
point or bar), `{"series": id, "x": [from, to]}` (a segment of a line),
`{"panel": name, …}` (small multiples). Missing targets are a validation
error. On a true time axis (`x.grid: none`) `x` names an event of the series
by its time as written in `points` ("2025-11-13T01:50"), or by its day when
that day holds one event only (several: refused, give the time; 6.13.12). On
small multiples and merged charts the series names the panel; `panel` names
it where the reference has no series (a row's or column's value, a measure's
id or heading; 6.12.5).

### 2.9 Example specs

A column chart with the unit phrase (00-01 F1):

```json
{
  "chartspec": 1,
  "id": "p1-01-headcount-by-region",
  "purpose": "communication",
  "job": {"kind": "answer", "text": "How does headcount compare across the regions?"},
  "insight": {"important": "East has the most employees and North the fewest"},
  "title": {"text": "Headcount by Region, January 2022", "kind": "descriptive"},
  "data": {
    "x": {"kind": "nominal", "values": ["East", "Central", "South", "West", "North"]},
    "measure": {"name": "headcount", "kind": "count", "zero": "meaningful", "min": 0,
                "unit": {"noun": "max employees during month"}},
    "series": [{"id": "headcount", "name": "Headcount", "values": [48, 41, 35, 33, 29]}]
  },
  "chart": {"type": "bar", "orientation": "auto"},
  "source": {"book": "p. 49 (s. 0049)", "notes": "00-01 F1",
             "shows": "pruned non-data elements; the unit phrase on the top label"}
}
```

A line chart with a reference series, projections and packed end labels
(06 T12; the insight and title are ours, corrected in stage 2 (lines):
Central, at 96, already passed East, at 90, in June):

```json
{
  "chartspec": 1,
  "id": "p1-11-line-formatting",
  "job": {"kind": "answer", "text": "Where is each region's headcount heading?"},
  "insight": {"important": "Central passed East in June and is projected to pull ahead in July",
              "targets": [{"series": "central"}]},
  "title": {"text": "Central Passed East in June and Is Projected to Pull Ahead", "kind": "insight",
            "descriptive": "Headcount by Region, Jan–Jul 2022 (July projected)"},
  "data": {
    "x": {"kind": "time", "grid": "month",
          "values": ["2022-01", "2022-02", "2022-03", "2022-04", "2022-05", "2022-06", "2022-07"]},
    "measure": {"name": "headcount", "kind": "count", "zero": "meaningful", "min": 0,
                "persistence": "summary", "unit": {"noun": "employees"}},
    "series": [
      {"id": "east", "name": "East",
       "values": [100, 90, 98, 115, 90, 90, {"value": 110, "certainty": "projection"}]},
      {"id": "central", "name": "Central",
       "values": [90, 91, 85, 93, 82, 96, {"value": 123, "certainty": "projection"}]},
      {"id": "average", "name": "Average", "role": "average",
       "values": [56, 54, 60, 59, 54, 55, {"value": 62, "certainty": "projection"}]},
      {"id": "west", "name": "West",
       "values": [40, 44, 48, 38, 37, 47, {"value": 38, "certainty": "projection"}]},
      {"id": "south", "name": "South",
       "values": [30, 28, 30, 30, 27, 30, {"value": 30, "certainty": "projection"}]},
      {"id": "intl", "name": "Int'l",
       "values": [20, 19, 19, 17, 22, 22, {"value": 20, "certainty": "projection"}]}
    ]
  },
  "facts": {"message": "pattern"},
  "chart": {"type": "line"},
  "highlight": [{"targets": [{"series": "central"}], "method": "auto"}],
  "source": {"book": "p. 218 (s. 0201)", "notes": "06 T12",
             "shows": "end labels packed; dotted projection; average as a reference series"}
}
```

A bar chart with an outlier inset (02 #9, 08-12 #4):

```json
{
  "chartspec": 1,
  "id": "p1-07-transactions-outlier",
  "job": {"kind": "answer", "text": "How do the smaller regions compare with one another?"},
  "insight": {"important": "Central and South each had two to four times as many as West, North or Others"},
  "title": {"text": "Successful Transactions by Region, 2022", "kind": "descriptive"},
  "data": {
    "x": {"kind": "nominal", "values": ["East", "Central", "South", "West", "North", "Others"],
          "roles": {"Others": "all_others"}},
    "measure": {"name": "transactions", "kind": "count", "zero": "meaningful", "min": 0,
                "unit": {"noun": "successful transactions"}},
    "series": [{"id": "tx", "name": "Transactions", "values": [451, 16, 15, 7, 5, 4]}]
  },
  "facts": {"small_values_matter": true},
  "chart": {"type": "bar"},
  "inset": {"kind": "auto", "position": "auto"},
  "source": {"book": "p. 76 (s. 0076); p. 334 (s. 0304)", "notes": "02 #9; 08-12 #4",
             "shows": "outlier inset inside the plot, linked to a frame round the small bars"}
}
```

### 2.10 Colours across a set of charts (R3-24, G4.22 [L]; evaluation round 1, B11)

A thing keeps its colour in every chart of a document. A chart hands out its
palette slots by its own order (lines by their ends, bars and stacks by series
or stacking order, slices by size), so two charts of one set showing the same
centres in another order would colour them differently. The spec fixes the
slots, never the colours, which stay the style's:

- `series.slot` fixes one series' slot, in every layout: lines, bars, stacks,
  pies of several totals, bumps, and small multiples, where the series of one
  label (its values of the dims drawn inside the panels) take one slot in every
  panel (6.10);
- `colours.slots`, {name: slot}, fixes them by name: a series by its name (in
  small multiples by its label, "Elstead Park"), a pie's part by its category
  (one total). A name the chart does not show is passed over, so that one map
  serves every chart of the set; a series' own `slot` comes first (an info when
  the two differ);
- `colours.registry` names the document's registry: a JSON file of the same
  map (`{"Elstead Park": 2, "Brackley Hill": 1}`), its path relative to the
  spec's file (`render(…, base_dir=…)`; the command line passes the spec's
  folder), read under the spec's own `slots`. A registry that cannot be read,
  or that maps a name to anything but a slot 1–16, makes the spec invalid
  (CG-4). The examples' registry is `examples/registry.json` (R1-64 …
  R1-66), outside `specs/` so that the build does not read it as a spec.

The slots not fixed are handed out as before, skipping the fixed ones. Two
series (or parts) of one chart never share a slot: where a map gives two names
one slot, the later takes the next free one, with a warning (R3-24). The
report names the fixed slots (R3-24, or R6-28 in small multiples: "fixed, the
same in every chart of the set").

## 3. Pipeline

### 3.1 Validate

1. Parse against the schema; apply the variant's merge patch.
2. Structural checks: `values` aligned with `x.values`; period strings on
   the declared grid and strictly increasing; one value per slot; irregular
   series given as `points`; percent within its limits; references resolve.
3. Semantic checks (refusals, 1.4): the facts required by the requested or
   chosen type are present (2.5); data inside declared theoretical limits;
   no two measures in one panel (R2-13); unequal time bins (R2-43); a
   communication chart has `job` (R0-07) and a title (R2-12); a chart whose
   job is to answer, persuade or make aware (or to explain or show with a
   stated insight) states its insight in an insight title (R0-10, G8.2
   [L] under that condition, D-55; callouts count once drawn, phase 2).
4. Everything missing but defaultable is filled in, and each default that
   stands in for a fact is recorded as an info in the report.

### 3.2 Choose or check the chart type

`type: auto` runs the procedures of `choose.py`; a given type is checked
against the same procedures, and a disagreement is a guided finding (1.4)
naming the rule and the type the procedure gives (stage 1: not an error; a
given type is refused only when it would break a locked rule). The result
(type, orientation, sort, labelling) and the path of decisions, each with
its rule ID and level, go into the report and into a comment at the top of
the SVG.

Phase 1 procedures:

- time x (06 DP6-1): two periods → arrow chart (phase 2); `enough_values`
  false (06 DP6-2: known fraction < 0.5 or longest run < 3, N) → dot plot;
  regular grid, `summary`: `rank` → bumps (phase 3), `periods` → bars,
  otherwise line; regular grid with several values per slot → error asking
  for aggregation; irregular `state` → step (phase 2); irregular `event`:
  `individual_values` → thin bars (phase 2), otherwise error asking for
  aggregation (R6-10). Never a line for irregular data. As built (stage 2,
  lines; `choose.recommended_type`, `irregular_type`, `line_refusals`): an
  irregular series without `measure.persistence` is refused (R6-1, G0.17
  [L]); a regular one without it is taken as summaries, with an info. A
  line through irregular values is refused under R6-3 (G1.50 [L]), the
  type's departure from the step or thin bars being a guided finding under
  R6-9 or R6-12 (G1.53, G1.54 [G]); events whose pattern is the message are
  refused under R6-3 too, asking for the totals per interval (the decision
  under R6-10), since aggregating is a change of data. A line through too
  few known values is refused (R6-2, locked through G7.18, dots only and
  never a line; the chooser's dot plot is guided, G1.49), across nominal
  categories (R6-42) and, for an audience that is not data-savvy, across
  ordinal ones (R6-41); `orientation: horizontal` on a line is refused
  (R6-38, and R2-37 over time). `purpose: analysis` turns each into a
  warning. The line's rule is R6-4 (G1.51). (Time: the step chart, thin
  bars, the arrow chart and the bumps chart are drawn (6.13); the bumps
  chart is phase 2, not 3. Two periods as bars (`type: bar`, the book's
  clustered bars) are refused under R6-17 (G1.47, G1.61 [L]), the type's
  departure a guided finding under R6-15; a step chart or thin bars of
  several series are reported as a later stage.)
- nominal or ordinal x: parts of a total (`data.parts` given) → 07 §4.1
  (phase 3; until then an error naming the recommended type); otherwise
  bars, or dots when the value scale should not start at zero (the lower-
  limit mode of 6.1 for a non-length mark is a theoretical minimum or
  truncated: G1.57, D-30 (a); stage 1). A line over nominal categories is
  refused (R6-42); over ordinal ones it needs `audience.data_savvy` (R6-41).
- several series of one measure (series; `choose.several_series`): bars
  side by side in clusters (G1.56, R9-11, a guided decision), over
  categories or, when the periods are the point, over time; one series and
  a series of role `target` or `reference`, bars with a flat marker across
  each (G1.60, R11-3; the reference takes no place in a cluster); dots of
  several series share their categories' positions. Over the parts of
  several totals (`data.parts` given, categories not time), 07 §4.2: parts
  compared across totals (`facts.message: compare_parts_across_totals`, the
  default), clustered bars (R7-19); the totals compared, multiple stacked
  bars (R7-20), and the fractions within each, multiple pies (R7-21), both
  phase 3; not every part shown (`parts.all_parts: false`), merged bars
  (R7-22, G1.42): under `type: auto` these are reported as not drawn
  (CG-3), and `type: bar` draws clustered bars with the guided finding.
  Reference series alone, a reference on clustered bars or on dots, more
  than one reference, and an inset on several series are later stages
  (CG-3).
- orientation (6.7), sort (02 D1), labelling (03-04 §4.2). Sorting nominal
  categories is guided: by value, largest first (R2-01, G2.1); an explicit
  other order is a guided finding. Ordinal categories and periods keep their
  built-in order, a locked rule (R2-02, G2.2 [L]): sorting them by value is
  refused unless overridden.

**Phase 3: the full chooser (as built, "(chooser)"; `choose.procedure`,
tests `tests/test_chooser.py`, examples P3-60–P3-79).** One procedure, GUIDE
G1.10's ten steps in D-12 (a)'s order, replaces the separate trees; the
phase 1 and 2 branches are folded in without changing the type of any
earlier example (`tests/fixtures/chosen-types.json`; the one intended change:
P2-04 `compare-totals` now goes to multiple stacked bars by name). Each step
is recorded in the report's `path` (`[{step, name, rule, level, guide,
group, text}]`, in the JSON report and printed by `chartgen choose` above
the decisions); the type decision itself is unchanged in form (`type X:
why`, guided, G1.10). The choice has two stages (G1.8, 05 DP-5): the step
that applies names the group, one of the Part 2 opener's eight (D-13 (a)),
then the type within it. A step that hands the chart on to a type outside its
group moves it (a breakdown's parts compared over two periods: step 4 enters
breakdowns over time, step 5 gives the arrow chart and enters change between
two periods, the path saying "in place of", RP2-8); the last group entered
is the route's.

| Step | Decides (rules) | Group | Types |
|---|---|---|---|
| 0 brief | the job and the insight (R0-07, open) | | |
| 1 table or graph | `message` `lookup_only`, `export_only`, `not_comparable`: a table alone (R10-11, G1.12); else a graph (R10-1), with an aligned table for `exact_values_needed` (R10-7) | tables | `table` |
| 2 variables | panels come from `data.dims`/`data.measures` (6.10; the path of the first panel's type follows); more than 30 series in a plot: a heatmap (R6-36, G1.20, D-28) | data over time; tables | `heatmap` |
| 3 location | `location_matters`: a map (R8-2), over time lines on regions (R8-13); phase 4, reported not drawn | maps; maps changing over time | `map`, `map_lines` |
| 4 parts of a total | when `data.parts` is given or the message is 07 §4.0's: see below | breakdowns of totals; … changing over time | `pie`, `stacked_bar`, `stacked_bar_100`, `treemap`, `wrapped_bar`, `waterfall`, `stacked_area`, `stacked_area_100`, `line_total`; bars on to step 7, lines on to step 6 |
| 5 two periods | an arrow chart (R6-15); a rank message over at most 10 categories, a slope chart (R6-16, the book's exception); two series named as periods across categories, the same route (below) | change between two periods | `arrow`, `slope` |
| 6 over time | the time tree of phase 2, unchanged (06 DP6-1, DP6-2) | data over time | `line`, `bar`, `dot`, `bumps`, `step`, `thin_bar`, `aggregate` |
| 7 across categories | bars, or dots when the scale need not hold zero (G1.57); clustered bars for several series (R9-11); more than 40 nominal bars, wrapped (R7-14); `action_needed`, action dots (R12-4); `stage_conversion`, the funnel's replacement (R12-5); `profiles`, a heatmap table (R12-6); `pattern` over ordinal categories (08-12 DP-2's first question), a line for `audience.data_savvy`, else bars in the categories' order (R6-41, G1.58) | bar charts (tables for `profiles`) | `bar`, `dot`, `wrapped_bar`, `action_dots`, `funnel_bars`, `heatmap`, `line` |
| 8 combos | one mark type, series told apart by colour (R11-2, locked); a reference series another mark (R11-3) | | |
| 9 avoid-list | the result is never on it (R12-1, locked); an unfamiliar type is taught (R6-18, G1.5, G1.6) | | |

Step 4 in detail (07 §4.1–4.3). **The four checks** come before any pie,
stacked chart, treemap or waterfall, in the tree's order, each a locked step
of the path (G0.12–G0.15): the values add up to a total someone would quote
(`parts.meaningful_total`, R7-02), every part is shown (`parts.all_parts`,
R7-03), no part contains or overlaps another (`parts.overlap` or
`parts.overlaps`, R7-05), the signs are not mixed (computed, R7-04). For
several totals and over time they hold for every total and every period
(D-25 (a)). A failure routes: one total, a plain bar chart (mixed signs with
`cumulative_subtotals`, a waterfall, R7-11; overlaps, a bar chart with D-24
(b)'s better answer, disjoint parts or each parent stacked from its
sub-parts, named in the path, since making parts disjoint is a change of data
the generator does not make); several totals, clustered bars; over time, a
line per part. When a check's fact is missing and the route leads to one of
those types, the chart is refused asking for it (D-1 (d): a guess could
misrepresent); overlap is such a fact too (owner, 1 Oct 2026): `overlap`
false, or the pairs in `overlaps`, else refused under R7-05. A
type asked for explicitly is checked the same way: `chart.type: pie` on
averages is refused under R7-02 (overridable, to rebuild the book's bad
versions), and so on for each check. Then by the message: one total,
`compare_parts_precisely` (the default, with an info, D-1) a bar chart
(R7-06; more than 40 parts wrapped, R7-14); more than 7 parts with a
fraction or subtotal message, a treemap (R7-15; the path names lumping into
All others, R7-17, which is the maker's); 7 parts, a pie flagged with a
guided warning (D-23 (a)); `subtotal_amount`, `cumulative_subtotals`, a
stacked bar (R7-08); `fraction_of_total`, `subtotal_fraction`, a pie
(R7-07), or a 100 % stacked bar for `audience.pie_averse` (R7-40, D-39).
Several totals (x the totals, the series their parts, as P2-04):
`compare_parts_across_totals` (the default) clustered bars (R7-19), not
every part shown merged bars (R7-22, drawn through `data.dims` or as
clustered bars with the finding, as in phase 2), `compare_totals` multiple
stacked bars (R7-20), `fractions_within_each_total` multiple pies (R7-21) or
100 % stacked bars when pie-averse (D-39); more than 7 parts, clustered
bars. Over time: `total_matters` and `focus` (2.6) with the message give a
line per part (R7-26, on to the time tree), lines with a total panel
(R7-27), absolute stacks (R7-29) or 100 % stacks (R7-30), as areas for the
pattern and bars for periods (R7-28).

**The avoid-list** (`choose.AVOIDED`; G1.61 [L], 08-12 DP-7): `chart.type`
`donut` (R7-39: a pie), `dual_axis` (R2-14: a merged or index chart),
`radar` (R12-6: a heatmap table), `funnel` (R12-5), `word_cloud` (R12-2),
`packed_bubbles` (R12-3: sorted bars), `bullet` (R12-4: action dots),
`choropleth` (R8-5: dots, bubbles or bars on regions), `3d` (R12-7, R12-8)
and `smoothed_line` (R6-39) are refused, the replacement named in `supply`
and in the path's step 9; an override lets the request through, drawn only
where a layout exists (the donut, P3-10). A slope chart asked for outside
its case is refused under R6-16 (locked through G1.61); in its case the
chooser gives it, drawn by the line layout (6.13.10). Other types asked for
are checked where the data cannot carry them: an arrow chart of other than
two periods is refused under R6-15 (its arrows would join only the first
two; overridden, it is not drawn), and lines with a total panel take a line's
refusals (R6-2, R6-3, R6-42). A map's CG-3 note is short ("maps are phase
4 …; the path says why"), the reason being in the path.

**Two periods as bars** (G1.46 [G], G1.47 [L], G1.61; round 1, B08: both
generator arms found that `facts.message: periods` opened the way to
clustered columns with no reason asked, and sonnet-G fell into the trap).
Step 5 says plainly what the default is: an arrow chart, each category's
change in units and in %, or a chart of the change itself (the % panel alone,
`facts.message: relative_change`; or the change as bars, a previous_period
comparison with `transform: deviation`) (`choose.TWO_PERIODS_DEFAULT`). The
guide's exception, each period's own values as the message (G1.51,
`facts.message: periods`), stands only with its reason, grounded in the
request, in `reasons["facts.message"]`: the path records it at step 5 and the
report shows it (R6-15, guided, an info with the reason); without one the
route stays the arrow chart, a guided warning naming the default, and bars
asked for are refused (R6-17). The same holds for the other form of the data,
the categories along x and two series whose names are periods ("2019",
"2025"; "2024-Q1"; "FY2024"; "Mar 2025"; `choose.period_series`): the route
is the change between two periods, and clustered bars of them, or an arrow
chart, which needs x the two periods and a series per category (a reshaping
the generator does not make), are refused under R6-17 with that remedy;
dots of them keep the type's guided warning (dots are not on the
avoid-list).

**Drawn or not.** A type is drawn once its layout is registered (`LAYOUTS`)
or it is named in `choose.STAGE_TYPES`; until then the chart is reported as
a later stage (CG-3). Sorting: a waterfall (the process's own order,
D-43), a table and a heatmap (the order the spec gives, arranged for the
message: G1.17, G7.35, G7.36) and the funnel's replacement (its stages,
G7.59) keep the given order (`choose.GIVEN_ORDER`, a reason per type); an
explicit other order is a guided finding naming that reason.

### 3.3 Scales

For each value scale: the lower-limit mode (6.1: zero, theoretical minimum,
truncated), the limits and step (02 D3–D5, exact arithmetic), then the label
set (6.2: magnitude, decimals, prefix and suffix, signs), then the unit
phrase parts. Inset scales (6.6) and time axes (6.3) are computed here too.
Lint needs the unscaled candidates, so the scale records every step it
rejected and why.

### 3.4 Layout

1. **Collect texts.** Every string the layout might set, with its role
   classes: scale labels, the top line's parts, the unit line's one- and
   two-line forms, each category name and all its two-line splits, time
   labels in full and narrow forms, end labels, notes, title lines.
2. **Measure** them in one batch (`measure.py`): advance width, ascent and
   descent at the size, weight and style that bake will use
   (`bake.label_style`). A JSON cache keyed by (mode, code, size, weight,
   style, Typst version) makes rebuilds cheap. A later step that needs a new
   string triggers a second batch.
3. **Stack the figure vertically**: title lines (wrapped to the width, at
   most two lines), legend row if any, the band of the top scale line, the
   plot, category or time rows, category axis title, notes. Horizontally:
   the scale-label column, the plot, the end-label column or legend.
4. **Place by type** (bars 6.7, lines, dots), then the unit phrase and its
   fallbacks (6.4), end labels (6.5), missing notes (6.8), highlights (6.9),
   the inset (6.6).
5. **Collision pass.** Every text box is tested against every other (they
   must stay `--text-pad`, 0.15 em, apart: each box is padded by half, so
   two lines at the normal pitch pass) and against the marks; the parts of
   one label (the top number and its unit phrase, marked by a shared
   `joint`) are not tested against each other. The remedies are those of
   each algorithm (wrap, fallback line, packing, thinning, orientation
   change, legend); an unresolved collision is a lint error (CG-1), never an
   overlap left silently.

Every label is placed by its **baseline**, not by `dominant-baseline`:
numbers and one-line texts centred on a line y get baseline y + h/2, where h
is the typeset height of the digit "0" (0.666 em in New Computer Modern) or
of "H" for texts (0.683 em); so labels with and without descenders sit
alike.

### 3.5 Emit

`emit.py` walks the model and writes the house format (§4): coordinates
rounded to 0.01 pt, `<text class="prose …">` placeholders holding Typst
markup escaped by `typst.py` (below), empty `<style id="house"/>` and
`<defs id="house-defs"/>` for bake, the chart's own clip paths and masks in
`<defs id="chart-defs">`. Output is deterministic.

`typst.py` escapes plain strings for Typst markup: backslash before `\ # $
* _ @ < > [ ] ` ~ /` (so `$300,000` becomes `\$300,000`, `//` cannot start a
comment and `~` is not a non-breaking space), and before a leading `-`, `+`
or `=` that would start a list or heading; a double hyphen is written `-\-`
so that it does not become a dash. Quotes are left to Typst's smart quotes
("Int'l" becomes Int’l). All of these were checked with Typst 0.15.1. A
spec string may instead be `{"typst": "…"}` to pass Typst through unchanged
(for mathematics).
Negative numbers use U+2212, never a hyphen. XML escaping is the writer's.

### 3.6 Lint

Three levels, one report:

- **Style lint** (once per style, `chartgen style`): non-data strokes of
  contrast ≤ 1.5 against the page and below every data colour (R1-02; the
  signed zero line of R7-12 and comment connectors are exempt);
  text contrast ≥ 4.5 (R5-8, O); regular palette slots: OKLCH chroma ≤ 0.15
  (R3-2, N), lightness spread ≤ 0.12 among slots 1–5 (R3-6, N), no grey,
  black or red in them (R3-16, R3-18); CVD distance ≥ 0.06 between slots
  1–5 under the Machado matrices (R3-19, 03-04 §5.7, N); the labelling
  typeface tabular and lining (05 DP-1).
- **Model lint** (per chart, with the geometry): the checks listed in §7
  for the phase's types, e.g. breathing room 5–25 % (R2-27), ≤ 8 intervals
  (R2-34), values fewer than scale labels (R2-32, info), end-label
  displacement ≤ 1 pitch (R6-32), text boxes disjoint, ≤ 4 distinct text
  styles in use (R5-6), at most 5 lines unless the crossing test passes
  (R6-35), a year in the title of time-bound data (R2-12), every text within
  the figure, none past its left or right edge or above its top (CG-1, an
  error naming the text and how far it runs over; round 1: an overlong line
  had been clipped off the canvas unreported).
- **SVG lint** (any SVG in the vocabulary): no `chart-frame`, `plot-frame`,
  `tick`, `axis-line`, `mark-outline` (R1-03–R1-05); gridlines on the value
  scale unless value labels replace it (R1-06); every gridline before every
  mark in document order (R1-09); no `transform` with rotation on text
  (R0-06); every zero-length or unknown bar has a `missing-note` (R9-7).

**The SVG lint as built (stage 2)**, `chartgen/svglint.py`. It resolves the
look as it will be drawn: the style given stands in for `<style
id="house">`, as `bake.py --style` puts it there, and any other `<style>`
in the file follows it; marks take the CSS cascade (inline style over rules,
by specificity and order, over presentation attributes; paint inherited),
labels bake's own lookup (`bake.label_style`); text extents come from Typst
through the measurer's cache, or from the model for chartgen's own charts.
It runs in two places: `chartgen lint` on any file, and as the last model
check (`lint.svg_rules`, in `lint.CHECKS`) on every chart chartgen draws,
where the checks the model lint already makes are skipped, and an override
or `purpose: analysis` settles a finding as it settles a refusal (a finding
of a rule the spec has already overridden is not repeated). Severity: error
for a locked rule broken outright; warn for a locked rule tested against a
provisional number (D-6, D-34), for a guided rule, and for a style value;
info for what the lint cannot read. R1-02, R5-6 and R5-8 are reported at the
level of their locked rules (G3.2, G6.2, G6.5), since a drawing shows the
chart, not the style's values. "(model)" marks the checks skipped on
generated charts.

| Check | Rules | G9 | What it reads |
|---|---|---|---|
| frames | R1-03, R1-04, R1-05 | 37 | `chart-frame`, `plot-frame`, `mark-outline`, `tick`, `axis-line`; a visible outline on a bar, slice, area or cell |
| gridlines | R1-06, R1-09, R1-07 | 38 | two or more gridlines where marks sit on a value scale (unless every value is labelled); no gridline or divider drawn after a mark it crosses; gridlines across the direction of "more"; the zero line like the others unless `signed` |
| non_data | R1-02 | 37, 38 | gridlines, dividers, inset frames and links: ≤ 1.5 : 1 on the page, lighter than every data colour in the chart, gridlines ≤ 0.1 em wide |
| text_orientation | R2-08 | 54 | a rotated, skewed or mirrored text or group; `rotate`; a vertical writing mode |
| text_size | R5-7 | 55 | at final size (`--final-width`): labels ≥ 9 pt, notes and inset labels ≥ 0.8 em |
| text_contrast | R5-8 | 55 | each text ≥ 4.5 : 1 against what lies under its centre, the page or a mark's fill (G7.4) |
| collisions (model) | CG-1 | 51 | text boxes `--text-pad` apart; no text over a bar or dot (value labels, and an inset's texts against the main chart it covers, excepted) |
| text_styles (model) | R5-6 | 53 | at most four styles (size, colour, weight, slant) |
| subtitle | R5-27 | 5, 53 | the line under a title set in the label style (D-5 (a)) |
| colours | R3-21, R3-22, R3-2, R3-16, R3-18, R3-6, R3-19 | 42, 43, 46 | the regular colours in use (not the reserved greys, highlight or context shades, gradients): nine or more an error, or a guided warning when every series is labelled directly; eight with a legend a warning; chroma ≤ 0.15; no grey, no red; the series' first-tier colours spread ≤ 0.12 in OKLCH lightness; ≥ 0.06 apart in OKLab under protanopia, deuteranopia and tritanopia (Machado 2009; a warning with a legend, an info when labelled directly), good against bad too, an info (their semantic red and green kept for every reader, black and white telling them apart without colour, D-18; none where a texture or a filled dot tells them apart); the special categories grey |
| series_colours | R3-4 | 40 | one colour per series: no hue per bar, no shading by value, no gradient |
| highlight | R15-6, R6-28 | 7, 48 | a highlight in its own hue (within 25° of the series' other marks) and, for a bar, its own width; `uniform-context` |
| empty_bars | R9-7 | 58 | every category slot has a bar or a note, and every bar of no length a note |
| bars_from_zero | R2-24 | 22, 69 | main-chart bars based on the zero gridline, or on the 0 of the scale read from gridlines or labels |
| scale_labels | R2-21, R5-14, R2-10, R2-09, R2-18, R2-19 | 27, 30, 32, 56 | one number of decimals and one form (the top label may add the unit, D-3); four or more integer digits grouped; equal steps of 1, 2 or 5 × 10ⁿ (2.5 warns); positions linear in value (a break, or a log scale) |
| scale_count (model) | R2-34, R2-05 | 28 | at most nine labels; no label with four trailing zeros |
| unit_phrase | R2-07 | 31 | a unit phrase, unit line or scale title for every value scale; an inset's scale only if needed (G2.27): bare numbers pass where the main scale's unit writes no currency, magnitude or suffix, and are warned about where it does (inset stage) |
| percent_labels | R2-21 | 30 | a scale whose title or unit says "(%)": every label carries "%", horizontal scales too, an error where one is bare, a warning where the labels carry it and the title repeats it (owner, 1 Oct 2026: G2.24 over G2.19) |
| insets | 6.6, R9-6 | 23 | each `inset` group: one `inset-source`, one `inset-frame`, two `inset-link`s, drawn links first, then the source frame, the frame and the inset's panel; the frame covers no mark or text of the main chart (the collision check skips pairs across an inset and the main chart); the links cross no text or bar or dot, the frames' outlines no text; every text of the inset carries `inset-text`; a truncated inset's bars (its lowest gridline above 0) under a masked `inset-fade` group, an inset from zero with none (G2.27–G2.29; inset stage) |
| flat | RP6-1, R14-1 | 20, 39 | no filter effect, no fill behind the plot or page, no animation |
| dates | R5-20, R5-21 | 57 | no numeric month (01/2022, 2022-01) or two-digit year (’22) in any text, unless the audience has its own format |
| title_year (model) | R2-12 | 5 | a chart over time names the year in its title block |
| vocabulary | CG-5 | – | classes outside §4.3, texts bake does not typeset (info) |
| line_shapes (time) | R6-39, R6-40 | 61 | no curve command in a line, projection or step line; a bumps chart's curves flat at their points (6.13.8) |
| dashes (time) | R6-31 | 48 | a dash pattern only on a projection, reference, target or incomplete mark |
| shared_colours (time) | R6-28 | 48 | no two regular lines of one colour, except with their own marker symbols; small multiples and merged charts excepted |
| legend_order (time) | R4-5 | 50 | a line chart's legend entries (`legend-entry`, `data-series`) in the order of the line ends |
| empty_dots (time) | R9-7 | 58 | every name of a categorical dot plot with its dot or a note |
| time_ticks (time) | R1-04 | 37 | `time-tick` only on a step or thin bar chart (D-45 (ii)) |
| time_label_dividers (time) | CG-6 | 34 | every time label (row 1 and row 2) clear of every divider by `--divider-clear`, or with its halo (a `text-halo` copy in a `divider-halo` group, after the divider, before the label) (6.3.8) |
| event_bars (time) | R2-24 | 22 | every thin bar from zero, or stacked on the event before it at its instant |
| footnotes (notes) | CG-8 | – | every mark in the chart with its note in the footer, every footnote's mark set somewhere above, footnotes only in the footer, the notes below everything else (6.19); marks and notes take the notes' floors (text_size, text_contrast) and are one label with the text they follow (collisions) |

**Where each check of G9 is made** (stage 2; `GUIDE.md` G9 numbers). A
check may be made in more than one place: a refusal before drawing, a
default of the layout, and a lint of the result.

| Where | G9 checks (rules) |
|---|---|
| Refusal (validation, `_semantic`, the chooser, the scale) | 1 (R0-07), 3 (R0-10), 7 (R15-6: `thickness` on bars, a bigger dot), 13 (R6-3, R6-42, R6-41), 22 (R2-24, R9-5), 25 (R2-28), 27 (R2-09, a custom step), 34 (R2-37, R6-38; periods on their grid), 36 (R2-02), 38 (R1-06, `gridlines: false`) |
| By construction (no spec field or type allows it) | 16 (no avoided type in the schema), 18 and 33 (one measure per chart), 32 (no broken or log scale), 39 (flat marks), 61 (paths are straight segments) |
| Procedure, guided (a decision; an explicit departure is a guided finding) | 2 (a default standing in for a fact is an info; `chartgen choose` lists them), 9, 14 (drawn in phase 2), 23 (the inset too, inset stage), 24, 26, 28, 35, 49 |
| Layout default | 6 (R15-5), 30 (R2-21), 31 (6.4), 34 (R2-39, R2-42), 37, 38, 40 (R3-3), 42, 48, 50 (R4-5), 54 (R5-26, R9-10), 56 (the formatter), 57 (month names), 58 (R9-7), 59 (R6-22), 60 (R6-25–R6-27) |
| Model lint (`lint.CHECKS`) | 5 (R2-12), 7 (dots of one size), 21 (R6-35), 26 (R2-27), 28 (R2-34), 29 (R2-32), 31 (R2-11), 51 (CG-1, R6-32), 53 (R5-6); time: 59 (R6-22), 60 (R6-25–R6-27, R6-30), 61 (R6-40), 13 (a thin bar per event, an arrow per panel) |
| SVG lint (the table above) | 5, 7, 20, 22, 23 (the inset's form, inset stage), 27, 28, 30, 31, 32, 37, 38, 39, 40, 42, 43, 46, 48, 51, 53, 54, 55, 56, 57, 58, 69 |
| Style lint (`style.lint_style`) | S1 (R1-02: colours), S3 (chroma, spread, grey), S8 (text colours) |
| Left to a human (the checker of `CHECKING.md`) | 4, 8, 67, 68, 70, 71 (every stated fact recomputed from the data, G8.27), 72 beyond the marks the SVG lint pairs (that nothing the reader needs is left to the message, G6.24); whether the words of 3 and 5 state the insight and name the measure and breakdown; whether the precision of 28 suits the job; the reading of 69 beyond bars from zero and gridlines |
| Later stages and phases | 1 (a question-titled set), 10–12, 15, 17, 19, 41, 44, 45, 47 (a document registry, §9 Q9), 52, 53 (comments), 62–66; style checks S2, S4 (made per chart by R3-19), S5–S7, S9, S10 |

Report format:

```json
{"chart": "p1-16-glycemic-index", "style": "plain", "variant": "alphabetical", "type": "bar",
 "status": "drawn",
 "decisions": [{"rule": "02 D5", "level": "guided", "text": "scale 0–80 by 20", "guide": ["G2.15"]}],
 "findings": [{"rule": "R2-01", "severity": "warn", "level": "guided", "path": "chart.sort",
               "message": "chart.sort given departs from the procedure, which gives desc (…)",
               "recommended": "desc", "guide": ["G0.18", "G2.1"], "title": "…"}],
 "overrides": [], "expect": {"findings": ["R2-01"]}, "expect_met": true,
 "source": {"book": "p. 54–55 (s. 0053–0054)", "notes": "02 #1", "shows": "…"},
 "summary": {"scale": "0–80 by 20", "labels": ["0", "20", "40", "60", "80"], "top": "80 glycemic index",
             "orientation": "vertical", "width": 368, "height": 246.45}}
```

`status` is `drawn`, `refused` (a locked rule, no SVG), `invalid` (the spec
is malformed, CG-4) or `unsupported` (a type or feature of a later stage,
CG-3). Rule IDs beginning with CG are the generator's own checks: CG-1 text
boxes disjoint, CG-2 the unit line on at most two lines, CG-3 not drawn in
this stage, CG-4 the spec well formed, CG-5 the drawing written in the chart
vocabulary (stage 2, the SVG lint; an info), CG-6 every time label clear of the
time axis's dividers (time, 6.3.8; an error).

### 3.7 Bake

`bake.py --style build/<style>/style.css --defs style/<defs>.svg src.svg
out.svg`, unchanged except for 1.6. Nothing in the plain style casts a
shadow or arches: the plain charts are flat on the page. The one texture is
the black-and-white mode's (6.12.3): flat `<pattern>` fills the generator
computes into the chart's own defs, which bake passes through.


## 4. The SVG output

### 4.1 Skeleton

```xml
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"
     viewBox="0 0 368 262" width="368" height="262">
  <!-- p1-07-transactions-outlier: GENERATED by chartgen from specs/p1-07-….json under style plain; do not edit.
       type bar (02 D1 sorted; R2-20 outlier inset) -->
  <title>Successful Transactions by Region, 2022</title>
  <desc>Central and South each had two to four times as many as West, North or Others</desc>
  <style id="house"/>
  <defs id="house-defs"/>
  <defs id="chart-defs"> clip paths, fade masks </defs>
  <g class="chart bar vertical categorical" id="chart">
    <g class="header"> title lines, legend </g>
    <g class="panel" id="main">
      <g class="grid"> gridlines </g>
      <g class="marks"> <g class="series single" data-series="tx"> bars </g> </g>
      <g class="scale value"> scale labels, top line, unit line </g>
      <g class="scale category"> category labels, axis title </g>
      <g class="annotations"> missing notes, end labels </g>
    </g>
    <g class="inset" id="inset-1"> links, source frame, frame, <g class="panel inset-panel"> … </g> </g>
    <g class="comments"> comments and their arrows (phase 2) </g>
    <g class="footer"> notes under the chart: its own, the footnotes, the plain notes (6.19) </g>
  </g>
</svg>
```

Marks carry `data-series`, `data-x` and `data-value` (the lint and tests read
them; an interactive version could too). Ids are derived from spec ids.

### 4.2 Drawing order

1. gridlines, zero line, time dividers (under everything, R1-09);
2. context marks, then regular marks, then highlighted marks (a highlighted
   line lies on top), then reference series, then point markers;
3. value labels, scale labels, top line and unit line, category and time
   labels, end labels, missing notes;
4. inset: links, then the source frame (stroke only), then the inset frame
   (filled with the page colour, hiding what lies under it), then the inset
   panel in the same order 1–3; its marks group is clipped to its plot
   (`clip-path`, a `clipPath` in `chart-defs`) only where a mark leaves it,
   and a truncated inset's bars group is masked (`mask`, the fade) (inset
   stage);
5. comments and their arrows, title connector (phase 2).

### 4.3 Role classes

Classes name roles, never looks. A mark carries its kind, its colour slot or
role, and its emphasis state; the stylesheet combines them.

| Group | Classes |
|---|---|
| chart and structure | `chart` + type (`bar`, `line`, `dot`, …) + `vertical`/`horizontal` + `time`/`categorical` (the kind of x); `header`, `footer`, `panel`, `inset-panel`, `grid`, `marks`, `scale value`, `scale category`, `time-axis`, `annotations`, `legend`, `comments`; several panels (6.10; panels): the chart classed `multiples` or `merged`, its panels `panel` groups with ids `panel-i-j`, a `headers` group |
| non-data | `gridline`, `gridline zero`, `gridline zero signed` (a zero line inside a signed range, 07 R7-12), `time-divider`, `panel-divider` (between small multiples, 6.10; panels); absent by default and emitted only through overrides: `chart-frame`, `plot-frame`, `tick`, `axis-line`, `mark-outline` |
| marks | `bar`, `line` (a polyline of known values), `projection` (its dotted continuation), `point` (a marker on a line), `dot` (a dot-plot mark; filled or hollow is the stylesheet's choice); later `segment`, `slice`, `area`, `change-shaft`, `change-head`, `change-start`, `step-line`, `event-bar`, `target-mark`, `reference-line`, `cell` |
| colour slot or role | `single` (one-colour chart, R3-3), `series-1` … `series-16` (palette slots), `average`, `total`, `all-others`, `reference`, `missing`, `good`, `bad`; later `seq-1` … `seq-9`, `div-m4` … `div-0` … `div-p4`. On a line (stage 2, lines), `total` and `reference` take the average's dark grey (R6-34, D-21 (c)); a series of role `target` is drawn as `reference`, dotted; a sole regular line takes `single`, the single-line colour |
| emphasis | `highlight` (a target), `context` (non-targets, only when something is highlighted and `dim_others` is set); `uniform-context` (non-targets drawn alike, the book's rejected grey-out of R6-28) exists only through an override |
| inset | `inset`, `inset-frame`, `inset-source`, `inset-link`, `inset-fade` (the masked group of faded bars) |
| text (all `<text class="prose …">`) | `title`, `subtitle`, `scale-label`, `unit-phrase` (the words after the top number), `scale-title` (the title under a horizontal value scale), `unit-line` (the unit phrase on its own line), `category-label`, `axis-title`, `time-label`, `time-label-2` (second row), `end-label`, `value-label`, `missing-note`, `legend-text`, `comment`, `panel-title`, `row-header`, `column-header` |
| text modifiers | `text-strong` (bold: labels of highlighted and reference series, R6-34), `inset-text` (inset size) |
| legends and leaders (stage 2, lines) | `legend-key` (a line's key: a short stroke of its width and colour classes, `projection` added for a dotted series), `leader` (a thin grey line from a line's end to its moved label) |
| legends, markers and tables (series) | `legend-entry` (a group per entry, key then name, carrying `data-series`), `legend-swatch` (a bar's square key, with its colour class), `dot-key` (a dot's key: the dot itself, open or filled as the chart's), `target-key` (a flat marker's key); `target-mark` with `reference` and the bar's colour class (a flat reference marker across a bar, drawn in the strong shade of that colour, or the dark grey for a grey bar), and with `secondary` (a second reference's mark, a `<path>` stroke as wide as the bar, dotted in the dark grey, 6.12.2); `table` (the group of a table with the graph), `table-cell` (its numbers), `row-header`, `column-header` |
| the message layer (message, 6.12.6) | `comments` (the group of comments, their arrows and the title connector), `comment-arrow` and `title-link` (a `<path>` of one cubic bend, running into its head's notch) with `comment-arrow-head` and `title-link-head` (the house arrowhead, bent along the curve, filled in the arrow's colour), `underlay` (the panel's first group: `glow`, a flat halo under a bar or dot, `glow-line`, a flat stroke under a line's stretch, `highlight-band`, a band behind a category or stretch), `overlay` (after the marks: `reference-line` with `average` or `reference`, `border` with the slot and `highlight`), `icon` (a highlight's marker), `cvd-safe` (on the chart's root, under a style that chooses the colour-blind-safe pair: that pair for `good` and `bad`, D-18), `textures` and `texture` (in black and white: a good or bad area mark's shape again, painted with the chart's two-tone pattern, 6.12.3), `filled` (in black and white: a bad dot filled); text modifiers `text-good`, `text-bad`, `text-good-cvd`, `text-bad-cvd` (a caption naming a side, in its colour) |
| time charts (time, 6.13) | the chart classed `step`, `thin-bar`, `arrow` or `bumps`; marks `step-line` (a stretch of known states, a `<path>` of level and vertical segments), `event-bar` (a thin bar; `data-x` its instant, `data-stacked` on an event standing on another at the same instant), an arrow's `change-start` (a `<use>` dot), `change-shaft` and `change-head` (`<path>`s; `data-from`, `data-to` its values), `arrow-halo` (a page-coloured copy of a short change's head under it, where the head is drawn over its start dot, 6.13.4), `incomplete-mark` (an incomplete period's dotted level mark on bars, a `<line>` with `data-complete="false"`; on a line the partial value is a `point` with that attribute); `time-tick` (a true time axis's tick, in the `time-axis` group); `points` (the group of a line chart's point markers, and a bumps chart's end dots, after the lines, 4.2); `legend-entry` on a line chart's legend too; texts `arrow-key` (an arrow chart's key, in the note style) and `incomplete-note` ("As of Dec. 10"); `events` (the group of the dates marked on the axis, over the gridlines and under the marks), `event-line` (a `<line>` across the plot at a marked date, `data-x` its date as given) and the text `event-label` (its label, in the note style), 6.13.12 |
| tables and heatmaps (tables, 6.14–6.17) | the chart classed `table` (with `graphics` when it has some), `heatmap`, `action-dots` or `funnel-bars`; a table's `table-rule` (under the header) and `table-rule-total` (above the totals), `table-fill` (a highlighted cell's light fill), `zebra` (only through an override, R10-16), all in an `underlay` group or the `table` group; a heatmap's or shaded table's `cell` (a `<rect>`, `data-series`, `data-x`, `data-value`) with its step's class (`seq-1` … `seq-9`; `div-m4` … `div-0` … `div-p4`, with `good` or `bad` where the sides are judged; `labelled` when it carries its value; `missing` when not known); in-table `cell-bar`, `cell-dot` and `sparkline`; a ramp's key, a `legend ramp-key` group of `legend-swatch` rects with the steps' classes, a `key-tick`, and `legend-text`s; action dots' `bar action-bar` and `action-dot` (a `<use>` of `mark-dot` with `good` or `bad` and its step); the funnel's replacement, two panels `stages` and `changes` with a `panel-title`; grouped shades `shade-light`, `shade-dark` with the slot, a grouped legend's `group-rule` and its names' `text-group-k` |
| later | `pie-gap` (breakdown: not used, the gaps being true gaps, 6.11) |
| totals stacked (stacks, 6.18) | the chart classed `stacked bar`, `stacked area` or `waterfall`; marks `segment` (a `<rect>`: a stack's part in a total, `data-series` the part, `data-x` the total, `data-value`, `data-share` in a 100 % stack; a waterfall's step, `data-series` naming its rises, falls or totals), `area` (a closed `<path>` per run of periods, `data-from`, `data-to`), a waterfall's totals `bar total`; `connector` (a waterfall's hairline at a running total, `data-from`, `data-to`) in a `connectors` group after the series groups; `tint` (a group's second part); `seq-3` … `seq-9` on ordered parts |
| one total's breakdown (breakdown, 6.11–6.11b) | the chart classed `pie` (`donut` added through an override), `treemap`, or `bar wrapped`; marks `slice` (a `<path>`, `data-series`, `data-x`, `data-value`) and `tile` (a `<rect>`, `data-group` in a hierarchy); `tint` (a group's second part in a pie, its colour paled, not a colour of its own for the lint); texts `slice-label` (a slice's name and value lines), `tile-label` (a tile's name, `text-strong` for a group's), `chart-note` (the chart's own note under it, italic); `leader-head` (a pie leader's arrowhead); a wrapped chart's columns `panel` groups with ids `column-k` |
| notes under the chart (notes, 6.19) | texts `footnote` (a note referred to by a mark, in the footer) and `note-mark` (a mark, raised, after the text it explains and before its note; both in the note style), a plain note being a `chart-note`; text modifier `on-fill` (a mark on a filled tile or cell, after the classes of the text it marks: in that text's colour) |

A line's marks carry `data-series`, and its paths `data-from` and `data-to`
(the first and last period of the stretch), so that the lint and the tests
can check that no path spans a slot whose value is not known (stage 2,
lines). Marks are drawn as `<rect>` (bars), `<path>` (lines, projections),
`<line>` (gridlines, dividers, links), `<rect rx>` (frames) and, for dots
and markers, `<use href="#mark-dot" x y width height>`: the mark library
gives the shape (a symbol drawn in a `viewBox` of −1…1), the layout the size
(from `--dot-size`, `--category-dot-size` for a categorical dot plot's open
circles (stage 2), `--marker-size`), the stylesheet the colour and whether a
dot is filled or open (the plain style opens categorical dots, `--dot-ring`
giving the ring in units of the radius). (Time: a black-and-white symbol is
a `<use>` of `mark-square`, `mark-triangle`, `mark-x`, `mark-dot`,
`mark-plus` or `mark-asterisk`, `--symbol-size` across; a bumps chart's line
is a `<path>` of cubic segments whose control points lie level with their
ends, `M… C…`, the only curved data line.)

### 4.4 Label classes and bake's lookup

`bake.py` resolves a label's size, fill (and, after 1.6, weight and style)
by trying the element's classes **in order**, taking for each class the
**first** rule whose selector mentions it; it does not apply the cascade.
Hence three constraints on chart sources and styles:

- text elements are `class="prose <modifiers…> <role>"`, modifiers first
  (`prose inset-text scale-label`, `prose text-strong end-label`);
- text roles and modifiers are single-class rules in the stylesheet, and the
  `.prose` rule comes before them;
- no text class is ever used on a mark, and no mark class on a text: a
  `.series-2.highlight` fill must never be found for a label.

### 4.5 Mark library (`plain-defs.svg`)

Symbols in a `viewBox="-1 -1 2 2"`, their paint inherited from the `<use>`:
`mark-dot` (a circle of radius 1; the stylesheet fills it, or leaves it
hollow with a stroke given in symbol units), `mark-square`,
`mark-triangle`, `mark-x`, `mark-plus`, `mark-asterisk` (black and white
symbols, R6-30; phase 2), `comment-head` (an open V marker; phase 2).
(Message: `mark-icon-down`, `mark-icon-up`, `mark-icon-left` and
`mark-icon-right`, the highlight icon pointing at its target, 6.12.4. The
`comment-head` marker is gone: a comment's arrow and the title connector
carry the house arrowhead, the diagrams' own, bent along the curve and drawn
as a path of its own (6.12.5), since a marker can only be set along the
tangent at the tip.)

### 4.6 Custom properties the stylesheet sets

Metric tokens (read by `chartgen`; values in 5.2):

`--label-size`, `--title-size`, `--note-size`, `--inset-text-size`,
`--line-pitch`, `--scale-gap`, `--unit-space`, `--unit-gridline-gap`,
`--unit-min-gridline`, `--unit-clearance`, `--category-gap`,
`--category-label-gap`, `--category-label-sep`, `--time-row-pitch`,
`--title-gap`, `--title-align`, `--title-align-insight`, `--plot-aspect`,
`--bar-row-pitch`, `--band-fill`, `--cluster-fill`, `--cluster-inner-gap`,
`--line-width`, `--line-width-highlight`, `--marker-size`, `--dot-size`,
`--category-dot-size` (stage 2), `--end-label-gap`, `--end-label-pitch`, `--end-label-max-shift`,
`--end-label-leader-shift`, `--leader-run`, `--legend-margin`, `--legend-line-key` (stage 2, lines),
`--missing-note-lift`, `--inset-frame-width`, `--inset-radius`,
`--inset-pad-x`, `--inset-pad-y`, `--inset-margin`, `--inset-gap` (inset
stage), `--inset-fade`, `--inset-height-ratio`, `--inset-max-height`,
`--inset-share-height` (D-56), `--panel-gap`, `--panel-min-height`, `--panel-max-aspect` (6.10; panels), `--grid-width`, `--legend-key`,
`--legend-gap`, `--legend-entry-gap`, `--cluster-min-gap`, `--target-mark`, `--target-mark-max`,
`--value-label-gap`, `--table-gap`, `--table-row-pitch`, `--table-col-gap`, `--table-header-drop` (series),
`--event-bar-width` (0.28em, D-42), `--time-tick` (0.45em), `--time-tick-minor` (0.25em; D-45 (ii)),
`--arrow-head-length` (0.65em), `--arrow-head-width` (0.55em), `--arrow-start-size` (0.5em),
`--arrow-panel-gap` (2em), `--arrow-short-head` (`fit`; also `overhang`, `head-only`, `hidden`: a change
shorter than a full head beside its dot, 6.13.4), `--arrow-head-halo` (0.12em), `--symbol-size` (0.55em, R6-30) (time, 6.13), `--divider-clear` (0.3em), `--divider-halo` (0.6em) (6.3.8),
`--pie-radius` (0.2 of the width), `--pie-min-radius` (3em), `--pie-gap` (0.007 of the diameter), `--pie-label-gap` (0.1 r), `--pie-label-width` (7em), `--pie-leader-angle` (8), `--donut-hole` (0.74), `--tile-gap` (1), `--tile-group-gap` (2.5), `--tile-pad` (0.3em), `--tile-min-chars` (3), `--wrap-max-aspect` (1.2), `--wrap-column-gap` (1.5em) (breakdown, 6.11–6.11b); paint `--tile-text` (#1A1A1A) and `--series-k-tint` (k ≤ 6).
`--segment-gap` (0.1em, M ≈ 1.5–2 px, s. 0230: between a stack's segments, a true gap), `--single-stack-width` (0.35 of the plot's height: one stack's bar, M 0.17–0.38) (stacks, 6.18); paint `--connector` (#A6A6A6, a waterfall's running-total hairline, M ≈ #A9A9A9, s. 0237).
(Notes, 6.19.) Metric: `--note-mark-gap` (0.05em), `--note-mark-lift` (0), `--note-hang` (0.3em), `--notes-gap` (1em), `--note-mark-size` (`--note-size`); and `--note-marks` (`letters`, `numerals` or `symbols`), `--note-mark-slant` (`italic`), `--note-footer-marks` (`raised` or `baseline`); paint `--note-mark-ink` (`--note`).

Paint tokens (used by the stylesheet's own rules; values in 5.3–5.5):

`--page`, `--text`, `--note`, `--grid`, `--zero-line`, `--zero-line-signed`,
`--divider`, `--inset-frame`, `--uniform-context`, `--single`,
`--single-strong`, `--single-line`, `--single-line-strong`, `--series-1` …
`--series-16`, `--series-1-strong` … `--series-16-strong`, `--average`,
`--total`, `--all-others`, `--missing`, `--reference-line`, `--good`,
`--bad`, `--good-cvd`, `--bad-cvd` (a safe-pair style's, D-18), `--projection-dash`,
`--projection-dash-highlight` and `--leader` (stage 2, lines), `--comment`,
`--link`, and the ramps `--seq-*`, `--div-*` (later phases).

(Tables, 6.14–6.17.) Metric: `--table-alone-pitch` (2.15em), `--table-total-gap` (3em),
`--table-rule-width` (0.6), `--table-rule-drop` (0.6em), `--table-fill-pad` (0.35em),
`--table-fill-inset` (0.2em), `--cell-pitch` (1.6em), `--heat-row-pitch` (1em), `--summary-gap`
(0.4em), `--heat-max-column` (5em), `--ramp-key-width` (12em), `--ramp-key-height` (0.7em),
`--ramp-key-gap` (0.35em), `--key-tick` (0.3em), `--cell-bar-height` (0.9em), `--cell-dot`
(0.55em), `--cell-strip` (4em), `--sparkline-width` (0.12em), `--trend-width` (4em),
`--action-pitch` (2em), `--action-bar` (0.35 of the pitch), `--action-bar-length` (8em),
`--action-dot` (0.75em), `--action-gap` (0.6em). Paint: `--table-rule` (#A3A3A3),
`--table-rule-total` (#656565), `--table-fill` (#E8E8E8) in base.css; the ramps of 5.5 (as
built), `--cell-l-k`, `--cell-chroma`, `--series-k-shade-light`, `--series-k-text`,
`--cell-bar`, `--sparkline`, `--action-bar-fill` in palette-muted.css.


## 5. The plain stylesheet

The book's plain look, flat on the page, in New Computer Modern. Values are
tagged M (measured in the book), N (the notes' proposal) or O (ours). Book
sizes were measured relative to the label size and are carried over as em.

### 5.1 Files

`plain.css` imports `base.css` (everything but data colours) and
`palette-muted.css`; `plain-office.css` imports `base.css` and
`palette-office.css`. Both use `plain-defs.svg`. (Message: `plain.css`
imports `message.css` last, the message layer's tokens and rules, 6.12.6.) A later house style is
another stylesheet over the same classes; effects such as shadows enter
through bake by role class, never in the plain files.

### 5.2 Typography and metrics

| Token | Value | Source |
|---|---|---|
| font | New Computer Modern (Typst; `NewComputerModern` for editors) | owner; passes the tabular and lining tests (05 DP-1) |
| `--label-size` | 10 | owner (30 Sep 2026): as in the diagrams, at the paper's 368 pt text width (the book's labels, about 1/31 of the chart width, would be 12 pt there) |
| `--title-size` | 1.2em (12 pt), bold | M 1.17–1.35; owner |
| `--note-size` | 0.8em, italic | M |
| `--inset-text-size` | 0.8em | M 0.73–0.82 (inset stage: kept at 8 pt, under the 9 pt floor of scale labels, because G6.4's provisional floor (D-6 (a)) sets inset labels, like notes, at 0.8 em, and the lint reads them so by `inset-text`; at 9 pt the inset's labels would read hardly smaller than the main chart's 10 pt ones. For the owner: 0.9em in a style gives 9 pt, one token) |
| unit phrase size | = label size | M 0.85–1.0; O equal, to save a text style (R5-6) |
| end labels | label size, regular; `text-strong` bold | M (R6-34) |
| `--line-pitch` | 1.25em (wrapped labels) | M 1.25–1.3 |
| `--scale-gap` | 0.9em (label column's right edge to plot left) | M 0.9–1.0 |
| `--unit-space` | 0.333em (number to unit phrase: the typeface's word space) | M 0.25; O the word space |
| `--unit-gridline-gap` | 0.6em (phrase end to where the top gridline starts) | M 0.5–0.7 |
| `--unit-min-gridline` | 0.15 of plot width | N |
| `--unit-clearance` | 0.3em (padding of the phrase box in the collision test) | N |
| `--category-gap` | 0.8em (baseline to cap top of category and first time row) | M 0.6–1.0 |
| `--category-label-gap` | 0.6em (horizontal bars: label right edge to plot left) | M |
| `--category-label-sep` | 1em (least clear gap between neighbouring category labels under vertical bars, which may reach past their bands) | owner (30 Sep 2026): at 0.5 em "Far West Rockies Midwest" read as one phrase; the gap must read wider than the spaces inside a name (the name `--category-label-gap` is the horizontal bars' label-to-plot gap) |
| `--time-row-pitch` | 1.8em (cap top to cap top between time rows) | M ≈ 2; O |
| `--title-gap` | 1.6em (last title baseline to the top scale line's baseline; for horizontal bars, to the plot top plus half a digit) | M 1.3 + 1.45 f with subtitle; O without |
| `--title-align-insight`, `--title-align` | `start` for every title, flush with the figure's left edge (the left edge of the label column), the descriptive line under an insight title likewise | D-5 (a) (stage 1; was: `middle` for descriptive titles, M 13-15 §5.2) |
| `--plot-aspect` | 0.6 (plot height / plot width) | M ≈ 0.64; O |
| `--bar-row-pitch` | 2em (band height of horizontal bars when the height is free) | O |
| `--band-fill` | 0.556 (bar : gap = 5 : 4) | M 0.55–0.56 |
| `--cluster-fill` | 0.714; `--cluster-inner-gap` 0 | M 0.72; touching bars |
| `--cluster-min-gap` | 1 (bar widths between clusters, at least) | G7.5s; M 1.0 with 2–3 series (s. 0307, 0319), 2.25 with six (s. 0331) (series) |
| `--target-mark`, `--target-mark-max` | 0.2 of the bar's width, at most 0.4em (a flat reference marker's thickness) | G7.57s, M 0.2 (s. 0321); M ≈ 0.36–0.4em (s. 0148, 0321) (series: 0.2 alone made the five departments' markers 7.5 pt thick) |
| `--value-label-gap` | 0.3em (a value label to its bar's end) | O (series) |
| `--table-gap`, `--table-row-pitch` | 2em (the last category row's baseline to a table's first row); 1.6em between rows | M ≈ 2em and 1.65em (s. 0307) (series; G7.39s's 2.15em is a table alone's) |
| `--table-col-gap`, `--table-header-drop` | 1.5em between a table's columns (and bars to a table beside them); 0.75em, the headers' baseline above the plot | G7.39s; O (series) |
| `--line-width` | 2 (0.2em) | M 0.15–0.23em |
| `--line-width-highlight` | 4 (× 2) | M × 2 (06); × 2.5 in Part 5 |
| `--marker-size` | 4 (diameter, 2 × line) | M |
| `--dot-size` | 5 (0.5em; a time dot plot's filled dot) | M (s. 0163) |
| `--category-dot-size` | 7.5 (0.75em, centre-line diameter of a categorical dot plot's open circle; ≈ 0.85em outside, with its ring) | M 0.8–0.9em outside (s. 0082, 0085) (stage 2: the book's open circles are larger than its filled dots, and at 0.5em with a quarter-radius ring they read faint at print size) |
| `--dot-ring` | 0.3 (the open circle's ring, in units of its radius: ≈ 1.1 pt) | M 0.11–0.16em (s. 0082, 0085) (stage 2) |
| `--end-label-gap` | 0.5em | M 0.3–0.9 |
| `--end-label-pitch` | 1.25em | M |
| `--end-label-max-shift` | 1 (pitches a label may move with nothing more) | N |
| `--end-label-leader-shift` | 1.5 (pitches with a thin leader to its line; beyond, a legend) | D-7 (stage 2, lines) |
| `--leader-run` | 1em (how much further right a label with a leader sits) | O (stage 2, lines) |
| `--legend-margin` | 1em (plot edge to a line chart's legend column at its right) | O (stage 2, lines) |
| `--missing-note-lift` | 0.3em (baseline above the zero line) | O |
| `--missing-note-inset` | 0.3em (horizontal bars: zero line to the note's start) | O (stage 1) |
| `--note-marks` | `letters`: a note's mark, numbered in reading order (a, b, c; `numerals` 1, 2, 3; `symbols` *, †, ‡, §, ‖, ¶) | O (notes, 6.19; the owner's to choose) |
| `--note-mark-size`, `--note-mark-slant`, `--note-mark-ink` | the notes' size (0.8em), italic, the note grey | O (notes) |
| `--note-mark-gap`, `--note-mark-lift` | 0.05em after the text; the mark's x-height at the text's cap height, 0 higher | O (notes) |
| `--note-hang`, `--notes-gap`, `--note-footer-marks` | 0.3em from a note's mark to its text; 1em from the chart's lowest ink to the first note's cap tops; the notes' marks `raised` | O (notes) |
| `--text-pad` | 0.15em (least space between text boxes, collision test) | O (stage 1) |
| `--scale-label-drop` | 0.5em (horizontal scale: plot bottom to the labels' cap tops) | M (stage 1: a token) |
| `--unit-line-drop` | 0.55em (horizontal scale: label row to the unit line's cap top, a line pitch between baselines) | O (stage 1; M 0.3em looked cramped at 10 pt) |
| `--time-label-sep` | 0.6em (least space between row-1 time labels; thinning) | N (stage 1: a token) |
| `--grid-width` | 0.6 | M ≈ 0.72 at this label size; O 0.6, the diagrams' hairline |
| `--inset-frame-width` | 1.5 (2.5 × grid) | M 2.5–4 × |
| `--inset-radius` | 6 | M ≈ 1.8 % of chart width |
| `--inset-pad-y`, `--inset-pad-x` | 0.4em; 0.3 band (source frame round its marks: along them and across them; a band's source frame round the main chart's content, inset stage) | N |
| `--inset-margin` | 0.6em (frame to inset content; a band's source frame inside the inset frame by as much, so the cone widens) | O |
| `--inset-gap` | 1.5em (a band inset's frame to its source frame, where the links run) | O; M ≈ 1.6em (s. 0081, s. 0297) (inset stage) |
| `--inset-fade` | 0.85 × bar width, below the lowest inset gridline | M (s. 0297) |
| `--inset-height-ratio` | 0.8 of the main plot height (narrow-range inset) | M |
| `--inset-max-height` | 1 × the main plot's height as drawn (an inset above or below) | owner (30 Sep 2026): at most about as tall as its main chart |
| `--inset-share-height` | 0.9 × the figure's width: an aligned inset whose figure, labelled twice, would be taller shares its main chart's axis (`share_axis: auto`, D-56) | O: about half the text block's height (672 pt) at the text width; the book's aligned insets share at about 0.88–1.0 of their width (s. 0081, 0082 left, 0297) and repeat at about 0.7 and 0.86 (s. 0077, 0082 right) |
| `--panel-gap` | 0.8em: the least space between the texts of neighbouring panels; a separator runs in its middle (6.10) | O (panels); merged bars M about one band apart (notes 02 §5.3) |
| `--panel-min-height` | 3em: a panel's plot at least this high (three intervals one em apart) | O (panels): the book's stacks ≈ 3.5–4em (s. 0203); 3em lets a stack of twelve fit a page at the text width |
| `--panel-max-aspect` | 1.5: a panel's plot at most this × its width high | O (panels): the book's rows of panels stand 1.5–1.8 × as high as wide (s. 0339–0340) |
| `--legend-key` | 0.55em square, `--legend-line-key` 2em; `--legend-gap` 0.25em key to text, `--legend-entry-gap` 1em between entries | M |

### 5.3 Greys and lines

| Token | Value | Source |
|---|---|---|
| `--page` | #FFFFFF | M |
| `--text` | #595959 (titles, labels, unit phrase, end labels, legend) | M; contrast 7.0 |
| `--note` | #767676 (contrast 4.5) | O: the book's ≈ #B0B0B0 is 2.1 : 1 on white, against R5-8 |
| `--grid` | #D9D9D9 | M; contrast 1.41 |
| `--zero-line` | #D9D9D9 (= grid, R1-07) | M |
| `--zero-line-signed` | #A6A6A6 (signed ranges) | M (s. 0070, 0235) |
| `--divider` | #D9D9D9 (time dividers) | M |
| `--inset-frame` | #D9D9D9, frame fill `--page`, links the same colour and width | M |
| `--projection-dash` | `0 5` with round caps (dots one line wide, 1.8 pt gaps) | M dots ≈ 1 stroke, gaps 1.5–2 strokes |
| `--projection-dash-highlight` | `0 10` (the same dots on a line twice as thick) | O (stage 2, lines) |
| `--leader` | #BFBFBF (end-label leaders) | M (the pie leaders' #C0C0C0, 5.5) (stage 2, lines) |
| `--uniform-context` | #BFBFBF (only for rebuilding the rejected grey-out, R6-28) | M (s. 0197) |
| `--comment` | #3F63B0 (phase 2) | N |
| `--link` | #BFBFBF (title connector, phase 2) | M |
| lines | `stroke-linejoin: round; stroke-linecap: round` | M (s. 0198) |
| bars | flat fill, no stroke, no rounded corners | M, R1-03 |

### 5.4 Palettes: the book's two colour families

The notes found two families in the book's figures (08-12 §8 Q1, 13-15 §5):

- the **muted family** (#95B7D3 blue, #DD8045 orange, #A4AB81 sage, #D8B35B
  mustard, #7CA79D grey-teal, …), used in Part 1 (where the colour rules
  are made), the pie chapter and the maps; 00-01 identifies it, unverified,
  as Office's "Median" theme;
- the **Office family** (#5B9BD5, #ED7D31, #A5A5A5, #FFC000, #4472C4,
  #70AD47), the default of the author's spreadsheet, used in many bar,
  table and combo figures.

Proposal: the palette is a separate file with the same token names in each
family, so a chart is built under either by choosing the stylesheet
(`plain` or `plain-office`), exactly as the diagrams are baked under several
styles. **`plain` uses the muted family**: it is the palette of the colour
chapter itself, and it passes that chapter's checks (chroma 0.05–0.14,
lightness spread 0.09, no grey or red among regular slots). `plain-office`
reproduces the Office look for comparison with book pages drawn in it; to
obey R3-16 its grey moves out of the regular slots, and the style lint
reports its gold (#FFC000, chroma above 0.15, R3-2) and green (R3-18) as
known warnings. The examples document can be built under both.

Slot order is fixed, so a category keeps its colour whatever the number of
categories (R3-24): slots 1–5 are the first tier, 6 the sixth colour, 7–11
the darker second tier, 12–16 the third tier; more than 8 regular slots
needs direct labels (R3-21, R3-22). The **strong** token of a slot, used for
highlighting (R3-7, R15-5), is the same hue darker: for the muted family the
book's own next tier (M, 03-04 §5.1).

Muted family (`palette-muted.css`, all M from 03-04 §5.1 unless noted):

| Slot | Base | Strong |
|---|---|---|
| 1 | #95B7D3 | #558BB9 |
| 2 | #DD8045 | #BA5C28 |
| 3 | #A4AB81 | #81875B |
| 4 | #D8B35B | #B98E2F |
| 5 | #78A6AA (the book's #7CA79D turned 24° bluer, owner, 30 Sep 2026: 0.070 from the olive under protanopia, not 0.051) | #558286 |
| 6 | #9A77B7 | #7A60A9 (s. 0115) |
| 7–11 | #558BB9, #BA5C28, #81875B, #B98E2F, #558286 | #3F6C96, #924A22, #666C46, #94712A, #46686C |
| 12–16 | #3F6C96, #924A22, #666C46, #94712A, #46686C | `oklch-shift(base, −0.12, 0)` (O) |

The strong of slot k (k ≤ 5) equals the base of slot k + 6, so a chart with
more than six series cannot highlight by colour alone; the model lint warns
and the line method (thickness) still works.

| Token | Muted | Office | Source |
|---|---|---|---|
| `--single` (one-colour bars and dots) | #95B7D3 | #5B9BD5 | M (R3-3; 08-12 §5.1) |
| `--single-strong` | #558BB9 | #2E75B6 | M (03-04 §5.1; 08-12 s. 0319) |
| `--single-line` | #95B7D3 (owner, 30 Sep 2026: blue like single bars; the book's F5 line is #DD8045) | #5B9BD5 | O (R3-3) |
| `--single-line-strong` | #558BB9 | #2E75B6 | O |
| `--average`, reference series | #595959 | #595959 | M (s. 0201; 03-04 #505050) |
| `--total` (total bar) | #A6A6A6 | #A6A6A6 | M (07) |
| `--all-others` | #968C8B | #968C8B | M (s. 0120, 0230) |
| `--missing` (missing marks on maps, heat cells) | #A6A6A6 | #A6A6A6 | M |
| `--reference-line` (scalar comparison) | #4472C4 | #4472C4 | M (s. 0382) |
| `--good`, `--bad` | #6EAE44, #C5392A | #70AD47, #C73A28 | M |
| `--good-cvd`, `--bad-cvd` (a safe-pair style's, D-18) | #79A2C0, #D48752 | same | M (the s. 0124 ramp); pairing O |

Office regular slots (`palette-office.css`): 1 #5B9BD5, 2 #ED7D31, 3
#FFC000, 4 #4472C4, 5 #70AD47 (M, with Office's grey #A5A5A5 removed from
slot 3); slot 6 #9A77B7 and slots 7–16 as in the muted file (O: the notes
record no further Office colours); strong tokens `oklch-shift(base, −0.12,
+0.01)` (O, the rule 03-04 §5.3 proposes), except slot 1 #2E75B6 (M).

### 5.5 Ramps for later phases

Recorded now so that the palette files are complete; all M.

| Token | Values |
|---|---|
| `--seq-blue-3` | #BED8E7, #6AAED5, #3082BE |
| `--seq-blue-5` (dark = more; ordered risk) | #086785, #07799D, #0F89B2, #84AEC7, #B5CBD9 |
| `--seq-multihue-9` | #FDFBE2, #D8EAC4, #B4DBAF, #8EC69F, #72A994, #57838C, #426086, #2A3881, #282A74 |
| `--div-red-green-5` | #E24A33, #FDCA9F, #F3F3F3, #BFDDA9, #70AD45 |
| `--div-blue-orange-9` (CVD-safe, continuous anchors) | #2E5D89, #79A2C0, #BFE0F3, #DDEEF8, #FFFFFF, #FFE2C0, #FDC683, #D48752, #AB4E2D |
| `--div-multihue-9` | #8A171A, #C15C28, #EA9747, #FFCE8B, #FDFBE2, #CCE5BD, #9AC5A9, #668B9E, #184795 |
| grouped shades (R3-11) | purple #9C7FBC, #7A60A9; orange #F49C60, #DC7F44, #F6B78E |
| de-emphasis (bars, pies) | `mix(base, #FFFFFF, 0.4)` (07 §5.1) |
| pie leader | #C0C0C0; table rules #A3A3A3 (header), #656565 (total); table highlight #E8E8E8 |

**As built (phase 3, "(tables)"; `style/palette-muted.css`, `style/base.css`; 6.14).** The
plain style keeps one ramp of each kind, nine steps each, named by step, not by colour (a
house style gives its own values under the same names):

| Tokens | Values | Source |
|---|---|---|
| `--seq-1` … `--seq-9` | #EDF5FB, #CFDFEC, #B2CBE0, #95B7D3, #74A1C7, #558BB9, #497BA8, #3F6C96, #33587D | single-hue, the muted blue's (h ≈ 243°), about even in OKLCH lightness (0.97 to 0.45), through the palette's own blues: step 4 the single blue, 6 its strong shade, 8 the third tier's (M); the rest O, set in OKLCH. *Departure:* not `--seq-blue-3` nor the teal-blue 5 of G4.28 (whose hue drifts 174°–238°): one hue, the plain style's, that the book's own muted figures shade tables and tile maps in (s. 0156) |
| `--div-m4` … `--div-0` … `--div-p4` | #2E5D89, #79A2C0, #BFE0F3, #DDEEF8, **#F3F3F3**, #FFE2C0, #FDC683, #D48752, #AB4E2D | the CVD-safe blue–orange ramp (M, s. 0124), blue below the dividing value; its white neutral made the light grey of s. 0118 (so that a cell at the dividing value reads as a cell; R3-14 allows either) |
| `--good-1` … `--good-4`, `--bad-1` … `--bad-4` | #ADD89E, #75BA69, #4A9556, #317248; #FBC2BB, #F48772, #E5494C, #BE2343 | the green–red ramp (M, s. 0117), a good/bad ramp's sides (D-18); `--good-cvd-k`, `--bad-cvd-k` are the blue–orange sides under a safe-pair style |
| `--cell-l-1` … `--cell-l-9`, `--cell-chroma` | 0.975 … 0.87 (OKLCH lightness), 0.06 | a cell that carries its value (`labelled`): its step at that lightness, hue kept, chroma at most 0.06 (`oklch-light`, a colour function added to the flattener): the text grey at 4.6 : 1 or more on every step of every ramp (O; the book's cells are darker and fail) |
| `--series-k-shade-light`, `--series-k-text` (k ≤ 6) | the slot's colour 40 % to the page; the third tier (purple: its strong) | grouped shades (light, base, strong) and a group's name in its hue at 4.5 : 1 (O; M s. 0115: orange #F6B78E, #F49C60, #DC7F44) |
| `--cell-bar`, `--sparkline`, `--action-bar-fill` | the single blue 40 % to the page; the strong blue; #D9D9D9 | in-table bars, pale enough for the text grey (M s. 0309); sparklines (M); action dots' bars (M s. 0327) |

The style lint (`style.lint_ramps`) checks each ramp darkens step by step away from its light
end or neutral (R3-9, R3-14), the neutral is quiet and has no bright yellow or amber beside it
(R3-15), and the text grey keeps 4.5 : 1 on every `labelled` step and on `--cell-bar` (G6.5).

### 5.6 Sketch of `base.css`

(The built file is `style/base.css`; this sketch shows its order.)

The order matters (4.4, and specificity ties): tokens, text, non-data,
slot paints, highlight paints, then mark-kind rules, which reset what does
not apply.

```css
/* @defs plain-defs.svg */
:root {
  --label-size: 10;  --title-size: 1.2em;  --note-size: 0.8em;  --inset-text-size: 0.8em;
  --grid-width: 0.6;  --line-width: 0.2em;  --line-width-highlight: 0.4em;
  --page: #ffffff;  --text: #595959;  --note: #767676;  --grid: #d9d9d9;
  /* … every metric and grey token of 5.2 and 5.3 … */
}
.prose        { font-family: NewComputerModern, "New Computer Modern";
                font-size: var(--label-size); fill: var(--text); }
.title        { font-size: var(--title-size); font-weight: bold; }
.missing-note { font-size: var(--note-size); font-style: italic; fill: var(--note); }
.inset-text   { font-size: var(--inset-text-size); }
.text-strong  { font-weight: bold; }
.gridline     { fill: none; stroke: var(--grid); stroke-width: var(--grid-width); }
.gridline.zero.signed { stroke: var(--zero-line-signed); }
.time-divider { stroke: var(--divider); stroke-width: var(--grid-width); }
.inset-frame  { fill: var(--page); stroke: var(--inset-frame); stroke-width: var(--inset-frame-width); }
.inset-source, .inset-link { fill: none; stroke: var(--inset-frame); stroke-width: var(--inset-frame-width); }
.single       { fill: var(--single); stroke: var(--single); }
.series-1     { fill: var(--series-1); stroke: var(--series-1); }        /* … to 16 */
.average      { fill: var(--average); stroke: var(--average); }
.single.highlight   { fill: var(--single-strong); stroke: var(--single-strong); }
.series-1.highlight { fill: var(--series-1-strong); stroke: var(--series-1-strong); }  /* … */
.line.single        { stroke: var(--single-line); }
.line.single.highlight { stroke: var(--single-line-strong); }
.chart .bar   { stroke: none; }
.chart .line, .chart .projection { fill: none; stroke-width: var(--line-width);
                stroke-linejoin: round; stroke-linecap: round; }
.chart .line.highlight, .chart .projection.highlight { stroke-width: var(--line-width-highlight); }
.chart .projection { stroke-dasharray: var(--projection-dash); }
.chart .point, .chart .dot { stroke-width: 0; }
.chart.categorical .dot { fill: none; stroke-width: var(--dot-ring); }  /* hollow (s. 0085); symbol units, 0.3 of the radius (stage 2) */
.uniform-context { fill: var(--uniform-context); stroke: var(--uniform-context); }  /* rejected grey-out only */
```


## 6. Algorithms

### 6.1 Nice range and interval

Arithmetic is exact: a step is s = m·10^k (m ∈ `steps`, default {1, 2, 5};
2.5 only with `allow_2_5`; `custom` lists such as 12 or 45 allowed with a
declared reason, R2-09), limits are integer multiples of s, and values are
`Decimal`. `vmin`, `vmax` are over the known values of all series of the
panel (projections included); `spread = vmax − vmin`; b = `breathing`
(0.15), whose band is [b − 0.05, b + 0.05] = [0.10, 0.20].

**Lower-limit mode** (02 D2, D3; R2-22–R2-26, R9-5):

```
length_encoded = type in {bar, stacked bar, stacked area, waterfall …}
if vmin < 0 < vmax:                     mode = MIXED (a length-encoded chart keeps 0 inside, which it is)
elif vmax <= 0:                         mirror everything below (all negative, R2-29)
else:                                   # all values >= 0
    T = measure.min
    if length_encoded or scale.zero == include:               mode = FIXED(0)   # bars never truncated
    elif T is not None and T > 0 and vmin - 0.15*spread <= T: mode = FIXED(T)   # e.g. credit scores
    elif spread == 0 or vmin < spread:                        mode = FIXED(0)   # not narrow and far (R2-25)
    elif scale.zero == truncate or measure.zero == arbitrary \
         or facts.distance_to_zero_irrelevant or audience.knows_scale:
                                                              mode = TRUNCATED  # R2-24, D2 Q2/Q6/Q7
    else:                                                     mode = FIXED(0)   # + inset if small differences matter
```

A spec `scale.min` other than 0 on a length-encoded chart is refused (R2-24,
R9-5). `mode = TRUNCATED` on a chart without the facts that allow it is
impossible by construction; with `scale.min` set by hand on lines or dots it
is a guided finding naming the procedure's lower limit (stage 2: truncating
lines and dots is guided, G2.8 and G2.11, so it is reported, not refused;
bars keep the refusal). The report names the fact that allowed a truncation
(`scale.zero truncate`, an arbitrary zero, G2.10, or a declared
irrelevance, G2.8), and records it at the guided level.

**Snapping** (the band is a target for the snapped limit; 02 D5):

```
snap_up(vmax, base, s):     band = [vmax + (b-0.05)*base, vmax + (b+0.05)*base]
    if multiples of s lie in band: return the smallest
    else: among floor(band.lo/s)*s and ceil(band.hi/s)*s keep those with U - vmax >= 0.05*base,
          return the one nearest the band;  miss = distance outside band / base
snap_down(vmin, spread, s): mirror image, band = [vmin - (b+0.05)*spread, vmin - (b-0.05)*spread]
```

**Choosing the step:**

```
span = vmax - (the FIXED lower limit, or vmin - 0.2*spread when TRUNCATED or MIXED)
for k in floor(log10(span)) - 2 … floor(log10(span)) + 2, m in steps:  s = m*10^k
    L = FIXED value, or snap_down(vmin, spread, s) (TRUNCATED, MIXED), never below T or 0
    base = spread if MIXED else vmax - L
    U = snap_up(vmax, base, s); if M declared: U = min(U, M); skip s if M < U or M is not a multiple of s
    n = (U - L)/s;  skip unless 2 <= n <= 10
    skip if precision_value is set and not a multiple of s                       (R2-33)
    score = |n - target_intervals| + 10*(missL + missU) + (0.5 if m == 2.5)
choose the lowest score; ties: fewer intervals, then the larger step
```

**A step of 5 or 1 that leaves too much room** (owner, 30 Sep 2026; stage 1):
when the chosen step is 5 × 10^k and leaves more than 25 % room above the
data (of the data's extent from the lower limit), 2 × 10^k is tried, the
upper limit then the first multiple leaving 5 %; it is taken if it needs at
most 8 intervals, and the rejected step and its room are recorded in the
scale's guided decision (02 D5). A maximum of 1,515 gets 0–1,600 by 200
(6 %) instead of 0–2,000 by 500 (32 %). A step of 1 × 10^k gives way to
5 × 10^(k−1) the same way (owner, 30 Sep 2026): days in a month, 28–31, get
0–35 by 5 rather than 0–40 by 10 (29 %). This departs from the book once,
knowingly: 3.16 gets 0–3.5 by 0.5 where the book keeps 0–4 by 1 (27 %). A
step of 2 is left alone, and 396 keeps 0–500 by 100 (26 %), where 50 would
need 9 intervals.

Checked against the book (02 D5; a prototype of exactly this reproduces
them): max 21 → 0–25 by 5; 49.5 → 0–60 by 10; 114.5 → 0–140 by 20; 396 →
0–500 by 100; 18.1 → 0–20 by 5; 53.7 → 0–60 by 10; 2.20 → 0–2.5 by 0.5;
3.16 → 0–4 by 1; 1,794 → 0–2,000 by 500; 95 → 0–100 by 20; credit scores
307–467 with T = 300 → 300–500 by 50; index −14…+58 → −20…+80 by 20;
revenue 16.29–17.76 truncated → 16.0–18.0 by 0.5; 45–97 % with M = 100 →
0–100 by 20. Known deviations (class scores 6.9–7.8 give 6.8–8.0 by 0.2,
the book 6.5–8.0 by 0.5) stay deviations; `scale.step` pins a step.

**Inset scales** (6.6): an outlier inset uses the same procedure on its
members with L = 0 (so 16 at most gives 0–20 by 5, where the book stops at
15 with a bar of 16; we do not let a bar pass the top gridline). A
narrow-range inset of **bars** uses L = the largest multiple of s strictly
below vmin, because the fade (6.6) shows the cut and the lowest bar still
shows a stub (232–274 gives 230–280 by 10, as on s. 0297); of **lines or
dots** it uses the TRUNCATED mode (16.29–17.76 gives 16–18 by 0.5, as on
s. 0081; 137.8–141.4 gives 137–142 by 1, where the book starts a step lower
at 136, s. 0082: a known deviation, the book's lower limits being a step
below the procedure's in both figures of s. 0082 and not on s. 0081 or
s. 0297, so no rule reproduces them; inset stage). The inset takes the
spec's step mantissas but not its pinned step or limits, which are the main
scale's.

**Direct labels instead of a scale** (R2-32, phase 2): if the chart has
fewer values than the scale would have labels, report an info in phase 1;
in phase 2 `value_labels: auto` switches to direct labels and removes the
gridlines, keeping the zero line. (Series: built for bars, 6.7.12; the scale
is still computed, since the bars' lengths and the labels' magnitude come
from it; lines and dots keep the info.)

### 6.2 Numbers and dates formatted as a set

Scale labels are formatted **once for the whole set** (R2-21, R5-17), never
one at a time.

Inputs: the ticks t0 < … < tn (exact), the step s, `measure.kind`, `unit`,
`scale.magnitude`, `scale.magnitude_style`, locale (group separator ",",
decimal ".", minus U+2212).

1. **Actual values**: v × 10^`unit.magnitude` (data given in thousands stay
   exact).
2. **Magnitude** e ∈ {0, 3, 6, 9, 12}: if `magnitude` is `auto`, e = 3 ·
   floor(log10(max |t|)/3) when the largest tick has at least 5 integer
   digits (R5-10, D-4 (i)), else 0; then, while e > 0 and s/10^e would need
   more than one decimal, e − 3 (D-4 (iii): $0.0M–$2.0M, but $0K–$1,250K).
3. **Style** (`auto`): `word_on_top` for every magnitude (owner, D-3 (a));
   `suffix` is a setting (D-3 (b)); the top value written in full is never
   produced (D-3 (c)). The magnitude's word (D-4 (ii)): "K" for thousands,
   set straight after the number; "million", "billion" spelled out after a
   space; the audience's own abbreviations when `audience.abbreviations`
   gives them (R2-06, R5-13). (Stage 1; was: e = 3 → `full_top_value`.)
4. **Decimals** d: the fewest decimals that write s/10^e exactly (5 → 0, 0.5
   → 1, 0.05 → 2, 0.25 → 2). Every label, zero included, gets d decimals.
   **Money** (round 1, sonnet-G B12; the book's "$53.70"): a set of amounts
   in the currency's own unit (e = 0; a currency sign in the prefix or
   suffix, or its ISO code) that needs any decimals takes two, pence or
   cents, never one: 0.00, 0.50, 1.00 … £2.00 (`fmt.money_decimals`; the
   yen, the won and the Icelandic krona, with no minor unit in use, keep
   theirs). The same holds for data labels ("£2.80", never "£2.8"), the
   totals beside stacks and the cells of every table.
5. **Round** half away from zero (`Decimal.quantize(ROUND_HALF_UP)`, R5-16);
   ticks are exact anyway, data labels are not.
6. **Group** integer parts of 4 or more digits in threes (R5-14): 1,200.
7. **Assemble**. The number n_i (scaled, rounded, grouped) of every label is
   bare, except:
   - `%` is on every label ("0%", "20%", … "100%"), because a bare 20 is
     not a percentage;
   - `word_on_top` (the default): the prefix, the magnitude and the unit
     are on the top label only: "25 million inhabitants", "$300K in
     donations", "$2.0 million in revenue", "35°C" (a suffix without a
     space); the others are bare: "20", "250", "1.5", "30";
   - `suffix`: every label is prefix + n + abbreviation + suffix ("$0K",
     "$100K" … "$600K"; R5-11); the top adds the phrase.
   The phrase is the magnitude word (if a word), a suffix that begins with
   a space (" lbs."), the `noun`, running on, and `what`, always in
   parentheses: "0.25 lbs. (weight of product)", "60 (max employees during
   month)", "25 million inhabitants" (D-20, decided by the owner). This is
   the vertical scale's form; a horizontal scale's labels are bare numbers
   (the same magnitude and decimals, no prefix or suffix), its unit in its
   title (6.4), except a percentage's "%", on every label of a horizontal
   scale too ("0%", "20%" … "100%"; "+20%", "0%", "−20%"), its title then
   without "(%)" (owner, 1 Oct 2026: G2.24 over G2.19; `fmt.scale_title`).
   Magnitudes below one (thousandths, millionths; R2-05) follow the same
   scheme in phase 2.
8. **Signs**: negatives "−" before the prefix ("−$20K"); on `change` and
   `index` kinds positive labels get "+" and zero none ("+20%", "0%",
   "−20%").

The top label is returned in three parts, because 6.4 places them
separately: the **number** (sign, prefix, digits and "%": "$300", "100%"),
right-aligned with the other labels; the **attached** part, set straight
after it (an abbreviation "K", a suffix without a space "°C"); and the
**phrase**, after a word space ("in donations", "million inhabitants").
Fallback A keeps the number with its currency, magnitude and suffix on the
top label ("$300K", "$20 million", "0.25 lbs.", "35°C", "100%") and moves
only the description, the noun and the `what` ("revenue per month"), or
`unit.fallback`, to the unit line: a number never parts from its currency
or magnitude (owner, 30 Sep 2026). So the phrase is split in two
(`fmt.unit_words`): the kept words (a magnitude word, a spaced suffix) and
the description.

**Data labels and value notes** (`format_values`, phase 1 for tests, phase 2
for drawing): one e and one d for all values of a series; d = the fewest
decimals at which values that the message compares stay distinct, bounded
by `measure.precision`, then money's pence (step 4); prefix and suffix on
every label ("$53.70"). A censored value (2.4) is written as its limit with
its sign, "<1", ">2,420", in the set's decimals or more, never rounded
(`fmt.censored_text`: "<0.5" stays).

**Dates.** Month tables are fixed, not locale-dependent: axis row 1 `Jan …
Dec` (narrow `J F M …`); inline and axis row 2 `Jan. Feb. Mar. Apr. May June
July Aug. Sept. Oct. Nov. Dec.` (O, consistent with the book's "Mar. 11,
2022", "Oct. 2022", "July 2022"). Inline: "Mar. 11, 2022" (R5-20, R5-21;
`audience.date_format` overrides everything, R5-19). Columns (tables):
`DD-Mon-YYYY` (R5-22). Never numeric months or two-digit years (lint R5-20,
R5-21).

### 6.3 Time axes (multi-level)

For regular grids (phase 1); irregular axes (step, thin bar) are phase 2
with the tick ladder of 06 §5.3 (time: built, 6.13.1).

1. **Positions**: N equal slots across the plot; a point sits at its slot
   centre (half a slot of padding at both ends, O for all grains). A line
   chart with end labels (stage 2, lines) keeps half a slot at the left, but
   its gridlines stop half an end-label gap past the last point, as on the
   book's cheat sheet (s. 0201), so that the labels sit by the line ends and
   never on a gridline; when the axis has dividers, half a slot past it, so
   that the right-hand divider clears the last label, the end labels then
   starting half a gap after it. An outer time label (or, over ordinal
   categories, an outer name) that would reach past the figure's edge pulls
   the plot in by as much, and the layout runs again (as bars do, 6.7.1).
2. **Rows**: row 1 is the grid unit, row 2 the next unit (hour → day, day →
   month, week → month, month → year, quarter → year; year: no row 2).
   Formats: hour "6 AM"; day "16" (no leading zero); week "Jun 19–25"; month
   "Jan" or "J"; quarter "Q1"; year "2021". Row 2: "Oct. 2022" under days,
   "2022" under months. Row 2 is drawn for every group; a group too narrow
   for its row-2 text keeps its dividers and drops the text (built in stage
   2, lines: `timeaxis.fit_row2`, the text plus `--time-label-sep` against
   the group's slots).
3. **Fitting row 1**: slot width w; for the format list (months: full, then
   narrow; others: full only) find the first with k = ceil((max width + 0.6
   em)/w) = 1; else take the full format with that k and **thin**: in each
   row-2 group show the labels at indices 0, k, 2k … counted from the group's
   first period (02 D9: 16 18 … 30 | 1 3 5 …); if the last label of a group
   and the first of the next are closer than max width + 0.6 em, drop the
   earlier. If k > 7 (O), refuse and ask for a coarser grid (aggregation is
   a change of data, not of drawing). Never rotate (R2-40).
4. **Vertical placement**: row 1 cap top at `--category-gap` below the
   baseline, row 2 `--time-row-pitch` lower; labels centred on their slot
   (row 1) or on their group's span (row 2). A single row-2 group whose text
   the title block already gives ("2022" in "Jan–Jun 2022") is left out
   (G2.37 (fig.); stage 1).
5. **Dividers** (`time-divider`): when there are two or more row-2 groups,
   at every group boundary (midway between slots) and at both outer plot
   edges, from the baseline to 0.3 em below the row-2 text. They are drawn
   in the `grid` group, under the marks like every non-data line (R1-09;
   stage 2, lines: the SVG lint found them after the marks).
6. **Incomplete periods** (R2-42, 02 D10; stage 2, lines): a first or last
   period that `data.as_of` cuts short, or whose value says `complete:
   false`, is left out of a line chart, axis and all, with a decision naming
   it (G2.40's first option); one inside the series cannot be and is
   warned about. Drawing it apart (an isolated point with "As of Dec. 10")
   is for a later stage, as are bars' dotted level marks. (Time: both are
   drawn when `facts.incomplete_value_needed` says the value must be shown,
   and bars leave the period out otherwise, 6.13.5.)
7. The time axis always runs left to right (R2-37); a vertical time axis is
   refused (R6-38).
8. **Labels clear of the dividers** (owner, 30 Sep and 1 Oct 2026; time,
   CG-6). Every time label clears every divider by `--divider-clear` (0.3 em,
   half the least space between two labels, `--time-label-sep`), or has a
   halo. At the outer ends the first and last slots move in from the
   dividers, which stay at the plot's edges with the gridlines' ends, until
   the first and last shown labels clear them, each still centred on its
   slot and none dropped (`timeaxis.edge_room`, `edge_needs`; a decision
   names the room when it binds): kept rather than haloed, because an
   unbroken divider at each end reads as the axis's end, and the room costs a
   few points of plot. Beside a divider between two groups a shown label
   stays on its slot and gets a **knock-out halo** (`timeaxis.add_halos`): a
   copy of the label (`text-halo`) in a `divider-halo` group just before it,
   after the divider, which `.divider-halo use` fills and strokes in the page
   colour, `--divider-halo` (2 × the clearance) wide with round joins, so
   that the divider stops short of the letters by the clearance instead of
   crossing them (the baked glyphs are `<use>`s; Typst's renderer honours
   the rule; flat, no card or shadow). `fit(…, clear)` names these labels in
   a decision and drops none. Label-to-label crowding is unchanged
   (thinning); where even every seventh label would crowd, the chart is
   refused as too narrow for its time axis (R2-40, `timeaxis.Crowded`),
   supplying a `layout.width` that holds the labels (`too_narrow`); labels
   are never shrunk. This holds on lines, bars and dots over periods (hooks
   in `layout/bars.py`: `edge_for`, `clear_for`, `widen`, `add_halos`, the
   refusal's supply), bumps charts, and the true time axes of step charts and
   thin bars (`continuous_fit(…, clear)`, `continuous_edge`), where t0 and t1
   move in from the plot's edges. An inset's panel takes its main chart's
   slots. In P1-21 `narrow` November's "1" keeps its slot with a halo, "16 19
   … 28 | 1 4 7"; at the full width it clears the divider without one. The
   SVG lint fails a time label, row 1 or row 2, within the clearance of a
   divider unless its halo is there (CG-6). (The first fix of 30 September
   dropped such labels, losing November's "1"; the owner preferred halos.)

### 6.4 The unit phrase and its fallbacks

The unit phrase replaces the rotated axis title (R2-07, R0-06, R2-11); it is
never moved into the title.

**Vertical value scale.** Let the label column's right edge be xr = plot
left − `--scale-gap`, the top gridline's y be yt, the plot span x0…x1.

```
column width C = max width of all scale labels except the top one, and of the top one's number part
                 (with its prefix: "$300"); the figure's left edge is the column's left edge
top number:  right-aligned at xr, like every label                            (02 §5.3)
phrase:      the attached part and the phrase ("K in donations") start at xr, straight after the
             number; a phrase alone ("million inhabitants") at xr + --unit-space; same baseline
box B:       from the top number's left edge to the phrase's end, ascent/descent of the line,
             padded by --unit-clearance
gridline:    the top gridline starts at B.right + --unit-gridline-gap - clearance
inline OK  ⇔ (x1 - gridline start) >= --unit-min-gridline × (x1 - x0)
             and B meets no mark, marker, end label, legend or inset frame
```

Fallback A (inline not OK): the top label keeps its number with its
currency, magnitude and suffix ("$300K", "$20 million", "0.25 lbs.",
"35°C", "100%"; owner, 30 Sep 2026: a number never parts from its currency
or magnitude), the top gridline starting after them as it does after an
inline phrase; the **unit line**, the description alone, is set
on its own line above the top scale line, left-aligned with the column's
left edge, its baseline `--line-pitch` above the top label's; it may extend
over the plot (nothing is drawn there). Its text is `unit.fallback`, else
the noun and the `what` ("inhabitants", "revenue per month"); a `what`
alone loses its parentheses on its own line ("max employees during month").
A unit with no description ("35°C") has no unit line. If it is wider than
the chart it wraps once at a space (02 #15, s. 0087); longer than two lines
is a lint error. (Was: the magnitude word and a spaced suffix moved with the
description, so P1-09's compact main chart read "$20" under "million
(revenue per month)". Of the examples only P1-09 changes: P1-03 gives its
own fallback text, "% of employees working remotely", its "100%" keeping the
"%", and P1-19's unit is a noun.)

Fallback order: inline → A. `scale.unit_position` forces either (a guided
option: forcing what the procedure would not choose is a guided finding).
The title gap is measured to the unit line when there is one.

**Horizontal value scale** (horizontal bars, dot plots). The unit runs on
from the top label only on a vertical scale ("50 accidents per month", "8.0
on 10", "35°C", p. 57, 85). A horizontal scale's labels are bare numbers,
centred on their vertical gridlines, cap tops `--scale-label-drop` (0.5 em)
below the plot, with no prefix, suffix or magnitude ("%" excepted: a
percentage's labels carry it here too, "0%" … "100%", owner, 1 Oct 2026, G2.24
over G2.19); under them, its
cap top `--unit-line-drop` below the label row, the scale's **title**
(class `scale-title`), centred under the scale span: the quantity followed
by its unit and magnitude in parentheses, as the book titles horizontal
scales and writes money units in titles and headers ("Weirdness Score (on
10)", p. 57; "Satisfaction Rating (on 10)", p. 85; "Donations ($000s)",
p. 278; "Change in Donations ($K)", p. 200; "Funds raised ($M)", p. 342).
The quantity is `unit.what`, else `measure.name`; the parentheses hold the
currency with the magnitude ("$K", "$ million": K for thousands, words for
millions and more, D-4), then a suffix ("lbs."; never "%", which its labels
carry: "Respondents using the feature", not "… (%)"), or, with neither, the
magnitude and the noun ("million inhabitants", "on 10"); `unit.scale_title`
replaces all of it (owner, 30 Sep 2026; stage 1), and a "(%)" in it over
labels that carry "%" is warned by the SVG lint (R2-21). The plot's right edge
leaves half the last label's width. The plot's top
sits a title gap below the last title line, less half a digit, where a
vertical chart's top gridline would be.

**Insets** restate the phrase on their own top label at inset size (or,
under a horizontal scale, their title) only if needed (G2.27; inset stage):
where the main scale's unit writes what bare numbers lack, a currency, a
magnitude or a suffix ("$280K in donations", s. 0297; "$18.0 million
(revenue per month)", s. 0081); a count's noun or a description stays the
main scale's, beside it, and the inset's labels are bare numbers (s. 0076,
s. 0082).

### 6.5 Direct end labels without collisions

For lines (and later slope charts and stacked areas). R4-1, R6-32, 03-04
§5.8, 06 §5.6.

1. **Anchors**: for each series, its last drawn point (the projection's end
   if any; the last known point if the series ends in missing values);
   target y = that point's y. Special-role series are labelled like the
   others, `text-strong` for averages, references and highlighted series.
2. **x**: anchor x + `--end-label-gap`, left-aligned; series ending at the
   last slot share one x.
3. **Pack** vertically with pitch p = `--end-label-pitch`, inside [plot
   top, plot bottom]:
   ```
   items sorted by target y (top first); clusters = [[i] for each item]
   repeat:
       for each cluster c (n items):  top = mean(target) - p(n-1)/2
                                      top = clamp(top, ymin, ymax - p(n-1)); y_k = top + k·p
       find the first adjacent pair (a, b) with b.first.y - a.last.y < p; merge them; else stop
   ```
   At most n − 1 merges; order is preserved by construction.
4. **Test** (D-7, as the owner's provisional default; stage 2, lines):
   every |y − target| ≤ `--end-label-max-shift` × p (1) leaves the label
   as it is; up to `--end-label-leader-shift` × p (1.5) the label moves
   `--leader-run` further right and a thin grey `leader` joins it to its
   line's end (only that label moves: the others stay by their lines); and
   no label box may meet another series' line or marker (a series that ends
   early is labelled at its last known point, inside the plot). If either
   test fails, the chart gets a legend instead (a decision under R4-2): a
   column `--legend-margin` right of the plot, a `legend-key` stroke
   `--legend-line-key` long in the series' width and colour, then its name
   in `legend-text`, entries ordered by the last drawn values, top to
   bottom, one pitch apart, vertically centred on the plot (R4-5, R4-7). The
   gridlines then run half a slot past the last point. (Was: no leaders in
   phase 1, since the book shows none for lines; the caller's brief takes
   D-7's recommendation.) With `chart.labelling: direct` a failing test is
   a guided finding and the labels stay; `legend` where the labels fit is a
   guided finding (R4-1).
5. **Right margin**: the slot width is the largest that leaves each label,
   at its x, inside the figure: for a label at the last slot, (W − plot
   left − gap − its width) / (n − ½), or, with dividers, (W − plot left −
   gap/2 − its width) / n; the figure width is fixed, so the plot shrinks.
   One series has no label (G5.6) and its plot runs to the figure's edge.
6. **Colours** of series are given in the order of the packed labels, top
   to bottom (so slot 1 is the top line at the end), unless `slot` fixes
   them; special roles take their tokens. A sole regular line takes
   `single`, the single-line colour (R3-3). (The book's cheat sheet, s.
   0201, keeps East blue and Central orange with Central on top at the
   end: it colours in data order; the end order here follows this section,
   and `slot` gives a document its fixed colours, R3-24.)

### 6.6 Inset charts

Both uses keep the full chart from zero and add a magnified view (R2-18,
R2-20, R2-23, R9-6, R10-4). No broken scales, no log scales. The book
singles the device out because software lacks it (p. 80, s. 0077): here it
is built from the data by `layout/inset.py` (inset stage), the main chart
and the inset's small chart laid out by the type's own layout.

**Decision** (`inset.kind: auto`, the default; 08-12 DP-5, N thresholds;
guided, G2.7, G2.26, D-11):

```
narrow   = vmin > 0 and vmin >= spread and (vmax - vmin)/vmax < 0.2
outliers = sort values descending v1 >= v2 >= …; the first k in 1 … floor(n/3)
           with max(rest after the top k) < 0.1 × v_k (values of every series)
if the main scale does not start at zero:            none (nothing to magnify)
elif facts.small_differences_matter and narrow:      NARROW_RANGE
elif facts.small_values_matter and outliers:         OUTLIER (members: the rest, or inset.members)
else:                                                none
```

The thresholds only raise the question the brief answers (D-11): where the
data are narrow or have outliers and no fact says so, the report carries an
info (R2-23, R2-20) naming the fact that would add the inset (inset stage;
was: a warning). An explicit kind the procedure would not give, or `none`
where it gives one, is a guided finding; an outlier inset asked for where
nothing dwarfs the rest, without `inset.members`, is a warning and the main
chart is drawn alone.

**Scales** (6.1): an outlier inset from zero on its members; a narrow range
truncated (bars: the largest step below the lowest value). One scale per
panel. The labels are formatted as a set (6.2); the unit is restated only
if needed (6.4, G2.27): where the main scale's unit writes a currency, a
magnitude or a suffix.

**Two forms** (inset stage):

- *Region*: an outlier inset over categories (bars or dots) re-plots its
  members only, in the plot's largest empty area (s. 0076, s. 0304).
- *Band*: a narrow range, or an outlier inset over time or on lines, whose
  marks cannot leave their axis, re-plots every mark at the main chart's
  positions (s. 0077, s. 0081, s. 0082, s. 0297); values outside its range
  are clipped at its plot.

**The inset's small chart** (both forms): the type's own layout on a copy
of the spec (the members, or every category, in the main chart's order,
`sort: given`; no title) and a context whose style scales every length tied
to the text (an em token) by `--inset-text-size` / `--label-size` and keeps
the marks' own sizes (the book's inset dots and lines are the main chart's,
s. 0081, s. 0082), and whose batch measures every text with `inset-text`
first, so that the layout decides at the sizes bake sets; every text then
carries the modifier. The layouts take hooks from `ctx.hooks` (empty
without an inset, so every other chart is laid out as before): `plot_left`,
`plot_right`, `slot` (lines), `band` (horizontal bars), `plot_height`,
`plot_height_factor` (a compact main chart), `fade`, `title` and `axis`
(False: no category or time labels, an inset sharing the main chart's
axis), and `inset` (True: the small chart's own layout, whose rows are not
fitted to its names, 6.7.3) and `event_labels` (False: a date marked on the
axis drawn as its line alone, its label the main chart's, 6.13.12); the main
chart of an inset on the members' rows takes `foot` (the room under its
rows, region step 6). The inset
layout's own decisions stay in a scratch report; its refusals become the
chart's warnings.

**Region inset** (outliers over categories).

1. *Source frame* (`inset-source`): a rounded rectangle round the members'
   marks: across them `--inset-pad-x` band beyond the first and last member;
   along them from `--inset-pad-y` (0.4 em) past the zero line to 0.4 em
   past the tallest member mark; kept inside the figure.
2. *Empty rectangle* (SPEC's step 4): obstacles are the boxes, padded 0.5 em,
   of every main mark (bars as their full rectangles, dots), every main text
   and the source frame. The region is the plot, extended to the figure's
   right edge (inset stage: the book's inset reaches past the plot, s. 0076).
   Candidate x edges are the region's and every obstacle's; for each pair
   xa < xb, the free vertical intervals left by the obstacles overlapping
   (xa, xb) give rectangles; the largest by area meeting the size bounds
   wins (width ≥ the inset's label column + members × 1.6 inset em + 2
   margins; height ≥ 6 em).
3. *Inset frame* (`inset-frame`): the rectangle less 0.5 em on the sides
   that face an obstacle (the sides on the plot's top and the figure's
   edge stay, so the frame's top is the top gridline and its outer edge is
   flush with the source frame's); its near edge at least 15 % of the source
   frame's span inside it, so that the cone visibly widens (s. 0076); height
   at most 0.8 × width, then fitted to its chart (inset stage).
4. *Content*: laid out at the frame's width less 2 `--inset-margin`, its plot
   height (vertical) or row pitch (horizontal, at most the main chart's)
   chosen by a trial layout so that it fills the frame.
5. *Links* (`inset-link`): the frames are rounded rectangles; take the
   convex hull of their outlines (the corners as arcs of 8 segments); the
   two hull edges that join a point of one frame to a point of the other are
   the links, from the source to the inset (a widening cone crossing neither
   frame). If one frame holds the other, the layout has failed: an error.
6. *On the members' rows* (evaluation round 1, B06: one venue's 1.84
   million visits flattening nine others, where both generator arms widened
   the chart to 480 pt): where no empty area holds the inset, or the area
   left or its chart does not fit, an inset over horizontal bars whose
   members are the last rows, under the outliers (the default sort), stands
   beside the members' bars, on their rows, the two charts sharing the names
   at the left, drawn once: the owner's exception for an aligned inset when
   space is short (one combined chart, labelled once), and how the best
   answer to B06 drew it. Its small chart is laid out with the main chart's
   rows (`band`), no names (`axis` false) and its plot from its left edge;
   its scale and its title go under its last row, inside the frame, so the
   main plot runs on under its last row by as much (hook `foot`, the
   gridlines with it). The source frame is the region's; the inset frame
   runs from the source frame's top, under the outliers' bars, to the foot
   of the main plot, from `--inset-gap` right of the source frame (and of
   any note on the members' rows) to the figure's edge (or a text right of
   it, a table's column); its gridlines start inside it. The links join
   the facing corners, as beside a narrow range (step 6 of the band inset).
   The decision says the main plot's extra height. Where the members are
   not the last rows, over columns, or where the inset's scale would not
   fit beside the source frame, the inset is not drawn: a warning (6.6)
   giving both reasons and the remedy, a wider chart, and the main chart
   alone (G2.29's places below, beside or above the chart are not built
   yet).

**Band inset** (narrow ranges; outliers over time or on lines).

1. *Where* (G2.29): a vertical chart takes the inset above it (s. 0081,
   s. 0297), unless the framed band lies in the main chart's lower half (an
   outlier inset's band from zero), where the links would run the chart's
   height: then below (s. 0077); `position: above` or `below` is honoured
   (s. 0082). Horizontal bars take it beside them, on the same rows
   (`right`; inset stage: an inset above horizontal bars could not share
   their rows). The main chart is laid out once to learn its orientation,
   at the width it will have.
2. *Above or below*: the inset frame spans the figure's width (its stroke
   inside the figure), the inset's content inside it by `--inset-margin`.
   The source frame spans the width less that margin on each side, and the
   main chart is laid out at the width less `--inset-pad-y` more, then
   indented, so the frame surrounds it without cutting a text (inset stage:
   was "across the full plot width"). Both panels share the plot's left
   edge (the wider of their label columns: the main chart is laid out again
   when the inset's is the wider) and its slots or bands, so marks stand at
   the same x in both; the inset's plot is `--inset-height-ratio` of the
   main plot's height before `compact_main` halves it (s. 0081), and at most
   `--inset-max-height` (1) of the main plot's height as drawn (owner, 30 Sep
   2026: an inset is at most about as tall as its main chart; P1-19's was
   twice its compact main chart, the links running the figure's height).
   *Labels* (D-56, guided): the inset repeats its category or time labels by
   default (G2.27); an inset aligned above or below its main chart, when
   space is short, drops them, and the two panels form one chart on a shared
   axis, labelled once, as the book's merged charts (p. 68–72, s. 0067–0070)
   and its compact inset (s. 0081) are. The axis kept is the main chart's, at
   label size and outside the frames: at the foot of the stack under an
   inset above (as the merged charts and s. 0297 have it), between the
   panels over an inset below (as s. 0082, left, has it); s. 0081 labels its
   upper panel, the inset, instead, and s. 0082 (right) repeats the days in
   both panels. Space is short (`share_axis: auto`) when the spec asks for a
   compact main chart, the book's compact layout (02 §5.4 d.8), or when the
   figure labelled twice would be taller than `--inset-share-height` (0.9) of
   its width, about half the paper's text block at the text width. Beside
   horizontal bars and for an outlier inset over categories the labels are
   repeated (the inset is not aligned above or below), except an outlier
   inset set on its members' rows when no empty area holds it (region inset,
   step 6), which shares the main chart's names.
3. *Source frame*: over the main chart's band of gridlines that holds the
   inset's range, expanded outward to the nearest main gridlines (230–280 on
   0–300 by 50 gives 200–300, s. 0297), `--inset-pad-y` beyond them, and
   further to take in whole any text an edge would cut (the band's labels,
   a unit line, end labels).
4. *Vertical placement*: the title at the figure's width; above: the inset
   frame's top where the main chart's first line would begin, the source
   frame (or the main chart's top, if higher) `--inset-gap` under the inset
   frame; below: the main chart under the title, the inset `--inset-gap`
   under it. The figure's height takes in both frames.
5. *Links* (inset stage: not the hull): from the facing corners of the two
   frames, each from the midpoint of one corner's arc to the other's (the
   book's trapezoid, s. 0297; 08-12 §5.4). Above: the source frame's top
   corners to the inset frame's bottom ones; below: its bottom corners to
   the inset frame's top ones, running down the margins beside the main
   chart, clear of its labels (the book's links, s. 0082, cross them). The
   hull's edges would here run along the source frame's sides.
6. *Beside horizontal bars*: both panels' rows at the same heights, the
   inset's row pitch the main chart's; the names repeated at inset size; the
   width left by the names, the frames and the gap shared between the two
   plots as their scale labels need it (at least `--time-label-sep` between
   neighbours) and the rest in proportion, a warning when they crowd; the
   source frame over the band across the rows, its bottom edge between the
   rows and the scale labels, its right edge past the band's top label; the
   links from its right corners to the inset frame's left ones.
7. *Truncation mark for bars* (R9-6, G2.28): the inset's bars run
   `--inset-fade` × bar width past the lowest gridline (the category labels
   below them) and fade to transparent over that stretch: the bars' group
   gets class `inset-fade` and `mask="url(#inset-k-fade)"`, a mask of a white
   rectangle over the plot up to the lowest gridline and a rectangle filled
   with a `linearGradient` (userSpaceOnUse, white at the lowest gridline to
   black at the fade's end), in `chart-defs`. Everything above the lowest
   gridline is drawn exactly; nothing below it pretends to be a length.
8. *Clipping* (G2.28): lines and dots are not faded; where a mark leaves the
   inset's range its marks group gets `clip-path="url(#inset-k-clip)"`, the
   plot (widened by half a highlighted line or a dot for lines and dots), so
   a line visibly leaves through the edge (s. 0077). The inset's extent (and
   its frame) is that of its texts, rules and plot; the model records the
   clip, and the line lint tests only what is drawn.

**Order** (4.2): links, source frame, inset frame (filled with `--page`),
inset content. The inset group follows the main panel. **Report**: the
decision and the place (R2-20 or R2-23, guided), the form (6.6, locked,
G2.27), the fade or clipping (R9-6, locked, G2.28), and `summary.inset`
(kind, position, scale, band, frame and source boxes). `chartgen choose`
shows the decision and the scale; the place waits for the layout.

**Checks** (inset stage): the SVG lint's `insets` (3.6) and `unit_phrase`
for inset scales; the model lint's dot check counts the main chart's dots
and the inset's apart; the line check clips an inset's paths to its clip.

### 6.7 Bar geometry

1. **Orientation** (`auto`; R2-04, R9-10, R2-37; owner, 30 Sep 2026): time
   x → vertical, always. Otherwise each category name is centred under its
   bar and **may reach past its band**, as long as the clear gap to each
   neighbouring name is at least `--category-label-sep` (1 em: the gap must
   read wider than the spaces inside a name) and the outer names stay inside
   the figure. An outer name that would reach past the figure's edge pulls
   the plot's edge in by as much (at most half a band; the gridlines then
   stop short of the edge). If any two names would come closer, **every
   multi-word name** wraps onto two lines, so that the row reads evenly
   (2, below); then the test runs once more, and if names still meet, or
   n > 12 (N), the chart turns horizontal, a guided decision (R9-10) naming
   the pair. Guided (1.4): an explicit orientation the procedure would not
   choose is a guided finding; an explicit `vertical` whose names still
   meet is refused (R2-08, locked: text is never rotated or overlapped). At
   10 pt and the text width, P1-06 and P1-08 turn: "Rockies" and "Midwest"
   are single words 7 pt apart, and "N. England" and "Gr. Lakes" do not
   break; P1-06 keeps its columns from 480 pt, as the book draws it (p. 327,
   s. 0298).
2. **Wrapping** (owner, 30 Sep 2026): a name breaks only at a space, where
   the book would break it: never after an abbreviation (a word ending in
   ".": "N. England" and "Gr. Lakes" are unbreakable), never just before
   "&" nor so that a line is "&" alone ("Finance & / Admin", never "Finance
   / & Admin"); a single word, hyphenated or not, is never broken; the break
   chosen balances the two lines (the wider as narrow as possible); the
   same rules break titles; never more than two lines (R5-26); never rotated (R0-06). A
   name that fits on neither line count in the horizontal layout's label
   column (at most 40 % of the width) is an error.
3. **Bands**: vertical — band = plot width / n; horizontal — band =
   `--bar-row-pitch`, or plot height / n when `layout.height` is set. Bar
   thickness = `--band-fill` × band, centred; the outer bars keep half a gap
   to the plot edge (R9-8). Beside horizontal bars, neighbouring names keep
   `--category-label-sep` (1 em) clear across the rows, ink to ink, as names
   under columns do along them (owner, 30 Sep 2026: the gap must read wider
   than the spaces inside a name, here the leading between a name's two
   lines; evaluation round 1, B01, where two wrapped names met at 368 pt).
   One-line names keep the usual rows (they stand about 11 pt apart at
   `--bar-row-pitch`). Where wrapped names would come closer, the name
   column first widens past 40 % of the width, at most to
   `NAME_COLUMN_MAX` (half of it, O), to the width at which the fewest
   crowding names wrap (a name only a little over 40 % stands whole); then
   the rows open up, all alike, to the least pitch that keeps every
   neighbouring pair apart (`bars.rows_apart`). Both are guided decisions
   (R5-26) giving the widths and the pitch. A layout whose rows are set by
   hooks (an inset's small chart, a panel) is not refitted; the panels of
   small multiples of horizontal bars open their rows by the same test
   (6.10).
4. **Clustered** (phase 2): cluster = `--cluster-fill` × band, bars inside
   touch (`--cluster-inner-gap`), one bar per series in series order.
   (Series, as built.) The bar is min(`--cluster-fill`/k, 1/(k +
   `--cluster-min-gap`)) of the band, so the clusters stand at least a bar
   apart (G7.5s): 1 bar with two series, 1.2 with three, 2.4 with six (the
   book: 1.0, 1.0, 2.25). Horizontal clusters keep a single bar's thickness
   (`--band-fill` × `--bar-row-pitch`) and widen the band. Each regular
   series takes the palette slot of its place among the regular series (or
   its fixed `slot`, R3-24), a special series its reserved grey; one series
   is `single`, and its categories' roles keep their greys. A highlight
   reference names a category (every bar of it), a series (every bar of it)
   or both (one bar). How the series are named (guided; G5.1, G5.7, G5.8,
   G5.10, G5.14; D-7, D-31), by the component `layout/legend.py`:
   - columns: a legend row above the plot, below the title (the top scale
     line then a `--title-gap` below its last row), centred over the plot,
     its entries in the order of the bars, wrapped into a grid filled row by
     row when wider than the plot, each keyed by a `--legend-key` square;
     `labelling: direct` draws the book's messy version, each series named
     beside its bar of the last cluster on a leader, the plot narrowed for
     them, with the guided finding R4-2;
   - horizontal clusters: their colours run top to bottom, so each series is
     named beside its bar of the last cluster, left-aligned a gap past its
     end, where the names clear everything and stay in the plot (D-31 (c));
     else, or with `labelling: legend`, a legend column right of the plot,
     centred on it (D-31 (a)); with a table at the right, a legend row
     above;
   - the entries of both legends are `legend-entry` groups, as a line
     chart's are, so the order can be read off the drawing.
   A value not known in a cluster keeps its slot empty and its note centred
   on the slot, raised above the bars of the cluster that it spans. No inset
   on clustered bars in this stage (CG-3).
5. **Length**: from the zero line to the value; negative values hang below
   (or left of) the zero line, which then takes `gridline zero signed` (also
   on `change` and `index` scales, D-17 (b)). Only an overridden truncation
   (the book's bad version, rebuilt) moves the bars' base to the scale's end
   nearer zero. A known zero is a bar of no height with its "(zero)" note.
6. **Category labels**: vertical — centred on the band, cap top
   `--category-gap` below the zero line (or the plot bottom when negatives
   exist), two lines at `--line-pitch`; horizontal — right-aligned at plot
   left − `--category-label-gap`, the block's cap-height centre on the bar's
   centre.
7. **Sort** (02 D1): nominal by value of `sort_by` (default the series the
   insight names, else the first; D-46), descending (ascending with `sort:
   asc`); stable for ties; `all_others`, then a `total`, then unknown values
   last (R9-9; the total closes a breakdown, G7.3); ordinal and time in
   their order.
8. **Axis title** (R2-03): only if `x.title`, centred under the labels.
9. **Height**: vertical plot height = `--plot-aspect` × plot width.
10. **Dots** (stage 2; G7.18, G1.49, G1.57): a dot plot of one series is
    laid out by exactly these steps, with a dot centred where each bar would
    end (the band's centre, at the value) instead of the bar: the same order,
    orientation and names (1, 2, 6, 7), the same value scale with its unit
    phrase or, horizontal, its title below (6.4; the inline phrase is tested
    against the dots), the same time axis (6.3). The dot is the library's
    `mark-dot`, `--dot-size` across over time and `--category-dot-size` over
    categories, every dot of one size and colour; the plain style fills the
    first and opens the second (s. 0163, s. 0082, 0085). No guide lines run
    from the names to the dots (neither the guide nor the book's figures draw
    them). The scale is the non-length one of 6.1: a theoretical minimum
    below the data, or truncated when zero is arbitrary or declared
    irrelevant; otherwise zero, with an inset if small differences matter.
    Several series (a legend or direct labels) are phase 2. (Series, as
    built.) The dots of a category share its position (no cluster), each
    series in its own slot, one size and one form per chart; a value not
    known is named in one note per category ("(2021 missing)"). Upright
    over categories: each series named beside its dot of the last category,
    packed in order one pitch apart, a leader past one pitch, the plot
    narrowed for them (the test of D-7); where the packing fails, or with
    `labelling: legend`, a legend row above. Turned: a legend row above.
    Over time: a legend column at the right, beside the latest values, its
    entries in the order of each series' last known value (G5.9, G5.11),
    since sparse series end at different periods. Keys are the dots
    themselves (`dot-key`, G5.14).
11. **Flat reference markers** (series; R11-3, R11-4, G1.60, G5.5, G7.57,
    D-7, D-21). A series of role `target` or `reference` beside one bar
    series is drawn as a mark across each bar: as wide as the bar,
    `--target-mark` of it thick (at most `--target-mark-max`), centred on
    the reference's value on the bars' one scale from zero (the scale is
    computed over every series, the reference included), classed
    `target-mark reference` with the bar's colour class and painted in that
    colour's strong shade (the dark grey on a grey bar), drawn after the
    bars. The bar series and the reference are named once (G5.5): beside
    the first bar, each name left-aligned a leader's run past the bar and
    packed in the order of the bar's end and the marker, on thin grey
    leaders; where the names would meet anything, the chart is laid out
    again with room at the right and they stand beside the last bar (the
    book's s. 0321); else a legend row above. Horizontal bars: beside the
    first bar, else a legend row. A highlighted bar under a marker shares
    its strong shade (reported, CG-3); references on clustered bars or on
    dots, and more than one, are later stages.
12. **Value labels** (series; R2-31, R2-32, G2.18, G7.4, G6.16; 02 D8,
    §5.4 (c)). `value_labels: auto` labels the values when the chart has
    fewer of them than its scale would have labels (the known values of
    every bar series against the ticks) and the labels keep apart over their
    bars (a label no wider than its bar's pitch, less `--text-pad`; else the
    scale stays, a guided decision); `on` and `off` against that are guided
    findings (R2-32), and `on` whose labels would meet over the bars is
    refused (CG-1: text is never overlapped, as with names under columns,
    R2-08), an override drawing the book's version with each collision
    reported. The labels are one set (`fmt.format_values`: one
    magnitude, one number of decimals, the fewest that tell the values
    apart, the prefix, magnitude and suffix on each: "$54", "−$16K"). Each
    sits just inside its bar's end in the text grey where that reads at
    4.5 : 1 on the bar's fill and fits, else just outside (above a column,
    below a negative one, past a horizontal bar's end), `--value-label-gap`
    from it; white is not offered (no style colour for text on a mark), and
    in the plain palettes every label sits outside. The scale's labels and
    gridlines go, the zero line stays as the baseline (signed when negative
    values are drawn), and the plot takes the label column's room; the
    unit's description (`sl.unit_line`: `unit.fallback`, else the noun and
    `what`) stands on its own line above the plot. A chart laid out under
    hooks (an inset, a panel) keeps its scale unless a hook
    `value_labels` says otherwise; references keep the scale. The lint's
    phase-1 info (R2-32) is left to this decision for bars. The phase-1
    bar examples whose values would now be labelled (P1-01, P1-02, P1-14
    `bars`, P1-15, P1-17, P1-18, P1-29) keep the book's scale with the
    reason in their specs, so their sources are unchanged.
13. **A table with the graph** (series; R10-7, R2-31, G1.14, G7.41, G7.46,
    D-52). `facts.exact_values_needed`, when the values are not labelled
    (02 D8), puts a table under a vertical chart: one row per series (the
    reference included), its name right-aligned in the scale-label column
    (widened if the name needs it), one column centred under each category
    or cluster, `--table-gap` below the last row of names and
    `--table-row-pitch` apart, no rules, the numbers bare (the unit is the
    scale's), grouped and with one number of decimals for the whole table,
    a value not known as its note. Cells that would come within half an em
    of each other leave the table out with a warning (D-52 (iv): the
    transposed table beside the graph is a later stage). Beside horizontal
    bars: one column per series right of the plot (`--table-col-gap`
    apart), each right-aligned under its heading (`column-header`, its
    baseline `--table-header-drop` above the plot), each row level with its
    category; the plot narrows for it.
    **Row keys** (`table.row_key`, under a vertical chart). The default,
    decided by the owner (1 Oct 2026): under several series (a reference
    included) each row is keyed by its name and then its square
    (`swatch_after`), and the legend is dropped, the table being the key
    (`table.legend: keep` restores it; P2-01 `legend`); under one series
    the rows are not keyed. `swatch_after` (owner, 1 Oct 2026): the names
    stay exactly where they stand without keys, right-aligned on the scale
    labels' right edge, so the label column is no wider than with `none`;
    each row's `--legend-key` square stands on the value axis, its left
    edge on the gridlines' left end (the plot's left edge). The
    name-to-square gap is so the whole `--scale-gap` (9.0 pt in P2-01), and
    everything lines up with something already there: the names with the
    scale's labels, the squares with the axis. The value columns stay
    centred under their clusters; the report gives the square's distance to
    its name and to the first column of values (P2-01: 9.0 pt and 8.9 pt,
    about even), and a square clearly nearer the values than its name (by a
    fifth) is warned about (CG-1), the names never moved to mend it. The
    other keys, for comparison: `swatch`, the square in a column at the
    table's left edge; `swatch_beside`, the square `--key-gap` left of each
    right-aligned name (ragged); `text`, each name in its series' strong
    shade (`text-series-k`: 3.63, 4.52 and 3.78 : 1 on white for slots 1–3,
    so slots 1 and 3 fail G6.5, and three more text styles, R5-6); `pill`,
    each name on a rounded pill of the series colour `--key-tint` (0.6) of
    the way to the page (`key-pill`; the text grey on it 5.31, 4.68, 5.08 :
    1); `halo`, a flat halo of the same tint round the letters (a copy of
    the name, `text-halo`, in a `key-halo` group that the stylesheet fills
    and strokes `--key-halo-width` wide, the name drawn over it). The rows
    keep the series' order, which is the order of the bars in each cluster
    and of the legend. `table.legend: drop` (the default's `auto` when the
    rows are keyed) removes the legend, the table then the key below the
    plot; asked for without keys it is warned and the legend kept.
14. **The legend component** (series; `layout/legend.py`): `row(entries,
    style, batch, max_width)` and `column(entries, style, batch, pitch)`
    lay out entries (`Entry`: series, name, colour classes, a key of kind
    `square`, `line`, `dotted`, `dot` or `marker`) in local coordinates,
    returning keys and names apart (to place and shift) and as
    `legend-entry` groups; `side_labels(anchors, …)` packs names beside
    their marks (packing.py) with leaders where they moved, or on every one,
    and reports D-7's test. `column` reproduces a line chart's legend key
    for key and name for name (tests/test_clustered.py), so lines.py can
    switch to it by replacing its legend loop (the recipe is in the
    module's docstring); the line's black-and-white symbol would become a
    key kind.

### 6.8 Missing values

The data model keeps three things apart: a known value (0 included), a
missing value (`null`), and a status. There is no coercion to zero anywhere
(R6-23); `scales.py` ignores unknown values; the lint compares the drawn
marks with the data.

- **Bars**: no bar; a `missing-note` with the status text ("(missing)",
  "(unknown)" …; `labels.notes`) centred on the band, its baseline
  `--missing-note-lift` above the zero line (vertical), or left-aligned 0.3
  em right of the zero line, centred on the band (horizontal). A known zero
  gets the note "(zero)" (R0-12, R9-7). Unknown categories sort last.
  **Where a vertical slot's note goes** (owner, 1 Oct 2026; the logic of the names under vertical
  bars, wrap first; `bars._slot_note`, `_resolve_notes`; bars, clusters, stacks, stacked areas
  and dots alike): (1) wrapped to at most two lines by the names' break rules (common.splits:
  at a space, never after an abbreviation or before "&"), in its empty slot on the baseline,
  where it fits the room the marks either side leave (bars and stacks: their facing edges, an
  area's bands ending at their slots' centres; dots, which have no mark at the baseline: the
  slot less the names' 1 em), `--text-pad` clear of each; one line as before where it fits;
  (2) where wrapping is not enough and nothing stands beneath the axis's labels (no axis title,
  no second time row), under its slot's label, below the row's lowest line, wrapped to the slot
  where it can be, provided it meets no other text; (3) otherwise (an axis title or a second
  row, a slot whose label was thinned away, a cluster's bar, which its category's label does
  not name, or a note under the label that would meet another text) above, over every mark it
  would cover, a thin `leader` down to the slot's centre on the baseline. A guided decision
  (R9-7) names each note's step. Every outcome keeps the note centred on its slot, which is
  how the SVG lint's R9-7 finds it. Thin bars on a true time axis have no slots: their notes
  stay on the baseline at their instants; lines leave a gap and take no note (R6-22).
- **Lines** (06 DP6-4): split each series into maximal runs of consecutive
  known slots; each run of 2 or more is drawn, one path per stretch of equal
  certainty: `line` where both ends of a segment are actual values,
  `projection` (dotted) where either end is a projection, estimate or
  target, so a projection starts at the last actual value (R6-24, R6-33); a
  `target` or `reference` series is dotted throughout (D-21 (c)); nothing,
  solid or dotted, ever touches a missing slot (R6-22). Markers (`point`, on
  every known point of the series, twice the series' own line width across,
  G7.11s) when a run has length 1 (R6-25), or when a series with a gap has
  three known values in a row perfectly straight, equal steps, which could
  be taken for a join across a missing value (R6-26; the cheat sheet's
  example, s. 0201). Otherwise none (R6-27); `markers_on_gaps` is not a
  phase-1 option (Appendix A). (Stage 2, lines: the notes' visual test, a
  vertex within 0.5 × line width of its neighbours' chord, N, marked the
  lines of nearly every approved figure (P1-09's Consumer, all four series
  of P1-21), and exact straightness alone still marked P1-11's West (40,
  44, 48) and P1-12's Int'l (18, 19, 20), transcriptions of figures that
  have no markers; the reading above reproduces every figure. For the
  owner to confirm.)
- **Sparse series**: `enough_values` fails → the chooser gives a dot plot
  (R6-2), whatever the message; a state series, known between its changes,
  never (D-27); the axis still spans the whole period so the empty days show.
- **Dots**: no dot; categorical dot plots get the note, like bars; a known
  zero is a visible dot and gets no "(zero)" note; a time dot plot's empty
  slots are its point and get no notes (stage 2). The lint checks one dot
  for each known value and none for a value not known.
- **End labels**: at the last known point (6.5).
- **Incomplete periods** (phase 2, 02 D10): omitted by default; drawn as an
  isolated `point` (lines) or a dotted level mark (bars) with "As of Dec.
  10" when requested. Stage 2 (lines) omits a first or last incomplete
  period from line charts (6.3.6); the drawn forms and bars wait. (Time:
  built for lines and bars, "requested" being `facts.incomplete_value_needed`,
  6.13.5; a time dot plot leaves the period out.)

### 6.9 Highlighting

R3-7, R6-28, R6-29, R15-5, R15-6.

- **Targets**: `highlight[].targets`, or `insight.targets` when `highlight`
  is absent but the insight names elements (13-15 lint 2 then requires a
  highlight).
- **Method `auto`**: bars and dots → `strong` (the slot's strong token, same
  hue, darker; an open circle's ring takes it, stage 2); lines → `thickness`
  (`--line-width-highlight`) and `strong` (R6-29); their end labels
  `text-strong`.
- **Others**: keep their colours and widths (R6-28: never grey out crossing
  lines). With `dim_others` (bars only, 07 §5.1), non-targets get `context`,
  which the stylesheet maps to `mix(base, white, 0.4)`; since D-9 keeps the
  others unchanged unless the message is a subtotal, `dim_others` is a
  guided finding, as is `bold_label` on bars (stage 1).
- **Forbidden** (refusal R15-6): a different hue for the target; a bigger
  bar, dot or text size (line thickness is the one size change the book
  allows); `thickness` on bars and dots is refused (stage 2: dots too). The
  lint also checks that every dot has one size. `grey_others` is refused
  (R6-28) unless overridden.
- **Lines** (stage 2, lines): `auto` and `thickness` draw the target twice
  as thick in the strong shade of its hue (`highlight`), its end label
  bold; `strong` on a line is drawn the same way, with an info (CG-3: the
  one line highlight of this stage); `bold_label` bolds only the label, a
  guided finding; `dim_others` pales the other lines (`context`), a guided
  finding (D-8: lines keep their colours); `grey_others`, through its
  override, draws the others in `uniform-context` and the target in its
  own colour and width, as on s. 0197.
- **Order**: targets are drawn after the other marks of their kind; on a
  line chart, paled or greyed lines first, then the others, then the
  highlighted, then the reference series, then every marker (4.2).
- **Phase 2**: `border` (pie slices, bars), `glow` (an underlay stroke 4 ×
  the line width, 13-15 §5.4, no filters), `background`, `icon`,
  `selective_label`, the title connector and comments. (Message: built as
  6.12.4 and 6.12.5; the glow along a line is three times a highlighted
  line, `--glow-line`, flat and opaque beneath the gridlines.)

### 6.10 Small multiples and merged charts (phase 2; as built, "(panels)")

RP3-3–RP3-5, RP3-8, RP3-9, R6-36, R6-37, R2-13, R2-14, R2-16, R9-11; 13-15
DP-1, §5.8; 02 D7, §5.3 "merged panels"; 08-12 §5.3; D-28, D-38. Built in
`layout/multiples.py`, entered from `render` when the spec has `data.dims`
or `data.measures` (a hook before the type's own pipeline); each panel is
laid out by its type's own layout (bars, dots, lines) on a copy of the spec
holding its series, through the hooks the inset stage added (`plot_left`,
`plot_right`, `slot`, `band`, `plot_height`, `title`, `axis`), so that the
layouts need nothing new.

**The spec** (panels). Small multiples: `data.dims` = [{`id`, `name`,
`kind` nominal | ordinal, `values`}], the breakdown variables besides x;
each series names its value of every dim in `series.keys`. Merged charts:
`data.measures` = [Measure with `id` and an optional `heading`], each series
naming its measure in `series.measure`; `data.measure` is then unused.
`facts.compare` lists the dims whose values the insight compares directly;
`facts.compare_shapes` says the message compares the shapes of series of
very different size. `panels` = {`arrangement`: `auto` | {`rows`,
`columns`, `within`}; `scale`: `auto` | `shared` | `per_row` |
`independent`; `series_labels`: `auto` | `after_last` | `first_panel` |
`legend`}, all guided. A reference may name a panel (`{"panel": value,
"category": …}`, §2.8): it applies in the panels whose row or column is that
value. Structural errors (CG-4): keys missing or foreign, two series with
the same keys, a series without its measure, a dim placed twice or not at
all, `measure` as a dim's id.

1. **Arrangement** (guided, RP3-3, RP3-4; G1.18, G1.19; D-38 (i)). The dims
   in `facts.compare` are drawn as series inside every panel; the panels
   facet on the others: none, one panel; one, a row of panels, or a stack
   of rows when it has more than 6 values (O; the book's rows hold 3–6, its
   stacks 12, R6-36); two, a grid, the dim with more values across (at most
   6), the other down. Without `facts.compare`: one series per panel, the
   most panels and the least clutter (DP-1), with an info asking for the
   fact. The report names the runner-up along the book's spectrum (many
   simple panels … one busy chart) for review; the examples render it as a
   variant. A merged chart's measures are its panels: stacked over time,
   side by side over categories. More than two faceted dims, or dims with
   several measures: a later stage (CG-3). The values of a nominal dim keep
   the spec's order (the examples give them in order of their totals).
2. **One plot for several measures is refused**: the measures within one
   panel is a dual axis (R2-14, G2.31 [L]), or one scale for them with
   `panels.scale: shared` (R2-13, G2.30 [L]); a merged chart with
   `panels.scale: shared` is refused likewise. The supply names the merged
   chart (actual values) and the index chart (patterns of change, a
   transform).
3. **Order and colour.** Nominal categories take one order in every panel:
   by their total over all panels (small multiples), or by the measure the
   insight names, else the first (merged, D-46), largest first (02 D1,
   G2.1). With series inside the panels, each takes one palette slot in
   every panel, handed out top to bottom by the line ends of the labelled
   panel (R6-28, G7.50), except where it is fixed: by its series' `slot`, or
   by `colours.slots` or the document's registry under its label (its values
   of the dims inside the panels; 2.10). Fixed slots hold in every panel and
   in every chart of the set (R3-24, G4.22 [L]; evaluation round 1, B11,
   where the slots were ignored and two centres swapped colours between the
   author's two charts); the others take the free slots in the line-end
   order. Two labels given one slot, or one label two, keep the first, with
   a warning (R3-24). With one series a panel, every mark is in the single
   colour (R3-3, G7.50).
4. **Type** per measure: the first panel's by the procedure (§3.2), the
   others checked against it (a departure is reported for that panel, a
   refusal refuses the chart).
5. **Scales** (guided, R6-37, G7.48; D-38 (ii)): `shared` (the default), 6.1
   on the union of every panel's values; `independent` when
   `facts.compare_shapes`, one per panel, and for lines and dots the
   distance to zero is then irrelevant (G7.48: such panels need not start
   at zero); `per_row` only when asked. A merged chart: one per measure
   (R2-16, locked). Each scale is the usual procedure's for its own data
   (6.1, the owner's step rule included; round 1, sonnet-G B02: a cap of
   five intervals had run the trips panel to 150 for data under 100, with
   55 % of room). Only a panel too short or too narrow for its labels takes
   fewer intervals, so that they stay one em apart: at most ph / em of them
   under a vertical scale, at most pw / (the widest of its own labels +
   `--time-label-sep`) across a horizontal one (a percentage's labels, with
   their "%", are wider than their neighbours'); it first aims at that many,
   then takes a larger step of 1, 2, 5 × 10ⁿ (O; it may leave more than 25 %
   of room, a guided R2-27 warning), and its decision says so ("at most 3
   intervals in a panel about 30 pt high"); the other panels keep theirs.
6. **Orientation.** Lines and time are vertical. Bars or dots over
   categories: the names are tried under the columns of a panel at its own
   width (the owner's rule on names under bars, R9-10); where they do not
   fit, every panel turns horizontal. Merged bars are always horizontal
   (G7.6 [L]: `orientation: vertical` is refused under R9-11).
7. **Vertical plots** (lines, columns, dots over time). Widths: the row
   headers' column (wrapped within a quarter of the width, plus 1 em), the
   scale-label column of each column of panels that shows one (the first,
   or every column with independent scales), then equal plot widths pw; the
   right reserve for series labels, and any axis label that would pass an
   edge, found by laying out and trying again. Heights: ph = max(
   `--panel-min-height` (3 em), min(`--panel-max-aspect` (1.5) × pw,
   plot aspect × c·pw / r)): the rows share a single chart's plot aspect,
   within those bounds (O). Labels once (G7.49 [L]): the scale labels only
   left of the first column; the unit phrase once, on the first row's top
   label or its fallback line; the other rows' top labels keep the number's
   currency, magnitude and suffix ("$60" and "K", the owner's rule), a count
   noun no more; the time or category labels only under the bottom row;
   column headers (`column-header`) centred above the top row, row headers
   (`row-header`) at the left, horizontal. Every gridline spans its panel.
   Rows stand `--panel-gap` apart between their texts; the first row's
   first line sits a title gap under the title (or under the column
   headers).
8. **Horizontal bars** (merged bars; multiples whose names do not fit under
   columns). The names once, right-aligned left of the first column (as
   bars.py sets them); the rows at `--bar-row-pitch`, the same in every
   panel, or further apart where wrapped names would come within
   `--category-label-sep` of their neighbours (6.7.3); equal plot widths, the gap between neighbours the halves of their
   facing scale labels plus `--panel-gap`; each bottom panel's scale labels
   below it. A shared scale's title once, centred under the row of panels.
   A merged chart: each panel's own scale from zero and a bold heading
   (`panel-title`, left-aligned on the plot, wrapped to its room, top
   lines level): the measure's `heading` (a question, RP5-6) over a title
   kept below; without one, the scale's title itself, set above as the
   heading, as the book sets merged bars (s. 0067, 0304) — the one place a
   horizontal scale's title is not below it (owner to judge, D-20).
9. **Merged time panels**: stacked, one column, no row headers; each panel
   its own scale with its unit phrase on its top label, the plots sharing
   their left edge and slots (label columns right-aligned to the widest);
   the time axis once, under the last panel (G2.32 [L]); a `heading`, if
   given, set over its panel.
10. **Separators** (small multiples only, `panel-divider`, the divider grey
    and gridline width; G7.51 [L]): between columns, from just above the top
    gridlines to just below the last row's plots, crossing no label; between
    rows, across the whole width at the middle of the gap. No box round a
    panel. A merged chart's panels stand apart by their gap (the book's
    merged charts have no separators).
11. **Series labels** (guided, RP3-9, G5.15; D-38 (iii)): series inside the
    panels are labelled once, at their line ends right of the last panel of
    the first row (packed, with leaders, or a legend by the line layout's
    own test); `legend` forces the legend there; `first_panel` is a later
    stage (CG-3). One series a panel needs none: the headers name it.
12. **Dates marked on the axis** (`data.x.events`, 6.13.12; evaluation round
    1, B11, where the competitor's opening could not be marked): the line
    stands in every panel of lines, at the same place, and its label once, in
    the first panel in reading order that holds it beside its line (hook
    `event_labels`: the other panels draw the line alone); a date whose label
    no panel holds loses its lines in every panel, with a warning (CG-1: a
    marked date is never left unnamed). A decision (6.13) names the panels
    and where the label went. Panels of other types (columns over time) do
    not draw them yet: an info (CG-3), never a silent drop.
13. **Checks**: RP3-5 warns above 5 lines in a panel unless they cross at
    most about once each and are labelled directly (D-28 (a), provisional);
    R6-36 warns above 30 panels (a heatmap then). The model lint runs its
    scale checks (breathing, intervals, trailing zeros, the unit's wording)
    once per scale with that scale's series, the others once on the whole.
    The SVG lint reads each panel apart (its gridlines, bars from zero,
    empty slots); in a chart classed `multiples` (one measure by
    construction) a scale without a unit passes when another scale of the
    chart states it (G7.49); in a chart classed `merged` a `panel-title` in
    the panel counts as its scale's title; `panel-divider` is a non-data
    line (R1-02); `headers` is a group of the vocabulary.
14. **Report**: the arrangement and runner-up (RP3-3), the colours (R6-28 or
    R3-3), the order (02 D1), the scale mode (R6-37) and each scale (02 D5,
    prefixed by its panel or measure where there are several), the panel
    layouts' own decisions once each (their scale and unit decisions from
    the panel that shows them in full), the grid (6.10, RP3-8, RP3-9; R9-11
    or R2-16 for a merged chart); `summary`: `arrangement`, `kind`
    (multiples, merged), `rows`, `columns`, `panels`, `scale_mode`,
    `scales` and `plots` by panel, `orientation`, `width`, `height`. The
    SVG: the root classed `chart multiples|merged <types> <orientation>
    <x kind>`, a `grid` group of separators, a `headers` group, then one
    `<g class="panel" id="panel-i-j">` per panel (its own grid, marks,
    scales, annotations, and a merged panel's `headers`).

### 6.11 Pie ordering and labels (phase 3)

07 §5.2, R7-32–R7-39, 03-04 §5.8.

1. **Gates first**: `parts.meaningful_total`, `all_parts`, no overlaps, one
   sign, parts ≤ 7 (6 fine, 7 warned; 8+ rerouted by 07 §4.1).
2. **Order**: descending (`asc` or `given` for ordered categories), then
   pinned last: `all_others`, then zero-value parts. Angles θ clockwise
   from 12 o'clock; slice i spans 360°·vᵢ/Σv; rim point (cx + r sin θ, cy −
   r cos θ).
3. **Colours**: slots in slice order (`series-1` for the largest), no
   borders; gaps as background-coloured radial strokes 0.7 % of the diameter
   (`pie-gap`) over the slices, or true gaps (wedges offset by half the gap)
   for styles where a background stroke would show.
4. **Labels** outside, never a legend, never over slices: name on line 1,
   value on line 2 (6.2 `format_values`), centred in the block; block centre
   at distance r + 0.1 r + h(θm) along the mid-angle ray, h(θ) = |w/2 · sin
   θ| + |h/2 · cos θ|.
5. **Collisions**: split into the right half (θm < 180°) and the left; sort
   each by y; pack each side like 6.5 with the block heights; a block moved
   more than half its height, or a slice under 8°, or a zero part, gets a
   `leader` (hairline curve to the rim point at θm, open head). A zero part's
   label sits outside the rim at 12 o'clock, on the side with more room.
6. **Highlight**: strong colour (others optionally dimmed), or a border of
   the strong colour 0.05 r wide clipped inside the wedge; never an exploded
   slice (R7-38); donuts are refused in favour of a pie unless overridden
   (R7-39).

**As built (phase 3, "(breakdown)"; `layout/pie.py`).** The gates of item 1
are the chooser's (07 §4.1, `choose.py`); the layout refuses only what the
drawing would get wrong, and rechecks one sign and known values. Departures
from the items above:

- **Refusals in the drawing**, each through an override to rebuild the
  book's bad version: `chart.type: donut` (R7-39; a ring whose hole is
  `--donut-hole`, 0.74, of the radius, M s. 0269; the book's total in the
  hole is words the generator does not write), `highlight[].method:
  explode` (R7-38; the slice pulled out a tenth of the radius along its
  middle ray, its label with it), `chart.labelling: legend` (R7-34; a
  legend row above the pie, the slices unlabelled), `thickness` (R15-6),
  `grey_others` (R6-28). A part whose value is not known is refused (R0-12:
  the total and every fraction would be unknown); parts of both signs
  (R7-04) where the chooser has not refused them; parts all negative are
  drawn by their size.
- **Order**: as item 2; with `sort: given` (ordinal categories, or an
  explicit order) a zero keeps its place. With `x.groups` (new: {category:
  group}), the groups largest first and each group's parts together in
  their given order (07 §5.2 "grouped", s. 0240).
- **Colours**: slots in slice order; `all-others` for All others; a group's
  first part the group's slot, its second `tint` (new tokens
  `--series-k-tint`, 20 % towards the page, M 07 §5.1; a third part shares
  the tint, with an info); a zero has no slice and no slot. With more than
  eight colours an info (R7-32), and the SVG lint's R3-22 warning, the
  slices being labelled directly. Ordered parts (one total: `x.kind:
  ordinal`; several: `data.parts.kind: ordinal`) keep their order from 12
  o'clock and take steps of the sequential ramp, `seq-9` (the first) to
  `seq-3` (the last), as a stack's (6.18; G4.7, R3-8: each slice still its
  own colour); more than seven, a hue each with a guided warning (R3-8). An
  ordered part is not highlighted (a step has no strong shade, and a darker
  step would read as another part's): an info (CG-3). Several totals keep a
  part's `series.slot` (R3-24), the others taking the free slots in slice
  order.
- **Gaps**: true gaps by default, not strokes (the notes' own advice, 07
  §6: offer gaps directly): each wedge's straight edges moved in by half of
  `--pie-gap` × the diameter (the model's `Wedge.inset`, its apex moving out
  along the bisector), no `pie-gap` element; a sliver narrower than its gaps
  is not drawn, its label and leader still name it.
- **Labels** (`slice-label`, both lines): the name wrapped past
  `--pie-label-width` (7 em, O) by `common.splits` (never after an
  abbreviation: "Mr. & / Mrs. Perez" where the book writes "Mr. & Mrs. /
  Perez"), then the value, or the percentage of the total with
  `labels.parts: percent` (new, open), formatted as one set by
  `fmt.format_values` (percentages: the fewest decimals that keep them
  apart); `labels.parts: name` leaves the value out. The block centre as
  item 4 with the gap `--pie-label-gap` × r (0.1, N).
- **Collisions**: each half packed top to bottom in the order of the slices
  round the rim (not by the ideal y, which near 12 o'clock does not follow
  the rim), blocks at least 0.5 em apart, then pushed clear of the circle of
  radius r + gap; blocks of the two halves that meet near 12 or 6 o'clock
  pushed apart sideways. A leader goes to a zero part (its label beside the
  rim's top, right of 12 o'clock, as s. 0266), a slice under
  `--pie-leader-angle` (8°, D-34) and a label moved more than half its
  height; such a label stands `--leader-run` further out sideways (its
  place down the page kept), and where a half has two or more, they and
  every label between them line up on their inner edges, each with a
  leader, so that no leader crosses a label or another leader. A leader is
  a `leader` curve from the side of its label facing the slice, arriving
  along the slice's ray, with the house arrowhead (`leader-head`, the
  comments' `bend` head at the leader's width), as the book's grey arrow.
- **Radius**: `--pie-radius` × the width (0.2, O; M ≈ 0.18–0.19 of the
  book's column), shrunk until the labels fit the width, down to
  `--pie-min-radius` (3 em); beyond that a CG-1 error. The pie is centred
  in the width where its labels allow.
- **Highlight**: `auto`/`strong` the strong shade (`highlight`), the others
  `context` with `dim_others` (a guided departure under R15-5 unless the
  message is a subtotal, D-9, as for bars); `border` drawn by the layout
  (annotate.LAYOUT_DRAWS leaves every highlight method in the spec a pie,
  treemap or wrapped bar chart sees): a wedge in
  `overlay`, classes `border <slot> highlight`, `--border-width` wide and
  kept inside the slice, the slice keeping its colour; the targets drawn
  last. `glow`, `background`, `icon`, `selective_label`, `bold_label`: the
  strong shade, with a CG-3 info. The message layer draws comments and the
  title connector on a pie (6.12.5, 6.12.6; P3-16).
- **Part limit** (D-23): the chooser's, not the layout's (a report names each
  rule once): seven parts a guided warning at the limit (R7-13), eight or
  more a guided departure of the type (R7-15). The layout reports only what
  the drawing decides; its refusals of mixed signs (R7-04) and of a donut
  (R7-39) stand only where the chooser has not already made them.
- **Several totals** (R7-21, G1.41; R7-24): read as the chooser reads them
  (07 §4.2, as for clustered bars): the totals are the categories (x), their
  parts the series. `chart.type: pie` with several series draws a pie per
  total in a row, of one radius, evenly spaced (one pitch between every two
  neighbouring centres, the second pie clear of the first's labels by 2 em,
  the row centred) where that fits at the least radius, else the first pie
  standing apart with its labels and the others 2 em apart (P3-11 at the
  text width), the totals in their order (sorted by the
  series the insight names, else the first, D-46, when nominal), the parts
  in the first pie's order by size and in one colour each in every pie;
  each total's name above its pie (`column-header`); the parts named once,
  round the first pie, by name alone (`labels.parts: auto`), where the book
  keys them in a legend (s. 0247; G7.27 never keys a pie); a highlight
  names a part by its series. The root is classed `chart pie categorical`
  (`donut` added for a donut); slices are `<path class="slice <slot> [tint]
  [highlight|context]">` with `data-series`, `data-x` (one total: the
  series and the part; several: the part's series and the total) and
  `data-value`, in a `series` group per pie (`data-total` on it when there
  are several); labels and leaders in `annotations`; `summary`: `kind`,
  `pies`, `radius`, `centres`, `order`, `labels`, `leaders`.

### 6.11a Treemaps (phase 3; as built, "(breakdown)")

07 §5.6, R7-15, R7-16; G1.39, G7.34 [L]. `layout/treemap.py`, `chart.type:
treemap`, one series (several: CG-3).

1. **Layout**: squarified (Bruls, Huizing and van Wijk), the parts largest
   first, the largest at the top left; with `x.groups`, the groups laid out
   first (largest first, All others last), then each group's parts in its
   rectangle. The plot is the figure's width by `layout.plot_aspect` (or
   `--plot-aspect`) of it.
2. **Gaps**: tiles (`<rect class="tile <colour>">`, `data-group` when
   grouped) inset by half of `--tile-gap` (1 pt, M) each side, groups by
   half of `--tile-group-gap` (2.5 pt, M 1–2 px); no outlines.
3. **Colours**: a flat treemap in `single` (R3-3; the book's #558BB9 is the
   strong shade here, its highlight); a grouped one a slot per group in the
   order of the groups (D-22 (a)); All others `all-others`.
4. **Labels** (`tile-label`, `--tile-text` #1A1A1A, O: the text grey is
   3.3 : 1 on the single blue, and the book's white on its #558BB9 3.6 : 1,
   both under G6.5's 4.5; the near black keeps 4.5 : 1 on every first-tier
   colour and the purple): the name at the tile's bottom left, `--tile-pad`
   in; on two lines when narrow and tall enough; else cut to the most
   letters that fit, at least `--tile-min-chars` (3, O), and "…"; else left
   out. A group's name bold (`text-strong`) at its top left, the first
   tile's name then kept clear of it. The parts left unnamed are counted in
   a note under the plot (`chart-note`, new role, italic in the note grey:
   `labels.notes.unnamed`, "({n} parts too small to name)"); a part of zero
   value has no tile and is named in the note with `labels.notes.zero`.
   With `labels.parts` value or percent (open; `auto`, as the book's tiles,
   names alone) the value or percentage goes under the name where both fit,
   else the name alone; the names are placed first, then the values
   formatted as one set over the tiles named (the tail's tiny values, never
   shown, would ask for more decimals) and the labels set again (P3-05
   `values`; `summary.without_value` the tiles named without theirs).
5. **Checks**: that parts few enough for a pie want one (R7-07, R7-15) is
   the chooser's guided finding. Highlight as for pies (`border` a rect in
   `overlay`, a guided departure under R15-5 as on bars; the other layer
   methods the strong shade, with an info). Comments and the title
   connector point at a tile's middle (6.12.5).

### 6.11b Wrapped bar charts (phase 3; as built, "(breakdown)")

07 §5.7, R7-14; G1.39, G7.7 [L]. `layout/wrapped.py`, `chart.type:
wrapped_bar` (length-encoded: `scales.LENGTH_ENCODED`), one series.

1. **Columns**: the sorted list (02 D1; All others, then values not known,
   last) cut into k columns filled top to bottom, the earlier ones one
   longer (100 → 34, 33, 33); k the fewest whose rows, `--line-pitch`
   apart, stay within `--wrap-max-aspect` (1.2, O) × the width in height.
   Bars `--band-fill` of the row pitch.
2. **One scale**: the same points per unit in every column (the largest u
   at which the columns fit the width, found by bisection, since the
   columns' tops depend on it). The first column takes the chart's scale
   (02 D5, with its breathing room: 0–12 by 2 for 9.6 %, where the book
   draws 0–10); each other column stops at the smallest 1, 2 or 5 × 10ⁿ at
   least its largest value, in steps of a fifth or a half of that where its
   labels all stand `--time-label-sep` apart, else in one step (0–1 and
   0–0.2 for the book's tail, where it draws 0–1 twice).
3. **Labels**: names right-aligned `--category-label-gap` left of each zero
   line; each column's scale labels (bare numbers, `fmt.format_scale`) above
   its first row and below its last, each row in its own `scale value`
   group, thinned where they would meet (every m-th back from the top one,
   R2-40), and where even its zero and top would meet, the top label only
   (the zero line standing for 0); gridlines at each column's ticks, its
   zero classed `zero`; the scale's title once, centred under the chart, in
   the first column's lower scale group (the SVG lint accepts it for every
   column of a chart classed `wrapped`). `--wrap-column-gap` (1.5 em)
   between a column's labels and the next column's names.
4. The root is classed `chart bar wrapped horizontal categorical`, each
   column a `panel` with id `column-k`; `summary`: `columns`, `rows`,
   `unit`, `column_scales`. No inset (CG-3 when asked).

### 6.12 The message layer: making charts obvious (phase 2; as built, "(message)")

The book's Part 5 (notes 13-15; G6.3, G8.2–G8.19, G4.16, G4.18; D-9, D-18,
D-19, D-21, D-48) in two modules that work around the layouts rather than
inside them. `transform.py` runs on the spec before anything reads its
values; `annotate.py` lays the chart out through the type's own layout (with
its inset) and then works on the laid-out model through the geometry the
layouts leave (`ctx.panel`: plot, orientation, the value's position, the
marks; `ctx.line_layout` and `ctx.series`: slots and each line's points).
The layouts are asked for nothing but to lay the chart out, once, or once
more at a narrower width (6.12.5). The pipeline's hooks (`chartgen.render`):
`transform.apply` after validation (before the several-panel branch, since
an index turns several measures into one), `transform.for_type` after the
chooser, `transform.scale_values` in the scale, `annotate.layout` round the
layout call, `annotate.copies` after emitting (a safe-pair style's red/green file, 6.12.3).

**6.12.1 Derived measures** (`spec.transform`; R15-13–R15-17, R2-17; G8.15–
G8.18), computed before the chooser, so that type, sort, scale and lint all
see the chart as drawn:

- `deviation`: each value minus its comparison value (`comparison`, an id in
  `data.comparisons`), or in % of it (`relative`); the measure becomes
  `change` (signed labels, the signed zero line), its dividing value 0, and
  the comparison is no longer drawn (it is the zero line); nominal bars sort
  by the deviation as they sort by value (R15-14). The unit phrase says so:
  the source's currency and magnitude with "above target", "over budget",
  "above average", "above forecast", "more than the previous period", "above
  peers" (their other sides "below target", "under budget" … name the
  bottom label, 6.12.3). The other comparison values are taken as deviations
  from the same comparison value, so that they are drawn in the deviation's
  unit (a threshold given in MWh as 5 % over each target becomes +5 %); where
  that gives every category the same value, it is one value, a line across the
  plot (the evaluation's B09, where a threshold left in MWh stretched a % scale
  to 4,000).
- `ratio`: each value per its exposure (`exposure: {values, noun}`), times
  `per`: "accidents per 100K work hours" (R15-15).
- `index`: each series rebased at `at` (default the first x) to `base`:
  10000 ("$10,000 invested"), 100, or 0 for "% change since" (a `change`
  measure). Series of different measures (`data.measures`, `series.measure`,
  the merged chart's fields, 6.10) share one scale once indexed: the index
  chart instead of two value axes (R2-14, R2-17, G2.31); `data.measures` is
  then emptied, so the several-panel branch does not apply. A series with no
  start value is refused (R15-16).
- The derived unit is built from the facts; `transform.unit` (and `name`)
  replace it where the chart needs particular words ("(value of $10,000
  invested on Feb. 4)", s. 0381). Every transform is a guided decision
  recorded under its rule and R15-13.

**6.12.2 Comparison values** (`data.comparisons`; R15-7–R15-9, R11-4; G0.19,
G8.12, G8.13, G7.57; D-21, D-48). A comparison has one value, or one per x
value; `compute: mean` makes an average of the plotted data (per slot on a
line chart, over the categories on bars). `transform.for_type` decides what
is drawn, and how (guided; `render: auto`):

- `valid: false` (another industry, years-old figures) is left out with a
  warning (R15-9); asked to be drawn (`render` other than auto) it is
  refused, overridable.
- The order of preference (G0.19): targets, budgets, tolerances and
  forecasts, then averages, previous periods, peers. A previous period
  where a target, budget or average exists is left out, with a decision
  (D-48); asked for explicitly it is drawn with a guided warning; alone it
  is drawn with an info flagging its limitations (D-48). A peer beside a
  better comparator is an info.
- Drawn as series of the spec where a layout draws references: on a line
  chart every comparison is a reference series (an average solid in the
  reserved dark grey, a target or benchmark dotted, labelled at its end in
  bold and packed with the other end labels; D-21 (c), G7.16); on bars, one
  value per category is a series of role `target` or `reference`, which the
  bar layout draws as a flat mark across each bar, named once with leaders
  (6.7, R11-4, G5.5). One value on bars or dots is drawn by the layer as a
  line across the plot (`reference-line`, class `average` solid or
  `reference` dotted, in the dark grey), its label (the comparison's, bold,
  as a reference series' label is) at the plot's right end above the line,
  else below it, else at its left end, else where the line is free; the
  scale holds its value (`scale_values`).
- A chart whose values are read as good or bad (`measure.good`) with no
  comparison, no dividing value and no deviation gets an info (R15-7).
- **Several references on one chart** (evaluation round 1, B09: each
  building's target and the Board's +5 % threshold). Every comparison drawn
  is drawn, of each kind the chart has: lines across the plot, labelled in
  the order of preference (targets, budgets, tolerances and forecasts first,
  then averages; the spec's order among equals), each label where it is free
  of the marks, of the other labels and of the other lines; marks across the
  bars, two at most, told apart by their look: the first a flat mark in the
  strong shade of the bar's hue (R11-4, G7.57), the second a dotted stroke as
  wide as the bar in the dark grey (`target-mark secondary`), the look of a
  reference line, which good and bad do not recolour. Their names are set
  once beside the first bar with leaders, the bar series' and each
  reference's (on horizontal bars the references named from the tops of
  their marks, so that the labels stack upwards into the room above the
  plot), else beside the last bar, else in a legend row, the second
  reference's key its dotted stroke. A third reference across the bars is not
  drawn, with a warning (R11-4: give it as one value, a line); two marks on a
  bar nearer than half their thicknesses and a gap are a warning naming the
  bars (the scale is too coarse for the distance between them: a deviation
  shows it). A horizontal bar chart with a comparison line keeps a label's
  room above its plot, where the line's label goes first.
- **Units** (R2-13, G2.30; the evaluation's B09, targets in kWh against use
  in MWh). A comparison value is drawn on the measure's scale, so it must be
  in the measure's unit. One given with its own `unit` is converted to the
  measure's by its `factor` (a decision names the conversion), or, where the
  units differ only in magnitude (£K against £M), by the powers of ten
  between them; one in another unit without a factor is refused (R2-13,
  `data.comparisons[i].factor`). Without a stated unit, a comparison value
  drawn an order of magnitude or more outside the data (ten times the largest
  value, or under a tenth of the smallest of data that keep away from zero)
  is a warning naming units as the likely cause (R2-13; FAR = 10, N).
- **With an inset** (6.6; the evaluation's B07, the 500 g line dropped once
  the chart had an inset). A comparison line is drawn across the main chart
  (under a region inset's frame, which covers it, and inside a band inset's
  source frame, whose outline alone its label must clear) and, where its
  value lies inside the inset's range, across the inset's plot too, in the
  inset's own overlay, its label at the inset's size where one is free inside
  the inset's frame. The line is named at least once: one that finds a label
  in only one of the two is drawn unlabelled in the other, and the decision
  says which names it; one named in neither is not drawn, with a warning
  (CG-1). A value outside the inset's range is drawn in the main chart only,
  a decision says so.

**6.12.3 Good and bad** (R15-11, R15-12, R3-17, R3-20; G4.16, G4.18, G8.14;
D-18). With `measure.good` and `measure.dividing_point` (a number, or
`{comparison: id}`, one value per category allowed; 0 after a deviation), each
regular bar or dot above or below it takes the class `good` or `bad`, a value
on it keeps its colour; a reference mark across a good or bad bar takes the
reserved dark grey. Lines keep their colours (an info). A layout that judges
its own marks (a waterfall's steps, an arrow chart's arrows, a heatmap's
sides, a table's action dots) leaves the same classes, and what follows
applies to them too.

Static output (D-18, owner, 1 Oct 2026: semantic colours for every reader).
The style's `--good-bad-mode` chooses the colours, never the chart:

- `semantic` (the plain style's, the default): plain red and green in one
  file, no texture, for every reader; the R3-19 finding on the pair is an
  info (3.6).
- `safe-pair` (a style for publications to unknown readers): the chart's
  root carries `cvd-safe`, under which the style draws the colour-blind-safe
  pair (orange bad, blue good, and the blue/orange ramp for a heatmap's
  sides), a neutral mark moves from slots 1–2 to slot 3, and
  `annotate.copies` writes the red/green version beside the chart as
  `<name>.red-green.svg` (the report's `summary.annotations.copies` names
  it); `audience.cvd: no` draws red and green in one file instead.
- An unknown value is a warning (CG-4) and draws as `semantic`.

In black and white (`audience.bw_risk: true`, a fact of the chart's
audience, R6-30), in either mode, good and bad are told apart without colour
too, which serves readers with colour-vision deficiency as well
(`annotate.textures`): every good or bad area mark of any layout (a bar, a
waterfall's step, a slice, a tile, a cell, a key's swatch, a status dot) is
followed in the drawing by its shape again (`texture`, its side as
`data-side`) in a group (`textures`) painted with a `<pattern>` computed into
the chart's own defs, one per side and colour: two tones of the mark's own
colour interleaved, their mean in linear light the colour itself (the
tones' saturation chosen so), apart in lightness so that the pattern
survives in grey; a 45° hatch for bad (1.6 pt a period, `--texture-bad:
hatch 62 34`, the OKHSL lightness of ground and strokes) and an ordered 8 × 8
bayer dither for good (0.7 pt pixels, `--texture-good: bayer 72 46`), the
diagrams' textures (`tools/texture.py`); a colour other than the side's own
(a ramp's step, the safe pair's) keeps the tones' distances from it in
lightness; `--texture-hue-shift` turns the two tones apart in hue. A dot
plot's dots, open, say it by their fill instead: a bad dot `filled`, a good
one open. Cells that carry their values, and their key, keep a flat fill (a
texture would cost the numbers their contrast, G6.5); an arrow chart's
arrows say rise or fall by their direction; lines carry no good or bad
colour, and two lines or more their own symbols (6.13.7). The safe pair in
black and white is one file, textured, with no red/green copy. The report
records each pattern's tones, coverage and mean (`summary.annotations.
textures`).

On a vertical bar chart with a signed scale, the words on the top label take
the colour of their side (`text-bad`, `text-good`; `-cvd` in a safe-pair
style's file), and after a deviation the bottom label gets the other side's
words ("−10% under budget"), the bottom gridline starting after them, unless
they would meet a bar (G8.14). The caption colours are text tokens dark
enough for text (`--good-text` … `--bad-cvd-text`, 4.5 : 1).

**6.12.4 Highlight methods of the layer** (R15-5, R15-6; G8.9–G8.11; D-9):
`border`, `glow`, `background`, `icon` and `selective_label`. The layouts do
not see these entries (`annotate.layout_spec` takes them out, and the
insight's targets with them when nothing is left), so the target keeps its
own colour and size unless another entry names it with `auto`, `strong` or
`thickness` (the book combines methods, s. 0366). Each is a guided departure
from D-9 unless combined with the default (R15-5, recommended `strong` for
bars and dots, `thickness` for lines).

| Method | Bars | Dots | Lines |
|---|---|---|---|
| border | a stroke of `--border-width` in the strong shade, just inside the bar (`border` + slot + `highlight`, over the marks) | the layout's strong ring (info) | not drawn (info) |
| glow | a flat light underlay `--glow-spread` round the bar (`glow`), beneath the gridlines | a disc round the dot | an underlay stroke `--glow-line` wide along the stretch `{series, x: [from, to]}` or the whole line (`glow-line`) |
| background | a band `--band` behind the category's slot, across the plot (`highlight-band`) | the same | a band behind the stretch or the period |
| icon | a marker of the library (`mark-icon-down`, `-up`, `-left`, `-right`) pointing at the bar's end, where it is free, in `--icon` | above the dot | at the point, where it is free; left out with a warning where nothing is free |
| selective_label | the value, formatted as the series' set (fmt.format_values), just beyond the bar's end (`value-label`); a value the bar layout labels already is left alone (info) | above the dot | above a peak, below a trough, else the nearest free place round the point |

Glow and band are flat and opaque (no filter, no transparency: G3.9s); the
SVG lint's check of fills behind the plot does not count them.

The glow's colour (owner, 30 Sep 2026) is a per-chart choice, guided, and
computed, never fixed. `highlight[].glow`: `own`, the target's own colour
lightened; `complementary`, that colour's hue turned 180° in OKLCH (the
perceptual wheel: red's complement is a cyan-blue, not the painter's green
nor a purple) at the same lightness, so that the two differ only in hue.
Both take the style's `--glow-lightness` (0.87) and at most its
`--glow-chroma` (0.065), the book's own glow (#f8cbad, s. 0366), so that
each stays a pale glow; the book's blue turned 180° at these values comes
to #efcdb0, its light orange. The colour is the one the target is drawn in,
in each file: the strong shade of a highlighted line, red or green for a
good or bad mark (under a safe-pair style, orange or blue in its file and
red or green in the copy) (the group holding the glow is painted with the computed colour, `fill` or
`stroke` and `data-glow`, the one paint a chart source carries, since it is
computed from the chart). `auto` (guided; `highlight[].emphasis`, open,
`normal` or `strong`): `own` for a normal emphasis; for a strong one the
complement, unless a contrast would confuse: more than 3 regular colours in
use (GLOW_FEW, the readers' number, N), a legend keying the chart's
colours, or a colour in use within 30° of the complement's hue (GLOW_NEAR,
N) in either file where there are two, when it is `own` with an info
saying why. An explicit
choice against that is a guided finding (R15-5, `highlight[i].glow`). The
style-wide `--glow-mode` of an earlier draft is gone: the choice depends on
the chart, and a style that fixed it would override the procedure.

**6.12.5 Comments and the title connector** (R5-5, RP3-7, R15-1, R15-2;
G6.3, G8.3, G8.4, G8.7, G8.8; D-19). A comment (`comments[]`: `text`,
`anchors`, `placement`, `answers`, `arrow`) is set in New Computer Modern
italic in `--comment` (`comment`), outside the count of text styles (the
model and SVG lints skip it), at `--comment-size` (1 em, 10 pt, for a paper;
a talk style sets it larger, 1.3 em say: the layer measures, wraps and
places comments at whatever size the style gives, checked at 1.3 em), in
centred lines no wider than `--comment-width` (its length at the label size,
scaled by `--comment-size`) times 0.7–1.2 (about 18–28 characters, G8.7s),
balanced, at most five. It is
placed in the free space of the figure: a raster of every text (grown by
`--comment-gap`), mark, line, frame and other comment (gridlines, dividers
and the layer's underlays excepted), with a summed-area table so that each
candidate box is one lookup; the candidates, every 2 pt, are scored by their
distance from the anchors (at least 2 em, room for an arrow), a small
preference for the plot, and the width; for each of the best, an arrow is
routed to every anchor: a quadratic bend from the side of the box towards
the anchor, leaving it square, its tip stopping `--comment-arrow-gap` short
of the target (a bar's end, else a side near it where a label sits on the
end; a dot's rim; a line's point, or the nearest point of a stretch or
line; phase 3: a point inside a pie's slice, 0.72 of the way from the
centre, or a donut's hole, to the rim on its middle ray, or either side of
it on a slice over 40°, as the book's arrow into the West, s. 0148; a
tile's middle), clear of every text and crossing as few marks as it can. Its head is
the house arrowhead (owner, 30 Sep 2026: the diagrams' "bend" head, owner 29
Sep 2026, `diagrams/README.md`, `studies/arrow_heads.py`): the
Computer Modern-like outline of `tools/arrowheads.py` (curved barbs, a notch
0.72 of its length back), at `head_size` of the arrow's stroke width times
`--comment-arrow-head-scale` (1: 4.2 pt long and across at 0.8 pt, as the
diagrams' heads at their 0.9 pt), laid along the curve (`--comment-arrow-head:
bend`, the default: each point of the outline at its arc distance back from
the tip, offset across by the curve's normal there, as `arrows.Arrow`
lays the diagrams'), or set along the tangent at the tip (`quick`), or aimed
from the notch's point to the tip (`flex`). The shaft (`Curve`, one cubic:
the quadratic split where the notch falls) runs into the notch with a
uniform width; the head is a second path filled in the arrow's colour
(`comment-arrow-head`, `title-link-head`); nothing of the diagrams' strip,
swelling ink or shadow, the plain style being flat. The arrow charts' data
arrows (time charts) keep their own straight heads. `placement`: `auto` (as
above), `inside` (the plot only), `right` (a column at the right), `below`.
Where a comment finds no space, the chart is laid out again `need` narrower
(the widest line of the comment and two gaps; not below 120 pt) and the
comment goes into the column at the right (the book's clinic chart,
s. 0366); failing that, under the chart, without the arrows that would cross
text (a warning). A comment without anchors is a remark on the whole chart,
placed near the plot's top right. `answers` says which of R15-1's four
questions it answers (`important`, `why_care`, `explain`, `action`); a
comment answering `important` states the insight, so that a descriptive
title passes R0-10 (G8.2, D-55). The report records where each comment went
(RP3-7) and which questions the title and comments answer (R15-1, open).

The title connector (`title.link`, a reference; G8.8, open): a thin grey
curve (`title-link`, `--link`, `--link-width`) from the end of the title's
last line, leaving rightwards (s. 0342, 0359), or from under its end where
that way is blocked, to the element named, routed like a comment's arrow.

**6.12.6 In the SVG and the style.** Groups: `underlay` (first in the panel,
beneath the gridlines: glows and bands), `overlay` (after the marks:
reference lines, borders), the panel's `annotations` (labels of reference
lines, icons, value labels), `comments` (after the panel and any inset: the
comments, their arrows, the title connector). Classes added to the
vocabulary: `underlay`, `overlay`, `glow`, `glow-line`, `highlight-band`,
`border`, `icon`, `cvd-safe`, `texture`, `textures`, `filled`, `comment-arrow-head`, `title-link-head`; text modifiers `text-good`, `text-bad`,
`text-good-cvd`, `text-bad-cvd`; the mark kinds `reference-line` and roles
`comment`, `comment-arrow`, `title-link` were in it already. The style's
part is `style/message.css`, imported by `plain.css` after the palette
(tokens `--comment-size`, `--comment-width`, `--comment-gap`,
`--comment-arrow-width`, `--comment-arrow-head` (bend, quick or flex),
`--comment-arrow-head-scale`, `--comment-arrow-gap`, `--link-width`,
`--border-width`, `--glow-spread`, `--glow-line`, `--glow-lightness`,
`--glow-chroma`, `--band`, `--icon`, `--icon-size`, `--icon-gap`,
`--value-label-gap`, `--good-bad-mode`, `--texture-bad`, `--texture-good`,
`--texture-hue-shift`), with the caption tokens in `palette-muted.css`. The layer draws on bars, dots and
lines; on other types (arrows, bumps) what the spec asks of
it is an info (CG-3), and only good and bad's static output (6.12.3) is
made for their good and bad marks. On pies, donuts and treemaps (phase 3,
`annotate.PART_KINDS`) it draws comments and the title connector, a slice
an obstacle by its bounding box (P3-16); their layouts draw every highlight
method themselves (`LAYOUT_DRAWS`); a comparison line or good and bad asked
of them is an info (CG-3). On a true time axis, thin bars and step charts
(`annotate.TIME_KINDS`; evaluation round 1, B03), it draws comments and the
title connector: an anchor `{"series": id, "x": time}` names an event (2.8),
the arrow pointing at its thin bar's top (the top of a stack of coincident
events), at a step's start, or, for an event whose value is not known, at its
note; comparison lines, its highlight methods and good and bad asked of them
are an info (CG-3). Inside an inset the layer draws its comparison lines
(6.12.2) and nothing else (its comments and connectors point at the main
chart's marks). Small multiples and merged charts (6.10) are laid out by
their own branch; once they are assembled the layer draws their comments and
title connector (`annotate.panels`; evaluation round 1, B02): each anchor is
found in its panel, the one holding its series or the one `panel` names (a
measure's id or heading, a row's or column's value), as that panel's layout
drew it (a line's point, a bar's end, a dot), and each comment is placed in
the free space of the whole figure by the rules of 6.12.5, under the chart
where none is free; the report says which panel a comment stands in. Nothing
else of the layer is drawn on panels yet.

### 6.13 Time charts (phase 2; as built, "(time)")

Notes 06 §5.3–§5.5, DP6-1, DP6-3; 02 D9, D10, §5.4 (g); G1.46–G1.54, G2.40,
G2.42, G7.14, G7.17, G7.19–G7.24; D-33, D-40, D-41, D-42, D-45. The chooser
already routed these types (3.2); phase 2 draws them.

**6.13.1 A true time axis** (`timeaxis.continuous_fit`, G2.42, D-45). An
irregular series (`x.grid: none`) gives its values as `points`, [time,
value], the time a day ("2022-04-27"), an hour ("2022-08-14T06") or a
minute ("…T06:30"); the points are checked in the structural pass (in order,
coincident times allowed). The axis is linear in time. It runs from the
first value (a state) or the start of the first event's day, to the end of
the as-of day, or for events to the end of the last event's day; `x.values`
may instead give the window the series covers, [first day, last day], so
that its empty stretches show (a value outside it is refused, CG-4). A date
alone is the start of its day for a state (a price from April 27) and the
middle of its day for an event (a sale some time on June 20). The labels
follow the regular axis's form (6.3): row 1 names periods of one unit,
hours, days, months or years, the finest that fits, each centred on its span
as drawn (an hour at its instant, "6 AM"), every k-th, k from the notes'
ladder (hours 1, 2, 3, 6, 12, counted from midnight; days 1, 2, 3, beyond
which the months are labelled; months 1, 2, 3, 6; years 1, 2, 5, 10, 20),
restarting in each row-2 group, the earlier of two crowding labels across a
group boundary dropped; a partial first or last period is labelled only
when its span holds its text. Row 2 names the next unit under its span,
dropped where too narrow and left out where the title gives it (G2.37);
dividers at the group boundaries and both ends when there are two groups or
more. Short ticks (`time-tick`, --time-tick 0.45 em) mark the boundaries of
the labelled unit, and minor ticks (--time-tick-minor 0.25 em) each day
under month labels (each month under years) where they fall at least 3 pt
apart: D-45 (ii)'s ticks, only on these axes, which the SVG lint checks.
Outer labels past the figure pull the plot in, and the layout runs again.

**6.13.2 Step charts** (`layout/step.py`, R6-8, R6-9, G7.19, D-42). One path
per stretch of known states, level from each change to the next and then
straight up or down to the new value (square corners, butt caps, so that
the last step stops exactly at the axis's end); the last value runs to the
end of the as-of day (D-42). Where the last state ends cannot be read from
the changes, so a step chart without `data.as_of` or a window is refused
(R6-8, G7.19 [L]). A state not known (`null`, a status) leaves a gap from its
change to the next (R6-22). No markers. The value scale and its unit phrase
are a line chart's (6.4; the phrase moves to its own line where it would
meet the line); a step line may leave zero out where the zero rules allow a
line to (G2.8: P1-26 declares the distance to zero irrelevant for looking a
price up, as the book's figure truncates at $200). One series; several are
reported as a later stage.

**6.13.3 Thin bars** (`layout/step.py`, R6-11, R6-12, G7.20, D-42). One
`event-bar` per event, --event-bar-width wide (0.28 em, 1.4 × the line
width, D-42), centred on its instant and rising from zero (thin bars are
length-encoded: `scales.LENGTH_ENCODED`); nothing between events. Events at
the same instant are stacked with a hairline gap (--grid-width), each its
own length (D-42), and the scale holds the stacks' totals; events closer
than a bar's width touch or overlap, as the book's concerts on consecutive
days do (s. 0183); an event whose value is not known gets its note at its
instant, placed as an empty slot's note is (6.8; owner, 1 Oct 2026: wrap
first; evaluation round 1, B03, where "(duration not recorded)" ran over the
spill before it): on the baseline at its instant, wrapped to at most two
lines by the names' break rules, where it fits between the thin bars either
side (their facing edges, --text-pad clear of each) and inside the figure;
else above, on one line or two, whichever stands lower, centred over its
instant (moved in where it would leave the figure), over every bar it would
cover, with a thin `leader` down to its instant on the baseline (a thin bar
chart has no slot labels for the second step to stand under). A guided
decision (R9-7) names each note's place. The SVG lint checks that every thin
bar stands on zero or on the event before it at its instant.

**6.13.4 Arrow charts** (`layout/arrowchart.py`, R6-15, R6-17, G1.46, G1.47,
G7.21, G7.22, D-40). Data: `x` time with two periods; each series one
category, its values before and after. Rows --bar-row-pitch apart (further
where wrapped names would come within --category-label-sep of their
neighbours, 6.7.3), names right-aligned at the left; by the value before, largest first (D-40), or by
the change in % when `facts.message` is `change`; a category with a value
not known last, with its note. Two panels sharing the rows (G1.47 [L]): the
values in the measure's unit, each arrow from before to after, on the
absolute scale compute_scale gives (an arrow is a position: the zero rules
of G2.8); and the change in %, each arrow from 0 %, on a scale holding 0,
its zero line the signed one when changes go both ways (D-17), its labels
signed (6.2), its title "Change in <measure> (%)". Both scales are
horizontal (6.4: vertical gridlines, bare labels, the title below); each is
refitted with fewer intervals where its labels would crowd in its half of
the width. `facts.message: relative_change` draws the % panel alone;
`chart.change_panels: absolute` is refused under R6-17 unless overridden (the
book's s. 0187); a before value of 0 or less makes the change in %
undefined, and the % panel is left out with a decision (D-40). An arrow is
a `change-start` dot (--arrow-start-size), a `change-shaft` and a filled
`change-head` (--arrow-head-length, --arrow-head-width) whose tip sits
exactly on the end value (the book's heads overshoot, notes 06 §6.10). A
change shorter than a full head beside its start dot keeps its direction
readable, the tip still on the end value (evaluation round 1, B08: two small
rises showed a dot alone, the head hidden under it); the style chooses how
(`--arrow-short-head`, owner to judge, examples/review/r1-arrows): `fit`, the
default, shrinks the head to the room between the dot's edge and the end
value, its length and width in proportion, down to half the full head
(`HEAD_MIN`, O), no shaft; shorter still, the head at full size overhangs the
start, drawn over the dot with a halo of the page colour round it
(`arrow-halo`, `--arrow-head-halo`), so that its point reads against the dot;
`overhang` draws every short change so; `head-only` draws a change shorter
than the head and the dot as the head alone, no dot; `hidden` is the former
drawing (the dot over the head). A decision (R6-15, guided) names the short
changes and their form; no change is a dot alone. Colour (G7.22, D-40): `good` and
`bad` where `measure.good` gives the reading, red and green, which the
message layer treats as any good and bad marks (6.12.3, D-18); otherwise slots 1 and 2
for rises and falls. The key (G7.21): the periods' names (`x.names`, else
the period's own name) in the note style (`arrow-key`) over the ends of the
longest rise whose two words fit between them, else beside the ends of the
longest arrow. Two periods as bars are refused (3.2). Departure from D-40:
no row separators; the panels keep their vertical gridlines (G3.6 [L]),
which with separators would make a table's grid.

**6.13.5 Incomplete periods drawn apart** (`layout/lines.py`,
`layout/incomplete.py`, R2-42, G2.40, 02 D10, §5.4 (g)). A first or last
period the as-of date cuts short, or whose value says `complete: false`, is
left out, axis and all, by default (lines, and now bars and time dot plots
too). With `facts.incomplete_value_needed` it keeps its slot: on a line the
line ends at the last complete period and the partial value is a filled dot
(--dot-size, `data-complete="false"`) with the note "As of Dec. 10"
(`incomplete-note`, the year left out when the title gives it; "(incomplete)"
without an as-of date) on the side away from the line; end labels stay at
the lines' ends, clear of the dot. On bars (a hook in `layout/bars.py`) the
bar is replaced by a dotted level mark as wide as a bar at the value's
height (`incomplete-mark`, dotted like a projection), the note to its right,
for which the plot keeps room at the right (`hooks["room_right"]`); the SVG
lint counts the mark as the slot's bar. A dot plot over time leaves the
period out.

**6.13.6 Bumps charts** (`layout/bumps.py`, R6-7, R6-40, G1.52, G7.17, G7.24,
D-41). The values are the ranks (`measure.kind: rank`) or the quantities,
ranked each period, 1 the largest (the smallest where `measure.good` is
`lower`), tied values sharing a rank (D-41). compute_scale takes the rank
scale from the layout's hook: 1 … N by 1, "Rank" set horizontally above the
numbers as the unit's line, no gridlines (G7.24 [L]; the SVG lint's gridline
rule gives way to it), 1 at the top, the rows at least an end-label pitch
apart. The time axis is a line chart's (equal slots). Each line is a path of
cubic curves whose control points lie level with their ends, flat at every
period, so that it stays between the ranks it joins (R6-40): the only curved
data line, which the SVG lint checks. Colours top to bottom by the final
ranks; a line that starts late or ends early has a dot at that end (D-41);
each line labelled at its final rank (tied labels packed); a highlighted
competitor twice as thick in the strong shade, its label bold. More than ten
competitors warn (D-41). Bumps charts are always smoothed so: the book shows
the straight form first and calls the flat curves fine; there is no option.

**6.13.7 Black-and-white symbols** (`layout/lines.py`, R6-30, G7.14, D-33).
With `audience.bw_risk` and two lines or more, each line gets its own
symbol at every point, square, triangle, ×, filled circle, + and asterisk
(the book's six, s. 0200), handed out in the order of the colours, top to
bottom by the line ends, a reference series after the regular ones;
--symbol-size (0.55 em) across. The lines keep their colours (a departure
from the book's one dark ink, which shows the chart after a black-and-white
copy: the chart is made in colour and must survive the copy); an emphasised
line stays thicker. End labels sit beside each line's last symbol; a legend's
key shows the symbol on its stroke. More lines than symbols warn. One line
needs no symbol (a guided decision). The same fact turns on the textures of
good and bad marks (6.12.3, D-18); lines carry no good or bad colour (G4.27,
an info), so that a line chart with a good direction needs nothing more in
black and white than these symbols.

**6.13.8 Checks that were only defaults** (G9 13, 22, 48, 50, 58, 59, 60, 61).
Model lint (`lint.time_charts`): a line or projection crossing a slot whose
value is not known, or one drawn apart (R6-22); point markers exactly where
required, every known value of a line that needs them (R6-25, R6-26, R6-30)
and none on one that does not (R6-27); a step line or thin bar under a text
(CG-1); each known event drawn once (R0-12); an arrow in each panel for each
drawable row (R6-17); a bumps line not flat, its end labels out of final-rank
order (R6-40, R6-32). SVG lint (§3.6): `line_shapes` (R6-39: no curve in a
line, projection or step line; R6-40: a bumps chart's curves flat at their
points), `dashes` (R6-31: dash patterns only on projections, references,
targets and incomplete marks), `shared_colours` (R6-28: no two regular lines
sharing a colour, black-and-white symbols, small multiples and merged charts
excepted), `legend_order` (R4-5: a line chart's legend in the order of the
line ends, read from each entry's `data-series`), `empty_dots` (R9-7: every
name of a categorical dot plot with its dot or a note), `time_ticks` (R1-04:
ticks only on a true time axis), `event_bars` (R2-24: thin bars from zero or
stacked). Each is tested by putting its fault into a generated chart.

**6.13.9 For the owner to judge.** (a) D-42's stacking of coincident events:
the two sales of July 3 and July 5 (P1-27) stand as one taller bar, the
scale running to $6,000 for them; the book's figure shows them side by side,
because its sales have times within the day, which the notes read to the day
only. (b) Labels on true time axes centred on their periods (days, months)
rather than at the tick of their start, as the book's software does ("Apr"
at April 1): the same form as the regular axes; hours stay at their instants.
(c) The TV's price on a truncated scale (P1-26, from the declared irrelevance
of zero) against the `from-zero` variant. (d) The arrow chart's % labels
signed (+20). (e) No
row separators on arrow charts (6.13.4). (f) Bumps charts always flat-curved.
(g) Black-and-white symbols on coloured lines (6.13.7). (h) The weekly
totals of 06 T7 are not an example: the week grid is ISO (Monday to Sunday),
the book's weeks run Sunday to Saturday, and regrouped so the totals fall
instead of rising; a week-start setting would be needed. (i) 02 #20 (unequal
time bins) cannot be written: no grid for decades or binned periods. (Phase
3: (h) and (i) are built, 6.13.11: P3-71 and P3-72.)

**6.13.10 Slope charts (phase 3, "(chooser)")** (R6-16, G1.46, G7.23; 06
DP6-3, §5.4). The chooser gives a slope chart only in the book's exception, a
change of rank between two periods (`facts.message: rank`, x time with two
periods, at most ten categories; more, an arrow chart), and refuses one asked
for otherwise (R6-16, locked through G1.61). It is drawn by the line layout,
as the book defines it, "a line chart with two points in time" (p. 201): the
report's type is `slope`, the choice's `line`, so that the line layout, its
lint and the message layer serve it unchanged (a line through two periods
passes no test of enough values: the chooser skips line_refusals for it).
Each category is labelled at the right end of its line (G7.23), the targets
of the insight emphasised as on any line chart (P3-69). **Departures, for
the owner:** G7.23 colours the lines as G7.22 colours arrows, by direction
(two colours, or good and bad); here each line keeps its own colour, since
two lines of one colour where they cross break R6-28's lint (every line its
own colour) and a rank message is about which category is which; G7.23's
fallback to an arrow chart when the right-end labels collide is left to the
line layout's own fallback (a legend, R4-2), which a slope chart should not
take; and labels at both ends are not drawn (the left end meets the value
scale): the book's figure labels the right end only.

**6.13.11 Weeks and bins (phase 3, "(chooser)")** (R2-43, G2.41 [L]; 02 §5.4
(f), D9; 06 T7). *Weeks.* A week grid's weeks start on `x.week_start`:
`monday` (ISO 8601), `sunday` (the book's weeks), or `auto`, Sunday for a
Sunday-first region of `audience.locale` (US, CA, MX, BR, JP, KR, TW, IL, PH;
proposed) and Monday otherwise. A week is written as its first day
(`2022-06-19`), or as an ISO week (`2022-W25`) only when weeks start on
Monday; the structural check refuses an ISO week under Sunday weeks, a first
day on the wrong weekday, and weeks that do not follow one another (a week
without a value is `null`, never left out) (CG-4, `timeaxis.check_grid`).
Row 1 is "Jun 19–25", "Jun 26–Jul 2"; row 2 is now the **year** (it was the
month, which row 1 already names; the book's figure has "2022" under its
weeks), left out where the title gives it (G2.37). `timeaxis.period_start`
gives the period holding a day, for totalling events per week (the chooser
still asks for the totals, R6-10). 06 T7's weekly totals are P3-71: the sales
of P1-27 totalled in Sunday weeks, $11,010, $11,575, $13,150, $13,935, the
book's $11.0K to $14.0K; its variant totals the same sales in ISO weeks,
$14,420, $13,210, $10,680 (and a last week cut short by the data's end, left
out, R2-42): the trend reverses, which is why the week's first day is a fact
the spec states. *Bins.* `x.grid: bin` takes ranges of years, "1960–1969" (or
with a hyphen), inclusive, contiguous and in order (CG-4); row 1 reads
"1960–69" (both years in full across a century), no row 2. Bins of one length
are a locked decision (R2-43). Unequal bins are re-aggregated to the common
larger bin (`timeaxis.rebin`, in the chooser before anything reads the
values), 2000–2004 + 2005–2009 → 2000–2009, when the shorter bins tile it,
aligned with the others, and `measure.additive` says the values add up
(totals over their bins); the values are added (a bin with a value not known
gives a value not known), comparisons per bin too, and the decision names
each merge. Otherwise the chart is refused under R2-43: the bins cannot tile
the larger one, or whether the values add is not stated (D-1: summing
averages would misrepresent them); an override keeps the unequal bins, the
book's bad version (P3-72 `unequal`). Only ranges of whole years are read; a
bin of dates or months is a later stage.

**6.13.12 Dates marked on a time axis** (`data.x.events`, `layout/events.py`;
evaluation round 1, B03 and B11). Something that happened outside the data
and that the message refers to, a storage tank finished in mid-June among a
year's spills, a competitor's opening among a garden centre's sales, is a
fact of the time axis: `{"at": "2025-06-15", "label": "Storage tank
finished"}`, `at` a date or an instant (a period on a regular grid, its
start). It is drawn as a thin line across the plot at its date (`event-line`,
in the reserved dark grey at the gridlines' width, in a group `events` over
the gridlines and under the marks), from the zero line to the top gridline,
and its label beside the line's top, inside the plot, in the note style
(`event-label`: italic, --note-size, the note grey; the chart's own note, as
"As of Dec. 10" is): right of the line, else left of it, else lower down
beside it, a line's pitch at a time, else above the plot, the line run up to
it and the plot moved down to keep the title gap; wherever the label meets no
text, no mark and no other date's label. A date alone is the start of its
day, or its middle on a thin bar chart whose events are dated to the day (as
those events sit). A date outside the axis is not drawn, with a warning
(6.13); a label with no free place is not drawn, nor its line (a marked date
is never left unnamed; CG-1, a warning). A decision (6.13, open) names each
date and where its label went. As built, thin bars and step charts
(`layout/step.py`) and line charts draw them; in small multiples the line
stands in every panel and its label once (6.10). Other chart types report the
dates as not drawn yet (CG-3).

### 6.14 Colour ramps and their keys (phase 3; as built, "(tables)")

`chartgen/ramps.py`; R3-8–R3-15, R4-8, R4-9; G4.5, G4.7–G4.14, G4.19, G4.21, G5.12, G5.13;
D-37, D-49. What colour encodes decides the palette (G4.5):

1. **A quantity** (a heatmap's cells, a shaded table): `ramps.choose(spec, report, values)`,
   the guided decision, recorded. Sequential (R3-12, G4.10), unless `measure.dividing_point`
   gives the value the audience sees as dividing: then diverging about it (R3-13, G4.11; never
   about the middle of the data, D-37 (a); a rating whose own middle is the audience's neutral
   gives it as the dividing value, D-37 (c)). `chart.encoding` asks otherwise: diverging
   without a dividing value is refused (R3-13), categorical too (R3-9, G4.14: the hues have no
   order), both except through an override; sequential against a dividing value is a guided
   finding. A dividing value that is a comparison per category (`{comparison: id}`) is not
   drawn on one colour scale (CG-3, sequential).
2. **Steps of a heatmap: the cut** (`ramps.classify`, `ramps.Classes`; GUIDE G4.29–G4.34,
   Datawrapper's advice adopted by the owner, 1 Oct 2026, and G4.33 ours; the book gives no
   rule). A heatmap's colours are a few steps of value, each a step of the style's ramp (`seq-k`;
   `div-mk`, `div-0`, `div-pk`), so that a cell carries a class, never a colour written into the
   chart. Where the steps are cut is guided (`chart.classing`), recorded with its rule:
   - `linear` (G4.29): equal steps on rounded values (1, 2 or 5 × 10ⁿ), the finest giving at
     most seven, from zero where zero means none of it (cut at the theoretical limits);
   - `natural` (G4.30): Jenks's natural breaks (Fisher's exact optimisation on the distinct
     values, at most 600 groups), six steps, the breaks rounded to two significant figures for
     the key, the top step open ("2,000 or more");
   - auto: linear when the values are fairly even; when they are **skewed** (ours: one of seven
     equal steps from the lowest value to the highest would hold more than half of them,
     `ramps.skew_share`, G4.30's "a few extreme values would put nearly every mark in one step";
     the measles data: 80 %), the message decides, by the fact `facts.ramp_focus`: `extremes`,
     linear; `pattern` (the default, with an info, D-1): `log` (G4.33; owner, 1 Oct 2026) when
     the values are amounts on a ratio scale (zero meaning none of it, none below it; sequential
     ramps only) spanning at least two orders of magnitude, measured as the 95th percentile of
     the positive values over their 5th (`ramps.log_span` ≥ 100, `LOG_SPAN`: robust, so that one
     stray tiny value or one outlier does not make a narrow range logarithmic, while the bulk of
     the marks spanning two decades does; the measles rates: about 10,600), else natural;
     `log` is 1–2–5 × 10ⁿ, else every power of ten, else every second or third, at most seven,
     zero a step of its own, the top step open;
   - only when asked for, a guided departure warned without a reason: `quantile` (G4.31; six
     steps of equal count, the key's title saying "each step about a sixth of the cells"),
     `custom` (G4.32; `chart.breaks`), and `natural` or `log` where the procedure gives the
     other; `log` asked for is refused with a warning, and linear drawn, for values that are not
     ratio-scaled (`measure.zero` arbitrary, or below zero);
   - diverging: the distances from the dividing value are cut the same way (at most four steps a
     side, the steps spread over the side's four) and the steps mirrored about it, symmetric
     (owner, for now; *departure* from G4.12's freedom to give the sides different numbers of
     steps); a value exactly at the dividing value takes the neutral `div-0`, a step of its own.
   G4.34 [L]: the key shows the breaks rounded (`fv`: each break's own digits, grouped, signed
   for a change) and the lightest step used (the neutral excepted, which G4.12 lets be white) at
   least OKLab ΔE 0.05 (`ramps.DISTINCT`, ours) from the page and from the missing grey, else a
   warning naming `--seq-1`: the plain style's first step is now #DCEAF6, L 0.93, ΔE 0.073 from
   white (it was #EDF5FB, ΔE 0.036; the second step re-spaced to #C6DBEB; steps 3–9, which the
   stacks and pies use, unchanged). A shaded table (6.16) keeps the continuous nine equal steps
   below (its colour only hints, no key).

   **Continuous steps (a shaded table).** A continuous ramp is drawn as nine equal steps of value
   (`seq-1` … `seq-9`; `div-m4` … `div-0` … `div-p4`) (*departure*: the book's ramps are
   continuous; nine steps are its own anchors, 03-04 §5.1). A sequential ramp runs over its key's limits: from zero
   when zero means none of it and the values are positive, else from the step below the lowest
   value (never below a theoretical minimum), to the step above the highest (never above a
   theoretical maximum), on a step of 1, 2 or 5 × 10ⁿ giving about eight intervals (a finer
   step than a scale's, so that little of the key is unused). A diverging ramp is **symmetric
   about the dividing value** (*departure*, O): its half-range the larger side's, rounded up to
   a step, so that a step of colour is the same distance from the dividing value on either
   side (G4.12, R3-14: intensity grows with the distance); its neutral step straddles the
   dividing value (a band one ninth of the range wide); the key is cut at the theoretical
   limits, which places the dividing value off its middle when the audience's neutral is (the
   book's 7 on a 10-point rating, s. 0118). A shaded table's groups (6.16) take their own
   minimum and maximum, no key. With `measure.good`, each step outside the neutral also takes
   `good` or `bad`: the stylesheet paints the sides green and red (G4.16, D-18), and, under
   `cvd-safe` (a safe-pair style's file, 6.12.3), blue for good and orange for bad, the
   red/green copy beside it (G4.18). Without a good direction a diverging ramp is blue below
   and orange above, free of red (G4.17).
3. **A cell's strength, and the value in it** (owner, 1 Oct 2026). `chart.cells` (guided;
   `ramps.cell_strength`): `strong`, the step at full strength, auto for a heatmap, whose
   colour is the message (R12-6, R6-36); `muted`, the step paled (`labelled`, its step at a
   light OKLCH lightness, `--cell-l-k`, the k-th step out from the page or the neutral, chroma
   at most `--cell-chroma`) and the value in the text grey, at 4.5 : 1 or more on every step of
   every ramp (4.62 at worst): auto for a shaded table (`table.graphics: heat`, R10-9), a table
   whose colour only hints at sizes (G7.47), and a choice for a heatmap that is really such a
   table (P3-50 `muted`). It is the spec's, not the style's, since it depends on the chart.
   A value on a strong cell keeps 4.5 : 1 (G6.5) by the style's `--cell-values`
   (`ramps.CELL_VALUE_MODES`; `heatmap.value_on_cell`, `ramps.treatment`): `contrast` (the
   owner's choice, the default), the value black (`on-mid-cell`, `--cell-text-dark`, #000) or
   white (`on-dark-cell`, `--cell-text-light`), whichever has more contrast on the cell, in
   every file the chart is written in: a switch, since no continuous ramp of tones keeps
   4.5 : 1 through the middle of a ramp (on `--seq-7` only black passes, 4.68 : 1; white 4.49,
   #1a1a1a 3.88); `plate`, kept for comparison, the value in the text grey on a plate
   (`cell-plate`) of its step's `labelled` version, `--cell-plate-pad` round its ink. The halo
   treatments and the grey-where-it-reads switch compared on 1 Oct 2026 were set aside and
   removed. The report gives the strength, the treatment, the tones or plates, and the lowest
   contrast (P3-50: 5.72 : 1, 22 values black, 2 white).
4. **The key** of a heatmap's steps (`legend.class_key`; G5.13 [L], R4-9, G4.34 [L]): a bar of
   the steps as equal abutting swatches (`legend-swatch` with the steps' classes), each at least
   `--ramp-key-width` / n and as wide as the widest break plus `--time-label-sep`, so that every
   break is written: the breaks under the boundaries, the low end under the bar's left end, the
   high end under its right unless the top step is open, zero's or the dividing value's own step
   labelled under its middle with the `key-tick` there (else the tick on the boundary at the
   dividing value); the title, the placing and the missing entry as below. The book's key is a
   continuous bar (*departure*: its steps are equal swatches, not lengths of value, since the
   steps may be logarithmic or uneven). The key of a continuous ramp (`legend.ramp_key`, a
   categorical palette through an override only) is a graduated bar, `--ramp-key-width` by
   `--ramp-key-height`, of the steps as abutting swatches (`legend-swatch` with the steps'
   classes), each as wide as its range of values; its title above, left-aligned: the quantity
   and its unit in parentheses, as a horizontal scale's title (`heatmap.key_title`: "Measles
   cases (per 100,000 people)"); the values under it at their places, the ends and the dividing
   value first, the roundest of the rest while they keep `--time-label-sep` apart; a short
   `key-tick` at the dividing value; and, when values are not known, a separate entry after the
   bar, a grey square and its name ("Missing"). A heatmap places it above the grid, below the
   title, right-aligned with the grid (the book's s. 0204): the bar's end (or the missing
   entry's, when there is one) at the grid's right edge, an end value's overhang kept within
   the figure (a wider title of the key is right-aligned instead). *Departure:* where every cell
   prints its value, no key is drawn (the numbers are the key, as in the book's heatmap table,
   s. 0329), although G5.13 keys every continuous scale.
5. **Ordered categories** (`ramps.ordered(n)`, G4.7, R3-8): steps of the sequential ramp in
   their order, spread over steps 3–9, the two palest being lighter than the gridlines (G3.2);
   more than seven are refused (G4.19, G4.21). The stacked bars' ordered parts take these steps
   (layout/bars.py, P3-34).
6. **Groups of categories** (`ramps.grouped(groups)`, G4.9, R3-11, D-49): group g in palette
   slot g, its members its shades (`shade-light`, the base, `shade-dark`: the slot's colour 40 %
   to the page, itself, its strong shade), the darkest in the middle (two members: light then
   dark; three: base, dark, light, s. 0115); more than three a ValueError (G4.21). Their legend,
   `legend.grouped_row` (G5.12, R4-8): each group's entries side by side, a thin rule in the
   group's colour under them (`group-rule`), the group's name centred under it in the hue dark
   enough for text (`text-group-k`, the third tier, 4.5 : 1). *Departure:* the names are
   upright, not italic as G5.12s and the book have them: italic marks comments (owner, 30 Sep
   2026). Stacks of several totals whose parts are in groups draw them (`data.parts.groups`,
   6.18.3, P3-39).
7. **Action dots' statuses** (`ramps.action_classes`): strong and light good, light and strong
   bad, steps 4 and 2 of the good and bad ramps.

### 6.15 Heatmaps (phase 3; as built, "(tables)")

`chartgen/layout/heatmap.py`, `chart.type: heatmap`; R6-36, R12-6; G1.20, G7.35, G7.36. A row
per series (its name right-aligned at the left, `row-header`), a column per x value, the cells
touching, each a `cell` classed by its step (6.14), strong unless `chart.cells` asks for muted
ones, its steps cut by 6.14.2 (G4.29–G4.33) and keyed by the steps' bar (6.14.4); a value not
known a grey `missing` cell, never the colour of zero (R6-23), with its own key entry. Two forms
by x:

- **over time** (G7.35 [L]): the columns on the regular time axis under the grid (6.3: row 1
  thinned, row 2 and its dividers, halos; on a year grid the shown years are the round ones,
  every 2nd, 5th, 10th … by value, which the eye counts on); the rows `--cell-pitch` apart
  while the grid keeps within the plot's usual proportions (`--plot-aspect`), else
  `--heat-row-pitch` (a row each at the label size, widened when names with descenders would
  meet); the key above the grid at its right;
- **a heatmap table** (x nominal or ordinal; G7.36 [L], the radar's replacement): the items as
  columns, their headings centred above (`column-header`), in the given order (the chooser's
  `GIVEN_ORDER`), or sorted as bars are when it sorts them; each column as wide as its widest
  value or heading plus `--table-fill-pad` each side (at most `--heat-max-column`); a summary
  row (a series of role average or total, "Overall Avg.") last, `--summary-gap` apart (s. 0329);
  headings that would meet over columns narrowed to the width are refused with the width they
  need (CG-1).

Touching cells are drawn each running 1 pt under the next (`heatmap.SEAM`; not the last row or
column, nor the row before a summary row's gap), so that no hairline of the page shows where two
abut when a renderer antialiases their edges; the cells' places and sizes in the report are the
slots'. A shaded table's cells (6.16) are drawn the same way.

`chart.value_labels` (guided, G2.18, G7.36): auto prints each value in its cell, centred,
with the data's decimals (or the precision's), when every value fits its cell (`table-cell`
over a `labelled` cell); `on` where they would not fit is refused (CG-1); `off` draws the key
instead. Highlighting cells is a later stage (CG-3). The book's vaccine line (s. 0204) is not
drawn: a comment or event line on a heatmap waits for the message layer.

**Censored values** (round 1, sonnet-G B10: twelve "<1" results had been drawn as the grey of
eight lost samples, which the reviewer failed as misleading). A value below a detection limit
("<1") or above a test's range (">2,420"; 2.4) is a state of its own: never a `missing` cell,
never the colour of zero, and kept out of the classes' cut (6.14.2), which is made from the
measured values; with the limits several, the highest below and the lowest above stand for them,
a measured value beyond a limit warned (R0-12). Its cell's text, where cells carry their values,
is the limit with its sign, in the set's decimals and never rounded (`fmt.censored_text`: "<1",
"<0.5"). How its cell looks is the style's, `--censored-cells` (`heatmap.CENSORED_MODES`), a
visual decision for the owner, the alternatives rendered on the E. coli data in
`examples/review/r1-below-limit/`:

- `step` (provisional default): a step of the ramp of its own below the lowest measured class
  (above the highest), the measured classes spread over the steps that remain
  (`ramps.Classes.below`, `.above`, `censored_classes`); the key shows it as a swatch of the
  bar, its limit ("<1") under its middle and the first measured class starting at the limit;
- `printed`: the palest measured class's colour (the darkest above), the limit printed in the
  cell, black or white by contrast, the key an entry after the bar ("<1: printed in its cell");
- `pale`: a pale fill of its own (`--censored`, a light warm grey apart from the page and from
  the missing grey), an entry after the bar;
- `hatch`, `dots`: the page's white under a fine hatch at 45° or a grid of dots
  (`censored-hatch`, `censored-dot` in `--censored-mark`, `--censored-hatch-gap` apart), an
  entry after the bar with the same texture.

The report's R0-12 decision (locked) counts them apart from the values not known and names the
treatment; the summary gives `heatmap.censored`. A diverging ramp has no step beyond its ends
for them yet: refused (R0-12).

### 6.16 A table alone (phase 3; as built, "(tables)")

`chartgen/layout/table.py`, `chart.type: table`; R10-9, R10-11–R10-25; G1.12, G1.16,
G7.37–G7.47; D-52. Only when its sole use is looking up or exporting values, or when no two of
them are comparable (R10-11, G1.12: the chooser's step 1, `facts.message` lookup_only,
export_only, not_comparable). x gives the columns, the series the rows; a series of role total
(or average) is the totals row, last, and an x value of role total the Total column. Its text
is the table under a graph's (6.7.13): `column-header`, `row-header`, `table-cell`, numbers by
`fmt.number`, a value not known as its note (`missing-note`). In the book's order (08-12 DP-4):

1. **Digits** (R10-12, G7.37): each column one number of decimals, the measure's precision or
   the data's own; every digit kept, grouped by the audience's separator (R5-14, R5-15); no
   magnitude is hidden (a table states what the purpose needs; `unit.given_in` says the data
   are already in thousands, and the title says so). Money in its own unit that needs any
   decimals takes two, pence or cents, down its column (`fmt.money_decimals`; round 1,
   sonnet-G B12: "45.00", "67.50", never "45.0", "67.5"; a column of whole pounds stays whole).
   A censored value is its limit with its sign, "<1", in the column's decimals and aligned as a
   number, never a note; its limit counts among the column's decimals (2.4).
2. **Rules** (R10-13, G7.38 [L], D-52 (iii)): a thin `table-rule` (#A3A3A3) under the header
   row, `--table-rule-drop` below the headings' baseline, and a darker `table-rule-total`
   (#656565) above the totals row, both across the table; no grid, box or vertical rule. A table
   with no headings (a list of metrics) has neither header row nor its rule.
3. **Spacing** (R10-14–R10-16, G7.39, G7.40 [L]): the columns `--table-col-gap` apart (1.5em),
   `--table-total-gap` (3em) before a Total column; rows `--table-alone-pitch` (2.15em) apart,
   `--cell-pitch` when shaded; stripes (`table.stripes`) refused under R10-16, drawn (`zebra`,
   every other body row) only through an override.
4. **Alignment** (R10-17–R10-20, G7.41 [L]): stub columns (`table.stub`, text or ISO dates) and
   the names left; numbers to be compared right-aligned; numbers not to be compared (facts.message
   not_comparable, or series of different measures in `data.measures`, which then make one
   table, not a merged chart) left-aligned, each with its own unit and precision ("$2.337M" with
   the audience's M, "14.60%"); each heading aligned like its column; the totals row's name
   right-aligned where the names end (s. 0317; D-52 (ii): "Total" kept, regular weight).
5. **Emphasis** (R10-21–R10-23, G7.42–G7.44): nothing bold but the title (`table.bold` refused
   under R10-22, drawn only through an override); the cells the highlight names (a cell
   {series, x}, a row {series}, a column {category}; else the insight's targets) on a
   `table-fill` (#E8E8E8) round the text, nothing else; more than a few warned (G7.44).
   *Departure:* the values of a list not to be compared are not bold, as the book's are
   (s. 0316, G7.41's figure): the owner's rule is that nothing but the title is bold.
6. **Formats** (R10-24, R10-25, G6.21, G7.45 [L]): dates in a stub column written
   05-Jan-2021, every part of fixed width (`fmt.date_column`; New Computer Modern, accepting the
   drift, D-32 columns (a)); the cells bare numbers, and a warning when the title does not state
   the unit's currency, suffix or noun.

The table is as wide as its columns, never stretched (R10-14); wider than `layout.width` it is
refused with the width it needs (CG-1). In-table graphics (`table.graphics`, R10-9, G1.16,
G7.47, guided): `heat`, each number's cell (the column and half its gutters, the body's cells
touching) shaded on a sequential ramp over its group's own minimum and maximum, the body, the
Total column and the totals row each a group, the grand total unshaded, `labelled` so that the
numbers keep 4.5 : 1 (s. 0309), the table half a gutter wider so that the last column's cells
stay inside it and under its rules; `bars`, a `cell-bar` under each number of
`graphics_columns` (default every column, a Total column too, as the book's, s. 0309; the totals
row has none), from the cell's left edge, on one scale from zero for each column (the owner's
instruction; the book scales the months as one group and the Total column as another), pale
enough for the text grey; `dots` (not the book's), a `cell-dot` in a `--cell-strip` (3em)
beside each number of the same columns, on one scale per column (from zero for a measure whose
zero means none of it; P3-49 `dots` needs 552 pt); `trend`, a Trend column before the
Total, each row's values (the totals row's too) as a `sparkline` scaled to its own extent, a
gap where a value is not known. Graphics in a table of values not to be compared are not drawn
(CG-3). A row of role `missing` ("No answer") comes after the other body rows, before a totals
row, its dots in the missing grey and its bars in the grey paled so that the text grey keeps
4.5 : 1 on them (G4.15, G6.5). A censored cell ("<1") of a `heat` table takes the end step of its
side of the ramp, its text saying what it is (on a diverging ramp, the censored fill, an info);
`bars` and `dots` draw nothing for it, its text standing; a `trend` line breaks there.

### 6.17 Action dots and the funnel's replacement (phase 3; as built, "(tables)")

**Action dots** (`layout/table.py`, `chart.type: action_dots`; R12-4, G7.58 [L], G7.58s; the
chooser gives them for `facts.message: action_needed`): rows sorted by value, largest first,
those not known last; each name right-aligned (`category-label`); a thin bar from zero in
proportion to its value (`bar action-bar`, `--action-bar` of the `--action-pitch` thick, the
longest `--action-bar-length`, in light grey #D9D9D9, not a colour of the series); the value,
bold (`value-label text-strong`, G7.58s); and a dot (`action-dot`, `--action-dot` across) only
where `data.actions` names a status: `good_strong`, `good`, `bad`, `bad_strong` (D-1: the
thresholds are the maker's, the book leaves them to its website). The dots are good or bad,
green and red, textured in black and white (6.12.3, D-18). No
statuses at all is warned (R12-4).

**The funnel's replacement** (`layout/funnel.py`, `chart.type: funnel_bars`; R12-5, G7.59 [L];
the chooser gives it for `facts.message: stage_conversion` or in place of a funnel): x ordinal
(the stages' own order; nominal refused, R2-02), one series of counts. (1) The counts as
vertical bars in stage order, laid out by the bar layout (its scale and unit phrase, or value
labels by G2.18). (2) Directly below, under a `panel-title` heading
(`labels.notes.stage_change`, default "% change from each stage to the next"), the change from
each stage to the next, (next − this) / this in % to one decimal, computed here, as bars on
their own scale (a change: signed labels, the signed zero line), half as high as the counts'
plot, laid out by the bar layout on the same plot edges without category labels, each bar then
moved between the two stages it links and given a stage bar's width (s. 0328). A count not
known leaves both changes beside it not known. The book draws the loss bars red; here they keep
the single colour, the losses being expected rather than judged.

### 6.18 Stacked bars, waterfalls and totals over time (phase 3; as built, "(stacks)")

Notes 07 §4.1–§4.3, §5.4, §5.5, §5.9; R7-08, R7-11, R7-17, R7-20, R7-24, R7-27–R7-30, R7-33;
G1.37, G1.38, G1.41, G1.43–G1.45, G5.4, G7.31–G7.33; D-9, D-22, D-24, D-26, D-31, D-39, D-43.
The types are `stacked_bar`, `stacked_bar_100`, `waterfall`, `stacked_area`, `stacked_area_100`
and `line_total`, which the procedure of §3.2 gives after the four checks.

1. **Bars in stack mode.** A stacked chart is laid out by the bar layout itself under a `Stack`
   (`layout_bars(ctx, stack=…)`, `layout/bars.py`), so the names under the bars and the
   orientation (6.7.1–2), the value scale and its unit phrase (6.4), the time axis (6.3), the
   title, notes and legend component are the bars'; the Stack says what differs. Without one,
   every bar chart is laid out as before (the phase-1 and phase-2 sources byte-identical). The
   waterfall (`layout/waterfall.py`) and stacked areas (`layout/areas.py`) are Stacks too; lines
   with a total panel are a merged chart (6.10).
2. **The data.** Several totals: x the totals (categories or periods), a series per part (07
   #12). One total: one series over its parts, as a pie's data, drawn as one stack with no name
   under it (a category of role `total`, which the top shows, left out). The scale (the hook
   `bars.stack_scale` in compute_scale) holds each total's positive and negative sums, a
   waterfall's running totals, or a 100 % stack's own 0–100 scale (6.1 with 100 the maximum).
3. **Order.** Totals: nominal by their totals, largest first (R7-20, G2.1), a catch-all, then
   totals with a part not known, last (R9-9); ordinal and periods in order. Parts (G7.31,
   D-26 (b), R7-24): those the message names at the base, larger first, then the rest largest
   first by their sum, "All others" at the top in its grey (R7-17, G4.15); one order and colour
   in every total, the colours the palette's slots in stacking order (D-22 (a); a `slot` kept).
   Charts of one set showing the same parts fix their slots in the spec, so that a part keeps
   its colour as a line, a band or a segment (R3-24, G4.22): 07 #14's six charts and P3-38
   give the regions the book's colours (East blue, West orange, North olive, South gold), which
   stacking by size would otherwise hand out as East, West, South, North.
   Ordered parts (`data.parts.kind: ordinal`, or an ordinal x of one total) keep their order,
   the first at the base, in steps of the style's sequential ramp (`seq-k`, darkest first,
   steps 3–9: the palest, lighter than the gridlines, left out, G3.2; G4.7, R3-8). Parts in
   groups (`x.groups`, one total; 07 #10, D-24 (b)): the largest group at the base, each
   group's parts together in their given order, the second in a tint of the first (`tint`).
   Parts of several totals in groups (`data.parts.groups`, {series: group}; 03-04 #7, s. 0115;
   G4.9, R3-11, D-49): the largest group at the base, each group's parts together in their
   given order, coloured by `ramps.grouped` (6.14.6): the group's hue (its slot from the base,
   or its first part's fixed slot), its parts the shades `shade-light`, the base and
   `shade-dark`, the darkest in the middle; named by the grouped legend (`legend.grouped_row`:
   a row above the plot, each group's parts side by side over a `group-rule` in its hue and its
   name, `text-group-k`), stacks upright or lying down, in place of names beside the last stack
   (the legend shows that the shades are members, not amounts: G5.12, D-49; P3-39). More than
   three parts in a group, or an All others part, give a hue each (a guided warning). The SVG
   lint's balanced-lightness check (R3-6) leaves the shades out. A
   subtotal message with named parts draws them in their strong shades and pales the others
   (R7-08, D-9: the procedure's, not a guided departure). **A fixed order** (owner, 1 Oct 2026;
   `data.parts.order: fixed`, a fact: the data's order is shared, a table with several charts
   ordered differently): the parts stack in the order given, the first at the base, a group's
   parts together, and several totals stand in their given order, whatever their size or the
   message; a guided decision (R7-24, 02 D1) naming what size would have given (P3-30 `fixed`:
   East, West, North, South, the book's; P3-39 `fixed`: Asia at the base, the book's).
   `data.x.order: fixed` (2.2) keeps the totals' order alone, or one total's parts. The
   chooser's sort (3.2, `choose.py`) gives `given` for a breakdown drawn as plain bars whose
   parts' order is fixed, and for any bars whose `x.order` is, with the reason "the data's
   order is fixed" (guided); a table under the bars follows them.
4. **Segments** (`segment` with the part's colour class, `data-share` in a 100 % stack): each
   total's parts end to end from zero (negative parts down, only through an override of
   R7-04, the book's s. 0236), parted where two meet by a true gap in the page colour,
   `--segment-gap` (0.1 em), half from each, so the base stays on zero and the top on the
   total (G7.32, R7-33). A part of no length has no rectangle. A part not known: the stack is
   not drawn, where the parts above it start being unknown, and one note names it ("(HR
   missing)", R0-12, R9-7), placed as every vertical slot's note is (6.8, owner, 1 Oct 2026):
   wrapped in the empty slot on the baseline, between its neighbours' facing edges (an area's
   bands end at their slots' centres: `Stack.clear_span`), else under its slot's label, else
   above, over the stacks or bands it would cover (`Stack.tops_over`), with a leader to the
   slot (P3-38 and its `bars`: "(North / missing)" in May's gap). The comparison of
   alternatives that led to it (a pale band, a bracket, a footnote "May*") is not kept: no
   `--missing-note-mode`. One total stands upright (07 §5.4), the plot as high as
   a chart the figure's width, the bar `--single-stack-width` (0.35) of that height.
5. **Value labels** (G2.18, G7.4, G5.4): the segments' values counted against the scale's
   labels; each centred inside its segment in the text grey where that reads on every part's
   colour at 4.5 : 1 and fits. On the plain palette it never reads, so one total's values
   stand before its parts' names beside the stack (a right-aligned column), and several totals
   keep their scale. With the totals the message (`compare_totals`, R7-20), each total just
   past its stack's end, formatted as a set with the scale it stands beside (owner, 1 Oct 2026:
   totals and labels follow the scale's magnitude): its magnitude and at least its decimals, and
   its form, bare numbers where the scale states the unit once ("3.1" beside "$3.5 million in
   expenses"; under a horizontal scale's title "320", never "320K") or the unit on each where
   the style's scale labels carry it (`--scale-label-style: suffix`). Labels that replace the
   scale (one total's values, a waterfall's) carry the unit themselves, the unit's words on a
   line above (6.4). **A line crossing a label stops short of it** (owner, 1 Oct 2026): a value
   label a gridline (the zero line among them) would cross takes the time labels' knock-out
   halo (`bars._label_halos`: a copy of it, `text-halo`, in a `divider-halo` group appended to
   the gridlines' group, so it lies over the gridlines and under the marks); the message layer
   does the same for its selective labels (P2-46 `selective-labels`). SVG lint CG-7 (G9 55):
   a value label crossed by a gridline or a connector has its halo, drawn after the line.
6. **Naming the parts** (G5.4, G5.8, D-7, D-31): beside the last stack (an area's last
   period), level with each segment's middle, packed top to bottom a gap past the plot's right
   edge; else (D-7's test, `labelling: legend`, or the last stack not drawn) a legend column at
   the right, top to bottom as stacked (D-31 (a)). A thin leader (`leader`,
   `bars._stack_leaders`) runs from just past a segment's edge, at its middle, to its name (or
   to the value standing before it) where the name moved past D-7's free pitch, and also where
   the name is no longer level with its own segment, its middle past the segment's ends by more
   than half its cap height: beside a stack a name reads as whatever it is level with, so a
   thin part's name packed off it would name its neighbour or nothing (P3-35's three thin
   parts, P3-27's Development). Stacks lying down: a legend row above in the stacks' order
   (00-01 F3); direct labels there are a later stage.
7. **A 100 % scale's unit** (6.4): "100% of total"; the stacks filling the top gridline, the
   words go to their own line, "Share of total <measure>" (the book's "100%" alone leaves the
   scale without its unit, G2.19); under a horizontal scale "Share of total <measure> (%)". A
   measure already in % keeps its own unit.
8. **Waterfalls** (R7-11, G7.33, D-43): one series, its steps in the given order (never
   sorted); a category of role `total` is a full bar from zero (`bar total`), the first a start
   balance, a later one a subtotal or end total checked against the running total (a warning
   past the data's last decimal, D-54); each step a floating `segment` from the running total
   before it, rises `series-1` and falls `series-2`, or `good` and `bad` by `measure.good`,
   green and red, hatched and dithered in black and white (6.12.3, D-18);
   a `connector` hairline (`--connector`, the signed zero line's grey, ≈ the book's #A9A9A9) at
   each running total from one bar to the next, in a `connectors` group after the marks;
   labels signed for the steps, unsigned for the totals, past each bar's far end, or past its
   near end where the far one would lay the label across the zero line or over another bar (a
   fall ending just above zero, a rise just below it: P3-37 `labelled`); upright only (a
   horizontal one asked for: a guided finding); a step not known refuses (R7-11). Rises, falls
   and totals each make a series group of their own (the SVG lint's R3-4). Running totals may
   go below zero: the scale holds them all and the zero line is signed (P3-37).
9. **Stacked areas**: x a regular time grid, the bars' slots and time axis; each part a closed
   `<path class="area …">` per run of periods with the stack drawn, its top edge through the
   slots' centres, its bottom along the part below; the bands touch; no value labels. A period
   whose stack is not drawn (a part not known) breaks every band there, the runs either side
   ending at their last slots' centres (P3-38); a part of zero is known, and its band narrows to
   nothing at that slot and runs on (`keep_empty`).
10. **Lines with a total panel** (R7-27, G1.43, G2.32): drawn from `chartgen.render` by
    `areas.render_panels` as a merged chart (6.10) of the total above the parts, each on its
    own scale, the time axis once; the total is the spec's series of role `total` (checked
    against the parts, D-54) or their sum per period ("Total"), its panel headed by its name;
    in a merged chart each panel of several series now names its own (multiples.py).
11. **Checks.** Model lint `bars.check_stacked` (lint `stacked`): one order of the parts (R7-24),
    end to end from zero (R2-24), 100 % adding to 100, a waterfall's steps from the running
    total and its totals from zero (R7-11), no text over a segment but its own label (CG-1).
    SVG lint `svglint.stacks` (R2-24, R7-11; G9 22): each stack's segments (by `data-x`) end to
    end from the zero line, at most 0.3 em apart; a waterfall's step starting where the bar
    before it ended; `empty_bars` counts segments as a slot's bars. The four checks are the
    chooser's; the layouts refuse only what they draw (a mixed stack the chooser let through,
    R7-04; a waterfall's step not known).
12. **Not built.** The stacked inset of 02 #10b (P3-35 shows the stack alone); direct labels on
    stacks lying down; the message layer's comments, connector and highlight methods on stacks
    (CG-3, as for the other phase-3 types); the dual scale of R7-18 (07 #2); a parent stacked
    from its sub-parts in one colour, named inside (07 #10's bar version).
13. **For the owner to judge.** (a) One total's values standing before its parts' names when
    the procedure labels them (P3-22 `drawn`, P3-26, P3-35); (b) the totals labelled when they
    are the message (P3-27), which the book's figure is not; (c) the 100 % unit line (P3-32,
    P3-33); (d) parts stacked by size over time (D-26 (b): East, West, South, North), where the
    book keeps the table's order, in the book's colours fixed by slot across the set (so the
    stack reads blue, orange, gold, olive from the base), and the part the message names at the
    base instead (P3-33 `west-first`: the West's share on the baseline); (e) the first ordered part darkest
    (P3-34); (f) a waterfall's rises and falls blue and orange without a good/bad reading, green
    and red with one (under a safe-pair style the two would look the same); (g) one stack 0.35 of its
    plot's height wide; (h) the total's panel headed by its name rather than labelled at its
    line's end, and the merged chart's gap between the panels (8 pt, P2-66's), which sets the
    top panel's 0 close above the parts' top label (P3-29); (i) leaders to a thin part's name
    only where it left its segment's level (P3-35: North and Int'l, not West between them);
    (j) decided (owner, 1 Oct 2026): a gridline crossing a label stops short of it, the halo
    (item 5); (k) decided (owner, 1 Oct 2026): an empty slot's note wrapped first, then under its
    label, then above with a leader (6.8, item 4); (l) the unit line of a labelled stack or
    waterfall given by the spec (`unit.fallback`: "donations per donor"), its noun alone ("in
    donations") reading badly on a line of its own; (m) parts in groups stacked by group size
    (P3-39: Africa at the base, where the book has Asia first; `fixed` keeps the book's order)
    and named by the grouped legend above even when the stacks stand upright (their colours
    then run bottom to top, the legend left to right); (n) decided (owner, 1 Oct 2026): the
    totals follow the scale's form, bare beside a scale that states the unit once (item 5).

### 6.19 Notes under the chart and their marks (owner, 1 Oct 2026; as built, "(notes)")

The owner: "tables need to be able to have footnotes, as do charts in general, as a comment or
label might need them". In the evaluation (round 1, B12) a table could not say what "POA" and
"n/a" meant, and notes had nowhere to go. `chartgen/notes.py` draws them for every chart type and
for tables; the guide has no rule of its own yet (one is proposed).

1. **Two kinds of note** (`notes`, 2.1): a **footnote**, referred to by a mark set on what it
   explains, and a **plain note**, with no mark (a general remark, "POA: price on application.
   n/a: not offered."; the data's source, "Source: …"). A note is a sentence or two of the
   author's; the generator writes neither the note nor its mark's text, only the mark itself.
2. **Where a mark goes** (`notes[].on`, one or more places sharing one mark): `{"element":
   "title" | "descriptive" | "unit" | "x_title" | "names_title"}`; `{"category": c}`, the
   category's name where the chart writes it (a label, a table's column heading, a slice's or
   tile's name); `{"series": id}`, the series' name (an end label, a legend entry, a table's row
   name); `{"series": id, "category": c}`, that value (its value label, its table cell, the note
   in its empty slot, a slice's value line); `{"status": s}` (`missing`, `unknown`, `pending`,
   `tbd`, `error`, `zero`), every value of that status where its words are written (`labels.notes`:
   an empty slot's "(missing)", a table cell's "POA"), narrowed by `series` or `category`;
   `{"comment": k}`, the k-th comment (from 0); `{"stub": j}`, a table's stub column's heading,
   with `series` its entry on that row. The mark follows the text as drawn: the last line of a
   wrapped one; a name or an element written more than once (a panel's labels, an inset's
   copies) is marked where it is first read; a status's words wherever they are written.
3. **A value's own note** (2.4): `{"status": "tbd", "note": "Price on application."}`, or
   `{"value": 45, "note": "Estimate."}`, is a footnote on that value (as `{"series", "category"}`);
   values with the same note share one note and one mark; a note that only repeats the words of
   its slot ("(missing)") is left. Notes a transform makes (6.12.1) are the generator's and never
   drawn.
4. **Room** (before the layout): the layout's Batch (`notes.NoteBatch`) measures every text that
   will carry a mark wider by the mark's room (the widest of the chart's marks, `--note-mark-gap`
   after the text; for k notes on one text, k marks and their commas), so that the layouts keep
   it clear as they place, wrap, sort and test texts: a name under a bar may wrap or the bars turn,
   an end label's plot narrows, a comment's block widens. Every line that may end a wrapped label
   is measured so. The unit phrase, which the layouts compose, is matched by its last word.
5. **Marks** (after the layout and the message layer, before the lint; `notes.apply`): each place
   is found among the texts drawn, by role and words (a value by its mark's `data-series` and
   `data-x`, or a table's cell), never in an inset or the footer. The marks are numbered in
   reading order (rows of texts top to bottom, each left to right) in the style's sequence
   (`--note-marks`): letters a, b … z, aa … (the default: beside figures a raised numeral can read
   as an exponent), numerals 1, 2 …, or symbols *, †, ‡, §, ‖, ¶, then doubled. A note referred
   to twice keeps one mark; a text with two notes carries both, "a,b". A mark is a text of its own
   (`note-mark`), in the note style (`--note-mark-size`, `--note-mark-slant`, `--note-mark-ink`:
   0.8 em, italic, the note grey), set `--note-mark-gap` after the text, raised so that its
   x-height (letters, numerals; a symbol's figure height) stands at the text's cap height
   (`--note-mark-lift` more). The text then moves back by the room it was given, so that text and
   mark keep the text's place (a right-aligned name and its mark end at the names' edge; a centred
   name and its mark are centred); a table's cell keeps its place and its mark hangs in the gutter,
   so that the figures of its column stay aligned (G6.17). On a fill (a tile, a heat cell, a
   highlighted cell) the mark takes the text's colour classes, then `on-fill` (the text grey), so
   that it reads there as the text does (G6.5). Text and mark share a `joint`, one label for the
   collision test.
6. **The footer**: below everything else (the lowest ink, `--notes-gap` above the first note's cap
   tops; after the chart's own notes, such as a treemap's, at the notes' pitch), left-aligned with
   the chart and wrapped to its width at the notes' pitch (`--line-pitch` × `--note-size`): the
   footnotes in the order of their marks, each its mark (raised, or on the baseline with
   `--note-footer-marks: baseline`) then its text (`footnote`), `--note-hang` apart, the lines after
   the first indented to the text; then the plain notes (`chart-note`), in the spec's order. Lines
   break at spaces, never before "&" nor between an abbreviation and its number ("Aug. 1"); a note
   in Typst markup is set whole.
7. **What cannot be drawn is said** (CG-8): a place no drawn text answers (warn: a series' name in
   a chart that writes none, a period's label the time axis thinned, a status no value has; info
   for a value's own note with no text to mark, a gap in a line), the note then standing under the
   chart without a mark; a mark past `layout.width`, or a note's word wider than the chart (error:
   the figure is widened to keep it, never clipped). The model lint's collision test (CG-1) and the
   SVG lint's floors (R5-7: notes and marks at least 0.8 em; R5-8: 4.5 : 1 on what lies under them)
   apply to marks and notes as to every text; the SVG lint's `footnotes` check (CG-8) reads off any
   drawing that every mark in the chart has its note in the footer and every footnote's mark is set
   somewhere above, and that the notes lie below everything else; the style lint checks the notes'
   and marks' sizes against the floor and `--note-marks` against the three sequences.
8. **Not built.** A guide rule (proposed in the builder's report); a source line of its own (the
   guide has no rule on stating the data's source: a plain note "Source: …" serves); marks on texts
   in an inset; a period's label on a time axis as a category's place (its label is the period's,
   and may be thinned).
9. **For the owner to judge.** The marks' look: their sequence (letters, numerals, symbols), slant,
   grey and size, and the notes' own marks raised or on the baseline (`examples/review/r1-footnotes/`,
   seven looks on a table, a bar chart and a line chart); a value's own note drawn as a footnote by
   default; footnotes before plain notes; a mark on a fill in the text's colour rather than the note
   grey.


## 7. Chart types and phases

**Phase 1 — scales, labels and insets** (what shows the book's value
fastest). Built in stages: stage 1 (30 Sep 2026) is the foundation and bar
charts, vertical and horizontal, one series, over categories or periods,
with the regular-grid time axis bars need, the unit phrase and its fallback,
missing notes, highlighting by the strong shade, guided findings for the
options it implements, the plain style and the examples document; later
stages add lines with end labels and full time axes, dot plots, insets and
the full lint (style, model and SVG):

- types: **bar** (vertical and horizontal, one series), **line** (one or
  more series on a regular grid, gaps and markers, projections, reference
  series; stage 2, with end labels packed or a legend, highlighting by
  thickness, the zero rules for lines, and over ordinal categories for a
  data-savvy audience), **dot plot** (time: sparse series; categorical: non-zero lower
  limit allowed with a theoretical minimum or truncation facts; stage 2,
  one series), and the **inset** on all three (outlier and narrow range;
  built in the inset stage, 6.6);
- scales (6.1), formatting as a set (6.2), the unit phrase with its
  fallback (6.4), multi-level time axes on regular grids (6.3), wrapping
  and orientation (6.7), missing values (6.8), end labels and the legend
  fallback (6.5), highlighting by strong colour and thickness (6.9);
- the chooser for time series (06 DP6-1, DP6-2), orientation, sort and
  labelling; refusals for everything phase 1 cannot draw correctly, naming
  the type the book recommends;
- lint: style lint; model and SVG lint for the rules above (R0-06, R0-07,
  R0-10, R1-02–R1-09, R2-01–R2-12, R2-21–R2-28, R2-32–R2-35, R2-37–R2-43,
  R3-2, R3-6, R3-16, R3-18, R3-19, R3-21, R4-1, R5-6–R5-8, R5-11, R5-14,
  R5-17, R5-20, R5-21, R5-26, R5-27, R6-2, R6-9, R6-12, R6-22–R6-28, R6-32,
  R6-35, R6-38, R6-42, R9-5–R9-9, R15-6);
- the plain and plain-office styles, the bake change of 1.6, the examples
  document with its build.

**Phase 2 — comparison, several series, other time charts:** clustered bars
with a top legend (R9-12, R4-4, R4-6; built, series: 6.7.4, examples P2-01
to P2-11, with dot plots of several series, 6.7.10, bars with flat reference
markers, 6.7.11, value labels, 6.7.12, a table with the graph, 6.7.13, and
the legend component, 6.7.14); merged bars and index charts instead
of dual axes (R2-14–R2-17); direct value labels and a table aligned under
the graph (R2-31, R2-32, R10-7); small multiples (6.10; built with the
merged charts, panels: examples P2-60–P2-70); comments and title
connectors (R5-5, R15-1, R15-2); comparison values: target marks, reference
lines, reference series (R15-7–R15-9); good/bad colouring and deviation,
ratio and index transforms (R15-11–R15-17) (these, the highlight methods and
good and bad's static output, red and green in one file, textured in black
and white, a safe-pair style's two files (D-18), built as the message layer, 6.12, examples
P2-40–P2-59); step and thin-bar charts on
continuous time axes; arrow charts with absolute and % panels (R6-15,
R6-17); incomplete periods (R2-42); black-and-white symbols (R6-30) (these
built as the time charts, 6.13, with bumps charts, brought forward from
phase 3, and the lint checks of 6.13.8: examples P1-26, P1-27, P2-20–P2-26); bars
with flat reference markers (R11-4); highlight methods border, glow,
background, icon, selective labels; good and bad's static output (R3-20,
D-18: one file; two under a safe-pair style).

**Phase 3 — breakdowns, ranks, tables:** pie (6.11), stacked and 100 %
stacked bars, multiple stacked bars and pies, waterfall, treemap (squarified),
wrapped bars, stacked and 100 % stacked areas, lines with a total panel (07
§4.1–4.3); bumps charts with flat-tangent curves (R6-40; built in phase 2,
6.13.6); slope charts
(the rare exception); heatmaps (R6-36, R12-6); tables and in-table graphics
(R10-12–R10-25, R10-9); action dots (R12-4); funnel replacement (R12-5);
sequential, diverging and grouped palettes with their keys (R3-8–R3-15,
R4-8, R4-9); the full chart chooser across all groups (05 DP-5, 07 §4,
08-12 DP-2, DP-6, DP-7). (Chooser, as built: the procedure of 3.2 with its
path, the four checks and the avoid-list; slope charts through the line
layout, 6.13.10; the week's first day and unequal bins, 6.13.11; examples
P3-60–P3-79, tests `tests/test_chooser.py`.) (Tables, as built: colour ramps and
their keys, 6.14; heatmaps over time and heatmap tables, 6.15; a table alone with
its in-table graphics, 6.16; action dots and the funnel's replacement, 6.17;
examples P3-40–P3-55, tests `tests/test_tables.py`.) (Stacks, as built: stacked
and 100 % stacked bars of one total or several, waterfalls, stacked and 100 %
stacked areas and lines with a total panel, 6.18; examples P3-20–P3-39, tests
`tests/test_stacks.py`.)

**Phase 4 — maps and house styles:** tile maps (geometry-free), then coloured
dots, bubbles, bars and small lines on regions over base geometry (08-12
DP-1; needs base maps as data); house styles as further stylesheets (and
bake effects keyed on role classes).

Never: 3-D (R12-7, R12-8), smoothing of data lines (R6-39), dual axes of
different variables (R12-1), broken or log scales (R2-18, R2-19),
animation and filters in static output (R13-1, R14-1), word clouds and
packed bubbles (R12-2, R12-3); donuts only through an override.


## 8. Test cases

### 8.1 The notes' test figures by phase

| Notes | Phase 1 | Phase 2 | Phase 3 | Phase 4 / not planned |
|---|---|---|---|---|
| 00-01 | F1 (+ heavy and over-pruned fixtures), F4, F6, F7 (without its arrows), F5 (without its arrow) | F5, F7 arrows | F2 table, F3 100 % stacked | |
| 02 | #1, #2, #3, #5 (unit phrase and title lint), #9, #10a (line inset), #11, #13, #14 (insets and truncations), #15, #17 (unit test), #18 (day and month rows), #4 and #12 (refusal fixtures) | #6, #7, #14 level labels, #16, #18 (thinned and narrow variants), #19, #20 (time: #19 P2-24; #20 not writable, 6.13.9) | #8 allowed dual axis, #10b stacked inset | |
| 03-04 | #2, #4 (five lines), #10a, #16 (lines), #18 (packing failure → legend), #1 and #3 (lint fixtures) | #11, #12, #17 | #4–#7, #9, #10b, #14, #15, #16 pie | #8, #10c maps |
| 05 | #1 (style count), #4, #5 (font check), #6 (formatter tests) | #8 | #2, #3, #7, #9, #10 | |
| 06 | T1, T2, T4, T10, T11 (colour, emphasis), T12, T15–T17 (refusals), T18 | T3, T6–T9, T11 (B&W), T13 (time: T3 P2-23, T5 P2-25 brought forward, T6 P1-26, T7 P1-27 (its weekly totals not: 6.13.9), T8 P2-20, T9 P2-21, T11 P2-22; T13 with the panels, P2-64) | T14, T16b | |
| 07 | #6 (refusal: pie of averages) | #12 clustered and merged, #13 | #1–#5, #7–#10, #12 stacked and pies, #14–#16 | #11 Euler |
| 08-12 | #1 (bars), #2, #3, #4, #12, #13 | #5, #7, #11, #17 | #6, #8–#10, #14–#16, #19 | #1 and #18 maps |
| 13-15 | F3 (bars, highlight) | F1, F2, F5–F9, F11–F18 | F4 bar grid, F10 | F4 scatter matrix |

**Phase 3, one total's breakdown (breakdown).** Built as P3-01–P3-18
(`examples/specs/p3-0*.json`, `p3-1*.json`; tests in `tests/test_breakdown.py`;
6.11–6.11b), the book's single-total figures of 07 and the pies of 03-04,
05 and 13-15:
- P3-01, 07 #1 (p. 238–241): "Mrs. Smith contributed almost a quarter", a
  pie, the named slice in its strong shade; variants `percent` (s. 0222),
  `legend` (refused, R7-34), `legend-overridden`, `treemap` (the five parts
  as a treemap, a guided finding). P3-02, 07 #1 (p. 242–244): a subtotal as
  a fraction, the two parts strong and the others paled.
- P3-03, 07 #3 (p. 249–250): the top five and All others, last and grey;
  variant `fifteen-slices` (s. 0225, R7-15, R3-22, its small slices on
  lined-up leaders).
- P3-04, 07 #4 (p. 245): a hundred countries (synthetic, the book's head and
  a long tail) as a wrapped bar chart at 480 pt; variant `text-width`.
  P3-05, P3-06, 07 #5 (p. 247–249): the same as a flat and a grouped treemap;
  P3-05 variant `values` (each share under its name where both fit, ours).
- P3-07, 07 #10 (p. 259–262): overlapping categories separated, grouped, in
  tints. P3-08, 07 #15 (p. 287–292): the formatting sequence's end, East's
  $0K on a leader at 12 o'clock; variant `unsorted`. P3-09, 07 #15 (p.
  293–294): West darker, the others paled; variants `border`, `exploded`
  (refused, R7-38), `exploded-overridden`. P3-10, 07 #16 (p. 295–297): the
  donut's data as a pie; variants `donut` (refused, R7-39),
  `donut-overridden`. P3-11, 07 #12 (p. 269–271): a pie per region;
  variant `top-three` (s. 0249 chart C: pies of three departments, refused,
  R7-03).
- P3-12, 05 #2 (p. 152): hidden digits in a pie's labels ($583K, $196K).
  P3-13, 03-04 #16 (p. 144): name and percentage outside, where the book's
  bad version keys a legend (variant `legend`, refused). P3-14, 03-04 #4
  (p. 120): the balanced palette on a pie.
- P3-15, 13-15 F10 (p. 402): Mrs. Smith's slice outlined, the names alone;
  variant `columns` (the book's bad columns, drawn with the chooser's
  guided R7-07). P3-16, 05 #9 (p. 158): the cheat sheet's pie, the insight
  in a comment with an arrow into the West's slice (6.12.5 on a pie).
- P3-17 (ours, 00-01 F3's and 13-15 F10's data): the aggressive portfolio's
  ordered risk levels as a pie, in their order and in steps of one hue;
  variant `hues` (taken as unordered: largest first, a hue each). P3-18,
  03-04 #6 (p. 123): six regions in a categorical palette, where the book's
  bad version shades them in one blue (a bad version a spec cannot ask for:
  colours are the style's, the parts' kind the data's).
07 #6–#8 (the checks), #9 (waterfall), #14 (over time), #12's stacked bars
and 00-01 F3 (100 % stacked) are the chooser's and the stacked charts'.

**Phase 3, totals stacked (stacks).** Built as P3-20–P3-39 (`examples/specs/p3-2*.json`,
`p3-3*.json`; tests in `tests/test_stacks.py`; 6.18), in the examples document's
section "Stacked charts, waterfalls and totals over time", each chosen by the procedure:
- P3-20, 07 #1 (p. 242–244, s. 0223): a subtotal as an amount, one stack, Smith and Jones
  at its base in their strong shades, the others paled; variant `precise` (the bar chart of
  R7-06). P3-21, 07 #3 (s. 0230): the top five and All others in a 100 % stack, for an
  audience averse to pies (D-39); variant `fifteen-parts` (s. 0225, R7-13, R3-21 overridden).
- P3-22, 07 #7 (s. 0234): the three largest donors stacked, refused (R7-03); variant `drawn`.
  P3-23, 07 #8 (s. 0236): a stack of mixed signs, refused (R7-04); variant `drawn`, the
  losses down from a signed zero line.
- P3-24, 07 #9 (s. 0237): the waterfall; variants `labelled` (signed labels), `good-bad`
  (green and red), `horizontal` (drawn upright, a guided finding). P3-25 (ours, D-43): a start
  balance, a subtotal and the end balance; variant `contradiction` (D-54).
- P3-26, 07 #10 (s. 0240): overlapping categories made disjoint, stacked by group in tints,
  the values before the names; variant `overlapping` (refused, R7-05).
- P3-27, 07 #12 (s. 0247): multiple stacked bars, the totals labelled; variants `legend`
  (the book's), `fractions` (100 %, D-39), `missing` (a stack not drawn, its note).
- P3-28–P3-33, 07 #14 (s. 0256–0260): the six over-time charts, a line per part, lines with
  a total panel, stacked areas, stacked bars, 100 % areas, 100 % bars, each region in the
  book's colour in all six (fixed slots, R3-24); P3-30 `fixed`: the data's order fixed, the
  book's stack (East, West, North, South).
- P3-34, 00-01 F3 (s. 0020): ordered risk levels in 100 % stacks lying down, steps of one
  hue, the legend row above; variant `hues` (unordered: a hue each, by size).
- P3-35, 02 #10b (s. 0077): one stack with three thin parts at its top, the values before the
  names, leaders to the names packed off their parts; the book's inset on it is not built.
- P3-36, 07 #1 and §4.1 (s. 0223–0224): a subtotal stated as a fraction for an audience averse
  to pies, a 100 % stack, its two parts at the base though neither is the largest.
- P3-37 (ours, D-43): a waterfall whose running total goes $85K below zero, the zero line
  signed; variant `labelled` (a label kept off the zero line at its bar's near end); good and
  bad in green and red; variant `bw` (black and white: the falls hatched, the rises dithered).
- P3-38, 07 #14 (s. 0258) and R0-12: a look-up chart, stacked areas with one part of one month
  not known, every band broken round it, its note lifted clear; variant `bars`.
- P3-33 `west-first`, 07 #14 (s. 0260) and D-26 (b): the 100 % stacked bars with the part the
  message is about (the West) at the base of every stack.
- P3-39, 03-04 #7 (s. 0115): stacks lying down of four crops' production, the countries grouped
  by continent in shades of a hue per continent, named by the grouped legend, the totals bare
  numbers as the scale's; variants `fixed` (the data's order, Asia at the base as in the book)
  and `hues` (the book's first version, a hue each).
P2-04's `compare-totals` variant (multiple stacked bars) is now drawn. The chooser's examples
P3-60 (`subtotal`, `pie-averse`), P3-64, P3-67 and P3-68 reach these layouts too.

**Phase 3, tables, heatmaps and colour ramps (tables).** Built as P3-40–P3-55
(`examples/specs/p3-4*.json`, `p3-5*.json`; tests in `tests/test_tables.py`; 6.14–6.17), in
the examples document's section "Tables, heatmaps and colour ramps". Every type but P3-50's is
chosen from the facts (`chart.type: auto`: `facts.message` lookup_only, export_only,
not_comparable, profiles, action_needed, stage_conversion, or more than 30 series), the book's
rejected types asked for in variants; P3-50 asks for the heatmap by name, which the facts also
give (P3-73 reaches it from the facts alone); tables are look-up charts (`job.kind: lookup`)
except where noted:
- P3-40, 08-12 #8 (p. 344–349, s. 0314–0318): Funds Raised (US$) by Event by Region, the book's
  formatting sequence at its end: every digit, two rules, a wider gutter before Total, dates
  of fixed width (the book's earlier dates, 21 Jul and 1 Sep; its last version has 21 Jun and
  1 Oct, D-54), the three outliers on a light fill; at 480 pt. Variants `zebra` (s. 0315,
  through an override of R10-16), `stripes-refused`, `bold` (the bold header, totals and Total
  column of s. 0316, through an override of R10-22).
- P3-41, 00-01 F2 (p. 50): Units Produced by Region, the light version, the printed totals kept
  (one more than their rows in January and February, D-54) and "Total" kept (D-52 (ii));
  variant `heavy` (stripes and bold, through overrides; the cell borders and black header band
  have no spec field).
- P3-42, 08-12 #10 (p. 342, 346): Key Metrics, six measures (`data.measures`), not comparable
  (R10-11 (c)): left-aligned, each with its unit, no header; not bold (6.16). The book's
  one-scale bar chart has no variant: once the six measures are one series nothing tells the
  generator their units differ.
- P3-43, 05 #3 (p. 153): inhabitants of three countries, every digit grouped; variant `fr`
  (spaces as separators, R5-15). 05 #7's column of dates is P3-40's date column (and
  `fmt.date_column`'s tests); 05 #2 and #9 (pies) are P3-12 and the breakdowns'.
- P3-44, 08-12 R10-11 (b): Units in Stock by Product and Warehouse, a table alone because its
  sole use is importing the values elsewhere (`facts.message: export_only`, `job.kind: other`;
  synthetic): a text column of codes before the names, a zero printed, a count not made as its
  note ("(not counted)"), the totals as the data give them.
- P3-45, 05 #10 (p. 167): Monthly Sales ($K) by Salesperson, the unit once in the title;
  variant `heat`, the gallery's table with graphical elements, shaded.
- P3-46, 08-12 #6 (p. 329–331): Monthly Revenue (US$) by Region, the demonstration's 24 values
  as a table, turned (months down the page) to fit the text width; variant `line`, the same
  data as the graph with an insight title (R10-1).
- P3-47, P3-48, P3-49, 08-12 #9 (p. 339): graphics built into tables (R10-9): the heat table
  (each group on its own ramp, the grand total unshaded; the 5K Walk total one more than its
  row, as printed), the Trend column of sparklines, bars in the cells (the Total column's too,
  on its own scale); P3-49 `dots` (at 560 pt).
- P3-50, 08-12 #16 (p. 361): the radar's replacement, a heatmap table asked for by name,
  diverging about 3 (the rating's neutral middle, D-37 (c)), good above, the Overall Avg. row
  apart, an insight title; variants `radar` (refused, R12-6, the chooser's avoid-list),
  `no-dividing-value` (sequential, D-37 (a)), `sorted` (the models by their overall average,
  best first: a departure from the given order with its reason), `muted` (`chart.cells:
  muted`, with its reason: the steps paled, the scores grey), `rainbow` (a categorical
  palette, refused under R3-9 and drawn through overrides of R3-9 and of the lint's R3-21 and
  R3-4, its R5-8 warnings expected). The cells strong, each score black or white (5.72 : 1 at
  worst); four linear steps a side of 0.5, the T-300's overall 3.0 in the neutral step.
- P3-51, 06 T14 (p. 221): 51 states by 50 years as a heatmap over time (synthetic data after
  the book's shape: a level per state, epidemics every two or three years, a fall after 1963;
  the vaccine line not drawn), the key above the grid, the years not known grey. Skewed (one of
  seven equal steps would hold 80 % of the values), the pattern the message
  (`facts.ramp_focus: pattern`), rates spanning four orders of magnitude (the 95th percentile of
  the positive values about 10,600 times the 5th): logarithmic steps, 0 | under 1 | 1–10 |
  10–100 | 100–1,000 | 1,000 or more (G4.33, automatic); variants `natural` (G4.30, with its
  reason: 0 | 120 | 370 | 690 | 1,200 | 2,000 or more), `extremes` (linear, 0–3,500 by 500,
  G4.29) and `quantile` (G4.31, with its reason, the key saying each step holds about a sixth
  of the cells).
- P3-52, 03-04 #9 (p. 126–128): customer satisfaction on 10 by store and department
  (synthetic; `facts.message: profiles`, a heatmap table), diverging about 7, the audience's
  dividing value, the values left out (a guided departure with its reason) so that the key is
  read: linear steps of 1 a side (G4.29), 4–7 and 7–10, the stores' 7.0 in the neutral step at
  the key's tick; variants `values` (the values printed, black or white, no key) and
  `no-dividing-value`.
- P3-53, 03-04 #8, #9 and 06 T14 (synthetic): 32 stations by 42 years of temperature against
  the 1961–1990 average, a heatmap over time (more than 30 series) diverging about 0, a value
  the audience sees as dividing with no good or bad side: blue below and orange above, the
  colour-blind-safe ramp, free of red (G4.17), linear steps of 1 a side to ±3, symmetric about
  0, a year at exactly the average neutral; variants `sequential` (a guided warning, R3-13)
  and `diverging-about-the-middle` (no dividing value: refused, R3-13).
- P3-54, 08-12 #14 (p. 359): action dots, Units Produced by Plant (the month ours); variant
  `bullet` (refused, R12-4).
- P3-55, 08-12 #15 (p. 360): the funnel's replacement, from the book's funnel labels (its bar
  version draws two stages and a change differently, D-54); variant `funnel` (refused, R12-5).
Not drawn here: 03-04 #4–#7, #10 (b) and #15 are pies and stacked bars (the breakdowns', whose
ordered parts take this phase's steps, P3-34; #7's grouped shades and their legend, 6.14, are
drawn by the stacks of several totals, P3-39); 03-04 #14's multi-hued ramps (G4.21) have no
token set yet, the plain style's single-hue steps serving up to seven ordered categories; 06 T5 is the bumps chart P2-25, which the book draws as no heatmap; 08-12 #19, the
decision trees, are the chooser's.

**Phase 3, choosing a chart (chooser).** Built as P3-60–P3-79
(`examples/specs/p3-6*.json`, `p3-7*.json`; tests in `tests/test_chooser.py`;
3.2, 6.13.10, 6.13.11), each chart's path shown under it in the examples
document's section "Choosing a chart". Every type is chosen from the facts
(`chart.type: auto`); the book's requests are variants:
- P3-60, 07 #1 (p. 236–244): one total and three messages, parts compared (a
  bar chart, R7-06), variants `fraction` (a pie), `subtotal` (a stacked
  bar), `pie-averse` (a 100 % stacked bar, D-39), `no-message` (bars, with
  the info, D-1).
- The four checks, each routing a pie request away: P3-61, 07 #6 (p. 252),
  averages (R7-02; variants `pie`, `stacked`, refused, and `facts-missing`,
  the two facts asked for); P3-62, 07 #7 (p. 253), a top 3 (R7-03; `pie`
  refused); P3-65, 07 #10 (p. 259–264), overlapping categories (R7-05; `pie`
  refused; P3-07 is the parts made disjoint); P3-63, 07 #8 (p. 255–257),
  mixed signs (R7-04), a bar chart closed by its Total bar, good and bad
  coloured (`pie` refused).
- P3-64, 07 #9 (p. 257–258): mixed signs with running subtotals, a
  waterfall (R7-11); variant `compare`, bars. P3-66, 07 #3 (p. 244–251):
  fifteen parts as fractions, a treemap (R7-15); variants `compare` (bars),
  `pie` (drawn, with the guided finding). P3-67, 07 #12 (p. 266–271): the
  totals compared, multiple stacked bars (R7-20); variant `pie-averse` (100 %
  stacked bars). P3-68, 07 #14 (p. 278–286): a total over time, 100 % areas
  (R7-30); variants `with-total` (areas), `periods` (100 % bars), `parts`
  (lines), `parts-and-total` (lines with a total panel).
- P3-69, 06 T9 (p. 201–203): South overtaking Central, a slope chart
  (6.13.10); variant `change`, a slope chart asked for, refused (R6-16).
- P3-70, 08-12 #7 (p. 336–343): values only looked up, a table alone
  (R10-11).
- P3-71, 06 T7 (p. 194–195): the weekly totals of P1-27's sales, Sunday
  weeks; variants `monday-weeks` (ISO weeks: the totals fall) and
  `iso-weeks` (invalid under Sunday weeks). P3-72, 02 #20 (p. 110–111):
  decades and half-decades re-aggregated to decades; variants `not-additive`
  (refused, R2-43) and `unequal` (the book's bad version, overridden).
- The avoid-list: P3-73, 08-12 #16 (a radar's data, `profiles`: a heatmap
  table; `radar` refused), P3-74, 08-12 #12 (word counts as sorted bars;
  `word-cloud` refused), P3-79, 08-12 #13 (the top 15 donors as bars;
  `packed-bubbles` refused), P3-75, 08-12 #15 (the funnel's replacement,
  `stage_conversion`; `funnel` refused; `pattern-data-savvy`, the stages'
  pattern for a data-savvy audience, a line by 08-12 DP-2's first question,
  and `pattern`, the same for a general audience, bars), P3-76, 08-12 #14 (action dots,
  `action_needed`; `bullet` refused), P3-78, 08-12 #17 (clustered bars; `3d`
  refused).
- P3-77, 08-12 #1 (p. 300–304): location the message, a map, reported as
  phase 4; variants `bars` (the book's own answer, a bar chart) and
  `choropleth` (refused, R8-5).

**Phase 2, several panels (panels).** Built as P2-60–P2-70
(`examples/specs/p2-6*.json`, `p2-70-*.json`; tests in
`tests/test_multiples.py`): 13-15 F1, the 3 × 5 line grid, one series a
panel, with the book's 15-line spaghetti chart as an overridden variant
(P2-60); F2 (a), regions as panels and the methods the insight compares in
each, with the runner-up rendered (P2-61), and F2 (b), methods as panels
with the regions labelled right of the last (P2-62); F4's bar grid, whose
names do not fit under columns at the text width, so its panels turn
horizontal (P2-63, the book's arrangement a variant); 06 T13, a stack of
twelve line panels on a shared scale, with a scale per panel (the shapes'
message) and the spaghetti chart as variants (P2-64, synthetic data); 02 #6
merged bars, with the one-scale version refused (P2-65); 02 #7 merged time
panels, with the dual axis refused (P2-66); 07 #12, merged bars of several
totals (the top three departments; all six a variant, P2-67); 07 #13 (08-12
#5), three measures, with the one-scale version refused (P2-68); 13-15 F9,
three views with question headings (P2-69); 13-15 F15, raw and rate side by
side (P2-70). F5's filter chart and 1 × 6 grid lack their data (its "Other"
region); F4's scatter-plot matrix is phase 3.

**Phase 2, the message layer (message).** Built as P2-40–P2-59
(`examples/specs/p2-4*.json`, `p2-5*.json`; tests in `tests/test_annotate.py`
and `tests/test_transform.py`; 6.12), charts that answer, persuade or make
aware, with insight titles, except where noted:
- P2-40, 13-15 F7 (p. 400, s. 0366): the clinic chart, the recommended action
  as the title, Clinic C thicker with a glow on March–June, a comment on what
  matters and one at March on what explains it; variants
  `interpretation-free` (refused, R0-10), `lookup` (the same data as a
  look-up chart, drawn), `callout-only` (a descriptive title, the insight in
  the callout, drawn) and `margin` (the comment in a column at the right, as
  the book places it, the chart laid out narrower).
- P2-41, F8 (p. 393): Clinic B's fall, a comment at March (the cause) and one
  at June (the action). P2-42, F8: the transit clinic's bar named by the title
  connector (horizontal bars, as the book's). P2-43 and P2-44, F3 (p. 376):
  the connector from the title to the lowest and the highest bar.
- P2-45, F11 (p. 404): the catalogue on bars, the default (the strong shade)
  and one variant per method (`border`, `glow`, `background`, `icon`,
  `selective-label`, `bold-label`), and `wider-bar` refused (R15-6); value
  labels off, with the reason. P2-46, F11: the catalogue on lines (default
  thickness; `glow`, `bold-label`, `icon`, `selective-labels`: the highest
  and lowest values). P2-47, 06 T11 with 13-15 §5.4: Central's fall glowing
  under the thicker line; variants `background` and `labels`.
- P2-48, F12 (p. 405): targets as flat marks, named once (the bar layout's
  markers); variants `bars-alone` (the report asks for a comparison,
  R15-7) and `red-green` (each bar good or bad by its own target, in red and green).
  P2-49, F14 (p. 411): the deviation from target with a comment; variant
  `raw` (the bars with their targets, the book's first version).
- P2-50, F13 (p. 409): the deviation from budget, red and green, the extreme
  labels naming the sides in their colours, one file; variant `bw` (black and
  white: a hatch over the bad bars and a dither over the good, D-18).
- P2-51, F15 (p. 412–413): accidents per 100K work hours; variant `raw`.
  P2-52, F17 (p. 417): the industry average as a dotted line, the factories
  above it bad.
- P2-53, F16 (p. 414–416): $10,000 invested, with the comment; variant `raw`
  (share prices). P2-54, F6 (p. 386): the regions with their computed average
  (the book's drawn average is not their mean, D-54). P2-55, F9 (p. 397): one
  spray-and-pray view, a question title (job `other`), the deviation in $K.
  P2-56, R15-7–R15-9 and D-48 on F12's data (the 2021 values and
  the sector figure supposed): the targets drawn, last year's values left
  out, the doubtful figure left out with a warning; variants
  `doubtful-drawn` (refused, R15-9) and `previous-only` (drawn, flagged).
- P2-57, 02 #7 (p. 72, s. 0070): sales and satisfaction as % change since
  January, the index chart instead of two axes (P2-66 is its merged
  version). P2-58, F13's call-centre ratings redrawn as a dot plot, the
  target as a dotted line, the centres good or bad by it; variant `bw` (a bad
  dot filled, a good one open). P2-59,
  F3's stable regions (values supposed month by month): a "nothing is wrong"
  insight with a remark on the whole chart, a comment without an arrow.
F1, F2 and F5 (comments in small multiples) wait for the several-panel
branch to pass through the layer; F18's arrows are the arrow chart's
(time); 07 #13 has no good/bad reading.

**Phase 2, several series over categories (series).** Built as P2-01–P2-11
(`examples/specs/p2-0*.json`, `p2-10-*.json`, `p2-11-*.json`; tests in
`tests/test_clustered.py`), look-up charts except P2-04 (an insight title):
- P2-01, 08-12 #7 and 02 #16 (p. 337, 93; s. 0307, 0089): clustered bars
  of three payment methods in six regions, the legend above, and, exact
  values being needed, the table under the graph, as the book draws it;
  variants `no-table`, `labels-on-every-bar` (s. 0088: eighteen labels over
  the touching bars, refused and drawn through an override, each collision
  reported, CG-1, with the guided finding), `horizontal` (named beside the
  last cluster, D-31 (c)) and
  `horizontal-table` (480 pt, the table beside the bars).
- P2-02, 02 #16 (p. 93, s. 0089): four averages against seven scale labels,
  labelled instead; "$54" … "$17", where the book writes cents ("$53.70":
  the fewest digits that tell the values apart, G6.10), outside the bars
  (the book's white on green fails 4.5 : 1, and so would white or grey on
  the muted blue, G7.4); variants `scale` (the book's first version, a
  guided finding) and `horizontal`.
- P2-03, 03-04 #17 (p. 145, s. 0135): four departments in four regions, the
  legend in the order of the bars; "Other depts", a catch-all, in its grey
  (the book: gold) and the regions sorted by Operations (the book keeps
  them unsorted); variant `direct`, the book's "messy" labels on leaders.
- P2-04, 07 #12 (p. 267–268, s. 0245–0246): six departments in three
  regions, parts compared across totals, the two bars of the insight
  highlighted; variants `top-3` (clustered bars where not all parts are
  shown: the guided finding for merged bars, R7-22; the merged version is
  P2-67) and `compare-totals` (multiple stacked bars, phase 3).
- P2-05, 03-04 #12 (p. 136, s. 0126): the book's ten product lines as
  categories and its two markets as series, two colours instead of ten,
  turned and named beside the last cluster; variant `ten-colours`, the
  book's version, refused by the colour count (R3-21) and drawn through an
  override to show why, its legend a grid of four columns filled row by row.
- P2-06, 08-12 #17 (p. 364, s. 0331): the 2-D version, six regions in four
  payment methods, clusters about 2.4 bar widths apart (the book 2.25); the
  3-D version cannot be asked for.
- P2-07, 05 #8 (p. 158, s. 0148): expenses as bars and the budget as flat
  markers, named once beside the first bar with leaders (the book's
  "non-ugly" chart; P1-18 is its bars alone); variant `clustered`.
- P2-08, 08-12 #11 (p. 351–353, s. 0319–0321): the same by month; the names
  would meet March's bar beside January's, so they stand beside December's,
  as the book puts them (and the months take their one-letter names, the
  plot having narrowed for the names); variants `clustered` (the book's
  "only bars") and `lines` (its "only lines", the budget a dotted target).
  The book's first combo, bars and a line for two series of one role, has
  no spec: a series' mark follows its role (G1.59).
- P2-09 (after 02 #15, p. 89, with 2021 scores of ours): credit scores in
  two years as dots sharing each customer's place, named beside Andy's;
  variants `legend` and `horizontal`.
- P2-10 (after 06 T1, p. 172–174, with a second region of ours): two sparse
  series as dots, a legend column in the order of their last values.
- P2-11 (05 #10, p. 165–167, P1-17's data): value labels with a loss below
  its bar, the zero line signed.
02 #6 and #7 (merged and index charts) are P2-65 and P2-66; 08-12 #5 is
P2-68; 03-04 #11 (good and bad colours) is the message layer's.

### 8.2 Phase 1: fourteen charts for the examples document

Each is a spec in `examples/specs/`, with the book's alternative versions as
variants where the generator can produce them (a refusal is shown as its
report). "Expected" is what the algorithms above give; where it differs from
the book, the difference is deliberate and named.

The book's figures with descriptive titles are rebuilt as the job their
source shows: look-up charts (`job.kind: lookup`, no insight), and P1-17,
a gallery thumbnail of a single total, as `explain` without an insight;
P1-15 alone makes the reader aware, with an insight title (D-55).

Stage 1 (30 Sep 2026) builds the bar charts among them: P1-01 to P1-06, the
main charts of P1-07 and P1-08 (their insets are reported as not yet drawn,
CG-3), and P1-14 as bars; P1-15 to P1-17 are added for what stage 1 draws
that no case above shows (highlighting, sorting, negative values). The
expected values below are those of the owner's decisions (10 pt labels;
D-3, D-4, D-20 decided; D-5 and the others as provisional defaults).
The inset stage draws the insets of P1-07, P1-08, P1-09 and P1-19 (each
entry below says how), adds P1-08's variant `wide` (the book's columns with
the inset above) and gathers the four into the examples document's
"Insets" section.
Stage 2 builds the dot plots: P1-13 and P1-14, and adds P1-19 and P1-20,
the two dot plots of 02 #14 (a sparse time series with a narrow-range
inset, and a categorical dot plot on a truncated scale, horizontal, with a
highlight). Stage 2 (lines) builds the line charts: P1-09 to P1-12 (P1-09's
inset reported as not yet drawn, CG-3), and adds P1-21 to P1-29 for what
the line stage draws or refuses that no case above shows: the canonical
multi-level time axis (02 #18), end labels against a legend (03-04 #16,
#18), a truncated line on a two-year month axis and a year axis (06 T2), a
single line and an incomplete month (06 T18), and the chooser's time-series
fixtures (06 T6, T7, T17). Their jobs: look-up charts, except P1-11, which
answers a question, and P1-12, which makes the reader aware, both with
insight titles.

**P1-01 Headcount by Region** — 00-01 F1, p. 46–49 (s. 0046–0049); spec in
2.9. Data East 48, Central 41, South 35, West 33, North 29. Expected: bars
in given (descending) order, 0–60 by 10, "60 (max employees during month)"
on the top line (the unit given as `what`, D-20) with the top gridline starting after it; no frame, ticks or
axis line; the zero line as a light gridline; title "Headcount by Region,
January 2022" (R5-27 folds the book's subtitle in). Demonstrates R1-01–R1-09,
R2-07, 00-01 §5.4 geometry. Variant `over-pruned` (`chart.gridlines: false`
with an override) must report R1-06; variant `no-job` (the job removed) is
refused (R0-07); variant `as-answer` (the job an answer, the same
descriptive title) is refused (R0-10: the title does not state the insight,
G8.2, D-55), while the base, a look-up chart, passes. The book's heavy
state (a) is a
hand-made SVG fixture in `tests/`, which the SVG lint must fail on R1-02,
R1-03, R1-04, R1-05.

**P1-02 Population by Region, 2022** — 02 #3, p. 59–61 (s. 0058–0061). Data
in persons: East 21,000,000; Central 14,500,000; Coast 13,300,000; South
6,800,000; North 2,400,000; unit noun "inhabitants". Expected: 0–25 by 5,
labels 0 … 20 and "25 million inhabitants" (e = 6, `word_on_top`).
Demonstrates R2-05, R2-06, R2-07, R2-09. Variant `all-zeros`
(`scale.magnitude: "none"`) must report R2-05: a guided finding naming the
procedure's millions.

**P1-03 Employees Working Remotely, Jan–Jun 2022** — 02 #15, p. 91–92
(s. 0087). Months 2022-01…06: 45, 97, 92, 94, 46, 84; percent, min 0, max
100; noun "of employees working remotely", fallback "% of employees working
remotely"; `facts.message: periods` (so bars).
Expected: 0–100 % by 20 with "%" on every label; the inline phrase would meet
the February bar (97 %), so fallback A: the unit line above, left-aligned
with the column, wrapped only if needed. Demonstrates R2-28 (no 120 %), the
unit-phrase fallback, time as columns (R6-5, R6-6). Row 2 of the time axis
("2022") is left out, the title giving it (G2.37). Variant `past-limit`
(`scale.max: 120`) is refused (R2-28).

**P1-04 Weight of Products** — 02 #11, p. 81 (s. 0078). Widgets 0.21,
Doohickies 0.20, Bébelles 0.14, Whatsits 0.13, Thingies 0.124, Bobbles
0.06; suffix " lbs.", what "weight of product". Expected: 0.00–0.25 by 0.05,
every label with two decimals, top "0.25 lbs. (weight of product)" (D-20,
decided). Demonstrates R2-21, R5-17 (the bad "0, 0.05, 0.1" cannot be
produced; a formatter unit test covers it).

**P1-05 Weirdness of Real U.K. Town Names** — 02 #2, p. 56 (s. 0055).
Upton Snodsbury, Worcester 8.8; Lickfold, Petworth 8.8; Great Snoring,
Fakenham 8.0; Bishop's Itchington, Southam 7.2; Over Poever, Knutsford 6.3;
Uploders, Bridport 6.0; score, min 0, max 10; measure name "weirdness
score", unit noun "on 10". Expected: horizontal bars (neighbouring names
collide under vertical bars even wrapped), names right-aligned, 0–10 by 2
(capped at the maximum), vertical gridlines, bare labels and the title
"Weirdness score (on 10)" centred below (the book's "Weirdness Score (on
10)", p. 57); the tie keeps the given order. Demonstrates R2-04, R5-26,
R9-10, 6.4 horizontal. Variant `vertical` (`orientation: vertical`) is
refused (R2-08: the names would have to overlap or be rotated).

**P1-06 Donations by Region, 2022 (missing Southwest)** — 08-12 #3, p. 327
(s. 0298); 00-01 F4, p. 19 (s. 0020). $K (`unit.magnitude` 3): Far West
273, Rockies 268, Midwest 215, N. England 195, Gr. Lakes 175, Southeast
172, Plains 144, Southwest `{"status": "missing"}`; prefix "$", noun "in
donations". Expected: 0–300 by 50, "$300K in donations" (e = 3, D-3 (a),
D-4 (ii)) over bare 250 … 0; Southwest last with "(missing)" at the zero
line. At the text width the names cannot keep 1 em apart under columns,
even with every multi-word name wrapped ("Rockies" and "Midwest" are
single words; "N. England" and "Gr. Lakes" do not break after an
abbreviation), so the chart turns horizontal: bare numbers on the scale
and the title "Donations ($K)" below. Variants (stage 1, for the owner to
compare): `wide` (480 pt, the narrowest width in steps of 8 pt at which
the names keep their distance), in columns as the book draws it (p. 327),
with "$300K in donations" on the top label; `wide-horizontal`, the same
width turned horizontal by choice with a reason (a guided info).
Demonstrates R0-12, R9-7, R9-9, R9-10, the magnitude style.

**P1-18 Expenses by Department, 2022** — 05 #8, p. 158 (s. 0148), the
book's "non-ugly" chart without its budget markers (phase 2), so a look-up
chart of the expenses: Operations 534,942; Marketing 377,123; Finance &
Admin 363,122; IT 161,994; R&D 133,800 (dollars). Expected: 0–600,000 by
100,000, "$600K in expenses"; on one line "Marketing" and "Finance & Admin"
would be 6.3 pt apart, under 1 em, so every multi-word name wraps, giving
"Finance & / Admin" as the book does, and the columns stay at the text
width (they turn below 312 pt). Demonstrates 6.7.1–2 (stage 1 addition).

**P1-07 Successful Transactions by Region (outlier inset)** — 02 #9, p. 76
(s. 0076); 08-12 #4, p. 334 (s. 0304); spec in 2.9. East 451, Central 16,
South 15, West 7, North 5, Others 4. Expected: main 0–500 by 100 with "500
successful transactions"; Others last; inset of the five small bars, 0–20
by 5 (the book: 0–15), placed in the empty plot area above them, right of
East; source frame round the small bars at the baseline; two links forming
a widening cone. Demonstrates R2-18, R2-20, R10-4, 6.6. (Stage 1 draws the
main chart, Others in its reserved grey, and reports the inset as not yet
drawn.) Built (inset stage): as expected, a look-up chart; the inset
(Central … Others, Others in its grey) fills the largest empty area right
of East and below the top line, its top at the top gridline and its right
edge flush with the source frame's at the figure's edge, so the right link
is short and the left one slants from the source frame's top-left corner to
the inset's (s. 0076); its numbers bare, the unit being the main scale's.

**P1-08 Donations by Region, 2022 (narrow-range inset)** — 08-12 #2, p.
324–325 (s. 0296–0297). $K: Far West 274, Rockies 272, Midwest 264, N.
England 263, Gr. Lakes 257, Southeast 252, Plains 251, Southwest 232;
`small_differences_matter`. Expected: main 0–300 by 50; inset above, same
bar positions, 230–280 by 10 with "$280K in donations", bars fading
below the 230 gridline; source frame over the main chart's 200–300 band;
links from the inset's lower corners to the band's upper corners.
Demonstrates R2-23, R9-5, R9-6. Variant `truncated` (`scale.min: 210`) is
refused (R9-5, R2-24), and variant `truncated-overridden` draws the book's
bad chart through an override with the findings kept. (Stage 1 draws the
main chart, horizontal at the text width as P1-06 is, so the inset stage
must place a narrow-range inset beside horizontal bars, with the same rows.)
Built (inset stage): at the text width the chart is horizontal and the
inset stands beside it (`position: auto`; the spec's `above` is gone, since
an inset above horizontal bars cannot share their rows), its rows level
with the main chart's, the names repeated at inset size, its bars fading
left of the 230 gridline, its scale titled "Donations ($K)"; the source
frame is the 200–300 band across the rows, its right edge past the "300"
label so that the lower link runs clear of it; the two plots share the width
as their labels need it (124 and 92 pt). The variant `wide` (520 pt, the
book's plot proportion 0.4) keeps the columns and draws the book's figure:
the inset above, the bars at the same x, "$280K in donations", the fade
below 230, the frame round the 200–300 band across the chart's width with
its labels, the links from the band's upper corners to the inset's lower
ones (s. 0297). At 500 pt the main chart, indented for the frames (6.6),
still turns horizontal: its names fit under columns from 480 pt of full
width (P1-06), and the frames take 22 pt of it.

**P1-09 Revenue by Business Unit, Mar–Sep 2021** — 02 #13, p. 83–85
(s. 0080–0081). $M (`unit.magnitude` 6): Consumer 16.69, 16.79, 17.18, 17.31,
17.57, 16.39, 17.76; Business 16.39, 17.68, 16.29, 17.29, 16.55, 17.68,
17.19; prefix "$", what "revenue per month"; `small_differences_matter`.
Expected: main line chart 0–20 by 5 ("$20 million (revenue per month)", D-20),
compact (`inset.compact_main`); inset above on the same months, 16.0–18.0 by
0.5 with one decimal throughout; end labels in both panels. Demonstrates the
inset on lines, `share_axis` off (inset stage: `auto`, which shares it in
the compact layout, D-56), 6.1 truncated mode.
Built (stage 2, lines): the main chart as expected, a look-up chart, both
labels packed 0.3 line heights apart ("Consumer" over "Business"); the inset
is reported as not yet drawn (CG-3), and `compact_main` waits for it.
Built (inset stage): as expected; the main plot half as high (the inline
unit phrase would then meet the lines, so the description takes its own
line, "revenue per month", and "$20 million" stays on the scale, 6.4), the
inset above at the main chart's months with "$18.0 million (revenue per
month)", as high as the compact main plot (`--inset-max-height`); the
compact layout shares one time axis, the main chart's, under the stack
(D-56), row 2 ("2021") left out as the title gives it; the source frame
round the 15–20 band across the chart's width, the unit line and end
labels inside it. The book's main chart has no end labels and no months
(the inset's serve both, s. 0081); here the main chart keeps its labels,
so it stays complete (G2.27), and the inset drops its months.
Variant `truncated` (`scale.zero: truncate`, no inset) is the book's 16.0–18.0
version (the summary writes "16–18 by 0.5"), drawn with the guided finding
R2-22 that the procedure keeps zero and adds the inset.

**P1-10 Transactions per Month, 2022 (gaps)** — 06 T10, p. 205–211
(s. 0191–0195). Jan 669, Feb 620, Mar 669, Apr 609, May 589, Jun 651, Jul
789, Aug 843, Sep 971, Oct `null`, Nov 931, Dec 949; noun "successful
transactions per month". Expected: 0–1,200 by 200 (grouped "1,000"), a gap
at October with no markers. Variant `isolated` (Aug and Oct `null`): Sep
isolated, markers on every point. There is no variant for the book's
joined line or its October at zero: a `null` cannot be joined across or
become 0 anywhere in `chartgen`; writing 0 in the data states a known zero,
which the generator then draws as such. Demonstrates R6-22, R6-23, R6-25,
R6-27.
Built (stage 2, lines): as expected (a single line, `single`, no label);
two paths, Jan–Sep and Nov–Dec; `isolated` has a point on each of its ten
values. Added variants: `sparse` (five months known, never three in a
row, the type left to the chooser) is drawn as a dot plot over the whole
year; `sparse-line` (the same with `type: line`) is refused (R6-2).

**P1-11 Line formatting cheat sheet, Jan–Jul 2022** — 06 T12, p. 218
(s. 0201); spec in 2.9. Expected: 0–140 by 20, "140 employees"; July dotted
from June in each series' colour; Average in #595959 with a bold label;
Central highlighted; end labels at the projection ends, packed without
overlap and in order (West 38, South 30, Int'l 20 are the close ones).
Demonstrates R6-24, R6-32, R6-33, R6-34, 6.5.
Built (stage 2, lines): as expected, with an insight corrected against the
data: Central (96) already passed East (90) in June, so the title reads
"Central Passed East in June and Is Projected to Pull Ahead" over
"Headcount by Region, Jan–Jul 2022 (July projected)". Colours by the line
ends (6.5.6): Central slot 1 in the strong blue, East orange (the book has
them the other way round); West, South and Int'l move 0.03–0.10 line
heights. The highlighted projection's dots are spaced by
`--projection-dash-highlight`.

**P1-12 Headcount by Region, Jan–Jul 2019 (emphasis)** — 06 T11, p. 212–214
(s. 0196–0198). East 100, 110, 109, 88, 107, 96, 100; Central 90, 96, 106,
82, 78, 78, 90; North 60, 47, 62, 53, 55, 47, 62; West 40, 32, 38, 42, 42,
47, 45; South 30, 31, 31, 29, 33, 32, 30; Int'l 20, 18, 19, 20, 18, 17, 20;
Central highlighted. Expected: 0–120 by 20, "120 employees"; Central twice
as thick in its strong colour, drawn last, bold label; every other line
keeps its colour. Variant `greyed` (`highlight.method: "grey_others"` with
an override) draws the rejected grey-out and must report R6-28 (North and
West meet in June). Demonstrates R6-28, R6-29, R15-6.
Built (stage 2, lines): made the job of making the reader aware, with the
insight title "Central's Headcount Fell by a Quarter from March to May"
(106 to 78) over "Headcount by Region, Jan–Jul 2019"; as expected, the six
lines in slots 1–6 by the line ends (the lint warns that slot 6, purple,
spreads the lightness to 0.155, R3-6: the palette's matter). `greyed` draws
the others in #BFBFBF and Central in its own colour and width, as on s.
0197, with R6-28 overridden. Added variant `vertical` (`orientation:
horizontal`): refused (R6-38, R2-37; 06 T15).

**P1-13 Daily Expenses, May 2022 (dot plot)** — 06 T1, p. 172–174
(s. 0161–0163); 00-01 F6, p. 20. Day grid 2022-05-01…31 with values on day 3
325, day 8 424, day 12 197, day 22 704, day 23 368, day 29 499 ($K), all
other days `null`; `type: auto`. Expected: the chooser gives a dot plot
(6 of 31 known, R6-2); 0–800,000 by 200,000 with "$800K expenses per
day" (D-3, D-4); day numbers thinned from day 1 of the group, "May 2022"
below (unless the title gives it, G2.37).
Variant `line` (`type: line`) is refused (R6-2).
Built (stage 2): as expected, a look-up chart; filled dots `--dot-size`
across at their slot centres, no notes on the 25 empty days, every second
day number (1, 3 … 31), and "May 2022" left out, the title giving it.

**P1-14 Credit Scores** — 02 #15, p. 89–90 (s. 0085). Jasmine 467, Pablo
445, Elena 361, Cynthia 314, Andy 307; score, min 300, max 850, zero
arbitrary; `type: dot`. Expected: 300–500 by 50 (the theoretical minimum is
the lower limit, R2-26); the dot plot passes the non-zero lint because dots
do not encode length (R2-24); hollow dots, by the plain style's rule for
categorical dot plots (as on s. 0085). The same spec with `type: bar`
(variant `bars`) draws bars from 0, 0–500 by 100 (the theoretical minimum
never truncates bars), with a guided finding (R2-24, G1.57): the procedure
gives dots, and bars from zero show scores no one can have.
Built (stage 2): as expected, vertical (the names fit under the dots), the
title "Credit Scores, Dec. 31, 2022" and the unit as `what`, so "500 (credit
score)" on the top label, both as the book has them.

**P1-19 Successful Transactions per Day, Feb.–Mar. 2022 (sparse, inset)**
— 02 #14, p. 85 (s. 0082) (stage 2 addition). Day grid 2022-02-23…03-09
(the book gives no year; 2022 is ours) with Feb 23 137.8, Feb 27 140.9, Feb
28 138.4, Mar 3 137.8, Mar 5 138.1, Mar 8 141.4, the other days `null`;
count, noun "successful transactions per day"; `small_differences_matter`,
`inset.kind: narrow_range` below; `type: auto`. Expected: a dot plot (6 of
15 known, R6-2); the main scale keeps zero, the values being counts with
nothing declaring the distance to zero irrelevant (R2-22, D-10 (a)): 0–150
by 50 with "150 successful transactions per day" (the book: 0–160 by 20; 50
scores better on five intervals and leaves 6 % room, a known deviation of 02
D5); row 2 "Feb. 2022", "Mar. 2022" with dividers; the inset (136–142 by 1
in the book; on dots it uses the TRUNCATED mode of 6.1, and dots are not
faded, 6.6) is reported as not yet drawn (CG-3). Demonstrates a narrow range
over time on a sparse series, for the inset stage. Built (inset stage): the
spec adds `compact_main`, as the book's main chart is low (s. 0082); the
inset below, 137–142 by 1 (the book: 136–142), as high as the compact main
plot (`--inset-max-height`; it was twice as high), the dots at the same
slots, not faded, the numbers bare; the compact layout shares one time axis
(D-56), the main chart's days and months with their dividers, between the
panels (the book, s. 0082 right, repeats them in both); the source frame
round the 100–150 band across the chart's width, the unit line included
(inline it would meet the dots), the links from its lower corners down the
figure's edges to the inset's upper corners, clear of the main chart's
labels (the book's links cross them).

**P1-20 Client Satisfaction Ratings, 2020 (truncated dots)** — 02 #14, p. 85
(s. 0082) (stage 2 addition). Client A 9.0, B 8.3, C 8.1, D 7.4; score,
zero arbitrary, max 10, noun "on 10"; `type: auto`, `orientation:
horizontal` with its reason (the book's layout). Expected: the chooser
gives dots (an arbitrary zero, G2.10, G1.57); the scale truncated, 7.0–9.5
by 0.5 (the book: 7–10 by 1, which also places its level labels "Neutral"
and "Very satisfied" at the ends, G2.10; the level labels are not drawn);
bare labels with one decimal and the title "Satisfaction rating (on 10)"
below; open circles; the orientation a guided info (the names would fit
under the dots). Variants: `highlight` (a make-aware job with the insight
title "Client D Is the Least Satisfied": Client D's ring in the strong
shade, the others unchanged, R3-7, D-9); `bigger-dot` (`method:
thickness`) is refused (R15-6).

**P1-15 Revenue by Region, 2022 (highlight)** — 13-15 F3, p. 376
(s. 0342). $K: Central 1,515, West 996, North 612, East 498, Int'l 189;
insight title "Central Brought in the Most Revenue" (ours) with the
descriptive line "Revenue by Region, 2022"; `insight.targets` Central.
Expected: 0–1,600 by 200 (0.0 … 1.4, "$1.6 million in revenue": the larger
unit with one decimal, D-4 (iii)); the step of 500 would leave 32 % room
above the data, so 200 is taken (6.1, owner); Central in the strong shade of
its hue, the others unchanged (R3-7, D-9); the title block left-aligned
(D-5). Variants: `dim-others` (the others paled, a guided
finding with the spec's reason); `bigger-bar` (`method: thickness`) is
refused (R15-6). The book's arrow from the title is phase 2 (stage 1
addition).

**P1-16 Glycemic Index of Fruits** — 02 #1, p. 54–55 (s. 0053–0054). Apple
38, Banana 61, Grapefruit 24, Grapes 59, Melon 72, Orange 39. Expected:
sorted Melon, Banana, Grapes, Orange, Apple, Grapefruit; 0–80 by 20 (the
book's scale); no axis title. Variant `alphabetical` (`sort: given`, the
axis title "Type of fruit") is the book's bad version, drawn with a guided
finding (R2-01). (Stage 1 addition.)

**P1-17 Profit by Region, 2022** — 05 #10, p. 165–167 (s. 0154–0156), a
thumbnail read approximately. $K: East 34, South 9, North 8, West −16,
Total 35 (`x.roles` Total: total). Expected: −20–40 by 10 (R2-30: room at
both ends), "−20" with a true minus, "$40K in profit"; West hanging below a
darker zero line (D-17); Total last in its reserved grey (R3-16, G7.3).
(Stage 1 addition.)

**P1-21 Daily Donations by Region, Oct. 16 to Nov. 9, 2022** — 02 #18,
p. 105–106 (s. 0099–0100) (stage 2, lines, addition). Four regions over 25
days ($K, `unit.magnitude` 3, noun "total donations per day"). Expected and
built: 0–140 by 20 with "$140K total donations per day"; row 1 every second
day, restarting at the first of November (16 18 … 30 | 1 3 … 9: at 368 pt a
day's slot is 12 pt, under a label and its gap), row 2 "Oct. 2022" and
"Nov. 2022" with dividers at both ends and the boundary; Central and North
packed. Variant `narrow` (260 pt): every third day (16 19 … 28 | 1 4 7), the
title on two lines. (Time, 6.3.8: "16" clears the left divider, the slots
starting 4.2 pt in from it (2.0 pt at the full width); in `narrow` November's
"1", closer than 0.3 em to the divider between the months, keeps its slot with
a knock-out halo.) Demonstrates R2-39, R2-40, G2.37, G2.38 (the canonical
time-axis test of 02 §7).

**P1-22 Headcount by Region, Jan–Jul 2022 (end labels and the legend)** —
03-04 #16, p. 144 (s. 0134); #18, p. 146 (s. 0136) (stage 2, lines,
addition). Base: #16's five regions, labelled at their ends, none moved.
Variants: `dynamic` (#18's data, where East 91 and Central 90, North 60 and
West 58 end together and the book's software overprints them) packs each
pair 0.4 line heights apart; `converging` (synthetic: five regions ending at
62, 61, 60, 59, 58) would move South's label 1.8 line heights, so it takes a
legend right of the plot in the order of the line ends (R4-2, R4-5, R4-7;
the lint warns that slots 3 and 5 are close under protanopia, R3-19, once a
legend makes readers match colours); `legend` (`chart.labelling: legend`
where the labels fit) is drawn with the guided finding R4-1. Demonstrates
G5.1, G5.3, G5.7, G5.9, G5.11, 6.5.

**P1-23 Mean Monthly Employee Satisfaction, Apr 2021–Mar 2022** — 06 T2 (a),
p. 175 (s. 0164) (stage 2, lines, addition). 7.2, 7.0, 6.8, 7.1, 7.3, 7.5,
7.3, 7.4, 7.7, 7.2, 7.5, 7.4; score, zero arbitrary (a rating's anchor),
0–10, noun "on 10". Built: a line may leave zero out when it means nothing
(G2.8, G2.10): 6.6–7.8 by 0.2 with "7.8 on 10" (the book 6.0–8.0 by 0.5,
the known kind of deviation of 6.1); twelve months with "2021" and "2022"
on row 2 and a divider at the new year; one line, no label.

**P1-24 Students with Learning Disabilities by School, 2012–2021** — 06 T2
(b), p. 175 (s. 0164) (stage 2, lines, addition). Built: a year axis, every
year shown, no row 2; three schools labelled at their ends; 0–100 by 20,
"100 students".

**P1-25 Monthly Sales, Jan–Jun 2022** — 06 T18, p. 230 (s. 0212) (stage 2,
lines, addition). $M 1.87, 1.73, 2.30, 1.81, 2.07, 2.50. Built: one line in
the single-line colour with no label or legend (G5.6); 0.0–3.0 by 0.5 with
"$3.0 million (total sales per month)" (the book's "$3.0M (Total Sales per
Month)"); "2022" left out of the axis, the title gives it. Variant
`incomplete-july` (July's 0.62 with `data.as_of` 2022-07-10): July is left
out (R2-42, G2.40).

**P1-26 Price of the XLCD65 TV, Apr–Jun 2022** — 06 T6, p. 188–190
(s. 0176–0178) (stage 2, lines, addition). Thirteen price changes as
`points` on an irregular axis, persistence `state`. Base: the chooser gives
a step chart, phase 2 (unsupported). Variants: `line` is refused (R6-3; the
departure from the step chart a guided finding, R6-9); `no-persistence` is
refused (R6-1). Built (time): the step chart (6.13.2) from April 1 to the
end of the as-of day, June 30, the last price, $250, one day wide at the
right edge; months centred on their spans with a tick at each day, the
year left out (the title gives it); 200–450 by 50, "$450 (price of the TV)",
the spec declaring the distance to zero irrelevant for looking a price up,
as the book's figure truncates (s. 0177). New variants: `no-as-of` is
refused (R6-8: where the last price ends cannot be read), `from-zero` keeps
zero (0–500 by 100).

**P1-27 Sales Transactions, Jun 20 to Jul 16, 2022** — 06 T7, p. 194–196
(s. 0181–0183) (stage 2, lines, addition). Eighteen sales as `points`,
persistence `event`, message `individual_values`. Base: thin bars, phase 2
(unsupported). Variants: `pattern` (the overall pattern is the message) is
refused under R6-3, asking for the totals per week (R6-10); `line` is
refused (R6-3, with the guided finding R6-10). Built (time): eighteen thin
bars (6.13.3), each in the middle of its day, 2.8 pt wide; the two sales of
July 3 and of July 5 stacked with a hairline gap (D-42), so the scale runs to
6,000, "$6,000 (amount of each sale)"; every second day, restarting at July
1, "June 2022" and "July 2022" under them with dividers.

**P2-20 Concert Attendance, Feb–May 2022** — 06 T8, p. 196 (s. 0183)
(time). Six concerts as events over the window February 1 to May 31
(`x.values`), a look-up chart. Built: thin bars, the concerts of March 26
and 27 touching, as in the book; months on row 1, "2,500 attendees".
Variants: `totals` (the pattern is the message) refused under R6-3 (the
totals are a change of data, R6-10); `line` refused (R6-3; R6-12 guided).

**P2-21 Donations Before and After the Ad Blitz** — 06 T9, p. 200–204
(s. 0186–0190) (time). Five regions, $K before and after (the quarters are
ours: the book names the periods only before and after the blitz, which
`x.names` gives the key); an answer, "North Rose Most After the Ad Blitz;
South Gained the Most Dollars". Built: the arrow chart (6.13.4), rows East,
Central, South, North, West (the value before, D-40), 0–600 by 100 in $K
beside −20–60 by 20 in %, the key over South's arrow. Variants: `clustered`
refused (R6-17; R6-15 guided); `absolute-only` refused (R6-17);
`absolute-only-overridden` drawn, the book's dollar panel alone (s. 0187);
`by-change` rows by the change in %, North first; `no-good-direction` a
look-up chart without `measure.good`, rises and falls in slots 1 and 2.

**P2-22 Headcount by Region in Black and White, Jan–Jul 2019** — 06 T11,
p. 214–216 (s. 0198–0200) (time). P1-12's data with `audience.bw_risk`, a
look-up chart. Built: six symbols, one per line in the order of the line
ends (6.13.7). Variants: `emphasis`, P1-12's make-aware chart with Central
thicker and its symbol kept; `legend`, the legend's keys with their symbols
(R4-1 guided).

**P2-23 Total Monthly Expenses by Region, 2021** — 06 T3, p. 177 (s. 0166)
(time). West and East, $K; East's quarterly cycle as an insight title.
Built: two lines labelled at their ends. Variant `bars`: clustered bars of
the months, a guided departure (R6-4), drawn by phase 2's clustered bars.

**P2-24 Monthly Revenue, 2022 (incomplete December)** — 02 #19, p. 107–108
(s. 0101–0102) (time). €M, the good figure's values (D-54: the bad figure's
differ), the data to December 10, `facts.incomplete_value_needed`. Built
(6.13.5): the line ends at November, December's 0.78 a dot with "As of
Dec. 10". Variants: `bars`, the dotted level mark with its note to the
right; `left-out`, the default without the fact; `joined`, the book's bad
version, possible only when the spec withholds the as-of date.

**P2-25 Daily Rank by Medals Won, Winter Olympics, 2018** — 06 T5, p. 181–187,
226 (s. 0170, 0209) (time). Ten countries' ranks over the book's days 1–16,
read as the competition days of the 2018 Games (February 10–25); a
make-aware chart naming the Netherlands. Built: the bumps chart (6.13.6),
the Netherlands highlighted; ten colours, labelled directly (R3-22 warns).
Variants: `lookup`, the book's plain version; `line`, the ranks as a
standard line, a guided departure (R6-7) in which the Netherlands' line
rises as it falls behind.

**P2-26 ATM Withdrawals, Aug. 14, 2022** — 06 §5.3, after p. 194 (s. 0181)
(time; synthetic data). Thirty-four withdrawals at times of one day. Built:
hours at their instants every third, a tick at each hour, the day left out
(the title gives it); two withdrawals at 6:45 AM stacked.

**P1-28 Employees by Level of Seniority in Four Companies** — 06 T17 (a),
p. 226–228 (s. 0209–0210) (stage 2, lines, addition). Ordinal levels
Temps/Interns … Executive, four companies, percent; `audience.data_savvy`,
`type: line` with its reason. Built: the line allowed, a guided info against
the procedure's bars; 0–60 % by 10, "60% of employees in company"; Company
B, C and D packed. Variant `general-audience` is refused (R6-41).

**P1-29 Annual Expenses by Department, 2022** — 06 T17 (b), p. 228–229
(s. 0210–0211) (stage 2, lines, addition). Marketing 14.4, Operations 13.9,
Finance 7.5, R&D 5.7, I.T. 5.5 ($M). Base: the chooser gives bars (0–16 by
2, "$16 million (annual expenses)", the book's own scale). Variant `line` is
refused (R6-42).

### 8.3 Unit tests and fixtures (`tests/`)

Stage 1 has `test_scales.py` (the 02 D5 table, the known deviation, 02 #17,
the P1 limits, the lower-limit modes), `test_fmt.py` (the formatting and
date cases below, under D-3 and D-4), `test_typst.py` (each escaped string
typesets exactly as the same text given to Typst as a string literal),
`test_spec.py` (paths of errors, values, merge patch, structure, the
schema, and that no spec field sets a style value), `test_style.py`
(flattening, bake's lookup, the style lint) and `test_render.py` (every
example meets its expectation; the SVG rules of 3.6; levels on every
finding and decision). Stage 2 adds `test_dots.py`: the chooser's dot
branches (06 DP6-2 on the book's three cases, 6 of 31, 11 of 12, 10 of 12
with an isolated value, and half known without a run of three; categories
with a theoretical minimum, an arbitrary zero or a declared irrelevance
against those that keep bars), the lower-limit modes and scales of dots
(P1-13, P1-14, P1-19 and P1-20 with their known deviations), both layouts
(each dot at its value and under its name, one size, the time axis over
the whole period, no notes on empty days, a note for a categorical value
not known, no "(zero)" note for a dot at zero), the highlight and the
refused bigger dot, a pinned lower limit as a guided finding, several
series as a later phase, and the dot lint. Stage 2 (lines) adds `test_lines.py`: the
time axis (02 #18 thinned restarting at November, a collision across a
group boundary, months full then narrow then thinned and refused past every
seventh, months over two years, a row 2 dropped where its group is too
narrow or left out where the title gives it, years without a row 2, period
ends and incomplete periods, period names); packing (labels that stay, a
pair spread about its mean, order kept, the band's edges, D-7's three
outcomes, and the book's charts below); missing values (runs, a projection
from the last actual value, crossings, a thick segment meeting a box); the
chooser (regular, two-period, rank and irregular series, the refusals of a
line, `purpose: analysis`, persistence by default); and the line examples
as drawn (no path spans a value not known, the gap and markers of P1-10,
projections, end labels in the order of the line ends one pitch apart,
reference and highlighted series, the greyed variant, the time axis, one
line without a label, the legend, the scales). Stage 2 (checks) adds
`test_svglint.py`: the hand-written fixtures in `tests/fixtures/`, each
failing on exactly the rules its notes give: 00-01 F1 heavy (R1-02, R1-03,
R1-04, R1-05; the same chart as chartgen draws it passes) and over-pruned
(R1-06 only); 03-04 #1 saturated (R3-2, R3-6, and R1-02 for the yellow line
lighter than the gridlines) and muted (the book's good trio just past D-34's
numbers: chroma 0.152, spread 0.144, and blue against purple 0.032 apart
under deuteranopia, warnings and an info); 03-04 #3, a line shaded by value
(R3-4, R1-02); 05 #1 in five text styles (R5-6, R5-7, R5-8, R5-27) and in
two (no finding); 05 #8, the "fugly" chart (R1-02, R1-03, R2-05, R2-07,
R2-08, R2-34, R3-2, R3-18). Every chart chartgen draws passes, measured
afresh from Typst, except what its spec overrides; each check catches its
fault put into a copy of a generated chart; the CVD matrices reproduce the
calibration of 03-04 §5.7. `test_cli.py` runs `choose`, `lint`, `render`,
`style` and `schema`; `test_chartspec_jl.py` (skipped without Julia) checks
the writer's round trip, its refusals, and the example generator's spec.
Phase 2 (series) adds `test_clustered.py`: clustered bars (touching bars,
the clusters' gap by the number of series, the order and colours in every
cluster, the legend row under the title in the order of the bars and
centred over the plot, the catch-all's grey, the regions sorted by the
first series, direct labels on leaders as a guided departure, turned
clusters named beside the last and a legend column when asked, the bars an
insight names, several totals and their routing, ten colours refused, a
value not known in a cluster); flat markers (as wide as the bar, their
thickness, their value on the one scale, drawn after the bars, named beside
the first bar, else the last, no place in a cluster, the line variant, a
reference that never sorts); value labels (the procedure's count, no scale
or gridlines but the zero line, outside the muted bars, `off` a guided
finding, the pinned phase-1 examples, a loss below its bar, horizontal
labels, labels that would meet keeping the scale, the book's labels on
every bar colliding, a chart under hooks keeping its scale, the lint's info
left to bars); tables (under clusters, beside turned bars, none when the
values are labelled, none when cells would meet); dots of several series
(one place per category, named beside the last, a legend asked for, turned,
over time, one note per category); the legend component (a row wrapping
into a grid, keys in the form of their marks, `column` reproducing a line
chart's legend, side labels); and the chooser's decisions. `test_dots.py`'s
several series are now drawn.
Phase 2 (time) adds `test_time_charts.py`: the true time axis (instants,
months over three months with a tick each day and the year left to the
title, days every second restarting in July with June 30 dropped, hours at
their instants, years; points checked; incomplete edges and the note); the
chooser (two periods as bars refused, several irregular series waiting);
the step chart (level and vertical segments only, the last value to the
plot's edge, no markers, the ticks, the refusal without an as-of date, the
scale from zero, a gap for a state not known); thin bars (their width, the
stacked events and the scale holding them, touching concerts, hours); the
arrow chart (rows by the value before or by the change, each head's tip on
its value read against the gridlines, the % arrows from 0, the key, the
panels refused and overridden, the % panel dropped where the change is
undefined, the colours of rises and falls); the bumps chart (flat-tangent
paths, no gridlines, the rank labels and "Rank", the end labels in final
rank order, the highlight, ranks from quantities with ties); incomplete
periods (the line ending at November, the dot and its note, the period
left out, the level mark on bars as wide as a bar); black-and-white symbols
(one symbol a line, six different, at every point, in the legend's keys);
the legend entries' series; the line marks' faults (a marker taken away, a
line across a gap); and each new SVG check's fault put into a generated
chart (a curve in a line, a bumps curve not flat, a dash identifying a
series, two lines of one colour, the legend out of order, a dot taken away,
a tick off a true time axis, a thin bar off zero).

- **Scales**: the 02 D5 table (6.1) and its known deviations; P1 limits
  above; 02 #17 expenses by region ($M 18.1, 13.6, 12.2, 10.6, 6.3): 0–20 by
  5, and by 2 with `precision_value: 12`; 02 #4 unequal intervals and 02 #12
  truncated bars as refusals.
- **Formatting**: $582,793.32 → "$583K" and $195,818.56 → "$196K" as
  data labels (not 582, 195: R5-16); 0.00–0.25; "−$20K"; "0%", "50%",
  "100%"; "+20%", "0%", "−20%"; 5,477,210 grouped; the date forms of 05 #6
  (the four ambiguous forms are never produced).
- **Typst escaping**: "$300,000", "Int'l", "R&D", "Finance & Admin",
  "−20%", a leading "-", "C++" survive baking unchanged.
- **Chooser**: 06 DP6-2 cases (6/31 → dot plot; 11/12 → line with a gap;
  10/12 with an isolated value → line with markers); 06 T7 irregular events
  → refusal asking to aggregate; 06 T15 vertical line and T16 smoothing →
  refused; 06 T17(b) line over departments → refused, T17(a) over seniority
  levels → allowed only with `data_savvy`; 07 #6 pie of averages → refused
  (R7-02). (Stage 2, lines: T16 smoothing cannot be asked for, since no
  spec field smooths a line; the pie waits for phase 3.)
- **Packing**: 06 T12 (pass), 03-04 #13 (twelve lines; N.W.T. and Yukon
  touch but do not overlap), 03-04 #16 (pass) against #18 (fail → legend).
  (Stage 2, lines: packed, #18 passes, each pair moving under half a line;
  its overprinting in the book is the software's, which had no collision
  test (G5.7). The failing case is P1-22's synthetic `converging`.)
- **Inset geometry**: links from the hull on the P1-07 and P1-08 layouts;
  one frame inside the other → error. (Inset stage, `test_insets.py`: the
  hull's two links on a pair of frames and none when one holds the other;
  the four insets as drawn, P1-07 inside the plot right of East and above
  the small bars, the members only, from zero, no fade; P1-08 beside its
  horizontal bars on the same rows, its bars under the fade mask, and at
  520 pt above its columns at the same x, wider than the source frame, the
  links from the band's upper corners to the inset's lower corners; P1-09's
  lines at the main chart's months, the main plot compact, end labels in
  both panels; P1-19's dots once in each panel at the same slots; the form
  (links, source, frame, panel, in that order; `inset-text` on every text;
  no title); the scales and the unit restated only where needed; the
  decision's prompts, the guided finding for `kind: none` against the
  facts, `auto` by default, an outlier inset where nothing dwarfs the rest,
  none on a truncated main chart; each fault caught in a copy of a generated
  chart: a missing fade, a fade on an inset from zero, a missing link, the
  links' order, a frame over the main chart, inset text at label size, a
  bare inset scale under a unit in $K; the inset style scaling the text's
  lengths and not the marks; the book's s. 0077 line inset placed below and
  clipped. After the owner's review: P1-09's "$20 million" kept on its scale
  under fallback A, the description alone on the unit line; the shared axis
  (P1-09 and P1-19 share it, their months or days in the main chart only;
  P1-08 `wide` and beside its bars repeat the names), `share_axis: false`
  against the procedure a guided finding, `true` on an unaligned inset an
  info; every inset above or below at most as tall as its main plot.)
- **Style lint**: the muted palette passes; the Office palette gives the
  known warnings; 03-04 #1's saturated trio (#00ADEF, #ED008C, #FEF200) and
  the unbalanced five (#9595C9, #9A3466, #FBF6B6, #D2EDF4, #642166) fail
  R3-2 and R3-6; the CVD calibration of 03-04 §5.7 (green–red map endpoints
  0.051 apart under deuteranopia fail; blue–orange ≥ 0.147 pass); New
  Computer Modern passes 05 DP-1; a stylesheet with five text styles fails
  R5-6 (05 #1).
- **The chooser** (phase 3, `test_chooser.py`): a table of cases, one per
  branch of G1.10's steps 1–7, each with its type, rule and group (D-13
  (a)); the path's form (every step in order from 0 to 9, a rule and a level
  each, the group entered once); the four checks, each routing a pie request
  away under `auto` and refusing it when asked for (locked, overridable),
  the three facts they need, overlap among them, refused when missing
  (D-1; owner, 1 Oct 2026), the checks for every total (D-25 (a)), seven parts flagged
  (D-23 (a)); the avoid-list, each request refused with its replacement and
  drawn through an override only where a layout exists; the slope chart in
  its case and refused outside it; a line across ordered categories for the
  data-savvy only (08-12 DP-2, R6-41); an arrow chart asked for over other
  than two periods refused (R6-15); a total panel's lines refused as a line
  is (R6-2); maps reported as phase 4; teaching an
  unfamiliar type; the week's first day by locale, ISO weeks only on
  Mondays, weeks following one another, 06 T7's totals recomputed from
  P1-27's sales in Sunday and Monday weeks and checked against P3-71; bins
  re-aggregated, refused, and kept through an override; and every example
  of phases 1 and 2 chosen as before (`fixtures/chosen-types.json`), every
  spec in `examples/specs` going through the chooser, P3-60–P3-79 getting
  their types.
- **Tables, heatmaps and ramps** (phase 3, `test_tables.py`): the ramps' nice domains
  (sequential from zero on a finer step; diverging symmetric about the dividing value, its
  ticks d ± k steps), their steps and sides (good and bad by the good direction; `labelled`),
  the key cut at a theoretical maximum, ordered steps (3–9, at most seven) and grouped shades
  (D-49), the palette's decision and its two refusals (R3-13, R3-9); the style's ramps darken
  step by step, step 4 the single blue, the text grey at 4.5 : 1 on every `labelled` step of
  every ramp, in the default file and under `cvd-safe`, and on `--cell-bar`, the sides green and
  red and under `cvd-safe` blue and orange; the ramp
  key a graduated bar with its title, ends, dividing value and tick, and its missing entry; the
  grouped legend's rules and names; P3-40's formatting (right-aligned bare numbers, every
  digit, fixed-width dates, the two rules, three fills, nothing bold, the Total name
  right-aligned), its stripes refused and drawn through the override, bold drawn through its
  override; P3-42's values left-aligned with their units and no header rule; the unit missing
  from a title warned (R10-25); a table too wide refused with its width; the French
  separators; P3-47's heat groups (the grand total unshaded, each group's extremes at the
  ramp's ends; its last column's cells inside the table and its rules); P3-48's sparklines;
  P3-49's bars on one scale per column, the Total column's too, none in the totals row, and its
  dots; P3-44's export table (chosen at step 1, its codes left-aligned, a zero printed, a count
  not made as its note); P3-50's labelled cells, red and green in one file (R3-19 on the pair
  an info), the
  given column order and the `sorted` one, good and bad by the dividing value, the summary row
  apart, no key, the cells running under their neighbours; P3-51's 2,550 cells, missing never
  zero, round years, its zero and five logarithmic steps rounded as a set in an equal-swatch
  key; the cut (G4.29–G4.34): the skew share, the span (one stray value not counting), natural
  breaks, the measles' pattern (logarithmic over four decades; natural breaks asked for, and
  automatic for skew within two decades) and extremes, the info
  when the focus is not given, quantiles saying so, logarithmic steps (zero its own) and their
  refusal for a score, custom breaks warned without a reason, even data linear, the classes of
  values and the key's entries, the lightest step's check passing and failing (the old
  `--seq-1`); the cells strong with black or white values (4.5 : 1), muted for a shaded table
  and asked for in P3-50 `muted`, plates, an unknown `--cell-values` warned; P3-52's neutral
  step at 7 under the key's tick, the key's bar ending at the grid's edge, and the sequential
  variant; P3-53's
  diverging ramp without sides (no good or bad, no copy, the tick in the key's middle), its
  sequential departure and its refusal; headings that cannot fit and values asked for where
  they cannot fit refused (CG-1); a rainbow and a traffic-light ramp in a style failing the
  style lint (R3-9, R3-14, R3-15); P3-54's order, statuses and bar lengths; P3-55's changes and
  their places between the stages; the new spec fields' structural checks.
- **Stacks** (phase 3, `test_stacks.py`; 6.18): several totals end to end from zero, the gap
  between segments, one order and colour of the parts in every total, the totals sorted and
  labelled when they are the message, the parts named beside the last stack or by a legend
  column or row, a stack with a part not known left out with its note; one total upright, a
  featured subtotal at the base in strong shades, a subtotal stated as a fraction in a 100 %
  stack (P3-36), values before the names, leaders to thin parts' names packed off their
  segments (P3-35), groups in tints, All others on top; 100 % stacks adding to 100, ordered
  parts in steps of one hue, the named part at the base of every period (P3-33 `west-first`),
  parts of several totals in groups in shades of a hue each with the grouped legend (P3-39);
  a fixed order kept (P3-30 and P3-39 `fixed`), and the chooser's sort given for plain bars
  of a breakdown with `data.parts.order` or any bars with `data.x.order` fixed, the table
  under them following; the totals formatted with the scale, bare
  (P3-39's 320); a gridline stopping short of a total, its halo, and CG-7 finding one
  missing; an empty slot's note in each of its three steps (wrapped in the slot, P3-38; under
  its label, a narrow chart; above with a leader, an x-axis title beneath), R9-7 satisfied;
  waterfalls
  from the running total, signed labels kept off the zero line (P3-37), good and bad through
  the message layer (one file; textured in black and white), a contradicting total warned, a step not known refused, upright only,
  totals at declared places, a running total below zero; stacked areas touching through the
  slots, broken round a period not known with the note above the bands (P3-38), a part of zero
  pinching its band; lines with a total panel; one colour per region across 07 #14's charts
  (R3-24); the model and SVG lints of stacks; every example P3-20–P3-39 as its spec expects,
  and the chooser's types for them.
- **Notes** (`test_notes.py`; 6.19): the spec's form (each place names one thing the spec has, a
  known value's note, the schema); the marks' sequences, a value's own notes merged, the room kept,
  the lines that may end a label; wrapping (never before "&", nor after an abbreviation before its
  number; a word wider than the chart alone); the B12 table (eval round 1) with every POA marked
  and its marks hanging, the figures aligned, the notes below everything (R1-01, its plain-note and
  value-notes variants, and opus-G's spec as delivered); rows named alike, one note in two places,
  a figure's mark (R1-02); reading order and right-aligned names (R1-03); a wrapped name's last
  line, a value label (R1-04); an end label's room and a comment (R1-05); a slice's name and value
  (R1-06); a tile's mark in its colour after the treemap's own note (R1-07); small multiples
  (R1-08); a place not drawn (warn), a gap's note (info), a word too wide (error, the figure widened)
  (R1-09); symbols from a style; the SVG lint's CG-8 on a mark without its note and a footnote
  without its mark; the style lint's floor and sequence checks.
- **References and annotations** (`test_references.py`; 6.12.1, 6.12.2, 6.12.6, 6.13.3, 6.13.12;
  evaluation round 1, the examples R1-20 … R1-31): two references across each bar, solid and
  dotted, each in its own group and named once with leaders (R1-22); a third not drawn and two
  marks too near, warned (R1-21); two lines labelled apart, the target first, in a column beyond
  the plot where they do not fit along it (R1-23); a deviation taking its threshold along as one
  +5 % line, units converted by a factor, refused without one, a reference ten times past the data
  warned, a magnitude alone converted (R1-20, R1-24); a line in the main chart and an inset, named
  once, a value outside the inset's range in the main chart only, a line cut clear of a region
  inset's frames (R1-25, R1-26); comments on a merged chart and on small multiples, their arrows
  ending at their points in their panels, a panel named by its measure (R1-27, R1-28); a thin bar's
  note above with a leader, or on the baseline where it fits, a marked date beneath the bars, one
  outside the axis warned, a comment on one event and on a step (R1-30, R1-31); events named by
  their time or day, two on one day refused, dates on a time axis only.
- **Fitting and colours across a set** (`test_fitting.py`; 2.10, 6.6, 6.7.3, 6.10, 6.13.4,
  6.13.12; evaluation round 1, the examples R1-60 … R1-66): `rows_apart` on one- and two-line
  names; B01's names at 368 pt, every neighbouring pair at least 1 em apart ink to ink, the column
  widened and the rows opened (R1-61), the usual rows where every name fits (R1-61 `wide`), rows
  opened further for two wrapped neighbours, and in merged bars and an arrow chart; B06's inset
  on its members' rows, level with them and naming none (R1-63), in the plot's empty area at
  480 pt (`wide`), not drawn with its warning when the members are not the last rows
  (`ascending`); each short-arrow form (`fit`, `overhang`, `head-only`, `hidden`), the tip on the
  end value, the head over its dot pointing its way, and the token read from the style (R1-60);
  B11's set keeping one colour per centre from the registry (R1-64 … R1-66), a clash
  warned once (`clash`), series slots kept in small multiples, a name map on a single chart with
  its clash and a series' own slot, the registry file read, missing or malformed, slots 1–16
  checked, a pie's part by name; the opening's line in every panel at March's start, labelled
  once (R1-64, R1-65), on a single line chart (R1-66), outside the axis warned, and on panels of columns
  reported (CG-3).

### 8.4 The examples document

`examples/charts-examples.typ`, built by `examples/build.sh [--pack]
[style]` like the diameters document: the same page geometry (A4, 4 cm side
margins, 368 pt text width, New Computer Modern 11 pt), so charts are seen
at print size.

`build.sh` runs any `gen/*.jl`, flattens the style into
`build/<style>/style.css` (style lint, `lint/<style>/style.json`), writes the
JSON Schema, renders every spec and variant into `src/<style>/` with reports
in `lint/<style>/` (`chartgen build`), bakes into `build/<style>/`, and
compiles `charts-examples-<style>.pdf` with `--input style=<style>`; it exits
non-zero, after compiling, when a chart does not do what its spec expects.

Each entry prints: the name; "after *Practical Charts* p. 49 (s. 0049) ·
notes 00-01 F1 · status"; the chart (or, for a refused variant, a box
listing the refusal and what to supply; for a type of a later stage, a
dashed box) with a caption saying what it shows and the book page it
rebuilds; and the report, read with Typst's `json()`: its decisions grouped
by level (locked, guided, open) and its findings of severity warn and above
with their levels, plus the guided departures the spec gives a reason for.
The document lists its entries explicitly; a later stage adds its entries.
Status is `undrawn`, `generated`, or `compared` (checked against the book
page by the owner). There are no page images; the page references are for
the owner's copy.


## 9. Open design questions

The first three should be settled before phase 1 starts; each has a
recommendation. Status after stage 1: the owner settled 2 (muted as
`plain`) and 3 (labels at 10 pt, titles 1.2 × bold, full text width); the
others are built as recommended, provisionally: per-style generation (1),
the bake change (4), charts under `diagrams/charts/` (5), JSON only (6), the
bad versions as variants (7), refusal whenever a fact is missing (8, with the
owner's levels of 1.4 deciding what a missing fact means); 9 (a colour
registry) waits for multi-series charts.

1. **Per-style layout (settle first).** Layout reads the stylesheet's
   metrics, so chart sources are generated per style (1.5), unlike diagram
   sources, which are one file restyled by bake. The alternative is to fix
   all metrics (label size, gaps, line widths, band fractions) for every
   chart style, so that a style may change only paint; then one source
   serves all styles, but a house style could never change a size or a
   spacing. Recommendation: per-style generation.
2. **The default colour family (settle first).** `plain` in the muted family
   with `plain-office` as a second stylesheet for comparison (5.4), or the
   other way round, or only one of them. Recommendation: muted as `plain`.
3. **Chart proportions (settle first; settled: labels at 10 pt, titles 1.2 ×
   bold, owner 30 Sep 2026).** Labels at 9 pt (the diagrams use
   10 pt; the book's charts, scaled to 368 pt, would have 12 pt), titles at
   1.2 × bold, plot aspect 0.6, full text width by default. Every example
   and every layout threshold depends on these.
4. **The bake change.** Bold and italic labels by class in `bake.py` (1.6),
   or keeping bake as it is and writing weight into the Typst code of titles
   and notes (which puts typography outside the stylesheet).
5. **Where charts live.** The package and its stylesheets under
   `diagrams/charts/` (as proposed), or chart styles in `diagrams/style/`
   next to `neutral.css`, so that a future house style is one stylesheet
   covering diagrams and charts.
6. **Spec format for hand-written specs.** JSON only (comments impossible;
   the `source.shows` field carries the purpose), or also TOML or YAML for
   specs written by hand.
7. **The book's bad versions in the examples document.** Rebuild them as
   variants through overrides, with their findings printed (as proposed for
   about half the phase-1 cases), or show only the good versions.
8. **Strictness for AI-written specs.** Refuse whenever a fact is missing
   (as proposed), or accept with defaults and warnings when the fact only
   changes styling (for example `measure.persistence` on a regular grid).
9. **A document-level colour registry** (R3-24): a registry file per
   document (`examples/registry.json`, category → slot, filled in document
   order), or slots always given in specs. (As built, evaluation round 1:
   both, 2.10: a spec's `colours.slots` by name, or the registry file it
   names, written by hand rather than filled as the charts are made; a
   series' own `slot` still first.)


## Appendix A. Assumed rule-level defaults

Each is a single setting, so a decision changes one default. Stage 1 applies
`DECISIONS.md` as the owner directs (D-2, D-3 decided; the other
recommendations the provisional defaults); the rows below say so where that
changed what this table first assumed.

| Setting | Assumed | Notes' question |
|---|---|---|
| magnitude style | the magnitude once, on the top label, for every magnitude ("$300K", "25 million"); the suffix style a setting; the top value in full never; trigger 5 integer digits, the larger unit unless it needs two decimals | D-3 (decided), D-4; 02 §8 Q6, 05 §8 Q3–Q5, 06 §8 Q12 |
| unit phrase wording | `what` always in parentheses, a noun running on ("0.25 lbs. (weight of product)", "25 million inhabitants"); a horizontal scale's unit in its title below, in parentheses ("Donations ($K)") | D-20 (decided, owner 30 Sep 2026) |
| breathing band | 10–20 % target, nearest multiple with ≥ 5 % room | 02 §8 Q1 |
| inset triggers | narrow band 0.2, crush ratio 0.1, only when the facts ask; without the fact, the thresholds only prompt the question (an info, D-11; inset stage) | 02 §8 Q2, 08-12 §8 Q5 |
| zero decision when flags disagree | inset when `small_differences_matter`, truncation only with the facts of 6.1 (the audience's knowledge of the scale is not one: D-10 (a)) | D-10; 02 §8 Q3 |
| enough values for a line | known fraction ≥ 0.5 and a run of 3 | 06 §8 Q1 |
| markers on gaps | off (markers only round isolated values and straight runs; stage 2, lines: a straight run counts only on a line with a gap, three values in equal steps, 6.8) | 06 §8 Q2 |
| colour count | warn at 8 regular colours, error above unless directly labelled | 03-04 §8 Q1 |
| title alignment | start (left, flush with the label column) for every title | D-5 (a); 00-01 §8 Q5, 13-15 §8 Q6 |
| subtitle | an insight or question title has the descriptive line (what, by what, period) under it in the label style; a descriptive title carries the period itself (R5-27) | D-5 (a); 05 §8 Q6 |
| direct-label colour | neutral text grey | 03-04 §8 Q12, 06 §8 Q16 |
| end-label displacement limit | 1 pitch; up to 1.5 with a leader; then a legend (D-7; stage 2, lines) | 06 §8 Q5, 03-04 §8 Q7 |
| zero-line emphasis | inside signed ranges, and on change and index scales | D-17 (b); 00-01 §8 Q3, 02 §8 Q10 |
| highlight of bars | the strong shade of the bar's own hue, the others unchanged; paling the others a guided departure | D-9 |
| sort with several series | by the series the insight names, else the first; catch-all, total and missing last | D-46 |
| missing-note colour | #767676 rather than the book's lighter grey | R5-8 against 08-12 §5.1 |
| comment face (phase 2) | New Computer Modern italic in `--comment` | 05 §8 Q2, 13-15 §8 Q7 |
