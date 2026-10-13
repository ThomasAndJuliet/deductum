//! Version 2 measured portable figures. Coordinates use top-left origin, downward Y, points.
use crate::{Bounds, Paint, PathCommand, Point, TextContent};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
macro_rules! identifier {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.into())
            }
        }
        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
identifier!(ElementId);
identifier!(RequestId);
identifier!(ContextId);
identifier!(ResourceId);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasurementContext {
    pub id: ContextId,
    pub engine: String,
    pub engine_version: String,
    pub font_environment: String,
    pub settings: String,
    pub registry_revision: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Typography {
    pub font_family: String,
    pub font_size: f64,
    pub weight: String,
    pub font_style: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRequest {
    pub id: RequestId,
    pub content: TextContent,
    pub typography: Typography,
    pub max_width: Option<f64>,
    pub context: MeasurementContext,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockMetrics {
    pub size: Size,
    pub baseline: Option<f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasurementReply {
    pub request: TextRequest,
    pub metrics: BlockMetrics,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextPaint {
    Resolved { fill: String },
    NativeInherited { host: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasuredText {
    pub request: TextRequest,
    pub metrics: BlockMetrics,
    pub paint: TextPaint,
}
impl MeasuredText {
    /// Bind metrics only to the exact request that was measured, including context and native revision.
    pub fn from_reply(
        request: TextRequest,
        reply: MeasurementReply,
        paint: TextPaint,
    ) -> FigureResult<Self> {
        if request != reply.request {
            return Err(FigureError::new(
                ErrorKind::StaleMeasurement,
                "measurement reply does not match the complete request",
            ));
        }
        Ok(Self {
            request,
            metrics: reply.metrics,
            paint,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizontalAnchor {
    Start,
    Middle,
    End,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMetadata {
    pub element_id: Option<ElementId>,
    pub authored_id: Option<String>,
    #[serde(default)]
    pub classes: Vec<String>,
    pub label_group: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacedText {
    pub text: MeasuredText,
    pub position: Point,
    pub anchor: HorizontalAnchor,
    pub metadata: ElementMetadata,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Primitive {
    Rect {
        bounds: Bounds,
        radius: f64,
        paint: Paint,
        metadata: ElementMetadata,
    },
    Line {
        from: Point,
        to: Point,
        paint: Paint,
        metadata: ElementMetadata,
    },
    Circle {
        center: Point,
        radius: f64,
        paint: Paint,
        metadata: ElementMetadata,
    },
    Path {
        commands: Vec<PathCommand>,
        paint: Paint,
        metadata: ElementMetadata,
    },
    Text {
        placed: PlacedText,
    },
    Group {
        children: Vec<Primitive>,
        translate: Point,
        clip: Option<ResourceId>,
        /// The group composited as one layer at this opacity (0–1): overlapping children inside
        /// it do not darken one another. 1 (the default when absent) draws them as they are.
        opacity: f64,
        metadata: ElementMetadata,
    },
    Use {
        symbol: ResourceId,
        position: Point,
        metadata: ElementMetadata,
    },
}
impl Primitive {
    pub fn metadata(&self) -> &ElementMetadata {
        match self {
            Self::Rect { metadata, .. }
            | Self::Line { metadata, .. }
            | Self::Circle { metadata, .. }
            | Self::Path { metadata, .. }
            | Self::Group { metadata, .. }
            | Self::Use { metadata, .. } => metadata,
            Self::Text { placed } => &placed.metadata,
        }
    }
    pub fn metadata_mut(&mut self) -> &mut ElementMetadata {
        match self {
            Self::Rect { metadata, .. }
            | Self::Line { metadata, .. }
            | Self::Circle { metadata, .. }
            | Self::Path { metadata, .. }
            | Self::Group { metadata, .. }
            | Self::Use { metadata, .. } => metadata,
            Self::Text { placed } => &mut placed.metadata,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Clip {
    Rect { bounds: Bounds },
    Path { commands: Vec<PathCommand> },
}
/// Symbol geometry has no nested resource references or text measurement dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub shapes: Vec<SymbolShape>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolShape {
    pub commands: Vec<PathCommand>,
    pub paint: Paint,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resources {
    #[serde(default)]
    pub clips: BTreeMap<ResourceId, Clip>,
    #[serde(default)]
    pub symbols: BTreeMap<ResourceId, Symbol>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FigureMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Figure {
    pub version: u32,
    pub size: Size,
    pub metadata: FigureMetadata,
    pub background: String,
    pub primitives: Vec<Primitive>,
    pub resources: Resources,
}
/// Raw compatibility resources never form part of a portable Figure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompatibilityResources {
    pub definitions: Vec<String>,
    pub attributes: BTreeMap<ElementId, indexmap::IndexMap<String, String>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidInput,
    UnsupportedCapability,
    MissingMeasurement,
    StaleMeasurement,
    ResourceFailure,
    BudgetExceeded,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct FigureError {
    pub kind: ErrorKind,
    pub message: String,
    pub path: Option<String>,
}
impl FigureError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            path: None,
        }
    }
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}
pub type FigureResult<T> = std::result::Result<T, FigureError>;
/// Configurable limits checked before allocation/expansion; counts include resource geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limits {
    pub max_depth: usize,
    pub max_primitives: usize,
    pub max_path_segments: usize,
    pub max_resources: usize,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_measurement_requests: usize,
    pub max_measurement_rounds: usize,
    pub max_style_imports: usize,
    pub max_style_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_primitives: 100_000,
            max_path_segments: 1_000_000,
            max_resources: 4096,
            max_input_bytes: 16 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
            max_measurement_requests: 100_000,
            max_measurement_rounds: 8,
            max_style_imports: 256,
            max_style_bytes: 16 * 1024 * 1024,
        }
    }
}

fn field<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    key: &str,
) -> std::result::Result<T, String> {
    serde_json::from_value(
        value
            .get(key)
            .cloned()
            .ok_or_else(|| format!("missing field {key}"))?,
    )
    .map_err(|e| e.to_string())
}
impl<'de> Deserialize<'de> for Primitive {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        let decode = || -> std::result::Result<Self, String> {
            Ok(
                match value.get("kind").and_then(serde_json::Value::as_str) {
                    Some("rect") => Self::Rect {
                        bounds: field(&value, "bounds")?,
                        radius: field(&value, "radius")?,
                        paint: field(&value, "paint")?,
                        metadata: field(&value, "metadata")?,
                    },
                    Some("line") => Self::Line {
                        from: field(&value, "from")?,
                        to: field(&value, "to")?,
                        paint: field(&value, "paint")?,
                        metadata: field(&value, "metadata")?,
                    },
                    Some("circle") => Self::Circle {
                        center: field(&value, "center")?,
                        radius: field(&value, "radius")?,
                        paint: field(&value, "paint")?,
                        metadata: field(&value, "metadata")?,
                    },
                    Some("path") => Self::Path {
                        commands: field(&value, "commands")?,
                        paint: field(&value, "paint")?,
                        metadata: field(&value, "metadata")?,
                    },
                    Some("text") => Self::Text {
                        placed: field(&value, "placed")?,
                    },
                    Some("group") => Self::Group {
                        children: field(&value, "children")?,
                        translate: field(&value, "translate")?,
                        clip: field(&value, "clip")?,
                        opacity: value.get("opacity").and_then(|v| v.as_f64()).unwrap_or(1.),
                        metadata: field(&value, "metadata")?,
                    },
                    Some("use") => Self::Use {
                        symbol: field(&value, "symbol")?,
                        position: field(&value, "position")?,
                        metadata: field(&value, "metadata")?,
                    },
                    _ => return Err("unknown kind".into()),
                },
            )
        };
        decode().map_err(serde::de::Error::custom)
    }
}
impl<'de> Deserialize<'de> for Clip {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        let decode = || -> std::result::Result<Self, String> {
            Ok(
                match value.get("kind").and_then(serde_json::Value::as_str) {
                    Some("rect") => Self::Rect {
                        bounds: field(&value, "bounds")?,
                    },
                    Some("path") => Self::Path {
                        commands: field(&value, "commands")?,
                    },
                    _ => return Err("unknown kind".into()),
                },
            )
        };
        decode().map_err(serde::de::Error::custom)
    }
}
