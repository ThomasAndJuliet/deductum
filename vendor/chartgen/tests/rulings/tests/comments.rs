//! Comments, notes and keys. Owner decisions, phase 2, 30 Sep 2026 (RULINGS.md O-8).
use chartgen_model::Theme;
use chartgen_rulings::{render, svg_texts};

/// O-8: the maker's comments are New Computer Modern italic in the reserved blue.
#[test]
fn o8_comments_are_italic_in_the_reserved_blue() {
    let blue = Theme::default().tokens["--comment"].as_str().unwrap().to_lowercase();
    let r = render("p3-16-expenses-comment", None);
    let comments: Vec<_> = svg_texts(&r.svg.unwrap())
        .into_iter()
        .filter(|(a, _)| a.get("class").map(String::as_str) == Some("comment"))
        .collect();
    assert!(!comments.is_empty());
    for (a, text) in comments {
        assert_eq!(a["font-style"], "italic", "{text}");
        assert_eq!(a["fill"].to_lowercase(), blue, "{text}");
        assert!(a["font-family"].contains("New Computer Modern"), "{text}");
    }
}

/// O-8: italic also marks the chart's own notes ("(missing)"), which are comments too.
#[test]
fn o8_the_charts_own_notes_are_italic() {
    let r = render("p1-06-donations-missing", None);
    let notes: Vec<_> = svg_texts(&r.svg.unwrap()).into_iter().filter(|(_, t)| t.contains("(missing)")).collect();
    assert!(!notes.is_empty(), "the missing value is noted");
    for (a, _) in notes {
        assert_eq!(a["font-style"], "italic");
    }
}

/// O-8: keys are upright.
#[test]
fn o8_keys_are_upright() {
    let r = render("p3-52-satisfaction-heatmap", None);
    let keys: Vec<_> = svg_texts(&r.svg.unwrap())
        .into_iter()
        .filter(|(a, _)| a.get("class").is_some_and(|c| c.starts_with("legend")))
        .collect();
    assert!(!keys.is_empty(), "the heatmap has a key");
    for (a, text) in keys {
        assert_eq!(a["font-style"], "normal", "{text}");
    }
}
