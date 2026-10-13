//! Pie charts: one total broken down into its parts, or several totals in a row of pies;
//! a port of the original `layout/pie.py` (SPEC §6.11; R7-32–R7-39; G7.25–G7.30; D-34, D-51).
//!
//! The slices run clockwise from 12 o'clock, largest first, "All others" and zeros last; each
//! is labelled outside, on its middle ray, the blocks of each half packed down the page and
//! pushed clear of the circle, with a leader to a zero, a thin slice or a moved label. The
//! radius shrinks from --pie-radius of the width until the labels fit, down to --pie-min-radius.
//! True gaps between slices, no outlines. The highlight is the strong shade, or a border kept
//! inside the slice; exploding, a legend, a bigger slice and greying the others are refused
//! unless overridden.
use super::kit::{self, Kit, Placed, err, finding, guided, info, meta, r2, refusal};
use chartgen_format as fmt;
use chartgen_model::*;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const TAIL: [&str; 2] = ["all_others", "missing"];
const DEFER: [&str; 5] = [
    "glow",
    "background",
    "icon",
    "selective_label",
    "bold_label",
];
const SEQ_STEPS: usize = 9;
const SEQ_LIGHTEST: usize = 3;

#[derive(Debug, Clone)]
pub(crate) struct Part {
    pub name: String,
    pub content: TextContent,
    pub index: usize,
    pub role: Option<String>,
    pub group: Option<String>,
    pub values: Vec<Decimal>,
    pub colour: Option<String>,
    pub tint: bool,
    pub target: bool,
    pub key: String,
}

struct Pie {
    id: String,
    content: TextContent,
}

#[derive(Clone)]
struct Block {
    part: usize,
    lines: Vec<(TextContent, kit::Ext)>,
    w: f64,
    h: f64,
    asc: f64,
    theta: f64,
    span: f64,
    zero: bool,
    side: i32,
    ideal: (f64, f64),
    x: f64,
    y: f64,
    leader: bool,
}
impl Block {
    fn bbox(&self, pad: f64) -> [f64; 4] {
        [
            self.x - self.w / 2. - pad,
            self.y - self.h / 2. - pad,
            self.x + self.w / 2. + pad,
            self.y + self.h / 2. + pad,
        ]
    }
}

fn ray(cx: f64, cy: f64, theta: f64, d: f64) -> (f64, f64) {
    let t = theta.to_radians();
    (cx + d * t.sin(), cy - d * t.cos())
}

/// The centres of blocks of these heights, in the order given (top to bottom), at least `gap`
/// apart between neighbours' edges; each run that would crowd centred on its targets.
fn pack(targets: &[f64], heights: &[f64], gap: f64, order: &[usize]) -> Vec<f64> {
    let mut clusters: Vec<Vec<usize>> = order.iter().map(|&i| vec![i]).collect();
    let mut ys = targets.to_vec();
    loop {
        for c in &clusters {
            let mut offs = vec![0.];
            let mut acc = 0.;
            for w in c.windows(2) {
                acc += (heights[w[0]] + heights[w[1]]) / 2. + gap;
                offs.push(acc);
            }
            let start = c
                .iter()
                .zip(&offs)
                .map(|(&i, o)| targets[i] - o)
                .sum::<f64>()
                / c.len() as f64;
            for (&i, o) in c.iter().zip(&offs) {
                ys[i] = start + o;
            }
        }
        let mut merged = false;
        for a in 0..clusters.len().saturating_sub(1) {
            let p = *clusters[a].last().unwrap();
            let q = clusters[a + 1][0];
            if ys[q] - ys[p] < (heights[p] + heights[q]) / 2. + gap - 1e-6 {
                let next = clusters.remove(a + 1);
                clusters[a].extend(next);
                merged = true;
                break;
            }
        }
        if !merged {
            return ys;
        }
    }
}

pub(crate) fn text_of(v: &Value, fallback: &str) -> Result<(TextContent, String)> {
    match kit::spec_text(v)? {
        Some(c) => {
            let plain = c.readable().to_string();
            Ok((c, plain))
        }
        None => Ok((TextContent::plain(fallback), fallback.to_string())),
    }
}

fn known(d: &Datum) -> bool {
    crate::table::known(d)
}

/// The sizes of the cells, in order; None (refused) when a value is not known (R0-12).
fn sizes(
    spec: &Value,
    findings: &mut Vec<Finding>,
    cells: &[(String, Option<&Datum>, String)],
) -> Option<Vec<Decimal>> {
    let mut out = vec![];
    let mut stop = false;
    for (name, v, path) in cells {
        match v {
            Some(d) if known(d) => out.push(d.value.unwrap().abs()),
            _ => {
                stop |= refusal(
                    spec,
                    findings,
                    "R0-12",
                    &format!(
                        "{name}'s value is not known: every slice is a fraction of the total, and a part not known leaves the total and every fraction unknown"
                    ),
                    Some(path),
                    Some(
                        "the value; or a bar chart of the parts that are known, which claims no total (G0.13)",
                    ),
                );
                out.push(Decimal::ZERO);
            }
        }
    }
    (!stop).then_some(out)
}

pub(crate) fn series_rows(spec: &Value, data: &PreparedData) -> Vec<usize> {
    (0..data.series.len())
        .filter(|&i| {
            matches!(
                spec["data"]["series"][i]["role"].as_str(),
                None | Some("regular" | "all_others" | "missing")
            )
        })
        .collect()
}

/// The parts clockwise from 12 o'clock (pie.order): by group, largest group first; or largest
/// first (smallest with sort asc; ordered parts in their order), the special parts and zeros last.
pub(crate) fn order(parts: Vec<Part>, sort: &str, key: usize, ordinal: bool) -> Vec<Part> {
    let size = |p: &Part| p.values[key];
    if parts.iter().any(|p| p.group.is_some()) {
        let mut groups: Vec<(String, Vec<Part>)> = vec![];
        for p in parts {
            let g = p.group.clone().unwrap_or_else(|| p.name.clone());
            match groups.iter_mut().find(|(k, _)| *k == g) {
                Some((_, v)) => v.push(p),
                None => groups.push((g, vec![p])),
            }
        }
        groups.sort_by(|a, b| {
            let sa: Decimal = a.1.iter().map(size).sum();
            let sb: Decimal = b.1.iter().map(size).sum();
            sb.cmp(&sa).then(a.1[0].index.cmp(&b.1[0].index))
        });
        return groups.into_iter().flat_map(|(_, v)| v).collect();
    }
    let is_tail = |p: &Part| p.role.as_deref().is_some_and(|r| TAIL.contains(&r));
    let mut others: Vec<Part> = parts.iter().filter(|p| is_tail(p)).cloned().collect();
    others.sort_by_key(|p| TAIL.iter().position(|t| Some(*t) == p.role.as_deref()));
    let (mut head, zeros): (Vec<Part>, Vec<Part>) = if sort == "given" || ordinal {
        (
            parts.iter().filter(|p| !is_tail(p)).cloned().collect(),
            vec![],
        )
    } else {
        (
            parts
                .iter()
                .filter(|p| !is_tail(p) && size(p) > Decimal::ZERO)
                .cloned()
                .collect(),
            parts
                .iter()
                .filter(|p| !is_tail(p) && size(p) == Decimal::ZERO)
                .cloned()
                .collect(),
        )
    };
    if !(sort == "given" || ordinal) {
        if sort == "asc" {
            head.sort_by(|a, b| size(a).cmp(&size(b)).then(b.index.cmp(&a.index)));
        } else {
            head.sort_by(|a, b| size(b).cmp(&size(a)).then(a.index.cmp(&b.index)));
        }
    }
    head.into_iter().chain(others).chain(zeros).collect()
}

/// Colour classes in slice order (pie.colours, R7-32, D-22 (a)): palette slots handed out in
/// the order of the slices, which is this layout's; fixed slots kept (colours.slots by part
/// name for one total, series.slot for several); reserved greys for "All others" and missing;
/// a group's second part a tint of its first; ordered parts steps of one hue.
///
/// The palette's semantic plan (chartgen-colours) assigns category slots in the grammar's
/// order, which is not the slice order (grouped parts: p3-07), so the slices take their slots
/// here, as the original's pie does.
pub(crate) fn colours(
    parts: &mut [Part],
    plan: &SemanticPlan,
    several: bool,
    ordinal: bool,
    findings: &mut Vec<Finding>,
) {
    let spec = &plan.spec.raw;
    let mut fixed: BTreeMap<String, u64> = BTreeMap::new();
    if several {
        for p in parts.iter() {
            if let Some(k) = spec["data"]["series"][p.index]["slot"].as_u64() {
                fixed.insert(p.key.clone(), k);
            }
        }
    } else if let Some(slots) = spec.pointer("/colours/slots").and_then(Value::as_object) {
        let mut taken: BTreeMap<u64, String> = BTreeMap::new();
        for p in parts.iter() {
            let Some(k) = slots.get(&p.name).and_then(Value::as_u64) else {
                continue;
            };
            if p.role.is_some() {
                continue;
            }
            if let Some(first) = taken.get(&k) {
                let mut f = finding(
                    "R3-24",
                    "warn",
                    &format!(
                        "{first} and {} would share palette slot {k}: {} takes a free slot (each slice its own colour, G7.25)",
                        p.name, p.name
                    ),
                    Some("colours.slots"),
                );
                f.level = "locked".into();
                findings.push(f);
                continue;
            }
            fixed.insert(p.key.clone(), k);
            taken.insert(k, p.name.clone());
        }
    }
    let mut used: BTreeSet<u64> = fixed.values().copied().collect();
    let mut free = move || {
        let k = (1..=16).find(|k| !used.contains(k)).unwrap_or(16);
        used.insert(k);
        k
    };
    let mut seen: BTreeMap<String, (String, usize)> = BTreeMap::new();
    let mut k = 0;
    for p in parts.iter_mut() {
        if let Some(r) = p.role.as_deref().filter(|r| TAIL.contains(r)) {
            p.colour = Some(
                if r == "all_others" {
                    "all-others"
                } else {
                    "missing"
                }
                .into(),
            );
            continue;
        }
        if p.values.iter().all(|v| v.is_zero()) {
            continue;
        }
        if let Some(slot) = fixed.get(&p.key) {
            k += 1;
            p.colour = Some(format!("series-{slot}"));
            continue;
        }
        if let Some(g) = &p.group {
            if let Some((first, n)) = seen.get(g).cloned() {
                p.colour = Some(first.clone());
                p.tint = true;
                seen.insert(g.clone(), (first, n + 1));
                if n + 1 > 2 {
                    findings.push(info(
                        "CG-3",
                        &format!("a group of more than two parts ({g}): its third part and after share the second's tint"),
                        Some("data.x.groups"),
                    ));
                }
                continue;
            }
        }
        k += 1;
        let colour = format!("series-{}", free().min(16));
        if let Some(g) = &p.group {
            seen.insert(g.clone(), (colour.clone(), 1));
        }
        p.colour = Some(colour);
    }
    let regular: Vec<usize> = (0..parts.len())
        .filter(|&i| {
            parts[i].colour.is_some()
                && !parts[i].role.as_deref().is_some_and(|r| TAIL.contains(&r))
        })
        .collect();
    let n = regular.len();
    if ordinal && fixed.is_empty() && seen.is_empty() && n > 1 && n <= SEQ_STEPS - SEQ_LIGHTEST + 1
    {
        for (j, &i) in regular.iter().enumerate() {
            let step = (SEQ_STEPS as f64
                - (SEQ_STEPS - SEQ_LIGHTEST) as f64 * j as f64 / (n - 1) as f64
                + 0.5) as usize;
            parts[i].colour = Some(format!("seq-{step}"));
        }
    } else if ordinal && fixed.is_empty() && n > SEQ_STEPS - SEQ_LIGHTEST + 1 {
        let mut f = finding(
            "R3-8",
            "warn",
            &format!(
                "{n} ordered parts: more than {} steps of one hue cannot be told apart; a hue each (G4.7, G4.21)",
                SEQ_STEPS - SEQ_LIGHTEST + 1
            ),
            Some(if several {
                "data.parts.kind"
            } else {
                "data.x.kind"
            }),
        );
        f.level = "guided".into();
        findings.push(f);
    }
    if k > 8 {}
}

/// A part's name, wrapped onto two lines when wider than `wrap` (pie._name_lines).
fn name_lines(kit: &Kit, p: &Part, wrap: f64) -> Result<Vec<TextContent>> {
    let TextContent::Plain { text } = &p.content else {
        return Ok(vec![p.content.clone()]);
    };
    let width = |s: &str| kit.width("slice-label", &[], &kit::plain(s));
    let lines = match kit::best_lines(text, Some(wrap), 2, width)? {
        Some(l) => l,
        None => {
            let opts = kit::splits(text);
            let mut best: Option<(f64, Vec<String>)> = None;
            for (a, b) in opts {
                let w = width(&a)?.max(width(&b)?);
                if best.as_ref().is_none_or(|(bw, _)| w < *bw) {
                    best = Some((w, vec![a, b]));
                }
            }
            best.map(|b| b.1).unwrap_or_else(|| vec![text.clone()])
        }
    };
    Ok(lines.iter().map(|s| kit::plain(s)).collect())
}

/// Place the label blocks round a pie of radius r centred on (0, 0) (pie._place).
fn place(
    kit: &Kit,
    blocks: &mut [Block],
    ang: &[(f64, f64)],
    r: f64,
    push: &BTreeMap<usize, f64>,
) -> Result<[f64; 4]> {
    let em = kit.metric("--label-size")?;
    let r0 = r * (1. + kit.metric("--pie-label-gap")?);
    let sep = 0.5 * em;
    let min_angle = kit.metric("--pie-leader-angle")?;
    for (b, &(a0, a1)) in blocks.iter_mut().zip(ang) {
        let rr = r0 + push.get(&b.part).copied().unwrap_or(0.);
        b.span = a1 - a0;
        b.zero = b.span <= 1e-9;
        b.theta = if !b.zero {
            (a0 + a1) / 2.
        } else {
            a0.rem_euclid(360.)
        };
        b.leader = false;
        let th = b.theta.rem_euclid(360.);
        if b.zero && (th < 1e-6 || th > 360. - 1e-6) {
            b.side = 1;
            b.ideal = (b.w / 2. + 0.6 * em, -rr - b.h / 2.);
        } else {
            b.side = if 0. < th && th < 180. {
                1
            } else if th > 180. {
                -1
            } else if th == 0. {
                1
            } else {
                -1
            };
            let t = th.to_radians();
            let hr = (b.w / 2. * t.sin()).abs() + (b.h / 2. * t.cos()).abs();
            b.ideal = ((rr + hr) * t.sin(), -(rr + hr) * t.cos());
        }
        b.x = b.ideal.0;
        b.y = b.ideal.1;
    }
    for side in [1, -1] {
        let idx: Vec<usize> = (0..blocks.len())
            .filter(|&i| blocks[i].side == side)
            .collect();
        if idx.is_empty() {
            continue;
        }
        let mut down: Vec<usize> = (0..idx.len()).collect();
        let key = |i: usize| {
            let b = &blocks[idx[i]];
            let t = b.theta.rem_euclid(360.);
            (
                if side == 1 {
                    t
                } else {
                    (360. - t).rem_euclid(360.)
                },
                b.ideal.1,
            )
        };
        down.sort_by(|&a, &b| key(a).partial_cmp(&key(b)).unwrap());
        let targets: Vec<f64> = idx.iter().map(|&i| blocks[i].ideal.1).collect();
        let heights: Vec<f64> = idx.iter().map(|&i| blocks[i].h).collect();
        let ys = pack(&targets, &heights, sep, &down);
        for (&i, y) in idx.iter().zip(ys) {
            let b = &mut blocks[i];
            b.y = y;
            let rr = r0 + push.get(&b.part).copied().unwrap_or(0.);
            let dy = if b.y - b.h / 2. <= 0. && 0. <= b.y + b.h / 2. {
                0.
            } else {
                (b.y - b.h / 2.).abs().min((b.y + b.h / 2.).abs())
            };
            let need = (rr * rr - dy * dy).max(0.).sqrt() + b.w / 2.;
            if side == 1 {
                b.x = if dy < rr {
                    b.ideal.0.max(need)
                } else {
                    b.ideal.0
                };
            } else {
                b.x = if dy < rr {
                    b.ideal.0.min(-need)
                } else {
                    b.ideal.0
                };
            }
        }
    }
    for _ in 0..3 {
        let mut moved = false;
        let right: Vec<usize> = (0..blocks.len()).filter(|&i| blocks[i].side == 1).collect();
        let left: Vec<usize> = (0..blocks.len())
            .filter(|&i| blocks[i].side == -1)
            .collect();
        for &a in &right {
            for &c in &left {
                let aa = blocks[a].bbox(sep / 2.);
                let cc = blocks[c].bbox(sep / 2.);
                if chartgen_geometry::Bbox::new(aa[0], aa[1], aa[2], aa[3])
                    .overlaps(&chartgen_geometry::Bbox::new(cc[0], cc[1], cc[2], cc[3]))
                {
                    let p = (cc[2] - aa[0]) / 2. + 0.01;
                    blocks[a].x += p;
                    blocks[c].x -= p;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    let run = kit.metric("--leader-run")?;
    for b in blocks.iter_mut() {
        let moved = (b.x - b.ideal.0).hypot(b.y - b.ideal.1);
        b.leader = b.zero || b.span < min_angle || moved > b.h / 2. + 0.01;
        if b.leader && b.ideal.1 != -r0 - b.h / 2. {
            b.x += b.side as f64 * run;
        }
    }
    for side in [1, -1] {
        let mut led: Vec<usize> = (0..blocks.len())
            .filter(|&i| {
                let b = &blocks[i];
                b.leader && b.side == side && b.ideal.1 != -r0 - b.h / 2.
            })
            .collect();
        if led.len() < 2 {
            continue;
        }
        let y0 = led
            .iter()
            .map(|&i| blocks[i].y)
            .fold(f64::INFINITY, f64::min);
        let y1 = led
            .iter()
            .map(|&i| blocks[i].y)
            .fold(f64::NEG_INFINITY, f64::max);
        for i in 0..blocks.len() {
            let b = &blocks[i];
            if b.side == side
                && y0 <= b.y
                && b.y <= y1
                && !led.contains(&i)
                && b.ideal.1 != -r0 - b.h / 2.
            {
                blocks[i].leader = true;
                blocks[i].x += side as f64 * run;
                led.push(i);
            }
        }
        if side == 1 {
            let edge = led
                .iter()
                .map(|&i| blocks[i].x - blocks[i].w / 2.)
                .fold(f64::NEG_INFINITY, f64::max);
            for &i in &led {
                blocks[i].x = edge + blocks[i].w / 2.;
            }
        } else {
            let edge = led
                .iter()
                .map(|&i| blocks[i].x + blocks[i].w / 2.)
                .fold(f64::INFINITY, f64::min);
            for &i in &led {
                blocks[i].x = edge - blocks[i].w / 2.;
            }
        }
    }
    let mut bb = [-r, -r, r, r];
    for b in blocks.iter() {
        let x = b.bbox(0.);
        bb[0] = bb[0].min(x[0]);
        bb[1] = bb[1].min(x[1]);
        bb[2] = bb[2].max(x[2]);
        bb[3] = bb[3].max(x[3]);
    }
    Ok(bb)
}

/// A slice's figkit geometry (chartgen-geometry `Wedge`).
fn geom(
    x: f64,
    y: f64,
    r: f64,
    a0: f64,
    a1: f64,
    r0: f64,
    inset: f64,
) -> Result<chartgen_geometry::Wedge> {
    chartgen_geometry::Wedge::new(x, y, r, a0, a1, r0, inset)
        .map_err(|e| err(format!("slice: {e}")))
}

/// The bounding box of a drawn slice (figkit Wedge.box).
pub(crate) fn wedge_box(
    x: f64,
    y: f64,
    r: f64,
    a0: f64,
    a1: f64,
    r0: f64,
    inset: f64,
) -> Result<[f64; 4]> {
    let w = geom(x, y, r, a0, a1, r0, inset)?;
    Ok(w.bbox(0.)
        .map_err(|e| err(format!("slice: {e}")))?
        .as_array())
}

/// A slice's outline: the figkit wedge's path (chartgen-geometry `Wedge::path`, lines and arcs,
/// as emit.py writes it); None when nothing is drawn (a sliver narrower than its gaps).
fn wedge(
    x: f64,
    y: f64,
    r: f64,
    a0: f64,
    a1: f64,
    r0: f64,
    inset: f64,
) -> Result<Option<Vec<PathCommand>>> {
    let w = geom(x, y, r, a0, a1, r0, inset)?;
    let commands = w.path().map_err(|e| err(format!("slice: {e}")))?;
    Ok((!commands.is_empty()).then(|| kit::scene_commands(&commands)))
}

fn dec_f(v: Decimal) -> f64 {
    v.to_f64().unwrap_or(0.)
}

/// One total, its parts the categories, each with its size (pie.parts_of); None when refused
/// (a part not known, R0-12; every part zero, CG-4).
pub(crate) fn parts_of(
    spec: &Value,
    data: &PreparedData,
    i: usize,
    findings: &mut Vec<Finding>,
) -> Result<Option<Vec<Part>>> {
    let x = &spec["data"]["x"];
    let xs = &data.categories;
    let s = &data.series[i];
    let cells: Vec<(String, Option<&Datum>, String)> = xs
        .iter()
        .enumerate()
        .map(|(j, name)| {
            (
                name.clone(),
                s.values.get(j),
                format!("data.series[{i}].values[{j}]"),
            )
        })
        .collect();
    let Some(sz) = sizes(spec, findings, &cells) else {
        return Ok(None);
    };
    if sz.iter().all(|v| v.is_zero()) {
        findings.push(finding(
            "CG-4",
            "error",
            "every part is zero: there is no total to break down",
            Some("data.series"),
        ));
        return Ok(None);
    }
    xs.iter()
        .enumerate()
        .map(|(j, name)| {
            let (content, plain) = text_of(&x["names"][name], name)?;
            Ok(Part {
                name: plain,
                content,
                index: j,
                role: x["roles"][name].as_str().map(str::to_owned),
                group: x["groups"][name].as_str().map(str::to_owned),
                values: vec![sz[j]],
                colour: None,
                tint: false,
                target: false,
                key: name.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// The parts in the colour plan's order with its colour classes (`series_order` and
/// `series_classes` for several totals, `category_order` and `category_classes` for one); None
/// when the plan carries none.
fn planned_parts(plan: &SemanticPlan, parts: &[Part], several: bool) -> Result<Option<Vec<Part>>> {
    let (order, classes) = if several {
        (&plan.colors.series_order, &plan.colors.series_classes)
    } else {
        (&plan.colors.category_order, &plan.colors.category_classes)
    };
    if order.is_empty() || classes.is_empty() {
        return Ok(None);
    }
    let mut out = vec![];
    for &i in order {
        let p = parts.iter().find(|p| p.index == i).ok_or_else(|| {
            err(format!(
                "the colour plan orders a part {i} the pie does not draw"
            ))
        })?;
        let mut p = p.clone();
        let c = classes
            .get(i)
            .ok_or_else(|| err(format!("the colour plan has no classes for part {i}")))?;
        p.colour = c.first().cloned();
        p.tint = c.iter().any(|x| x == "tint");
        out.push(p);
    }
    if out.len() != parts.len() {
        return Err(err(
            "the colour plan's part order leaves out parts the pie draws",
        ));
    }
    Ok(Some(out))
}

pub fn layout_pie(kit: &Kit, plan: &SemanticPlan, data: &PreparedData) -> Result<PlacedChart> {
    let spec = &plan.spec.raw;
    let mut findings: Vec<Finding> = kit::inset_info(spec, plan.chart_type.as_str())
        .into_iter()
        .collect();
    let mut decisions: Vec<Value> = vec![];
    let w_fig = kit.c.width;
    let em = kit.metric("--label-size")?;
    let donut = plan.chart_type == ChartType::Donut;
    let hl: Vec<&Value> = spec["highlight"].as_array().into_iter().flatten().collect();
    let method = |h: &Value| h["method"].as_str().unwrap_or("auto").to_string();
    let methods_hl: Vec<String> = hl.iter().map(|h| method(h)).collect();
    // The rejected versions are refused (or recorded as overridden) by the policy before layout
    // (chartgen-policy marks.rs); a layout that runs has been allowed to draw them.
    let legend = spec.pointer("/chart/labelling").and_then(Value::as_str) == Some("legend");
    let rows = series_rows(spec, data);
    if rows.len() < data.series.len() {
        findings.push(info(
            "CG-3",
            "a reference or target series has no place in a pie: not drawn",
            Some("data.series"),
        ));
    }
    if rows.is_empty() {
        return Err(err("a pie needs a series of parts").at("data.series"));
    }
    let several = rows.len() > 1;
    let xs = &data.categories;
    let x = &spec["data"]["x"];
    let (parts, pies): (Option<Vec<Part>>, Vec<Pie>) = if several {
        // several totals: the totals the categories, their parts the series (pie.totals_of)
        let mut totals: Vec<usize> = (0..xs.len()).collect();
        let (choice_sort, sort_by) = kit::choice_sort(spec, plan.chart_type);
        let choice_sort = choice_sort.as_str();
        let by = rows
            .iter()
            .copied()
            .find(|&i| data.series[i].id == sort_by)
            .unwrap_or(rows[0]);
        let nominal = x["kind"].as_str() == Some("nominal");
        if matches!(choice_sort, "desc" | "asc") && nominal {
            let val = |j: usize| {
                let d = &data.series[by].values[j];
                if known(d) {
                    d.value.unwrap()
                } else {
                    Decimal::ZERO
                }
            };
            totals.sort_by(|&a, &b| {
                if choice_sort == "desc" {
                    val(b).cmp(&val(a))
                } else {
                    val(a).cmp(&val(b))
                }
            });
        }
        let mut pies = vec![];
        for &j in &totals {
            let (content, _) = text_of(&x["names"][&xs[j]], &xs[j])?;
            pies.push(Pie {
                id: xs[j].clone(),
                content,
            });
        }
        let mut parts = vec![];
        let mut refused = false;
        for &i in &rows {
            let s = &data.series[i];
            let (content, plain) = text_of(&spec["data"]["series"][i]["name"], &s.id)?;
            let cells: Vec<(String, Option<&Datum>, String)> = totals
                .iter()
                .map(|&j| {
                    (
                        format!("{plain} in {}", xs[j]),
                        s.values.get(j),
                        format!("data.series[{i}].values[{j}]"),
                    )
                })
                .collect();
            match sizes(spec, &mut findings, &cells) {
                None => refused = true,
                Some(v) => parts.push(Part {
                    name: plain,
                    content,
                    index: i,
                    role: spec["data"]["series"][i]["role"]
                        .as_str()
                        .filter(|r| TAIL.contains(r))
                        .map(str::to_owned),
                    group: None,
                    values: v,
                    colour: None,
                    tint: false,
                    target: false,
                    key: s.id.clone(),
                }),
            }
        }
        if !refused {
            for (k, pie) in pies.iter().enumerate() {
                if parts.iter().map(|p| p.values[k]).sum::<Decimal>().is_zero() {
                    findings.push(finding(
                        "CG-4",
                        "error",
                        &format!(
                            "every part of {} is zero: there is no total to break down",
                            pie.content.readable()
                        ),
                        Some("data.series"),
                    ));
                    refused = true;
                }
            }
        }
        (if refused { None } else { Some(parts) }, pies)
    } else {
        let i = rows[0];
        let s = &data.series[i];
        let (content, _) = text_of(&spec["data"]["series"][i]["name"], &s.id)?;
        let pies = vec![Pie {
            id: s.id.clone(),
            content,
        }];
        let parts = parts_of(spec, data, i, &mut findings)?;
        (parts, pies)
    };
    let Some(parts) = parts else {
        return Ok(kit::refused(findings, decisions));
    };
    {
        let vals: Vec<Decimal> = rows
            .iter()
            .flat_map(|&i| data.series[i].values.iter())
            .filter_map(|d| d.value)
            .collect();
        if vals.iter().any(|v| *v < Decimal::ZERO) && !vals.iter().any(|v| *v > Decimal::ZERO) {
            kit::decide(
                &mut decisions,
                "R7-04",
                "every part negative: the slices show their sizes, a whole of one sign (G0.15)",
                None,
            );
        }
    }
    if several {
        kit::decide(
            &mut decisions,
            "R7-24",
            format!(
                "{} totals, a pie each, in the order {}; their parts the series, in one order and one colour in every pie (G4.22)",
                pies.len(),
                pies.iter()
                    .map(|p| p.content.readable())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None,
        );
    }
    let ordinal = if several {
        spec.pointer("/data/parts/kind").and_then(Value::as_str) == Some("ordinal")
    } else {
        x["kind"].as_str() == Some("ordinal")
    };
    let sort = if several {
        "desc".to_string()
    } else {
        kit::choice_sort(spec, plan.chart_type).0
    };
    // the slices' order and colours are the colour plan's (chartgen-colours: pie.order and
    // pie.colours, with their decisions); a plan compiled without colours takes the layout's port
    let mut parts = match planned_parts(plan, &parts, several)? {
        Some(p) => p,
        None => {
            let mut p = order(parts, &sort, 0, ordinal);
            colours(&mut p, plan, several, ordinal, &mut findings);
            p
        }
    };
    kit::colour_decisions_here(&mut decisions);

    // the highlight: targets, method, paling (R7-38)
    let mut refs: Vec<&Value> = hl
        .iter()
        .flat_map(|h| h["targets"].as_array().into_iter().flatten())
        .collect();
    if refs.is_empty() && hl.is_empty() {
        refs.extend(
            spec.pointer("/insight/targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
    }
    let names: BTreeSet<&str> = refs
        .iter()
        .filter_map(|r| r["category"].as_str().or_else(|| r["x"].as_str()))
        .collect();
    let named_series: BTreeSet<&str> = refs.iter().filter_map(|r| r["series"].as_str()).collect();
    for p in parts.iter_mut() {
        p.target = if several {
            named_series.contains(p.key.as_str()) || named_series.contains(p.name.as_str())
        } else {
            names.contains(p.name.as_str()) || names.contains(p.key.as_str())
        };
    }
    if parts
        .iter()
        .any(|p| p.target && p.colour.as_deref().is_some_and(|c| c.starts_with("seq-")))
    {
        findings.push(info(
            "CG-3",
            "the parts are ordered, in steps of one hue: a step has no strong shade, and a darker one would read as another part's; the highlight is not drawn (a comment can point at the part)",
            Some(if hl.is_empty() { "insight.targets" } else { "highlight" }),
        ));
        for p in parts.iter_mut() {
            p.target = false;
        }
    }
    let methods: BTreeSet<String> = if methods_hl.is_empty() {
        ["auto".to_string()].into()
    } else {
        methods_hl.iter().cloned().collect()
    };
    let border = methods.contains("border");
    let exploded = methods.contains("explode");
    let dim = hl.iter().any(|h| h["dim_others"].as_bool() == Some(true));
    let message = spec.pointer("/facts/message").and_then(Value::as_str);
    for (i, h) in hl.iter().enumerate() {
        let m = method(h);
        if DEFER.contains(&m.as_str()) {
            findings.push(info(
                "CG-3",
                &format!(
                    "highlight[{i}] {m}: on a pie, drawn as the strong shade of the slice's colour"
                ),
                Some(&format!("highlight[{i}].method")),
            ));
        }
        if m == "border" {
            guided(
                spec,
                &mut findings,
                "R15-5",
                &format!("highlight[{i}].method"),
                "border",
                "strong",
                "a slice in the strong shade of its hue (D-9); a border works too (G7.30, G8.9)",
            );
        }
        if h["dim_others"].as_bool() == Some(true)
            && !matches!(message, Some("subtotal_fraction" | "subtotal_amount"))
        {
            guided(
                spec,
                &mut findings,
                "R15-5",
                &format!("highlight[{i}].dim_others"),
                "true",
                "false",
                "the others keep their colour unless the message is a subtotal (D-9)",
            );
        }
    }

    if hl.is_empty()
        && !names.is_empty()
        && spec
            .pointer("/insight/targets")
            .and_then(Value::as_array)
            .is_some_and(|t| !t.is_empty())
    {
        kit::decide(
            &mut decisions,
            "R15-5",
            "highlighting what the insight names",
            None,
        );
    }
    // label contents (R7-34, D-51): the value or the percentage under the name, as one set
    let mut mode = spec
        .pointer("/labels/parts")
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string();
    if mode == "auto" {
        mode = if several { "name" } else { "value" }.into();
    }
    let mut value_text: BTreeMap<usize, String> = BTreeMap::new();
    if matches!(mode.as_str(), "value" | "percent") {
        let vals: Vec<Decimal> = parts.iter().map(|p| p.values[0]).collect();
        let texts = if mode == "percent" {
            let total: Decimal = vals.iter().copied().sum();
            let pct: Vec<Option<Decimal>> = vals
                .iter()
                .map(|v| Some(*v * Decimal::from(100) / total))
                .collect();
            fmt::format_values_with_measure(&plan.spec, &json!({"kind": "percent"}), &pct)?
        } else {
            fmt::format_values(
                &plan.spec,
                &vals.iter().map(|v| Some(*v)).collect::<Vec<_>>(),
            )?
        };
        for (k, t) in texts.into_iter().enumerate() {
            value_text.insert(k, t.unwrap_or_default());
        }
    }

    kit::decide(
        &mut decisions,
        "R7-34",
        if matches!(mode.as_str(), "value" | "percent") {
            format!(
                "each slice named outside it, its {} under the name, formatted as one set (G7.27, D-51 (ii))",
                if mode == "percent" {
                    "percentage of the total"
                } else {
                    "value"
                }
            )
        } else if several {
            "each part named outside its slice, once, round the first pie (G7.27, G7.49)"
                .to_string()
        } else {
            "each part named outside its slice (G7.27)".to_string()
        },
        None,
    );
    let block = kit::title_block(kit, spec, w_fig, &mut findings, &mut decisions)?;
    let top = match block.last_baseline {
        Some(b) => b + kit.metric("--title-gap")? - 0.683 * em,
        None => 0.,
    };
    let pitch = kit.metric("--line-pitch")?;
    let wrap = kit.metric("--pie-label-width")?;
    let mut blocks: Vec<Block> = vec![];
    if !legend {
        for (k, p) in parts.iter().enumerate() {
            kit.need("slice-label", &[], &p.content)?;
            for (a, b) in kit::splits(&p.name) {
                kit.need("slice-label", &[], &kit::plain(&a))?;
                kit.need("slice-label", &[], &kit::plain(&b))?;
            }
            if let Some(v) = value_text.get(&k) {
                kit.need("slice-label", &[], &TextContent::plain(v))?;
            }
        }
        for (k, p) in parts.iter().enumerate() {
            let mut lines = name_lines(kit, p, wrap)?;
            if let Some(v) = value_text.get(&k) {
                lines.push(TextContent::plain(v));
            }
            let exts: Vec<kit::Ext> = lines
                .iter()
                .map(|c| kit.ext("slice-label", &[], c))
                .collect::<Result<_>>()?;
            let h =
                exts[0].ascent + pitch * (lines.len() - 1) as f64 + exts.last().unwrap().descent;
            let w = exts.iter().map(|e| e.width).fold(0., f64::max);
            blocks.push(Block {
                part: k,
                asc: exts[0].ascent,
                lines: lines.into_iter().zip(exts).collect(),
                w,
                h,
                theta: 0.,
                span: 0.,
                zero: false,
                side: 1,
                ideal: (0., 0.),
                x: 0.,
                y: 0.,
                leader: false,
            });
        }
    }
    let angles = |j: usize| -> Vec<(f64, f64)> {
        let total: Decimal = parts.iter().map(|p| p.values[j]).sum();
        let mut out = vec![];
        let mut a = 0.;
        for p in &parts {
            let span = if total.is_zero() {
                0.
            } else {
                dec_f(p.values[j] / total) * 360.
            };
            out.push((a, a + span));
            a += span;
        }
        out
    };
    let mut r = kit.metric("--pie-radius")? * w_fig;
    let rmin = kit.metric("--pie-min-radius")?;
    let n_pies = pies.len();
    let gap_pies = 2. * em;
    let row = |bbox: [f64; 4], r: f64, even: bool| -> (f64, Vec<f64>) {
        if n_pies == 1 {
            return (bbox[2] - bbox[0], vec![]);
        }
        let first = (2. * r + gap_pies).max(bbox[2] + gap_pies + r);
        let steps: Vec<f64> = if even {
            vec![first; n_pies - 1]
        } else {
            std::iter::once(first)
                .chain(std::iter::repeat_n(2. * r + gap_pies, n_pies - 2))
                .collect()
        };
        (steps.iter().sum::<f64>() + r - bbox[0], steps)
    };
    let mut even = true;
    let mut bbox = [0.; 4];
    let mut width = 0.;
    let mut steps = vec![];
    for _ in 0..24 {
        let ang = angles(0);
        let push: BTreeMap<usize, f64> = parts
            .iter()
            .enumerate()
            .filter(|(_, p)| exploded && p.target)
            .map(|(k, _)| (k, 0.1 * r))
            .collect();
        bbox = place(kit, &mut blocks, &ang, r, &push)?;
        (width, steps) = row(bbox, r, even);
        if width <= w_fig + 1e-6 {
            break;
        }
        if r <= rmin {
            if !even {
                break;
            }
            even = false;
            r = kit.metric("--pie-radius")? * w_fig;
            continue;
        }
        r = rmin.max(r - (width - w_fig) / (2. * n_pies as f64) - 0.5);
    }
    if width > w_fig + 1e-6 {
        findings.push(finding(
            "CG-1",
            "error",
            &format!("the labels do not fit round a pie of the least radius ({rmin:.0} pt) within {w_fig:.0} pt: shorten the names, or widen the chart"),
            Some("layout.width"),
        ));
    }
    let gap = kit.metric("--pie-gap")? * 2. * r;
    let cx0 = if n_pies == 1 {
        (w_fig / 2.).max(-bbox[0]).min(w_fig - bbox[2])
    } else {
        ((w_fig - width) / 2.).max(0.) - bbox[0]
    };
    if n_pies > 1 {
        kit::decide(
            &mut decisions,
            "R7-24",
            format!(
                "{n_pies} pies in a row, of one radius ({r:.1} pt), {}",
                if even {
                    "evenly spaced: the parts named round the first, the second standing clear of its labels"
                } else {
                    "the parts named round the first, which stands apart with its labels, the others closer together (evenly spaced they do not fit the width)"
                }
            ),
            Some("open"),
        );
    }
    let mut head_h = 0.;
    if several {
        let mut a: f64 = 0.;
        for s in &pies {
            a = a.max(kit.ext("column-header", &[], &s.content)?.ascent);
        }
        head_h = a + em;
    }
    let target_any = parts.iter().any(|p| p.target);
    let emph_of = |p: &Part| -> Vec<&'static str> {
        if p.target {
            vec!["highlight"]
        } else if dim && target_any {
            vec!["context"]
        } else {
            vec![]
        }
    };
    let mut legend_h = 0.;
    let mut legend_prims: Vec<Primitive> = vec![];
    let mut legend_texts: Vec<Placed> = vec![];
    let mut legend_bottom: Option<f64> = None;
    if legend {
        let gap_l = kit.metric("--legend-gap")?;
        let sep = kit.metric("--legend-entry-gap")?;
        let lpitch = kit.metric("--line-pitch")?;
        let key = kit.metric("--legend-key")?;
        let cap_l = kit.plain_ext("legend-text", &[], "H")?.ascent;
        let entries: Vec<&Part> = parts.iter().filter(|p| p.colour.is_some()).collect();
        let widths: Vec<f64> = entries
            .iter()
            .map(|p| Ok(key + gap_l + kit.width("legend-text", &[], &p.content)?))
            .collect::<Result<_>>()?;
        let n = entries.len();
        let mut c = n;
        let mut cols = widths.clone();
        while c > 0 {
            cols = if c == n {
                widths.clone()
            } else {
                (0..c)
                    .map(|j| (j..n).step_by(c).map(|i| widths[i]).fold(0., f64::max))
                    .collect()
            };
            if cols.iter().sum::<f64>() + sep * (c - 1) as f64 <= w_fig + 1e-6 {
                break;
            }
            c -= 1;
        }
        let c = c.max(1);
        let dy = top + cap_l / 2.;
        let mut lbox = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for (i, p) in entries.iter().enumerate() {
            let (j, rr) = (i % c, i / c);
            let x0 = cols[..j].iter().sum::<f64>() + sep * j as f64;
            let yc = rr as f64 * lpitch + dy;
            let mut classes = vec!["legend-swatch", p.colour.as_deref().unwrap()];
            let emph = emph_of(p);
            if p.tint && emph.is_empty() {
                classes.push("tint");
            }
            classes.extend(emph.iter());
            legend_prims.push(kit::rect(
                x0,
                yc - key / 2.,
                key,
                key,
                kit::fill(&kit::class_paint(
                    kit,
                    p.colour.as_deref().unwrap(),
                    &classes,
                )),
                meta(&classes, &[("series", p.key.clone())]),
            ));
            lbox = [
                lbox[0].min(x0),
                lbox[1].min(yc - key / 2.),
                lbox[2].max(x0 + key),
                lbox[3].max(yc + key / 2.),
            ];
            let t = kit.text(
                "legend-text",
                &[],
                &p.content,
                x0 + key + gap_l,
                yc + cap_l / 2.,
                "start",
            )?;
            let b = t.bbox();
            lbox = [
                lbox[0].min(b[0]),
                lbox[1].min(b[1]),
                lbox[2].max(b[2]),
                lbox[3].max(b[3]),
            ];
            legend_texts.push(t);
        }
        legend_h = lbox[3] - top + em;
        legend_bottom = Some(lbox[3]);
    }
    let cy = top + head_h + legend_h - bbox[1];
    for b in blocks.iter_mut() {
        b.x += cx0;
        b.y += cy;
    }
    let border_w = kit.metric("--border-width").unwrap_or(0.2 * em);
    let hole = kit.metric("--donut-hole").unwrap_or(0.74);
    let mut slices: Vec<Primitive> = vec![];
    let mut overlay: Vec<Primitive> = vec![];
    let mut headers: Vec<Placed> = vec![];
    let mut centres = vec![];
    let mut marks = BTreeMap::new();
    let methods_strong = methods.contains("auto") || methods.contains("strong");
    for (j, s) in pies.iter().enumerate() {
        let cxj = cx0 + steps[..j].iter().sum::<f64>();
        centres.push((cxj, cy));
        let ang = angles(j);
        let mut drawn: Vec<(bool, Primitive)> = vec![];
        for (p, &(a0, a1)) in parts.iter().zip(&ang) {
            let Some(colour) = &p.colour else { continue };
            if a1 - a0 <= 0. {
                continue;
            }
            let mut emph = emph_of(p);
            if p.target && border && !methods_strong {
                emph.clear();
            }
            let mut classes = vec!["slice", colour.as_str()];
            if p.tint && emph.is_empty() {
                classes.push("tint");
            }
            classes.extend(emph.iter());
            let (ox, oy) = if exploded && p.target {
                ray(0., 0., (a0 + a1) / 2., 0.1 * r)
            } else {
                (0., 0.)
            };
            let ids: Vec<(&str, String)> = if several {
                vec![("series", p.key.clone()), ("x", s.id.clone())]
            } else {
                vec![("series", s.id.clone()), ("x", p.key.clone())]
            };
            let mut with_value = ids.clone();
            with_value.push(("value", kit::dec_text(p.values[j])));
            let (sk, xk) = if several {
                (p.key.clone(), s.id.clone())
            } else {
                (s.id.clone(), p.key.clone())
            };
            marks.insert(
                (sk, xk),
                crate::annotate::Mark::Wedge {
                    x: cxj + ox,
                    y: cy + oy,
                    r,
                    r0: if donut { hole * r } else { 0. },
                    a0,
                    a1,
                    inset: gap / 2.,
                },
            );
            if let Some(points) = wedge(
                cxj + ox,
                cy + oy,
                r,
                a0,
                a1,
                if donut { hole * r } else { 0. },
                gap / 2.,
            )? {
                drawn.push((
                    p.target,
                    Primitive::Shape {
                        commands: points,
                        paint: kit::fill(&kit::class_paint(kit, colour, &classes)),
                        meta: meta(&classes, &with_value),
                    },
                ));
            }
            if p.target && border {
                let bclasses = ["border", colour.as_str(), "highlight"];
                if let Some(points) = wedge(
                    cxj + ox,
                    cy + oy,
                    r - border_w / 2.,
                    a0,
                    a1,
                    if donut { hole * r + border_w / 2. } else { 0. },
                    gap / 2. + border_w / 2.,
                )? {
                    overlay.push(Primitive::Shape {
                        commands: points,
                        paint: kit::stroke(
                            &kit::class_paint(kit, colour, &["highlight"]),
                            border_w,
                        ),
                        meta: meta(&bclasses, &ids),
                    });
                }
            }
        }
        // the targets drawn last
        let mut ordered: Vec<Primitive> =
            drawn.iter().filter(|d| !d.0).map(|d| d.1.clone()).collect();
        ordered.extend(drawn.iter().filter(|d| d.0).map(|d| d.1.clone()));
        slices.push(kit::group(
            &["series"],
            None,
            &[(if several { "total" } else { "series" }, s.id.clone())],
            ordered,
        ));
        if several {
            let e = kit.ext("column-header", &[], &s.content)?;
            headers.push(kit.text(
                "column-header",
                &[],
                &s.content,
                cxj,
                top + e.ascent,
                "middle",
            )?);
        }
    }

    // the labels and their leaders (the first pie's)
    let lw = kit.metric("--grid-width")?;
    let mut exp_off: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
    if exploded {
        for (k, (p, &(a0, a1))) in parts.iter().zip(&angles(0)).enumerate() {
            if p.target {
                exp_off.insert(k, ray(0., 0., (a0 + a1) / 2., 0.1 * r));
            }
        }
    }
    let mut labels: Vec<Placed> = vec![];
    let mut leaders_prims: Vec<Primitive> = vec![];
    let mut leaders: Vec<String> = vec![];
    let ink = kit::token_paint(kit, "--leader");
    for b in &blocks {
        for (k, (content, _)) in b.lines.iter().enumerate() {
            let base = b.y - b.h / 2. + b.asc + k as f64 * pitch;
            labels.push(kit.text("slice-label", &[], content, b.x, base, "middle")?);
        }
        if b.leader {
            let (ex, ey) = exp_off.get(&b.part).copied().unwrap_or((0., 0.));
            let e = ray(cx0 + ex, cy + ey, b.theta, r + 0.15 * em);
            let bx = b.bbox(0.);
            let off = 0.25 * em;
            let cands = [(bx[0] - off, b.y), (bx[2] + off, b.y)];
            let s = if (cands[0].0 - e.0).hypot(cands[0].1 - e.1)
                <= (cands[1].0 - e.0).hypot(cands[1].1 - e.1)
            {
                cands[0]
            } else {
                cands[1]
            };
            let dist = (s.0 - e.0).hypot(s.1 - e.1);
            let q = ray(
                cx0 + ex,
                cy + ey,
                b.theta,
                r + 0.15 * em + (0.8 * em).max(0.45 * dist),
            );
            let p = &parts[b.part];
            let data: Vec<(&str, String)> = if several {
                vec![("series", p.key.clone())]
            } else {
                vec![("x", p.key.clone())]
            };
            leaders_prims.push(kit::curve(s, e, q, lw, &ink, "leader", &data));
            leaders.push(p.name.clone());
        }
    }
    if !leaders.is_empty() {
        kit::decide(
            &mut decisions,
            "R7-37",
            format!(
                "leaders to {} (a zero, a slice under {}°, or a label moved from its place; G7.29, D-34)",
                leaders.join(", "),
                kit.metric("--pie-leader-angle")?
            ),
            None,
        );
    }
    kit::decide(
        &mut decisions,
        "R7-33",
        format!(
            "no outlines: true gaps {gap:.1} pt wide between the slices (G7.26s: {}% of the diameter)",
            kit.metric("--pie-gap")? * 100.
        ),
        None,
    );
    if parts.iter().any(|p| p.target) {
        let how = if border && !methods_strong {
            "a border in the strong shade inside"
        } else if exploded {
            "exploded (through the override)"
        } else {
            "the strong shade of their colour"
        };
        kit::decide(
            &mut decisions,
            "R3-7",
            format!(
                "highlighted by {how}: {}{}",
                parts
                    .iter()
                    .filter(|p| p.target)
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                if dim {
                    "; the others paled"
                } else {
                    "; the others keep their colour"
                }
            ),
            None,
        );
    }
    // labels clear of the pie (CG-1)
    for b in &blocks {
        let x = b.bbox(0.);
        let nx = cx0.max(x[0]).min(x[2]);
        let ny = cy.max(x[1]).min(x[3]);
        if (nx - cx0).hypot(ny - cy) < r - 0.01 {
            findings.push(finding(
                "CG-1",
                "error",
                &format!("the label of {} lies over the pie", parts[b.part].name),
                None,
            ));
        }
    }
    let bottom = labels
        .iter()
        .chain(block.texts.iter())
        .chain(headers.iter())
        .map(|t| t.bbox()[3])
        .chain(legend_bottom)
        .fold(cy + r, f64::max);
    let height = bottom + 1.;
    let mut header_g: Vec<Primitive> = block.texts.iter().map(Placed::primitive).collect();
    if legend {
        let entries: Vec<Primitive> = legend_prims
            .into_iter()
            .zip(&legend_texts)
            .zip(parts.iter().filter(|p| p.colour.is_some()))
            .map(|((key, text), p)| {
                kit::group(
                    &["legend-entry"],
                    None,
                    &[("series", p.key.clone())],
                    vec![key, text.primitive()],
                )
            })
            .collect();
        header_g.push(kit::group(&["legend"], None, &[], entries));
    }
    let mut panel_g = vec![kit::group(&["marks"], None, &[], slices)];
    if !overlay.is_empty() {
        panel_g.push(kit::group(&["overlay"], None, &[], overlay));
    }
    let mut notes_g: Vec<Primitive> = labels.iter().map(Placed::primitive).collect();
    notes_g.extend(leaders_prims);
    panel_g.push(kit::group(&["annotations"], None, &[], notes_g));
    if !headers.is_empty() {
        panel_g.push(kit::group(
            &["headers"],
            None,
            &[],
            headers.iter().map(Placed::primitive).collect(),
        ));
    }
    let tree = vec![
        kit::group(&["header"], None, &[], header_g),
        kit::group(&["panel"], Some("main"), &[], panel_g),
    ];
    // the message layer sees everything drawn; what it adds is the comments group
    let mut primitives: Vec<Primitive> = kit::flatten(&tree).into_iter().cloned().collect();
    let drawn_n = primitives.len();
    let mut summary = BTreeMap::new();
    let mut height = height;
    let header_bottom = block.texts.iter().map(|t| t.bbox()[3]).fold(0., f64::max);
    let last_cx = centres.last().map_or(cx0, |c| c.0);
    crate::annotate::annotate(
        kit,
        spec,
        &mut primitives,
        w_fig,
        &mut height,
        [cx0 - r, cy - r, last_cx + r, cy + r],
        header_bottom,
        &block.texts,
        &marks,
        &mut findings,
        &mut summary,
        &mut decisions,
    )?;
    let comments = primitives.split_off(drawn_n);
    let mut root = tree;
    if !comments.is_empty() {
        root.push(kit::group(&["comments"], None, &[], comments));
    }
    root.push(kit::group(&["footer"], None, &[], vec![]));
    let mut classes = vec!["chart", "pie"];
    if donut {
        classes.push("donut");
    }
    classes.push("categorical");
    let primitives = vec![kit::group(&classes, Some("chart"), &[], root)];
    summary.insert("kind".into(), json!(if donut { "donut" } else { "pie" }));
    summary.insert("pies".into(), json!(n_pies));
    summary.insert("radius".into(), r2(r));
    summary.insert(
        "centres".into(),
        json!(
            centres
                .iter()
                .map(|(a, b)| json!([r2(*a), r2(*b)]))
                .collect::<Vec<_>>()
        ),
    );
    summary.insert(
        "order".into(),
        json!(parts.iter().map(|p| &p.name).collect::<Vec<_>>()),
    );
    summary.insert(
        "labels".into(),
        json!(if legend { "legend".to_string() } else { mode }),
    );
    summary.insert("leaders".into(), json!(leaders));
    summary.insert("width".into(), r2(w_fig));
    summary.insert("height".into(), r2(height));
    Ok(PlacedChart {
        decisions,
        width: w_fig,
        height,
        primitives,
        findings,
        summary,
    })
}
