//! One round of a chart for a LaTeX document, for `chartgen.sty`'s shell-escape route.
//!
//! Usage: `chartgen-tex SPEC OPTIONS MEASUREMENTS OUT`
//!
//! Reads the chart specification (JSON), the document's options (`key=value` lines) and the
//! measurements so far (`id wd ht dp max` lines; the file may be missing in the first round),
//! and writes TeX to OUT for the document to `\input`: the labels to measure next, or the
//! finished PGF, or the reason the chart cannot be drawn. See `chartgen_tex`.
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [spec, options, measurements, out] = &args[..] else {
        eprintln!("usage: chartgen-tex SPEC OPTIONS MEASUREMENTS OUT");
        return ExitCode::from(2);
    };
    let read = |path: &str| std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"));
    let reply = match (read(spec), read(options)) {
        (Ok(spec), Ok(options)) => {
            let measurements = std::fs::read_to_string(measurements).unwrap_or_default();
            chartgen_tex::reply_tex(&spec, &options, &measurements)
        }
        (Err(error), _) | (_, Err(error)) => format!(
            "\\chartgen@status{{error}}\\chartgen@fail{{{}}}%\n",
            chartgen_tex::tex_message(&error)
        ),
    };
    match std::fs::write(out, reply) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("chartgen-tex: {out}: {error}");
            ExitCode::FAILURE
        }
    }
}
