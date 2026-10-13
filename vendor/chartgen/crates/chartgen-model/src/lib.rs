//! Shared contracts only: no chart policy, layout algorithms, rendering, or I/O.
pub use indexmap::IndexMap;
pub use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub mod rules;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    pub rule: String,
    pub severity: String,
    pub message: String,
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supply: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<String>,
    #[serde(default)]
    pub elements: Vec<String>,
    #[serde(default)]
    pub guide: Vec<String>,
    /// The original rule table's title for the rule (none for rules it does not list).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}
/// A rule's level when a finding gives none, as the original's rules table sets it
/// (`rules.level_of`): every rule is locked except the guided procedures (02 D1, 02 D2,
/// 02 D5, 6.4, 6.7) and the open ones (6.12, 6.13, CG-3).
pub fn rule_level(rule: &str) -> &'static str {
    match rule {
        "02 D1" | "02 D2" | "02 D5" | "6.4" | "6.7" => "guided",
        "6.12" | "6.13" | "CG-3" => "open",
        _ => "locked",
    }
}
impl Finding {
    pub fn new(rule: &str, severity: &str, message: impl Into<String>) -> Self {
        Self {
            rule: rule.into(),
            severity: if severity == "warning" {
                "warn"
            } else {
                severity
            }
            .into(),
            message: message.into(),
            level: rule_level(rule).into(),
            path: None,
            reason: None,
            supply: None,
            recommended: None,
            elements: vec![],
            guide: vec![],
            title: rules::rule_title(rule).map(str::to_owned),
        }
    }
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub chart: String,
    pub style: String,
    pub variant: Option<String>,
    #[serde(rename = "type")]
    pub chart_type: Option<String>,
    pub status: String,
    pub findings: Vec<Finding>,
    pub decisions: Vec<Value>,
    pub overrides: Vec<Value>,
    pub expect: Value,
    pub expect_met: bool,
    pub source: Value,
    pub summary: BTreeMap<String, Value>,
    pub copies: BTreeMap<String, String>,
    pub path: Vec<Value>,
}
impl Report {
    pub fn new(chart: &str, style: &str) -> Self {
        Self {
            chart: chart.into(),
            style: style.into(),
            variant: None,
            chart_type: None,
            status: "drawn".into(),
            findings: vec![],
            decisions: vec![],
            overrides: vec![],
            expect: serde_json::json!({}),
            expect_met: true,
            source: serde_json::json!({}),
            summary: BTreeMap::new(),
            copies: BTreeMap::new(),
            path: vec![],
        }
    }
}

/// Lossless original input retained alongside compiled typed intermediate forms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub raw: Value,
}
impl Spec {
    pub fn id(&self) -> &str {
        self.raw.get("id").and_then(Value::as_str).unwrap_or("?")
    }
    pub fn get(&self, pointer: &str) -> &Value {
        self.raw.pointer(pointer).unwrap_or(&Value::Null)
    }
    pub fn string(&self, pointer: &str, default: &str) -> String {
        self.get(pointer).as_str().unwrap_or(default).into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    Bar,
    Dot,
    Line,
    Step,
    ThinBar,
    Arrow,
    Bumps,
    Pie,
    Donut,
    Treemap,
    WrappedBar,
    StackedBar,
    StackedBar100,
    Waterfall,
    StackedArea,
    StackedArea100,
    LineTotal,
    Table,
    Heatmap,
    ActionDots,
    FunnelBars,
    // Statistical charts (docs/chartgen/STATS.md): the input is samples, points or counts, and
    // `chartgen-statistics` reads it.
    Box,
    Strip,
    Ecdf,
    Scatter,
    Dumbbell,
    IntervalDot,
    IntervalBar,
    StateMatrix,
    Contingency,
}
impl ChartType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Dot => "dot",
            Self::Line => "line",
            Self::Step => "step",
            Self::ThinBar => "thin_bar",
            Self::Arrow => "arrow",
            Self::Bumps => "bumps",
            Self::Pie => "pie",
            Self::Donut => "donut",
            Self::Treemap => "treemap",
            Self::WrappedBar => "wrapped_bar",
            Self::StackedBar => "stacked_bar",
            Self::StackedBar100 => "stacked_bar_100",
            Self::Waterfall => "waterfall",
            Self::StackedArea => "stacked_area",
            Self::StackedArea100 => "stacked_area_100",
            Self::LineTotal => "line_total",
            Self::Table => "table",
            Self::Heatmap => "heatmap",
            Self::ActionDots => "action_dots",
            Self::FunnelBars => "funnel_bars",
            Self::Box => "box",
            Self::Strip => "strip",
            Self::Ecdf => "ecdf",
            Self::Scatter => "scatter",
            Self::Dumbbell => "dumbbell",
            Self::IntervalDot => "interval_dot",
            Self::IntervalBar => "interval_bar",
            Self::StateMatrix => "state_matrix",
            Self::Contingency => "contingency",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bar" => Self::Bar,
            "dot" => Self::Dot,
            "line" | "slope" => Self::Line,
            "step" => Self::Step,
            "thin_bar" => Self::ThinBar,
            "arrow" => Self::Arrow,
            "bumps" => Self::Bumps,
            "pie" => Self::Pie,
            "donut" => Self::Donut,
            "treemap" => Self::Treemap,
            "wrapped_bar" => Self::WrappedBar,
            "stacked_bar" => Self::StackedBar,
            "stacked_bar_100" => Self::StackedBar100,
            "waterfall" => Self::Waterfall,
            "stacked_area" => Self::StackedArea,
            "stacked_area_100" => Self::StackedArea100,
            "line_total" => Self::LineTotal,
            "table" => Self::Table,
            "heatmap" => Self::Heatmap,
            "action_dots" => Self::ActionDots,
            "funnel_bars" => Self::FunnelBars,
            "box" => Self::Box,
            "strip" => Self::Strip,
            "ecdf" => Self::Ecdf,
            "scatter" => Self::Scatter,
            "dumbbell" => Self::Dumbbell,
            "interval_dot" => Self::IntervalDot,
            "interval_bar" => Self::IntervalBar,
            "state_matrix" => Self::StateMatrix,
            "contingency" => Self::Contingency,
            _ => return None,
        })
    }
    /// The statistical charts, whose input `chartgen-statistics` reads.
    pub fn is_statistical(self) -> bool {
        matches!(
            self,
            Self::Box
                | Self::Strip
                | Self::Ecdf
                | Self::Scatter
                | Self::Dumbbell
                | Self::IntervalDot
                | Self::IntervalBar
                | Self::StateMatrix
                | Self::Contingency
        )
    }
    pub fn length_encoded(self) -> bool {
        matches!(
            self,
            Self::Bar
                | Self::IntervalBar
                | Self::ThinBar
                | Self::WrappedBar
                | Self::StackedBar
                | Self::StackedBar100
                | Self::StackedArea
                | Self::StackedArea100
                | Self::Waterfall
                | Self::FunnelBars
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Datum {
    pub value: Option<Decimal>,
    pub status: String,
    pub certainty: String,
    pub complete: bool,
    pub note: Option<String>,
    pub limit: Option<Decimal>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub id: String,
    pub name: String,
    pub values: Vec<Datum>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedData {
    pub categories: Vec<String>,
    pub x_kind: String,
    pub series: Vec<Series>,
    pub measure: Value,
    /// Exact irregular event/state points, preserving coincident events.
    #[serde(default)]
    pub irregular: Option<Vec<Vec<TimedDatum>>>,
    /// Comparison values remain separate from the plotted series until grammar selection.
    #[serde(default)]
    pub comparisons: Vec<PreparedComparison>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedComparison {
    pub id: String,
    pub kind: String,
    pub label: String,
    /// One value, one per category, or empty for a computed mean awaiting preparation.
    pub values: Vec<Datum>,
    pub render: String,
    pub valid: bool,
    pub series: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedDatum {
    pub time: String,
    /// Seconds from the Unix epoch, independent of host timezone.
    pub position: Decimal,
    pub datum: Datum,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFacts {
    pub min: Option<Decimal>,
    pub max: Option<Decimal>,
    pub known: usize,
    pub missing: usize,
    pub censored: usize,
    pub series: usize,
    pub categories: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub chart_type: Option<ChartType>,
    pub orientation: Orientation,
    pub order: Vec<usize>,
    pub findings: Vec<Finding>,
    pub decisions: Vec<Value>,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub mark: ChartType,
    /// Explicit data source for series, comparison marks, or a derived total.
    #[serde(default)]
    pub source: Option<LayerSource>,
    pub series: Vec<usize>,
    pub x_channel: String,
    pub y_channel: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayerSource {
    Series { indices: Vec<usize> },
    Comparison { index: usize, render: String },
    DerivedTotal,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticPlan {
    pub spec: Spec,
    pub chart_type: ChartType,
    pub orientation: Orientation,
    pub order: Vec<usize>,
    pub layers: Vec<Layer>,
    pub coordinates: String,
    #[serde(default)]
    pub colors: ColorPlan,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColorPlan {
    /// One palette slot per prepared series; None selects its semantic single/role paint.
    #[serde(default)]
    pub series_slots: Vec<Option<u8>>,
    /// One palette slot per prepared category for single-series part charts.
    #[serde(default)]
    pub category_slots: Vec<Option<u8>>,
    /// A later grouped part uses a tint of its group's first colour.
    #[serde(default)]
    pub category_tints: Vec<bool>,
    /// The original's colour classes of each prepared series, its colour class first (`single`,
    /// `series-k`, a reserved role class such as `average` or `all-others`, `seq-k` for an ordered
    /// stack part), then modifiers (`tint`, `shade-light`, `shade-dark`). Empty: no palette colour
    /// (a heatmap's rows, a table). The vector is empty when no colour plan was compiled.
    #[serde(default)]
    pub series_classes: Vec<Vec<String>>,
    /// The colour classes of each prepared category where the parts of one total are coloured
    /// (pie and donut slices, treemap tiles, wrapped bars). Empty: the part has no mark (a zero).
    #[serde(default)]
    pub category_classes: Vec<Vec<String>>,
    /// Parts that are series in drawing order (series indices): a stack's from the base up,
    /// several pies' slices clockwise; empty where the source order applies.
    #[serde(default)]
    pub series_order: Vec<usize>,
    /// One total's parts in drawing order (category indices: a pie's slices clockwise, a
    /// treemap's tiles group by group); empty where `SemanticPlan::order` applies.
    #[serde(default)]
    pub category_order: Vec<usize>,
    /// The classed quantity ramp of a heatmap's cells.
    #[serde(default)]
    pub ramp: Option<RampPlan>,
    /// Colour classes of single marks that differ from their series' or category's, indexed
    /// `[series][category]` (good and bad sides of bars and dots against the dividing value);
    /// an empty list keeps the mark's own colour. Empty when no mark differs.
    #[serde(default)]
    pub mark_classes: Vec<Vec<Vec<String>>>,
}
/// A colour ramp cut into classes (the original's `ramps.Classes`, or an unclassed `ramps.Ramp`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RampPlan {
    /// `sequential`, `diverging`, or `categorical` (only through an override).
    pub kind: String,
    /// `linear`, `log`, `natural`, `quantile`, `custom`; empty for an unclassed ramp.
    pub method: String,
    /// The classes of each cell, `[series][category]`, without `labelled` (added by a layout that
    /// pales its cells): a value not known `missing`; a censored value its own step beyond the
    /// ramp's end (the style's default `step` treatment), else `censored`.
    pub cells: Vec<Vec<Vec<String>>>,
    /// The key's entries in drawing order.
    pub key: Vec<RampKeyEntry>,
    /// Where a diverging key's tick stands (an entry's index; a boundary as k − 0.5).
    pub tick: Option<f64>,
    /// The classes of a censored value's own step: below a limit, above a limit.
    pub censored_below: Vec<String>,
    pub censored_above: Vec<String>,
    /// The classes' inner breaks (of the distance from the dividing value when diverging).
    #[serde(default)]
    pub inner: Vec<Decimal>,
    /// The classed domain's ends (None: open); an unclassed ramp's extent.
    #[serde(default)]
    pub lo: Option<Decimal>,
    #[serde(default)]
    pub hi: Option<Decimal>,
    /// A diverging ramp's dividing value.
    #[serde(default)]
    pub dividing: Option<Decimal>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RampKeyEntry {
    pub classes: Vec<String>,
    pub lo: Option<Decimal>,
    pub hi: Option<Decimal>,
    /// A value written under the entry's middle (zero's own class, the dividing value).
    pub single: Option<Decimal>,
    /// A censored step: (`below` or `above`, its limit).
    pub censored: Option<(String, Decimal)>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainedScale {
    pub lo: Decimal,
    pub hi: Decimal,
    pub step: Decimal,
    pub ticks: Vec<Decimal>,
    pub mode: String,
}
impl TrainedScale {
    pub fn fraction(&self, value: Decimal) -> Result<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if self.hi <= self.lo {
            return Err(Error::new("scale", "scale domain has no extent"));
        }
        (value - self.lo)
            .checked_div(self.hi - self.lo)
            .and_then(|x| x.to_f64())
            .ok_or_else(|| Error::new("scale", "value cannot be mapped to finite geometry"))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub font_family: String,
    pub font_size: f64,
    pub text: String,
    pub grid: String,
    pub background: String,
    pub palette: Vec<String>,
    pub tokens: BTreeMap<String, Value>,
    /// The flattened stylesheet (literal CSS, as figkit's `Style.css`) from which label
    /// typography is resolved (`chartgen_style::label_style`, the original's `bake.label_style`).
    #[serde(default)]
    pub stylesheet: String,
}
impl Default for Theme {
    /// The original's default style, `plain` (charts/style/plain.css with base.css), flattened.
    /// The asset is generated from the bundled original stylesheets by `chartgen-style`, whose
    /// tests check that it equals `Style::theme()` for `plain`.
    fn default() -> Self {
        serde_json::from_str(include_str!("../assets/theme-plain.json"))
            .expect("bundled plain theme is valid")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConstraints {
    pub width: f64,
    pub height: Option<f64>,
    pub measurement_context: String,
    pub max_rounds: u32,
    /// How a grid of panels steers this chart's layout; None for a chart laid out on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<LayoutHooks>,
}
impl Default for LayoutConstraints {
    fn default() -> Self {
        Self {
            // The original's figure width when a spec gives none (spec.py LayoutOpts.width).
            width: 368.,
            height: None,
            measurement_context: "default".into(),
            max_rounds: 8,
            hooks: None,
        }
    }
}
fn hook_true() -> bool {
    true
}
fn hook_full() -> String {
    "full".into()
}
/// How a grid of panels (small multiples, a merged chart) steers one panel's layout: the
/// original's `ctx.hooks`. Coordinates are the panel chart's own (its x from its left edge).
///
/// - `plot_left`, `plot_right`: the plot's edges (the value scale's labels stand left of
///   `plot_left` when drawn); `plot_height`: the plot's height (vertical plots).
/// - `slot`: the width of a line chart's period slot; `band`: the pitch of horizontal bars' rows.
/// - `title`: whether the panel draws its title block (never, in a grid); `axis`: whether it
///   draws its category or time labels; `event_labels`: whether its dates marked on the axis
///   carry their labels (lines alone otherwise).
/// - `scale_labels`: the value scale's labels the panel shows: `full`; `kept` (the top label's
///   number with its currency, magnitude and suffix, no description: the other rows' first
///   column); `none` (no unit phrase at all).
///
/// A layout given hooks it does not honour must fail explicitly; a panel grid checks the plot
/// a layout reports (summary `plot`, `slot`, `band`) against the hooks it gave.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutHooks {
    #[serde(default)]
    pub plot_left: Option<f64>,
    #[serde(default)]
    pub plot_right: Option<f64>,
    #[serde(default)]
    pub plot_height: Option<f64>,
    #[serde(default)]
    pub slot: Option<f64>,
    #[serde(default)]
    pub band: Option<f64>,
    #[serde(default = "hook_true")]
    pub title: bool,
    #[serde(default = "hook_true")]
    pub axis: bool,
    #[serde(default)]
    pub event_labels: Option<bool>,
    #[serde(default = "hook_full")]
    pub scale_labels: String,
}
impl Default for LayoutHooks {
    fn default() -> Self {
        Self {
            plot_left: None,
            plot_right: None,
            plot_height: None,
            slot: None,
            band: None,
            title: true,
            axis: true,
            event_labels: None,
            scale_labels: "full".into(),
        }
    }
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
    /// The value a text writes, as (series id, x): a table's cell, a value label, the note in
    /// a value's empty slot. An in-process identity (the original's `ctx.value_texts`) by which
    /// notes mark the text of one value; never written to an output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_of: Option<(String, String)>,
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
pub struct PlacedChart {
    pub width: f64,
    pub height: f64,
    pub primitives: Vec<Primitive>,
    pub findings: Vec<Finding>,
    pub summary: BTreeMap<String, Value>,
    /// The layout's decisions (`{rule, level, text, guide}`), in the order made; the report lists
    /// them after the policy's and the scale's, as the original's layouts record theirs.
    #[serde(default)]
    pub decisions: Vec<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LayoutStep {
    NeedMeasurements {
        requests: Vec<TextRequest>,
        round: u32,
    },
    Complete {
        chart: PlacedChart,
    },
    /// The layout refuses the chart: a rule the original decides at layout time (a step
    /// chart without an end date, a time axis too narrow for its labels). The findings
    /// carry the refusal; no chart is drawn.
    Refused {
        findings: Vec<Finding>,
    },
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

pub use figkit_model::legacy::{Measurements, TextMetrics, TextRequest};
pub use figkit_model::{
    Bounds, CubicSegment, Error, Paint, PathCommand, Point, Result, TextContent, TextStyle,
};

/// Chart datum associations indexed by snapshot-local references, independent of exported IDs.
pub type DatumReferenceMap = BTreeMap<figkit_model::ElementId, (String, String)>;
impl Metadata {
    pub fn to_legacy(&self) -> figkit_model::legacy::Metadata {
        figkit_model::legacy::Metadata {
            id: self.id.clone(),
            role: self.role.clone(),
            classes: self.classes.clone(),
            data: self.data.clone(),
            attrs: self.attrs.clone(),
            joint: self.joint.clone(),
        }
    }
}
impl Primitive {
    /// Remove chart datum associations at the explicit house-format boundary.
    pub fn to_legacy(&self) -> figkit_model::legacy::Primitive {
        use figkit_model::legacy::Primitive as L;
        match self {
            Self::Rect {
                bounds,
                paint,
                meta,
            } => L::Rect {
                bounds: bounds.clone(),
                paint: paint.clone(),
                meta: meta.to_legacy(),
            },
            Self::Line {
                from,
                to,
                paint,
                meta,
            } => L::Line {
                from: from.clone(),
                to: to.clone(),
                paint: paint.clone(),
                meta: meta.to_legacy(),
            },
            Self::Path {
                points,
                closed,
                paint,
                meta,
            } => L::Path {
                points: points.clone(),
                closed: closed.clone(),
                paint: paint.clone(),
                meta: meta.to_legacy(),
            },
            Self::Circle {
                center,
                radius,
                paint,
                meta,
            } => L::Circle {
                center: center.clone(),
                radius: radius.clone(),
                paint: paint.clone(),
                meta: meta.to_legacy(),
            },
            Self::Text {
                position,
                request,
                metrics,
                anchor,
                meta,
            } => L::Text {
                position: position.clone(),
                request: request.clone(),
                metrics: metrics.clone(),
                anchor: anchor.clone(),
                meta: meta.to_legacy(),
            },
            Self::Group {
                children,
                translate,
                clip,
                meta,
            } => L::Group {
                children: children.iter().map(Self::to_legacy).collect(),
                translate: translate.clone(),
                clip: clip.clone(),
                meta: meta.to_legacy(),
            },
            Self::Use {
                center,
                size,
                href,
                paint,
                meta,
            } => L::Use {
                center: center.clone(),
                size: size.clone(),
                href: href.clone(),
                paint: paint.clone(),
                meta: meta.to_legacy(),
            },
            Self::Curve {
                start,
                segments,
                head,
                paint,
                meta,
            } => L::Curve {
                start: start.clone(),
                segments: segments.clone(),
                head: head.clone(),
                paint: paint.clone(),
                meta: meta.to_legacy(),
            },
            Self::Wedge {
                center,
                radius,
                inner_radius,
                start_angle,
                end_angle,
                inset,
                paint,
                meta,
            } => L::Wedge {
                center: center.clone(),
                radius: radius.clone(),
                inner_radius: inner_radius.clone(),
                start_angle: start_angle.clone(),
                end_angle: end_angle.clone(),
                inset: inset.clone(),
                paint: paint.clone(),
                meta: meta.to_legacy(),
            },
            Self::Shape {
                commands,
                paint,
                meta,
            } => L::Shape {
                commands: commands.clone(),
                paint: paint.clone(),
                meta: meta.to_legacy(),
            },
        }
    }
}
impl Scene {
    pub fn to_legacy(&self) -> figkit_model::legacy::Scene {
        figkit_model::legacy::Scene {
            version: self.version,
            width: self.width,
            height: self.height,
            title: self.title.clone(),
            description: self.description.clone(),
            background: self.background.clone(),
            primitives: self.primitives.iter().map(Primitive::to_legacy).collect(),
            comment: self.comment.clone(),
            defs: self.defs.clone(),
        }
    }
    /// Return chart associations keyed by deterministic structural references for this snapshot.
    /// References deliberately do not reuse authored SVG IDs.
    pub fn datum_references(&self) -> DatumReferenceMap {
        fn walk(nodes: &[Primitive], prefix: &str, out: &mut DatumReferenceMap) {
            for (i, node) in nodes.iter().enumerate() {
                let key = format!("{prefix}/{i}");
                if let Some(value) = &node.meta().value_of {
                    out.insert(figkit_model::ElementId(key.clone()), value.clone());
                }
                if let Primitive::Group { children, .. } = node {
                    walk(children, &key, out);
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.primitives, "element", &mut out);
        out
    }
}
