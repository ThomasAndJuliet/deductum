//! Specialist layouts whose geometry cannot be expressed as a single cartesian panel.
use super::*;
#[path = "special/annotate.rs"]
pub(crate) mod annotate;
#[path = "special/arrow.rs"]
pub(crate) mod arrow;
#[path = "special/bumps.rs"]
pub(crate) mod bumps;
#[path = "special/continuous.rs"]
pub(crate) mod continuous;
#[path = "special/events.rs"]
pub(crate) mod events;
#[path = "special/fig.rs"]
pub(crate) mod fig;
#[path = "special/regular.rs"]
pub(crate) mod regular;
#[path = "special/step.rs"]
pub(crate) mod step;
#[path = "special/wrapped.rs"]
pub(crate) mod wrapped;
use chrono::{NaiveDate, NaiveDateTime};
fn funnel_heading(plan: &SemanticPlan) -> &str {
    plan.spec
        .raw
        .pointer("/labels/notes/stage_change")
        .and_then(|v| v.as_str())
        .unwrap_or("% change from each stage to the next")
}

fn reqs(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
    theme: &Theme,
    c: &LayoutConstraints,
) -> Result<Vec<TextRequest>> {
    let mut out = guides::plan(plan, data, scale, theme, c)?;
    if plan.chart_type == ChartType::FunnelBars {
        for label in ["-100%", "-50%", "0%", "50%", "100%", funnel_heading(plan)] {
            out.push(guides::request(
                "change-label",
                TextContent::plain(label),
                theme.font_size * 0.9,
                None,
                theme,
                c,
            )?);
        }
    }
    if plan.chart_type == ChartType::LineTotal {
        let part_top = part_max(plan, data)?;
        for v in [0., part_top / 2., part_top] {
            out.push(guides::request(
                "part-scale",
                TextContent::plain(format!("{v:0.0}")),
                theme.font_size * 0.9,
                None,
                theme,
                c,
            )?);
        }
    }
    if matches!(plan.chart_type, ChartType::Step | ChartType::ThinBar) && data.irregular.is_some() {
        if let Some(rows) = &data.irregular {
            for row in rows {
                for p in row {
                    if p.datum.value.is_none() {
                        out.push(guides::request(
                            "missing-note",
                            TextContent::plain(guides::missing_label(plan, &p.datum.status)?),
                            theme.font_size,
                            None,
                            theme,
                            c,
                        )?);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}
fn measured<'a>(
    requests: &'a [TextRequest],
    m: &'a Measurements,
    role: &str,
    content: &TextContent,
) -> Result<(&'a TextRequest, &'a TextMetrics)> {
    let r = requests
        .iter()
        .find(|r| r.role == role && r.content == *content)
        .ok_or_else(|| err(format!("no text request for {role}")))?;
    Ok((
        r,
        m.entries
            .get(&r.id)
            .ok_or_else(|| err(format!("no metrics for {role}")))?,
    ))
}
fn header(
    plan: &SemanticPlan,
    requests: &[TextRequest],
    m: &Measurements,
    out: &mut Vec<Primitive>,
) -> Result<f64> {
    let mut y = 12.;
    let title = &plan.spec.raw["title"];
    for (role, v) in [
        (
            "title",
            if title.is_object() {
                &title["text"]
            } else {
                title
            },
        ),
        ("subtitle", &title["descriptive"]),
    ] {
        if let Some(c) = guides::content(v)? {
            let (r, ext) = measured(requests, m, role, &c)?;
            out.push(text(r, ext, 12., y, "start"));
            y += ext.height + 5.;
        }
    }
    Ok(y)
}
fn validate_measurements(
    requests: &[TextRequest],
    m: &Measurements,
    c: &LayoutConstraints,
) -> Result<Option<Vec<TextRequest>>> {
    if !m.entries.is_empty() && m.context != c.measurement_context {
        return Err(err("measurement context does not match layout context"));
    }
    let missing = requests
        .iter()
        .filter(|r| !m.entries.contains_key(&r.id))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Ok(Some(missing));
    }
    for r in requests {
        let v = &m.entries[&r.id];
        if !v.width.is_finite()
            || !v.height.is_finite()
            || v.width < 0.
            || v.height <= 0.
            || v.baseline
                .is_some_and(|b| !b.is_finite() || b < 0. || b > v.height)
            || r.max_width.is_some_and(|w| v.width > w + 1e-6)
        {
            return Err(err(format!("invalid metrics for {}", r.id)));
        }
    }
    Ok(None)
}
fn finish(
    plan: &SemanticPlan,
    data: &PreparedData,
    width: f64,
    height: f64,
    primitives: Vec<Primitive>,
    findings: Vec<Finding>,
) -> Result<LayoutStep> {
    let mut summary = BTreeMap::new();
    summary.insert("chart_type".into(), json!(plan.chart_type.as_str()));
    summary.insert("categories".into(), json!(data.categories.len()));
    summary.insert("series".into(), json!(data.series.len()));
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
pub fn solve(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
) -> Result<LayoutStep> {
    if !c.width.is_finite()
        || c.width <= 48.
        || c.height.is_some_and(|h| !h.is_finite() || h <= 48.)
    {
        return Err(err("invalid chart dimensions"));
    }
    if data.series.is_empty() || data.categories.is_empty() {
        return Err(err("chart needs series and categories"));
    }
    for s in &data.series {
        if s.values.len() != data.categories.len() {
            return Err(err("series length differs from category count"));
        }
    }
    if plan.chart_type == ChartType::WrappedBar {
        return wrapped::layout(
            plan,
            data,
            scale.ok_or_else(|| err("wrapped bars need a trained scale"))?,
            theme,
            c,
            m,
        );
    }
    if matches!(
        plan.chart_type,
        ChartType::FunnelBars | ChartType::LineTotal
    ) && data
        .series
        .iter()
        .flat_map(|s| &s.values)
        .any(|d| d.value.is_none())
    {
        return Err(err(
            "missing values in this specialist family need a dedicated visible note layout",
        ));
    }
    if plan.chart_type == ChartType::Bumps {
        return bumps::layout(plan, data, theme, c, m);
    }
    if plan.chart_type == ChartType::Arrow {
        return arrow::layout(
            plan,
            data,
            scale.ok_or_else(|| err("arrow chart needs an absolute scale"))?,
            theme,
            c,
            m,
        );
    }
    if matches!(plan.chart_type, ChartType::Step | ChartType::ThinBar) && data.irregular.is_some() {
        return step::layout(
            plan,
            data,
            scale.ok_or_else(|| err("irregular chart needs a trained value scale"))?,
            theme,
            c,
            m,
        );
    }
    let requests = reqs(plan, data, scale, theme, c)?;
    if let Some(missing) = validate_measurements(&requests, m, c)? {
        return if c.max_rounds == 0 {
            Err(err("measurement round limit is zero"))
        } else {
            Ok(LayoutStep::NeedMeasurements {
                requests: missing,
                round: 1,
            })
        };
    }
    match plan.chart_type {
        ChartType::StackedBar if data.series.len() == 1 => single_stack(
            plan,
            data,
            scale.ok_or_else(|| err("single stack needs a trained cumulative scale"))?,
            theme,
            c,
            m,
            &requests,
        ),
        ChartType::FunnelBars => funnel(
            plan,
            data,
            scale.ok_or_else(|| err("funnel bars need a trained count scale"))?,
            theme,
            c,
            m,
            &requests,
        ),
        ChartType::LineTotal => line_total(
            plan,
            data,
            scale.ok_or_else(|| err("line_total needs a total scale"))?,
            theme,
            c,
            m,
            &requests,
        ),
        _ => Err(err("unsupported specialist chart")),
    }
}
fn single_stack(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: &TrainedScale,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
    requests: &[TextRequest],
) -> Result<LayoutStep> {
    if plan
        .spec
        .raw
        .pointer("/data/x/roles")
        .and_then(|v| v.as_object())
        .is_some_and(|roles| roles.values().any(|v| v.as_str() == Some("total")))
    {
        return Err(err(
            "one-series stack with a total-role category needs exclusion from trained cumulative scale",
        ));
    }
    if !data.comparisons.is_empty() {
        return Err(err("comparisons on a single stack need separate placement"));
    }
    if data.series[0].values.iter().any(|d| d.value.is_none()) {
        return Err(err("missing stack segments need a visible gap and note"));
    }
    let n = data.categories.len();
    let order = if plan.order.is_empty() {
        (0..n).collect::<Vec<_>>()
    } else {
        plan.order.clone()
    };
    if order.len() != n || order.iter().any(|&i| i >= n) || {
        let mut sorted = order.clone();
        sorted.sort();
        sorted.dedup();
        sorted.len() != n
    } {
        return Err(err("single-stack order must contain every category once"));
    }
    let mut out = Vec::new();
    let header_bottom = header(plan, requests, m, &mut out)?;
    let horizontal = plan.orientation == Orientation::Horizontal;
    let max_label = order
        .iter()
        .map(|&i| {
            measured(
                requests,
                m,
                "category-label",
                &guides::category_content(plan, data, i)?,
            )
            .map(|(_, ext)| ext.width)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(0., f64::max);
    let height = c
        .height
        .unwrap_or((c.width * 0.72).clamp(170., 360.) + header_bottom + 28.);
    let plot = if horizontal {
        Bounds {
            x: 48.,
            y: header_bottom + 25.,
            width: c.width - 66. - max_label,
            height: height - header_bottom - 54.,
        }
    } else {
        Bounds {
            x: 48.,
            y: header_bottom + 18.,
            width: c.width - 70. - max_label,
            height: height - header_bottom - 42.,
        }
    };
    if plot.width < 35. || plot.height < 35. {
        return Err(err("single-stack labels leave no usable plot area"));
    }
    let formatted = chartgen_format::scale_labels(&plan.spec, scale)?;
    for (i, tick) in scale.ticks.iter().enumerate() {
        let v = tick.to_f64().ok_or_else(|| err("non-finite stack tick"))?;
        let pos = geometry_scale(scale, v, &plot, horizontal)?;
        let label = &formatted.labels[i];
        let (r, ext) = measured(
            requests,
            m,
            "scale-label-formatted",
            &TextContent::plain(label),
        )?;
        if horizontal {
            out.push(line(
                pos,
                plot.y,
                pos,
                plot.y + plot.height,
                &theme.grid,
                "grid",
            ));
            out.push(text(r, ext, pos, plot.y + plot.height + 4., "middle"));
        } else {
            out.push(line(
                plot.x,
                pos,
                plot.x + plot.width,
                pos,
                &theme.grid,
                "grid",
            ));
            if ext.width > plot.x - 8. {
                return Err(err("stack scale label exceeds left margin"));
            }
            out.push(text(r, ext, plot.x - 5., pos - ext.height / 2., "end"));
        }
    }
    let mut positive = 0.;
    let mut negative = 0.;
    let mut label_boxes: Vec<Bounds> = Vec::new();
    for (slot, &idx) in order.iter().enumerate() {
        let v = value(&data.series[0].values[idx])?.ok_or_else(|| err("missing stack segment"))?;
        let (a, b) = if v >= 0. {
            let start = positive;
            positive += v;
            (start, positive)
        } else {
            let start = negative;
            negative += v;
            (start, negative)
        };
        let p0 = geometry_scale(scale, a, &plot, horizontal)?;
        let p1 = geometry_scale(scale, b, &plot, horizontal)?;
        let start = p0.min(p1);
        let length = (p1 - p0).abs();
        let thickness = if horizontal {
            plot.height * 0.34
        } else {
            plot.width * 0.34
        };
        let bounds = if horizontal {
            Bounds {
                x: start,
                y: plot.y + (plot.height - thickness) / 2.,
                width: length,
                height: thickness,
            }
        } else {
            Bounds {
                x: plot.x + (plot.width - thickness) / 2.,
                y: start,
                width: thickness,
                height: length,
            }
        };
        out.push(Primitive::Rect {
            bounds,
            paint: paint(Some(colours::category(plan, theme, idx)?), None, 0.),
            meta: Metadata {
                id: Some(format!("{}-{idx}", data.series[0].id)),
                role: "data-mark".into(),
                data: [
                    ("category".into(), data.categories[idx].clone()),
                    ("series".into(), data.series[0].id.clone()),
                ]
                .into(),
                ..Default::default()
            },
        });
        let (r, ext) = measured(
            requests,
            m,
            "category-label",
            &guides::category_content(plan, data, idx)?,
        )?;
        let (x, y, anchor, box_x) = if horizontal {
            let x = start + length / 2.;
            let y = plot.y + (plot.height - thickness) / 2.
                - ext.height
                - 5.
                - (slot % 2) as f64 * (ext.height + 2.);
            (x, y, "middle", x - ext.width / 2.)
        } else {
            let x = plot.x + (plot.width + thickness) / 2. + 6.;
            let y = start + length / 2. - ext.height / 2.;
            (x, y, "start", x)
        };
        let label_box = Bounds {
            x: box_x,
            y,
            width: ext.width,
            height: ext.height,
        };
        if label_box.x < 0.
            || label_box.y < 0.
            || label_box.x + label_box.width > c.width
            || label_box.y + label_box.height > height
            || label_boxes
                .iter()
                .any(|other| boxes_overlap(&label_box, other))
        {
            return Err(err(format!(
                "single-stack part label {} does not fit beside its segment",
                data.categories[idx]
            )));
        }
        label_boxes.push(label_box);
        out.push(text(r, ext, x, y, anchor));
    }
    let mut findings = Vec::new();
    annotations::apply(plan, data, theme, &mut out, &mut findings)?;
    finish(plan, data, c.width, height, out, findings)
}

fn funnel(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: &TrainedScale,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
    requests: &[TextRequest],
) -> Result<LayoutStep> {
    if data.series.len() != 1
        || plan.orientation != Orientation::Vertical
        || data.x_kind != "ordinal"
        || data.categories.len() < 2
    {
        return Err(err(
            "funnel bars require one ordinal series, vertical orientation, and at least two stages",
        ));
    }
    let n = data.categories.len();
    let order: Vec<usize> = if plan.order.is_empty() {
        (0..n).collect()
    } else {
        plan.order.clone()
    };
    if order != (0..n).collect::<Vec<_>>() {
        return Err(err("funnel stage order must be the given order"));
    }
    let mut out = vec![];
    let title_bottom = header(plan, requests, m, &mut out)?;
    let plot_x = 52.;
    let plot_w = c.width - plot_x - 16.;
    if plot_w < 50. {
        return Err(err("funnel plot is too narrow"));
    }
    let max = scale
        .hi
        .to_f64()
        .ok_or_else(|| err("invalid count scale"))?;
    if max <= 0. || scale.lo > Decimal::ZERO {
        return Err(err(
            "funnel count scale must include zero and positive counts",
        ));
    }
    let top_y = title_bottom + 12.;
    let top_h = 145.;
    let lower_y = top_y + top_h + 43.;
    let lower_h = 75.;
    let height = c.height.unwrap_or(lower_y + lower_h + 38.);
    if height < lower_y + lower_h + 30. {
        return Err(err("funnel panels exceed requested height"));
    }
    let band = plot_w / n as f64;
    let mut findings = vec![];
    let s = &data.series[0];
    for (i, d) in s.values.iter().enumerate() {
        if let Some(v) = value(d)? {
            if v < 0. {
                return Err(err("funnel counts cannot be negative"));
            }
            let h = top_h * v / max;
            out.push(rect(
                plot_x + (i as f64 + 0.5) * band - band * 0.35,
                top_y + top_h - h,
                band * 0.7,
                h,
                colours::series(plan, theme, 0)?,
                "stage-count",
                format!("stage-{i}"),
            ));
        } else {
            findings.push(Finding::new(
                "missing-value",
                "warning",
                format!("stage {} has no count", data.categories[i]),
            ));
        }
        let (r, ext) = measured(
            requests,
            m,
            "category-label",
            &guides::category_content(plan, data, i)?,
        )?;
        if ext.width > band - 3. {
            return Err(err("funnel stage labels overlap"));
        }
        out.push(text(
            r,
            ext,
            plot_x + (i as f64 + 0.5) * band,
            top_y + top_h + 5.,
            "middle",
        ));
    }
    out.push(line(
        plot_x,
        top_y + top_h,
        plot_x + plot_w,
        top_y + top_h,
        &theme.grid,
        "zero-line",
    ));
    let (r, h) = measured(
        requests,
        m,
        "change-label",
        &TextContent::plain(funnel_heading(plan)),
    )?;
    out.push(text(r, h, plot_x, lower_y - 17., "start"));
    let mut changes = vec![];
    for i in 0..n - 1 {
        let a = value(&s.values[i])?;
        let b = value(&s.values[i + 1])?;
        changes.push(if let (Some(a), Some(b)) = (a, b) {
            if a > 0. {
                Some((b - a) / a * 100.)
            } else {
                None
            }
        } else {
            None
        });
    }
    let extent = changes
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(10., f64::max)
        .ceil()
        .max(10.);
    let baseline = lower_y + lower_h * 0.5;
    out.push(line(
        plot_x,
        baseline,
        plot_x + plot_w,
        baseline,
        &theme.grid,
        "change-zero",
    ));
    for (i, ch) in changes.iter().enumerate() {
        if let Some(v) = ch {
            let x = plot_x + (i as f64 + 1.) * band;
            let y = baseline - v / extent * lower_h * 0.44;
            out.push(rect(
                x - band * 0.3,
                y.min(baseline),
                band * 0.6,
                (y - baseline).abs(),
                color(theme, 1)?,
                "stage-change",
                format!("change-{i}"),
            ));
        } else {
            findings.push(Finding::new(
                "undefined-change",
                "warning",
                format!("change after stage {} is undefined", data.categories[i]),
            ));
        }
    }
    for (label, v) in [
        ("-100%", -100.0_f64),
        ("-50%", -50.),
        ("0%", 0.),
        ("50%", 50.),
        ("100%", 100.),
    ] {
        if v.abs() > extent + 1e-6 {
            continue;
        }
        let y = baseline - v / extent * lower_h * 0.44;
        let (r, ext) = measured(requests, m, "change-label", &TextContent::plain(label))?;
        out.push(text(r, ext, plot_x - 6., y - ext.height / 2., "end"));
    }
    finish(plan, data, c.width, height, out, findings)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(
        kind: ChartType,
    ) -> (
        SemanticPlan,
        PreparedData,
        TrainedScale,
        Theme,
        LayoutConstraints,
    ) {
        let plan = SemanticPlan {
            spec: Spec {
                raw: json!({"title":{"text":"Stages"}}),
            },
            chart_type: kind,
            orientation: if kind == ChartType::WrappedBar {
                Orientation::Horizontal
            } else {
                Orientation::Vertical
            },
            order: vec![0, 1, 2],
            layers: vec![],
            coordinates: "cartesian".into(),
            colors: Default::default(),
        };
        let d = |v: i64| Datum {
            value: Some(Decimal::from(v)),
            status: "known".into(),
            certainty: "actual".into(),
            complete: true,
            note: None,
            limit: None,
        };
        let data = PreparedData {
            categories: vec!["First".into(), "Second".into(), "Third".into()],
            x_kind: "ordinal".into(),
            series: vec![Series {
                id: "s".into(),
                name: "Count".into(),
                values: vec![d(80), d(40), d(20)],
            }],
            irregular: None,
            comparisons: vec![],
            measure: json!({}),
        };
        let scale = TrainedScale {
            lo: Decimal::ZERO,
            hi: Decimal::from(100),
            step: Decimal::from(50),
            ticks: vec![Decimal::ZERO, Decimal::from(50), Decimal::from(100)],
            mode: "linear".into(),
        };
        (
            plan,
            data,
            scale,
            Theme::default(),
            LayoutConstraints {
                width: 300.,
                ..Default::default()
            },
        )
    }
    #[test]
    fn wrapped_and_funnel_finish_after_batch() {
        for kind in [ChartType::FunnelBars] {
            let (p, d, s, t, c) = fixture(kind);
            let LayoutStep::NeedMeasurements { requests, .. } =
                solve(&p, &d, Some(&s), &t, &c, &Measurements::default()).unwrap()
            else {
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
                                width: 20.,
                                height: 10.,
                                baseline: None,
                            },
                        )
                    })
                    .collect(),
            };
            let LayoutStep::Complete { chart } = solve(&p, &d, Some(&s), &t, &c, &m).unwrap()
            else {
                panic!()
            };
            assert!(
                chart
                    .primitives
                    .iter()
                    .any(|p| matches!(p, Primitive::Rect { .. }))
            );
        }
    }
}

fn part_max(plan: &SemanticPlan, data: &PreparedData) -> Result<f64> {
    let mut max: f64 = 1.;
    for (i, s) in data.series.iter().enumerate() {
        if plan.spec.raw["data"]["series"][i]["role"] == "total" {
            continue;
        }
        for d in &s.values {
            if let Some(v) = value(d)? {
                max = max.max(v);
            }
        }
    }
    Ok(max)
}
fn line_total(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: &TrainedScale,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
    requests: &[TextRequest],
) -> Result<LayoutStep> {
    if data.categories.len() < 2
        || data.series.len() < 2
        || plan.orientation != Orientation::Vertical
    {
        return Err(err(
            "line_total needs multiple series and periods, vertical orientation",
        ));
    }
    let total_index =
        (0..data.series.len()).find(|&i| plan.spec.raw["data"]["series"][i]["role"] == "total");
    let parts = (0..data.series.len())
        .filter(|i| Some(*i) != total_index)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(err("line_total needs part series"));
    }
    let mut out = vec![];
    let top = header(plan, requests, m, &mut out)? + 15.;
    let left = 48.;
    let right = c.width - 16.;
    if right - left < 100. {
        return Err(err("line_total is too narrow"));
    }
    let panel_h = 105.;
    let lower_top = top + panel_h + 30.;
    let height = c.height.unwrap_or(lower_top + panel_h + 45.);
    if height < lower_top + panel_h + 35. {
        return Err(err("line_total panels exceed requested height"));
    }
    let total_max = scale
        .hi
        .to_f64()
        .ok_or_else(|| err("invalid total scale"))?;
    let total_min = scale
        .lo
        .to_f64()
        .ok_or_else(|| err("invalid total scale"))?;
    if total_max <= total_min {
        return Err(err("invalid total scale"));
    }
    let part_top = part_max(plan, data)?;
    let dx = (right - left) / (data.categories.len() - 1) as f64;
    let mut totals = vec![];
    for p in 0..data.categories.len() {
        let values = parts
            .iter()
            .map(|&i| value(&data.series[i].values[p]))
            .collect::<Result<Vec<_>>>()?;
        let sum: Option<f64> = values
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .map(|v| v.into_iter().sum());
        let got = if let Some(ti) = total_index {
            let explicit = value(&data.series[ti].values[p])?;
            if let (Some(explicit), Some(sum)) = (explicit, sum) {
                if (explicit - sum).abs() > 1e-6 {
                    return Err(err(format!(
                        "explicit total does not equal parts in {}",
                        data.categories[p]
                    )));
                }
            }
            explicit
        } else {
            sum
        };
        totals.push(got);
    }
    for tick in &scale.ticks {
        let v = tick.to_f64().ok_or_else(|| err("invalid total tick"))?;
        if v < total_min || v > total_max {
            continue;
        }
        let y = top + panel_h - (v - total_min) / (total_max - total_min) * panel_h;
        out.push(line(left, y, right, y, &theme.grid, "total-grid"));
        let (r, ext) = measured(
            requests,
            m,
            "scale-label",
            &TextContent::plain(tick.normalize().to_string()),
        )?;
        out.push(text(r, ext, left - 6., y - ext.height / 2., "end"));
    }
    for v in [0., part_top / 2., part_top] {
        let y = lower_top + panel_h - v / part_top * panel_h;
        out.push(line(left, y, right, y, &theme.grid, "parts-grid"));
        let (r, ext) = measured(
            requests,
            m,
            "part-scale",
            &TextContent::plain(format!("{v:0.0}")),
        )?;
        out.push(text(r, ext, left - 6., y - ext.height / 2., "end"));
    }
    let total_points = totals
        .iter()
        .enumerate()
        .filter_map(|(p, v)| {
            v.map(|v| Point {
                x: left + p as f64 * dx,
                y: top + panel_h - (v - total_min) / (total_max - total_min) * panel_h,
            })
        })
        .collect::<Vec<_>>();
    if total_points.len() == totals.len() {
        out.push(Primitive::Path {
            points: total_points,
            closed: false,
            paint: paint(
                None,
                Some(colours::paint_of(theme, &["total".into()], true, false)?),
                1.8,
            ),
            meta: Metadata {
                role: "total-line".into(),
                ..Default::default()
            },
        });
    } else {
        return Err(err(
            "line_total with missing periods needs segmented total lines",
        ));
    }
    for &si in parts.iter() {
        let s = &data.series[si];
        let mut points = vec![];
        for p in 0..data.categories.len() {
            if let Some(v) = value(&s.values[p])? {
                points.push(Point {
                    x: left + p as f64 * dx,
                    y: lower_top + panel_h - v / part_top * panel_h,
                });
            } else {
                return Err(err("line_total with missing parts needs segmented lines"));
            }
        }
        out.push(Primitive::Path {
            points,
            closed: false,
            paint: paint(None, Some(colours::series_line(plan, theme, si)?), 1.5),
            meta: Metadata {
                role: "part-line".into(),
                data: [("series".into(), s.id.clone())].into(),
                ..Default::default()
            },
        });
    }
    for p in 0..data.categories.len() {
        let (r, ext) = measured(
            requests,
            m,
            "category-label",
            &guides::category_content(plan, data, p)?,
        )?;
        if ext.width > dx - 3. {
            return Err(err("line_total period labels overlap"));
        }
        out.push(text(
            r,
            ext,
            left + p as f64 * dx,
            lower_top + panel_h + 8.,
            "middle",
        ));
    }
    finish(plan, data, c.width, height, out, vec![])
}

#[cfg(test)]
mod more_tests {
    use super::*;
    fn datum(v: i64) -> Datum {
        Datum {
            value: Some(Decimal::from(v)),
            status: "known".into(),
            certainty: "actual".into(),
            complete: true,
            note: None,
            limit: None,
        }
    }
    #[test]
    fn arrow_bumps_and_line_total_complete() {
        for family in [ChartType::LineTotal] {
            let categories = if family == ChartType::Arrow {
                vec!["Before".into(), "After".into()]
            } else {
                vec!["One".into(), "Two".into(), "Three".into()]
            };
            let values = if family == ChartType::Arrow {
                (vec![datum(50), datum(75)], vec![datum(30), datum(45)])
            } else if family == ChartType::Bumps {
                (
                    vec![datum(1), datum(2), datum(1)],
                    vec![datum(2), datum(1), datum(2)],
                )
            } else {
                (
                    vec![datum(10), datum(20), datum(30)],
                    vec![datum(30), datum(20), datum(10)],
                )
            };
            let data = PreparedData {
                categories,
                x_kind: "time".into(),
                series: vec![
                    Series {
                        id: "a".into(),
                        name: "A".into(),
                        values: values.0,
                    },
                    Series {
                        id: "b".into(),
                        name: "B".into(),
                        values: values.1,
                    },
                ],
                irregular: None,
                comparisons: vec![],
                measure: if family == ChartType::Bumps {
                    json!({"kind":"rank"})
                } else {
                    json!({})
                },
            };
            let plan = SemanticPlan {
                spec: Spec {
                    raw: json!({"title":{"text":"Example"}}),
                },
                chart_type: family,
                orientation: Orientation::Vertical,
                order: (0..data.categories.len()).collect(),
                layers: vec![],
                coordinates: "cartesian".into(),
                colors: Default::default(),
            };
            let scale = TrainedScale {
                lo: Decimal::ZERO,
                hi: Decimal::from(100),
                step: Decimal::from(50),
                ticks: vec![Decimal::ZERO, Decimal::from(50), Decimal::from(100)],
                mode: "linear".into(),
            };
            let t = Theme::default();
            let c = LayoutConstraints::default();
            let LayoutStep::NeedMeasurements { requests, .. } =
                solve(&plan, &data, Some(&scale), &t, &c, &Measurements::default()).unwrap()
            else {
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
                solve(&plan, &data, Some(&scale), &t, &c, &m).unwrap()
            else {
                panic!()
            };
            assert!(
                chart
                    .primitives
                    .iter()
                    .any(|p| matches!(p, Primitive::Path { .. }))
            );
        }
    }
}

pub(super) fn parse_time(text: &str) -> Result<(f64, bool)> {
    if let Ok(d) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Ok((
            d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64,
            true,
        ));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(d) = NaiveDateTime::parse_from_str(text, format) {
            return Ok((d.and_utc().timestamp() as f64, false));
        }
    }
    Err(err(format!("unsupported time instant {text}")))
}
