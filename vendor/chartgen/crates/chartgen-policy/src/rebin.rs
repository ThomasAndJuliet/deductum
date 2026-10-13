//! R2-43 (G2.41 [L], 02 §5.4 (f); `timeaxis.rebin`): bins of unequal length re-aggregated to the
//! common larger bin when the shorter bins tile it and the values add up; otherwise refused.
//! The re-aggregation rewrites the source spec's bins, values and comparison values, as the
//! original changes `spec.data` in place, so that every later stage sees the merged bins.
use crate::view::dec;
use chartgen_model::{Decimal, Spec};
use serde_json::{Map, Value, json};

pub enum Rebin {
    /// Not a bin grid.
    NotBins,
    /// The spec to draw (merged, or unchanged) and the decision's text.
    Decided(Spec, String),
    /// (path, message, supply): the refusal of R2-43.
    Refused(&'static str, String, &'static str),
}

fn bin_years(v: &str) -> Option<(i64, i64)> {
    let (a, b) = v.split_once('–').or_else(|| v.split_once('-'))?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn known(v: &Value) -> Option<(Decimal, String, bool)> {
    if v.is_number() {
        return Some((dec(v)?, "actual".into(), true));
    }
    let o = v.as_object()?;
    let value = dec(o.get("value")?)?;
    Some((
        value,
        o.get("certainty")
            .and_then(Value::as_str)
            .unwrap_or("actual")
            .into(),
        o.get("complete").and_then(Value::as_bool).unwrap_or(true),
    ))
}

fn number(x: Decimal) -> Value {
    serde_json::from_str(&x.normalize().to_string()).unwrap_or(Value::Null)
}

fn merged(values: &[Value], groups: &[Vec<usize>]) -> Vec<Value> {
    groups
        .iter()
        .map(|g| {
            if g.len() == 1 {
                return values[g[0]].clone();
            }
            let parts: Vec<_> = g.iter().map(|&k| known(&values[k])).collect();
            if parts.iter().all(Option::is_some) {
                let parts: Vec<_> = parts.into_iter().flatten().collect();
                let sum: Decimal = parts.iter().map(|p| p.0).sum();
                let lower = parts
                    .iter()
                    .map(|p| p.1.clone())
                    .find(|c| c != "actual")
                    .unwrap_or_else(|| "actual".into());
                let complete = parts.iter().all(|p| p.2);
                if lower == "actual" && complete {
                    number(sum)
                } else {
                    let mut o = Map::new();
                    o.insert("value".into(), number(sum));
                    o.insert("certainty".into(), json!(lower));
                    o.insert("complete".into(), json!(complete));
                    Value::Object(o)
                }
            } else {
                json!({"status": "missing", "note": "part of the bin not known"})
            }
        })
        .collect()
}

pub fn rebin(spec: &Spec) -> Rebin {
    if spec.get("/data/x/kind") != "time" || spec.get("/data/x/grid") != "bin" {
        return Rebin::NotBins;
    }
    let x: Vec<String> = spec
        .get("/data/x/values")
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| v.as_str().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default();
    let Some(bins) = x.iter().map(|v| bin_years(v)).collect::<Option<Vec<_>>>() else {
        return Rebin::NotBins;
    };
    let lengths: Vec<i64> = bins.iter().map(|(a, b)| b - a + 1).collect();
    let distinct: std::collections::BTreeSet<i64> = lengths.iter().copied().collect();
    if distinct.len() <= 1 {
        if bins.is_empty() {
            return Rebin::NotBins;
        }
        let n = lengths[0];
        return Rebin::Decided(
            spec.clone(),
            format!(
                "bins of one length, {n} year{} each (G2.41)",
                if n != 1 { "s" } else { "" }
            ),
        );
    }
    let l = *lengths.iter().max().unwrap();
    let full = bins
        .iter()
        .zip(&lengths)
        .find(|(_, n)| **n == l)
        .map(|((a, _), _)| *a)
        .unwrap();
    let mut groups: Option<Vec<Vec<usize>>> = Some(vec![]);
    let mut i = 0;
    while i < bins.len() {
        let mut j = i;
        let mut total = 0;
        while j < bins.len() && total < l {
            total += lengths[j];
            j += 1;
        }
        if total != l || (bins[i].0 - full).rem_euclid(l) != 0 {
            groups = None;
            break;
        }
        groups.as_mut().unwrap().push((i..j).collect());
        i = j;
    }
    let short: Vec<String> = distinct
        .iter()
        .filter(|n| **n != l)
        .map(|n| n.to_string())
        .collect();
    let what = format!(
        "the bins are of unequal length ({l} years, and {}): a shorter bin holds less and fakes a decline",
        short.join(", ")
    );
    let Some(groups) = groups else {
        return Rebin::Refused(
            "data.x.values",
            format!(
                "{what}, and they cannot be re-aggregated into bins of {l} years aligned with the others"
            ),
            "bins of one length in the data",
        );
    };
    if spec.get("/data/measure/additive") != true {
        return Rebin::Refused(
            "data.measure.additive",
            format!(
                "{what}; they can be re-aggregated to {l} years only by adding their values, and whether the values add up is not stated"
            ),
            "data.measure.additive true (the values are totals over their bins), or bins of one length in the data",
        );
    }
    let labels: Vec<String> = groups
        .iter()
        .map(|g| format!("{}–{}", bins[g[0]].0, bins[*g.last().unwrap()].1))
        .collect();
    let mut raw = spec.raw.clone();
    if let Some(series) = raw
        .pointer_mut("/data/series")
        .and_then(Value::as_array_mut)
    {
        for s in series {
            if let Some(values) = s.get("values").and_then(Value::as_array)
                && values.len() == bins.len()
            {
                s["values"] = Value::Array(merged(values, &groups));
            }
        }
    }
    if let Some(comps) = raw
        .pointer_mut("/data/comparisons")
        .and_then(Value::as_array_mut)
    {
        for c in comps {
            if let Some(values) = c.get("values").and_then(Value::as_array)
                && values.len() == bins.len()
            {
                c["values"] = Value::Array(merged(values, &groups));
            }
        }
    }
    let keep: std::collections::BTreeMap<String, String> = groups
        .iter()
        .zip(&labels)
        .filter(|(g, _)| g.len() == 1)
        .map(|(g, lab)| (x[g[0]].clone(), lab.clone()))
        .collect();
    for key in ["roles", "names"] {
        if let Some(map) = raw
            .pointer(&format!("/data/x/{key}"))
            .and_then(Value::as_object)
        {
            let remapped: Map<String, Value> = map
                .iter()
                .filter_map(|(c, r)| keep.get(c).map(|k| (k.clone(), r.clone())))
                .collect();
            raw["data"]["x"][key] = Value::Object(remapped);
        }
    }
    raw["data"]["x"]["values"] = json!(labels);
    let done: Vec<String> = groups
        .iter()
        .zip(&labels)
        .filter(|(g, _)| g.len() > 1)
        .map(|(g, lab)| {
            format!(
                "{} → {lab}",
                g.iter()
                    .map(|k| x[*k].as_str())
                    .collect::<Vec<_>>()
                    .join(" + ")
            )
        })
        .collect();
    Rebin::Decided(
        Spec { raw },
        format!(
            "unequal bins re-aggregated to the common larger bin of {l} years, their values added (measure.additive): {} (G2.41, 02 §5.4 (f))",
            done.join("; ")
        ),
    )
}
