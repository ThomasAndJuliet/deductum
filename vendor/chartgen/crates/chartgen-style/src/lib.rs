//! Chart style assets and typed chart theme adaptation.
use chartgen_model::Theme;
pub use figkit_style::*;
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Style {
    pub name: String,
    pub maker: String,
    pub defs: Option<String>,
    pub tokens: Tokens,
    pub rules: Vec<Rule>,
    pub css: String,
}
impl From<figkit_style::Style> for Style {
    fn from(s: figkit_style::Style) -> Self {
        Self {
            name: s.name,
            maker: s.maker,
            defs: s.defs,
            tokens: s.tokens,
            rules: s.rules,
            css: s.css,
        }
    }
}
impl Style {
    fn shared(&self) -> figkit_style::Style {
        figkit_style::Style {
            name: self.name.clone(),
            maker: self.maker.clone(),
            defs: self.defs.clone(),
            tokens: self.tokens.clone(),
            rules: self.rules.clone(),
            css: self.css.clone(),
        }
    }
    pub fn metric(&self, name: &str) -> Result<f64> {
        self.shared().metric(name)
    }
    pub fn paint(&self, name: &str) -> Option<&str> {
        self.tokens.get(name)
    }
    pub fn has(&self, name: &str) -> bool {
        self.tokens.get(name).is_some()
    }
    pub fn load(name: &str, maker: &str, root: &str, source: &mut dyn StyleSource) -> Result<Self> {
        figkit_style::Style::load(name, maker, root, source).map(Into::into)
    }
    pub fn from_flat_source(
        name: &str,
        maker: &str,
        raw: &str,
        defs: Option<String>,
    ) -> Result<Self> {
        figkit_style::Style::from_flat_source(name, maker, raw, defs).map(Into::into)
    }
    pub fn theme(&self) -> Result<Theme> {
        let t = self.shared().theme()?;
        Ok(Theme {
            name: t.name,
            font_family: t.font_family,
            font_size: t.font_size,
            text: t.text,
            grid: t.grid,
            background: t.background,
            palette: t.palette,
            tokens: t.tokens,
            stylesheet: t.stylesheet,
        })
    }
}
// ---- the original's bundled stylesheets ---------------------------------------------------------

/// The original's stylesheets (charts/style/*.css, bundled unchanged with the Python package).
pub const BUILTIN_SHEETS: [(&str, &str); 4] = [
    (
        "base.css",
        include_str!("../../../python/chartgen/styles/base.css"),
    ),
    (
        "plain.css",
        include_str!("../../../python/chartgen/styles/plain.css"),
    ),
    (
        "message.css",
        include_str!("../../../python/chartgen/styles/message.css"),
    ),
    (
        "palette-muted.css",
        include_str!("../../../python/chartgen/styles/palette-muted.css"),
    ),
];

/// One of the original's bundled styles (`plain`, `message`, `palette-muted`, `base`), flattened
/// as the original's chartgen `Style(name)` does (maker `chartgen`).
pub fn builtin(name: &str) -> Result<Style> {
    let root = format!("{name}.css");
    if !BUILTIN_SHEETS.iter().any(|(n, _)| *n == root) {
        return Err(StyleError::new(
            ErrorKind::Value,
            format!("no bundled style {name:?}"),
        ));
    }
    let mut source = MemorySource::new(BUILTIN_SHEETS);
    Style::load(name, "chartgen", &root, &mut source)
}
