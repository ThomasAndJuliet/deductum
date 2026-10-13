# Making a chart here: a note for AI producers

You do not draw a chart here. You write a **spec**: a JSON file of the facts
about the data and the choices that belong to this one chart. `chartgen` then
makes every decision the book's procedures can make from those facts, draws
the chart in the house style, and writes a report of what it decided and what
it found, each item with its rule and its **level**. The rules are in
`GUIDE.md`; the spec's fields are in `SPEC.md` §2 and in the JSON Schema
(`python3 -m chartgen schema`, or `chartgen/schema/chartspec-1.json`).

## What is yours to decide

Every rule has a level (`GUIDE.md`, "Levels of guidance"):

| Level | What it means for you |
|---|---|
| **locked** (chart) | Never depart from it. Supply the facts it depends on; never guess them. A spec that would break it is refused. |
| **style** (locked by the style) | Nothing to decide. No field sets a font, size, colour, weight or spacing, and a spec that tries fails validation ("unknown field"). Another look is another stylesheet. |
| **guided** | A procedure decides from the facts: chart type, orientation, sort, zero or truncation, inset, labels or legend, number format, highlight method. Leave the option on `auto`. Choose otherwise only with a reason. |
| **open** | Yours: the job, the insight, the title's words, what to highlight, category names, width (120–600 pt) and plot aspect (0.3–1.2). Nothing judges these, but the job and title must be there. |

## Before the spec: what was asked, and what you must not guess

The evaluation of 1 Oct 2026 (`eval/REPORT.md`) found that the generator
draws what the spec says and cannot know what it leaves out. Before writing
the spec:

- **List what the request explicitly asks for**: a chart type, values on
  the marks ("percentages on the slices"), where a scale starts, colours, a
  size, a format, a date style. For each wish, either name the spec field
  that carries it (values on the bars: `value_labels: "on"`, with the
  request as the reason) or decide not to follow it. A wish you do not
  follow goes in `reasons` where a field departs, and in your reply to the
  requester, in plain words: what you did instead and why.
- **Never let these default silently.** They come from the requester, never
  from you (G0.10, G0.18, G0.20):
  - the period with its year: a month or week given without a year is an
    assumption; choose the likeliest year, put it on the chart's
    descriptive line, and say in the reply that you assumed it;
  - special categories: "no answer", "missing", "all others", a total or an
    average, marked by their role (`x.roles`, `series.role`: `missing`,
    `all_others`, `total`, `average`) so that they take their reserved greys,
    whatever they are called in the data;
  - good and bad: colour a side good or bad only when the audience judges
    it so, and say which side is good and why in `reasons`; above a target
    is not good merely for being above it (overfilling gives product away);
  - units: check every data file's unit before combining them (kWh and MWh,
    thousands and units), and convert before the spec, never in a label.

## Writing the spec

1. **The job** (`job.kind`, `job.text`): `answer`, `make_aware`, `persuade`,
   `lookup`, `explain` or `other`, and the question, problem or use in words.
   A chart that answers, persuades or makes aware states its insight
   (`insight.important`, and the elements it names in `insight.targets`) in
   an insight title (`title.kind: "insight"`), with the descriptive line
   under it (`title.descriptive`: what, by what, the period with its year).
   A look-up chart has a descriptive title that names the same things. The
   generator never writes prose: every word of the title is yours. The title
   and the descriptive line each wrap onto at most two lines of the figure's
   width; a line that two lines cannot hold, or Typst markup wider than the
   figure (markup is never wrapped), is an error (CG-1): shorten it. Any text
   that would run past the figure's edge is reported the same way, never
   clipped.
2. **The data and what the numbers cannot say.** `data.x.kind` (`nominal`,
   `ordinal` or `time`) is required, because the order of categories cannot
   be read from them. A time axis needs `grid`, and `measure.persistence`
   says whether each value summarises its period, holds until the next, or
   is an event. Say whether zero means "none of it" (`measure.zero`), and
   give theoretical limits (`measure.min`, `measure.max`). A value that is
   not known is `null` or `{"status": "missing"}` (or `unknown`, `pending`,
   `tbd`, `error`), never `0`. `0` is a known zero. A result below a
   detection limit is neither: write it as the laboratory does, `"<1"` (or
   `{"below": 1}`; above the top of a test's range, `">2420"` or
   `{"above": 2420}`), never `0` and never a status. Heatmaps draw such
   values as a step of their own and tables write "<1"; other chart types
   refuse them (R0-12): choose a heatmap or a table, or, where the limit may
   stand for the value in this message, give the limit as a number and say so
   in a note. Mark totals, averages, "all others"
   and a category or series that stands for what is not known ("No answer",
   "Not known": role `missing`) with `x.roles` or `series.role`, so they take
   their reserved greys and go last.
3. **The unit.** `measure.unit` holds the parts of the unit phrase:
   `prefix` ("$"), `suffix` ("%", " lbs."), `noun` (runs on: "500 successful
   transactions") or `what` (in parentheses: "0.25 lbs. (weight of
   product)"). Be specific ("max employees during month", not "employees").
   Give the numbers as the data give them. `given_in` (`"thousands"`,
   `"millions"`, `"billions"`) says only that **the source itself is already
   scaled**: a sheet headed "£000" whose 1840 means £1,840,000. It is never a
   request to show the numbers in thousands: the generator hides digits by
   itself from five integer digits (G6.11), so monthly sales of £413,849 get
   a scale to "£500K" with no field at all, while `given_in: "thousands"` on
   those pounds would print "£500 million". The report warns (R5-11) when
   values given in thousands still reach five integer digits, or when a word
   of the unit ("thousand trips") says the magnitude again; if the source
   really is scaled that way, say so in
   `reasons["data.measure.unit.given_in"]`. (`magnitude`, a power
   of ten, is the older spelling of `given_in`.) Amounts of money are
   formatted by the generator: in pounds and pence (two decimals) wherever
   any value needs pence, never one decimal. A percentage scale writes "%" on
   every label, a horizontal one too, so a `scale_title` of your own leaves
   out "(%)".
4. **The facts the message needs** (`facts`): `message`,
   `small_differences_matter`, `small_values_matter`,
   `distance_to_zero_irrelevant`. Give only what you know. The defaults are
   listed by `chartgen choose`. An inset chart comes only from the two
   `…_matter` facts: when the small values beside an outlier, or the small
   differences in a narrow range far from zero, must be compared, say so and
   leave `inset` on `auto`; where the data merely look that way, the report
   asks the question (an info) and draws no inset. Keep the text width:
   where no empty area of the plot holds an outlier inset beside horizontal
   bars, it stands on its members' rows, sharing their names, and long names
   wrap and open the rows by themselves. Widen the chart only when the
   report says the inset was not drawn (a warning naming the remedy).
5. **Guided options**: leave `chart.type`, `orientation`, `sort`,
   `labelling`, `value_labels`, `scale.*`, `inset.*`, `panels.*` and
   `highlight[].method` on `auto`. If the chart must depart, set the option
   and say why in `reasons`, keyed by its path: `"reasons": {"chart.sort":
   "the order of the programme"}`.
6. **More than one series, measure or period** (phase 2; SPEC §2, §6.7,
   §6.10, §6.13):
   - several series over categories give clustered bars with a legend, or
     dots of several series; a series of role `target` or `reference` is
     drawn as a flat marker across each bar (a budget), named once;
   - `exact_values_needed` in `facts` puts a table under the graph; with
     fewer values than scale labels the bars are labelled and the scale
     dropped (`value_labels: auto`);
   - several variables go into panels: `data.dims` names them, each series
     its `keys`; `facts.compare` says which series the insight compares, so
     they share a panel; two measures in different units (`data.measures`,
     each series its `measure`) become a merged chart, never a dual axis;
   - irregular times take `x.kind: time` without a grid: states (a price)
     give a step chart and need an as-of date, events give thin bars; a
     rank message gives a bumps chart; `facts.incomplete_value_needed` draws
     a partial last period apart with its "As of" note;
   - **two periods** (2019 and 2025, before and after): the change between
     them is the point, and the default is an arrow chart, each category's
     change in units and in % (G1.46, G1.47), or a chart of the change
     itself (the % panel alone, `facts.message: relative_change`). Give x the
     two periods (`kind: time`, two values; `x.names` may name them) and a
     series per category. Clustered bars of the two periods, as requests
     often ask, are the book's most common mistake: two series named as
     periods across the categories are refused (R6-17), and
     `facts.message: periods` (each period's own values, not the change, is
     what the reader needs) counts only with its reason in
     `reasons["facts.message"]`, grounded in the request; without one the
     chart stays an arrow chart, with a warning. `chartgen choose` says so
     at step 5;
   - `audience.bw_risk` gives each line its own symbol (and textures good
     and bad marks: below).
7. **The parts of one total** (phase 3; SPEC §6.11–6.11b): give `data.parts`
   (`meaningful_total`, `all_parts`) and what the message features in
   `facts.message`; the chooser then gives a pie, a treemap, a wrapped bar
   chart or bars. A pie's slices are named outside with their values
   (`labels.parts: percent` for percentages of the total, `name` for names
   alone); "All others" (`x.roles`) goes last in grey, and after it a part
   that stands for what is not known ("No answer", role `missing`) in the
   missing grey; a part of zero value
   stays, named, with a leader; `x.groups` keeps a group's parts together
   (a pie of separated overlaps, a treemap's continents). Highlight a slice
   by `auto` (its strong shade) or `border`; a donut, an exploded slice and
   a legend are refused (R7-39, R7-38, R7-34). Several series in a pie give a
   pie per total, only when the fractions within each are the whole message.
   Parts with an order of their own (risk levels) are `x.kind: ordinal`
   (several totals: `data.parts.kind: ordinal`): they keep their order and
   take steps of one hue. A comment's anchor may name a slice or a tile; a
   treemap's tiles take their values under the names with `labels.parts`.
   Stacks and waterfalls (SPEC §6.18): the parts the insight names stack at
   the base; a waterfall's start balance, subtotals and end total are
   categories of role `total`, its steps in the process's order; charts of
   one set showing the same parts give each series a `slot`, so that a part
   keeps its colour as a line, a band or a segment (R3-24; a set of charts,
   item 10). Where the data's
   order is fixed (a table shared by several charts ordered differently),
   say so, `data.parts.order: fixed` (for categories along x, bars included,
   `data.x.order: fixed`): the parts, totals or bars then keep the order
   given rather than being sorted by size, and a table under the bars too. Parts in groups
   (`data.parts.groups`) take shades of a hue per group. Where values are
   labelled instead of a scale, the unit's description stands on a line of
   its own: give `unit.fallback` ("donations per donor") where the noun alone
   ("in donations") would read badly there.
8. **What the chooser reads** (phase 3; SPEC §3.2): leave `chart.type` on
   `auto` and state the facts; `chartgen choose` prints the path, step by
   step. For parts of a total, `data.parts.meaningful_total`, `all_parts`
   and overlap are required before any pie, stacked chart, treemap or
   waterfall: `parts.overlap: false` when no category contains another,
   else the pairs in `parts.overlaps` ([part, parent]); a missing one is
   refused, never assumed (owner, 1 Oct 2026); the message (`compare_parts_precisely`, `fraction_of_total`,
   `subtotal_amount`, `subtotal_fraction`, `cumulative_subtotals`; for
   several totals, x the totals and the series their parts,
   `compare_parts_across_totals`, `compare_totals`,
   `fractions_within_each_total`; over time `facts.total_matters` and
   `facts.focus`) picks the type. A table alone takes `facts.message`
   `lookup_only`, `export_only` or `not_comparable`; action dots
   `action_needed` (with `data.actions`); the funnel's replacement
   `stage_conversion`; a heatmap table `profiles`; a map
   `facts.location_matters` (phase 4); a line across ordinal categories
   `pattern` with `audience.data_savvy` (otherwise bars). A user's request
   for a donut, radar, funnel, word cloud, packed bubbles, bullet graph,
   choropleth, 3-D chart, smoothed line or dual axis is refused with its
   replacement: ask for the replacement's facts instead. Weeks are written by
   their first day, with
   `x.week_start` when it is not the locale's; binned years use `x.grid:
   bin` ("1960–1969"), and `measure.additive` when unequal bins may be added
   up.
9. **Notes under the chart** (SPEC §6.19; owner, 1 Oct 2026). Whatever a
   reader needs to read the chart right and the chart itself cannot say goes
   in a note: what a status in a cell means ("POA", "n/a"), a shortened name
   spelled out, an estimate, a definition, where the data come from. Never
   leave it to your reply: the chart travels without it. Give each note in
   `notes`, its words yours (a sentence, with its full stop), and in `on`
   every place its mark goes; with `on` empty it is a plain note, with no
   mark. A place is one of: `{"element": "title"}` (or `descriptive`,
   `unit`, `x_title`, `names_title`); `{"category": c}` or `{"series": id}`
   (the name where the chart writes it); both, for one value (its label, its
   cell, its empty slot's note); `{"status": "tbd"}`, every value of that
   status (its words from `labels.notes`), narrowed by `series` or
   `category`; `{"comment": 0}`; `{"stub": 0}`, a table's stub heading (with
   `series`, that row's entry). A value's own `note` (`{"status": "tbd",
   "note": "…"}`, `{"value": 45, "note": "Estimate."}`) is a footnote on it
   too, and values with the same note share it. The generator numbers the
   marks in reading order, keeps room for them, and sets the notes under the
   chart, wrapped to its width; the look of the marks is the style's. A place
   it cannot find (a name the chart does not write) is a warning, the note
   then standing without a mark: give another place, or make it a plain note.

   ```json
   "labels": {"notes": {"tbd": "POA", "missing": "n/a"}},
   "notes": [
     {"text": "Price on application.", "on": [{"status": "tbd"}]},
     {"text": "Not available to commercial users.", "on": [{"series": "sorter", "category": "Commercial"}]},
     {"text": "Source: the facility's price list, 2026–27."}
   ]
   ```
10. **A set of charts** (SPEC §2.10; G4.22, locked). Charts of one document
    that show the same things (centres, regions, products) must give each
    the same colour in every chart, and each chart hands out its colours by
    its own order (lines by their ends), so fix them. For the document, write
    its registry once, a JSON file of names and palette slots outside the
    specs' folder (`{"Brackley Hill": 1, "Elstead Park": 2}`), and name it in
    every spec: `"colours": {"registry": "../registry.json"}` (the path from
    the spec's file). For one spec, `"colours": {"slots": {…}}` does the same;
    `series.slot` fixes one series. Names are those the reader sees: a
    series' name, in small multiples its value of the dims inside the panels,
    a pie's category. Slots are 1–16; give two names one slot only by mistake:
    the second then takes a free slot, with a warning. The report lists the
    slots fixed (R3-24, or R6-28 in small multiples).

## Running it

From `diagrams/charts`:

```
python3 -m chartgen choose spec.json          # the decision path, before any layout
python3 -m chartgen render spec.json --style plain --out chart.svg --report report.json
python3 -m chartgen lint chart.svg --style plain   # any SVG in the vocabulary; render already runs it
```

`choose` prints each decision with its rule and level, the refusals so far,
and the facts you left to defaults. Orientation and labelling wait for the
typeset label sizes, so only `render` settles them. `render` writes the SVG
source (baked by `../tools/bake.py`, as `examples/build.sh` does) and the
report.

## Reading the report

- `status`: `drawn`; `refused` (a locked rule, so no SVG is written);
  `invalid` (a malformed spec, CG-4); `unsupported` (a later stage of the
  generator, CG-3).
- `decisions`: what the generator decided and why, each with its rule, its
  guide rules and its level. The guided ones are what `auto` gave.
- `findings`, by level and severity:
  - **locked, error**: the chart as specified would misrepresent the data,
    or lacks a fact. Supply the fact or change the request (below).
  - **guided, warn**: your explicit option departs from the procedure. The
    finding names what the procedure gives (`recommended`). Return to `auto`,
    or add the reason, which turns it into an info.
  - **locked, warn**: a locked rule measured against a provisional number
    (text contrast 4.5 : 1, text of at least 9 pt, colours distinguishable
    under colour-vision deficiency). Treat it as a problem to fix.
  - **style**: the stylesheet's affair, not the spec's.
  - **info**: defaults standing in for facts, and features of a later stage.

## Refusals and overrides

A refusal names the rule, the field (`path`) and what to `supply`. Do what
it says. Never work around it: do not write `0` for a missing value, pin
`scale.min` for bars, rotate or shrink text, or relabel a chart `lookup` to
escape the insight rule when the chart really does answer a question.

`overrides: [{"rule": "R2-24", "reason": "…"}]` lets a refusal through. The
finding stays in the report as `overridden`, with the reason. It exists to
rebuild a deliberately bad version for comparison, such as the book's
truncated bars or its over-pruned chart, and is not for a chart anyone is
meant to trust. `purpose: "analysis"` turns refusals into warnings for
charts made only to explore the data.

## Making the message obvious (phase 2)

The book's Part 5 (`GUIDE.md` G8; `SPEC.md` §6.12) asks for things an AI
leaves out unless told: comparison values, callouts, a good/bad reading, a
calculated value. They are facts and open choices of the spec:

- **Comparison values** (`data.comparisons`): what the audience would judge
  the numbers against, a target or budget before an average, a previous
  period or a peer (G0.19). One value per category, or one value
  (`"values": [3.45]`); `compute: "mean"` for an average of the plotted
  data. Mark a doubtful one `"valid": false` with its `reason`: it is left
  out, never drawn. Leave `render` on `auto`. Give every comparison the
  chart needs, each with its own `label`: several are drawn together (a
  target per bar and a threshold line; at most two marks across each bar,
  the second dotted). A comparison derived from another (5 % over each
  target) is best given beside a `transform: deviation` from that target:
  it is then drawn in the deviation's unit, +5 %, as one line. Give a
  comparison's values in the measure's unit; if the source gives them in
  another (targets in kWh, use in MWh), say so, `"unit": {"suffix": " kWh"}`
  with `"factor": 0.001`, and the generator converts them and says so. A
  comparison in another unit without a factor is refused (R2-13); one an
  order of magnitude away from the data is a warning naming units as the
  likely cause. On a chart with an inset the line is drawn in the inset too
  where its value lies in the inset's range.
- **Good and bad** (`measure.good`: `higher` or `lower`, and
  `measure.dividing_point`: a number or `{"comparison": id}`): the bars and
  dots on each side are coloured, green and red, for every reader, in one
  file (D-18). Colour is the default. For black and white or print, set
  `audience.bw_risk: true`: good and bad are then told apart without colour
  too (a hatch over bad marks, a dither over good ones, a bad dot filled),
  which serves colour-blind readers as well. For a publication to unknown
  readers, choose a style that sets `--good-bad-mode: safe-pair` (orange and
  blue, with a red/green copy beside the file); the spec never chooses it.
- **Colour for a quantity** (a heatmap, a shaded table): leave
  `chart.encoding` on `auto`, and give `measure.dividing_point` only when the
  audience sees a value as dividing (zero, a target, a rating's neutral
  middle; never the middle of the data), with `measure.good` when its sides
  are good and bad. The ramp is then diverging, otherwise sequential; its
  colours are the style's. Leave `chart.classing` and `chart.cells` on
  `auto` too; when the values are skewed, say in `facts.ramp_focus` whether
  the few extremes (`extremes`) or the pattern among the many (`pattern`) is
  the message (for the pattern, amounts spanning two orders of magnitude take
  logarithmic steps by themselves, others natural breaks). Ask for
  quantiles or your own `chart.breaks` only with a reason, and for muted
  cells only for a table whose colour merely hints.
- **A calculated value** (`transform`): `deviation` from a comparison value
  (who beat the target by most), `ratio` per an exposure (entities of
  different size), `index` at a common start (patterns of change of series
  in different units, instead of two axes). Keep the raw values in `data`;
  the transform is part of the spec.
- **Callouts** (`comments`): your words, each with the elements it points at
  (`anchors`) and which of the four questions it answers (`answers`:
  `important`, `why_care`, `explain`, `action`). A callout answering
  `important` states the insight as an insight title does. The generator
  places them and draws their arrows; it never writes them. On thin bars and
  step charts an anchor names an event by its time as written in `points`
  (`{"series": "spills", "x": "2025-11-13T01:50"}`), or by its day when that
  day holds one event only. On merged charts and small multiples the anchor's
  series names its panel (`panel`, a measure's id or a row's value, where it
  has no series).
- **Dates marked on a time axis** (`data.x.events`: `[{"at": "2025-06-15",
  "label": "Storage tank finished"}]`): something outside the data that the
  message refers to (a tank finished, a competitor opened). It is drawn as a
  thin line across the plot at its date, its label beside it; a date the
  chart cannot draw is reported, never dropped. Give the date as the source
  gives it; a "mid-June" without a day is an assumption to state in the
  reply. A month alone ("2026-03") is its first day. In small multiples the
  line stands in every panel and its label once, in the first panel that
  holds it. A note for an event whose value is not known is
  `labels.notes.unknown` ("(duration not recorded)"); it is placed clear of
  the bars beside it, above them with a leader where it must be.
- **Highlight methods** besides the default: `border`, `glow`, `background`,
  `icon`, `selective_label` (a line's stretch is `{"series": id, "x": [from,
  to]}`). Choosing one is a guided departure: give the reason. A glow's
  hue is guided: say `"emphasis": "strong"` when the target must stand out
  and leave `glow` on `auto`; it takes the complement of the target's
  colour unless a contrast would confuse (more than 3 colours, a legend, a
  colour near the complement), else a pale tint of its own.
- **The title connector** (`title.link`): a reference to the mark the insight
  title names, where the eye would not find it at once (G8.8).

## Before you deliver: verify, then have it checked

1. **Look at the rendered chart**, not only the report: the report cannot
   see a label that says the wrong thing, a year that is wrong, or a value
   the requester asked for that is not there.
2. **Verify** (G8.27 [L]): recompute from the data, with code, every number,
   date, name and claim the chart shows (labels, title, callouts, notes)
   and every one your reply makes, and compare. Any fact taken from neither
   the data nor the request (a year, a date, a cause) is an assumption: say
   so on the chart or in the reply. Most misleading charts in the
   evaluation were well drawn but stated something false.
3. **Have it checked.** When all the figures of a job are made, a fresh
   agent that did not make them checks each one against the guide, following
   `CHECKING.md`, and you fix what it finds. Where the check asks for
   something the generator cannot draw, report the gap; do not redraw the
   chart in another tool.

## A worked example: a look-up chart

```json
{
  "chartspec": 1,
  "id": "workshop-registrations",
  "job": {"kind": "lookup", "text": "Look up how many people registered for each workshop"},
  "title": {"text": "Registrations by Workshop, Spring 2026", "kind": "descriptive"},
  "data": {
    "x": {"kind": "nominal",
          "values": ["Algebra", "Geometry", "Topology", "Analysis", "Combinatorics", "Logic"]},
    "measure": {"name": "registrations", "kind": "count", "zero": "meaningful", "min": 0,
                "unit": {"noun": "registered participants", "scale_title": "Registered participants"}},
    "series": [{"id": "reg", "name": "Registrations", "values": [42, 37, 29, 25, 18, null]}]
  },
  "facts": {"message": "compare"},
  "chart": {"type": "auto"}
}
```

Logic's count is not known, so it is `null`. The job is to look values up,
so the title only describes the chart. Nothing about the look is set.

`python3 -m chartgen choose` (excerpt):

```
Decisions, in order (level, rule, guide rules)
    guided   02 D1   –                  type bar: categories compared by value
    locked   R2-24   G1.57, G2.6, G2.8  bars start at zero
    guided   02 D5   –                  scale 0–50 by 10

The choice
    orientation  auto: the layout settles it from the typeset label widths (6.7; R9-10, G2.4): run render
    sort         desc by reg  [R2-01 (G2.1), R9-9; guided]

Facts the spec does not give (the generator's default; what reads it)
    facts.small_differences_matter       false      a narrow-range inset (R2-23, R9-6)
    facts.small_values_matter            false      an outlier inset (R2-20)
```

The report after `render` (excerpt; status `drawn`):

```
guided  R9-9   last: Logic (catch-all, total, then values not known)
guided  R9-10  “Analysis” and “Combinatorics” come within 10 pt of each other under vertical
               bars, with every multi-word name wrapped: horizontal bars, names right-aligned
locked  R9-7   empty slots say why: Logic (missing)
locked  R2-07  a horizontal scale: bare numbers, and its title centred below …: “Registered participants”
finding  guided info  R2-32  5 values against 6 scale labels: labelling the values directly would
                             put fewer numbers on the chart (phase 2)
```

It has no error or warning, so the chart is done. The info only notes a
phase-2 alternative. Three changes to the same spec show the other outcomes:

- `"job": {"kind": "answer", …}` with the same title is **refused** (R0-10,
  locked, `title.kind`). The remedy is an insight title,
  `{"text": "Algebra Drew the Most Registrations", "kind": "insight",
  "descriptive": "Registrations by Workshop, Spring 2026"}`, with
  `insight.targets: [{"category": "Algebra"}]`. Algebra is then drawn in
  the strong shade of its own colour.
- `"chart": {"orientation": "vertical"}` is **refused** (R2-08): the names
  would have to overlap or be rotated.
- `"chart": {"sort": "given"}` is drawn with a **guided warning** (R2-01;
  the procedure gives `desc`). With
  `"reasons": {"chart.sort": "the order of the programme"}` the warning
  becomes an info that records the reason.
