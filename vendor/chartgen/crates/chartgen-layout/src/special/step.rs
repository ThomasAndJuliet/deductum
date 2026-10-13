//! Irregular series on a true time axis: step charts and thin bars, ported from the
//! original's `layout/step.py` (notes 06 §5.3, §5.4; G1.53, G1.54, G2.42, G7.19, G7.20;
//! D-42, D-45).
//!
//! - A step chart draws each state level from its change to the next, then straight up or
//!   down; the last value runs to the as-of date (or the window's end). Without either, it
//!   is refused (R6-8). A state not known leaves a gap.
//! - Thin bars: one narrow bar per event, centred on its instant and rising from zero;
//!   events at one instant stacked with a hairline gap; an event whose value is not known
//!   gets a note at its instant.
//! - The axis is a true linear time scale labelled by `continuous::continuous_fit`; an outer
//!   label that would reach past the figure pulls the plot in, and the first and last labels
//!   keep clear of the outer dividers, the layout running again.
use super::continuous::{self as ta, ContinuousAxis};
use super::fig::{self, Batch, Bx, Item, Txt, attrs, overlap, segment_meets_box};
use super::*;
use chartgen_format::ScaleLabels;
use chrono::{Duration, NaiveDate, NaiveDateTime};
use serde_json::Value;

/// One point of the chart's series: (instant, resolution, datum, source time text).
pub(crate) struct Pt<'a> {
    pub t: NaiveDateTime,
    pub res: &'static str,
    pub d: &'a Datum,
    pub text: &'a str,
}

pub(crate) fn points<'a>(data: &'a PreparedData) -> Result<Vec<Pt<'a>>> {
    let rows = data
        .irregular
        .as_ref()
        .ok_or_else(|| err("missing irregular points"))?;
    let row = rows
        .first()
        .ok_or_else(|| err("irregular chart has no series"))?;
    row.iter()
        .map(|p| {
            let (t, res) = ta::parse_instant(&p.time)?;
            Ok(Pt {
                t,
                res,
                d: &p.datum,
                text: &p.time,
            })
        })
        .collect()
}

/// Where a value sits on the axis: a dated event in the middle of its day.
pub(crate) fn instant(t: NaiveDateTime, res: &str, thin: bool) -> NaiveDateTime {
    if thin && res == "day" {
        t + Duration::hours(12)
    } else {
        t
    }
}

fn day_start(text: &str) -> Option<NaiveDateTime> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
}

/// A refusal finding as the original records it: overridden when the spec overrides the
/// rule, a warning for analysis, else an error. Returns the finding.
pub(crate) fn refusal(
    plan: &SemanticPlan,
    rule: &str,
    message: impl Into<String>,
    path: &str,
    supply: Option<String>,
) -> Finding {
    let raw = &plan.spec.raw;
    let reason = raw["overrides"].as_array().and_then(|o| {
        o.iter()
            .find(|o| o["rule"].as_str() == Some(rule))
            .and_then(|o| o["reason"].as_str())
    });
    let mut f = if reason.is_some() {
        Finding::new(rule, "overridden", message)
    } else if raw["purpose"].as_str() == Some("analysis") {
        Finding::new(rule, "warn", message)
    } else {
        Finding::new(rule, "error", message)
    }
    .at(path);
    f.supply = supply;
    f.reason = reason.map(str::to_owned);
    f
}

/// A guided choice (rules.guided): where the spec's explicit choice departs from what the
/// procedure gives, a warning naming the recommendation (an info when the spec gives its reason).
pub(crate) fn guided(
    plan: &SemanticPlan,
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
    let reason = plan
        .spec
        .raw
        .pointer("/reasons")
        .and_then(|r| r.get(path))
        .and_then(Value::as_str);
    let mut f = Finding::new(
        rule,
        if reason.is_some() { "info" } else { "warn" },
        format!("{path} {chosen} departs from the procedure, which gives {recommended} ({why})"),
    )
    .at(path);
    f.level = "guided".into();
    f.reason = reason.map(str::to_owned);
    f.recommended = Some(recommended.to_owned());
    findings.push(f);
    true
}

/// (t0, t1, grain, how) of the axis, or a refusal (rule, path, message, supply).
#[allow(clippy::type_complexity)]
fn domain(
    plan: &SemanticPlan,
    thin: bool,
    pts: &[Pt],
) -> std::result::Result<
    (NaiveDateTime, NaiveDateTime, &'static str, String),
    (&'static str, &'static str, String, Option<String>),
> {
    let grain = pts
        .iter()
        .map(|p| p.res)
        .min_by_key(|r| ta::grain_order(r))
        .unwrap_or("day");
    let raw = &plan.spec.raw;
    let window = match raw.pointer("/data/x/values").and_then(Value::as_array) {
        Some(v) if !v.is_empty() => {
            let bad = || {
                (
                    "CG-4",
                    "data.x.values",
                    "x.values of an irregular series is its window, [first day, last day]"
                        .to_owned(),
                    None,
                )
            };
            if v.len() != 2 {
                return Err(bad());
            }
            let a = v[0].as_str().and_then(|s| ta::parse_instant(s).ok());
            let b = v[1].as_str().and_then(|s| ta::parse_instant(s).ok());
            let (Some((a, _)), Some((b, rb))) = (a, b) else {
                return Err(bad());
            };
            let b = if rb == "day" {
                b + Duration::days(1)
            } else {
                b
            };
            if a >= b {
                return Err((
                    "CG-4",
                    "data.x.values",
                    "the window's first day must come before its last".into(),
                    None,
                ));
            }
            Some((a, b))
        }
        _ => None,
    };
    let end = raw
        .pointer("/data/as_of")
        .and_then(Value::as_str)
        .and_then(day_start)
        .map(|d| d + Duration::days(1));
    let first = pts.iter().map(|p| instant(p.t, p.res, thin)).min().unwrap();
    let last = pts.iter().map(|p| instant(p.t, p.res, thin)).max().unwrap();
    let (t0, mut t1, mut how);
    if let Some((a, b)) = window {
        t0 = a;
        t1 = b;
        how = "the window given".to_owned();
    } else {
        t0 = if thin {
            ta::unit_floor(first, "day")
        } else {
            first
        };
        how = "from the first value".to_owned();
        if let Some(e) = end {
            t1 = e;
            how += " to the end of the as-of day";
        } else if thin {
            t1 = if grain == "day" {
                ta::unit_next(last, "day")
            } else {
                let span = last - first;
                last + Duration::microseconds(
                    (span.num_microseconds().unwrap_or(0) as f64 * 0.02) as i64,
                )
            };
            how += " to the end of the last event's day";
        } else {
            return Err((
                "R6-8",
                "data.as_of",
                "a step chart runs its last value to the date the series is known to (G7.19, D-42), which cannot be read from the changes".into(),
                Some("data.as_of, the last day the series covers".into()),
            ));
        }
    }
    if let (Some(e), Some(_)) = (end, window) {
        if e < t1 {
            t1 = t1.min(e);
        }
    }
    let latest = pts.iter().map(|p| p.t).max().unwrap();
    if (first < t0 && thin) || last > t1 || (!thin && latest >= t1) {
        return Err((
            "CG-4",
            "data.series[0].points",
            "a value lies outside the period the axis covers (the window or the as-of date)".into(),
            Some("points inside the window, or a later as-of date".into()),
        ));
    }
    Ok((t0, t1, grain, how))
}

/// The value scale's texts, gridlines and unit phrase (the original's `vertical_scale`).
pub(crate) struct VScale {
    pub decisions: Vec<Value>,
    pub texts: Vec<(usize, Txt)>,
    pub grid: Vec<Item>,
    pub first_local: f64,
    pub first_asc: f64,
}

/// Group indices for texts placed by the scale (`SCALE`) and the axis (`AXIS`), notes (`NOTES`).
pub(crate) const SCALE: usize = 0;
pub(crate) const AXIS: usize = 1;
pub(crate) const NOTES: usize = 2;

#[allow(clippy::too_many_arguments)]
pub(crate) fn vertical_scale(
    plan: &SemanticPlan,
    batch: &Batch,
    scale: &TrainedScale,
    sl: &ScaleLabels,
    plot_l: f64,
    plot_r: f64,
    yv: &dyn Fn(Decimal) -> Result<f64>,
    hits_of: &dyn Fn(Bx) -> Vec<String>,
    findings: &mut Vec<Finding>,
    w_total: f64,
    what: &str,
) -> Result<VScale> {
    let top = &sl.top;
    let h0 = batch.ext("scale-label", "0")?.ascent;
    let pitch = batch.metric("--line-pitch")?;
    let n = sl.labels.len();
    if n == 0 || scale.ticks.len() != n {
        return Err(err("scale labels do not match the scale's ticks"));
    }
    let mut xr = batch.width("scale-label", &top.number)?;
    for l in &sl.labels[..n - 1] {
        xr = xr.max(batch.width("scale-label", l)?);
    }
    let mut texts = vec![];
    let mut decisions = vec![];
    let top_base = yv(scale.ticks[n - 1])? + h0 / 2.;
    for (t, l) in scale.ticks[..n - 1].iter().zip(&sl.labels[..n - 1]) {
        texts.push((
            SCALE,
            batch.plain("scale-label", l, xr, yv(*t)? + h0 / 2., "end")?,
        ));
    }
    let num = batch.plain("scale-label", &top.number, xr, top_base, "end")?;
    let e_num = num.ext;
    texts.push((SCALE, num));
    let mut grid_start = plot_l;
    let mut unit_lines: Vec<String> = vec![];
    let after = format!(
        "{}{}",
        top.attached,
        if top.phrase.is_empty() {
            String::new()
        } else {
            format!(" {}", top.phrase)
        }
    );
    let has_after = !after.trim().is_empty();
    if has_after {
        let ph_x = if !top.attached.is_empty() {
            xr
        } else {
            xr + batch.metric("--unit-space")?
        };
        let e_ph = batch.ext("unit-phrase", &after)?;
        let ph_end = ph_x + e_ph.width;
        let clr = batch.metric("--unit-clearance")?;
        let asc = e_num.ascent.max(e_ph.ascent);
        let desc = e_num.descent.max(e_ph.descent);
        let b = (
            xr - e_num.width - clr,
            top_base - asc - clr,
            ph_end + clr,
            top_base + desc + clr,
        );
        let start = ph_end + batch.metric("--unit-gridline-gap")?;
        let room = plot_r - start >= batch.metric("--unit-min-gridline")? * (plot_r - plot_l);
        let hits = hits_of(b);
        let pos = plan.spec.string("/scale/unit_position", "auto");
        let auto_inline = room && hits.is_empty();
        let inline = pos == "inline" || (pos == "auto" && auto_inline);
        if pos != "auto" {
            guided(
                plan,
                findings,
                "6.4",
                "scale.unit_position",
                if inline { "inline" } else { "above" },
                if auto_inline { "inline" } else { "above" },
                &format!(
                    "the phrase on the top label unless it would meet a {what} or leave too little gridline"
                ),
            );
        }
        if inline {
            texts.push((
                SCALE,
                batch.plain("unit-phrase", &after, ph_x, top_base, "start")?,
            ));
            grid_start = start;
            decisions.push(fig::decision(
                "R2-07",
                format!(
                    "the unit phrase on the top label: “{}”; the top gridline starts after it",
                    top.inline()
                ),
                None,
            ));
        } else {
            let kept = top.kept_text();
            if !kept.is_empty() {
                let kx = if !top.attached.is_empty() {
                    xr
                } else {
                    xr + batch.metric("--unit-space")?
                };
                let t = batch.plain("unit-phrase", &kept, kx, top_base, "start")?;
                grid_start = plot_l.max(kx + t.ext.width + batch.metric("--unit-gridline-gap")?);
                texts.push((SCALE, t));
            }
            let unit_plain = sl.unit_line.clone();
            if !unit_plain.is_empty() {
                let w = |s: &str| batch.width("unit-line", s);
                unit_lines = match fig::best_lines(&unit_plain, &w, Some(w_total), 2)? {
                    Some(l) => l,
                    None => {
                        findings.push(
                            Finding::new(
                                "CG-2",
                                "error",
                                format!(
                                    "the unit line “{unit_plain}” does not fit two lines of the width"
                                ),
                            )
                            .at("data.measure.unit.fallback"),
                        );
                        vec![unit_plain.clone()]
                    }
                };
            }
            let why = if pos == "above" || pos == "below" {
                "forced by scale.unit_position".to_owned()
            } else if let Some(h) = hits.first() {
                format!("inline it would meet the {what} {h}")
            } else {
                "inline it would leave too little of the top gridline".to_owned()
            };
            decisions.push(fig::decision(
                "6.4",
                format!(
                    "{}; “{}” stays on the top label",
                    if unit_lines.is_empty() {
                        format!("the unit phrase shortened ({why})")
                    } else {
                        format!(
                            "the unit's description on its own line above the scale ({why}): “{unit_plain}”{}",
                            if unit_lines.len() > 1 { " (two lines)" } else { "" }
                        )
                    },
                    top.fallback()
                ),
                None,
            ));
            let count = unit_lines.len();
            for (j, l) in unit_lines.iter().enumerate() {
                let yb = top_base - pitch * (count - j) as f64;
                texts.push((SCALE, batch.plain("unit-line", l, 0., yb, "start")?));
            }
        }
    }
    decisions.push(fig::decision(
        "R2-21",
        format!(
            "labels as a set, {} decimals: {}",
            sl.decimals,
            sl.labels.join(", ")
        ),
        None,
    ));
    if sl.magnitude != 0 {
        let name = match sl.magnitude {
            3 => "thousands",
            6 => "millions",
            9 => "billions",
            12 => "trillions",
            _ => "none",
        };
        decisions.push(fig::decision(
            "R5-10",
            format!(
                "digits hidden: {name}, stated once on the top label (R2-05, D-3): “{}”",
                top.inline()
            ),
            None,
        ));
    }
    let signed = scale.lo < Decimal::ZERO
        || matches!(
            plan.spec
                .raw
                .pointer("/data/measure/kind")
                .and_then(Value::as_str),
            Some("change" | "index")
        );
    let mut grid = vec![];
    if plan.spec.raw.pointer("/chart/gridlines") != Some(&Value::Bool(false)) {
        for t in &scale.ticks {
            let mut c = "gridline".to_owned();
            if t.is_zero() {
                c += " zero";
                if signed {
                    c += " signed";
                }
            }
            let x0 = if *t == scale.ticks[n - 1] {
                grid_start
            } else {
                plot_l
            };
            let y = yv(*t)?;
            grid.push(Item::line(
                x0,
                y,
                plot_r,
                y,
                &c,
                attrs(&[("value", t.normalize().to_string())]),
            ));
        }
    }
    let first_local = top_base - pitch * unit_lines.len() as f64;
    let first_asc = if let Some(l) = unit_lines.first() {
        batch.ext("unit-line", l)?.ascent
    } else {
        e_num.ascent.max(if has_after {
            batch.ext("unit-phrase", &after)?.ascent
        } else {
            0.
        })
    };
    Ok(VScale {
        decisions,
        texts,
        grid,
        first_local,
        first_asc,
    })
}

/// Every string `vertical_scale` might set.
pub(crate) fn need_scale_texts(batch: &Batch, sl: &ScaleLabels) -> Result<()> {
    let top = &sl.top;
    let n = sl.labels.len();
    for c in sl.labels[..n.saturating_sub(1)].iter().cloned().chain([
        top.number.clone(),
        top.short(),
        "0".into(),
    ]) {
        batch.need("scale-label", &c)?;
    }
    let after = format!(
        "{}{}",
        top.attached,
        if top.phrase.is_empty() {
            String::new()
        } else {
            format!(" {}", top.phrase)
        }
    );
    if !after.trim().is_empty() {
        batch.need("unit-phrase", &after)?;
    }
    if !top.attached.is_empty() {
        batch.need("unit-phrase", &top.attached)?;
    }
    if !top.kept_text().is_empty() {
        batch.need("unit-phrase", &top.kept_text())?;
    }
    if !sl.unit_line.is_empty() {
        for opt in fig::line_options(&sl.unit_line) {
            for l in opt {
                batch.need("unit-line", &l)?;
            }
        }
    }
    batch.need("unit-line", "H")
}

/// Every string a true time axis over [t0, t1) might set.
fn need_axis(batch: &Batch, t0: NaiveDateTime, t1: NaiveDateTime) -> Result<()> {
    for unit in ta::UNITS {
        if unit == "hour" && (t1 - t0) > Duration::days(4) {
            continue;
        }
        let sp = ta::spans(t0, t1, unit);
        if sp.len() > 400 {
            continue;
        }
        for s in &sp {
            let forms: &[&str] = if unit == "month" {
                &["full", "narrow"]
            } else {
                &["full"]
            };
            for f in forms {
                batch.need("time-label", &ta::row1(&s.period, f))?;
            }
            if let Some(g) = s.period.group() {
                batch.need("time-label-2", &ta::row2(&g))?;
            }
        }
    }
    batch.need("time-label", "H")
}

pub(crate) fn title_plain(plan: &SemanticPlan) -> String {
    let title = &plan.spec.raw["title"];
    let mut parts = vec![];
    let text = if title.is_object() {
        &title["text"]
    } else {
        title
    };
    for v in [text, &title["descriptive"]] {
        if let Ok(Some(c)) = guides::content(v) {
            parts.push(c.readable().to_owned());
        }
    }
    parts.join(" ")
}

struct Axis {
    decisions: Vec<Value>,
    texts: Vec<Txt>,
    ticks: Vec<Item>,
    dividers: Vec<Item>,
    ax: ContinuousAxis,
}

#[allow(clippy::too_many_arguments)]
fn time_axis(
    plan: &SemanticPlan,
    batch: &Batch,
    t0: NaiveDateTime,
    t1: NaiveDateTime,
    grain: &str,
    x_of: &dyn Fn(NaiveDateTime) -> f64,
    base: f64,
    plot_l: f64,
    plot_r: f64,
    w: f64,
) -> Result<std::result::Result<Axis, Finding>> {
    let em = batch.metric("--label-size")?;
    let clear = batch.metric("--divider-clear")?;
    let width = |r: &str, t: &str| batch.width(r, t);
    let ax = match ta::continuous_fit(
        t0,
        t1,
        x_of,
        &width,
        batch.metric("--time-label-sep")?,
        grain,
        &title_plain(plan),
        Some(clear),
    )? {
        Ok(ax) => ax,
        Err(crowded) => {
            let supply = match crowded.factor {
                Some(f) => format!(
                    "layout.width at least {} (or a coarser grid)",
                    (w + (plot_r - plot_l) * (f - 1.)).ceil()
                ),
                None => "a wider chart, or a coarser grid".into(),
            };
            return Ok(Err(refusal(
                plan,
                "R2-40",
                "no unit of hours, days, months or years labels this axis without crowding: too narrow for its time axis",
                "layout.width",
                Some(supply),
            )));
        }
    };
    let mut texts = vec![];
    let mut ticks = vec![];
    let mut dividers = vec![];
    for (t, major) in &ax.ticks {
        let x = x_of(*t);
        let ln = if *major {
            batch.metric("--time-tick")?
        } else {
            batch.metric("--time-tick-minor")?
        };
        ticks.push(Item::line(x, base, x, base + ln, "time-tick", vec![]));
    }
    let cap = batch.ext("time-label", "H")?.ascent;
    let b1 = base + batch.metric("--category-gap")? + cap;
    let b2 = b1 + batch.metric("--time-row-pitch")?;
    for (t, text) in &ax.labels {
        texts.push(batch.plain("time-label", text, x_of(*t), b1, "middle")?);
    }
    let any_row2 = ax.groups.iter().any(|g| g.2.is_some());
    if any_row2 {
        for (a, b, text) in &ax.groups {
            let Some(text) = text else { continue };
            let xa = if *a == t0 && !ax.dividers.is_empty() {
                plot_l
            } else {
                x_of(*a)
            };
            let xb = if *b == t1 && !ax.dividers.is_empty() {
                plot_r
            } else {
                x_of(*b)
            };
            texts.push(batch.plain("time-label-2", text, (xa + xb) / 2., b2, "middle")?);
        }
    }
    if !ax.dividers.is_empty() {
        let y_end = if any_row2 {
            let mut d: f64 = f64::NEG_INFINITY;
            for (_, _, t) in &ax.groups {
                if let Some(t) = t {
                    d = d.max(batch.ext("time-label-2", t)?.descent);
                }
            }
            b2 + d
        } else {
            b1
        } + 0.3 * em;
        for t in &ax.dividers {
            let x = if *t == t0 {
                plot_l
            } else if *t == t1 {
                plot_r
            } else {
                x_of(*t)
            };
            dividers.push(Item::line(x, base, x, y_end, "time-divider", vec![]));
        }
    }
    let mut decisions: Vec<Value> = ax
        .decisions
        .iter()
        .map(|d| {
            fig::decision(
                if d.contains("every") {
                    "R2-40"
                } else {
                    "R2-39"
                },
                d.clone(),
                None,
            )
        })
        .collect();
    let row2: Vec<&str> = ax.groups.iter().filter_map(|g| g.2.as_deref()).collect();
    if !row2.is_empty() {
        decisions.push(fig::decision(
            "R2-39",
            format!(
                "time axis on two levels: row 2 {}{}",
                row2.join(", "),
                if ax.groups.len() >= 2 {
                    ", dividers between the groups"
                } else {
                    ""
                }
            ),
            None,
        ));
    }
    Ok(Ok(Axis {
        decisions,
        texts,
        ticks,
        dividers,
        ax,
    }))
}

/// Give each time label that comes closer than `clear` to a divider a knock-out halo copy.
pub(crate) fn add_halos(children: Vec<Item>, dividers: &[Item], clear: f64) -> Vec<Item> {
    let lines: Vec<(f64, f64, f64)> = dividers
        .iter()
        .filter_map(|d| match d {
            Item::Line { x1, y1, x2, y2, .. } if (x1 - x2).abs() < 0.01 => {
                Some((*x1, y1.min(*y2), y1.max(*y2)))
            }
            _ => None,
        })
        .collect();
    if lines.is_empty() {
        return children;
    }
    let mut out = vec![];
    let mut n = 0;
    for c in children {
        if let Item::Text(t) = &c {
            if matches!(t.role.as_str(), "time-label" | "time-label-2")
                && !t.modifiers.iter().any(|m| m == "text-halo")
            {
                let (x0, y0, x1, y1) = t.bbox(0.);
                if lines.iter().any(|(x, ya, yb)| {
                    *ya < y1 && y0 < *yb && x0 - clear + 0.01 < *x && *x < x1 + clear - 0.01
                }) {
                    let mut copy = t.clone();
                    copy.modifiers.insert(0, "text-halo".into());
                    copy.joint = Some(format!("halo-{n}-{}", t.plain));
                    n += 1;
                    out.push(Item::group("divider-halo", vec![Item::Text(copy)]));
                }
            }
        }
        out.push(c);
    }
    out
}

/// The layout of a step chart or thin bars.
pub(crate) fn layout(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: &TrainedScale,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
) -> Result<LayoutStep> {
    let batch = Batch::new(theme, c, m);
    let pass = solve(plan, data, scale, &batch);
    fig::run_pass(c, m, &batch, pass)
}

fn solve(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: &TrainedScale,
    batch: &Batch,
) -> Result<LayoutStep> {
    let thin = plan.chart_type == ChartType::ThinBar;
    if data.series.len() != 1 {
        return Err(err("a step chart or thin bars draw one series"));
    }
    let pts = points(data)?;
    if pts.is_empty() {
        return Err(err("irregular series has no points"));
    }
    let (t0, t1, grain, how) = match domain(plan, thin, &pts) {
        Ok(d) => d,
        Err((rule, path, message, supply)) => {
            return Ok(LayoutStep::Refused {
                findings: vec![refusal(plan, rule, message, path, supply)],
            });
        }
    };
    let mut pre = vec![];
    let raw = &plan.spec.raw;
    if raw.get("highlight").is_some_and(|h| !h.is_null())
        || raw
            .pointer("/insight/targets")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    {
        pre.push(
            Finding::new(
                "CG-3",
                "info",
                "highlighting a step or a thin bar is a later stage",
            )
            .at("highlight"),
        );
    }
    for (j, p) in pts.iter().enumerate() {
        let Some(note) = p.d.note.as_deref().filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        pre.push(
            Finding::new(
                "CG-3",
                "info",
                format!(
                    "the note on the point at {} (“{}”) is not drawn: notes under the chart refer to values on a categorical or regular axis",
                    p.text,
                    note.trim()
                ),
            )
            .at(format!("data.series[0].points[{j}][1].note")),
        );
    }
    let sl = chartgen_format::scale_labels(&plan.spec, scale)?;
    // everything the layout might set, in one batch
    need_scale_texts(batch, &sl)?;
    need_axis(batch, t0, t1)?;
    fig::need_title(plan, batch)?;
    for p in pts.iter().filter(|p| p.d.value.is_none()) {
        let plain = guides::missing_label(plan, &p.d.status)?;
        batch.need("missing-note", &plain)?;
        for (a, b) in fig::splits(&plain) {
            batch.need("missing-note", &a)?;
            batch.need("missing-note", &b)?;
        }
    }
    batch.need("missing-note", "H")?;
    events::need(plan, batch)?;
    let draw = |w: f64| -> Result<annotate::Drawn> {
        let (mut pad, mut edge) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
        let mut depth = 0;
        loop {
            match pass(
                plan,
                data,
                scale,
                &sl,
                batch,
                &pts,
                (t0, t1, grain, how.as_str()),
                thin,
                pad,
                edge,
                depth,
                w,
            )? {
                Pass::Done(laid) => return Ok(annotate::Drawn::Laid(laid)),
                Pass::Refused(f) => return Ok(annotate::Drawn::Refused(vec![f])),
                Pass::Again(p, e) => {
                    pad = p;
                    edge = e;
                    depth += 1;
                }
            }
        }
    };
    match annotate::layout(plan, data, batch, &draw)? {
        Err(mut findings) => {
            let mut all = pre;
            all.append(&mut findings);
            Ok(LayoutStep::Refused { findings: all })
        }
        Ok(laid) => {
            let mut findings = pre;
            findings.extend(laid.findings.iter().cloned());
            let mut summary = laid.summary;
            summary.insert("width".into(), json!(r2(laid.width)));
            summary.insert("height".into(), json!(r2(laid.height)));
            Ok(LayoutStep::Complete {
                chart: PlacedChart {
                    width: laid.width,
                    height: laid.height,
                    primitives: vec![fig::lower(&laid.root, batch.theme)?],
                    findings,
                    summary,
                    decisions: laid.decisions,
                },
            })
        }
    }
}

enum Pass {
    Done(annotate::Laid),
    Refused(Finding),
    Again((f64, f64), (f64, f64)),
}

#[allow(clippy::too_many_arguments)]
fn pass(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: &TrainedScale,
    sl: &ScaleLabels,
    batch: &Batch,
    pts: &[Pt],
    (t0, t1, grain, how): (NaiveDateTime, NaiveDateTime, &'static str, &str),
    thin: bool,
    pad: (f64, f64),
    edge: (f64, f64),
    depth: usize,
    w_total: f64,
) -> Result<Pass> {
    let em = batch.metric("--label-size")?;
    let sid = data.series[0].id.clone();
    let mut findings = vec![];
    let n = sl.labels.len();
    let mut xr = batch.width("scale-label", &sl.top.number)?;
    for l in &sl.labels[..n.saturating_sub(1)] {
        xr = xr.max(batch.width("scale-label", l)?);
    }
    let plot_l = xr + batch.metric("--scale-gap")? + pad.0;
    let plot_r = w_total - pad.1;
    let plot_w = plot_r - plot_l;
    let aspect = match plan.spec.raw.pointer("/layout/plot_aspect") {
        Some(Value::Number(v)) => v.as_f64().unwrap_or(0.6),
        Some(Value::String(s)) => s.parse().unwrap_or(0.6),
        _ => batch.metric("--plot-aspect")?,
    };
    let plot_h = aspect * plot_w;
    if plot_w <= 0. {
        return Err(err("measured scale labels leave no usable plot width"));
    }
    let span = ta::secs(t0, t1);
    let x_of =
        |t: NaiveDateTime| plot_l + edge.0 + (plot_w - edge.0 - edge.1) * ta::secs(t0, t) / span;
    let yv = |v: Decimal| -> Result<f64> { Ok(plot_h * (1. - scale.fraction(v)?)) };
    let y0 = yv(Decimal::ZERO.max(scale.lo).min(scale.hi))?;
    let mut paths: Vec<Item> = vec![];
    let mut bars: Vec<Item> = vec![];
    let mut texts: Vec<(usize, Txt)> = vec![];
    let mut lead_lines: Vec<Item> = vec![];
    let mut decisions: Vec<Value> = vec![];
    if !thin {
        let mut runs: Vec<Vec<(NaiveDateTime, NaiveDateTime, Decimal, NaiveDateTime)>> = vec![];
        let mut cur = vec![];
        for (i, p) in pts.iter().enumerate() {
            let start = p.t.max(t0);
            let end = if i + 1 < pts.len() { pts[i + 1].t } else { t1 };
            if let Some(v) = p.d.value {
                cur.push((start, end, v, p.t));
            } else if !cur.is_empty() {
                runs.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            runs.push(cur);
        }
        let gaps: Vec<String> = pts
            .iter()
            .filter(|p| p.d.value.is_none())
            .map(|p| ta::instant_name(p.t, p.res))
            .collect();
        for run in runs {
            let mut ps = vec![];
            for (start, end, v, _) in &run {
                let y = yv(*v)?;
                ps.push((x_of(*start), y));
                ps.push((x_of(*end), y));
            }
            paths.push(Item::path(
                ps,
                "step-line single",
                attrs(&[
                    ("series", sid.clone()),
                    ("from", ta::stamp(run[0].3)),
                    ("to", ta::stamp(run[run.len() - 1].3)),
                ]),
            ));
        }
        let last = pts.last().and_then(|p| p.d.value);
        decisions.push(fig::decision(
            "R6-8",
            format!(
                "a step chart: each value level from its change to the next, then straight up or down to the new one; {} changes; the last value {}runs to the end of the axis, {} (G7.19, D-42); no markers",
                pts.len(),
                last.map(|v| format!("({}) ", v.normalize())).unwrap_or_default(),
                end_name(plan, t1)
            ),
            None,
        ));
        if !gaps.is_empty() {
            decisions.push(fig::decision(
                "R6-22",
                format!(
                    "a state not known leaves a gap from its change to the next, never a line at zero: {}",
                    gaps.join(", ")
                ),
                None,
            ));
        }
    } else {
        let bw = batch.metric("--event-bar-width")?;
        let gap = batch.metric("--grid-width")?;
        let mut stacks: Vec<((NaiveDateTime, bool), f64)> = vec![];
        let mut keys = vec![];
        let mut coincident: Vec<(NaiveDateTime, usize)> = vec![];
        for p in pts {
            let k = instant(p.t, p.res, true);
            let Some(v) = p.d.value else { continue };
            let up = v >= Decimal::ZERO;
            let slot = stacks.iter().position(|s| s.0 == (k, up));
            let off = slot.map_or(0., |i| stacks[i].1);
            let y = yv(v)?;
            let length = (y - y0).abs();
            let mut d = attrs(&[
                ("series", sid.clone()),
                ("x", ta::stamp(p.t)),
                ("value", v.normalize().to_string()),
            ]);
            if off != 0. {
                d.push(("stacked".into(), "1".into()));
                match coincident
                    .iter_mut()
                    .find(|c: &&mut (NaiveDateTime, usize)| c.0 == k)
                {
                    Some(c) => c.1 += 1,
                    None => coincident.push((k, 2)),
                }
            }
            let top = if up { y0 - off - length } else { y0 + off };
            bars.push(Item::rect(
                x_of(k) - bw / 2.,
                top,
                bw,
                length,
                "event-bar single",
                d,
            ));
            match slot {
                Some(i) => stacks[i].1 = off + length + gap,
                None => stacks.push(((k, up), off + length + gap)),
            }
            keys.push(k);
        }
        decisions.push(fig::decision(
            "R6-11",
            format!(
                "{} events as thin bars, each at its instant, {} pt wide (D-42), rising from zero; nothing drawn between them{}",
                bars.len(),
                fig::fmt_g(bw),
                if grain == "day" {
                    " (an event dated to the day sits in the middle of its day)"
                } else {
                    ""
                }
            ),
            None,
        ));
        if !coincident.is_empty() {
            coincident.sort();
            decisions.push(fig::decision(
                "R6-11",
                format!(
                    "events at the same time stacked with a hairline gap, each its own length (D-42): {}",
                    coincident
                        .iter()
                        .map(|(k, n)| format!("{} ({n})", ta::instant_name(*k, grain)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None,
            ));
        }
        let mut close: Vec<f64> = stacks
            .iter()
            .map(|((k, _), _)| (x_of(*k) * 1000.).round() / 1000.)
            .collect();
        close.sort_by(|a, b| a.partial_cmp(b).unwrap());
        close.dedup();
        let near = close.windows(2).filter(|w| w[1] - w[0] < bw).count();
        if near > 0 {
            decisions.push(fig::decision(
                "R6-11",
                format!(
                    "{near} pair{} of events closer in time than a bar's width: their bars touch or overlap, as on the book's concert chart (s. 0183)",
                    if near > 1 { "s" } else { "" }
                ),
                None,
            ));
        }
        let unknown: Vec<&Pt> = pts.iter().filter(|p| p.d.value.is_none()).collect();
        if !unknown.is_empty() {
            let (got, leaders, steps) =
                event_notes(plan, batch, &unknown, &bars, &x_of, y0, plot_l, w_total)?;
            texts.extend(got.into_iter().map(|t| (NOTES, t)));
            lead_lines = leaders;
            decisions.push(fig::decision(
                "R9-7",
                format!(
                    "events whose value is not known are marked by a note at their instant, wrapped first (the empty slots' procedure, owner, 1 Oct 2026): {} (the names' break rules, G2.4)",
                    steps.join("; ")
                ),
                Some("guided"),
            ));
        }
    }
    let end = if grain == "day" {
        t1 - Duration::seconds(1)
    } else {
        t1
    };
    decisions.push(fig::decision(
        if thin { "R6-11" } else { "R2-39" },
        format!(
            "a true time axis (G2.42), {how}: {} to {}",
            ta::instant_name(t0, grain),
            ta::instant_name(end, grain)
        ),
        None,
    ));
    // the value scale, with the unit phrase inline or on its own line (6.4)
    let lw = batch.metric("--line-width")? / 2.;
    let hits_of = |b: Bx| -> Vec<String> {
        let mut out = vec![];
        for p in &paths {
            if let Item::Path { points, .. } = p {
                if points
                    .windows(2)
                    .any(|s| segment_meets_box(s[0], s[1], b, lw))
                {
                    out.push("line".to_owned());
                    break;
                }
            }
        }
        for r in &bars {
            if let (Some(rb), Item::Rect { data, .. }) = (r.bbox(), r) {
                if overlap(b, rb) {
                    out.extend(data.iter().filter(|d| d.0 == "x").map(|d| d.1.clone()));
                }
            }
        }
        out
    };
    let vs = vertical_scale(
        plan,
        batch,
        scale,
        sl,
        plot_l,
        plot_r,
        &yv,
        &hits_of,
        &mut findings,
        w_total,
        if thin { "bar" } else { "step" },
    )?;
    texts.extend(vs.texts.iter().cloned());
    decisions.extend(vs.decisions.iter().cloned());
    let mut grid = vs.grid.clone();
    // the time axis (6.3, phase 2)
    let axis = match time_axis(
        plan, batch, t0, t1, grain, &x_of, plot_h, plot_l, plot_r, w_total,
    )? {
        Ok(a) => a,
        Err(f) => return Ok(Pass::Refused(f)),
    };
    let mut over_l: f64 = 0.;
    let mut over_r: f64 = 0.;
    for t in axis.texts.iter().filter(|t| t.role == "time-label") {
        let b = t.bbox(0.);
        over_l = over_l.max(-b.0);
        over_r = over_r.max(b.2 - w_total);
    }
    if (over_l > 0.05 || over_r > 0.05) && depth < 3 {
        return Ok(Pass::Again((pad.0 + over_l, pad.1 + over_r), edge));
    }
    let width = |r: &str, t: &str| batch.width(r, t);
    let need = ta::continuous_edge(
        &axis.ax,
        &x_of,
        plot_l,
        plot_r,
        &width,
        batch.metric("--divider-clear")?,
    )?;
    if (need.0 > 0.01 || need.1 > 0.01) && depth < 8 {
        return Ok(Pass::Again(
            pad,
            (
                edge.0 + if need.0 > 0.01 { need.0 + 0.05 } else { 0. },
                edge.1 + if need.1 > 0.01 { need.1 + 0.05 } else { 0. },
            ),
        ));
    }
    texts.extend(axis.texts.iter().cloned().map(|t| (AXIS, t)));
    decisions.extend(axis.decisions.iter().cloned());
    if edge != (0., 0.) {
        decisions.push(fig::decision(
            "R2-39",
            format!(
                "time axis: the first and last labels kept {} pt clear of the outer dividers, centred on their periods, which start {:.1} pt in from the left divider and end {:.1} pt before the right one",
                fig::fmt_g(batch.metric("--divider-clear")?),
                edge.0,
                edge.1
            ),
            None,
        ));
    }
    if pad != (0., 0.) {
        decisions.push(fig::decision(
            "R2-39",
            format!(
                "the plot gives way by {:.1} pt so that the outer time labels stay inside the figure",
                pad.0 + pad.1
            ),
            None,
        ));
    }
    grid.extend(axis.dividers.iter().cloned());
    // dates marked on the axis (6.13.12)
    let mut event_lines = vec![];
    let mut ev_texts = vec![];
    if events::has_events(plan) {
        let mut mark_boxes: Vec<Bx> = bars.iter().filter_map(Item::bbox).collect();
        for p in &paths {
            if let Item::Path { points, .. } = p {
                for s in points.windows(2) {
                    let hw = lw;
                    mark_boxes.push((
                        s[0].0.min(s[1].0) - hw,
                        s[0].1.min(s[1].1) - hw,
                        s[0].0.max(s[1].0) + hw,
                        s[0].1.max(s[1].1) + hw,
                    ));
                }
            }
        }
        for l in &lead_lines {
            if let Item::Line { x1, y1, y2, .. } = l {
                mark_boxes.push((x1 - 0.5, y1.min(*y2), x1 + 0.5, y1.max(*y2)));
            }
        }
        let placed: Vec<Txt> = texts.iter().map(|(_, t)| t.clone()).collect();
        let top_y = yv(*scale.ticks.last().unwrap())?;
        let (ls, ts) = events::place(
            plan,
            batch,
            thin,
            &x_of,
            t0,
            t1,
            (plot_l, top_y, plot_r, y0),
            &placed,
            &mark_boxes,
            w_total,
            &mut findings,
            &mut decisions,
        )?;
        event_lines = ls;
        ev_texts = ts;
    }
    // the title above; everything shifted under it
    let block = fig::title_block(plan, batch, w_total, &mut findings)?;
    decisions.extend(block.decisions.iter().cloned());
    let mut dy = match block.last_baseline {
        Some(b) => b + batch.metric("--title-gap")? - vs.first_local,
        None => vs.first_asc - vs.first_local,
    };
    let top_ev = ev_texts
        .iter()
        .map(|t: &Txt| t.bbox(0.).1)
        .fold(0.0f64, f64::min);
    if block.last_baseline.is_some() && top_ev < vs.first_local - vs.first_asc {
        dy += vs.first_local - vs.first_asc - top_ev;
    }
    for (_, t) in &mut texts {
        t.y += dy;
    }
    for t in &mut ev_texts {
        t.y += dy;
    }
    for it in grid
        .iter_mut()
        .chain(axis.ticks.clone().iter_mut())
        .chain(paths.iter_mut())
        .chain(bars.iter_mut())
        .chain(lead_lines.iter_mut())
        .chain(event_lines.iter_mut())
    {
        it.shift(0., dy);
    }
    let mut ticks = axis.ticks.clone();
    for t in &mut ticks {
        t.shift(0., dy);
    }
    // groups, in drawing order
    let scale_g: Vec<Item> = texts
        .iter()
        .filter(|(g, _)| *g == SCALE)
        .map(|(_, t)| Item::Text(t.clone()))
        .collect();
    let mut axis_children: Vec<Item> = ticks;
    axis_children.extend(
        texts
            .iter()
            .filter(|(g, _)| *g == AXIS)
            .map(|(_, t)| Item::Text(t.clone())),
    );
    let dividers: Vec<Item> = grid
        .iter()
        .filter(|i| matches!(i, Item::Line { classes, .. } if classes[0] == "time-divider"))
        .cloned()
        .collect();
    let axis_children = add_halos(axis_children, &dividers, batch.metric("--divider-clear")?);
    let mut notes_children: Vec<Item> = texts
        .iter()
        .filter(|(g, _)| *g == NOTES)
        .map(|(_, t)| Item::Text(t.clone()))
        .collect();
    notes_children.extend(ev_texts.iter().cloned().map(Item::Text));
    notes_children.extend(lead_lines.iter().cloned());
    let series_g = Item::Group {
        classes: fig::cls("series single"),
        id: None,
        data: attrs(&[("series", sid.clone())]),
        children: if thin { bars.clone() } else { paths.clone() },
    };
    let mut panel_children = vec![Item::group("grid", grid)];
    if !event_lines.is_empty() {
        panel_children.push(Item::group("events", event_lines.clone()));
    }
    panel_children.push(Item::group("marks", vec![series_g]));
    panel_children.push(Item::group("scale value", scale_g));
    panel_children.push(Item::group("time-axis", axis_children));
    panel_children.push(Item::group("annotations", notes_children));
    let root = Item::Group {
        classes: fig::cls(if thin {
            "chart thin-bar vertical time"
        } else {
            "chart step vertical time"
        }),
        id: Some("chart".into()),
        data: vec![],
        children: vec![
            Item::group(
                "header",
                block.texts.iter().cloned().map(Item::Text).collect(),
            ),
            Item::Group {
                classes: fig::cls("panel"),
                id: Some("main".into()),
                data: vec![],
                children: panel_children,
            },
            Item::group("footer", vec![]),
        ],
    };
    let mut height = dy + plot_h;
    for t in texts
        .iter()
        .map(|(_, t)| t)
        .chain(&block.texts)
        .chain(&ev_texts)
    {
        height = height.max(t.bbox(0.).3);
    }
    for b in &bars {
        if let Some(bb) = b.bbox() {
            height = height.max(bb.3);
        }
    }
    height += 1.0;
    let _ = em;
    let mut summary = BTreeMap::new();
    summary.insert("orientation".into(), json!("vertical"));
    summary.insert(
        "plot".into(),
        json!([r2(plot_l), r2(dy), r2(plot_r), r2(dy + plot_h)]),
    );
    summary.insert("time".into(), json!([ta::iso(t0), ta::iso(t1)]));
    summary.insert("unit".into(), json!(axis.ax.unit));
    summary.insert("k".into(), json!(axis.ax.k));
    if !event_lines.is_empty() {
        summary.insert(
            "events".into(),
            json!(
                event_lines
                    .iter()
                    .filter_map(|l| match l {
                        Item::Line { data, .. } =>
                            data.iter().find(|d| d.0 == "x").map(|d| d.1.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            ),
        );
    }
    scale_summary(&mut summary, sl, false);
    Ok(Pass::Done(annotate::Laid {
        root,
        width: w_total,
        height,
        plot: (plot_l, dy, plot_r, dy + plot_h),
        findings,
        summary,
        time: Some(annotate::TimeMarks {
            t0,
            span,
            x0: plot_l + edge.0,
            w: plot_w - edge.0 - edge.1,
            dy,
            plot_h,
            scale: scale.clone(),
            thin,
        }),
        decisions,
    }))
}

/// Where the last value runs to, as the original names it (step._end_name).
fn end_name(plan: &SemanticPlan, t1: NaiveDateTime) -> String {
    let inline = |d: NaiveDate| {
        format!(
            "{} {}, {}",
            ta::MONTHS_INLINE[chrono::Datelike::month0(&d) as usize],
            chrono::Datelike::day(&d),
            chrono::Datelike::year(&d)
        )
    };
    match plan
        .spec
        .raw
        .pointer("/data/as_of")
        .and_then(Value::as_str)
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    {
        Some(d) => format!("the as-of date, {}", inline(d)),
        None => format!(
            "the end of the window, {}",
            inline((t1 - Duration::seconds(1)).date())
        ),
    }
}

/// The scale's report entries, as the original adds them after layout.
pub(crate) fn scale_summary(
    summary: &mut BTreeMap<String, Value>,
    sl: &ScaleLabels,
    horizontal: bool,
) {
    summary.insert(
        "labels".into(),
        json!(if horizontal { &sl.bare } else { &sl.labels }),
    );
    summary.insert("top".into(), json!(sl.top.inline()));
    summary.insert(
        "scale_title".into(),
        if horizontal {
            json!(sl.title)
        } else {
            Value::Null
        },
    );
}

/// Round to 2 decimals as the original's report geometry (Python's round).
pub(crate) fn r2(v: f64) -> f64 {
    // Python's round(v, 2): the exact binary value correctly rounded, ties to even
    format!("{v:.2}").parse().unwrap_or(v)
}

/// The notes of events whose value is not known: at the instant on the baseline, wrapped to
/// two lines, where they fit between the bars either side; else above every bar they would
/// cover, with a thin leader down to the instant.
#[allow(clippy::too_many_arguments)]
fn event_notes(
    plan: &SemanticPlan,
    batch: &Batch,
    unknown: &[&Pt],
    bars: &[Item],
    x_of: &dyn Fn(NaiveDateTime) -> f64,
    y0: f64,
    plot_l: f64,
    w_total: f64,
) -> Result<(Vec<Txt>, Vec<Item>, Vec<String>)> {
    let em = batch.metric("--label-size")?;
    let pad = batch.metric("--text-pad")?;
    let lift = batch.metric("--missing-note-lift")?;
    let mut steps = vec![];
    let pitch = batch.metric("--line-pitch")? * batch.metric("--note-size")? / em;
    let rects: Vec<Bx> = bars
        .iter()
        .filter_map(Item::bbox)
        .filter(|b| b.3 - b.1 > 0.01)
        .collect();
    let mut out: Vec<Txt> = vec![];
    let mut leaders = vec![];
    let width = |s: &str| batch.width("missing-note", s);
    for p in unknown {
        let k = instant(p.t, p.res, true);
        let cx = x_of(k);
        let plain = guides::missing_label(plan, &p.d.status)?;
        let key = ta::instant_name(p.t, p.res);
        let left = rects
            .iter()
            .filter(|r| r.2 <= cx + 0.01)
            .map(|r| r.2)
            .fold(f64::NEG_INFINITY, f64::max);
        let right = rects
            .iter()
            .filter(|r| r.0 >= cx - 0.01)
            .map(|r| r.0)
            .fold(f64::INFINITY, f64::min);
        let clear = 2.
            * (cx - if left.is_finite() { left } else { plot_l })
                .min(if right.is_finite() { right } else { w_total } - cx);
        let room = (clear - 2. * pad).min(2. * (cx - plot_l).min(w_total - cx));
        let ls = if room > 0. {
            fig::best_lines(&plain, &width, Some(room), 2)?
        } else {
            None
        };
        if let Some(ls) = &ls {
            let placed = note_texts(batch, ls, cx, y0 - lift, &key, pitch)?;
            if !placed
                .iter()
                .any(|a| out.iter().any(|b| overlap(a.bbox(0.), b.bbox(0.))))
            {
                out.extend(placed);
                steps.push(format!(
                    "{key}: on the baseline at its instant{}",
                    if ls.len() == 2 { ", on 2 lines" } else { "" }
                ));
                continue;
            }
        }
        let mut options = vec![vec![plain.clone()]];
        if let Some(two) = fig::splits(&plain)
            .into_iter()
            .map(|(a, b)| -> Result<(f64, Vec<String>)> {
                Ok((width(&a)?.max(width(&b)?), vec![a, b]))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        {
            options.push(two.1);
        }
        let mut best: Option<(f64, Vec<String>, f64, f64)> = None;
        for ls in options {
            let mut w: f64 = 0.;
            for l in &ls {
                w = w.max(width(l)?);
            }
            let x = cx.max(plot_l + w / 2.).min(w_total - w / 2.);
            let half = w / 2. + pad;
            let mut tops: Vec<f64> = rects
                .iter()
                .filter(|b| b.0 < x + half && b.2 > x - half)
                .map(|b| b.1)
                .collect();
            tops.extend(out.iter().filter_map(|o| {
                let b = o.bbox(0.);
                (b.0 < x + half && b.2 > x - half).then_some(b.1 - pad)
            }));
            let yb = tops.into_iter().fold(y0, f64::min) - lift - 0.25 * em;
            let score = yb - (ls.len() - 1) as f64 * pitch;
            if best.as_ref().is_none_or(|b| score > b.0 + 0.01) {
                best = Some((score, ls, x, yb));
            }
        }
        let (_, ls, x, yb) = best.unwrap();
        let ts = note_texts(batch, &ls, x, yb, &key, pitch)?;
        let bottom = ts
            .iter()
            .map(|t| t.bbox(0.).3)
            .fold(f64::NEG_INFINITY, f64::max);
        out.extend(ts);
        steps.push(format!(
            "{key}: above the bars beside it{}, with a thin leader down to its instant",
            if ls.len() == 2 { ", on 2 lines" } else { "" }
        ));
        leaders.push(Item::line(
            cx,
            bottom + 0.25 * em,
            cx,
            y0,
            "leader",
            attrs(&[("x", ta::stamp(p.t))]),
        ));
    }
    Ok((out, leaders, steps))
}

fn note_texts(
    batch: &Batch,
    ls: &[String],
    cx: f64,
    bottom: f64,
    key: &str,
    pitch: f64,
) -> Result<Vec<Txt>> {
    let n = ls.len();
    ls.iter()
        .enumerate()
        .map(|(k, l)| {
            let mut t = batch.plain(
                "missing-note",
                l,
                cx,
                bottom - (n - 1 - k) as f64 * pitch,
                "middle",
            )?;
            if n > 1 {
                t.joint = Some(format!("note-{key}"));
            }
            Ok(t)
        })
        .collect()
}
