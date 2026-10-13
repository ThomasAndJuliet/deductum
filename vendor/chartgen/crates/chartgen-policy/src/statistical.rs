//! The statistical chart types (STATS.md): which one a spec asks for or implies, and what must
//! be refused or noted about it. The input is read by `chartgen-statistics`, the same code that
//! scale training and layout use, so the policy cannot accept what the layout would misdraw.
use crate::procedure::Route;
use crate::rules;
use crate::view::View;
use chartgen_model::Report;
use chartgen_statistics::{
    self as statistics,
    notes::{Level, Notes},
};
use serde_json::Value;

/// What a type is for, in the words of the route.
fn purpose(t: &str) -> &'static str {
    match t {
        "box" => {
            "quartiles, whiskers and outliers of the measurements in each category, with n shown"
        }
        "strip" => "every measurement, with its median: the whole distribution of a small sample",
        "ecdf" => {
            "the share of measurements at or below each value: the shape of the distribution and its tail"
        }
        "scatter" => "two numeric measures against each other, one mark per case",
        "dumbbell" => "two conditions measured on the same categories, joined, or their difference",
        "interval_dot" | "interval_bar" => "an estimate with an interval that says what it is",
        "state_matrix" => "a declared state in every cell of a rows-by-columns grid",
        "contingency" => "the counts of an exhaustive two-way partition, with marginals",
        _ => "",
    }
}

/// The type the data themselves call for under `chart.type: auto`.
fn implied(v: &View) -> Option<(&'static str, String)> {
    statistics::infer_type(&v.spec.raw)
}

/// Step "variables" of the procedure for a statistical chart (G1.10): the type asked for, or
/// the one the data imply. True when it decided, which skips the steps for point values.
pub(crate) fn route(v: &View, r: &mut Route) -> bool {
    let asked = v.chart_type();
    let (t, why) = if statistics::is_statistical(asked) {
        (asked, format!("type {asked}, as asked: {}", purpose(asked)))
    } else if asked == "auto" {
        match implied(v) {
            Some((t, why)) => (t, why),
            None => return false,
        }
    } else {
        return false;
    };
    r.guided(2, "XS-ROUTE", format!("statistical chart: {why}"));
    r.done(t, "XS-ROUTE", why);
    true
}

/// A JSON pointer as chartgen's finding paths write it: `/data/series/0/samples` is
/// `data.series[0].samples`.
fn dotted(pointer: &str) -> String {
    let mut out = String::new();
    for seg in pointer.split('/').filter(|s| !s.is_empty()) {
        if seg.chars().all(|c| c.is_ascii_digit()) {
            out.push_str(&format!("[{seg}]"));
        } else {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(seg);
        }
    }
    out
}

fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The refusals, warnings and notes of a statistical chart, and the choices made for the caller.
/// True when the chart must not be drawn. These refusals are not overridable (R0-04): there is no
/// form of the chart to draw without the fact they ask for.
pub(crate) fn refusals(v: &View, t: &str, report: &mut Report) -> bool {
    if !statistics::is_statistical(t) {
        return false;
    }
    let mut notes = Notes::default();
    statistics::parse(&v.spec.raw, t, &mut notes);
    let mut stop = false;
    for i in &notes.issues {
        let severity = match i.level {
            Level::Error => {
                stop = true;
                "error"
            }
            Level::Warn => "warn",
            Level::Info => "info",
        };
        let mut f = rules::finding(
            &i.rule,
            severity,
            i.message.clone(),
            Some(&dotted(&i.path)),
            None,
        );
        f.supply = i.supply.clone();
        report.findings.push(f);
    }
    for c in &notes.choices {
        rules::decide(
            report,
            "XS-DEFAULT",
            format!("{}: {} ({})", c.what, plain(&c.value), c.why),
            Some("guided"),
        );
    }
    stop
}
