//! Read-only diagnostics over semantic inputs and placed scenes. No chart selection or relayout.
pub mod style;
use chartgen_model::*;
use chartgen_scales::Explained;
use std::collections::BTreeSet;

/// The original's model-lint checks that read the scale and the spec's meaning, not the drawing
/// (`lint.breathing`, `lint.intervals`, `lint.trailing_zeros`, `lint.title_period`,
/// `lint.unit_phrase`). `chart_type` is the type drawn (None for several panels); `found` the
/// findings so far (a guided R2-05 on scale.magnitude replaces the trailing-zeros check).
pub fn semantic(
    spec: &Spec,
    data: &PreparedData,
    scale: Option<&Explained>,
    chart_type: Option<&str>,
    found: &[Finding],
) -> Vec<Finding> {
    let mut findings = vec![];
    let bumps = chart_type == Some("bumps");
    // the types drawn without a value scale (`__init__.NO_SCALE`), and the panels of lines with a
    // total panel, which have their own
    let scaled = scale.filter(|_| {
        !matches!(
            chart_type,
            None | Some(
                "pie" | "donut" | "treemap" | "table" | "heatmap" | "action_dots" | "line_total"
            )
        )
    });
    // the bar layout decides R2-32 itself (layout/bars.py sets ctx.value_labels)
    let bar_layout = matches!(
        chart_type,
        Some("bar" | "stacked_bar" | "stacked_bar_100" | "waterfall" | "funnel_bars")
    ) || chart_type.is_some_and(|t| {
        // the statistical charts draw summaries or samples, not one value per scale label: R2-32's
        // count of values against labels does not apply to them
        matches!(
            t,
            "box" | "strip" | "ecdf" | "scatter" | "dumbbell" | "interval_dot" | "interval_bar"
        )
    });
    // a bumps chart's rank scale (layout/bumps.py) has no breathing room, intervals or magnitude
    if let Some(x) = scaled.filter(|_| !bumps) {
        let (top, bottom) = chartgen_scales::breathing_room(x);
        for (place, room) in [("top", top), ("bottom", bottom)] {
            if let Some(room) = room
                && !(0.05..=0.25).contains(&room)
            {
                findings.push(Finding::new(
                    "R2-27",
                    "warn",
                    format!(
                        "{:.0}% of room at the {place} of the scale (5–25 % expected)",
                        room * 100.0
                    ),
                ));
            }
        }
        let n = x.scale.ticks.len().saturating_sub(1);
        if n > 8 {
            findings.push(Finding::new(
                "R2-34",
                "warn",
                format!(
                    "{n} intervals, {} scale labels (at most about eight)",
                    n + 1
                ),
            ));
        }
        if !bar_layout {
            let mut known = 0;
            if let Some(points) = &data.irregular {
                known += points
                    .iter()
                    .flatten()
                    .filter(|p| p.datum.value.is_some())
                    .count();
            } else {
                known += data
                    .series
                    .iter()
                    .flat_map(|s| &s.values)
                    .filter(|d| d.value.is_some())
                    .count();
                // comparison values drawn as series of the spec (transform.for_type)
                known += data
                    .comparisons
                    .iter()
                    .filter(|c| c.render == "series")
                    .flat_map(|c| &c.values)
                    .filter(|d| d.value.is_some())
                    .count();
            }
            if known < n + 1 {
                findings.push(Finding::new(
                    "R2-32",
                    "info",
                    format!(
                        "{known} values against {} scale labels: labelling the values directly would put fewer numbers on the chart (phase 2)",
                        n + 1
                    ),
                ));
            }
        }
        let magnitude_departure = found
            .iter()
            .any(|f| f.rule == "R2-05" && f.path.as_deref() == Some("scale.magnitude"));
        if !magnitude_departure
            && let Ok(labels) =
                chartgen_format::scale_labels_with_measure(spec, &data.measure, &x.scale)
        {
            for label in &labels.labels {
                let whole = label.split('.').next().unwrap_or("");
                let digits: String = whole
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                let zeros = digits.len() - digits.trim_end_matches('0').len();
                if zeros >= 4 {
                    findings.push(Finding::new(
                        "R2-05",
                        "warn",
                        format!(
                            "the label “{label}” carries {zeros} trailing zeros: state the magnitude once instead"
                        ),
                    ));
                    break;
                }
            }
        }
    }
    let title = spec.get("/title");
    if !title.is_null() {
        // a spec text: a string, Typst markup, or another native form's alternative words (or
        // key), as chartgen-policy reads it
        let text = |v: &serde_json::Value| {
            v.as_str().map(str::to_owned).or_else(|| {
                ["typst", "alternative", "latex", "tex"]
                    .iter()
                    .find_map(|k| v.get(*k).and_then(|t| t.as_str()))
                    .map(str::to_owned)
            })
        };
        let words: Vec<String> = [&title["text"], &title["descriptive"]]
            .into_iter()
            .filter_map(text)
            .collect();
        let words = words.join(" ");
        let year = words
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()));
        if spec.get("/data/x/kind") == "time" && !year {
            findings.push(Finding::new(
                "R2-12",
                "warn",
                "the title block does not give the period's year",
            ));
        }
        if matches!(title["kind"].as_str(), Some("insight" | "question"))
            && title["descriptive"].is_null()
        {
            findings.push(
                Finding::new(
                    "R2-12",
                    "warn",
                    "an insight or question title wants the descriptive line under it (what, by what, period; D-5)",
                )
                .at("title.descriptive"),
            );
        }
    }
    if scaled.is_some() {
        let unit = &data.measure["unit"];
        let field = |k: &str| unit[k].as_str().unwrap_or("");
        let suffix = field("suffix").trim();
        let noun = field("noun");
        let what = field("what");
        if noun.is_empty() && what.is_empty() && (suffix.is_empty() || suffix == "%") {
            findings.push(
                Finding::new(
                    "R2-11",
                    "warn",
                    "no unit phrase: give unit.noun or unit.what",
                )
                .at("data.measure.unit"),
            );
        } else if !noun.is_empty() && what.is_empty() && noun.split_whitespace().count() == 1 {
            findings.push(Finding::new("R2-11","info",format!("the unit phrase “{noun}” is one word; say what is counted, which subset and per what period")).at("data.measure.unit.noun"));
        }
    }
    findings
}

struct Label<'a> {
    bounds: Bounds,
    request: &'a TextRequest,
    joint: Option<&'a str>,
}
fn collect<'a>(primitives: &'a [Primitive], offset: Point, labels: &mut Vec<Label<'a>>) {
    for primitive in primitives {
        match primitive {
            Primitive::Text {
                position,
                request,
                metrics,
                anchor,
                meta,
            } => {
                let shift = match anchor.as_str() {
                    "middle" => metrics.width / 2.,
                    "end" => metrics.width,
                    _ => 0.,
                };
                labels.push(Label {
                    bounds: Bounds {
                        x: position.x + offset.x - shift,
                        y: position.y + offset.y,
                        width: metrics.width,
                        height: metrics.height,
                    },
                    request,
                    joint: meta.data.get("joint").map(|v| v.as_str()),
                });
            }
            Primitive::Group {
                children,
                translate,
                ..
            } => collect(
                children,
                Point {
                    x: offset.x + translate.x,
                    y: offset.y + translate.y,
                },
                labels,
            ),
            _ => {}
        }
    }
}
/// Inspect measured text boxes, including translations through nested panels.
/// A shared `meta.data.joint` exempts pieces of one deliberately composed label.
pub fn scene(scene: &Scene) -> Vec<Finding> {
    let mut labels = vec![];
    collect(&scene.primitives, Point { x: 0., y: 0. }, &mut labels);
    let boxes: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| figkit_scene::lint::ElementBounds {
            element: figkit_model::ElementId(i.to_string()),
            bounds: label.bounds.clone(),
        })
        .collect();
    let mut allowances = figkit_scene::lint::OverlapAllowances::default();
    for (i, a) in labels.iter().enumerate() {
        for (j, b) in labels.iter().enumerate().skip(i + 1) {
            if a.joint.is_some() && a.joint == b.joint {
                allowances
                    .pairs
                    .insert((boxes[i].element.clone(), boxes[j].element.clone()));
            }
        }
    }
    let observations = figkit_scene::lint::observe_boxes(
        &Bounds {
            x: 0.,
            y: 0.,
            width: scene.width,
            height: scene.height,
        },
        &boxes,
        &allowances,
        0.01,
    );
    let mut outside = BTreeSet::new();
    let mut overlaps = BTreeSet::new();
    for observation in observations {
        match observation {
            figkit_scene::lint::Observation::OutsideBounds { element } => {
                outside.insert(element);
            }
            figkit_scene::lint::Observation::Overlap { first, second } => {
                overlaps.insert((first, second));
            }
            _ => {}
        }
    }
    let mut findings = vec![];
    let mut styles = BTreeSet::new();
    for (i, a) in labels.iter().enumerate() {
        if a.request.role != "comment" {
            styles.insert((a.request.font_family.clone(), a.request.font_size.to_bits()));
        }
        if outside.contains(&boxes[i].element) {
            findings.push(
                Finding::new(
                    "CG-1",
                    "error",
                    format!(
                        "“{}” ({}) lies outside the chart",
                        a.request.content.readable(),
                        a.request.role
                    ),
                )
                .at(format!("text.{}", a.request.id)),
            );
        }
        for (j, b) in labels.iter().enumerate().skip(i + 1) {
            if overlaps.contains(&(boxes[i].element.clone(), boxes[j].element.clone())) {
                findings.push(Finding::new(
                    "CG-1",
                    "error",
                    format!(
                        "“{}” ({}) overlaps “{}” ({})",
                        a.request.content.readable(),
                        a.request.role,
                        b.request.content.readable(),
                        b.request.role
                    ),
                ));
            }
        }
        if a.request.role == "scale-label" {
            let label = a.request.content.readable();
            let integer = label.split('.').next().unwrap_or("");
            let digits: String = integer.chars().filter(char::is_ascii_digit).collect();
            let zeros = digits.len() - digits.trim_end_matches('0').len();
            if zeros >= 4 && !findings.iter().any(|f| f.rule == "R2-05") {
                findings.push(Finding::new("R2-05","warning",format!("the label “{label}” carries {zeros} trailing zeros: state the magnitude once instead")));
            }
        }
    }
    if styles.len() > 4 {
        findings.push(Finding::new(
            "R5-6",
            "warning",
            format!("{} text styles in use (at most four)", styles.len()),
        ));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    fn label(x: f64, y: f64) -> Primitive {
        Primitive::Text {
            position: Point { x, y },
            request: TextRequest {
                id: format!("{x}-{y}"),
                content: TextContent::plain("label"),
                role: "category-label".into(),
                font_family: "serif".into(),
                font_size: 10.,
                style: TextStyle::default(),
                max_width: None,
                context: "document".into(),
            },
            metrics: TextMetrics {
                width: 20.,
                height: 10.,
                baseline: None,
            },
            anchor: "middle".into(),
            meta: Metadata::default(),
        }
    }
    #[test]
    fn detects_overlap_after_panel_translation_and_outside_labels() {
        let scene = Scene {
            version: 1,
            width: 100.,
            height: 100.,
            title: None,
            description: None,
            background: "white".into(),
            comment: None,
            defs: vec![],
            primitives: vec![
                label(30., 20.),
                Primitive::Group {
                    children: vec![label(15., 5.)],
                    translate: Point { x: 15., y: 15. },
                    clip: None,
                    meta: Metadata::default(),
                },
                label(99., 95.),
            ],
        };
        let findings = super::scene(&scene);
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.message.contains("overlaps"))
                .count(),
            1
        );
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.message.contains("outside"))
                .count(),
            1
        );
    }
    #[test]
    fn semantic_year_check_uses_whole_digit_tokens() {
        let data = PreparedData {
            categories: vec![],
            x_kind: "time".into(),
            series: vec![],
            measure: serde_json::json!({}),
            irregular: None,
            comparisons: vec![],
        };
        let spec = Spec {
            raw: serde_json::json!({"title":{"text":"Orders in 2025"},"data":{"x":{"kind":"time"}}}),
        };
        assert!(
            !semantic(&spec, &data, None, Some("line"), &[])
                .iter()
                .any(|f| f.rule == "R2-12")
        );
        let spec = Spec {
            raw: serde_json::json!({"title":{"text":"Orders: 20250"},"data":{"x":{"kind":"time"}}}),
        };
        assert!(
            semantic(&spec, &data, None, Some("line"), &[])
                .iter()
                .any(|f| f.rule == "R2-12")
        );
        // paper ISSUES #9: a LaTeX-native title counts by its alternative words
        let spec = Spec {
            raw: serde_json::json!({"title":{"text":{"latex":"nitrateTitle","alternative":"Nitrate, 2024"}},"data":{"x":{"kind":"time"}}}),
        };
        assert!(
            !semantic(&spec, &data, None, Some("line"), &[])
                .iter()
                .any(|f| f.rule == "R2-12")
        );
    }
}
