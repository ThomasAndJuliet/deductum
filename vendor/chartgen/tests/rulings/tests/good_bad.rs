//! Good and bad colours. D-18 as decided by the owner, 1 Oct 2026 (RULINGS.md).
use chartgen_model::Theme;
use chartgen_rulings::{render, render_with};

const EXAMPLE: &str = "p2-50-deviation-from-budget";

/// D-18 (decided): semantic colours for every reader. In colour, the default, good and bad
/// are plain green and red in one file, with no texture.
#[test]
fn d18_in_colour_good_and_bad_are_plain_green_and_red_in_one_file() {
    let theme = Theme::default();
    assert_eq!(theme.tokens["--good-bad-mode"], "semantic");
    let r = render(EXAMPLE, None);
    assert_eq!(r.report.status, "drawn");
    let svg = r.svg.unwrap();
    for token in ["--good", "--bad"] {
        let colour = theme.tokens[token].as_str().unwrap().to_lowercase();
        assert!(svg.to_lowercase().contains(&format!("fill=\"{colour}\"")), "{token} {colour} is drawn");
    }
    assert!(!svg.contains("<pattern"), "no texture in colour");
    assert!(r.report.copies.is_empty(), "one file");
}

/// D-18 (decided): in black and white (audience.bw_risk) the same colours are textured, a
/// hatch for bad and a dither for good, still one file.
#[test]
fn d18_in_black_and_white_bad_is_hatched_and_good_dithered() {
    let r = render(EXAMPLE, Some("bw"));
    assert_eq!(r.report.status, "drawn");
    let svg = r.svg.unwrap();
    let pattern = |id: &str| {
        let start = svg.find(&format!("<pattern id=\"{id}\"")).unwrap_or_else(|| panic!("{id} defined"));
        let rest = &svg[start..];
        rest[..rest.find("</pattern>").unwrap()].to_string()
    };
    assert!(pattern("texture-bad").contains("rotate(45)"), "bad is a 45° hatch");
    assert!(pattern("texture-good").contains("<path"), "good is a dither of small squares");
    assert!(r.report.copies.is_empty(), "one file");
}

/// D-18 (decided): the colour-blind-safe pair remains a style option for publications to
/// unknown readers, with a red/green copy beside it.
#[test]
fn d18_the_safe_pair_is_a_style_option_with_a_red_green_copy() {
    let r = render_with(EXAMPLE, None, &[("--good-bad-mode", "safe-pair")]);
    assert_eq!(r.report.status, "drawn");
    assert!(r.report.copies.contains_key("red-green"), "{:?}", r.report.copies.keys());
}
