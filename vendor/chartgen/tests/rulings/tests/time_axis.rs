//! Time labels and dividers. Owner decisions, 30 Sep and 1 Oct 2026 (RULINGS.md O-10, D-45).
use chartgen_rulings::{decisions, render, svg_texts};

const EXAMPLE: &str = "p1-21-daily-donations";

/// O-10: labels crowding each other are thinned (every k-th shown), restarting in each group,
/// never dropped for a divider nor shrunk: at 260 pt the days run 16, 19 … 28 in October and
/// start again at 1 in November.
#[test]
fn o10_crowded_time_labels_are_thinned_restarting_in_each_group() {
    let r = render(EXAMPLE, Some("narrow"));
    assert_eq!(r.report.status, "drawn");
    let mut days: Vec<(f64, f64, String)> = svg_texts(&r.svg.unwrap())
        .into_iter()
        .filter(|(_, t)| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()))
        .map(|(a, t)| (a["x"].parse().unwrap(), a["y"].parse().unwrap(), t))
        .collect();
    // The axis's first row: the digits lying lowest (below the plot), not the value scale.
    let row = days.iter().map(|d| d.1).fold(f64::MIN, f64::max);
    days.retain(|d| (d.1 - row).abs() < 1.0);
    days.sort_by(|a, b| a.0.total_cmp(&b.0));
    let shown: Vec<&str> = days.iter().map(|d| d.2.as_str()).collect();
    assert_eq!(shown, ["16", "19", "22", "25", "28", "1", "4", "7"]);
}

/// O-10: a label never touches a divider. At the ends of the axis the first and last labels
/// move in until they clear the outer dividers; a label beside the divider between two months
/// keeps its place, and the divider breaks round it (a halo of the page colour round its
/// letters). The expectations are the prototype's recorded decisions for this example.
///
/// Ignored: the line chart's time axis in Rust is not yet the ported one
/// (`crates/chartgen-layout/src/special/regular.rs` has the halo and the end clearance, but
/// the cartesian line layout does not use it), so neither decision is made.
#[test]
#[ignore = "the ported time axis with its halo and end clearance is not yet used by the line chart"]
fn o10_a_label_never_touches_a_divider() {
    let r = render(EXAMPLE, Some("narrow"));
    assert_eq!(r.report.status, "drawn");
    assert!(
        decisions(&r.report, "R2-39").iter().any(|t| t.contains("clear of the outer dividers")),
        "the end labels clear the outer dividers: {:?}",
        decisions(&r.report, "R2-39")
    );
    assert!(
        decisions(&r.report, "R2-40").iter().any(|t| t.contains("kept on its slot") && t.contains("halo")),
        "the label beside the month divider keeps its slot, with a halo: {:?}",
        decisions(&r.report, "R2-40")
    );
}
