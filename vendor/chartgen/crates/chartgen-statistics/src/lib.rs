//! Descriptive statistics and the input models of chartgen's statistical chart types.
//!
//! Pure: no geometry, no I/O, no knowledge of reports. Each chart type's `parse` reads a
//! chartspec's JSON, checks it against the rules in `docs/chartgen/STATS.md`, computes the
//! statistics the chart shows, and returns a model that policy, scale training and layout all
//! read, so they cannot disagree about what the chart contains.

pub mod box_plot;
pub mod distribution;
pub mod interval;
pub mod notes;
pub mod paired;
pub mod read;
pub mod scatter;
pub mod states;
pub mod stats;

use notes::Notes;
use serde_json::Value;

/// The `chart.type` values handled here.
pub const TYPES: [&str; 9] = [
    "box",
    "strip",
    "ecdf",
    "scatter",
    "dumbbell",
    "interval_dot",
    "interval_bar",
    "state_matrix",
    "contingency",
];

pub fn is_statistical(chart_type: &str) -> bool {
    TYPES.contains(&chart_type)
}

pub enum Model {
    Box(box_plot::BoxModel),
    Strip(distribution::StripModel),
    Ecdf(distribution::EcdfModel),
    Scatter(scatter::ScatterModel),
    Paired(paired::PairedModel),
    Interval(interval::IntervalModel),
    States(states::StateMatrix),
    Contingency(states::Contingency),
}

impl Model {
    /// The value-axis extent the scale must hold; `None` for charts with no value axis.
    pub fn extent(&self) -> Option<(f64, f64)> {
        match self {
            Model::Box(m) => Some(m.extent()),
            Model::Strip(m) => Some(m.extent()),
            Model::Ecdf(m) => Some(m.extent()),
            Model::Scatter(m) => Some(m.extents().1),
            Model::Paired(m) => Some(m.extent()),
            Model::Interval(m) => Some(m.extent()),
            Model::States(_) | Model::Contingency(_) => None,
        }
    }
    /// The sentence under the chart stating how its marks were computed.
    pub fn note(&self) -> String {
        match self {
            Model::Box(m) => m.note(),
            Model::Strip(m) => m.note(),
            Model::Ecdf(m) => m.note(),
            Model::Scatter(m) => m.note(),
            Model::Paired(m) => m.note(),
            Model::Interval(m) => m.note(),
            Model::States(m) => m.note(),
            Model::Contingency(m) => m.note(),
        }
    }
}

/// Read a spec's statistical chart. `None` with an error in `notes` when it must be refused;
/// the notes also carry warnings and the choices that were defaulted.
pub fn parse(spec: &Value, chart_type: &str, notes: &mut Notes) -> Option<Model> {
    let model = match chart_type {
        "box" => Model::Box(box_plot::parse(spec, notes)?),
        "strip" => Model::Strip(distribution::parse_strip(spec, notes)?),
        "ecdf" => Model::Ecdf(distribution::parse_ecdf(spec, notes)?),
        "scatter" => Model::Scatter(scatter::parse(spec, notes)?),
        "dumbbell" => Model::Paired(paired::parse(spec, notes)?),
        "interval_dot" => Model::Interval(interval::parse(spec, false, notes)?),
        "interval_bar" => Model::Interval(interval::parse(spec, true, notes)?),
        "state_matrix" => Model::States(states::parse_matrix(spec, notes)?),
        "contingency" => Model::Contingency(states::parse_contingency(spec, notes)?),
        _ => return None,
    };
    if notes.refused() { None } else { Some(model) }
}

/// The point estimate of each category in each series, used to give the generic pipeline
/// (ordering, data facts, comparisons) ordinary `values` for a chart whose input is samples.
/// `None` for scatter, which is not category-by-series.
pub fn estimates(model: &Model) -> Option<Vec<Vec<f64>>> {
    Some(match model {
        Model::Box(m) => m.estimates(),
        Model::Strip(m) => m.estimates(),
        Model::Ecdf(m) => m.estimates(),
        Model::Paired(m) => m.estimates(),
        Model::Interval(m) => m.values(),
        // state index per cell; counts per cell: the data the generic stages see
        Model::States(m) => m
            .cells
            .iter()
            .map(|r| r.iter().map(|k| *k as f64).collect())
            .collect(),
        Model::Contingency(m) => m
            .counts
            .iter()
            .map(|r| r.iter().map(|c| *c as f64).collect())
            .collect(),
        Model::Scatter(_) => return None,
    })
}

impl Model {
    /// What the chart shows, as numbers: the statistics behind each mark, for the report's
    /// summary and a figure's provenance.
    pub fn summary(&self) -> Value {
        use serde_json::json;
        match self {
            Model::Box(m) => json!({
                "whiskers": m.whiskers.describe(),
                "quantiles": m.method.name(),
                "boxes": m.cells.iter().enumerate().flat_map(|(s, row)| row.iter().enumerate().map(move |(g, c)| {
                    json!({"series": m.series[s].id, "x": m.labels[g], "n": c.stats.n, "q1": c.stats.q1, "median": c.stats.median,
                           "q3": c.stats.q3, "lower_whisker": c.stats.lower_whisker, "upper_whisker": c.stats.upper_whisker,
                           "outliers": c.stats.outliers.len()})
                })).collect::<Vec<_>>(),
            }),
            Model::Strip(m) => {
                json!({"strips": m.samples.iter().enumerate().flat_map(|(s, row)| row.iter().enumerate().map(move |(g, v)| {
                json!({"series": m.series[s].id, "x": m.labels[g], "n": v.len(), "median": stats::median(v)})
            })).collect::<Vec<_>>()})
            }
            Model::Ecdf(m) => json!({"curves": m.curves.iter().map(|c| json!({
                "name": c.name, "n": c.n, "quantiles": c.quantiles.iter().map(|(p, v)| json!({"p": p, "value": v})).collect::<Vec<_>>()
            })).collect::<Vec<_>>()}),
            Model::Scatter(m) => json!({"n": m.total, "fits": m.fits.iter().map(|f| json!({
                "series": m.series[f.series].id, "slope": f.slope, "intercept": f.intercept, "r": f.r, "n": f.n
            })).collect::<Vec<_>>()}),
            Model::Paired(m) => {
                json!({"difference": m.diff_name, "rows": m.labels.iter().enumerate().map(|(g, l)| json!({"x": l, "difference": m.diffs[g]})).collect::<Vec<_>>()})
            }
            Model::Interval(m) => {
                json!({"interval": m.decl.note(), "marks": m.marks, "estimates": m.estimates.iter().enumerate().flat_map(|(s, row)| row.iter().enumerate().map(move |(g, e)| {
                json!({"series": m.series[s].id, "x": m.labels[g], "estimate": e.value, "lo": e.lo, "hi": e.hi, "size": e.size})
            })).collect::<Vec<_>>()})
            }
            Model::States(m) => {
                json!({"counts": m.rows.iter().zip(m.counts()).map(|(r, c)| json!({"row": r, "counts": c})).collect::<Vec<_>>()})
            }
            Model::Contingency(m) => {
                json!({"total": m.total, "row_totals": m.row_totals, "column_totals": m.col_totals})
            }
        }
    }
}

fn has_series_key(raw: &Value, key: &str) -> bool {
    raw.pointer("/data/series")
        .and_then(Value::as_array)
        .is_some_and(|a| a.iter().any(|s| s.get(key).is_some()))
}

fn min_n(raw: &Value) -> Option<usize> {
    raw.pointer("/data/series")?
        .as_array()?
        .iter()
        .filter_map(|s| s.get("samples")?.as_array())
        .flatten()
        .filter_map(|g| g.as_array().map(Vec::len))
        .min()
}

/// The statistical chart type the data call for under `chart.type: auto`, with the reason:
/// xy pairs, counts, declared states, successes with trials or supplied intervals, samples (a
/// strip when a group is too small for quartiles, else a box; an ECDF for the message
/// `distribution_shape`), or two paired series with the message `paired_change`.
pub fn infer_type(raw: &Value) -> Option<(&'static str, String)> {
    let message = raw.pointer("/facts/message").and_then(Value::as_str);
    if has_series_key(raw, "xy") {
        return Some((
            "scatter",
            "series of xy pairs: a relationship between two measures".into(),
        ));
    }
    if has_series_key(raw, "counts") {
        return Some((
            "contingency",
            "series of counts with data.parts: a two-way partition".into(),
        ));
    }
    if has_series_key(raw, "states") {
        return Some((
            "state_matrix",
            "series of declared states: a state in every cell".into(),
        ));
    }
    if has_series_key(raw, "successes")
        || has_series_key(raw, "intervals")
        || (has_series_key(raw, "samples") && message == Some("estimate_uncertainty"))
    {
        return Some((
            "interval_dot",
            "an estimate with an interval: the uncertainty is the message".into(),
        ));
    }
    if has_series_key(raw, "samples") || has_series_key(raw, "summaries") {
        if message == Some("distribution_shape") {
            return Some((
                "ecdf",
                "facts.message distribution_shape: the share at or below each value".into(),
            ));
        }
        return match min_n(raw) {
            Some(n) if n < 5 => Some((
                "strip",
                format!(
                    "a group of {n} measurements: too few for quartiles, so every value is drawn"
                ),
            )),
            _ => Some((
                "box",
                "measurements per category: their distribution, summarised by quartiles".into(),
            )),
        };
    }
    let two_series = raw
        .pointer("/data/series")
        .and_then(Value::as_array)
        .is_some_and(|a| a.len() == 2);
    if message == Some("paired_change") && two_series {
        return Some((
            "dumbbell",
            "facts.message paired_change: two conditions on the same categories".into(),
        ));
    }
    None
}
