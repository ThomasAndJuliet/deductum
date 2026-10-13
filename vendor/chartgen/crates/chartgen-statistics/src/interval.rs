//! Intervals around point estimates: confidence intervals, bounds, ranges and quantile bands.
//! An interval is never described without saying what it is (XS-IV-01..06).

use crate::notes::Notes;
use crate::read::*;
use crate::stats::*;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct Decl {
    pub kind: String,
    pub level: Option<f64>,
    pub method: Option<String>,
    pub statistic: Statistic,
    pub resamples: usize,
    pub seed: u64,
    pub basis: Option<String>,
    pub quantiles: Option<(f64, f64)>,
}

fn pct(l: f64) -> String {
    let s = format!("{:.2}", l * 100.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl Decl {
    /// The sentence the chart's note carries, stating what the whiskers are.
    pub fn note(&self) -> String {
        let level = self
            .level
            .map(|l| format!("{}%", pct(l)))
            .unwrap_or_default();
        match self.kind.as_str() {
            "confidence" => {
                let how = match self.method.as_deref().unwrap_or("") {
                    "bootstrap" => format!(
                        "percentile bootstrap, {} resamples, seed {}",
                        self.resamples, self.seed
                    ),
                    "wilson" => "Wilson score".to_string(),
                    "normal" => "normal approximation".to_string(),
                    m => m.to_string(),
                };
                format!("Whiskers: {level} confidence interval ({how}).")
            }
            "bound" => format!(
                "Whiskers: lower and upper bounds, not confidence intervals. {}",
                self.basis.as_deref().unwrap_or("")
            ),
            "range" => format!(
                "Whiskers: observed range. {}",
                self.basis.as_deref().unwrap_or("")
            )
            .trim()
            .to_string(),
            _ => {
                let (a, b) = self.quantiles.unwrap_or((0.05, 0.95));
                format!(
                    "Whiskers: {}th to {}th percentile of the measurements.",
                    pct(a),
                    pct(b)
                )
            }
        }
    }
}

pub fn parse_decl(spec: &Value, n: &mut Notes) -> Option<Decl> {
    let Some(d) = spec.pointer("/chart/interval") else {
        n.refuse(
            "XS-IV-01",
            "an interval must say what it is",
            "/chart/interval",
            "chart.interval.kind: confidence, bound, range or quantile_band",
        );
        return None;
    };
    let kind = d.get("kind").and_then(Value::as_str).unwrap_or("");
    if !["confidence", "bound", "range", "quantile_band"].contains(&kind) {
        n.refuse(
            "XS-IV-01",
            format!("interval kind '{kind}' is not one of confidence, bound, range, quantile_band"),
            "/chart/interval/kind",
            "a declared kind",
        );
        return None;
    }
    let level = d.get("level").and_then(Value::as_f64);
    let method = d.get("method").and_then(Value::as_str).map(str::to_string);
    let basis = d.get("basis").and_then(Value::as_str).map(str::to_string);
    if kind == "confidence" && (!level.is_some_and(|l| l > 0.0 && l < 1.0) || method.is_none()) {
        n.refuse(
            "XS-IV-02",
            "a confidence interval states its level (0..1) and method",
            "/chart/interval",
            "level and method (bootstrap, normal, wilson, or the name of the supplied method)",
        );
        return None;
    }
    if (kind == "bound" || kind == "range") && basis.is_none() {
        n.refuse("XS-IV-03", format!("a {kind} states what it bounds: it is not a confidence interval and must not read as one"), "/chart/interval/basis", "basis: one sentence on what the whiskers bound");
        return None;
    }
    let quantiles = d
        .get("quantiles")
        .and_then(Value::as_array)
        .and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
    if kind == "quantile_band" && !quantiles.is_some_and(|(a, b)| 0.0 <= a && a < b && b <= 1.0) {
        n.refuse(
            "XS-IV-03",
            "a quantile band names its quantiles",
            "/chart/interval/quantiles",
            "quantiles: [0.05, 0.95]",
        );
        return None;
    }
    let statistic = match d
        .get("statistic")
        .and_then(Value::as_str)
        .map(Statistic::parse)
    {
        None => Statistic::Mean,
        Some(Some(s)) => s,
        Some(None) => {
            n.refuse(
                "XS-IV-01",
                "statistic must be mean or median",
                "/chart/interval/statistic",
                "mean or median",
            );
            return None;
        }
    };
    let seed = d.get("seed").and_then(Value::as_u64).unwrap_or(1);
    let resamples = d.get("resamples").and_then(Value::as_u64).unwrap_or(2000) as usize;
    if kind == "confidence" && method.as_deref() == Some("bootstrap") {
        n.decide(
            "bootstrap",
            json!({"resamples": resamples, "seed": seed, "statistic": statistic.name()}),
            "recorded so the figure reproduces",
        );
    }
    Some(Decl {
        kind: kind.into(),
        level,
        method,
        statistic,
        resamples,
        seed,
        basis,
        quantiles,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    pub value: f64,
    pub lo: f64,
    pub hi: f64,
    /// Printed in the right-hand gutter: n, or successes/trials.
    pub size: String,
}

/// Estimates `[series][group]`, from whichever input each series gives.
pub fn estimates(
    labels: &[String],
    series: &[Raw],
    decl: &Decl,
    percent: bool,
    n: &mut Notes,
) -> Vec<Vec<Estimate>> {
    let mut out = vec![];
    for (si, s) in series.iter().enumerate() {
        let base = format!("/data/series/{si}");
        let mut row = vec![];
        if let Some(sam) = s.json.get("samples").and_then(Value::as_array) {
            if sam.len() != labels.len() {
                n.refuse(
                    "R15-2",
                    format!(
                        "series '{}' has {} sample groups for {} categories",
                        s.id,
                        sam.len(),
                        labels.len()
                    ),
                    &format!("{base}/samples"),
                    "one sample array per category",
                );
                return vec![];
            }
            for (gi, g) in sam.iter().enumerate() {
                let Some(v) = numbers(g, &format!("{base}/samples/{gi}"), n) else {
                    return vec![];
                };
                if v.len() < 2 {
                    n.refuse(
                        "XS-IV-06",
                        format!(
                            "'{}' in series '{}' has n={}: no interval can be computed",
                            labels[gi],
                            s.id,
                            v.len()
                        ),
                        &format!("{base}/samples/{gi}"),
                        "at least two samples",
                    );
                    return vec![];
                }
                let value = decl.statistic.of(&v);
                let (lo, hi) = match decl.kind.as_str() {
                    "confidence" => match decl.method.as_deref() {
                        Some("bootstrap") => {
                            if v.len() < 10 {
                                n.warn("XS-IV-05", format!("bootstrap interval for '{}' rests on n={}: it is unreliable", labels[gi], v.len()), &format!("{base}/samples/{gi}"));
                            }
                            bootstrap_ci(
                                &v,
                                decl.statistic,
                                decl.level.unwrap(),
                                decl.resamples,
                                decl.seed ^ (si * 1009 + gi) as u64,
                            )
                            .unwrap()
                        }
                        Some("normal") => {
                            if decl.statistic != Statistic::Mean {
                                n.refuse(
                                    "XS-IV-06",
                                    "the normal-approximation interval is for the mean",
                                    "/chart/interval",
                                    "method: bootstrap for a median",
                                );
                                return vec![];
                            }
                            if v.len() < 30 {
                                n.warn("XS-IV-05", format!("normal-approximation interval for '{}' rests on n={}: it is optimistic below about 30", labels[gi], v.len()), &format!("{base}/samples/{gi}"));
                            }
                            normal_ci(&v, decl.level.unwrap()).unwrap()
                        }
                        m => {
                            n.refuse(
                                "XS-IV-06",
                                format!("method {m:?} cannot be computed from samples"),
                                "/chart/interval/method",
                                "bootstrap or normal, or supply the intervals",
                            );
                            return vec![];
                        }
                    },
                    "range" => (min(&v), max(&v)),
                    _ => {
                        let (a, b) = decl.quantiles.unwrap();
                        (
                            quantile(&v, a, QuantileMethod::Linear),
                            quantile(&v, b, QuantileMethod::Linear),
                        )
                    }
                };
                row.push(Estimate {
                    value,
                    lo,
                    hi,
                    size: format!("n={}", v.len()),
                });
            }
        } else if let (Some(k), Some(t)) = (
            s.json.get("successes").and_then(Value::as_array),
            s.json.get("trials").and_then(Value::as_array),
        ) {
            if decl.kind != "confidence" || decl.method.as_deref() != Some("wilson") {
                n.refuse(
                    "XS-IV-06",
                    "counts of successes and trials take a Wilson confidence interval",
                    "/chart/interval",
                    "kind: confidence, method: wilson",
                );
                return vec![];
            }
            if k.len() != labels.len() || t.len() != labels.len() {
                n.refuse(
                    "R15-2",
                    "successes and trials need one entry per category",
                    &base,
                    "matching lengths",
                );
                return vec![];
            }
            for gi in 0..labels.len() {
                let (Some(kk), Some(nn)) = (k[gi].as_u64(), t[gi].as_u64()) else {
                    n.refuse(
                        "R15-3",
                        "successes and trials must be non-negative integers",
                        &base,
                        "integers",
                    );
                    return vec![];
                };
                let Some((lo, hi)) = wilson(kk, nn, decl.level.unwrap()) else {
                    n.refuse(
                        "XS-IV-06",
                        format!("'{}' has {kk} successes in {nn} trials", labels[gi]),
                        &base,
                        "trials > 0 and successes <= trials",
                    );
                    return vec![];
                };
                let k = if percent { 100.0 } else { 1.0 };
                row.push(Estimate {
                    value: kk as f64 / nn as f64 * k,
                    lo: lo * k,
                    hi: hi * k,
                    size: format!("{kk}/{nn}"),
                });
            }
        } else if let (Some(vals), Some(ivs)) = (
            s.json.get("values"),
            s.json.get("intervals").and_then(Value::as_array),
        ) {
            let Some(vals) = numbers(vals, &format!("{base}/values"), n) else {
                return vec![];
            };
            if vals.len() != labels.len() || ivs.len() != labels.len() {
                n.refuse(
                    "R15-2",
                    "values and intervals need one entry per category",
                    &base,
                    "matching lengths",
                );
                return vec![];
            }
            for gi in 0..labels.len() {
                let (Some(lo), Some(hi)) = (
                    ivs[gi].get("lo").and_then(Value::as_f64),
                    ivs[gi].get("hi").and_then(Value::as_f64),
                ) else {
                    n.refuse(
                        "R15-3",
                        "each interval needs numeric lo and hi",
                        &format!("{base}/intervals/{gi}"),
                        "lo and hi",
                    );
                    return vec![];
                };
                if !(lo <= vals[gi] && vals[gi] <= hi) {
                    n.refuse(
                        "XS-IV-06",
                        format!(
                            "the estimate for '{}' lies outside its own interval",
                            labels[gi]
                        ),
                        &format!("{base}/intervals/{gi}"),
                        "lo <= value <= hi",
                    );
                    return vec![];
                }
                let size = ivs[gi]
                    .get("n")
                    .and_then(Value::as_u64)
                    .map(|c| format!("n={c}"))
                    .unwrap_or_default();
                row.push(Estimate {
                    value: vals[gi],
                    lo,
                    hi,
                    size,
                });
            }
        } else {
            n.refuse("R15-2", format!("series '{}' needs `samples`, `successes` with `trials`, or `values` with `intervals`", s.id), &base, "an interval source");
            return vec![];
        }
        out.push(row);
    }
    out
}

#[derive(Debug, Clone)]
pub struct IntervalModel {
    pub labels: Vec<String>,
    pub series: Vec<SeriesMeta>,
    /// `[series][group]`
    pub estimates: Vec<Vec<Estimate>>,
    pub decl: Decl,
    pub bar: bool,
    pub measure: Measure,
    /// What the marks are: "mean", "median", "observed proportion" or "estimate as supplied".
    pub marks: String,
}

impl IntervalModel {
    pub fn extent(&self) -> (f64, f64) {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for e in self.estimates.iter().flatten() {
            lo = lo.min(e.lo);
            hi = hi.max(e.hi);
        }
        (lo, hi)
    }
    pub fn note(&self) -> String {
        let head = if self.marks == "observed proportion" {
            "Marks: observed proportion.".to_string()
        } else if self.marks == "estimate as supplied" {
            format!(
                "{}: estimate as supplied.",
                if self.bar { "Bars" } else { "Dots" }
            )
        } else {
            format!(
                "{}: {}.",
                if self.bar { "Bars" } else { "Dots" },
                self.marks
            )
        };
        format!("{head} {}", self.decl.note())
    }
    pub fn values(&self) -> Vec<Vec<f64>> {
        self.estimates
            .iter()
            .map(|r| r.iter().map(|e| e.value).collect())
            .collect()
    }
}

pub fn parse(spec: &Value, bar: bool, n: &mut Notes) -> Option<IntervalModel> {
    let decl = parse_decl(spec, n);
    let labels = labels(spec, n);
    let raw = series(spec, n);
    check_series_count(raw.len(), 6, n);
    let measure = Measure::parse(spec.pointer("/data/measure").unwrap_or(&Value::Null));
    if bar && !measure.zero_meaningful {
        n.refuse(
            "XS-IV-04",
            "bars encode their length from zero; this measure's zero is not declared meaningful",
            "/data/measure/zero",
            "type: interval_dot, or zero: meaningful",
        );
    }
    if n.refused() {
        return None;
    }
    let decl = decl?;
    let estimates = estimates(&labels, &raw, &decl, measure.percent, n);
    if n.refused() {
        return None;
    }
    let model = IntervalModel {
        marks: if raw.iter().any(|s| s.json.get("successes").is_some()) {
            "observed proportion".into()
        } else if raw.iter().any(|s| s.json.get("samples").is_some()) {
            decl.statistic.name().into()
        } else {
            "estimate as supplied".into()
        },
        labels,
        series: metas(&raw),
        estimates,
        decl,
        bar,
        measure,
    };
    Some(model)
}
