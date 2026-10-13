//! The original's SVG lint (charts/chartgen/svglint.py, SPEC §3.6), ported: the locked rules that
//! can be read off a drawing in the chart vocabulary, its look resolved under the flattened house
//! stylesheet (the CSS cascade, as bake will draw it) and its labels typeset as bake sets them.
//!
//! Text extents are the host's: pass the measured extent of each `<text>` in document order
//! (`text_queries` lists what to measure). Without them the checks that need text boxes (CG-1,
//! CG-6, CG-7, CG-8 and the inset crossings of 6.6) see no boxes, as the original does without a
//! measurer, and the findings say so (CG-SVG-PARTIAL).
use chartgen_model::{Error, Finding, Result};
use roxmltree::Document;
use std::collections::BTreeMap;

mod checks;
pub mod css;
pub mod drawing;

pub use checks::read_label;
pub use drawing::{Extent, TextQuery};

const VOCABULARY: &str = "action-bar action-dot action-dots all-others annotations area arrow arrow-halo arrow-key average axis-line axis-title bad bar border bumps categorical category category-label cell cell-bar cell-dot cell-plate censored censored-dot censored-dotted censored-hatch censored-hatched censored-marks change-head change-shaft change-start changes chart chart-frame chart-note clustered column-header comment comment-arrow comment-arrow-head comments connector connectors context cvd-safe div-0 div-m1 div-m2 div-m3 div-m4 div-p1 div-p2 div-p3 div-p4 divider-halo donut dot dot-key end-label event-bar event-label event-line events filled footer footnote funnel-bars glow glow-line good graphics grid gridline group-rule header headers heatmap highlight highlight-band horizontal icon incomplete-mark incomplete-note inset inset-fade inset-frame inset-link inset-panel inset-source inset-text key-halo key-pill key-tick labelled leader leader-head legend legend-entry legend-key legend-swatch legend-text line map mark-outline marks math merged missing missing-note multiples note-mark on-dark-cell on-fill on-mid-cell overlay panel panel-divider panel-title pie pie-gap plates plot-frame point points projection prose ramp-key reference reference-line row-header scale scale-label scale-title secondary segment seq-1 seq-2 seq-3 seq-4 seq-5 seq-6 seq-7 seq-8 seq-9 series series-1 series-10 series-11 series-12 series-13 series-14 series-15 series-16 series-2 series-3 series-4 series-5 series-6 series-7 series-8 series-9 shade-dark shade-light signed single slice slice-label slope sparkline stacked stages step step-line subtitle table table-cell table-fill table-rule table-rule-total target-key target-mark text-bad text-bad-cvd text-good text-good-cvd text-group-1 text-group-2 text-group-3 text-group-4 text-group-5 text-group-6 text-halo text-series-1 text-series-10 text-series-11 text-series-12 text-series-13 text-series-14 text-series-15 text-series-16 text-series-2 text-series-3 text-series-4 text-series-5 text-series-6 text-series-7 text-series-8 text-series-9 text-single text-strong texture textures thin-bar tick tile tile-label time time-axis time-divider time-label time-label-2 time-tick tint title title-link title-link-head total treemap trend underlay uniform-context unit-line unit-phrase value value-label vertical waterfall wrapped zebra zero";

pub(crate) fn in_vocabulary(c: &str) -> bool {
    static SET: std::sync::LazyLock<std::collections::HashSet<&'static str>> =
        std::sync::LazyLock::new(|| VOCABULARY.split(' ').collect());
    SET.contains(c)
}

/// Python's round(x, n): the correctly rounded decimal of the binary value.
pub(crate) fn py_round(x: f64, n: usize) -> f64 {
    format!("{:.*}", n, x).parse().unwrap_or(x)
}

/// Python's format(x, "g").
pub(crate) fn fmt_g(x: f64) -> String {
    if x == 0. {
        return "0".into();
    }
    let exp = x.abs().log10().floor() as i32;
    let rounded: f64 = format!("{:.5e}", x).parse().unwrap_or(x);
    let exp = if rounded.abs() >= 10f64.powi(exp + 1) {
        exp + 1
    } else {
        exp
    };
    if !(-4..6).contains(&exp) {
        let s = format!("{:.5e}", x);
        let (m, e) = s.split_once('e').unwrap();
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap();
        return format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let s = format!("{:.*}", (5 - exp).max(0) as usize, x);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

/// How a drawing is linted.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The flattened house stylesheet the drawing will be baked under (`<style id="house">`).
    pub css: Option<String>,
    /// The typeset extent of every `<text>`, in document order.
    pub extents: Option<Vec<Option<Extent>>>,
    /// Skip the checks the model lint already makes on generated charts.
    pub generated: bool,
    pub final_width: Option<f64>,
    /// The audience's colour-vision assumption: `assume` or `no`.
    pub cvd: String,
    pub date_format: Option<String>,
}

fn parse(svg: &str) -> Result<Document<'_>> {
    let doc = Document::parse_with_options(
        svg,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|e| Error::new("svg", e.to_string()))?;
    if doc.root_element().tag_name().name() != "svg" {
        return Err(Error::new("svg", "root element must be <svg>"));
    }
    Ok(doc)
}

fn tokens(css_text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (sel, decls) in css::parse(&css::strip_comments(css_text)) {
        if sel.trim() == ":root" {
            for (p, v) in decls {
                if p.starts_with("--") {
                    out.insert(p, v);
                }
            }
        }
    }
    out
}

/// What the host measures: each `<text>` as bake typesets it, in document order.
pub fn text_queries(svg: &str, css: Option<&str>) -> Result<Vec<TextQuery>> {
    let doc = parse(svg)?;
    let toks = css.map(tokens).unwrap_or_default();
    let style = css.map(|c| drawing::Style {
        css: c,
        tokens: &toks,
    });
    let d = drawing::Drawing::new(&doc, style.as_ref(), None);
    Ok(d.texts
        .iter()
        .map(|t| TextQuery {
            mode: t.mode.clone(),
            code: t.code.clone(),
            size: t.size,
            weight: t.weight.as_ref().map(|w| w.to_string()),
            slant: t.slant.clone(),
        })
        .collect())
}

/// The findings of the SVG lint on one drawing, in the original's check order.
pub fn lint(svg: &str, opts: &Options) -> Result<Vec<Finding>> {
    let doc = parse(svg)?;
    let toks = opts.css.as_deref().map(tokens).unwrap_or_default();
    let style = opts.css.as_deref().map(|c| drawing::Style {
        css: c,
        tokens: &toks,
    });
    let mut d = drawing::Drawing::new(&doc, style.as_ref(), opts.final_width);
    let measured = match &opts.extents {
        Some(e) if e.len() == d.texts.len() => {
            d.set_extents(e);
            true
        }
        Some(e) => {
            return Err(Error::new(
                "svg",
                format!("{} text extents for {} texts", e.len(), d.texts.len()),
            ));
        }
        None => d.texts.is_empty(),
    };
    let metric = |k: &str| style.as_ref().and_then(|s| s.metric(k));
    let mut l = checks::Lint {
        d,
        cvd: if opts.cvd.is_empty() {
            "assume".into()
        } else {
            opts.cvd.clone()
        },
        date_format: opts.date_format.clone(),
        divider_clear: metric("--divider-clear"),
        text_pad: metric("--text-pad"),
        findings: vec![],
    };
    type Check = fn(&mut checks::Lint);
    let all: &[(Check, bool)] = &[
        (checks::frames, false),
        (checks::gridlines, false),
        (checks::non_data, false),
        (checks::text_orientation, false),
        (checks::text_size, false),
        (checks::text_contrast, false),
        (checks::collisions, true),
        (checks::text_styles, true),
        (checks::subtitle, false),
        (checks::colours, false),
        (checks::series_colours, false),
        (checks::highlight, false),
        (checks::empty_bars, false),
        (checks::bars_from_zero, false),
        (checks::scale_labels, false),
        (checks::scale_count, true),
        (checks::unit_phrase, false),
        (checks::percent_labels, false),
        (checks::insets, false),
        (checks::flat, false),
        (checks::dates, false),
        (checks::title_year, true),
        (checks::vocabulary, false),
        (checks::time_label_dividers, false),
        (checks::label_lines, false),
        (checks::line_shapes, false),
        (checks::dashes, false),
        (checks::shared_colours, false),
        (checks::legend_order, false),
        (checks::empty_dots, false),
        (checks::time_ticks, false),
        (checks::event_bars, false),
        (checks::stacks, false),
        (checks::footnotes, false),
    ];
    for (check, model) in all {
        if opts.generated && *model {
            continue;
        }
        check(&mut l);
    }
    if !measured {
        let mut f = Finding::new(
            "CG-SVG-PARTIAL",
            "info",
            "text extents were not supplied: the checks that need text boxes (text collisions CG-1,              labels beside dividers CG-6 and lines CG-7, footnotes CG-8, an inset's crossings 6.6)              saw none; measure the texts (text_queries) and lint again",
        );
        f.elements = vec!["svg".into()];
        l.findings.push(f);
    }
    Ok(l.findings)
}

/// The lint as the protocol's `lint_svg` called it before the port: a style name and no
/// stylesheet or extents. Kept for the protocol; hosts should pass the flattened stylesheet.
pub fn lint_svg(
    svg: &str,
    _style: &str,
    final_width: Option<f64>,
    cvd: &str,
) -> Result<Vec<Finding>> {
    let mut findings = lint(
        svg,
        &Options {
            final_width,
            cvd: cvd.into(),
            ..Options::default()
        },
    )?;
    let mut f = Finding::new(
        "CG-SVG-PARTIAL",
        "info",
        "no flattened stylesheet was supplied: the drawing's colours and text styles were resolved          from the SVG alone, not as the house style will bake them",
    );
    f.elements = vec!["svg".into()];
    findings.push(f);
    Ok(findings)
}
