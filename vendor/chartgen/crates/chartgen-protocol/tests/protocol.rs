use chartgen_protocol::{Outcome, Response, handle};
#[test]
fn choose_and_schema_do_not_require_host_measurements() {
    let spec: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/basic.json")).unwrap();
    let input = serde_json::json!({"version":1,"operation":"choose","spec":spec});
    let response: Response =
        serde_json::from_slice(&handle(&serde_json::to_vec(&input).unwrap())).unwrap();
    let Outcome::Data { value } = response.outcome else {
        panic!("choose must return a report")
    };
    assert_eq!(value["status"], "ready");
    assert_eq!(value["type"], "bar");
    let response: Response =
        serde_json::from_slice(&handle(br#"{"version":1,"operation":"schema"}"#)).unwrap();
    let Outcome::Data { value } = response.outcome else {
        panic!("schema must return data")
    };
    assert!(value["properties"]["data"].is_object());
}
#[test]
fn malformed_and_unknown_protocol_versions_are_structured_errors() {
    for bytes in [b"not json".as_slice(),br##"{"version":2,"operation":"render_scene","scene":{"version":1,"width":100,"height":100,"title":null,"description":null,"background":"#fff","primitives":[]},"backend":"svg"}"##.as_slice()] {
        let response:Response=serde_json::from_slice(&handle(bytes)).unwrap();
        assert_eq!(response.version,1);
        assert!(matches!(response.outcome,Outcome::Error{error} if error.kind=="protocol"));
    }
}
#[test]
fn scene_protocol_preserves_fractional_geometry_with_exact_spec_numbers_enabled() {
    let input = serde_json::json!({"version":1,"operation":"render_scene","backend":"svg","scene":{"version":1,"width":100.5,"height":60.25,"title":null,"description":null,"background":"#fff","primitives":[{"kind":"rect","bounds":{"x":1.25,"y":2.75,"width":10.5,"height":20.125},"paint":{"fill":"#123456","stroke":null,"stroke_width":0.0,"opacity":0.75},"meta":{"id":null,"role":"mark","classes":[],"data":{}}}]}});
    let response: serde_json::Value =
        serde_json::from_slice(&handle(&serde_json::to_vec(&input).unwrap())).unwrap();
    assert_eq!(response["outcome"], "output", "{response}");
    assert!(response["output"].as_str().unwrap().contains("20.125000"));
}

#[test]
fn lint_svg_with_stylesheet_and_extents_is_not_partial() {
    use chartgen_protocol::handle;
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 40"><text class="prose title" x="0" y="10">A</text></svg>"#;
    let call = |extra: serde_json::Value| -> serde_json::Value {
        let mut req = serde_json::json!({"version": 1, "operation": "lint_svg", "svg": svg, "style": "plain", "cvd": "assume"});
        req.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_slice(&handle(&serde_json::to_vec(&req).unwrap())).unwrap()
    };
    let partial = |v: &serde_json::Value| {
        v["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["rule"] == "CG-SVG-PARTIAL")
    };
    assert!(partial(&call(serde_json::json!({}))));
    let css = ".title { font-size: 12px; fill: #595959; }";
    assert!(partial(&call(serde_json::json!({"css": css}))));
    let texts: serde_json::Value = serde_json::from_slice(&handle(
        &serde_json::to_vec(&serde_json::json!({"version": 1, "operation": "lint_svg_texts", "svg": svg, "css": css})).unwrap(),
    ))
    .unwrap();
    assert_eq!(texts["value"][0]["size"], 12.0);
    assert!(!partial(&call(
        serde_json::json!({"css": css, "extents": [[6.0, 8.5, 2.5]]})
    )));
}

fn error_of(bytes: &[u8]) -> chartgen_model::Error {
    let response: Response = serde_json::from_slice(&handle(bytes)).unwrap();
    assert_eq!(response.version, 1);
    match response.outcome {
        Outcome::Error { error } => error,
        other => panic!("expected an error outcome, got {other:?}"),
    }
}

#[test]
fn malformed_requests_fail_explicitly_instead_of_being_ignored() {
    let spec = include_str!("../../../examples/basic.json");
    let cases: Vec<(String, &str)> = vec![
        ("[1,2]".into(), "JSON object"),
        (r#"{"version":1}"#.into(), "missing operation"),
        (
            r#"{"version":1,"operation":7}"#.into(),
            "operation must be a string",
        ),
        (
            r#"{"version":1,"operation":"draw"}"#.into(),
            "unknown operation",
        ),
        (r#"{"version":"1","operation":"schema"}"#.into(), "version"),
        (
            r#"{"version":1,"operation":"schema","extra":true}"#.into(),
            "unknown field(s): extra",
        ),
        (
            r#"{"version":1,"operation":"render"}"#.into(),
            "missing render request",
        ),
        (
            r#"{"version":1,"operation":"render","request":[]}"#.into(),
            "must be an object",
        ),
        (
            r#"{"version":1,"operation":"render","request":{"theme":{}}}"#.into(),
            "missing spec",
        ),
        (
            format!(
                r#"{{"version":1,"operation":"render","request":{{"spec":{spec},"measurments":{{}}}}}}"#
            ),
            "unknown field(s): measurments",
        ),
        (
            format!(
                r#"{{"version":1,"operation":"render","request":{{"spec":{spec},"constraints":{{"width":"wide"}}}}}}"#
            ),
            "invalid type",
        ),
        (
            format!(r#"{{"version":1,"operation":"choose","spec":{spec},"variant":3}}"#),
            "variant must be a string",
        ),
        (
            format!(r#"{{"version":1,"operation":"choose","spec":{spec},"expect":["R0-07"]}}"#),
            "expect must be an object",
        ),
        (
            r#"{"version":1,"operation":"choose"}"#.into(),
            "missing spec",
        ),
        (
            r#"{"version":1,"operation":"render_scene","scene":{}}"#.into(),
            "missing",
        ),
        (
            r#"{"version":1,"operation":"lint_svg","svg":"<svg/>","cvd":false}"#.into(),
            "cvd must be a string",
        ),
    ];
    for (input, message) in cases {
        let error = error_of(input.as_bytes());
        assert_eq!(error.kind, "protocol", "{input}");
        assert!(
            error.message.contains(message),
            "{input}: {}",
            error.message
        );
    }
    let error = error_of(b"\xff\xfe{}");
    assert!(error.message.contains("not UTF-8"), "{}", error.message);
    let error = error_of(&vec![b' '; chartgen_protocol::MAX_INPUT_BYTES + 1]);
    assert!(error.message.contains("16 MiB"), "{}", error.message);
    let response: serde_json::Value =
        serde_json::from_slice(&chartgen_protocol::handle_oversized(1 << 30)).unwrap();
    assert_eq!(response["outcome"], "error");
    assert_eq!(response["error"]["kind"], "protocol");
}

/// The fields of the original's report (charts/chartgen/rules.py Report.to_json) plus copies,
/// as the bundled oracle outputs record them.
fn original_report_fields() -> Vec<String> {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/conformance/oracle/plain/p3-22-top-donors-stack.json"
    ))
    .unwrap();
    golden["report"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

fn assert_report_fields(report: &serde_json::Value) {
    for field in original_report_fields() {
        assert!(
            report.get(&field).is_some(),
            "report lacks {field}: {report}"
        );
    }
    assert!(report["copies"].is_object());
    for finding in report["findings"].as_array().unwrap() {
        assert!(finding["guide"].is_array(), "{finding}");
    }
}

#[test]
fn every_render_step_reports_the_original_fields_and_copies() {
    assert!(original_report_fields().contains(&"copies".to_string()));
    let spec: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/basic.json")).unwrap();
    let mut request = serde_json::json!({"spec": spec, "backend": "svg",
        "constraints": {"width": 480.0, "height": null, "measurement_context": "c", "max_rounds": 8},
        "measurements": {"context": "c", "entries": {}}});
    let mut statuses = vec![];
    for _ in 0..8 {
        let input = serde_json::json!({"version": 1, "operation": "render", "request": request});
        let response: serde_json::Value =
            serde_json::from_slice(&handle(&serde_json::to_vec(&input).unwrap())).unwrap();
        assert_eq!(response["outcome"], "render", "{response}");
        let result = &response["result"];
        assert_report_fields(&result["report"]);
        statuses.push(result["status"].as_str().unwrap().to_owned());
        if result["status"] != "need_measurements" {
            break;
        }
        // Accumulate entries across rounds, as every host must.
        for r in result["requests"].as_array().unwrap() {
            let size = r["font_size"].as_f64().unwrap();
            request["measurements"]["entries"][r["id"].as_str().unwrap()] =
                serde_json::json!({"width": size * 3.0, "height": size, "baseline": size * 0.75});
        }
    }
    assert_eq!(
        statuses.first().map(String::as_str),
        Some("need_measurements")
    );
    assert_eq!(statuses.last().map(String::as_str), Some("complete"));
    // A refusal stops before measurement and still reports every field.
    let mut refused = spec.clone();
    refused["purpose"] = "communication".into();
    let input =
        serde_json::json!({"version": 1, "operation": "render", "request": {"spec": refused}});
    let response: serde_json::Value =
        serde_json::from_slice(&handle(&serde_json::to_vec(&input).unwrap())).unwrap();
    assert_eq!(response["result"]["status"], "stopped");
    let report = &response["result"]["report"];
    assert_report_fields(report);
    let r007 = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["rule"] == "R0-07")
        .unwrap();
    // charts/chartgen/rules.py: ("R0-07", "a communication chart states its job", ...)
    assert_eq!(r007["title"], "a communication chart states its job");
}

/// tests/conformance/figkit/fixtures.json "escape": the original figkit.typst.escape on 92 strings.
#[test]
fn escape_reproduces_the_original_figkit_escape() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/conformance/figkit/fixtures.json"
    ))
    .unwrap();
    let cases = fixtures["escape"].as_array().unwrap();
    assert_eq!(cases.len(), 92);
    let mut wrong = vec![];
    for case in cases {
        let text = case["args"][0].as_str().unwrap();
        let input = serde_json::json!({"version": 1, "operation": "escape", "text": text});
        let response: serde_json::Value =
            serde_json::from_slice(&handle(&serde_json::to_vec(&input).unwrap())).unwrap();
        if response["value"] != case["expect"] {
            wrong.push((
                text.to_owned(),
                response["value"].clone(),
                case["expect"].clone(),
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of 92 differ: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(5)]
    );
}

#[test]
fn lint_style_runs_the_style_lint_over_a_flattened_style() {
    // the original's lint_style: a grey palette slot (R3-16) on a flattened style
    let req = serde_json::json!({"version": 1, "operation": "lint_style",
        "tokens": {"--series-1": "#808080", "--page": "#ffffff"}, "rules": [[".x", [["fill", "#000000"]]]]});
    let v: serde_json::Value =
        serde_json::from_slice(&handle(&serde_json::to_vec(&req).unwrap())).unwrap();
    assert!(
        v["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["rule"] == "R3-16")
    );
    let bad =
        serde_json::json!({"version": 1, "operation": "lint_style", "tokens": [], "rules": []});
    let v: serde_json::Value =
        serde_json::from_slice(&handle(&serde_json::to_vec(&bad).unwrap())).unwrap();
    assert_eq!(v["outcome"], "error");
}
