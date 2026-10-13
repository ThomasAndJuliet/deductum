//! The checks of the original's SVG lint (svglint.py CHECKS), in its order.
use crate::drawing::{Box4, Drawing, FORBIDDEN, NON_DATA, TextItem, apply, overlap, turned};
use crate::{fmt_g, py_round};
use chartgen_colour_space::{CVD, contrast, cvd_distance, luminance, oklch};
use chartgen_model::Finding;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::LazyLock;

const STROKED: &[&str] = &[
    "line",
    "projection",
    "step-line",
    "change-shaft",
    "reference-line",
    "incomplete-mark",
];
const FILLED: &[&str] = &[
    "bar",
    "slice",
    "area",
    "segment",
    "cell",
    "event-bar",
    "tile",
    "cell-bar",
];
const SPECIAL: &[&str] = &[
    "average",
    "total",
    "all-others",
    "missing",
    "censored",
    "censored-hatched",
    "censored-dotted",
];
const PARTS: &[&str] = &["slice", "tile"];
const DASHED: &[&str] = &["projection", "reference", "target", "incomplete-mark"];
const NOTE_ROLES: &[&str] = &[
    "missing-note",
    "incomplete-note",
    "arrow-key",
    "chart-note",
    "event-label",
    "footnote",
    "note-mark",
];
const UNIT_ROLES: &[&str] = &["unit-phrase", "unit-line", "scale-title"];
const NON_DATA_CONTRAST: f64 = 1.5;
const THIN: f64 = 0.1;
const TEXT_CONTRAST: f64 = 4.5;
const LABEL_FLOOR: f64 = 9.0;
const NOTE_FLOOR: f64 = 0.8;
const MAX_CHROMA: f64 = 0.15;
const MAX_SPREAD: f64 = 0.12;
const CVD_DISTANCE: f64 = 0.06;
const GREY_CHROMA: f64 = 0.02;
const SPECIAL_CHROMA: f64 = 0.03;
const HUE_TOLERANCE: f64 = 25.;
const STACK_GAP: f64 = 0.3;

pub fn ramp_class(c: &str) -> bool {
    matches!(c, "div-0")
        || c.strip_prefix("seq-")
            .and_then(|k| k.parse::<u32>().ok())
            .is_some_and(|k| (1..10).contains(&k))
        || c.strip_prefix("div-m")
            .or_else(|| c.strip_prefix("div-p"))
            .and_then(|k| k.parse::<u32>().ok())
            .is_some_and(|k| (1..5).contains(&k))
}

pub fn not_regular(c: &str) -> bool {
    SPECIAL.contains(&c)
        || matches!(
            c,
            "reference"
                | "good"
                | "bad"
                | "context"
                | "highlight"
                | "uniform-context"
                | "tint"
                | "labelled"
                | "cell-bar"
                | "action-bar"
        )
        || ramp_class(c)
}

pub struct Lint<'a> {
    pub d: Drawing<'a>,
    pub cvd: String,
    pub date_format: Option<String>,
    pub divider_clear: Option<f64>,
    pub text_pad: Option<f64>,
    pub findings: Vec<Finding>,
}

fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

impl Lint<'_> {
    fn add(
        &mut self,
        rule: &str,
        severity: &str,
        message: String,
        nodes: &[usize],
        level: Option<&str>,
    ) {
        let mut f = Finding::new(rule, severity, message);
        f.elements = nodes.iter().map(|&n| self.d.reference(n)).collect();
        f.level = level.unwrap_or("locked").into();
        self.findings.push(f);
    }
    fn n(&self, i: usize) -> &crate::drawing::Node<'_> {
        &self.d.nodes[i]
    }
    fn regular(&self, i: usize) -> bool {
        !self.d.nodes[i].classes.iter().any(|c| not_regular(c))
    }
    fn is_mark(&self, i: usize) -> bool {
        self.d.marks.contains(&i)
    }
    fn panel_ref(&self, panel: Option<usize>) -> String {
        panel
            .map(|p| format!(" in {}", self.d.reference(p)))
            .unwrap_or_default()
    }
}

fn group<K: PartialEq + Clone, V>(v: &mut Vec<(K, Vec<V>)>, k: K, x: V) {
    match v.iter_mut().find(|(a, _)| *a == k) {
        Some((_, xs)) => xs.push(x),
        None => v.push((k, vec![x])),
    }
}

pub fn frames(l: &mut Lint) {
    let mut found: Vec<(&str, Vec<usize>)> = vec![];
    for &n in &l.d.drawn {
        for f in FORBIDDEN {
            if l.n(n).has(f.0) {
                group(&mut found, f.0, n);
            }
        }
    }
    // the original's defaultdict keeps the classes in the order first met
    for (cls, ns) in found {
        let f = FORBIDDEN.iter().find(|f| f.0 == cls).unwrap();
        l.add(
            f.1,
            "error",
            format!("{}: {}", count(ns.len(), f.2), f.3),
            &ns,
            None,
        );
    }
    let mut outlined = vec![];
    for &n in &l.d.marks.clone() {
        let node = l.n(n);
        if node.kind().is_some_and(|k| FILLED.contains(&k))
            && !node.has_any(&["mark-outline", "highlight"])
        {
            let (fill, stroke) = (l.d.paint(n, "fill"), l.d.paint(n, "stroke"));
            if !fill.is_empty()
                && !stroke.is_empty()
                && l.d.number(n, "stroke-width") > 0.
                && stroke[0] != fill[0]
            {
                outlined.push(n);
            }
        }
    }
    if !outlined.is_empty() {
        let c = l.d.paint(outlined[0], "stroke")[0].clone();
        l.add(
            "R1-03",
            "error",
            format!(
                "{} drawn with an outline ({c}): marks are flat, with no outline (G3.3)",
                count(outlined.len(), "mark")
            ),
            &outlined,
            None,
        );
    }
}

fn direction(d: &Drawing, n: usize) -> Option<&'static str> {
    let pts = d.points(n);
    if pts.len() < 2 {
        return None;
    }
    let ((x0, y0), (x1, y1)) = (pts[0], pts[pts.len() - 1]);
    if (y1 - y0).abs() < 0.01 && (x1 - x0).abs() > 0.01 {
        return Some("across");
    }
    if (x1 - x0).abs() < 0.01 && (y1 - y0).abs() > 0.01 {
        return Some("down");
    }
    Some("slanted")
}

pub fn gridlines(l: &mut Lint) {
    let expected = match l.d.orientation {
        Some("vertical") => Some("across"),
        Some("horizontal") => Some("down"),
        _ => None,
    };
    for (panel, nodes) in l.d.panels() {
        let marks: Vec<usize> = nodes.iter().copied().filter(|n| l.is_mark(*n)).collect();
        let grids: Vec<usize> = nodes
            .iter()
            .copied()
            .filter(|&n| l.n(n).has("gridline"))
            .collect();
        let mut under = grids.clone();
        under.extend(
            nodes
                .iter()
                .copied()
                .filter(|&n| l.n(n).has("time-divider")),
        );
        let labels = l.d.texts_in(&nodes, &["scale-label"]);
        let values = l.d.texts_in(&nodes, &["value-label"]);
        let wher = l.panel_ref(panel);
        if !marks.is_empty()
            && !labels.is_empty()
            && grids.len() < 2
            && values.len() < marks.len()
            && !l.d.chart.iter().any(|c| c == "bumps")
        {
            let what = if grids.is_empty() {
                "no gridlines"
            } else {
                "only one gridline"
            };
            let els = if grids.is_empty() {
                vec![panel.unwrap_or(0)]
            } else {
                grids.clone()
            };
            l.add(
                "R1-06",
                "error",
                format!(
                    "{what}{wher}: without them the plot area is undefined, values cannot be estimated and the direction of “more” is not shown (G3.6)"
                ),
                &els,
                None,
            );
        }
        if !marks.is_empty() && !under.is_empty() {
            let boxes: Vec<(usize, Option<Box4>)> =
                marks.iter().map(|&m| (l.n(m).order, l.d.bx(m))).collect();
            let late: Vec<usize> = under
                .iter()
                .copied()
                .filter(|&g| {
                    let order = l.n(g).order;
                    l.d.bx(g).is_some_and(|gb| {
                        boxes.iter().any(|(o, b)| {
                            *o < order
                                && b.is_some_and(|b| {
                                    gb[0] < b[2] && b[0] < gb[2] && gb[1] < b[3] && b[1] < gb[3]
                                })
                        })
                    })
                })
                .collect();
            if !late.is_empty() {
                l.add(
                    "R1-09",
                    "error",
                    format!(
                        "{}{wher} drawn after the marks: non-data lines lie beneath the data, never over it (G3.8)",
                        count(late.len(), "gridline or divider")
                    ),
                    &late,
                    None,
                );
            }
        }
        let dirs: Vec<(usize, Option<&str>)> =
            grids.iter().map(|&g| (g, direction(&l.d, g))).collect();
        let mut want = expected;
        if want.is_none() && !dirs.is_empty() {
            let mut counts: Vec<(Option<&str>, usize)> = vec![];
            for (_, v) in &dirs {
                match counts.iter_mut().find(|(k, _)| k == v) {
                    Some((_, c)) => *c += 1,
                    None => counts.push((*v, 1)),
                }
            }
            let best = counts.iter().map(|c| c.1).max().unwrap();
            want = counts.iter().find(|c| c.1 == best).unwrap().0;
        }
        let wrong: Vec<usize> = dirs
            .iter()
            .filter(|(_, v)| v.is_some() && *v != want)
            .map(|(g, _)| *g)
            .collect();
        if !wrong.is_empty() {
            l.add(
                "R1-06",
                "error",
                format!(
                    "{}{wher} not perpendicular to the direction of “more”: gridlines mark the steps of the value scale, and none runs along a categorical scale (G3.6)",
                    count(wrong.len(), "gridline")
                ),
                &wrong,
                None,
            );
        }
        let zeros: Vec<usize> = grids
            .iter()
            .copied()
            .filter(|&g| l.n(g).has("zero") && !l.n(g).has("signed"))
            .collect();
        let others: Vec<usize> = grids
            .iter()
            .copied()
            .filter(|&g| !l.n(g).has("zero"))
            .collect();
        if !zeros.is_empty() && !others.is_empty() {
            let look = |g: usize| {
                (
                    l.d.paint(g, "stroke"),
                    py_round(l.d.number(g, "stroke-width"), 2),
                )
            };
            if look(zeros[0]) != look(others[0]) {
                l.add(
                    "R1-07",
                    "warn",
                    format!(
                        "the zero line{wher} is drawn unlike the other gridlines: it is the gridline for 0, darker only in a signed range or on a change or index scale (G3.7, D-17)"
                    ),
                    &zeros,
                    None,
                );
            }
        }
    }
}

pub fn non_data(l: &mut Lint) {
    let data: BTreeSet<String> = l.d.marks.iter().flat_map(|&n| l.d.colours(n)).collect();
    let mut groups: Vec<((&str, String, String), Vec<usize>)> = vec![];
    for &n in &l.d.drawn {
        let Some(role) = NON_DATA.iter().copied().find(|c| l.n(n).has(c)) else {
            continue;
        };
        if l.n(n).has("signed") {
            continue;
        }
        let cols = l.d.paint(n, "stroke");
        let w = l.d.number(n, "stroke-width");
        if !cols.is_empty() && w > 0. {
            group(
                &mut groups,
                (role, cols[0].clone(), format!("{}", py_round(w, 3))),
                n,
            );
        }
    }
    for ((role, c, w), ns) in groups {
        let w: f64 = w.parse().unwrap();
        let mut why = vec![];
        let k = contrast(&c, &l.d.page).unwrap_or(1.);
        if k > NON_DATA_CONTRAST {
            why.push(format!(
                "contrast {k:.2} : 1 against the page (at most {NON_DATA_CONTRAST} : 1)"
            ));
        }
        let lc = luminance(&c).unwrap_or(0.);
        if let Some(darker) = data.iter().find(|x| luminance(x).unwrap_or(0.) >= lc) {
            why.push(format!("not lighter than the data colour {darker}"));
        }
        if matches!(role, "gridline" | "time-divider") && w > THIN * l.d.em {
            why.push(format!("{} wide (about 0.08 em is thin)", fmt_g(w)));
        }
        if !why.is_empty() {
            l.add(
                "R1-02",
                "warn",
                format!(
                    "{} in {c}: {}: non-data lines must not draw the eye as much as the data (G3.2, D-34)",
                    count(ns.len(), &role.replace('-', " ")),
                    why.join("; ")
                ),
                &ns,
                Some("locked"),
            );
        }
    }
}

pub fn text_orientation(l: &mut Lint) {
    let mut turned_: Vec<(usize, Option<i64>)> = vec![];
    for t in &l.d.texts {
        let n = l.n(t.node);
        let m = n.ctm;
        let vertical =
            l.d.value(t.node, "writing-mode")
                .is_some_and(|v| v.starts_with("tb") || v.starts_with("vertical"));
        let rotated =
            n.el.descendants()
                .any(|e| e.is_element() && e.attribute("rotate").is_some_and(|v| !v.is_empty()));
        if turned(&m) || vertical || rotated {
            turned_.push((
                t.node,
                turned(&m).then(|| py_round(m[1].atan2(m[0]).to_degrees(), 0) as i64),
            ));
        }
    }
    if !turned_.is_empty() {
        let angles: BTreeSet<i64> = turned_.iter().filter_map(|t| t.1).collect();
        let how = if angles.is_empty() {
            String::new()
        } else {
            format!(
                " (turned {})",
                angles
                    .iter()
                    .map(|a| format!("{a}°"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let nodes: Vec<usize> = turned_.iter().map(|t| t.0).collect();
        l.add(
            "R2-08",
            "error",
            format!(
                "{} not horizontal{how}: text is always set horizontally; wrap it onto two lines or turn the chart, never the text (G6.6)",
                count(nodes.len(), "text")
            ),
            &nodes,
            None,
        );
    }
}

pub fn text_size(l: &mut Lint) {
    let mut small: Vec<((String, String, String), Vec<usize>)> = vec![];
    for t in &l.d.texts {
        let m = l.n(t.node).ctm;
        let fin = t.size * (m[0] * m[3] - m[1] * m[2]).abs().sqrt() * l.d.scale;
        let note = NOTE_ROLES.contains(&t.role.as_str()) || l.n(t.node).has("inset-text");
        let floor = if note {
            NOTE_FLOOR * l.d.em * l.d.scale
        } else {
            LABEL_FLOOR
        };
        if fin < floor - 1e-6 {
            group(
                &mut small,
                (
                    t.role.clone(),
                    fmt_g(py_round(fin, 2)),
                    fmt_g(py_round(floor, 2)),
                ),
                t.node,
            );
        }
    }
    for ((role, size, floor), ns) in small {
        l.add(
            "R5-7",
            "warn",
            format!(
                "{} at {size} pt at final size (the floor is {floor} pt): text must be large enough for readers with poor sight (G6.4, D-6)",
                count(ns.len(), &role.replace('-', " "))
            ),
            &ns,
            None,
        );
    }
}

fn background(d: &Drawing, t: &TextItem) -> String {
    let (x, y) = d.point(t);
    let mut under = d.page.clone();
    let order = d.nodes[t.node].order;
    for &n in &d.drawn {
        if d.nodes[n].order >= order
            || !matches!(
                d.nodes[n].tag.as_str(),
                "rect" | "use" | "circle" | "ellipse"
            )
        {
            continue;
        }
        if let Some(b) = d.bx(n) {
            if b[0] <= x && x <= b[2] && b[1] <= y && y <= b[3] {
                if let Some(f) = d.paint(n, "fill").into_iter().next() {
                    under = f;
                }
            }
        }
    }
    under
}

pub fn text_contrast(l: &mut Lint) {
    let mut low: Vec<((String, String, String), Vec<usize>)> = vec![];
    for t in &l.d.texts {
        let Some((c, _)) = crate::css::colour(Some(&t.fill)) else {
            continue;
        };
        let bg = background(&l.d, t);
        let k = contrast(&c, &bg).unwrap_or(21.);
        if k < TEXT_CONTRAST {
            group(
                &mut low,
                (t.fill.clone(), bg, format!("{:.2}", py_round(k, 2))),
                t.node,
            );
        }
    }
    for ((fill, bg, k), ns) in low {
        l.add(
            "R5-8",
            "warn",
            format!(
                "{} in {fill} on {bg}: contrast {k} : 1 (at least {TEXT_CONTRAST} : 1): text close to its background is hard to read (G6.5, D-6)",
                count(ns.len(), "text")
            ),
            &ns,
            Some("locked"),
        );
    }
}

fn joint(d: &Drawing, a: &TextItem, b: &TextItem) -> bool {
    if (d.nodes[a.node].has("text-halo") || d.nodes[b.node].has("text-halo")) && a.code == b.code {
        return true;
    }
    let roles: BTreeSet<&str> = [a.role.as_str(), b.role.as_str()].into();
    if roles.contains("note-mark") {
        if let (Some(ab), Some(bb)) = (a.bx, b.bx) {
            if ab[1] < bb[3] && bb[1] < ab[3] {
                let (m, o) = if a.role == "note-mark" {
                    (a, b)
                } else {
                    (b, a)
                };
                let (mb, ob) = (m.bx.unwrap(), o.bx.unwrap());
                return (mb[0] - ob[2]).abs() < 0.25 * m.size
                    || (0. <= ob[0] - mb[2] && ob[0] - mb[2] < 0.6 * m.size);
            }
        }
    }
    roles == ["scale-label", "unit-phrase"].into() && (a.y - b.y).abs() < 0.5
}

pub fn collisions(l: &mut Lint) {
    let pad = l.text_pad.unwrap_or(0.15 * l.d.em) / 2.;
    let grow = |b: Box4, p: f64| [b[0] - p, b[1] - p, b[2] + p, b[3] + p];
    let boxed: Vec<usize> = (0..l.d.texts.len())
        .filter(|&k| l.d.texts[k].bx.is_some())
        .collect();
    let mut adds = vec![];
    for (i, &ka) in boxed.iter().enumerate() {
        for &kb in &boxed[i + 1..] {
            let (a, b) = (&l.d.texts[ka], &l.d.texts[kb]);
            if joint(&l.d, a, b) || l.d.within(a.node, "inset") != l.d.within(b.node, "inset") {
                continue;
            }
            if overlap(&grow(a.bx.unwrap(), pad), &grow(b.bx.unwrap(), pad)) {
                adds.push((
                    format!(
                        "“{}” ({}) and “{}” ({}) overlap or touch: text must stay clear of text",
                        a.plain, a.role, b.plain, b.role
                    ),
                    vec![a.node, b.node],
                ));
            }
        }
    }
    let solid: Vec<usize> =
        l.d.marks
            .iter()
            .copied()
            .filter(|&n| matches!(l.n(n).tag.as_str(), "rect" | "use" | "circle" | "ellipse"))
            .collect();
    for &k in &boxed {
        let t = &l.d.texts[k];
        if matches!(t.role.as_str(), "value-label" | "tile-label") {
            continue;
        }
        if t.role == "note-mark" && l.n(t.node).has("on-fill") {
            continue;
        }
        let tb = t.bx.unwrap();
        for &n in &solid {
            if l.d.within(n, "inset") != l.d.within(t.node, "inset") {
                continue;
            }
            let Some(b) = l.d.bx(n) else { continue };
            let kind = l.n(n).kind().unwrap_or("");
            let cy = (tb[1] + tb[3]) / 2.;
            let cx = (tb[0] + tb[2]) / 2.;
            if matches!(kind, "cell" | "cell-bar")
                && b[1] <= cy
                && cy <= b[3]
                && (kind == "cell-bar" || (b[0] <= cx && cx <= b[2]))
            {
                continue;
            }
            if b[2] - b[0] > 0. && b[3] - b[1] > 0. && overlap(&tb, &b) {
                adds.push((
                    format!(
                        "“{}” ({}) lies over a {kind}: text must not hide the data",
                        t.plain, t.role
                    ),
                    vec![t.node, n],
                ));
            }
        }
    }
    for (m, ns) in adds {
        l.add("CG-1", "error", m, &ns, None);
    }
}

fn style_desc(k: &(String, String, Option<String>, Option<String>)) -> String {
    let size: f64 = k.0.parse().unwrap();
    format!(
        "{} pt {}{}{}",
        fmt_g(size),
        k.1,
        k.2.as_ref().map(|w| format!(" {w}")).unwrap_or_default(),
        k.3.as_ref().map(|s| format!(" {s}")).unwrap_or_default()
    )
}

pub fn text_styles(l: &mut Lint) {
    let mut styles: Vec<((String, String, Option<String>, Option<String>), Vec<usize>)> = vec![];
    for (k, t) in l.d.texts.iter().enumerate() {
        if t.role != "comment" {
            group(&mut styles, Drawing::style_key(t), k);
        }
    }
    if styles.len() > 4 {
        let desc = styles
            .iter()
            .map(|(s, ts)| {
                let roles: BTreeSet<&str> =
                    ts.iter().map(|&k| l.d.texts[k].role.as_str()).collect();
                format!(
                    "{} ({})",
                    style_desc(s),
                    roles.into_iter().collect::<Vec<_>>().join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let nodes: Vec<usize> = styles.iter().map(|(_, ts)| l.d.texts[ts[0]].node).collect();
        l.add(
            "R5-6",
            "warn",
            format!(
                "{} text styles in use (at most four): {desc} (G6.2)",
                styles.len()
            ),
            &nodes,
            Some("locked"),
        );
    }
}

pub fn subtitle(l: &mut Lint) {
    let subs: Vec<usize> = (0..l.d.texts.len())
        .filter(|&k| l.d.texts[k].role == "subtitle")
        .collect();
    if subs.is_empty() {
        return;
    }
    let label = match l.d.texts.iter().find(|t| {
        matches!(
            t.role.as_str(),
            "scale-label" | "category-label" | "time-label"
        )
    }) {
        Some(t) => Drawing::style_key(t),
        None => {
            let (s, f, w, sl) = crate::css::label_style(&l.d.sheet, &["prose".into()], &|_| None);
            (
                format!("{}", py_round(s, 2)),
                f,
                w.map(|w| w.to_string()),
                sl,
            )
        }
    };
    let odd: Vec<usize> = subs
        .iter()
        .filter(|&&k| Drawing::style_key(&l.d.texts[k]) != label)
        .map(|&k| l.d.texts[k].node)
        .collect();
    if !odd.is_empty() {
        l.add(
            "R5-27",
            "warn",
            "the line under the title is styled apart from the labels: the period and breakdown belong to the title block, in the label style, adding no text style (G6.8, D-5)".into(),
            &odd,
            None,
        );
    }
}

fn hue_gap(a: f64, b: f64) -> f64 {
    let g = (a - b).abs() % 360.;
    g.min(360. - g)
}

fn chroma(c: &str) -> [f64; 3] {
    oklch(c).unwrap_or([0., 0., 0.])
}

pub fn colours(l: &mut Lint) {
    let d = &l.d;
    let reg: Vec<usize> = d
        .marks
        .iter()
        .copied()
        .filter(|&n| {
            let stroked = d.nodes[n].kind().is_some_and(|k| STROKED.contains(&k));
            l.regular(n) && !d.gradient(n, if stroked { "stroke" } else { "fill" })
        })
        .collect();
    let mut used: Vec<(String, Vec<usize>)> = vec![];
    // series key: Some(Ok(series)) / Some(Err(colour)) for a part / None
    let mut by_series: Vec<(Option<Result<String, String>>, Vec<(String, usize)>)> = vec![];
    for &n in &reg {
        for c in d.colours(n) {
            group(&mut used, c.clone(), n);
            let key = if d.nodes[n].kind().is_some_and(|k| PARTS.contains(&k)) {
                Some(Err(c.clone()))
            } else {
                d.series_key(n).map(Ok)
            };
            let entry = match by_series.iter_mut().position(|(k, _)| *k == key) {
                Some(p) => &mut by_series[p].1,
                None => {
                    by_series.push((key, vec![]));
                    &mut by_series.last_mut().unwrap().1
                }
            };
            match entry.iter_mut().find(|(x, _)| *x == c) {
                Some((_, k)) => *k += 1,
                None => entry.push((c.clone(), 1)),
            }
        }
    }
    let legend = d.drawn.iter().any(|&n| d.nodes[n].has("legend"));
    let ends = d
        .texts
        .iter()
        .filter(|t| matches!(t.role.as_str(), "end-label" | "slice-label" | "tile-label"))
        .count();
    let keyed = by_series.iter().filter(|(k, _)| k.is_some()).count();
    let direct = !legend && ends >= keyed.max(1);
    let k = used.len();
    let firsts: Vec<usize> = used.iter().map(|(_, ns)| ns[0]).collect();
    let mut adds: Vec<(&str, &str, String, Vec<usize>)> = vec![];
    if k > 8 {
        if direct {
            adds.push(("R3-22", "warn", format!("{k} regular colours, every series labelled directly: check that similar colours never sit side by side (G4.20)"), firsts.clone()));
        } else {
            adds.push(("R3-21", "error", format!("{k} regular colours: beyond eight they cannot be told apart; drop, group or merge categories, or label every series directly (G4.19, G4.21, D-14)"), firsts.clone()));
        }
    } else if k == 8 && legend {
        adds.push((
            "R3-21",
            "warn",
            "eight regular colours keyed by a legend: at the limit (G4.19, D-14)".into(),
            firsts.clone(),
        ));
    }
    for (c, ns) in &used {
        let [_, ch, h] = chroma(c);
        if ch > MAX_CHROMA {
            let shown = if format!("{ch:.3}") != format!("{MAX_CHROMA:.3}") {
                format!("{ch:.3}")
            } else {
                format!("{ch:.4}")
            };
            adds.push(("R3-2", "warn", format!("{c} has OKLCH chroma {shown} (at most {MAX_CHROMA}): saturated colours are tiring; use muted ones (G4.2, D-34)"), vec![ns[0]]));
        }
        if ch < GREY_CHROMA {
            adds.push(("R3-16", "warn", format!("{c} is grey or black in a regular series: grey and black are reserved for total, average, all others and missing (G4.15)"), vec![ns[0]]));
        } else if (10. ..40.).contains(&h) && ch >= 0.1 {
            adds.push(("R3-18", "warn", format!("{c} is red in a regular series: red reads as bad news; keep it out of categorical palettes (G4.17)"), vec![ns[0]]));
        }
    }
    let reps: Vec<String> = by_series
        .iter()
        .filter(|(k, _)| k.is_some())
        .map(|(_, cs)| {
            let best = cs.iter().map(|c| c.1).max().unwrap();
            cs.iter().find(|c| c.1 == best).unwrap().0.clone()
        })
        .collect();
    let together: Vec<String> = if reps.len() >= 2 {
        reps.iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    } else {
        vec![]
    };
    let later = |n: usize| (6..17).any(|k| d.nodes[n].has(&format!("series-{k}")));
    let tier1: Vec<String> = reg
        .iter()
        .filter(|&&n| !later(n) && !d.nodes[n].has_any(&["shade-light", "shade-dark"]))
        .flat_map(|&n| d.colours(n))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|c| together.contains(c))
        .collect();
    let first_of = |c: &str| used.iter().find(|(x, _)| x == c).map(|(_, ns)| ns[0]);
    if (2..=5).contains(&tier1.len()) {
        let ls: Vec<f64> = tier1.iter().map(|c| chroma(c)[0]).collect();
        let lo = (0..ls.len()).fold(0, |b, i| if ls[i] < ls[b] { i } else { b });
        let hi = (0..ls.len()).fold(0, |b, i| if ls[i] > ls[b] { i } else { b });
        if ls[hi] - ls[lo] > MAX_SPREAD {
            adds.push((
                "R3-6",
                "warn",
                format!(
                    "the colours in use spread {:.3} in OKLCH lightness ({} to {}; at most {MAX_SPREAD}): none should be much darker or lighter than the rest (G4.6, D-34)",
                    ls[hi] - ls[lo],
                    tier1[lo],
                    tier1[hi]
                ),
                vec![first_of(&tier1[lo]).unwrap(), first_of(&tier1[hi]).unwrap()],
            ));
        }
    }
    if l.cvd != "no" {
        let mut pairs = BTreeSet::new();
        for i in 0..together.len() {
            for j in i + 1..together.len() {
                pairs.insert((together[i].clone(), together[j].clone()));
            }
        }
        let side_marks = |side: &str, ramp: bool| -> Vec<usize> {
            d.marks
                .iter()
                .copied()
                .filter(|&n| {
                    d.nodes[n].has(side) && d.nodes[n].classes.iter().any(|c| ramp_class(c)) == ramp
                })
                .collect()
        };
        let mut good: BTreeSet<String> = side_marks("good", false)
            .into_iter()
            .flat_map(|n| d.colours(n))
            .collect();
        let mut bad: BTreeSet<String> = side_marks("bad", false)
            .into_iter()
            .flat_map(|n| d.colours(n))
            .collect();
        for (side, into) in [("good", &mut good), ("bad", &mut bad)] {
            let steps: Vec<(u32, Vec<String>)> = side_marks(side, true)
                .into_iter()
                .map(|n| {
                    let k = d.nodes[n]
                        .classes
                        .iter()
                        .filter(|c| ramp_class(c))
                        .filter_map(|c| c.get(5..).and_then(|x| x.parse::<u32>().ok()))
                        .max()
                        .unwrap_or(0);
                    (k, d.colours(n))
                })
                .collect();
            if let Some(top) = steps.iter().map(|s| s.0).max() {
                into.extend(steps.iter().find(|s| s.0 == top).unwrap().1.iter().cloned());
            }
        }
        let cued = d.drawn.iter().any(|&n| d.nodes[n].has("texture"))
            || d.marks.iter().any(|&n| d.nodes[n].has("filled"));
        let gbm: Vec<usize> = d
            .marks
            .iter()
            .copied()
            .filter(|&n| d.nodes[n].has_any(&["good", "bad"]))
            .collect();
        let valued = (!good.is_empty() || !bad.is_empty())
            && gbm.iter().all(|&n| d.nodes[n].has("labelled"));
        let mut gb = BTreeSet::new();
        if !cued {
            for g in &good {
                for b in &bad {
                    gb.insert(if g <= b {
                        (g.clone(), b.clone())
                    } else {
                        (b.clone(), g.clone())
                    });
                }
            }
        }
        let worst_of = |ps: &BTreeSet<(String, String)>| -> Option<(f64, String, String, &str)> {
            let mut worst: Option<(f64, String, String, &str)> = None;
            for (a, b) in ps {
                for (name, m) in CVD.iter() {
                    let dist = cvd_distance(a, b, m).unwrap_or(1.);
                    if worst.as_ref().is_none_or(|w| dist < w.0) {
                        worst = Some((dist, a.clone(), b.clone(), name));
                    }
                }
            }
            worst.filter(|w| w.0 < CVD_DISTANCE)
        };
        if let Some((dist, a, b, name)) = worst_of(&pairs) {
            let els: Vec<usize> = [first_of(&a), first_of(&b)].into_iter().flatten().collect();
            adds.push((
                "R3-19",
                if direct { "info" } else { "warn" },
                format!(
                    "{a} and {b} are {dist:.3} apart under {name} (at least {CVD_DISTANCE}): readers with colour-vision deficiency may not tell them apart{} (G4.18, D-34)",
                    if direct { " (every series is labelled directly)" } else { "" }
                ),
                els,
            ));
        }
        if let Some((dist, x, y, name)) = worst_of(&gb) {
            let (g, b) = if good.contains(&x) && bad.contains(&y) {
                (x, y)
            } else {
                (y, x)
            };
            let first = |side: &str, c: &str| {
                d.marks
                    .iter()
                    .copied()
                    .find(|&n| d.nodes[n].has(side) && d.colours(n).iter().any(|x| x == c))
            };
            let els: Vec<usize> = [first("good", &g), first("bad", &b)]
                .into_iter()
                .flatten()
                .collect();
            adds.push((
                "R3-19",
                "info",
                format!(
                    "good {g} and bad {b} are {dist:.3} apart under {name} (at least {CVD_DISTANCE}): semantic colours for every reader (D-18, owner 1 Oct 2026){}; the black-and-white version (audience.bw_risk) tells them apart without colour (G4.18)",
                    if valued { ", each cell carrying its value" } else { "" }
                ),
                els,
            ));
        }
    }
    for &n in &d.marks {
        let node = &d.nodes[n];
        if node.has_any(SPECIAL) && !node.has("highlight") {
            for c in d.colours(n) {
                if chroma(&c)[1] > SPECIAL_CHROMA {
                    let sp: BTreeSet<&str> = node
                        .classes
                        .iter()
                        .map(String::as_str)
                        .filter(|c| SPECIAL.contains(c))
                        .collect();
                    adds.push((
                        "R3-16",
                        "warn",
                        format!(
                            "a {} mark of a special category ({}) in {c}: special categories take their reserved greys (G4.15)",
                            node.role(),
                            sp.into_iter().collect::<Vec<_>>().join(", ")
                        ),
                        vec![n],
                    ));
                    break;
                }
            }
        }
    }
    for (r, s, m, ns) in adds {
        l.add(r, s, m, &ns, None);
    }
}

pub fn series_colours(l: &mut Lint) {
    let d = &l.d;
    let mut groups: Vec<(Option<String>, Vec<usize>)> = vec![];
    for &n in &d.marks {
        if l.regular(n) && !d.nodes[n].kind().is_some_and(|k| PARTS.contains(&k)) {
            group(&mut groups, d.series_key(n), n);
        }
    }
    let mut adds = vec![];
    for (key, ns) in groups {
        let Some(key) = key else { continue };
        let cols: Vec<String> = ns.iter().flat_map(|&n| d.colours(n)).collect();
        let distinct: Vec<String> = cols
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if distinct.len() < 2 {
            continue;
        }
        let stroked = |n: usize| d.nodes[n].kind().is_some_and(|k| STROKED.contains(&k));
        let grad: Vec<usize> = ns
            .iter()
            .copied()
            .filter(|&n| d.gradient(n, if stroked(n) { "stroke" } else { "fill" }))
            .collect();
        let hues: Vec<[f64; 3]> = distinct.iter().map(|c| chroma(c)).collect();
        let chromatic: Vec<f64> = hues
            .iter()
            .filter(|h| h[1] >= SPECIAL_CHROMA)
            .map(|h| h[2])
            .collect();
        let one_hue = chromatic.is_empty()
            || chromatic
                .iter()
                .flat_map(|a| chromatic.iter().map(move |b| hue_gap(*a, *b)))
                .fold(0., f64::max)
                <= 20.;
        let mut valued: Vec<(f64, f64)> = ns
            .iter()
            .filter_map(|&n| {
                let v: f64 = d.nodes[n].get("data-value")?.parse().ok()?;
                let c = d.colours(n).into_iter().next()?;
                Some((v, chroma(&c)[0]))
            })
            .collect();
        valued.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let monotone = valued.len() >= 3
            && (valued.windows(2).all(|w| w[0].1 >= w[1].1)
                || valued.windows(2).all(|w| w[0].1 <= w[1].1));
        let how = if !grad.is_empty() {
            format!(
                "with a gradient ({} to {})",
                distinct[0],
                distinct[distinct.len() - 1]
            )
        } else if one_hue && monotone {
            "in shades that follow its values".into()
        } else {
            format!("in {} colours", distinct.len())
        };
        let name = if key == "single" {
            "the series".into()
        } else {
            format!("the series {key}")
        };
        let mut els = vec![ns[0]];
        if let Some(&g) = grad.first() {
            if g != ns[0] {
                els.push(g);
            }
        }
        adds.push((format!("{name} is drawn {how}: position and length already show the values, and a colour difference that means nothing sends readers looking for a meaning (G4.4)"), els));
    }
    for (m, ns) in adds {
        l.add("R3-4", "error", m, &ns, None);
    }
}

fn thickness(d: &Drawing, n: usize, o: &str) -> Option<f64> {
    let b = d.bx(n)?;
    Some(if o == "vertical" {
        b[2] - b[0]
    } else {
        b[3] - b[1]
    })
}

fn bar_orientation(d: &Drawing, bars: &[usize]) -> &'static str {
    if let Some(o) = d.orientation {
        return o;
    }
    let boxes: Vec<Box4> = bars.iter().filter_map(|&n| d.bx(n)).collect();
    let distinct = |k: usize| {
        boxes
            .iter()
            .map(|b| format!("{:.1}", py_round(b[k], 1)))
            .collect::<BTreeSet<_>>()
            .len()
    };
    if boxes.len() >= 2 {
        if (distinct(0) == 1 || distinct(2) == 1) && !(distinct(3) == 1 || distinct(1) == 1) {
            return "horizontal";
        }
        return "vertical";
    }
    if let Some(b) = boxes.first() {
        return if b[3] - b[1] >= b[2] - b[0] {
            "vertical"
        } else {
            "horizontal"
        };
    }
    "vertical"
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.
    }
}

pub fn highlight(l: &mut Lint) {
    let d = &l.d;
    let mut adds = vec![];
    for &h in d.marks.iter().filter(|&&n| d.nodes[n].has("highlight")) {
        let kind = d.nodes[h].kind().unwrap();
        let prop = if STROKED.contains(&kind) {
            "stroke"
        } else {
            "fill"
        };
        let now = d.colours(h);
        let key = d.series_key(h);
        let mut peers: Vec<usize> = d
            .marks
            .iter()
            .copied()
            .filter(|&n| {
                d.nodes[n].kind() == Some(kind)
                    && d.series_key(n) == key
                    && !d.nodes[n].has("highlight")
                    && !PARTS.contains(&kind)
            })
            .collect();
        peers.sort_by_key(|&n| d.nodes[n].has("context"));
        let base = if let Some(&p) = peers.first() {
            d.colours(p)
        } else {
            let cls: Vec<String> = d.nodes[h]
                .classes
                .iter()
                .filter(|c| *c != "highlight")
                .cloned()
                .collect();
            d.paint_as(h, &cls, prop)
        };
        if let (Some(a), Some(b)) = (now.first(), base.first()) {
            let (c1, c0) = (chroma(a), chroma(b));
            if c1[1] >= SPECIAL_CHROMA
                && c0[1] >= SPECIAL_CHROMA
                && hue_gap(c1[2], c0[2]) > HUE_TOLERANCE
            {
                adds.push((format!("a highlighted {kind} in {a}, a new hue beside its own {b}: highlight by a stronger shade of the mark's own hue (G4.26, G8.10)"), vec![h]));
            }
        }
        if kind == "bar" {
            let peers: Vec<usize> = d
                .marks
                .iter()
                .copied()
                .filter(|&n| {
                    d.nodes[n].kind() == Some("bar")
                        && !d.nodes[n].has("highlight")
                        && d.panel_of(n) == d.panel_of(h)
                })
                .collect();
            let mut all = peers.clone();
            all.push(h);
            let o = bar_orientation(d, &all);
            let mut th: Vec<f64> = peers
                .iter()
                .filter_map(|&n| thickness(d, n, o))
                .filter(|t| *t != 0.)
                .collect();
            let mine = thickness(d, h, o).filter(|t| *t != 0.);
            if let (false, Some(mine)) = (th.is_empty(), mine) {
                let med = median(&mut th);
                if (mine - med).abs() > 0.5 {
                    adds.push((format!("a highlighted bar {mine:.1} wide against {med:.1}: a bigger mark reads as a bigger value; highlight by colour (G8.10)"), vec![h]));
                }
            }
        }
    }
    for (m, ns) in adds {
        l.add("R15-6", "error", m, &ns, None);
    }
    let grey: Vec<usize> =
        l.d.drawn
            .iter()
            .copied()
            .filter(|&n| l.d.nodes[n].has("uniform-context"))
            .collect();
    if !grey.is_empty() {
        l.add(
            "R6-28",
            "error",
            format!("{} greyed out alike: context series keep their own colours, or crossing lines cannot be told apart (G4.27)", count(grey.len(), "mark")),
            &grey,
            None,
        );
    }
}

fn axis(d: &Drawing, t: &TextItem, o: &str) -> f64 {
    let (x, y) = d.point(t);
    if o == "vertical" { x } else { y }
}

pub fn empty_bars(l: &mut Lint) {
    let d = &l.d;
    let mut adds = vec![];
    for (_, nodes) in d.panels() {
        let bars: Vec<usize> = nodes
            .iter()
            .copied()
            .filter(|&n| {
                matches!(d.nodes[n].kind(), Some("bar" | "segment")) && d.marks.contains(&n)
            })
            .collect();
        if bars.is_empty() {
            continue;
        }
        let o = bar_orientation(d, &bars);
        let mut span: Vec<(usize, (f64, f64), f64)> = vec![];
        for &n in &bars {
            let Some(b) = d.bx(n) else { continue };
            let s = if o == "vertical" {
                (b[0], b[2])
            } else {
                (b[1], b[3])
            };
            let len = if o == "vertical" {
                b[3] - b[1]
            } else {
                b[2] - b[0]
            };
            span.push((n, s, len));
        }
        if span.is_empty() {
            continue;
        }
        let reach = span
            .iter()
            .map(|s| s.1.1 - s.1.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let notes: Vec<f64> = d
            .texts_in(&nodes, &["missing-note"])
            .into_iter()
            .map(|k| axis(d, &d.texts[k], o))
            .collect();
        let noted = |x: f64| notes.iter().any(|m| (x - m).abs() <= reach);
        let mut empty = vec![];
        let mut flat_slots = vec![];
        for (n, (lo, hi), len) in &span {
            if *len <= 0.01 {
                flat_slots.push((*lo, *hi));
                if !noted((lo + hi) / 2.) {
                    empty.push(*n);
                }
            }
        }
        let mut visible: Vec<(f64, f64)> = span
            .iter()
            .filter(|s| s.2 > 0.01)
            .map(|(_, (lo, hi), _)| (lo - (hi - lo) / 2., hi + (hi - lo) / 2.))
            .collect();
        for &n in &nodes {
            if d.nodes[n].kind() == Some("incomplete-mark") && d.marks.contains(&n) {
                if let Some(b) = d.bx(n) {
                    visible.push(if o == "vertical" {
                        (b[0], b[2])
                    } else {
                        (b[1], b[3])
                    });
                }
            }
        }
        for k in d.texts_in(&nodes, &["category-label", "time-label"]) {
            let x = axis(d, &d.texts[k], o);
            if flat_slots
                .iter()
                .any(|(lo, hi)| lo - 0.5 <= x && x <= hi + 0.5)
            {
                continue;
            }
            if !visible.iter().any(|(lo, hi)| *lo <= x && x <= *hi) && !noted(x) {
                empty.push(d.texts[k].node);
            }
        }
        if !empty.is_empty() {
            adds.push((format!("{} without a visible bar and without a note: say what the empty slot means, “(missing)”, “(zero)”, “(unknown)” … (G7.2)", count(empty.len(), "bar slot")), empty));
        }
    }
    for (m, ns) in adds {
        l.add("R9-7", "error", m, &ns, None);
    }
}

fn fit(pairs: &mut Vec<(f64, f64)>) -> Option<(f64, f64)> {
    pairs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if pairs.len() < 2 || pairs[pairs.len() - 1].0 == pairs[0].0 {
        return None;
    }
    let ((v0, c0), (v1, c1)) = (pairs[0], pairs[pairs.len() - 1]);
    let b = (c1 - c0) / (v1 - v0);
    Some((c0 - b * v0, b))
}

#[derive(Debug, Clone)]
pub struct Label {
    pub value: f64,
    pub decimals: usize,
    pub prefix: String,
    pub suffix: String,
    pub digits: usize,
    pub grouped: bool,
    pub text: String,
}

static LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?P<pre>[^\d.]*?)(?P<num>\d[\d,\x{2009}\x{202f}\x{a0}]*(?:\.\d+)?|\.\d+)(?P<suf>.*?)\s*$").unwrap()
});

/// A scale label read as a number with its prefix and suffix (svglint.read_label).
pub fn read_label(s: &str) -> Option<Label> {
    let c = LABEL.captures(s)?;
    let pre = &c["pre"];
    let num = &c["num"];
    let (whole, dec) = num.split_once('.').unwrap_or((num, ""));
    let digits: String = whole.chars().filter(|c| c.is_ascii_digit()).collect();
    let v: f64 = format!(
        "{}{}",
        if digits.is_empty() { "0" } else { &digits },
        if dec.is_empty() {
            String::new()
        } else {
            format!(".{dec}")
        }
    )
    .parse()
    .ok()?;
    let neg = pre.contains('−') || pre.contains('-');
    Some(Label {
        value: if neg { -v } else { v },
        decimals: dec.chars().count(),
        prefix: pre
            .chars()
            .filter(|c| !matches!(c, '−' | '+' | '-') && !c.is_whitespace())
            .collect(),
        suffix: c["suf"].trim().to_owned(),
        digits: digits.trim_start_matches('0').len().max(1),
        grouped: digits.chars().count() != whole.chars().count(),
        text: s.trim().to_owned(),
    })
}

fn scale_map(d: &Drawing, nodes: &[usize], o: &str) -> Option<(f64, f64)> {
    let mut pairs = vec![];
    for &g in nodes {
        if d.nodes[g].has("gridline") && d.nodes[g].get("data-value").is_some() {
            let v = crate::css::float(d.nodes[g].get("data-value"));
            let pts = d.points(g);
            if let (Some(v), Some(p)) = (v, pts.first()) {
                pairs.push((v, if o == "vertical" { p.1 } else { p.0 }));
            }
        }
    }
    if pairs
        .iter()
        .map(|p| p.0.to_bits())
        .collect::<BTreeSet<_>>()
        .len()
        >= 2
    {
        return fit(&mut pairs);
    }
    let mut pairs = vec![];
    for k in d.texts_in(nodes, &["scale-label"]) {
        let t = &d.texts[k];
        if let Some(lab) = read_label(&t.plain) {
            let (x, y) = apply(&d.nodes[t.node].ctm, t.x, t.y - 0.333 * t.size);
            pairs.push((lab.value, if o == "vertical" { y } else { x }));
        }
    }
    fit(&mut pairs)
}

fn zero_line(d: &Drawing, nodes: &[usize]) -> Option<usize> {
    nodes.iter().copied().find(|&g| {
        d.nodes[g].has("gridline")
            && (d.nodes[g].has("zero")
                || crate::css::float(d.nodes[g].get("data-value")) == Some(0.))
    })
}

pub fn bars_from_zero(l: &mut Lint) {
    let d = &l.d;
    let mut adds = vec![];
    for (panel, nodes) in d.panels() {
        if panel.is_some_and(|p| d.nodes[p].has("inset-panel")) {
            continue;
        }
        let bars: Vec<usize> = nodes
            .iter()
            .copied()
            .filter(|&n| {
                d.nodes[n].kind() == Some("bar") && d.marks.contains(&n) && !d.within(n, "inset")
            })
            .collect();
        if bars.is_empty() {
            continue;
        }
        let o = bar_orientation(d, &bars);
        let mut z = zero_line(d, &nodes)
            .and_then(|g| d.points(g).first().copied())
            .map(|p| if o == "vertical" { p.1 } else { p.0 });
        let f = scale_map(d, &nodes, o);
        if z.is_none() {
            z = f.map(|f| f.0);
        }
        let Some(z) = z else { continue };
        let mut off = vec![];
        let mut starts: Vec<f64> = vec![];
        for &n in &bars {
            let Some(b) = d.bx(n) else { continue };
            let ends = if o == "vertical" {
                [b[1], b[3]]
            } else {
                [b[0], b[2]]
            };
            let base = if (ends[0] - z).abs() <= (ends[1] - z).abs() {
                ends[0]
            } else {
                ends[1]
            };
            if (base - z).abs() > 0.5 {
                off.push(n);
                if let Some((a, s)) = f.filter(|f| f.1 != 0.) {
                    let v = py_round((base - a) / s, 6);
                    if !starts.contains(&v) {
                        starts.push(v);
                    }
                }
            }
        }
        if !off.is_empty() {
            starts.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let at = if starts.is_empty() {
                String::new()
            } else {
                format!(
                    " (they start at {})",
                    starts
                        .iter()
                        .map(|v| fmt_g(*v))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            adds.push((format!("{} not starting at zero{at}: a bar's length is its value, so bars start at zero; for small differences add an inset (G2.6, G2.7)", count(off.len(), "bar")), off));
        }
    }
    for (m, ns) in adds {
        l.add("R2-24", "error", m, &ns, None);
    }
}

/// The value scales: (group, inside, around).
fn scales(d: &Drawing) -> Vec<(Option<usize>, Vec<usize>, Vec<usize>)> {
    let groups: Vec<usize> = (0..d.nodes.len())
        .filter(|&n| d.nodes[n].has("scale") && d.nodes[n].has("value"))
        .collect();
    if groups.is_empty() {
        return d
            .panels()
            .into_iter()
            .map(|(p, nodes)| (p, nodes.clone(), nodes))
            .collect();
    }
    groups
        .into_iter()
        .map(|g| {
            let inside: Vec<usize> = d
                .drawn
                .iter()
                .copied()
                .filter(|&x| d.up(x).contains(&g))
                .collect();
            let panel = d.panel_of(g);
            let around = match panel {
                Some(p) => d
                    .drawn
                    .iter()
                    .copied()
                    .filter(|&x| d.panel_of(x) == Some(p))
                    .collect(),
                None => inside.clone(),
            };
            (Some(g), inside, around)
        })
        .collect()
}

fn read_scales(d: &Drawing) -> Vec<(Option<usize>, Vec<(usize, Label)>)> {
    let mut out = vec![];
    for (g, inside, _) in scales(d) {
        let read: Vec<(usize, Option<Label>)> = d
            .texts_in(&inside, &["scale-label"])
            .into_iter()
            .map(|k| (k, read_label(&d.texts[k].plain)))
            .collect();
        if read.len() >= 2 && read.iter().all(|r| r.1.is_some()) {
            out.push((g, read.into_iter().map(|(k, l)| (k, l.unwrap())).collect()));
        }
    }
    out
}

pub fn scale_labels(l: &mut Lint) {
    let d = &l.d;
    let mut adds: Vec<(&str, &str, String, Vec<usize>)> = vec![];
    for (_, read) in read_scales(d) {
        let labs: Vec<&Label> = read.iter().map(|r| &r.1).collect();
        let nodes: Vec<usize> = read.iter().map(|r| d.texts[r.0].node).collect();
        let texts = labs
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if labs
            .iter()
            .map(|l| l.decimals)
            .collect::<BTreeSet<_>>()
            .len()
            > 1
        {
            adds.push(("R2-21", "error", format!("scale labels with different numbers of decimals ({texts}): a set of numbers shares one format (G2.24)"), nodes.clone()));
        }
        let mut forms: Vec<((String, String), Vec<(usize, &Label)>)> = vec![];
        for (k, lab) in &read {
            group(
                &mut forms,
                (lab.prefix.clone(), lab.suffix.clone()),
                (*k, lab),
            );
        }
        if forms.len() > 1 {
            let best = forms.iter().map(|f| f.1.len()).max().unwrap();
            let common = forms.iter().find(|f| f.1.len() == best).unwrap().0.clone();
            let odd: Vec<(usize, &Label)> = forms
                .iter()
                .filter(|f| f.0 != common)
                .flat_map(|f| f.1.iter().copied())
                .collect();
            let vals: Vec<f64> = labs.iter().map(|l| l.value).collect();
            let mx = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let mn = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let ok = odd.len() == 1
                && (odd[0].1.value == mx || odd[0].1.value == mn)
                && odd[0].1.prefix.ends_with(&common.0)
                && odd[0].1.suffix.starts_with(&common.1);
            if !ok {
                adds.push(("R2-21", "error", format!("scale labels in different forms ({texts}): the same prefix and suffix on every label, or the unit once on the top label (G2.24, D-3)"), odd.iter().map(|o| d.texts[o.0].node).collect()));
            }
        }
        let ungrouped: Vec<usize> = read
            .iter()
            .filter(|r| r.1.digits >= 4 && !r.1.grouped)
            .map(|r| d.texts[r.0].node)
            .collect();
        if !ungrouped.is_empty() {
            adds.push(("R5-14", "error", format!("{} of four or more integer digits not grouped in threes: readers otherwise count digits (G6.14)", count(ungrouped.len(), "scale label")), ungrouped));
        }
        let mut vals: Vec<f64> = labs.iter().map(|l| l.value).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        vals.dedup();
        if vals.len() >= 3 {
            let steps: Vec<f64> = vals.windows(2).map(|w| w[1] - w[0]).collect();
            let s0 = steps[0];
            if steps
                .iter()
                .any(|s| (s - s0).abs() > 1e-9 * s0.abs().max(1.))
            {
                adds.push(("R2-10", "error", format!("unequal intervals ({}): every interval of a scale is the same size (G2.16)", vals.iter().map(|v| fmt_g(*v)).collect::<Vec<_>>().join(", ")), nodes.clone()));
            } else if s0 > 0. {
                let mant = py_round(s0 / 10f64.powf(s0.log10().floor()), 6);
                if mant == 2.5 {
                    adds.push(("R2-09", "warn", format!("a step of {} (2.5 × 10^n): 1, 2 or 5 × 10^n are easier to read (G2.15)", fmt_g(s0)), nodes[..1].to_vec()));
                } else if ![1., 2., 5.].contains(&mant) {
                    adds.push(("R2-09", "error", format!("a step of {}: steps of 1, 2 or 5 × 10^n are the easiest to count in (G2.15)", fmt_g(s0)), nodes[..1].to_vec()));
                }
            }
        }
        let pts: Vec<(f64, f64, f64)> = read
            .iter()
            .map(|(k, lab)| {
                let t = &d.texts[*k];
                let (x, y) = apply(&d.nodes[t.node].ctm, t.x, t.y);
                (lab.value, x, y)
            })
            .collect();
        let xs: Vec<f64> = pts.iter().map(|p| p.1).collect();
        let ys: Vec<f64> = pts.iter().map(|p| p.2).collect();
        let span = |v: &[f64]| {
            v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                - v.iter().cloned().fold(f64::INFINITY, f64::min)
        };
        let column = span(&xs) < span(&ys);
        let mut pairs: Vec<(f64, f64)> = pts
            .iter()
            .map(|p| (p.0, if column { p.2 } else { p.1 }))
            .collect();
        let orig = pairs.clone();
        if let Some((a, b)) = fit(&mut pairs) {
            if orig.len() >= 3 {
                let worst = orig
                    .iter()
                    .map(|(v, c)| (c - (a + b * v)).abs())
                    .fold(f64::NEG_INFINITY, f64::max);
                if worst > 0.6 {
                    let pos: Vec<f64> = vals.iter().copied().filter(|v| *v > 0.).collect();
                    let geometric = pos.len() >= 3
                        && pos
                            .windows(2)
                            .map(|w| format!("{}", py_round(w[1] / w[0], 6)))
                            .collect::<BTreeSet<_>>()
                            .len()
                            == 1;
                    let (rule, what) = if geometric {
                        ("R2-19", "a log scale")
                    } else {
                        ("R2-18", "a broken scale")
                    };
                    adds.push((rule, "error", format!("scale labels not placed in proportion to their values ({what}, off by up to {worst:.1} pt): a linear scale from zero, with an inset for small values (G1.61, G2.16, G2.35)"), nodes.clone()));
                }
            }
        }
    }
    for (r, s, m, ns) in adds {
        l.add(r, s, m, &ns, None);
    }
}

pub fn scale_count(l: &mut Lint) {
    let d = &l.d;
    let mut adds = vec![];
    for (_, read) in read_scales(d) {
        let nodes: Vec<usize> = read.iter().map(|r| d.texts[r.0].node).collect();
        if read.len() > 9 {
            adds.push((
                "R2-34",
                format!(
                    "{} scale labels (at most about eight): fewer intervals read faster (G2.17)",
                    read.len()
                ),
                vec![nodes[0]],
            ));
        }
        for (k, lab) in &read {
            let zeros = if lab.value != 0. {
                let s = format!("{}", lab.value.abs().trunc() as i128);
                s.len() - s.trim_end_matches('0').len()
            } else {
                0
            };
            if lab.decimals == 0 && zeros >= 4 {
                adds.push(("R2-05", format!("the label “{}” carries {zeros} trailing zeros: state the magnitude once instead (G2.22)", lab.text), vec![d.texts[*k].node]));
                break;
            }
        }
    }
    for (r, m, ns) in adds {
        l.add(r, "warn", m, &ns, None);
    }
}

static MAGNITUDE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(K|M|B|thousands?|millions?|billions?|trillions?)\b").unwrap());
static TITLE_UNIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\((?:[^)]*[$€£¥]|[^)]*\b(?:K|thousands?|millions?|billions?|trillions?)\b)")
        .unwrap()
});
static PERCENT_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\((?:[^)]*\s)?%\)|\(%").unwrap());

fn unit_marks(d: &Drawing, sc: &(Option<usize>, Vec<usize>, Vec<usize>)) -> bool {
    let (_, inside, around) = sc;
    let labels: Vec<Label> = d
        .texts_in(inside, &["scale-label"])
        .into_iter()
        .filter_map(|k| read_label(&d.texts[k].plain))
        .collect();
    if !labels.is_empty() {
        let mut top = &labels[0];
        for l in &labels {
            if l.value > top.value {
                top = l;
            }
        }
        if !top.prefix.is_empty() || (!top.suffix.is_empty() && top.suffix != "%") {
            return true;
        }
    }
    let mut units = d.texts_in(inside, UNIT_ROLES);
    if units.is_empty() {
        units = d.texts_in(around, UNIT_ROLES);
    }
    for k in units {
        let t = &d.texts[k];
        let text = t.plain.trim();
        if t.role == "scale-title" {
            if TITLE_UNIT.is_match(text) {
                return true;
            }
        } else if MAGNITUDE.is_match(text)
            || text
                .chars()
                .next()
                .is_some_and(|c| !c.is_alphabetic() && c != '(')
        {
            return true;
        }
    }
    false
}

pub fn unit_phrase(l: &mut Lint) {
    let d = &l.d;
    let scs = scales(d);
    let main: Vec<&(Option<usize>, Vec<usize>, Vec<usize>)> = scs
        .iter()
        .filter(|sc| sc.0.is_none_or(|g| !d.within(g, "inset")))
        .collect();
    let has_units = |sc: &(Option<usize>, Vec<usize>, Vec<usize>)| {
        !d.texts_in(&sc.1, UNIT_ROLES).is_empty() || !d.texts_in(&sc.2, UNIT_ROLES).is_empty()
    };
    let once = d.chart.iter().any(|c| c == "multiples" || c == "wrapped")
        && scs
            .iter()
            .any(|sc| !d.texts_in(&sc.1, UNIT_ROLES).is_empty());
    let mut adds = vec![];
    for sc in &scs {
        let (g, inside, around) = sc;
        let labels = d.texts_in(inside, &["scale-label"]);
        if labels.is_empty() {
            continue;
        }
        let mut units = d.texts_in(inside, UNIT_ROLES);
        if units.is_empty() {
            units = d.texts_in(around, UNIT_ROLES);
        }
        if units.is_empty() && d.chart.iter().any(|c| c == "merged") {
            units = d.texts_in(around, &["panel-title"]);
        }
        if !units.is_empty() || once {
            continue;
        }
        if g.is_some_and(|g| d.within(g, "inset")) && !main.is_empty() {
            let stated: Vec<&&(Option<usize>, Vec<usize>, Vec<usize>)> =
                main.iter().filter(|m| has_units(m)).collect();
            if !stated.is_empty() && !stated.iter().any(|m| unit_marks(d, m)) {
                continue;
            }
            if !stated.is_empty() {
                adds.push(("warn", "an inset's scale in bare numbers where the main scale's unit writes a currency, a magnitude or a suffix: restate the unit on the inset's top label or under its scale (G2.27)".to_owned(), vec![g.unwrap()]));
                continue;
            }
        }
        adds.push((
            "error",
            "a value scale without a unit phrase: say what the numbers count, horizontally, on the top label or under a horizontal scale, never in the title (G2.19)".to_owned(),
            vec![g.unwrap_or(d.texts[labels[0]].node)],
        ));
    }
    for (s, m, ns) in adds {
        l.add("R2-07", s, m, &ns, None);
    }
}

pub fn percent_labels(l: &mut Lint) {
    let d = &l.d;
    let scs = scales(d);
    let mut adds = vec![];
    for (g, read) in read_scales(d) {
        let sc = scs.iter().find(|sc| sc.0 == g).unwrap();
        let mut units = d.texts_in(&sc.1, UNIT_ROLES);
        if units.is_empty() {
            units = d.texts_in(&sc.2, UNIT_ROLES);
        }
        let said: Vec<usize> = units
            .into_iter()
            .filter(|&k| PERCENT_TITLE.is_match(&d.texts[k].plain))
            .collect();
        let Some(&first) = said.first() else { continue };
        let bare: Vec<usize> = read
            .iter()
            .filter(|r| r.1.suffix != "%")
            .map(|r| d.texts[r.0].node)
            .collect();
        if !bare.is_empty() {
            adds.push(("error", format!("a percentage scale with {} without “%” (the unit “{}”): every label carries “%”, horizontal scales too, and the title then drops “(%)” (G2.24; owner, 1 Oct 2026)", count(bare.len(), "label"), d.texts[first].plain), bare));
        } else {
            adds.push(("warn", format!("the labels carry “%” and the unit “{}” says it again: drop “(%)” from the title (owner, 1 Oct 2026)", d.texts[first].plain), vec![d.texts[first].node]));
        }
    }
    for (s, m, ns) in adds {
        l.add("R2-21", s, m, &ns, None);
    }
}

pub fn segment_meets_box(p: (f64, f64), q: (f64, f64), b: &Box4, pad: f64) -> bool {
    let (x0, y0, x1, y1) = (b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad);
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    let (mut t0, mut t1) = (0f64, 1f64);
    for (pk, qk) in [
        (-dx, p.0 - x0),
        (dx, x1 - p.0),
        (-dy, p.1 - y0),
        (dy, y1 - p.1),
    ] {
        if pk == 0. {
            if qk < 0. {
                return false;
            }
        } else {
            let t = qk / pk;
            if pk < 0. {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn crosses_outline(frame: &Box4, b: &Box4, width: f64) -> bool {
    let h = width / 2.;
    let outer = [frame[0] - h, frame[1] - h, frame[2] + h, frame[3] + h];
    let inner = [frame[0] + h, frame[1] + h, frame[2] - h, frame[3] - h];
    let inside = inner[0] <= b[0] && inner[1] <= b[1] && b[2] <= inner[2] && b[3] <= inner[3];
    overlap(&outer, b) && !inside
}

pub fn insets(l: &mut Lint) {
    let d = &l.d;
    let mut adds: Vec<(&str, String, Vec<usize>)> = vec![];
    let groups: Vec<usize> = (0..d.nodes.len())
        .filter(|&n| d.nodes[n].tag == "g" && d.nodes[n].has("inset"))
        .collect();
    for g in groups {
        let kids: Vec<usize> = d
            .drawn
            .iter()
            .copied()
            .filter(|&n| n != g && d.up(n).contains(&g))
            .collect();
        let pick = |c: &str| {
            kids.iter()
                .copied()
                .filter(|&n| d.nodes[n].has(c))
                .collect::<Vec<_>>()
        };
        let (src, frm, lnk) = (
            pick("inset-source"),
            pick("inset-frame"),
            pick("inset-link"),
        );
        let pnl: Vec<usize> = (0..d.nodes.len())
            .filter(|&n| d.nodes[n].has("inset-panel") && d.up(n).contains(&g))
            .collect();
        if (src.len(), frm.len(), lnk.len()) != (1, 1, 2) {
            adds.push(("error", format!("an inset with {}, {} and {}: one frame round the magnified marks, one round the small chart, two links joining them (G2.27)", count(src.len(), "source frame"), count(frm.len(), "inset frame"), count(lnk.len(), "link")), vec![g]));
            continue;
        }
        let ord = |n: usize| d.nodes[n].order as f64;
        let pmin = pnl.iter().map(|&p| ord(p)).fold(1e9, f64::min);
        if !(lnk
            .iter()
            .map(|&n| ord(n))
            .fold(f64::NEG_INFINITY, f64::max)
            < ord(src[0])
            && ord(src[0]) < ord(frm[0])
            && ord(frm[0]) < pmin)
        {
            adds.push(("error", "an inset drawn out of order: the links beneath the frames, the inset frame over the source frame, the small chart over its frame (G2.27)".into(), vec![g]));
        }
        let Some(fb) = d.bx(frm[0]) else { continue };
        let sw = d.number(frm[0], "stroke-width");
        let main_marks: Vec<usize> = d
            .marks
            .iter()
            .copied()
            .filter(|&n| !d.within(n, "inset"))
            .collect();
        let mut covered = vec![];
        for &n in &main_marks {
            if d.nodes[n].tag == "path" {
                let pts = d.points(n);
                if pts
                    .windows(2)
                    .any(|w| segment_meets_box(w[0], w[1], &fb, 0.))
                {
                    covered.push(n);
                }
            } else if let Some(b) = d.bx(n) {
                if b[2] > b[0] && b[3] > b[1] && overlap(&b, &fb) {
                    covered.push(n);
                }
            }
        }
        let texts: Vec<&TextItem> = d.texts.iter().filter(|t| t.bx.is_some()).collect();
        covered.extend(
            texts
                .iter()
                .filter(|t| !d.within(t.node, "inset") && overlap(&t.bx.unwrap(), &fb))
                .map(|t| t.node),
        );
        if !covered.is_empty() {
            adds.push(("error", format!("the inset frame covers {} of the main chart: an inset goes where it hides nothing, the main chart staying complete (G2.27, G2.29)", count(covered.len(), "mark or text")), covered));
        }
        let mut crossing = vec![];
        let solid: Vec<usize> = main_marks
            .iter()
            .copied()
            .filter(|&n| d.nodes[n].tag != "path")
            .collect();
        for &ln in &lnk {
            let pts = d.points(ln);
            if pts.len() < 2 {
                continue;
            }
            let (a, b) = (pts[0], pts[1]);
            let lw = d.number(ln, "stroke-width") / 2.;
            crossing.extend(
                texts
                    .iter()
                    .filter(|t| segment_meets_box(a, b, &t.bx.unwrap(), lw))
                    .map(|t| t.node),
            );
            crossing.extend(
                solid
                    .iter()
                    .copied()
                    .filter(|&n| d.bx(n).is_some_and(|bb| segment_meets_box(a, b, &bb, lw))),
            );
        }
        if let Some(sb) = d.bx(src[0]) {
            let w = d.number(src[0], "stroke-width");
            crossing.extend(
                texts
                    .iter()
                    .filter(|t| crosses_outline(&sb, &t.bx.unwrap(), w))
                    .map(|t| t.node),
            );
        }
        crossing.extend(
            texts
                .iter()
                .filter(|t| {
                    !d.within(t.node, "inset-panel") && crosses_outline(&fb, &t.bx.unwrap(), sw)
                })
                .map(|t| t.node),
        );
        if !crossing.is_empty() {
            let mut seen = HashSet::new();
            let uniq: Vec<usize> = crossing.into_iter().filter(|n| seen.insert(*n)).collect();
            adds.push(("error", format!("{} crossed by an inset's links or frames: they run clear of every text, and the links of every mark (G2.27)", count(uniq.len(), "text or mark")), uniq));
        }
        let plain: Vec<usize> = d
            .texts
            .iter()
            .filter(|t| d.within(t.node, "inset") && !d.nodes[t.node].has("inset-text"))
            .map(|t| t.node)
            .collect();
        if !plain.is_empty() {
            adds.push(("error", format!("{} of an inset without inset-text: the inset's chart is set at inset size, by that class (G2.27, SPEC §4.4)", count(plain.len(), "text")), plain));
        }
        for &p in &pnl {
            let nodes: Vec<usize> = d
                .drawn
                .iter()
                .copied()
                .filter(|&n| d.up(n).contains(&p))
                .collect();
            let bars: Vec<usize> = nodes
                .iter()
                .copied()
                .filter(|&n| d.nodes[n].kind() == Some("bar") && d.marks.contains(&n))
                .collect();
            let vals: Vec<f64> = nodes
                .iter()
                .filter(|&&n| d.nodes[n].has("gridline"))
                .filter_map(|&n| crate::css::float(d.nodes[n].get("data-value")))
                .collect();
            if bars.is_empty() || vals.is_empty() {
                continue;
            }
            let masked = |n: usize| {
                d.up(n).iter().any(|&a| {
                    d.nodes[a].has("inset-fade")
                        && d.nodes[a].get("mask").is_some_and(|m| !m.is_empty())
                })
            };
            if vals.iter().cloned().fold(f64::INFINITY, f64::min) > 0. {
                let bare: Vec<usize> = bars.iter().copied().filter(|&n| !masked(n)).collect();
                if !bare.is_empty() {
                    adds.push(("R9-6", format!("{} of a truncated inset without the fade: bars on a scale that does not start at zero fade to transparent over their lowest part (G2.28)", count(bare.len(), "bar")), bare));
                }
            } else if bars.iter().any(|&n| masked(n)) {
                adds.push(("R9-6", "the bars of an inset from zero faded: only a truncated inset's bars fade (G2.28)".into(), vec![bars.iter().copied().find(|&n| masked(n)).unwrap()]));
            }
        }
    }
    for (r, m, ns) in adds {
        if r == "R9-6" {
            l.add("R9-6", "error", m, &ns, None);
        } else {
            l.add("6.6", r, m, &ns, None);
        }
    }
}

pub fn flat(l: &mut Lint) {
    let d = &l.d;
    let fx: Vec<usize> = d
        .drawn
        .iter()
        .copied()
        .filter(|&n| d.value(n, "filter").unwrap_or_else(|| "none".into()).trim() != "none")
        .collect();
    let mut adds: Vec<(&str, String, Vec<usize>, Option<&str>)> = vec![];
    if !fx.is_empty() {
        adds.push(("RP6-1", format!("{} with filter effects: the plain style is flat, with no shadows, glows or blur (G3.9s)", count(fx.len(), "element")), fx, Some("style")));
    }
    let area = if d.width * d.height != 0. {
        d.width * d.height
    } else {
        1.
    };
    let mut fills = vec![];
    for &n in &d.drawn {
        let node = &d.nodes[n];
        if node.tag != "rect"
            || node.kind().is_some()
            || node.has("inset-frame")
            || FORBIDDEN.iter().any(|f| node.has(f.0))
            || node.has_any(&["highlight-band", "glow"])
        {
            continue;
        }
        let (Some(b), f) = (d.bx(n), d.paint(n, "fill")) else {
            continue;
        };
        if let Some(f0) = f.first() {
            if (b[2] - b[0]) * (b[3] - b[1]) >= 0.25 * area
                && (*f0 != d.page || d.gradient(n, "fill"))
            {
                fills.push(n);
            }
        }
    }
    if !fills.is_empty() {
        let c = d.paint(fills[0], "fill")[0].clone();
        adds.push(("RP6-1", format!("{} behind the plot or the page ({c}): the plain chart stands on the plain page, with no background fill, dark ground or gradient (G3.9s)", count(fills.len(), "fill")), fills, Some("style")));
    }
    let anim: Vec<usize> = (0..d.nodes.len())
        .filter(|&n| {
            matches!(
                d.nodes[n].tag.as_str(),
                "animate" | "animateTransform" | "animateMotion" | "set"
            )
        })
        .collect();
    if !anim.is_empty() {
        adds.push((
            "R14-1",
            format!(
                "{}: show every state in one still chart (G1.23)",
                count(anim.len(), "animation")
            ),
            anim,
            None,
        ));
    }
    for (r, m, ns, lv) in adds {
        l.add(r, "warn", m, &ns, lv);
    }
}

static NUMERIC_DATE_INNER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\d{1,2}[/.]\d{1,2}[/.]\d{2,4}|\d{1,2}/\d{4}|\d{4}/\d{1,2}|\d{1,2}/\d{1,2}|\d{4}-\d{2}(?:-\d{2})?)$").unwrap()
});
static SHORT_YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^['’]\d{2}\b").unwrap());

fn digit_or_dot(c: Option<char>) -> bool {
    static D: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\d.]$").unwrap());
    c.is_some_and(|c| D.is_match(c.encode_utf8(&mut [0; 4])))
}

/// `(?<![\d.])(…)(?![\d.])`: a numeric date anywhere in the text.
fn numeric_date(s: &str) -> bool {
    let idx: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([s.len()]).collect();
    for (a, &i) in idx.iter().enumerate() {
        if i == s.len() || digit_or_dot(s[..i].chars().last()) {
            continue;
        }
        for &j in &idx[a + 1..(a + 13).min(idx.len())] {
            if NUMERIC_DATE_INNER.is_match(&s[i..j]) && !digit_or_dot(s[j..].chars().next()) {
                return true;
            }
        }
    }
    false
}

fn short_year(s: &str) -> bool {
    s.char_indices().any(|(i, _)| {
        !s[..i]
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
            && SHORT_YEAR.is_match(&s[i..])
    })
}

pub fn dates(l: &mut Lint) {
    if l.date_format.is_some() {
        return;
    }
    let (mut numeric, mut short) = (vec![], vec![]);
    for t in &l.d.texts {
        if matches!(t.role.as_str(), "scale-label" | "value-label") {
            continue;
        }
        if numeric_date(&t.plain) {
            numeric.push(t.node);
        }
        if short_year(&t.plain) {
            short.push(t.node);
        }
    }
    if !numeric.is_empty() {
        l.add("R5-20", "warn", format!("{} with a numeric month: write months in letters (“Aug”, “Aug. 10, 2022”); on time axes, day, month and year go on separate rows (G6.19, G6.23)", count(numeric.len(), "text")), &numeric, None);
    }
    if !short.is_empty() {
        l.add(
            "R5-21",
            "warn",
            format!(
                "{} with a two-digit year: write years with four digits (G6.20)",
                count(short.len(), "text")
            ),
            &short,
            None,
        );
    }
}

pub fn title_year(l: &mut Lint) {
    let d = &l.d;
    let timed = d.chart.iter().any(|c| c == "time")
        || d.texts
            .iter()
            .any(|t| matches!(t.role.as_str(), "time-label" | "time-label-2"));
    let block: Vec<&TextItem> = d
        .texts
        .iter()
        .filter(|t| matches!(t.role.as_str(), "title" | "subtitle"))
        .collect();
    let year = Regex::new(r"\b(1[5-9]|2[0-9])\d{2}\b").unwrap();
    if timed && !block.is_empty() && !block.iter().any(|t| year.is_match(&t.plain)) {
        let nodes: Vec<usize> = block.iter().map(|t| t.node).collect();
        l.add("R2-12", "warn", "the title block of a chart over time does not give the year: a printout that resurfaces later must not read as current (G6.8)".into(), &nodes, None);
    }
}

pub fn vocabulary(l: &mut Lint) {
    let d = &l.d;
    let mut unknown: Vec<(String, Vec<usize>)> = vec![];
    for &n in &d.drawn {
        for c in &d.nodes[n].classes {
            if !crate::in_vocabulary(c) {
                group(&mut unknown, c.clone(), n);
            }
        }
    }
    let plain: Vec<usize> = d
        .texts
        .iter()
        .filter(|t| !d.nodes[t.node].has_any(&["prose", "math"]))
        .map(|t| t.node)
        .collect();
    if !unknown.is_empty() {
        let names: BTreeSet<&str> = unknown.iter().map(|u| u.0.as_str()).collect();
        let msg = format!(
            "classes outside the chart vocabulary: {}; their roles are not checked (SPEC §4.3)",
            names.into_iter().collect::<Vec<_>>().join(", ")
        );
        let firsts: Vec<usize> = unknown.iter().map(|u| u.1[0]).collect();
        l.add("CG-5", "info", msg, &firsts, None);
    }
    if !plain.is_empty() {
        l.add(
            "CG-5",
            "info",
            format!(
                "{} without the class prose or math: bake.py does not typeset them (SPEC §4.4)",
                count(plain.len(), "text")
            ),
            &plain,
            None,
        );
    }
}

// ---- phase 2 ----

static CURVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[CcSsQqTtAa]").unwrap());
static MLC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([MmLlCc])([^MmLlCcHhVvSsQqTtAaZz]*)").unwrap());

pub fn line_shapes(l: &mut Lint) {
    let d = &l.d;
    let bumps = d.chart.iter().any(|c| c == "bumps");
    let (mut bent, mut overshoot) = (vec![], vec![]);
    let num = Regex::new(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?").unwrap();
    for &n in &d.marks {
        let node = &d.nodes[n];
        if node.tag != "path" || !matches!(node.kind(), Some("line" | "projection" | "step-line")) {
            continue;
        }
        let path = node.get("d").unwrap_or("");
        if !CURVE.is_match(path) {
            continue;
        }
        if !bumps {
            bent.push(n);
            continue;
        }
        let mut y: Option<f64> = None;
        for c in MLC.captures_iter(path) {
            let nums: Vec<f64> = num
                .find_iter(&c[2])
                .filter_map(|m| m.as_str().parse().ok())
                .collect();
            if !c[1].eq_ignore_ascii_case("C") {
                if nums.len() >= 2 {
                    y = Some(nums[nums.len() - 1]);
                }
                continue;
            }
            let mut i = 0;
            while nums.len() >= 6 && i < nums.len() - 5 {
                let (c1y, c2y, ey) = (nums[i + 1], nums[i + 3], nums[i + 5]);
                if let Some(y0) = y {
                    if (c1y - y0).abs() > 0.02 || (c2y - ey).abs() > 0.02 {
                        // the original leaves only this command's cubics, so a line may be counted twice
                        overshoot.push(n);
                        break;
                    }
                }
                y = Some(ey);
                i += 6;
            }
        }
    }
    if !bent.is_empty() {
        l.add("R6-39", "error", format!("{} through data points drawn with curves: straight segments only; smoothing overshoots the data and is arbitrary (G7.17)", count(bent.len(), "line")), &bent, None);
    }
    if !overshoot.is_empty() {
        let n = overshoot.len();
        let mut seen = HashSet::new();
        overshoot.retain(|x| seen.insert(*x));
        l.add("R6-40", "error", format!("{} whose curves are not flat at their points: they may overshoot the ranks they join (G7.17, G7.24)", count(n, "bumps line")), &overshoot, None);
    }
}

pub fn dashes(l: &mut Lint) {
    let d = &l.d;
    let odd: Vec<usize> = d
        .marks
        .iter()
        .copied()
        .filter(|&n| {
            let node = &d.nodes[n];
            node.kind().is_some_and(|k| STROKED.contains(&k))
                && !node.has_any(DASHED)
                && !matches!(
                    d.value(n, "stroke-dasharray")
                        .unwrap_or_else(|| "none".into())
                        .trim(),
                    "none" | "" | "0"
                )
        })
        .collect();
    if !odd.is_empty() {
        let series: BTreeSet<String> = odd
            .iter()
            .map(|&n| d.series_key(n).unwrap_or_else(|| "?".into()))
            .collect();
        l.add("R6-31", "error", format!("{} dashed or dotted without a lower certainty ({}): dashes read as “less certain”; tell series apart by colour, never by dash pattern (G7.10)", count(odd.len(), "line"), series.into_iter().collect::<Vec<_>>().join(", ")), &odd, None);
    }
}

pub fn shared_colours(l: &mut Lint) {
    let d = &l.d;
    if d.chart.iter().any(|c| c == "multiples" || c == "merged") {
        return;
    }
    let mut colour: Vec<(String, (String, usize))> = vec![];
    let mut symbols: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for &n in &d.marks {
        let Some(key) = d.series_key(n) else { continue };
        if !l.regular(n) || d.within(n, "legend") {
            continue;
        }
        let kind = d.nodes[n].kind();
        if matches!(kind, Some("line" | "step-line")) {
            if let Some(c) = d.paint(n, "stroke").into_iter().next() {
                if !colour.iter().any(|(k, _)| *k == key) {
                    colour.push((key, (c, n)));
                }
            }
        } else if kind == Some("point") && d.nodes[n].tag == "use" {
            let href = d.nodes[n]
                .get("href")
                .or_else(|| {
                    d.nodes[n]
                        .el
                        .attribute(("http://www.w3.org/1999/xlink", "href"))
                })
                .unwrap_or("");
            symbols.entry(key).or_default().insert(href.into());
        }
    }
    let mut by: Vec<(String, Vec<(String, usize)>)> = vec![];
    for (key, (c, n)) in colour {
        group(&mut by, c, (key, n));
    }
    let mut adds = vec![];
    for (c, items) in by {
        if items.len() < 2 {
            continue;
        }
        let marks: Vec<Option<String>> = items
            .iter()
            .map(|(k, _)| {
                symbols
                    .get(k)
                    .filter(|s| s.len() == 1)
                    .map(|s| s.iter().next().unwrap().clone())
            })
            .collect();
        if marks.iter().all(Option::is_some)
            && marks.iter().collect::<BTreeSet<_>>().len() == marks.len()
        {
            continue;
        }
        adds.push((format!("the lines of {} share the colour {c}: every line its own colour, or crossing lines cannot be told apart (G4.27)", items.iter().map(|i| i.0.as_str()).collect::<Vec<_>>().join(", ")), items.iter().map(|i| i.1).collect::<Vec<_>>()));
    }
    for (m, ns) in adds {
        l.add("R6-28", "error", m, &ns, None);
    }
}

pub fn legend_order(l: &mut Lint) {
    let d = &l.d;
    if !d.chart.iter().any(|c| c == "line") {
        return;
    }
    let entries: Vec<usize> = (0..d.nodes.len())
        .filter(|&n| d.nodes[n].has("legend-entry") && d.nodes[n].get("data-series").is_some())
        .collect();
    if entries.len() < 2 {
        return;
    }
    let mut ends: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for &n in &d.marks {
        if matches!(d.nodes[n].kind(), Some("line" | "projection"))
            && !d.within(n, "legend")
            && !d.within(n, "inset")
        {
            let pts = d.points(n);
            if let (Some(key), false) = (d.series_key(n), pts.is_empty()) {
                let last = pts
                    .iter()
                    .copied()
                    .fold(pts[0], |b, p| if p.0 > b.0 { p } else { b });
                if ends.get(&key).is_none_or(|e| last.0 > e.0) {
                    ends.insert(key, last);
                }
            }
        }
    }
    let listed: Vec<String> = entries
        .iter()
        .filter_map(|&n| d.nodes[n].get("data-series"))
        .filter(|k| ends.contains_key(*k))
        .map(str::to_owned)
        .collect();
    let mut want = listed.clone();
    want.sort_by(|a, b| {
        ends[a].1.partial_cmp(&ends[b].1).unwrap().then(
            listed
                .iter()
                .position(|x| x == a)
                .cmp(&listed.iter().position(|x| x == b)),
        )
    });
    if listed != want {
        l.add("R4-5", "error", format!("legend entries in the order {}, the lines at their ends {}: order a line chart's legend by the lines at the latest point (G5.9)", listed.join(", "), want.join(", ")), &entries[..1], None);
    }
}

pub fn empty_dots(l: &mut Lint) {
    let d = &l.d;
    if !d.chart.iter().any(|c| c == "dot") || !d.chart.iter().any(|c| c == "categorical") {
        return;
    }
    let o = d.orientation.unwrap_or("vertical");
    let mut adds = vec![];
    for (_, nodes) in d.panels() {
        let dots: Vec<usize> = nodes
            .iter()
            .copied()
            .filter(|&n| d.nodes[n].kind() == Some("dot") && d.marks.contains(&n))
            .collect();
        if dots.is_empty() {
            continue;
        }
        let mut centres: Vec<f64> = dots
            .iter()
            .filter_map(|&n| d.bx(n))
            .map(|b| {
                py_round(
                    if o == "vertical" {
                        (b[0] + b[2]) / 2.
                    } else {
                        (b[1] + b[3]) / 2.
                    },
                    2,
                )
            })
            .collect();
        centres.sort_by(|a, b| a.partial_cmp(b).unwrap());
        centres.dedup();
        let steps: Vec<f64> = centres
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|s| *s > 0.5)
            .collect();
        let reach = if steps.is_empty() {
            f64::INFINITY
        } else {
            0.5 * steps.iter().cloned().fold(f64::INFINITY, f64::min)
        };
        let notes: Vec<f64> = d
            .texts_in(&nodes, &["missing-note"])
            .into_iter()
            .map(|k| axis(d, &d.texts[k], o))
            .collect();
        let empty: Vec<usize> = d
            .texts_in(&nodes, &["category-label"])
            .into_iter()
            .filter(|&k| {
                let x = axis(d, &d.texts[k], o);
                !centres.iter().any(|c| (x - c).abs() <= reach)
                    && !notes.iter().any(|m| (x - m).abs() <= reach)
            })
            .map(|k| d.texts[k].node)
            .collect();
        if !empty.is_empty() {
            adds.push((format!("{} without a dot and without a note: say why the value is not shown, “(missing)”, “(unknown)” … (G7.2)", count(empty.len(), "category")), empty));
        }
    }
    for (m, ns) in adds {
        l.add("R9-7", "error", m, &ns, None);
    }
}

pub fn time_ticks(l: &mut Lint) {
    let d = &l.d;
    if d.chart.iter().any(|c| c == "step" || c == "thin-bar") {
        return;
    }
    let ticks: Vec<usize> = d
        .drawn
        .iter()
        .copied()
        .filter(|&n| d.nodes[n].has("time-tick"))
        .collect();
    if !ticks.is_empty() {
        l.add("R1-04", "error", format!("{} on a chart that is not over a true time axis: labels sit centred on their slots, so ticks add nothing (G3.4, D-45)", count(ticks.len(), "time tick")), &ticks[..1], None);
    }
}

pub fn event_bars(l: &mut Lint) {
    let d = &l.d;
    let mut adds = vec![];
    for (_, nodes) in d.panels() {
        let bars: Vec<usize> = nodes
            .iter()
            .copied()
            .filter(|&n| {
                d.nodes[n].kind() == Some("event-bar")
                    && d.marks.contains(&n)
                    && !d.within(n, "inset")
            })
            .collect();
        if bars.is_empty() {
            continue;
        }
        let f = scale_map(d, &nodes, "vertical");
        let z = zero_line(d, &nodes)
            .and_then(|g| d.points(g).first().map(|p| p.1))
            .or(f.map(|f| f.0));
        let Some(z) = z else { continue };
        let boxes: Vec<(usize, Box4)> = bars
            .iter()
            .filter_map(|&n| d.bx(n).map(|b| (n, b)))
            .collect();
        let mut off = vec![];
        for &(n, b) in &boxes {
            let base = if (b[3] - z).abs() <= (b[1] - z).abs() {
                b[3]
            } else {
                b[1]
            };
            if (base - z).abs() <= 0.5 {
                continue;
            }
            let cx = (b[0] + b[2]) / 2.;
            if !boxes.iter().any(|&(m, c)| {
                m != n
                    && ((c[0] + c[2]) / 2. - cx).abs() < 0.5
                    && (c[1] - base).abs().min((c[3] - base).abs()) < 1.5
            }) {
                off.push(n);
            }
        }
        if !off.is_empty() {
            adds.push((format!("{} neither from zero nor stacked on an event at the same instant: a bar's length is its value (G2.6, G7.20, D-42)", count(off.len(), "thin bar")), off));
        }
    }
    for (m, ns) in adds {
        l.add("R2-24", "error", m, &ns, None);
    }
}

fn halo_ok(d: &Drawing, t: &TextItem, near: &[usize]) -> bool {
    let (ax, ay) = apply(&d.nodes[t.node].ctm, t.x, t.y);
    d.texts.iter().any(|h| {
        d.nodes[h.node].has("text-halo")
            && d.within(h.node, "divider-halo")
            && h.code == t.code
            && h.node < t.node
            && near.iter().all(|&n| h.node > n)
            && {
                let (hx, hy) = apply(&d.nodes[h.node].ctm, h.x, h.y);
                (hx - ax).abs() < 0.05 && (hy - ay).abs() < 0.05
            }
    })
}

pub fn time_label_dividers(l: &mut Lint) {
    let d = &l.d;
    let clear = l.divider_clear.unwrap_or(0.);
    let divs: Vec<(f64, f64, f64, usize)> = d
        .drawn
        .iter()
        .copied()
        .filter(|&n| d.nodes[n].has("time-divider"))
        .filter_map(|n| {
            let pts = d.points(n);
            let first = *pts.first()?;
            Some((
                first.0,
                pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
                pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
                n,
            ))
        })
        .collect();
    let mut bad = vec![];
    for t in &d.texts {
        if !matches!(t.role.as_str(), "time-label" | "time-label-2")
            || d.nodes[t.node].has("text-halo")
        {
            continue;
        }
        let Some([x0, y0, x1, y1]) = t.bx else {
            continue;
        };
        let near: Vec<usize> = divs
            .iter()
            .filter(|(x, ya, yb, _)| {
                *ya < y1 && y0 < *yb && x0 - clear + 0.02 < *x && *x < x1 + clear - 0.02
            })
            .map(|v| v.3)
            .collect();
        if near.is_empty() {
            continue;
        }
        if !halo_ok(d, t, &near) {
            bad.push(t.node);
        }
    }
    if !bad.is_empty() {
        l.add("CG-6", "error", format!("{} closer than {} pt to a divider without its halo: the outer dividers move out from the first and last slots, and a label beside a divider between two groups keeps its place with a knock-out halo drawn over the divider (G2.37)", count(bad.len(), "time label"), fmt_g(clear)), &bad, None);
    }
}

pub fn label_lines(l: &mut Lint) {
    let d = &l.d;
    let lines: Vec<([(f64, f64); 2], usize)> = d
        .drawn
        .iter()
        .copied()
        .filter(|&n| d.nodes[n].has_any(&["gridline", "connector"]))
        .filter_map(|n| {
            let p = d.points(n);
            (p.len() == 2).then(|| ([p[0], p[1]], n))
        })
        .collect();
    let through = |pts: &[(f64, f64); 2], b: &Box4| {
        let ((xa, ya), (xb, yb)) = (pts[0], pts[1]);
        if (ya - yb).abs() < 0.01 {
            return b[1] + 0.02 < ya
                && ya < b[3] - 0.02
                && xa.min(xb) < b[2] - 0.02
                && xa.max(xb) > b[0] + 0.02;
        }
        if (xa - xb).abs() < 0.01 {
            return b[0] + 0.02 < xa
                && xa < b[2] - 0.02
                && ya.min(yb) < b[3] - 0.02
                && ya.max(yb) > b[1] + 0.02;
        }
        false
    };
    let mut bad = vec![];
    for t in &d.texts {
        if t.role != "value-label" || d.nodes[t.node].has("text-halo") {
            continue;
        }
        let Some(b) = t.bx else { continue };
        let near: Vec<usize> = lines
            .iter()
            .filter(|(p, n)| *n < t.node && through(p, &b))
            .map(|x| x.1)
            .collect();
        if near.is_empty() {
            continue;
        }
        if !halo_ok(d, t, &near) {
            bad.push(t.node);
        }
    }
    if !bad.is_empty() {
        l.add("CG-7", "error", format!("{} crossed by a gridline or connector without its halo: a line crossing a label stops short of it, a knock-out halo of the page colour round its letters drawn over the line (owner, 1 Oct 2026)", count(bad.len(), "value label")), &bad, None);
    }
}

pub fn stacks(l: &mut Lint) {
    let d = &l.d;
    let em = d.em;
    let mut adds: Vec<(&str, String, Vec<usize>)> = vec![];
    for (panel, nodes) in d.panels() {
        if panel.is_some_and(|p| d.nodes[p].has("inset-panel")) {
            continue;
        }
        let segs: Vec<usize> = nodes
            .iter()
            .copied()
            .filter(|&n| {
                d.nodes[n].kind() == Some("segment")
                    && d.marks.contains(&n)
                    && !d.within(n, "inset")
            })
            .collect();
        if segs.is_empty() {
            continue;
        }
        let o = bar_orientation(d, &segs);
        let f = scale_map(d, &nodes, o);
        let mut z = zero_line(d, &nodes)
            .and_then(|g| d.points(g).first().copied())
            .map(|p| if o == "vertical" { p.1 } else { p.0 });
        if z.is_none() {
            z = f.map(|f| f.0);
        }
        let along = |b: Box4| {
            if o == "vertical" {
                (b[1], b[3])
            } else {
                (b[0], b[2])
            }
        };
        if d.chart.iter().any(|c| c == "waterfall") {
            let mut marks: Vec<usize> = nodes
                .iter()
                .copied()
                .filter(|&n| {
                    matches!(d.nodes[n].kind(), Some("segment" | "bar"))
                        && d.marks.contains(&n)
                        && d.bx(n).is_some()
                })
                .collect();
            marks.sort_by(|&a, &b| {
                let (ba, bb) = (d.bx(a).unwrap(), d.bx(b).unwrap());
                let k = if o == "vertical" { 0 } else { 1 };
                ba[k].partial_cmp(&bb[k]).unwrap()
            });
            let mut loose = vec![];
            for w in marks.windows(2) {
                let (a, b) = (w[0], w[1]);
                if d.nodes[b].kind() != Some("segment") {
                    continue;
                }
                let (ea, eb) = (along(d.bx(a).unwrap()), along(d.bx(b).unwrap()));
                let m = [ea.0, ea.1]
                    .iter()
                    .flat_map(|x| [eb.0, eb.1].map(|y| (x - y).abs()))
                    .fold(f64::INFINITY, f64::min);
                if m > 0.5 {
                    loose.push(b);
                }
            }
            if !loose.is_empty() {
                adds.push(("R7-11", format!("{} not starting where the bar before it ended: each step runs from the running total before it to the one after it (G7.33)", count(loose.len(), "waterfall step")), loose));
            }
            continue;
        }
        let mut by: Vec<(Option<String>, Vec<usize>)> = vec![];
        for &n in &segs {
            if d.bx(n).is_some() {
                let x = d.nodes[n]
                    .get("data-x")
                    .map(str::to_owned)
                    .or_else(|| d.attr(n, "data-x"));
                group(&mut by, x, n);
            }
        }
        let mut off: Vec<usize> = vec![];
        for (_, ns) in &by {
            let mut spans: Vec<(f64, f64)> = ns.iter().map(|&n| along(d.bx(n).unwrap())).collect();
            spans.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if let Some(z) = z {
                let m = spans
                    .iter()
                    .flat_map(|s| [s.0, s.1])
                    .map(|e| (e - z).abs())
                    .fold(f64::INFINITY, f64::min);
                if m > 0.5 {
                    off.extend(ns);
                    continue;
                }
            }
            for w in spans.windows(2) {
                let ((_, a1), (b0, _)) = (w[0], w[1]);
                if b0 < a1 - 0.5 || b0 - a1 > STACK_GAP * em {
                    off.extend(ns);
                    break;
                }
            }
        }
        if !off.is_empty() {
            let mut seen = HashSet::new();
            let uniq: Vec<usize> = off.into_iter().filter(|n| seen.insert(*n)).collect();
            adds.push(("R2-24", format!("{} of stacks not laid end to end from zero: a stack's height is its total only when its parts start at zero and meet, parted by at most a thin gap (G2.6, G7.32)", count(uniq.len(), "segment")), uniq));
        }
    }
    for (r, m, ns) in adds {
        l.add(r, "error", m, &ns, None);
    }
}

pub fn footnotes(l: &mut Lint) {
    let d = &l.d;
    let marks: Vec<&TextItem> = d.texts.iter().filter(|t| t.role == "note-mark").collect();
    if marks.is_empty() && !d.texts.iter().any(|t| t.role == "footnote") {
        return;
    }
    let mut above: Vec<(String, Vec<usize>)> = vec![];
    let mut below: Vec<(String, Vec<usize>)> = vec![];
    for t in &marks {
        for m in crate::drawing::unescape(&t.code).split(',') {
            if !m.is_empty() {
                if d.within(t.node, "footer") {
                    group(&mut below, m.to_owned(), t.node);
                } else {
                    group(&mut above, m.to_owned(), t.node);
                }
            }
        }
    }
    let mut adds = vec![];
    let lonely: Vec<&(String, Vec<usize>)> = above
        .iter()
        .filter(|(m, _)| !below.iter().any(|b| b.0 == *m))
        .collect();
    if !lonely.is_empty() {
        adds.push((format!("{} ({}) with no note under the chart: a mark refers the reader to a footnote that must be there", count(lonely.len(), "mark"), lonely.iter().map(|x| x.0.as_str()).collect::<Vec<_>>().join(", ")), lonely.iter().map(|x| x.1[0]).collect::<Vec<_>>()));
    }
    let unread: Vec<&(String, Vec<usize>)> = below
        .iter()
        .filter(|(m, _)| !above.iter().any(|a| a.0 == *m))
        .collect();
    if !unread.is_empty() {
        adds.push((format!("{} ({}) whose mark is set on nothing in the chart: give it a place, or make it a plain note", count(unread.len(), "footnote"), unread.iter().map(|x| x.0.as_str()).collect::<Vec<_>>().join(", ")), unread.iter().map(|x| x.1[0]).collect()));
    }
    let bare: Vec<usize> = d
        .texts
        .iter()
        .filter(|t| t.role == "footnote" && !d.within(t.node, "footer"))
        .map(|t| t.node)
        .collect();
    if !bare.is_empty() {
        adds.push((
            format!(
                "{} outside the footer: notes stand under the chart",
                count(bare.len(), "footnote")
            ),
            bare,
        ));
    }
    let foot: Vec<&TextItem> = d
        .texts
        .iter()
        .filter(|t| d.within(t.node, "footer") && t.bx.is_some())
        .collect();
    let rest: Vec<&TextItem> = d
        .texts
        .iter()
        .filter(|t| !d.within(t.node, "footer") && t.bx.is_some())
        .collect();
    let rest_marks: Vec<Box4> = d
        .marks
        .iter()
        .copied()
        .filter(|&n| !d.within(n, "footer"))
        .filter_map(|n| d.bx(n))
        .collect();
    if !foot.is_empty() && (!rest.is_empty() || !rest_marks.is_empty()) {
        let top = foot
            .iter()
            .map(|t| t.bx.unwrap()[1])
            .fold(f64::INFINITY, f64::min);
        let low = rest
            .iter()
            .map(|t| t.bx.unwrap()[3])
            .chain(rest_marks.iter().map(|b| b[3]))
            .fold(f64::NEG_INFINITY, f64::max);
        if top < low - 0.5 {
            let first = foot.iter().fold(foot[0], |b, t| {
                if t.bx.unwrap()[1] < b.bx.unwrap()[1] {
                    t
                } else {
                    b
                }
            });
            adds.push(("a note under the chart rises above the chart's lowest text or mark: the notes come below everything else".into(), vec![first.node]));
        }
    }
    for (m, ns) in adds {
        l.add("CG-8", "error", m, &ns, None);
    }
}
