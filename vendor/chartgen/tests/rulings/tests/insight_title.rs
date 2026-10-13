//! Which charts must state their insight. D-55 (RULINGS.md).
use chartgen_rulings::{choose, findings};

/// D-55 (decided): a chart whose job is to answer, persuade or make the reader aware states
/// its insight; with a descriptive title it is refused (R0-10, at title.kind).
#[test]
fn d55_an_answering_chart_with_a_descriptive_title_is_refused() {
    let r = choose("p1-01-headcount-by-region", Some("as-answer"));
    assert_eq!(r.status, "refused");
    let f = findings(&r, "R0-10");
    assert!(f.iter().any(|f| f.severity == "error" && f.path.as_deref() == Some("title.kind")), "{f:?}");
}

/// D-55 (decided): a look-up chart may have a descriptive title, and nothing is reported.
#[test]
fn d55_a_lookup_chart_may_have_a_descriptive_title() {
    let r = choose("p1-01-headcount-by-region", None);
    assert_ne!(r.status, "refused");
    assert!(findings(&r, "R0-10").is_empty());
}
