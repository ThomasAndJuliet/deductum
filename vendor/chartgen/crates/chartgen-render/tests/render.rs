use chartgen_model::{
    Bounds, Metadata, Paint, Point, Primitive, Scene, TextContent, TextMetrics, TextRequest,
    TextStyle,
};
use chartgen_render::{
    CompatibilitySvgRenderer, PgfRenderer, Renderer, SvgRenderer, TypstRenderer,
};

fn base(primitives: Vec<Primitive>) -> Scene {
    Scene {
        version: 1,
        width: 100.,
        height: 60.,
        title: Some("A&B".into()),
        description: None,
        background: "#ffffff".into(),
        comment: None,
        defs: vec![],
        primitives,
    }
}
fn paint() -> Paint {
    Paint {
        fill: Some("#123456".into()),
        stroke: Some("#abcdef".into()),
        stroke_width: 2.,
        opacity: 0.5,
    }
}
fn text(content: TextContent) -> Primitive {
    Primitive::Text {
        position: Point { x: 20., y: 10. },
        request: TextRequest {
            id: "t".into(),
            content,
            role: "label".into(),
            font_family: "serif".into(),
            font_size: 9.,
            style: TextStyle::default(),
            max_width: None,
            context: "test".into(),
        },
        metrics: TextMetrics {
            width: 12.,
            height: 10.,
            baseline: Some(7.),
        },
        anchor: "middle".into(),
        meta: Metadata::default(),
    }
}
#[test]
fn text_style_survives_every_backend() {
    let mut label = text(TextContent::plain("Styled"));
    if let Primitive::Text { request, .. } = &mut label {
        request.style = TextStyle {
            fill: Some("#3f63b0".into()),
            weight: "bold".into(),
            font_style: "italic".into(),
        };
    }
    let scene = base(vec![label]);
    let svg = SvgRenderer.render(&scene).unwrap();
    assert!(svg.contains("font-weight=\"bold\""));
    assert!(svg.contains("font-style=\"italic\""));
    assert!(svg.contains("fill=\"#3f63b0\""));
    let pgf = PgfRenderer.render(&scene).unwrap();
    assert!(pgf.contains("\\bfseries"));
    assert!(pgf.contains("\\itshape"));
    assert!(pgf.contains("\\color[RGB]{63,99,176}"));
    let plan: serde_json::Value =
        serde_json::from_str(&TypstRenderer.render(&scene).unwrap()).unwrap();
    let styled = plan["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "text")
        .unwrap();
    assert_eq!(styled["style"]["weight"], "bold");
    assert_eq!(styled["style"]["font_style"], "italic");
    assert_eq!(styled["style"]["fill"], "#3f63b0");
}
#[test]
fn svg_escapes_and_preserves_paint_and_group_clip() {
    let scene = base(vec![Primitive::Group {
        children: vec![
            Primitive::Rect {
                bounds: Bounds {
                    x: 1.,
                    y: 2.,
                    width: 10.,
                    height: 20.,
                },
                paint: paint(),
                meta: Metadata {
                    id: Some("r&1".into()),
                    role: "bar".into(),
                    classes: vec!["mark".into()],
                    ..Default::default()
                },
            },
            text(TextContent::plain("A<&\"'")),
        ],
        translate: Point { x: 3., y: 4. },
        clip: Some(Bounds {
            x: 0.,
            y: 0.,
            width: 40.,
            height: 20.,
        }),
        meta: Metadata::default(),
    }]);
    let s = SvgRenderer.render(&scene).unwrap();
    assert!(s.contains("<title>A&amp;B</title>"));
    assert!(s.contains(
        "fill=\"#123456\" stroke=\"#abcdef\" stroke-width=\"2.000000\" opacity=\"0.500000\""
    ));
    assert!(s.contains("clip-path=\"url(#chartgen-clip-0)\""));
    assert!(s.contains("A&lt;&amp;&quot;&apos;"));
    assert!(s.contains("id=\"r&amp;1\""));
    let c = CompatibilitySvgRenderer::default().render(&scene).unwrap();
    // The role is the element's last class, as in the original vocabulary; no data-role.
    assert!(c.contains(
        "<rect class=\"mark bar\" id=\"r&amp;1\" x=\"1\" y=\"2\" width=\"10\" height=\"20\""
    ));
    assert!(!c.contains("data-role"));
}
#[test]
fn missing_native_alternative_is_error_in_web_svg() {
    let scene = base(vec![text(TextContent::Native {
        host: "typst".into(),
        key: "k".into(),
        alternative: None,
    })]);
    assert!(
        SvgRenderer
            .render(&scene)
            .unwrap_err()
            .message
            .contains("no SVG alternative")
    );
}
#[test]
fn document_backends_keep_native_host_and_order() {
    let scene = base(vec![
        Primitive::Rect {
            bounds: Bounds {
                x: 0.,
                y: 0.,
                width: 3.,
                height: 3.,
            },
            paint: paint(),
            meta: Metadata::default(),
        },
        text(TextContent::Native {
            host: "typst".into(),
            key: "rich".into(),
            alternative: Some("alt".into()),
        }),
        Primitive::Line {
            from: Point { x: 0., y: 0. },
            to: Point { x: 1., y: 2. },
            paint: paint(),
            meta: Metadata::default(),
        },
    ]);
    let plan: serde_json::Value =
        serde_json::from_str(&TypstRenderer.render(&scene).unwrap()).unwrap();
    assert_eq!(plan["layers"][0]["kind"], "svg");
    assert_eq!(plan["layers"][1]["kind"], "svg");
    assert_eq!(plan["layers"][2]["kind"], "text");
    assert_eq!(plan["layers"][3]["kind"], "svg");
    assert_eq!(plan["layers"][2]["x"], 14.);
    assert_eq!(plan["layers"][2]["content"]["key"], "rich");
    // Explicit portable alternatives are supported in other document hosts.
    assert!(PgfRenderer.render(&scene).unwrap().contains("alt"));
    let no_alternative = base(vec![text(TextContent::Native {
        host: "typst".into(),
        key: "rich".into(),
        alternative: None,
    })]);
    assert!(
        PgfRenderer
            .render(&no_alternative)
            .unwrap_err()
            .message
            .contains("cannot render in LaTeX")
    );
}
#[test]
fn pgf_escapes_plain_and_rejects_bad_geometry() {
    let scene = base(vec![text(TextContent::plain("50% & $x_1$"))]);
    let p = PgfRenderer.render(&scene).unwrap();
    assert!(p.contains("50\\% \\& \\$x\\_1\\$"));
    assert!(p.contains("\\pgfpoint{14.000000bp}{-10.000000bp}"));
    let scene = base(vec![Primitive::Circle {
        center: Point { x: 0., y: 0. },
        radius: -1.,
        paint: paint(),
        meta: Metadata::default(),
    }]);
    assert!(SvgRenderer.render(&scene).is_err());
    assert!(PgfRenderer.render(&scene).is_err());
    assert!(TypstRenderer.render(&scene).is_err());
}
#[test]
fn typst_group_clip_reaches_native_label() {
    let scene = base(vec![Primitive::Group {
        children: vec![text(TextContent::Native {
            host: "typst".into(),
            key: "n".into(),
            alternative: None,
        })],
        translate: Point { x: 5., y: 3. },
        clip: Some(Bounds {
            x: 0.,
            y: 0.,
            width: 20.,
            height: 10.,
        }),
        meta: Metadata::default(),
    }]);
    let plan: serde_json::Value =
        serde_json::from_str(&TypstRenderer.render(&scene).unwrap()).unwrap();
    assert_eq!(plan["layers"][1]["x"], 19.);
    assert_eq!(plan["layers"][1]["clip"]["x"], 5.);
    assert_eq!(plan["layers"][1]["clip"]["height"], 10.);
}

#[test]
fn pgf_wraps_plain_text_when_measured_width_is_bounded() {
    let mut primitive = text(TextContent::plain("A long title that needs several lines"));
    if let Primitive::Text {
        request, metrics, ..
    } = &mut primitive
    {
        request.max_width = Some(32.);
        metrics.width = 32.;
        metrics.height = 30.;
    }
    let pgf = PgfRenderer.render(&base(vec![primitive])).unwrap();
    assert!(pgf.contains("\\setbox0=\\hbox{"));
    assert!(pgf.contains("\\ifdim\\wd0>32.000000bp \\parbox[t]{32.000000bp}"));
}

#[test]
fn typst_uses_foreign_native_alternative_as_plain_text() {
    let scene = base(vec![text(TextContent::Native {
        host: "latex".into(),
        key: "rich".into(),
        alternative: Some("Readable".into()),
    })]);
    let plan: serde_json::Value =
        serde_json::from_str(&TypstRenderer.render(&scene).unwrap()).unwrap();
    assert_eq!(plan["layers"][1]["content"]["kind"], "plain");
    assert_eq!(plan["layers"][1]["content"]["text"], "Readable");
    let no_alternative = base(vec![text(TextContent::Native {
        host: "latex".into(),
        key: "rich".into(),
        alternative: None,
    })]);
    assert!(TypstRenderer.render(&no_alternative).is_err());
}

#[test]
fn svg_rejects_multiline_plain_text() {
    let scene = base(vec![text(TextContent::plain("first\nsecond"))]);
    assert!(
        SvgRenderer
            .render(&scene)
            .unwrap_err()
            .message
            .contains("multiline")
    );
    assert!(
        CompatibilitySvgRenderer::default()
            .render(&scene)
            .unwrap_err()
            .message
            .contains("multiline")
    );
}

#[test]
fn wrapped_pgf_label_compiles_with_pdflatex() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    if Command::new("pdflatex").arg("--version").output().is_err() {
        // Never pass silently: skipping needs an explicit opt-out.
        assert!(
            std::env::var_os("CHARTGEN_SKIP_TEX").is_some(),
            "pdflatex is required (set CHARTGEN_SKIP_TEX=1 to skip explicitly)"
        );
        eprintln!("SKIPPED wrapped_pgf_label_compiles_with_pdflatex: CHARTGEN_SKIP_TEX set");
        return;
    }
    let mut primitive = text(TextContent::plain(
        "A long title with enough words to wrap within a narrow measured box",
    ));
    if let Primitive::Text {
        request, metrics, ..
    } = &mut primitive
    {
        request.max_width = Some(70.);
        metrics.width = 70.;
        metrics.height = 36.;
    }
    let pgf = PgfRenderer.render(&base(vec![primitive])).unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("chartgen-pgf-wrap-{}-{stamp}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("chart.tex"), format!("\\documentclass{{article}}\n\\usepackage{{pgf}}\n\\begin{{document}}\n{pgf}\\end{{document}}\n")).unwrap();
    let result = Command::new("pdflatex")
        .args(["-interaction=nonstopmode", "-halt-on-error", "chart.tex"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success(),
        "pdflatex failed: {}",
        stdout
            .chars()
            .rev()
            .take(2000)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    );
    assert!(dir.join("chart.pdf").metadata().unwrap().len() > 1000);
    fs::remove_dir_all(dir).unwrap();
}

fn use_mark(href: &str, size: f64) -> Primitive {
    Primitive::Use {
        center: Point { x: 0., y: 0. },
        size,
        href: href.into(),
        paint: Paint {
            fill: Some("#123456".into()),
            stroke: None,
            stroke_width: 0.,
            opacity: 1.,
        },
        meta: Metadata {
            classes: vec!["dot".into(), "single".into()],
            ..Default::default()
        },
    }
}

/// The plain style's mark library (charts/style/plain-defs.svg): every symbol the original
/// can place renders in every backend, drawn from its definition in a -1…1 box.
#[test]
fn library_marks_render_in_every_backend() {
    let symbols = [
        "mark-dot",
        "mark-square",
        "mark-triangle",
        "mark-x",
        "mark-plus",
        "mark-asterisk",
        "mark-icon-down",
        "mark-icon-up",
        "mark-icon-left",
        "mark-icon-right",
    ];
    for href in symbols {
        let scene = base(vec![use_mark(href, 2.)]);
        let svg = SvgRenderer.render(&scene).unwrap();
        assert!(
            svg.contains("fill=\"#123456\"") || svg.contains("stroke=\"#123456\""),
            "{href}"
        );
        assert!(
            PgfRenderer.render(&scene).unwrap().contains("123456") || href != "x",
            "{href}"
        );
        let plan: serde_json::Value =
            serde_json::from_str(&TypstRenderer.render(&scene).unwrap()).unwrap();
        assert_eq!(plan["layers"].as_array().unwrap().len(), 2, "{href}");
        let c = CompatibilitySvgRenderer.render(&scene).unwrap();
        assert!(c.contains(&format!("href=\"#{href}\"")), "{href}");
    }
    // <symbol id="mark-dot"><circle r="1"/>; mark-triangle: <path d="M0,-1.1 L1,0.7 L-1,0.7 z"/>.
    let svg = SvgRenderer
        .render(&base(vec![use_mark("mark-dot", 7.5)]))
        .unwrap();
    assert!(svg.contains("<circle cx=\"0.000000\" cy=\"0.000000\" r=\"3.750000\""));
    let svg = SvgRenderer
        .render(&base(vec![use_mark("mark-triangle", 2.)]))
        .unwrap();
    assert!(svg.contains("d=\"M0.000000 -1.100000 L1.000000 0.700000 L-1.000000 0.700000 Z\""));
    // mark-x: fill="none" stroke-width="0.35" in symbol units, the ink inherited from the use.
    let svg = SvgRenderer
        .render(&base(vec![use_mark("mark-x", 4.)]))
        .unwrap();
    assert!(svg.contains("fill=\"none\" stroke=\"#123456\" stroke-width=\"0.700000\""));
    assert!(
        SvgRenderer
            .render(&base(vec![use_mark("mark-unknown", 2.)]))
            .unwrap_err()
            .message
            .contains("unknown house mark")
    );
}

/// A wedge renders as the original's slice outline (tests/conformance/figkit/fixtures.json emit:
/// Wedge(100, 100, 50, 0, 120, inset 0.5) -> "M100.5,99.71 L100.5,50 A50,50 0 0,1 143.55,124.57 Z"),
/// and an empty one draws nothing.
#[test]
fn wedges_render_as_their_outline() {
    let wedge = |a1: f64| Primitive::Wedge {
        center: Point { x: 100., y: 100. },
        radius: 50.,
        inner_radius: 0.,
        start_angle: 0.,
        end_angle: a1,
        inset: 0.5,
        paint: paint(),
        meta: Metadata {
            classes: vec!["slice".into()],
            ..Default::default()
        },
    };
    let c = CompatibilitySvgRenderer
        .render(&base(vec![wedge(120.), wedge(120. - 120.)]))
        .unwrap();
    assert!(c.contains(
        "<path class=\"slice\" d=\"M100.5,99.71 L100.5,50 A50,50 0 0,1 143.55,124.57 Z\"/>"
    ));
    assert_eq!(c.matches("<path").count(), 1);
    let svg = SvgRenderer.render(&base(vec![wedge(120.)])).unwrap();
    assert!(
        svg.contains("A50.000000 50.000000 0.000000 0 1 143.549105 124.565737 Z\""),
        "{svg}"
    );
    assert!(
        PgfRenderer
            .render(&base(vec![wedge(120.)]))
            .unwrap()
            .contains("\\pgfpatharcto")
    );
    assert!(TypstRenderer.render(&base(vec![wedge(120.)])).is_ok());
}
