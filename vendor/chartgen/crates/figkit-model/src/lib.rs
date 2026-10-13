//! Portable figure contracts in physical points; no chart or host dependencies.
use serde::{Deserialize, Serialize};
pub mod legacy;
mod portable;
pub use portable::*;
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{kind}: {message}")]
pub struct Error {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}
impl Error {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            path: None,
        }
    }
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
/// One cubic Bézier segment: two control points and its end.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CubicSegment {
    pub c1: Point,
    pub c2: Point,
    pub end: Point,
}
/// A path command in physical coordinates (SVG path semantics, absolute).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PathCommand {
    MoveTo {
        to: Point,
    },
    LineTo {
        to: Point,
    },
    CubicTo {
        c1: Point,
        c2: Point,
        to: Point,
    },
    ArcTo {
        rx: f64,
        ry: f64,
        rotation: f64,
        large_arc: bool,
        sweep: bool,
        to: Point,
    },
    Close,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paint {
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub stroke_width: f64,
    pub opacity: f64,
}
impl Default for Paint {
    fn default() -> Self {
        Self {
            fill: None,
            stroke: Some("#595959".into()),
            stroke_width: 1.,
            opacity: 1.,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextContent {
    Plain {
        text: String,
    },
    Native {
        host: String,
        key: String,
        alternative: Option<String>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextStyle {
    /// Ink for rendered text. None means the host document's current text ink.
    #[serde(default)]
    pub fill: Option<String>,
    /// CSS/Typst/TeX style value: `normal` or `bold`.
    #[serde(default = "normal_text_style")]
    pub weight: String,
    /// CSS/Typst/TeX style value: `normal` or `italic`.
    #[serde(default = "normal_text_style")]
    pub font_style: String,
}
fn normal_text_style() -> String {
    "normal".into()
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            fill: None,
            weight: normal_text_style(),
            font_style: normal_text_style(),
        }
    }
}
impl TextContent {
    pub fn plain(text: impl Into<String>) -> Self {
        Self::Plain { text: text.into() }
    }
    pub fn readable(&self) -> &str {
        match self {
            Self::Plain { text } => text,
            Self::Native { alternative, .. } => alternative.as_deref().unwrap_or(""),
        }
    }
}

impl<'de> Deserialize<'de> for PathCommand {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        fn field<T: serde::de::DeserializeOwned>(
            v: &serde_json::Value,
            key: &str,
        ) -> std::result::Result<T, String> {
            serde_json::from_value(
                v.get(key)
                    .cloned()
                    .ok_or_else(|| format!("missing path field {key}"))?,
            )
            .map_err(|e| e.to_string())
        }
        let decode = || -> std::result::Result<Self, String> {
            Ok(match value.get("op").and_then(serde_json::Value::as_str) {
                Some("move_to") => Self::MoveTo {
                    to: field(&value, "to")?,
                },
                Some("line_to") => Self::LineTo {
                    to: field(&value, "to")?,
                },
                Some("cubic_to") => Self::CubicTo {
                    c1: field(&value, "c1")?,
                    c2: field(&value, "c2")?,
                    to: field(&value, "to")?,
                },
                Some("arc_to") => Self::ArcTo {
                    rx: field(&value, "rx")?,
                    ry: field(&value, "ry")?,
                    rotation: field(&value, "rotation")?,
                    large_arc: field(&value, "large_arc")?,
                    sweep: field(&value, "sweep")?,
                    to: field(&value, "to")?,
                },
                Some("close") => Self::Close,
                _ => return Err("unknown path command".into()),
            })
        };
        decode().map_err(serde::de::Error::custom)
    }
}
