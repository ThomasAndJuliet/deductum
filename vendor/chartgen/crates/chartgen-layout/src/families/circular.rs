//! One total's breakdown: pies, donuts (only through an override) and treemaps, laid out by the
//! ports of the original's `layout/pie.py` and `layout/treemap.py` (families/pie.rs, treemap.rs).
use chartgen_model::*;

/// Places one circular family, asking the host for every text it sets first.
pub fn layout(
    plan: &SemanticPlan,
    data: &PreparedData,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
) -> Result<LayoutStep> {
    if data.series.is_empty() || data.categories.is_empty() {
        return Err(Error::new("layout", "a breakdown needs parts and a series"));
    }
    if data
        .series
        .iter()
        .any(|s| s.values.len() != data.categories.len())
    {
        return Err(Error::new(
            "layout",
            "series length differs from categories",
        ));
    }
    crate::kit::guard(plan, data)?;
    let kit = crate::kit::Kit::new(theme, c, m);
    let result = match plan.chart_type {
        ChartType::Pie | ChartType::Donut => crate::pie::layout_pie(&kit, plan, data),
        ChartType::Treemap => crate::treemap::layout_treemap(&kit, plan, data),
        _ => return Err(Error::new("layout", "not a circular family")),
    };
    kit.finish(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    fn data(vals: &[Option<i64>]) -> PreparedData {
        PreparedData {
            categories: ["Small", "Large", "Other"].map(str::to_owned).into(),
            x_kind: "nominal".into(),
            series: vec![Series {
                id: "s".into(),
                name: "S".into(),
                values: vals
                    .iter()
                    .map(|v| Datum {
                        value: v.map(Decimal::from),
                        status: if v.is_some() { "known" } else { "missing" }.into(),
                        certainty: "actual".into(),
                        complete: true,
                        note: None,
                        limit: None,
                    })
                    .collect(),
            }],
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
            width: 368.,
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
                        let width = (r.content.readable().chars().count() as f64 * 5.).max(5.);
                        m.entries.insert(
                            r.id,
                            TextMetrics {
                                width,
                                height: 9.,
                                baseline: Some(7.),
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
        panic!("layout did not converge")
    }
    fn raw(x: Value) -> Value {
        json!({"data": {"x": x, "series": [{"id": "s", "values": []}], "measure": {"kind": "count"}}})
    }
    #[test]
    fn pie_orders_largest_first_all_others_last_and_zero_has_no_slice() {
        // pie.py order(): largest first, "All others" last whatever its size (R7-17); a part of
        // zero value keeps its label but has no slice (R7-36).
        let d = data(&[Some(10), Some(30), Some(0)]);
        let p = plan(
            ChartType::Pie,
            raw(
                json!({"kind": "nominal", "values": ["Small", "Large", "Other"], "roles": {"Other": "all_others"}}),
            ),
        );
        let chart = measure(&p, &d);
        assert_eq!(chart.summary["order"], json!(["Large", "Small", "Other"]));
        let slices = chart
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Shape { meta, .. } if meta.classes.first().is_some_and(|c| c == "slice")))
            .count();
        assert_eq!(slices, 2);
        assert_eq!(chart.summary["leaders"], json!(["Other"]));
    }
    #[test]
    fn unknown_part_refuses_the_fractions() {
        // R0-12: a part not known leaves the total and every fraction unknown.
        let d = data(&[Some(10), None, Some(5)]);
        let p = plan(
            ChartType::Pie,
            raw(json!({"kind": "nominal", "values": ["Small", "Large", "Other"]})),
        );
        let chart = measure(&p, &d);
        assert!(chart.primitives.is_empty());
        assert!(
            chart
                .findings
                .iter()
                .any(|f| f.rule == "R0-12" && f.severity == "error")
        );
    }
    #[test]
    fn treemap_areas_follow_values_and_groups_take_slots() {
        // treemap.py: squarified tiles whose areas are proportional to the values; each group a
        // palette slot in the order of the groups.
        let d = data(&[Some(10), Some(30), Some(5)]);
        let p = plan(
            ChartType::Treemap,
            raw(
                json!({"kind": "nominal", "values": ["Small", "Large", "Other"], "groups": {"Small": "A", "Large": "A", "Other": "B"}}),
            ),
        );
        let chart = measure(&p, &d);
        let tiles: Vec<(String, f64)> = chart
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Rect { bounds, meta, .. }
                    if meta.classes.first().is_some_and(|c| c == "tile") =>
                {
                    Some((meta.classes[1].clone(), bounds.width * bounds.height))
                }
                _ => None,
            })
            .collect();
        assert_eq!(tiles.len(), 3);
        assert!(tiles.iter().all(|(_, a)| *a > 0.));
        assert_eq!(chart.summary["groups"], json!(["A", "B"]));
        assert_eq!(tiles.iter().filter(|(c, _)| c == "series-1").count(), 2);
        assert_eq!(tiles.iter().filter(|(c, _)| c == "series-2").count(), 1);
    }
    #[test]
    fn squarify_matches_the_original_rows() {
        // treemap.squarify on the original's example of Bruls et al.: 6,6,4,3,2,2,1 in 6 x 4.
        let r = crate::treemap::squarify(&[6., 6., 4., 3., 2., 2., 1.], 0., 0., 6., 4.);
        // expected: the original's squarify, run on the same input
        let expected = [
            (0., 0., 3., 2.),
            (0., 2., 3., 2.),
            (3., 0., 1.7142857142857142, 2.3333333333333335),
            (
                4.714285714285714,
                0.,
                1.2857142857142856,
                2.3333333333333335,
            ),
            (
                3.,
                2.3333333333333335,
                1.2000000000000002,
                1.6666666666666665,
            ),
            (
                4.2,
                2.3333333333333335,
                1.2000000000000002,
                1.6666666666666665,
            ),
            (
                5.4,
                2.3333333333333335,
                0.5999999999999996,
                1.6666666666666676,
            ),
        ];
        assert_eq!(r.len(), expected.len());
        for (a, b) in r.iter().zip(expected) {
            assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
            assert!((a.2 - b.2).abs() < 1e-9 && (a.3 - b.3).abs() < 1e-9);
        }
    }
}
