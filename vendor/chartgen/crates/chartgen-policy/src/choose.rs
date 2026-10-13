//! Choosing, or checking, the chart type and its orderings (`choose.choose` and its refusals).
use crate::procedure::{
    Route, SLOPE_MAX, STEPS, State, TWO_PERIODS_DEFAULT, checks, enough_values, irregular_type,
    overlap_pairs, procedure, type_words, words_of,
};
use crate::rules::{decide, finding, guide_ids, guided, info, refusal};
use crate::view::{REFERENCE_ROLES, View};
use chartgen_model::Report;
use serde_json::json;

/// The chart needs a type or feature a later stage of the original generator draws
/// (`choose.Unsupported`): reported as CG-3, status unsupported.
#[derive(Debug, Clone)]
pub struct Unsupported(pub String);

/// The Choice of the original: the type drawn, orientation (auto, vertical, horizontal), sort
/// (desc, asc, given) and the series sorted by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub type_: String,
    pub orientation: String,
    pub sort: String,
    pub sort_by: String,
}

const WATERFALL: &str = "waterfall";
const SLOPE: &str = "slope";
const LINE_TOTAL: &str = "line_total";
pub(crate) const PART_TYPES: [&str; 8] = [
    "pie",
    "donut",
    "stacked_bar",
    "stacked_bar_100",
    "stacked_area",
    "stacked_area_100",
    "treemap",
    "waterfall",
];
/// `choose.STAGE_TYPES | LAYOUTS`: the types the original draws.
const DRAWABLE: [&str; 22] = [
    "bar",
    "dot",
    "line",
    "step",
    "thin_bar",
    "arrow",
    "bumps",
    "pie",
    "donut",
    "treemap",
    "wrapped_bar",
    "slope",
    "table",
    "heatmap",
    "action_dots",
    "funnel_bars",
    "stacked_bar",
    "stacked_bar_100",
    "waterfall",
    "stacked_area",
    "stacked_area_100",
    "line_total",
];
pub(crate) fn drawable(t: &str) -> bool {
    DRAWABLE.contains(&t) || chartgen_statistics::is_statistical(t)
}
pub(crate) fn given_order(t: &str) -> Option<&'static str> {
    Some(match t {
        "waterfall" => "a waterfall's steps in the process's own order (D-43)",
        "table" => {
            "a table's rows in the order the spec gives, its design chosen for the message and for looking values up (G1.17)"
        }
        "heatmap" => {
            "a heatmap's rows and columns in the order the spec gives, the items and attributes as arranged for the message (G7.35, G7.36)"
        }
        "funnel_bars" => "the stages in their order through the funnel (G7.59)",
        t if chartgen_statistics::is_statistical(t) => {
            "the categories in the order the spec gives: a statistical chart compares conditions, not magnitudes (STATS.md)"
        }
        _ => return None,
    })
}

/// The avoid-list (G1.61 [L], 08-12 DP-7): (rule, why, what to supply).
pub(crate) fn avoided(t: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match t {
        "donut" => (
            "R7-39",
            "a donut hides the angles at the centre and reads slightly worse than a pie",
            "type pie (or auto)",
        ),
        "dual_axis" => (
            "R2-14",
            "two variables against two value axes suggest false crossings and similar volatility",
            "the measures in data.measures (a merged chart, when the values matter), or transform index (an index chart, when the patterns of change matter)",
        ),
        "radar" => (
            "R12-6",
            "a radar's shapes do not show which item is best or what each item's strengths and weaknesses are",
            "type heatmap (a heatmap table: items as columns, attributes as rows, the values in coloured cells); circular data such as wind directions excepted, through an override",
        ),
        "funnel" => (
            "R12-5",
            "a funnel's layers allow only rough comparison, so it depends on a label on every layer",
            "type auto with facts.message stage_conversion (the stage counts as bars, the % change from each stage to the next beneath them)",
        ),
        "word_cloud" => (
            "R12-2",
            "font sizes cannot be compared, long words look frequent, and turned words are hard to read",
            "type auto (horizontal bars of the counts, sorted, largest first); decoration excepted, through an override",
        ),
        "packed_bubbles" => (
            "R12-3",
            "bubble sizes are hard to compare and their places suggest relations that do not exist",
            "type auto (a sorted horizontal bar chart)",
        ),
        "bullet" => (
            "R12-4",
            "on a dashboard of bullet graphs the metrics needing action are hard to spot",
            "type auto with facts.message action_needed (action dots)",
        ),
        "choropleth" => (
            "R8-5",
            "large regions grab attention whatever their values, and small ones vanish",
            "facts.location_matters with type auto (coloured dots, bubbles or bars on regions; phase 4); a choropleth only for regions of similar size, through an override",
        ),
        "3d" => (
            "R12-7",
            "perspective distorts the values and a z-axis hides the back marks",
            "type auto (the flat chart; a third variable as the series of clustered bars, R12-8)",
        ),
        "smoothed_line" => (
            "R6-39",
            "a smoothed line overshoots the data and its smoothing is arbitrary",
            "type line (straight segments)",
        ),
        _ => return None,
    })
}

/// `choose.time_facts`: an irregular series says whether its values are states or events (R6-1);
/// a regular one without it is taken as summaries, with an info. Returns true when refused.
pub fn time_facts(v: &View, report: &mut Report) -> bool {
    if v.x_kind() != "time" || v.measure_str("persistence").is_some() {
        return false;
    }
    if v.grid() == Some("none") {
        return refusal(
            report,
            v,
            "R6-1",
            "an irregular series must say whether each value is a state that holds until the next (a price, a balance) or a one-off event (a sale, a visit): the chart type depends on it, and the numbers cannot tell",
            Some("data.measure.persistence"),
            Some("persistence \"state\" or \"event\""),
        );
    }
    info(
        report,
        "R6-1",
        "measure.persistence not given: each value is taken to summarise its period (summary)",
        Some("data.measure.persistence"),
    );
    false
}

/// `choose.line_refusals`: what no line may do. Returns true when the line is refused.
pub fn line_refusals(v: &View, report: &mut Report) -> bool {
    let kind = v.x_kind();
    let mut stop = false;
    if kind == "time" && v.grid() == Some("none") {
        let what = if v.measure_str("persistence") == Some("state") {
            "states: the sloped segments suggest a gradual drift and values that never held (R6-9)"
        } else {
            "events: the segments suggest values between events and can even reverse the trend (R6-12)"
        };
        let supply = match irregular_type(v).0 {
            "step" => "type auto (a step chart)",
            "thin_bar" => "type auto (thin bars)",
            _ => "the events aggregated into regular intervals on a grid (R6-10), then a line",
        };
        stop |= refusal(
            report,
            v,
            "R6-3",
            format!(
                "a standard line joins one value per regular interval; across irregular {what}"
            ),
            Some("chart.type"),
            Some(supply),
        );
    } else if kind == "time" {
        let (ok, why) = enough_values(v);
        if !ok && v.measure_str("persistence") != Some("state") {
            stop |= refusal(
                report,
                v,
                "R6-2",
                format!(
                    "too few values are known to see the pattern of change ({why}): a line would invent one"
                ),
                Some("chart.type"),
                Some("type auto (a dot plot), or the missing values"),
            );
        }
    } else if kind == "nominal" {
        stop |= refusal(
            report,
            v,
            "R6-42",
            "a line across unordered categories implies an order and links between neighbours that do not exist, and reads as change over time",
            Some("chart.type"),
            Some("type auto (bars)"),
        );
    } else if !v.flag("/audience/data_savvy") {
        stop |= refusal(
            report,
            v,
            "R6-41",
            "many readers take any line for change over time: a line across ordered categories is only for a data-savvy audience",
            Some("chart.type"),
            Some("type auto (bars), or audience.data_savvy when the audience is"),
        );
    }
    stop
}

/// `choose.part_refusals`: the four checks on a pie, stacked chart, treemap or waterfall.
pub fn part_refusals(v: &View, t: &str, report: &mut Report) -> bool {
    let what = if t == "donut" {
        "donut".to_owned()
    } else {
        let w = words_of(t);
        w[..w.len() - 1].to_owned()
    };
    let pairs = overlap_pairs(v).join(", ");
    let mut stop = false;
    for c in checks(v) {
        if c.name == "signs" && t == WATERFALL {
            continue;
        }
        match c.state {
            State::Missing => {
                let (question, g, field, supply) = match c.name {
                    "meaningful total" => (
                        "whether the values add up to a total someone would quote",
                        "G0.12",
                        "data.parts.meaningful_total",
                        "data.parts.meaningful_total true or false",
                    ),
                    "all parts" => (
                        "whether every part of the total is shown",
                        "G0.13",
                        "data.parts.all_parts",
                        "data.parts.all_parts true or false",
                    ),
                    _ => (
                        "whether any part contains or overlaps another",
                        "G0.14",
                        "data.parts.overlap",
                        "data.parts.overlap false, or the pairs in data.parts.overlaps ([part, parent])",
                    ),
                };
                stop |= refusal(
                    report,
                    v,
                    c.rule,
                    format!(
                        "before a {what}, the spec must say {question} ({g}): the numbers cannot tell, and a guess could misrepresent them (D-1)"
                    ),
                    Some(field),
                    Some(supply),
                );
            }
            State::Fail => {
                let (message, supply) = match c.name {
                    "meaningful total" => (
                        format!(
                            "a {what} asserts a whole, and these values do not add up to a total anyone would quote (averages, rates, ratios, ratings: parts.meaningful_total false)"
                        ),
                        "type auto (a bar chart)",
                    ),
                    "all parts" => (
                        format!(
                            "a {what} implies the parts shown are the whole, and not every part is shown (a top N or a filtered subset: parts.all_parts false)"
                        ),
                        "type auto (a bar chart; several totals: merged bars), or an All others part completing the total",
                    ),
                    "overlap" => (
                        format!(
                            "a {what}'s separate slices or segments claim disjoint parts, and these overlap{}",
                            if pairs.is_empty() {
                                String::new()
                            } else {
                                format!(" ({pairs})")
                            }
                        ),
                        "the parts made disjoint in the data (Other cars = Cars − Sports cars), or type auto (a bar chart)",
                    ),
                    _ => (
                        format!(
                            "a {what} cannot show parts of mixed signs: areas cannot be negative, and a mixed stack shows neither the net total nor comparable parts"
                        ),
                        "type auto (a bar chart, or a waterfall when running subtotals are the message)",
                    ),
                };
                stop |= refusal(report, v, c.rule, message, Some("chart.type"), Some(supply));
            }
            State::Pass => {}
        }
    }
    stop
}

/// `choose._avoided`: a requested type on the avoid-list. Ok(None) when refused.
fn avoid(
    v: &View,
    t: &str,
    report: &mut Report,
) -> std::result::Result<Option<String>, Unsupported> {
    let (rule, why, supply) = avoided(t).unwrap();
    report.path.push(
        json!({"step": 9, "name": STEPS[9], "rule": rule, "level": "locked",
        "guide": guide_ids(rule),
        "text": format!("chart.type {t} is on the avoid-list: {why}; instead, {supply} (G1.61)")}),
    );
    if refusal(
        report,
        v,
        rule,
        format!(
            "{} charts are on the avoid-list: {why}",
            t.replace('_', " ")
        ),
        Some("chart.type"),
        Some(supply),
    ) {
        return Ok(None);
    }
    if drawable(t) {
        return Ok(Some(t.into()));
    }
    Err(Unsupported(format!(
        "{} charts are not drawn: the avoid-list (G1.61)",
        t.replace('_', " ")
    )))
}

/// `choose._periods_gate`: two periods drawn as bars. Returns true when refused.
fn periods_gate(v: &View, route: &Route, t: &str, report: &mut Report) -> bool {
    if let Some((reason, names)) = &route.periods_gate {
        let what = format!(
            "two periods ({}, {}) with facts.message periods",
            names[0], names[1]
        );
        if let Some(reason) = reason {
            let mut f = finding(
                "R6-15",
                "info",
                format!(
                    "{what}: bars of each period's values, by the guide's exception for individual periods (G1.51), where the procedure gives an arrow chart (G1.46)"
                ),
                Some("facts.message"),
                Some("guided"),
            );
            f.reason = Some(reason.clone());
            f.recommended = Some("arrow".into());
            report.findings.push(f);
        } else {
            let mut f = finding(
                "R6-15",
                "warn",
                format!(
                    "{what} but no reason: the change between two periods is the point unless the request says otherwise, so the route stays the arrow chart; {TWO_PERIODS_DEFAULT}"
                ),
                Some("facts.message"),
                Some("guided"),
            );
            f.recommended = Some("arrow".into());
            report.findings.push(f);
        }
    }
    if let Some(across) = &route.across
        && (t == "arrow" || t == "bar")
    {
        let (a, b) = (&across[0], &across[1]);
        return refusal(
            report,
            v,
            "R6-17",
            format!(
                "the two series are two periods ({a}, {b}): drawn side by side they show each category's two values with nothing to suggest change and no change in % (the book's most common mistake, R6-14; G1.46, G1.47)"
            ),
            Some("data.series"),
            Some(&format!(
                "for the change, the default: x the two periods (kind time, values [{a}, {b}] on their grid) with a series per category, type auto (an arrow chart, the change in units and in %); or the change alone as bars (data.comparisons, kind previous_period, the {a} values, with transform deviation, relative for %); or, where each period's own values are what the request needs, facts.message periods with the reason in reasons[\"facts.message\"]"
            )),
        );
    }
    false
}

/// `choose._record`: the path into the report, and the defaults that stand in for facts (D-1).
fn record(report: &mut Report, route: &Route) {
    for s in &route.steps {
        let mut s = s.clone();
        let rule = s["rule"].as_str().unwrap_or("").to_owned();
        s["guide"] = json!(guide_ids(&rule));
        s["group"] = json!(route.group);
        report.path.push(s);
    }
    for (rule, path, message) in &route.infos {
        info(report, rule, message.clone(), Some(path));
    }
}

/// `choose.several_series`: several series of one measure over categories or periods.
fn several_series(
    v: &View,
    t: &str,
    report: &mut Report,
    route: &Route,
) -> std::result::Result<(), Unsupported> {
    let series = v.series();
    let refs: Vec<_> = series
        .iter()
        .filter(|s| REFERENCE_ROLES.contains(&s.role.as_str()))
        .collect();
    let drawn: Vec<_> = series
        .iter()
        .filter(|s| !REFERENCE_ROLES.contains(&s.role.as_str()))
        .collect();
    if drawn.is_empty() {
        return Err(Unsupported(
            "reference series need the series they refer to (a bar or dot series)".into(),
        ));
    }
    if t == "dot" {
        decide(
            report,
            "6.7",
            format!(
                "a dot plot of {} series, the dots of a category sharing its position, each series in its own colour, every dot of one size (G7.18)",
                drawn.len()
            ),
            Some("locked"),
        );
        return Ok(());
    }
    if drawn.len() == 1 {
        let refs = refs
            .iter()
            .map(|r| r.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        decide(
            report,
            "R11-3",
            format!(
                "{} as bars, {refs} as a flat marker across each bar: the bars feature the individual categories or periods, the reference stays in the background (G1.60, D-21)",
                drawn[0].name
            ),
            Some("guided"),
        );
        return Ok(());
    }
    let explicit = v.chart_type() != "auto";
    if let Some(parts) = v.parts()
        && v.x_kind() != "time"
    {
        if parts["all_parts"] == false {
            let why = "not every part of the totals is shown: clustered bars may suggest that the parts shown are all the parts (G1.42)";
            if explicit {
                guided(
                    report,
                    v,
                    "R7-22",
                    "chart.type",
                    "bar (clustered)",
                    "merged bars",
                    why,
                );
            } else {
                return Err(Unsupported(format!(
                    "merged bars (R7-22: {why}) are drawn from the totals as data.dims, a panel per part (SPEC §6.10, as P2-67), which the chooser does not reshape; clustered bars are acceptable with chart.type bar and the reason"
                )));
            }
            return Ok(());
        }
        if route.type_.as_deref() != Some("bar") {
            let rt = route.type_.as_deref().unwrap_or("");
            decide(
                report,
                "R9-11",
                format!(
                    "clustered bars, as given (the procedure gives {})",
                    type_words(rt).unwrap_or("None")
                ),
                Some("guided"),
            );
            return Ok(());
        }
        let (rule, why) = match &route.form {
            Some((_, rule, why)) => (rule.clone(), why.clone()),
            None => (
                "R7-19".into(),
                "parts compared across totals: only clustered bars compare them precisely (G1.41)"
                    .into(),
            ),
        };
        decide(
            report,
            &rule,
            format!("clustered bars: {why}"),
            Some("guided"),
        );
        return Ok(());
    }
    decide(
        report,
        "R9-11",
        format!(
            "clustered bars: {} series of one measure across {} (G1.56, R9-12)",
            drawn.len(),
            if v.x_kind() == "time" {
                "periods"
            } else {
                "categories"
            }
        ),
        Some("guided"),
    );
    Ok(())
}

/// The outcome of `choose.choose`: Ok(Some(choice)), Ok(None) when a refusal stops the chart, or
/// Err(Unsupported). Records each decision, the path and the type in the report.
pub fn choose(v: &View, report: &mut Report) -> std::result::Result<Option<Choice>, Unsupported> {
    let x_kind = v.x_kind();
    let grid = v.grid();
    let n_x = v.x_values().len();
    let mut stop = false;
    if time_facts(v, report) {
        return Ok(None);
    }
    let route = procedure(v);
    record(report, &route);
    let rec = route.type_.clone().unwrap_or_default();
    let rule = route.rule.clone().unwrap_or_default();
    let why = route.why.clone().unwrap_or_default();
    let mut t = v.chart_type().to_owned();
    if avoided(&t).is_some() {
        match avoid(v, &t, report)? {
            Some(kept) => t = kept,
            None => return Ok(None),
        }
    } else if t == "auto" {
        t = rec.clone();
        decide(report, &rule, format!("type {rec}: {why}"), Some("guided"));
    } else if t == SLOPE && rec != SLOPE {
        let text = if n_x != 2 || x_kind != "time" {
            "a slope chart joins the values of two periods, and these data have not two".to_owned()
        } else if v.message() == Some("rank") {
            format!(
                "a change of rank is the slope chart's case, but {} categories are more than its labels hold at the right (at most {SLOPE_MAX}, 06 DP6-3)",
                v.drawn().len()
            )
        } else {
            "a slope chart shows the change only by its steepness, so large relative changes among small values look small (the book's rare exception is a change of rank, facts.message rank)".to_owned()
        };
        if refusal(
            report,
            v,
            "R6-16",
            text,
            Some("chart.type"),
            Some("type auto (an arrow chart: change in units and in %)"),
        ) {
            return Ok(None);
        }
    } else if PART_TYPES.contains(&t.as_str())
        && checks(v)
            .iter()
            .any(|c| c.state == State::Fail && !(c.name == "signs" && t == WATERFALL))
    {
        // a check refuses it (below): the departure is that refusal
    } else {
        guided(report, v, &rule, "chart.type", &t, &rec, &why);
    }
    report.chart_type = Some(t.clone());
    if t == rec {
        for (w_rule, w_path, w_text) in &route.warns {
            let reason = v.reason("chart.type");
            let mut f = finding(
                w_rule,
                if reason.is_some() { "info" } else { "warn" },
                w_text.clone(),
                Some(w_path),
                Some("guided"),
            );
            f.reason = reason;
            report.findings.push(f);
        }
    }
    if t == "aggregate" {
        refusal(
            report,
            v,
            "R6-3",
            "a line needs a regular series, one value per interval: these events must first be aggregated into regular intervals, a change of data the generator does not make",
            Some("data"),
            Some(
                "the events totalled (or averaged) per day, week or month on a regular grid (R6-10)",
            ),
        );
        return Ok(None);
    }
    if PART_TYPES.contains(&t.as_str()) && part_refusals(v, &t, report) {
        return Ok(None);
    }
    if periods_gate(v, &route, &t, report) {
        return Ok(None);
    }
    if (rec == "arrow" || rec == SLOPE)
        && t == "bar"
        && refusal(
            report,
            v,
            "R6-17",
            "two periods drawn as bars show the values side by side, with nothing to suggest change and no change in % (the book's most common version, R6-14)",
            Some("chart.type"),
            Some("type auto (an arrow chart: change in units and change in %)"),
        )
    {
        return Ok(None);
    }
    if t == "arrow" && (x_kind != "time" || grid == Some("none") || n_x != 2) {
        let what = if x_kind == "time" && grid != Some("none") {
            "periods"
        } else if x_kind == "time" {
            "irregular times"
        } else {
            "categories along x"
        };
        if refusal(
            report,
            v,
            "R6-15",
            format!(
                "an arrow chart joins each category's values in two periods, before and after, and these data have {n_x} {what}{}",
                if what == "periods" {
                    ": its arrows would join only the first two"
                } else {
                    ""
                }
            ),
            Some("chart.type"),
            Some(
                "type auto, or x the two periods (kind time, two values) with a series per category, the arrows' rows",
            ),
        ) {
            return Ok(None);
        }
        return Err(Unsupported(format!(
            "an arrow chart of {n_x} {what} is not drawn: it joins two periods"
        )));
    }
    if (t == "line" || t == LINE_TOTAL) && line_refusals(v, report) {
        return Ok(None);
    }
    if t == "map" || t == "map_lines" {
        return Err(Unsupported(format!(
            "{} are phase 4 (08-12 DP-1, {}); the path says why",
            type_words(&t).unwrap(),
            if t == "map_lines" { "G1.26" } else { "G1.25" }
        )));
    }
    if !drawable(&t) {
        return Err(Unsupported(format!(
            "{} are drawn by a later stage of the generator",
            words_of(&t)
        )));
    }
    let n_series = v.series().len();
    if n_series > 1 && (t == "step" || t == "thin_bar") {
        return Err(Unsupported(format!(
            "{} charts of several series are a later stage",
            if t == "step" { "step" } else { "thin bar" }
        )));
    }
    if n_series > 1 && (t == "bar" || t == "dot") {
        several_series(v, &t, report, &route)?;
    }
    let drawn_as = if t == SLOPE {
        "line".to_owned()
    } else {
        t.clone()
    };

    let mut orientation = v.str_or("/chart/orientation", "auto").to_owned();
    if x_kind == "time" && orientation == "horizontal" {
        stop |= refusal(
            report,
            v,
            "R2-37",
            "a time axis runs left to right; a chart over time is never turned on its side (R6-38)",
            Some("chart.orientation"),
            Some("orientation vertical or auto"),
        );
    }
    if drawn_as == "line" && orientation == "horizontal" {
        stop |= refusal(
            report,
            v,
            "R6-38",
            "a line chart is never turned on its side: most readers cannot read a pattern of change running down the page",
            Some("chart.orientation"),
            Some("orientation auto or vertical"),
        );
    }
    if x_kind == "time" {
        orientation = "vertical".into();
    }

    let mut sort = v.str_or("/chart/sort", "auto").to_owned();
    let mut recommended_sort = if x_kind == "nominal" && given_order(&t).is_none() {
        "desc"
    } else {
        "given"
    };
    let fixed = v.str_or("/data/x/order", "auto") == "fixed"
        || (v
            .parts()
            .is_some_and(|p| p["order"].as_str().unwrap_or("auto") == "fixed")
            && v.drawn().len() == 1);
    if x_kind == "nominal" && fixed {
        recommended_sort = "given";
        if sort == "auto" {
            decide(
                report,
                "02 D1",
                "categories in their given order: the data's order is fixed (data.x.order or data.parts.order: fixed; owner, 1 Oct 2026)",
                Some("guided"),
            );
        }
    }
    if x_kind != "nominal" && (sort == "desc" || sort == "asc") {
        let what = if x_kind == "time" {
            "periods"
        } else {
            "ordinal categories"
        };
        stop |= refusal(
            report,
            v,
            "R2-02",
            format!("{what} keep their built-in order and are never sorted by value"),
            Some("chart.sort"),
            Some("sort given or auto"),
        );
        sort = "given".into();
    } else if sort == "auto" {
        sort = recommended_sort.into();
    } else if x_kind == "nominal" {
        let why = if recommended_sort == "desc" {
            "nominal categories by value, largest first"
        } else if fixed {
            "the data's order is fixed"
        } else {
            given_order(&t).unwrap_or("the order given")
        };
        guided(
            report,
            v,
            "R2-01",
            "chart.sort",
            &sort,
            recommended_sort,
            why,
        );
    }
    let sort_by = match v.opt_str("/chart/sort_by") {
        Some(s) => s.to_owned(),
        None => {
            let targets: Vec<String> = v
                .get("/insight/targets")
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|r| r["series"].as_str().filter(|s| !s.is_empty()))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let all = v.series();
            let first = all
                .iter()
                .find(|s| !REFERENCE_ROLES.contains(&s.role.as_str()))
                .or(all.first())
                .map(|s| s.id.clone())
                .unwrap_or_default();
            targets.into_iter().next().unwrap_or(first)
        }
    };
    if v.str_or("/chart/labelling", "auto") == "legend" && n_series == 1 {
        guided(
            report,
            v,
            "R4-1",
            "chart.labelling",
            "legend",
            "none (one series)",
            "one series needs no legend: the axis labels and title carry the meaning (G5.6)",
        );
    }
    if stop {
        return Ok(None);
    }
    Ok(Some(Choice {
        type_: drawn_as,
        orientation,
        sort,
        sort_by,
    }))
}
