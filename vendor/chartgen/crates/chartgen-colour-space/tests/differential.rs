//! chartgen-colour-space against the original figkit style.py and okhsl.py: the fixture holds the
//! original's results for edge-case and random colours (tests/fixtures/generate.py).
use chartgen_colour_space::*;
use serde_json::Value;

fn fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/colour_space.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("bundled fixture")).unwrap()
}
fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1. + b.abs())
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

#[test]
fn single_colours_match_the_original() {
    let fx = fixture();
    let cases = fx["hex"].as_array().unwrap();
    assert!(cases.len() > 100);
    for c in cases {
        let h = s(&c["h"]);
        let rgb = hex_rgb(h).unwrap();
        for i in 0..3 {
            assert!(close(rgb[i], f(&c["rgb"][i])), "{h} rgb");
        }
        let l = oklch(h).unwrap();
        for i in 0..3 {
            // the hue of a grey is numerical noise: compare it only where there is chroma
            if i == 2 && l[1] < 1e-6 {
                continue;
            }
            assert!(
                close(l[i], f(&c["oklch"][i])),
                "{h} oklch {i}: {} vs {}",
                l[i],
                c["oklch"][i]
            );
        }
        assert!(
            close(luminance(h).unwrap(), f(&c["luminance"])),
            "{h} luminance"
        );
        for sh in c["shift"].as_array().unwrap() {
            assert_eq!(
                oklch_shift(h, f(&sh[0]), f(&sh[1])).unwrap(),
                s(&sh[2]),
                "{h} shift {sh}"
            );
        }
        for li in c["light"].as_array().unwrap() {
            assert_eq!(
                oklch_light(h, f(&li[0]), li[1].as_f64()).unwrap(),
                s(&li[2]),
                "{h} light {li}"
            );
        }
        let o = rgb.map(|x| x);
        let k = srgb_to_okhsl(o[0], o[1], o[2]);
        for i in 0..3 {
            if i == 0 && k[1] < 1e-6 {
                continue;
            }
            assert!(
                (k[i] - f(&c["okhsl"][i])).abs() < 1e-7,
                "{h} okhsl {i}: {} vs {}",
                k[i],
                c["okhsl"][i]
            );
        }
    }
}

#[test]
fn pairs_match_the_original() {
    let fx = fixture();
    for c in fx["pairs"].as_array().unwrap() {
        let (a, b) = (s(&c["a"]), s(&c["b"]));
        assert_eq!(mix(a, b, f(&c["t"])).unwrap(), s(&c["mix"]), "mix {a} {b}");
        assert!(
            close(contrast(a, b).unwrap(), f(&c["contrast"])),
            "contrast {a} {b}"
        );
        let (tone, k) = text_tone(&[a, b], "#000000", "#ffffff").unwrap();
        assert_eq!(tone, s(&c["tone"][0]));
        assert!(close(k, f(&c["tone"][1])));
        for (name, m) in CVD {
            assert!(
                close(cvd_distance(a, b, &m).unwrap(), f(&c["cvd"][name])),
                "cvd {name} {a} {b}"
            );
        }
    }
}

#[test]
fn gamut_reduction_and_okhsl_match_the_original() {
    let fx = fixture();
    for c in fx["oklch"].as_array().unwrap() {
        assert_eq!(
            from_oklch(f(&c["l"]), f(&c["c"]), f(&c["h"])),
            s(&c["hex"]),
            "{c}"
        );
    }
    for c in fx["okhsl"].as_array().unwrap() {
        assert_eq!(
            okhsl(f(&c["h"]), f(&c["s"]), f(&c["l"])),
            s(&c["hex"]),
            "{c}"
        );
        let rgb = okhsl_to_srgb(f(&c["h"]), f(&c["s"]), f(&c["l"]));
        for i in 0..3 {
            assert!((rgb[i] - f(&c["rgb"][i])).abs() < 1e-9, "{c}");
        }
    }
    for c in fx["rgb_hex"].as_array().unwrap() {
        let v = f(&c["v"]);
        assert_eq!(rgb_hex([v, v, v]), s(&c["hex"]), "{c}");
    }
}
