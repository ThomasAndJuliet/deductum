//! Partition multiple and merged charts into semantic panels before physical layout.
use chartgen_model::{Decision, Error, PreparedData, Result, Spec};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Arrangement {
    pub rows: Option<String>,
    pub columns: Option<String>,
    pub within: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Panel {
    pub row: usize,
    pub column: usize,
    pub row_value: Option<String>,
    pub column_value: Option<String>,
    pub title: Value,
    pub series_indices: Vec<usize>,
    pub spec: Spec,
    pub data: PreparedData,
    pub kind: String,
    pub arrangement: Arrangement,
    pub scale_mode: String,
    pub rows: usize,
    pub columns: usize,
}
fn err(rule: &str, path: &str, message: impl Into<String>) -> Error {
    Error::new(rule, message).at(path)
}
fn ids(items: &[Value], path: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, v) in items.iter().enumerate() {
        let id = v["id"]
            .as_str()
            .ok_or_else(|| err("CG-4", &format!("{path}[{i}].id"), "id required"))?;
        if !seen.insert(id) {
            return Err(err("CG-4", path, "ids must be distinct"));
        }
        out.push(id.into());
    }
    Ok(out)
}
fn string_values(v: &Value, path: &str) -> Result<Vec<String>> {
    v.as_array()
        .ok_or_else(|| err("CG-4", path, "values required"))?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_owned)
                .ok_or_else(|| err("CG-4", path, "value must be text"))
        })
        .collect()
}
/// The messages whose chart is one table, never panels (the original's choose.TABLE_MESSAGES).
const TABLE_MESSAGES: &[&str] = &["lookup_only", "export_only", "not_comparable"];

/// Whether the spec asks for several panels: dims to facet on, or several measures; a table
/// of several measures (asked for, or chosen for a table's message) is one table, not panels.
pub fn applies(spec: &Spec) -> bool {
    let kind = spec.get("/chart/type").as_str().unwrap_or("auto");
    if kind == "table"
        || (kind == "auto"
            && spec
                .get("/facts/message")
                .as_str()
                .is_some_and(|m| TABLE_MESSAGES.contains(&m)))
    {
        return false;
    }
    spec.get("/data/dims")
        .as_array()
        .is_some_and(|a| !a.is_empty())
        || spec
            .get("/data/measures")
            .as_array()
            .is_some_and(|a| !a.is_empty())
}
fn arrangement(spec: &Spec, dims: &[Value], measures: &[Value]) -> Result<Arrangement> {
    let opt = spec.get("/panels/arrangement");
    if opt.is_object() {
        return Ok(Arrangement {
            rows: opt["rows"].as_str().map(str::to_owned),
            columns: opt["columns"].as_str().map(str::to_owned),
            within: opt["within"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        });
    }
    if !measures.is_empty() {
        return Ok(if spec.get("/data/x/kind") == "time" {
            Arrangement {
                rows: Some("measure".into()),
                columns: None,
                within: vec![],
            }
        } else {
            Arrangement {
                rows: None,
                columns: Some("measure".into()),
                within: vec![],
            }
        });
    }
    let compare = spec
        .get("/facts/compare")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let within = dims
        .iter()
        .filter_map(|d| d["id"].as_str())
        .filter(|id| compare.contains(id))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let facets = dims
        .iter()
        .filter_map(|d| {
            d["id"]
                .as_str()
                .map(|id| (id, d["values"].as_array().map_or(0, Vec::len)))
        })
        .filter(|(id, _)| !compare.contains(id))
        .collect::<Vec<_>>();
    let (rows, columns) = match facets.as_slice() {
        [] => (None, None),
        [(id, n)] => {
            if *n <= 6 {
                (None, Some((*id).into()))
            } else {
                (Some((*id).into()), None)
            }
        }
        [(a, na), (b, nb)] => {
            let (big, big_n, small) = if na >= nb { (a, na, b) } else { (b, nb, a) };
            if *big_n <= 6 {
                (Some((*small).into()), Some((*big).into()))
            } else {
                (Some((*big).into()), Some((*small).into()))
            }
        }
        _ => {
            return Err(err(
                "CG-3",
                "data.dims",
                "more than two faceted dimensions are not drawn",
            ));
        }
    };
    Ok(Arrangement {
        rows,
        columns,
        within,
    })
}
fn axis_values(
    id: Option<&str>,
    dims: &[Value],
    measures: &[Value],
) -> Result<Vec<Option<String>>> {
    let Some(id) = id else { return Ok(vec![None]) };
    if id == "measure" {
        return Ok(measures
            .iter()
            .filter_map(|m| m["id"].as_str().map(|s| Some(s.to_owned())))
            .collect());
    }
    let d = dims
        .iter()
        .find(|d| d["id"] == id)
        .ok_or_else(|| err("CG-4", "panels.arrangement", "unknown dimension"))?;
    Ok(string_values(&d["values"], "data.dims.values")?
        .into_iter()
        .map(Some)
        .collect())
}
fn panel_order(
    spec: &Spec,
    data: &PreparedData,
    measures: &[Value],
    fallback: &Decision,
) -> Vec<usize> {
    let n = data.categories.len();
    if data.x_kind != "nominal" || spec.get("/chart/sort") == "given" {
        return (0..n).collect();
    }
    if !measures.is_empty() {
        let first = measures
            .first()
            .and_then(|m| m["id"].as_str())
            .unwrap_or("");
        let sources = spec.get("/data/series").as_array();
        let indices = (0..data.series.len())
            .filter(|i| {
                sources
                    .and_then(|a| a.get(*i))
                    .is_some_and(|s| s["measure"] == first)
            })
            .collect::<Vec<_>>();
        return sorted_order(spec, data, &indices);
    }
    if fallback.order.len() == n {
        return sorted_order(spec, data, &(0..data.series.len()).collect::<Vec<_>>());
    }
    (0..n).collect()
}
fn sorted_order(spec: &Spec, data: &PreparedData, series: &[usize]) -> Vec<usize> {
    let mut normal = Vec::new();
    let mut tail = Vec::new();
    let mut unknown = Vec::new();
    for j in 0..data.categories.len() {
        let vals = series
            .iter()
            .filter_map(|&i| {
                data.series
                    .get(i)
                    .and_then(|s| s.values.get(j))
                    .and_then(|d| d.value)
            })
            .collect::<Vec<_>>();
        if vals.is_empty() {
            unknown.push(j);
            continue;
        }
        let role = spec
            .get("/data/x/roles")
            .get(&data.categories[j])
            .and_then(Value::as_str)
            .unwrap_or("");
        let rank = match role {
            "all_others" => 1,
            "missing" => 2,
            "total" => 3,
            _ => 0,
        };
        let total = vals.into_iter().sum::<chartgen_model::Decimal>();
        if rank == 0 {
            normal.push((j, total));
        } else {
            tail.push((j, rank));
        }
    }
    let asc = spec.get("/chart/sort") == "asc";
    normal.sort_by(|a, b| if asc { a.1.cmp(&b.1) } else { b.1.cmp(&a.1) }.then(a.0.cmp(&b.0)));
    tail.sort_by_key(|x| x.1);
    normal
        .into_iter()
        .map(|x| x.0)
        .chain(tail.into_iter().map(|x| x.0))
        .chain(unknown)
        .collect()
}
/// Return every panel, including empty cells, in row-major order. Each panel keeps its own measure.
pub fn prepare(spec: &Spec, data: &PreparedData, decision: &Decision) -> Result<Vec<Panel>> {
    let dims = spec
        .get("/data/dims")
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let measures = spec
        .get("/data/measures")
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if dims.is_empty() && measures.is_empty() {
        return Ok(vec![]);
    }
    if !dims.is_empty() && !measures.is_empty() {
        return Err(err(
            "CG-3",
            "data.measures",
            "small multiples of merged measures are not drawn",
        ));
    }
    let dids = ids(dims, "data.dims")?;
    let mids = ids(measures, "data.measures")?;
    if dids.iter().any(|d| d == "measure") {
        return Err(err(
            "CG-4",
            "data.dims",
            "measure is a reserved dimension id",
        ));
    }
    let arr = arrangement(spec, dims, measures)?;
    let names = dids
        .iter()
        .cloned()
        .chain(if measures.is_empty() {
            None
        } else {
            Some("measure".into())
        })
        .collect::<BTreeSet<_>>();
    let used = arr
        .rows
        .iter()
        .chain(arr.columns.iter())
        .chain(arr.within.iter())
        .cloned()
        .collect::<Vec<_>>();
    if used.iter().collect::<BTreeSet<_>>().len() != used.len()
        || used.iter().any(|u| !names.contains(u))
        || used.iter().collect::<BTreeSet<_>>() != names.iter().collect::<BTreeSet<_>>()
    {
        return Err(err(
            "CG-4",
            "panels.arrangement",
            "every dimension must occur exactly once in rows, columns, or within",
        ));
    }
    if arr.within.iter().any(|w| w == "measure") {
        return Err(err(
            if spec.get("/panels/scale") == "shared" {
                "R2-13"
            } else {
                "R2-14"
            },
            "panels.arrangement",
            "different measures require separate panels and scales",
        ));
    }
    let scale = spec.string("/panels/scale", "auto");
    if !measures.is_empty() && scale == "shared" {
        return Err(err(
            "R2-13",
            "panels.scale",
            "merged measures require separate value scales",
        ));
    }
    let scale_mode = if !measures.is_empty() {
        "independent"
    } else if scale == "auto" {
        if spec.get("/facts/compare_shapes") == true {
            "independent"
        } else {
            "shared"
        }
    } else {
        scale.as_str()
    };
    let row_values = axis_values(arr.rows.as_deref(), dims, measures)?;
    let col_values = axis_values(arr.columns.as_deref(), dims, measures)?;
    let sources = spec
        .get("/data/series")
        .as_array()
        .ok_or_else(|| err("CG-4", "data.series", "series required"))?;
    if sources.len() != data.series.len() {
        return Err(err(
            "CG-4",
            "data.series",
            "prepared and source series differ",
        ));
    }
    for (i, s) in sources.iter().enumerate() {
        if !measures.is_empty() && !mids.iter().any(|id| s["measure"] == *id) {
            return Err(err(
                "CG-4",
                &format!("data.series[{i}].measure"),
                "series must name a measure",
            ));
        }
        if !dims.is_empty() {
            let keys = s["keys"].as_object().ok_or_else(|| {
                err(
                    "CG-4",
                    &format!("data.series[{i}].keys"),
                    "series needs dimension keys",
                )
            })?;
            if keys.len() != dims.len() || dids.iter().any(|id| !keys.contains_key(id)) {
                return Err(err(
                    "CG-4",
                    &format!("data.series[{i}].keys"),
                    "a value for each dimension is required",
                ));
            }
            for d in dims {
                let id = d["id"].as_str().unwrap_or("");
                if !d["values"]
                    .as_array()
                    .is_some_and(|v| v.contains(&keys[id]))
                {
                    return Err(err(
                        "CG-4",
                        &format!("data.series[{i}].keys.{id}"),
                        "unknown dimension value",
                    ));
                }
            }
        }
    }
    if !dims.is_empty() {
        let mut combinations = BTreeSet::new();
        for (i, s) in sources.iter().enumerate() {
            let key = dids
                .iter()
                .map(|d| s["keys"][d].as_str().unwrap_or(""))
                .collect::<Vec<_>>();
            if !combinations.insert(key) {
                return Err(err(
                    "CG-4",
                    &format!("data.series[{i}].keys"),
                    "dimension keys repeat another series",
                ));
            }
        }
    }
    let order = panel_order(spec, data, measures, decision);
    let mut panels = Vec::new();
    for (row, rv) in row_values.iter().enumerate() {
        for (column, cv) in col_values.iter().enumerate() {
            let matches = |s: &Value, dim: Option<&str>, val: &Option<String>| -> bool {
                match (dim, val) {
                    (None, _) => true,
                    (Some("measure"), Some(v)) => s["measure"] == *v,
                    (Some(d), Some(v)) => s["keys"][d] == *v,
                    _ => false,
                }
            };
            let indices = (0..sources.len())
                .filter(|&i| {
                    matches(&sources[i], arr.rows.as_deref(), rv)
                        && matches(&sources[i], arr.columns.as_deref(), cv)
                })
                .collect::<Vec<_>>();
            let mid = if arr.rows.as_deref() == Some("measure") {
                rv.as_deref()
            } else if arr.columns.as_deref() == Some("measure") {
                cv.as_deref()
            } else {
                None
            };
            let measure = mid
                .and_then(|id| measures.iter().find(|m| m["id"] == id))
                .cloned()
                .unwrap_or_else(|| data.measure.clone());
            let heading = mid.and_then(|id| {
                measures.iter().find(|m| m["id"] == id).map(|m| {
                    if !m["heading"].is_null() {
                        m["heading"].clone()
                    } else if !m["name"].is_null() {
                        Value::String(m["name"].as_str().unwrap_or(id).to_owned())
                    } else {
                        Value::String(id.into())
                    }
                })
            });
            let title = heading.unwrap_or_else(|| {
                Value::String(
                    rv.as_ref()
                        .into_iter()
                        .chain(cv.as_ref())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            });
            let mut raw = spec.raw.clone();
            let mut selected = Vec::new();
            let mut series = Vec::new();
            let mut irregular = Vec::new();
            for &i in &indices {
                let mut original = sources[i].clone();
                if !arr.within.is_empty() {
                    let label = arr
                        .within
                        .iter()
                        .filter_map(|w| original["keys"][w].as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    original["name"] = Value::String(label.clone());
                }
                if let Some(values) = original["values"].as_array().cloned() {
                    original["values"] = Value::Array(
                        order
                            .iter()
                            .filter_map(|&j| values.get(j).cloned())
                            .collect(),
                    );
                }
                if let Some(o) = original.as_object_mut() {
                    o.remove("keys");
                    o.remove("measure");
                }
                selected.push(original);
                let mut s = data.series[i].clone();
                s.values = order
                    .iter()
                    .filter_map(|&j| s.values.get(j).cloned())
                    .collect();
                if !arr.within.is_empty() {
                    s.name = arr
                        .within
                        .iter()
                        .filter_map(|w| sources[i]["keys"][w].as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                }
                series.push(s);
                if let Some(points) = data.irregular.as_ref().and_then(|a| a.get(i)) {
                    irregular.push(points.clone());
                }
            }
            raw["data"]["series"] = Value::Array(selected);
            if let Some(comparisons) = raw
                .get_mut("data")
                .and_then(|d| d.get_mut("comparisons"))
                .and_then(Value::as_array_mut)
            {
                for c in comparisons {
                    if let Some(values) = c["values"].as_array().cloned() {
                        if values.len() == order.len() {
                            c["values"] = Value::Array(
                                order
                                    .iter()
                                    .filter_map(|&j| values.get(j).cloned())
                                    .collect(),
                            );
                        }
                    }
                }
            }
            raw["data"]["measure"] = measure.clone();
            raw["data"]["dims"] = json!([]);
            raw["data"]["measures"] = json!([]);
            raw["data"]["x"]["values"] = Value::Array(
                order
                    .iter()
                    .filter_map(|&j| data.categories.get(j).map(|s| Value::String(s.clone())))
                    .collect(),
            );
            raw["panels"] = json!({});
            raw["facts"]["compare"] = json!([]);
            raw["facts"]["compare_shapes"] = json!(false);
            raw["chart"]["sort"] = json!("given");
            raw["chart"]["sort_by"] = Value::Null;
            raw["inset"] = json!({"kind":"none"});
            raw["comments"] = json!([]);
            raw["notes"] = json!([]);
            let selected_ids = indices
                .iter()
                .map(|&i| data.series[i].id.as_str())
                .collect::<BTreeSet<_>>();
            let panel_values = rv
                .iter()
                .chain(cv.iter())
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let keep = |target: &Value| {
                target["series"]
                    .as_str()
                    .is_none_or(|id| selected_ids.contains(id))
                    && target["panel"]
                        .as_str()
                        .is_none_or(|id| panel_values.contains(id))
            };
            if let Some(targets) = raw
                .get_mut("insight")
                .and_then(|v| v.get_mut("targets"))
                .and_then(Value::as_array_mut)
            {
                targets.retain(|t| keep(t));
                for t in targets {
                    if let Some(o) = t.as_object_mut() {
                        o.remove("panel");
                    }
                }
            }
            if let Some(highlights) = raw.get_mut("highlight").and_then(Value::as_array_mut) {
                for h in highlights.iter_mut() {
                    if let Some(targets) = h.get_mut("targets").and_then(Value::as_array_mut) {
                        targets.retain(|t| keep(t));
                        for t in targets {
                            if let Some(o) = t.as_object_mut() {
                                o.remove("panel");
                            }
                        }
                    }
                }
                highlights.retain(|h| h["targets"].as_array().is_some_and(|a| !a.is_empty()));
            }
            let comparisons = data
                .comparisons
                .iter()
                .cloned()
                .map(|mut c| {
                    if c.values.len() == order.len() {
                        c.values = order
                            .iter()
                            .filter_map(|&j| c.values.get(j).cloned())
                            .collect();
                    }
                    c
                })
                .collect();
            let pdata = PreparedData {
                categories: order
                    .iter()
                    .filter_map(|&j| data.categories.get(j).cloned())
                    .collect(),
                x_kind: data.x_kind.clone(),
                series,
                measure,
                irregular: if data.irregular.is_some() {
                    Some(irregular)
                } else {
                    None
                },
                comparisons,
            };
            panels.push(Panel {
                row,
                column,
                row_value: rv.clone(),
                column_value: cv.clone(),
                title,
                series_indices: indices,
                spec: Spec { raw },
                data: pdata,
                kind: if measures.is_empty() {
                    "multiples"
                } else {
                    "merged"
                }
                .into(),
                arrangement: arr.clone(),
                scale_mode: scale_mode.into(),
                rows: row_values.len(),
                columns: col_values.len(),
            });
        }
    }
    Ok(panels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chartgen_model::{ChartType, Orientation};
    #[test]
    fn source_panel_partitions() {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/conformance/examples/specs"
        );
        for (id, rows, cols, kind) in [
            ("p2-60-donations-small-multiples", 3, 5, "multiples"),
            ("p2-65-call-centres-merged", 1, 2, "merged"),
            ("p2-66-sales-and-satisfaction", 2, 1, "merged"),
        ] {
            let path = format!("{root}/{id}.json");
            let fixture: Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let spec = Spec {
                raw: fixture.get("base").unwrap_or(&fixture).clone(),
            };
            let data = chartgen_data::parse(&spec).unwrap();
            let decision = Decision {
                chart_type: Some(ChartType::Bar),
                orientation: Orientation::Vertical,
                order: (0..data.categories.len()).collect(),
                findings: vec![],
                decisions: vec![],
                status: "drawn".into(),
            };
            let ps = prepare(&spec, &data, &decision).unwrap();
            assert_eq!(
                (ps[0].rows, ps[0].columns, ps[0].kind.as_str()),
                (rows, cols, kind),
                "{id}"
            );
            assert_eq!(ps.len(), rows * cols);
            if kind == "merged" {
                assert!(ps.iter().all(|p| p.scale_mode == "independent"));
            }
            assert!(ps.iter().all(|p| *p.spec.get("/data/dims") == json!([])
                && *p.spec.get("/data/measures") == json!([])));
        }
    }
    #[test]
    fn all_original_panel_specs_partition_every_series_once() {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/conformance/examples/specs"
        );
        for stem in [
            "p2-60-donations-small-multiples",
            "p2-61-donations-by-region",
            "p2-62-donations-by-method",
            "p2-63-donations-bar-grid",
            "p2-64-orders-by-province",
            "p2-65-call-centres-merged",
            "p2-66-sales-and-satisfaction",
            "p2-67-regional-expenses",
            "p2-68-donor-measures",
            "p2-69-expenses-vs-budget",
            "p2-70-accidents-count-and-rate",
        ] {
            let fixture: Value = serde_json::from_str(
                &std::fs::read_to_string(format!("{root}/{stem}.json")).unwrap(),
            )
            .unwrap();
            let spec = Spec {
                raw: fixture.get("base").unwrap_or(&fixture).clone(),
            };
            let data = chartgen_data::parse(&spec).unwrap();
            let decision = Decision {
                chart_type: Some(ChartType::Bar),
                orientation: Orientation::Vertical,
                order: (0..data.categories.len()).collect(),
                findings: vec![],
                decisions: vec![],
                status: "drawn".into(),
            };
            let panels = prepare(&spec, &data, &decision).unwrap();
            assert!(!panels.is_empty(), "{stem}");
            let mut found = panels
                .iter()
                .flat_map(|p| p.series_indices.iter().copied())
                .collect::<Vec<_>>();
            found.sort();
            assert_eq!(found, (0..data.series.len()).collect::<Vec<_>>(), "{stem}");
            assert!(
                panels
                    .iter()
                    .all(|p| p.data.measure == p.spec.get("/data/measure").clone()),
                "{stem}"
            );
            for p in panels.iter().filter(|p| !p.series_indices.is_empty()) {
                let parsed = chartgen_data::parse(&p.spec);
                assert!(
                    parsed.is_ok(),
                    "{stem} panel ({},{}) fails independent parse: {:?}",
                    p.row,
                    p.column,
                    parsed.err()
                );
            }
        }
    }
    #[test]
    fn comparison_dimension_changes_grid_and_merged_shared_scale_refuses() {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/conformance/examples/specs"
        );
        let fixture: Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/p2-60-donations-small-multiples.json"))
                .unwrap(),
        )
        .unwrap();
        let mut raw = fixture["base"].clone();
        raw["facts"]["compare"] = json!(["method"]);
        let spec = Spec { raw };
        let data = chartgen_data::parse(&spec).unwrap();
        let d = Decision {
            chart_type: Some(ChartType::Line),
            orientation: Orientation::Vertical,
            order: (0..data.categories.len()).collect(),
            findings: vec![],
            decisions: vec![],
            status: "drawn".into(),
        };
        let ps = prepare(&spec, &data, &d).unwrap();
        assert_eq!(
            (ps[0].rows, ps[0].columns, ps[0].series_indices.len()),
            (1, 5, 3)
        );
        assert_eq!(ps[0].arrangement.within, vec!["method"]);
        let fixture: Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/p2-65-call-centres-merged.json")).unwrap(),
        )
        .unwrap();
        let mut raw = fixture["base"].clone();
        raw["panels"] = json!({"scale":"shared"});
        let spec = Spec { raw };
        let data = chartgen_data::parse(&spec).unwrap();
        assert_eq!(prepare(&spec, &data, &d).unwrap_err().kind, "R2-13");
    }
}
