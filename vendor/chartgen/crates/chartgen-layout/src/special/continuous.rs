//! A true, linear time axis for irregular series (the original's `timeaxis.continuous_fit`
//! and its helpers): row 1 names periods of one unit (hours, days, months or years), each
//! centred on its span (an hour at its instant), thinned every k-th restarting in each
//! row-2 group; row 2 names the next unit, with dividers at the group boundaries and both
//! ends when there are two groups or more; short ticks at the unit's boundaries and minor
//! ticks at the data's grain where they fall 3 pt apart.
use chartgen_model::Error;
use chartgen_model::Result;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};

pub(crate) const MONTHS_AXIS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub(crate) const MONTHS_NARROW: [&str; 12] =
    ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"];
pub(crate) const MONTHS_INLINE: [&str; 12] = [
    "Jan.", "Feb.", "Mar.", "Apr.", "May", "June", "July", "Aug.", "Sept.", "Oct.", "Nov.", "Dec.",
];
pub(crate) const UNITS: [&str; 4] = ["hour", "day", "month", "year"];
const MIN_TICK_GAP: f64 = 3.0;

pub(crate) fn grain_order(g: &str) -> i32 {
    match g {
        "second" | "minute" => 0,
        "hour" => 1,
        "day" => 2,
        "month" => 3,
        "year" => 4,
        _ => 2,
    }
}

fn steps(unit: &str) -> &'static [usize] {
    match unit {
        "hour" => &[1, 2, 3, 6, 12],
        "day" => &[1, 2, 3],
        "month" => &[1, 2, 3, 6],
        _ => &[1, 2, 5, 10, 20],
    }
}

fn nth(k: usize) -> String {
    match k {
        2 => "second".into(),
        3 => "third".into(),
        5 => "fifth".into(),
        6 => "sixth".into(),
        10 => "tenth".into(),
        12 => "twelfth".into(),
        20 => "twentieth".into(),
        _ => format!("{k}th"),
    }
}

/// Seconds between two instants, as the original's `timedelta.total_seconds()`.
pub(crate) fn secs(a: NaiveDateTime, b: NaiveDateTime) -> f64 {
    (b - a).num_microseconds().map_or_else(
        || (b - a).num_milliseconds() as f64 / 1e3,
        |us| us as f64 / 1e6,
    )
}

/// (instant, resolution) of an irregular series' time: a day, an hour, a minute or a second.
pub(crate) fn parse_instant(text: &str) -> Result<(NaiveDateTime, &'static str)> {
    for (f, res) in [
        ("%Y-%m-%dT%H:%M:%S", "second"),
        ("%Y-%m-%dT%H:%M", "minute"),
    ] {
        if let Ok(t) = NaiveDateTime::parse_from_str(text, f) {
            return Ok((t, res));
        }
    }
    if let Some((d, h)) = text.split_once('T') {
        if h.len() == 2 && h.bytes().all(|b| b.is_ascii_digit()) {
            if let (Ok(day), Ok(hour)) =
                (NaiveDate::parse_from_str(d, "%Y-%m-%d"), h.parse::<u32>())
            {
                if let Some(t) = day.and_hms_opt(hour, 0, 0) {
                    return Ok((t, "hour"));
                }
            }
        }
    } else if let Ok(day) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Ok((day.and_hms_opt(0, 0, 0).unwrap(), "day"));
    }
    Err(Error::new(
        "layout",
        format!("{text:?} is not a time (YYYY-MM-DD, or with THH, THH:MM, THH:MM:SS)"),
    ))
}

pub(crate) fn unit_floor(t: NaiveDateTime, unit: &str) -> NaiveDateTime {
    let d = t.date();
    match unit {
        "hour" => d.and_hms_opt(t.hour(), 0, 0).unwrap(),
        "day" => d.and_hms_opt(0, 0, 0).unwrap(),
        "month" => NaiveDate::from_ymd_opt(d.year(), d.month(), 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        _ => NaiveDate::from_ymd_opt(d.year(), 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
    }
}

/// The start of the unit after the one holding t.
pub(crate) fn unit_next(t: NaiveDateTime, unit: &str) -> NaiveDateTime {
    let s = unit_floor(t, unit);
    match unit {
        "hour" => s + Duration::hours(1),
        "day" => s + Duration::days(1),
        "month" => {
            let (y, m) = (s.year(), s.month());
            NaiveDate::from_ymd_opt(y + i32::from(m == 12), m % 12 + 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
        }
        _ => NaiveDate::from_ymd_opt(s.year() + 1, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Period {
    pub unit: &'static str,
    pub start: NaiveDate,
    pub hour: u32,
}

impl Period {
    /// The row-2 group: a day, a month or a year (none under years).
    pub fn group(&self) -> Option<Vec<i64>> {
        let s = self.start;
        match self.unit {
            "month" => Some(vec![s.year() as i64]),
            "day" => Some(vec![s.year() as i64, s.month() as i64]),
            "hour" => Some(vec![s.year() as i64, s.month() as i64, s.day() as i64]),
            _ => None,
        }
    }
}

/// Row 1: "Jan" (narrow "J"), "2021", "16", "6 AM".
pub(crate) fn row1(p: &Period, form: &str) -> String {
    match p.unit {
        "month" => (if form == "narrow" {
            MONTHS_NARROW
        } else {
            MONTHS_AXIS
        })[p.start.month0() as usize]
            .to_owned(),
        "year" => p.start.year().to_string(),
        "day" => p.start.day().to_string(),
        _ => {
            let h = if p.hour % 12 == 0 { 12 } else { p.hour % 12 };
            format!("{h} {}", if p.hour < 12 { "AM" } else { "PM" })
        }
    }
}

/// Row 2 under its group: "2022", "Oct. 2022", "Mar. 11, 2022".
pub(crate) fn row2(group: &[i64]) -> String {
    match group.len() {
        1 => group[0].to_string(),
        2 => format!("{} {}", MONTHS_INLINE[(group[1] - 1) as usize], group[0]),
        _ => format!(
            "{} {}, {}",
            MONTHS_INLINE[(group[1] - 1) as usize],
            group[2],
            group[0]
        ),
    }
}

fn unit_period(t: NaiveDateTime, unit: &'static str) -> Period {
    match unit {
        "hour" => Period {
            unit,
            start: t.date(),
            hour: t.hour(),
        },
        "day" => Period {
            unit,
            start: t.date(),
            hour: 0,
        },
        "month" => Period {
            unit,
            start: NaiveDate::from_ymd_opt(t.year(), t.month(), 1).unwrap(),
            hour: 0,
        },
        _ => Period {
            unit: "year",
            start: NaiveDate::from_ymd_opt(t.year(), 1, 1).unwrap(),
            hour: 0,
        },
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Span {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub full_start: NaiveDateTime,
    pub full_end: NaiveDateTime,
    pub period: Period,
}

impl Span {
    fn partial(&self) -> bool {
        self.start > self.full_start || self.end < self.full_end
    }
}

/// The periods of `unit` that meet [t0, t1), each clipped to it.
pub(crate) fn spans(t0: NaiveDateTime, t1: NaiveDateTime, unit: &'static str) -> Vec<Span> {
    let mut out = vec![];
    let mut s = unit_floor(t0, unit);
    while s < t1 {
        let e = unit_next(s, unit);
        out.push(Span {
            start: s.max(t0),
            end: e.min(t1),
            full_start: s,
            full_end: e,
            period: unit_period(s, unit),
        });
        s = e;
        if out.len() > 100_000 {
            break;
        }
    }
    out
}

#[derive(Debug, Clone)]
pub(crate) struct ContinuousAxis {
    pub unit: &'static str,
    pub k: usize,
    /// (where it is centred, text) of the row-1 labels shown.
    pub labels: Vec<(NaiveDateTime, String)>,
    /// (start, end, text or none) of row 2, clipped.
    pub groups: Vec<(NaiveDateTime, NaiveDateTime, Option<String>)>,
    /// (instant, major).
    pub ticks: Vec<(NaiveDateTime, bool)>,
    pub dividers: Vec<NaiveDateTime>,
    pub decisions: Vec<String>,
}

/// No unit labels the axis without crowding: how much wider the axis must be.
#[derive(Debug, Clone)]
pub(crate) struct Crowded {
    pub factor: Option<f64>,
}

fn centre(s: &Span, unit: &str) -> NaiveDateTime {
    if unit == "hour" {
        s.start
    } else {
        s.start + (s.end - s.start) / 2
    }
}

type Width<'a> = dyn Fn(&str, &str) -> Result<f64> + 'a;
type XOf<'a> = dyn Fn(NaiveDateTime) -> f64 + 'a;

#[allow(clippy::type_complexity)]
fn try_labels(
    sp: &[Span],
    unit: &str,
    form: &str,
    k: usize,
    x_of: &XOf,
    w1: &dyn Fn(&str) -> Result<f64>,
    sep: f64,
    clear: Option<f64>,
) -> Result<Option<(Vec<(usize, String)>, Vec<String>)>> {
    let mut groups: Vec<(Vec<usize>, Option<Vec<i64>>)> = vec![];
    for (i, s) in sp.iter().enumerate() {
        let g = s.period.group();
        match groups.last_mut() {
            Some(last) if last.1 == g => last.0.push(i),
            _ => groups.push((vec![i], g)),
        }
    }
    let mut shown = vec![];
    for (idx, _) in &groups {
        for (n, &i) in idx.iter().enumerate() {
            let key = if unit == "hour" {
                sp[i].full_start.hour() as usize
            } else {
                n
            };
            if key % k == 0 {
                shown.push(i);
            }
        }
    }
    let inner: Vec<f64> = if clear.is_some() {
        groups[1..]
            .iter()
            .map(|(idx, _)| x_of(sp[idx[0]].start))
            .collect()
    } else {
        vec![]
    };
    let mut out: Vec<(f64, f64, usize, String)> = vec![];
    for i in shown {
        let s = &sp[i];
        let text = row1(&s.period, form);
        let w = w1(&text)?;
        let c = x_of(centre(s, unit));
        if s.partial() && unit != "hour" && x_of(s.end) - x_of(s.start) < w {
            continue;
        }
        if unit == "hour" && s.start != s.full_start {
            continue;
        }
        if let Some(&(pc, pw, pi, _)) = out.last() {
            if c - pc < (w + pw) / 2. + sep {
                if sp[pi].period.group() != s.period.group() {
                    out.pop();
                } else {
                    return Ok(None);
                }
            }
        }
        out.push((c, w, i, text));
    }
    let clear = clear.unwrap_or(0.);
    let near = out
        .iter()
        .filter(|(c, w, _, _)| inner.iter().any(|xb| (c - xb).abs() < w / 2. + clear))
        .map(|(_, _, _, t)| t.clone())
        .collect();
    Ok(Some((
        out.into_iter().map(|(_, _, i, t)| (i, t)).collect(),
        near,
    )))
}

#[allow(clippy::too_many_arguments)]
fn axis(
    sp: &[Span],
    unit: &'static str,
    form: &str,
    k: usize,
    got: Vec<(usize, String)>,
    t0: NaiveDateTime,
    t1: NaiveDateTime,
    x_of: &XOf,
    width: &Width,
    sep: f64,
    grain: &str,
    title: &str,
) -> Result<ContinuousAxis> {
    let labels = got
        .iter()
        .map(|(i, t)| (centre(&sp[*i], unit), t.clone()))
        .collect();
    let mut groups: Vec<(
        NaiveDateTime,
        NaiveDateTime,
        Option<String>,
        Option<Vec<i64>>,
    )> = vec![];
    let mut dropped = vec![];
    if unit != "year" {
        for s in sp {
            let g = s.period.group();
            match groups.last_mut() {
                Some(cur) if cur.3 == g => cur.1 = s.end,
                _ => groups.push((s.start, s.end, g.as_deref().map(row2), g)),
            }
        }
        for g in &mut groups {
            if let Some(t) = &g.2 {
                if x_of(g.1) - x_of(g.0) < width("time-label-2", t)? + sep {
                    dropped.push(t.clone());
                    g.2 = None;
                }
            }
        }
    }
    let mut groups: Vec<(NaiveDateTime, NaiveDateTime, Option<String>)> =
        groups.into_iter().map(|(a, b, t, _)| (a, b, t)).collect();
    let mut decisions = vec![format!(
        "a true time axis (G2.42): row 1 {unit}s{}{}{}",
        if form == "narrow" { " (narrow)" } else { "" },
        if k > 1 {
            format!(
                ", every {}{}",
                nth(k),
                if unit == "hour" {
                    ""
                } else {
                    ", restarting in each group"
                }
            )
        } else {
            String::new()
        },
        if unit == "hour" {
            ", each at its instant"
        } else {
            ", each centred on its span"
        }
    )];
    if !dropped.is_empty() {
        decisions.push(format!(
            "time axis: row 2 drops {}, its span too narrow for the text",
            dropped.join(", ")
        ));
    }
    let texts: Vec<String> = groups.iter().filter_map(|g| g.2.clone()).collect();
    if groups.len() == 1 && !texts.is_empty() && title.contains(texts[0].as_str()) {
        decisions.push(format!(
            "time axis: row 2 (“{}”) left out, the title gives it (G2.37)",
            texts[0]
        ));
        groups = groups.into_iter().map(|(a, b, _)| (a, b, None)).collect();
    }
    let mut ticks: Vec<(NaiveDateTime, bool)> = sp
        .iter()
        .filter(|s| t0 <= s.full_start && s.full_start <= t1)
        .map(|s| (s.full_start, true))
        .collect();
    if let Some(last) = sp.last() {
        if last.full_end <= t1 {
            ticks.push((last.full_end, true));
        }
    }
    let minor = match unit {
        "month" => Some("day"),
        "year" => Some("month"),
        "day" => Some("hour"),
        _ => None,
    };
    if let Some(mu) = minor {
        if grain_order(mu) >= grain_order(grain) {
            let ms = spans(t0, t1, mu);
            if !ms.is_empty()
                && ms
                    .iter()
                    .filter(|s| !s.partial())
                    .all(|s| x_of(s.full_end) - x_of(s.full_start) >= MIN_TICK_GAP)
            {
                let majors: Vec<NaiveDateTime> = ticks.iter().map(|t| t.0).collect();
                ticks.extend(
                    ms.iter()
                        .filter(|s| {
                            t0 <= s.full_start
                                && s.full_start <= t1
                                && !majors.contains(&s.full_start)
                        })
                        .map(|s| (s.full_start, false)),
                );
                decisions.push(format!("minor ticks at each {mu} (D-45 (ii))"));
            }
        }
    }
    ticks.sort();
    let mut dividers = vec![];
    if groups.len() >= 2 {
        dividers.push(t0);
        dividers.extend(groups[1..].iter().map(|g| g.0));
        dividers.push(t1);
    }
    Ok(ContinuousAxis {
        unit,
        k,
        labels,
        groups,
        ticks,
        dividers,
        decisions,
    })
}

/// The rows of a true time axis over [t0, t1), or Crowded when no unit fits.
#[allow(clippy::too_many_arguments)]
pub(crate) fn continuous_fit(
    t0: NaiveDateTime,
    t1: NaiveDateTime,
    x_of: &XOf,
    width: &Width,
    sep: f64,
    grain: &str,
    title: &str,
    clear: Option<f64>,
) -> Result<std::result::Result<ContinuousAxis, Crowded>> {
    let w1 = |t: &str| width("time-label", t);
    let mut last: Option<(Vec<Span>, &'static str, &str, usize)> = None;
    for unit in UNITS {
        if grain_order(unit) < grain_order(grain)
            && !(unit == "hour" && matches!(grain, "minute" | "second"))
        {
            continue;
        }
        if unit == "hour" && (t1 - t0) > Duration::days(4) {
            continue;
        }
        let sp = spans(t0, t1, unit);
        if sp.len() > 400 {
            continue;
        }
        let forms: &[&str] = if unit == "month" {
            &["full", "narrow"]
        } else {
            &["full"]
        };
        for &form in forms {
            for &k in steps(unit) {
                if k > 1 && form == "narrow" {
                    break;
                }
                if let Some((got, near)) = try_labels(&sp, unit, form, k, x_of, &w1, sep, clear)? {
                    let mut ax = axis(
                        &sp, unit, form, k, got, t0, t1, x_of, width, sep, grain, title,
                    )?;
                    if !near.is_empty() {
                        ax.decisions.push(format!(
                            "time axis: {} closer than {} pt to the divider between two groups: kept, with a halo in the page colour round its letters",
                            near.iter().map(|t| format!("“{t}”")).collect::<Vec<_>>().join(", "),
                            clear.unwrap_or(0.)
                        ));
                    }
                    return Ok(Ok(ax));
                }
                last = Some((sp.clone(), unit, form, k));
            }
        }
    }
    let factor = match last {
        Some((sp, unit, form, k)) => {
            let cs: Vec<(f64, f64)> = (0..sp.len())
                .filter(|n| n % k == 0)
                .map(|i| Ok((x_of(centre(&sp[i], unit)), w1(&row1(&sp[i].period, form))?)))
                .collect::<Result<_>>()?;
            cs.windows(2)
                .filter(|p| p[1].0 > p[0].0)
                .map(|p| ((p[0].1 + p[1].1) / 2. + sep) / (p[1].0 - p[0].0))
                .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
        }
        None => None,
    };
    Ok(Err(Crowded { factor }))
}

/// How far the first and last row-1 labels come inside `clear` of the outer dividers.
pub(crate) fn continuous_edge(
    ax: &ContinuousAxis,
    x_of: &XOf,
    plot_l: f64,
    plot_r: f64,
    width: &Width,
    clear: f64,
) -> Result<(f64, f64)> {
    if ax.dividers.is_empty() || ax.labels.is_empty() {
        return Ok((0., 0.));
    }
    let (ta, a) = &ax.labels[0];
    let (tb, b) = &ax.labels[ax.labels.len() - 1];
    let left = clear - (x_of(*ta) - width("time-label", a)? / 2. - plot_l);
    let right = clear - (plot_r - (x_of(*tb) + width("time-label", b)? / 2.));
    Ok((left.max(0.), right.max(0.)))
}

/// A time as a report or note names it: "Apr. 27, 2022", "6 AM, Aug. 14, 2022".
pub(crate) fn instant_name(t: NaiveDateTime, resolution: &str) -> String {
    let day = format!(
        "{} {}, {}",
        MONTHS_INLINE[t.month0() as usize],
        t.day(),
        t.year()
    );
    if matches!(resolution, "hour" | "minute" | "second") {
        let h = if t.hour() % 12 == 0 {
            12
        } else {
            t.hour() % 12
        };
        let mm = if t.minute() != 0 || matches!(resolution, "minute" | "second") {
            format!(":{:02}", t.minute())
        } else {
            String::new()
        };
        return format!("{h}{mm} {}, {day}", if t.hour() < 12 { "AM" } else { "PM" });
    }
    day
}

/// A time as data-x: 2022-04-27, or 2022-08-14T06:30.
pub(crate) fn stamp(t: NaiveDateTime) -> String {
    if t.hour() == 0 && t.minute() == 0 && t.second() == 0 {
        t.date().format("%Y-%m-%d").to_string()
    } else {
        t.format("%Y-%m-%dT%H:%M").to_string()
    }
}

/// ISO form of an instant, as the original reports it ("2022-04-01T00:00:00").
pub(crate) fn iso(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}
