use chartgen_model::{
    Bounds, Metadata, Paint, Point, Primitive, Scene, TextContent, TextMetrics, TextRequest,
    TextStyle,
};
use chartgen_render::{PgfRenderer, Renderer};
fn main() {
    let scene = Scene {
        version: 1,
        width: 150.,
        height: 90.,
        title: None,
        description: None,
        background: "#ffffff".into(),
        comment: None,
        defs: vec![],
        primitives: vec![Primitive::Group {
            translate: Point { x: 12., y: 10. },
            clip: Some(Bounds {
                x: 0.,
                y: 0.,
                width: 100.,
                height: 70.,
            }),
            meta: Metadata::default(),
            children: vec![
                Primitive::Rect {
                    bounds: Bounds {
                        x: 0.,
                        y: 0.,
                        width: 55.,
                        height: 25.,
                    },
                    paint: Paint {
                        fill: Some("#123456".into()),
                        stroke: Some("#abcdef".into()),
                        stroke_width: 1.,
                        opacity: 0.8,
                    },
                    meta: Metadata::default(),
                },
                Primitive::Line {
                    from: Point { x: 0., y: 30. },
                    to: Point { x: 70., y: 30. },
                    paint: Paint {
                        fill: None,
                        stroke: Some("black".into()),
                        stroke_width: 1.,
                        opacity: 1.,
                    },
                    meta: Metadata::default(),
                },
                Primitive::Text {
                    position: Point { x: 3., y: 35. },
                    request: TextRequest {
                        id: "plain".into(),
                        content: TextContent::plain("Price: 50% & $1"),
                        role: "label".into(),
                        font_family: "serif".into(),
                        font_size: 10.,
                        style: TextStyle::default(),
                        max_width: None,
                        context: "latex".into(),
                    },
                    metrics: TextMetrics {
                        width: 80.,
                        height: 11.,
                        baseline: Some(8.),
                    },
                    anchor: "start".into(),
                    meta: Metadata::default(),
                },
                Primitive::Text {
                    position: Point { x: 3., y: 52. },
                    request: TextRequest {
                        id: "native".into(),
                        content: TextContent::Native {
                            host: "latex".into(),
                            key: "rich".into(),
                            alternative: None,
                        },
                        role: "label".into(),
                        font_family: "serif".into(),
                        font_size: 10.,
                        style: TextStyle::default(),
                        max_width: None,
                        context: "latex".into(),
                    },
                    metrics: TextMetrics {
                        width: 80.,
                        height: 11.,
                        baseline: Some(8.),
                    },
                    anchor: "start".into(),
                    meta: Metadata::default(),
                },
            ],
        }],
    };
    print!("{}", PgfRenderer.render(&scene).unwrap());
}
