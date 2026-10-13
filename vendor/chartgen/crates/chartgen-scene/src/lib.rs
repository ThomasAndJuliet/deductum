//! Validation and lowering of an already placed chart. No relayout occurs here.
use chartgen_model::*;

/// Validate chart version-1 DTOs through the chart-free legacy validator.
pub fn validate(scene: &Scene) -> Result<()> {
    figkit_scene::legacy::validate(&scene.to_legacy())
}
/// Preserve authored primitive order. Renderers consume the scene without changing geometry.
pub fn lower(plan: &SemanticPlan, placed: &PlacedChart, theme: &Theme) -> Result<Scene> {
    let title = plan
        .spec
        .raw
        .pointer("/title/text")
        .and_then(|v| {
            v.as_str()
                .or_else(|| v.get("plain").and_then(|x| x.as_str()))
        })
        .or_else(|| plan.spec.raw.get("title").and_then(|v| v.as_str()))
        .map(str::to_owned);
    // the figure's <desc> is the insight's important point (the original's
    // `fig.desc = spec.insight.important`), never the descriptive line
    let description = plan
        .spec
        .raw
        .pointer("/insight/important")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let scene = Scene {
        version: 1,
        width: placed.width,
        height: placed.height,
        title,
        description,
        background: theme.background.clone(),
        primitives: placed.primitives.clone(),
        comment: None,
        defs: vec![],
    };
    validate(&scene)?;
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn plan() -> SemanticPlan {
        SemanticPlan {
            spec: Spec {
                raw: json!({"title":{"text":"Test"}}),
            },
            chart_type: ChartType::Bar,
            orientation: Orientation::Vertical,
            order: vec![],
            layers: vec![],
            coordinates: "cartesian".into(),
            colors: ColorPlan::default(),
        }
    }
    #[test]
    fn validates_geometry_and_preserves_order() {
        let first = Primitive::Rect {
            bounds: Bounds {
                x: 0.,
                y: 0.,
                width: 10.,
                height: 8.,
            },
            paint: Paint {
                fill: Some("#123456".into()),
                stroke: None,
                stroke_width: 0.,
                opacity: 1.,
            },
            meta: Metadata {
                role: "grid".into(),
                ..Default::default()
            },
        };
        let second = Primitive::Line {
            from: Point { x: 1., y: 2. },
            to: Point { x: 5., y: 6. },
            paint: Paint::default(),
            meta: Metadata {
                role: "mark".into(),
                ..Default::default()
            },
        };
        let mut placed = PlacedChart {
            decisions: vec![],
            width: 100.,
            height: 80.,
            primitives: vec![first, second],
            findings: vec![],
            summary: Default::default(),
        };
        let scene = lower(&plan(), &placed, &Theme::default()).unwrap();
        assert_eq!(scene.title.as_deref(), Some("Test"));
        assert!(matches!(scene.primitives[0], Primitive::Rect { .. }));
        if let Primitive::Line { to, .. } = &mut placed.primitives[1] {
            to.x = f64::NAN;
        }
        assert!(lower(&plan(), &placed, &Theme::default()).is_err());
    }
}
