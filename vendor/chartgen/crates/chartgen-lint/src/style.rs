//! The original's style lint (charts/chartgen/style.py: lint_style, lint_notes, lint_ramps): a
//! flattened style's values checked against the locked chart rules. Text contrast (R5-8);
//! non-data strokes light and lighter than every data colour (R1-02); regular palette slots
//! muted, balanced and free of grey (R3-2, R3-6, R3-16); the notes' sizes and marks (R5-7,
//! CG-8); the ramps' steps, neutral and cell text (R3-9, R3-14, R3-15, R5-8).
//!
//! The input is the flattened style: its tokens as literal values and its rules with literal
//! declarations, as the original figkit `Style` produces them (`tokens`, `rules`). The
//! `chartgen-style` crate owns flattening; this module only reads the result.
use chartgen_colour_space::{contrast, luminance, oklch};
use chartgen_model::Finding;
use std::collections::BTreeMap;

/// A flattened style: literal token values and rules `(selector, [(property, value)])`.
#[derive(Debug, Clone, Default)]
pub struct FlatStyle {
    pub tokens: BTreeMap<String, String>,
    pub rules: Vec<(String, Vec<(String, String)>)>,
}

impl FlatStyle {
    pub fn has(&self, name: &str) -> bool {
        self.tokens.contains_key(name)
    }
    pub fn paint(&self, name: &str) -> Option<&str> {
        self.tokens.get(name).map(String::as_str)
    }
    /// A metric token in points (its leading number), as figkit's `Style.metric`.
    pub fn metric(&self, name: &str) -> Option<f64> {
        let v = self.tokens.get(name)?.trim_start();
        let end = v
            .char_indices()
            .take_while(|(i, c)| c.is_ascii_digit() || *c == '.' || (*i == 0 && *c == '-'))
            .map(|(i, c)| i + c.len_utf8())
            .last()?;
        v[..end].parse().ok()
    }
}

/// Python's `format(x, "g")`: six significant digits, trailing zeros dropped.
pub fn fmt_g(x: f64) -> String {
    if x == 0. {
        return "0".into();
    }
    let exp = x.abs().log10().floor() as i32;
    if !(-4..6).contains(&exp) {
        let s = format!("{:.5e}", x);
        let (m, e) = s.split_once('e').unwrap();
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap();
        return format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let decimals = (5 - exp).max(0) as usize;
    let s = format!("{:.*}", decimals, x);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

fn warn(rule: &str, message: String) -> Finding {
    // the rule's level from the guide (rules.level_of): every rule here is locked
    Finding::new(rule, "warn", message)
}

/// The findings of the style lint, in the original's order.
pub fn lint_style(style: &FlatStyle) -> Vec<Finding> {
    let mut out = vec![];
    let page = style.paint("--page").unwrap_or("#ffffff");
    for tok in [
        "--text",
        "--note",
        "--comment",
        "--good-text",
        "--bad-text",
        "--good-cvd-text",
        "--bad-cvd-text",
        "--note-mark-ink",
    ] {
        let Some(v) = style.paint(tok) else { continue };
        if let Some(c) = contrast(v, page) {
            if c < 4.5 {
                out.push(warn(
                    "R5-8",
                    format!("{tok} {v} has contrast {c:.2} : 1 on the page (< 4.5)"),
                ));
            }
        }
    }
    let mut data: Vec<&str> = (1..6)
        .filter_map(|k| style.paint(&format!("--series-{k}")))
        .collect();
    if let Some(s) = style.paint("--single") {
        data.push(s);
    }
    for tok in ["--grid", "--zero-line", "--divider", "--inset-frame"] {
        let Some(g) = style.paint(tok) else { continue };
        if let Some(c) = contrast(g, page) {
            if c > 1.5 {
                out.push(warn(
                    "R1-02",
                    format!("{tok} {g} has contrast {c:.2} : 1 (> 1.5)"),
                ));
            }
        }
        let lg = luminance(g);
        if let Some(d) = data
            .iter()
            .find(|d| luminance(d).zip(lg).is_some_and(|(a, b)| a >= b))
        {
            out.push(warn(
                "R1-02",
                format!("{tok} {g} is not lighter than the data colour {d}"),
            ));
        }
    }
    let mut ls = vec![];
    for k in 1..6 {
        let Some(c) = style.paint(&format!("--series-{k}")) else {
            continue;
        };
        let Some([l, ch, _]) = oklch(c) else { continue };
        ls.push(l);
        if ch > 0.15 {
            out.push(warn(
                "R3-2",
                format!("slot {k} {c} has OKLCH chroma {ch:.3} (> 0.15)"),
            ));
        }
        if ch < 0.02 {
            out.push(warn(
                "R3-16",
                format!("slot {k} {c} is grey, a colour reserved for special categories"),
            ));
        }
    }
    if !ls.is_empty() {
        let hi = ls.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo = ls.iter().cloned().fold(f64::INFINITY, f64::min);
        if hi - lo > 0.12 {
            out.push(warn(
                "R3-6",
                format!(
                    "slots 1–5 spread {:.3} in OKLCH lightness (> 0.12)",
                    hi - lo
                ),
            ));
        }
    }
    out.extend(lint_notes(style));
    out.extend(lint_ramps(style));
    out
}

/// lint_notes: the notes and their marks at least 0.8 em (R5-7, G6.4); the marks' sequence one
/// the generator knows (CG-8).
pub fn lint_notes(style: &FlatStyle) -> Vec<Finding> {
    let mut out = vec![];
    let Some(em) = style.metric("--label-size") else {
        return out;
    };
    for tok in ["--note-size", "--note-mark-size"] {
        if let Some(v) = style.metric(tok) {
            if v < 0.8 * em - 1e-6 {
                out.push(warn(
                    "R5-7",
                    format!(
                        "{tok} {} pt is under the notes' floor of 0.8 em ({} pt): a note or its mark too small to read (G6.4, D-6)",
                        fmt_g(v),
                        fmt_g(0.8 * em)
                    ),
                ));
            }
        }
    }
    let kind = style.paint("--note-marks").unwrap_or("letters").trim();
    if !matches!(kind, "letters" | "numerals" | "symbols") {
        out.push(warn(
            "CG-8",
            format!("--note-marks '{kind}' is not letters, numerals or symbols: letters are used"),
        ));
    }
    out
}

/// lint_ramps: each ramp darker step by step away from its light end or its neutral (R3-9,
/// R3-14); a diverging ramp's neutral quiet and no bright yellow or amber beside it (R3-14,
/// R3-15); the text grey at 4.5 : 1 on a cell that carries its value and on a bar under a
/// cell's number (R5-8); a group's name legible in its hue (R5-8).
pub fn lint_ramps(style: &FlatStyle) -> Vec<Finding> {
    let mut out = vec![];
    if !style.has("--seq-1") {
        return out;
    }
    let text = style.paint("--text").unwrap_or("#595959");
    let page = style.paint("--page").unwrap_or("#ffffff");
    let side = |prefix: &str| -> Vec<String> {
        std::iter::once("--div-0".to_owned())
            .chain((1..5).map(|k| format!("{prefix}{k}")))
            .collect()
    };
    let ramps: [(&str, Vec<String>); 5] = [
        (
            "sequential",
            (1..10).map(|k| format!("--seq-{k}")).collect(),
        ),
        ("diverging below", side("--div-m")),
        ("diverging above", side("--div-p")),
        ("good", side("--good-")),
        ("bad", side("--bad-")),
    ];
    for (name, toks) in ramps {
        let toks: Vec<&String> = toks.iter().filter(|t| style.has(t)).collect();
        let ls: Vec<f64> = toks
            .iter()
            .map(|t| style.paint(t).and_then(oklch).map_or(f64::NAN, |c| c[0]))
            .collect();
        if let Some(i) = (0..toks.len().saturating_sub(1)).find(|&i| ls[i + 1] >= ls[i]) {
            out.push(warn(
                if name == "sequential" { "R3-9" } else { "R3-14" },
                format!(
                    "the {name} ramp does not darken step by step: {} is not darker than {} (darker must mean more, or further from the neutral)",
                    toks[i + 1],
                    toks[i]
                ),
            ));
        }
    }
    if let Some(n) = style.paint("--div-0") {
        if let Some([ln, cn, _]) = oklch(n) {
            if ln < 0.9 || cn > 0.03 {
                out.push(warn(
                    "R3-14",
                    format!(
                        "--div-0 {n} is not a quiet neutral (OKLCH L {ln:.2}, C {cn:.3}: white or a light grey)"
                    ),
                ));
            }
        }
        for tok in ["--div-m1", "--div-p1", "--good-1", "--bad-1"] {
            let Some(v) = style.paint(tok) else { continue };
            if let Some([_, c, h]) = oklch(v) {
                if (60. ..=110.).contains(&h) && c > 0.08 {
                    out.push(warn(
                        "R3-15",
                        format!(
                            "{tok} {v} is a bright yellow or amber beside the dividing value, which deserves the least attention"
                        ),
                    ));
                }
            }
        }
    }
    for (sel, decls) in &style.rules {
        if !sel.contains(".labelled") && !sel.contains(".cell-bar") {
            continue;
        }
        for (p, v) in decls {
            if p != "fill" || !v.starts_with('#') {
                continue;
            }
            if let Some(c) = contrast(text, v).filter(|c| *c < 4.5) {
                out.push(warn(
                    "R5-8",
                    format!(
                        "{sel}: the text grey has {c:.2} : 1 on {v} (< 4.5), a cell's value would be hard to read"
                    ),
                ));
            }
        }
    }
    for k in 1..7 {
        let tok = format!("--series-{k}-text");
        let Some(v) = style.paint(&tok) else { continue };
        if let Some(c) = contrast(v, page).filter(|c| *c < 4.5) {
            out.push(warn(
                "R5-8",
                format!(
                    "{tok} {v} has {c:.2} : 1 on the page (< 4.5): a group's name in the grouped legend"
                ),
            ));
        }
    }
    out
}
