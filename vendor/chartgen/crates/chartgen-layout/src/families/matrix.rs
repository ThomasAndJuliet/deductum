//! Tables, heatmaps and action dots: dispatch to the ports of the original's
//! `layout/table.py` (families/table.rs) and `layout/heatmap.py` (families/heatmap.rs).
use chartgen_model::*;

fn err(s: impl Into<String>) -> Error {
    Error::new("layout", s)
}
fn validate(data: &PreparedData, theme: &Theme, c: &LayoutConstraints) -> Result<()> {
    if !c.width.is_finite()
        || c.width <= 80.
        || c.height.is_some_and(|h| !h.is_finite() || h <= 60.)
    {
        return Err(err("invalid table size"));
    }
    if data.series.is_empty() || data.categories.is_empty() || theme.palette.is_empty() {
        return Err(err("table needs rows, columns and palette"));
    }
    if data
        .series
        .iter()
        .any(|s| s.values.len() != data.categories.len())
    {
        return Err(err("series length differs from categories"));
    }
    Ok(())
}

/// Requests native measurements, then lays out a matrix family without substituting semantics.
pub fn layout(
    plan: &SemanticPlan,
    data: &PreparedData,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
) -> Result<LayoutStep> {
    if !matches!(
        plan.chart_type,
        ChartType::Table | ChartType::Heatmap | ChartType::ActionDots
    ) {
        return Err(err("not a matrix family"));
    }
    validate(data, theme, c)?;
    crate::kit::guard(plan, data)?;
    let kit = crate::kit::Kit::new(theme, c, m);
    let result = match plan.chart_type {
        ChartType::Table => crate::table::layout_table(&kit, plan, data),
        ChartType::ActionDots => crate::table::layout_action_dots(&kit, plan, data),
        _ => crate::heatmap::layout_heatmap(&kit, plan, data),
    };
    kit.finish(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    fn datum(v: Option<i64>, status: &str) -> Datum {
        Datum {
            value: v.map(Decimal::from),
            status: status.into(),
            certainty: "actual".into(),
            complete: true,
            note: None,
            limit: None,
        }
    }
    fn data() -> PreparedData {
        PreparedData {
            categories: ["A", "B", "C"].map(str::to_owned).into(),
            x_kind: "nominal".into(),
            series: vec![
                Series {
                    id: "one".into(),
                    name: "One".into(),
                    values: vec![
                        datum(Some(3), "known"),
                        datum(None, "missing"),
                        datum(Some(8), "known"),
                    ],
                },
                Series {
                    id: "total".into(),
                    name: "Total".into(),
                    values: vec![
                        datum(Some(6), "known"),
                        datum(Some(4), "known"),
                        datum(Some(9), "known"),
                    ],
                },
            ],
            measure: json!({}),
            irregular: None,
            comparisons: vec![],
        }
    }
    fn plan(kind: ChartType, raw: Value) -> SemanticPlan {
        SemanticPlan {
            spec: Spec { raw },
            chart_type: kind,
            orientation: Orientation::Vertical,
            order: vec![],
            layers: vec![],
            coordinates: "categorical".into(),
            colors: ColorPlan::default(),
        }
    }
    fn measure(p: &SemanticPlan, d: &PreparedData) -> PlacedChart {
        let t = Theme::default();
        let c = LayoutConstraints {
            width: 420.,
            height: None,
            measurement_context: "test".into(),
            max_rounds: 8,
            hooks: None,
        };
        let mut m = Measurements {
            context: "test".into(),
            entries: BTreeMap::new(),
        };
        for _ in 0..8 {
            match layout(p, d, &t, &c, &m).unwrap() {
                LayoutStep::NeedMeasurements { requests, .. } => {
                    for r in requests {
                        let width = (r.content.readable().chars().count() as f64 * 6.).max(5.);
                        m.entries.insert(
                            r.id,
                            TextMetrics {
                                width,
                                height: 10.,
                                baseline: Some(8.),
                            },
                        );
                    }
                }
                LayoutStep::Refused { findings } => panic!("refused: {findings:?}"),
                LayoutStep::Complete { mut chart } => {
                    chart.primitives = crate::kit::flatten(&chart.primitives)
                        .into_iter()
                        .cloned()
                        .collect();
                    return chart;
                }
            }
        }
        panic!("layout did not converge");
    }
    #[test]
    fn table_places_total_last_and_keeps_missing_note() {
        let d = data();
        let p = plan(
            ChartType::Table,
            json!({"data":{"series":[{}, {"role":"total"}]}}),
        );
        let chart = measure(&p, &d);
        assert_eq!(chart.summary["table"]["rows"], json!(["one", "total"]));
        assert!(chart.primitives.iter().any(
            |p| matches!(p,Primitive::Text{request,..} if request.content.readable()=="(missing)")
        ));
    }
    #[test]
    fn heatmap_missing_cell_is_grey_and_keyed_apart() {
        // heatmap.py: a value not known a `missing` cell, never the colour of zero (R6-23), with
        // its own entry after the key's bar when the values are not printed.
        let d = data();
        let p = plan(
            ChartType::Heatmap,
            json!({"data":{"x":{"kind":"nominal"}},"chart":{"value_labels":"off"}}),
        );
        let chart = measure(&p, &d);
        assert_eq!(chart.summary["heatmap"]["labelled"], false);
        let missing = |p: &&Primitive| matches!(p, Primitive::Rect { meta, .. } if meta.classes.iter().any(|c| c == "missing"));
        let cells: Vec<&Primitive> = chart.primitives.iter().filter(missing).collect();
        assert!(
            cells
                .iter()
                .any(|p| matches!(p, Primitive::Rect { meta, .. } if meta.classes[0] == "cell"))
        );
        assert!(cells.iter().any(
            |p| matches!(p, Primitive::Rect { meta, .. } if meta.classes[0] == "legend-swatch")
        ));
    }
    #[test]
    fn censored_heatmap_cell_keeps_limit_label() {
        let mut d = data();
        d.series[0].values[1].status = "below".into();
        d.series[0].values[1].limit = Some(Decimal::from(2));
        let p = plan(
            ChartType::Heatmap,
            json!({"data":{"x":{"kind":"nominal"}},"chart":{"value_labels":"on"}}),
        );
        let chart = measure(&p, &d);
        assert!(chart.primitives.iter().any(
            |p| matches!(p, Primitive::Text { request, .. } if request.content.readable() == "<2")
        ));
    }
    #[test]
    fn action_dots_only_for_supplied_statuses() {
        let mut d = data();
        d.series.truncate(1);
        let p = plan(
            ChartType::ActionDots,
            json!({"data":{"x":{"kind":"nominal"},"actions":{"C":"good_strong"}}}),
        );
        let chart = measure(&p, &d);
        assert_eq!(chart.summary["actions"]["order"], json!(["C", "A", "B"]));
        assert_eq!(
            chart
                .primitives
                .iter()
                .filter(|p| matches!(p,Primitive::Use{meta,..} if meta.classes.iter().any(|c| c=="action-dot")))
                .count(),
            1
        );
    }
}
