//! Physical chart placement, in 1/72-inch units with a top-left origin.
#[path = "families/annotate.rs"]
mod annotate;
mod annotations;
#[path = "families/circular.rs"]
mod circular;
pub mod colours;
mod comments;
#[path = "families/composition.rs"]
mod composition;
pub mod engine;
pub mod guides;
pub use engine::{DefaultLayout, LayoutEngine};
#[path = "families/heatmap.rs"]
mod heatmap;
#[path = "families/kit.rs"]
mod kit;
#[path = "families/matrix.rs"]
mod matrix;
mod orientation;
#[path = "families/pie.rs"]
mod pie;
mod special;
#[path = "families/statistical.rs"]
mod statistical;
#[path = "families/statistical_grid.rs"]
mod statistical_grid;
#[path = "families/statistical_xy.rs"]
mod statistical_xy;
#[path = "families/table.rs"]
mod table;
mod timeaxis;
#[path = "families/treemap.rs"]
mod treemap;
use chartgen_model::*;
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;
use std::collections::BTreeMap;

fn content_key(content: &TextContent) -> String {
    serde_json::to_string(content).expect("TextContent serializes")
}
fn err(message: impl Into<String>) -> Error {
    Error::new("layout", message)
}
fn paint(fill: Option<String>, stroke: Option<String>, width: f64) -> Paint {
    Paint {
        fill,
        stroke,
        stroke_width: width,
        opacity: 1.,
    }
}
fn meta(role: &str, id: String) -> Metadata {
    Metadata {
        id: Some(id),
        role: role.into(),
        ..Default::default()
    }
}
fn color(theme: &Theme, i: usize) -> Result<String> {
    theme
        .palette
        .get(i)
        .cloned()
        .ok_or_else(|| err("theme palette is empty"))
}
fn value(d: &Datum) -> Result<Option<f64>> {
    d.value
        .map(|v| {
            v.to_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(|| err("data value cannot be represented as finite geometry"))
        })
        .transpose()
}
fn text(request: &TextRequest, metrics: &TextMetrics, x: f64, y: f64, anchor: &str) -> Primitive {
    Primitive::Text {
        position: Point { x, y },
        request: request.clone(),
        metrics: metrics.clone(),
        anchor: anchor.into(),
        meta: Metadata {
            role: request.role.clone(),
            ..Default::default()
        },
    }
}
fn line(x1: f64, y1: f64, x2: f64, y2: f64, colour: &str, role: &str) -> Primitive {
    Primitive::Line {
        from: Point { x: x1, y: y1 },
        to: Point { x: x2, y: y2 },
        paint: paint(None, Some(colour.into()), 0.8),
        meta: Metadata {
            role: role.into(),
            ..Default::default()
        },
    }
}
fn rect(x: f64, y: f64, w: f64, h: f64, fill: String, role: &str, id: String) -> Primitive {
    Primitive::Rect {
        bounds: Bounds {
            x,
            y,
            width: w,
            height: h,
        },
        paint: paint(Some(fill), None, 0.),
        meta: meta(role, id),
    }
}
fn boxes_overlap(a: &Bounds, b: &Bounds) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}
fn text_box(p: &Primitive) -> Option<Bounds> {
    if let Primitive::Text {
        position,
        metrics,
        anchor,
        ..
    } = p
    {
        let x = position.x
            - match anchor.as_str() {
                "middle" => metrics.width / 2.,
                "end" => metrics.width,
                _ => 0.,
            };
        Some(Bounds {
            x,
            y: position.y,
            width: metrics.width,
            height: metrics.height,
        })
    } else {
        None
    }
}
fn geometry_scale(scale: &TrainedScale, v: f64, plot: &Bounds, horizontal: bool) -> Result<f64> {
    let lo = scale
        .lo
        .to_f64()
        .ok_or_else(|| err("invalid scale lower bound"))?;
    let hi = scale
        .hi
        .to_f64()
        .ok_or_else(|| err("invalid scale upper bound"))?;
    if !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return Err(err("scale domain must be finite and increasing"));
    }
    let f = (v - lo) / (hi - lo);
    if !f.is_finite() {
        return Err(err("non-finite scale mapping"));
    }
    if f < -1e-9 || f > 1. + 1e-9 {
        return Err(err(format!(
            "value {v} lies outside trained scale [{lo}, {hi}]"
        )));
    }
    Ok(if horizontal {
        plot.x + f * plot.width
    } else {
        plot.y + (1. - f) * plot.height
    })
}
fn ensure_supported(plan: &SemanticPlan, data: &PreparedData) -> Result<()> {
    let raw = &plan.spec.raw;
    fn active(v: &serde_json::Value) -> bool {
        if v.is_null() || v == false {
            return false;
        }
        if let Some(a) = v.as_array() {
            return a.iter().any(active);
        }
        if let Some(o) = v.as_object() {
            if o.is_empty() || o.get("kind").and_then(|x| x.as_str()) == Some("none") {
                return false;
            }
            return o.values().any(active);
        }
        v.as_str().is_none_or(|s| !s.is_empty())
    }
    for key in [
        "facet",
        "facets",
        "inset",
        "insets",
        "annotations",
        "notes",
        "panels",
        "events",
        "bands",
        "reference_lines",
    ] {
        if raw.get(key).is_some_and(active) {
            return Err(err(format!("{key} is not supported by portable layout")).at(key));
        }
    }
    if let Some(labels) = raw.get("labels").filter(|v| active(v)) {
        let labels = labels
            .as_object()
            .ok_or_else(|| err("labels must be an object").at("labels"))?;
        if labels.len() != 1 || !labels.contains_key("notes") {
            return Err(err("only labels.notes is placed by this layout").at("labels"));
        }
        let notes = labels["notes"]
            .as_object()
            .ok_or_else(|| err("labels.notes must be a dictionary").at("labels.notes"))?;
        for (status, text) in notes {
            if !text.is_string() {
                return Err(err(format!("labels.notes.{status} must be text")).at("labels.notes"));
            }
            let used = if plan.chart_type == ChartType::FunnelBars && status == "stage_change" {
                true
            } else if plan.chart_type == ChartType::Arrow {
                // the arrow layout writes a row's note where its change cannot be drawn
                data.series.iter().flat_map(|s| &s.values).any(|d| {
                    d.value.is_none()
                        && (d.status == *status || (d.status.is_empty() && status == "missing"))
                })
            } else if matches!(
                plan.chart_type,
                ChartType::Bar
                    | ChartType::ThinBar
                    | ChartType::Dot
                    | ChartType::Line
                    | ChartType::Step
            ) {
                data.series
                    .iter()
                    .flat_map(|s| &s.values)
                    .any(|d| d.value.is_none() && d.status == *status)
            } else {
                false
            };
            if !used {
                return Err(err(format!(
                    "labels.notes.{status} has no placed status in this family"
                ))
                .at("labels.notes"));
            }
        }
    }
    if plan.chart_type != ChartType::Arrow
        && raw
            .pointer("/chart/change_panels")
            .is_some_and(|v| active(v) && v.as_str() != Some("auto"))
    {
        // only the arrow layout draws its panels as asked
        return Err(
            err("/chart/change_panels is not supported by portable layout")
                .at("/chart/change_panels"),
        );
    }
    for pointer in [
        "/data/dims",
        "/data/measures",
        "/chart/labelling",
        "/chart/value_labels",
    ] {
        if raw.pointer(pointer).is_some_and(|v| {
            active(v)
                && !matches!(v.as_str(), Some("auto" | "off" | "none"))
                && !(pointer == "/chart/value_labels"
                    && v.as_str() == Some("on")
                    && matches!(
                        plan.chart_type,
                        ChartType::Bar | ChartType::ThinBar | ChartType::Dot | ChartType::Heatmap
                    ))
        }) {
            return Err(err(format!("{pointer} is not supported by portable layout")).at(pointer));
        }
    }
    if raw.pointer("/data/comparisons").is_some_and(active) && data.comparisons.is_empty() {
        return Err(err("comparison declarations were not prepared"));
    }
    annotations::validate(plan, data)?;
    comments::validate(plan)?;
    special::events::validate(plan, data)?;
    if !plan.layers.is_empty() {
        if plan.layers[0].mark != plan.chart_type {
            return Err(err("main semantic layer does not match chart family"));
        }
        for layer in &plan.layers[1..] {
            if !matches!(layer.source, Some(LayerSource::Comparison { .. })) {
                return Err(err("unsupported compound semantic layer"));
            }
        }
    }
    Ok(())
}

/// Returns one batch of native text requests, or a completed placed chart when all metrics arrive.
/// The caller may resubmit the same plan with the supplied measurements; no hidden state is kept.
pub fn solve(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
    theme: &Theme,
    constraints: &LayoutConstraints,
    measurements: &Measurements,
) -> Result<LayoutStep> {
    // Statistical charts: their own family (STATS.md), reading their input through
    // chartgen-statistics like the policy and scale stages.
    if plan.chart_type.is_statistical() {
        return statistical::layout(plan, data, scale, theme, constraints, measurements);
    }
    // Ported matrix families validate their own features (families/kit.rs::guard).
    if matches!(
        plan.chart_type,
        ChartType::Table | ChartType::ActionDots | ChartType::Heatmap
    ) {
        return matrix::layout(plan, data, theme, constraints, measurements);
    }
    if matches!(
        plan.chart_type,
        ChartType::Pie | ChartType::Donut | ChartType::Treemap
    ) {
        return circular::layout(plan, data, theme, constraints, measurements);
    }
    ensure_supported(plan, data)?;
    if !constraints.width.is_finite()
        || constraints.width <= 48.
        || constraints
            .height
            .is_some_and(|h| !h.is_finite() || h <= 48.)
    {
        return Err(err(
            "chart dimensions must be finite and larger than 48 points",
        ));
    }
    if !theme.font_size.is_finite() || theme.font_size <= 0. || theme.palette.is_empty() {
        return Err(err("invalid theme font size or palette"));
    }
    if data.series.is_empty() || data.categories.is_empty() {
        return Err(err("chart needs categories and series"));
    }
    for s in &data.series {
        if s.values.len() != data.categories.len() {
            return Err(err(format!(
                "series {} length differs from category count",
                s.id
            )));
        }
    }
    for s in &data.series {
        for (i, d) in s.values.iter().enumerate() {
            // a value's own note is a footnote, marked and set by the note layout
            if !matches!(d.certainty.as_str(), "" | "actual") {
                return Err(err(format!(
                    "certainty {} for {} / {} needs distinct mark styling",
                    d.certainty, s.name, data.categories[i]
                )));
            }
            if d.value.is_some() && !d.complete {
                return Err(err(format!(
                    "incomplete value for {} / {} needs distinct mark styling",
                    s.name, data.categories[i]
                )));
            }
            if d.limit.is_some()
                && !matches!(plan.chart_type, ChartType::Table | ChartType::Heatmap)
            {
                return Err(err(format!(
                    "censored value for {} / {} needs limit styling",
                    s.name, data.categories[i]
                )));
            }
            if d.value.is_some() && !matches!(d.status.as_str(), "" | "known" | "actual") {
                return Err(err(format!(
                    "status {} conflicts with a drawable value",
                    d.status
                )));
            }
        }
    }
    if !data.comparisons.is_empty() {
        if data.irregular.is_some()
            || !matches!(
                plan.chart_type,
                ChartType::Bar
                    | ChartType::ThinBar
                    | ChartType::Dot
                    | ChartType::Line
                    | ChartType::Step
            )
        {
            return Err(err(
                "comparison rendering is not implemented for this chart family",
            ));
        }
        for c in &data.comparisons {
            if c.valid && !matches!(c.render.as_str(), "none" | "mark" | "line" | "series") {
                return Err(err(format!("unsupported comparison render {}", c.render)));
            }
            if c.valid
                && c.render != "none"
                && c.values.len() != 1
                && c.values.len() != data.categories.len()
            {
                return Err(err(format!(
                    "comparison {} has neither one nor category-aligned values",
                    c.id
                )));
            }
        }
    }
    if matches!(
        plan.chart_type,
        ChartType::StackedBar
            | ChartType::StackedBar100
            | ChartType::Waterfall
            | ChartType::StackedArea
            | ChartType::StackedArea100
    ) && data
        .series
        .iter()
        .flat_map(|s| &s.values)
        .any(|d| d.value.is_none())
    {
        return Err(err(
            "missing stacked or cumulative values need a dedicated gap and note layout",
        ));
    }
    if data.irregular.is_some() && !matches!(plan.chart_type, ChartType::Step | ChartType::ThinBar)
    {
        return Err(err(
            "irregular points require a dedicated step or thin-bar layout",
        ));
    }
    if matches!(
        plan.chart_type,
        ChartType::Pie | ChartType::Donut | ChartType::Treemap
    ) {
        return circular::layout(plan, data, theme, constraints, measurements);
    }
    if matches!(
        plan.chart_type,
        ChartType::Table | ChartType::Heatmap | ChartType::ActionDots
    ) {
        return matrix::layout(plan, data, theme, constraints, measurements);
    }
    if plan.chart_type == ChartType::StackedBar && data.series.len() == 1 {
        return special::solve(plan, data, scale, theme, constraints, measurements);
    }
    if matches!(
        plan.chart_type,
        ChartType::WrappedBar
            | ChartType::FunnelBars
            | ChartType::Arrow
            | ChartType::Bumps
            | ChartType::LineTotal
    ) || data.irregular.is_some()
    {
        return special::solve(plan, data, scale, theme, constraints, measurements);
    }
    let order: Vec<usize> = if plan.order.is_empty() {
        (0..data.categories.len()).collect()
    } else {
        plan.order.clone()
    };
    let mut seen = vec![false; data.categories.len()];
    for &i in &order {
        if i >= seen.len() || seen[i] {
            return Err(err("category order must contain each index exactly once"));
        }
        seen[i] = true;
    }
    if seen.iter().any(|x| !*x) {
        return Err(err("category order must contain each index exactly once"));
    }
    let mut requests = guides::plan(plan, data, scale, theme, constraints)?;
    requests.extend(timeaxis::requests(plan, data, theme, constraints)?);
    requests.sort_by(|a, b| a.id.cmp(&b.id));
    requests.dedup_by(|a, b| a.id == b.id);
    if !measurements.entries.is_empty() && measurements.context != constraints.measurement_context {
        return Err(err("measurement context does not match layout context"));
    }
    let missing: Vec<_> = requests
        .iter()
        .filter(|r| !measurements.entries.contains_key(&r.id))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return if constraints.max_rounds == 0 {
            Err(err("measurement round limit is zero"))
        } else {
            Ok(LayoutStep::NeedMeasurements {
                requests: missing,
                round: 1,
            })
        };
    }
    if measurements.context != constraints.measurement_context {
        return Err(err("measurement context does not match layout context"));
    }
    for r in &requests {
        let m = &measurements.entries[&r.id];
        if !m.width.is_finite()
            || !m.height.is_finite()
            || m.width < 0.
            || m.height <= 0.
            || m.baseline
                .is_some_and(|b| !b.is_finite() || b < 0. || b > m.height)
        {
            return Err(err(format!("invalid text metrics for {}", r.id)));
        }
        if r.max_width.is_some_and(|max| m.width > max + 1e-6) {
            return Err(err(format!("measured {} exceeds its maximum width", r.id)));
        }
    }
    if orientation::horizontal_if_needed(plan, data, theme, constraints, measurements)? {
        let mut horizontal = plan.clone();
        horizontal.orientation = Orientation::Horizontal;
        let mut step = solve(&horizontal, data, scale, theme, constraints, measurements)?;
        if let LayoutStep::Complete { chart } = &mut step {
            chart
                .summary
                .insert("orientation".into(), json!("horizontal"));
            chart.findings.push(Finding::new(
                "R9-10",
                "info",
                "measured category names require horizontal marks, with names at the left",
            ));
        }
        return Ok(step);
    }
    let mut by_key = BTreeMap::new();
    for r in &requests {
        by_key.insert(
            (r.role.clone(), content_key(&r.content)),
            (r, &measurements.entries[&r.id]),
        );
    }
    let mut primitives = vec![];
    let mut y = 12.;
    for (role, part) in [
        (
            "title",
            if plan.spec.raw["title"].is_object() {
                &plan.spec.raw["title"]["text"]
            } else {
                &plan.spec.raw["title"]
            },
        ),
        ("subtitle", &plan.spec.raw["title"]["descriptive"]),
    ] {
        if let Some(c) = guides::content(part)? {
            let (r, m) = by_key
                .get(&(role.into(), content_key(&c)))
                .ok_or_else(|| err("missing title measurement"))?;
            primitives.push(text(r, m, 12., y, "start"));
            y += m.height + 5.;
        }
    }
    let width = constraints.width;
    let horizontal = plan.orientation == Orientation::Horizontal;
    let formatted_scale = scale
        .map(|s| chartgen_format::scale_labels(&plan.spec, s))
        .transpose()?;
    let left = if horizontal {
        (0..data.categories.len())
            .map(|i| {
                guides::category_content(plan, data, i).map(|c| {
                    by_key
                        .get(&("category-label".into(), content_key(&c)))
                        .map(|(_, m)| m.width)
                        .unwrap_or(0.)
                })
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .fold(0., f64::max)
            + 20.
    } else {
        52.
    };
    let bottom = if timeaxis::applies(plan, data) {
        53.
    } else if horizontal {
        35.
    } else {
        (0..data.categories.len())
            .map(|i| {
                guides::category_content(plan, data, i).map(|c| {
                    by_key
                        .get(&("category-label".into(), content_key(&c)))
                        .map(|(_, m)| m.height)
                        .unwrap_or(0.)
                })
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .fold(0., f64::max)
            + 22.
    };
    let plot_h = (data.categories.len() as f64 * 26.).clamp(125., 360.);
    let has_unit = formatted_scale
        .as_ref()
        .is_some_and(|f| !f.unit_line.is_empty());
    let mut top_gap = if !horizontal && has_unit && data.series.len() > 1 {
        38.
    } else if !horizontal && has_unit || data.series.len() > 1 {
        25.
    } else {
        12.
    };
    let visible_comparisons = data
        .comparisons
        .iter()
        .filter(|c| c.valid && c.render != "none")
        .count();
    top_gap += visible_comparisons as f64 * (theme.font_size + 5.);
    let mut height = constraints.height.unwrap_or(y + plot_h + bottom + top_gap);
    let plot = Bounds {
        x: left,
        y: y + top_gap,
        width: width - left - 16.,
        height: height - y - bottom - top_gap,
    };
    if plot.width < 30. || plot.height < 30. {
        return Err(err("measured labels leave no usable plot area"));
    }
    let n = order.len();
    let mut findings = vec![];
    if matches!(
        plan.chart_type,
        ChartType::Pie
            | ChartType::Donut
            | ChartType::Treemap
            | ChartType::Table
            | ChartType::Heatmap
            | ChartType::ActionDots
            | ChartType::FunnelBars
    ) {
        return Err(err(format!(
            "{} needs a family-specific layout that is not yet implemented",
            plan.chart_type.as_str()
        )));
    }
    if matches!(
        plan.chart_type,
        ChartType::WrappedBar | ChartType::Arrow | ChartType::Bumps | ChartType::LineTotal
    ) {
        return Err(err(format!(
            "{} needs a family-specific layout that is not yet implemented",
            plan.chart_type.as_str()
        )));
    }
    let scale = scale.ok_or_else(|| err("cartesian chart requires a trained value scale"))?;
    if matches!(
        plan.chart_type,
        ChartType::Bar
            | ChartType::ThinBar
            | ChartType::StackedBar
            | ChartType::StackedBar100
            | ChartType::Waterfall
            | ChartType::StackedArea
            | ChartType::StackedArea100
    ) && (scale.lo > Decimal::ZERO || scale.hi < Decimal::ZERO)
    {
        return Err(err("length encoded chart scale must include zero"));
    }
    if scale.lo <= Decimal::ZERO && scale.hi >= Decimal::ZERO {
        let zero = geometry_scale(scale, 0., &plot, horizontal)?;
        if horizontal {
            primitives.push(line(
                zero,
                plot.y,
                zero,
                plot.y + plot.height,
                &theme.grid,
                "zero-line",
            ));
        } else {
            primitives.push(line(
                plot.x,
                zero,
                plot.x + plot.width,
                zero,
                &theme.grid,
                "zero-line",
            ));
        }
    }
    let scale_index = primitives.len();
    let mut scale_primitives = vec![];
    let formatted = formatted_scale
        .as_ref()
        .ok_or_else(|| err("missing formatted scale"))?;
    let inline = formatted.top.inline();
    let top_inline_fits = by_key
        .get(&(
            String::from("scale-label-formatted"),
            content_key(&TextContent::plain(&inline)),
        ))
        .is_some_and(|(_, m)| m.width <= left - 10.);
    for (i, tick) in scale.ticks.iter().enumerate() {
        let v = tick.to_f64().ok_or_else(|| err("invalid scale tick"))?;
        let pos = geometry_scale(scale, v, &plot, horizontal)?;
        let label = if horizontal {
            &formatted.bare[i]
        } else if i == scale.ticks.len() - 1 && top_inline_fits {
            &inline
        } else {
            &formatted.labels[i]
        };
        let (r, m) = by_key
            .get(&(
                String::from("scale-label-formatted"),
                content_key(&TextContent::plain(label)),
            ))
            .ok_or_else(|| err("missing formatted scale measurement"))?;
        if horizontal {
            scale_primitives.push(line(
                pos,
                plot.y,
                pos,
                plot.y + plot.height,
                &theme.grid,
                "grid",
            ));
            scale_primitives.push(text(r, m, pos, plot.y + plot.height + 4., "middle"));
        } else {
            scale_primitives.push(line(
                plot.x,
                pos,
                plot.x + plot.width,
                pos,
                &theme.grid,
                "grid",
            ));
            if m.width > left - 10. {
                return Err(err("value scale labels do not fit the left column"));
            }
            scale_primitives.push(text(r, m, plot.x - 6., pos - m.height / 2., "end"));
        }
    }
    if horizontal && !formatted.title.is_empty() {
        let (r, m) = by_key
            .get(&(
                String::from("scale-title"),
                content_key(&TextContent::plain(&formatted.title)),
            ))
            .ok_or_else(|| err("missing scale title measurement"))?;
        if m.width > plot.width {
            return Err(err("scale title is wider than the plot"));
        }
        scale_primitives.push(text(
            r,
            m,
            plot.x + plot.width / 2.,
            plot.y + plot.height + 18.,
            "middle",
        ));
    }
    if !horizontal && !top_inline_fits && !formatted.unit_line.is_empty() {
        let (r, m) = by_key
            .get(&(
                String::from("unit-line"),
                content_key(&TextContent::plain(&formatted.unit_line)),
            ))
            .ok_or_else(|| err("missing unit line measurement"))?;
        if m.width > plot.width {
            return Err(err("unit phrase is wider than the plot"));
        }
        scale_primitives.push(text(
            r,
            m,
            plot.x,
            plot.y - if data.series.len() > 1 { 26. } else { 16. },
            "start",
        ));
    }
    let band = if horizontal {
        plot.height / n as f64
    } else {
        plot.width / n as f64
    };
    let show_values = guides::show_value_labels(plan, data, Some(scale));
    let formatted_values = if show_values {
        Some(guides::value_labels(plan, data)?)
    } else {
        None
    };
    let mut value_texts = vec![];
    let mut value_boxes: Vec<Bounds> = vec![];
    if !timeaxis::applies(plan, data) {
        for (slot, &idx) in order.iter().enumerate() {
            let (_, m) = by_key
                .get(&(
                    "category-label".to_owned(),
                    content_key(&guides::category_content(plan, data, idx)?),
                ))
                .ok_or_else(|| err("missing category label measurement"))?;
            if (horizontal && m.height > band - 2.) || (!horizontal && m.width > band - 4.) {
                return Err(err(format!(
                    "category label {} overlaps its neighbour",
                    slot
                )));
            }
        }
    }
    if timeaxis::applies(plan, data) {
        timeaxis::place(
            plan,
            data,
            theme,
            constraints,
            measurements,
            &requests,
            &plot,
            &order,
            &mut primitives,
            &mut findings,
        )?;
    } else {
        for (slot, &idx) in order.iter().enumerate() {
            let (r, m) = by_key
                .get(&(
                    "category-label".into(),
                    content_key(&guides::category_content(plan, data, idx)?),
                ))
                .ok_or_else(|| err("missing category label measurement"))?;
            if horizontal {
                primitives.push(text(
                    r,
                    m,
                    plot.x - 7.,
                    plot.y + (slot as f64 + 0.5) * band - m.height / 2.,
                    "end",
                ));
            } else {
                primitives.push(text(
                    r,
                    m,
                    plot.x + (slot as f64 + 0.5) * band,
                    plot.y + plot.height + 6.,
                    "middle",
                ));
            }
        }
    }
    match plan.chart_type {
        ChartType::Bar
        | ChartType::ThinBar
        | ChartType::Dot
        | ChartType::StackedBar
        | ChartType::StackedBar100
        | ChartType::Waterfall
        | ChartType::StackedArea
        | ChartType::StackedArea100
        | ChartType::Line
        | ChartType::Step => {}
        _ => return Err(err("chart family has no portable layout")),
    }
    if matches!(plan.chart_type, ChartType::Line | ChartType::Step) && horizontal {
        return Err(err("horizontal line and step layouts are not supported"));
    }
    if matches!(
        plan.chart_type,
        ChartType::StackedBar
            | ChartType::StackedBar100
            | ChartType::StackedArea
            | ChartType::StackedArea100
    ) && data.series.len() < 2
    {
        return Err(err("stacked bars require multiple series"));
    }
    if plan.chart_type == ChartType::Waterfall && data.series.len() != 1 {
        return Err(err("waterfall requires one series"));
    }
    if matches!(
        plan.chart_type,
        ChartType::StackedBar | ChartType::StackedBar100
    ) && data
        .series
        .iter()
        .flat_map(|s| &s.values)
        .any(|d| d.value.is_some_and(|v| v < Decimal::ZERO))
    {
        return Err(err(
            "signed stacks need separate positive and negative accumulation",
        ));
    }
    if matches!(
        plan.chart_type,
        ChartType::StackedArea | ChartType::StackedArea100
    ) {
        draw_areas(&mut primitives, plan, data, &order, scale, &plot, theme)?;
    } else {
        let mut running = 0.;
        let marks_from = primitives.len();
        for (si, s) in data.series.iter().enumerate() {
            let col = colours::series(plan, theme, si)?;
            if matches!(plan.chart_type, ChartType::Line | ChartType::Step) {
                let mut run: Vec<Point> = vec![];
                let mut markers = vec![];
                for (slot, &idx) in order.iter().enumerate() {
                    if let Some(v) = value(&s.values[idx])? {
                        let p = Point {
                            x: plot.x + (slot as f64 + 0.5) * band,
                            y: geometry_scale(scale, v, &plot, false)?,
                        };
                        if plan.chart_type == ChartType::Step && !run.is_empty() {
                            let last = run.last().unwrap();
                            run.push(Point { x: p.x, y: last.y });
                        }
                        run.push(p.clone());
                        markers.push(Primitive::Circle {
                            center: p,
                            radius: 2.8,
                            paint: paint(Some(col.clone()), None, 0.),
                            meta: meta("data-mark", format!("{}-{idx}", s.id)),
                        });
                    } else {
                        flush_path(&mut primitives, &mut run, &col, &s.id);
                        let label = guides::missing_label(plan, &s.values[idx].status)?;
                        let (r, m) = by_key
                            .get(&(
                                String::from("missing-note"),
                                content_key(&TextContent::plain(&label)),
                            ))
                            .ok_or_else(|| err("missing status measurement"))?;
                        let x = plot.x + (slot as f64 + 0.5) * band;
                        if x - m.width / 2. < 0. || x + m.width / 2. > width {
                            return Err(err("missing line status label exceeds chart width"));
                        }
                        let y = plot.y + 4. + si as f64 * (m.height + 3.);
                        if y + m.height > plot.y + plot.height {
                            return Err(err("missing line status labels exceed plot height"));
                        }
                        primitives.push(text(r, m, x, y, "middle"));
                        findings.push(Finding::new(
                            "missing-value",
                            "warning",
                            format!(
                                "{} / {} has no drawable value",
                                s.name, data.categories[idx]
                            ),
                        ));
                    }
                }
                flush_path(&mut primitives, &mut run, &col, &s.id);
                primitives.extend(markers);
                continue;
            }
            for (slot, &idx) in order.iter().enumerate() {
                let Some(v) = value(&s.values[idx])? else {
                    let label = guides::missing_label(plan, &s.values[idx].status)?;
                    let (r, m) = by_key
                        .get(&(
                            String::from("missing-note"),
                            content_key(&TextContent::plain(&label)),
                        ))
                        .ok_or_else(|| err("missing status measurement"))?;
                    let clustered = data.series.len() > 1;
                    let share = if clustered {
                        1. / data.series.len() as f64
                    } else {
                        1.
                    };
                    let centre = if clustered {
                        slot as f64 + (si as f64 + 0.5) * share
                    } else {
                        slot as f64 + 0.5
                    };
                    let origin = if scale.lo <= Decimal::ZERO && scale.hi >= Decimal::ZERO {
                        geometry_scale(scale, 0., &plot, horizontal)?
                    } else if horizontal {
                        plot.x
                    } else {
                        plot.y + plot.height
                    };
                    let (x, y, anchor, bx) = if horizontal {
                        let x = (origin + 4.).min(width - m.width);
                        (x, plot.y + centre * band - m.height / 2., "start", x)
                    } else {
                        let x = plot.x + centre * band;
                        (x, origin - m.height - 4., "middle", x - m.width / 2.)
                    };
                    if bx < 0.
                        || bx + m.width > width
                        || y < 0.
                        || y + m.height > height
                        || (!horizontal && m.width > band * share - 2.)
                    {
                        return Err(err("missing status label does not fit category band"));
                    }
                    primitives.push(text(r, m, x, y, anchor));
                    findings.push(Finding::new(
                        "missing-value",
                        "warning",
                        format!(
                            "{} / {} has no drawable value",
                            s.name, data.categories[idx]
                        ),
                    ));
                    continue;
                };
                let clustered = data.series.len() > 1
                    && !matches!(
                        plan.chart_type,
                        ChartType::StackedBar | ChartType::StackedBar100
                    );
                let share = if clustered {
                    1. / data.series.len() as f64
                } else {
                    1.
                };
                let center = if clustered {
                    slot as f64 + (si as f64 + 0.5) * share
                } else {
                    slot as f64 + 0.5
                };
                let thick = band
                    * share
                    * if plan.chart_type == ChartType::ThinBar {
                        0.22
                    } else {
                        0.7
                    };
                let id = format!("{}-{idx}", s.id);
                let col = colours::mark(plan, theme, si, idx, &col)?;
                if let Some(labels) = &formatted_values {
                    if let Some(label) = &labels[si][idx] {
                        let (r, m) = by_key
                            .get(&(
                                String::from("value-label"),
                                content_key(&TextContent::plain(label)),
                            ))
                            .ok_or_else(|| err("missing value label measurement"))?;
                        let endpoint = geometry_scale(scale, v, &plot, horizontal)?;
                        let (x, y, anchor, bx) = if horizontal {
                            let x = if v >= 0. {
                                endpoint + 3.
                            } else {
                                endpoint - m.width - 3.
                            };
                            (x, plot.y + center * band - m.height / 2., "start", x)
                        } else {
                            let x = plot.x + center * band;
                            (
                                x,
                                if v >= 0. {
                                    endpoint - m.height - 3.
                                } else {
                                    endpoint + 3.
                                },
                                "middle",
                                x - m.width / 2.,
                            )
                        };
                        value_boxes.push(Bounds {
                            x: bx,
                            y,
                            width: m.width,
                            height: m.height,
                        });
                        value_texts.push(text(r, m, x, y, anchor));
                    }
                }
                if plan.chart_type == ChartType::Dot {
                    let pos = geometry_scale(scale, v, &plot, horizontal)?;
                    let c = if horizontal {
                        Point {
                            x: pos,
                            y: plot.y + center * band,
                        }
                    } else {
                        Point {
                            x: plot.x + center * band,
                            y: pos,
                        }
                    };
                    primitives.push(Primitive::Circle {
                        center: c,
                        radius: 4.,
                        paint: paint(Some(col.clone()), None, 0.),
                        meta: meta("data-mark", id),
                    });
                    continue;
                }
                let (start, end) = if plan.chart_type == ChartType::Waterfall {
                    let a = running;
                    running += v;
                    (a, running)
                } else if matches!(
                    plan.chart_type,
                    ChartType::StackedBar | ChartType::StackedBar100
                ) {
                    let below: f64 = data.series[..si]
                        .iter()
                        .filter_map(|prior| prior.values[idx].value.and_then(|d| d.to_f64()))
                        .sum();
                    if plan.chart_type == ChartType::StackedBar100 {
                        let total: f64 = data
                            .series
                            .iter()
                            .filter_map(|series| series.values[idx].value.and_then(|d| d.to_f64()))
                            .sum();
                        if total <= 0. {
                            return Err(err("100% stack requires positive category totals"));
                        }
                        (below / total * 100., (below + v) / total * 100.)
                    } else {
                        (below, below + v)
                    }
                } else {
                    (0., v)
                };
                let a = geometry_scale(scale, start, &plot, horizontal)?;
                let b = geometry_scale(scale, end, &plot, horizontal)?;
                if horizontal {
                    primitives.push(rect(
                        a.min(b),
                        plot.y + center * band - thick / 2.,
                        (a - b).abs(),
                        thick,
                        col.clone(),
                        "data-mark",
                        id,
                    ));
                } else {
                    primitives.push(rect(
                        plot.x + center * band - thick / 2.,
                        a.min(b),
                        thick,
                        (a - b).abs(),
                        col.clone(),
                        "data-mark",
                        id,
                    ));
                }
            }
        }
        // the colour plan's classes on the marks, as the stylesheet colours them
        colours::classify_marks(plan, data, &mut primitives[marks_from..]);
    }
    for (ci, comparison) in data.comparisons.iter().enumerate() {
        if !comparison.valid {
            findings.push(Finding::new(
                "comparison-validity",
                "warning",
                format!(
                    "{} is not a valid comparator and is not drawn",
                    comparison.label
                ),
            ));
            continue;
        }
        if comparison.render == "none" {
            continue;
        }
        let label = guides::comparison_content(plan, data, ci)?;
        let (r, m) = by_key
            .get(&(String::from("comparison-label"), content_key(&label)))
            .ok_or_else(|| err("missing comparison label measurement"))?;
        let preceding = data.comparisons[..ci]
            .iter()
            .filter(|c| c.valid && c.render != "none")
            .count();
        let label_y = plot.y
            - (visible_comparisons - preceding) as f64 * (theme.font_size + 5.)
            - if data.series.len() > 1 { 13. } else { 0. };
        if label_y < 0. || plot.x + 15. + m.width > width {
            return Err(err("comparison key does not fit"));
        }
        primitives.push(line(
            plot.x,
            label_y + m.height / 2.,
            plot.x + 9.,
            label_y + m.height / 2.,
            &theme.text,
            "comparison-key",
        ));
        primitives.push(text(r, m, plot.x + 15., label_y, "start"));
        let target_series = comparison
            .series
            .as_ref()
            .and_then(|id| data.series.iter().position(|s| &s.id == id))
            .unwrap_or(0);
        if comparison.series.is_some()
            && data
                .series
                .get(target_series)
                .is_some_and(|s| Some(&s.id) != comparison.series.as_ref())
        {
            return Err(err(format!(
                "unknown comparison target series for {}",
                comparison.id
            )));
        }
        let wants_line = comparison.render == "line"
            || (comparison.render == "series"
                && matches!(plan.chart_type, ChartType::Line | ChartType::Step))
            || (comparison.values.len() == 1 && comparison.render != "mark")
            || matches!(plan.chart_type, ChartType::Line | ChartType::Step);
        if wants_line && comparison.values.len() == 1 {
            let v = value(&comparison.values[0])?
                .ok_or_else(|| err("constant comparison value is unknown"))?;
            let pos = geometry_scale(scale, v, &plot, horizontal)?;
            primitives.push(if horizontal {
                line(
                    pos,
                    plot.y,
                    pos,
                    plot.y + plot.height,
                    &theme.text,
                    "comparison-line",
                )
            } else {
                line(
                    plot.x,
                    pos,
                    plot.x + plot.width,
                    pos,
                    &theme.text,
                    "comparison-line",
                )
            });
        } else if wants_line {
            if horizontal {
                return Err(err(
                    "varying comparison line on horizontal chart is unsupported",
                ));
            }
            let mut run = vec![];
            for (slot, &idx) in order.iter().enumerate() {
                if let Some(v) = value(&comparison.values[idx])? {
                    run.push(Point {
                        x: plot.x + (slot as f64 + 0.5) * band,
                        y: geometry_scale(scale, v, &plot, false)?,
                    });
                } else {
                    if run.len() > 1 {
                        primitives.push(Primitive::Path {
                            points: std::mem::take(&mut run),
                            closed: false,
                            paint: paint(None, Some(theme.text.clone()), 1.4),
                            meta: Metadata {
                                role: "comparison-line".into(),
                                data: [("comparison".into(), comparison.id.clone())].into(),
                                ..Default::default()
                            },
                        });
                    }
                    run.clear();
                }
            }
            if run.len() > 1 {
                primitives.push(Primitive::Path {
                    points: run,
                    closed: false,
                    paint: paint(None, Some(theme.text.clone()), 1.4),
                    meta: Metadata {
                        role: "comparison-line".into(),
                        data: [("comparison".into(), comparison.id.clone())].into(),
                        ..Default::default()
                    },
                });
            }
        } else {
            for (slot, &idx) in order.iter().enumerate() {
                let d = &comparison.values[if comparison.values.len() == 1 { 0 } else { idx }];
                let Some(v) = value(d)? else {
                    findings.push(Finding::new(
                        "missing-comparison",
                        "warning",
                        format!(
                            "{} has no value for {}",
                            comparison.label, data.categories[idx]
                        ),
                    ));
                    continue;
                };
                let clustered = data.series.len() > 1;
                let share = if clustered {
                    1. / data.series.len() as f64
                } else {
                    1.
                };
                let centre = if clustered {
                    slot as f64 + (target_series as f64 + 0.5) * share
                } else {
                    slot as f64 + 0.5
                };
                let pos = geometry_scale(scale, v, &plot, horizontal)?;
                let thick = band * share * 0.7;
                primitives.push(if horizontal {
                    line(
                        pos,
                        plot.y + centre * band - thick / 2.,
                        pos,
                        plot.y + centre * band + thick / 2.,
                        &theme.text,
                        "comparison-mark",
                    )
                } else {
                    line(
                        plot.x + centre * band - thick / 2.,
                        pos,
                        plot.x + centre * band + thick / 2.,
                        pos,
                        &theme.text,
                        "comparison-mark",
                    )
                });
            }
        }
    }
    let label_conflict = value_boxes.iter().enumerate().any(|(i, b)| {
        b.x < 0.
            || b.y < 0.
            || b.x + b.width > width
            || b.y + b.height > height
            || value_boxes[..i].iter().any(|other| boxes_overlap(b, other))
            || primitives
                .iter()
                .filter_map(text_box)
                .any(|other| boxes_overlap(b, &other))
    });
    if show_values && !label_conflict {
        primitives.extend(value_texts);
    } else {
        if show_values {
            if plan
                .spec
                .raw
                .pointer("/chart/value_labels")
                .and_then(|v| v.as_str())
                == Some("on")
            {
                return Err(err("requested value labels overlap or exceed the chart"));
            }
            findings.push(Finding::new(
                "value-label-fit",
                "warning",
                "value labels do not fit; retaining the measured scale",
            ));
        }
        primitives.splice(scale_index..scale_index, scale_primitives);
    }
    // Series names remain native measured text, placed as a compact legend above the plot.
    if data.series.len() > 1 {
        let mut x = plot.x;
        for (si, s) in data.series.iter().enumerate() {
            let (r, m) = by_key
                .get(&(
                    "series-label".into(),
                    content_key(&guides::series_content(plan, data, si)?),
                ))
                .ok_or_else(|| err("missing series label measurement"))?;
            if x + m.width + 20. > width - 8. {
                return Err(err("series legend does not fit the chart width"));
            }
            primitives.push(rect(
                x,
                plot.y - 11.,
                7.,
                7.,
                colours::series(plan, theme, si)?,
                "legend-key",
                format!("legend-{}", s.id),
            ));
            primitives.push(text(r, m, x + 11., plot.y - 13., "start"));
            x += m.width + 24.;
        }
    }
    annotations::apply(plan, data, theme, &mut primitives, &mut findings)?;
    comments::place(
        plan,
        data,
        theme,
        constraints,
        &requests,
        measurements,
        &mut primitives,
        &mut height,
        &mut findings,
    )?;
    let mut summary = BTreeMap::new();
    summary.insert("chart_type".into(), json!(plan.chart_type.as_str()));
    summary.insert("categories".into(), json!(n));
    summary.insert("series".into(), json!(data.series.len()));
    summary.insert(
        "orientation".into(),
        json!(if horizontal { "horizontal" } else { "vertical" }),
    );
    Ok(LayoutStep::Complete {
        chart: PlacedChart {
            decisions: vec![],
            width,
            height,
            primitives,
            findings,
            summary,
        },
    })
}
fn flush_path(out: &mut Vec<Primitive>, run: &mut Vec<Point>, col: &str, id: &str) {
    if run.len() > 1 {
        out.push(Primitive::Path {
            points: std::mem::take(run),
            closed: false,
            paint: paint(None, Some(col.into()), 1.6),
            meta: Metadata {
                role: "data-line".into(),
                data: [("series".into(), id.into())].into(),
                ..Default::default()
            },
        });
    } else {
        run.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn fixture() -> (
        SemanticPlan,
        PreparedData,
        TrainedScale,
        Theme,
        LayoutConstraints,
    ) {
        let spec = Spec {
            raw: json!({"title":{"text":"Income"}}),
        };
        let plan = SemanticPlan {
            spec,
            chart_type: ChartType::Bar,
            orientation: Orientation::Vertical,
            order: vec![0, 1],
            layers: vec![],
            coordinates: "cartesian".into(),
            colors: Default::default(),
        };
        let datum = |n: &str| Datum {
            value: Some(Decimal::from_str(n).unwrap()),
            status: "known".into(),
            certainty: "actual".into(),
            complete: true,
            note: None,
            limit: None,
        };
        let data = PreparedData {
            categories: vec!["A".into(), "A".into()],
            x_kind: "nominal".into(),
            series: vec![Series {
                id: "s".into(),
                name: "Income".into(),
                values: vec![datum("10"), datum("20")],
            }],
            irregular: None,
            comparisons: vec![],
            measure: json!({}),
        };
        let scale = TrainedScale {
            lo: Decimal::ZERO,
            hi: Decimal::from(20),
            step: Decimal::from(10),
            ticks: vec![Decimal::ZERO, Decimal::from(10), Decimal::from(20)],
            mode: "linear".into(),
        };
        (
            plan,
            data,
            scale,
            Theme::default(),
            LayoutConstraints::default(),
        )
    }
    #[test]
    fn batched_measurement_and_context_rejection() {
        let (plan, data, scale, theme, constraints) = fixture();
        let empty = Measurements::default();
        let LayoutStep::NeedMeasurements { requests, .. } =
            solve(&plan, &data, Some(&scale), &theme, &constraints, &empty).unwrap()
        else {
            panic!("expected requests")
        };
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.role == "category-label")
                .count(),
            1
        );
        let entries = requests
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    TextMetrics {
                        width: if r.role == "title" { 42. } else { 12. },
                        height: 10.,
                        baseline: None,
                    },
                )
            })
            .collect();
        let mut metrics = Measurements {
            context: constraints.measurement_context.clone(),
            entries,
        };
        let LayoutStep::Complete { chart } =
            solve(&plan, &data, Some(&scale), &theme, &constraints, &metrics).unwrap()
        else {
            panic!("expected chart")
        };
        assert_eq!(
            chart
                .primitives
                .iter()
                .filter(|p| matches!(p,Primitive::Rect{meta,..} if meta.role=="data-mark"))
                .count(),
            2
        );
        assert_eq!(
            chart
                .primitives
                .iter()
                .filter(|p| matches!(p,Primitive::Text{request,..} if request.role=="value-label"))
                .count(),
            2
        );
        assert!(
            !chart
                .primitives
                .iter()
                .any(|p| matches!(p,Primitive::Text{request,..} if request.role=="scale-label"))
        );
        metrics.context = "stale".into();
        assert!(solve(&plan, &data, Some(&scale), &theme, &constraints, &metrics).is_err());
    }
    #[test]
    fn special_data_states_require_visible_encoding() {
        let (plan, mut data, scale, theme, c) = fixture();
        data.series[0].values[0].certainty = "projection".into();
        assert!(
            solve(
                &plan,
                &data,
                Some(&scale),
                &theme,
                &c,
                &Measurements::default()
            )
            .unwrap_err()
            .message
            .contains("certainty")
        );
        // a value's own note is the note layout's (a footnote), no longer refused here
    }
    #[test]
    fn one_series_stack_accumulates_categories_around_zero() {
        let (mut plan, mut data, mut scale, theme, mut c) = fixture();
        plan.chart_type = ChartType::StackedBar;
        plan.order = vec![0, 1];
        plan.spec.raw["data"] = json!({"x":{"values":["A","B"]}});
        plan.spec.raw["insight"] = json!({"targets":[{"category":"B"}]});
        data.categories = vec!["A".into(), "B".into()];
        data.series[0].values[1].value = Some(Decimal::from(-7));
        scale.lo = Decimal::from(-10);
        scale.hi = Decimal::from(20);
        scale.step = Decimal::from(10);
        scale.ticks = vec![
            Decimal::from(-10),
            Decimal::ZERO,
            Decimal::from(10),
            Decimal::from(20),
        ];
        c.width = 300.;
        let LayoutStep::NeedMeasurements { requests, .. } = solve(
            &plan,
            &data,
            Some(&scale),
            &theme,
            &c,
            &Measurements::default(),
        )
        .unwrap() else {
            panic!("expected metrics")
        };
        let entries = requests
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    TextMetrics {
                        width: if r.role == "title" { 45. } else { 12. },
                        height: 10.,
                        baseline: None,
                    },
                )
            })
            .collect();
        let metrics = Measurements {
            context: c.measurement_context.clone(),
            entries,
        };
        let LayoutStep::Complete { chart } =
            solve(&plan, &data, Some(&scale), &theme, &c, &metrics).unwrap()
        else {
            panic!("expected placed stack")
        };
        let marks = chart
            .primitives
            .iter()
            .filter_map(|p| {
                if let Primitive::Rect { bounds, meta, .. } = p {
                    (meta.role == "data-mark").then_some(bounds)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(marks.len(), 2);
        assert!(marks[0].y + marks[0].height <= marks[1].y + 1e-8);
        assert!(chart.primitives.iter().any(|p| matches!(p, Primitive::Rect { meta, .. } if meta.id.as_deref() == Some("s-1") && meta.classes.contains(&"highlight".into()))));
    }
    #[test]
    fn insight_highlights_exact_bar_and_preserves_other_hue() {
        let (mut plan, mut data, scale, theme, c) = fixture();
        plan.spec.raw["data"] = json!({"x":{"values":["A","B"]}});
        plan.spec.raw["insight"] = json!({"targets":[{"category":"B"}]});
        data.categories = vec!["A".into(), "B".into()];
        let LayoutStep::NeedMeasurements { requests, .. } = solve(
            &plan,
            &data,
            Some(&scale),
            &theme,
            &c,
            &Measurements::default(),
        )
        .unwrap() else {
            panic!("expected metrics")
        };
        let entries = requests
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    TextMetrics {
                        width: if r.role == "title" { 45. } else { 12. },
                        height: 10.,
                        baseline: None,
                    },
                )
            })
            .collect();
        let metrics = Measurements {
            context: c.measurement_context.clone(),
            entries,
        };
        let LayoutStep::Complete { chart } =
            solve(&plan, &data, Some(&scale), &theme, &c, &metrics).unwrap()
        else {
            panic!("expected chart")
        };
        let marks = chart
            .primitives
            .iter()
            .filter_map(|p| {
                if let Primitive::Rect { paint, meta, .. } = p {
                    (meta.role == "data-mark").then_some((paint, meta))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].0.fill.as_deref(), Some(theme.palette[0].as_str()));
        assert_eq!(marks[1].1.id.as_deref(), Some("s-1"));
        assert!(marks[1].1.classes.contains(&"highlight".into()));
        assert_ne!(marks[1].0.fill, marks[0].0.fill);
    }
    #[test]
    fn comment_keeps_measured_text_and_exact_anchor() {
        let (mut plan, mut data, scale, theme, c) = fixture();
        plan.spec.raw["data"] = json!({"x":{"values":["A","B"]}});
        plan.spec.raw["comments"] = json!([{"text":"A marked point","anchors":[{"series":"s","category":"B"}],"placement":"below"}]);
        data.categories = vec!["A".into(), "B".into()];
        let LayoutStep::NeedMeasurements { requests, .. } = solve(
            &plan,
            &data,
            Some(&scale),
            &theme,
            &c,
            &Measurements::default(),
        )
        .unwrap() else {
            panic!("expected metrics")
        };
        assert!(
            requests
                .iter()
                .any(|r| r.role == "comment" && r.content == TextContent::plain("A marked point"))
        );
        let entries = requests
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    TextMetrics {
                        width: if r.role == "comment" { 65. } else { 12. },
                        height: 10.,
                        baseline: None,
                    },
                )
            })
            .collect();
        let metrics = Measurements {
            context: c.measurement_context.clone(),
            entries,
        };
        let LayoutStep::Complete { chart } =
            solve(&plan, &data, Some(&scale), &theme, &c, &metrics).unwrap()
        else {
            panic!("expected chart")
        };
        assert!(chart.primitives.iter().any(
            |p| matches!(p, Primitive::Text { meta, .. } if meta.id.as_deref() == Some("comment-0"))
        ));
        assert!(
            chart
                .primitives
                .iter()
                .any(|p| matches!(p, Primitive::Path { meta, .. } if meta.role == "comment-arrow"))
        );
        assert!(chart.height > c.height.unwrap_or(0.));
    }
    #[test]
    fn custom_status_note_is_requested_and_drawn_verbatim() {
        let (mut plan, mut data, scale, theme, c) = fixture();
        plan.spec.raw["data"] = json!({"x":{"values":["A","B"]}});
        plan.spec.raw["labels"] = json!({"notes":{"missing":"n/a"}});
        data.categories = vec!["A".into(), "B".into()];
        data.series[0].values[0].value = None;
        data.series[0].values[0].status = "missing".into();
        let LayoutStep::NeedMeasurements { requests, .. } = solve(
            &plan,
            &data,
            Some(&scale),
            &theme,
            &c,
            &Measurements::default(),
        )
        .unwrap() else {
            panic!("expected metrics")
        };
        assert!(
            requests
                .iter()
                .any(|r| r.role == "missing-note" && r.content == TextContent::plain("n/a"))
        );
        let entries = requests
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    TextMetrics {
                        width: 12.,
                        height: 10.,
                        baseline: None,
                    },
                )
            })
            .collect();
        let metrics = Measurements {
            context: c.measurement_context.clone(),
            entries,
        };
        let LayoutStep::Complete { chart } =
            solve(&plan, &data, Some(&scale), &theme, &c, &metrics).unwrap()
        else {
            panic!("expected chart")
        };
        assert!(chart.primitives.iter().any(|p| matches!(p, Primitive::Text { request, .. } if request.role == "missing-note" && request.content == TextContent::plain("n/a"))));
    }
    #[test]
    fn monthly_time_axis_uses_measured_narrow_labels_without_dropping_marks() {
        let (mut plan, mut data, scale, theme, mut c) = fixture();
        let months = (1..=12).map(|m| format!("2022-{m:02}")).collect::<Vec<_>>();
        plan.spec.raw["data"] = json!({"x":{"kind":"time","grid":"month","values":months}});
        data.categories = (1..=12).map(|m| format!("2022-{m:02}")).collect();
        data.x_kind = "time".into();
        data.series[0].values = vec![data.series[0].values[0].clone(); 12];
        plan.order = (0..12).collect();
        c.width = 240.;
        let LayoutStep::NeedMeasurements { requests, .. } = solve(
            &plan,
            &data,
            Some(&scale),
            &theme,
            &c,
            &Measurements::default(),
        )
        .unwrap() else {
            panic!("expected metrics")
        };
        let entries = requests
            .iter()
            .map(|r| {
                let width = if r.role == "time-label" {
                    if r.content.readable().len() == 1 {
                        8.
                    } else {
                        28.
                    }
                } else if r.role == "time-label-2" {
                    36.
                } else {
                    12.
                };
                (
                    r.id.clone(),
                    TextMetrics {
                        width,
                        height: 10.,
                        baseline: None,
                    },
                )
            })
            .collect();
        let metrics = Measurements {
            context: c.measurement_context.clone(),
            entries,
        };
        let LayoutStep::Complete { chart } =
            solve(&plan, &data, Some(&scale), &theme, &c, &metrics).unwrap()
        else {
            panic!("expected chart")
        };
        assert_eq!(
            chart
                .primitives
                .iter()
                .filter(|p| matches!(p,Primitive::Rect{meta,..} if meta.role=="data-mark"))
                .count(),
            12
        );
        assert_eq!(
            chart
                .primitives
                .iter()
                .filter(|p| matches!(p,Primitive::Text{request,..} if request.role=="time-label"))
                .count(),
            12
        );
        assert!(
            !chart
                .primitives
                .iter()
                .any(|p| matches!(p,Primitive::Text{request,..} if request.role=="category-label"))
        );
    }
    #[test]
    fn category_aligned_target_is_drawn_as_flat_marks() {
        let (mut plan, mut data, scale, theme, c) = fixture();
        plan.spec.raw["data"] = json!({"comparisons":[{"id":"target","label":"Target"}]});
        plan.layers = vec![
            Layer {
                mark: ChartType::Bar,
                series: vec![0],
                x_channel: "category".into(),
                y_channel: "value".into(),
                source: Some(LayerSource::Series { indices: vec![0] }),
            },
            Layer {
                mark: ChartType::Bar,
                series: vec![],
                x_channel: "category".into(),
                y_channel: "value".into(),
                source: Some(LayerSource::Comparison {
                    index: 0,
                    render: "series".into(),
                }),
            },
        ];
        data.comparisons.push(PreparedComparison {
            id: "target".into(),
            kind: "target".into(),
            label: "Target".into(),
            values: data.series[0].values.clone(),
            render: "series".into(),
            valid: true,
            series: None,
        });
        let LayoutStep::NeedMeasurements { requests, .. } = solve(
            &plan,
            &data,
            Some(&scale),
            &theme,
            &c,
            &Measurements::default(),
        )
        .unwrap() else {
            panic!()
        };
        let m = Measurements {
            context: c.measurement_context.clone(),
            entries: requests
                .into_iter()
                .map(|r| {
                    (
                        r.id,
                        TextMetrics {
                            width: 12.,
                            height: 10.,
                            baseline: None,
                        },
                    )
                })
                .collect(),
        };
        let LayoutStep::Complete { chart } =
            solve(&plan, &data, Some(&scale), &theme, &c, &m).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            chart
                .primitives
                .iter()
                .filter(|p| matches!(p,Primitive::Line{meta,..} if meta.role=="comparison-mark"))
                .count(),
            2
        );
    }
    #[test]
    fn missing_bar_has_measured_status_text_and_stack_refuses_gap() {
        let (mut plan, mut data, scale, theme, c) = fixture();
        data.series[0].values[0].value = None;
        data.series[0].values[0].status = "pending".into();
        let LayoutStep::NeedMeasurements { requests, .. } = solve(
            &plan,
            &data,
            Some(&scale),
            &theme,
            &c,
            &Measurements::default(),
        )
        .unwrap() else {
            panic!()
        };
        let m = Measurements {
            context: c.measurement_context.clone(),
            entries: requests
                .into_iter()
                .map(|r| {
                    (
                        r.id,
                        TextMetrics {
                            width: 12.,
                            height: 10.,
                            baseline: None,
                        },
                    )
                })
                .collect(),
        };
        let LayoutStep::Complete { chart } =
            solve(&plan, &data, Some(&scale), &theme, &c, &m).unwrap()
        else {
            panic!()
        };
        assert!(chart.primitives.iter().any(|p|matches!(p,Primitive::Text{request,..} if request.role=="missing-note"&&request.content.readable()=="(pending)")));
        plan.chart_type = ChartType::StackedBar;
        data.series.push(data.series[0].clone());
        assert!(
            solve(
                &plan,
                &data,
                Some(&scale),
                &theme,
                &c,
                &Measurements::default()
            )
            .unwrap_err()
            .message
            .contains("missing stacked")
        );
    }
}

fn draw_areas(
    out: &mut Vec<Primitive>,
    plan: &SemanticPlan,
    data: &PreparedData,
    order: &[usize],
    scale: &TrainedScale,
    plot: &Bounds,
    theme: &Theme,
) -> Result<()> {
    if plan.orientation != Orientation::Vertical || order.len() < 2 {
        return Err(err(
            "stacked areas require at least two periods and vertical orientation",
        ));
    }
    let normalized = plan.chart_type == ChartType::StackedArea100;
    if normalized && (scale.lo > Decimal::ZERO || scale.hi < Decimal::from(100)) {
        return Err(err("100% area scale must include 0 through 100"));
    }
    let pitch = plot.width / order.len() as f64;
    for (si, s) in data.series.iter().enumerate() {
        let mut upper = vec![];
        let mut lower = vec![];
        let area_colour = colours::series(plan, theme, si)?;
        let flush = |out: &mut Vec<Primitive>, upper: &mut Vec<Point>, lower: &mut Vec<Point>| {
            if upper.len() >= 2 {
                let mut points = std::mem::take(upper);
                points.extend(lower.drain(..).rev());
                out.push(Primitive::Path {
                    points,
                    closed: true,
                    paint: paint(Some(area_colour.clone()), None, 0.),
                    meta: Metadata {
                        role: "data-area".into(),
                        data: [("series".into(), s.id.clone())].into(),
                        ..Default::default()
                    },
                });
            } else {
                upper.clear();
                lower.clear();
            }
        };
        for (slot, &idx) in order.iter().enumerate() {
            let vals: Option<Vec<f64>> = data
                .series
                .iter()
                .map(|part| value(&part.values[idx]))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .collect();
            let Some(vals) = vals else {
                flush(out, &mut upper, &mut lower);
                continue;
            };
            let total: f64 = vals.iter().sum();
            if normalized && total <= 0. {
                return Err(err("100% area period requires a positive total"));
            }
            let low: f64 = vals[..si].iter().sum();
            let high = low + vals[si];
            let factor = if normalized { 100. / total } else { 1. };
            let x = plot.x + (slot as f64 + 0.5) * pitch;
            upper.push(Point {
                x,
                y: geometry_scale(scale, high * factor, plot, false)?,
            });
            lower.push(Point {
                x,
                y: geometry_scale(scale, low * factor, plot, false)?,
            });
        }
        flush(out, &mut upper, &mut lower);
    }
    Ok(())
}
