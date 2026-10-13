//! Physical orientation is resolved from measured labels when the author asks for auto.
use chartgen_model::*;

pub(crate) fn horizontal_if_needed(
    plan: &SemanticPlan,
    data: &PreparedData,
    theme: &Theme,
    constraints: &LayoutConstraints,
    measurements: &Measurements,
) -> Result<bool> {
    if plan.orientation != Orientation::Vertical
        || data.x_kind == "time"
        || plan.spec.string("/chart/orientation", "auto") != "auto"
        || !matches!(
            plan.chart_type,
            ChartType::Bar | ChartType::Dot | ChartType::StackedBar | ChartType::StackedBar100
        )
    {
        return Ok(false);
    }
    let n = data.categories.len();
    if n > 12 {
        return Ok(true);
    }
    if n == 0 {
        return Ok(false);
    }
    // The vertical solver reserves a 52pt scale column and a 16pt right margin.
    let pitch = (constraints.width - 68.) / n as f64;
    for i in 0..n {
        let content = crate::guides::category_content(plan, data, i)?;
        let request = crate::guides::request(
            "category-label",
            content,
            theme.font_size,
            None,
            theme,
            constraints,
        )?;
        if let Some(metrics) = measurements.entries.get(&request.id) {
            if metrics.width + 4. > pitch {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
