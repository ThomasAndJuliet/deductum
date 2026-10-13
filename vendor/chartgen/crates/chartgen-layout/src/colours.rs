//! Resolve the colour plan's classes (the original's `single`, `series-k`, reserved role
//! classes, `seq-k`, `div-*`, `tint`, shades) to physical paints from the style's tokens, as
//! the original's base stylesheet paints them. A class whose token the style lacks is an
//! explicit error; nothing wraps to another palette colour.
use super::*;

fn token(theme: &Theme, key: &str) -> Option<String> {
    theme
        .tokens
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

fn slot(theme: &Theme, number: u8) -> Result<String> {
    if !(1..=16).contains(&number) {
        return Err(err(format!("palette slot {number} is outside 1–16")));
    }
    token(theme, &format!("--series-{number}"))
        .or_else(|| {
            theme
                .palette
                .get(usize::from(number - 1))
                .filter(|p| !p.is_empty())
                .cloned()
        })
        .ok_or_else(|| err(format!("theme has no paint for palette slot {number}")))
}

fn need(theme: &Theme, key: &str) -> Result<String> {
    token(theme, key).ok_or_else(|| err(format!("theme has no paint {key}")))
}

/// A reserved grey: the style's token, else (a theme without the original's tokens) its text grey.
fn grey(theme: &Theme, key: &str) -> String {
    token(theme, key).unwrap_or_else(|| theme.text.clone())
}

/// Whether the style draws good and bad in the colour-blind-safe pair (--good-bad-mode).
pub fn safe_pair(theme: &Theme, spec: &Spec) -> bool {
    token(theme, "--good-bad-mode").as_deref().map(str::trim) == Some("safe-pair")
        && spec.raw["audience"]["cvd"].as_str() != Some("no")
}

/// The paint of a mark carrying these colour classes (its colour class first), as the base
/// stylesheet resolves them; `line` for a stroked line mark (`.line.single`, `.line.total`).
pub fn paint_of(theme: &Theme, classes: &[String], line: bool, safe: bool) -> Result<String> {
    let has = |c: &str| classes.iter().any(|x| x == c);
    let Some(first) = classes.first() else {
        return Err(err("mark has no colour class"));
    };
    let first = first.as_str();
    let side = if has("good") {
        Some("good")
    } else if has("bad") {
        Some("bad")
    } else {
        None
    };
    if let Some(step) = first.strip_prefix("seq-") {
        return need(theme, &format!("--seq-{step}"));
    }
    if first == "div-0" {
        return need(theme, "--div-0");
    }
    if let Some(rest) = first.strip_prefix("div-") {
        let k = &rest[1..];
        return match side {
            Some(s) if safe => need(theme, &format!("--{s}-cvd-{k}")),
            Some(s) => need(theme, &format!("--{s}-{k}")),
            None => need(theme, &format!("--div-{rest}")),
        };
    }
    match first {
        "single" => {
            if line {
                token(theme, "--single-line")
                    .map(Ok)
                    .unwrap_or_else(|| need(theme, "--single"))
            } else {
                token(theme, "--single")
                    .or_else(|| theme.palette.first().cloned())
                    .ok_or_else(|| err("theme has no paint --single"))
            }
        }
        "average" => Ok(grey(theme, "--average")),
        "total" if line => Ok(grey(theme, "--average")),
        "total" => Ok(grey(theme, "--total")),
        "all-others" => Ok(grey(theme, "--all-others")),
        "missing" => Ok(grey(theme, "--missing")),
        "reference" if line => Ok(grey(theme, "--average")),
        "reference" => {
            Ok(token(theme, "--reference-line").unwrap_or_else(|| grey(theme, "--average")))
        }
        "uniform-context" => need(theme, "--uniform-context"),
        "good" | "bad" if safe => need(theme, &format!("--{first}-cvd")),
        "good" | "bad" => need(theme, &format!("--{first}")),
        "censored" => Ok(token(theme, "--censored").unwrap_or_else(|| grey(theme, "--missing"))),
        c => {
            let k: u8 = c
                .strip_prefix("series-")
                .and_then(|k| k.parse().ok())
                .ok_or_else(|| err(format!("unknown colour class {c}")))?;
            let base = slot(theme, k)?;
            // a modifier the style has no rule for leaves the base colour (as the stylesheet does)
            if has("tint") {
                return Ok(token(theme, &format!("--series-{k}-tint")).unwrap_or(base));
            }
            if has("shade-light") {
                return Ok(token(theme, &format!("--series-{k}-shade-light")).unwrap_or(base));
            }
            if has("shade-dark") {
                return Ok(token(theme, &format!("--series-{k}-strong")).unwrap_or(base));
            }
            Ok(base)
        }
    }
}

/// The strong shade of a colour class (a highlight, a flat reference marker across a bar).
pub fn strong_of(theme: &Theme, classes: &[String], line: bool) -> Result<String> {
    match classes.first().map(String::as_str) {
        Some("single") if line => token(theme, "--single-line-strong")
            .map(Ok)
            .unwrap_or_else(|| need(theme, "--single-strong")),
        Some("single") => need(theme, "--single-strong"),
        Some(c) if c.starts_with("series-") => need(theme, &format!("--{c}-strong")),
        _ => paint_of(theme, classes, line, false),
    }
}

/// The colour classes of a prepared series (empty when it has no palette colour or no plan was
/// compiled).
pub fn series_classes(plan: &SemanticPlan, index: usize) -> &[String] {
    plan.colors
        .series_classes
        .get(index)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// The colour classes of a category's part (pie slices, treemap tiles, a one-series bar chart's
/// bars, a waterfall's steps); empty for a part with no mark or no plan.
pub fn category_classes(plan: &SemanticPlan, index: usize) -> &[String] {
    plan.colors
        .category_classes
        .get(index)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// The classes of one heatmap cell, from the plan's ramp.
pub fn cell_classes(plan: &SemanticPlan, series: usize, category: usize) -> Option<&[String]> {
    plan.colors
        .ramp
        .as_ref()?
        .cells
        .get(series)?
        .get(category)
        .map(Vec::as_slice)
        .filter(|c| !c.is_empty())
}

fn legacy_series(plan: &SemanticPlan, theme: &Theme, index: usize) -> Result<String> {
    let n = u8::try_from(index + 1).map_err(|_| err("more than 16 series need color policy"))?;
    if plan.spec.raw["data"]["series"]
        .as_array()
        .is_some_and(|series| series.len() == 1)
    {
        return token(theme, "--single")
            .or_else(|| theme.palette.first().cloned())
            .ok_or_else(|| err("theme palette is empty"));
    }
    slot(theme, n)
}

/// The paint of a prepared series.
pub fn series(plan: &SemanticPlan, theme: &Theme, index: usize) -> Result<String> {
    if plan.colors.series_classes.is_empty() {
        return legacy_series(plan, theme, index);
    }
    let classes = plan
        .colors
        .series_classes
        .get(index)
        .ok_or_else(|| err("series paint index out of range"))?;
    if classes.is_empty() {
        return Err(err(format!(
            "series {index} has no palette colour in this {} chart",
            plan.chart_type.as_str()
        )));
    }
    paint_of(theme, classes, false, safe_pair(theme, &plan.spec))
}

/// The paint of a prepared series drawn as a line.
pub fn series_line(plan: &SemanticPlan, theme: &Theme, index: usize) -> Result<String> {
    if plan.colors.series_classes.is_empty() {
        return legacy_series(plan, theme, index);
    }
    let classes = series_classes(plan, index);
    if classes.is_empty() {
        return Err(err(format!("series {index} has no palette colour")));
    }
    paint_of(theme, classes, true, safe_pair(theme, &plan.spec))
}

pub fn tint(theme: &Theme, base: &str) -> Result<String> {
    // the original's tint: the colour 20 % of the way to the page (palette-muted.css)
    chartgen_format_free_mix(base, &theme.background, 0.2)
}

fn chartgen_format_free_mix(a: &str, b: &str, t: f64) -> Result<String> {
    fn rgb(s: &str) -> Result<[f64; 3]> {
        let hex = s
            .strip_prefix('#')
            .ok_or_else(|| err("category tint needs hex paints"))?;
        if hex.len() != 6 {
            return Err(err("category tint needs six-digit hex paints"));
        }
        let n = u32::from_str_radix(hex, 16).map_err(|_| err("invalid tint paint"))?;
        Ok([
            ((n >> 16) & 255) as f64 / 255.,
            ((n >> 8) & 255) as f64 / 255.,
            (n & 255) as f64 / 255.,
        ])
    }
    let (x, y) = (rgb(a)?, rgb(b)?);
    let c = |i: usize| {
        let v = (x[i] + (y[i] - x[i]) * t).clamp(0., 1.) * 255.;
        // Python's round(): halves to even
        let r = v.round();
        (if (v - v.trunc()).abs() == 0.5 && r % 2. != 0. {
            r - 1.
        } else {
            r
        }) as u8
    };
    Ok(format!("#{:02x}{:02x}{:02x}", c(0), c(1), c(2)))
}

/// The paint of one mark of series `si` at category `ci`: the category's own colour where the
/// plan colours marks by category (a one-series bar chart's reserved greys, a waterfall's rises
/// and falls), else the series' paint `series_paint`.
pub fn mark(
    plan: &SemanticPlan,
    theme: &Theme,
    si: usize,
    ci: usize,
    series_paint: &str,
) -> Result<String> {
    if let Some(classes) = mark_classes(plan, si, ci) {
        return paint_of(theme, classes, false, safe_pair(theme, &plan.spec));
    }
    let own = category_classes(plan, ci);
    let coloured_by_series = series_classes(plan, si).first().is_some_and(|c| {
        c == "reference" || plan.colors.category_classes.iter().all(Vec::is_empty)
    });
    if own.is_empty() || coloured_by_series {
        return Ok(series_paint.to_owned());
    }
    paint_of(theme, own, false, safe_pair(theme, &plan.spec))
}

/// The classes of one mark where they differ from its series' or category's (good and bad).
pub fn mark_classes(plan: &SemanticPlan, si: usize, ci: usize) -> Option<&[String]> {
    plan.colors
        .mark_classes
        .get(si)?
        .get(ci)
        .map(Vec::as_slice)
        .filter(|c| !c.is_empty())
}

/// The paint of a flat reference marker across a bar (base.css `.chart .target-mark`): the
/// strong shade of the bar's colour class carried after `reference`, else the reserved dark grey.
pub fn target_mark(plan: &SemanticPlan, theme: &Theme, si: usize) -> Result<String> {
    match series_classes(plan, si) {
        [first, bar, ..] if first == "reference" => {
            strong_of(theme, std::slice::from_ref(bar), false)
        }
        _ => Ok(grey(theme, "--average")),
    }
}

/// The colour classes of one mark of series `si` at category `ci`, as `mark` paints it.
pub fn mark_class_list(plan: &SemanticPlan, si: usize, ci: Option<usize>) -> Vec<String> {
    if let Some(ci) = ci {
        if let Some(c) = mark_classes(plan, si, ci) {
            return c.to_vec();
        }
        let own = category_classes(plan, ci);
        let by_series = series_classes(plan, si).first().is_some_and(|c| {
            c == "reference" || plan.colors.category_classes.iter().all(Vec::is_empty)
        });
        if !own.is_empty() && !by_series {
            return own.to_vec();
        }
    }
    series_classes(plan, si).to_vec()
}

/// Put the plan's colour classes on placed marks (the compatibility SVG's colour is its
/// classes): a mark identified `{series}-{category index}`, or a line by its `series` datum.
pub fn classify_marks(plan: &SemanticPlan, data: &PreparedData, marks: &mut [Primitive]) {
    for p in marks {
        let meta = match p {
            Primitive::Rect { meta, .. }
            | Primitive::Circle { meta, .. }
            | Primitive::Path { meta, .. }
            | Primitive::Line { meta, .. } => meta,
            _ => continue,
        };
        let found = data.series.iter().enumerate().find_map(|(si, s)| {
            if let Some(id) = &meta.id {
                let rest = id.strip_prefix(s.id.as_str())?.strip_prefix('-')?;
                return Some((si, Some(rest.parse::<usize>().ok()?)));
            }
            (meta.data.get("series") == Some(&s.id)).then_some((si, None))
        });
        let Some((si, ci)) = found else { continue };
        for c in mark_class_list(plan, si, ci).into_iter().rev() {
            if !meta.classes.contains(&c) {
                meta.classes.insert(0, c);
            }
        }
    }
}

/// The paint of a category's part.
pub fn category(plan: &SemanticPlan, theme: &Theme, index: usize) -> Result<String> {
    if !plan.colors.category_classes.is_empty() {
        let classes = plan
            .colors
            .category_classes
            .get(index)
            .ok_or_else(|| err("category paint index out of range"))?;
        if classes.is_empty() {
            return Err(err(format!(
                "category {index} has no mark colour (a zero part, or not coloured by part)"
            )));
        }
        return paint_of(theme, classes, false, safe_pair(theme, &plan.spec));
    }
    let n =
        u8::try_from(index + 1).map_err(|_| err("more than 16 categories need color policy"))?;
    slot(theme, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn theme() -> Theme {
        // palette-muted.css values (oracle charts/style/palette-muted.css), no other tokens
        let mut t = Theme::default();
        t.tokens.clear();
        for (k, v) in [
            ("--series-1", "#95b7d3"),
            ("--series-1-strong", "#558bb9"),
            ("--series-1-tint", "#aac5dc"),
            ("--series-12", "#3f6c96"),
            ("--single", "#95b7d3"),
            ("--single-line", "#95b7d3"),
            ("--all-others", "#968c8b"),
            ("--missing", "#a6a6a6"),
            ("--total", "#a6a6a6"),
            ("--average", "#595959"),
            ("--seq-7", "#497ba8"),
            ("--good-2", "#75ba69"),
            ("--good-cvd-2", "#bfe0f3"),
        ] {
            t.tokens.insert(k.into(), json!(v));
        }
        t
    }
    fn c(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn fixed_slot_does_not_wrap_to_first_palette_color() {
        let mut theme = Theme::default();
        theme.tokens.insert("--series-12".into(), json!("#123456"));
        assert_eq!(slot(&theme, 12).unwrap(), "#123456");
        theme.tokens.clear();
        theme.palette.truncate(5); // a style with five series colours: slot 12 must not wrap
        assert!(slot(&theme, 12).is_err());
    }
    #[test]
    fn classes_resolve_as_the_base_stylesheet_paints_them() {
        // base.css: .series-k.tint uses --series-k-tint; a slot without a tint rule keeps its colour;
        // .line.total is drawn in --average; .good.div-p2 in --good-2 (the safe pair --good-cvd-2)
        let t = theme();
        assert_eq!(
            paint_of(&t, &c(&["series-1", "tint"]), false, false).unwrap(),
            "#aac5dc"
        );
        assert_eq!(
            paint_of(&t, &c(&["series-12", "tint"]), false, false).unwrap(),
            "#3f6c96"
        );
        assert_eq!(
            paint_of(&t, &c(&["total"]), true, false).unwrap(),
            "#595959"
        );
        assert_eq!(
            paint_of(&t, &c(&["total"]), false, false).unwrap(),
            "#a6a6a6"
        );
        assert_eq!(
            paint_of(&t, &c(&["seq-7"]), false, false).unwrap(),
            "#497ba8"
        );
        assert_eq!(
            paint_of(&t, &c(&["div-p2", "good"]), false, false).unwrap(),
            "#75ba69"
        );
        assert_eq!(
            paint_of(&t, &c(&["div-p2", "good"]), false, true).unwrap(),
            "#bfe0f3"
        );
        assert_eq!(strong_of(&t, &c(&["series-1"]), false).unwrap(), "#558bb9");
        assert!(paint_of(&t, &c(&["seq-2"]), false, false).is_err());
    }
    #[test]
    fn tint_is_twenty_percent_towards_the_page() {
        // palette-muted.css: --series-1-tint: mix(var(--series-1), #ffffff, 0.2) == #aac5dc
        let t = Theme::default();
        assert_eq!(tint(&t, "#95b7d3").unwrap(), "#aac5dc");
    }
}
