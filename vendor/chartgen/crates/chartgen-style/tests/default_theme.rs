//! `Theme::default()` is the original's default style, `plain`, flattened from the original
//! stylesheets. The bundled asset in chartgen-model is generated here:
//! `CHARTGEN_REGENERATE_THEME=1 cargo test -p chartgen-style --test default_theme`.
use chartgen_model::Theme;

#[test]
fn default_theme_is_the_flattened_original_plain_style() {
    let theme = chartgen_style::builtin("plain").unwrap().theme().unwrap();
    assert!(
        theme.stylesheet.starts_with(
            "/* Flattened by chartgen from the plain stylesheet: literal values only */"
        )
    );
    let asset = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../chartgen-model/assets/theme-plain.json"
    );
    if std::env::var_os("CHARTGEN_REGENERATE_THEME").is_some() {
        std::fs::write(asset, serde_json::to_string_pretty(&theme).unwrap() + "\n").unwrap();
    }
    assert_eq!(
        serde_json::to_value(Theme::default()).unwrap(),
        serde_json::to_value(&theme).unwrap(),
        "regenerate the asset with CHARTGEN_REGENERATE_THEME=1"
    );
}
