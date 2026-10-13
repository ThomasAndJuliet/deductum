//! Semantic note preparation. Physical host matching and footer layout belong to layout.
use chartgen_model::{Error, PreparedData, Result, Spec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoteTarget {
    Element {
        name: String,
    },
    Category {
        category: String,
    },
    Series {
        series: String,
    },
    Datum {
        series: String,
        category: String,
    },
    Status {
        status: String,
        series: Option<String>,
        category: Option<String>,
    },
    Comment {
        index: usize,
    },
    Stub {
        index: usize,
        series: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedNote {
    /// Original text, including Typst/TeX alternatives where supplied.
    pub text: Value,
    pub display: String,
    pub targets: Vec<NoteTarget>,
    pub auto: bool,
    pub source_index: Option<usize>,
}

fn err(path: impl Into<String>, message: impl Into<String>) -> Error {
    Error::new("CG-4", message).at(path)
}
fn display(v: &Value) -> Option<String> {
    v.as_str()
        .or_else(|| v["alternative"].as_str())
        .or_else(|| v["typst"].as_str())
        .or_else(|| v["tex"].as_str())
        .or_else(|| v["latex"].as_str())
        .map(str::to_owned)
}
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn reference(v: &Value, path: &str) -> Result<NoteTarget> {
    let text = |k: &str| v[k].as_str().map(str::to_owned);
    let series = text("series");
    let category = text("category");
    let fields = ["element", "comment", "stub", "status"]
        .iter()
        .filter(|k| !v[**k].is_null())
        .copied()
        .collect::<Vec<_>>();
    if fields.len() > 1
        || (fields
            .first()
            .is_some_and(|k| ["element", "comment"].contains(k))
            && (series.is_some() || category.is_some()))
        || (fields.first() == Some(&"stub") && category.is_some())
    {
        return Err(err(path, "one place per note reference"));
    }
    match fields.first().copied() {
        Some("element") => Ok(NoteTarget::Element {
            name: text("element").ok_or_else(|| err(path, "element must be text"))?,
        }),
        Some("comment") => Ok(NoteTarget::Comment {
            index: v["comment"]
                .as_u64()
                .ok_or_else(|| err(path, "comment must be a nonnegative index"))?
                as usize,
        }),
        Some("stub") => Ok(NoteTarget::Stub {
            index: v["stub"]
                .as_u64()
                .ok_or_else(|| err(path, "stub must be a nonnegative index"))?
                as usize,
            series,
        }),
        Some("status") => Ok(NoteTarget::Status {
            status: text("status").ok_or_else(|| err(path, "status must be text"))?,
            series,
            category,
        }),
        _ => match (series, category) {
            (Some(series), Some(category)) => Ok(NoteTarget::Datum { series, category }),
            (Some(series), None) => Ok(NoteTarget::Series { series }),
            (None, Some(category)) => Ok(NoteTarget::Category { category }),
            _ => Err(err(
                path,
                "where the mark goes: element, category, series, status, comment or stub",
            )),
        },
    }
}

fn status_label(spec: &Spec, status: &str) -> String {
    spec.get("/labels/notes")
        .get(status)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("({status})"))
}

/// Parse explicit notes, then adopt value notes. Call on parsed source values before transforms.
pub fn prepare(spec: &Spec, data: &PreparedData) -> Result<Vec<PreparedNote>> {
    let mut notes = Vec::new();
    if let Some(source) = spec.get("/notes").as_array() {
        for (j, n) in source.iter().enumerate() {
            let path = format!("notes[{j}]");
            let original = n
                .get("text")
                .ok_or_else(|| err(format!("{path}.text"), "a note needs its words"))?
                .clone();
            let shown = display(&original).ok_or_else(|| {
                err(
                    format!("{path}.text"),
                    "note text must be plain or native text",
                )
            })?;
            if norm(&shown).is_empty() {
                return Err(err(format!("{path}.text"), "a note needs its words"));
            }
            let targets = n["on"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, r)| reference(r, &format!("{path}.on[{i}]")))
                .collect::<Result<Vec<_>>>()?;
            notes.push(PreparedNote {
                text: original,
                display: shown,
                targets,
                auto: false,
                source_index: Some(j),
            });
        }
    }
    // BTreeMap provides stable text groups; the first encounter order below is retained explicitly.
    let mut groups: BTreeMap<String, Vec<NoteTarget>> = BTreeMap::new();
    let mut order = Vec::new();
    // The original adopts the notes of values on a categorical or regular x only
    // (notes.adopt reads `values`); an irregular series' points keep theirs, which the
    // irregular layouts report as not drawn.
    let series: &[chartgen_model::Series] = if data.irregular.is_some() {
        &[]
    } else {
        &data.series
    };
    for s in series {
        for (j, d) in s.values.iter().enumerate() {
            let Some(note) = d.note.as_deref() else {
                continue;
            };
            let content = norm(note);
            if content.is_empty() {
                continue;
            }
            if d.value.is_none()
                && content
                    .trim_matches(['(', ')'])
                    .eq_ignore_ascii_case(status_label(spec, &d.status).trim_matches(['(', ')']))
            {
                continue;
            }
            let Some(category) = data.categories.get(j) else {
                continue;
            };
            if !groups.contains_key(&content) {
                order.push(content.clone());
            }
            groups.entry(content).or_default().push(NoteTarget::Datum {
                series: s.id.clone(),
                category: category.clone(),
            });
        }
    }
    for content in order {
        notes.push(PreparedNote {
            text: Value::String(content.clone()),
            display: content.clone(),
            targets: groups.remove(&content).unwrap_or_default(),
            auto: true,
            source_index: None,
        });
    }
    Ok(notes)
}

/// The source mark sequence, indexed after physical hosts are sorted in reading order.
pub fn mark(kind: &str, index: usize) -> String {
    match kind {
        "numerals" => (index + 1).to_string(),
        "symbols" => {
            const S: &[&str] = &["*", "†", "‡", "§", "‖", "¶"];
            S[index % S.len()].repeat(index / S.len() + 1)
        }
        _ => ((b'a' + (index % 26) as u8) as char)
            .to_string()
            .repeat(index / 26 + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn explicit_and_deduplicated_value_notes_preserve_order() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"notes","title":{"text":"Example"},"notes":[{"text":"Source: annual survey.","on":[]},{"text":"Explains the title","on":[{"element":"title"}]}],"data":{"x":{"kind":"nominal","values":["A","B","C"]},"series":[{"id":"s","values":[{"value":1,"note":" provisional "},{"value":2,"note":"provisional"},{"status":"missing","note":"(missing)"}]}]}}),
        };
        let data = chartgen_data::parse(&spec).unwrap();
        let notes = prepare(&spec, &data).unwrap();
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[0].source_index, Some(0));
        assert_eq!(notes[2].display, "provisional");
        assert_eq!(notes[2].targets.len(), 2);
        assert_eq!(mark("letters", 26), "aa");
        assert_eq!(mark("symbols", 6), "**");
    }
}
