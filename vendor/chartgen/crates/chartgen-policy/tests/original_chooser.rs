//! The original's `tests/test_chooser.py` (classes Procedure, Checks and Choices), translated:
//! the same specs, the same expected types, rules, groups, refusals and paths.
use chartgen_data::{parse, transform};
use chartgen_model::{PreparedData, Report, Spec};
use chartgen_policy::{Choice, Route, Unsupported, choose, part_checks, route};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const GROUPS: [&str; 8] = [
    "data over time",
    "change between two periods",
    "breakdowns of totals",
    "breakdowns of totals changing over time",
    "maps",
    "maps changing over time",
    "bar charts",
    "tables",
];

fn ok() -> Value {
    json!({"meaningful_total": true, "all_parts": true, "overlap": false})
}
fn with(parts: Value, key: &str, value: Value) -> Value {
    let mut p = parts;
    p[key] = value;
    p
}
fn five() -> Value {
    json!([165, 160, 135, 125, 100])
}

/// `spec(values, x, names, grid, message, parts, series, **top)` of test_chooser.py.
#[derive(Default)]
struct S {
    values: Option<Value>,
    x: Option<&'static str>,
    names: Option<Vec<String>>,
    grid: Option<&'static str>,
    message: Option<&'static str>,
    parts: Option<Value>,
    series: Option<Vec<(String, Value)>>,
    top: Vec<(&'static str, Value)>,
}
impl S {
    fn build(self) -> Value {
        let series = self
            .series
            .unwrap_or_else(|| vec![("s".into(), self.values.clone().unwrap())]);
        let n = series[0].1.as_array().unwrap().len();
        let names = self
            .names
            .unwrap_or_else(|| (0..n).map(|i| format!("C{i}")).collect());
        let mut d = json!({"chartspec": 1, "id": "t", "job": {"kind": "lookup", "text": "t"},
            "title": {"text": "T"},
            "data": {"x": {"kind": self.x.unwrap_or("nominal"), "values": names},
                     "measure": {"kind": "amount", "zero": "meaningful", "unit": {"prefix": "$"}},
                     "series": series.iter().map(|(k, v)| json!({"id": k, "values": v})).collect::<Vec<_>>()},
            "facts": {}});
        if let Some(g) = self.grid {
            d["data"]["x"]["grid"] = json!(g);
            d["data"]["measure"]["persistence"] = json!("summary");
        }
        if let Some(m) = self.message {
            d["facts"]["message"] = json!(m);
        }
        if let Some(p) = self.parts {
            d["data"]["parts"] = p;
        }
        for (k, v) in self.top {
            if v.is_object() && d.get(k).is_some_and(Value::is_object) {
                for (kk, vv) in v.as_object().unwrap() {
                    d[k][kk] = vv.clone();
                }
            } else {
                d[k] = v;
            }
        }
        d
    }
}
fn months(n: usize, series: Vec<(String, Value)>, s: S) -> Value {
    S {
        x: Some("time"),
        names: Some((1..=n).map(|m| format!("2022-{m:02}")).collect()),
        grid: Some("month"),
        series: Some(series),
        ..s
    }
    .build()
}
fn ser(pairs: &[(&str, Value)]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
fn seq(a: i64, b: i64) -> Value {
    json!((a..b).collect::<Vec<_>>())
}

fn load(raw: &Value) -> (Spec, PreparedData) {
    let spec = Spec { raw: raw.clone() };
    let data = transform(&spec, &parse(&spec).expect("parses")).expect("transforms");
    (spec, data)
}
fn procedure(raw: &Value) -> Route {
    let (spec, data) = load(raw);
    route(&spec, &data)
}
/// `chosen(d)`: (choice, report), Err for Unsupported.
fn chosen(raw: &Value) -> (std::result::Result<Option<Choice>, Unsupported>, Report) {
    let (spec, data) = load(raw);
    let mut report = Report::new("t", "-");
    let c = choose(&spec, &data, &mut report);
    if let Err(Unsupported(m)) = &c {
        report
            .findings
            .push(chartgen_model::Finding::new("CG-3", "info", m.clone()));
    }
    (c, report)
}
fn errors(rep: &Report) -> BTreeSet<String> {
    rep.findings
        .iter()
        .filter(|f| f.severity == "error")
        .map(|f| f.rule.clone())
        .collect()
}
fn set(rules: &[&str]) -> BTreeSet<String> {
    rules.iter().map(|r| r.to_string()).collect()
}

fn cases() -> Vec<(
    &'static str,
    Value,
    &'static str,
    &'static str,
    &'static str,
)> {
    let two = || ser(&[("a", json!([5, 6, 7])), ("b", json!([3, 2, 4]))]);
    let ab = || ser(&[("a", seq(1, 13)), ("b", seq(2, 14))]);
    vec![
        ("lookup only", S { values: Some(five()), message: Some("lookup_only"), ..S::default() }.build(), "table", "R10-11", "tables"),
        ("export only", S { values: Some(five()), message: Some("export_only"), ..S::default() }.build(), "table", "R10-11", "tables"),
        ("not comparable", S { values: Some(five()), message: Some("not_comparable"), ..S::default() }.build(), "table", "R10-11", "tables"),
        ("31 series over time", months(12, (0..31).map(|i| (format!("s{i}"), seq(1, 13))).collect(), S::default()), "heatmap", "R6-36", "data over time"),
        ("31 series over categories", S { series: Some((0..31).map(|i| (format!("s{i}"), json!([1, 2, 3]))).collect()), ..S::default() }.build(), "heatmap", "R6-36", "tables"),
        ("location", S { values: Some(five()), top: vec![("facts", json!({"location_matters": true}))], ..S::default() }.build(), "map", "R8-2", "maps"),
        ("location over time", months(12, ser(&[("a", seq(1, 13))]), S { top: vec![("facts", json!({"location_matters": true}))], ..S::default() }), "map_lines", "R8-13", "maps changing over time"),
        ("parts, no message", S { values: Some(five()), parts: Some(ok()), ..S::default() }.build(), "bar", "R7-06", "breakdowns of totals"),
        ("parts compared", S { values: Some(five()), parts: Some(ok()), message: Some("compare_parts_precisely"), ..S::default() }.build(), "bar", "R7-06", "breakdowns of totals"),
        ("many parts compared", S { values: Some(json!((1..=45).rev().collect::<Vec<_>>())), parts: Some(ok()), message: Some("compare_parts_precisely"), ..S::default() }.build(), "wrapped_bar", "R7-14", "breakdowns of totals"),
        ("fractions", S { values: Some(five()), parts: Some(ok()), message: Some("fraction_of_total"), ..S::default() }.build(), "pie", "R7-07", "breakdowns of totals"),
        ("subtotal fraction", S { values: Some(five()), parts: Some(ok()), message: Some("subtotal_fraction"), ..S::default() }.build(), "pie", "R7-07", "breakdowns of totals"),
        ("many parts, fractions", S { values: Some(json!((1..=9).rev().collect::<Vec<_>>())), parts: Some(ok()), message: Some("fraction_of_total"), ..S::default() }.build(), "treemap", "R7-15", "breakdowns of totals"),
        ("subtotal amount", S { values: Some(five()), parts: Some(ok()), message: Some("subtotal_amount"), ..S::default() }.build(), "stacked_bar", "R7-08", "breakdowns of totals"),
        ("running subtotals, one sign", S { values: Some(five()), parts: Some(ok()), message: Some("cumulative_subtotals"), ..S::default() }.build(), "stacked_bar", "R7-08", "breakdowns of totals"),
        ("running subtotals, mixed signs", S { values: Some(json!([3.7, 3.55, -2.2, -0.6])), parts: Some(ok()), message: Some("cumulative_subtotals"), ..S::default() }.build(), "waterfall", "R7-11", "breakdowns of totals"),
        ("pie-averse", S { values: Some(five()), parts: Some(ok()), message: Some("fraction_of_total"), top: vec![("audience", json!({"pie_averse": true}))], ..S::default() }.build(), "stacked_bar_100", "R7-40", "breakdowns of totals"),
        ("check: averages", S { values: Some(json!([24.55, 17.92, 16.24])), parts: Some(with(ok(), "meaningful_total", json!(false))), message: Some("fraction_of_total"), ..S::default() }.build(), "bar", "R7-02", "breakdowns of totals"),
        ("check: a top 3", S { values: Some(json!([375, 348, 200])), parts: Some(with(ok(), "all_parts", json!(false))), message: Some("fraction_of_total"), ..S::default() }.build(), "bar", "R7-03", "breakdowns of totals"),
        ("check: overlaps", S { values: Some(json!([118.4, 61.6, 36.6, 21.4, 20.0])), names: Some(["Cars", "Trucks", "Sports cars", "Pickup trucks", "Motorcycles"].map(String::from).to_vec()), parts: Some(json!({"meaningful_total": true, "all_parts": true, "overlaps": [["Sports cars", "Cars"]]})), message: Some("fraction_of_total"), ..S::default() }.build(), "bar", "R7-05", "breakdowns of totals"),
        ("check: mixed signs", S { values: Some(json!([34, 8.5, 8, -4.3, -16.5])), parts: Some(ok()), message: Some("fraction_of_total"), ..S::default() }.build(), "bar", "R7-04", "breakdowns of totals"),
        ("parts across totals", S { parts: Some(ok()), message: Some("compare_parts_across_totals"), series: Some(two()), ..S::default() }.build(), "bar", "02 D1", "breakdowns of totals"),
        ("totals compared", S { parts: Some(ok()), message: Some("compare_totals"), series: Some(two()), ..S::default() }.build(), "stacked_bar", "R7-20", "breakdowns of totals"),
        ("fractions within each", S { parts: Some(ok()), message: Some("fractions_within_each_total"), series: Some(two()), ..S::default() }.build(), "pie", "R7-21", "breakdowns of totals"),
        ("fractions within each, pie-averse", S { parts: Some(ok()), message: Some("fractions_within_each_total"), series: Some(two()), top: vec![("audience", json!({"pie_averse": true}))], ..S::default() }.build(), "stacked_bar_100", "R7-40", "breakdowns of totals"),
        ("totals compared, not a total", S { parts: Some(with(ok(), "meaningful_total", json!(false))), message: Some("compare_totals"), series: Some(two()), ..S::default() }.build(), "bar", "02 D1", "breakdowns of totals"),
        ("totals compared, eight parts", S { parts: Some(ok()), message: Some("compare_totals"), series: Some((0..8).map(|i| (format!("p{i}"), json!([5, 6, 7]))).collect()), ..S::default() }.build(), "bar", "02 D1", "breakdowns of totals"),
        ("several totals, parts missing", S { parts: Some(with(ok(), "all_parts", json!(false))), message: Some("compare_totals"), series: Some(two()), ..S::default() }.build(), "bar", "02 D1", "breakdowns of totals"),
        ("parts over time", months(12, ab(), S { parts: Some(ok()), ..S::default() }), "line", "R6-4", "breakdowns of totals changing over time"),
        ("parts and total over time", months(12, ab(), S { parts: Some(ok()), top: vec![("facts", json!({"total_matters": true}))], ..S::default() }), "line_total", "R7-27", "breakdowns of totals changing over time"),
        ("fractions over time", months(12, ab(), S { parts: Some(ok()), message: Some("fraction_of_total"), ..S::default() }), "stacked_area_100", "R7-30", "breakdowns of totals changing over time"),
        ("fractions over time, periods", months(12, ab(), S { parts: Some(ok()), message: Some("fraction_of_total"), top: vec![("facts", json!({"focus": "periods"}))], ..S::default() }), "stacked_bar_100", "R7-30", "breakdowns of totals changing over time"),
        ("fractions and total over time", months(12, ab(), S { parts: Some(ok()), message: Some("fraction_of_total"), top: vec![("facts", json!({"total_matters": true}))], ..S::default() }), "stacked_area", "R7-29", "breakdowns of totals changing over time"),
        ("fractions and total, periods", months(12, ab(), S { parts: Some(ok()), message: Some("fraction_of_total"), top: vec![("facts", json!({"total_matters": true, "focus": "periods"}))], ..S::default() }), "stacked_bar", "R7-29", "breakdowns of totals changing over time"),
        ("fractions over time, mixed signs", months(12, ser(&[("a", seq(1, 13)), ("b", json!([-1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]))]), S { parts: Some(ok()), message: Some("fraction_of_total"), ..S::default() }), "line", "R6-4", "breakdowns of totals changing over time"),
        ("two periods", S { x: Some("time"), names: Some(vec!["2021".into(), "2022".into()]), grid: Some("year"), series: Some(ser(&[("e", json!([482, 517])), ("c", json!([372, 318]))])), ..S::default() }.build(), "arrow", "R6-15", "change between two periods"),
        ("two periods, rank", S { x: Some("time"), names: Some(vec!["2021".into(), "2022".into()]), grid: Some("year"), message: Some("rank"), series: Some(ser(&[("e", json!([482, 517])), ("c", json!([372, 318])), ("s", json!([356, 455]))])), ..S::default() }.build(), "slope", "R6-16", "change between two periods"),
        ("two periods, rank, eleven", S { x: Some("time"), names: Some(vec!["2021".into(), "2022".into()]), grid: Some("year"), message: Some("rank"), series: Some((0..11).map(|i| (format!("s{i}"), json!([i, i + 1]))).collect()), ..S::default() }.build(), "arrow", "R6-15", "change between two periods"),
        ("a regular series", months(12, ser(&[("a", seq(1, 13))]), S::default()), "line", "R6-4", "data over time"),
        ("periods", months(12, ser(&[("a", seq(1, 13))]), S { message: Some("periods"), ..S::default() }), "bar", "R6-5", "data over time"),
        ("rank", months(12, ab(), S { message: Some("rank"), ..S::default() }), "bumps", "R6-7", "data over time"),
        ("sparse", months(12, ser(&[("a", json!([1, null, 3, null, null, 5, null, null, 7, null, null, 9]))]), S::default()), "dot", "R6-2", "data over time"),
        ("bars", S { values: Some(five()), ..S::default() }.build(), "bar", "02 D1", "bar charts"),
        ("clustered", S { series: Some(two()), ..S::default() }.build(), "bar", "02 D1", "bar charts"),
        ("dots", S { values: Some(json!([700, 710, 690])), top: vec![("facts", json!({"distance_to_zero_irrelevant": true}))], ..S::default() }.build(), "dot", "R2-24", "bar charts"),
        ("wrapped", S { values: Some(json!((1..=45).rev().collect::<Vec<_>>())), ..S::default() }.build(), "wrapped_bar", "R7-14", "bar charts"),
        ("action dots", S { values: Some(five()), message: Some("action_needed"), ..S::default() }.build(), "action_dots", "R12-4", "bar charts"),
        ("funnel", S { values: Some(json!([9152, 6681, 5211, 2658, 2126])), x: Some("ordinal"), message: Some("stage_conversion"), ..S::default() }.build(), "funnel_bars", "R12-5", "bar charts"),
        ("profiles", S { message: Some("profiles"), series: Some(ser(&[("a", json!([2.4, 3.7])), ("b", json!([2.9, 3.4]))])), ..S::default() }.build(), "heatmap", "R12-6", "tables"),
        ("ordered, a pattern, data-savvy", S { values: Some(json!([5, 9, 7, 3])), x: Some("ordinal"), message: Some("pattern"), top: vec![("audience", json!({"data_savvy": true}))], ..S::default() }.build(), "line", "R6-41", "bar charts"),
        ("ordered, a pattern", S { values: Some(json!([5, 9, 7, 3])), x: Some("ordinal"), message: Some("pattern"), ..S::default() }.build(), "bar", "R6-41", "bar charts"),
    ]
}

#[test]
fn every_branch() {
    for (name, d, t, rule, group) in cases() {
        let r = procedure(&d);
        assert_eq!(
            (r.type_.as_deref(), r.rule.as_deref(), r.group.as_deref()),
            (Some(t), Some(rule), Some(group)),
            "{name}"
        );
        assert!(GROUPS.contains(&group));
    }
}

#[test]
fn the_path() {
    for (name, d, ..) in cases() {
        let r = procedure(&d);
        let steps: Vec<i64> = r
            .steps
            .iter()
            .map(|s| s["step"].as_i64().unwrap())
            .collect();
        let mut sorted = steps.clone();
        sorted.sort();
        assert_eq!(steps, sorted, "{name}");
        assert_eq!((steps[0], *steps.last().unwrap()), (0, 9), "{name}");
        assert!(r.steps.iter().all(|s| {
            ["locked", "guided", "open"].contains(&s["level"].as_str().unwrap())
                && !s["rule"].as_str().unwrap().is_empty()
        }));
        let entered: Vec<&str> = r
            .steps
            .iter()
            .map(|s| s["text"].as_str().unwrap())
            .filter(|t| t.starts_with("group: "))
            .collect();
        let last = entered.last().unwrap();
        assert_eq!(
            entered.len(),
            if last.contains("in place of") { 2 } else { 1 },
            "{name}"
        );
        assert!(
            last.starts_with(&format!("group: {}", r.group.clone().unwrap())),
            "{name}"
        );
    }
}

#[test]
fn a_breakdown_over_two_periods_moves_to_the_arrow_chart() {
    let d = S {
        x: Some("time"),
        names: Some(vec!["2021".into(), "2022".into()]),
        grid: Some("year"),
        parts: Some(ok()),
        series: Some(ser(&[("e", json!([482, 517])), ("c", json!([372, 318]))])),
        ..S::default()
    }
    .build();
    let r = procedure(&d);
    assert_eq!(
        (r.type_.as_deref(), r.group.as_deref()),
        (Some("arrow"), Some("change between two periods"))
    );
    let five = r.steps.iter().find(|s| s["step"] == 5).unwrap();
    assert!(
        five["text"]
            .as_str()
            .unwrap()
            .contains("in place of breakdowns of totals changing over time")
    );
}

#[test]
fn the_checks_are_locked_and_in_order() {
    let d = S {
        values: Some(json!([24.55, 17.92, 16.24])),
        parts: Some(with(ok(), "meaningful_total", json!(false))),
        message: Some("fraction_of_total"),
        ..S::default()
    }
    .build();
    let r = procedure(&d);
    let c: Vec<&Value> = r
        .steps
        .iter()
        .filter(|s| s["text"].as_str().unwrap().starts_with("check, "))
        .collect();
    assert_eq!(
        c.iter()
            .map(|s| s["rule"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["R7-02", "R7-03", "R7-05", "R7-04"]
    );
    assert!(c.iter().all(|s| s["level"] == "locked"));
    assert!(c[0]["text"].as_str().unwrap().contains("fails"));
    let d = S {
        values: Some(five()),
        parts: Some(ok()),
        message: Some("compare_parts_precisely"),
        ..S::default()
    }
    .build();
    assert!(
        !procedure(&d)
            .steps
            .iter()
            .any(|s| s["text"].as_str().unwrap().starts_with("check, "))
    );
}

#[test]
fn recommended_type_keeps_its_form() {
    let r = procedure(
        &S {
            values: Some(five()),
            ..S::default()
        }
        .build(),
    );
    assert_eq!(
        (r.type_.as_deref(), r.rule.as_deref(), r.why.as_deref()),
        (
            Some("bar"),
            Some("02 D1"),
            Some("categories compared by value")
        )
    );
}

#[test]
fn a_pie_request_routed_away_by_each_check() {
    let cases = [
        (
            with(ok(), "meaningful_total", json!(false)),
            five(),
            "R7-02",
        ),
        (with(ok(), "all_parts", json!(false)), five(), "R7-03"),
        (with(ok(), "overlap", json!(true)), five(), "R7-05"),
        (ok(), json!([34, 8.5, 8, -4.3, -16.5]), "R7-04"),
    ];
    for (parts, values, rule) in cases {
        let mut d = S {
            values: Some(values),
            parts: Some(parts),
            message: Some("fraction_of_total"),
            ..S::default()
        }
        .build();
        let (c, rep) = chosen(&d);
        assert_eq!(c.unwrap().unwrap().type_, "bar", "{rule}");
        assert!(errors(&rep).is_empty(), "{rule}");
        d["chart"] = json!({"type": "pie"});
        let (c, rep) = chosen(&d);
        assert!(c.unwrap().is_none(), "{rule}");
        assert_eq!(errors(&rep), set(&[rule]));
        let f = rep.findings.iter().find(|f| f.rule == rule).unwrap();
        assert_eq!(
            (f.level.as_str(), f.path.as_deref()),
            ("locked", Some("chart.type"))
        );
        d["overrides"] = json!([{"rule": rule, "reason": "the book's bad version"}]);
        let (c, rep) = chosen(&d);
        assert_eq!(c.unwrap().unwrap().type_, "pie");
        assert!(
            rep.findings
                .iter()
                .any(|f| f.rule == rule && f.severity == "overridden")
        );
    }
}

#[test]
fn missing_facts_are_required_before_a_pie() {
    let (c, rep) = chosen(
        &S {
            values: Some(five()),
            message: Some("fraction_of_total"),
            ..S::default()
        }
        .build(),
    );
    assert!(c.unwrap().is_none());
    assert_eq!(errors(&rep), set(&["R7-02", "R7-03", "R7-05"]));
    let paths: BTreeSet<_> = rep
        .findings
        .iter()
        .filter(|f| f.severity == "error")
        .filter_map(|f| f.path.clone())
        .collect();
    assert_eq!(
        paths,
        set(&[
            "data.parts.meaningful_total",
            "data.parts.all_parts",
            "data.parts.overlap"
        ])
    );
    let (c, rep) = chosen(
        &S {
            values: Some(five()),
            message: Some("compare_parts_precisely"),
            ..S::default()
        }
        .build(),
    );
    assert_eq!(c.unwrap().unwrap().type_, "bar");
    assert!(errors(&rep).is_empty());
}

#[test]
fn overlap_must_be_stated() {
    let mut d = S {
        values: Some(five()),
        parts: Some(json!({"meaningful_total": true, "all_parts": true})),
        message: Some("fraction_of_total"),
        ..S::default()
    }
    .build();
    let r = procedure(&d);
    assert_eq!(r.type_.as_deref(), Some("pie"));
    assert!(r.steps.iter().any(|s| {
        s["text"]
            .as_str()
            .unwrap()
            .contains("check, overlap: not known")
    }));
    let (c, rep) = chosen(&d);
    assert!(c.unwrap().is_none());
    let f = rep.findings.iter().find(|f| f.rule == "R7-05").unwrap();
    assert_eq!(
        (f.severity.as_str(), f.level.as_str(), f.path.as_deref()),
        ("error", "locked", Some("data.parts.overlap"))
    );
    assert!(f.supply.as_deref().unwrap().contains("data.parts.overlaps"));
    assert!(
        !rep.findings
            .iter()
            .any(|f| f.rule == "R7-05" && f.severity == "info")
    );
    d["data"]["parts"]["overlap"] = json!(false);
    let (c, rep) = chosen(&d);
    assert_eq!(c.unwrap().unwrap().type_, "pie");
    assert!(errors(&rep).is_empty());
    d["data"]["parts"]
        .as_object_mut()
        .unwrap()
        .remove("overlap");
    d["data"]["parts"]["overlaps"] = json!([["C1", "C0"]]);
    let (c, rep) = chosen(&d);
    assert_eq!(c.unwrap().unwrap().type_, "bar");
    assert!(errors(&rep).is_empty());
}

#[test]
fn a_waterfall_is_for_mixed_signs() {
    let d = S {
        values: Some(json!([3.7, 3.55, -2.2, -0.6])),
        parts: Some(ok()),
        message: Some("cumulative_subtotals"),
        top: vec![("chart", json!({"type": "waterfall"}))],
        ..S::default()
    }
    .build();
    let (c, rep) = chosen(&d);
    assert!(errors(&rep).is_empty());
    let c = c.unwrap().unwrap();
    assert_eq!((c.type_.as_str(), c.sort.as_str()), ("waterfall", "given"));
}

#[test]
fn seven_parts_flagged() {
    let (c, rep) = chosen(
        &S {
            values: Some(json!([30, 25, 20, 10, 8, 5, 2])),
            parts: Some(ok()),
            message: Some("fraction_of_total"),
            ..S::default()
        }
        .build(),
    );
    assert_eq!(c.unwrap().unwrap().type_, "pie");
    assert!(
        rep.findings
            .iter()
            .any(|f| (f.rule.as_str(), f.severity.as_str(), f.level.as_str())
                == ("R7-13", "warn", "guided"))
    );
}

#[test]
fn checks_for_every_total() {
    let d = S {
        parts: Some(ok()),
        message: Some("compare_totals"),
        series: Some(ser(&[("a", json!([5, -6, 7])), ("b", json!([3, 2, 4]))])),
        ..S::default()
    }
    .build();
    let (spec, data) = load(&d);
    assert_eq!(
        part_checks(&spec, &data)
            .iter()
            .map(|c| c.1)
            .collect::<Vec<_>>(),
        ["pass", "pass", "pass", "fail"]
    );
    assert_eq!(procedure(&d).type_.as_deref(), Some("bar"));
}

#[test]
fn defaults_stand_in_with_infos() {
    let (_, rep) = chosen(
        &S {
            values: Some(five()),
            parts: Some(ok()),
            ..S::default()
        }
        .build(),
    );
    assert!(rep.findings.iter().any(|f| f.rule == "R7-06"
        && f.severity == "info"
        && f.path.as_deref() == Some("facts.message")));
    let (_, rep) = chosen(&months(
        12,
        ser(&[("a", seq(1, 13)), ("b", seq(2, 14))]),
        S {
            parts: Some(ok()),
            ..S::default()
        },
    ));
    assert!(
        rep.findings
            .iter()
            .any(|f| f.rule == "R7-25" && f.severity == "info")
    );
}

#[test]
fn the_avoid_list() {
    let avoided = [
        ("donut", "R7-39", "type pie (or auto)"),
        (
            "dual_axis",
            "R2-14",
            "the measures in data.measures (a merged chart, when the values matter), or transform index (an index chart, when the patterns of change matter)",
        ),
        (
            "radar",
            "R12-6",
            "type heatmap (a heatmap table: items as columns, attributes as rows, the values in coloured cells); circular data such as wind directions excepted, through an override",
        ),
        (
            "funnel",
            "R12-5",
            "type auto with facts.message stage_conversion (the stage counts as bars, the % change from each stage to the next beneath them)",
        ),
        (
            "word_cloud",
            "R12-2",
            "type auto (horizontal bars of the counts, sorted, largest first); decoration excepted, through an override",
        ),
        (
            "packed_bubbles",
            "R12-3",
            "type auto (a sorted horizontal bar chart)",
        ),
        (
            "bullet",
            "R12-4",
            "type auto with facts.message action_needed (action dots)",
        ),
        (
            "choropleth",
            "R8-5",
            "facts.location_matters with type auto (coloured dots, bubbles or bars on regions; phase 4); a choropleth only for regions of similar size, through an override",
        ),
        (
            "3d",
            "R12-7",
            "type auto (the flat chart; a third variable as the series of clustered bars, R12-8)",
        ),
        ("smoothed_line", "R6-39", "type line (straight segments)"),
    ];
    for (t, rule, supply) in avoided {
        let (c, rep) = chosen(
            &S {
                values: Some(five()),
                top: vec![("chart", json!({"type": t}))],
                ..S::default()
            }
            .build(),
        );
        assert!(c.unwrap().is_none(), "{t}");
        assert_eq!(errors(&rep), set(&[rule]), "{t}");
        let f = rep.findings.iter().find(|f| f.rule == rule).unwrap();
        assert_eq!(
            (f.level.as_str(), f.supply.as_deref()),
            ("locked", Some(supply))
        );
        assert_eq!(rep.path.last().unwrap()["step"], 9);
    }
}

#[test]
fn an_overridden_request_is_drawn_only_where_a_layout_exists() {
    let d = S {
        values: Some(five()),
        parts: Some(ok()),
        message: Some("fraction_of_total"),
        top: vec![
            ("chart", json!({"type": "radar"})),
            (
                "overrides",
                json!([{"rule": "R12-6", "reason": "the book's radar"}]),
            ),
        ],
        ..S::default()
    }
    .build();
    let (c, rep) = chosen(&d);
    assert!(c.is_err());
    assert!(rep.findings.last().unwrap().message.contains("avoid-list"));
}

#[test]
fn a_slope_chart_in_its_case() {
    let mut d = S {
        x: Some("time"),
        names: Some(vec!["2022-Q1".into(), "2022-Q2".into()]),
        grid: Some("quarter"),
        message: Some("rank"),
        series: Some(ser(&[
            ("e", json!([482, 517])),
            ("c", json!([372, 318])),
            ("s", json!([356, 455])),
            ("n", json!([123, 174])),
            ("w", json!([88, 113])),
        ])),
        ..S::default()
    }
    .build();
    let (c, rep) = chosen(&d);
    assert_eq!(
        (
            c.unwrap().unwrap().type_.as_str(),
            rep.chart_type.as_deref()
        ),
        ("line", Some("slope"))
    );
    assert!(errors(&rep).is_empty());
    assert_eq!(
        (
            rep.decisions[0]["rule"].as_str(),
            rep.decisions[0]["level"].as_str()
        ),
        (Some("R6-16"), Some("guided"))
    );
    d["facts"] = json!({"message": "change"});
    d["chart"] = json!({"type": "slope"});
    let (c, rep) = chosen(&d);
    assert!(c.unwrap().is_none());
    assert_eq!(errors(&rep), set(&["R6-16"]));
    assert_eq!(
        rep.findings
            .iter()
            .find(|f| f.rule == "R6-16")
            .unwrap()
            .level,
        "locked"
    );
    d["chart"] = json!({"type": "bar"});
    d["facts"] = json!({"message": "rank"});
    let (_, rep) = chosen(&d);
    assert_eq!(errors(&rep), set(&["R6-17"]));
}

#[test]
fn an_ordered_line_for_the_data_savvy_only() {
    let mut d = S {
        values: Some(json!([5, 9, 7, 3])),
        x: Some("ordinal"),
        message: Some("pattern"),
        top: vec![("audience", json!({"data_savvy": true}))],
        ..S::default()
    }
    .build();
    let (c, rep) = chosen(&d);
    assert_eq!(c.unwrap().unwrap().type_, "line");
    assert!(errors(&rep).is_empty());
    d["audience"] = json!({"data_savvy": false});
    let r = procedure(&d);
    assert!(
        r.steps.iter().find(|s| s["rule"] == "R6-41").unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("data-savvy")
    );
    d["chart"] = json!({"type": "line"});
    let (_, rep) = chosen(&d);
    assert_eq!(errors(&rep), set(&["R6-41"]));
}

#[test]
fn an_arrow_chart_joins_two_periods() {
    for mut d in [
        months(
            6,
            ser(&[("a", json!([5, 4, 3, 6, 7, 5]))]),
            S {
                top: vec![("chart", json!({"type": "arrow"}))],
                ..S::default()
            },
        ),
        S {
            values: Some(five()),
            top: vec![("chart", json!({"type": "arrow"}))],
            ..S::default()
        }
        .build(),
    ] {
        let (c, rep) = chosen(&d);
        assert!(c.unwrap().is_none());
        assert_eq!(errors(&rep), set(&["R6-15"]));
        d["overrides"] = json!([{"rule": "R6-15", "reason": "t"}]);
        assert!(chosen(&d).0.is_err());
    }
}

#[test]
fn a_total_panel_takes_the_lines_refusals() {
    let d = S {
        x: Some("time"),
        names: Some(vec!["2021".into(), "2022".into()]),
        grid: Some("year"),
        parts: Some(ok()),
        top: vec![("chart", json!({"type": "line_total"}))],
        series: Some(ser(&[("e", json!([482, 517])), ("c", json!([372, 318]))])),
        ..S::default()
    }
    .build();
    assert_eq!(errors(&chosen(&d).1), set(&["R6-2"]));
}

#[test]
fn a_map_is_phase_4() {
    let (c, rep) = chosen(
        &S {
            values: Some(five()),
            top: vec![("facts", json!({"location_matters": true}))],
            ..S::default()
        }
        .build(),
    );
    assert!(c.is_err());
    assert!(rep.findings.last().unwrap().message.contains("phase 4"));
}

#[test]
fn unfamiliar_types_are_taught() {
    let mut d = months(
        12,
        ser(&[("a", seq(1, 13)), ("b", seq(2, 14))]),
        S {
            message: Some("rank"),
            ..S::default()
        },
    );
    assert!(procedure(&d).steps.iter().any(|s| s["rule"] == "R6-18"));
    d["audience"] = json!({"data_savvy": true});
    assert!(!procedure(&d).steps.iter().any(|s| s["rule"] == "R6-18"));
}

#[test]
fn the_path_is_in_the_report() {
    let (_, rep) = chosen(
        &S {
            values: Some(five()),
            parts: Some(ok()),
            message: Some("fraction_of_total"),
            ..S::default()
        }
        .build(),
    );
    assert_eq!(
        rep.path
            .iter()
            .take(2)
            .map(|p| p["step"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(
        rep.path
            .iter()
            .all(|p| p.get("guide").is_some() && p["group"] == "breakdowns of totals")
    );
}
