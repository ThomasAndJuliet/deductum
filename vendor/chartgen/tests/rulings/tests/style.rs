//! The plain style's fixed values. Owner decisions, 30 Sep 2026 (RULINGS.md O-1, O-2, O-3).
use chartgen_model::Theme;
use chartgen_rulings::{render, svg_texts};

/// O-2 (D-2, decided): the plain style's palette is the colour chapter's muted one, purple
/// sixth, with the teal turned 24° towards blue (#7CA79D → #78A6AA) so that under protanopia
/// it stays apart from the olive.
#[test]
fn o2_the_plain_palette_is_the_muted_one() {
    let theme = chartgen_style::builtin("plain").unwrap().theme().unwrap();
    let want = ["#95b7d3", "#dd8045", "#a4ab81", "#d8b35b", "#78a6aa", "#9a77b7"];
    let got: Vec<String> = theme.palette.iter().take(6).map(|c| c.to_lowercase()).collect();
    assert_eq!(got, want);
    // The plain style is the default theme, so every render without a style gets it.
    assert_eq!(Theme::default().palette, theme.palette);
}

/// O-2: a single series is blue, like single bars and dots, not the orange of one figure.
#[test]
fn o2_a_single_series_is_blue() {
    let theme = Theme::default();
    assert_eq!(theme.tokens["--single"].as_str().map(str::to_lowercase).as_deref(), Some("#95b7d3"));
}

/// O-3: labels at 10 pt, as in the diagrams.
#[test]
fn o3_labels_are_10_pt() {
    assert_eq!(Theme::default().font_size, 10.0);
}

/// O-3: titles 1.2 times the label size, bold.
#[test]
fn o3_titles_are_1_2_times_the_labels_and_bold() {
    let theme = Theme::default();
    let title = theme.tokens["--title-size"].as_str().unwrap().parse::<f64>().unwrap();
    assert_eq!(title, 1.2 * theme.font_size);
    let r = render("p3-12-expenses-hidden-digits", None);
    let svg = r.svg.unwrap();
    let titles: Vec<_> = svg_texts(&svg).into_iter().filter(|(a, _)| a.get("class").map(String::as_str) == Some("title")).collect();
    assert!(!titles.is_empty(), "the chart has a title");
    for (a, text) in titles {
        assert_eq!(a["font-size"].parse::<f64>().unwrap(), title, "{text}");
        assert_eq!(a["font-weight"], "bold", "{text}");
    }
}

/// O-1: the plain style is flat: no cards, shadows or arched strips, so no filters, no
/// gradients, nothing classed as a card or a shadow, in any chart it draws.
#[test]
fn o1_the_plain_style_is_flat() {
    for (example, variant) in [
        ("gen-01-days-in-each-month", None),
        ("p1-02-population-by-region", None),
        ("p1-06-donations-missing", None),
        ("p2-50-deviation-from-budget", Some("bw")),
        ("p3-16-expenses-comment", None),
        ("p3-30-donations-stacked-area", None),
        ("p3-39-crops-grouped-stacks", None),
        ("p3-50-car-models-heatmap", None),
    ] {
        let r = render(example, variant);
        assert_eq!(r.report.status, "drawn", "{example}");
        let svg = r.svg.unwrap();
        for forbidden in ["<filter", "Gradient", "shadow", "card"] {
            assert!(!svg.contains(forbidden), "{example}: the plain style draws no {forbidden}");
        }
    }
}
