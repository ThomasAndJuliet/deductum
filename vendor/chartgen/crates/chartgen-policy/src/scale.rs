//! Scale decisions (`__init__.compute_scale`'s decisions and guided findings). The limits are
//! trained by `chartgen-scales`; this records what the original's report says of them.
use crate::rules::{decide, guided, refusal};
use crate::view::{View, dec, plain};
use chartgen_format as fmt;
use chartgen_model::{Decimal, Report};
use chartgen_scales::{Explained, LENGTH_ENCODED, lower_limit_mode, summary};
use serde_json::Value;

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

/// The scale's decisions once trained (`trained`: Err(message) when no step gives 2 to 10
/// intervals, a CG-4 refusal). Returns false when the chart is refused.
pub(crate) fn scale_report(
    report: &mut Report,
    v: &View,
    chart_type: &str,
    trained: std::result::Result<&Explained, String>,
) -> bool {
    let scale = match trained {
        Ok(s) => s,
        Err(e) => {
            refusal(
                report,
                v,
                "CG-4",
                format!("no scale: {e}"),
                Some("scale"),
                None,
            );
            return false;
        }
    };
    if chart_type == "step" || chart_type == "thin_bar" {
        // layout/step.scale_values: the series' points, past no theoretical limit
        let known: Vec<Decimal> = v
            .data
            .irregular
            .as_ref()
            .map(|rows| {
                rows.iter()
                    .flatten()
                    .filter_map(|p| p.datum.value)
                    .collect()
            })
            .unwrap_or_default();
        let (t, m) = theoretical_limits(v);
        if let (Some(lo), Some(t)) = (known.iter().min(), t)
            && *lo < t
            && refusal(
                report,
                v,
                "R2-28",
                format!(
                    "a value ({}) lies below the theoretical minimum {}",
                    plain(*lo),
                    plain(t)
                ),
                Some("data.measure.min"),
                Some("correct data or limit"),
            )
        {
            return false;
        }
        if let (Some(hi), Some(m)) = (known.iter().max(), m)
            && *hi > m
            && refusal(
                report,
                v,
                "R2-28",
                format!(
                    "a value ({}) lies above the theoretical maximum {}",
                    plain(*hi),
                    plain(m)
                ),
                Some("data.measure.max"),
                Some("correct data or limit"),
            )
        {
            return false;
        }
        if known.is_empty() {
            refusal(
                report,
                v,
                "CG-4",
                "no value of the series is known",
                Some("data.series[0].points"),
                None,
            );
            return false;
        }
    }
    if chart_type == "bumps" {
        // layout/bumps.rank_scale: a rank scale of its own, 1 … N by 1
        let (_, n) = crate::procedure::ranks(v);
        let m = v.measure();
        let ranked = m["kind"].as_str() != Some("rank");
        decide(
            report,
            "R6-7",
            format!(
                "a rank scale, 1 at the top to {n} at the bottom, one label per rank, “Rank” above them (G7.24){}",
                if ranked && m["good"].as_str() != Some("lower") {
                    " (the values ranked each period, the largest first)"
                } else if ranked {
                    " (the values ranked each period, the smallest first)"
                } else {
                    ""
                }
            ),
            Some("locked"),
        );
        return true;
    }
    if chart_type == "stacked_bar_100" || chart_type == "stacked_area_100" {
        // layout/bars.stack_scale: each total 100 %, on its own scale
        decide(
            report,
            "02 D5",
            format!(
                "scale {}: each total 100 %, its parts in % of it (R7-30, G7.32)",
                summary(&scale.scale)
            ),
            None,
        );
        return true;
    }
    let mode = scale.scale.mode.as_str();
    for (rule, text) in &scale.reasons {
        decide(
            report,
            rule,
            text.clone(),
            if mode == "truncated" {
                Some("guided")
            } else {
                None
            },
        );
    }
    let zero = v.str_or("/scale/zero", "auto");
    let measure_zero = v.measure_str("zero").unwrap_or("meaningful");
    let (theoretical, _) = theoretical_limits(v);
    let irrelevant = v.flag("/facts/distance_to_zero_irrelevant");
    let values = &scale.values;
    if zero != "auto" && !values.is_empty() {
        let lo = *values.iter().min().unwrap();
        let hi = *values.iter().max().unwrap();
        let (auto_mode, _, auto_why) = lower_limit_mode(
            lo,
            hi,
            chart_type,
            "auto",
            measure_zero,
            theoretical,
            irrelevant,
        );
        guided(
            report,
            v,
            "R2-22",
            "scale.zero",
            mode,
            auto_mode.as_str(),
            &auto_why[0].1,
        );
    }
    if let Some(min) = scale_dec(v, "min")
        && !values.is_empty()
        && !LENGTH_ENCODED.contains(&chart_type)
        && let Some(auto_lo) = scale.unpinned_lo
    {
        let truncating = auto_lo.is_zero() && Decimal::ZERO < min;
        guided(
            report,
            v,
            if truncating { "R2-24" } else { "R2-26" },
            "scale.min",
            &plain(min),
            &plain(auto_lo),
            if truncating {
                "zero stays unless a theoretical minimum, an arbitrary zero or a declared irrelevance allows truncation (G2.8, D-10 (a))"
            } else {
                "the theoretical minimum, else 10–20 % of the spread below the lowest value (G2.11)"
            },
        );
    }
    decide(
        report,
        "02 D5",
        format!(
            "scale {}{}",
            summary(&scale.scale),
            if scale.capped_hi {
                " (capped at the theoretical maximum)"
            } else {
                ""
            }
        ),
        None,
    );
    let magnitude = v.get("/scale/magnitude");
    if !(magnitude.is_null() || magnitude == "auto") {
        let measure = v.measure();
        let unit = fmt::Unit::from_value(&measure["unit"]);
        let kind = measure["kind"].as_str().unwrap_or("count");
        let setting = match magnitude {
            Value::String(s) if s == "none" => Some(0),
            Value::Number(n) => n.as_i64().map(|x| x as i32),
            _ => None,
        };
        let labels = |m: Option<i32>| {
            fmt::format_scale(
                &scale.scale.ticks,
                scale.scale.step,
                kind,
                &unit,
                m,
                "auto",
                &Value::Null,
                fmt::Locale::default(),
                "",
            )
            .map(|l| l.magnitude)
        };
        if let (Ok(chosen), Ok(auto)) = (labels(setting), labels(None)) {
            guided(
                report,
                v,
                "R2-05",
                "scale.magnitude",
                magnitude_name(chosen),
                magnitude_name(auto),
                "digits hidden from five integer digits, by the largest unit needing at most one decimal (R5-10, D-4)",
            );
        }
    }
    let style = v.str_or("/scale/magnitude_style", "auto");
    if style != "auto" && style != "word_on_top" {
        guided(
            report,
            v,
            "R2-05",
            "scale.magnitude_style",
            style,
            "word_on_top",
            "the magnitude and unit once, on the top label; the others bare numbers (D-3 (a))",
        );
    }
    true
}

/// `__init__._limits`: theoretical limits, declared, or 0 (and 100 for parts of a meaningful
/// total) for percentages.
pub(crate) fn theoretical_limits(v: &View) -> (Option<Decimal>, Option<Decimal>) {
    let mut t = v.measure_dec("min");
    let mut m = v.measure_dec("max");
    if v.measure_str("kind") == Some("percent") {
        if t.is_none() {
            t = Some(Decimal::ZERO);
        }
        if m.is_none() && v.parts().is_some_and(|p| p["meaningful_total"] == true) {
            m = Some(Decimal::from(100));
        }
    }
    (t, m)
}

pub(crate) fn scale_dec(v: &View, key: &str) -> Option<Decimal> {
    dec(v.get(&format!("/scale/{key}")))
}
