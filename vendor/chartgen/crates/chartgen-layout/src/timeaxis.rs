//! Measured two-row labels for regular calendar grids.
use super::*;
use chrono::{Datelike, NaiveDate};

#[derive(Clone)]
struct Period {
    full: TextContent,
    narrow: Option<TextContent>,
    group: Option<String>,
}

fn month(m: u32) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec",
    ][(m - 1) as usize]
}
fn month_narrow(m: u32) -> &'static str {
    ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"][(m - 1) as usize]
}
fn group_date(date: NaiveDate) -> String {
    format!("{} {}, {}", month(date.month()), date.day(), date.year())
}

fn parse(grid: &str, raw: &str) -> Result<(String, Option<String>, Option<String>)> {
    let invalid = || err(format!("time period {raw} does not match {grid} grid"));
    match grid {
        "year" => {
            let year = raw.parse::<i32>().map_err(|_| invalid())?;
            Ok((year.to_string(), None, None))
        }
        "month" => {
            let d = NaiveDate::parse_from_str(&format!("{raw}-01"), "%Y-%m-%d")
                .map_err(|_| invalid())?;
            Ok((
                month(d.month()).into(),
                Some(month_narrow(d.month()).into()),
                Some(d.year().to_string()),
            ))
        }
        "quarter" => {
            let (year, q) = raw.split_once("-Q").ok_or_else(invalid)?;
            let year = year.parse::<i32>().map_err(|_| invalid())?;
            let q = q.parse::<u32>().map_err(|_| invalid())?;
            if !(1..=4).contains(&q) {
                return Err(invalid());
            }
            Ok((format!("Q{q}"), None, Some(year.to_string())))
        }
        "day" => {
            let d = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| invalid())?;
            Ok((
                d.day().to_string(),
                None,
                Some(format!("{} {}", month(d.month()), d.year())),
            ))
        }
        "week" => {
            let d = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| invalid())?;
            let end = d + chrono::Duration::days(6);
            let tail = if d.month() == end.month() {
                end.day().to_string()
            } else {
                format!("{} {}", month(end.month()), end.day())
            };
            Ok((
                format!("{} {}–{tail}", month(d.month()), d.day()),
                None,
                Some(d.year().to_string()),
            ))
        }
        "hour" => {
            let (date, hour) = raw.split_once('T').ok_or_else(invalid)?;
            let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| invalid())?;
            let h = hour.parse::<u32>().map_err(|_| invalid())?;
            if h > 23 {
                return Err(invalid());
            }
            Ok((
                format!("{} {}", (h % 12).max(1), if h < 12 { "AM" } else { "PM" }),
                None,
                Some(group_date(d)),
            ))
        }
        "bin" => Ok((raw.to_owned(), None, None)),
        _ => Err(err(format!("unsupported regular time grid {grid}"))),
    }
}

pub fn applies(plan: &SemanticPlan, data: &PreparedData) -> bool {
    data.x_kind == "time"
        && data.irregular.is_none()
        && plan
            .spec
            .raw
            .pointer("/data/x/grid")
            .and_then(|v| v.as_str())
            .is_some_and(|g| g != "none")
}

fn periods(plan: &SemanticPlan, data: &PreparedData) -> Result<Vec<Period>> {
    let grid = plan
        .spec
        .raw
        .pointer("/data/x/grid")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("regular time grid is missing"))?;
    data.categories
        .iter()
        .map(|raw| {
            let (full, narrow, group) = parse(grid, raw)?;
            if let Some(mapped) = plan.spec.raw["data"]["x"]["names"].get(raw) {
                if let Some(content) = guides::content(mapped)? {
                    return Ok(Period {
                        full: content,
                        narrow: None,
                        group,
                    });
                }
            }
            Ok(Period {
                full: TextContent::plain(full),
                narrow: narrow.map(TextContent::plain),
                group,
            })
        })
        .collect()
}

pub fn requests(
    plan: &SemanticPlan,
    data: &PreparedData,
    theme: &Theme,
    c: &LayoutConstraints,
) -> Result<Vec<TextRequest>> {
    if !applies(plan, data) {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for p in periods(plan, data)? {
        out.push(guides::request(
            "time-label",
            p.full,
            theme.font_size,
            None,
            theme,
            c,
        )?);
        if let Some(narrow) = p.narrow {
            out.push(guides::request(
                "time-label",
                narrow,
                theme.font_size,
                None,
                theme,
                c,
            )?);
        }
        if let Some(group) = p.group {
            out.push(guides::request(
                "time-label-2",
                TextContent::plain(group),
                theme.font_size * 0.9,
                None,
                theme,
                c,
            )?);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}

fn measured<'a>(
    requests: &'a [TextRequest],
    m: &'a Measurements,
    role: &str,
    content: &TextContent,
) -> Result<(&'a TextRequest, &'a TextMetrics)> {
    let r = requests
        .iter()
        .find(|r| r.role == role && &r.content == content)
        .ok_or_else(|| err("missing time label request"))?;
    let metrics = m
        .entries
        .get(&r.id)
        .ok_or_else(|| err("missing time label metrics"))?;
    Ok((r, metrics))
}

pub fn place(
    plan: &SemanticPlan,
    data: &PreparedData,
    theme: &Theme,
    c: &LayoutConstraints,
    m: &Measurements,
    requests: &[TextRequest],
    plot: &Bounds,
    order: &[usize],
    out: &mut Vec<Primitive>,
    findings: &mut Vec<Finding>,
) -> Result<()> {
    if !applies(plan, data) {
        return Ok(());
    }
    if plan.orientation != Orientation::Vertical {
        return Err(err("regular time labels need a vertical time axis"));
    }
    let all = periods(plan, data)?;
    let ordered = order
        .iter()
        .map(|&i| {
            all.get(i)
                .cloned()
                .ok_or_else(|| err("time axis order index out of range"))
        })
        .collect::<Result<Vec<_>>>()?;
    let n = ordered.len();
    if n == 0 {
        return Err(err("time axis has no periods"));
    }
    let band = plot.width / n as f64;
    let max_width = |narrow: bool| -> Result<f64> {
        ordered
            .iter()
            .map(|p| {
                let content = if narrow {
                    p.narrow.as_ref().unwrap_or(&p.full)
                } else {
                    &p.full
                };
                measured(requests, m, "time-label", content).map(|(_, ext)| ext.width)
            })
            .collect::<Result<Vec<_>>>()
            .map(|v| v.into_iter().fold(0., f64::max))
    };
    let sep = theme.font_size * 0.6;
    let (narrow, stride, widest) = if max_width(false)? + sep <= band {
        (false, 1, max_width(false)?)
    } else if ordered.iter().all(|p| p.narrow.is_some()) && max_width(true)? + sep <= band {
        (true, 1, max_width(true)?)
    } else {
        let w = max_width(false)?;
        (false, ((w + sep) / band).ceil() as usize, w)
    };
    if stride > 7 {
        return Err(err(format!(
            "regular time labels need every {stride}th period; at most every seventh is supported"
        )));
    }
    let mut groups = Vec::<(usize, usize, Option<String>)>::new();
    for (i, p) in ordered.iter().enumerate() {
        if let Some(last) = groups.last_mut() {
            if last.2 == p.group {
                last.1 = i;
                continue;
            }
        }
        groups.push((i, i, p.group.clone()));
    }
    let mut shown = Vec::new();
    for (first, last, _) in &groups {
        shown.extend((*first..=*last).step_by(stride));
    }
    let mut kept: Vec<usize> = Vec::new();
    for i in shown {
        if kept
            .last()
            .is_some_and(|&last| (i - last) as f64 * band < widest + sep)
        {
            kept.pop();
        }
        kept.push(i);
    }
    for i in kept {
        let p = &ordered[i];
        let content = if narrow {
            p.narrow.as_ref().unwrap_or(&p.full)
        } else {
            &p.full
        };
        let (r, ext) = measured(requests, m, "time-label", content)?;
        let x = plot.x + (i as f64 + 0.5) * band;
        if x - ext.width / 2. < 0. || x + ext.width / 2. > c.width {
            return Err(err("time label exceeds chart edge"));
        }
        out.push(text(r, ext, x, plot.y + plot.height + 6., "middle"));
    }
    for (j, (first, last, group)) in groups.iter().enumerate() {
        if j > 0 {
            let x = plot.x + *first as f64 * band;
            out.push(line(
                x,
                plot.y,
                x,
                plot.y + plot.height + 26.,
                &theme.grid,
                "time-divider",
            ));
        }
        let Some(group) = group else { continue };
        let (r, ext) = measured(requests, m, "time-label-2", &TextContent::plain(group))?;
        if (*last - *first + 1) as f64 * band < ext.width + sep {
            findings.push(Finding::new(
                "time-axis",
                "info",
                format!("time group {group} is too narrow for its second-row label"),
            ));
            continue;
        }
        let x = plot.x + (*first as f64 + *last as f64 + 1.) * band / 2.;
        out.push(text(r, ext, x, plot.y + plot.height + 21., "middle"));
    }
    findings.push(Finding::new(
        "time-axis",
        "open",
        format!(
            "regular time labels shown every {stride} period(s){}",
            if narrow { " in narrow form" } else { "" }
        ),
    ));
    Ok(())
}
