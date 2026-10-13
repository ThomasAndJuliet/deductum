//! Bounded placement of supplied measured alternatives. No content, chart policy or I/O.
use figkit_model::{
    BlockMetrics, Bounds, ElementId, ErrorKind, FigureError, FigureResult as Result, Point,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};
pub mod family_geometry;
pub mod raster;
pub mod routing;
pub use raster::Occupancy;
pub use routing::{Connector, Route, RouteOptions, Start, Target, route, starts};
pub type Box2 = [f64; 4];
pub type Position = (f64, f64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    LegacyFamily,
    LegacySpecial,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementBudget {
    pub max_occupancy_cells: usize,
    pub max_raster_work: usize,
    pub max_candidates: usize,
    pub max_routes: usize,
    pub max_route_samples: usize,
    pub max_scoring_work: usize,
    pub max_items: usize,
    pub max_alternatives: usize,
    pub max_targets: usize,
    pub max_obstacles: usize,
    pub max_regions: usize,
    pub max_preferences: usize,
}
impl Default for PlacementBudget {
    fn default() -> Self {
        Self {
            max_occupancy_cells: 16_000_000,
            max_raster_work: 200_000_000,
            max_candidates: 2_000_000,
            max_routes: 200_000,
            max_route_samples: 20_000_000,
            max_scoring_work: 20_000_000,
            max_items: 4096,
            max_alternatives: 4096,
            max_targets: 4096,
            max_obstacles: 100_000,
            max_regions: 4096,
            max_preferences: 4096,
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Work {
    pub occupancy_cells: usize,
    pub raster_work: usize,
    pub candidates: usize,
    pub routes: usize,
    pub route_samples: usize,
    pub scoring_work: usize,
}
/// Shared per-operation counters. Cloned rasters and routing attempts consume the same budget.
#[derive(Debug, Clone)]
pub struct Budget {
    limits: PlacementBudget,
    work: Rc<RefCell<Work>>,
}
impl Budget {
    pub fn new(limits: &PlacementBudget) -> Self {
        Self {
            limits: limits.clone(),
            work: Rc::new(RefCell::new(Work::default())),
        }
    }
    pub fn work(&self) -> Work {
        self.work.borrow().clone()
    }
    pub fn limits(&self) -> &PlacementBudget {
        &self.limits
    }
    pub(crate) fn charge(&self, kind: &str, n: usize) -> Result<()> {
        let mut w = self.work.borrow_mut();
        let (count, max) = match kind {
            "cells" => (&mut w.occupancy_cells, self.limits.max_occupancy_cells),
            "raster" => (&mut w.raster_work, self.limits.max_raster_work),
            "candidates" => (&mut w.candidates, self.limits.max_candidates),
            "routes" => (&mut w.routes, self.limits.max_routes),
            "scoring" => (&mut w.scoring_work, self.limits.max_scoring_work),
            _ => (&mut w.route_samples, self.limits.max_route_samples),
        };
        *count = count
            .checked_add(n)
            .filter(|v| *v <= max)
            .ok_or_else(|| exhausted(kind))?;
        Ok(())
    }
}
pub(crate) fn exhausted(kind: &str) -> FigureError {
    FigureError::new(
        ErrorKind::BudgetExceeded,
        format!("placement {kind} budget exceeded"),
    )
}
pub(crate) fn invalid(message: &str) -> FigureError {
    FigureError::new(ErrorKind::InvalidInput, message)
}
pub(crate) fn finite(values: &[f64]) -> Result<()> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("placement coordinates must be finite"))
    }
}
pub fn grow(b: Box2, pad: f64) -> Box2 {
    [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]
}
pub fn box_distance(b: Box2, p: Position) -> f64 {
    (b[0] - p.0)
        .max(0.)
        .max(p.0 - b[2])
        .hypot((b[1] - p.1).max(0.).max(p.1 - b[3]))
}
fn valid_box(b: Box2) -> Result<()> {
    finite(&b)?;
    if b[2] < b[0] || b[3] < b[1] {
        return Err(invalid("inverted placement bounds"));
    }
    Ok(())
}
fn array(b: &Bounds) -> Box2 {
    [b.x, b.y, b.x + b.width, b.y + b.height]
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alternative {
    pub id: ElementId,
    pub metrics: BlockMetrics,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementItem {
    pub id: ElementId,
    pub alternatives: Vec<Alternative>,
    pub allowed_regions: Vec<Bounds>,
    pub preferred_positions: Vec<Point>,
    pub target_groups: Vec<Vec<Target>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obstacle {
    pub bounds: Bounds,
    pub clearance: f64,
    pub treatment: ObstacleTreatment,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObstacleTreatment {
    Hard,
    Soft,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentObstacle {
    pub from: Point,
    pub to: Point,
    pub clearance: f64,
    pub treatment: ObstacleTreatment,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementProblem {
    pub region: Bounds,
    pub items: Vec<PlacementItem>,
    pub obstacles: Vec<Obstacle>,
    pub segments: Vec<SegmentObstacle>,
    pub profile: Profile,
    pub em: f64,
    pub connector_width: f64,
    pub head_scale: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedReason {
    NoFeasibleCandidate,
    BudgetExceeded,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unresolved {
    pub item: ElementId,
    pub reason: UnresolvedReason,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placement {
    pub item: ElementId,
    pub alternative: ElementId,
    pub translation: Point,
    pub connectors: Vec<Connector>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementOutcome {
    pub placements: Vec<Placement>,
    pub unresolved: Vec<Unresolved>,
    pub work: Work,
}
/// Candidate score, supplied alternative index, placed measured box.
pub type Candidate = (f64, usize, Box2);
/// Enumerate the legacy two-point raster grid in authored alternative order, then sort by
/// distance/inside/alternative score and lexicographic box position. Counts all grid probes.
pub fn candidates(
    sizes: &[(f64, f64)],
    region: Box2,
    plot: Box2,
    targets: &[Vec<Target>],
    em: f64,
    occ: &mut Occupancy,
    budget: &Budget,
) -> Result<Vec<Candidate>> {
    valid_box(region)?;
    valid_box(plot)?;
    finite(&[em])?;
    if sizes.len() > budget.limits.max_alternatives {
        return Err(exhausted("alternatives"));
    }
    validate_targets(targets, budget)?;
    let mut out = vec![];
    for (bi, &(w, h)) in sizes.iter().enumerate() {
        finite(&[w, h])?;
        if w < 0. || h <= 0. {
            return Err(invalid("measured alternative has invalid dimensions"));
        }
        let mut y = region[1];
        while y + h <= region[3] {
            let mut x = region[0];
            while x + w <= region[2] {
                budget.charge("candidates", 1)?;
                let b = [x, y, x + w, y + h];
                if occ.free(b)? {
                    budget.charge(
                        "scoring",
                        targets.iter().map(Vec::len).sum::<usize>().max(1),
                    )?;
                    out.push((score(b, plot, targets, em, bi), bi, b));
                }
                let next = x + 2.;
                if next == x {
                    return Err(invalid("candidate grid cannot advance"));
                }
                x = next;
            }
            let next = y + 2.;
            if next == y {
                return Err(invalid("candidate grid cannot advance"));
            }
            y = next;
        }
    }
    out.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap()
            .then(a.1.cmp(&b.1))
            .then(a.2.partial_cmp(&b.2).unwrap())
    });
    Ok(out)
}
fn score(b: Box2, plot: Box2, targets: &[Vec<Target>], em: f64, bi: usize) -> f64 {
    let dmin = 2. * em;
    let d = if targets.is_empty() {
        ((b[0] + b[2]) / 2. - (plot[2] - 0.25 * (plot[2] - plot[0]))).hypot(b[1] - plot[1])
    } else {
        let d = targets
            .iter()
            .map(|g| {
                g.iter()
                    .map(|p| box_distance(b, p.position))
                    .fold(f64::INFINITY, f64::min)
            })
            .sum::<f64>()
            / targets.len() as f64;
        if d >= dmin { d } else { dmin + (dmin - d) * 4. }
    };
    d + if b[0] >= plot[0] && b[2] <= plot[2] && b[1] >= plot[1] && b[3] <= plot[3] {
        0.
    } else {
        0.6 * em
    } + bi as f64 * 0.8
}
fn validate_targets(groups: &[Vec<Target>], budget: &Budget) -> Result<()> {
    let mut n = 0usize;
    for g in groups {
        if g.is_empty() {
            return Err(invalid("target groups must not be empty"));
        }
        n = n
            .checked_add(g.len())
            .filter(|n| *n <= budget.limits.max_targets)
            .ok_or_else(|| exhausted("targets"))?;
        for t in g {
            finite(&[t.position.0, t.position.1, t.clearance, t.cost])?;
            if t.clearance < 0. {
                return Err(invalid("negative endpoint clearance"));
            }
        }
    }
    Ok(())
}

/// Place items in input order without mutating the problem. Budget exhaustion preserves prior
/// placements in the returned owned outcome and marks the current and remaining items exhausted.
/// NoFeasibleCandidate describes only the enumerated two-point grid and legacy 60/6 search.
pub fn place(problem: &PlacementProblem, limits: &PlacementBudget) -> Result<PlacementOutcome> {
    let region = array(&problem.region);
    valid_box(region)?;
    if region[0] < 0. || region[1] < 0. {
        return Err(invalid("raster placement region must be nonnegative"));
    }
    finite(&[problem.em, problem.connector_width, problem.head_scale])?;
    if problem.em <= 0. || problem.connector_width < 0. || problem.head_scale <= 0. {
        return Err(invalid("invalid placement style parameters"));
    }
    if problem.items.len() > limits.max_items {
        return Err(exhausted("items"));
    }
    if problem.items.is_empty() {
        return Ok(PlacementOutcome {
            placements: vec![],
            unresolved: vec![],
            work: Work::default(),
        });
    }
    if problem
        .obstacles
        .len()
        .checked_add(problem.segments.len())
        .is_none_or(|n| n > limits.max_obstacles)
    {
        return Err(exhausted("obstacles"));
    }
    let budget = Budget::new(limits);
    let mut hard = Occupancy::new(region[2], region[3], problem.profile, &budget)?;
    let mut soft = Occupancy::new(region[2], region[3], problem.profile, &budget)?;
    for o in &problem.obstacles {
        valid_box(array(&o.bounds))?;
        finite(&[o.clearance])?;
        if o.clearance < 0. {
            return Err(invalid("negative obstacle clearance"));
        }
        let target = match o.treatment {
            ObstacleTreatment::Hard => &mut hard,
            ObstacleTreatment::Soft => &mut soft,
        };
        target.rect(grow(array(&o.bounds), o.clearance))?;
    }
    for o in &problem.segments {
        let target = match o.treatment {
            ObstacleTreatment::Hard => &mut hard,
            ObstacleTreatment::Soft => &mut soft,
        };
        target.segment((o.from.x, o.from.y), (o.to.x, o.to.y), o.clearance)?;
    }
    let mut outcome = PlacementOutcome {
        placements: vec![],
        unresolved: vec![],
        work: Work::default(),
    };
    let mut ids = std::collections::BTreeSet::new();
    for item in &problem.items {
        if item.id.0.is_empty() || !ids.insert(item.id.clone()) {
            return Err(invalid("item IDs must be nonempty and unique"));
        }
    }
    for (index, item) in problem.items.iter().enumerate() {
        let result = place_item(problem, item, region, &mut hard, &mut soft, &budget);
        match result {
            Ok(Some(placement)) => outcome.placements.push(placement),
            Ok(None) => outcome.unresolved.push(Unresolved {
                item: item.id.clone(),
                reason: UnresolvedReason::NoFeasibleCandidate,
            }),
            Err(e) if e.kind == ErrorKind::BudgetExceeded => {
                for item in &problem.items[index..] {
                    outcome.unresolved.push(Unresolved {
                        item: item.id.clone(),
                        reason: UnresolvedReason::BudgetExceeded,
                    });
                }
                break;
            }
            Err(e) => return Err(e),
        }
    }
    outcome.work = budget.work();
    Ok(outcome)
}
fn place_item(
    problem: &PlacementProblem,
    item: &PlacementItem,
    region: Box2,
    hard: &mut Occupancy,
    soft: &mut Occupancy,
    budget: &Budget,
) -> Result<Option<Placement>> {
    if item.allowed_regions.len() > budget.limits.max_regions {
        return Err(exhausted("regions"));
    }
    if item.preferred_positions.len() > budget.limits.max_preferences {
        return Err(exhausted("preferences"));
    }
    validate_targets(&item.target_groups, budget)?;
    if item.alternatives.len() > budget.limits.max_alternatives {
        return Err(exhausted("alternatives"));
    }
    let mut alt_ids = std::collections::BTreeSet::new();
    let mut sizes = vec![];
    for a in &item.alternatives {
        if a.id.0.is_empty() || !alt_ids.insert(a.id.clone()) {
            return Err(invalid(
                "alternative IDs must be nonempty and unique per item",
            ));
        }
        let m = &a.metrics;
        finite(&[m.size.width, m.size.height])?;
        if m.size.width < 0.
            || m.size.height <= 0.
            || m.baseline
                .is_some_and(|b| !b.is_finite() || b < 0. || b > m.size.height)
        {
            return Err(invalid("invalid measured alternative"));
        }
        sizes.push((m.size.width, m.size.height));
    }
    let regions = if item.allowed_regions.is_empty() {
        vec![region]
    } else {
        item.allowed_regions.iter().map(array).collect()
    };
    let mut cands = vec![];
    for r in regions {
        valid_box(r)?;
        let clipped = [
            r[0].max(region[0]),
            r[1].max(region[1]),
            r[2].min(region[2]),
            r[3].min(region[3]),
        ];
        if clipped[0] > clipped[2] || clipped[1] > clipped[3] {
            continue;
        }
        cands.extend(candidates(
            &sizes,
            clipped,
            region,
            &item.target_groups,
            problem.em,
            hard,
            budget,
        )?);
    }
    for (bi, &(w, h)) in sizes.iter().enumerate() {
        for p in &item.preferred_positions {
            finite(&[p.x, p.y])?;
            budget.charge("candidates", 1)?;
            let b = [p.x, p.y, p.x + w, p.y + h];
            let allowed = item.allowed_regions.is_empty()
                || item.allowed_regions.iter().any(|r| {
                    let r = array(r);
                    b[0] >= r[0] && b[1] >= r[1] && b[2] <= r[2] && b[3] <= r[3]
                });
            if allowed
                && b[0] >= region[0]
                && b[1] >= region[1]
                && b[2] <= region[2]
                && b[3] <= region[3]
                && hard.free(b)?
            {
                budget.charge(
                    "scoring",
                    item.target_groups
                        .iter()
                        .map(Vec::len)
                        .sum::<usize>()
                        .max(1),
                )?;
                cands.push((score(b, region, &item.target_groups, problem.em, bi), bi, b));
            }
        }
    }
    // Preferred positions add a distance penalty, retaining the authored alternative tie order.
    for c in &mut cands {
        budget.charge("scoring", item.preferred_positions.len().max(1))?;
        if !item.preferred_positions.is_empty() {
            c.0 += item
                .preferred_positions
                .iter()
                .map(|p| (c.2[0] - p.x).hypot(c.2[1] - p.y))
                .fold(f64::INFINITY, f64::min);
        }
        if !soft.free(c.2)? {
            c.0 += 3. * problem.em;
        }
    }
    cands.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap()
            .then(a.1.cmp(&b.1))
            .then(a.2.partial_cmp(&b.2).unwrap())
    });
    let mut base = hard.try_clone()?;
    let mut tried: Vec<Box2> = vec![];
    let mut found: Vec<(f64, usize, Box2, Vec<Connector>)> = vec![];
    let options = RouteOptions {
        profile: problem.profile,
        origin: (region[0], region[1]),
        width: region[2],
        height: region[3],
        em: problem.em,
        head_length: (3.2 + 1.2 * problem.connector_width) * problem.head_scale,
    };
    for (score, bi, b) in cands {
        if tried
            .iter()
            .any(|t| (b[0] - t[0]).abs() < 5. && (b[1] - t[1]).abs() < 5.)
        {
            continue;
        }
        tried.push(b);
        if tried.len() > 60 || found.len() >= 6 {
            break;
        }
        let mut copy = if item.target_groups.len() > 1 {
            Some(base.try_clone()?)
        } else {
            None
        };
        let occupancy = copy.as_mut().unwrap_or(&mut base);
        let mut connectors = vec![];
        let mut cost = 0.;
        let mut ok = true;
        for group in &item.target_groups {
            let aim = group
                .iter()
                .map(|t| t.position)
                .min_by(|a, c| box_distance(b, *a).total_cmp(&box_distance(b, *c)))
                .unwrap();
            let Some(route) = route(
                &options,
                &routing::starts_with_profile(problem.em, b, aim, problem.profile)?,
                group,
                Some(b),
                occupancy,
                soft,
                budget,
            )?
            else {
                ok = false;
                break;
            };
            let Some(connector) = routing::connector_with_profile(
                &route,
                problem.connector_width,
                problem.head_scale,
                problem.profile,
                budget,
            )?
            else {
                ok = false;
                break;
            };
            cost += route.score;
            for p in routing::points(&connector, budget)? {
                occupancy.rect([p.0 - 0.8, p.1 - 0.8, p.0 + 0.8, p.1 + 0.8])?;
            }
            connectors.push(connector);
        }
        if ok {
            found.push((
                score + 0.5 * cost / item.target_groups.len().max(1) as f64,
                bi,
                b,
                connectors,
            ));
        }
    }
    let best = match problem.profile {
        Profile::LegacyFamily => found
            .into_iter()
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap()),
        Profile::LegacySpecial => found
            .into_iter()
            .reduce(|a, b| if b.0 < a.0 { b } else { a }),
    };
    let Some((_, bi, b, connectors)) = best else {
        return Ok(None);
    };
    hard.rect(b)?;
    for connector in &connectors {
        for p in routing::points(connector, budget)? {
            hard.rect([p.0 - 0.8, p.1 - 0.8, p.0 + 0.8, p.1 + 0.8])?;
        }
    }
    Ok(Some(Placement {
        item: item.id.clone(),
        alternative: item.alternatives[bi].id.clone(),
        translation: Point { x: b[0], y: b[1] },
        connectors,
    }))
}
pub use figkit_model::FigureError as PlacementError;
