//! Lossless chart value parsing and derived measures. No host or layout dependencies.
use chartgen_model::{
    ChartType, DataFacts, Datum, Decimal, Error, PreparedComparison, PreparedData, Result, Series,
    Spec, TimedDatum,
};
use chrono::{Duration, NaiveDate, Weekday};
use serde_json::Value;
use serde_json::json;
use std::str::FromStr;
pub mod irregular;
mod schema;
mod statistical;
pub use schema::{schema, validate};

fn err(path: &str, message: impl Into<String>) -> Error {
    Error::new("CG-4", message).at(path)
}
fn decimal(v: &Value, path: &str) -> Result<Decimal> {
    if !v.is_number() {
        return Err(err(path, "expected a number"));
    }
    let written = v.to_string();
    Decimal::from_str(&written)
        .or_else(|_| Decimal::from_scientific(&written))
        .map_err(|_| err(path, "number exceeds decimal range"))
}
fn datum(v: &Value, path: &str) -> Result<Datum> {
    let mut d = Datum {
        value: None,
        status: "missing".into(),
        certainty: "actual".into(),
        complete: true,
        note: None,
        limit: None,
    };
    if v.is_number() {
        d.value = Some(decimal(v, path)?);
        d.status = "known".into();
        return Ok(d);
    }
    if v.is_null() {
        return Ok(d);
    }
    if let Some(s) = v.as_str() {
        let s = s.trim();
        let (side, number) = s
            .split_at_checked(1)
            .ok_or_else(|| err(path, "expected a censored value such as <1"))?;
        if side != "<" && side != ">" {
            return Err(err(path, "expected a censored value such as <1"));
        }
        d.status = if side == "<" { "below" } else { "above" }.into();
        d.limit = Some(
            Decimal::from_str(number.trim()).map_err(|_| err(path, "invalid censored limit"))?,
        );
        return Ok(d);
    }
    let o = v
        .as_object()
        .ok_or_else(|| err(path, "expected a number, null, or value object"))?;
    let note = o.get("note");
    if let Some(n) = note {
        d.note = Some(
            n.as_str()
                .ok_or_else(|| err(path, "note must be text"))?
                .into(),
        );
    }
    for side in ["below", "above"] {
        if let Some(limit) = o.get(side) {
            if o.keys().any(|k| k != side && k != "note") {
                return Err(err(path, "unexpected censored value field"));
            }
            d.status = side.into();
            d.limit = Some(decimal(limit, path)?);
            return Ok(d);
        }
    }
    if let Some(status) = o.get("status") {
        if o.keys().any(|k| k != "status" && k != "note") {
            return Err(err(path, "unexpected status field"));
        }
        let status = status
            .as_str()
            .ok_or_else(|| err(path, "status must be text"))?;
        if !["missing", "unknown", "pending", "tbd", "error"].contains(&status) {
            return Err(err(path, "invalid missing status"));
        }
        d.status = status.into();
        return Ok(d);
    }
    if let Some(value) = o.get("value") {
        if o.keys()
            .any(|k| !["value", "certainty", "complete", "note"].contains(&k.as_str()))
        {
            return Err(err(path, "unexpected value field"));
        }
        d.value = Some(decimal(value, path)?);
        d.status = "known".into();
        d.certainty = o
            .get("certainty")
            .map(|x| {
                x.as_str()
                    .ok_or_else(|| err(path, "certainty must be text"))
            })
            .transpose()?
            .unwrap_or("actual")
            .into();
        if !["actual", "estimate", "projection", "target"].contains(&d.certainty.as_str()) {
            return Err(err(path, "invalid certainty"));
        }
        d.complete = o
            .get("complete")
            .map(|x| {
                x.as_bool()
                    .ok_or_else(|| err(path, "complete must be boolean"))
            })
            .transpose()?
            .unwrap_or(true);
        return Ok(d);
    }
    Err(err(path, "unrecognized value object"))
}
fn array<'a>(v: &'a Value, path: &str) -> Result<&'a Vec<Value>> {
    v.as_array().ok_or_else(|| err(path, "expected an array"))
}
fn missing() -> Datum {
    Datum {
        value: None,
        status: "missing".into(),
        certainty: "actual".into(),
        complete: true,
        note: None,
        limit: None,
    }
}
fn inherit_certainty(d: &mut Datum, raw: &Value, series: &Value) {
    if raw.get("certainty").is_none() {
        d.certainty = series["certainty"].as_str().unwrap_or("actual").into();
    }
}
fn period_end(grid: &str, value: &str) -> Result<NaiveDate> {
    let bad = || err("data.x.values", "invalid regular time period");
    let date =
        |year: i32, month: u32, day: u32| NaiveDate::from_ymd_opt(year, month, day).ok_or_else(bad);
    match grid {
        "year" => date(value.parse::<i32>().map_err(|_| bad())? + 1, 1, 1),
        "quarter" => {
            let (year, q) = value.split_once("-Q").ok_or_else(bad)?;
            let year = year.parse::<i32>().map_err(|_| bad())?;
            let q = q.parse::<u32>().map_err(|_| bad())?;
            if !(1..=4).contains(&q) {
                return Err(bad());
            }
            let next = q * 3 + 1;
            if next > 12 {
                date(year + 1, 1, 1)
            } else {
                date(year, next, 1)
            }
        }
        "month" => {
            let (year, month) = value.split_once('-').ok_or_else(bad)?;
            let year = year.parse::<i32>().map_err(|_| bad())?;
            let month = month.parse::<u32>().map_err(|_| bad())?;
            if !(1..=12).contains(&month) {
                return Err(bad());
            }
            if month == 12 {
                date(year + 1, 1, 1)
            } else {
                date(year, month + 1, 1)
            }
        }
        "week" => {
            let start = if let Some((year, week)) = value.split_once("-W") {
                NaiveDate::from_isoywd_opt(
                    year.parse::<i32>().map_err(|_| bad())?,
                    week.parse::<u32>().map_err(|_| bad())?,
                    Weekday::Mon,
                )
                .ok_or_else(bad)?
            } else {
                NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| bad())?
            };
            start.checked_add_signed(Duration::days(7)).ok_or_else(bad)
        }
        "day" | "hour" => {
            let day = value.split_once('T').map_or(value, |(day, _)| day);
            NaiveDate::parse_from_str(day, "%Y-%m-%d")
                .map_err(|_| bad())?
                .checked_add_signed(Duration::days(1))
                .ok_or_else(bad)
        }
        "bin" => {
            let (_, last) = value
                .split_once('–')
                .or_else(|| value.split_once('-'))
                .ok_or_else(bad)?;
            date(last.parse::<i32>().map_err(|_| bad())? + 1, 1, 1)
        }
        _ => Err(err("data.x.grid", "as_of requires a regular calendar grid")),
    }
}
fn incomplete_periods(spec: &Spec, categories: &[String]) -> Result<Vec<bool>> {
    let Some(as_of) = spec.get("/data/as_of").as_str() else {
        return Ok(vec![false; categories.len()]);
    };
    let as_of = NaiveDate::parse_from_str(as_of, "%Y-%m-%d")
        .map_err(|_| err("data.as_of", "not a date (YYYY-MM-DD)"))?;
    let cutoff = as_of
        .checked_add_signed(Duration::days(1))
        .ok_or_else(|| err("data.as_of", "date out of range"))?;
    let grid = spec.string("/data/x/grid", "");
    categories
        .iter()
        .map(|c| Ok(period_end(&grid, c)? > cutoff))
        .collect()
}
fn text_value(v: &Value) -> Option<&str> {
    v.as_str().or_else(|| {
        ["alternative", "typst", "tex", "latex"]
            .iter()
            .find_map(|k| v[*k].as_str())
    })
}
fn unit_magnitude(u: &Value) -> i32 {
    match u["given_in"].as_str().unwrap_or("") {
        "thousands" => 3,
        "millions" => 6,
        "billions" => 9,
        "trillions" => 12,
        _ => u["magnitude"].as_i64().unwrap_or(0) as i32,
    }
}
fn unit_key(u: &Value) -> (String, String, String) {
    let prefix = u["prefix"].as_str().unwrap_or("").trim().to_owned();
    let suffix = u["suffix"].as_str().unwrap_or("").trim().to_owned();
    let noun = if prefix.is_empty() && suffix.is_empty() {
        u["noun"].as_str().unwrap_or("").trim().to_lowercase()
    } else {
        String::new()
    };
    (prefix, suffix, noun)
}
fn pow10(n: i32) -> Decimal {
    if n < 0 {
        Decimal::from_i128_with_scale(1, (-n) as u32)
    } else {
        let mut x = Decimal::ONE;
        for _ in 0..n {
            x *= Decimal::from(10)
        }
        x
    }
}
fn parse_comparisons(spec: &Spec, n: usize) -> Result<Vec<PreparedComparison>> {
    let Some(source) = spec.get("/data/comparisons").as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    let series_ids: std::collections::BTreeSet<&str> = spec
        .get("/data/series")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["id"].as_str())
        .collect();
    for (i, c) in source.iter().enumerate() {
        let path = format!("data.comparisons[{i}]");
        let id = c["id"]
            .as_str()
            .ok_or_else(|| err(&format!("{path}.id"), "required comparison id"))?;
        if !ids.insert(id) || series_ids.contains(id) {
            return Err(err(
                &format!("{path}.id"),
                "comparison ids must be distinct from each other and from series",
            ));
        }
        let kind = c["kind"]
            .as_str()
            .ok_or_else(|| err(&format!("{path}.kind"), "comparison kind required"))?;
        let compute = c["compute"].as_str();
        let raw_values = c["values"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        if compute.is_none() && raw_values.len() != 1 && raw_values.len() != n {
            return Err(err(
                &format!("{path}.values"),
                format!("one value, or one per x value ({n})"),
            ));
        }
        if compute.is_some() && !raw_values.is_empty() {
            return Err(err(
                &format!("{path}.values"),
                "computed comparison takes no values",
            ));
        }
        if compute.is_some() && (compute != Some("mean") || kind != "average") {
            return Err(err(
                &format!("{path}.compute"),
                "only an average is computed from plotted data",
            ));
        }
        if let Some(series) = c["series"].as_str() {
            if !series_ids.contains(series) {
                return Err(err(&format!("{path}.series"), "no such series"));
            }
        }
        let mut values = raw_values
            .iter()
            .enumerate()
            .map(|(j, v)| datum(v, &format!("{path}.values[{j}]")))
            .collect::<Result<Vec<_>>>()?;
        let factor = if !c["factor"].is_null() {
            Some(decimal(&c["factor"], &format!("{path}.factor"))?)
        } else if !c["unit"].is_null()
            && unit_key(&c["unit"]) == unit_key(spec.get("/data/measure/unit"))
        {
            Some(pow10(
                unit_magnitude(&c["unit"]) - unit_magnitude(spec.get("/data/measure/unit")),
            ))
        } else {
            None
        };
        if let Some(factor) = factor {
            for v in &mut values {
                if let Some(x) = v.value {
                    v.value = Some(
                        x.checked_mul(factor)
                            .ok_or_else(|| err(&path, "comparison unit conversion overflow"))?,
                    );
                }
            }
        }
        out.push(PreparedComparison {
            id: id.into(),
            kind: kind.into(),
            label: text_value(&c["label"]).unwrap_or(kind).into(),
            values,
            render: c["render"].as_str().unwrap_or("auto").into(),
            valid: c["valid"].as_bool().unwrap_or(true),
            series: c["series"].as_str().map(str::to_owned),
        });
    }
    Ok(out)
}
fn parse_irregular(spec: &Spec) -> Result<PreparedData> {
    let sources = array(spec.get("/data/series"), "data.series")?;
    let mut rows = Vec::new();
    let mut time_names = std::collections::BTreeMap::new();
    for (i, s) in sources.iter().enumerate() {
        let mut points = irregular::parse_points(s, i)?;
        for (p, raw) in points
            .iter_mut()
            .zip(s["points"].as_array().into_iter().flatten())
        {
            inherit_certainty(&mut p.datum, &raw[1], s);
        }
        for p in &points {
            time_names
                .entry(p.position)
                .or_insert_with(|| p.time.clone());
        }
        rows.push(points);
    }
    let categories = time_names.values().cloned().collect::<Vec<_>>();
    let positions = time_names.keys().copied().collect::<Vec<_>>();
    let mut series = Vec::new();
    let mut irregular = Vec::new();
    for (i, s) in sources.iter().enumerate() {
        let id = s["id"]
            .as_str()
            .ok_or_else(|| err(&format!("data.series[{i}].id"), "required series id"))?;
        let name = s["name"]
            .as_str()
            .or_else(|| s["name"]["typst"].as_str())
            .unwrap_or(id);
        let mut values = vec![missing(); categories.len()];
        let mut exact = Vec::new();
        for p in &rows[i] {
            let j = positions
                .binary_search(&p.position)
                .expect("source position in union");
            values[j] = p.datum.clone();
            exact.push(TimedDatum {
                time: p.time.clone(),
                position: p.position,
                datum: p.datum.clone(),
            });
        }
        series.push(Series {
            id: id.into(),
            name: name.into(),
            values,
        });
        irregular.push(exact);
    }
    Ok(PreparedData {
        comparisons: parse_comparisons(spec, categories.len())?,
        categories,
        x_kind: "time".into(),
        series,
        measure: spec.get("/data/measure").clone(),
        irregular: Some(irregular),
    })
}

pub fn parse(spec: &Spec) -> Result<PreparedData> {
    if let Some(f) = validate(spec).first() {
        return Err(err(f.path.as_deref().unwrap_or(""), f.message.clone()));
    }
    if schema::is_statistical(spec) {
        return statistical::parse(spec);
    }
    let x = spec.get("/data/x");
    if x["kind"] == "time" && x["grid"] == "none" {
        return parse_irregular(spec);
    }
    let kind = x
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| err("data.x.kind", "required x kind"))?;
    if !["nominal", "ordinal", "time"].contains(&kind) {
        return Err(err("data.x.kind", "unknown x kind"));
    }
    let categories: Vec<String> = array(&x["values"], "data.x.values")?
        .iter()
        .enumerate()
        .map(|(i, v)| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| err(&format!("data.x.values[{i}]"), "expected text"))
        })
        .collect::<Result<_>>()?;
    if categories.is_empty() {
        return Err(err("data.x.values", "at least one x value is required"));
    }
    if categories
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != categories.len()
    {
        return Err(err("data.x.values", "x values must be distinct"));
    }
    let incomplete = if kind == "time" {
        incomplete_periods(spec, &categories)?
    } else {
        vec![false; categories.len()]
    };
    let mut series = Vec::new();
    for (i, s) in array(spec.get("/data/series"), "data.series")?
        .iter()
        .enumerate()
    {
        let id = s
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| err(&format!("data.series[{i}].id"), "required series id"))?;
        let name = s
            .get("name")
            .and_then(|v| {
                v.as_str()
                    .or_else(|| v.get("typst").and_then(Value::as_str))
            })
            .unwrap_or(id);
        let values = s.get("values").ok_or_else(|| {
            err(
                &format!("data.series[{i}].values"),
                "values required; irregular points require a separate representation",
            )
        })?;
        let vals = array(values, &format!("data.series[{i}].values"))?;
        if vals.len() != categories.len() {
            return Err(err(
                &format!("data.series[{i}].values"),
                format!("expected {} values, found {}", categories.len(), vals.len()),
            ));
        }
        let values = vals
            .iter()
            .enumerate()
            .map(|(j, v)| {
                let mut d = datum(v, &format!("data.series[{i}].values[{j}]"))?;
                inherit_certainty(&mut d, v, s);
                if incomplete[j] {
                    d.complete = false;
                }
                Ok(d)
            })
            .collect::<Result<Vec<_>>>()?;
        series.push(Series {
            id: id.into(),
            name: name.into(),
            values,
        });
    }
    if series.is_empty() {
        return Err(err("data.series", "at least one series is required"));
    }
    if series
        .iter()
        .map(|s| &s.id)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != series.len()
    {
        return Err(err("data.series", "series ids must be distinct"));
    }
    Ok(PreparedData {
        comparisons: parse_comparisons(spec, categories.len())?,
        categories,
        x_kind: kind.into(),
        series,
        measure: spec.get("/data/measure").clone(),
        irregular: None,
    })
}

pub fn inspect(data: &PreparedData) -> DataFacts {
    let mut f = DataFacts {
        min: None,
        max: None,
        known: 0,
        missing: 0,
        censored: 0,
        series: data.series.len(),
        categories: data.categories.len(),
    };
    let data_values: Vec<&Datum> = if let Some(irregular) = &data.irregular {
        irregular
            .iter()
            .flat_map(|row| row.iter().map(|p| &p.datum))
            .collect()
    } else {
        data.series.iter().flat_map(|s| &s.values).collect()
    };
    for d in data_values {
        if let Some(v) = d.value {
            f.known += 1;
            f.min = Some(f.min.map_or(v, |x| x.min(v)));
            f.max = Some(f.max.map_or(v, |x| x.max(v)));
        } else if d.status == "below" || d.status == "above" {
            f.censored += 1;
        } else {
            f.missing += 1;
        }
    }
    f
}

/// Resolve chart-specific comparison values after the chart choice is known.
pub fn resolve_comparisons(
    spec: &Spec,
    chart_type: ChartType,
    data: &PreparedData,
) -> Result<PreparedData> {
    let mut out = data.clone();
    let raw = spec.get("/data/comparisons").as_array();
    let n = out.categories.len();
    let rank = |kind: &str| match kind {
        "target" | "budget" | "tolerance" | "forecast" => 0,
        "average" => 1,
        "previous_period" => 2,
        "peer" => 3,
        _ => 4,
    };
    let best = out
        .comparisons
        .iter()
        .filter(|c| c.valid)
        .map(|c| rank(&c.kind))
        .min()
        .unwrap_or(4);
    for (i, c) in out.comparisons.iter_mut().enumerate() {
        if c.render == "none" {
            continue;
        }
        if !c.valid && c.render == "auto" {
            c.render = "none".into();
            continue;
        }
        if c.kind == "previous_period" && best <= 1 && c.render == "auto" {
            c.render = "none".into();
            continue;
        }
        if raw
            .and_then(|a| a.get(i))
            .and_then(|v| v["compute"].as_str())
            == Some("mean")
        {
            if chart_type == ChartType::Line {
                let active: Vec<_> = out
                    .series
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| {
                        spec.get("/data/series")
                            .as_array()
                            .and_then(|a| a.get(*j))
                            .and_then(|s| s["role"].as_str())
                            .map_or(true, |r| r == "regular")
                    })
                    .map(|(_, s)| s)
                    .collect();
                c.values = (0..n)
                    .map(|j| {
                        let vals: Vec<_> = active
                            .iter()
                            .filter_map(|s| s.values.get(j).and_then(|d| d.value))
                            .collect();
                        if vals.is_empty() {
                            missing()
                        } else {
                            let mut d = missing();
                            d.value = Some(
                                vals.iter().copied().sum::<Decimal>() / Decimal::from(vals.len()),
                            );
                            d.status = "known".into();
                            d
                        }
                    })
                    .collect();
            } else {
                let series = c
                    .series
                    .as_ref()
                    .and_then(|id| out.series.iter().find(|s| &s.id == id))
                    .or_else(|| out.series.first())
                    .ok_or_else(|| err("data.series", "comparison mean needs a series"))?;
                let vals: Vec<_> = series.values.iter().filter_map(|d| d.value).collect();
                let mut d = missing();
                if !vals.is_empty() {
                    d.value =
                        Some(vals.iter().copied().sum::<Decimal>() / Decimal::from(vals.len()));
                    d.status = "known".into();
                }
                c.values = vec![d];
            }
        }
        c.render = if chart_type == ChartType::Line || (c.values.len() == n && n > 1) {
            "series"
        } else {
            "line"
        }
        .into();
    }
    Ok(out)
}
fn set(d: &mut Datum, value: Option<Decimal>) {
    d.value = value;
    if value.is_none() {
        d.status = "unknown".into();
    }
}

pub fn transform(spec: &Spec, data: &PreparedData) -> Result<PreparedData> {
    let t = spec.get("/transform");
    if t.is_null() {
        return Ok(data.clone());
    }
    if data.irregular.is_some() {
        return Err(err(
            "transform",
            "a derived measure needs values on a regular grid",
        ));
    }
    let kind = t["kind"]
        .as_str()
        .ok_or_else(|| err("transform.kind", "transform kind required"))?;
    let mut out = data.clone();
    match kind {
        "deviation" => {
            let id = t["comparison"]
                .as_str()
                .ok_or_else(|| err("transform.comparison", "comparison id required"))?;
            let comps = array(spec.get("/data/comparisons"), "data.comparisons")?;
            let c = comps
                .iter()
                .find(|x| x["id"] == id)
                .ok_or_else(|| err("transform.comparison", "comparison not found"))?;
            let base_comparison = data
                .comparisons
                .iter()
                .find(|x| x.id == id)
                .ok_or_else(|| err("transform.comparison", "prepared comparison not found"))?;
            let cv = &base_comparison.values;
            if cv.len() != 1 && cv.len() != data.categories.len() {
                return Err(err(
                    "data.comparisons.values",
                    "comparison needs one value or one per category",
                ));
            }
            let relative = t["relative"].as_bool().unwrap_or(false);
            let targets: Vec<usize> = if let Some(id) = c["series"].as_str() {
                vec![
                    out.series
                        .iter()
                        .position(|s| s.id == id)
                        .ok_or_else(|| err("data.comparisons.series", "series not found"))?,
                ]
            } else {
                vec![0]
            };
            for i in targets {
                for j in 0..out.categories.len() {
                    let r = &cv[if cv.len() == 1 { 0 } else { j }];
                    let d = &mut out.series[i].values[j];
                    if let (Some(v), Some(base)) = (d.value, r.value) {
                        if relative && base.is_zero() {
                            set(d, None);
                            d.note = Some("comparison value 0".into());
                        } else {
                            set(
                                d,
                                Some(if relative {
                                    (v - base)
                                        .checked_div(base)
                                        .and_then(|x| x.checked_mul(Decimal::from(100)))
                                        .ok_or_else(|| err("transform", "decimal overflow"))?
                                } else {
                                    v - base
                                }),
                            );
                        }
                    } else if d.value.is_some() {
                        set(d, None);
                        d.note = r.note.clone();
                    }
                }
            }
            for comparison in &mut out.comparisons {
                if comparison.id == id {
                    comparison.render = "none".into();
                    continue;
                }
                if comparison.render == "none" || comparison.values.is_empty() {
                    continue;
                }
                let mut derived = Vec::with_capacity(out.categories.len());
                for j in 0..out.categories.len() {
                    let v = &comparison.values[if comparison.values.len() == 1 { 0 } else { j }];
                    let r = &cv[if cv.len() == 1 { 0 } else { j }];
                    let mut d = v.clone();
                    d.value = match (v.value, r.value) {
                        (Some(value), Some(base)) if !relative => Some(value - base),
                        (Some(value), Some(base)) if !base.is_zero() => (value - base)
                            .checked_div(base)
                            .and_then(|x| x.checked_mul(Decimal::from(100))),
                        _ => None,
                    };
                    if d.value.is_none() {
                        d.status = "unknown".into();
                    }
                    derived.push(d);
                }
                let known: Vec<_> = derived.iter().filter_map(|v| v.value).collect();
                let constant = known.len() == derived.len()
                    && !known.is_empty()
                    && known.iter().all(|v| {
                        (*v - known[0]).abs()
                            <= Decimal::new(1, 3) * known[0].abs().max(Decimal::ONE)
                    });
                comparison.values = if constant {
                    vec![derived[0].clone()]
                } else {
                    derived
                };
            }
            out.measure["kind"] = Value::from("change");
            out.measure["zero"] = Value::from("meaningful");
            out.measure["min"] = Value::Null;
            out.measure["max"] = Value::Null;
            out.measure["precision"] = Value::Null;
            out.measure["dividing_point"] = json!(0);
            let kind = c["kind"].as_str().unwrap_or("comparison");
            let side = match kind {
                "budget" => "over budget",
                "tolerance" => "over tolerance",
                "average" => "above average",
                "previous_period" => "more than the previous period",
                "peer" => "above peers",
                _ => "above target",
            };
            out.measure["name"] =
                Value::from(t["name"].as_str().map(str::to_owned).unwrap_or_else(|| {
                    format!(
                        "deviation from {}",
                        c["label"].as_str().unwrap_or(kind).to_lowercase()
                    )
                }));
            let old = &data.measure["unit"];
            out.measure["unit"] = if !t["unit"].is_null() {
                t["unit"].clone()
            } else if relative {
                json!({"suffix":"%","noun":side})
            } else {
                json!({"prefix":old["prefix"],"suffix":old["suffix"],"magnitude":old["magnitude"],"noun":side})
            };
        }
        "ratio" => {
            let e = array(&t["exposure"]["values"], "transform.exposure.values")?;
            if e.len() != out.categories.len() {
                return Err(err(
                    "transform.exposure.values",
                    "one exposure per x value required",
                ));
            }
            let per = if t["per"].is_null() {
                Decimal::ONE
            } else {
                decimal(&t["per"], "transform.per")?
            };
            if per <= Decimal::ZERO {
                return Err(err("transform.per", "must be positive"));
            }
            for s in &mut out.series {
                for (j, d) in s.values.iter_mut().enumerate() {
                    let exposure = datum(&e[j], "transform.exposure.values")?;
                    if let (Some(v), Some(base)) = (d.value, exposure.value) {
                        if base.is_zero() {
                            set(d, None);
                        } else {
                            let result = v
                                .checked_div(base)
                                .and_then(|x| x.checked_mul(per))
                                .ok_or_else(|| err("transform", "decimal overflow"))?;
                            set(d, Some(result));
                        }
                    } else if d.value.is_some() {
                        set(d, None);
                    }
                }
            }
            let old = &data.measure["unit"];
            let base = old["noun"]
                .as_str()
                .or_else(|| old["what"].as_str())
                .or_else(|| data.measure["name"].as_str())
                .unwrap_or("values");
            let per_text = if per == Decimal::ONE {
                String::new()
            } else if per >= Decimal::from(100000) && per % Decimal::from(1000) == Decimal::ZERO {
                format!("{}K ", per / Decimal::from(1000))
            } else {
                format!("{per} ")
            };
            let noun = format!(
                "{base} per {per_text}{}",
                t["exposure"]["noun"].as_str().unwrap_or("exposure")
            );
            out.measure["name"] = Value::from(
                t["name"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| noun.clone()),
            );
            out.measure["unit"] = if !t["unit"].is_null() {
                t["unit"].clone()
            } else {
                json!({"prefix":old["prefix"],"suffix":old["suffix"],"noun":noun})
            };
            out.measure["kind"] =
                Value::from(if old["prefix"].as_str().is_some_and(|s| !s.is_empty()) {
                    "amount"
                } else {
                    "quantity"
                });
            out.measure["zero"] = Value::from("meaningful");
            out.measure["max"] = Value::Null;
            out.measure["precision"] = Value::Null;
        }
        "index" => {
            let at = t["at"].as_str().unwrap_or(&out.categories[0]);
            let idx = out
                .categories
                .iter()
                .position(|x| x == at)
                .ok_or_else(|| err("transform.at", "start x value not found"))?;
            let base = if t["base"].is_null() {
                Decimal::from(100)
            } else {
                decimal(&t["base"], "transform.base")?
            };
            if base < Decimal::ZERO {
                return Err(err("transform.base", "must be nonnegative"));
            }
            for s in &mut out.series {
                let start = s.values[idx].value.ok_or_else(|| {
                    err(
                        "transform.at",
                        format!("start value for {} is not known", s.id),
                    )
                })?;
                if start.is_zero() {
                    return Err(err(
                        "transform.at",
                        format!("start value for {} is zero", s.id),
                    ));
                }
                for d in &mut s.values {
                    if let Some(v) = d.value {
                        let result = if base.is_zero() {
                            v.checked_div(start)
                                .and_then(|x| x.checked_sub(Decimal::ONE))
                                .and_then(|x| x.checked_mul(Decimal::from(100)))
                        } else {
                            v.checked_div(start).and_then(|x| x.checked_mul(base))
                        }
                        .ok_or_else(|| err("transform", "decimal overflow"))?;
                        set(d, Some(result));
                    }
                }
            }
            let old = &data.measure["unit"];
            let prefix = old["prefix"].as_str().unwrap_or("");
            if base.is_zero() {
                out.measure["kind"] = Value::from("change");
                out.measure["name"] = Value::from(
                    t["name"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("% change since {at}")),
                );
                out.measure["unit"] = if !t["unit"].is_null() {
                    t["unit"].clone()
                } else {
                    json!({"suffix":"%","what":format!("change since {at}")})
                };
            } else {
                out.measure["kind"] = Value::from(if prefix.is_empty() {
                    "quantity"
                } else {
                    "amount"
                });
                out.measure["name"] = Value::from(
                    t["name"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("indexed to {prefix}{base} on {at}")),
                );
                out.measure["unit"] = if !t["unit"].is_null() {
                    t["unit"].clone()
                } else {
                    json!({"prefix":prefix,"what":format!("indexed: {prefix}{base} on {at}")})
                };
            }
            out.measure["zero"] = Value::from("meaningful");
            out.measure["min"] = Value::Null;
            out.measure["max"] = Value::Null;
            out.measure["precision"] = Value::Null;
        }
        _ => return Err(err("transform.kind", "unknown transform")),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn values_and_transforms() {
        let s = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"nominal","values":["A","B"]},"series":[{"id":"s","values":[{"value":0,"certainty":"estimate"},"<1"]}],"measure":{}}}),
        };
        let d = parse(&s).unwrap();
        assert_eq!(inspect(&d).known, 1);
        assert_eq!(inspect(&d).censored, 1);
        assert_eq!(d.series[0].values[0].value, Some(Decimal::ZERO));
    }
    #[test]
    fn ratio_preserves_missing() {
        let s = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"nominal","values":["A","B"]},"series":[{"id":"s","values":[20,null]}]},"transform":{"kind":"ratio","exposure":{"values":[4,2],"noun":"hours"},"per":100}}),
        };
        let d = transform(&s, &parse(&s).unwrap()).unwrap();
        assert_eq!(d.series[0].values[0].value, Some(Decimal::from(500)));
        assert_eq!(d.series[0].values[1].value, None);
    }
    #[test]
    fn deviation_and_index_keep_exact_values() {
        let s = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"time","grid":"year","values":["2020","2021"]},"series":[{"id":"s","values":[10,15]}],"comparisons":[{"id":"target","kind":"target","values":[8,12]}]},"transform":{"kind":"deviation","comparison":"target","relative":true}}),
        };
        let got = transform(&s, &parse(&s).unwrap()).unwrap();
        assert_eq!(got.series[0].values[0].value, Some(Decimal::from(25)));
        assert_eq!(got.series[0].values[1].value, Some(Decimal::from(25)));
        let s = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"time","grid":"year","values":["2020","2021"]},"series":[{"id":"s","values":[10,15]}]},"transform":{"kind":"index","base":0}}),
        };
        let got = transform(&s, &parse(&s).unwrap()).unwrap();
        assert_eq!(got.series[0].values[0].value, Some(Decimal::ZERO));
        assert_eq!(got.series[0].values[1].value, Some(Decimal::from(50)));
    }
    #[test]
    fn invalid_missing_value_is_not_zero() {
        let s = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"nominal","values":["A"]},"series":[{"id":"s","values":[{"status":"pending","value":0}]}]}}),
        };
        assert_eq!(
            parse(&s).unwrap_err().path.as_deref(),
            Some("data.series[0].values[0]")
        );
    }
    #[test]
    fn irregular_without_x_values_keeps_timed_points() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"price","data":{"x":{"kind":"time","grid":"none"},"measure":{"persistence":"state"},"series":[{"id":"price","points":[["2022-04-01",400],["2022-04-27",350],["2022-04-27",375]]}]}}),
        };
        let data = parse(&spec).unwrap();
        assert_eq!(data.categories.len(), 2);
        let exact = data.irregular.as_ref().unwrap();
        assert_eq!(exact[0].len(), 3);
        assert_eq!(exact[0][1].position, exact[0][2].position);
        assert_eq!(inspect(&data).known, 3);
        assert_eq!(inspect(&data).max, Some(Decimal::from(400)));
    }
    #[test]
    fn irregular_axis_bounds_do_not_become_data_categories() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"events","data":{"x":{"kind":"time","grid":"none","values":["2022-08-14T00","2022-08-15T00"]},"measure":{"persistence":"event"},"series":[{"id":"e","points":[["2022-08-14T05:20",200],["2022-08-14T05:50",80]]}]}}),
        };
        let data = parse(&spec).unwrap();
        assert_eq!(
            data.categories,
            vec!["2022-08-14T05:20", "2022-08-14T05:50"]
        );
        assert_eq!(data.irregular.as_ref().unwrap()[0].len(), 2);
    }
    #[test]
    fn comparisons_convert_compute_and_resolve() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"comparisons","data":{"x":{"kind":"time","grid":"year","values":["2020","2021"]},"measure":{"unit":{"prefix":"£","magnitude":6}},"series":[{"id":"s","values":[10,20]}],"comparisons":[{"id":"target","kind":"target","values":[12000],"unit":{"prefix":"£","magnitude":3}},{"id":"average","kind":"average","compute":"mean"},{"id":"prior","kind":"previous_period","values":[8,9]}]}}),
        };
        let parsed = parse(&spec).unwrap();
        assert_eq!(
            parsed.comparisons[0].values[0].value,
            Some(Decimal::from(12))
        );
        let line = resolve_comparisons(&spec, ChartType::Line, &parsed).unwrap();
        assert_eq!(line.comparisons[1].values[0].value, Some(Decimal::from(10)));
        assert_eq!(line.comparisons[1].values[1].value, Some(Decimal::from(20)));
        assert_eq!(line.comparisons[0].render, "series");
        assert_eq!(line.comparisons[2].render, "none");
        let bar = resolve_comparisons(&spec, ChartType::Bar, &parsed).unwrap();
        assert_eq!(bar.comparisons[1].values[0].value, Some(Decimal::from(15)));
        assert_eq!(bar.comparisons[0].render, "line");
    }
    #[test]
    fn deviation_converts_other_comparators() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"dev","data":{"x":{"kind":"nominal","values":["A","B"]},"series":[{"id":"s","values":[10,20]}],"comparisons":[{"id":"target","kind":"target","values":[8,18]},{"id":"tolerance","kind":"tolerance","values":[9,19]}]},"transform":{"kind":"deviation","comparison":"target"}}),
        };
        let data = transform(&spec, &parse(&spec).unwrap()).unwrap();
        assert_eq!(data.comparisons[0].render, "none");
        assert_eq!(data.comparisons[1].values[0].value, Some(Decimal::ONE));
        assert_eq!(data.comparisons[1].values.len(), 1);
    }
    #[test]
    fn deviation_can_recompute_a_mean_from_derived_values() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"dev-mean","data":{"x":{"kind":"nominal","values":["A","B"]},"series":[{"id":"s","values":[10,20]}],"comparisons":[{"id":"target","kind":"target","values":[8,18]},{"id":"mean","kind":"average","compute":"mean"}]},"transform":{"kind":"deviation","comparison":"target"}}),
        };
        let data = transform(&spec, &parse(&spec).unwrap()).unwrap();
        let chosen = resolve_comparisons(&spec, ChartType::Bar, &data).unwrap();
        assert_eq!(
            chosen.comparisons[1].values[0].value,
            Some(Decimal::from(2))
        );
    }
    #[test]
    fn series_certainty_inherits_unless_value_explicitly_overrides() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"certainty","data":{"x":{"kind":"time","grid":"month","values":["2022-01","2022-02","2022-03"]},"series":[{"id":"s","certainty":"projection","values":[1,{"value":2},{"value":3,"certainty":"actual"}]}]}}),
        };
        let data = parse(&spec).unwrap();
        assert_eq!(
            data.series[0]
                .values
                .iter()
                .map(|d| d.certainty.as_str())
                .collect::<Vec<_>>(),
            vec!["projection", "projection", "actual"]
        );
        let irregular = Spec {
            raw: json!({"chartspec":1,"id":"irregular-certainty","data":{"x":{"kind":"time","grid":"none"},"series":[{"id":"s","certainty":"estimate","points":[["2022-01-01",1],["2022-01-02",{"value":2,"certainty":"target"}]]}]}}),
        };
        let data = parse(&irregular).unwrap();
        assert_eq!(
            data.irregular.as_ref().unwrap()[0]
                .iter()
                .map(|p| p.datum.certainty.as_str())
                .collect::<Vec<_>>(),
            vec!["estimate", "target"]
        );
    }
    #[test]
    fn as_of_marks_calendar_periods_incomplete_at_exclusive_end() {
        let cases = [
            (
                "year",
                vec!["2021", "2022"],
                "2022-12-30",
                vec![false, true],
            ),
            (
                "quarter",
                vec!["2022-Q1", "2022-Q2"],
                "2022-06-29",
                vec![false, true],
            ),
            (
                "month",
                vec!["2022-04", "2022-05"],
                "2022-04-30",
                vec![false, true],
            ),
            (
                "week",
                vec!["2022-W25", "2022-W26"],
                "2022-06-26",
                vec![false, true],
            ),
            (
                "day",
                vec!["2022-06-25", "2022-06-26"],
                "2022-06-25",
                vec![false, true],
            ),
            (
                "hour",
                vec!["2022-06-25T05", "2022-06-26T05"],
                "2022-06-25",
                vec![false, true],
            ),
            (
                "bin",
                vec!["1960–1969", "1970–1979"],
                "1979-12-30",
                vec![false, true],
            ),
        ];
        for (grid, categories, as_of, want) in cases {
            let spec = Spec {
                raw: json!({"chartspec":1,"id":"asof","data":{"x":{"kind":"time","grid":grid,"values":categories},"as_of":as_of,"series":[{"id":"s","values":[1,2]}]}}),
            };
            let data = parse(&spec).unwrap();
            assert_eq!(
                data.series[0]
                    .values
                    .iter()
                    .map(|d| !d.complete)
                    .collect::<Vec<_>>(),
                want,
                "{grid}"
            );
        }
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"explicit-incomplete","data":{"x":{"kind":"time","grid":"year","values":["2021","2022"]},"as_of":"2023-12-31","series":[{"id":"s","values":[{"value":1,"complete":false},2]}]}}),
        };
        let data = parse(&spec).unwrap();
        assert_eq!(
            data.series[0]
                .values
                .iter()
                .map(|d| d.complete)
                .collect::<Vec<_>>(),
            vec![false, true]
        );
    }
}
