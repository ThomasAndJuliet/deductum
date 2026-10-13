//! A table alone and action dots: a port of the original `layout/table.py` (SPEC §6.16;
//! R10-9, R10-12–R10-25, R12-4; G7.37–G7.47, G7.58; D-52).
//!
//! The table is formatted in the book's order: each column's digits as one set, two thin
//! rules, rows set well apart, a wider gutter before a Total column, numbers to be compared
//! right-aligned and bare, nothing bold but the title, the few cells that matter on a light
//! fill. Stub columns (text, or dates of fixed width) come before the names. The book's
//! rejected versions (stripes, bold) are drawn only through an override. In-table graphics:
//! bars and dots per column, a Trend column of sparklines, and (heat) shaded cells.
use super::kit::{self, Kit, Placed, dec_text, err, finding, meta, r2, refusal};
use chartgen_format as fmt;
use chartgen_model::*;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SUMMARY: [&str; 2] = ["total", "average"];

/// A series as the table reads it: its prepared values and its spec fields.
pub(crate) struct Row<'a> {
    pub index: usize,
    pub id: &'a str,
    pub role: &'a str,
    pub measure: Option<&'a str>,
    pub values: &'a [Datum],
    pub name: TextContent,
}

pub(crate) fn known(d: &Datum) -> bool {
    d.value.is_some() && matches!(d.status.as_str(), "" | "known")
}
pub(crate) fn censored(d: &Datum) -> bool {
    matches!(d.status.as_str(), "below" | "above") && d.limit.is_some()
}
fn status(d: &Datum) -> &str {
    if d.status.is_empty() {
        "missing"
    } else {
        &d.status
    }
}

pub(crate) fn rows<'a>(plan: &'a SemanticPlan, data: &'a PreparedData) -> Result<Vec<Row<'a>>> {
    let raw = &plan.spec.raw["data"]["series"];
    data.series
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let spec = raw
                .as_array()
                .and_then(|a| a.iter().find(|r| r["id"].as_str() == Some(&s.id)))
                .unwrap_or(&raw[i]);
            let name = match crate::guides::content(&spec["name"])? {
                Some(c) => c,
                None => TextContent::plain(if s.name.is_empty() { &s.id } else { &s.name }),
            };
            Ok(Row {
                index: i,
                id: &s.id,
                role: spec["role"].as_str().unwrap_or(""),
                measure: spec["measure"].as_str(),
                values: &s.values,
                name,
            })
        })
        .collect()
}

/// The words written for a value that is not known (labels.notes; the spec's defaults).
pub(crate) fn note_text(spec: &Value, status: &str) -> String {
    if let Some(notes) = spec.pointer("/labels/notes").and_then(Value::as_object) {
        return notes
            .get(status)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("({status})"));
    }
    match status {
        "tbd" => "(TBD)".into(),
        s => format!("({s})"),
    }
}

const MONTHS_AXIS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Row 1 of a period on its grid (timeaxis.row1, the full form).
pub(crate) fn period_row1(value: &str, grid: &str) -> Option<String> {
    use chrono::{Datelike, NaiveDate};
    match grid {
        "month" => {
            let (_, m) = value.split_once('-')?;
            let m: usize = m.parse().ok()?;
            MONTHS_AXIS.get(m.checked_sub(1)?).map(|s| s.to_string())
        }
        "quarter" => {
            let (_, q) = value.split_once("-Q")?;
            Some(format!("Q{}", q.parse::<u32>().ok()?))
        }
        "year" => Some(value.parse::<i32>().ok()?.to_string()),
        "day" => Some(
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()?
                .day()
                .to_string(),
        ),
        "hour" => {
            let (_, h) = value.split_once('T')?;
            let h: u32 = h.parse().ok()?;
            Some(format!(
                "{} {}",
                if h % 12 == 0 { 12 } else { h % 12 },
                if h < 12 { "AM" } else { "PM" }
            ))
        }
        "week" => {
            let start = if let Some((y, w)) = value.split_once("-W") {
                NaiveDate::from_isoywd_opt(y.parse().ok()?, w.parse().ok()?, chrono::Weekday::Mon)?
            } else {
                NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?
            };
            let end = start + chrono::Duration::days(6);
            let tail = if end.month() == start.month() {
                end.day().to_string()
            } else {
                format!("{} {}", MONTHS_AXIS[end.month0() as usize], end.day())
            };
            Some(format!(
                "{} {}–{}",
                MONTHS_AXIS[start.month0() as usize],
                start.day(),
                tail
            ))
        }
        _ => None,
    }
}

/// A column heading: the code's name, a period's row-1 label, or the value itself.
pub(crate) fn header(spec: &Value, value: &str) -> Result<TextContent> {
    let x = &spec["data"]["x"];
    if let Some(name) = x["names"].get(value) {
        if let Some(c) = crate::guides::content(name)? {
            return Ok(c);
        }
    }
    let grid = x["grid"].as_str();
    if x["kind"].as_str() == Some("time") && !matches!(grid, None | Some("none" | "bin")) {
        if let Some(t) = period_row1(value, grid.unwrap()) {
            return Ok(TextContent::plain(t));
        }
    }
    Ok(TextContent::plain(value))
}

fn dec(v: &Value) -> Option<Decimal> {
    if v.is_null() {
        return None;
    }
    let raw = v.to_string();
    raw.parse::<Decimal>()
        .ok()
        .or_else(|| Decimal::from_scientific(&raw).ok())
}

/// A measure as the table formats it.
struct Measure<'a> {
    raw: &'a Value,
    unit: fmt::Unit,
    precision: Option<Decimal>,
    magnitude: i32,
    kind: &'a str,
}
impl<'a> Measure<'a> {
    fn of(raw: &'a Value) -> Self {
        Self {
            raw,
            unit: fmt::Unit::from_value(&raw["unit"]),
            precision: dec(&raw["precision"]),
            magnitude: raw["unit"]["magnitude"].as_i64().unwrap_or(0) as i32,
            kind: raw["kind"].as_str().unwrap_or("count"),
        }
    }
    fn decimals(&self, values: &[Decimal]) -> u32 {
        fmt::set_decimals(values, self.precision, &self.unit, self.magnitude)
    }
    /// A value with its own unit (table._unit_text, R10-20): "$2.337M", "14.60%", "945.5".
    fn unit_text(&self, v: Decimal, d: u32, loc: fmt::Locale, abbr: &Value) -> String {
        let u = &self.raw["unit"];
        let prefix = u["prefix"].as_str().unwrap_or("");
        let suffix = match u["suffix"].as_str() {
            Some(s) if !s.is_empty() => s,
            _ if self.kind == "percent" => "%",
            _ => "",
        };
        let e = self.magnitude;
        let (word, attached) = if e != 0 {
            fmt::magnitude_word(e, abbr, "word_on_top")
        } else {
            (String::new(), false)
        };
        let after = if attached {
            word
        } else if word.is_empty() {
            String::new()
        } else {
            format!(" {word}")
        } + suffix;
        fmt::number(v, d, loc, prefix, matches!(self.kind, "change" | "index")) + &after
    }
}

struct Stub {
    head: Option<TextContent>,
    vals: Vec<String>,
}

/// [(series id, column index)] the highlight names, or without one the insight's targets.
pub(crate) fn highlighted(
    spec: &Value,
    rows: &[&Row],
    cols: &[usize],
    xs: &[String],
    findings: &mut Vec<Finding>,
) -> Vec<(String, usize)> {
    let mut refs: Vec<&Value> = vec![];
    for h in spec["highlight"].as_array().into_iter().flatten() {
        let method = h["method"].as_str().unwrap_or("auto");
        if !matches!(method, "auto" | "strong" | "background") {
            let mut f = finding(
                "CG-3",
                "info",
                &format!(
                    "highlight method {method} in a table: the cells are filled instead (G7.44)"
                ),
                Some("highlight"),
            );
            f.level = "open".into();
            findings.push(f);
        }
        refs.extend(h["targets"].as_array().into_iter().flatten());
    }
    if refs.is_empty() {
        refs.extend(
            spec.pointer("/insight/targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
    }
    let mut out = vec![];
    for r in refs {
        let cat = r["category"].as_str().or_else(|| r["x"].as_str());
        let series = r["series"].as_str();
        for s in rows {
            if series.is_some_and(|x| x != s.id) {
                continue;
            }
            for &i in cols {
                if cat.is_some_and(|c| c != xs[i]) {
                    continue;
                }
                let key = (s.id.to_string(), i);
                if !out.contains(&key) {
                    out.push(key);
                }
            }
        }
    }
    out
}

/// A table alone (table.layout_table).
pub fn layout_table(kit: &Kit, plan: &SemanticPlan, data: &PreparedData) -> Result<PlacedChart> {
    let spec = &plan.spec.raw;
    let mut findings: Vec<Finding> = kit::inset_info(spec, "table").into_iter().collect();
    let mut decisions: Vec<Value> = vec![];
    let w_fig = kit.c.width;
    let t = &spec["table"];
    let loc = fmt::locale(
        spec.pointer("/audience/locale")
            .and_then(Value::as_str)
            .unwrap_or("en-GB"),
    );
    let abbr = &spec["audience"]["abbreviations"];
    let xs = &data.categories;
    let n = xs.len();

    // ---- the book's rejected versions, only through an override ----
    let stripes = t["stripes"].as_bool() == Some(true);
    // The rejected versions are refused (or recorded as overridden) by the policy before layout
    // (chartgen-policy marks.rs); a layout that runs has been allowed to draw them.
    let bold: Vec<&str> = t["bold"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if spec.pointer("/chart/value_labels").and_then(Value::as_str) == Some("off") {
        let mut f = finding(
            "CG-3",
            "info",
            "a table's cells are its values: value_labels off does not apply",
            Some("chart.value_labels"),
        );
        f.level = "open".into();
        findings.push(f);
    }

    // ---- rows and columns ----
    let all = rows(plan, data)?;
    let mut body: Vec<&Row> = all.iter().filter(|s| !SUMMARY.contains(&s.role)).collect();
    body.sort_by_key(|s| s.role == "missing");
    let summ: Vec<&Row> = all.iter().filter(|s| SUMMARY.contains(&s.role)).collect();
    let rows: Vec<&Row> = body.iter().chain(summ.iter()).copied().collect();
    let in_summ = |s: &Row| SUMMARY.contains(&s.role);
    let total_cols: BTreeSet<usize> = (0..n)
        .filter(|&i| {
            spec.pointer("/data/x/roles")
                .and_then(|r| r.get(&xs[i]))
                .and_then(Value::as_str)
                == Some("total")
        })
        .collect();
    let cols: Vec<usize> = (0..n).collect();
    let measures: BTreeMap<&str, &Value> = spec["data"]["measures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(|id| (id, m)))
        .collect();
    let distinct: BTreeSet<Option<&str>> = all.iter().map(|s| s.measure).collect();
    let per_row = distinct.len() > 1
        && all
            .iter()
            .all(|s| s.measure.is_some_and(|m| measures.contains_key(m)));
    let comparable = !(spec.pointer("/facts/message").and_then(Value::as_str)
        == Some("not_comparable")
        || per_row);
    let graphics = t["graphics"].as_str().unwrap_or("none");
    if !matches!(graphics, "none" | "heat" | "bars" | "dots" | "trend") {
        return Err(err(format!("unsupported in-table graphics {graphics}")).at("table.graphics"));
    }
    let main_measure = Measure::of(&spec["data"]["measure"]);
    kit::decide(
        &mut decisions,
        "R10-13",
        format!(
            "a table alone, formatted in the book's order: the digits the purpose needs, two thin rules, space between rows and columns, the numbers {}, nothing bold but the title, the unit once in the title (08-12 DP-4, G7.37–G7.45)",
            if comparable {
                "right-aligned to be compared"
            } else {
                "left-aligned, each with its unit, not to be compared"
            }
        ),
        None,
    );
    let prec_d = main_measure.precision.filter(|_| comparable);
    kit::decide(
        &mut decisions,
        "R10-12",
        format!(
            "digits: {}, one number of decimals down each column (G7.37, G6.16)",
            match prec_d {
                Some(p) => format!("to the measure's precision, {}", p.normalize()),
                None if comparable => "each column at its data's own decimals".into(),
                None => "each value to its measure's precision, else its own decimals".into(),
            }
        ),
        None,
    );
    if !comparable {
        kit::decide(
            &mut decisions,
            "R10-20",
            format!(
                "the values are not comparable with one another ({}): left-aligned, each with its own unit (G7.41)",
                if per_row {
                    "each row its own measure"
                } else {
                    "facts.message not_comparable"
                }
            ),
            Some("locked"),
        );
    }

    // ---- the numbers of each column, formatted as a set ----
    let mut col_d = BTreeMap::new();
    for &i in &cols {
        let mut vals: Vec<Decimal> = rows
            .iter()
            .filter(|s| known(&s.values[i]))
            .map(|s| s.values[i].value.unwrap())
            .collect();
        vals.extend(
            rows.iter()
                .filter(|s| censored(&s.values[i]))
                .map(|s| s.values[i].limit.unwrap()),
        );
        col_d.insert(i, main_measure.decimals(&vals));
    }
    // (series id, i) -> (text, role)
    let mut cells: BTreeMap<(String, usize), (String, &'static str)> = BTreeMap::new();
    for s in &rows {
        let m = if comparable {
            Measure::of(&spec["data"]["measure"])
        } else {
            s.measure
                .and_then(|id| measures.get(id))
                .map(|v| Measure::of(v))
                .unwrap_or_else(|| Measure::of(&spec["data"]["measure"]))
        };
        for &i in &cols {
            let v = &s.values[i];
            let key = (s.id.to_string(), i);
            if censored(v) {
                let limit = v.limit.unwrap();
                let txt = if comparable {
                    fmt::censored_text(&v.status, limit, col_d[&i], loc, "")
                } else {
                    format!(
                        "{}{}",
                        if v.status == "below" { "<" } else { ">" },
                        m.unit_text(limit, m.decimals(&[limit]), loc, abbr)
                    )
                };
                cells.insert(key, (txt, "table-cell"));
                continue;
            }
            if !known(v) {
                cells.insert(key, (note_text(spec, status(v)), "missing-note"));
                continue;
            }
            let value = v.value.unwrap();
            let txt = if comparable {
                fmt::number(value, col_d[&i], loc, "", false)
            } else {
                m.unit_text(value, m.decimals(&[value]), loc, abbr)
            };
            cells.insert(key, (txt, "table-cell"));
        }
    }
    // ---- stub columns, the names column, the headers ----
    let mut stubs = vec![];
    for col in t["stub"].as_array().into_iter().flatten() {
        let kind = col["kind"].as_str().unwrap_or("text");
        let mut vals = vec![];
        for v in col["values"].as_array().into_iter().flatten() {
            let v = v.as_str().unwrap_or("");
            vals.push(if kind == "date" && !v.is_empty() {
                fmt::date_column(fmt::parse_date(v).map_err(|e| e.at("table.stub"))?)
            } else {
                v.to_string()
            });
        }
        stubs.push((
            Stub {
                head: crate::guides::content(&col["title"])?,
                vals,
            },
            kind,
        ));
    }
    if stubs.iter().any(|(_, k)| *k == "date") {
        kit::decide(
            &mut decisions,
            "R10-24",
            "dates written with day, month and year of fixed width (05-Jan-2021), so that their parts line up down the column (G6.21, D-32 columns (a))",
            Some("locked"),
        );
    }
    let names_head = crate::guides::content(&t["names_title"])?;
    let heads: Vec<TextContent> = cols
        .iter()
        .map(|&i| header(spec, &xs[i]))
        .collect::<Result<_>>()?;
    let trend = graphics == "trend";
    let trend_head = TextContent::plain(
        spec.pointer("/labels/notes/trend")
            .and_then(Value::as_str)
            .unwrap_or("Trend"),
    );
    let bold_head: &[&str] = if bold.contains(&"header") {
        &["text-strong"]
    } else {
        &[]
    };
    let bold_tot: &[&str] = if bold.contains(&"totals") {
        &["text-strong"]
    } else {
        &[]
    };
    let bold_col: &[&str] = if bold.contains(&"total_column") {
        &["text-strong"]
    } else {
        &[]
    };
    let mods_of = |s: &Row, i: usize| -> &[&str] {
        if in_summ(s) && !bold_tot.is_empty() {
            bold_tot
        } else if total_cols.contains(&i) {
            bold_col
        } else {
            &[]
        }
    };

    let block = kit::title_block(kit, spec, w_fig, &mut findings, &mut decisions)?;
    let tb = block.last_baseline.unwrap_or(0.);
    let cap_h = kit.plain_ext("row-header", &[], "H")?.ascent;
    let h0 = kit.plain_ext("table-cell", &[], "0")?.ascent;
    let gap = kit.metric("--table-col-gap")?;
    let tgap = kit.metric("--table-total-gap")?;
    let pad = kit.metric("--table-fill-pad")?;
    let lgap = kit.metric("--legend-gap")?;

    // ---- widths across ----
    let mut stub_w = vec![];
    for (st, _) in &stubs {
        let mut w: f64 = 0.;
        for v in &st.vals {
            w = w.max(kit.width("row-header", &[], &TextContent::plain(v))?);
        }
        if let Some(h) = &st.head {
            w = w.max(kit.width("column-header", bold_head, h)?);
        }
        stub_w.push(w);
    }
    let mut names_w: f64 = match &names_head {
        Some(h) => kit.width("column-header", bold_head, h)?,
        None => 0.,
    };
    for s in &rows {
        let mods: &[&str] = if in_summ(s) { bold_tot } else { &[] };
        names_w = names_w.max(kit.width("row-header", mods, &s.name)?);
    }
    let mut col_w = BTreeMap::new();
    for &i in &cols {
        let mut w = kit.width("column-header", bold_head, &heads[i])?;
        for s in &rows {
            let (txt, role) = &cells[&(s.id.to_string(), i)];
            w = w.max(kit.width(role, mods_of(s, i), &TextContent::plain(txt))?);
        }
        col_w.insert(i, w);
    }
    let strip = if graphics == "dots" {
        kit.metric("--cell-strip")?
    } else {
        0.
    };
    let g_cols: Vec<usize> = if matches!(graphics, "bars" | "dots") {
        match t["graphics_columns"].as_array() {
            None => cols.clone(),
            Some(list) => list
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|v| xs.iter().position(|x| x == v))
                .collect(),
        }
    } else {
        vec![]
    };
    if graphics == "dots" {
        for i in &g_cols {
            *col_w.get_mut(i).unwrap() += strip + lgap * 2.;
        }
    }
    let mut stub_x = vec![];
    let mut xpos = 0.;
    for w in &stub_w {
        stub_x.push(xpos);
        xpos += w + gap;
    }
    let names_x = xpos;
    xpos += names_w;
    let mut col_x = BTreeMap::new();
    let mut trend_x = None;
    let last_body_col = cols
        .iter()
        .copied()
        .filter(|i| !total_cols.contains(i))
        .max();
    for &i in &cols {
        xpos += if total_cols.contains(&i) { tgap } else { gap };
        col_x.insert(i, xpos);
        xpos += col_w[&i];
        if trend && Some(i) == last_body_col {
            xpos += gap;
            trend_x = Some(xpos);
            xpos += kit.metric("--trend-width")?;
        }
    }
    let mut table_w = xpos;
    if graphics == "heat" {
        table_w += gap / 2.;
    }
    if table_w > w_fig + 0.01
        && refusal(
            spec,
            &mut findings,
            "CG-1",
            &format!(
                "the table needs {:.0} pt across, wider than the chart's {:.0} pt: its columns would meet",
                table_w, w_fig
            ),
            Some("layout.width"),
            Some(&format!(
                "layout.width at least {}, or fewer columns",
                table_w as i64 + 1
            )),
        )
    {
        return Ok(kit::refused(findings, decisions));
    }
    let title_right = block.texts.iter().map(|b| b.bbox()[2]).fold(0., f64::max);
    let fig_w = w_fig.min(table_w.max(title_right));

    // ---- rows down ----
    let pitch = if graphics == "heat" {
        kit.metric("--cell-pitch")?
    } else {
        kit.metric("--table-alone-pitch")?
    };
    let headed = names_head.is_some()
        || stubs.iter().any(|(s, _)| s.head.is_some())
        || trend
        || heads.iter().any(|h| !h.readable().trim().is_empty());
    let title_gap = kit.metric("--title-gap")?;
    let drop = kit.metric("--table-rule-drop")?;
    let hb = if block.texts.is_empty() {
        cap_h
    } else {
        tb + title_gap
    };
    let rule_y = if headed {
        hb + drop
    } else {
        tb + title_gap - cap_h - kit.metric("--table-alone-pitch")? / 2. + drop
    };
    let centre: BTreeMap<&str, f64> = rows
        .iter()
        .enumerate()
        .map(|(j, s)| (s.id, rule_y + pitch * (j as f64 + 0.5)))
        .collect();
    let bottom = rule_y + pitch * rows.len() as f64;

    let mut texts: Vec<Placed> = vec![];
    let mut under: Vec<Primitive> = vec![];
    let mut marks: Vec<Primitive> = vec![];

    // headers, aligned like their columns (R10-17)
    for ((st, _), &x0) in stubs.iter().zip(&stub_x) {
        if let Some(h) = &st.head {
            texts.push(kit.text("column-header", bold_head, h, x0, hb, "start")?);
        }
    }
    if let Some(h) = &names_head {
        texts.push(kit.text("column-header", bold_head, h, names_x, hb, "start")?);
    }
    let dots_shift = |i: usize| {
        if graphics == "dots" && g_cols.contains(&i) {
            strip + 2. * lgap
        } else {
            0.
        }
    };
    for &i in &cols {
        if heads[i].readable().trim().is_empty() {
            continue;
        }
        let (xh, anchor) = if comparable {
            (col_x[&i] + col_w[&i], "end")
        } else {
            (col_x[&i] + dots_shift(i), "start")
        };
        texts.push(kit.text("column-header", bold_head, &heads[i], xh, hb, anchor)?);
    }
    if let Some(tx) = trend_x {
        texts.push(kit.text("column-header", bold_head, &trend_head, tx, hb, "start")?);
    }

    // the two rules (R10-13, G7.38)
    let rule_w = kit.metric("--table-rule-width").unwrap_or(0.6);
    let mut lines = vec![];
    if headed {
        lines.push(kit::line(
            0.,
            rule_y,
            table_w,
            rule_y,
            kit::stroke(&kit::token_paint(kit, "--table-rule"), rule_w),
            meta(&["table-rule"], &[]),
        ));
    }
    if !summ.is_empty() && !body.is_empty() {
        let yt = rule_y + pitch * body.len() as f64;
        lines.push(kit::line(
            0.,
            yt,
            table_w,
            yt,
            kit::stroke(&kit::token_paint(kit, "--table-rule-total"), rule_w),
            meta(&["table-rule-total"], &[]),
        ));
    }

    // stripes (only through an override)
    if stripes {
        for (j, s) in body.iter().enumerate() {
            if j % 2 == 1 {
                under.push(kit::rect(
                    0.,
                    centre[s.id] - pitch / 2.,
                    table_w,
                    pitch,
                    kit::fill(&kit::token_paint(kit, "--table-fill")),
                    meta(&["zebra"], &[("series", s.id.to_string())]),
                ));
            }
        }
        kit::decide(
            &mut decisions,
            "R10-16",
            "every other row shaded: the book's rejected version, through an override",
            Some("locked"),
        );
    }

    // stub cells, names, numbers
    let mut cell_text: BTreeMap<(String, usize), usize> = BTreeMap::new();
    for s in &rows {
        let cy = centre[s.id];
        for ((st, _), &x0) in stubs.iter().zip(&stub_x) {
            if let Some(v) = st.vals.get(s.index).filter(|v| !v.is_empty()) {
                texts.push(kit.text(
                    "row-header",
                    &[],
                    &TextContent::plain(v),
                    x0,
                    cy + cap_h / 2.,
                    "start",
                )?);
            }
        }
        if in_summ(s) {
            texts.push(kit.text(
                "row-header",
                bold_tot,
                &s.name,
                names_x + names_w,
                cy + cap_h / 2.,
                "end",
            )?);
        } else {
            texts.push(kit.text(
                "row-header",
                &[],
                &s.name,
                names_x,
                cy + cap_h / 2.,
                "start",
            )?);
        }
        for &i in &cols {
            let (txt, role) = &cells[&(s.id.to_string(), i)];
            let mods = mods_of(s, i);
            let yb = cy
                + if *role == "table-cell" {
                    h0
                } else {
                    cap_h * 0.8
                } / 2.;
            let content = TextContent::plain(txt);
            let tx = if comparable {
                kit.text(role, mods, &content, col_x[&i] + col_w[&i], yb, "end")?
            } else {
                kit.text(role, mods, &content, col_x[&i] + dots_shift(i), yb, "start")?
            };
            let mut tx = tx;
            tx.value_of = Some((s.id.to_string(), xs[i].clone()));
            cell_text.insert((s.id.to_string(), i), texts.len());
            texts.push(tx);
        }
    }

    if !summ.is_empty() {
        kit::decide(
            &mut decisions,
            "R10-22",
            "the totals last under the darker rule, named in the names' column, in regular weight (D-52 (ii)); the printed totals are the data's, never recomputed",
            Some("locked"),
        );
    }
    if !total_cols.is_empty() {
        kit::decide(
            &mut decisions,
            "R10-14",
            format!(
                "{}: a total column, set apart by a wider gutter ({tgap} pt against {gap} pt; G7.39s)",
                total_cols
                    .iter()
                    .map(|&i| xs[i].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Some("locked"),
        );
    }
    // ---- highlight: the few cells that matter, on a light fill (R10-23, G7.44) ----
    let cells_hl = highlighted(spec, &rows, &cols, xs, &mut findings);
    let inset = kit.metric("--table-fill-inset")?;
    for (sid, i) in &cells_hl {
        let cy = centre[sid.as_str()];
        let y0 = cy - pitch / 2. + inset;
        let bx = texts[cell_text[&(sid.clone(), *i)]].bbox();
        under.push(kit::rect(
            bx[0] - pad,
            y0,
            bx[2] - bx[0] + 2. * pad,
            pitch - 2. * inset,
            kit::fill(&kit::token_paint(kit, "--table-fill")),
            meta(
                &["table-fill"],
                &[("series", sid.clone()), ("x", xs[*i].clone())],
            ),
        ));
    }
    if !cells_hl.is_empty() {
        kit::decide(
            &mut decisions,
            "R10-23",
            format!(
                "{} cells that matter on a light grey fill, nothing else changed: {} (G7.44)",
                cells_hl.len(),
                cells_hl
                    .iter()
                    .map(|(sid, i)| format!(
                        "{} of {}",
                        xs[*i],
                        rows.iter()
                            .find(|r| r.id == sid)
                            .map_or("", |r| r.name.readable())
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Some("guided"),
        );
    }
    if !cells_hl.is_empty() && cells_hl.len() > 3.max(rows.len() * cols.len() / 5) {
        let mut f = finding(
            "R10-23",
            "warn",
            &format!(
                "{} highlighted cells: a fill singles out a few, not a large part of the table (G7.44)",
                cells_hl.len()
            ),
            Some("highlight"),
        );
        f.level = "guided".into();
        findings.push(f);
    }

    // ---- in-table graphics (R10-9, G7.47) ----
    let mut cell_values = Value::Null;
    if graphics != "none" {
        let how = match graphics {
            "heat" => {
                "the cells shaded by value, the body, the Total column and the totals row each on its own ramp from its lowest to its highest value, the grand total unshaded (s. 0309); pale, so that the numbers stay legible (G6.5)"
            }
            "bars" => {
                "a bar under each number, from the cell's left edge, on one scale from zero for each column (s. 0309)"
            }
            "dots" => "a dot beside each number, on one scale for each column",
            _ => {
                "a Trend column: each row's values as a sparkline scaled to the row's own extent, before the Total (s. 0309)"
            }
        };
        kit::decide(
            &mut decisions,
            "R10-9",
            format!(
                "{how}; built into the table for an audience that will not take a graph (G1.16, G7.47)"
            ),
            Some("guided"),
        );
        if !comparable && graphics != "trend" {
            let mut f = finding(
                "CG-3",
                "info",
                "graphics in a table of values that are not comparable would compare them: none drawn",
                Some("table.graphics"),
            );
            f.level = "open".into();
            findings.push(f);
        } else {
            match graphics {
                "bars" | "dots" => in_cell_marks(
                    kit,
                    spec,
                    graphics,
                    &body,
                    &g_cols,
                    &col_x,
                    &col_w,
                    &centre,
                    strip,
                    xs,
                    &mut marks,
                    &mut findings,
                )?,
                "trend" => {
                    let tw = kit.metric("--trend-width")?;
                    let th = pitch * 0.55;
                    let body_cols: Vec<usize> = cols
                        .iter()
                        .copied()
                        .filter(|i| !total_cols.contains(i))
                        .collect();
                    let tx = trend_x.unwrap();
                    for s in body.iter().chain(summ.iter()) {
                        let pts: Vec<Decimal> = body_cols
                            .iter()
                            .filter(|&&i| known(&s.values[i]))
                            .map(|&i| s.values[i].value.unwrap())
                            .collect();
                        if pts.len() < 2 {
                            continue;
                        }
                        let lo = *pts.iter().min().unwrap();
                        let hi = *pts.iter().max().unwrap();
                        let nb = body_cols.len();
                        let cy = centre[s.id];
                        let y_of = |v: Decimal| -> f64 {
                            let f = if hi == lo {
                                0.5
                            } else {
                                ((v - lo) / (hi - lo)).to_f64().unwrap_or(0.5)
                            };
                            cy + th / 2. - f * th
                        };
                        let mut run: Vec<Point> = vec![];
                        let ink = kit::token_paint(kit, "--single");
                        let flush = |run: &mut Vec<Point>, marks: &mut Vec<Primitive>| {
                            if run.len() > 1 {
                                marks.push(Primitive::Path {
                                    points: std::mem::take(run),
                                    closed: false,
                                    paint: kit::stroke(&ink, 1.),
                                    meta: meta(&["sparkline"], &[("series", s.id.to_string())]),
                                });
                            }
                            run.clear();
                        };
                        for (k, &i) in body_cols.iter().enumerate() {
                            let v = &s.values[i];
                            if known(v) {
                                run.push(Point {
                                    x: tx + tw * k as f64 / (nb.max(2) - 1) as f64,
                                    y: y_of(v.value.unwrap()),
                                });
                            } else {
                                flush(&mut run, &mut marks);
                            }
                        }
                        flush(&mut run, &mut marks);
                    }
                }
                "heat" => {
                    // the cells' strength (chart.cells, guided; ramps.cell_strength for a table)
                    let why = "a table whose colour only hints at sizes: the steps paled, the numbers in the text grey (G7.47)";
                    match spec
                        .pointer("/chart/cells")
                        .and_then(Value::as_str)
                        .unwrap_or("auto")
                    {
                        "auto" => kit::decide(
                            &mut decisions,
                            "R10-9",
                            format!("the cells muted: {why} (owner, 1 Oct 2026)"),
                            Some("guided"),
                        ),
                        asked => {
                            kit::guided(
                                spec,
                                &mut findings,
                                "R10-9",
                                "chart.cells",
                                asked,
                                "muted",
                                why,
                            );
                            if asked != "muted" {
                                return Err(err(
                                    "strong cells in a shaded table need the palette's ramp and the style's value treatment, which are not ported",
                                )
                                .at("chart.cells"));
                            }
                        }
                    }
                    // the ramp's decision (ramps.choose) is the colour plan's, made here
                    kit::colour_decisions_here(&mut decisions);
                    cell_values = heat_cells(
                        kit,
                        plan,
                        &rows,
                        &body,
                        &summ,
                        &cols,
                        &total_cols,
                        &col_x,
                        &col_w,
                        &centre,
                        pitch,
                        xs,
                        &mut under,
                    )?;
                }
                _ => unreachable!(),
            }
        }
    }

    // ---- the unit once, in the title (R10-25, G7.45) ----
    unit_in_title(spec, comparable, &mut findings, &mut decisions);

    // ---- assembly ----
    let mut table_g = lines;
    table_g.extend(texts.iter().map(Placed::primitive));
    let mut root = vec!["chart", "table", "categorical"];
    if graphics != "none" {
        root.push("graphics");
    }
    let primitives = vec![kit::group(
        &root,
        Some("chart"),
        &[],
        vec![
            kit::group(
                &["header"],
                None,
                &[],
                block.texts.iter().map(Placed::primitive).collect(),
            ),
            kit::group(
                &["panel"],
                Some("main"),
                &[],
                vec![
                    kit::group(&["underlay"], None, &[], under),
                    kit::group(&["marks"], None, &[], marks),
                    kit::group(&["table"], None, &[], table_g),
                ],
            ),
            kit::group(&["footer"], None, &[], vec![]),
        ],
    )];
    let height = texts
        .iter()
        .chain(block.texts.iter())
        .map(|t| t.bbox()[3])
        .fold(bottom, f64::max)
        + 1.;
    let mut summary = BTreeMap::new();
    summary.insert("orientation".into(), json!("vertical"));
    summary.insert(
        "plot".into(),
        json!([0.0, r2(rule_y), r2(table_w), r2(bottom)]),
    );
    summary.insert(
        "table".into(),
        json!({
            "rows": rows.iter().map(|s| s.id).collect::<Vec<_>>(),
            "columns": cols.iter().map(|&i| &xs[i]).collect::<Vec<_>>(),
            "comparable": comparable,
            "pitch": r2(pitch),
            "width": r2(table_w),
            "graphics": graphics,
            "highlighted": cells_hl.iter().map(|(s, i)| json!([s, xs[*i]])).collect::<Vec<_>>(),
            "values": cell_values,
        }),
    );
    summary.insert("width".into(), r2(fig_w));
    summary.insert("height".into(), r2(height));
    Ok(PlacedChart {
        decisions,
        width: fig_w,
        height,
        primitives,
        findings,
        summary,
    })
}

#[allow(clippy::too_many_arguments)]
fn in_cell_marks(
    kit: &Kit,
    spec: &Value,
    kind: &str,
    body: &[&Row],
    g_cols: &[usize],
    col_x: &BTreeMap<usize, f64>,
    col_w: &BTreeMap<usize, f64>,
    centre: &BTreeMap<&str, f64>,
    strip: f64,
    xs: &[String],
    marks: &mut Vec<Primitive>,
    findings: &mut Vec<Finding>,
) -> Result<()> {
    let pad = kit.metric("--table-fill-pad")?;
    for &i in g_cols {
        let known_v: Vec<Decimal> = body
            .iter()
            .filter(|s| known(&s.values[i]))
            .map(|s| s.values[i].value.unwrap())
            .collect();
        if known_v.is_empty() {
            continue;
        }
        let lo_k = *known_v.iter().min().unwrap();
        let hi = *known_v.iter().max().unwrap();
        if kind == "bars" && lo_k < Decimal::ZERO {
            let mut f = finding(
                "CG-3",
                "info",
                &format!(
                    "bars in the column {}, whose values go below zero, are a later stage",
                    xs[i]
                ),
                Some("table.graphics"),
            );
            f.level = "open".into();
            findings.push(f);
            continue;
        }
        let grey = |s: &Row| -> Vec<&'static str> {
            if s.role == "missing" {
                vec!["missing"]
            } else {
                vec![]
            }
        };
        if kind == "bars" {
            let x0 = col_x[&i] - pad;
            let span = col_w[&i] + 2. * pad;
            let hgt = kit.metric("--cell-bar-height")?;
            for s in body {
                let v = &s.values[i];
                if !known(v) || hi <= Decimal::ZERO {
                    continue;
                }
                let l = (v.value.unwrap() / hi).to_f64().unwrap_or(0.) * span;
                let mut classes = vec!["cell-bar"];
                classes.extend(grey(s));
                marks.push(kit::rect(
                    x0,
                    centre[s.id] - hgt / 2.,
                    l,
                    hgt,
                    kit::fill(&kit::token_paint(kit, "--cell-bar")),
                    meta(
                        &classes,
                        &[
                            ("series", s.id.to_string()),
                            ("x", xs[i].clone()),
                            ("value", dec_text(v.value.unwrap())),
                        ],
                    ),
                ));
            }
        } else {
            let zero = spec
                .pointer("/data/measure/zero")
                .and_then(Value::as_str)
                .unwrap_or("meaningful")
                == "meaningful"
                && lo_k >= Decimal::ZERO;
            let lo = if zero { Decimal::ZERO } else { lo_k };
            let size = kit.metric("--cell-dot")?;
            let x0 = col_x[&i] + size / 2.;
            let span = strip - size;
            for s in body {
                let v = &s.values[i];
                if !known(v) {
                    continue;
                }
                let f = if hi == lo {
                    0.5
                } else {
                    ((v.value.unwrap() - lo) / (hi - lo))
                        .to_f64()
                        .unwrap_or(0.5)
                };
                let mut classes = vec!["cell-dot"];
                classes.extend(grey(s));
                marks.push(kit::dot(
                    x0 + f * span,
                    centre[s.id],
                    size,
                    kit::fill(&kit::token_paint(kit, "--single")),
                    meta(
                        &classes,
                        &[
                            ("series", s.id.to_string()),
                            ("x", xs[i].clone()),
                            ("value", dec_text(v.value.unwrap())),
                        ],
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// A shaded table's cells (table._graphics heat): the body, the Total column and the totals
/// row each over its own extent, the grand total unshaded, the body's cells touching.
///
/// The ramp's classes are the colour plan's (chartgen-colours ramps::table_heat), each cell
/// `labelled` (muted, paled); a plan compiled without colours falls back to a pale interpolation.
#[allow(clippy::too_many_arguments)]
fn heat_cells(
    kit: &Kit,
    plan: &SemanticPlan,
    rows: &[&Row],
    body: &[&Row],
    summ: &[&Row],
    cols: &[usize],
    total_cols: &BTreeSet<usize>,
    col_x: &BTreeMap<usize, f64>,
    col_w: &BTreeMap<usize, f64>,
    centre: &BTreeMap<&str, f64>,
    pitch: f64,
    xs: &[String],
    under: &mut Vec<Primitive>,
) -> Result<Value> {
    let gap = kit.metric("--table-col-gap")?;
    let seam = crate::heatmap::SEAM;
    let groups: [Vec<(&Row, usize)>; 3] = [
        body.iter()
            .flat_map(|s| {
                cols.iter()
                    .filter(|i| !total_cols.contains(i))
                    .map(move |&i| (*s, i))
            })
            .collect(),
        body.iter()
            .flat_map(|s| {
                cols.iter()
                    .filter(|i| total_cols.contains(i))
                    .map(move |&i| (*s, i))
            })
            .collect(),
        summ.iter()
            .flat_map(|s| {
                cols.iter()
                    .filter(|i| !total_cols.contains(i))
                    .map(move |&i| (*s, i))
            })
            .collect(),
    ];
    let order: Vec<&str> = rows.iter().map(|s| s.id).collect();
    for members in &groups {
        let vals: Vec<f64> = members
            .iter()
            .filter(|(s, i)| known(&s.values[*i]))
            .filter_map(|(s, i)| s.values[*i].value.and_then(|v| v.to_f64()))
            .collect();
        if vals.is_empty() {
            continue;
        }
        let lo = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let ids: BTreeSet<(&str, usize)> = members.iter().map(|(s, i)| (s.id, *i)).collect();
        for (s, i) in members {
            let v = &s.values[*i];
            let x0 = col_x[i] - gap / 2.;
            let x1 = col_x[i] + col_w[i] + gap / 2.;
            let j = order.iter().position(|id| *id == s.id).unwrap();
            let right = if ids.contains(&(s.id, i + 1)) {
                seam
            } else {
                0.
            };
            let down = if j + 1 < order.len() && ids.contains(&(order[j + 1], *i)) {
                seam
            } else {
                0.
            };
            let planned = plan
                .colors
                .ramp
                .as_ref()
                .and_then(|r| r.cells.get(s.index)?.get(*i))
                .filter(|c| !c.is_empty());
            let (classes, colour, value) = if let Some(c) = planned {
                let mut cls: Vec<String> = vec!["cell".into()];
                cls.extend(c.iter().cloned());
                if !cls.iter().any(|x| x == "missing") {
                    cls.push("labelled".into());
                }
                let paint = if cls.iter().any(|x| x == "missing") {
                    kit::token_paint(kit, "--missing")
                } else {
                    crate::heatmap::cell_paint(kit, &cls[1..], true)
                };
                let value = if known(v) {
                    dec_text(v.value.unwrap())
                } else if censored(v) {
                    format!("{} {}", v.status, dec_text(v.limit.unwrap()))
                } else {
                    status(v).to_string()
                };
                (cls, paint, value)
            } else if known(v) {
                let f = if hi > lo {
                    (v.value.unwrap().to_f64().unwrap_or(lo) - lo) / (hi - lo)
                } else {
                    0.5
                };
                (
                    vec!["cell".to_string()],
                    crate::heatmap::pale_step(kit, f)?,
                    dec_text(v.value.unwrap()),
                )
            } else if censored(v) {
                (
                    vec!["cell".to_string()],
                    crate::heatmap::pale_step(kit, if v.status == "below" { 0. } else { 1. })?,
                    format!("{} {}", v.status, dec_text(v.limit.unwrap())),
                )
            } else {
                (
                    vec!["cell".to_string(), "missing".to_string()],
                    kit::token_paint(kit, "--missing"),
                    status(v).to_string(),
                )
            };
            under.push(kit::rect(
                x0,
                centre[s.id] - pitch / 2.,
                x1 - x0 + right,
                pitch + down,
                kit::fill(&colour),
                meta(
                    &classes.iter().map(String::as_str).collect::<Vec<_>>(),
                    &[
                        ("series", s.id.to_string()),
                        ("x", xs[*i].clone()),
                        ("value", value),
                    ],
                ),
            ));
        }
    }
    Ok(Value::Null)
}

/// R10-25 (G7.45): the unit stated once, in the title; a warning when the title does not.
fn unit_in_title(
    spec: &Value,
    comparable: bool,
    findings: &mut Vec<Finding>,
    decisions: &mut Vec<Value>,
) {
    if !comparable {
        return;
    }
    let title = &spec["title"];
    let text_of = |v: &Value| -> Option<String> {
        if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else {
            v.get("typst")
                .or_else(|| v.get("plain"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        }
    };
    let Some(t) = text_of(&title["text"]) else {
        return;
    };
    let mut words = t;
    if let Some(d) = text_of(&title["descriptive"]) {
        words = format!("{words} {d}");
    }
    let words = words.to_lowercase();
    let u = &spec["data"]["measure"]["unit"];
    let mut wanted: Vec<String> = [
        u["prefix"].as_str().unwrap_or("").trim().to_string(),
        u["suffix"].as_str().unwrap_or("").trim().to_string(),
        u["noun"]
            .as_str()
            .unwrap_or("")
            .split(' ')
            .next()
            .unwrap_or("")
            .to_string(),
    ]
    .into_iter()
    .filter(|w| !w.is_empty())
    .collect();
    if spec.pointer("/data/measure/kind").and_then(Value::as_str) == Some("percent")
        && u["suffix"].as_str().unwrap_or("").trim().is_empty()
    {
        wanted.push("%".into());
    }
    if !wanted.is_empty() && !wanted.iter().any(|w| words.contains(&w.to_lowercase())) {
        findings.push(finding(
            "R10-25",
            "warn",
            &format!(
                "the unit ({}) is stated in no cell, and the title does not state it: say it once in the title, “Funds Raised (US$) by Event by Region” (G7.45)",
                wanted.join(", ")
            ),
            Some("title.text"),
        ));
    } else if !wanted.is_empty() {
        kit::decide(
            decisions,
            "R10-25",
            "the unit once, in the title; the cells bare numbers (G7.45)",
            Some("locked"),
        );
    }
}

// ---- action dots (R12-4, G7.58) ----

pub fn layout_action_dots(
    kit: &Kit,
    plan: &SemanticPlan,
    data: &PreparedData,
) -> Result<PlacedChart> {
    let spec = &plan.spec.raw;
    let mut findings: Vec<Finding> = kit::inset_info(spec, "action_dots").into_iter().collect();
    let mut decisions: Vec<Value> = vec![];
    let w_fig = kit.c.width;
    let em = kit.metric("--label-size")?;
    let xs = &data.categories;
    if data.series.len() > 1 {
        let mut f = finding(
            "CG-3",
            "info",
            "action dots of several series are a later stage: the first drawn",
            Some("data.series"),
        );
        f.level = "open".into();
        findings.push(f);
    }
    let s = &data.series[0];
    let idx: Vec<usize> = (0..xs.len()).collect();
    let mut kn: Vec<usize> = idx
        .iter()
        .copied()
        .filter(|&i| known(&s.values[i]))
        .collect();
    let unknown: Vec<usize> = idx
        .iter()
        .copied()
        .filter(|&i| !known(&s.values[i]))
        .collect();
    let nominal = spec.pointer("/data/x/kind").and_then(Value::as_str) == Some("nominal");
    let (sort, _) = kit::choice_sort(spec, plan.chart_type);
    let sort = sort.as_str();
    if matches!(sort, "desc" | "asc" | "auto") && nominal {
        kn.sort_by(|&a, &b| {
            let (va, vb) = (s.values[a].value.unwrap(), s.values[b].value.unwrap());
            if sort == "asc" {
                va.cmp(&vb)
            } else {
                vb.cmp(&va)
            }
        });
        kit::decide(
            &mut decisions,
            "R12-4",
            "rows sorted by value, largest first, those not known last (G7.58)",
            Some("locked"),
        );
    }
    let order: Vec<usize> = if nominal {
        kn.iter().chain(unknown.iter()).copied().collect()
    } else {
        idx.clone()
    };
    let formatted = fmt::format_values(
        &plan.spec,
        &idx.iter()
            .map(|&i| {
                if known(&s.values[i]) {
                    s.values[i].value
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
    )?;
    let names: Vec<TextContent> = idx
        .iter()
        .map(|&i| crate::guides::category_content(plan, data, i))
        .collect::<Result<_>>()?;
    let vals: Vec<(String, &str, &[&str])> = idx
        .iter()
        .map(|&i| {
            if known(&s.values[i]) {
                (
                    formatted[i].clone().unwrap_or_default(),
                    "value-label",
                    &["text-strong"][..],
                )
            } else {
                (
                    note_text(spec, status(&s.values[i])),
                    "missing-note",
                    &[][..],
                )
            }
        })
        .collect();
    let block = kit::title_block(kit, spec, w_fig, &mut findings, &mut decisions)?;
    let tb = block.last_baseline.unwrap_or(0.);
    let cap_h = kit.plain_ext("category-label", &[], "H")?.ascent;
    let h0 = kit.plain_ext("value-label", &["text-strong"], "0")?.ascent;
    let pitch = kit.metric("--action-pitch")?;
    let g = kit.metric("--action-gap")?;
    let mut name_w: f64 = 0.;
    let mut val_w: f64 = 0.;
    for &i in &idx {
        name_w = name_w.max(kit.width("category-label", &[], &names[i])?);
        let (txt, role, mods) = &vals[i];
        val_w = val_w.max(kit.width(role, mods, &TextContent::plain(txt))?);
    }
    let bar_x = name_w + g;
    let mut bar_len = kit.metric("--action-bar-length")?;
    let dot = kit.metric("--action-dot")?;
    let mut val_r = bar_x + bar_len + g + val_w;
    let mut dot_x = val_r + g + dot / 2.;
    let mut width = dot_x + dot / 2.;
    if width > w_fig + 0.01 {
        bar_len = (3. * em).max(bar_len - (width - w_fig));
        val_r = bar_x + bar_len + g + val_w;
        dot_x = val_r + g + dot / 2.;
        width = dot_x + dot / 2.;
    }
    let top = if block.texts.is_empty() {
        0.
    } else {
        tb + kit.metric("--title-gap")? - pitch / 2.
    };
    let hi = kn
        .iter()
        .map(|&i| s.values[i].value.unwrap())
        .fold(Decimal::ZERO, Decimal::max);
    if kn
        .iter()
        .any(|&i| s.values[i].value.unwrap() < Decimal::ZERO)
        && refusal(
            spec,
            &mut findings,
            "R2-24",
            "action dots' bars run from zero; a value below zero has no bar to draw here",
            Some("data.series"),
            Some("values of zero or more"),
        )
    {
        return Ok(kit::refused(findings, decisions));
    }
    let thick = kit.metric("--action-bar")? * pitch;
    let statuses = spec.pointer("/data/actions").and_then(Value::as_object);
    let mut texts = vec![];
    let mut bars = vec![];
    let mut dots = vec![];
    let mut shown = serde_json::Map::new();
    for (j, &i) in order.iter().enumerate() {
        let cy = top + pitch * (j as f64 + 0.5);
        texts.push(kit.text(
            "category-label",
            &[],
            &names[i],
            name_w,
            cy + cap_h / 2.,
            "end",
        )?);
        let v = &s.values[i];
        if known(v) && hi > Decimal::ZERO {
            let l = (v.value.unwrap() / hi).to_f64().unwrap_or(0.) * bar_len;
            bars.push(kit::rect(
                bar_x,
                cy - thick / 2.,
                l,
                thick,
                kit::fill(&kit::token_paint(kit, "--action-bar-fill")),
                meta(
                    &["bar", "action-bar"],
                    &[
                        ("series", s.id.clone()),
                        ("x", xs[i].clone()),
                        ("value", dec_text(v.value.unwrap())),
                    ],
                ),
            ));
        }
        let (txt, role, mods) = &vals[i];
        let yb = cy
            + if *role == "value-label" {
                h0
            } else {
                cap_h * 0.8
            } / 2.;
        texts.push(kit.text(role, mods, &TextContent::plain(txt), val_r, yb, "end")?);
        let level = statuses.and_then(|m| m.get(&xs[i])).and_then(Value::as_str);
        let classes: Option<(&str, &str, &str)> = match level {
            None | Some("none") => None,
            Some("good_strong") => Some(("good", "div-p4", "--good-4")),
            Some("good") => Some(("good", "div-p2", "--good-2")),
            Some("bad") => Some(("bad", "div-m2", "--bad-2")),
            Some("bad_strong") => Some(("bad", "div-m4", "--bad-4")),
            Some(other) => {
                return Err(err(format!("unsupported action status {other}")).at("data.actions"));
            }
        };
        if let Some((side, step, token)) = classes {
            dots.push(kit::dot(
                dot_x,
                cy,
                dot,
                kit::fill(&kit::token_paint(kit, token)),
                meta(
                    &["action-dot", side, step],
                    &[
                        ("series", s.id.clone()),
                        ("x", xs[i].clone()),
                        ("status", level.unwrap().to_string()),
                    ],
                ),
            ));
            shown.insert(xs[i].clone(), json!(level.unwrap()));
        }
    }
    let bottom = top + pitch * order.len() as f64;
    let words = |l: &str| match l {
        "good_strong" => "strong green",
        "good" => "light green",
        "bad" => "light red",
        _ => "strong red",
    };
    kit::decide(
        &mut decisions,
        "R12-4",
        format!(
            "action dots: each name right-aligned, a thin light-grey bar in proportion to its value from zero, the value, and a dot only where action is warranted: {}; the statuses supplied with the data (G7.58, D-1)",
            if shown.is_empty() {
                "none".to_string()
            } else {
                shown
                    .iter()
                    .map(|(k, v)| format!("{k} {}", words(v.as_str().unwrap_or(""))))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ),
        Some("locked"),
    );
    if statuses.is_none_or(|m| m.is_empty()) {
        findings.push(finding(
            "R12-4",
            "warn",
            "no statuses given (data.actions): no dot says where action is needed, which is the point of action dots (G7.58)",
            Some("data.actions"),
        ));
    }
    let height = texts
        .iter()
        .chain(block.texts.iter())
        .map(|t| t.bbox()[3])
        .fold(bottom, f64::max)
        + 1.;
    let title_right = block.texts.iter().map(|b| b.bbox()[2]).fold(0., f64::max);
    let fig_w = if width < w_fig {
        width.max(title_right)
    } else {
        w_fig
    };
    let names: Vec<Primitive> = texts
        .iter()
        .filter(|t| t.role == "category-label")
        .map(Placed::primitive)
        .collect();
    let values: Vec<Primitive> = texts
        .iter()
        .filter(|t| t.role != "category-label")
        .map(Placed::primitive)
        .collect();
    let primitives = vec![kit::group(
        &["chart", "action-dots", "horizontal", "categorical"],
        Some("chart"),
        &[],
        vec![
            kit::group(
                &["header"],
                None,
                &[],
                block.texts.iter().map(Placed::primitive).collect(),
            ),
            kit::group(
                &["panel"],
                Some("main"),
                &[],
                vec![
                    kit::group(
                        &["marks"],
                        None,
                        &[],
                        vec![
                            kit::group(
                                &["series", "single"],
                                None,
                                &[("series", s.id.clone())],
                                bars,
                            ),
                            kit::group(&["points"], None, &[], dots),
                        ],
                    ),
                    kit::group(&["scale", "category"], None, &[], names),
                    kit::group(&["annotations"], None, &[], values),
                ],
            ),
            kit::group(&["footer"], None, &[], vec![]),
        ],
    )];
    let mut summary = BTreeMap::new();
    summary.insert("orientation".into(), json!("horizontal"));
    summary.insert(
        "plot".into(),
        json!([r2(bar_x), r2(top), r2(bar_x + bar_len), r2(bottom)]),
    );
    summary.insert(
        "actions".into(),
        json!({"order": order.iter().map(|&i| &xs[i]).collect::<Vec<_>>(), "dots": shown}),
    );
    summary.insert("width".into(), r2(fig_w));
    summary.insert("height".into(), r2(height));
    Ok(PlacedChart {
        decisions,
        width: fig_w,
        height,
        primitives,
        findings,
        summary,
    })
}
