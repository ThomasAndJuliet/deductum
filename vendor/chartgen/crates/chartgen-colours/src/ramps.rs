//! Colour that encodes a quantity: ramps, their classes and keys (the original's ramps.py;
//! G4.5, G4.7–G4.14, G4.29–G4.34; R3-8–R3-15). Classes name the style's steps (`seq-k`,
//! `div-m4` … `div-0` … `div-p4`, with `good` or `bad` where the sides are judged); the style
//! paints them.
use crate::src::Src;
use crate::{decision, finding};
use chartgen_model::{Decimal, Finding, RampKeyEntry, RampPlan};
use serde_json::Value;
use std::str::FromStr;

pub const STEPS: usize = 9;
pub const SIDE: usize = 4;
const SKEW_BINS: i64 = 7;
const LOG_SPAN: i64 = 100;
const MAX_CLASSES: usize = 7;
const SIDE_CLASSES: usize = 4;
const NATURAL_CLASSES: usize = 6;
const QUANTILE_CLASSES: usize = 6;

/// A Decimal from a float, as Python's `Decimal(str(x))` writes it.
pub fn d_of(x: f64) -> Decimal {
    Decimal::from_str(&format!("{x}"))
        .or_else(|_| Decimal::from_scientific(&format!("{x:e}")))
        .unwrap_or_default()
}
fn f(x: Decimal) -> f64 {
    x.to_string().parse().unwrap_or(0.)
}
fn pow10(e: i64) -> Decimal {
    if e >= 0 {
        Decimal::from_i128_with_scale(10i128.pow(e as u32), 0)
    } else {
        Decimal::new(1, (-e) as u32)
    }
}
/// The report's form of a limit or step (scales._plain): grouped in threes, a true minus.
pub fn plain(x: Decimal) -> String {
    let x = x.normalize();
    let s = x.abs().to_string();
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut w = whole.to_owned();
    if whole.len() >= 4 {
        let head = match whole.len() % 3 {
            0 => 3,
            k => k,
        };
        w = whole[..head].to_owned();
        let mut i = head;
        while i < whole.len() {
            w.push(',');
            w.push_str(&whole[i..i + 3]);
            i += 3;
        }
    }
    format!(
        "{}{}{}",
        if x.is_sign_negative() && !x.is_zero() {
            "\u{2212}"
        } else {
            ""
        },
        w,
        if frac.is_empty() {
            String::new()
        } else {
            format!(".{frac}")
        }
    )
}

// ---- an unclassed ramp ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Ramp {
    pub kind: String,
    pub lo: Decimal,
    pub hi: Decimal,
    pub d: Option<Decimal>,
    pub good: Option<String>,
    pub ticks: Vec<Decimal>,
    pub step: Option<Decimal>,
    pub labelled: bool,
    pub key_lo: Decimal,
    pub key_hi: Decimal,
}

impl Ramp {
    /// Where v lies in steps from lo: 0 … 9 (a diverging ramp: d at 4.5).
    pub fn u(&self, v: Decimal) -> f64 {
        if self.kind != "diverging" {
            let span = self.hi - self.lo;
            return if span <= Decimal::ZERO {
                0.
            } else {
                STEPS as f64 * f((v - self.lo) / span)
            };
        }
        let d = self.d.unwrap_or_default();
        let half = STEPS as f64 / 2.;
        if v < d {
            if d <= self.lo {
                0.
            } else {
                half * f((v - self.lo) / (d - self.lo))
            }
        } else if self.hi <= d {
            half
        } else {
            half + half * f((v - d) / (self.hi - d))
        }
    }
    pub fn index(&self, v: Decimal) -> usize {
        ((self.u(v) + 1e-9).floor().max(0.) as usize).min(STEPS - 1)
    }
    pub fn classes(&self, v: Decimal) -> Vec<String> {
        self.step_classes(self.index(v))
    }
    pub fn step_classes(&self, k: usize) -> Vec<String> {
        if self.kind == "categorical" {
            return vec![format!("series-{}", k + 1)];
        }
        let mut out = if self.kind == "sequential" {
            vec![format!("seq-{}", k + 1)]
        } else if k == SIDE {
            vec!["div-0".into()]
        } else if k < SIDE {
            vec![format!("div-m{}", SIDE - k)]
        } else {
            vec![format!("div-p{}", k - SIDE)]
        };
        if self.kind == "diverging" && k != SIDE {
            if let Some(good) = &self.good {
                out.push(
                    if (k > SIDE) == (good == "higher") {
                        "good"
                    } else {
                        "bad"
                    }
                    .into(),
                );
            }
        }
        if self.labelled {
            out.push("labelled".into());
        }
        out
    }
    /// The value at u steps from lo (the inverse of u).
    pub fn value_at(&self, u: usize) -> Decimal {
        let u = Decimal::from(u);
        if self.kind != "diverging" {
            return self.lo + (self.hi - self.lo) * u / Decimal::from(STEPS);
        }
        let d = self.d.unwrap_or_default();
        let half = Decimal::from(STEPS) / Decimal::TWO;
        if u <= half {
            self.lo + (d - self.lo) * u / half
        } else {
            d + (self.hi - d) * (u - half) / half
        }
    }
    /// The steps the key shows, low to high, cut at its extent.
    pub fn bins(&self) -> Vec<(Decimal, Decimal, Vec<String>)> {
        (0..STEPS)
            .filter_map(|k| {
                let a = self.value_at(k).max(self.key_lo);
                let b = self.value_at(k + 1).min(self.key_hi);
                (b > a).then(|| (a, b, self.step_classes(k)))
            })
            .collect()
    }
}

/// The step of 1, 2 or 5 × 10^n giving about `target` intervals (R2-09).
pub fn nice_step(spread: Decimal, target: i64) -> Decimal {
    if spread <= Decimal::ZERO {
        return Decimal::ONE;
    }
    let raw = spread / Decimal::from(target);
    let e = pow10(f(raw).log10().floor() as i64);
    for m in [1, 2, 5, 10] {
        if Decimal::from(m) * e >= raw {
            return Decimal::from(m) * e;
        }
    }
    Decimal::TEN * e
}

/// (lo, hi, step, ticks): a key's limits on the step's multiples (ramps.nice_domain).
pub fn nice_domain(
    vmin: Decimal,
    vmax: Decimal,
    zero_floor: bool,
    t: Option<Decimal>,
    m: Option<Decimal>,
    d: Option<Decimal>,
    target: i64,
) -> (Decimal, Decimal, Decimal, Vec<Decimal>) {
    let (lo, hi, s);
    if let Some(d) = d {
        let mut r = (d - vmin).max(vmax - d).max(Decimal::ZERO);
        if r.is_zero() {
            r = if d.is_zero() { Decimal::ONE } else { d.abs() };
        }
        s = nice_step(Decimal::TWO * r, target);
        r = (r / s).ceil() * s;
        lo = d - r;
        hi = d + r;
    } else {
        let floor = (zero_floor && vmin >= Decimal::ZERO).then_some(Decimal::ZERO);
        let base = floor.unwrap_or(vmin);
        s = nice_step(
            if vmax > base {
                vmax - base
            } else if vmax.is_zero() {
                Decimal::ONE
            } else {
                vmax.abs()
            },
            target,
        );
        let mut l = floor.unwrap_or_else(|| (vmin / s).floor() * s);
        let mut h = (vmax / s).ceil() * s;
        if h == l {
            h = l + s;
        }
        if let Some(t) = t {
            if l < t && t <= vmin {
                l = t;
            }
        }
        if let Some(m) = m {
            if vmax <= m && m < h {
                h = m;
            }
        }
        lo = l;
        hi = h;
    }
    let mut ticks = vec![];
    let mut t = if d.is_some() { lo } else { (lo / s).ceil() * s };
    while t <= hi {
        ticks.push(t);
        t += s;
    }
    if ticks.first() != Some(&lo) {
        ticks.insert(0, lo);
    }
    if ticks.last() != Some(&hi) {
        ticks.push(hi);
    }
    (lo, hi, s, ticks)
}

// ---- the decision ---------------------------------------------------------------------------

/// What a ramp decision adds to the report.
#[derive(Debug, Default)]
pub struct Notes {
    pub findings: Vec<Finding>,
    pub decisions: Vec<Value>,
    pub overrides: Vec<Value>,
    pub refused: bool,
}
impl Notes {
    fn decide(&mut self, rule: &str, level: &str, text: impl Into<String>) {
        self.decisions.push(decision(rule, level, text));
    }
    /// rules.guided: a departure from the procedure's recommendation.
    fn guided(
        &mut self,
        src: &Src,
        rule: &str,
        path: &str,
        chosen: &str,
        rec: &str,
        why: &str,
    ) -> bool {
        if chosen == rec {
            return false;
        }
        let reason = src.reasons(path);
        let mut f = finding(
            rule,
            if reason.is_some() { "info" } else { "warn" },
            "guided",
            format!("{path} {chosen} departs from the procedure, which gives {rec} ({why})"),
            Some(path.into()),
        );
        f.reason = reason.map(str::to_owned);
        f.recommended = Some(rec.into());
        self.findings.push(f);
        true
    }
    /// rules.refusal: an error unless overridden (R0-04) or for analysis (R0-02).
    fn refusal(&mut self, src: &Src, rule: &str, message: &str, path: &str, supply: &str) -> bool {
        let mut f = finding(rule, "error", "locked", message, Some(path.into()));
        f.supply = Some(supply.into());
        if let Some(reason) = src.overridden(rule) {
            f.severity = "overridden".into();
            f.reason = Some(reason.into());
            self.overrides
                .push(serde_json::json!({"rule": rule, "reason": reason}));
            self.findings.push(f);
            return false;
        }
        if src.analysis() {
            f.severity = "warn".into();
            self.findings.push(f);
            return false;
        }
        self.findings.push(f);
        self.refused = true;
        true
    }
}

fn number(v: &Value) -> Option<Decimal> {
    v.as_str()
        .and_then(|s| Decimal::from_str(s).ok())
        .or_else(|| Decimal::from_str(&v.to_string()).ok())
        .filter(|_| v.is_number())
}

/// The dividing value the audience sees, a number, or None (ramps.dividing).
pub fn dividing(src: &Src, notes: &mut Notes) -> Option<Decimal> {
    let d = src.get("/data/measure/dividing_point");
    if d.is_null() {
        return None;
    }
    if d.is_object() {
        notes.findings.push(finding(
            "CG-3",
            "info",
            "locked",
            "a dividing value that is a comparison per category is not drawn on a colour scale: give one number",
            Some("data.measure.dividing_point".into()),
        ));
        return None;
    }
    match number(d) {
        Some(x) => Some(x),
        None => {
            notes.findings.push(finding(
                "CG-4",
                "error",
                "locked",
                "a number, or {\"comparison\": id}",
                Some("data.measure.dividing_point".into()),
            ));
            None
        }
    }
}

/// The ramp for `values`, by the procedure of G4.5, G4.10 and G4.11 (ramps.choose); None when
/// refused.
pub fn choose(
    src: &Src,
    notes: &mut Notes,
    values: &[Decimal],
    labelled: bool,
    nice: bool,
    what: &str,
) -> Option<Ramp> {
    let d = dividing(src, notes);
    let asked = src.string("/chart/encoding", "auto");
    let rec = if d.is_some() {
        "diverging"
    } else {
        "sequential"
    };
    let why = match d {
        Some(d) => format!(
            "the audience sees a dividing value, {}: two ramps meeting at a neutral there (G4.11, G4.12)",
            plain(d)
        ),
        None => "no dividing value the audience sees: one ramp, darker meaning more, never diverging about the middle of the data (G4.10, D-37 (a))".into(),
    };
    let mut kind = if asked == "auto" { rec } else { asked };
    if asked == "auto" {
        notes.decide(
            if d.is_some() { "R3-13" } else { "R3-12" },
            "guided",
            format!("{what}: a {rec} palette; {why}"),
        );
    } else if asked == "diverging" && d.is_none() {
        // the refusal (R3-13), its override or its analysis warning are the policy's
        // (chartgen-policy marks::layout_refusals, which runs first): a chart drawn past it is
        // sequential
        kind = "sequential";
    }
    // a categorical palette's refusal (R3-9) and a departure's guided finding (R3-12, R3-13)
    // are recorded by the policy too
    let _ = &why;
    let known: Vec<Decimal> = if values.is_empty() {
        vec![Decimal::ZERO]
    } else {
        values.to_vec()
    };
    let vmin = *known.iter().min().unwrap();
    let vmax = *known.iter().max().unwrap();
    let m_good = src.get("/data/measure/good").as_str().map(str::to_owned);
    let m_min = number(src.get("/data/measure/min"));
    let m_max = number(src.get("/data/measure/max"));
    let zero_meaningful = src.string("/data/measure/zero", "meaningful") == "meaningful";
    let diverging = kind == "diverging";
    let good = if diverging { m_good.clone() } else { None };
    let (lo, hi, s, ticks);
    if nice {
        let (a, b, c, t) = nice_domain(
            vmin,
            vmax,
            zero_meaningful && !diverging,
            m_min,
            m_max,
            if diverging { d } else { None },
            if diverging { 5 } else { 8 },
        );
        lo = a;
        hi = b;
        s = Some(c);
        ticks = t;
    } else if diverging {
        let dd = d.unwrap_or_default();
        let mut r = (dd - vmin).max(vmax - dd);
        if r.is_zero() {
            r = Decimal::ONE;
        }
        lo = dd - r;
        hi = dd + r;
        s = None;
        ticks = vec![dd - r, dd, dd + r];
    } else {
        lo = vmin;
        hi = vmax;
        s = None;
        ticks = vec![vmin, vmax];
    }
    let key_lo = m_min.map(|m| lo.max(m)).unwrap_or(lo);
    let key_hi = m_max.map(|m| hi.min(m)).unwrap_or(hi);
    let mut t: Vec<Decimal> = ticks
        .into_iter()
        .filter(|t| key_lo <= *t && *t <= key_hi)
        .collect();
    if t.first() != Some(&key_lo) {
        t.insert(0, key_lo);
    }
    if t.last() != Some(&key_hi) {
        t.push(key_hi);
    }
    let ramp = Ramp {
        kind: kind.into(),
        lo,
        hi,
        d: if diverging { d } else { None },
        good,
        ticks: t,
        step: s,
        labelled,
        key_lo,
        key_hi,
    };
    if diverging && m_good.is_none() {
        notes.decide(
            "R3-18",
            "locked",
            format!(
                "{what}: the sides are not judged good or bad (no measure.good): blue below {}, orange above, the pair safe for colour-vision deficiency and free of red (G4.17, G4.18)",
                plain(d.unwrap_or_default())
            ),
        );
    }
    Some(ramp)
}

/// The sequential steps (1-based) for n ordered categories, lightest first, over steps 3–9
/// (ramps.ordered); None for more than seven.
pub fn ordered(n: usize) -> Option<Vec<usize>> {
    match n {
        0 => Some(vec![]),
        1 => Some(vec![6]),
        n if n > 7 => None,
        n => Some(
            (0..n)
                .map(|j| (3. + 6. * j as f64 / (n - 1) as f64 + 0.5).floor() as usize)
                .collect(),
        ),
    }
}

/// The classes of an action dot's status (ramps.action_classes); None for no dot.
pub fn action_classes(level: Option<&str>) -> Option<Vec<String>> {
    let (side, k) = match level? {
        "good_strong" => ("good", 4),
        "good" => ("good", 2),
        "bad" => ("bad", 2),
        "bad_strong" => ("bad", 4),
        _ => return None,
    };
    Some(vec![
        "action-dot".into(),
        side.into(),
        if side == "good" {
            format!("div-p{k}")
        } else {
            format!("div-m{k}")
        },
    ])
}

// ---- cutting a ramp into classes (G4.29–G4.34) -------------------------------------------------

/// The q-quantile of xs (linear interpolation between order statistics).
pub fn percentile(xs: &[Decimal], q: Decimal) -> Option<Decimal> {
    let mut s = xs.to_vec();
    s.sort();
    if s.is_empty() {
        return None;
    }
    let pos = (s.len() - 1) as f64 * f(q);
    let i = pos.floor() as usize;
    let frac = d_of(pos - i as f64);
    Some(if i + 1 >= s.len() {
        s[i]
    } else {
        s[i] + (s[i + 1] - s[i]) * frac
    })
}

fn round_sig(v: Decimal, sig: i64) -> Decimal {
    if v.is_zero() {
        return v;
    }
    let e = f(v.abs()).log10().floor() as i64 - sig + 1;
    let q = pow10(e);
    (v / q).round() * q
}

/// A classed ramp (ramps.Classes).
#[derive(Debug, Clone, PartialEq)]
pub struct Classes {
    pub kind: String,
    pub method: String,
    pub inner: Vec<Decimal>,
    pub lo: Option<Decimal>,
    pub hi: Option<Decimal>,
    pub zero: bool,
    pub d: Option<Decimal>,
    pub good: Option<String>,
    pub labelled: bool,
    pub key_lo: Option<Decimal>,
    pub key_hi: Option<Decimal>,
    pub below: Option<Decimal>,
    pub above: Option<Decimal>,
}

impl Classes {
    pub fn n(&self) -> usize {
        self.inner.len() + 1
    }
    fn spread(count: usize, top: usize) -> Vec<usize> {
        if count <= 1 {
            return if top == 4 { vec![top] } else { vec![5] };
        }
        (0..count)
            .map(|j| (1. + (top - 1) as f64 * j as f64 / (count - 1) as f64 + 0.5).floor() as usize)
            .collect()
    }
    fn off(&self) -> usize {
        usize::from(self.zero) + usize::from(self.below.is_some())
    }
    pub fn steps(&self) -> Vec<usize> {
        if self.kind == "diverging" {
            return Self::spread(self.n(), 4);
        }
        Self::spread(self.n() + self.off() + usize::from(self.above.is_some()), 9)
    }
    fn lab(&self) -> Vec<String> {
        if self.labelled {
            vec!["labelled".into()]
        } else {
            vec![]
        }
    }
    /// A censored value's classes: the step below the lowest measured class, or above the
    /// highest; None where the ramp has none.
    pub fn censored_classes(&self, status: &str) -> Option<Vec<String>> {
        if self.kind == "diverging" {
            return None;
        }
        let st = self.steps();
        let mut out = match status {
            "below" if self.below.is_some() => {
                vec![format!("seq-{}", st[if self.zero { 1 } else { 0 }])]
            }
            "above" if self.above.is_some() => vec![format!("seq-{}", st[st.len() - 1])],
            _ => return None,
        };
        out.extend(self.lab());
        Some(out)
    }
    /// The class of v: -1 for zero's (or the dividing value's) own class, else 0 … n-1.
    pub fn index(&self, v: Decimal) -> i64 {
        let x = if self.kind == "diverging" {
            (v - self.d.unwrap_or_default()).abs()
        } else {
            v
        };
        if self.zero && x.is_zero() {
            return -1;
        }
        self.inner.iter().filter(|b| **b <= x).count() as i64
    }
    pub fn side(&self, v: Decimal) -> Option<&'static str> {
        let d = self.d?;
        if self.kind != "diverging" || v == d {
            return None;
        }
        let good = self.good.as_deref()?;
        Some(if (v > d) == (good == "higher") {
            "good"
        } else {
            "bad"
        })
    }
    pub fn classes(&self, v: Decimal) -> Vec<String> {
        let k = self.index(v);
        let mut out;
        if self.kind == "diverging" {
            if k < 0 {
                out = vec!["div-0".into()];
            } else {
                let step = self.steps()[k as usize];
                out = vec![if v > self.d.unwrap_or_default() {
                    format!("div-p{step}")
                } else {
                    format!("div-m{step}")
                }];
                if let Some(s) = self.side(v) {
                    out.push(s.into());
                }
            }
        } else {
            let st = self.steps();
            out = vec![format!("seq-{}", st[(k + self.off() as i64) as usize])];
        }
        out.extend(self.lab());
        out
    }
    /// The key's entries in drawing order, and where a diverging key's tick stands.
    pub fn key(&self) -> (Vec<RampKeyEntry>, Option<f64>) {
        let lab = self.lab();
        let with = |mut c: Vec<String>| {
            c.extend(lab.iter().cloned());
            c
        };
        let st = self.steps();
        if self.kind != "diverging" {
            let off = self.off();
            let mut out = vec![];
            if self.zero {
                out.push(RampKeyEntry {
                    classes: with(vec![format!("seq-{}", st[0])]),
                    single: Some(Decimal::ZERO),
                    ..Default::default()
                });
            }
            if let Some(b) = self.below {
                out.push(RampKeyEntry {
                    classes: with(vec![format!("seq-{}", st[off - 1])]),
                    censored: Some(("below".into(), b)),
                    ..Default::default()
                });
            }
            let first = match self.below {
                Some(b) if self.lo.is_none_or(|lo| b > lo) => Some(b),
                _ => {
                    if self.zero {
                        None
                    } else {
                        self.lo
                    }
                }
            };
            let mut edges = vec![first];
            edges.extend(self.inner.iter().map(|b| Some(*b)));
            edges.push(self.hi);
            for k in 0..self.n() {
                out.push(RampKeyEntry {
                    classes: with(vec![format!("seq-{}", st[k + off])]),
                    lo: edges[k],
                    hi: edges[k + 1],
                    ..Default::default()
                });
            }
            if let Some(a) = self.above {
                out.push(RampKeyEntry {
                    classes: with(vec![format!("seq-{}", st[st.len() - 1])]),
                    censored: Some(("above".into(), a)),
                    ..Default::default()
                });
            }
            return (out, None);
        }
        let d = self.d.unwrap_or_default();
        let mut far: Vec<Option<Decimal>> = self.inner.iter().map(|b| Some(*b)).collect();
        far.push(self.hi);
        let mut near = vec![Decimal::ZERO];
        near.extend(self.inner.iter().copied());
        let mut below: Vec<RampKeyEntry> = vec![];
        let mut above: Vec<RampKeyEntry> = vec![];
        let side_of = |higher_is: &str| -> Vec<String> {
            match self.good.as_deref() {
                Some(g) => vec![if (g == "higher") == (higher_is == "higher") {
                    "good".into()
                } else {
                    "bad".into()
                }],
                None => vec![],
            }
        };
        for k in 0..self.n() {
            let (a, b) = (near[k], far[k]);
            let mut lo_v = b.map(|b| d - b);
            let hi_v = d - a;
            if self.key_lo.is_none_or(|kl| hi_v > kl) {
                if let Some(kl) = self.key_lo {
                    lo_v = lo_v.map(|l| l.max(kl));
                }
                let mut cls = vec![format!("div-m{}", st[k])];
                cls.extend(side_of("lower"));
                below.insert(
                    0,
                    RampKeyEntry {
                        classes: with(cls),
                        lo: lo_v,
                        hi: Some(hi_v),
                        ..Default::default()
                    },
                );
            }
            let lo_v = d + a;
            let mut hi_v = b.map(|b| d + b);
            if self.key_hi.is_none_or(|kh| lo_v < kh) {
                if let Some(kh) = self.key_hi {
                    hi_v = hi_v.map(|h| h.min(kh));
                }
                let mut cls = vec![format!("div-p{}", st[k])];
                cls.extend(side_of("higher"));
                above.push(RampKeyEntry {
                    classes: with(cls),
                    lo: Some(lo_v),
                    hi: hi_v,
                    ..Default::default()
                });
            }
        }
        let mut mid = vec![];
        if self.zero {
            mid.push(RampKeyEntry {
                classes: with(vec!["div-0".into()]),
                single: Some(d),
                ..Default::default()
            });
            if let Some(last) = below.last_mut() {
                last.hi = None;
            }
            if let Some(first) = above.first_mut() {
                first.lo = None;
            }
        }
        let tick = if self.zero {
            below.len() as f64
        } else {
            below.len() as f64 - 0.5
        };
        let mut entries = below;
        entries.extend(mid);
        entries.extend(above);
        (entries, Some(tick))
    }
}

fn nice_steps(span: Decimal, limit: usize) -> impl Iterator<Item = Decimal> {
    let start = if span <= Decimal::ZERO {
        None
    } else {
        Some(f(span / Decimal::from(limit)).log10().floor() as i64)
    };
    let mut e = start.unwrap_or(0);
    let mut m = 0usize;
    let mut done = false;
    std::iter::from_fn(move || {
        if start.is_none() {
            if done {
                return None;
            }
            done = true;
            return Some(Decimal::ONE);
        }
        let v = Decimal::from([1, 2, 5][m]) * pow10(e);
        m += 1;
        if m == 3 {
            m = 0;
            e += 1;
        }
        Some(v)
    })
}

fn linear(
    values: &[Decimal],
    zero_floor: bool,
    limit: usize,
    lo_limit: Option<Decimal>,
    hi_limit: Option<Decimal>,
) -> (Decimal, Vec<Decimal>, Decimal) {
    let vmin = *values.iter().min().unwrap();
    let vmax = *values.iter().max().unwrap();
    let base = if zero_floor { Decimal::ZERO } else { vmin };
    let span = if vmax > base {
        vmax - base
    } else if vmax.is_zero() {
        Decimal::ONE
    } else {
        vmax.abs()
    };
    let (mut lo, mut hi, mut s) = (Decimal::ZERO, Decimal::ONE, Decimal::ONE);
    for step in nice_steps(span, limit) {
        s = step;
        lo = if zero_floor {
            Decimal::ZERO
        } else {
            (vmin / s).floor() * s
        };
        hi = (vmax / s).ceil() * s;
        if hi <= lo {
            hi = lo + s;
        }
        if (hi - lo) / s <= Decimal::from(limit) {
            break;
        }
    }
    if let Some(l) = lo_limit {
        if lo < l && l <= vmin {
            lo = l;
        }
    }
    let mut inner = vec![];
    let mut b = (lo / s).floor() * s + s;
    while b < hi {
        if b > lo {
            inner.push(b);
        }
        b += s;
    }
    if let Some(h) = hi_limit {
        if vmax <= h && h < hi {
            hi = h;
        }
    }
    (lo, inner, hi)
}

/// The share of the values in the fullest of seven equal steps (ramps.skew_share).
pub fn skew_share(values: &[Decimal], zero: bool) -> Decimal {
    if values.is_empty() {
        return Decimal::ZERO;
    }
    let lo = if zero {
        Decimal::ZERO
    } else {
        *values.iter().min().unwrap()
    };
    let hi = *values.iter().max().unwrap();
    if hi <= lo {
        return Decimal::ZERO;
    }
    let w = (hi - lo) / Decimal::from(SKEW_BINS);
    let mut counts = [0usize; SKEW_BINS as usize];
    for v in values {
        let k = ((*v - lo) / w).trunc();
        let k = i64::try_from(k).unwrap_or(0).clamp(0, SKEW_BINS - 1) as usize;
        counts[k] += 1;
    }
    Decimal::from(*counts.iter().max().unwrap()) / Decimal::from(values.len())
}

fn log_breaks(values: &[Decimal], limit: usize) -> (Vec<Decimal>, bool) {
    let pos: Vec<Decimal> = values
        .iter()
        .copied()
        .filter(|v| *v > Decimal::ZERO)
        .collect();
    let lo = *pos.iter().min().unwrap();
    let hi = *pos.iter().max().unwrap();
    let e0 = f(lo).log10().floor() as i64 - 1;
    let e1 = f(hi).log10().ceil() as i64 + 1;
    let series: [(&[i64], i64); 4] = [(&[1, 2, 5], 1), (&[1], 1), (&[1], 2), (&[1], 3)];
    let mut inner = vec![];
    for (mults, every) in series {
        inner = vec![];
        for e in e0..=e1 {
            if (e - e0) % every != 0 {
                continue;
            }
            for &m in mults {
                let b = Decimal::from(m) * pow10(e);
                if lo < b && b <= hi {
                    inner.push(b);
                }
            }
        }
        if inner.len() + 1 <= limit {
            let open = inner.last().is_none_or(|l| hi > *l);
            return (inner, open);
        }
    }
    (inner, true)
}

fn quantile_breaks(values: &[Decimal], k: usize) -> Vec<Decimal> {
    let min = *values.iter().min().unwrap();
    let mut inner: Vec<Decimal> = vec![];
    for j in 1..k {
        let q = Decimal::from(j) / Decimal::from(k);
        let Some(p) = percentile(values, q) else {
            continue;
        };
        let b = round_sig(p, 2);
        if b > min && inner.last().is_none_or(|l| b > *l) {
            inner.push(b);
        }
    }
    inner
}

/// Jenks's natural breaks (Fisher's exact optimisation), on the distinct values weighted by
/// their counts (at most 600 groups: beyond, equal-count groups).
fn natural_breaks(values: &[Decimal], k: usize) -> Vec<Decimal> {
    let mut floats: Vec<f64> = values.iter().map(|v| f(*v)).collect();
    floats.sort_by(f64::total_cmp);
    let mut xs: Vec<f64> = vec![];
    let mut ws: Vec<f64> = vec![];
    for x in &floats {
        if xs.last() == Some(x) {
            *ws.last_mut().unwrap() += 1.;
        } else {
            xs.push(*x);
            ws.push(1.);
        }
    }
    if xs.len() > 600 {
        let size = floats.len() as f64 / 600.;
        xs.clear();
        ws.clear();
        for g in 0..600 {
            let a = (g as f64 * size) as usize;
            let b = ((g + 1) as f64 * size) as usize;
            let run = &floats[a..b.min(floats.len())];
            if !run.is_empty() {
                xs.push(run.iter().sum::<f64>() / run.len() as f64);
                ws.push(run.len() as f64);
            }
        }
    }
    let n = xs.len();
    let k = k.min(n);
    if k < 2 {
        return vec![];
    }
    let mut w = vec![0.; n + 1];
    let mut s = vec![0.; n + 1];
    let mut q = vec![0.; n + 1];
    for i in 0..n {
        w[i + 1] = w[i] + ws[i];
        s[i + 1] = s[i] + ws[i] * xs[i];
        q[i + 1] = q[i] + ws[i] * xs[i] * xs[i];
    }
    let ssd = |i: usize, j: usize| {
        let ww = w[j] - w[i];
        let ss = s[j] - s[i];
        q[j] - q[i] - ss * ss / ww
    };
    let inf = f64::INFINITY;
    let mut cost = vec![vec![inf; n + 1]; k + 1];
    let mut back = vec![vec![0usize; n + 1]; k + 1];
    cost[0][0] = 0.;
    for c in 1..=k {
        for j in c..=n {
            let (mut best, mut arg) = (inf, 0);
            for i in (c - 1)..j {
                let v = cost[c - 1][i];
                if v == inf {
                    continue;
                }
                let v = v + ssd(i, j);
                if v < best {
                    best = v;
                    arg = i;
                }
            }
            cost[c][j] = best;
            back[c][j] = arg;
        }
    }
    let mut cuts = vec![];
    let mut j = n;
    for c in (1..=k).rev() {
        let i = back[c][j];
        cuts.push(i);
        j = i;
    }
    cuts.sort();
    let mut inner: Vec<Decimal> = vec![];
    for &i in &cuts[1..] {
        let b = round_sig(d_of((xs[i - 1] + xs[i]) / 2.), 2);
        if inner.last().is_none_or(|l| b > *l) {
            inner.push(b);
        }
    }
    inner
}

/// How many times the 95th percentile of the positive values exceeds their 5th (None for fewer
/// than ten).
pub fn log_span(positive: &[Decimal]) -> Option<Decimal> {
    if positive.len() < 10 {
        return None;
    }
    let lo = percentile(positive, Decimal::new(5, 2))?;
    let hi = percentile(positive, Decimal::new(95, 2))?;
    (lo > Decimal::ZERO).then(|| hi / lo)
}

/// A classed ramp, or an unclassed one for a categorical palette (through an override).
#[derive(Debug, Clone, PartialEq)]
pub enum Classed {
    Classes(Classes),
    Ramp(Ramp),
}

/// The steps of a heatmap's colours (ramps.classify; G4.29–G4.33).
pub fn classify(
    src: &Src,
    notes: &mut Notes,
    values: &[Decimal],
    labelled: bool,
    what: &str,
) -> Option<Classed> {
    let base = choose(src, notes, values, labelled, true, what)?;
    if base.kind == "categorical" {
        return Some(Classed::Ramp(base));
    }
    let known: Vec<Decimal> = if values.is_empty() {
        vec![Decimal::ZERO]
    } else {
        values.to_vec()
    };
    let div = base.kind == "diverging";
    let d = base.d.unwrap_or_default();
    let mags: Vec<Decimal> = if div {
        known.iter().map(|v| (*v - d).abs()).collect()
    } else {
        known.clone()
    };
    let pos: Vec<Decimal> = mags
        .iter()
        .copied()
        .filter(|v| *v > Decimal::ZERO)
        .collect();
    let share = skew_share(&mags, div);
    let skewed = share > Decimal::new(5, 1);
    let pct = format!(
        "{} %",
        (share * Decimal::ONE_HUNDRED + Decimal::new(5, 1)).trunc()
    );
    let focus = src.get("/facts/ramp_focus").as_str();
    let m_zero = src.string("/data/measure/zero", "meaningful");
    let kmin = *known.iter().min().unwrap();
    let (rec, rule, why): (&str, &str, String);
    if !skewed {
        rec = "linear";
        rule = "G4.29";
        why = format!(
            "the {} are fairly even (the fullest of seven equal steps would hold {pct} of them): equal steps on rounded values, the most honest cut",
            if div {
                format!("distances from {}", plain(d))
            } else {
                "values".into()
            }
        );
    } else {
        if focus.is_none() {
            notes.findings.push(finding(
                "G4.30",
                "info",
                "guided",
                format!(
                    "{what}: the values are skewed (one of seven equal steps would hold {pct} of them), and facts.ramp_focus does not say whether the few extremes or the pattern among the many is the message: the pattern assumed (logarithmic steps over two orders of magnitude, else natural breaks); give extremes for a linear cut (D-1)"
                ),
                Some("facts.ramp_focus".into()),
            ));
        }
        let ratio_scaled = !div && m_zero == "meaningful" && kmin >= Decimal::ZERO;
        let span = log_span(&pos);
        if focus == Some("extremes") {
            rec = "linear";
            rule = "G4.29";
            why = format!(
                "the values are skewed (one of seven equal steps would hold {pct} of them) and their few extremes are the message (facts.ramp_focus): equal steps on rounded values"
            );
        } else if ratio_scaled && span.is_some_and(|s| s >= Decimal::from(LOG_SPAN)) {
            rec = "log";
            rule = "G4.33";
            why = format!(
                "the values are skewed (one of seven equal steps would hold {pct} of them), the pattern among the many is the message{}, and these amounts span {} times from the 5th percentile of the positive values to the 95th, at least {LOG_SPAN} (owner, 1 Oct 2026): logarithmic 1–2–5 steps, each a readable ratio, zero a step of its own",
                if focus.is_some() {
                    " (facts.ramp_focus)"
                } else {
                    ""
                },
                plain(span.unwrap().round_dp(0))
            );
        } else {
            rec = "natural";
            rule = "G4.30";
            why = format!(
                "the values are skewed (one of seven equal steps would hold {pct} of them) and the pattern among the many is the message{}{}: natural breaks, rounded for the key, the extremes in small steps of their own",
                if focus.is_some() {
                    " (facts.ramp_focus)"
                } else {
                    ""
                },
                match span {
                    Some(s) if ratio_scaled => format!(
                        ", within {} times from the 5th percentile to the 95th",
                        plain(s.round_dp(1))
                    ),
                    _ => String::new(),
                }
            );
        }
    }
    let mut asked = src.string("/chart/classing", "auto");
    let custom: Vec<Decimal> = src
        .get("/chart/breaks")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(number)
        .collect();
    if asked == "auto" && !custom.is_empty() {
        asked = "custom";
    }
    let mut method = if asked == "auto" { rec } else { asked };
    if asked == "auto" {
        notes.decide(rule, "guided", format!("{what}: {why}"));
    } else {
        let rule = match asked {
            "linear" => "G4.29",
            "natural" => "G4.30",
            "quantile" => "G4.31",
            "custom" => "G4.32",
            _ => "G4.33",
        };
        notes.guided(src, rule, "chart.classing", asked, rec, &why);
    }
    if method == "log"
        && (pos.is_empty() || (!div && kmin < Decimal::ZERO) || m_zero == "arbitrary")
    {
        notes.findings.push(finding(
            "G4.33",
            "warn",
            "guided",
            "logarithmic steps are for ratio-scaled amounts above zero (counts, rates, sizes), not for scores or values below zero: equal steps instead",
            Some("chart.classing".into()),
        ));
        method = "linear";
    }
    if method == "custom" && custom.is_empty() {
        notes.findings.push(finding(
            "CG-4",
            "error",
            "locked",
            "custom steps need chart.breaks",
            Some("chart.breaks".into()),
        ));
        notes.refused = true;
        return None;
    }
    let limit = if div { SIDE_CLASSES } else { MAX_CLASSES };
    let zero = mags.iter().any(|v| v.is_zero()) && (div || method == "log");
    let m_min = number(src.get("/data/measure/min"));
    let m_max = number(src.get("/data/measure/max"));
    let (lo, inner, hi): (Option<Decimal>, Vec<Decimal>, Option<Decimal>);
    match method {
        "linear" => {
            let (a, b, c) = if div {
                linear(&mags, true, limit, None, None)
            } else {
                let zero_floor = m_zero == "meaningful" && kmin >= Decimal::ZERO;
                linear(&known, zero_floor, limit, m_min, m_max)
            };
            lo = Some(a);
            inner = b;
            hi = Some(c);
        }
        "log" => {
            let (b, open) = log_breaks(&pos, limit - if div { 0 } else { usize::from(zero) });
            inner = b;
            lo = Some(Decimal::ZERO);
            hi = if div || !open {
                pos.iter().max().copied()
            } else {
                None
            };
        }
        "custom" => {
            let smin = *mags.iter().min().unwrap();
            let smax = *mags.iter().max().unwrap();
            let mut b: Vec<Decimal> = custom
                .into_iter()
                .filter(|b| smin < *b && *b < smax)
                .collect();
            b.sort();
            b.dedup();
            inner = b;
            lo = Some(if div { Decimal::ZERO } else { kmin });
            hi = None;
        }
        _ => {
            let srcv: Vec<Decimal> = if div {
                mags.iter()
                    .copied()
                    .filter(|v| *v > Decimal::ZERO)
                    .collect()
            } else {
                known.clone()
            };
            let k = if div {
                SIDE_CLASSES
            } else if method == "quantile" {
                QUANTILE_CLASSES
            } else {
                NATURAL_CLASSES
            };
            inner = if srcv.is_empty() {
                vec![]
            } else if method == "quantile" {
                quantile_breaks(&srcv, k)
            } else {
                natural_breaks(&srcv, k)
            };
            lo = Some(if div { Decimal::ZERO } else { kmin });
            hi = None;
        }
    }
    let good = if div {
        src.get("/data/measure/good").as_str().map(str::to_owned)
    } else {
        None
    };
    Some(Classed::Classes(Classes {
        kind: if div { "diverging" } else { "sequential" }.into(),
        method: method.into(),
        inner: inner.into_iter().map(|b| b.normalize()).collect(),
        lo,
        hi,
        zero,
        d: div.then_some(d),
        good,
        labelled,
        key_lo: if div { m_min } else { None },
        key_hi: if div { m_max } else { None },
        below: None,
        above: None,
    }))
}

impl Classed {
    pub fn classes(&self, v: Decimal) -> Vec<String> {
        match self {
            Classed::Classes(c) => c.classes(v),
            Classed::Ramp(r) => r.classes(v),
        }
    }
    pub fn kind(&self) -> &str {
        match self {
            Classed::Classes(c) => &c.kind,
            Classed::Ramp(r) => &r.kind,
        }
    }
}

/// The ramp plan of a heatmap's cells (heatmap.py): every known value classed on one ramp;
/// censored values a step of their own beyond the ramp's end (the style's default treatment,
/// `step`). The plan is None when refused; the notes carry findings, decisions and overrides.
pub fn heatmap_notes(src: &Src) -> (Option<RampPlan>, Notes) {
    let mut notes = Notes::default();
    let plan = heatmap_plan(src, &mut notes);
    (plan, notes)
}

fn heatmap_plan(src: &Src, notes: &mut Notes) -> Option<RampPlan> {
    let n = src.n_series();
    let values: Vec<Decimal> = (0..n)
        .flat_map(|i| src.data.series[i].values.iter().filter_map(|d| d.value))
        .collect();
    let mut ramp = classify(src, notes, &values, false, "the cells")?;
    // censored values: the step treatment, the ramp's own steps below and above (heatmap.censored_setup)
    let censored: Vec<(&str, Decimal)> = (0..n)
        .flat_map(|i| src.data.series[i].values.iter())
        .filter(|d| d.value.is_none() && matches!(d.status.as_str(), "below" | "above"))
        .filter_map(|d| d.limit.map(|l| (d.status.as_str(), l)))
        .collect();
    if !censored.is_empty() {
        let classed_seq = matches!(&ramp, Classed::Classes(c) if c.kind != "diverging");
        if !classed_seq
            && notes.refusal(
                src,
                "R0-12",
                &format!(
                    "{} censored values (“<…”, “>…”) on a {} ramp: a step of their own beyond its ends is a later stage; they are never drawn as not known or as zero",
                    censored.len(),
                    ramp.kind()
                ),
                "data.series",
                "a sequential ramp (no measure.dividing_point), or a table",
            )
        {
            return None;
        }
        let lo = censored
            .iter()
            .filter(|(s, _)| *s == "below")
            .map(|(_, l)| *l)
            .max();
        let hi = censored
            .iter()
            .filter(|(s, _)| *s == "above")
            .map(|(_, l)| *l)
            .min();
        if let (Some(lo), Some(min)) = (lo, values.iter().min()) {
            if *min < lo {
                notes.findings.push(finding(
                    "R0-12",
                    "warn",
                    "locked",
                    format!(
                        "a measured value ({}) lies below the detection limit {}: the limits differ, or a value is wrong",
                        min.normalize(),
                        lo.normalize()
                    ),
                    Some("data.series".into()),
                ));
            }
        }
        if let (Some(hi), Some(max)) = (hi, values.iter().max()) {
            if *max > hi {
                notes.findings.push(finding(
                    "R0-12",
                    "warn",
                    "locked",
                    format!(
                        "a measured value ({}) lies above the limit {} of the censored values",
                        max.normalize(),
                        hi.normalize()
                    ),
                    Some("data.series".into()),
                ));
            }
        }
        if let Classed::Classes(c) = &mut ramp {
            c.below = lo;
            c.above = hi;
            let st = c.steps();
            let mut uniq = st.clone();
            uniq.dedup();
            if uniq.len() != st.len() {
                notes.findings.push(finding(
                    "G4.34",
                    "warn",
                    "locked",
                    "the censored values' steps and the ramp's classes are more than its nine steps tell apart",
                    Some("chart.classing".into()),
                ));
            }
        }
    }
    let cells = (0..n)
        .map(|i| {
            src.data.series[i]
                .values
                .iter()
                .map(|d| match d.value {
                    Some(v) => ramp.classes(v),
                    None => match (&ramp, d.status.as_str()) {
                        (Classed::Classes(c), s @ ("below" | "above")) => c
                            .censored_classes(s)
                            .unwrap_or_else(|| vec!["censored".into()]),
                        (_, "below" | "above") => vec!["censored".into()],
                        // a value not known: a grey cell, never the colour of zero (R6-23)
                        _ => vec!["missing".into()],
                    },
                })
                .collect()
        })
        .collect();
    Some(match ramp {
        Classed::Classes(c) => {
            let (key, tick) = c.key();
            RampPlan {
                kind: c.kind.clone(),
                method: c.method.clone(),
                cells,
                key,
                tick,
                censored_below: c.censored_classes("below").unwrap_or_default(),
                censored_above: c.censored_classes("above").unwrap_or_default(),
                inner: c.inner.clone(),
                lo: c.lo,
                hi: c.hi,
                dividing: c.d,
            }
        }
        Classed::Ramp(r) => RampPlan {
            kind: r.kind.clone(),
            method: String::new(),
            cells,
            key: r
                .bins()
                .into_iter()
                .map(|(a, b, c)| RampKeyEntry {
                    classes: c,
                    lo: Some(a),
                    hi: Some(b),
                    ..Default::default()
                })
                .collect(),
            tick: None,
            censored_below: vec![],
            censored_above: vec![],
            inner: vec![],
            lo: Some(r.lo),
            hi: Some(r.hi),
            dividing: r.d,
        },
    })
}

/// A shaded table's cells (table._graphics heat; R10-9): the body, the Total column and the
/// totals row each on its own ramp from its lowest to its highest value (ramps.choose, not on a
/// nice step), the grand total unshaded; the ramp's decision once, for the first group. None when
/// the table is not shaded (`table.graphics` not heat, or values that are not comparable).
pub fn table_heat(src: &Src) -> (Option<RampPlan>, Notes) {
    let mut notes = Notes::default();
    let raw = &src.spec.raw;
    if raw["table"]["graphics"].as_str() != Some("heat") {
        return (None, notes);
    }
    // table.py: comparable unless the message says not, or each row is its own measure
    let measures: Vec<&str> = raw["data"]["measures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str())
        .collect();
    let row_measures: std::collections::BTreeSet<Option<&str>> = (0..src.n_series())
        .map(|i| src.series_raw(i)["measure"].as_str())
        .collect();
    let per_row = row_measures.len() > 1
        && (0..src.n_series()).all(|i| {
            src.series_raw(i)["measure"]
                .as_str()
                .is_some_and(|m| measures.contains(&m))
        });
    if src.string("/facts/message", "") == "not_comparable" || per_row {
        return (None, notes);
    }
    let summary = |i: usize| matches!(src.role(i), Some("total" | "average"));
    let total_col = |j: usize| src.x_role(j) == Some("total");
    let n = src.n_series();
    let cats = src.categories().len();
    let groups: [(&str, Vec<(usize, usize)>); 3] = [
        (
            "body",
            (0..n)
                .filter(|&i| !summary(i))
                .flat_map(|i| (0..cats).filter(|&j| !total_col(j)).map(move |j| (i, j)))
                .collect(),
        ),
        (
            "total column",
            (0..n)
                .filter(|&i| !summary(i))
                .flat_map(|i| (0..cats).filter(|&j| total_col(j)).map(move |j| (i, j)))
                .collect(),
        ),
        (
            "totals row",
            (0..n)
                .filter(|&i| summary(i))
                .flat_map(|i| (0..cats).filter(|&j| !total_col(j)).map(move |j| (i, j)))
                .collect(),
        ),
    ];
    let mut cells = vec![vec![vec![]; cats]; n];
    let mut kind = String::new();
    let mut first = true;
    for (name, members) in &groups {
        let vals: Vec<Decimal> = members
            .iter()
            .filter_map(|&(i, j)| src.value(i, j))
            .collect();
        if vals.is_empty() {
            continue;
        }
        let mut local = Notes::default();
        let Some(ramp) = choose(
            src,
            &mut local,
            &vals,
            false,
            false,
            &format!("the {name}'s cells"),
        ) else {
            notes.findings.extend(local.findings);
            notes.overrides.extend(local.overrides);
            notes.refused = true;
            return (None, notes);
        };
        notes.findings.extend(local.findings);
        notes.overrides.extend(local.overrides);
        if first {
            notes.decisions.extend(local.decisions);
            kind = ramp.kind.clone();
        }
        first = false;
        for &(i, j) in members {
            let d = &src.data.series[i].values[j];
            cells[i][j] = match d.value {
                Some(v) => ramp.classes(v),
                None if matches!(d.status.as_str(), "below" | "above") => {
                    if ramp.kind == "sequential" {
                        ramp.step_classes(if d.status == "below" { 0 } else { STEPS - 1 })
                    } else {
                        notes.findings.push(finding(
                            "CG-3",
                            "info",
                            "locked",
                            "a censored value (“<…”, “>…”) on a diverging shaded table takes the censored values' pale fill, its text in the cell (its side of the dividing value is a later stage)",
                            Some("table.graphics".into()),
                        ));
                        vec!["censored".into()]
                    }
                }
                None => vec!["missing".into()],
            };
        }
    }
    (
        Some(RampPlan {
            kind,
            method: String::new(),
            cells,
            ..RampPlan::default()
        }),
        notes,
    )
}
