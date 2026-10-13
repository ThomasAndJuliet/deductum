//! A laid-out figure in the original's model (texts by baseline, marks with classes and
//! data attributes, groups in drawing order), and a measurement batch that asks the host
//! for every text a layout pass reads. Ported specialist layouts build this model with the
//! original's arithmetic, then lower it to scene primitives.
//!
//! Measurement follows the original's `Batch`: a pass reads extents on demand; a text not
//! yet measured is recorded and given a provisional extent, and the pass is answered with
//! a measurement request instead of a chart, so no provisional extent ever reaches a
//! placed chart.
use super::*;
use serde_json::Value;
use std::cell::RefCell;

/// A typeset line: advance width, ink above and below the baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ext {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
}

pub(crate) type Bx = (f64, f64, f64, f64);

pub(crate) fn overlap(a: Bx, b: Bx) -> bool {
    chartgen_geometry::overlap(
        &chartgen_geometry::Bbox::new(a.0, a.1, a.2, a.3),
        &chartgen_geometry::Bbox::new(b.0, b.1, b.2, b.3),
    )
}

/// The typeset size, weight, slant and ink of a text of these classes: the stylesheet's
/// cascade (`chartgen-style::label_style`, the original bake.label_style), from the theme's
/// flattened stylesheet; a theme without one falls back to its tokens.
pub(crate) fn label_typography(
    role: &str,
    mods: &[&str],
    theme: &Theme,
) -> Result<(f64, TextStyle)> {
    if theme.stylesheet.trim().is_empty() {
        let (size, mut style) = role_style(role, theme);
        if mods.contains(&"text-strong") {
            style.weight = "bold".into();
        }
        return Ok((size, style));
    }
    let mut classes = vec!["prose"];
    classes.extend(mods.iter().copied());
    classes.push(role);
    let l = chartgen_style::label_style(&theme.stylesheet, &classes, &BTreeMap::new())
        .map_err(|e| err(format!("the stylesheet's typography of {role}: {e:?}")))?;
    Ok((l.size, l.text_style()))
}

/// The typeset size, weight, slant and ink of a text role from the theme's tokens (a theme
/// without its flattened stylesheet).
pub(crate) fn role_style(role: &str, theme: &Theme) -> (f64, TextStyle) {
    let m = |n: &str, d: f64| metric_or(theme, n, d);
    let label = m("--label-size", theme.font_size);
    let note = m("--note-size", label * 0.8);
    let size = match role {
        "title" => m("--title-size", label * 1.2),
        "missing-note" | "incomplete-note" | "chart-note" | "footnote" | "event-label"
        | "arrow-key" => note,
        "note-mark" => m("--note-mark-size", note),
        "inset-text" => m("--inset-text-size", note),
        "comment" => m("--comment-size", label),
        _ => label,
    };
    let ink = |n: &str| {
        theme
            .tokens
            .get(n)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let fill = match role {
        "comment" => ink("--comment"),
        "missing-note" | "incomplete-note" | "chart-note" | "footnote" | "event-label"
        | "arrow-key" => ink("--note"),
        "note-mark" => ink("--note-mark-ink").or_else(|| ink("--note")),
        "tile-label" => ink("--tile-text"),
        _ => None,
    }
    .unwrap_or_else(|| theme.text.clone());
    let italic = matches!(
        role,
        "comment" | "missing-note" | "incomplete-note" | "chart-note" | "footnote" | "event-label"
    ) || (role == "note-mark"
        && ink("--note-mark-slant").as_deref() != Some("normal"));
    (
        size,
        TextStyle {
            fill: Some(fill),
            weight: if matches!(role, "title" | "panel-title" | "text-strong") {
                "bold"
            } else {
                "normal"
            }
            .into(),
            font_style: if italic { "italic" } else { "normal" }.into(),
        },
    )
}

pub(crate) fn metric_or(theme: &Theme, name: &str, default: f64) -> f64 {
    match theme.tokens.get(name) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
        Some(Value::String(s)) => s
            .trim()
            .trim_end_matches("pt")
            .parse::<f64>()
            .unwrap_or(default),
        _ => default,
    }
}

/// The measurement batch of one layout pass.
pub(crate) struct Batch<'a> {
    pub theme: &'a Theme,
    pub c: &'a LayoutConstraints,
    pub m: &'a Measurements,
    missing: RefCell<BTreeMap<String, TextRequest>>,
}

impl<'a> Batch<'a> {
    pub fn new(theme: &'a Theme, c: &'a LayoutConstraints, m: &'a Measurements) -> Self {
        Self {
            theme,
            c,
            m,
            missing: RefCell::new(BTreeMap::new()),
        }
    }
    /// A style metric (`--name`) in points; an absent metric is an explicit error.
    pub fn metric(&self, name: &str) -> Result<f64> {
        let v = metric_or(self.theme, name, f64::NAN);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(err(format!("style metric {name} is missing")))
        }
    }
    /// A text's request: its role's style, with the modifiers' (`text-strong`: bold).
    pub fn request(&self, role: &str, content: &TextContent, mods: &[&str]) -> Result<TextRequest> {
        let (size, style) = label_typography(role, mods, self.theme)?;
        guides::request_with_style(role, content.clone(), size, None, style, self.theme, self.c)
    }
    /// The measured extent of a text in a role; provisional (and recorded as wanted) when the
    /// host has not measured it yet.
    pub fn ext_of(
        &self,
        role: &str,
        content: &TextContent,
    ) -> Result<(TextRequest, TextMetrics, Ext)> {
        self.ext_of_mod(role, content, &[])
    }
    #[allow(dead_code)]
    pub fn ext_mod(&self, role: &str, text: &str, mods: &[&str]) -> Result<Ext> {
        Ok(self.ext_of_mod(role, &TextContent::plain(text), mods)?.2)
    }
    pub fn ext_of_mod(
        &self,
        role: &str,
        content: &TextContent,
        mods: &[&str],
    ) -> Result<(TextRequest, TextMetrics, Ext)> {
        let r = self.request(role, content, mods)?;
        if let Some(v) = self.m.entries.get(&r.id) {
            if !v.width.is_finite() || !v.height.is_finite() || v.width < 0. || v.height < 0. {
                return Err(err(format!("invalid metrics for {}", r.id)));
            }
            let ascent = v.baseline.unwrap_or(v.height);
            let e = Ext {
                width: v.width,
                ascent,
                descent: (v.height - ascent).max(0.),
            };
            return Ok((r, v.clone(), e));
        }
        let size = r.font_size;
        let chars = match content {
            TextContent::Plain { text } => text.chars().count(),
            TextContent::Native {
                alternative, key, ..
            } => alternative.as_deref().unwrap_or(key).chars().count(),
        } as f64;
        let e = Ext {
            width: 0.5 * size * chars,
            ascent: 0.7 * size,
            descent: 0.2 * size,
        };
        let metrics = TextMetrics {
            width: e.width,
            height: e.ascent + e.descent,
            baseline: Some(e.ascent),
        };
        self.missing.borrow_mut().insert(r.id.clone(), r.clone());
        Ok((r, metrics, e))
    }
    pub fn ext(&self, role: &str, text: &str) -> Result<Ext> {
        Ok(self.ext_of(role, &TextContent::plain(text))?.2)
    }
    pub fn width(&self, role: &str, text: &str) -> Result<f64> {
        Ok(self.ext(role, text)?.width)
    }
    pub fn need(&self, role: &str, text: &str) -> Result<()> {
        self.ext(role, text).map(|_| ())
    }
    pub fn need_content(&self, role: &str, content: &TextContent) -> Result<()> {
        self.ext_of(role, content).map(|_| ())
    }
    /// A text placed by its baseline.
    pub fn text(
        &self,
        role: &str,
        content: TextContent,
        x: f64,
        y: f64,
        anchor: &str,
    ) -> Result<Txt> {
        self.text_mod(role, content, x, y, anchor, &[])
    }
    /// A text with modifiers (classes before its role, `text-strong` bold).
    pub fn text_mod(
        &self,
        role: &str,
        content: TextContent,
        x: f64,
        y: f64,
        anchor: &str,
        mods: &[&str],
    ) -> Result<Txt> {
        let (request, metrics, ext) = self.ext_of_mod(role, &content, mods)?;
        Ok(Txt {
            role: role.into(),
            plain: match &content {
                TextContent::Plain { text } => text.clone(),
                TextContent::Native {
                    alternative, key, ..
                } => alternative.clone().unwrap_or_else(|| key.clone()),
            },
            x,
            y,
            anchor: anchor.into(),
            ext,
            request,
            metrics,
            modifiers: mods.iter().map(|m| (*m).to_owned()).collect(),
            joint: None,
        })
    }
    pub fn plain(&self, role: &str, text: &str, x: f64, y: f64, anchor: &str) -> Result<Txt> {
        self.text(role, TextContent::plain(text), x, y, anchor)
    }
    pub fn wanted(&self) -> Vec<TextRequest> {
        self.missing.borrow().values().cloned().collect()
    }
}

/// Run a layout pass; answer with a measurement request while any text it read is unmeasured.
pub(crate) fn run_pass(
    c: &LayoutConstraints,
    m: &Measurements,
    batch: &Batch,
    pass: Result<LayoutStep>,
) -> Result<LayoutStep> {
    let wanted = batch.wanted();
    if !wanted.is_empty() {
        if !m.entries.is_empty() && m.context != c.measurement_context {
            return Err(err("measurement context does not match layout context"));
        }
        if c.max_rounds == 0 {
            return Err(err("measurement round limit is zero"));
        }
        return Ok(LayoutStep::NeedMeasurements {
            requests: wanted,
            round: 1,
        });
    }
    pass
}

#[derive(Debug, Clone)]
pub(crate) struct Txt {
    pub role: String,
    pub plain: String,
    pub x: f64,
    /// The baseline.
    pub y: f64,
    pub anchor: String,
    pub ext: Ext,
    pub request: TextRequest,
    pub metrics: TextMetrics,
    pub modifiers: Vec<String>,
    pub joint: Option<String>,
}

impl Txt {
    pub fn bbox(&self, pad: f64) -> Bx {
        let w = self.ext.width;
        let x0 = self.x
            - match self.anchor.as_str() {
                "middle" => w / 2.,
                "end" => w,
                _ => 0.,
            };
        (
            x0 - pad,
            self.y - self.ext.ascent - pad,
            x0 + w + pad,
            self.y + self.ext.descent + pad,
        )
    }
}

pub(crate) type Data = Vec<(String, String)>;

#[derive(Debug, Clone)]
#[allow(dead_code)] // circles serve the dot marks of later ports
pub(crate) enum Item {
    Text(Txt),
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        classes: Vec<String>,
        data: Data,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        classes: Vec<String>,
        data: Data,
    },
    Path {
        points: Vec<(f64, f64)>,
        closed: bool,
        /// Drawn as cubic curves flat at every point (a bumps chart's lines, R6-40).
        flat: bool,
        classes: Vec<String>,
        data: Data,
    },
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
        classes: Vec<String>,
        data: Data,
    },
    Group {
        classes: Vec<String>,
        id: Option<String>,
        data: Data,
        children: Vec<Item>,
    },
    /// A stroke that is not data (a comment's arrow, the title connector): cubic segments from
    /// `start`, each (c1, c2, end), and the head's outline, filled in the shaft's colour.
    Curve {
        start: (f64, f64),
        segments: Vec<((f64, f64), (f64, f64), (f64, f64))>,
        classes: Vec<String>,
        head: Vec<(f64, f64)>,
    },
}

pub(crate) fn cls(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

pub(crate) fn attrs(pairs: &[(&str, String)]) -> Data {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

impl Item {
    pub fn group(classes: &str, children: Vec<Item>) -> Item {
        Item::Group {
            classes: cls(classes),
            id: None,
            data: vec![],
            children,
        }
    }
    pub fn line(x1: f64, y1: f64, x2: f64, y2: f64, classes: &str, d: Data) -> Item {
        Item::Line {
            x1,
            y1,
            x2,
            y2,
            classes: cls(classes),
            data: d,
        }
    }
    pub fn rect(x: f64, y: f64, w: f64, h: f64, classes: &str, d: Data) -> Item {
        Item::Rect {
            x,
            y,
            w,
            h,
            classes: cls(classes),
            data: d,
        }
    }
    pub fn path(points: Vec<(f64, f64)>, classes: &str, d: Data) -> Item {
        Item::Path {
            points,
            closed: false,
            flat: false,
            classes: cls(classes),
            data: d,
        }
    }
    pub fn flat_path(points: Vec<(f64, f64)>, classes: &str, d: Data) -> Item {
        Item::Path {
            points,
            closed: false,
            flat: true,
            classes: cls(classes),
            data: d,
        }
    }
    pub fn bbox(&self) -> Option<Bx> {
        match self {
            Item::Text(t) => Some(t.bbox(0.)),
            Item::Rect { x, y, w, h, .. } => Some((*x, *y, x + w, y + h)),
            Item::Line { x1, y1, x2, y2, .. } => {
                Some((x1.min(*x2), y1.min(*y2), x1.max(*x2), y1.max(*y2)))
            }
            Item::Path { points, .. } if !points.is_empty() => Some(points.iter().fold(
                (
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ),
                |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)),
            )),
            Item::Circle { cx, cy, r, .. } => Some((cx - r, cy - r, cx + r, cy + r)),
            Item::Curve {
                start,
                segments,
                head,
                ..
            } => {
                let mut pts = vec![*start];
                for (a, b, c) in segments {
                    pts.extend([*a, *b, *c]);
                }
                pts.extend(head.iter().copied());
                Some(pts.iter().fold(
                    (
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ),
                    |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)),
                ))
            }
            _ => None,
        }
    }
    /// Translate in place (every descendant of a group).
    pub fn shift(&mut self, dx: f64, dy: f64) {
        match self {
            Item::Text(t) => {
                t.x += dx;
                t.y += dy;
            }
            Item::Rect { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Item::Line { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
            Item::Path { points, .. } => {
                for p in points {
                    p.0 += dx;
                    p.1 += dy;
                }
            }
            Item::Circle { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            Item::Group { children, .. } => {
                for c in children {
                    c.shift(dx, dy);
                }
            }
            Item::Curve {
                start,
                segments,
                head,
                ..
            } => {
                let mv = |p: &mut (f64, f64)| {
                    p.0 += dx;
                    p.1 += dy;
                };
                mv(start);
                for (a, b, c) in segments {
                    mv(a);
                    mv(b);
                    mv(c);
                }
                head.iter_mut().for_each(mv);
            }
        }
    }
    #[allow(dead_code)]
    pub fn walk<'s>(&'s self, out: &mut Vec<&'s Item>) {
        out.push(self);
        if let Item::Group { children, .. } = self {
            for c in children {
                c.walk(out);
            }
        }
    }
    #[allow(dead_code)]
    pub fn texts(&self) -> Vec<&Txt> {
        let mut all = vec![];
        self.walk(&mut all);
        all.into_iter()
            .filter_map(|i| match i {
                Item::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }
}

/// The ink of a mark's colour slot, from its classes (`single`, `series-N`, roles), as the
/// original stylesheet resolves it; `line` asks for the line variant (`--single-line`).
pub(crate) fn slot_ink(classes: &[String], theme: &Theme, line: bool) -> Option<String> {
    let tok = |n: &str| {
        theme
            .tokens
            .get(n)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let strong = classes.iter().any(|c| c == "highlight");
    let context = classes.iter().any(|c| c == "context");
    for c in classes {
        let base = match c.as_str() {
            "single" if line => "--single-line".to_owned(),
            "single" => "--single".to_owned(),
            s if s.starts_with("series-") && s[7..].parse::<u32>().is_ok() => format!("--{s}"),
            "average" | "total" | "all-others" | "missing" | "good" | "bad" => format!("--{c}"),
            "reference" => "--reference-line".to_owned(),
            _ => continue,
        };
        let name = if strong {
            format!("{base}-strong")
        } else if context {
            format!("{base}-context")
        } else {
            base.clone()
        };
        return tok(&name).or_else(|| tok(&base));
    }
    None
}

fn has(classes: &[String], c: &str) -> bool {
    classes.iter().any(|x| x == c)
}

/// The paint of a mark, from its classes (the original stylesheet's rules for the marks the
/// ported layouts draw). An unknown mark class is an explicit error, never a guessed paint.
pub(crate) fn paint_of(classes: &[String], theme: &Theme) -> Result<Paint> {
    let tok = |n: &str| {
        theme
            .tokens
            .get(n)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let hair = metric_or(theme, "--grid-width", 0.6);
    let line_w = metric_or(theme, "--line-width", 2.);
    let stroke = |ink: Option<String>, w: f64| Paint {
        fill: None,
        stroke: ink,
        stroke_width: w,
        opacity: 1.,
    };
    let fill = |ink: Option<String>| Paint {
        fill: ink,
        stroke: None,
        stroke_width: 0.,
        opacity: 1.,
    };
    let first = classes.first().map(String::as_str).unwrap_or("");
    Ok(match first {
        "gridline" => {
            let ink = if has(classes, "zero") && has(classes, "signed") {
                tok("--zero-line-signed")
            } else if has(classes, "zero") {
                tok("--zero-line")
            } else {
                tok("--grid")
            };
            stroke(ink.or_else(|| Some(theme.grid.clone())), hair)
        }
        "time-divider" | "panel-divider" | "time-tick" => {
            stroke(tok("--divider").or_else(|| Some(theme.grid.clone())), hair)
        }
        "leader" => stroke(tok("--leader"), hair),
        "event-line" => stroke(tok("--average"), hair),
        "connector" => stroke(tok("--connector"), hair),
        "step-line" | "line" | "projection" | "change-shaft" | "bump-line" => stroke(
            slot_ink(classes, theme, true),
            if has(classes, "highlight") {
                metric_or(theme, "--line-width-highlight", 4.)
            } else {
                line_w
            },
        ),
        "event-bar" | "bar" | "segment" | "funnel-bar" | "change-head" | "change-start"
        | "point" | "dot" | "legend-swatch" | "wrapped-bar" => {
            fill(slot_ink(classes, theme, false))
        }
        "arrow-halo" => Paint {
            fill: tok("--page"),
            stroke: tok("--page"),
            stroke_width: metric_or(theme, "--arrow-head-halo", 1.2),
            opacity: 1.,
        },
        "comment-arrow" => stroke(
            tok("--comment"),
            metric_or(theme, "--comment-arrow-width", 0.8),
        ),
        "comment-arrow-head" => fill(tok("--comment")),
        "title-link-head" => fill(tok("--link")),
        "title-link" | "link" => stroke(tok("--link"), metric_or(theme, "--link-width", 0.8)),
        other => {
            return Err(err(format!("no paint rule for mark class {other}")));
        }
    })
    .and_then(|p| {
        if p.fill.is_none() && p.stroke.is_none() {
            Err(err(format!(
                "mark class {first} has no colour in this style"
            )))
        } else {
            Ok(p)
        }
    })
}

fn metadata(classes: &[String], data: &Data) -> Metadata {
    Metadata {
        id: None,
        role: classes.first().cloned().unwrap_or_default(),
        classes: classes.to_vec(),
        data: data.iter().cloned().collect(),
        ..Default::default()
    }
}

/// Points along a curve's cubic segments (n each) and round its head's outline (the
/// original's `_curve_points`).
pub(crate) fn curve_points(item: &Item, n: usize) -> Vec<(f64, f64)> {
    let Item::Curve {
        start,
        segments,
        head,
        ..
    } = item
    else {
        return vec![];
    };
    let mut pts = vec![];
    let mut p0 = *start;
    for (c1, c2, e) in segments {
        for j in 0..=n {
            let t = j as f64 / n as f64;
            let (a, b, c, d) = (
                (1. - t).powi(3),
                3. * (1. - t).powi(2) * t,
                3. * (1. - t) * t * t,
                t.powi(3),
            );
            pts.push((
                a * p0.0 + b * c1.0 + c * c2.0 + d * e.0,
                a * p0.1 + b * c1.1 + c * c2.1 + d * e.1,
            ));
        }
        p0 = *e;
    }
    for (i, p) in head.iter().enumerate() {
        let q = head[(i + 1) % head.len()];
        for j in 0..5 {
            let f = j as f64 / 4.;
            pts.push((p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f));
        }
    }
    pts
}

/// Lower the figure to scene primitives (groups keep their classes; paints from classes).
pub(crate) fn lower(item: &Item, theme: &Theme) -> Result<Primitive> {
    Ok(match item {
        Item::Curve {
            start,
            segments,
            classes,
            head,
        } => Primitive::Curve {
            start: Point {
                x: start.0,
                y: start.1,
            },
            segments: segments
                .iter()
                .map(|(a, b, c)| CubicSegment {
                    c1: Point { x: a.0, y: a.1 },
                    c2: Point { x: b.0, y: b.1 },
                    end: Point { x: c.0, y: c.1 },
                })
                .collect(),
            head: (!head.is_empty())
                .then(|| head.iter().map(|p| Point { x: p.0, y: p.1 }).collect()),
            paint: paint_of(classes, theme)?,
            meta: metadata(classes, &vec![]),
        },
        Item::Text(t) => {
            let mut classes = t.modifiers.clone();
            classes.push(t.role.clone());
            Primitive::Text {
                position: Point {
                    x: t.x,
                    y: t.y - t.ext.ascent,
                },
                request: t.request.clone(),
                metrics: t.metrics.clone(),
                anchor: t.anchor.clone(),
                meta: Metadata {
                    id: None,
                    role: t.role.clone(),
                    classes,
                    joint: t.joint.clone(),
                    ..Default::default()
                },
            }
        }
        Item::Rect {
            x,
            y,
            w,
            h,
            classes,
            data,
        } => Primitive::Rect {
            bounds: Bounds {
                x: *x,
                y: *y,
                width: *w,
                height: *h,
            },
            paint: paint_of(classes, theme)?,
            meta: metadata(classes, data),
        },
        Item::Line {
            x1,
            y1,
            x2,
            y2,
            classes,
            data,
        } => Primitive::Line {
            from: Point { x: *x1, y: *y1 },
            to: Point { x: *x2, y: *y2 },
            paint: paint_of(classes, theme)?,
            meta: metadata(classes, data),
        },
        Item::Path {
            points,
            closed,
            flat,
            classes,
            data,
        } if *flat => {
            // a bumps chart's curve, flat at every point (R6-40): cubic path commands
            let pts: Vec<Point> = points.iter().map(|p| Point { x: p.0, y: p.1 }).collect();
            let commands = chartgen_geometry::flat_curve(&pts)
                .map_err(|e| err(format!("a flat curve without points: {e:?}")))?
                .into_iter()
                .filter_map(|c| match c {
                    chartgen_geometry::PathCommand::MoveTo { x, y } => {
                        Some(PathCommand::MoveTo { to: Point { x, y } })
                    }
                    chartgen_geometry::PathCommand::CubicTo {
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    } => Some(PathCommand::CubicTo {
                        c1: Point { x: x1, y: y1 },
                        c2: Point { x: x2, y: y2 },
                        to: Point { x, y },
                    }),
                    _ => None,
                })
                .collect();
            let _ = closed;
            Primitive::Shape {
                commands,
                paint: paint_of(classes, theme)?,
                meta: metadata(classes, data),
            }
        }
        Item::Path {
            points,
            closed,
            classes,
            data,
            ..
        } => Primitive::Path {
            points: points.iter().map(|p| Point { x: p.0, y: p.1 }).collect(),
            closed: *closed,
            paint: paint_of(classes, theme)?,
            meta: metadata(classes, data),
        },
        Item::Circle {
            cx,
            cy,
            r,
            classes,
            data,
        } => Primitive::Use {
            // a mark of the style's library (the original's <use href="#mark-dot">)
            center: Point { x: *cx, y: *cy },
            size: 2. * r,
            href: "mark-dot".into(),
            paint: paint_of(classes, theme)?,
            meta: metadata(classes, data),
        },
        Item::Group {
            classes,
            id,
            data,
            children,
        } => Primitive::Group {
            children: children
                .iter()
                .map(|c| lower(c, theme))
                .collect::<Result<_>>()?,
            translate: Point { x: 0., y: 0. },
            clip: None,
            meta: Metadata {
                id: id.clone(),
                role: String::new(),
                classes: classes.clone(),
                data: data.iter().cloned().collect(),
                ..Default::default()
            },
        },
    })
}

// ---- line breaking (common.splits, best_lines) -----------------------------------------------

/// Two-line splits where the original would break: at a space only; never after an
/// abbreviation (a word ending in "."), never just before "&", nor leaving "&" alone.
pub(crate) fn splits(plain: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let bytes: Vec<char> = plain.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == ' ' {
            let start = i;
            while i < bytes.len() && bytes[i] == ' ' {
                i += 1;
            }
            let a: String = bytes[..start].iter().collect();
            let b: String = bytes[i..].iter().collect();
            if a.is_empty() || b.is_empty() || a.ends_with('.') {
                continue;
            }
            if b.split(' ').next() == Some("&") || a == "&" {
                continue;
            }
            out.push((a, b));
        } else {
            i += 1;
        }
    }
    out
}

/// The whole string and every two-line split.
pub(crate) fn line_options(plain: &str) -> Vec<Vec<String>> {
    let mut out = vec![vec![plain.to_owned()]];
    out.extend(splits(plain).into_iter().map(|(a, b)| vec![a, b]));
    out
}

/// One line if it fits `limit` (or no limit), else the two-line split minimising the wider
/// line among those that fit; None if nothing fits.
pub(crate) fn best_lines(
    plain: &str,
    width: &dyn Fn(&str) -> Result<f64>,
    limit: Option<f64>,
    max_lines: usize,
) -> Result<Option<Vec<String>>> {
    let whole = width(plain)?;
    if limit.is_none_or(|l| whole <= l) {
        return Ok(Some(vec![plain.to_owned()]));
    }
    if max_lines < 2 {
        return Ok(None);
    }
    let limit = limit.unwrap();
    let mut best: Option<(f64, Vec<String>)> = None;
    for (a, b) in splits(plain) {
        let w = width(&a)?.max(width(&b)?);
        if w <= limit && best.as_ref().is_none_or(|(bw, _)| w < *bw) {
            best = Some((w, vec![a, b]));
        }
    }
    Ok(best.map(|b| b.1))
}

// ---- the title block (common.title_block) ------------------------------------------------------

pub(crate) struct TitleBlock {
    pub texts: Vec<Txt>,
    pub last_baseline: Option<f64>,
    /// The block's decisions (R5-26: a line wrapped onto two).
    pub decisions: Vec<Value>,
}

/// A report decision as the original records it: `{rule, level, text, guide}`, the level the
/// rule's unless given (no guide ids: the original's GUIDE.md is not part of the target).
pub(crate) fn decision(rule: &str, text: impl Into<String>, level: Option<&str>) -> Value {
    serde_json::json!({
        "rule": rule,
        "level": level.unwrap_or(chartgen_model::rule_level(rule)),
        "text": text.into(),
        "guide": [],
    })
}

/// A number as Python's `{:g}` writes it (six significant digits, no trailing zeros).
pub(crate) fn fmt_g(v: f64) -> String {
    if v == 0. {
        return "0".into();
    }
    let e = v.abs().log10().floor() as i32;
    if !(-5..6).contains(&e) {
        let s = format!("{:.5e}", v);
        let (m, x) = s.split_once('e').unwrap();
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let x: i32 = x.parse().unwrap();
        return format!("{m}e{}{:02}", if x < 0 { "-" } else { "+" }, x.abs());
    }
    let d = (5 - e).max(0) as usize;
    let s = format!("{:.*}", d, v);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

fn title_parts(plan: &SemanticPlan) -> Result<(Option<TextContent>, Option<TextContent>)> {
    let title = &plan.spec.raw["title"];
    let text = if title.is_object() {
        guides::content(&title["text"])?
    } else {
        guides::content(title)?
    };
    let descriptive = if title.is_object() {
        guides::content(&title["descriptive"])?
    } else {
        None
    };
    Ok((text, descriptive))
}

/// Every string the title block might set.
pub(crate) fn need_title(plan: &SemanticPlan, batch: &Batch) -> Result<()> {
    let (text, descriptive) = title_parts(plan)?;
    if let Some(t) = text {
        match &t {
            TextContent::Plain { text } => {
                for opt in line_options(text) {
                    for l in opt {
                        batch.need("title", &l)?;
                    }
                }
            }
            other => batch.need_content("title", other)?,
        }
    }
    if let Some(d) = descriptive {
        batch.need_content("subtitle", &d)?;
    }
    Ok(())
}

/// The title lines and the descriptive line (D-5 (a)), from y = 0 down; CG-1 findings when a
/// line cannot fit.
pub(crate) fn title_block(
    plan: &SemanticPlan,
    batch: &Batch,
    width: f64,
    findings: &mut Vec<Finding>,
) -> Result<TitleBlock> {
    let mut block = TitleBlock {
        texts: vec![],
        last_baseline: None,
        decisions: vec![],
    };
    let (text, descriptive) = title_parts(plan)?;
    let Some(t) = text else {
        return Ok(block);
    };
    let em = batch.metric("--label-size")?;
    let tsize = batch.metric("--title-size")?;
    let pitch = batch.metric("--line-pitch")?;
    let pitch_t = pitch * tsize / em;
    let contents: Vec<TextContent> = match &t {
        TextContent::Plain { text } => {
            let w = |s: &str| batch.width("title", s);
            let lines = match best_lines(text, &w, Some(width), 2)? {
                Some(l) => l,
                None => {
                    findings.push(
                        Finding::new(
                            "CG-1",
                            "error",
                            "the title does not fit the width on two lines; shorten it",
                        )
                        .at("title.text"),
                    );
                    vec![text.clone()]
                }
            };
            lines.into_iter().map(TextContent::plain).collect()
        }
        other => vec![other.clone()],
    };
    let mut y: Option<f64> = None;
    for c in &contents {
        let tx = batch.text("title", c.clone(), 0., 0., "start")?;
        let yy = match y {
            None => tx.ext.ascent,
            Some(v) => v + pitch_t,
        };
        y = Some(yy);
        block.texts.push(Txt { y: yy, ..tx });
    }
    if contents.len() > 1 {
        block
            .decisions
            .push(decision("R5-26", "the title wraps onto two lines", None));
    }
    if contents.len() == 1 && !matches!(t, TextContent::Plain { .. }) {
        let w = batch.ext_of("title", &contents[0])?.2.width;
        if w > width {
            findings.push(
                Finding::new(
                    "CG-1",
                    "error",
                    format!(
                        "the title (Typst markup, which is not wrapped) is wider than the figure's {width:.0} pt; shorten it, or give it as plain text to wrap"
                    ),
                )
                .at("title.text"),
            );
        }
    }
    if let Some(d) = descriptive {
        let dcontents: Vec<TextContent> = match &d {
            TextContent::Plain { text } => {
                let w = |s: &str| batch.width("subtitle", s);
                match best_lines(text, &w, Some(width), 2)? {
                    Some(l) => l.into_iter().map(TextContent::plain).collect(),
                    None => {
                        findings.push(
                            Finding::new(
                                "CG-1",
                                "error",
                                format!(
                                    "the descriptive line does not fit the figure's {width:.0} pt on two lines; shorten it"
                                ),
                            )
                            .at("title.descriptive"),
                        );
                        vec![d.clone()]
                    }
                }
            }
            other => vec![other.clone()],
        };
        if dcontents.len() > 1 {
            block.decisions.push(decision(
                "R5-26",
                "the descriptive line wraps onto two lines",
                None,
            ));
        }
        for (i, c) in dcontents.iter().enumerate() {
            let tx = batch.text("subtitle", c.clone(), 0., 0., "start")?;
            let yy = y.unwrap_or(0.) + if i == 0 { pitch_t } else { pitch };
            y = Some(yy);
            block.texts.push(Txt { y: yy, ..tx });
        }
    }
    block.last_baseline = y;
    Ok(block)
}

/// Whether the segment p–q, thickened by pad, meets the box (Liang–Barsky clipping).
pub(crate) fn segment_meets_box(p: (f64, f64), q: (f64, f64), b: Bx, pad: f64) -> bool {
    let (x0, y0, x1, y1) = (b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad);
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (pk, qk) in [
        (-dx, p.0 - x0),
        (dx, x1 - p.0),
        (-dy, p.1 - y0),
        (dy, y1 - p.1),
    ] {
        if pk == 0. {
            if qk < 0. {
                return false;
            }
        } else {
            let t = qk / pk;
            if pk < 0. {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}
