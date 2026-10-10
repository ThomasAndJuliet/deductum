// SPDX-License-Identifier: GPL-3.0-only

//! `ct-chartgen`: chartgen for the agents in threads, which write charts as
//! `chartgen` code blocks for the thread to draw.
//!
//! `ct-chartgen check [FILE]` lays a specification out, as the thread will,
//! and says which chart chartgen drew or why it refused; `schema` prints the
//! specification's JSON schema, and `guide` how threads show charts. Text
//! is measured as an average serif would be, which places labels a little
//! differently from the thread but decides the same charts.

use std::io::Read;
use std::process::ExitCode;

use chartgen::model::{LayoutConstraints, Measurements, Report, TextContent, TextMetrics};

const GUIDE: &str = include_str!("../thread/chartgen-guide.md");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => check(args.get(1).map(String::as_str)),
        Some("schema") => {
            println!("{}", serde_json::to_string_pretty(&chartgen::data::schema()).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Some("guide") => {
            print!("{GUIDE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: ct-chartgen check [FILE] | schema | guide");
            ExitCode::from(2)
        }
    }
}

fn check(path: Option<&str>) -> ExitCode {
    let mut text = String::new();
    let read = match path {
        Some(path) if path != "-" => std::fs::read_to_string(path).map(|t| text = t),
        _ => std::io::stdin().read_to_string(&mut text).map(|_| ()),
    };
    if let Err(err) = read {
        eprintln!("ct-chartgen: {err}");
        return ExitCode::from(2);
    }
    // A specification may come as the thread's code block, fences and all
    let spec = text.trim().trim_start_matches("```chartgen").trim_end_matches("```");
    let spec: serde_json::Value = match serde_json::from_str(spec) {
        Ok(spec) => spec,
        Err(err) => {
            println!("refused: the specification is not JSON: {err}");
            return ExitCode::FAILURE;
        }
    };
    let constraints = LayoutConstraints::default();
    let mut request = chartgen::RenderRequest {
        spec,
        theme: Default::default(),
        measurements: Measurements { context: constraints.measurement_context.clone(), ..Measurements::default() },
        constraints,
        backend: "svg".into(),
        variant: None,
        expect: serde_json::Value::Null,
        source_name: path.map(str::to_string),
    };
    for _ in 0..request.constraints.max_rounds.max(8) {
        let step = match chartgen::render_step(&request) {
            Ok(step) => step,
            Err(err) => {
                println!("refused: {err}");
                return ExitCode::FAILURE;
            }
        };
        match step {
            chartgen::RenderStep::NeedMeasurements { requests, .. } => {
                for text in requests {
                    let content = match &text.content {
                        TextContent::Plain { text } => text.as_str(),
                        TextContent::Native { alternative: Some(text), .. } => text.as_str(),
                        TextContent::Native { key, .. } => key.as_str(),
                    };
                    let width = content.chars().count() as f64 * text.font_size * 0.5;
                    request.measurements.entries.insert(
                        text.id.clone(),
                        TextMetrics { width, height: text.font_size * 1.2, baseline: Some(text.font_size * 0.92) },
                    );
                }
            }
            chartgen::RenderStep::Complete { scene, report, .. } => {
                println!(
                    "ok: a {} chart, {:.0} × {:.0} pt",
                    report.chart_type.as_deref().unwrap_or("chartgen"),
                    scene.width,
                    scene.height
                );
                print_findings(&report, false);
                return ExitCode::SUCCESS;
            }
            chartgen::RenderStep::Stopped { report } => {
                println!("refused: chartgen did not draw this chart ({})", report.status);
                print_findings(&report, true);
                return ExitCode::FAILURE;
            }
        }
    }
    println!("refused: chartgen needed more rounds of text measurement than it is given");
    ExitCode::FAILURE
}

/// The report's findings: all of them for a refusal, else what is not merely
/// informative
fn print_findings(report: &Report, all: bool) {
    for finding in &report.findings {
        if all || finding.severity != "info" {
            println!("{} {}: {}", finding.severity, finding.rule, finding.message);
        }
    }
}
