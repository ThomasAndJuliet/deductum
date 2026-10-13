//! Refusals and findings that concern the spec whatever the type, and the type's own refusals:
//! `__init__._semantic`, `_given_in`, `censored_refusal`, `_type_refusals` and `_stage_notes`.
use crate::rules::{error, finding, info, refusal};
use crate::scale::{scale_dec, theoretical_limits};
use crate::view::{View, dec, plain};
use chartgen_format as fmt;
use chartgen_model::{Decimal, PreparedData, Report};
use chartgen_scales::LENGTH_ENCODED;
use regex::RegexBuilder;
use serde_json::Value;

const INSIGHT_JOBS: [&str; 3] = ["answer", "persuade", "make_aware"];
fn job_verb(kind: &str) -> String {
    match kind {
        "answer" => "answer a question",
        "persuade" => "persuade",
        "make_aware" => "make the reader aware",
        "explain" => "explain",
        "other" => "show something",
        k => k,
    }
    .into()
}

/// `__init__._semantic`: refusals that concern the spec whatever the type (SPEC §3.1 step 3).
/// Returns true when the chart must not be drawn.
pub fn semantic(v: &View, report: &mut Report) -> bool {
    let purpose = v.purpose();
    let mut stop = false;
    let job = v.job();
    let has_job = !v.get("/job").is_null();
    if purpose == "communication" && !has_job {
        stop |= refusal(
            report,
            v,
            "R0-07",
            "a communication chart needs its job: the question it answers, the problem it shows, the action it urges, or lookup",
            Some("job"),
            Some("{\"kind\": \"answer\", \"text\": \"…\"}"),
        );
    }
    let title = v.get("/title");
    if purpose == "communication" && (title.is_null() || title["text"].is_null()) {
        stop |= refusal(
            report,
            v,
            "R2-12",
            "a communication chart needs a title saying what, by what and when (the generator never writes prose)",
            Some("title.text"),
            Some("{\"text\": \"…\", \"kind\": \"descriptive\"}"),
        );
    }
    let stated = v.insight_important().is_some();
    let callout = v.get("/comments").as_array().is_some_and(|cs| {
        cs.iter().any(|c| {
            c["answers"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == "important"))
        })
    });
    let title_kind = if title.is_null() {
        None
    } else {
        Some(title["kind"].as_str().unwrap_or("descriptive"))
    };
    if purpose == "communication"
        && (job.is_some_and(|k| INSIGHT_JOBS.contains(&k))
            || (job.is_some_and(|k| k == "explain" || k == "other") && stated))
        && title_kind != Some("insight")
        && !callout
    {
        let kind = job.unwrap_or("");
        stop |= refusal(
            report,
            v,
            "R0-10",
            format!(
                "a chart whose job is to {} states its insight; this title only names the data",
                job_verb(kind)
            ),
            Some("title.kind"),
            Some(
                "an insight title (title.kind insight, the descriptive line under it); a callout (comments[], answers: [important]); or job.kind lookup if the chart is for looking values up",
            ),
        );
    }
    if v.get("/chart/gridlines") == &Value::Bool(false) {
        stop |= refusal(
            report,
            v,
            "R1-06",
            "gridlines mark out the plot, help estimate values and show which way is more; the scale keeps them",
            Some("chart.gridlines"),
            Some("gridlines true"),
        );
    }
    let (t, m) = theoretical_limits(v);
    let known = v.known();
    let lo = known.iter().min().copied();
    let hi = known.iter().max().copied();
    if let (Some(lo), Some(t)) = (lo, t)
        && lo < t
    {
        stop |= refusal(
            report,
            v,
            "R2-28",
            format!(
                "a value ({}) lies below the theoretical minimum {}",
                plain(lo),
                plain(t)
            ),
            Some("data.measure.min"),
            Some("correct data or limit"),
        );
    }
    if let (Some(hi), Some(m)) = (hi, m)
        && hi > m
    {
        stop |= refusal(
            report,
            v,
            "R2-28",
            format!(
                "a value ({}) lies above the theoretical maximum {}",
                plain(hi),
                plain(m)
            ),
            Some("data.measure.max"),
            Some("correct data or limit"),
        );
    }
    let sc_max = scale_dec(v, "max");
    let sc_min = scale_dec(v, "min");
    if let (Some(smax), Some(m)) = (sc_max, m)
        && smax > m
    {
        stop |= refusal(
            report,
            v,
            "R2-28",
            format!(
                "the scale would run to {}, past the theoretical maximum {}: labels beyond it show impossible values",
                plain(smax),
                plain(m)
            ),
            Some("scale.max"),
            Some(&format!("scale.max at most {}, or none", plain(m))),
        );
    }
    if let (Some(smin), Some(t)) = (sc_min, t)
        && smin < t
    {
        stop |= refusal(
            report,
            v,
            "R2-28",
            format!(
                "the scale would start at {}, below the theoretical minimum {}",
                plain(smin),
                plain(t)
            ),
            Some("scale.min"),
            Some("scale.min none"),
        );
    }
    if let (Some(smax), Some(hi)) = (sc_max, hi)
        && smax < hi
    {
        stop |= refusal(
            report,
            v,
            "R2-27",
            "the pinned maximum lies below the data, which would be cut off",
            Some("scale.max"),
            Some("scale.max none"),
        );
    }
    let magnitude = v.get("/scale/magnitude");
    let magnitude_ok = magnitude.is_null()
        || magnitude
            .as_str()
            .is_some_and(|s| s == "auto" || s == "none")
        || (magnitude.is_i64() && [0, 3, 6, 9, 12].contains(&magnitude.as_i64().unwrap_or(-1)));
    if !magnitude_ok {
        error(
            report,
            "CG-4",
            "scale.magnitude is auto, none, 3, 6, 9 or 12",
            Some("scale.magnitude"),
        );
        stop = true;
    }
    let steps = v.get("/scale/steps");
    if let Some(steps) = steps.as_array() {
        let allowed: [Decimal; 4] = [
            Decimal::ONE,
            Decimal::from(2),
            Decimal::new(25, 1),
            Decimal::from(5),
        ];
        let odd: Vec<Decimal> = steps
            .iter()
            .filter_map(dec)
            .filter(|s| !allowed.contains(s))
            .collect();
        if !odd.is_empty() {
            // `f"{s:f}"` keeps the digits as written
            let written: Vec<String> = steps
                .iter()
                .filter(|s| dec(s).is_some_and(|d| !allowed.contains(&d)))
                .map(|s| s.to_string())
                .collect();
            stop |= refusal(
                report,
                v,
                "R2-09",
                format!(
                    "steps based on {}: a step of 1, 2 or 5 × 10^n is easiest to read (a custom step needs a declared reason)",
                    written.join(", ")
                ),
                Some("scale.steps"),
                Some("[1, 2, 5], or an override with the reason"),
            );
        }
    }
    stop
}

const MAGNITUDE_WORDS: &str = r"\b(thousand|million|billion|trillion)s?\b|\b000s?\b|\bbn\b";
fn magnitude_name(e: i32) -> &'static str {
    match e {
        0 => "none",
        3 => "thousands",
        6 => "millions",
        9 => "billions",
        12 => "trillions",
        _ => "None",
    }
}
fn integer_digits(x: Decimal) -> usize {
    let a = x.abs();
    if a >= Decimal::ONE {
        a.trunc().to_string().len()
    } else {
        1
    }
}
fn unit_magnitude(u: &Value) -> i32 {
    let m = u["magnitude"].as_i64().unwrap_or(0) as i32;
    if m != 0 {
        return m;
    }
    match u["given_in"].as_str() {
        Some("thousands") => 3,
        Some("millions") => 6,
        Some("billions") => 9,
        Some("trillions") => 12,
        _ => 0,
    }
}

/// `__init__._given_in`: R5-11, the unit the data are written in agrees with the values. Reads
/// the source spec's values (before any derived measure), as the original does.
pub fn given_in(v: &View, source: &PreparedData, report: &mut Report) {
    let raw_values = |i: usize| -> Vec<Decimal> {
        match &source.irregular {
            Some(points) => points
                .get(i)
                .map(|row| row.iter().filter_map(|p| p.datum.value).collect())
                .unwrap_or_default(),
            None => source
                .series
                .get(i)
                .map(|s| s.values.iter().filter_map(|d| d.value).collect())
                .unwrap_or_default(),
        }
    };
    let loc = fmt::locale(v.str_or("/audience/locale", "en-GB"));
    let series = v
        .get("/data/series")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let measures = v
        .get("/data/measures")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut entries: Vec<(String, Value, Vec<usize>)> = vec![(
        "data.measure.unit".into(),
        v.get("/data/measure").clone(),
        series
            .iter()
            .enumerate()
            .filter(|(_, s)| s["measure"].is_null() || measures.is_empty())
            .map(|(i, _)| i)
            .collect(),
    )];
    for (i, m) in measures.iter().enumerate() {
        entries.push((
            format!("data.measures[{i}].unit"),
            m.clone(),
            series
                .iter()
                .enumerate()
                .filter(|(_, s)| !s["measure"].is_null() && s["measure"] == m["id"])
                .map(|(i, _)| i)
                .collect(),
        ));
    }
    let re = RegexBuilder::new(MAGNITUDE_WORDS)
        .case_insensitive(true)
        .build()
        .unwrap();
    for (p, m, members) in entries {
        let u = &m["unit"];
        if u.is_null() || members.is_empty() {
            continue;
        }
        let vals: Vec<Decimal> = members
            .iter()
            .flat_map(|i| raw_values(*i))
            .map(|x| x.abs())
            .collect();
        let top = vals.iter().max().copied();
        let e = unit_magnitude(u);
        let leaf = if u["given_in"].is_string() {
            "given_in"
        } else {
            "magnitude"
        };
        let field = format!("{p}.{leaf}");
        let words: Vec<String> = ["noun", "what", "suffix"]
            .iter()
            .filter_map(|k| u[*k].as_str().filter(|s| !s.is_empty()).map(str::to_owned))
            .collect();
        let words = words.join(" ");
        let word = re.find(&words).map(|m| m.as_str().to_owned());
        let mut found: Vec<(String, String)> = vec![];
        if e != 0
            && let Some(top) = top
            && integer_digits(top) >= 5
        {
            let name = magnitude_name(e);
            let actual = top * pow10(e);
            let shown = fmt::choose_magnitude(&[actual], None, None);
            let given = u["given_in"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| e.to_string());
            found.push((field.clone(), format!(
                "the values are given in {name} ({leaf} {given}), yet the largest, {}, has {} integer digits: it stands for {}, and the scale will read in {}. If the values are the actual amounts, leave {leaf} out: the generator hides digits by itself (G6.11). If they really are in {name}, say so in reasons[\"{field}\"]",
                fmt::number(top, fmt::decimals(top), loc, "", false),
                integer_digits(top),
                fmt::number(actual, 0, loc, u["prefix"].as_str().unwrap_or(""), false),
                magnitude_name(shown),
            )));
        }
        if e != 0
            && let Some(w) = &word
        {
            found.push((field.clone(), format!(
                "the unit's words say “{w}” and {leaf} says the values are in {}: the magnitude would be stated twice, and the generator writes it itself from the values; take the word out of the unit",
                magnitude_name(e)
            )));
        } else if let Some(w) = &word
            && e == 0
            && let Some(top) = top
            && integer_digits(top) >= 5
        {
            found.push((p.clone(), format!(
                "the unit's words say “{w}”, yet the values reach {}, which the generator shortens by itself (G6.11): the label would read in {} and “{w}” both. If the values are full amounts, take the word out of the unit; if they are already in thousands or millions, give unit.given_in instead",
                fmt::number(top, fmt::decimals(top), loc, "", false),
                magnitude_name(fmt::choose_magnitude(&[top], None, None)),
            )));
        }
        for (path, text) in found {
            let reason = v.reason(&path);
            let mut f = finding(
                "R5-11",
                if reason.is_some() { "info" } else { "warn" },
                text,
                Some(&path),
                None,
            );
            f.reason = reason;
            report.findings.push(f);
        }
    }
}
fn pow10(e: i32) -> Decimal {
    let mut x = Decimal::ONE;
    for _ in 0..e {
        x *= Decimal::from(10);
    }
    x
}

const CENSORED_TYPES: [&str; 2] = ["heatmap", "table"];
fn type_name(t: &str) -> String {
    match t {
        "bar" => "a bar chart".into(),
        "line" => "a line chart".into(),
        "dot" => "a dot plot".into(),
        "pie" => "a pie".into(),
        "step" => "a step chart".into(),
        "thin_bar" => "a thin bar chart".into(),
        "arrow" => "an arrow chart".into(),
        t => format!("a {} chart", t.replace('_', " ")),
    }
}

/// `__init__.censored_refusal`: censored values are a state of their own, drawn only by a
/// heatmap or a table (R0-12). Returns true when refused.
pub fn censored_refusal(v: &View, t: &str, report: &mut Report) -> bool {
    let mut cens = vec![];
    for s in v.series() {
        for (j, d) in s.values.unwrap_or(&[]).iter().enumerate() {
            if d.status == "below" || d.status == "above" {
                cens.push((s.index, j, d));
            }
        }
    }
    if cens.is_empty() || CENSORED_TYPES.contains(&t) {
        return false;
    }
    let (i, j, d) = cens[0];
    refusal(
        report,
        v,
        "R0-12",
        format!(
            "{} censored value{} (below or above a limit, as “{}{}”): {} would draw them as gaps, as if not known, or as zero; only a heatmap (a step of their own, in the key) and a table (“<1” in the cell) draw them yet",
            cens.len(),
            if cens.len() > 1 { "s" } else { "" },
            if d.status == "below" { "<" } else { ">" },
            plain(d.limit.unwrap_or_default()),
            type_name(t)
        ),
        Some(&format!("data.series[{i}].values[{j}]")),
        Some(
            "chart.type heatmap or table; or, if the limit may stand for the value for this message, the limit itself as a number, said in a note",
        ),
    )
}

/// `__init__._type_refusals`: bars start at zero (R2-24, R9-5).
pub fn type_refusals(v: &View, chart_type: &str, report: &mut Report) -> bool {
    let mut stop = false;
    if LENGTH_ENCODED.contains(&chart_type) {
        let min = scale_dec(v, "min");
        let truncated =
            min.is_some_and(|m| !m.is_zero()) || v.str_or("/scale/zero", "auto") == "truncate";
        if truncated {
            let at = match min {
                Some(m) => format!("at {}", plain(m)),
                None => "above zero".into(),
            };
            stop |= refusal(
                report,
                v,
                "R2-24",
                format!(
                    "bars on a scale starting {at}: a bar's length must be its value, and here a bar twice as long would not mean twice the value"
                ),
                Some(if min.is_some() {
                    "scale.min"
                } else {
                    "scale.zero"
                }),
                Some(
                    "no scale.min; for small differences, facts.small_differences_matter (an inset)",
                ),
            );
            stop |= refusal(
                report,
                v,
                "R9-5",
                "bar scales are never truncated",
                Some("scale.min"),
                None,
            );
        }
    }
    stop
}

/// `__init__._stage_notes`: what the spec asks for that the original's stage does not draw.
pub fn stage_notes(v: &View, report_type: Option<&str>, report: &mut Report) {
    if v.str_or("/chart/value_labels", "auto") == "on"
        && !matches!(report_type, Some("bar" | "dot"))
    {
        info(
            report,
            "CG-3",
            "direct value labels are phase 2",
            Some("chart.value_labels"),
        );
    }
    if !v.get("/layout/height").is_null() {
        info(
            report,
            "CG-3",
            "layout.height is not used yet; the plot aspect sets the height",
            Some("layout.height"),
        );
    }
}
