//! The statistical charts through the whole pipeline: data, policy, scales, colours, layout with
//! host-measured text (a stand-in measurer here), scene, lint and SVG.
use chartgen::model::*;
use chartgen::{RenderRequest, RenderStep, render_step};
use serde_json::{Value, json};

use std::collections::BTreeMap;
use std::path::PathBuf;

fn load(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/statistical")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p:?}: {e}")))
        .unwrap()
}

/// Run the render loop, answering each round of text requests with a fixed-width model.
fn run(spec: Value, width: f64) -> RenderStep {
    let mut request = RenderRequest {
        spec,
        theme: Theme::default(),
        constraints: LayoutConstraints {
            width,
            height: None,
            measurement_context: "test".into(),
            max_rounds: 12,
            hooks: None,
        },
        measurements: Measurements {
            context: "test".into(),
            entries: BTreeMap::new(),
        },
        backend: "svg".into(),
        variant: None,
        expect: Value::Null,
        source_name: None,
    };
    for _ in 0..24 {
        match render_step(&request)
            .unwrap_or_else(|e| panic!("render step: {e:?} for {}", request.spec["id"]))
        {
            RenderStep::NeedMeasurements { requests, .. } => {
                for r in requests {
                    let chars = r.content.readable().chars().count() as f64;
                    request.measurements.entries.insert(
                        r.id,
                        TextMetrics {
                            width: chars * r.font_size * 0.52,
                            height: r.font_size * 1.2,
                            baseline: Some(r.font_size),
                        },
                    );
                }
            }
            done => return done,
        }
    }
    panic!("the render loop did not converge");
}

fn report(step: &RenderStep) -> &Report {
    match step {
        RenderStep::Complete { report, .. }
        | RenderStep::Stopped { report }
        | RenderStep::NeedMeasurements { report, .. } => report,
    }
}

fn drawn(name: &str) -> (String, Report) {
    let step = run(load(name), 560.);
    match step {
        RenderStep::Complete { output, report, .. } => {
            if let Ok(dir) = std::env::var("CHARTGEN_STATS_OUT") {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(format!("{dir}/{name}.svg"), &output).unwrap();
            }
            (output, report)
        }
        other => panic!("{name} was not drawn: {:#?}", report(&other).findings),
    }
}

const TYPES: [&str; 10] = [
    "box",
    "strip",
    "ecdf",
    "scatter",
    "dumbbell",
    "dumbbell_difference",
    "interval_dot",
    "interval_bar",
    "state_matrix",
    "contingency",
];

#[test]
fn every_statistical_chart_draws_through_the_main_pipeline() {
    for n in TYPES {
        let (svg, report) = drawn(n);
        assert!(svg.starts_with("<svg") || svg.contains("<svg"), "{n}");
        assert_eq!(report.status, "drawn", "{n}");
        assert!(
            !svg.contains("NaN") && !svg.contains("inf"),
            "{n}: non-finite coordinate"
        );
        let errors: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.severity == "error")
            .collect();
        assert!(errors.is_empty(), "{n}: {errors:#?}");
    }
}

#[test]
#[ignore = "prints the reports"]
fn print_reports() {
    for n in TYPES {
        let (_, r) = drawn(n);
        println!("== {n} ({})", r.chart_type.clone().unwrap_or_default());
        for f in &r.findings {
            println!(
                "  finding {} {} {:?}: {}",
                f.rule, f.severity, f.path, f.message
            );
        }
        for d in &r.decisions {
            println!("  decision {} [{}]: {}", d["rule"], d["level"], d["text"]);
        }
    }
}

/// The text a reader sees in the SVG: wrapped notes span several `<text>` elements.
fn visible(svg: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in svg.replace("</text>", " ").chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_with(spec: Value) -> RenderStep {
    run(spec, 560.)
}

fn complete(step: RenderStep) -> (String, Report, Scene) {
    match step {
        RenderStep::Complete {
            output,
            report,
            scene,
        } => (output, report, scene),
        other => panic!("not drawn: {:#?}", report(&other).findings),
    }
}

fn summary(r: &Report, key: &str) -> Value {
    r.summary.get(key).cloned().unwrap_or(Value::Null)
}

// ---- refusals ----------------------------------------------------------------------------

fn assert_refused(name: &str, rule: &str) {
    let step = run(load(&format!("refuse_{name}")), 560.);
    let RenderStep::Stopped { report } = &step else {
        panic!("{name} was not stopped")
    };
    assert!(
        report.status == "refused" || report.status == "invalid",
        "{name}: {}",
        report.status
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == rule && f.severity == "error"),
        "{name}: expected {rule}, got {:#?}",
        report
            .findings
            .iter()
            .map(|f| (&f.rule, &f.severity, &f.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn refusals_name_their_rule_and_what_to_supply() {
    for (name, rule) in [
        ("box_n_hidden", "XS-BOX-02"),
        ("box_notch", "XS-BOX-06"),
        ("box_small_n", "XS-BOX-03"),
        ("box_summary_no_whisker_rule", "XS-BOX-08"),
        ("box_summary_no_outliers", "XS-BOX-10"),
        ("interval_unlabelled", "XS-IV-01"),
        ("interval_no_method", "XS-IV-02"),
        ("interval_bound_no_basis", "XS-IV-03"),
        ("interval_bar_no_zero", "XS-IV-04"),
        ("dumbbell_three", "XS-DB-01"),
        ("scatter_size", "XS-SC-01"),
        ("contingency_overlap", "XS-CT-01"),
        ("matrix_blank", "XS-MX-01"),
        ("no_job", "R0-07"),
    ] {
        assert_refused(name, rule);
    }
}

#[test]
fn a_refusal_carries_its_supply_and_a_path() {
    let RenderStep::Stopped { report } = run(load("refuse_interval_bound_no_basis"), 560.) else {
        panic!()
    };
    let f = report
        .findings
        .iter()
        .find(|f| f.rule == "XS-IV-03")
        .unwrap();
    assert!(f.supply.as_deref().is_some_and(|s| s.contains("basis")));
    assert_eq!(f.path.as_deref(), Some("chart.interval.basis"));
    assert_eq!(
        f.level, "guided",
        "proposed rules are guided until the owner rules"
    );
}

#[test]
fn unpaired_conditions_and_missing_data_are_refused_not_imputed() {
    let mut spec = load("dumbbell");
    spec["data"]["series"][1]["values"] = json!([60, 10]);
    let RenderStep::Stopped { report } = render_with(spec) else {
        panic!()
    };
    assert!(report.findings.iter().any(|f| f.rule == "XS-DB-02"));
    let mut spec = load("strip");
    spec["data"]["series"][0]["samples"][0][3] = Value::Null;
    // the schema stops a null among the samples before any statistic could impute it
    let RenderStep::Stopped { report } = render_with(spec) else {
        panic!()
    };
    assert!(
        report.findings.iter().any(|f| f.severity == "error"
            && f.path
                .as_deref()
                .is_some_and(|p| p.starts_with("data.series[0].samples"))),
        "{:?}",
        report.findings
    );
}

// ---- what is drawn, and what was computed -----------------------------------------------

#[test]
fn box_statistics_are_computed_and_reported() {
    let (svg, report, _) = complete(render_with(load("box")));
    let boxes = summary(&report, "statistics")["boxes"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(boxes.len(), 8);
    assert!(boxes.iter().all(|b| b["n"] == 30));
    // an independent computation of the first box's quartiles
    let spec = load("box");
    let mut v: Vec<f64> = spec["data"]["series"][0]["samples"][0]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| {
        let h = p * (v.len() - 1) as f64;
        let (lo, f) = (h.floor() as usize, h - h.floor());
        v[lo] + f * (v[(lo + 1).min(v.len() - 1)] - v[lo])
    };
    assert!((boxes[0]["median"].as_f64().unwrap() - q(0.5)).abs() < 1e-9);
    assert!((boxes[0]["q1"].as_f64().unwrap() - q(0.25)).abs() < 1e-9);
    assert!((boxes[0]["q3"].as_f64().unwrap() - q(0.75)).abs() < 1e-9);
    let text = visible(&svg);
    assert!(
        text.contains("n=30") && text.contains("1.5\u{d7}IQR"),
        "{text}"
    );
}

#[test]
fn defaulted_choices_are_recorded_and_drawn() {
    let mut spec = load("box");
    spec["chart"].as_object_mut().unwrap().remove("whiskers");
    spec["chart"].as_object_mut().unwrap().remove("quantiles");
    let (svg, report, _) = complete(render_with(spec));
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == "XS-BOX-01" && f.severity == "info")
    );
    let defaults: Vec<_> = report
        .decisions
        .iter()
        .filter(|d| d["rule"] == "XS-DEFAULT")
        .collect();
    assert_eq!(defaults.len(), 2, "{defaults:?}");
    assert!(visible(&svg).contains("linear quantiles"));
}

#[test]
fn the_value_axis_follows_the_librarys_scale_rules() {
    // bars start at zero (R2-24); a box's scale is trained like a dot's, from what is drawn
    let (_, bar, _) = complete(render_with(load("interval_bar")));
    assert!(bar.decisions.iter().any(|d| d["rule"] == "R2-24"));
    let (_, boxed, _) = complete(render_with(load("box")));
    assert!(boxed.decisions.iter().any(|d| d["rule"] == "02 D5"));
    // percentages stop at their theoretical maximum
    let (_, pct, _) = complete(render_with(load("interval_dot")));
    assert!(pct.decisions.iter().any(|d| {
        d["text"]
            .as_str()
            .is_some_and(|t| t.contains("theoretical maximum"))
    }));
}

#[test]
fn the_wilson_interval_is_exact_for_counts() {
    let (svg, report, _) = complete(render_with(load("interval_dot")));
    let est = summary(&report, "statistics")["estimates"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(est[0]["size"], "29/30");
    let (lo, hi) = (
        est[2]["lo"].as_f64().unwrap(),
        est[2]["hi"].as_f64().unwrap(),
    );
    // 18 of 30 at 95 %: Wilson interval 42.3 % to 75.4 %
    assert!(
        (lo - 42.3).abs() < 0.1 && (hi - 75.4).abs() < 0.1,
        "{lo} {hi}"
    );
    assert!(visible(&svg).contains("Wilson score"));
}

#[test]
fn a_bound_is_never_called_a_confidence_interval() {
    let (svg, _, _) = complete(render_with(load("dumbbell_difference")));
    let text = visible(&svg);
    assert!(text.contains("not confidence intervals"));
    assert!(!text.contains("confidence interval ("));
}

#[test]
fn small_samples_warn_and_bimodal_ones_get_an_advisory() {
    let mut spec = load("box");
    spec["data"]["series"][0]["samples"][0] = json!([5.0, 6.0, 7.0, 8.0, 9.0, 12.0]);
    let (_, report, _) = complete(render_with(spec));
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == "XS-BOX-04" && f.severity == "warn")
    );
    let mut spec = load("box");
    let mut v: Vec<f64> = (0..20).map(|i| 1.0 + (i % 5) as f64 * 0.1).collect();
    v.extend((0..20).map(|i| 10.0 + (i % 5) as f64 * 0.1));
    spec["data"]["series"][0]["samples"][0] = json!(v);
    let (_, report, _) = complete(render_with(spec));
    assert!(report.findings.iter().any(|f| f.rule == "XS-BOX-07"));
}

#[test]
fn an_ecdf_states_its_requested_quantiles() {
    let (_, report, _) = complete(render_with(load("ecdf")));
    let curves = summary(&report, "statistics")["curves"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(curves.len(), 4);
    assert_eq!(curves[0]["quantiles"].as_array().unwrap().len(), 2);
    let mut spec = load("ecdf");
    let g = spec["data"]["series"][0]["samples"][0].clone();
    spec["data"]["x"]["values"] = json!(["a", "b", "c", "d", "e"]);
    for s in spec["data"]["series"].as_array_mut().unwrap() {
        s["samples"] = json!([g, g, g, g, g]);
    }
    let RenderStep::Stopped { report } = render_with(spec) else {
        panic!()
    };
    assert!(report.findings.iter().any(|f| f.rule == "XS-ECDF-01"));
}

// ---- routing, order and orientation -------------------------------------------------------

fn routed(mut spec: Value) -> String {
    spec["chart"]["type"] = json!("auto");
    let (_, report, _) = complete(render_with(spec));
    report.chart_type.unwrap()
}

#[test]
fn auto_routes_the_data_to_a_statistical_chart() {
    assert_eq!(routed(load("box")), "box", "samples are a distribution");
    let mut small = load("box");
    for s in small["data"]["series"].as_array_mut().unwrap() {
        s["samples"] = json!([
            [1.0, 2.0, 3.0],
            [2.0, 3.0, 4.0],
            [3.0, 4.0, 5.0],
            [1.0, 5.0, 9.0]
        ]);
    }
    assert_eq!(
        routed(small),
        "strip",
        "too few for quartiles: every value is drawn"
    );
    let mut shape = load("box");
    shape["facts"] = json!({"message": "distribution_shape"});
    shape["data"]["x"]["values"] = json!(["W1 stale read", "W2 equal-valued write"]);
    for s in shape["data"]["series"].as_array_mut().unwrap() {
        s["samples"] = json!([s["samples"][0], s["samples"][1]]);
    }
    assert_eq!(routed(shape), "ecdf");
    assert_eq!(routed(load("scatter")), "scatter");
    assert_eq!(routed(load("interval_dot")), "interval_dot");
    assert_eq!(routed(load("state_matrix")), "state_matrix");
    assert_eq!(routed(load("contingency")), "contingency");
    let mut paired = load("dumbbell");
    paired["facts"] = json!({"message": "paired_change"});
    assert_eq!(routed(paired), "dumbbell");
}

#[test]
fn an_explicit_type_records_the_route_without_a_departure_warning() {
    let (_, report, _) = complete(render_with(load("box")));
    assert!(
        report.path.iter().any(|d| d["rule"] == "XS-ROUTE"),
        "{:?}",
        report.path
    );
    assert!(
        !report.findings.iter().any(|f| f.severity == "warn"),
        "{:?}",
        report.findings
    );
}

#[test]
fn row_charts_keep_the_given_order_and_dumbbells_sort_by_the_difference() {
    let (_, report, _) = complete(render_with(load("dumbbell")));
    let order = summary(&report, "order");
    assert_eq!(order[0], "Candidate correspondence", "the given order");
    let mut spec = load("dumbbell");
    spec["chart"]["sort"] = json!("desc");
    spec["chart"]["sort_by"] = json!("difference");
    let (_, report, _) = complete(render_with(spec));
    // U minus F: the most negative difference first under desc? desc puts the largest first
    let order = summary(&report, "order");
    assert_eq!(
        order[0], "Retained model",
        "the smallest drop is the largest difference (U - F = -7)"
    );
    let mut spec = load("dumbbell");
    spec["chart"]["sort"] = json!("asc");
    spec["chart"]["sort_by"] = json!("difference");
    let (_, report, _) = complete(render_with(spec));
    assert_eq!(summary(&report, "order")[0], "Equal-valued writes");
}

#[test]
fn vertical_orientation_draws_the_row_charts() {
    for n in [
        "box",
        "strip",
        "interval_dot",
        "interval_bar",
        "dumbbell",
        "dumbbell_difference",
    ] {
        let mut spec = load(n);
        spec["chart"]["orientation"] = json!("vertical");
        spec["data"]["x"]["values"] = spec["data"]["x"]["values"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, _)| json!(format!("C{}", i + 1)))
            .collect();
        let step = run(spec, 640.);
        let (svg, report, _) = complete(step);
        assert_eq!(summary(&report, "orientation"), "vertical", "{n}");
        assert!(!svg.contains("NaN"), "{n}");
        let errors: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.severity == "error")
            .collect();
        assert!(errors.is_empty(), "{n}: {errors:#?}");
    }
}

#[test]
fn features_the_family_does_not_place_are_refused_not_dropped() {
    let mut spec = load("box");
    spec["highlight"] = json!([{"targets": [{"series": "r256"}], "method": "strong"}]);
    let err = render_step(&RenderRequest {
        spec,
        theme: Theme::default(),
        constraints: LayoutConstraints {
            width: 560.,
            height: None,
            measurement_context: "t".into(),
            max_rounds: 8,
            hooks: None,
        },
        measurements: Measurements {
            context: "t".into(),
            entries: BTreeMap::new(),
        },
        backend: "svg".into(),
        variant: None,
        expect: Value::Null,
        source_name: None,
    });
    // the first round only asks for measurements; run to the layout with them
    let _ = err;
    let mut spec = load("box");
    spec["highlight"] = json!([{"targets": [{"series": "r256"}], "method": "strong"}]);
    let mut request = RenderRequest {
        spec,
        theme: Theme::default(),
        constraints: LayoutConstraints {
            width: 560.,
            height: None,
            measurement_context: "t".into(),
            max_rounds: 8,
            hooks: None,
        },
        measurements: Measurements {
            context: "t".into(),
            entries: BTreeMap::new(),
        },
        backend: "svg".into(),
        variant: None,
        expect: Value::Null,
        source_name: None,
    };
    let mut failed = None;
    for _ in 0..8 {
        match render_step(&request) {
            Ok(RenderStep::NeedMeasurements { requests, .. }) => {
                for r in requests {
                    request.measurements.entries.insert(
                        r.id,
                        TextMetrics {
                            width: 40.,
                            height: 10.,
                            baseline: Some(8.),
                        },
                    );
                }
            }
            Ok(_) => break,
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    assert!(
        failed.is_some_and(|e| format!("{e:?}").contains("highlight")),
        "a highlight must be refused, not silently dropped"
    );
}

#[test]
fn rendering_is_deterministic() {
    for n in ["box", "strip", "ecdf", "interval_bar", "scatter"] {
        let a = complete(render_with(load(n))).0;
        let b = complete(render_with(load(n))).0;
        assert_eq!(a, b, "{n}");
    }
}

#[test]
fn states_are_told_apart_by_shape_as_well_as_colour() {
    let (_, _, scene) = complete(render_with(load("state_matrix")));
    let mut hrefs = std::collections::BTreeSet::new();
    fn walk(p: &Primitive, out: &mut std::collections::BTreeSet<String>) {
        match p {
            Primitive::Use { href, meta, .. }
                if meta.classes.first().is_some_and(|c| c == "state") =>
            {
                out.insert(href.clone());
            }
            Primitive::Group { children, .. } => children.iter().for_each(|c| walk(c, out)),
            _ => {}
        }
    }
    scene.primitives.iter().for_each(|p| walk(p, &mut hrefs));
    assert!(hrefs.len() >= 3, "{hrefs:?}");
}

#[test]
fn ecdf_dashes_tell_groups_apart_without_colour() {
    // four curves: two series by colour, two groups by dash
    let (svg, _, _) = complete(render_with(load("ecdf")));
    assert!(visible(&svg).contains("W2 equal-valued write \u{b7} 256 records/s"));
}

/// Writes every fixture's report (drawn or refused) as JSON, for the test document. Run with
/// `CHARTGEN_STATS_OUT=dir cargo test -p chartgen --test statistical write_reports -- --ignored`.
#[test]
#[ignore = "writes files"]
fn write_reports() {
    let dir = std::env::var("CHARTGEN_STATS_OUT").expect("set CHARTGEN_STATS_OUT");
    std::fs::create_dir_all(&dir).unwrap();
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/statistical");
    let mut names: Vec<String> = std::fs::read_dir(tests)
        .unwrap()
        .filter_map(|e| {
            e.ok()?
                .path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    for n in names {
        let step = run(load(&n), 560.);
        let r = report(&step);
        std::fs::write(
            format!("{dir}/{n}.report.json"),
            serde_json::to_string_pretty(r).unwrap(),
        )
        .unwrap();
    }
}
