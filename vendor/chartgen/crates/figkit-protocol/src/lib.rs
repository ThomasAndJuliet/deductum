//! Independent versioned figure transport; legacy routes remain version 1.
use figkit_model::{ErrorKind, FigureError, FigureResult, Limits};
use serde_json::{Value, json};
pub mod legacy;
pub mod session;
pub const VERSION: u32 = 2;
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
fn invalid(message: impl Into<String>) -> FigureError {
    FigureError::new(ErrorKind::InvalidInput, message)
}
fn field<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> FigureResult<T> {
    serde_json::from_value(
        v.get(key)
            .cloned()
            .ok_or_else(|| invalid(format!("missing {key}")))?,
    )
    .map_err(|e| invalid(e.to_string()))
}
pub fn render_legacy(
    scene: &figkit_model::legacy::Scene,
    backend: &str,
) -> figkit_model::legacy::Result<String> {
    use figkit_render::legacy::*;
    match backend {
        "svg" => SvgRenderer.render(scene),
        "compatibility_svg" => CompatibilitySvgRenderer.render(scene),
        "pgf" => PgfRenderer.render(scene),
        "typst" => TypstRenderer.render(scene),
        _ => Err(figkit_model::legacy::Error::new(
            "backend",
            format!("unsupported backend {backend}"),
        )),
    }
}
fn dispatch(v: &Value, limits: &Limits) -> FigureResult<Value> {
    let version: u32 = field(v, "version")?;
    let operation: String = field(v, "operation")?;
    let allowed: &[&str] = match operation.as_str() {
        "negotiate" => &[],
        "figkit" => &["call", "args"],
        "render_scene" if version == 1 => &["scene", "backend"],
        "render_scene" => &["figure", "backend", "session"],
        "measure" => &["context", "session", "requests", "replies"],
        _ => &[],
    };
    if v.as_object()
        .ok_or_else(|| invalid("request must be an object"))?
        .keys()
        .any(|k| k != "version" && k != "operation" && !allowed.contains(&k.as_str()))
    {
        return Err(invalid("unknown request field"));
    }

    if version == 1 {
        let result = match operation.as_str() {
            "figkit" => legacy::call(
                &field::<String>(v, "call")?,
                v.get("args").unwrap_or(&json!({})),
            )
            .map(|value| json!({"value":value,"outcome":"data"})),
            "render_scene" => render_legacy(&field(v, "scene")?, &field::<String>(v, "backend")?)
                .map(|output| json!({"output":output,"outcome":"output"})),
            _ => return Err(invalid("unsupported legacy operation")),
        };
        return Ok(result.unwrap_or_else(|error| json!({"outcome":"error","error":error})));
    }
    if version != VERSION {
        return Err(invalid(format!("unsupported protocol version {version}")));
    }
    match operation.as_str() {
        "negotiate" => Ok(
            json!({"outcome":"data","value":{"versions":[1,2],"scene_versions":[1,2],"limits":limits}}),
        ),
        "render_scene" => {
            let figure = field(v, "figure")?;
            if let Some(state) = v.get("session").filter(|s| !s.is_null()) {
                let session: session::Session =
                    serde_json::from_value(state.clone()).map_err(|e| invalid(e.to_string()))?;
                session.validate_figure(&figure)?;
            }
            let scene = figkit_scene::prepare(&figure, limits)?;
            let backend = match field::<String>(v, "backend")?.as_str() {
                "svg" => figkit_render::Backend::Svg,
                "browser" => figkit_render::Backend::Browser,
                "pgf" => figkit_render::Backend::Pgf,
                "typst" => figkit_render::Backend::Typst,
                _ => {
                    return Err(FigureError::new(
                        ErrorKind::UnsupportedCapability,
                        "unknown portable backend",
                    ));
                }
            };
            Ok(json!({"outcome":"output","output":figkit_render::render(&scene, backend)?}))
        }
        "measure" => {
            let mut session: session::Session = match v.get("session") {
                Some(s) if !s.is_null() => {
                    serde_json::from_value(s.clone()).map_err(|e| invalid(e.to_string()))?
                }
                _ => session::Session::new(field(v, "context")?),
            };
            let requests: Vec<figkit_model::TextRequest> = field(v, "requests")?;
            let replies = v.get("replies").cloned().unwrap_or(json!([]));
            let replies: Vec<figkit_model::MeasurementReply> =
                serde_json::from_value(replies).map_err(|e| invalid(e.to_string()))?;
            let pending = session.advance(&requests, &replies, limits)?;
            Ok(
                json!({"outcome":"data","value":{"session":session,"requests":pending,"complete":pending.is_empty()}}),
            )
        }
        _ => Err(invalid("unknown operation")),
    }
}
struct BoundedOutput {
    bytes: Vec<u8>,
    limit: usize,
}
impl std::io::Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > self.limit)
        {
            return Err(std::io::Error::other("output byte limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn handle_with_limits(input: &[u8], limits: &Limits) -> Vec<u8> {
    let mut version = VERSION;
    let result = if input.len() > limits.max_input_bytes {
        Err(FigureError::new(
            ErrorKind::BudgetExceeded,
            "input byte limit exceeded",
        ))
    } else {
        serde_json::from_slice::<Value>(input)
            .map_err(|e| invalid(e.to_string()))
            .and_then(|v| {
                version = v
                    .get("version")
                    .and_then(Value::as_u64)
                    .unwrap_or(VERSION as u64) as u32;
                dispatch(&v, limits)
            })
    };
    let mut response = result.unwrap_or_else(|error| {
        if version == 1 { json!({"outcome":"error","error":{"kind":"protocol","message":error.message,"path":error.path}}) }
        else { json!({"outcome":"error","error":error}) }
    });
    response["version"] = json!(version);
    let mut out = BoundedOutput {
        bytes: Vec::new(),
        limit: limits.max_output_bytes,
    };
    if serde_json::to_writer(&mut out, &response).is_err() {
        return serde_json::to_vec(&json!({"version":version,"outcome":"error","error":FigureError::new(ErrorKind::BudgetExceeded,"output byte limit exceeded")})).unwrap();
    }
    out.bytes
}
pub fn handle(input: &[u8]) -> Vec<u8> {
    handle_with_limits(input, &Limits::default())
}
pub fn handle_oversized(length: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({"version":VERSION,"outcome":"error","error":FigureError::new(ErrorKind::BudgetExceeded,format!("input length {length} exceeds limit"))})).unwrap()
}
