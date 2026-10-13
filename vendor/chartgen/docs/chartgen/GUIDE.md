# Chart guide

Rules for making a chart, taken from Nick Desbarats, *Practical Charts* (2023), by way of the reading notes in `notes/`. They are written for whoever makes a chart, and above all for an AI: each rule is an instruction whose result can be checked. The sections follow the order in which a chart is made, not the order of the book. The guide is meant to be used on its own.

## Levels of guidance

Every rule carries a tag, right after its ID, that says how much the maker of a chart may decide and how the generator (`SPEC.md` §1.4 and §2) holds the rule. The four levels apply the owner's principle of 30 September 2026 (`README.md`), which takes the locked / guided / open split of design kits and divides "locked" into what every chart obeys and what a style fixes.

- **[L] Locked (chart).** The rule holds in every chart, whatever the style: bars start at zero (G2.6), no text is rotated (G6.6), a missing value is a gap and never zero (G7.9, G0.16), scale labels are formatted as a set (G2.24), gridlines are lighter than every data colour (G3.2).
  - *For an AI:* never depart from it. Supply the facts it depends on (whether categories are ordered, whether an irregular series holds states or events, theoretical limits); never guess them.
  - *Generator:* a **default** built into the drawing, with no spec field to switch it off (no frames, ticks or axis lines; horizontal text; labels formatted as a set; gaps for missing values), or a **refusal**: a validation error that names the rule, the field and what to supply (a bar scale not from zero, a line across unordered categories, a scale past a theoretical limit, a communication chart without a job). The only way past is `overrides: [{rule, reason}]`, kept for rebuilding the book's bad versions and always reported, with severity `overridden`. `purpose: "analysis"` turns refusals into warnings.
- **[S] Locked (style).** A value that one style fixes: the typeface, text sizes, the palette and its slot order, the greys, line widths, spacings, the bar-to-gap ratio as a number.
  - *For an AI:* there is nothing to decide, and nothing of this kind goes into a chart spec. Changing such a value means making a different style.
  - *Generator:* the stylesheet (`style/*.css`) holds the value as a token (SPEC §1.5, §4.6), and the chart spec has no field that sets it. The **style lint** checks the values once per style against the locked rules they serve (the Style lint checks of G9).
- **[G] Guided.** A decision made from facts by a procedure the book gives or implies: the chart type, orientation, sort order, zero or truncation, whether to add an inset or a table, direct labels or a legend, number format and magnitude, the palette type for what colour means, the highlight method, which comparison values are valid. Each [G] rule says what to take into account, in its *When* and *Why* or on a **Consider:** line.
  - *For an AI:* leave the option at `auto` and supply the facts; choose otherwise only with a stated reason.
  - *Generator:* options default to `auto` (`chart.type`, `orientation`, `sort`, `labelling`, `value_labels`; `scale.zero`, `magnitude`, `magnitude_style`, `unit_position`; `inset.kind`, `position`; `highlight.method`), and `auto` runs the procedure on the spec's `data`, `facts` and `audience` (SPEC §2.2–2.7). An explicit choice that differs from the procedure's answer is reported as a finding that names the answer and the rule; with its reason given in the spec (`reasons`, keyed by the option) it is recorded as a deliberate departure (G0.3). Every decision goes into the report with its rule, and the examples document should show every guided decision (the index "Guided decisions and the examples that should illustrate them").
- **[O] Open.** A choice made for the one chart that no procedure settles: its purpose and job, the insight, the wording of the title and callouts, what to highlight, which variables to leave out for this message, the names of the categories, the chart's size and proportions within the style's limits. Each [O] rule says what makes a good choice, on a **Good choice:** line, not what to choose. Category names have no rule of their own: short, in the audience's words, and fitting on two lines (G6.7).
  - *For an AI:* make the choice and record it; when the request does not settle it, ask or state an assumption (G0.8).
  - *Generator:* it never makes or judges these choices, and it writes no prose. They are spec fields (`purpose`, `job`, `insight`, `title`, `highlight[].targets`, `comments`, `layout`). That the required ones are present is locked (G0.8, G0.10): a communication chart without a job or a title is refused.

**In kit terms.** The [S] values are the *atoms*, the tokens of a stylesheet. The *components* (a value scale with its unit phrase, a time axis, end labels, a legend, an inset, the note on an empty bar, a callout) carry the [L] rules and take their looks from the atoms. The [G] procedures choose each *chart type* and configure its components. The [O] choices fill in *the chart*.

**Rules of mixed level.** A rule is tagged by its main instruction. Where it also touches another level, it names the rule that holds that part: G2.4 decides the orientation [G], and the horizontal text it relies on is G6.6 [L]. Where a book rule joined a constraint to a value, the rule is split. The original ID keeps the constraint as [L], and the same ID followed by *s* holds the value as [S] (G3.2: non-data lines light, thin and lighter than every data colour; G3.2s: ≈ #D9D9D9, about 0.08 em). The values in [S] rules are the book's, or the owner's where the rule says so; the stylesheet holds the current ones. No ID has been renumbered or removed.

**Counts.** The guide has 322 rules: 162 [L], 38 [S] (36 of them the *s* parts of split rules), 111 [G] and 11 [O]. The counts per section are at the end. The checklist (G9) gives each check its level and gathers the checks of style values under Style lint.

## Reading the guide

**How to read a rule.** Each rule has an ID (G‹section›.‹n›), its level ([L], [S], [G] or [O]; see Levels of guidance), an instruction, *When* it applies, *Why* (one line), *Except* (exceptions) and *Src*: the rule IDs in the notes and the book's pages (p.). A guided rule may add a **Consider:** line with the facts and trade-offs to weigh; an open rule adds a **Good choice:** line. The style part of a split rule has its main rule's ID followed by *s* (G3.2s). Markers, after the level:

- **[KT]** the book states it in a key-takeaway box.
- **(fig.)** read from the book's figures, not stated in its prose.
- **(readers')** a number or procedure of the note-takers, not of the book.
- **(proposed)** a default proposed in this guide where neither the book nor the notes give one.
- **[D-n]** the rule gives a provisional default for an open question or a conflict; the owner's decision on D-n in `DECISIONS.md` replaces it.

In *Src*, the same markers after a rule ID show which source rules carry them. A rule that names another rule (G2.6) means "apply that rule here". Sizes in *em* are multiples of the scale-label font size. The index of sources lists, for every rule in the notes, the guide rules that cite it (the *s* part of a split rule shares its main rule's entry); after it come the indexes of the style's values, of the guided decisions with the examples that should show them, and the counts per level.

**The owner's decisions override the book and the notes.**

- The generator draws the book's plain style: flat, on white, with no cards, shadows, strips or other 3-D effects. A house style may come later; shadows never enter the plain version.
- Every element carries a class naming its role (bar, gridline, scale label, highlighted series), never its look; a stylesheet gives the look.
- Labels are typeset by Typst in New Computer Modern and baked into the SVG.
- What software does badly gets particular care: the value-scale title is set horizontally at the top of the scale, never rotated, and inset charts are built in.
- Guidance has levels: what every chart obeys, what a style fixes, what the book's procedures decide from facts, and what is chosen for the one chart (Levels of guidance).

**Terms.**

- *Spread*: highest value minus lowest value.
- *Theoretical minimum / maximum*: a bound the data cannot cross (0 % and 100 %; a credit score of 300).
- *Ratio scale*: a measure whose zero means "none of it" (counts, money, lengths). Temperatures in °C, dates and rating anchors are not ratio scales.
- *Nominal / ordinal categories*: categories without / with a built-in order (regions / risk levels).
- *Regular series*: exactly one summary value per equal interval (day, week, month). *Irregular series*: values stamped at arbitrary instants.
- *State / event*: a value that holds until the next one (a price, rate, balance, record) / a one-off occurrence (a sale, visit, concert).
- *Dividing value*: a value the audience sees as separating two kinds of value (zero, a target, the average, an expected value).
- *Special categories*: total, average, all others, missing.
- *Non-data element*: anything that would remain if the data were deleted (frames, axis lines, ticks, gridlines, fills).
- *Unit phrase*: the words saying what the value scale measures, written after the top scale label ("25 million inhabitants").
- *Band*: the slot a category occupies along a categorical axis.
- *Insight*: the specific thing the audience should notice, understand or do.
- *Comment* (callout): the maker's remark about the data, as opposed to a label that names part of the chart.

## G0 Before drawing

### The job and the message

**G0.1** [O] [KT] Decide whether the chart is for communication (an audience must see something) or for the maker's own analysis, and apply this guide in full to communication charts.
*When:* at the start of every chart. *Why:* analysis charts may use any type and need no stated insight; communication charts must be familiar and obvious. *Except:* data art, infographics, dashboards and specialised scientific charts are outside the guide; charts judged in competitions may be scored on other criteria. *Src:* R0-02 [KT] p. 25–28; RP7-5 p. 427.
**Good choice:** communication whenever anyone but the maker will see the chart; analysis only for the maker's own exploration.

**G0.2** [L] [KT] Treat every rule on formatting and on chart choice as a correctness requirement, whoever asked for the chart and however good its insight; aim for charts that are clear and credible, not beautiful.
*When:* always. *Why:* such mistakes make audiences misread the data or doubt the maker, and they defeat charts made by experts too. *Except:* none. *Src:* R0-01 [KT] p. 16–17, 21, 44–45; R5-25 p. 158–159.

**G0.3** [L] [KT] Settle each choice by the rule that covers it. Where a rule leaves a choice open, expose it as a named setting with a stated default; never resolve it silently. Depart from a rule only deliberately, and record why.
*When:* every design choice. *Why:* the book exists to replace "use your judgement" with explicit defaults that can come off once understood. *Except:* the author concedes that some choices resist rules. *Src:* R0-04 [KT] p. 28–31, 34.

**G0.4** [O] [KT] Before any design choice, state the chart's job: the question it answers, the problem it makes the audience aware of, the action it urges, or that it is for looking up values. If no job can be stated, do not design the chart; find out first.
*When:* before designing. *Why:* the job is the main input to nearly every later decision; starting from the data or from the software is the common mistake. *Except:* none (lookup counts as a job). *Src:* R0-07 [KT] p. 35–40.
**Good choice:** a job specific enough to judge the chart by: a question someone could answer by looking at it, a named problem, a named action, or the values to be looked up, for a named audience.

**G0.5** [O] [KT] Decide the insight before choosing anything else, and derive the chart type, measure, categories, time span, palette, sort order, scale and highlight from it. The structure of the data is a secondary input.
*When:* every communication chart. *Why:* every design choice brings some insights forward and hides others, so a chart made to "just show the data" communicates random ones. *Except:* none. *Src:* R0-08 [KT] p. 39–40; RP5-2 [KT] p. 392–395; R15-4 [KT] p. 402–403.
**Good choice:** one sentence the audience should take away, true of the data, naming the elements it concerns (so they can be highlighted), and specific enough that it could have been false; "sales by region" is a subject, not an insight.

**G0.6** [O] Choose the insights to feature from your most honest reading of the data and in the audience's interest; never design a chart to play down or hide a problem the audience would want to know about.
*When:* choosing what to feature. *Why:* doing so is lying, as it would be in a report. *Except:* none. *Src:* RP5-3 p. 394.
**Good choice:** the insights you would give the audience in person if they asked what matters, the unwelcome ones included.

**G0.7** [G] [KT] If you cannot find out why the audience needs the data, make a set of views instead of one chart: list two to four specific questions they probably care about, make one view per question, title each view with its question, and revise after their feedback. Merge views that share their categories into one figure (side-by-side panels on one category axis).
*When:* the request comes without a purpose. *Why:* a general-purpose chart that answers every question does not exist, even in theory. *Except:* none. *Src:* RP5-5 [KT] p. 395–398; RP5-6 p. 396–397.
**Consider:** whether the purpose can still be learned by asking, which beats guessing; which questions the audience's role makes likely; which views share their categories and can be merged.

**G0.8** [L] When an AI makes the chart, it must obtain from the requester, or state as an explicit assumption: which chart is needed (the job), what part of it to highlight, the callouts stating the key insights, and any comparison values from outside the data. [D-1]
*When:* every AI-made chart. *Why:* these are the things the author found AI-made charts get wrong on their own. *Except:* none. *Src:* RP7-3 p. 430–433.

**G0.9** [O] [KT] Show only information this audience needs. For a mixed audience, make separate charts or reports for each member or segment rather than one for everyone.
*When:* deciding content. *Why:* unneeded information is a main cause of boredom, and no design skill fixes it. *Except:* none. *Src:* RP6-4 [KT] p. 423–425.
**Good choice:** what this audience needs for its question or decision and nothing it would skip; when one member needs what the others do not, a separate chart.

### The brief

**G0.10** [L] Fill in this brief before drawing. Rows marked *required* must come from the requester. For the others, use the default shown and record it on the chart as an assumption (readers', assembled from the notes' input lists) [D-1].
*When:* every chart. *Why:* most of the rules below depend on facts the numbers cannot supply. *Except:* none. *Src:* R0-07 [KT] p. 35–40; RP7-3 p. 430–433; R6-1 p. 170–171; R3-13 [KT] p. 125–127.

| Field | Values | Used by | Default when unknown |
|---|---|---|---|
| job | question / problem / action / lookup | everything | *required* |
| insight | a sentence, and the elements it names | title, highlight, type | *required* (else G0.7) |
| audience expertise | general / data-savvy | G1.4, G2.35, G1.58 | general |
| audience conventions | dates, number separators, abbreviations, colours | G6, G4.23 | none known |
| colour-vision deficiency possible | yes / no | G4.18 | yes |
| red = bad and green = good in this culture | yes / no | G4.16 | yes |
| black-and-white reproduction likely | yes / no | G4.25, G7.14 | no [D-33] |
| exact values needed (legal or genuine) | yes / no | G1.14 | no |
| readers with visual, cognitive or physical impairments | yes / no | G1.15 | no |
| pie-averse audience | yes / no | G1.40 | no |
| audience already knows the scale of the data | yes / no | G2.8 | no |
| output | static / interactive | G5.7, G4.18 | static |
| measure and unit | full unit phrase | G2.19 | *required* |
| theoretical minimum and maximum | numbers | G2.11, G2.13 | none |
| zero means "none" (ratio scale) | yes / no | G2.10 | yes for counts, money, lengths [D-10] |
| category kind | nominal / ordinal / grouped | G0.18 | nominal |
| special categories | total, average, all others, missing | G4.15 | none (names are only hints) |
| parts of a total | the four checks | G0.12 to G0.15 | not parts of a total |
| time-series class | regular / irregular; state / event; enough values | G0.17 | computed; state or event *required* for irregular series |
| as-of date | date | G2.40 | the last date in the data |
| value status | number / zero / missing / unknown / TBD / error | G0.16 | a blank is missing, never zero |
| comparison values | kind, values, valid or not | G0.19 | none |
| good/bad reading | dividing value and good side | G0.20 | none: no semantic colour, sequential palette |
| highlight targets | elements | G8.9 | those the insight names |

### Checks on the data

**G0.11** [O] [KT] Before breaking the data down by every variable, try one or two, aggregating the rest away; use three or more only when the insight needs them (then G1.18).
*When:* data with several categorical dimensions. *Why:* charts with fewer breakdowns are much simpler to design and to read, and makers habitually assume every variable must appear. *Except:* none. *Src:* RP3-1 [KT] p. 375–377.
**Good choice:** keep a variable only if the insight names it or the job fails without it, and remove the others by aggregating (totals, averages) rather than by showing part of the data.

**G0.12** [L] [KT] Meaningful-total check: use a pie, stacked chart or treemap only if the values add up to a total someone would quote. Averages, rates, ratios, ratings and discounts fail; show them as a regular bar chart.
*When:* before any breakdown chart. *Why:* slicing or stacking asserts that a whole exists (three regional average donations stack to a meaningless $58). *Except:* none. *Src:* R7-02 [KT] p. 252–253.

**G0.13** [L] [KT] All-parts check: use a pie, stacked chart or treemap only if every part of the total is shown. A filtered, truncated or top-N subset goes in a regular bar chart (several totals: merged bars), unless an "All others" part completes the total.
*When:* the data are filtered or ranked. *Why:* readers take the parts shown for the whole, so fractions and stack heights read wrongly. *Except:* none. *Src:* R7-03 [KT] p. 253–255; R7-22 [KT] p. 271–274; R7-17 p. 249–250.

**G0.14** [L] [KT] Overlap check: confirm no category contains or overlaps another (sports cars within cars). If one does, subtract to make disjoint parts ("Other cars" = cars − sports cars), or show the containment by stacking each parent from its sub-parts. [D-24]
*When:* before a pie, stacked chart or treemap. *Why:* separate slices or segments imply disjoint parts, so every fraction is wrong. *Except:* complex overlaps need judgement (an Euler diagram may serve). *Src:* R7-05 [KT] p. 259–264.

**G0.15** [L] [KT] Sign check: if some parts are positive and others negative, use no pie, treemap or stacked bar; use a regular bar chart or a waterfall (G1.38).
*When:* any breakdown. *Why:* areas cannot be negative, so fractions are misstated, and a mixed stack shows neither the net total nor comparable parts. *Except:* parts all positive or all negative pass. *Src:* R7-04 [KT] p. 255–257.

**G0.16** [L] Keep a known zero distinct from missing, unknown, pending (TBD) and erroneous values from the data to the drawing: never replace a missing value by 0, and never drop a true zero.
*When:* every data set. *Why:* zero is a known value and may itself be the news; missing means unknown. *Except:* a count or sum of events over an empty interval is a true zero (readers'). *Src:* R6-23 p. 208; R9-7 p. 325–327; R7-36 [KT] p. 292; R0-12 p. 19, 33.

**G0.17** [L] Classify every time series before choosing its chart: (a) are enough values known to see its pattern of change; (b) is it regular (one summary value per equal interval) or irregular; (c) for an irregular series, is each value a state or an event? Take (c) from the requester; it cannot be read from the numbers.
*When:* the horizontal dimension is time. *Why:* for time data the right type follows mostly from these properties, and a mismatch misrepresents the data. *Except:* none. *Src:* R6-1 p. 170–171; R6-3 [KT] p. 175–176; R6-8 [KT] p. 188–190.

**G0.18** [L] Classify each categorical variable as nominal, ordinal or grouped nominal (members of named groups the message uses, such as countries by continent), and mark special members (total, average, all others, missing) by their role, not by their name.
*When:* every categorical variable. *Why:* the kind decides sort order (G2.1, G2.2) and palette (G4.5). *Except:* none. *Src:* R2-01 [KT], R2-02 [KT] p. 53–55; R3-5 [KT], R3-8 [KT], R3-11 [KT] p. 117–124; R3-16 [KT] p. 128–130.

**G0.19** [G] [KT] Find what the audience would compare the numbers against, in this order of preference: expectations (targets, budgets, tolerances, forecasts), then averages, then previous periods, then peers. Keep only valid ones (same domain, current); if only doubtful ones exist (another industry, years-old figures), use none. [D-48]
*When:* the audience may lack the background to judge the numbers. *Why:* audiences latch onto comparison values, so a doubtful one misleads more than none. *Except:* none needed when the insight compares the shown items with each other (G8.12). *Src:* R15-7 [KT] p. 405–406, 408; R15-9 [KT] p. 407–408.

**G0.20** [L] [KT] Ask whether the audience judges the values as good or bad and, if so, where the dividing value lies and which side is good. Take both from the requester, since they come from how the audience thinks, not from the data. If people disagree about the threshold, use the agreed one and flag the dispute; do not drop the colouring. [D-1]
*When:* before choosing colours. *Why:* they decide red/green colouring and diverging palettes. *Except:* none. *Src:* R3-13 [KT] p. 125–127; R3-17 [KT] p. 130–131; R15-11 [KT] p. 408–410; R15-10 p. 407–410.

## G1 Choosing the chart type

### Chart types used in this guide

- **Bar chart**: one bar per category from a common zero baseline, vertical (columns) or horizontal.
- **Clustered bar chart**: the bars of several series side by side within each category.
- **Merged bar chart**: side-by-side panels of bars sharing one category axis, each panel with its own scale.
- **Stacked bar / 100 % stacked bar**: parts stacked into one bar per total; the 100 % form scales each bar to 100 %.
- **Stacked area / 100 % stacked area**: stacked bands over time.
- **Wrapped bar chart**: a long sorted bar chart cut into side-by-side columns on one scale.
- **Waterfall**: bars for successive positive and negative parts, each starting where the previous one ended, closed by a total bar.
- **Pie**: slices proportional to the parts of a total. **Donut**: a pie with a hole.
- **Treemap**: rectangles with areas proportional to the parts, optionally nested in groups.
- **Standard line chart**: the points of a regular series joined by straight segments.
- **Dot plot**: dots without joining lines: over time (the known values of a sparse series) or one per category on a value scale.
- **Step chart**: a line that stays level until each change and then jumps.
- **Thin bar chart**: narrow bars at the exact times of irregular events, on a true time scale.
- **Bumps chart**: ranks over time, rank 1 at the top.
- **Arrow chart**: for each category, an arrow from the before value to the after value.
- **Slope chart**: for each category, a line from the before value (left) to the after value (right).
- **Index chart**: series rebased to a common value at a common start (0 % change, 100, $10,000) on one scale.
- **Inset chart**: a small chart linked to the main one that re-plots some values on a magnified scale.
- **Small multiples**: a grid of small charts arranged like a table.
- **Heatmap**: a grid of cells coloured by value. **Heatmap table**: the same with the values printed in the cells.
- **Maps**: choropleth (regions filled by value), coloured dots (one same-size dot per region, coloured by value), bubbles (circles sized by value), bars on regions, tile map (equal squares in rough geographic positions), lines on regions (a small line chart on each region).
- **Combo chart**: different mark types (bars, lines, markers) in one plot.
- **Action dots**: a sorted list of values with thin bars and a coloured dot only where action is needed.

### Principles

**G1.1** [G] [KT] Choose the chart type from several factors at once (the job, the insight, the checks of G0, the audience) using G1.10; never from a one-line mapping such as "time → line chart" or "parts of a total → pie", and never start from the pie.
*When:* every choice of chart type. *Why:* five to ten factors can bear on the choice, and simple mappings give plainly bad charts in many situations. *Except:* none. *Src:* RP2-2 [KT] p. 163–164; R7-01 [KT] p. 233–235; R9-2 [KT] p. 322–323.

**G1.2** [L] [KT] Treat a wrong chart type as an error that confuses or misleads, not as a matter of taste.
*When:* reviewing a chart. *Why:* in several common situations the familiar type misrepresents the data. *Except:* none. *Src:* RP2-3 [KT] p. 162–164; R6-1 p. 170–171.

**G1.3** [L] [KT] Draw from the full everyday vocabulary (about 30 types, listed above) and nothing beyond it.
*When:* choosing a type; scoping a generator. *Why:* too few types lead to misunderstanding, like too few words; the hundreds of other types belong to analysis, data art or specialist fields. *Except:* none. *Src:* RP2-1 [KT] p. 161–162, 164; RP2-6 p. 163.

**G1.4** [G] [KT] For a general audience use a familiar type whenever one can do the job: bar, stacked bar, clustered bar, line and pie, plus the step chart, dot plot and merged bar chart where accuracy calls for them. Keep scatter plots, histograms, cycle plots and log scales for audiences known to be data-savvy.
*When:* choosing a type. *Why:* a type the audience must first learn costs effort. *Except:* G1.5. *Src:* R0-03 p. 26–27, 37; R6-18 [KT] p. 182–184, 187.
**Consider:** the audience's expertise; whether a familiar type can show these data without misrepresenting them (if not, G1.5); the effort of learning a new type against the risk of misreading an inaccurate one.

**G1.5** [G] [KT] When only a less familiar type (dot plot, step, thin bar, bumps, arrow, treemap, stacked area) represents the data accurately, use it rather than a familiar inaccurate one, and teach it: build it up step by step when presenting live; show first the familiar chart that fails and say what it cannot show; or, for a chart read alone, add two or three callouts stating things obvious to an expert.
*When:* the accurate type may be new to the audience. *Why:* familiarity does not excuse misrepresentation, and inexperienced readers misread such charts (the top of a stacked area taken for the top series). *Except:* G1.6. *Src:* RP2-4 p. 162; R6-18 [KT], R6-19 [KT] p. 182–187; R7-31 p. 247, 282–283.

**G1.6** [G] [KT] If the insight would need an unfamiliar or complex chart but fits in a few words, state it as text and keep the chart as backup (appendix, hidden slide).
*When:* a sentence carries the message ("the price fell from $400 in April to $250 in July"). *Why:* the audience gets the point with no chart to learn. *Except:* none. *Src:* R6-20 [KT] p. 190–194.
**Consider:** whether the sentence carries the whole message; whether the audience will want the evidence at hand (then the chart goes to the appendix, not away).

**G1.7** [L] [KT] Never vary chart types across a report, or combine types in one chart, to add variety or interest; make charts engaging by making them obvious (G8).
*When:* sets of charts; combo charts. *Why:* each type changes how the data are perceived, so arbitrary types send unintended signals and hide the insight. *Except:* none. *Src:* RP2-5 p. 162–163; R9-4 p. 322; R11-1 [KT] p. 351–353; R12-10 p. 365; RP6-1 [KT] p. 420, 424.

**G1.8** [G] Choose in two stages: first the group (G1.10), then the type within it; afterwards apply the combo rules and the avoid-list. When the data involve time, consider every time-related group (change between two periods, breakdowns over time, maps over time), not only "data over time". [D-13]
*When:* every choice of chart type. *Why:* the group is usually obvious; the choice within it is the hard part. *Except:* none. *Src:* RP2-7 p. 168; RP2-8 p. 165–167; RP2-9 p. 168.

**G1.9** [G] [KT] Always consider a bar chart, since it can show any quantitative data, but prefer a more specific type when one fits (a line for patterns of change, a pie for fractions of a total, a map for location). When bars are used, the title block and labels must say what the measure and the categories are (readers'). [D-47]
*When:* choosing a type. *Why:* bars are precise but look alike whatever they show; a specific type tells the audience the kind of data before any text is read. *Except:* none. *Src:* R9-1 [KT] p. 320–321, 323; R9-3 p. 321–322.

### The procedure

**G1.10** [G] Choose the type with these steps, in this order (the order is provisional [D-12]); each step names the rules that decide it.
*When:* every chart. *Why:* the book's separate decision trees, merged into one path. *Except:* none. *Src:* RP2-7 p. 168; trees p. 172, 264, 276, 286, 318, 323; RP2-8 p. 165–167.

```
Step 0  Brief: job and insight known (G0.4, G0.5)? If not, make a question-titled set (G0.7).
Step 1  Table or graph:
          only lookup, only export, or no two values comparable  → table alone (G1.12)
          otherwise → graph; add an aligned table if exact values are genuinely needed
          or for readers with impairments (G1.14, G1.15)
Step 2  Variables: keep only those the insight needs (G0.11).
          three or more breakdown variables still needed → small multiples (G1.18),
          each panel's type chosen by steps 3–8
Step 3  Location: does the message depend on where the values are more than on reading them?
          no → step 4;  both matter → map + bar or line chart side by side;
          yes → maps (G1.25 to G1.33)
Step 4  Parts of a total?
          run the four checks (G0.12 to G0.15); a failure sends the chart
          where that check says (bar chart, merged bars, waterfall, or separated parts)
          one total → G1.35 to G1.40
          several totals → G1.41, G1.42
          a total changing over time → G1.43 to G1.45
Step 5  Exactly two periods per category → arrow chart with absolute and % panels
          (G1.46, G1.47)
Step 6  Values over three or more periods (G1.48):
          too few known values → dot plot (G1.49)
          regular series: rank → bumps; individual periods → bars; otherwise → line
          (G1.50, G1.51, G1.52)
          irregular states → step chart (G1.53)
          irregular events: individual values → thin bars; otherwise aggregate to regular
          intervals and return to "regular series" (G1.54)
Step 7  Values across categories: one series → bar; several series in one measure → clustered
          bars; several measures → merged bars (G1.56); a scale that need not include
          zero → dots (G1.57); never a line across unordered categories (G1.58)
Step 8  Series with different roles (pattern, individual periods, reference) → combo (G1.60);
          otherwise one mark type, series told apart by colour (G1.59)
Step 9  Check the result against the avoid-list and substitute (G1.61). If the type is new
          to the audience, teach it or state the insight as text (G1.5, G1.6).
```

### Table or graph

**G1.11** [G] [KT] Show data as a graph by default, including when the job is to show a pattern such as a cycle.
*When:* every data set. *Why:* compared with a table, a graph makes insights obvious and more likely to be noticed, is faster to read, and shows the kind of data and the big picture. *Except:* G1.12. *Src:* R10-1 [KT] p. 329–332, 341; R0-13 p. 19.

**G1.12** [G] [KT] Show a table without a graph only when its sole use is looking up individual values, its sole use is importing into another program, or none of its values are comparable with one another (then left-align the numbers, G7.41).
*When:* a table is considered. *Why:* a graph invites comparison, which is wrong for unrelated key metrics and useless for pure lookup. *Except:* none. *Src:* R10-11 [KT] p. 341–343.

**G1.13** [G] Do not switch to a table because the data have many values, details and totals, outliers, several measures, or more than three variables. Use instead a graph (graphs carry thousands of values), stacked bars or areas, an inset chart (G2.26), a merged or index chart (G2.31), or small multiples (G1.18).
*When:* a table is proposed for one of these reasons. *Why:* each has a graphical answer that shows more, faster. *Except:* none. *Src:* R10-2 p. 332–333; R10-3 p. 333; R10-4 p. 333–334; R10-5 p. 334; R10-6 p. 334–336; RP3-2 [KT] p. 369–371, 377.

**G1.14** [G] [KT] When exact values are genuinely needed (a legal requirement; close values whose small differences matter), keep the graph and put the numbers in a table beside or below it, its columns aligned under the graph's categories where possible (G7.46); do not print a value on every mark.
*When:* exact values are required; this is rarer than makers assume. *Why:* the graph shows the pattern, the table the precision, and long numbers do not clutter the graph. *Except:* none. *Src:* R10-7 [KT] p. 336–337, 341; R2-31 [KT] p. 92–94; R5-12 p. 152–153.

**G1.15** [G] If the audience includes people with visual, cognitive or physical impairments, provide the data as a table as well as the graph.
*When:* such readers are expected. *Why:* some readers can use only the table. *Except:* none. *Src:* R10-8 p. 337.

**G1.16** [G] If an audience insists on tables: (1) have them look for insights first in the table, then in the graph; (2) if they still want tables, add graphs beside them; (3) if they refuse graphs, build graphics into the table (G7.47).
*When:* an audience rejects graphs. *Why:* the demonstration usually convinces; the later steps keep some graphical help. *Except:* none. *Src:* R10-9 p. 338–339.

**G1.17** [O] [KT] Design a table from the message, as a graph is designed: choose its breakdown, measures ($ or %), shares of total, periods, totals and rounding for the insight.
*When:* every table. *Why:* a table is not a neutral dump of numbers; each choice shows some insights and hides others. *Except:* none. *Src:* R10-10 [KT] p. 339–341.
**Good choice:** rows, columns, measures and rounding that put the numbers the insight compares next to each other, at the precision the audience needs.

### Many variables

**G1.18** [G] [KT] When the insight needs three or more breakdown variables, use small multiples. Generate several arrangements across the range (many panels with one series each; intermediate ones with a few series per panel; one chart holding everything), render them, and keep the one that best carries the insight, usually an intermediate one. [D-38]
*When:* data with more than three variables. *Why:* a grid may take 10–15 seconds to work out but is far quicker than a table, and no general rule picks the arrangement; it depends on the comparisons the insight needs. *Except:* lookup use (G1.12). *Src:* RP3-2 [KT], RP3-3 [KT] p. 369–377.

**G1.19** [G] (fig.) Put the series the insight compares directly in the same panel, and facet on the variable whose comparison is secondary.
*When:* choosing panels and series. *Why:* comparisons across distant panels are hard, within a panel easy. *Except:* none. *Src:* RP3-4 (fig.) p. 372–375.

**G1.20** [G] [KT] Do not show a general audience one chart of many overlapping series. Four or five lines in one plot are usually fine; more only if they rarely cross and each can be labelled directly. When a line chart is too busy, split it into a stack of small line charts (up to about 20–30); with more series than that, use a heatmap (G7.35). [D-28]
*When:* many series. *Why:* frequent crossings make a tangle; stacks and heatmaps lose some direct comparison but stay readable. *Except:* none. *Src:* RP3-5 p. 372–373; R6-35 p. 219; R6-36 [KT] p. 219–222.

**G1.21** [G] Any chart type may fill the panels of small multiples (lines, bars, scatter plots).
*When:* small multiples. *Why:* the arrangement is independent of the mark. *Except:* none. *Src:* RP3-6 p. 377.

**G1.22** [L] [KT] Show all the data at once; do not add interactive filters. Use a filter only when the chart's main use is looking up individual values, or when there is far too much data for any arrangement (about 100 categories rather than 6). [D-53]
*When:* interactive output. *Why:* with one slice visible, every comparison between slices means clicking and memorising. *Except:* those two cases. *Src:* R13-1 [KT], R13-2 [KT] p. 380–383.

**G1.23** [L] [KT] Animate a chart only when its main purpose is to catch attention, or when only a few marks move a little; never morph one whole chart into another. Otherwise show all the states in one static chart. [D-53]
*When:* animation is proposed. *Why:* viewers must remember values that have left the screen. *Except:* those two cases. *Src:* R14-1 [KT] p. 384–387; R14-2 [KT] p. 386–387; R14-3 p. 386.

### Maps

**G1.24** [G] [KT] Do not choose a map because the data contain places, postcodes or other locations.
*When:* data with a location field. *Why:* maps have real costs and are only sometimes the best form. *Except:* none. *Src:* R8-1 [KT] p. 300, 304.

**G1.25** [G] [KT] Treat every map as a low-precision chart. Use one only when where the values are (region size, neighbours, spatial clusters) matters more to the message than reading and comparing them; otherwise use a bar or line chart. When both matter, show the map and a bar or line chart side by side.
*When:* data with locations. *Why:* sizes and shades on a map can only be roughly estimated; one chart cannot give both location and precision. *Except:* none. *Src:* R8-2 [KT] p. 301–302, 304; R8-3 [KT] p. 302–304; R8-4 p. 304.

**G1.26** [G] [KT] For values at locations that change over time, put a small line chart on each region when location matters more; when precise comparison of the patterns matters more, drop the map and use one line chart with a line per region.
*When:* location plus time. *Why:* small charts on a map are the best available option but are not arranged for comparison. *Except:* none. *Src:* R8-13 [KT] p. 315–316; R8-14 [KT] p. 316.

**G1.27** [L] [KT] Do not fill regions by value (choropleth) unless all regions are about the same size and shape.
*When:* choosing a map type. *Why:* large regions grab attention whatever their value, and small ones become invisible even when their values are the largest. *Except:* regions of similar size and shape. *Src:* R8-5 [KT] p. 305–308.

**G1.28** [G] [KT] Treat coloured dots (one dot per region, all the same size, the value shown by colour) as a generally safe map type.
*When:* colour is to carry the value. *Why:* region area no longer affects perception. *Except:* none. *Src:* R8-6 [KT] p. 309.

**G1.29** [G] [KT] Use a tile map only when a general sense of location is enough and the real geography does not matter.
*When:* choosing a map type. *Why:* tiles remove the size bias but badly distort geography (implied borders that do not exist). *Except:* none. *Src:* R8-8 [KT] p. 310–311.

**G1.30** [G] [KT] Treat bubbles (circles sized by value) as generally safe, but not when some values are very small.
*When:* size is to carry the value. *Why:* a tiny bubble is missed or mistaken for a town marker, so the region looks empty. *Except:* none. *Src:* R8-9 [KT] p. 312–313.

**G1.31** [G] [KT] Use bars on regions only when a little more precision than bubbles or colour is wanted, and not when some values are very small, the map has many regions, or spatial patterns must be seen.
*When:* choosing a map type. *Why:* bars remain far less precise than a bar chart, look busier than bubbles or dots, and a very short bar looks like no value. *Except:* none. *Src:* R8-11 [KT] p. 313–314; R8-12 [KT] p. 314.

**G1.32** [G] (fig.) When all values lie in a narrow range and their small differences are the point, use a colour-coded map type, not bubbles or bars.
*When:* choosing a map type. *Why:* sizes and lengths start from zero, so small differences between large values vanish, while a colour scale can span the data's own range (readers'). *Except:* none. *Src:* R8-15 (fig.) p. 318.

**G1.33** [G] Choose freely among the map types that survive G1.27 to G1.32; the provisional order is coloured dots, bubbles, bars on regions, tiles, choropleth [D-36]. In a close call, lean towards a map when engagement matters more than top precision, when the audience knows the geography well, or when announcing that the values belong to places helps.
*When:* several map types remain. *Why:* the book leaves the final choice to preference. *Except:* none. *Src:* R8-16 p. 318; R8-17 p. 318–319.

**G1.34** [G] Apply the map rules to any two-dimensional space (a floor plan, a circuit board), not only to geography.
*When:* values placed in a physical layout. *Why:* the same trade-off between location and precision holds. *Except:* none. *Src:* R8-18 p. 319.

### Parts of a total

**G1.35** [G] [KT] If the message needs a precise comparison of parts (A gave more than B when they are close), use a regular bar chart.
*When:* one total; parts that may differ by a few per cent. *Why:* only bars share a common baseline; slices, segments, rectangles and waterfall bars allow only rough comparison. *Except:* none. *Src:* R7-06 [KT] p. 236–237, 258.

**G1.36** [G] [KT] If the message is what fraction of the total each part is, use a pie, provided it has at most 6–7 parts [D-23] and the checks of G0 pass. A bar chart with a "% of total" scale is a weaker substitute; a stacked bar lies between the two.
*When:* one total; fractions matter most. *Why:* a pie shows familiar fractions (½, ⅓, ¼) directly, announces that the parts form a whole, and steers readers to fractions. *Except:* G1.40. *Src:* R7-07 [KT] p. 238–242.

**G1.37** [G] [KT] If the message is a subtotal of several parts as an amount ("A + B exceed $300K"), use a stacked bar with those parts adjacent at the base and emphasised; if the subtotal is a fraction ("A + B are almost half"), use a pie.
*When:* one total; a subtotal is the point. *Why:* adjacent segments add up visually; in a pie or bar chart the reader must add in their head. *Except:* none. *Src:* R7-08 [KT] p. 242–244.

**G1.38** [G] [KT] If parts mix positive and negative values, use a waterfall when running subtotals are the message and no precise part comparison is needed; otherwise use a regular bar chart, optionally closed by a grey Total bar (G7.3). [D-43]
*When:* one total with mixed signs. *Why:* a waterfall shows the running total after each step but its bars start at different heights. *Except:* none. *Src:* R7-11 [KT] p. 257–258; R7-12 p. 234, 264.

**G1.39** [G] [KT] With more than 6–7 parts [D-23]: first lump the parts unimportant to the message into one "All others" part (G4.21). If that is not possible, use a bar chart (a wrapped bar chart for very many, about 100) when fractions do not matter, or a treemap when fractions or subtotals must be featured; prefer a pie whenever the parts fit in one.
*When:* one total with many parts. *Why:* pies and stacked bars clutter beyond 6–7 parts; bars stay readable but do not show part-of-whole; treemaps handle many parts but are less familiar and hard to label. *Except:* none. *Src:* R7-13 [KT] p. 244, 251, 264; R7-14 [KT] p. 244–245, 251; R7-15 [KT] p. 245–249, 251; R7-17 p. 249–250.

**G1.40** [G] [KT] If a pie could provoke a "pies are bad practice" objection that would derail the discussion, use another type even where a pie would be best (provisional: a 100 % stacked bar, readers') [D-39].
*When:* the audience is known or likely to object to pies. *Why:* many practitioners still reject pies outright. *Except:* none. *Src:* R7-40 [KT] p. 235, 297–298.

**G1.41** [G] [KT] For several totals: to compare parts across totals, use a clustered bar chart; to compare the totals and show roughly which parts dominate, use multiple stacked bars; use multiple pies only when the message is entirely about the fractions within each total and needs at most very rough comparison between totals, which is rare. The checks of G0 apply to every total (provisional) [D-25] [D-39].
*When:* two or more totals. *Why:* clustered bars compare parts precisely but hide totals; stacked bars the reverse; pies block almost every comparison between totals. *Except:* G1.42. *Src:* R7-19 [KT] p. 267–268; R7-20 [KT] p. 268–269; R7-21 [KT] p. 269–271.

**G1.42** [G] [KT] When not all parts of the totals are shown (only the top three departments), or the totals are in different measures, use merged bars; clustered bars are acceptable but second best for the first case, and never for the second.
*When:* several totals with missing parts or mixed units. *Why:* stacks and pies imply completeness, and one shared scale for different units produces nonsense ("160 of what?"). *Except:* none. *Src:* R7-22 [KT] p. 271–274; R7-23 [KT] p. 274–275.

**G1.43** [G] When a total changes over time and the message compares its parts: if the total does not matter, draw a standard line chart with one line per part; if it does, add the total as a line in its own panel above the parts, sharing the time axis.
*When:* a breakdown over time, parts compared. *Why:* lines compare parts well but show no total; on a shared scale the total flattens the parts. *Except:* none. *Src:* R7-25 p. 278–280, 286; R7-26 p. 280–281; R7-27 p. 281.

**G1.44** [G] When a total changes over time and the message is about fractions or rough subtotals: if the total's change matters, use an absolute stacked area or stacked bar chart; if only the fractions' change matters, use a 100 % stacked area or bar chart.
*When:* a breakdown over time, fractions featured. *Why:* the absolute form shows the total and rough fractions; the 100 % form shows fraction trends more clearly but loses the total. Only stacked forms show rough subtotals. *Except:* none. *Src:* R7-29 p. 282–283; R7-30 p. 283–284.

**G1.45** [G] Draw stacked forms over time as areas when the message is about overall patterns of change, and as bars when it is about particular periods.
*When:* a stacked chart over time. *Why:* areas carry the eye along the pattern; bars separate the periods. The choice here is forgiving, unlike the checks of G0. *Except:* none. *Src:* R7-28 p. 282–284; R7-25 p. 278–280.

### Change between two periods

**G1.46** [G] [KT] To show each category's change between two periods, use an arrow chart. Never use clustered bars; use a slope chart only in rare cases such as changes of rank, and even then an arrow chart usually works.
*When:* each category has a before and an after value. *Why:* clustered bars show pairs without any sense of change, and slope steepness shows only absolute change while labels collide; arrow charts are misread less and hold many categories. *Except:* rank changes (slope chart). *Src:* R6-14 p. 200–201; R6-15 [KT] p. 202–204; R6-16 p. 201–203; R12-1 [KT] p. 355.

**G1.47** [L] [KT] Never show absolute changes between two periods without the relative (%) changes: put an absolute panel and a % panel side by side, sharing the category rows. [D-40]
*When:* any two-period comparison. *Why:* most audiences confuse the two and name the wrong category as the most improved. *Except:* a message purely about relative change needs only the % panel. *Src:* R6-17 [KT] p. 202–204.

### Values over time

**G1.48** [G] Use the time tree of G1.10 step 6 for series of more than two periods that are not the breakdown of a total and not tied to places.
*When:* time series. *Why:* two periods, breakdowns and maps have their own procedures (steps 3–5). *Except:* none. *Src:* R6-13 p. 197–199.

**G1.49** [G] [KT] If too few values are known to see the pattern of change, draw a dot plot: the known values as dots, no lines, on an axis that covers the whole period so the empty stretches show (provisional test: at least half the slots known and a run of three consecutive known values, readers') [D-27].
*When:* a sparse series (the book's case: 6 of 31 days). *Why:* joining the known points invents a pattern; many different complete series pass through the same points. *Except:* one or two missing values call for gaps (G7.9). *Src:* R6-2 [KT] p. 172–174, 176; R0-14 p. 20.

**G1.50** [L] [KT] Use a standard line chart only for a regular series, one summary value per interval; if an interval holds several raw values, aggregate them first (total, mean, median).
*When:* a line is considered for time data. *Why:* a segment reads as the change from one interval to the next, which is valid only when each interval has one value. *Except:* none. *Src:* R6-3 [KT] p. 175–176.

**G1.51** [G] [KT] For a regular series, default to a line; use bars when the message is about individual periods ("October was 20 % below September"), and then only if each bar stands for one regular interval.
*When:* a regular series. *Why:* lines show patterns of change (a quarterly cycle) more clearly; bars make readers treat each period as a separate item. *Except:* the difference is subtle; a line is not wrong for an individual-period message. *Src:* R6-4 [KT] p. 177, 179; R6-5 [KT] p. 178–179; R6-6 [KT] p. 177, 179.

**G1.52** [G] [KT] When the message is about changes in rank (overtaking, losing ground), consider a bumps chart, only for regular series. [D-41]
*When:* rank messages. *Why:* a line of the underlying quantity can suggest the opposite (a cumulative medal count rises while the rank falls). *Except:* an unfamiliar audience (G1.5). *Src:* R6-7 [KT] p. 179–182.

**G1.53** [G] [KT] Draw an irregular series of states as a step chart; never as a standard line chart.
*When:* values at irregular times, each holding until the next (price, stock level, lending rate, balance, highest bid). *Why:* sloped segments falsely suggest gradual drift and no value in between (a price shown as $375 in mid-April when it was $400 all month). *Except:* none. *Src:* R6-8 [KT] p. 188–190; R6-9 [KT] p. 189–190.

**G1.54** [G] [KT] For irregular events (transactions, visits), aggregate them into regular intervals and draw a standard line when the message is the overall pattern; when each event matters in itself, use a thin bar chart. Never draw raw events as a standard line.
*When:* irregular events. *Why:* a line through raw events can even reverse the apparent trend; thin bars show each event at its instant with nothing between. *Except:* none. *Src:* R6-10 p. 195–196; R6-11 [KT] p. 194–197; R6-12 [KT] p. 195, 197.

**G1.55** [G] Do not avoid a standard line chart of regular summary values on the ground that its straight segments hide what happened between the points.
*When:* regular series. *Why:* such a chart neither hides information nor misrepresents the data. *Except:* none. *Src:* R6-43 p. 229–230.

### Values across categories

**G1.56** [G] For values across categories, choose by series and measures: one series → bar chart; several series of one measure → clustered bar chart; series in different measures → merged bar chart, never a dual axis.
*When:* categorical comparisons. *Why:* one scale per measure; clusters for comparable series. *Except:* none. *Src:* R9-11 p. 323, 334.

**G1.57** [G] (fig.) Use dots instead of bars when the value scale should not start at zero (a theoretical minimum such as 300 for credit scores, or truncation allowed by G2.8) [D-30].
*When:* categorical values on a non-zero scale. *Why:* a bar's length must be proportional to its value (G2.6); a dot's position need not start from zero. *Except:* none. *Src:* R2-24 [KT] p. 83, 85, 88; figures p. 85–87.

**G1.58** [L] [KT] Join categories with a line only when they have a natural order and the audience is data-savvy (or for analysis); never join unordered categories (departments) with a line.
*When:* a non-time horizontal scale. *Why:* many readers take any line for change over time, and a line implies order and links between neighbours. *Except:* parallel-coordinates plots in specialist analysis. *Src:* R6-41 [KT] p. 226–229; R6-42 p. 228–229.

### Combo charts

**G1.59** [L] [KT] Tell series apart by colour (a dark and a light blue for expenses and budget) or occasionally by marker shape, never by giving them different chart types.
*When:* several series in one plot. *Why:* colour and shape distinguish without changing how the data are perceived. *Except:* G1.60. *Src:* R11-2 [KT] p. 353.

**G1.60** [G] [KT] Combine mark types only when the message needs series perceived differently: a line for a series whose pattern of change matters, bars for one whose individual periods matter, a flat marker across each bar for a reference (budget, target). Put all of them on one zero-based scale (fig.); different measures go to a merged or index chart instead.
*When:* series with different roles. *Why:* the mark type decides what readers attend to; two scales would make a dual-axis chart. *Except:* none. *Src:* R11-3 [KT] p. 352–354; R11-4 p. 353; R11-5 (fig.) p. 351–353.

### Types to avoid

**G1.61** [L] [KT] Do not use the types in the left column in everyday charts; use the replacement.
*When:* after choosing, and whenever one of these is requested. *Why:* each is hard to read precisely or misleads; the reasons are in the table. *Except:* as listed. *Src:* R12-1 [KT] p. 355, 365; R12-2 [KT] p. 356–357; R12-3 [KT] p. 357–358; R12-4 [KT] p. 358–359; R12-5 [KT] p. 360; R12-6 [KT] p. 360–363; R12-7 [KT] p. 363–364; R12-8 [KT] p. 364; R2-14 [KT] p. 68–74; R2-18 p. 74–76; R2-19 [KT] p. 76–78; R6-16 p. 201–203; R6-39 [KT] p. 223–226; R7-39 [KT] p. 295–297; R8-5 [KT] p. 305–308; RP6-1 [KT] p. 420; RP7-4 p. 429–430.

| Avoid | Use instead | Why | Exception |
|---|---|---|---|
| dual-axis chart of two variables | merged chart (values matter) or index chart (patterns of change) | suggests false crossings and slopes | exact conversions (G2.33) |
| broken value scale | inset chart (G2.26) | the break goes unnoticed or misunderstood | none |
| log scale | linear scale with an inset | most readers cannot read it | data-savvy audience, analysis |
| smoothed line through the data | straight segments | overshoots the data; smoothing is arbitrary | trend lines, bumps charts (G7.17) |
| donut | pie | loses the centre angles; slightly worse to read | none |
| slope chart; clustered bars for two periods | arrow chart | steepness shows only absolute change | rank changes (G1.46) |
| choropleth | coloured dots, bubbles, bars on regions | size of region dominates | regions of similar size (G1.27) |
| word cloud | horizontal bars of word counts, sorted descending | font sizes cannot be compared; long words look frequent; rotated words | decoration (an advertisement) |
| packed bubbles | sorted horizontal bar chart | sizes hard to compare; position implies false relations | none |
| bullet graphs | action dots (G7.58) | busy; metrics needing attention hard to spot | none |
| funnel chart | stage counts as bars plus % change between stages (G7.59) | layers allow only rough comparison | none |
| radar (spider) chart | heatmap table (G7.36) | shapes do not show which item is best or its strengths | circular data (wind rose) |
| 3-D effects on a 2-D chart | the flat chart | perspective distorts values | none |
| third variable on a z-axis | 2-D, e.g. as the series of a clustered bar chart | parallax; front bars hide back ones | trained scientific users |
| non-spatial data as 3-D objects in VR or AR | a flat chart | novelty is the only gain | none |
| eye candy: drop shadows, gradients, dark backgrounds, bright colours | the plain style (G3.9) | adds effort and hides insights | none |

**G1.62** [L] Reserve the avoided types for data art, infographics, advertisements and similar pieces where fast, clear communication is not the aim; never use them in everyday reporting.
*When:* an avoided type is wanted for effect. *Why:* they trade clarity for attention. *Except:* none. *Src:* R12-9 p. 356.

## G2 Scales

### Categorical scales

**G2.1** [G] [KT] Sort nominal categories by value, largest first; smallest first when the small values are the point. Keep a catch-all ("Others", "All others") last and categories whose value is missing last (fig.). With several series, order by the first or primary series (fig.). [D-46]
*When:* categories without a built-in order. *Why:* the shape of the data shows, the extremes are found at once, and near-equal values sit together where they are easy to compare. *Except:* ordinal categories and time (G2.2); the panels of a merged chart share one order. *Src:* R2-01 [KT] p. 54–55; R9-9 (fig.) p. 302–359; R7-17 (fig.) p. 249–250.

**G2.2** [L] [KT] Keep ordinal categories, time periods and process stages in their built-in order, never sorted by value.
*When:* schooling level, satisfaction level, rank, months, funnel stages. *Why:* the audience expects the natural sequence. *Except:* none. *Src:* R2-02 [KT] p. 53–55; R9-9 (fig.) p. 351, 360.

**G2.3** [G] [KT] Give the category axis no title when the names say what they are (fruit names need no "Type of fruit"); give it a short one, centred beneath the labels, when they do not (Low / Medium / High → "Threat level"). [D-47]
*When:* every categorical axis. *Why:* an obvious title is clutter; a missing one leaves the categories ambiguous. *Except:* none. *Src:* R2-03 [KT] p. 55–56.
**Consider:** whether a reader who sees only the names knows what kind of thing they are; whether the descriptive line of the title block already names the breakdown ("by Region") [D-47].

**G2.4** [G] [KT] Keep category labels horizontal and centred on their band. When they do not fit under vertical bars, wrap each onto at most two lines; if they still do not fit, turn the chart 90° into horizontal bars with the names right-aligned at the left. Never rotate the text. Provisional switch to horizontal bars: a label would need three lines, or there are more than about 12 categories (readers') [D-34].
*When:* categorical axes. *Why:* vertical and diagonal text is slow and uncomfortable to read. *Except:* a time chart is never turned (G2.36). *Src:* R2-04 [KT] p. 56; R5-26 (fig.) p. 158; R9-10 (fig.) p. 302–359; R0-06 p. 37.
**Consider:** the width of the longest name against the band; the number of categories; whether the horizontal dimension is time (never turned, G2.36). Only the orientation is decided here: horizontal text is locked (G6.6).

### Zero and the range

**G2.5** [G] [KT] Include zero on every value scale by default.
*When:* every quantitative scale. *Why:* the distance from zero is part of what the audience reads. *Except:* G2.8, G2.10, and theoretical minima (G2.11). *Src:* R2-22 [KT] p. 84, 88.
**Consider:** whether the marks encode length or area (then zero is locked, G2.6); whether zero means "none" (G2.10); whether the distance from zero matters to the job (G2.8); whether small differences in a narrow band must be seen too (G2.7).

**G2.6** [L] [KT] Start the main scale at zero for every mark whose length or area encodes the value: bars of every kind, stacked bars, stacked areas and waterfalls. Never truncate them.
*When:* those marks. *Why:* a bar twice as long must mean twice the value; on a scale starting at 500,000, a value that is 77 % of another looks like half of it. *Except:* an inset chart may use a truncated scale (G2.7). *Src:* R2-24 [KT] p. 83, 85, 88; R9-5 p. 324, 327.

**G2.7** [G] [KT] When all values lie in a narrow band far from zero (the gap from zero to the nearest value is at least the spread) and small differences among them must be shown, keep zero in the main chart and add an inset with a truncated scale (G2.27). [D-10] [D-11]
*When:* bars, lines and dots alike. *Why:* the main chart keeps the big picture (the values are stable); the inset shows the detail. *Except:* G2.8. *Src:* R2-23 [KT] p. 83–85, 88; R9-6 p. 324–325.
**Consider:** whether the brief says the small differences are part of the message (the narrow-band test only raises the question [D-11]); the inset costs the reader a second scale.

**G2.8** [G] [KT] Truncate the main scale (no zero and no inset) only when the distance from the values to zero is truly irrelevant to the chart's purpose, which is rare, and only for lines and dots (class scores 6.5–8.0 out of 10; daily highs 20–35 °C; client ratings 7–10). [D-10]
*When:* a scale far from zero. *Why:* truncation magnifies differences, which is right only when nobody needs the distance to zero. *Except:* never for length or area marks (G2.6). *Src:* R2-24 [KT] p. 83, 85, 88.

**G2.9** [G] Do not start a scale close to zero but not at zero (counts starting at 2): if the lowest value is nearer zero than it is to the highest value, start at zero.
*When:* all-positive data. *Why:* it looks odd and gains almost no room for the data. *Except:* a theoretical minimum (G2.11). *Src:* R2-25 p. 83, 90.

**G2.10** [G] When zero does not mean "none" (°C, rating anchors, arbitrary index levels), extending the scale to zero is optional; avoid length and area marks, and consider naming meaningful levels beside the scale ("Neutral" … "Very satisfied"; "Take action / Expected / Best case", in colour). [D-10]
*When:* measures that are not on a ratio scale. *Why:* an arbitrary zero adds nothing, and lengths measured from it mean nothing. *Except:* none. *Src:* R2-36 p. 85, 88.

**G2.11** [G] [KT] When all values are positive and zero is not required, set the lower limit at the theoretical minimum if there is one below the data (credit scores: 300); otherwise 10–20 % of the spread below the lowest value, snapped down to a multiple of the step, never below the theoretical minimum or below 0.
*When:* a truncated scale, or an inset. *Why:* room below the data without wasting the range. *Except:* G2.9. *Src:* R2-26 [KT] p. 89–92.

**G2.12** [G] [KT] Set the upper limit 10–20 % of (highest value − lower limit) above the highest value, snapped up to a multiple of the step; if no multiple falls in that band, take the nearest one that leaves at least 5 % (readers') [D-16].
*When:* all-positive data. *Why:* values touching the top edge look cut off. *Except:* G2.13; an inset may end at its top gridline. *Src:* R2-27 [KT] p. 90–92.

**G2.13** [L] [KT] Never extend a scale past a theoretical minimum or maximum (a percentage of employees stops at 100 %), even if the data then touch the edge.
*When:* bounded measures. *Why:* labels beyond the bound show impossible values. *Except:* none. *Src:* R2-28 [KT] p. 90–92.

**G2.14** [G] [KT] All values negative: mirror G2.11 and G2.12 (the upper limit is zero, the theoretical maximum, or 10–20 % of the spread above the highest value; room goes below the lowest). Mixed signs: add 10–20 % of the spread above the highest and below the lowest value, clipped at theoretical limits.
*When:* negative or mixed data. *Why:* the same breathing room at every free end. *Except:* G2.13. *Src:* R2-29 [KT], R2-30 [KT] p. 91–92.

### Intervals and value labels

**G2.15** [L] [KT] Use a scale step of 1, 2 or 5 × 10ⁿ; 2.5 × 10ⁿ is acceptable but less intuitive; never a step based on 3, 4, 6, 7, 8 or 9.
*When:* every linear value scale. *Why:* values between gridlines are easiest to estimate when the step divides 10 cleanly. *Except:* units the audience counts otherwise (dozens; 45° and 90° angles). *Src:* R2-09 [KT] p. 61–63.

**G2.16** [L] [KT] Make every interval of a linear scale the same size, with label positions linear in value.
*When:* every linear scale. *Why:* unequal steps at equal spacing distort the shape of every line and bar. *Except:* none. *Src:* R2-10 [KT] p. 63–64.

**G2.17** [G] [KT] Use just enough intervals for the precision the job needs: few and wide by default; more when the message depends on a threshold, so that it falls on a gridline ("North exceeded $12M" → steps of 2 million). Never crowd the scale; if that much precision is needed, add a table (G1.14). Provisional aim: about five intervals and at most about eight labels (readers') [D-34].
*When:* choosing the step. *Why:* a cleaner chart, and high precision is seldom needed. *Except:* none. *Src:* R2-33 [KT] p. 94–98; R2-34 p. 97.

**G2.18** [G] [KT] Do not put a value label on every mark. When the chart has fewer values than its scale would have labels (4 bars against 7 scale labels), label the values directly and remove the value scale and its gridlines, keeping the baseline; otherwise keep the scale and add no value labels.
*When:* every chart with a value scale. *Why:* choose whichever option puts fewer numbers on the chart. *Except:* labels on selected marks as a highlight (G8.9); pies (G7.27). *Src:* R2-31 [KT], R2-32 [KT] p. 92–94.

### Unit phrase and scale labels

**G2.19** [L] [KT] Write the unit phrase horizontally beside the value scale: on a vertical scale, on the top label's line after the top value ("25 million inhabitants"), with the top gridline starting just after the text; on a horizontal scale, centred below the scale labels. Never rotate it and never move it into the title. [D-20] On a horizontal scale the labels are bare numbers and the text below is the scale's title, the quantity with its unit in parentheses ("Weirdness Score (on 10)", "Donations ($000s)"), as the book writes such titles and table headers. Percentages are the exception (owner, 1 Oct 2026): on a horizontal % scale every label carries "%", as G2.24 asks ("0%, 20% … 100%"), and the title below names the quantity without "(%)" ("Respondents using the feature") [D-59].
*When:* every value scale. *Why:* horizontal text is easier to read, leaves room for a precise unit, and sits next to the scale it describes. *Except:* G2.20. *Src:* R2-07 [KT] p. 60–61, 99; R0-06 p. 37; horizontal scale titles (fig.) p. 57, 85; money units in titles and headers (fig.) p. 100, 200–201, 278, 342.

**G2.20** [G] (fig.) If the unit phrase would collide with data near the top, or leave less than about 15 % of the top gridline (readers'), set it above the scale-label column instead, right-aligned, wrapped onto two lines if needed. [D-20] Only the description moves: the number keeps its currency, magnitude and suffix on the top label ("$20 million", "0.25 lbs.", "35°C"), for a number never parts from its currency or magnitude (owner, 30 Sep 2026). This concerns vertical scales only: the unit runs on from the top label only there ("50 accidents per month", "8.0 on 10", "35°C"), while a horizontal scale always carries it in its title below (G2.19).
*When:* crowded tops of vertical scales. *Why:* the phrase must stay horizontal and next to its scale without covering data. *Except:* none. *Src:* R2-07 (fig.) p. 89–92; vertical scales only (fig.) p. 57, 85.

**G2.21** [L] [KT] Make the unit phrase specific enough to allow one reading: what is counted and which subset (successful or attempted), the rate or period (per month, daily average), and whether it is cumulative ("successful transactions per month", not "transactions"). Four to six words is normal.
*When:* every unit phrase. *Why:* a vague unit allows several readings. *Except:* none. *Src:* R2-11 [KT] p. 65–66.

**G2.22** [G] [KT] On a scale of very large or very small values, hide the trailing (or leading) zeros and state the magnitude once, in the unit phrase ("24 million inhabitants" above bare 20, 16, 12 …); likewise thousandths or millionths. The trigger and the wording follow G6.11 and G2.23. [D-4] [D-3]
*When:* scales whose labels would carry long runs of zeros. *Why:* less clutter, and nobody has to count zeros. *Except:* none. *Src:* R2-05 [KT] p. 59–60.

**G2.23** [G] Spell the magnitude out ("thousand", "million", "billion") unless the audience's own convention (K, M, 000s) is known. [D-4]
*When:* naming a magnitude. *Why:* "M" can be read as thousands (Latin *mille*) or as millions. *Except:* a known audience convention. *Src:* R2-06 p. 60.

**G2.24** [L] [KT] Format the scale labels as one set: every label, zero included, has the same number of decimals (0.00, 0.05, 0.10, never 0, 0.05, 0.1), namely those needed to write the step exactly; percentage labels all carry "%", on horizontal scales too (G2.19) [D-59]; positive labels on index and difference scales carry a plus sign ("+20%", "0%", "−20%"). Provisional: the currency symbol and the magnitude appear only in the top label's unit phrase, and the other labels are bare numbers [D-3].
*When:* every value scale. *Why:* a set formatted alike reads as one scale; mixed formats look like different units. *Except:* none. *Src:* R2-21 [KT] p. 81; R5-17 (fig.) p. 158, 165–167.

**G2.25** [L] (fig.) Right-align the labels of a vertical value scale in one column just left of the plot, each centred vertically on its gridline; centre the labels of a horizontal scale under their gridlines.
*When:* every value scale. *Why:* tabular digits line up only when their right edges do. *Except:* none. *Src:* R5-18 (fig.) p. 149, 153, 167; figures p. 46–111.

**G2.25s** [S] (fig.) Set the scale-label column about 0.9 em left of the plot.
*When:* as G2.25. *Src:* as G2.25.

### Outliers and narrow ranges: inset charts

**G2.26** [G] [KT] When one or a few outliers flatten the other values and those values must be compared precisely, keep the full linear scale from zero and add an inset that re-plots the small values on their own zero-based scale. If the only message is that the outlier is far bigger, keep the plain chart. Never break the scale, and never switch to a log scale to cope. [D-11]
*When:* outliers. *Why:* a break may go unnoticed (451 against 16 then looks about 20 % larger, not 28 times) or be misunderstood; an inset adds detail without distortion. *Except:* none. *Src:* R2-20 [KT] p. 76–80; R2-18 p. 74–76, 99; R0-15 p. 20; R10-4 p. 333–334.

**G2.27** [L] (fig.) Build an inset from: a frame round the magnified marks; a larger frame holding a complete small chart (own gridlines and scale, a unit phrase if needed, category or time labels repeated, no title); two straight connectors joining the frames in a widening zoom. Frames and connectors are light non-data lines (G3.2), connectors beneath, the inset filled with the background. The main chart stays complete. The labels are repeated by default; an inset aligned with its main chart (the same slots or categories at the same positions, above or below it) when space is short instead forms one chart with it on a shared axis, labelled once, as the book's merged charts (p. 68–72) and its compact inset (p. 84) do; whether to share is a guided decision [D-56].
*When:* every inset. *Why:* the frames show which marks are magnified, and the main chart keeps the honest overall view. *Except:* the shared axis of an aligned inset when space is short [D-56]. *Src:* R2-20 [KT], R2-23 (fig.) p. 20, 76–85; R9-6 (fig.) p. 324–325; R10-4 (fig.) p. 334.
The unit phrase is needed where the inset's bare numbers would be misread beside the main scale: a currency, a magnitude or a suffix ("$280,000 in donations", "18.0 million US$"); the figures leave a count's plain numbers bare (p. 76, 85).

**G2.27s** [S] (fig.) Round the corners of both inset frames, and draw frames and connectors in light grey at about 3 × the gridline width.
*When:* as G2.27. *Src:* as G2.27.

**G2.28** [L] (fig.) In a truncated inset, fade bars and stacked segments to transparent over their lowest part, so they have no hard bottom edge; let a line that leaves the inset's range run out through its edge. [D-11]
*When:* insets with a truncated scale. *Why:* the fade signals that the bars do not start at zero. *Except:* outlier insets start at zero and need no fade. *Src:* R9-6 (fig.) p. 325; R2-23 (fig.) p. 80–85.

**G2.28s** [S] (fig.) Let the fade cover about the lower half of the visible part of each bar or segment. [D-11]
*When:* as G2.28. *Src:* as G2.28.

**G2.29** [G] (fig.) Place the inset in the largest empty area of the plot that overlaps no data; failing that, below, to the right of, or above the main chart (order readers'). Make it clearly larger than the region it magnifies (2 to 6 times its height in the figures). [D-11]
*When:* every inset. *Why:* the inset must be legible without hiding data. *Except:* none. *Src:* R2-20 (fig.) p. 76–80; R10-4 (fig.) p. 334.

### Units, dual axes and log scales

**G2.30** [L] [KT] Never put different units or measures on one value scale, even when their numbers share a range (satisfaction out of 10 beside minutes on hold); give each its own scale, usually as a merged chart (G2.32).
*When:* several measures. *Why:* a shared scale invites meaningless comparisons across units. *Except:* none. *Src:* R2-13 [KT] p. 66–68; R7-23 [KT] p. 274–275; R11-5 (fig.) p. 351–353.

**G2.31** [L] [KT] Do not draw two different variables against two value axes in one plot. When their actual values matter, use a merged chart; when the job is to compare their patterns of change, use an index chart (G8.18).
*When:* two measures over the same categories or time. *Why:* two scales in one plot suggest false conclusions: similar volatility, one line overtaking or rising faster than the other. *Except:* G2.33. *Src:* R2-14 [KT] p. 68–74; R10-5 p. 334; R12-1 [KT] p. 355.

**G2.32** [L] Build a merged chart as panels sharing one category or time axis, each with its own scale and unit phrase. Stack time-series panels vertically on one identical time scale drawn only at the bottom; set category panels side by side, with the category names once at the left and a short heading above each panel (fig.).
*When:* merged charts. *Why:* the separation deliberately prevents comparisons across units while keeping categories and times aligned. *Except:* none. *Src:* R2-16 p. 68–71; R9-11 p. 323, 334.

**G2.33** [L] [KT] Use a second value axis only when its unit is an exact, constant conversion of the first (millimetres and inches; a value and its "% of total"), not dollars and euros, whose rate varies. Place the secondary labels at their exact converted positions. [D-44]
*When:* exact conversions. *Why:* then both axes describe the same marks. *Except:* none. *Src:* R2-15 [KT] p. 72–74; R7-18 p. 250–251.

**G2.34** [G] On a bar or stacked-bar breakdown where both absolute values and "% of total" matter, show both scales, one on each side; set the one that serves the message in the normal label colour and the other lighter. [D-6]
*When:* breakdowns of one total. *Why:* the % scale also tells readers that the values are parts of a total. *Except:* none. *Src:* R7-18 p. 250–251.
**Consider:** whether both the amounts and the shares matter to the message; which one the message is about (its scale gets the normal label colour); the lighter scale must still meet the contrast floor (G6.5).

**G2.35** [L] [KT] Do not use a logarithmic scale in everyday charts.
*When:* every chart. *Why:* most people cannot estimate or compare values on one with useful accuracy. *Except:* very data-savvy audiences; analysis. *Src:* R2-19 [KT] p. 76–78.

### Time axes

**G2.36** [L] [KT] In graphs, run time from left to right. Never turn a time chart 90° to make its date labels horizontal, and never draw a line chart with time running down the page.
*When:* every time scale in a graph. *Why:* most audiences cannot read patterns of change drawn vertically. *Except:* tables (time may run downwards, or most recent first in very long tables if a visual cue makes the order obvious) and charts with two time scales (a day × month heatmap). *Src:* R2-37 [KT] p. 99–104; R2-38 [KT] p. 101–104; R6-38 [KT] p. 222–223.

**G2.37** [L] [KT] Label time axes on several levels: the finest unit on the first row (day numbers without leading zeros, or month abbreviations), each coarser unit on a row below, centred under its span ("Oct. 2022", "2021"), with thin gridline-grey separators at group boundaries and at both ends. Leave out a row whose content the title already gives (fig.).
*When:* every time axis in a graph. *Why:* labels stay horizontal, the span is obvious and each point is easy to date. *Except:* none. *Src:* R2-39 [KT] p. 105–106; R5-24 p. 157; figures p. 175–230.

**G2.38** [G] When first-row time labels would crowd, show every k-th one, restarting at the start of each group; never rotate or overlap them. Single-letter months (J F M …) serve narrow axes. If thinning is not enough, move to a coarser base unit or widen the chart (readers'). [D-45]
*When:* dense time axes. *Why:* thinning keeps the labels horizontal and readable. *Except:* none. *Src:* R2-40 p. 105–106, 111; R5-24 p. 157.

**G2.39** [L] Do not label a time axis with sparse full dates ("16-Oct-22" once a week).
*When:* time axes. *Why:* readers cannot tell when values occurred or what span is covered. *Except:* none. *Src:* R2-41 p. 104.

**G2.40** [L] [KT] If the first or last period is incomplete (the as-of date falls inside it), leave it out, or draw it visibly differently: an isolated marker beyond the end of the line, or a short dotted level mark instead of a full bar, with a small note "As of Dec. 10" (fig.).
*When:* every time series. *Why:* a partial period looks like a collapse. *Except:* none. *Src:* R2-42 [KT] p. 107–108.

**G2.41** [L] [KT] When time is grouped into ranges, make every range the same length (decades throughout, not decades followed by half-decades); if the source ranges differ, re-aggregate to a common one or do not plot.
*When:* binned time. *Why:* shorter bins hold less and fake a decline. *Except:* none. *Src:* R2-43 [KT] p. 110–111.

**G2.42** [L] (fig.) Give each interval of a regular series an equal slot, with its point or bar at the slot centre and half a slot of room at each end, whatever the calendar length of the interval; draw irregular series (step, thin bar) on a true linear time axis, so that gaps between events are to scale. [D-45]
*When:* every time axis. *Why:* equal slots match one value per interval; a true time axis shows irregular spacing. *Except:* none. *Src:* figures p. 172–196; R6-3 [KT] p. 175–176; R6-11 [KT] p. 194–197.

## G3 Non-data elements

**G3.1** [L] [KT] Remove every non-data element unless removing it makes the chart harder to read or ambiguous; test each one.
*When:* every chart and table. *Why:* non-data elements matter less than data, and extra ones add clutter that competes with it. *Except:* gridlines usually pass the test (G3.6). *Src:* R1-01 [KT] p. 46–47, 50; R5-28 (fig.) p. 158.

**G3.2** [L] [KT] Make the non-data elements that remain light and thin, so that none draws the eye as much as any data mark: a light grey, thin, and lighter than the palest data colour. Provisional check: contrast with the background at most 1.5 : 1 (readers') [D-34].
*When:* every remaining non-data element. *Why:* dark or thick lines distract from bars, lines and numbers. *Except:* table rules (G7.38). *Src:* R1-02 [KT] p. 47, 50.

**G3.2s** [S] (fig.) Draw non-data lines in grey ≈ #D9D9D9, about 0.08 em wide.
*When:* as G3.2. *Src:* as G3.2.

**G3.3** [L] Draw no frame around the chart or the plot area, and no outline on bars, slices, areas or other marks.
*When:* every chart, including charts placed side by side. *Why:* the chart is clear without them, and where one chart ends and the next begins stays obvious. *Except:* inset frames (G2.27), outlines on map bubbles and dots (G7.54), a border used as a highlight (G8.9), the two rules of a table (G7.38). *Src:* R1-03 p. 47; R7-33 p. 288–289.

**G3.4** [L] Draw no tick marks on either axis.
*When:* every axis with gridlines. *Why:* value labels sit centred on their gridlines and category labels on their bands, so it is clear what each belongs to. *Except:* an axis with no gridlines (a secondary scale; the dual-scale stacked bar, fig.) may have an axis line with short ticks [D-45]. *Src:* R1-04 p. 48; R2-35 p. 99; figure p. 250.

**G3.5** [L] Draw no axis line along the value scale.
*When:* default. *Why:* it adds nothing when gridlines are present. *Except:* keeping one is a minor fault, not an error. *Src:* R1-05 p. 48; R2-35 p. 99.

**G3.6** [L] Keep light gridlines at the steps of the value scale, perpendicular to the direction in which values grow (horizontal for columns and lines, vertical for horizontal bars), and none along a categorical scale. After pruning, the plot area must still be visible, values estimable to within one interval, and the direction of "more" obvious.
*When:* almost always. *Why:* gridlines mark out the plot area, help estimate and compare values, and show which way is "more". *Except:* charts labelled directly without a scale (G2.18); bumps charts, pies, treemaps and maps. *Src:* R1-06 p. 49.

**G3.7** [L] Draw the zero line as a gridline, in the gridline style; there is no separate category-axis line. Provisional: make it darker than the other gridlines only when the scale includes negative values or is a difference or index scale (fig.) [D-17].
*When:* every scale that includes zero. *Why:* it is simply the gridline for 0. *Except:* none. *Src:* R1-07 p. 48–49; R7-12 (fig.) p. 255.

**G3.8** [L] (fig.) Draw gridlines beneath the data marks, never over them.
*When:* always. *Why:* non-data must not overprint data. *Except:* none. *Src:* R1-09 (fig.) p. 47–49.

**G3.9** [L] [KT] Draw flat: no 3-D or perspective, and no effect that moves, resizes, distorts or hides a data mark.
*When:* every chart, in every style. *Why:* such effects distort values. *Except:* none. *Src:* R12-7 [KT] p. 363–365; RP6-1 [KT] p. 420, 424.

**G3.9s** [S] In the plain style, draw no bevels, gradients, drop shadows or glossy effects, and no dark or patterned background; the page is plain white, with no fill behind the plot. [D-50]
*When:* every chart in the plain style. *Why:* the book counts these effects as eye candy that adds effort and hides insights; the owner keeps shadows for a later house style, allowed there because they leave every mark where the data put it (`README.md`). *Except:* the fade that marks truncated bars (G2.28). *Src:* RP6-1 [KT] p. 420, 424; owner.

## G4 Colour

### What colour may mean

**G4.1** [L] Make every colour difference among data marks encode something (a series, a category, a quantity, good or bad) or be a deliberate highlight.
*When:* every chart. *Why:* readers take colour as information and go looking for a meaning that is not there. *Except:* none. *Src:* R3-1 p. 113, 115.

**G4.2** [L] [KT] Use muted, low-saturation colours; never fully saturated primaries or process colours (pure red, blue, cyan, magenta, yellow). Provisional check for regular series colours: OKLCH chroma at most 0.15 (readers') [D-34].
*When:* every palette. *Why:* saturated colours are tiring to look at. *Except:* the good/bad red may be stronger. *Src:* R3-2 [KT] p. 113–114.

**G4.3** [L] If nothing needs telling apart (a single series, or categories already named on the axis), draw all marks in one soft colour.
*When:* single-series charts. *Why:* only comfort matters then. *Except:* a deliberate highlight (G4.26). *Src:* R3-3 p. 115.

**G4.4** [L] [KT] Do not vary colours to make a chart "more interesting": no hue per bar when the bars are already labelled, and no shading of marks by their value when position or length already shows it (a line that darkens as it rises).
*When:* any chart clear in one colour. *Why:* readers hunt for a second variable or a highlight that does not exist. *Except:* none. *Src:* R3-4 [KT] p. 115–116; RP6-1 [KT] p. 420.

### Palette type

**G4.5** [G] [KT] Choose the palette type from what colour encodes: nothing → one colour (G4.3); unordered categories → categorical (G4.6); unordered categories in groups the message uses → a hue per group (G4.9); ordered categories → sequential (G4.7); a quantity → a sequential ramp (G4.10); either of the last two, when the audience sees a dividing value → diverging (G4.11).
*When:* every chart that uses colour for data. *Why:* the palette must match what the colour means and how the audience thinks; software cannot choose it. *Except:* none. *Src:* R3-5 [KT], R3-8 [KT], R3-11 [KT], R3-12 [KT], R3-13 [KT] p. 117–127.

**G4.6** [L] [KT] For unordered categories use distinct hues, balanced so that none is much darker, lighter or more saturated than the rest. Provisional check: OKLCH lightness spread at most 0.12 among the colours in use (readers') [D-34].
*When:* categories that could be listed in any order (regions, departments). *Why:* distinct hues suggest no order; an unbalanced palette highlights some categories by accident. *Except:* a deliberate highlight. *Src:* R3-5 [KT] p. 117–119; R3-6 [KT] p. 118–119.

**G4.7** [G] [KT] For ordered categories use one hue with lightness changing steadily in category order (darker = more), never a categorical palette.
*When:* risk levels, age bands, satisfaction or skill levels. *Why:* readers remember one rule instead of N colours, and the overall pattern shows before any text is read. *Except:* a dividing value (G4.11); more than 7–8 categories (G4.21). *Src:* R3-8 [KT] p. 120–121; R0-11 p. 19, 33; R15-4 [KT] p. 402–403.

**G4.8** [L] [KT] Never use a sequential palette for unordered categories.
*When:* nominal categories. *Why:* shades read as amounts (a darker slice seems to hold more). *Except:* G4.9. *Src:* R3-10 [KT] p. 121–123.

**G4.9** [G] [KT] To show groups of unordered categories, give each group its own hue and its members shades of it, deliberately not in lightness order (the darkest in the middle), and gather each group under its name in the legend (G5.12). [D-49]
*When:* the grouping is part of the message (countries by continent). *Why:* the groups show at once, and the two conditions stop the shades reading as amounts. *Except:* none. *Src:* R3-11 [KT] p. 123–124; R4-8 p. 124.
**Consider:** whether the grouping is part of the message (if not, a categorical palette, G4.6); how many members each group has (with two, a light and a dark shade and the grouped legend [D-49]).

**G4.10** [G] [KT] For a quantity shown by colour (dots on a map, heatmap cells), use a continuous sequential ramp, darker meaning more.
*When:* colour encodes a number. *Why:* readers perceive intensity as amount. *Except:* a dividing value (G4.11). *Src:* R3-12 [KT] p. 125.

**G4.11** [G] [KT] Use a diverging palette only when the audience sees values on either side of a dividing value as different in kind (good or bad, over or under, early or late). Put the division where the audience puts it (zero, the average, a target, an expected value), not at the middle of the data; without such a value, use a sequential palette. [D-37]
*When:* ordered categories or quantities. *Why:* each side then reads as its own kind, not as a paler version of the other. *Except:* none. *Src:* R3-13 [KT] p. 125–127.

**G4.12** [L] Build a diverging palette from two sequential ramps meeting at a quiet neutral (white or very light grey) at the dividing value, intensity growing with distance on each side. The sides may have different numbers of steps, and a side may stop growing (clamp) before its end.
*When:* every diverging palette. *Why:* values near the division are normal and deserve the least attention. *Except:* the dividing value may also be left uncoloured. *Src:* R3-14 p. 126–128.

**G4.13** [L] [KT] Never put a bright, attention-catching colour (yellow, amber) at or near the dividing value; no red–amber–green traffic lights.
*When:* every diverging palette. *Why:* the middle values are the least interesting. *Except:* none. *Src:* R3-15 [KT] p. 127–128.

**G4.14** [L] [KT] On a light background darker means more; on a dark one, lighter means more. [D-50]
*When:* every sequential or diverging palette. *Why:* readers see the colour that contrasts most with the background as "more". *Except:* none. *Src:* R3-9 [KT] p. 121, 125.

### Cutting a colour scale into steps

Not from the book, which gives no rule for where a ramp's steps fall: these rules follow Datawrapper's guide to colour-scale interpolation (L. C. Muth, "How to choose an interpolation for your color scale", blog.datawrapper.de), adopted by the owner on 1 Oct 2026. They apply to the steps of any sequential ramp and to each side of a diverging one (G4.10–G4.12), classed or continuous. The cut is chosen from the message as well as from the data.

**G4.29** [G] Start from equal steps on rounded values (a linear cut): the most intuitive cut and the most honest about the values.
*When:* colour encodes a quantity and the values are spread fairly evenly; or the few extreme values are themselves the message. *Why:* equal steps mean equal differences, so the colours can be read back without surprise. *Except:* G4.30–G4.32. *Src:* Datawrapper (linear and rounded interpolation).
**Consider:** the distribution of the values (whether a few extremes would leave nearly every mark in the lightest steps, as with disease rates before and after a vaccine); whether the outliers or the pattern among the many is the message.

**G4.30** [G] When a few extreme values would put nearly every mark in one step and the message is the pattern among the many, cut by natural breaks (steps that group values lying close together, Jenks), then round the breaks for the key.
*When:* skewed values where both truthfulness and usefulness matter. *Why:* it shows the pattern among the many while keeping the extremes in small groups of their own, a compromise between the linear and the equal-count cuts. *Except:* G4.33 when the values are amounts spanning at least two orders of magnitude; G4.31 when equal representation is itself the point. *Src:* Datawrapper (natural breaks; custom to round them).

**G4.31** [G] Use equal-count steps (quantiles: quartiles, quintiles, deciles) only when showing each step equally often is itself the point, and say so in the key.
*When:* rarely; ranking-like messages ("the top fifth"). *Why:* they reveal differences a linear cut hides, but make similar values look very different and imply that a fifth of the marks are extreme when only a few are. *Except:* none. *Src:* Datawrapper (quantile interpolation; median, quartiles, quintiles, deciles for continuous scales).

**G4.32** [G] Set breaks by hand only for a stated reason (a threshold the audience uses, rounding natural breaks), never to dramatise a pattern.
*When:* a meaningful threshold exists, or natural breaks need rounding. *Why:* full control over the cut carries the responsibility not to misrepresent. *Except:* none. *Src:* Datawrapper (custom interpolation).

**G4.33** [G] (ours; in neither the book nor Datawrapper) When skewed values whose pattern among the many is the message are amounts on a ratio scale spanning at least two orders of magnitude, cut by logarithmic steps on 1–2–5 values, with zero a step of its own.
*When:* rates or sizes over orders of magnitude (the generator: the 95th percentile of the positive values at least 100 times the 5th), where G4.30's breaks would be hard to read and would lump the small values together. *Why:* each step is then a readable ratio. *Except:* data that are not ratio-scaled (scores, temperatures) never take it; the few extremes as the message take G4.29. *Src:* common practice; owner, 1 Oct 2026 (automatic in that case).

**G4.34** [L] Whatever the cut, show the breaks in the key as rounded values formatted as a set, and keep the lightest step clearly distinct from the page and from the missing-data grey.
*When:* every stepped ramp. *Why:* the key is how a colour is read back; a first step that matches the page reads as "nothing" or as missing. *Except:* none. *Src:* Datawrapper (rounded values); G6 (numbers as a set); G4.15 (the missing grey).

### Reserved colours

**G4.15** [L] [KT] Reserve grey and black (and white on a dark background) for special categories: total, average, all others, missing. Keep them out of the regular slots of a categorical palette. [D-15] In black and white, where every part is grey, tell the regular parts apart by textures or patterns and give the special categories plain flat greys with none, placed last (owner, 1 Oct 2026) [D-60].
*When:* every chart with a categorical palette. *Why:* audiences see grey and black as set apart and read a grey series as special. *Except:* the neutral middle of a diverging palette. *Src:* R3-16 [KT] p. 128–130; R6-34 (fig.) p. 218; R7-12, R7-17 (fig.) p. 249–255.

**G4.15s** [S] (fig.) The special greys (provisional): an average or total line dark grey (#595959) with a bold label; "All others" warm grey (#968C8B); missing values mid grey (#A6A6A6); a total bar grey (#A6A6A6). [D-15]
*When:* as G4.15. *Src:* as G4.15.

**G4.16** [G] [KT] When the audience judges the values as good or bad (G0.20), colour bad red and good green, leave neutral values in the base colour, and colour any caption naming a side to match ("over budget" in red).
*When:* good/bad readings. *Why:* the association is immediate in most cultures, so problems stand out before any label is read. *Except:* audiences without the association (some Asian cultures) and colour-blind readers (G4.18). *Src:* R3-17 [KT] p. 130–131; R15-11 [KT] p. 408–410.
**Consider:** whether the audience reads the values as good or bad, and where the dividing value lies (G0.20); whether red and green carry that meaning for this audience; colour-vision deficiency (G4.18).

**G4.17** [L] Keep red and green out of the regular palette, and by default leave red out of categorical palettes altogether.
*When:* every categorical palette. *Why:* a red series is read as bad news. *Except:* green causes fewer problems. *Src:* R3-18 p. 131.

**G4.18** [L] [KT] Unless told otherwise, assume some readers have colour-vision deficiency, and check the chart in a simulation of protanopia, deuteranopia and tritanopia. If colours that must be told apart merge, use a safe pair (blue for good, orange for bad, in place of green and red). Interactive output: red/green by default with a switch. Static output (owner, 1 Oct 2026): good and bad keep their semantic green and red for every reader, in one file; in black and white they are textured as well (a hatch for bad, a dither for good), which serves these readers too; the safe pair, with the red/green version as a second file, is a style's option for publications to unknown readers [D-18].
*When:* every chart with colours that must be distinguished. *Why:* red and green look alike in the commonest forms (about 4 % of people, up to 8 % in some groups). *Except:* audiences known to be unaffected. *Src:* R3-19 [KT] p. 132–135; R3-20 [KT] p. 133–135; R15-12 [KT] p. 410.

### How many colours

**G4.19** [L] [KT] Use at most seven or eight colours in a categorical or sequential palette, especially with a legend. Provisional: warn at 8, fail above 8; the special greys are not counted [D-14].
*When:* every palette. *Why:* beyond that, colours become too alike to tell apart. *Except:* G4.20; multi-hue ramps (G4.21). *Src:* R3-21 [KT] p. 135–138.

**G4.20** [G] More than eight colours may work when every series is labelled directly and similar colours never sit next to each other; render the chart and check.
*When:* many series. *Why:* with no legend there is nothing to memorise, and distance keeps similar colours apart. *Except:* none. *Src:* R3-22 p. 136–137.

**G4.21** [G] [KT] With too many categories, reduce them first: drop those the point does not need, merge them into groups, or merge minor ones into one "All others" (grey). For ordered categories, a multi-hue sequential or diverging ramp keeps up to about nine steps distinct.
*When:* more than 7–8 categories. *Why:* fewer categories need fewer colours; hue changes keep neighbouring steps apart. *Except:* multi-hue ramps only for ordered categories. *Src:* R3-23 [KT] p. 137–139; R7-17 p. 249–250.
**Consider:** which categories the insight needs (what to leave out is the chart's own choice, G0.11); whether the audience knows groups the categories fall into; whether the categories are ordered (then a multi-hue ramp may keep them all).

### Consistency

**G4.22** [L] [KT] In a set of charts (report, deck, dashboard), give each category the same colour in every chart, and each part the same order and colour in every bar, cluster, pie or panel of one chart. [D-22]
*When:* more than one chart, or several totals. *Why:* readers learn the colours once. *Except:* too many categories or charts to keep it up. *Src:* R3-24 [KT] p. 139; R7-24 (fig.) p. 266–276.

**G4.23** [G] [KT] Use colours the audience already links with the categories (parties, teams, countries).
*When:* such associations exist. *Why:* nothing has to be memorised, and contrary colours cause misreading. *Except:* colours conventionally tied to races, genders or other demographic groups may offend; use arbitrary ones. *Src:* R3-25 [KT] p. 139–140.

**G4.24** [L] Do not force one fixed brand palette on every chart.
*When:* an organisation mandates brand colours. *Why:* one palette cannot fit ordered and unordered categories and every count, and red or green in it is harmful. *Except:* none. *Src:* R3-26 p. 140–141.

**G4.25** [G] If the chart may be printed, copied or faxed in black and white, redesign it to read without colour (for lines: G7.14; for special categories: G4.15). [D-33]
*When:* monochrome reproduction is likely. *Why:* colours collapse to black or grey. *Except:* none. *Src:* R3-27 p. 142.

### Colour in highlighting

**G4.26** [L] Highlight by strengthening the colour of the target marks (a darker, more saturated shade of their own hue) while the others keep their base colours; never by a new hue. [D-9] [D-8]
*When:* the message concerns particular marks. *Why:* intensity draws attention, so it must be used only where attention is wanted; a new hue reads as another category. *Except:* bars and slices may pale the others (G7.30). *Src:* R3-7 p. 119; R15-6 p. 404; R7-38 [KT] p. 293–294.

**G4.27** [L] [KT] Give each line in a line chart its own colour; do not turn the non-focus lines grey when lines cross. [D-8]
*When:* two or more lines. *Why:* same-coloured lines cannot be told apart where they cross. *Except:* single-line panels of small multiples; black-and-white output with symbols (G7.14). *Src:* R6-28 [KT] p. 212–213.

### The plain-style colours

**G4.28** [S] (fig.) Use these colours in the plain style (the muted palette, decided by the owner [D-2]; values sampled from the book's figures, with readers' derivations marked).
*When:* every chart until the owner fixes the palette. *Why:* muted, balanced, and passing the reserved-colour rules. *Except:* known colours (G4.23) and the document's registry (G4.22) come first. *Src:* notes 03-04 §5.1, 07 §5.1, 00-01 §5.2 (fig.).

| Role | Colours |
|---|---|
| categorical, slots 1–5 | #95B7D3 blue, #DD8045 orange, #A4AB81 olive, #D8B35B mustard, #7CA79D grey-teal (the plain style turns it to #78A6AA, owner 30 Sep 2026, so it stays apart from the olive under protanopia) |
| slot 6, when there are exactly six categories | #9A77B7 purple |
| slots 6–10, when there are seven or more (darker twins of slots 1–5; never next to their twin) | #558BB9, #BA5C28, #81875B, #B98E2F, #58837A |
| single series | bars #95B7D3; a single line #DD8045 in the book, blue #95B7D3 in the plain style (owner, 30 Sep 2026) |
| emphasis of a colour | the same hue about 0.12 darker in OKLCH lightness, more saturated (#95B7D3 → #558BB9) (readers') [D-9] |
| de-emphasis of bars and slices | the colour mixed 40 % towards white (#95B7D3 → #C0D3E4) (readers') |
| special categories | average or total line #595959; total bar #A6A6A6; all others #968C8B; missing #A6A6A6 |
| good / bad | #70AD47 / #C5392A |
| good / bad, colour-blind-safe | blue #2E5D89 / orange #AB4E2D (ramp ends; no discrete pair shown in the book) [D-18] |
| sequential, 3 steps (light → dark) | #BED8E7, #6AAED5, #3082BE |
| sequential, 5 steps | #EEF7F4, #B8CECB, #83A9AC, #548292, #2B5C7A |
| sequential, 9 steps, multi-hue | #FDFBE2, #D8EAC4, #B4DBAF, #8EC69F, #72A994, #57838C, #426086, #2A3881, #282A74 |
| diverging, 5 steps (bad → good) | #E24A33, #FDCA9F, #F3F3F3, #BFDDA9, #70AD45 |
| diverging, colour-blind-safe, continuous (good → bad) | #2E5D89, #79A2C0, #BFE0F3, #DDEEF8, #FFFFFF, #FFE2C0, #FDC683, #D48752, #AB4E2D |
| text | #595959 |
| gridlines, separators, inset frames | #D9D9D9 |
| comments (callouts) | #3F63B0 [D-19] |

## G5 Labels and legends

**G5.1** [G] [KT] Label marks directly rather than through a legend whenever possible: each line at its right end, each pie slice beside the slice, each stacked segment beside the stack.
*When:* every chart whose colours need a key. *Why:* a legend has to be memorised before the chart can be read. *Except:* G5.7. *Src:* R4-1 [KT] p. 143–144; R6-32 (fig.) p. 175–230; R7-34 p. 289, 295.

**G5.2** [L] (fig.) Put each line's label just right of its last drawn point (the end of its projection, if any), left-aligned and vertically centred on it, in the regular label style. When line ends are close, stack the labels at the label pitch, in the order of the line ends and as near them as possible. [D-7]
*When:* line, bumps and slope charts. *Why:* labels next to the latest values need no memorising. *Except:* single-series charts (G5.6). *Src:* R6-32 (fig.) p. 175–230.

**G5.2s** [S] (fig.) Set end labels about 0.5 em right of their point, in the text grey rather than the series colour, stacked one line-height apart. [D-7]
*When:* as G5.2. *Src:* as G5.2.

**G5.3** [G] Provisional test for direct labels (readers'): a label may move up to one line-height from its point without a leader line, and up to 1.5 with a thin grey leader; beyond that, or if the labels would overlap or change order, use a legend (G5.7). [D-7]
*When:* placing direct labels. *Why:* a packed label must still point unambiguously at its mark. *Except:* none. *Src:* R6-32 (fig.) p. 219–228; R4-2 p. 144–145.

**G5.4** [L] (fig.) Label stacked segments to the right of the (last) stack, level with each segment's middle; inside the segment when it is tall enough, in whichever text colour contrasts better with the fill; push crowded labels apart and add a thin leader where needed.
*When:* stacked bars and areas. *Why:* the stack's order is kept, with nothing to memorise. *Except:* none. *Src:* R4-1 [KT] p. 143–144; figures p. 250, 258–260.

**G5.5** [G] (fig.) Label a secondary series once, at its first occurrence, instead of with a legend: for bars with budget markers, "Expenses" and "Budget" beside the first bar with thin grey leader lines. [D-7]
*When:* bars with reference markers or targets. *Why:* one label per series is enough when the pattern repeats. *Except:* none. *Src:* R5-28 (fig.) p. 158; R11-4 (fig.) p. 353; figure p. 405.

**G5.6** [L] A single-series chart needs no series label or legend; the axis labels and the title carry the meaning (readers').
*When:* one series. *Why:* there is nothing to tell apart. *Except:* none. *Src:* R3-3 p. 115; notes 03-04 §5.8.

**G5.7** [G] Use a legend when direct labels would clutter the chart (clustered bars, where each label would need its own leader line), or when collisions cannot be checked before readers see the chart (charts generated from data nobody has looked at). A generator that tests for collisions while rendering may label directly and fall back to a legend when the test fails (readers'). [D-7]
*When:* direct labelling fails. *Why:* a messy chart costs more than a legend. *Except:* none. *Src:* R4-2 p. 144–145; R4-3 p. 145–146.

**G5.8** [L] [KT] Order legend entries as their colours appear in the chart, and lay them out in the same direction: a row when the colours run left to right, a column when they run top to bottom. A wrapped row fills row by row. Never use alphabetical or data-entry order when it differs.
*When:* every legend. *Why:* the reader then matches entries to marks by position as well as colour. *Except:* layouts that cannot match exactly. *Src:* R4-4 [KT] p. 145–147; R7-24 (fig.) p. 266–276.

**G5.9** [L] In a line chart with a legend, order the entries by the lines' vertical order at the most recent point.
*When:* line charts over time with a legend. *Why:* lines cross elsewhere, and the latest period matters most. *Except:* none. *Src:* R4-5 p. 146.

**G5.10** [G] [KT] Put a legend above the plot and below the title, centred over the plot (fig.). A legend below or to the left is a departure, not an error. [D-31]
*When:* every legend except those of G5.11. *Why:* the legend must be read before the marks. *Except:* G5.11. *Src:* R4-6 [KT] p. 146–147; R9-12 (fig.) p. 337, 351, 364.
**Consider:** the direction in which the colours run (top to bottom: direct labels at the right of the last bar, else a column at the right [D-31]); whether the chart is a time-series line chart (G5.11); whether direct labels would make the legend unnecessary (G5.1).

**G5.11** [G] [KT] For a time-series line chart, put the legend to the right of the plot, beside the most recent values (and next to the latest values wherever time runs).
*When:* line charts over time. *Why:* the legend sits next to the line ends it describes. *Except:* none. *Src:* R4-7 [KT] p. 146–147.

**G5.12** [L] When categories are coloured by group (G4.9), put each group's members side by side in the legend, draw a thin rule under them, and set the group name under the rule, in the group's hue (fig.).
*When:* grouped shades. *Why:* the legend itself shows that the shades are members of groups, not amounts. *Except:* none. *Src:* R4-8 p. 124.

**G5.12s** [S] (fig.) Set the group names of a grouped legend in italics.
*When:* as G5.12. *Src:* as G5.12.

**G5.13** [L] (fig.) Key a continuous colour scale with a horizontal gradient bar: its title above (variable and unit), the end values below its ends, a tick with its value at a diverging scale's dividing value, and a separate entry for missing data ("Data not available").
*When:* colour encodes a quantity. *Why:* readers can estimate values only against the key. *Except:* none. *Src:* R4-9 (fig.) p. 125–134.

**G5.14** [L] (fig.) Draw each legend key in the form of its marks: a small filled square for bars, slices and areas; a short stroke at the series' width for lines.
*When:* every legend. *Why:* the key then looks like what it stands for. *Except:* none. *Src:* R4-10 (fig.) p. 118–146.

**G5.15** [G] (fig.) In small multiples, label each series once: at the line ends in one panel, to the right of the last panel, or with a single legend above the grid. [D-38]
*When:* series repeated across panels. *Why:* repeating labels in every panel is clutter. *Except:* none. *Src:* RP3-9 (fig.) p. 371–377.
**Consider:** whether every panel holds the same series (then right of the last panel, beside the latest values); how many panels and series there are [D-38].

## G6 Text, numbers and dates

### Typeface and text styles

**G6.1** [L] [KT] Set all labelling in one plain, legible typeface with tabular digits (all one width) and lining digits (all from baseline to cap height); no script, display or fully monospaced face. Test a face by rendering "1234567890", "1111111111" and "8888888888" with kerning off: equal widths mean tabular, equal heights in the first mean lining. Turn kerning off in runs of digits.
*When:* choosing or configuring a font. *Why:* stylised faces read slowly; tabular lining digits let place values line up and read evenly. *Except:* date columns (G6.21). *Src:* R5-1 [KT] p. 148–151; R5-2 [KT] p. 149, 151; R5-3 [KT] p. 149–151; R5-4 p. 150.

**G6.1s** [S] The typeface is New Computer Modern, set by Typst (the owner's choice); it passes the test of G6.1.
*When:* the plain style. *Src:* owner (`README.md`).

**G6.2** [L] [KT] Use at most four text styles in a chart (a style is one combination of size, weight, colour and slant). Count every combination that is drawn, value labels, highlighted labels and notes included.
*When:* every chart. *Why:* fewer styles look cleaner and less busy. *Except:* comments are a separate layer (G6.3) [D-19]. *Src:* R5-6 [KT] p. 151, 158.

**G6.2s** [S] Use two text styles: a bold title, and one regular style for everything else (scale labels, unit phrase, category and series labels).
*When:* the plain style. *Except:* comments, a separate layer (G6.3) [D-19]. *Src:* as G6.2.

**G6.3** [L] [KT] Make the maker's comments (callouts and other remarks about the data) look plainly different from the chart's labelling, in a colour used for nothing else; the book uses a handwritten face. [D-19]
*When:* every comment. *Why:* the audience must see at once what is data and labelling and what is the maker's interpretation. *Except:* none. *Src:* R5-5 [KT] p. 150–151, 158.

**G6.3s** [S] Set comments in New Computer Modern italic, in a reserved blue (#3F63B0) (provisional [D-19]).
*When:* as G6.3. *Src:* as G6.3.

**G6.4** [L] [KT] Make all text large enough for the intended audience to read comfortably, including people with poor sight. Provisional floor: scale labels at least 9 pt at final size, notes and inset labels at least 0.8 em (proposed) [D-6].
*When:* every text element. *Why:* small text forces effort or cannot be read. *Except:* none. *Src:* R5-7 [KT] p. 151–152.

**G6.5** [L] [KT] Never give text a colour close to its background; set all labelling in one dark colour. Provisional floor: 4.5 : 1 for every text element, including notes, secondary scales and de-emphasised values (readers') [D-6].
*When:* every text element. *Why:* low contrast is hard to read, especially with poor sight. *Except:* none. *Src:* R5-8 [KT] p. 151–152.

**G6.5s** [S] (fig.) The labelling colour is dark grey #595959 (about 7 : 1 on white).
*When:* as G6.5. *Src:* as G6.5.

**G6.6** [L] [KT] Set every piece of text horizontally: titles, the unit phrase (G2.19), category labels (G2.4), time labels (G2.37), data labels and comments.
*When:* always. *Why:* vertical or diagonal text is one of the named causes of effort. *Except:* none; when text does not fit, change the layout, never the angle. *Src:* R0-06 p. 37; R2-08 [KT] p. 61, 106–107; R5-26 (fig.) p. 158.

**G6.7** [L] (fig.) Wrap a long label onto at most two lines rather than shrinking or rotating it ("Finance & / Admin", "New / England").
*When:* labels wider than their space. *Why:* two short lines read faster than rotated or tiny text. *Except:* none. *Src:* R5-26 (fig.) p. 158; R9-10 (fig.) p. 302.

### Titles

**G6.8** [L] [KT] Let the title block say what is measured, broken down by what, and the full period including the year ("Expenses by Region, 2022"; "Jan–Jun 2022", never "Jan–Jun"). Provisional: the insight is the title (G8.2), and this descriptive line sits directly beneath it in the regular label style, adding no text style [D-5].
*When:* every chart. *Why:* a printout that resurfaces a year later must not be read as current; the descriptive line identifies the data. *Except:* none. *Src:* R2-12 [KT] p. 65–66; R5-27 (fig.) p. 151–158.

**G6.9** [L] (fig.) Make the title stand out from the labels without shouting, in the labelling colour; wrap it onto a second line rather than shrinking it.
*When:* every chart and table. *Why:* the title must lead without shouting. *Except:* none. *Src:* figures p. 46–417 (notes 05 §5.1, 13-15 §5.2); R10-21 p. 346–348.

**G6.9s** [S] (fig.) Set the title bold, about 1.2–1.3 × the label size. Provisional alignment: left, flush with the left edge of the scale labels, for charts and tables alike [D-5].
*When:* as G6.9. *Src:* as G6.9.

### Numbers

**G6.10** [G] [KT] Show only as many digits as the chart's message or question needs.
*When:* every number: scale labels, data labels, tables. *Why:* extra digits add clutter and make readers count. *Except:* G6.13. *Src:* R5-9 [KT] p. 153; R10-12 p. 344.
**Consider:** the smallest difference the message needs to show; whether a rule or authority requires all digits (G6.13).

**G6.11** [G] [KT] Consider hiding the trailing digits of numbers longer than four digits by scaling to thousands, millions or billions ($195,818.56 → $196K), with one unit for a whole set of numbers read together. Provisional: scale when the largest number has five or more integer digits, choosing the largest unit that leaves at least 1 before the decimal point and at most one decimal (readers') [D-4].
*When:* large numbers. *Why:* less clutter; large numbers become easier to read. *Except:* G6.13. *Src:* R5-10 [KT] p. 152, 158.

**G6.12** [L] [KT] When digits are hidden, make it unmistakable on the label itself: a suffix straight after the number ($196K) or a unit note ("$000s"), in the audience's own convention; never drop digits silently. Provisional for an unknown audience: "K" for thousands, and "million" and "billion" spelled out (G2.23) [D-4].
*When:* every scaled number that can be read on its own (data labels, pie labels, table cells). *Why:* silently dropped digits make readers misjudge by orders of magnitude. *Except:* scale labels, whose magnitude is stated once in the unit phrase (G2.22). *Src:* R5-11 [KT] p. 152, 154; R5-13 p. 153; R2-06 p. 60.

**G6.13** [G] Keep all digits when small differences between close values are the point, or when a body (a regulator, an authority) requires them; then put the long numbers in a table beside or below the graph (G1.14).
*When:* those two cases. *Why:* hiding digits would destroy needed information, but long numbers clutter a graph. *Except:* none. *Src:* R5-12 p. 152–153; R10-12 p. 344.

**G6.14** [L] [KT] When all digits are shown, group the integer digits in threes from four integer digits up (5,477,210; $1,500K), using the separator the audience expects: comma, full stop or (thin) space.
*When:* numbers with four or more integer digits. *Why:* readers otherwise count digits; a wrong separator can be read as a decimal mark. *Except:* years. *Src:* R5-14 [KT] p. 153–154; R5-15 p. 153.

**G6.15** [L] (fig.) Round to the nearest displayed unit, halves away from zero; never truncate ($582,793 → $583K, not $582K).
*When:* every shortened number. *Why:* truncation biases every value downwards. *Except:* none. *Src:* R5-16 (fig.) p. 152.

**G6.16** [L] (fig.) Format every set of numbers read together (a series' data labels, a table column) with the same prefix, suffix and number of decimals, zero included; write a negative amount with its sign before the currency symbol (−$20K) [D-51]. Scale labels: G2.24.
*When:* data labels and tables. *Why:* mixed formats ($0 beside $500K) look like different units. *Except:* none. *Src:* R5-17 (fig.) p. 158, 165–167.

**G6.17** [L] (fig.) Right-align numbers stacked in a column (table columns of comparable numbers, vertical scale labels) so that place values line up.
*When:* vertical stacks of numbers. *Why:* tabular digits line up only when the right edges do. *Except:* numbers not meant to be compared (G7.41). *Src:* R5-18 (fig.) p. 149, 153, 167.

### Dates

**G6.18** [G] [KT] If the audience is used to a particular date format, use it.
*When:* checked first; it overrides the date rules below. *Why:* the most intuitive format differs by region and organisation. *Except:* none. *Src:* R5-19 [KT] p. 154, 157.

**G6.19** [L] [KT] Otherwise write months in letters ("Aug", "August"), never as numbers.
*When:* no audience format. *Why:* month names are more intuitive and show at once which part is the month. *Except:* none. *Src:* R5-20 [KT] p. 154, 157.

**G6.20** [L] [KT] Otherwise write years with four digits.
*When:* no audience format. *Why:* "22" could be a day; "2022" can only be a year. *Except:* none. *Src:* R5-21 [KT] p. 154–155, 157.

**G6.21** [L] [KT] In a column of dates, give each part a fixed width: a two-digit day, a three-letter month and a four-digit year (02-Dec-2021).
*When:* vertical lists of dates (table columns). *Why:* days, months and years line up and the column scans quickly. *Except:* none. *Src:* R5-22 [KT] p. 155–158; R5-23 p. 156; R10-24 p. 348–349; R0-16 p. 29.

**G6.21s** [S] Set date columns in the labelling face, accepting the slight drift of month widths in a proportional face (provisional); a monospaced face is the alternative [D-32].
*When:* as G6.21. *Src:* R5-23 p. 156.

**G6.22** [L] Write single dates in titles, notes and callouts with a lettered month and a four-digit year; provisional form "Mar. 11, 2022" (readers', from the book's titles) [D-32].
*When:* inline dates. *Why:* unambiguous and familiar. *Except:* G6.18. *Src:* R5-20 [KT], R5-21 [KT] p. 154–157; R5-27 (fig.) p. 151.

**G6.23** [L] On time axes, put day, month and year on separate rows of a multi-level scale (G2.37) instead of composite date strings.
*When:* time axes. *Why:* separate rows remove the need for long date labels. *Except:* none. *Src:* R5-24 p. 157.

### Notes under a chart or table

**G6.24** [L] Put in a note under the chart or table whatever the reader needs to read it right and its labels cannot say: what a word or mark in a cell means ("POA", "n/a"), a shortened name in full, an estimate or a break in a series, a definition, the data's source. Never leave it to the message that carries the chart. A note that explains one element is a footnote: set its mark on that element (on the last line of a wrapped text, and on every cell of a status) and start the note with the same mark; a note on the whole chart has no mark. Number the marks in reading order, top to bottom and then left to right; give a note referred to in several places one mark; give every mark its note and every footnote its mark. Set the notes below everything else, left-aligned with the chart and wrapped to its width, the footnotes first in the order of their marks, then the other notes; dates in them as G6.22.
*When:* every chart and table with something its labels cannot explain. *Why:* a chart travels without its covering message, and an unexplained status or abbreviation is misread, or read as a value. *Except:* none. *Src:* owner, 1 Oct 2026 (evaluation round 1, B12, where the meaning of POA and n/a was only in the reply).

**G6.24s** [S] (provisional) Letter the marks a, b, c (a raised numeral beside a figure reads as an exponent), raised so that their x-height meets the cap height of the text they mark, in the notes' style (0.8 em, italic, the note grey; G7.2s), so that they add no text style. A table cell's mark hangs in the gutter so that the column's figures stay aligned (G6.17); on a fill, the mark takes the colour of the text it marks (G6.5).
*When:* as G6.24. *Src:* as G6.24; the look to be confirmed by the owner (`examples/review/r1-footnotes/`).

## G7 Formatting each chart type

### Bars

**G7.1** [L] (fig.) Keep the gaps between bars narrower than the bars, with half a gap between the outer bars and the plot edge.
*When:* bar charts. *Why:* the book asks to avoid wide gaps; it gives no reason or number. *Except:* clustered bars (G7.5). *Src:* R9-8 (fig.) p. 327.

**G7.1s** [S] (fig.) Make each bar about 0.55 of its band (a gap of about 0.8 × the bar), within 0.5–0.63 [D-35].
*When:* as G7.1. *Src:* as G7.1.

**G7.2** [L] Label every bar with no visible length with what it means ("(zero)", "(missing)", "(unknown)", "(TBD)", "(error)"), just above the baseline in the bar's slot (fig.) [D-6].
*When:* any category whose bar is empty. *Why:* an empty slot could mean any of these, and makers usually leave it unexplained. *Except:* none. *Src:* R9-7 p. 325–327; R0-12 p. 19, 33.
**Consider:** a note wider than its slot is wrapped first, as names under bars are (G2.4), within the room the neighbouring bars leave; else set under the slot's label when nothing stands beneath the labels; else above, with a thin leader down to the slot (owner, 1 Oct 2026; P3-38 and its `bars`).

**G7.2s** [S] (fig.) Set these notes in small italic text, in a grey that meets the contrast floor of G6.5 [D-6].
*When:* as G7.2. *Src:* as G7.2.

**G7.3** [L] With negative values, draw every bar from zero and make the zero line darker (G3.7); colour positive and negative bars differently only when the audience reads the sign as good or bad (G4.16); a breakdown may close with a grey Total bar (fig.).
*When:* bars with negative values. *Why:* the baseline is then the reference the reader needs. *Except:* none. *Src:* R7-12 (fig.) p. 234, 255, 264; R7-04 [KT] p. 255–257.

**G7.4** [L] (fig.) When values are labelled directly (G2.18), put each label just inside the end of its bar, all in one format (G6.16), in white or the text grey, whichever meets the contrast floor on that fill; if neither does, put it just outside the end (readers') [D-6].
*When:* directly labelled bars. *Why:* the label sits where the value ends. *Except:* none. *Src:* R2-32 (fig.) p. 93–94; figure p. 252.

**G7.5** [L] (fig.) In a clustered bar chart, let the bars of one cluster touch, separate the clusters clearly, keep the series in the same order in every cluster, and give the series a legend row above the plot (G5.7). [D-7]
*When:* clustered bars. *Why:* touching bars read as one group per category. *Except:* none. *Src:* R9-12 (fig.) p. 337, 351, 364; R7-24 (fig.) p. 266–276.

**G7.5s** [S] (fig.) Separate clusters by one to two bar widths.
*When:* as G7.5. *Src:* as G7.5.

**G7.6** [L] (fig.) Draw merged bars horizontally in one colour: panels side by side, the category names once at the left, each panel with its own zero-based scale along the bottom and a short heading naming its measure, the categories in the same order in every panel.
*When:* merged bar charts. *Why:* shared rows keep categories aligned while the scales stay separate. *Except:* none. *Src:* R9-11 p. 323, 334; R2-16 p. 68–71.

**G7.7** [L] (fig.) Draw a wrapped bar chart as one list sorted descending and cut into k side-by-side columns of equal length, filled top to bottom, all on the same scale (the same length per unit), with scale labels at the top and bottom of each column.
*When:* very many parts (about 100). *Why:* a single column would be too long, and one scale keeps every bar comparable. *Except:* none. *Src:* R7-14 [KT] p. 244–245; figure p. 245.

### Lines

**G7.8** [L] Apply every rule of G2–G6 to line charts; the rules below add to them.
*When:* line charts. *Why:* line-formatting mistakes confuse or mislead, not just look bad. *Except:* none. *Src:* R6-21 p. 204, 218.

**G7.9** [L] [KT] Never draw a line through a missing value: end the line at the last known point and resume it at the next, leaving a visible gap. Never plot a missing value as zero, and never bridge the gap with a dashed segment.
*When:* a regular series with one or a few missing values (many: G1.49). *Why:* a joined line claims the value is known; zero is a known value; a dashed bridge implies an estimate that does not exist. *Except:* none. *Src:* R6-22 [KT] p. 205–211; R6-23 p. 208; R6-24 [KT] p. 208–211.

**G7.10** [L] [KT] Keep dashed and dotted lines for values of lower certainty or importance (projections, estimates, targets); never use dash patterns to tell series apart. [D-21]
*When:* every line. *Why:* dashes read as "less certain", so as identifiers they imply differences that do not exist. *Except:* none. *Src:* R6-24 [KT] p. 208–211, 215, 218; R6-31 p. 215.

**G7.11** [L] [KT] Add small point markers in the line's colour at every point of a line that has a known value with missing values on both sides, or that runs perfectly straight through intermediate points; otherwise draw no markers. [D-29]
*When:* lines. *Why:* an isolated value is otherwise invisible, and a straight run hides that the intermediate values are known. *Except:* black-and-white symbols (G7.14). *Src:* R6-25 [KT] p. 209–211; R6-26 p. 218; R6-27 p. 211, 216.

**G7.11s** [S] (fig.) Draw point markers as filled dots about twice the line's width across.
*When:* as G7.11. *Src:* as G7.11.

**G7.12** [S] (fig.) Draw series lines about 0.2 em thick (2–3 × the gridline width), with round joins and ends [D-35].
*When:* line charts. *Why:* clearly heavier than gridlines, light enough for several lines. *Except:* emphasis (G7.13). *Src:* figures p. 172–230 (notes 06 §5.1).

**G7.13** [L] To emphasise one line, draw it thicker and in a stronger shade of its own hue (fig.), and keep every other line in its own colour and width. [D-9] [D-8]
*When:* the message concerns one series among several. *Why:* thickness singles the line out while the others stay identifiable. *Except:* none. *Src:* R6-29 p. 213–214.

**G7.13s** [S] (fig.) Draw an emphasised line about twice as thick as the others [D-9].
*When:* as G7.13. *Src:* as G7.13.

**G7.14** [G] [KT] If black-and-white reproduction is likely, give each line a different marker symbol at every point (square, triangle, ×, filled circle, +, asterisk), all in one dark ink, an emphasised line staying thicker; if it is not likely, add no symbols. [D-33]
*When:* line charts. *Why:* symbols survive printers and copiers that turn colours to black; they are the least bad option. *Except:* none. *Src:* R6-30 [KT] p. 214–216.

**G7.15** [L] (fig.) Draw a projection as a dotted continuation of its line, in the same colour and width, starting at the last actual value, with the series label at the projection's end.
*When:* projected values. *Why:* dotted marks lower certainty (G7.10) while the colour keeps the series. *Except:* none. *Src:* R6-33 (fig.) p. 218.

**G7.16** [L] (fig.) Draw an average of the plotted series as a solid line in the reserved dark grey (G4.15), dotted where it is projected; provisionally draw a target or outside benchmark as a dotted line in the same grey [D-21].
*When:* reference series in line charts. *Why:* grey marks a special category (G4.15); dots mark lower certainty or importance. *Except:* none. *Src:* R6-34 (fig.) p. 218; R3-16 (fig.) p. 130; R14-3 (fig.) p. 386.

**G7.16s** [S] (fig.) The reference grey is #595959, and an average's end label is bold.
*When:* as G7.16. *Src:* as G7.16.

**G7.17** [L] [KT] Do not smooth a line that passes through data points. Smoothing is allowed only for a trend line fitted separately (the raw series thin and pale beneath a thicker, darker trend, both labelled) and for bumps charts, whose curves must be flat at every point (a cubic curve with horizontal tangents) so they cannot overshoot.
*When:* every line. *Why:* smoothing can pass above 100 % or dip to levels the data never reached, the amount is arbitrary, and it adds only looks. *Except:* those two cases. *Src:* R6-39 [KT] p. 223–226; R6-40 p. 225–226.

### Dot plots and other time charts

**G7.18** [L] (fig.) Draw dot plots with one dot per value, all of one size, in one colour; categorical dot plots sit on a scale that need not start at zero (G1.57), with categories ordered as for bars (G2.1).
*When:* dot plots. *Why:* position alone carries the value. *Except:* none. *Src:* R6-2 [KT] p. 172–174; R0-14 p. 20; figures p. 85–87.

**G7.18s** [S] (fig.) Dots about 0.5 em across; open circles in categorical dot plots. The open circles are drawn larger, about 0.85 em across with a ring about 0.12 em (fig. p. 85, 89).
*When:* as G7.18. *Src:* as G7.18.

**G7.19** [L] (fig.) Draw a step chart on a true time axis: from each change, run level to the next change and then rise or drop vertically; extend the last value to the chart's as-of date; no markers. [D-42]
*When:* irregular states (G1.53). *Why:* the value held throughout each interval and then jumped. *Except:* none. *Src:* R6-8 [KT] p. 188–190; figure p. 188.

**G7.20** [L] (fig.) Draw a thin bar chart on a true time axis: one narrow bar of constant width per event, centred on its exact time and rising from zero, with nothing between events. [D-42]
*When:* individual irregular events (G1.54). *Why:* each event appears at its instant without implying values in between. *Except:* none. *Src:* R6-11 [KT] p. 194–197; figures p. 194–196.

**G7.20s** [S] (fig.) Thin bars about 1.2–1.5 × the line width [D-42].
*When:* as G7.20. *Src:* as G7.20.

**G7.21** [L] [KT] Draw an arrow chart with one row per category, its name at the left: a dot at the before value and an arrowhead whose tip sits exactly on the after value (readers'). Use two panels sharing the rows, change in units and change in % (the % arrows start at 0 %), and a small key ("Before", "After") over the two ends of one arrow instead of a legend (fig.). [D-40]
*When:* two-period comparisons (G1.46). *Why:* the arrow shows direction and size of change without crossings. *Except:* none. *Src:* R6-15 [KT], R6-17 [KT] p. 202–204; figures p. 202, 423.

**G7.21s** [S] (fig.) Set the arrow chart's key in tiny italic text.
*When:* as G7.21. *Src:* as G7.21.

**G7.22** [G] Colour arrows (and slope-chart lines) green for good changes and red for bad ones; with no good/bad reading, use two regular palette colours for rises and falls (proposed). [D-40]
*When:* arrow and slope charts. *Why:* red and green claim a good/bad reading (G4.17), which rises do not always have. *Except:* none. *Src:* R6-15 (fig.) p. 202; R3-17 [KT] p. 130–131; R3-18 p. 131.

**G7.23** [L] Draw a slope chart (only in the rare case of G1.46) as one line per category between two positions on a zero-based scale, coloured as in G7.22 and labelled at the right; if the labels collide, use an arrow chart instead.
*When:* rank changes between two periods. *Why:* close values make slope-chart labels collide. *Except:* none. *Src:* R6-16 p. 201–203.

**G7.24** [L] (fig.) Draw a bumps chart with rank on the vertical scale, 1 at the top, the word "Rank" set horizontally above the rank numbers, and no gridlines; one line per competitor, each labelled at the right in final-rank order; optionally smoothed as in G7.17. [D-41]
*When:* bumps charts. *Why:* the labels then fall in order and the eye follows each competitor. *Except:* none. *Src:* R6-7 [KT] p. 179–182; R6-40 p. 225–226; figures p. 181–187.

### Pies

**G7.25** [L] Give every slice its own colour.
*When:* every pie. *Why:* same-coloured slices merge into one blob outside the point of focus. *Except:* none. *Src:* R7-32 p. 287–288.

**G7.26** [L] Draw no borders round slices; separate them with thin gaps in the background colour, which keeps a border free for highlighting.
*When:* every pie. *Why:* less clutter. *Except:* a highlighted slice (G7.30). *Src:* R7-33 p. 288–289.

**G7.26s** [S] (fig.) Make the gaps between slices about 0.7 % of the diameter.
*When:* as G7.26. *Src:* as G7.26.

**G7.27** [L] Label each slice outside it with its name and then its value or percentage (two lines, fig.), with a leader line where the label cannot sit beside its slice; never a legend and never text on the slices. [D-51]
*When:* every pie. *Why:* text reads more easily on a plain ground, long names fit, and text on slices hinders comparing their sizes. *Except:* none. *Src:* R7-34 p. 289, 295.

**G7.28** [G] Order slices by size, largest first, starting at 12 o'clock and running clockwise (smallest first when that serves the message); keep ordinal categories in their order; put "All others" and then any zero-value slices last (fig.).
*When:* every pie. *Why:* readers care most about the largest parts, and in a pie "first" is at 12 o'clock. *Except:* none. *Src:* R7-35 p. 289–291; R7-17 (fig.) p. 249–250.
**Consider:** whether the small parts are the point (then smallest first); whether the categories are ordinal (then their own order). The start at 12 o'clock and the clockwise direction do not change.

**G7.29** [L] [KT] Keep zero-value parts in the chart, labelled, unless they are known to be irrelevant, and draw a leader line to every zero or very small slice so that it is not missed (provisional: slices under 8°, readers') [D-34].
*When:* pies with zero or tiny parts. *Why:* zero is neither unimportant nor unknown, and it may be the headline. *Except:* parts known to be irrelevant. *Src:* R7-36 [KT], R7-37 [KT] p. 292.

**G7.30** [L] [KT] Never explode a slice. Highlight a slice with a darker shade of its colour (optionally paling the others) or with a border in a darker shade. [D-9]
*When:* a slice is the point. *Why:* pulled-out slices are harder to compare with each other and with the whole. *Except:* none. *Src:* R7-38 [KT] p. 293–294.

### Stacked bars and areas

**G7.31** [G] (fig.) Stack the largest or most important part at the baseline, the parts of a featured subtotal together at the base, and "All others" at the top; keep one order and one colour per part in every bar. [D-26]
*When:* stacked bars and areas. *Why:* only the bottom part sits on the common baseline and can be read precisely. *Except:* none. *Src:* R7-08 [KT] p. 242–244; R7-17 (fig.) p. 249–250; R7-24 (fig.) p. 266–276.
**Consider:** which part the message needs read precisely (it goes at the base); whether a subtotal is featured (its parts together at the base); the order chosen is kept in every bar (G4.22) [D-26].

**G7.32** [L] (fig.) Normalise each bar or period of a 100 % form to its own total on a 0–100 % scale; separate stacked-bar segments by hairline gaps in the background colour, and let the bands of stacked areas touch [D-35].
*When:* stacked charts. *Why:* the gaps separate segments without borders (G3.3). *Except:* none. *Src:* R7-30 p. 283–284; figures p. 250, 258–260.

### Waterfalls and treemaps

**G7.33** [L] (fig.) Draw a waterfall with each bar starting where the previous one ended, gains and losses in two colours (green and red only for a good/bad reading), a hairline connector at each running total, and a final total bar from zero in grey, on a scale that includes zero. [D-43]
*When:* waterfalls (G1.38). *Why:* the running total is visible after each step. *Except:* none. *Src:* R7-11 [KT] p. 257–258; figure p. 257.

**G7.34** [L] Draw a treemap with a squarified layout (largest rectangle at the top left) and thin background-coloured separators; for a hierarchy, colour by the top-level group and label the groups; shorten leaf labels with "…" and leave out those that cannot fit.
*When:* treemaps (G1.39). *Why:* small rectangles cannot hold labels, and tooltips help only in interactive output. *Except:* none. *Src:* R7-15 [KT] p. 245–249; R7-16 p. 247–249.

### Heatmaps

**G7.35** [L] (fig.) Draw a heatmap of many series with a row per series and a column per period, cells coloured on a sequential ramp (diverging only with a dividing value, G4.11), missing cells in a grey distinct from every value colour, and a gradient key (G5.13).
*When:* too many series for small multiples (G1.20). *Why:* one row per series fits dozens of series. *Except:* none. *Src:* R6-36 [KT] p. 219–222; figure p. 221.

**G7.36** [L] (fig.) Draw a heatmap table (the replacement for a radar chart) with items as columns and attributes as rows, each value printed in its coloured cell with one decimal, the cells touching. [D-37]
*When:* comparing items across attributes. *Why:* it shows directly which item is best and each one's strengths and weaknesses. *Except:* none. *Src:* R12-6 [KT] p. 360–363; figure p. 361.

### Tables

**G7.37** [G] Show only the digits the table's purpose needs, with the same number of decimals down each column; ask what precision the audience needs.
*When:* every table. *Why:* extra digits clutter; required ones (legal) must stay. *Except:* none. *Src:* R10-12 p. 344.
**Consider:** the precision the audience needs for its job; any legal or regulatory requirement (G6.13). The same decimals down a column are locked (G6.16).

**G7.38** [L] Remove cell borders, boxes and vertical rules; keep one thin rule under the header row (mid grey) and one above the totals row (darker grey) (fig.), and separate rows and columns with space. [D-52]
*When:* every table. *Why:* the same pruning as for charts (G3.1). *Except:* none. *Src:* R10-13 p. 344–345, 349; R1-08 p. 49–50; R0-16 p. 29.

**G7.39** [L] (fig.) Keep columns close together with extra space before a Total column, and set rows well apart.
*When:* every table. *Why:* the eye travels a short way from value to value and does not skip rows. *Except:* none. *Src:* R10-14, R10-15 p. 345.

**G7.39s** [S] (fig.) Gutters about 1.5 em, about 3 em before a Total column, and a row pitch of about 2.15 em [D-35].
*When:* as G7.39. *Src:* as G7.39.

**G7.40** [L] Do not shade alternate rows (zebra stripes). [D-52]
*When:* every table. *Why:* stripes add clutter, and a shaded row can look highlighted; row spacing solves scanning instead. *Except:* none stated. *Src:* R10-16 p. 345, 349; R1-08 p. 49–50.

**G7.41** [L] Left-align text and date columns; right-align numbers that are to be compared; left-align a column of numbers that should not be compared (a key-metrics list, its values bold with their own units, fig.); align each heading like its column.
*When:* every table. *Why:* alignment shows which numbers are commensurable and lines up place values. *Except:* none. *Src:* R10-17, R10-18, R10-19, R10-20 p. 346–349; R0-16 p. 29.

**G7.42** [L] Keep a table title quiet: never coloured, aligned as in G6.9.
*When:* every table. *Why:* its position already identifies it. *Except:* none. *Src:* R10-21 p. 346–348.

**G7.42s** [S] Set a table title bold, in the dark text grey, a little larger than the body.
*When:* as G7.42. *Src:* as G7.42.

**G7.43** [L] Do not bold the header row, the totals row or the Total column.
*When:* every table. *Why:* position, spacing and rules already set them apart; bold is for important content. *Except:* none. *Src:* R10-22 p. 348.

**G7.44** [G] Once the rest is calm, highlight the few values that matter, subtly: a light grey cell fill.
*When:* tables with values to single out. *Why:* in a calm table a slight fill is enough. *Except:* none. *Src:* R10-23 p. 346–348.
**Consider:** which values the insight names (choosing them is open); how few they are: a fill singles out a handful, not half the table.

**G7.44s** [S] (fig.) The table highlight fill is #E8E8E8.
*When:* as G7.44. *Src:* as G7.44.

**G7.45** [L] State the unit once, in the title ("Funds Raised (US$) by Event by Region"), not in every cell.
*When:* every table. *Why:* a "$" in fifty cells is clutter. *Except:* a key-metrics list with mixed units (G7.41). *Src:* R10-25 p. 348–349; R0-16 p. 29.

**G7.46** [L] (fig.) Place a companion table directly under its graph: one column under each category or cluster, the series names as row labels at the left, no rules.
*When:* G1.14. *Why:* each number sits under the mark it belongs to. *Except:* none. *Src:* R10-7 (fig.) p. 337; R2-31 (fig.) p. 94.

**G7.47** [G] (fig.) Build graphics into a table as: heatmap shading scaled separately for the body, the Total column and the totals row; a "Trend" column of sparklines, each scaled to its own row; or bars inside the cells, from the left edge, under right-aligned numbers.
*When:* the audience refuses graphs (G1.16). *Why:* some pattern then shows even in a table. *Except:* none. *Src:* R10-9 (fig.) p. 339.
**Consider:** what the audience must see in the table: sizes across the body (shading), each row's pattern over time (sparklines), or sizes within a column (bars in cells) (proposed).

### Small multiples

**G7.48** [G] (fig.) Give all panels one shared value scale by default; give each panel its own scale when the message compares the patterns of change of series of very different size (such panels need not start at zero). [D-38]
*When:* small multiples. *Why:* a shared scale lets sizes be compared across panels; separate scales show each shape clearly but lose that comparison. *Except:* none. *Src:* RP3-8 (fig.) p. 371–377; R6-37 p. 220–221.

**G7.49** [L] (fig.) Show scale labels only left of the first column (once per row) and category or time labels only under the bottom row; put column headings above the top row and row headings at the left, horizontal; give the grid one overall title.
*When:* small multiples. *Why:* each dimension is labelled once. *Except:* none. *Src:* RP3-8 (fig.), RP3-9 (fig.) p. 371–377.

**G7.50** [L] (fig.) Draw all marks in one colour when every categorical variable is on the rows or columns; use a categorical palette only for a variable shown within the panels.
*When:* small multiples. *Why:* colour then means something (G4.1). *Except:* none. *Src:* R6-28 [KT] p. 212–213; figures p. 219–221, 371–382.

**G7.51** [L] (fig.) Make panels equal in size and let them abut, divided by very light separators, with no box round each panel. [D-38]
*When:* small multiples. *Why:* the grid reads as one figure. *Except:* none. *Src:* figures p. 219–221, 371–382.

**G7.51s** [S] (fig.) Panel separators 1 px wide.
*When:* as G7.51. *Src:* as G7.51.

**G7.52** [L] Place callouts in empty panels or margins, and let one callout send arrows into several panels to state a comparison between them.
*When:* small multiples with insights. *Why:* comparisons across panels become obvious. *Except:* none. *Src:* RP3-7 p. 371, 375.

### Maps

**G7.53** [L] [KT] Draw coloured dots one per region, all the same size, coloured on a sequential ramp (diverging with a dividing value) with a thin darker outline; where a region is too small to show beside its dot, move the dot beside the region and join them with a thin leader line.
*When:* coloured-dot maps. *Why:* equal dots remove the size bias; the leader keeps tiny regions visible. *Except:* none. *Src:* R8-6 [KT] p. 309; R8-7 p. 309.

**G7.53s** [S] (fig.) Map dots about 5 % of the map width across.
*When:* as G7.53. *Src:* as G7.53.

**G7.54** [L] [KT] Draw map bubbles with area proportional to the value, semi-transparent, with a thin dark outline.
*When:* bubble maps. *Why:* a small bubble lying under a larger one stays visible. *Except:* none. *Src:* R8-10 [KT] p. 312–313.

**G7.54s** [S] (fig.) Map bubbles about 45 % opaque.
*When:* as G7.54. *Src:* as G7.54.

**G7.55** [L] (fig.) Draw bars on regions at one narrow width, rising from zero, keyed by a single reference bar; tiles as equal squares in a rough geographic grid; lines on regions as small framed panels sharing one value range and one time range; the base map in light grey with hairline borders. [D-36]
*When:* those map types. *Why:* shared ranges keep the small marks comparable; a quiet base map keeps attention on the data. *Except:* none. *Src:* R8-8 [KT] p. 310–311; R8-11 [KT] p. 313–314; R8-13 [KT] p. 315–316.

**G7.56** [L] (fig.) Beside a map, draw the companion bar chart in the same colour for the same measure, sorted descending.
*When:* G1.25 "both matter". *Why:* one colour links the two views. *Except:* none. *Src:* R8-4 p. 304; figure p. 302.

### Combos and replacements

**G7.57** [L] (fig.) Draw a reference series (budget, target) with bars as a short flat marker across each bar: as wide as the bar, in a darker shade of the bar's hue, on the bars' zero-based scale, labelled once (G5.5).
*When:* bars featuring individual periods, with a reference (G1.60). *Why:* a flat marker pushes the reference into the background and invites period-by-period comparison. *Except:* none. *Src:* R11-4 p. 353; R11-5 (fig.) p. 351–353.

**G7.57s** [S] (fig.) Reference markers about 0.2 bar-widths thick.
*When:* as G7.57. *Src:* as G7.57.

**G7.58** [L] [KT] Replace bullet graphs with action dots: rows sorted by value; the name right-aligned; a thin light-grey bar proportional to the value; the value itself, emphasised; and a coloured dot (strong or light green, light or strong red) only where action is warranted, its status supplied with the data (fig.). [D-1]
*When:* dashboards of metrics against thresholds. *Why:* the metrics needing attention stand out at once. *Except:* none. *Src:* R12-4 [KT] p. 358–359.

**G7.58s** [S] (fig.) Set the values of action dots in bold.
*When:* as G7.58. *Src:* as G7.58.

**G7.59** [L] [KT] Replace a funnel chart with a bar chart of the count at each stage, in stage order, and beneath it a bar chart of the % change from each stage to the next, each change bar centred between the two stages it links (fig.).
*When:* stage-by-stage processes. *Why:* bars allow precise comparison without a label on every layer. *Except:* none. *Src:* R12-5 [KT] p. 360.

## G8 Making the message obvious

**G8.1** [G] Make the message obvious with six techniques: state the insight (G8.2); choose the design for it (G0.5); highlight what matters (G8.9); add comparison values (G8.12); colour good and bad (G4.16); show calculated values where raw ones mislead (G8.15).
*When:* every communication chart. *Why:* these are what make a chart's point quick to see. *Except:* each applies only where relevant. *Src:* R15-0 p. 399, 417.
**Consider:** which of the six the insight and the audience need: no comparison values when the insight compares the items shown (G8.12), no red and green without a good/bad reading (G4.16), no calculated values when the raw ones answer directly (G8.15).

### Stating the insight

**G8.2** [L] [KT] When the chart's job is to answer a question, persuade, or make the audience aware of something, state the key insight explicitly, in the title, in callouts or in both; a title that only names the data ("X by Y") is not enough. The rule holds only for those jobs (owner, 30 Sep 2026; [D-55]): a look-up chart, an analysis chart, and one that explains or shows something without a stated insight may have a descriptive title.
*When:* communication charts whose job is to answer, persuade or make aware (and explain or other ones that have an insight to state). *Why:* the audience may lack the background to interpret the data, is spared working out why it is shown the chart, and every chart carries an interpretation anyway. *Except:* none; the objections that stating it is biased, or that a good chart needs no words, are rejected. *Src:* RP5-1 [KT] p. 389–395; R0-10 p. 19–20, 26.

**G8.3** [O] [KT] In the title and callouts, answer as many as are known of four questions: what matters most; why the audience should care (unless obvious); what explains it; what the audience could or should do about it.
*When:* writing the title and callouts. *Why:* each answer moves the audience closer to acting. *Except:* answer only what is known; there may be no cause or recommendation. *Src:* R15-1 [KT] p. 400–401, 417.
**Good choice:** short, specific statements in the audience's terms, with the numbers that matter ("West missed its target by 12 %"), each answering one of the questions, none going beyond what the chart and its context support.

**G8.4** [G] (fig.) Make the title the recommended action if there is one, otherwise the most important statement; put the "what and why" beside the highlighted element in a callout with an arrow to it; put the explanation in a callout pointing at where the cause shows (the month it began).
*When:* charts with several answers. *Why:* each answer sits next to the evidence for it. *Except:* none. *Src:* R15-2 (fig.) p. 400, 417.
**Consider:** whether a recommended action is known (it takes the title); where on the chart the evidence for each answer lies (each callout sits there); how many callouts fit in empty space (G8.7).

**G8.5** [O] State "nothing is wrong" insights too (all values normal, stable, as expected).
*When:* the data hold no problem. *Why:* otherwise the audience wastes time looking for one. *Except:* none. *Src:* R15-3 p. 401.
**Good choice:** say what is normal and by what standard ("All regions within 2 % of budget").

**G8.6** [O] State your interpretation even when informed readers may read the data differently.
*When:* contested readings. *Why:* the disagreement exposes different understandings worth resolving; the audience may reject the explanation, which is fine. *Except:* none. *Src:* RP5-4 p. 394, 401.
**Good choice:** the reading you can defend from the data and its context, stated plainly, with a callout pointing at its evidence.

**G8.7** [L] (fig.) Write callouts in the comment style (G6.3) as short lines, with a thin arrow of the same colour stopping just short of its target. Place them in empty space, never over marks; a chart-wide statement needs no arrow, and one callout may send arrows to several marks or panels.
*When:* every callout. *Why:* short text near its evidence is read; arrows tie claim to mark. *Except:* none. *Src:* RP3-7 p. 371, 375; figures p. 371–417.

**G8.7s** [S] (fig.) Set callouts as centred lines of about 18–28 characters, two to five lines, with arrows of one smooth bend.
*When:* as G8.7. *Src:* as G8.7.

**G8.8** [O] (fig.) Optionally draw a thin curved grey arrow from the end of an insight title to the mark it names.
*When:* the title names one mark. *Why:* it points the reader straight at the evidence. *Except:* none. *Src:* figures p. 19–20, 376, 393.
**Good choice:** draw it when the eye would not find the named mark at once (a small bar, one line among several); leave it out when the mark is already highlighted and obvious.

### Highlighting

**G8.9** [G] [KT] Highlight the element or elements the insight is about with any of: stronger colour, an arrow or icon, text weight or colour, a border or enclosure, a glow or tinted background, labels on those elements only, a thicker line. Methods may be combined. Provisional default: thicker line for lines, stronger colour for bars and slices, a light cell fill in tables [D-9].
*When:* the insight names particular elements. *Why:* the point of the chart becomes obvious. *Except:* none. *Src:* R15-5 [KT] p. 403–405; R3-7 p. 119.
**Consider:** the mark type (lines: thickness; bars and slices: stronger colour; tables: a light fill [D-9]); whether colour already separates many series (a slot's strong shade can be another slot's base colour, so thickness or a border serves better); how many elements are targets. Which elements to highlight is open: those the insight names (G0.8).

**G8.10** [L] Never highlight by giving an element a different hue or a larger size (bigger dot, wider bar, larger font), and never by exploding a pie slice.
*When:* every highlight. *Why:* a new hue reads as another category, a bigger mark as a bigger value. *Except:* none. *Src:* R15-6 p. 404; R7-38 [KT] p. 293–294.

**G8.11** [L] (fig.) Draw the highlight methods as the book does (table). [D-9]
*When:* applying G8.9. *Why:* these forms stay within G8.10. *Except:* none. *Src:* R15-5 (fig.) p. 403–405; figures p. 213–214, 346–348, 400.

| Method | Form |
|---|---|
| stronger colour | a darker, more saturated shade of the target's own hue; in tables, others grey and the target black [D-6] |
| arrow or icon | a small filled marker (a red dot) beside or above the target; a dot before a table value |
| text | the target's label or value in bold and darker |
| border | a stroke in the darker shade round a bar or slice; a thin box round a table value |
| glow or background | a soft light halo under the target (on a line, only the segment the insight is about), or a tinted cell or band; in the plain style drawn flat as a wide translucent underlay (readers') |
| selective labels | value labels on the targets only (the maximum and the minimum) |
| thickness | the target line markedly thicker than the others |

**G8.11s** [S] (fig.) Highlight strengths: a border about 2 px wide; a thickened line about 2–2.5 × as thick; a light-orange glow [D-9]. The glow's hue is chosen per chart (owner, 30 Sep 2026): a pale tint of the target's own hue, or its true complement (turned 180° in OKLCH) at the same pale lightness, the stronger emphasis, where a contrasting hue would not be taken for another category; the book's light orange is the complement of its blue; the style fixes only the lightness and the chroma's cap.
*When:* as G8.11. *Src:* as G8.11.

### Comparison values

**G8.12** [G] [KT] Add comparison values whenever the audience may lack the background to judge the numbers, chosen by G0.19; leave them out when the insight is itself a comparison among the items shown ("South had the highest donations").
*When:* every communication chart. *Why:* without them the audience cannot tell how to feel about the numbers, so it does not act. *Except:* none. *Src:* R15-7 [KT] p. 405–406, 408; R15-8 p. 406–407.

**G8.13** [L] (fig.) Draw per-category targets as short dark marks across the bars (a darker shade of the bar's hue), labelled once with thin leader lines ("Target", "Actual"); draw a single comparison value as a line across the plot, labelled at its right end [D-21].
*When:* comparison values are shown. *Why:* the comparison sits on the mark it qualifies. *Except:* none. *Src:* R11-4 p. 353; figures p. 405, 417.

**G8.13s** [S] (fig.) Target marks half to the full bar width.
*When:* as G8.13. *Src:* as G8.13.

**G8.14** [L] (fig.) In a deviation chart, colour each bar by whether its side is good or bad and colour the extreme scale labels to match ("12% over budget" in red, "−10% under budget" in green); in a table, put a small red dot before each value that misses its target and state the targets under the columns.
*When:* good/bad readings (G4.16). *Why:* problems show before any label is read. *Except:* none. *Src:* R15-11 (fig.) p. 409; R3-17 (fig.) p. 131.

### Calculated values

**G8.15** [G] [KT] For every chart, ask whether a calculated value (a deviation, a ratio, an index) would make the insight more obvious than the raw values; never leave the audience to add, subtract or divide in their heads.
*When:* every chart. *Why:* mental arithmetic is slow and error-prone; if readers must calculate, the chart needs redesigning. *Except:* none. *Src:* R15-13 [KT] p. 410–416; R0-09 p. 37; R7-09 p. 242–243.

**G8.16** [G] Show deviations (actual minus comparison value) when the insight is about beating or missing a target; sort by the deviation and say so in the unit phrase ("$25,000 above target").
*When:* performance against targets. *Why:* raw bars with target marks leave "who beat the target by most" to arithmetic. *Except:* none. *Src:* R15-14 p. 410–411.

**G8.17** [G] Show ratios (accidents per 100K work hours) when comparing entities of different size.
*When:* counts across entities of different size or exposure. *Why:* raw counts can give the wrong answer, not just a less obvious one. *Except:* none. *Src:* R15-15 p. 412–413.

**G8.18** [G] Show indexed values when comparing the patterns of change of series that are not directly comparable: rebase every series to a common value at a common start (0 % change, 100, $10,000) and state the base in the unit phrase ("value of $10,000 invested on Feb. 4").
*When:* share prices of different companies; spend against revenue. *Why:* the patterns become comparable on one scale; the cost is that actual values are no longer shown. *Except:* none. *Src:* R15-16 p. 414–416; R2-17 p. 72.

**G8.19** [G] When the insight needs both raw and calculated values, show both as panels of a merged chart sharing the categories.
*When:* e.g. accidents and accidents per 100K hours. *Why:* each panel keeps its own scale. *Except:* none. *Src:* R15-17 p. 417.

**G8.20** [L] [KT] Let the graphics, not printed numbers, carry the message: if the chart works only because readers read value labels (percentages on close pie slices, a label on every funnel layer), change the chart. Check: hide all value labels; if the values can no longer be judged, the encoding is wrong (readers').
*When:* every chart with value labels. *Why:* reading quantities from shapes is faster and more informative than reading numbers. *Except:* labels as a supplement (G7.27). *Src:* R7-10 [KT] p. 241; R12-5 [KT] p. 360.

### Engagement and review

**G8.21** [L] [KT] Make charts engaging by making them obvious and easy (graphs rather than tables; types, palettes and scales chosen for the insight; highlighting; stated insights and actions; good/bad colours; comparison values; complicated charts split into simpler ones), never by eye candy (G3.9, G4.4, G1.7).
*When:* every chart. *Why:* boredom comes from an unclear point or a chart too complicated to be worth decoding, not from plainness. *Except:* none. *Src:* RP6-2 [KT] p. 420–421, 425; RP6-1 [KT] p. 420, 424.

**G8.22** [G] To win over an audience that equates good with fancy, have them answer basic questions with a fancy and a useful version of the same chart, without asking which they prefer.
*When:* stakeholders demand eye candy. *Why:* after three or four rounds most people stop equating fancy with useful. *Except:* a few stay unconvinced. *Src:* RP6-3 p. 421–423.

**G8.23** [L] [KT] Accept a chart only if it does its stated job quickly (least reading time), comfortably (no new chart type to learn, no rotated text, no mental arithmetic) and safely (little risk of misreading).
*When:* final review. *Why:* this is the book's definition of a good everyday chart; other qualities count only as means to it. *Except:* an inherently complex message may need real effort, but no more than necessary. *Src:* R0-05 [KT] p. 34–37.

**G8.24** [L] Judge a design by answering the audience's real questions with it, not by how simple it looks.
*When:* comparing designs. *Why:* a simpler-looking chart (filtered, animated) can be harder to use. *Except:* none. *Src:* R13-3 p. 380–382, 386.

**G8.25** [L] Try several versions and render each one rather than imagining it.
*When:* designing. *Why:* seeing the result reveals what imagination misses. *Except:* none. *Src:* RP7-1 p. 427.

**G8.26** [G] Have at least one other trained person review any chart going into a major publication (a board presentation, a press release).
*When:* important charts. *Why:* others catch what the maker cannot see. *Except:* none. *Src:* RP7-2 p. 427–428.

**G8.27** [L] Before delivering, recompute from the data every number, date, name and claim that the chart shows (labels, title, callouts, notes) and that the message sent with it makes; a fact taken from neither the data nor the request (a year, a date, a cause) is an assumption, and is said to be one (G0.10). [D-58]
*When:* every chart, table and message. *Why:* a well-drawn chart that states a wrong figure or date misleads as surely as a truncated bar; in the evaluation of 1 Oct 2026, nine of the eleven misleading responses were wrong statements, not wrong drawings (`eval/REPORT.md`). *Except:* none. *Src:* owner, 1 Oct 2026.

## G9 Checklist

Run these checks on a finished chart. Each names the rules it tests and shows its level. A failed [L] check means fixing the chart; only a rebuilt bad version of the book passes, by an override that stays in the report. A failed [G] check means following the procedure, or recording the reason for the departure (G0.3). An [O] check is a question for the maker or a reviewer, which no generator answers. The checks of a style's values ([S]) are run once per stylesheet, not per chart, and are listed under Style lint. The book's own before-and-after checklist (R5-28, p. 158) is covered by items 5, 27–30, 37–38, 49–50, 54 and 56 and by style checks S1 and S3; its change from a serif to a sans-serif face is superseded by the owner's choice of New Computer Modern. `SPEC.md` §3.6 tables where the generator makes each check (a refusal, a default of the layout, the model lint, the SVG lint, the style lint) and which are left to a person; `python3 -m chartgen lint` runs the checks that can be read off a drawing on any SVG in the chart vocabulary, hand-made ones included.

**Brief and message**

1. [L] The job and the insight are recorded, or the chart is one of a question-titled set. (G0.4, G0.5, G0.7, G0.8)
2. [L] Every assumption made for a missing brief field is recorded. (G0.10)
3. [L] When the job is to answer, persuade or make aware, the title or a callout states the insight, not only "X by Y"; a look-up chart may have a descriptive title. (G8.2)
4. [O] The title and callouts answer what is known of the four questions, in the audience's terms. (G8.3)
5. [L] The title block names measure, breakdown and full period with the year. (G6.8)
6. [G] Every element the insight names is highlighted, by the default method for its mark or a stated one. (G8.9)
7. [L] Nothing is highlighted by a new hue, a larger size or an exploded slice. (G8.10)
8. [O] The chart shows only what this audience needs, and plays down no problem it would want to know about. (G0.6, G0.9)

**Chart type**

9. [G] The type is the one G1.10 gives for the declared data properties, or the reason for another is recorded. (G1.10, G1.2)
10. [L] A pie, stacked chart or treemap passes all four data checks. (G0.12, G0.13, G0.14, G0.15)
11. [G] A pie has at most 6–7 parts. (G1.36, G1.39)
12. [L] There is no donut and no exploded slice. (G1.61, G7.30)
13. [L] No standard line joins irregular timestamps; no line joins unordered categories. (G1.50, G1.58)
14. [G] Two-period data are an arrow chart. (G1.46)
15. [L] Absolute changes between two periods appear with the % changes, and never as clustered bars. (G1.47, G1.61)
16. [L] The type is not on the avoid-list, or its exception is declared. (G1.61, G1.62)
17. [G] Mark types are mixed only for series with different roles. (G1.60)
18. [L] There is no second value axis except an exact conversion. (G2.31, G2.33)
19. [G] A table stands alone only for lookup, export or non-comparable values. (G1.12)
20. [L] Static output has no filter or animation. (G1.22, G1.23)
21. [G] No plot holds more than five lines unless they rarely cross and are all labelled directly. (G1.20)

**Scales**

22. [L] Bars, stacks, areas and waterfalls start at zero in the main chart. (G2.6)
23. [G] A main scale that does not include zero rests on a theoretical minimum or a declared irrelevance of zero, and carries only lines or dots; a narrow range that matters otherwise gets an inset. (G2.5, G2.7, G2.8, G2.11)
24. [G] No scale starts near zero without reaching it. (G2.9)
25. [L] No scale passes a theoretical limit. (G2.13)
26. [G] The free end has 10–20 % of room (at least 5 % after snapping). (G2.12, G2.14)
27. [L] The step is 1, 2 or 5 × 10ⁿ (2.5 warns), and the intervals are equal. (G2.15, G2.16)
28. [G] The number of intervals suits the precision the job needs, with at most about eight labels. (G2.17)
29. [G] If there are fewer values than scale labels, the values are labelled and the scale removed. (G2.18)
30. [L] All scale labels share one number of decimals and one format. (G2.24)
31. [L] A unit phrase exists, is horizontal, sits by its scale and is specific. (G2.19, G2.21)
32. [L] There is no broken scale, and no log scale for a general audience. (G1.61, G2.35)
33. [L] Each scale carries one unit. (G2.30)
34. [L] Time runs left to right on a multi-level axis with horizontal labels, equal bins, and no unmarked incomplete period. (G2.36, G2.37, G2.41, G2.40)
35. [G] Nominal categories are sorted by value, with the catch-all and missing categories last. (G2.1)
36. [L] Ordinal categories and time keep their own order. (G2.2)

**Non-data elements**

37. [L] There are no frames, outlines, tick marks or value-axis line. (G3.3, G3.4, G3.5)
38. [L] Gridlines lie beneath the marks, perpendicular to the value direction, and none runs along a category scale. (G3.6, G3.8)
39. [L] There is no 3-D or perspective, and no effect moves or distorts a mark. (G3.9)

**Colour**

40. [L] Every colour difference means something; a single series has one colour. (G4.1, G4.3, G4.4)
41. [G] The palette type matches what colour encodes. (G4.5)
42. [L] Special categories take their reserved colours, never a regular slot. (G4.15)
43. [L] There are at most 8 regular colours, unless all series are labelled directly with similar colours apart. (G4.19, G4.20)
44. [L] A diverging palette has a dividing value declared by the requester. (G0.20, G4.11)
45. [G] Good and bad readings are coloured red and green. (G4.16)
46. [L] Colours that must be told apart survive the colour-blindness simulation, or a safe version is produced. (G4.18)
47. [L] Each category keeps its colour across the set of charts. (G4.22)
48. [L] Every line has its own colour, and no dash pattern identifies a series. (G4.27, G7.10)

**Labels and legends**

49. [G] Series are labelled directly where the placement test passes; otherwise a legend sits above the plot (right of a time-series line chart). (G5.1, G5.3, G5.7, G5.10, G5.11)
50. [L] Legend entries follow the order and direction of the colours in the chart. (G5.8, G5.9)
51. [L] Direct labels do not overlap and keep the order of the line ends. (G5.2)
52. [L] Pie slices are labelled outside, with no legend. (G7.27)

**Text, numbers and dates**

53. [L] At most four text styles are in use, and comments look plainly different from labelling. (G6.2, G6.3)
54. [L] All text is horizontal. (G6.6)
55. [L] Every text element meets the size and contrast floors at final size, labels on coloured fills included. (G6.4, G6.5, G7.4)
56. [L] Full numbers are grouped in threes; hidden digits are marked; numbers are rounded, not truncated; each set shares one format. (G6.14, G6.12, G6.15, G6.16)
57. [L] No date has a numeric month or two-digit year unless the audience's format requires it; date columns have fixed-width parts. (G6.19, G6.20, G6.21)

**Chart-specific**

58. [L] Every empty bar carries a note saying what it means. (G7.2)
59. [L] Missing values leave gaps in lines, never zeros or dashed bridges. (G7.9, G0.16)
60. [L] Point markers appear exactly where they are required. (G7.11)
61. [L] No line through data points is smoothed. (G7.17)
62. [L] Tables have no vertical rules, at most two horizontal rules, no zebra stripes, no bold header or totals, the unit in the title, and aligned columns. (G7.38, G7.40, G7.43, G7.45, G7.41)
63. [G] Small multiples share one scale, unless the message compares the shapes of series of very different size. (G7.48)
64. [L] Small multiples label each dimension once. (G7.49)
65. [L] Maps: a choropleth only for regions of similar size; bubbles translucent, with outlines. (G1.27, G7.54)
66. [G] Maps: no bubbles or bars on regions where some values are very small. (G1.30, G1.31)

**Message**

67. [G] Comparison values appear where the audience lacks context, and none is doubtful. (G8.12, G0.19)
68. [G] A deviation, ratio or index has been considered, and used where it makes the insight more obvious. (G8.15, G8.16, G8.17, G8.18)
69. [L] With all value labels hidden, the values can still be judged from the graphics. (G8.20)
70. [L] The chart answers the audience's real questions quickly, comfortably and safely. (G8.23, G8.24)
71. [L] Every number, date, name and claim on the chart, in its notes and in the message sent with it has been recomputed from the data, and every fact taken from neither the data nor the request is said to be an assumption. (G8.27, G0.10)
72. [L] Every mark has its note under the chart and every footnote its mark; nothing the reader needs to read the chart is left to the covering message. (G6.24)

**Style lint** (once per stylesheet, by `chartgen style`)

- S1. [S] Non-data lines: the grey is lighter than every data colour, with a contrast of at most 1.5 : 1 against the page, and the width is thin beside the data strokes. (G3.2, G3.2s)
- S2. [S] The plain style is flat: no shadow, bevel, gloss, dark background, fill behind the plot, or gradient other than the truncation fade. (G3.9s)
- S3. [S] The regular slots are muted (OKLCH chroma at most 0.15) and balanced (lightness spread at most 0.12 among colours used together), with no grey, black or red among them. (G4.2, G4.6, G4.15, G4.17, G4.28)
- S4. [S] Slots 1–5, and the colour-blind-safe good/bad pair, stay distinguishable under protanopia, deuteranopia and tritanopia (provisionally an OKLab distance of at least 0.06 [D-34]). (G4.18, G4.28)
- S5. [S] Sequential ramps darken towards "more" on the page colour; diverging ramps meet at a quiet neutral, with no bright colour near it. (G4.12, G4.13, G4.14, G4.28)
- S6. [S] The labelling face has tabular, lining digits. (G6.1, G6.1s)
- S7. [S] The style defines at most four text styles for labelling, and a comment style in a colour used for nothing else. (G6.2, G6.2s, G6.3, G6.3s)
- S8. [S] Every text colour of the style reaches 4.5 : 1 against the page, and every text size meets the floors of G6.4. (G6.4, G6.5, G6.5s, G7.2s)
- S9. [S] Bars fill more than half their band, so gaps are narrower than bars. (G7.1, G7.1s)
- S10. [S] Series lines are clearly heavier than gridlines (2–3 ×), an emphasised line about twice a series line, and point markers about twice the line width. (G7.12, G7.13s, G7.11s)

## Index of sources

Each rule of the notes, with the guide rules that cite it.

- R0-01 G0.2; R0-02 G0.1; R0-03 G1.4; R0-04 G0.3; R0-05 G8.23; R0-06 G2.4, G2.19, G6.6; R0-07 G0.4, G0.10; R0-08 G0.5; R0-09 G8.15; R0-10 G8.2; R0-11 G4.7; R0-12 G0.16, G7.2; R0-13 G1.11; R0-14 G1.49, G7.18; R0-15 G2.26; R0-16 G6.21, G7.38, G7.41, G7.45.
- R1-01 G3.1; R1-02 G3.2; R1-03 G3.3; R1-04 G3.4; R1-05 G3.5; R1-06 G3.6; R1-07 G3.7; R1-08 G7.38, G7.40; R1-09 G3.8.
- R2-01 G0.18, G2.1; R2-02 G0.18, G2.2; R2-03 G2.3; R2-04 G2.4; R2-05 G2.22; R2-06 G2.23, G6.12; R2-07 G2.19, G2.20; R2-08 G6.6; R2-09 G2.15; R2-10 G2.16; R2-11 G2.21; R2-12 G6.8; R2-13 G2.30; R2-14 G1.61, G2.31; R2-15 G2.33; R2-16 G2.32, G7.6; R2-17 G8.18; R2-18 G1.61, G2.26; R2-19 G1.61, G2.35; R2-20 G2.26, G2.27, G2.29; R2-21 G2.24; R2-22 G2.5; R2-23 G2.7, G2.27, G2.28; R2-24 G1.57, G2.6, G2.8; R2-25 G2.9; R2-26 G2.11; R2-27 G2.12; R2-28 G2.13; R2-29 G2.14; R2-30 G2.14; R2-31 G1.14, G2.18, G7.46; R2-32 G2.18, G7.4; R2-33 G2.17; R2-34 G2.17; R2-35 G3.4, G3.5; R2-36 G2.10; R2-37 G2.36; R2-38 G2.36; R2-39 G2.37; R2-40 G2.38; R2-41 G2.39; R2-42 G2.40; R2-43 G2.41.
- R3-1 G4.1; R3-2 G4.2; R3-3 G4.3, G5.6; R3-4 G4.4; R3-5 G0.18, G4.5, G4.6; R3-6 G4.6; R3-7 G4.26, G8.9; R3-8 G0.18, G4.5, G4.7; R3-9 G4.14; R3-10 G4.8; R3-11 G0.18, G4.5, G4.9; R3-12 G4.5, G4.10; R3-13 G0.10, G0.20, G4.5, G4.11; R3-14 G4.12; R3-15 G4.13; R3-16 G0.18, G4.15, G7.16; R3-17 G0.20, G4.16, G7.22, G8.14; R3-18 G4.17, G7.22; R3-19 G4.18; R3-20 G4.18; R3-21 G4.19; R3-22 G4.20; R3-23 G4.21; R3-24 G4.22; R3-25 G4.23; R3-26 G4.24; R3-27 G4.25.
- R4-1 G5.1, G5.4; R4-2 G5.3, G5.7; R4-3 G5.7; R4-4 G5.8; R4-5 G5.9; R4-6 G5.10; R4-7 G5.11; R4-8 G4.9, G5.12; R4-9 G5.13; R4-10 G5.14.
- R5-1 G6.1; R5-2 G6.1; R5-3 G6.1; R5-4 G6.1; R5-5 G6.3; R5-6 G6.2; R5-7 G6.4; R5-8 G6.5; R5-9 G6.10; R5-10 G6.11; R5-11 G6.12; R5-12 G1.14, G6.13; R5-13 G6.12; R5-14 G6.14; R5-15 G6.14; R5-16 G6.15; R5-17 G2.24, G6.16; R5-18 G2.25, G6.17; R5-19 G6.18; R5-20 G6.19, G6.22; R5-21 G6.20, G6.22; R5-22 G6.21; R5-23 G6.21; R5-24 G2.37, G2.38, G6.23; R5-25 G0.2; R5-26 G2.4, G6.6, G6.7; R5-27 G6.8, G6.22; R5-28 G3.1, G5.5.
- R6-1 G0.10, G0.17, G1.2; R6-2 G1.49, G7.18; R6-3 G0.17, G1.50, G2.42; R6-4 G1.51; R6-5 G1.51; R6-6 G1.51; R6-7 G1.52, G7.24; R6-8 G0.17, G1.53, G7.19; R6-9 G1.53; R6-10 G1.54; R6-11 G1.54, G2.42, G7.20; R6-12 G1.54; R6-13 G1.48; R6-14 G1.46; R6-15 G1.46, G7.21, G7.22; R6-16 G1.46, G1.61, G7.23; R6-17 G1.47, G7.21; R6-18 G1.4, G1.5; R6-19 G1.5; R6-20 G1.6; R6-21 G7.8; R6-22 G7.9; R6-23 G0.16, G7.9; R6-24 G7.9, G7.10; R6-25 G7.11; R6-26 G7.11; R6-27 G7.11; R6-28 G4.27, G7.50; R6-29 G7.13; R6-30 G7.14; R6-31 G7.10; R6-32 G5.1, G5.2, G5.3; R6-33 G7.15; R6-34 G4.15, G7.16; R6-35 G1.20; R6-36 G1.20, G7.35; R6-37 G7.48; R6-38 G2.36; R6-39 G1.61, G7.17; R6-40 G7.17, G7.24; R6-41 G1.58; R6-42 G1.58; R6-43 G1.55.
- R7-01 G1.1; R7-02 G0.12; R7-03 G0.13; R7-04 G0.15, G7.3; R7-05 G0.14; R7-06 G1.35; R7-07 G1.36; R7-08 G1.37, G7.31; R7-09 G8.15; R7-10 G8.20; R7-11 G1.38, G7.33; R7-12 G1.38, G3.7, G4.15, G7.3; R7-13 G1.39; R7-14 G1.39, G7.7; R7-15 G1.39, G7.34; R7-16 G7.34; R7-17 G0.13, G1.39, G2.1, G4.15, G4.21, G7.28, G7.31; R7-18 G2.33, G2.34; R7-19 G1.41; R7-20 G1.41; R7-21 G1.41; R7-22 G0.13, G1.42; R7-23 G1.42, G2.30; R7-24 G4.22, G5.8, G7.5, G7.31; R7-25 G1.43, G1.45; R7-26 G1.43; R7-27 G1.43; R7-28 G1.45; R7-29 G1.44; R7-30 G1.44, G7.32; R7-31 G1.5; R7-32 G7.25; R7-33 G3.3, G7.26; R7-34 G5.1, G7.27; R7-35 G7.28; R7-36 G0.16, G7.29; R7-37 G7.29; R7-38 G4.26, G7.30, G8.10; R7-39 G1.61; R7-40 G1.40.
- R8-1 G1.24; R8-2 G1.25; R8-3 G1.25; R8-4 G1.25, G7.56; R8-5 G1.27, G1.61; R8-6 G1.28, G7.53; R8-7 G7.53; R8-8 G1.29, G7.55; R8-9 G1.30; R8-10 G7.54; R8-11 G1.31, G7.55; R8-12 G1.31; R8-13 G1.26, G7.55; R8-14 G1.26; R8-15 G1.32; R8-16 G1.33; R8-17 G1.33; R8-18 G1.34.
- R9-1 G1.9; R9-2 G1.1; R9-3 G1.9; R9-4 G1.7; R9-5 G2.6; R9-6 G2.7, G2.27, G2.28; R9-7 G0.16, G7.2; R9-8 G7.1; R9-9 G2.1, G2.2; R9-10 G2.4, G6.7; R9-11 G1.56, G2.32, G7.6; R9-12 G5.10, G7.5.
- R10-1 G1.11; R10-2 G1.13; R10-3 G1.13; R10-4 G1.13, G2.26, G2.27, G2.29; R10-5 G1.13, G2.31; R10-6 G1.13; R10-7 G1.14, G7.46; R10-8 G1.15; R10-9 G1.16, G7.47; R10-10 G1.17; R10-11 G1.12; R10-12 G6.10, G6.13, G7.37; R10-13 G7.38; R10-14 G7.39; R10-15 G7.39; R10-16 G7.40; R10-17 G7.41; R10-18 G7.41; R10-19 G7.41; R10-20 G7.41; R10-21 G6.9, G7.42; R10-22 G7.43; R10-23 G7.44; R10-24 G6.21; R10-25 G7.45.
- R11-1 G1.7; R11-2 G1.59; R11-3 G1.60; R11-4 G1.60, G5.5, G7.57, G8.13; R11-5 G1.60, G2.30, G7.57.
- R12-1 G1.46, G1.61, G2.31; R12-2 G1.61; R12-3 G1.61; R12-4 G1.61, G7.58; R12-5 G1.61, G7.59, G8.20; R12-6 G1.61, G7.36; R12-7 G1.61, G3.9; R12-8 G1.61; R12-9 G1.62; R12-10 G1.7.
- R13-1 G1.22; R13-2 G1.22; R13-3 G8.24.
- R14-1 G1.23; R14-2 G1.23; R14-3 G1.23, G7.16.
- R15-0 G8.1; R15-1 G8.3; R15-2 G8.4; R15-3 G8.5; R15-4 G0.5, G4.7; R15-5 G8.9, G8.11; R15-6 G4.26, G8.10; R15-7 G0.19, G8.12; R15-8 G8.12; R15-9 G0.19; R15-10 G0.20; R15-11 G0.20, G4.16, G8.14; R15-12 G4.18; R15-13 G8.15; R15-14 G8.16; R15-15 G8.17; R15-16 G8.18; R15-17 G8.19.
- RP2-1 G1.3; RP2-2 G1.1; RP2-3 G1.2; RP2-4 G1.5; RP2-5 G1.7; RP2-6 G1.3; RP2-7 G1.8, G1.10; RP2-8 G1.8, G1.10; RP2-9 G1.8.
- RP3-1 G0.11; RP3-2 G1.13, G1.18; RP3-3 G1.18; RP3-4 G1.19; RP3-5 G1.20; RP3-6 G1.21; RP3-7 G7.52, G8.7; RP3-8 G7.48, G7.49; RP3-9 G5.15, G7.49.
- RP5-1 G8.2; RP5-2 G0.5; RP5-3 G0.6; RP5-4 G8.6; RP5-5 G0.7; RP5-6 G0.7.
- RP6-1 G1.7, G1.61, G3.9, G4.4, G8.21; RP6-2 G8.21; RP6-3 G8.22; RP6-4 G0.9.
- RP7-1 G8.25; RP7-2 G8.26; RP7-3 G0.8, G0.10; RP7-4 G1.61; RP7-5 G0.1.

## Locked by the style

The [S] rules: the checklist a stylesheet is written against. Each row gives what the style fixes and the book's value (or the owner's, where said). The stylesheet (`style/*.css`, being written) holds the current value; the token column names where it lives at the time of writing (SPEC §4.6), and "later" marks a chart type not yet built (SPEC §7).

| Rule | What the style fixes | The book's value | Token |
|---|---|---|---|
| G2.25s | gap from the scale-label column to the plot | about 0.9 em | `--scale-gap` |
| G2.27s | inset frames and connectors | rounded corners; light grey; about 3 × the gridline width | `--inset-radius`, `--inset-frame`, `--inset-frame-width` |
| G2.28s | extent of the truncation fade | about the lower half of the visible bar | `--inset-fade` |
| G3.2s | non-data lines | grey ≈ #D9D9D9, about 0.08 em wide | `--grid`, `--grid-width` |
| G3.9s | the plain style's flatness | no shadows, bevels, gradients or gloss; white page; no fill behind the plot | `--page` |
| G4.15s | the special greys | average or total line #595959 with a bold label; all others #968C8B; missing #A6A6A6; total bar #A6A6A6 | `--average`, `--all-others`, `--missing`, `--total` |
| G4.28 | the plain style's colours: categorical slots and their order, strong and pale shades, special greys, good and bad, ramps, text, gridlines, comments | the table in G4.28 (the muted palette, decided: D-2) | `--series-*`, `--series-*-strong`, `--single*`, `--good`, `--bad`, `--good-cvd`, `--bad-cvd`, `--text`, `--grid`, `--comment`; ramps later |
| G5.2s | end labels | about 0.5 em from the point; text grey; one line-height apart | `--end-label-gap`, `--end-label-pitch`, `--text` |
| G5.12s | group names in a grouped legend | italic | later |
| G6.1s | the typeface | New Computer Modern, set by Typst (owner) | the `.prose` rule |
| G6.2s | the set of text styles | two: a bold title and one regular style | the `.prose` and `.title` rules |
| G6.3s | the comment style | New Computer Modern italic, #3F63B0 (provisional) | `--comment` |
| G6.5s | the labelling colour | #595959 | `--text` |
| G6.9s | the title | bold, 1.2–1.3 × the label size; left, flush with the scale labels (provisional) | `--title-size`, `--title-align`, `--title-align-insight` |
| G6.21s | the face of date columns | the labelling face, accepting drift (provisional) | later |
| G7.1s | bar width in its band | about 0.55 (0.5–0.63) | `--band-fill` |
| G7.2s | notes on empty bars | small italic, in a grey that meets the contrast floor | `--note-size`, `--note` |
| G7.5s | gap between clusters | one to two bar widths | `--cluster-fill` |
| G7.11s | point markers | filled dots about 2 × the line width | `--marker-size` |
| G7.12 | series lines | about 0.2 em (2–3 × the gridline), round joins and ends | `--line-width` |
| G7.13s | an emphasised line | about 2 × as thick | `--line-width-highlight` |
| G7.16s | reference lines | #595959; an average's end label bold | `--average` |
| G7.18s | dots | about 0.5 em; open circles in categorical dot plots, about 0.85 em with a ring about 0.12 em | `--dot-size`; `--category-dot-size`, `--dot-ring` |
| G7.20s | thin bars | 1.2–1.5 × the line width | `--event-bar-width` (1.4 ×) |
| G7.21s | the arrow chart's key | tiny italic | the `.arrow-key` rule: `--note-size`, `--note` |
| G7.26s | gaps between pie slices | about 0.7 % of the diameter | later |
| G7.39s | table spacing | gutters 1.5 em, 3 em before a Total column, row pitch 2.15 em | later |
| G7.42s | a table title | bold, dark grey, a little larger than the body | later |
| G7.44s | the table highlight | a fill of #E8E8E8 | later |
| G7.51s | separators between small multiples | 1 px | later |
| G7.53s | map dots | about 5 % of the map width | later |
| G7.54s | map bubbles | about 45 % opaque | later |
| G7.57s | reference markers on bars | about 0.2 bar-widths thick | later |
| G7.58s | the values of action dots | bold | later |
| G8.7s | callouts | centred lines of 18–28 characters, two to five lines; arrows with one smooth bend | later |
| G8.11s | highlight strengths | a border about 2 px; a line 2–2.5 × as thick; a light-orange glow | `--line-width-highlight`; the others later |
| G8.13s | target marks | half to the full bar width | later |

The stylesheet also fixes values that no rule states: the label size (the book gives none; the owner's plain style sets labels at 10 pt at the 368 pt text width), the line pitch, the gaps round labels and titles, and a default plot aspect, which a chart may change within limits as an open choice (SPEC §5.2). A few tokens hold thresholds of guided procedures rather than looks (`--unit-min-gridline` for G2.20, `--end-label-max-shift` for G5.3); they belong to those [G] rules, not to the style.

## Guided decisions and the examples that should illustrate them

For each [G] rule, the test figures of `SPEC.md` §8 that would show the decision being made: P1-01 … P1-14 are the phase-1 examples (§8.2); the others are the notes' figures by file and number (§8.1 gives their phases), and §8.3 lists the unit tests. Where no figure shows the decision, or shows only one side of it, the last column names the example that is missing.

**G0 Before drawing**

| Rule | Decision | Shown by | Missing example |
|---|---|---|---|
| G0.7 | a question-titled set of views when the purpose is unknown | 13-15 F9 (views merged, a question above each panel); P2-69 | — |
| G0.19 | which comparison values are valid | 13-15 F12 (targets), F17 (an industry average); P2-48, P2-52, P2-56 (last year's values left out beside a target, D-48) | a doubtful benchmark (another industry, years old) left out: P2-56 (with a warning; refused where the spec asks to draw it) |

**G1 Choosing the chart type**

| Rule | Decision | Shown by | Missing example |
|---|---|---|---|
| G1.1 | the type, from several factors | 07 #1 (one data set, three messages, three types); 13-15 F10 | — |
| G1.4 | a familiar type for a general audience | 06 T17 (a) (a line over ordered categories only when data-savvy); 02 #9 (log scale, rejected) | — |
| G1.5 | an accurate unfamiliar type, taught | 06 T5 (bumps, revealed step by step, with callouts); 06 T6 | — |
| G1.6 | the insight as text | 06 T6 (the prose alternative) | — |
| G1.8 | the group first, then the type | 07 #14 (breakdown over time); 06 T9 (two periods); P3-60–P3-79 (each path names its group) | — |
| G1.9 | a bar chart considered, a more specific type preferred | 08-12 #1 (map against bars); 06 T3 (bars against lines) | — |
| G1.10 | the whole procedure | 08-12 #19 (the decision trees, a case per leaf); the chooser tests of §8.3; P1-13 (`type: auto`); P3-60–P3-79 (the section "Choosing a chart", each chart with its path) | — |
| G1.11 | a graph by default | 00-01 F5 (table, then line chart); 08-12 #6; P3-46 (the table, and its `line` variant) | — |
| G1.12 | a table alone | 08-12 #10 (key metrics); P3-70 (values only looked up); P3-40, P3-41, P3-43, P3-45 (looked up), P3-42 (not comparable), P3-44 (exported) | — |
| G1.13 | no table for many values or outliers | 13-15 F1 (table against small multiples); P1-07 (an inset instead); P2-60 | — |
| G1.14 | a table beside the graph for exact values | 02 #16; 08-12 #7; P2-01 (under the clusters; beside turned bars at 480 pt) | — |
| G1.15 | a table for readers with impairments | none | a chart with such readers declared, drawn with its table |
| G1.16 | graphics built into a table | 08-12 #9; P3-47, P3-48, P3-49, P3-45 `heat` | — |
| G1.18 | the arrangement of small multiples | 13-15 F1, F2, F5; P2-60, P2-61 (with its runner-up), P2-62 | — |
| G1.19 | which variable is compared within a panel | 13-15 F2 (a) and (b); P2-61, P2-62 | — |
| G1.20 | many series: one plot, a stack or a heatmap | 03-04 #13; 06 T13; 06 T14; P2-64 (a stack, the spaghetti chart a variant); P2-60 (spaghetti variant); P3-51 (a heatmap of 51 series) | — |
| G1.21 | the type inside the panels | 13-15 F4 (a grid of bar charts); P2-63 | — |
| G1.24 | locations alone do not call for a map | 08-12 #1 | — |
| G1.25 | a map only when location matters more | 08-12 #1; P3-77 (the map routed, phase 4; `bars` the book's answer) | a map and a bar chart side by side, for "both matter" (G7.56) |
| G1.26 | location and time | 08-12 #18 (lines on regions against a line chart) | — |
| G1.28 | coloured dots | 08-12 #18 | — |
| G1.29 | tiles | 08-12 #18 | — |
| G1.30 | bubbles, not for tiny values | 08-12 #18 (one region a speck) | — |
| G1.31 | bars on regions | 08-12 #18 | — |
| G1.32 | a colour map for a narrow range | none | a map of values in a narrow band, as bubbles (differences vanish) and as coloured dots |
| G1.33 | the order among suitable map types | 08-12 #18 (one data set in every map type) | — |
| G1.34 | map rules for any 2-D layout | none | values on a floor plan or another non-geographic layout |
| G1.35 | bars for a precise comparison of parts | 07 #1 ("Smith > Jones"); P3-60; P3-64 `compare` | — |
| G1.36 | a pie for fractions | 07 #1 ("Smith ≈ ¼"); 05 #2; 07 #15; P3-01, P3-08, P3-12–P3-14, P3-18 (phase 3); P3-15 (13-15 F10: a pie, not columns, for ¼; `columns`); P3-60 `fraction` (chosen from the message); P3-61, P3-62, P3-63, P3-65 (the four checks, each sending a pie away) | — |
| G1.37 | a stacked bar or a pie for a subtotal | 07 #1 (stacked, "Smith + Jones > $300K"); P3-60 `subtotal` (stacked); P3-20 (the subtotal at the base, the others paled); P3-02 (a subtotal stated as a fraction, in a pie); P3-36 (the same for an audience averse to pies, a 100 % stack) | a subtotal stated as a fraction, in a pie (drawn in phase 3: P3-02) |
| G1.38 | a waterfall or bars for mixed signs | 07 #8; 07 #9; P3-63 (bars and a Total bar); P3-64 (a waterfall); P3-24, P3-25 (waterfalls: totals at declared positions, signed labels, good and bad); P3-37 (a running total below zero); P3-23 (the mixed stack refused) | — |
| G1.39 | many parts: All others, bars, wrapped bars, treemap | 07 #3; 07 #4; 07 #5; P3-03 (All others; fifteen slices), P3-04 (wrapped bars), P3-05, P3-06 (treemaps); P3-66 (fifteen parts as fractions: a treemap, chosen) | — |
| G1.40 | no pie for a pie-averse audience | none; P3-60 `pie-averse`, P3-67 `pie-averse` (phase 3) | the pie of 07 #1 for a pie-averse audience, drawn as a 100 % stacked bar [D-39] (chosen in phase 3: P3-60 `pie-averse`) |
| G1.41 | several totals: clustered, stacked or pies | 07 #12; P2-04 (clustered; `compare-totals`, stacked, phase 3); P3-11 (a pie per total, the rare case; `top-three`, pies of some parts refused); P3-67 (the totals compared: stacked bars, chosen); P3-27 (stacked bars, the totals labelled; `fractions`, 100 % stacks) | — |
| G1.42 | merged bars for missing parts or mixed measures | 07 #12 (top-3 variant); 07 #13; P2-67, P2-68; P2-04 `top-3` (clustered, the guided finding) | — |
| G1.43 | parts over time, with or without the total | 07 #14 (lines; lines with a total panel); P3-68 `parts`, `parts-and-total`; P3-28, P3-29 | — |
| G1.44 | stacked or 100 % stacked over time | 07 #14; P3-68, P3-68 `with-total`; P3-30–P3-33; P3-38 (a month not known), P3-33 `west-first` | — |
| G1.45 | areas or bars over time | 07 #14; P3-68, P3-68 `periods`; P3-30, P3-31 (areas, bars); P3-32, P3-33 (100 %); P3-38 and its `bars` | — |
| G1.46 | an arrow chart for two periods | 06 T9; 13-15 F18; P2-21 (its clustered bars refused); P3-69 (the slope chart's rare case, a change of rank; `change` refused) | — |
| G1.48 | the time procedure | 06 T1, T3, T6, T7, T8; P1-26, P1-27, P2-20, P2-23 | — |
| G1.49 | too few values: a dot plot | P1-13 (06 T1); P1-19 (02 #14, transactions per day); the chooser tests of §8.3 (11 of 12, 10 of 12); P1-10 `sparse` and `sparse-line` (06 T10, five months of twelve) | — |
| G1.51 | a line or bars for a regular series | 06 T3; 06 T4; P1-03; P1-10, P1-25 (06 T10, T18) | — |
| G1.52 | a bumps chart for ranks | 06 T5; P2-25 (the ranks as a line, a guided departure) | — |
| G1.53 | a step chart for irregular states | 06 T6 (P1-26, drawn to its as-of date; its line refused) | — |
| G1.54 | thin bars or aggregation for events | 06 T7 (P1-27: thin bars, the totals asked for, the line refused); 06 T8 (P2-20); P2-26 (events over one day); P3-71 (06 T7's weekly totals, Sunday weeks) | — |
| G1.55 | a line of regular summaries is fine | 06 T18 | — |
| G1.56 | bars, clustered or merged across categories | 02 #6; 08-12 #5; 08-12 #17; P2-65, P2-68 (merged); P2-01, P2-03, P2-06 (clustered); P3-78 (clustered, the book's 3-D z-axis flattened) | — |
| G1.57 | dots when the scale need not start at zero | P1-14 (02 #15); P1-20 (02 #14, client satisfaction) | — |
| G1.60 | mark types combined for different roles | 08-12 #11; 05 #8; P2-07, P2-08 (flat budget markers; `clustered` and `lines` variants) | — |

**G2 Scales**

| Rule | Decision | Shown by | Missing example |
|---|---|---|---|
| G2.1 | the order of nominal categories | 02 #1; P1-06 (missing last); P1-07 (Others last); 03-04 #12 (several series, sorted by total) | a chart sorted smallest first because the small values are the point |
| G2.3 | a category-axis title or none | 02 #1 (the title dropped) | categories whose names need a title (Low / Medium / High, "Threat level") |
| G2.4 | orientation and wrapping | P1-05 (02 #2); P1-06 and 08-12 #1 (names wrapped) | — |
| G2.5 | zero on the scale | P1-01 and every bar chart; 02 #14 (acceptable truncation) | — |
| G2.7 | zero plus an inset for a narrow band | P1-08 (08-12 #2; bars, and its variant `wide`); P1-09 (02 #13; lines); 02 #14 (P1-19, dots); the prompt when no fact asks (gen-01, days in each month) | — |
| G2.8 | truncation, for lines and dots only | 02 #14 (class scores, temperatures, client ratings: P1-20); P1-23 (06 T2 (a), a rating's arbitrary zero); P1-09 `truncated` (the book's volatile-looking version, a guided finding) | — |
| G2.9 | start at zero when the data come near it | 02 #12 (the implied fix) | — |
| G2.10 | zero optional when it does not mean "none" | 02 #14 (levels "Neutral" and "Very satisfied"); P1-14 | — |
| G2.11 | the lower limit when zero is not required | P1-14 (the theoretical minimum); 02 #14 | — |
| G2.12 | room above the data | 02 #15 (crop yield 0–60, not 0–50); the 02 D5 table of §8.3 | — |
| G2.14 | limits for negative and mixed data | 07 #8 (mixed signs); 02 #7 (index, −20 % to +80 %) | a scale of all-negative values |
| G2.17 | the number of intervals | 02 #17 (step 2 when the message is "North over $12M") | — |
| G2.18 | value labels or a scale | 02 #16 (average donation: labels win); 07 #6; P2-02 (labels, `scale` the departure); P2-11; P2-01 `labels-on-every-bar` (the scale wins) | — |
| G2.20 | the unit phrase on its own line | P1-03 (02 #15) | — |
| G2.22 | the magnitude stated once | P1-02 (02 #3); P1-06; P1-09 | — |
| G2.23 | the magnitude spelled out | P1-02 | the same scale under an audience's own abbreviations (K, M) |
| G2.26 | an inset for outliers | P1-07 (02 #9, 08-12 #4); 00-01 F7; 02 #10 (a) (a line leaving the inset's top) | — |
| G2.27 (the shared axis, D-56) | an aligned inset's labels repeated, or one axis shared when space is short | P1-09 and P1-19 (shared, compact main charts); P1-08 `wide` (repeated); P1-07 and P1-08 (repeated: not aligned above or below) | — |
| G2.29 | where the inset goes | P1-07 (inside the plot); P1-08 `wide` and P1-09 (above); P1-19 (below); P1-08 (beside horizontal bars, on their rows); 02 #10 (below, right) | the places outside the plot for an outlier inset that finds no room inside |
| G2.34 | a second "% of total" scale | 07 #2 | — |
| G2.38 | thinning the time labels | 02 #18 (every second day: P1-21; every third, P1-21 `narrow`); P1-13; 06 T8; P1-27 (a true time axis, every second day); P2-26 (every third hour) | — |

**G4 Colour**

| Rule | Decision | Shown by | Missing example |
|---|---|---|---|
| G4.5 | the palette type from what colour encodes | 03-04 #15 (eight bad and good pairs); 13-15 F10 | — |
| G4.7 | a sequential palette for ordered categories | 03-04 #5 (00-01 F3); P3-34 (ordered parts in steps of the ramp); P3-17 (a pie's ordered parts; `hues`) | — |
| G4.9 | grouped shades | 03-04 #7; P3-39 (stacks of crops, the countries by continent, with the grouped legend; `hues`, the book's first version) | — |
| G4.10 | a ramp for a quantity | 03-04 #8; P3-51; P3-50 and P3-52 `no-dividing-value`; P3-47 | — |
| G4.11 | diverging only with a dividing value | 03-04 #8; 03-04 #9; 08-12 #16; P3-50 (a rating's neutral middle, D-37 (c)), P3-52 (7 on 10, off the key's middle), each with a sequential variant; P3-53 (an average with no good side: blue below, orange above; `sequential` a departure, `diverging-about-the-middle` refused) | — |
| G4.16 | red and green for good and bad | 03-04 #11; 07 #8; 13-15 F13, F17; P2-50, P2-52, P2-55, P2-58, P2-48 (`red-green`: each bar by its own target) | — |
| G4.20 | more than eight colours, directly labelled | 03-04 #13 | — |
| G4.21 | fewer categories first | 07 #3 (All others); 03-04 #14 (multi-hue ramps) | — |
| G4.23 | colours the audience already knows | none | categories with conventional colours (parties, teams) kept over the palette order |
| G4.25 | a design for black and white | 06 T11 (symbols) | — |
| G4.29 | a linear cut | P3-50, P3-52, P3-53 (fairly even values); P3-51 `extremes` (skewed, the extremes the message) | — |
| G4.30 | natural breaks, rounded | P3-51 `natural` (asked for, to compare) | skewed amounts within two orders of magnitude, the pattern the message (the tests only) |
| G4.31 | equal-count steps | P3-51 `quantile` (asked for, the key saying so) | a ranking message ("the top fifth") where equal counts are the point |
| G4.32 | breaks set by hand | none (the tests only) | a threshold the audience uses |
| G4.33 | logarithmic 1–2–5 steps | P3-51 (rates over four orders of magnitude, skewed, the pattern the message: automatic) | — |

**G5 Labels and legends**

| Rule | Decision | Shown by | Missing example |
|---|---|---|---|
| G5.1 | direct labels | 03-04 #16 (P1-22); P1-11; P1-12; P2-05, P2-09 (beside the last cluster or category) | — |
| G5.3 | the direct-label test | 03-04 #16 (passes) against 03-04 #18 (fails, legend); 03-04 #13; P1-11; P1-22 and its variants (#18's data pack; synthetic converging lines fail, legend) | — |
| G5.5 | a secondary series labelled once | 05 #8; 08-12 #11; P2-07 (beside the first bar), P2-08 (beside the last) | — |
| G5.7 | a legend when direct labels fail | 03-04 #17; 03-04 #18; P1-22 `converging`; P2-03 (and `direct`, the book's labels on leaders) | — |
| G5.10 | where the legend goes | 03-04 #17; 08-12 #7; 07 #12 (several totals, colours running top to bottom [D-31]); P2-01 (above; turned, beside the last cluster); P2-10 (a column beside the latest values) | — |
| G5.11 | the legend right of a time-series line chart | 03-04 #18; P1-22 `converging` | — |
| G5.15 | series labels in small multiples | 13-15 F2; 13-15 F5; P2-61, P2-62 | — |

**G6 Text, numbers and dates**

| Rule | Decision | Shown by | Missing example |
|---|---|---|---|
| G6.10 | how many digits | 08-12 #8 (the fewer-digits stage); 05 #2 | — |
| G6.11 | scaling to thousands or millions | 05 #2; P1-06; 05 #8 | — |
| G6.13 | all digits kept | 02 #16 (the table under the graph); 05 #3 | — |
| G6.18 | the audience's own date format | none (05 #6 tests only that the ambiguous forms never appear) | a time axis and title under a known day-first convention |

**G7 Formatting each chart type**

| Rule | Decision | Shown by | Missing example |
|---|---|---|---|
| G7.14 | symbols for black and white | 06 T11 (P2-22, with an emphasised line and a legend) | — |
| G7.22 | the colours of arrows | 06 T9; 13-15 F18 (green up, red down); P2-21 `no-good-direction` (no good/bad reading: two palette colours) | — |
| G7.28 | the order of slices | 07 #15; 05 #2; P3-08 (and `unsorted`), P3-03 (All others last), P3-07 (grouped), P3-17 (ordered parts in their order) | — |
| G7.31 | the stacking order | 07 #1 (the subtotal at the base); 07 #14; 03-04 #10 (b) (All others on top); P3-20, P3-21 (All others on top), P3-26 (by group), P3-27, P3-30 (the largest at the base); P3-36, P3-33 `west-first` (the parts the message names at the base); P3-39 (parts in groups, by group); P3-30 `fixed`, P3-39 `fixed` (the data's order fixed: kept) | — |
| G7.37 | digits in tables | 08-12 #8; P3-40 (every digit, required), P3-42 (each metric at its own precision) | — |
| G7.44 | the table highlight | 08-12 #8 (the highlighting stage); 13-15 F11; P3-40 (the three outliers) | — |
| G7.47 | graphics inside a table | 08-12 #9; P3-47 (shading), P3-48 (sparklines), P3-49 (bars; `dots`) | — |
| G7.48 | shared or separate scales in small multiples | 06 T13 (uniform and independent); 13-15 F1; P2-64 and its independent variant | — |

**G8 Making the message obvious**

| Rule | Decision | Shown by | Missing example |
|---|---|---|---|
| G8.1 | which techniques the message needs | 13-15 F7; 13-15 F17; P2-40, P2-52 | — |
| G8.4 | what goes in the title and in each callout | 13-15 F7; 13-15 F8; P2-40 (with its `margin` variant), P2-41 | — |
| G8.9 | the highlight method | 13-15 F11 (the catalogue); P1-12 (06 T11); 07 #15; 13-15 F3; P2-45 (bars), P2-46 (lines), P2-47 (a glow on a stretch with the thicker line); P3-09 (a slice: darker, bordered; exploded refused); P3-15 (a slice outlined, 13-15 F10) | — |
| G8.12 | whether to add comparison values | 13-15 F12; 13-15 F17; P1-11 (an average); P2-48 (`bars-alone`: none, reported), P2-52, P2-54 (a computed average) | a chart whose insight compares the items shown, left without comparison values |
| G8.15 | whether a calculated value helps | 13-15 F14, F15, F16; P2-49, P2-51, P2-53 (each with its raw variant) | — |
| G8.16 | deviations | 13-15 F14; 13-15 F9; P2-49, P2-50, P2-55 | — |
| G8.17 | ratios | 13-15 F15; P2-51 | — |
| G8.18 | indexed values | 02 #7 (index); 13-15 F16; P2-53, P2-57 | — |
| G8.19 | raw and calculated values side by side | 13-15 F15 (merged); 13-15 F9; P2-70, P2-69 | — |
| G8.22 | winning over an audience that wants eye candy | 13-15 F18 (the fancy and the useful version) | — |
| G8.26 | a second reviewer | none; a review step no figure can show | — |

No figure shows G1.15, G1.32, G1.34, G1.40, G4.23 or G6.18. For G0.19, G1.25, G1.37, G2.1, G2.3, G2.14, G2.23, G7.22 and G8.12 the figures show one side of the decision only, and the missing example is the other side. Of all the figures above, only P1-01 … P1-14 are in the phase-1 examples document.

## Counts

Rules by section and level; a split rule counts twice, its main part under [L] or [G] and its *s* part under [S].

| Section | [L] | [S] | [G] | [O] | Rules |
|---|---|---|---|---|---|
| G0 Before drawing | 12 | 0 | 2 | 6 | 20 |
| G1 Choosing the chart type | 12 | 0 | 49 | 1 | 62 |
| G2 Scales | 22 | 3 | 20 | 0 | 45 |
| G3 Non-data elements | 9 | 2 | 0 | 0 | 11 |
| G4 Colour | 17 | 2 | 10 | 0 | 29 |
| G5 Labels and legends | 8 | 2 | 7 | 0 | 17 |
| G6 Text, numbers and dates | 20 | 7 | 4 | 0 | 31 |
| G7 Formatting each chart type | 50 | 19 | 8 | 0 | 77 |
| G8 Making the message obvious | 12 | 3 | 11 | 4 | 30 |
| **All** | **162** | **38** | **111** | **11** | **322** |

The 284 original IDs are all kept; 35 of them were split, adding G2.25s … G8.13s; two whole rules are [S] (G4.28, G7.12); G6.24 (split, with G6.24s) and G8.27 were added after the evaluation of 1 Oct 2026. The checklist has 72 checks per chart (50 [L], 20 [G], 2 [O]) and 10 style checks [S].

## Levels to confirm

Tagged by best judgement; one line each on the choice.

- **G0.1** [O]: communication or analysis is treated as the chart's own purpose, like its job; it could be [G], decided from the fact of who will read the chart.
- **G0.4, G0.5** [O]: the job and the insight are open choices; that they must be stated is locked through G0.8 and G0.10, which could instead make these two [L].
- **G2.4** [G]: tagged by the orientation it decides; the "never rotate" it repeats is held by G6.6 [L].
- **G2.21** [L]: the unit phrase's wording is the maker's, but a phrase open to two readings is an error; [O], with specificity as the test of a good choice, is the alternative.
- **G2.38** [G]: which time labels to show is a procedure; the "never rotate or overlap" it states is held by G6.6 and the layout's collision pass.
- **G2.11, G2.12, G2.14** [G]: the limits are computed by default and seldom changed, but a set of charts may share limits on purpose.
- **G1.22, G1.23** [L]: locked because the output is static; in interactive output their exceptions would make them guided.
- **G1.27** [L] beside G1.29–G1.32 [G]: the choropleth is on the avoid-list (G1.61); the other map conditions choose among acceptable types.
- **G3.9 and G3.9s**: the book lists drop shadows with 3-D among the effects to avoid; after the owner's decision on house shadows, only distortion is locked and the rest of flatness is the plain style's.
- **G3.7, G4.3** [L]: drawing zero as a gridline, and one colour for one series, hold in every style; only the zero line's shade and the colour itself are the style's.
- **G4.18** [L]: the colour-blindness check is locked for the palette's colours; good and bad keep their semantic green and red for every reader (owner, 1 Oct 2026), the check on that pair an info, and the black-and-white version (textures) tells them apart without colour; the safe pair with a red/green copy is a style's option [D-18].
- **G4.26** [L]: read as the form of the colour highlight, like G8.11; choosing that method is G8.9 [G].
- **G8.9** [G]: the method is guided and the targets are open; the rule is tagged by the method.
- **G8.22** [G]: a way of persuading stakeholders rather than a chart decision; only the demonstration pair of 13-15 F18 illustrates it.
