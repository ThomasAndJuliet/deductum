//! Rule titles of the original implementation (charts/chartgen/rules.py, RULES), keyed by rule ID.
//! A finding of one of these rules carries its title in reports, as the original's does.
pub const RULE_TITLES: &[(&str, &str)] = &[
    ("02 D1", "category order and orientation"),
    ("02 D2", "must the scale include zero"),
    ("02 D5", "interval and snapped limits"),
    (
        "6.10",
        "small multiples: equal panels, abutting, divided by light separators, no box round a panel",
    ),
    (
        "6.12",
        "a title connector: a thin grey curved arrow from the end of the title to the mark it names",
    ),
    (
        "6.13",
        "a date marked on a time axis: a thin line across the plot at the date, its label beside it",
    ),
    (
        "6.4",
        "the unit phrase on the top label, or on its own line",
    ),
    (
        "6.6",
        "an inset: a frame round the magnified marks, a larger frame round a complete small chart, two links forming a widening cone, beneath the frames; nothing of the main chart covered",
    ),
    ("6.7", "bar geometry"),
    ("CG-1", "text boxes do not overlap each other or the marks"),
    ("CG-2", "the unit line takes at most two lines"),
    (
        "CG-3",
        "requested but not drawn in this stage of the generator",
    ),
    ("CG-4", "the spec is well formed"),
    (
        "CG-5",
        "the drawing is written in the chart vocabulary (SPEC §4.3)",
    ),
    (
        "CG-6",
        "every time label clear of the time axis's dividers by --divider-clear (owner, 30 Sep 2026)",
    ),
    (
        "CG-7",
        "a gridline or connector crossing a value label stops short of it: the label's knock-out halo (owner, 1 Oct 2026)",
    ),
    (
        "CG-8",
        "notes under the chart: every mark with its note and every footnote with its mark, the notes below everything else, wrapped to the chart's width, never clipped or dropped (owner, 1 Oct 2026)",
    ),
    (
        "G4.29",
        "a linear cut on rounded values, for fairly even values or when the extremes are the message",
    ),
    (
        "G4.30",
        "natural breaks, then rounded, for skewed values when the pattern among the many is the message",
    ),
    (
        "G4.31",
        "equal-count steps only when equal representation is the point, said in the key",
    ),
    ("G4.32", "breaks set by hand only for a stated reason"),
    (
        "G4.33",
        "logarithmic 1–2–5 steps for ratio-scaled amounts over orders of magnitude, zero a step of its own",
    ),
    (
        "G4.34",
        "the breaks in the key rounded and formatted as a set; the lightest step distinct from the page and the missing grey",
    ),
    (
        "R0-02",
        "communication or analysis: analysis charts turn refusals into warnings",
    ),
    ("R0-07", "a communication chart states its job"),
    (
        "R0-10",
        "the key insight stated in the title or a callout, when the job is to answer, persuade or make aware (G8.2, owner 30 Sep 2026)",
    ),
    (
        "R0-12",
        "a missing value is never drawn as zero; a known zero is labelled",
    ),
    (
        "R1-02",
        "non-data elements light and thin, lighter than every data colour",
    ),
    (
        "R1-03",
        "no frame round the chart or the plot, no outline on marks",
    ),
    ("R1-04", "no tick marks"),
    ("R1-05", "no axis line along the value scale"),
    ("R1-06", "light gridlines on the value scale"),
    ("R1-07", "the zero line drawn as a gridline"),
    ("R1-09", "gridlines beneath the marks"),
    ("R10-1", "a graph by default"),
    (
        "R10-11",
        "a table alone only for lookup, export or values not comparable",
    ),
    (
        "R10-12",
        "a table's digits: those its purpose needs, one number of decimals down each column",
    ),
    (
        "R10-13",
        "a table's rules: one thin rule under the header and one above the totals; no grid or box",
    ),
    (
        "R10-14",
        "a table's columns close together, a wider gutter before a Total column",
    ),
    ("R10-16", "no zebra stripes"),
    (
        "R10-20",
        "numbers not to be compared left-aligned, each with its own unit",
    ),
    ("R10-22", "no bold header, totals row or Total column"),
    (
        "R10-23",
        "the few cells that matter on a light fill, nothing else",
    ),
    (
        "R10-24",
        "dates in a column with every part of fixed width (05-Jan-2021)",
    ),
    ("R10-25", "the unit once, in the title, never in every cell"),
    (
        "R10-7",
        "exact values needed: a table aligned with the graph",
    ),
    (
        "R10-9",
        "graphics built into a table whose audience refuses graphs: shading, sparklines, bars in cells",
    ),
    ("R11-2", "series told apart by colour, not by chart type"),
    (
        "R11-3",
        "mark types combined only for series of different roles",
    ),
    (
        "R11-4",
        "a reference per bar as a short flat mark across it, labelled once",
    ),
    ("R12-1", "the avoid-list: its types replaced"),
    (
        "R12-2",
        "no word cloud: sorted horizontal bars of the counts",
    ),
    ("R12-3", "no packed bubbles: a sorted horizontal bar chart"),
    ("R12-4", "no bullet graphs: action dots"),
    (
        "R12-5",
        "no funnel: the stage counts as bars and the % change between stages",
    ),
    ("R12-6", "no radar: a heatmap table"),
    ("R12-7", "no 3-D effects on a 2-D chart"),
    ("R14-1", "no animation in a static chart"),
    (
        "R15-1",
        "title and callouts answer what matters, why care, what explains it, what to do",
    ),
    (
        "R15-11",
        "good values green and bad ones red, by the dividing value",
    ),
    (
        "R15-12",
        "good and bad told apart without colour in black and white (audience.bw_risk), which serves readers with colour-vision deficiency too: a texture of two tones of the colour, hatch for bad and dither for good, a bad dot filled (D-18, owner 1 Oct 2026); or, a style's choice for publications to unknown readers, the colour-blind-safe pair (orange bad, blue good)",
    ),
    (
        "R15-13",
        "calculated values where raw ones leave arithmetic to the reader",
    ),
    (
        "R15-14",
        "deviations from the comparison value, sorted by the deviation",
    ),
    ("R15-15", "ratios for entities of different size"),
    (
        "R15-16",
        "indexed values for patterns of change of series not directly comparable",
    ),
    (
        "R15-2",
        "the action in the title, what matters beside the highlight, the explanation where the cause shows",
    ),
    (
        "R15-5",
        "highlight what the insight names, by a method chosen for the marks",
    ),
    ("R15-6", "no new hue, no bigger mark for a highlight"),
    (
        "R15-7",
        "comparison values where the audience lacks context: targets and budgets before averages, previous periods and peers",
    ),
    (
        "R15-8",
        "no comparison value when the insight compares the items shown",
    ),
    ("R15-9", "no comparison value rather than a doubtful one"),
    ("R2-01", "nominal categories sorted by value, largest first"),
    (
        "R2-02",
        "ordinal categories and time periods keep their order, never sorted by value",
    ),
    (
        "R2-04",
        "category labels fit under vertical bars, or the chart turns",
    ),
    (
        "R2-05",
        "large magnitudes stated once, not as trailing zeros",
    ),
    (
        "R2-06",
        "magnitudes spelled out unless the audience's abbreviation is known",
    ),
    (
        "R2-07",
        "the unit phrase horizontal, beside its scale, never in the title",
    ),
    (
        "R2-08",
        "all text horizontal: wrapped, or the chart turned, never rotated",
    ),
    ("R2-09", "steps of 1, 2 or 5 × 10^n (2.5 allowed)"),
    (
        "R2-10",
        "equal intervals, labels placed in proportion to their values",
    ),
    ("R2-11", "a unit phrase specific enough for one reading"),
    (
        "R2-12",
        "the title block gives what, by what, and the period with its year",
    ),
    ("R2-13", "one unit per scale"),
    (
        "R2-14",
        "no dual axis for two different variables: a merged chart, or an index chart",
    ),
    (
        "R2-16",
        "a merged chart: panels sharing one category or time axis, each with its own scale and unit",
    ),
    (
        "R2-17",
        "an index chart: every series relative to a common base period",
    ),
    ("R2-18", "no broken scale"),
    ("R2-19", "no log scale in everyday charts"),
    (
        "R2-20",
        "outliers that flatten the small values: the full scale kept, an inset on their own scale from zero",
    ),
    (
        "R2-21",
        "scale labels formatted as a set, one number of decimals",
    ),
    ("R2-22", "zero on every value scale by default"),
    (
        "R2-23",
        "a narrow range far from zero whose small differences matter: zero kept, an inset with a truncated scale",
    ),
    (
        "R2-24",
        "bars start at zero; truncation only for lines and dots",
    ),
    ("R2-25", "no scale starting close to zero but not at zero"),
    ("R2-26", "a theoretical minimum as the lower limit"),
    (
        "R2-27",
        "the upper limit leaves 10–20 % of room (the data never touch it)",
    ),
    ("R2-28", "no scale past a theoretical limit"),
    ("R2-29", "all negative: the mirror image"),
    ("R2-30", "mixed signs: room at both ends"),
    (
        "R2-32",
        "fewer values than scale labels: label the values directly",
    ),
    ("R2-33", "a threshold the message needs falls on a gridline"),
    ("R2-34", "at most about eight scale labels"),
    ("R2-37", "time runs left to right"),
    (
        "R2-39",
        "a multi-level time axis: finest unit on row 1, the coarser under its span",
    ),
    ("R2-40", "crowded time labels thinned"),
    (
        "R2-42",
        "an incomplete period left out or drawn visibly differently",
    ),
    ("R2-43", "time bins of equal length"),
    (
        "R3-11",
        "groups of categories: a hue per group, its members shades of it",
    ),
    (
        "R3-12",
        "a quantity shown by colour: a sequential ramp, darker meaning more",
    ),
    (
        "R3-13",
        "a diverging palette only about a dividing value the audience sees",
    ),
    (
        "R3-14",
        "a diverging palette: two ramps meeting at a quiet neutral, intensity growing with distance",
    ),
    (
        "R3-15",
        "no bright yellow or amber at or near the dividing value",
    ),
    ("R3-16", "grey reserved for special categories"),
    ("R3-17", "captions naming a side coloured to match it"),
    ("R3-18", "no red in a regular series"),
    (
        "R3-19",
        "colours that must be told apart survive colour-vision deficiency",
    ),
    ("R3-2", "muted colours"),
    (
        "R3-20",
        "static output: one file, plain red and green in colour, textured in black and white; or, where a style chooses the safe pair, the colour-blind-safe version and the red/green one as a second file (in black and white one file, textured)",
    ),
    ("R3-21", "at most eight regular colours"),
    (
        "R3-22",
        "more than eight colours only with every series labelled directly",
    ),
    ("R3-3", "one colour when nothing needs telling apart"),
    (
        "R3-4",
        "no colour difference that means nothing: no hue per bar, no shading by value",
    ),
    ("R3-6", "balanced palette lightness"),
    (
        "R3-7",
        "a highlight strengthens the mark's own hue; never a new hue",
    ),
    (
        "R3-8",
        "ordered categories: one hue, lightness in their order",
    ),
    ("R3-9", "darker means more on a light background"),
    (
        "R4-1",
        "label directly rather than with a legend; one series needs neither",
    ),
    ("R4-10", "legend keys in the form of their marks"),
    (
        "R4-2",
        "a legend when direct labels would move too far (the collision test)",
    ),
    (
        "R4-4",
        "legend entries in the order of the colours in the chart",
    ),
    (
        "R4-5",
        "a line chart's legend in the order of the lines at the latest point",
    ),
    ("R4-6", "a legend above the plot, below the title"),
    (
        "R4-7",
        "a time-series line chart's legend right of the plot, beside the latest values",
    ),
    (
        "R4-8",
        "a grouped legend: each group's members together, a rule under them, the group's name below",
    ),
    (
        "R4-9",
        "a continuous colour scale keyed by a graduated bar: its title, end values and dividing value",
    ),
    ("R5-10", "digits hidden from five integer digits"),
    (
        "R5-11",
        "hidden digits never mistaken: the unit the data are written in (unit.given_in) agrees with the values",
    ),
    ("R5-14", "four or more integer digits grouped in threes"),
    ("R5-16", "rounding half away from zero"),
    (
        "R5-17",
        "a set of numbers shares prefix, suffix and decimals",
    ),
    ("R5-20", "months in letters, not numbers"),
    ("R5-21", "years in four digits"),
    (
        "R5-26",
        "long labels wrapped onto at most two lines, never rotated or shrunk",
    ),
    (
        "R5-27",
        "the period in the title block, not a separately styled subtitle",
    ),
    (
        "R5-5",
        "the maker's comments set apart from the labelling, in a colour used for nothing else",
    ),
    ("R5-6", "at most four text styles"),
    ("R5-7", "text large enough at final size"),
    ("R5-8", "text contrast"),
    (
        "R6-1",
        "a time series is classified first: an irregular one states whether it holds states or events",
    ),
    (
        "R6-10",
        "irregular events whose pattern matters: aggregated into regular intervals first",
    ),
    (
        "R6-11",
        "thin bars: one narrow bar per event at its instant, from zero, nothing between",
    ),
    (
        "R6-12",
        "individual irregular events: thin bars, never a standard line",
    ),
    (
        "R6-13",
        "the time tree for more than two periods, not a breakdown, not on a map",
    ),
    ("R6-15", "two periods: an arrow chart"),
    (
        "R6-16",
        "a slope chart only in the rare case of rank changes; otherwise an arrow chart",
    ),
    (
        "R6-17",
        "change between two periods: an absolute panel always with a % panel, never clustered bars",
    ),
    ("R6-18", "an unfamiliar but accurate type, taught"),
    (
        "R6-2",
        "too few known values for a line: dots only, never a line",
    ),
    (
        "R6-22",
        "a missing value is a gap in the line, never joined across",
    ),
    ("R6-23", "a value not known is never drawn as zero"),
    (
        "R6-24",
        "dotted lines only for values of lower certainty (projections, estimates, targets)",
    ),
    (
        "R6-25",
        "point markers when a known value is isolated between gaps",
    ),
    (
        "R6-26",
        "point markers when a straight run hides known intermediate values",
    ),
    ("R6-27", "otherwise no point markers"),
    ("R6-28", "context lines keep their colours (no grey-out)"),
    (
        "R6-29",
        "one line emphasised: thicker, in the strong shade of its own hue",
    ),
    (
        "R6-3",
        "a standard line only for a regular series, one value per interval",
    ),
    (
        "R6-30",
        "black-and-white reproduction likely: a marker symbol per line at every point",
    ),
    (
        "R6-31",
        "no dash pattern to tell series apart; dashes only for lower certainty",
    ),
    (
        "R6-32",
        "each line labelled at its right end, the labels packed in order",
    ),
    (
        "R6-33",
        "a projection: a dotted continuation in the line's colour, labelled at its end",
    ),
    (
        "R6-34",
        "an average or other reference series in the reserved dark grey, its label bold",
    ),
    (
        "R6-35",
        "at most about five lines, more only if they rarely cross and are labelled directly",
    ),
    (
        "R6-36",
        "a busy line chart split into a stack of small charts, up to about 20–30",
    ),
    (
        "R6-37",
        "small multiples on one shared scale; a scale per panel when shapes of very different sizes are compared",
    ),
    ("R6-38", "no time chart turned on its side"),
    (
        "R6-39",
        "no smoothed line through data points (bumps charts excepted)",
    ),
    (
        "R6-4",
        "a line for a regular series whose pattern of change is the point",
    ),
    (
        "R6-40",
        "a bumps chart's curves flat at every point, never overshooting",
    ),
    (
        "R6-41",
        "a line across ordered categories only for a data-savvy audience",
    ),
    ("R6-42", "never a line across unordered categories"),
    (
        "R6-5",
        "bars for values over time when the periods are the point",
    ),
    ("R6-7", "a bumps chart when changes in rank are the message"),
    (
        "R6-8",
        "a step chart: level from each change to the next, the last value to the as-of date",
    ),
    (
        "R6-9",
        "irregular states: a step chart, never a standard line",
    ),
    (
        "R7-01",
        "the breakdown chosen by the message, never by starting from the pie",
    ),
    (
        "R7-02",
        "the meaningful-total check before a pie, stacked chart or treemap",
    ),
    (
        "R7-03",
        "the all-parts check before a pie, stacked chart or treemap",
    ),
    (
        "R7-04",
        "the sign check: no pie, treemap or stack of mixed signs",
    ),
    (
        "R7-05",
        "the overlap check: disjoint parts before a pie, stacked chart or treemap",
    ),
    ("R7-06", "one total, parts compared precisely: a bar chart"),
    (
        "R7-07",
        "one total, fractions of it: a pie of at most 6-7 parts",
    ),
    ("R7-08", "one total, a subtotal as an amount: a stacked bar"),
    ("R7-11", "mixed signs and running subtotals: a waterfall"),
    (
        "R7-13",
        "a pie at most 6-7 parts: seven flagged, eight or more rerouted (D-23)",
    ),
    (
        "R7-14",
        "a wrapped bar chart: one sorted list cut into columns on one scale",
    ),
    (
        "R7-15",
        "a treemap, squarified, largest at the top left; a pie when the parts fit one",
    ),
    (
        "R7-16",
        "a treemap's small tiles unlabelled, names shortened with an ellipsis; groups coloured and labelled",
    ),
    (
        "R7-17",
        "minor parts lumped into a grey All others, placed last",
    ),
    (
        "R7-19",
        "several totals, parts compared across them: clustered bars",
    ),
    (
        "R7-20",
        "several totals, the totals compared: multiple stacked bars",
    ),
    (
        "R7-21",
        "several totals, fractions within each: multiple pies (rare)",
    ),
    (
        "R7-22",
        "not all parts of the totals shown: merged bars, clustered second best",
    ),
    (
        "R7-24",
        "several totals: the parts in one order and one colour in every pie",
    ),
    (
        "R7-25",
        "a total over time: the total's change, parts or fractions, patterns or periods",
    ),
    (
        "R7-26",
        "over time, parts compared without the total: a line per part",
    ),
    (
        "R7-27",
        "over time, parts compared and the total matters: a total panel above the lines",
    ),
    (
        "R7-29",
        "over time, fractions and the total's change: absolute stacks",
    ),
    (
        "R7-30",
        "over time, fractions without the total: 100 % stacks",
    ),
    ("R7-32", "every slice its own colour"),
    ("R7-33", "no slice borders: thin gaps in the page colour"),
    (
        "R7-34",
        "slices labelled directly, outside, never with a legend; leaders where needed",
    ),
    (
        "R7-35",
        "slices by size, largest first, from 12 o'clock clockwise",
    ),
    ("R7-36", "zero-value parts kept and labelled"),
    ("R7-37", "a leader to every zero or very small slice"),
    (
        "R7-38",
        "never an exploded slice: a darker shade or a border to highlight",
    ),
    ("R7-39", "a pie, not a donut"),
    ("R7-40", "a pie-averse audience: another type"),
    (
        "R8-13",
        "values at locations over time: lines on regions, or one line chart",
    ),
    (
        "R8-2",
        "a map only when location matters more than precision",
    ),
    (
        "R8-5",
        "no choropleth unless the regions are of similar size",
    ),
    (
        "R9-1",
        "a bar chart always considered, a more specific type preferred",
    ),
    ("R9-10", "long category names: horizontal bars"),
    (
        "R9-11",
        "merged bars: horizontal, one colour, the names once at the left, a scale and a heading per panel",
    ),
    (
        "R9-12",
        "clustered bars: the bars of a cluster touching, the clusters apart, a legend",
    ),
    ("R9-5", "bar scales are never truncated"),
    (
        "R9-6",
        "a truncated inset's bars fade to transparent at the bottom; its lines and dots leave through its edge",
    ),
    ("R9-7", "an empty bar slot says what it means"),
    ("R9-8", "gaps between bars narrower than the bars"),
    ("R9-9", "catch-all and missing categories last"),
    (
        "RP2-7",
        "choose in two stages: the group, then the type within it",
    ),
    ("RP2-8", "time-related groups beyond data over time"),
    (
        "RP3-2",
        "a third variable goes to small multiples, not a table",
    ),
    (
        "RP3-3",
        "the arrangement of small multiples chosen for the insight, from the range of candidates",
    ),
    (
        "RP3-4",
        "the series the insight compares directly share a panel",
    ),
    (
        "RP3-5",
        "no single chart of many overlapping series for a general audience",
    ),
    (
        "RP3-7",
        "callouts in empty space, never over marks, a thin arrow to each element they name",
    ),
    (
        "RP3-8",
        "small multiples: scale labels left of the first column, category or time labels under the bottom row, headers once",
    ),
    ("RP3-9", "small multiples: each series labelled once"),
    ("RP5-1", "the insight is the title"),
    (
        "RP6-1",
        "no eye candy: flat, no shadows, background fills or gradients",
    ),
];

/// The original title of a rule, if it has one.
pub fn rule_title(rule: &str) -> Option<&'static str> {
    RULE_TITLES
        .binary_search_by(|(id, _)| (*id).cmp(rule))
        .ok()
        .map(|i| RULE_TITLES[i].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_the_original_rule_titles() {
        // charts/chartgen/rules.py: ("R0-07", "a communication chart states its job", ...)
        assert_eq!(
            rule_title("R0-07"),
            Some("a communication chart states its job")
        );
        assert_eq!(RULE_TITLES.len(), 241);
        assert!(RULE_TITLES.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(rule_title("CG-SVG-PARTIAL"), None);
        assert_eq!(
            crate::Finding::new("R0-07", "error", "x").title.as_deref(),
            rule_title("R0-07")
        );
    }
}
