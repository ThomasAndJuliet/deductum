// SPDX-License-Identifier: GPL-3.0-only

//! Charts in threads: a `chartgen` code block, laid out by chartgen and
//! drawn natively, as shapes and selectable text.
//!
//! chartgen lays a chart out in rounds, asking its host to measure the
//! chart's text. [`render`] answers with the shaping the thread draws text
//! with, so labels sit where chartgen placed them at the widths they are
//! drawn at. The shapes are drawn as cached geometry; the labels come back
//! as [`Label`]s, for the rich-text widget to shape as cells, so that they
//! select and copy like the rest of the thread.
//!
//! Neutral colours are set between the page's colour and the text's, so a
//! chart styled for white paper reads on any page; the palette's colours
//! are kept.

use cosmic::iced::widget::canvas::{Cache, Frame, Path, Stroke, path};
use cosmic::iced::{Color, Point, Radians, Rectangle, Size, Vector};
use serde_json::Value;

use chartgen::model::{
    LayoutConstraints, Measurements, Paint, PathCommand, Primitive, Report, Scene, TextContent, TextMetrics,
    Theme,
};

/// The family chartgen names, as the bundled fonts call it
fn family(name: &str) -> &str {
    match name {
        "New Computer Modern" => "NewComputerModern",
        name => name,
    }
}

/// What a label's text is: native text has a plain alternative, or its key
fn content(content: &TextContent) -> &str {
    match content {
        TextContent::Plain { text } => text,
        TextContent::Native { alternative: Some(text), .. } => text,
        TextContent::Native { key, .. } => key,
    }
}

/// A piece of a chart's text, in pixels from the chart's top left
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
    /// Its left edge
    pub x: f32,
    pub baseline: f32,
}

/// How text is measured: its width, line height and baseline below the
/// line's top, in pixels, for text, family, size, bold and italic
pub type Measure<'a> = dyn FnMut(&str, &str, f32, bool, bool) -> (f32, f32, f32) + 'a;

/// A chart laid out at a width
pub struct Chart {
    scene: Scene,
    /// Pixels per chartgen unit
    scale: f32,
    pub size: Size,
    pub labels: Vec<Label>,
    page: Color,
    ink: Color,
    shapes: Cache,
}

/// Lay out the chart of `spec`, `width` pixels wide, its text `text_size`
/// pixels, for a page coloured `page` with text coloured `ink`
pub fn render(
    spec: &str,
    width: f32,
    text_size: f32,
    page: Color,
    ink: Color,
    measure: &mut Measure<'_>,
) -> Result<Chart, String> {
    let spec: Value = serde_json::from_str(spec).map_err(|err| format!("the specification is not JSON: {err}"))?;
    let theme = Theme::default();
    let scale = text_size / theme.font_size as f32;
    let constraints = LayoutConstraints { width: f64::from(width / scale), ..LayoutConstraints::default() };
    let mut request = chartgen::RenderRequest {
        spec,
        theme,
        measurements: Measurements { context: constraints.measurement_context.clone(), ..Measurements::default() },
        constraints,
        backend: "svg".into(),
        variant: None,
        expect: Value::Null,
        source_name: None,
    };
    let rounds = request.constraints.max_rounds.max(8);
    for _ in 0..rounds {
        match chartgen::render_step(&request).map_err(|err| err.to_string())? {
            chartgen::RenderStep::NeedMeasurements { requests, .. } => {
                for text in requests {
                    let (width, height, baseline) = measure(
                        content(&text.content),
                        family(&text.font_family),
                        text.font_size as f32 * scale,
                        text.style.weight == "bold",
                        text.style.font_style == "italic",
                    );
                    request.measurements.entries.insert(
                        text.id.clone(),
                        TextMetrics {
                            width: f64::from(width / scale),
                            height: f64::from(height / scale),
                            baseline: Some(f64::from(baseline / scale)),
                        },
                    );
                }
            }
            chartgen::RenderStep::Complete { scene, .. } => {
                let mut chart = Chart {
                    size: Size::new(scene.width as f32 * scale, scene.height as f32 * scale),
                    scene,
                    scale,
                    labels: Vec::new(),
                    page,
                    ink,
                    shapes: Cache::new(),
                };
                let mut labels = Vec::new();
                for primitive in &chart.scene.primitives {
                    chart.collect_labels(primitive, Vector::ZERO, &mut labels);
                }
                chart.labels = labels;
                return Ok(chart);
            }
            chartgen::RenderStep::Stopped { report } => return Err(refusal(&report)),
        }
    }
    Err("chartgen needed more rounds of text measurement than it is given".into())
}

/// Why chartgen refused to draw a chart, as its findings say
fn refusal(report: &Report) -> String {
    let findings: Vec<String> = report
        .findings
        .iter()
        .filter(|finding| finding.severity == "error")
        .map(|finding| format!("{}: {}", finding.rule, finding.message))
        .collect();
    if findings.is_empty() { format!("chartgen did not draw the chart ({})", report.status) } else { findings.join("\n") }
}

impl Chart {
    /// The chart's shapes, drawn once and kept
    pub fn geometry(
        &self,
        renderer: &cosmic::Renderer,
    ) -> <cosmic::Renderer as cosmic::iced::advanced::graphics::geometry::Renderer>::Geometry {
        self.shapes.draw(renderer, self.size, |frame| {
            for primitive in &self.scene.primitives {
                self.draw(frame, primitive, Vector::ZERO);
            }
        })
    }

    fn at(&self, offset: Vector, x: f64, y: f64) -> Point {
        Point::new(offset.x + x as f32 * self.scale, offset.y + y as f32 * self.scale)
    }

    fn collect_labels(&self, primitive: &Primitive, offset: Vector, labels: &mut Vec<Label>) {
        match primitive {
            Primitive::Text { position, request, metrics, anchor, .. } => {
                let shift = match anchor.as_str() {
                    "middle" => metrics.width / 2.0,
                    "end" => metrics.width,
                    _ => 0.0,
                };
                let at = self.at(offset, position.x - shift, position.y + metrics.baseline.unwrap_or(metrics.height));
                labels.push(Label {
                    text: content(&request.content).to_string(),
                    family: family(&request.font_family).to_string(),
                    size: request.font_size as f32 * self.scale,
                    bold: request.style.weight == "bold",
                    italic: request.style.font_style == "italic",
                    color: request.style.fill.as_deref().and_then(|fill| self.colour(fill)).unwrap_or(self.ink),
                    x: at.x,
                    baseline: at.y,
                });
            }
            Primitive::Group { children, translate, .. } => {
                let offset = Vector::new(
                    offset.x + translate.x as f32 * self.scale,
                    offset.y + translate.y as f32 * self.scale,
                );
                for child in children {
                    self.collect_labels(child, offset, labels);
                }
            }
            _ => {}
        }
    }

    fn draw(&self, frame: &mut Frame, primitive: &Primitive, offset: Vector) {
        match primitive {
            Primitive::Rect { bounds, paint, .. } => {
                let path = Path::rectangle(
                    self.at(offset, bounds.x, bounds.y),
                    Size::new(bounds.width as f32 * self.scale, bounds.height as f32 * self.scale),
                );
                self.paint(frame, &path, paint);
            }
            Primitive::Line { from, to, paint, .. } => {
                let path = Path::line(self.at(offset, from.x, from.y), self.at(offset, to.x, to.y));
                self.paint(frame, &path, paint);
            }
            Primitive::Path { points, closed, paint, .. } => {
                let path = Path::new(|b| {
                    for (i, p) in points.iter().enumerate() {
                        let p = self.at(offset, p.x, p.y);
                        if i == 0 { b.move_to(p) } else { b.line_to(p) }
                    }
                    if *closed {
                        b.close();
                    }
                });
                self.paint(frame, &path, paint);
            }
            Primitive::Circle { center, radius, paint, .. } => {
                let path = Path::circle(self.at(offset, center.x, center.y), *radius as f32 * self.scale);
                self.paint(frame, &path, paint);
            }
            Primitive::Shape { commands, paint, .. } => {
                let path = Path::new(|b| self.commands(b, commands, offset));
                self.paint(frame, &path, paint);
            }
            Primitive::Curve { start, segments, head, paint, .. } => {
                let shaft = Path::new(|b| {
                    b.move_to(self.at(offset, start.x, start.y));
                    for s in segments {
                        b.bezier_curve_to(
                            self.at(offset, s.c1.x, s.c1.y),
                            self.at(offset, s.c2.x, s.c2.y),
                            self.at(offset, s.end.x, s.end.y),
                        );
                    }
                });
                self.paint(frame, &shaft, &Paint { fill: None, ..paint.clone() });
                // The head is filled in the shaft's colour
                if let Some(head) = head.as_ref().filter(|h| !h.is_empty()) {
                    let outline = Path::new(|b| {
                        for (i, p) in head.iter().enumerate() {
                            let p = self.at(offset, p.x, p.y);
                            if i == 0 { b.move_to(p) } else { b.line_to(p) }
                        }
                        b.close();
                    });
                    let fill = Paint { fill: paint.stroke.clone(), stroke: None, stroke_width: 0.0, opacity: paint.opacity };
                    self.paint(frame, &outline, &fill);
                }
            }
            Primitive::Use { .. } | Primitive::Wedge { .. } => {
                for lowered in chartgen::render::marks::lower(primitive).ok().flatten().unwrap_or_default() {
                    self.draw(frame, &lowered, offset);
                }
            }
            Primitive::Group { children, translate, clip, .. } => {
                let offset = Vector::new(
                    offset.x + translate.x as f32 * self.scale,
                    offset.y + translate.y as f32 * self.scale,
                );
                match clip {
                    Some(clip) => {
                        let region = Rectangle::new(
                            self.at(offset, clip.x, clip.y),
                            Size::new(clip.width as f32 * self.scale, clip.height as f32 * self.scale),
                        );
                        // A clipped frame starts at the region's corner
                        let inner = offset - Vector::new(region.x, region.y);
                        frame.with_clip(region, |frame| {
                            for child in children {
                                self.draw(frame, child, inner);
                            }
                        });
                    }
                    None => {
                        for child in children {
                            self.draw(frame, child, offset);
                        }
                    }
                }
            }
            // Text is the widget's, as cells
            Primitive::Text { .. } => {}
        }
    }

    fn commands(&self, b: &mut path::Builder, commands: &[PathCommand], offset: Vector) {
        let mut current = Point::ORIGIN;
        for command in commands {
            match command {
                PathCommand::MoveTo { to } => {
                    current = self.at(offset, to.x, to.y);
                    b.move_to(current);
                }
                PathCommand::LineTo { to } => {
                    current = self.at(offset, to.x, to.y);
                    b.line_to(current);
                }
                PathCommand::CubicTo { c1, c2, to } => {
                    current = self.at(offset, to.x, to.y);
                    b.bezier_curve_to(self.at(offset, c1.x, c1.y), self.at(offset, c2.x, c2.y), current);
                }
                PathCommand::ArcTo { rx, ry, rotation, large_arc, sweep, to } => {
                    let end = self.at(offset, to.x, to.y);
                    let radii = Vector::new(*rx as f32 * self.scale, *ry as f32 * self.scale);
                    match endpoint_arc(current, end, radii, (*rotation as f32).to_radians(), *large_arc, *sweep) {
                        Some(arc) => b.ellipse(arc),
                        None => b.line_to(end),
                    }
                    current = end;
                }
                PathCommand::Close => b.close(),
            }
        }
    }

    fn paint(&self, frame: &mut Frame, path: &Path, paint: &Paint) {
        let opacity = paint.opacity.clamp(0.0, 1.0) as f32;
        if let Some(color) = paint.fill.as_deref().and_then(|fill| self.colour(fill)) {
            frame.fill(path, Color { a: color.a * opacity, ..color });
        }
        if paint.stroke_width > 0.0
            && let Some(color) = paint.stroke.as_deref().and_then(|stroke| self.colour(stroke))
        {
            frame.stroke(
                path,
                Stroke::default()
                    .with_color(Color { a: color.a * opacity, ..color })
                    .with_width(paint.stroke_width as f32 * self.scale),
            );
        }
    }

    /// A CSS colour as drawn on this page: greys between the page and the
    /// ink, as white paper and chartgen's text grey are
    fn colour(&self, css: &str) -> Option<Color> {
        let css = css.trim();
        if css.is_empty() || css == "none" || css == "transparent" {
            return None;
        }
        let Some(color) = parse_hex(css) else { return Some(self.ink) };
        let [r, g, b, _] = color.into_rgba8();
        let (lo, hi) = (r.min(g).min(b), r.max(g).max(b));
        if hi - lo > 6 {
            return Some(color);
        }
        // White is the page, chartgen's text grey (#595959) the ink
        let t = ((255.0 - f32::from(hi)) / (255.0 - 89.0)).clamp(0.0, 1.0);
        let mix = |p: f32, i: f32| p + (i - p) * t;
        Some(Color {
            r: mix(self.page.r, self.ink.r),
            g: mix(self.page.g, self.ink.g),
            b: mix(self.page.b, self.ink.b),
            a: color.a,
        })
    }
}

fn parse_hex(css: &str) -> Option<Color> {
    let hex = css.strip_prefix('#')?;
    let digit = |i: usize, n: usize| u8::from_str_radix(hex.get(i..i + n)?, 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 => (digit(0, 1)? * 17, digit(1, 1)? * 17, digit(2, 1)? * 17, 255),
        6 => (digit(0, 2)?, digit(2, 2)?, digit(4, 2)?, 255),
        8 => (digit(0, 2)?, digit(2, 2)?, digit(4, 2)?, digit(6, 2)?),
        _ => return None,
    };
    Some(Color::from_rgba8(r, g, b, f32::from(a) / 255.0))
}

/// An SVG arc, from `from` to `to`, as the centre-parameterised ellipse
/// iced draws (SVG 1.1, appendix F.6.5), or `None` for a straight line
fn endpoint_arc(from: Point, to: Point, radii: Vector, rotation: f32, large_arc: bool, sweep: bool) -> Option<path::arc::Elliptical> {
    let (mut rx, mut ry) = (radii.x.abs(), radii.y.abs());
    if rx == 0.0 || ry == 0.0 || from == to {
        return None;
    }
    let (sin, cos) = rotation.sin_cos();
    let (dx, dy) = ((from.x - to.x) / 2.0, (from.y - to.y) / 2.0);
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    // Radii too small for the endpoints are scaled up
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = (num / den).max(0.0).sqrt();
    if large_arc == sweep {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let center = Point::new(
        cos * cx1 - sin * cy1 + (from.x + to.x) / 2.0,
        sin * cx1 + cos * cy1 + (from.y + to.y) / 2.0,
    );
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let start = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    Some(path::arc::Elliptical {
        center,
        radii: Vector::new(rx, ry),
        rotation: Radians(rotation),
        start_angle: Radians(start),
        end_angle: Radians(start + delta),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text measured as a monospace font would be: 0.5 em a character
    fn measure(text: &str, _: &str, size: f32, _: bool, _: bool) -> (f32, f32, f32) {
        (text.chars().count() as f32 * size * 0.5, size * 1.2, size * 0.95)
    }

    const BAR: &str = r#"{"chartspec":1,"id":"headcount","purpose":"analysis","title":{"text":"Headcount by region"},
        "data":{"x":{"kind":"nominal","values":["East","Central","West"]},
        "measure":{"name":"headcount","kind":"count","zero":"meaningful"},
        "series":[{"id":"headcount","name":"Headcount","values":[48,41,29]}]},
        "chart":{"type":"bar","orientation":"vertical","value_labels":"off"}}"#;

    #[test]
    fn lays_out_a_bar_chart_with_its_labels() {
        let chart = render(BAR, 600.0, 16.0, Color::WHITE, Color::BLACK, &mut measure).expect("a chart");
        assert!((chart.size.width - 600.0).abs() < 1.0, "{:?}", chart.size);
        let texts: Vec<&str> = chart.labels.iter().map(|l| l.text.as_str()).collect();
        for expected in ["Headcount by region", "East", "Central", "West"] {
            assert!(texts.contains(&expected), "{texts:?}");
        }
        assert!(chart.labels.iter().all(|l| l.x >= 0.0 && l.baseline > 0.0 && l.baseline <= chart.size.height + 1.0));
    }

    #[test]
    fn refusals_and_bad_json_say_why() {
        let err = render("{not json", 600.0, 16.0, Color::WHITE, Color::BLACK, &mut measure).err().unwrap();
        assert!(err.contains("not JSON"), "{err}");
        assert!(render(r#"{"chartspec":1}"#, 600.0, 16.0, Color::WHITE, Color::BLACK, &mut measure).is_err());
    }

    #[test]
    fn greys_fall_between_page_and_ink() {
        let chart = render(BAR, 600.0, 16.0, Color::BLACK, Color::WHITE, &mut measure).unwrap();
        // White paper is the (black) page, chartgen's text grey the (white) ink
        assert_eq!(chart.colour("#ffffff"), Some(Color { a: 1.0, ..Color::BLACK }));
        assert_eq!(chart.colour("#595959").map(|c| c.into_rgba8()), Some([255, 255, 255, 255]));
        // A palette colour is kept
        assert_eq!(chart.colour("#dd8045").map(|c| c.into_rgba8()), Some([0xdd, 0x80, 0x45, 255]));
        assert_eq!(chart.colour("none"), None);
    }

    #[test]
    fn a_half_circle_arc_has_its_centre_between_the_ends() {
        let arc = endpoint_arc(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Vector::new(5.0, 5.0), 0.0, false, true).unwrap();
        assert!((arc.center.x - 5.0).abs() < 1e-4 && arc.center.y.abs() < 1e-4, "{:?}", arc.center);
        assert!(((arc.end_angle.0 - arc.start_angle.0).abs() - std::f32::consts::PI).abs() < 1e-4);
    }
}
