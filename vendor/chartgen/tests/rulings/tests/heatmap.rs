//! Values in heatmap cells. Owner decision, 1 Oct 2026 (RULINGS.md O-12).
use chartgen_model::Theme;
use chartgen_rulings::{contrast, render, svg_rects, svg_texts};

const EXAMPLE: &str = "p3-50-car-models-heatmap";

fn num(m: &std::collections::BTreeMap<String, String>, k: &str) -> f64 {
    m[k].parse().unwrap()
}

/// O-12: the cells keep their strong colours and each value is black or white, whichever
/// contrasts more with its cell.
#[test]
fn o12_values_on_strong_cells_are_black_or_white_whichever_contrasts_more() {
    let r = render(EXAMPLE, None);
    assert_eq!(r.report.status, "drawn");
    let svg = r.svg.unwrap();
    let cells: Vec<_> = svg_rects(&svg)
        .into_iter()
        .filter(|a| a.get("class").is_some_and(|c| c.split(' ').next() == Some("cell")))
        .collect();
    let values: Vec<_> = svg_texts(&svg)
        .into_iter()
        .filter(|(a, _)| a.get("class").is_some_and(|c| c.contains("table-cell")))
        .collect();
    assert!(!values.is_empty(), "the values are printed in their cells");
    for (a, text) in values {
        let (x, y) = (num(&a, "x"), num(&a, "y"));
        let cell = cells
            .iter()
            .find(|c| {
                let (cx, cy) = (num(c, "x"), num(c, "y"));
                x >= cx && x <= cx + num(c, "width") && y >= cy && y <= cy + num(c, "height")
            })
            .unwrap_or_else(|| panic!("{text} lies in a cell"));
        let fill = cell["fill"].to_lowercase();
        let ink = a["fill"].to_lowercase();
        assert!(ink == "#000000" || ink == "#ffffff", "{text}: black or white, not {ink}");
        let best = if contrast("#000000", &fill) >= contrast("#ffffff", &fill) { "#000000" } else { "#ffffff" };
        assert_eq!(ink, best, "{text} on {fill}");
    }
}

/// O-12: muted cells (pale, the values in the text grey) remain a per-chart option.
#[test]
fn o12_muted_cells_are_an_option_with_grey_values() {
    let r = render(EXAMPLE, Some("muted"));
    assert_eq!(r.report.status, "drawn");
    let grey = Theme::default().tokens["--text"].as_str().unwrap().to_lowercase();
    let values: Vec<_> = svg_texts(&r.svg.unwrap())
        .into_iter()
        .filter(|(a, _)| a.get("class").is_some_and(|c| c.contains("table-cell")))
        .collect();
    assert!(!values.is_empty());
    for (a, text) in values {
        assert_eq!(a["fill"].to_lowercase(), grey, "{text}");
    }
}
