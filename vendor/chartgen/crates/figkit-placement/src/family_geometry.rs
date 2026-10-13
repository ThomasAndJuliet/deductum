//! Exact legacy family arithmetic, including ties-away head rounding.
pub fn head_outline(width: f64) -> Vec<(f64, f64)> {
    let (l, wd) = (3.2 + 1.2 * width, 1.6 + 0.6 * width);
    let u = (1.0_f64, 0.0_f64);
    let n = (-u.1, u.0);
    let tip = (0.0, 0.0);
    let back = (tip.0 - l * u.0, tip.1 - l * u.1);
    let notch = (tip.0 - 0.72 * l * u.0, tip.1 - 0.72 * l * u.1);
    let r3 = |p: (f64, f64)| ((p.0 * 1000.).round() / 1000., (p.1 * 1000.).round() / 1000.);
    let mut segments = vec![];
    for side in [1.0, -1.0] {
        let barb = (back.0 + side * wd * n.0, back.1 + side * wd * n.1);
        let c1 = (
            tip.0 - 0.42 * l * u.0 + side * 0.22 * wd * n.0,
            tip.1 - 0.42 * l * u.1 + side * 0.22 * wd * n.1,
        );
        let c2 = (
            back.0 + 0.2 * l * u.0 + side * 0.35 * wd * n.0,
            back.1 + 0.2 * l * u.1 + side * 0.35 * wd * n.1,
        );
        if side > 0. {
            segments.push((r3(c1), r3(barb)));
            segments.push((r3(c2), r3(notch)));
        } else {
            segments.push((r3(c2), r3(barb)));
            segments.push((r3(c1), r3(tip)));
        }
    }
    let mut cur = r3(tip);
    let mut pts = vec![cur];
    for (c, q) in segments {
        for k in 1..=16 {
            let t = k as f64 / 16.;
            pts.push((
                (1. - t) * (1. - t) * cur.0 + 2. * (1. - t) * t * c.0 + t * t * q.0,
                (1. - t) * (1. - t) * cur.1 + 2. * (1. - t) * t * c.1 + t * t * q.1,
            ));
        }
        cur = q;
    }
    pts.into_iter().map(|(x, y)| (-x, y)).collect()
}

pub fn bezier(s: (f64, f64), q: (f64, f64), e: (f64, f64), t: f64) -> (f64, f64) {
    let (a, b, c) = ((1. - t) * (1. - t), 2. * (1. - t) * t, t * t);
    (a * s.0 + b * q.0 + c * e.0, a * s.1 + b * q.1 + c * e.1)
}
pub fn tangent(s: (f64, f64), q: (f64, f64), e: (f64, f64), t: f64) -> (f64, f64) {
    (
        2. * (1. - t) * (q.0 - s.0) + 2. * t * (e.0 - q.0),
        2. * (1. - t) * (q.1 - s.1) + 2. * t * (e.1 - q.1),
    )
}
pub fn unit(v: (f64, f64)) -> (f64, f64) {
    let l = v.0.hypot(v.1);
    let l = if l == 0. { 1. } else { l };
    (v.0 / l, v.1 / l)
}
pub fn arc_table(s: (f64, f64), q: (f64, f64), e: (f64, f64)) -> (Vec<f64>, Vec<f64>) {
    let n = 240;
    let ts: Vec<f64> = (0..=n).map(|k| k as f64 / n as f64).collect();
    let pts: Vec<(f64, f64)> = ts.iter().map(|&t| bezier(s, q, e, t)).collect();
    let mut cum = vec![0.];
    for w in pts.windows(2) {
        let d = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        cum.push(cum.last().unwrap() + d);
    }
    (ts, cum)
}
pub fn t_at(ts: &[f64], cum: &[f64], d: f64) -> f64 {
    if d <= 0. {
        return 0.;
    }
    if d >= *cum.last().unwrap() {
        return 1.;
    }
    let (mut lo, mut hi) = (0, cum.len() - 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if cum[mid] < d { lo = mid } else { hi = mid }
    }
    let span = cum[hi] - cum[lo];
    let f = (d - cum[lo]) / if span == 0. { 1. } else { span };
    ts[lo] + f * (ts[hi] - ts[lo])
}
