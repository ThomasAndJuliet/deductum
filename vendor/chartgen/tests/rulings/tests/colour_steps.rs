//! Cutting a colour scale into steps. D-57, decided by the owner, 1 Oct 2026 (RULINGS.md).
use chartgen_rulings::{decisions, findings, render};

/// D-57: fairly even values take equal steps on rounded values.
#[test]
fn d57_fairly_even_values_take_equal_steps() {
    for example in ["p3-50-car-models-heatmap", "p3-52-satisfaction-heatmap"] {
        let r = render(example, None);
        assert_eq!(r.report.status, "drawn", "{example}");
        assert!(
            decisions(&r.report, "G4.29").iter().any(|t| t.contains("fairly even") && t.contains("equal steps")),
            "{example}: {:?}",
            decisions(&r.report, "G4.29")
        );
    }
}

/// D-57 (iii): the lightest step is clearly distinct from the page (ΔE at least 0.05 in OKLab).
#[test]
fn d57_the_lightest_step_is_distinct_from_the_page() {
    let r = render("p3-52-satisfaction-heatmap", None);
    let text = decisions(&r.report, "G4.34").into_iter().next().expect("the lightest step is checked");
    let delta: f64 = text
        .split("ΔE ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("a ΔE from the page: {text}"));
    assert!(delta >= 0.05, "{text}");
}

/// D-57: skewed amounts on a ratio scale whose pattern is the message, spanning at least two
/// orders of magnitude, take logarithmic 1–2–5 steps automatically; when the few extremes are
/// the message, equal steps; other cuts (natural breaks, quantiles) only when asked, as a
/// stated departure. The expectations are the prototype's decisions for the measles heatmap.
///
/// Ignored: a heatmap over time does not render in Rust yet ("its time axis under the grid is
/// not ported").
#[test]
#[ignore = "a heatmap over time (p3-51) does not render in Rust yet"]
fn d57_skewed_amounts_over_orders_of_magnitude_take_logarithmic_steps() {
    let auto = render("p3-51-measles-heatmap", None);
    assert!(decisions(&auto.report, "G4.33").iter().any(|t| t.contains("skewed")), "logarithmic steps");
    let extremes = render("p3-51-measles-heatmap", Some("extremes"));
    assert!(decisions(&extremes.report, "G4.29").iter().any(|t| t.contains("extremes")), "equal steps for the extremes");
    for (variant, rule) in [("natural", "G4.30"), ("quantile", "G4.31")] {
        let r = render("p3-51-measles-heatmap", Some(variant));
        assert!(
            findings(&r.report, rule).iter().any(|f| f.message.contains("departs from the procedure")),
            "{variant} is a stated departure"
        );
    }
}
