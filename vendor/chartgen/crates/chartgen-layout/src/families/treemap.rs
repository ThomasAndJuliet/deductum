//! Treemaps: one total broken into many parts, each a rectangle whose area is its value; a
//! port of the original `layout/treemap.py` (SPEC §6.11a; R7-15, R7-16; G7.34).
//!
//! Squarified, largest first from the top left; with `x.groups` the groups first, then each
//! group's parts inside its rectangle. True gaps, no outlines. One colour for a flat treemap;
//! by group for a hierarchy (each group a palette slot in the order of the groups, "All
//! others" its reserved grey). Names inside the tiles at the bottom left, on two lines, or
//! shortened with "…", else counted in a note under the chart; a group's name in bold.
use super::kit::{self, Kit, Placed, finding, guided, info, meta, r2};
use super::pie::{self, Part};
use chartgen_format as fmt;
use chartgen_model::*;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const TAIL: [&str; 2] = ["all_others", "missing"];

type R = (f64, f64, f64, f64);

/// Rectangles in the order of `values` (largest first, all positive), each with an area in
/// proportion to its value, as near square as the greedy rows allow (Bruls et al., 2000).
pub fn squarify(values: &[f64], mut x: f64, mut y: f64, mut w: f64, mut h: f64) -> Vec<R> {
    let total: f64 = values.iter().sum();
    if total <= 0. || w <= 0. || h <= 0. {
        return values.iter().map(|_| (x, y, 0., 0.)).collect();
    }
    let areas: Vec<f64> = values.iter().map(|v| v / total * w * h).collect();
    let mut out = vec![];
    let mut i = 0;
    while i < areas.len() {
        let side = w.min(h);
        let worst = |r: &[f64]| -> f64 {
            let s: f64 = r.iter().sum();
            r.iter()
                .map(|&a| (side * side * a / (s * s)).max((s * s) / (side * side * a)))
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let mut row = vec![areas[i]];
        let mut j = i + 1;
        while j < areas.len() {
            let mut next = row.clone();
            next.push(areas[j]);
            if worst(&next) <= worst(&row) {
                row = next;
                j += 1;
            } else {
                break;
            }
        }
        let s: f64 = row.iter().sum();
        if w >= h {
            let cw = if h != 0. { s / h } else { 0. };
            let mut yy = y;
            for a in &row {
                let th = if cw != 0. { a / cw } else { 0. };
                out.push((x, yy, cw, th));
                yy += th;
            }
            x += cw;
            w -= cw;
        } else {
            let rh = if w != 0. { s / w } else { 0. };
            let mut xx = x;
            for a in &row {
                let tw = if rh != 0. { a / rh } else { 0. };
                out.push((xx, y, tw, rh));
                xx += tw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    out
}

fn inset(r: R, d: f64) -> R {
    (
        r.0 + d,
        r.1 + d,
        (r.2 - 2. * d).max(0.),
        (r.3 - 2. * d).max(0.),
    )
}

fn note(spec: &Value, key: &str, default: &str) -> String {
    match spec.pointer("/labels/notes").and_then(Value::as_object) {
        Some(n) => n
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_string(),
        None => default.to_string(),
    }
}

fn cut(name: &str, k: usize) -> String {
    let s: String = name.chars().take(k).collect();
    format!("{}…", s.trim_end())
}

fn tail_index(p: &Part) -> usize {
    p.role
        .as_deref()
        .and_then(|r| TAIL.iter().position(|t| *t == r))
        .map_or(0, |k| k + 1)
}

struct Tile {
    part: usize,
    rect: R,
    group: Option<String>,
    first: bool,
}

pub fn layout_treemap(kit: &Kit, plan: &SemanticPlan, data: &PreparedData) -> Result<PlacedChart> {
    let spec = &plan.spec.raw;
    let mut findings: Vec<Finding> = kit::inset_info(spec, "treemap").into_iter().collect();
    let mut decisions: Vec<Value> = vec![];
    let w_fig = kit.c.width;
    let em = kit.metric("--label-size")?;
    let x = &spec["data"]["x"];
    let xs = &data.categories;
    let hl: Vec<&Value> = spec["highlight"].as_array().into_iter().flatten().collect();
    let method = |h: &Value| h["method"].as_str().unwrap_or("auto").to_string();
    // The rejected versions are refused (or recorded as overridden) by the policy before layout
    // (chartgen-policy marks.rs); a layout that runs has been allowed to draw them.
    let grouped = x["groups"].as_object().is_some_and(|g| !g.is_empty());
    if spec.pointer("/chart/labelling").and_then(Value::as_str) == Some("legend") && !grouped {
        findings.push(info(
            "R7-16",
            "a flat treemap's parts are named in their tiles: no legend",
            Some("chart.labelling"),
        ));
    }
    let rows = pie::series_rows(spec, data);
    if rows.len() != 1 {
        return Err(kit::err(
            "a treemap of several totals is not drawn: one treemap shows one total",
        )
        .at("data.series"));
    }
    let pie_id = data.series[rows[0]].id.clone();
    let parts = pie::parts_of(spec, data, rows[0], &mut findings)?;
    let Some(parts) = parts else {
        return Ok(kit::refused(findings, decisions));
    };

    // the highlight: targets, method, paling
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
    let methods: BTreeSet<String> = if hl.is_empty() {
        ["auto".to_string()].into()
    } else {
        hl.iter().map(|h| method(h)).collect()
    };
    let border =
        methods.contains("border") && !methods.contains("auto") && !methods.contains("strong");
    let dim = hl.iter().any(|h| h["dim_others"].as_bool() == Some(true));
    for (i, h) in hl.iter().enumerate() {
        let m = method(h);
        if m == "border" {
            guided(
                spec,
                &mut findings,
                "R15-5",
                &format!("highlight[{i}].method"),
                "border",
                "strong",
                "a tile in the strong shade of its hue (D-9); a border works too (G8.9)",
            );
        }
        if !matches!(m.as_str(), "auto" | "strong" | "border" | "thickness") {
            findings.push(info("CG-3", &format!("highlight[{i}] {m}: on a treemap, drawn as the strong shade of the tile's colour"), Some(&format!("highlight[{i}].method"))));
        }
    }
    let targets: BTreeSet<usize> = (0..parts.len())
        .filter(|&k| {
            names.contains(parts[k].name.as_str()) || names.contains(xs[parts[k].index].as_str())
        })
        .collect();

    // groups: the groups largest first, then each group's parts largest first
    let size = |k: usize| parts[k].values[0];
    let zeros: Vec<usize> = (0..parts.len()).filter(|&k| size(k).is_zero()).collect();
    let live: Vec<usize> = (0..parts.len())
        .filter(|&k| size(k) > Decimal::ZERO)
        .collect();
    let mut runs: Vec<(String, Vec<usize>)> = vec![];
    for &k in &live {
        let g = if grouped {
            parts[k]
                .group
                .clone()
                .unwrap_or_else(|| parts[k].name.clone())
        } else {
            String::new()
        };
        match runs.iter_mut().find(|(n, _)| *n == g) {
            Some((_, m)) => m.push(k),
            None => runs.push((g, vec![k])),
        }
    }
    let sum = |m: &[usize]| m.iter().map(|&k| size(k)).sum::<Decimal>();
    runs.sort_by(|a, b| {
        sum(&b.1)
            .cmp(&sum(&a.1))
            .then(parts[a.1[0]].index.cmp(&parts[b.1[0]].index))
    });
    for (_, members) in runs.iter_mut() {
        members.sort_by(|&a, &b| {
            tail_index(&parts[a])
                .cmp(&tail_index(&parts[b]))
                .then(size(b).cmp(&size(a)))
                .then(parts[a].index.cmp(&parts[b].index))
        });
    }
    let all_tail = |m: &[usize]| m.iter().all(|&k| tail_index(&parts[k]) > 0);
    runs.sort_by(|a, b| {
        all_tail(&a.1)
            .cmp(&all_tail(&b.1))
            .then(sum(&b.1).cmp(&sum(&a.1)))
    });
    let mut colour: BTreeMap<usize, String> = BTreeMap::new();
    let planned =
        !plan.colors.category_order.is_empty() && !plan.colors.category_classes.is_empty();
    if planned {
        // the tiles' order and colours are the colour plan's (chartgen-colours: treemap.py's
        // groups and colours, with their decisions)
        let mut ordered: Vec<(String, Vec<usize>)> = vec![];
        for &i in &plan.colors.category_order {
            let k = (0..parts.len())
                .find(|&k| parts[k].index == i)
                .ok_or_else(|| {
                    kit::err(format!(
                        "the colour plan orders a part {i} the treemap does not draw"
                    ))
                })?;
            if size(k).is_zero() {
                continue;
            }
            let g = if grouped {
                parts[k]
                    .group
                    .clone()
                    .unwrap_or_else(|| parts[k].name.clone())
            } else {
                String::new()
            };
            match ordered.last_mut() {
                Some((n, m)) if *n == g => m.push(k),
                _ => ordered.push((g, vec![k])),
            }
            if let Some(c) = plan.colors.category_classes.get(i).and_then(|c| c.first()) {
                colour.insert(k, c.clone());
            }
        }
        if ordered.iter().map(|(_, m)| m.len()).sum::<usize>() != live.len() {
            return Err(kit::err(
                "the colour plan's tile order leaves out tiles the treemap draws",
            ));
        }
        runs = ordered;
    } else {
        let mut k = 0;
        for (_, members) in &runs {
            for &p in members {
                if let Some(r) = parts[p].role.as_deref().filter(|r| TAIL.contains(r)) {
                    colour.insert(
                        p,
                        if r == "all_others" {
                            "all-others"
                        } else {
                            "missing"
                        }
                        .into(),
                    );
                }
            }
            if !grouped {
                for &p in members {
                    colour.entry(p).or_insert_with(|| "single".into());
                }
                continue;
            }
            if all_tail(members) {
                continue;
            }
            k += 1;
            for &p in members {
                colour
                    .entry(p)
                    .or_insert_with(|| format!("series-{}", k.min(16)));
            }
        }
    }
    let block = kit::title_block(kit, spec, w_fig, &mut findings, &mut decisions)?;
    let top = match block.last_baseline {
        Some(b) => b + kit.metric("--title-gap")? - 0.333 * em,
        None => 0.,
    };
    let aspect = spec
        .pointer("/layout/plot_aspect")
        .and_then(Value::as_f64)
        .map_or_else(|| kit.metric("--plot-aspect"), Ok)?;
    let h_plot = w_fig * aspect;
    let gap = kit.metric("--tile-gap")?;
    let ggap = kit.metric("--tile-group-gap")?;
    let f = |v: Decimal| v.to_f64().unwrap_or(0.);
    let mut tiles: Vec<Tile> = vec![];
    let mut group_rects: Vec<(String, R, Vec<usize>)> = vec![];
    if grouped {
        let grects = squarify(
            &runs.iter().map(|(_, m)| f(sum(m))).collect::<Vec<_>>(),
            0.,
            top,
            w_fig,
            h_plot,
        );
        for ((g, members), gr) in runs.iter().zip(grects) {
            let inner = inset(gr, ggap / 2.);
            group_rects.push((g.clone(), inner, members.clone()));
            let rs = squarify(
                &members.iter().map(|&p| f(size(p))).collect::<Vec<_>>(),
                inner.0,
                inner.1,
                inner.2,
                inner.3,
            );
            for (j, (&p, r)) in members.iter().zip(rs).enumerate() {
                tiles.push(Tile {
                    part: p,
                    rect: inset(r, gap / 2.),
                    group: Some(g.clone()),
                    first: j == 0,
                });
            }
        }
    } else if let Some((_, members)) = runs.first() {
        let rs = squarify(
            &members.iter().map(|&p| f(size(p))).collect::<Vec<_>>(),
            0.,
            top,
            w_fig,
            h_plot,
        );
        for (&p, r) in members.iter().zip(rs) {
            tiles.push(Tile {
                part: p,
                rect: inset(r, gap / 2.),
                group: None,
                first: false,
            });
        }
    }

    kit::decide(
        &mut decisions,
        "R7-15",
        format!(
            "{}, squarified, the largest at the top left (G7.34); gaps of {gap} pt{}",
            if grouped {
                "groups laid out first, largest first, then each group's parts"
            } else {
                "the parts largest first"
            },
            if grouped {
                format!(", {ggap} pt between groups")
            } else {
                String::new()
            }
        ),
        None,
    );
    kit::colour_decisions_here(&mut decisions);
    // labels: whole, on two lines, shortened with an ellipsis, or left out
    let mode = spec
        .pointer("/labels/parts")
        .and_then(Value::as_str)
        .filter(|m| matches!(*m, "value" | "percent"));
    let pad = kit.metric("--tile-pad")?;
    let min_chars = kit.metric("--tile-min-chars")? as usize;
    let pitch = kit.metric("--line-pitch")?;
    let strong = ["text-strong"];
    for t in &tiles {
        let name = &parts[t.part].name;
        kit.need("tile-label", &[], &kit::plain(name))?;
        for (a, b) in kit::splits(name) {
            kit.need("tile-label", &[], &kit::plain(&a))?;
            kit.need("tile-label", &[], &kit::plain(&b))?;
        }
        for kk in min_chars..name.chars().count() {
            kit.need("tile-label", &[], &kit::plain(&cut(name, kk)))?;
        }
    }
    for (g, _, _) in &group_rects {
        kit.need("tile-label", &strong, &kit::plain(g))?;
        for kk in min_chars..g.chars().count() {
            kit.need("tile-label", &strong, &kit::plain(&cut(g, kk)))?;
        }
    }
    let mut texts: Vec<Placed> = vec![];
    let mut shortened: Vec<String> = vec![];
    let mut group_boxes: BTreeMap<String, [f64; 4]> = BTreeMap::new();
    let first_tile: BTreeMap<String, R> = tiles
        .iter()
        .filter(|t| t.first)
        .map(|t| (t.group.clone().unwrap(), t.rect))
        .collect();
    let mut unnamed_groups = vec![];
    for (g, _, _) in &group_rects {
        let (gx, gy, gw, gh) = first_tile[g];
        let n = g.chars().count();
        let mut cands = vec![g.clone()];
        cands.extend((min_chars..n).rev().map(|kk| cut(g, kk)));
        let mut placed = false;
        for cand in cands {
            let c = kit::plain(&cand);
            let e = kit.ext("tile-label", &strong, &c)?;
            if e.width <= gw - 2. * pad && e.ascent + e.descent <= gh - 2. * pad {
                let t = kit.text(
                    "tile-label",
                    &strong,
                    &c,
                    gx + pad,
                    gy + pad + e.ascent,
                    "start",
                )?;
                let b = t.bbox();
                let p = 0.15 * em;
                group_boxes.insert(g.clone(), [b[0] - p, b[1] - p, b[2] + p, b[3] + p]);
                texts.push(t);
                if cand != *g {
                    shortened.push(g.clone());
                }
                placed = true;
                break;
            }
        }
        if !placed {
            unnamed_groups.push(g.clone());
        }
    }
    let name_tiles = |value_text: &BTreeMap<usize, String>| -> Result<(Vec<Placed>, Vec<String>, Vec<String>, Vec<String>)> {
        let (mut out, mut cut_, mut unnamed_, mut bare_) = (vec![], vec![], vec![], vec![]);
        for t in &tiles {
            let p = &parts[t.part];
            let (tx, ty, tw, th) = t.rect;
            let (room_w, room_h) = (tw - 2. * pad, th - 2. * pad);
            let clash = if t.first { t.group.as_ref().and_then(|g| group_boxes.get(g)) } else { None };
            let base_y = ty + th - pad;
            let fits = |lines: &[String]| -> Result<Option<Vec<Placed>>> {
                let exts: Vec<kit::Ext> = lines
                    .iter()
                    .map(|l| kit.ext("tile-label", &[], &kit::plain(l)))
                    .collect::<Result<_>>()?;
                let hh = exts[0].ascent + pitch * (lines.len() - 1) as f64 + exts.last().unwrap().descent;
                if exts.iter().map(|e| e.width).fold(0., f64::max) > room_w + 1e-6 || hh > room_h + 1e-6 {
                    return Ok(None);
                }
                let last_d = exts.last().unwrap().descent;
                let mut items = vec![];
                for (i, l) in lines.iter().enumerate() {
                    let yy = base_y - last_d - pitch * (lines.len() - 1 - i) as f64;
                    items.push(kit.text("tile-label", &[], &kit::plain(l), tx + pad, yy, "start")?);
                }
                if let Some(c) = clash {
                    let b0 = items.iter().map(|it| it.bbox()[1]).fold(f64::INFINITY, f64::min);
                    if b0 < c[3] {
                        return Ok(None);
                    }
                }
                Ok(Some(items))
            };
            let mut two = kit::splits(&p.name);
            two.sort_by_key(|(a, b)| a.chars().count().max(b.chars().count()));
            let mut got = None;
            let val = value_text.get(&t.part);
            if let Some(v) = val {
                let mut options = vec![vec![p.name.clone(), v.clone()]];
                options.extend(two.iter().map(|(a, b)| vec![a.clone(), b.clone(), v.clone()]));
                for lines in options {
                    got = fits(&lines)?;
                    if got.is_some() {
                        break;
                    }
                }
            }
            if got.is_none() {
                got = fits(&[p.name.clone()])?;
            }
            if got.is_none() {
                for (a, b) in &two {
                    got = fits(&[a.clone(), b.clone()])?;
                    if got.is_some() {
                        break;
                    }
                }
            }
            if got.is_none() {
                let n = p.name.chars().count();
                for kk in (min_chars..n).rev() {
                    got = fits(&[cut(&p.name, kk)])?;
                    if got.is_some() {
                        cut_.push(p.name.clone());
                        break;
                    }
                }
            }
            let Some(got) = got else {
                unnamed_.push(p.name.clone());
                continue;
            };
            if let Some(v) = val {
                if got.iter().all(|it| it.request.content.readable() != v) {
                    bare_.push(p.name.clone());
                }
            }
            out.extend(got);
        }
        Ok((out, cut_, unnamed_, bare_))
    };
    let (mut got_texts, mut cut_parts, mut unnamed, mut bare) = name_tiles(&BTreeMap::new())?;
    let mut value_text: BTreeMap<usize, String> = BTreeMap::new();
    if let Some(mode) = mode {
        let named: Vec<usize> = tiles
            .iter()
            .map(|t| t.part)
            .filter(|&p| !unnamed.contains(&parts[p].name))
            .collect();
        let vals: Vec<Decimal> = named.iter().map(|&p| size(p)).collect();
        let texts_v = if mode == "percent" {
            let total: Decimal = parts.iter().map(|p| p.values[0]).sum();
            fmt::format_values_with_measure(
                &plan.spec,
                &json!({"kind": "percent"}),
                &vals
                    .iter()
                    .map(|v| Some(*v * Decimal::from(100) / total))
                    .collect::<Vec<_>>(),
            )?
        } else {
            fmt::format_values(
                &plan.spec,
                &vals.iter().map(|v| Some(*v)).collect::<Vec<_>>(),
            )?
        };
        for (p, t) in named.into_iter().zip(texts_v) {
            let t = t.unwrap_or_default();
            kit.need("tile-label", &[], &kit::plain(&t))?;
            value_text.insert(p, t);
        }
        (got_texts, cut_parts, unnamed, bare) = name_tiles(&value_text)?;
    }
    texts.extend(got_texts);
    shortened.extend(cut_parts);

    // the marks: tiles in drawing order, the highlighted last
    let bw = kit.metric("--border-width").unwrap_or(0.2 * em);
    let mut rects: Vec<(bool, Primitive)> = vec![];
    let mut overlay = vec![];
    for t in &tiles {
        let p = &parts[t.part];
        let (tx, ty, tw, th) = t.rect;
        let hit = targets.contains(&t.part);
        let c = colour[&t.part].clone();
        let emph: Vec<&str> = if hit && !border {
            vec!["highlight"]
        } else if dim && !targets.is_empty() && !hit {
            vec!["context"]
        } else {
            vec![]
        };
        let mut classes = vec!["tile", c.as_str()];
        classes.extend(emph.iter());
        let mut data = vec![
            ("series", pie_id.clone()),
            ("x", xs[p.index].clone()),
            ("value", kit::dec_text(p.values[0])),
        ];
        if let Some(g) = &t.group {
            data.push(("group", g.clone()));
        }
        rects.push((
            hit,
            kit::rect(
                tx,
                ty,
                tw,
                th,
                kit::fill(&kit::class_paint(kit, &c, &emph)),
                meta(&classes, &data),
            ),
        ));
        if hit && border {
            overlay.push(kit::rect(
                tx + bw / 2.,
                ty + bw / 2.,
                (tw - bw).max(0.),
                (th - bw).max(0.),
                kit::stroke(&kit::class_paint(kit, &c, &["highlight"]), bw),
                meta(
                    &["border", c.as_str(), "highlight"],
                    &[("series", pie_id.clone()), ("x", xs[p.index].clone())],
                ),
            ));
        }
    }
    let mut lines = vec![];
    if !unnamed.is_empty() {
        lines.push(
            note(spec, "unnamed", "({n} parts too small to name)")
                .replace("{n}", &unnamed.len().to_string()),
        );
    }
    if !zeros.is_empty() {
        lines.push(format!(
            "{} {}",
            zeros
                .iter()
                .map(|&k| parts[k].name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            note(spec, "zero", "(zero)")
        ));
    }
    let mut footer = vec![];
    let mut y = top + h_plot;
    for line in &lines {
        y += pitch;
        footer.push(kit.text("chart-note", &[], &kit::plain(line), 0., y, "start")?);
    }
    let named = parts.len() - zeros.len() - unnamed.len();
    if !unnamed_groups.is_empty() {
        kit::decide(
            &mut decisions,
            "R7-16",
            format!(
                "too small for its name, even shortened: the group {}",
                unnamed_groups.join(", ")
            ),
            None,
        );
    }
    kit::decide(
        &mut decisions,
        "R7-16",
        format!(
            "{named} of {} tiles named inside, at the bottom left{}{} (G7.34)",
            live.len(),
            if shortened.is_empty() {
                String::new()
            } else {
                format!("; {} shortened with “…”", shortened.len())
            },
            if unnamed.is_empty() {
                String::new()
            } else {
                format!(
                    "; {} too small to name, counted in a note (a static chart has no tooltips)",
                    unnamed.len()
                )
            }
        ),
        None,
    );
    if !value_text.is_empty() {
        kit::decide(
            &mut decisions,
            "R7-34",
            format!(
                "the {} under the name in the tiles where both fit, formatted as one set (D-51); the names alone where both do not fit (labels.parts, open)",
                if mode == Some("percent") {
                    "percentage of the total"
                } else {
                    "value"
                }
            ),
            None,
        );
    }
    if !zeros.is_empty() {
        kit::decide(
            &mut decisions,
            "R7-36",
            format!(
                "{}: zero, no tile, named in the note under the chart",
                zeros
                    .iter()
                    .map(|&k| parts[k].name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None,
        );
    }
    if !targets.is_empty() {
        kit::decide(
            &mut decisions,
            "R3-7",
            format!(
                "{}{}{}",
                if border {
                    "a border in the strong shade inside: "
                } else {
                    "highlighted in the strong shade of its colour: "
                },
                targets
                    .iter()
                    .map(|&k| parts[k].name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                if dim { "; the others paled" } else { "" }
            ),
            None,
        );
    }
    let height = footer
        .iter()
        .map(|t| t.bbox()[3])
        .fold(top + h_plot, f64::max)
        + 1.;
    let mut tiles_g: Vec<Primitive> = rects.iter().filter(|r| !r.0).map(|r| r.1.clone()).collect();
    tiles_g.extend(rects.iter().filter(|r| r.0).map(|r| r.1.clone()));
    let series_classes: &[&str] = if grouped {
        &["series"]
    } else {
        &["series", "single"]
    };
    let mut panel_g = vec![kit::group(
        &["marks"],
        None,
        &[],
        vec![kit::group(
            series_classes,
            None,
            &[("series", pie_id.clone())],
            tiles_g,
        )],
    )];
    if !overlay.is_empty() {
        panel_g.push(kit::group(&["overlay"], None, &[], overlay));
    }
    panel_g.push(kit::group(
        &["annotations"],
        None,
        &[],
        texts.iter().map(Placed::primitive).collect(),
    ));
    let tree = vec![
        kit::group(
            &["header"],
            None,
            &[],
            block.texts.iter().map(Placed::primitive).collect(),
        ),
        kit::group(&["panel"], Some("main"), &[], panel_g),
    ];
    let mut primitives: Vec<Primitive> = kit::flatten(&tree).into_iter().cloned().collect();
    primitives.extend(footer.iter().map(Placed::primitive));
    let drawn_n = primitives.len();
    let without: Vec<&String> = bare.iter().filter(|q| !unnamed.contains(q)).collect();
    let mut summary = BTreeMap::new();
    let mut height = height;
    let marks: BTreeMap<(String, String), crate::annotate::Mark> = tiles
        .iter()
        .map(|t| {
            let (tx, ty, tw, th) = t.rect;
            (
                (pie_id.clone(), xs[parts[t.part].index].clone()),
                crate::annotate::Mark::Tile([tx, ty, tx + tw, ty + th]),
            )
        })
        .collect();
    let header_bottom = block.texts.iter().map(|t| t.bbox()[3]).fold(0., f64::max);
    crate::annotate::annotate(
        kit,
        spec,
        &mut primitives,
        w_fig,
        &mut height,
        [0., top, w_fig, top + h_plot],
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
    root.push(kit::group(
        &["footer"],
        None,
        &[],
        footer.iter().map(Placed::primitive).collect(),
    ));
    let primitives = vec![kit::group(
        &["chart", "treemap", "categorical"],
        Some("chart"),
        &[],
        root,
    )];
    summary.insert("kind".into(), json!("treemap"));
    summary.insert(
        "plot".into(),
        json!([0.0, r2(top), r2(w_fig), r2(top + h_plot)]),
    );
    summary.insert(
        "groups".into(),
        json!(group_rects.iter().map(|g| &g.0).collect::<Vec<_>>()),
    );
    summary.insert("named".into(), json!(named));
    summary.insert("shortened".into(), json!(shortened));
    summary.insert("unnamed".into(), json!(unnamed));
    summary.insert("labels".into(), json!(mode.unwrap_or("name")));
    summary.insert("without_value".into(), json!(without));
    summary.insert("width".into(), r2(w_fig));
    summary.insert("height".into(), r2(height));
    let _ = finding;
    Ok(PlacedChart {
        decisions,
        width: w_fig,
        height,
        primitives,
        findings,
        summary,
    })
}
