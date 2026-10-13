//! An empty slot's note. Owner decision, 1 Oct 2026 (RULINGS.md O-13).
use chartgen_rulings::{render, svg_rects, svg_texts};

/// O-13: an empty slot says why, and its note stands inside the empty slot, on the baseline,
/// when it fits between the marks either side (Southwest, missing).
#[test]
fn o13_an_empty_slots_note_stands_in_its_slot() {
    let r = render("p1-06-donations-missing", None);
    assert_eq!(r.report.status, "drawn");
    let svg = r.svg.unwrap();
    let texts = svg_texts(&svg);
    let y_of = |s: &str| -> f64 {
        texts.iter().find(|(_, t)| t.contains(s)).unwrap_or_else(|| panic!("{s} is drawn")).0["y"].parse().unwrap()
    };
    let note = texts.iter().find(|(_, t)| t.contains("(missing)")).expect("the empty slot has a note");
    let bars: Vec<_> = svg_rects(&svg).into_iter().filter(|a| a.get("class").map(String::as_str) == Some("single")).collect();
    let band: f64 = bars[0]["height"].parse().unwrap();
    let baseline = bars.iter().map(|b| b["x"].parse::<f64>().unwrap()).fold(f64::MAX, f64::min);
    assert!((y_of("(missing)") - y_of("Southwest")).abs() < band / 2.0, "the note is on Southwest's row");
    assert!(note.0["x"].parse::<f64>().unwrap() >= baseline, "the note starts at the baseline, inside the plot");
}
