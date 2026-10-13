//! Scatter plots of two numeric measures (XS-SC-01..05).

use crate::notes::Notes;
use crate::read::*;
use crate::stats::ols;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub series: usize,
    pub intercept: f64,
    pub slope: f64,
    pub r: f64,
    pub n: usize,
}

#[derive(Debug, Clone)]
pub struct ScatterModel {
    pub series: Vec<SeriesMeta>,
    pub points: Vec<Vec<(f64, f64)>>,
    pub x_measure: Measure,
    pub y_measure: Measure,
    pub fits: Vec<Fit>,
    pub total: usize,
    /// Markers are transparent (more than 500 points).
    pub transparent: bool,
}

impl ScatterModel {
    pub fn extents(&self) -> ((f64, f64), (f64, f64)) {
        let (mut xlo, mut xhi, mut ylo, mut yhi) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for (x, y) in self.points.iter().flatten() {
            xlo = xlo.min(*x);
            xhi = xhi.max(*x);
            ylo = ylo.min(*y);
            yhi = yhi.max(*y);
        }
        ((xlo, xhi), (ylo, yhi))
    }
    pub fn note(&self) -> String {
        let mut note = format!("n = {} points.", self.total);
        for f in &self.fits {
            let who = if self.series.len() > 1 {
                format!(" for {}", self.series[f.series].name)
            } else {
                String::new()
            };
            note.push_str(&format!(" Line: least-squares fit{who} (r = {:.2}).", f.r));
        }
        note
    }
}

pub fn parse(spec: &Value, n: &mut Notes) -> Option<ScatterModel> {
    let raw = series(spec, n);
    check_series_count(raw.len(), 6, n);
    if spec.pointer("/data/x_measure").is_none() || spec.pointer("/data/measure").is_none() {
        n.refuse(
            "XS-SC-03",
            "a scatter plot needs two measures, each named with its unit",
            "/data",
            "data.x_measure and data.measure",
        );
    }
    if spec.pointer("/chart/size").is_some() || spec.pointer("/chart/size_by").is_some() {
        n.refuse(
            "XS-SC-01",
            "marker size is not an accurate encoding; it is not offered",
            "/chart/size",
            "remove it; use a second panel for a third measure",
        );
    }
    let fit = spec.pointer("/chart/fit").and_then(Value::as_str);
    if let Some(f) = fit
        && f != "linear"
        && f != "none"
    {
        n.refuse(
            "XS-SC-02",
            format!("fit '{f}' is not offered; a smoother implies a model the chart cannot state"),
            "/chart/fit",
            "fit: linear (ordinary least squares), or none",
        );
    }
    if n.refused() {
        return None;
    }
    let x_measure = Measure::parse(spec.pointer("/data/x_measure").unwrap_or(&Value::Null));
    let y_measure = Measure::parse(spec.pointer("/data/measure").unwrap_or(&Value::Null));
    let mut points: Vec<Vec<(f64, f64)>> = vec![];
    for (si, s) in raw.iter().enumerate() {
        let Some(pts) = s.json.get("xy").and_then(Value::as_array) else {
            n.refuse(
                "R15-2",
                format!("series '{}' needs `xy`: [[x, y], ...]", s.id),
                &format!("/data/series/{si}/xy"),
                "xy",
            );
            return None;
        };
        let mut row = vec![];
        for (i, p) in pts.iter().enumerate() {
            match p
                .as_array()
                .filter(|a| a.len() == 2)
                .and_then(|a| Some((a[0].as_f64()?, a[1].as_f64()?)))
            {
                Some((x, y)) if x.is_finite() && y.is_finite() => row.push((x, y)),
                _ => {
                    n.refuse(
                        "R15-3",
                        format!(
                            "point {i} of series '{}' is not a pair of finite numbers",
                            s.id
                        ),
                        &format!("/data/series/{si}/xy/{i}"),
                        "[x, y] with finite numbers",
                    );
                    return None;
                }
            }
        }
        if row.is_empty() {
            n.refuse(
                "R15-2",
                format!("series '{}' has no points", s.id),
                &format!("/data/series/{si}/xy"),
                "xy",
            );
            return None;
        }
        points.push(row);
    }
    let total: usize = points.iter().map(Vec::len).sum();
    if total > 2000 {
        n.warn("XS-SC-04", format!("{total} points overplot even with transparency; aggregate them (binned counts) instead"), "/data/series");
    } else if total > 500 {
        n.info(
            "XS-SC-04",
            format!("{total} points: markers are made transparent"),
            "/data/series",
        );
    }
    let mut fits = vec![];
    if fit == Some("linear") {
        for (si, row) in points.iter().enumerate() {
            match ols(row) {
                Some((intercept, slope, r)) => fits.push(Fit {
                    series: si,
                    intercept,
                    slope,
                    r,
                    n: row.len(),
                }),
                None => n.warn(
                    "XS-SC-05",
                    format!(
                        "series '{}' has too few or constant points for a fit; none is drawn",
                        raw[si].id
                    ),
                    &format!("/data/series/{si}"),
                ),
            }
        }
    }
    let model = ScatterModel {
        series: metas(&raw),
        points,
        x_measure,
        y_measure,
        fits,
        total,
        transparent: total > 500,
    };
    Some(model)
}
