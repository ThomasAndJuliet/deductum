//! Palette slots fixed by name across a set of charts (the original's `colour_slots`,
//! R3-24, G4.22) and the slots of the series inside small multiples (layout/multiples.py,
//! R6-28, G7.50).
use crate::src::{Src, display, is_regular};
use crate::{decision, info, warn};
use chartgen_model::{Error, Finding, Result, Spec};
use serde_json::Value;
use std::collections::BTreeMap;

fn err(path: impl Into<String>, message: impl Into<String>) -> Error {
    Error::new("CG-4", message).at(path)
}

/// Check a `colours.slots` map: every slot an integer 1–16.
pub fn validate(spec: &Spec) -> Result<()> {
    if let Some(names) = spec.raw["colours"]["slots"].as_object() {
        for (name, v) in names {
            let ok = v.as_u64().is_some_and(|k| (1..=16).contains(&k));
            if !ok {
                return Err(err(
                    format!("colours.slots.{name}"),
                    "palette slot must be an integer 1–16",
                ));
            }
        }
    }
    for (i, s) in spec.raw["data"]["series"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let v = &s["slot"];
        if !v.is_null() && !v.as_u64().is_some_and(|k| (1..=16).contains(&k)) {
            return Err(err(
                format!("data.series[{i}].slot"),
                "palette slot must be an integer 1–16",
            ));
        }
    }
    Ok(())
}

/// The slot each prepared series takes before the chart is drawn: its own (`series.slot`), else
/// the slot `colours.slots` gives its name; two regular series of one chart (outside small
/// multiples) never share one, the later losing its slot with a warning. Returns the slots,
/// findings and the leading R3-24 decision, in the original's order.
pub fn fixed(src: &Src) -> (Vec<Option<u8>>, Vec<Finding>, Vec<Value>) {
    let n = src.n_series();
    let mut slots: Vec<Option<u8>> = (0..n).map(|i| src.own_slot(i)).collect();
    let mut findings = vec![];
    let mut named: Vec<(String, u8)> = vec![];
    if src.has_named_slots() {
        for (i, slot) in slots.iter_mut().enumerate() {
            let name = src.name(i);
            let Some(k) = src.named_slot(&name) else {
                continue;
            };
            match *slot {
                None => {
                    *slot = Some(k);
                    named.push((name, k));
                }
                Some(own) if own != k => findings.push(info(
                    "R3-24",
                    format!(
                        "{name} fixes its own slot {own}, not the {k} of colours.slots: its own is kept"
                    ),
                    Some(format!("data.series[{}].slot", src.source_index(i))),
                )),
                _ => {}
            }
        }
    }
    let dims = src
        .get("/data/dims")
        .as_array()
        .is_some_and(|d| !d.is_empty());
    let mut by_slot = BTreeMap::<u8, String>::new();
    for (i, slot) in slots.iter_mut().enumerate() {
        let Some(k) = *slot else { continue };
        if !is_regular(src.role(i)) {
            continue;
        }
        let name = src.name(i);
        let first = by_slot.entry(k).or_insert_with(|| name.clone()).clone();
        if first != name && !dims {
            findings.push(warn(
                "R3-24",
                format!(
                    "{first} and {name} would share palette slot {k}: {name} takes the next free slot (each series its own colour, G4.27)"
                ),
                Some(format!("data.series[{}].slot", src.source_index(i))),
            ));
            *slot = None;
        }
    }
    let mut leading = vec![];
    if !named.is_empty() {
        let mut seen = vec![];
        for x in named {
            if !seen.contains(&x) {
                seen.push(x);
            }
        }
        leading.push(decision(
            "R3-24",
            "locked",
            format!(
                "colours fixed by name, the same in every chart of the set (colours.slots): {} (G4.22)",
                seen.iter()
                    .map(|(n, k)| format!("{n} slot {k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    (slots, findings, leading)
}

/// The label of a series inside the panels: its values of the dims drawn within them
/// (multiples._label).
pub fn panel_label(series: &Value, within: &[String]) -> String {
    let keys = &series["keys"];
    let parts: Vec<String> = within
        .iter()
        .filter_map(|d| keys[d.as_str()].as_str().map(str::to_owned))
        .collect();
    if parts.is_empty() {
        display(&series["name"])
            .or_else(|| series["id"].as_str())
            .unwrap_or("")
            .to_owned()
    } else {
        parts.join(", ")
    }
}

/// The slots of small multiples' series (multiples.py, R6-28, R3-24, G7.50): each series label
/// inside the panels its own slot, the same in every panel. `combos` are the labels in the order
/// the slots are handed out (top to bottom by the line ends of the last panel); a label's slot is
/// fixed by one of its series (`series.slot`, or `colours.slots` by the series' name) or by
/// `colours.slots` by the label. Returns {series index: slot}, the findings, and the R6-28
/// decision when there are several panels or fixed slots.
pub fn panels(
    spec: &Spec,
    within: &[String],
    combos: &[String],
    several_panels: bool,
) -> (BTreeMap<usize, u8>, Vec<Finding>, Vec<Value>) {
    let series = spec.raw["data"]["series"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let names = &spec.raw["colours"]["slots"];
    let slot_of = |v: &Value| v.as_u64().filter(|k| (1..=16).contains(k)).map(|k| k as u8);
    let mut findings = vec![];
    let mut fixed = BTreeMap::<String, u8>::new();
    let mut taken = BTreeMap::<u8, String>::new();
    let mut warned = std::collections::BTreeSet::new();
    for (i, s) in series.iter().enumerate() {
        let k = panel_label(s, within);
        // a series' own slot, or the slot colours.slots gives its name (set before the layout)
        let own = slot_of(&s["slot"]).or_else(|| {
            display(&s["name"])
                .or_else(|| s["id"].as_str())
                .and_then(|n| slot_of(&names[n]))
        });
        let Some(slot) = own.or_else(|| slot_of(&names[k.as_str()])) else {
            continue;
        };
        if warned.contains(&k) {
            continue;
        }
        if let Some(&f) = fixed.get(&k) {
            if f != slot {
                findings.push(warn(
                    "R3-24",
                    format!(
                        "the series of {k} fix two slots, {f} and {slot}: {f} kept, the same in every panel (G7.50)"
                    ),
                    Some(format!("data.series[{i}].slot")),
                ));
                warned.insert(k);
            }
            continue;
        }
        if let Some(first) = taken.get(&slot) {
            findings.push(warn(
                "R3-24",
                format!(
                    "{first} and {k} would share palette slot {slot}: {k} takes a free slot (each series its own colour, G4.27)"
                ),
                Some(format!("data.series[{i}].slot")),
            ));
            warned.insert(k);
            continue;
        }
        fixed.insert(k.clone(), slot);
        taken.insert(slot, k);
    }
    let fixed: Vec<(String, u8)> = combos
        .iter()
        .filter_map(|k| fixed.get(k).map(|&s| (k.clone(), s)))
        .collect();
    let used: Vec<u8> = fixed.iter().map(|(_, s)| *s).collect();
    let mut free = (1..=16u8).filter(|k| !used.contains(k));
    let mut by_combo = BTreeMap::new();
    for k in combos {
        let slot = fixed
            .iter()
            .find(|(f, _)| f == k)
            .map(|(_, s)| *s)
            .unwrap_or_else(|| free.next().unwrap_or(16));
        by_combo.insert(k.clone(), slot);
    }
    let mut out = BTreeMap::new();
    for (i, s) in series.iter().enumerate() {
        if let Some(&slot) = by_combo.get(&panel_label(s, within)) {
            out.insert(i, slot);
        }
    }
    let mut decisions = vec![];
    if several_panels || !fixed.is_empty() {
        let handed: Vec<&String> = combos
            .iter()
            .filter(|k| !fixed.iter().any(|(f, _)| f == *k))
            .collect();
        let mut text = String::from("each series its own colour, the same in every panel");
        if !handed.is_empty() {
            text.push_str(", handed out top to bottom by the line ends of the last panel: ");
            text.push_str(
                &handed
                    .iter()
                    .map(|k| format!("{k} slot {}", by_combo[*k]))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if !fixed.is_empty() {
            text.push_str(if handed.is_empty() { ", " } else { "; " });
            text.push_str("fixed, the same in every chart of the set (R3-24, G4.22): ");
            text.push_str(
                &fixed
                    .iter()
                    .map(|(k, v)| format!("{k} slot {v}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        text.push_str(" (G4.27, G7.50)");
        decisions.push(decision("R6-28", "locked", text));
    }
    (out, findings, decisions)
}
