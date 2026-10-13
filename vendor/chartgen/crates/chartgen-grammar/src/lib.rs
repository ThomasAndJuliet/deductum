//! Compile an accepted chart choice into explicit semantic mark layers.
use chartgen_model::{
    ChartType, ColorPlan, Decision, Error, Layer, LayerSource, PreparedData, Result, SemanticPlan,
    Spec,
};
pub fn compile(spec: &Spec, decision: &Decision, data: &PreparedData) -> Result<SemanticPlan> {
    let kind = decision
        .chart_type
        .ok_or_else(|| Error::new("grammar", "cannot compile a refused chart"))?;
    if decision.order.len() != data.categories.len()
        || decision.order.iter().any(|&i| i >= data.categories.len())
        || {
            let mut x = decision.order.clone();
            x.sort();
            x.dedup();
            x.len() != data.categories.len()
        }
    {
        return Err(Error::new("grammar", "decision order is not a permutation"));
    }
    if data.series.is_empty() {
        return Err(Error::new("grammar", "chart has no series"));
    }
    let coordinates = match kind {
        ChartType::Pie | ChartType::Donut => "polar",
        ChartType::Treemap => "hierarchical",
        ChartType::Table | ChartType::Heatmap | ChartType::StateMatrix | ChartType::Contingency => "matrix",
        _ => "cartesian",
    }
    .to_string();
    let (x, y) = match kind {
        ChartType::Table | ChartType::Heatmap | ChartType::StateMatrix | ChartType::Contingency => ("category", "series"),
        ChartType::Scatter => ("x", "y"),
        ChartType::Pie | ChartType::Donut | ChartType::Treemap => ("part", "value"),
        ChartType::Bumps => ("time", "rank"),
        ChartType::Arrow => ("period", "value"),
        _ => ("category", "value"),
    };
    let mut layers = vec![Layer {
        mark: kind,
        source: Some(LayerSource::Series {
            indices: (0..data.series.len()).collect(),
        }),
        series: (0..data.series.len()).collect(),
        x_channel: x.into(),
        y_channel: y.into(),
    }];
    for (index, comparison) in data.comparisons.iter().enumerate() {
        if comparison.render == "none" {
            continue;
        }
        if comparison.render == "auto" {
            return Err(Error::new(
                "grammar",
                "comparison render must be resolved before compilation",
            ));
        }
        layers.push(Layer {
            mark: if comparison.render == "line" {
                ChartType::Line
            } else {
                kind
            },
            source: Some(LayerSource::Comparison {
                index,
                render: comparison.render.clone(),
            }),
            series: vec![],
            x_channel: x.into(),
            y_channel: y.into(),
        });
    }
    Ok(SemanticPlan {
        spec: spec.clone(),
        chart_type: kind,
        orientation: decision.orientation,
        order: decision.order.clone(),
        layers,
        coordinates,
        colors: ColorPlan::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chartgen_model::{Datum, Decimal, Orientation, PreparedComparison, Series};
    use serde_json::json;
    fn prepared() -> PreparedData {
        PreparedData {
            categories: vec!["A".into()],
            x_kind: "nominal".into(),
            series: vec![Series {
                id: "s".into(),
                name: "S".into(),
                values: vec![Datum {
                    value: Some(Decimal::ONE),
                    status: "known".into(),
                    certainty: "actual".into(),
                    complete: true,
                    note: None,
                    limit: None,
                }],
            }],
            measure: json!({}),
            irregular: None,
            comparisons: vec![],
        }
    }
    fn decision() -> Decision {
        Decision {
            chart_type: Some(ChartType::Bar),
            orientation: Orientation::Vertical,
            order: vec![0],
            findings: vec![],
            decisions: vec![],
            status: "drawn".into(),
        }
    }
    #[test]
    fn consumed_deviation_comparator_is_not_drawn() {
        let s = Spec {
            raw: json!({"transform":{"kind":"deviation","comparison":"target"},"data":{"comparisons":[{"id":"target"}]}}),
        };
        assert!(compile(&s, &decision(), &prepared()).is_ok());
    }
    #[test]
    fn active_comparator_requires_semantic_layer() {
        let s = Spec {
            raw: json!({"data":{"comparisons":[{"id":"target"}]}}),
        };
        let mut data = prepared();
        data.comparisons.push(PreparedComparison {
            id: "target".into(),
            kind: "target".into(),
            label: "Target".into(),
            values: data.series[0].values.clone(),
            render: "line".into(),
            valid: true,
            series: None,
        });
        let plan = compile(&s, &decision(), &data).unwrap();
        assert_eq!(plan.layers.len(), 2);
        assert!(matches!(
            plan.layers[1].source,
            Some(LayerSource::Comparison { index: 0, .. })
        ));
    }
}
