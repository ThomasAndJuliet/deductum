//! Good and bad told apart without colour, in black and white (the original's
//! annotate.Annotator.textures, texture_pattern and _two_tones; D-18, R15-12): every good or bad
//! area mark laid over with its own colour as two tones of it interleaved, a 45° hatch for bad
//! and an ordered (bayer) dither for good, their mean in linear light the colour itself; a dot
//! plot's bad dot filled instead.
use crate::{decision, finding};
use chartgen_colour_space::{from_gamma, linear_srgb_to_oklab, okhsl, srgb_to_okhsl, to_gamma};
use chartgen_model::{Finding, Metadata, Point, Primitive, Theme};
use serde_json::Value;
use std::collections::BTreeMap;

const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
const HATCH_PITCH: f64 = 0.8; // texture.PITCH
const BAYER_PITCH: f64 = 0.35;
const BAYER: [[u32; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];
/// The good/bad area marks the textures lay over, by their mark class (annotate.AREA_KINDS).
const AREA_KINDS: [&str; 9] = [
    "bar",
    "segment",
    "slice",
    "tile",
    "cell",
    "legend-swatch",
    "action-dot",
    "event-bar",
    "area",
];

fn rgb255(h: &str) -> Option<[f64; 3]> {
    let h = h.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16)
            .ok()
            .map(|v| v as f64 / 255.)
    };
    Some([p(0)?, p(2)?, p(4)?])
}
/// texture.linear: the colour in linear light.
fn linear(h: &str) -> [f64; 3] {
    rgb255(h).unwrap_or([0.; 3]).map(from_gamma)
}
fn luma(c: [f64; 3]) -> f64 {
    c[0] * LUMA[0] + c[1] * LUMA[1] + c[2] * LUMA[2]
}
/// texture.hexcode: Python's round of each channel.
fn hexcode(lin: [f64; 3]) -> String {
    let c = |v: f64| (255. * to_gamma(v)).round_ties_even() as u8;
    format!("#{:02x}{:02x}{:02x}", c(lin[0]), c(lin[1]), c(lin[2]))
}
fn hsl(h: &str) -> [f64; 3] {
    let [r, g, b] = rgb255(h).unwrap_or([0.; 3]);
    srgb_to_okhsl(r, g, b)
}

/// annotate._two_tones: (ground, marks, coverage), or None where the colour has no room for two
/// tones between those lightnesses.
fn two_tones(
    fill: &str,
    ground_l: f64,
    mark_l: f64,
    levels: Option<u32>,
    shift: f64,
) -> Option<(String, String, f64)> {
    let lab = |hx: &str| {
        let l = linear(hx);
        linear_srgb_to_oklab(l[0], l[1], l[2])
    };
    let [h, sat, _] = hsl(fill);
    let yc = luma(linear(fill));
    let mut best: Option<(f64, String, String, f64)> = None;
    for sg in [0.8, 0.9, 1.0, 1.1, 1.2, 1.3] {
        for sm in [0.8, 0.9, 1.0, 1.1, 1.2, 1.3] {
            let g = okhsl((h + shift).rem_euclid(360.), (sat * sg).min(100.), ground_l);
            let (hm, ms) = ((h - shift).rem_euclid(360.), (sat * sm).min(100.));
            let mut m = okhsl(hm, ms, mark_l);
            let (yg, ym) = (luma(linear(&g)), luma(linear(&m)));
            if !(ym < yc && yc < yg) {
                continue;
            }
            let mut a = (yg - yc) / (yg - ym);
            if let Some(n) = levels {
                let n = n as f64;
                a = ((a * n).round_ties_even().max(1.)).min(n - 1.) / n;
                let want = yg - (yg - yc) / a;
                let (mut lo, mut hi) = (0.0f64, 100.0f64);
                for _ in 0..40 {
                    let mid = (lo + hi) / 2.;
                    if luma(linear(&okhsl(hm, ms, mid))) < want {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                m = okhsl(hm, ms, (lo + hi) / 2.);
            }
            let (lm, lg) = (linear(&m), linear(&g));
            let mean = hexcode([0, 1, 2].map(|i| a * lm[i] + (1. - a) * lg[i]));
            let (x, y) = (lab(&mean), lab(fill));
            let d = ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt();
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, g, m, a));
            }
        }
    }
    best.map(|b| (b.1, b.2, b.3))
}

fn tok(theme: &Theme, k: &str) -> Option<String> {
    theme
        .tokens
        .get(k)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
}

/// annotate.fill_of: the flat fill the stylesheet's class rules give a mark of these classes under
/// ancestors carrying `ctx` (class selectors only; more classes stronger, the later on a tie).
fn fill_of(rules: &[chartgen_style::Rule], classes: &[String], ctx: &[String]) -> Option<String> {
    let mut best: Option<((usize, usize), String)> = None;
    let mut anc: Vec<&str> = ctx.iter().map(String::as_str).collect();
    anc.push("chart");
    for (order, (sel, decls)) in rules.iter().enumerate() {
        let v = decls
            .iter()
            .find(|(p, _)| p == "fill")
            .map(|(_, v)| v.trim().to_lowercase())
            .unwrap_or_default();
        if !v.starts_with('#') {
            continue;
        }
        for one in sel.split(',') {
            let parts: Vec<&str> = one.split_whitespace().collect();
            let simple = |p: &str| {
                p.starts_with('.')
                    && p[1..].split('.').all(|c| {
                        !c.is_empty()
                            && c.chars()
                                .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
                    })
            };
            if parts.is_empty() || !parts.iter().all(|p| simple(p)) {
                continue;
            }
            let need: Vec<&str> = parts[parts.len() - 1][1..].split('.').collect();
            let up: Vec<&str> = parts[..parts.len() - 1]
                .iter()
                .flat_map(|p| p[1..].split('.'))
                .collect();
            if need.iter().all(|c| classes.iter().any(|x| x == c))
                && up.iter().all(|c| anc.contains(c))
            {
                let key = (need.len() + up.len(), order);
                if best.as_ref().is_none_or(|b| key >= b.0) {
                    best = Some((key, v.clone()));
                }
            }
        }
    }
    let h = best?.1;
    let h = h.trim_start_matches('#');
    Some(format!(
        "#{}",
        if h.len() == 3 {
            h.chars().flat_map(|c| [c, c]).collect()
        } else {
            h.to_owned()
        }
    ))
}

/// What the textures did: the pattern definitions, their decisions, the kinds of mark textured
/// (in the order first met) and whether flat-filled valued cells were left untextured.
#[derive(Debug, Default)]
pub struct Textured {
    pub defs: Vec<String>,
    pub decisions: Vec<Value>,
    pub findings: Vec<Finding>,
    pub kinds: Vec<(String, usize)>,
    pub untextured: usize,
}

fn side(classes: &[String]) -> Option<&'static str> {
    if classes.iter().any(|c| c == "bad") {
        Some("bad")
    } else if classes.iter().any(|c| c == "good") {
        Some("good")
    } else {
        None
    }
}

struct Patterns<'a> {
    theme: &'a Theme,
    made: BTreeMap<(String, String), Option<String>>,
    out: Textured,
}

impl Patterns<'_> {
    /// annotate.texture_pattern: the id of the side's pattern for this fill, made once.
    fn pattern(&mut self, side: &str, fill: &str) -> Option<String> {
        let key = (side.to_owned(), fill.to_owned());
        if let Some(p) = self.made.get(&key) {
            return p.clone();
        }
        let p = self.make(side, fill);
        self.made.insert(key, p.clone());
        p
    }
    fn make(&mut self, side: &str, fill: &str) -> Option<String> {
        let colour = tok(self.theme, &format!("--{side}"))?.to_lowercase();
        let fill = fill.to_lowercase();
        let spec = tok(self.theme, &format!("--texture-{side}")).unwrap_or_default();
        let spec: Vec<&str> = spec.split_whitespace().collect();
        let system = spec
            .first()
            .copied()
            .unwrap_or(if side == "bad" { "hatch" } else { "bayer" });
        let (mut g, mut m) = if spec.len() >= 3 {
            (spec[1].parse().unwrap_or(0.), spec[2].parse().unwrap_or(0.))
        } else if side == "bad" {
            (62.0, 34.0)
        } else {
            (72.0, 46.0)
        };
        let mut pid = format!("texture-{side}");
        if fill != colour {
            let (lb, lf) = (hsl(&colour)[2], hsl(&fill)[2]);
            g = (lf + g - lb).min(97.);
            m = (lf - (lb - m)).max(3.);
            pid = format!("texture-{side}-{}", &fill[1..]);
        }
        let shift = tok(self.theme, "--texture-hue-shift")
            .and_then(|v| {
                v.split_whitespace()
                    .next()
                    .and_then(|n| n.parse::<f64>().ok())
            })
            .unwrap_or(0.);
        let Some((gr, mk, a)) = two_tones(&fill, g, m, (system == "bayer").then_some(8), shift)
        else {
            self.out.findings.push(finding(
                "R15-12",
                "info",
                "locked",
                format!("{fill} ({side}) leaves no room for two tones of it: drawn flat"),
                Some("data.measure.good".into()),
            ));
            return None;
        };
        let pat = if system == "hatch" {
            let p = 2. * HATCH_PITCH;
            format!(
                "<pattern id=\"{pid}\" patternUnits=\"userSpaceOnUse\" width=\"{p:.2}\" height=\"{p:.2}\" patternTransform=\"rotate(45)\"><rect width=\"{p:.2}\" height=\"{p:.2}\" fill=\"{gr}\"/><rect width=\"{:.3}\" height=\"{p:.2}\" fill=\"{mk}\"/></pattern>",
                a * p
            )
        } else {
            let p = 2. * BAYER_PITCH;
            let mut cells = String::new();
            for (j, row) in BAYER.iter().enumerate() {
                for (i, v) in row.iter().enumerate() {
                    if (*v as f64 + 0.5) / 64. < a {
                        cells.push_str(&format!(
                            "M{:.2},{:.2}h{p:.2}v{p:.2}h-{p:.2}z",
                            i as f64 * p,
                            j as f64 * p
                        ));
                    }
                }
            }
            format!(
                "<pattern id=\"{pid}\" patternUnits=\"userSpaceOnUse\" width=\"{w:.2}\" height=\"{w:.2}\"><rect width=\"{w:.2}\" height=\"{w:.2}\" fill=\"{gr}\"/><path d=\"{cells}\" fill=\"{mk}\"/></pattern>",
                w = 8. * p
            )
        };
        let (lm, lg) = (linear(&mk), linear(&gr));
        let mean = hexcode([0, 1, 2].map(|i| a * lm[i] + (1. - a) * lg[i]));
        let shown = if fill == colour {
            String::new()
        } else {
            format!(" ({fill})")
        };
        self.out.decisions.push(decision(
            "R15-12",
            "locked",
            format!(
                "{side}{shown}: {} of {gr} and {mk} ({a:.3} of the area the darker; their mean {mean}, the colour {fill}){}",
                if system == "hatch" { "a 45° hatch" } else { "an ordered dither" },
                if shift != 0. { format!(", the tones turned ±{shift}° in hue") } else { String::new() }
            ),
        ));
        self.out.defs.push(pat);
        Some(pid)
    }
    fn count(&mut self, kind: &str) {
        match self.out.kinds.iter_mut().find(|k| k.0 == kind) {
            Some(k) => k.1 += 1,
            None => self.out.kinds.push((kind.to_owned(), 1)),
        }
    }
}

fn is_marker(p: &Primitive) -> bool {
    matches!(
        p,
        Primitive::Text { .. }
            | Primitive::Line { .. }
            | Primitive::Path { .. }
            | Primitive::Curve { .. }
    )
}

/// Lay the textures over a placed chart's good and bad area marks (annotate.textures), each
/// overlay a `textures` group painted with its pattern right after its mark. `safe`: the chart
/// carries `cvd-safe`, whose rules give the safe pair's fills.
pub fn apply(
    primitives: &mut Vec<Primitive>,
    theme: &Theme,
    safe: bool,
    dot_chart: bool,
) -> Textured {
    let rules = chartgen_style::parse(&chartgen_style::strip_comments(&theme.stylesheet));
    let mut pats = Patterns {
        theme,
        made: BTreeMap::new(),
        out: Textured::default(),
    };
    fn labelled(ps: &[Primitive]) -> bool {
        ps.iter().any(|p| match p {
            Primitive::Group { children, .. } => labelled(children),
            p => {
                p.meta().classes.iter().any(|c| c == "labelled")
                    && side(&p.meta().classes).is_some()
            }
        })
    }
    let any_labelled = labelled(primitives);
    let ctx = if safe {
        vec!["cvd-safe".to_owned()]
    } else {
        vec![]
    };
    visit(
        primitives,
        &ctx,
        &rules,
        safe,
        any_labelled,
        dot_chart,
        &mut pats,
    );
    pats.out
}

fn visit(
    items: &mut Vec<Primitive>,
    ctx: &[String],
    rules: &[chartgen_style::Rule],
    safe: bool,
    labelled: bool,
    dot_chart: bool,
    pats: &mut Patterns,
) {
    let old = std::mem::take(items);
    for mut it in old {
        if let Primitive::Group { children, meta, .. } = &mut it {
            let mut inner = ctx.to_vec();
            inner.extend(meta.classes.iter().cloned());
            visit(children, &inner, rules, safe, labelled, dot_chart, pats);
            items.push(it);
            continue;
        }
        let cls = it.meta().classes.clone();
        let Some(sd) = side(&cls) else {
            items.push(it);
            continue;
        };
        if cls.iter().any(|c| c == "texture") || is_marker(&it) {
            items.push(it);
            continue;
        }
        if cls.iter().any(|c| c == "labelled")
            || (labelled
                && cls
                    .iter()
                    .any(|c| c.starts_with("seq-") || c.starts_with("div-")))
        {
            pats.out.untextured += 1;
            items.push(it);
            continue;
        }
        let m = it.meta();
        let mut data = indexmap_of(m, sd);
        // a dot plot's dot (the rewrite's dot layouts may draw circles without the class)
        let dot = cls.iter().any(|c| c == "dot")
            || (dot_chart && matches!(it, Primitive::Circle { .. } | Primitive::Use { .. }));
        let shape = match &mut it {
            Primitive::Use { .. } | Primitive::Circle { .. } if dot => {
                // a dot plot's dots, open: a bad one filled
                if sd == "bad" && !cls.iter().any(|c| c == "filled") {
                    it.meta_mut().classes.push("filled".into());
                }
                pats.count("dot");
                items.push(it);
                continue;
            }
            Primitive::Use {
                center, size, href, ..
            } => {
                if href != "mark-dot" || cls.iter().any(|c| c == "change-start") {
                    items.push(it);
                    continue;
                }
                circle(center, *size / 2.)
            }
            Primitive::Circle { center, radius, .. } => circle(center, *radius),
            Primitive::Shape { commands, .. } => Primitive::Shape {
                commands: commands.clone(),
                paint: Default::default(),
                meta: Metadata::default(),
            },
            Primitive::Rect { bounds, meta, .. } => {
                let mut mm = Metadata::default();
                if let Some(rx) = meta.attrs.get("rx") {
                    mm.attrs.insert("rx".into(), rx.clone());
                }
                Primitive::Rect {
                    bounds: bounds.clone(),
                    paint: Default::default(),
                    meta: mm,
                }
            }
            Primitive::Wedge { .. } => {
                let mut w = it.clone();
                *w.meta_mut() = Metadata::default();
                w
            }
            _ => {
                items.push(it);
                continue;
            }
        };
        let fill = fill_of(rules, &cls, ctx)
            .or_else(|| {
                tok(
                    pats.theme,
                    &if safe {
                        format!("--{sd}-cvd")
                    } else {
                        format!("--{sd}")
                    },
                )
            })
            .unwrap_or_default();
        let Some(pid) = pats.pattern(sd, &fill) else {
            items.push(it);
            continue;
        };
        let kind = cls
            .iter()
            .find(|c| AREA_KINDS.contains(&c.as_str()))
            .cloned()
            .unwrap_or_else(|| "mark".into());
        pats.count(&kind);
        let mut shape = shape;
        let paint = chartgen_model::Paint {
            fill: Some(format!("url(#{pid})")),
            stroke: None,
            stroke_width: 0.,
            opacity: 1.,
        };
        match &mut shape {
            Primitive::Rect { paint: p, meta, .. }
            | Primitive::Wedge { paint: p, meta, .. }
            | Primitive::Shape { paint: p, meta, .. } => {
                *p = paint;
                meta.classes = vec!["texture".into()];
                meta.role = "texture".into();
                std::mem::swap(&mut meta.data, &mut data);
            }
            _ => {}
        }
        let mut gmeta = Metadata {
            classes: vec!["textures".into()],
            role: "textures".into(),
            ..Default::default()
        };
        gmeta.attrs.insert("fill".into(), format!("url(#{pid})"));
        items.push(it);
        items.push(Primitive::Group {
            children: vec![shape],
            translate: Point { x: 0., y: 0. },
            clip: None,
            meta: gmeta,
        });
    }
}

fn indexmap_of(m: &Metadata, side: &str) -> chartgen_model::IndexMap<String, String> {
    let mut out = chartgen_model::IndexMap::new();
    for (k, v) in &m.data {
        if k == "series" || k == "x" {
            out.insert(k.clone(), v.clone());
        }
    }
    out.insert("side".into(), side.into());
    out
}

/// annotate.cvd's account of black and white: what told good from bad without colour.
pub fn bw_how(t: &Textured, arrows: bool) -> String {
    let name = |k: &str| match k {
        "bar" | "event-bar" => "bars",
        "segment" => "steps",
        "slice" => "slices",
        "tile" => "tiles",
        "cell" => "cells",
        "legend-swatch" => "the key's swatches",
        "action-dot" => "status dots",
        "area" => "areas",
        _ => "marks",
    };
    let mut area: Vec<&str> = vec![];
    for (k, _) in &t.kinds {
        if k != "dot" && !area.contains(&name(k)) {
            area.push(name(k));
        }
    }
    let mut how = vec![];
    if !area.is_empty() {
        how.push(format!(
            "{} laid over with two tones of their own colour, a hatch for bad and a dither for good, their mean the colour",
            area.join(", ")
        ));
    }
    if t.kinds.iter().any(|k| k.0 == "dot") {
        how.push("a bad dot filled and a good one open".into());
    }
    if arrows {
        how.push("an arrow says rise or fall by its direction, and the good direction makes that good or bad: no texture".into());
    }
    if t.untextured > 0 {
        how.push("cells that carry their values keep a flat fill, and so does their key (a texture would cost the numbers their contrast, G6.5): each value against the dividing value says its side".into());
    }
    if how.is_empty() {
        "nothing else to tell apart".into()
    } else {
        how.join("; ")
    }
}

/// A whole circle as path data (figkit Wedge(x, y, r, 0, 360).path, chartgen-geometry).
fn circle(c: &Point, r: f64) -> Primitive {
    use chartgen_geometry::PathCommand as G;
    use chartgen_model::PathCommand as P;
    let p = |x: f64, y: f64| Point { x, y };
    let commands = chartgen_geometry::Wedge::new(c.x, c.y, r, 0., 360., 0., 0.)
        .and_then(|w| w.path())
        .unwrap_or_default()
        .into_iter()
        .map(|g| match g {
            G::MoveTo { x, y } => P::MoveTo { to: p(x, y) },
            G::LineTo { x, y } => P::LineTo { to: p(x, y) },
            G::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => P::CubicTo {
                c1: p(x1, y1),
                c2: p(x2, y2),
                to: p(x, y),
            },
            G::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                x,
                y,
            } => P::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to: p(x, y),
            },
            G::Close => P::Close,
        })
        .collect();
    Primitive::Shape {
        commands,
        paint: Default::default(),
        meta: Metadata::default(),
    }
}
