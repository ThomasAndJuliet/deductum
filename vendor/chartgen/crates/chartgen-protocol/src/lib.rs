//! Host-neutral, versioned byte protocol. No host I/O or mutable global state.
use chartgen::{RenderRequest, RenderStep};
use chartgen_model::{Error, Result, Scene};
use serde::{Deserialize, Serialize};

pub mod figkit;

pub const VERSION: u32 = 1;
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
#[derive(Debug, Serialize)]
pub struct Request {
    pub version: u32,
    #[serde(flatten)]
    pub operation: Operation,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Operation {
    Render {
        request: Box<RenderRequest>,
    },
    RenderScene {
        scene: Scene,
        backend: String,
    },
    Choose {
        spec: serde_json::Value,
        variant: Option<String>,
        expect: serde_json::Value,
    },
    Schema,
    LintScene {
        scene: Scene,
    },
    LintSvg {
        svg: String,
        style: String,
        final_width: Option<f64>,
        cvd: String,
        /// The flattened house stylesheet the drawing will be baked under.
        #[serde(default)]
        css: Option<String>,
        /// Each `<text>`'s typeset extent `[width, ascent, descent]`, in document order.
        #[serde(default)]
        extents: Option<Vec<Option<[f64; 3]>>>,
        #[serde(default)]
        date_format: Option<String>,
    },
    /// The texts of a source SVG a host measures before `lint_svg` (bake's typesetting).
    LintSvgTexts {
        svg: String,
        #[serde(default)]
        css: Option<String>,
    },
    /// The style lint (the original's lint_style) over a flattened style: its tokens as literal
    /// values and its rules `[[selector, [[property, value], …]], …]`.
    LintStyle {
        tokens: serde_json::Value,
        rules: serde_json::Value,
    },
    /// Plain text as Typst markup (the original's typst.escape), for hosts.
    Escape {
        text: String,
    },
    /// figkit's public functions for hosts (see the `figkit` module).
    Figkit {
        call: String,
        args: serde_json::Value,
    },
}
#[derive(Debug, Serialize)]
pub struct Response {
    pub version: u32,
    #[serde(flatten)]
    pub outcome: Outcome,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Render { result: Box<RenderStep> },
    Output { output: String },
    Error { error: Error },
    Data { value: serde_json::Value },
}

pub fn dispatch(request: Request) -> Result<Outcome> {
    if request.version != VERSION {
        return Err(Error::new(
            "protocol",
            format!("unsupported protocol version {}", request.version),
        ));
    }
    match request.operation {
        Operation::Render { request } => {
            chartgen::render_step(&request).map(|result| Outcome::Render {
                result: Box::new(result),
            })
        }
        Operation::RenderScene { scene, backend } => {
            chartgen::render_scene(&scene, &backend).map(|output| Outcome::Output { output })
        }
        Operation::Choose {
            spec,
            variant,
            expect,
        } => chartgen::choose_detail(spec, variant, expect).and_then(|(report, detail)| {
            let mut value =
                serde_json::to_value(report).map_err(|e| Error::new("protocol", e.to_string()))?;
            // The chooser's choice and the parts-of-a-total test, for the CLI's "The choice"
            // and "Facts the spec does not give" (the original's __main__ prints both).
            value["choice"] = detail.choice.map_or(serde_json::Value::Null, |c| {
                serde_json::json!({"type": c.type_, "orientation": c.orientation, "sort": c.sort,
                    "sort_by": c.sort_by,
                    "given_order": chartgen::policy::given_order(&c.type_)})
            });
            value["breakdown"] = serde_json::Value::Bool(detail.breakdown);
            Ok(Outcome::Data { value })
        }),
        Operation::Schema => Ok(Outcome::Data {
            value: chartgen::data::schema(),
        }),
        Operation::LintScene { scene } => {
            chartgen::scene::validate(&scene)?;
            serde_json::to_value(chartgen::lint::scene(&scene))
                .map(|value| Outcome::Data { value })
                .map_err(|error| Error::new("protocol", error.to_string()))
        }
        Operation::LintSvg {
            svg,
            style,
            final_width,
            cvd,
            css: None,
            extents: None,
            date_format: None,
        } => serde_json::to_value(chartgen::svg_lint::lint_svg(
            &svg,
            &style,
            final_width,
            &cvd,
        )?)
        .map(|value| Outcome::Data { value })
        .map_err(|error| Error::new("protocol", error.to_string())),
        Operation::LintSvg {
            svg,
            final_width,
            cvd,
            css,
            extents,
            date_format,
            ..
        } => serde_json::to_value(chartgen::svg_lint::lint(
            &svg,
            &chartgen::svg_lint::Options {
                css,
                extents: extents.map(|e| {
                    e.into_iter()
                        .map(|x| {
                            x.map(|[width, ascent, descent]| chartgen::svg_lint::Extent {
                                width,
                                ascent,
                                descent,
                            })
                        })
                        .collect()
                }),
                generated: false,
                final_width,
                cvd,
                date_format,
            },
        )?)
        .map(|value| Outcome::Data { value })
        .map_err(|error| Error::new("protocol", error.to_string())),
        Operation::LintSvgTexts { svg, css } => {
            serde_json::to_value(chartgen::svg_lint::text_queries(&svg, css.as_deref())?)
                .map(|value| Outcome::Data { value })
                .map_err(|error| Error::new("protocol", error.to_string()))
        }
        Operation::LintStyle { tokens, rules } => {
            let bad = || {
                protocol_error(
                    "lint_style needs tokens {name: value} and rules [[selector, [[property, value]]]]",
                )
            };
            let mut style = chartgen::lint::style::FlatStyle::default();
            for (k, v) in tokens.as_object().ok_or_else(bad)? {
                style
                    .tokens
                    .insert(k.clone(), v.as_str().ok_or_else(bad)?.to_owned());
            }
            for r in rules.as_array().ok_or_else(bad)? {
                let sel = r[0].as_str().ok_or_else(bad)?.to_owned();
                let decls = r[1]
                    .as_array()
                    .ok_or_else(bad)?
                    .iter()
                    .map(|d| Some((d[0].as_str()?.to_owned(), d[1].as_str()?.to_owned())))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(bad)?;
                style.rules.push((sel, decls));
            }
            serde_json::to_value(chartgen::lint::style::lint_style(&style))
                .map(|value| Outcome::Data { value })
                .map_err(|error| Error::new("protocol", error.to_string()))
        }
        Operation::Escape { text } => Ok(Outcome::Data {
            value: serde_json::Value::String(chartgen::render::typst::escape(&text)),
        }),
        Operation::Figkit { call, args } => {
            figkit::call(&call, &args).map(|value| Outcome::Data { value })
        }
    }
}
fn respond(outcome: Outcome) -> Vec<u8> {
    let response = Response {
        version: VERSION,
        outcome,
    };
    serde_json::to_vec(&response).expect("protocol response contains serializable fields")
}

/// The response to an input of `length` bytes that exceeds [`MAX_INPUT_BYTES`],
/// for hosts that know the length before copying the input.
pub fn handle_oversized(length: usize) -> Vec<u8> {
    respond(Outcome::Error {
        error: protocol_error(format!(
            "request of {length} bytes exceeds the 16 MiB limit"
        )),
    })
}

pub fn handle(input: &[u8]) -> Vec<u8> {
    let result = if input.len() > MAX_INPUT_BYTES {
        Err(Error::new("protocol", "request exceeds 16 MiB limit"))
    } else {
        std::str::from_utf8(input)
            .map_err(|e| protocol_error(format!("request is not UTF-8: {e}")))
            .and_then(|text| {
                serde_json::from_str::<serde_json::Value>(text)
                    .map_err(|e| protocol_error(format!("request is not JSON: {e}")))
            })
            .and_then(parse_request)
            .and_then(dispatch)
    };
    respond(result.unwrap_or_else(|error| Outcome::Error { error }))
}

// Deserialize numeric-bearing fields directly from Value. Serde's generic
// flattened/tagged content buffer cannot preserve arbitrary_precision numbers
// while also decoding geometry f64 fields.
fn protocol_error(message: impl Into<String>) -> Error {
    Error::new("protocol", message)
}

/// Keys each operation accepts; anything else is a malformed request, never ignored.
fn operation_keys(operation: &str) -> Option<&'static [&'static str]> {
    Some(match operation {
        "render" => &["request"],
        "render_scene" => &["scene", "backend"],
        "choose" => &["spec", "variant", "expect"],
        "schema" => &[],
        "lint_scene" => &["scene"],
        "lint_svg" => &[
            "svg",
            "style",
            "final_width",
            "cvd",
            "css",
            "extents",
            "date_format",
        ],
        "lint_svg_texts" => &["svg", "css"],
        "escape" => &["text"],
        "lint_style" => &["tokens", "rules"],
        "figkit" => &["call", "args"],
        _ => return None,
    })
}

const RENDER_REQUEST_KEYS: &[&str] = &[
    "spec",
    "theme",
    "constraints",
    "measurements",
    "backend",
    "variant",
    "expect",
    "source_name",
];

fn check_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
    what: &str,
) -> Result<()> {
    let unknown: Vec<_> = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(protocol_error(format!(
            "{what} has unknown field(s): {}",
            unknown.join(", ")
        )))
    }
}

/// An optional string field: absent or null is None; any other non-string is an error.
fn optional_string(value: &serde_json::Value, key: &str) -> Result<Option<String>> {
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(protocol_error(format!("{key} must be a string"))),
    }
}

// Deserialize numeric-bearing fields directly from Value. Serde's generic
// flattened/tagged content buffer cannot preserve arbitrary_precision numbers
// while also decoding geometry f64 fields.
fn parse_request(value: serde_json::Value) -> Result<Request> {
    let object = value
        .as_object()
        .ok_or_else(|| protocol_error("a request must be a JSON object"))?;
    let version = object
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| protocol_error("version must be an integer"))?;
    if version != VERSION {
        return Err(protocol_error(format!(
            "unsupported protocol version {version}"
        )));
    }
    let name = object
        .get("operation")
        .ok_or_else(|| protocol_error("missing operation"))?
        .as_str()
        .ok_or_else(|| protocol_error("operation must be a string"))?;
    let keys = operation_keys(name)
        .ok_or_else(|| protocol_error(format!("unknown operation {name:?}")))?;
    let mut allowed = vec!["version", "operation"];
    allowed.extend_from_slice(keys);
    check_keys(object, &allowed, &format!("the {name} request"))?;
    let parse_error = |e: serde_json::Error| protocol_error(e.to_string());
    let required = |key: &str, what: &str| {
        value
            .get(key)
            .filter(|v| !v.is_null())
            .cloned()
            .ok_or_else(|| protocol_error(format!("missing {what}")))
    };
    let operation = match name {
        "render" => {
            let request = required("request", "render request")?;
            let fields = request
                .as_object()
                .ok_or_else(|| protocol_error("the render request must be an object"))?;
            check_keys(fields, RENDER_REQUEST_KEYS, "the render request")?;
            if !fields.contains_key("spec") {
                return Err(protocol_error("missing spec in the render request"));
            }
            Operation::Render {
                request: Box::new(serde_json::from_value(request).map_err(parse_error)?),
            }
        }
        "render_scene" => Operation::RenderScene {
            scene: serde_json::from_value(required("scene", "scene")?).map_err(parse_error)?,
            backend: optional_string(&value, "backend")?
                .ok_or_else(|| protocol_error("missing backend"))?,
        },
        "choose" => Operation::Choose {
            spec: required("spec", "spec")?,
            variant: optional_string(&value, "variant")?,
            expect: match value.get("expect") {
                None | Some(serde_json::Value::Null) => serde_json::json!({}),
                Some(expect @ serde_json::Value::Object(_)) => expect.clone(),
                Some(_) => return Err(protocol_error("expect must be an object")),
            },
        },
        "schema" => Operation::Schema,
        "lint_scene" => Operation::LintScene {
            scene: serde_json::from_value(required("scene", "scene")?).map_err(parse_error)?,
        },
        "lint_svg" => Operation::LintSvg {
            svg: optional_string(&value, "svg")?
                .ok_or_else(|| protocol_error("missing SVG source"))?,
            style: optional_string(&value, "style")?.unwrap_or_default(),
            final_width: value
                .get("final_width")
                .filter(|v| !v.is_null())
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(parse_error)?,
            cvd: optional_string(&value, "cvd")?.unwrap_or_else(|| "none".into()),
            css: optional_string(&value, "css")?,
            extents: value
                .get("extents")
                .filter(|v| !v.is_null())
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(parse_error)?,
            date_format: optional_string(&value, "date_format")?,
        },
        "lint_svg_texts" => Operation::LintSvgTexts {
            svg: optional_string(&value, "svg")?
                .ok_or_else(|| protocol_error("missing SVG source"))?,
            css: optional_string(&value, "css")?,
        },
        "lint_style" => Operation::LintStyle {
            tokens: required("tokens", "style tokens")?,
            rules: required("rules", "style rules")?,
        },
        "escape" => Operation::Escape {
            text: optional_string(&value, "text")?
                .ok_or_else(|| protocol_error("missing text to escape"))?,
        },
        "figkit" => Operation::Figkit {
            call: optional_string(&value, "call")?
                .ok_or_else(|| protocol_error("missing figkit call"))?,
            args: match value.get("args") {
                None | Some(serde_json::Value::Null) => serde_json::json!({}),
                Some(args) => args.clone(),
            },
        },
        _ => unreachable!("operation_keys lists every operation"),
    };
    Ok(Request { version, operation })
}
impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        parse_request(serde_json::Value::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}
impl<'de> Deserialize<'de> for Response {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        fn field<T: serde::de::DeserializeOwned>(
            value: &serde_json::Value,
            key: &str,
        ) -> std::result::Result<T, String> {
            serde_json::from_value(
                value
                    .get(key)
                    .cloned()
                    .ok_or_else(|| format!("missing {key}"))?,
            )
            .map_err(|e| e.to_string())
        }
        let decode = || -> std::result::Result<Self, String> {
            let outcome = match value["outcome"].as_str() {
                Some("render") => Outcome::Render {
                    result: Box::new(field(&value, "result")?),
                },
                Some("output") => Outcome::Output {
                    output: field(&value, "output")?,
                },
                Some("error") => Outcome::Error {
                    error: field(&value, "error")?,
                },
                Some("data") => Outcome::Data {
                    value: field(&value, "value")?,
                },
                _ => return Err("unknown protocol outcome".into()),
            };
            Ok(Self {
                version: field(&value, "version")?,
                outcome,
            })
        };
        decode().map_err(serde::de::Error::custom)
    }
}
