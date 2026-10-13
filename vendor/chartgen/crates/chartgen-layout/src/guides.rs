//! Host-measured guide text. Request IDs include the complete measurement context.
use chartgen_model::{
    Error, LayoutConstraints, PreparedData, Result, SemanticPlan, TextContent, TextRequest,
    TextStyle, Theme, TrainedScale,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn request(
    role: &str,
    content: TextContent,
    size: f64,
    max_width: Option<f64>,
    theme: &Theme,
    constraints: &LayoutConstraints,
) -> Result<TextRequest> {
    request_with_style(
        role,
        content,
        size,
        max_width,
        style_for(role, theme),
        theme,
        constraints,
    )
}

fn token(theme: &Theme, name: &str) -> Option<String> {
    theme
        .tokens
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn style_for(role: &str, theme: &Theme) -> TextStyle {
    let ink = match role {
        "comment" => token(theme, "--comment"),
        "missing-note" | "incomplete-note" | "chart-note" | "footnote" => token(theme, "--note"),
        "note-mark" => token(theme, "--note-mark-ink").or_else(|| token(theme, "--note")),
        "tile-label" => token(theme, "--tile-text"),
        _ => None,
    };
    TextStyle {
        fill: Some(ink.unwrap_or_else(|| theme.text.clone())),
        weight: if matches!(role, "title" | "panel-title" | "text-strong") {
            "bold"
        } else {
            "normal"
        }
        .into(),
        font_style: if matches!(
            role,
            "comment" | "missing-note" | "incomplete-note" | "chart-note" | "footnote"
        ) {
            "italic".into()
        } else if role == "note-mark" {
            token(theme, "--note-mark-slant").unwrap_or_else(|| "italic".into())
        } else {
            "normal".into()
        },
    }
}

pub fn request_with_style(
    role: &str,
    content: TextContent,
    size: f64,
    max_width: Option<f64>,
    style: TextStyle,
    theme: &Theme,
    constraints: &LayoutConstraints,
) -> Result<TextRequest> {
    if !size.is_finite() || size <= 0. || max_width.is_some_and(|w| !w.is_finite() || w <= 0.) {
        return Err(Error::new(
            "layout",
            "text size and maximum width must be positive and finite",
        ));
    }
    if theme.font_family.is_empty() || constraints.measurement_context.is_empty() {
        return Err(Error::new(
            "layout",
            "font family and measurement context are required",
        ));
    }
    if style.fill.as_deref().is_some_and(str::is_empty)
        || !matches!(style.weight.as_str(), "normal" | "bold")
        || !matches!(style.font_style.as_str(), "normal" | "italic")
    {
        return Err(Error::new(
            "layout",
            "invalid text fill, weight or font style",
        ));
    }
    if let TextContent::Native { host, key, .. } = &content {
        if host.is_empty() || key.is_empty() {
            return Err(Error::new("layout", "native text requires host and key"));
        }
    }
    let identity = serde_json::to_string(&(
        role,
        &content,
        &theme.font_family,
        size,
        &style,
        max_width,
        &constraints.measurement_context,
    ))
    .map_err(|e| Error::new("layout", e.to_string()))?;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in identity.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(TextRequest {
        id: format!("t-{hash:016x}"),
        content,
        role: role.into(),
        font_family: theme.font_family.clone(),
        font_size: size,
        style,
        max_width,
        context: constraints.measurement_context.clone(),
    })
}

pub fn content(value: &Value) -> Result<Option<TextContent>> {
    if let Some(s) = value.as_str() {
        return Ok(Some(TextContent::plain(s)));
    }
    if value.is_null() {
        return Ok(None);
    }
    if let Some(s) = value.get("plain").and_then(Value::as_str) {
        return Ok(Some(TextContent::plain(s)));
    }
    if let Some(s) = value.get("text").and_then(Value::as_str) {
        return Ok(Some(TextContent::plain(s)));
    }
    for host in ["typst", "tex", "latex"] {
        if let Some(key) = value.get(host).and_then(Value::as_str) {
            return Ok(Some(TextContent::Native {
                host: host.into(),
                key: key.into(),
                alternative: value
                    .get("alternative")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }));
        }
    }
    Err(Error::new(
        "layout",
        "unsupported text content; expected plain text or a native host reference",
    ))
}

pub fn category_content(
    plan: &SemanticPlan,
    data: &PreparedData,
    index: usize,
) -> Result<TextContent> {
    if data.irregular.is_some() {
        let name = &data.categories[index];
        if let Some(mapped) = plan.spec.raw["data"]["x"]["names"].get(name) {
            if let Some(content) = content(mapped)? {
                return Ok(content);
            }
        }
        return Ok(TextContent::plain(name));
    }
    let raw = &plan.spec.raw["data"]["x"]["values"][index];
    if let Some(name) = raw.as_str() {
        if let Some(mapped) = plan.spec.raw["data"]["x"]["names"].get(name) {
            if let Some(content) = content(mapped)? {
                return Ok(content);
            }
        }
    }
    Ok(content(raw)?.unwrap_or_else(|| TextContent::plain(&data.categories[index])))
}

pub fn series_content(
    plan: &SemanticPlan,
    data: &PreparedData,
    index: usize,
) -> Result<TextContent> {
    let raw = &plan.spec.raw["data"]["series"][index]["name"];
    Ok(content(raw)?.unwrap_or_else(|| TextContent::plain(&data.series[index].name)))
}
pub fn comparison_content(
    plan: &SemanticPlan,
    data: &PreparedData,
    index: usize,
) -> Result<TextContent> {
    let raw = &plan.spec.raw["data"]["comparisons"][index]["label"];
    Ok(content(raw)?.unwrap_or_else(|| TextContent::plain(&data.comparisons[index].label)))
}
pub fn missing_label(plan: &SemanticPlan, status: &str) -> Result<String> {
    let key = if status.is_empty() { "missing" } else { status };
    if let Some(custom) = plan
        .spec
        .raw
        .pointer("/labels/notes")
        .and_then(Value::as_object)
        .and_then(|notes| notes.get(key))
    {
        return custom
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::new("layout", format!("labels.notes.{key} must be plain text")));
    }
    Ok(format!("({key})"))
}

pub fn show_value_labels(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
) -> bool {
    if !matches!(
        plan.chart_type,
        chartgen_model::ChartType::Bar
            | chartgen_model::ChartType::ThinBar
            | chartgen_model::ChartType::Dot
    ) {
        return false;
    }
    let mode = plan
        .spec
        .raw
        .pointer("/chart/value_labels")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    if mode == "on" {
        return true;
    }
    if mode == "off" {
        return false;
    }
    let known = data
        .series
        .iter()
        .flat_map(|s| &s.values)
        .filter(|d| d.value.is_some())
        .count();
    scale.is_some_and(|s| known < s.ticks.len())
}

pub fn value_labels(plan: &SemanticPlan, data: &PreparedData) -> Result<Vec<Vec<Option<String>>>> {
    data.series
        .iter()
        .map(|s| {
            chartgen_format::format_values(
                &plan.spec,
                &s.values.iter().map(|d| d.value).collect::<Vec<_>>(),
            )
        })
        .collect()
}

pub fn plan(
    plan: &SemanticPlan,
    data: &PreparedData,
    scale: Option<&TrainedScale>,
    theme: &Theme,
    constraints: &LayoutConstraints,
) -> Result<Vec<TextRequest>> {
    let mut requests = BTreeMap::new();
    let mut add =
        |role: &str, content: TextContent, size: f64, max_width: Option<f64>| -> Result<()> {
            let request = request(role, content, size, max_width, theme, constraints)?;
            requests.entry(request.id.clone()).or_insert(request);
            Ok(())
        };
    let title = &plan.spec.raw["title"];
    if let Some(t) = content(if title.is_object() {
        &title["text"]
    } else {
        title
    })? {
        add(
            "title",
            t,
            theme.font_size * 1.2,
            Some(constraints.width - 24.),
        )?;
    }
    if let Some(t) = content(&title["descriptive"])? {
        add(
            "subtitle",
            t,
            theme.font_size,
            Some(constraints.width - 24.),
        )?;
    }
    for comment in crate::comments::contents(plan)? {
        add(
            "comment",
            comment,
            theme.font_size,
            Some(constraints.width - 24.),
        )?;
    }
    for i in 0..data.categories.len() {
        add(
            "category-label",
            category_content(plan, data, i)?,
            theme.font_size,
            None,
        )?;
    }
    if data.series.len() > 1 {
        for i in 0..data.series.len() {
            add(
                "series-label",
                series_content(plan, data, i)?,
                theme.font_size,
                None,
            )?;
        }
    }
    for (i, comparison) in data.comparisons.iter().enumerate() {
        if comparison.valid && comparison.render != "none" {
            add(
                "comparison-label",
                comparison_content(plan, data, i)?,
                theme.font_size * 0.9,
                None,
            )?;
        }
    }
    if matches!(
        plan.chart_type,
        chartgen_model::ChartType::Bar
            | chartgen_model::ChartType::ThinBar
            | chartgen_model::ChartType::Dot
            | chartgen_model::ChartType::Line
            | chartgen_model::ChartType::Step
    ) && data.irregular.is_none()
    {
        for d in data.series.iter().flat_map(|s| &s.values) {
            if d.value.is_none() {
                add(
                    "missing-note",
                    TextContent::plain(missing_label(plan, &d.status)?),
                    theme.font_size,
                    None,
                )?;
            }
        }
    }
    if let Some(scale) = scale {
        let formatted = chartgen_format::scale_labels(&plan.spec, scale)?;
        for label in &formatted.labels {
            add(
                "scale-label-formatted",
                TextContent::plain(label),
                theme.font_size * 0.9,
                None,
            )?;
        }
        for label in &formatted.bare {
            add(
                "scale-label-formatted",
                TextContent::plain(label),
                theme.font_size * 0.9,
                None,
            )?;
        }
        add(
            "scale-label-formatted",
            TextContent::plain(formatted.top.inline()),
            theme.font_size * 0.9,
            None,
        )?;
        if !formatted.unit_line.is_empty() {
            add(
                "unit-line",
                TextContent::plain(&formatted.unit_line),
                theme.font_size * 0.9,
                None,
            )?;
        }
        if !formatted.title.is_empty() {
            add(
                "scale-title",
                TextContent::plain(&formatted.title),
                theme.font_size * 0.9,
                None,
            )?;
        }
        for tick in &scale.ticks {
            add(
                "scale-label",
                TextContent::plain(tick.normalize().to_string()),
                theme.font_size * 0.9,
                None,
            )?;
        }
    }
    if show_value_labels(plan, data, scale) {
        for series in value_labels(plan, data)? {
            for label in series.into_iter().flatten() {
                add(
                    "value-label",
                    TextContent::plain(label),
                    theme.font_size * 0.9,
                    None,
                )?;
            }
        }
    }
    Ok(requests.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chartgen_model::{ChartType, ColorPlan, Orientation, Spec};
    use serde_json::json;
    #[test]
    fn native_x_name_and_context_are_preserved() {
        let p = SemanticPlan {
            spec: Spec {
                raw: json!({"data":{"x":{"values":["North"],"names":{"North":{"typst":"#smallcaps[North]","alternative":"North"}}}}}),
            },
            chart_type: ChartType::Bar,
            orientation: Orientation::Vertical,
            order: vec![0],
            layers: vec![],
            coordinates: "cartesian".into(),
            colors: ColorPlan::default(),
        };
        let d = PreparedData {
            categories: vec!["North".into()],
            x_kind: "nominal".into(),
            series: vec![],
            irregular: None,
            comparisons: vec![],
            measure: json!({}),
        };
        let c = category_content(&p, &d, 0).unwrap();
        assert!(
            matches!(c,TextContent::Native{host,key,..} if host=="typst"&&key=="#smallcaps[North]")
        );
        let t = Theme::default();
        let a = LayoutConstraints::default();
        let mut b = a.clone();
        b.measurement_context = "other".into();
        assert_ne!(
            request(
                "category-label",
                TextContent::plain("North"),
                10.,
                None,
                &t,
                &a
            )
            .unwrap()
            .id,
            request(
                "category-label",
                TextContent::plain("North"),
                10.,
                None,
                &t,
                &b
            )
            .unwrap()
            .id
        );
    }
    #[test]
    fn measured_identity_and_role_ink_include_font_style() {
        let mut theme = Theme::default();
        theme.tokens.insert("--comment".into(), json!("#3f63b0"));
        theme.tokens.insert("--note".into(), json!("#767676"));
        let constraints = LayoutConstraints::default();
        let content = TextContent::plain("Same words");
        let normal = request(
            "category-label",
            content.clone(),
            10.,
            None,
            &theme,
            &constraints,
        )
        .unwrap();
        let italic = request_with_style(
            "category-label",
            content.clone(),
            10.,
            None,
            TextStyle {
                font_style: "italic".into(),
                ..normal.style.clone()
            },
            &theme,
            &constraints,
        )
        .unwrap();
        let bold = request_with_style(
            "category-label",
            content,
            10.,
            None,
            TextStyle {
                weight: "bold".into(),
                ..normal.style.clone()
            },
            &theme,
            &constraints,
        )
        .unwrap();
        assert_ne!(normal.id, italic.id);
        assert_ne!(normal.id, bold.id);
        assert_ne!(italic.id, bold.id);
        let comment = request(
            "comment",
            TextContent::plain("x"),
            10.,
            None,
            &theme,
            &constraints,
        )
        .unwrap();
        assert_eq!(comment.style.fill.as_deref(), Some("#3f63b0"));
        assert_eq!(comment.style.font_style, "italic");
        let footnote = request(
            "footnote",
            TextContent::plain("x"),
            10.,
            None,
            &theme,
            &constraints,
        )
        .unwrap();
        assert_eq!(footnote.style.fill.as_deref(), Some("#767676"));
        assert_eq!(footnote.style.font_style, "italic");
    }
}
