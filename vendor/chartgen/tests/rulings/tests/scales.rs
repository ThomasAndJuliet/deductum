//! Value scales. Owner decision, 30 Sep 2026 (RULINGS.md O-4).
use chartgen::model::Decimal;
use chartgen_rulings::{decisions, render, summary_str};
use std::str::FromStr;

fn scale(max: &str) -> String {
    let d = |s: &str| Decimal::from_str(s).unwrap();
    let s = chartgen::scales::nice_scale(
        &[d(max)], "bar", "auto", "meaningful", None, None,
        &[d("1"), d("2"), d("5")], false, 5, None, None, None, None, d("0.15"), false,
    )
    .unwrap();
    chartgen::scales::summary(&s)
}

/// O-4: a step of 5 or of 1 that leaves more than a quarter of the scale empty above the data
/// gives way to the next smaller step (5 → 2, 1 → 0.5, 10 → 5) if that needs at most 8
/// intervals: 0–1.6 by 0.2 rather than 0–2.0 by 0.5, and 0–35 by 5 rather than 0–40 by 10.
#[test]
fn o4_a_coarse_step_leaving_a_quarter_empty_gives_way() {
    assert_eq!(scale("1.5"), "0–1.6 by 0.2");
    assert_eq!(scale("31"), "0–35 by 5");
}

/// O-4: it departs from the book once, knowingly: 3.16 gives 0–3.5 by 0.5 where the book
/// has 0–4 by 1.
#[test]
fn o4_the_known_departure_from_the_book() {
    assert_eq!(scale("3.16"), "0–3.5 by 0.5");
}

/// O-4: a step that leaves a quarter or less empty stays (32 on 0–40 leaves a fifth).
#[test]
fn o4_a_step_leaving_less_than_a_quarter_stays() {
    assert_eq!(scale("32"), "0–40 by 10");
}

/// O-4, end to end: the days-in-each-month example draws 0–35 by 5 and says why.
#[test]
fn o4_end_to_end_with_its_reason() {
    let r = render("gen-01-days-in-each-month", None);
    assert_eq!(r.report.status, "drawn");
    assert_eq!(summary_str(&r.report, "scale").as_deref(), Some("0–35 by 5"));
    assert!(
        decisions(&r.report, "02 D5").iter().any(|t| t.contains("would leave") && t.contains("5 gives 0–35")),
        "the report records why the step gave way: {:?}",
        decisions(&r.report, "02 D5")
    );
}
