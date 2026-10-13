# Rulings

Juliet's **decided** rulings for chartgen, and the tests that hold the
implementation to them (`tests/rulings/`). A change that breaks a ruling fails
CI, whoever makes it. Changing a ruling, or its test, needs Juliet's approval
(the `needs-juliet` label; see `AGENTS.md`).

Only decided rulings are here: the entries of `DECISIONS.md` marked "Decided
(owner …)" or carrying an owner's note, and the owner decisions recorded in the
Python prototype's README (in Juliet's sandbox, `charts/README.md`; their
substance is restated below). Every other recommendation in `DECISIONS.md` is
a provisional default, to be judged in the example charts, and is deliberately
not locked by a test.

A new ruling is added here, to `DECISIONS.md` where it has an entry, and to
`tests/rulings/` in the same change. Where a ruling cannot yet be tested, the
reason is given.

Status: **tested** (the tests named), **partly tested** (what is still open is
said), **not yet tested** (testable; to do), **not built** (the Rust
implementation does not do it yet), **deviation** (Rust does something else; the
ignored test says what), **not testable** (a working practice, not behaviour).

A test may be written before its behaviour is built, marked `#[ignore]` with the
reason: it is the acceptance test for that work, and whoever builds the
behaviour removes the `#[ignore]` (a ruling-test change, so it comes to Juliet).
`cargo test -p chartgen-rulings -- --ignored` runs them.

## From the prototype's README (owner decisions, 30 Sep – 1 Oct 2026)

| ID | Ruling | Status | Tests |
|---|---|---|---|
| O-1 | The plain style is flat: no cards, shadows or arched strips; shadows belong to a later house style, never to the plain one. | tested | `style.rs`: `o1_*` |
| O-2 | The plain palette is the muted one (#95B7D3, #DD8045, #A4AB81, #D8B35B, teal, purple #9A77B7 sixth), the teal turned 24° towards blue (#78A6AA); a single series is blue. Same as D-2. | tested | `style.rs`: `o2_*` |
| O-3 | Labels at 10 pt; titles 1.2 times, bold. | tested | `style.rs`: `o3_*` |
| O-4 | A step of 5 or of 1 that leaves more than a quarter of the scale empty above the data gives way to the next smaller step (5 → 2, 1 → 0.5, 10 → 5) if that needs at most 8 intervals; it departs from the book once, knowingly (3.16: 0–3.5 by 0.5). | tested | `scales.rs`: `o4_*` |
| O-5 | Guidance has three levels. Locked: rules no chart breaks, and the values a style fixes (font, sizes, palette, greys), which live only in the stylesheet, so a spec cannot set them. Guided: book procedures, departed from only with a stated reason. Open: the one chart's choices. | tested (a spec setting style, font, theme or palette is invalid) | `locked.rs`: `o5_*` |
| O-6 | Names under vertical bars are centred and keep 1 em between neighbours; where they meet, every multi-word name wraps to two lines, never after an abbreviation nor just before "&"; if they still meet, the chart turns horizontal. | partly tested (a chart whose names would still meet turns horizontal); deviation: Rust turns horizontal as soon as a name exceeds its band and never wraps first, where O-6 lets names reach past their bands with 1 em between neighbours (ignored test) | `names.rs`: `o6_*` (one ignored) |
| O-7 | Insets: text 8 pt; labels repeated by default, one shared axis when aligned and space is short (D-56); at most about as tall as the main chart; a number never parts from its currency or magnitude. | partly tested (the style's inset values); insets not built (see D-56) | `insets.rs`: `o7_*` |
| O-8 | Value labels by default where the book's procedure gives them; comments in New Computer Modern italic in the reserved blue; italic marks comments and the chart's own notes, keys stay upright. | tested | `comments.rs`: `o8_*`; `value_labels.rs`: `o8_*` |
| O-9 | A table under several series keys its rows: names where they would be, a square of each series' colour on the value axis; the legend dropped by default. | not built (ported in `chartgen-layout`'s `composition.rs`, not yet wired into the bar layout); tests written and ignored until then | `table_keys.rs`: `o9_*` (ignored) |
| O-10 | Time labels never touch a divider: the end labels move in, a label beside an inner divider keeps its place and the divider breaks round it; labels are thinned, never dropped for a divider nor shrunk; too narrow even then, the chart is refused with the width it needs (D-45). | partly tested (thinning, restarting in each group); not built: the halo beside an inner divider and the end clearance (the ported time axis is not used by the line chart; ignored test); the refusal of a chart too narrow still open | `time_axis.rs`: `o10_*` (one ignored) |
| O-11 | Before any pie, stack or treemap the spec states whether parts overlap, as well as a meaningful total and that all parts are present (D-1, D-24). Where the data's order is fixed, bars and stacks follow it (D-26). Diverging ramps are symmetric about the dividing value (D-37). Totals and labels follow the scale's magnitude; a line crossing a label stops short of it. | partly tested (stated facts and overlaps, the fixed order, the symmetric ramp, labels following the magnitude); lines stopping short of labels still open | `parts.rs`: `d1_*`, `d24_*`; `order.rs`: `o11_*`; `diverging.rs`: `d37_*`; `value_labels.rs`: `o11_*` |
| O-12 | Values in heatmap cells: strong cells, each value black or white, whichever contrasts more; muted cells a per-chart option (D-57 (ii)). | tested | `heatmap.rs`: `o12_*` |
| O-13 | An empty slot's note wraps to at most two lines inside its slot; else under the slot's label; else above, with a thin leader. | partly tested (the note in its slot, on horizontal bars); wrapping and the fallbacks still open (`p3-27` `missing` does not render in Rust yet) | `notes.rs`: `o13_*` |

## From DECISIONS.md

| ID | Ruling | Status | Tests |
|---|---|---|---|
| D-2 | The muted palette (see O-2). | tested | `style.rs`: `o2_*` |
| D-3 | A value scale states its unit and magnitude once, on the top label; the other labels are bare numbers with the same decimals; the suffix on every label is a setting; the top value in full, never. | tested | `unit_phrase.rs`: `d3_*` |
| D-18 | Semantic colours for every reader: in colour, plain green and red in one file; in black and white, textured (hatch for bad, dither for good); the colour-blind-safe pair a style option, with a red/green copy. | tested | `good_bad.rs`: `d18_*` |
| D-20 | A unit's description is in parentheses, a count noun runs on; only a vertical scale runs its unit on from the top label; a horizontal scale has bare numbers and a title below, the unit in parentheses. | tested | `unit_phrase.rs`: `d20_*` |
| D-37 | A ramp diverges only about a stated dividing value (refused otherwise, R3-13); symmetric about it, for now. | tested | `diverging.rs`: `d37_*` |
| D-55 | A chart whose job is to answer, persuade or make aware states its insight (refused otherwise, R0-10); a look-up or analysis chart may have a descriptive title, and nothing is reported. | tested | `insight_title.rs`: `d55_*` |
| D-56 | An inset repeats its labels by default; aligned and short of space, one shared axis. | not built (insets are not implemented in Rust); tests written and ignored until then | `insets.rs`: `d56_*` (ignored) |
| D-57 | Colour steps cut by the data and the message: equal steps, natural breaks for skewed data whose pattern is the message, logarithmic 1–2–5 steps automatically for such amounts on a ratio scale spanning two orders of magnitude, never for data not on a ratio scale. | partly tested (equal steps for fairly even values, the lightest step distinct from the page); logarithmic steps, extremes and asked-for cuts not built for their example (`p3-51`, a heatmap over time, does not render in Rust yet; ignored test) | `colour_steps.rs`: `d57_*` (one ignored) |
| D-58 | Charts made by AI are made with the generator, every stated fact recomputed from the data, then checked by a fresh agent (`CHECKING.md`). | not testable (a working practice) | |
| D-59 | A horizontal percentage scale writes "%" on every label; its title has no "(%)". | tested | `unit_phrase.rs`: `d59_*` |
| D-60 | In black and white, special categories are plain flat greys among textured regular parts. | not built (only good and bad are textured so far) | |
