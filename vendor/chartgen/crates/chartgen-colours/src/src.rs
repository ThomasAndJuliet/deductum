//! Read-only access to the original spec fields that colour planning consults.
use chartgen_model::{Decimal, PreparedData, Spec};
use serde_json::Value;

/// The special parts placed last, in this order (pie.TAIL, bars.TAIL_ROLES).
pub const TAIL: [&str; 2] = ["all_others", "missing"];
/// Series drawn as flat markers across bars (choose.REFERENCE_ROLES).
pub const REF_ROLES: [&str; 2] = ["target", "reference"];

/// The display text of a spec string (`str(Text)` in the original: the plain text, else markup).
pub fn display(v: &Value) -> Option<&str> {
    v.as_str()
        .or_else(|| v["typst"].as_str())
        .or_else(|| v["alternative"].as_str())
        .or_else(|| v["tex"].as_str())
        .or_else(|| v["latex"].as_str())
}

pub fn is_regular(role: Option<&str>) -> bool {
    matches!(role, None | Some("regular"))
}

/// bars.ROLE_CLASS / lines.ROLE_CLASS: a special series' or category's reserved class.
pub fn role_class(role: &str, line: bool) -> Option<&'static str> {
    Some(match role {
        "all_others" => "all-others",
        "total" => "total",
        "average" => "average",
        "missing" => "missing",
        "reference" | "target" if line => "reference",
        _ => return None,
    })
}

pub struct Src<'a> {
    pub spec: &'a Spec,
    pub data: &'a PreparedData,
    series: Vec<&'a Value>,
}

impl<'a> Src<'a> {
    pub fn new(spec: &'a Spec, data: &'a PreparedData) -> Self {
        let raw = spec.raw["data"]["series"].as_array();
        let series = data
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| {
                raw.and_then(|a| {
                    a.get(i)
                        .filter(|v| v["id"].as_str() == Some(s.id.as_str()))
                        .or_else(|| a.iter().find(|v| v["id"].as_str() == Some(s.id.as_str())))
                })
                .unwrap_or(&Value::Null)
            })
            .collect();
        Self { spec, data, series }
    }
    pub fn get(&self, pointer: &str) -> &'a Value {
        self.spec.get(pointer)
    }
    pub fn string(&self, pointer: &str, default: &'a str) -> &'a str {
        self.get(pointer).as_str().unwrap_or(default)
    }
    pub fn n_series(&self) -> usize {
        self.data.series.len()
    }
    pub fn series_raw(&self, i: usize) -> &'a Value {
        self.series[i]
    }
    /// The source index of a prepared series (its `data.series[i]` path).
    pub fn source_index(&self, i: usize) -> usize {
        self.spec.raw["data"]["series"]
            .as_array()
            .and_then(|a| a.iter().position(|v| std::ptr::eq(v, self.series[i])))
            .unwrap_or(i)
    }
    pub fn role(&self, i: usize) -> Option<&'a str> {
        self.series[i]["role"].as_str()
    }
    pub fn id(&self, i: usize) -> &str {
        &self.data.series[i].id
    }
    pub fn name(&self, i: usize) -> String {
        display(&self.series[i]["name"])
            .map(str::to_owned)
            .unwrap_or_else(|| self.data.series[i].id.clone())
    }
    pub fn own_slot(&self, i: usize) -> Option<u8> {
        self.series[i]["slot"]
            .as_u64()
            .filter(|k| (1..=16).contains(k))
            .map(|k| k as u8)
    }
    pub fn x_kind(&self) -> &str {
        &self.data.x_kind
    }
    pub fn categories(&self) -> &[String] {
        &self.data.categories
    }
    pub fn x_name(&self, j: usize) -> String {
        let c = &self.data.categories[j];
        display(&self.spec.raw["data"]["x"]["names"][c.as_str()])
            .map(str::to_owned)
            .unwrap_or_else(|| c.clone())
    }
    pub fn x_role(&self, j: usize) -> Option<&'a str> {
        self.spec.raw["data"]["x"]["roles"][self.data.categories[j].as_str()].as_str()
    }
    pub fn x_group(&self, j: usize) -> Option<&'a str> {
        self.spec.raw["data"]["x"]["groups"][self.data.categories[j].as_str()].as_str()
    }
    pub fn value(&self, i: usize, j: usize) -> Option<Decimal> {
        self.data.series.get(i)?.values.get(j)?.value
    }
    /// The named slots (`colours.slots`, the host having merged the registry under it).
    pub fn named_slot(&self, name: &str) -> Option<u8> {
        self.spec.raw["colours"]["slots"][name]
            .as_u64()
            .filter(|k| (1..=16).contains(k))
            .map(|k| k as u8)
    }
    pub fn has_named_slots(&self) -> bool {
        self.spec.raw["colours"]["slots"]
            .as_object()
            .is_some_and(|m| !m.is_empty())
    }
    pub fn reasons(&self, path: &str) -> Option<&'a str> {
        self.spec.raw["reasons"][path].as_str()
    }
    pub fn overridden(&self, rule: &str) -> Option<&'a str> {
        self.spec.raw["overrides"]
            .as_array()?
            .iter()
            .find(|o| o["rule"].as_str() == Some(rule))
            .and_then(|o| o["reason"].as_str())
    }
    pub fn analysis(&self) -> bool {
        self.spec.raw["purpose"].as_str() == Some("analysis")
    }
    /// The references of the highlight, else of the insight (bars.targets, pie, treemap).
    pub fn highlight_refs(&self) -> Vec<&'a Value> {
        let hl = self.spec.raw["highlight"].as_array();
        if let Some(hl) = hl.filter(|h| !h.is_empty()) {
            return hl
                .iter()
                .flat_map(|h| h["targets"].as_array().into_iter().flatten())
                .collect();
        }
        self.spec.raw["insight"]["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .collect()
    }
    /// The chooser's sort (choose.py: `chart.sort`, else desc for nominal categories of a type
    /// that does not keep the given order, else given).
    pub fn choice_sort(&self, keeps_given: bool) -> &'a str {
        let asked = self.string("/chart/sort", "auto");
        let nominal = self.x_kind() == "nominal";
        let fixed = self.string("/data/x/order", "auto") == "fixed"
            || (!self.get("/data/parts").is_null()
                && self.string("/data/parts/order", "auto") == "fixed"
                && self
                    .series
                    .iter()
                    .filter(|s| !s["role"].as_str().is_some_and(|r| REF_ROLES.contains(&r)))
                    .count()
                    == 1);
        let recommended = if nominal && !keeps_given && !fixed {
            "desc"
        } else {
            "given"
        };
        if !nominal && matches!(asked, "desc" | "asc") {
            "given"
        } else if asked == "auto" {
            recommended
        } else {
            asked
        }
    }
}
