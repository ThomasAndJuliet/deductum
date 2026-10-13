//! Checked preparation of already placed figures. No layout or text measurement occurs here.
use figkit_model::*;
use std::collections::BTreeSet;
pub mod conversion;
pub mod legacy;
pub mod lint;
pub mod profile;

#[derive(Debug, Clone)]
pub struct PreparedScene {
    figure: Figure,
    compatibility: CompatibilityResources,
    limits: Limits,
}
impl PreparedScene {
    pub fn figure(&self) -> &Figure {
        &self.figure
    }
    pub fn compatibility(&self) -> &CompatibilityResources {
        &self.compatibility
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
}
fn invalid(message: impl Into<String>) -> FigureError {
    FigureError::new(ErrorKind::InvalidInput, message)
}
fn budget(message: impl Into<String>) -> FigureError {
    FigureError::new(ErrorKind::BudgetExceeded, message)
}
fn point(p: &Point) -> FigureResult<()> {
    if p.x.is_finite() && p.y.is_finite() {
        Ok(())
    } else {
        Err(invalid("coordinates must be finite"))
    }
}
fn bounds(b: &Bounds) -> FigureResult<()> {
    point(&Point { x: b.x, y: b.y })?;
    if [b.width, b.height, b.x + b.width, b.y + b.height]
        .iter()
        .any(|v| !v.is_finite())
        || b.width < 0.
        || b.height < 0.
    {
        return Err(invalid("bounds must be finite with nonnegative size"));
    }
    Ok(())
}
fn color(c: &str) -> FigureResult<()> {
    if c.is_empty() || c.chars().any(char::is_control) {
        Err(invalid("paint must be nonempty and contain no controls"))
    } else {
        Ok(())
    }
}
fn paint(p: &Paint) -> FigureResult<()> {
    if p.fill.is_none() && p.stroke.is_none() {
        return Err(invalid("paint has neither fill nor stroke"));
    }
    for c in [&p.fill, &p.stroke].into_iter().flatten() {
        color(c)?;
    }
    if !p.stroke_width.is_finite()
        || p.stroke_width < 0.
        || !p.opacity.is_finite()
        || !(0. ..=1.).contains(&p.opacity)
    {
        return Err(invalid("invalid stroke width or opacity"));
    }
    Ok(())
}
struct Counts<'a> {
    limits: &'a Limits,
    nodes: usize,
    segments: usize,
    texts: usize,
}
fn add(count: &mut usize, n: usize, maximum: usize, name: &str) -> FigureResult<()> {
    *count = count
        .checked_add(n)
        .filter(|v| *v <= maximum)
        .ok_or_else(|| budget(format!("{name} limit exceeded")))?;
    Ok(())
}
fn path(commands: &[PathCommand], counts: &mut Counts<'_>) -> FigureResult<()> {
    add(
        &mut counts.segments,
        commands.len(),
        counts.limits.max_path_segments,
        "path segments",
    )?;
    if !matches!(commands.first(), Some(PathCommand::MoveTo { .. })) {
        return Err(invalid("path must start with move"));
    }
    for c in commands {
        match c {
            PathCommand::MoveTo { to } | PathCommand::LineTo { to } => point(to)?,
            PathCommand::CubicTo { c1, c2, to } => {
                point(c1)?;
                point(c2)?;
                point(to)?;
            }
            PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                to,
                ..
            } => {
                point(to)?;
                if !rx.is_finite()
                    || !ry.is_finite()
                    || !rotation.is_finite()
                    || *rx < 0.
                    || *ry < 0.
                {
                    return Err(invalid("invalid arc"));
                }
            }
            PathCommand::Close => {}
        }
    }
    Ok(())
}
fn text(t: &PlacedText) -> FigureResult<()> {
    point(&t.position)?;
    let request = &t.text.request;
    let typography = &request.typography;
    let metrics = &t.text.metrics;
    if request.id.0.is_empty()
        || request.context.id.0.is_empty()
        || request.context.engine.is_empty()
        || typography.font_family.is_empty()
        || !typography.font_size.is_finite()
        || typography.font_size <= 0.
        || typography.weight.is_empty()
        || typography.font_style.is_empty()
    {
        return Err(invalid("invalid measured request identity or typography"));
    }
    if request.max_width.is_some_and(|w| !w.is_finite() || w <= 0.) {
        return Err(invalid("invalid measurement width"));
    }
    if !metrics.size.width.is_finite()
        || !metrics.size.height.is_finite()
        || metrics.size.width < 0.
        || metrics.size.height < 0.
        || metrics
            .baseline
            .is_some_and(|b| !b.is_finite() || b < 0. || b > metrics.size.height)
    {
        return Err(invalid("invalid block metrics"));
    }
    if request
        .max_width
        .is_some_and(|w| metrics.size.width > w + 1e-6)
    {
        return Err(invalid("measured width exceeds request"));
    }
    match &request.content {
        TextContent::Plain { text } if text.contains('\0') => {
            return Err(invalid("text contains NUL"));
        }
        TextContent::Native { host, key, .. } if host.is_empty() || key.is_empty() => {
            return Err(invalid("native content requires host and key"));
        }
        _ => {}
    }
    match &t.text.paint {
        TextPaint::Resolved { fill } => color(fill)?,
        TextPaint::NativeInherited { host } => match &request.content {
            TextContent::Native { host: actual, .. } if !host.is_empty() && host == actual => {}
            _ => return Err(invalid("inherited ink requires matching native host")),
        },
    }
    Ok(())
}
fn world_point(p: &Point, offset: &Point) -> FigureResult<()> {
    point(&Point {
        x: p.x + offset.x,
        y: p.y + offset.y,
    })
}
fn world_bounds(b: &Bounds, offset: &Point) -> FigureResult<()> {
    bounds(&Bounds {
        x: b.x + offset.x,
        y: b.y + offset.y,
        width: b.width,
        height: b.height,
    })
}
fn world_path(commands: &[PathCommand], offset: &Point) -> FigureResult<()> {
    for command in commands {
        match command {
            PathCommand::MoveTo { to } | PathCommand::LineTo { to } => world_point(to, offset)?,
            PathCommand::CubicTo { c1, c2, to } => {
                world_point(c1, offset)?;
                world_point(c2, offset)?;
                world_point(to, offset)?;
            }
            PathCommand::ArcTo { rx, ry, to, .. } => {
                world_point(to, offset)?;
                // Reject unrepresentable intermediate radii as well as translated endpoints.
                point(&Point {
                    x: to.x + offset.x + rx,
                    y: to.y + offset.y + ry,
                })?;
                point(&Point {
                    x: to.x + offset.x - rx,
                    y: to.y + offset.y - ry,
                })?;
            }
            PathCommand::Close => {}
        }
    }
    Ok(())
}
fn world_node(node: &Primitive, offset: &Point, resources: &Resources) -> FigureResult<()> {
    match node {
        Primitive::Rect { bounds, .. } => world_bounds(bounds, offset)?,
        Primitive::Line { from, to, .. } => {
            world_point(from, offset)?;
            world_point(to, offset)?;
        }
        Primitive::Circle { center, radius, .. } => {
            world_point(
                &Point {
                    x: center.x + radius,
                    y: center.y + radius,
                },
                offset,
            )?;
            world_point(
                &Point {
                    x: center.x - radius,
                    y: center.y - radius,
                },
                offset,
            )?;
        }
        Primitive::Path { commands, .. } => world_path(commands, offset)?,
        Primitive::Text { placed } => {
            let size = &placed.text.metrics.size;
            let x = placed.position.x
                - match placed.anchor {
                    HorizontalAnchor::Start => 0.,
                    HorizontalAnchor::Middle => size.width / 2.,
                    HorizontalAnchor::End => size.width,
                };
            world_bounds(
                &Bounds {
                    x,
                    y: placed.position.y,
                    width: size.width,
                    height: size.height,
                },
                offset,
            )?;
        }
        Primitive::Use {
            position, symbol, ..
        } => {
            let offset = Point {
                x: position.x + offset.x,
                y: position.y + offset.y,
            };
            point(&offset)?;
            if let Some(symbol) = resources.symbols.get(symbol) {
                for shape in &symbol.shapes {
                    world_path(&shape.commands, &offset)?;
                }
            }
        }
        Primitive::Group {
            translate, clip, ..
        } => {
            let offset = Point {
                x: translate.x + offset.x,
                y: translate.y + offset.y,
            };
            point(&offset)?;
            if let Some(clip) = clip.as_ref().and_then(|id| resources.clips.get(id)) {
                match clip {
                    Clip::Rect { bounds } => world_bounds(bounds, &offset)?,
                    Clip::Path { commands } => world_path(commands, &offset)?,
                }
            }
        }
    }
    Ok(())
}
fn nodes(
    primitives: &[Primitive],
    depth: usize,
    offset: Point,
    resources: &Resources,
    counts: &mut Counts<'_>,
    ids: &mut BTreeSet<ElementId>,
    authored: &mut BTreeSet<String>,
) -> FigureResult<()> {
    if !primitives.is_empty() && depth > counts.limits.max_depth {
        return Err(budget("scene depth limit exceeded"));
    }
    for node in primitives {
        add(
            &mut counts.nodes,
            1,
            counts.limits.max_primitives,
            "primitives",
        )?;
        world_node(node, &offset, resources)?;
        let meta = node.metadata();
        if let Some(id) = &meta.element_id {
            if id.0.is_empty() || !ids.insert(id.clone()) {
                return Err(invalid("element ID is empty or duplicated"));
            }
        }
        if let Some(id) = &meta.authored_id {
            if id.is_empty() || !authored.insert(id.clone()) {
                return Err(invalid("authored ID is empty or duplicated"));
            }
        }
        match node {
            Primitive::Rect {
                bounds: b,
                radius,
                paint: p,
                ..
            } => {
                bounds(b)?;
                if !radius.is_finite() || *radius < 0. {
                    return Err(invalid("invalid rectangle radius"));
                }
                paint(p)?;
            }
            Primitive::Line {
                from, to, paint: p, ..
            } => {
                point(from)?;
                point(to)?;
                paint(p)?;
            }
            Primitive::Circle {
                center,
                radius,
                paint: p,
                ..
            } => {
                point(center)?;
                if !radius.is_finite() || *radius < 0. {
                    return Err(invalid("invalid circle radius"));
                }
                paint(p)?;
            }
            Primitive::Path {
                commands, paint: p, ..
            } => {
                path(commands, counts)?;
                paint(p)?;
            }
            Primitive::Text { placed } => {
                add(
                    &mut counts.texts,
                    1,
                    counts.limits.max_measurement_requests,
                    "measurement requests",
                )?;
                text(placed)?;
            }
            Primitive::Group {
                children,
                translate,
                clip,
                ..
            } => {
                point(translate)?;
                let offset = Point {
                    x: offset.x + translate.x,
                    y: offset.y + translate.y,
                };
                point(&offset)?;
                if let Some(id) = clip {
                    if !resources.clips.contains_key(id) {
                        return Err(FigureError::new(
                            ErrorKind::ResourceFailure,
                            format!("missing clip {id}"),
                        ));
                    }
                }
                nodes(
                    children,
                    depth + 1,
                    offset,
                    resources,
                    counts,
                    ids,
                    authored,
                )?;
            }
            Primitive::Use {
                symbol, position, ..
            } => {
                point(position)?;
                let symbol = resources.symbols.get(symbol).ok_or_else(|| {
                    FigureError::new(ErrorKind::ResourceFailure, "missing symbol")
                })?;
                // Charge each instance before renderers expand it.
                add(
                    &mut counts.nodes,
                    symbol.shapes.len(),
                    counts.limits.max_primitives,
                    "expanded primitives",
                )?;
                for shape in &symbol.shapes {
                    add(
                        &mut counts.segments,
                        shape.commands.len(),
                        counts.limits.max_path_segments,
                        "expanded path segments",
                    )?;
                }
            }
        }
    }
    Ok(())
}
struct ByteCounter {
    count: usize,
    limit: usize,
}
impl std::io::Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.count = self
            .count
            .checked_add(bytes.len())
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| std::io::Error::other("input byte limit exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn assign(primitives: &mut [Primitive], ids: &mut BTreeSet<ElementId>, next: &mut usize) {
    for node in primitives {
        if node.metadata().element_id.is_none() {
            loop {
                let id = ElementId(format!("figkit-{next}"));
                *next += 1;
                if ids.insert(id.clone()) {
                    node.metadata_mut().element_id = Some(id);
                    break;
                }
            }
        }
        if let Primitive::Group { children, .. } = node {
            assign(children, ids, next);
        }
    }
}
pub fn prepare(figure: &Figure, limits: &Limits) -> FigureResult<PreparedScene> {
    prepare_with_compatibility(figure, &CompatibilityResources::default(), limits)
}
pub fn prepare_with_compatibility(
    figure: &Figure,
    compatibility: &CompatibilityResources,
    limits: &Limits,
) -> FigureResult<PreparedScene> {
    if figure.version != 2 {
        return Err(invalid("unsupported portable scene version"));
    }
    if !figure.size.width.is_finite()
        || !figure.size.height.is_finite()
        || figure.size.width <= 0.
        || figure.size.height <= 0.
    {
        return Err(invalid("figure dimensions must be positive and finite"));
    }
    color(&figure.background)?;
    let resources = &figure.resources;
    if resources
        .clips
        .len()
        .checked_add(resources.symbols.len())
        .and_then(|n| n.checked_add(compatibility.definitions.len()))
        .and_then(|n| n.checked_add(compatibility.attributes.len()))
        .is_none_or(|n| n > limits.max_resources)
    {
        return Err(budget("resource count limit exceeded"));
    }
    let mut counts = Counts {
        limits,
        nodes: 0,
        segments: 0,
        texts: 0,
    };
    for (id, clip) in &resources.clips {
        if id.0.is_empty() {
            return Err(invalid("empty clip ID"));
        }
        match clip {
            Clip::Rect { bounds: b } => bounds(b)?,
            Clip::Path { commands } => path(commands, &mut counts)?,
        }
    }
    for (id, symbol) in &resources.symbols {
        if id.0.is_empty() {
            return Err(invalid("empty symbol ID"));
        }
        add(
            &mut counts.nodes,
            symbol.shapes.len(),
            limits.max_primitives,
            "resource primitives",
        )?;
        for shape in &symbol.shapes {
            path(&shape.commands, &mut counts)?;
            paint(&shape.paint)?;
        }
    }
    let mut ids = BTreeSet::new();
    nodes(
        &figure.primitives,
        0,
        Point { x: 0., y: 0. },
        resources,
        &mut counts,
        &mut ids,
        &mut BTreeSet::new(),
    )?;
    let mut counter = ByteCounter {
        count: 0,
        limit: limits.max_input_bytes,
    };
    serde_json::to_writer(&mut counter, figure).map_err(|_| budget("input byte limit exceeded"))?;
    serde_json::to_writer(&mut counter, compatibility)
        .map_err(|_| budget("input byte limit exceeded"))?;
    let mut figure = figure.clone();
    assign(&mut figure.primitives, &mut ids, &mut 0);
    for id in compatibility.attributes.keys() {
        if !ids.contains(id) {
            return Err(FigureError::new(
                ErrorKind::ResourceFailure,
                format!("attributes refer to missing element {id}"),
            ));
        }
    }
    Ok(PreparedScene {
        figure,
        compatibility: compatibility.clone(),
        limits: limits.clone(),
    })
}
