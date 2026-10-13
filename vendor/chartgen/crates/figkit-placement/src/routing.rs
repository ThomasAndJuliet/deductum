//! Deterministic quadratic route search. Curves use supplied endpoints and measured obstacles.
use crate::*;
use figkit_model::CubicSegment;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub position: Position,
    pub clearance: f64,
    pub cost: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Start {
    pub position: Position,
    pub normal: Position,
    pub cost: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteOptions {
    pub origin: Position,
    pub profile: Profile,
    pub width: f64,
    pub height: f64,
    pub em: f64,
    pub head_length: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub score: f64,
    pub start: Position,
    pub control: Position,
    pub end: Position,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connector {
    pub start: Point,
    pub segments: Vec<CubicSegment>,
    pub head: Vec<Point>,
}
pub fn route(
    options: &RouteOptions,
    starts: &[Start],
    targets: &[Target],
    own_box: Option<Box2>,
    hard: &Occupancy,
    soft: &Occupancy,
    budget: &Budget,
) -> Result<Option<Route>> {
    finite(&[
        options.origin.0,
        options.origin.1,
        options.width,
        options.height,
        options.em,
        options.head_length,
    ])?;
    if options.origin.0 < 0.
        || options.origin.1 < 0.
        || options.origin.0 > options.width
        || options.origin.1 > options.height
        || options.em <= 0.
        || options.head_length < 0.
    {
        return Err(invalid("invalid route dimensions"));
    }
    if let Some(b) = own_box {
        valid_box(b)?;
    }
    if starts.len() > budget.limits.max_targets || targets.len() > budget.limits.max_targets {
        return Err(exhausted("targets"));
    }
    let mut best: Option<Route> = None;
    for start in starts {
        let (s, n, scost) = (start.position, start.normal, start.cost);
        finite(&[s.0, s.1, n.0, n.1, scost])?;
        for target in targets {
            budget.charge("routes", 1)?;
            let t = target.position;
            let r = target.clearance;
            finite(&[t.0, t.1, r, target.cost])?;
            if r < 0. {
                return Err(invalid("negative endpoint clearance"));
            }
            let l = (t.0 - s.0).hypot(t.1 - s.1);
            if !l.is_finite() {
                return Err(invalid("route length overflow"));
            }
            if l < r + 1.6 * options.head_length + 0.3 * options.em {
                continue;
            }
            let tang = (-n.1, n.0);
            let lat = (t.0 - s.0) * tang.0 + (t.1 - s.1) * tang.1;
            let ahead = (t.0 - s.0) * n.0 + (t.1 - s.1) * n.1;
            let back = (-ahead).max(0.) * 2.;
            let bend = if lat.abs() < 0.25 * l {
                0.18 * l * if lat >= 0. { 1. } else { -1. }
            } else {
                0.
            };
            let k = (0.35 * l).max((0.6 * l).min(ahead.abs() * 0.8 + 0.2 * l));
            let q = (s.0 + n.0 * k + tang.0 * bend, s.1 + n.1 * k + tang.1 * bend);
            let u = unit((t.0 - q.0, t.1 - q.1));
            let e = (t.0 - u.0 * r, t.1 - u.1 * r);
            finite(&[q.0, q.1, e.0, e.1])?;
            if l / 1.5 >= usize::MAX as f64 {
                return Err(exhausted("route samples"));
            }
            let steps = 12.max((l / 1.5) as usize);
            let mut ok = true;
            let mut cross = 0;
            for j in 1..=steps {
                budget.charge("samples", 1)?;
                let t = j as f64 / steps as f64;
                let p = match options.profile {
                    Profile::LegacyFamily => {
                        let (a, b, c) = ((1. - t) * (1. - t), 2. * (1. - t) * t, t * t);
                        (a * s.0 + b * q.0 + c * e.0, a * s.1 + b * q.1 + c * e.1)
                    }
                    Profile::LegacySpecial => bezier(s, q, e, t),
                };
                if own_box.is_some_and(|b| {
                    b[0] - 0.5 < p.0 && p.0 < b[2] + 0.5 && b[1] - 0.5 < p.1 && p.1 < b[3] + 0.5
                }) {
                    ok = false;
                    break;
                }
                if hard.at(p.0, p.1)
                    || !(options.origin.0 <= p.0
                        && p.0 <= options.width
                        && options.origin.1 <= p.1
                        && p.1 <= options.height)
                {
                    ok = false;
                    break;
                }
                if soft.at(p.0, p.1) && (p.0 - e.0).hypot(p.1 - e.1) > options.head_length {
                    cross += 1;
                }
            }
            if !ok {
                continue;
            }
            let score = l + cross as f64 * 3. + back + target.cost + scost;
            if best.as_ref().is_none_or(|b| score < b.score) {
                best = Some(Route {
                    score,
                    start: s,
                    control: q,
                    end: e,
                });
            }
        }
    }
    Ok(best)
}
pub fn starts(em: f64, b: Box2, target: Position) -> Result<Vec<Start>> {
    starts_with_profile(em, b, target, Profile::LegacyFamily)
}
pub fn starts_with_profile(
    em: f64,
    b: Box2,
    target: Position,
    profile: Profile,
) -> Result<Vec<Start>> {
    valid_box(b)?;
    finite(&[em, target.0, target.1])?;
    if em <= 0. {
        return Err(invalid("routing em must be positive"));
    }
    // Bound intermediate rounding arithmetic before deriving candidate coordinates.
    if b.iter().any(|v| v.abs() > f64::MAX / 1000.) || em > f64::MAX / 1000. {
        return Err(invalid("route start arithmetic overflow"));
    }
    let g = 0.25 * em;
    let [x0, y0, x1, y1] = b;
    let cl = |v: f64, a: f64, c: f64| v.max(a).min(c);
    let (mx0, mx1) = (x0 + 0.2 * (x1 - x0), x1 - 0.2 * (x1 - x0));
    let (my0, my1) = (y0 + 0.25 * (y1 - y0), y1 - 0.25 * (y1 - y0));
    let set = |v: Vec<f64>| {
        let mut v: Vec<f64> = v
            .into_iter()
            .map(|v| match profile {
                Profile::LegacyFamily => (v * 100.).round() / 100.,
                Profile::LegacySpecial => format!("{v:.2}").parse().unwrap_or(v),
            })
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v.dedup();
        v
    };
    let xs = set(vec![cl(target.0, mx0, mx1), mx0, (x0 + x1) / 2., mx1]);
    let ys = set(vec![cl(target.1, my0, my1), my0, (y0 + y1) / 2., my1]);
    let mut out = vec![];
    out.extend(xs.iter().map(|&x| Start {
        position: (x, y0 - g),
        normal: (0., -1.),
        cost: 0.,
    }));
    out.extend(xs.iter().map(|&x| Start {
        position: (x, y1 + g),
        normal: (0., 1.),
        cost: 0.,
    }));
    out.extend(ys.iter().map(|&y| Start {
        position: (x0 - g, y),
        normal: (-1., 0.),
        cost: 0.,
    }));
    out.extend(ys.iter().map(|&y| Start {
        position: (x1 + g, y),
        normal: (1., 0.),
        cost: 0.,
    }));
    Ok(out)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn bezier(s: (f64, f64), q: (f64, f64), e: (f64, f64), t: f64) -> (f64, f64) {
    let (a, b, c) = ((1. - t).powi(2), 2. * (1. - t) * t, t * t);
    (a * s.0 + b * q.0 + c * e.0, a * s.1 + b * q.1 + c * e.1)
}

fn tangent(s: (f64, f64), q: (f64, f64), e: (f64, f64), t: f64) -> (f64, f64) {
    (
        2. * (1. - t) * (q.0 - s.0) + 2. * t * (e.0 - q.0),
        2. * (1. - t) * (q.1 - s.1) + 2. * t * (e.1 - q.1),
    )
}

fn unit(v: (f64, f64)) -> (f64, f64) {
    let l = v.0.hypot(v.1);
    let l = if l == 0. { 1. } else { l };
    (v.0 / l, v.1 / l)
}

fn arc_table(s: (f64, f64), q: (f64, f64), e: (f64, f64)) -> (Vec<f64>, Vec<f64>) {
    let n = 240;
    let ts: Vec<f64> = (0..=n).map(|k| k as f64 / n as f64).collect();
    let pts: Vec<(f64, f64)> = ts.iter().map(|t| bezier(s, q, e, *t)).collect();
    let mut cum = vec![0.0];
    for w in pts.windows(2) {
        let last = *cum.last().unwrap();
        cum.push(last + dist(w[0], w[1]));
    }
    (ts, cum)
}

fn t_at(ts: &[f64], cum: &[f64], d: f64) -> f64 {
    if d <= 0. {
        return 0.;
    }
    if d >= *cum.last().unwrap() {
        return 1.;
    }
    let (mut lo, mut hi) = (0usize, cum.len() - 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if cum[mid] < d {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let span = cum[hi] - cum[lo];
    let f = (d - cum[lo]) / if span == 0. { 1. } else { span };
    ts[lo] + f * (ts[hi] - ts[lo])
}

// ---- the house arrowhead (tools/arrowheads.py) ------------------------------------------------

const NOTCH: f64 = 0.72;

fn head_size(width: f64) -> (f64, f64) {
    (3.2 + 1.2 * width, 1.6 + 0.6 * width)
}

fn r3(v: f64) -> f64 {
    format!("{v:.3}").parse().unwrap()
}

/// The head's outline in its own frame: (a back from the tip, c across, left positive), as
/// the original samples its path written to three decimals.
fn head_outline(width: f64) -> Vec<(f64, f64)> {
    let (l, wd) = head_size(width);
    let u = (1.0, 0.0);
    let n = (-u.1, u.0);
    let tip = (0.0, 0.0);
    let back = (tip.0 - l * u.0, tip.1 - l * u.1);
    let notch = (tip.0 - 0.72 * l * u.0, tip.1 - 0.72 * l * u.1);
    let r = |p: (f64, f64)| (r3(p.0), r3(p.1));
    let mut segs: Vec<((f64, f64), (f64, f64))> = vec![];
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
        if side == 1.0 {
            segs.push((r(c1), r(barb)));
            segs.push((r(c2), r(notch)));
        } else {
            segs.push((r(c2), r(barb)));
            segs.push((r(c1), r(tip)));
        }
    }
    let mut cur = r(tip);
    let mut pts = vec![cur];
    for (c, q) in segs {
        for k in 1..=16 {
            let t = k as f64 / 16.;
            pts.push((
                (1. - t).powi(2) * cur.0 + 2. * (1. - t) * t * c.0 + t * t * q.0,
                (1. - t).powi(2) * cur.1 + 2. * (1. - t) * t * c.1 + t * t * q.1,
            ));
        }
        cur = q;
    }
    pts.into_iter().map(|(x, y)| (-x, y)).collect()
}

/// Encode a routed quadratic as its cubic shaft and supplied-scale house head geometry.
pub fn connector(
    route: &Route,
    width: f64,
    scale: f64,
    budget: &Budget,
) -> Result<Option<Connector>> {
    connector_with_profile(route, width, scale, Profile::LegacySpecial, budget)
}
pub fn connector_with_profile(
    route: &Route,
    width: f64,
    scale: f64,
    profile: Profile,
    budget: &Budget,
) -> Result<Option<Connector>> {
    finite(&[width, scale])?;
    if width < 0. || scale <= 0. {
        return Err(invalid("invalid connector stroke or head scale"));
    }
    // Fixed 241 arc samples + 65 outline points + 25 shaft/325 head occupancy points.
    budget.charge("samples", 306)?;
    let (s, q, e) = (route.start, route.control, route.end);
    finite(&[
        s.0,
        s.1,
        q.0,
        q.1,
        e.0,
        e.1,
        (3.2 + 1.2 * width) * scale,
        width * scale,
    ])?;
    let lh = head_size(width).0 * scale;
    let outline: Vec<Position> = match profile {
        Profile::LegacyFamily => crate::family_geometry::head_outline(width),
        Profile::LegacySpecial => head_outline(width),
    }
    .into_iter()
    .map(|(a, c)| (a * scale, c * scale))
    .collect();
    let (ts, cum) = match profile {
        Profile::LegacyFamily => crate::family_geometry::arc_table(s, q, e),
        Profile::LegacySpecial => arc_table(s, q, e),
    };
    let bz = |s, q, e, t| match profile {
        Profile::LegacyFamily => crate::family_geometry::bezier(s, q, e, t),
        Profile::LegacySpecial => bezier(s, q, e, t),
    };
    let total = *cum.last().unwrap();
    if total < 1.4 * lh {
        return Ok(None);
    }
    let tn = t_at(&ts, &cum, total - NOTCH * lh);
    let a1 = (s.0 + tn * (q.0 - s.0), s.1 + tn * (q.1 - s.1));
    let a2 = bz(s, q, e, tn);
    let c1 = (s.0 + 2. / 3. * (a1.0 - s.0), s.1 + 2. / 3. * (a1.1 - s.1));
    let c2 = (
        a2.0 + 2. / 3. * (a1.0 - a2.0),
        a2.1 + 2. / 3. * (a1.1 - a2.1),
    );
    let point = |p: Position| Point { x: p.0, y: p.1 };
    let head = outline
        .into_iter()
        .map(|(a, c)| {
            let t = t_at(&ts, &cum, total - a);
            let p = bz(s, q, e, t);
            let u = unit(tangent(s, q, e, t));
            point((p.0 - c * u.1, p.1 + c * u.0))
        })
        .collect();
    Ok(Some(Connector {
        start: point(s),
        segments: vec![CubicSegment {
            c1: point(c1),
            c2: point(c2),
            end: point(a2),
        }],
        head,
    }))
}
pub fn points(connector: &Connector, budget: &Budget) -> Result<Vec<Position>> {
    let n = connector
        .segments
        .len()
        .checked_mul(25)
        .and_then(|n| {
            connector
                .head
                .len()
                .checked_mul(5)
                .and_then(|h| n.checked_add(h))
        })
        .ok_or_else(|| exhausted("samples"))?;
    budget.charge("samples", n)?;
    let mut out = Vec::with_capacity(n);
    let mut p0 = &connector.start;
    for s in &connector.segments {
        for j in 0..=24 {
            let t = j as f64 / 24.;
            let (a, b, c, d) = (
                (1. - t).powi(3),
                3. * (1. - t).powi(2) * t,
                3. * (1. - t) * t * t,
                t.powi(3),
            );
            out.push((
                a * p0.x + b * s.c1.x + c * s.c2.x + d * s.end.x,
                a * p0.y + b * s.c1.y + c * s.c2.y + d * s.end.y,
            ));
        }
        p0 = &s.end;
    }
    for (i, p) in connector.head.iter().enumerate() {
        let q = &connector.head[(i + 1) % connector.head.len()];
        for j in 0..5 {
            let t = j as f64 / 4.;
            out.push((p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t));
        }
    }
    Ok(out)
}
