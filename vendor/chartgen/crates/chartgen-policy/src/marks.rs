//! The refusals the original makes as a type's layout begins, before any geometry: a highlight
//! method a family never draws (R15-6, R6-28, R7-38), a legend on a pie (R7-34), a table's
//! rejected versions (R10-16, R10-22), a step chart without its end (R6-8), an arrow chart
//! without its % panel (R6-17), and a colour encoding a quantity cannot take (R3-13, R3-9).
//!
//! They depend only on the spec and the chosen type, so the policy makes them; the layouts
//! receive only charts that pass them (an override lets one through, reported here once).
use crate::Choice;
use crate::rules::{decide, guided, info, refusal};
use crate::view::View;
use chartgen_model::Report;
use serde_json::Value;

/// `layout/bars.targets`: the method of the last highlight entry, else auto.
fn bar_method(v: &View) -> String {
    v.get("/highlight")
        .as_array()
        .and_then(|hl| hl.last())
        .map(|h| h["method"].as_str().unwrap_or("auto").to_owned())
        .unwrap_or_else(|| "auto".into())
}
fn any_method(v: &View, method: &str) -> bool {
    v.get("/highlight")
        .as_array()
        .is_some_and(|hl| hl.iter().any(|h| h["method"] == method))
}

/// Returns true when the chart must not be drawn.
pub(crate) fn layout_refusals(v: &View, choice: &Choice, report: &mut Report) -> bool {
    let t = choice.type_.as_str();
    let mut stop = false;
    match t {
        "bar" | "dot" | "stacked_bar" | "stacked_bar_100" | "waterfall" | "funnel_bars" => {
            let what = match t {
                "dot" => "dot",
                "stacked_bar" | "stacked_bar_100" => "stacked bar",
                _ => "bar",
            };
            let method = bar_method(v);
            // layout/bars.layout_bars decides the bars' order before the highlight's method
            let ordered = t == "bar" || t == "dot";
            let order = |report: &mut Report| {
                if ordered {
                    order_decisions(v, choice, report)
                }
            };
            if method == "thickness" {
                order(report);
                return refusal(
                    report,
                    v,
                    "R15-6",
                    format!(
                        "a highlighted {what} may not be bigger: {what}s are highlighted by colour"
                    ),
                    Some("highlight[].method"),
                    Some("method auto or strong"),
                );
            }
            if method == "grey_others" {
                order(report);
                return refusal(
                    report,
                    v,
                    "R6-28",
                    "greying the other marks is the book's rejected version; others keep their colour",
                    Some("highlight[].method"),
                    Some("method auto, with dim_others for bars"),
                );
            }
        }
        "line" => {
            if bar_method(v) == "grey_others" {
                return refusal(
                    report,
                    v,
                    "R6-28",
                    "greying the other lines is the book's rejected version: where lines cross, grey lines cannot be told apart",
                    Some("highlight[].method"),
                    Some("method auto: the target thicker, in the strong shade of its own hue"),
                );
            }
        }
        "pie" | "donut" => {
            if any_method(v, "explode") {
                stop |= refusal(
                    report,
                    v,
                    "R7-38",
                    "an exploded slice is harder to compare with the others and with the whole",
                    Some("highlight[].method"),
                    Some("method auto (the strong shade of its colour), or border"),
                );
            }
            if v.str_or("/chart/labelling", "auto") == "legend" {
                stop |= refusal(
                    report,
                    v,
                    "R7-34",
                    "a legend sends the eye back and forth between keys and slices; each slice is named beside it",
                    Some("chart.labelling"),
                    Some("labelling auto"),
                );
            }
            if any_method(v, "thickness") {
                stop |= refusal(
                    report,
                    v,
                    "R15-6",
                    "a highlighted slice may not be bigger: it is highlighted by colour or a border",
                    Some("highlight[].method"),
                    Some("method auto or border"),
                );
            }
            if any_method(v, "grey_others") {
                stop |= refusal(
                    report,
                    v,
                    "R6-28",
                    "greying the other slices is the rejected version; they keep their colours",
                    Some("highlight[].method"),
                    Some("method auto, with dim_others"),
                );
            }
        }
        "treemap" => {
            if any_method(v, "thickness") {
                stop |= refusal(
                    report,
                    v,
                    "R15-6",
                    "a highlighted tile may not be bigger: it is highlighted by colour or a border",
                    Some("highlight[].method"),
                    Some("method auto or border"),
                );
            }
        }
        "wrapped_bar" => {
            let regular = v.series().iter().filter(|s| s.role == "regular").count();
            if regular != 1 {
                info(
                    report,
                    "CG-3",
                    "a wrapped bar chart of several series is not drawn: one series, cut into columns",
                    Some("data.series"),
                );
                return true;
            }
            if any_method(v, "thickness") {
                return refusal(
                    report,
                    v,
                    "R15-6",
                    "a highlighted bar may not be wider: bars are highlighted by colour",
                    Some("highlight[].method"),
                    Some("method auto or border"),
                );
            }
        }
        "table" => {
            if v.get("/table/stripes") == &Value::Bool(true)
                && refusal(
                    report,
                    v,
                    "R10-16",
                    "zebra stripes add clutter, and a shaded row looks highlighted; rows set well apart are scanned without them",
                    Some("table.stripes"),
                    Some("no stripes"),
                )
            {
                return true;
            }
            let bold: Vec<&str> = v
                .get("/table/bold")
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if !bold.is_empty()
                && refusal(
                    report,
                    v,
                    "R10-22",
                    format!(
                        "bold {}: their place, spacing and rules already set them apart; bold is kept for what matters",
                        bold.join(", ")
                    ),
                    Some("table.bold"),
                    Some("no bold"),
                )
            {
                return true;
            }
        }
        "step" => {
            // layout/step.domain: without a window (x.values) or data.as_of, a step chart cannot
            // know where its last value ends
            let window = v
                .get("/data/x/values")
                .as_array()
                .is_some_and(|a| !a.is_empty());
            if !window && v.opt_str("/data/as_of").is_none() {
                refusal(
                    report,
                    v,
                    "R6-8",
                    "a step chart runs its last value to the date the series is known to (G7.19, D-42), which cannot be read from the changes",
                    Some("data.as_of"),
                    Some("data.as_of, the last day the series covers"),
                );
                return true;
            }
        }
        "arrow" => {
            // layout/arrowchart.panels: the absolute panel alone only through an override
            if v.str_or("/chart/change_panels", "auto") == "absolute"
                && refusal(
                    report,
                    v,
                    "R6-17",
                    "absolute changes without the relative (%) changes: most readers then name the wrong category as the most improved (G1.47)",
                    Some("chart.change_panels"),
                    Some("change_panels auto (both panels)"),
                )
            {
                return true;
            }
        }
        "heatmap" => {
            let asked = v.str_or("/chart/encoding", "auto");
            let dividing = v.measure().get("dividing_point").filter(|d| !d.is_null());
            if asked == "diverging" && dividing.is_none() {
                if refusal(
                    report,
                    v,
                    "R3-13",
                    "a diverging palette needs the dividing value the audience sees (zero, a target, an expected value); it cannot be read from the data, and its middle is not one (D-37 (a))",
                    Some("chart.encoding"),
                    Some("data.measure.dividing_point, or encoding auto (sequential)"),
                ) {
                    return true;
                }
            } else if asked != "auto" && asked != "categorical" {
                // ramps.choose: a departure from the procedure's palette, guided
                let d = dividing.and_then(crate::view::dec);
                let rec = if d.is_some() {
                    "diverging"
                } else {
                    "sequential"
                };
                let why = match d {
                    Some(d) => format!(
                        "the audience sees a dividing value, {}: two ramps meeting at a neutral there (G4.11, G4.12)",
                        chartgen_scales::plain(d)
                    ),
                    None => "no dividing value the audience sees: one ramp, darker meaning more, never diverging about the middle of the data (G4.10, D-37 (a))".into(),
                };
                guided(
                    report,
                    v,
                    if rec == "diverging" { "R3-13" } else { "R3-12" },
                    "chart.encoding",
                    asked,
                    rec,
                    &why,
                );
            } else if asked == "categorical"
                && refusal(
                    report,
                    v,
                    "R3-9",
                    "a categorical palette for a quantity: its hues have no order, so the reader cannot tell which colour means more (darker must mean more, G4.14; the book's categorical palette on ordered levels, s. 0112)",
                    Some("chart.encoding"),
                    Some("encoding auto"),
                )
            {
                return true;
            }
        }
        _ => {}
    }
    stop
}

/// `layout/bars.order`'s decisions: the categories in their given order, or sorted by value with
/// the catch-all, missing and total categories and the values not known last (R9-9).
fn order_decisions(v: &View, choice: &Choice, report: &mut Report) {
    let kind = v.x_kind();
    if choice.sort == "given" {
        decide(
            report,
            if kind != "nominal" { "R2-02" } else { "02 D1" },
            format!(
                "{} in their given order",
                if kind == "time" {
                    "periods".to_owned()
                } else {
                    format!("{kind} categories")
                }
            ),
            None,
        );
        return;
    }
    let (_, rest) = crate::sorted_order(v.spec, v.data, &choice.sort, &choice.sort_by);
    decide(
        report,
        "02 D1",
        format!(
            "nominal categories sorted by value, {}{}",
            if choice.sort == "desc" {
                "descending"
            } else {
                "ascending"
            },
            if v.series().len() > 1 {
                format!(" (by series {})", choice.sort_by)
            } else {
                String::new()
            }
        ),
        None,
    );
    if !rest.is_empty() {
        let names: Vec<&str> = rest
            .iter()
            .map(|&i| v.data.categories[i].as_str())
            .collect();
        decide(
            report,
            "R9-9",
            format!(
                "last: {} (catch-all, missing, total, then values not known)",
                names.join(", ")
            ),
            None,
        );
    }
}
