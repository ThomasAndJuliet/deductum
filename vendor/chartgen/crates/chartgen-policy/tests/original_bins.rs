//! The original's `tests/test_chooser.py` class Bins (R2-43, `timeaxis.rebin`), translated.
use chartgen_model::Spec;
use chartgen_policy::{Rebin, rebin};
use serde_json::{Value, json};

const DEATHS: [&str; 8] = [
    "1960–1969",
    "1970–1979",
    "1980–1989",
    "1990–1999",
    "2000–2004",
    "2005–2009",
    "2010–2014",
    "2015–2019",
];

fn binned(names: &[&str], values: Value, additive: Option<bool>) -> Spec {
    let mut d = json!({"chartspec": 1, "id": "t", "job": {"kind": "lookup", "text": "t"},
        "title": {"text": "T"},
        "data": {"x": {"kind": "time", "values": names, "grid": "bin"},
                 "measure": {"kind": "count", "zero": "meaningful", "unit": {"prefix": "$"},
                             "persistence": "summary"},
                 "series": [{"id": "s", "values": values}]}});
    if let Some(a) = additive {
        d["data"]["measure"]["additive"] = json!(a);
    }
    Spec { raw: d }
}

#[test]
fn re_aggregated() {
    let Rebin::Decided(s, text) = rebin(&binned(
        &DEATHS,
        json!([455, 500, 460, 418, 215, 200, 165, 185]),
        Some(true),
    )) else {
        panic!("re-aggregated")
    };
    assert_eq!(
        s.get("/data/x/values"),
        &json!([
            "1960–1969",
            "1970–1979",
            "1980–1989",
            "1990–1999",
            "2000–2009",
            "2010–2019"
        ])
    );
    assert_eq!(
        s.get("/data/series/0/values"),
        &json!([455, 500, 460, 418, 415, 350])
    );
    assert!(text.contains("2000–2004 + 2005–2009 → 2000–2009"));
}

#[test]
fn refused_when_the_values_may_not_add() {
    let Rebin::Refused(path, ..) = rebin(&binned(
        &DEATHS,
        json!([455, 500, 460, 418, 215, 200, 165, 185]),
        None,
    )) else {
        panic!("refused")
    };
    assert_eq!(path, "data.measure.additive");
}

#[test]
fn refused_when_the_bins_do_not_tile() {
    assert!(matches!(
        rebin(&binned(
            &["1960–1969", "1970–1974", "1975–1984"],
            json!([1, 2, 3]),
            Some(true)
        )),
        Rebin::Refused("data.x.values", ..)
    ));
}
