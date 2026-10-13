//! Colour planning: the original's palette slots, part colours and orders, good and bad sides,
//! and quantity ramps, independent of CSS paint values and physical geometry.
//!
//! The plan names colours by the original's classes (`single`, `series-k`, `all-others`,
//! `seq-k`, …); a style resolves them to paints. Every rule here is ported from the original's
//! layouts (bars.py, lines.py, bumps.py, pie.py, treemap.py, wrapped.py, waterfall.py,
//! arrowchart.py, multiples.py), `colour_slots` in `__init__.py`, and ramps.py.
use chartgen_model::{ChartType, ColorPlan, Finding, PreparedData, Result, SemanticPlan, Spec};
use serde_json::{Value, json};

/// The colour-space maths (figkit style.py, okhsl.py), re-exported.
pub use chartgen_colour_space as colour;
mod parts;
pub mod ramps;
mod series;
mod slots;
mod src;
pub mod textures;

pub use slots::{panel_label, panels as panel_slots, validate};
pub use src::Src;

/// What colour planning adds to the report.
#[derive(Debug, Clone, Default)]
pub struct Assignments {
    pub findings: Vec<Finding>,
    /// Decisions made before the chart is chosen (the original's `colour_slots`): they lead the
    /// report's decisions.
    pub leading: Vec<Value>,
    pub decisions: Vec<Value>,
    /// Overrides a colour refusal recorded (rules.refusal).
    pub overrides: Vec<Value>,
    /// A colour rule refused the chart (a heatmap's palette, R3-13, R3-9).
    pub refused: bool,
    /// Marks on the dividing value (series id, category index): they keep their colour, and
    /// move to palette slot 3 where the style draws good and bad in the safe pair (annotate.good_bad).
    pub neutral: Vec<(String, usize)>,
}

pub(crate) fn decision(rule: &str, level: &str, text: impl Into<String>) -> Value {
    json!({"rule": rule, "level": level, "text": text.into()})
}
pub(crate) fn finding(
    rule: &str,
    severity: &str,
    level: &str,
    message: impl Into<String>,
    path: Option<String>,
) -> Finding {
    let mut f = Finding::new(rule, severity, message);
    f.level = level.into();
    f.path = path;
    f
}
pub(crate) fn info(rule: &str, message: impl Into<String>, path: Option<String>) -> Finding {
    finding(rule, "info", "locked", message, path)
}
pub(crate) fn warn(rule: &str, message: impl Into<String>, path: Option<String>) -> Finding {
    finding(rule, "warn", "locked", message, path)
}

/// One class list per series or category, written by the planners.
#[derive(Debug, Clone, Default)]
pub(crate) struct Draft {
    pub series: Vec<Vec<String>>,
    pub categories: Vec<Vec<String>>,
    pub series_order: Vec<usize>,
    pub category_order: Vec<usize>,
    pub ramp: Option<chartgen_model::RampPlan>,
    pub marks: Vec<Vec<Vec<String>>>,
}

pub(crate) fn class(c: impl Into<String>) -> Vec<String> {
    vec![c.into()]
}
pub(crate) fn slot_of(classes: &[String]) -> Option<u8> {
    classes.first()?.strip_prefix("series-")?.parse().ok()
}

/// Resolve the colour plan of the selected semantic plan without rewriting its source spec.
/// Slots fixed by name come first (`colour_slots`); then the chart type's own colouring.
pub fn compile(plan: &mut SemanticPlan, data: &PreparedData) -> Result<Assignments> {
    compile_with(plan, data, None)
}

/// As `compile`, with the slots a small-multiples grid fixed for this panel's series (by
/// prepared series index), which take the place of the chart's own hand-out.
pub fn compile_with(
    plan: &mut SemanticPlan,
    data: &PreparedData,
    panel: Option<&[Option<u8>]>,
) -> Result<Assignments> {
    validate(&plan.spec)?;
    let src = Src::new(&plan.spec, data);
    let (fixed, findings, leading) = match panel {
        // a panel of small multiples: the slots were fixed once over the whole spec
        Some(p) => (
            (0..src.n_series())
                .map(|i| p.get(i).copied().flatten())
                .collect(),
            vec![],
            vec![],
        ),
        None => slots::fixed(&src),
    };
    let mut out = Assignments {
        findings,
        leading,
        ..Assignments::default()
    };
    let draft = plan_type(plan, &src, &fixed, &mut out)?;
    plan.colors = to_plan(draft);
    Ok(out)
}

fn plan_type(
    plan: &SemanticPlan,
    src: &Src,
    fixed: &[Option<u8>],
    out: &mut Assignments,
) -> Result<Draft> {
    let n = src.n_series();
    let mut d = Draft {
        series: vec![vec![]; n],
        categories: vec![vec![]; src.categories().len()],
        ..Draft::default()
    };
    match plan.chart_type {
        // Statistical charts colour their series as clustered bars do: one series the single
        // colour, several the palette slots in order, fixed slots kept (STATS.md).
        ChartType::Box
        | ChartType::Strip
        | ChartType::Ecdf
        | ChartType::Scatter
        | ChartType::Dumbbell
        | ChartType::IntervalDot
        | ChartType::IntervalBar => series::bars(src, fixed, plan.chart_type, &mut d, out),
        ChartType::StateMatrix | ChartType::Contingency => {
            // the layout hands out the palette to the declared states, in order
            for c in &mut d.series {
                *c = class("single");
            }
        }
        ChartType::Bar | ChartType::Dot | ChartType::FunnelBars => {
            series::bars(src, fixed, plan.chart_type, &mut d, out);
            if plan.chart_type != ChartType::FunnelBars {
                series::good_bad(src, &plan.order, &mut d, out);
            }
        }
        ChartType::ThinBar | ChartType::Step => {
            // step.py: one series (R3-3)
            for c in &mut d.series {
                *c = class("single");
            }
        }
        ChartType::Line => {
            series::lines(src, fixed, &(0..n).collect::<Vec<_>>(), &mut d, out);
            if src.get("/data/measure/good").is_string()
                && !src.get("/data/measure/dividing_point").is_null()
            {
                out.findings.push(info(
                    "CG-3",
                    "good and bad colouring of lines is not drawn: lines keep their colours (G4.27)",
                    Some("data.measure.good".into()),
                ));
            }
        }
        ChartType::LineTotal => series::line_total(src, fixed, &mut d, out),
        ChartType::Bumps => series::bumps(src, fixed, &mut d, out),
        ChartType::Arrow => series::arrows(src, &mut d, out),
        ChartType::Waterfall => series::waterfall(src, &mut d),
        ChartType::Pie | ChartType::Donut => parts::pie(src, fixed, &mut d, out),
        ChartType::Treemap => parts::treemap(src, &mut d, out),
        ChartType::WrappedBar => parts::wrapped(src, &mut d),
        ChartType::StackedBar
        | ChartType::StackedBar100
        | ChartType::StackedArea
        | ChartType::StackedArea100 => parts::stack(src, fixed, &mut d, out),
        ChartType::Heatmap => {
            let (ramp, notes) = ramps::heatmap_notes(src);
            out.findings.extend(notes.findings);
            out.decisions.extend(notes.decisions);
            out.overrides.extend(notes.overrides);
            out.refused |= notes.refused;
            d.ramp = ramp.filter(|_| !notes.refused);
        }
        ChartType::Table => {
            // table.py: a series group in the single colour; a shaded table's cells on its ramps
            for c in &mut d.series {
                *c = class("single");
            }
            let (ramp, notes) = ramps::table_heat(src);
            out.findings.extend(notes.findings);
            out.decisions.extend(notes.decisions);
            out.overrides.extend(notes.overrides);
            out.refused |= notes.refused;
            d.ramp = ramp;
        }
        ChartType::ActionDots => {
            // action dots: grey bars (action-bar), and a status dot where action is warranted,
            // a step of the good or bad ramp (ramps.action_classes)
            for (j, c) in src.categories().iter().enumerate() {
                if let Some(cls) =
                    ramps::action_classes(src.get("/data/actions")[c.as_str()].as_str())
                {
                    d.categories[j] = cls;
                }
            }
        }
    }
    Ok(d)
}

fn to_plan(d: Draft) -> ColorPlan {
    ColorPlan {
        series_slots: d.series.iter().map(|c| slot_of(c)).collect(),
        category_slots: d.categories.iter().map(|c| slot_of(c)).collect(),
        category_tints: d
            .categories
            .iter()
            .map(|c| c.iter().any(|x| x == "tint"))
            .collect(),
        series_classes: d.series,
        category_classes: d.categories,
        series_order: d.series_order,
        category_order: d.category_order,
        ramp: d.ramp,
        mark_classes: d.marks,
    }
}

/// The slots fixed over a whole spec before it is drawn (`colour_slots`): per prepared series,
/// with its findings and leading R3-24 decision. Small multiples call this once on the full spec.
pub fn fixed_slots(
    spec: &Spec,
    data: &PreparedData,
) -> Result<(Vec<Option<u8>>, Vec<Finding>, Vec<Value>)> {
    validate(spec)?;
    Ok(slots::fixed(&Src::new(spec, data)))
}

/// The labels of the series inside small multiples' panels, in the order their slots are handed
/// out: each label once, in series order, then top to bottom by the line ends of the top right
/// panel (`last_panel`: its series indices), a label without a value there last.
pub fn panel_combos(
    spec: &Spec,
    data: &PreparedData,
    within: &[String],
    last_panel: &[usize],
) -> Vec<String> {
    let series = spec.raw["data"]["series"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut combos: Vec<String> = vec![];
    for s in &series {
        let k = panel_label(s, within);
        if !combos.contains(&k) {
            combos.push(k);
        }
    }
    let mut last = std::collections::BTreeMap::<String, Option<chartgen_model::Decimal>>::new();
    for &i in last_panel {
        let (Some(s), Some(p)) = (series.get(i), data.series.get(i)) else {
            continue;
        };
        let v = p.values.iter().rev().find_map(|d| d.value);
        last.insert(panel_label(s, within), v);
    }
    combos.sort_by(|a, b| {
        let (x, y) = (
            last.get(a).copied().flatten(),
            last.get(b).copied().flatten(),
        );
        match (x, y) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    combos
}

/// What a chart's good and bad colours make of its output (annotate.cvd; D-18, G4.18, R3-20).
#[derive(Debug, Default)]
pub struct GoodBadOutput {
    pub findings: Vec<Finding>,
    pub decisions: Vec<Value>,
    /// The colour-blind-safe file is this one: the red/green version is written beside it as
    /// the copy `red-green`.
    pub copy: bool,
    /// Black and white (audience.bw_risk): good and bad must be told apart by textures too.
    pub textured: bool,
    /// Good and bad take the colour-blind-safe pair (the chart carries `cvd-safe`).
    pub safe: bool,
}

/// annotate.good_bad under the colour-blind-safe pair: a mark on the dividing value in `single`,
/// `series-1` or `series-2` (too close to the pair's blue and orange) moves to palette slot 3.
/// Marks are found by their `{series}-{category index}` identity.
pub fn safe_pair_neutrals(
    ps: &mut [chartgen_model::Primitive],
    neutral: &[(String, usize)],
    slot3: Option<&str>,
) {
    for p in ps.iter_mut() {
        if let chartgen_model::Primitive::Group { children, .. } = p {
            safe_pair_neutrals(children, neutral, slot3);
            continue;
        }
        let hit = p
            .meta()
            .id
            .as_ref()
            .is_some_and(|id| neutral.iter().any(|(s, j)| id == &format!("{s}-{j}")));
        if !hit {
            continue;
        }
        let meta = p.meta_mut();
        let mut moved = false;
        for c in meta.classes.iter_mut() {
            if matches!(c.as_str(), "single" | "series-1" | "series-2") {
                *c = "series-3".into();
                moved = true;
            }
        }
        if !moved {
            continue;
        }
        if let (Some(paint), Some(c)) = (paint_mut(p), slot3) {
            if paint.fill.is_some() {
                paint.fill = Some(c.into());
            }
            if paint.stroke.is_some() && paint.stroke_width > 0. {
                paint.stroke = Some(c.into());
            }
        }
    }
}

fn paint_mut(p: &mut chartgen_model::Primitive) -> Option<&mut chartgen_model::Paint> {
    use chartgen_model::Primitive::*;
    match p {
        Rect { paint, .. }
        | Circle { paint, .. }
        | Path { paint, .. }
        | Wedge { paint, .. }
        | Shape { paint, .. } => Some(paint),
        _ => None,
    }
}

/// Whether an arrow chart's shafts carry good or bad (they say rise or fall by direction).
pub fn has_good_bad_arrows(ps: &[chartgen_model::Primitive]) -> bool {
    ps.iter().any(|p| match p {
        chartgen_model::Primitive::Group { children, .. } => has_good_bad_arrows(children),
        p => {
            let c = &p.meta().classes;
            c.iter().any(|x| x == "change-shaft") && c.iter().any(|x| x == "good" || x == "bad")
        }
    })
}

fn good_bad_items(c: &ColorPlan) -> bool {
    let gb = |v: &Vec<String>| v.iter().any(|x| x == "good" || x == "bad");
    c.series_classes.iter().any(gb)
        || c.category_classes.iter().any(gb)
        || c.mark_classes.iter().flatten().any(gb)
        || c.ramp
            .as_ref()
            .is_some_and(|r| r.cells.iter().flatten().any(gb))
}

/// The style's treatment of good and bad (`--good-bad-mode`: semantic or safe-pair) for a chart
/// whose plan colours marks good or bad: the decisions the original records (annotate.cvd), and
/// whether a red/green copy is written or textures are needed.
pub fn good_bad_output(colors: &ColorPlan, spec: &Spec, mode: Option<&str>) -> GoodBadOutput {
    let mut out = GoodBadOutput::default();
    let mut mode = mode.unwrap_or("semantic").trim().to_owned();
    if !matches!(mode.as_str(), "semantic" | "safe-pair") {
        out.findings.push(warn(
            "CG-4",
            format!("--good-bad-mode '{mode}' is not one of semantic, safe-pair: semantic"),
            None,
        ));
        mode = "semantic".into();
    }
    if !good_bad_items(colors) {
        return out;
    }
    let safe = mode == "safe-pair" && spec.raw["audience"]["cvd"].as_str() != Some("no");
    let bw = &spec.raw["audience"]["bw_risk"];
    out.textured = bw.as_bool().unwrap_or(false);
    out.copy = safe && !out.textured;
    out.safe = safe;
    if safe {
        out.decisions.push(decision(
            "R15-12",
            "locked",
            "orange for bad and blue for good, safe for readers with colour-vision deficiency (G4.18): the style's colour-blind-safe pair (--good-bad-mode: safe-pair, for publications to unknown readers; D-18); the other colours from palette slot 3 on, apart from the pair",
        ));
    } else if mode == "safe-pair" {
        out.decisions.push(decision(
            "R15-11",
            "guided",
            "red for bad, green for good: the audience has no colour-vision deficiency (audience.cvd no), so the style's colour-blind-safe pair is not used",
        ));
    }
    let pair = if safe {
        "orange and blue"
    } else {
        "red and green"
    };
    if out.textured {
        out.decisions.push(decision(
            "R15-12",
            "locked",
            format!(
                "black and white (audience.bw_risk): {pair} told apart without colour too, which serves readers with colour-vision deficiency as well (D-18; owner, 1 Oct 2026)"
            ),
            // (the façade appends what told them apart: annotate.cvd's account of the textures)
        ));
    }
    let text = if out.copy {
        "static output: this file the colour-blind-safe version, the red/green version beside it as a second file (<name>.red-green.svg; D-18)"
    } else if safe {
        "static output, in black and white: one file, the safe pair told apart without colour too; no red/green copy, which would add nothing in grey (D-18)"
    } else if out.textured {
        "static output, in black and white: one file, red for bad and green for good, told apart without colour too (D-18; owner, 1 Oct 2026)"
    } else if mode == "safe-pair" {
        "static output, in colour: one file, red for bad and green for good, plain (audience.cvd no; D-18)"
    } else {
        "static output, in colour: one file, red for bad and green for good, plain, for every reader (D-18; owner, 1 Oct 2026); black and white (audience.bw_risk) adds the textures, and a style may choose the colour-blind-safe pair (--good-bad-mode: safe-pair)"
    };
    out.decisions.push(decision("R3-20", "locked", text));
    out
}

/// One panel of small multiples or a merged chart, as colour planning sees it.
pub struct PanelRef<'a> {
    /// The panel's series (indices into the whole spec's prepared series).
    pub series: &'a [usize],
    pub row: usize,
    pub column: usize,
}

/// The palette slots of a grid of panels (multiples.py): slots fixed by name once over the whole
/// spec (`colour_slots`); with series drawn inside the panels (`within`), each series label its own
/// slot, the same in every panel (R6-28, G7.50); else one colour for every mark (R3-3).
#[derive(Debug, Default)]
pub struct PanelColours {
    /// Per panel, the fixed slot of each of its series.
    pub fixed: Vec<Vec<Option<u8>>>,
    pub findings: Vec<Finding>,
    pub leading: Vec<Value>,
    pub decisions: Vec<Value>,
}

pub fn panels_plan(
    spec: &Spec,
    data: &PreparedData,
    within: &[String],
    merged: bool,
    panels: &[PanelRef],
) -> Result<PanelColours> {
    let (global, findings, leading) = fixed_slots(spec, data)?;
    let mut out = PanelColours {
        findings,
        leading,
        ..PanelColours::default()
    };
    let mut by_series = std::collections::BTreeMap::new();
    if !within.is_empty() {
        let columns = panels.iter().map(|p| p.column).max().unwrap_or(0);
        let last = panels
            .iter()
            .find(|p| p.row == 0 && p.column == columns)
            .map(|p| p.series.to_vec())
            .unwrap_or_default();
        let combos = panel_combos(spec, data, within, &last);
        let (map, findings, decisions) = slots::panels(spec, within, &combos, panels.len() > 1);
        out.findings.extend(findings);
        out.decisions.extend(decisions);
        by_series = map;
    } else if merged {
        out.decisions.push(decision(
            "R3-3",
            "locked",
            "one colour for every mark: each panel holds one measure, and the panels tell them apart (G7.6)",
        ));
    } else if !panels.is_empty() {
        out.decisions.push(decision(
            "R3-3",
            "locked",
            "one colour for every mark: every variable is on the rows or columns (G7.50)",
        ));
    }
    out.fixed = panels
        .iter()
        .map(|p| {
            p.series
                .iter()
                .map(|&i| {
                    by_series
                        .get(&i)
                        .copied()
                        .or(global.get(i).copied().flatten())
                })
                .collect()
        })
        .collect();
    Ok(out)
}

/// Whether the spec's series are drawn in small multiples or a merged chart, where the panels
/// hand out the slots of the series inside them (multiples.py).
pub fn within_dims(spec: &Spec) -> Vec<String> {
    if let Some(a) = spec.get("/panels/arrangement/within").as_array() {
        return a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    let compare: Vec<&str> = spec
        .get("/facts/compare")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    spec.get("/data/dims")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d["id"].as_str())
        .filter(|id| compare.contains(id))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests;
