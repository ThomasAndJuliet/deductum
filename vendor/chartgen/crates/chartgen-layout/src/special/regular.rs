//! Periods on a regular time grid and the rows of their axis (the original's
//! `timeaxis.parse`, `row1`, `row2`, `fit`, `edge_needs`, `fit_row2`): row 1 the grid unit,
//! row 2 the next unit centred under its group, dividers between groups, and thinning
//! (every k-th label, restarting in each group) when row 1 would crowd. Used by the ported
//! specialist layouts (bumps, arrows) on equal slots.
use super::continuous::{MONTHS_AXIS, MONTHS_INLINE, MONTHS_NARROW};
use chartgen_model::{Error, Result};
use chrono::{Datelike, Duration, NaiveDate};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Period {
    pub grid: String,
    pub start: NaiveDate,
    pub hour: u32,
    pub quarter: u32,
    pub text: String,
    pub stop: Option<NaiveDate>,
}

fn row2_unit(grid: &str) -> Option<&'static str> {
    match grid {
        "hour" => Some("day"),
        "day" => Some("month"),
        "week" | "month" | "quarter" => Some("year"),
        _ => None,
    }
}

impl Period {
    /// The row-2 group: a day, a month or a year (none under years and bins).
    pub fn group(&self) -> Option<Vec<i64>> {
        let s = self.start;
        match row2_unit(&self.grid)? {
            "year" => Some(vec![s.year() as i64]),
            "month" => Some(vec![s.year() as i64, s.month() as i64]),
            _ => Some(vec![s.year() as i64, s.month() as i64, s.day() as i64]),
        }
    }
    /// The first day after the period.
    #[allow(dead_code)]
    pub fn end(&self) -> NaiveDate {
        let s = self.start;
        match self.grid.as_str() {
            "year" => NaiveDate::from_ymd_opt(s.year() + 1, 1, 1).unwrap(),
            "quarter" | "month" => {
                let m = s.month() + if self.grid == "quarter" { 3 } else { 1 };
                NaiveDate::from_ymd_opt(s.year() + ((m - 1) / 12) as i32, (m - 1) % 12 + 1, 1)
                    .unwrap()
            }
            "week" => s + Duration::days(7),
            "bin" => self.stop.unwrap_or(s),
            _ => s + Duration::days(1),
        }
    }
}

fn bad(value: &str, grid: &str) -> Error {
    Error::new(
        "layout",
        format!("{value:?} is not a period of the {grid} grid"),
    )
}

/// A Period from its string on the grid.
pub(crate) fn parse(value: &str, grid: &str) -> Result<Period> {
    let p = |start: NaiveDate| Period {
        grid: grid.into(),
        start,
        hour: 0,
        quarter: 0,
        text: value.into(),
        stop: None,
    };
    let num = |s: &str| s.trim().parse::<i32>().map_err(|_| bad(value, grid));
    Ok(match grid {
        "year" => p(NaiveDate::from_ymd_opt(num(value)?, 1, 1).ok_or_else(|| bad(value, grid))?),
        "quarter" => {
            let (y, q) = value.split_once("-Q").ok_or_else(|| bad(value, grid))?;
            let q = num(q)? as u32;
            let mut out =
                p(NaiveDate::from_ymd_opt(num(y)?, 3 * q - 2, 1)
                    .ok_or_else(|| bad(value, grid))?);
            out.quarter = q;
            out
        }
        "month" => {
            let (y, m) = value.split_once('-').ok_or_else(|| bad(value, grid))?;
            p(NaiveDate::from_ymd_opt(num(y)?, num(m)? as u32, 1)
                .ok_or_else(|| bad(value, grid))?)
        }
        "week" => {
            if let Some((y, w)) = value.split_once("-W") {
                p(
                    NaiveDate::from_isoywd_opt(num(y)?, num(w)? as u32, chrono::Weekday::Mon)
                        .ok_or_else(|| bad(value, grid))?,
                )
            } else {
                p(NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| bad(value, grid))?)
            }
        }
        "bin" => {
            let parts: Vec<&str> = value.split(['–', '-']).collect();
            if parts.len() != 2 {
                return Err(bad(value, grid));
            }
            let (a, b) = (num(parts[0])?, num(parts[1])?);
            let mut out = p(NaiveDate::from_ymd_opt(a, 1, 1).ok_or_else(|| bad(value, grid))?);
            out.stop = NaiveDate::from_ymd_opt(b + 1, 1, 1);
            out
        }
        "day" => p(NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| bad(value, grid))?),
        "hour" => {
            let (d, h) = value.split_once('T').ok_or_else(|| bad(value, grid))?;
            let mut out =
                p(NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| bad(value, grid))?);
            out.hour = num(h)? as u32;
            out
        }
        _ => return Err(Error::new("layout", format!("no regular grid {grid:?}"))),
    })
}

/// Row 1: "Jan" (narrow "J"), "Q1", "2021", "16", "6 AM", "Jun 19–25".
pub(crate) fn row1(p: &Period, form: &str) -> String {
    match p.grid.as_str() {
        "month" => (if form == "narrow" {
            MONTHS_NARROW
        } else {
            MONTHS_AXIS
        })[p.start.month0() as usize]
            .to_owned(),
        "quarter" => format!("Q{}", p.quarter),
        "year" => p.start.year().to_string(),
        "day" => p.start.day().to_string(),
        "hour" => {
            let h = if p.hour % 12 == 0 { 12 } else { p.hour % 12 };
            format!("{h} {}", if p.hour < 12 { "AM" } else { "PM" })
        }
        "week" => {
            let end = p.start + Duration::days(6);
            let tail = if end.month() == p.start.month() {
                end.day().to_string()
            } else {
                format!("{} {}", MONTHS_AXIS[end.month0() as usize], end.day())
            };
            format!(
                "{} {}–{tail}",
                MONTHS_AXIS[p.start.month0() as usize],
                p.start.day()
            )
        }
        _ => {
            let a = p.start.year();
            let b = p.stop.map_or(a, |s| s.year() - 1);
            if a / 100 == b / 100 && b > a {
                format!("{a}–{:02}", b % 100)
            } else if b > a {
                format!("{a}–{b}")
            } else {
                a.to_string()
            }
        }
    }
}

/// Row 2 under its group: "2022", "Oct. 2022", "Mar. 11, 2022".
pub(crate) fn row2(group: &[i64]) -> String {
    super::continuous::row2(group)
}

pub(crate) fn forms(grid: &str) -> &'static [&'static str] {
    if grid == "month" {
        &["full", "narrow"]
    } else {
        &["full"]
    }
}

/// Every string the axis might set: row 1 in each form, and row 2.
pub(crate) fn candidates(periods: &[Period]) -> Vec<(&'static str, String)> {
    let mut out = vec![];
    for p in periods {
        for f in forms(&p.grid) {
            out.push(("time-label", row1(p, f)));
        }
        if let Some(g) = p.group() {
            out.push(("time-label-2", row2(&g)));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // k serves the report only
pub(crate) struct Rows {
    pub k: usize,
    pub shown: Vec<usize>,
    pub labels: Vec<String>,
    /// (first index, last index, text or none)
    pub groups: Vec<(usize, usize, Option<String>)>,
    pub decisions: Vec<String>,
}

/// The axis cannot hold its labels even every seventh: the slot every seventh label needs.
#[derive(Debug, Clone)]
pub(crate) struct Crowded {
    pub message: String,
    pub need_slot: f64,
    pub slot: f64,
    pub wmax: f64,
}

type Width<'a> = dyn Fn(&str, &str) -> Result<f64> + 'a;

/// The rows for n periods in slots of `slot` points.
pub(crate) fn fit(
    periods: &[Period],
    slot: f64,
    width: &Width,
    sep: f64,
    clear: Option<f64>,
) -> Result<std::result::Result<Rows, Crowded>> {
    let max_k = 7usize;
    let grid = periods[0].grid.clone();
    let mut chosen = None;
    for f in forms(&grid) {
        let mut wmax = f64::NEG_INFINITY;
        for p in periods {
            wmax = wmax.max(width("time-label", &row1(p, f))?);
        }
        let k = ((wmax + sep) / slot).ceil();
        if k <= 1. {
            chosen = Some((*f, 1usize, wmax));
            break;
        }
    }
    let (form, k, wmax) = match chosen {
        Some(c) => c,
        None => {
            let mut wmax = f64::NEG_INFINITY;
            for p in periods {
                wmax = wmax.max(width("time-label", &row1(p, "full"))?);
            }
            ("full", ((wmax + sep) / slot).ceil().max(1.) as usize, wmax)
        }
    };
    if k > max_k {
        return Ok(Err(Crowded {
            message: format!(
                "row-1 labels would need every {k}th shown (at most every {max_k}th), too narrow for its time axis; widen the chart or aggregate to a coarser grid"
            ),
            need_slot: (wmax + sep) / max_k as f64,
            slot,
            wmax,
        }));
    }
    let labels: Vec<String> = periods.iter().map(|p| row1(p, form)).collect();
    let mut groups: Vec<(usize, usize, Option<String>, Option<Vec<i64>>)> = vec![];
    for (i, p) in periods.iter().enumerate() {
        let g = p.group();
        match groups.last_mut() {
            Some(last) if last.3 == g => last.1 = i,
            _ => groups.push((i, i, g.as_deref().map(row2), g)),
        }
    }
    let mut shown = vec![];
    for (first, last, _, _) in &groups {
        shown.extend((*first..=*last).step_by(k));
    }
    let mut kept: Vec<usize> = vec![];
    for i in shown {
        if let Some(&last) = kept.last() {
            if (i - last) as f64 * slot < wmax + sep {
                kept.pop();
            }
        }
        kept.push(i);
    }
    let nth = match k {
        2 => "second".to_owned(),
        3 => "third".to_owned(),
        4 => "fourth".to_owned(),
        5 => "fifth".to_owned(),
        6 => "sixth".to_owned(),
        7 => "seventh".to_owned(),
        _ => format!("{k}th"),
    };
    let mut decisions = vec![format!(
        "time axis: row 1 {form}{}",
        if k > 1 {
            format!(", every {nth} label, restarting in each group")
        } else {
            String::new()
        }
    )];
    if let Some(clear) = clear {
        if groups.len() >= 2 {
            let starts: Vec<usize> = groups[1..].iter().map(|g| g.0).collect();
            let mut near = vec![];
            for &i in &kept {
                let w = width("time-label", &labels[i])?;
                if starts
                    .iter()
                    .any(|a| (i as f64 + 0.5 - *a as f64).abs() * slot < w / 2. + clear)
                {
                    near.push(format!("“{}”", labels[i]));
                }
            }
            if !near.is_empty() {
                decisions.push(format!(
                    "time axis: {} closer than {clear} pt to the divider between two groups: kept on its slot, with a halo in the page colour round its letters, which interrupts the divider",
                    near.join(", ")
                ));
            }
        }
    }
    Ok(Ok(Rows {
        k,
        shown: kept,
        labels,
        groups: groups.into_iter().map(|(a, b, t, _)| (a, b, t)).collect(),
        decisions,
    }))
}

/// The room needed between the outer dividers and the first and last slots so that the
/// first and last shown labels clear the dividers by `clear`.
pub(crate) fn edge_needs(rows: &Rows, slot: f64, width: &Width, clear: f64) -> Result<(f64, f64)> {
    if rows.groups.len() < 2 || rows.shown.is_empty() {
        return Ok((0., 0.));
    }
    let n = rows.labels.len() as f64;
    let a = rows.shown[0];
    let b = *rows.shown.last().unwrap();
    let left = clear + width("time-label", &rows.labels[a])? / 2. - (a as f64 + 0.5) * slot;
    let right = clear + width("time-label", &rows.labels[b])? / 2. - (n - b as f64 - 0.5) * slot;
    Ok((left.max(0.), right.max(0.)))
}

/// Row 2 as drawn: a group narrower than its text keeps its dividers and drops the text.
pub(crate) fn fit_row2(
    rows: &Rows,
    slot: f64,
    width: &Width,
    sep: f64,
) -> Result<(Vec<(usize, usize, Option<String>)>, Vec<String>)> {
    let mut out = vec![];
    let mut dropped = vec![];
    for (a, b, t) in &rows.groups {
        let mut t = t.clone();
        if let Some(text) = &t {
            if (b - a + 1) as f64 * slot < width("time-label-2", text)? + sep {
                dropped.push(text.clone());
                t = None;
            }
        }
        out.push((*a, *b, t));
    }
    let decisions = if dropped.is_empty() {
        vec![]
    } else {
        vec![format!(
            "time axis: row 2 drops {}, its group too narrow for the text",
            dropped.join(", ")
        )]
    };
    Ok((out, decisions))
}

/// The slot indices at which a row-2 group starts, the first excluded.
pub(crate) fn boundaries(rows: &Rows) -> Vec<usize> {
    rows.groups[1..].iter().map(|g| g.0).collect()
}

/// Whether row 2 is a single group whose text the title block already gives.
pub(crate) fn title_gives(rows: &Rows, title: &str) -> bool {
    let texts: Vec<&String> = rows.groups.iter().filter_map(|g| g.2.as_ref()).collect();
    rows.groups.len() == 1 && !texts.is_empty() && title.contains(texts[0].as_str())
}

/// The supply of a refusal of a time axis too narrow for its labels (R2-40).
pub(crate) fn too_narrow(e: &Crowded, width: f64, n: usize, clear: f64) -> String {
    let need = width + n as f64 * (e.need_slot - e.slot) + e.wmax + 2. * clear;
    format!("layout.width at least {} (or a coarser grid)", need.ceil())
}

#[allow(dead_code)]
pub(crate) fn period_name(p: &Period) -> String {
    let s = p.start;
    match p.grid.as_str() {
        "year" => s.year().to_string(),
        "bin" => format!("{}–{}", s.year(), p.stop.map_or(s.year(), |x| x.year() - 1)),
        "quarter" => format!("Q{} {}", p.quarter, s.year()),
        "month" => format!("{} {}", MONTHS_INLINE[s.month0() as usize], s.year()),
        _ => {
            let day = format!(
                "{} {}, {}",
                MONTHS_INLINE[s.month0() as usize],
                s.day(),
                s.year()
            );
            match p.grid.as_str() {
                "week" => format!("week of {day}"),
                "hour" => format!("{}, {day}", row1(p, "full")),
                _ => day,
            }
        }
    }
}
