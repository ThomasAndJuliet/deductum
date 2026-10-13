//! Colours and orders of the parts of a total: pie and donut slices (pie.order, pie.colours),
//! treemap tiles (treemap.py), wrapped bars (wrapped.py) and stacks (bars.Stack.setup).
use crate::src::{REF_ROLES, Src, TAIL, is_regular, role_class};
use crate::{Assignments, Draft, class, decision, finding, info, warn};
use chartgen_model::Decimal;
use serde_json::Value;
use std::collections::BTreeMap;

const SEQ_STEPS: usize = 9;
const SEQ_LIGHTEST: usize = 3;

fn tail_class(role: &str) -> &'static str {
    if role == "all_others" {
        "all-others"
    } else {
        "missing"
    }
}
fn in_tail(role: Option<&str>) -> bool {
    role.is_some_and(|r| TAIL.contains(&r))
}
fn tail_index(role: Option<&str>) -> usize {
    role.and_then(|r| TAIL.iter().position(|t| *t == r))
        .unwrap_or(0)
}

/// One part of a total.
#[derive(Debug, Clone)]
struct Part {
    name: String,
    /// The category index (one total) or the series index (several totals).
    index: usize,
    role: Option<String>,
    group: Option<String>,
    /// Its size in each pie (one total: one).
    sizes: Vec<Decimal>,
    colour: Option<String>,
    tint: bool,
    shade: Option<&'static str>,
    fixed: Option<u8>,
}
impl Part {
    fn size(&self) -> Decimal {
        self.sizes.first().copied().unwrap_or_default()
    }
    fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }
}

/// The steps of the sequential ramp for n ordered parts, the first the darkest (G4.7, R3-8).
fn ordered_steps(n: usize) -> Vec<usize> {
    (0..n)
        .map(|j| {
            (SEQ_STEPS as f64 - (SEQ_STEPS - SEQ_LIGHTEST) as f64 * j as f64 / (n - 1) as f64 + 0.5)
                .floor() as usize
        })
        .collect()
}

fn words(c: &str) -> String {
    c.replace("series-", "slot ").replace("seq-", "step ")
}

/// The parts of one total: the categories, sized by the one series' values (pie.parts_of).
fn categories_as_parts(src: &Src, series: usize) -> Vec<Part> {
    (0..src.categories().len())
        .map(|j| Part {
            name: src.x_name(j),
            index: j,
            role: src.x_role(j).map(str::to_owned),
            group: src.x_group(j).map(str::to_owned),
            sizes: vec![src.value(series, j).map(|v| v.abs()).unwrap_or_default()],
            colour: None,
            tint: false,
            shade: None,
            fixed: None,
        })
        .collect()
}

/// pie.order: the parts clockwise from 12 o'clock.
fn pie_order(
    parts: Vec<Part>,
    sort: &str,
    ordinal: bool,
    nominal: bool,
    out: &mut Assignments,
) -> Vec<Part> {
    if parts.iter().any(|p| p.group.is_some()) {
        let mut groups: Vec<(String, Vec<Part>)> = vec![];
        for p in parts {
            let key = p.group.clone().unwrap_or_else(|| p.name.clone());
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(p),
                None => groups.push((key, vec![p])),
            }
        }
        groups.sort_by(|a, b| {
            let sa: Decimal = a.1.iter().map(Part::size).sum();
            let sb: Decimal = b.1.iter().map(Part::size).sum();
            sb.cmp(&sa).then(a.1[0].index.cmp(&b.1[0].index))
        });
        let out_parts: Vec<Part> = groups.into_iter().flat_map(|(_, v)| v).collect();
        out.decisions.push(decision(
            "R7-35",
            "guided",
            format!(
                "slices by group, largest group first, each group's parts together in their given order, clockwise from 12 o'clock (07 §5.2): {}",
                out_parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
            ),
        ));
        return out_parts;
    }
    let mut others: Vec<Part> = parts
        .iter()
        .filter(|p| in_tail(p.role()))
        .cloned()
        .collect();
    others.sort_by_key(|p| tail_index(p.role()));
    let (mut head, zeros): (Vec<Part>, Vec<Part>);
    let how;
    if sort == "given" || ordinal {
        head = parts
            .iter()
            .filter(|p| !in_tail(p.role()))
            .cloned()
            .collect();
        zeros = vec![];
        how = if !nominal || ordinal {
            "in their order"
        } else {
            "in the given order"
        };
    } else {
        head = parts
            .iter()
            .filter(|p| !in_tail(p.role()) && p.size() > Decimal::ZERO)
            .cloned()
            .collect();
        zeros = parts
            .iter()
            .filter(|p| !in_tail(p.role()) && p.size().is_zero())
            .cloned()
            .collect();
        if sort == "asc" {
            head.sort_by(|a, b| a.size().cmp(&b.size()).then(b.index.cmp(&a.index)));
            how = "smallest first";
        } else {
            head.sort_by(|a, b| b.size().cmp(&a.size()).then(a.index.cmp(&b.index)));
            how = "largest first";
        }
    }
    out.decisions.push(decision(
        "R7-35",
        "guided",
        format!(
            "slices {how}, clockwise from 12 o'clock: {}",
            head.iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ));
    if !others.is_empty() {
        let one = others.len() == 1;
        out.decisions.push(decision(
            "R7-17",
            "locked",
            format!(
                "{} last, whatever {} size, in {} reserved grey{} (G2.1, G4.15, G7.28)",
                others
                    .iter()
                    .map(|p| format!("“{}”", p.name))
                    .collect::<Vec<_>>()
                    .join(" and "),
                if one { "its" } else { "their" },
                if one { "its" } else { "their" },
                if one { "" } else { "s" }
            ),
        ));
    }
    if !zeros.is_empty() {
        out.decisions.push(decision(
            "R7-36",
            "locked",
            format!(
                "{}: zero, kept and labelled, last, at 12 o'clock (G7.29)",
                zeros
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    head.into_iter().chain(others).chain(zeros).collect()
}

/// pie.colours: palette slots in slice order (R7-32, D-22 (a)); a group's second part a tint
/// of its first; a fixed slot kept; ordered parts in steps of one hue (G4.7, R3-8).
fn pie_colours(parts: &mut [Part], src: &Src, several: bool, ordinal: bool, out: &mut Assignments) {
    let mut fixed = BTreeMap::<usize, u8>::new();
    if several {
        for p in parts.iter() {
            if let Some(k) = p.fixed {
                fixed.insert(p.index, k);
            }
        }
    } else if src.has_named_slots() {
        let mut taken: Vec<(u8, String)> = vec![];
        for p in parts.iter() {
            let Some(k) = src.named_slot(&p.name) else {
                continue;
            };
            if p.role.is_some() {
                continue; // a special part keeps its reserved grey
            }
            if let Some((_, first)) = taken.iter().find(|(t, _)| *t == k) {
                out.findings.push(warn(
                    "R3-24",
                    format!(
                        "{first} and {} would share palette slot {k}: {} takes a free slot (each slice its own colour, G7.25)",
                        p.name, p.name
                    ),
                    Some("colours.slots".into()),
                ));
                continue;
            }
            fixed.insert(p.index, k);
            taken.push((k, p.name.clone()));
        }
        if !fixed.is_empty() {
            out.decisions.push(decision(
                "R3-24",
                "locked",
                format!(
                    "colours fixed by name, the same in every chart of the set (colours.slots): {} (G4.22)",
                    taken
                        .iter()
                        .map(|(k, n)| format!("{n} slot {k}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }
    let used: Vec<u8> = fixed.values().copied().collect();
    let mut free = (1..=16u8).filter(|k| !used.contains(k));
    let mut seen = BTreeMap::<String, (String, usize)>::new();
    let mut k = 0;
    for p in parts.iter_mut() {
        if in_tail(p.role()) {
            p.colour = Some(tail_class(p.role().unwrap()).into());
            continue;
        }
        if p.sizes.iter().all(|v| v.is_zero()) {
            continue;
        }
        if let Some(&slot) = fixed.get(&p.index) {
            k += 1;
            p.colour = Some(format!("series-{slot}"));
            continue;
        }
        if let Some(g) = p.group.clone() {
            if let Some((first, n)) = seen.get_mut(&g) {
                p.colour = Some(first.clone());
                p.tint = true;
                *n += 1;
                if *n > 2 {
                    out.findings.push(info(
                        "CG-3",
                        format!(
                            "a group of more than two parts ({g}): its third part and after share the second's tint"
                        ),
                        Some("data.x.groups".into()),
                    ));
                }
                continue;
            }
        }
        k += 1;
        let c = format!("series-{}", free.next().unwrap_or(16));
        if let Some(g) = &p.group {
            seen.insert(g.clone(), (c.clone(), 1));
        }
        p.colour = Some(c);
    }
    let regular: Vec<usize> = (0..parts.len())
        .filter(|&i| parts[i].colour.is_some() && !in_tail(parts[i].role()))
        .collect();
    let limit = SEQ_STEPS - SEQ_LIGHTEST + 1;
    let stepped = ordinal
        && fixed.is_empty()
        && seen.is_empty()
        && regular.len() > 1
        && regular.len() <= limit;
    if stepped {
        for (&i, step) in regular.iter().zip(ordered_steps(regular.len())) {
            parts[i].colour = Some(format!("seq-{step}"));
        }
        out.decisions.push(decision(
            "R3-8",
            "guided",
            "the parts are ordered: steps of one hue, darker for each part from the last to the first, the first at 12 o'clock (G4.7; each slice still its own colour, G7.25)",
        ));
    } else if ordinal && fixed.is_empty() && regular.len() > limit {
        out.findings.push(finding(
            "R3-8",
            "warn",
            "guided",
            format!(
                "{} ordered parts: more than {limit} steps of one hue cannot be told apart; a hue each (G4.7, G4.21)",
                regular.len()
            ),
            Some(if several { "data.parts.kind" } else { "data.x.kind" }.into()),
        ));
    }
    let named: Vec<String> = parts
        .iter()
        .filter_map(|p| {
            p.colour
                .as_ref()
                .map(|c| format!("{} {c}{}", p.name, if p.tint { " tint" } else { "" }))
        })
        .collect();
    out.decisions.push(decision(
        "R7-32",
        "locked",
        format!(
            "{}{}{}",
            if several {
                "each part its own colour, the same in every pie (R7-24), "
            } else {
                "each slice its own colour, "
            },
            if stepped {
                "in steps of one hue in their order: "
            } else {
                "handed out in slice order (D-22 (a)): "
            },
            named.join(", ")
        ),
    ));
    if k > 8 {
        out.findings.push(info(
            "R7-32",
            format!(
                "{k} colours: beyond eight they are hard to tell apart; each slice is labelled directly (G4.20)"
            ),
            None,
        ));
    }
}

fn classes(p: &Part) -> Vec<String> {
    let mut out = vec![];
    if let Some(c) = &p.colour {
        out.push(c.clone());
        if let Some(s) = p.shade {
            out.push(s.into());
        }
        if p.tint {
            out.push("tint".into());
        }
    }
    out
}

/// The series a pie draws (pie._series): regular series and the special parts.
fn pie_series(src: &Src) -> Vec<usize> {
    (0..src.n_series())
        .filter(|&i| {
            matches!(
                src.role(i),
                None | Some("regular" | "all_others" | "missing")
            )
        })
        .collect()
}

/// The series that sorts (D-46): `chart.sort_by`, else the series the insight names, else the
/// first that is not a reference.
fn sort_by(src: &Src) -> usize {
    let id = src.get("/chart/sort_by").as_str().or_else(|| {
        src.get("/insight/targets")
            .as_array()
            .and_then(|a| a.iter().find_map(|t| t["series"].as_str()))
    });
    id.and_then(|id| (0..src.n_series()).find(|&i| src.id(i) == id))
        .or_else(|| {
            (0..src.n_series()).find(|&i| !src.role(i).is_some_and(|r| REF_ROLES.contains(&r)))
        })
        .unwrap_or(0)
}

/// Pies and donuts: one total (its parts the categories) or several (the categories the totals,
/// the series their parts).
pub fn pie(src: &Src, fixed: &[Option<u8>], d: &mut Draft, out: &mut Assignments) {
    let series = pie_series(src);
    if series.is_empty() {
        return;
    }
    let several = series.len() > 1;
    let nominal = src.x_kind() == "nominal";
    if several {
        // the totals in the chooser's order: sorted by the sorting series where nominal
        let sort = src.choice_sort(false);
        let mut totals: Vec<usize> = (0..src.categories().len()).collect();
        if matches!(sort, "desc" | "asc") && nominal {
            let by = sort_by(src);
            let val = |j: usize| src.value(by, j).unwrap_or_default();
            if sort == "desc" {
                totals.sort_by(|&a, &b| val(b).cmp(&val(a)));
            } else {
                totals.sort_by_key(|&j| val(j));
            }
        }
        let parts: Vec<Part> = series
            .iter()
            .map(|&i| Part {
                name: src.name(i),
                index: i,
                role: src.role(i).filter(|r| TAIL.contains(r)).map(str::to_owned),
                group: None,
                sizes: totals
                    .iter()
                    .map(|&j| src.value(i, j).map(|v| v.abs()).unwrap_or_default())
                    .collect(),
                colour: None,
                tint: false,
                shade: None,
                fixed: fixed[i],
            })
            .collect();
        let ordinal = src.string("/data/parts/kind", "") == "ordinal";
        let mut parts = pie_order(parts, "desc", ordinal, nominal, out);
        pie_colours(&mut parts, src, true, ordinal, out);
        for p in &parts {
            d.series[p.index] = classes(p);
        }
        d.series_order = parts.iter().map(|p| p.index).collect();
    } else {
        let parts = categories_as_parts(src, series[0]);
        let ordinal = src.x_kind() == "ordinal";
        let sort = src.choice_sort(false);
        let mut parts = pie_order(parts, sort, ordinal, nominal, out);
        pie_colours(&mut parts, src, false, ordinal, out);
        for p in &parts {
            d.categories[p.index] = classes(p);
        }
        d.category_order = parts.iter().map(|p| p.index).collect();
    }
}

/// treemap.py: one colour for a flat treemap; by group for a hierarchy, each group its own slot
/// in the order of the groups; "All others" and a part standing for what is not known their
/// reserved greys. Tiles group by group, largest first, the special parts last.
pub fn treemap(src: &Src, d: &mut Draft, out: &mut Assignments) {
    let series = pie_series(src);
    if series.len() != 1 {
        return;
    }
    let parts = categories_as_parts(src, series[0]);
    let grouped = parts.iter().any(|p| p.group.is_some())
        || src
            .get("/data/x/groups")
            .as_object()
            .is_some_and(|g| !g.is_empty());
    let live: Vec<Part> = parts
        .into_iter()
        .filter(|p| p.size() > Decimal::ZERO)
        .collect();
    let mut runs: Vec<(String, Vec<Part>)> = vec![];
    for p in live {
        let key = if grouped {
            p.group.clone().unwrap_or_else(|| p.name.clone())
        } else {
            String::new()
        };
        match runs.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(p),
            None => runs.push((key, vec![p])),
        }
    }
    let total = |m: &[Part]| m.iter().map(Part::size).sum::<Decimal>();
    runs.sort_by(|a, b| {
        total(&b.1)
            .cmp(&total(&a.1))
            .then(a.1[0].index.cmp(&b.1[0].index))
    });
    for (_, members) in &mut runs {
        members.sort_by(|a, b| {
            let ra = if in_tail(a.role()) {
                tail_index(a.role()) + 1
            } else {
                0
            };
            let rb = if in_tail(b.role()) {
                tail_index(b.role()) + 1
            } else {
                0
            };
            ra.cmp(&rb)
                .then(b.size().cmp(&a.size()))
                .then(a.index.cmp(&b.index))
        });
    }
    runs.sort_by(|a, b| {
        let ta = a.1.iter().all(|p| in_tail(p.role()));
        let tb = b.1.iter().all(|p| in_tail(p.role()));
        ta.cmp(&tb).then(total(&b.1).cmp(&total(&a.1)))
    });
    let mut k = 0;
    for (_, members) in &mut runs {
        for p in members.iter_mut() {
            if in_tail(p.role()) {
                p.colour = Some(tail_class(p.role().unwrap()).into());
            }
        }
        if !grouped {
            for p in members.iter_mut() {
                p.colour.get_or_insert_with(|| "single".into());
            }
            continue;
        }
        if members.iter().all(|p| in_tail(p.role())) {
            continue;
        }
        k += 1;
        for p in members.iter_mut() {
            p.colour
                .get_or_insert_with(|| format!("series-{}", k.min(16)));
        }
    }
    for (_, members) in &runs {
        for p in members {
            d.categories[p.index] = classes(p);
        }
    }
    d.category_order = runs
        .iter()
        .flat_map(|(_, m)| m.iter().map(|p| p.index))
        .collect();
    if grouped {
        out.decisions.push(decision(
            "R7-16",
            "locked",
            format!(
                "each group in its own colour, in the order of the groups, its name in bold at its top left: {}",
                runs.iter()
                    .map(|(g, m)| format!("{g} {}", m[0].colour.clone().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    } else {
        out.decisions.push(decision(
            "R3-3",
            "locked",
            "one colour: the parts are told apart by place and name, not by colour",
        ));
    }
}

/// wrapped.py: one series in the single colour, a special category in its reserved grey.
pub fn wrapped(src: &Src, d: &mut Draft) {
    for c in d.series.iter_mut() {
        *c = class("single");
    }
    for j in 0..src.categories().len() {
        d.categories[j] = class(
            src.x_role(j)
                .and_then(|r| role_class(r, false))
                .unwrap_or("single"),
        );
    }
}

/// A stack's parts from the base up and their colours (bars.Stack.setup): one total's parts are
/// its categories, several totals' parts the series.
pub fn stack(src: &Src, fixed: &[Option<u8>], d: &mut Draft, out: &mut Assignments) {
    let n = src.n_series();
    let single = n == 1 && src.x_kind() != "time";
    let refs = |i: usize| src.role(i).is_some_and(|r| REF_ROLES.contains(&r));
    let ordinal;
    let order_fixed;
    let mut groups = BTreeMap::<String, String>::new(); // one total: x.groups
    let mut part_groups = BTreeMap::<String, String>::new(); // several: data.parts.groups
    let mut parts: Vec<Part>;
    let mut ids: Vec<String>;
    let named_refs = src.highlight_refs();
    let named: Vec<String>;
    if single {
        ordinal = src.x_kind() == "ordinal";
        order_fixed = src.string("/data/parts/order", "auto") == "fixed"
            || src.string("/data/x/order", "auto") == "fixed";
        parts = vec![];
        ids = vec![];
        for j in 0..src.categories().len() {
            let role = src.x_role(j);
            if role == Some("total") {
                continue;
            }
            parts.push(Part {
                name: src.categories()[j].clone(),
                index: j,
                role: Some(
                    if in_tail(role) {
                        role.unwrap()
                    } else {
                        "regular"
                    }
                    .into(),
                ),
                group: None,
                sizes: vec![src.value(0, j).map(|v| v.abs()).unwrap_or_default()],
                colour: None,
                tint: false,
                shade: None,
                fixed: None,
            });
            ids.push(src.categories()[j].clone());
        }
        if let Some(g) = src.get("/data/x/groups").as_object() {
            for (k, v) in g {
                if let Some(v) = v.as_str() {
                    groups.insert(k.clone(), v.into());
                }
            }
        }
        named = named_refs
            .iter()
            .filter_map(|r| {
                r["category"]
                    .as_str()
                    .or_else(|| r["x"].as_str())
                    .or_else(|| {
                        r["series"]
                            .as_str()
                            .filter(|_| r["category"].is_null() && r["x"].is_null())
                    })
            })
            .filter(|n| src.categories().iter().any(|c| c == n))
            .map(str::to_owned)
            .collect();
    } else {
        ordinal = src.string("/data/parts/kind", "") == "ordinal";
        order_fixed = src.string("/data/parts/order", "auto") == "fixed";
        parts = (0..n)
            .filter(|&i| !refs(i))
            .map(|i| Part {
                name: src.name(i),
                index: i,
                role: Some(src.role(i).unwrap_or("regular").into()),
                group: None,
                sizes: vec![
                    src.data.series[i]
                        .values
                        .iter()
                        .filter_map(|d| d.value)
                        .map(|v| v.abs())
                        .sum(),
                ],
                colour: None,
                tint: false,
                shade: None,
                fixed: fixed[i],
            })
            .collect();
        ids = parts.iter().map(|p| src.id(p.index).to_owned()).collect();
        if let Some(g) = src.get("/data/parts/groups").as_object() {
            for (k, v) in g {
                if let Some(v) = v.as_str() {
                    part_groups.insert(k.clone(), v.into());
                }
            }
        }
        named = named_refs
            .iter()
            .filter(|r| r["category"].is_null() && r["x"].is_null())
            .filter_map(|r| r["series"].as_str().map(str::to_owned))
            .collect();
    }
    let id_of = |p: &Part| -> String {
        if single {
            src.categories()[p.index].clone()
        } else {
            src.id(p.index).to_owned()
        }
    };
    let _ = &mut ids;
    let tail = |p: &Part| in_tail(p.role());
    let regular_role = |p: &Part| is_regular(p.role());
    let mut group_runs: Vec<(String, Vec<usize>)> = vec![]; // indices into `ordered`
    let mut ordered: Vec<Part>;
    if ordinal {
        ordered = parts.clone();
        out.decisions.push(decision(
            "R2-02",
            "locked",
            "the parts are ordered categories: stacked in their own order, the first at the base (G2.2)",
        ));
    } else if order_fixed {
        // _given: a group's parts together, the groups in the order their first part comes
        let key = |p: &Part| -> String {
            let id = id_of(p);
            if !groups.is_empty() {
                groups.get(&id).cloned().unwrap_or(id)
            } else if !part_groups.is_empty() {
                part_groups
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| p.name.clone())
            } else {
                id
            }
        };
        let mut runs: Vec<(String, Vec<Part>)> = vec![];
        for p in &parts {
            let k = key(p);
            match runs.iter_mut().find(|(r, _)| *r == k) {
                Some((_, v)) => v.push(p.clone()),
                None => runs.push((k, vec![p.clone()])),
            }
        }
        if !groups.is_empty() {
            for (_, members) in &mut runs {
                for (j, p) in members.iter_mut().enumerate() {
                    p.group = groups.get(&id_of(p)).cloned();
                    p.tint = j > 0;
                }
            }
        }
        ordered = runs.iter().flat_map(|(_, m)| m.iter().cloned()).collect();
        if !part_groups.is_empty() {
            let mut pos = 0;
            let mut plain = vec![];
            let mut with_tail = vec![];
            for (g, m) in &runs {
                let idx: Vec<usize> = (pos..pos + m.len()).collect();
                pos += m.len();
                if m.iter().any(tail) {
                    with_tail.push((g.clone(), idx));
                } else {
                    plain.push((g.clone(), idx));
                }
            }
            group_runs = plain.into_iter().chain(with_tail).collect();
        }
        let size_of = |p: &Part| p.size();
        let grouped = !groups.is_empty() || !part_groups.is_empty();
        let (differs, would) = if grouped {
            let given: Vec<String> = runs.iter().map(|(g, _)| g.clone()).collect();
            let mut by_size = runs.clone();
            by_size.sort_by(|a, b| {
                let sa: Decimal = a.1.iter().map(size_of).sum();
                let sb: Decimal = b.1.iter().map(size_of).sum();
                sb.cmp(&sa)
            });
            let by: Vec<String> = by_size.into_iter().map(|(g, _)| g).collect();
            (
                by != given,
                format!("the largest group first they would stack {}", by.join(", ")),
            )
        } else {
            let mut by: Vec<&Part> = parts.iter().collect();
            by.sort_by(|a, b| b.size().cmp(&a.size()));
            let names: Vec<&str> = by.iter().map(|p| p.name.as_str()).collect();
            let given: Vec<&str> = ordered.iter().map(|p| p.name.as_str()).collect();
            (
                names != given,
                format!("largest first they would stack {}", names.join(", ")),
            )
        };
        out.decisions.push(decision(
            "R7-24",
            "guided",
            format!(
                "stacking order from the base: {} (the data's given order, which is fixed: data.parts.order; owner, 1 Oct 2026{}{}; G7.31, D-26)",
                ordered.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "),
                if grouped { "; each group's parts together" } else { "" },
                if differs { format!("; {would}") } else { String::new() }
            ),
        ));
    } else if !groups.is_empty() {
        let mut others: Vec<Part> = parts.iter().filter(|p| tail(p)).cloned().collect();
        others.sort_by_key(|p| tail_index(p.role()));
        let mut runs: Vec<(String, Vec<Part>)> = vec![];
        for p in parts.iter().filter(|p| !tail(p)) {
            let id = id_of(p);
            let k = groups.get(&id).cloned().unwrap_or(id);
            match runs.iter_mut().find(|(r, _)| *r == k) {
                Some((_, v)) => v.push(p.clone()),
                None => runs.push((k, vec![p.clone()])),
            }
        }
        runs.sort_by(|a, b| {
            let sa: Decimal = a.1.iter().map(Part::size).sum();
            let sb: Decimal = b.1.iter().map(Part::size).sum();
            sb.cmp(&sa).then(a.1[0].index.cmp(&b.1[0].index))
        });
        for (_, m) in &mut runs {
            for (j, p) in m.iter_mut().enumerate() {
                p.group = groups.get(&id_of(p)).cloned();
                p.tint = j > 0;
            }
        }
        ordered = runs
            .into_iter()
            .flat_map(|(_, m)| m)
            .chain(others)
            .collect();
        out.decisions.push(decision(
            "R7-24",
            "guided",
            format!(
                "stacking order from the base: {} (by group, the largest at the base, each group's parts together in their given order: 07 #10, D-24 (b), G7.31)",
                ordered.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
            ),
        ));
    } else if !part_groups.is_empty() {
        let mut others: Vec<Part> = parts.iter().filter(|p| tail(p)).cloned().collect();
        others.sort_by_key(|p| tail_index(p.role()));
        let mut runs: Vec<(String, Vec<Part>)> = vec![];
        for p in parts.iter().filter(|p| !tail(p)) {
            let k = part_groups
                .get(&id_of(p))
                .cloned()
                .unwrap_or_else(|| p.name.clone());
            match runs.iter_mut().find(|(r, _)| *r == k) {
                Some((_, v)) => v.push(p.clone()),
                None => runs.push((k, vec![p.clone()])),
            }
        }
        runs.sort_by(|a, b| {
            let sa: Decimal = a.1.iter().map(Part::size).sum();
            let sb: Decimal = b.1.iter().map(Part::size).sum();
            sb.cmp(&sa).then(a.1[0].index.cmp(&b.1[0].index))
        });
        let mut pos = 0;
        for (g, m) in &runs {
            group_runs.push((g.clone(), (pos..pos + m.len()).collect()));
            pos += m.len();
        }
        let text = runs
            .iter()
            .map(|(g, m)| {
                format!(
                    "{g}, {}",
                    m.iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        ordered = runs
            .into_iter()
            .flat_map(|(_, m)| m)
            .chain(others)
            .collect();
        out.decisions.push(decision(
            "R7-24",
            "guided",
            format!(
                "stacking order from the base: {} (by group: {text}; the largest group at the base, each group's parts together in their given order: G4.9, G7.31, D-26)",
                ordered.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
            ),
        ));
    } else {
        let mut others: Vec<Part> = parts.iter().filter(|p| tail(p)).cloned().collect();
        others.sort_by_key(|p| tail_index(p.role()));
        let regular: Vec<Part> = parts.iter().filter(|p| !tail(p)).cloned().collect();
        let mut feat: Vec<Part> = regular
            .iter()
            .filter(|p| named.contains(&id_of(p)))
            .cloned()
            .collect();
        feat.sort_by(|a, b| b.size().cmp(&a.size()));
        let mut rest: Vec<Part> = regular
            .iter()
            .filter(|p| !named.contains(&id_of(p)))
            .cloned()
            .collect();
        rest.sort_by(|a, b| b.size().cmp(&a.size()));
        let mut why = vec![];
        if !feat.is_empty() {
            let sub = matches!(
                src.string("/facts/message", ""),
                "subtotal_amount" | "subtotal_fraction"
            );
            why.push(format!(
                "{}, {}",
                feat.iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" and "),
                if sub {
                    "the parts of the subtotal the message features, together at the base (R7-08)"
                } else {
                    "the parts the message names, at the base, where they are read precisely"
                }
            ));
        }
        why.push(
            if feat.is_empty() {
                "the largest at the base, where it is read precisely"
            } else {
                "the others largest first"
            }
            .to_owned(),
        );
        if !others.is_empty() {
            why.push(format!(
                "{} at the top (R7-17)",
                others
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        ordered = feat.into_iter().chain(rest).chain(others).collect();
        out.decisions.push(decision(
            "R7-24",
            "guided",
            format!(
                "stacking order from the base: {} ({}; G7.31, D-26 (b))",
                ordered
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                why.join("; ")
            ),
        ));
    }
    // colours by the order of the stack, the same in every total (R7-24, G4.22, D-22 (a))
    let fixed_slots: Vec<u8> = ordered.iter().filter_map(|p| p.fixed).collect();
    let any_fixed = !fixed_slots.is_empty();
    let mut free = (1..=16u8).filter(|k| !fixed_slots.contains(k));
    let regular: Vec<usize> = (0..ordered.len())
        .filter(|&i| regular_role(&ordered[i]))
        .collect();
    let mut seen = BTreeMap::<String, String>::new();
    for i in 0..ordered.len() {
        let p = &ordered[i];
        let colour = if !regular_role(p) {
            role_class(p.role().unwrap_or(""), false)
                .unwrap_or("single")
                .to_owned()
        } else if p.tint && p.group.as_ref().is_some_and(|g| seen.contains_key(g)) {
            seen[p.group.as_ref().unwrap()].clone()
        } else if regular.len() == 1 && !any_fixed {
            "single".to_owned()
        } else {
            let c = format!(
                "series-{}",
                p.fixed.unwrap_or_else(|| free.next().unwrap_or(16))
            );
            if let Some(g) = &p.group {
                seen.insert(g.clone(), c.clone());
            }
            c
        };
        ordered[i].colour = Some(colour);
    }
    if !group_runs.is_empty() {
        shades(src, &mut ordered, &mut group_runs, out);
    }
    let limit = SEQ_STEPS - SEQ_LIGHTEST + 1;
    if ordinal && !any_fixed && regular.len() > 1 && regular.len() <= limit {
        for (&i, step) in regular.iter().zip(ordered_steps(regular.len())) {
            ordered[i].colour = Some(format!("seq-{step}"));
        }
        out.decisions.push(decision(
            "R3-8",
            "guided",
            "the parts are ordered: steps of one hue, darker for each part from the last to the first, the first at the base; the palest steps, lighter than the gridlines, left out (G4.7, G3.2)",
        ));
    } else if ordinal && !any_fixed && regular.len() > limit {
        out.findings.push(finding(
            "R3-8",
            "warn",
            "guided",
            format!(
                "{} ordered parts: more than {limit} steps of one hue cannot be told apart; a hue each (G4.7, G4.21)",
                regular.len()
            ),
            Some("data.parts.kind".into()),
        ));
    }
    let totals = if single { 1 } else { src.categories().len() };
    out.decisions.push(decision(
        "R7-24",
        "locked",
        format!(
            "each part in one colour and one place in every total: {}{}",
            ordered
                .iter()
                .map(|p| format!(
                    "{} ({}{}{})",
                    p.name,
                    words(p.colour.as_deref().unwrap_or("")),
                    if p.tint { ", its tint" } else { "" },
                    match p.shade {
                        Some("shade-light") => ", light",
                        Some("shade-dark") => ", dark",
                        _ => "",
                    }
                ))
                .collect::<Vec<_>>()
                .join(", "),
            if totals > 1 {
                " (G4.22, G7.31)"
            } else {
                " (G4.22)"
            }
        ),
    ));
    for p in &ordered {
        if single {
            d.categories[p.index] = classes(p);
        } else {
            d.series[p.index] = classes(p);
        }
    }
    if single {
        d.category_order = ordered.iter().map(|p| p.index).collect();
        // the one series is not drawn as such (bars.parts_form); a fallback paint for its group
        d.series[0] = class("single");
    } else {
        d.series_order = ordered.iter().map(|p| p.index).collect();
        for i in 0..n {
            if refs(i) {
                d.series[i] = class("reference");
            }
        }
    }
}

/// bars.Stack._shades and ramps.grouped: each group a hue (palette slot g from the base, or the
/// fixed slot of its first part), its parts that hue's shades, the darkest in the middle.
fn shades(
    src: &Src,
    ordered: &mut [Part],
    runs: &mut Vec<(String, Vec<usize>)>,
    out: &mut Assignments,
) {
    if ordered.iter().any(|p| in_tail(p.role())) {
        out.findings.push(finding(
            "R3-11",
            "warn",
            "guided",
            "parts in groups with an All others part, whose grey stands outside every group: the grouped legend cannot show it yet, so the parts take a hue each and are named one by one (a later stage)",
            Some("data.parts.groups".into()),
        ));
        runs.clear();
        return;
    }
    let mut assigned = vec![];
    for (gi, (g, members)) in runs.iter().enumerate() {
        let k = ordered[members[0]].fixed.unwrap_or((gi + 1).min(16) as u8);
        if members.len() > 3 {
            out.findings.push(finding(
                "R3-11",
                "warn",
                "guided",
                format!(
                    "group {g:?} has {} members: at most three shades of one hue stay apart without reading as an order (G4.9); merge or group them further (G4.21): a hue each instead, and the parts named one by one",
                    members.len()
                )
                .replace('"', "'"),
                Some("data.parts.groups".into()),
            ));
            runs.clear();
            return;
        }
        let order: &[Option<&'static str>] = match members.len() {
            1 => &[None],
            2 => &[Some("shade-light"), Some("shade-dark")],
            _ => &[None, Some("shade-dark"), Some("shade-light")],
        };
        for (&m, shade) in members.iter().zip(order) {
            assigned.push((m, format!("series-{k}"), *shade));
        }
    }
    for (m, c, shade) in assigned {
        ordered[m].colour = Some(c);
        ordered[m].shade = shade;
    }
    out.decisions.push(decision(
        "R3-11",
        "guided",
        format!(
            "the parts by group, each group a hue and its parts shades of it, not in lightness order (the darkest in the middle), so that they read as members, not amounts: {} (G4.9, D-49)",
            runs.iter()
                .map(|(g, m)| format!(
                    "{g} ({}: {})",
                    words(ordered[m[0]].colour.as_deref().unwrap_or("")),
                    m.iter()
                        .map(|&i| format!(
                            "{} {}",
                            ordered[i].name,
                            match ordered[i].shade {
                                Some("shade-light") => "light",
                                Some("shade-dark") => "dark",
                                _ => "base",
                            }
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    ));
    let _ = src;
    let _: Option<&Value> = None;
}
