//! Notes under the chart and the marks that refer to them, ported from the original's
//! `notes.py` (SPEC §6.19), as a decorator around any chart layout:
//!
//! 1. **Room.** Before the chart is laid out, the texts that will carry a mark are measured
//!    wider by the mark's room (the original's NoteBatch), so that every layout keeps it clear.
//!    The decorator learns which texts a layout measures from the layout's own measurement
//!    requests, and widens exactly those whose role and words a note marks (every line that
//!    may end a wrapped label).
//! 2. **Marks.** After layout, each note's targets are found among the texts drawn (by role and
//!    words; a value by its mark or its cell), the first in reading order for a name or an
//!    element, every one for a status. Marks run in reading order in the style's sequence; a
//!    mark is its own text, raised to the cap height of the text it marks, just after it; the
//!    text moves back by the room it was given.
//! 3. **The footer.** Below everything else, wrapped to the figure's width by measured words
//!    and measured whole lines: footnotes in the order of their marks (each its mark, then its
//!    text with the lines after the first indented), then plain notes.
//!
//! What cannot be drawn is said (CG-8): a place no text answers, a mark or a word past the
//! figure's width (the figure is widened rather than clipped).
use chartgen_layout::{LayoutEngine, guides};
use chartgen_model::*;
use chartgen_notes::{NoteTarget, PreparedNote};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

const RULE: &str = "CG-8";
const MARK: &str = "note-mark";
const FOOTNOTE: &str = "footnote";
const PLAIN: &str = "chart-note";
const UNIT_ROLES: &[&str] = &["unit-phrase", "unit-line", "scale-title"];
const CATEGORY_ROLES: &[&str] = &[
    "category-label",
    "column-header",
    "row-header",
    "slice-label",
    "tile-label",
    "legend-text",
    "panel-title",
    "time-label",
];
const SERIES_ROLES: &[&str] = &[
    "end-label",
    // the rewrite's cartesian layout on main names its end labels so
    "series-label",
    "legend-text",
    "row-header",
    "column-header",
    "category-label",
    "slice-label",
    "panel-title",
];
const NOTE_ROLES: &[&str] = &["missing-note"];
const VALUE_ROLES: &[&str] = &[
    "value-label",
    "table-cell",
    "missing-note",
    "slice-label",
    "tile-label",
];
const CELL_ROLES: &[&str] = &["table-cell", "missing-note"];
const FILLS: &[&str] = &["bar", "tile", "cell", "segment", "table-fill"];

fn error(message: impl Into<String>) -> Error {
    Error::new("note-layout", message)
}

fn element_roles(name: &str) -> &'static [&'static str] {
    match name {
        "title" => &["title"],
        "descriptive" => &["subtitle"],
        "unit" => UNIT_ROLES,
        "x_title" => &["axis-title"],
        "names_title" => &["column-header"],
        _ => &[],
    }
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn metric(theme: &Theme, name: &str) -> Option<f64> {
    match theme.tokens.get(name)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_end_matches("pt").parse().ok(),
        _ => None,
    }
}

fn token(theme: &Theme, name: &str) -> Option<String> {
    theme
        .tokens
        .get(name)
        .and_then(Value::as_str)
        .map(|s| s.trim().trim_matches(['"', '\'']).to_owned())
}

/// The typeset size and style of a text role (the original stylesheet's rules; to be served
/// by the style crate's label_style once it is on main).
fn role_style(role: &str, theme: &Theme) -> (f64, TextStyle) {
    type Memo = std::sync::Mutex<BTreeMap<(String, String), (f64, TextStyle)>>;
    static MEMO: std::sync::OnceLock<Memo> = std::sync::OnceLock::new();
    let memo = MEMO.get_or_init(Default::default);
    let key = (role.to_owned(), theme.stylesheet.clone());
    if let Some(v) = memo.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return v;
    }
    let v = role_style_uncached(role, theme);
    if let Ok(mut m) = memo.lock() {
        m.insert(key, v.clone());
    }
    v
}

fn role_style_uncached(role: &str, theme: &Theme) -> (f64, TextStyle) {
    if !theme.stylesheet.trim().is_empty() {
        if let Ok(l) =
            chartgen_style::label_style(&theme.stylesheet, &["prose", role], &BTreeMap::new())
        {
            return (l.size, l.text_style());
        }
    }
    let label = metric(theme, "--label-size").unwrap_or(theme.font_size);
    let note = metric(theme, "--note-size").unwrap_or(label * 0.8);
    let size = match role {
        "title" => metric(theme, "--title-size").unwrap_or(label * 1.2),
        "missing-note" | "incomplete-note" | "chart-note" | "footnote" | "event-label"
        | "arrow-key" => note,
        "note-mark" => metric(theme, "--note-mark-size").unwrap_or(note),
        "inset-text" => metric(theme, "--inset-text-size").unwrap_or(note),
        "comment" => metric(theme, "--comment-size").unwrap_or(label),
        _ => label,
    };
    let mut style = guides::style_for(role, theme);
    if role == "event-label" {
        style.font_style = "italic".into();
        style.fill = token(theme, "--note").or(style.fill);
    }
    (size, style)
}

/// The measurements a pass reads; what is not measured yet is recorded and asked for.
struct Meas<'a> {
    theme: &'a Theme,
    c: &'a LayoutConstraints,
    m: &'a Measurements,
    wanted: RefCell<BTreeMap<String, TextRequest>>,
}

#[derive(Clone, Copy, Debug)]
struct Ext {
    width: f64,
    ascent: f64,
    descent: f64,
}

impl<'a> Meas<'a> {
    fn request(&self, role: &str, content: &TextContent) -> Result<TextRequest> {
        let (size, style) = role_style(role, self.theme);
        guides::request_with_style(role, content.clone(), size, None, style, self.theme, self.c)
    }
    fn ext_of(&self, role: &str, content: &TextContent) -> Result<(TextRequest, TextMetrics, Ext)> {
        let r = self.request(role, content)?;
        match self.m.entries.get(&r.id) {
            Some(v) => {
                let a = v.baseline.unwrap_or(v.height);
                Ok((
                    r,
                    v.clone(),
                    Ext {
                        width: v.width,
                        ascent: a,
                        descent: (v.height - a).max(0.),
                    },
                ))
            }
            None => {
                let size = r.font_size;
                let e = Ext {
                    width: 0.5 * size * content.readable().chars().count() as f64,
                    ascent: 0.7 * size,
                    descent: 0.2 * size,
                };
                self.wanted.borrow_mut().insert(r.id.clone(), r.clone());
                Ok((
                    r,
                    TextMetrics {
                        width: e.width,
                        height: e.ascent + e.descent,
                        baseline: Some(e.ascent),
                    },
                    e,
                ))
            }
        }
    }
    fn ext(&self, role: &str, text: &str) -> Result<Ext> {
        Ok(self.ext_of(role, &TextContent::plain(text))?.2)
    }
    fn width(&self, role: &str, text: &str) -> Result<f64> {
        Ok(self.ext(role, text)?.width)
    }
}

/// The k-th mark of the style's sequence.
fn mark_text(kind: &str, k: usize) -> String {
    chartgen_notes::mark(kind, k)
}

fn kind_of(theme: &Theme) -> String {
    let v = token(theme, "--note-marks").unwrap_or_else(|| "letters".into());
    if ["letters", "numerals", "symbols"].contains(&v.as_str()) {
        v
    } else {
        "letters".into()
    }
}

fn status_text(spec: &Spec, status: &str) -> String {
    spec.get("/labels/notes")
        .get(status)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("({status})"))
}

fn category_text(spec: &Spec, c: &str) -> String {
    spec.get("/data/x/names")
        .get(c)
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v["plain"].as_str().map(str::to_owned))
                .or_else(|| v["text"].as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| c.to_owned())
}

fn series_text(data: &PreparedData, sid: &str) -> Option<String> {
    data.series
        .iter()
        .find(|s| s.id == sid)
        .map(|s| s.name.clone())
}

fn element_text(spec: &Spec, name: &str) -> Option<String> {
    let t = spec.get("/title");
    let v = match name {
        "title" => {
            if t.is_object() {
                t["text"].clone()
            } else {
                t.clone()
            }
        }
        "descriptive" => t["descriptive"].clone(),
        "x_title" => spec.get("/data/x/title").clone(),
        "names_title" => spec.get("/table/names_title").clone(),
        _ => Value::Null,
    };
    guides::content(&v)
        .ok()
        .flatten()
        .map(|c| c.readable().to_owned())
}

/// Every line that may end a label reading `text`: its last words, one to all.
fn suffixes(text: &str) -> Vec<String> {
    let text = norm(text);
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    (0..words.len()).map(|i| words[i..].join(" ")).collect()
}

fn unit_tails(spec: &Spec) -> Vec<String> {
    let u = spec.get("/data/measure/unit");
    let text = |v: &Value| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v["plain"].as_str().map(str::to_owned))
    };
    let mut out = vec![];
    for k in ["scale_title", "fallback"] {
        if let Some(t) = text(&u[k]) {
            out.extend(suffixes(&t).into_iter().last());
        }
    }
    if let Some(w) = u["what"].as_str() {
        out.push(format!("{})", norm(w).split(' ').next_back().unwrap_or("")));
    }
    if let Some(n) = u["noun"].as_str() {
        out.push(norm(n).split(' ').next_back().unwrap_or("").to_owned());
    }
    if let Some(s) = u["suffix"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push(s.to_owned());
    }
    out.retain(|t| !t.is_empty());
    out
}

/// A stub column's entry for a series (notes._stub_entry): its value, a date written as a
/// column of dates writes it ("02-Dec-2021").
fn stub_entry(spec: &Spec, data: &PreparedData, j: usize, sid: &str) -> Option<String> {
    let col = &spec.get("/table/stub")[j];
    let k = data.series.iter().position(|s| s.id == sid)?;
    let v = col["values"].get(k)?;
    let v = v.as_str().map(str::to_owned).unwrap_or_default();
    if col["kind"] == "date" && !v.is_empty() {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&v, "%Y-%m-%d") {
            return Some(chartgen_format::date_column(d));
        }
    }
    Some(v)
}

fn datum<'d>(data: &'d PreparedData, sid: &str, c: &str) -> Option<&'d Datum> {
    let s = data.series.iter().find(|s| s.id == sid)?;
    let j = data.categories.iter().position(|x| x == c)?;
    s.values.get(j)
}

/// [(roles, lines)] of the texts a target's mark may follow, to be measured wider.
fn widened(
    spec: &Spec,
    data: &PreparedData,
    t: &NoteTarget,
) -> Vec<(Vec<&'static str>, Vec<String>)> {
    match t {
        NoteTarget::Element { name } if name == "unit" => vec![],
        NoteTarget::Element { name } => element_text(spec, name)
            .map(|t| vec![(element_roles(name).to_vec(), suffixes(&t))])
            .unwrap_or_default(),
        NoteTarget::Comment { index } => guides::content(&spec.get("/comments")[*index]["text"])
            .ok()
            .flatten()
            .map(|c| vec![(vec!["comment"], suffixes(c.readable()))])
            .unwrap_or_default(),
        NoteTarget::Stub { index, series } => {
            let col = &spec.get("/table/stub")[*index];
            if col.is_null() {
                return vec![];
            }
            match series {
                Some(sid) => stub_entry(spec, data, *index, sid)
                    .map(|e| vec![(vec!["row-header"], suffixes(&e))])
                    .unwrap_or_default(),
                None => guides::content(&col["title"])
                    .ok()
                    .flatten()
                    .map(|t| vec![(vec!["column-header"], suffixes(t.readable()))])
                    .unwrap_or_default(),
            }
        }
        NoteTarget::Status { status, .. } => {
            vec![(NOTE_ROLES.to_vec(), suffixes(&status_text(spec, status)))]
        }
        NoteTarget::Datum { series, category } => match datum(data, series, category) {
            Some(v) if v.value.is_none() => {
                vec![(NOTE_ROLES.to_vec(), suffixes(&status_text(spec, &v.status)))]
            }
            _ => vec![],
        },
        NoteTarget::Series { series } => series_text(data, series)
            .map(|t| vec![(SERIES_ROLES.to_vec(), suffixes(&t))])
            .unwrap_or_default(),
        NoteTarget::Category { category } => {
            vec![(
                CATEGORY_ROLES.to_vec(),
                suffixes(&category_text(spec, category)),
            )]
        }
    }
}

/// The room each (role, line) keeps for its marks, and the unit phrase's tails.
struct Rooms {
    exact: BTreeMap<(String, String), f64>,
    tails: Vec<(Vec<&'static str>, String, f64)>,
}

impl Rooms {
    fn room(&self, role: &str, text: &str) -> f64 {
        let text = norm(text);
        let mut r = self
            .exact
            .get(&(role.to_owned(), text.clone()))
            .copied()
            .unwrap_or(0.);
        for (roles, tail, w) in &self.tails {
            if roles.contains(&role) && text.ends_with(tail.as_str()) {
                r = r.max(*w);
            }
        }
        r
    }
}

fn rooms(
    spec: &Spec,
    data: &PreparedData,
    notes: &[PreparedNote],
    meas: &Meas,
    kind: &str,
    gap: f64,
) -> Result<Rooms> {
    let marked: Vec<&PreparedNote> = notes.iter().filter(|n| !n.targets.is_empty()).collect();
    let mut one: f64 = 0.;
    for k in 0..marked.len() {
        one = one.max(meas.width(MARK, &mark_text(kind, k))?);
    }
    let comma = meas.width(MARK, ",")?;
    let mut per: BTreeMap<(String, String), BTreeSet<usize>> = BTreeMap::new();
    let mut tails = vec![];
    for (j, n) in marked.iter().enumerate() {
        for t in &n.targets {
            if matches!(t, NoteTarget::Element { name } if name == "unit") {
                for tail in unit_tails(spec) {
                    tails.push((UNIT_ROLES.to_vec(), tail, one + gap));
                }
                continue;
            }
            for (roles, lines) in widened(spec, data, t) {
                for role in roles {
                    for l in &lines {
                        per.entry((role.to_owned(), l.clone()))
                            .or_default()
                            .insert(j);
                    }
                }
            }
        }
    }
    Ok(Rooms {
        exact: per
            .into_iter()
            .map(|(k, js)| {
                let n = js.len() as f64;
                (k, n * one + (n - 1.) * comma + gap)
            })
            .collect(),
        tails,
    })
}

// ---- the placed texts and marks, read from the scene ----------------------------------------------

#[derive(Debug, Clone)]
struct T {
    path: Vec<usize>,
    role: String,
    plain: String,
    /// Global anchor x and baseline.
    x: f64,
    y: f64,
    anchor: String,
    width: f64,
    ascent: f64,
    descent: f64,
    id: String,
    ancestors: Vec<String>,
    offset: (f64, f64),
    mods: Vec<String>,
    value_of: Option<(String, String)>,
}

impl T {
    fn bbox(&self) -> (f64, f64, f64, f64) {
        let x0 = self.x
            - match self.anchor.as_str() {
                "middle" => self.width / 2.,
                "end" => self.width,
                _ => 0.,
            };
        (
            x0,
            self.y - self.ascent,
            x0 + self.width,
            self.y + self.descent,
        )
    }
}

#[derive(Debug, Clone)]
struct Mk {
    classes: Vec<String>,
    data: BTreeMap<String, String>,
    bbox: (f64, f64, f64, f64),
    ancestors: Vec<String>,
}

fn walk(
    ps: &[Primitive],
    path: &mut Vec<usize>,
    off: (f64, f64),
    anc: &mut Vec<String>,
    texts: &mut Vec<T>,
    marks: &mut Vec<Mk>,
    bottom: &mut f64,
) {
    for (i, p) in ps.iter().enumerate() {
        path.push(i);
        match p {
            Primitive::Group {
                children,
                translate,
                meta,
                ..
            } => {
                let n = anc.len();
                anc.extend(meta.classes.iter().cloned());
                walk(
                    children,
                    path,
                    (off.0 + translate.x, off.1 + translate.y),
                    anc,
                    texts,
                    marks,
                    bottom,
                );
                anc.truncate(n);
            }
            Primitive::Text {
                position,
                request,
                metrics,
                anchor,
                meta,
            } => {
                let ascent = metrics.baseline.unwrap_or(metrics.height);
                let role = if request.role.is_empty() {
                    meta.role.clone()
                } else {
                    request.role.clone()
                };
                let mods = meta
                    .classes
                    .iter()
                    .filter(|c| *c != "prose" && **c != role)
                    .cloned()
                    .collect();
                let t = T {
                    path: path.clone(),
                    role,
                    plain: request.content.readable().to_owned(),
                    x: off.0 + position.x,
                    y: off.1 + position.y + ascent,
                    anchor: anchor.clone(),
                    width: metrics.width,
                    ascent,
                    descent: (metrics.height - ascent).max(0.),
                    id: request.id.clone(),
                    ancestors: anc.clone(),
                    offset: off,
                    mods,
                    value_of: meta.value_of.clone(),
                };
                *bottom = bottom.max(t.bbox().3);
                texts.push(t);
            }
            Primitive::Line { from, to, .. } => {
                *bottom = bottom.max(off.1 + from.y.max(to.y));
            }
            other => {
                let (b, meta) = match other {
                    Primitive::Rect { bounds, meta, .. } => (
                        (
                            bounds.x,
                            bounds.y,
                            bounds.x + bounds.width,
                            bounds.y + bounds.height,
                        ),
                        meta,
                    ),
                    Primitive::Circle {
                        center,
                        radius,
                        meta,
                        ..
                    } => (
                        (
                            center.x - radius,
                            center.y - radius,
                            center.x + radius,
                            center.y + radius,
                        ),
                        meta,
                    ),
                    Primitive::Use {
                        center, size, meta, ..
                    } => (
                        (
                            center.x - size / 2.,
                            center.y - size / 2.,
                            center.x + size / 2.,
                            center.y + size / 2.,
                        ),
                        meta,
                    ),
                    Primitive::Wedge {
                        center,
                        radius,
                        meta,
                        ..
                    } => (
                        (
                            center.x - radius,
                            center.y - radius,
                            center.x + radius,
                            center.y + radius,
                        ),
                        meta,
                    ),
                    Primitive::Shape { commands, meta, .. } if !commands.is_empty() => (
                        commands
                            .iter()
                            .filter_map(|c| match c {
                                PathCommand::MoveTo { to }
                                | PathCommand::LineTo { to }
                                | PathCommand::CubicTo { to, .. }
                                | PathCommand::ArcTo { to, .. } => Some(to),
                                PathCommand::Close => None,
                            })
                            .fold(
                                (
                                    f64::INFINITY,
                                    f64::INFINITY,
                                    f64::NEG_INFINITY,
                                    f64::NEG_INFINITY,
                                ),
                                |b, p| (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y)),
                            ),
                        meta,
                    ),
                    Primitive::Path { points, meta, .. } if !points.is_empty() => (
                        points.iter().fold(
                            (
                                f64::INFINITY,
                                f64::INFINITY,
                                f64::NEG_INFINITY,
                                f64::NEG_INFINITY,
                            ),
                            |b, p| (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y)),
                        ),
                        meta,
                    ),
                    _ => {
                        path.pop();
                        continue;
                    }
                };
                let b = (b.0 + off.0, b.1 + off.1, b.2 + off.0, b.3 + off.1);
                *bottom = bottom.max(b.3);
                marks.push(Mk {
                    classes: meta.classes.clone(),
                    data: meta
                        .data
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    bbox: b,
                    ancestors: anc.clone(),
                });
            }
        }
        path.pop();
    }
}

struct Finder<'a> {
    spec: &'a Spec,
    data: &'a PreparedData,
    em: f64,
    texts: Vec<T>,
    marks: Vec<Mk>,
    order: BTreeMap<usize, (usize, f64)>,
    horizontal: bool,
}

/// Reading order: rows by the texts' centre lines, top to bottom, then left to right.
fn reading(texts: &[T], em: f64) -> BTreeMap<usize, (usize, f64)> {
    let mut idx: Vec<usize> = (0..texts.len()).collect();
    let key = |i: &usize| {
        let b = texts[*i].bbox();
        ((b.1 + b.3) / 2., b.0)
    };
    idx.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap());
    let mut out = BTreeMap::new();
    let mut row: i64 = -1;
    let mut start: Option<f64> = None;
    for i in idx {
        let b = texts[i].bbox();
        let cy = (b.1 + b.3) / 2.;
        if start.is_none_or(|s| cy - s > 0.6 * em) {
            row += 1;
            start = Some(cy);
        }
        out.insert(i, (row as usize, b.0));
    }
    out
}

impl<'a> Finder<'a> {
    fn first(&self, hosts: Vec<usize>) -> Vec<usize> {
        hosts
            .into_iter()
            .min_by(|a, b| self.order[a].partial_cmp(&self.order[b]).unwrap())
            .into_iter()
            .collect()
    }

    /// The texts of these roles that end a label reading `display`.
    fn stacked(&self, roles: &[&str], display: &str) -> Vec<usize> {
        let want = norm(display);
        let cands: Vec<usize> = (0..self.texts.len())
            .filter(|i| roles.contains(&self.texts[*i].role.as_str()))
            .collect();
        let mut out = vec![];
        for &i in &cands {
            let t = &self.texts[i];
            let p = norm(&t.plain);
            if p == want {
                out.push(i);
                continue;
            }
            if !want.ends_with(&format!(" {p}")) {
                continue;
            }
            let mut acc = p.clone();
            let mut cur = i;
            loop {
                let c = &self.texts[cur];
                let ups: Vec<usize> = cands
                    .iter()
                    .copied()
                    .filter(|&u| {
                        let ut = &self.texts[u];
                        u != cur
                            && ut.anchor == c.anchor
                            && (ut.x - c.x).abs() < 0.5
                            && 0. < c.y - ut.y
                            && c.y - ut.y < 3. * self.em
                            && want.ends_with(&format!("{} {acc}", norm(&ut.plain)))
                    })
                    .collect();
                let Some(up) = ups
                    .into_iter()
                    .max_by(|a, b| self.texts[*a].y.partial_cmp(&self.texts[*b].y).unwrap())
                else {
                    break;
                };
                cur = up;
                acc = format!("{} {acc}", norm(&self.texts[cur].plain));
                if acc == want {
                    out.push(i);
                    break;
                }
            }
        }
        out
    }

    /// Of several texts reading alike, the one on the row of the series' name.
    fn on_row(&self, got: Vec<usize>, sid: &str) -> Vec<usize> {
        if got.len() <= 1 {
            return got;
        }
        let Some(name) = series_text(self.data, sid) else {
            return vec![];
        };
        let Some(&n) = self
            .stacked(&["row-header", "category-label"], &name)
            .first()
        else {
            return vec![];
        };
        let y = self.texts[n].y;
        let near = got
            .into_iter()
            .min_by(|a, b| {
                (self.texts[*a].y - y)
                    .abs()
                    .partial_cmp(&(self.texts[*b].y - y).abs())
                    .unwrap()
            })
            .unwrap();
        if (self.texts[near].y - y).abs() < 0.5 * self.em {
            vec![near]
        } else {
            vec![]
        }
    }

    fn element(&self, name: &str) -> Vec<usize> {
        if name == "unit" {
            let mut units: Vec<usize> = (0..self.texts.len())
                .filter(|i| UNIT_ROLES.contains(&self.texts[*i].role.as_str()))
                .collect();
            units.sort_by(|a, b| self.order[a].partial_cmp(&self.order[b]).unwrap());
            let Some(&first) = units.first() else {
                return vec![];
            };
            let parent = &self.texts[first].path[..self.texts[first].path.len() - 1];
            let block: Vec<usize> = units
                .into_iter()
                .filter(|&i| {
                    let t = &self.texts[i];
                    &t.path[..t.path.len() - 1] == parent
                        && t.y - self.texts[first].y < 3. * self.em
                })
                .collect();
            return block
                .into_iter()
                .max_by(|a, b| {
                    let (ta, tb) = (&self.texts[*a], &self.texts[*b]);
                    (ta.y, ta.bbox().2)
                        .partial_cmp(&(tb.y, tb.bbox().2))
                        .unwrap()
                })
                .into_iter()
                .collect();
        }
        let roles = element_roles(name);
        let Some(text) = element_text(self.spec, name) else {
            return vec![];
        };
        let mut got = self.stacked(roles, &text);
        if got.is_empty() && (name == "title" || name == "descriptive") {
            got = (0..self.texts.len())
                .filter(|i| roles.contains(&self.texts[*i].role.as_str()))
                .max_by(|a, b| {
                    let (ta, tb) = (&self.texts[*a], &self.texts[*b]);
                    (ta.y, ta.x).partial_cmp(&(tb.y, tb.x)).unwrap()
                })
                .into_iter()
                .collect();
        }
        self.first(got)
    }

    fn value(&self, sid: &str, c: &str) -> Vec<usize> {
        // the layout's own text of the value (a table's cell)
        if let Some(i) = self
            .texts
            .iter()
            .position(|t| t.value_of.as_ref().is_some_and(|(s, x)| s == sid && x == c))
        {
            return vec![i];
        }
        let v = datum(self.data, sid, c);
        let marks: Vec<&Mk> = self
            .marks
            .iter()
            .filter(|m| {
                m.data.get("series").map(String::as_str) == Some(sid)
                    && m.data.get("x").map(String::as_str) == Some(c)
                    && !m.ancestors.iter().any(|a| a == "inset")
            })
            .collect();
        let vals: Vec<usize> = (0..self.texts.len())
            .filter(|i| VALUE_ROLES.contains(&self.texts[*i].role.as_str()))
            .collect();
        if marks
            .iter()
            .any(|m| m.classes.iter().any(|k| k == "slice" || k == "tile"))
        {
            for nm in self.stacked(&["slice-label", "tile-label"], &category_text(self.spec, c)) {
                let n = &self.texts[nm];
                let under: Vec<usize> = vals
                    .iter()
                    .copied()
                    .filter(|&t| {
                        let tt = &self.texts[t];
                        t != nm
                            && tt.anchor == n.anchor
                            && (tt.x - n.x).abs() < 0.5
                            && 0. < tt.y - n.y
                            && tt.y - n.y < 2. * self.em
                    })
                    .collect();
                if let Some(u) = under
                    .into_iter()
                    .min_by(|a, b| self.texts[*a].y.partial_cmp(&self.texts[*b].y).unwrap())
                {
                    return vec![u];
                }
            }
            return vec![];
        }
        if !marks.is_empty() {
            let b = marks.iter().fold(
                (
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ),
                |b, m| {
                    (
                        b.0.min(m.bbox.0),
                        b.1.min(m.bbox.1),
                        b.2.max(m.bbox.2),
                        b.3.max(m.bbox.3),
                    )
                },
            );
            let (cx, cy) = ((b.0 + b.2) / 2., (b.1 + b.3) / 2.);
            let gap = |t: &T| {
                let (x0, y0, x1, y1) = t.bbox();
                (b.0 - x1).max(x0 - b.2).max(0.) + (b.1 - y1).max(y0 - b.3).max(0.)
            };
            return vals
                .into_iter()
                .filter(|&i| {
                    let t = &self.texts[i];
                    ["value-label", "table-cell", "missing-note"].contains(&t.role.as_str())
                        && gap(t) <= 0.25 * self.em
                })
                .min_by(|a, b| {
                    let d = |i: &usize| {
                        let x = self.texts[*i].bbox();
                        ((x.0 + x.2) / 2. - cx).abs() + ((x.1 + x.3) / 2. - cy).abs()
                    };
                    d(a).partial_cmp(&d(b)).unwrap()
                })
                .into_iter()
                .collect();
        }
        let Some(v) = v.filter(|v| v.value.is_none()) else {
            return vec![];
        };
        let got = self.stacked(NOTE_ROLES, &status_text(self.spec, &v.status));
        if got.len() <= 1 {
            return got;
        }
        let label = self.first(self.stacked(CATEGORY_ROLES, &category_text(self.spec, c)));
        let Some(&l) = label.first() else {
            return vec![];
        };
        let lb = self.texts[l].bbox();
        got.into_iter()
            .min_by(|a, b| {
                let d = |i: &usize| {
                    let x = self.texts[*i].bbox();
                    if self.horizontal {
                        ((x.1 + x.3) / 2. - (lb.1 + lb.3) / 2.).abs()
                    } else {
                        ((x.0 + x.2) / 2. - (lb.0 + lb.2) / 2.).abs()
                    }
                };
                d(a).partial_cmp(&d(b)).unwrap()
            })
            .into_iter()
            .collect()
    }

    fn find(&self, r: &NoteTarget) -> Vec<usize> {
        match r {
            NoteTarget::Element { name } => self.element(name),
            NoteTarget::Comment { index } => {
                let Some(text) = guides::content(&self.spec.get("/comments")[*index]["text"])
                    .ok()
                    .flatten()
                else {
                    return vec![];
                };
                self.first(self.stacked(&["comment"], text.readable()))
            }
            NoteTarget::Stub { index, series } => {
                let col = &self.spec.get("/table/stub")[*index];
                if col.is_null() {
                    return vec![];
                }
                match series {
                    Some(sid) => {
                        let Some(entry) = stub_entry(self.spec, self.data, *index, sid) else {
                            return vec![];
                        };
                        let got = self.stacked(&["row-header"], &entry);
                        self.on_row(got, sid)
                    }
                    None => {
                        let Some(t) = guides::content(&col["title"]).ok().flatten() else {
                            return vec![];
                        };
                        self.first(self.stacked(&["column-header"], t.readable()))
                    }
                }
            }
            NoteTarget::Status {
                status,
                series,
                category,
            } => {
                if series.is_some() || category.is_some() {
                    let mut out = vec![];
                    for s in &self.data.series {
                        if series.as_ref().is_some_and(|x| *x != s.id) {
                            continue;
                        }
                        for (c, v) in self.data.categories.iter().zip(&s.values) {
                            if category.as_ref().is_some_and(|x| x != c) {
                                continue;
                            }
                            let is = if status == "zero" {
                                v.value.is_some_and(|x| x.is_zero())
                            } else {
                                v.value.is_none() && v.status == *status
                            };
                            if is {
                                out.extend(self.value(&s.id, c));
                            }
                        }
                    }
                    return out;
                }
                self.stacked(NOTE_ROLES, &status_text(self.spec, status))
            }
            NoteTarget::Datum { series, category } => self.value(series, category),
            NoteTarget::Series { series } => {
                let Some(name) = series_text(self.data, series) else {
                    return vec![];
                };
                let mut got = self.stacked(SERIES_ROLES, &name);
                // rows named alike (two "5K Walk"s): the one on its row
                let row: Vec<f64> = self
                    .texts
                    .iter()
                    .filter(|t| t.value_of.as_ref().is_some_and(|(s, _)| s == series))
                    .map(|t| t.y)
                    .collect();
                if got.len() > 1 && !row.is_empty() {
                    let on: Vec<usize> = got
                        .iter()
                        .copied()
                        .filter(|&i| (self.texts[i].y - row[0]).abs() < 0.5 * self.em)
                        .collect();
                    if !on.is_empty() {
                        got = on;
                    }
                }
                self.first(got)
            }
            NoteTarget::Category { category } => {
                self.first(self.stacked(CATEGORY_ROLES, &category_text(self.spec, category)))
            }
        }
    }
}

fn describe(r: &NoteTarget) -> String {
    match r {
        NoteTarget::Element { name } => match name.as_str() {
            "title" => "the title".into(),
            "descriptive" => "the descriptive line".into(),
            "unit" => "the unit phrase".into(),
            "x_title" => "the axis title".into(),
            _ => "the names' heading".into(),
        },
        NoteTarget::Comment { index } => format!("comment {}", index + 1),
        NoteTarget::Stub { index, series } => format!(
            "stub column {}{}",
            index + 1,
            series
                .as_ref()
                .map(|s| format!(", row {s}"))
                .unwrap_or_default()
        ),
        NoteTarget::Status {
            status,
            series,
            category,
        } => format!(
            "every value {status}{}{}",
            series
                .as_ref()
                .map(|s| format!(" of {s}"))
                .unwrap_or_default(),
            category
                .as_ref()
                .map(|c| format!(" at {c}"))
                .unwrap_or_default()
        ),
        NoteTarget::Datum { series, category } => format!("the value of {series} at {category}"),
        NoteTarget::Series { series } => format!("the name of {series}"),
        NoteTarget::Category { category } => format!("the name of {category}"),
    }
}

fn short(s: &str, n: usize) -> String {
    let s = norm(s);
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>().trim_end())
    }
}

/// Words grouped where a line must not break: never before "&", never between a short
/// abbreviation and the number after it.
fn units(words: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for w in words {
        let abbrev = out.last().is_some_and(|l| {
            let last = l.split(' ').next_back().unwrap_or("");
            last.len() >= 2
                && last.len() <= 5
                && last.ends_with('.')
                && last[..last.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_alphabetic())
        });
        if let Some(l) = out.last_mut() {
            if l.ends_with(" &") || l == "&" || *w == "&" {
                l.push(' ');
                l.push_str(w);
                continue;
            }
            if abbrev && w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                l.push(' ');
                l.push_str(w);
                continue;
            }
        }
        out.push((*w).to_owned());
    }
    out
}

/// The lines of a note within `limit`, filled greedily at spaces, then measured whole.
fn wrap(meas: &Meas, role: &str, plain: &str, limit: f64) -> Result<Vec<String>> {
    let n = norm(plain);
    let words: Vec<&str> = n.split(' ').collect();
    let us = units(&words);
    let space = meas.width(role, "x x")? - 2. * meas.width(role, "x")?;
    let mut lines: Vec<Vec<String>> = vec![];
    let mut cur: Vec<String> = vec![];
    let mut cw = 0.;
    for u in &us {
        let w = meas.width(role, u)?;
        if !cur.is_empty() && cw + space + w > limit {
            lines.push(std::mem::take(&mut cur));
            cur.push(u.clone());
            cw = w;
        } else {
            cw = if cur.is_empty() { w } else { cw + space + w };
            cur.push(u.clone());
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    for _ in 0..us.len() {
        let mut over = None;
        for (i, ln) in lines.iter().enumerate() {
            if ln.len() > 1 && meas.width(role, &ln.join(" "))? > limit + 0.01 {
                over = Some(i);
                break;
            }
        }
        let Some(i) = over else { break };
        let moved = lines[i].pop().unwrap();
        if i + 1 < lines.len() {
            lines[i + 1].insert(0, moved);
        } else {
            lines.push(vec![moved]);
        }
    }
    Ok(lines.into_iter().map(|l| l.join(" ")).collect())
}

fn text_primitive(
    req: TextRequest,
    metrics: TextMetrics,
    x: f64,
    baseline: f64,
    classes: Vec<String>,
) -> Primitive {
    let ascent = metrics.baseline.unwrap_or(metrics.height);
    let role = req.role.clone();
    Primitive::Text {
        position: Point {
            x,
            y: baseline - ascent,
        },
        request: req,
        metrics,
        anchor: "start".into(),
        meta: Metadata {
            id: None,
            role,
            classes,
            ..Default::default()
        },
    }
}

fn get_mut<'p>(ps: &'p mut Vec<Primitive>, path: &[usize]) -> Option<&'p mut Primitive> {
    let (first, rest) = path.split_first()?;
    let p = ps.get_mut(*first)?;
    if rest.is_empty() {
        return Some(p);
    }
    match p {
        Primitive::Group { children, .. } => get_mut(children, rest),
        _ => None,
    }
}

fn children_mut<'p>(ps: &'p mut Vec<Primitive>, path: &[usize]) -> Option<&'p mut Vec<Primitive>> {
    if path.is_empty() {
        return Some(ps);
    }
    match get_mut(ps, path)? {
        Primitive::Group { children, .. } => Some(children),
        _ => None,
    }
}

/// The path of the first group of a class (pre-order).
fn find_group(ps: &[Primitive], class: &str, path: &mut Vec<usize>) -> Option<Vec<usize>> {
    for (i, p) in ps.iter().enumerate() {
        if let Primitive::Group { children, meta, .. } = p {
            path.push(i);
            if meta.classes.iter().any(|c| c == class) {
                return Some(path.clone());
            }
            if let Some(found) = find_group(children, class, path) {
                return Some(found);
            }
            path.pop();
        }
    }
    None
}

/// Lay a chart out, keeping room for its notes' marks, then mark the texts and set the
/// notes under the chart.
#[allow(clippy::too_many_arguments)]
pub fn solve(
    engine: &impl LayoutEngine,
    plan: &SemanticPlan,
    data: &PreparedData,
    notes: &[PreparedNote],
    scale: Option<&TrainedScale>,
    theme: &Theme,
    constraints: &LayoutConstraints,
    measurements: &Measurements,
) -> Result<LayoutStep> {
    if notes.is_empty() {
        return engine.solve(plan, data, scale, theme, constraints, measurements);
    }
    if !measurements.entries.is_empty() && measurements.context != constraints.measurement_context {
        return Err(error(
            "measurement context does not match note layout context",
        ));
    }
    let mut bare = plan.clone();
    if let Some(raw) = bare.spec.raw.as_object_mut() {
        raw.remove("notes");
    }
    let meas = Meas {
        theme,
        c: constraints,
        m: measurements,
        wanted: RefCell::new(BTreeMap::new()),
    };
    let kind = kind_of(theme);
    let gap = metric(theme, "--note-mark-gap").unwrap_or(0.);
    let rooms = rooms(&plan.spec, data, notes, &meas, &kind, gap)?;
    // 1. learn the texts the layout measures, from its own requests, round by round
    let mut known: BTreeMap<String, TextRequest> = BTreeMap::new();
    let mut chart = None;
    for _ in 0..16 {
        let mut expanded = Measurements {
            context: measurements.context.clone(),
            entries: BTreeMap::new(),
        };
        for (id, r) in &known {
            if let Some(v) = measurements.entries.get(id) {
                let mut v = v.clone();
                v.width += rooms.room(&r.role, r.content.readable());
                expanded.entries.insert(id.clone(), v);
            }
        }
        match engine.solve(&bare, data, scale, theme, constraints, &expanded)? {
            LayoutStep::Refused { findings } => return Ok(LayoutStep::Refused { findings }),
            LayoutStep::Complete { chart: c } => {
                chart = Some(c);
                break;
            }
            LayoutStep::NeedMeasurements { requests, .. } => {
                let mut fresh = false;
                for r in requests {
                    if !measurements.entries.contains_key(&r.id) {
                        meas.wanted.borrow_mut().insert(r.id.clone(), r.clone());
                    }
                    if known.insert(r.id.clone(), r).is_none() {
                        fresh = true;
                    }
                }
                if !meas.wanted.borrow().is_empty() {
                    break;
                }
                if !fresh {
                    return Err(error(
                        "the chart layout asks again for texts already measured",
                    ));
                }
            }
        }
    }
    let wanted: Vec<TextRequest> = meas.wanted.borrow().values().cloned().collect();
    let Some(mut chart) = chart.filter(|_| wanted.is_empty()) else {
        if wanted.is_empty() {
            return Err(error("the chart layout did not converge with its notes"));
        }
        if constraints.max_rounds == 0 {
            return Err(error("measurement round limit is zero"));
        }
        return Ok(LayoutStep::NeedMeasurements {
            requests: wanted,
            round: 1,
        });
    };
    apply(plan, data, notes, &meas, &rooms, &kind, gap, &mut chart)?;
    let wanted: Vec<TextRequest> = meas.wanted.borrow().values().cloned().collect();
    if !wanted.is_empty() {
        return Ok(LayoutStep::NeedMeasurements {
            requests: wanted,
            round: 2,
        });
    }
    Ok(LayoutStep::Complete { chart })
}

#[allow(clippy::too_many_arguments)]
fn apply(
    plan: &SemanticPlan,
    data: &PreparedData,
    notes: &[PreparedNote],
    meas: &Meas,
    rooms: &Rooms,
    kind: &str,
    gap: f64,
    chart: &mut PlacedChart,
) -> Result<()> {
    let theme = meas.theme;
    let em = metric(theme, "--label-size").unwrap_or(theme.font_size);
    let lift = metric(theme, "--note-mark-lift").unwrap_or(0.);
    let width_limit = meas.c.width;
    let mut texts = vec![];
    let mut marks = vec![];
    let mut bottom = f64::NEG_INFINITY;
    walk(
        &chart.primitives,
        &mut vec![],
        (0., 0.),
        &mut vec![],
        &mut texts,
        &mut marks,
        &mut bottom,
    );
    let texts: Vec<T> = texts
        .into_iter()
        .filter(|t| !t.ancestors.iter().any(|a| a == "inset" || a == "footer") && t.role != MARK)
        .collect();
    let order = reading(&texts, em);
    let finder = Finder {
        spec: &plan.spec,
        data,
        em,
        texts,
        marks,
        order,
        horizontal: chart.summary.get("orientation") == Some(&json!("horizontal")),
    };
    // what each note marks
    let mut hosts: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut missing = vec![];
    for (j, n) in notes.iter().enumerate() {
        for (i, r) in n.targets.iter().enumerate() {
            let got = finder.find(r);
            if got.is_empty() {
                missing.push((j, i, r.clone()));
            }
            for t in got {
                hosts.entry(t).or_default().insert(j);
            }
        }
    }
    let mut first: BTreeMap<usize, (usize, f64)> = BTreeMap::new();
    for (t, js) in &hosts {
        for j in js {
            let o = finder.order[t];
            let e = first.entry(*j).or_insert(o);
            if o < *e {
                *e = o;
            }
        }
    }
    let mut marked: Vec<usize> = first.keys().copied().collect();
    marked.sort_by(|a, b| first[a].partial_cmp(&first[b]).unwrap().then(a.cmp(b)));
    let letter: BTreeMap<usize, String> = marked
        .iter()
        .enumerate()
        .map(|(k, j)| (*j, mark_text(kind, k)))
        .collect();

    // the texts kept room for give it back; the marked ones take their marks
    let mut fixes: Vec<(Vec<usize>, TextMetrics, f64)> = vec![]; // path, true metrics, dx
    for (i, t) in finder.texts.iter().enumerate() {
        if hosts.contains_key(&i) || rooms.room(&t.role, &t.plain) <= 0. {
            continue;
        }
        if let Some(true_m) = meas.m.entries.get(&t.id) {
            if t.width > true_m.width + 0.01 {
                fixes.push((t.path.clone(), true_m.clone(), 0.));
            }
        }
    }
    let ref_asc = meas
        .ext(MARK, if kind == "symbols" { "0" } else { "x" })?
        .ascent;
    let mut inserts: Vec<(Vec<usize>, Primitive)> = vec![];
    let mut placed: Vec<(f64, usize, BTreeSet<usize>)> = vec![]; // mark right edge, host, notes
    let mut ordered: Vec<(&usize, &BTreeSet<usize>)> = hosts.iter().collect();
    ordered.sort_by(|a, b| finder.order[a.0].partial_cmp(&finder.order[b.0]).unwrap());
    for (h, js) in ordered {
        let t = &finder.texts[*h];
        let true_w = meas.m.entries.get(&t.id).map_or(t.width, |m| m.width);
        let mut ms: Vec<&usize> = js.iter().collect();
        ms.sort_by_key(|j| marked.iter().position(|x| x == *j));
        let code = ms
            .iter()
            .map(|j| letter[j].clone())
            .collect::<Vec<_>>()
            .join(",");
        let kept = (t.width - true_w).max(0.);
        let delta = if kept > 0. {
            kept
        } else {
            meas.width(MARK, &code)? + gap
        };
        let hang = t.anchor == "end"
            && CELL_ROLES.contains(&t.role.as_str())
            && t.ancestors.iter().any(|a| a == "table");
        let dx = if hang {
            0.
        } else {
            match t.anchor.as_str() {
                "middle" => delta / 2.,
                "end" => delta,
                _ => 0.,
            }
        };
        let true_m = meas.m.entries.get(&t.id).cloned().unwrap_or(TextMetrics {
            width: t.width,
            height: t.ascent + t.descent,
            baseline: Some(t.ascent),
        });
        fixes.push((t.path.clone(), true_m, -dx));
        // the host's box after the move, at its true width
        let x_anchor = t.x - dx;
        let x0 = x_anchor
            - match t.anchor.as_str() {
                "middle" => true_w / 2.,
                "end" => true_w,
                _ => 0.,
            };
        let right = x0 + true_w;
        let on_fill = {
            let (cx, cy) = (x0 + true_w / 2., t.y + (t.descent - t.ascent) / 2.);
            finder.marks.iter().any(|m| {
                m.classes.iter().any(|c| FILLS.contains(&c.as_str()))
                    && m.bbox.0 <= cx
                    && cx <= m.bbox.2
                    && m.bbox.1 <= cy
                    && cy <= m.bbox.3
                    && m.bbox.2 > m.bbox.0
            })
        };
        let (req, metrics, _) = meas.ext_of(MARK, &TextContent::plain(&code))?;
        let rise = meas.ext(&t.role, "H")?.ascent - ref_asc + lift;
        let mut classes = vec!["prose".to_owned()];
        if on_fill {
            classes.push(t.role.clone());
            classes.extend(t.mods.iter().cloned());
            classes.push("on-fill".into());
        }
        classes.push(MARK.into());
        let local_x = right + gap - t.offset.0;
        let local_y = t.y - rise - t.offset.1;
        let mut at = t.path.clone();
        *at.last_mut().unwrap() += 1;
        inserts.push((
            at,
            text_primitive(req, metrics.clone(), local_x, local_y, classes),
        ));
        placed.push((right + gap + metrics.width, *h, js.clone()));
    }
    for (path, m, dx) in fixes {
        if let Some(Primitive::Text {
            position, metrics, ..
        }) = get_mut(&mut chart.primitives, &path)
        {
            let old_ascent = metrics.baseline.unwrap_or(metrics.height);
            let new_ascent = m.baseline.unwrap_or(m.height);
            position.y += old_ascent - new_ascent;
            position.x += dx;
            *metrics = m;
        }
    }
    // insert the marks after their texts, later places first so that paths stay valid
    inserts.sort_by(|a, b| b.0.cmp(&a.0));
    for (path, p) in inserts {
        let (parent, idx) = path.split_at(path.len() - 1);
        if let Some(children) = children_mut(&mut chart.primitives, parent) {
            let i = idx[0].min(children.len());
            children.insert(i, p);
        }
    }
    let mut findings = vec![];
    // the figure keeps every mark: wider, up to layout.width; past it, an error
    let right = placed.iter().map(|p| p.0).fold(0.0f64, f64::max);
    if right > chart.width + 0.01 {
        if right > width_limit + 0.01 {
            let over = placed
                .iter()
                .find(|p| p.0 > width_limit + 0.01)
                .map(|p| finder.texts[p.1].plain.clone())
                .unwrap_or_default();
            findings.push(
                Finding::new(
                    RULE,
                    "error",
                    format!(
                        "the mark after “{over}” runs {:.1} pt past the chart's {width_limit:.0} pt: the figure is widened to keep it; widen the chart (layout.width) or shorten the text it follows",
                        right - width_limit
                    ),
                )
                .at("layout.width"),
            );
        }
        chart.width = right + 0.5;
    }
    // findings for what no text answers
    let unplaced: Vec<usize> = (0..notes.len())
        .filter(|j| !letter.contains_key(j))
        .collect();
    for (j, i, r) in &missing {
        let n = &notes[*j];
        if n.auto {
            findings.push(
                Finding::new(
                    RULE,
                    "info",
                    format!(
                        "the note “{}” of {}: nothing in the chart writes that value (a gap in a line has no text to mark), so the note stands under the chart without a mark",
                        short(&n.display, 48),
                        describe(r)
                    ),
                )
                .at("data.series"),
            );
        } else {
            let src = n.source_index.unwrap_or(*j);
            findings.push(
                Finding::new(
                    RULE,
                    "warn",
                    format!(
                        "note {}'s mark on {}: no text in the chart reads it{}",
                        src + 1,
                        describe(r),
                        if unplaced.contains(j) {
                            "; the note stands under the chart without a mark"
                        } else {
                            "; marked where the note's other places are"
                        }
                    ),
                )
                .at(format!("notes[{src}].on[{i}]")),
            );
        }
    }

    // the footer: footnotes in the order of their marks, then the plain notes
    let footer = find_group(&chart.primitives, "footer", &mut vec![]);
    let pitch = metric(theme, "--line-pitch").unwrap_or(12.5)
        * metric(theme, "--note-size").unwrap_or(8.)
        / em;
    let hang_gap = metric(theme, "--note-hang").unwrap_or(0.3 * em);
    let cap_n = meas.ext(FOOTNOTE, "H")?.ascent;
    let mut y = bottom.max(0.) + metric(theme, "--notes-gap").unwrap_or(em) + cap_n;
    {
        // the chart's own notes in its footer (a treemap's): the notes continue them
        let (mut all, mut ms, mut b) = (vec![], vec![], f64::NEG_INFINITY);
        walk(
            &chart.primitives,
            &mut vec![],
            (0., 0.),
            &mut vec![],
            &mut all,
            &mut ms,
            &mut b,
        );
        let own: Vec<&T> = all
            .iter()
            .filter(|t| t.ancestors.iter().any(|a| a == "footer"))
            .collect();
        if let Some(low) = own.iter().map(|t| t.bbox().3).reduce(f64::max) {
            if low >= b - 0.01 {
                y = own.iter().map(|t| t.y).fold(f64::NEG_INFINITY, f64::max) + pitch;
            }
        }
    }
    let mut indent: f64 = 0.;
    for j in &marked {
        indent = indent.max(meas.width(MARK, &letter[j])?);
    }
    if !marked.is_empty() {
        indent += hang_gap;
    }
    let on_line = token(theme, "--note-footer-marks").as_deref() == Some("baseline");
    let rise_f = if on_line { 0. } else { cap_n - ref_asc + lift };
    let width = chart.width;
    let mut footer_items = vec![];
    let mut max_bottom: f64 = f64::NEG_INFINITY;
    for &j in marked.iter().chain(&unplaced) {
        let n = &notes[j];
        let role = if letter.contains_key(&j) {
            FOOTNOTE
        } else {
            PLAIN
        };
        let x0 = if letter.contains_key(&j) { indent } else { 0. };
        let content = guides::content(&n.text)?.ok_or_else(|| error("note needs text"))?;
        let lines: Vec<TextContent> = match &content {
            TextContent::Plain { text } => wrap(meas, role, text, width - x0)?
                .into_iter()
                .map(TextContent::plain)
                .collect(),
            other => vec![other.clone()],
        };
        for (k, line) in lines.iter().enumerate() {
            let (req, metrics, e) = meas.ext_of(role, line)?;
            if x0 + e.width > width + 0.01 {
                findings.push(
                    Finding::new(
                        RULE,
                        "error",
                        format!(
                            "note {} cannot be wrapped to the chart's {width:.0} pt: “{}” is {:.0} pt wide; the figure is widened to keep it (shorten the word, or give the note as plain text to wrap)",
                            j + 1,
                            short(line.readable(), 48),
                            x0 + e.width
                        ),
                    )
                    .at(format!("notes[{j}].text")),
                );
                chart.width = chart.width.max(x0 + e.width + 0.5);
            }
            if k == 0 {
                if let Some(l) = letter.get(&j) {
                    let (mreq, mm, _) = meas.ext_of(MARK, &TextContent::plain(l))?;
                    footer_items.push(text_primitive(
                        mreq,
                        mm,
                        0.,
                        y - rise_f,
                        vec!["prose".into(), MARK.into()],
                    ));
                }
            }
            max_bottom = max_bottom.max(y + e.descent);
            footer_items.push(text_primitive(
                req,
                metrics,
                x0,
                y,
                vec!["prose".into(), role.into()],
            ));
            y += pitch;
        }
    }
    if !footer_items.is_empty() {
        match footer.and_then(|p| children_mut(&mut chart.primitives, &p)) {
            Some(children) => children.extend(footer_items),
            None => chart.primitives.push(Primitive::Group {
                children: footer_items,
                translate: Point { x: 0., y: 0. },
                clip: None,
                meta: Metadata {
                    classes: vec!["footer".into()],
                    ..Default::default()
                },
            }),
        }
        chart.height = chart.height.max(max_bottom + 1.0);
    }
    // the summary: each mark, what it is on, and the plain notes
    let mut on: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (_, h, js) in &placed {
        for j in js {
            on.entry(*j)
                .or_default()
                .push(finder.texts[*h].plain.clone());
        }
    }
    let mut items: Vec<String> = marked
        .iter()
        .map(|j| {
            let wh = on.get(j).cloned().unwrap_or_default();
            let mut shown = wh
                .iter()
                .take(3)
                .map(|w| format!("“{}”", short(w, 24)))
                .collect::<Vec<_>>()
                .join(", ");
            if wh.len() > 3 {
                shown += &format!(" and {} more", wh.len() - 3);
            }
            format!("{} on {shown}", letter[j])
        })
        .collect();
    if !unplaced.is_empty() {
        items.push(format!(
            "{} plain note{}",
            unplaced.len(),
            if unplaced.len() != 1 { "s" } else { "" }
        ));
    }
    if !items.is_empty() {
        chart.decisions.push(json!({
            "rule": RULE,
            "level": "locked",
            "text": format!(
                "notes under the chart, below everything else, left-aligned and wrapped to its {:.0} pt: {} (marks in reading order)",
                chart.width,
                items.join("; ")
            ),
            "guide": [],
        }));
    }
    let mut summary: Vec<Value> = marked
        .iter()
        .map(|j| json!({"mark": letter[j], "text": notes[*j].display, "on": on.get(j).cloned().unwrap_or_default()}))
        .collect();
    summary.extend(
        unplaced
            .iter()
            .map(|j| json!({"mark": null, "text": notes[*j].display})),
    );
    chart.summary.insert("notes".into(), Value::Array(summary));
    if chart.summary.contains_key("width") {
        chart
            .summary
            .insert("width".into(), json!(round2(chart.width)));
    }
    if chart.summary.contains_key("height") {
        chart
            .summary
            .insert("height".into(), json!(round2(chart.height)));
    }
    chart.findings.extend(findings);
    Ok(())
}

fn round2(v: f64) -> f64 {
    format!("{v:.2}").parse().unwrap_or(v)
}

#[cfg(test)]
mod tests {
    //! Translated from the original's tests/test_notes.py (Wrapping, test_suffixes_…): widths
    //! of 5 pt a character, as the original's test batch measures.
    use super::*;

    fn wrap5(text: &str, limit: f64) -> Vec<String> {
        let theme = Theme::default();
        let c = LayoutConstraints::default();
        let mut m = Measurements {
            context: c.measurement_context.clone(),
            entries: BTreeMap::new(),
        };
        for _ in 0..6 {
            let meas = Meas {
                theme: &theme,
                c: &c,
                m: &m,
                wanted: RefCell::new(BTreeMap::new()),
            };
            let lines = wrap(&meas, FOOTNOTE, text, limit).unwrap();
            let wanted: Vec<TextRequest> = meas.wanted.borrow().values().cloned().collect();
            if wanted.is_empty() {
                return lines;
            }
            for r in wanted {
                let w = 5. * r.content.readable().chars().count() as f64;
                m.entries.insert(
                    r.id,
                    TextMetrics {
                        width: w,
                        height: 8.,
                        baseline: Some(6.),
                    },
                );
            }
        }
        panic!("wrap did not converge")
    }

    #[test]
    fn suffixes_cover_every_last_line() {
        assert_eq!(
            suffixes("Finance & Admin"),
            ["Finance & Admin", "& Admin", "Admin"]
        );
    }

    #[test]
    fn greedy_lines_within_the_limit() {
        let lines = wrap5("one two three four five six", 65.);
        assert_eq!(lines, ["one two three", "four five six"]);
        assert!(lines.iter().all(|l| 5. * l.len() as f64 <= 65.));
    }

    #[test]
    fn never_before_ampersand_nor_after_an_abbreviation_before_its_number() {
        let lines = wrap5("Finance & Admin from Aug. 1, 2026", 50.);
        assert!(lines.iter().all(|l| !l.ends_with('&')));
        assert!(lines.iter().any(|l| l.contains("Aug. 1,")));
    }

    #[test]
    fn a_word_wider_than_the_limit_stands_alone() {
        let lines = wrap5("see https://example.org/a/very/long/path", 60.);
        assert_eq!(lines, ["see", "https://example.org/a/very/long/path"]);
    }
}
