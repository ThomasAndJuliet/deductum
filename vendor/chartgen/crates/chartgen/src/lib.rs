//! Chartgen's high-level façade over independently usable computational layers.
//! Native and WebAssembly hosts service text measurement requests explicitly.
pub use chartgen_colours as colours;
pub use chartgen_data as data;
pub use chartgen_grammar as grammar;
pub use chartgen_layout as layout;
pub use chartgen_lint as lint;
pub use chartgen_model as model;
pub use chartgen_note_layout as note_layout;
pub use chartgen_notes as notes;
pub use chartgen_panel_layout as panel_layout;
pub use chartgen_panels as panels;
pub use chartgen_policy as policy;
pub use chartgen_render as render;
pub use chartgen_scales as scales;
pub use chartgen_scene as scene;
pub use chartgen_svg_lint as svg_lint;
use model::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderRequest {
    pub spec: Value,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub constraints: LayoutConstraints,
    #[serde(default)]
    pub measurements: Measurements,
    #[serde(default = "svg_backend")]
    pub backend: String,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub expect: Value,
    /// The specification's file as the host names it, for the compatibility SVG's header comment.
    #[serde(default)]
    pub source_name: Option<String>,
}
fn svg_backend() -> String {
    "svg".into()
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RenderStep {
    NeedMeasurements {
        requests: Vec<TextRequest>,
        round: u32,
        report: Report,
    },
    Complete {
        output: String,
        scene: Scene,
        report: Report,
    },
    Stopped {
        report: Report,
    },
}
impl<'de> Deserialize<'de> for RenderStep {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        fn field<T: serde::de::DeserializeOwned>(
            value: &Value,
            key: &str,
        ) -> std::result::Result<T, String> {
            serde_json::from_value(
                value
                    .get(key)
                    .cloned()
                    .ok_or_else(|| format!("missing {key}"))?,
            )
            .map_err(|e| e.to_string())
        }
        let decode = || -> std::result::Result<Self, String> {
            Ok(match value["status"].as_str() {
                Some("need_measurements") => Self::NeedMeasurements {
                    requests: field(&value, "requests")?,
                    round: field(&value, "round")?,
                    report: field(&value, "report")?,
                },
                Some("complete") => Self::Complete {
                    output: field(&value, "output")?,
                    scene: field(&value, "scene")?,
                    report: field(&value, "report")?,
                },
                Some("stopped") => Self::Stopped {
                    report: field(&value, "report")?,
                },
                _ => return Err("unknown render status".into()),
            })
        };
        decode().map_err(serde::de::Error::custom)
    }
}

/// Perform one pure render step. Measurement ownership remains with the caller.
pub fn render_step(request: &RenderRequest) -> Result<RenderStep> {
    render_with_layout(request, &layout::DefaultLayout)
}

/// Use custom physical layout while retaining the same data, policy and backend layers.
pub fn render_with_layout(
    request: &RenderRequest,
    engine: &impl layout::LayoutEngine,
) -> Result<RenderStep> {
    let mut spec = Spec {
        raw: request.spec.clone(),
    };
    let mut report = Report::new(spec.id(), &request.theme.name);
    report.variant = request.variant.clone();
    report.expect = if request.expect.is_null() {
        json!({})
    } else {
        request.expect.clone()
    };
    report.source = spec.get("/source").clone();
    let Some(prepared) = prepare(&mut spec, &mut report, true) else {
        check_expect(&mut report);
        return Ok(RenderStep::Stopped { report });
    };
    let Prepared {
        source_data,
        data: prepared,
        bins,
        panels,
    } = prepared;
    let notes = notes::prepare(&spec, &source_data)?;
    let (choice, decision) =
        match policy::evaluate(&spec, &source_data, &prepared, bins, panels, &mut report) {
            policy::Outcome::Stopped => {
                check_expect(&mut report);
                return Ok(RenderStep::Stopped { report });
            }
            policy::Outcome::Panels => {
                let facts = data::inspect(&prepared);
                let decision = policy::decide(&spec, &prepared, &facts)?;
                report.chart_type = policy::report_type(&spec, &decision);
                if decision.status != "drawn" {
                    report.status = decision.status.clone();
                    report.findings.extend(decision.findings.iter().cloned());
                    check_expect(&mut report);
                    return Ok(RenderStep::Stopped { report });
                }
                (None, decision)
            }
            policy::Outcome::Drawn(choice, decision) => (Some(choice), decision),
        };
    let prepared = data::resolve_comparisons(
        &spec,
        decision
            .chart_type
            .expect("drawn decision has a chart type"),
        &prepared,
    )?;
    let mut plan = grammar::compile(&spec, &decision, &prepared)?;
    let mut colour_decisions = vec![];
    let mut neutral: Vec<(String, usize)> = vec![];
    let panel_charts = if choice.is_none() {
        Some(compile_panels_deferring(
            &spec,
            &prepared,
            &decision,
            &mut report,
            &mut colour_decisions,
        )?)
    } else {
        None
    };
    if report.status == "refused" {
        if choice.is_none() {
            // several panels refused while being prepared: the original names no type
            report.chart_type = None;
        }
        check_expect(&mut report);
        return Ok(RenderStep::Stopped { report });
    }
    let explained = match &choice {
        Some(choice) => match scale_step(&spec, &plan, &prepared, choice, &mut report, true)? {
            ScaleStep::Refused => {
                report.status = "refused".into();
                check_expect(&mut report);
                return Ok(RenderStep::Stopped { report });
            }
            ScaleStep::Trained(scale) => scale,
        },
        None => None,
    };
    if let Some(choice) = &choice
        && policy::layout_refusals(&spec, &prepared, choice, &mut report)
    {
        report.status = "refused".into();
        check_expect(&mut report);
        return Ok(RenderStep::Stopped { report });
    }
    if panel_charts.is_none() {
        // palette slots fixed by name lead the decisions (`colour_slots`); the colours' own
        // decisions are the original layouts', recorded with the layout's when it completes
        let assigned = colours::compile(&mut plan, &prepared)?;
        neutral = assigned.neutral.clone();
        colour_decisions = add_colours(&mut report, assigned);
        if report.status == "refused" {
            check_expect(&mut report);
            return Ok(RenderStep::Stopped { report });
        }
    }
    let scale = explained.as_ref().map(|x| x.scale.clone());
    let panel_engine = panel_charts.map(|panels| panel_layout::PanelLayout { engine, panels });
    let step = if let Some(panel_engine) = &panel_engine {
        note_layout::solve(
            panel_engine,
            &plan,
            &prepared,
            &notes,
            None,
            &request.theme,
            &request.constraints,
            &request.measurements,
        )?
    } else {
        note_layout::solve(
            engine,
            &plan,
            &prepared,
            &notes,
            scale.as_ref(),
            &request.theme,
            &request.constraints,
            &request.measurements,
        )?
    };
    match step {
        LayoutStep::NeedMeasurements { requests, round } => Ok(RenderStep::NeedMeasurements {
            requests,
            round,
            report,
        }),
        LayoutStep::Refused { findings } => {
            report.findings.extend(findings);
            report.status = "refused".into();
            check_expect(&mut report);
            Ok(RenderStep::Stopped { report })
        }
        LayoutStep::Complete { mut chart } => {
            report.findings.extend(chart.findings.iter().cloned());
            // the layout's decisions follow the policy's and the scale's, as in the original
            // The colour plan's decisions are the original layouts' (pie.colours, treemap's
            // groups, ramps.classify): a layout marks where it made them with a
            // `{"colours": "here"}` entry; without one they come first.
            let mut colours_at = None;
            for d in &chart.decisions {
                if d.get("colours").and_then(Value::as_str) == Some("here") {
                    colours_at = Some(report.decisions.len());
                    report
                        .decisions
                        .extend(std::mem::take(&mut colour_decisions));
                } else {
                    report.decisions.push(d.clone());
                }
            }
            if colours_at.is_none() {
                let at = report.decisions.len() - chart.decisions.len();
                report.decisions.splice(at..at, colour_decisions);
            }
            // A layout's own refusals (the original layouts' `refusal`, R0-04): an overridden
            // one is recorded with its reason; a refused chart has no marks and an error.
            for f in chart.findings.iter().filter(|f| f.severity == "overridden") {
                report
                    .overrides
                    .push(json!({"rule": f.rule, "reason": f.reason}));
            }
            if chart.primitives.is_empty() && chart.findings.iter().any(|f| f.severity == "error") {
                report.status = "refused".into();
                check_expect(&mut report);
                return Ok(RenderStep::Stopped { report });
            }
            report.summary.extend(chart.summary.clone());
            report
                .summary
                .insert("width".into(), json!(round2(chart.width)));
            report
                .summary
                .insert("height".into(), json!(round2(chart.height)));
            if let Some(scale) = &scale
                && !choice
                    .as_ref()
                    .is_some_and(|c| NO_SCALE.contains(&c.type_.as_str()))
            {
                scale_summary(&spec, &prepared, scale, &mut report);
            }
            // good and bad in the output (annotate.cvd; D-18): the style's mode, black and white
            let mode = request
                .theme
                .tokens
                .get("--good-bad-mode")
                .and_then(Value::as_str);
            let mut gb = colours::good_bad_output(&plan.colors, &spec, mode);
            report.findings.extend(gb.findings.drain(..));
            // black and white (audience.bw_risk): good and bad told apart without colour too
            // (annotate.textures), before the account of the output (annotate.cvd)
            if gb.safe {
                let slot3 = request
                    .theme
                    .tokens
                    .get("--series-3")
                    .and_then(Value::as_str);
                colours::safe_pair_neutrals(&mut chart.primitives, &neutral, slot3);
            }
            let mut texture_defs = vec![];
            if gb.textured {
                let arrows = colours::has_good_bad_arrows(&chart.primitives);
                let t = colours::textures::apply(
                    &mut chart.primitives,
                    &request.theme,
                    gb.safe,
                    plan.chart_type == ChartType::Dot,
                );
                let how = colours::textures::bw_how(&t, arrows);
                report.findings.extend(t.findings.iter().cloned());
                report.decisions.extend(t.decisions.iter().cloned());
                for d in gb.decisions.iter_mut() {
                    if let Some(text) = d["text"]
                        .as_str()
                        .filter(|x| x.starts_with("black and white"))
                    {
                        d["text"] = json!(format!("{text}: {how}"));
                    }
                }
                texture_defs = t.defs;
            }
            // annotate.cvd decides before the notes under the chart are set (footnotes.apply)
            let notes_at = report
                .decisions
                .iter()
                .rposition(|d| d["rule"] == "CG-8")
                .filter(|&i| i + 1 == report.decisions.len())
                .unwrap_or(report.decisions.len());
            report
                .decisions
                .splice(notes_at..notes_at, gb.decisions.drain(..));
            let mut scene = scene::lower(&plan, &chart, &request.theme)?;
            scene.defs.extend(texture_defs);
            let found = report.findings.clone();
            report.findings.extend(lint::semantic(
                &spec,
                &prepared,
                explained.as_ref(),
                choice.as_ref().map(|c| c.type_.as_str()),
                &found,
            ));
            report.findings.extend(lint::scene(&scene));
            scene.comment = Some(source_comment(request, &report));
            if gb.safe {
                // the colour-blind-safe pair: the chart carries `cvd-safe` (annotate.cvd), which
                // the stylesheet paints; the red/green copy is the chart without it
                for p in scene.primitives.iter_mut() {
                    if let Primitive::Group { meta, .. } = p {
                        if meta.classes.first().map(String::as_str) == Some("chart")
                            && !meta.classes.iter().any(|c| c == "cvd-safe")
                        {
                            meta.classes.push("cvd-safe".into());
                        }
                    }
                }
            }
            let output = render_scene(&scene, &request.backend)?;
            if gb.copy {
                // the colour-blind-safe file is this one: its red/green version beside it (copies)
                let mut semantic = request.clone();
                semantic
                    .theme
                    .tokens
                    .insert("--good-bad-mode".into(), json!("semantic"));
                match render_with_layout(&semantic, engine)? {
                    RenderStep::Complete { output: copy, .. } if copy != output => {
                        report.copies.insert("red-green".into(), copy);
                    }
                    _ => report.findings.push(
                        Finding::new(
                            "CG-3",
                            "info",
                            "the red/green copy of a colour-blind-safe chart is not written: this chart's layout draws its good and bad marks in the same colours under either mode",
                        )
                        .at("audience.cvd"),
                    ),
                }
            }
            check_expect(&mut report);
            Ok(RenderStep::Complete {
                output,
                scene,
                report,
            })
        }
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// The data stages before the policy: validation, the R2-43 re-aggregation of unequal bins,
/// parsing and any derived measure. None when the spec is invalid (reported).
struct Prepared {
    source_data: PreparedData,
    data: PreparedData,
    bins: policy::BinStep,
    panels: bool,
}
fn prepare(spec: &mut Spec, report: &mut Report, table_alone: bool) -> Option<Prepared> {
    let invalid = data::validate(spec);
    if !invalid.is_empty() {
        report.status = "invalid".into();
        report.findings = invalid;
        return None;
    }
    // the original's chooser re-aggregates unequal bins in place before anything reads them
    let bins = match policy::rebin(spec) {
        policy::Rebin::NotBins => policy::BinStep::None,
        policy::Rebin::Decided(merged, text) => {
            *spec = merged;
            policy::BinStep::Decided(text)
        }
        policy::Rebin::Refused(path, message, supply) => {
            policy::BinStep::Refused(path, message, supply)
        }
    };
    let fail = |report: &mut Report, error: Error| {
        report.status = "invalid".into();
        report
            .findings
            .push(Finding::new("CG-4", "error", error.message).at(error.path.unwrap_or_default()));
    };
    let source_data = match data::parse(spec) {
        Ok(data) => data,
        Err(error) => {
            fail(report, error);
            return None;
        }
    };
    let prepared = match data::transform(spec, &source_data) {
        Ok(data) => data,
        Err(error) => {
            fail(report, error);
            return None;
        }
    };
    // several panels (layout/multiples.py), unless an index put the measures on one scale or the
    // chart is one table
    let table_message = matches!(
        spec.get("/facts/message").as_str(),
        Some("lookup_only" | "export_only" | "not_comparable")
    );
    let chart_type = spec.string("/chart/type", "auto");
    let panels = panels::applies(spec)
        && spec.get("/transform/kind") != "index"
        && !(table_alone && (chart_type == "table" || (chart_type == "auto" && table_message)));
    Some(Prepared {
        source_data,
        data: prepared,
        bins,
        panels,
    })
}

enum ScaleStep {
    Trained(Option<scales::Explained>),
    Refused,
}
/// Types drawn without a value scale (`__init__.NO_SCALE`), and lines with a total panel, whose
/// panels have their own (`layout/areas.py`).
const NO_SCALE: [&str; 7] = [
    "pie",
    "donut",
    "treemap",
    "table",
    "heatmap",
    "action_dots",
    "line_total",
];
/// Train the value scale and record the original's scale decisions (`compute_scale`).
fn scale_step(
    spec: &Spec,
    plan: &SemanticPlan,
    data: &PreparedData,
    choice: &policy::Choice,
    report: &mut Report,
    panels_own_scales: bool,
) -> Result<ScaleStep> {
    let trained = scales::train_explained(plan, data);
    // `choose_path` trains a line_total's scale; `render` leaves it to the total panel's layout
    let unscaled = NO_SCALE.contains(&choice.type_.as_str())
        && (panels_own_scales || choice.type_ != "line_total");
    if unscaled {
        return Ok(ScaleStep::Trained(trained.ok().flatten()));
    }
    let explained = match trained {
        Ok(x) => x,
        Err(error) if error.kind == "scale" => {
            policy::scale_decisions(spec, data, &choice.type_, Err(error.message), report);
            return Ok(ScaleStep::Refused);
        }
        Err(error) => return Err(error),
    };
    let Some(explained) = explained else {
        return Ok(ScaleStep::Trained(None));
    };
    if !policy::scale_decisions(spec, data, &choice.type_, Ok(&explained), report) {
        return Ok(ScaleStep::Refused);
    }
    Ok(ScaleStep::Trained(Some(explained)))
}

/// The report summary's scale entries (`render`): the scale, its labels (bare numbers under a
/// horizontal scale), the top label and a horizontal scale's title.
/// The scale's report entries, unless the layout gave its own (a bumps chart's rank scale,
/// an arrow chart's panels).
fn scale_summary(spec: &Spec, data: &PreparedData, scale: &TrainedScale, report: &mut Report) {
    report
        .summary
        .entry("scale".into())
        .or_insert_with(|| json!(scales::summary(scale)));
    let Ok(labels) = chartgen_format::scale_labels_with_measure(spec, &data.measure, scale) else {
        return;
    };
    let horizontal = report.summary.get("orientation") == Some(&json!("horizontal"));
    report
        .summary
        .entry("labels".into())
        .or_insert(json!(if horizontal {
            &labels.bare
        } else {
            &labels.labels
        }));
    report
        .summary
        .entry("top".into())
        .or_insert_with(|| json!(labels.top.inline()));
    report
        .summary
        .entry("scale_title".into())
        .or_insert(if horizontal {
            json!(labels.title)
        } else {
            Value::Null
        });
}

/// Validate and choose without fonts or physical layout (`chartgen choose`'s `choose_path`): the
/// same report content up to the scale's decisions, status `ready` where the original's is.
pub fn choose(spec: Value, variant: Option<String>, expect: Value) -> Result<Report> {
    choose_detail(spec, variant, expect).map(|(report, _)| report)
}

/// What `chartgen choose` prints beside the report: the chooser's choice when it made one,
/// and whether the values are parts of a total (which facts the chooser reads).
#[derive(Debug, Clone, Default)]
pub struct ChooseDetail {
    pub choice: Option<policy::Choice>,
    pub breakdown: bool,
}

/// [`choose`] with the [`ChooseDetail`] hosts need to print the original's choose output.
pub fn choose_detail(
    spec: Value,
    variant: Option<String>,
    expect: Value,
) -> Result<(Report, ChooseDetail)> {
    let mut detail = ChooseDetail::default();
    let mut spec = Spec { raw: spec };
    let mut report = Report::new(spec.id(), "plain");
    report.variant = variant;
    report.expect = expect;
    report.source = spec.get("/source").clone();
    let Some(Prepared {
        source_data,
        data: prepared,
        bins,
        panels,
    }) = prepare(&mut spec, &mut report, false)
    else {
        check_expect(&mut report);
        return Ok((report, detail));
    };
    detail.breakdown = policy::is_breakdown(&spec, &prepared);
    let (outcome, choice) =
        policy::evaluate_choice(&spec, &source_data, &prepared, bins, panels, &mut report);
    detail.choice = choice;
    match outcome {
        policy::Outcome::Stopped => {}
        policy::Outcome::Panels => {
            let facts = data::inspect(&prepared);
            let decision = policy::decide(&spec, &prepared, &facts)?;
            report.chart_type = policy::report_type(&spec, &decision);
            report.status = if decision.status == "drawn" {
                "ready".into()
            } else {
                decision.status.clone()
            };
            match panels::prepare(&spec, &prepared, &decision) {
                Ok(panels) => {
                    if let Some(first) = panels.first() {
                        let facts = data::inspect(&first.data);
                        report.path =
                            policy::panel_path(&first.spec, &first.data, &facts, &first.kind);
                    }
                }
                Err(error) if error.kind.starts_with('R') => {
                    report.status = "refused".into();
                    report.chart_type = None;
                    report.findings.push(
                        Finding::new(&error.kind, "error", error.message)
                            .at(error.path.unwrap_or_default()),
                    );
                }
                Err(error) => return Err(error),
            }
        }
        policy::Outcome::Drawn(choice, decision) => {
            report.status = "ready".into();
            let prepared = data::resolve_comparisons(
                &spec,
                decision
                    .chart_type
                    .expect("drawn decision has a chart type"),
                &prepared,
            )?;
            let plan = grammar::compile(&spec, &decision, &prepared)?;
            if let ScaleStep::Refused =
                scale_step(&spec, &plan, &prepared, &choice, &mut report, false)?
            {
                report.status = "refused".into();
            }
        }
    }
    check_expect(&mut report);
    Ok((report, detail))
}

/// The original's source header: `<id>[--<variant>]: GENERATED by chartgen from <file> under
/// style <style>; do not edit. type <type> (<orientation and scale decisions>)`.
fn source_comment(request: &RenderRequest, report: &Report) -> String {
    let decisions: Vec<&str> = report
        .decisions
        .iter()
        .filter(|d| matches!(d["rule"].as_str(), Some("R9-10" | "02 D5")))
        .filter_map(|d| d["text"].as_str())
        .collect();
    format!(
        "{}{}: GENERATED by chartgen from {} under style {}; do not edit. type {} ({})",
        report.chart,
        report
            .variant
            .as_deref()
            .map(|v| format!("--{v}"))
            .unwrap_or_default(),
        request.source_name.as_deref().unwrap_or("a spec"),
        report.style,
        report.chart_type.as_deref().unwrap_or("?"),
        decisions.join("; ")
    )
}

pub fn render_scene(scene: &Scene, backend: &str) -> Result<String> {
    use render::Renderer;
    scene::validate(scene)?;
    match backend {
        "svg" => render::SvgRenderer.render(scene),
        "compatibility_svg" => render::CompatibilitySvgRenderer.render(scene),
        "pgf" | "latex" => render::PgfRenderer.render(scene),
        "typst" => render::TypstRenderer.render(scene),
        _ => Err(Error::new(
            "backend",
            format!("unknown rendering backend: {backend}"),
        )),
    }
}

/// Orchestrate semantic panel preparation before handing physical composition to layout.
pub fn compile_panels(
    spec: &Spec,
    data: &PreparedData,
    decision: &Decision,
    report: &mut Report,
) -> Result<Vec<panel_layout::CompiledPanel>> {
    let mut deferred = vec![];
    let out = compile_panels_deferring(spec, data, decision, report, &mut deferred)?;
    report.decisions.extend(deferred);
    Ok(out)
}

/// As `compile_panels`, the panels' colour decisions (the layouts' in the original) left in
/// `deferred` for the façade to record with the layout's.
fn compile_panels_deferring(
    spec: &Spec,
    data: &PreparedData,
    decision: &Decision,
    report: &mut Report,
    deferred: &mut Vec<Value>,
) -> Result<Vec<panel_layout::CompiledPanel>> {
    if spec
        .get("/comments")
        .as_array()
        .is_some_and(|v| !v.is_empty())
        || !spec.get("/title/link").is_null()
        || (!spec.get("/inset").is_null() && spec.string("/inset/kind", "none") != "none")
    {
        return Err(Error::new(
            "layout",
            "panel-level comments, title connectors and insets need explicit cross-panel geometry",
        ));
    }
    let panels = match panels::prepare(spec, data, decision) {
        Ok(panels) => panels,
        Err(error) if error.kind.starts_with('R') => {
            report.status = "refused".into();
            report.findings.push(
                Finding::new(&error.kind, "error", error.message)
                    .at(error.path.unwrap_or_default()),
            );
            return Ok(vec![]);
        }
        Err(error) => return Err(error),
    };
    // palette slots: fixed by name once over the whole spec (colour_slots), then the series inside
    // the panels each its own slot, the same in every panel (multiples.py, R6-28, G7.50)
    let refs: Vec<colours::PanelRef> = panels
        .iter()
        .map(|p| colours::PanelRef {
            series: &p.series_indices,
            row: p.row,
            column: p.column,
        })
        .collect();
    let within = panels
        .first()
        .map(|p| p.arrangement.within.clone())
        .unwrap_or_default();
    let merged = panels.first().is_some_and(|p| p.kind == "merged");
    let grid = colours::panels_plan(spec, data, &within, merged, &refs)?;
    drop(refs);
    report.findings.extend(grid.findings);
    report.decisions.splice(0..0, grid.leading);
    // the grid's colours follow its arrangement and order (multiples.py): with the layout's
    deferred.extend(grid.decisions);
    let mut compiled = vec![];
    let mut seen_measures = std::collections::BTreeSet::new();
    for (panel, fixed) in panels.into_iter().zip(grid.fixed) {
        if panel.kind == "merged"
            && panel
                .data
                .comparisons
                .iter()
                .any(|c| c.valid && c.render != "none")
        {
            return Err(Error::new(
                "grammar",
                "merged comparisons need per-measure unit conversion",
            ));
        }
        let facts = data::inspect(&panel.data);
        let decision = policy::decide(&panel.spec, &panel.data, &facts)?;
        // multiples._merge: the grid decides the sort, orientation and labelling (OWN_PATHS);
        // a panel's infos count once per measure; a finding repeated by another panel, once
        let measure = panel.spec.get("/data/measure").to_string();
        let first = seen_measures.insert(measure);
        for f in &decision.findings {
            let own = matches!(
                f.path.as_deref(),
                Some("chart.sort" | "chart.orientation" | "chart.labelling")
            );
            if (own && f.severity != "error") || (f.severity == "info" && !first) {
                continue;
            }
            if report
                .findings
                .iter()
                .any(|g| g.rule == f.rule && g.severity == f.severity && g.message == f.message)
            {
                continue;
            }
            report.findings.push(f.clone());
        }
        if decision.status != "drawn" {
            report.status = "refused".into();
            return Ok(vec![]);
        }
        let prepared = data::resolve_comparisons(
            &panel.spec,
            decision
                .chart_type
                .ok_or_else(|| Error::new("grammar", "panel decision has no mark"))?,
            &panel.data,
        )?;
        let mut plan = grammar::compile(&panel.spec, &decision, &prepared)?;
        let mut assigned = colours::compile_with(&mut plan, &prepared, Some(&fixed))?;
        // the colours are the grid's, decided once above (multiples.GRID_RULES)
        assigned.decisions.retain(|d| d["rule"] != "R6-28");
        deferred.extend(add_colours(report, assigned));
        let scale = scales::train(&plan, &prepared)?;
        compiled.push(panel_layout::CompiledPanel {
            panel,
            plan,
            data: prepared,
            scale,
        });
    }
    // Scale sharing is semantic. The physical compositor receives final scales.
    let mut groups = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for (i, c) in compiled.iter().enumerate() {
        let key = match c.panel.scale_mode.as_str() {
            "shared" => Some(0),
            "per_row" => Some(c.panel.row),
            "independent" => None,
            _ => return Err(Error::new("scale", "unsupported panel scale-sharing mode")),
        };
        if let Some(key) = key {
            groups.entry(key).or_default().push(i);
        }
    }
    for indices in groups.values() {
        let first = &compiled[indices[0]];
        if !matches!(
            first.plan.chart_type,
            ChartType::Bar | ChartType::Dot | ChartType::Line | ChartType::Step
        ) || indices
            .iter()
            .any(|i| compiled[*i].plan.chart_type != first.plan.chart_type)
        {
            return Err(Error::new(
                "scale",
                "shared panel scales require matching bar, dot, line or step marks",
            ));
        }
        let mut union = first.data.clone();
        union.series.clear();
        union.comparisons.clear();
        if union.irregular.is_some() {
            union.irregular = Some(vec![]);
        }
        for &i in indices {
            union.series.extend(compiled[i].data.series.iter().cloned());
            union
                .comparisons
                .extend(compiled[i].data.comparisons.iter().cloned());
            if let (Some(union), Some(points)) = (&mut union.irregular, &compiled[i].data.irregular)
            {
                union.extend(points.iter().cloned());
            }
        }
        let scale = scales::train(&first.plan, &union)?;
        for &i in indices {
            compiled[i].scale = scale.clone();
        }
    }
    Ok(compiled)
}

/// Add colour planning's findings to the report, the slots fixed by name leading the decisions
/// (the original records them before the chart is chosen); returns the colours' other decisions,
/// which the original's layouts record. A colour refusal (a heatmap's palette) records them now.
fn add_colours(report: &mut Report, assigned: colours::Assignments) -> Vec<Value> {
    report.findings.extend(assigned.findings);
    report.decisions.splice(0..0, assigned.leading);
    report.overrides.extend(assigned.overrides);
    if assigned.refused {
        report.status = "refused".into();
        report.decisions.extend(assigned.decisions);
        return vec![];
    }
    assigned.decisions
}

/// Check original variant expectations without hiding unexpected errors.
pub fn check_expect(report: &mut Report) {
    let expected = |key: &str| {
        report
            .expect
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let refused = expected("refused");
    let findings = expected("findings");
    let mut missing = Vec::new();
    for rule in &refused {
        if !report.findings.iter().any(|f| {
            Some(f.rule.as_str()) == rule.as_str()
                && matches!(f.severity.as_str(), "error" | "overridden")
        }) {
            missing.push(rule.clone());
        }
    }
    for rule in &findings {
        if !report
            .findings
            .iter()
            .any(|f| Some(f.rule.as_str()) == rule.as_str())
        {
            missing.push(rule.clone());
        }
    }
    let unexpected: Vec<_> = report
        .findings
        .iter()
        .filter(|f| {
            f.severity == "error" && !refused.iter().any(|r| r.as_str() == Some(f.rule.as_str()))
        })
        .map(|f| json!(f.rule))
        .collect();
    report.expect_met = missing.is_empty() && unexpected.is_empty();
    report
        .summary
        .insert("expect_missing".into(), json!(missing));
    report
        .summary
        .insert("unexpected_errors".into(), json!(unexpected));
}
