//! Self-contained geometry for primitives that refer to shared definitions, for the renderers
//! that have no house stylesheet or mark library (portable SVG, PGF, Typst layers).
//!
//! - `Use`: the plain style's mark library (the original's charts/style/plain-defs.svg). Each
//!   symbol is drawn in a -1…1 box scaled to the mark's size and inherits the mark's fill and
//!   stroke, as `<use>` content does; the stroked symbols (x, plus, asterisk) set their own
//!   unfilled stroke width in symbol units, as their paths do.
//! - `Wedge`: the sector's outline from chartgen-geometry (a wedge with nothing to draw draws
//!   nothing, as the original's emit writes nothing).
use crate::legacy::err;
use figkit_model::legacy::{Bounds, Metadata, Paint, PathCommand, Point, Primitive, Result};

fn shape(points: &[(f64, f64)], closed: bool, cx: f64, cy: f64, r: f64) -> Vec<PathCommand> {
    let p = |(x, y): (f64, f64)| Point {
        x: cx + x * r,
        y: cy + y * r,
    };
    let mut out = vec![PathCommand::MoveTo { to: p(points[0]) }];
    out.extend(
        points[1..]
            .iter()
            .map(|q| PathCommand::LineTo { to: p(*q) }),
    );
    if closed {
        out.push(PathCommand::Close);
    }
    out
}

/// Separate strokes ("M a L b M c L d"), each a pair of points.
fn strokes(pairs: &[((f64, f64), (f64, f64))], cx: f64, cy: f64, r: f64) -> Vec<PathCommand> {
    pairs
        .iter()
        .flat_map(|(a, b)| shape(&[*a, *b], false, cx, cy, r))
        .collect()
}

/// The library symbol `href` drawn at `center` with diameter `size` in `paint`.
pub fn symbol(
    href: &str,
    center: &Point,
    size: f64,
    paint: &Paint,
    meta: &Metadata,
) -> Result<Primitive> {
    let (cx, cy, r) = (center.x, center.y, size / 2.);
    let filled = |commands: Vec<PathCommand>| Primitive::Shape {
        commands,
        paint: paint.clone(),
        meta: meta.clone(),
    };
    let stroked = |commands: Vec<PathCommand>, width: f64| -> Result<Primitive> {
        let ink = paint
            .stroke
            .clone()
            .or_else(|| paint.fill.clone())
            .ok_or_else(|| err(format!("symbol {href} needs an ink")))?;
        Ok(Primitive::Shape {
            commands,
            paint: Paint {
                fill: None,
                stroke: Some(ink),
                stroke_width: width * r,
                opacity: paint.opacity,
            },
            meta: meta.clone(),
        })
    };
    Ok(match href {
        "mark-dot" => Primitive::Circle {
            center: center.clone(),
            radius: r,
            paint: paint.clone(),
            meta: meta.clone(),
        },
        "mark-square" => Primitive::Rect {
            bounds: Bounds {
                x: cx - 0.886 * r,
                y: cy - 0.886 * r,
                width: 1.772 * r,
                height: 1.772 * r,
            },
            paint: paint.clone(),
            meta: meta.clone(),
        },
        "mark-triangle" => filled(shape(&[(0., -1.1), (1., 0.7), (-1., 0.7)], true, cx, cy, r)),
        "mark-x" => stroked(
            strokes(
                &[((-0.8, -0.8), (0.8, 0.8)), ((-0.8, 0.8), (0.8, -0.8))],
                cx,
                cy,
                r,
            ),
            0.35,
        )?,
        "mark-plus" => stroked(
            strokes(&[((0., -1.), (0., 1.)), ((-1., 0.), (1., 0.))], cx, cy, r),
            0.35,
        )?,
        "mark-asterisk" => stroked(
            strokes(
                &[
                    ((0., -1.), (0., 1.)),
                    ((-0.87, -0.5), (0.87, 0.5)),
                    ((-0.87, 0.5), (0.87, -0.5)),
                ],
                cx,
                cy,
                r,
            ),
            0.3,
        )?,
        "mark-icon-down" => filled(shape(
            &[
                (-0.75, -1.),
                (0.75, -1.),
                (0.75, 0.15),
                (0., 1.),
                (-0.75, 0.15),
            ],
            true,
            cx,
            cy,
            r,
        )),
        "mark-icon-up" => filled(shape(
            &[
                (-0.75, 1.),
                (0.75, 1.),
                (0.75, -0.15),
                (0., -1.),
                (-0.75, -0.15),
            ],
            true,
            cx,
            cy,
            r,
        )),
        "mark-icon-left" => filled(shape(
            &[
                (1., -0.75),
                (1., 0.75),
                (-0.15, 0.75),
                (-1., 0.),
                (-0.15, -0.75),
            ],
            true,
            cx,
            cy,
            r,
        )),
        "mark-icon-right" => filled(shape(
            &[
                (-1., -0.75),
                (-1., 0.75),
                (0.15, 0.75),
                (1., 0.),
                (0.15, -0.75),
            ],
            true,
            cx,
            cy,
            r,
        )),
        _ => return Err(err(format!("unknown house mark symbol: {href}"))),
    })
}

/// A wedge's outline as a shape, or None when nothing is drawn.
pub fn wedge(p: &Primitive) -> Result<Option<Primitive>> {
    let Primitive::Wedge {
        center,
        radius,
        inner_radius,
        start_angle,
        end_angle,
        inset,
        paint,
        meta,
    } = p
    else {
        return Err(err("not a wedge"));
    };
    let commands = figkit_geometry::Wedge::new(
        center.x,
        center.y,
        *radius,
        *start_angle,
        *end_angle,
        *inner_radius,
        *inset,
    )
    .and_then(|w| w.path())
    .map_err(|e| err(e.message))?;
    if commands.is_empty() {
        return Ok(None);
    }
    Ok(Some(Primitive::Shape {
        commands: commands.iter().map(PathCommand::from).collect(),
        paint: paint.clone(),
        meta: meta.clone(),
    }))
}

/// The self-contained stand-in for a primitive the portable renderers cannot draw directly:
/// Some(primitives to draw instead), or None when it draws itself.
pub fn lower(p: &Primitive) -> Result<Option<Vec<Primitive>>> {
    Ok(match p {
        Primitive::Use {
            center,
            size,
            href,
            paint,
            meta,
        } => Some(vec![symbol(href, center, *size, paint, meta)?]),
        Primitive::Wedge { .. } => Some(wedge(p)?.into_iter().collect()),
        _ => None,
    })
}
