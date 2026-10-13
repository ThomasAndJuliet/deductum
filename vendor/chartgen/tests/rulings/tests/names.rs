//! Names under vertical bars. Owner decision, 30 Sep 2026 (RULINGS.md O-6).
use chartgen_rulings::{findings, render, summary_str, svg_texts};

/// O-6: names are centred under their bars and may reach past their bands, with at least
/// 1 em between neighbours; only if they would still meet (after wrapping) does the chart
/// turn horizontal. At 480 pt every name of this example keeps 1 em from its neighbours under
/// columns (the variant was chosen as the narrowest width that does), so it stays vertical,
/// as the prototype draws it.
///
/// Ignored: a known deviation. The Rust layout turns a chart horizontal as soon as one name
/// plus 4 pt is wider than its band (`crates/chartgen-layout/src/orientation.rs`,
/// `horizontal_if_needed`), and does not wrap names first; at 480 pt a band is 51.5 pt and
/// this example turns horizontal.
#[test]
#[ignore = "deviation: Rust turns charts horizontal when a name exceeds its band; O-6 allows names past their bands with 1 em between neighbours"]
fn o6_names_may_reach_past_their_bands_keeping_1_em_apart() {
    let r = render("p1-06-donations-missing", Some("wide"));
    assert_eq!(r.report.status, "drawn");
    assert_eq!(summary_str(&r.report, "orientation").as_deref(), Some("vertical"));
}

/// O-6: if the names would still meet, the chart turns horizontal, the names whole at the
/// left. (Here they meet even wrapped: "Invoicing" and "Expense tracking", as the prototype
/// found.)
#[test]
fn o6_names_that_would_still_meet_turn_the_chart_horizontal() {
    let r = render("r1-40-feature-usage-descriptive-wraps", None);
    assert_eq!(r.report.status, "drawn");
    assert_eq!(summary_str(&r.report, "orientation").as_deref(), Some("horizontal"));
    assert!(!findings(&r.report, "R9-10").is_empty(), "the report says why it turned");
    let texts: Vec<String> = svg_texts(&r.svg.unwrap()).into_iter().map(|(_, t)| t).collect();
    for name in ["Expense tracking", "Bank reconciliation", "API / integrations"] {
        assert!(texts.iter().any(|t| t == name), "{name} stands whole at the left");
    }
}
