//! The unit and magnitude of a value scale. D-3, D-20, D-59 (RULINGS.md).
use chartgen_rulings::{render, render_spec, spec, summary_labels, summary_str};
use serde_json::json;

/// D-3 (decided): a value scale states its unit and magnitude once, on the top label; the
/// other labels are bare numbers with the same decimals ("25 million inhabitants", 20, 15 …).
#[test]
fn d3_unit_and_magnitude_once_on_the_top_label() {
    let r = render("p1-02-population-by-region", None);
    assert_eq!(r.report.status, "drawn");
    assert_eq!(summary_str(&r.report, "orientation").as_deref(), Some("vertical"));
    assert_eq!(summary_str(&r.report, "top").as_deref(), Some("25 million inhabitants"));
    assert_eq!(summary_labels(&r.report), ["0", "5", "10", "15", "20", "25"]);
}

/// D-20 (decided): only a vertical scale runs its unit on from the top label; a horizontal
/// scale has bare numbers and a title below it, the quantity with its unit in parentheses
/// ("Donations ($K)", "Weirdness score (on 10)").
#[test]
fn d20_a_horizontal_scale_has_bare_numbers_and_the_unit_in_its_title() {
    for (example, title, labels) in [
        ("p1-06-donations-missing", "Donations ($K)", vec!["0", "50", "100", "150", "200", "250", "300"]),
        ("p1-05-weirdness-of-town-names", "Weirdness score (on 10)", vec!["0", "2", "4", "6", "8", "10"]),
    ] {
        let r = render(example, None);
        assert_eq!(r.report.status, "drawn", "{example}");
        assert_eq!(summary_str(&r.report, "orientation").as_deref(), Some("horizontal"), "{example}");
        assert_eq!(summary_str(&r.report, "scale_title").as_deref(), Some(title), "{example}");
        assert_eq!(summary_labels(&r.report), labels, "{example}");
    }
}

/// D-59 (decided): a horizontal percentage scale writes "%" on every label, and its title
/// has no "(%)".
#[test]
fn d59_a_horizontal_percentage_scale_writes_percent_on_every_label() {
    let r = render("p3-04-donations-wrapped-bars", None);
    assert_eq!(r.report.status, "drawn");
    assert_eq!(summary_str(&r.report, "orientation").as_deref(), Some("horizontal"));
    let labels = summary_labels(&r.report);
    assert!(!labels.is_empty() && labels.iter().all(|l| l.ends_with('%')), "{labels:?}");
    let title = summary_str(&r.report, "scale_title").unwrap();
    assert!(!title.contains("(%)"), "{title}");
}

/// D-3 (decided): the suffix on every label is a setting (scale.magnitude_style: suffix).
#[test]
fn d3_the_suffix_on_every_label_is_a_setting() {
    let mut s = spec("p1-02-population-by-region", None);
    s["scale"]["magnitude_style"] = json!("suffix");
    let r = render_spec(s, None, &[]);
    assert_eq!(r.report.status, "drawn");
    assert_eq!(summary_labels(&r.report), ["0M", "5M", "10M", "15M", "20M", "25M"]);
}

/// D-20 (decided): a unit's description is always in parentheses; a count noun runs on.
#[test]
fn d20_a_description_in_parentheses_a_count_noun_runs_on() {
    for (example, top) in [
        ("p2-01-donations-table", "$6,000 (monthly donation)"),
        ("p3-04-donations-wrapped-bars", "12% (of total donations)"),
        ("p1-02-population-by-region", "25 million inhabitants"),
        ("gen-01-days-in-each-month", "35 days in the month"),
    ] {
        let r = render(example, None);
        assert_eq!(summary_str(&r.report, "top").as_deref(), Some(top), "{example}");
    }
}
