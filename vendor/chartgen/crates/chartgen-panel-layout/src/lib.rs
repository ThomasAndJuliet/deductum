//! Physical composition of independently selected, trained panels.
use chartgen_layout::{LayoutEngine, guides};
use chartgen_model::{
    Bounds, Error, LayoutConstraints, LayoutStep, Measurements, Metadata, PlacedChart, Point,
    PreparedData, Primitive, Result, SemanticPlan, TextMetrics, TextRequest, Theme, TrainedScale,
};
use chartgen_panels::Panel;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const OUTER_MARGIN: f64 = 12.0;
const GAP: f64 = 14.0;
const HEADING_GAP: f64 = 5.0;
const MIN_CELL_WIDTH: f64 = 120.0;
const MIN_CHART_HEIGHT: f64 = 90.0;

pub struct CompiledPanel {
    pub panel: Panel,
    pub plan: SemanticPlan,
    pub data: PreparedData,
    pub scale: Option<TrainedScale>,
}

pub struct PanelLayout<'a, E: LayoutEngine> {
    pub engine: &'a E,
    pub panels: Vec<CompiledPanel>,
}

fn error(message: impl Into<String>) -> Error {
    Error::new("panel-layout", message)
}
fn validated<'a>(request: &TextRequest, measurements: &'a Measurements) -> Result<&'a TextMetrics> {
    let m = measurements
        .entries
        .get(&request.id)
        .ok_or_else(|| error("missing measured heading"))?;
    if !m.width.is_finite()
        || !m.height.is_finite()
        || m.width < 0.0
        || m.height <= 0.0
        || m.baseline
            .is_some_and(|b| !b.is_finite() || b < 0.0 || b > m.height)
        || request.max_width.is_some_and(|w| m.width > w + 1e-6)
    {
        return Err(error(format!("invalid text metrics for {}", request.id)));
    }
    Ok(m)
}
fn add_request(requests: &mut BTreeMap<String, TextRequest>, request: TextRequest) -> Result<()> {
    if let Some(previous) = requests.get(&request.id) {
        if serde_json::to_value(previous).ok() != serde_json::to_value(&request).ok() {
            return Err(error("text request ID collision across panels"));
        }
    } else {
        requests.insert(request.id.clone(), request);
    }
    Ok(())
}
fn group_id(row: usize, col: usize) -> String {
    format!("panel-r{row}-c{col}")
}
fn prefix_ids(primitive: &mut Primitive, prefix: &str) {
    match primitive {
        Primitive::Rect { meta, .. }
        | Primitive::Line { meta, .. }
        | Primitive::Path { meta, .. }
        | Primitive::Circle { meta, .. }
        | Primitive::Text { meta, .. }
        | Primitive::Use { meta, .. }
        | Primitive::Curve { meta, .. }
        | Primitive::Wedge { meta, .. }
        | Primitive::Shape { meta, .. } => {
            if let Some(id) = &mut meta.id {
                *id = format!("{prefix}-{id}");
            }
        }
        Primitive::Group { children, meta, .. } => {
            if let Some(id) = &mut meta.id {
                *id = format!("{prefix}-{id}");
            }
            for child in children {
                prefix_ids(child, prefix);
            }
        }
    }
}
fn heading(
    request: &TextRequest,
    metrics: &TextMetrics,
    x: f64,
    y: f64,
    id: Option<String>,
) -> Primitive {
    Primitive::Text {
        position: Point { x, y },
        request: request.clone(),
        metrics: metrics.clone(),
        anchor: "start".into(),
        meta: Metadata {
            id,
            role: request.role.clone(),
            ..Default::default()
        },
    }
}
fn title_value(plan: &SemanticPlan, key: &str) -> Value {
    if key == "title" && plan.spec.raw["title"].is_object() {
        plan.spec.raw["title"]["text"].clone()
    } else if key == "subtitle" {
        plan.spec.raw["title"]["descriptive"].clone()
    } else {
        plan.spec.raw["title"].clone()
    }
}

impl<E: LayoutEngine> LayoutEngine for PanelLayout<'_, E> {
    fn solve(
        &self,
        plan: &SemanticPlan,
        _data: &PreparedData,
        _scale: Option<&TrainedScale>,
        theme: &Theme,
        constraints: &LayoutConstraints,
        measurements: &Measurements,
    ) -> Result<LayoutStep> {
        if self.panels.is_empty() {
            return Err(error("panel composition requires at least one panel"));
        }
        if !constraints.width.is_finite()
            || constraints.width <= 0.0
            || constraints
                .height
                .is_some_and(|h| !h.is_finite() || h <= 0.0)
        {
            return Err(error("chart dimensions must be positive and finite"));
        }
        let rows = self.panels[0].panel.rows;
        let columns = self.panels[0].panel.columns;
        if rows == 0 || columns == 0 || rows.checked_mul(columns) != Some(self.panels.len()) {
            return Err(error("panel grid is incomplete"));
        }
        let mut positions = BTreeSet::new();
        for compiled in &self.panels {
            let p = &compiled.panel;
            if p.rows != rows
                || p.columns != columns
                || p.row >= rows
                || p.column >= columns
                || !positions.insert((p.row, p.column))
            {
                return Err(error("panel grid coordinates are inconsistent"));
            }
        }
        let cell_width =
            (constraints.width - 2.0 * OUTER_MARGIN - GAP * (columns - 1) as f64) / columns as f64;
        if !cell_width.is_finite() || cell_width < MIN_CELL_WIDTH {
            return Err(error(format!(
                "panel grid is too narrow: {cell_width:.1} pt per column"
            )));
        }
        if !measurements.entries.is_empty()
            && measurements.context != constraints.measurement_context
        {
            return Err(error("measurement context does not match panel layout"));
        }
        let mut requested = BTreeMap::new();
        let mut top = Vec::new();
        for role in ["title", "subtitle"] {
            if let Some(content) = guides::content(&title_value(plan, role))? {
                let request = guides::request(
                    role,
                    content,
                    if role == "title" {
                        theme.font_size * 1.35
                    } else {
                        theme.font_size
                    },
                    Some(constraints.width - 2.0 * OUTER_MARGIN),
                    theme,
                    constraints,
                )?;
                add_request(&mut requested, request.clone())?;
                top.push(request);
            }
        }
        let mut headings = BTreeMap::new();
        for compiled in &self.panels {
            if let Some(content) = guides::content(&compiled.panel.title)? {
                let request = guides::request(
                    "panel-title",
                    content,
                    theme.font_size,
                    Some(cell_width - 2.0 * OUTER_MARGIN),
                    theme,
                    constraints,
                )?;
                add_request(&mut requested, request.clone())?;
                headings.insert((compiled.panel.row, compiled.panel.column), request);
            }
        }
        let missing_own = requested
            .values()
            .any(|r| !measurements.entries.contains_key(&r.id));
        let mut title_bottom = OUTER_MARGIN;
        let mut top_primitives = Vec::new();
        if !missing_own {
            for request in &top {
                let m = validated(request, measurements)?;
                top_primitives.push(heading(request, m, OUTER_MARGIN, title_bottom, None));
                title_bottom += m.height + HEADING_GAP;
            }
        }
        let mut row_headings = vec![0.0_f64; rows];
        if !missing_own {
            for ((row, _), request) in &headings {
                row_headings[*row] =
                    row_headings[*row].max(validated(request, measurements)?.height);
            }
        }
        let available = constraints
            .height
            .map(|h| h - title_bottom - OUTER_MARGIN - GAP * (rows - 1) as f64);
        let fixed_row_height = available.map(|h| h / rows as f64);
        if let Some(h) = fixed_row_height {
            if !missing_own
                && (0..rows).any(|row| h - row_headings[row] - HEADING_GAP < MIN_CHART_HEIGHT)
            {
                return Err(error("fixed chart height leaves no usable panel area"));
            }
        }
        let mut completed = Vec::new();
        for compiled in &self.panels {
            let p = &compiled.panel;
            let mut subplan = compiled.plan.clone();
            if let Some(raw) = subplan.spec.raw.as_object_mut() {
                raw.remove("title");
                raw.remove("notes");
                raw.remove("panels");
            }
            let child_height = if missing_own {
                None
            } else {
                fixed_row_height.map(|h| h - row_headings[p.row] - HEADING_GAP)
            };
            let child_constraints = LayoutConstraints {
                width: cell_width,
                height: child_height,
                measurement_context: constraints.measurement_context.clone(),
                max_rounds: constraints.max_rounds,
                hooks: None,
            };
            match self.engine.solve(
                &subplan,
                &compiled.data,
                compiled.scale.as_ref(),
                theme,
                &child_constraints,
                measurements,
            )? {
                LayoutStep::NeedMeasurements { requests, .. } => {
                    for request in requests {
                        add_request(&mut requested, request)?;
                    }
                }
                LayoutStep::Complete { chart } => {
                    if !missing_own {
                        completed.push((p.row, p.column, chart));
                    }
                }
                refused @ LayoutStep::Refused { .. } => return Ok(refused),
            }
        }
        let missing: Vec<_> = requested
            .into_values()
            .filter(|r| !measurements.entries.contains_key(&r.id))
            .collect();
        if !missing.is_empty() {
            if constraints.max_rounds == 0 {
                return Err(error("measurement round limit is zero"));
            }
            return Ok(LayoutStep::NeedMeasurements {
                requests: missing,
                round: 1,
            });
        }
        if measurements.context != constraints.measurement_context {
            return Err(error("measurement context does not match panel layout"));
        }
        if completed.len() != self.panels.len() {
            return Err(error("panel layout did not complete after measurements"));
        }
        completed.sort_by_key(|(row, column, _)| (*row, *column));
        let mut row_heights = vec![0.0_f64; rows];
        for (row, _, chart) in &completed {
            if !chart.width.is_finite()
                || !chart.height.is_finite()
                || chart.width < 0.0
                || chart.height < MIN_CHART_HEIGHT
                || chart.width > cell_width + 1e-6
                || fixed_row_height
                    .is_some_and(|h| chart.height > h - row_headings[*row] - HEADING_GAP + 1e-6)
            {
                return Err(error("child panel exceeded its allotted cell"));
            }
            row_heights[*row] =
                row_heights[*row].max(row_headings[*row] + HEADING_GAP + chart.height);
        }
        if let Some(h) = fixed_row_height {
            for height in &mut row_heights {
                *height = h;
            }
        }
        let mut row_y = vec![title_bottom; rows];
        for row in 1..rows {
            row_y[row] = row_y[row - 1] + row_heights[row - 1] + GAP;
        }
        let total_height = constraints
            .height
            .unwrap_or(row_y[rows - 1] + row_heights[rows - 1] + OUTER_MARGIN);
        let mut primitives = top_primitives;
        let mut findings = Vec::new();
        let mut summary = BTreeMap::new();
        for (row, column, mut chart) in completed {
            let prefix = group_id(row, column);
            let x = OUTER_MARGIN + column as f64 * (cell_width + GAP);
            let y = row_y[row];
            if let Some(request) = headings.get(&(row, column)) {
                let m = validated(request, measurements)?;
                primitives.push(heading(
                    request,
                    m,
                    x + OUTER_MARGIN,
                    y,
                    Some(format!("{prefix}-heading")),
                ));
            }
            for primitive in &mut chart.primitives {
                prefix_ids(primitive, &prefix);
            }
            primitives.push(Primitive::Group {
                children: chart.primitives,
                translate: Point {
                    x,
                    y: y + row_headings[row] + HEADING_GAP,
                },
                clip: Some(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: cell_width,
                    height: chart.height,
                }),
                meta: Metadata {
                    id: Some(prefix.clone()),
                    role: "panel".into(),
                    ..Default::default()
                },
            });
            findings.extend(chart.findings);
            summary.insert(prefix, json!(chart.summary));
        }
        summary.insert("panel_rows".into(), json!(rows));
        summary.insert("panel_columns".into(), json!(columns));
        Ok(LayoutStep::Complete {
            chart: PlacedChart {
                decisions: vec![],
                width: constraints.width,
                height: total_height,
                primitives,
                findings,
                summary,
            },
        })
    }
}
