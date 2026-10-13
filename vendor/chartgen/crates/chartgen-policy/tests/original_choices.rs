use chartgen_data::{inspect, parse, transform};
use chartgen_model::Spec;
use chartgen_policy::decide;
use serde_json::Value;
#[test]
fn source_choice_fixtures() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/choices.json")).unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let expected = case["type"].as_str().unwrap();
        let spec = Spec {
            raw: case["raw"].clone(),
        };
        let data = transform(&spec, &parse(&spec).unwrap()).unwrap();
        let decision = decide(&spec, &data, &inspect(&data)).unwrap();
        assert_eq!(decision.chart_type.unwrap().as_str(), expected, "{name}");
        assert_eq!(decision.status, "drawn", "{name}: {:?}", decision.findings);
    }
}
