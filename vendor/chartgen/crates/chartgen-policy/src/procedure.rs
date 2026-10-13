//! The chooser's procedure (`choose.procedure`, GUIDE G1.10's ten steps in D-12 (a)'s order):
//! the group first, then the type within it, each step recorded with its rule and level.
use crate::view::{SeriesRef, View, known, plain};
use chartgen_model::Decimal;
use chartgen_scales::{Mode, lower_limit_mode};
use regex::Regex;
use serde_json::{Value, json};
use std::sync::OnceLock;

pub(crate) const STEPS: [&str; 10] = [
    "brief",
    "table or graph",
    "variables",
    "location",
    "parts of a total",
    "two periods",
    "values over time",
    "values across categories",
    "combos",
    "avoid-list",
];

pub(crate) const PARTS_ONE: [&str; 5] = [
    "compare_parts_precisely",
    "fraction_of_total",
    "subtotal_amount",
    "subtotal_fraction",
    "cumulative_subtotals",
];
pub(crate) const PARTS_SEVERAL: [&str; 3] = [
    "compare_parts_across_totals",
    "compare_totals",
    "fractions_within_each_total",
];
const FRACTIONS: [&str; 4] = [
    "fraction_of_total",
    "subtotal_amount",
    "subtotal_fraction",
    "cumulative_subtotals",
];
pub(crate) fn table_message(msg: &str) -> Option<&'static str> {
    Some(match msg {
        "lookup_only" => "its sole use is looking up individual values",
        "export_only" => "its sole use is importing the values into another program",
        "not_comparable" => {
            "none of its values are comparable with one another (key metrics in different units), \
             the numbers left-aligned (G7.41)"
        }
        _ => return None,
    })
}
const ACTION_MESSAGE: &str = "action_needed";
const FUNNEL_MESSAGE: &str = "stage_conversion";
const PROFILE_MESSAGE: &str = "profiles";
const MAX_PARTS: usize = 7;
const WRAP_BARS: usize = 40;
const HEATMAP_SERIES: usize = 30;
const MANY_LINES: usize = 5;
pub(crate) const SLOPE_MAX: usize = 10;
const UNFAMILIAR: [&str; 9] = [
    "dot",
    "step",
    "thin_bar",
    "bumps",
    "arrow",
    "treemap",
    "stacked_area",
    "stacked_area_100",
    "slope",
];

pub(crate) const TWO_PERIODS_DEFAULT: &str = "the default is an arrow chart, each category's change in units and in % side by side (G1.47), or a chart of the change itself (the % panel alone, facts.message relative_change; or the change as bars, a previous_period comparison with transform deviation); clustered bars of the two periods only where each period's own values, not the change, are what the request needs: facts.message periods, with the reason, grounded in the request, in reasons[\"facts.message\"]";

/// `choose.TYPE_WORDS`
pub(crate) fn type_words(t: &str) -> Option<&'static str> {
    Some(match t {
        "bar" => "bar charts",
        "dot" => "dot plots",
        "line" => "line charts",
        "thin_bar" => "thin bar charts",
        "pie" => "pies",
        "stacked_bar" => "stacked bar charts",
        "stacked_bar_100" => "100 % stacked bar charts",
        "stacked_area" => "stacked area charts",
        "stacked_area_100" => "100 % stacked area charts",
        "line_total" => "line charts with a total panel",
        "waterfall" => "waterfall charts",
        "treemap" => "treemaps",
        "wrapped_bar" => "wrapped bar charts",
        "table" => "tables",
        "heatmap" => "heatmaps",
        "action_dots" => "action dots",
        "funnel_bars" => "stage bars with the % change between stages (the funnel's replacement)",
        "slope" => "slope charts",
        "map" => "maps",
        "map_lines" => "lines on a map",
        _ => return None,
    })
}
/// `TYPE_WORDS.get(t, f"{t.replace('_', ' ')} charts")`
pub(crate) fn words_of(t: &str) -> String {
    type_words(t)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{} charts", t.replace('_', " ")))
}

/// The procedure's answer (G1.10): the type with its rule and reason, the group (D-13), and the
/// path of steps that led there.
#[derive(Debug, Clone, Default)]
pub struct Route {
    pub type_: Option<String>,
    pub rule: Option<String>,
    pub why: Option<String>,
    pub group: Option<String>,
    pub steps: Vec<Value>,
    /// (rule, path, message): defaults standing in for facts (D-1)
    pub infos: Vec<(String, String, String)>,
    /// (rule, path, message): guided flags (7 parts, D-23 (a))
    pub warns: Vec<(String, String, String)>,
    /// several totals as bars: (form, rule, why)
    pub form: Option<(String, String, String)>,
    /// (rule, why) of a bar chart that step 4 gives, for step 7
    pub lead: Option<(String, String)>,
    /// two series that are two periods, given across the categories (step 5)
    pub across: Option<Vec<String>>,
    /// two periods with facts.message periods: (reason or None, [the periods])
    pub periods_gate: Option<(Option<String>, Vec<String>)>,
}

impl Route {
    pub(crate) fn step(&mut self, n: usize, rule: &str, text: impl Into<String>, level: &str) {
        self.steps.push(
            json!({"step": n, "name": STEPS[n], "rule": rule, "level": level,
                               "text": text.into()}),
        );
    }
    pub(crate) fn guided(&mut self, n: usize, rule: &str, text: impl Into<String>) {
        self.step(n, rule, text, "guided")
    }
    fn enter(&mut self, n: usize, group: &str, rule: &str, text: &str) {
        let moved = match &self.group {
            Some(g) if g != group => {
                format!(", in place of {g}, whose types do not include it (RP2-8)")
            }
            _ => String::new(),
        };
        self.group = Some(group.into());
        let mut chars = text.chars();
        let capital = chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default();
        self.guided(n, rule, format!("group: {group}{moved}. {capital}"));
    }
    pub(crate) fn done(&mut self, t: &str, rule: &str, why: impl Into<String>) {
        self.type_ = Some(t.into());
        self.rule = Some(rule.into());
        self.why = Some(why.into());
    }
}

/// 06 DP6-2, the readers' test (D-27): every series on a regular grid has at least half its slots
/// known and a run of three known in a row. Returns (ok, why).
pub(crate) fn enough_values(v: &View) -> (bool, String) {
    for s in v.series() {
        let vals = s.values.unwrap_or(&[]);
        let mut run = 0;
        let mut best = 0;
        for d in vals {
            run = if known(d) { run + 1 } else { 0 };
            best = best.max(run);
        }
        let n = vals.iter().filter(|d| known(d)).count();
        if !vals.is_empty() && ((n as f64) < 0.5 * vals.len() as f64 || best < 3) {
            return (
                false,
                format!(
                    "{n} of {} {}s known, at most {best} in a row",
                    vals.len(),
                    v.grid().unwrap_or("None")
                ),
            );
        }
    }
    (true, String::new())
}

/// The Route of a spec.
pub(crate) fn procedure(v: &View) -> Route {
    let mut r = Route::default();
    let drawn = v.drawn();
    brief(v, &mut r);
    if crate::statistical::route(v, &mut r) {
        return r;
    }
    if !(table_or_graph(v, &mut r)
        || variables(v, &mut r, &drawn)
        || location(v, &mut r)
        || parts(v, &mut r, &drawn))
    {
        if v.x_kind() == "time" {
            if !two_periods(v, &mut r, &drawn) {
                over_time(v, &mut r);
            }
        } else if !two_periods_across(v, &mut r, &drawn) {
            categories(v, &mut r, &drawn);
        }
    }
    combos(v, &mut r, &drawn);
    avoid_list(v, &mut r);
    r
}

fn brief(v: &View, r: &mut Route) {
    let job = v.job();
    let insight = v.insight_important();
    if v.purpose() == "analysis" {
        r.step(
            0,
            "R0-02",
            "purpose analysis: the maker's own exploration, refusals turned into warnings (G0.1)",
            "open",
        );
        return;
    }
    let Some(job) = job else {
        r.step(
            0,
            "R0-07",
            "no job: a communication chart states it (G0.10)",
            "open",
        );
        return;
    };
    let tail = match insight {
        Some(i) => format!("insight “{i}”"),
        None => format!(
            "no insight stated{}",
            if ["lookup", "explain", "other"].contains(&job) {
                " (a descriptive title will do, D-55)"
            } else {
                ""
            }
        ),
    };
    r.step(0, "R0-07", format!("job {job}: {tail}"), "open");
}

fn table_or_graph(v: &View, r: &mut Route) -> bool {
    if let Some(msg) = v.message()
        && let Some(what) = table_message(msg)
    {
        let why = format!("a table alone: {what} (facts.message {msg}; G1.12)");
        r.enter(1, "tables", "R10-11", &why);
        r.done("table", "R10-11", why);
        return true;
    }
    let mut text = String::from("a graph, the values to be compared or a pattern seen (G1.11)");
    if v.flag("/facts/exact_values_needed") {
        text += "; exact values are needed, so a table aligned with it (R10-7, G1.14), drawn by the layout";
    }
    r.guided(1, "R10-1", text);
    false
}

fn variables(v: &View, r: &mut Route, drawn: &[SeriesRef]) -> bool {
    let n = drawn.len();
    let time = v.x_kind() == "time";
    if n > HEATMAP_SERIES {
        let why = format!(
            "{n} series, more than a stack of small charts holds (about 20–30): a heatmap, a row per series (G1.20, G7.35; D-28)"
        );
        r.enter(
            2,
            if time { "data over time" } else { "tables" },
            "R6-36",
            &why,
        );
        r.done("heatmap", "R6-36", why);
        return true;
    }
    let mut text = format!(
        "one plot: {} along x and {n} series",
        if time { "time" } else { "categories" }
    );
    if time && n > MANY_LINES {
        text += &format!(
            "; more than {MANY_LINES} lines read well only if they rarely cross, else a stack of small line charts (panels by data.dims, G1.20)"
        );
    }
    r.guided(
        2,
        "RP3-2",
        text + "; a further variable would go to small multiples (data.dims, G1.18)",
    );
    false
}

fn location(v: &View, r: &mut Route) -> bool {
    if !v.flag("/facts/location_matters") {
        r.guided(
            3,
            "R8-2",
            "not a map: where the values are is not the message (facts.location_matters; G1.24, G1.25)",
        );
        return false;
    }
    if v.x_kind() == "time" {
        let why = "where the values are matters more than reading them, and they change over time: a small line chart on each region, or one line chart of a line per region when comparing the patterns matters more (G1.26)";
        r.enter(3, "maps changing over time", "R8-13", why);
        r.done("map_lines", "R8-13", why);
    } else {
        let why = "where the values are (the regions' sizes, neighbours, clusters) matters more than reading and comparing them: a map, coloured dots, bubbles or bars on regions (a choropleth only for regions of similar size, G1.27–G1.33, D-36), beside a bar chart when the values must be read too (G1.25)";
        r.enter(3, "maps", "R8-2", why);
        r.done("map", "R8-2", why);
    }
    true
}

// -- step 4: parts of a total ------------------------------------------------------------------

pub(crate) fn is_breakdown(v: &View) -> bool {
    v.parts().is_some()
        || v.message()
            .is_some_and(|m| PARTS_ONE.contains(&m) || PARTS_SEVERAL.contains(&m))
}

/// The parts of each total, as lists of known values (for the sign check, D-25 (a)).
pub(crate) fn part_groups(v: &View) -> Vec<Vec<Decimal>> {
    let x = v.x_values();
    let drawn = v.drawn();
    let skip = |c: &str| matches!(v.x_role(c), Some("total" | "average"));
    if v.x_kind() != "time" && drawn.len() == 1 {
        let vals = drawn[0].values.unwrap_or(&[]);
        return vec![
            x.iter()
                .zip(vals)
                .filter(|(c, d)| known(d) && !skip(c))
                .filter_map(|(_, d)| d.value)
                .collect(),
        ];
    }
    x.iter()
        .enumerate()
        .filter(|(_, c)| !skip(c))
        .map(|(i, _)| {
            drawn
                .iter()
                .filter_map(|s| s.values.and_then(|vals| vals.get(i)).and_then(|d| d.value))
                .collect()
        })
        .collect()
}

pub(crate) fn part_count(v: &View) -> usize {
    let drawn = v.drawn();
    if v.x_kind() != "time" && drawn.len() == 1 {
        return v
            .x_values()
            .iter()
            .filter(|c| !matches!(v.x_role(c), Some("total" | "average")))
            .count();
    }
    drawn.len()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum State {
    Pass,
    Fail,
    Missing,
}

pub(crate) struct Check {
    pub name: &'static str,
    pub state: State,
    pub rule: &'static str,
    pub text: String,
}

pub(crate) fn overlap_pairs(v: &View) -> Vec<String> {
    v.parts()
        .and_then(|p| p["overlaps"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(|pr| {
                    let pr = pr.as_array()?;
                    (pr.len() == 2).then(|| {
                        format!(
                            "{} within {}",
                            pr[0].as_str().unwrap_or(""),
                            pr[1].as_str().unwrap_or("")
                        )
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The four checks before any pie, stacked chart, treemap or waterfall.
pub(crate) fn checks(v: &View) -> Vec<Check> {
    let p = v.parts();
    let fact = |k: &str| p.and_then(|p| p[k].as_bool());
    let state = |b: Option<bool>| match b {
        Some(true) => State::Pass,
        Some(false) => State::Fail,
        None => State::Missing,
    };
    let mut out = vec![];
    let mt = fact("meaningful_total");
    out.push(Check {
        name: "meaningful total",
        state: state(mt),
        rule: "R7-02",
        text: match mt {
            Some(true) => "the values add up to a total someone would quote (parts.meaningful_total)",
            Some(false) => "the values do not add up to a total anyone would quote (averages, rates, ratios, ratings): no pie, stacked chart or treemap (G0.12)",
            None => "whether the values add up to a total someone would quote is not stated (data.parts.meaningful_total), and the numbers cannot tell (D-1)",
        }
        .into(),
    });
    let ap = fact("all_parts");
    out.push(Check {
        name: "all parts",
        state: state(ap),
        rule: "R7-03",
        text: match ap {
            Some(true) => "every part of the total is shown (parts.all_parts)",
            Some(false) => "not every part is shown (a top N or a filtered subset): readers would take the parts shown for the whole (G0.13)",
            None => "whether every part of the total is shown is not stated (data.parts.all_parts), and the numbers cannot tell (D-1)",
        }
        .into(),
    });
    let pairs = overlap_pairs(v);
    let overlap = fact("overlap");
    if p.is_some() && (overlap == Some(true) || !pairs.is_empty()) {
        out.push(Check {
            name: "overlap",
            state: State::Fail,
            rule: "R7-05",
            text: format!(
                "parts contain or overlap one another{}: separate slices or segments would claim disjoint parts (G0.14)",
                if pairs.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", pairs.join(", "))
                }
            ),
        });
    } else if p.is_some() && overlap == Some(false) {
        out.push(Check {
            name: "overlap",
            state: State::Pass,
            rule: "R7-05",
            text: "no part contains or overlaps another (parts.overlap)".into(),
        });
    } else {
        out.push(Check {
            name: "overlap",
            state: State::Missing,
            rule: "R7-05",
            text: "whether any part contains or overlaps another is not stated (data.parts.overlap, overlaps), and the numbers cannot tell (D-1)".into(),
        });
    }
    let mixed = part_groups(v)
        .iter()
        .any(|g| g.iter().any(|x| *x > Decimal::ZERO) && g.iter().any(|x| *x < Decimal::ZERO));
    out.push(Check {
        name: "signs",
        state: if mixed { State::Fail } else { State::Pass },
        rule: "R7-04",
        text: if mixed {
            "the parts mix positive and negative values: areas cannot be negative, and a mixed stack shows neither the net total nor comparable parts (G0.15)"
        } else {
            "the parts are all of one sign"
        }
        .into(),
    });
    out
}

/// Record the four checks in the path; (the first failure, whether a fact is missing).
fn run_checks(v: &View, r: &mut Route) -> (Option<(&'static str, &'static str, String)>, bool) {
    let mut fail = None;
    let mut missing = false;
    for c in checks(v) {
        let mark = match c.state {
            State::Pass => "passes",
            State::Fail => "fails",
            State::Missing => "not known",
        };
        r.step(
            4,
            c.rule,
            format!("check, {}: {mark}: {}", c.name, c.text),
            "locked",
        );
        if c.state == State::Fail && fail.is_none() {
            fail = Some((c.name, c.rule, c.text.clone()));
        }
        missing |= c.state == State::Missing;
    }
    (fail, missing)
}

fn parts(v: &View, r: &mut Route, drawn: &[SeriesRef]) -> bool {
    if !is_breakdown(v) {
        r.guided(
            4,
            "R7-01",
            "not parts of a total (no data.parts, no breakdown message; G1.1)",
        );
        return false;
    }
    if v.x_kind() == "time" {
        if drawn.len() < 2 {
            r.guided(
                4,
                "R7-01",
                "one series over time is not a breakdown: the time tree (G1.48)",
            );
            return false;
        }
        return over_time_parts(v, r, drawn);
    }
    if drawn.len() > 1 {
        return several_totals(v, r, drawn);
    }
    one_total(v, r)
}

fn one_total(v: &View, r: &mut Route) -> bool {
    let mut msg = v.message().map(str::to_owned);
    let n = part_count(v);
    r.enter(
        4,
        "breakdowns of totals",
        "RP2-7",
        &format!("one total of {n} parts (07 §4.1, G1.8)"),
    );
    if !msg.as_deref().is_some_and(|m| PARTS_ONE.contains(&m)) {
        r.infos.push(("R7-06".into(), "facts.message".into(), "facts.message not one of 07 §4.1's for one total: its parts are compared, in a bar chart (G1.35), which never misrepresents them; fraction_of_total, subtotal_amount, subtotal_fraction or cumulative_subtotals would give a pie, a stacked bar, a treemap or a waterfall".into()));
        msg = Some("compare_parts_precisely".into());
    }
    let msg = msg.unwrap();
    if msg == "compare_parts_precisely" {
        r.guided(4, "R7-06", "parts compared precisely: a regular bar chart, the only one whose parts share a baseline (G1.35); no pie, stacked chart or treemap is in question, so the four checks are not needed");
        if n > WRAP_BARS {
            let why = format!(
                "{n} parts compared: too many bars for one column, a wrapped bar chart on one scale (G1.39, G7.7)"
            );
            r.guided(4, "R7-14", why.clone());
            r.done("wrapped_bar", "R7-14", why);
            return true;
        }
        r.lead = Some(("R7-06".into(), "parts compared precisely: a regular bar chart, the only one whose parts share a baseline (G1.35)".into()));
        return false;
    }
    let (fail, missing) = run_checks(v, r);
    if let Some((name, rule, _)) = fail {
        if name == "signs" && msg == "cumulative_subtotals" {
            let why = "the parts mix positive and negative values and running subtotals are the message: a waterfall, each bar starting where the last ended (G1.38)";
            r.guided(4, "R7-11", why);
            r.done("waterfall", "R7-11", why);
            return true;
        }
        let why = match name {
            "meaningful total" => {
                "the values do not add up to a total anyone would quote: a regular bar chart (G0.12)"
            }
            "all parts" => "not every part of the total is shown: a regular bar chart (G0.13)",
            "overlap" => {
                "the parts overlap: a plain bar chart (the book's tree); better, D-24 (b), make them disjoint in the data (Other cars = Cars − Sports cars) or stack each parent from its sub-parts (G0.14)"
            }
            _ => {
                "the parts mix positive and negative values: a regular bar chart, closed by a grey Total bar when the net total is part of the message (G0.15, G1.38, D-43)"
            }
        };
        r.guided(4, rule, why);
        r.lead = Some((rule.into(), why.into()));
        return false;
    }
    if n > MAX_PARTS {
        let why = format!(
            "{n} parts, more than the 6–7 a pie or stacked bar holds (D-23 (a)), and fractions or subtotals are the message: a treemap (G1.39); or lump the parts unimportant to the message into one All others part in the data (R7-17), and a pie holds"
        );
        r.guided(4, "R7-15", why.clone());
        r.done("treemap", "R7-15", why);
        return true;
    }
    if n == MAX_PARTS {
        r.warns.push(("R7-13".into(), "data.x.values".into(), format!("{n} parts, at the limit of about 6–7 for a pie or stacked bar (D-23 (a): seven flagged): lump the parts unimportant to the message into All others (R7-17) if you can, or give reasons[\"chart.type\"]")));
    }
    if msg == "subtotal_amount" || msg == "cumulative_subtotals" {
        let why = if msg == "subtotal_amount" {
            "a subtotal of several parts as an amount: a stacked bar, the summed parts adjacent at its base (G1.37)"
        } else {
            "running subtotals of parts of one sign: a stacked bar, the parts adding up visually (07 §4.1)"
        };
        r.guided(4, "R7-08", why);
        r.done("stacked_bar", "R7-08", why);
        return true;
    }
    if v.flag("/audience/pie_averse") {
        let why = "fractions of the total are the message, but the audience objects to pies: a 100 % stacked bar (G1.40, D-39)";
        r.guided(4, "R7-40", why);
        r.done("stacked_bar_100", "R7-40", why);
        return true;
    }
    let mut why = String::from(if msg == "fraction_of_total" {
        "fractions of the total are the message: a pie, which shows ½, ⅓ and ¼ at a glance and announces the whole (G1.36)"
    } else {
        "a subtotal stated as a fraction of the total: a pie, its parts adjacent (G1.37)"
    });
    if missing {
        why += "; once the facts above are given";
    }
    r.guided(4, "R7-07", why.clone());
    r.done("pie", "R7-07", why);
    true
}

fn several_totals(v: &View, r: &mut Route, drawn: &[SeriesRef]) -> bool {
    let mut msg = v.message().map(str::to_owned);
    let n = drawn.len();
    let totals = v.x_values().len();
    r.enter(
        4,
        "breakdowns of totals",
        "RP2-7",
        &format!("{totals} totals of {n} parts each, the parts as series (07 §4.2, G1.8)"),
    );
    if v.parts().is_some_and(|p| p["all_parts"] == false) {
        let why = "not every part of the totals is shown: merged bars, one panel per part (the totals as data.dims, SPEC §6.10); clustered bars are acceptable but second best (G1.42)";
        r.guided(4, "R7-22", why);
        r.form = Some(("merged".into(), "R7-22".into(), why.into()));
        return false;
    }
    if !msg.as_deref().is_some_and(|m| PARTS_SEVERAL.contains(&m)) {
        match msg.as_deref() {
            None => r.infos.push(("R7-19".into(), "facts.message".into(), "facts.message not given for several totals: their parts are compared across them, in clustered bars (G1.41); compare_totals or fractions_within_each_total would give stacked bars or pies".into())),
            Some(m) if PARTS_ONE.contains(&m) => r.infos.push(("R7-19".into(), "facts.message".into(), format!("facts.message {m} is one total's (07 §4.1), and these are {totals} totals: their parts compared across them, in clustered bars (G1.41); compare_totals or fractions_within_each_total would give stacked bars or pies"))),
            _ => {}
        }
        msg = Some("compare_parts_across_totals".into());
    }
    let msg = msg.unwrap();
    if msg == "compare_parts_across_totals" {
        let why =
            "parts compared across totals: only clustered bars compare them precisely (G1.41)";
        r.guided(4, "R7-19", why);
        r.form = Some(("clustered".into(), "R7-19".into(), why.into()));
        return false;
    }
    let (fail, _) = run_checks(v, r);
    if let Some((name, rule, _)) = fail {
        let why = format!(
            "a check fails for the totals ({name}): clustered bars, which claim no whole (G1.41, D-25 (a))"
        );
        r.guided(4, rule, why.clone());
        r.form = Some(("clustered".into(), rule.into(), why));
        return false;
    }
    if n > MAX_PARTS {
        let why = format!(
            "{n} parts, more than the 6–7 a stack or a pie holds (D-23 (a), D-25 (a)): clustered bars (G1.41)"
        );
        r.guided(4, "R7-13", why.clone());
        r.form = Some(("clustered".into(), "R7-13".into(), why));
        return false;
    }
    if msg == "compare_totals" {
        let why = "the totals compared, and roughly which parts dominate each: multiple stacked bars (G1.41)";
        r.guided(4, "R7-20", why);
        r.done("stacked_bar", "R7-20", why);
        return true;
    }
    if v.flag("/audience/pie_averse") {
        let why = "the fractions within each total, for an audience that objects to pies: 100 % stacked bars (D-39)";
        r.guided(4, "R7-40", why);
        r.done("stacked_bar_100", "R7-40", why);
        return true;
    }
    let why = "the message is entirely the fractions within each total, with at most a very rough comparison between them: multiple pies, which is rare (G1.41)";
    r.guided(4, "R7-21", why);
    r.done("pie", "R7-21", why);
    true
}

fn over_time_parts(v: &View, r: &mut Route, drawn: &[SeriesRef]) -> bool {
    let msg = v.message();
    let n = drawn.len();
    r.enter(
        4,
        "breakdowns of totals changing over time",
        "RP2-8",
        &format!(
            "a total of {n} parts over {} periods (07 §4.3, R6-13, G1.8)",
            v.x_values().len()
        ),
    );
    let total = match v.opt_bool("/facts/total_matters") {
        Some(t) => t,
        None => {
            r.infos.push(("R7-25".into(), "facts.total_matters".into(), "facts.total_matters not given: the total's change is taken not to be part of the message (07 §4.3); say true for a total panel or absolute stacks".into()));
            false
        }
    };
    let focus = v
        .opt_str("/facts/focus")
        .unwrap_or(if msg == Some("periods") {
            "periods"
        } else {
            "pattern"
        });
    let periods = focus == "periods";
    let mut fractions = msg.is_some_and(|m| FRACTIONS.contains(&m));
    if fractions {
        let (fail, _) = run_checks(v, r);
        if fail.is_some() || n > MAX_PARTS {
            let why = match &fail {
                Some((name, _, _)) => format!("a check fails for the periods ({name})"),
                None => format!("{n} parts, more than the 6–7 a stack holds (D-23 (a))"),
            } + ": no stacked form, a line per part (R7-26, D-25 (a))";
            r.guided(4, fail.as_ref().map_or("R7-13", |f| f.1), why);
            fractions = false;
        }
    }
    if !fractions {
        if total {
            let why = "the parts compared over time and the total matters: a line per part, with the total as a line in its own panel above them, sharing the time axis (G1.43)";
            r.guided(4, "R7-27", why);
            r.done("line_total", "R7-27", why);
            return true;
        }
        r.guided(4, "R7-26", "the parts compared over time, the total not needed: a line per part, or the time tree's variant (G1.43); on to step 6");
        return false;
    }
    let (t, rule, why) = if total {
        (
            if periods {
                "stacked_bar"
            } else {
                "stacked_area"
            },
            "R7-29",
            format!(
                "fractions or rough subtotals over time, and the total's change matters: absolute stacks, {}",
                if periods {
                    "bars, the particular periods being the point (G1.44, G1.45)"
                } else {
                    "areas, the overall pattern of change being the point (G1.44, G1.45)"
                }
            ),
        )
    } else {
        (
            if periods {
                "stacked_bar_100"
            } else {
                "stacked_area_100"
            },
            "R7-30",
            format!(
                "how the fractions of the total change, the total itself not needed: 100 % stacks, {}",
                if periods {
                    "bars, the particular periods being the point (G1.44, G1.45)"
                } else {
                    "areas, the overall pattern of change being the point (G1.44, G1.45)"
                }
            ),
        )
    };
    r.guided(4, rule, why.clone());
    r.done(t, rule, why);
    true
}

// -- steps 5 to 7 ------------------------------------------------------------------------------

fn period_name() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let year = r"(?:1[89]|2[01])\d{2}";
        let months = r"Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|June?|July?|Aug(?:ust)?|Sept?(?:ember)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?";
        let pattern = format!(
            r"(?i)^(?:(?:FY ?)?{year}(?:[–/-](?:\d{{2}}|{year}))?|{year}[- ]?Q[1-4]|Q[1-4][ -]{year}|{year}-(?:0[1-9]|1[0-2])|(?:{months})\.? {year})$"
        );
        Regex::new(&pattern).expect("period pattern")
    })
}

/// The names of two drawn series that are two periods, given across categories (x not time).
pub(crate) fn period_series(v: &View, drawn: &[SeriesRef]) -> Option<Vec<String>> {
    if v.x_kind() == "time" || drawn.len() != 2 {
        return None;
    }
    let names: Vec<String> = drawn.iter().map(|s| s.name.trim().to_owned()).collect();
    names
        .iter()
        .all(|n| period_name().is_match(n))
        .then_some(names)
}

fn two_periods_across(v: &View, r: &mut Route, drawn: &[SeriesRef]) -> bool {
    let Some(names) = period_series(v, drawn) else {
        return false;
    };
    let msg = v.message();
    let reason = v.reason("facts.message");
    if msg == Some("periods")
        && let Some(reason) = reason
    {
        r.guided(5, "R6-15", format!("the two series are two periods ({}, {}), but each period's own values are the message (facts.message periods), for the reason given: “{reason}” (G1.51's exception to G1.46): on to clustered bars", names[0], names[1]));
        r.periods_gate = Some((Some(reason), names));
        return false;
    }
    let why = format!(
        "the two series are two periods ({}, {}) of each category: the change between them is the point (06 DP6-3, G1.46): {TWO_PERIODS_DEFAULT}",
        names[0], names[1]
    );
    r.enter(5, "change between two periods", "R6-15", &why);
    r.done("arrow", "R6-15", why);
    r.across = Some(names.clone());
    if msg == Some("periods") {
        r.periods_gate = Some((None, names));
    }
    true
}

fn two_periods(v: &View, r: &mut Route, drawn: &[SeriesRef]) -> bool {
    let x = v.x_values();
    let msg = v.message();
    if x.len() != 2 || v.grid() == Some("none") {
        return false;
    }
    if msg == Some("periods") {
        let reason = v.reason("facts.message");
        r.periods_gate = Some((reason.clone(), x.clone()));
        if let Some(reason) = reason {
            r.guided(5, "R6-15", format!("two periods ({}, {}), but each period's own values are the message (facts.message periods), for the reason given: “{reason}” (G1.51's exception to G1.46): on to the time tree", x[0], x[1]));
            return false;
        }
    }
    if msg == Some("rank") && drawn.len() <= SLOPE_MAX {
        let why = "changes of rank between two periods, the rare case for a slope chart: a line chart with two points in time, the lines crossing where one category overtakes another, each labelled at the right (06 DP6-3, G1.46, G7.23)";
        r.enter(5, "change between two periods", "R6-16", why);
        r.done("slope", "R6-16", why);
        return true;
    }
    let why = format!(
        "two periods: the change between them is the point (06 DP6-3, G1.46): {TWO_PERIODS_DEFAULT}"
    );
    let note = if msg == Some("rank") {
        format!(
            "; a rank message, but {} categories, more than a slope chart's labels hold (06 DP6-3)",
            drawn.len()
        )
    } else {
        String::new()
    };
    r.enter(
        5,
        "change between two periods",
        "R6-15",
        &format!("{why}{note}"),
    );
    r.done("arrow", "R6-15", why);
    true
}

/// (type, rule, why) for an irregular series (x.grid none; 06 DP6-1, G1.53, G1.54).
pub(crate) fn irregular_type(v: &View) -> (&'static str, &'static str, &'static str) {
    let p = v.measure_str("persistence");
    let msg = v.message();
    match p {
        Some("state") => (
            "step",
            "R6-9",
            "an irregular series of states, each holding until the next: a step chart, never a standard line (G1.53)",
        ),
        Some("event" | "summary") => {
            if msg == Some("individual_values") {
                (
                    "thin_bar",
                    "R6-12",
                    "individual irregular events: thin bars at their instants on a true time axis, never a standard line (G1.54)",
                )
            } else {
                (
                    "aggregate",
                    "R6-10",
                    "irregular events whose overall pattern is the message: aggregated into regular intervals (weekly totals), then a line (G1.54)",
                )
            }
        }
        _ => (
            "step or thin bar",
            "R6-1",
            "an irregular series: a step chart for states, thin bars for events (G0.17)",
        ),
    }
}

fn over_time(v: &View, r: &mut Route) {
    if r.group.is_none() {
        r.group = Some("data over time".into());
        r.guided(
            6,
            "R6-13",
            "group: data over time. A series of more than two periods, not a breakdown and not tied to places (G1.48)",
        );
    }
    let persistence = v.measure_str("persistence");
    if v.grid() == Some("none") {
        let (t, rule, why) = irregular_type(v);
        r.guided(
            6,
            rule,
            format!(
                "an irregular series ({}): {why}",
                persistence.unwrap_or("states or events not said")
            ),
        );
        r.done(t, rule, why);
        return;
    }
    let (ok, sparse) = enough_values(v);
    if !ok && persistence != Some("state") {
        let why = format!(
            "too few known values to see the pattern of change ({sparse}; 06 DP6-2, D-27): a line would invent one; dots show what is known"
        );
        r.guided(6, "R6-2", why.clone());
        r.done("dot", "R6-2", why);
        return;
    }
    r.step(
        6,
        "R6-3",
        "a regular series, enough values known (06 DP6-2)",
        "locked",
    );
    let (t, rule, why) = match v.message() {
        Some("rank") => (
            "bumps",
            "R6-7",
            "changes in rank are the point (06 DP6-1): a bumps chart",
        ),
        Some("periods") => ("bar", "R6-5", "the periods are the point"),
        _ => (
            "line",
            "R6-4",
            "a regular series whose pattern of change is the point (06 DP6-1, G1.51)",
        ),
    };
    r.guided(6, rule, why);
    r.done(t, rule, why);
}

fn categories(v: &View, r: &mut Route, drawn: &[SeriesRef]) {
    let msg = v.message();
    let x_kind = v.x_kind();
    if msg == Some(PROFILE_MESSAGE) && drawn.len() > 1 && r.group.is_none() {
        let why = "items compared across several attributes, which is best and each one's strengths and weaknesses being the message (a radar's data): a heatmap table, the values printed in coloured cells (G7.36)";
        r.enter(7, "tables", "R12-6", why);
        r.done("heatmap", "R12-6", why);
        return;
    }
    if r.group.is_none() {
        r.enter(
            7,
            "bar charts",
            "R9-1",
            "values compared across categories (G1.9)",
        );
    } else {
        r.guided(7, "R9-1", "on to the bar chart of step 7 (G1.9)");
    }
    if msg == Some(ACTION_MESSAGE) && drawn.len() == 1 {
        let why = "which items need action is the message: action dots, a sorted list with thin bars and a coloured dot only where action is needed, its status given with the data (G7.58, D-1)";
        r.guided(7, "R12-4", why);
        r.done("action_dots", "R12-4", why);
        return;
    }
    if msg == Some(FUNNEL_MESSAGE) && drawn.len() == 1 {
        let why = "counts through ordered stages, the losses between them being the message: the stage counts as bars and beneath them the % change from each stage to the next (G7.59)";
        r.guided(7, "R12-5", why);
        r.done("funnel_bars", "R12-5", why);
        return;
    }
    if x_kind == "ordinal" && msg == Some("pattern") && r.lead.is_none() {
        if v.flag("/audience/data_savvy") || v.purpose() == "analysis" {
            let why = "the pattern across ordered categories is the message, for a data-savvy audience: a line, the categories' order giving it its sense (08-12 DP-2, G1.58)";
            r.guided(7, "R6-41", why);
            r.done("line", "R6-41", why);
            return;
        }
        let why = "the pattern across ordered categories is the message, but many readers take any line for change over time, so a line is only for a data-savvy audience (audience.data_savvy; 08-12 DP-2, G1.58): bars, in the categories' order";
        r.guided(7, "R6-41", why);
        r.lead = Some(("R6-41".into(), why.into()));
    }
    let n_x = v.x_values().len();
    if drawn.len() == 1 && x_kind == "nominal" && n_x > WRAP_BARS {
        let why = format!(
            "{n_x} categories, too many bars for one column: a wrapped bar chart, one sorted list in columns on one scale (G7.7)"
        );
        r.guided(7, "R7-14", why.clone());
        r.done("wrapped_bar", "R7-14", why);
        return;
    }
    let known_values = v.known();
    if !known_values.is_empty() {
        let lo = *known_values.iter().min().unwrap();
        let hi = *known_values.iter().max().unwrap();
        let mz = v.measure_str("zero");
        let min = v.measure_dec("min");
        let (mode, _, _) = lower_limit_mode(
            lo,
            hi,
            "dot",
            "auto",
            mz.unwrap_or("meaningful"),
            min,
            v.flag("/facts/distance_to_zero_irrelevant"),
        );
        if matches!(mode, Mode::Theoretical | Mode::Truncated) {
            let why = if mode == Mode::Theoretical {
                format!(
                    "the theoretical minimum {} lies just below the data",
                    plain(min.unwrap_or_default())
                )
            } else if mz == Some("arbitrary") {
                "zero is arbitrary here, not “none of it” (G2.10)".into()
            } else {
                "the distance to zero is declared irrelevant (G2.8)".into()
            };
            let why = format!(
                "a scale that should not start at zero wants dots, not bars (G1.57, D-30): {why}"
            );
            r.guided(7, "R2-24", why.clone());
            r.done("dot", "R2-24", why);
            return;
        }
    }
    if drawn.len() > 1 {
        let form = r
            .form
            .as_ref()
            .map_or("clustered".to_owned(), |f| f.0.clone());
        r.guided(
            7,
            "R9-11",
            format!(
                "{} series of one measure: {form} bars (G1.56){}",
                drawn.len(),
                if r.form.is_some() {
                    ""
                } else {
                    "; several measures would be a merged chart (data.measures), never a dual axis"
                }
            ),
        );
    }
    if let Some((rule, why)) = r.lead.clone() {
        r.done("bar", &rule, why);
        return;
    }
    r.guided(
        7,
        "02 D1",
        if drawn.len() == 1 {
            "one scale from zero: bars, sorted by value when the categories are nominal (02 D1)"
        } else {
            "bars from zero (02 D1)"
        },
    );
    r.done("bar", "02 D1", "categories compared by value");
}

// -- steps 8 and 9 -----------------------------------------------------------------------------

fn combos(v: &View, r: &mut Route, drawn: &[SeriesRef]) {
    let all = v.series();
    let refs: Vec<_> = all
        .iter()
        .filter(|s| crate::view::REFERENCE_ROLES.contains(&s.role.as_str()))
        .collect();
    let grey: Vec<_> = all
        .iter()
        .filter(|s| s.role == "average" || s.role == "total")
        .collect();
    let names = |ss: &[&SeriesRef]| {
        ss.iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !refs.is_empty() {
        r.guided(8, "R11-3", format!("{}: a reference, drawn as another mark, a flat marker across each bar or a line, on the same scale (G1.60)", names(&refs)));
    } else if !grey.is_empty() && v.x_kind() == "time" {
        r.step(
            8,
            "R6-34",
            format!(
                "{}: a reference series in the reserved dark grey, the same mark (G4.15, G7.16)",
                names(&grey)
            ),
            "locked",
        );
    } else {
        r.step(
            8,
            "R11-2",
            format!(
                "one mark type{}",
                if drawn.len() > 1 {
                    ", the series told apart by colour (G1.59)"
                } else {
                    " (G1.59)"
                }
            ),
            "locked",
        );
    }
}

fn avoid_list(v: &View, r: &mut Route) {
    let t = r.type_.clone().unwrap_or_default();
    if type_words(&t).is_none() && (t == "aggregate" || t == "step or thin bar") {
        return;
    }
    let words = words_of(&t);
    r.step(
        9,
        "R12-1",
        format!("{words} are not on the avoid-list (G1.61)"),
        "locked",
    );
    if UNFAMILIAR.contains(&t.as_str())
        && !v.flag("/audience/data_savvy")
        && v.purpose() != "analysis"
    {
        r.guided(9, "R6-18", format!("{words} may be new to a general audience: teach them (build the chart up, show first the familiar chart that fails, or add two or three callouts stating the obvious; G1.5), or state the insight in a sentence and keep the chart as backup (G1.6)"));
    }
}

/// `layout/bumps.ranks`: each series' rank in each period, and the number of ranks N.
pub(crate) fn ranks(v: &View) -> (Vec<Vec<Option<i64>>>, i64) {
    let series = v.series();
    let n = v.x_values().len();
    let m = v.measure();
    let mut out = vec![vec![None; n]; series.len()];
    if m["kind"].as_str() == Some("rank") {
        for (k, s) in series.iter().enumerate() {
            for (j, d) in s.values.unwrap_or(&[]).iter().enumerate().take(n) {
                out[k][j] = d.value.map(|x| x.trunc().to_string().parse().unwrap_or(0));
            }
        }
    } else {
        let low = m["good"].as_str() == Some("lower");
        for j in 0..n {
            let known: Vec<(usize, Decimal)> = series
                .iter()
                .enumerate()
                .filter_map(|(k, s)| {
                    s.values
                        .and_then(|v| v.get(j))
                        .and_then(|d| d.value)
                        .map(|x| (k, x))
                })
                .collect();
            for &(k, x) in &known {
                let better = known
                    .iter()
                    .filter(|(_, w)| if low { *w < x } else { *w > x })
                    .count() as i64;
                out[k][j] = Some(better + 1);
            }
        }
    }
    let top = out
        .iter()
        .flatten()
        .flatten()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    (out, top)
}
