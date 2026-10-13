//! The message layer's comments and title connector, ported from the original's
//! `annotate.py` (R5-5, RP3-7, R15-1, R15-2; G6.3, G8.3, G8.4, G8.7, G8.8; D-19): the maker's
//! remarks in centred lines of about 18–28 characters, placed in the empty space nearest
//! their anchors, never over marks or text, each with a thin arrow of one smooth bend to its
//! anchors, its head the house arrowhead laid along the curve. Where no space is free the
//! chart is laid out again narrower, leaving a column at the right; failing that the comment
//! goes under the chart.
//!
//! The layer works on the laid-out figure (the original's model): what is drawn is read from
//! the figure's items by their classes, and anchors from the layout's own geometry.
use super::continuous as ta;
use super::fig::{self, Batch, Bx, Item, Txt};
use super::*;
use chrono::NaiveDateTime;
use serde_json::Value;

/// Classes that are never obstacles to a comment.
const NOT_OBSTACLES: [&str; 6] = [
    "glow",
    "glow-line",
    "highlight-band",
    "gridline",
    "time-divider",
    "time-tick",
];

// ---- free space ------------------------------------------------------------------------------

struct Occupancy(figkit_placement::Occupancy);
impl Occupancy {
    fn new(w: f64, h: f64) -> Result<Self> {
        let budget = figkit_placement::Budget::new(&figkit_placement::PlacementBudget::default());
        figkit_placement::Occupancy::new(w, h, figkit_placement::Profile::LegacySpecial, &budget)
            .map(Self)
            .map_err(placement_error)
    }
    fn rect(&mut self, b: Bx) -> Result<()> {
        self.0.rect([b.0, b.1, b.2, b.3]).map_err(placement_error)
    }
    fn segment(&mut self, p: (f64, f64), q: (f64, f64), pad: f64) -> Result<()> {
        self.0.segment(p, q, pad).map_err(placement_error)
    }
    fn try_clone(&self) -> Result<Self> {
        self.0.try_clone().map(Self).map_err(placement_error)
    }
}
fn placement_error(e: figkit_placement::PlacementError) -> Error {
    Error::new("placement", e.to_string())
}

fn grow(b: Bx, p: f64) -> Bx {
    (b.0 - p, b.1 - p, b.2 + p, b.3 + p)
}

fn box_dist(b: Bx, p: (f64, f64)) -> f64 {
    let dx = (b.0 - p.0).max(0.).max(p.0 - b.2);
    let dy = (b.1 - p.1).max(0.).max(p.1 - b.3);
    dx.hypot(dy)
}

// ---- the comments of a spec ------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Ref {
    pub series: Option<String>,
    pub category: Option<String>,
    pub x: Option<Value>,
}

impl Ref {
    fn from(v: &Value) -> Self {
        Ref {
            series: v["series"].as_str().map(str::to_owned),
            category: v["category"].as_str().map(str::to_owned),
            x: v.get("x").filter(|x| !x.is_null()).cloned(),
        }
    }
    fn words(&self) -> String {
        match (&self.series, &self.x) {
            (Some(s), Some(Value::Array(a))) if a.len() == 2 => format!(
                "{s} ({}–{})",
                a[0].as_str().unwrap_or(""),
                a[1].as_str().unwrap_or("")
            ),
            (Some(s), Some(x)) => format!("{s} at {}", x.as_str().unwrap_or("")),
            _ => self
                .category
                .clone()
                .or_else(|| self.series.clone())
                .unwrap_or_else(|| {
                    self.x
                        .as_ref()
                        .map(|x| x.as_str().unwrap_or("").to_owned())
                        .unwrap_or_default()
                }),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Comment {
    pub text: TextContent,
    pub anchors: Vec<Ref>,
    pub placement: String,
    pub arrow: bool,
}

pub(crate) fn comments_of(plan: &SemanticPlan) -> Result<Vec<Comment>> {
    let Some(rows) = plan.spec.raw.get("comments").and_then(Value::as_array) else {
        return Ok(vec![]);
    };
    rows.iter()
        .enumerate()
        .map(|(i, c)| {
            Ok(Comment {
                text: guides::content(&c["text"])?
                    .ok_or_else(|| err(format!("comments[{i}] needs text")))?,
                anchors: c["anchors"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(Ref::from)
                    .collect(),
                placement: c["placement"].as_str().unwrap_or("auto").to_owned(),
                arrow: c["arrow"].as_bool().unwrap_or(true),
            })
        })
        .collect()
}

pub(crate) fn title_link(plan: &SemanticPlan) -> Option<Ref> {
    plan.spec
        .raw
        .pointer("/title/link")
        .filter(|v| v.is_object())
        .map(Ref::from)
}

/// Every string the comments may set: each run of up to nine words.
pub(crate) fn need(plan: &SemanticPlan, batch: &Batch) -> Result<()> {
    for c in comments_of(plan)? {
        match &c.text {
            TextContent::Plain { text } => {
                let words: Vec<&str> = text.split_whitespace().collect();
                let n = words.len();
                for i in 0..n {
                    for j in i + 1..=(n.min(i + 9)) {
                        batch.need("comment", &words[i..j].join(" "))?;
                    }
                }
            }
            other => batch.need_content("comment", other)?,
        }
    }
    if title_link(plan).is_some() {
        batch.need("title", "H")?;
    }
    Ok(())
}

// ---- what the layout leaves ------------------------------------------------------------------

/// How a true time axis maps instants and values onto the figure (step.py's time_marks).
#[derive(Debug, Clone)]
pub(crate) struct TimeMarks {
    pub t0: NaiveDateTime,
    pub span: f64,
    pub x0: f64,
    pub w: f64,
    pub dy: f64,
    pub plot_h: f64,
    pub scale: TrainedScale,
    pub thin: bool,
}

impl TimeMarks {
    pub fn x_of(&self, t: NaiveDateTime) -> f64 {
        self.x0 + self.w * ta::secs(self.t0, t) / self.span
    }
    pub fn y(&self, v: Decimal) -> Result<f64> {
        Ok(self.plot_h * (1. - self.scale.fraction(v)?) + self.dy)
    }
}

/// A laid-out figure and the geometry the message layer reads.
pub(crate) struct Laid {
    pub root: Item,
    pub width: f64,
    pub height: f64,
    pub plot: Bx,
    pub findings: Vec<Finding>,
    pub summary: BTreeMap<String, Value>,
    pub time: Option<TimeMarks>,
    /// The layout's decisions (`{rule, level, text, guide}`), in the order made.
    pub decisions: Vec<Value>,
}

pub(crate) enum Drawn {
    Laid(Laid),
    Refused(Vec<Finding>),
}

/// The block a comment is set in: plain lines, their extents, its size.
#[derive(Debug, Clone)]
struct Block {
    lines: Vec<TextContent>,
    exts: Vec<fig::Ext>,
    w: f64,
    h: f64,
    pitch: f64,
}

struct Target {
    x: f64,
    y: f64,
    r: f64,
    cost: Option<f64>,
}

fn wrap(
    n: usize,
    wd: &BTreeMap<(usize, usize), f64>,
    limit: f64,
    max_lines: usize,
) -> Option<Vec<(usize, usize)>> {
    fn f(
        i: usize,
        m: usize,
        n: usize,
        wd: &BTreeMap<(usize, usize), f64>,
        limit: f64,
        memo: &mut BTreeMap<(usize, usize), (f64, Option<Vec<(usize, usize)>>)>,
    ) -> (f64, Option<Vec<(usize, usize)>>) {
        if i == n {
            return (0., Some(vec![]));
        }
        if m == 0 {
            return (f64::INFINITY, None);
        }
        if let Some(v) = memo.get(&(i, m)) {
            return v.clone();
        }
        let mut res: (f64, Option<Vec<(usize, usize)>>) = (f64::INFINITY, None);
        for j in i + 1..=n {
            let Some(&w) = wd.get(&(i, j)) else { break };
            if w > limit {
                break;
            }
            let rest = f(j, m - 1, n, wd, limit, memo);
            let Some(tail) = rest.1 else { continue };
            let cand = w.max(rest.0);
            if cand < res.0 {
                let mut lines = vec![(i, j)];
                lines.extend(tail);
                res = (cand, Some(lines));
            }
        }
        memo.insert((i, m), res.clone());
        res
    }
    for k in 1..=max_lines {
        let mut memo = BTreeMap::new();
        if let (_, Some(lines)) = f(0, k, n, wd, limit, &mut memo) {
            return Some(lines);
        }
    }
    None
}

fn short(s: &str) -> String {
    if s.chars().count() <= 40 {
        s.to_owned()
    } else {
        let head: String = s.chars().take(39).collect();
        format!("{}…", head.trim_end())
    }
}

/// The message layer over one laid-out figure.
pub(crate) struct Annotator<'a> {
    plan: &'a SemanticPlan,
    data: &'a PreparedData,
    batch: &'a Batch<'a>,
    pub laid: Laid,
    column: Option<(f64, f64)>,
    before: usize,
    last_route: f64,
    em: f64,
    added: Vec<Item>,
    comment_boxes: Vec<Value>,
    title_link_summary: Option<Value>,
    decisions: Vec<Value>,
}

impl<'a> Annotator<'a> {
    pub fn new(
        plan: &'a SemanticPlan,
        data: &'a PreparedData,
        batch: &'a Batch<'a>,
        laid: Laid,
        column: Option<(f64, f64)>,
    ) -> Result<Self> {
        Ok(Self {
            plan,
            data,
            batch,
            laid,
            column,
            before: 0,
            last_route: 0.,
            em: batch.metric("--label-size")?,
            added: vec![],
            comment_boxes: vec![],
            title_link_summary: None,
            decisions: vec![],
        })
    }

    fn m(&self, name: &str) -> Result<f64> {
        self.batch.metric(name)
    }

    fn all_items(&self) -> Vec<&Item> {
        let mut out = vec![];
        self.laid.root.walk(&mut out);
        out.extend(self.added.iter());
        out
    }

    /// What a new text or comment must stay clear of.
    fn occupancy(&self, pad: f64, texts: bool, marks: bool, lines: bool) -> Result<Occupancy> {
        let mut occ = Occupancy::new(self.laid.width, self.laid.height.max(1.0) + 400.)?;
        let (tpad, mpad) = (pad, pad);
        let lw = self.m("--line-width")?;
        let lwh = self.m("--line-width-highlight")?;
        for it in self.all_items() {
            let classes: &[String] = match it {
                Item::Text(_) | Item::Group { .. } => &[],
                Item::Rect { classes, .. }
                | Item::Line { classes, .. }
                | Item::Path { classes, .. }
                | Item::Circle { classes, .. }
                | Item::Curve { classes, .. } => classes,
            };
            if matches!(it, Item::Group { .. })
                || classes.iter().any(|c| NOT_OBSTACLES.contains(&c.as_str()))
            {
                continue;
            }
            match it {
                Item::Text(t) => {
                    if texts {
                        occ.rect(grow(t.bbox(0.), tpad))?;
                    }
                }
                Item::Rect { .. } | Item::Circle { .. } => {
                    if marks {
                        occ.rect(grow(it.bbox().unwrap(), mpad))?;
                    }
                }
                Item::Line { x1, y1, x2, y2, .. } => {
                    if lines {
                        occ.segment((*x1, *y1), (*x2, *y2), mpad + 0.5)?;
                    }
                }
                Item::Path { points, .. } => {
                    if lines {
                        let hw = if classes.iter().any(|c| c == "highlight") {
                            lwh
                        } else {
                            lw
                        } / 2.;
                        for w in points.windows(2) {
                            occ.segment(w[0], w[1], mpad + hw)?;
                        }
                    }
                }
                Item::Curve { .. } => {
                    if lines {
                        for p in fig::curve_points(it, 24) {
                            occ.rect((
                                p.0 - mpad - 0.5,
                                p.1 - mpad - 0.5,
                                p.0 + mpad + 0.5,
                                p.1 + mpad + 0.5,
                            ))?;
                        }
                    }
                }
                Item::Group { .. } => {}
            }
        }
        Ok(occ)
    }

    // ---- anchors ----
    fn anchor(&self, r: &Ref) -> Result<Vec<Target>> {
        let gap = self.m("--comment-arrow-gap")?;
        if let Some(tm) = &self.laid.time {
            return self.time_anchor(r, gap, tm);
        }
        Ok(vec![])
    }

    fn time_anchor(&self, r: &Ref, gap: f64, tm: &TimeMarks) -> Result<Vec<Target>> {
        let x =
            r.x.as_ref()
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| r.category.clone());
        let Some(x) = x else { return Ok(vec![]) };
        let Some((k, t)) = find_event(self.plan, self.data, r.series.as_deref(), &x)? else {
            return Ok(vec![]);
        };
        let pts = self.data.irregular.as_ref().and_then(|rows| rows.first());
        if !tm.thin {
            let Some(v) = pts.and_then(|p| p.get(k)).and_then(|p| p.datum.value) else {
                return Ok(vec![]);
            };
            return Ok(vec![Target {
                x: tm.x_of(t),
                y: tm.y(v)?,
                r: self.m("--line-width")? / 2. + gap,
                cost: None,
            }]);
        }
        let stamp = ta::stamp(t);
        let bars: Vec<Bx> = self
            .all_items()
            .into_iter()
            .filter_map(|it| match it {
                Item::Rect { classes, data, .. }
                    if classes.iter().any(|c| c == "event-bar")
                        && data.iter().any(|d| d.0 == "x" && d.1 == stamp) =>
                {
                    it.bbox()
                }
                _ => None,
            })
            .collect();
        let side = 0.8 * self.em;
        if bars.is_empty() {
            let res = if t.format("%H:%M").to_string() == "00:00" {
                "day"
            } else {
                "minute"
            };
            let cx = tm.x_of(super::step::instant(t, res, true));
            let notes: Vec<Bx> = self
                .all_items()
                .into_iter()
                .filter_map(|it| match it {
                    Item::Text(n) if n.role == "missing-note" => {
                        let b = n.bbox(0.);
                        ((b.0 + b.2 - 2. * cx).abs() < b.2 - b.0 + 1.).then_some(b)
                    }
                    _ => None,
                })
                .collect();
            if let Some(b) = notes.first() {
                return Ok(vec![
                    Target {
                        x: (b.0 + b.2) / 2.,
                        y: b.1,
                        r: gap,
                        cost: None,
                    },
                    Target {
                        x: b.0,
                        y: (b.1 + b.3) / 2.,
                        r: gap,
                        cost: None,
                    },
                    Target {
                        x: b.2,
                        y: (b.1 + b.3) / 2.,
                        r: gap,
                        cost: None,
                    },
                ]);
            }
            return Ok(vec![Target {
                x: cx,
                y: tm.y(Decimal::ZERO)?,
                r: gap,
                cost: None,
            }]);
        }
        let top = bars
            .iter()
            .copied()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap();
        let yn = top.1 + (0.3 * (top.3 - top.1)).min(1.5 * self.em);
        Ok(vec![
            Target {
                x: (top.0 + top.2) / 2.,
                y: top.1,
                r: gap,
                cost: None,
            },
            Target {
                x: top.0,
                y: yn,
                r: gap,
                cost: Some(side),
            },
            Target {
                x: top.2,
                y: yn,
                r: gap,
                cost: Some(side),
            },
        ])
    }

    // ---- arrows ----
    fn head_style(&self, classes: &[&str]) -> Result<(f64, f64)> {
        let w = if classes.contains(&"title-link") {
            self.m("--link-width")?
        } else {
            self.m("--comment-arrow-width")?
        };
        let mode = self
            .batch
            .theme
            .tokens
            .get("--comment-arrow-head")
            .and_then(Value::as_str)
            .unwrap_or("bend")
            .trim()
            .to_owned();
        if mode != "bend" {
            return Err(err(format!(
                "--comment-arrow-head {mode} is not ported; only the default bend head is drawn"
            )));
        }
        let scale = fig::metric_or(self.batch.theme, "--comment-arrow-head-scale", 1.0);
        Ok((w, scale))
    }

    #[allow(clippy::type_complexity)]
    fn route(
        &mut self,
        starts: &[((f64, f64), (f64, f64), Option<f64>)],
        targets: &[Target],
        bx: Option<Bx>,
        occ_text: &Occupancy,
        occ_marks: &Occupancy,
        classes: &[&str],
    ) -> Result<Option<Item>> {
        let (w, scale) = self.head_style(classes)?;
        let budget = occ_text.0.budget().clone();
        let start_values: Vec<_> = starts
            .iter()
            .map(|&(position, normal, cost)| figkit_placement::Start {
                position,
                normal,
                cost: cost.unwrap_or(0.),
            })
            .collect();
        let target_values: Vec<_> = targets
            .iter()
            .map(|t| figkit_placement::Target {
                position: (t.x, t.y),
                clearance: t.r,
                cost: t.cost.unwrap_or(0.),
            })
            .collect();
        let options = figkit_placement::RouteOptions {
            origin: (0., 0.),
            profile: figkit_placement::Profile::LegacySpecial,
            width: self.laid.width,
            height: self.laid.height,
            em: self.em,
            head_length: (3.2 + 1.2 * w) * scale,
        };
        let Some(route) = figkit_placement::route(
            &options,
            &start_values,
            &target_values,
            bx.map(|b| [b.0, b.1, b.2, b.3]),
            &occ_text.0,
            &occ_marks.0,
            &budget,
        )
        .map_err(placement_error)?
        else {
            return Ok(None);
        };
        self.last_route = route.score;
        let Some(connector) = figkit_placement::routing::connector(&route, w, scale, &budget)
            .map_err(placement_error)?
        else {
            return Ok(None);
        };
        let pair = |p: Point| (p.x, p.y);
        Ok(Some(Item::Curve {
            start: pair(connector.start),
            segments: connector
                .segments
                .into_iter()
                .map(|s| (pair(s.c1), pair(s.c2), pair(s.end)))
                .collect(),
            head: connector.head.into_iter().map(pair).collect(),
            classes: classes.iter().map(|s| (*s).to_owned()).collect(),
        }))
    }

    #[allow(clippy::type_complexity)]
    fn starts(
        &self,
        b: Bx,
        target: (f64, f64),
    ) -> Result<Vec<((f64, f64), (f64, f64), Option<f64>)>> {
        Ok(figkit_placement::routing::starts_with_profile(
            self.em,
            [b.0, b.1, b.2, b.3],
            target,
            figkit_placement::Profile::LegacySpecial,
        )
        .map_err(placement_error)?
        .into_iter()
        .map(|s| (s.position, s.normal, None))
        .collect())
    }

    fn header_texts(&self) -> Vec<&Txt> {
        if let Item::Group { children, .. } = &self.laid.root {
            for c in children {
                if let Item::Group {
                    classes, children, ..
                } = c
                {
                    if classes.iter().any(|x| x == "header") {
                        return children
                            .iter()
                            .filter_map(|i| match i {
                                Item::Text(t) => Some(t),
                                _ => None,
                            })
                            .collect();
                    }
                }
            }
        }
        vec![]
    }

    // ---- the title connector (G8.8) ----
    fn title_link(&mut self, findings: &mut Vec<Finding>) -> Result<()> {
        let Some(link) = title_link(self.plan) else {
            return Ok(());
        };
        let titles: Vec<Txt> = self
            .header_texts()
            .into_iter()
            .filter(|t| t.role == "title")
            .cloned()
            .collect();
        let targets = self.anchor(&link)?;
        if titles.is_empty() || targets.is_empty() {
            findings.push(
                Finding::new(
                    "6.12",
                    "info",
                    "the title connector finds no title line or no mark to point at",
                )
                .at("title.link"),
            );
            return Ok(());
        }
        let last = titles
            .iter()
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .unwrap();
        let b = last.bbox(0.);
        let mid = last.y - self.batch.ext("title", "H")?.ascent / 2.;
        let g = 0.45 * self.em;
        let starts = vec![
            ((b.2 + g, mid), (1.0, 0.0), None),
            ((b.2 - self.em, b.3 + g), (0.0, 1.0), Some(6. * self.em)),
            (
                (b.2 - 3. * self.em, b.3 + g),
                (0.0, 1.0),
                Some(6. * self.em),
            ),
        ];
        let occ_t = self.occupancy(1.0, true, false, false)?;
        let occ_m = self.occupancy(0.0, false, true, true)?;
        let Some(curve) = self.route(&starts, &targets, None, &occ_t, &occ_m, &["title-link"])?
        else {
            findings.push(
                Finding::new(
                    "6.12",
                    "warning",
                    "the title connector finds no way to its mark clear of the text",
                )
                .at("title.link"),
            );
            return Ok(());
        };
        if let Item::Curve {
            start, segments, ..
        } = &curve
        {
            let end = segments.last().unwrap().2;
            self.title_link_summary = Some(json!([
                super::step::r2(start.0),
                super::step::r2(start.1),
                super::step::r2(end.0),
                super::step::r2(end.1)
            ]));
        }
        self.added.push(curve);
        let name = link
            .category
            .clone()
            .or_else(|| link.series.clone())
            .or_else(|| link.x.as_ref().map(|x| x.as_str().unwrap_or("").to_owned()))
            .unwrap_or_default();
        self.decisions.push(fig::decision(
            "6.12",
            format!("a thin grey curved arrow from the end of the title to {name} (G8.8)"),
            Some("open"),
        ));
        Ok(())
    }

    // ---- comments ----
    fn blocks(&self, c: &Comment) -> Result<Vec<Block>> {
        let TextContent::Plain { text } = &c.text else {
            let e = self.batch.ext_of("comment", &c.text)?.2;
            return Ok(vec![Block {
                lines: vec![c.text.clone()],
                exts: vec![e],
                w: e.width,
                h: e.ascent + e.descent,
                pitch: 0.,
            }]);
        };
        let words: Vec<&str> = text.split_whitespace().collect();
        let pitch = self.m("--line-pitch")? * self.m("--comment-size")? / self.em;
        let n = words.len();
        let mut spans = BTreeMap::new();
        let mut wd = BTreeMap::new();
        for i in 0..n {
            for j in i + 1..=(n.min(i + 9)) {
                let s = words[i..j].join(" ");
                wd.insert((i, j), self.batch.width("comment", &s)?);
                spans.insert((i, j), s);
            }
        }
        let base = self.m("--comment-width")? * self.m("--comment-size")? / self.em;
        let mut out = vec![];
        let mut seen: Vec<Vec<(usize, usize)>> = vec![];
        for f in [1.0, 0.85, 1.2, 0.7] {
            let Some(lines) = wrap(n, &wd, base * f, 5) else {
                continue;
            };
            if seen.contains(&lines) {
                continue;
            }
            seen.push(lines.clone());
            let plain: Vec<String> = lines.iter().map(|k| spans[k].clone()).collect();
            let exts = plain
                .iter()
                .map(|p| self.batch.ext("comment", p))
                .collect::<Result<Vec<_>>>()?;
            let w = exts
                .iter()
                .map(|e| e.width)
                .fold(f64::NEG_INFINITY, f64::max);
            let h = exts[0].ascent + pitch * (exts.len() - 1) as f64 + exts.last().unwrap().descent;
            out.push(Block {
                lines: plain.into_iter().map(TextContent::plain).collect(),
                exts,
                w,
                h,
                pitch,
            });
        }
        Ok(out)
    }

    fn where_(&self, b: Bx) -> &'static str {
        let (x0, y0, x1, y1) = self.laid.plot;
        if self.column.is_some_and(|c| b.0 >= c.0 - 0.5) {
            return "in the column at the right";
        }
        if b.1 >= self.laid.height - 1.0 && b.1 > y1 {
            return "under the chart";
        }
        if b.0 >= x0 - 1. && b.2 <= x1 + 1. && b.1 >= y0 - 1. && b.3 <= y1 + 1. {
            "in empty space inside the plot"
        } else {
            "in empty space beside the plot"
        }
    }

    fn put(
        &mut self,
        k: usize,
        c: &Comment,
        block: &Block,
        b: Bx,
        arrows: Vec<Item>,
    ) -> Result<()> {
        let n = arrows.len();
        self.decisions.push(fig::decision(
            "RP3-7",
            format!(
                "comment {} (“{}”) {}{} (G8.7)",
                k + 1,
                short(c.text.readable()),
                self.where_(b),
                if n > 0 {
                    format!(
                        ", {n} arrow{} to {}",
                        if n != 1 { "s" } else { "" },
                        c.anchors
                            .iter()
                            .map(Ref::words)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    ", no arrow (a remark on the whole chart)".to_owned()
                }
            ),
            None,
        ));
        let cx = (b.0 + b.2) / 2.;
        let y = b.1 + block.exts[0].ascent;
        for (j, line) in block.lines.iter().enumerate() {
            let t = self.batch.text(
                "comment",
                line.clone(),
                cx,
                y + j as f64 * block.pitch,
                "middle",
            )?;
            self.added.push(Item::Text(t));
        }
        self.added.extend(arrows);
        self.comment_boxes.push(json!([
            super::step::r2(b.0),
            super::step::r2(b.1),
            super::step::r2(b.2),
            super::step::r2(b.3)
        ]));
        Ok(())
    }

    fn top_of_free(&self) -> Result<f64> {
        let gap = self.m("--comment-gap")?;
        Ok(self
            .header_texts()
            .iter()
            .map(|t| t.bbox(0.).3)
            .fold(0.0f64, f64::max)
            + gap)
    }

    fn place(
        &mut self,
        c: &Comment,
        blocks: &[Block],
        targets: &[Vec<Target>],
    ) -> Result<Option<(usize, Bx, Vec<Item>)>> {
        let gap = self.m("--comment-gap")?;
        let mut occ = self.occupancy(gap, true, true, true)?;
        let base_text = self.occupancy(1.0, true, false, false)?;
        let occ_marks = self.occupancy(0.0, false, true, true)?;
        let (x0, y0, x1, y1) = self.laid.plot;
        let (w_fig, h_fig) = (self.laid.width, self.laid.height);
        let top = self.top_of_free()?;
        let mut region = (0.0, top, w_fig, h_fig);
        if c.placement == "inside" {
            region = (x0, top.max(y0), x1, y1);
        } else if c.placement == "right" && self.column.is_some() {
            region = (self.column.unwrap().0, top, w_fig, h_fig);
        } else if c.placement == "right" || c.placement == "below" {
            return Ok(None);
        }
        let em = self.em;
        let shared_targets: Vec<Vec<figkit_placement::Target>> = targets
            .iter()
            .map(|g| {
                g.iter()
                    .map(|t| figkit_placement::Target {
                        position: (t.x, t.y),
                        clearance: t.r,
                        cost: t.cost.unwrap_or(0.),
                    })
                    .collect()
            })
            .collect();
        let budget = occ.0.budget().clone();
        let sizes: Vec<_> = blocks.iter().map(|b| (b.w, b.h)).collect();
        let cands = figkit_placement::candidates(
            &sizes,
            [region.0, region.1, region.2, region.3],
            [x0, y0, x1, y1],
            &shared_targets,
            em,
            &mut occ.0,
            &budget,
        )
        .map_err(placement_error)?;
        let cands: Vec<_> = cands
            .into_iter()
            .map(|(s, i, b)| (s, i, (b[0], b[1], b[2], b[3])))
            .collect();
        let mut tried: Vec<Bx> = vec![];
        let mut found: Vec<(f64, usize, Bx, Vec<Item>)> = vec![];
        let mut base_text = base_text;
        for (score, bi, bx) in cands {
            if tried
                .iter()
                .any(|t| (bx.0 - t.0).abs() < 5. && (bx.1 - t.1).abs() < 5.)
            {
                continue;
            }
            tried.push(bx);
            if tried.len() > 60 || found.len() >= 6 {
                break;
            }
            let mut copy = if targets.len() > 1 {
                Some(base_text.try_clone()?)
            } else {
                None
            };
            let mut arrows = Some(vec![]);
            let mut cost = 0.;
            for t in targets {
                let aim = if t.len() == 1 {
                    (t[0].x, t[0].y)
                } else {
                    t.iter()
                        .map(|p| (p.x, p.y))
                        .min_by(|a, b| box_dist(bx, *a).partial_cmp(&box_dist(bx, *b)).unwrap())
                        .unwrap()
                };
                let starts = self.starts(bx, aim)?;
                let occ_text = copy.as_ref().unwrap_or(&base_text);
                let a = self.route(
                    &starts,
                    t,
                    Some(bx),
                    occ_text,
                    &occ_marks,
                    &["comment-arrow"],
                )?;
                let Some(a) = a else {
                    arrows = None;
                    break;
                };
                cost += self.last_route;
                let target = copy.as_mut().unwrap_or(&mut base_text);
                for p in fig::curve_points(&a, 24) {
                    target.rect((p.0 - 0.8, p.1 - 0.8, p.0 + 0.8, p.1 + 0.8))?;
                }
                arrows.as_mut().unwrap().push(a);
            }
            if let Some(arrows) = arrows {
                found.push((
                    score + 0.5 * cost / targets.len().max(1) as f64,
                    bi,
                    bx,
                    arrows,
                ));
            }
        }
        let _ = &mut occ;
        let Some(best) = found
            .into_iter()
            .reduce(|a, b| if b.0 < a.0 { b } else { a })
        else {
            return Ok(None);
        };
        Ok(Some((best.1, best.2, best.3)))
    }

    /// The comments in order; the ones that found no free space, with their narrowest block.
    fn comments(&mut self, findings: &mut Vec<Finding>) -> Result<Vec<(usize, f64)>> {
        let comments = comments_of(self.plan)?;
        let mut failed = vec![];
        let mut placed = 0;
        for (k, c) in comments.iter().enumerate() {
            let blocks = self.blocks(c)?;
            let mut targets = vec![];
            if c.arrow {
                for r in &c.anchors {
                    let a = self.anchor(r)?;
                    if !a.is_empty() {
                        targets.push(a);
                    }
                }
            }
            if c.arrow && !c.anchors.is_empty() && targets.len() < c.anchors.len() {
                findings.push(
                    Finding::new(
                        "RP3-7",
                        "warning",
                        format!(
                            "comment {}: an anchor names no drawn element; no arrow to it",
                            k + 1
                        ),
                    )
                    .at(format!("comments[{k}].anchors")),
                );
            }
            match self.place(c, &blocks, &targets)? {
                None => {
                    let w = blocks.iter().map(|b| b.w).fold(f64::INFINITY, f64::min);
                    failed.push((k, w));
                }
                Some((bi, bx, arrows)) => {
                    self.put(k, c, &blocks[bi], bx, arrows)?;
                    placed += 1;
                }
            }
        }
        if placed > 0 {
            self.decisions.push(fig::decision(
                "R5-5",
                "the maker's comments in New Computer Modern italic, in the reserved blue, set apart from the labelling and outside its count of text styles (G6.3, D-19)",
                None,
            ));
        }
        self.questions();
        Ok(failed)
    }

    /// R15-1 (G8.3): which of the four questions the title and comments answer.
    fn questions(&mut self) {
        let raw = &self.plan.spec.raw;
        let comments = raw["comments"].as_array().cloned().unwrap_or_default();
        let mut answered: BTreeMap<&str, String> = BTreeMap::new();
        if raw.pointer("/title/kind").and_then(Value::as_str) == Some("insight") {
            answered.insert("important", "the title".into());
        }
        let names = [
            ("important", "what matters"),
            ("why_care", "why care"),
            ("explain", "what explains it"),
            ("action", "what to do"),
        ];
        for (k, c) in comments.iter().enumerate() {
            for a in c["answers"].as_array().into_iter().flatten() {
                if let Some(q) = names.iter().find(|n| Some(n.0) == a.as_str()) {
                    answered
                        .entry(q.0)
                        .or_insert_with(|| format!("comment {}", k + 1));
                }
            }
        }
        if answered.is_empty() || comments.is_empty() {
            return;
        }
        let got = names
            .iter()
            .filter_map(|(q, n)| answered.get(q).map(|a| format!("{n}: {a}")))
            .collect::<Vec<_>>()
            .join("; ");
        let miss: Vec<&str> = names
            .iter()
            .filter(|(q, _)| !answered.contains_key(q))
            .map(|(_, n)| *n)
            .collect();
        self.decisions.push(fig::decision(
            "R15-1",
            format!(
                "the four questions answered: {got}{} (G8.3)",
                if miss.is_empty() {
                    String::new()
                } else {
                    format!(
                        " (not answered: {}, which need be answered only where known)",
                        miss.join(", ")
                    )
                }
            ),
            Some("open"),
        ));
    }

    /// Comments that found no space: under the chart, their arrows if they run clear.
    fn below(&mut self, failed: &[(usize, f64)], findings: &mut Vec<Finding>) -> Result<()> {
        if failed.is_empty() {
            return Ok(());
        }
        let gap = self.m("--comment-gap")?;
        let occ_text = self.occupancy(1.0, true, false, false)?;
        let occ_marks = self.occupancy(0.0, false, true, true)?;
        let comments = comments_of(self.plan)?;
        for (k, _) in failed {
            let c = &comments[*k];
            let blocks = self.blocks(c)?;
            let b = blocks
                .iter()
                .cloned()
                .reduce(|a, b| if b.w > a.w { b } else { a })
                .ok_or_else(|| err("a comment has no block"))?;
            let bottom = self
                .all_items()
                .into_iter()
                .filter(|i| !matches!(i, Item::Group { .. } | Item::Line { .. }))
                .filter_map(Item::bbox)
                .map(|b| b.3)
                .fold(f64::NEG_INFINITY, f64::max);
            let y = bottom + gap;
            let mut targets = vec![];
            if c.arrow {
                for r in &c.anchors {
                    let a = self.anchor(r)?;
                    if !a.is_empty() {
                        targets.push(a);
                    }
                }
            }
            let cx = if targets.is_empty() {
                (self.laid.plot.0 + self.laid.plot.2) / 2.
            } else {
                targets.iter().map(|t| t[0].x).sum::<f64>() / targets.len() as f64
            };
            let x = (cx - b.w / 2.).max(0.0).min(self.laid.width - b.w);
            let bx = (x, y, x + b.w, y + b.h);
            let mut arrows = vec![];
            for t in &targets {
                let starts = self.starts(bx, (t[0].x, t[0].y))?;
                if let Some(a) = self.route(
                    &starts,
                    t,
                    Some(bx),
                    &occ_text,
                    &occ_marks,
                    &["comment-arrow"],
                )? {
                    arrows.push(a);
                }
            }
            if !targets.is_empty() && arrows.len() < targets.len() {
                findings.push(
                    Finding::new(
                        "RP3-7",
                        "warning",
                        format!(
                            "comment {}: no free space near its anchors; set under the chart, without the arrows that would cross text",
                            k + 1
                        ),
                    )
                    .at(format!("comments[{k}]")),
                );
            } else {
                findings.push(
                    Finding::new(
                        "RP3-7",
                        "info",
                        format!(
                            "comment {}: no free space near its anchors; set under the chart",
                            k + 1
                        ),
                    )
                    .at(format!("comments[{k}]")),
                );
            }
            self.put(*k, c, &b, bx, arrows)?;
            self.laid.height = self.laid.height.max(bx.3 + 1.0);
        }
        Ok(())
    }

    /// Everything in order (the time kinds' layer: good and bad, the title connector, then the
    /// comments); returns the comments that found no free space.
    pub fn run(&mut self, findings: &mut Vec<Finding>) -> Result<Vec<(usize, f64)>> {
        self.before = self.added.len();
        self.title_link(findings)?;
        self.comments(findings)
    }

    /// The figure with the layer's items in a `comments` group before the footer, its height
    /// grown to hold them, and the layer's summary.
    pub fn finish(mut self) -> Laid {
        self.laid.decisions.append(&mut self.decisions);
        let bottom = self
            .added
            .iter()
            .filter(|i| !matches!(i, Item::Line { .. }))
            .filter_map(Item::bbox)
            .map(|b| b.3)
            .fold(self.laid.height - 1.0, f64::max);
        self.laid.height = self.laid.height.max(bottom + 1.0);
        if !self.added.is_empty() {
            if let Item::Group { children, .. } = &mut self.laid.root {
                let at = children
                    .iter()
                    .position(|c| matches!(c, Item::Group { classes, .. } if classes.iter().any(|x| x == "footer")))
                    .unwrap_or(children.len());
                children.insert(at, Item::group("comments", std::mem::take(&mut self.added)));
            }
        }
        let mut ann = serde_json::Map::new();
        if let Some(t) = self.title_link_summary.take() {
            ann.insert("title_link".into(), t);
        }
        if !self.comment_boxes.is_empty() {
            ann.insert(
                "comments".into(),
                Value::Array(std::mem::take(&mut self.comment_boxes)),
            );
        }
        if !ann.is_empty() {
            let entry = self
                .laid
                .summary
                .entry("annotations".into())
                .or_insert_with(|| json!({}));
            if let Some(o) = entry.as_object_mut() {
                o.extend(ann);
            }
        }
        self.laid
    }
}

/// (index, instant) of the event a reference's x names in the chart's irregular series, or
/// None when it names none (a day alone names the one event of that day).
fn find_event(
    plan: &SemanticPlan,
    data: &PreparedData,
    sid: Option<&str>,
    text: &str,
) -> Result<Option<(usize, NaiveDateTime)>> {
    let Ok((t, res)) = ta::parse_instant(text) else {
        return Ok(None);
    };
    let Some(rows) = &data.irregular else {
        return Ok(None);
    };
    let ids: Vec<&str> = plan.spec.raw["data"]["series"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| s["id"].as_str().unwrap_or(""))
        .collect();
    let mut found = vec![];
    let mut day = vec![];
    for (si, row) in rows.iter().enumerate() {
        if sid.is_some_and(|s| ids.get(si) != Some(&s)) {
            continue;
        }
        for (k, p) in row.iter().enumerate() {
            let (tp, _) = ta::parse_instant(&p.time)?;
            if p.time == text || (tp == t && res != "day") {
                found.push((k, tp));
            } else if res == "day" && tp.date() == t.date() {
                day.push((k, tp));
            }
        }
    }
    if let Some(f) = found.first() {
        return Ok(Some(*f));
    }
    if day.len() == 1 {
        return Ok(Some(day[0]));
    }
    Ok(None)
}

/// Lay a chart out with its comments and title connector: when a comment finds no free
/// space the chart is laid out again narrower, leaving a column at the right; failing that
/// the comment is set under the chart.
pub(crate) fn layout(
    plan: &SemanticPlan,
    data: &PreparedData,
    batch: &Batch,
    draw: &dyn Fn(f64) -> Result<Drawn>,
) -> Result<std::result::Result<Laid, Vec<Finding>>> {
    let width = batch.c.width;
    let laid = match draw(width)? {
        Drawn::Laid(l) => l,
        Drawn::Refused(f) => return Ok(Err(f)),
    };
    if !matches!(plan.chart_type, ChartType::Step | ChartType::ThinBar) {
        // the layer draws on bars, dots, lines and the time kinds: here only the layout's own
        // good and bad colours take the style's treatment, and what the layer would draw is said
        let raw = &plan.spec.raw;
        const LAYER: [&str; 5] = ["border", "glow", "background", "icon", "selective_label"];
        let wants = !comments_of(plan)?.is_empty()
            || title_link(plan).is_some()
            || (plan.chart_type != ChartType::WrappedBar
                && raw["highlight"].as_array().is_some_and(|h| {
                    h.iter()
                        .any(|e| e["method"].as_str().is_some_and(|m| LAYER.contains(&m)))
                }))
            || raw
                .pointer("/data/comparisons")
                .and_then(Value::as_array)
                .is_some_and(|c| c.iter().any(|c| c["render"].as_str() == Some("line")))
            || (!raw
                .pointer("/data/measure/good")
                .unwrap_or(&Value::Null)
                .is_null()
                && !raw
                    .pointer("/data/measure/dividing_point")
                    .unwrap_or(&Value::Null)
                    .is_null());
        let mut findings = vec![];
        if wants {
            findings.push(Finding::new(
                "CG-3",
                "info",
                format!(
                    "the message layer (comments, comparison lines, highlight methods, good and bad) draws on bars, dots and lines; not on {} charts yet",
                    plan.chart_type.as_str()
                ),
            ));
        }
        let a = Annotator::new(plan, data, batch, laid, None)?;
        let mut laid = a.finish();
        laid.findings.extend(findings);
        return Ok(Ok(laid));
    }
    let wants = !comments_of(plan)?.is_empty() || title_link(plan).is_some();
    if !wants {
        return Ok(Ok(laid));
    }
    need(plan, batch)?;
    let mut findings = vec![];
    let mut a = Annotator::new(plan, data, batch, laid, None)?;
    let failed = a.run(&mut findings)?;
    if failed.is_empty() {
        let mut laid = a.finish();
        laid.findings.extend(findings);
        return Ok(Ok(laid));
    }
    let gap = batch.metric("--comment-gap")?;
    let need_w = failed
        .iter()
        .map(|(_, w)| w + gap * 2.)
        .fold(f64::NEG_INFINITY, f64::max);
    let comments = comments_of(plan)?;
    let forced = failed
        .iter()
        .any(|(k, _)| comments[*k].placement == "inside");
    if !forced && width - need_w >= 120. {
        let narrow = super::step::r2(width - need_w);
        if let Drawn::Laid(mut laid2) = draw(narrow)? {
            laid2.width = width;
            let mut findings = vec![];
            let mut a2 = Annotator::new(plan, data, batch, laid2, Some((width - need_w, width)))?;
            let left = a2.run(&mut findings)?;
            a2.below(&left, &mut findings)?;
            let mut laid2 = a2.finish();
            let entry = laid2
                .summary
                .entry("annotations".into())
                .or_insert_with(|| json!({}));
            if let Some(o) = entry.as_object_mut() {
                o.insert(
                    "column".into(),
                    json!([super::step::r2(width - need_w), super::step::r2(width)]),
                );
            }
            laid2.findings.extend(findings);
            return Ok(Ok(laid2));
        }
    }
    a.below(&failed, &mut findings)?;
    let mut laid = a.finish();
    laid.findings.extend(findings);
    Ok(Ok(laid))
}
