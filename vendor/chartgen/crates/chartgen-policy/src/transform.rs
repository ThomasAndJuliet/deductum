//! The report of derived measures and comparison values (`transform.apply`, `_units`,
//! `for_type`, `_missing`, `_far`). The values themselves are computed by `chartgen-data`.
use crate::rules::{decide, finding, guided, refusal, warn};
use crate::view::{View, dec, plain};
use chartgen_format as fmt;
use chartgen_model::{Decimal, PreparedData, Report};
use serde_json::Value;

fn kind_name(kind: &str) -> String {
    match kind {
        "target" => "Target",
        "budget" => "Budget",
        "tolerance" => "Tolerance",
        "forecast" => "Forecast",
        "average" => "Average",
        "previous_period" => "Previous period",
        "peer" => "Peers",
        k => k,
    }
    .into()
}
fn sides(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "target" => ("above target", "below target"),
        "budget" => ("over budget", "under budget"),
        "tolerance" => ("over tolerance", "within tolerance"),
        "forecast" => ("above forecast", "below forecast"),
        "average" => ("above average", "below average"),
        "previous_period" => (
            "more than the previous period",
            "less than the previous period",
        ),
        "peer" => ("above peers", "below peers"),
        _ => ("above", "below"),
    }
}
/// `transform.label_of`: a comparison's label, plain.
fn label_of(c: &Value) -> String {
    crate::view::text(&c["label"]).unwrap_or_else(|| kind_name(c["kind"].as_str().unwrap_or("")))
}
/// `transform.named`: as a report's sentence names it.
fn named(c: &Value) -> String {
    crate::view::text(&c["label"])
        .unwrap_or_else(|| kind_name(c["kind"].as_str().unwrap_or("")).to_lowercase())
}
fn rank(kind: &str) -> u32 {
    match kind {
        "target" | "budget" | "tolerance" | "forecast" => 0,
        "average" => 1,
        "previous_period" => 2,
        "peer" => 3,
        _ => 9,
    }
}
fn comparisons(v: &View) -> Vec<Value> {
    v.get("/data/comparisons")
        .as_array()
        .cloned()
        .unwrap_or_default()
}
fn unit_key(u: &Value) -> (String, String, String) {
    let prefix = u["prefix"].as_str().unwrap_or("").trim().to_owned();
    let suffix = u["suffix"].as_str().unwrap_or("").trim().to_owned();
    let noun = if prefix.is_empty() && suffix.is_empty() {
        u["noun"].as_str().unwrap_or("").trim().to_lowercase()
    } else {
        String::new()
    };
    (prefix, suffix, noun)
}
fn unit_magnitude(u: &Value) -> i64 {
    let m = u["magnitude"].as_i64().unwrap_or(0);
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
fn unit_words(u: &Value) -> String {
    let words: Vec<&str> = ["prefix", "suffix", "noun"]
        .iter()
        .filter_map(|k| u[*k].as_str().map(str::trim).filter(|s| !s.is_empty()))
        .collect();
    let words = if words.is_empty() {
        "no unit".to_owned()
    } else {
        words.join(" ")
    };
    let m = unit_magnitude(u);
    if m != 0 {
        format!("{words} (×10^{m})")
    } else {
        words
    }
}
fn pow10(e: i64) -> Decimal {
    if e < 0 {
        Decimal::from_i128_with_scale(1, (-e) as u32)
    } else {
        let mut x = Decimal::ONE;
        for _ in 0..e {
            x *= Decimal::from(10);
        }
        x
    }
}

/// `transform._units`: each comparison given in a unit of its own converted to the measure's, or
/// refused (R2-13). Returns true when refused.
pub fn units(v: &View, report: &mut Report) -> bool {
    let comps = comparisons(v);
    let measure_unit = v.get("/data/measure/unit");
    let mut stop = false;
    for (i, c) in comps.iter().enumerate() {
        if c["unit"].is_null() && c["factor"].is_null() {
            continue;
        }
        let p = format!("data.comparisons[{i}]");
        let cu = if c["unit"].is_null() {
            measure_unit
        } else {
            &c["unit"]
        };
        let same = unit_key(cu) == unit_key(measure_unit);
        let how = if let Some(f) = dec(&c["factor"]) {
            Some(format!("× {}", plain(f)))
        } else if same {
            let f = pow10(unit_magnitude(cu) - unit_magnitude(measure_unit));
            (f != Decimal::ONE).then(|| format!("× {}", plain(f)))
        } else {
            stop |= refusal(
                report,
                v,
                "R2-13",
                format!(
                    "the {} ({}) is given in {}, the measure in {}: drawn on the measure's scale it would be read in the measure's unit (one unit per scale, G2.30)",
                    named(c),
                    c["id"].as_str().unwrap_or(""),
                    unit_words(cu),
                    unit_words(measure_unit)
                ),
                Some(&format!("{p}.factor")),
                Some(
                    "factor, the number each value is multiplied by to give the measure's unit (0.001 from kWh to MWh), or the values in the measure's unit",
                ),
            );
            continue;
        };
        let Some(how) = how else { continue };
        decide(
            report,
            "R2-13",
            format!(
                "the {} ({}) converted from {} to the measure's {}: each value {how} (one unit per scale, G2.30)",
                named(c),
                c["id"].as_str().unwrap_or(""),
                unit_words(cu),
                unit_words(measure_unit)
            ),
            None,
        );
    }
    stop
}

/// The decisions of `transform.apply` for a derived measure (after `_units`), and its refusal of
/// an index without a start value (R15-16). Returns true when refused.
pub fn apply(v: &View, source: &PreparedData, report: &mut Report) -> bool {
    let t = v.get("/transform");
    if t.is_null() {
        return false;
    }
    let kind = t["kind"].as_str().unwrap_or("");
    let x = v.x_values();
    let mut stop = false;
    match kind {
        "deviation" => {
            let comps = comparisons(v);
            let id = t["comparison"].as_str().unwrap_or("");
            let Some(c) = comps.iter().find(|c| c["id"] == id) else {
                return false;
            };
            let relative = t["relative"] == true;
            let (above, _) = sides(c["kind"].as_str().unwrap_or(""));
            let label = label_of(c).to_lowercase();
            // `_deviate_others`: the other comparison values taken as deviations from this one
            for o in &comps {
                if o["id"] == c["id"]
                    || !o["compute"].is_null()
                    || o["render"].as_str() == Some("none")
                {
                    continue;
                }
                let one = deviate_is_one(v, source, o, id);
                let what = match one {
                    Some(value) => format!(
                        "{}{} for every category: one value, a line across the plot",
                        fmt::number(
                            value,
                            fmt::decimals(value).min(2),
                            fmt::Locale::default(),
                            "",
                            true
                        ),
                        if relative { "%" } else { "" }
                    ),
                    None => "one value per category, its marks in the derived unit".into(),
                };
                decide(
                    report,
                    "R15-14",
                    format!(
                        "the {} ({}) taken as a deviation from the {} too, so that it is drawn in the deviation's unit: {what} (R2-13)",
                        named(o),
                        o["id"].as_str().unwrap_or(""),
                        named(c)
                    ),
                    None,
                );
            }
            decide(
                report,
                "R15-14",
                format!(
                    "each value {} its {label} ({id}): a deviation, sorted by the deviation, the {label} itself the zero line; the unit says so (“{above}”) (G8.16)",
                    if relative { "in % of" } else { "minus" }
                ),
                None,
            );
            if let Some(good) = v.opt_str("/data/measure/good") {
                decide(
                    report,
                    "R15-11",
                    format!(
                        "the dividing value is 0, the {label} itself: {above} is {} (G0.20)",
                        if good == "higher" { "good" } else { "bad" }
                    ),
                    None,
                );
            }
        }
        "ratio" => {
            let e = &t["exposure"];
            let per = per_text(dec(&t["per"]).unwrap_or(Decimal::ONE));
            let u = v.get("/data/measure/unit");
            let base = u["noun"]
                .as_str()
                .or(u["what"].as_str())
                .or(v.opt_str("/data/measure/name"))
                .unwrap_or("values");
            let per_sp = if per.is_empty() {
                String::new()
            } else {
                format!("{per} ")
            };
            let noun = format!("{base} per {per_sp}{}", e["noun"].as_str().unwrap_or(""));
            decide(
                report,
                "R15-15",
                format!(
                    "each value per {per_sp}{}: a ratio, since the entities differ in size; the unit says so (“{noun}”) (G8.17)",
                    e["noun"].as_str().unwrap_or("")
                ),
                None,
            );
        }
        "index" => {
            let i0 = t["at"]
                .as_str()
                .and_then(|a| x.iter().position(|c| c == a))
                .unwrap_or(0);
            let when = x.get(i0).map(|c| when_text(v, c)).unwrap_or_default();
            let base = dec(&t["base"]).unwrap_or(Decimal::from(100));
            let raw = v
                .get("/data/series")
                .as_array()
                .cloned()
                .unwrap_or_default();
            let measures = v
                .get("/data/measures")
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mixed = raw
                .iter()
                .map(|s| s["measure"].as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1;
            for s in &source.series {
                let v0 = s.values.get(i0);
                let start = v0.and_then(|d| d.value);
                if start.is_none_or(|x| x.is_zero()) {
                    stop |= refusal(
                        report,
                        v,
                        "R15-16",
                        format!(
                            "the start value of {} ({when}) is {}: nothing to rebase it on",
                            s.id,
                            if start.is_some() { "zero" } else { "not known" }
                        ),
                        Some("transform.at"),
                        Some("a start where every series has a value"),
                    );
                }
            }
            let prefixes: std::collections::BTreeSet<Option<String>> = raw
                .iter()
                .map(|s| {
                    let own = s["measure"]
                        .as_str()
                        .and_then(|id| measures.iter().find(|m| m["id"].as_str() == Some(id)));
                    let u = own.map_or(v.get("/data/measure/unit"), |m| &m["unit"]);
                    u["prefix"].as_str().map(str::to_owned)
                })
                .collect();
            let prefix = if prefixes.len() == 1 {
                prefixes.into_iter().next().flatten()
            } else {
                None
            };
            let what = if base.is_zero() {
                "% change since".to_owned()
            } else {
                format!(
                    "{} at",
                    fmt::number(
                        base,
                        0,
                        fmt::Locale::default(),
                        prefix.as_deref().unwrap_or(""),
                        false
                    )
                )
            };
            decide(
                report,
                "R15-16",
                format!(
                    "every series rebased: {what} {when}, so that their patterns of change compare on one scale; the actual values are no longer shown (G8.18)"
                ),
                None,
            );
            if mixed {
                decide(
                    report,
                    "R2-17",
                    "an index chart: series in different units on one scale of change, instead of two value axes (R2-14, G2.31)",
                    None,
                );
            }
        }
        _ => return false,
    }
    decide(
        report,
        "R15-13",
        format!("a calculated value in place of the raw one: the {kind} (G8.15)"),
        None,
    );
    stop
}

/// Whether another comparison, as a deviation from comparison `base`, is one value for every
/// category (`_deviate_others`); its value when it is.
fn deviate_is_one(v: &View, source: &PreparedData, o: &Value, base: &str) -> Option<Decimal> {
    let oc = source
        .comparisons
        .iter()
        .find(|c| o["id"].as_str() == Some(c.id.as_str()))?;
    let bc = source.comparisons.iter().find(|c| c.id == base)?;
    let n = v.x_values().len();
    let each = |vals: &[chartgen_model::Datum], j: usize| {
        if vals.len() == 1 {
            vals[0].value
        } else {
            vals.get(j).and_then(|d| d.value)
        }
    };
    let relative = v.get("/transform/relative") == true;
    let mut out = vec![];
    for j in 0..n {
        match (each(&oc.values, j), each(&bc.values, j)) {
            (Some(a), Some(b)) => {
                if relative {
                    if b.is_zero() {
                        return None;
                    }
                    out.push((a - b) / b * Decimal::from(100));
                } else {
                    out.push(a - b);
                }
            }
            _ => return None,
        }
    }
    if out.len() <= 1 {
        return None;
    }
    let lo = *out.iter().min()?;
    let hi = *out.iter().max()?;
    let top = out.iter().map(|x| x.abs()).max()?.max(Decimal::ONE);
    if hi - lo > Decimal::new(1, 3) * top {
        return None;
    }
    let mean = out.iter().copied().sum::<Decimal>() / Decimal::from(out.len());
    Some(mean.round_dp(6).normalize())
}

/// `transform._per_text`: 100000 → "100K", 1000 → "1,000", 1 → "".
fn per_text(per: Decimal) -> String {
    let loc = fmt::Locale::default();
    if per == Decimal::ONE {
        return String::new();
    }
    let thousand = Decimal::from(1000);
    if per >= Decimal::from(100000) && (per % thousand).is_zero() {
        let k = per / thousand;
        if k >= thousand && (k % thousand).is_zero() {
            return format!("{} million", fmt::number(k / thousand, 0, loc, "", false));
        }
        return format!("{}K", fmt::number(k, 0, loc, "", false));
    }
    fmt::number(per, if per == per.trunc() { 0 } else { 2 }, loc, "", false)
}

/// `transform.for_type`'s report: which comparison values are drawn, and how (R15-7–R15-9,
/// R11-4), then `_missing` and `_far`. Returns true when refused.
pub fn for_type(v: &View, data: &PreparedData, kind: &str, report: &mut Report) -> bool {
    let comps = comparisons(v);
    if comps.is_empty() {
        missing(v, &[], report);
        return false;
    }
    // the render each comparison has after the derived measure (a deviation's own is "none")
    let prepared = |i: usize| data.comparisons.get(i);
    let mut render: Vec<String> = comps
        .iter()
        .enumerate()
        .map(|(i, c)| {
            prepared(i).map_or_else(
                || c["render"].as_str().unwrap_or("auto").to_owned(),
                |p| p.render.clone(),
            )
        })
        .collect();
    let valid: Vec<bool> = comps.iter().map(|c| c["valid"] != false).collect();
    let mut stop = false;
    for (i, c) in comps.iter().enumerate() {
        if valid[i] {
            continue;
        }
        let why = c["reason"]
            .as_str()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default();
        let id = c["id"].as_str().unwrap_or("");
        if render[i] != "auto" && render[i] != "none" {
            let refused = refusal(
                report,
                v,
                "R15-9",
                format!(
                    "the {} ({id}) is doubtful{why}: audiences latch onto comparison values, so no comparator is better than a doubtful one",
                    label_of(c).to_lowercase()
                ),
                Some(&format!("data.comparisons[{i}].render")),
                Some("render auto, or a valid comparison value"),
            );
            stop |= refused;
            if !refused {
                continue;
            }
        } else {
            warn(
                report,
                "R15-9",
                format!(
                    "the {} ({id}) is doubtful{why}: it is left out, since no comparator is better than a doubtful one (G0.19)",
                    label_of(c).to_lowercase()
                ),
                Some(&format!("data.comparisons[{i}]")),
            );
        }
        render[i] = "none".into();
    }
    let mut drawn: Vec<usize> = (0..comps.len())
        .filter(|&i| render[i] != "none" && (valid[i] || render[i] != "auto"))
        .collect();
    let best = comps
        .iter()
        .enumerate()
        .filter(|(i, _)| valid[*i])
        .map(|(_, c)| rank(c["kind"].as_str().unwrap_or("")))
        .min()
        .unwrap_or(9);
    for &i in &drawn {
        let c = &comps[i];
        let id = c["id"].as_str().unwrap_or("");
        let ck = c["kind"].as_str().unwrap_or("");
        if ck == "previous_period" {
            if best <= rank("average") {
                let why = "D-48: a previous period only where no target, budget or average exists";
                if render[i] == "auto" {
                    decide(
                        report,
                        "R15-7",
                        format!(
                            "the previous period ({id}) left out: a target, budget or average is there ({why})"
                        ),
                        Some("guided"),
                    );
                    render[i] = "none".into();
                } else {
                    let chosen = render[i].clone();
                    guided(
                        report,
                        v,
                        "R15-7",
                        &format!("data.comparisons[{i}].render"),
                        &chosen,
                        "none",
                        why,
                    );
                }
            } else {
                report.findings.push(finding("R15-7", "info", format!("the comparison value is a previous period ({id}): it has significant limitations; the same period a year earlier avoids seasonal effects (D-48)"), Some(&format!("data.comparisons[{i}]")), Some("guided")));
            }
        } else if ck == "peer" && best < rank("peer") {
            report.findings.push(finding("R15-7", "info", format!("a peer comparison ({id}) beside a better one: expectations and averages come first (G0.19)"), Some(&format!("data.comparisons[{i}]")), Some("guided")));
        }
    }
    drawn.retain(|&i| render[i] != "none");
    let n = v.x_values().len();
    let regular = v
        .series()
        .into_iter()
        .filter(|s| s.role == "regular")
        .count();
    for &i in &drawn {
        let c = &comps[i];
        if c["compute"].as_str() != Some("mean") {
            continue;
        }
        let what = if kind == "line" {
            format!("the mean of the {regular} series in each period")
        } else {
            let sid = c["series"].as_str();
            let s = sid
                .and_then(|id| data.series.iter().find(|s| s.id == id))
                .or_else(|| {
                    let first = v.series().into_iter().find(|s| s.role == "regular")?;
                    data.series.get(first.index)
                });
            let count = s.map_or(0, |s| s.values.iter().filter(|d| d.value.is_some()).count());
            format!("the mean of the {count} values")
        };
        decide(
            report,
            "R15-7",
            format!(
                "the {} computed from the plotted data: {what}",
                label_of(c).to_lowercase()
            ),
            None,
        );
    }
    for &i in &drawn {
        let c = &comps[i];
        let len = if c["compute"].as_str() == Some("mean") {
            if kind == "line" { n } else { 1 }
        } else {
            prepared(i).map_or_else(
                || c["values"].as_array().map_or(0, Vec::len),
                |p| p.values.len(),
            )
        };
        let per_x = len == n && n > 1;
        let want = if kind == "line" || per_x {
            "series"
        } else {
            "line"
        };
        let r = render[i].clone();
        if r != "auto" && r != "series" && !(r == "mark" && want == "series") && r != want {
            guided(
                report,
                v,
                "R11-4",
                &format!("data.comparisons[{i}].render"),
                &r,
                want,
                "one value per category as a flat mark across its bar, one value as a line across the plot, a line chart's comparison as a reference series (G8.13, G7.57, D-21)",
            );
        }
        if want == "line" {
            render[i] = "line".into();
            continue;
        }
        let ck = c["kind"].as_str().unwrap_or("");
        let label = label_of(c).to_lowercase();
        if kind == "line" {
            let role = if !c["compute"].is_null() || ck == "average" {
                "average"
            } else {
                "other"
            };
            let look = if role == "average" {
                "solid, in the average's dark grey"
            } else {
                "dotted, in the dark grey"
            };
            decide(
                report,
                "R15-7",
                format!(
                    "the {label} drawn as a reference series, {look}, labelled at its end in bold (D-21 (c), G7.16)"
                ),
                None,
            );
        } else {
            decide(
                report,
                "R15-7",
                format!(
                    "the {label} of each category drawn as a flat mark across its {}, named once (R11-4, G8.13, G5.5)",
                    if kind == "bar" { "bar" } else { "mark" }
                ),
                None,
            );
        }
        render[i] = "series".into();
    }
    missing(v, &render, report);
    far(v, data, &comps, &render, report);
    stop
}

/// `transform._missing`: R15-7, a good/bad reading with nothing to judge it by.
pub fn missing(v: &View, render: &[String], report: &mut Report) {
    let comps = comparisons(v);
    let drawn = comps.iter().enumerate().any(|(i, c)| {
        render
            .get(i)
            .map_or(c["render"].as_str().unwrap_or("auto"), String::as_str)
            != "none"
            && c["valid"] != false
    });
    let m = v.measure();
    if m["good"].is_string()
        && m["dividing_point"].is_null()
        && !drawn
        && v.job()
            .is_some_and(|k| ["answer", "persuade", "make_aware"].contains(&k))
        && v.get("/transform").is_null()
    {
        report.findings.push(finding("R15-7", "info", "the values are read as good or bad, with no comparison value or dividing value: a target, budget or average would say how to feel about them (G8.12, G0.19)", Some("data.comparisons"), Some("guided")));
    }
}

/// `transform._far`: R2-13, a comparison value an order of magnitude or more outside the data.
fn far(v: &View, data: &PreparedData, comps: &[Value], render: &[String], report: &mut Report) {
    let values: Vec<Decimal> = if let Some(points) = &data.irregular {
        points
            .iter()
            .flat_map(|row| row.iter().filter_map(|p| p.datum.value))
            .collect()
    } else {
        v.series()
            .iter()
            .filter(|s| s.role == "regular")
            .flat_map(|s| s.values.unwrap_or(&[]).iter().filter_map(|d| d.value))
            .collect()
    };
    if values.is_empty() {
        return;
    }
    let lo = *values.iter().min().unwrap();
    let hi = *values.iter().max().unwrap();
    let top = lo.abs().max(hi.abs());
    if top.is_zero() {
        return;
    }
    let ten = Decimal::from(10);
    let loc = fmt::Locale::default();
    let show = |x: Decimal| fmt::number(x, fmt::decimals(x).min(2), loc, "", false);
    for (i, c) in comps.iter().enumerate() {
        if render[i] == "none" || c["valid"] == false || !c["compute"].is_null() {
            continue;
        }
        let vals: Vec<Decimal> = data
            .comparisons
            .iter()
            .find(|p| c["id"].as_str() == Some(p.id.as_str()))
            .map(|p| p.values.iter().filter_map(|d| d.value).collect())
            .unwrap_or_default();
        let far: Vec<Decimal> = vals
            .into_iter()
            .filter(|&x| {
                x.abs() >= ten * top
                    || (!x.is_zero() && lo > Decimal::ZERO && x < lo && x * ten <= lo)
                    || (!x.is_zero() && hi < Decimal::ZERO && x > hi && x * ten >= hi)
            })
            .collect();
        if far.is_empty() {
            continue;
        }
        let fmin = *far.iter().min().unwrap();
        let fmax = *far.iter().max().unwrap();
        let got = if fmin != fmax {
            format!("{} to {}", show(fmin), show(fmax))
        } else {
            show(fmin)
        };
        warn(
            report,
            "R2-13",
            format!(
                "the {} ({}) lies at {got}, an order of magnitude or more outside the data ({} to {}): it is most likely given in another unit than the data's; give its unit (and factor) so that it is converted, or correct it",
                named(c),
                c["id"].as_str().unwrap_or(""),
                show(lo),
                show(hi)
            ),
            Some(&format!("data.comparisons[{i}].unit")),
        );
    }
}

/// `transform._when`: an x value as text names it ("Feb. 4, 2022", "Jan. 2022", "Central"),
/// through `timeaxis.period_name` on a regular time grid.
fn when_text(v: &View, value: &str) -> String {
    if v.x_kind() != "time" {
        return value.to_owned();
    }
    period_name(value, v.grid().unwrap_or("none")).unwrap_or_else(|| value.to_owned())
}

/// `timeaxis.period_name`: "Sept. 2022", "Oct. 16, 2022", "Q1 2022", "2022", "week of June 19,
/// 2022", "6 AM, Mar. 11, 2022".
fn period_name(value: &str, grid: &str) -> Option<String> {
    use chrono::{NaiveDate, Weekday};
    let day = |d: NaiveDate| fmt::date_inline(d, None);
    Some(match grid {
        "year" => value.to_owned(),
        "bin" => {
            let (a, b) = value.split_once('–').or_else(|| value.split_once('-'))?;
            format!("{}–{}", a.trim(), b.trim())
        }
        "quarter" => {
            let (y, q) = value.split_once("-Q")?;
            format!("Q{q} {y}")
        }
        "month" => {
            let (y, m) = value.split_once('-')?;
            fmt::month_year(y.parse().ok()?, m.parse().ok()?).ok()?
        }
        "week" => {
            let start = if let Some((y, w)) = value.split_once("-W") {
                NaiveDate::from_isoywd_opt(y.parse().ok()?, w.parse().ok()?, Weekday::Mon)?
            } else {
                NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?
            };
            format!("week of {}", day(start))
        }
        "hour" => {
            let (d, h) = value.split_once('T')?;
            let d = NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
            let h: u32 = h.get(..2)?.parse().ok()?;
            let twelve = if h % 12 == 0 { 12 } else { h % 12 };
            format!("{twelve} {}, {}", if h < 12 { "AM" } else { "PM" }, day(d))
        }
        "day" => {
            let d = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
            day(d)
        }
        _ => return None,
    })
}
