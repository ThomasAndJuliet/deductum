//! Box plots, computed from samples or from declared summaries (XS-BOX-01..10).

use crate::notes::Notes;
use crate::read::*;
use crate::stats::*;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub stats: BoxStats,
    pub samples: Option<Vec<f64>>,
    /// Every point is drawn over the box (n of 8 or fewer, or `points: all`).
    pub draw_all: bool,
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum Points {
    Outliers,
    All,
    None,
}

#[derive(Debug, Clone)]
pub struct BoxModel {
    pub labels: Vec<String>,
    pub series: Vec<SeriesMeta>,
    /// `[series][group]`
    pub cells: Vec<Vec<Cell>>,
    pub whiskers: Whiskers,
    pub method: QuantileMethod,
    pub points: Points,
    pub mean_marker: bool,
    pub measure: Measure,
}

impl BoxModel {
    /// The values the axis must hold: whisker ends and every drawn outlier.
    pub fn extent(&self) -> (f64, f64) {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in self.cells.iter().flatten() {
            lo = lo.min(c.stats.lower_whisker);
            hi = hi.max(c.stats.upper_whisker);
            for o in &c.stats.outliers {
                lo = lo.min(*o);
                hi = hi.max(*o);
            }
            if c.draw_all
                && let Some(s) = &c.samples
            {
                lo = lo.min(min(s));
                hi = hi.max(max(s));
            }
        }
        (lo, hi)
    }
    /// The sentence stating how the box was built.
    pub fn note(&self) -> String {
        let mut note = format!(
            "Box: first to third quartile, line at the median ({} quantiles); {}.",
            self.method.name(),
            self.whiskers.describe()
        );
        if self.points != Points::None {
            note.push_str(" Points beyond the whiskers are drawn individually.");
        }
        if self.mean_marker {
            note.push_str(" Diamond: mean.");
        }
        note.push_str(" n at right.");
        note
    }
    /// One estimate per series and category: the median.
    pub fn estimates(&self) -> Vec<Vec<f64>> {
        self.cells
            .iter()
            .map(|row| row.iter().map(|c| c.stats.median).collect())
            .collect()
    }
}

pub fn parse_whiskers(spec: &Value, n: &mut Notes) -> Whiskers {
    let parsed = match spec.pointer("/chart/whiskers") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match s.as_str() {
            "tukey" | "tukey_1.5iqr" => Some(Whiskers::Tukey(1.5)),
            "min_max" => Some(Whiskers::MinMax),
            _ => {
                n.refuse(
                    "XS-BOX-01",
                    format!("unknown whisker rule '{s}'"),
                    "/chart/whiskers",
                    "tukey, min_max, or {rule: percentile, lower, upper}",
                );
                return Whiskers::Tukey(1.5);
            }
        },
        Some(o) => match o.get("rule").and_then(Value::as_str) {
            Some("tukey") => Some(Whiskers::Tukey(
                o.get("k").and_then(Value::as_f64).unwrap_or(1.5),
            )),
            Some("min_max") => Some(Whiskers::MinMax),
            Some("percentile") => match (
                o.get("lower").and_then(Value::as_f64),
                o.get("upper").and_then(Value::as_f64),
            ) {
                (Some(a), Some(b)) if 0.0 <= a && a < b && b <= 100.0 => {
                    Some(Whiskers::Percentile(a, b))
                }
                _ => {
                    n.refuse(
                        "XS-BOX-01",
                        "percentile whiskers need 0 <= lower < upper <= 100",
                        "/chart/whiskers",
                        "lower and upper percentiles",
                    );
                    return Whiskers::Tukey(1.5);
                }
            },
            _ => {
                n.refuse(
                    "XS-BOX-01",
                    "unknown whisker rule",
                    "/chart/whiskers",
                    "tukey, min_max or percentile",
                );
                return Whiskers::Tukey(1.5);
            }
        },
    };
    parsed.unwrap_or_else(|| {
        n.info(
            "XS-BOX-01",
            "no whisker rule given: Tukey's 1.5\u{d7}IQR is used and stated in the chart's note",
            "/chart/whiskers",
        );
        n.decide(
            "whiskers",
            "tukey 1.5",
            "default (matplotlib, ggplot2, Altair); drawn in the note",
        );
        Whiskers::Tukey(1.5)
    })
}

pub fn parse_quantiles(spec: &Value, n: &mut Notes) -> QuantileMethod {
    match spec.pointer("/chart/quantiles").and_then(Value::as_str) {
        None => {
            n.decide(
                "quantiles",
                "linear",
                "default (R type 7: pandas, NumPy, Altair)",
            );
            QuantileMethod::Linear
        }
        Some(name) => QuantileMethod::parse(name).unwrap_or_else(|| {
            n.refuse(
                "XS-BOX-02",
                format!("unknown quantile method '{name}'"),
                "/chart/quantiles",
                "linear, weibull, lower, higher, nearest or midpoint",
            );
            QuantileMethod::Linear
        }),
    }
}

pub fn parse(spec: &Value, n: &mut Notes) -> Option<BoxModel> {
    if spec.pointer("/chart/notch").is_some() {
        n.refuse("XS-BOX-06", "notches imply a confidence-interval comparison of medians, which this chart does not compute", "/chart/notch", "remove the notch, or use type: interval_dot with a declared interval");
    }
    if spec.pointer("/chart/n").and_then(Value::as_str) == Some("hide") {
        n.refuse(
            "XS-BOX-02",
            "the sample size is always shown: a box without n cannot be judged",
            "/chart/n",
            "remove chart.n",
        );
    }
    let whiskers = parse_whiskers(spec, n);
    let method = parse_quantiles(spec, n);
    let labels = labels(spec, n);
    let raw = series(spec, n);
    check_series_count(raw.len(), 6, n);
    let measure = Measure::parse(spec.pointer("/data/measure").unwrap_or(&Value::Null));
    let points = match spec.pointer("/chart/points").and_then(Value::as_str) {
        None | Some("outliers") => Points::Outliers,
        Some("all") => Points::All,
        Some("none") => Points::None,
        Some(o) => {
            n.refuse(
                "XS-BOX-02",
                format!("unknown points mode '{o}'"),
                "/chart/points",
                "outliers, all or none",
            );
            Points::Outliers
        }
    };
    if n.refused() {
        return None;
    }
    let mut cells = vec![];
    for (si, s) in raw.iter().enumerate() {
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
                    &format!("/data/series/{si}/samples"),
                    "one sample array per category",
                );
                return None;
            }
            for (gi, g) in sam.iter().enumerate() {
                let path = format!("/data/series/{si}/samples/{gi}");
                let v = numbers(g, &path, n)?;
                let who = format!("'{}' in series '{}'", labels[gi], s.id);
                if v.is_empty() {
                    n.refuse(
                        "XS-BOX-05",
                        format!("{who} has no samples"),
                        &path,
                        "samples, or drop the category",
                    );
                    return None;
                }
                if v.len() < 5 {
                    n.refuse(
                        "XS-BOX-03",
                        format!(
                            "{who} has n={}, too few for quartiles to mean anything",
                            v.len()
                        ),
                        &path,
                        "a strip chart (type: strip), which shows every value",
                    );
                    continue;
                }
                if v.len() <= 8 {
                    n.warn(
                        "XS-BOX-04",
                        format!("{who} has n={}: every point is drawn over the box", v.len()),
                        &path,
                    );
                }
                if v.len() >= 20
                    && let Some(bc) = bimodality_coefficient(&v)
                    && bc > 5.0 / 9.0
                {
                    n.info("XS-BOX-07", format!("{who} may be multimodal (bimodality coefficient {bc:.2}); a box hides that. Consider type: ecdf or strip"), &path);
                }
                let stats = box_stats(&v, whiskers, method).expect("non-empty");
                row.push(Cell {
                    draw_all: points == Points::All || v.len() <= 8,
                    stats,
                    samples: Some(v),
                });
            }
        } else if let Some(sum) = s.json.get("summaries").and_then(Value::as_array) {
            if sum.len() != labels.len() {
                n.refuse(
                    "R15-2",
                    format!(
                        "series '{}' has {} summaries for {} categories",
                        s.id,
                        sum.len(),
                        labels.len()
                    ),
                    &format!("/data/series/{si}/summaries"),
                    "one summary per category",
                );
                return None;
            }
            if spec.pointer("/chart/whiskers").is_none() {
                n.refuse("XS-BOX-08", "summarised input must state its whisker rule: the five numbers cannot be reinterpreted", "/chart/whiskers", "chart.whiskers describing how the supplied whiskers were computed");
            }
            for (gi, c) in sum.iter().enumerate() {
                let path = format!("/data/series/{si}/summaries/{gi}");
                let f = |k: &str| c.get(k).and_then(Value::as_f64);
                let count = c.get("n").and_then(Value::as_u64);
                if count.is_none() {
                    n.refuse(
                        "XS-BOX-09",
                        "a summary must carry n",
                        &path,
                        "n, the sample size",
                    );
                }
                let outliers = match c.get("outliers") {
                    Some(Value::String(s)) if s == "none" => Some(vec![]),
                    Some(a @ Value::Array(_)) => numbers(a, &format!("{path}/outliers"), n),
                    _ => {
                        n.refuse("XS-BOX-10", "a summary must supply its outliers, or state outliers: \"none\"; a missing outlier is not a clean distribution", &path, "outliers: [...] or \"none\"");
                        None
                    }
                };
                match (
                    f("q1"),
                    f("median"),
                    f("q3"),
                    f("lower_whisker"),
                    f("upper_whisker"),
                    count,
                    outliers,
                ) {
                    (
                        Some(q1),
                        Some(median),
                        Some(q3),
                        Some(lw),
                        Some(uw),
                        Some(count),
                        Some(outliers),
                    ) => row.push(Cell {
                        stats: BoxStats {
                            n: count as usize,
                            q1,
                            median,
                            q3,
                            mean: f("mean").unwrap_or(f64::NAN),
                            lower_whisker: lw,
                            upper_whisker: uw,
                            outliers,
                        },
                        samples: None,
                        draw_all: false,
                    }),
                    _ => {
                        if !n.refused() {
                            n.refuse(
                                "R15-2",
                                "a summary needs q1, median, q3, lower_whisker and upper_whisker",
                                &path,
                                "the five numbers",
                            );
                        }
                    }
                }
            }
        } else {
            n.refuse(
                "R15-2",
                format!(
                    "series '{}' needs `samples` (one array per category) or `summaries`",
                    s.id
                ),
                &format!("/data/series/{si}"),
                "samples",
            );
            return None;
        }
        cells.push(row);
    }
    if n.refused() {
        return None;
    }
    let model = BoxModel {
        labels,
        series: metas(&raw),
        cells,
        whiskers,
        method,
        points,
        mean_marker: spec.pointer("/chart/mean").and_then(Value::as_str) == Some("marker"),
        measure,
    };
    Some(model)
}
