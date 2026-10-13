//! The harness of the ruling tests: Juliet's example specs (`tests/conformance/examples/specs`),
//! loaded as the original's `read_file` merges them, then run through the chooser or rendered
//! end to end in the plain style. The tests themselves are in `tests/`, one file per topic,
//! each test naming the ruling it encodes as `docs/chartgen/RULINGS.md` lists it.
use chartgen::model::{Finding, LayoutConstraints, Measurements, Report, TextMetrics, Theme};
use chartgen::{RenderRequest, RenderStep, render_step};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance")
}

fn merge_patch(target: &Value, patch: &Value) -> Value {
    let Some(p) = patch.as_object() else {
        return patch.clone();
    };
    let mut out = target.as_object().cloned().unwrap_or_default();
    for (k, v) in p {
        if v.is_null() {
            out.remove(k);
        } else {
            let merged = merge_patch(out.get(k).unwrap_or(&Value::Null), v);
            out.insert(k.clone(), merged);
        }
    }
    Value::Object(out)
}

/// An example's spec (`name` without `.json`), with a variant's patch merged and the colour
/// registry merged under `colours.slots`.
pub fn spec(name: &str, variant: Option<&str>) -> Value {
    let path = root().join("examples/specs").join(format!("{name}.json"));
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("example spec"))
        .expect("example spec is JSON");
    let base = raw.get("base").cloned().unwrap_or_else(|| raw.clone());
    let mut spec = match variant {
        None => base,
        Some(v) => {
            let found = raw["variants"]
                .as_array()
                .and_then(|vs| vs.iter().find(|x| x["name"] == v))
                .unwrap_or_else(|| panic!("{name} has no variant {v}"));
            merge_patch(&base, &found["patch"])
        }
    };
    if let Some(reg) = spec["colours"]["registry"].as_str() {
        let reg: Value = serde_json::from_str(
            &std::fs::read_to_string(path.parent().unwrap().join(reg)).expect("registry"),
        )
        .unwrap();
        let mut slots = reg.as_object().cloned().unwrap();
        if let Some(own) = spec["colours"]["slots"].as_object() {
            slots.extend(own.clone());
        }
        spec["colours"]["slots"] = Value::Object(slots);
        spec["colours"].as_object_mut().unwrap().remove("registry");
    }
    spec
}

/// The chooser's report for an example: no fonts needed; enough for refusals and decisions.
pub fn choose(name: &str, variant: Option<&str>) -> Report {
    chartgen::choose(spec(name, variant), variant.map(String::from), Value::Null)
        .unwrap_or_else(|e| panic!("choose {name}: {e}"))
}

/// The outcome of rendering an example end to end.
pub struct Rendered {
    /// The SVG, when the chart was drawn.
    pub svg: Option<String>,
    pub report: Report,
}

type MeasureKey = (String, u64, bool, bool);

/// The prototype's own Typst measurements of every label it drew
/// (`tests/conformance/fixtures/oracle-measure.json`: `[typst text, size, weight, style]` →
/// `[width, ascent, descent]`), keyed by the Typst-escaped text, size, bold and italic.
fn oracle_measurements() -> &'static std::collections::HashMap<MeasureKey, (f64, f64, f64)> {
    static MAP: std::sync::OnceLock<std::collections::HashMap<MeasureKey, (f64, f64, f64)>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let raw: serde_json::Map<String, Value> = serde_json::from_str(
            &std::fs::read_to_string(root().join("fixtures/oracle-measure.json")).expect("oracle measurements"),
        )
        .expect("oracle measurements are JSON");
        raw.into_iter()
            .filter_map(|(k, v)| {
                let k: Value = serde_json::from_str(&k).ok()?;
                let text = k[0].as_str()?.to_string();
                let size = k[1].as_f64()?;
                let bold = k[2].as_str() == Some("bold");
                let italic = k[3].as_str() == Some("italic");
                let m = (v[0].as_f64()?, v[1].as_f64()?, v[2].as_f64()?);
                Some(((text, size.to_bits(), bold, italic), m))
            })
            .collect()
    })
}

/// A text request's metrics: the prototype's measurement where it measured that label (the
/// layout then sees the text as the prototype did), else a stand-in of half an em a character;
/// text wider than its maximum width is broken at spaces into lines, as a host would.
pub fn measure(r: &chartgen::model::TextRequest) -> TextMetrics {
    let text = r.content.readable();
    let key = (
        figkit_render::legacy::typst_escape(text),
        r.font_size.to_bits(),
        r.style.weight == "bold",
        r.style.font_style == "italic",
    );
    let one_line = |s: &str| -> (f64, f64, f64) {
        let k = (figkit_render::legacy::typst_escape(s), key.1, key.2, key.3);
        oracle_measurements().get(&k).copied().unwrap_or_else(|| {
            (0.5 * r.font_size * s.chars().count() as f64, 0.9 * r.font_size, 0.3 * r.font_size)
        })
    };
    let (width, ascent, descent) = one_line(text);
    match r.max_width {
        // Too wide for its room: the host breaks it at spaces, as a host typesetting it would,
        // and reports the block (its widest line, every line's height, no single baseline).
        Some(max) if width > max + 1e-6 => {
            let mut lines: Vec<String> = Vec::new();
            for word in text.split(' ') {
                match lines.last_mut() {
                    Some(line) if one_line(&format!("{line} {word}")).0 <= max + 1e-6 => {
                        line.push(' ');
                        line.push_str(word);
                    }
                    _ => lines.push(word.to_string()),
                }
            }
            let widest = lines.iter().map(|l| one_line(l).0).fold(0.0, f64::max);
            let leading = 1.2 * r.font_size;
            TextMetrics { width: widest, height: ascent + descent + leading * (lines.len() as f64 - 1.0), baseline: None }
        }
        _ => TextMetrics { width, height: ascent + descent, baseline: Some(ascent) },
    }
}

/// Renders an example in the plain style, or in the plain style with `tokens` overridden.
/// Text is measured as the prototype measured it where it can be (`measure`): rulings
/// concern decisions, labels and colours, never exact text metrics.
pub fn render_with(name: &str, variant: Option<&str>, tokens: &[(&str, &str)]) -> Rendered {
    render_spec(spec(name, variant), variant, tokens)
}

/// Renders a spec given directly (an example's spec changed by a test, say).
pub fn render_spec(spec: Value, variant: Option<&str>, tokens: &[(&str, &str)]) -> Rendered {
    let name = spec["id"].as_str().unwrap_or("spec").to_string();
    let width = spec.pointer("/layout/width").and_then(Value::as_f64).unwrap_or(368.0);
    let mut theme = Theme::default();
    for (k, v) in tokens {
        theme.tokens.insert((*k).into(), Value::String((*v).into()));
    }
    let context = "rulings".to_string();
    let mut req = RenderRequest {
        spec,
        theme,
        constraints: LayoutConstraints {
            width,
            height: None,
            measurement_context: context.clone(),
            max_rounds: 8,
            hooks: None,
        },
        measurements: Measurements { context, entries: Default::default() },
        backend: "svg".into(),
        variant: variant.map(String::from),
        expect: Value::Null,
        source_name: None,
    };
    for _ in 0..8 {
        match render_step(&req).unwrap_or_else(|e| panic!("render {name}: {e}")) {
            RenderStep::NeedMeasurements { requests, .. } => {
                for r in requests {
                    let m = measure(&r);
                    req.measurements.entries.insert(r.id, m);
                }
            }
            RenderStep::Complete { output, report, .. } => {
                return Rendered { svg: Some(output), report };
            }
            RenderStep::Stopped { report } => return Rendered { svg: None, report },
        }
    }
    panic!("render {name}: layout did not settle in 8 rounds")
}

pub fn render(name: &str, variant: Option<&str>) -> Rendered {
    render_with(name, variant, &[])
}

/// The findings of a report under one rule.
pub fn findings<'a>(report: &'a Report, rule: &str) -> Vec<&'a Finding> {
    report.findings.iter().filter(|f| f.rule == rule).collect()
}

/// The texts of a report's decisions under one rule.
pub fn decisions(report: &Report, rule: &str) -> Vec<String> {
    report
        .decisions
        .iter()
        .filter(|d| d["rule"] == rule)
        .map(|d| d["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// A string field of a report's summary (`scale`, `top`, `scale_title`, `orientation` …).
pub fn summary_str(report: &Report, key: &str) -> Option<String> {
    report.summary.get(key).and_then(Value::as_str).map(String::from)
}

/// The value-scale labels of a report's summary.
pub fn summary_labels(report: &Report) -> Vec<String> {
    report.summary.get("labels").and_then(Value::as_array).map_or_else(Vec::new, |a| {
        a.iter().map(|v| v.as_str().unwrap_or_default().to_string()).collect()
    })
}

/// The text elements of an SVG: (attributes, text) pairs, attributes by name.
pub fn svg_texts(svg: &str) -> Vec<(std::collections::BTreeMap<String, String>, String)> {
    let doc = roxmltree::Document::parse(svg).expect("the SVG parses");
    doc.descendants()
        .filter(|n| n.has_tag_name("text"))
        .map(|n| {
            let attrs = n.attributes().map(|a| (a.name().to_string(), a.value().to_string())).collect();
            let text: String = n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect();
            (attrs, text)
        })
        .collect()
}

/// The rect elements of an SVG, attributes by name.
pub fn svg_rects(svg: &str) -> Vec<std::collections::BTreeMap<String, String>> {
    let doc = roxmltree::Document::parse(svg).expect("the SVG parses");
    doc.descendants()
        .filter(|n| n.has_tag_name("rect"))
        .map(|n| n.attributes().map(|a| (a.name().to_string(), a.value().to_string())).collect())
        .collect()
}

/// WCAG contrast ratio between two #rrggbb colours.
pub fn contrast(a: &str, b: &str) -> f64 {
    fn lum(hex: &str) -> f64 {
        let h = hex.trim_start_matches('#');
        let c = |i: usize| {
            let v = u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * c(0) + 0.7152 * c(2) + 0.0722 * c(4)
    }
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}
