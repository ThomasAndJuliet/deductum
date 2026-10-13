//! Explicit v1 conversion. Legacy metrics do not identify a complete v2 measurement.
use figkit_model::legacy as old;
use figkit_model::*;

/// Geometry-only migration with default limits. Legacy text needs a fresh complete request.
pub fn from_legacy(scene: &old::Scene) -> FigureResult<(Figure, CompatibilityResources)> {
    from_legacy_with_limits(scene, &Limits::default())
}
/// Geometry-only migration. Raw attributes/definitions stay in the compatibility envelope.
/// Catalogue marks remain an explicit compatibility-renderer concern; callers can lower them
/// to paths with `figkit_render::legacy::marks` before requesting portable conversion.
pub fn from_legacy_with_limits(
    scene: &old::Scene,
    limits: &Limits,
) -> FigureResult<(Figure, CompatibilityResources)> {
    if scene.version != 1 {
        return Err(FigureError::new(
            ErrorKind::InvalidInput,
            "unsupported legacy scene version",
        ));
    }
    // Check counts and depth before any recursive serialization or cloning.
    fn scan(
        nodes: &[old::Primitive],
        depth: usize,
        count: &mut usize,
        segments: &mut usize,
        limits: &Limits,
    ) -> FigureResult<()> {
        if !nodes.is_empty() && depth > limits.max_depth {
            return Err(super::budget("legacy conversion depth limit exceeded"));
        }
        for node in nodes {
            super::add(
                count,
                1,
                limits.max_primitives,
                "legacy conversion primitives",
            )?;
            let n = match node {
                old::Primitive::Path { points, .. } => points.len(),
                old::Primitive::Shape { commands, .. } => commands.len(),
                old::Primitive::Curve { segments, head, .. } => segments
                    .len()
                    .checked_add(head.as_ref().map_or(0, Vec::len))
                    .ok_or_else(|| super::budget("legacy path segments overflow"))?,
                _ => 0,
            };
            super::add(
                segments,
                n,
                limits.max_path_segments,
                "legacy path segments",
            )?;
            match node {
                old::Primitive::Text { .. } => {
                    return Err(FigureError::new(
                        ErrorKind::MissingMeasurement,
                        "version 1 text lacks a complete measured request/context; obtain a version 2 measurement",
                    ));
                }
                old::Primitive::Group { children, .. } => {
                    scan(children, depth + 1, count, segments, limits)?
                }
                _ => {}
            }
        }
        Ok(())
    }
    scan(&scene.primitives, 0, &mut 0, &mut 0, limits)?;
    let mut counter = super::ByteCounter {
        count: 0,
        limit: limits.max_input_bytes,
    };
    serde_json::to_writer(&mut counter, scene)
        .map_err(|_| super::budget("legacy conversion input byte limit exceeded"))?;
    super::legacy::validate(scene)
        .map_err(|e| FigureError::new(ErrorKind::InvalidInput, e.message))?;
    let mut compatibility = CompatibilityResources {
        definitions: scene.defs.clone(),
        ..Default::default()
    };
    let mut resources = Resources::default();
    let primitives = convert_nodes(
        &scene.primitives,
        &mut compatibility,
        &mut resources,
        &mut 0,
    )?;
    let figure = Figure {
        version: 2,
        size: Size {
            width: scene.width,
            height: scene.height,
        },
        metadata: FigureMetadata {
            title: scene.title.clone(),
            description: scene.description.clone(),
        },
        background: scene.background.clone(),
        primitives,
        resources,
    };
    // Also checks expansion introduced by geometric lowering before publishing the result.
    super::prepare_with_compatibility(&figure, &compatibility, limits)?;
    Ok((figure, compatibility))
}
fn unsupported(message: &str) -> FigureError {
    FigureError::new(ErrorKind::UnsupportedCapability, message)
}
fn convert_nodes(
    nodes: &[old::Primitive],
    compat: &mut CompatibilityResources,
    resources: &mut Resources,
    next: &mut usize,
) -> FigureResult<Vec<Primitive>> {
    let mut result = Vec::with_capacity(nodes.len());
    for node in nodes {
        let old = node.meta();
        let id = ElementId(format!("legacy-{}", *next));
        *next += 1;
        let metadata = ElementMetadata {
            element_id: Some(id.clone()),
            authored_id: old.id.clone(),
            classes: old.classes.clone(),
            label_group: old.joint.clone(),
        };
        if !old.attrs.is_empty() {
            compat.attributes.insert(id.clone(), old.attrs.clone());
        }
        if !old.data.is_empty() {
            let attrs = compat.attributes.entry(id.clone()).or_default();
            for (k, v) in &old.data {
                attrs.insert(format!("data-{k}"), v.clone());
            }
        }
        let converted = match node {
            old::Primitive::Rect { bounds, paint, .. } => {
                let radius = old
                    .attrs
                    .get("rx")
                    .map(|r| {
                        r.parse::<f64>()
                            .map_err(|_| unsupported("legacy rectangle radius is not numeric"))
                    })
                    .transpose()?
                    .unwrap_or(0.);
                if let Some(attrs) = compat.attributes.get_mut(&id) {
                    attrs.shift_remove("rx");
                }
                Primitive::Rect {
                    bounds: bounds.clone(),
                    radius,
                    paint: paint.clone(),
                    metadata,
                }
            }
            old::Primitive::Line {
                from, to, paint, ..
            } => Primitive::Line {
                from: from.clone(),
                to: to.clone(),
                paint: paint.clone(),
                metadata,
            },
            old::Primitive::Circle {
                center,
                radius,
                paint,
                ..
            } => Primitive::Circle {
                center: center.clone(),
                radius: *radius,
                paint: paint.clone(),
                metadata,
            },
            old::Primitive::Shape {
                commands, paint, ..
            } => Primitive::Path {
                commands: commands.clone(),
                paint: paint.clone(),
                metadata,
            },
            old::Primitive::Path {
                points,
                closed,
                paint,
                ..
            } => {
                let mut commands = points
                    .iter()
                    .enumerate()
                    .map(|(i, to)| {
                        if i == 0 {
                            PathCommand::MoveTo { to: to.clone() }
                        } else {
                            PathCommand::LineTo { to: to.clone() }
                        }
                    })
                    .collect::<Vec<_>>();
                if *closed {
                    commands.push(PathCommand::Close);
                }
                Primitive::Path {
                    commands,
                    paint: paint.clone(),
                    metadata,
                }
            }
            old::Primitive::Group {
                children,
                translate,
                clip,
                ..
            } => {
                let clip = clip.as_ref().map(|bounds| {
                    let id = ResourceId(format!("legacy-clip-{}", *next));
                    resources.clips.insert(
                        id.clone(),
                        Clip::Rect {
                            bounds: bounds.clone(),
                        },
                    );
                    id
                });
                Primitive::Group {
                    opacity: 1.,
                    children: convert_nodes(children, compat, resources, next)?,
                    translate: translate.clone(),
                    clip,
                    metadata,
                }
            }
            old::Primitive::Curve {
                start,
                segments,
                head,
                paint,
                ..
            } => {
                let mut commands = vec![PathCommand::MoveTo { to: start.clone() }];
                commands.extend(segments.iter().map(|s| PathCommand::CubicTo {
                    c1: s.c1.clone(),
                    c2: s.c2.clone(),
                    to: s.end.clone(),
                }));
                let shaft = Primitive::Path {
                    commands,
                    paint: Paint {
                        fill: None,
                        ..paint.clone()
                    },
                    metadata: ElementMetadata::default(),
                };
                let mut children = vec![shaft];
                if let Some(head) = head.as_ref().filter(|h| !h.is_empty()) {
                    let fill = paint
                        .stroke
                        .clone()
                        .ok_or_else(|| unsupported("curve head needs a stroke colour"))?;
                    let mut commands = vec![PathCommand::MoveTo {
                        to: head[0].clone(),
                    }];
                    commands.extend(
                        head[1..]
                            .iter()
                            .map(|p| PathCommand::LineTo { to: p.clone() }),
                    );
                    commands.push(PathCommand::Close);
                    children.push(Primitive::Path {
                        commands,
                        paint: Paint {
                            fill: Some(fill),
                            stroke: None,
                            stroke_width: 0.,
                            opacity: paint.opacity,
                        },
                        metadata: ElementMetadata::default(),
                    });
                }
                Primitive::Group {
                    opacity: 1.,
                    children,
                    translate: Point { x: 0., y: 0. },
                    clip: None,
                    metadata,
                }
            }
            old::Primitive::Wedge {
                center,
                radius,
                inner_radius,
                start_angle,
                end_angle,
                inset,
                paint,
                ..
            } => {
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
                .map_err(|e| FigureError::new(ErrorKind::InvalidInput, e.message))?;
                if commands.is_empty() {
                    Primitive::Group {
                        opacity: 1.,
                        children: vec![],
                        translate: Point { x: 0., y: 0. },
                        clip: None,
                        metadata,
                    }
                } else {
                    Primitive::Path {
                        commands: commands.iter().map(PathCommand::from).collect(),
                        paint: paint.clone(),
                        metadata,
                    }
                }
            }
            old::Primitive::Text { .. } => {
                return Err(FigureError::new(
                    ErrorKind::MissingMeasurement,
                    "version 1 text requires a complete measured request",
                ));
            }
            old::Primitive::Use { .. } => {
                return Err(unsupported(
                    "lower catalogue marks to supplied path geometry before portable conversion",
                ));
            }
        };
        result.push(converted);
    }
    Ok(result)
}
