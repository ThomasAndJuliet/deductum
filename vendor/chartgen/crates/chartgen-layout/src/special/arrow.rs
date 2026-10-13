//! Arrow charts: each category's change between two periods, ported from the original's
//! `layout/arrowchart.py` (notes 06 §5.4, DP6-3; G1.46, G1.47, G7.21, G7.22; D-40): one row
//! per category, two panels sharing the rows (the values before to after, and the change in
//! % from 0 %), horizontal scales below with their titles, arrows with a start dot, a shaft
//! and a head whose tip sits on the end value, coloured good and bad or rise and fall, and a
//! key of the two periods over the ends of one arrow.
use super::annotate::{Drawn, Laid};
use super::fig::{self, Batch, Bx, Item, Txt, attrs, overlap};
use super::regular as rg;
use super::step::{r2, refusal};
use super::*;
use chartgen_format::ScaleLabels;
use rust_decimal::prelude::ToPrimitive;
use serde_json::Value;

struct Row {
    id: String,
    index: usize,
    name: String,
    content: TextContent,
    plain_name: bool,
    before: Option<Decimal>,
    after: Option<Decimal>,
    lines: Vec<String>,
    colour: String,
    note: String,
}

impl Row {
    fn drawn(&self) -> bool {
        self.before.is_some() && self.after.is_some()
    }
    fn pct(&self) -> Option<Decimal> {
        let (b, a) = (self.before?, self.after?);
        if b <= Decimal::ZERO {
            return None;
        }
        Some((a - b) / b * Decimal::from(100))
    }
    /// A one-line name keeps its own content (markup); wrapped lines are plain.
    fn line_content(&self, l: &str) -> TextContent {
        if self.lines.len() > 1 {
            TextContent::plain(l)
        } else {
            self.content.clone()
        }
    }
}

const HEAD_MIN: f64 = 0.5;

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
    let draw = |w: f64| draw(plan, data, scale, batch, w);
    match super::annotate::layout(plan, data, batch, &draw)? {
        Err(findings) => Ok(LayoutStep::Refused { findings }),
        Ok(laid) => {
            let mut summary = laid.summary;
            summary.insert("width".into(), json!(r2(laid.width)));
            summary.insert("height".into(), json!(r2(laid.height)));
            Ok(LayoutStep::Complete {
                chart: PlacedChart {
                    width: laid.width,
                    height: laid.height,
                    primitives: vec![fig::lower(&laid.root, batch.theme)?],
                    findings: laid.findings,
                    summary,
                    decisions: laid.decisions,
                },
            })
        }
    }
}

/// The panels drawn, or a refusal (R6-17).
fn panels(
    plan: &SemanticPlan,
    rows: &[Row],
    findings: &mut Vec<Finding>,
    decisions: &mut Vec<Value>,
) -> std::result::Result<Vec<&'static str>, Finding> {
    let raw = &plan.spec.raw;
    let want = raw
        .pointer("/chart/change_panels")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let relative_only =
        raw.pointer("/facts/message").and_then(Value::as_str) == Some("relative_change");
    let mut panels: Vec<&'static str> = match want {
        "both" => vec!["absolute", "relative"],
        "absolute" => vec!["absolute"],
        "relative" => vec!["relative"],
        _ if relative_only => vec!["relative"],
        _ => vec!["absolute", "relative"],
    };
    if want == "relative" && !relative_only {
        super::step::guided(
            plan,
            findings,
            "R6-15",
            "chart.change_panels",
            "relative",
            "both",
            "change in units and change in % side by side, unless the message is only about relative change (G1.47)",
        );
    }
    // the absolute panel alone (R6-17) is refused, or overridden, by the policy before layout
    let undefined: Vec<&str> = rows
        .iter()
        .filter(|r| r.drawn() && r.before.unwrap() <= Decimal::ZERO)
        .map(|r| r.name.as_str())
        .collect();
    if panels.contains(&"relative") && !undefined.is_empty() {
        panels.retain(|p| *p != "relative");
        decisions.push(fig::decision(
            "R6-17",
            format!(
                "the change in % is undefined where the before value is 0 or negative ({}): the % panel is left out (D-40)",
                undefined.join(", ")
            ),
            None,
        ));
        if panels.is_empty() {
            return Err(refusal(
                plan,
                "R6-17",
                format!(
                    "only the % panel was asked for, and the change in % is undefined for {}",
                    undefined.join(", ")
                ),
                "data.series",
                Some("before values above zero".into()),
            ));
        }
    }
    let what = |p: &str| {
        if p == "absolute" {
            "the values, before to after"
        } else {
            "the change in %, from 0 %"
        }
    };
    decisions.push(fig::decision(
        "R6-17",
        if panels.len() == 2 {
            format!(
                "two panels sharing the rows: {} (G1.47, G7.21)",
                panels
                    .iter()
                    .map(|p| what(p))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        } else {
            format!(
                "one panel: {}{}",
                what(panels[0]),
                if relative_only {
                    " (the message is only about relative change, G1.47)"
                } else {
                    ""
                }
            )
        },
        None,
    ));
    Ok(panels)
}

fn pct_scale(
    plan: &SemanticPlan,
    rows: &[Row],
    target: Option<i64>,
) -> Result<(TrainedScale, ScaleLabels)> {
    let (steps, allow25, target0, breathing, ..) = chartgen_scales::spec_settings(&plan.spec)?;
    let mut vals: Vec<Decimal> = rows.iter().filter_map(Row::pct).collect();
    vals.push(Decimal::ZERO);
    let sc = chartgen_scales::nice_scale(
        &vals,
        "bar",
        "include",
        "meaningful",
        None,
        None,
        &steps,
        allow25,
        target.unwrap_or(target0),
        None,
        None,
        None,
        None,
        breathing,
        false,
    )?;
    let mut labels = chartgen_format::format_scale(
        &sc.ticks,
        sc.step,
        "change",
        &chartgen_format::Unit::from_value(&json!({"suffix": "%"})),
        Some(0),
        "auto",
        &Value::Null,
        chartgen_format::locale("en-GB"),
        "",
    )?;
    let name = plan
        .spec
        .raw
        .pointer("/data/measure/name")
        .and_then(Value::as_str)
        .unwrap_or("value");
    labels.title = format!("Change in {name}");
    Ok((sc, labels))
}

fn abs_scale(
    plan: &SemanticPlan,
    rows: &[Row],
    target: i64,
) -> Result<(TrainedScale, ScaleLabels)> {
    let raw = &plan.spec.raw;
    let (steps, allow25, _, breathing, step, precision, min, max) =
        chartgen_scales::spec_settings(&plan.spec)?;
    let num = |p: &str| -> Option<Decimal> {
        raw.pointer(p)
            .filter(|v| v.is_number())
            .and_then(|v| v.to_string().parse().ok())
    };
    let vals: Vec<Decimal> = rows
        .iter()
        .filter(|r| r.drawn())
        .flat_map(|r| [r.before.unwrap(), r.after.unwrap()])
        .collect();
    let sc = chartgen_scales::nice_scale(
        &vals,
        "arrow",
        raw.pointer("/scale/zero")
            .and_then(Value::as_str)
            .unwrap_or("auto"),
        raw.pointer("/data/measure/zero")
            .and_then(Value::as_str)
            .unwrap_or("meaningful"),
        num("/data/measure/min"),
        num("/data/measure/max"),
        &steps,
        allow25,
        target,
        step,
        min,
        max,
        precision,
        breathing,
        raw.pointer("/facts/distance_to_zero_irrelevant") == Some(&Value::Bool(true)),
    )?;
    let labels = chartgen_format::scale_labels(&plan.spec, &sc)?;
    Ok((sc, labels))
}

fn intervals(sc: &TrainedScale) -> f64 {
    ((sc.hi - sc.lo) / sc.step).to_f64().unwrap_or(1.)
}

/// The least row pitch at which neighbouring names stay `sep` apart from ink to ink.
fn rows_apart(names: &[Vec<(f64, f64)>], pitch: f64, sep: f64) -> f64 {
    let mut need: f64 = 0.;
    for w in names.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if a.is_empty() || b.is_empty() {
            continue;
        }
        need = need.max(
            (a.len() - 1) as f64 * pitch / 2.
                + (b.len() - 1) as f64 * pitch / 2.
                + a.last().unwrap().1
                + b[0].0
                + sep,
        );
    }
    need
}

#[allow(clippy::too_many_arguments)]
fn arrow(
    out: &mut Vec<Item>,
    x0: f64,
    x1: f64,
    cy: f64,
    colour: &str,
    d: &[(String, String)],
    hl: f64,
    hw: f64,
    ds: f64,
    short: &str,
) -> &'static str {
    let length = (x1 - x0).abs();
    let sgn = if x1 >= x0 { 1. } else { -1. };
    let room = length - ds / 2.;
    let head = |h: f64, w: f64, cls: &str| {
        let base = x1 - sgn * h;
        Item::path(
            vec![(base, cy - w / 2.), (x1, cy), (base, cy + w / 2.)],
            cls,
            d.to_vec(),
        )
    };
    let dot = || Item::Circle {
        cx: x0,
        cy,
        r: ds / 2.,
        classes: fig::cls(&format!("change-start {colour}")),
        data: d.to_vec(),
    };
    let head_cls = format!("change-head {colour}");
    if length <= 0.01 {
        out.push(dot());
        return "dot";
    }
    if short == "hidden" || room >= hl - 0.01 {
        let h = hl.min(length);
        if length > h + 0.01 {
            out.push(Item::path(
                vec![(x0, cy), (x1 - sgn * h, cy)],
                &format!("change-shaft {colour}"),
                d.to_vec(),
            ));
        }
        out.push(head(h, hw, &head_cls));
        out.push(dot());
        return if room >= hl - 0.01 { "full" } else { "hidden" };
    }
    if short == "head-only" && length < hl + ds / 2. {
        out.push(head(hl, hw, &head_cls));
        return "head";
    }
    if short == "fit" && room >= HEAD_MIN * hl {
        let k = room / hl;
        out.push(head(room, hw * k, &head_cls));
        out.push(dot());
        return "fitted";
    }
    out.push(dot());
    let mut halo = head(hl, hw, "arrow-halo");
    if let Item::Path { data, .. } = &mut halo {
        data.clear();
    }
    out.push(halo);
    out.push(head(hl, hw, &head_cls));
    "over"
}

fn draw(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: &TrainedScale,
    batch: &Batch,
    w_total: f64,
) -> Result<Drawn> {
    let raw = &plan.spec.raw;
    let em = batch.metric("--label-size")?;
    let grid = raw
        .pointer("/data/x/grid")
        .and_then(Value::as_str)
        .unwrap_or("none");
    if data.categories.len() != 2 {
        return Err(err(
            "an arrow chart needs exactly two periods, before and after",
        ));
    }
    let periods = data
        .categories
        .iter()
        .map(|v| rg::parse(v, grid))
        .collect::<Result<Vec<_>>>()?;
    let notes = raw.pointer("/labels/notes");
    let mut rows: Vec<Row> = data
        .series
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (b, a) = (&s.values[0], &s.values[1]);
            let status = if b.value.is_none() {
                &b.status
            } else {
                &a.status
            };
            let status = if status.is_empty() { "missing" } else { status };
            let named = raw["data"]["series"][i]["name"].clone();
            Ok(Row {
                id: s.id.clone(),
                index: i,
                name: s.name.clone(),
                content: guides::series_content(plan, data, i)?,
                plain_name: named.is_string() || named.get("plain").is_some(),
                before: b.value,
                after: a.value,
                lines: vec![s.name.clone()],
                colour: String::new(),
                note: notes
                    .and_then(|n| n.get(status))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("({status})")),
            })
        })
        .collect::<Result<_>>()?;
    let mut findings = vec![];
    let mut decisions: Vec<Value> = vec![];
    let panels = match panels(plan, &rows, &mut findings, &mut decisions) {
        Ok(p) => p,
        Err(f) => return Ok(Drawn::Refused(vec![f])),
    };
    // rows: by the before value, or the change in % when the change is the message
    if raw.pointer("/chart/sort").and_then(Value::as_str) != Some("given") {
        let by_change = matches!(
            raw.pointer("/facts/message").and_then(Value::as_str),
            Some("change" | "relative_change")
        );
        let (mut drawn, rest): (Vec<Row>, Vec<Row>) = rows
            .into_iter()
            .partition(|r| r.drawn() && (r.pct().is_some() || !by_change));
        drawn.sort_by(|a, b| {
            let ka = if by_change {
                a.pct().unwrap()
            } else {
                a.before.unwrap()
            };
            let kb = if by_change {
                b.pct().unwrap()
            } else {
                b.before.unwrap()
            };
            kb.cmp(&ka).then(a.index.cmp(&b.index))
        });
        decisions.push(fig::decision(
            "D-40",
            format!(
                "rows by {}, largest first{}",
                if by_change {
                    "the change in %"
                } else {
                    "the before value"
                },
                if rest.is_empty() {
                    String::new()
                } else {
                    format!(
                        "; last: {} (a value not known)",
                        rest.iter()
                            .map(|r| r.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            ),
            Some("guided"),
        ));
        drawn.extend(rest);
        rows = drawn;
    } else {
        decisions.push(fig::decision(
            "D-40",
            "rows in the order given",
            Some("open"),
        ));
    }
    // colours: good and bad where the measure gives the reading, else rise and fall
    let good = raw.pointer("/data/measure/good").and_then(Value::as_str);
    for r in &mut rows {
        if !r.drawn() {
            continue;
        }
        let (b, a) = (r.before.unwrap(), r.after.unwrap());
        let rise = a > b;
        r.colour = match good {
            Some(g @ ("higher" | "lower")) => {
                if a == b {
                    "series-1".into()
                } else if rise == (g == "higher") {
                    "good".into()
                } else {
                    "bad".into()
                }
            }
            _ => if a >= b { "series-1" } else { "series-2" }.into(),
        };
    }
    // the colours' decision (R3-17) is the colour planner's: it falls here, as the original's
    decisions.push(json!({"colours": "here"}));
    if raw.get("highlight").is_some_and(|h| !h.is_null())
        || raw
            .pointer("/insight/targets")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    {
        findings.push(
            Finding::new("CG-3", "info", "highlighting an arrow is a later stage").at("highlight"),
        );
    }
    let base_labels = chartgen_format::scale_labels(&plan.spec, scale)?;
    let mut scales: BTreeMap<&str, (TrainedScale, ScaleLabels)> = BTreeMap::new();
    if panels.contains(&"absolute") {
        scales.insert("absolute", (scale.clone(), base_labels.clone()));
    }
    if panels.contains(&"relative") {
        let pct = pct_scale(plan, &rows, None)?;
        decisions.push(fig::decision(
            "02 D5",
            format!(
                "the % panel: {}, holding 0 %, where its arrows start",
                summary(&pct.0)
            ),
            None,
        ));
        scales.insert("relative", pct);
    }
    let names = raw.pointer("/data/x/names");
    let key_contents: Vec<TextContent> =
        data.categories
            .iter()
            .zip(&periods)
            .map(|(v, p)| {
                Ok(match names.and_then(|n| n.get(v)) {
                    Some(n) => guides::content(n)?
                        .unwrap_or_else(|| TextContent::plain(rg::period_name(p))),
                    None => TextContent::plain(rg::period_name(p)),
                })
            })
            .collect::<Result<_>>()?;

    // everything the layout might set
    for (sc, sl) in scales.values() {
        let _ = sc;
        for b in &sl.bare {
            batch.need("scale-label", b)?;
        }
        batch.need("scale-title", &sl.title)?;
        batch.need("scale-title", "H")?;
    }
    for r in &rows {
        batch.need_content("category-label", &r.content)?;
    }
    batch.need("category-label", "H")?;
    for c in &key_contents {
        batch.need_content("arrow-key", c)?;
    }
    for r in rows.iter().filter(|r| !r.drawn()) {
        batch.need("missing-note", &r.note)?;
    }
    batch.need("missing-note", "H")?;
    fig::need_title(plan, batch)?;

    // geometry: names, panels, rows
    let cap = batch.ext("category-label", "H")?.ascent;
    let pitch = batch.metric("--line-pitch")?;
    let limit = 0.3 * w_total;
    for r in &mut rows {
        if batch.ext_of("category-label", &r.content)?.2.width > limit && r.plain_name {
            let w = |s: &str| batch.width("category-label", s);
            r.lines = fig::best_lines(&r.name, &w, Some(limit), 2)?
                .unwrap_or_else(|| vec![r.name.clone()]);
        }
    }
    let mut lab_w: f64 = 0.;
    for r in &rows {
        for l in &r.lines {
            lab_w = lab_w.max(batch.ext_of("category-label", &r.line_content(l))?.2.width);
        }
    }
    let plot_l = lab_w + batch.metric("--category-label-gap")?;
    let sep = batch.metric("--time-label-sep")?;
    let geometry = |scales: &BTreeMap<&str, (TrainedScale, ScaleLabels)>| -> Result<(f64, BTreeMap<&'static str, (f64, f64)>)> {
        let first_w = |p: &str| batch.width("scale-label", &scales[p].1.bare[0]);
        let last_w = |p: &str| batch.width("scale-label", scales[p].1.bare.last().unwrap());
        let right = w_total - last_w(panels[panels.len() - 1])? / 2.;
        let mut spans = BTreeMap::new();
        if panels.len() == 2 {
            let g = batch.metric("--arrow-panel-gap")? + last_w(panels[0])? / 2. + first_w(panels[1])? / 2.;
            let p = (right - plot_l - g) / 2.;
            spans.insert(panels[0], (plot_l, plot_l + p));
            spans.insert(panels[1], (plot_l + p + g, right));
        } else {
            spans.insert(panels[0], (plot_l, right));
        }
        Ok((right, spans))
    };
    let (mut right, mut spans) = geometry(&scales)?;
    for &p in &panels {
        for target in [4, 3, 2] {
            let (sc, sl) = &scales[p];
            let mut wmax: f64 = 0.;
            for b in &sl.bare {
                wmax = wmax.max(batch.width("scale-label", b)?);
            }
            if (spans[p].1 - spans[p].0) / intervals(sc) >= wmax + sep {
                break;
            }
            let refit = if p == "absolute" {
                abs_scale(plan, &rows, target)?
            } else {
                pct_scale(plan, &rows, Some(target))?
            };
            for b in &refit.1.bare {
                batch.need("scale-label", b)?;
            }
            decisions.push(fig::decision(
                "02 D5",
                format!(
                    "the {} panel's labels would crowd in its width: {}",
                    if p == "absolute" { "values" } else { "%" },
                    summary(&refit.0)
                ),
                None,
            ));
            scales.insert(p, refit);
        }
        (right, spans) = geometry(&scales)?;
    }
    let mut band = batch.metric("--bar-row-pitch")?;
    let mut name_exts = vec![];
    for r in &rows {
        let mut v = vec![];
        for l in &r.lines {
            let e = batch.ext_of("category-label", &r.line_content(l))?.2;
            v.push((e.ascent, e.descent));
        }
        name_exts.push(v);
    }
    let need = rows_apart(&name_exts, pitch, batch.metric("--category-label-sep")?);
    if need > band + 0.01 {
        decisions.push(fig::decision(
            "R5-26",
            format!(
                "the rows {need:.1} pt apart, not {}, so that wrapped names stay {} pt clear of their neighbours (owner, 30 Sep 2026)",
                fig::fmt_g(band),
                fig::fmt_g(batch.metric("--category-label-sep")?)
            ),
            Some("guided"),
        ));
        band = need;
    }
    let n = rows.len();
    let plot_h = n as f64 * band;
    let xv = |p: &str, v: Decimal| -> Result<f64> {
        let (a, b) = spans[p];
        Ok(a + (b - a) * scales[p].0.fraction(v)?)
    };

    let mut texts: Vec<Txt> = vec![];
    let mut names_g = vec![];
    for (i, r) in rows.iter().enumerate() {
        let cy = (i as f64 + 0.5) * band;
        let first = cy - (r.lines.len() - 1) as f64 * pitch / 2. + cap / 2.;
        for (j, l) in r.lines.iter().enumerate() {
            let mut t = batch.text(
                "category-label",
                r.line_content(l),
                plot_l - batch.metric("--category-label-gap")?,
                first + j as f64 * pitch,
                "end",
            )?;
            t.plain = l.clone();
            names_g.push(Item::Text(t.clone()));
            texts.push(t);
        }
    }
    let hl = batch.metric("--arrow-head-length")?;
    let hw = batch.metric("--arrow-head-width")?;
    let ds = batch.metric("--arrow-start-size")?;
    let h0 = batch.ext("scale-label", "0")?.ascent;
    let cap_s = batch.ext("scale-title", "H")?.ascent;
    let short = batch
        .theme
        .tokens
        .get("--arrow-short-head")
        .and_then(Value::as_str)
        .unwrap_or("fit")
        .trim()
        .to_owned();
    let short = if ["fit", "overhang", "head-only", "hidden"].contains(&short.as_str()) {
        short
    } else {
        findings.push(Finding::new(
            "CG-4",
            "warning",
            format!(
                "--arrow-short-head {short:?} is not one of fit, overhang, head-only, hidden: fit"
            ),
        ));
        "fit".into()
    };
    let mut arrows: BTreeMap<(&str, String), (f64, f64, f64)> = BTreeMap::new();
    let mut shorts: Vec<(&str, Vec<String>)> = vec![];
    let mut panel_items: Vec<(&str, Vec<Item>, Vec<Item>, Vec<Item>, Vec<Item>)> = vec![];
    for &p in &panels {
        let (sc, sl) = &scales[p];
        let (a, b) = spans[p];
        let mut grid = vec![];
        let mut marks = vec![];
        let mut scale_v = vec![];
        let mut notes_g = vec![];
        let signed = (sc.lo < Decimal::ZERO && Decimal::ZERO < sc.hi)
            || (p == "relative" && sc.lo < Decimal::ZERO);
        for t in &sc.ticks {
            let mut c = "gridline".to_owned();
            if t.is_zero() {
                c += " zero";
                if signed {
                    c += " signed";
                }
            }
            let x = xv(p, *t)?;
            grid.push(Item::line(
                x,
                0.,
                x,
                plot_h,
                &c,
                attrs(&[("value", t.normalize().to_string())]),
            ));
        }
        let lab_b = plot_h + batch.metric("--scale-label-drop")? + h0;
        for (t, bare) in sc.ticks.iter().zip(&sl.bare) {
            let tx = batch.plain("scale-label", bare, xv(p, *t)?, lab_b, "middle")?;
            scale_v.push(Item::Text(tx.clone()));
            texts.push(tx);
        }
        let ub = lab_b
            + batch.ext("scale-label", "0")?.descent
            + batch.metric("--unit-line-drop")?
            + cap_s;
        let tt = batch.plain("scale-title", &sl.title, (a + b) / 2., ub, "middle")?;
        scale_v.push(Item::Text(tt.clone()));
        texts.push(tt);
        for (i, r) in rows.iter().enumerate() {
            let cy = (i as f64 + 0.5) * band;
            if !r.drawn() || (p == "relative" && r.pct().is_none()) {
                if !r.drawn() {
                    let cap_n = batch.ext("missing-note", "H")?.ascent;
                    let tx = batch.plain(
                        "missing-note",
                        &r.note,
                        a + batch.metric("--missing-note-inset")?,
                        cy + cap_n / 2.,
                        "start",
                    )?;
                    notes_g.push(Item::Text(tx.clone()));
                    texts.push(tx);
                }
                continue;
            }
            let (v0, v1, d) = if p == "absolute" {
                let (b0, a1) = (r.before.unwrap(), r.after.unwrap());
                (
                    b0,
                    a1,
                    attrs(&[
                        ("series", r.id.clone()),
                        ("from", b0.normalize().to_string()),
                        ("to", a1.normalize().to_string()),
                    ]),
                )
            } else {
                let pc = r.pct().unwrap();
                (
                    Decimal::ZERO,
                    pc,
                    attrs(&[
                        ("series", r.id.clone()),
                        ("from", "0".into()),
                        ("to", format!("{:.1}", pc.to_f64().unwrap_or(0.))),
                    ]),
                )
            };
            let (x0, x1) = (xv(p, v0)?, xv(p, v1)?);
            let mut g = vec![];
            let form = arrow(&mut g, x0, x1, cy, &r.colour, &d, hl, hw, ds, &short);
            if form != "full" && form != "dot" {
                let who = if panels.len() == 1 {
                    r.name.clone()
                } else {
                    format!(
                        "{} ({})",
                        r.name,
                        if p == "absolute" { "values" } else { "%" }
                    )
                };
                match shorts
                    .iter_mut()
                    .find(|s: &&mut (&str, Vec<String>)| s.0 == form)
                {
                    Some(e) => e.1.push(who),
                    None => shorts.push((form, vec![who])),
                }
            }
            marks.push(Item::Group {
                classes: fig::cls(&format!("series {}", r.colour)),
                id: None,
                data: attrs(&[("series", r.id.clone())]),
                children: g,
            });
            arrows.insert((p, r.id.clone()), (x0, x1, cy));
        }
        panel_items.push((p, grid, marks, scale_v, notes_g));
        decisions.push(fig::decision(
            if p == "absolute" { "R6-15" } else { "R6-17" },
            format!(
                "{}; the scale's title below: “{}”",
                if p == "absolute" {
                    format!(
                        "the values in the measure's unit, {}, each arrow from before to after",
                        summary(sc)
                    )
                } else {
                    format!("the change in %, {}, each arrow from 0 %", summary(sc))
                },
                sl.title
            ),
            None,
        ));
    }

    // the key: the two periods over the ends of one arrow (G7.21)
    decisions.push(fig::decision(
        "R6-15",
        "an arrow chart (G7.21): a dot at the start, a shaft, a head whose tip sits exactly on the end value; one row per category, its name at the left",
        None,
    ));
    if !shorts.is_empty() {
        let words = |k: &str| match k {
            "fitted" => "the head shrunk to fit between the start dot and the end",
            "over" => "the head over the start dot, haloed",
            "head" => "the head alone, no start dot",
            "hidden" => "the head under the start dot",
            _ => "no change: the start dot alone",
        };
        decisions.push(fig::decision(
            "R6-15",
            format!(
                "changes shorter than a full head with its start dot, their direction kept readable (--arrow-short-head: {short}): {}",
                shorts
                    .iter()
                    .map(|(k, v)| format!("{}: {}", words(k), v.join(", ")))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            Some("guided"),
        ));
    }
    let kp = if panels.contains(&"absolute") {
        "absolute"
    } else {
        "relative"
    };
    let mut cands: Vec<&Row> = rows
        .iter()
        .filter(|r| arrows.contains_key(&(kp, r.id.clone())))
        .collect();
    cands.sort_by(|a, b| {
        let (a0, a1, _) = arrows[&(kp, a.id.clone())];
        let (b0, b1, _) = arrows[&(kp, b.id.clone())];
        (b1 > b0)
            .cmp(&(a1 > a0))
            .then((b1 - b0).abs().partial_cmp(&(a1 - a0).abs()).unwrap())
    });
    let mut key_texts: Vec<Txt> = vec![];
    for r in &cands {
        let (x0, x1, cy) = arrows[&(kp, r.id.clone())];
        let w0 = batch.ext_of("arrow-key", &key_contents[0])?.2.width;
        let w1 = batch.ext_of("arrow-key", &key_contents[1])?.2.width;
        let y = cy - hw / 2. - 0.35 * em;
        let k0 = batch.text("arrow-key", key_contents[0].clone(), x0, y, "middle")?;
        let k1 = batch.text("arrow-key", key_contents[1].clone(), x1, y, "middle")?;
        let fits = (x1 - x0).abs() >= (w0 + w1) / 2. + sep
            && k0.bbox(0.).0 >= 0.
            && k1.bbox(0.).2 <= w_total
            && ![&k0, &k1]
                .iter()
                .any(|k| texts.iter().any(|t| overlap(k.bbox(0.), t.bbox(0.))));
        if fits {
            key_texts = vec![k0, k1];
            decisions.push(fig::decision(
                "R6-15",
                format!(
                    "the key “{}” and “{}” over the two ends of {}'s arrow, no legend (G7.21)",
                    key_contents[0].readable(),
                    key_contents[1].readable(),
                    r.name
                ),
                None,
            ));
            break;
        }
    }
    if key_texts.is_empty() && !cands.is_empty() {
        let r = cands
            .iter()
            .copied()
            .reduce(|a, b| {
                let (a0, a1, _) = arrows[&(kp, a.id.clone())];
                let (b0, b1, _) = arrows[&(kp, b.id.clone())];
                if (b1 - b0).abs() > (a1 - a0).abs() {
                    b
                } else {
                    a
                }
            })
            .unwrap();
        let (x0, x1, cy) = arrows[&(kp, r.id.clone())];
        let y = cy - hw / 2. - 0.35 * em;
        let (left, rightx) = if x1 >= x0 { (x0, x1) } else { (x1, x0) };
        let (c_left, c_right) = if x1 >= x0 {
            (&key_contents[0], &key_contents[1])
        } else {
            (&key_contents[1], &key_contents[0])
        };
        key_texts = vec![
            batch.text("arrow-key", c_left.clone(), left - 0.15 * em, y, "end")?,
            batch.text("arrow-key", c_right.clone(), rightx + 0.15 * em, y, "start")?,
        ];
        decisions.push(fig::decision(
            "R6-15",
            format!(
                "the key “{}” and “{}” beside the ends of {}'s arrow, the longest, too short to hold both words over it (G7.21)",
                key_contents[0].readable(),
                key_contents[1].readable(),
                r.name
            ),
            None,
        ));
    }
    texts.extend(key_texts.iter().cloned());

    // the title above; everything shifted under it
    let block = fig::title_block(plan, batch, w_total, &mut findings)?;
    decisions.extend(block.decisions.iter().cloned());
    let top_pad = key_texts
        .iter()
        .map(|t| -t.bbox(0.).1)
        .fold(0.0f64, f64::max);
    let dy = match block.last_baseline {
        Some(b) => b + batch.metric("--title-gap")? - h0 / 2.,
        None => 0.,
    } + top_pad;
    let mut key_items: Vec<Item> = key_texts.iter().cloned().map(Item::Text).collect();
    for it in names_g.iter_mut().chain(key_items.iter_mut()) {
        it.shift(0., dy);
    }
    for t in &mut texts {
        t.y += dy;
    }
    let mut root_children = vec![Item::group(
        "header",
        block.texts.iter().cloned().map(Item::Text).collect(),
    )];
    for (i, (p, grid, marks, scale_v, notes_g)) in panel_items.into_iter().enumerate() {
        let mut children = vec![
            Item::group("grid", grid),
            Item::group("marks", marks),
            Item::group("scale value", scale_v),
            Item::group("annotations", notes_g),
        ];
        if i == 0 {
            children.push(Item::group("scale category", names_g.clone()));
            children.push(Item::group("annotations", key_items.clone()));
        }
        let mut panel = Item::Group {
            classes: fig::cls("panel"),
            id: Some(p.into()),
            data: vec![],
            children,
        };
        if i == 0 {
            // the names and key were shifted already
            if let Item::Group { children, .. } = &mut panel {
                for c in children.iter_mut().take(4) {
                    c.shift(0., dy);
                }
            }
        } else {
            panel.shift(0., dy);
        }
        root_children.push(panel);
    }
    root_children.push(Item::group("footer", vec![]));
    let root = Item::Group {
        classes: fig::cls("chart arrow horizontal time"),
        id: Some("chart".into()),
        data: vec![],
        children: root_children,
    };
    let height = texts
        .iter()
        .chain(&block.texts)
        .map(|t| t.bbox(0.).3)
        .fold(dy + plot_h, f64::max)
        + 1.0;
    let mut summary = BTreeMap::new();
    summary.insert("orientation".into(), json!("horizontal"));
    summary.insert("panels".into(), json!(panels));
    summary.insert(
        "plot".into(),
        json!([r2(plot_l), r2(dy), r2(right), r2(dy + plot_h)]),
    );
    summary.insert(
        "rows".into(),
        json!(rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>()),
    );
    summary.insert(
        "colours".into(),
        Value::Object(
            rows.iter()
                .map(|r| {
                    (
                        r.id.clone(),
                        if r.colour.is_empty() {
                            Value::Null
                        } else {
                            json!(r.colour)
                        },
                    )
                })
                .collect(),
        ),
    );
    if let Some((sc, _)) = scales.get("relative") {
        summary.insert(
            "pct_scale".into(),
            json!(format!(
                "{}–{} by {}",
                plain_number(sc.lo),
                plain_number(sc.hi),
                plain_number(sc.step)
            )),
        );
    }
    // the report's scale entries, from the scale the chart was given (horizontal: bare labels)
    super::step::scale_summary(&mut summary, &base_labels, true);
    let _: Option<Bx> = None;
    Ok(Drawn::Laid(Laid {
        root,
        width: w_total,
        height,
        plot: (plot_l, dy, right, dy + plot_h),
        findings,
        summary,
        time: None,
        decisions,
    }))
}

/// `Scale.summary()`: "0–60 by 10".
fn summary(s: &TrainedScale) -> String {
    format!(
        "{}–{} by {}",
        plain_number(s.lo),
        plain_number(s.hi),
        plain_number(s.step)
    )
}

/// A limit or step as the original's report writes it (scales._plain).
pub(crate) fn plain_number(x: Decimal) -> String {
    let x = x.normalize();
    let text = x.abs().to_string();
    let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = whole.to_owned();
    if whole.len() >= 4 {
        let head = if whole.len() % 3 == 0 {
            3
        } else {
            whole.len() % 3
        };
        grouped = whole[..head].to_owned();
        let mut i = head;
        while i < whole.len() {
            grouped.push(',');
            grouped.push_str(&whole[i..i + 3]);
            i += 3;
        }
    }
    format!(
        "{}{}{}",
        if x < Decimal::ZERO { "\u{2212}" } else { "" },
        grouped,
        if frac.is_empty() {
            String::new()
        } else {
            format!(".{frac}")
        }
    )
}
