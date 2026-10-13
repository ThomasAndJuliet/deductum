//! Basic descriptive statistics, in the spirit of pandas `describe` and Altair's
//! aggregate/bin/density transforms. Every function is pure and deterministic; the
//! bootstrap takes an explicit seed.

use std::cmp::Ordering;

/// How a quantile is interpolated between order statistics (pandas' `interpolation`,
/// plus Weibull's type 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantileMethod {
    /// R type 7, the pandas/NumPy/Altair default.
    Linear,
    /// R type 6 (Weibull), exclusive.
    Weibull,
    Lower,
    Higher,
    Nearest,
    Midpoint,
}

impl QuantileMethod {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "linear" | "type7" => Self::Linear,
            "weibull" | "type6" => Self::Weibull,
            "lower" => Self::Lower,
            "higher" => Self::Higher,
            "nearest" => Self::Nearest,
            "midpoint" => Self::Midpoint,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Weibull => "weibull",
            Self::Lower => "lower",
            Self::Higher => "higher",
            Self::Nearest => "nearest",
            Self::Midpoint => "midpoint",
        }
    }
}

pub fn sorted(values: &[f64]) -> Vec<f64> {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    v
}

/// Quantile `p` (0..=1) of an already sorted, non-empty slice.
pub fn quantile_sorted(s: &[f64], p: f64, method: QuantileMethod) -> f64 {
    let n = s.len();
    assert!(n > 0, "quantile of an empty sample");
    let p = p.clamp(0.0, 1.0);
    // Zero-based fractional index into the order statistics.
    let h = match method {
        QuantileMethod::Weibull => (p * (n as f64 + 1.0) - 1.0).clamp(0.0, (n - 1) as f64),
        _ => p * (n as f64 - 1.0),
    };
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = h - lo as f64;
    match method {
        QuantileMethod::Linear | QuantileMethod::Weibull => s[lo] + frac * (s[hi] - s[lo]),
        QuantileMethod::Lower => s[lo],
        QuantileMethod::Higher => {
            if frac > 0.0 {
                s[hi]
            } else {
                s[lo]
            }
        }
        QuantileMethod::Nearest => {
            if frac > 0.5 || (frac == 0.5 && lo % 2 == 1) {
                s[hi]
            } else {
                s[lo]
            }
        }
        QuantileMethod::Midpoint => {
            if frac > 0.0 {
                (s[lo] + s[hi]) / 2.0
            } else {
                s[lo]
            }
        }
    }
}

pub fn quantile(values: &[f64], p: f64, method: QuantileMethod) -> f64 {
    quantile_sorted(&sorted(values), p, method)
}

pub fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

pub fn median(v: &[f64]) -> f64 {
    quantile(v, 0.5, QuantileMethod::Linear)
}

/// Sample variance (n - 1 denominator, as pandas); `None` for fewer than two values.
pub fn variance(v: &[f64]) -> Option<f64> {
    if v.len() < 2 {
        return None;
    }
    let m = mean(v);
    Some(v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64)
}

pub fn stdev(v: &[f64]) -> Option<f64> {
    variance(v).map(f64::sqrt)
}

pub fn stderr(v: &[f64]) -> Option<f64> {
    stdev(v).map(|s| s / (v.len() as f64).sqrt())
}

pub fn min(v: &[f64]) -> f64 {
    v.iter().cloned().fold(f64::INFINITY, f64::min)
}

pub fn max(v: &[f64]) -> f64 {
    v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
}

/// The statistics pandas' `describe` reports, plus the interquartile range.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub mean: f64,
    pub stdev: Option<f64>,
    pub min: f64,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub max: f64,
}

impl Summary {
    pub fn iqr(&self) -> f64 {
        self.q3 - self.q1
    }
}

pub fn describe(values: &[f64], method: QuantileMethod) -> Option<Summary> {
    if values.is_empty() {
        return None;
    }
    let s = sorted(values);
    Some(Summary {
        count: s.len(),
        mean: mean(&s),
        stdev: stdev(&s),
        min: s[0],
        q1: quantile_sorted(&s, 0.25, method),
        median: quantile_sorted(&s, 0.5, method),
        q3: quantile_sorted(&s, 0.75, method),
        max: s[s.len() - 1],
    })
}

/// Where a box plot's whiskers end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Whiskers {
    /// The most extreme observations within `k` interquartile ranges of the box (k = 1.5 is
    /// Tukey's, and the default in matplotlib, ggplot2 and Altair).
    Tukey(f64),
    MinMax,
    /// Fixed percentiles, such as 5 and 95.
    Percentile(f64, f64),
}

impl Whiskers {
    pub fn describe(self) -> String {
        match self {
            Self::Tukey(k) => {
                format!("whiskers reach the furthest value within {k}\u{d7}IQR of the box")
            }
            Self::MinMax => "whiskers span the minimum and maximum".into(),
            Self::Percentile(a, b) => format!("whiskers mark the {a}th and {b}th percentiles"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoxStats {
    pub n: usize,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub mean: f64,
    pub lower_whisker: f64,
    pub upper_whisker: f64,
    pub outliers: Vec<f64>,
}

pub fn box_stats(values: &[f64], whiskers: Whiskers, method: QuantileMethod) -> Option<BoxStats> {
    let s = sorted(values);
    let n = s.len();
    if n == 0 {
        return None;
    }
    let q1 = quantile_sorted(&s, 0.25, method);
    let median = quantile_sorted(&s, 0.5, method);
    let q3 = quantile_sorted(&s, 0.75, method);
    let (lo_fence, hi_fence) = match whiskers {
        Whiskers::Tukey(k) => (q1 - k * (q3 - q1), q3 + k * (q3 - q1)),
        Whiskers::MinMax => (s[0], s[n - 1]),
        Whiskers::Percentile(a, b) => (
            quantile_sorted(&s, a / 100.0, method),
            quantile_sorted(&s, b / 100.0, method),
        ),
    };
    let inside = |x: &&f64| **x >= lo_fence && **x <= hi_fence;
    let lower_whisker = match whiskers {
        Whiskers::Percentile(..) => lo_fence,
        _ => s
            .iter()
            .filter(inside)
            .cloned()
            .fold(f64::INFINITY, f64::min)
            .min(q1),
    };
    let upper_whisker = match whiskers {
        Whiskers::Percentile(..) => hi_fence,
        _ => s
            .iter()
            .filter(inside)
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max)
            .max(q3),
    };
    let outliers = s
        .iter()
        .cloned()
        .filter(|x| *x < lower_whisker || *x > upper_whisker)
        .collect();
    Some(BoxStats {
        n,
        q1,
        median,
        q3,
        mean: mean(&s),
        lower_whisker,
        upper_whisker,
        outliers,
    })
}

/// The empirical CDF as step points `(x, F(x))`, one per distinct value.
pub fn ecdf(values: &[f64]) -> Vec<(f64, f64)> {
    let s = sorted(values);
    let n = s.len() as f64;
    let mut out: Vec<(f64, f64)> = vec![];
    for (i, x) in s.iter().enumerate() {
        let f = (i + 1) as f64 / n;
        match out.last_mut() {
            Some(last) if last.0 == *x => last.1 = f,
            _ => out.push((*x, f)),
        }
    }
    out
}

/// A histogram with equal-width bins on a "nice" step, as Altair's `bin` transform.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub start: f64,
    pub step: f64,
    pub counts: Vec<usize>,
}

pub fn histogram(values: &[f64], max_bins: usize) -> Option<Histogram> {
    if values.is_empty() || max_bins == 0 {
        return None;
    }
    let (lo, hi) = (min(values), max(values));
    let span = if hi > lo { hi - lo } else { 1.0 };
    let step = nice_step(span / max_bins as f64);
    let start = (lo / step).floor() * step;
    let bins = (((hi - start) / step).floor() as usize + 1).max(1);
    let mut counts = vec![0; bins];
    for x in values {
        let i = (((x - start) / step).floor() as usize).min(bins - 1);
        counts[i] += 1;
    }
    Some(Histogram {
        start,
        step,
        counts,
    })
}

/// The smallest of 1, 2, 5 times a power of ten that is at least `raw`.
pub fn nice_step(raw: f64) -> f64 {
    if !(raw.is_finite() && raw > 0.0) {
        return 1.0;
    }
    let p = 10f64.powf(raw.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * p >= raw * (1.0 - 1e-12) {
            return m * p;
        }
    }
    10.0 * p
}

/// A deterministic generator (SplitMix64); the bootstrap's only source of randomness.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// Which statistic a bootstrap or interval is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Statistic {
    Mean,
    Median,
}

impl Statistic {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "mean" => Some(Self::Mean),
            "median" => Some(Self::Median),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Median => "median",
        }
    }
    pub fn of(self, v: &[f64]) -> f64 {
        match self {
            Self::Mean => mean(v),
            Self::Median => median(v),
        }
    }
}

/// Percentile bootstrap interval for `stat` of `values` at confidence `level` (0..1).
pub fn bootstrap_ci(
    values: &[f64],
    stat: Statistic,
    level: f64,
    resamples: usize,
    seed: u64,
) -> Option<(f64, f64)> {
    if values.len() < 2 || resamples < 2 {
        return None;
    }
    let mut rng = Rng::new(seed);
    let mut sample = vec![0.0; values.len()];
    let mut estimates: Vec<f64> = (0..resamples)
        .map(|_| {
            for slot in sample.iter_mut() {
                *slot = values[rng.below(values.len())];
            }
            stat.of(&sample)
        })
        .collect();
    estimates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let alpha = (1.0 - level) / 2.0;
    Some((
        quantile_sorted(&estimates, alpha, QuantileMethod::Linear),
        quantile_sorted(&estimates, 1.0 - alpha, QuantileMethod::Linear),
    ))
}

/// Standard normal quantile (Acklam's rational approximation, relative error < 1.2e-9).
pub fn normal_ppf(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383_577_518_672_69e2,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    let p_low = 0.02425;
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    if p < p_low {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - p_low {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        -normal_ppf(1.0 - p)
    }
}

/// Wilson score interval for a proportion `successes / trials` at confidence `level`.
/// Defined for `trials > 0`, including 0 and `trials` successes (unlike the Wald interval).
pub fn wilson(successes: u64, trials: u64, level: f64) -> Option<(f64, f64)> {
    if trials == 0 || successes > trials {
        return None;
    }
    let n = trials as f64;
    let p = successes as f64 / n;
    let z = normal_ppf(1.0 - (1.0 - level) / 2.0);
    let z2 = z * z;
    let centre = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
    let half = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / (1.0 + z2 / n);
    Some(((centre - half).max(0.0), (centre + half).min(1.0)))
}

/// Normal-approximation interval for the mean (`z` quantile, so optimistic for small n).
pub fn normal_ci(values: &[f64], level: f64) -> Option<(f64, f64)> {
    let se = stderr(values)?;
    let z = normal_ppf(1.0 - (1.0 - level) / 2.0);
    let m = mean(values);
    Some((m - z * se, m + z * se))
}

/// Sarle's bimodality coefficient; values above 5/9 suggest more than one mode.
pub fn bimodality_coefficient(v: &[f64]) -> Option<f64> {
    let n = v.len() as f64;
    if v.len() < 4 {
        return None;
    }
    let m = mean(v);
    let m2 = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n;
    if m2 == 0.0 {
        return None;
    }
    let m3 = v.iter().map(|x| (x - m).powi(3)).sum::<f64>() / n;
    let m4 = v.iter().map(|x| (x - m).powi(4)).sum::<f64>() / n;
    let skew = m3 / m2.powf(1.5);
    let kurt = m4 / (m2 * m2) - 3.0;
    Some((skew * skew + 1.0) / (kurt + 3.0 * (n - 1.0).powi(2) / ((n - 2.0) * (n - 3.0))))
}

/// Ordinary least squares `y = intercept + slope * x`, with Pearson's r.
pub fn ols(points: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = points.len() as f64;
    if points.len() < 3 {
        return None;
    }
    let (mx, my) = (
        points.iter().map(|p| p.0).sum::<f64>() / n,
        points.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let syy: f64 = points.iter().map(|p| (p.1 - my).powi(2)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    if sxx == 0.0 || syy == 0.0 {
        return None;
    }
    let slope = sxy / sxx;
    Some((my - slope * mx, slope, sxy / (sxx * syy).sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn quantiles_match_numpy_and_pandas() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        close(quantile(&v, 0.25, QuantileMethod::Linear), 3.25);
        close(quantile(&v, 0.5, QuantileMethod::Linear), 5.5);
        close(quantile(&v, 0.75, QuantileMethod::Linear), 7.75);
        close(quantile(&v, 0.25, QuantileMethod::Lower), 3.0);
        close(quantile(&v, 0.25, QuantileMethod::Higher), 4.0);
        close(quantile(&v, 0.25, QuantileMethod::Midpoint), 3.5);
        // R type 6: h = p (n + 1) - 1 = 1.75 -> 2 + 0.75
        close(quantile(&v, 0.25, QuantileMethod::Weibull), 2.75);
    }

    #[test]
    fn describe_matches_pandas() {
        let s = describe(
            &[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0],
            QuantileMethod::Linear,
        )
        .unwrap();
        assert_eq!(s.count, 8);
        close(s.mean, 5.0);
        close(s.stdev.unwrap(), (32.0f64 / 7.0).sqrt());
        close(s.q1, 4.0);
        close(s.median, 4.5);
        close(s.q3, 5.5);
    }

    #[test]
    fn tukey_whiskers_exclude_outliers() {
        let b = box_stats(
            &[1.0, 2.0, 3.0, 4.0, 5.0, 100.0],
            Whiskers::Tukey(1.5),
            QuantileMethod::Linear,
        )
        .unwrap();
        assert_eq!(b.outliers, vec![100.0]);
        close(b.upper_whisker, 5.0);
        close(b.lower_whisker, 1.0);
    }

    #[test]
    fn ecdf_merges_ties() {
        assert_eq!(
            ecdf(&[1.0, 1.0, 2.0, 4.0]),
            vec![(1.0, 0.5), (2.0, 0.75), (4.0, 1.0)]
        );
    }

    #[test]
    fn histogram_uses_nice_steps() {
        let h = histogram(&[0.0, 1.0, 2.0, 9.0, 10.0], 5).unwrap();
        close(h.step, 2.0);
        assert_eq!(h.counts.iter().sum::<usize>(), 5);
    }

    #[test]
    fn normal_quantile_and_wilson_are_accurate() {
        assert!((normal_ppf(0.975) - 1.959963984540054).abs() < 1e-8);
        // 0 of 10 at 95%: lower bound 0, upper bound 0.2775 (Wilson).
        let (lo, hi) = wilson(0, 10, 0.95).unwrap();
        close(lo, 0.0);
        assert!((hi - 0.2775).abs() < 1e-3, "{hi}");
        let (lo, hi) = wilson(10, 10, 0.95).unwrap();
        assert!((lo - 0.7225).abs() < 1e-3 && (hi - 1.0).abs() < 1e-9);
        assert!(wilson(1, 0, 0.95).is_none());
    }

    #[test]
    fn bootstrap_is_seeded_and_brackets_the_estimate() {
        let v: Vec<f64> = (1..=40).map(f64::from).collect();
        let a = bootstrap_ci(&v, Statistic::Mean, 0.95, 500, 7).unwrap();
        assert_eq!(a, bootstrap_ci(&v, Statistic::Mean, 0.95, 500, 7).unwrap());
        assert!(a.0 < mean(&v) && mean(&v) < a.1);
        assert_ne!(a, bootstrap_ci(&v, Statistic::Mean, 0.95, 500, 8).unwrap());
    }

    #[test]
    fn ols_recovers_a_line() {
        let pts: Vec<(f64, f64)> = (0..10).map(|i| (i as f64, 2.0 * i as f64 + 1.0)).collect();
        let (b, m, r) = ols(&pts).unwrap();
        close(b, 1.0);
        close(m, 2.0);
        close(r, 1.0);
    }

    #[test]
    fn bimodal_sample_is_flagged() {
        let mut v = vec![1.0; 20];
        v.extend(vec![10.0; 20]);
        for (i, x) in v.iter_mut().enumerate() {
            *x += (i % 5) as f64 * 0.1;
        }
        assert!(bimodality_coefficient(&v).unwrap() > 5.0 / 9.0);
    }
}
