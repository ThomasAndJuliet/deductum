//! Irregular time point parsing; keeps exact instants and coincident events.
use super::{datum, err};
use chartgen_model::{Datum, Decimal, Result};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::Value;

fn instant(text: &str) -> Option<NaiveDateTime> {
    let (day, time) = text
        .split_once('T')
        .map_or((text, None), |(d, t)| (d, Some(t)));
    let day = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let time = match time {
        None => NaiveTime::from_hms_opt(0, 0, 0)?,
        Some(s) => ["%H", "%H:%M", "%H:%M:%S"]
            .iter()
            .find_map(|f| NaiveTime::parse_from_str(s, f).ok())?,
    };
    Some(day.and_time(time))
}
/// A point's source timestamp, exact numeric position in seconds, and value.
#[derive(Debug, Clone)]
pub struct ParsedPoint {
    pub time: String,
    pub position: Decimal,
    pub datum: Datum,
}
/// Parse one series' [[time,value]] points. Equal times are valid; descending times are not.
pub fn parse_points(series: &Value, series_index: usize) -> Result<Vec<ParsedPoint>> {
    let path = format!("data.series[{series_index}].points");
    let points = series["points"]
        .as_array()
        .ok_or_else(|| err(&path, "irregular time takes points"))?;
    if points.is_empty() {
        return Err(err(&path, "at least one point is needed"));
    }
    let mut out = Vec::with_capacity(points.len());
    let mut previous = None;
    for (j, p) in points.iter().enumerate() {
        let point_path = format!("{path}[{j}]");
        let pair = p
            .as_array()
            .filter(|p| p.len() == 2)
            .ok_or_else(|| err(&point_path, "a point is [time, value]"))?;
        let time = pair[0]
            .as_str()
            .ok_or_else(|| err(&format!("{point_path}[0]"), "not a time string"))?;
        let stamp = instant(time).ok_or_else(|| {
            err(
                &format!("{point_path}[0]"),
                "not a time (YYYY-MM-DD, or with THH, THH:MM, THH:MM:SS)",
            )
        })?;
        let position = Decimal::from(stamp.and_utc().timestamp());
        if previous.is_some_and(|old| position < old) {
            return Err(err(&point_path, "points must be in time order"));
        }
        previous = Some(position);
        out.push(ParsedPoint {
            time: time.into(),
            position,
            datum: datum(&pair[1], &format!("{point_path}[1]"))?,
        });
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn distinct_and_coincident_instants() {
        let s = json!({"points":[["2022-08-14T05:20",200],["2022-08-14T05:20",80],["2022-08-14T05:50",140]]});
        let p = parse_points(&s, 0).unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].position, p[1].position);
        assert!(p[2].position > p[1].position);
    }
    #[test]
    fn descending_is_invalid() {
        let s = json!({"points":[["2022-08-15",1],["2022-08-14",2]]});
        assert_eq!(
            parse_points(&s, 0).unwrap_err().path.as_deref(),
            Some("data.series[0].points[1]")
        );
    }
}
