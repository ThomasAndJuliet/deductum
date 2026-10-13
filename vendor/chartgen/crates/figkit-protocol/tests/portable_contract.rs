//! Literal physical-point fixtures exercise the public scene and host boundaries.
use figkit_model::*;
use figkit_protocol::session::Session;
use figkit_render::{Backend, render};
use figkit_scene::{prepare, profile::*};

fn request() -> TextRequest {
    TextRequest {
        id: "r".into(),
        content: TextContent::Plain { text: "ab".into() },
        typography: Typography {
            font_family: "cmr".into(),
            font_size: 10.,
            weight: "normal".into(),
            font_style: "normal".into(),
        },
        max_width: None,
        context: MeasurementContext {
            id: "c".into(),
            engine: "recording".into(),
            engine_version: "1".into(),
            font_environment: "fixed".into(),
            settings: "preamble".into(),
            registry_revision: "1".into(),
        },
    }
}
fn reply(r: &TextRequest) -> MeasurementReply {
    MeasurementReply {
        request: r.clone(),
        metrics: BlockMetrics {
            size: Size {
                width: 20.,
                height: 10.,
            },
            baseline: Some(7.),
        },
    }
}
fn rect(x: f64, y: f64, width: f64, height: f64, fill: &str) -> Primitive {
    Primitive::Rect {
        bounds: Bounds {
            x,
            y,
            width,
            height,
        },
        radius: 2.,
        paint: Paint {
            fill: Some(fill.into()),
            stroke: None,
            stroke_width: 0.,
            opacity: 1.,
        },
        metadata: ElementMetadata::default(),
    }
}
fn text(anchor: HorizontalAnchor) -> Primitive {
    let r = request();
    Primitive::Text {
        placed: PlacedText {
            text: MeasuredText::from_reply(
                r.clone(),
                reply(&r),
                TextPaint::Resolved {
                    fill: "black".into(),
                },
            )
            .unwrap(),
            position: Point { x: 40., y: 30. },
            anchor,
            metadata: ElementMetadata::default(),
        },
    }
}
fn figure(nodes: Vec<Primitive>) -> Figure {
    Figure {
        version: 2,
        size: Size {
            width: 100.,
            height: 80.,
        },
        metadata: FigureMetadata::default(),
        background: "white".into(),
        primitives: nodes,
        resources: Resources::default(),
    }
}
fn fixture(anchor: HorizontalAnchor) -> Figure {
    figure(vec![
        rect(5., 6., 20., 10., "red"),
        Primitive::Path {
            commands: vec![
                PathCommand::MoveTo {
                    to: Point { x: 0., y: 0. },
                },
                PathCommand::LineTo {
                    to: Point { x: 10., y: 20. },
                },
            ],
            paint: Paint::default(),
            metadata: ElementMetadata::default(),
        },
        text(anchor),
        rect(60., 50., 8., 8., "blue"),
    ])
}

#[test]
fn measured_anchors_and_order_survive_each_encoder() {
    for (anchor, x) in [
        (HorizontalAnchor::Start, 40.),
        (HorizontalAnchor::Middle, 30.),
        (HorizontalAnchor::End, 20.),
    ] {
        let prepared = prepare(&fixture(anchor), &Limits::default()).unwrap();
        for backend in [Backend::Svg, Backend::CompatibilitySvg] {
            let output = render(&prepared, backend).unwrap();
            assert!(
                output.contains(&format!("x=\"{x}\" y=\"37\"")),
                "{output}"
            );
            assert!(output.find("fill=\"red\"").unwrap() < output.find(">ab</text>").unwrap());
            assert!(output.find(">ab</text>").unwrap() < output.find("fill=\"blue\"").unwrap());
            assert!(output.contains("rx=\"2\""));
        }
        let pgf = render(&prepared, Backend::Pgf).unwrap();
        assert!(
            pgf.contains(&format!("\\pgfpoint{{{x}bp}}{{-37bp}}")),
            "{pgf}"
        );
        for backend in [Backend::Browser, Backend::Typst] {
            let plan: serde_json::Value =
                serde_json::from_str(&render(&prepared, backend).unwrap()).unwrap();
            let layers = plan["layers"].as_array().unwrap();
            assert_eq!(layers.len(), 5); // background + four authored primitives
            assert_eq!(layers[3]["kind"], "text");
            assert_eq!(layers[3]["x"], x);
            assert_eq!(layers[3]["y"], 30.);
            assert_eq!(layers[3]["metrics"]["baseline"], 7.);
            assert_eq!(layers[3]["metrics"]["size"]["width"], 20.);
            assert_eq!(
                layers[3]["request"],
                serde_json::to_value(request()).unwrap()
            );
            assert!(layers[1]["svg"].as_str().unwrap().contains("fill=\"red\""));
            assert!(layers[4]["svg"].as_str().unwrap().contains("fill=\"blue\""));
        }
    }
}

#[test]
fn complete_request_identity_rejects_every_stale_field() {
    let original = request();
    let measured = reply(&original);
    let mut changes = Vec::new();
    macro_rules! changed { ($($field:ident).+, $value:expr) => {{ let mut r=original.clone(); r.$($field).+ = $value; changes.push(r); }}; }
    changed!(
        content,
        TextContent::Plain {
            text: "changed".into()
        }
    );
    changed!(max_width, Some(40.));
    changed!(typography.weight, "bold".into());
    changed!(typography.font_style, "italic".into());
    changed!(typography.font_family, "sans".into());
    changed!(typography.font_size, 11.);
    changed!(context.id, "different".into());
    changed!(context.engine, "other".into());
    changed!(context.engine_version, "2".into());
    changed!(context.font_environment, "new fonts".into());
    changed!(context.settings, "new preamble".into());
    changed!(context.registry_revision, "2".into());
    for changed in changes {
        assert_eq!(
            MeasuredText::from_reply(
                changed.clone(),
                measured.clone(),
                TextPaint::Resolved {
                    fill: "black".into()
                }
            )
            .unwrap_err()
            .kind,
            ErrorKind::StaleMeasurement,
            "{changed:?}"
        );
        let mut session = Session::new(changed.context.clone());
        assert_eq!(
            session
                .advance(&[changed.clone()], &[measured.clone()], &Limits::default())
                .unwrap_err()
                .kind,
            ErrorKind::StaleMeasurement,
            "{changed:?}"
        );
        assert!(session.replies.is_empty());
    }
    assert!(
        MeasuredText::from_reply(
            original,
            measured,
            TextPaint::Resolved {
                fill: "black".into()
            }
        )
        .is_ok()
    );
}

#[test]
fn session_batches_progress_and_limits_count_actual_work() {
    let a = request();
    let mut b = a.clone();
    b.id = "b".into();
    b.content = TextContent::Plain { text: "B".into() };
    let limits = Limits {
        max_measurement_requests: 2,
        max_measurement_rounds: 2,
        ..Limits::default()
    };
    let mut s = Session::new(a.context.clone());
    assert_eq!(
        s.advance(&[a.clone(), b.clone()], &[], &limits)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(s.rounds, 0);
    assert_eq!(
        s.advance(&[a.clone(), b.clone()], &[reply(&a)], &limits)
            .unwrap(),
        vec![b.clone()]
    );
    assert_eq!(s.rounds, 1);
    let snapshot = serde_json::to_value(&s).unwrap();
    assert_eq!(
        s.advance(&[a.clone(), b.clone()], &[reply(&a)], &limits)
            .unwrap_err()
            .kind,
        ErrorKind::MissingMeasurement
    );
    assert_eq!(serde_json::to_value(&s).unwrap(), snapshot);
    assert!(
        s.advance(&[a.clone(), b.clone()], &[reply(&b)], &limits)
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.rounds, 2);
    assert!(
        s.advance(&[a.clone(), b.clone()], &[], &limits)
            .unwrap()
            .is_empty()
    );
    let mut c = a.clone();
    c.id = "c".into();
    assert_eq!(
        s.advance(&[c.clone()], &[reply(&c)], &limits)
            .unwrap_err()
            .kind,
        ErrorKind::BudgetExceeded
    );
    for count in 1..=3 {
        let reqs: Vec<_> = (0..count)
            .map(|i| {
                let mut r = a.clone();
                r.id = format!("r{i}").into();
                r
            })
            .collect();
        let replies: Vec<_> = reqs.iter().map(reply).collect();
        let mut s = Session::new(a.context.clone());
        let result = s.advance(&reqs, &replies, &limits);
        assert_eq!(result.is_ok(), count <= 2);
    }
    let mut s = Session::new(a.context.clone());
    s.advance(&[a.clone()], &[], &limits).unwrap();
    assert_eq!(
        s.advance(&[a], &[], &limits).unwrap_err().kind,
        ErrorKind::MissingMeasurement
    );
}

#[test]
fn prepared_snapshot_retains_authored_metadata_and_does_not_borrow_inputs() {
    let mut f = fixture(HorizontalAnchor::Start);
    f.primitives[0].metadata_mut().authored_id = Some("caller".into());
    f.primitives[0].metadata_mut().element_id = Some("figkit-0".into());
    let prepared = prepare(&f, &Limits::default()).unwrap();
    let before = render(&prepared, Backend::Svg).unwrap();
    f.primitives.clear();
    assert_eq!(render(&prepared, Backend::Svg).unwrap(), before);
    assert!(before.contains("id=\"caller\""));
    let ids: std::collections::BTreeSet<_> = prepared
        .figure()
        .primitives
        .iter()
        .map(|p| p.metadata().element_id.as_ref().unwrap())
        .collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(
        prepared.figure().primitives[0]
            .metadata()
            .element_id
            .as_ref()
            .unwrap()
            .0,
        "figkit-0"
    );
}

#[test]
fn scene_counts_and_output_bytes_have_inclusive_boundaries() {
    for n in 1..=3 {
        let f = figure((0..n).map(|_| rect(0., 0., 1., 1., "red")).collect());
        let limits = Limits {
            max_primitives: 2,
            ..Limits::default()
        };
        assert_eq!(prepare(&f, &limits).is_ok(), n <= 2);
    }
    for n in 1..=3 {
        let commands = (0..n)
            .map(|i| {
                if i == 0 {
                    PathCommand::MoveTo {
                        to: Point { x: 0., y: 0. },
                    }
                } else {
                    PathCommand::LineTo {
                        to: Point { x: i as f64, y: 0. },
                    }
                }
            })
            .collect();
        let f = figure(vec![Primitive::Path {
            commands,
            paint: Paint::default(),
            metadata: ElementMetadata::default(),
        }]);
        let limits = Limits {
            max_path_segments: 2,
            ..Limits::default()
        };
        assert_eq!(prepare(&f, &limits).is_ok(), n <= 2);
    }
    for depth in 1..=3 {
        let mut node = rect(0., 0., 1., 1., "red");
        for _ in 0..depth {
            node = Primitive::Group {
                opacity: 1.,
                children: vec![node],
                translate: Point { x: 0., y: 0. },
                clip: None,
                metadata: ElementMetadata::default(),
            };
        }
        assert_eq!(
            prepare(
                &figure(vec![node]),
                &Limits {
                    max_depth: 2,
                    ..Limits::default()
                }
            )
            .is_ok(),
            depth <= 2
        );
    }
    let f = fixture(HorizontalAnchor::Start);
    let bytes = serde_json::to_vec(&f).unwrap().len()
        + serde_json::to_vec(&CompatibilityResources::default())
            .unwrap()
            .len();
    for cap in [bytes - 1, bytes, bytes + 1] {
        assert_eq!(
            prepare(
                &f,
                &Limits {
                    max_input_bytes: cap,
                    ..Limits::default()
                }
            )
            .is_ok(),
            cap >= bytes
        );
    }
    for backend in [
        Backend::Svg,
        Backend::CompatibilitySvg,
        Backend::Pgf,
        Backend::Browser,
        Backend::Typst,
    ] {
        let length = render(&prepare(&f, &Limits::default()).unwrap(), backend)
            .unwrap()
            .len();
        for cap in [length - 1, length, length + 1] {
            let result = render(
                &prepare(
                    &f,
                    &Limits {
                        max_output_bytes: cap,
                        ..Limits::default()
                    },
                )
                .unwrap(),
                backend,
            );
            if cap < length {
                assert_eq!(result.unwrap_err().kind, ErrorKind::BudgetExceeded);
            } else {
                assert_eq!(result.unwrap().len(), length);
            }
        }
    }
}

#[test]
fn baseline_free_native_blocks_require_the_matching_host() {
    for (host, backend) in [
        ("browser", Backend::Browser),
        ("typst", Backend::Typst),
        ("latex", Backend::Pgf),
    ] {
        let mut node = text(HorizontalAnchor::Start);
        let Primitive::Text { placed } = &mut node else {
            unreachable!()
        };
        placed.text.metrics.baseline = None;
        placed.text.request.content = TextContent::Native {
            host: host.into(),
            key: "opaque-key".into(),
            alternative: None,
        };
        placed.text.paint = TextPaint::NativeInherited { host: host.into() };
        let prepared = prepare(
            &figure(vec![
                rect(0., 0., 1., 1., "red"),
                node,
                rect(3., 3., 1., 1., "blue"),
            ]),
            &Limits::default(),
        )
        .unwrap();
        assert!(render(&prepared, backend).is_ok());
        for wrong in [Backend::Svg, Backend::CompatibilitySvg] {
            assert_eq!(
                render(&prepared, wrong).unwrap_err().kind,
                ErrorKind::UnsupportedCapability
            );
        }
    }
    let mut node = text(HorizontalAnchor::Start);
    let Primitive::Text { placed } = &mut node else {
        unreachable!()
    };
    placed.text.metrics.baseline = None;
    let prepared = prepare(&figure(vec![node]), &Limits::default()).unwrap();
    assert_eq!(
        render(&prepared, Backend::Svg).unwrap_err().kind,
        ErrorKind::UnsupportedCapability
    );
}

#[test]
fn profile_contrast_stroke_and_declared_distinctions_use_explicit_thresholds() {
    let black = ProfileElement {
        element: "black".into(),
        paint: "black".into(),
        opacity: 1.,
        stroke_width: Some(1.),
    };
    // Black/white luminances are 0 and 1, so contrast is 1.05/.05 = 21.
    for threshold in [20.999, 21., 21.001] {
        let report = check(
            &OutputProfile {
                minimum_contrast: threshold,
                ..OutputProfile::default()
            },
            &[black.clone()],
            &[],
        );
        assert_eq!(
            report
                .observations
                .iter()
                .any(|o| matches!(o, ProfileObservation::LowContrast { .. })),
            threshold > 21.
        );
        assert!(!report.provenance.is_empty());
    }
    for scale in [0.5, 1., 2.] {
        let report = check(
            &OutputProfile {
                output_scale: scale,
                minimum_stroke_pt: 1.,
                ..OutputProfile::default()
            },
            &[black.clone()],
            &[],
        );
        assert_eq!(
            report.observations.iter().any(
                |o| matches!(o,ProfileObservation::ThinStroke {width_pt,..} if *width_pt==scale)
            ),
            scale < 1.
        );
    }
    let mut white = black.clone();
    white.element = "white".into();
    white.paint = "white".into();
    let distinction = RequiredDistinction {
        first: black.element.clone(),
        second: white.element.clone(),
    };
    let elements = [black.clone(), white.clone()];
    let before = serde_json::to_value(&elements).unwrap();
    assert!(
        !check(&OutputProfile::default(), &elements, &[distinction])
            .observations
            .iter()
            .any(|o| matches!(o, ProfileObservation::LostDistinction { .. }))
    );
    assert_eq!(serde_json::to_value(&elements).unwrap(), before);
    white.paint = "black".into();
    let pair = RequiredDistinction {
        first: black.element.clone(),
        second: white.element.clone(),
    };
    assert!(
        check(
            &OutputProfile::default(),
            &[black.clone(), white.clone()],
            &[pair]
        )
        .observations
        .iter()
        .any(
            |o| matches!(o,ProfileObservation::LostDistinction {difference,..} if *difference==0.)
        )
    );
    assert!(
        check(&OutputProfile::default(), &[black, white], &[])
            .observations
            .is_empty()
    );
    let unsupported = ProfileElement {
        element: "gradient".into(),
        paint: "url(#gradient)".into(),
        opacity: 1.,
        stroke_width: None,
    };
    assert!(matches!(
        check(&OutputProfile::default(), &[unsupported], &[])
            .observations
            .as_slice(),
        [ProfileObservation::Indeterminate { .. }]
    ));
}

#[test]
fn transport_versions_errors_and_serialized_output_limits_are_distinct() {
    use figkit_protocol::{handle, handle_with_limits};
    use serde_json::{Value, json};
    let decode = |bytes: Vec<u8>| serde_json::from_slice::<Value>(&bytes).unwrap();
    let negotiate = br#"{"version":2,"operation":"negotiate"}"#;
    assert_eq!(
        decode(handle(negotiate))["value"]["versions"],
        json!([1, 2])
    );
    for input in [
        b"{".as_slice(),
        br#"{"version":99,"operation":"negotiate"}"#,
        br#"{"version":2,"operation":"render_scene","backend":"svg","figure":{}}"#,
    ] {
        let response = decode(handle(input));
        assert_eq!(response["outcome"], "error");
        assert_eq!(response["error"]["kind"], "invalid_input");
        assert!(response.get("output").is_none());
    }
    let request=serde_json::to_vec(&json!({"version":2,"operation":"render_scene","backend":"svg","figure":fixture(HorizontalAnchor::Start)})).unwrap();
    let normal = handle(&request);
    assert_eq!(decode(normal.clone())["outcome"], "output");
    for cap in [normal.len() - 1, normal.len(), normal.len() + 1] {
        let response = decode(handle_with_limits(
            &request,
            &Limits {
                max_output_bytes: cap,
                ..Limits::default()
            },
        ));
        if cap < normal.len() {
            assert_eq!(response["error"]["kind"], "budget_exceeded");
            assert!(response.get("output").is_none());
        } else {
            assert_eq!(response["outcome"], "output");
        }
    }
    for cap in [negotiate.len() - 1, negotiate.len(), negotiate.len() + 1] {
        let response = decode(handle_with_limits(
            negotiate,
            &Limits {
                max_input_bytes: cap,
                ..Limits::default()
            },
        ));
        assert_eq!(
            response["outcome"],
            if cap < negotiate.len() {
                "error"
            } else {
                "data"
            }
        );
    }
    let legacy = decode(handle(
        br#"{"version":1,"operation":"figkit","call":"missing-operation","args":{}}"#,
    ));
    assert_eq!(legacy["outcome"], "error");
    assert_eq!(legacy["version"], 1);
}

#[test]
fn resources_and_path_clips_validate_before_any_artifact_is_returned() {
    let mut f = figure(vec![
        rect(0., 0., 1., 1., "red"),
        Primitive::Use {
            symbol: "missing".into(),
            position: Point { x: 0., y: 0. },
            metadata: ElementMetadata::default(),
        },
    ]);
    assert_eq!(
        prepare(&f, &Limits::default()).unwrap_err().kind,
        ErrorKind::ResourceFailure
    );
    let commands = vec![
        PathCommand::MoveTo {
            to: Point { x: 0., y: 0. },
        },
        PathCommand::LineTo {
            to: Point { x: 10., y: 20. },
        },
    ];
    f.resources.symbols.insert(
        "missing".into(),
        Symbol {
            shapes: vec![SymbolShape {
                commands: commands.clone(),
                paint: Paint::default(),
            }],
        },
    );
    assert!(
        render(&prepare(&f, &Limits::default()).unwrap(), Backend::Svg)
            .unwrap()
            .contains("L10 20")
    );
    f.resources
        .clips
        .insert("clip".into(), Clip::Path { commands });
    f.primitives.push(Primitive::Group {
        opacity: 1.,
        children: vec![rect(0., 0., 2., 2., "blue")],
        translate: Point { x: 3., y: 4. },
        clip: Some("clip".into()),
        metadata: ElementMetadata::default(),
    });
    let prepared = prepare(&f, &Limits::default()).unwrap();
    for backend in [Backend::Svg, Backend::Pgf, Backend::CompatibilitySvg] {
        assert!(render(&prepared, backend).is_ok());
    }
    for backend in [Backend::Browser, Backend::Typst] {
        assert!(!figkit_render::capabilities(backend).path_clips);
        assert_eq!(
            render(&prepared, backend).unwrap_err().kind,
            ErrorKind::UnsupportedCapability
        );
    }
    for cap in [1, 2, 3] {
        assert_eq!(
            prepare(
                &f,
                &Limits {
                    max_resources: cap,
                    ..Limits::default()
                }
            )
            .is_ok(),
            cap >= 2
        );
    }
    let mut invalid = fixture(HorizontalAnchor::Start);
    if let Primitive::Rect { bounds, .. } = &mut invalid.primitives[0] {
        bounds.width = f64::MAX;
        bounds.x = f64::MAX;
    }
    assert_eq!(
        prepare(&invalid, &Limits::default()).unwrap_err().kind,
        ErrorKind::InvalidInput
    );
}

#[test]
fn box_observations_report_positive_area_and_only_explicit_allowances() {
    use figkit_scene::lint::*;
    let region = Bounds {
        x: 0.,
        y: 0.,
        width: 100.,
        height: 80.,
    };
    let boxes = vec![
        ElementBounds {
            element: "a".into(),
            bounds: Bounds {
                x: 0.,
                y: 0.,
                width: 10.,
                height: 10.,
            },
        },
        ElementBounds {
            element: "touch".into(),
            bounds: Bounds {
                x: 10.,
                y: 0.,
                width: 10.,
                height: 10.,
            },
        },
        ElementBounds {
            element: "overlap".into(),
            bounds: Bounds {
                x: 9.,
                y: 0.,
                width: 1.,
                height: 10.,
            },
        },
        ElementBounds {
            element: "outside".into(),
            bounds: Bounds {
                x: 99.,
                y: 70.,
                width: 2.,
                height: 2.,
            },
        },
    ];
    let observations = observe_boxes_checked(
        &region,
        &boxes,
        &OverlapAllowances::default(),
        0.,
        &LintLimits::default(),
    )
    .unwrap();
    assert_eq!(observations.len(), 2);
    assert!(
        matches!(&observations[0],Observation::Overlap {first,second} if first.0=="a"&&second.0=="overlap")
    );
    assert!(
        matches!(&observations[1],Observation::OutsideBounds {element} if element.0=="outside")
    );
    let mut allowed = OverlapAllowances::default();
    allowed.pairs.insert(("overlap".into(), "a".into()));
    assert!(matches!(
        observe_boxes_checked(&region, &boxes, &allowed, 0., &LintLimits::default())
            .unwrap()
            .as_slice(),
        [Observation::OutsideBounds { .. }]
    ));
    for cap in [5, 6, 7] {
        assert_eq!(
            observe_boxes_checked(
                &region,
                &boxes,
                &allowed,
                0.,
                &LintLimits {
                    max_pairs: cap,
                    max_observations: 2
                }
            )
            .is_ok(),
            cap >= 6
        );
    }
    for cap in [1, 2, 3] {
        assert_eq!(
            observe_boxes_checked(
                &region,
                &boxes,
                &OverlapAllowances::default(),
                0.,
                &LintLimits {
                    max_pairs: 6,
                    max_observations: cap
                }
            )
            .is_ok(),
            cap >= 2
        );
    }
}

#[test]
fn style_expansion_is_host_supplied_bounded_and_recomputed() {
    use figkit_style::{MemorySource, Style, load_checked};
    let sheets = [
        (
            "root",
            "@import \"child\";\n:root { --label-size: 10; --gap: 2em; }\n.label { font-size: var(--label-size); fill: black; }",
        ),
        ("child", ":root { --ink: red; }"),
    ];
    for cap in [1, 2, 3] {
        let mut source = MemorySource::new(sheets);
        let result = load_checked(
            "fixture",
            "tests",
            "root",
            &mut source,
            &Limits {
                max_style_imports: cap,
                ..Limits::default()
            },
        );
        assert_eq!(result.is_ok(), cap >= 2, "import cap {cap}: {result:?}");
    }
    let mut source = MemorySource::new(sheets);
    let mut style = Style::load("fixture", "tests", "root", &mut source).unwrap();
    assert_eq!(style.metric("--gap").unwrap(), 20.);
    source.sheets.insert(
        "root".into(),
        sheets[0].1.replace("--label-size: 10", "--label-size: 12"),
    );
    style = Style::load("fixture", "tests", "root", &mut source).unwrap();
    assert_eq!(style.metric("--gap").unwrap(), 24.);
    let mut missing = MemorySource::default();
    assert_eq!(
        load_checked(
            "fixture",
            "tests",
            "absent",
            &mut missing,
            &Limits::default()
        )
        .unwrap_err()
        .kind,
        ErrorKind::ResourceFailure
    );
}

#[test]
fn exhausted_round_does_not_commit_partial_session_state() {
    let a = request();
    let mut b = a.clone();
    b.id = "b".into();
    let mut session = Session::new(a.context.clone());
    let limits = Limits {
        max_measurement_rounds: 1,
        ..Limits::default()
    };
    session
        .advance(&[a.clone(), b.clone()], &[], &limits)
        .unwrap();
    let before = serde_json::to_value(&session).unwrap();
    assert_eq!(
        session
            .advance(&[a.clone(), b], &[reply(&a)], &limits)
            .unwrap_err()
            .kind,
        ErrorKind::BudgetExceeded
    );
    assert_eq!(
        serde_json::to_value(&session).unwrap(),
        before,
        "a failed batch must not consume a round or admit partial replies"
    );
}

#[test]
fn plain_text_preserves_repeated_and_edge_spaces() {
    let mut node = text(HorizontalAnchor::Start);
    let Primitive::Text { placed } = &mut node else {
        unreachable!()
    };
    placed.text.request.content = TextContent::Plain {
        text: "  a  b  ".into(),
    };
    let scene = prepare(&figure(vec![node]), &Limits::default()).unwrap();
    let svg = render(&scene, Backend::Svg).unwrap();
    assert!(svg.contains("xml:space=\"preserve\""), "{svg}");
    assert!(svg.contains(">  a  b  </text>"));
    let pgf = render(&scene, Backend::Pgf).unwrap();
    assert!(pgf.contains("\\ \\ a\\ \\ b\\ \\ "), "{pgf}");
}
