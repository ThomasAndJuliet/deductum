//! Local validation against the original generated ChartSpec schema.
//! Deliberately no network resolution: references must be local `#/$defs/…`.
use chartgen_model::{Finding, Spec};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::OnceLock;

static SCHEMA: OnceLock<Value> = OnceLock::new();
fn bundled_schema() -> &'static Value {
    SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../schema.json")).expect("bundled schema")
    })
}
pub(crate) fn is_statistical(spec: &Spec) -> bool {
    statistical_type(spec).is_some()
}
/// The statistical chart type of a spec: the one it names, or (under `auto`) the one its series
/// imply by their samples, xy pairs, counts or states.
pub(crate) fn statistical_type(spec: &Spec) -> Option<String> {
    let named = spec.string("/chart/type", "auto");
    if chartgen_statistics::is_statistical(&named) {
        return Some(named);
    }
    if named == "auto" {
        return chartgen_statistics::infer_type(&spec.raw).map(|(t, _)| t.to_string());
    }
    None
}
fn at(path: &str, msg: impl Into<String>) -> Finding {
    Finding::new("CG-4", "error", msg).at(path)
}
fn child(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}
fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}
fn type_ok(value: &Value, name: &str) -> bool {
    match name {
        "null" => value.is_null(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        _ => false,
    }
}
fn validate_at(value: &Value, rule: &Value, path: &str, out: &mut Vec<Finding>, depth: usize) {
    if depth > 80 {
        out.push(at(path, "schema reference depth exceeded"));
        return;
    }
    if let Some(reference) = rule.get("$ref").and_then(Value::as_str) {
        let target = reference
            .strip_prefix("#/")
            .and_then(|ptr| bundled_schema().pointer(&format!("/{ptr}")));
        match target {
            Some(target) => validate_at(value, target, path, out, depth + 1),
            None => out.push(at(
                path,
                format!("unknown local schema reference {reference}"),
            )),
        }
        return;
    }
    if let Some(alternatives) = rule.get("oneOf").and_then(Value::as_array) {
        let mut matches = 0;
        let mut closest: Option<Vec<Finding>> = None;
        for candidate in alternatives {
            let mut findings = Vec::new();
            validate_at(value, candidate, path, &mut findings, depth + 1);
            if findings.is_empty() {
                matches += 1;
            }
            if closest.as_ref().is_none_or(|x| findings.len() < x.len()) {
                closest = Some(findings);
            }
        }
        if matches != 1 {
            let detail = if matches == 0 {
                "matches none of the allowed forms"
            } else {
                "matches more than one allowed form"
            };
            out.push(at(path, detail));
            if matches == 0 {
                if let Some(mut details) = closest {
                    if details.len() <= 2 {
                        out.append(&mut details);
                    }
                }
            }
        }
        return;
    }
    if let Some(alternatives) = rule.get("anyOf").and_then(Value::as_array) {
        if !value.is_null()
            && alternatives.len() == 2
            && alternatives[1].get("type") == Some(&Value::String("null".into()))
        {
            validate_at(value, &alternatives[0], path, out, depth + 1);
            return;
        }
        if !alternatives.iter().any(|candidate| {
            let mut findings = Vec::new();
            validate_at(value, candidate, path, &mut findings, depth + 1);
            findings.is_empty()
        }) {
            out.push(at(path, "matches none of the allowed forms"));
        }
        return;
    }
    if let Some(name) = rule.get("type").and_then(Value::as_str) {
        if !type_ok(value, name) {
            out.push(at(path, format!("expected {name}")));
            return;
        }
    } else if let Some(type_rule) = rule.get("type") {
        if type_rule.is_object() {
            validate_at(value, type_rule, path, out, depth + 1);
        }
    }
    if let Some(constant) = rule.get("const") {
        if value != constant {
            out.push(at(path, format!("expected {constant}")));
        }
    }
    if let Some(allowed) = rule.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            out.push(at(
                path,
                format!("{value} is not one of the allowed values"),
            ));
        }
    }
    if let (Some(text), Some(pattern)) =
        (value.as_str(), rule.get("pattern").and_then(Value::as_str))
    {
        match Regex::new(pattern) {
            Ok(regex) if !regex.is_match(text) => {
                out.push(at(path, format!("does not match {pattern}")))
            }
            Err(_) => out.push(at(path, "invalid bundled schema pattern")),
            _ => {}
        }
    }
    if let Some(obj) = value.as_object() {
        let props = rule.get("properties").and_then(Value::as_object);
        if let Some(required) = rule.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !obj.contains_key(key) {
                    out.push(at(&child(path, key), "required field is missing"));
                }
            }
        }
        for (key, item) in obj {
            let field_path = child(path, key);
            if let Some(property) = props.and_then(|p| p.get(key)) {
                validate_at(item, property, &field_path, out, depth + 1);
            } else if let Some(additional) = rule.get("additionalProperties") {
                if additional == false {
                    out.push(at(&field_path, "unknown field"));
                } else if additional.is_object() {
                    validate_at(item, additional, &field_path, out, depth + 1);
                }
            }
        }
    }
    if let Some(items) = value.as_array() {
        if let Some(item_rule) = rule.get("items") {
            for (i, item) in items.iter().enumerate() {
                validate_at(item, item_rule, &index(path, i), out, depth + 1);
            }
        }
    }
}

pub fn validate(spec: &Spec) -> Vec<Finding> {
    let mut findings = Vec::new();
    validate_at(&spec.raw, bundled_schema(), "", &mut findings, 0);
    if is_statistical(spec) {
        // a scatter plot or a table of counts has no categories: data.x is not required of them
        findings.retain(|f| !(f.path.as_deref() == Some("data.x") && f.message == "required field is missing"));
    }
    if let Some(id) = spec.get("/id").as_str() {
        if !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            || id.is_empty()
        {
            findings.push(at("id", "must match [a-z0-9-]+"));
        }
    }
    if findings.is_empty() {
        validate_structure(spec, &mut findings);
    }
    findings
}
pub fn schema() -> Value {
    bundled_schema().clone()
}
fn date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}
fn instant(text: &str) -> Option<NaiveDateTime> {
    let (day, time) = text
        .split_once('T')
        .map_or((text, None), |(d, t)| (d, Some(t)));
    let day = date(day)?;
    let time = match time {
        None => NaiveTime::from_hms_opt(0, 0, 0)?,
        Some(t) => ["%H", "%H:%M", "%H:%M:%S"]
            .iter()
            .find_map(|f| NaiveTime::parse_from_str(t, f).ok())?,
    };
    Some(day.and_time(time))
}
fn period_start(text: &str, grid: &str) -> Option<NaiveDate> {
    match grid {
        "year" => NaiveDate::from_ymd_opt(text.parse().ok()?, 1, 1),
        "quarter" => {
            let (y, q) = text.split_once("-Q")?;
            NaiveDate::from_ymd_opt(
                y.parse().ok()?,
                q.parse::<u32>().ok()?.checked_sub(1)? * 3 + 1,
                1,
            )
        }
        "month" => {
            let (y, m) = text.split_once('-')?;
            NaiveDate::from_ymd_opt(y.parse().ok()?, m.parse().ok()?, 1)
        }
        "week" => {
            if let Some((y, w)) = text.split_once("-W") {
                NaiveDate::from_isoywd_opt(y.parse().ok()?, w.parse().ok()?, Weekday::Mon)
            } else {
                date(text)
            }
        }
        "day" => date(text),
        "hour" => date(text.split_once('T')?.0),
        "bin" => NaiveDate::from_ymd_opt(text.split(['-', '–']).next()?.parse().ok()?, 1, 1),
        _ => None,
    }
}
fn check_ref(
    spec: &Spec,
    r: &Value,
    path: &str,
    ids: &[&str],
    names: &[&str],
    out: &mut Vec<Finding>,
) {
    if let Some(id) = r["series"].as_str() {
        if !ids.contains(&id) {
            out.push(at(path, format!("no series {id:?}")));
        }
    }
    let category = r["category"].as_str().or_else(|| r["x"].as_str());
    if let Some(cat) = category {
        let irregular = spec.get("/data/x/kind") == "time" && spec.get("/data/x/grid") == "none";
        let match_point = if irregular {
            spec.get("/data/series").as_array().is_some_and(|series| {
                series
                    .iter()
                    .filter(|s| r["series"].as_str().is_none_or(|id| s["id"] == id))
                    .any(|s| {
                        s["points"].as_array().is_some_and(|points| {
                            points.iter().any(|p| {
                                let text = p
                                    .as_array()
                                    .and_then(|a| a.first())
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                text == cat || (cat.len() == 10 && text.starts_with(cat))
                            })
                        })
                    })
            })
        } else {
            false
        };
        if !names.contains(&cat) && !match_point {
            out.push(at(path, format!("no category {cat:?}")));
        }
    }
}

fn validate_structure(spec: &Spec, out: &mut Vec<Finding>) {
    let x = spec.get("/data/x");
    let categories = x["values"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let names: Vec<&str> = categories.iter().filter_map(Value::as_str).collect();
    let n = names.len();
    if names.iter().copied().collect::<BTreeSet<_>>().len() != n {
        out.push(at("data.x.values", "category names must be distinct"));
    }
    let series = spec
        .get("/data/series")
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if series.is_empty() {
        out.push(at("data.series", "at least one series is needed"));
    }
    let ids: Vec<&str> = series.iter().filter_map(|s| s["id"].as_str()).collect();
    if ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len() {
        out.push(at("data.series", "series ids must be distinct"));
    }
    let irregular = x["grid"] == "none" && x["kind"] == "time";
    for (i, s) in series.iter().enumerate() {
        let base = format!("data.series[{i}]");
        if irregular {
            if s["points"].is_null() {
                out.push(at(
                    &base,
                    "irregular time (grid none) takes points, not values",
                ));
            } else if let Some(points) = s["points"].as_array() {
                if points.is_empty() {
                    out.push(at(
                        &format!("{base}.points"),
                        "at least one point is needed",
                    ));
                }
                let mut previous = None;
                for (j, p) in points.iter().enumerate() {
                    let path = format!("{base}.points[{j}]");
                    let Some(pair) = p.as_array().filter(|a| a.len() == 2) else {
                        out.push(at(&path, "a point is [time, value]"));
                        continue;
                    };
                    let Some(text) = pair[0].as_str() else {
                        out.push(at(&format!("{path}[0]"), "not a time string"));
                        continue;
                    };
                    if let Some(now) = instant(text) {
                        if previous.is_some_and(|old| now < old) {
                            out.push(at(&path, "points must be in time order"));
                        }
                        previous = Some(now);
                    } else {
                        out.push(at(
                            &format!("{path}[0]"),
                            "not a time (YYYY-MM-DD, or with THH, THH:MM, THH:MM:SS)",
                        ));
                    }
                    let mut value_findings = Vec::new();
                    validate_at(
                        &pair[1],
                        &bundled_schema()["$defs"]["Series"]["properties"]["values"]["items"],
                        &format!("{path}[1]"),
                        &mut value_findings,
                        0,
                    );
                    out.append(&mut value_findings);
                }
            }
        } else if is_statistical(spec) {
            // samples, xy pairs, counts or states: read and checked by chartgen-statistics
        } else if let Some(values) = s["values"].as_array() {
            if values.len() != n {
                out.push(at(
                    &format!("{base}.values"),
                    format!("{} values for {n} categories", values.len()),
                ));
            }
        } else {
            out.push(at(&format!("{base}.values"), "required"));
        }
        if let Some(slot) = s["slot"].as_i64() {
            if !(1..=16).contains(&slot) {
                out.push(at(&format!("{base}.slot"), "a palette slot, 1–16"));
            }
        }
    }
    if x["kind"] == "time" {
        if x["grid"].is_null() {
            out.push(at(
                "data.x.grid",
                "required for time: regular intervals, or none (R6-3)",
            ));
        } else if !irregular {
            let grid = x["grid"].as_str().unwrap_or("");
            let pattern = match grid {
                "year" => r"^\d{4}$",
                "quarter" => r"^\d{4}-Q[1-4]$",
                "month" => r"^\d{4}-(0[1-9]|1[0-2])$",
                "week" => r"^(\d{4}-W(0[1-9]|[1-4]\d|5[0-3])|\d{4}-\d{2}-\d{2})$",
                "day" => r"^\d{4}-\d{2}-\d{2}$",
                "hour" => r"^\d{4}-\d{2}-\d{2}T\d{2}$",
                "bin" => r"^\d{4}[–-]\d{4}$",
                _ => "",
            };
            if !pattern.is_empty() {
                let regex = Regex::new(pattern).expect("static period pattern");
                if let Some(bad) = names.iter().find(|v| !regex.is_match(v)) {
                    out.push(at(
                        "data.x.values",
                        format!("{bad:?} is not a {grid} period"),
                    ));
                } else if names.windows(2).any(|w| w[0] >= w[1]) {
                    out.push(at("data.x.values", "periods must be strictly increasing"));
                } else if names.iter().any(|v| period_start(v, grid).is_none()) {
                    out.push(at("data.x.values", format!("invalid {grid} period")));
                } else if grid == "week" {
                    let iso = names.iter().filter(|v| v.contains("-W")).count();
                    let region = spec
                        .string("/audience/locale", "en-GB")
                        .replace('_', "-")
                        .split('-')
                        .next_back()
                        .unwrap_or("")
                        .to_ascii_uppercase();
                    let sunday = match spec.string("/data/x/week_start", "auto").as_str() {
                        "sunday" => true,
                        "monday" => false,
                        _ => ["US", "CA", "MX", "BR", "JP", "KR", "TW", "IL", "PH"]
                            .contains(&region.as_str()),
                    };
                    // timeaxis.check_grid's words
                    let week_start = spec.string("/data/x/week_start", "auto");
                    let how = if week_start == "auto" {
                        format!(
                            "x.week_start, by audience.locale {}",
                            spec.string("/audience/locale", "en-GB")
                        )
                    } else {
                        "x.week_start".to_owned()
                    };
                    if iso > 0 && iso < n {
                        out.push(at(
                            "data.x.values",
                            "weeks are written one way: as ISO weeks (2022-W25) or by their first days (2022-06-19)",
                        ));
                    } else if iso > 0 && sunday {
                        let first = names
                            .iter()
                            .find(|v| v.contains("-W"))
                            .cloned()
                            .unwrap_or_default();
                        out.push(at(
                            "data.x.values",
                            format!("an ISO week ({first}) runs Monday to Sunday, and these weeks start on Sunday ({how}): give each week by its first day, 2022-06-19"),
                        ));
                    } else {
                        let starts: Vec<_> =
                            names.iter().filter_map(|v| period_start(v, grid)).collect();
                        let want = if sunday { Weekday::Sun } else { Weekday::Mon };
                        if iso == 0 && starts.iter().any(|d| d.weekday() != want) {
                            out.push(at("data.x.values", "week starts on the wrong weekday"));
                        }
                        if starts.windows(2).any(|w| w[1] - w[0] != Duration::days(7)) {
                            out.push(at("data.x.values", "weeks must follow one another"));
                        }
                    }
                } else if grid == "bin" {
                    let bins: Vec<(i32, i32)> = names
                        .iter()
                        .filter_map(|v| {
                            let (a, b) = v.split_once(['-', '–'])?;
                            Some((a.parse().ok()?, b.parse().ok()?))
                        })
                        .collect();
                    if bins.iter().any(|(a, b)| b < a) {
                        out.push(at("data.x.values", "a bin ends before it starts"));
                    } else if bins.windows(2).any(|w| w[1].0 != w[0].1 + 1) {
                        out.push(at(
                            "data.x.values",
                            "bins must follow one another without a gap or overlap",
                        ));
                    }
                }
            }
        }
    }
    for (pointer, path, known) in [
        ("/data/x/roles", "data.x.roles", &names),
        ("/data/x/groups", "data.x.groups", &names),
        ("/data/actions", "data.actions", &names),
        ("/data/parts/groups", "data.parts.groups", &ids),
    ] {
        if let Some(obj) = spec.get(pointer).as_object() {
            for key in obj.keys() {
                if !known.contains(&key.as_str()) {
                    out.push(at(&format!("{path}.{key}"), "unknown category or series"));
                }
            }
        }
    }
    if let Some(as_of) = spec.get("/data/as_of").as_str() {
        if date(as_of).is_none() {
            out.push(at("data.as_of", "not a date (YYYY-MM-DD)"));
        }
    }
    if let Some(stubs) = spec.get("/table/stub").as_array() {
        for (j, stub) in stubs.iter().enumerate() {
            // StubColumn.values defaults to an empty list (spec.py)
            let empty = vec![];
            if let Some(values) = stub["values"].as_array().or(Some(&empty)) {
                if values.len() != series.len() {
                    out.push(at(
                        &format!("table.stub[{j}].values"),
                        format!(
                            "{} entries for {} rows (series)",
                            values.len(),
                            series.len()
                        ),
                    ));
                }
                if stub["kind"] == "date" {
                    for (i, v) in values.iter().enumerate() {
                        if let Some(text) = v.as_str() {
                            if !text.is_empty() && date(text).is_none() {
                                out.push(at(
                                    &format!("table.stub[{j}].values[{i}]"),
                                    "not a date (YYYY-MM-DD)",
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(columns) = spec.get("/table/graphics_columns").as_array() {
        for c in columns.iter().filter_map(Value::as_str) {
            if !names.contains(&c) {
                out.push(at("table.graphics_columns", format!("no column {c:?}")));
            }
        }
    }
    if let Some(events) = spec.get("/data/x/events").as_array() {
        for (j, event) in events.iter().enumerate() {
            let path = format!("data.x.events[{j}].at");
            if x["kind"] != "time" {
                out.push(at(&path, "dates are marked on a time axis only"));
            } else if let Some(text) = event["at"].as_str() {
                if instant(text).is_none()
                    && period_start(text, x["grid"].as_str().unwrap_or("")).is_none()
                {
                    out.push(at(&path, "not a date or time"));
                }
            }
        }
    }
    if let Some(targets) = spec.get("/insight/targets").as_array() {
        for (i, r) in targets.iter().enumerate() {
            check_ref(spec, r, &format!("insight.targets[{i}]"), &ids, &names, out);
        }
    }
    if let Some(highlights) = spec.get("/highlight").as_array() {
        for (j, h) in highlights.iter().enumerate() {
            if let Some(targets) = h["targets"].as_array() {
                for (i, r) in targets.iter().enumerate() {
                    check_ref(
                        spec,
                        r,
                        &format!("highlight[{j}].targets[{i}]"),
                        &ids,
                        &names,
                        out,
                    );
                }
            }
        }
    }
    if let Some(comments) = spec.get("/comments").as_array() {
        for (j, c) in comments.iter().enumerate() {
            if let Some(anchors) = c["anchors"].as_array() {
                for (i, r) in anchors.iter().enumerate() {
                    check_ref(
                        spec,
                        r,
                        &format!("comments[{j}].anchors[{i}]"),
                        &ids,
                        &names,
                        out,
                    );
                }
            }
        }
    }
    if !spec.get("/title/link").is_null() {
        check_ref(
            spec,
            spec.get("/title/link"),
            "title.link",
            &ids,
            &names,
            out,
        );
    }
    if let Some(notes) = spec.get("/notes").as_array() {
        for (j, note) in notes.iter().enumerate() {
            let text = note["text"]
                .as_str()
                .or_else(|| {
                    ["typst", "tex", "latex", "alternative"]
                        .iter()
                        .find_map(|k| note["text"][*k].as_str())
                })
                .unwrap_or("");
            if text.trim().is_empty() {
                out.push(at(&format!("notes[{j}].text"), "a note needs its words"));
            }
            if let Some(on) = note["on"].as_array() {
                for (i, r) in on.iter().enumerate() {
                    let path = format!("notes[{j}].on[{i}]");
                    let kinds = ["element", "comment", "stub", "status"]
                        .iter()
                        .filter(|k| !r[**k].is_null())
                        .count();
                    let named = !r["series"].is_null() || !r["category"].is_null();
                    if kinds == 0 && !named {
                        out.push(at(&path,"where the mark goes: element, category, series, status, comment or stub"));
                    }
                    let element = !r["element"].is_null();
                    let comment = !r["comment"].is_null();
                    let stub = !r["stub"].is_null();
                    if kinds > 1
                        || ((element || comment) && named)
                        || (stub && !r["category"].is_null())
                    {
                        out.push(at(&path, "one place per entry: an element, a comment, a stub column with its series, a status with series or category, or a series and category"));
                    }
                    check_ref(spec, r, &path, &ids, &names, out);
                    if let Some(element) = r["element"].as_str() {
                        let present = match element {
                            "title" => !spec.get("/title/text").is_null(),
                            "descriptive" => !spec.get("/title/descriptive").is_null(),
                            "unit" => true,
                            "x_title" => !spec.get("/data/x/title").is_null(),
                            "names_title" => !spec.get("/table/names_title").is_null(),
                            _ => false,
                        };
                        if !present {
                            out.push(at(
                                &format!("{path}.element"),
                                format!("the spec has no {element}"),
                            ));
                        }
                    }
                    if let Some(c) = r["comment"].as_u64() {
                        if c >= spec.get("/comments").as_array().map_or(0, Vec::len) as u64 {
                            out.push(at(&format!("{path}.comment"), "no such comment"));
                        }
                    }
                    if let Some(stub) = r["stub"].as_u64() {
                        if stub >= spec.get("/table/stub").as_array().map_or(0, Vec::len) as u64 {
                            out.push(at(&format!("{path}.stub"), "no such stub column"));
                        }
                    }
                }
            }
        }
    }
    if let Some(id) = spec.get("/chart/sort_by").as_str() {
        // a paired contrast (dumbbell) sorts by the difference of its two series
        let by_difference = id == "difference" && ids.len() == 2;
        if !ids.contains(&id) && !by_difference {
            out.push(at("chart.sort_by", format!("no series {id:?}")));
        }
    }
    if let Some(slots) = spec.get("/colours/slots").as_object() {
        for (name, v) in slots {
            if !v.as_i64().is_some_and(|x| (1..=16).contains(&x)) {
                out.push(at(&format!("colours.slots.{name}"), "a palette slot, 1–16"));
            }
        }
    }
    for (path, min, max) in [
        ("/layout/width", 120.0, 600.0),
        ("/layout/plot_aspect", 0.3, 1.2),
    ] {
        if let Some(value) = spec.get(path).as_f64() {
            if value < min || value > max {
                out.push(at(
                    &path[1..].replace('/', "."),
                    format!("must be within {min}–{max}"),
                ));
            }
        }
    }
    let mut units = vec![(
        "data.measure.unit".to_owned(),
        spec.get("/data/measure/unit"),
    )];
    for (i, m) in spec
        .get("/data/measures")
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        units.push((format!("data.measures[{i}].unit"), &m["unit"]));
    }
    for (path, unit) in units {
        if let Some(mag) = unit["magnitude"].as_i64() {
            if ![0, 3, 6, 9, 12].contains(&mag) {
                out.push(at(&format!("{path}.magnitude"), "3, 6, 9 or 12"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_unknown_fields_at_all_levels() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"nominal","values":["A"],"oops":1},"series":[{"id":"s","values":[1]}]},"unknown":true}),
        };
        let findings = validate(&spec);
        assert!(
            findings
                .iter()
                .any(|f| f.path.as_deref() == Some("unknown"))
        );
        assert!(
            findings
                .iter()
                .any(|f| f.path.as_deref() == Some("data.x.oops"))
        );
    }
    #[test]
    fn catches_bad_nested_value_and_enum() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"banana","values":["A"]},"series":[{"id":"s","values":[{"certainty":"guess","value":1}]}]}}),
        };
        let findings = validate(&spec);
        assert!(
            findings
                .iter()
                .any(|f| f.path.as_deref() == Some("data.x.kind"))
        );
        assert!(findings.iter().any(|f| {
            f.path
                .as_deref()
                .is_some_and(|p| p.starts_with("data.series[0].values[0]"))
        }));
    }
    #[test]
    fn accepts_native_text_aliases_and_explicit_optional_null() {
        for key in ["tex", "latex", "typst"] {
            let mut title = serde_json::Map::new();
            title.insert(key.into(), json!("Native markup"));
            title.insert("alternative".into(), json!("Readable title"));
            let spec = Spec {
                raw: json!({"chartspec":1,"id":"test-1","title":{"text":title},"data":{"x":{"kind":"nominal","values":["A"],"title":null},"series":[{"id":"s","values":[1]}]}}),
            };
            assert!(validate(&spec).is_empty(), "{key}: {:?}", validate(&spec));
        }
    }
    #[test]
    fn checks_cross_field_lengths_and_references() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"nominal","values":["A","B"],"roles":{"C":"all_others"}},"series":[{"id":"s","values":[1]}]},"chart":{"sort_by":"other"},"layout":{"width":100}}),
        };
        let paths: Vec<_> = validate(&spec)
            .iter()
            .filter_map(|f| f.path.clone())
            .collect();
        for expected in [
            "data.series[0].values",
            "data.x.roles.C",
            "chart.sort_by",
            "layout.width",
        ] {
            assert!(paths.iter().any(|p| p == expected), "{expected}: {paths:?}");
        }
    }
    #[test]
    fn checks_calendar_and_note_targets() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"test","data":{"x":{"kind":"time","grid":"week","values":["2022-06-20","2022-06-27"],"week_start":"sunday"},"series":[{"id":"s","values":[1,2]}],"as_of":"2022-02-30"},"notes":[{"text":"A note","on":[{"series":"missing"}]}]}),
        };
        let paths: Vec<_> = validate(&spec)
            .iter()
            .filter_map(|f| f.path.clone())
            .collect();
        assert!(paths.contains(&"data.x.values".into()));
        assert!(paths.contains(&"data.as_of".into()));
        assert!(paths.contains(&"notes[0].on[0]".into()));
    }
}
