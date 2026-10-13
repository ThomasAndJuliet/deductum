//! Stateless scene renderers. Coordinates are 1/72-inch units, top-left origin.
use figkit_model::legacy::{
    Bounds, CubicSegment, Error, Metadata, Paint, PathCommand, Point, Primitive, Result, Scene,
    TextContent, TextMetrics, TextStyle,
};
use serde::Serialize;
use std::fmt::Write;

pub trait Renderer {
    fn render(&self, scene: &Scene) -> Result<String>;
}
#[derive(Default)]
pub struct SvgRenderer;
/// The original's house source format for bake.py (figkit/emit.py): the scene's
/// comment, title and description, `style#house`, `defs#house-defs`,
/// `defs#chart-defs` with the scene's definitions, and its groups, classes,
/// ids and `data-*` identities, with Typst label placeholders placed by baseline.
#[derive(Default)]
pub struct CompatibilitySvgRenderer;
#[derive(Default)]
pub struct PgfRenderer;

#[path = "compat.rs"]
mod compat;
#[path = "marks.rs"]
pub mod marks;
#[path = "typst.rs"]
pub mod typst;
#[derive(Default)]
pub struct TypstRenderer;

fn err(message: impl Into<String>) -> Error {
    Error::new("render", message)
}
fn finite(x: f64) -> Result<f64> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(err("non-finite coordinate or dimension"))
    }
}
fn positive(x: f64) -> Result<f64> {
    if finite(x)? >= 0. {
        Ok(x)
    } else {
        Err(err("negative dimension"))
    }
}
fn fmt(x: f64) -> Result<String> {
    Ok(format!("{:.6}", finite(x)?))
}
fn svg_path_data(commands: &[PathCommand]) -> Result<String> {
    if !matches!(commands.first(), Some(PathCommand::MoveTo { .. })) {
        return Err(err("path data must start with a move"));
    }
    let mut d = vec![];
    for c in commands {
        d.push(match c {
            PathCommand::MoveTo { to } => format!("M{} {}", fmt(to.x)?, fmt(to.y)?),
            PathCommand::LineTo { to } => format!("L{} {}", fmt(to.x)?, fmt(to.y)?),
            PathCommand::CubicTo { c1, c2, to } => format!(
                "C{} {} {} {} {} {}",
                fmt(c1.x)?,
                fmt(c1.y)?,
                fmt(c2.x)?,
                fmt(c2.y)?,
                fmt(to.x)?,
                fmt(to.y)?
            ),
            PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to,
            } => format!(
                "A{} {} {} {} {} {} {}",
                fmt(positive(*rx)?)?,
                fmt(positive(*ry)?)?,
                fmt(*rotation)?,
                u8::from(*large_arc),
                u8::from(*sweep),
                fmt(to.x)?,
                fmt(to.y)?
            ),
            PathCommand::Close => "Z".into(),
        });
    }
    Ok(d.join(" "))
}
/// A curve's shaft and its closed head outline as path commands.
fn curve_commands(
    start: &Point,
    segments: &[CubicSegment],
    head: Option<&[Point]>,
) -> (Vec<PathCommand>, Option<Vec<PathCommand>>) {
    let mut shaft = vec![PathCommand::MoveTo { to: start.clone() }];
    shaft.extend(segments.iter().map(|s| PathCommand::CubicTo {
        c1: s.c1.clone(),
        c2: s.c2.clone(),
        to: s.end.clone(),
    }));
    let outline = head.filter(|h| !h.is_empty()).map(|h| {
        let mut o = vec![PathCommand::MoveTo { to: h[0].clone() }];
        o.extend(h[1..].iter().map(|p| PathCommand::LineTo { to: p.clone() }));
        o.push(PathCommand::Close);
        o
    });
    (shaft, outline)
}
/// A curve's head is filled in the shaft's colour.
fn head_paint(paint: &Paint) -> Result<Paint> {
    let ink = paint
        .stroke
        .clone()
        .ok_or_else(|| err("a curve with a head needs a stroke colour"))?;
    Ok(Paint {
        fill: Some(ink),
        stroke: None,
        stroke_width: 0.,
        opacity: paint.opacity,
    })
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn text_style_svg(style: &TextStyle) -> Result<String> {
    if !matches!(style.weight.as_str(), "normal" | "bold")
        || !matches!(style.font_style.as_str(), "normal" | "italic")
    {
        return Err(err("unsupported text weight or font style"));
    }
    Ok(format!(
        " font-weight=\"{}\" font-style=\"{}\"{}",
        style.weight,
        style.font_style,
        style
            .fill
            .as_ref()
            .map_or(String::new(), |fill| format!(" fill=\"{}\"", xml(fill)))
    ))
}
/// Plain text as Typst markup that typesets to exactly that text (figkit `typst.escape`): the
/// one implementation (`typst::escape`), exposed to hosts through the protocol.
pub fn typst_escape(s: &str) -> String {
    typst::escape(s)
}
fn tex(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        o.push_str(match c {
            '\\' => "\\textbackslash{}",
            '{' => "\\{",
            '}' => "\\}",
            '%' => "\\%",
            '&' => "\\&",
            '#' => "\\#",
            '$' => "\\$",
            '_' => "\\_",
            '^' => "\\textasciicircum{}",
            '~' => "\\textasciitilde{}",
            '−' => "\\ensuremath{-}",
            '–' => "--",
            '—' => "---",
            _ => {
                o.push(c);
                continue;
            }
        });
    }
    o
}
/// How PGF output sets a label: its size, weight, slant and ink, then its content, or a
/// top-aligned paragraph `max_width` wide when the line is wider (plain and native content
/// alike). A TeX host measures a label by setting exactly this, without the ink, so that
/// measured and rendered text agree.
pub fn pgf_label(
    content: &TextContent,
    font_size: f64,
    style: &TextStyle,
    max_width: Option<f64>,
) -> Result<String> {
    let content = match content {
        TextContent::Plain { text } => tex(text),
        TextContent::Native { host, key, .. } if host == "latex" || host == "tex" => {
            if !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':')
            {
                return Err(err("unsafe LaTeX native key"));
            }
            format!("\\chartgenUseNative{{{key}}}")
        }
        TextContent::Native {
            alternative: Some(text),
            ..
        } => tex(text),
        TextContent::Native { host, .. } => {
            return Err(err(format!(
                "native text host {host} cannot render in LaTeX"
            )));
        }
    };
    let styled = format!(
        "\\fontsize{{{}bp}}{{{}bp}}\\selectfont {}{}{} {content}",
        fmt(font_size)?,
        fmt(font_size * 1.2)?,
        if style.weight == "bold" {
            "\\bfseries "
        } else if style.weight == "normal" {
            ""
        } else {
            return Err(err("unsupported TeX font weight"));
        },
        if style.font_style == "italic" {
            "\\itshape "
        } else if style.font_style == "normal" {
            ""
        } else {
            return Err(err("unsupported TeX font style"));
        },
        style
            .fill
            .as_ref()
            .map_or(Ok(String::new()), |c| tex_color(c))?,
    );
    Ok(match max_width {
        Some(max) => {
            positive(max)?;
            format!(
                "\\setbox0=\\hbox{{{styled}}}\\ifdim\\wd0>{}bp \\parbox[t]{{{}bp}}{{{styled}}}\\else\\box0\\fi",
                fmt(max)?,
                fmt(max)?
            )
        }
        None => styled,
    })
}

fn single_line_svg(s: &str) -> Result<()> {
    if s.contains('\n') || s.contains('\r') {
        return Err(err("multiline SVG text needs a measured line layout"));
    }
    Ok(())
}
fn anchor_offset(anchor: &str, m: &TextMetrics) -> Result<f64> {
    positive(m.width)?;
    positive(m.height)?;
    Ok(match anchor {
        "start" => 0.,
        "middle" => m.width / 2.,
        "end" => m.width,
        _ => return Err(err(format!("unsupported text anchor: {anchor}"))),
    })
}
fn check_scene(s: &Scene) -> Result<()> {
    if s.version != 1 {
        return Err(err(format!("unsupported scene version: {}", s.version)));
    }
    positive(s.width)?;
    positive(s.height)?;
    if s.width == 0. || s.height == 0. {
        return Err(err("scene dimensions must be positive"));
    }
    Ok(())
}
fn check_paint(p: &Paint) -> Result<()> {
    positive(p.stroke_width)?;
    if !p.opacity.is_finite() || !(0. ..=1.).contains(&p.opacity) {
        return Err(err("opacity must be between zero and one"));
    }
    Ok(())
}
fn paint_svg(p: &Paint) -> Result<String> {
    check_paint(p)?;
    Ok(format!(
        " fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" opacity=\"{}\"",
        xml(p.fill.as_deref().unwrap_or("none")),
        xml(p.stroke.as_deref().unwrap_or("none")),
        fmt(p.stroke_width)?,
        fmt(p.opacity)?
    ))
}
fn meta_svg(m: &Metadata) -> String {
    let mut out = String::new();
    if let Some(id) = &m.id {
        write!(out, " id=\"{}\"", xml(id)).unwrap();
    }
    if !m.classes.is_empty() {
        write!(out, " class=\"{}\"", xml(&m.classes.join(" "))).unwrap();
    }
    for (k, v) in &m.data {
        if k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            write!(out, " data-{}=\"{}\"", k, xml(v)).unwrap();
        }
    }
    out
}
fn svg_primitive(
    p: &Primitive,
    out: &mut String,
    defs: &mut String,
    next: &mut usize,
) -> Result<()> {
    match p {
        Primitive::Rect {
            bounds: b,
            paint,
            meta,
        } => {
            positive(b.width)?;
            positive(b.height)?;
            writeln!(
                out,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{} />",
                fmt(b.x)?,
                fmt(b.y)?,
                fmt(b.width)?,
                fmt(b.height)?,
                paint_svg(paint)?,
                meta_svg(meta)
            )
            .unwrap();
        }
        Primitive::Line {
            from,
            to,
            paint,
            meta,
        } => {
            writeln!(
                out,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}{} />",
                fmt(from.x)?,
                fmt(from.y)?,
                fmt(to.x)?,
                fmt(to.y)?,
                paint_svg(paint)?,
                meta_svg(meta)
            )
            .unwrap();
        }
        Primitive::Path {
            points,
            closed,
            paint,
            meta,
        } => {
            if points.is_empty() {
                return Err(err("path has no points"));
            }
            let mut d = String::new();
            for (i, p) in points.iter().enumerate() {
                write!(
                    d,
                    "{}{} {} ",
                    if i == 0 { "M" } else { "L" },
                    fmt(p.x)?,
                    fmt(p.y)?
                )
                .unwrap();
            }
            if *closed {
                d.push('Z');
            }
            writeln!(
                out,
                "<path d=\"{}\"{}{} />",
                d,
                paint_svg(paint)?,
                meta_svg(meta)
            )
            .unwrap();
        }
        Primitive::Circle {
            center,
            radius,
            paint,
            meta,
        } => {
            positive(*radius)?;
            writeln!(
                out,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}{} />",
                fmt(center.x)?,
                fmt(center.y)?,
                fmt(*radius)?,
                paint_svg(paint)?,
                meta_svg(meta)
            )
            .unwrap();
        }
        Primitive::Shape {
            commands,
            paint,
            meta,
        } => {
            writeln!(
                out,
                "<path d=\"{}\"{}{} />",
                svg_path_data(commands)?,
                paint_svg(paint)?,
                meta_svg(meta)
            )
            .unwrap();
        }
        Primitive::Curve {
            start,
            segments,
            head,
            paint,
            meta,
        } => {
            let (shaft, outline) = curve_commands(start, segments, head.as_deref());
            writeln!(
                out,
                "<path d=\"{}\"{}{} />",
                svg_path_data(&shaft)?,
                paint_svg(&Paint {
                    fill: None,
                    ..paint.clone()
                })?,
                meta_svg(meta)
            )
            .unwrap();
            if let Some(outline) = outline {
                writeln!(
                    out,
                    "<path d=\"{}\"{} />",
                    svg_path_data(&outline)?,
                    paint_svg(&head_paint(paint)?)?
                )
                .unwrap();
            }
        }
        Primitive::Use { .. } | Primitive::Wedge { .. } => {
            for q in marks::lower(p)?.unwrap_or_default() {
                svg_primitive(&q, out, defs, next)?;
            }
        }
        Primitive::Text {
            position,
            request,
            metrics,
            anchor,
            meta,
        } => {
            if let TextContent::Plain { text } = &request.content {
                single_line_svg(text)?;
            }
            let x = position.x - anchor_offset(anchor, metrics)?;
            let baseline = metrics.baseline.unwrap_or(metrics.height);
            positive(baseline)?;
            let value = match &request.content {
                TextContent::Plain { text } => text,
                TextContent::Native {
                    alternative: Some(text),
                    ..
                } => text,
                TextContent::Native {
                    host,
                    key,
                    alternative: None,
                } => {
                    return Err(err(format!(
                        "{host} native text {key} has no SVG alternative"
                    )));
                }
            };
            single_line_svg(value)?;
            positive(request.font_size)?;
            writeln!(out,"<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" dominant-baseline=\"alphabetic\"{}{}>{}</text>",fmt(x)?,fmt(position.y+baseline)?,xml(&request.font_family),fmt(request.font_size)?,text_style_svg(&request.style)?,meta_svg(meta),xml(value)).unwrap();
        }
        Primitive::Group {
            children,
            translate,
            clip,
            meta,
        } => {
            let clip_id = if let Some(c) = clip {
                positive(c.width)?;
                positive(c.height)?;
                let id = format!("chartgen-clip-{}", *next);
                *next += 1;
                writeln!(defs,"<clipPath id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" /></clipPath>",id,fmt(c.x)?,fmt(c.y)?,fmt(c.width)?,fmt(c.height)?).unwrap();
                Some(id)
            } else {
                None
            };
            write!(
                out,
                "<g transform=\"translate({} {})\"{}",
                fmt(translate.x)?,
                fmt(translate.y)?,
                meta_svg(meta)
            )
            .unwrap();
            if let Some(id) = clip_id {
                write!(out, " clip-path=\"url(#{id})\"").unwrap();
            }
            out.push_str(">\n");
            for child in children {
                svg_primitive(child, out, defs, next)?;
            }
            out.push_str("</g>\n");
        }
    }
    Ok(())
}
fn svg(s: &Scene) -> Result<String> {
    check_scene(s)?;
    let mut body = String::new();
    let mut defs = String::new();
    let mut next = 0;
    for p in &s.primitives {
        svg_primitive(p, &mut body, &mut defs, &mut next)?;
    }
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}pt\" height=\"{}pt\" viewBox=\"0 0 {} {}\" role=\"img\">\n",
        fmt(s.width)?,
        fmt(s.height)?,
        fmt(s.width)?,
        fmt(s.height)?,
    );
    if let Some(v) = &s.title {
        writeln!(out, "<title>{}</title>", xml(v)).unwrap();
    }
    if let Some(v) = &s.description {
        writeln!(out, "<desc>{}</desc>", xml(v)).unwrap();
    }
    // the scene's own definitions (patterns of textures, masks) beside the clip paths
    for d in &s.defs {
        defs.push_str(d);
    }
    if !defs.is_empty() {
        write!(out, "<defs>{defs}</defs>\n").unwrap();
    }
    writeln!(
        out,
        "<rect width=\"{}\" height=\"{}\" fill=\"{}\" />",
        fmt(s.width)?,
        fmt(s.height)?,
        xml(&s.background)
    )
    .unwrap();
    out.push_str(&body);
    out.push_str("</svg>\n");
    Ok(out)
}
impl Renderer for SvgRenderer {
    fn render(&self, s: &Scene) -> Result<String> {
        svg(s)
    }
}
impl Renderer for CompatibilitySvgRenderer {
    fn render(&self, s: &Scene) -> Result<String> {
        compat::svg(s)
    }
}

fn pgf_color(s: &str) -> Result<String> {
    let s = s.trim();
    let hex = if s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        s[1..].to_owned()
    } else {
        match s.to_ascii_lowercase().as_str() {
            "black" => "000000".into(),
            "white" => "FFFFFF".into(),
            "red" => "FF0000".into(),
            "green" => "008000".into(),
            "blue" => "0000FF".into(),
            "gray" | "grey" => "808080".into(),
            _ => return Err(err(format!("unsupported PGF color: {s}"))),
        }
    };
    Ok(format!(
        "{{rgb,255:red,{};green,{};blue,{}}}",
        u8::from_str_radix(&hex[0..2], 16).unwrap(),
        u8::from_str_radix(&hex[2..4], 16).unwrap(),
        u8::from_str_radix(&hex[4..6], 16).unwrap()
    ))
}
fn tex_color(s: &str) -> Result<String> {
    let s = s.trim();
    let hex = if s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        s[1..].to_owned()
    } else {
        match s.to_ascii_lowercase().as_str() {
            "black" => "000000".into(),
            "white" => "FFFFFF".into(),
            "red" => "FF0000".into(),
            "green" => "008000".into(),
            "blue" => "0000FF".into(),
            "gray" | "grey" => "808080".into(),
            _ => return Err(err(format!("unsupported TeX text color: {s}"))),
        }
    };
    Ok(format!(
        "\\color[RGB]{{{},{},{}}} ",
        u8::from_str_radix(&hex[0..2], 16).unwrap(),
        u8::from_str_radix(&hex[2..4], 16).unwrap(),
        u8::from_str_radix(&hex[4..6], 16).unwrap()
    ))
}
fn pgf_paint(p: &Paint, out: &mut String) -> Result<()> {
    check_paint(p)?;
    writeln!(out, "\\pgfsetlinewidth{{{}bp}}", fmt(p.stroke_width)?).unwrap();
    writeln!(
        out,
        "\\pgfsetfillopacity{{{}}}\\pgfsetstrokeopacity{{{}}}",
        fmt(p.opacity)?,
        fmt(p.opacity)?
    )
    .unwrap();
    if let Some(c) = &p.fill {
        writeln!(out, "\\pgfsetfillcolor{}", pgf_color(c)?).unwrap();
    }
    if let Some(c) = &p.stroke {
        writeln!(out, "\\pgfsetstrokecolor{}", pgf_color(c)?).unwrap();
    }
    Ok(())
}
fn pgf_finish(p: &Paint, out: &mut String) {
    out.push_str(match (p.fill.is_some(), p.stroke.is_some()) {
        (true, true) => "\\pgfusepath{fill,stroke}\n",
        (true, false) => "\\pgfusepath{fill}\n",
        (false, true) => "\\pgfusepath{stroke}\n",
        (false, false) => "\\pgfusepath{discard}\n",
    });
}
fn pgf_primitive(p: &Primitive, out: &mut String) -> Result<()> {
    match p {
        Primitive::Rect {
            bounds: b, paint, ..
        } => {
            positive(b.width)?;
            positive(b.height)?;
            out.push_str("\\begin{pgfscope}\n");
            pgf_paint(paint, out)?;
            writeln!(
                out,
                "\\pgfpathrectangle{{\\pgfpoint{{{}bp}}{{{}bp}}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
                fmt(b.x)?,
                fmt(-b.y - b.height)?,
                fmt(b.width)?,
                fmt(b.height)?
            )
            .unwrap();
            pgf_finish(paint, out);
            out.push_str("\\end{pgfscope}\n");
        }
        Primitive::Line {
            from, to, paint, ..
        } => {
            out.push_str("\\begin{pgfscope}\n");
            pgf_paint(paint, out)?;
            writeln!(
                out,
                "\\pgfpathmoveto{{\\pgfpoint{} }}\\pgfpathlineto{{\\pgfpoint{} }}",
                pgf_point_args(from)?,
                pgf_point_args(to)?
            )
            .unwrap();
            pgf_finish(paint, out);
            out.push_str("\\end{pgfscope}\n");
        }
        Primitive::Path {
            points,
            closed,
            paint,
            ..
        } => {
            if points.is_empty() {
                return Err(err("path has no points"));
            }
            out.push_str("\\begin{pgfscope}\n");
            pgf_paint(paint, out)?;
            writeln!(
                out,
                "\\pgfpathmoveto{{\\pgfpoint{} }}",
                pgf_point_args(&points[0])?
            )
            .unwrap();
            for p in &points[1..] {
                writeln!(out, "\\pgfpathlineto{{\\pgfpoint{} }}", pgf_point_args(p)?).unwrap();
            }
            if *closed {
                out.push_str("\\pgfpathclose\n");
            }
            pgf_finish(paint, out);
            out.push_str("\\end{pgfscope}\n");
        }
        Primitive::Shape {
            commands, paint, ..
        } => {
            out.push_str("\\begin{pgfscope}\n");
            pgf_paint(paint, out)?;
            pgf_path(commands, out)?;
            pgf_finish(paint, out);
            out.push_str("\\end{pgfscope}\n");
        }
        Primitive::Curve {
            start,
            segments,
            head,
            paint,
            ..
        } => {
            let (shaft, outline) = curve_commands(start, segments, head.as_deref());
            let stroke = Paint {
                fill: None,
                ..paint.clone()
            };
            out.push_str("\\begin{pgfscope}\n");
            pgf_paint(&stroke, out)?;
            pgf_path(&shaft, out)?;
            pgf_finish(&stroke, out);
            out.push_str("\\end{pgfscope}\n");
            if let Some(outline) = outline {
                let fill = head_paint(paint)?;
                out.push_str("\\begin{pgfscope}\n");
                pgf_paint(&fill, out)?;
                pgf_path(&outline, out)?;
                pgf_finish(&fill, out);
                out.push_str("\\end{pgfscope}\n");
            }
        }
        Primitive::Use { .. } | Primitive::Wedge { .. } => {
            for q in marks::lower(p)?.unwrap_or_default() {
                pgf_primitive(&q, out)?;
            }
        }
        Primitive::Circle {
            center,
            radius,
            paint,
            ..
        } => {
            positive(*radius)?;
            out.push_str("\\begin{pgfscope}\n");
            pgf_paint(paint, out)?;
            writeln!(
                out,
                "\\pgfpathcircle{{\\pgfpoint{} }}{{{}bp}}",
                pgf_point_args(center)?,
                fmt(*radius)?
            )
            .unwrap();
            pgf_finish(paint, out);
            out.push_str("\\end{pgfscope}\n");
        }
        Primitive::Text {
            position,
            request,
            metrics,
            anchor,
            ..
        } => {
            let x = position.x - anchor_offset(anchor, metrics)?;
            let content = pgf_label(
                &request.content,
                request.font_size,
                &request.style,
                request.max_width,
            )?;
            writeln!(
                out,
                "\\pgftext[left,top,at={{\\pgfpoint{{{}bp}}{{{}bp}}}}]{{{content}}}",
                fmt(x)?,
                fmt(-position.y)?
            )
            .unwrap();
        }
        Primitive::Group {
            children,
            translate,
            clip,
            ..
        } => {
            out.push_str("\\begin{pgfscope}\n");
            writeln!(
                out,
                "\\pgftransformshift{{\\pgfpoint{{{}bp}}{{{}bp}}}}",
                fmt(translate.x)?,
                fmt(-translate.y)?
            )
            .unwrap();
            if let Some(c) = clip {
                positive(c.width)?;
                positive(c.height)?;
                writeln!(out,"\\pgfpathrectangle{{\\pgfpoint{{{}bp}}{{{}bp}}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}\\pgfusepath{{clip}}",fmt(c.x)?,fmt(-c.y-c.height)?,fmt(c.width)?,fmt(c.height)?).unwrap();
            }
            for p in children {
                pgf_primitive(p, out)?;
            }
            out.push_str("\\end{pgfscope}\n");
        }
    }
    Ok(())
}
fn pgf_path(commands: &[PathCommand], out: &mut String) -> Result<()> {
    if !matches!(commands.first(), Some(PathCommand::MoveTo { .. })) {
        return Err(err("path data must start with a move"));
    }
    for c in commands {
        match c {
            PathCommand::MoveTo { to } => {
                writeln!(out, "\\pgfpathmoveto{{\\pgfpoint{} }}", pgf_point_args(to)?).unwrap()
            }
            PathCommand::LineTo { to } => {
                writeln!(out, "\\pgfpathlineto{{\\pgfpoint{} }}", pgf_point_args(to)?).unwrap()
            }
            PathCommand::CubicTo { c1, c2, to } => writeln!(
                out,
                "\\pgfpathcurveto{{\\pgfpoint{} }}{{\\pgfpoint{} }}{{\\pgfpoint{} }}",
                pgf_point_args(c1)?,
                pgf_point_args(c2)?,
                pgf_point_args(to)?
            )
            .unwrap(),
            PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to,
            } => writeln!(
                // SVG's y axis points down: the PGF arc turns the other way (sweep inverted).
                out,
                "\\pgfpatharcto{{{}bp}}{{{}bp}}{{{}}}{{{}}}{{{}}}{{\\pgfpoint{} }}",
                fmt(positive(*rx)?)?,
                fmt(positive(*ry)?)?,
                fmt(-rotation)?,
                u8::from(*large_arc),
                u8::from(!*sweep),
                pgf_point_args(to)?
            )
            .unwrap(),
            PathCommand::Close => out.push_str("\\pgfpathclose\n"),
        }
    }
    Ok(())
}
fn pgf_point_args(p: &Point) -> Result<String> {
    Ok(format!("{{{}bp}}{{{}bp}}", fmt(p.x)?, fmt(-p.y)?))
}
impl Renderer for PgfRenderer {
    fn render(&self, s: &Scene) -> Result<String> {
        check_scene(s)?;
        let mut o = String::from(
            "% chartgen PGF scene; coordinates converted from top-left pt to TeX bp\n\\begin{pgfpicture}\n",
        );
        writeln!(o,"\\pgfpathrectangle{{\\pgfpoint{{0bp}}{{0bp}}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}\\pgfusepath{{use as bounding box}}",fmt(s.width)?,fmt(s.height)?).unwrap();
        writeln!(o,"\\pgfsetfillcolor{}\\pgfpathrectangle{{\\pgfpoint{{0bp}}{{0bp}}}}{{\\pgfpoint{{{}bp}}{{{}bp}}}}\\pgfusepath{{fill}}",pgf_color(&s.background)?,fmt(s.width)?,fmt(s.height)?).unwrap();
        writeln!(
            o,
            "\\pgftransformshift{{\\pgfpoint{{0bp}}{{{}bp}}}}",
            fmt(s.height)?
        )
        .unwrap();
        for p in &s.primitives {
            pgf_primitive(p, &mut o)?;
        }
        o.push_str("\\end{pgfpicture}\n");
        Ok(o)
    }
}

#[derive(Serialize)]
pub struct TypstPlan {
    pub version: u32,
    pub width: f64,
    pub height: f64,
    pub layers: Vec<TypstLayer>,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypstLayer {
    Svg {
        svg: String,
    },
    Text {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        content: TextContent,
        font_family: String,
        font_size: f64,
        style: TextStyle,
        clip: Option<Bounds>,
    },
}
fn intersect(a: Option<Bounds>, b: Bounds) -> Option<Bounds> {
    match a {
        None => Some(b),
        Some(a) => {
            let x = a.x.max(b.x);
            let y = a.y.max(b.y);
            let r = (a.x + a.width).min(b.x + b.width);
            let d = (a.y + a.height).min(b.y + b.height);
            Some(Bounds {
                x,
                y,
                width: (r - x).max(0.),
                height: (d - y).max(0.),
            })
        }
    }
}
fn shift(b: &Bounds, p: &Point) -> Bounds {
    Bounds {
        x: b.x + p.x,
        y: b.y + p.y,
        width: b.width,
        height: b.height,
    }
}
fn moved(p: &Primitive, dx: f64, dy: f64) -> Primitive {
    let mut x = p.clone();
    match &mut x {
        Primitive::Rect { bounds, .. } => {
            bounds.x += dx;
            bounds.y += dy
        }
        Primitive::Line { from, to, .. } => {
            from.x += dx;
            from.y += dy;
            to.x += dx;
            to.y += dy
        }
        Primitive::Path { points, .. } => {
            for p in points {
                p.x += dx;
                p.y += dy
            }
        }
        Primitive::Circle { center, .. } => {
            center.x += dx;
            center.y += dy
        }
        Primitive::Text { position, .. } => {
            position.x += dx;
            position.y += dy
        }
        Primitive::Group { translate, .. } => {
            translate.x += dx;
            translate.y += dy
        }
        Primitive::Use { center, .. } | Primitive::Wedge { center, .. } => {
            center.x += dx;
            center.y += dy
        }
        Primitive::Curve {
            start,
            segments,
            head,
            ..
        } => {
            let shift = |p: &mut Point| {
                p.x += dx;
                p.y += dy
            };
            shift(start);
            for s in segments {
                shift(&mut s.c1);
                shift(&mut s.c2);
                shift(&mut s.end);
            }
            for p in head.iter_mut().flatten() {
                shift(p);
            }
        }
        Primitive::Shape { commands, .. } => {
            for c in commands {
                match c {
                    PathCommand::MoveTo { to } | PathCommand::LineTo { to } => {
                        to.x += dx;
                        to.y += dy
                    }
                    PathCommand::CubicTo { c1, c2, to } => {
                        for p in [c1, c2, to] {
                            p.x += dx;
                            p.y += dy
                        }
                    }
                    PathCommand::ArcTo { to, .. } => {
                        to.x += dx;
                        to.y += dy
                    }
                    PathCommand::Close => {}
                }
            }
        }
    }
    x
}
fn typst_layers(
    s: &Scene,
    p: &Primitive,
    offset: Point,
    clip: Option<Bounds>,
    layers: &mut Vec<TypstLayer>,
) -> Result<()> {
    match p {
        Primitive::Group {
            children,
            translate,
            clip: local,
            ..
        } => {
            let new = Point {
                x: offset.x + translate.x,
                y: offset.y + translate.y,
            };
            let clip = if let Some(c) = local {
                positive(c.width)?;
                positive(c.height)?;
                intersect(clip, shift(c, &new))
            } else {
                clip
            };
            for child in children {
                typst_layers(s, child, new.clone(), clip.clone(), layers)?;
            }
        }
        Primitive::Text {
            position,
            request,
            metrics,
            anchor,
            ..
        } => {
            let x = position.x + offset.x - anchor_offset(anchor, metrics)?;
            let y = position.y + offset.y;
            positive(request.font_size)?;
            let content = match &request.content {
                TextContent::Native {
                    host,
                    alternative: Some(text),
                    ..
                } if host != "typst" => TextContent::Plain { text: text.clone() },
                TextContent::Native {
                    host,
                    alternative: None,
                    ..
                } if host != "typst" => {
                    return Err(err(format!(
                        "native text host {host} cannot render in Typst without an alternative"
                    )));
                }
                content => content.clone(),
            };
            layers.push(TypstLayer::Text {
                x: finite(x)?,
                y: finite(y)?,
                width: metrics.width,
                height: metrics.height,
                content,
                font_family: request.font_family.clone(),
                font_size: request.font_size,
                style: request.style.clone(),
                clip,
            });
        }
        _ => {
            let moved = moved(p, offset.x, offset.y);
            let mut body = String::new();
            let mut defs = String::new();
            let mut next = 0;
            svg_primitive(&moved, &mut body, &mut defs, &mut next)?;
            let mut svg = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}pt\" height=\"{}pt\" viewBox=\"0 0 {} {}\">",
                fmt(s.width)?,
                fmt(s.height)?,
                fmt(s.width)?,
                fmt(s.height)?
            );
            if let Some(c) = clip {
                write!(svg,"<defs><clipPath id=\"c\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" /></clipPath></defs><g clip-path=\"url(#c)\">",fmt(c.x)?,fmt(c.y)?,fmt(c.width)?,fmt(c.height)?).unwrap();
                svg.push_str(&body);
                svg.push_str("</g>");
            } else {
                svg.push_str(&body);
            }
            svg.push_str("</svg>");
            layers.push(TypstLayer::Svg { svg });
        }
    }
    Ok(())
}
impl Renderer for TypstRenderer {
    fn render(&self, s: &Scene) -> Result<String> {
        check_scene(s)?;
        let mut layers = vec![];
        let bg = Primitive::Rect {
            bounds: Bounds {
                x: 0.,
                y: 0.,
                width: s.width,
                height: s.height,
            },
            paint: Paint {
                fill: Some(s.background.clone()),
                stroke: None,
                stroke_width: 0.,
                opacity: 1.,
            },
            meta: Metadata::default(),
        };
        typst_layers(s, &bg, Point { x: 0., y: 0. }, None, &mut layers)?;
        for p in &s.primitives {
            typst_layers(s, p, Point { x: 0., y: 0. }, None, &mut layers)?;
        }
        serde_json::to_string(&TypstPlan {
            version: 1,
            width: s.width,
            height: s.height,
            layers,
        })
        .map_err(|e| err(e.to_string()))
    }
}
