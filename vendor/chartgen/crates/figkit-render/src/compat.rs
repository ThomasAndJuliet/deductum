//! Compatibility source SVG in the original's house format (tools/figkit/emit.py).
//!
//! Elements carry their scene classes (with the role as the last class when it is
//! not among them), ids, `data-*` identities and further attributes, in the
//! original's attribute order: class, id, geometry, data. Coordinates are rounded
//! to 0.01 pt as the original writes them. Labels are `<text class="prose …">`
//! placeholders holding Typst markup, placed by their baseline and `text-anchor`.
//! As in the original, the source carries no paint and no typography: the house
//! stylesheet styles every element by its classes when bake.py bakes it, so a
//! layout must request text measurement with the typography its classes give
//! (bake.label_style), and colours with the classes the stylesheet colours.
use crate::legacy::{err, finite, positive, single_line_svg, typst_escape};
use figkit_model::legacy::{Metadata, PathCommand, Point, Primitive, Result, Scene, TextContent};
use std::fmt::Write;

/// Character data escaped as the original (xml.sax.saxutils.escape): &, < and >.
fn text_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A double-quoted attribute value: the original's escapes plus the quote.
fn xml(s: &str) -> String {
    text_xml(s).replace('"', "&quot;")
}

/// An attribute with its value quoted as xml.sax.saxutils.quoteattr does (the original's
/// data-* and group attributes): single quotes when the value holds a double quote only.
fn quoted(name: &str, v: &str) -> String {
    let v = text_xml(v)
        .replace('\n', "&#10;")
        .replace('\r', "&#13;")
        .replace('\t', "&#9;");
    if v.contains('"') {
        if v.contains('\'') {
            format!(" {name}=\"{}\"", v.replace('"', "&quot;"))
        } else {
            format!(" {name}='{v}'")
        }
    } else {
        format!(" {name}=\"{v}\"")
    }
}

/// A number as the original writes it: two decimals, trailing zeros dropped.
pub(crate) fn n(x: f64) -> Result<String> {
    let s = format!("{:.2}", finite(x)?);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    Ok(if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.into()
    })
}

fn classes(meta: &Metadata, first: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    if let Some(first) = first {
        out.push(first.into());
    }
    for c in &meta.classes {
        if !out.contains(c) {
            out.push(c.clone());
        }
    }
    if !meta.role.is_empty() && !out.contains(&meta.role) {
        out.push(meta.role.clone());
    }
    out
}

fn class_id(classes: &[String], meta: &Metadata) -> String {
    let mut out = String::new();
    if !classes.is_empty() {
        write!(out, " class=\"{}\"", xml(&classes.join(" "))).unwrap();
    }
    if let Some(id) = &meta.id {
        write!(out, " id=\"{}\"", xml(id)).unwrap();
    }
    out
}

fn head(meta: &Metadata, first: Option<&str>) -> String {
    class_id(&classes(meta, first), meta)
}

fn safe_name(k: &str) -> bool {
    !k.is_empty()
        && k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':')
}

fn data(meta: &Metadata) -> String {
    let mut out = String::new();
    for (k, v) in &meta.data {
        if safe_name(k) {
            out.push_str(&quoted(&format!("data-{k}"), v));
        }
    }
    out
}

/// Further attributes except the named ones, in order, their values as given (figkit Group.attrs).
fn attrs(meta: &Metadata, skip: &[&str]) -> Result<String> {
    let mut out = String::new();
    for (k, v) in &meta.attrs {
        if !skip.contains(&k.as_str()) && safe_name(k) {
            out.push_str(&quoted(k, v));
        }
    }
    Ok(out)
}

fn point(p: &Point) -> Result<String> {
    Ok(format!("{},{}", n(p.x)?, n(p.y)?))
}

/// SVG path data written as the original writes it ("M1,2 L3,4 C… A… Z").
pub(crate) fn path_data(commands: &[PathCommand]) -> Result<String> {
    let mut d = vec![];
    for c in commands {
        d.push(match c {
            PathCommand::MoveTo { to } => format!("M{}", point(to)?),
            PathCommand::LineTo { to } => format!("L{}", point(to)?),
            PathCommand::CubicTo { c1, c2, to } => {
                format!("C{} {} {}", point(c1)?, point(c2)?, point(to)?)
            }
            PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to,
            } => {
                positive(*rx)?;
                positive(*ry)?;
                format!(
                    "A{},{} {} {},{} {}",
                    n(*rx)?,
                    n(*ry)?,
                    n(*rotation)?,
                    u8::from(*large_arc),
                    u8::from(*sweep),
                    point(to)?
                )
            }
            PathCommand::Close => "Z".into(),
        });
    }
    if !matches!(commands.first(), Some(PathCommand::MoveTo { .. })) {
        return Err(err("path data must start with a move"));
    }
    Ok(d.join(" "))
}

fn primitive(
    p: &Primitive,
    depth: usize,
    out: &mut Vec<String>,
    defs: &mut Vec<String>,
) -> Result<()> {
    let ind = "  ".repeat(depth);
    match p {
        Primitive::Rect {
            bounds: b, meta, ..
        } => {
            positive(b.width)?;
            positive(b.height)?;
            // figkit Rect.rx: a number, written by the original's rounding.
            let rx = match meta.attrs.get("rx") {
                Some(v) => {
                    let r: f64 = v
                        .parse()
                        .map_err(|_| err(format!("rect rx is not a number: {v}")))?;
                    format!(" rx=\"{}\"", n(positive(r)?)?)
                }
                None => String::new(),
            };
            out.push(format!(
                "{ind}<rect{} x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rx}{}{}/>",
                head(meta, None),
                n(b.x)?,
                n(b.y)?,
                n(b.width)?,
                n(b.height)?,
                data(meta),
                attrs(meta, &["rx"])?
            ));
        }
        Primitive::Line { from, to, meta, .. } => out.push(format!(
            "{ind}<line{} x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}{}/>",
            head(meta, None),
            n(from.x)?,
            n(from.y)?,
            n(to.x)?,
            n(to.y)?,
            data(meta),
            attrs(meta, &[])?
        )),
        Primitive::Path {
            points,
            closed,
            meta,
            ..
        } => {
            if points.is_empty() {
                return Err(err("path has no points"));
            }
            let mut d = vec![];
            for (i, q) in points.iter().enumerate() {
                d.push(format!("{}{}", if i == 0 { "M" } else { "L" }, point(q)?));
            }
            if *closed {
                d.push("Z".into());
            }
            out.push(format!(
                "{ind}<path{} d=\"{}\"{}{}/>",
                head(meta, None),
                d.join(" "),
                data(meta),
                attrs(meta, &[])?
            ));
        }
        Primitive::Shape { commands, meta, .. } => out.push(format!(
            "{ind}<path{} d=\"{}\"{}{}/>",
            head(meta, None),
            path_data(commands)?,
            data(meta),
            attrs(meta, &[])?
        )),
        Primitive::Curve {
            start,
            segments,
            head: arrow,
            meta,
            ..
        } => {
            let mut d = format!("M{}", point(start)?);
            for s in segments {
                write!(
                    d,
                    " C{} {} {}",
                    point(&s.c1)?,
                    point(&s.c2)?,
                    point(&s.end)?
                )
                .unwrap();
            }
            let all = classes(meta, None);
            out.push(format!(
                "{ind}<path{} d=\"{d}\"{}{}/>",
                class_id(&all, meta),
                data(meta),
                attrs(meta, &[])?
            ));
            if let Some(outline) = arrow {
                if outline.is_empty() {
                    return Err(err("a curve's head has no points"));
                }
                let mut head_classes = all.clone();
                match head_classes.first_mut() {
                    Some(first) => first.push_str("-head"),
                    None => return Err(err("a curve with a head needs a class")),
                }
                let pts = outline.iter().map(point).collect::<Result<Vec<_>>>()?;
                out.push(format!(
                    "{ind}<path class=\"{}\" d=\"M{} Z\"{}/>",
                    xml(&head_classes.join(" ")),
                    pts.join(" L"),
                    data(meta)
                ));
            }
        }
        Primitive::Use {
            center,
            size,
            href,
            meta,
            ..
        } => {
            positive(*size)?;
            if href.is_empty() || href.starts_with('#') {
                return Err(err("a symbol mark names its symbol's id, without #"));
            }
            let r = size / 2.;
            out.push(format!(
                "{ind}<use{} href=\"#{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{}/>",
                head(meta, None),
                xml(href),
                n(center.x - r)?,
                n(center.y - r)?,
                n(*size)?,
                n(*size)?,
                data(meta),
                attrs(meta, &[])?
            ));
        }
        Primitive::Wedge { .. } => {
            // emit.py writes a slice as its outline path (nothing when nothing is drawn).
            if let Some(shape) = crate::legacy::marks::wedge(p)? {
                primitive(&shape, depth, out, defs)?;
            }
        }
        Primitive::Circle {
            center,
            radius,
            meta,
            ..
        } => {
            positive(*radius)?;
            out.push(format!(
                "{ind}<circle{} cx=\"{}\" cy=\"{}\" r=\"{}\"{}{}/>",
                head(meta, None),
                n(center.x)?,
                n(center.y)?,
                n(*radius)?,
                data(meta),
                attrs(meta, &[])?
            ));
        }
        Primitive::Text {
            position,
            request,
            metrics,
            anchor,
            meta,
        } => {
            let code = match &request.content {
                TextContent::Plain { text } => {
                    single_line_svg(text)?;
                    typst_escape(text)
                }
                TextContent::Native { host, key, .. } if host == "typst" => key.clone(),
                TextContent::Native {
                    alternative: Some(text),
                    ..
                } => {
                    single_line_svg(text)?;
                    typst_escape(text)
                }
                TextContent::Native { host, .. } => {
                    return Err(err(format!(
                        "native {host} text needs a Typst alternative for compatibility SVG"
                    )));
                }
            };
            if !matches!(anchor.as_str(), "start" | "middle" | "end") {
                return Err(err(format!("unsupported text anchor: {anchor}")));
            }
            positive(request.font_size)?;
            let baseline = metrics.baseline.unwrap_or(metrics.height);
            out.push(format!(
                "{ind}<text{} x=\"{}\" y=\"{}\"{}{}{}>{}</text>",
                head(meta, Some("prose")),
                n(position.x)?,
                n(position.y + baseline)?,
                if anchor == "start" {
                    String::new()
                } else {
                    format!(" text-anchor=\"{anchor}\"")
                },
                data(meta),
                attrs(meta, &[])?,
                text_xml(&code)
            ));
        }
        Primitive::Group {
            children,
            translate,
            clip,
            meta,
        } => {
            let mut a = head(meta, None);
            // figkit Group.attrs: written in order; a data-* name there keeps its place.
            a.push_str(&attrs(meta, &[])?);
            for (k, v) in &meta.data {
                if safe_name(k) && !meta.attrs.contains_key(&format!("data-{k}")) {
                    a.push_str(&quoted(&format!("data-{k}"), v));
                }
            }
            if translate.x != 0. || translate.y != 0. {
                write!(
                    a,
                    " transform=\"translate({} {})\"",
                    n(translate.x)?,
                    n(translate.y)?
                )
                .unwrap();
            }
            if let Some(c) = clip {
                positive(c.width)?;
                positive(c.height)?;
                let id = format!("chartgen-clip-{}", defs.len() + 1);
                defs.push(format!(
                    "<clipPath id=\"{id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",
                    n(c.x)?,
                    n(c.y)?,
                    n(c.width)?,
                    n(c.height)?
                ));
                write!(a, " clip-path=\"url(#{id})\"").unwrap();
            }
            if children.is_empty() {
                out.push(format!("{ind}<g{a}/>"));
            } else {
                out.push(format!("{ind}<g{a}>"));
                for child in children {
                    primitive(child, depth + 1, out, defs)?;
                }
                out.push(format!("{ind}</g>"));
            }
        }
    }
    Ok(())
}

/// The figure's root: a scene whose only top-level primitive is a group (figkit Figure.root).
fn is_root_group(p: &Primitive) -> bool {
    matches!(p, Primitive::Group { translate, clip: None, .. } if translate.x == 0. && translate.y == 0.)
}

pub(crate) fn svg(s: &Scene) -> Result<String> {
    // emit.py writes any size, an empty figure's zero height included.
    if s.version != 1 {
        return Err(err(format!("unsupported scene version: {}", s.version)));
    }
    positive(s.width)?;
    positive(s.height)?;
    let (w, h) = (n(s.width)?, n(s.height)?);
    let mut out = vec![
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\""
            .to_string(),
        format!("     viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\">"),
    ];
    if let Some(comment) = &s.comment {
        // An XML comment cannot hold "--"; the original writes an en dash instead.
        out.push(format!("  <!-- {} -->", comment.replace("--", "–")));
    }
    if let Some(title) = &s.title {
        out.push(format!("  <title>{}</title>", text_xml(title)));
    }
    if let Some(desc) = &s.description {
        out.push(format!("  <desc>{}</desc>", text_xml(desc)));
    }
    out.push("  <style id=\"house\"/>".into());
    out.push("  <defs id=\"house-defs\"/>".into());
    let mut body = vec![];
    let mut defs = vec![];
    // The document root is the scene's single group; other scenes are wrapped in the chart group.
    let has_chart = s.primitives.iter().any(|p| {
        matches!(p, Primitive::Group { meta, .. } if meta.classes.first().map(String::as_str) == Some("chart"))
    });
    if (s.primitives.len() == 1 && is_root_group(&s.primitives[0])) || has_chart {
        // The scene's own root; elements a layout placed beside its chart group stay beside it.
        for p in &s.primitives {
            primitive(p, 1, &mut body, &mut defs)?;
        }
    } else {
        body.push("  <g class=\"chart\" id=\"chart\">".into());
        for p in &s.primitives {
            primitive(p, 2, &mut body, &mut defs)?;
        }
        body.push("  </g>".into());
    }
    // The figure's own definitions first (figkit Figure.defs), then the group clips.
    let defs: Vec<String> = s.defs.iter().cloned().chain(defs).collect();
    if defs.is_empty() {
        out.push("  <defs id=\"chart-defs\"/>".into());
    } else {
        out.push("  <defs id=\"chart-defs\">".into());
        out.extend(defs.into_iter().map(|d| format!("    {d}")));
        out.push("  </defs>".into());
    }
    out.extend(body);
    out.push("</svg>".into());
    Ok(out.join("\n") + "\n")
}
