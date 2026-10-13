//! The style lint against the original's lint_style: the original plain style and faulty
//! stylesheets importing it, flattened and linted by the original
//! (tests/fixtures/generate_style_lint.py); every finding's rule, severity, level and message.
use chartgen_lint::style::{FlatStyle, lint_style};
use serde_json::Value;

#[test]
fn style_lint_matches_the_original() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/style_lint.json"
    );
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(path).expect("bundled fixture")).unwrap();
    assert!(cases.len() >= 13);
    let mut total = 0;
    for case in &cases {
        let style = FlatStyle {
            tokens: case["tokens"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                .collect(),
            rules: case["rules"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        r[0].as_str().unwrap().to_owned(),
                        r[1].as_array()
                            .unwrap()
                            .iter()
                            .map(|d| {
                                (
                                    d[0].as_str().unwrap().to_owned(),
                                    d[1].as_str().unwrap().to_owned(),
                                )
                            })
                            .collect(),
                    )
                })
                .collect(),
        };
        let ours: Vec<(String, String, String, String)> = lint_style(&style)
            .into_iter()
            .map(|f| (f.rule, f.severity, f.level, f.message))
            .collect();
        let oracle: Vec<(String, String, String, String)> = case["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let s = |k: &str| f[k].as_str().unwrap().to_owned();
                (s("rule"), s("severity"), s("level"), s("message"))
            })
            .collect();
        assert_eq!(ours, oracle, "style {}", case["name"]);
        total += oracle.len();
    }
    assert!(total > 50, "too few findings compared: {total}");
}
