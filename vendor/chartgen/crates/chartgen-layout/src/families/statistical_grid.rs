//! The grid statistical charts: the categorical state matrix (a declared state in every cell,
//! shown by colour and by shape) and the exhaustive two-way count table (STATS.md).
use crate::guides;
use crate::kit::{self, Kit};
use crate::statistical::*;
use chartgen_model::*;
use chartgen_statistics::states::{Contingency, StateMatrix};
use serde_json::json;
use std::collections::BTreeMap;

/// Rows are the x categories (labels read horizontally), columns the series: the transposed
/// reading of the spec's `states`, which are given per series. With `orientation: vertical` the
/// categories run across, when their labels fit a cell.
pub(crate) fn layout_matrix(
    kit: &Kit,
    plan: &SemanticPlan,
    data: &PreparedData,
    m: &StateMatrix,
) -> Result<PlacedChart> {
    let (w, em) = (kit.c.width, kit.metric("--label-size")?);
    let (mut findings, mut decisions) = (vec![], vec![]);
    let (title_prims, top) = title(kit, plan, &mut findings, &mut decisions)?;
    let across = !is_horizontal(plan);
    // m.rows are the series, m.cols the categories
    let (row_labels, col_labels): (Vec<TextContent>, Vec<TextContent>) = if across {
        (
            (0..m.rows.len())
                .map(|i| guides::series_content(plan, data, i))
                .collect::<Result<_>>()?,
            (0..m.cols.len())
                .map(|i| guides::category_content(plan, data, i))
                .collect::<Result<_>>()?,
        )
    } else {
        (
            (0..m.cols.len())
                .map(|i| guides::category_content(plan, data, i))
                .collect::<Result<_>>()?,
            (0..m.rows.len())
                .map(|i| guides::series_content(plan, data, i))
                .collect::<Result<_>>()?,
        )
    };
    let (n_rows, n_cols) = (row_labels.len(), col_labels.len());
    let state_at = |r: usize, c: usize| if across { m.cells[r][c] } else { m.cells[c][r] };

    // legend of the declared states: colour and shape together
    let keys: Vec<Key> = m
        .states
        .iter()
        .enumerate()
        .map(|(k, (_, name))| {
            let class = format!("series-{}", k + 1);
            Key {
                glyph: Glyph::Mark(MARKS[k % 6]),
                fill: kit::class_paint(kit, &class, &[]),
                strong: kit::class_paint(kit, &class, &["highlight"]),
                text: TextContent::plain(name),
            }
        })
        .collect();
    let legend_y = top + em * 0.4;
    let (legend_prims, legend_h) = legend(kit, &keys, 0., legend_y, w)?;
    let grid_top = legend_y + legend_h + em * 0.8;

    let row_w = row_labels
        .iter()
        .map(|l| kit.width("category-label", &[], l))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(0., f64::max);
    let gap = kit.metric("--category-label-gap")?;
    let left = row_w + gap;
    let pitch = kit.metric("--cell-pitch")?.max(em * 1.6);
    let col_w = ((w - left - em) / n_cols as f64)
        .min(pitch * 2.2)
        .max(pitch);
    let widest_col = col_labels
        .iter()
        .map(|l| kit.width("column-header", &[], l))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(0., f64::max);
    if widest_col > col_w - 2. {
        return Err(kit::err(if across {
            "the category labels do not fit across the matrix; use orientation horizontal"
        } else {
            "the series names do not fit across the matrix"
        })
        .at("chart.orientation"));
    }
    if left + col_w * n_cols as f64 > w + 1e-6 {
        return Err(kit::err(
            "the matrix is wider than the figure; fewer columns, or a wider figure",
        ));
    }
    let head_h = kit.ext("column-header", &[], &col_labels[0])?.ascent + 8.;
    let grid_y = grid_top + head_h;

    let (mut behind, mut front, mut marks) = (vec![], vec![], vec![]);
    for (c, l) in col_labels.iter().enumerate() {
        let e = kit.ext("column-header", &[], l)?;
        front.push(
            kit.text(
                "column-header",
                &[],
                l,
                left + c as f64 * col_w + col_w / 2.,
                grid_top + e.ascent,
                "middle",
            )?
            .primitive(),
        );
    }
    let mut counts: Vec<Vec<usize>> = vec![vec![0; m.states.len()]; n_rows];
    for r in 0..n_rows {
        let y = grid_y + r as f64 * pitch;
        let e = kit.ext("category-label", &[], &row_labels[r])?;
        front.push(
            kit.text(
                "category-label",
                &[],
                &row_labels[r],
                left - gap,
                y + pitch / 2. + (e.ascent - e.descent) / 2.,
                "end",
            )?
            .primitive(),
        );
        for c in 0..n_cols {
            let x = left + c as f64 * col_w;
            behind.push(kit::rect(
                x,
                y,
                col_w,
                pitch,
                paint(None, Some(&kit.theme.grid), kit.metric("--grid-width")?),
                kit::meta(&["cell"], &[]),
            ));
            let k = state_at(r, c);
            counts[r][k] += 1;
            let class = format!("series-{}", k + 1);
            let ink = Ink {
                fill: kit::class_paint(kit, &class, &[]),
                strong: kit::class_paint(kit, &class, &["highlight"]),
            };
            let (series_i, cat_i) = if across { (r, c) } else { (c, r) };
            marks.push(mark(
                MARKS[k % 6],
                x + col_w / 2.,
                y + pitch / 2.,
                (pitch * 0.55).min(10.),
                &ink,
                kit::meta(
                    &["state", &class],
                    &[
                        ("series", m.rows[series_i].clone()),
                        ("x", m.cols[cat_i].clone()),
                        ("value", m.states[k].0.clone()),
                    ],
                ),
            ));
        }
    }
    let after = grid_y + n_rows as f64 * pitch;
    let (note_prims, note_h) = note(kit, &m.note(), 0., after + em * 0.8, w)?;
    let computed = after + em * 0.8 + note_h + em * 0.4;
    let height = kit.c.height.map_or(Ok(computed), |h| {
        if h + 1e-6 < computed {
            Err(kit::err(format!(
                "the chart needs {computed:.0} pt of height, {h:.0} given"
            )))
        } else {
            Ok(h)
        }
    })?;
    behind.splice(0..0, title_prims);
    front.extend(legend_prims);
    front.extend(note_prims);
    kit::decide(
        &mut decisions,
        "XS-LAYOUT",
        format!(
            "a state matrix: {} states, each by colour and by shape; categories {}",
            m.states.len(),
            if across { "across" } else { "down the rows" }
        ),
        Some("guided"),
    );
    let mut summary = BTreeMap::new();
    summary.insert("kind".into(), json!("state_matrix"));
    summary.insert(
        "states".into(),
        json!(m.states.iter().map(|s| s.0.clone()).collect::<Vec<_>>()),
    );
    summary.insert("counts".into(), json!(counts));
    summary.insert("width".into(), kit::r2(w));
    summary.insert("height".into(), kit::r2(height));
    Ok(PlacedChart {
        width: w,
        height,
        primitives: vec![kit::group(
            &["chart", "state_matrix", "categorical"],
            Some("chart"),
            &[],
            vec![
                kit::group(&["frame"], None, &[], behind),
                kit::group(&["marks"], None, &[], marks),
                kit::group(&["labels"], None, &[], front),
            ],
        )],
        findings,
        summary,
        decisions,
    })
}

pub(crate) fn layout_contingency(
    kit: &Kit,
    plan: &SemanticPlan,
    data: &PreparedData,
    t: &Contingency,
) -> Result<PlacedChart> {
    let (w, em) = (kit.c.width, kit.metric("--label-size")?);
    let (mut findings, mut decisions) = (vec![], vec![]);
    let (title_prims, top) = title(kit, plan, &mut findings, &mut decisions)?;
    let row_labels: Vec<TextContent> = (0..t.rows.len())
        .map(|i| guides::series_content(plan, data, i))
        .collect::<Result<_>>()?;
    let col_labels: Vec<TextContent> = (0..t.cols.len())
        .map(|i| guides::category_content(plan, data, i))
        .collect::<Result<_>>()?;
    let gap = kit.metric("--category-label-gap")?;
    let row_w = row_labels
        .iter()
        .map(|l| kit.width("row-header", &[], l))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(0., f64::max);
    let name_w = if t.row_name.is_empty() {
        0.
    } else {
        kit.plain_ext("column-header", &[], &t.row_name)?.width
    };
    let left = row_w.max(name_w) + gap;
    let cols = t.cols.len() + 1;
    let cell_w = ((w - left - em) / cols as f64).clamp(48., 110.);
    let cell_h = (em * 4.2).max(40.);
    let head = em * 2.8;
    let top_y = top + em * 0.6 + if t.col_name.is_empty() { 0. } else { em * 1.4 };
    let (mut behind, mut front, mut marks) = (vec![], vec![], vec![]);
    if !t.col_name.is_empty() {
        front.push(
            text_at(
                kit,
                "column-header",
                &t.col_name,
                left + (cell_w * t.cols.len() as f64) / 2.,
                top + em * 0.6 + em * 0.5,
                "middle",
            )?
            .primitive(),
        );
    }
    for (j, l) in col_labels.iter().enumerate() {
        let e = kit.ext("column-header", &[], l)?;
        front.push(
            kit.text(
                "column-header",
                &[],
                l,
                left + j as f64 * cell_w + cell_w / 2.,
                top_y + e.ascent,
                "middle",
            )?
            .primitive(),
        );
    }
    front.push(
        text_at(
            kit,
            "column-header",
            "all",
            left + t.cols.len() as f64 * cell_w + cell_w / 2.,
            top_y + em * 0.6,
            "middle",
        )?
        .primitive(),
    );
    if !t.row_name.is_empty() {
        front.push(
            text_at(
                kit,
                "column-header",
                &t.row_name,
                left - gap,
                top_y + em * 0.6,
                "end",
            )?
            .primitive(),
        );
    }
    let grid_y = top_y + head;
    let shade = kit
        .token("--series-1")
        .unwrap_or_else(|| kit::class_paint(kit, "series-1", &[]));
    for (r, l) in row_labels.iter().enumerate() {
        let y = grid_y + r as f64 * cell_h;
        let e = kit.ext("row-header", &[], l)?;
        front.push(
            kit.text(
                "row-header",
                &[],
                l,
                left - gap,
                y + cell_h / 2. + (e.ascent - e.descent) / 2.,
                "end",
            )?
            .primitive(),
        );
        for c in 0..t.cols.len() {
            let x = left + c as f64 * cell_w;
            let count = t.counts[r][c];
            let share = count as f64 / t.total as f64;
            let mut p = paint(Some(&shade), Some(&kit.theme.background), 1.);
            p.opacity = 0.1 + 0.7 * share;
            marks.push(kit::rect(
                x,
                y,
                cell_w,
                cell_h,
                p,
                kit::meta(
                    &["cell"],
                    &[
                        ("series", t.rows[r].clone()),
                        ("x", t.cols[c].clone()),
                        ("value", count.to_string()),
                    ],
                ),
            ));
            front.push(
                text_at(
                    kit,
                    "table-cell",
                    &count.to_string(),
                    x + cell_w / 2.,
                    y + cell_h * 0.42,
                    "middle",
                )?
                .primitive(),
            );
            front.push(
                text_at(
                    kit,
                    "value-label",
                    &format!("{}%", pct1(share)),
                    x + cell_w / 2.,
                    y + cell_h * 0.74,
                    "middle",
                )?
                .primitive(),
            );
        }
        front.push(
            text_at(
                kit,
                "table-cell",
                &t.row_totals[r].to_string(),
                left + t.cols.len() as f64 * cell_w + cell_w / 2.,
                y + cell_h / 2.,
                "middle",
            )?
            .primitive(),
        );
    }
    let y = grid_y + t.rows.len() as f64 * cell_h;
    for (c, total) in t.col_totals.iter().enumerate() {
        front.push(
            text_at(
                kit,
                "table-cell",
                &total.to_string(),
                left + c as f64 * cell_w + cell_w / 2.,
                y + cell_h / 2.,
                "middle",
            )?
            .primitive(),
        );
    }
    front.push(
        text_at(
            kit,
            "table-cell",
            &t.total.to_string(),
            left + t.cols.len() as f64 * cell_w + cell_w / 2.,
            y + cell_h / 2.,
            "middle",
        )?
        .primitive(),
    );
    behind.push(kit::line(
        left,
        y,
        left + cols as f64 * cell_w,
        y,
        paint(None, Some(&kit.theme.grid), 1.),
        kit::meta(&["rule"], &[]),
    ));
    let after = y + cell_h;
    let (note_prims, note_h) = note(kit, &t.note(), 0., after + em * 0.6, w)?;
    let computed = after + em * 0.6 + note_h + em * 0.4;
    let height = kit.c.height.map_or(Ok(computed), |h| {
        if h + 1e-6 < computed {
            Err(kit::err(format!(
                "the chart needs {computed:.0} pt of height, {h:.0} given"
            )))
        } else {
            Ok(h)
        }
    })?;
    behind.splice(0..0, title_prims);
    front.extend(note_prims);
    kit::decide(
        &mut decisions,
        "XS-LAYOUT",
        "a table of counts with marginals, shaded by each cell's share of all cases",
        Some("guided"),
    );
    let mut summary = BTreeMap::new();
    summary.insert("kind".into(), json!("contingency"));
    summary.insert("total".into(), json!(t.total));
    summary.insert("row_totals".into(), json!(t.row_totals));
    summary.insert("column_totals".into(), json!(t.col_totals));
    summary.insert("width".into(), kit::r2(w));
    summary.insert("height".into(), kit::r2(height));
    Ok(PlacedChart {
        width: w,
        height,
        primitives: vec![kit::group(
            &["chart", "contingency", "categorical"],
            Some("chart"),
            &[],
            vec![
                kit::group(&["frame"], None, &[], behind),
                kit::group(&["marks"], None, &[], marks),
                kit::group(&["labels"], None, &[], front),
            ],
        )],
        findings,
        summary,
        decisions,
    })
}

fn pct1(share: f64) -> String {
    let v = (share * 1000.).round() / 10.;
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}
