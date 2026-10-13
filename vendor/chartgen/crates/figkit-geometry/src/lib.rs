//! Geometry of placed figures: the algorithms of the original figkit `model` (text boxes from
//! anchor and extent, every primitive's padded box, pie wedges with gaps, rings and slivers,
//! curve points and arrowhead classes, the bumps chart's flat curve, box overlap), shared by
//! layout collision checks, scene lowering, lint and renderers instead of private copies.
//!
//! Coordinates are 1/72 in, top-left origin, y down. Angles are degrees clockwise from
//! 12 o'clock, as in the original. Paths are returned as [`PathCommand`] data; renderers format
//! the numbers ([`path_data`] writes them in the original `emit.py` layout with a supplied
//! number formatter). No I/O, no rendering, no layout policy.
use figkit_model::legacy::TextMetrics;
use figkit_model::{Bounds, Point};
use serde::{Deserialize, Serialize};

/// What went wrong, by the original's exception class: an invalid value (`ValueError`), an
/// unknown name (`KeyError`), or a missing item (`IndexError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Value,
    Key,
    Index,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeometryError {
    pub kind: ErrorKind,
    pub message: String,
}
impl GeometryError {
    fn value(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Value,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for GeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for GeometryError {}
impl From<GeometryError> for figkit_model::Error {
    fn from(e: GeometryError) -> Self {
        figkit_model::Error::new("geometry", e.message)
    }
}
pub type Result<T> = std::result::Result<T, GeometryError>;

fn finite(name: &str, values: &[f64]) -> Result<()> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(GeometryError::value(format!(
            "{name}: a coordinate or dimension is not finite"
        )))
    }
}

/// `math.radians` of the original (x · (π / 180)).
pub fn radians(x: f64) -> f64 {
    x * (std::f64::consts::PI / 180.0)
}
/// `math.degrees` of the original (x · (180 / π)).
pub fn degrees(x: f64) -> f64 {
    x * (180.0 / std::f64::consts::PI)
}

/// A box (x0, y0, x1, y1), the original figkit's box tuple.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bbox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}
impl Bbox {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }
    pub fn as_array(&self) -> [f64; 4] {
        [self.x0, self.y0, self.x1, self.y1]
    }
    /// Whether two boxes overlap with positive area (`model.overlap`).
    pub fn overlaps(&self, other: &Bbox) -> bool {
        overlap(self, other)
    }
    pub fn to_bounds(&self) -> Bounds {
        Bounds {
            x: self.x0,
            y: self.y0,
            width: self.x1 - self.x0,
            height: self.y1 - self.y0,
        }
    }
}
impl From<&Bounds> for Bbox {
    fn from(b: &Bounds) -> Self {
        Self::new(b.x, b.y, b.x + b.width, b.y + b.height)
    }
}

/// Whether two boxes (x0, y0, x1, y1) overlap with positive area (figkit `model.overlap`).
pub fn overlap(a: &Bbox, b: &Bbox) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}
/// [`overlap`] of two scene bounds (x, y, width, height): the test the layout and lint crates
/// keep as private copies (`boxes_overlap`, `overlap`).
pub fn bounds_overlap(a: &Bounds, b: &Bounds) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Where a label's anchor point lies on its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    Start,
    Middle,
    End,
}
impl Anchor {
    /// The SVG `text-anchor` name; any other name is an error (the original's KeyError).
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "start" => Ok(Self::Start),
            "middle" => Ok(Self::Middle),
            "end" => Ok(Self::End),
            _ => Err(GeometryError {
                kind: ErrorKind::Key,
                message: format!("'{name}'"),
            }),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Middle => "middle",
            Self::End => "end",
        }
    }
    /// How far left of the anchor point the line starts, for a line of this advance width.
    pub fn offset(self, width: f64) -> f64 {
        match self {
            Self::Start => 0.,
            Self::Middle => width / 2.,
            Self::End => width,
        }
    }
}

/// A typeset line (figkit `measure.Extent`): advance width, ink above and below the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Extent {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
}
impl Extent {
    pub fn height(&self) -> f64 {
        self.ascent + self.descent
    }
    /// The extent of measured metrics whose `baseline` (distance from the top to the baseline)
    /// is known; None without a baseline.
    pub fn from_metrics(m: &TextMetrics) -> Option<Self> {
        m.baseline.map(|b| Self {
            width: m.width,
            ascent: b,
            descent: m.height - b,
        })
    }
}

/// `Text.box(pad)`: the typeset line placed by its baseline `(x, y)` and anchor; the advance
/// width across, the ink height up and down.
pub fn text_box(x: f64, y: f64, anchor: Anchor, extent: &Extent, pad: f64) -> Result<Bbox> {
    finite(
        "text",
        &[x, y, extent.width, extent.ascent, extent.descent, pad],
    )?;
    let w = extent.width;
    let x0 = x - anchor.offset(w);
    Ok(Bbox::new(
        x0 - pad,
        y - extent.ascent - pad,
        x0 + w + pad,
        y + extent.descent + pad,
    ))
}

/// The bounds of a scene text primitive placed by its top-left reference `position` (the
/// rewrite's `Primitive::Text`, y at the top of the measured box) and anchor.
pub fn placed_text_bounds(position: &Point, anchor: &str, metrics: &TextMetrics) -> Result<Bounds> {
    let anchor = Anchor::parse(anchor)?;
    finite(
        "text",
        &[position.x, position.y, metrics.width, metrics.height],
    )?;
    Ok(Bounds {
        x: position.x - anchor.offset(metrics.width),
        y: position.y,
        width: metrics.width,
        height: metrics.height,
    })
}

/// `Rect.box(pad)`.
pub fn rect_box(x: f64, y: f64, w: f64, h: f64, pad: f64) -> Result<Bbox> {
    finite("rect", &[x, y, w, h, pad])?;
    Ok(Bbox::new(x - pad, y - pad, x + w + pad, y + h + pad))
}

/// `Use.box(pad)`: a library mark centred on (x, y) with diameter `size`.
pub fn use_box(x: f64, y: f64, size: f64, pad: f64) -> Result<Bbox> {
    finite("use", &[x, y, size, pad])?;
    let r = size / 2.;
    Ok(Bbox::new(
        x - r - pad,
        y - r - pad,
        x + r + pad,
        y + r + pad,
    ))
}

/// The padded box of points (`Path.box`, `Curve.box`); points must not be empty.
pub fn points_box(points: &[Point], pad: f64) -> Result<Bbox> {
    if points.is_empty() {
        return Err(GeometryError::value("min() iterable argument is empty"));
    }
    finite("points", &[pad])?;
    for p in points {
        finite("points", &[p.x, p.y])?;
    }
    let min = |f: fn(&Point) -> f64| points.iter().map(f).fold(f64::INFINITY, f64::min);
    let max = |f: fn(&Point) -> f64| points.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
    Ok(Bbox::new(
        min(|p| p.x) - pad,
        min(|p| p.y) - pad,
        max(|p| p.x) + pad,
        max(|p| p.y) + pad,
    ))
}

/// One command of SVG path data, with its numbers unformatted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PathCommand {
    MoveTo {
        x: f64,
        y: f64,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    CubicTo {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
    },
    ArcTo {
        rx: f64,
        ry: f64,
        rotation: f64,
        large_arc: bool,
        sweep: bool,
        x: f64,
        y: f64,
    },
    Close,
}

/// SVG path data in the original `emit.py` layout (`M1,2 L3,4 C… A5,5 0 0,1 6,7 Z`), each
/// number written by `n` (emit's `_n` rounds to 0.01); a zero arc rotation is written `0`.
pub fn path_data(commands: &[PathCommand], n: impl Fn(f64) -> String) -> String {
    let p = |x: f64, y: f64| format!("{},{}", n(x), n(y));
    commands
        .iter()
        .map(|c| match *c {
            PathCommand::MoveTo { x, y } => format!("M{}", p(x, y)),
            PathCommand::LineTo { x, y } => format!("L{}", p(x, y)),
            PathCommand::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => format!("C{} {} {}", p(x1, y1), p(x2, y2), p(x, y)),
            PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                x,
                y,
            } => format!(
                "A{},{} {} {},{} {}",
                n(rx),
                n(ry),
                if rotation == 0. {
                    "0".to_string()
                } else {
                    n(rotation)
                },
                u8::from(large_arc),
                u8::from(sweep),
                p(x, y)
            ),
            PathCommand::Close => "Z".into(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A polyline through points (`Path` without a curve): `M` then `L`s; never closed.
pub fn polyline(points: &[Point]) -> Vec<PathCommand> {
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i == 0 {
                PathCommand::MoveTo { x: p.x, y: p.y }
            } else {
                PathCommand::LineTo { x: p.x, y: p.y }
            }
        })
        .collect()
}

/// A bumps chart's curve (`Path` with `curve: "flat"`, R6-40): cubic segments with horizontal
/// tangents at every point, control points half-way across each step, so that the curve stays
/// between its ends and never overshoots.
pub fn flat_curve(points: &[Point]) -> Result<Vec<PathCommand>> {
    let Some(first) = points.first() else {
        return Err(GeometryError {
            kind: ErrorKind::Index,
            message: "list index out of range".into(),
        });
    };
    let (mut x0, mut y0) = (first.x, first.y);
    let mut out = vec![PathCommand::MoveTo { x: x0, y: y0 }];
    for p in &points[1..] {
        let (x1, y1) = (p.x, p.y);
        let h = (x1 - x0) / 2.;
        out.push(PathCommand::CubicTo {
            x1: x0 + h,
            y1: y0,
            x2: x1 - h,
            y2: y1,
            x: x1,
            y: y1,
        });
        (x0, y0) = (x1, y1);
    }
    Ok(out)
}

/// A stroke that is not data (a comment's arrow, the title connector): a smooth shaft of cubic
/// segments from `start`, each (c1, c2, end), and optionally its head's closed outline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub start: Point,
    pub segments: Vec<[Point; 3]>,
    #[serde(default)]
    pub head: Option<Vec<Point>>,
}
impl Curve {
    /// `Curve.points()`: the start, every segment's points, then the head's.
    pub fn points(&self) -> Vec<Point> {
        let mut out = vec![self.start.clone()];
        out.extend(self.segments.iter().flat_map(|s| s.iter().cloned()));
        out.extend(self.head.iter().flatten().cloned());
        out
    }
    pub fn bbox(&self, pad: f64) -> Result<Bbox> {
        points_box(&self.points(), pad)
    }
    /// The shaft: `M` start then one `C` per segment.
    pub fn shaft(&self) -> Vec<PathCommand> {
        let mut out = vec![PathCommand::MoveTo {
            x: self.start.x,
            y: self.start.y,
        }];
        out.extend(self.segments.iter().map(|[a, b, c]| PathCommand::CubicTo {
            x1: a.x,
            y1: a.y,
            x2: b.x,
            y2: b.y,
            x: c.x,
            y: c.y,
        }));
        out
    }
    /// The head's outline, closed; None when there is no head (or it has no points).
    pub fn head_path(&self) -> Option<Vec<PathCommand>> {
        let head = self.head.as_ref().filter(|h| !h.is_empty())?;
        let mut out = polyline(head);
        out.push(PathCommand::Close);
        Some(out)
    }
}

/// `Curve.head_classes`: the shaft's first class with `-head`, then the rest.
pub fn head_classes<S: AsRef<str>>(classes: &[S]) -> Result<Vec<String>> {
    let Some(first) = classes.first() else {
        return Err(GeometryError {
            kind: ErrorKind::Index,
            message: "tuple index out of range".into(),
        });
    };
    let mut out = vec![format!("{}-head", first.as_ref())];
    out.extend(classes[1..].iter().map(|c| c.as_ref().to_string()));
    Ok(out)
}

/// A pie slice: the sector of the circle about (x, y) of radius r from angle a0 to a1 (degrees
/// clockwise from 12 o'clock); with r0 > 0 the sector of a ring. `inset` moves both straight
/// edges inward by that much, parallel to themselves, so that the apex leaves the centre along
/// the bisector, inset / sin(half the angle) out.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Wedge {
    pub x: f64,
    pub y: f64,
    pub r: f64,
    pub a0: f64,
    pub a1: f64,
    #[serde(default)]
    pub r0: f64,
    #[serde(default)]
    pub inset: f64,
}

/// What a wedge draws (`Wedge.shape()`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WedgeShape {
    /// A whole circle of radius r, or a ring when r0 > 0.
    Circle { r: f64, r0: f64 },
    /// A sector: its apex (unused for a ring), the outer arc's angles b0 < b1, and for a ring
    /// the inner arc's angles.
    Sector {
        apex: Point,
        outer: (f64, f64),
        inner: Option<(f64, f64)>,
    },
}

impl Wedge {
    /// A wedge; every number must be finite.
    pub fn new(x: f64, y: f64, r: f64, a0: f64, a1: f64, r0: f64, inset: f64) -> Result<Self> {
        let w = Self {
            x,
            y,
            r,
            a0,
            a1,
            r0,
            inset,
        };
        w.check()?;
        Ok(w)
    }
    fn check(&self) -> Result<()> {
        finite(
            "wedge",
            &[
                self.x, self.y, self.r, self.a0, self.a1, self.r0, self.inset,
            ],
        )
    }
    /// The point at angle `a`, `r` from the centre (the wedge's radius when None).
    pub fn point(&self, a: f64, r: Option<f64>) -> Point {
        let t = radians(a);
        let r = r.unwrap_or(self.r);
        Point {
            x: self.x + r * t.sin(),
            y: self.y - r * t.cos(),
        }
    }
    /// What is drawn: None (a zero part, or a sliver narrower than its gaps), a whole circle or
    /// ring, or a sector.
    pub fn shape(&self) -> Result<Option<WedgeShape>> {
        self.check()?;
        let span = self.a1 - self.a0;
        if span <= 1e-9 {
            return Ok(None);
        }
        if span >= 360. - 1e-9 {
            return Ok(Some(WedgeShape::Circle {
                r: self.r,
                r0: self.r0,
            }));
        }
        let g = self.inset;
        let half = radians(span / 2.);
        let d = if g > 0. { g / half.sin() } else { 0.0 };
        if g >= self.r || d >= self.r {
            return Ok(None);
        }
        let da = if g > 0. {
            degrees((g / self.r).asin())
        } else {
            0.0
        };
        let (b0, b1) = (self.a0 + da, self.a1 - da);
        if b1 <= b0 {
            return Ok(None);
        }
        let inner = if self.r0 > 0. {
            if g >= self.r0 || d >= self.r0 {
                let mid = self.a0 + span / 2.;
                Some((mid, mid))
            } else {
                let dc = if g > 0. {
                    degrees((g / self.r0).asin())
                } else {
                    0.0
                };
                Some((self.a0 + dc, self.a1 - dc))
            }
        } else {
            None
        };
        Ok(Some(WedgeShape::Sector {
            apex: self.point((self.a0 + self.a1) / 2., Some(d)),
            outer: (b0, b1),
            inner,
        }))
    }
    /// The outline as path commands; empty when nothing is drawn.
    pub fn path(&self) -> Result<Vec<PathCommand>> {
        let arc = |r: f64, large: bool, sweep: bool, p: Point| PathCommand::ArcTo {
            rx: r,
            ry: r,
            rotation: 0.,
            large_arc: large,
            sweep,
            x: p.x,
            y: p.y,
        };
        let mv = |p: Point| PathCommand::MoveTo { x: p.x, y: p.y };
        let ln = |p: Point| PathCommand::LineTo { x: p.x, y: p.y };
        Ok(match self.shape()? {
            None => vec![],
            Some(WedgeShape::Circle { r, r0 }) => {
                let (top, bottom) = (self.point(0., None), self.point(180., None));
                let mut d = vec![
                    mv(top.clone()),
                    arc(r, true, true, bottom),
                    arc(r, true, true, top),
                    PathCommand::Close,
                ];
                if r0 > 0. {
                    let (t0, b0) = (self.point(0., Some(r0)), self.point(180., Some(r0)));
                    d.extend([
                        mv(t0.clone()),
                        arc(r0, true, false, b0),
                        arc(r0, true, false, t0),
                        PathCommand::Close,
                    ]);
                }
                d
            }
            Some(WedgeShape::Sector {
                apex,
                outer: (b0, b1),
                inner,
            }) => {
                let large = b1 - b0 > 180.;
                let (out0, out1) = (self.point(b0, None), self.point(b1, None));
                match inner {
                    None => vec![
                        mv(apex),
                        ln(out0),
                        arc(self.r, large, true, out1),
                        PathCommand::Close,
                    ],
                    Some((c0, c1)) => {
                        let (in0, in1) =
                            (self.point(c0, Some(self.r0)), self.point(c1, Some(self.r0)));
                        vec![
                            mv(in0.clone()),
                            ln(out0),
                            arc(self.r, large, true, out1),
                            ln(in1),
                            arc(self.r0, c1 - c0 > 180., false, in0),
                            PathCommand::Close,
                        ]
                    }
                }
            }
        })
    }
    /// The bounding box of what is drawn: the outer arc's ends, the apex (or the inner arc's
    /// ends), and the arc's extreme points at every quarter turn it crosses; a point box at the
    /// centre when nothing is drawn.
    pub fn bbox(&self, pad: f64) -> Result<Bbox> {
        finite("wedge", &[pad])?;
        match self.shape()? {
            None => {
                let c = self.point(self.a0, Some(0.));
                Ok(Bbox::new(c.x - pad, c.y - pad, c.x + pad, c.y + pad))
            }
            Some(WedgeShape::Circle { .. }) => Ok(Bbox::new(
                self.x - self.r - pad,
                self.y - self.r - pad,
                self.x + self.r + pad,
                self.y + self.r + pad,
            )),
            Some(WedgeShape::Sector {
                apex,
                outer: (b0, b1),
                inner,
            }) => {
                let mut pts = vec![self.point(b0, None), self.point(b1, None)];
                match inner {
                    None => pts.push(apex),
                    Some((c0, c1)) => {
                        pts.push(self.point(c0, Some(self.r0)));
                        pts.push(self.point(c1, Some(self.r0)));
                    }
                }
                let mut a = ((b0 / 90.).floor() + 1.) * 90.;
                while a < b1 {
                    pts.push(self.point(a, None));
                    a += 90.;
                }
                points_box(&pts, pad)
            }
        }
    }
}

/// Convert the historical flat-coordinate transport encoding to the canonical model path.
/// New prepared figures store only `figkit_model::PathCommand`.
impl From<&PathCommand> for figkit_model::PathCommand {
    fn from(value: &PathCommand) -> Self {
        match *value {
            PathCommand::MoveTo { x, y } => Self::MoveTo { to: Point { x, y } },
            PathCommand::LineTo { x, y } => Self::LineTo { to: Point { x, y } },
            PathCommand::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => Self::CubicTo {
                c1: Point { x: x1, y: y1 },
                c2: Point { x: x2, y: y2 },
                to: Point { x, y },
            },
            PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                x,
                y,
            } => Self::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to: Point { x, y },
            },
            PathCommand::Close => Self::Close,
        }
    }
}
/// Explicit compatibility encoding retained for version-1 hosts and custom number formatters.
pub mod legacy {
    pub use super::PathCommand;
}
/// Canonical path construction for portable scenes, sharing the existing geometry algorithms.
pub mod portable {
    use super::*;
    pub fn polyline(points: &[Point]) -> Vec<figkit_model::PathCommand> {
        super::polyline(points).iter().map(Into::into).collect()
    }
    pub fn flat_curve(points: &[Point]) -> Result<Vec<figkit_model::PathCommand>> {
        Ok(super::flat_curve(points)?.iter().map(Into::into).collect())
    }
}
