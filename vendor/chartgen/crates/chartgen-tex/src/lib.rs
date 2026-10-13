//! Charts set inside a LaTeX document, the TeX host's half of the render protocol.
//!
//! `chartgen.sty` draws a chart where `\chartgen{spec.json}` stands, by running the engine
//! during compilation: in process through the LuaTeX module (`chartgen-luatex`), or as the
//! `chartgen-tex` program through shell escape. Both drive this adapter, one round at a time:
//!
//! 1. [`prepare`] reads the specification as authored. A label given as
//!    `{"typst": …, "latex": …, "alternative": …}` keeps its LaTeX markup, under a key the
//!    PGF output refers to (`\chartgenUseNative{key}`); other hosts' markup gives way to the
//!    alternative.
//! 2. [`round`] asks the engine for the chart with the measurements so far. Either it needs
//!    more labels measured, and the round is TeX that declares the native labels and sets
//!    each label as the PGF output will (`figkit_render::legacy::pgf_label`) for the document
//!    to measure where the chart stands; or the round is the finished PGF.
//! 3. The document records each label's box (`id wd ht dp max`, in scaled points) and the
//!    next round reads them with [`parse_measurements`].
//!
//! Every input and output is text, so the in-process and shell routes share all of it and
//! keep no state between rounds.
use chartgen::{RenderRequest, RenderStep};
use chartgen_model::{TextContent, TextRequest, TextStyle};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fmt::Write;

/// TeX scaled points to the engine's physical points (1/72 inch, TeX's `bp`)
const SP_TO_BP: f64 = 72.0 / 72.27 / 65536.0;

/// What the document says about one chart
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Distinguishes the chart's native label keys from other charts' in the document
    pub id: String,
    /// Figure width in bp; the specification's own layout width when none
    pub width: Option<f64>,
    /// The width when neither the document nor the specification gives one: the line width
    pub default_width: Option<f64>,
    pub height: Option<f64>,
    /// The font environment the labels are measured and set in, so that measurements are
    /// never reused across a change of fonts
    pub context: String,
    /// The theme as JSON (the engine's `Theme`); the engine's default, `plain`, when none
    pub theme: Option<String>,
    pub variant: Option<String>,
    pub max_rounds: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            id: "chart".into(),
            width: None,
            default_width: None,
            height: None,
            context: "latex".into(),
            theme: None,
            variant: None,
            max_rounds: 8,
        }
    }
}

impl Options {
    /// Options from `key=value` lines, as the TeX package writes them
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut options = Self::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("not a key=value option: {line}"))?;
            let (key, value) = (key.trim(), value.trim());
            let number = |v: &str| {
                v.parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite() && *n > 0.0)
                    .ok_or_else(|| format!("{key} must be a positive number of bp, not {v}"))
            };
            match key {
                "id" => options.id = value.into(),
                "width" => options.width = Some(number(value)?),
                "default_width" => options.default_width = Some(number(value)?),
                "height" => {
                    options.height = (!value.is_empty()).then(|| number(value)).transpose()?
                }
                "context" => options.context = value.into(),
                "theme" => options.theme = (!value.is_empty()).then(|| value.into()),
                // A theme file, relative to the document's directory
                "theme_file" if !value.is_empty() => {
                    options.theme = Some(
                        std::fs::read_to_string(value)
                            .map_err(|e| format!("cannot read the chart theme {value}: {e}"))?,
                    )
                }
                "theme_file" => {}
                "variant" => options.variant = (!value.is_empty()).then(|| value.into()),
                "max_rounds" => {
                    options.max_rounds = value
                        .parse()
                        .map_err(|_| format!("max_rounds must be a whole number, not {value}"))?
                }
                _ => return Err(format!("unknown chart option: {key}")),
            }
        }
        if !valid_key(&options.id) {
            return Err(format!(
                "chart id {:?} may only use letters, digits, -, _ and :",
                options.id
            ));
        }
        Ok(options)
    }
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b))
}

/// A chart ready for rounds: the engine's request, and the LaTeX markup of its native labels
#[derive(Debug, Clone)]
pub struct Chart {
    request: Value,
    /// Native label keys and their LaTeX markup, in the order they occur in the spec
    pub native: Vec<(String, String)>,
}

/// Whether a value is a label given for several hosts
fn is_native(value: &Map<String, Value>) -> bool {
    value.contains_key("alternative")
        && value.len() > 1
        && value
            .keys()
            .all(|k| matches!(k.as_str(), "typst" | "latex" | "alternative"))
}

/// The specification as LaTeX sees it: LaTeX markup becomes a native key, other hosts'
/// markup the plain alternative
fn for_latex(value: Value, prefix: &str, native: &mut Vec<(String, String)>) -> Value {
    match value {
        Value::Object(map) if is_native(&map) => match map.get("latex").and_then(Value::as_str) {
            Some(markup) => {
                let key = format!("{prefix}-n{}", native.len() + 1);
                native.push((key.clone(), markup.to_string()));
                json!({"latex": key, "alternative": map["alternative"]})
            }
            None => map["alternative"].clone(),
        },
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, for_latex(v, prefix, native)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|v| for_latex(v, prefix, native))
                .collect(),
        ),
        other => other,
    }
}

/// Read a specification and the document's options into a chart
pub fn prepare(spec: &str, options: &Options) -> Result<Chart, String> {
    let spec: Value = serde_json::from_str(spec)
        .map_err(|e| format!("the chart specification is not JSON: {e}"))?;
    if !spec.is_object() {
        return Err("a chart specification must be a JSON object".into());
    }
    let mut native = Vec::new();
    let spec = for_latex(spec, &options.id, &mut native);
    let layout = spec.get("layout").cloned().unwrap_or(Value::Null);
    let width = options
        .width
        .or_else(|| layout.get("width").and_then(Value::as_f64))
        .or(options.default_width)
        .unwrap_or(368.0);
    let height = options
        .height
        .or_else(|| layout.get("height").and_then(Value::as_f64));
    let mut request = json!({
        "spec": spec,
        "backend": "pgf",
        "variant": options.variant,
        "constraints": {"width": width, "height": height,
            "measurement_context": options.context, "max_rounds": options.max_rounds},
        "measurements": {"context": options.context, "entries": {}},
    });
    if let Some(theme) = &options.theme {
        let theme: Value =
            serde_json::from_str(theme).map_err(|e| format!("the chart theme is not JSON: {e}"))?;
        request["theme"] = theme;
    }
    Ok(Chart { request, native })
}

/// One round's outcome
#[derive(Debug, Clone, PartialEq)]
pub enum Round {
    /// TeX that declares the native labels and has the document measure each label
    Measure { tex: String, round: u32 },
    /// TeX that declares the native labels, then the chart's PGF; and any warnings
    Done { tex: String, warnings: Vec<String> },
}

/// Measurements from `id wd ht dp max` lines: TeX box dimensions in scaled points, and the
/// label's maximum width in bp or `-`. A paragraph is exactly its maximum width; TeX's
/// rounding to scaled points is not allowed to exceed it
pub fn parse_measurements(text: &str) -> Result<BTreeMap<String, Value>, String> {
    let mut entries = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [id, wd, ht, dp, max] = fields[..] else {
            return Err(format!("not a measurement: {line}"));
        };
        let sp = |v: &str| {
            v.trim_end_matches("sp")
                .parse::<i64>()
                .map_err(|_| format!("not a dimension in sp: {v}"))
        };
        let (wd, ht, dp) = (sp(wd)?, sp(ht)?, sp(dp)?);
        let mut width = wd as f64 * SP_TO_BP;
        if max != "-" {
            let max: f64 = max.parse().map_err(|_| format!("not a width: {max}"))?;
            width = width.min(max);
        }
        entries.insert(
            id.to_string(),
            json!({"width": width, "height": (ht + dp) as f64 * SP_TO_BP,
                "baseline": ht as f64 * SP_TO_BP}),
        );
    }
    Ok(entries)
}

/// TeX for a message inside a macro argument: no special characters
pub fn tex_message(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\\' | '{' | '}' | '%' | '#' | '&' | '$' | '^' | '_' | '~' => ' ',
            '\n' => ' ',
            c => c,
        })
        .collect()
}

fn declarations(chart: &Chart) -> String {
    let mut out = String::new();
    for (key, markup) in &chart.native {
        writeln!(out, "\\chartgenDeclareNative{{{key}}}{{{markup}}}%").unwrap();
    }
    out
}

fn messages(report: &chartgen_model::Report, severities: &[&str]) -> Vec<String> {
    report
        .findings
        .iter()
        .filter(|f| severities.contains(&f.severity.as_str()))
        .map(|f| format!("{} ({})", f.message, f.rule))
        .collect()
}

/// Ask the engine for the chart, given the measurements so far
pub fn round(chart: &Chart, measurements: &BTreeMap<String, Value>) -> Result<Round, String> {
    let mut request = chart.request.clone();
    request["measurements"]["entries"] = json!(measurements);
    let request: RenderRequest =
        serde_json::from_value(request).map_err(|e| format!("bad chart request: {e}"))?;
    match chartgen::render_step(&request).map_err(|e| e.to_string())? {
        RenderStep::NeedMeasurements {
            requests, round, ..
        } => {
            if round >= request.constraints.max_rounds {
                return Err(
                    "the chart's layout did not settle within its measurement rounds".into(),
                );
            }
            let mut tex = declarations(chart);
            for request in &requests {
                tex.push_str(&measure_tex(request)?);
            }
            Ok(Round::Measure { tex, round })
        }
        RenderStep::Complete { output, report, .. } => {
            let mut tex = declarations(chart);
            tex.push_str(&output);
            Ok(Round::Done {
                tex,
                warnings: messages(&report, &["warning"]),
            })
        }
        RenderStep::Stopped { report } => {
            let reasons = messages(&report, &["error"]);
            Err(if reasons.is_empty() {
                format!("the engine refused the chart ({})", report.status)
            } else {
                format!("the engine refused the chart: {}", reasons.join("; "))
            })
        }
    }
}

/// `\chartgen@measure{id}{max}{body}`: the label set exactly as the PGF output sets it
fn measure_tex(request: &TextRequest) -> Result<String, String> {
    if !valid_key(&request.id) {
        return Err(format!("unexpected label id {:?}", request.id));
    }
    // Ink does not change a label's box
    let style = TextStyle {
        fill: None,
        ..request.style.clone()
    };
    let body = figkit_render::legacy::pgf_label(
        &request.content,
        request.font_size,
        &style,
        request.max_width,
    )
    .map_err(|e| e.to_string())?;
    if let TextContent::Native {
        host,
        alternative: None,
        ..
    } = &request.content
        && host != "latex"
        && host != "tex"
    {
        return Err(format!(
            "a {host} label has no LaTeX form or plain alternative"
        ));
    }
    let max = request.max_width.map_or("-".to_string(), |m| m.to_string());
    Ok(format!(
        "\\chartgen@measure{{{}}}{{{max}}}{{{body}}}%\n",
        request.id
    ))
}

/// A whole round as one text reply, as the shell route's program and the Lua module return
/// it: `measure` or `done` on the first line, then TeX; or `error` and a message
pub fn reply(spec: &str, options: &str, measurements: &str) -> String {
    let result = Options::parse(options)
        .and_then(|options| prepare(spec, &options))
        .and_then(|chart| Ok((chart, parse_measurements(measurements)?)))
        .and_then(|(chart, entries)| round(&chart, &entries));
    match result {
        Ok(Round::Measure { tex, .. }) => format!("measure\n{tex}"),
        Ok(Round::Done { tex, warnings }) => {
            let mut out = String::from("done\n");
            for warning in warnings {
                writeln!(out, "\\chartgen@warning{{{}}}%", tex_message(&warning)).unwrap();
            }
            out.push_str(&tex);
            out
        }
        Err(message) => format!("error\n{}", tex_message(&message)),
    }
}

/// [`reply`] as a file for TeX to `\\input`: `\\chartgen@status{measure}` or `{done}`
/// before the round's TeX, or `\\chartgen@status{error}` and `\\chartgen@fail{message}`
pub fn reply_tex(spec: &str, options: &str, measurements: &str) -> String {
    let reply = reply(spec, options, measurements);
    let (status, body) = reply.split_once('\n').unwrap_or((&reply, ""));
    if status == "error" {
        format!("\\chartgen@status{{error}}\\chartgen@fail{{{body}}}%\n")
    } else {
        format!("\\chartgen@status{{{status}}}%\n{body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic counts with LaTeX markup in the title and one category
    const SPEC: &str = include_str!("../tests/fixtures/native-labels.json");
    const REFUSED: &str = include_str!("../tests/fixtures/missing-job.json");

    #[test]
    fn options_parse_and_reject() {
        let options =
            Options::parse("id=fig-1\nwidth=345.0\nheight=\ncontext=latex:cmr:10\nmax_rounds=4\n")
                .unwrap();
        assert_eq!(options.id, "fig-1");
        assert_eq!(options.width, Some(345.0));
        assert_eq!(options.height, None);
        assert_eq!(options.max_rounds, 4);
        assert!(Options::parse("width=wide").is_err());
        assert!(Options::parse("colour=red").is_err());
        assert!(Options::parse("id=a b").is_err());
    }

    #[test]
    fn the_width_is_the_documents_then_the_specs_then_the_lines() {
        let width = |options: &str, spec: &str| {
            prepare(spec, &Options::parse(options).unwrap())
                .unwrap()
                .request["constraints"]["width"]
                .as_f64()
        };
        assert_eq!(width("width=300\ndefault_width=340", SPEC), Some(300.0));
        assert_eq!(width("default_width=340", SPEC), Some(480.0));
        assert_eq!(width("default_width=340", "{}"), Some(340.0));
        assert_eq!(width("", "{}"), Some(368.0));
    }

    #[test]
    fn latex_markup_becomes_native_keys_and_typst_gives_way() {
        let chart = prepare(
            SPEC,
            &Options {
                id: "c1".into(),
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(
            chart.native,
            [
                (
                    "c1-n1".to_string(),
                    "Counts by Site ($n = 60$)".to_string()
                ),
                ("c1-n2".to_string(), "\\textsc{Site B}".to_string())
            ]
        );
        let spec = &chart.request["spec"];
        assert_eq!(
            spec["title"]["text"],
            json!({"latex": "c1-n1", "alternative": "Counts by Site (n = 60)"})
        );
        assert!(!spec.to_string().contains("smallcaps"));
    }

    #[test]
    fn measurements_convert_scaled_points_and_clamp_paragraphs() {
        let entries =
            parse_measurements("a 6578176 459459 127431 -\nb 6578177 100 0 100\n").unwrap();
        let a = &entries["a"];
        // 100.375 pt is exactly 100 bp
        assert!((a["width"].as_f64().unwrap() - 100.0).abs() < 1e-9);
        assert!((a["baseline"].as_f64().unwrap() - 459459.0 * SP_TO_BP).abs() < 1e-9);
        assert_eq!(entries["b"]["width"].as_f64(), Some(100.0));
        assert!(parse_measurements("a 1 2").is_err());
    }

    /// Answer measurement rounds with fixed metrics, as a stand-in for TeX
    fn fake_measure(tex: &str) -> String {
        tex.lines()
            .filter_map(|l| l.strip_prefix("\\chartgen@measure{"))
            .map(|rest| {
                let (id, rest) = rest.split_once('}').unwrap();
                let max = rest.trim_start_matches('{').split('}').next().unwrap();
                format!("{id} 1966080 458752 131072 {max}\n")
            })
            .collect()
    }

    #[test]
    fn rounds_end_in_pgf_that_uses_the_declared_labels() {
        let options = "id=c1\nwidth=300";
        let mut measured = String::new();
        let mut rounds = 0;
        let pgf = loop {
            let reply = reply(SPEC, options, &measured);
            let (status, tex) = reply.split_once('\n').unwrap();
            match status {
                "measure" => {
                    assert!(
                        tex.contains("\\chartgenDeclareNative{c1-n2}{\\textsc{Site B}}")
                    );
                    assert!(tex.contains("\\chartgen@measure{"));
                    measured.push_str(&fake_measure(tex));
                }
                "done" => break tex.to_string(),
                _ => panic!("{reply}"),
            }
            rounds += 1;
            assert!(rounds < 8);
        };
        assert!(pgf.contains("\\chartgenDeclareNative{c1-n1}"));
        assert!(pgf.contains("\\chartgenUseNative{c1-n2}"));
        assert!(pgf.contains("\\begin{pgfpicture}") || pgf.contains("pgfpicture"));
    }

    #[test]
    fn replies_for_tex_name_their_status_first() {
        let first = reply_tex(SPEC, "id=c1", "");
        assert!(
            first.starts_with("\\chartgen@status{measure}%\n"),
            "{first}"
        );
        let failed = reply_tex("[]", "", "");
        assert_eq!(
            failed,
            "\\chartgen@status{error}\\chartgen@fail{a chart specification must be a JSON object}%\n"
        );
    }

    #[test]
    fn refusals_and_bad_input_are_errors_for_tex() {
        let reply = reply(REFUSED, "", "");
        assert!(reply.starts_with("error\n"), "{reply}");
        assert!(!reply[6..].contains(['\\', '{', '}']));
        assert!(
            super::reply("not json", "", "")
                .starts_with("error\nthe chart specification is not JSON")
        );
    }
}
