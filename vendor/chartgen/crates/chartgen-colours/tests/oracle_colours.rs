//! The colour plan against the original's golden source SVGs (tests/conformance/oracle/plain):
//! every data mark's colour classes (`single`, `series-k`, reserved greys, tints, shades, ramp
//! steps, good and bad) as the original drew them, for every drawn single chart. The specs and
//! variants are the bundled originals, merged as the original's `read_file` merges them (RFC 7386).
use chartgen_model::{ChartType, ColorPlan, Orientation, SemanticPlan, Spec};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance")
}

fn merge_patch(target: &Value, patch: &Value) -> Value {
    let Some(p) = patch.as_object() else {
        return patch.clone();
    };
    let mut out = target.as_object().cloned().unwrap_or_default();
    for (k, v) in p {
        if v.is_null() {
            out.remove(k);
        } else {
            let merged = merge_patch(out.get(k).unwrap_or(&Value::Null), v);
            out.insert(k.clone(), merged);
        }
    }
    Value::Object(out)
}

/// The spec of an oracle run, the registry merged under `colours.slots` as the host does.
pub fn spec_of(file: &str, variant: Option<&str>) -> Value {
    let path = root().join("examples/specs").join(file);
    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("bundled spec")).unwrap();
    let base = raw.get("base").cloned().unwrap_or_else(|| raw.clone());
    let mut spec = match variant {
        None => base,
        Some(name) => {
            let v = raw["variants"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == name)
                .expect("variant");
            merge_patch(&base, &v["patch"])
        }
    };
    if let Some(reg) = spec["colours"]["registry"].as_str() {
        let reg: Value = serde_json::from_str(
            &std::fs::read_to_string(path.parent().unwrap().join(reg)).expect("registry"),
        )
        .unwrap();
        let mut slots = reg.as_object().cloned().unwrap();
        if let Some(own) = spec["colours"]["slots"].as_object() {
            slots.extend(own.clone());
        }
        spec["colours"]["slots"] = Value::Object(slots);
        spec["colours"].as_object_mut().unwrap().remove("registry");
    }
    spec
}

const MARKS: &[&str] = &[
    "bar",
    "dot",
    "segment",
    "slice",
    "tile",
    "cell",
    "line",
    "projection",
    "area",
    "change-shaft",
    "step-line",
    "event-bar",
    "target-mark",
];
const IGNORED: &[&str] = &["highlight", "context", "labelled", "texture", "filled"];

fn colour_class(c: &str) -> bool {
    c == "single"
        || c.starts_with("series-")
        || c.starts_with("seq-")
        || c.starts_with("div-")
        || matches!(
            c,
            "average"
                | "total"
                | "all-others"
                | "missing"
                | "reference"
                | "good"
                | "bad"
                | "tint"
                | "shade-light"
                | "shade-dark"
                | "censored"
                | "censored-hatched"
                | "censored-dotted"
                | "uniform-context"
        )
}

#[derive(Debug, Clone)]
pub struct Mark {
    pub kind: String,
    pub series: Option<String>,
    pub x: Option<String>,
    pub classes: BTreeSet<String>,
}

pub fn golden_marks(svg: &str) -> Vec<Mark> {
    let doc = roxmltree::Document::parse(svg).expect("golden svg");
    let mut out = vec![];
    for n in doc.descendants().filter(|n| n.is_element()) {
        let classes: Vec<&str> = n
            .attribute("class")
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let Some(kind) = MARKS.iter().rev().find(|m| classes.contains(m)) else {
            continue;
        };
        if n.tag_name().name() == "g" {
            continue;
        }
        let up = |a: &str| {
            n.ancestors()
                .filter(|x| x.is_element())
                .find_map(|x| x.attribute(a))
                .map(str::to_owned)
        };
        out.push(Mark {
            kind: kind.to_string(),
            series: up("data-series"),
            x: n.attribute("data-x").map(str::to_owned),
            classes: classes
                .iter()
                .filter(|c| colour_class(c) && !IGNORED.contains(c))
                .map(|c| c.to_string())
                .collect(),
        });
    }
    out
}

pub struct Run {
    pub key: String,
    pub chart: ChartType,
    pub plan: ColorPlan,
    pub assigned: chartgen_colours::Assignments,
    pub spec: Value,
    pub data: chartgen_model::PreparedData,
    pub golden: Vec<Mark>,
    pub report: Value,
}

/// Every drawn single-chart oracle run whose spec the rewrite's data layer prepares.
pub fn runs() -> (Vec<Run>, Vec<String>) {
    let dir = root().join("oracle/plain");
    let mut keys: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("bundled oracle outputs")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    keys.sort();
    assert!(keys.len() >= 414, "bundled oracle reports are missing");
    let mut out = vec![];
    let mut unprepared = vec![];
    for path in keys {
        let entry: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let report = &entry["report"];
        if report["status"] != "drawn" {
            continue;
        }
        let Some(chart) = report["type"].as_str().and_then(ChartType::parse) else {
            continue; // multiples, merged charts: the panels' colours (façade tests)
        };
        if report["type"] == "slope" {
            continue;
        }
        let key = entry["key"].as_str().unwrap().to_owned();
        let raw = spec_of(entry["file"].as_str().unwrap(), entry["variant"].as_str());
        let svg = std::fs::read_to_string(path.with_extension("svg")).expect("golden svg");
        match build(key.clone(), chart, raw, golden_marks(&svg), report.clone()) {
            Some(run) => out.push(run),
            None => unprepared.push(key),
        }
    }
    (out, unprepared)
}

/// The colour plan of one oracle chart; None when the rewrite's data layer cannot prepare it.
pub fn build(
    key: String,
    chart: ChartType,
    raw: Value,
    golden: Vec<Mark>,
    report: Value,
) -> Option<Run> {
    {
        let spec = Spec { raw: raw.clone() };
        let data = chartgen_data::parse(&spec)
            .and_then(|d| chartgen_data::transform(&spec, &d))
            .and_then(|d| chartgen_data::resolve_comparisons(&spec, chart, &d));
        let Ok(data) = data else {
            return None;
        };
        let mut plan = SemanticPlan {
            spec: spec.clone(),
            chart_type: chart,
            orientation: Orientation::Vertical,
            order: (0..data.categories.len()).collect(),
            layers: vec![],
            coordinates: "cartesian".into(),
            colors: ColorPlan::default(),
        };
        let assigned = chartgen_colours::compile(&mut plan, &data).expect("colour plan");
        Some(Run {
            key,
            chart,
            plan: plan.colors,
            assigned,
            spec: raw,
            data,
            golden,
            report,
        })
    }
}

/// The charts the original's own tests draw (test_breakdown, test_stacks, test_lines,
/// test_clustered, test_dots, test_multiples), recorded by running them against the original
/// (fixtures/capture_original_test_charts.py): their specs, mark colour classes and decisions.
pub fn original_test_runs() -> (Vec<Run>, Vec<String>) {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/original_test_charts.json");
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).unwrap();
    assert!(
        cases.len() >= 200,
        "recorded original test charts are missing"
    );
    let (mut out, mut unprepared) = (vec![], vec![]);
    for (n, case) in cases.iter().enumerate() {
        if case["status"] != "drawn" {
            continue;
        }
        let Some(chart) = case["type"].as_str().and_then(ChartType::parse) else {
            continue;
        };
        if case["type"] == "slope" {
            continue;
        }
        let key = format!("{}#{n}", case["test"].as_str().unwrap_or("?"));
        let golden = case["marks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| Mark {
                kind: m[0].as_str().unwrap().into(),
                series: m[1].as_str().map(str::to_owned),
                x: m[2].as_str().map(str::to_owned),
                classes: m[3]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|c| colour_class(c) && !IGNORED.contains(c))
                    .map(str::to_owned)
                    .collect(),
            })
            .collect();
        let report = json!({"decisions": case["decisions"], "findings": case["findings"]});
        match build(key.clone(), chart, case["spec"].clone(), golden, report) {
            Some(run) => out.push(run),
            None => unprepared.push(key),
        }
    }
    (out, unprepared)
}

fn set(v: &[String]) -> BTreeSet<String> {
    v.iter()
        .filter(|c| !IGNORED.contains(&c.as_str()))
        .cloned()
        .collect()
}

/// The classes the plan gives a golden mark; None when the plan has no such series or category.
pub fn predicted(run: &Run, m: &Mark) -> Option<BTreeSet<String>> {
    let p = &run.plan;
    let si = m
        .series
        .as_ref()
        .and_then(|s| run.data.series.iter().position(|x| &x.id == s));
    let ci =
        m.x.as_ref()
            .and_then(|x| run.data.categories.iter().position(|c| c == x));
    let by_category = |j: usize| {
        p.category_classes
            .get(j)
            .filter(|c| !c.is_empty())
            .map(|c| set(c))
    };
    match m.kind.as_str() {
        "target-mark" => p.series_classes.get(si?).map(|c| set(c)),
        "cell" => p.ramp.as_ref()?.cells.get(si?)?.get(ci?).map(|c| set(c)),
        "slice" | "tile" => match (ci, si) {
            (Some(j), _) if run.data.series.len() == 1 || si.is_none() => by_category(j),
            (_, Some(i)) => p.series_classes.get(i).map(|c| set(c)),
            _ => None,
        },
        "segment" => {
            // a single total's parts are its categories (bars.parts_form: data-series the part)
            if let Some(j) = m
                .series
                .as_ref()
                .and_then(|s| run.data.categories.iter().position(|c| c == s))
                .filter(|_| run.data.series.len() == 1)
            {
                return by_category(j);
            }
            if run.chart == ChartType::Waterfall {
                return by_category(ci?);
            }
            p.series_classes.get(si?).map(|c| set(c))
        }
        "bar" if run.chart == ChartType::ActionDots => p.series_classes.get(si?).map(|c| set(c)),
        "action-dot" => by_category(ci?),
        "bar" if run.chart == ChartType::Waterfall => by_category(ci?),
        "bar" if run.chart == ChartType::FunnelBars => p.series_classes.first().map(|c| set(c)),
        "bar" | "dot" => {
            let i = si?;
            if let Some(j) = ci {
                if let Some(c) = p
                    .mark_classes
                    .get(i)
                    .and_then(|r| r.get(j))
                    .filter(|c| !c.is_empty())
                {
                    return Some(set(c));
                }
                if let Some(c) = by_category(j) {
                    return Some(c);
                }
            }
            p.series_classes.get(i).map(|c| set(c))
        }
        _ => p.series_classes.get(si?).map(|c| set(c)),
    }
}

#[derive(Debug, Default)]
pub struct Tally {
    pub matched: usize,
    pub differ: Vec<String>,
    pub absent: usize,
    pub absent_keys: Vec<String>,
}

pub fn tally(run: &Run) -> Tally {
    let mut t = Tally::default();
    let mut seen = BTreeMap::new();
    for m in &run.golden {
        if m.classes.contains("uniform-context") {
            continue;
        }
        match predicted(run, m) {
            None => {
                t.absent += 1;
                // allowed: comparison values the original draws as series (transform.for_type)
                let comparison = m.series.as_ref().is_some_and(|s| {
                    run.spec["data"]["comparisons"]
                        .as_array()
                        .is_some_and(|c| c.iter().any(|c| c["id"].as_str() == Some(s)))
                });
                let derived_total = m.series.as_deref() == Some("total")
                    && run.spec["data"]["series"]
                        .as_array()
                        .is_some_and(|a| !a.iter().any(|s| s["id"] == "total"));
                if !(comparison || derived_total) {
                    t.absent_keys
                        .push(format!("{} {:?}/{:?}", m.kind, m.series, m.x));
                }
            }
            Some(p) if p == m.classes => t.matched += 1,
            Some(p) => {
                let k = format!("{} {:?}/{:?}", m.kind, m.series, m.x);
                if seen.insert(k.clone(), ()).is_none() {
                    t.differ
                        .push(format!("{k}: oracle {:?} plan {:?}", m.classes, p));
                }
            }
        }
    }
    t
}

#[test]
fn colour_classes_match_the_golden_svgs() {
    check_classes(runs(), 250);
}

#[test]
fn colour_classes_match_the_original_tests_charts() {
    check_classes(original_test_runs(), 150);
}

fn check_classes((runs, unprepared): (Vec<Run>, Vec<String>), least: usize) {
    let mut report = vec![];
    let (mut matched, mut absent) = (0, 0);
    for run in &runs {
        let t = tally(run);
        matched += t.matched;
        absent += t.absent;
        if !t.absent_keys.is_empty() {
            report.push(format!(
                "{}: no plan entry for {:?}",
                run.key, t.absent_keys
            ));
        }
        if !t.differ.is_empty() {
            report.push(format!(
                "{} ({}): {}",
                run.key,
                run.chart.as_str(),
                t.differ.join("; ")
            ));
        }
    }
    eprintln!(
        "{} runs, {} marks matched, {} marks without a plan entry, {} runs not prepared: {:?}",
        runs.len(),
        matched,
        absent,
        unprepared.len(),
        unprepared
    );
    assert!(
        runs.len() > least,
        "too few oracle runs compared: {}",
        runs.len()
    );
    assert!(
        report.is_empty(),
        "colour classes differ from the original:\n{}",
        report.join("\n")
    );
    let _ = json!(null);
}

/// The colour rules a chart type's colouring records, as the original's layouts record them.
fn owned_rules(chart: ChartType) -> &'static [&'static str] {
    use ChartType::*;
    match chart {
        Bar | Dot => &["R9-12", "R15-11"],
        Line => &["R6-28"],
        Bumps => &["R6-28"],
        Arrow => &["R3-17"],
        Pie | Donut => &["R7-32", "R7-35", "R7-17", "R7-36", "R3-8"],
        Treemap => &["R3-3", "R7-16"],
        StackedBar | StackedBar100 | StackedArea | StackedArea100 => &["R7-24", "R3-8", "R3-11"],
        Heatmap => &[
            "R3-12", "R3-13", "R3-18", "G4.29", "G4.30", "G4.31", "G4.32", "G4.33",
        ],
        _ => &[],
    }
}

fn rule_seq(items: &[Value], rules: &[&str], key: &str) -> Vec<String> {
    items
        .iter()
        .filter(|d| rules.contains(&d["rule"].as_str().unwrap_or("")))
        .filter(|d| {
            // the treemap's R7-16 naming of tiles is the layout's, not the colours'
            !(d["rule"] == "R7-16" && !d["text"].as_str().unwrap_or("").starts_with("each group"))
                && !(d["rule"] == "R3-3" && !d["text"].as_str().unwrap_or("").starts_with("one colour:"))
                // a derived deviation's own R15-11 is the transform's (transform.py)
                && !(d["rule"] == "R15-11" && !d["text"].as_str().unwrap_or("").starts_with("good and bad"))
        })
        .map(|d| format!("{}/{}/{}", d["rule"].as_str().unwrap_or(""), d["level"].as_str().unwrap_or(""), d[key].as_str().unwrap_or("")))
        .collect()
}

#[test]
fn colour_decisions_match_the_original_reports() {
    check_decisions(runs().0, 150);
}

#[test]
fn colour_decisions_match_the_original_tests_charts() {
    check_decisions(original_test_runs().0, 100);
}

fn check_decisions(runs: Vec<Run>, least: usize) {
    let mut report = vec![];
    let mut compared = 0;
    for run in &runs {
        let rules = owned_rules(run.chart);
        if rules.is_empty() {
            continue;
        }
        let oracle = rule_seq(run.report["decisions"].as_array().unwrap(), rules, "text");
        let ours = rule_seq(&run.assigned.decisions, rules, "text");
        let leading = |v: &[Value]| -> Vec<String> {
            v.iter()
                .map(|d| {
                    format!(
                        "{}/{}",
                        d["rule"].as_str().unwrap(),
                        d["level"].as_str().unwrap()
                    )
                })
                .collect()
        };
        let oracle_r24: Vec<String> = leading(
            &run.report["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|d| {
                    d["rule"] == "R3-24"
                        && d["text"].as_str().unwrap_or("").contains("colours.slots")
                })
                .cloned()
                .collect::<Vec<_>>(),
        );
        let mut ours_r24 = leading(&run.assigned.leading);
        ours_r24.extend(leading(
            &run.assigned
                .decisions
                .iter()
                .filter(|d| d["rule"] == "R3-24")
                .cloned()
                .collect::<Vec<_>>(),
        ));
        compared += 1;
        let strip = |v: &[String]| {
            v.iter()
                .map(|x| x.splitn(3, '/').take(2).collect::<Vec<_>>().join("/"))
                .collect::<Vec<_>>()
        };
        if strip(&oracle) != strip(&ours) || oracle_r24 != ours_r24 {
            report.push(format!(
                "{}: oracle {:?} {:?}\n   plan {:?} {:?}",
                run.key, oracle, oracle_r24, ours, ours_r24
            ));
        }
    }
    assert!(compared > least, "too few runs compared: {compared}");
    assert!(
        report.is_empty(),
        "colour decisions differ:\n{}",
        report.join("\n")
    );
}

fn parse_label(t: &str) -> Option<chartgen_model::Decimal> {
    let t: String = t
        .replace('\u{2212}', "-")
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    t.parse().ok()
}

/// The ramp key's bar (its swatches' classes, in order) and the values written under it, from a
/// golden heatmap's `ramp-key` group.
fn golden_key(svg: &str) -> Option<(Vec<Vec<String>>, BTreeSet<chartgen_model::Decimal>)> {
    let doc = roxmltree::Document::parse(svg).unwrap();
    let key = doc.descendants().find(|n| {
        n.attribute("class")
            .is_some_and(|c| c.split_whitespace().any(|x| x == "ramp-key"))
    })?;
    let swatches: Vec<roxmltree::Node> = key
        .children()
        .filter(|n| {
            n.attribute("class")
                .is_some_and(|c| c.contains("legend-swatch"))
        })
        .collect();
    let h = swatches.first()?.attribute("height")?;
    let bar: Vec<Vec<String>> = swatches
        .iter()
        .filter(|n| {
            n.attribute("height") == Some(h) && n.attribute("y") == swatches[0].attribute("y")
        })
        .map(|n| {
            n.attribute("class")
                .unwrap()
                .split_whitespace()
                .filter(|c| !matches!(*c, "legend-swatch" | "labelled"))
                .map(str::to_owned)
                .collect()
        })
        .collect();
    let bottom: f64 = swatches[0].attribute("y")?.parse::<f64>().ok()? + h.parse::<f64>().ok()?;
    let right: f64 = bar.len() as f64 * swatches[0].attribute("width")?.parse::<f64>().ok()?
        + swatches[0].attribute("x")?.parse::<f64>().ok()?;
    let values = key
        .children()
        .filter(|n| n.tag_name().name() == "text")
        .filter(|n| {
            n.attribute("y")
                .and_then(|y| y.parse::<f64>().ok())
                .is_some_and(|y| y > bottom)
                && n.attribute("x")
                    .and_then(|x| x.parse::<f64>().ok())
                    .is_some_and(|x| x <= right + 0.5)
        })
        .filter_map(|n| n.text().and_then(parse_label))
        .collect();
    Some((bar, values))
}

#[test]
fn ramp_keys_match_the_golden_heatmaps() {
    let (runs, _) = runs();
    let mut compared = 0;
    let mut report = vec![];
    for run in runs.iter().filter(|r| r.chart == ChartType::Heatmap) {
        let svg =
            std::fs::read_to_string(root().join("oracle/plain").join(format!("{}.svg", run.key)))
                .unwrap();
        let Some((bar, values)) = golden_key(&svg) else {
            continue; // every value printed in its cell: no key (R4-9)
        };
        compared += 1;
        let ramp = run.plan.ramp.as_ref().expect("heatmap ramp plan");
        let ours: Vec<Vec<String>> = ramp
            .key
            .iter()
            .map(|e| {
                e.classes
                    .iter()
                    .filter(|c| *c != "labelled")
                    .cloned()
                    .collect()
            })
            .collect();
        let ours_values: BTreeSet<chartgen_model::Decimal> = ramp
            .key
            .iter()
            .flat_map(|e| [e.lo, e.hi, e.single, e.censored.as_ref().map(|c| c.1)])
            .flatten()
            .map(|v| v.normalize())
            .collect();
        let values: BTreeSet<_> = values.into_iter().map(|v| v.normalize()).collect();
        if bar != ours || values != ours_values {
            report.push(format!(
                "{}: oracle {:?} {:?}\n   plan {:?} {:?}",
                run.key, bar, values, ours, ours_values
            ));
        }
    }
    assert!(compared >= 8, "too few ramp keys compared: {compared}");
    assert!(
        report.is_empty(),
        "ramp keys differ:\n{}",
        report.join("\n")
    );
}

/// Small multiples and merged charts: each series inside the panels its own slot, the same in
/// every panel; slots fixed by name or registry kept (multiples.py, R6-28, R3-24, G7.50).
#[test]
fn panel_colours_match_the_golden_multiples() {
    let dir = root().join("oracle/plain");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    let (mut compared, mut matched) = (0, 0);
    let mut report = vec![];
    for path in paths {
        let entry: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let r = &entry["report"];
        let Some(kind) = r["type"].as_str().and_then(|t| {
            t.strip_prefix("multiples of ")
                .or_else(|| t.strip_prefix("merged of "))
        }) else {
            continue;
        };
        if r["status"] != "drawn" {
            continue;
        }
        let chart = ChartType::parse(kind).unwrap();
        let key = entry["key"].as_str().unwrap();
        let raw = spec_of(entry["file"].as_str().unwrap(), entry["variant"].as_str());
        let spec = Spec { raw: raw.clone() };
        let data = chartgen_data::parse(&spec)
            .and_then(|d| chartgen_data::transform(&spec, &d))
            .unwrap();
        let facts = chartgen_data::inspect(&data);
        let decision = chartgen_policy::decide(&spec, &data, &facts).unwrap();
        let panels = match chartgen_panels::prepare(&spec, &data, &decision) {
            Ok(p) => p,
            Err(e) => {
                report.push(format!("{key}: panels not prepared: {e}"));
                continue;
            }
        };
        let refs: Vec<chartgen_colours::PanelRef> = panels
            .iter()
            .map(|p| chartgen_colours::PanelRef {
                series: &p.series_indices,
                row: p.row,
                column: p.column,
            })
            .collect();
        let within = panels[0].arrangement.within.clone();
        let grid =
            chartgen_colours::panels_plan(&spec, &data, &within, panels[0].kind == "merged", &refs)
                .unwrap();
        // the series' classes, each panel planned on its own series with the grid's slots
        let mut classes = BTreeMap::<String, BTreeSet<String>>::new();
        for (panel, fixed) in panels.iter().zip(&grid.fixed) {
            let mut plan = SemanticPlan {
                spec: panel.spec.clone(),
                chart_type: chart,
                orientation: Orientation::Vertical,
                order: (0..panel.data.categories.len()).collect(),
                layers: vec![],
                coordinates: "cartesian".into(),
                colors: ColorPlan::default(),
            };
            chartgen_colours::compile_with(&mut plan, &panel.data, Some(fixed)).unwrap();
            for (k, s) in panel.data.series.iter().enumerate() {
                classes.insert(s.id.clone(), set(&plan.colors.series_classes[k]));
            }
        }
        let svg = std::fs::read_to_string(path.with_extension("svg")).unwrap();
        compared += 1;
        for m in golden_marks(&svg) {
            if !matches!(m.kind.as_str(), "line" | "bar" | "dot" | "projection") {
                continue;
            }
            let Some(p) = m.series.as_ref().and_then(|s| classes.get(s)) else {
                continue;
            };
            if *p == m.classes {
                matched += 1;
            } else {
                report.push(format!(
                    "{key}: {} {:?}/{:?} oracle {:?} plan {:?}",
                    m.kind, m.series, m.x, m.classes, p
                ));
            }
        }
        let ours: Vec<String> = grid
            .decisions
            .iter()
            .map(|d| {
                format!(
                    "{}/{}",
                    d["rule"].as_str().unwrap(),
                    d["level"].as_str().unwrap()
                )
            })
            .collect();
        let oracle: Vec<String> = r["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| {
                (d["rule"] == "R6-28" && d["text"].as_str().unwrap_or("").contains("every panel"))
                    || (d["rule"] == "R3-3"
                        && d["text"]
                            .as_str()
                            .unwrap_or("")
                            .starts_with("one colour for every mark"))
            })
            .map(|d| {
                format!(
                    "{}/{}",
                    d["rule"].as_str().unwrap(),
                    d["level"].as_str().unwrap()
                )
            })
            .collect();
        if ours != oracle {
            report.push(format!("{key}: decisions oracle {oracle:?} plan {ours:?}"));
        }
    }
    eprintln!("{compared} panel charts, {matched} marks matched");
    assert!(compared >= 15, "too few panel charts compared: {compared}");
    assert!(
        report.is_empty(),
        "panel colours differ:\n{}",
        report.join("\n")
    );
}

/// annotate.cvd: every drawn chart with good or bad marks records how its output treats them
/// (R3-20, and R15-12 in black and white), under the plain style's semantic mode.
#[test]
fn good_bad_output_decisions_match_the_original() {
    let (runs, _) = runs();
    let mut bad = vec![];
    let mut with = 0;
    for run in &runs {
        let spec = chartgen_model::Spec {
            raw: run.spec.clone(),
        };
        let out = chartgen_colours::good_bad_output(&run.plan, &spec, None);
        let ours: Vec<String> = out
            .decisions
            .iter()
            .map(|d| {
                format!(
                    "{}/{}",
                    d["rule"].as_str().unwrap(),
                    d["level"].as_str().unwrap()
                )
            })
            .collect();
        let oracle: Vec<String> = run.report["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| {
                d["rule"] == "R3-20"
                    || (d["rule"] == "R15-12"
                        && d["text"]
                            .as_str()
                            .unwrap_or("")
                            .starts_with("black and white"))
            })
            .map(|d| {
                format!(
                    "{}/{}",
                    d["rule"].as_str().unwrap(),
                    d["level"].as_str().unwrap()
                )
            })
            .collect();
        with += usize::from(!oracle.is_empty());
        if ours != oracle {
            bad.push(format!("{}: oracle {oracle:?} plan {ours:?}", run.key));
        }
        assert!(!out.copy, "the plain style writes no copies");
    }
    assert!(with >= 25, "too few good/bad charts: {with}");
    assert!(
        bad.is_empty(),
        "good/bad output differs:\n{}",
        bad.join("\n")
    );
}
