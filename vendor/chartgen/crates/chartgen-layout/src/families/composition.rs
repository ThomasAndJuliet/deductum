//! A table with the graph (R10-7, R2-31, G1.14, G7.46, D-52): when exact values are needed
//! (`facts.exact_values_needed`) and the bars are not labelled, a table aligned with the bars,
//! drawn by the bar layout: under vertical bars one column under each category or cluster, the
//! series as rows; beside horizontal bars one column per series, each row level with its
//! category. A port of the table pieces of the original `layout/bars.py` (`table_for`, `cell`,
//! `head_width`, the table under the graph, the table beside the bars, `_row_key`, and the
//! legend decision of `table.legend`).
//!
//! The pieces are independent of the bar layout's own geometry: the bar layout asks for the
//! table's plan, reserves its left column with `head_width` (vertical) or its right room with
//! `beside_widths` (horizontal), and places it from its plot geometry with `under` or `beside`.
//! The current cartesian bar layout does not call them yet (it is not a port of bars.py); the
//! cartesian stream wires them in where it computes the left column and the last baseline.
#![allow(dead_code)]
use super::kit::{self, Kit, Placed, finding, meta};
use chartgen_format as fmt;
use chartgen_model::*;
use serde_json::Value;

/// One row of the table: a bar series (or a reference series, keyed in its bars' hue).
#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub name: TextContent,
    /// The colour class its key takes (`series-k`, `single`).
    pub colour: String,
    pub reference: bool,
    /// (text, role) of each category, in the bars' category order.
    pub cells: Vec<(String, &'static str)>,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub rows: Vec<Row>,
    /// How the rows name the colours (table.row_key, resolved).
    pub row_key: String,
    /// Whether table.row_key was left to the owner's default.
    pub row_key_auto: bool,
    pub legend: String,
}

/// A series as the bar layout draws it: its prepared index, colour class, and whether it is a
/// reference (flat markers across the bars).
pub struct BarSeries {
    pub index: usize,
    pub colour: String,
    pub reference: bool,
}

/// The table the bars take, or None (R10-7, 02 D8): wanted when exact values are needed, left
/// out when the values are labelled on the bars (a decision the caller reports). `bars` are the
/// bar series then the reference series, in the order of the bars in each cluster; `cats` the
/// category indices in the bars' order.
pub fn plan(
    spec: &Value,
    data: &PreparedData,
    bars: &[BarSeries],
    cats: &[usize],
    labelled: bool,
) -> Result<Option<Plan>> {
    if spec
        .pointer("/facts/exact_values_needed")
        .and_then(Value::as_bool)
        != Some(true)
        || labelled
    {
        return Ok(None);
    }
    let unit = fmt::Unit::from_value(&spec["data"]["measure"]["unit"]);
    let magnitude = spec["data"]["measure"]["unit"]["magnitude"]
        .as_i64()
        .unwrap_or(0) as i32;
    let known: Vec<Decimal> = bars
        .iter()
        .flat_map(|b| data.series[b.index].values.iter())
        .filter(|d| crate::table::known(d))
        .map(|d| d.value.unwrap())
        .collect();
    let d = fmt::set_decimals(&known, None, &unit, magnitude);
    let loc = fmt::locale(
        spec.pointer("/audience/locale")
            .and_then(Value::as_str)
            .unwrap_or("en-GB"),
    );
    let first_colour = bars.iter().find(|b| !b.reference).map(|b| b.colour.clone());
    let mut rows = vec![];
    for b in bars {
        let s = &data.series[b.index];
        let raw = &spec["data"]["series"][b.index];
        let name = kit::spec_text(&raw["name"])?.unwrap_or_else(|| TextContent::plain(&s.id));
        let cells = cats
            .iter()
            .map(|&c| {
                let v = &s.values[c];
                if crate::table::known(v) {
                    (
                        fmt::number(v.value.unwrap(), d, loc, "", false),
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
            })
            .collect();
        rows.push(Row {
            id: s.id.clone(),
            name,
            colour: if b.reference {
                first_colour.clone().unwrap_or_else(|| b.colour.clone())
            } else {
                b.colour.clone()
            },
            reference: b.reference,
            cells,
        });
    }
    let opt = spec
        .pointer("/table/row_key")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let row_key = if opt == "auto" {
        if rows.len() > 1 {
            "swatch_after"
        } else {
            "none"
        }
        .to_string()
    } else {
        opt.to_string()
    };
    Ok(Some(Plan {
        rows,
        row_key,
        row_key_auto: opt == "auto",
        legend: spec
            .pointer("/table/legend")
            .and_then(Value::as_str)
            .unwrap_or("auto")
            .into(),
    }))
}

fn row_mods(p: &Plan, r: &Row) -> Vec<String> {
    if p.row_key == "text" {
        vec![format!("text-{}", r.colour)]
    } else {
        vec![]
    }
}

/// The widest row name with the room its key takes: the left column a table under vertical bars
/// needs (bars.head_width).
pub fn head_width(kit: &Kit, p: &Plan) -> Result<f64> {
    let mut w: f64 = 0.;
    for r in &p.rows {
        let mods = row_mods(p, r);
        let m: Vec<&str> = mods.iter().map(String::as_str).collect();
        w = w.max(kit.width("row-header", &m, &r.name)?);
    }
    if p.rows.is_empty() || matches!(p.row_key.as_str(), "none" | "text" | "swatch_after") {
        return Ok(w);
    }
    Ok(match p.row_key.as_str() {
        "swatch" | "swatch_beside" => w + kit.metric("--legend-key")? + kit.metric("--key-gap")?,
        "pill" => w + 2. * kit.metric("--key-pill-pad-x")?,
        _ => w + kit.metric("--key-halo-width")?,
    })
}

/// The table under vertical bars: its texts, its row keys, its findings, and whether its rows
/// are keyed (the chart's key, so the legend may go).
pub struct Under {
    pub texts: Vec<Placed>,
    pub keys: Vec<Primitive>,
    pub findings: Vec<Finding>,
    pub keyed: bool,
}

/// The table under the graph, one column under each category (`plot_l`, `band`), the rows
/// named right-aligned at `xr` (the left column's right edge), the first row `--table-gap`
/// under the last baseline of the category labels (`last_b`). None when its cells would meet
/// under the categories (a warning, R10-7: the table beside the graph, transposed, is a later
/// stage in the original too).
pub fn under(kit: &Kit, p: &Plan, plot_l: f64, xr: f64, band: f64, last_b: f64) -> Result<Under> {
    let em = kit.metric("--label-size")?;
    let pad = kit.metric("--text-pad")?;
    let mut out = Under {
        texts: vec![],
        keys: vec![],
        findings: vec![],
        keyed: false,
    };
    let mut ok = true;
    for r in &p.rows {
        let ws: Vec<f64> = r
            .cells
            .iter()
            .map(|(t, role)| kit.width(role, &[], &TextContent::plain(t)))
            .collect::<Result<_>>()?;
        if ws
            .windows(2)
            .any(|w| band - (w[0] + w[1]) / 2. < pad + 0.5 * em)
        {
            ok = false;
        }
    }
    if !ok {
        let mut f = finding(
            "R10-7",
            "warn",
            "the table's cells would meet under the categories; a table beside the graph, transposed, is a later stage (D-52 (iv)): no table drawn",
            Some("facts.exact_values_needed"),
        );
        f.level = "guided".into();
        out.findings.push(f);
        return Ok(out);
    }
    let yb = last_b + kit.metric("--table-gap")?;
    let cap = kit.plain_ext("row-header", &[], "H")?.ascent;
    let pitch = kit.metric("--table-row-pitch")?;
    let mut first_values = f64::INFINITY;
    for (j, r) in p.rows.iter().enumerate() {
        let y = yb + j as f64 * pitch;
        let mods = row_mods(p, r);
        let m: Vec<&str> = mods.iter().map(String::as_str).collect();
        let head = kit.text("row-header", &m, &r.name, xr, y, "end")?;
        out.keys
            .extend(row_key(kit, &p.row_key, &head, r, cap, plot_l)?);
        out.texts.push(head);
        for (i, (t, role)) in r.cells.iter().enumerate() {
            let cx = plot_l + (i as f64 + 0.5) * band;
            let tx = kit.text(role, &[], &TextContent::plain(t), cx, y, "middle")?;
            if i == 0 {
                first_values = first_values.min(tx.bbox()[0]);
            }
            out.texts.push(tx);
        }
    }
    if p.row_key != "none" {
        out.keyed = true;
        if p.row_key == "swatch_after" {
            let sq: Vec<[f64; 2]> = out
                .keys
                .iter()
                .filter_map(|k| match k {
                    Primitive::Rect { bounds: b, .. } => Some([b.x, b.x + b.width]),
                    _ => None,
                })
                .collect();
            let sq_l = sq.iter().map(|s| s[0]).fold(f64::INFINITY, f64::min);
            let sq_r = sq.iter().map(|s| s[1]).fold(f64::NEG_INFINITY, f64::max);
            let name_end = out
                .texts
                .iter()
                .filter(|t| t.role == "row-header")
                .map(|t| t.bbox()[2])
                .fold(f64::NEG_INFINITY, f64::max);
            let (to_name, to_value) = (sq_l - name_end, first_values - sq_r);
            if to_value < 0.8 * to_name {
                out.findings.push(finding(
                    "CG-1",
                    "warn",
                    &format!("the table's key squares stand {to_value:.1} pt from the first column of values and {to_name:.1} pt from their names: they would read with the numbers"),
                    Some("table.row_key"),
                ));
            }
        }
    }
    Ok(out)
}

/// The key a table row's name carries (bars._row_key). Halo keys (a group holding a copy of
/// the name) need a group primitive with classes and are an explicit error until scenes carry
/// them.
fn row_key(
    kit: &Kit,
    kind: &str,
    head: &Placed,
    r: &Row,
    cap: f64,
    plot_left: f64,
) -> Result<Vec<Primitive>> {
    if matches!(kind, "none" | "text") {
        return Ok(vec![]);
    }
    let b = head.bbox();
    let yc = head.y - cap / 2.;
    let (x_start, x_end) = (b[0], b[2]);
    let k = kit.metric("--legend-key")?;
    let mut cls = vec!["legend-swatch"];
    if r.reference {
        cls.push("target-key");
    }
    cls.push(r.colour.as_str());
    let paint = kit::fill(&kit::class_paint(kit, &r.colour, &[]));
    let data = [("series", r.id.clone())];
    Ok(match kind {
        "swatch_after" => vec![kit::rect(
            plot_left,
            yc - k / 2.,
            k,
            k,
            paint,
            meta(&cls, &data),
        )],
        "swatch" => vec![kit::rect(0., yc - k / 2., k, k, paint, meta(&cls, &data))],
        "swatch_beside" => vec![kit::rect(
            x_start - kit.metric("--key-gap")? - k,
            yc - k / 2.,
            k,
            k,
            paint,
            meta(&cls, &data),
        )],
        "pill" => {
            let (px, py) = (
                kit.metric("--key-pill-pad-x")?,
                kit.metric("--key-pill-pad-y")?,
            );
            let h = cap + 2. * py + 2.;
            vec![kit::rect(
                x_start - px,
                yc - h / 2.,
                (x_end - x_start) + 2. * px,
                h,
                paint,
                meta(&["key-pill", r.colour.as_str()], &data),
            )]
        }
        other => {
            return Err(kit::err(format!(
                "table.row_key {other}: a halo key round the name is not placed by this layout"
            ))
            .at("table.row_key"));
        }
    })
}

/// The legend decision (table.legend): keyed rows are the chart's key, so the legend goes
/// unless kept; `drop` without keyed rows keeps it with a warning. Returns whether the legend
/// is dropped.
pub fn drop_legend(p: &Plan, keyed: bool, has_refs: bool, findings: &mut Vec<Finding>) -> bool {
    let drop = p.legend == "drop" || (p.legend == "auto" && keyed && !has_refs);
    if drop && !(keyed && !has_refs) {
        let mut f = finding(
            "R4-1",
            "warn",
            "table.legend drop needs a table whose rows are keyed (table.row_key): the legend kept",
            Some("table.legend"),
        );
        f.level = "guided".into();
        findings.push(f);
        return false;
    }
    drop
}

/// The widths of the columns of a table beside horizontal bars, one per series (the right room
/// the bars give up): each the widest of its header and cells.
pub fn beside_widths(kit: &Kit, p: &Plan) -> Result<Vec<f64>> {
    p.rows
        .iter()
        .map(|r| {
            let mut w = kit.width("column-header", &[], &r.name)?;
            for (t, role) in &r.cells {
                w = w.max(kit.width(role, &[], &TextContent::plain(t))?);
            }
            Ok(w)
        })
        .collect()
}

/// The table beside horizontal bars: one column per series from the right edge `w_fig`
/// leftwards, headers `--table-header-drop` above the plot top (y = 0 in the plot's frame,
/// shifted by `top`), each value level with its category (band `band`, digit height `h0`).
pub fn beside(
    kit: &Kit,
    p: &Plan,
    w_fig: f64,
    top: f64,
    band: f64,
    h0: f64,
) -> Result<Vec<Placed>> {
    let widths = beside_widths(kit, p)?;
    let gap = kit.metric("--table-col-gap")?;
    let hy = top - kit.metric("--table-header-drop")?;
    let mut xc = w_fig;
    let mut out = vec![];
    for (r, w) in p.rows.iter().zip(&widths).rev() {
        out.push(kit.text("column-header", &[], &r.name, xc, hy, "end")?);
        for (i, (t, role)) in r.cells.iter().enumerate() {
            out.push(kit.text(
                role,
                &[],
                &TextContent::plain(t),
                xc,
                top + (i as f64 + 0.5) * band + h0 / 2.,
                "end",
            )?);
        }
        xc -= w + gap;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn datum(v: i64) -> Datum {
        Datum {
            value: Some(Decimal::from(v)),
            status: "known".into(),
            certainty: "actual".into(),
            complete: true,
            note: None,
            limit: None,
        }
    }

    /// p2-01-donations-table: the golden table under the bars. Bars' plot left 59.06, band
    /// 51.49, the names' right edge 50.06, the category labels' baseline 221.19 (golden SVG);
    /// the ascent of H the original measures (6.83 at 10 pt).
    #[test]
    fn table_under_the_bars_matches_the_original() {
        let spec = serde_json::json!({
            "data": {"x": {"kind": "nominal", "values": ["East", "North", "West", "Central", "South", "Northwest"]},
                     "measure": {"kind": "amount", "unit": {"prefix": "$"}},
                     "series": [{"id": "cash", "name": "Cash"}, {"id": "card", "name": "Credit card"}, {"id": "cheque", "name": "Cheque"}]},
            "facts": {"exact_values_needed": true}
        });
        let vals = [
            [5374, 4225, 3950, 1741, 1456, 791],
            [5136, 4374, 3608, 1835, 1615, 737],
            [3821, 2988, 2530, 1395, 1094, 524],
        ];
        let data = PreparedData {
            categories: ["East", "North", "West", "Central", "South", "Northwest"]
                .map(String::from)
                .into(),
            x_kind: "nominal".into(),
            series: ["cash", "card", "cheque"]
                .iter()
                .zip(vals)
                .map(|(id, v)| Series {
                    id: id.to_string(),
                    name: id.to_string(),
                    values: v.iter().map(|x| datum(*x)).collect(),
                })
                .collect(),
            measure: serde_json::json!({}),
            irregular: None,
            comparisons: vec![],
        };
        let bars: Vec<BarSeries> = (0..3)
            .map(|i| BarSeries {
                index: i,
                colour: format!("series-{}", i + 1),
                reference: false,
            })
            .collect();
        let p = plan(&spec, &data, &bars, &[0, 1, 2, 3, 4, 5], false)
            .unwrap()
            .unwrap();
        assert_eq!(p.row_key, "swatch_after");
        assert_eq!(p.rows[0].cells[0].0, "5,374");
        // labelled bars take no table (02 D8)
        assert!(
            plan(&spec, &data, &bars, &[0, 1, 2, 3, 4, 5], true)
                .unwrap()
                .is_none()
        );
        let theme = Theme::default();
        let c = LayoutConstraints {
            width: 368.,
            height: None,
            measurement_context: "t".into(),
            max_rounds: 8,
            hooks: None,
        };
        // stand-in widths (the positions checked do not depend on them); the ink ascent of H at
        // 10 pt, 6.83, as the original measures it
        let mut m = Measurements {
            context: "t".into(),
            entries: BTreeMap::new(),
        };
        let k0 = Kit::new(&theme, &c, &m);
        let _ = under(&k0, &p, 59.06, 50.06, 51.49, 221.19);
        let mut reqs = vec![];
        for r in &p.rows {
            reqs.push(k0.request("row-header", &[], &r.name).unwrap());
            for (t, role) in &r.cells {
                reqs.push(k0.request(role, &[], &TextContent::plain(t)).unwrap());
            }
        }
        reqs.push(
            k0.request("row-header", &[], &TextContent::plain("H"))
                .unwrap(),
        );
        for r in reqs {
            let w = r.content.readable().chars().count() as f64 * 5.;
            m.entries.insert(
                r.id,
                TextMetrics {
                    width: w,
                    height: 6.83,
                    baseline: Some(6.83),
                },
            );
        }
        let kit = Kit::new(&theme, &c, &m);
        let u = under(&kit, &p, 59.06, 50.06, 51.49, 221.19).unwrap();
        assert!(u.keyed && u.findings.is_empty());
        let heads: Vec<(f64, f64)> = u
            .texts
            .iter()
            .filter(|t| t.role == "row-header")
            .map(|t| (t.x, t.y))
            .collect();
        // golden: row headers at x 50.06, baselines 241.19, 257.19, 273.19
        assert_eq!(heads.len(), 3);
        for (k, (x, y)) in heads.iter().enumerate() {
            assert!((x - 50.06).abs() < 0.01 && (y - (241.19 + 16. * k as f64)).abs() < 0.01);
        }
        // golden: the first column's cells at x 84.81 (the band's middle)
        let first: Vec<f64> = u
            .texts
            .iter()
            .filter(|t| t.role == "table-cell")
            .map(|t| t.x)
            .take(1)
            .collect();
        assert!((first[0] - 84.805).abs() < 0.01);
        // golden: swatches at x 59.06, y 235.03, 251.03, 267.03
        let sq: Vec<(f64, f64)> = u
            .keys
            .iter()
            .filter_map(|k| match k {
                Primitive::Rect { bounds, .. } => Some((bounds.x, bounds.y)),
                _ => None,
            })
            .collect();
        for (k, (x, y)) in sq.iter().enumerate() {
            assert!(
                (x - 59.06).abs() < 0.01 && (y - (235.025 + 16. * k as f64)).abs() < 0.01,
                "{x} {y}"
            );
        }
        let mut f = vec![];
        assert!(drop_legend(&p, u.keyed, false, &mut f) && f.is_empty());
    }
}
