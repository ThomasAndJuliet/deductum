//! A table under several series keys its rows. Owner decision, 1 Oct 2026 (RULINGS.md O-9).
//!
//! Ignored until the table with the graph is drawn: it is ported
//! (`crates/chartgen-layout/src/families/composition.rs`, which resolves `table.row_key: auto`
//! to `swatch_after` and drops the legend when the rows are keyed) but the bar layout does not
//! call it yet, so `p2-01` renders as bars with a legend and no table. Whoever wires it in
//! removes the `#[ignore]`s; adjusting the assertions to the drawn output is a ruling-test
//! change and comes to Juliet. The expectations follow the prototype's reference output,
//! `tests/conformance/oracle/plain/p2-01-donations-table{,--legend}.{json,svg}`.
use chartgen_model::Theme;
use chartgen_rulings::{decisions, render, svg_rects, svg_texts};

const EXAMPLE: &str = "p2-01-donations-table";
const SERIES: [&str; 3] = ["Cash", "Credit card", "Cheque"];

fn count(svg: &str, name: &str) -> usize {
    svg_texts(svg).iter().filter(|(_, t)| t.trim() == name).count()
}

/// O-9: under several series the table's rows are keyed (each name where it would stand
/// without keys, then a square of its series' colour on the value axis), and the legend is
/// left out, the table being the key.
#[test]
#[ignore = "the table with the graph is ported but not yet wired into the bar layout"]
fn o9_a_table_under_several_series_keys_its_rows_and_drops_the_legend() {
    let r = render(EXAMPLE, None);
    assert_eq!(r.report.status, "drawn");
    assert!(!decisions(&r.report, "R10-7").is_empty(), "a table under the graph");
    assert!(
        decisions(&r.report, "R4-10").iter().any(|t| t.contains("keyed") && t.contains("value axis")),
        "the rows keyed by squares on the value axis: {:?}",
        decisions(&r.report, "R4-10")
    );
    assert!(
        decisions(&r.report, "R4-1").iter().any(|t| t.contains("no legend")),
        "no legend beside the keyed rows: {:?}",
        decisions(&r.report, "R4-1")
    );
    let svg = r.svg.unwrap();
    for name in SERIES {
        assert_eq!(count(&svg, name), 1, "{name} is named once, as a row, with no legend entry");
    }
    // One key square per series, in its colour, all in one column (the value axis).
    let palette: Vec<String> = Theme::default().palette.iter().take(3).map(|c| c.to_lowercase()).collect();
    let squares: Vec<_> = svg_rects(&svg)
        .into_iter()
        .filter(|a| {
            let w: f64 = a.get("width").and_then(|s| s.parse().ok()).unwrap_or(f64::MAX);
            let h: f64 = a.get("height").and_then(|s| s.parse().ok()).unwrap_or(f64::MAX);
            w <= 10.0 && (w - h).abs() < 0.01
        })
        .collect();
    for colour in &palette {
        let keyed: Vec<_> = squares.iter().filter(|a| a.get("fill").map(|f| f.to_lowercase()) == Some(colour.clone())).collect();
        assert_eq!(keyed.len(), 1, "one key square in {colour}");
    }
    let xs: Vec<String> = squares.iter().map(|a| a["x"].clone()).collect();
    assert!(xs.windows(2).all(|w| w[0] == w[1]), "the key squares stand in one column: {xs:?}");
}

/// O-9: keeping the legend as well is an option (table.legend: keep).
#[test]
#[ignore = "the table with the graph is ported but not yet wired into the bar layout"]
fn o9_keeping_the_legend_is_an_option() {
    let r = render(EXAMPLE, Some("legend"));
    assert_eq!(r.report.status, "drawn");
    assert!(!decisions(&r.report, "R4-10").is_empty(), "the rows are still keyed");
    assert!(decisions(&r.report, "R4-1").iter().all(|t| !t.contains("no legend")));
    let svg = r.svg.unwrap();
    for name in SERIES {
        assert_eq!(count(&svg, name), 2, "{name} is named in the legend and as a row");
    }
}
