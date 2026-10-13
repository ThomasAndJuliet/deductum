//! The statistical charts (STATS.md): box, strip, interval dots and bars, and the paired
//! contrast, laid out on one frame of category rows and a value axis; the two-axis charts
//! (ECDF, scatter) are in `statistical_xy.rs`, the grids (state matrix, contingency table) in
//! `statistical_grid.rs`.
//!
//! The input is read by `chartgen-statistics`, as the policy and scale stages read it. The value
//! axis is the scale trained by `chartgen-scales`, so its zero, breathing room and labels follow
//! the library's rules, and every text is measured by the host through the `Kit`.
use crate::guides;
use crate::kit::{self, Kit, Placed};
use chartgen_model::*;
use chartgen_statistics::{
    self as statistics, Model,
    box_plot::{BoxModel, Points},
    distribution::StripModel,
    interval::IntervalModel,
    notes::Notes,
    paired::{Good, PairedModel, View},
    read::SeriesMeta,
};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn layout(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
) -> Result<LayoutStep> {
    if !c.width.is_finite()
        || c.width <= 120.
        || c.height.is_some_and(|h| !h.is_finite() || h <= 60.)
    {
        return Err(kit::err("invalid statistical chart size"));
    }
    if data.series.is_empty() || theme.palette.is_empty() {
        return Err(kit::err("a statistical chart needs series and a palette"));
    }
    kit::guard(plan, data)?;
    unsupported(plan)?;
    let kit = Kit::new(theme, c, m);
    let mut notes = Notes::default();
    let model = statistics::parse(&plan.spec.raw, plan.chart_type.as_str(), &mut notes)
        .ok_or_else(|| {
            kit::err("the statistical input was refused: the policy stage states why")
        })?;
    let result = match &model {
        Model::Box(b) => rows(&kit, plan, data, scale, &Rows::from_box(b)).map(|(mut f, order)| {
            draw_box(&kit, plan, b, &mut f, &order);
            f.finish("box", &order, plan)
        }),
        Model::Strip(s) => {
            rows(&kit, plan, data, scale, &Rows::from_strip(s)).map(|(mut f, order)| {
                draw_strip(&kit, plan, s, &mut f, &order);
                f.finish("strip", &order, plan)
            })
        }
        Model::Interval(i) => {
            rows(&kit, plan, data, scale, &Rows::from_interval(i)).map(|(mut f, order)| {
                draw_interval(&kit, plan, i, &mut f, &order);
                f.finish(
                    if i.bar {
                        "interval_bar"
                    } else {
                        "interval_dot"
                    },
                    &order,
                    plan,
                )
            })
        }
        Model::Paired(p) => {
            rows(&kit, plan, data, scale, &Rows::from_paired(p)).map(|(mut f, order)| {
                draw_paired(&kit, plan, p, &mut f, &order);
                f.finish("dumbbell", &order, plan)
            })
        }
        Model::Ecdf(e) => crate::statistical_xy::layout_ecdf(&kit, plan, data, scale, e),
        Model::Scatter(s) => crate::statistical_xy::layout_scatter(&kit, plan, data, scale, s),
        Model::States(s) => crate::statistical_grid::layout_matrix(&kit, plan, data, s),
        Model::Contingency(t) => crate::statistical_grid::layout_contingency(&kit, plan, data, t),
    };
    // a layout that read an unmeasured text asks the host for it (Kit::finish)
    let result = result.map(|mut chart| {
        chart.summary.insert("statistics".into(), model.summary());
        chart
    });
    kit.finish(result)
}

/// Spec features these charts do not place: an explicit error, never a silent omission.
fn unsupported(plan: &SemanticPlan) -> Result<()> {
    let raw = &plan.spec.raw;
    let active = |v: &Value| match v {
        Value::Null | Value::Bool(false) => false,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty() && o.get("kind").and_then(Value::as_str) != Some("none"),
        _ => true,
    };
    for key in [
        "highlight",
        "facet",
        "facets",
        "inset",
        "insets",
        "annotations",
        "panels",
        "bands",
        "reference_lines",
    ] {
        if raw.get(key).is_some_and(active) {
            return Err(kit::err(format!(
                "{key} is not placed on a {} chart",
                plan.chart_type.as_str()
            ))
            .at(key));
        }
    }
    Ok(())
}

// ---- shared pieces -------------------------------------------------------------------------

pub(crate) fn is_horizontal(plan: &SemanticPlan) -> bool {
    plan.spec
        .raw
        .pointer("/chart/orientation")
        .and_then(Value::as_str)
        != Some("vertical")
}

pub(crate) fn display_order(plan: &SemanticPlan, n: usize) -> Vec<usize> {
    if plan.order.len() == n {
        plan.order.clone()
    } else {
        (0..n).collect()
    }
}

pub(crate) struct Ink {
    pub fill: String,
    pub strong: String,
}

pub(crate) fn series_ink(kit: &Kit, plan: &SemanticPlan, si: usize) -> Ink {
    let class = plan
        .colors
        .series_classes
        .get(si)
        .and_then(|c| c.first())
        .map_or("single", String::as_str);
    Ink {
        fill: kit::class_paint(kit, class, &[]),
        strong: kit::class_paint(kit, class, &["highlight"]),
    }
}

pub(crate) fn series_class(plan: &SemanticPlan, si: usize) -> String {
    plan.colors
        .series_classes
        .get(si)
        .and_then(|c| c.first())
        .cloned()
        .unwrap_or_else(|| "single".into())
}

pub(crate) fn paint(fill: Option<&str>, stroke: Option<&str>, width: f64) -> Paint {
    Paint {
        fill: fill.map(str::to_owned),
        stroke: stroke.map(str::to_owned),
        stroke_width: width,
        opacity: 1.,
    }
}

pub(crate) fn px(v: Decimal) -> f64 {
    v.to_f64().unwrap_or(0.)
}

pub(crate) fn dec(v: f64) -> Decimal {
    Decimal::from_f64_retain(v).unwrap_or_default()
}

/// Greedy wrapping of a note to `width`, measuring each word once (so the host answers in one
/// round) and summing; the wrapped lines are measured when they are placed.
pub(crate) fn wrap(kit: &Kit, role: &str, text: &str, width: f64) -> Result<Vec<String>> {
    // a blank text has no height to measure: the space is the difference of two words
    let space =
        (kit.plain_ext(role, &[], "n n")?.width - kit.plain_ext(role, &[], "nn")?.width).max(0.);
    let mut lines: Vec<String> = vec![];
    let (mut cur, mut used) = (String::new(), 0.);
    for word in text.split_whitespace() {
        let w = kit.plain_ext(role, &[], word)?.width;
        if !cur.is_empty() && used + space + w > width {
            lines.push(std::mem::take(&mut cur));
            used = 0.;
        }
        if !cur.is_empty() {
            cur.push(' ');
            used += space;
        }
        cur.push_str(word);
        used += w;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    Ok(lines)
}

pub(crate) fn text_at(
    kit: &Kit,
    role: &str,
    s: &str,
    x: f64,
    centre_y: f64,
    anchor: &'static str,
) -> Result<Placed> {
    let content = TextContent::plain(s);
    let e = kit.ext(role, &[], &content)?;
    // the baseline that centres the line's ink on `centre_y`
    kit.text(
        role,
        &[],
        &content,
        x,
        centre_y + (e.ascent - e.descent) / 2.,
        anchor,
    )
}

#[derive(Clone)]
pub(crate) enum Glyph {
    Swatch,
    Mark(&'static str),
    /// A line key; the number picks its dash pattern (0 solid).
    Line(usize),
}

#[derive(Clone)]
pub(crate) struct Key {
    pub glyph: Glyph,
    pub fill: String,
    pub strong: String,
    pub text: TextContent,
}

pub(crate) const MARKS: [&str; 6] = [
    "mark-dot",
    "mark-square",
    "mark-triangle",
    "mark-x",
    "mark-plus",
    "mark-asterisk",
];

/// One mark of the house library, in the palette's ink (a cross or plus is a stroke).
pub(crate) fn mark(href: &str, x: f64, y: f64, size: f64, ink: &Ink, meta: Metadata) -> Primitive {
    let stroked = matches!(href, "mark-x" | "mark-plus" | "mark-asterisk");
    Primitive::Use {
        center: Point { x, y },
        size,
        href: href.into(),
        paint: if stroked {
            paint(None, Some(&ink.strong), 1.6)
        } else {
            paint(Some(&ink.fill), Some(&ink.strong), 1.)
        },
        meta,
    }
}

/// The legend, in rows that wrap at `width`; returns its primitives and its height.
pub(crate) fn legend(
    kit: &Kit,
    keys: &[Key],
    x0: f64,
    y0: f64,
    width: f64,
) -> Result<(Vec<Primitive>, f64)> {
    if keys.is_empty() {
        return Ok((vec![], 0.));
    }
    let key = kit.metric("--legend-key")?;
    let gap = kit.metric("--legend-entry-gap")?;
    let pitch = kit.metric("--line-pitch")?;
    let mut out = vec![];
    let (mut x, mut row) = (x0, 0usize);
    for k in keys {
        let e = kit.ext("legend-text", &[], &k.text)?;
        let need = 2. * key + 3. + e.width;
        if x > x0 && x + need > x0 + width {
            x = x0;
            row += 1;
        }
        let cy = y0 + row as f64 * pitch + pitch / 2.;
        let m = kit::meta(&["legend-key"], &[]);
        let ink = Ink {
            fill: k.fill.clone(),
            strong: k.strong.clone(),
        };
        match &k.glyph {
            Glyph::Swatch => out.push(kit::rect(
                x,
                cy - key,
                2. * key,
                2. * key,
                paint(Some(&k.fill), Some(&k.strong), 1.),
                kit::meta(&["legend-swatch"], &[]),
            )),
            Glyph::Mark(h) => out.push(mark(h, x + key, cy, 2. * key, &ink, m)),
            Glyph::Line(d) => {
                for (a, b) in dash_runs(0., 2. * key, *d) {
                    out.push(kit::line(
                        x + a,
                        cy,
                        x + b,
                        cy,
                        paint(None, Some(&k.strong), 2.5),
                        kit::meta(&["legend-line"], &[]),
                    ));
                }
            }
        }
        let placed = kit.text(
            "legend-text",
            &[],
            &k.text,
            x + 2. * key + 3.,
            cy + (e.ascent - e.descent) / 2.,
            "start",
        )?;
        out.push(placed.primitive());
        x += need + gap;
    }
    Ok((out, (row + 1) as f64 * pitch))
}

/// The title block, and the y where the figure's content starts below it.
pub(crate) fn title(
    kit: &Kit,
    plan: &SemanticPlan,
    findings: &mut Vec<Finding>,
    decisions: &mut Vec<Value>,
) -> Result<(Vec<Primitive>, f64)> {
    let em = kit.metric("--label-size")?;
    let block = kit::title_block(kit, &plan.spec.raw, kit.c.width, findings, decisions)?;
    let top = match block.last_baseline {
        Some(b) => b + kit.metric("--title-gap")? - 0.333 * em,
        None => 0.,
    };
    Ok((
        vec![kit::group(
            &["title"],
            None,
            &[],
            block.texts.iter().map(Placed::primitive).collect(),
        )],
        top,
    ))
}

/// The note under the chart that states how its marks were computed: wrapped lines in the
/// note's role, returning the primitives and the height they take.
pub(crate) fn note(
    kit: &Kit,
    text: &str,
    x: f64,
    y0: f64,
    width: f64,
) -> Result<(Vec<Primitive>, f64)> {
    let pitch = kit.metric("--line-pitch")? * 0.9;
    let lines = wrap(kit, "chart-note", text, width)?;
    let mut out = vec![];
    for (i, l) in lines.iter().enumerate() {
        let cy = y0 + i as f64 * pitch + pitch / 2.;
        out.push(text_at(kit, "chart-note", l, x, cy, "start")?.primitive());
    }
    Ok((
        vec![kit::group(&["chart-note"], None, &[], out)],
        lines.len() as f64 * pitch,
    ))
}

/// The value axis' formatted labels, as the generic layout places them.
pub(crate) fn formatted(
    plan: &SemanticPlan,
    scale: &TrainedScale,
    measure: Option<&Value>,
) -> Result<chartgen_format::ScaleLabels> {
    match measure {
        Some(m) => chartgen_format::scale_labels_with_measure(&plan.spec, m, scale),
        None => chartgen_format::scale_labels(&plan.spec, scale),
    }
}

// ---- the row frame -------------------------------------------------------------------------

/// What a row chart asks of the frame.
pub(crate) struct Rows {
    pub keys: Vec<(usize, Glyph)>,
    pub slots: usize,
    pub slot_h: f64,
    /// `[category][slot]` text in the gutter beside a row (n, successes, a difference).
    pub gutter: Vec<Vec<Option<String>>>,
    pub note: String,
    /// The series each slot draws, for its ink.
    pub slot_series: Vec<usize>,
    /// Draw a line at zero through the plot (a difference from nothing).
    pub zero_line: bool,
    /// The measure the axis is labelled with, when it is not the chart's (a difference).
    pub measure: Option<Value>,
}

impl Rows {
    fn base() -> Self {
        Self {
            keys: vec![],
            slots: 1,
            slot_h: 22.,
            gutter: vec![],
            note: String::new(),
            slot_series: vec![],
            zero_line: false,
            measure: None,
        }
    }
    fn from_box(b: &BoxModel) -> Self {
        let mut r = Self::base();
        r.slots = b.series.len();
        r.slot_series = (0..b.series.len()).collect();
        r.keys = (0..b.series.len()).map(|i| (i, Glyph::Swatch)).collect();
        r.gutter = (0..b.labels.len())
            .map(|g| {
                (0..b.series.len())
                    .map(|s| Some(format!("n={}", b.cells[s][g].stats.n)))
                    .collect()
            })
            .collect();
        r.note = b.note();
        r
    }
    fn from_strip(s: &StripModel) -> Self {
        let mut r = Self::base();
        r.slots = s.series.len();
        r.slot_h = 26.;
        r.slot_series = (0..s.series.len()).collect();
        r.keys = (0..s.series.len()).map(|i| (i, Glyph::Swatch)).collect();
        r.gutter = (0..s.labels.len())
            .map(|g| {
                (0..s.series.len())
                    .map(|k| Some(format!("n={}", s.samples[k][g].len())))
                    .collect()
            })
            .collect();
        r.note = s.note();
        r
    }
    fn from_interval(m: &IntervalModel) -> Self {
        let mut r = Self::base();
        r.slots = m.series.len();
        r.slot_series = (0..m.series.len()).collect();
        r.keys = (0..m.series.len()).map(|i| (i, Glyph::Swatch)).collect();
        r.gutter = (0..m.labels.len())
            .map(|g| {
                (0..m.series.len())
                    .map(|s| Some(m.estimates[s][g].size.clone()).filter(|t| !t.is_empty()))
                    .collect()
            })
            .collect();
        r.note = m.note();
        r
    }
    fn from_paired(p: &PairedModel) -> Self {
        let mut r = Self::base();
        r.slots = 1;
        r.slot_h = if p.view == View::Pair { 26. } else { 24. };
        r.slot_series = vec![0];
        if p.view == View::Pair {
            r.keys = vec![(0, Glyph::Mark(MARKS[0])), (1, Glyph::Mark(MARKS[1]))];
        }
        let pp = p.measure.percent;
        r.gutter = (0..p.labels.len())
            .map(|g| vec![Some(diff_text(p.diffs[g], pp))])
            .collect();
        r.note = p.note();
        r.zero_line = p.view == View::Difference;
        if p.view == View::Difference {
            // the axis counts the difference, in percentage points when the measure is a percentage
            r.measure = Some(
                json!({"name": format!("difference, {}", p.diff_name), "kind": "change", "unit": {"suffix": if pp { " pp" } else { "" }}}),
            );
        }
        r
    }
}

pub(crate) fn diff_text(d: f64, percent: bool) -> String {
    let s = chartgen_format_plain(d);
    format!(
        "{}{s}{}",
        if d > 0. {
            "+"
        } else if d < 0. {
            "\u{2212}"
        } else {
            ""
        },
        if percent { " pp" } else { "" }
    )
    .replace("\u{2212}-", "\u{2212}")
}

fn chartgen_format_plain(d: f64) -> String {
    let mut s = format!("{:.1}", d.abs());
    if s.ends_with(".0") {
        s.truncate(s.len() - 2);
    }
    s
}

/// The frame a row chart's marks are drawn on.
pub(crate) struct Frame {
    pub horizontal: bool,
    pub plot: Bounds,
    scale: TrainedScale,
    pitch: f64,
    block: f64,
    slot_h: f64,
    pub behind: Vec<Primitive>,
    pub marks: Vec<Primitive>,
    pub front: Vec<Primitive>,
    pub width: f64,
    pub height: f64,
    pub findings: Vec<Finding>,
    pub decisions: Vec<Value>,
}

impl Frame {
    /// The category-axis coordinate of the centre of `slot` in display row `row`.
    pub fn cat(&self, row: usize, slot: usize) -> f64 {
        let start = if self.horizontal {
            self.plot.y
        } else {
            self.plot.x
        };
        start
            + row as f64 * self.pitch
            + (self.pitch - self.block) / 2.
            + (slot as f64 + 0.5) * self.slot_h
    }
    /// The value-axis coordinate of a value.
    pub fn val(&self, v: f64) -> f64 {
        let f = self.scale.fraction(dec(v)).unwrap_or(0.);
        if self.horizontal {
            self.plot.x + f * self.plot.width
        } else {
            self.plot.y + (1. - f) * self.plot.height
        }
    }
    /// A point from a category-axis and a value-axis coordinate.
    pub fn at(&self, cat: f64, val: f64) -> (f64, f64) {
        if self.horizontal {
            (val, cat)
        } else {
            (cat, val)
        }
    }
    pub fn line(&mut self, c0: f64, v0: f64, c1: f64, v1: f64, p: Paint, m: Metadata) {
        let ((x1, y1), (x2, y2)) = (self.at(c0, v0), self.at(c1, v1));
        self.marks.push(kit::line(x1, y1, x2, y2, p, m));
    }
    /// A rectangle spanning two values and `half` either side of a category coordinate.
    pub fn bar(&mut self, c: f64, half: f64, v0: f64, v1: f64, p: Paint, m: Metadata) {
        let (a, b) = (self.at(c - half, v0.min(v1)), self.at(c + half, v0.max(v1)));
        let (x, y) = (a.0.min(b.0), a.1.min(b.1));
        self.marks
            .push(kit::rect(x, y, (a.0 - b.0).abs(), (a.1 - b.1).abs(), p, m));
    }
    pub fn finish(mut self, kind: &str, order: &[usize], plan: &SemanticPlan) -> PlacedChart {
        let root = vec![
            kit::group(&["frame"], None, &[], std::mem::take(&mut self.behind)),
            kit::group(&["marks"], None, &[], std::mem::take(&mut self.marks)),
            kit::group(&["labels"], None, &[], std::mem::take(&mut self.front)),
        ];
        let mut summary = BTreeMap::new();
        summary.insert("kind".into(), json!(kind));
        summary.insert(
            "plot".into(),
            json!([
                kit::round2(self.plot.x),
                kit::round2(self.plot.y),
                kit::round2(self.plot.x + self.plot.width),
                kit::round2(self.plot.y + self.plot.height)
            ]),
        );
        summary.insert(
            "orientation".into(),
            json!(if self.horizontal {
                "horizontal"
            } else {
                "vertical"
            }),
        );
        summary.insert(
            "order".into(),
            json!(
                order
                    .iter()
                    .map(|&i| plan
                        .spec
                        .raw
                        .pointer(&format!("/data/x/values/{i}"))
                        .cloned()
                        .unwrap_or(Value::Null))
                    .collect::<Vec<_>>()
            ),
        );
        summary.insert(
            "scale".into(),
            json!({"lo": px(self.scale.lo), "hi": px(self.scale.hi)}),
        );
        summary.insert("width".into(), kit::r2(self.width));
        summary.insert("height".into(), kit::r2(self.height));
        PlacedChart {
            width: self.width,
            height: self.height,
            primitives: vec![kit::group(
                &["chart", kind, "categorical"],
                Some("chart"),
                &[],
                root,
            )],
            findings: self.findings,
            summary,
            decisions: self.decisions,
        }
    }
}

/// Lay out the frame of a row chart: title, legend, category labels, value axis with its grid,
/// the gutter texts and the note; return the frame for the marks and the display order.
pub(crate) fn rows(
    kit: &Kit,
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
    spec: &Rows,
) -> Result<(Frame, Vec<usize>)> {
    let scale = scale
        .ok_or_else(|| kit::err("a statistical chart needs a trained value scale"))?
        .clone();
    let horizontal = is_horizontal(plan);
    let n = data.categories.len();
    let order = display_order(plan, n);
    let w = kit.c.width;
    let em = kit.metric("--label-size")?;
    let mut findings = vec![];
    let mut decisions = vec![];
    let (title_prims, top) = title(kit, plan, &mut findings, &mut decisions)?;

    // legend
    let keys: Vec<Key> = spec
        .keys
        .iter()
        .map(|(i, glyph)| {
            let ink = series_ink(kit, plan, *i);
            Ok(Key {
                glyph: glyph.clone(),
                fill: ink.fill,
                strong: ink.strong,
                text: guides::series_content(plan, data, *i)?,
            })
        })
        .collect::<Result<_>>()?;
    let legend_y = top + em * 0.4;
    let (legend_prims, legend_h) = if keys.len() > 1 {
        legend(kit, &keys, 0., legend_y, w)?
    } else {
        (vec![], 0.)
    };
    let plot_top = legend_y + legend_h + if legend_h > 0. { em * 0.6 } else { em * 0.8 };

    // category labels and the gutter
    let label_gap = kit.metric("--category-label-gap")?;
    let cats: Vec<TextContent> = order
        .iter()
        .map(|&i| guides::category_content(plan, data, i))
        .collect::<Result<_>>()?;
    let label_w = cats
        .iter()
        .map(|c| kit.width("category-label", &[], c))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(0., f64::max);
    let gutter_w = if horizontal {
        let mut g: f64 = 0.;
        for row in &order {
            for t in spec.gutter.get(*row).into_iter().flatten().flatten() {
                g = g.max(kit.plain_ext("value-label", &[], t)?.width);
            }
        }
        if g > 0. { g + label_gap } else { 0. }
    } else {
        0.
    };

    let f = formatted(plan, &scale, spec.measure.as_ref())?;
    let tick_h = kit.plain_ext("scale-label-formatted", &[], "0")?;
    let axis_h = if horizontal {
        tick_h.ascent
            + tick_h.descent
            + 8.
            + if f.title.is_empty() {
                0.
            } else {
                kit.plain_ext("scale-title", &[], &f.title)?.ascent + 12.
            }
    } else {
        12.
    };

    let pitch = spec.slots as f64 * spec.slot_h + em;
    let block = spec.slots as f64 * spec.slot_h;
    let (plot, left_labels_w);
    if horizontal {
        left_labels_w = label_w + label_gap;
        let ph = n as f64 * pitch;
        let last_half = match f.bare.last() {
            Some(l) => kit.plain_ext("scale-label-formatted", &[], l)?.width / 2. + 2.,
            None => 0.,
        };
        plot = Bounds {
            x: left_labels_w,
            y: plot_top,
            width: w - left_labels_w - gutter_w.max(last_half) - em,
            height: ph,
        };
    } else {
        // value labels at the left; category labels under the plot
        let vl = f
            .labels
            .iter()
            .map(|l| {
                kit.plain_ext("scale-label-formatted", &[], l)
                    .map(|e| e.width)
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .fold(0., f64::max);
        left_labels_w = vl + label_gap;
        let band = (w - left_labels_w - em) / n as f64;
        if cats.iter().any(|c| {
            kit.width("category-label", &[], c)
                .map(|x| x > band - 4.)
                .unwrap_or(false)
        }) {
            return Err(kit::err("the category labels do not fit across a vertical chart; use orientation horizontal").at("chart.orientation"));
        }
        let ph = (spec.slots as f64 * 28. + 120.).max(180.);
        plot = Bounds {
            x: left_labels_w,
            y: plot_top + em * 2.,
            width: band * n as f64,
            height: ph,
        };
    }
    if plot.width < 40. || plot.height < 30. {
        return Err(kit::err("the measured labels leave no usable plot area"));
    }
    let (pitch, block) = if horizontal {
        (pitch, block)
    } else {
        (
            plot.width / n as f64,
            spec.slots as f64 * (plot.width / n as f64 / (spec.slots as f64 + 1.)).min(spec.slot_h),
        )
    };
    let slot_h = if horizontal {
        spec.slot_h
    } else {
        block / spec.slots as f64
    };

    let mut behind = vec![];
    let mut front = vec![];
    // grid and tick labels
    let frame_stub = Frame {
        horizontal,
        plot: plot.clone(),
        scale: scale.clone(),
        pitch,
        block,
        slot_h,
        behind: vec![],
        marks: vec![],
        front: vec![],
        width: w,
        height: 0.,
        findings: vec![],
        decisions: vec![],
    };
    let inline = f.top.inline();
    for (i, tick) in scale.ticks.iter().enumerate() {
        let v = px(*tick);
        let p = frame_stub.val(v);
        if horizontal {
            behind.push(kit::line(
                p,
                plot.y,
                p,
                plot.y + plot.height,
                paint(None, Some(&kit.theme.grid), kit.metric("--grid-width")?),
                kit::meta(&["grid"], &[]),
            ));
            let label = &f.bare[i];
            front.push(
                text_at(
                    kit,
                    "scale-label-formatted",
                    label,
                    p,
                    plot.y + plot.height + 6. + tick_h.ascent / 2.,
                    "middle",
                )?
                .primitive(),
            );
        } else {
            behind.push(kit::line(
                plot.x,
                p,
                plot.x + plot.width,
                p,
                paint(None, Some(&kit.theme.grid), kit.metric("--grid-width")?),
                kit::meta(&["grid"], &[]),
            ));
            let top_inline_fits =
                kit.plain_ext("scale-label-formatted", &[], &inline)?.width <= left_labels_w - 8.;
            let label = if i == scale.ticks.len() - 1 && top_inline_fits {
                &inline
            } else {
                &f.labels[i]
            };
            front.push(
                text_at(kit, "scale-label-formatted", label, plot.x - 6., p, "end")?.primitive(),
            );
        }
    }
    let axis_y = plot.y + plot.height;
    if horizontal {
        behind.push(kit::line(
            plot.x,
            axis_y,
            plot.x + plot.width,
            axis_y,
            paint(None, Some(&kit.theme.text), 0.8),
            kit::meta(&["axis"], &[]),
        ));
        if !f.title.is_empty() {
            front.push(
                text_at(
                    kit,
                    "scale-title",
                    &f.title,
                    plot.x + plot.width / 2.,
                    axis_y + 6. + tick_h.ascent + 8. + 4.,
                    "middle",
                )?
                .primitive(),
            );
        }
    } else {
        behind.push(kit::line(
            plot.x,
            plot.y,
            plot.x,
            axis_y,
            paint(None, Some(&kit.theme.text), 0.8),
            kit::meta(&["axis"], &[]),
        ));
        behind.push(kit::line(
            plot.x,
            axis_y,
            plot.x + plot.width,
            axis_y,
            paint(None, Some(&kit.theme.text), 0.8),
            kit::meta(&["axis"], &[]),
        ));
        let inline_fits =
            kit.plain_ext("scale-label-formatted", &[], &inline)?.width <= left_labels_w - 8.;
        if !inline_fits && !f.unit_line.is_empty() {
            front.push(
                text_at(
                    kit,
                    "unit-line",
                    &f.unit_line,
                    plot.x,
                    plot.y - em * 1.5,
                    "start",
                )?
                .primitive(),
            );
        }
    }
    if spec.zero_line && scale.lo <= Decimal::ZERO && scale.hi >= Decimal::ZERO {
        let z = frame_stub.val(0.);
        let ((x1, y1), (x2, y2)) = (
            frame_stub.at(
                plot.y.min(plot.x) * 0. + if horizontal { plot.y } else { plot.x },
                z,
            ),
            frame_stub.at(
                if horizontal {
                    plot.y + plot.height
                } else {
                    plot.x + plot.width
                },
                z,
            ),
        );
        behind.push(kit::line(
            x1,
            y1,
            x2,
            y2,
            paint(None, Some(&kit.theme.text), 1.5),
            kit::meta(&["zero-line"], &[]),
        ));
    }

    // category labels, the gutter texts
    let mut below_labels = 0.;
    for (row, &cat) in order.iter().enumerate() {
        let centre = frame_stub.cat(row, 0) + (spec.slots as f64 - 1.) * slot_h / 2.;
        if horizontal {
            front.push(
                kit.text(
                    "category-label",
                    &[],
                    &cats[row],
                    plot.x - label_gap,
                    centre
                        + (kit.ext("category-label", &[], &cats[row])?.ascent
                            - kit.ext("category-label", &[], &cats[row])?.descent)
                            / 2.,
                    "end",
                )?
                .primitive(),
            );
            for s in 0..spec.slots {
                if let Some(Some(t)) = spec.gutter.get(cat).map(|r| r.get(s).cloned().flatten()) {
                    front.push(
                        text_at(
                            kit,
                            "value-label",
                            &t,
                            plot.x + plot.width + label_gap,
                            frame_stub.cat(row, s),
                            "start",
                        )?
                        .primitive(),
                    );
                }
            }
        } else {
            let e = kit.ext("category-label", &[], &cats[row])?;
            let y = axis_y + 6. + e.ascent;
            front.push(
                kit.text("category-label", &[], &cats[row], centre, y, "middle")?
                    .primitive(),
            );
            let mut y2 = y + e.descent + 3.;
            for s in 0..spec.slots {
                if let Some(Some(t)) = spec.gutter.get(cat).map(|r| r.get(s).cloned().flatten()) {
                    let g = kit.plain_ext("value-label", &[], &t)?;
                    y2 += g.ascent;
                    front.push(
                        kit.text(
                            "value-label",
                            &[],
                            &TextContent::plain(&t),
                            centre,
                            y2,
                            "middle",
                        )?
                        .primitive(),
                    );
                    y2 += g.descent + 2.;
                }
            }
            below_labels = f64::max(below_labels, y2 - axis_y);
        }
    }

    // note
    let after_axis = if horizontal {
        axis_y + axis_h
    } else {
        axis_y + below_labels.max(24.)
    };
    let (note_prims, note_h) = note(kit, &spec.note, 0., after_axis + em * 0.6, w)?;
    let mut tail = vec![];
    tail.extend(legend_prims);
    tail.extend(note_prims);
    let computed = after_axis + em * 0.6 + note_h + em * 0.4;
    let height = match kit.c.height {
        Some(h) if h + 1e-6 < computed => {
            return Err(kit::err(format!(
                "the chart needs {computed:.0} pt of height, {h:.0} given"
            )));
        }
        Some(h) => h,
        None => computed,
    };
    let mut frame = Frame {
        horizontal,
        plot: plot.clone(),
        scale,
        pitch,
        block,
        slot_h,
        behind,
        marks: vec![],
        front,
        width: w,
        height,
        findings,
        decisions,
    };
    frame.behind.splice(0..0, title_prims);
    frame.front.extend(tail);
    kit::decide(
        &mut frame.decisions,
        "XS-LAYOUT",
        format!(
            "{} on a {} frame; the value axis is the trained scale",
            plan.chart_type.as_str(),
            if horizontal { "horizontal" } else { "vertical" }
        ),
        Some("guided"),
    );
    let _ = (
        left_labels_w,
        SeriesMeta {
            id: String::new(),
            name: String::new(),
        },
    );
    Ok((frame, order))
}

// ---- marks ---------------------------------------------------------------------------------

fn data_of(series: &str, group: &str, value: f64) -> Vec<(&'static str, String)> {
    vec![
        ("series", series.to_owned()),
        ("x", group.to_owned()),
        ("value", kit::dec_text(dec(value).round_dp(6))),
    ]
}

pub(crate) fn draw_box(
    kit: &Kit,
    plan: &SemanticPlan,
    b: &BoxModel,
    f: &mut Frame,
    order: &[usize],
) {
    let _ = kit;
    for (row, &g) in order.iter().enumerate() {
        for (s, series) in b.series.iter().enumerate() {
            let cell = &b.cells[s][g];
            let st = &cell.stats;
            let ink = series_ink(kit, plan, s);
            let class = series_class(plan, s);
            let c = f.cat(row, s);
            let m = |kind: &str| {
                kit::meta(
                    &[kind, &class],
                    &data_of(&series.id, &b.labels[g], st.median),
                )
            };
            let (lo, hi, q1, q3, med) = (
                f.val(st.lower_whisker),
                f.val(st.upper_whisker),
                f.val(st.q1),
                f.val(st.q3),
                f.val(st.median),
            );
            let cap = f.slot_h * 0.18;
            let half = f.slot_h * 0.32;
            f.line(
                c,
                lo,
                c,
                q1,
                paint(None, Some(&ink.strong), 1.4),
                m("whisker"),
            );
            f.line(
                c,
                q3,
                c,
                hi,
                paint(None, Some(&ink.strong), 1.4),
                m("whisker"),
            );
            f.line(
                c - cap,
                lo,
                c + cap,
                lo,
                paint(None, Some(&ink.strong), 1.4),
                m("whisker-cap"),
            );
            f.line(
                c - cap,
                hi,
                c + cap,
                hi,
                paint(None, Some(&ink.strong), 1.4),
                m("whisker-cap"),
            );
            f.bar(
                c,
                half,
                q1,
                q3,
                paint(Some(&ink.fill), Some(&ink.strong), 1.),
                m("box"),
            );
            f.line(
                c - half,
                med,
                c + half,
                med,
                paint(None, Some(&kit.theme.text), 2.4),
                m("median"),
            );
            let shown: Vec<f64> = match (&cell.samples, b.points) {
                (_, Points::None) if !cell.draw_all => vec![],
                (Some(sam), _) if cell.draw_all => sam.clone(),
                _ => st.outliers.clone(),
            };
            for (k, v) in shown.iter().enumerate() {
                let dy = if cell.draw_all {
                    jitter(k + 31 * g + 7 * s) * half * 0.6
                } else {
                    0.
                };
                let (x, y) = f.at(c + dy, f.val(*v));
                f.marks.push(Primitive::Circle {
                    center: Point { x, y },
                    radius: 2.2,
                    paint: paint(Some("#ffffff"), Some(&ink.strong), 1.),
                    meta: kit::meta(&["point", &class], &data_of(&series.id, &b.labels[g], *v)),
                });
            }
            if b.mean_marker && st.mean.is_finite() {
                let (x, y) = f.at(c, f.val(st.mean));
                let r = 3.4;
                f.marks.push(Primitive::Path {
                    points: vec![
                        Point { x, y: y - r },
                        Point { x: x + r, y },
                        Point { x, y: y + r },
                        Point { x: x - r, y },
                    ],
                    closed: true,
                    paint: paint(Some(&kit.theme.text), Some(&kit.theme.text), 1.),
                    meta: kit::meta(
                        &["mean", &class],
                        &data_of(&series.id, &b.labels[g], st.mean),
                    ),
                });
            }
        }
    }
}

pub(crate) fn draw_strip(
    kit: &Kit,
    plan: &SemanticPlan,
    st: &StripModel,
    f: &mut Frame,
    order: &[usize],
) {
    for (row, &g) in order.iter().enumerate() {
        for (s, series) in st.series.iter().enumerate() {
            let v = &st.samples[s][g];
            let ink = series_ink(kit, plan, s);
            let class = series_class(plan, s);
            let c = f.cat(row, s);
            let half = f.slot_h * 0.38;
            for (k, x) in v.iter().enumerate() {
                let (px_, py) = f.at(c + jitter(k + 17 * g + 5 * s) * half, f.val(*x));
                let mut p = paint(Some(&ink.fill), Some(&ink.strong), 0.8);
                p.opacity = if v.len() > 100 { 0.5 } else { 0.85 };
                f.marks.push(Primitive::Circle {
                    center: Point { x: px_, y: py },
                    radius: 2.4,
                    paint: p,
                    meta: kit::meta(&["point", &class], &data_of(&series.id, &st.labels[g], *x)),
                });
            }
            let med = f.val(chartgen_statistics::stats::median(v));
            f.line(
                c - half - 2.,
                med,
                c + half + 2.,
                med,
                paint(None, Some(&kit.theme.text), 2.),
                kit::meta(
                    &["median", &class],
                    &data_of(
                        &series.id,
                        &st.labels[g],
                        chartgen_statistics::stats::median(v),
                    ),
                ),
            );
        }
    }
}

pub(crate) fn draw_interval(
    kit: &Kit,
    plan: &SemanticPlan,
    m: &IntervalModel,
    f: &mut Frame,
    order: &[usize],
) {
    for (row, &g) in order.iter().enumerate() {
        for (s, series) in m.series.iter().enumerate() {
            let e = &m.estimates[s][g];
            let ink = series_ink(kit, plan, s);
            let class = series_class(plan, s);
            let c = f.cat(row, s);
            let meta = |kind: &str| {
                kit::meta(&[kind, &class], &data_of(&series.id, &m.labels[g], e.value))
            };
            let half = f.slot_h * 0.32;
            if m.bar {
                f.bar(
                    c,
                    half,
                    f.val(0.),
                    f.val(e.value),
                    paint(Some(&ink.fill), Some(&ink.strong), 1.),
                    meta("bar"),
                );
            }
            let cap = f.slot_h * 0.18;
            let (lo, hi) = (f.val(e.lo), f.val(e.hi));
            f.line(
                c,
                lo,
                c,
                hi,
                paint(None, Some(&kit.theme.text), 1.6),
                meta("interval"),
            );
            f.line(
                c - cap,
                lo,
                c + cap,
                lo,
                paint(None, Some(&kit.theme.text), 1.6),
                meta("interval-cap"),
            );
            f.line(
                c - cap,
                hi,
                c + cap,
                hi,
                paint(None, Some(&kit.theme.text), 1.6),
                meta("interval-cap"),
            );
            let (x, y) = f.at(c, f.val(e.value));
            if m.bar {
                f.line(
                    c - half,
                    f.val(e.value),
                    c + half,
                    f.val(e.value),
                    paint(None, Some(&kit.theme.text), 2.),
                    meta("estimate"),
                );
            } else {
                f.marks
                    .push(mark(MARKS[0], x, y, 10., &ink, meta("estimate")));
            }
        }
    }
}

fn good_class(good: Option<Good>, d: f64) -> Option<&'static str> {
    match (good, d) {
        (_, 0.) => None,
        (Some(Good::Up), d) => Some(if d > 0. { "good" } else { "bad" }),
        (Some(Good::Down), d) => Some(if d < 0. { "good" } else { "bad" }),
        _ => None,
    }
}

pub(crate) fn draw_paired(
    kit: &Kit,
    plan: &SemanticPlan,
    p: &PairedModel,
    f: &mut Frame,
    order: &[usize],
) {
    for (row, &g) in order.iter().enumerate() {
        let c = f.cat(row, 0);
        let cls = good_class(p.good, p.diffs[g]);
        let neutral = kit
            .token("--average")
            .unwrap_or_else(|| kit.theme.text.clone());
        let link = match cls {
            Some(k) => kit::class_paint(kit, k, &[]),
            None => neutral.clone(),
        };
        let classes: Vec<&str> = match cls {
            Some(k) => vec!["link", k],
            None => vec!["link"],
        };
        match p.view {
            View::Pair => {
                let (a, b) = (p.values[0][g], p.values[1][g]);
                f.line(
                    c,
                    f.val(a),
                    c,
                    f.val(b),
                    paint(None, Some(&link), 3.),
                    kit::meta(&classes, &data_of(&p.diff_name, &p.labels[g], p.diffs[g])),
                );
                for (s, v) in [(0usize, a), (1, b)] {
                    let ink = series_ink(kit, plan, s);
                    let (x, y) = f.at(c, f.val(v));
                    f.marks.push(mark(
                        MARKS[s],
                        x,
                        y,
                        10.,
                        &ink,
                        kit::meta(
                            &["estimate", &series_class(plan, s)],
                            &data_of(&p.series[s].id, &p.labels[g], v),
                        ),
                    ));
                }
            }
            View::Difference => {
                let d = p.diffs[g];
                f.line(
                    c,
                    f.val(0.),
                    c,
                    f.val(d),
                    paint(None, Some(&link), 3.),
                    kit::meta(&classes, &data_of(&p.diff_name, &p.labels[g], d)),
                );
                if let Some((iv, _)) = &p.intervals {
                    let cap = f.slot_h * 0.18;
                    let (lo, hi) = (f.val(iv[g].0), f.val(iv[g].1));
                    f.line(
                        c,
                        lo,
                        c,
                        hi,
                        paint(None, Some(&kit.theme.text), 1.4),
                        kit::meta(&["interval"], &data_of(&p.diff_name, &p.labels[g], d)),
                    );
                    f.line(
                        c - cap,
                        lo,
                        c + cap,
                        lo,
                        paint(None, Some(&kit.theme.text), 1.4),
                        kit::meta(&["interval-cap"], &[]),
                    );
                    f.line(
                        c - cap,
                        hi,
                        c + cap,
                        hi,
                        paint(None, Some(&kit.theme.text), 1.4),
                        kit::meta(&["interval-cap"], &[]),
                    );
                }
                let (x, y) = f.at(c, f.val(d));
                let ink = Ink {
                    fill: link.clone(),
                    strong: kit.theme.text.clone(),
                };
                f.marks.push(mark(
                    MARKS[0],
                    x,
                    y,
                    10.,
                    &ink,
                    kit::meta(&["estimate"], &data_of(&p.diff_name, &p.labels[g], d)),
                ));
            }
        }
    }
}

/// A deterministic spread in [-1, 1] from an index (no randomness: renders are reproducible).
pub(crate) fn jitter(i: usize) -> f64 {
    let mut z = (i as u64)
        .wrapping_add(1)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z ^= z >> 27;
    ((z >> 11) as f64 / (1u64 << 53) as f64) * 2. - 1.
}

/// The dash patterns of the curves of a group: solid, dashed, dotted, dash-dot (on, off lengths).
const DASHES: [&[f64]; 4] = [&[], &[7., 3.5], &[2., 3.], &[9., 3., 2., 3.]];

/// The (start, end) runs of a dash pattern over a length; one run for a solid line.
pub(crate) fn dash_runs(from: f64, to: f64, pattern: usize) -> Vec<(f64, f64)> {
    let p = DASHES[pattern % DASHES.len()];
    if p.is_empty() {
        return vec![(from, to)];
    }
    let (mut at, mut i, mut out) = (from, 0, vec![]);
    while at < to {
        let len = p[i % p.len()];
        if i % 2 == 0 {
            out.push((at, (at + len).min(to)));
        }
        at += len;
        i += 1;
    }
    out
}

/// A polyline cut into dashes by arc length; a solid pattern keeps it whole.
pub(crate) fn dashed(points: &[Point], pattern: usize) -> Vec<Vec<Point>> {
    let p = DASHES[pattern % DASHES.len()];
    if p.is_empty() || points.len() < 2 {
        return vec![points.to_vec()];
    }
    let (mut out, mut cur): (Vec<Vec<Point>>, Vec<Point>) = (vec![], vec![]);
    let (mut i, mut left) = (0usize, p[0]);
    let mut on = true;
    cur.push(points[0].clone());
    for w in points.windows(2) {
        let (a, b) = (w[0].clone(), w[1].clone());
        let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        let mut done = 0.;
        while len - done > left {
            done += left;
            let t = done / len;
            let at = Point {
                x: a.x + (b.x - a.x) * t,
                y: a.y + (b.y - a.y) * t,
            };
            if on {
                cur.push(at);
                out.push(std::mem::take(&mut cur));
            } else {
                cur = vec![at];
            }
            on = !on;
            i += 1;
            left = p[i % p.len()];
        }
        left -= len - done;
        if on {
            cur.push(b.clone());
        }
    }
    if on && cur.len() > 1 {
        out.push(cur);
    }
    out
}
