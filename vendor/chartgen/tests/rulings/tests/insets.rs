//! Insets. Owner decisions, 30 Sep 2026 (RULINGS.md O-7, D-56).
use chartgen_model::Theme;
use chartgen_rulings::{decisions, render};

fn token(name: &str) -> f64 {
    Theme::default().tokens[name].as_str().unwrap().parse().unwrap()
}

/// O-7 / D-56: the style's inset values: inset text 8 pt (0.8 em), an inset at most about as
/// tall as its main chart, and the share threshold (a figure labelled twice taller than 0.9
/// of its width shares one axis).
#[test]
fn o7_the_styles_inset_values() {
    assert_eq!(token("--inset-text-size"), 8.0);
    assert_eq!(token("--inset-max-height"), 1.0);
    assert_eq!(token("--inset-share-height"), 0.9);
}

fn inset_decisions(example: &str, variant: Option<&str>) -> Vec<String> {
    let r = render(example, variant);
    assert_eq!(r.report.status, "drawn", "{example} {variant:?}");
    decisions(&r.report, "6.6")
}

/// D-56 (decided): an inset repeats its category or time labels by default.
///
/// Ignored, like the next test: insets are not implemented in Rust ("inset is not supported
/// by portable layout"). The expectations are the prototype's decisions.
#[test]
#[ignore = "insets are not implemented in Rust yet"]
fn d56_an_inset_repeats_its_labels_by_default() {
    let d = inset_decisions("p1-08-donations-narrow-range", Some("wide"));
    assert!(d.iter().any(|t| t.contains("labels repeated in the inset")), "{d:?}");
}

/// D-56 (decided): an inset aligned with its main chart shares one axis, the main chart's,
/// when space is short: a compact main chart, or a figure that labelled twice would be taller
/// than --inset-share-height of its width.
#[test]
#[ignore = "insets are not implemented in Rust yet"]
fn d56_an_aligned_inset_shares_one_axis_when_space_is_short() {
    let compact = inset_decisions("p1-09-revenue-by-business-unit", None);
    assert!(compact.iter().any(|t| t.contains("one time axis for the two panels") && t.contains("does not repeat")), "{compact:?}");
    let tall = inset_decisions("r1-25-fill-weight-inset-line", None);
    assert!(tall.iter().any(|t| t.contains("one category axis for the two panels") && t.contains("space is short")), "{tall:?}");
}
