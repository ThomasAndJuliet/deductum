//! Host-measured comments anchored to placed marks.
use super::*;
use serde_json::Value;

fn rows(plan: &SemanticPlan) -> Result<Vec<&Value>> {
    match plan.spec.raw.get("comments") {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(items)) => Ok(items.iter().collect()),
        _ => Err(err("comments must be an array").at("comments")),
    }
}

pub fn validate(plan: &SemanticPlan) -> Result<()> {
    let rows = rows(plan)?;
    let title_link = plan
        .spec
        .raw
        .pointer("/title/link")
        .is_some_and(|v| !v.is_null());
    if (!rows.is_empty() || title_link)
        && !matches!(
            plan.chart_type,
            ChartType::Bar
                | ChartType::Dot
                | ChartType::Line
                | ChartType::Step
                | ChartType::ThinBar
        )
    {
        return Err(err(format!(
            "comments need dedicated {} anchor placement",
            plan.chart_type.as_str()
        ))
        .at("comments"));
    }
    for (i, row) in rows.iter().enumerate() {
        if !row.is_object() {
            return Err(err(format!("comments[{i}] must be an object")));
        }
        let Some(text) = row.get("text") else {
            return Err(err(format!("comments[{i}] needs text")));
        };
        if guides::content(text)?.is_none() {
            return Err(err(format!("comments[{i}] has empty text")));
        }
        if let Some(placement) = row.get("placement").and_then(Value::as_str) {
            if !matches!(placement, "auto" | "inside" | "right" | "below") {
                return Err(err(format!(
                    "comments[{i}] placement {placement} is unsupported"
                )));
            }
        }
        if let Some(anchors) = row.get("anchors") {
            let Some(anchors) = anchors.as_array() else {
                return Err(err(format!("comments[{i}].anchors must be an array")));
            };
            for reference in anchors {
                if reference.get("category").and_then(Value::as_str).is_none()
                    && reference.get("x").and_then(Value::as_str).is_none()
                {
                    return Err(err(format!(
                        "comments[{i}] anchor needs a category or scalar x"
                    )));
                }
            }
        }
    }
    Ok(())
}

pub fn contents(plan: &SemanticPlan) -> Result<Vec<TextContent>> {
    rows(plan)?
        .iter()
        .map(|row| guides::content(&row["text"])?.ok_or_else(|| err("comment has no text")))
        .collect()
}

fn bounds(p: &Primitive) -> Option<Bounds> {
    match p {
        Primitive::Rect { bounds, .. } => Some(bounds.clone()),
        Primitive::Circle { center, radius, .. } => Some(Bounds {
            x: center.x - radius,
            y: center.y - radius,
            width: radius * 2.,
            height: radius * 2.,
        }),
        Primitive::Text { .. } => text_box(p),
        _ => None,
    }
}

fn point_of(p: &Primitive) -> Option<Point> {
    match p {
        Primitive::Rect { bounds, .. } => Some(Point {
            x: bounds.x + bounds.width / 2.,
            y: bounds.y + bounds.height / 2.,
        }),
        Primitive::Circle { center, .. } => Some(center.clone()),
        _ => None,
    }
}

fn meta_of(p: &Primitive) -> Option<&Metadata> {
    match p {
        Primitive::Rect { meta, .. }
        | Primitive::Circle { meta, .. }
        | Primitive::Path { meta, .. } => Some(meta),
        _ => None,
    }
}

fn anchors(row: &Value, data: &PreparedData, primitives: &[Primitive]) -> Result<Vec<Point>> {
    let mut points = Vec::new();
    for reference in row["anchors"].as_array().into_iter().flatten() {
        let cat = reference
            .get("category")
            .and_then(Value::as_str)
            .or_else(|| reference.get("x").and_then(Value::as_str))
            .ok_or_else(|| err("comment anchor needs a category or x"))?;
        let category = data
            .categories
            .iter()
            .position(|name| name == cat)
            .ok_or_else(|| err(format!("comment anchor names unknown category {cat}")))?;
        let series = reference.get("series").and_then(Value::as_str);
        if let Some(id) = series {
            if !data.series.iter().any(|s| s.id == id) {
                return Err(err(format!("comment anchor names unknown series {id}")));
            }
        }
        let mut matched = false;
        for p in primitives {
            let Some(meta) = meta_of(p) else { continue };
            if meta.role != "data-mark" {
                continue;
            }
            let Some(id) = &meta.id else {
                continue;
            };
            if data.series.iter().any(|s| {
                series.is_none_or(|given| given == s.id) && id == &format!("{}-{category}", s.id)
            }) {
                if let Some(point) = point_of(p) {
                    points.push(point);
                    matched = true;
                }
            }
        }
        if !matched {
            return Err(err(format!("comment anchor {cat} has no drawable mark")));
        }
    }
    Ok(points)
}

fn clear(box_: &Bounds, obstacles: &[Bounds], chart_width: f64, chart_height: f64) -> bool {
    box_.x >= 4.
        && box_.y >= 4.
        && box_.x + box_.width <= chart_width - 4.
        && box_.y + box_.height <= chart_height - 4.
        && obstacles.iter().all(|b| {
            !boxes_overlap(
                &Bounds {
                    x: box_.x - 4.,
                    y: box_.y - 4.,
                    width: box_.width + 8.,
                    height: box_.height + 8.,
                },
                b,
            )
        })
}

fn connector(start: Point, end: Point, theme: &Theme, id: usize) -> Vec<Primitive> {
    let cx = (start.x + end.x) / 2.;
    let cy = (start.y + end.y) / 2. - 7.;
    let points = (0..=12)
        .map(|i| {
            let t = i as f64 / 12.;
            let u = 1. - t;
            Point {
                x: u * u * start.x + 2. * u * t * cx + t * t * end.x,
                y: u * u * start.y + 2. * u * t * cy + t * t * end.y,
            }
        })
        .collect::<Vec<_>>();
    let before = &points[11];
    let dx = end.x - before.x;
    let dy = end.y - before.y;
    let len = (dx * dx + dy * dy).sqrt().max(1.);
    let ux = dx / len;
    let uy = dy / len;
    let left = Point {
        x: end.x - 5. * ux - 2.5 * uy,
        y: end.y - 5. * uy + 2.5 * ux,
    };
    let right = Point {
        x: end.x - 5. * ux + 2.5 * uy,
        y: end.y - 5. * uy - 2.5 * ux,
    };
    vec![
        Primitive::Path {
            points,
            closed: false,
            paint: paint(None, Some(theme.grid.clone()), 0.8),
            meta: meta("comment-arrow", format!("comment-arrow-{id}")),
        },
        Primitive::Path {
            points: vec![left, end.clone(), right],
            closed: false,
            paint: paint(None, Some(theme.grid.clone()), 0.8),
            meta: meta("comment-arrowhead", format!("comment-arrowhead-{id}")),
        },
    ]
}

pub fn place(
    plan: &SemanticPlan,
    data: &PreparedData,
    theme: &Theme,
    c: &LayoutConstraints,
    requests: &[TextRequest],
    measurements: &Measurements,
    primitives: &mut Vec<Primitive>,
    height: &mut f64,
    _findings: &mut Vec<Finding>,
) -> Result<()> {
    for (i, row) in rows(plan)?.iter().enumerate() {
        let content = guides::content(&row["text"])?.ok_or_else(|| err("comment has no text"))?;
        let request = requests
            .iter()
            .find(|r| r.role == "comment" && r.content == content)
            .ok_or_else(|| err("missing comment request"))?;
        let metrics = measurements
            .entries
            .get(&request.id)
            .ok_or_else(|| err("missing comment metrics"))?;
        let targets = anchors(row, data, primitives)?;
        let placement = row
            .get("placement")
            .and_then(Value::as_str)
            .unwrap_or("auto");
        let obstacles = primitives.iter().filter_map(bounds).collect::<Vec<_>>();
        let mid = if targets.is_empty() {
            Point {
                x: c.width * 0.72,
                y: *height * 0.55,
            }
        } else {
            Point {
                x: targets.iter().map(|p| p.x).sum::<f64>() / targets.len() as f64,
                y: targets.iter().map(|p| p.y).sum::<f64>() / targets.len() as f64,
            }
        };
        let w = metrics.width;
        let h = metrics.height;
        let mut chosen = None;
        if placement != "below" && placement != "right" {
            let candidates = [
                (mid.x + 16., mid.y - h - 22.),
                (mid.x - w - 16., mid.y - h - 22.),
                (mid.x + 16., mid.y + 22.),
                (mid.x - w - 16., mid.y + 22.),
            ];
            for (x, y) in candidates {
                let b = Bounds {
                    x,
                    y,
                    width: w,
                    height: h,
                };
                if clear(&b, &obstacles, c.width, *height) {
                    chosen = Some(b);
                    break;
                }
            }
            if chosen.is_none() && placement == "inside" {
                return Err(err(format!("comments[{i}] has no empty inside placement")));
            }
        }
        if chosen.is_none() && placement != "below" {
            let x = c.width - w - 8.;
            for j in 0..12 {
                let y = 12. + j as f64 * (h + 8.);
                let b = Bounds {
                    x,
                    y,
                    width: w,
                    height: h,
                };
                if clear(&b, &obstacles, c.width, *height) {
                    chosen = Some(b);
                    break;
                }
            }
        }
        let b = if let Some(b) = chosen {
            b
        } else {
            let x = (mid.x - w / 2.).clamp(8., c.width - w - 8.);
            let y = *height + 10.;
            *height = y + h + 8.;
            Bounds {
                x,
                y,
                width: w,
                height: h,
            }
        };
        if b.x < 0. || b.x + b.width > c.width {
            return Err(err(format!("comments[{i}] is wider than the chart")));
        }
        if row.get("arrow").and_then(Value::as_bool).unwrap_or(true) {
            for target in &targets {
                let start = Point {
                    x: (target.x).clamp(b.x, b.x + b.width),
                    y: if target.y < b.y { b.y } else { b.y + b.height },
                };
                primitives.extend(connector(start, target.clone(), theme, i));
            }
        }
        primitives.push(Primitive::Text {
            position: Point { x: b.x, y: b.y },
            request: request.clone(),
            metrics: metrics.clone(),
            anchor: "start".into(),
            meta: Metadata {
                id: Some(format!("comment-{i}")),
                role: "comment".into(),
                ..Default::default()
            },
        });
        // where a comment went is a decision in the original (RP3-7), never a finding
    }
    if let Some(link) = plan
        .spec
        .raw
        .pointer("/title/link")
        .filter(|v| !v.is_null())
    {
        let title = primitives
            .iter()
            .find_map(|p| {
                if let Primitive::Text { request, .. } = p {
                    (request.role == "title").then(|| text_box(p)).flatten()
                } else {
                    None
                }
            })
            .ok_or_else(|| err("title.link needs a measured title"))?;
        let ref_row = json!({"anchors":[link]});
        let targets = anchors(&ref_row, data, primitives)?;
        if targets.is_empty() {
            return Err(err("title.link has no drawable mark"));
        }
        let target = targets
            .into_iter()
            .min_by(|a, b| {
                let sx = title.x + title.width;
                let sy = title.y + title.height / 2.;
                let da = (a.x - sx).powi(2) + (a.y - sy).powi(2);
                let db = (b.x - sx).powi(2) + (b.y - sy).powi(2);
                da.total_cmp(&db)
            })
            .unwrap();
        let start = Point {
            x: title.x + title.width + 3.,
            y: title.y + title.height / 2.,
        };
        let mut paths = connector(start, target, theme, 0);
        for (j, path) in paths.iter_mut().enumerate() {
            if let Primitive::Path { meta: metadata, .. } = path {
                *metadata = meta(
                    if j == 0 {
                        "title-link"
                    } else {
                        "title-link-arrowhead"
                    },
                    format!("title-link-{j}"),
                );
            }
        }
        primitives.extend(paths);
        // the title connector drawn is a decision in the original (6.12), never a finding
    }
    Ok(())
}
