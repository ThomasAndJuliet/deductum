//! Group opacity: a group composited as one layer at its opacity, so overlapping children (two
//! cells' shared edge) do not darken one another.
use figkit_model::{ElementMetadata, Figure, FigureMetadata, Limits, Paint, PathCommand, Point, Primitive, Resources, Size};
use figkit_render::{Backend, render};

fn line(x: f64) -> Primitive {
    Primitive::Path {
        commands: vec![PathCommand::MoveTo { to: Point { x, y: 0. } }, PathCommand::LineTo { to: Point { x, y: 10. } }],
        paint: Paint { fill: None, stroke: Some("#000000".into()), stroke_width: 1., opacity: 1. },
        metadata: ElementMetadata::default(),
    }
}

fn figure(opacity: f64) -> Figure {
    Figure {
        version: 2,
        size: Size { width: 20., height: 10. },
        metadata: FigureMetadata::default(),
        background: "#ffffff".into(),
        primitives: vec![Primitive::Group {
            children: vec![line(5.), line(5.)],
            translate: Point { x: 0., y: 0. },
            clip: None,
            opacity,
            metadata: ElementMetadata::default(),
        }],
        resources: Resources::default(),
    }
}

fn out(opacity: f64, backend: Backend) -> Result<String, String> {
    let scene = figkit_scene::prepare(&figure(opacity), &Limits::default()).map_err(|e| format!("{e:?}"))?;
    render(&scene, backend).map_err(|e| format!("{e:?}"))
}

#[test]
fn svg_writes_the_groups_opacity() {
    let svg = out(0.4, Backend::Svg).unwrap();
    assert!(svg.contains("<g transform=\"translate(0 0)\" opacity=\"0.4\""), "{svg}");
}

#[test]
fn an_opaque_group_is_drawn_as_before() {
    let svg = out(1., Backend::Svg).unwrap();
    let group = svg.lines().find(|l| l.starts_with("<g ")).unwrap();
    assert!(!group.contains("opacity"), "{group}");
}

#[test]
fn pgf_draws_a_transparency_group() {
    let pgf = out(0.4, Backend::Pgf).unwrap();
    assert!(pgf.contains("\\pgfsetfillopacity{0.4}\\pgfsetstrokeopacity{0.4}\\begin{pgftransparencygroup}"), "{pgf}");
    assert!(pgf.contains("\\end{pgftransparencygroup}"), "{pgf}");
}

#[test]
fn layer_plans_refuse_a_translucent_group() {
    for backend in [Backend::Typst, Backend::Browser] {
        let e = out(0.4, backend).unwrap_err();
        assert!(e.contains("group opacity"), "{backend:?}: {e}");
        assert!(out(1., backend).is_ok(), "{backend:?}: an opaque group still renders");
    }
}

#[test]
fn opacity_outside_zero_to_one_is_refused() {
    assert!(out(1.5, Backend::Svg).is_err());
    assert!(out(f64::NAN, Backend::Svg).is_err());
}
