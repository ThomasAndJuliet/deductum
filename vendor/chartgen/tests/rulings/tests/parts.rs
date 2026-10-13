//! Parts of a total. D-1 and D-24 as decided by the owner, 1 Oct 2026 (RULINGS.md).
use chartgen_rulings::{choose, findings};

fn refused_at(example: &str, variant: &str, rule: &str, path: &str) {
    let r = choose(example, Some(variant));
    assert_eq!(r.status, "refused", "{example} {variant}");
    let f = findings(&r, rule);
    assert!(
        f.iter().any(|f| f.severity == "error" && f.path.as_deref() == Some(path)),
        "{example} {variant}: {rule} at {path}: {f:?}"
    );
}

/// D-1 / O-11 (decided): before any pie, stack, treemap or waterfall the spec must state
/// whether a part contains or overlaps another, as it must state a meaningful total and that
/// all parts are present; not stated, the chart is refused, asking for it.
#[test]
fn d1_overlap_must_be_stated_with_the_total_and_all_parts() {
    refused_at("p3-61-average-donations", "facts-missing", "R7-05", "data.parts.overlap");
    refused_at("p3-61-average-donations", "facts-missing", "R7-02", "data.parts.meaningful_total");
    refused_at("p3-61-average-donations", "facts-missing", "R7-03", "data.parts.all_parts");
}

/// D-24: overlapping parts are never drawn as a pie or a stack (R7-05).
#[test]
fn d24_overlapping_parts_are_not_drawn_as_parts_of_a_whole() {
    refused_at("p3-65-vehicle-sales-overlapping", "pie", "R7-05", "chart.type");
    refused_at("p3-26-vehicle-sales-separated-stack", "overlapping", "R7-05", "chart.type");
}
