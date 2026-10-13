//! What a spec may not set. Owner decision, 30 Sep 2026 (RULINGS.md O-5).
use chartgen_rulings::spec;
use serde_json::json;

/// O-5: the values a style fixes (font, sizes, palette, greys) live only in the stylesheet,
/// so a chart spec cannot set them; changing them means making another style.
#[test]
fn o5_a_spec_cannot_set_the_values_a_style_fixes() {
    for (key, value) in [
        ("style", json!({"--series-1": "#ff0000"})),
        ("font", json!("Comic Sans")),
        ("theme", json!({"font_size": 14})),
    ] {
        let mut s = spec("p1-02-population-by-region", None);
        s[key] = value;
        let r = chartgen::choose(s, None, serde_json::Value::Null).unwrap();
        assert_eq!(r.status, "invalid", "{key}");
        assert!(r.findings.iter().any(|f| f.path.as_deref() == Some(key)), "{key}: {:?}", r.findings);
    }
    let mut s = spec("p1-02-population-by-region", None);
    s["colours"] = json!({"palette": ["#ff0000"]});
    let r = chartgen::choose(s, None, serde_json::Value::Null).unwrap();
    assert_eq!(r.status, "invalid", "colours.palette");
}
