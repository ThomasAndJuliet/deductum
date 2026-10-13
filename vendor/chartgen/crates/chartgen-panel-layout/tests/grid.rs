use chartgen_layout::{LayoutEngine, guides};
use chartgen_model::*;
use chartgen_panel_layout::{CompiledPanel, PanelLayout};
use chartgen_panels::{Arrangement, Panel};
use serde_json::json;
use std::collections::BTreeMap;

struct Child;
impl LayoutEngine for Child {
    fn solve(
        &self,
        plan: &SemanticPlan,
        _: &PreparedData,
        _: Option<&TrainedScale>,
        theme: &Theme,
        constraints: &LayoutConstraints,
        measurements: &Measurements,
    ) -> Result<LayoutStep> {
        assert!(plan.spec.get("/title").is_null());
        assert!(plan.spec.get("/notes").is_null());
        let request = guides::request(
            "category-label",
            TextContent::plain("same label"),
            theme.font_size,
            Some(constraints.width - 20.0),
            theme,
            constraints,
        )?;
        if !measurements.entries.contains_key(&request.id) {
            return Ok(LayoutStep::NeedMeasurements {
                requests: vec![request],
                round: 1,
            });
        }
        let height = constraints.height.unwrap_or(100.0);
        Ok(LayoutStep::Complete {
            chart: PlacedChart {
                decisions: vec![],
                width: constraints.width,
                height,
                primitives: vec![Primitive::Rect {
                    bounds: Bounds {
                        x: 1.0,
                        y: 2.0,
                        width: 10.0,
                        height: 10.0,
                    },
                    paint: Paint::default(),
                    meta: Metadata {
                        id: Some("mark".into()),
                        role: "data-mark".into(),
                        ..Default::default()
                    },
                }],
                findings: vec![],
                summary: BTreeMap::new(),
            },
        })
    }
}
fn plan(title: serde_json::Value) -> SemanticPlan {
    SemanticPlan {
        spec: Spec {
            raw: json!({"title":{"text":title},"notes":[{"text":"outer note"}]}),
        },
        chart_type: ChartType::Bar,
        orientation: Orientation::Vertical,
        order: vec![],
        layers: vec![],
        coordinates: "cartesian".into(),
        colors: ColorPlan::default(),
    }
}
fn data() -> PreparedData {
    PreparedData {
        categories: vec![],
        x_kind: "nominal".into(),
        series: vec![],
        measure: json!({}),
        irregular: None,
        comparisons: vec![],
    }
}
fn grid() -> Vec<CompiledPanel> {
    (0..2)
        .flat_map(|row| {
            (0..2).map(move |column| {
                let p = plan(json!("outer"));
                CompiledPanel {
                    panel: Panel {
                        row,
                        column,
                        row_value: Some(format!("R{row}")),
                        column_value: Some(format!("C{column}")),
                        title: if row == 0 && column == 0 {
                            json!({"typst":"#smallcaps[Panel]","alternative":"Panel"})
                        } else {
                            json!(format!("Panel {row}{column}"))
                        },
                        series_indices: vec![],
                        spec: p.spec.clone(),
                        data: data(),
                        kind: "multiples".into(),
                        arrangement: Arrangement {
                            rows: Some("r".into()),
                            columns: Some("c".into()),
                            within: vec![],
                        },
                        scale_mode: "shared".into(),
                        rows: 2,
                        columns: 2,
                    },
                    plan: p,
                    data: data(),
                    scale: None,
                }
            })
        })
        .collect()
}
fn measured(requests: Vec<TextRequest>) -> Measurements {
    let context = "test".to_string();
    let entries = requests
        .into_iter()
        .map(|r| {
            (
                r.id,
                TextMetrics {
                    width: 25.0,
                    height: 12.0,
                    baseline: Some(9.0),
                },
            )
        })
        .collect();
    Measurements { context, entries }
}
#[test]
fn two_by_two_grid_aggregates_measurements_and_translates_children() {
    let child = Child;
    let layout = PanelLayout {
        engine: &child,
        panels: grid(),
    };
    let outer = plan(json!("Outer title"));
    let constraints = LayoutConstraints {
        width: 420.0,
        height: None,
        measurement_context: "test".into(),
        max_rounds: 8,
        hooks: None,
    };
    let first = layout
        .solve(
            &outer,
            &data(),
            None,
            &Theme::default(),
            &constraints,
            &Measurements::default(),
        )
        .unwrap();
    let requests = match first {
        LayoutStep::NeedMeasurements { requests, .. } => requests,
        _ => panic!("expected requests"),
    };
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.role == "category-label")
            .count(),
        1
    );
    assert!(
        requests
            .iter()
            .any(|r| matches!(&r.content, TextContent::Native { host, .. } if host == "typst"))
    );
    let result = layout
        .solve(
            &outer,
            &data(),
            None,
            &Theme::default(),
            &constraints,
            &measured(requests),
        )
        .unwrap();
    let chart = match result {
        LayoutStep::Complete { chart } => chart,
        _ => panic!("expected chart"),
    };
    assert_eq!(chart.summary["panel_rows"], 2);
    assert_eq!(chart.summary["panel_columns"], 2);
    let groups: Vec<_> = chart
        .primitives
        .iter()
        .filter_map(|p| {
            if let Primitive::Group {
                translate,
                children,
                meta,
                ..
            } = p
            {
                Some((translate, children, meta))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(groups.len(), 4);
    assert!(groups[0].0.x < groups[1].0.x && groups[0].0.y == groups[1].0.y);
    assert!(groups[2].0.y > groups[0].0.y && groups[2].0.x == groups[0].0.x);
    let ids: Vec<_> = groups
        .iter()
        .map(|(_, children, _)| match &children[0] {
            Primitive::Rect { meta, .. } => meta.id.as_ref().unwrap().clone(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        ids.len(),
        ids.iter().collect::<std::collections::BTreeSet<_>>().len()
    );
    assert!(ids[0].starts_with("panel-r0-c0"));
    assert!(chart.height > groups[2].0.y + 100.0);
}

#[test]
fn fixed_height_reserves_title_and_headings() {
    let child = Child;
    let layout = PanelLayout {
        engine: &child,
        panels: grid(),
    };
    let outer = plan(json!("Outer title"));
    let constraints = LayoutConstraints {
        width: 420.0,
        height: Some(430.0),
        measurement_context: "test".into(),
        max_rounds: 8,
        hooks: None,
    };
    let requests = match layout
        .solve(
            &outer,
            &data(),
            None,
            &Theme::default(),
            &constraints,
            &Measurements::default(),
        )
        .unwrap()
    {
        LayoutStep::NeedMeasurements { requests, .. } => requests,
        _ => panic!(),
    };
    let chart = match layout
        .solve(
            &outer,
            &data(),
            None,
            &Theme::default(),
            &constraints,
            &measured(requests),
        )
        .unwrap()
    {
        LayoutStep::Complete { chart } => chart,
        _ => panic!(),
    };
    assert_eq!(chart.height, 430.0);
    let groups: Vec<_> = chart
        .primitives
        .iter()
        .filter_map(|p| {
            if let Primitive::Group {
                translate, clip, ..
            } = p
            {
                Some((translate, clip.as_ref().unwrap()))
            } else {
                None
            }
        })
        .collect();
    assert!(groups[0].0.y > 12.0 + 12.0);
    assert!(groups[2].0.y > groups[0].0.y + groups[0].1.height);
}

#[test]
fn narrow_grid_is_rejected_before_inner_layout() {
    let child = Child;
    let layout = PanelLayout {
        engine: &child,
        panels: grid(),
    };
    let constraints = LayoutConstraints {
        width: 220.0,
        height: None,
        measurement_context: "test".into(),
        max_rounds: 8,
        hooks: None,
    };
    let err = layout
        .solve(
            &plan(json!("Title")),
            &data(),
            None,
            &Theme::default(),
            &constraints,
            &Measurements::default(),
        )
        .unwrap_err();
    assert!(err.message.contains("too narrow"));
}

#[test]
fn default_child_layouts_fit_composed_grid() {
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/basic.json")).unwrap();
    let mut panels = grid();
    let datum = |n| Datum {
        value: Some(Decimal::from(n)),
        status: "known".into(),
        certainty: "actual".into(),
        complete: true,
        note: None,
        limit: None,
    };
    let data = PreparedData {
        categories: vec!["East".into(), "West".into()],
        x_kind: "nominal".into(),
        series: vec![Series {
            id: "headcount".into(),
            name: "Headcount".into(),
            values: vec![datum(20), datum(35)],
        }],
        measure: raw["data"]["measure"].clone(),
        irregular: None,
        comparisons: vec![],
    };
    let scale = TrainedScale {
        lo: Decimal::ZERO,
        hi: Decimal::from(40),
        step: Decimal::from(10),
        ticks: (0..=4).map(|i| Decimal::from(i * 10)).collect(),
        mode: "linear".into(),
    };
    for compiled in &mut panels {
        compiled.plan.spec.raw = raw.clone();
        compiled.plan.order = vec![0, 1];
        compiled.data = data.clone();
        compiled.scale = Some(scale.clone());
    }
    let child = chartgen_layout::DefaultLayout;
    let layout = PanelLayout {
        engine: &child,
        panels,
    };
    let constraints = LayoutConstraints {
        width: 440.0,
        height: None,
        measurement_context: "test".into(),
        max_rounds: 8,
        hooks: None,
    };
    let first = layout
        .solve(
            &plan(json!("Outer title")),
            &data,
            None,
            &Theme::default(),
            &constraints,
            &Measurements::default(),
        )
        .unwrap();
    let requests = match first {
        LayoutStep::NeedMeasurements { requests, .. } => requests,
        _ => panic!(),
    };
    let second = layout
        .solve(
            &plan(json!("Outer title")),
            &data,
            None,
            &Theme::default(),
            &constraints,
            &measured(requests),
        )
        .unwrap();
    let chart = match second {
        LayoutStep::Complete { chart } => chart,
        _ => panic!("expected complete"),
    };
    assert_eq!(
        chart
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Group { .. }))
            .count(),
        4
    );
}
