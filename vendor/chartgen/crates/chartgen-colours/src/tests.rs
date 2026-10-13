//! Unit tests with values from the original (figkit style.py, okhsl.py, svglint.py, its tests).
use super::colour::*;
use super::*;
use chartgen_model::Orientation;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn colour_functions_match_the_original() {
    // test_style.Flatten.test_colour_functions
    assert_eq!(mix("#000000", "#ffffff", 0.5).unwrap(), "#808080");
    // values computed by the original's figkit (oracle tools/figkit/style.py)
    let l = oklch("#3f6c96").unwrap();
    assert!(close(l[0], 0.5175754105191265, 1e-12) && close(l[1], 0.08344776132474727, 1e-12));
    assert!(close(l[2], 248.1501713274728, 1e-9));
    assert_eq!(oklch_shift("#3f6c96", -0.12, 0.).unwrap(), "#1d4a72");
    assert_eq!(oklch_shift("#95b7d3", -0.12, 0.).unwrap(), "#7192ad");
    assert_eq!(oklch_shift("#dd8045", -0.12, 0.).unwrap(), "#b55b1a");
    assert_eq!(oklch_shift("#c5392a", -0.12, 0.).unwrap(), "#980d03");
    assert_eq!(oklch_shift("#f3f3f3", -0.12, 0.).unwrap(), "#cccccc");
    assert_eq!(oklch_light("#3f6c96", 0.9, Some(0.05)).unwrap(), "#c5e2fe");
    assert_eq!(oklch_light("#c5392a", 0.9, Some(0.05)).unwrap(), "#fed2cb");
    assert_eq!(from_oklch(0.7, 0.3, 140.), "#3ebd11");
    assert!(close(
        contrast("#3f6c96", "#ffffff").unwrap(),
        5.531004285667715,
        1e-12
    ));
    assert!(close(
        contrast("#dd8045", "#ffffff").unwrap(),
        2.897337710030366,
        1e-12
    ));
    assert_eq!(
        text_tone(&["#95b7d3"], "#000000", "#ffffff").map(|t| t.0),
        Some("dark")
    );
    assert_eq!(
        text_tone(&["#c5392a"], "#000000", "#ffffff").map(|t| t.0),
        Some("light")
    );
    // test_style.Lint.test_plain_passes
    assert!(contrast("#595959", "#ffffff").unwrap() > 7.);
}

#[test]
fn okhsl_matches_the_original() {
    // oracle tools/okhsl.py
    assert_eq!(okhsl(250., 60., 50.), "#437bb2");
    assert_eq!(okhsl(30., 90., 70.), "#f98574");
    let h = srgb_to_okhsl(0x3f as f64 / 255., 0x6c as f64 / 255., 0x96 as f64 / 255.);
    assert!(close(h[0], 248.1501713274728, 1e-9));
    assert!(close(h[1], 51.391591220287935, 1e-9));
    assert!(close(h[2], 44.12204278374768, 1e-9));
}

#[test]
fn cvd_calibration_of_the_original() {
    // test_svglint.Pieces.test_cvd_calibration (notes 03-04 §5.7)
    let m = |name: &str| CVD.iter().find(|(n, _)| *n == name).unwrap().1;
    assert!(close(
        cvd_distance("#A4AB81", "#7CA79D", &m("protanopia")).unwrap(),
        0.051,
        5e-4
    ));
    assert!(close(
        cvd_distance("#DD8045", "#B98E2F", &m("deuteranopia")).unwrap(),
        0.023,
        5e-4
    ));
    let worst = CVD
        .iter()
        .map(|(_, mm)| cvd_distance("#2E5D89", "#AB4E2D", mm).unwrap())
        .fold(f64::INFINITY, f64::min);
    assert!(worst >= 0.147);
}

fn plan(chart: ChartType, raw: serde_json::Value) -> (SemanticPlan, PreparedData) {
    let spec = Spec { raw };
    let data = chartgen_data::parse(&spec).unwrap();
    (
        SemanticPlan {
            spec,
            chart_type: chart,
            orientation: Orientation::Vertical,
            order: (0..data.categories.len()).collect(),
            layers: vec![],
            coordinates: "cartesian".into(),
            colors: ColorPlan::default(),
        },
        data,
    )
}

#[test]
fn registry_names_own_slots_and_collisions() {
    // __init__.colour_slots: a named slot, a series' own slot kept (info), a collision freed (warn);
    // the classes are the original's for this spec (rendered by the oracle: a series-6, b series-1,
    // c series-3; findings R3-24 info and warn)
    let (mut p, data) = plan(
        ChartType::Line,
        json!({"chartspec":1,"id":"c","data":{"x":{"kind":"time","grid":"month","values":["2026-01","2026-02","2026-03"]},"series":[{"id":"a","name":"Alpha","values":[1,2,3]},{"id":"b","name":"Beta","slot":6,"values":[3,2,1]},{"id":"c","name":"Gamma","values":[2,2,2]}]},"colours":{"slots":{"Beta":4,"Alpha":6,"Gamma":3,"Omega":1}}}),
    );
    let a = compile(&mut p, &data).unwrap();
    assert!(
        a.findings
            .iter()
            .any(|f| f.rule == "R3-24" && f.severity == "info")
    );
    assert!(
        a.findings
            .iter()
            .any(|f| f.rule == "R3-24" && f.severity == "warn")
    );
    // Alpha takes 6 by name, Beta keeps its own 6: the later one, Beta, loses it and takes the
    // first free slot by the line ends (Alpha ends at 3, Gamma at 2, Beta at 1)
    assert_eq!(p.colors.series_classes[0], vec!["series-6"]);
    assert_eq!(p.colors.series_classes[2], vec!["series-3"]);
    assert_eq!(p.colors.series_classes[1], vec!["series-1"]);
    assert_eq!(a.leading.len(), 1);
}

#[test]
fn more_parts_than_slots_take_the_last_slot_instead_of_failing() {
    // pie.colours: f"series-{min(next(free, 16), 16)}"; the oracle draws p15…p19 in series-16 and
    // reports R7-32 info (more than eight colours)
    let values: Vec<String> = (0..20).map(|k| format!("p{k}")).collect();
    let numbers: Vec<i64> = (0..20).map(|k| 40 - k).collect();
    let (mut p, data) = plan(
        ChartType::Pie,
        json!({"chartspec":1,"id":"many","data":{"x":{"kind":"nominal","values":values},"parts":{"meaningful_total":true,"all_parts":true,"overlap":false},"series":[{"id":"s","values":numbers}]}}),
    );
    let a = compile(&mut p, &data).unwrap();
    assert_eq!(p.colors.category_classes[19], vec!["series-16"]);
    assert_eq!(p.colors.category_classes[15], vec!["series-16"]);
    assert!(
        a.findings
            .iter()
            .any(|f| f.rule == "R7-32" && f.severity == "info")
    );
}
