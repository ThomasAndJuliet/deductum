//! Geometric observations without application rules or severity.
//! Bounds are supplied by the caller; overlaps mean positive-area box overlap,
//! not exact intersection of painted paths.
use figkit_model::{Bounds, ElementId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementBounds {
    pub element: ElementId,
    pub bounds: Bounds,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OverlapAllowances {
    /// Explicit pairs of elements permitted to overlap, in either order.
    pub pairs: BTreeSet<(ElementId, ElementId)>,
}
impl OverlapAllowances {
    pub fn allows(&self, a: &ElementId, b: &ElementId) -> bool {
        self.pairs.contains(&(a.clone(), b.clone())) || self.pairs.contains(&(b.clone(), a.clone()))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Observation {
    Overlap {
        first: ElementId,
        second: ElementId,
    },
    OutsideBounds {
        element: ElementId,
    },
    UnsupportedResource {
        element: Option<ElementId>,
        resource: String,
    },
}
/// Inspect explicit boxes in authored order. Tolerance affects outside checks only.
/// Callers bound the box count before invoking this quadratic operation.
pub fn observe_boxes(
    region: &Bounds,
    elements: &[ElementBounds],
    allowances: &OverlapAllowances,
    tolerance: f64,
) -> Vec<Observation> {
    let mut observations = Vec::new();
    for (i, a) in elements.iter().enumerate() {
        let b = &a.bounds;
        if b.x < region.x - tolerance
            || b.y < region.y - tolerance
            || b.x + b.width > region.x + region.width + tolerance
            || b.y + b.height > region.y + region.height + tolerance
        {
            observations.push(Observation::OutsideBounds {
                element: a.element.clone(),
            });
        }
        for b in &elements[i + 1..] {
            if !allowances.allows(&a.element, &b.element)
                && figkit_geometry::bounds_overlap(&a.bounds, &b.bounds)
            {
                observations.push(Observation::Overlap {
                    first: a.element.clone(),
                    second: b.element.clone(),
                });
            }
        }
    }
    observations
}

/// Named limits for optional geometric diagnostics (independent of preparation limits).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintLimits {
    pub max_pairs: usize,
    pub max_observations: usize,
}
impl Default for LintLimits {
    fn default() -> Self {
        Self {
            max_pairs: 1_000_000,
            max_observations: 100_000,
        }
    }
}
/// Bounded variant for untrusted/general-purpose callers. No partial report is returned.
pub fn observe_boxes_checked(
    region: &Bounds,
    elements: &[ElementBounds],
    allowances: &OverlapAllowances,
    tolerance: f64,
    limits: &LintLimits,
) -> figkit_model::FigureResult<Vec<Observation>> {
    use figkit_model::{ErrorKind, FigureError};
    super::bounds(region)?;
    if !tolerance.is_finite() || tolerance < 0. {
        return Err(FigureError::new(
            ErrorKind::InvalidInput,
            "invalid bounds tolerance",
        ));
    }
    let mut ids = BTreeSet::new();
    for e in elements {
        super::bounds(&e.bounds)?;
        if e.element.0.is_empty() || !ids.insert(&e.element) {
            return Err(FigureError::new(
                ErrorKind::InvalidInput,
                "invalid or duplicate lint element ID",
            ));
        }
    }
    let n = elements.len();
    let pairs = if n % 2 == 0 {
        (n / 2).checked_mul(n.saturating_sub(1))
    } else {
        n.checked_mul(n.saturating_sub(1) / 2)
    };
    if pairs.is_none_or(|n| n > limits.max_pairs) {
        return Err(FigureError::new(
            ErrorKind::BudgetExceeded,
            "lint pair budget exceeded",
        ));
    }
    let mut result = Vec::new();
    let mut emit = |observation| {
        if result.len() >= limits.max_observations {
            return Err(FigureError::new(
                ErrorKind::BudgetExceeded,
                "lint observation budget exceeded",
            ));
        }
        result.push(observation);
        Ok(())
    };
    for (i, a) in elements.iter().enumerate() {
        let b = &a.bounds;
        if b.x < region.x - tolerance
            || b.y < region.y - tolerance
            || b.x + b.width > region.x + region.width + tolerance
            || b.y + b.height > region.y + region.height + tolerance
        {
            emit(Observation::OutsideBounds {
                element: a.element.clone(),
            })?;
        }
        for b in &elements[i + 1..] {
            if !allowances.allows(&a.element, &b.element)
                && figkit_geometry::bounds_overlap(&a.bounds, &b.bounds)
            {
                emit(Observation::Overlap {
                    first: a.element.clone(),
                    second: b.element.clone(),
                })?;
            }
        }
    }
    Ok(result)
}
/// Collect translated rectangle, line, circle and measured-text boxes, including supplied
/// symbols. Curves use their control hull; arcs use the full endpoint-derived ellipse envelope.
/// Clipping does not erase authored content outside the figure from diagnostics.
pub fn observe(
    scene: &super::PreparedScene,
    allowances: &OverlapAllowances,
    limits: &LintLimits,
) -> figkit_model::FigureResult<Vec<Observation>> {
    use figkit_model::*;
    fn points_bounds(points: &[Point]) -> Option<Bounds> {
        let first = points.first()?;
        let (mut x, mut y, mut r, mut b) = (first.x, first.y, first.x, first.y);
        for p in &points[1..] {
            x = x.min(p.x);
            y = y.min(p.y);
            r = r.max(p.x);
            b = b.max(p.y);
        }
        Some(Bounds {
            x,
            y,
            width: r - x,
            height: b - y,
        })
    }
    fn path_bounds(commands: &[PathCommand]) -> Option<Bounds> {
        let mut points = Vec::new();
        let mut current = Point { x: 0., y: 0. };
        let mut start = current.clone();
        for command in commands {
            match command {
                PathCommand::MoveTo { to } => {
                    start = to.clone();
                    current = to.clone();
                    points.push(to.clone());
                }
                PathCommand::LineTo { to } => {
                    current = to.clone();
                    points.push(to.clone());
                }
                PathCommand::CubicTo { c1, c2, to } => {
                    points.extend([c1.clone(), c2.clone(), to.clone()]);
                    current = to.clone();
                }
                PathCommand::ArcTo {
                    rx,
                    ry,
                    rotation,
                    large_arc,
                    sweep,
                    to,
                } => {
                    if *rx > 0. && *ry > 0. && current != *to {
                        let phi = rotation.to_radians();
                        let (c, s) = (phi.cos(), phi.sin());
                        let (dx, dy) = ((current.x - to.x) / 2., (current.y - to.y) / 2.);
                        let (x, y) = (c * dx + s * dy, -s * dx + c * dy);
                        let scale = ((x / rx).powi(2) + (y / ry).powi(2)).sqrt().max(1.);
                        let (rx, ry) = (rx * scale, ry * scale);
                        let normalized = (x / rx).powi(2) + (y / ry).powi(2);
                        let coefficient = if normalized == 0. {
                            0.
                        } else {
                            ((1. - normalized).max(0.) / normalized).sqrt()
                                * if large_arc == sweep { -1. } else { 1. }
                        };
                        let (cx, cy) = (coefficient * rx * y / ry, -coefficient * ry * x / rx);
                        let (cx, cy) = (
                            c * cx - s * cy + (current.x + to.x) / 2.,
                            s * cx + c * cy + (current.y + to.y) / 2.,
                        );
                        let (ex, ey) = ((rx * c).hypot(ry * s), (rx * s).hypot(ry * c));
                        points.push(Point {
                            x: cx - ex,
                            y: cy - ey,
                        });
                        points.push(Point {
                            x: cx + ex,
                            y: cy + ey,
                        });
                    }
                    points.push(to.clone());
                    current = to.clone();
                }
                PathCommand::Close => current = start.clone(),
            }
        }
        points_bounds(&points)
    }
    fn collect(
        nodes: &[Primitive],
        offset: Point,
        resources: &Resources,
        out: &mut Vec<ElementBounds>,
    ) -> FigureResult<()> {
        for node in nodes {
            let id = node
                .metadata()
                .element_id
                .clone()
                .expect("prepared identity");
            let b = match node {
                Primitive::Rect { bounds, .. } => Some(bounds.clone()),
                Primitive::Circle { center, radius, .. } => Some(Bounds {
                    x: center.x - radius,
                    y: center.y - radius,
                    width: 2. * radius,
                    height: 2. * radius,
                }),
                Primitive::Line { from, to, .. } => points_bounds(&[from.clone(), to.clone()]),
                Primitive::Path { commands, .. } => path_bounds(commands),
                Primitive::Text { placed } => {
                    let size = &placed.text.metrics.size;
                    Some(Bounds {
                        x: placed.position.x
                            - match placed.anchor {
                                HorizontalAnchor::Start => 0.,
                                HorizontalAnchor::Middle => size.width / 2.,
                                HorizontalAnchor::End => size.width,
                            },
                        y: placed.position.y,
                        width: size.width,
                        height: size.height,
                    })
                }
                Primitive::Group {
                    children,
                    translate,
                    ..
                } => {
                    collect(
                        children,
                        Point {
                            x: offset.x + translate.x,
                            y: offset.y + translate.y,
                        },
                        resources,
                        out,
                    )?;
                    None
                }
                Primitive::Use {
                    symbol, position, ..
                } => {
                    let corners = resources.symbols[symbol]
                        .shapes
                        .iter()
                        .filter_map(|shape| path_bounds(&shape.commands))
                        .flat_map(|b| {
                            [
                                Point {
                                    x: b.x + position.x,
                                    y: b.y + position.y,
                                },
                                Point {
                                    x: b.x + b.width + position.x,
                                    y: b.y + b.height + position.y,
                                },
                            ]
                        })
                        .collect::<Vec<_>>();
                    points_bounds(&corners)
                }
            };
            if let Some(mut bounds) = b {
                bounds.x += offset.x;
                bounds.y += offset.y;
                super::bounds(&bounds)?;
                out.push(ElementBounds {
                    element: id,
                    bounds,
                });
            }
        }
        Ok(())
    }
    let mut elements = Vec::new();
    collect(
        &scene.figure().primitives,
        Point { x: 0., y: 0. },
        &scene.figure().resources,
        &mut elements,
    )?;
    let mut observations = observe_boxes_checked(
        &Bounds {
            x: 0.,
            y: 0.,
            width: scene.figure().size.width,
            height: scene.figure().size.height,
        },
        &elements,
        allowances,
        0.,
        limits,
    )?;
    for (i, _) in scene.compatibility().definitions.iter().enumerate() {
        if observations.len() >= limits.max_observations {
            return Err(super::budget("lint observation budget exceeded"));
        }
        observations.push(Observation::UnsupportedResource {
            element: None,
            resource: format!("raw SVG definition {i} cannot be inspected geometrically"),
        });
    }
    for (id, attributes) in &scene.compatibility().attributes {
        if !attributes.is_empty() {
            if observations.len() >= limits.max_observations {
                return Err(super::budget("lint observation budget exceeded"));
            }
            observations.push(Observation::UnsupportedResource {
                element: Some(id.clone()),
                resource: "raw SVG attributes cannot be inspected geometrically".into(),
            });
        }
    }
    Ok(observations)
}
