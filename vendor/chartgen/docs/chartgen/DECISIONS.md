# Chart guide: decisions

**Status (owner, 30 Sep 2026):** D-2, D-3, D-20, D-55 and D-56 are decided; every other recommendation below is the provisional default, to be judged in the example charts.

Open questions and conflicts in the reading notes (`notes/`, section 8 of each file, and conflicts found between files), deduplicated. Each entry gives the question, the options, a recommendation with its reason, the rules of `GUIDE.md` that carry a provisional default pointing here, and the sources in the notes (file, section and question number). They are ordered by how much they matter for implementing the chart generator, most first.

Left out because the owner has already settled them in `README.md`: the flat plain style with no shadows, cards or strips (so the notes' questions on drop shadows and the house style, 13-15 §8 Q12, need no decision); New Computer Modern, typeset by Typst, for labels (so the question of a serif against a sans-serif face, 05 §8 Q1, needs none). Questions that other notes already answer are listed at the end.

### D-1. Missing inputs: ask, refuse or assume

- **Question.** Many rules need facts the numbers cannot supply: the job and the insight, the dividing value and good side of a good/bad reading, whether an irregular series is a state or an event, theoretical limits, the audience profile (expertise, colour-vision deficiency, culture, black-and-white printing), valid comparison values, the status behind action dots. What does the generator do when one is missing?
- **Options.** (a) Refuse to draw until every field is given. (b) Ask for each missing field. (c) Assume a documented default, record it on the chart, and warn. (d) Mixed: require the job and the insight (or produce a question-titled set, G0.7), require state or event for irregular series and the dividing value for any diverging or good/bad colouring, and default the rest as in G0.10.
- **Recommendation.** (d). **Reason:** the book says the job and insight drive every choice and names them as what AI-made charts get wrong (RP7-3); a guessed dividing value or state/event class misrepresents the data (R3-13, R6-8), while audience facts have safe defaults (general audience, colour-blind readers possible). Recording each assumption keeps the choice visible, as G0.3 asks.
- **Rules that depend on it:** G0.8, G0.10, G0.20, G7.58. **Sources:** 03-04 §8 Q6, Q11; 06 §8 Q14, Q15; 02 §8 Q2; 08-12 §8 Q12; 13-15 §8 Q9; 07 §4.0; 13-15 §5.1.
- **As built (phase 3, the chooser; SPEC §3.2).** (d) for the chooser's facts: required, refused when absent, where a guess could misrepresent: before any pie, stacked chart, treemap or waterfall, whether the values add up to a total someone would quote (`parts.meaningful_total`), whether every part is shown (`parts.all_parts`) and whether a part contains or overlaps another (`parts.overlap` false, or the pairs in `parts.overlaps`; owner, 1 Oct 2026: stated like the other two, since a pie of overlapping parts misrepresents as a pie of averages does), P3-61 `facts-missing`; whether unequal time bins add up (`measure.additive`), P3-72 `not-additive`. Defaulted with an info: the message of parts of a total (compared, a bar chart, which cannot mislead; P3-60 `no-message`), whether a total over time matters (no). Left to the default without a finding: a map's location (no), a table alone (no). For the owner to judge: the parts' message defaulted to a bar chart rather than asked for.

### D-2. The plain-style palette: muted or Office

**Decided (owner, 30 Sep 2026): the muted palette.**

- **Question.** The book's figures use two palettes: a muted one (#95B7D3, #DD8045, #A4AB81, #D8B35B, #7CA79D), which the colour chapter uses to illustrate its rules, and the Office theme (#5B9BD5, #ED7D31, #A5A5A5, #FFC000, #4472C4, #70AD47) in many bar, table, combo and time figures, with a Tableau-like set in some small multiples. Which is the plain style's? And is the sixth colour purple #9A77B7 (colour chapter) or pink #C785B3 (breakdown chapter)?
- **Options.** (a) The muted palette. (b) The Office palette. (c) Both, selectable per document.
- **Recommendation.** (a), with purple as the sixth colour; the Office colours at most as an alternative stylesheet. **Reason:** the muted palette passes the book's own colour rules (balanced lightness, no grey, no red), while the Office palette has a grey slot (#A5A5A5, against G4.15), a saturated gold and uneven lightness. Purple comes from the chapter that sets the rules.
- **Rules that depend on it:** G4.28. **Sources:** 08-12 §8 Q1; 02 §5.2; 03-04 §5.1; 06 §5.1; 07 §5.1; 13-15 §5.11.

### D-3. Scale labels: magnitude once, suffix on every label, or top value in full

**Decided (owner, 30 Sep 2026): (a), magnitude once on the top label; (b) a setting; (c) never.**

- **Question.** The figures format value scales in three ways: (a) the top label carries the magnitude in its unit phrase and the others are bare numbers ("25 million inhabitants", 20, 15 …), the scales chapter's key takeaway (R2-05); (b) every label carries the same prefix and suffix ($0K, $100K … $600K), as the formatting chapter's figures do (R5-17); (c) the top label is written in full and the rest shortened ("$800,000", then 600, 400 …), common in the time and bar chapters. Which is the default, and does the currency symbol go on every label?
- **Options.** (a), (b) or (c) as the default; any of them as a setting.
- **Recommendation.** (a) as the default, with the currency symbol only on the top label; (b) available as a setting; (c) never. **Reason:** (a) is the stated key takeaway and keeps labels short; (b) suits scales whose labels are read one at a time; (c) sets "$800,000" beside "$600", which reads as six hundred dollars, the silent loss of digits that R5-11 warns against. All three agree on one number of decimals throughout.
- **Rules that depend on it:** G2.22, G2.24. **Sources:** 06 §8 Q12; 02 §8 Q6; 02 §5.3, §5.4; 05 R5-17; 08-12 §5.2; 00-01 §7 F4–F6.

### D-4. Hiding digits: trigger, abbreviation and unit

- **Question.** (i) When are digits hidden? The text says to consider it for numbers of more than four digits (R5-10), the figures abbreviate some four-digit scales and leave some six-digit ones in full, and one reader proposes "four or more trailing zeros". Do decimals count? (ii) Which abbreviation for an unknown audience: spelled-out words (R2-06, because "M" can mean thousand) or the K/M/B suffixes of the formatting chapter's examples? (iii) Which unit at a boundary: $0.0M–$1.5M or $0K–$1,500K?
- **Options.** (i) a: the largest value has five or more integer digits; b: labels would carry four or more trailing zeros; c: case by case. (ii) a: words everywhere; b: suffixes everywhere; c: "K" for thousands, "million" and "billion" spelled out unless the audience uses M and B. (iii) a: the larger unit; b: the smaller unit; c: whichever needs fewer characters.
- **Recommendation.** (i) a, counting integer digits only; (ii) c; (iii) a, unless it would need more than one decimal. **Reason:** (i) follows the stated trigger, and decimals are dropped anyway when not needed (R5-9); (ii) the ambiguity R2-06 names concerns "M", while "K" is unambiguous and used throughout the figures; (iii) fewer digits, and the book shows $0.0M–$1.0M.
- **Rules that depend on it:** G2.22, G2.23, G6.11, G6.12. **Sources:** 05 §8 Q3, Q4, Q5; 02 §8 Q6; 02 R2-06; 05 R5-10, R5-11, R5-13.

### D-5. The title block: insight, descriptive line, alignment

- **Question.** The book wants the insight stated in the title (RP5-1) and also "what, by what, period" with the year (R2-12). It prefers the period in the title's own style to a separately styled subtitle (R5-27, fig.), yet the Chapter 1 figures have a subtitle, and the Part 5 figures put the period in the unit phrase or on the time axis. Titles are centred in the figures of Parts 1–4, left-aligned when they state an insight (Part 5), and left-aligned over tables. What does the title block contain, and how is it aligned?
- **Options.** (a) An insight title, with a descriptive line ("Donations by Region, 2022") beneath it in the regular label style. (b) The insight title only, the period carried by the unit phrase or the time axis. (c) A descriptive title, the insight only in callouts. Alignment: left; centred; left for insight titles and centred for descriptive ones.
- **Recommendation.** (a), both lines left-aligned with the scale labels. **Reason:** it satisfies R2-12 (a printout that resurfaces still dates itself) and RP5-1, and it adds no text style because the descriptive line uses the label style (R5-6). Left alignment matches the book's insight titles and tables and gives one rule for everything.
- **Rules that depend on it:** G6.8, G6.9. **Sources:** 00-01 §8 Q5; 05 §8 Q6, Q7; 13-15 §8 Q6; 02 §8 Q14; 13-15 §5.2.

### D-6. Text contrast, the book's light-grey text, and minimum size

- **Question.** R5-8 forbids text close to its background but gives no number, and R5-7 gives no minimum size. Yet the book's figures use light text: "(missing)" notes at about #B0B0B0 (about 2.1 : 1 on white), the secondary "% of total" scale and de-emphasised table values at #A6A6A6, and white labels on muted fills (white on #95B7D3 is about 2 : 1). What floors apply, and do they bind the book's light greys?
- **Options.** Contrast: (a) 4.5 : 1 for all text (WCAG AA), so the lightest text grey is about #767676; (b) 4.5 : 1 for labels and 3 : 1 for notes and de-emphasised text; (c) follow the figures. Size: (a) scale labels at least 9 pt at final size, notes and inset labels at least 0.8 em; (b) relative sizes only.
- **Recommendation.** Contrast (a); size (a). **Reason:** R5-8 is a key takeaway aimed at readers with poor sight, and the book's own bad example is exactly a light-grey unit word (#A2A2A2); de-emphasis can still use weight, size within the floor, or position. The 9 pt floor is a common print minimum, proposed here.
- **Rules that depend on it:** G2.34, G6.4, G6.5, G7.2, G7.4, G8.11. **Sources:** 05 §8 Q8; 05 §5.1; 08-12 §5.1; 07 §5.1; 13-15 §5.11; 02 §5.4.

### D-7. Direct labels or legend: the collision test, clustered bars, label colour

- **Question.** Direct labels are the default, but the book gives no test for "cluttered" (R4-2). The readers propose different ones: use a legend if a label moves more than 1.5 line heights (03-04), add a leader line beyond one line height (06), short leaders where labels collide (08-12). Clustered bars always have a legend in the figures, yet bars with budget markers are labelled once at the first bar with leader lines. Most labels are neutral grey, one figure colours them like their lines. Where does a series that ends early get its label?
- **Options.** Leader threshold 0.5, 1 or 1.5 line heights; fall back to a legend when the order breaks or labels overlap; clustered bars always with a legend, or labelled at the first cluster when two series fit without leaders; label colour neutral or series; an early-ending series labelled at its last known point or aligned with the others.
- **Recommendation.** Pack labels in order; move them up to one line height without a leader and up to 1.5 with one; beyond that, or if the order would change, use a legend. Clustered bars get a legend; bars with a reference marker are labelled at the first bar. Labels in neutral text grey. An early-ending series is labelled at its last known point. **Reason:** this reproduces the approved figures, adds no text style (R5-6), and keeps contrast high.
- **Rules that depend on it:** G5.2, G5.3, G5.5, G5.7, G7.5. **Sources:** 03-04 §8 Q7, Q12; 06 §8 Q5, Q6, Q16; 07 §8 Q4; 08-12 §8 Q14; 02 §8 Q14.
- **As built (phase 2, series; `layout/bars.py`, `layout/legend.py`, SPEC §6.7.4, §6.7.11).** Clustered columns take a legend row above (P2-01, P2-03); labels on leaders beside the last cluster only when asked, a guided finding (P2-03 `direct`). Bars with a reference marker are named once beside the first bar, on thin leaders; where the names would meet a bar there (the months of P2-08: March's bar), beside the last bar, as the book's s. 0321 has them; failing both, a legend row. Several series of dots are named beside the last category's dots by the same packing test as end labels (P2-09). Labels in the text grey throughout.

### D-8. Context lines: grey or coloured

- **Question.** R6-28 [KT] forbids greying the non-focus lines when lines cross, because grey lines cannot be told apart; the author says there are exceptions without naming them. Elsewhere the book pales the other slices or bars when one is highlighted (R7-38, R7-08) and greys non-target table values (Part 5), and greyed context is a common style. When, if ever, may context marks be greyed?
- **Options.** (a) Never grey lines; bars and slices may be paled (same hue, lighter). (b) Grey context lines allowed when they do not cross one another, or when each is labelled directly. (c) Greying allowed with any highlight.
- **Recommendation.** (a); greying of table values only if they meet the contrast floor ([D-6]). **Reason:** R6-28 is a key takeaway with a concrete failure; paling keeps each hue, so categories stay identifiable, and grey is reserved for special categories (R3-16).
- **Rules that depend on it:** G4.26, G4.27, G7.13. **Sources:** 06 §8 Q3; 03-04 §8 Q4; 13-15 §5.4; 07 §5.1.

### D-9. Highlighting: default methods and strength

- **Question.** Any of seven methods works (R15-5), and the worked example combines several. The strengths differ: a line twice as thick plus a vivid hue (Chapter 6) or 2.5 times (Part 5); a colour darkened by 13–18 HSL points (Chapter 7), by 0.12 in OKLCH lightness (a reader's proposal) or by about 25 % (Part 5). A glow is drawn as a blurred halo, which sits uneasily in the flat plain style. What are the defaults?
- **Options.** A default method per chart type or one for all; one strength constant or several; a blurred or a flat glow.
- **Recommendation.** Lines: twice as thick in the strong shade of their hue. Bars and slices: the strong shade (OKLCH lightness about 0.12–0.15 lower, same hue), others unchanged unless the message is a subtotal (then pale the others). Tables: a light cell fill. Glow only as a flat translucent underlay. Callout arrows may be added to any of these. **Reason:** these are the book's usual choices per chart type; a flat glow respects the owner's plain style.
- **Rules that depend on it:** G4.26, G4.28, G7.13, G7.30, G8.9, G8.11. **Sources:** 13-15 §8 Q5; 06 §8 Q4; 03-04 §8 Q4, §5.3; 07 §5.1; 13-15 §5.4.

### D-10. Zero on the scale: the short rules or the full tree

- **Question.** The short rules send "narrow range, small differences matter" to zero plus an inset (R2-22 to R2-24); the full tree (p. 88) sends it to "do not extend to zero", without an inset, when the differences would be invisible with zero or when the audience already knows the data's scale. The tree uses "ratio scale" without defining it. Which does the generator follow, and what counts as a ratio scale?
- **Options.** (a) The short rules, with an explicit override for truncation. (b) The full tree with its seven questions, each a flag. (c) The short rules plus the tree's "audience knows the scale" exit.
- **Recommendation.** (a), and define a ratio scale as one whose zero means "none of the quantity" (counts, money, lengths; not °C, ratings or dates). **Reason:** the author says the short version covers about 95 % of cases and that the tree is not self-explanatory; (a) needs one flag and errs on the safe side, since an inset shows both views.
- **Rules that depend on it:** G0.10, G2.7, G2.8, G2.10. **Sources:** 02 §8 Q3, Q4; 02 §4 D2.

### D-11. Insets: when to add one, how to mark truncation, where to place it

- **Question.** The book gives no number for "outlier" or "narrow range" beyond the tree's test (the gap from zero to the nearest value is at least the spread). The fade that marks truncated bars, and the placement and size of insets, are read from figures. Readers propose: an outlier when the largest value is at least 5 times the next, or the small marks would be under about 10 % of the plot height (02), or the largest small value is under 0.1 of the largest (08-12); a narrow band when (max − min)/max < 0.2 (08-12).
- **Options.** Triggers automatic, or from an author's flag with thresholds only suggesting; truncation marked by a fade, or by a jagged or cut edge; the readers' placement order, or a fixed position.
- **Recommendation.** An inset only when the brief says the small values must be compared; the thresholds (largest small value under 0.1 of the largest; the tree's narrow-range test) only prompt the question. Fade as the one mark of truncation. The placement order of G2.29. **Reason:** whether small values must be compared is part of the message, not the data; the fade is the book's consistent device.
- **Rules that depend on it:** G2.7, G2.26, G2.28, G2.29. **Sources:** 02 §8 Q2, Q8; 08-12 §8 Q5; 08-12 §4 DP-5.

### D-12. The order of the chart-type questions

- **Question.** The book says the group is usually obvious and gives no precedence when data fit several (a breakdown over time by region; exact values over time). The merged procedure needs an order.
- **Options.** (a) Table or graph, variables, location, breakdown, two periods, over time, categories, combos, avoid-list (as in G1.10). (b) The bar-chart tree's order: ordered categories, breakdowns, locations, bars (08-12 DP-2). (c) A reader's order for Part 2: maps, tables, parts of a total, two periods, time, categories (05 DP-5).
- **Recommendation.** (a). **Reason:** purpose-driven exits (lookup, export) come first; location comes before breakdowns because a map changes every later choice and applies only when location outweighs precision; breakdowns come before time because the time tree excludes them (R6-13), and two periods before the general time tree.
- **Rules that depend on it:** G1.10. **Sources:** 05 §8 Q12; 05 §4 DP-5; 08-12 §4 DP-2.
- **As built (phase 3, the chooser; SPEC §3.2).** (a), each step recorded in the report's path with its rule and level (`chartgen choose` prints it; the examples document shows it under P3-60–P3-79). Two consequences to judge: a breakdown over exactly two periods goes to step 4 (stacks), not step 5 (an arrow chart), since breakdowns come first (its parts compared, the message stacks do not serve, are handed on to step 5's arrow chart, and the group moves to change between two periods, the path saying so: the arrow chart is not among the types of breakdowns over time, RP2-8); and a failed check hands a single total on to step 7, so that a bar chart of averages whose zero is arbitrary becomes dots there (G1.57). Every earlier example keeps its type (`tests/fixtures/chosen-types.json`).

### D-13. Seven or eight chart-type groups

- **Question.** The Introduction (p. 41) says Part 2 has seven groups (time, breakdowns, maps, bars, tables, combo charts, types to avoid, i.e. its chapters); the Part 2 opener (p. 164) lists eight group headings (data over time; change between two periods; breakdowns of totals; breakdowns changing over time; maps; maps changing over time; bar charts; tables). Which grouping is stage 1 of the choice?
- **Options.** (a) The opener's eight headings, with combos and the avoid-list as checks afterwards. (b) The Introduction's seven chapters. (c) No named groups, only the procedure.
- **Recommendation.** (a). **Reason:** the two lists classify different things: the seven are chapters, the eight are the groups meant for choosing, and RP2-9 already treats combos and the avoid-list as checks after the choice.
- **Rules that depend on it:** G1.8. **Sources:** 05 §8 Q11; 00-01 §3.0.
- **As built (phase 3, the chooser).** (a): the step that applies enters one of the eight groups (the path's `group`), and the type is chosen within it; combos and the avoid-list are steps 8 and 9, checks on the result. Where the eight do not say, the chooser's placing: a heatmap of more than about 30 series, "data over time" (a heatmap table, "tables"); action dots and the funnel's replacement, "bar charts" (the replacements the avoid-list gives are bar charts); the radar's heatmap table, "tables"; a line across ordered categories for a data-savvy audience, "bar charts" (the bar-chart tree's first question, 08-12 DP-2, gives it).

### D-14. Seven or eight colours; do special colours count

- **Question.** The book says "seven or eight" colours at most without choosing (R3-21). Do the special greys ("All others", missing) count? Can a grey diverging midpoint and a grey for missing values share a chart?
- **Options.** A limit of 7, or 8, or a warning at 7 and a failure above 8; special greys counted or not.
- **Recommendation.** Fail above 8 regular colours unless G4.20 applies; warn at 8 when a legend is used; do not count the special greys. With a diverging palette and missing values in one chart, make the midpoint white and missing values mid grey, as the book's map does. **Reason:** 8 is the book's upper bound; the ten-colour palette already merges pairs under colour-blindness simulation, so legends need the margin; special greys are not memorised as categories.
- **Rules that depend on it:** G4.19. **Sources:** 03-04 §8 Q1, Q10.

### D-15. Grey and red in regular slots: the rules or the figures

- **Question.** The rules keep grey, black and red out of regular palette slots (R3-16, R3-18). The figures use a warm grey (#968C8B) as an ordinary category (s. 0115, s. 0134) and also for "All others" (s. 0120); Chapter 6's series palette includes a grey (#BFBFBF); a red appears in a palette swatch (s. 0109) and in the ten-colour bumps chart. Which wins?
- **Options.** (a) The rules. (b) The figures. (c) The rules, but grey allowed as a regular colour when no special category is present.
- **Recommendation.** (a). **Reason:** the rules are key takeaways with stated reasons, while the figures' uses look like leftovers of tool palettes; (c) would break colour consistency across a set of charts (R3-24) as soon as a later chart adds "All others".
- **Rules that depend on it:** G4.15. **Sources:** 03-04 §8 Q3; 06 §5.1; 08-12 §5.1.

### D-16. Breathing room and snapping to the step

- **Question.** The upper limit should leave 10–20 % of room (R2-27), but after rounding to the step some of the book's charts keep only 5–8 % (rainfall 95 → 100; an index minimum of −14 → −20 %). Is the band a target, a constraint after snapping, or a rough guide?
- **Options.** (a) A strict constraint after snapping (which may force a different step). (b) A target for the snapped limit, falling back to the nearest multiple that leaves at least 5 % (the readers' algorithm). (c) Add 15 % and then snap up.
- **Recommendation.** (b). **Reason:** the readers checked it against thirteen of the book's charts, and it never lets data touch the edge; (a) would change steps the book chose.
- **Rules that depend on it:** G2.12. **Sources:** 02 §8 Q1; 02 §4 D5.

### D-17. The zero line: like the gridlines or darker

- **Question.** Chapter 1 draws the zero line exactly like the other gridlines (R1-07), and most later figures agree; a darker zero line appears with negative values (s. 0235, s. 0237), on an index chart (s. 0070) and under merged panels (s. 0069). When is it darker?
- **Options.** (a) Never. (b) When the scale has negative values or is a difference or index scale. (c) Always.
- **Recommendation.** (b), in a mid grey (about #A6A6A6), still a non-data colour. **Reason:** with values on both sides, zero becomes the reference the reader needs; otherwise it is just a gridline, as Chapter 1 says. The merged-panel case has no stated reason.
- **Rules that depend on it:** G3.7. **Sources:** 00-01 §8 Q3; 02 §8 Q10; 07 R7-12; 08-12 §5.1.

### D-18. Colour-blind safety in static output

**Decided (owner, 1 Oct 2026), departing from the recommendation:** semantic colours for every reader. In colour (the default) good and bad are plain green and red in one file; in black and white (`audience.bw_risk`) the same colours are textured (hatch for bad, dither for good), which also serves colour-blind readers; the safe pair (option (c)) remains a style option for publications to unknown readers.

- **Question.** For interactive output the book shows red/green with a switch to a safe version (R3-20); for print it says choose one version; Part 5 says it is better to publish both (R15-12). Which of blue and orange means bad is only implied (orange bad, blue good), and the safe pair collides with palette slots 1 and 2 (blue, orange). What does the generator emit as static SVG when the audience is unknown?
- **Options.** (a) Red/green only. (b) The safe version only. (c) The safe version as the main file, plus a red/green copy. (d) Red/green as the main file, plus a safe copy.
- **Recommendation.** (c), with orange for bad and blue for good; a chart using the safe pair takes its other colours from slots 3–5. **Reason:** the key takeaway replaces red/green whenever colour-blind readers may be present, which an unknown audience implies; the copy keeps the intuitive version available.
- **Rules that depend on it:** G4.18, G4.28. **Sources:** 13-15 §8 Q8; 07 §8 Q7; 03-04 R3-20; 13-15 §5.6.
- **As built (1 Oct 2026; `chartgen/annotate.py`, `style/message.css`, SPEC §6.12.3).** The style token `--good-bad-mode` chooses the colours, never the spec: `semantic`, the plain style's default, plain green and red in one file, no texture; `safe-pair`, a style for publications to unknown readers, the blue/orange pair in the file (the chart's root `cvd-safe`, a neutral bar moved to slot 3, captions in the pair's text colours) with the red/green version beside it as `<name>.red-green.svg`, or red and green in one file where the spec says `audience.cvd: no`. The chart's fact `audience.bw_risk: true` switches the textures on in either mode: every good or bad area mark of any layout (bars, waterfall steps, slices, tiles, heatmap cells and their key, action dots) is overlaid with a pattern of two tones of its own colour computed into the chart's defs, a 45° hatch for bad and a bayer dither for good, their mean the colour; a dot plot's bad dot is filled and a good one open; cells that carry their values keep a flat fill (their contrast, G6.5). The safe pair in black and white makes one file, textured, with no red/green copy, which would add nothing in grey. Lines carry no good/bad colour (G4.27), so black and white gives them only their symbols (D-33), which every line chart of two lines or more already gets; nothing further was needed. An arrow chart's arrows keep red and green and say rise or fall by their direction. The colour-blindness check (R3-19) reports the good/bad pair as an info (semantic colours for every reader; the black-and-white version tells them apart), and none when a texture or a filled dot does; the palette's own colours still warn. Examples: the new defaults are byte-identical to the former red/green copies; P2-50's `no-cvd` variant is replaced by `bw`, and P2-58 and P3-37 gain a `bw` variant; the examples document shows a red/green copy under its chart only when a safe-pair style wrote one.

### D-19. Comment typography under New Computer Modern

- **Question.** The book sets the maker's comments in a handwritten face in a reserved colour (R5-5), a deliberate second typeface. The owner has fixed New Computer Modern for labels. What distinguishes comments, and do they count towards the four text styles?
- **Options.** (a) New Computer Modern italic in a reserved blue. (b) A second, handwriting-like face for the comment role only. (c) New Computer Modern upright in a reserved colour.
- **Recommendation.** (a), with comments outside the style count. **Reason:** italic plus a reserved colour is an immediate difference, which R5-5 accepts; it keeps one family (R5-1) within the owner's decision, and the book itself treats comments as a separate layer.
- **Rules that depend on it:** G4.28, G6.2, G6.3. **Sources:** 05 §8 Q2; 13-15 §8 Q7.

### D-20. The unit phrase: wording and fallback placement

**Decided (owner, 30 Sep 2026): wording (c), placement (a); a horizontal scale's unit goes in its title, in parentheses.** A unit's description (`what`) is always in parentheses, "0.25 lbs. (weight of product)", "60 (max employees during month)"; a count noun runs on, as the book does, "500 successful transactions per month", "25 million inhabitants". The unit runs on from the top label only on a vertical scale ("50 accidents per month", "8.0 on 10", "35°C"); a horizontal scale has bare numbers and a title centred below it, the quantity with its unit in parentheses: "Weirdness Score (on 10)" (p. 57, s. 0056), "Satisfaction Rating (on 10)" (p. 85, s. 0082), and money likewise, "Total donations per day ($000s)" (p. 100, s. 0095), "Change in Donations ($K)" (p. 200–201, s. 0186–0187), "Donations ($000s)" (p. 278, s. 0254), "Funds raised ($M)" (p. 342, s. 0312). The generator writes K for thousands and "$ million" for millions (D-4).

- **Question.** The unit phrase appears as a running phrase ("25 million inhabitants") and in parentheses ("60 (max employees during month)", "$6,000 (monthly donation)"), even in consecutive figures. The book shows three placements (on the top label's line; above the label column, wrapped; on its own line above the top value for a scatter plot's vertical scale) with no rule; the README speaks of a horizontal title above the vertical axis.
- **Options.** Wording: (a) a running phrase, no parentheses; (b) always parentheses; (c) parentheses only around descriptions that are not units. Placement: (a) on the top label's line, falling back to above the label column when it would collide with data or leave less than about 15 % of the top gridline (the readers'); (b) always above the label column.
- **Recommendation.** Wording (a); placement (a). **Reason:** the scales chapter's key figures run the phrase on, and parentheses add characters without meaning; the top-line placement is what the book draws, and both placements satisfy the README's aim of a horizontal title at the top of the axis.
- **Rules that depend on it:** G2.19, G2.20. **Sources:** 00-01 §8 Q4; 02 §8 Q7; 02 §4 D11; README.

### D-21. Reference lines: solid grey or dotted

- **Question.** An average appears as a solid dark grey line with a bold label (colour chapter; line cheat sheet), as a black dotted line (animation chapter), and an industry average as a dotted blue line (Part 5). R6-24 keeps dashes for values of lower certainty or importance, naming targets. How are averages, targets and outside benchmarks drawn?
- **Options.** (a) All solid dark grey. (b) All dotted dark grey. (c) An average of the plotted data solid dark grey; a target or outside benchmark dotted dark grey. (d) As in the figures, blue included.
- **Recommendation.** (c). **Reason:** grey is reserved for averages and totals (R3-16); dots mark lower certainty or importance (R6-24), which fits a target or benchmark but not a computed average; a blue line would borrow a regular hue (R3-1).
- **Rules that depend on it:** G7.10, G7.16, G8.13. **Sources:** 06 §8 Q13; 13-15 §5.5; 03-04 R3-16.

### D-22. Colour by category or by rank

- **Question.** In the pie and breakdown figures colours are handed out by rank (the largest part blue, the second orange …), so a category's colour changes when its rank does; R3-24 asks for one colour per category across a set of charts.
- **Options.** (a) A registry across the document: each category keeps the colour it first received. (b) Rank order within each chart. (c) A registry for categories that appear in more than one chart, rank order otherwise.
- **Recommendation.** (a), filled in display order, so that a single sorted chart still looks like the book's. **Reason:** R3-24 is a key takeaway; colouring by rank is a habit of the figures with no stated rule.
- **Rules that depend on it:** G4.22. **Sources:** 07 §8 Q6; 03-04 §5.2.

### D-23. The part limit for pies and stacked bars

- **Question.** The book allows "about six or seven" parts (R7-13), and the tree asks "fewer than 6–7 parts?". Is seven allowed?
- **Options.** (a) Up to 6 fine, 7 flagged, 8 or more rerouted. (b) Up to 7 fine. (c) Up to 6 only.
- **Recommendation.** (a). **Reason:** it honours both readings and sends the borderline case to a person.
- **Rules that depend on it:** G1.36, G1.39. **Sources:** 07 §8 Q1; 00-01 §8 Q7.
- **As built (phase 3, the chooser).** (a): with 7 parts the chooser gives the pie with a guided warning (R7-13), turned into an info by `reasons["chart.type"]`; with 8 or more and a fraction or subtotal message, a treemap (P3-66), the path naming the lumping of minor parts into All others (R7-17), which is the maker's to do.

### D-24. Overlapping parts: a bar chart, or separate them

- **Question.** The pie decision tree sends overlapping categories to a plain bar chart; the key takeaway prefers subtracting to make disjoint parts, or making the containment visible.
- **Options.** (a) The tree: a bar chart. (b) The takeaway: separate the parts where possible, otherwise a bar chart with each parent stacked from its sub-parts. (c) Ask each time.
- **Recommendation.** (b). **Reason:** the takeaway is the more specific guidance and keeps the part-of-whole message; a plain bar chart hides how the categories relate.
- **Rules that depend on it:** G0.14. **Sources:** 07 §8 Q2.
- **As built (phase 3, the chooser).** Whether parts overlap must be stated before any pie, stacked chart, treemap or waterfall (owner, 1 Oct 2026; D-1): `parts.overlap` false, or the pairs in `parts.overlaps`; not stated, the chart is refused under R7-05 (locked), asking for it. The check fails whenever `parts.overlap` is true or `parts.overlaps` lists a pair, and the chooser gives the tree's plain bar chart (a), naming (b)'s better answers in the path: making the parts disjoint is a change of data the generator does not make, and a parent stacked from its sub-parts needs the data reshaped as stacks. A pie asked for is refused under R7-05 (P3-65); the parts made disjoint in the data are P3-07.

### D-25. Do the breakdown checks apply to several totals and over time

- **Question.** The checks for mixed signs, overlap and the part limit are stated for one total; the book does not restate them for multiple stacked bars, multiple pies or stacked areas. (Its preference for merged over clustered bars when parts are missing rests on the author's impression, not research.)
- **Options.** (a) Apply all four checks and the part limit to every total and every period. (b) Only the meaningful-total and all-parts checks. (c) None.
- **Recommendation.** (a), and keep merged bars as the first choice when parts are missing. **Reason:** each stack or pie invites the same misreadings as a single one, and nothing in the book says otherwise.
- **Rules that depend on it:** G1.41. **Sources:** 07 §8 Q3, Q10.
- **As built (phase 3, the chooser).** (a): the four checks run on every total and every period, the sign check within each total (P3-67's regions; over time, each month of P3-68); a failure gives clustered bars for several totals and a line per part over time; more than 7 parts, likewise. Merged bars stay the first choice when parts are missing (R7-22, drawn through `data.dims`, P2-67).

### D-26. Stacking order

- **Question.** No rule is stated. The figures put the largest or most important part at the base (Smith, Germany, East), but the over-time stack follows the data's row order (North below a larger South), and "All others" goes on top.
- **Options.** (a) The largest part (over the whole chart) at the base. (b) The part most important to the message at the base, otherwise the largest. (c) Data order.
- **Recommendation.** (b), with "All others" on top. **Reason:** only the bottom part can be read precisely, and the message decides which part needs precision (R7-08 already puts summed parts at the base).
- **Rules that depend on it:** G7.31. **Sources:** 07 §8 Q5.
- **As built (phase 3, stacks; SPEC §6.18).** (b): the parts the insight or highlight names at the base, larger first, then the rest largest first by their sum over every total, "All others" on top in its grey; one order and one colour in every stack, the colours handed out in stacking order (D-22 (a)). Ordered parts (`data.parts.kind: ordinal`) keep their own order, the first at the base, in steps of one hue; parts in groups (`x.groups`) stack by group, largest group first. Over time this puts East, West, South, North where the book keeps the table's order (North below South): for the owner to judge on P3-30–P3-33. Those charts and the lines of P3-28–P3-29 fix the regions' slots in their specs (the book's colours), so that each region keeps one colour across the set (R3-24), which colouring each chart by stacking place would not; P3-33 `west-first` puts the part the message is about (the West) at the base of every month instead. Parts of several totals in groups (`data.parts.groups`) stack by group, the largest group at the base (P3-39, where the book's figure has Asia, the smaller, first). Where the data's order is fixed (owner, 1 Oct 2026: a table shared by several charts ordered differently; `data.parts.order: fixed`), the parts and the totals keep the order given, a guided decision (P3-30 `fixed`, the book's East, West, North, South; P3-39 `fixed`, Asia at the base).

### D-27. Enough values for a line

- **Question.** The book gives only examples: 6 of 31 days known → dot plot; 11 of 12 → a line with a gap; 10 of 12 with an isolated value → a line with markers. The readers propose: at least half the slots known and a run of three consecutive known values. And can a state series with few changes be sent to a dot plot, as the tree's order implies?
- **Options.** (a) The readers' test as a default with an override. (b) Another threshold. (c) Always ask.
- **Recommendation.** (a), applied to regular series and events only; a state series is fully known between its changes, so it goes to a step chart however few the changes. **Reason:** the test reproduces the book's three cases, and a price that rarely changes is not missing data.
- **Rules that depend on it:** G1.49. **Sources:** 06 §8 Q1, Q9.

### D-28. How many series per plot

- **Question.** Four or five lines are usually fine, more only if they rarely cross and are labelled directly (R6-35); a stack holds about 20–30 small charts. The book gives no number for series per panel in small multiples (its good panels hold 3–6, its rejected chart 15). The readers propose warning above 6–8 series per panel and allowing about one crossing per series.
- **Options.** (a) Warn above 5 lines in a plot unless crossings are at most about one per series and the labels pack. (b) Warn above 6–8. (c) No automatic limit.
- **Recommendation.** (a), with 20–30 panels as the limit for a stack. **Reason:** five is the book's number, and the crossing test turns "rarely cross" into a check.
- **Rules that depend on it:** G1.20. **Sources:** 13-15 §8 Q1; 06 §4 DP6-5; 13-15 §5.13.

### D-29. Point markers on lines with gaps

- **Question.** The key takeaway asks for markers only around a value isolated between gaps (R6-25); the cheat sheet lists "the line has gaps" as a condition of its own, but the approved chart with one gap has no markers.
- **Options.** (a) Markers only for isolated values and straight runs. (b) Markers on every line with a gap. (c) A setting defaulting to (a).
- **Recommendation.** (c). **Reason:** the key takeaway and the approved figure agree; the cheat sheet's wording may be a summary.
- **Rules that depend on it:** G7.11. **Sources:** 06 §8 Q2.

### D-30. Dots or bars for categories

- **Question.** Categorical dot plots (hollow circles) appear only where the scale does not start at zero (credit scores, satisfaction ratings of 7–10); narrow ranges are otherwise answered with insets. When else, if ever, should categories be drawn as dots?
- **Options.** (a) Only when the scale should not start at zero. (b) Also for many categories or several series per category. (c) Never; use insets.
- **Recommendation.** (a). **Reason:** it is the only use the book shows, and bars are more familiar (R0-03).
- **Rules that depend on it:** G1.57. **Sources:** 08-12 §8 Q4.

### D-31. Legend position when colours run vertically

- **Question.** A legend goes above the plot (R4-6), but its entries must run in the same direction as the colours (R4-4). In vertical stacked columns and horizontal clustered bars the colours run top to bottom, so a row above breaks the direction and a column at the right breaks the position. The several-totals figures put such legends at the right.
- **Options.** (a) A column at the right of the plot, in stack order. (b) A row above, read left to right as bottom to top. (c) Direct labels at the right of the last bar where they fit, otherwise (a).
- **Recommendation.** (c). **Reason:** matching direction is the point of the key takeaway (matching by position), the figures place such legends at the right, and direct labels avoid the question.
- **Rules that depend on it:** G5.10. **Sources:** 03-04 §8 Q8; 07 §5.8.
- **As built (phase 2, series; SPEC §6.7.4).** (c) for horizontal clusters: each series named beside its bar of the last cluster, where the names clear everything and stay in the plot (P2-01 `horizontal`, P2-05); else, or when a legend is asked for, (a), a column right of the plot, centred on it. "The last bar" is read as the last cluster, whose bars, sorted descending, are the shortest and leave the names room inside the plot; the first cluster would read first but needs the plot narrowed. For the owner to judge on P2-05.

### D-32. Date formats: inline default and date columns

- **Question.** For an unknown audience the book's good examples include both month-first ("Feb. 10, 2007", as in its titles) and day-first ("10-Feb-2007") forms. In a date column the author leaves the choice between accepting slight drift in a proportional face and using a monospaced one (R5-23).
- **Options.** Inline: (a) "Mar. 11, 2022"; (b) "11 Mar. 2022"; (c) "11-Mar-2022". Columns: (a) New Computer Modern, accepting the drift; (b) New Computer Modern Mono.
- **Recommendation.** Inline (a), as a setting that a known audience convention overrides (a British audience would read (b)); columns (a). **Reason:** (a) is the book's own title form; the author does not mind the drift, and monospaced faces look technical.
- **Rules that depend on it:** G6.21, G6.22. **Sources:** 05 §8 Q9, Q10.

### D-33. Black-and-white reproduction

- **Question.** Only the chart's maker knows whether it will be printed or copied in black and white, and the generator needs a default, particularly for documents meant for print.
- **Options.** (a) Default no. (b) Default yes for print outputs, no for screen. (c) Always add symbols.
- **Recommendation.** (a), with a per-document switch. **Reason:** the author calls symbols the least bad option and says not to add them when they are not needed (R6-30).
- **Rules that depend on it:** G0.10, G4.25, G7.14. **Sources:** 06 §8 Q14.
- **As built (phase 2, time; `layout/lines.py`, SPEC §6.13.7).** (a): `audience.bw_risk` defaults to false. Where it is set, each of two or more lines gets its own symbol at every point, in the book's six, handed out in the order of the line ends; the lines keep their colours (the book's one dark ink is the chart after the copy), an emphasised line stays thicker, a legend's key shows the symbol (P2-22). For the owner to judge.

### D-34. The readers' numeric thresholds

- **Question.** Several checks need numbers the book does not give; the readers calibrated these on the book's figures: non-data strokes at most 1.5 : 1 against the background and lighter than the palest data colour; OKLCH chroma at most 0.15 for regular colours; a lightness spread of at most 0.12 in a categorical palette; an OKLab distance of at least 0.06 between colours that must stay distinct under colour-blindness simulation; about five intervals and at most eight scale labels; horizontal bars when a category label would need three lines or there are more than about 12 categories; a leader line to pie slices under 8°. Adopt them?
- **Options.** (a) Adopt all as errors. (b) Adopt all as warnings. (c) Re-derive each from rendered test charts.
- **Recommendation.** (b) now, then (c) once the book's test figures have been rebuilt. **Reason:** they reproduce the book's good and bad examples, but none is the author's.
- **Rules that depend on it:** G2.4, G2.17, G3.2, G4.2, G4.6, G7.29. **Sources:** 00-01 §8 Q8; 03-04 §8 Q2; 02 §8 Q5; 08-12 §8 Q3; 07 §8 Q9.

### D-35. Measured figure values as plain-style defaults

- **Question.** Many geometric values come from measuring the book's screenshots, not its text: a bar band of 0.55 (range 0.5–0.63), lines about 0.2 em thick, gridlines about 0.08 em, titles 1.2–1.3 × the label size, unit text 0.85–1.0 × the label size, inset text 0.8 ×, pie gaps 0.7 % of the diameter, a donut hole of 0.74, table rows 2.15 em apart with 1.5 em gutters and 3 em before totals, dots 0.5 em across. Are they the plain style's defaults, and should the bar gap vary with the number of bars?
- **Options.** (a) Adopt them as stylesheet defaults. (b) Adopt them, with the bar band varying with the number of bars. (c) Derive new values.
- **Recommendation.** (a), with a fixed band of 0.55 and the unit text at the label size. **Reason:** they are style, not rules, and the stylesheet can change them; the unit text at label size keeps the chart to two text styles (R5-6).
- **Rules that depend on it:** G7.1, G7.12, G7.32, G7.39. **Sources:** 07 §8 Q13; 08-12 §8 Q2, Q8; 02 §5.2; 05 §5.1; 06 §5.1.

### D-36. Maps: thresholds, preference order, tile layouts

- **Question.** "Very different sizes", "very close to zero", "narrow range" and "many regions" have no numbers; the final choice among suitable map types is left to preference; the book gives a tile layout only for Australia.
- **Options.** The readers' defaults (a choropleth only if the largest region is at most 10 times the smallest; near zero under 5 % of the maximum; many regions above 30; no mark under 4 px; the order coloured dots, bubbles, bars, tiles, choropleth) or others; tile grids shipped for common geographies or supplied with the data.
- **Recommendation.** The readers' defaults and order; tile grids supplied with the data until one is needed. **Reason:** coloured dots are the type the book calls generally safe, and maps are outside the current use.
- **Rules that depend on it:** G1.33, G7.55. **Sources:** 08-12 §8 Q6, Q7; 08-12 §5.9.

### D-37. The midpoint of a diverging heatmap

- **Question.** R3-13 puts a diverging palette's midpoint where the audience sees a dividing value, never at the middle of the data, but the heatmap that replaces the radar chart has its white point near the middle of its data (about 3.3 on values of 2.2–4.7), with no stated dividing value.
- **Options.** (a) Sequential unless a dividing value is supplied. (b) Diverging around the data's median. (c) Diverging around the scale's midpoint (3 on a 1–5 rating).
- **Recommendation.** (a), or (c) when the scale's midpoint is itself meaningful to the audience ("neutral"). **Reason:** it keeps R3-13, a key takeaway.
- **Rules that depend on it:** G4.11, G7.36. **Sources:** 08-12 §8 Q13.
- **As built (phase 3, tables; SPEC §6.14).** (a), with (c) given as data: a ramp diverges only about `data.measure.dividing_point`, else it is sequential (P3-50 and P3-52 `no-dividing-value`); a rating's neutral middle is given as that value (P3-50, 3 on 5). `chart.encoding: diverging` without one is refused (R3-13; P3-53 `diverging-about-the-middle`). The ramp is symmetric about the dividing value, one step of colour the same distance from it on either side, and cut at the theoretical limits, so that the dividing value stands off the key's middle where the audience's does (P3-52, 7 on 10) and the side with the smaller values reaches fewer steps (P3-53). For the owner to judge: symmetric sides against each side scaled to its own extreme (03-04 §5.2 item 7), which would show the cold years of P3-53 as strongly as the warm ones. **Owner (1 Oct 2026):** symmetric, for now. Since the cut of G4.29–G4.34 (D-57), the steps a side are the cut's (at most four), mirrored, and a value at the dividing value takes the neutral step of its own; the key is a bar of equal steps, the tick at the neutral's middle.
- **Rules that depend on it, since:** G4.29–G4.34 (D-57).

### D-57. Cutting a colour scale into steps, and the values in its cells

- **Question.** (i) Where are a heatmap's colour steps cut? A scale linear in value leaves almost every cell of skewed data (disease rates before and after a vaccine) in the palest step (P3-51: 80 % of the values in one of seven equal steps). (ii) How does a value printed in a strong cell keep 4.5 : 1? (iii) How is the lightest step told from the page?
- **Options.** (i) (a) Equal steps on rounded values; (b) natural breaks, rounded; (c) quantiles; (d) custom breaks; (e) logarithmic 1–2–5 steps; each by the data alone, or by the data and the message. (ii) Pale cells and the text grey; black or white text by the cell; the grey where it reads; knock-out halos (where the grey fails, or always); a pale plate under each value. (iii) A darker first step; a ramp starting at the page.
- **Decision (owner, 1 Oct 2026).** (i) GUIDE G4.29–G4.34, after Datawrapper's guide to colour-scale interpolation, G4.33 ours: (a) for fairly even values or when the extremes are the message; (b) for skewed values when the pattern among the many is the message; (c), (d) and (e) only when asked for, with a reason. **Revised (owner, 1 Oct 2026):** (e) is automatic for skewed amounts on a ratio scale whose pattern is the message when they span at least two orders of magnitude, (b) then only for skew within a narrower range; data not on a ratio scale never take (e). (ii) Strong cells with black or white values by default; muted cells (pale, the text grey) a per-chart option for a table with colour hinting. (iii) The lightest step clearly distinct from the page and the missing grey (G4.34).
- **As built (phase 3, tables; SPEC §6.14).** (i) `chart.classing`, auto; skewed (ours): one of seven equal steps from the lowest value to the highest holding more than half of them; then the fact `facts.ramp_focus` (`extremes` | `pattern`, default pattern with an info, D-1) decides; for the pattern, logarithmic 1–2–5 steps (zero its own, the top open) when the values are on a ratio scale (zero meaning none of it, none below it; sequential ramps only) and span at least two orders of magnitude, measured as the 95th percentile of the positive values over the 5th (`ramps.log_span`, at least 100: robust, so that one stray tiny value or one outlier does not make a narrow range logarithmic, while the bulk of the marks spanning two decades does), else natural breaks six, rounded to two significant figures, the top step open; quantiles six, the key saying "each step about a sixth of the cells"; custom `chart.breaks`; logarithmic steps asked for are refused, with linear drawn, for values not ratio-scaled. The rules registered by their guide IDs, each with its own tag's level (rules.py). (ii) `chart.cells` (`auto` | `strong` | `muted`): auto strong for a heatmap, muted for a shaded table (`table.graphics: heat`, a table first: its colour only hints, G7.47, and its numbers are what is read); the style's `--cell-values: contrast` (black #000 or white; 4.68 : 1 at worst, on `--seq-7`) or `plate`; the halos and the grey switch removed. (iii) `--seq-1` #EDF5FB (ΔE 0.036 from white) → #DCEAF6 (0.073), `--seq-2` #CFDFEC → #C6DBEB; checked per chart against ΔE 0.05 (ours), the neutral step excepted (G4.12). P3-51 (logarithmic: its rates span about 10,600 times from the 5th percentile to the 95th) and its variants `natural`, `extremes`, `quantile`; P3-50 `muted`.
- **For the owner to judge.** The skew threshold (more than half in one of seven equal steps), the span measure (the 95th over the 5th percentile of the positive values) and the six steps of natural breaks are ours. Natural breaks (P3-51 `natural`) put the measles' post-vaccine cells and the low pre-vaccine years (under 120) in one step; the logarithmic steps show the fall after 1963 step by step.
- **Rules that depend on it:** G4.29–G4.34, G6.5, G7.36, G7.47. **Sources:** owner and coordinator, 1 Oct 2026; Datawrapper (L. C. Muth, "How to choose an interpolation for your color scale").

### D-38. Small multiples: arrangement, scales, labels, frames

- **Question.** (i) The arrangement is found by trial and error: does the generator pick one or render candidates for a person? (ii) A shared scale is the default (RP3-8) but separate scales suit comparisons of shape (R6-37): switch automatically? (iii) Series labels in the first panel or right of the last? (iv) The time chapter frames its stacked panels; the Part 3 grids have no frames.
- **Options.** As in the question.
- **Recommendation.** (i) Pick by the readers' test (every required comparison within a panel, least clutter) and also render the runner-up for review. (ii) Shared unless the brief says the message is about shapes. (iii) Right of the last panel, next to the latest values, when all panels have the same series; the first panel otherwise. (iv) No frames, light separators. **Reason:** each follows the book's figures and keeps the choice checkable.
- **Rules that depend on it:** G1.18, G5.15, G7.48, G7.51. **Sources:** 13-15 §8 Q2, Q3, Q4; 06 §5.4.
- **As built (phase 2, `layout/multiples.py`, SPEC §6.10).** (i) The arrangement comes from `facts.compare` (the dims the insight compares share a panel; with none, one series a panel), and the report names the runner-up, which the examples render as a variant (P2-61); the generator does not render it on its own. (ii) Shared unless `facts.compare_shapes` (P2-64). (iii) Right of the last panel of the first row; the first panel is not built yet, though P2-61 shows where it would read better (Int'l's lines end close together). (iv) No frames; separators in the divider grey. The thresholds stand in provisionally: more than 6 values of a dim are stacked in rows rather than set across (O), more than 5 lines in a panel are warned about unless they rarely cross (D-28 (a)), more than 30 panels likewise (R6-36).

### D-39. Pie-averse audiences; fractions within several totals

- **Question.** The book says to avoid a pie for a pie-averse audience but not what to use instead; and for several totals, "fractions within each total, with rough comparison across totals" falls between multiple pies and multiple stacked bars.
- **Options.** A 100 % stacked bar; a bar chart with a "% of total" scale; multiple pies anyway.
- **Recommendation.** A 100 % stacked bar in both cases. **Reason:** it shows fractions better than bars (R7-07), allows rough comparison across totals, and the book uses it for fractions over time.
- **Rules that depend on it:** G1.40, G1.41. **Sources:** 07 §8 Q8, Q12.

### D-40. Arrow chart details

- **Question.** The book lists rows by their before value, largest first, without saying so; it does not say how to set the % panel's range, what to do when a relative change is undefined (before = 0) or crosses zero, whether the panels share row separators, or how to colour arrows when rises are not simply good.
- **Options.** Row order by before value, by change or by % change; drop or keep the % panel when undefined; colours by good/bad, by direction, or neutral.
- **Recommendation.** Rows by before value, largest first, unless the message is about the change (then by % change); the % range by the scale rules, including 0; if a before value is 0 or negative, drop the % panel and say why; thin shared row separators; green and red only for a good/bad reading, otherwise two regular palette colours for rises and falls. **Reason:** the order matches the figures, and colouring by direction with red and green would claim a good/bad reading the audience may not have (R3-18).
- **Rules that depend on it:** G1.47, G7.21, G7.22. **Sources:** 06 §8 Q10.
- **As built (phase 2, time; `layout/arrowchart.py`, SPEC §6.13.4).** As recommended, except the row separators: the panels keep their vertical gridlines (G3.6 [L]), with which separators would draw a table's grid, so none is drawn (P2-21). The % scale holds 0 and is refitted with fewer intervals where its labels would crowd; its labels are signed (6.2). Good and bad arrows are green and red, as the message layer draws good and bad (D-18); in black and white their direction says rise or fall. For the owner to judge.

### D-41. Bumps chart details

- **Question.** How are tied ranks shown, and competitors that enter or leave? How many lines can a bumps chart hold (the book shows ten)? Should labels also appear at the left, at the starting ranks?
- **Options.** Ties as shared ranks or broken by a rule; entries and exits as line starts and ends or excluded; a warning limit; labels right only or both sides.
- **Recommendation.** Ties share a rank and are drawn at one height with their labels stacked; entries and exits start and end lines with a small marker; warn above ten lines; labels at the right only. **Reason:** these keep every line honest and within the book's shown capacity.
- **Rules that depend on it:** G1.52, G7.24. **Sources:** 06 §8 Q7.
- **As built (phase 2, time; `layout/bumps.py`, SPEC §6.13.6).** As recommended: ties share a rank and a height, their labels packed; a line that starts late or ends early has a dot there; more than ten lines warn; labels at the right only. The lines are always drawn with flat-tangent curves (P2-25).

### D-42. Step and thin-bar details

- **Question.** Does the last step run to the chart's as-of date (as the book's figure does) or stop at the last change? How wide is a thin bar, and what happens to two events at the same instant?
- **Options.** Last step to the as-of date or to the last change; bar width fixed or proportional to the axis; coincident events overdrawn, summed, stacked or offset.
- **Recommendation.** The last step runs to the as-of date; bars 1.2–1.5 × the line width; coincident events stacked with a hairline gap. **Reason:** the state still holds at the as-of date; stacking keeps each event visible.
- **Rules that depend on it:** G7.19, G7.20. **Sources:** 06 §8 Q8, Q9.
- **As built (phase 2, time; `layout/step.py`, SPEC §6.13.2–3).** As recommended: the last step runs to the end of the as-of day (without an as-of date or a window a step chart is refused, R6-8); thin bars 1.4 × the line width; coincident events stacked with a hairline gap, the scale holding their total (P1-27: the book's figure has the two sales of July 3 side by side, its times being within the day; for the owner to judge). A dated event sits in the middle of its day, a dated change at its start.

### D-43. Waterfall and total-bar details

- **Question.** In what order do the items go (the example puts gains before costs)? Are intermediate subtotal bars and horizontal waterfalls allowed? When does a breakdown bar chart end with a Total bar ("with or without" in the tree)?
- **Options.** Order as given, or gains first; subtotals allowed or not; orientation; Total bar always, never or on request.
- **Recommendation.** The process's own order (an income statement's), otherwise gains first; subtotal bars at declared positions; vertical only; a Total bar when the net total is part of the message. **Reason:** a waterfall tells a sequence, and the book shows only the vertical form.
- **Rules that depend on it:** G1.38, G7.33. **Sources:** 07 §8 Q11.
- **As built (phase 3, stacks; SPEC §6.18).** The steps in the given order, never sorted; a category of role `total` is a full grey bar from zero at its place: the first a start balance, a later one a subtotal or the end total, checked against the running total (a warning where they differ, D-54); vertical only, an explicit horizontal waterfall drawn upright with a guided finding; names that would meet under the bars are refused (R2-08), so many steps need a wider figure (P3-25 at 480 pt). Rises and falls in slots 1 and 2 without a good/bad reading, green and red (the colour-blind-safe pair in the file) with one. Running totals may go below zero (P3-37, the zero line signed); a label stands past its bar's near end where the far one would cross the zero line.

### D-44. The allowed dual axis: exact positions or aligned steps

- **Question.** In the rainfall figure 4.0 inches is drawn level with 100 mm, which is 101.6 mm. Do secondary labels sit at their exact positions (between gridlines), or are the ranges chosen so that they align?
- **Options.** (a) Exact positions. (b) Ranges chosen to align.
- **Recommendation.** (a). **Reason:** a dual axis is allowed only because it is exact (R2-15), and aligned ranges would need unnatural steps (R2-09).
- **Rules that depend on it:** G2.33. **Sources:** 02 §8 Q11.

### D-45. Time-axis details: thinning, ticks, equal slots

- **Question.** (i) How is the k of label thinning chosen, and what if the last label of one group collides with the first of the next (the book shows only every second label)? (ii) Some time axes in the figures have daily minor ticks, and the dual-scale stacked bar has an axis line with ticks, while R1-04 removes ticks "in most cases" without naming the exceptions. (iii) Months differ in length but the book spaces them equally.
- **Options.** As in the question.
- **Recommendation.** (i) The readers' rule: k = ⌈(widest label + 0.6 em) / slot width⌉, restarting in each group, dropping the earlier label on a collision. (ii) Short minor ticks only on true-time axes of irregular series (step and thin-bar charts), and an axis line with ticks only where there are no gridlines; none otherwise. (iii) Equal slots for regular series, true time for irregular ones (G2.42). **Reason:** these reproduce the figures and keep R1-04's default.
- **Rules that depend on it:** G2.38, G2.42, G3.4. **Sources:** 02 §8 Q12; 00-01 §8 Q1; 06 §8 Q11; 06 §5.3.
- **As built (phase 2, time; `timeaxis.continuous_fit`, SPEC §6.13.1).** (ii) and (iii) on the true time axes of step and thin bar charts: ticks at the boundaries of the labelled unit, minor ticks each day under month labels where 3 pt apart, the SVG lint allowing ticks only there; the labels centred on their periods (hours at their instants), thinned every second or third and restarting in each group (i). For the owner to judge: the book's software labels a month at the tick of its first day. (i), owner, 30 Sep and 1 Oct 2026: a time label clears every divider by `--divider-clear` (0.3 em): the first and last slots move in from the outer dividers; a label beside a divider between two groups keeps its slot with a knock-out halo in the page colour round its letters (P1-21 `narrow` keeps November's "1"); too narrow even every seventh label, the chart is refused with the width it needs; the SVG lint checks it (CG-6; SPEC §6.3.8).

### D-46. Sorting with several series; catch-all and missing last

- **Question.** Multi-series charts are sorted by their first series, and "Others" and missing values are kept last, in the figures only.
- **Options.** Sort by the first series, by the series the insight is about, or by the total across series; catch-all and missing last always or not.
- **Recommendation.** Sort by the series the insight is about, otherwise the first series; catch-all and missing categories always last. **Reason:** the sort should serve the message (G0.5); the catch-all is not a real category.
- **Rules that depend on it:** G2.1. **Sources:** 02 §8 Q9; 08-12 §8 Q3.
- **As built (phase 2, series).** By the series the insight names, else the first series that is not a reference (a budget never sorts, P2-07); the catch-all, a total and categories without a known value last. The series keep the spec's order within each cluster and in the legend (the book's figures list them by their totals, which a spec can do). The book leaves the regions of s. 0135 and the methods of s. 0331 unsorted; P2-03 and P2-06 sort them.

### D-47. A category-axis title on bar charts

- **Question.** R2-03 drops the category-axis title when the names explain themselves, but the bar chapter's look-alike examples all carry one ("Region", "Donor"), and a reader infers that bar charts need one because bars look alike (R9-3).
- **Options.** (a) Follow R2-03. (b) Always title the category axis of a bar chart.
- **Recommendation.** (a), with the descriptive line of the title block naming the breakdown ("by Region"). **Reason:** the descriptive line already does the job, and an obvious axis title is clutter.
- **Rules that depend on it:** G1.9, G2.3. **Sources:** 08-12 R9-3; 02 R2-03.

### D-48. Previous-period comparison values

- **Question.** Previous periods are listed as comparison values, but their "significant limitations" are described only on the author's website.
- **Options.** Allow freely; allow only when no target, budget or average exists; never.
- **Recommendation.** Only when no target, budget or average exists, preferring the same period a year earlier to the immediately previous one, and flagged in the brief. **Reason:** it keeps the book's order of preference and avoids seasonal effects.
- **Rules that depend on it:** G0.19. **Sources:** 13-15 §8 Q10.

### D-49. Grouped shades with two members

- **Question.** Two shades cannot be "out of lightness order". Is the grouped legend alone enough to stop them reading as amounts?
- **Options.** Yes; no, use distinct hues.
- **Recommendation.** Yes: a light and a dark shade with the grouped legend; if the grouping is not part of the message, use the categorical palette. **Reason:** the legend states the grouping explicitly.
- **Rules that depend on it:** G4.9. **Sources:** 03-04 §8 Q9.
- **As built (phase 3, tables; SPEC §6.14).** Yes: with two members a light then a dark shade of the group's hue (with three, the base, the dark, the light, s. 0115), and the grouped legend of G5.12, its names upright in the hue's text shade (`ramps.grouped`, `legend.grouped_row`, tested). No chart draws them yet: the stacked bars give a group's second part a tint of its first instead (07 #10). (Phase 3, stacks: stacks of several totals whose parts are in groups, `data.parts.groups`, now draw them with the grouped legend, P3-39; one total's separated overlaps keep the tint.)

### D-50. Dark backgrounds and plot fills

- **Question.** The book shows how a dark background reverses the reading of a ramp (s. 0113) but no corrected chart, and lists dark backgrounds among the eye candy to avoid (RP6-1); a fill behind the plot is not discussed.
- **Options.** Support dark backgrounds with reversed ramps; do not support them.
- **Recommendation.** No dark background and no plot fill in the plain style; if a later house style uses one, reverse the ramps and add white to the reserved colours. **Reason:** it follows RP6-1 and the owner's plain style.
- **Rules that depend on it:** G3.9, G4.14. **Sources:** 03-04 §8 Q5; 00-01 §8 Q9.

### D-51. Small formatting variants

- **Question.** (i) The book writes negatives with a hyphen ("-$20K"); a true minus sign is typographically correct. (ii) Pie labels appear on one line ("West, $728K") and on two (name above value). (iii) Do data labels share the scale's unit and decimals, and how precise are percentages?
- **Options.** As in the question.
- **Recommendation.** (i) A true minus sign, before the currency symbol; (ii) two lines; (iii) data labels share the scale's unit, with decimals set by G6.10; whole percentages unless close values must be told apart. **Reason:** Typst sets the minus correctly with no change of meaning; the two-line form is the pie chapter's.
- **Rules that depend on it:** G6.16, G7.27. **Sources:** 05 §8 Q13; 03-04 §5.8; 07 §5.2; 05 §5.1.

### D-52. Table details

- **Question.** (i) The author "hardly ever" uses zebra stripes: forbid them outright? (ii) The improved Chapter 1 table drops the "Total:" label that the cheat sheet keeps. (iii) The rules differ: Chapter 1 has two light rules (#BFBFBF); Chapter 10 a mid-grey rule under the header and a darker one above the totals. (iv) What if a companion table has more columns than fit under the graph's categories?
- **Options.** As in the question.
- **Recommendation.** (i) Forbid them; (ii) keep a "Total" label in regular weight; (iii) Chapter 10's rules, since that is the tables chapter; (iv) put the table beside the graph, transposed. **Reason:** each follows the more specific chapter.
- **Rules that depend on it:** G7.38, G7.40. **Sources:** 08-12 §8 Q9, Q10; 00-01 §8 Q6; 00-01 R1-08; 08-12 R10-13.
- **As built (phase 2, series; SPEC §6.7.13), for a table with the graph only.** No rules and no stripes at all (G7.46), the numbers bare and grouped with one number of decimals, the unit the scale's. (iv): under a vertical chart, cells that would come within half an em of each other leave the table out with a warning; the transposed table beside the graph is not built yet. Beside horizontal bars the table is built, one column per series, each row level with its category (P2-01 `horizontal-table`). Tables alone (i)–(iii) wait for phase 3. **Owner (1 Oct 2026):** under several series the rows of a table with the graph are keyed by name then square (`table.row_key: auto` is `swatch_after`), and the legend is left out, the table being the key (`table.legend: keep` includes it; P2-01 and its `legend` variant). The names stay where they stand without keys, right-aligned with the scale's labels, and each square's left edge stands on the gridlines' left end, the value axis: the name-to-square gap is the scale gap (9.0 pt in P2-01), the table no wider, the square 8.9 pt from the first value, about even with its name. Compared and not chosen: squares in a column at the table's left, squares before the names, names in the strong shade (3.63 and 3.78 : 1, failing D-6 (a)), pale pills and flat halos.
- **As built (phase 3, tables; SPEC §6.16), for a table alone.** (i) stripes refused under R10-16, drawn only through an override (P3-40 `zebra`, P3-41 `heavy`); (ii) "Total" kept, regular weight, right-aligned where the names end (P3-40, P3-41); (iii) Chapter 10's rules, #A3A3A3 under the header and #656565 above the totals. Bold header, totals or Total column refused under R10-22, likewise drawn through an override (P3-40 `bold`).

### D-53. Filters and animation

- **Question.** "Too much data for any arrangement" (filters) and "small changes" (animation) are quantified only by examples (about 100 regions against 6; two points moving).
- **Options.** Set thresholds now; leave until interactive output exists.
- **Recommendation.** Leave it: the output is static SVG, so there are no filters or animations; revisit if interactive output is added. **Reason:** nothing depends on it yet.
- **Rules that depend on it:** G1.22, G1.23. **Sources:** 13-15 §8 Q13.

### D-54. Test figures whose data contradict themselves

- **Question.** Some figures are internally inconsistent: a chart called thirteen-colour has twelve lines; a headcount reads 94.5 in one figure and 945.5 in another; a funnel's bars disagree with its labels; an animation's "average" is not the mean of its lines; "HQ" and "Int'l" name the same region; table totals differ by 1 from their rows.
- **Options.** Copy the figures as printed; rebuild from printed labels and recompute.
- **Recommendation.** Rebuild from the printed labels and compute derived values (averages, changes), keeping printed totals where the text relies on them, and note each case in the test. **Reason:** tests must check the generator's arithmetic, not reproduce the book's slips.
- **Rules that depend on it:** none; it concerns the test cases. **Sources:** 03-04 §8 Q13; 08-12 §8 Q16; 13-15 §8 Q14; 00-01 §7 F2.

### D-55. Stating the insight: which charts must

**Decided (owner, 30 Sep 2026).**

- **Question.** G8.2 [L] asks every communication chart to state its key insight in the title or a callout, and says a title that only names the data is not enough. Yet the book's Part 1 figures, and a report's reference charts, have descriptive titles ("Headcount by Region, January 2022"), and a chart made so that readers can look values up has no single insight to state. Is G8.2 locked for every chart?
- **Options.** (a) Locked for every communication chart. (b) Locked only when the job is to answer a question, persuade, or make the audience aware; free for look-up charts and analysis charts. (c) Guided, a warning only.
- **Decision.** (b). A chart whose job (`job.kind`) is `answer`, `persuade` or `make_aware` states its insight, in an insight title (`title.kind: insight`, with the descriptive line under it, D-5) or a callout; one whose job is `lookup`, or made for `analysis`, or `explain` or `other` without a stated insight, may have a descriptive title, and nothing is reported. The generator refuses the first kind with a descriptive title (R0-10, overridable), and the rebuilt book figures carry the job kind their source shows (most are look-up charts). **Reason:** look-up charts matter to the owner; the job, which the spec must state anyway (R0-07), says which charts carry a message.
- **Rules that depend on it:** G8.2, G6.8, G0.4. **Sources:** 13-15 RP5-1; 00-01 R0-10; owner.

### D-56. An inset's labels: repeated, or one shared axis

**Decided (owner, 30 Sep 2026).**

- **Question.** G2.27 asks an inset to be a complete small chart with its category or time labels repeated. Yet the book's own insets often label the two panels once: the compact inset over its line chart (p. 84, s. 0081) carries the months for both, and the bar insets of p. 85 (s. 0082, left) and p. 325 (s. 0297) leave the names to the main chart, while others repeat them (s. 0077, s. 0082 right); its merged charts (p. 68–72, s. 0067–0070), panels on one time axis, label it once at the foot of the stack. Must an inset always repeat the labels?
- **Options.** (a) Always repeat them (locked, as G2.27 read). (b) Repeat them by default; an inset aligned with its main chart (the same slots or categories at the same positions, above or below it) shares one axis when space is short, a guided decision. (c) Share whenever the inset is aligned.
- **Decision.** (b). The repetition stays the default (G2.27 [L]); sharing is its guided exception. In the generator (`inset.share_axis: auto`), space is short when the spec asks for a compact main chart, the book's compact layout, or when the figure, labelled twice, would be taller than `--inset-share-height` (0.9) of its width; `true` or `false` against that is a guided finding. The axis kept is the main chart's, at label size, outside the frames: at the foot of the stack under an inset above (the merged charts, s. 0297), between the panels over an inset below (s. 0082, left); the inset drops its own. An inset beside horizontal bars, or an outlier inset over categories, is not aligned above or below and always repeats them. With it the owner fixed an inset's height at most about its main chart's (`--inset-max-height`, 1). **Reason:** two panels on the same positions, stacked, read as one chart, as the merged charts show; repeating their labels costs height where it is short, and the main chart keeps its own so that it stays complete.
- **Rules that depend on it:** G2.27, G2.29. **Sources:** 02 §5.4 (d) step 8; 02 #13, #14; 08-12 §5.4; owner.

### D-58. Verifying what a chart says, and checking charts made by AI

**Decided (owner, 1 Oct 2026).**

- **Question.** The evaluation of 1 Oct 2026 (`eval/REPORT.md`: twelve requests, Opus and Sonnet, unaided, with the guide, with the generator) found that help removed the wrong drawings but not the wrong statements. Of eleven misleading responses, two were wrong forms, both unaided; nine came from arms with help, seven of them a wrong date, figure or claim in the title, the notes or the reply. Neither the guide nor `AUTHORING.md` asked the maker to check what the chart and its message say against the data. How are charts made by AI to be checked?
- **Options.** (a) A locked rule: the maker recomputes every stated fact before delivery. (b) As (a), and, once the figures are made, a check against the guide by a fresh agent that did not make them. (c) A reviewer only.
- **Decision.** (b). G8.27 [L] and G9 #71: recompute from the data every number, date, name and claim on the chart, in its notes and in the message sent with it; a fact taken from neither the data nor the request is an assumption and is said to be one. Charts are made with the generator (`AUTHORING.md`, which now also asks the maker to map each wish of the request to a field or a stated reason, and never to let the year, special categories, good and bad, or units default silently), then checked by a fresh agent following `CHECKING.md`, given the requests, the data, the rendered charts, the replies and the generator's reports, not the maker's reasoning. Making and checking are Opus's job. Where the check needs something the generator cannot draw, the gap is reported and the generator extended, never the chart redrawn in another tool. **Reason:** in the evaluation Opus made no misleading chart under any condition, while Sonnet's errors grew with what it asserted; the reviewers, working as `CHECKING.md` says, caught every wrong statement the makers had missed in their own review; a generator enforces form but cannot know whether a title's claim is true.
- **Rules that depend on it:** G8.27, G0.10, G8.26, G9 #71. **Sources:** owner, 1 Oct 2026; `eval/REPORT.md`.

### D-59. "%" on a horizontal scale

**Decided (owner, 1 Oct 2026).**

- **Question.** G2.24 asks every percentage label to carry "%"; G2.19 gives a horizontal scale bare numbers, with the unit in parentheses in its title below, as the book writes horizontal scale titles. On a horizontal % scale they disagree, and the evaluation's reviewers scored the same form both ways (B05, B08, B09).
- **Options.** (a) "%" on every label ("0%, 20% … 100%"; "+20%", "−20%" on a change scale), the title below without "(%)". (b) Bare numbers, "(%)" in the title.
- **Decision.** (a): as on vertical scales and change scales; G2.19 makes percentages its exception. **Reason:** the sign belongs to each value, and a reader of one bar's label should not have to look down for it.
- **Rules that depend on it:** G2.19, G2.24, G9 #30, #31.

### D-60. Special categories in black and white

**Decided (owner, 1 Oct 2026).**

- **Question.** G4.15 reserves grey for special categories (total, average, all others, missing), but in a chart printed in black and white every part is grey; in the evaluation (B04, a pie for a photocopied handout) reviewers could not say whether "Other" was set apart.
- **Options.** (a) The regular parts told apart by textures or patterns, the special categories plain flat greys with none, placed last. (b) No distinction in black and white: the special category named and placed last.
- **Decision.** (a).
- **As built.** Not yet: the generator textures only good and bad in black and white (D-18); textures for regular parts are to come, with the special categories left flat.
- **Rules that depend on it:** G4.15, G4.25.

## Questions that need no decision

- 00-01 §8 Q2 (when gridlines may go): answered by R2-32 and the figures; see the exceptions in G3.6.
- 02 §8 Q13 (time direction in tables): answered by R2-38; see G2.36.
- 07 §8 Q11 ("line chart or variant"): the variants are the time-chart types of G1.10 step 6.
- 08-12 §8 Q3 (sorting and orientation of bars): stated in Part 1 (R2-01, R2-04); the numeric switch is in [D-34].
- 08-12 §8 Q11 (smoothed lines missing from Chapter 12's key takeaway): kept on the avoid-list, since R6-39 is a key takeaway of its own.
- 08-12 §8 Q15 (combos with two measures): answered by R2-14 and R12-1: a merged or index chart.
- 13-15 §8 Q11 (pie charts): Chapter 7 states when a pie is right (G1.36).
- 05 §8 Q1 (serif or sans-serif) and 13-15 §8 Q12 (shadows): settled by the owner.
