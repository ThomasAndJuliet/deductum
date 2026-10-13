//! Differential tests: every expected value was produced by the original figkit `style`,
//! `okhsl.py` and bake `label_style` (tests/conformance/figkit/fixtures.json from the
//! independent review's generator; tests/fixtures/style.json from tests/fixtures/generate.py).
use chartgen_style::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn load(path: &str) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("fixture {path}: {e}"));
    serde_json::from_str(&text).unwrap()
}
fn review() -> Value {
    load(&format!("{ROOT}/tests/conformance/figkit/fixtures.json"))
}
fn own() -> Value {
    load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/style.json"
    ))
}
fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| v.to_string().parse().unwrap())
}
fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(_), Value::Number(_)) => {
            let (x, y) = (f(a), f(b));
            (x - y).abs() <= 1e-9 * (1. + x.abs().max(y.abs()))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| close(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w)))
        }
        _ => a == b,
    }
}
fn exception(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Value => "ValueError",
        ErrorKind::Index => "IndexError",
        ErrorKind::Key => "KeyError",
        ErrorKind::Attribute => "AttributeError",
        ErrorKind::Source => "OSError",
    }
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn colour_call(name: &str, a: &[Value]) -> Value {
    use chartgen_style::colour::*;
    let arr = |x: [f64; 3]| json!(x);
    match name {
        "hex_rgb" => json!(hex_rgb(s(&a[0])).map(arr)),
        "rgb_hex" => {
            let v: Vec<f64> = a[0].as_array().unwrap().iter().map(f).collect();
            json!(rgb_hex([v[0], v[1], v[2]]))
        }
        "oklch" => json!(oklch(s(&a[0])).map(arr)),
        "from_oklch" => json!(from_oklch(f(&a[0]), f(&a[1]), f(&a[2]))),
        "mix" => json!(mix(s(&a[0]), s(&a[1]), f(&a[2]))),
        "oklch_shift" => json!(oklch_shift(s(&a[0]), f(&a[1]), f(&a[2]))),
        "oklch_light" => json!(oklch_light(s(&a[0]), f(&a[1]), a.get(2).map(f))),
        "luminance" => json!(luminance(s(&a[0]))),
        "contrast" => json!(contrast(s(&a[0]), s(&a[1]))),
        "text_tone" => {
            let fills: Vec<&str> = match &a[0] {
                Value::String(x) => vec![x.as_str()],
                v => v.as_array().unwrap().iter().map(s).collect(),
            };
            json!(text_tone(&fills, "#000000", "#ffffff").map(|(t, k)| json!([t, k])))
        }
        "okhsl" => json!(okhsl(f(&a[0]), f(&a[1]), f(&a[2]))),
        "srgb_to_okhsl" => arr(srgb_to_okhsl(f(&a[0]), f(&a[1]), f(&a[2]))),
        "okhsl_to_srgb" => arr(okhsl_to_srgb(f(&a[0]), f(&a[1]), f(&a[2]))),
        _ => panic!("unknown colour function {name}"),
    }
}

#[test]
fn colour_functions_and_okhsl_match_the_original() {
    let fx = review();
    let mut n = 0;
    for (name, cases) in fx["colour"].as_object().unwrap() {
        for c in cases.as_array().unwrap() {
            let got = colour_call(name, c["args"].as_array().unwrap());
            assert!(
                close(&got, &c["expect"]),
                "{name}{}: {got} != {}",
                c["args"],
                c["expect"]
            );
            n += 1;
        }
    }
    assert!(n > 500);
}

/// The snapshot of a loaded style in the fixtures' terms.
fn snapshot(case: &Value, maker: &str) -> Value {
    let files: BTreeMap<String, String> = case["files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), s(v).to_string()))
        .collect();
    let entry = s(&case["entry"]);
    let name = entry.trim_end_matches(".css");
    let mut source = MemorySource { sheets: files };
    match Style::load(name, maker, entry, &mut source) {
        Err(e) => json!({"error": {"type": exception(e.kind), "message": e.message}}),
        Ok(st) => {
            let mut metric = serde_json::Map::new();
            for (k, _) in st.tokens.iter() {
                metric.insert(
                    k.into(),
                    match st.metric(k) {
                        Ok(v) => json!(v),
                        Err(e) => json!({"error": e.message}),
                    },
                );
            }
            json!({"expect": {
                "name": st.name,
                "tokens": st.tokens.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
                "css": st.css,
                "defs": st.defs,
                "metric": metric,
                "rules": st.rules,
            }})
        }
    }
}

#[test]
fn stylesheets_of_the_independent_fixtures() {
    for case in review()["style"].as_array().unwrap() {
        let mut got = snapshot(case, "figkit");
        let want = if case.get("error").is_some() {
            json!({"error": case["error"]})
        } else {
            let mut w = json!({"expect": case["expect"]});
            // the fixture keeps tokens as a JSON object; compare them as a mapping, order by css
            let tokens = got["expect"]["tokens"].as_array().unwrap().clone();
            got["expect"]["tokens"] = Value::Object(
                tokens
                    .iter()
                    .map(|p| (s(&p[0]).to_string(), p[1].clone()))
                    .collect(),
            );
            w["expect"]["tokens"] = case["expect"]["tokens"].clone();
            w
        };
        assert!(
            close(&got, &want),
            "{}:\n got {got:#}\nwant {want:#}",
            case["case"]
        );
    }
}

#[test]
fn stylesheet_edge_cases_match_the_original() {
    for case in own()["styles"].as_array().unwrap() {
        let got = snapshot(case, "figkit");
        let want = if case.get("error").is_some() {
            json!({"error": case["error"]})
        } else {
            json!({"expect": case["expect"]})
        };
        assert!(
            close(&got, &want),
            "{}:\n got {got:#}\nwant {want:#}",
            case["case"]
        );
    }
}

#[test]
fn the_maker_names_the_generator_in_the_first_line() {
    let mut src = MemorySource::new([("s.css", ":root { --a: 1; }")]);
    let st = Style::load("s", "chartgen", "s.css", &mut src).unwrap();
    assert!(
        st.css.starts_with(
            "/* Flattened by chartgen from the s stylesheet: literal values only */\n"
        )
    );
    // figkit Style.metric: a missing token is a KeyError with the original's message
    let e = st.metric("--b").unwrap_err();
    assert_eq!(
        (e.kind, e.message.as_str()),
        (ErrorKind::Key, "style s has no token --b")
    );
}

fn label(css: &str, case: &Value) -> Value {
    let classes: Vec<&str> = case["classes"].as_array().unwrap().iter().map(s).collect();
    let own: BTreeMap<String, String> = case
        .get("own")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), s(v).to_string()))
                .collect()
        })
        .unwrap_or_default();
    match label_style(css, &classes, &own) {
        Ok(l) => json!({"value": [l.size, l.fill, l.weight, l.slant]}),
        Err(e) => json!({"error": {"type": exception(e.kind), "message": e.message}}),
    }
}

#[test]
fn label_style_of_the_original_styles() {
    let mut css = BTreeMap::new();
    for case in review()["label_style"].as_array().unwrap() {
        let name = s(&case["style"]).to_string();
        let flat = css.entry(name.clone()).or_insert_with(|| {
            let dir = format!("{ROOT}/python/chartgen/styles");
            let sheets = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap())
                .map(|e| {
                    (
                        e.file_name().into_string().unwrap(),
                        std::fs::read_to_string(e.path()).unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            let mut src = MemorySource::new(sheets);
            Style::load(&name, "figkit", &format!("{name}.css"), &mut src)
                .unwrap()
                .css
        });
        let got = label(flat, case);
        assert!(
            close(&got, &json!({"value": case["expect"]})),
            "{name} {}: {got}",
            case["classes"]
        );
    }
}

#[test]
fn label_style_edge_cases_match_bake() {
    let fx = own();
    let css = s(&fx["label_css"]);
    for case in fx["label_style"].as_array().unwrap() {
        let got = label(css, case);
        let mut want = case.clone();
        want.as_object_mut().unwrap().remove("classes");
        want.as_object_mut().unwrap().remove("own");
        assert!(close(&got, &want), "{}: {got} != {want}", case["classes"]);
    }
}

#[test]
fn num_comments_and_parse_match_the_original() {
    let fx = own();
    for c in fx["num"].as_array().unwrap() {
        let x = match &c[0] {
            Value::String(t) => t.parse::<f64>().unwrap(),
            v => f(v),
        };
        assert_eq!(num(x), s(&c[1]), "{x}");
    }
    for c in fx["strip_comments"].as_array().unwrap() {
        assert_eq!(strip_comments(s(&c[0])), s(&c[1]));
    }
    for c in fx["parse"].as_array().unwrap() {
        assert_eq!(json!(parse(s(&c[0]))), c[1]);
    }
}

#[test]
fn theme_carries_the_label_size_paints_and_palette() {
    let mut src = MemorySource::new([(
        "t.css",
        ":root { --label-size: 9.5px; --text: #111111; --page: ; --series-2: #222222; }",
    )]);
    let theme = Style::load("t", "chartgen", "t.css", &mut src)
        .unwrap()
        .theme()
        .unwrap();
    assert_eq!(theme.font_size, 9.5);
    assert_eq!(theme.text, "#111111");
    assert_eq!(theme.background, "#ffffff");
    assert_eq!(theme.grid, "#dddddd");
    assert_eq!(theme.palette.len(), 16);
    assert_eq!(theme.palette[1], "#222222");
    assert_eq!(theme.palette[0], "");
}
