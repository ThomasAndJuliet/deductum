//! Reading the parts of a chartspec the statistical charts share.

use crate::notes::Notes;
use serde_json::Value;

/// The facts about the value axis the statistics need (the axis itself is trained by
/// `chartgen-scales` from the measure, as for every chart).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measure {
    pub name: String,
    /// Zero means "none of it" (the measure's default, R2-24).
    pub zero_meaningful: bool,
    /// Values are percentages, in percent units (45 for 45 %).
    pub percent: bool,
}

impl Measure {
    pub fn parse(v: &Value) -> Self {
        Self {
            name: v
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            zero_meaningful: v.get("zero").and_then(Value::as_str) != Some("arbitrary"),
            percent: v.get("kind").and_then(Value::as_str) == Some("percent"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesMeta {
    pub id: String,
    pub name: String,
}

pub struct Raw<'a> {
    pub id: String,
    pub name: String,
    pub json: &'a Value,
}

pub fn labels(spec: &Value, n: &mut Notes) -> Vec<String> {
    match spec.pointer("/data/x/values").and_then(Value::as_array) {
        Some(a) if !a.is_empty() => a
            .iter()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        _ => {
            n.refuse(
                "R15-2",
                "the chart needs categories in data.x.values",
                "/data/x/values",
                "data.x.values",
            );
            vec![]
        }
    }
}

pub fn series<'a>(spec: &'a Value, n: &mut Notes) -> Vec<Raw<'a>> {
    let Some(a) = spec
        .pointer("/data/series")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    else {
        n.refuse(
            "R15-2",
            "the chart needs data.series",
            "/data/series",
            "data.series",
        );
        return vec![];
    };
    a.iter()
        .enumerate()
        .map(|(i, s)| {
            let id = s
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or(format!("s{i}"));
            let name = s
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or(id.clone());
            Raw { id, name, json: s }
        })
        .collect()
}

pub fn metas(series: &[Raw]) -> Vec<SeriesMeta> {
    series
        .iter()
        .map(|s| SeriesMeta {
            id: s.id.clone(),
            name: s.name.clone(),
        })
        .collect()
}

/// An array of finite numbers; `None` (with a refusal) otherwise.
pub fn numbers(v: &Value, path: &str, n: &mut Notes) -> Option<Vec<f64>> {
    let Some(a) = v.as_array() else {
        n.refuse(
            "R15-3",
            "expected an array of numbers",
            path,
            "an array of finite numbers",
        );
        return None;
    };
    let mut out = Vec::with_capacity(a.len());
    for (i, x) in a.iter().enumerate() {
        match x.as_f64().filter(|f| f.is_finite()) {
            Some(f) => out.push(f),
            None => {
                n.refuse(
                    "R15-3",
                    format!("value {i} is not a finite number; missing values are never imputed"),
                    path,
                    "finite numbers only; remove or declare missing values",
                );
                return None;
            }
        }
    }
    Some(out)
}

/// More series than colour can tell apart (G4.28's spirit).
pub fn check_series_count(count: usize, max: usize, n: &mut Notes) {
    if count > max {
        n.refuse(
            "XS-GEN-01",
            format!("{count} series exceed the {max} this chart can distinguish"),
            "/data/series",
            "fewer series, or small multiples (panels)",
        );
    }
}

pub fn chart_type(spec: &Value) -> &str {
    spec.pointer("/chart/type")
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// Samples `[series][group]` for the distribution charts.
pub fn samples(
    spec: &Value,
    labels: &[String],
    series: &[Raw],
    empty_rule: &str,
    n: &mut Notes,
) -> Vec<Vec<Vec<f64>>> {
    let mut out = vec![];
    for (si, s) in series.iter().enumerate() {
        let Some(sam) = s.json.get("samples").and_then(Value::as_array) else {
            n.refuse(
                "R15-2",
                format!("series '{}' needs `samples`", s.id),
                &format!("/data/series/{si}"),
                "samples: one array per category",
            );
            return vec![];
        };
        if sam.len() != labels.len() {
            n.refuse(
                "R15-2",
                format!(
                    "series '{}' has {} sample groups for {} categories",
                    s.id,
                    sam.len(),
                    labels.len()
                ),
                &format!("/data/series/{si}/samples"),
                "one sample array per category",
            );
            return vec![];
        }
        let mut row = vec![];
        for (gi, g) in sam.iter().enumerate() {
            let path = format!("/data/series/{si}/samples/{gi}");
            let Some(v) = numbers(g, &path, n) else {
                return vec![];
            };
            if v.is_empty() {
                n.refuse(
                    empty_rule,
                    format!("'{}' in series '{}' has no samples", labels[gi], s.id),
                    &path,
                    "samples, or drop the category",
                );
                return vec![];
            }
            row.push(v);
        }
        out.push(row);
    }
    let _ = spec;
    out
}
