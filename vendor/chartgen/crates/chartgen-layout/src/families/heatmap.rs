//! Heatmaps: a port of the original `layout/heatmap.py` for heatmap tables (items as columns,
//! attributes as rows; R12-6, G7.36) and the key of a classed ramp (`legend.class_key`, G5.13,
//! R4-9).
//!
//! The geometry is the original's: names right-aligned at the left, headings centred over
//! columns at most --heat-max-column wide, the cells touching (each running a seam under the
//! next), a summary row a little apart, the values printed in their cells when every value fits
//! (`chart.value_labels` guided), else a key above the grid at its right.
//!
//! The colour classes of the cells and of the key's steps are the colour plan's ramp (ramps.py,
//! chartgen-colours), read through [`classing`]; a plan compiled without colours falls back to a
//! provisional five-step stand-in.
//!
//! A heatmap over time (its time axis under the grid) is not ported and is an explicit error.
use super::kit::{self, Kit, Placed, finding, guided, info, meta, r2, refusal};
use crate::table::{censored, known};
use chartgen_format as fmt;
use chartgen_model::*;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const SEAM: f64 = 1.0;

/// A pale step at fraction `f` of a shaded table's cell group (provisional, as `classing`).
pub fn pale_step(kit: &Kit, f: f64) -> Result<String> {
    let base = kit
        .token("--seq-5")
        .unwrap_or_else(|| kit.theme.palette[0].clone());
    Ok(blend(&base, "#ffffff", 0.12 + 0.43 * f.clamp(0., 1.)))
}
const SUMMARY: [&str; 2] = ["average", "total"];

/// One entry of a classed ramp's key (the palette's `RampKeyEntry`).
#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub classes: Vec<String>,
    pub lo: Option<Decimal>,
    pub hi: Option<Decimal>,
    pub single: Option<Decimal>,
    pub censored: Option<(String, Decimal)>,
}

/// The cells' colour classes and the key's steps, as the palette's ramp plan gives them.
#[derive(Debug, Clone)]
pub struct Classing {
    pub kind: String,
    pub method: Option<String>,
    /// `[series][category]`, without `labelled`.
    pub cells: Vec<Vec<Vec<String>>>,
    pub key: Vec<KeyEntry>,
    pub tick: Option<f64>,
    pub inner: Vec<Decimal>,
    pub lo: Option<Decimal>,
    pub hi: Option<Decimal>,
    pub dividing: Option<Decimal>,
}

/// The classing of the cells: the colour plan's ramp (chartgen-colours, ramps.classify), else
/// (a plan compiled without colours) the provisional stand-in below.
pub fn classing(plan: &SemanticPlan, data: &PreparedData) -> Classing {
    let Some(r) = &plan.colors.ramp else {
        return standin_classing(data);
    };
    Classing {
        kind: r.kind.clone(),
        method: (!r.method.is_empty()).then(|| r.method.clone()),
        cells: r.cells.clone(),
        key: r
            .key
            .iter()
            .map(|e| KeyEntry {
                classes: e.classes.clone(),
                lo: e.lo,
                hi: e.hi,
                single: e.single,
                censored: e.censored.clone(),
            })
            .collect(),
        tick: r.tick,
        inner: r.inner.clone(),
        lo: r.lo,
        hi: r.hi,
        dividing: r.dividing,
    }
}

/// The provisional stand-in where no colour plan was compiled: five equal sequential classes
/// over the known values' extent; censored values the end classes; values not known `missing`.
fn standin_classing(data: &PreparedData) -> Classing {
    let vals: Vec<Decimal> = data
        .series
        .iter()
        .flat_map(|s| s.values.iter())
        .filter(|d| known(d))
        .map(|d| d.value.unwrap())
        .collect();
    let lo = vals.iter().copied().min();
    let hi = vals.iter().copied().max();
    let steps = [1, 3, 5, 7, 9];
    let class_of = |v: Decimal| -> Vec<String> {
        let (Some(lo), Some(hi)) = (lo, hi) else {
            return vec!["seq-5".into()];
        };
        let f = if hi > lo {
            ((v - lo) / (hi - lo)).to_f64().unwrap_or(0.)
        } else {
            0.5
        };
        let k = ((f * 5.).floor() as usize).min(4);
        vec![format!("seq-{}", steps[k])]
    };
    let cells = data
        .series
        .iter()
        .map(|s| {
            s.values
                .iter()
                .map(|d| {
                    if known(d) {
                        class_of(d.value.unwrap())
                    } else if censored(d) {
                        vec![
                            if d.status == "below" {
                                "seq-1"
                            } else {
                                "seq-9"
                            }
                            .to_string(),
                        ]
                    } else {
                        vec!["missing".to_string()]
                    }
                })
                .collect()
        })
        .collect();
    let mut key = vec![];
    if let (Some(lo), Some(hi)) = (lo, hi) {
        for (k, step) in steps.iter().enumerate() {
            let a = lo + (hi - lo) * Decimal::from(k as i64) / Decimal::from(5);
            let b = lo + (hi - lo) * Decimal::from(k as i64 + 1) / Decimal::from(5);
            key.push(KeyEntry {
                classes: vec![format!("seq-{step}")],
                lo: Some(a.round_dp(2).normalize()),
                hi: Some(b.round_dp(2).normalize()),
                single: None,
                censored: None,
            });
        }
    }
    Classing {
        kind: "sequential".into(),
        method: Some("linear".into()),
        cells,
        inner: key.iter().skip(1).filter_map(|e| e.lo).collect(),
        key,
        tick: None,
        lo,
        hi,
        dividing: None,
    }
}

fn token_f(kit: &Kit, name: &str) -> Option<String> {
    kit.token(name).filter(|v| v.starts_with('#'))
}

/// The paint of a cell of these classes: the step's fill, paled for a labelled (muted) cell.
pub(crate) fn cell_paint(kit: &Kit, classes: &[String], pale: bool) -> String {
    let side = classes.iter().find(|c| *c == "good" || *c == "bad");
    let mut fill = None;
    for c in classes {
        if c == "missing" || c.starts_with("seq-") || c == "div-0" {
            fill = token_f(kit, &format!("--{c}"));
        } else if let Some(k) = c.strip_prefix("div-m").or_else(|| c.strip_prefix("div-p")) {
            fill = match side {
                Some(s) => token_f(kit, &format!("--{s}-{k}")),
                None => token_f(kit, &format!("--{c}")),
            };
        }
        if fill.is_some() {
            break;
        }
    }
    let fill = fill.unwrap_or_else(|| kit.theme.palette[0].clone());
    if !pale {
        return fill;
    }
    // the `labelled` version (ramps.pale_fills): the step at OKLCH lightness --cell-l-k (a
    // diverging step k out from the neutral at --cell-l-(2k+1)), its chroma at most --cell-chroma
    let mut k = None;
    for c in classes {
        if let Some(n) = c.strip_prefix("seq-") {
            k = n.parse::<usize>().ok();
        } else if let Some(n) = c.strip_prefix("div-m").or_else(|| c.strip_prefix("div-p")) {
            k = n.parse::<usize>().ok().map(|n| 2 * n + 1);
        }
    }
    match (
        k.and_then(|k| kit.metric(&format!("--cell-l-{k}")).ok()),
        kit.metric("--cell-chroma"),
    ) {
        (Some(l), Ok(cmax)) => {
            chartgen_colour_space::oklch_light(&fill, l, Some(cmax)).unwrap_or(fill)
        }
        _ => fill,
    }
}

/// G4.34 (ramps.lightest_distinct): the lightest step the key uses (the diverging neutral
/// left out) and its OKLab distances from the page and from the missing grey.
fn lightest_distinct(kit: &Kit, cl: &Classing) -> Option<(String, f64, f64)> {
    let fills: Vec<String> = cl
        .key
        .iter()
        .filter(|e| !e.classes.iter().any(|c| c == "div-0"))
        .map(|e| cell_paint(kit, &e.classes, false))
        .collect();
    let lightest = fills.iter().max_by(|a, b| {
        let l = |c: &str| chartgen_colour_space::oklch(c).map_or(0., |v| v[0]);
        l(a).partial_cmp(&l(b)).unwrap()
    })?;
    let page = token_f(kit, "--page").unwrap_or_else(|| "#ffffff".into());
    let miss = token_f(kit, "--missing").unwrap_or_else(|| "#a6a6a6".into());
    let lab = |c: &str| chartgen_colour_space::oklab(c);
    let (l, p, m) = (lab(lightest)?, lab(&page)?, lab(&miss)?);
    let dist = |a: [f64; 3], b: [f64; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    Some((lightest.clone(), dist(l, p), dist(l, m)))
}

fn rgb(c: &str) -> Option<[f64; 3]> {
    let h = c.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16)
            .ok()
            .map(|v| v as f64 / 255.)
    };
    Some([p(0)?, p(2)?, p(4)?])
}
fn blend(a: &str, b: &str, t: f64) -> String {
    match (rgb(a), rgb(b)) {
        (Some(x), Some(y)) => format!(
            "#{:02x}{:02x}{:02x}",
            ((x[0] * t + y[0] * (1. - t)) * 255.).round() as u8,
            ((x[1] * t + y[1] * (1. - t)) * 255.).round() as u8,
            ((x[2] * t + y[2] * (1. - t)) * 255.).round() as u8
        ),
        _ => a.to_string(),
    }
}

/// The words a key's title says (heatmap.key_title).
fn key_title(spec: &Value, e: i32) -> String {
    let m = &spec["data"]["measure"];
    let u = fmt::Unit::from_value(&m["unit"]);
    if !u.scale_title.is_empty() {
        return u.scale_title;
    }
    let mut bare = u.clone();
    bare.what = String::new();
    let base = fmt::scale_title(
        m["name"].as_str().unwrap_or(""),
        &bare,
        e,
        m["kind"].as_str().unwrap_or("count"),
        &spec["audience"]["abbreviations"],
    );
    if u.what.is_empty() {
        base
    } else if let Some(stripped) = base.strip_suffix(')') {
        format!("{stripped}; {})", u.what)
    } else {
        format!("{base} ({})", u.what)
    }
}

struct Key {
    swatches: Vec<(f64, f64, f64, f64, Vec<String>)>,
    tick: Option<(f64, f64, f64)>,
    texts: Vec<Placed>,
    width: f64,
}
impl Key {
    fn bbox(&self) -> [f64; 4] {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut add = |x: [f64; 4]| {
            b = [
                b[0].min(x[0]),
                b[1].min(x[1]),
                b[2].max(x[2]),
                b[3].max(x[3]),
            ];
        };
        for s in &self.swatches {
            add([s.0, s.1, s.0 + s.2, s.1 + s.3]);
        }
        for t in &self.texts {
            add(t.bbox());
        }
        if let Some((x, y0, y1)) = self.tick {
            add([x, y0, x, y1]);
        }
        b
    }
    fn shift(&mut self, dx: f64, dy: f64) {
        for s in &mut self.swatches {
            s.0 += dx;
            s.1 += dy;
        }
        for t in &mut self.texts {
            t.x += dx;
            t.y += dy;
        }
        if let Some(t) = &mut self.tick {
            t.0 += dx;
            t.1 += dy;
            t.2 += dy;
        }
    }
}

/// The key of a classed ramp (legend.class_key): equal swatches, the breaks under their
/// boundaries, a single value under its class's middle, the title above, the missing entry after.
fn class_key(
    kit: &Kit,
    cl: &Classing,
    label: &dyn Fn(Decimal) -> String,
    title: &str,
    missing: Option<&str>,
) -> Result<Key> {
    let h = kit.metric("--ramp-key-height")?;
    let gap = kit.metric("--ramp-key-gap")?;
    let sep = kit.metric("--time-label-sep")?;
    let entries = &cl.key;
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut widest: f64 = 0.;
    for e in entries {
        for v in [e.lo, e.hi, e.single].into_iter().flatten() {
            texts.insert(v.normalize().to_string(), label(v));
        }
        if let Some((status, lim)) = &e.censored {
            texts.insert(
                format!("{status} {}", lim.normalize()),
                format!(
                    "{}{}",
                    if status == "below" { "<" } else { ">" },
                    label(*lim)
                ),
            );
        }
    }
    for t in texts.values() {
        kit.need("legend-text", &[], &kit::plain(t))?;
    }
    for t in texts.values() {
        widest = widest.max(kit.width("legend-text", &[], &kit::plain(t))?);
    }
    let sw = (kit.metric("--ramp-key-width")? / entries.len().max(1) as f64).max(widest + sep);
    let mut key = Key {
        swatches: vec![],
        tick: None,
        texts: vec![],
        width: sw * entries.len() as f64,
    };
    for (k, e) in entries.iter().enumerate() {
        key.swatches
            .push((k as f64 * sw, 0., sw, h, e.classes.clone()));
    }
    let cap0 = kit.plain_ext("legend-text", &[], "0")?.ascent;
    let by = h + gap + cap0;
    let mut marks: Vec<(f64, String)> = vec![];
    for (k, e) in entries.iter().enumerate() {
        if let Some(v) = e.single {
            marks.push(((k as f64 + 0.5) * sw, v.normalize().to_string()));
        }
        if let Some((status, lim)) = &e.censored {
            marks.push((
                (k as f64 + 0.5) * sw,
                format!("{status} {}", lim.normalize()),
            ));
        }
        if let Some(lo) = e.lo {
            if k == 0 || entries[k - 1].hi.is_none_or(|hi| hi != lo) {
                marks.push((k as f64 * sw, lo.normalize().to_string()));
            }
        }
        if let Some(hi) = e.hi {
            marks.push(((k + 1) as f64 * sw, hi.normalize().to_string()));
        }
    }
    let mut seen = vec![];
    let mut placed = vec![];
    for (x, v) in marks {
        let id = ((x * 1000.).round() as i64, v.clone());
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        placed.push(kit.text("legend-text", &[], &kit::plain(&texts[&v]), x, by, "middle")?);
    }
    placed.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
    if let Some(t) = cl.tick {
        let xd = (t + 0.5) * sw;
        key.tick = Some((xd, 0., h + kit.metric("--key-tick")?));
    }
    let e = kit.ext("legend-text", &[], &kit::plain(title))?;
    let mut all = vec![kit.text(
        "legend-text",
        &[],
        &kit::plain(title),
        0.,
        -gap - e.descent,
        "start",
    )?];
    all.extend(placed);
    if let Some(m) = missing {
        let k = kit.metric("--legend-key")?;
        let x = key.width + kit.metric("--legend-entry-gap")?;
        key.swatches
            .push((x, (h - k) / 2., k, k, vec!["missing".into()]));
        let cap_l = kit.plain_ext("legend-text", &[], "H")?.ascent;
        all.push(kit.text(
            "legend-text",
            &[],
            &kit::plain(m),
            x + k + kit.metric("--legend-gap")?,
            h / 2. + cap_l / 2.,
            "start",
        )?);
    }
    key.texts = all;
    Ok(key)
}

struct Row<'a> {
    index: usize,
    id: &'a str,
    role: &'a str,
    name: TextContent,
}

pub fn layout_heatmap(kit: &Kit, plan: &SemanticPlan, data: &PreparedData) -> Result<PlacedChart> {
    let spec = &plan.spec.raw;
    let mut findings: Vec<Finding> = kit::inset_info(spec, "heatmap").into_iter().collect();
    let mut decisions: Vec<Value> = vec![];
    let mut w_fig = kit.c.width;
    let em = kit.metric("--label-size")?;
    let x = &spec["data"]["x"];
    let xs = &data.categories;
    if x["kind"].as_str() == Some("time") {
        return Err(
            kit::err("a heatmap over time: its time axis under the grid is not ported")
                .at("data.x"),
        );
    }
    let loc = fmt::locale(
        spec.pointer("/audience/locale")
            .and_then(Value::as_str)
            .unwrap_or("en-GB"),
    );
    let rows_all: Vec<Row> = crate::table::rows(plan, data)?
        .into_iter()
        .map(|r| Row {
            index: r.index,
            id: r.id,
            role: r.role,
            name: r.name,
        })
        .collect();
    for r in &rows_all {
        if matches!(r.role, "reference" | "target" | "all_others" | "missing") {
            findings.push(info(
                "CG-3",
                &format!(
                    "a series of role {} in a heatmap is drawn as a row like the others",
                    r.role
                ),
                Some("data.series[].role"),
            ));
        }
    }
    if spec["highlight"].as_array().is_some_and(|h| !h.is_empty()) {
        findings.push(info(
            "CG-3",
            "highlighting cells of a heatmap is a later stage: none drawn",
            Some("highlight"),
        ));
    }
    let body: Vec<&Row> = rows_all
        .iter()
        .filter(|r| !SUMMARY.contains(&r.role))
        .collect();
    let summ: Vec<&Row> = rows_all
        .iter()
        .filter(|r| SUMMARY.contains(&r.role))
        .collect();
    let rows: Vec<&Row> = body.iter().chain(summ.iter()).copied().collect();
    let n = xs.len();
    let mut cols: Vec<usize> = (0..n).collect();
    if !summ.is_empty() {
        kit::decide(
            &mut decisions,
            "R12-6",
            format!(
                "the summary rows last, a little apart: {} (s. 0329)",
                summ.iter()
                    .map(|r| r.name.readable())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Some("guided"),
        );
    }
    let (sort, sort_by) = kit::choice_sort(spec, plan.chart_type);
    let sorted_by = matches!(sort.as_str(), "desc" | "asc")
        .then(|| rows_all.iter().find(|r| r.id == sort_by))
        .flatten();
    match sorted_by {
        Some(by) => kit::decide(
            &mut decisions,
            "02 D1",
            format!(
                "the columns sorted by {}, {} first (G2.1)",
                by.name.readable(),
                if sort == "desc" {
                    "largest"
                } else {
                    "smallest"
                }
            ),
            Some("guided"),
        ),
        None => kit::decide(
            &mut decisions,
            if x["kind"].as_str() == Some("ordinal") {
                "R2-02"
            } else {
                "02 D1"
            },
            "the columns in their given order",
            None,
        ),
    }
    if matches!(sort.as_str(), "desc" | "asc") {
        if let Some(by) = rows_all.iter().find(|r| r.id == sort_by) {
            let vals = &data.series[by.index].values;
            let key = |i: usize| {
                if known(&vals[i]) {
                    Some(vals[i].value.unwrap())
                } else {
                    None
                }
            };
            cols.sort_by(|&a, &b| {
                let o = key(a).cmp(&key(b));
                if sort == "desc" { o.reverse() } else { o }
            });
        }
    }
    // the numbers, one set
    let known_v: Vec<Decimal> = rows
        .iter()
        .flat_map(|r| data.series[r.index].values.iter())
        .filter(|d| known(d))
        .map(|d| d.value.unwrap())
        .collect();
    let limits: Vec<Decimal> = rows
        .iter()
        .flat_map(|r| data.series[r.index].values.iter())
        .filter(|d| censored(d))
        .map(|d| d.limit.unwrap())
        .collect();
    let unit = fmt::Unit::from_value(&spec["data"]["measure"]["unit"]);
    let prec = spec
        .pointer("/data/measure/precision")
        .and_then(|v| v.to_string().parse::<Decimal>().ok());
    let mut all = known_v.clone();
    all.extend(limits.iter().copied());
    let d = fmt::set_decimals(
        &all,
        prec,
        &unit,
        spec["data"]["measure"]["unit"]["magnitude"]
            .as_i64()
            .unwrap_or(0) as i32,
    );
    let code = |v: &Datum| -> (String, &'static str) {
        if known(v) {
            (
                fmt::number(v.value.unwrap(), d, loc, "", false),
                "table-cell",
            )
        } else if censored(v) {
            (
                fmt::censored_text(&v.status, v.limit.unwrap(), d, loc, ""),
                "table-cell",
            )
        } else {
            let status = if v.status.is_empty() {
                "missing"
            } else {
                &v.status
            };
            (crate::table::note_text(spec, status), "missing-note")
        }
    };
    let heads: Vec<TextContent> = (0..n)
        .map(|i| crate::table::header(spec, &xs[i]))
        .collect::<Result<_>>()?;
    let block = kit::title_block(kit, spec, w_fig, &mut findings, &mut decisions)?;
    let tb = block.last_baseline.unwrap_or(0.);
    let cap_h = kit.plain_ext("row-header", &[], "H")?.ascent;
    let h0 = kit.plain_ext("table-cell", &[], "0")?.ascent;
    let mut name_w: f64 = 0.;
    for r in &rows {
        name_w = name_w.max(kit.width("row-header", &[], &r.name)?);
    }
    let gap_l = kit.metric("--category-label-gap")?;
    let pad = kit.metric("--table-fill-pad")?;
    let mut val_w: f64 = 0.;
    for r in &rows {
        for v in &data.series[r.index].values {
            let (t, role) = code(v);
            val_w = val_w.max(kit.width(role, &[], &TextContent::plain(t))?);
        }
    }
    let plot_l = name_w + gap_l;
    let mut head_w: f64 = 0.;
    for &i in &cols {
        head_w = head_w.max(kit.width("column-header", &[], &heads[i])?);
    }
    let mut cell_w = val_w.max(head_w) + 2. * pad;
    let fits = cell_w <= kit.metric("--heat-max-column")? && plot_l + n as f64 * cell_w <= w_fig;
    cell_w = cell_w.min((val_w + 2. * pad).max((w_fig - plot_l) / n as f64));
    if head_w + 2. * pad > cell_w + 1e-6 {
        let need = plot_l + n as f64 * (head_w + 2. * pad);
        if refusal(
            spec,
            &mut findings,
            "CG-1",
            &format!(
                "{n} columns of {cell_w:.1} pt: the widest heading needs {:.1} pt, so neighbouring headings would meet",
                head_w + 2. * pad
            ),
            Some("layout.width"),
            Some(&format!(
                "layout.width at least {}, fewer items, or shorter names (data.x.names)",
                need as i64 + 1
            )),
        ) {
            return Ok(kit::refused(findings, decisions));
        }
    }
    let opt = spec
        .pointer("/chart/value_labels")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let rec = if fits { "on" } else { "off" };
    let why = if fits {
        "every value fits its cell: the numbers carry the values exactly and the colour shows the pattern (G7.36)"
    } else {
        "the values do not fit their cells: the colour alone, read against the key (G5.13)"
    };
    let labelled = if opt == "auto" {
        kit::decide(
            &mut decisions,
            "R12-6",
            format!(
                "{}{why}",
                if fits {
                    "each value printed in its cell; "
                } else {
                    "no values in the cells; "
                }
            ),
            Some("guided"),
        );
        fits
    } else {
        guided(
            spec,
            &mut findings,
            "R2-32",
            "chart.value_labels",
            opt,
            rec,
            why,
        );
        if opt == "on"
            && !fits
            && refusal(
                spec,
                &mut findings,
                "CG-1",
                &format!(
                    "values in cells {cell_w:.1} pt wide would overlap their neighbours (the widest needs {:.1} pt): text is never overlapped",
                    val_w + 2. * pad
                ),
                Some("chart.value_labels"),
                Some("value_labels auto or off (a key instead)"),
            )
        {
            return Ok(kit::refused(findings, decisions));
        }
        opt == "on"
    };
    // the cells' strength (chart.cells, guided)
    let asked = spec
        .pointer("/chart/cells")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let muted = if asked == "auto" {
        kit::decide(
            &mut decisions,
            "R12-6",
            format!(
                "the cells strong: a heatmap, whose colour is the message: the steps at full strength{} (owner, 1 Oct 2026)",
                if labelled {
                    ", each value black or white, whichever reads better on its cell"
                } else {
                    ""
                }
            ),
            Some("guided"),
        );
        false
    } else {
        guided(
            spec,
            &mut findings,
            "R12-6",
            "chart.cells",
            asked,
            "strong",
            &format!(
                "a heatmap, whose colour is the message: the steps at full strength{}",
                if labelled {
                    ", each value black or white, whichever reads better on its cell"
                } else {
                    ""
                }
            ),
        );
        asked == "muted"
    };
    // the ramp's classing decisions (ramps.classify) are the colour plan's, made here
    kit::colour_decisions_here(&mut decisions);
    let cl = classing(plan, data);
    if cl.method.is_none() && !labelled {
        return Err(kit::err(
            "an unclassed ramp's graduated key (legend.ramp_key) is not drawn by this layout",
        ));
    }
    let pitch = kit.metric("--cell-pitch")?;
    let widths = vec![cell_w; n];
    let width = plot_l + widths.iter().sum::<f64>();
    let title_right = block.texts.iter().map(|t| t.bbox()[2]).fold(0., f64::max);
    w_fig = width.max(title_right).min(kit.c.width);

    // the key, when the cells do not carry their values
    let mut key = None;
    if !labelled {
        let pct = spec.pointer("/data/measure/kind").and_then(Value::as_str) == Some("percent")
            || unit.suffix.trim() == "%";
        let plus = matches!(
            spec.pointer("/data/measure/kind").and_then(Value::as_str),
            Some("change" | "index")
        );
        let fv = move |v: Decimal| {
            let v = v.normalize();
            fmt::number(v, v.scale(), loc, "", plus) + if pct { "%" } else { "" }
        };
        let title = key_title(spec, 0);
        let miss: Vec<&Datum> = rows
            .iter()
            .flat_map(|r| data.series[r.index].values.iter())
            .filter(|d| !known(d) && !censored(d))
            .collect();
        let missing = (!miss.is_empty()).then(|| {
            if miss
                .iter()
                .all(|d| matches!(d.status.as_str(), "missing" | ""))
            {
                "Missing"
            } else {
                "Not known"
            }
        });
        let mut k = class_key(kit, &cl, &fv, &title, missing)?;
        let kb = k.bbox();
        let dx = (width - if missing.is_some() { kb[2] } else { k.width })
            .min(w_fig - kb[2])
            .max(-kb[0]);
        let dy = if block.texts.is_empty() {
            -kb[1]
        } else {
            tb + kit.metric("--title-gap")? - kb[1] - kit.plain_ext("legend-text", &[], "H")?.ascent
        };
        k.shift(dx, dy);
        kit::decide(
            &mut decisions,
            "R4-9",
            format!(
                "a key: a bar of the {} classes, “{title}” above it{}, above the grid at its right (G5.13, s. 0204)",
                cl.key.len(),
                if missing.is_some() {
                    ", and a separate entry for the values not known"
                } else {
                    ""
                }
            ),
            Some("locked"),
        );
        key = Some(k);
    } else {
        kit::decide(
            &mut decisions,
            "R4-9",
            "no key: every cell carries its value, which the colour only repeats (the book's heatmap table, s. 0329)",
            Some("guided"),
        );
    }
    // G4.34: the lightest step clear of the page and the missing grey
    if cl.method.is_some() && !muted {
        if let Some((colour, dp, dm)) = lightest_distinct(kit, &cl) {
            let text = format!(
                "the lightest step {colour}: OKLab ΔE {dp:.3} from the page and {dm:.3} from the missing grey{}",
                if key.is_some() {
                    "; the breaks in the key rounded and formatted as a set"
                } else {
                    ""
                }
            );
            if dp.min(dm) < 0.05 {
                findings.push(finding(
                    "G4.34",
                    "warn",
                    &format!("{text}: under 0.05, hard to tell apart (the style's ramp)"),
                    Some("--seq-1"),
                ));
            } else {
                kit::decide(&mut decisions, "G4.34", text, Some("locked"));
            }
        }
    }

    // geometry down
    let title_gap = kit.metric("--title-gap")?;
    let mut texts: Vec<Placed> = vec![];
    let hb = if key.is_some() {
        key.as_ref().unwrap().bbox()[3] + 0.8 * em + cap_h
    } else if !block.texts.is_empty() {
        tb + title_gap
    } else {
        cap_h
    };
    let mut lefts = BTreeMap::new();
    let mut xx = plot_l;
    for &i in &cols {
        lefts.insert(i, xx);
        xx += widths[i];
    }
    let mut desc: f64 = 0.;
    for &i in &cols {
        texts.push(kit.text(
            "column-header",
            &[],
            &heads[i],
            lefts[&i] + widths[i] / 2.,
            hb,
            "middle",
        )?);
        desc = desc.max(kit.ext("column-header", &[], &heads[i])?.descent);
    }
    let top = hb + desc + 0.35 * em;
    let mut y = top;
    let mut cells: Vec<Primitive> = vec![];
    let summary_gap = kit.metric("--summary-gap")?;
    let text_ink = kit.theme.text.clone();
    let dark = kit
        .token("--cell-text-dark")
        .unwrap_or_else(|| "#000000".into());
    let light = kit
        .token("--cell-text-light")
        .unwrap_or_else(|| "#ffffff".into());
    let mut tones: BTreeMap<&str, usize> = BTreeMap::new();
    let mut min_contrast = f64::INFINITY;
    for (j, r) in rows.iter().enumerate() {
        let mut row_cells: Vec<Primitive> = vec![];
        if SUMMARY.contains(&r.role) && j == body.len() && !body.is_empty() {
            y += summary_gap;
        }
        texts.push(kit.text(
            "row-header",
            &[],
            &r.name,
            name_w,
            y + pitch / 2. + cap_h / 2.,
            "end",
        )?);
        let below =
            j + 1 < rows.len() && !(SUMMARY.contains(&rows[j + 1].role) && j + 1 == body.len());
        for (k, &i) in cols.iter().enumerate() {
            let v = &data.series[r.index].values[i];
            let mut classes = vec!["cell".to_string()];
            classes.extend(cl.cells[r.index][i].iter().cloned());
            if muted && labelled {
                classes.push("labelled".into());
            }
            let value = if known(v) {
                kit::dec_text(v.value.unwrap())
            } else if censored(v) {
                format!("{} {}", v.status, kit::dec_text(v.limit.unwrap()))
            } else if v.status.is_empty() {
                "missing".into()
            } else {
                v.status.clone()
            };
            let fill = cell_paint(kit, &cl.cells[r.index][i], muted && labelled);
            let cls: Vec<&str> = classes.iter().map(String::as_str).collect();
            row_cells.push(kit::rect(
                lefts[&i],
                y,
                widths[i] + if k + 1 < cols.len() { SEAM } else { 0. },
                pitch + if below { SEAM } else { 0. },
                kit::fill(&fill),
                meta(
                    &cls,
                    &[
                        ("series", r.id.to_string()),
                        ("x", xs[i].clone()),
                        ("value", value),
                    ],
                ),
            ));
            if labelled {
                let (t, role) = code(v);
                let yb = y
                    + pitch / 2.
                    + if known(v) || censored(v) {
                        h0
                    } else {
                        cap_h * 0.8
                    } / 2.;
                let mut mods: Vec<&str> = vec![];
                if !muted {
                    let (tone, k) =
                        chartgen_colour_space::text_tone(&[fill.as_str()], &dark, &light)
                            .ok_or_else(|| {
                                kit::err(format!("the cell fill {fill} is not a colour"))
                            })?;
                    mods.push(if tone == "dark" {
                        "on-mid-cell"
                    } else {
                        "on-dark-cell"
                    });
                    *tones.entry(tone).or_default() += 1;
                    min_contrast = min_contrast.min(k);
                    let _ = &text_ink;
                }
                texts.push(kit.text(
                    role,
                    &mods,
                    &TextContent::plain(t),
                    lefts[&i] + widths[i] / 2.,
                    yb,
                    "middle",
                )?);
            }
        }
        cells.push(kit::group(
            &["series"],
            None,
            &[("series", r.id.to_string())],
            row_cells,
        ));
        y += pitch;
    }
    let bottom = y;
    kit::decide(
        &mut decisions,
        "R3-9",
        format!(
            "the cells coloured in classes, {} {} (provisional classing until the palette's ramp plan)",
            cl.kind,
            cl.method.as_deref().unwrap_or("")
        ),
        Some("locked"),
    );
    if labelled && !muted && !tones.is_empty() {
        kit::decide(
            &mut decisions,
            "R5-8",
            "values on strong cells: each value black or white, whichever has more contrast on its cell (owner, 1 Oct 2026)",
            Some("locked"),
        );
    }
    let miss_n = rows
        .iter()
        .flat_map(|r| data.series[r.index].values.iter())
        .filter(|d| !known(d) && !censored(d))
        .count();
    if miss_n > 0 {
        kit::decide(
            &mut decisions,
            "R6-23",
            format!("{miss_n} values not known in grey cells, never the colour of zero (G7.35)"),
            Some("locked"),
        );
    }
    let mut header_g: Vec<Primitive> = block.texts.iter().map(Placed::primitive).collect();
    if let Some(k) = &key {
        let mut key_g = vec![];
        for (x0, y0, w, h, classes) in &k.swatches {
            let mut c = vec!["legend-swatch".to_string()];
            c.extend(classes.iter().cloned());
            let cls: Vec<&str> = c.iter().map(String::as_str).collect();
            key_g.push(kit::rect(
                *x0,
                *y0,
                *w,
                *h,
                kit::fill(&cell_paint(kit, classes, false)),
                meta(&cls, &[]),
            ));
        }
        if let Some((xd, y0, y1)) = k.tick {
            key_g.push(kit::line(
                xd,
                y0,
                xd,
                y1,
                kit::stroke(&kit.theme.text, kit.metric("--grid-width")?),
                meta(&["key-tick"], &[]),
            ));
        }
        key_g.extend(k.texts.iter().map(Placed::primitive));
        header_g.push(kit::group(&["legend", "ramp-key"], None, &[], key_g));
    }
    let table_g: Vec<Primitive> = texts
        .iter()
        .filter(|t| matches!(t.role.as_str(), "column-header" | "row-header"))
        .map(Placed::primitive)
        .collect();
    let notes_g: Vec<Primitive> = texts
        .iter()
        .filter(|t| !matches!(t.role.as_str(), "column-header" | "row-header"))
        .map(Placed::primitive)
        .collect();
    let primitives = vec![kit::group(
        &["chart", "heatmap", "categorical"],
        Some("chart"),
        &[],
        vec![
            kit::group(&["header"], None, &[], header_g),
            kit::group(
                &["panel"],
                Some("main"),
                &[],
                vec![
                    kit::group(&["marks"], None, &[], cells),
                    kit::group(&["scale", "category"], None, &[], vec![]),
                    kit::group(&["table"], None, &[], table_g),
                    kit::group(&["annotations"], None, &[], notes_g),
                ],
            ),
            kit::group(&["footer"], None, &[], vec![]),
        ],
    )];
    let mut height = bottom;
    for t in texts
        .iter()
        .chain(block.texts.iter())
        .chain(key.iter().flat_map(|k| k.texts.iter()))
    {
        height = height.max(t.bbox()[3]);
    }
    if let Some(k) = &key {
        height = height.max(k.bbox()[3]);
    }
    height += 1.;
    let values = if labelled {
        if muted {
            json!({"mode": "muted", "tones": {}, "plates": 0, "min_contrast": null})
        } else {
            json!({"mode": "contrast", "tones": tones, "plates": 0,
                   "min_contrast": if min_contrast.is_finite() { json!(kit::round2(min_contrast)) } else { Value::Null }})
        }
    } else {
        Value::Null
    };
    let nn = |v: Option<Decimal>| v.map(|v| json!(kit::dec_text(v))).unwrap_or(Value::Null);
    let cens = rows
        .iter()
        .flat_map(|r| data.series[r.index].values.iter())
        .filter(|d| censored(d))
        .count();
    let mut summary = BTreeMap::new();
    summary.insert("orientation".into(), json!("vertical"));
    summary.insert(
        "plot".into(),
        json!([
            r2(plot_l),
            r2(top),
            r2(plot_l + widths.iter().sum::<f64>()),
            r2(bottom)
        ]),
    );
    summary.insert(
        "heatmap".into(),
        json!({
            "rows": rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            "columns": cols.iter().map(|&i| &xs[i]).collect::<Vec<_>>(),
            "labelled": labelled,
            "ramp": cl.kind,
            "cells": if muted { "muted" } else { "strong" },
            "classing": cl.method,
            "breaks": cl.inner.iter().map(|b| kit::dec_text(*b)).collect::<Vec<_>>(),
            "domain": [nn(cl.lo), nn(cl.hi)],
            "dividing": nn(cl.dividing),
            "key": key.as_ref().map(|k| k.texts.iter().map(|t| t.request.content.readable().to_string()).collect::<Vec<_>>()),
            "pitch": r2(pitch),
            "values": values,
            "censored": if cens > 0 { json!({"count": cens, "treatment": kit.token("--censored-cells").unwrap_or_else(|| "step".into())}) } else { Value::Null },
        }),
    );
    summary.insert("width".into(), r2(w_fig));
    summary.insert("height".into(), r2(height));
    let _ = finding;
    Ok(PlacedChart {
        decisions,
        width: w_fig,
        height,
        primitives,
        findings,
        summary,
    })
}
