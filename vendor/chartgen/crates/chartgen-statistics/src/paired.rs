//! The paired contrast: two conditions on the same categories, as a dumbbell or as the
//! difference from zero (XS-DB-01..04).

use crate::interval::{Decl, parse_decl};
use crate::notes::Notes;
use crate::read::*;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Pair,
    Difference,
}

#[derive(Debug, Clone)]
pub struct PairedModel {
    pub labels: Vec<String>,
    pub series: Vec<SeriesMeta>,
    /// `[series][category]`, in the input order.
    pub values: Vec<Vec<f64>>,
    pub diffs: Vec<f64>,
    pub view: View,
    pub good: Option<Good>,
    pub diff_name: String,
    pub intervals: Option<(Vec<(f64, f64)>, Decl)>,
    pub measure: Measure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Good {
    Up,
    Down,
}

impl PairedModel {
    pub fn extent(&self) -> (f64, f64) {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        match self.view {
            View::Pair => {
                for v in self.values.iter().flatten() {
                    lo = lo.min(*v);
                    hi = hi.max(*v);
                }
            }
            View::Difference => {
                lo = 0.0;
                hi = 0.0;
                for (i, d) in self.diffs.iter().enumerate() {
                    lo = lo
                        .min(*d)
                        .min(self.intervals.as_ref().map_or(*d, |v| v.0[i].0));
                    hi = hi
                        .max(*d)
                        .max(self.intervals.as_ref().map_or(*d, |v| v.0[i].1));
                }
            }
        }
        (lo, hi)
    }
    pub fn note(&self) -> String {
        match self.view {
            View::Pair => {
                let mut note = format!(
                    "Each row joins the two conditions measured on the same category. Label at right: {}.",
                    self.diff_name
                );
                if self.good.is_some() {
                    note.push_str(" Colour shows the direction that is better.");
                }
                note
            }
            View::Difference => {
                let mut note = format!(
                    "Each mark is the difference {} for one category; the line at zero is no difference.",
                    self.diff_name
                );
                if let Some(good) = self.good {
                    note.push_str(&format!(
                        " Colour shows the direction that is better ({}).",
                        if good == Good::Up { "higher" } else { "lower" }
                    ));
                }
                if let Some((_, d)) = &self.intervals {
                    note.push(' ');
                    note.push_str(&d.note());
                }
                note
            }
        }
    }
    /// The first series' values: the estimate each category is sorted and scaled by.
    pub fn estimates(&self) -> Vec<Vec<f64>> {
        match self.view {
            View::Pair => self.values.clone(),
            View::Difference => vec![self.diffs.clone()],
        }
    }
}

pub fn parse(spec: &Value, n: &mut Notes) -> Option<PairedModel> {
    let labels = labels(spec, n);
    let raw = series(spec, n);
    let measure = Measure::parse(spec.pointer("/data/measure").unwrap_or(&Value::Null));
    if !n.refused() && raw.len() != 2 {
        n.refuse(
            "XS-DB-01",
            format!(
                "a paired contrast compares exactly two conditions, not {}",
                raw.len()
            ),
            "/data/series",
            "exactly two series; use interval_dot or box for more",
        );
    }
    let view = match spec
        .pointer("/chart/view")
        .and_then(Value::as_str)
        .unwrap_or("pair")
    {
        "pair" => View::Pair,
        "difference" => View::Difference,
        v => {
            n.refuse(
                "XS-DB-04",
                format!("unknown view '{v}'"),
                "/chart/view",
                "pair or difference",
            );
            View::Pair
        }
    };
    // the direction that is better is the measure's (R3-17): colour only says good or bad when
    // it is declared
    let good = match spec.pointer("/data/measure/good").and_then(Value::as_str) {
        Some("higher") => Some(Good::Up),
        Some("lower") => Some(Good::Down),
        _ => None,
    };
    if n.refused() {
        return None;
    }
    let mut values: Vec<Vec<f64>> = vec![];
    for (si, s) in raw.iter().enumerate() {
        let Some(v) = s
            .json
            .get("values")
            .and_then(|v| numbers(v, &format!("/data/series/{si}/values"), n))
        else {
            if !n.refused() {
                n.refuse(
                    "R15-2",
                    format!("series '{}' needs `values`", s.id),
                    &format!("/data/series/{si}/values"),
                    "one value per category",
                );
            }
            return None;
        };
        if v.len() != labels.len() {
            n.refuse(
                "XS-DB-02",
                format!(
                    "series '{}' has {} values for {} categories: the conditions are not paired",
                    s.id,
                    v.len(),
                    labels.len()
                ),
                &format!("/data/series/{si}/values"),
                "a value for every category in both conditions",
            );
            return None;
        }
        values.push(v);
    }
    let first_minus_second =
        spec.pointer("/chart/difference").and_then(Value::as_str) == Some("first_minus_second");
    let (a, b) = if first_minus_second { (1, 0) } else { (0, 1) };
    let diffs: Vec<f64> = (0..labels.len())
        .map(|i| values[b][i] - values[a][i])
        .collect();
    let diff_name = format!("{} \u{2212} {}", raw[b].name, raw[a].name);
    let mut intervals = None;
    if view == View::Difference
        && let Some(arr) = spec
            .pointer("/data/difference_intervals")
            .and_then(Value::as_array)
    {
        if arr.len() != labels.len() {
            n.refuse(
                "XS-DB-03",
                "difference_intervals needs one interval per category",
                "/data/difference_intervals",
                "matching length",
            );
            return None;
        }
        let decl = parse_decl(spec, n)?;
        let mut out = vec![];
        for (i, iv) in arr.iter().enumerate() {
            match (
                iv.get("lo").and_then(Value::as_f64),
                iv.get("hi").and_then(Value::as_f64),
            ) {
                (Some(lo), Some(hi)) if lo <= diffs[i] + 1e-12 && diffs[i] <= hi + 1e-12 => {
                    out.push((lo, hi))
                }
                _ => {
                    n.refuse(
                        "XS-DB-03",
                        format!(
                            "the interval for '{}' is missing or excludes its own difference",
                            labels[i]
                        ),
                        &format!("/data/difference_intervals/{i}"),
                        "lo <= difference <= hi",
                    );
                    return None;
                }
            }
        }
        intervals = Some((out, decl));
    }
    Some(PairedModel {
        labels,
        series: metas(&raw),
        values,
        diffs,
        view,
        good,
        diff_name,
        intervals,
        measure,
    })
}
