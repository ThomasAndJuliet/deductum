//! Stylesheets with tokens, flattened: the semantics of the original figkit `style.Style`
//! (shared by chartgen and the other house-format generators) and bake's `label_style`.
//!
//! A style is a stylesheet whose values are tokens: custom properties on `:root`, used by the
//! role rules through `var()`. Flattening inlines `@import`s, resolves `var(--x[, fallback])`,
//! then `em` (a multiple of `--label-size`, never a CSS em), then the colour functions
//! `mix(a, b, t)`, `oklch-shift(c, dL, dC)` and `oklch-light(c, L[, Cmax])`, replaces every
//! `var()` in the rules by its literal value and gives a bare `font-size` `px`. A style names its
//! mark library in a comment directive, `/* @defs plain-defs.svg */`.
//!
//! The crate is pure: stylesheets are read through a host-supplied [`StyleSource`], which
//! resolves an import or defs target against the importing sheet and reads a sheet's text.
//! Pattern matching follows the original's regular expressions; where Python and Rust differ
//! (Unicode `\w`, non-ASCII digits in `float`), the difference is documented on the function.

pub mod colour;
pub mod py;

use legacy::Theme;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

/// What went wrong, by the original's exception class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// `ValueError`: undefined or self-referring tokens, unparsable numbers or colours.
    Value,
    /// `IndexError`: a colour function with too few arguments.
    Index,
    /// `KeyError`: `metric` of a token the style lacks.
    Key,
    /// `AttributeError`: a pattern the original applies to a value that does not match it
    /// (`em` with a `--label-size` that is not a number; a `font-size` that is not one).
    Attribute,
    /// The host's [`StyleSource`] failed (a sheet that cannot be read).
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleError {
    pub kind: ErrorKind,
    pub message: String,
}
impl StyleError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    fn value(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Value, message)
    }
    fn none_group() -> Self {
        Self::new(
            ErrorKind::Attribute,
            "'NoneType' object has no attribute 'group'",
        )
    }
}
impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for StyleError {}
impl From<StyleError> for figkit_model::Error {
    fn from(e: StyleError) -> Self {
        figkit_model::Error::new("style", e.message)
    }
}
pub type Result<T> = std::result::Result<T, StyleError>;

/// Where stylesheets come from. Identities are opaque strings chosen by the host (a resolved
/// path, a URL, a key); the crate only compares them, so an import cycle or a sheet imported
/// twice is inlined once.
pub trait StyleSource {
    /// The identity of `target`, as written in an `@import` or `@defs` of the sheet `from`
    /// (the original: `(Path(from).parent / target).resolve()`). An import is read next; a
    /// defs target (the mark library) is only named.
    fn resolve(&mut self, from: &str, target: &str, link: Link) -> Result<String>;
    /// The text of the sheet with this identity.
    fn read(&mut self, id: &str) -> Result<String>;
}

/// What a resolved target is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    /// `@import`: a stylesheet to inline.
    Import,
    /// `/* @defs … */`: the style's mark library, named but never read here.
    Defs,
}

/// Sheets held in memory, keyed by slash-separated paths; targets resolve lexically against
/// the importing sheet's directory (`.` and `..` segments folded; an absolute target replaces it).
#[derive(Debug, Clone, Default)]
pub struct MemorySource {
    pub sheets: BTreeMap<String, String>,
}
impl MemorySource {
    pub fn new<K: Into<String>, V: Into<String>>(sheets: impl IntoIterator<Item = (K, V)>) -> Self {
        Self {
            sheets: sheets
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }
}
impl StyleSource for MemorySource {
    fn resolve(&mut self, from: &str, target: &str, _link: Link) -> Result<String> {
        let base = if target.starts_with('/') {
            ""
        } else {
            from.rsplit_once('/').map_or("", |(d, _)| d)
        };
        let mut parts: Vec<&str> = vec![];
        for seg in base.split('/').chain(target.split('/')) {
            match seg {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        let joined = parts.join("/");
        Ok(if from.starts_with('/') || target.starts_with('/') {
            format!("/{joined}")
        } else {
            joined
        })
    }
    fn read(&mut self, id: &str) -> Result<String> {
        self.sheets.get(id).cloned().ok_or_else(|| {
            StyleError::new(
                ErrorKind::Source,
                format!("[Errno 2] No such file or directory: {}", py::repr(id)),
            )
        })
    }
}

// Python's `\s` of a str pattern is str.isspace: Unicode White_Space and U+001C…U+001F.
macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                &$pattern
                    .replace(r"\s", r"[\s\x1c-\x1f]")
                    .replace(r"\S", r"[^\s\x1c-\x1f]"),
            )
            .expect("valid pattern")
        });
    };
}
// The negated class of the bare import target holds \s; written out to keep it a class.
static IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"@import[\s\x1c-\x1f]+(?:url\([\s\x1c-\x1f]*)?(?:"([^"]+)"|'([^']+)'|([^"')\s\x1c-\x1f;]+))[\s\x1c-\x1f]*\)?[\s\x1c-\x1f]*;"#,
    )
    .expect("valid pattern")
});
re!(DEFS, r"/\*\s*@defs\s+(\S+)\s*\*/");
re!(COMMENT, r"(?s)/\*.*?\*/");
re!(RULE, r"([^{}]+)\{([^}]*)\}");
re!(VAR, r"var\(\s*(--[\w-]+)\s*(?:,\s*([^()]*?))?\s*\)");
re!(FUNC, r"(mix|oklch-shift|oklch-light)\(([^()]*)\)");
re!(EM_AT, r"\A(-?\d*\.?\d+)em");
re!(NUMBER_ONLY, r"\A-?\d*\.?\d+\z");
re!(LEADING_NUMBER, r"\A\s*(-?\d*\.?\d+)");
re!(FIRST_NUMBER, r"\A\s*([-+\d.eE]+)");
re!(PROP_AFTER, r"\A\s*:\s*([^;]+)");
re!(COLOUR, r"\A\s*#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})\s*\z");
re!(WEIGHT, r"\A[1-9]00\z");

/// figkit `strip_comments`: the stylesheet without `/* … */` comments.
pub fn strip_comments(css: &str) -> String {
    COMMENT.replace_all(css, "").into_owned()
}

/// A rule: its selector (whitespace collapsed) and its declarations (property, value).
pub type Rule = (String, Vec<(String, String)>);

/// figkit `parse`: the rules of a comment-free stylesheet, in order.
pub fn parse(css: &str) -> Vec<Rule> {
    RULE.captures_iter(css)
        .map(|m| {
            let decls = m[2]
                .split(';')
                .filter_map(|part| {
                    part.split_once(':')
                        .map(|(p, v)| (py::strip(p).to_string(), py::collapse(v)))
                })
                .collect();
            (py::collapse(&m[1]), decls)
        })
        .collect()
}

/// figkit `_num`: a number as CSS writes it, at most four decimals, no trailing zeros.
pub fn num(x: f64) -> String {
    py::num(x)
}

/// The `em` lengths of a value, as the original's `(?<![\w#-])(-?\d*\.?\d+)em\b` finds them:
/// (start, end, number) for each, left to right.
fn em_matches(value: &str) -> Vec<(usize, usize, String)> {
    let mut out = vec![];
    let mut i = 0;
    while i < value.len() {
        let before = value[..i].chars().next_back();
        let ok_before = !before.is_some_and(|c| py::is_word(c) || c == '#' || c == '-');
        if ok_before {
            if let Some(m) = EM_AT.captures(&value[i..]) {
                let end = i + m.get(0).unwrap().end();
                let after = value[end..].chars().next();
                if !after.is_some_and(py::is_word) {
                    out.push((i, end, m[1].to_string()));
                    i = end;
                    continue;
                }
            }
        }
        i += value[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// The original's error for a colour `int(…, 16)` cannot read: the first slice it fails on.
pub fn hex_error(h: &str) -> StyleError {
    let slice = colour::hex_slices(h)
        .into_iter()
        .find(|s| colour::hex_slice(s).is_none())
        .unwrap_or_default();
    StyleError::value(format!(
        "invalid literal for int() with base 16: {}",
        py::repr(&slice)
    ))
}
fn float(s: &str) -> Result<f64> {
    py::float(s).map_err(StyleError::value)
}

/// An insertion-ordered map, as a Python dict.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tokens(Vec<(String, String)>);
impl Tokens {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn set(&mut self, name: &str, value: String) {
        match self.0.iter_mut().find(|(k, _)| k == name) {
            Some(e) => e.1 = value,
            None => self.0.push((name.to_string(), value)),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A stylesheet, flattened: its tokens as literal values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Style {
    /// The stylesheet's name (the original: its file's stem).
    pub name: String,
    /// The generator named in the flattened text's first line (figkit, chartgen …).
    pub maker: String,
    /// The identity of the mark library named by the first `@defs` directive found (the
    /// sheet's own, else its first import's that has one).
    pub defs: Option<String>,
    /// Every `:root` custom property, resolved, in the order of resolution.
    pub tokens: Tokens,
    /// Every other rule with its values resolved.
    pub rules: Vec<Rule>,
    /// The flattened stylesheet: literal values only.
    pub css: String,
}

struct Loader<'a> {
    name: &'a str,
    raw: Tokens,
    tokens: Tokens,
    max_bytes: usize,
    max_depth: usize,
    expanded: usize,
}

impl Loader<'_> {
    fn token(&mut self, name: &str, stack: &[String]) -> Result<Option<String>> {
        if stack.len() >= self.max_depth {
            return Err(budget_error());
        }
        if let Some(v) = self.tokens.get(name) {
            return Ok(Some(v.to_string()));
        }
        if stack.iter().any(|s| s == name) {
            return Err(StyleError::value(format!(
                "style {}: token {name} refers to itself ({})",
                self.name,
                stack.join(" → ")
            )));
        }
        let Some(raw) = self.raw.get(name).map(str::to_string) else {
            return Ok(None);
        };
        let mut inner = stack.to_vec();
        inner.push(name.to_string());
        let value = self.resolve(&raw, &inner)?;
        self.expanded = self
            .expanded
            .checked_add(value.len())
            .filter(|n| *n <= self.max_bytes)
            .ok_or_else(budget_error)?;
        self.tokens.set(name, value.clone());
        Ok(Some(value))
    }

    fn resolve(&mut self, value: &str, stack: &[String]) -> Result<String> {
        let mut value = value.to_string();
        while VAR.is_match(&value) {
            let mut out = String::new();
            let mut last = 0;
            let caps: Vec<_> = VAR
                .captures_iter(&value)
                .map(|m| {
                    (
                        m.get(0).unwrap().range(),
                        m[1].to_string(),
                        m.get(2).map(|g| g.as_str().to_string()),
                    )
                })
                .collect();
            for (range, name, fallback) in caps {
                bounded_push(&mut out, &value[last..range.start], self.max_bytes)?;
                let v = match self.token(&name, stack)? {
                    Some(v) => v,
                    None => match fallback {
                        None => {
                            return Err(StyleError::value(format!(
                                "style {}: undefined token {name}",
                                self.name
                            )));
                        }
                        Some(f) => self.resolve(&f, stack)?,
                    },
                };
                bounded_push(&mut out, &v, self.max_bytes)?;
                last = range.end;
            }
            bounded_push(&mut out, &value[last..], self.max_bytes)?;
            value = out;
        }
        let ems = em_matches(&value);
        if !ems.is_empty() {
            let label = self
                .token("--label-size", stack)?
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "10".into());
            let size = float(
                &LEADING_NUMBER
                    .captures(&label)
                    .ok_or_else(StyleError::none_group)?[1],
            )?;
            let mut out = String::new();
            let mut last = 0;
            for (start, end, number) in ems {
                bounded_push(&mut out, &value[last..start], self.max_bytes)?;
                bounded_push(&mut out, &num(float(&number)? * size), self.max_bytes)?;
                last = end;
            }
            bounded_push(&mut out, &value[last..], self.max_bytes)?;
            value = out;
        }
        while FUNC.is_match(&value) {
            let mut out = String::new();
            let mut last = 0;
            let caps: Vec<_> = FUNC
                .captures_iter(&value)
                .map(|m| {
                    (
                        m.get(0).unwrap().range(),
                        m[1].to_string(),
                        m[2].to_string(),
                    )
                })
                .collect();
            for (range, func, args) in caps {
                bounded_push(&mut out, &value[last..range.start], self.max_bytes)?;
                bounded_push(&mut out, &colour_function(&func, &args)?, self.max_bytes)?;
                last = range.end;
            }
            bounded_push(&mut out, &value[last..], self.max_bytes)?;
            value = out;
        }
        Ok(value)
    }

    fn literal(&mut self, prop: &str, value: &str) -> Result<String> {
        let mut value = self.resolve(value, &[])?;
        if prop == "font-size" && NUMBER_ONLY.is_match(&value) {
            value.push_str("px");
        }
        Ok(value)
    }
}

fn colour_function(func: &str, args: &str) -> Result<String> {
    let args: Vec<&str> = args.split(',').map(py::strip).collect();
    let arg = |i: usize| -> Result<&str> {
        args.get(i)
            .copied()
            .ok_or_else(|| StyleError::new(ErrorKind::Index, "list index out of range"))
    };
    let hex = |h: &str, r: Option<String>| r.ok_or_else(|| hex_error(h));
    match func {
        "mix" => {
            let (a, b, t) = (arg(0)?, arg(1)?, float(arg(2)?)?);
            if colour::hex_rgb(a).is_none() {
                return Err(hex_error(a));
            }
            hex(b, colour::mix(a, b, t))
        }
        "oklch-light" => {
            let (c, l) = (arg(0)?, float(arg(1)?)?);
            let cmax = if args.len() > 2 {
                Some(float(arg(2)?)?)
            } else {
                None
            };
            hex(c, colour::oklch_light(c, l, cmax))
        }
        _ => {
            let (c, dl, dc) = (arg(0)?, float(arg(1)?)?, float(arg(2)?)?);
            hex(c, colour::oklch_shift(c, dl, dc))
        }
    }
}

/// The text of a sheet with its imports inlined, and its `@defs` directive (figkit `_read`).
fn read(
    source: &mut dyn StyleSource,
    id: &str,
    seen: &mut HashSet<String>,
    limits: &figkit_model::Limits,
    total: &mut usize,
) -> Result<(String, Option<String>)> {
    if !seen.insert(id.to_string()) {
        return Ok((String::new(), None));
    }
    if seen.len() > limits.max_style_imports {
        return Err(budget_error());
    }
    let text = source.read(id)?;
    *total = total
        .checked_add(text.len())
        .filter(|n| *n <= limits.max_style_bytes)
        .ok_or_else(budget_error)?;
    let mut defs = match DEFS.captures(&text) {
        Some(m) => Some(source.resolve(id, &m[1], Link::Defs)?),
        None => None,
    };
    let mut out = String::new();
    let mut last = 0;
    for m in IMPORT.captures_iter(&text) {
        let whole = m.get(0).unwrap();
        out.push_str(&text[last..whole.start()]);
        let target = (1..=3)
            .filter_map(|i| m.get(i))
            .map(|g| g.as_str())
            .find(|g| !g.is_empty())
            .unwrap_or_default();
        let child = source.resolve(id, target, Link::Import)?;
        let (inner, inner_defs) = read(source, &child, seen, limits, total)?;
        if defs.is_none() {
            defs = inner_defs;
        }
        out.push_str(&inner);
        last = whole.end();
    }
    out.push_str(&text[last..]);
    Ok((out, defs))
}

/// figkit `Style.metric` of a token's literal value: its leading number, or the original's
/// error naming the style and token.
pub fn metric_value(style: &str, name: &str, value: &str) -> Result<f64> {
    let m = LEADING_NUMBER.captures(value).ok_or_else(|| {
        StyleError::value(format!(
            "style {style}: token {name} = {} is not a number",
            py::repr(value)
        ))
    })?;
    float(&m[1])
}

impl Style {
    /// Read the sheet `root` (an identity of `source`), inline its imports and flatten it.
    /// `name` is the style's name (errors and the first line name it); `maker` the generator.
    pub fn load(name: &str, maker: &str, root: &str, source: &mut dyn StyleSource) -> Result<Self> {
        Self::load_bounded(name, maker, root, source, &figkit_model::Limits::default())
    }

    /// Resolve imports and expansions under caller-selected finite limits.
    pub fn load_bounded(
        name: &str,
        maker: &str,
        root: &str,
        source: &mut dyn StyleSource,
        limits: &figkit_model::Limits,
    ) -> Result<Self> {
        let (raw, defs) = read(source, root, &mut HashSet::new(), limits, &mut 0)?;
        Self::from_flat_source_bounded(name, maker, &raw, defs, limits)
    }
    /// Flatten stylesheet text whose imports are already inlined.
    pub fn from_flat_source(
        name: &str,
        maker: &str,
        raw: &str,
        defs: Option<String>,
    ) -> Result<Self> {
        Self::from_flat_source_bounded(name, maker, raw, defs, &figkit_model::Limits::default())
    }
    pub fn from_flat_source_bounded(
        name: &str,
        maker: &str,
        raw: &str,
        defs: Option<String>,
        limits: &figkit_model::Limits,
    ) -> Result<Self> {
        if raw.len() > limits.max_style_bytes {
            return Err(budget_error());
        }
        let rules = parse(&strip_comments(raw));
        let mut raw_tokens = Tokens::default();
        for (sel, decls) in &rules {
            if sel == ":root" {
                for (p, v) in decls {
                    if p.starts_with("--") {
                        raw_tokens.set(p, v.clone());
                    }
                }
            }
        }
        let mut loader = Loader {
            name,
            raw: raw_tokens.clone(),
            tokens: Tokens::default(),
            max_bytes: limits.max_style_bytes,
            max_depth: limits.max_depth,
            expanded: 0,
        };
        for (token, _) in raw_tokens.iter() {
            loader.token(token, &[])?;
        }
        let mut out_rules = vec![];
        for (sel, decls) in rules {
            if sel == ":root" {
                continue;
            }
            let mut d = vec![];
            for (p, v) in decls {
                let lit = loader.literal(&p, &v)?;
                loader.expanded = loader
                    .expanded
                    .checked_add(lit.len())
                    .filter(|n| *n <= limits.max_style_bytes)
                    .ok_or_else(budget_error)?;
                d.push((p, lit));
            }
            out_rules.push((sel, d));
        }
        let mut style = Self {
            name: name.into(),
            maker: maker.into(),
            defs,
            tokens: loader.tokens,
            rules: out_rules,
            css: String::new(),
        };
        style.css = style.flatten();
        if style.css.len() > limits.max_style_bytes {
            return Err(budget_error());
        }
        Ok(style)
    }

    fn flatten(&self) -> String {
        let mut lines = vec![
            format!(
                "/* Flattened by {} from the {} stylesheet: literal values only */",
                self.maker, self.name
            ),
            ":root {".into(),
        ];
        lines.extend(self.tokens.iter().map(|(k, v)| format!("  {k}: {v};")));
        lines.push("}".into());
        for (sel, decls) in &self.rules {
            let body: Vec<String> = decls.iter().map(|(p, v)| format!("{p}: {v};")).collect();
            lines.push(format!("{sel} {{ {} }}", body.join(" ")));
        }
        lines.join("\n") + "\n"
    }

    /// A metric token in points (or a plain fraction): its leading number.
    pub fn metric(&self, name: &str) -> Result<f64> {
        let v = self.tokens.get(name).ok_or_else(|| {
            StyleError::new(
                ErrorKind::Key,
                format!("style {} has no token {name}", self.name),
            )
        })?;
        metric_value(&self.name, name, v)
    }
    /// A paint token's literal value.
    pub fn paint(&self, name: &str) -> Option<&str> {
        self.tokens.get(name)
    }
    pub fn has(&self, name: &str) -> bool {
        self.tokens.get(name).is_some()
    }

    /// The engine's theme: the label size, text, grid and page paints, the 16 palette slots
    /// (empty where the style has none) and every token.
    pub fn theme(&self) -> Result<Theme> {
        let paint = |name: &str, default: &str| {
            self.paint(name)
                .filter(|v| !v.is_empty())
                .unwrap_or(default)
                .to_string()
        };
        Ok(Theme {
            name: self.name.clone(),
            font_family: "New Computer Modern".into(),
            font_size: self.metric("--label-size")?,
            text: paint("--text", "#595959"),
            grid: paint("--grid", "#dddddd"),
            background: paint("--page", "#ffffff"),
            palette: (1..=16)
                .map(|i| paint(&format!("--series-{i}"), ""))
                .collect(),
            tokens: self
                .tokens
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
                .collect(),
            stylesheet: self.css.clone(),
        })
    }
}

// ---- bake.label_style -------------------------------------------------------------------------

/// A label's font weight as Typst takes it (None is regular).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Weight {
    Named(NamedWeight),
    Numeric(u16),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedWeight {
    Bold,
    Light,
}
impl Weight {
    pub fn as_css(&self) -> String {
        match self {
            Self::Named(NamedWeight::Bold) => "bold".into(),
            Self::Named(NamedWeight::Light) => "light".into(),
            Self::Numeric(n) => n.to_string(),
        }
    }
}

/// The typography a label is typeset with (bake `label_style`): size in points, `#rrggbb` fill,
/// weight (None: regular) and slant (`italic`/`oblique`; None: upright).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelStyle {
    pub size: f64,
    pub fill: String,
    pub weight: Option<Weight>,
    pub slant: Option<String>,
}
impl LabelStyle {
    /// The measured-text style this typography asks of a host: weight `normal`, `bold`,
    /// `light` or a number; font style `normal`, `italic` or `oblique`. Hosts and renderers
    /// that support fewer refuse the rest explicitly.
    pub fn text_style(&self) -> figkit_model::TextStyle {
        figkit_model::TextStyle {
            fill: Some(self.fill.clone()),
            weight: self
                .weight
                .map_or_else(|| "normal".to_string(), |w| w.as_css()),
            font_style: self.slant.clone().unwrap_or_else(|| "normal".into()),
        }
    }
}

const REGEX_SYNTAX: &str = r".^$*+?{}[]\|()";

fn mentions_class(selectors: &str, cls: &str) -> Result<bool> {
    if cls.chars().any(|c| REGEX_SYNTAX.contains(c)) {
        return Err(StyleError::value(format!(
            "label class {} holds regular-expression syntax, which bake.label_style would \
             interpret as a pattern: not supported",
            py::repr(cls)
        )));
    }
    let needle = format!(".{cls}");
    Ok(selectors.match_indices(&needle).any(|(i, _)| {
        !selectors[i + needle.len()..]
            .chars()
            .next()
            .is_some_and(|c| py::is_word(c) || c == '-')
    }))
}

fn property_value(body: &str, prop: &str) -> Option<String> {
    let mut from = 0;
    while let Some(off) = body[from..].find(prop) {
        let i = from + off;
        let before = body[..i].chars().next_back();
        if !before.is_some_and(|c| py::is_word(c) || c == '-') {
            if let Some(m) = PROP_AFTER.captures(&body[i + prop.len()..]) {
                return Some(m[1].to_string());
            }
        }
        from = i + body[i..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// bake `stylesheet_value`: the value the first rule whose selector mentions `.cls` gives the
/// property (no cascade), stripped.
pub fn stylesheet_value(css: &str, cls: &str, prop: &str) -> Result<Option<String>> {
    for m in RULE.captures_iter(css) {
        if mentions_class(&m[1], cls)? {
            if let Some(v) = property_value(&m[2], prop) {
                return Ok(Some(py::strip(&v).to_string()));
            }
        }
    }
    Ok(None)
}

/// bake `font_size`: user units (points here); an explicit `pt` converted as CSS does (× 4/3).
pub fn font_size(value: &str) -> Result<f64> {
    let m = FIRST_NUMBER
        .captures(value)
        .ok_or_else(StyleError::none_group)?;
    let n = float(&m[1])?;
    Ok(if py::strip(value).ends_with("pt") {
        n * 4. / 3.
    } else {
        n
    })
}
/// bake `colour_value`: a `#rgb`/`#rrggbb` colour as lower-case `#rrggbb`, or None.
pub fn colour_value(value: &str) -> Option<String> {
    let m = COLOUR.captures(value)?;
    let h = &m[1];
    Some(if h.len() == 3 {
        format!("#{}", h.chars().flat_map(|c| [c, c]).collect::<String>()).to_lowercase()
    } else {
        format!("#{h}").to_lowercase()
    })
}
/// bake `weight_value`.
pub fn weight_value(value: &str) -> Option<Weight> {
    let v = py::strip(value).to_lowercase();
    match v.as_str() {
        "bold" | "bolder" => Some(Weight::Named(NamedWeight::Bold)),
        "lighter" => Some(Weight::Named(NamedWeight::Light)),
        "400" => None,
        _ if WEIGHT.is_match(&v) => v.parse().ok().map(Weight::Numeric),
        _ => None,
    }
}
/// bake `slant_value`.
pub fn slant_value(value: &str) -> Option<String> {
    let v = py::strip(value).to_lowercase();
    matches!(v.as_str(), "italic" | "oblique").then_some(v)
}

/// bake `label_style(css, classes, own)`: the size, fill, weight and slant a label with these
/// classes is typeset with, from the flattened stylesheet. Each is the label's own value if it
/// has one, otherwise the value the stylesheet gives the first of its classes that has one,
/// trying the classes in order with `math` or `prose` last; for each class the first rule whose
/// selector mentions it and sets the property counts. A value that does not convert falls
/// through to the next source. Defaults: 10, `#000000`, regular, upright. Lookups are lazy, as
/// the original's: a later source that would fail is never read once one gives a value.
pub fn label_style<S: AsRef<str>>(
    css: &str,
    classes: &[S],
    own: &BTreeMap<String, String>,
) -> Result<LabelStyle> {
    let classes: Vec<&str> = classes.iter().map(AsRef::as_ref).collect();
    let mode = if classes.contains(&"math") {
        Some("math")
    } else if classes.contains(&"prose") {
        Some("prose")
    } else {
        None
    };
    let mut order: Vec<&str> = classes
        .iter()
        .copied()
        .filter(|c| Some(*c) != mode)
        .collect();
    order.extend(mode);

    fn lookup<T>(
        css: &str,
        order: &[&str],
        own: &BTreeMap<String, String>,
        prop: &str,
        convert: impl Fn(&str) -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        let own_value = own.get(prop).cloned();
        let sources = std::iter::once(None).chain(order.iter().map(Some));
        for source in sources {
            let found = match source {
                None => own_value.clone(),
                Some(c) => stylesheet_value(css, c, prop)?,
            };
            if let Some(f) = found.filter(|f| !f.is_empty()) {
                if let Some(v) = convert(&f)? {
                    return Ok(Some(v));
                }
            }
        }
        Ok(None)
    }
    // a size of zero is false in the original's truth test and falls through
    let size = lookup(css, &order, own, "font-size", |v| {
        font_size(v).map(|n| (n != 0.).then_some(n))
    })?
    .unwrap_or(10.);
    let fill = lookup(css, &order, own, "fill", |v| Ok(colour_value(v)))?
        .unwrap_or_else(|| "#000000".into());
    let weight = lookup(css, &order, own, "font-weight", |v| Ok(weight_value(v)))?;
    let slant = lookup(css, &order, own, "font-style", |v| Ok(slant_value(v)))?;
    Ok(LabelStyle {
        size,
        fill,
        weight,
        slant,
    })
}

/// Chart-free legacy token projection for the historical Python dictionary.
pub mod legacy {
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Theme {
        pub name: String,
        pub font_family: String,
        pub font_size: f64,
        pub text: String,
        pub grid: String,
        pub background: String,
        pub palette: Vec<String>,
        pub tokens: BTreeMap<String, serde_json::Value>,
        pub stylesheet: String,
    }
}

fn budget_error() -> StyleError {
    StyleError::value("style expansion budget exceeded")
}
fn bounded_push(out: &mut String, value: &str, limit: usize) -> Result<()> {
    out.len()
        .checked_add(value.len())
        .filter(|n| *n <= limit)
        .ok_or_else(budget_error)?;
    out.push_str(value);
    Ok(())
}

/// Portable style entry point with typed resource-budget failures.
pub fn load_checked(
    name: &str,
    maker: &str,
    root: &str,
    source: &mut dyn StyleSource,
    limits: &figkit_model::Limits,
) -> figkit_model::FigureResult<Style> {
    Style::load_bounded(name, maker, root, source, limits).map_err(|error| {
        let kind = if error.message == "style expansion budget exceeded" {
            figkit_model::ErrorKind::BudgetExceeded
        } else if error.kind == ErrorKind::Source {
            figkit_model::ErrorKind::ResourceFailure
        } else {
            figkit_model::ErrorKind::InvalidInput
        };
        figkit_model::FigureError::new(kind, error.message)
    })
}
/// Resolve the exact typography and ink to retain in a portable measurement request.
/// The font environment and chosen family belong to the caller, never bundled chart defaults.
pub fn resolve_text<S: AsRef<str>>(
    css: &str,
    classes: &[S],
    own: &BTreeMap<String, String>,
    family: &str,
) -> Result<(figkit_model::Typography, figkit_model::TextPaint)> {
    let label = label_style(css, classes, own)?;
    let style = label.text_style();
    Ok((
        figkit_model::Typography {
            font_family: family.to_owned(),
            font_size: label.size,
            weight: style.weight,
            font_style: style.font_style,
        },
        figkit_model::TextPaint::Resolved { fill: label.fill },
    ))
}
