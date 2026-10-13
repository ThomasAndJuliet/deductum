//! The two-axis statistical charts: the empirical CDF and the scatter plot (STATS.md).
use crate::guides;
use crate::kit::{self, Kit};
use crate::statistical::*;
use chartgen_model::*;
use chartgen_statistics::{distribution::EcdfModel, scatter::ScatterModel};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// One numeric axis of a two-axis chart.
struct Axis<'a> {
    scale: &'a TrainedScale,
    /// A label per tick (bare on the x axis; the top one may carry the unit on the y axis).
    labels: Vec<String>,
    /// Below the x axis, or above the y axis.
    caption: String,
}

impl Axis<'_> {
    fn frac(&self, v: f64) -> f64 {
        self.scale.fraction(dec(v)).unwrap_or(0.)
    }
}

struct Plane<'a> {
    plot: Bounds,
    x: Axis<'a>,
    y: Axis<'a>,
}

impl Plane<'_> {
    fn at(&self, x: f64, y: f64) -> Point {
        Point {
            x: self.plot.x + self.x.frac(x) * self.plot.width,
            y: self.plot.y + (1. - self.y.frac(y)) * self.plot.height,
        }
    }
}

/// Grid, tick labels and captions of both axes; returns (behind, labels).
fn axes(kit: &Kit, p: &Plane, y_label_w: f64) -> Result<(Vec<Primitive>, Vec<Primitive>)> {
    let (plot, em) = (p.plot.clone(), kit.metric("--label-size")?);
    let grid = paint(None, Some(&kit.theme.grid), kit.metric("--grid-width")?);
    let (mut behind, mut front) = (vec![], vec![]);
    let tick_h = kit.plain_ext("scale-label-formatted", &[], "0")?;
    for (i, t) in p.x.scale.ticks.iter().enumerate() {
        let x = plot.x + p.x.frac(px(*t)) * plot.width;
        behind.push(kit::line(
            x,
            plot.y,
            x,
            plot.y + plot.height,
            grid.clone(),
            kit::meta(&["grid"], &[]),
        ));
        front.push(
            text_at(
                kit,
                "scale-label-formatted",
                &p.x.labels[i],
                x,
                plot.y + plot.height + 6. + tick_h.ascent / 2.,
                "middle",
            )?
            .primitive(),
        );
    }
    for (i, t) in p.y.scale.ticks.iter().enumerate() {
        let y = plot.y + (1. - p.y.frac(px(*t))) * plot.height;
        behind.push(kit::line(
            plot.x,
            y,
            plot.x + plot.width,
            y,
            grid.clone(),
            kit::meta(&["grid"], &[]),
        ));
        front.push(
            text_at(
                kit,
                "scale-label-formatted",
                &p.y.labels[i],
                plot.x - 6.,
                y,
                "end",
            )?
            .primitive(),
        );
    }
    let ink = paint(None, Some(&kit.theme.text), 0.8);
    behind.push(kit::line(
        plot.x,
        plot.y + plot.height,
        plot.x + plot.width,
        plot.y + plot.height,
        ink.clone(),
        kit::meta(&["axis"], &[]),
    ));
    behind.push(kit::line(
        plot.x,
        plot.y,
        plot.x,
        plot.y + plot.height,
        ink,
        kit::meta(&["axis"], &[]),
    ));
    if !p.x.caption.is_empty() {
        front.push(
            text_at(
                kit,
                "scale-title",
                &p.x.caption,
                plot.x + plot.width / 2.,
                plot.y + plot.height + 6. + tick_h.ascent + 8. + 4.,
                "middle",
            )?
            .primitive(),
        );
    }
    if !p.y.caption.is_empty() {
        front.push(
            text_at(
                kit,
                "unit-line",
                &p.y.caption,
                plot.x - y_label_w - 6.,
                plot.y - em * 1.6,
                "start",
            )?
            .primitive(),
        );
    }
    Ok((behind, front))
}

fn unit_scale(ticks: &[f64]) -> TrainedScale {
    TrainedScale {
        lo: dec(ticks[0]),
        hi: dec(ticks[ticks.len() - 1]),
        step: dec(ticks[1] - ticks[0]),
        ticks: ticks.iter().map(|t| dec(*t)).collect(),
        mode: "fixed".into(),
    }
}

pub(crate) fn layout_ecdf(
    kit: &Kit,
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
    m: &EcdfModel,
) -> Result<PlacedChart> {
    let scale = scale.ok_or_else(|| kit::err("an ECDF needs a trained value scale"))?;
    let (w, em) = (kit.c.width, kit.metric("--label-size")?);
    let (mut findings, mut decisions) = (vec![], vec![]);
    let (title_prims, top) = title(kit, plan, &mut findings, &mut decisions)?;
    let n_series = data.series.len();
    let groups = m.curves.iter().map(|c| c.group).max().map_or(1, |g| g + 1);

    // colour by series; a single series with several groups takes the palette by group
    let by_group = n_series == 1 && groups > 1;
    let ink_of = |c: &chartgen_statistics::distribution::Curve| -> Ink {
        if by_group {
            let class = format!("series-{}", c.group + 1);
            Ink {
                fill: kit::class_paint(kit, &class, &[]),
                strong: kit::class_paint(kit, &class, &["highlight"]),
            }
        } else {
            series_ink(kit, plan, c.series)
        }
    };
    let class_of = |c: &chartgen_statistics::distribution::Curve| {
        if by_group {
            format!("series-{}", c.group + 1)
        } else {
            series_class(plan, c.series)
        }
    };
    let dash_of = |c: &chartgen_statistics::distribution::Curve| if by_group { 0 } else { c.group };
    if by_group {
        kit::decide(
            &mut decisions,
            "XS-LAYOUT",
            "one series in several groups: each group takes a palette colour",
            Some("guided"),
        );
    }

    let keys: Vec<Key> = m
        .curves
        .iter()
        .map(|c| {
            let ink = ink_of(c);
            Ok(Key {
                glyph: Glyph::Line(dash_of(c)),
                fill: ink.fill,
                strong: ink.strong,
                text: TextContent::plain(format!("{} (n={})", c.name, c.n)),
            })
        })
        .collect::<Result<_>>()?;
    let legend_y = top + em * 0.4;
    let (legend_prims, legend_h) = legend(kit, &keys, 0., legend_y, w)?;
    let plot_top = legend_y + legend_h + em * 2.6;

    let f = formatted(plan, scale, None)?;
    let yticks = [0., 0.25, 0.5, 0.75, 1.];
    let ys = unit_scale(&yticks);
    let ylabels: Vec<String> = yticks
        .iter()
        .map(|t| format!("{}%", (t * 100.) as i64))
        .collect();
    let y_w = ylabels
        .iter()
        .map(|l| {
            kit.plain_ext("scale-label-formatted", &[], l)
                .map(|e| e.width)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(0., f64::max);
    let left = y_w + kit.metric("--category-label-gap")? + 4.;
    let plot_h = (w * 0.5).clamp(200., 340.);
    let right = right_pad(kit, &f.bare, em)?;
    let plot = Bounds {
        x: left,
        y: plot_top,
        width: w - left - right,
        height: plot_h,
    };
    if plot.width < 40. {
        return Err(kit::err("the measured labels leave no usable plot area"));
    }
    let plane = Plane {
        plot: plot.clone(),
        x: Axis {
            scale,
            labels: f.bare.clone(),
            caption: f.title.clone(),
        },
        y: Axis {
            scale: &ys,
            labels: ylabels,
            caption: "share of measurements at or below the value".into(),
        },
    };
    let (mut behind, mut front) = axes(kit, &plane, y_w)?;

    let mut marks = vec![];
    // reference quantiles as dotted lines
    for p in &m.reference {
        let y = plane.at(0., *p).y;
        for (a, b) in dash_runs(plot.x, plot.x + plot.width, 2) {
            marks.push(kit::line(
                a,
                y,
                b,
                y,
                paint(None, Some(&kit.theme.text), 1.),
                kit::meta(&["reference"], &[("quantile", p.to_string())]),
            ));
        }
        front.push(
            text_at(
                kit,
                "scale-label-formatted",
                &format!("p{}", chartgen_format_pct(*p)),
                plot.x + plot.width - 2.,
                y - 7.,
                "end",
            )?
            .primitive(),
        );
    }
    let mut summary_curves = vec![];
    for c in &m.curves {
        let ink = ink_of(c);
        let class = class_of(c);
        let mut pts = vec![plane.at(c.steps[0].0, 0.)];
        let mut prev = 0.;
        for (x, fval) in &c.steps {
            pts.push(plane.at(*x, prev));
            pts.push(plane.at(*x, *fval));
            prev = *fval;
        }
        pts.push(Point {
            x: plot.x + plot.width,
            y: plane.at(0., 1.).y,
        });
        for run in dashed(&pts, dash_of(c)) {
            if run.len() > 1 {
                marks.push(Primitive::Path {
                    points: run,
                    closed: false,
                    paint: paint(None, Some(&ink.strong), 2.2),
                    meta: kit::meta(
                        &["curve", &class],
                        &[
                            (
                                "series",
                                plan.spec
                                    .raw
                                    .pointer(&format!("/data/series/{}/id", c.series))
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_owned(),
                            ),
                            ("x", c.name.clone()),
                        ],
                    ),
                });
            }
        }
        summary_curves.push(json!({"name": c.name, "n": c.n}));
    }
    let after =
        plot.y + plot.height + 6. + kit.plain_ext("scale-label-formatted", &[], "0")?.ascent + 22.;
    let (note_prims, note_h) = note(kit, &m.note(), 0., after + em * 0.6, w)?;
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
    front.extend(legend_prims);
    front.extend(note_prims);
    kit::decide(
        &mut decisions,
        "XS-LAYOUT",
        "an ECDF: steps over the trained value scale; the last step runs to the plot's edge",
        Some("guided"),
    );
    let mut summary = BTreeMap::new();
    summary.insert("kind".into(), json!("ecdf"));
    summary.insert("curves".into(), json!(summary_curves));
    summary.insert(
        "plot".into(),
        json!([
            kit::round2(plot.x),
            kit::round2(plot.y),
            kit::round2(plot.x + plot.width),
            kit::round2(plot.y + plot.height)
        ]),
    );
    summary.insert("width".into(), kit::r2(w));
    summary.insert("height".into(), kit::r2(height));
    let _ = (
        data,
        guides::content as fn(&Value) -> Result<Option<TextContent>>,
    );
    Ok(PlacedChart {
        width: w,
        height,
        primitives: vec![kit::group(
            &["chart", "ecdf", "categorical"],
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

fn chartgen_format_pct(p: f64) -> String {
    let v = p * 100.;
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

pub(crate) fn layout_scatter(
    kit: &Kit,
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
    m: &ScatterModel,
) -> Result<PlacedChart> {
    let ys = scale.ok_or_else(|| kit::err("a scatter plot needs a trained value scale"))?;
    let (w, em) = (kit.c.width, kit.metric("--label-size")?);
    let (mut findings, mut decisions) = (vec![], vec![]);
    let (title_prims, top) = title(kit, plan, &mut findings, &mut decisions)?;
    let ((xlo, xhi), _) = m.extents();
    let x_measure = plan
        .spec
        .raw
        .pointer("/data/x_measure")
        .cloned()
        .unwrap_or(Value::Null);
    let xs =
        chartgen_scales::explain_values(plan, vec![dec(xlo), dec(xhi)], &x_measure, &Value::Null)?
            .ok_or_else(|| kit::err("the x axis could not be trained"))?
            .scale;
    let fy = formatted(plan, ys, None)?;
    let fx = formatted(plan, &xs, Some(&x_measure))?;

    let keys: Vec<Key> = (0..m.series.len())
        .map(|i| {
            let ink = series_ink(kit, plan, i);
            Ok(Key {
                glyph: Glyph::Mark(if m.series.len() > 1 {
                    MARKS[i % 6]
                } else {
                    MARKS[0]
                }),
                fill: ink.fill,
                strong: ink.strong,
                text: guides::series_content(plan, data, i)?,
            })
        })
        .collect::<Result<_>>()?;
    let legend_y = top + em * 0.4;
    let (legend_prims, legend_h) = if keys.len() > 1 {
        legend(kit, &keys, 0., legend_y, w)?
    } else {
        (vec![], 0.)
    };
    let inline = fy.top.inline();
    let y_w = fy
        .labels
        .iter()
        .map(|l| {
            kit.plain_ext("scale-label-formatted", &[], l)
                .map(|e| e.width)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(0., f64::max);
    let inline_fits = kit.plain_ext("scale-label-formatted", &[], &inline)?.width <= y_w + 8.;
    let ylabels: Vec<String> = fy
        .labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i + 1 == fy.labels.len() && inline_fits {
                inline.clone()
            } else {
                l.clone()
            }
        })
        .collect();
    let caption = if inline_fits {
        String::new()
    } else {
        fy.unit_line.clone()
    };
    let left = y_w + kit.metric("--category-label-gap")? + 4.;
    let plot_top = legend_y
        + legend_h
        + if caption.is_empty() {
            em * 0.8
        } else {
            em * 2.6
        };
    let plot_h = (w * 0.55).clamp(200., 360.);
    let right = right_pad(kit, &fx.bare, em)?;
    let plot = Bounds {
        x: left,
        y: plot_top,
        width: w - left - right,
        height: plot_h,
    };
    if plot.width < 40. {
        return Err(kit::err("the measured labels leave no usable plot area"));
    }
    let plane = Plane {
        plot: plot.clone(),
        x: Axis {
            scale: &xs,
            labels: fx.bare.clone(),
            caption: fx.title.clone(),
        },
        y: Axis {
            scale: ys,
            labels: ylabels,
            caption,
        },
    };
    let (mut behind, mut front) = axes(kit, &plane, y_w)?;

    let mut marks = vec![];
    for (si, pts) in m.points.iter().enumerate() {
        let ink = series_ink(kit, plan, si);
        let class = series_class(plan, si);
        let href = if m.series.len() > 1 {
            MARKS[si % 6]
        } else {
            MARKS[0]
        };
        for (x, y) in pts {
            let p = plane.at(*x, *y);
            let mut mk = mark(
                href,
                p.x,
                p.y,
                7.,
                &ink,
                kit::meta(
                    &["point", &class],
                    &[
                        ("series", m.series[si].id.clone()),
                        ("x", kit::dec_text(dec(*x).round_dp(6))),
                        ("value", kit::dec_text(dec(*y).round_dp(6))),
                    ],
                ),
            );
            if let Primitive::Use { paint, .. } = &mut mk
                && m.transparent
            {
                paint.opacity = 0.4;
            }
            marks.push(mk);
        }
    }
    for f in &m.fits {
        let ink = series_ink(kit, plan, f.series);
        let (x0, x1) = m.points[f.series]
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| {
                (a.0.min(p.0), a.1.max(p.0))
            });
        let (a, b) = (
            plane.at(x0, f.intercept + f.slope * x0),
            plane.at(x1, f.intercept + f.slope * x1),
        );
        marks.push(kit::line(
            a.x,
            a.y,
            b.x,
            b.y,
            paint(None, Some(&ink.strong), 2.),
            kit::meta(
                &["fit", &series_class(plan, f.series)],
                &[("series", m.series[f.series].id.clone())],
            ),
        ));
    }
    let after =
        plot.y + plot.height + 6. + kit.plain_ext("scale-label-formatted", &[], "0")?.ascent + 22.;
    let (note_prims, note_h) = note(kit, &m.note(), 0., after + em * 0.6, w)?;
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
    front.extend(legend_prims);
    front.extend(note_prims);
    let mut summary = BTreeMap::new();
    summary.insert("kind".into(), json!("scatter"));
    summary.insert("n".into(), json!(m.total));
    summary.insert("fits".into(), json!(m.fits.iter().map(|f| json!({"series": m.series[f.series].id, "slope": f.slope, "intercept": f.intercept, "r": f.r})).collect::<Vec<_>>()));
    summary.insert(
        "plot".into(),
        json!([
            kit::round2(plot.x),
            kit::round2(plot.y),
            kit::round2(plot.x + plot.width),
            kit::round2(plot.y + plot.height)
        ]),
    );
    summary.insert("width".into(), kit::r2(w));
    summary.insert("height".into(), kit::r2(height));
    kit::decide(
        &mut decisions,
        "XS-LAYOUT",
        "a scatter plot: the y axis is the trained scale, the x axis trained from its own measure",
        Some("guided"),
    );
    Ok(PlacedChart {
        width: w,
        height,
        primitives: vec![kit::group(
            &["chart", "scatter", "categorical"],
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

/// The room the last x tick label needs right of the plot (it is centred on its tick).
fn right_pad(kit: &Kit, labels: &[String], em: f64) -> Result<f64> {
    let last = match labels.last() {
        Some(l) => kit.plain_ext("scale-label-formatted", &[], l)?.width / 2.,
        None => 0.,
    };
    Ok(em.max(last + 2.))
}
