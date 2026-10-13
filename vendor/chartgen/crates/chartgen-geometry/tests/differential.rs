//! Differential tests: every expected value was produced by the original figkit `model`/`emit`
//! (tests/conformance/figkit/fixtures.json from the independent review's generator, and
//! tests/fixtures/geometry.json from tests/fixtures/generate.py).
use chartgen_geometry::*;
use chartgen_model::Point;
use serde_json::Value;

fn load(path: &str) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("fixture {path}: {e}"));
    serde_json::from_str(&text).unwrap()
}
fn review() -> Value {
    load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/conformance/figkit/fixtures.json"
    ))["geometry"]
        .clone()
}
fn own() -> Value {
    load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/geometry.json"
    ))
}
fn f(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| v.to_string().parse().expect("number"))
}
fn pt(v: &Value) -> Point {
    Point {
        x: f(&v[0]),
        y: f(&v[1]),
    }
}
fn pts(v: &Value) -> Vec<Point> {
    v.as_array().unwrap().iter().map(pt).collect()
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1. + a.abs().max(b.abs()))
}
fn assert_box(got: Bbox, want: &Value, what: &str) {
    let w: Vec<f64> = want.as_array().unwrap().iter().map(f).collect();
    assert!(
        got.as_array().iter().zip(&w).all(|(a, b)| close(*a, *b)),
        "{what}: {got:?} != {w:?}"
    );
}
/// emit.py `_n`: f"{x:.2f}" without trailing zeros, "-0" as "0".
fn n(x: f64) -> String {
    let s = format!("{x:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.into()
    }
}

#[test]
fn number_formatting_helper_matches_emit() {
    for c in own()["numbers"].as_array().unwrap() {
        assert_eq!(n(f(&c[0])), c[1].as_str().unwrap(), "{}", c[0]);
    }
}

#[test]
fn text_boxes_follow_anchor_and_extent() {
    for c in review()["text_box"].as_array().unwrap() {
        let t = &c["text"];
        let e = &c["extent"];
        let ext = Extent {
            width: f(&e["width"]),
            ascent: f(&e["ascent"]),
            descent: f(&e["descent"]),
        };
        let anchor = Anchor::parse(t["anchor"].as_str().unwrap()).unwrap();
        let got = text_box(f(&t["x"]), f(&t["y"]), anchor, &ext, f(&c["pad"])).unwrap();
        assert_box(got, &c["expect"], "Text.box");
    }
    // the original's lookup {"start", "middle", "end"}[anchor] raises KeyError otherwise
    assert_eq!(Anchor::parse("left").unwrap_err().kind, ErrorKind::Key);
}

#[test]
fn primitive_boxes_match_the_original() {
    for c in review()["box"].as_array().unwrap() {
        let a = &c["args"];
        let pad = f(&c["pad"]);
        let got = match c["kind"].as_str().unwrap() {
            "Rect" => rect_box(f(&a["x"]), f(&a["y"]), f(&a["w"]), f(&a["h"]), pad),
            "Use" => use_box(f(&a["x"]), f(&a["y"]), f(&a["size"]), pad),
            "Path" => points_box(&pts(&a["points"]), pad),
            k => panic!("unexpected kind {k}"),
        }
        .unwrap();
        assert_box(got, &c["expect"], c["kind"].as_str().unwrap());
    }
}

#[test]
fn overlap_has_positive_area_only() {
    for c in review()["overlap"].as_array().unwrap() {
        let b = |v: &Value| Bbox::new(f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3]));
        let (a, bb) = (b(&c["args"][0]), b(&c["args"][1]));
        assert_eq!(overlap(&a, &bb), c["expect"].as_bool().unwrap());
        assert_eq!(
            bounds_overlap(&a.to_bounds(), &bb.to_bounds()),
            overlap(&a, &bb)
        );
    }
}

fn check_curve(c: &Value, head_key: &str) -> Curve {
    let segs = c["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| [pt(&s[0]), pt(&s[1]), pt(&s[2])])
        .collect();
    let curve = Curve {
        start: pt(&c["start"]),
        segments: segs,
        head: c[head_key].as_array().map(|_| pts(&c[head_key])),
    };
    let classes: Vec<String> = c["classes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        serde_json::to_value(head_classes(&classes).unwrap()).unwrap(),
        c["head_classes"]
    );
    let got = curve.points();
    let want = pts(&c["points"]);
    assert_eq!(got.len(), want.len());
    assert!(
        got.iter()
            .zip(&want)
            .all(|(a, b)| close(a.x, b.x) && close(a.y, b.y))
    );
    curve
}

#[test]
fn curves_points_boxes_and_head_classes() {
    for c in review()["curve"].as_array().unwrap() {
        let a = &c["args"];
        let mut v = a.clone();
        v["points"] = c["points"].clone();
        v["head_classes"] = c["head_classes"].clone();
        let curve = check_curve(&v, "head");
        assert_box(curve.bbox(1.0).unwrap(), &c["box"], "Curve.box");
    }
    for c in own()["curves"].as_array().unwrap() {
        let curve = check_curve(c, "head");
        assert_box(curve.bbox(0.5).unwrap(), &c["box"], "Curve.box");
        // emit.py writes the shaft, then the head (if any) as a closed outline
        let lines = c["lines"].as_array().unwrap();
        let shaft = lines[0].as_str().unwrap();
        assert!(
            shaft.contains(&format!(" d=\"{}\"", path_data(&curve.shaft(), n))),
            "{shaft}"
        );
        match curve.head_path() {
            None => assert_eq!(lines.len(), 1),
            Some(h) => {
                assert_eq!(lines.len(), 2);
                assert!(
                    lines[1]
                        .as_str()
                        .unwrap()
                        .contains(&format!(" d=\"{}\"", path_data(&h, n)))
                );
            }
        }
    }
    assert_eq!(
        head_classes::<&str>(&[]).unwrap_err().kind,
        ErrorKind::Index
    );
}

#[test]
fn polylines_and_flat_bumps_curves_as_emit_writes_them() {
    for c in own()["paths"].as_array().unwrap() {
        let points = pts(&c["points"]);
        let commands = if c["curve"].as_str() == Some("flat") {
            flat_curve(&points).unwrap()
        } else {
            polyline(&points)
        };
        assert_eq!(path_data(&commands, n), c["d"].as_str().unwrap());
        match c["box"].as_array() {
            Some(_) => assert_box(points_box(&points, 1.0).unwrap(), &c["box"], "Path.box"),
            None => assert!(points_box(&points, 1.0).is_err()),
        }
    }
    assert_eq!(flat_curve(&[]).unwrap_err().kind, ErrorKind::Index);
}

fn shape_json(s: &Option<WedgeShape>) -> Value {
    match s {
        None => Value::Null,
        Some(WedgeShape::Circle { r, r0 }) => serde_json::json!(["circle", r, r0]),
        Some(WedgeShape::Sector { apex, outer, inner }) => serde_json::json!([
            "sector",
            [apex.x, apex.y],
            [outer.0, outer.1],
            inner.map(|(a, b)| vec![a, b])
        ]),
    }
}
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(p, q))
        }
        (Value::Number(_), Value::Number(_)) => close(f(a), f(b)),
        _ => a == b,
    }
}

#[test]
fn wedges_from_the_independent_fixtures() {
    for c in review()["wedge"].as_array().unwrap() {
        let a = &c["args"];
        let w = Wedge::new(
            f(&a["x"]),
            f(&a["y"]),
            f(&a["r"]),
            f(&a["a0"]),
            f(&a["a1"]),
            f(&a["r0"]),
            f(&a["inset"]),
        )
        .unwrap();
        let label = format!("{a}");
        assert!(
            same_json(&shape_json(&w.shape().unwrap()), &c["shape"]),
            "shape {label}"
        );
        assert_eq!(
            path_data(&w.path().unwrap(), n),
            c["path"].as_str().unwrap(),
            "path {label}"
        );
        assert_box(w.bbox(0.5).unwrap(), &c["box"], &label);
        let p0 = w.point(w.a0, None);
        let p1 = w.point(w.a1, Some(10.0));
        assert!(same_json(
            &serde_json::json!([[p0.x, p0.y], [p1.x, p1.y]]),
            &c["point"]
        ));
    }
}

#[test]
fn wedges_rings_insets_slivers_and_large_arcs() {
    for c in own()["wedges"].as_array().unwrap() {
        let a: Vec<f64> = c["args"].as_array().unwrap().iter().map(f).collect();
        let w = Wedge::new(a[0], a[1], a[2], a[3], a[4], a[5], a[6]).unwrap();
        let label = format!("{:?}", a);
        assert!(
            same_json(&shape_json(&w.shape().unwrap()), &c["shape"]["value"]),
            "shape {label}: {:?} vs {}",
            w.shape(),
            c["shape"]
        );
        assert_eq!(
            path_data(&w.path().unwrap(), n),
            c["path"]["value"].as_str().unwrap(),
            "path {label}"
        );
        assert_box(w.bbox(0.25).unwrap(), &c["box"]["value"], &label);
        for (angle, want) in [a[3], a[4], 0., 90., 123.4]
            .iter()
            .zip(c["points"].as_array().unwrap())
        {
            let p = w.point(*angle, None);
            assert!(
                same_json(&serde_json::json!([p.x, p.y]), want),
                "{label} point"
            );
        }
    }
    assert!(Wedge::new(0., 0., f64::NAN, 0., 1., 0., 0.).is_err());
}
