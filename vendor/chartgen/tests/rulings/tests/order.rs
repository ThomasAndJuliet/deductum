//! The order of parts. The owner's phase-3 decision, 1 Oct 2026 (RULINGS.md O-11, D-26).
use chartgen_rulings::{decisions, render};

fn stacking(example: &str, variant: Option<&str>) -> String {
    let r = render(example, variant);
    assert_eq!(r.report.status, "drawn", "{example} {variant:?}");
    decisions(&r.report, "R7-24")
        .into_iter()
        .find(|t| t.starts_with("stacking order from the base: "))
        .unwrap_or_else(|| panic!("{example} {variant:?} records its stacking order"))
}

/// O-11: parts are usually reordered to suit the chart, but where the data's order is fixed
/// (data.parts.order: fixed; a table shared by several charts ordered differently), the
/// stacks follow the order given.
#[test]
fn o11_a_fixed_order_is_kept() {
    let fixed = stacking("p3-30-donations-stacked-area", Some("fixed"));
    assert!(fixed.starts_with("stacking order from the base: East, West, North, South"), "{fixed}");
    assert!(fixed.contains("fixed"), "{fixed}");
    let free = stacking("p3-30-donations-stacked-area", None);
    assert!(free.starts_with("stacking order from the base: East, West, South, North"), "{free}");

    let fixed = stacking("p3-39-crops-grouped-stacks", Some("fixed"));
    assert!(fixed.starts_with("stacking order from the base: India, Thailand, Egypt"), "{fixed}");
    let free = stacking("p3-39-crops-grouped-stacks", None);
    assert!(free.starts_with("stacking order from the base: Egypt"), "{free}");
}
