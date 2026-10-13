//! Bumps charts: changes in rank over a regular series, ported from the original's
//! `layout/bumps.py` (notes 06 §5.4, §5.5; G1.52, G7.17, G7.24; D-41): a rank scale with
//! 1 at the top and "Rank" above the numbers, no gridlines; the time axis of equal slots;
//! one line per competitor, coloured top to bottom by the final ranks, drawn with curves
//! flat at every point; each line labelled at its right end at its final rank, tied labels
//! packed about their rank; a highlighted competitor twice as thick, its label bold.
use super::annotate::{Drawn, Laid};
use super::fig::{self, Batch, Item, Txt, attrs};
use super::regular as rg;
use super::step::{r2, refusal, title_plain};
use super::*;
use serde_json::Value;

struct Comp {
    id: String,
    index: usize,
    ranks: Vec<Option<i64>>,
    name: String,
    content: TextContent,
    first: Option<usize>,
    last: Option<usize>,
    hl: bool,
    colour: String,
}

impl Comp {
    fn classes(&self) -> String {
        if self.hl {
            format!("{} highlight", self.colour)
        } else {
            self.colour.clone()
        }
    }
    fn mods(&self) -> Vec<&'static str> {
        if self.hl { vec!["text-strong"] } else { vec![] }
    }
}

/// The rank of each competitor in each period (1 the first; tied values share a rank) and
/// the number of ranks.
fn ranks(plan: &SemanticPlan, data: &PreparedData) -> Result<(Vec<Vec<Option<i64>>>, i64)> {
    let kind = plan
        .spec
        .raw
        .pointer("/data/measure/kind")
        .and_then(Value::as_str);
    let low = plan
        .spec
        .raw
        .pointer("/data/measure/good")
        .and_then(Value::as_str)
        == Some("lower");
    let n = data.categories.len();
    let mut out = vec![vec![None; n]; data.series.len()];
    if kind == Some("rank") {
        for (si, s) in data.series.iter().enumerate() {
            for (j, d) in s.values.iter().enumerate() {
                out[si][j] = d
                    .value
                    .map(|v| {
                        v.trunc()
                            .to_string()
                            .parse::<i64>()
                            .map_err(|_| err("a rank must be a whole number"))
                    })
                    .transpose()?;
            }
        }
    } else {
        for j in 0..n {
            let known: Vec<(usize, Decimal)> = data
                .series
                .iter()
                .enumerate()
                .filter_map(|(si, s)| s.values[j].value.map(|v| (si, v)))
                .collect();
            for (si, v) in &known {
                let better = known
                    .iter()
                    .filter(|(_, w)| if low { w < v } else { w > v })
                    .count() as i64;
                out[*si][j] = Some(better + 1);
            }
        }
    }
    let big = out
        .iter()
        .flatten()
        .flatten()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    Ok((out, big))
}

/// Pack labels at `pitch` without collisions, in the order of their targets (packing.pack).
fn pack(targets: &[f64], pitch: f64, lo: f64, hi: f64) -> Vec<f64> {
    let n = targets.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| {
        targets[*a]
            .partial_cmp(&targets[*b])
            .unwrap()
            .then(a.cmp(b))
    });
    let mut clusters: Vec<Vec<usize>> = order.into_iter().map(|i| vec![i]).collect();
    let mut ys = targets.to_vec();
    loop {
        for c in &clusters {
            let k = c.len() as f64;
            let mut top = c.iter().map(|i| targets[*i]).sum::<f64>() / k - pitch * (k - 1.) / 2.;
            top = top.min(hi - pitch * (k - 1.));
            top = top.max(lo);
            for (j, i) in c.iter().enumerate() {
                ys[*i] = top + j as f64 * pitch;
            }
        }
        let mut merged = false;
        for a in 0..clusters.len().saturating_sub(1) {
            if ys[clusters[a + 1][0]] - ys[*clusters[a].last().unwrap()] < pitch - 1e-6 {
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

fn runs_of(values: &[Option<i64>]) -> Vec<Vec<usize>> {
    let mut out = vec![];
    let mut cur = vec![];
    for (i, v) in values.iter().enumerate() {
        if v.is_none() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(i);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub(crate) fn layout(
    plan: &SemanticPlan,
    data: &PreparedData,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
) -> Result<LayoutStep> {
    let batch = Batch::new(theme, c, m);
    let pass = solve(plan, data, &batch);
    fig::run_pass(c, m, &batch, pass)
}

fn solve(plan: &SemanticPlan, data: &PreparedData, batch: &Batch) -> Result<LayoutStep> {
    let draw = |w: f64| draw(plan, data, batch, w);
    match super::annotate::layout(plan, data, batch, &draw)? {
        Err(findings) => Ok(LayoutStep::Refused { findings }),
        Ok(laid) => {
            let mut summary = laid.summary;
            summary.insert("width".into(), json!(r2(laid.width)));
            summary.insert("height".into(), json!(r2(laid.height)));
            Ok(LayoutStep::Complete {
                chart: PlacedChart {
                    width: laid.width,
                    height: laid.height,
                    primitives: vec![fig::lower(&laid.root, batch.theme)?],
                    findings: laid.findings,
                    summary,
                    decisions: laid.decisions,
                },
            })
        }
    }
}

fn draw(plan: &SemanticPlan, data: &PreparedData, batch: &Batch, w_total: f64) -> Result<Drawn> {
    let raw = &plan.spec.raw;
    let em = batch.metric("--label-size")?;
    let x_kind = raw
        .pointer("/data/x/kind")
        .and_then(Value::as_str)
        .unwrap_or("");
    let grid = raw
        .pointer("/data/x/grid")
        .and_then(Value::as_str)
        .unwrap_or("none");
    if x_kind != "time" || grid == "none" {
        return Ok(Drawn::Refused(vec![refusal(
            plan,
            "R6-7",
            "a bumps chart needs a regular series, one value per period (G1.52)",
            "data.x",
            Some("a regular time grid".into()),
        )]));
    }
    let periods = data
        .categories
        .iter()
        .map(|v| rg::parse(v, grid))
        .collect::<Result<Vec<_>>>()?;
    let n = periods.len();
    if n == 0 {
        return Err(err("a bumps chart needs periods"));
    }
    let (rs, big) = ranks(plan, data)?;
    if let Some(bad) = rs.iter().flatten().flatten().find(|r| **r < 1) {
        return Ok(Drawn::Refused(vec![refusal(
            plan,
            "CG-4",
            format!("a rank below 1 ({bad}): ranks count from 1, the first"),
            "data.series",
            None,
        )]));
    }
    let mut comps: Vec<Comp> = data
        .series
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let known: Vec<usize> = rs[i]
                .iter()
                .enumerate()
                .filter(|(_, r)| r.is_some())
                .map(|(j, _)| j)
                .collect();
            Ok(Comp {
                id: s.id.clone(),
                index: i,
                ranks: rs[i].clone(),
                name: s.name.clone(),
                content: guides::series_content(plan, data, i)?,
                first: known.first().copied(),
                last: known.last().copied(),
                hl: false,
                colour: String::new(),
            })
        })
        .collect::<Result<_>>()?;
    // highlight: what the highlight entries name, else what the insight names
    let mut decisions: Vec<Value> = vec![];
    let mut ids = vec![];
    if let Some(hl) = raw
        .get("highlight")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    {
        for h in hl {
            for r in h["targets"].as_array().into_iter().flatten() {
                if let Some(s) = r["series"].as_str() {
                    ids.push(s.to_owned());
                }
            }
        }
    } else if let Some(t) = raw
        .pointer("/insight/targets")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    {
        decisions.push(fig::decision(
            "R15-5",
            "highlighting what the insight names",
            None,
        ));
        for r in t {
            if let Some(s) = r["series"].as_str() {
                ids.push(s.to_owned());
            }
        }
    }
    for c in &mut comps {
        c.hl = ids.contains(&c.id);
    }
    // colours top to bottom by the final ranks, unless fixed
    let mut order: Vec<usize> = (0..comps.len()).collect();
    order.sort_by_key(|&i| {
        (
            comps[i]
                .last
                .and_then(|l| comps[i].ranks[l])
                .unwrap_or(big + 1),
            comps[i].index,
        )
    });
    let fixed: BTreeMap<String, u64> = raw["data"]["series"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s["id"].as_str()?.to_owned(), s["slot"].as_u64()?)))
        .collect();
    let taken: Vec<u64> = fixed.values().copied().collect();
    let mut free = (1..=16u64).filter(|k| !taken.contains(k));
    for &i in &order {
        let slot = match fixed.get(&comps[i].id) {
            Some(k) => *k,
            None => free
                .next()
                .ok_or_else(|| err("more competitors than the palette's sixteen slots"))?,
        };
        comps[i].colour = format!("series-{slot}");
    }
    // the colours' decision (R6-28) is the colour planner's: it falls here, as the original's
    decisions.push(json!({"colours": "here"}));
    let mut findings = vec![];
    if comps.len() > 10 {
        findings.push(Finding::new(
            "R6-7",
            "warning",
            format!(
                "{} competitors: a bumps chart holds about ten lines (D-41)",
                comps.len()
            ),
        ));
    }
    // the batch
    let rank_codes: Vec<String> = (1..=big).map(|k| k.to_string()).collect();
    for c in &rank_codes {
        batch.need("scale-label", c)?;
    }
    batch.need("unit-line", "Rank")?;
    for c in &comps {
        batch.ext_of_mod("end-label", &c.content, &c.mods())?;
    }
    batch.need("end-label", "H")?;
    for (role, t) in rg::candidates(&periods) {
        batch.need(role, &t)?;
    }
    batch.need("time-label", "H")?;
    fig::need_title(plan, batch)?;
    let x_title = guides::content(raw.pointer("/data/x/title").unwrap_or(&Value::Null))?;
    if let Some(t) = &x_title {
        batch.need_content("axis-title", t)?;
    }

    let h0 = batch.ext("scale-label", "1")?.ascent;
    let cap_e = batch.ext("end-label", "H")?.ascent;
    let lpitch = batch.metric("--end-label-pitch")?;
    let gap = batch.metric("--end-label-gap")?;
    let lw = batch.metric("--line-width")?;
    let lw_hi = batch.metric("--line-width-highlight")?;
    let mut xr: f64 = 0.;
    for c in &rank_codes {
        xr = xr.max(batch.width("scale-label", c)?);
    }
    let plot_l = xr + batch.metric("--scale-gap")?;
    let mut lab_w: f64 = 0.;
    for c in &comps {
        lab_w = lab_w.max(
            batch
                .ext_of_mod("end-label", &c.content, &c.mods())?
                .2
                .width,
        );
    }
    let clear = batch.metric("--divider-clear")?;
    let sep = batch.metric("--time-label-sep")?;
    let wfn = |r: &str, t: &str| batch.width(r, t);
    let mut edge = (0.0f64, 0.0f64);
    for _ in 0..6 {
        let w = (w_total - plot_l - edge.0 - gap - lab_w) / (n as f64 - 0.5);
        let rows = match rg::fit(&periods, w, &wfn, sep, Some(clear))? {
            Ok(r) => r,
            Err(_) => break,
        };
        let need = rg::edge_needs(&rows, w, &wfn, clear)?;
        if need.0 <= edge.0 + 0.01 && need.1 <= edge.1 + 0.01 {
            break;
        }
        edge = (edge.0.max(need.0 + 0.05), edge.1.max(need.1 + 0.05));
    }
    let w = (w_total - plot_l - edge.0 - gap - lab_w) / (n as f64 - 0.5);
    let s0 = plot_l + edge.0;
    let plot_r = s0 + (n as f64 - 0.5) * w + (w / 2.).min(gap / 2.);
    let cx: Vec<f64> = (0..n).map(|i| s0 + (i as f64 + 0.5) * w).collect();
    let aspect = match raw.pointer("/layout/plot_aspect") {
        Some(Value::Number(v)) => v.as_f64().unwrap_or(0.6),
        _ => batch.metric("--plot-aspect")?,
    };
    if edge != (0., 0.) {
        decisions.push(fig::decision(
            "R2-39",
            format!(
                "time axis: the first and last labels kept {} pt clear of the outer dividers, centred on their slots, which start {:.1} pt in from the left divider and end {:.1} pt before the right one",
                fig::fmt_g(clear),
                edge.0,
                edge.1
            ),
            None,
        ));
    }
    let plot_h = (aspect * (plot_r - plot_l)).max((big - 1) as f64 * lpitch);
    if plot_h > aspect * (plot_r - plot_l) + 0.01 {
        decisions.push(fig::decision(
            "R6-7",
            format!(
                "the plot is {plot_h:.0} pt high, so that the {big} ranks stand a label's pitch apart (G7.24)"
            ),
            None,
        ));
    }
    let step = if big > 1 {
        plot_h / (big - 1) as f64
    } else {
        plot_h
    };
    let yr = |r: i64| (r - 1) as f64 * step;

    // the lines: flat-tangent curves through the ranks (R6-40)
    let mut paths: BTreeMap<usize, Vec<Item>> = BTreeMap::new();
    let mut dots = vec![];
    for (ci, c) in comps.iter().enumerate() {
        let runs = runs_of(&c.ranks);
        let mut ps = vec![];
        for run in &runs {
            if run.len() < 2 {
                continue;
            }
            ps.push(Item::flat_path(
                run.iter()
                    .map(|&i| (cx[i], yr(c.ranks[i].unwrap())))
                    .collect(),
                &format!("line {}", c.classes()),
                attrs(&[
                    ("series", c.id.clone()),
                    ("from", periods[run[0]].text.clone()),
                    ("to", periods[*run.last().unwrap()].text.clone()),
                ]),
            ));
        }
        paths.insert(ci, ps);
        let mut ends: Vec<usize> = [c.first, c.last].into_iter().flatten().collect();
        ends.sort();
        ends.dedup();
        for i in ends {
            let alone = runs
                .iter()
                .find(|r| r.contains(&i))
                .is_some_and(|r| r.len() == 1);
            if alone || (Some(i) == c.first && i > 0) || (Some(i) == c.last && i < n - 1) {
                let size = batch.metric("--marker-size")? * if c.hl { lw_hi / lw } else { 1. };
                dots.push(Item::Circle {
                    cx: cx[i],
                    cy: yr(c.ranks[i].unwrap()),
                    r: size / 2.,
                    classes: fig::cls(&format!("point {}", c.classes())),
                    data: attrs(&[
                        ("series", c.id.clone()),
                        ("x", periods[i].text.clone()),
                        ("value", c.ranks[i].unwrap().to_string()),
                    ]),
                });
            }
        }
    }

    // the end labels: at the final ranks, in their order (G7.24)
    let lab: Vec<usize> = (0..comps.len())
        .filter(|i| comps[*i].last.is_some())
        .collect();
    let targets: Vec<f64> = lab
        .iter()
        .map(|&i| yr(comps[i].ranks[comps[i].last.unwrap()].unwrap()))
        .collect();
    decisions.push(fig::decision(
        "R6-40",
        "each line drawn with curves flat at every point, so that it never overshoots the ranks it joins (G7.17, G7.24)",
        None,
    ));
    let late: Vec<&str> = comps
        .iter()
        .filter(|c| c.first.unwrap_or(0) > 0 || c.last.is_some_and(|l| l < n - 1))
        .map(|c| c.name.as_str())
        .collect();
    if !late.is_empty() {
        decisions.push(fig::decision(
            "R6-7",
            format!(
                "a line that starts late or ends early has a dot at that end (D-41): {}",
                late.join(", ")
            ),
            None,
        ));
    }
    let mut ties: Vec<(String, i64)> = vec![];
    for i in 0..n {
        let at: Vec<i64> = comps.iter().filter_map(|c| c.ranks[i]).collect();
        for r in &at {
            if at.iter().filter(|x| *x == r).count() > 1 {
                ties.push((periods[i].text.clone(), *r));
            }
        }
    }
    ties.sort();
    ties.dedup();
    if !ties.is_empty() {
        decisions.push(fig::decision(
            "R6-7",
            format!("tied ranks share their height (D-41): {} ties", ties.len()),
            None,
        ));
    }
    let ys = pack(&targets, lpitch, -step / 2., plot_h + step / 2.);
    let max_shift = ys
        .iter()
        .zip(&targets)
        .map(|(y, t)| (y - t).abs() / lpitch)
        .fold(0.0f64, f64::max);
    decisions.push(fig::decision(
        "R6-32",
        format!(
            "each line labelled at its right end, at its final rank, so the labels read in final-rank order (G7.24){}",
            if max_shift > 0.02 {
                format!("; tied labels packed: moved at most {max_shift:.2} line heights")
            } else {
                String::new()
            }
        ),
        None,
    ));
    if comps.iter().any(|c| c.hl) {
        decisions.push(fig::decision(
            "R6-29",
            format!(
                "highlighted, twice as thick, in the strong shade of its hue, its label bold: {}; the others keep their colours and widths",
                comps
                    .iter()
                    .filter(|c| c.hl)
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None,
        ));
    }
    const SCALE: usize = 0;
    const AXIS: usize = 1;
    const NOTES: usize = 2;
    let mut texts: Vec<(usize, Txt)> = vec![];
    for (&i, y) in lab.iter().zip(&ys) {
        let c = &comps[i];
        let last = c.last.unwrap();
        let xa = if last < n - 1 {
            cx[last] + gap
        } else {
            (cx[last] + gap).max(plot_r + gap / 2.)
        };
        let mut t = batch.text_mod(
            "end-label",
            c.content.clone(),
            xa,
            y + cap_e / 2.,
            "start",
            &c.mods(),
        )?;
        t.plain = c.name.clone();
        texts.push((NOTES, t));
    }
    // the rank scale: numbers at the left, "Rank" above them, no gridlines (G7.24)
    for (k, code) in rank_codes.iter().enumerate() {
        texts.push((
            SCALE,
            batch.plain("scale-label", code, xr, yr(k as i64 + 1) + h0 / 2., "end")?,
        ));
    }
    let pitch = batch.metric("--line-pitch")?;
    let e_rank = batch.ext("unit-line", "Rank")?;
    texts.push((
        SCALE,
        batch.plain("unit-line", "Rank", 0., h0 / 2. - pitch, "start")?,
    ));

    decisions.push(fig::decision(
        "R6-7",
        "no gridlines: the rank numbers stand beside each row, and the lines are read by their order (G7.24)",
        None,
    ));
    // the time axis (6.3)
    let mut lines: Vec<Item> = vec![];
    let base = if big > 1 { plot_h + step / 2. } else { plot_h };
    let cap_t = batch.ext("time-label", "H")?.ascent;
    let mut rows = match rg::fit(&periods, w, &wfn, sep, Some(clear))? {
        Ok(r) => r,
        Err(e) => {
            return Ok(Drawn::Refused(vec![refusal(
                plan,
                "R2-40",
                e.message.clone(),
                "layout.width",
                Some(rg::too_narrow(&e, w_total, n, clear)),
            )]));
        }
    };
    for d in &rows.decisions {
        decisions.push(fig::decision("R2-40", d.clone(), None));
    }
    let (groups, dec2) = rg::fit_row2(&rows, w, &wfn, sep)?;
    for d in dec2 {
        decisions.push(fig::decision("R2-39", d, None));
    }
    rows.groups = groups;
    if rg::title_gives(&rows, &title_plain(plan)) {
        decisions.push(fig::decision(
            "R2-39",
            format!(
                "time axis: row 2 (“{}”) left out, the title gives it (G2.37)",
                rows.groups[0].2.clone().unwrap_or_default()
            ),
            None,
        ));
        rows.groups = rows
            .groups
            .into_iter()
            .map(|(a, b, _)| (a, b, None))
            .collect();
    }
    let b1 = base + batch.metric("--category-gap")? + cap_t;
    let b2 = b1 + batch.metric("--time-row-pitch")?;
    let mut last_b = b1;
    for &i in &rows.shown {
        texts.push((
            AXIS,
            batch.plain("time-label", &rows.labels[i], cx[i], b1, "middle")?,
        ));
    }
    let any2 = rows.groups.iter().any(|g| g.2.is_some());
    if any2 {
        last_b = b2;
        for (a, b, t) in &rows.groups {
            let Some(t) = t else { continue };
            let xa = if *a == 0 && rows.groups.len() >= 2 {
                plot_l
            } else {
                s0 + *a as f64 * w
            };
            let xb = if *b == n - 1 && rows.groups.len() >= 2 {
                s0 + n as f64 * w + edge.1
            } else {
                s0 + (*b + 1) as f64 * w
            };
            texts.push((
                AXIS,
                batch.plain("time-label-2", t, (xa + xb) / 2., b2, "middle")?,
            ));
        }
    }
    if rows.groups.len() >= 2 {
        let y_end = if any2 {
            let mut d: f64 = f64::NEG_INFINITY;
            for (_, _, t) in &rows.groups {
                if let Some(t) = t {
                    d = d.max(batch.ext("time-label-2", t)?.descent);
                }
            }
            b2 + d
        } else {
            b1
        } + 0.3 * em;
        let mut xs = vec![plot_l];
        xs.extend(rg::boundaries(&rows).iter().map(|a| s0 + *a as f64 * w));
        xs.push(s0 + n as f64 * w + edge.1);
        for xd in xs {
            lines.push(Item::line(xd, base, xd, y_end, "time-divider", vec![]));
        }
    }
    if let Some(t) = &x_title {
        texts.push((
            AXIS,
            batch.text(
                "axis-title",
                t.clone(),
                (plot_l + plot_r) / 2.,
                last_b + 1.6 * em,
                "middle",
            )?,
        ));
    }

    // the title above; everything shifted under it
    let block = fig::title_block(plan, batch, w_total, &mut findings)?;
    decisions.extend(block.decisions.iter().cloned());
    let first_local = h0 / 2. - pitch;
    let dy = match block.last_baseline {
        Some(b) => b + batch.metric("--title-gap")? - first_local,
        None => e_rank.ascent - first_local,
    };
    for (_, t) in &mut texts {
        t.y += dy;
    }
    for it in lines.iter_mut().chain(dots.iter_mut()) {
        it.shift(0., dy);
    }
    for ps in paths.values_mut() {
        for p in ps {
            p.shift(0., dy);
        }
    }
    let group_of = |g: usize| -> Vec<Item> {
        texts
            .iter()
            .filter(|(k, _)| *k == g)
            .map(|(_, t)| Item::Text(t.clone()))
            .collect()
    };
    let mut marks = vec![];
    let mut draw_order: Vec<usize> = (0..comps.len()).collect();
    draw_order.sort_by_key(|&i| (comps[i].hl, comps[i].index));
    for i in draw_order {
        let ps = &paths[&i];
        if !ps.is_empty() {
            marks.push(Item::Group {
                classes: fig::cls(&format!("series {}", comps[i].classes())),
                id: None,
                data: attrs(&[("series", comps[i].id.clone())]),
                children: ps.clone(),
            });
        }
    }
    if !dots.is_empty() {
        marks.push(Item::group("points", dots));
    }
    let axis_children = super::step::add_halos(group_of(AXIS), &lines, clear);
    let height = texts
        .iter()
        .map(|(_, t)| t.bbox(0.).3)
        .chain(block.texts.iter().map(|t| t.bbox(0.).3))
        .fold(dy + base, f64::max)
        + 1.0;
    let root = Item::Group {
        classes: fig::cls("chart bumps vertical time"),
        id: Some("chart".into()),
        data: vec![],
        children: vec![
            Item::group(
                "header",
                block.texts.iter().cloned().map(Item::Text).collect(),
            ),
            Item::Group {
                classes: fig::cls("panel"),
                id: Some("main".into()),
                data: vec![],
                children: vec![
                    Item::group("grid", lines),
                    Item::group("marks", marks),
                    Item::group("scale value", group_of(SCALE)),
                    Item::group("time-axis", axis_children),
                    Item::group("annotations", group_of(NOTES)),
                ],
            },
            Item::group("footer", vec![]),
        ],
    };
    let mut summary = BTreeMap::new();
    summary.insert("orientation".into(), json!("vertical"));
    summary.insert(
        "plot".into(),
        json!([r2(plot_l), r2(dy), r2(plot_r), r2(dy + plot_h)]),
    );
    summary.insert("slot".into(), json!(r2(w)));
    summary.insert("ranks".into(), json!(big));
    summary.insert(
        "colours".into(),
        Value::Object(
            comps
                .iter()
                .map(|c| (c.id.clone(), json!(c.colour)))
                .collect(),
        ),
    );
    // the report's scale entries (compute_scale's rank scale: 1 … N by 1, "Rank" its line)
    let top = big.max(2);
    summary.insert("scale".into(), json!(format!("1–{top} by 1")));
    summary.insert(
        "labels".into(),
        json!((1..=top).map(|k| k.to_string()).collect::<Vec<_>>()),
    );
    summary.insert("top".into(), json!(top.to_string()));
    summary.insert("scale_title".into(), Value::Null);
    Ok(Drawn::Laid(Laid {
        root,
        width: w_total,
        height,
        plot: (plot_l, dy, plot_r, dy + plot_h),
        findings,
        summary,
        time: None,
        decisions,
    }))
}
