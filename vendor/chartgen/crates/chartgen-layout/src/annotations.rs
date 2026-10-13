//! Exact cartesian highlight targets and their mark-level appearance.
use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
struct Target {
    series: Option<String>,
    category: Option<String>,
}

#[derive(Clone, Debug)]
struct Highlight {
    targets: Vec<Target>,
    dim_others: bool,
}

fn entries(plan: &SemanticPlan) -> Vec<&Value> {
    let explicit = plan.spec.raw.get("highlight").and_then(Value::as_array);
    if let Some(items) = explicit.filter(|items| !items.is_empty()) {
        items.iter().collect()
    } else {
        plan.spec
            .raw
            .pointer("/insight/targets")
            .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()))
            .map(|v| vec![v])
            .unwrap_or_default()
    }
}

fn parse(plan: &SemanticPlan, data: &PreparedData) -> Result<Option<Highlight>> {
    if let Some(raw) = plan.spec.raw.get("highlight") {
        if !raw.is_null() && !raw.is_array() {
            return Err(err("highlight must be an array").at("highlight"));
        }
    }
    let sources = entries(plan);
    if sources.is_empty() {
        return Ok(None);
    }
    if !matches!(
        plan.chart_type,
        ChartType::Bar | ChartType::Dot | ChartType::Line
    ) && !(plan.chart_type == ChartType::StackedBar && data.series.len() == 1)
    {
        return Err(err(format!(
            "highlight targets need a dedicated {} layout",
            plan.chart_type.as_str()
        ))
        .at("highlight"));
    }
    if data.irregular.is_some() {
        return Err(
            err("irregular-time highlight targets need exact timed point placement")
                .at("highlight"),
        );
    }
    let mut targets = Vec::new();
    let mut dim_others = false;
    for entry in sources {
        let (refs, method) = if let Some(refs) = entry.as_array() {
            (refs, "auto")
        } else {
            (
                entry
                    .get("targets")
                    .and_then(Value::as_array)
                    .ok_or_else(|| err("highlight targets must be an array"))?,
                entry
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("auto"),
            )
        };
        if !matches!(method, "auto" | "strong") {
            return Err(err(format!(
                "highlight method {method} is not placed by this cartesian layout"
            ))
            .at("highlight.method"));
        }
        if refs.is_empty() {
            return Err(err("highlight has no targets").at("highlight.targets"));
        }
        dim_others |= entry
            .get("dim_others")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        for reference in refs {
            let series = reference
                .get("series")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let category = reference
                .get("category")
                .and_then(Value::as_str)
                .or_else(|| reference.get("x").and_then(Value::as_str))
                .map(str::to_owned);
            if series.is_none() && category.is_none() {
                return Err(
                    err("highlight target needs a category or series").at("highlight.targets")
                );
            }
            if let Some(s) = &series {
                if !data.series.iter().any(|candidate| &candidate.id == s) {
                    return Err(
                        err(format!("highlight names unknown series {s}")).at("highlight.targets")
                    );
                }
            }
            if let Some(cat) = &category {
                if !data.categories.contains(cat) {
                    return Err(err(format!("highlight names unknown category {cat}"))
                        .at("highlight.targets"));
                }
            }
            if plan.chart_type == ChartType::Line && (series.is_none() || category.is_some()) {
                return Err(
                    err("partial line highlight needs stretch-specific path placement")
                        .at("highlight.targets"),
                );
            }
            targets.push(Target { series, category });
        }
    }
    Ok(Some(Highlight {
        targets,
        dim_others,
    }))
}

pub fn validate(plan: &SemanticPlan, data: &PreparedData) -> Result<()> {
    // The ported specialist layouts draw (bumps) or report (arrows, step, thin bars) their
    // own highlight, as the original's do.
    if matches!(plan.chart_type, ChartType::Bumps | ChartType::Arrow)
        || (matches!(plan.chart_type, ChartType::Step | ChartType::ThinBar)
            && data.irregular.is_some())
    {
        return Ok(());
    }
    parse(plan, data).map(|_| ())
}

fn darken(base: &str) -> Result<String> {
    let hex = base
        .strip_prefix('#')
        .ok_or_else(|| err("highlight requires six-digit hex palette colors"))?;
    if hex.len() != 6 {
        return Err(err("highlight requires six-digit hex palette colors"));
    }
    let n = u32::from_str_radix(hex, 16).map_err(|_| err("invalid palette color for highlight"))?;
    let r = ((n >> 16) & 255) as f64;
    let g = ((n >> 8) & 255) as f64;
    let b = (n & 255) as f64;
    Ok(format!(
        "#{:02x}{:02x}{:02x}",
        (r * 0.72).round() as u8,
        (g * 0.72).round() as u8,
        (b * 0.72).round() as u8
    ))
}

fn paint_of(p: &mut Primitive) -> Option<(&mut Paint, &mut Metadata)> {
    match p {
        Primitive::Rect { paint, meta, .. }
        | Primitive::Circle { paint, meta, .. }
        | Primitive::Path { paint, meta, .. } => Some((paint, meta)),
        _ => None,
    }
}

pub fn apply(
    plan: &SemanticPlan,
    data: &PreparedData,
    theme: &Theme,
    primitives: &mut Vec<Primitive>,
    findings: &mut Vec<Finding>,
) -> Result<()> {
    let Some(highlight) = parse(plan, data)? else {
        return Ok(());
    };
    let mut selected = BTreeSet::new();
    for target in &highlight.targets {
        for (si, s) in data.series.iter().enumerate() {
            if target.series.as_ref().is_some_and(|id| id != &s.id) {
                continue;
            }
            for (ci, cat) in data.categories.iter().enumerate() {
                if target.category.as_ref().is_some_and(|name| name != cat) {
                    continue;
                }
                if s.values[ci].value.is_some() {
                    selected.insert((si, ci));
                }
            }
        }
    }
    if selected.is_empty() {
        return Err(err("highlight targets have no drawable marks"));
    }
    let selected_series = selected.iter().map(|(si, _)| *si).collect::<BTreeSet<_>>();
    let mut found = BTreeSet::new();
    let mut extracted = Vec::new();
    let mut retained = Vec::with_capacity(primitives.len());
    let mut insertion = 0;
    for mut p in std::mem::take(primitives) {
        let mut selected_mark = None;
        let mut ordinary_mark = false;
        if let Some((paint, meta)) = paint_of(&mut p) {
            if meta.role == "data-mark" {
                ordinary_mark = true;
                if let Some(id) = &meta.id {
                    for &(si, ci) in &selected {
                        if id == &format!("{}-{ci}", data.series[si].id) {
                            selected_mark = Some(si);
                            found.insert((si, ci));
                            break;
                        }
                    }
                }
            } else if meta.role == "data-line" {
                ordinary_mark = true;
                if let Some(series) = meta.data.get("series") {
                    if let Some(si) = data.series.iter().position(|s| &s.id == series) {
                        if selected_series.contains(&si) {
                            selected_mark = Some(si);
                        }
                    }
                }
            }
            if let Some(si) = selected_mark {
                let own = paint
                    .fill
                    .as_deref()
                    .map(str::to_owned)
                    .unwrap_or(colours::series(plan, theme, si)?);
                let token = if data.series.len() == 1 {
                    "--single-strong".to_owned()
                } else {
                    format!("--series-{}-strong", si + 1)
                };
                let strong = theme
                    .tokens
                    .get(&token)
                    .filter(|_| plan.chart_type != ChartType::StackedBar)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or(darken(&own)?);
                if paint.fill.is_some() {
                    paint.fill = Some(strong.clone());
                }
                if paint.stroke.is_some() {
                    paint.stroke = Some(strong);
                }
                if meta.role == "data-line" {
                    paint.stroke_width *= 2.;
                }
                meta.classes.push("highlight".into());
            } else if ordinary_mark && highlight.dim_others {
                paint.opacity = 0.32;
                meta.classes.push("context".into());
            }
        }
        if selected_mark.is_some() {
            insertion = retained.len();
            extracted.push(p);
        } else {
            retained.push(p);
            if ordinary_mark {
                insertion = retained.len();
            }
        }
    }
    if plan.chart_type != ChartType::Line && found != selected {
        return Err(err("highlight target does not match placed mark"));
    }
    retained.splice(insertion..insertion, extracted);
    *primitives = retained;
    findings.push(Finding::new(
        "R15-5",
        "guided",
        "highlighted targets use the strong shade of their own hue",
    ));
    Ok(())
}
