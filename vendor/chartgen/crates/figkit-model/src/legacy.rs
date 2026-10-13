//! Chart-free version-1 house-format contracts.
pub use super::{
    Bounds, CubicSegment, Error, Paint, PathCommand, Point, Result, TextContent, TextStyle,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRequest {
    pub id: String,
    pub content: TextContent,
    pub role: String,
    pub font_family: String,
    pub font_size: f64,
    #[serde(default)]
    pub style: TextStyle,
    pub max_width: Option<f64>,
    pub context: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextMetrics {
    pub width: f64,
    pub height: f64,
    pub baseline: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Measurements {
    pub context: String,
    pub entries: BTreeMap<String, TextMetrics>,
}
/// An element's identity as the original's figure model carries it (tools/figkit/model.py):
/// its classes (role and modifiers), id and `data-*` values. Maps keep insertion order,
/// which is the order the compatibility source writes their attributes in.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metadata {
    pub id: Option<String>,
    pub role: String,
    pub classes: Vec<String>,
    pub data: IndexMap<String, String>,
    /// Further attributes, written as given in this order: a group's presentation attributes
    /// (`fill`, `stroke`, `mask`; figkit `Group.attrs`, where a `data-*` name fixes that value's
    /// position among them) and a rectangle's corner radius `rx`.
    #[serde(default)]
    pub attrs: IndexMap<String, String>,
    /// Texts with the same joint are parts of one label (figkit `Text.joint`).
    #[serde(default)]
    pub joint: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Primitive {
    Rect {
        bounds: Bounds,
        paint: Paint,
        meta: Metadata,
    },
    Line {
        from: Point,
        to: Point,
        paint: Paint,
        meta: Metadata,
    },
    Path {
        points: Vec<Point>,
        closed: bool,
        paint: Paint,
        meta: Metadata,
    },
    Circle {
        center: Point,
        radius: f64,
        paint: Paint,
        meta: Metadata,
    },
    Text {
        position: Point,
        request: TextRequest,
        metrics: TextMetrics,
        anchor: String,
        meta: Metadata,
    },
    Group {
        children: Vec<Primitive>,
        translate: Point,
        clip: Option<Bounds>,
        meta: Metadata,
    },
    /// A mark of the house mark library (figkit `Use`): symbol `href` (its id, without `#`)
    /// centred on `center` with diameter `size`.
    Use {
        center: Point,
        size: f64,
        href: String,
        /// The mark's ink for renderers without the house stylesheet (portable SVG, PGF, Typst);
        /// compatibility SVG leaves colour to the stylesheet's classes, as the original does.
        paint: Paint,
        meta: Metadata,
    },
    /// A stroke that is not data (figkit `Curve`): cubic segments from `start`, and an optional
    /// closed head outline filled in the stroke's colour, classed with the first class + `-head`.
    Curve {
        start: Point,
        segments: Vec<CubicSegment>,
        head: Option<Vec<Point>>,
        paint: Paint,
        meta: Metadata,
    },
    /// A pie or ring sector (figkit `Wedge`): angles in degrees clockwise from 12 o'clock,
    /// `inner_radius` > 0 for a ring, `inset` moving its straight edges inward. Scene lowering
    /// turns it into a `Shape` with chartgen-geometry; renderers do not compute its outline.
    Wedge {
        center: Point,
        radius: f64,
        inner_radius: f64,
        start_angle: f64,
        end_angle: f64,
        inset: f64,
        paint: Paint,
        meta: Metadata,
    },
    /// General path data: lowered wedges, flat (bumps) curves and other outlines.
    Shape {
        commands: Vec<PathCommand>,
        paint: Paint,
        meta: Metadata,
    },
}
impl Primitive {
    /// The element's identity metadata, whatever its kind.
    pub fn meta(&self) -> &Metadata {
        match self {
            Self::Rect { meta, .. }
            | Self::Line { meta, .. }
            | Self::Path { meta, .. }
            | Self::Circle { meta, .. }
            | Self::Text { meta, .. }
            | Self::Group { meta, .. }
            | Self::Use { meta, .. }
            | Self::Curve { meta, .. }
            | Self::Wedge { meta, .. }
            | Self::Shape { meta, .. } => meta,
        }
    }
    pub fn meta_mut(&mut self) -> &mut Metadata {
        match self {
            Self::Rect { meta, .. }
            | Self::Line { meta, .. }
            | Self::Path { meta, .. }
            | Self::Circle { meta, .. }
            | Self::Text { meta, .. }
            | Self::Group { meta, .. }
            | Self::Use { meta, .. }
            | Self::Curve { meta, .. }
            | Self::Wedge { meta, .. }
            | Self::Shape { meta, .. } => meta,
        }
    }
}
impl<'de> Deserialize<'de> for Primitive {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        fn field<T: serde::de::DeserializeOwned>(
            value: &Value,
            key: &str,
        ) -> std::result::Result<T, String> {
            serde_json::from_value(
                value
                    .get(key)
                    .cloned()
                    .ok_or_else(|| format!("missing primitive field {key}"))?,
            )
            .map_err(|e| e.to_string())
        }
        let decode = || -> std::result::Result<Self, String> {
            Ok(match value.get("kind").and_then(Value::as_str) {
                Some("rect") => Self::Rect {
                    bounds: field(&value, "bounds")?,
                    paint: field(&value, "paint")?,
                    meta: field(&value, "meta")?,
                },
                Some("line") => Self::Line {
                    from: field(&value, "from")?,
                    to: field(&value, "to")?,
                    paint: field(&value, "paint")?,
                    meta: field(&value, "meta")?,
                },
                Some("path") => Self::Path {
                    points: field(&value, "points")?,
                    closed: field(&value, "closed")?,
                    paint: field(&value, "paint")?,
                    meta: field(&value, "meta")?,
                },
                Some("circle") => Self::Circle {
                    center: field(&value, "center")?,
                    radius: field(&value, "radius")?,
                    paint: field(&value, "paint")?,
                    meta: field(&value, "meta")?,
                },
                Some("text") => Self::Text {
                    position: field(&value, "position")?,
                    request: field(&value, "request")?,
                    metrics: field(&value, "metrics")?,
                    anchor: field(&value, "anchor")?,
                    meta: field(&value, "meta")?,
                },
                Some("group") => Self::Group {
                    children: field(&value, "children")?,
                    translate: field(&value, "translate")?,
                    clip: field(&value, "clip")?,
                    meta: field(&value, "meta")?,
                },
                Some("use") => Self::Use {
                    center: field(&value, "center")?,
                    size: field(&value, "size")?,
                    href: field(&value, "href")?,
                    paint: field(&value, "paint")?,
                    meta: field(&value, "meta")?,
                },
                Some("curve") => Self::Curve {
                    start: field(&value, "start")?,
                    segments: field(&value, "segments")?,
                    head: field(&value, "head")?,
                    paint: field(&value, "paint")?,
                    meta: field(&value, "meta")?,
                },
                Some("wedge") => Self::Wedge {
                    center: field(&value, "center")?,
                    radius: field(&value, "radius")?,
                    inner_radius: field(&value, "inner_radius")?,
                    start_angle: field(&value, "start_angle")?,
                    end_angle: field(&value, "end_angle")?,
                    inset: field(&value, "inset")?,
                    paint: field(&value, "paint")?,
                    meta: field(&value, "meta")?,
                },
                Some("shape") => Self::Shape {
                    commands: field(&value, "commands")?,
                    paint: field(&value, "paint")?,
                    meta: field(&value, "meta")?,
                },
                _ => return Err("unknown primitive kind".into()),
            })
        };
        decode().map_err(serde::de::Error::custom)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub version: u32,
    pub width: f64,
    pub height: f64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub background: String,
    pub primitives: Vec<Primitive>,
    /// The source's header comment (figkit `Figure.comment`: `<key>: GENERATED by chartgen ...`).
    #[serde(default)]
    pub comment: Option<String>,
    /// Raw SVG definitions for `defs#chart-defs` (figkit `Figure.defs`: masks, clip paths).
    #[serde(default)]
    pub defs: Vec<String>,
}
