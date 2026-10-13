//! Diverging ramps. D-37 and the owner's phase-3 decision, 1 Oct 2026 (RULINGS.md).
use chartgen_rulings::{decisions, findings, render};

/// D-37: a ramp diverges only about a dividing value the spec states; a diverging encoding
/// without one is refused (R3-13), never centred on the middle of the data.
#[test]
fn d37_a_diverging_ramp_without_a_dividing_value_is_refused() {
    let r = render("p3-53-station-temperatures", Some("diverging-about-the-middle"));
    assert_eq!(r.report.status, "refused");
    assert!(r.svg.is_none());
    assert!(findings(&r.report, "R3-13").iter().any(|f| f.severity == "error"));
}

/// D-37 / O-11 (owner, 1 Oct 2026): a diverging ramp is symmetric about the dividing value,
/// one step the same distance from it on either side; the key mirrors its steps about the
/// neutral, even where the dividing value stands off the data's middle (7 on 10).
#[test]
fn d37_a_diverging_ramp_is_symmetric_about_the_dividing_value() {
    let r = render("p3-52-satisfaction-heatmap", None);
    assert_eq!(r.report.status, "drawn");
    assert!(decisions(&r.report, "R3-13").iter().any(|t| t.contains("dividing value, 7")));
    let svg = r.svg.unwrap();
    let steps: Vec<i32> = svg
        .match_indices("legend-swatch div-")
        .map(|(i, _)| {
            let s = &svg[i + "legend-swatch div-".len()..];
            let tok = &s[..s.find(|c: char| c == ' ' || c == '"').unwrap()];
            match tok.strip_prefix('m') {
                Some(k) => -k.parse::<i32>().unwrap(),
                None => tok.trim_start_matches('p').parse::<i32>().unwrap(),
            }
        })
        .collect();
    assert!(steps.contains(&0), "a neutral step: {steps:?}");
    for k in &steps {
        assert!(steps.contains(&-k), "step {k} has its mirror: {steps:?}");
    }
}
