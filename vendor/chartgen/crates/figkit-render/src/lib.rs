//! Stateless encoders for checked measured scenes; the explicit legacy module retains v1 output.
pub mod legacy;
use figkit_model::*;
use figkit_scene::PreparedScene;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Svg,
    Pgf,
    Typst,
    Browser,
    CompatibilitySvg,
}
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub path_clips: bool,
    pub native_host: Option<&'static str>,
    pub compatibility_resources: bool,
}
pub fn capabilities(backend: Backend) -> Capabilities {
    Capabilities {
        path_clips: !matches!(backend, Backend::Typst | Backend::Browser),
        native_host: match backend {
            Backend::Pgf => Some("latex"),
            Backend::Typst => Some("typst"),
            Backend::Browser => Some("browser"),
            _ => None,
        },
        compatibility_resources: backend == Backend::CompatibilitySvg,
    }
}
fn unsupported(message: impl Into<String>) -> FigureError {
    FigureError::new(ErrorKind::UnsupportedCapability, message)
}
fn solid(c: &str) -> FigureResult<()> {
    if figkit_scene::profile::relative_luminance(c).is_some() {
        Ok(())
    } else {
        Err(unsupported(format!("unsupported solid paint {c}")))
    }
}
fn check_paint(p: &Paint) -> FigureResult<()> {
    for c in [&p.fill, &p.stroke].into_iter().flatten() {
        solid(c)?;
    }
    Ok(())
}
fn check_nodes(nodes: &[Primitive], backend: Backend) -> FigureResult<()> {
    for node in nodes {
        match node {
            Primitive::Rect { paint, .. }
            | Primitive::Line { paint, .. }
            | Primitive::Circle { paint, .. }
            | Primitive::Path { paint, .. } => check_paint(paint)?,
            Primitive::Text { placed } => {
                let text = &placed.text;
                let typography = &text.request.typography;
                if !matches!(typography.weight.as_str(), "normal" | "bold")
                    || !matches!(typography.font_style.as_str(), "normal" | "italic")
                {
                    return Err(unsupported(
                        "backend supports normal/bold and normal/italic typography",
                    ));
                }
                match &text.paint {
                    TextPaint::Resolved { fill } => solid(fill)?,
                    TextPaint::NativeInherited { host } => {
                        if !matches!(
                            (backend, host.as_str()),
                            (Backend::Typst, "typst")
                                | (Backend::Browser, "browser")
                                | (Backend::Pgf, "latex")
                                | (Backend::Pgf, "tex")
                        ) {
                            return Err(unsupported("backend cannot inherit native ink"));
                        }
                    }
                }
                match &text.request.content {
                    TextContent::Plain { text: value } => {
                        if matches!(
                            backend,
                            Backend::Svg | Backend::CompatibilitySvg | Backend::Pgf
                        ) && value.contains('\t')
                        {
                            return Err(unsupported(
                                "portable SVG/PGF plain text does not support tabs; supply composed lines or native content",
                            ));
                        }
                        if !matches!(backend, Backend::Typst | Backend::Browser)
                            && (value.contains('\n')
                                || value.contains('\r')
                                || text.request.max_width.is_some())
                        {
                            return Err(unsupported(
                                "portable text requires composed lines or native blocks for wrapping",
                            ));
                        }
                        if matches!(
                            backend,
                            Backend::Svg | Backend::CompatibilitySvg | Backend::Pgf
                        ) && text.metrics.baseline.is_none()
                        {
                            return Err(unsupported(
                                "plain text encoding requires a measured baseline",
                            ));
                        }
                        if backend == Backend::Pgf
                            && !typography
                                .font_family
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                        {
                            return Err(unsupported(
                                "PGF font family must be an NFSS family identifier",
                            ));
                        }
                    }
                    TextContent::Native { host, key, .. } => {
                        if !matches!(
                            (backend, host.as_str()),
                            (Backend::Typst, "typst")
                                | (Backend::Browser, "browser")
                                | (Backend::Pgf, "latex")
                                | (Backend::Pgf, "tex")
                        ) {
                            return Err(unsupported(format!(
                                "native host {host} is not supported by {backend:?}; an alternative must be independently measured"
                            )));
                        }
                        if backend == Backend::Pgf
                            && !key.bytes().all(|b| {
                                b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':')
                            })
                        {
                            return Err(unsupported(
                                "PGF native key contains unsupported characters",
                            ));
                        }
                    }
                }
            }
            Primitive::Group {
                children, opacity, ..
            } => {
                if !opacity.is_finite() || !(0. ..=1.).contains(opacity) {
                    return Err(unsupported("group opacity must lie between 0 and 1"));
                }
                // The Typst and browser plans flatten groups into separate layers, which cannot
                // composite as one: refused rather than drawn with every child translucent.
                if *opacity < 1. && matches!(backend, Backend::Typst | Backend::Browser) {
                    return Err(unsupported(
                        "group opacity is not supported by the Typst and browser layer plans",
                    ));
                }
                check_nodes(children, backend)?
            }
            Primitive::Use { .. } => {}
        }
    }
    Ok(())
}
/// Completes all capability checks before a renderer allocates its output buffer.
pub fn validate_capabilities(scene: &PreparedScene, backend: Backend) -> FigureResult<()> {
    let caps = capabilities(backend);
    let resources = scene.compatibility();
    if !caps.compatibility_resources
        && (!resources.definitions.is_empty()
            || resources.attributes.values().any(|a| !a.is_empty()))
    {
        return Err(unsupported(
            "raw SVG resources require the compatibility SVG backend",
        ));
    }
    if !caps.path_clips
        && scene
            .figure()
            .resources
            .clips
            .values()
            .any(|c| matches!(c, Clip::Path { .. }))
    {
        return Err(unsupported(
            "Native placement plans currently support rectangular clipping only",
        ));
    }
    fn check_ids(nodes: &[Primitive], clip_count: usize, native_plan: bool) -> FigureResult<()> {
        for node in nodes {
            if let Some(id) = &node.metadata().authored_id {
                let collision = id
                    .strip_prefix("figkit-clip-")
                    .and_then(|n| n.parse::<usize>().ok())
                    .is_some_and(|n| n < clip_count && id == &format!("figkit-clip-{n}"))
                    || (native_plan && id == "figkit-host-clip");
                if collision {
                    return Err(FigureError::new(
                        ErrorKind::ResourceFailure,
                        "authored SVG ID collides with generated clipping resource",
                    ));
                }
            }
            if let Primitive::Group { children, .. } = node {
                check_ids(children, clip_count, native_plan)?;
            }
        }
        Ok(())
    }
    check_ids(
        &scene.figure().primitives,
        scene.figure().resources.clips.len(),
        matches!(backend, Backend::Typst | Backend::Browser),
    )?;
    solid(&scene.figure().background)?;
    for symbol in scene.figure().resources.symbols.values() {
        for shape in &symbol.shapes {
            check_paint(&shape.paint)?;
        }
    }
    check_nodes(&scene.figure().primitives, backend)
}
pub trait Renderer {
    fn render(&self, scene: &PreparedScene) -> FigureResult<String>;
}
macro_rules! renderer {
    ($name:ident,$backend:ident) => {
        #[derive(Default)]
        pub struct $name;
        impl Renderer for $name {
            fn render(&self, scene: &PreparedScene) -> FigureResult<String> {
                render(scene, Backend::$backend)
            }
        }
    };
}
renderer!(SvgRenderer, Svg);
renderer!(PgfRenderer, Pgf);
renderer!(TypstRenderer, Typst);
renderer!(BrowserRenderer, Browser);
renderer!(CompatibilitySvgRenderer, CompatibilitySvg);

struct Output {
    value: String,
    limit: usize,
}
impl Output {
    fn new(limit: usize) -> Self {
        Self {
            value: String::new(),
            limit,
        }
    }
}
impl std::fmt::Write for Output {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        if self
            .value
            .len()
            .checked_add(s.len())
            .is_none_or(|n| n > self.limit)
        {
            return Err(std::fmt::Error);
        }
        self.value.push_str(s);
        Ok(())
    }
}
impl std::io::Write for Output {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let s = std::str::from_utf8(b).map_err(std::io::Error::other)?;
        self.write_str(s)
            .map_err(|_| std::io::Error::other("output byte limit exceeded"))?;
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn exhausted(_: std::fmt::Error) -> FigureError {
    FigureError::new(ErrorKind::BudgetExceeded, "output byte limit exceeded")
}
macro_rules! w {($out:expr,$($arg:tt)*)=>{write!($out,$($arg)*).map_err(exhausted)?};}
fn xml(out: &mut Output, value: &str) -> FigureResult<()> {
    for c in value.chars() {
        match c {
            '&' => w!(out, "&amp;"),
            '<' => w!(out, "&lt;"),
            '>' => w!(out, "&gt;"),
            '"' => w!(out, "&quot;"),
            '\'' => w!(out, "&apos;"),
            c => w!(out, "{c}"),
        }
    }
    Ok(())
}
fn meta(
    out: &mut Output,
    metadata: &ElementMetadata,
    scene: &PreparedScene,
    compat: bool,
) -> FigureResult<()> {
    if let Some(id) = &metadata.authored_id {
        w!(out, " id=\"");
        xml(out, id)?;
        w!(out, "\"");
    }
    if !metadata.classes.is_empty() {
        w!(out, " class=\"");
        for (i, class) in metadata.classes.iter().enumerate() {
            if i > 0 {
                w!(out, " ");
            }
            xml(out, class)?;
        }
        w!(out, "\"");
    }
    if compat {
        if let Some(attrs) = metadata
            .element_id
            .as_ref()
            .and_then(|id| scene.compatibility().attributes.get(id))
        {
            for (k, v) in attrs {
                w!(out, " ");
                xml(out, k)?;
                w!(out, "=\"");
                xml(out, v)?;
                w!(out, "\"");
            }
        }
    }
    Ok(())
}
fn svg_paint(out: &mut Output, paint: &Paint) -> FigureResult<()> {
    w!(out, " fill=\"");
    xml(out, paint.fill.as_deref().unwrap_or("none"))?;
    w!(out, "\" stroke=\"");
    xml(out, paint.stroke.as_deref().unwrap_or("none"))?;
    w!(
        out,
        "\" stroke-width=\"{}\" opacity=\"{}\"",
        paint.stroke_width,
        paint.opacity
    );
    Ok(())
}
fn svg_path(out: &mut Output, commands: &[PathCommand]) -> FigureResult<()> {
    for c in commands {
        match c {
            PathCommand::MoveTo { to } => w!(out, "M{} {} ", to.x, to.y),
            PathCommand::LineTo { to } => w!(out, "L{} {} ", to.x, to.y),
            PathCommand::CubicTo { c1, c2, to } => w!(
                out,
                "C{} {} {} {} {} {} ",
                c1.x,
                c1.y,
                c2.x,
                c2.y,
                to.x,
                to.y
            ),
            PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to,
            } => w!(
                out,
                "A{rx} {ry} {rotation} {} {} {} {} ",
                u8::from(*large_arc),
                u8::from(*sweep),
                to.x,
                to.y
            ),
            PathCommand::Close => w!(out, "Z "),
        }
    }
    Ok(())
}
fn anchor_x(t: &PlacedText) -> f64 {
    t.position.x
        - match t.anchor {
            HorizontalAnchor::Start => 0.,
            HorizontalAnchor::Middle => t.text.metrics.size.width / 2.,
            HorizontalAnchor::End => t.text.metrics.size.width,
        }
}
fn clip_index(scene: &PreparedScene, id: &ResourceId) -> usize {
    scene
        .figure()
        .resources
        .clips
        .keys()
        .position(|k| k == id)
        .expect("prepared resource")
}
fn svg_nodes(
    out: &mut Output,
    nodes: &[Primitive],
    scene: &PreparedScene,
    compat: bool,
) -> FigureResult<()> {
    for node in nodes {
        match node {
            Primitive::Rect {
                bounds: b,
                radius,
                paint,
                metadata,
            } => {
                w!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{radius}\"",
                    b.x,
                    b.y,
                    b.width,
                    b.height
                );
                svg_paint(out, paint)?;
                meta(out, metadata, scene, compat)?;
                w!(out, "/>\n");
            }
            Primitive::Line {
                from,
                to,
                paint,
                metadata,
            } => {
                w!(
                    out,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"",
                    from.x,
                    from.y,
                    to.x,
                    to.y
                );
                svg_paint(out, paint)?;
                meta(out, metadata, scene, compat)?;
                w!(out, "/>\n");
            }
            Primitive::Circle {
                center,
                radius,
                paint,
                metadata,
            } => {
                w!(
                    out,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{radius}\"",
                    center.x,
                    center.y
                );
                svg_paint(out, paint)?;
                meta(out, metadata, scene, compat)?;
                w!(out, "/>\n");
            }
            Primitive::Path {
                commands,
                paint,
                metadata,
            } => {
                w!(out, "<path d=\"");
                svg_path(out, commands)?;
                w!(out, "\"");
                svg_paint(out, paint)?;
                meta(out, metadata, scene, compat)?;
                w!(out, "/>\n");
            }
            Primitive::Text { placed: t } => {
                let typography = &t.text.request.typography;
                w!(
                    out,
                    "<text xml:space=\"preserve\" style=\"white-space: pre\" x=\"{}\" y=\"{}\" font-family=\"",
                    anchor_x(t),
                    t.position.y + t.text.metrics.baseline.expect("capability checked")
                );
                xml(out, &typography.font_family)?;
                w!(
                    out,
                    "\" font-size=\"{}\" font-weight=\"{}\" font-style=\"{}\" fill=\"",
                    typography.font_size,
                    typography.weight,
                    typography.font_style
                );
                if let TextPaint::Resolved { fill } = &t.text.paint {
                    xml(out, fill)?;
                }
                w!(out, "\"");
                meta(out, &t.metadata, scene, compat)?;
                w!(out, ">");
                if let TextContent::Plain { text } = &t.text.request.content {
                    xml(out, text)?;
                }
                w!(out, "</text>\n");
            }
            Primitive::Group {
                children,
                translate,
                clip,
                opacity,
                metadata,
            } => {
                w!(
                    out,
                    "<g transform=\"translate({} {})\"",
                    translate.x,
                    translate.y
                );
                if *opacity < 1. {
                    w!(out, " opacity=\"{}\"", opacity);
                }
                meta(out, metadata, scene, compat)?;
                if let Some(id) = clip {
                    w!(
                        out,
                        " clip-path=\"url(#figkit-clip-{})\"",
                        clip_index(scene, id)
                    );
                }
                w!(out, ">\n");
                svg_nodes(out, children, scene, compat)?;
                w!(out, "</g>\n");
            }
            Primitive::Use {
                symbol,
                position,
                metadata,
            } => {
                w!(
                    out,
                    "<g transform=\"translate({} {})\"",
                    position.x,
                    position.y
                );
                meta(out, metadata, scene, compat)?;
                w!(out, ">\n");
                for shape in &scene.figure().resources.symbols[symbol].shapes {
                    w!(out, "<path d=\"");
                    svg_path(out, &shape.commands)?;
                    w!(out, "\"");
                    svg_paint(out, &shape.paint)?;
                    w!(out, "/>\n");
                }
                w!(out, "</g>\n");
            }
        }
    }
    Ok(())
}
fn svg_start(out: &mut Output, scene: &PreparedScene) -> FigureResult<()> {
    let f = scene.figure();
    w!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}pt\" height=\"{}pt\" viewBox=\"0 0 {} {}\" role=\"img\">\n",
        f.size.width,
        f.size.height,
        f.size.width,
        f.size.height
    );
    for (tag, value) in [
        ("title", &f.metadata.title),
        ("desc", &f.metadata.description),
    ] {
        if let Some(value) = value {
            w!(out, "<{tag}>");
            xml(out, value)?;
            w!(out, "</{tag}>\n");
        }
    }
    w!(out, "<defs>");
    for (i, clip) in f.resources.clips.values().enumerate() {
        w!(out, "<clipPath id=\"figkit-clip-{i}\">");
        match clip {
            Clip::Rect { bounds: b } => w!(
                out,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                b.x,
                b.y,
                b.width,
                b.height
            ),
            Clip::Path { commands } => {
                w!(out, "<path d=\"");
                svg_path(out, commands)?;
                w!(out, "\"/>");
            }
        }
        w!(out, "</clipPath>");
    }
    Ok(())
}
fn svg(out: &mut Output, scene: &PreparedScene, compat: bool) -> FigureResult<()> {
    svg_start(out, scene)?;
    if compat {
        for definition in &scene.compatibility().definitions {
            w!(out, "{definition}");
        }
    }
    w!(out, "</defs>\n");
    let f = scene.figure();
    w!(
        out,
        "<rect width=\"{}\" height=\"{}\" fill=\"",
        f.size.width,
        f.size.height
    );
    xml(out, &f.background)?;
    w!(out, "\"/>\n");
    svg_nodes(out, &f.primitives, scene, compat)?;
    w!(out, "</svg>\n");
    Ok(())
}
fn rgb(c: &str) -> (u8, u8, u8) {
    let value = c.trim().to_ascii_lowercase();
    let h = match value.as_str() {
        "black" => "000000",
        "white" => "ffffff",
        "red" => "ff0000",
        "green" => "008000",
        "blue" => "0000ff",
        "gray" | "grey" => "808080",
        _ => &value[1..],
    };
    let expanded;
    if h.len() == 3 {
        expanded = h.chars().flat_map(|c| [c, c]).collect::<String>();
        let channel = |i| u8::from_str_radix(&expanded[i..i + 2], 16).unwrap();
        (channel(0), channel(2), channel(4))
    } else {
        let channel = |i| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
        (channel(0), channel(2), channel(4))
    }
}
fn pgf_color(out: &mut Output, c: &str) -> FigureResult<()> {
    let (r, g, b) = rgb(c);
    w!(out, "{{rgb,255:red,{r};green,{g};blue,{b}}}");
    Ok(())
}
fn pgf_paint(out: &mut Output, p: &Paint) -> FigureResult<()> {
    w!(
        out,
        "\\pgfsetlinewidth{{{}bp}}\\pgfsetfillopacity{{{}}}\\pgfsetstrokeopacity{{{}}}\n",
        p.stroke_width,
        p.opacity,
        p.opacity
    );
    if let Some(c) = &p.fill {
        w!(out, "\\pgfsetfillcolor");
        pgf_color(out, c)?;
    }
    if let Some(c) = &p.stroke {
        w!(out, "\\pgfsetstrokecolor");
        pgf_color(out, c)?;
    }
    Ok(())
}
fn pgf_finish(out: &mut Output, p: &Paint) -> FigureResult<()> {
    w!(
        out,
        "\\pgfusepath{{{}}}\n",
        match (p.fill.is_some(), p.stroke.is_some()) {
            (true, true) => "fill,stroke",
            (true, false) => "fill",
            (false, true) => "stroke",
            _ => "discard",
        }
    );
    Ok(())
}
fn pgf_path(out: &mut Output, commands: &[PathCommand]) -> FigureResult<()> {
    for c in commands {
        match c {
            PathCommand::MoveTo { to } => w!(
                out,
                "\\pgfpathmoveto{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
                to.x,
                -to.y
            ),
            PathCommand::LineTo { to } => w!(
                out,
                "\\pgfpathlineto{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
                to.x,
                -to.y
            ),
            PathCommand::CubicTo { c1, c2, to } => w!(
                out,
                "\\pgfpathcurveto{{\\pgfpoint{{{}bp}}{{{}bp}}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
                c1.x,
                -c1.y,
                c2.x,
                -c2.y,
                to.x,
                -to.y
            ),
            PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to,
            } => w!(
                out,
                "\\pgfpatharcto{{{rx}bp}}{{{ry}bp}}{{{}}}{{{}}}{{{}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
                -rotation,
                u8::from(*large_arc),
                u8::from(!*sweep),
                to.x,
                -to.y
            ),
            PathCommand::Close => w!(out, "\\pgfpathclose"),
        }
    }
    Ok(())
}
fn pgf_rect(out: &mut Output, b: &Bounds, radius: f64) -> FigureResult<()> {
    let radius_x = radius.min(b.width / 2.);
    let radius_y = radius.min(b.height / 2.);
    w!(
        out,
        "\\pgfsetcornersarced{{\\pgfpoint{{{radius_x}bp}}{{{radius_y}bp}}}}\\pgfpathrectangle{{\\pgfpoint{{{}bp}}{{{}bp}}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
        b.x,
        -b.y - b.height,
        b.width,
        b.height
    );
    Ok(())
}
fn tex(out: &mut Output, text: &str) -> FigureResult<()> {
    for c in text.chars() {
        match c {
            ' ' => w!(out, "\\ "),
            '\\' => w!(out, "\\textbackslash{{}}"),
            '{' => w!(out, "\\{{"),
            '}' => w!(out, "\\}}"),
            '%' => w!(out, "\\%"),
            '&' => w!(out, "\\&"),
            '#' => w!(out, "\\#"),
            '$' => w!(out, "\\$"),
            '_' => w!(out, "\\_"),
            '^' => w!(out, "\\textasciicircum{{}}"),
            '~' => w!(out, "\\textasciitilde{{}}"),
            c => w!(out, "{c}"),
        }
    }
    Ok(())
}
fn pgf_nodes(out: &mut Output, nodes: &[Primitive], scene: &PreparedScene) -> FigureResult<()> {
    for node in nodes {
        w!(out, "\\begin{{pgfscope}}\n");
        match node {
            Primitive::Rect {
                bounds,
                radius,
                paint,
                ..
            } => {
                pgf_paint(out, paint)?;
                pgf_rect(out, bounds, *radius)?;
                pgf_finish(out, paint)?;
            }
            Primitive::Line {
                from, to, paint, ..
            } => {
                pgf_paint(out, paint)?;
                pgf_path(
                    out,
                    &[
                        PathCommand::MoveTo { to: from.clone() },
                        PathCommand::LineTo { to: to.clone() },
                    ],
                )?;
                pgf_finish(out, paint)?;
            }
            Primitive::Circle {
                center,
                radius,
                paint,
                ..
            } => {
                pgf_paint(out, paint)?;
                w!(
                    out,
                    "\\pgfpathcircle{{\\pgfpoint{{{}bp}}{{{}bp}}}}{{{radius}bp}}",
                    center.x,
                    -center.y
                );
                pgf_finish(out, paint)?;
            }
            Primitive::Path {
                commands, paint, ..
            } => {
                pgf_paint(out, paint)?;
                pgf_path(out, commands)?;
                pgf_finish(out, paint)?;
            }
            Primitive::Group {
                children,
                translate,
                clip,
                opacity,
                ..
            } => {
                w!(
                    out,
                    "\\pgftransformshift{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
                    translate.x,
                    -translate.y
                );
                if let Some(clip) = clip {
                    match &scene.figure().resources.clips[clip] {
                        Clip::Rect { bounds } => pgf_rect(out, bounds, 0.)?,
                        Clip::Path { commands } => pgf_path(out, commands)?,
                    }
                    w!(out, "\\pgfusepath{{clip}}");
                }
                if *opacity < 1. {
                    // A transparency group composites its contents as one, at the opacity in
                    // force where it begins.
                    w!(
                        out,
                        "\\pgfsetfillopacity{{{}}}\\pgfsetstrokeopacity{{{}}}\\begin{{pgftransparencygroup}}\n",
                        opacity,
                        opacity
                    );
                    pgf_nodes(out, children, scene)?;
                    w!(out, "\\end{{pgftransparencygroup}}\n");
                } else {
                    pgf_nodes(out, children, scene)?;
                }
            }
            Primitive::Use {
                symbol, position, ..
            } => {
                w!(
                    out,
                    "\\pgftransformshift{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
                    position.x,
                    -position.y
                );
                for shape in &scene.figure().resources.symbols[symbol].shapes {
                    pgf_paint(out, &shape.paint)?;
                    pgf_path(out, &shape.commands)?;
                    pgf_finish(out, &shape.paint)?;
                }
            }
            Primitive::Text { placed: t } => {
                let native = matches!(t.text.request.content, TextContent::Native { .. });
                // Plain text baseline uses measured ascent; native saved boxes use their host-owned top.
                let y = if native {
                    t.position.y
                } else {
                    t.position.y + t.text.metrics.baseline.expect("capability checked")
                };
                w!(
                    out,
                    "\\pgftext[left,{},at={{\\pgfpoint{{{}bp}}{{{}bp}}}}]{{",
                    if native { "top" } else { "base" },
                    anchor_x(t),
                    -y
                );
                if let TextPaint::Resolved { fill } = &t.text.paint {
                    let (r, g, b) = rgb(fill);
                    w!(out, "\\color[RGB]{{{r},{g},{b}}}");
                }
                match &t.text.request.content {
                    TextContent::Native { key, .. } => w!(out, "\\figkitUseNative{{{key}}}"),
                    TextContent::Plain { text } => {
                        let style = &t.text.request.typography;
                        w!(
                            out,
                            "\\fontfamily{{{}}}\\fontsize{{{}bp}}{{{}bp}}\\fontseries{{{}}}\\fontshape{{{}}}\\selectfont ",
                            style.font_family,
                            style.font_size,
                            style.font_size,
                            if style.weight == "bold" { "b" } else { "m" },
                            if style.font_style == "italic" {
                                "it"
                            } else {
                                "n"
                            }
                        );
                        tex(out, text)?;
                    }
                }
                w!(out, "}}\n");
            }
        }
        w!(out, "\\end{{pgfscope}}\n");
    }
    Ok(())
}
fn pgf(out: &mut Output, scene: &PreparedScene) -> FigureResult<()> {
    let f = scene.figure();
    w!(
        out,
        "% figkit prepared scene v2; native keys refer to host-owned saved boxes\n\\begin{{pgfpicture}}\n\\pgfpathrectangle{{\\pgfpoint{{0bp}}{{0bp}}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}\\pgfusepath{{use as bounding box}}\n\\pgftransformshift{{\\pgfpoint{{0bp}}{{{}bp}}}}\n",
        f.size.width,
        f.size.height,
        f.size.height
    );
    pgf_paint(
        out,
        &Paint {
            fill: Some(f.background.clone()),
            stroke: None,
            stroke_width: 0.,
            opacity: 1.,
        },
    )?;
    pgf_rect(
        out,
        &Bounds {
            x: 0.,
            y: 0.,
            width: f.size.width,
            height: f.size.height,
        },
        0.,
    )?;
    w!(out, "\\pgfusepath{{fill}}\n");
    pgf_nodes(out, &f.primitives, scene)?;
    w!(out, "\\end{{pgfpicture}}\n");
    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Layer<'a> {
    Svg {
        svg: &'a str,
    },
    Text {
        x: f64,
        y: f64,
        request: &'a TextRequest,
        metrics: &'a BlockMetrics,
        paint: &'a TextPaint,
        clip: Option<Bounds>,
    },
}
fn layer(out: &mut Output, value: &Layer<'_>, first: &mut bool) -> FigureResult<()> {
    if !*first {
        w!(out, ",");
    }
    *first = false;
    serde_json::to_writer(out, value)
        .map_err(|_| FigureError::new(ErrorKind::BudgetExceeded, "output byte limit exceeded"))
}
fn intersect(a: Option<Bounds>, b: Bounds) -> Bounds {
    if let Some(a) = a {
        let x = a.x.max(b.x);
        let y = a.y.max(b.y);
        Bounds {
            x,
            y,
            width: ((a.x + a.width).min(b.x + b.width) - x).max(0.),
            height: ((a.y + a.height).min(b.y + b.height) - y).max(0.),
        }
    } else {
        b
    }
}
fn typst_nodes(
    out: &mut Output,
    nodes: &[Primitive],
    scene: &PreparedScene,
    offset: Point,
    clip: Option<Bounds>,
    first: &mut bool,
) -> FigureResult<()> {
    for node in nodes {
        match node {
            Primitive::Group {
                children,
                translate,
                clip: local,
                ..
            } => {
                let offset = Point {
                    x: offset.x + translate.x,
                    y: offset.y + translate.y,
                };
                let clip = if let Some(id) = local {
                    let Clip::Rect { bounds } = &scene.figure().resources.clips[id] else {
                        unreachable!("capability checked")
                    };
                    Some(intersect(
                        clip.clone(),
                        Bounds {
                            x: bounds.x + offset.x,
                            y: bounds.y + offset.y,
                            ..bounds.clone()
                        },
                    ))
                } else {
                    clip.clone()
                };
                typst_nodes(out, children, scene, offset, clip, first)?;
            }
            Primitive::Text { placed: t } => layer(
                out,
                &Layer::Text {
                    x: anchor_x(t) + offset.x,
                    y: t.position.y + offset.y,
                    request: &t.text.request,
                    metrics: &t.text.metrics,
                    paint: &t.text.paint,
                    clip: clip.clone(),
                },
                first,
            )?,
            _ => {
                // Every geometry fragment is bounded too; no unbounded intermediate SVG allocation.
                let mut fragment = Output::new(out.limit - out.value.len());
                svg_start(&mut fragment, scene)?;
                if let Some(c) = &clip {
                    w!(
                        fragment,
                        "<clipPath id=\"figkit-host-clip\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",
                        c.x,
                        c.y,
                        c.width,
                        c.height
                    );
                }
                w!(fragment, "</defs>");
                if clip.is_some() {
                    w!(fragment, "<g clip-path=\"url(#figkit-host-clip)\">");
                }
                w!(
                    fragment,
                    "<g transform=\"translate({} {})\">",
                    offset.x,
                    offset.y
                );
                svg_nodes(&mut fragment, std::slice::from_ref(node), scene, false)?;
                w!(fragment, "</g>");
                if clip.is_some() {
                    w!(fragment, "</g>");
                }
                w!(fragment, "</svg>");
                layer(
                    out,
                    &Layer::Svg {
                        svg: &fragment.value,
                    },
                    first,
                )?;
            }
        }
    }
    Ok(())
}
fn typst(out: &mut Output, scene: &PreparedScene) -> FigureResult<()> {
    let f = scene.figure();
    w!(
        out,
        "{{\"version\":2,\"width\":{},\"height\":{},\"layers\":[",
        f.size.width,
        f.size.height
    );
    let mut first = true;
    let background = Primitive::Rect {
        bounds: Bounds {
            x: 0.,
            y: 0.,
            width: f.size.width,
            height: f.size.height,
        },
        radius: 0.,
        paint: Paint {
            fill: Some(f.background.clone()),
            stroke: None,
            stroke_width: 0.,
            opacity: 1.,
        },
        metadata: ElementMetadata::default(),
    };
    typst_nodes(
        out,
        &[background],
        scene,
        Point { x: 0., y: 0. },
        None,
        &mut first,
    )?;
    typst_nodes(
        out,
        &f.primitives,
        scene,
        Point { x: 0., y: 0. },
        None,
        &mut first,
    )?;
    w!(out, "]}}");
    Ok(())
}
/// Render after complete capability validation; output growth stops at the configured byte limit.
pub fn render(scene: &PreparedScene, backend: Backend) -> FigureResult<String> {
    validate_capabilities(scene, backend)?;
    let mut out = Output::new(scene.limits().max_output_bytes);
    match backend {
        Backend::Svg => svg(&mut out, scene, false)?,
        Backend::CompatibilitySvg => svg(&mut out, scene, true)?,
        Backend::Pgf => pgf(&mut out, scene)?,
        Backend::Typst | Backend::Browser => typst(&mut out, scene)?,
    }
    Ok(out.value)
}
