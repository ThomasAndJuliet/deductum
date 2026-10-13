//! Semantic chart choice, refusal and ordering, ported from the original `choose.py`, the
//! semantic checks of `__init__.py`, the report side of `transform.py` and the rule registry of
//! `rules.py`. Decisions never depend on fonts or geometry.
//!
//! The report the policy writes follows the original's `Report` field by field: findings (rule,
//! severity, level, path), decisions (rule, level, text), overrides, the chooser's path (each step's
//! number, name, rule, level, text, guide ids and group) and the report's type.
mod arrangement;
mod choose;
mod marks;
mod procedure;
mod rebin;
mod rules;
mod scale;
mod semantic;
mod statistical;
mod transform;
pub(crate) mod view;

pub use chartgen_scales::{Explained, LENGTH_ENCODED, Mode, lower_limit_mode};
pub use choose::{Choice, Unsupported};
pub use procedure::Route;
pub use rebin::Rebin;
pub use rules::{guide_ids, level_of};
pub use view::text as spec_text;

use chartgen_model::{
    ChartType, DataFacts, Decision, Orientation, PreparedData, Report, Result, Spec,
};
use serde_json::Value;
use view::View;

/// R2-43: unequal time bins re-aggregated or refused (`timeaxis.rebin`). The caller parses the
/// returned spec; the decision or refusal is recorded by [`evaluate`] at the original's place.
pub fn rebin(spec: &Spec) -> Rebin {
    rebin::rebin(spec)
}

/// What the policy stages leave for the rest of the pipeline.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// The chart is drawn with this choice and typed decision.
    Drawn(Choice, Decision),
    /// Several panels (small multiples or a merged chart): the panel pipeline takes over after the
    /// policy's checks that precede it.
    Panels,
    /// Refused, unsupported or invalid: `report.status` says which.
    Stopped,
}

/// The rebin result as [`evaluate`] records it.
pub enum BinStep {
    None,
    Decided(String),
    Refused(&'static str, String, &'static str),
}

/// The original render orchestration from the spec checks to the type's comparison values
/// (`render`: `_given_in`, `transform.apply`, the panels' censored check, `_semantic`,
/// `censored_refusal`, `choose`, `_type_refusals`, `transform.for_type`, `_stage_notes`).
///
/// `source` is the data as written (for R5-11 and an index's start values), `data` the data after
/// any derived measure. `panels` says whether the spec is drawn as several panels.
pub fn evaluate(
    spec: &Spec,
    source: &PreparedData,
    data: &PreparedData,
    bins: BinStep,
    panels: bool,
    report: &mut Report,
) -> Outcome {
    evaluate_choice(spec, source, data, bins, panels, report).0
}

/// [`evaluate`] with the chooser's choice, also when a refusal after the choice stops the chart
/// (the original's `choose_path` returns it then).
pub fn evaluate_choice(
    spec: &Spec,
    source: &PreparedData,
    data: &PreparedData,
    bins: BinStep,
    panels: bool,
    report: &mut Report,
) -> (Outcome, Option<Choice>) {
    let v = View::new(spec, data);
    let vs = View::new(spec, source);
    semantic::given_in(&vs, source, report);
    let mut stopped = transform::units(&vs, report);
    if !stopped {
        stopped = transform::apply(&v, source, report);
    }
    if stopped {
        return (stop(report, "refused"), None);
    }
    if panels {
        if semantic::censored_refusal(&v, "small multiples or a merged chart", report) {
            return (stop(report, "refused"), None);
        }
        if let Some(status) = arrangement::arrangement(&v, report) {
            return (stop(report, status), None);
        }
        return (Outcome::Panels, None);
    }
    let mut halt = semantic::semantic(&v, report);
    let asked = v.chart_type();
    if asked != "auto" && semantic::censored_refusal(&v, asked, report) {
        return (stop(report, "refused"), None);
    }
    match bins {
        BinStep::None => {}
        BinStep::Decided(text) => rules::decide(report, "R2-43", text, Some("locked")),
        BinStep::Refused(path, message, supply) => {
            if rules::refusal(report, &v, "R2-43", message, Some(path), Some(supply)) {
                return (stop(report, "refused"), None);
            }
        }
    }
    let choice = match choose::choose(&v, report) {
        Ok(choice) => choice,
        Err(Unsupported(message)) => {
            rules::info(report, "CG-3", message, None);
            return (stop(report, "unsupported"), None);
        }
    };
    if let Some(c) = &choice
        && asked == "auto"
        && semantic::censored_refusal(&v, &c.type_, report)
    {
        halt = true;
    }
    let Some(choice) = choice else {
        return (stop(report, "refused"), None);
    };
    if halt
        || semantic::type_refusals(&v, &choice.type_, report)
        || statistical::refusals(&v, &choice.type_, report)
    {
        return (stop(report, "refused"), Some(choice));
    }
    if transform::for_type(&v, data, &choice.type_, report) {
        return (stop(report, "refused"), Some(choice));
    }
    let report_type = report.chart_type.clone();
    semantic::stage_notes(&v, report_type.as_deref(), report);
    let decision = decision_of(spec, data, &choice);
    (Outcome::Drawn(choice.clone(), decision), Some(choice))
}

fn stop(report: &mut Report, status: &str) -> Outcome {
    report.status = status.into();
    Outcome::Stopped
}

/// The typed decision of a choice: the drawn family, the orientation asked for (auto is settled
/// by the layout; vertical here) and the category order of the sort.
pub fn decision_of(spec: &Spec, data: &PreparedData, choice: &Choice) -> Decision {
    let orientation = if choice.orientation == "horizontal" {
        Orientation::Horizontal
    } else {
        Orientation::Vertical
    };
    Decision {
        chart_type: ChartType::parse(&choice.type_),
        orientation,
        order: order(spec, data, &choice.sort, &choice.sort_by),
        findings: vec![],
        decisions: vec![],
        status: "drawn".into(),
    }
}

/// The categories' order under a sort (`layout/bars.order`, 02 D1, R9-9): the known values of
/// the sort series by value (a stable sort), then the catch-all, missing and total categories
/// (R9-9's tail, in that order), then the values not known, each in the given order.
fn order(spec: &Spec, data: &PreparedData, sort: &str, sort_by: &str) -> Vec<usize> {
    sorted_order(spec, data, sort, sort_by).0
}

/// (order, tail and not-known categories) of `layout/bars.order`.
pub(crate) fn sorted_order(
    spec: &Spec,
    data: &PreparedData,
    sort: &str,
    sort_by: &str,
) -> (Vec<usize>, Vec<usize>) {
    let all: Vec<usize> = (0..data.categories.len()).collect();
    if !(sort == "asc" || sort == "desc") || data.series.is_empty() {
        return (all, vec![]);
    }
    // a paired contrast (dumbbell) sorts by the difference of its two conditions
    let difference: Option<Vec<Option<chartgen_model::Decimal>>> = (sort_by == "difference" && data.series.len() == 2).then(|| {
        let first_minus_second = spec.get("/chart/difference").as_str() == Some("first_minus_second");
        let (a, b) = if first_minus_second { (1, 0) } else { (0, 1) };
        (0..data.categories.len())
            .map(|i| match (data.series[b].values.get(i).and_then(|d| d.value), data.series[a].values.get(i).and_then(|d| d.value)) {
                (Some(x), Some(y)) => Some(x - y),
                _ => None,
            })
            .collect()
    });
    let s = data
        .series
        .iter()
        .find(|s| s.id == sort_by)
        .unwrap_or(&data.series[0]);
    let roles = spec.get("/data/x/roles");
    let last = |i: usize| match roles
        .get(&data.categories[i])
        .and_then(Value::as_str)
        .unwrap_or("")
    {
        "all_others" => Some(1),
        "missing" => Some(2),
        "total" => Some(3),
        _ => None,
    };
    let value = |i: usize| match &difference {
        Some(d) => d.get(i).copied().flatten(),
        None => s.values.get(i).and_then(|d| d.value),
    };
    let mut regular: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| value(i).is_some() && last(i).is_none())
        .collect();
    let mut tail: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| value(i).is_some() && last(i).is_some())
        .collect();
    let unknown: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| value(i).is_none())
        .collect();
    regular.sort_by(|&a, &b| {
        let c = value(a).cmp(&value(b));
        if sort == "desc" { c.reverse() } else { c }
    });
    tail.sort_by_key(|&i| last(i));
    let rest: Vec<usize> = tail.iter().chain(&unknown).copied().collect();
    let mut order = regular;
    order.extend(&rest);
    (order, rest)
}

/// The chooser alone for a spec whose checks are done elsewhere (a panel of small multiples):
/// `choose` and the type's refusals into a scratch report, as a typed decision.
pub fn decide(spec: &Spec, data: &PreparedData, _facts: &DataFacts) -> Result<Decision> {
    let v = View::new(spec, data);
    let mut report = Report::new(spec.id(), "plain");
    let choice = choose::choose(&v, &mut report);
    let mut decision = match choice {
        Ok(Some(c)) if !semantic::type_refusals(&v, &c.type_, &mut report) => {
            decision_of(spec, data, &c)
        }
        Ok(_) => Decision {
            chart_type: None,
            orientation: Orientation::Vertical,
            order: (0..data.categories.len()).collect(),
            findings: vec![],
            decisions: vec![],
            status: "refused".into(),
        },
        Err(Unsupported(message)) => {
            rules::info(&mut report, "CG-3", message, None);
            Decision {
                chart_type: None,
                orientation: Orientation::Vertical,
                order: (0..data.categories.len()).collect(),
                findings: vec![],
                decisions: vec![],
                status: "unsupported".into(),
            }
        }
    };
    decision.findings = report.findings;
    decision.decisions = report.decisions;
    Ok(decision)
}

/// The chooser's path for a spec (`choose.procedure`'s steps with their guide ids and group), as
/// `chartgen choose` prints it.
pub fn guide_path(spec: &Spec, data: &PreparedData, _facts: &DataFacts) -> Vec<Value> {
    let v = View::new(spec, data);
    let route = procedure::procedure(&v);
    route
        .steps
        .iter()
        .map(|s| {
            let mut s = s.clone();
            let rule = s["rule"].as_str().unwrap_or("").to_owned();
            s["guide"] = serde_json::json!(guide_ids(&rule));
            s["group"] = serde_json::json!(route.group);
            s
        })
        .collect()
}

/// The path of a panel's type after the step that made the panels (`choose.panel_path`), as
/// `chartgen choose` shows it for the first panel of small multiples or a merged chart.
pub fn panel_path(sub: &Spec, data: &PreparedData, facts: &DataFacts, kind: &str) -> Vec<Value> {
    let what = if kind == "merged" {
        "a merged chart, a panel per measure (R2-16, G2.32)"
    } else {
        "small multiples, the panels arranged for the comparison the insight needs (G1.18, G1.19)"
    };
    guide_path(sub, data, facts)
        .into_iter()
        .map(|s| {
            if s["step"] == 2 {
                serde_json::json!({"step": 2, "name": "variables", "rule": "RP3-2", "level": "guided",
                    "guide": guide_ids("RP3-2"), "group": s["group"],
                    "text": format!("several panels: {what}; the path of the first panel's type follows")})
            } else {
                s
            }
        })
        .collect()
}

/// The report type of several panels: `multiples of bar`, `merged of line`.
pub fn report_type(spec: &Spec, decision: &Decision) -> Option<String> {
    let mark = decision.chart_type?.as_str();
    if spec
        .get("/data/measures")
        .as_array()
        .is_some_and(|v| v.len() > 1)
    {
        Some(format!("merged of {mark}"))
    } else if spec
        .get("/data/dims")
        .as_array()
        .is_some_and(|v| !v.is_empty())
    {
        Some(format!("multiples of {mark}"))
    } else {
        Some(mark.into())
    }
}

/// The scale's decisions and guided findings once it is trained (`compute_scale`): `trained` is
/// Err(message) when no step fits, a CG-4 refusal. Returns false when the chart is refused.
pub fn scale_decisions(
    spec: &Spec,
    data: &PreparedData,
    chart_type: &str,
    trained: std::result::Result<&Explained, String>,
    report: &mut Report,
) -> bool {
    let v = View::new(spec, data);
    scale::scale_report(report, &v, chart_type, trained)
}

/// The refusals the original makes as the type's layout begins (a highlight method the family
/// never draws, a pie's legend, a table's rejected versions, a step chart without its end, an
/// arrow chart without its % panel, a colour encoding a quantity cannot take). Returns true when
/// the chart must not be drawn; the layouts receive only charts that pass.
pub fn layout_refusals(
    spec: &Spec,
    data: &PreparedData,
    choice: &Choice,
    report: &mut Report,
) -> bool {
    let v = View::new(spec, data);
    marks::layout_refusals(&v, choice, report)
}

/// Why a type keeps the order the spec gives (`choose.GIVEN_ORDER`), if it does.
pub fn given_order(chart_type: &str) -> Option<&'static str> {
    choose::given_order(chart_type)
}

/// Whether the values are parts of a total (`choose.is_breakdown`).
pub fn is_breakdown(spec: &Spec, data: &PreparedData) -> bool {
    procedure::is_breakdown(&View::new(spec, data))
}

/// The procedure's route for a spec (`choose.procedure`): the recommended type, its rule and why,
/// the group and the steps.
pub fn route(spec: &Spec, data: &PreparedData) -> Route {
    procedure::procedure(&View::new(spec, data))
}

/// `choose.choose` alone: the choice (None when a refusal stops the chart) or Unsupported, with
/// its findings, decisions, path and type recorded in the report.
pub fn choose(
    spec: &Spec,
    data: &PreparedData,
    report: &mut Report,
) -> std::result::Result<Option<Choice>, Unsupported> {
    choose::choose(&View::new(spec, data), report)
}

/// `choose.checks`: the four checks before a pie, stacked chart, treemap or waterfall, as (name,
/// state: pass, fail or missing, rule).
pub fn part_checks(
    spec: &Spec,
    data: &PreparedData,
) -> Vec<(&'static str, &'static str, &'static str)> {
    procedure::checks(&View::new(spec, data))
        .into_iter()
        .map(|c| {
            let state = match c.state {
                procedure::State::Pass => "pass",
                procedure::State::Fail => "fail",
                procedure::State::Missing => "missing",
            };
            (c.name, state, c.rule)
        })
        .collect()
}
