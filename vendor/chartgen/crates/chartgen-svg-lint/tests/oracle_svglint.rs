//! The SVG lint against the original's lint_svg (tests/fixtures/generate_svglint.py): every
//! golden source SVG of the oracle runs, and every SVG the original's test_svglint lints (its
//! hand-made fixtures of the book's charts and faulty copies of generated ones), each with the
//! text extents the original measured with Typst. Findings compared whole: rule, severity,
//! level, message and elements, in order.
use chartgen_svg_lint::{Extent, Options, lint};
use serde_json::Value;
use std::path::Path;

fn fixture() -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/svglint_cases.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("bundled fixture")).unwrap()
}

fn extents(v: &Value) -> Vec<Option<Extent>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| {
            e.as_array().map(|a| Extent {
                width: a[0].as_f64().unwrap(),
                ascent: a[1].as_f64().unwrap(),
                descent: a[2].as_f64().unwrap(),
            })
        })
        .collect()
}

fn view(findings: &[chartgen_model::Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|f| {
            format!(
                "{} {} {} | {} | {:?}",
                f.rule, f.severity, f.level, f.message, f.elements
            )
        })
        .collect()
}

fn oracle(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let els: Vec<String> = f["elements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e.as_str().unwrap().to_owned())
                .collect();
            format!(
                "{} {} {} | {} | {:?}",
                f["rule"].as_str().unwrap(),
                f["severity"].as_str().unwrap(),
                f["level"].as_str().unwrap(),
                f["message"].as_str().unwrap(),
                els
            )
        })
        .collect()
}

fn run(svg: &str, case: &Value, css: &str) -> Vec<String> {
    let opts = Options {
        css: Some(css.into()),
        extents: Some(extents(&case["extents"])),
        generated: case["generated"].as_bool().unwrap_or(false),
        final_width: case["final_width"].as_f64(),
        cvd: case["cvd"].as_str().unwrap_or("assume").into(),
        date_format: case["date_format"].as_str().map(str::to_owned),
    };
    view(&lint(svg, &opts).expect("lint"))
}

#[test]
fn golden_svgs_lint_as_the_original() {
    let fx = fixture();
    let css = fx["style_css"].as_str().unwrap();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance/oracle/plain");
    let cases = fx["golden"].as_array().unwrap();
    assert!(cases.len() >= 350);
    let mut bad = vec![];
    for case in cases {
        let key = case["key"].as_str().unwrap();
        let svg = std::fs::read_to_string(golden.join(format!("{key}.svg"))).expect("golden svg");
        let (ours, want) = (run(&svg, case, css), oracle(&case["findings"]));
        if ours != want {
            bad.push(format!("{key}:\n  oracle {want:#?}\n  ours   {ours:#?}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} differ:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

#[test]
fn the_original_tests_svgs_lint_as_the_original() {
    let fx = fixture();
    let css = fx["style_css"].as_str().unwrap();
    let cases = fx["tests"].as_array().unwrap();
    assert!(cases.len() >= 50);
    let mut bad = vec![];
    for case in cases {
        let (ours, want) = (
            run(case["svg"].as_str().unwrap(), case, css),
            oracle(&case["findings"]),
        );
        if ours != want {
            bad.push(format!(
                "{}:\n  oracle {want:#?}\n  ours   {ours:#?}",
                case["source"]
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} differ:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

#[test]
fn without_extents_the_lint_says_it_is_partial() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><text class="prose title" x="0" y="10">A</text></svg>"#;
    let f = lint(svg, &Options::default()).unwrap();
    assert!(f.iter().any(|f| f.rule == "CG-SVG-PARTIAL"));
    let f = lint(
        svg,
        &Options {
            extents: Some(vec![Some(Extent {
                width: 5.,
                ascent: 7.,
                descent: 2.,
            })]),
            ..Options::default()
        },
    )
    .unwrap();
    assert!(!f.iter().any(|f| f.rule == "CG-SVG-PARTIAL"));
}

#[test]
fn rejects_malformed_and_non_svg_documents() {
    assert!(lint("<svg><g></svg>", &Options::default()).is_err());
    assert!(lint("<html/>", &Options::default()).is_err());
}
