//! Shared measured-text and style-metric support for the matrix and circular families.
//!
//! These families are ports of the original layouts (`layout/table.py`, `heatmap.py`,
//! `pie.py`, `treemap.py`), which place every text by its baseline from the host's measured
//! advance width and ink ascent/descent, size their gaps from the stylesheet's metrics, and
//! start their figure at the title block's top-left corner. The host's `TextMetrics.baseline`
//! is that ink ascent; a host that cannot report one is read as an ascent equal to the box
//! height, which is what the renderers do with such a measurement.
//!
//! Measurement follows the original `Batch`: a layout asks for any text it needs while it runs.
//! Unmeasured texts are collected; the layout is then abandoned and the host asked for all of
//! them, and the layout is run again, until every text it reads has been measured.
use crate::guides;
use chartgen_model::*;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;

pub fn err(message: impl Into<String>) -> Error {
    Error::new("layout", message)
}

/// A typeset line: advance width and ink above and below the baseline (points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ext {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
}

/// A text placed by its baseline (`y`) and anchor, as the original's `Text`.
#[derive(Debug, Clone)]
pub struct Placed {
    pub role: String,
    pub mods: Vec<String>,
    pub x: f64,
    pub y: f64,
    pub anchor: &'static str,
    pub ext: Ext,
    pub request: TextRequest,
    pub metrics: TextMetrics,
    pub data: BTreeMap<String, String>,
    /// The value the text writes, (series, x), for the notes' marks (never written out).
    pub value_of: Option<(String, String)>,
}
impl Placed {
    /// (x0, y0, x1, y1) of the typeset line: advance width, ink height.
    pub fn bbox(&self) -> [f64; 4] {
        let w = self.ext.width;
        let x0 = self.x
            - match self.anchor {
                "middle" => w / 2.,
                "end" => w,
                _ => 0.,
            };
        [
            x0,
            self.y - self.ext.ascent,
            x0 + w,
            self.y + self.ext.descent,
        ]
    }
    /// The scene text: its classes those the original writes (`prose`, modifiers, role).
    pub fn primitive(&self) -> Primitive {
        let mut classes = self.mods.clone();
        classes.push(self.role.clone());
        Primitive::Text {
            position: Point {
                x: self.x,
                y: self.y - self.metrics.baseline.unwrap_or(self.metrics.height),
            },
            request: self.request.clone(),
            metrics: self.metrics.clone(),
            anchor: self.anchor.into(),
            meta: Metadata {
                classes,
                data: self
                    .data
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                value_of: self.value_of.clone(),
                ..Default::default()
            },
        }
    }
}

/// The original plain style's metrics, in points (style/base.css flattened at --label-size 10).
const PLAIN_METRICS: &[(&str, f64)] = &[
    ("--label-size", 10.0),
    ("--title-size", 12.0),
    ("--note-size", 8.0),
    ("--note-mark-size", 8.0),
    ("--inset-text-size", 8.0),
    ("--comment-size", 10.0),
    ("--line-pitch", 12.5),
    ("--title-gap", 16.0),
    ("--text-pad", 1.5),
    ("--table-col-gap", 15.0),
    ("--table-total-gap", 30.0),
    ("--table-fill-pad", 3.5),
    ("--table-fill-inset", 2.0),
    ("--table-rule-drop", 6.0),
    ("--table-alone-pitch", 21.5),
    ("--table-rule-width", 0.6),
    ("--cell-pitch", 16.0),
    ("--legend-gap", 2.5),
    ("--cell-strip", 30.0),
    ("--cell-dot", 5.5),
    ("--cell-bar-height", 9.0),
    ("--trend-width", 40.0),
    ("--action-pitch", 20.0),
    ("--action-bar", 0.35),
    ("--action-bar-length", 80.0),
    ("--action-dot", 7.5),
    ("--action-gap", 6.0),
    ("--pie-radius", 0.2),
    ("--pie-min-radius", 30.0),
    ("--pie-gap", 0.007),
    ("--pie-label-gap", 0.1),
    ("--pie-label-width", 70.0),
    ("--pie-leader-angle", 8.0),
    ("--tile-gap", 1.0),
    ("--tile-group-gap", 2.5),
    ("--tile-pad", 3.0),
    ("--tile-min-chars", 3.0),
    ("--legend-margin", 10.0),
    ("--legend-key", 5.5),
    ("--legend-entry-gap", 10.0),
    ("--category-label-gap", 6.0),
    ("--heat-max-column", 50.0),
    ("--heat-row-pitch", 10.0),
    ("--summary-gap", 4.0),
    ("--plot-aspect", 0.6),
    ("--table-gap", 20.0),
    ("--table-row-pitch", 16.0),
    ("--table-header-drop", 7.5),
    ("--key-gap", 2.5),
    ("--scale-gap", 9.0),
    ("--leader-run", 10.0),
    ("--donut-hole", 0.74),
    ("--grid-width", 0.6),
    ("--border-width", 2.0),
    ("--ramp-key-height", 7.0),
    ("--ramp-key-gap", 3.5),
    ("--time-label-sep", 6.0),
    ("--ramp-key-width", 120.0),
    ("--key-tick", 3.0),
    ("--key-pill-pad-x", 3.5),
    ("--key-pill-pad-y", 2.0),
    ("--key-halo-width", 3.0),
    ("--link-width", 0.8),
    ("--comment-arrow-width", 0.8),
    ("--comment-arrow-gap", 3.0),
    ("--comment-gap", 5.0),
    ("--comment-width", 110.0),
];

pub struct Kit<'a> {
    pub theme: &'a Theme,
    pub c: &'a LayoutConstraints,
    m: &'a Measurements,
    missing: RefCell<BTreeMap<String, TextRequest>>,
    styles: RefCell<BTreeMap<Vec<String>, (f64, TextStyle)>>,
}

impl<'a> Kit<'a> {
    pub fn new(theme: &'a Theme, c: &'a LayoutConstraints, m: &'a Measurements) -> Self {
        Self {
            theme,
            c,
            m,
            missing: RefCell::new(BTreeMap::new()),
            styles: RefCell::new(BTreeMap::new()),
        }
    }

    /// A stylesheet metric in points (the flattened token); a theme without it takes the
    /// original plain style's value (base.css at its 10 pt label size).
    pub fn metric(&self, name: &str) -> Result<f64> {
        let Some(v) = self.theme.tokens.get(name) else {
            return PLAIN_METRICS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| *v)
                .ok_or_else(|| err(format!("the style has no metric {name}")));
        };
        let x = match v {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        };
        x.filter(|x| x.is_finite())
            .ok_or_else(|| err(format!("style metric {name} is not a number")))
    }
    pub fn token(&self, name: &str) -> Option<String> {
        match self.theme.tokens.get(name)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    /// The size, weight and slant the stylesheet gives a label of these classes (base.css text
    /// roles; the first class that sets a property wins, the modifiers before the role).
    fn style_of(&self, role: &str, mods: &[&str]) -> Result<(f64, TextStyle)> {
        if self.theme.stylesheet.trim().is_empty() {
            return Err(err(
                "the theme carries no stylesheet: label typography (bake.label_style) cannot be resolved",
            ));
        }
        let key: Vec<String> = mods
            .iter()
            .map(|m| m.to_string())
            .chain([role.to_string()])
            .collect();
        if let Some(hit) = self.styles.borrow().get(&key) {
            return Ok(hit.clone());
        }
        let classes: Vec<&str> = ["prose"]
            .into_iter()
            .chain(mods.iter().copied())
            .chain([role])
            .collect();
        let style = chartgen_style::label_style(&self.theme.stylesheet, &classes, &BTreeMap::new())
            .map_err(|e| err(format!("label style of {}: {e}", classes.join(" "))))?;
        let out = (style.size, style.text_style());
        self.styles.borrow_mut().insert(key, out.clone());
        Ok(out)
    }

    pub fn request(&self, role: &str, mods: &[&str], content: &TextContent) -> Result<TextRequest> {
        let (size, style) = self.style_of(role, mods)?;
        guides::request_with_style(role, content.clone(), size, None, style, self.theme, self.c)
    }

    /// The measured extent of a text, or a stand-in while it awaits measurement.
    pub fn measure(
        &self,
        role: &str,
        mods: &[&str],
        content: &TextContent,
    ) -> Result<(TextRequest, TextMetrics)> {
        let r = self.request(role, mods, content)?;
        match self.m.entries.get(&r.id) {
            Some(v) => {
                if !v.width.is_finite()
                    || !v.height.is_finite()
                    || v.width < 0.
                    || v.height <= 0.
                    || v.baseline
                        .is_some_and(|b| !b.is_finite() || b < 0. || b > v.height)
                {
                    return Err(err(format!("invalid measurement {}", r.id)));
                }
                let v = v.clone();
                Ok((r, v))
            }
            None => {
                let stand_in = TextMetrics {
                    width: 0.,
                    height: r.font_size,
                    baseline: Some(r.font_size * 0.7),
                };
                self.missing.borrow_mut().insert(r.id.clone(), r.clone());
                Ok((r, stand_in))
            }
        }
    }
    /// Ask for a text the layout may set (the original's `batch.need`), so that it is measured
    /// with the others in one round.
    pub fn need(&self, role: &str, mods: &[&str], content: &TextContent) -> Result<()> {
        self.measure(role, mods, content).map(|_| ())
    }
    pub fn ext(&self, role: &str, mods: &[&str], content: &TextContent) -> Result<Ext> {
        let (_, m) = self.measure(role, mods, content)?;
        Ok(ext_of(&m))
    }
    pub fn width(&self, role: &str, mods: &[&str], content: &TextContent) -> Result<f64> {
        Ok(self.ext(role, mods, content)?.width)
    }
    pub fn plain_ext(&self, role: &str, mods: &[&str], text: &str) -> Result<Ext> {
        self.ext(role, mods, &TextContent::plain(text))
    }

    /// A text placed by its baseline and anchor.
    pub fn text(
        &self,
        role: &str,
        mods: &[&str],
        content: &TextContent,
        x: f64,
        y: f64,
        anchor: &'static str,
    ) -> Result<Placed> {
        let (request, metrics) = self.measure(role, mods, content)?;
        Ok(Placed {
            role: role.into(),
            mods: mods.iter().map(|s| s.to_string()).collect(),
            x,
            y,
            anchor,
            ext: ext_of(&metrics),
            request,
            metrics,
            data: BTreeMap::new(),
            value_of: None,
        })
    }

    /// Run a layout; while texts it read are unmeasured, ask the host for them instead.
    pub fn finish(&self, result: Result<PlacedChart>) -> Result<LayoutStep> {
        let missing: Vec<TextRequest> = self.missing.borrow().values().cloned().collect();
        if !missing.is_empty() {
            if self.c.max_rounds == 0 {
                return Err(err("measurement rounds disabled"));
            }
            return Ok(LayoutStep::NeedMeasurements {
                requests: missing,
                round: 1,
            });
        }
        if !self.m.entries.is_empty() && self.m.context != self.c.measurement_context {
            return Err(err("measurement context differs"));
        }
        Ok(LayoutStep::Complete { chart: result? })
    }
}

pub fn ext_of(m: &TextMetrics) -> Ext {
    let ascent = m.baseline.unwrap_or(m.height);
    Ext {
        width: m.width,
        ascent,
        descent: (m.height - ascent).max(0.),
    }
}

/// Plain text as the original's Typst markup would read; used only for the stand-in
/// identity of texts (the renderers escape plain text themselves).
pub fn plain(s: &str) -> TextContent {
    TextContent::plain(s.split_whitespace().collect::<Vec<_>>().join(" "))
}

// ---- the spec's report findings that a layout itself makes (rules.py) ----

pub fn finding(rule: &str, severity: &str, message: &str, path: Option<&str>) -> Finding {
    let mut f = Finding::new(rule, severity, message);
    f.path = path.map(str::to_owned);
    f
}

/// The original `refusal`: an error, unless overridden (R0-04, the finding is then
/// `overridden` and carries the reason) or the chart is for analysis (a warning).
/// Returns true when the chart must not be drawn.
pub fn refusal(
    spec: &Value,
    findings: &mut Vec<Finding>,
    rule: &str,
    message: &str,
    path: Option<&str>,
    supply: Option<&str>,
) -> bool {
    let reason = spec
        .get("overrides")
        .and_then(Value::as_array)
        .and_then(|o| {
            o.iter()
                .find(|o| o.get("rule").and_then(Value::as_str) == Some(rule))
        })
        .and_then(|o| o.get("reason").and_then(Value::as_str));
    let mut f = finding(rule, "error", message, path);
    f.supply = supply.map(str::to_owned);
    if let Some(reason) = reason {
        f.severity = "overridden".into();
        f.reason = Some(reason.into());
        findings.push(f);
        return false;
    }
    if spec.get("purpose").and_then(Value::as_str) == Some("analysis") {
        f.severity = "warn".into();
        findings.push(f);
        return false;
    }
    findings.push(f);
    true
}

/// A chart the layout refused: no marks, only its findings (the façade reports it refused).
pub fn refused(findings: Vec<Finding>, decisions: Vec<Value>) -> PlacedChart {
    PlacedChart {
        decisions,
        width: 1.,
        height: 1.,
        primitives: vec![],
        findings,
        summary: BTreeMap::new(),
    }
}

// ---- the title block (layout/common.py, D-5 (a)) ----

/// Two-line splits of a plain string where the book would break (common.splits).
pub fn splits(plain: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let bytes: Vec<(usize, char)> = plain.char_indices().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].1 == ' ' {
            let start = bytes[i].0;
            let mut j = i;
            while j < bytes.len() && bytes[j].1 == ' ' {
                j += 1;
            }
            let end = if j < bytes.len() {
                bytes[j].0
            } else {
                plain.len()
            };
            let a = &plain[..start];
            let b = &plain[end..];
            i = j;
            if a.is_empty() || b.is_empty() || a.ends_with('.') {
                continue;
            }
            if b.split(' ').next() == Some("&") || a == "&" {
                continue;
            }
            out.push((a.to_string(), b.to_string()));
        } else {
            i += 1;
        }
    }
    out
}

/// The lines to set a plain string in (common.best_lines): one line if it fits, else the
/// balanced two-line split; None if nothing fits.
pub fn best_lines(
    plain: &str,
    limit: Option<f64>,
    max_lines: usize,
    mut width: impl FnMut(&str) -> Result<f64>,
) -> Result<Option<Vec<String>>> {
    let whole = width(plain)?;
    if limit.is_none_or(|l| whole <= l) {
        return Ok(Some(vec![plain.to_string()]));
    }
    if max_lines < 2 {
        return Ok(None);
    }
    let limit = limit.unwrap();
    let mut best: Option<(f64, Vec<String>)> = None;
    for (a, b) in splits(plain) {
        let w = width(&a)?.max(width(&b)?);
        if w <= limit && best.as_ref().is_none_or(|(bw, _)| w < *bw) {
            best = Some((w, vec![a, b]));
        }
    }
    Ok(best.map(|b| b.1))
}

pub struct TitleBlock {
    pub texts: Vec<Placed>,
    pub last_baseline: Option<f64>,
}

/// The title lines and the descriptive line, from y = 0 down (common.title_block).
pub fn title_block(
    kit: &Kit,
    spec: &Value,
    width: f64,
    findings: &mut Vec<Finding>,
    decisions: &mut Vec<Value>,
) -> Result<TitleBlock> {
    let mut block = TitleBlock {
        texts: vec![],
        last_baseline: None,
    };
    let title = &spec["title"];
    let text = if title.is_object() && title.get("text").is_some() {
        &title["text"]
    } else if title.is_string() {
        title
    } else {
        return Ok(block);
    };
    let Some(content) = guides::content(text)? else {
        return Ok(block);
    };
    let em = kit.metric("--label-size")?;
    let tsize = kit.metric("--title-size")?;
    let pitch = kit.metric("--line-pitch")?;
    let pitch_t = pitch * tsize / em;
    let lines: Vec<TextContent> = match &content {
        TextContent::Plain { text } => {
            let lines = best_lines(text, Some(width), 2, |s| kit.width("title", &[], &plain(s)))?;
            let lines = match lines {
                Some(l) => l,
                None => {
                    findings.push(finding(
                        "CG-1",
                        "error",
                        "the title does not fit the width on two lines; shorten it",
                        Some("title.text"),
                    ));
                    vec![text.clone()]
                }
            };
            lines.iter().map(|s| plain(s)).collect()
        }
        native => {
            if kit.width("title", &[], native)? > width {
                findings.push(finding(
                    "CG-1",
                    "error",
                    &format!(
                        "the title (native markup, which is not wrapped) is wider than the figure's {width:.0} pt; shorten it, or give it as plain text to wrap"
                    ),
                    Some("title.text"),
                ));
            }
            vec![native.clone()]
        }
    };
    let mut y: Option<f64> = None;
    for line in &lines {
        let e = kit.ext("title", &[], line)?;
        let yy = match y {
            None => e.ascent,
            Some(y) => y + pitch_t,
        };
        y = Some(yy);
        block
            .texts
            .push(kit.text("title", &[], line, 0., yy, "start")?);
    }
    if lines.len() > 1 {
        decide(decisions, "R5-26", "the title wraps onto two lines", None);
    }
    if let Some(d) = title.get("descriptive").filter(|d| !d.is_null()) {
        if let Some(dc) = guides::content(d)? {
            let dlines: Vec<TextContent> = match &dc {
                TextContent::Plain { text } => {
                    match best_lines(text, Some(width), 2, |s| {
                        kit.width("subtitle", &[], &plain(s))
                    })? {
                        Some(l) => l.iter().map(|s| plain(s)).collect(),
                        None => {
                            findings.push(finding(
                                "CG-1",
                                "error",
                                &format!("the descriptive line does not fit the figure's {width:.0} pt on two lines; shorten it"),
                                Some("title.descriptive"),
                            ));
                            vec![dc.clone()]
                        }
                    }
                }
                native => {
                    if kit.width("subtitle", &[], native)? > width {
                        findings.push(finding(
                            "CG-1",
                            "error",
                            &format!("the descriptive line (native markup, which is not wrapped) is wider than the figure's {width:.0} pt; shorten it, or give it as plain text to wrap"),
                            Some("title.descriptive"),
                        ));
                    }
                    vec![native.clone()]
                }
            };
            for (i, line) in dlines.iter().enumerate() {
                let yy = y.unwrap_or(0.) + if i == 0 { pitch_t } else { pitch };
                y = Some(yy);
                block
                    .texts
                    .push(kit.text("subtitle", &[], line, 0., yy, "start")?);
            }
            if dlines.len() > 1 {
                decide(
                    decisions,
                    "R5-26",
                    "the descriptive line wraps onto two lines",
                    None,
                );
            }
        }
    }
    block.last_baseline = y;
    Ok(block)
}

// ---- marks with the original's classes and data-* identity ----

pub fn meta(classes: &[&str], data: &[(&str, String)]) -> Metadata {
    Metadata {
        classes: classes.iter().map(|s| s.to_string()).collect(),
        data: data
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        ..Default::default()
    }
}
pub fn fill(colour: &str) -> Paint {
    Paint {
        fill: Some(colour.into()),
        stroke: None,
        stroke_width: 0.,
        opacity: 1.,
    }
}
pub fn stroke(colour: &str, width: f64) -> Paint {
    Paint {
        fill: None,
        stroke: Some(colour.into()),
        stroke_width: width,
        opacity: 1.,
    }
}
pub fn rect(x: f64, y: f64, w: f64, h: f64, paint: Paint, meta: Metadata) -> Primitive {
    Primitive::Rect {
        bounds: Bounds {
            x,
            y,
            width: w.max(0.),
            height: h.max(0.),
        },
        paint,
        meta,
    }
}
pub fn line(x1: f64, y1: f64, x2: f64, y2: f64, paint: Paint, meta: Metadata) -> Primitive {
    Primitive::Line {
        from: Point { x: x1, y: y1 },
        to: Point { x: x2, y: y2 },
        paint,
        meta,
    }
}
/// A library dot mark (the original's `<use href="#mark-dot">`), centred on (x, y). In the
/// compatibility source its colour is the stylesheet's (by its classes); other renderers draw
/// the symbol in `paint`.
pub fn dot(x: f64, y: f64, size: f64, paint: Paint, meta: Metadata) -> Primitive {
    Primitive::Use {
        center: Point { x, y },
        size,
        href: "mark-dot".into(),
        paint,
        meta,
    }
}

/// Path commands of the shared geometry as the scene's.
pub fn scene_commands(commands: &[chartgen_geometry::PathCommand]) -> Vec<PathCommand> {
    use chartgen_geometry::PathCommand as G;
    let p = |x: f64, y: f64| Point { x, y };
    commands
        .iter()
        .map(|c| match *c {
            G::MoveTo { x, y } => PathCommand::MoveTo { to: p(x, y) },
            G::LineTo { x, y } => PathCommand::LineTo { to: p(x, y) },
            G::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => PathCommand::CubicTo {
                c1: p(x1, y1),
                c2: p(x2, y2),
                to: p(x, y),
            },
            G::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                x,
                y,
            } => PathCommand::ArcTo {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to: p(x, y),
            },
            G::Close => PathCommand::Close,
        })
        .collect()
}

/// Exact decimal text as the original writes data-value (`f"{v.normalize():f}"`).
pub fn dec_text(v: Decimal) -> String {
    let n = v.normalize();
    if n.is_zero() {
        "0".into()
    } else {
        n.to_string()
    }
}

pub fn round2(x: f64) -> f64 {
    (x * 100.).round() / 100.
}
pub fn r2(x: f64) -> Value {
    json!(round2(x))
}

/// A paint for a class the stylesheet colours by token, else the text colour.
pub fn token_paint(kit: &Kit, name: &str) -> String {
    kit.token(name).unwrap_or_else(|| kit.theme.text.clone())
}

/// Spec features these families' layouts do not place yet: an explicit error, never a silent
/// omission. (Notes under the chart are the note layout's; panels the panel layout's.)
pub fn guard(plan: &SemanticPlan, data: &PreparedData) -> Result<()> {
    let raw = &plan.spec.raw;
    let kind = plan.chart_type.as_str();
    let active = |v: &Value| match v {
        Value::Null | Value::Bool(false) => false,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    };
    let part_kind = matches!(
        plan.chart_type,
        ChartType::Pie | ChartType::Donut | ChartType::Treemap
    );
    if !part_kind
        && (raw.get("comments").is_some_and(active)
            || raw.pointer("/title/link").is_some_and(active))
    {
        return Err(err(format!("comments need dedicated {kind} anchor placement")).at("comments"));
    }
    if !data.comparisons.is_empty() || raw.pointer("/data/comparisons").is_some_and(active) {
        return Err(
            err(format!("comparison values are not drawn on a {kind}")).at("data.comparisons")
        );
    }
    if raw.pointer("/data/events").is_some_and(active) {
        return Err(err(format!("dated events are not drawn on a {kind}")).at("data.events"));
    }
    if data.irregular.is_some() {
        return Err(err(format!("irregular points are not drawn on a {kind}")));
    }
    if plan.chart_type == ChartType::ActionDots && raw.get("highlight").is_some_and(active) {
        return Err(err("highlight targets need a dedicated action_dots layout").at("highlight"));
    }
    if plan.chart_type == ChartType::Table {
        if let Some(h) = raw.get("highlight").and_then(Value::as_array) {
            for entry in h {
                let method = entry["method"].as_str().unwrap_or("auto");
                if matches!(
                    method,
                    "border" | "glow" | "background" | "icon" | "selective_label"
                ) {
                    return Err(err(format!(
                        "highlight method {method} on a table is drawn by the message layer, which is not ported"
                    ))
                    .at("highlight"));
                }
            }
        }
    }
    Ok(())
}

/// An info for an inset asked of a type that draws none (the original's CG-3).
pub fn inset_info(spec: &Value, kind: &str) -> Option<Finding> {
    let k = spec.pointer("/inset/kind").and_then(Value::as_str)?;
    if matches!(k, "auto" | "none") {
        return None;
    }
    let mut f = finding(
        "CG-3",
        "info",
        &format!(
            "an inset on a {} chart is not drawn",
            kind.replace('_', " ")
        ),
        Some("inset.kind"),
    );
    f.level = "open".into();
    Some(f)
}

/// The paint of a mark's colour class with its emphasis, as the stylesheet's slot rules
/// give it (`.series-k`, `.series-k.highlight` the strong shade, `.context`, `.tint`; the
/// reserved greys). Paints are not semantic: the classes are what the original writes.
pub fn class_paint(kit: &Kit, colour: &str, emphasis: &[&str]) -> String {
    let base = match colour {
        "all-others" => "--all-others".to_string(),
        "missing" => "--missing".to_string(),
        c => format!("--{c}"),
    };
    for (class, suffix) in [
        ("highlight", "-strong"),
        ("context", "-context"),
        ("tint", "-tint"),
    ] {
        if emphasis.contains(&class) {
            if let Some(v) = kit.token(&format!("{base}{suffix}")) {
                return v;
            }
        }
    }
    kit.token(&base)
        .or_else(|| {
            colour
                .strip_prefix("series-")
                .and_then(|k| k.parse::<usize>().ok())
                .and_then(|k| kit.theme.palette.get(k - 1).cloned())
        })
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| kit.theme.text.clone())
}

/// A guided choice departing from the procedure (rules.guided): a warning naming the
/// recommendation, an info when the spec gives its reason (`reasons[path]`).
pub fn guided(
    spec: &Value,
    findings: &mut Vec<Finding>,
    rule: &str,
    path: &str,
    chosen: &str,
    recommended: &str,
    why: &str,
) -> bool {
    if chosen == recommended {
        return false;
    }
    let reason = spec
        .pointer("/reasons")
        .and_then(|r| r.get(path))
        .and_then(Value::as_str);
    let mut f = finding(
        rule,
        if reason.is_some() { "info" } else { "warn" },
        &format!("{path} {chosen} departs from the procedure, which gives {recommended} ({why})"),
        Some(path),
    );
    f.level = "guided".into();
    f.reason = reason.map(str::to_owned);
    f.recommended = Some(recommended.into());
    findings.push(f);
    true
}

pub fn info(rule: &str, message: &str, path: Option<&str>) -> Finding {
    let mut f = finding(rule, "info", message, path);
    if rule == "CG-3" {
        f.level = "open".into();
    }
    f
}

/// A text's content from a spec text value (plain, or native), and its plain reading.
pub fn spec_text(v: &Value) -> Result<Option<TextContent>> {
    crate::guides::content(v)
}

/// The house arrowhead's outline in its own frame (tools/arrowheads.py head_outline): (a, c),
/// a the distance back from the tip, c across (left positive).
pub use figkit_placement::family_geometry::{arc_table, bezier, head_outline, t_at, tangent, unit};

/// Points along a cubic segment from p0, sampled for a polyline path.
pub fn cubic_points(
    p0: (f64, f64),
    c1: (f64, f64),
    c2: (f64, f64),
    e: (f64, f64),
    n: usize,
) -> Vec<Point> {
    (0..=n)
        .map(|j| {
            let t = j as f64 / n as f64;
            let (a, b, c, d) = (
                (1. - t).powi(3),
                3. * (1. - t).powi(2) * t,
                3. * (1. - t) * t * t,
                t.powi(3),
            );
            Point {
                x: a * p0.0 + b * c1.0 + c * c2.0 + d * e.0,
                y: a * p0.1 + b * c1.1 + c * c2.1 + d * e.1,
            }
        })
        .collect()
}

/// A thin curve S–Q–E (a quadratic drawn as a cubic) running into the house arrowhead, whose
/// tip is E (pie.leader, Annotator.arrow): a figkit Curve classed `class`, its head the
/// renderer's `class-head`; with no head when the path is too short for one.
pub fn curve(
    s: (f64, f64),
    e: (f64, f64),
    q: (f64, f64),
    width: f64,
    ink: &str,
    class: &str,
    data: &[(&str, String)],
) -> Primitive {
    let lh = 3.2 + 1.2 * width;
    let outline = head_outline(width);
    let (ts, cum) = arc_table(s, q, e);
    let total = *cum.last().unwrap();
    let pt = |p: (f64, f64)| Point { x: p.0, y: p.1 };
    let paint = Paint {
        fill: Some(ink.into()),
        stroke: Some(ink.into()),
        stroke_width: width,
        opacity: 1.,
    };
    if total < 1.4 * lh {
        return Primitive::Curve {
            start: pt(s),
            segments: vec![CubicSegment {
                c1: pt(s),
                c2: pt(e),
                end: pt(e),
            }],
            head: None,
            paint,
            meta: meta(&[class], data),
        };
    }
    let tn = t_at(&ts, &cum, total - 0.72 * lh);
    let a1 = (s.0 + tn * (q.0 - s.0), s.1 + tn * (q.1 - s.1));
    let a2 = bezier(s, q, e, tn);
    let c1 = (s.0 + 2. / 3. * (a1.0 - s.0), s.1 + 2. / 3. * (a1.1 - s.1));
    let c2 = (
        a2.0 + 2. / 3. * (a1.0 - a2.0),
        a2.1 + 2. / 3. * (a1.1 - a2.1),
    );
    let mut head = vec![];
    for (a, c) in outline {
        let t = t_at(&ts, &cum, total - a);
        let p = bezier(s, q, e, t);
        let u = unit(tangent(s, q, e, t));
        head.push(Point {
            x: p.0 - c * u.1,
            y: p.1 + c * u.0,
        });
    }
    Primitive::Curve {
        start: pt(s),
        segments: vec![CubicSegment {
            c1: pt(c1),
            c2: pt(c2),
            end: pt(a2),
        }],
        head: Some(head),
        paint,
        meta: meta(&[class], data),
    }
}

/// The points along a curve and its head (annotate._curve_points), for occupancy.
pub fn curve_points(p: &Primitive) -> Vec<(f64, f64)> {
    let Primitive::Curve {
        start,
        segments,
        head,
        ..
    } = p
    else {
        return vec![];
    };
    let mut pts = vec![];
    let mut p0 = (start.x, start.y);
    for sgm in segments {
        let (c1, c2, e) = (
            (sgm.c1.x, sgm.c1.y),
            (sgm.c2.x, sgm.c2.y),
            (sgm.end.x, sgm.end.y),
        );
        pts.extend(
            cubic_points(p0, c1, c2, e, 24)
                .into_iter()
                .map(|q| (q.x, q.y)),
        );
        p0 = e;
    }
    if let Some(h) = head {
        for (k, a) in h.iter().enumerate() {
            let b = &h[(k + 1) % h.len()];
            for j in 0..5 {
                let t = j as f64 / 4.;
                pts.push((a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
            }
        }
    }
    pts
}

/// The chooser's resolved sort and sort series (choose.py): auto is largest first for nominal
/// categories (the given order for tables, heatmaps, waterfalls and funnel bars, or where the
/// data's order is fixed); ordered or time categories are never sorted by value.
pub fn choice_sort(spec: &Value, chart_type: ChartType) -> (String, String) {
    let x = &spec["data"]["x"];
    let nominal = x["kind"].as_str() == Some("nominal");
    let given_type = matches!(
        chart_type,
        ChartType::Waterfall | ChartType::Table | ChartType::Heatmap | ChartType::FunnelBars
    );
    let series = spec["data"]["series"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let reference = |s: &Value| matches!(s["role"].as_str(), Some("target" | "reference"));
    let fixed = x["order"].as_str() == Some("fixed")
        || (spec.pointer("/data/parts/order").and_then(Value::as_str) == Some("fixed")
            && series.iter().filter(|s| !reference(s)).count() == 1);
    let recommended = if nominal && !given_type && !fixed {
        "desc"
    } else {
        "given"
    };
    let opt = spec
        .pointer("/chart/sort")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let sort = if !nominal && matches!(opt, "desc" | "asc") {
        "given"
    } else if opt == "auto" {
        recommended
    } else {
        opt
    };
    let sort_by = spec
        .pointer("/chart/sort_by")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            spec.pointer("/insight/targets")
                .and_then(Value::as_array)
                .and_then(|t| t.iter().find_map(|r| r["series"].as_str()))
                .map(str::to_owned)
        })
        .or_else(|| {
            series
                .iter()
                .find(|s| !reference(s))
                .or(series.first())
                .and_then(|s| s["id"].as_str())
                .map(str::to_owned)
        })
        .unwrap_or_default();
    (sort.into(), sort_by)
}

/// A group of the original's figure tree (`<g class=… id=… data-…>`), untranslated.
pub fn group(
    classes: &[&str],
    id: Option<&str>,
    data: &[(&str, String)],
    children: Vec<Primitive>,
) -> Primitive {
    let mut m = meta(classes, data);
    m.id = id.map(str::to_owned);
    Primitive::Group {
        children,
        translate: Point { x: 0., y: 0. },
        clip: None,
        meta: m,
    }
}

/// The primitives under a tree of untranslated groups, in drawing order.
pub fn flatten(prims: &[Primitive]) -> Vec<&Primitive> {
    let mut out = vec![];
    for p in prims {
        match p {
            Primitive::Group { children, .. } => out.extend(flatten(children)),
            other => out.push(other),
        }
    }
    out
}

/// `rules.level_of` as the oracle runs it (GUIDE.md absent): the registry's level letters, else
/// locked.
pub fn level_of(rule: &str) -> &'static str {
    match rule {
        "02 D1" | "02 D2" | "02 D5" | "6.4" | "6.7" => "guided",
        "6.12" | "6.13" | "CG-3" => "open",
        _ => "locked",
    }
}

/// A layout decision for the report (`rep.decide`): rule, level (the rule's, unless given),
/// the words, and its guide sources (none without GUIDE.md).
pub fn decide(
    decisions: &mut Vec<Value>,
    rule: &str,
    text: impl Into<String>,
    level: Option<&str>,
) {
    decisions.push(decision(rule, text, level));
}
pub fn decision(rule: &str, text: impl Into<String>, level: Option<&str>) -> Value {
    json!({"rule": rule, "level": level.unwrap_or(level_of(rule)), "text": text.into(), "guide": []})
}

// The colour-vision step (annotate.Annotator.cvd: R3-20, R15-11, R15-12, the red/green copy and
// the black-and-white diagnostic) is planned once for every family by chartgen-colours.

/// Mark where the colour plan's decisions fall among the layout's (the original layouts make
/// them at this point: pie.order and pie.colours, the treemap's groups, ramps.classify). The
/// façade puts the colour planner's decisions here, once.
pub fn colour_decisions_here(decisions: &mut Vec<Value>) {
    decisions.push(json!({"colours": "here"}));
}
