//! The findings and refusals of the arrangement of several panels (`layout/multiples._arrangement`):
//! a departure from the procedure's arrangement (RP3-3, guided), facts.compare not given (RP3-4),
//! two measures in one plot (R2-13 on one scale, R2-14 on two axes), and the arrangements the
//! original does not draw (CG-3). The arrangement's own decision (RP3-3) and the grid belong to
//! chartgen-panels.
use crate::rules::{guided, info, refusal};
use crate::view::{View, text};
use chartgen_model::Report;
use serde_json::Value;

const MEASURE: &str = "measure";
const MAX_COLUMNS: usize = 6;

#[derive(Clone, PartialEq, Eq, Default)]
struct Arr {
    rows: Option<String>,
    columns: Option<String>,
    within: Vec<String>,
}
impl Arr {
    fn text(&self, name: &dyn Fn(&str) -> String) -> String {
        let mut parts = vec![];
        if let Some(r) = &self.rows {
            parts.push(format!("rows by {}", name(r)));
        }
        if let Some(c) = &self.columns {
            parts.push(format!("columns by {}", name(c)));
        }
        if !self.within.is_empty() {
            parts.push(format!(
                "series in each panel by {}",
                self.within
                    .iter()
                    .map(|w| name(w))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ));
        }
        if parts.is_empty() {
            "one panel".into()
        } else {
            parts.join(", ")
        }
    }
}

/// The outcome: None to go on, or the status that stops the chart (refused, unsupported).
pub(crate) fn arrangement(v: &View, report: &mut Report) -> Option<&'static str> {
    let dims = v.get("/data/dims").as_array().cloned().unwrap_or_default();
    let measures = v
        .get("/data/measures")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let name = |id: &str| -> String {
        if id == MEASURE {
            return "measure".into();
        }
        dims.iter()
            .find(|d| d["id"] == id)
            .and_then(|d| text(&d["name"]))
            .unwrap_or_else(|| id.to_owned())
    };
    let size = |id: &str| -> usize {
        if id == MEASURE {
            return measures.len();
        }
        dims.iter()
            .find(|d| d["id"] == id)
            .and_then(|d| d["values"].as_array())
            .map_or(0, Vec::len)
    };
    let time = v.x_kind() == "time";
    if !dims.is_empty() && !measures.is_empty() {
        info(
            report,
            "CG-3",
            "small multiples of a merged chart (dims and several measures) are a later stage",
            Some("data.measures"),
        );
        return Some("unsupported");
    }
    let ids: Vec<String> = dims
        .iter()
        .filter_map(|d| d["id"].as_str().map(str::to_owned))
        .collect();
    let compare: Vec<String> = v
        .get("/facts/compare")
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    // _auto
    let (auto, why) = if !measures.is_empty() {
        if time {
            (
                Arr {
                    rows: Some(MEASURE.into()),
                    ..Arr::default()
                },
                "series in different measures: one panel per measure, stacked on one time axis drawn at the foot, each with its own scale (G2.31, G2.32)".to_owned(),
            )
        } else {
            (
                Arr {
                    columns: Some(MEASURE.into()),
                    ..Arr::default()
                },
                "series in different measures: one panel per measure, side by side on the shared categories, each with its own scale (G1.56, G2.32)".to_owned(),
            )
        }
    } else {
        let within: Vec<String> = ids
            .iter()
            .filter(|c| compare.contains(c))
            .cloned()
            .collect();
        let facets: Vec<String> = ids
            .iter()
            .filter(|c| !within.contains(c))
            .cloned()
            .collect();
        let arr = match facets.len() {
            0 => Arr {
                within: within.clone(),
                ..Arr::default()
            },
            1 => {
                let f = facets[0].clone();
                if size(&f) <= MAX_COLUMNS {
                    Arr {
                        columns: Some(f),
                        within: within.clone(),
                        ..Arr::default()
                    }
                } else {
                    Arr {
                        rows: Some(f),
                        within: within.clone(),
                        ..Arr::default()
                    }
                }
            }
            _ => {
                let (a, b) = (facets[0].clone(), facets[1].clone());
                let (big, small) = if size(&a) >= size(&b) { (a, b) } else { (b, a) };
                if size(&big) <= MAX_COLUMNS {
                    Arr {
                        rows: Some(small),
                        columns: Some(big),
                        within: within.clone(),
                    }
                } else {
                    Arr {
                        rows: Some(big),
                        columns: Some(small),
                        within: within.clone(),
                    }
                }
            }
        };
        let mut why = if within.is_empty() {
            "no comparison within a panel is stated (facts.compare): one series per panel, the most panels and the least clutter (13-15 DP-1)".to_owned()
        } else {
            format!(
                "the insight compares {} directly (facts.compare): drawn as series in each panel (RP3-4, G1.19){}",
                within.join(" and "),
                if facets.is_empty() {
                    "; every dim within one chart".to_owned()
                } else {
                    format!("; the panels facet on {}", facets.join(" and "))
                }
            )
        };
        if facets.len() == 1 && size(&facets[0]) > MAX_COLUMNS {
            why += &format!(
                "; {} panels are stacked in rows (more than {MAX_COLUMNS} across would be too narrow)",
                size(&facets[0])
            );
        }
        (arr, why)
    };
    let opt = v.get("/panels/arrangement");
    let arr = if let Some(o) = opt.as_object() {
        Arr {
            rows: o.get("rows").and_then(Value::as_str).map(str::to_owned),
            columns: o.get("columns").and_then(Value::as_str).map(str::to_owned),
            within: o
                .get("within")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|w| w.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        }
    } else {
        if !dims.is_empty() && compare.is_empty() {
            info(
                report,
                "RP3-4",
                "facts.compare not given: say which dims the insight compares directly, so that their series share a panel (G1.19)",
                Some("facts.compare"),
            );
        }
        auto.clone()
    };
    if opt.is_object() && arr != auto {
        guided(
            report,
            v,
            "RP3-3",
            "panels.arrangement",
            &arr.text(&name),
            &auto.text(&name),
            &why,
        );
    }
    if arr.within.iter().any(|w| w == MEASURE) && measures.len() > 1 {
        if v.str_or("/panels/scale", "auto") == "shared" {
            refusal(
                report,
                v,
                "R2-13",
                "different measures on one value scale invite comparisons across units that mean nothing",
                Some("panels.arrangement"),
                Some("arrangement auto: a merged chart, one panel and one scale per measure"),
            );
        } else {
            refusal(
                report,
                v,
                "R2-14",
                "two variables against two value axes in one plot suggest false conclusions: similar volatility, one line overtaking or rising faster than the other",
                Some("panels.arrangement"),
                Some(
                    "arrangement auto: a merged chart, one panel per measure on a shared axis (actual values); or an index chart (patterns of change)",
                ),
            );
        }
        let overridden = v
            .overrides()
            .iter()
            .any(|(r, _)| r == "R2-13" || r == "R2-14");
        return Some(if overridden { "unsupported" } else { "refused" });
    }
    let faceted = arr.rows.is_some() as usize + arr.columns.is_some() as usize + arr.within.len();
    if faceted > 0 && ids.iter().filter(|f| !arr.within.contains(f)).count() > 2 {
        info(
            report,
            "CG-3",
            "small multiples faceted on more than two dims are a later stage",
            Some("data.dims"),
        );
        return Some("unsupported");
    }
    if !measures.is_empty()
        && arr.rows.as_deref() != Some(MEASURE)
        && arr.columns.as_deref() != Some(MEASURE)
    {
        info(
            report,
            "CG-3",
            "a merged chart's measures go to the rows or the columns",
            Some("panels.arrangement"),
        );
        return Some("unsupported");
    }
    None
}
