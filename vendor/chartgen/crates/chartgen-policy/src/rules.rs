//! The report operations of the original `rules.py`: levels, refusals, guided departures and
//! decisions.
//!
//! Levels: the original derives a rule's level from `charts/GUIDE.md` (its tags and its index of
//! sources). That file is absent from the original source tree the oracle runs from, so
//! `rules._guide()` yields no levels and no index: every registered rule whose registry entry
//! names a level letter takes that letter, every other rule (and every unregistered one) is
//! `locked`, and every `guide` list is empty. This module reproduces the oracle as run. With
//! GUIDE.md present the original would give other levels for many rules (see the original's
//! `tests/test_rules.py`, which fails in the oracle's environment for that reason).
use crate::view::View;
use chartgen_model::{Finding, Report};
use serde_json::{Value, json};

/// `rules.level_of(rule)` with GUIDE.md absent.
pub fn level_of(rule: &str) -> &'static str {
    match rule {
        // registered with the level letter G
        "02 D1" | "02 D2" | "02 D5" | "6.4" | "6.7" => "guided",
        // registered with the level letter O
        "6.12" | "6.13" | "CG-3" => "open",
        // proposed rules of the statistical charts (STATS.md): guided until the owner rules
        r if r.starts_with("XS-") => "guided",
        _ => "locked",
    }
}

/// `rules.guide_ids(rule)`: empty without GUIDE.md's index of sources.
pub fn guide_ids(_rule: &str) -> Vec<String> {
    vec![]
}

pub(crate) fn finding(
    rule: &str,
    severity: &str,
    message: impl Into<String>,
    path: Option<&str>,
    level: Option<&str>,
) -> Finding {
    let mut f = Finding::new(rule, severity, message);
    f.path = path.map(str::to_owned);
    f.level = level.unwrap_or(level_of(rule)).into();
    f.guide = guide_ids(rule);
    f
}

pub(crate) fn info(
    report: &mut Report,
    rule: &str,
    message: impl Into<String>,
    path: Option<&str>,
) {
    report
        .findings
        .push(finding(rule, "info", message, path, None));
}
pub(crate) fn warn(
    report: &mut Report,
    rule: &str,
    message: impl Into<String>,
    path: Option<&str>,
) {
    report
        .findings
        .push(finding(rule, "warn", message, path, None));
}
pub(crate) fn error(
    report: &mut Report,
    rule: &str,
    message: impl Into<String>,
    path: Option<&str>,
) {
    report
        .findings
        .push(finding(rule, "error", message, path, None));
}

/// `Report.decide(rule, text, level)`.
pub(crate) fn decide(
    report: &mut Report,
    rule: &str,
    text: impl Into<String>,
    level: Option<&str>,
) {
    report.decisions.push(decision(rule, text, level));
}
pub(crate) fn decision(rule: &str, text: impl Into<String>, level: Option<&str>) -> Value {
    json!({"rule": rule, "level": level.unwrap_or(level_of(rule)), "text": text.into(),
           "guide": guide_ids(rule)})
}

/// `rules.refusal`: an error, unless overridden (R0-04, reported with its reason) or the chart is
/// for analysis (R0-02, a warning). Returns true when the chart must not be drawn.
pub(crate) fn refusal(
    report: &mut Report,
    v: &View,
    rule: &str,
    message: impl Into<String>,
    path: Option<&str>,
    supply: Option<&str>,
) -> bool {
    refusal_with(
        report,
        &v.overrides(),
        v.purpose(),
        rule,
        message,
        path,
        supply,
    )
}

pub(crate) fn refusal_with(
    report: &mut Report,
    overrides: &[(String, String)],
    purpose: &str,
    rule: &str,
    message: impl Into<String>,
    path: Option<&str>,
    supply: Option<&str>,
) -> bool {
    let reason = overrides
        .iter()
        .find(|(r, _)| r == rule)
        .map(|(_, reason)| reason.clone());
    let severity = if reason.is_some() {
        "overridden"
    } else if purpose == "analysis" {
        "warn"
    } else {
        "error"
    };
    let mut f = finding(rule, severity, message, path, None);
    f.supply = supply.map(str::to_owned);
    if let Some(reason) = &reason {
        f.reason = Some(reason.clone());
        report
            .overrides
            .push(json!({"rule": rule, "reason": reason}));
    }
    report.findings.push(f);
    severity == "error"
}

/// `rules.guided`: a departure from the procedure's recommendation, a warning naming it (an info
/// with the spec's reason). Returns whether the choice departs.
pub(crate) fn guided(
    report: &mut Report,
    v: &View,
    rule: &str,
    path: &str,
    chosen: &str,
    recommended: &str,
    why: &str,
) -> bool {
    if chosen == recommended {
        return false;
    }
    let reason = v.reason(path);
    let mut f = finding(
        rule,
        if reason.is_some() { "info" } else { "warn" },
        format!("{path} {chosen} departs from the procedure, which gives {recommended} ({why})"),
        Some(path),
        Some("guided"),
    );
    f.reason = reason;
    f.recommended = Some(recommended.into());
    report.findings.push(f);
    true
}
