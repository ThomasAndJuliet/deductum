//! Typed reads of the source spec with the original's defaults (`spec.py` dataclasses), and the
//! prepared values as the original's chooser sees them (`spec.data.series[i].values`, which an
//! irregular series does not have: its `points` are read only where the original reads them).
use chartgen_model::{Datum, Decimal, PreparedData, Spec};
use serde_json::Value;
use std::str::FromStr;

pub(crate) const REFERENCE_ROLES: [&str; 2] = ["target", "reference"];

/// A JSON number as the exact decimal written.
pub(crate) fn dec(v: &Value) -> Option<Decimal> {
    if !v.is_number() {
        return None;
    }
    let written = v.to_string();
    Decimal::from_str(&written)
        .or_else(|_| Decimal::from_scientific(&written))
        .ok()
}

/// `f"{x.normalize():f}"`: the shortest exact decimal, no exponent.
pub(crate) fn plain(x: Decimal) -> String {
    let n = x.normalize();
    if n.is_zero() {
        "0".into()
    } else {
        n.to_string()
    }
}

/// `str(Text)`: a spec text is a string or native markup. The original's `{"typst": markup}`
/// reads as its markup; the rewrite's other native forms (`{"latex"|"tex": key, "alternative":
/// words}`) read as their alternative words, else their key, so that a LaTeX-native title counts
/// as text exactly as a Typst one does.
pub fn text(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.into());
    }
    ["typst", "alternative", "latex", "tex"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::to_owned)
}

#[derive(Clone)]
pub(crate) struct SeriesRef<'a> {
    pub index: usize,
    pub id: String,
    /// `str(s.name) if s.name is not None else s.id`
    pub name: String,
    pub role: String,
    /// `s.values` (None for an irregular series, which has points)
    pub values: Option<&'a [Datum]>,
}

pub(crate) struct View<'a> {
    pub spec: &'a Spec,
    pub data: &'a PreparedData,
}

impl<'a> View<'a> {
    pub fn new(spec: &'a Spec, data: &'a PreparedData) -> Self {
        Self { spec, data }
    }
    pub fn get(&self, pointer: &str) -> &'a Value {
        self.spec.get(pointer)
    }
    pub fn str_or(&self, pointer: &str, default: &'a str) -> &'a str {
        self.spec.get(pointer).as_str().unwrap_or(default)
    }
    pub fn opt_str(&self, pointer: &str) -> Option<&'a str> {
        self.spec.get(pointer).as_str()
    }
    pub fn flag(&self, pointer: &str) -> bool {
        self.spec.get(pointer).as_bool().unwrap_or(false)
    }
    pub fn opt_bool(&self, pointer: &str) -> Option<bool> {
        self.spec.get(pointer).as_bool()
    }
    pub fn purpose(&self) -> &'a str {
        self.str_or("/purpose", "communication")
    }
    pub fn job(&self) -> Option<&'a str> {
        let job = self.get("/job");
        if job.is_null() {
            None
        } else {
            job.get("kind").and_then(Value::as_str)
        }
    }
    pub fn insight_important(&self) -> Option<String> {
        self.opt_str("/insight/important")
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }
    pub fn message(&self) -> Option<&'a str> {
        self.opt_str("/facts/message")
    }
    pub fn reason(&self, path: &str) -> Option<String> {
        self.get("/reasons")
            .get(path)
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
    pub fn x_kind(&self) -> &'a str {
        self.str_or("/data/x/kind", "nominal")
    }
    pub fn grid(&self) -> Option<&'a str> {
        self.opt_str("/data/x/grid")
    }
    pub fn x_values(&self) -> Vec<String> {
        self.get("/data/x/values")
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or("").to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn x_role(&self, category: &str) -> Option<&'a str> {
        self.get("/data/x/roles")
            .get(category)
            .and_then(Value::as_str)
    }
    pub fn parts(&self) -> Option<&'a Value> {
        let p = self.get("/data/parts");
        (!p.is_null()).then_some(p)
    }
    /// The measure as the chooser reads it: after a derived measure replaced it (transform).
    pub fn measure(&self) -> &'a Value {
        &self.data.measure
    }
    pub fn measure_str(&self, key: &str) -> Option<&'a str> {
        self.data.measure.get(key).and_then(Value::as_str)
    }
    pub fn measure_dec(&self, key: &str) -> Option<Decimal> {
        self.data.measure.get(key).and_then(dec)
    }
    pub fn series(&self) -> Vec<SeriesRef<'a>> {
        let irregular = self.data.irregular.is_some();
        let raw = self.get("/data/series").as_array();
        let mut out = vec![];
        for (i, s) in self.data.series.iter().enumerate() {
            let r = raw.and_then(|a| a.get(i)).unwrap_or(&Value::Null);
            let given_name = text(&r["name"]);
            out.push(SeriesRef {
                index: i,
                id: s.id.clone(),
                name: given_name.unwrap_or_else(|| s.id.clone()),
                role: r["role"].as_str().unwrap_or("regular").into(),
                values: (!irregular).then_some(s.values.as_slice()),
            });
        }
        out
    }
    pub fn drawn(&self) -> Vec<SeriesRef<'a>> {
        self.series()
            .into_iter()
            .filter(|s| !REFERENCE_ROLES.contains(&s.role.as_str()))
            .collect()
    }
    /// The known values of every series (`v.value for s in series for v in (s.values or []) if v.known`).
    pub fn known(&self) -> Vec<Decimal> {
        self.series()
            .iter()
            .flat_map(|s| s.values.unwrap_or(&[]).iter().filter_map(|d| d.value))
            .collect()
    }
    pub fn overrides(&self) -> Vec<(String, String)> {
        self.get("/overrides")
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| {
                        Some((o["rule"].as_str()?.into(), o["reason"].as_str()?.into()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn chart_type(&self) -> &'a str {
        self.str_or("/chart/type", "auto")
    }
}

/// The original's `Value.known` for a prepared datum.
pub(crate) fn known(d: &Datum) -> bool {
    d.value.is_some()
}
