//! Exact decimal scale training, ported from the legacy `scales.py` snapping procedure.
use chartgen_model::{Decimal, Error, PreparedData, Result, SemanticPlan, TrainedScale};
use serde_json::Value;
use std::str::FromStr;
fn err(m: &str) -> Error {
    Error::new("scale", m)
}
fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}
fn num(v: &Value) -> Result<Option<Decimal>> {
    if v.is_null() {
        Ok(None)
    } else {
        Decimal::from_str(&v.to_string())
            .map(Some)
            .map_err(|_| err("invalid scale number"))
    }
}
fn ceil_q(v: Decimal, s: Decimal) -> Decimal {
    (v / s).ceil() * s
}
fn floor_q(v: Decimal, s: Decimal) -> Decimal {
    (v / s).floor() * s
}
fn outside(x: Decimal, lo: Decimal, hi: Decimal) -> Decimal {
    if x < lo {
        lo - x
    } else if x > hi {
        x - hi
    } else {
        Decimal::ZERO
    }
}
fn snap_up(vmax: Decimal, base: Decimal, s: Decimal, b: Decimal) -> (Decimal, Decimal) {
    let lo = vmax + (b - d("0.05")) * base;
    let hi = vmax + (b + d("0.05")) * base;
    let inside = ceil_q(lo, s);
    if inside <= hi {
        return (inside, Decimal::ZERO);
    }
    let mut choices = vec![floor_q(lo, s), ceil_q(hi, s)];
    choices.retain(|x| *x - vmax >= d("0.05") * base);
    if choices.is_empty() {
        choices.push(floor_q(vmax, s) + s)
    }
    choices.sort_by_key(|x| (outside(*x, lo, hi), *x));
    let u = choices[0];
    (u, outside(u, lo, hi) / base)
}
fn snap_down(vmin: Decimal, base: Decimal, s: Decimal, b: Decimal) -> (Decimal, Decimal) {
    let (u, m) = snap_up(-vmin, base, s, b);
    (-u, m)
}
fn power10(k: i32) -> Decimal {
    if k < 0 {
        Decimal::from_i128_with_scale(1, (-k) as u32)
    } else {
        let mut x = Decimal::ONE;
        for _ in 0..k {
            x *= Decimal::from(10)
        }
        x
    }
}
fn order10(v: Decimal) -> i32 {
    let v = v.abs();
    let mut k = 0i32;
    let mut n = v;
    if n >= Decimal::ONE {
        while n >= Decimal::from(10) {
            n /= Decimal::from(10);
            k += 1;
        }
    } else {
        while n < Decimal::ONE {
            n *= Decimal::from(10);
            k -= 1;
        }
    }
    k
}
/// The original's nice scale (`scales.nice_scale`) over values, for layouts that train a
/// scale of their own (an arrow chart's panels).
#[allow(clippy::too_many_arguments)]
pub fn nice_scale(
    values: &[Decimal],
    kind: &str,
    zero: &str,
    measure_zero: &str,
    t: Option<Decimal>,
    m: Option<Decimal>,
    steps: &[Decimal],
    allow25: bool,
    target: i64,
    pinned_step: Option<Decimal>,
    pinned_min: Option<Decimal>,
    pinned_max: Option<Decimal>,
    precision: Option<Decimal>,
    breathing: Decimal,
    distance_irrelevant: bool,
) -> Result<TrainedScale> {
    nice_explained(
        values,
        kind,
        zero,
        measure_zero,
        t,
        m,
        steps,
        allow25,
        target,
        pinned_step,
        pinned_min,
        pinned_max,
        precision,
        breathing,
        distance_irrelevant,
    )
    .map(|x| x.scale)
}

/// The spec's scale settings as `nice_scale` takes them: (steps, allow 2.5, target
/// intervals, breathing, step, precision, min, max).
#[allow(clippy::type_complexity)]
pub fn spec_settings(
    spec: &chartgen_model::Spec,
) -> Result<(
    Vec<Decimal>,
    bool,
    i64,
    Decimal,
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
)> {
    let sc = spec.get("/scale");
    let steps = sc["steps"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| num(v).ok().flatten())
                .collect::<Vec<_>>()
        })
        .filter(|x| !x.is_empty())
        .unwrap_or_else(|| vec![Decimal::ONE, Decimal::from(2), Decimal::from(5)]);
    Ok((
        steps,
        sc["allow_2_5"].as_bool().unwrap_or(false),
        sc["target_intervals"].as_i64().unwrap_or(5),
        num(&sc["breathing"])?.unwrap_or(d("0.15")),
        num(&sc["step"])?,
        num(&sc["precision_value"])?,
        num(&sc["min"])?,
        num(&sc["max"])?,
    ))
}

/// A trained scale with what the report says of it (`scales.Scale.reasons`, `capped_hi`).
#[derive(Debug, Clone)]
pub struct Explained {
    pub scale: TrainedScale,
    /// (rule, text): how the lower-limit mode was chosen, and a smaller step's reason (02 D5)
    pub reasons: Vec<(String, String)>,
    /// the top is a theoretical maximum the data may touch
    pub capped_hi: bool,
    /// the bottom is a theoretical minimum the data may touch (a mirrored, all-negative scale)
    pub capped_lo: bool,
    /// the values the scale holds
    pub values: Vec<Decimal>,
    /// the lower limit the procedure gives without a pinned `scale.min` (lines and dots only;
    /// None when nothing is pinned or no step fits)
    pub unpinned_lo: Option<Decimal>,
}

/// `scales.LENGTH_ENCODED`: marks whose length is their value, which always start at zero.
pub const LENGTH_ENCODED: [&str; 9] = [
    "bar",
    "interval_bar",
    "stacked_bar",
    "stacked_area",
    "waterfall",
    "thin_bar",
    "wrapped_bar",
    "stacked_bar_100",
    "stacked_area_100",
];

/// The lower-limit modes of `scales.py`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Zero,
    Theoretical,
    Truncated,
    Mixed,
    Fixed,
}
impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Zero => "zero",
            Mode::Theoretical => "theoretical",
            Mode::Truncated => "truncated",
            Mode::Mixed => "mixed",
            Mode::Fixed => "fixed",
        }
    }
}

/// `scales._plain`: a limit or step for reports, grouped in threes, a true minus sign.
pub fn plain(x: Decimal) -> String {
    let n = x.normalize();
    let text = if n.is_zero() {
        "0".to_owned()
    } else {
        n.abs().to_string()
    };
    let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = whole.to_owned();
    if whole.len() >= 4 {
        let head = match whole.len() % 3 {
            0 => 3,
            h => h,
        };
        grouped = whole[..head].to_owned();
        let mut i = head;
        while i < whole.len() {
            grouped.push(',');
            grouped.push_str(&whole[i..i + 3]);
            i += 3;
        }
    }
    format!(
        "{}{}{}",
        if n < Decimal::ZERO { "\u{2212}" } else { "" },
        grouped,
        if frac.is_empty() {
            String::new()
        } else {
            format!(".{frac}")
        }
    )
}

/// `Scale.summary()`: "0–60 by 10".
pub fn summary(scale: &TrainedScale) -> String {
    // `Scale.summary()`
    format!(
        "{}–{} by {}",
        plain(scale.lo),
        plain(scale.hi),
        plain(scale.step)
    )
}

/// `scales.breathing_room`: (room at the top, room at the bottom) as fractions of the data's
/// extent from the other limit, for the R2-27 lint; None where the limit is fixed.
pub fn breathing_room(x: &Explained) -> (Option<f64>, Option<f64>) {
    let (Some(vmin), Some(vmax)) = (x.values.iter().min(), x.values.iter().max()) else {
        return (None, None);
    };
    let (vmin, vmax) = (*vmin, *vmax);
    let s = &x.scale;
    let mixed = s.mode == "mixed";
    let ratio = |a: Decimal, b: Decimal| {
        if b.is_zero() { None } else { (a / b).to_f64() }
    };
    let top = if vmax > Decimal::ZERO && !x.capped_hi {
        ratio(s.hi - vmax, vmax - if mixed { vmin } else { s.lo })
    } else {
        None
    };
    let bottom = if vmin < Decimal::ZERO && !x.capped_lo {
        ratio(vmin - s.lo, if mixed { vmax } else { s.hi } - vmin)
    } else {
        None
    };
    (top, bottom)
}

fn percent(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

/// The lower-limit mode of 02 D2/D3 as SPEC §6.1 writes it, under D-10 (a)
/// (`scales.lower_limit_mode`): (mode, fixed lower limit, [(rule, reason)]). Bars and the other
/// length-encoded marks always start at zero (R2-24, R9-5).
pub fn lower_limit_mode(
    vmin: Decimal,
    vmax: Decimal,
    chart_type: &str,
    zero: &str,
    measure_zero: &str,
    theoretical: Option<Decimal>,
    distance_irrelevant: bool,
) -> (Mode, Option<Decimal>, Vec<(String, String)>) {
    let spread = vmax - vmin;
    let length = LENGTH_ENCODED.contains(&chart_type);
    let one = |rule: &str, text: &str| vec![(rule.to_owned(), text.to_owned())];
    if vmin < Decimal::ZERO && Decimal::ZERO < vmax {
        return (
            Mode::Mixed,
            None,
            one("R2-30", "values on both sides of zero: room at both ends"),
        );
    }
    if vmax <= Decimal::ZERO {
        return (
            Mode::Zero,
            Some(Decimal::ZERO),
            one("R2-29", "all values negative: the scale ends at zero"),
        );
    }
    if length || zero == "include" {
        return (
            Mode::Zero,
            Some(Decimal::ZERO),
            if length {
                one("R2-24", "bars start at zero")
            } else {
                one("R2-22", "zero included by the spec")
            },
        );
    }
    if let Some(t) = theoretical
        && t > Decimal::ZERO
        && vmin - d("0.15") * spread <= t
    {
        return (
            Mode::Theoretical,
            Some(t),
            one(
                "R2-26",
                &format!(
                    "the theoretical minimum {} lies just below the data",
                    plain(t)
                ),
            ),
        );
    }
    if spread.is_zero() || vmin < spread {
        return (
            Mode::Zero,
            Some(Decimal::ZERO),
            one(
                "R2-25",
                "the values are not far from zero compared with their spread",
            ),
        );
    }
    if zero == "truncate" || measure_zero == "arbitrary" || distance_irrelevant {
        let fact = if zero == "truncate" {
            "scale.zero truncate"
        } else if measure_zero == "arbitrary" {
            "zero is arbitrary here, not “none of it” (G2.10)"
        } else {
            "the distance to zero is declared irrelevant (G2.8)"
        };
        return (
            Mode::Truncated,
            None,
            one(
                "R2-24",
                &format!(
                    "{fact}: truncated, the lower limit 10–20 % of the spread below the lowest value (G2.11)"
                ),
            ),
        );
    }
    (
        Mode::Zero,
        Some(Decimal::ZERO),
        one(
            "R2-22",
            "zero included (a narrow range far from zero wants an inset)",
        ),
    )
}
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn nice(
    values: &[Decimal],
    kind: &str,
    zero: &str,
    measure_zero: &str,
    t: Option<Decimal>,
    m: Option<Decimal>,
    steps: &[Decimal],
    allow25: bool,
    target: i64,
    pinned_step: Option<Decimal>,
    pinned_min: Option<Decimal>,
    pinned_max: Option<Decimal>,
    precision: Option<Decimal>,
    breathing: Decimal,
    distance_irrelevant: bool,
) -> Result<TrainedScale> {
    nice_explained(
        values,
        kind,
        zero,
        measure_zero,
        t,
        m,
        steps,
        allow25,
        target,
        pinned_step,
        pinned_min,
        pinned_max,
        precision,
        breathing,
        distance_irrelevant,
    )
    .map(|x| x.scale)
}
#[allow(clippy::too_many_arguments)]
fn nice_explained(
    values: &[Decimal],
    kind: &str,
    zero: &str,
    measure_zero: &str,
    t: Option<Decimal>,
    m: Option<Decimal>,
    steps: &[Decimal],
    allow25: bool,
    target: i64,
    pinned_step: Option<Decimal>,
    pinned_min: Option<Decimal>,
    pinned_max: Option<Decimal>,
    precision: Option<Decimal>,
    breathing: Decimal,
    distance_irrelevant: bool,
) -> Result<Explained> {
    let (mut vmin, mut vmax) = values
        .iter()
        .fold((Decimal::MAX, Decimal::MIN), |(a, b), v| {
            (a.min(*v), b.max(*v))
        });
    if values.is_empty() {
        vmin = Decimal::ZERO;
        vmax = Decimal::ZERO;
    }
    let mut t = t;
    let mut m = m;
    let mut pmin = pinned_min;
    let mut pmax = pinned_max;
    let mirror = vmax <= Decimal::ZERO && vmin < Decimal::ZERO;
    if mirror {
        let (a, b) = (-vmax, -vmin);
        vmin = a;
        vmax = b;
        let (a, b) = (m.map(|x| -x), t.map(|x| -x));
        t = a;
        m = b;
        let (a, b) = (pmax.map(|x| -x), pmin.map(|x| -x));
        pmin = a;
        pmax = b;
    }
    let spread = vmax - vmin;
    let (mode, fixed, mut reasons) =
        lower_limit_mode(vmin, vmax, kind, zero, measure_zero, t, distance_irrelevant);
    let (mut mode, mut fixed) = (mode.as_str(), fixed);
    if mirror {
        reasons = vec![(
            "R2-29".into(),
            "all values negative: the scale is mirrored, ending at zero".into(),
        )];
    }
    if let Some(x) = pmin {
        mode = "fixed";
        fixed = Some(x);
        reasons = vec![(
            "02 D2".into(),
            format!("the lower limit {} is pinned by the spec", plain(x)),
        )];
    }
    let extent = vmax - fixed.unwrap_or(vmin - d("0.2") * spread);
    let extent = if extent > Decimal::ZERO {
        extent
    } else {
        vmax.abs().max(Decimal::ONE)
    };
    let k = order10(extent);
    let mut candidates = Vec::new();
    if let Some(s) = pinned_step {
        candidates.push(s)
    } else {
        for n in k - 2..=k + 2 {
            for x in steps {
                let p = power10(n);
                candidates.push(*x * p)
            }
            if allow25 && !steps.contains(&d("2.5")) {
                candidates.push(d("2.5") * power10(n))
            }
        }
    }
    let mut best: Option<(f64, i64, Decimal, Decimal, Decimal, Decimal, bool)> = None;
    for s in candidates {
        if s <= Decimal::ZERO {
            continue;
        }
        let (l, miss_l) = if let Some(x) = fixed {
            if x % s != Decimal::ZERO {
                continue;
            }
            (x, Decimal::ZERO)
        } else {
            let base = if spread > Decimal::ZERO {
                spread
            } else {
                vmin.abs().max(Decimal::ONE)
            };
            let (mut l, mut miss) = snap_down(vmin, base, s, breathing);
            if mode == "truncated" && l < Decimal::ZERO && vmin >= Decimal::ZERO {
                l = Decimal::ZERO;
                miss = Decimal::ZERO
            }
            if let Some(t) = t {
                if l < t {
                    l = t;
                    miss = Decimal::ZERO;
                    if t % s != Decimal::ZERO {
                        continue;
                    }
                }
            }
            (l, miss)
        };
        let base = if mode == "mixed" { spread } else { vmax - l };
        let base = if base > Decimal::ZERO {
            base
        } else {
            (vmax - l).abs().max(Decimal::ONE)
        };
        let (u, miss_u, capped) = if let Some(x) = pmax {
            if x % s != Decimal::ZERO {
                continue;
            }
            (x, Decimal::ZERO, false)
        } else {
            let (mut u, mut miss) = snap_up(vmax, base, s, breathing);
            let mut capped = false;
            if let Some(m) = m {
                if u > m {
                    if m % s != Decimal::ZERO {
                        continue;
                    }
                    u = m;
                    miss = Decimal::ZERO;
                    capped = true
                }
            }
            (u, miss, capped)
        };
        if u <= l {
            continue;
        }
        let n = ((u - l) / s)
            .to_i64()
            .ok_or_else(|| err("too many intervals"))?;
        if !(2..=10).contains(&n) {
            continue;
        }
        if precision.is_some_and(|p| p % s != Decimal::ZERO) {
            continue;
        }
        let mant = s / power10(order10(s));
        let score = (n - target).abs() as f64
            + 10.0 * (miss_l + miss_u).to_f64().unwrap_or(1000.0)
            + if mant == d("2.5") { 0.5 } else { 0.0 };
        let replace = best.as_ref().is_none_or(|b| {
            score < b.0 - 1e-9 || ((score - b.0).abs() < 1e-9 && (n < b.1 || (n == b.1 && s > b.2)))
        });
        if replace {
            best = Some((score, n, s, l, u, vmax, capped));
        }
    }
    let (_, _, mut s, mut l, mut u, _, capped) =
        best.ok_or_else(|| err("no step gives 2 to 10 intervals"))?;
    // Legacy late refinement: reduce a coarse 5 or 1 step that leaves over 25% headroom.
    let base = if mode == "mixed" { spread } else { vmax - l };
    if pinned_step.is_none()
        && !capped
        && pmax.is_none()
        && base > Decimal::ZERO
        && (s / power10(order10(s)) == Decimal::ONE || s / power10(order10(s)) == Decimal::from(5))
        && (u - vmax) / base > d("0.25")
    {
        let s2 = if s / power10(order10(s)) == Decimal::from(5) {
            s * d("0.4")
        } else {
            s / d("2")
        };
        if precision.is_none_or(|p| p % s2 == Decimal::ZERO) {
            let l2 = if fixed.is_some() {
                l
            } else {
                let mut x = snap_down(
                    vmin,
                    if spread > Decimal::ZERO {
                        spread
                    } else {
                        vmin.abs().max(Decimal::ONE)
                    },
                    s2,
                    breathing,
                )
                .0;
                if mode == "truncated" && x < Decimal::ZERO && vmin >= Decimal::ZERO {
                    x = Decimal::ZERO
                }
                x
            };
            if t.is_none_or(|t| l2 >= t) && l2 % s2 == Decimal::ZERO {
                let base2 = if mode == "mixed" { spread } else { vmax - l2 };
                let u2 = ceil_q(vmax + d("0.05") * base2, s2);
                let n2 = (u2 - l2) / s2;
                if n2 <= Decimal::from(8)
                    && n2 >= Decimal::from(2)
                    && (u2 - vmax) / base2 >= d("0.05")
                    && m.is_none_or(|m| u2 <= m)
                {
                    let room = ((u - vmax) / base).to_f64().unwrap_or(0.0);
                    let room2 = ((u2 - vmax) / base2).to_f64().unwrap_or(0.0);
                    reasons.push((
                        "02 D5".into(),
                        format!(
                            "the step {} would leave {} room above the data; {} gives {}–{}, {} intervals, {} room",
                            plain(s),
                            percent(room),
                            plain(s2),
                            plain(l2),
                            plain(u2),
                            n2.normalize(),
                            percent(room2)
                        ),
                    ));
                    l = l2;
                    u = u2;
                    s = s2;
                }
            }
        }
    }
    if mirror {
        let (a, b) = (-u, -l);
        l = a;
        u = b;
    }
    let n = ((u - l) / s)
        .to_u64()
        .ok_or_else(|| err("invalid interval count"))?;
    let ticks = (0..=n).map(|i| l + s * Decimal::from(i)).collect();
    Ok(Explained {
        scale: TrainedScale {
            lo: l,
            hi: u,
            step: s,
            ticks,
            mode: mode.into(),
        },
        reasons,
        capped_hi: capped && !mirror,
        capped_lo: capped && mirror,
        values: values.to_vec(),
        unpinned_lo: None,
    })
}
use rust_decimal::prelude::ToPrimitive;
pub fn train(plan: &SemanticPlan, data: &PreparedData) -> Result<Option<TrainedScale>> {
    Ok(train_explained(plan, data)?.map(|x| x.scale))
}
/// Train the value scale of a plan, with the reasons `compute_scale` reports.
pub fn train_explained(plan: &SemanticPlan, data: &PreparedData) -> Result<Option<Explained>> {
    if plan.chart_type == chartgen_model::ChartType::Bumps {
        let scale = rank_scale(plan, data)?;
        return Ok(Some(Explained {
            values: scale.ticks.clone(),
            scale,
            reasons: vec![],
            capped_hi: false,
            capped_lo: false,
            unpinned_lo: None,
        }));
    }
    if matches!(
        plan.chart_type,
        chartgen_model::ChartType::Table
            | chartgen_model::ChartType::Heatmap
            | chartgen_model::ChartType::ActionDots
            | chartgen_model::ChartType::Pie
            | chartgen_model::ChartType::Donut
            | chartgen_model::ChartType::Treemap
            | chartgen_model::ChartType::StateMatrix
            | chartgen_model::ChartType::Contingency
    ) {
        return Ok(None);
    }
    let mut values: Vec<Decimal> =
        match plan.chart_type {
            // the extent of what the statistical chart draws: whiskers and outliers, intervals,
            // both conditions, the curves (chartgen-statistics)
            t if t.is_statistical() => statistical_extent(plan).unwrap_or_default(),
            chartgen_model::ChartType::StackedBar100
            | chartgen_model::ChartType::StackedArea100 => vec![Decimal::ZERO, Decimal::from(100)],
            chartgen_model::ChartType::StackedBar if data.series.len() == 1 => {
                let mut pos = Decimal::ZERO;
                let mut neg = Decimal::ZERO;
                for d in &data.series[0].values {
                    if let Some(v) = d.value {
                        if v >= Decimal::ZERO {
                            pos += v;
                        } else {
                            neg += v;
                        }
                    }
                }
                vec![pos, neg]
            }
            // Thin bars stack the events at one instant (the original's step.scale_values, D-42):
            // the scale holds each instant's positive and negative totals.
            chartgen_model::ChartType::ThinBar if data.irregular.is_some() => {
                let mut pos: Vec<(Decimal, Decimal)> = vec![];
                let mut neg: Vec<(Decimal, Decimal)> = vec![];
                for p in data.irregular.iter().flatten().flatten() {
                    let Some(v) = p.datum.value else { continue };
                    let side = if v >= Decimal::ZERO {
                        &mut pos
                    } else {
                        &mut neg
                    };
                    match side.iter_mut().find(|(k, _)| *k == p.position) {
                        Some(entry) => entry.1 += v,
                        None => side.push((p.position, v)),
                    }
                }
                pos.into_iter().chain(neg).map(|(_, v)| v).collect()
            }
            chartgen_model::ChartType::StackedBar | chartgen_model::ChartType::StackedArea => (0
                ..data.categories.len())
                .flat_map(|j| {
                    let mut pos = Decimal::ZERO;
                    let mut neg = Decimal::ZERO;
                    for s in &data.series {
                        if let Some(v) = s.values.get(j).and_then(|d| d.value) {
                            if v >= Decimal::ZERO {
                                pos += v
                            } else {
                                neg += v
                            }
                        }
                    }
                    [pos, neg]
                })
                .collect(),
            chartgen_model::ChartType::Waterfall => {
                // layout/waterfall.scale_values: every running total of the one series, a total
                // category resetting it, and zero, up to the first value not known
                let roles = plan.spec.get("/data/x/roles");
                let mut run = Decimal::ZERO;
                let mut out = vec![run];
                if let Some(s) = data.series.first() {
                    for (j, d) in s.values.iter().enumerate() {
                        let Some(v) = d.value else { break };
                        let total = data
                            .categories
                            .get(j)
                            .and_then(|c| roles.get(c))
                            .is_some_and(|r| r == "total");
                        if total {
                            run = v;
                            out.push(run);
                        } else {
                            out.push(run);
                            out.push(run + v);
                            run += v;
                        }
                    }
                }
                out
            }
            _ => {
                if let Some(irregular) = &data.irregular {
                    if plan.chart_type == chartgen_model::ChartType::ThinBar {
                        // layout/step.scale_values: events at one instant stacked (D-42), their
                        // positive and negative sums
                        let mut pos = std::collections::BTreeMap::<&str, Decimal>::new();
                        let mut neg = std::collections::BTreeMap::<&str, Decimal>::new();
                        for p in irregular.iter().flatten() {
                            if let Some(v) = p.datum.value {
                                let side = if v >= Decimal::ZERO {
                                    &mut pos
                                } else {
                                    &mut neg
                                };
                                *side.entry(p.time.as_str()).or_default() += v;
                            }
                        }
                        pos.into_values().chain(neg.into_values()).collect()
                    } else {
                        irregular
                            .iter()
                            .flat_map(|row| row.iter().filter_map(|p| p.datum.value))
                            .collect()
                    }
                } else {
                    data.series
                        .iter()
                        .flat_map(|s| s.values.iter().filter_map(|d| d.value))
                        .collect()
                }
            }
        };
    for c in &data.comparisons {
        if c.render == "none" {
            continue;
        }
        for v in &c.values {
            if let Some(x) = v.value {
                values.push(x);
            }
        }
    }
    explain_values(plan, values, &data.measure, plan.spec.get("/scale"))
}

/// The value extent a statistical chart draws on its value axis, as exact decimals.
fn statistical_extent(plan: &SemanticPlan) -> Option<Vec<Decimal>> {
    let mut notes = chartgen_statistics::notes::Notes::default();
    let model = chartgen_statistics::parse(&plan.spec.raw, plan.chart_type.as_str(), &mut notes)?;
    let (lo, hi) = model.extent()?;
    Some(vec![
        Decimal::from_f64_retain(lo)?.round_dp(9),
        Decimal::from_f64_retain(hi)?.round_dp(9),
    ])
}

/// Train a scale from the values it must hold, a measure and the scale options (`train_explained`
/// for the chart's value axis; a scatter plot's x axis trains here with its own measure).
pub fn explain_values(
    plan: &SemanticPlan,
    values: Vec<Decimal>,
    m: &Value,
    sc: &Value,
) -> Result<Option<Explained>> {
    if values.is_empty() {
        return Ok(None);
    }
    let steps = sc["steps"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| num(v).ok().flatten())
                .collect::<Vec<_>>()
        })
        .filter(|x| !x.is_empty())
        .unwrap_or_else(|| vec![Decimal::ONE, Decimal::from(2), Decimal::from(5)]);
    let theoretical_max = if matches!(
        plan.chart_type,
        chartgen_model::ChartType::StackedBar100 | chartgen_model::ChartType::StackedArea100
    ) {
        Some(Decimal::from(100))
    } else {
        num(&m["max"])?
    };
    let train = |pinned_min: Option<Decimal>| -> Result<Explained> {
        nice_explained(
            &values,
            plan.chart_type.as_str(),
            sc["zero"].as_str().unwrap_or("auto"),
            m["zero"].as_str().unwrap_or("meaningful"),
            num(&m["min"])?,
            theoretical_max,
            &steps,
            sc["allow_2_5"].as_bool().unwrap_or(false),
            sc["target_intervals"].as_i64().unwrap_or(5),
            num(&sc["step"])?,
            pinned_min,
            num(&sc["max"])?,
            num(&sc["precision_value"])?,
            num(&sc["breathing"])?.unwrap_or(d("0.15")),
            plan.spec.get("/facts/distance_to_zero_irrelevant") == true,
        )
    };
    let pinned = num(&sc["min"])?;
    let mut explained = train(pinned)?;
    if pinned.is_some() && !LENGTH_ENCODED.contains(&plan.chart_type.as_str()) {
        explained.unpinned_lo = train(None).ok().map(|x| x.scale.lo);
    }
    Ok(Some(explained))
}
#[cfg(test)]
mod tests {
    use super::*;
    use chartgen_model::{
        ChartType, Datum, Layer, Orientation, PreparedData, SemanticPlan, Series, Spec,
    };
    use serde_json::json;
    #[test]
    fn one_series_stacked_bar_uses_cumulative_part_extent() {
        for (values, expected_hi, expected_lo) in [
            (&[375, 348, 200][..], Decimal::from(923), Decimal::ZERO),
            (&[34, 8, -4, -16][..], Decimal::from(42), Decimal::from(-20)),
        ] {
            let data = PreparedData {
                categories: (0..values.len()).map(|i| i.to_string()).collect(),
                x_kind: "nominal".into(),
                series: vec![Series {
                    id: "s".into(),
                    name: "S".into(),
                    values: values
                        .iter()
                        .map(|v| Datum {
                            value: Some(Decimal::from(*v)),
                            status: "known".into(),
                            certainty: "actual".into(),
                            complete: true,
                            note: None,
                            limit: None,
                        })
                        .collect(),
                }],
                measure: json!({"zero":"meaningful"}),
                irregular: None,
                comparisons: vec![],
            };
            let plan = SemanticPlan {
                spec: Spec { raw: json!({}) },
                chart_type: ChartType::StackedBar,
                orientation: Orientation::Vertical,
                order: (0..values.len()).collect(),
                layers: Vec::<Layer>::new(),
                coordinates: "cartesian".into(),
                colors: chartgen_model::ColorPlan::default(),
            };
            let scale = train(&plan, &data).unwrap().unwrap();
            assert!(scale.hi >= expected_hi);
            assert!(scale.lo <= expected_lo);
        }
    }
    #[test]
    fn legacy_book_scales() {
        let cases: &[(&[&str], &str, Option<&str>, Option<&str>, &str, &str, &str)] = &[
            (&["21"], "bar", None, None, "0", "25", "5"),
            (&["49.5"], "bar", None, None, "0", "60", "10"),
            (&["114.5"], "bar", None, None, "0", "140", "20"),
            (&["396"], "bar", None, None, "0", "500", "100"),
            (&["18.1"], "bar", None, None, "0", "20", "5"),
            (&["53.7"], "bar", None, None, "0", "60", "10"),
            (&["2.20"], "bar", None, None, "0", "2.5", "0.5"),
            (&["3.16"], "bar", None, None, "0", "3.5", "0.5"),
            (&["1794"], "bar", None, None, "0", "2000", "500"),
            (
                &["59", "57", "80", "95", "90", "64"],
                "bar",
                None,
                None,
                "0",
                "100",
                "20",
            ),
            (
                &["467", "445", "361", "314", "307"],
                "dot",
                Some("300"),
                Some("850"),
                "300",
                "500",
                "50",
            ),
            (&["-14", "58"], "line", None, None, "-20", "80", "20"),
            (
                &["45", "97", "92", "94", "46", "84"],
                "bar",
                None,
                Some("100"),
                "0",
                "100",
                "20",
            ),
        ];
        for &(v, kind, t, m, lo, hi, step) in cases {
            let vals = v.iter().map(|x| d(x)).collect::<Vec<_>>();
            let got = nice(
                &vals,
                kind,
                "auto",
                "meaningful",
                t.map(d),
                m.map(d),
                &[d("1"), d("2"), d("5")],
                false,
                5,
                None,
                None,
                None,
                None,
                d("0.15"),
                false,
            )
            .unwrap();
            assert_eq!((got.lo, got.hi, got.step), (d(lo), d(hi), d(step)), "{v:?}");
        }
    }
    #[test]
    fn rainfall() {
        let x = nice(
            &[d("95")],
            "bar",
            "auto",
            "meaningful",
            None,
            None,
            &[d("1"), d("2"), d("5")],
            false,
            5,
            None,
            None,
            None,
            None,
            d("0.15"),
            false,
        )
        .unwrap();
        assert_eq!(x.hi, d("100"));
    }
    #[test]
    fn negative_mirror() {
        let x = nice(
            &[d("-95")],
            "bar",
            "auto",
            "meaningful",
            None,
            None,
            &[d("1"), d("2"), d("5")],
            false,
            5,
            None,
            None,
            None,
            None,
            d("0.15"),
            false,
        )
        .unwrap();
        assert_eq!(x.lo, d("-100"));
        assert_eq!(x.hi, d("0"));
    }
}

/// A bumps chart's own rank scale, 1 … N by 1 (the original's bumps.rank_scale): the ranks
/// given (`measure.kind` rank) or the values ranked each period, the largest first (the
/// smallest where `measure.good` is lower), tied values sharing a rank.
fn rank_scale(plan: &SemanticPlan, data: &PreparedData) -> Result<TrainedScale> {
    let kind = plan.spec.get("/data/measure/kind").as_str().unwrap_or("");
    let low = plan.spec.get("/data/measure/good").as_str() == Some("lower");
    let mut big = Decimal::ONE;
    for j in 0..data.categories.len() {
        let known: Vec<Decimal> = data
            .series
            .iter()
            .filter_map(|s| s.values.get(j).and_then(|d| d.value))
            .collect();
        for v in &known {
            let r = if kind == "rank" {
                v.trunc()
            } else {
                Decimal::from(
                    known
                        .iter()
                        .filter(|w| if low { *w < v } else { *w > v })
                        .count()
                        + 1,
                )
            };
            big = big.max(r);
        }
    }
    let hi = big.max(Decimal::from(2));
    let n = hi.to_u64().ok_or_else(|| err("invalid rank count"))?;
    Ok(TrainedScale {
        lo: Decimal::ONE,
        hi,
        step: Decimal::ONE,
        ticks: (1..=n).map(Decimal::from).collect(),
        mode: "rank".into(),
    })
}
