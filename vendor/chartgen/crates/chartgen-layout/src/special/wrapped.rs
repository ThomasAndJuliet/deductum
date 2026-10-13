//! Wrapped bar charts: one long sorted bar chart cut into side-by-side columns on one scale,
//! ported from the original's `layout/wrapped.py` (SPEC §6.11b; notes 07 R7-14, §5.7; G1.39,
//! G7.7): one list, sorted, cut into the fewest columns whose rows fit the height allowed; one
//! length per unit in every column, each column's scale stopping at its own top; names left
//! of each bar, scale labels at the top and the bottom of each column, thinned where they
//! would meet; the highlight in the strong shade or as a border.
use super::annotate::{Drawn, Laid};
use super::fig::{self, Batch, Item, Txt, attrs};
use super::step::{guided, r2};
use super::*;
use rust_decimal::prelude::ToPrimitive;
use serde_json::Value;

pub(crate) fn layout(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: &TrainedScale,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
) -> Result<LayoutStep> {
    let batch = Batch::new(theme, c, m);
    let draw = |w: f64| draw(plan, data, scale, &batch, w);
    let pass = match super::annotate::layout(plan, data, &batch, &draw) {
        Err(e) => Err(e),
        Ok(Err(findings)) => Ok(LayoutStep::Refused { findings }),
        Ok(Ok(laid)) => {
            let mut summary = laid.summary;
            summary.insert("width".into(), json!(r2(laid.width)));
            summary.insert("height".into(), json!(r2(laid.height)));
            fig::lower(&laid.root, batch.theme).map(|root| LayoutStep::Complete {
                chart: PlacedChart {
                    width: laid.width,
                    height: laid.height,
                    primitives: vec![root],
                    findings: laid.findings,
                    summary,
                    decisions: laid.decisions,
                },
            })
        }
    };
    fig::run_pass(c, m, &batch, pass)
}

/// The sizes of k columns holding n items, the earlier ones one longer (34, 33, 33).
fn columns(n: usize, k: usize) -> Vec<usize> {
    (0..k).map(|j| n / k + usize::from(j < n % k)).collect()
}

/// 10^e, exact.
fn pow10(e: i64) -> Decimal {
    if e >= 0 {
        Decimal::from(10u64.pow(e as u32))
    } else {
        Decimal::ONE / Decimal::from(10u64.pow((-e) as u32))
    }
}

fn plain(v: Decimal) -> String {
    v.normalize().to_string()
}

fn mantissa(v: Decimal) -> Decimal {
    let f = v.to_f64().unwrap_or(1.);
    let e = f.log10().floor() as i32;
    let p = pow10(e as i64);
    (v / p).normalize()
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
    let mut findings = vec![];
    let mut decisions: Vec<Value> = vec![];
    let regular: Vec<usize> = (0..data.series.len())
        .filter(|&i| {
            matches!(
                raw["data"]["series"][i]["role"].as_str(),
                None | Some("regular")
            )
        })
        .collect();
    if regular.len() != 1 {
        findings.push(
            Finding::new(
                "CG-3",
                "info",
                "a wrapped bar chart of several series is not drawn: one series, cut into columns",
            )
            .at("data.series"),
        );
        return Ok(Drawn::Refused(findings));
    }
    let s = &data.series[regular[0]];
    let hl: Vec<Value> = raw["highlight"].as_array().cloned().unwrap_or_default();
    if hl.iter().any(|h| h["method"] == "thickness") {
        let f = super::step::refusal(
            plan,
            "R15-6",
            "a highlighted bar may not be wider: bars are highlighted by colour",
            "highlight[].method",
            Some("method auto or border".into()),
        );
        if f.severity == "error" {
            return Ok(Drawn::Refused(vec![f]));
        }
        findings.push(f);
    }
    // the list, sorted; the catch-all, then values not known, last
    let roles = &raw["data"]["x"]["roles"];
    let role_of = |name: &str| roles.get(name).and_then(Value::as_str);
    let tail_roles = ["all_others", "missing"];
    let items: Vec<(usize, &Datum)> = s.values.iter().enumerate().collect();
    let mut known: Vec<(usize, &Datum)> = items
        .iter()
        .copied()
        .filter(|(i, v)| {
            v.value.is_some()
                && !role_of(&data.categories[*i]).is_some_and(|r| tail_roles.contains(&r))
        })
        .collect();
    let mut tail: Vec<(usize, &Datum)> = items
        .iter()
        .copied()
        .filter(|(i, v)| {
            v.value.is_some()
                && role_of(&data.categories[*i]).is_some_and(|r| tail_roles.contains(&r))
        })
        .collect();
    tail.sort_by_key(|(i, _)| {
        tail_roles
            .iter()
            .position(|r| Some(*r) == role_of(&data.categories[*i]))
    });
    let unknown: Vec<(usize, &Datum)> = items
        .iter()
        .copied()
        .filter(|(_, v)| v.value.is_none())
        .collect();
    let sort = match raw.pointer("/chart/sort").and_then(Value::as_str) {
        Some("given") => "given",
        Some("asc") => "asc",
        Some("desc") => "desc",
        _ if raw.pointer("/data/x/kind").and_then(Value::as_str) == Some("ordinal") => "given",
        _ => "desc",
    };
    if sort != "given" {
        // a stable sort, as Python's sorted(…, reverse=…) keeps ties in their order
        if sort == "asc" {
            known.sort_by(|a, b| a.1.value.cmp(&b.1.value));
        } else {
            known.sort_by(|a, b| b.1.value.cmp(&a.1.value));
        }
        decisions.push(fig::decision(
            "02 D1",
            format!(
                "one list, sorted by value, {}",
                if sort == "asc" {
                    "ascending"
                } else {
                    "descending"
                }
            ),
            None,
        ));
    }
    let mut order = known;
    order.extend(tail);
    order.extend(unknown);
    let n = order.len();
    // the columns: the fewest whose rows fit the height allowed (G7.7)
    let pitch = batch.metric("--line-pitch")?;
    let limit = batch.metric("--wrap-max-aspect")? * w_total;
    let k = (1..=n)
        .find(|k| n.div_ceil(*k) as f64 * pitch <= limit)
        .unwrap_or(n);
    let sizes = columns(n, k);
    let mut cols: Vec<Vec<(usize, &Datum)>> = vec![];
    let mut at = 0;
    for size in &sizes {
        cols.push(order[at..at + size].to_vec());
        at += size;
    }
    let names = &raw["data"]["x"]["names"];
    let content = |i: usize| -> Result<TextContent> { guides::category_content(plan, data, i) };
    let plain_name = |i: usize| -> String {
        let c = &data.categories[i];
        names
            .get(c)
            .and_then(|v| guides::content(v).ok().flatten())
            .map(|t| t.readable().to_owned())
            .unwrap_or_else(|| c.clone())
    };
    let mut name_w = vec![];
    for col in &cols {
        let mut w: f64 = 0.;
        for (i, _) in col {
            w = w.max(batch.ext_of("category-label", &content(*i)?)?.2.width);
        }
        name_w.push(w);
    }
    // one scale, the same length per unit in every column; each column's own top
    let labels = chartgen_format::scale_labels(&plan.spec, scale)?;
    let mag = labels.magnitude;
    let measure = &plan.spec.raw["data"]["measure"];
    let kind = measure["kind"].as_str().unwrap_or("count");
    let unit = chartgen_format::Unit::from_value(&measure["unit"]);
    let loc = chartgen_format::locale(&plan.spec.string("/audience/locale", "en-GB"));
    let abbreviations = plan.spec.get("/audience/abbreviations");
    let mname = measure["name"].as_str().unwrap_or("");
    let bare = |ticks: &[Decimal], step: Decimal| -> Result<Vec<String>> {
        Ok(chartgen_format::format_scale(
            ticks,
            step,
            kind,
            &unit,
            Some(mag),
            "auto",
            abbreviations,
            loc.clone(),
            mname,
        )?
        .bare)
    };
    let col_gap = batch.metric("--wrap-column-gap")?;
    let lab_gap = batch.metric("--category-label-gap")?;
    let sep = batch.metric("--time-label-sep")?;
    let maxes: Vec<Decimal> = cols
        .iter()
        .map(|col| {
            col.iter()
                .filter_map(|(_, v)| v.value)
                .max()
                .unwrap_or(Decimal::ZERO)
        })
        .collect();
    let nice_top = |v: Decimal| -> Decimal {
        if v <= Decimal::ZERO {
            return scale.step;
        }
        let p = v.to_f64().unwrap_or(1.).log10().floor() as i64;
        for q in [p, p + 1] {
            for mant in [1u32, 2, 5] {
                let t = Decimal::from(mant) * pow10(q);
                if t >= v {
                    return t.normalize();
                }
            }
        }
        pow10(p + 1)
    };
    let fit = |ticks: &[Decimal], labs: &[String], u: f64| -> Result<bool> {
        let mut ws = vec![];
        for t in labs {
            ws.push(batch.width("scale-label", t)?);
        }
        Ok(ticks
            .windows(2)
            .zip(ws.windows(2))
            .all(|(t, w)| (t[1] - t[0]).to_f64().unwrap_or(0.) * u - (w[0] + w[1]) / 2. >= sep))
    };
    let col_scale = |j: usize, u: f64| -> Result<(Vec<Decimal>, Decimal, Vec<String>)> {
        let (ticks, step) = if j == 0 || maxes[j] >= scale.hi {
            (scale.ticks.clone(), scale.step)
        } else {
            let top = nice_top(maxes[j]);
            let mut chosen = (vec![Decimal::ZERO, top], top);
            for div in [5u32, 2] {
                let st = top / Decimal::from(div);
                let mant = mantissa(st);
                if ![Decimal::ONE, Decimal::from(2), Decimal::from(5)].contains(&mant) {
                    continue;
                }
                let cand: Vec<Decimal> = (0..=div)
                    .map(|i| (st * Decimal::from(i)).normalize())
                    .collect();
                let labs = bare(&cand, st)?;
                if fit(&cand, &labs, u)? {
                    chosen = (cand, st);
                    break;
                }
            }
            chosen
        };
        let labs = bare(&ticks, step)?;
        Ok((ticks, step, labs))
    };
    let total = |u: f64| -> Result<f64> {
        let mut w = (k - 1) as f64 * col_gap;
        for (j, nw) in name_w.iter().enumerate() {
            let (ticks, _, labs) = col_scale(j, u)?;
            w += nw
                + lab_gap
                + ticks.last().unwrap().to_f64().unwrap_or(0.) * u
                + batch.width("scale-label", labs.last().unwrap())? / 2.;
        }
        Ok(w)
    };
    let (mut lo_u, mut hi_u) = (0.0f64, w_total);
    for _ in 0..40 {
        let mid = (lo_u + hi_u) / 2.;
        if total(mid)? <= w_total {
            lo_u = mid;
        } else {
            hi_u = mid;
        }
    }
    let u = lo_u;
    if u <= 0.5 {
        findings.push(
            Finding::new(
                "CG-1",
                "error",
                format!(
                    "the names of {k} columns leave no room for the bars within {w_total:.0} pt: widen the chart"
                ),
            )
            .at("layout.width"),
        );
        return Ok(Drawn::Refused(findings));
    }
    let scales: Vec<(Vec<Decimal>, Decimal, Vec<String>)> =
        (0..k).map(|j| col_scale(j, u)).collect::<Result<_>>()?;
    batch.need("category-label", "H")?;
    // the title block, the top labels' row, the rows, the bottom labels' row, the scale title
    let block = fig::title_block(plan, batch, w_total, &mut findings)?;
    let cap = batch
        .ext("scale-label", scales[0].2.last().unwrap())?
        .ascent;
    let drop = batch.metric("--scale-label-drop")?;
    let top_base = match block.last_baseline {
        Some(b) => b + batch.metric("--title-gap")?,
        None => cap,
    };
    let plot_top = top_base + drop;
    let rows = *sizes.iter().max().unwrap_or(&0);
    let band = pitch;
    let bw = batch.metric("--band-fill")? * band;
    let plot_bottom = plot_top + rows as f64 * band;
    let bot_base = plot_bottom + drop + cap;
    let cap_h = batch.ext("category-label", "H")?.ascent;
    // the highlight: what the entries name, else what the insight names
    let mut refs: Vec<&Value> = hl
        .iter()
        .flat_map(|h| h["targets"].as_array().into_iter().flatten())
        .collect();
    if hl.is_empty() {
        refs.extend(
            raw.pointer("/insight/targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
    }
    let names_hl: Vec<String> = refs
        .iter()
        .filter_map(|r| {
            r["category"]
                .as_str()
                .or_else(|| r["x"].as_str())
                .map(str::to_owned)
        })
        .collect();
    let methods: Vec<&str> = hl
        .iter()
        .map(|h| h["method"].as_str().unwrap_or("auto"))
        .collect();
    let methods: Vec<&str> = if methods.is_empty() {
        vec!["auto"]
    } else {
        methods
    };
    let border =
        methods.contains(&"border") && !methods.iter().any(|m| matches!(*m, "auto" | "strong"));
    let dim = hl.iter().any(|h| h["dim_others"] == true);
    for (i, h) in hl.iter().enumerate() {
        let m = h["method"].as_str().unwrap_or("auto");
        if m == "border" {
            guided(
                plan,
                &mut findings,
                "R15-5",
                &format!("highlight[{i}].method"),
                "border",
                "strong",
                "a bar in the strong shade of its hue (D-9); a border works too (G8.9)",
            );
        }
        if !matches!(m, "auto" | "strong" | "border" | "thickness") {
            findings.push(
                Finding::new(
                    "CG-3",
                    "info",
                    format!(
                        "highlight[{i}] {m}: on a wrapped bar chart, drawn as the strong shade"
                    ),
                )
                .at(format!("highlight[{i}].method")),
            );
        }
    }
    let notes_text = raw.pointer("/labels/notes");
    let note_of = |status: &str| -> String {
        notes_text
            .and_then(|n| n.get(status))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("({status})"))
    };
    let mut left = 0.0;
    let mut col_geo: Vec<String> = vec![];
    let (mut dropped, mut thinned) = (vec![], vec![]);
    let bwid = fig::metric_or(batch.theme, "--border-width", 0.2 * em);
    let mut root_children = vec![Item::group(
        "header",
        block.texts.iter().cloned().map(Item::Text).collect(),
    )];
    decisions.extend(block.decisions.iter().cloned());
    let mut texts: Vec<Txt> = block.texts.clone();
    let mut title_target: Option<usize> = None;
    let mut panels: Vec<Vec<Item>> = vec![];
    for (j, (col, (ticks, step, labs))) in cols.iter().zip(&scales).enumerate() {
        let x0 = left + name_w[j] + lab_gap;
        let xv = |v: Decimal| x0 + v.to_f64().unwrap_or(0.) * u;
        let x1 = xv(*ticks.last().unwrap());
        let mut grid = vec![];
        let (mut top_g, mut bot_g, mut cat_g, mut notes) = (vec![], vec![], vec![], vec![]);
        for t in ticks {
            grid.push(Item::line(
                xv(*t),
                plot_top,
                xv(*t),
                plot_bottom,
                if t.is_zero() {
                    "gridline zero"
                } else {
                    "gridline"
                },
                attrs(&[("value", plain(*t))]),
            ));
        }
        let mut ws = vec![];
        for t in labs {
            ws.push(batch.width("scale-label", t)?);
        }
        let mut show = vec![labs.len() - 1];
        for mstep in 1..labs.len() {
            let shown_step = *step * Decimal::from(mstep as u64);
            if mstep > 1
                && ![Decimal::ONE, Decimal::from(2), Decimal::from(5)]
                    .contains(&mantissa(shown_step))
            {
                continue;
            }
            let idx: Vec<usize> = (0..labs.len())
                .filter(|&i| (ticks[i] / shown_step).fract().is_zero())
                .collect();
            if idx.len() < 2 {
                break;
            }
            if idx
                .windows(2)
                .all(|p| xv(ticks[p[1]]) - xv(ticks[p[0]]) - (ws[p[0]] + ws[p[1]]) / 2. >= sep)
            {
                show = idx;
                break;
            }
        }
        if show.len() == 1 && labs.len() > 1 {
            dropped.push(j + 1);
        } else if show.len() < labs.len() {
            thinned.push(j + 1);
        }
        for (g, y) in [(&mut top_g, top_base), (&mut bot_g, bot_base)] {
            for &i in &show {
                let t = batch.plain("scale-label", &labs[i], xv(ticks[i]), y, "middle")?;
                texts.push(t.clone());
                g.push(Item::Text(t));
            }
        }
        let mut drawn: Vec<(bool, Item)> = vec![];
        for (r, (i, v)) in col.iter().enumerate() {
            let yc = plot_top + (r as f64 + 0.5) * band;
            let name = &data.categories[*i];
            let mut t = batch.text(
                "category-label",
                content(*i)?,
                x0 - lab_gap,
                yc + cap_h / 2.,
                "end",
            )?;
            t.plain = plain_name(*i);
            texts.push(t.clone());
            cat_g.push(Item::Text(t));
            let Some(value) = v.value else {
                let status = if v.status.is_empty() {
                    "missing"
                } else {
                    &v.status
                };
                let note = note_of(status);
                let t = batch.plain(
                    "missing-note",
                    &note,
                    x0 + batch.metric("--missing-note-inset")?,
                    yc + cap_h / 2.,
                    "start",
                )?;
                texts.push(t.clone());
                notes.push(Item::Text(t));
                continue;
            };
            let hit = names_hl.contains(name);
            let colour = match role_of(name) {
                Some("all_others") => "all-others",
                Some("total") => "total",
                Some("average") => "average",
                Some("missing") => "missing",
                _ => "single",
            };
            let emph = if hit && !border {
                " highlight"
            } else if dim && !names_hl.is_empty() && !hit {
                " context"
            } else {
                ""
            };
            let length = value.to_f64().unwrap_or(0.) * u;
            drawn.push((
                hit,
                Item::rect(
                    x0,
                    yc - bw / 2.,
                    length,
                    bw,
                    &format!("bar {colour}{emph}"),
                    attrs(&[
                        ("series", s.id.clone()),
                        ("x", name.clone()),
                        ("value", plain(value)),
                    ]),
                ),
            ));
            if value.is_zero() {
                let z = note_of("zero");
                let t = batch.plain(
                    "missing-note",
                    &z,
                    x0 + batch.metric("--missing-note-inset")?,
                    yc + cap_h / 2.,
                    "start",
                )?;
                texts.push(t.clone());
                notes.push(Item::Text(t));
            }
            if hit && border {
                notes.push(Item::rect(
                    x0 + bwid / 2.,
                    yc - bw / 2. + bwid / 2.,
                    (length - bwid).max(0.),
                    (bw - bwid).max(0.),
                    &format!("border {colour} highlight"),
                    attrs(&[("series", s.id.clone()), ("x", name.clone())]),
                ));
            }
        }
        let mut series = vec![];
        series.extend(drawn.iter().filter(|d| !d.0).map(|d| d.1.clone()));
        series.extend(drawn.iter().filter(|d| d.0).map(|d| d.1.clone()));
        if j == 0 && !labels.title.is_empty() {
            title_target = Some(j);
        }
        panels.push(vec![
            Item::group("grid", grid),
            Item::group(
                "marks",
                vec![Item::Group {
                    classes: fig::cls("series single"),
                    id: None,
                    data: attrs(&[("series", s.id.clone())]),
                    children: series,
                }],
            ),
            Item::group("scale value", top_g),
            Item::group("scale value", bot_g),
            Item::group("scale category", cat_g),
            Item::group("annotations", notes),
        ]);
        col_geo.push(format!(
            "0–{} by {}",
            plain(*ticks.last().unwrap()),
            plain(*step)
        ));
        let right = show
            .iter()
            .map(|&i| xv(ticks[i]) + ws[i] / 2.)
            .fold(x1, f64::max);
        left = right + col_gap;
    }
    // the scale's title, centred under the chart, in the first column's lower scale group
    let mut bottom = bot_base + 2.0;
    if let Some(j) = title_target {
        let e = batch.ext("scale-title", &labels.title)?;
        let ty = bot_base + batch.metric("--unit-line-drop")? + e.ascent + 2.0;
        let t = batch.plain("scale-title", &labels.title, w_total / 2., ty, "middle")?;
        texts.push(t.clone());
        if let Item::Group { children, .. } = &mut panels[j][3] {
            children.push(Item::Text(t));
        }
        bottom = ty + e.descent;
    }
    for (j, children) in panels.into_iter().enumerate() {
        root_children.push(Item::Group {
            classes: fig::cls("panel"),
            id: Some(format!("column-{}", j + 1)),
            data: vec![],
            children,
        });
    }
    root_children.push(Item::group("footer", vec![]));
    let unit_word = if kind == "percent" { "point" } else { "unit" };
    decisions.push(fig::decision(
        "R7-14",
        format!(
            "{n} parts in {k} column{} of {}, filled top to bottom, on one scale ({u:.2} pt per {unit_word}), each column's scale stopping at its own top: {} (G7.7)",
            if k > 1 { "s" } else { "" },
            sizes.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", "),
            col_geo.join("; ")
        ),
        None,
    ));
    let list = |v: &[usize]| {
        v.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !thinned.is_empty() {
        decisions.push(fig::decision(
            "R2-40",
            format!(
                "scale labels thinned where they would meet, in column{}{}, the gridlines kept at every step",
                if thinned.len() > 1 { "s " } else { " " },
                list(&thinned)
            ),
            None,
        ));
    }
    if !dropped.is_empty() {
        decisions.push(fig::decision(
            "R7-14",
            format!(
                "too narrow for two scale labels, column{}{} labelled at the top of its scale only, its zero the zero line",
                if dropped.len() > 1 { "s " } else { " " },
                list(&dropped)
            ),
            None,
        ));
    }
    if !names_hl.is_empty() {
        let mut sorted = names_hl.clone();
        sorted.sort();
        decisions.push(fig::decision(
            "R3-7",
            format!(
                "{}{}{}",
                if border {
                    "a border in the strong shade inside: "
                } else {
                    "highlighted in the strong shade of its colour: "
                },
                sorted.join(", "),
                if dim { "; the others paled" } else { "" }
            ),
            None,
        ));
    }
    let height = bottom + 1.0;
    let root = Item::Group {
        classes: fig::cls("chart bar wrapped horizontal categorical"),
        id: Some("chart".into()),
        data: vec![],
        children: root_children,
    };
    let mut summary = BTreeMap::new();
    summary.insert("orientation".into(), json!("horizontal"));
    summary.insert("columns".into(), json!(k));
    summary.insert("rows".into(), json!(sizes));
    summary.insert(
        "unit".into(),
        json!(format!("{u:.3}").parse::<f64>().unwrap_or(u)),
    );
    summary.insert("column_scales".into(), json!(col_geo));
    summary.insert(
        "plot".into(),
        json!([0.0, r2(plot_top), r2(w_total), r2(plot_bottom)]),
    );
    let _ = texts;
    Ok(Drawn::Laid(Laid {
        root,
        width: w_total,
        height,
        plot: (0., plot_top, w_total, plot_bottom),
        findings,
        summary,
        time: None,
        decisions,
    }))
}
