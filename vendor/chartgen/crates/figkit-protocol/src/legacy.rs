//! The `figkit` operation: figkit's public functions for hosts (stylesheets, label typography,
//! colour functions, Typst escaping, figure geometry), computed by `chartgen-style`,
//! `chartgen-geometry` and `chartgen-render`. Hosts keep only I/O: a stylesheet is loaded in
//! rounds, the host answering each `need` (resolve a target against its sheet, read a sheet).
//!
//! Request: `{"operation": "figkit", "call": NAME, "args": {…}}`. Response data:
//! `{"ok": RESULT}`, `{"raise": {"type": "ValueError"|"IndexError"|"KeyError"|"AttributeError",
//! "message": …}}` (the original's exception), or for `style` `{"need": [REQUEST…]}`.
use figkit_geometry as geo;
use figkit_model::legacy::{Error, Point, Result};
use figkit_style as style;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

fn bad(message: impl Into<String>) -> Error {
    Error::new("protocol", message)
}
fn get<'a>(args: &'a Value, key: &str) -> Result<&'a Value> {
    args.get(key)
        .ok_or_else(|| bad(format!("figkit: missing argument {key}")))
}
fn num(args: &Value, key: &str) -> Result<f64> {
    number(get(args, key)?).ok_or_else(|| bad(format!("figkit: {key} must be a number")))
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
}
fn opt_num(args: &Value, key: &str) -> Result<Option<f64>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => number(v)
            .map(Some)
            .ok_or_else(|| bad(format!("figkit: {key} must be a number"))),
    }
}
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    get(args, key)?
        .as_str()
        .ok_or_else(|| bad(format!("figkit: {key} must be a string")))
}
fn strings(v: &Value, key: &str) -> Result<Vec<String>> {
    v.as_array()
        .ok_or_else(|| bad(format!("figkit: {key} must be a list of strings")))?
        .iter()
        .map(|s| {
            s.as_str()
                .map(str::to_owned)
                .ok_or_else(|| bad(format!("figkit: {key} must be a list of strings")))
        })
        .collect()
}
fn point(v: &Value) -> Result<Point> {
    match v.as_array().map(Vec::as_slice) {
        Some([x, y]) => Ok(Point {
            x: number(x).ok_or_else(|| bad("figkit: a point is two numbers"))?,
            y: number(y).ok_or_else(|| bad("figkit: a point is two numbers"))?,
        }),
        _ => Err(bad("figkit: a point is two numbers")),
    }
}
fn points(v: &Value) -> Result<Vec<Point>> {
    v.as_array()
        .ok_or_else(|| bad("figkit: points must be a list"))?
        .iter()
        .map(point)
        .collect()
}
fn finite(x: f64) -> Value {
    if x.is_finite() {
        json!(x)
    } else {
        json!(x.to_string())
    }
}
fn xy(p: &Point) -> Value {
    json!([finite(p.x), finite(p.y)])
}
fn bbox(b: geo::Bbox) -> Value {
    json!(b.as_array().map(finite))
}

fn raise(kind: &str, message: impl Into<String>) -> Value {
    json!({"raise": {"type": kind, "message": message.into()}})
}
fn style_raise(e: style::StyleError) -> Value {
    raise(
        match e.kind {
            style::ErrorKind::Value => "ValueError",
            style::ErrorKind::Index => "IndexError",
            style::ErrorKind::Key => "KeyError",
            style::ErrorKind::Attribute => "AttributeError",
            style::ErrorKind::Source => "OSError",
        },
        e.message,
    )
}
fn geo_raise(e: geo::GeometryError) -> Value {
    raise(
        match e.kind {
            geo::ErrorKind::Value => "ValueError",
            geo::ErrorKind::Key => "KeyError",
            geo::ErrorKind::Index => "IndexError",
        },
        e.message,
    )
}
fn ok(v: Value) -> Value {
    json!({ "ok": v })
}

/// A source whose sheets and links the host supplies; what is missing becomes a `need`.
struct HostSource {
    sheets: BTreeMap<String, String>,
    links: BTreeMap<String, BTreeMap<String, String>>,
    need: Vec<Value>,
}
impl style::StyleSource for HostSource {
    fn resolve(&mut self, from: &str, target: &str, link: style::Link) -> style::Result<String> {
        if let Some(id) = self.links.get(from).and_then(|m| m.get(target)) {
            return Ok(id.clone());
        }
        self.need
            .push(json!({"resolve": target, "from": from, "link": link}));
        Err(style::StyleError::new(style::ErrorKind::Source, "pending"))
    }
    fn read(&mut self, id: &str) -> style::Result<String> {
        if let Some(t) = self.sheets.get(id) {
            return Ok(t.clone());
        }
        self.need.push(json!({ "read": id }));
        Err(style::StyleError::new(style::ErrorKind::Source, "pending"))
    }
}

fn load_style(args: &Value) -> Result<Value> {
    let sheets = match args.get("sheets") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad(e.to_string()))?,
    };
    let links = match args.get("links") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad(e.to_string()))?,
    };
    let mut source = HostSource {
        sheets,
        links,
        need: vec![],
    };
    let loaded = style::Style::load(
        text(args, "name")?,
        text(args, "maker")?,
        text(args, "root")?,
        &mut source,
    );
    if !source.need.is_empty() {
        return Ok(json!({ "need": source.need }));
    }
    let st = match loaded {
        Ok(st) => st,
        Err(e) => return Ok(style_raise(e)),
    };
    let mut metrics = Map::new();
    for (k, v) in st.tokens.iter() {
        metrics.insert(
            k.into(),
            json!([
                v,
                match st.metric(k) {
                    Ok(m) => ok(finite(m)),
                    Err(e) => style_raise(e),
                }
            ]),
        );
    }
    let theme = match st.theme() {
        Ok(t) => ok(serde_json::to_value(t).map_err(|e| bad(e.to_string()))?),
        Err(e) => style_raise(e),
    };
    Ok(ok(json!({
        "name": st.name,
        "maker": st.maker,
        "defs": st.defs,
        "tokens": st.tokens.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
        "rules": st.rules,
        "css": st.css,
        "metrics": metrics,
        "theme": theme,
    })))
}

fn label_style(args: &Value) -> Result<Value> {
    let classes = strings(get(args, "classes")?, "classes")?;
    let own: BTreeMap<String, String> = match args.get("own") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| bad("figkit: own must map properties to strings"))?,
    };
    Ok(
        match style::label_style(text(args, "css")?, &classes, &own) {
            Ok(l) => ok(json!([finite(l.size), l.fill, l.weight, l.slant])),
            Err(e) => style_raise(e),
        },
    )
}

fn colour(args: &Value) -> Result<Value> {
    use style::colour as c;
    let a = get(args, "args")?
        .as_array()
        .ok_or_else(|| bad("figkit: colour args must be a list"))?;
    let s = |i: usize| -> Result<&str> {
        a.get(i)
            .and_then(Value::as_str)
            .ok_or_else(|| bad(format!("figkit: colour argument {i} must be a string")))
    };
    let f = |i: usize| -> Result<f64> {
        a.get(i)
            .and_then(number)
            .ok_or_else(|| bad(format!("figkit: colour argument {i} must be a number")))
    };
    let invalid = |h: &str| style_raise(style::hex_error(h));
    let arr = |v: [f64; 3]| ok(json!(v.map(finite)));
    let hex = |h: &str, v: Option<Value>| v.map_or_else(|| invalid(h), ok);
    let function = text(args, "function")?;
    Ok(match function {
        "hex_rgb" => c::hex_rgb(s(0)?).map_or_else(|| invalid(s(0).unwrap_or("")), arr),
        "rgb_hex" => {
            let v = a
                .first()
                .and_then(Value::as_array)
                .filter(|v| v.len() == 3)
                .ok_or_else(|| bad("figkit: rgb_hex takes three numbers"))?;
            let n = |i: usize| number(&v[i]).ok_or_else(|| bad("figkit: rgb_hex takes numbers"));
            ok(json!(c::rgb_hex([n(0)?, n(1)?, n(2)?])))
        }
        "oklch" => c::oklch(s(0)?).map_or_else(|| invalid(s(0).unwrap_or("")), arr),
        "from_oklch" => ok(json!(c::from_oklch(f(0)?, f(1)?, f(2)?))),
        "mix" => {
            let (x, y, t) = (s(0)?, s(1)?, f(2)?);
            if c::hex_rgb(x).is_none() {
                invalid(x)
            } else {
                hex(y, c::mix(x, y, t).map(Value::from))
            }
        }
        "oklch_shift" => hex(s(0)?, c::oklch_shift(s(0)?, f(1)?, f(2)?).map(Value::from)),
        "oklch_light" => {
            let cmax = match a.get(2) {
                None | Some(Value::Null) => None,
                Some(_) => Some(f(2)?),
            };
            hex(s(0)?, c::oklch_light(s(0)?, f(1)?, cmax).map(Value::from))
        }
        "luminance" => hex(s(0)?, c::luminance(s(0)?).map(finite)),
        "contrast" => {
            let (x, y) = (s(0)?, s(1)?);
            match (c::hex_rgb(x), c::hex_rgb(y)) {
                (None, _) => invalid(x),
                (_, None) => invalid(y),
                _ => ok(finite(c::contrast(x, y).unwrap_or(f64::NAN))),
            }
        }
        "text_tone" => {
            let fills = strings(get(args, "fills")?, "fills")?;
            let refs: Vec<&str> = fills.iter().map(String::as_str).collect();
            let (dark, light) = (s(0)?, s(1)?);
            if refs.is_empty() {
                raise("ValueError", "min() iterable argument is empty")
            } else if let Some(bad) = refs
                .iter()
                .chain([&dark, &light])
                .find(|h| c::hex_rgb(h).is_none())
            {
                invalid(bad)
            } else {
                let (tone, k) = c::text_tone(&refs, dark, light).expect("valid colours");
                ok(json!([tone, finite(k)]))
            }
        }
        "okhsl" => ok(json!(c::okhsl(f(0)?, f(1)?, f(2)?))),
        "okhsl_to_srgb" => arr(c::okhsl_to_srgb(f(0)?, f(1)?, f(2)?)),
        "srgb_to_okhsl" => arr(c::srgb_to_okhsl(f(0)?, f(1)?, f(2)?)),
        "to_gamma" => ok(finite(c::to_gamma(f(0)?))),
        "from_gamma" => ok(finite(c::from_gamma(f(0)?))),
        "oklab_to_linear_srgb" => arr(c::oklab_to_linear_srgb(f(0)?, f(1)?, f(2)?)),
        "linear_srgb_to_oklab" => arr(c::linear_srgb_to_oklab(f(0)?, f(1)?, f(2)?)),
        other => return Err(bad(format!("figkit: unknown colour function {other}"))),
    })
}

fn wedge(v: &Value) -> Result<std::result::Result<geo::Wedge, geo::GeometryError>> {
    Ok(geo::Wedge::new(
        num(v, "x")?,
        num(v, "y")?,
        num(v, "r")?,
        num(v, "a0")?,
        num(v, "a1")?,
        opt_num(v, "r0")?.unwrap_or(0.),
        opt_num(v, "inset")?.unwrap_or(0.),
    ))
}
fn commands(c: &[geo::PathCommand]) -> Result<Value> {
    serde_json::to_value(c).map_err(|e| bad(e.to_string()))
}

/// One geometry item.
fn geometry_item(item: &Value) -> Result<Value> {
    let pad = || opt_num(item, "pad").map(|p| p.unwrap_or(0.));
    let done = |r: std::result::Result<Value, geo::GeometryError>| match r {
        Ok(v) => ok(v),
        Err(e) => geo_raise(e),
    };
    Ok(match text(item, "op")? {
        "text_box" => {
            let anchor = match geo::Anchor::parse(text(item, "anchor")?) {
                Ok(a) => a,
                Err(e) => return Ok(geo_raise(e)),
            };
            let ext = get(item, "extent")?;
            let extent = geo::Extent {
                width: num(ext, "width")?,
                ascent: num(ext, "ascent")?,
                descent: num(ext, "descent")?,
            };
            done(geo::text_box(num(item, "x")?, num(item, "y")?, anchor, &extent, pad()?).map(bbox))
        }
        "rect_box" => done(
            geo::rect_box(
                num(item, "x")?,
                num(item, "y")?,
                num(item, "w")?,
                num(item, "h")?,
                pad()?,
            )
            .map(bbox),
        ),
        "use_box" => done(
            geo::use_box(num(item, "x")?, num(item, "y")?, num(item, "size")?, pad()?).map(bbox),
        ),
        "points_box" => done(geo::points_box(&points(get(item, "points")?)?, pad()?).map(bbox)),
        "polyline" => ok(commands(&geo::polyline(&points(get(item, "points")?)?))?),
        "flat_curve" => match geo::flat_curve(&points(get(item, "points")?)?) {
            Ok(c) => ok(commands(&c)?),
            Err(e) => geo_raise(e),
        },
        "curve" => {
            let segments = get(item, "segments")?
                .as_array()
                .ok_or_else(|| bad("figkit: segments must be a list"))?
                .iter()
                .map(|s| match s.as_array().map(Vec::as_slice) {
                    Some([a, b, c]) => Ok([point(a)?, point(b)?, point(c)?]),
                    _ => Err(bad("figkit: a segment is three points")),
                })
                .collect::<Result<Vec<_>>>()?;
            let head = match item.get("head") {
                None | Some(Value::Null) => None,
                Some(h) => Some(points(h)?),
            };
            let curve = geo::Curve {
                start: point(get(item, "start")?)?,
                segments,
                head,
            };
            let pts = curve.points();
            ok(json!({
                "points": pts.iter().map(xy).collect::<Vec<_>>(),
                "box": match curve.bbox(pad()?) { Ok(b) => ok(bbox(b)), Err(e) => geo_raise(e) },
                "shaft": commands(&curve.shaft())?,
                "head": match curve.head_path() { Some(h) => commands(&h)?, None => Value::Null },
            }))
        }
        "head_classes" => {
            done(geo::head_classes(&strings(get(item, "classes")?, "classes")?).map(|c| json!(c)))
        }
        "wedge_point" => match wedge(get(item, "wedge")?)? {
            Ok(w) => ok(xy(&w.point(num(item, "a")?, opt_num(item, "r")?))),
            Err(e) => geo_raise(e),
        },
        "wedge_shape" => done(wedge(get(item, "wedge")?)?.and_then(|w| w.shape()).map(
            |s| match s {
                None => Value::Null,
                Some(geo::WedgeShape::Circle { r, r0 }) => json!(["circle", finite(r), finite(r0)]),
                Some(geo::WedgeShape::Sector { apex, outer, inner }) => json!([
                    "sector",
                    xy(&apex),
                    [finite(outer.0), finite(outer.1)],
                    inner.map(|(a, b)| json!([finite(a), finite(b)]))
                ]),
            },
        )),
        "wedge_path" => match wedge(get(item, "wedge")?)?.and_then(|w| w.path()) {
            Ok(c) => ok(commands(&c)?),
            Err(e) => geo_raise(e),
        },
        "wedge_box" => {
            let pad = pad()?;
            done(
                wedge(get(item, "wedge")?)?
                    .and_then(|w| w.bbox(pad))
                    .map(bbox),
            )
        }
        "overlap" => {
            let b = |k: &str| -> Result<geo::Bbox> {
                let v: Vec<f64> = get(item, k)?
                    .as_array()
                    .filter(|a| a.len() == 4)
                    .ok_or_else(|| bad("figkit: a box is four numbers"))?
                    .iter()
                    .map(|x| number(x).ok_or_else(|| bad("figkit: a box is four numbers")))
                    .collect::<Result<_>>()?;
                Ok(geo::Bbox::new(v[0], v[1], v[2], v[3]))
            };
            ok(json!(geo::overlap(&b("a")?, &b("b")?)))
        }
        other => return Err(bad(format!("figkit: unknown geometry op {other}"))),
    })
}

/// Evaluate one figkit call.
pub fn call(name: &str, args: &Value) -> Result<Value> {
    match name {
        "style" => load_style(args),
        "label_style" => label_style(args),
        "colour" => colour(args),
        "strip_comments" => Ok(ok(json!(style::strip_comments(text(args, "css")?)))),
        "parse" => Ok(ok(json!(style::parse(text(args, "css")?)))),
        "num" => Ok(ok(json!(style::num(num(args, "x")?)))),
        "metric" => Ok(
            match style::metric_value(
                text(args, "style")?,
                text(args, "token")?,
                text(args, "value")?,
            ) {
                Ok(m) => ok(finite(m)),
                Err(e) => style_raise(e),
            },
        ),
        "typst_escape" => Ok(ok(json!(
            strings(get(args, "texts")?, "texts")?
                .iter()
                .map(|t| figkit_render::legacy::typst_escape(t))
                .collect::<Vec<_>>()
        ))),
        "geometry" => Ok(ok(Value::Array(
            get(args, "items")?
                .as_array()
                .ok_or_else(|| bad("figkit: items must be a list"))?
                .iter()
                .map(geometry_item)
                .collect::<Result<_>>()?,
        ))),
        other => Err(bad(format!("figkit: unknown call {other}"))),
    }
}
