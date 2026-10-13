//! The message layer's comments and title connector on one total's parts (pies, donuts,
//! treemaps): a port of the original `annotate.py` Annotator (comments, title_link, place,
//! route, arrow, occupancy) as it runs on PART_KINDS (R5-5, RP3-7, G8.7, G8.8).
//!
//! A comment is set in balanced centred lines at a few widths, placed in the free space nearest
//! its anchors (a slice: on its middle ray, part of the way to the rim, and for a wide slice
//! either side; a tile: its middle), each anchor reached by a thin curved arrow clear of every
//! text. A comment finding no free space goes under the chart when the original would put it
//! there; the original's alternative, a column at the right with the chart laid out again
//! narrower, is not ported and is an explicit error.
use super::kit::{self, Kit, Placed, finding};
use chartgen_model::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// A part's geometry for anchoring an arrow.
#[derive(Debug, Clone, Copy)]
pub enum Mark {
    /// A slice: centre, radius, ring radius, angles (degrees clockwise from 12 o'clock).
    Wedge {
        x: f64,
        y: f64,
        r: f64,
        r0: f64,
        a0: f64,
        a1: f64,
        inset: f64,
    },
    /// A tile's box.
    Tile([f64; 4]),
}

struct Occupancy(figkit_placement::Occupancy);
impl Occupancy {
    fn new(w: f64, h: f64) -> Result<Self> {
        let budget = figkit_placement::Budget::new(&figkit_placement::PlacementBudget::default());
        figkit_placement::Occupancy::new(w, h, figkit_placement::Profile::LegacyFamily, &budget)
            .map(Self)
            .map_err(placement_error)
    }
    fn rect(&mut self, b: [f64; 4]) -> Result<()> {
        self.0.rect(b).map_err(placement_error)
    }
    fn segment(&mut self, p: (f64, f64), q: (f64, f64), pad: f64) -> Result<()> {
        self.0.segment(p, q, pad).map_err(placement_error)
    }
    fn try_clone(&self) -> Result<Self> {
        self.0.try_clone().map(Self).map_err(placement_error)
    }
}
fn placement_error(e: figkit_placement::PlacementError) -> Error {
    Error::new("placement", e.to_string())
}

fn grow(b: [f64; 4], p: f64) -> [f64; 4] {
    [b[0] - p, b[1] - p, b[2] + p, b[3] + p]
}
fn box_dist(b: [f64; 4], p: (f64, f64)) -> f64 {
    let dx = (b[0] - p.0).max(0.).max(p.0 - b[2]);
    let dy = (b[1] - p.1).max(0.).max(p.1 - b[3]);
    dx.hypot(dy)
}

/// The box of a placed text primitive (advance width, ink height): chartgen-geometry's
/// `placed_text_bounds`.
fn text_box(p: &Primitive) -> Option<[f64; 4]> {
    if let Primitive::Text {
        position,
        metrics,
        anchor,
        ..
    } = p
    {
        let b = chartgen_geometry::placed_text_bounds(position, anchor, metrics).ok()?;
        return Some([b.x, b.y, b.x + b.width, b.y + b.height]);
    }
    None
}

const CURVES: [&str; 6] = [
    "leader",
    "leader-head",
    "comment-arrow",
    "comment-arrow-head",
    "title-link",
    "title-link-head",
];

/// What a new text or comment must stay clear of (Annotator.occupancy).
fn occupancy(
    prims: &[Primitive],
    slices: &[[f64; 4]],
    w: f64,
    h: f64,
    tpad: f64,
    mpad: f64,
    texts: bool,
    marks: bool,
    lines: bool,
) -> Result<Occupancy> {
    let mut occ = Occupancy::new(w, h.max(1.) + 400.)?;
    if marks {
        for b in slices {
            occ.rect(grow(*b, mpad))?;
        }
    }
    for p in prims {
        match p {
            Primitive::Text { .. } => {
                if texts {
                    occ.rect(grow(text_box(p).unwrap(), tpad))?;
                }
            }
            Primitive::Rect { bounds: b, .. } => {
                if marks {
                    occ.rect(grow([b.x, b.y, b.x + b.width, b.y + b.height], mpad))?;
                }
            }
            Primitive::Circle { center, radius, .. } => {
                if marks {
                    occ.rect(grow(
                        [
                            center.x - radius,
                            center.y - radius,
                            center.x + radius,
                            center.y + radius,
                        ],
                        mpad,
                    ))?;
                }
            }
            Primitive::Line { from, to, .. } => {
                if lines {
                    occ.segment((from.x, from.y), (to.x, to.y), mpad + 0.5)?;
                }
            }
            Primitive::Path {
                points,
                meta,
                paint,
                ..
            } => {
                let classes: Vec<&str> = meta.classes.iter().map(String::as_str).collect();
                if classes.iter().any(|c| CURVES.contains(c)) {
                    if lines {
                        for q in points {
                            occ.rect([
                                q.x - mpad - 0.5,
                                q.y - mpad - 0.5,
                                q.x + mpad + 0.5,
                                q.y + mpad + 0.5,
                            ])?;
                        }
                    }
                } else if classes.contains(&"slice") || classes.contains(&"border") {
                    // slices by their sector's box (from the marks); a border is no obstacle
                } else if lines {
                    let hw = paint.stroke_width / 2.;
                    for s in points.windows(2) {
                        occ.segment((s[0].x, s[0].y), (s[1].x, s[1].y), mpad + hw)?;
                    }
                }
            }
            Primitive::Use { center, size, .. } => {
                if marks {
                    let r = size / 2.;
                    occ.rect(grow(
                        [center.x - r, center.y - r, center.x + r, center.y + r],
                        mpad,
                    ))?;
                }
            }
            Primitive::Curve { .. } => {
                if lines {
                    for q in kit::curve_points(p) {
                        occ.rect([
                            q.0 - mpad - 0.5,
                            q.1 - mpad - 0.5,
                            q.0 + mpad + 0.5,
                            q.1 + mpad + 0.5,
                        ])?;
                    }
                }
            }
            // slices (shapes or wedges) by their sector's box, from the marks; a border is none
            Primitive::Wedge { .. } | Primitive::Shape { .. } => {}
            Primitive::Group { .. } => {}
        }
    }
    Ok(occ)
}

struct Block {
    lines: Vec<String>,
    exts: Vec<kit::Ext>,
    w: f64,
    h: f64,
    pitch: f64,
    native: Option<TextContent>,
}

/// [(i, j)] word spans of lines no wider than limit, the fewest lines, balanced (annotate._wrap).
fn wrap(
    n: usize,
    wd: &BTreeMap<(usize, usize), f64>,
    limit: f64,
    max_lines: usize,
) -> Option<Vec<(usize, usize)>> {
    fn f(
        i: usize,
        m: usize,
        n: usize,
        wd: &BTreeMap<(usize, usize), f64>,
        limit: f64,
        memo: &mut BTreeMap<(usize, usize), (f64, Option<Vec<(usize, usize)>>)>,
    ) -> (f64, Option<Vec<(usize, usize)>>) {
        if i == n {
            return (0., Some(vec![]));
        }
        if m == 0 {
            return (f64::INFINITY, None);
        }
        if let Some(v) = memo.get(&(i, m)) {
            return v.clone();
        }
        let mut res = (f64::INFINITY, None);
        for j in i + 1..=n {
            let Some(&w) = wd.get(&(i, j)) else { break };
            if w > limit {
                break;
            }
            let rest = f(j, m - 1, n, wd, limit, memo);
            let Some(r) = rest.1 else { continue };
            let cand = w.max(rest.0);
            if cand < res.0 {
                let mut l = vec![(i, j)];
                l.extend(r);
                res = (cand, Some(l));
            }
        }
        memo.insert((i, m), res.clone());
        res
    }
    for k in 1..=max_lines {
        let mut memo = BTreeMap::new();
        if let (_, Some(l)) = f(0, k, n, wd, limit, &mut memo) {
            return Some(l);
        }
    }
    None
}

/// The ways to set a comment: centred lines at a few widths (Annotator.blocks).
fn blocks(kit: &Kit, text: &TextContent) -> Result<Vec<Block>> {
    let em = kit.metric("--label-size")?;
    let pitch = kit.metric("--line-pitch")? * kit.metric("--comment-size")? / em;
    let TextContent::Plain { text } = text else {
        let e = kit.ext("comment", &[], text)?;
        return Ok(vec![Block {
            lines: vec![],
            exts: vec![e],
            w: e.width,
            h: e.ascent + e.descent,
            pitch: 0.,
            native: Some(text.clone()),
        }]);
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    let n = words.len();
    let mut spans = BTreeMap::new();
    for i in 0..n {
        for j in i + 1..=(i + 9).min(n) {
            spans.insert((i, j), words[i..j].join(" "));
        }
    }
    for s in spans.values() {
        kit.need("comment", &[], &kit::plain(s))?;
    }
    let mut wd = BTreeMap::new();
    for (k, s) in &spans {
        wd.insert(*k, kit.width("comment", &[], &kit::plain(s))?);
    }
    let base = kit.metric("--comment-width")? * kit.metric("--comment-size")? / em;
    let mut out = vec![];
    let mut seen: Vec<Vec<(usize, usize)>> = vec![];
    for f in [1.0, 0.85, 1.2, 0.7] {
        let Some(lines) = wrap(n, &wd, base * f, 5) else {
            continue;
        };
        if seen.contains(&lines) {
            continue;
        }
        seen.push(lines.clone());
        let plain: Vec<String> = lines.iter().map(|k| spans[k].clone()).collect();
        let exts: Vec<kit::Ext> = plain
            .iter()
            .map(|p| kit.ext("comment", &[], &kit::plain(p)))
            .collect::<Result<_>>()?;
        let w = exts.iter().map(|e| e.width).fold(0., f64::max);
        let h = exts[0].ascent + pitch * (exts.len() - 1) as f64 + exts.last().unwrap().descent;
        out.push(Block {
            lines: plain,
            exts,
            w,
            h,
            pitch,
            native: None,
        });
    }
    Ok(out)
}

/// A target an arrow may point at: (x, y, stop short by r, cost).
type Target = (f64, f64, f64, f64);

/// The points an arrow may point at for a reference (Annotator.anchor, the part kinds).
fn anchor(
    kit: &Kit,
    spec: &Value,
    r: &Value,
    marks: &BTreeMap<(String, String), Mark>,
) -> Result<Vec<Target>> {
    let gap = kit.metric("--comment-arrow-gap")?;
    let em = kit.metric("--label-size")?;
    let Some(name) = r["category"].as_str().or_else(|| r["x"].as_str()) else {
        return Ok(vec![]);
    };
    let first = spec["data"]["series"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|s| matches!(s["role"].as_str(), None | Some("regular")))
                .or(a.first())
        })
        .and_then(|s| s["id"].as_str())
        .unwrap_or("");
    let sid = r["series"].as_str().unwrap_or(first);
    let mark = marks
        .get(&(sid.to_string(), name.to_string()))
        .or_else(|| marks.iter().find(|((_, x), _)| x == name).map(|(_, m)| m));
    Ok(match mark {
        None => vec![],
        Some(Mark::Wedge {
            x,
            y,
            r,
            r0,
            a0,
            a1,
            ..
        }) => {
            let rr = r0 + 0.72 * (r - r0);
            let span = a1 - a0;
            let mut angles = vec![(a0 + span / 2., 0.)];
            if span > 40. {
                angles.push((a0 + 0.3 * span, 0.8 * em));
                angles.push((a0 + 0.7 * span, 0.8 * em));
            }
            angles
                .into_iter()
                .map(|(a, cost)| {
                    let p = chartgen_geometry::Wedge {
                        x: *x,
                        y: *y,
                        r: *r,
                        a0: *a0,
                        a1: *a1,
                        r0: *r0,
                        inset: 0.,
                    }
                    .point(a, Some(rr));
                    let p = (p.x, p.y);
                    (p.0, p.1, gap, cost)
                })
                .collect()
        }
        Some(Mark::Tile(b)) => vec![((b[0] + b[2]) / 2., (b[1] + b[3]) / 2., gap, 0.)],
    })
}

/// Start points on the sides of a comment's box towards a target (Annotator._starts).
fn starts(em: f64, b: [f64; 4], target: (f64, f64)) -> Result<Vec<((f64, f64), (f64, f64), f64)>> {
    Ok(figkit_placement::routing::starts_with_profile(
        em,
        b,
        target,
        figkit_placement::Profile::LegacyFamily,
    )
    .map_err(placement_error)?
    .into_iter()
    .map(|s| (s.position, s.normal, s.cost))
    .collect())
}

struct Router<'a> {
    w: f64,
    h: f64,
    em: f64,
    width: f64,
    ink: &'a str,
}
impl Router<'_> {
    /// The best arrow from one of the starts to one of the targets, clear of every text,
    /// crossing the fewest marks (Annotator.route): (score, its paths), or None.
    #[allow(clippy::too_many_arguments)]
    fn route(
        &self,
        starts: &[((f64, f64), (f64, f64), f64)],
        targets: &[Target],
        b: Option<[f64; 4]>,
        occ_text: &Occupancy,
        occ_marks: &Occupancy,
        class: &str,
    ) -> Result<Option<(f64, Vec<Primitive>, Vec<(f64, f64)>)>> {
        let budget = occ_text.0.budget().clone();
        let start_values: Vec<_> = starts
            .iter()
            .map(|&(position, normal, cost)| figkit_placement::Start {
                position,
                normal,
                cost,
            })
            .collect();
        let target_values: Vec<_> = targets
            .iter()
            .map(|&(x, y, clearance, cost)| figkit_placement::Target {
                position: (x, y),
                clearance,
                cost,
            })
            .collect();
        let options = figkit_placement::RouteOptions {
            origin: (0., 0.),
            profile: figkit_placement::Profile::LegacyFamily,
            width: self.w,
            height: self.h,
            em: self.em,
            head_length: 3.2 + 1.2 * self.width,
        };
        let Some(route) = figkit_placement::route(
            &options,
            &start_values,
            &target_values,
            b,
            &occ_text.0,
            &occ_marks.0,
            &budget,
        )
        .map_err(placement_error)?
        else {
            return Ok(None);
        };
        let Some(connector) = figkit_placement::routing::connector_with_profile(
            &route,
            self.width,
            1.,
            figkit_placement::Profile::LegacyFamily,
            &budget,
        )
        .map_err(placement_error)?
        else {
            return Ok(None);
        };
        let pts =
            figkit_placement::routing::points(&connector, &budget).map_err(placement_error)?;
        let curve = Primitive::Curve {
            start: connector.start,
            segments: connector.segments,
            head: Some(connector.head),
            paint: Paint {
                fill: Some(self.ink.into()),
                stroke: Some(self.ink.into()),
                stroke_width: self.width,
                opacity: 1.,
            },
            meta: kit::meta(&[class], &[]),
        };
        Ok(Some((route.score, vec![curve], pts)))
    }
}

/// Comments and the title connector on a pie, donut or treemap (Annotator.run for PART_KINDS).
/// Adds to the primitives, the summary's `annotations` and the height.
#[allow(clippy::too_many_arguments)]
pub fn annotate(
    kit: &Kit,
    spec: &Value,
    prims: &mut Vec<Primitive>,
    width: f64,
    height: &mut f64,
    plot: [f64; 4],
    header_bottom: f64,
    title_lines: &[Placed],
    marks: &BTreeMap<(String, String), Mark>,
    findings: &mut Vec<Finding>,
    summary: &mut BTreeMap<String, Value>,
    decisions: &mut Vec<Value>,
) -> Result<()> {
    // Commit all chart outputs together only after successful bounded placement.
    let mut next_prims = prims.clone();
    let mut next_height = *height;
    let mut next_findings = findings.clone();
    let mut next_summary = summary.clone();
    let mut next_decisions = decisions.clone();
    annotate_inner(
        kit,
        spec,
        &mut next_prims,
        width,
        &mut next_height,
        plot,
        header_bottom,
        title_lines,
        marks,
        &mut next_findings,
        &mut next_summary,
        &mut next_decisions,
    )?;
    *prims = next_prims;
    *height = next_height;
    *findings = next_findings;
    *summary = next_summary;
    *decisions = next_decisions;
    Ok(())
}

fn annotate_inner(
    kit: &Kit,
    spec: &Value,
    prims: &mut Vec<Primitive>,
    width: f64,
    height: &mut f64,
    plot: [f64; 4],
    header_bottom: f64,
    title_lines: &[Placed],
    marks: &BTreeMap<(String, String), Mark>,
    findings: &mut Vec<Finding>,
    summary: &mut BTreeMap<String, Value>,
    decisions: &mut Vec<Value>,
) -> Result<()> {
    let comments: Vec<&Value> = spec["comments"].as_array().into_iter().flatten().collect();
    let link = spec.pointer("/title/link").filter(|v| !v.is_null());
    if comments.is_empty() && link.is_none() {
        return Ok(());
    }
    let em = kit.metric("--label-size")?;
    let ink = kit::token_paint(kit, "--comment");
    let slices: Vec<[f64; 4]> = marks
        .values()
        .filter_map(|m| match m {
            Mark::Wedge {
                x,
                y,
                r,
                r0,
                a0,
                a1,
                inset,
            } => Some(crate::pie::wedge_box(*x, *y, *r, *a0, *a1, *r0, *inset)),
            Mark::Tile(_) => None,
        })
        .collect::<Result<_>>()?;
    let mut notes = serde_json::Map::new();
    if let Some(link) = link {
        let router = Router {
            w: width,
            h: *height,
            em,
            width: kit.metric("--link-width").unwrap_or(1.),
            ink: &ink,
        };
        let targets = anchor(kit, spec, link, marks)?;
        match title_lines
            .iter()
            .filter(|t| t.role == "title")
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
        {
            Some(last) if !targets.is_empty() => {
                let b = last.bbox();
                let mid = last.y - kit.plain_ext("title", &[], "H")?.ascent / 2.;
                let g = 0.45 * em;
                let st = vec![
                    ((b[2] + g, mid), (1., 0.), 0.),
                    ((b[2] - em, b[3] + g), (0., 1.), 6. * em),
                    ((b[2] - 3. * em, b[3] + g), (0., 1.), 6. * em),
                ];
                let occ_t = occupancy(prims, &slices, width, *height, 1., 1., true, false, false)?;
                let occ_m = occupancy(prims, &slices, width, *height, 0., 0., false, true, true)?;
                match router.route(&st, &targets, None, &occ_t, &occ_m, "title-link")? {
                    Some((_, arrow, _)) => {
                        let (s, e) = arrow_ends(&arrow);
                        notes.insert(
                            "title_link".into(),
                            json!([
                                kit::round2(s.0),
                                kit::round2(s.1),
                                kit::round2(e.0),
                                kit::round2(e.1)
                            ]),
                        );
                        prims.extend(arrow);
                        let r = link;
                        let name = r["category"]
                            .as_str()
                            .or_else(|| r["series"].as_str())
                            .or_else(|| r["x"].as_str())
                            .unwrap_or("");
                        kit::decide(
                            decisions,
                            "6.12",
                            format!(
                                "a thin grey curved arrow from the end of the title to {name} (G8.8)"
                            ),
                            Some("open"),
                        );
                    }
                    None => findings.push(finding(
                        "6.12",
                        "warn",
                        "the title connector finds no way to its mark clear of the text",
                        Some("title.link"),
                    )),
                }
            }
            _ => findings.push(kit::info(
                "6.12",
                "the title connector finds no title line or no mark to point at",
                Some("title.link"),
            )),
        }
    }
    let gap = kit.metric("--comment-gap")?;
    let mut boxes = vec![];
    let mut placed_any = false;
    let mut below_decisions: Vec<Value> = vec![];
    for (k, c) in comments.iter().enumerate() {
        let text = kit::spec_text(&c["text"])?
            .ok_or_else(|| kit::err(format!("comments[{k}] needs text")))?;
        let bl = blocks(kit, &text)?;
        if bl.is_empty() {
            return Err(kit::err(format!(
                "comments[{k}]: no line width of 2–5 lines fits its words"
            )));
        }
        let arrow = c["arrow"].as_bool() != Some(false);
        let anchors: Vec<&Value> = c["anchors"].as_array().into_iter().flatten().collect();
        let mut targets = vec![];
        if arrow {
            for r in &anchors {
                let t = anchor(kit, spec, r, marks)?;
                if !t.is_empty() {
                    targets.push(t);
                }
            }
            if !anchors.is_empty() && targets.len() < anchors.len() {
                findings.push(finding(
                    "RP3-7",
                    "warn",
                    &format!(
                        "comment {}: an anchor names no drawn element; no arrow to it",
                        k + 1
                    ),
                    Some(&format!("comments[{k}].anchors")),
                ));
            }
        }
        let router = Router {
            w: width,
            h: *height,
            em,
            width: kit.metric("--comment-arrow-width")?,
            ink: &ink,
        };
        let placement = c["placement"].as_str().unwrap_or("auto");
        let got = place(
            kit,
            prims,
            &slices,
            width,
            *height,
            plot,
            header_bottom + gap,
            placement,
            &bl,
            &targets,
            &router,
        )?;
        let placed_here = got.is_some();
        let (bi, b, arrows) = match got {
            Some(g) => g,
            None => {
                if placement != "inside"
                    && width - (bl.iter().map(|b| b.w).fold(f64::INFINITY, f64::min) + 2. * gap)
                        >= 120.
                {
                    return Err(kit::err(format!(
                        "comment {} finds no free space: the original lays the chart out again narrower with a column at the right, which is not ported for {} charts",
                        k + 1,
                        spec.pointer("/chart/type").and_then(Value::as_str).unwrap_or("part")
                    ))
                    .at(format!("comments[{k}]")));
                }
                // under the chart (Annotator.below)
                let bi = (0..bl.len())
                    .max_by(|&a, &b| bl[a].w.partial_cmp(&bl[b].w).unwrap())
                    .unwrap();
                let bottom = prims
                    .iter()
                    .filter_map(text_box)
                    .map(|b| b[3])
                    .fold(*height - 1., f64::max);
                let y = bottom + gap;
                let cx = if targets.is_empty() {
                    (plot[0] + plot[2]) / 2.
                } else {
                    targets.iter().map(|t| t[0].0).sum::<f64>() / targets.len() as f64
                };
                let x = (cx - bl[bi].w / 2.).max(0.).min(width - bl[bi].w);
                let b = [x, y, x + bl[bi].w, y + bl[bi].h];
                let occ_t = occupancy(prims, &slices, width, *height, 1., 1., true, false, false)?;
                let occ_m = occupancy(prims, &slices, width, *height, 0., 0., false, true, true)?;
                let mut arrows = vec![];
                for t in &targets {
                    if let Some((_, a, _)) = router.route(
                        &starts(em, b, (t[0].0, t[0].1))?,
                        t,
                        Some(b),
                        &occ_t,
                        &occ_m,
                        "comment-arrow",
                    )? {
                        arrows.push(a);
                    }
                }
                if !targets.is_empty() && arrows.len() < targets.len() {
                    findings.push(finding("RP3-7", "warn", &format!("comment {}: no free space near its anchors; set under the chart, without the arrows that would cross text", k + 1), Some(&format!("comments[{k}]"))));
                } else {
                    findings.push(kit::info(
                        "RP3-7",
                        &format!(
                            "comment {}: no free space near its anchors; set under the chart",
                            k + 1
                        ),
                        Some(&format!("comments[{k}]")),
                    ));
                }
                *height = height.max(b[3] + 1.);
                (bi, b, arrows)
            }
        };
        let block = &bl[bi];
        let cx = (b[0] + b[2]) / 2.;
        let y = b[1] + block.exts[0].ascent;
        match &block.native {
            Some(t) => prims.push(kit.text("comment", &[], t, cx, y, "middle")?.primitive()),
            None => {
                for (j, line) in block.lines.iter().enumerate() {
                    prims.push(
                        kit.text(
                            "comment",
                            &[],
                            &kit::plain(line),
                            cx,
                            y + j as f64 * block.pitch,
                            "middle",
                        )?
                        .primitive(),
                    );
                }
            }
        }
        let arrows_n = arrows.len();
        for a in arrows {
            prims.extend(a);
        }
        let short = {
            let t = text.readable();
            if t.chars().count() <= 40 {
                t.to_string()
            } else {
                format!("{}…", t.chars().take(39).collect::<String>().trim_end())
            }
        };
        let place_words = if !placed_here {
            "under the chart"
        } else if b[0] >= plot[0] - 1.
            && b[2] <= plot[2] + 1.
            && b[1] >= plot[1] - 1.
            && b[3] <= plot[3] + 1.
        {
            "in empty space inside the plot"
        } else {
            "in empty space beside the plot"
        };
        let refs = anchors
            .iter()
            .map(|r| {
                r["category"]
                    .as_str()
                    .or_else(|| r["series"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| r["x"].to_string())
            })
            .collect::<Vec<_>>()
            .join(", ");
        let d = kit::decision(
            "RP3-7",
            format!(
                "comment {} (“{short}”) {place_words}{} (G8.7)",
                k + 1,
                match arrows_n {
                    0 => ", no arrow (a remark on the whole chart)".to_string(),
                    1 => format!(", 1 arrow to {refs}"),
                    n => format!(", {n} arrows to {refs}"),
                }
            ),
            None,
        );
        if placed_here {
            decisions.push(d);
            placed_any = true;
        } else {
            below_decisions.push(d);
        }
        boxes.push(json!([
            kit::round2(b[0]),
            kit::round2(b[1]),
            kit::round2(b[2]),
            kit::round2(b[3])
        ]));
    }
    if placed_any {
        kit::decide(
            decisions,
            "R5-5",
            "the maker's comments in New Computer Modern italic, in the reserved blue, set apart from the labelling and outside its count of text styles (G6.3, D-19)",
            None,
        );
    }
    // R15-1 (G8.3): which of the four questions the title and comments answer
    if !comments.is_empty() {
        let names = [
            ("important", "what matters"),
            ("why_care", "why care"),
            ("explain", "what explains it"),
            ("action", "what to do"),
        ];
        let mut answered: BTreeMap<&str, String> = BTreeMap::new();
        if spec.pointer("/title/kind").and_then(Value::as_str) == Some("insight") {
            answered.insert("important", "the title".into());
        }
        for (k, c) in comments.iter().enumerate() {
            for a in c["answers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                answered
                    .entry(a)
                    .or_insert_with(|| format!("comment {}", k + 1));
            }
        }
        if !answered.is_empty() {
            let got = names
                .iter()
                .filter_map(|(q, n)| answered.get(q).map(|a| format!("{n}: {a}")))
                .collect::<Vec<_>>()
                .join("; ");
            let miss: Vec<&str> = names
                .iter()
                .filter(|(q, _)| !answered.contains_key(q))
                .map(|(_, n)| *n)
                .collect();
            kit::decide(
                decisions,
                "R15-1",
                format!(
                    "the four questions answered: {got}{} (G8.3)",
                    if miss.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " (not answered: {}, which need be answered only where known)",
                            miss.join(", ")
                        )
                    }
                ),
                Some("open"),
            );
        }
    }
    decisions.extend(below_decisions);
    if !boxes.is_empty() {
        notes.insert("comments".into(), json!(boxes));
    }
    if !notes.is_empty() {
        summary.insert("annotations".into(), Value::Object(notes));
    }
    Ok(())
}

fn arrow_ends(prims: &[Primitive]) -> ((f64, f64), (f64, f64)) {
    match prims.first() {
        Some(Primitive::Curve {
            start, segments, ..
        }) if !segments.is_empty() => {
            let b = &segments.last().unwrap().end;
            ((start.x, start.y), (b.x, b.y))
        }
        _ => ((0., 0.), (0., 0.)),
    }
}

type Placement = (usize, [f64; 4], Vec<Vec<Primitive>>);

/// (block, box, arrows) for a comment in the free space, or None (Annotator.place).
#[allow(clippy::too_many_arguments)]
fn place(
    kit: &Kit,
    prims: &[Primitive],
    slices: &[[f64; 4]],
    w: f64,
    h: f64,
    plot: [f64; 4],
    top: f64,
    placement: &str,
    blocks: &[Block],
    targets: &[Vec<Target>],
    router: &Router,
) -> Result<Option<Placement>> {
    let gap = kit.metric("--comment-gap")?;
    let em = kit.metric("--label-size")?;
    let mut occ = occupancy(prims, slices, w, h, gap, gap, true, true, true)?;
    let mut base_text = occupancy(prims, slices, w, h, 1., 1., true, false, false)?;
    let occ_marks = occupancy(prims, slices, w, h, 0., 0., false, true, true)?;
    let [x0, y0, x1, y1] = plot;
    let region = match placement {
        "inside" => [x0, top.max(y0), x1, y1],
        "right" | "below" => return Ok(None),
        _ => [0., top, w, h],
    };
    let shared_targets: Vec<Vec<figkit_placement::Target>> = targets
        .iter()
        .map(|g| {
            g.iter()
                .map(|t| figkit_placement::Target {
                    position: (t.0, t.1),
                    clearance: t.2,
                    cost: t.3,
                })
                .collect()
        })
        .collect();
    let budget = occ.0.budget().clone();
    let sizes: Vec<_> = blocks.iter().map(|b| (b.w, b.h)).collect();
    let cands = figkit_placement::candidates(
        &sizes,
        region,
        plot,
        &shared_targets,
        em,
        &mut occ.0,
        &budget,
    )
    .map_err(placement_error)?;
    let mut tried: Vec<[f64; 4]> = vec![];
    let mut found: Vec<(f64, usize, [f64; 4], Vec<Vec<Primitive>>)> = vec![];
    for (score, bi, bx) in cands {
        if tried
            .iter()
            .any(|t| (bx[0] - t[0]).abs() < 5. && (bx[1] - t[1]).abs() < 5.)
        {
            continue;
        }
        tried.push(bx);
        if tried.len() > 60 || found.len() >= 6 {
            break;
        }
        // as the original: one anchor's arrows are stamped into the shared text occupancy, so a
        // later candidate's arrow keeps off an earlier candidate's (several anchors: a copy)
        let mut copy = if targets.len() > 1 {
            Some(base_text.try_clone()?)
        } else {
            None
        };
        let occ_t = match copy.as_mut() {
            Some(c) => c,
            None => &mut base_text,
        };
        let mut arrows = vec![];
        let mut cost = 0.;
        let mut ok = true;
        for t in targets {
            let aim = if t.len() == 1 {
                (t[0].0, t[0].1)
            } else {
                t.iter()
                    .map(|p| (p.0, p.1))
                    .min_by(|a, b| box_dist(bx, *a).partial_cmp(&box_dist(bx, *b)).unwrap())
                    .unwrap()
            };
            match router.route(
                &starts(em, bx, aim)?,
                t,
                Some(bx),
                occ_t,
                &occ_marks,
                "comment-arrow",
            )? {
                None => {
                    ok = false;
                    break;
                }
                Some((c, a, pts)) => {
                    cost += c;
                    for p in pts {
                        occ_t.rect([p.0 - 0.8, p.1 - 0.8, p.0 + 0.8, p.1 + 0.8])?;
                    }
                    arrows.push(a);
                }
            }
        }
        if ok {
            found.push((
                score + 0.5 * cost / targets.len().max(1) as f64,
                bi,
                bx,
                arrows,
            ));
        }
    }
    Ok(found
        .into_iter()
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .map(|(_, bi, bx, arrows)| (bi, bx, arrows)))
}
