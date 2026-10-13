//! The prepared data of a statistical chart.
//!
//! The raw samples, points or counts stay in the spec, where `chartgen-statistics` reads them
//! for policy, scale training and layout. What the generic stages (ordering, data facts,
//! comparisons) see is one ordinary value per category and series: the estimate the chart is
//! about (the median, the interval's estimate, the paired value, a state's index, a count).
//! A chart that cannot be read still gets placeholder data, so that the policy stage can state
//! the refusal with its rule.
use chartgen_model::{Datum, Decimal, PreparedData, Result, Series, Spec};
use chartgen_statistics::notes::Notes;
use serde_json::Value;

fn known(v: f64) -> Datum {
    Datum {
        value: Decimal::from_f64_retain(v).map(|d| d.round_dp(9).normalize()),
        status: "known".into(),
        certainty: "actual".into(),
        complete: true,
        note: None,
        limit: None,
    }
}

pub fn parse(spec: &Spec) -> Result<PreparedData> {
    let chart_type = crate::schema::statistical_type(spec).unwrap_or_default();
    let mut notes = Notes::default();
    let model = chartgen_statistics::parse(&spec.raw, &chart_type, &mut notes);
    let categories: Vec<String> = match spec
        .get("/data/x/values")
        .as_array()
        .filter(|a| !a.is_empty())
    {
        Some(a) => a
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect(),
        // a scatter plot has no categories: one placeholder holds its count of points
        None => vec!["points".into()],
    };
    let raw_series = spec
        .get("/data/series")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let estimates = model.as_ref().and_then(chartgen_statistics::estimates);
    let mut series = vec![];
    for (i, s) in raw_series.iter().enumerate() {
        let id = s
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let name = s
            .get("name")
            .and_then(|v| {
                v.as_str()
                    .or_else(|| v.get("typst").and_then(Value::as_str))
            })
            .unwrap_or(&id)
            .to_string();
        let values: Vec<Datum> = match (&model, &estimates) {
            (_, Some(est)) if est.get(i).is_some_and(|r| r.len() == categories.len()) => {
                est[i].iter().map(|v| known(*v)).collect()
            }
            (Some(chartgen_statistics::Model::Scatter(m)), _) => {
                vec![known(m.points[i].len() as f64)]
            }
            _ => vec![known(0.0); categories.len()],
        };
        series.push(Series { id, name, values });
    }
    Ok(PreparedData {
        comparisons: crate::parse_comparisons(spec, categories.len())?,
        categories,
        x_kind: spec.string("/data/x/kind", "nominal"),
        series,
        measure: spec.get("/data/measure").clone(),
        irregular: None,
    })
}
