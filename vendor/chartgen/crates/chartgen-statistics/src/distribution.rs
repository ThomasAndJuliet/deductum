//! Strip charts and empirical CDFs (XS-STRIP-*, XS-ECDF-*).

use crate::notes::Notes;
use crate::read::*;
use crate::stats::*;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct StripModel {
    pub labels: Vec<String>,
    pub series: Vec<SeriesMeta>,
    /// `[series][group]`
    pub samples: Vec<Vec<Vec<f64>>>,
    pub measure: Measure,
}

impl StripModel {
    pub fn extent(&self) -> (f64, f64) {
        extent_of(&self.samples)
    }
    pub fn note(&self) -> String {
        "Each point is one measurement; its position across the row is spread for visibility and has no meaning. Line: median. n at right.".into()
    }
    pub fn estimates(&self) -> Vec<Vec<f64>> {
        self.samples
            .iter()
            .map(|row| row.iter().map(|v| median(v)).collect())
            .collect()
    }
}

fn extent_of(samples: &[Vec<Vec<f64>>]) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in samples.iter().flatten().flatten() {
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    (lo, hi)
}

pub fn parse_strip(spec: &Value, n: &mut Notes) -> Option<StripModel> {
    let labels = labels(spec, n);
    let raw = series(spec, n);
    check_series_count(raw.len(), 6, n);
    let measure = Measure::parse(spec.pointer("/data/measure").unwrap_or(&Value::Null));
    if n.refused() {
        return None;
    }
    let samples = samples(spec, &labels, &raw, "XS-STRIP-02", n);
    if n.refused() {
        return None;
    }
    for (si, row) in samples.iter().enumerate() {
        for (gi, v) in row.iter().enumerate() {
            if v.len() > 300 {
                n.warn(
                    "XS-STRIP-01",
                    format!(
                        "'{}' in series '{}' has {} points: they overplot. Use type: ecdf",
                        labels[gi],
                        raw[si].id,
                        v.len()
                    ),
                    &format!("/data/series/{si}/samples/{gi}"),
                );
            }
        }
    }
    Some(StripModel {
        labels,
        series: metas(&raw),
        samples,
        measure,
    })
}

#[derive(Debug, Clone)]
pub struct Curve {
    pub name: String,
    pub series: usize,
    pub group: usize,
    pub n: usize,
    /// Step points `(x, F(x))`.
    pub steps: Vec<(f64, f64)>,
    /// The requested reference quantiles, as `(p, value)`.
    pub quantiles: Vec<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub struct EcdfModel {
    pub curves: Vec<Curve>,
    pub reference: Vec<f64>,
    pub measure: Measure,
}

impl EcdfModel {
    pub fn extent(&self) -> (f64, f64) {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in &self.curves {
            lo = lo.min(c.steps[0].0);
            hi = hi.max(c.steps[c.steps.len() - 1].0);
        }
        (lo, hi)
    }
    pub fn note(&self) -> String {
        let mut note = "Each curve shows the share of measurements at or below a value; steps are single measurements.".to_string();
        if !self.reference.is_empty() {
            note.push_str(" Dotted lines mark the requested quantiles.");
        }
        note
    }
    pub fn estimates(&self) -> Vec<Vec<f64>> {
        // one row per series; the curve's median
        let ns = self
            .curves
            .iter()
            .map(|c| c.series)
            .max()
            .map_or(0, |m| m + 1);
        let ng = self
            .curves
            .iter()
            .map(|c| c.group)
            .max()
            .map_or(0, |m| m + 1);
        let mut out = vec![vec![0.0; ng]; ns];
        for c in &self.curves {
            let at = c
                .steps
                .iter()
                .find(|(_, f)| *f >= 0.5)
                .map_or(c.steps[0].0, |p| p.0);
            out[c.series][c.group] = at;
        }
        out
    }
}

pub fn parse_ecdf(spec: &Value, n: &mut Notes) -> Option<EcdfModel> {
    let labels = labels(spec, n);
    let raw = series(spec, n);
    let measure = Measure::parse(spec.pointer("/data/measure").unwrap_or(&Value::Null));
    if n.refused() {
        return None;
    }
    let samples = samples(spec, &labels, &raw, "XS-STRIP-02", n);
    if n.refused() {
        return None;
    }
    let count = labels.len() * raw.len();
    if count > 8 {
        n.refuse(
            "XS-ECDF-01",
            format!("{count} curves cannot be told apart"),
            "/data",
            "at most 8 category-by-series curves, or small multiples",
        );
        return None;
    }
    let reference: Vec<f64> = spec
        .pointer("/chart/reference_quantiles")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_f64)
                .filter(|p| *p > 0.0 && *p < 1.0)
                .collect()
        })
        .unwrap_or_default();
    let mut curves = vec![];
    for (si, s) in raw.iter().enumerate() {
        for (gi, l) in labels.iter().enumerate() {
            let name = match (labels.len() > 1, raw.len() > 1) {
                (true, true) => format!("{l} \u{b7} {}", s.name),
                (true, false) => l.clone(),
                _ => s.name.clone(),
            };
            let v = &samples[si][gi];
            curves.push(Curve {
                name,
                series: si,
                group: gi,
                n: v.len(),
                steps: ecdf(v),
                quantiles: reference
                    .iter()
                    .map(|p| (*p, quantile(v, *p, QuantileMethod::Linear)))
                    .collect(),
            });
        }
    }
    Some(EcdfModel {
        curves,
        reference,
        measure,
    })
}
