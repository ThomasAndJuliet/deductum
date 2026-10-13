//! Validation and lowering of an already placed chart. No relayout occurs here.
use figkit_model::legacy::*;
use std::collections::BTreeSet;

fn invalid(path: &str, message: &str) -> Error {
    Error::new("scene", message).at(path)
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn point(p: &Point, path: &str) -> Result<()> {
    if finite(p.x) && finite(p.y) {
        Ok(())
    } else {
        Err(invalid(path, "point coordinates must be finite"))
    }
}
fn bounds(b: &Bounds, path: &str) -> Result<()> {
    if !finite(b.x)
        || !finite(b.y)
        || !finite(b.width)
        || !finite(b.height)
        || b.width < 0.
        || b.height < 0.
    {
        Err(invalid(
            path,
            "bounds must have finite coordinates and nonnegative size",
        ))
    } else {
        Ok(())
    }
}
fn color(c: &str, path: &str) -> Result<()> {
    if c.trim().is_empty() || c.chars().any(char::is_control) {
        Err(invalid(
            path,
            "paint color must be nonempty and contain no controls",
        ))
    } else {
        Ok(())
    }
}
fn paint(p: &Paint, path: &str) -> Result<()> {
    if p.fill.is_none() && p.stroke.is_none() {
        return Err(invalid(path, "paint has neither fill nor stroke"));
    }
    if let Some(c) = &p.fill {
        color(c, path)?;
    }
    if let Some(c) = &p.stroke {
        color(c, path)?;
    }
    if !finite(p.stroke_width)
        || p.stroke_width < 0.
        || !finite(p.opacity)
        || !(0. ..=1.).contains(&p.opacity)
    {
        return Err(invalid(path, "stroke width and opacity are invalid"));
    }
    Ok(())
}
fn primitive(p: &Primitive, path: &str, depth: usize, seen: &mut BTreeSet<String>) -> Result<()> {
    if depth > 32 {
        return Err(invalid(path, "group nesting exceeds 32 levels"));
    }
    let meta = p.meta();
    if let Some(id) = &meta.id {
        if id.is_empty() || !seen.insert(id.clone()) {
            return Err(invalid(path, "primitive ID is empty or duplicated"));
        }
    }
    match p {
        Primitive::Rect {
            bounds: b,
            paint: a,
            ..
        } => {
            bounds(b, path)?;
            paint(a, path)
        }
        Primitive::Line {
            from, to, paint: a, ..
        } => {
            point(from, path)?;
            point(to, path)?;
            paint(a, path)
        }
        Primitive::Path {
            points,
            closed,
            paint: a,
            ..
        } => {
            if points.len() < if *closed { 3 } else { 2 } {
                return Err(invalid(path, "path has too few points"));
            }
            for p in points {
                point(p, path)?;
            }
            paint(a, path)
        }
        Primitive::Circle {
            center,
            radius,
            paint: a,
            ..
        } => {
            point(center, path)?;
            if !finite(*radius) || *radius < 0. {
                return Err(invalid(
                    path,
                    "circle radius must be finite and nonnegative",
                ));
            }
            paint(a, path)
        }
        Primitive::Text {
            position,
            request,
            metrics,
            anchor,
            ..
        } => {
            point(position, path)?;
            if !["start", "middle", "end"].contains(&anchor.as_str()) {
                return Err(invalid(path, "text anchor must be start, middle or end"));
            }
            if request.id.is_empty()
                || request.context.is_empty()
                || request.role.is_empty()
                || request.font_family.is_empty()
                || !finite(request.font_size)
                || request.font_size <= 0.
            {
                return Err(invalid(path, "text request is invalid"));
            }
            if request.max_width.is_some_and(|w| !finite(w) || w <= 0.) {
                return Err(invalid(path, "text maximum width is invalid"));
            }
            match &request.content {
                TextContent::Plain { text } => {
                    if text.chars().any(|c| c == '\0') {
                        return Err(invalid(path, "text contains NUL"));
                    }
                }
                TextContent::Native { host, key, .. } => {
                    if host.is_empty() || key.is_empty() {
                        return Err(invalid(path, "native text requires host and key"));
                    }
                }
            }
            if !finite(metrics.width)
                || !finite(metrics.height)
                || metrics.width < 0.
                || metrics.height <= 0.
                || metrics
                    .baseline
                    .is_some_and(|b| !finite(b) || b < 0. || b > metrics.height)
            {
                return Err(invalid(path, "text metrics are invalid"));
            }
            if request.max_width.is_some_and(|w| metrics.width > w + 1e-6) {
                return Err(invalid(path, "text exceeds requested maximum width"));
            }
            Ok(())
        }
        Primitive::Group {
            children,
            translate,
            clip,
            ..
        } => {
            point(translate, path)?;
            if let Some(b) = clip {
                bounds(b, path)?;
            }
            for (i, p) in children.iter().enumerate() {
                primitive(p, &format!("{path}/children/{i}"), depth + 1, seen)?;
            }
            Ok(())
        }
        Primitive::Use {
            center,
            size,
            href,
            paint: a,
            ..
        } => {
            point(center, path)?;
            if !finite(*size) || *size <= 0. || href.is_empty() {
                return Err(invalid(
                    path,
                    "symbol mark needs a symbol and a positive size",
                ));
            }
            paint(a, path)
        }
        Primitive::Curve {
            start,
            segments,
            head,
            paint: a,
            ..
        } => {
            point(start, path)?;
            for s in segments {
                point(&s.c1, path)?;
                point(&s.c2, path)?;
                point(&s.end, path)?;
            }
            for p in head.iter().flatten() {
                point(p, path)?;
            }
            paint(a, path)
        }
        Primitive::Wedge {
            center,
            radius,
            inner_radius,
            start_angle,
            end_angle,
            inset,
            paint: a,
            ..
        } => {
            point(center, path)?;
            if [radius, inner_radius, start_angle, end_angle, inset]
                .iter()
                .any(|v| !finite(**v))
                || *radius <= 0.
                || *inner_radius < 0.
                || *inner_radius >= *radius
                || *inset < 0.
            {
                return Err(invalid(path, "wedge radii, angles or inset are invalid"));
            }
            paint(a, path)
        }
        Primitive::Shape {
            commands, paint: a, ..
        } => {
            if !matches!(commands.first(), Some(PathCommand::MoveTo { .. })) {
                return Err(invalid(path, "path data must start with a move"));
            }
            for c in commands {
                match c {
                    PathCommand::MoveTo { to } | PathCommand::LineTo { to } => point(to, path)?,
                    PathCommand::CubicTo { c1, c2, to } => {
                        point(c1, path)?;
                        point(c2, path)?;
                        point(to, path)?;
                    }
                    PathCommand::ArcTo {
                        rx,
                        ry,
                        rotation,
                        to,
                        ..
                    } => {
                        point(to, path)?;
                        if !finite(*rx)
                            || !finite(*ry)
                            || !finite(*rotation)
                            || *rx < 0.
                            || *ry < 0.
                        {
                            return Err(invalid(path, "arc radii and rotation are invalid"));
                        }
                    }
                    PathCommand::Close => {}
                }
            }
            paint(a, path)
        }
    }
}
fn validate_parts(
    width: f64,
    height: f64,
    background: &str,
    primitives: &[Primitive],
) -> Result<()> {
    if !finite(width) || !finite(height) || width <= 0. || height <= 0. {
        return Err(invalid(
            "chart",
            "chart dimensions must be positive and finite",
        ));
    }
    color(background, "background")?;
    let mut seen = BTreeSet::new();
    for (i, p) in primitives.iter().enumerate() {
        primitive(p, &format!("primitives/{i}"), 0, &mut seen)?;
    }
    Ok(())
}
/// Validate an externally supplied scene using the same invariants as lowering.
pub fn validate(scene: &Scene) -> Result<()> {
    if scene.version != 1 {
        return Err(invalid("version", "unsupported scene version"));
    }
    validate_parts(
        scene.width,
        scene.height,
        &scene.background,
        &scene.primitives,
    )
}
