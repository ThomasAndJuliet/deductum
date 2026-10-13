//! Value labels. Owner decisions, phase 2 and phase 3 (RULINGS.md O-8, O-11).
use chartgen_rulings::{render, summary_labels, summary_str, svg_texts};

/// O-8: value labels come by default where the book's procedure gives them: fewer values
/// than scale labels (4 against 7 here), so the values are labelled and the value scale and
/// its gridlines go, the zero line kept.
#[test]
fn o8_value_labels_come_by_default_when_there_are_fewer_values_than_scale_labels() {
    let r = render("p2-02-average-donation", None);
    assert_eq!(r.report.status, "drawn");
    let svg = r.svg.unwrap();
    let texts: Vec<String> = svg_texts(&svg).into_iter().map(|(_, t)| t).collect();
    for value in ["$54", "$42", "$40", "$17"] {
        assert!(texts.iter().any(|t| t == value), "{value} is labelled");
    }
    for scale_label in summary_labels(&r.report) {
        assert!(!texts.contains(&scale_label), "the scale label {scale_label} is not drawn");
    }
    let lines = roxmltree::Document::parse(&svg).unwrap().descendants().filter(|n| n.has_tag_name("line")).count();
    assert_eq!(lines, 1, "the gridlines removed, the zero line kept");
}

/// O-11: labels follow the scale's magnitude: on a scale in millions the values are labelled
/// "$1.6 million", not "$1,600,000".
#[test]
fn o11_value_labels_follow_the_scales_magnitude() {
    let r = render("p2-43-cash-lowest", None);
    assert_eq!(r.report.status, "drawn");
    assert_eq!(summary_str(&r.report, "top").as_deref(), Some("$2.0 million in donations"));
    let texts: Vec<String> = svg_texts(&r.svg.unwrap()).into_iter().map(|(_, t)| t).collect();
    for value in ["$1.6 million", "$1.4 million", "$0.9 million"] {
        assert!(texts.iter().any(|t| t == value), "{value} is labelled in the scale's magnitude");
    }
}
