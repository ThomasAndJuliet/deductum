//! Colours of series: bars and dots (bars.colours), lines (lines._colours), bumps, arrows,
//! and a waterfall's steps.
use crate::src::{REF_ROLES, Src, is_regular, role_class};
use crate::{Assignments, Draft, class, decision};
use chartgen_model::{ChartType, Decimal};
use std::collections::BTreeSet;

fn free_slots(fixed: &[Option<u8>], members: &[usize]) -> impl Iterator<Item = u8> {
    let used: BTreeSet<u8> = members.iter().filter_map(|&i| fixed[i]).collect();
    (1..=16u8).filter(move |k| !used.contains(k))
}

fn slot_words(c: &[String]) -> String {
    c.first()
        .map(|c| c.replace("series-", "slot "))
        .unwrap_or_default()
}

/// bars.py: one bar series is `single`; several regular series take slots 1, 2, … in series
/// order (a fixed slot kept), a special series its reserved grey. A reference or target series
/// is drawn as flat markers across the bars, in the strong shade of the bars' hue (R11-4).
pub fn bars(
    src: &Src,
    fixed: &[Option<u8>],
    kind: ChartType,
    d: &mut Draft,
    out: &mut Assignments,
) {
    let n = src.n_series();
    let refs: Vec<usize> = (0..n)
        .filter(|&i| src.role(i).is_some_and(|r| REF_ROLES.contains(&r)))
        .collect();
    let bs: Vec<usize> = (0..n).filter(|i| !refs.contains(i)).collect();
    if bs.len() == 1 {
        d.series[bs[0]] = class("single");
        for &i in &refs {
            // a flat marker across each bar, in the strong shade of the bar's hue (R11-4)
            d.series[i] = vec!["reference".into(), "single".into()];
        }
        // one series: each bar its category's reserved grey, else the single colour
        for j in 0..src.categories().len() {
            d.categories[j] = class(
                src.x_role(j)
                    .and_then(|r| role_class(r, false))
                    .unwrap_or("single"),
            );
        }
        return;
    }
    let regular: Vec<usize> = bs
        .iter()
        .copied()
        .filter(|&i| is_regular(src.role(i)))
        .collect();
    for &i in &bs {
        if !regular.contains(&i) {
            d.series[i] = class(
                src.role(i)
                    .and_then(|r| role_class(r, false))
                    .unwrap_or("single"),
            );
        }
    }
    let any_fixed = regular.iter().any(|&i| fixed[i].is_some());
    if regular.len() == 1 && !any_fixed {
        d.series[regular[0]] = class("single");
    } else {
        let mut free = free_slots(fixed, &regular);
        for &i in &regular {
            let k = fixed[i].unwrap_or_else(|| free.next().unwrap_or(16));
            d.series[i] = class(format!("series-{k}"));
        }
    }
    for &i in &refs {
        // markers across clustered bars are a later stage of the original: the reserved dark grey
        d.series[i] = class("reference");
    }
    if kind == ChartType::Bar && bs.len() > 1 {
        out.decisions.push(decision(
            "R9-12",
            "locked",
            format!(
                "{} series side by side in each cluster, the bars touching, in the same order in every cluster: {} (G7.5, G4.22)",
                bs.len(),
                bs.iter()
                    .map(|&i| format!("{} ({})", src.name(i), slot_words(&d.series[i])))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
}

/// The value at a line's end: its last known value, an incomplete period drawn apart or left
/// out not counted (lines.S.line_last).
fn line_end(src: &Src, i: usize) -> Option<Decimal> {
    let values = &src.data.series[i].values;
    values
        .iter()
        .rev()
        .find(|d| d.value.is_some() && d.complete)
        .or_else(|| values.iter().rev().find(|d| d.value.is_some()))
        .and_then(|d| d.value)
}

/// lines._colours: palette slots top to bottom by the line ends, unless fixed (6.5.6, R6-28); a
/// sole regular series takes the single colour (R3-3); special series their reserved classes.
pub fn lines(
    src: &Src,
    fixed: &[Option<u8>],
    members: &[usize],
    d: &mut Draft,
    out: &mut Assignments,
) {
    let regular: Vec<usize> = members
        .iter()
        .copied()
        .filter(|&i| is_regular(src.role(i)))
        .collect();
    for &i in members {
        if let Some(r) = src.role(i).filter(|r| !is_regular(Some(r))) {
            d.series[i] = class(role_class(r, true).unwrap_or("single"));
        }
    }
    let fixed_ids: Vec<usize> = regular
        .iter()
        .copied()
        .filter(|&i| fixed[i].is_some())
        .collect();
    if regular.len() == 1 && fixed_ids.is_empty() {
        d.series[regular[0]] = class("single");
        return;
    }
    let mut order = regular.clone();
    order.sort_by(|&a, &b| {
        let (ea, eb) = (line_end(src, a), line_end(src, b));
        // the highest end first; a line without values last; ties in series order
        match (ea, eb) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then(a.cmp(&b))
    });
    let mut free = free_slots(fixed, &regular);
    for &i in &order {
        let k = fixed[i].unwrap_or_else(|| free.next().unwrap_or(16));
        d.series[i] = class(format!("series-{k}"));
    }
    if regular.len() > 1 {
        let mut text = format!(
            "each line its own colour, handed out top to bottom by the line ends: {}",
            order
                .iter()
                .map(|&i| format!("{} {}", src.name(i), slot_words(&d.series[i])))
                .collect::<Vec<_>>()
                .join(", ")
        );
        if !fixed_ids.is_empty() {
            text.push_str(&format!(
                " ({} fixed)",
                fixed_ids
                    .iter()
                    .map(|&i| src.id(i).to_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out.decisions.push(decision("R6-28", "locked", text));
    }
}

/// Lines with a total panel (areas.render_panels): the total in its reserved class, the parts
/// coloured as a line chart's.
pub fn line_total(src: &Src, fixed: &[Option<u8>], d: &mut Draft, out: &mut Assignments) {
    let n = src.n_series();
    let parts: Vec<usize> = (0..n)
        .filter(|&i| !matches!(src.role(i), Some("total" | "reference" | "target")))
        .collect();
    for i in 0..n {
        match src.role(i) {
            Some("total") => d.series[i] = class("total"),
            Some("reference" | "target") => d.series[i] = class("reference"),
            _ => {}
        }
    }
    lines(src, fixed, &parts, d, out);
}

/// bumps.py: ranks per period (largest first, or smallest where lower is good), and slots top to
/// bottom by the final ranks, unless fixed (R6-28).
pub fn bumps(src: &Src, fixed: &[Option<u8>], d: &mut Draft, out: &mut Assignments) {
    let n = src.n_series();
    let periods = src.categories().len();
    let rank_kind = src.string("/data/measure/kind", "") == "rank";
    let low = src.string("/data/measure/good", "") == "lower";
    let mut ranks = vec![vec![None::<i64>; periods]; n];
    for j in 0..periods {
        let known: Vec<(usize, Decimal)> = (0..n)
            .filter_map(|i| src.value(i, j).map(|v| (i, v)))
            .collect();
        for &(i, v) in &known {
            ranks[i][j] = Some(if rank_kind {
                v.trunc().try_into().unwrap_or(0)
            } else {
                known
                    .iter()
                    .filter(|(_, w)| if low { *w < v } else { *w > v })
                    .count() as i64
                    + 1
            });
        }
    }
    let big_n = ranks
        .iter()
        .flatten()
        .flatten()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    let final_rank = |i: usize| ranks[i].iter().rev().find_map(|r| *r).unwrap_or(big_n + 1);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (final_rank(i), i));
    let all: Vec<usize> = (0..n).collect();
    let mut free = free_slots(fixed, &all);
    for &i in &order {
        let k = fixed[i].unwrap_or_else(|| free.next().unwrap_or(16));
        d.series[i] = class(format!("series-{k}"));
    }
    out.decisions.push(decision(
        "R6-28",
        "locked",
        format!(
            "each line its own colour, handed out top to bottom by the final ranks: {}",
            order
                .iter()
                .map(|&i| format!("{} {}", src.name(i), slot_words(&d.series[i])))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ));
}

/// arrowchart._colours: each row (a series, from its first value to its second) in the good or
/// bad colour where the measure has a good direction, else rises in slot 1 and falls in slot 2.
pub fn arrows(src: &Src, d: &mut Draft, out: &mut Assignments) {
    let good = src.string("/data/measure/good", "");
    for i in 0..src.n_series() {
        let (Some(before), Some(after)) = (src.value(i, 0), src.value(i, 1)) else {
            continue;
        };
        let rise = after > before;
        d.series[i] = class(if matches!(good, "higher" | "lower") {
            if after == before {
                "series-1"
            } else if rise == (good == "higher") {
                "good"
            } else {
                "bad"
            }
        } else if after >= before {
            "series-1"
        } else {
            "series-2"
        });
    }
    out.decisions.push(if !good.is_empty() {
        decision(
            "R3-17",
            "guided",
            format!(
                "arrows in the good colour where the change is good and the bad colour where it is bad ({good} is better; G7.22, D-40)"
            ),
        )
    } else {
        decision(
            "R3-17",
            "guided",
            "no good or bad reading is given (measure.good): rises in the first palette colour, falls in the second, never red and green (G7.22, D-40)",
        )
    });
}

/// waterfall.py: each step in slot 1 where it rises and slot 2 where it falls, or good and bad
/// with a good direction (a step of no change then in slot 3); a total a full bar in its grey.
pub fn waterfall(src: &Src, d: &mut Draft) {
    let good = src.string("/data/measure/good", "");
    for (i, c) in d.series.iter_mut().enumerate() {
        *c = class(if i == 0 { "series-1" } else { "single" });
    }
    for j in 0..src.categories().len() {
        if src.x_role(j) == Some("total") {
            d.categories[j] = class("total");
            continue;
        }
        let Some(v) = src.value(0, j) else { continue };
        let up = v > Decimal::ZERO;
        d.categories[j] = class(if !good.is_empty() {
            if v.is_zero() {
                "series-3"
            } else if up == (good == "higher") {
                "good"
            } else {
                "bad"
            }
        } else if v >= Decimal::ZERO {
            "series-1"
        } else {
            "series-2"
        });
    }
}

fn slot_class(c: &str) -> bool {
    c == "single" || c.starts_with("series-")
}

/// The dividing value of each category (annotate.dividing): a number, or a comparison's values.
fn dividing(src: &Src, out: &mut Assignments) -> Option<Vec<Option<Decimal>>> {
    // the prepared measure: a derived deviation divides at zero (transform.py)
    let m = if src.data.measure.is_object() {
        &src.data.measure
    } else {
        src.get("/data/measure")
    };
    let d = &m["dividing_point"];
    if m["good"].as_str().is_none() || d.is_null() {
        return None;
    }
    let n = src.categories().len();
    if let Some(id) = d["comparison"].as_str() {
        let Some(c) = src.data.comparisons.iter().find(|c| c.id == id) else {
            out.findings.push(crate::finding(
                "CG-4",
                "error",
                "locked",
                format!("no comparison {id:?}").replace('"', "'"),
                Some("data.measure.dividing_point".into()),
            ));
            return None;
        };
        return Some(match c.values.len() {
            1 => vec![c.values[0].value; n],
            k if k == n => c.values.iter().map(|v| v.value).collect(),
            _ => vec![None; n],
        });
    }
    let v = d
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| d.to_string());
    match (
        d.is_number(),
        v.parse::<Decimal>()
            .ok()
            .or_else(|| Decimal::from_scientific(&v).ok()),
    ) {
        (true, Some(x)) => Some(vec![Some(x); n]),
        _ => {
            out.findings.push(crate::finding(
                "CG-4",
                "error",
                "locked",
                "a number or {\"comparison\": id}",
                Some("data.measure.dividing_point".into()),
            ));
            None
        }
    }
}

/// annotate.good_bad: on bars and dots with a good direction and a dividing value, each mark on
/// the bad side takes `bad`, on the good side `good` in place of its slot colour; a value on the
/// dividing value keeps its colour (R15-11, G4.16; D-18). A flat reference marker then takes the
/// reserved dark grey.
pub fn good_bad(src: &Src, order: &[usize], d: &mut Draft, out: &mut Assignments) {
    let Some(div) = dividing(src, out) else {
        return;
    };
    let good = src.data.measure["good"]
        .as_str()
        .unwrap_or_else(|| src.string("/data/measure/good", "higher"));
    let n = src.n_series();
    let cats = src.categories().len();
    let multi = (0..n)
        .filter(|&i| !src.role(i).is_some_and(|r| REF_ROLES.contains(&r)))
        .count()
        > 1;
    let refs = src.highlight_refs();
    let hit = |sid: &str, x: &str| {
        refs.iter().any(|r| {
            let cat = r["category"].as_str().or_else(|| r["x"].as_str());
            let series = r["series"].as_str();
            (series.is_some() || cat.is_some())
                && series.is_none_or(|s| s == sid)
                && cat.is_none_or(|c| c == x)
        })
    };
    let mut marks = vec![vec![vec![]; cats]; n];
    let (mut goods, mut bads, mut neutral) = (vec![], vec![], vec![]);
    let order: Vec<usize> = if order.len() == cats {
        order.to_vec()
    } else {
        (0..cats).collect()
    };
    for i in 0..n {
        if !is_regular(src.role(i)) {
            continue;
        }
        for &j in &order {
            let (Some(v), Some(dv)) = (src.value(i, j), div[j]) else {
                continue;
            };
            let x = &src.categories()[j];
            let side = if v == dv {
                "neutral"
            } else if (v > dv) == (good == "higher") {
                "good"
            } else {
                "bad"
            };
            match side {
                "good" => goods.push(x.clone()),
                "bad" => bads.push(x.clone()),
                _ => {
                    neutral.push(x.clone());
                    out.neutral.push((src.id(i).to_owned(), j));
                    continue;
                }
            }
            if hit(src.id(i), x) {
                out.findings.push(crate::info(
                    "R15-11",
                    format!(
                        "{x}: its good/bad colour takes the place of the strong shade; highlight it by a border, a glow or an icon"
                    ),
                    Some("highlight".into()),
                ));
            }
            let base = if multi || d.categories[j].is_empty() {
                &d.series[i]
            } else {
                &d.categories[j]
            };
            marks[i][j] = base
                .iter()
                .map(|c| {
                    if slot_class(c) {
                        side.to_owned()
                    } else {
                        c.clone()
                    }
                })
                .collect();
        }
    }
    if goods.is_empty() && bads.is_empty() {
        return;
    }
    for i in 0..n {
        if src.role(i).is_some_and(|r| REF_ROLES.contains(&r)) {
            d.series[i] = class("reference");
        }
    }
    d.marks = marks;
    let dp = if src.data.measure.is_object() {
        &src.data.measure["dividing_point"]
    } else {
        src.get("/data/measure/dividing_point")
    };
    let what = match dp["comparison"].as_str() {
        Some(id) => format!(
            "its {}",
            src.data
                .comparisons
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.label.to_lowercase())
                .unwrap_or_else(|| id.to_owned())
        ),
        None => div
            .iter()
            .flatten()
            .next()
            .map(|v| v.normalize().to_string())
            .unwrap_or_default(),
    };
    let none = |v: &[String]| {
        if v.is_empty() {
            "none".to_owned()
        } else {
            v.join(", ")
        }
    };
    out.decisions.push(decision(
        "R15-11",
        "locked",
        format!(
            "good and bad coloured by the dividing value, {what} ({good} is good): bad {}; good {}{} (G4.16)",
            none(&bads),
            none(&goods),
            if neutral.is_empty() {
                String::new()
            } else {
                format!("; on it, in its own colour: {}", neutral.join(", "))
            }
        ),
    ));
}
