//! Dated markers on an irregular time axis.
use super::continuous as ta;
use super::fig::{self, Batch, Bx, Item, Txt, overlap};
use super::*;
use chrono::NaiveDate;
use serde_json::Value;

fn rows(plan: &SemanticPlan) -> Result<Vec<&serde_json::Value>> {
    match plan.spec.raw.pointer("/data/x/events") {
        None | Some(serde_json::Value::Null) => Ok(vec![]),
        Some(serde_json::Value::Array(items)) => Ok(items.iter().collect()),
        _ => Err(err("data.x.events must be an array").at("data.x.events")),
    }
}

fn instant(text: &str) -> Result<(f64, bool)> {
    if let Ok(value) = parse_time(text) {
        return Ok(value);
    }
    if let Ok(date) = NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d") {
        return Ok((
            date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64,
            false,
        ));
    }
    if text.len() == 4 {
        if let Ok(year) = text.parse::<i32>() {
            if let Some(date) = NaiveDate::from_ymd_opt(year, 1, 1) {
                return Ok((
                    date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64,
                    false,
                ));
            }
        }
    }
    Err(err(format!(
        "event date {text} is not a supported calendar instant"
    )))
}

pub fn validate(plan: &SemanticPlan, data: &PreparedData) -> Result<()> {
    let rows = rows(plan)?;
    if rows.is_empty() {
        return Ok(());
    }
    if data.x_kind != "time"
        || !matches!(plan.chart_type, ChartType::Step | ChartType::ThinBar)
        || data.irregular.is_none()
    {
        return Err(
            err("dated event markers require an irregular step or thin-bar time axis")
                .at("data.x.events"),
        );
    }
    for (i, row) in rows.iter().enumerate() {
        let at = row
            .get("at")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err(format!("data.x.events[{i}] needs at")))?;
        let _ = instant(at)?;
        let label = row
            .get("label")
            .ok_or_else(|| err(format!("data.x.events[{i}] needs label")))?;
        if guides::content(label)?.is_none() {
            return Err(err(format!("data.x.events[{i}] has empty label")));
        }
    }
    Ok(())
}

pub(crate) fn has_events(plan: &SemanticPlan) -> bool {
    rows(plan).is_ok_and(|r| !r.is_empty())
}

/// Every text the markers may set, for the layout's batch.
pub(crate) fn need(plan: &SemanticPlan, batch: &Batch) -> Result<()> {
    for row in rows(plan)? {
        if let Some(c) = guides::content(&row["label"])? {
            batch.need_content("event-label", &c)?;
        }
    }
    batch.need("event-label", "H")
}

/// The datetime of a marked date: a date alone the start of its day, or its middle on a thin
/// bar chart whose events are all dated to the day (as those events sit, D-42).
fn event_instant(
    plan: &SemanticPlan,
    at: &str,
    thin: bool,
) -> Result<(chrono::NaiveDateTime, &'static str)> {
    let (t, res) = match ta::parse_instant(at) {
        Ok(v) => v,
        Err(_) => {
            let (secs, _) = instant(at)?;
            (
                chrono::DateTime::from_timestamp(secs as i64, 0)
                    .ok_or_else(|| err(format!("event date {at} is out of range")))?
                    .naive_utc(),
                "day",
            )
        }
    };
    if thin && res == "day" {
        let all_day = plan.spec.raw["data"]["series"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|s| s["points"].as_array().into_iter().flatten())
            .filter_map(|p| p.get(0).and_then(Value::as_str))
            .all(|t| ta::parse_instant(t).is_ok_and(|(_, r)| r == "day"));
        if all_day {
            return Ok((t + chrono::Duration::hours(12), res));
        }
    }
    Ok((t, res))
}

/// The markers of `data.x.events` on a vertical plot (x0, y0, x1, y1) whose time runs along
/// x by `x_of`, over [t0, t1]: (lines, labels), each label clear of `texts` and `marks` and
/// of the others; findings warn of any not drawn (never silently).
#[allow(clippy::too_many_arguments)]
pub(crate) fn place(
    plan: &SemanticPlan,
    batch: &Batch,
    thin: bool,
    x_of: &dyn Fn(chrono::NaiveDateTime) -> f64,
    t0: chrono::NaiveDateTime,
    t1: chrono::NaiveDateTime,
    plot: (f64, f64, f64, f64),
    texts: &[Txt],
    marks: &[Bx],
    width: f64,
    findings: &mut Vec<Finding>,
    decisions: &mut Vec<Value>,
) -> Result<(Vec<Item>, Vec<Txt>)> {
    let mut done: Vec<String> = vec![];
    let em = batch.metric("--label-size")?;
    let pad = batch.metric("--text-pad")?;
    let gap = 0.3 * em;
    let (_x0, y0, _x1, y1) = plot;
    let mut lines = vec![];
    let mut labels: Vec<Txt> = vec![];
    let mut taken: Vec<Bx> = texts.iter().map(|t| t.bbox(pad)).collect();
    let mut boxes: Vec<Bx> = marks
        .iter()
        .map(|b| {
            (
                b.0 - pad / 2.,
                b.1 - pad / 2.,
                b.2 + pad / 2.,
                b.3 + pad / 2.,
            )
        })
        .collect();
    for (j, row) in rows(plan)?.into_iter().enumerate() {
        let at = row["at"]
            .as_str()
            .ok_or_else(|| err(format!("data.x.events[{j}] needs at")))?;
        let (t, res) = event_instant(plan, at, thin)?;
        let name = ta::instant_name(t, res);
        let content = guides::content(&row["label"])?.ok_or_else(|| err("empty event label"))?;
        let plain = content.readable().to_owned();
        if !(t0 <= t && t <= t1) {
            let mut f = Finding::new(
                "6.13",
                "warning",
                format!(
                    "the date marked {name} (“{plain}”) lies outside the time axis ({} to {}): not drawn",
                    ta::instant_name(t0, "day"),
                    ta::instant_name(t1 - chrono::Duration::seconds(1), "day")
                ),
            )
            .at(format!("data.x.events[{j}].at"));
            f.level = "open".into();
            findings.push(f);
            continue;
        }
        let x = x_of(t);
        let probe = batch.text("event-label", content.clone(), 0., 0., "start")?;
        let e = probe.ext;
        let pitch = batch.metric("--line-pitch")? * batch.metric("--note-size")? / em;
        let top = y0 + e.ascent + 0.35 * em;
        let mut tries = vec![];
        for k in 0..4 {
            let yb = top + k as f64 * pitch;
            tries.push(("right", x + gap, yb, "start"));
            tries.push(("left", x - gap, yb, "end"));
        }
        let above = y0 - 0.35 * em - e.descent;
        tries.push(("above-right", x + gap, above, "start"));
        tries.push(("above-left", x - gap, above, "end"));
        let mut got = None;
        for (where_, tx, ty, anchor) in tries {
            let t = Txt {
                x: tx,
                y: ty,
                anchor: anchor.into(),
                ..probe.clone()
            };
            let b = t.bbox(0.);
            if b.0 < 0. || b.2 > width {
                continue;
            }
            if taken.iter().chain(&boxes).any(|o| overlap(b, *o)) {
                continue;
            }
            got = Some((where_, t));
            break;
        }
        let Some((where_, tx)) = got else {
            findings.push(
                Finding::new(
                    "CG-1",
                    "warning",
                    format!(
                        "the date marked {name} (“{plain}”) finds no place for its label beside its line clear of the marks and texts: not drawn (a marked date is never left unnamed)"
                    ),
                )
                .at(format!("data.x.events[{j}]")),
            );
            continue;
        };
        let y_top = if where_.starts_with("above") {
            tx.bbox(0.).1
        } else {
            y0
        };
        lines.push(Item::line(
            x,
            y_top,
            x,
            y1,
            "event-line",
            fig::attrs(&[("x", at.to_owned())]),
        ));
        taken.push(tx.bbox(pad));
        boxes.push((x - 0.5, y_top, x + 0.5, y1));
        let side = if where_.starts_with("above") {
            "above the plot"
        } else if where_ == "right" {
            "right of its top"
        } else {
            "left of its top"
        };
        done.push(format!(
            "{name}, “{plain}”, labelled {side}{}",
            if tx.y > top + 0.01 && !where_.starts_with("above") {
                " (lower down)"
            } else {
                ""
            }
        ));
        labels.push(tx);
    }
    if !done.is_empty() {
        decisions.push(fig::decision(
            "6.13",
            format!(
                "dates marked on the time axis, each a thin dark-grey line across the plot under the marks, its label the chart's own note beside it: {}",
                done.join("; ")
            ),
            Some("open"),
        ));
    }
    Ok((lines, labels))
}
