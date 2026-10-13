//! An SVG source with its look resolved under a stylesheet (svglint.Drawing): the node tree with
//! transforms, the CSS cascade, paints, boxes and points, and the texts as bake typesets them.
use crate::css::{self, Selector, Weight};
use chartgen_colour_space::mix;
use regex::Regex;
use roxmltree::{Document, Node as XNode};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

pub type Matrix = [f64; 6];
pub const IDENTITY: Matrix = [1., 0., 0., 1., 0., 0.];

pub fn mul(m: &Matrix, n: &Matrix) -> Matrix {
    let [a1, b1, c1, d1, e1, f1] = *m;
    let [a2, b2, c2, d2, e2, f2] = *n;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

static TRANSFORM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(matrix|translate|scale|rotate|skewX|skewY)\s*\(([^)]*)\)").unwrap()
});
static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s,]+").unwrap());
static NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?").unwrap());
static PATH_CMD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([MmLlHhVvCcSsQqTtAaZz])([^MmLlHhVvCcSsQqTtAaZz]*)").unwrap());
static VAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"var\(\s*(--[\w-]+)\s*(?:,\s*([^()]*))?\)").unwrap());
static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^url\(\s*['"]?#([^)'"]+)['"]?\s*\)"#).unwrap());
static URL_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^url\(\s*['"]?#([^)'"]+)"#).unwrap());

pub fn transform(text: Option<&str>) -> Matrix {
    let mut m = IDENTITY;
    for c in TRANSFORM.captures_iter(text.unwrap_or("")) {
        let a: Vec<f64> = SPLIT
            .split(c[2].trim())
            .filter(|v| !v.is_empty())
            .filter_map(|v| v.parse().ok())
            .collect();
        let t = match &c[1] {
            "matrix" if a.len() == 6 => [a[0], a[1], a[2], a[3], a[4], a[5]],
            "translate" if !a.is_empty() => [1., 0., 0., 1., a[0], *a.get(1).unwrap_or(&0.)],
            "scale" if !a.is_empty() => [a[0], 0., 0., *a.get(1).unwrap_or(&a[0]), 0., 0.],
            "rotate" if !a.is_empty() => {
                let r = a[0].to_radians();
                let t = [r.cos(), r.sin(), -r.sin(), r.cos(), 0., 0.];
                if a.len() == 3 {
                    mul(
                        &mul(&[1., 0., 0., 1., a[1], a[2]], &t),
                        &[1., 0., 0., 1., -a[1], -a[2]],
                    )
                } else {
                    t
                }
            }
            "skewX" if !a.is_empty() => [1., 0., a[0].to_radians().tan(), 1., 0., 0.],
            "skewY" if !a.is_empty() => [1., a[0].to_radians().tan(), 0., 1., 0., 0.],
            _ => continue,
        };
        m = mul(&m, &t);
    }
    m
}

pub fn apply(m: &Matrix, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

pub type Box4 = [f64; 4];

pub fn tbox(m: &Matrix, b: Box4) -> Box4 {
    let pts = [
        apply(m, b[0], b[1]),
        apply(m, b[2], b[1]),
        apply(m, b[0], b[3]),
        apply(m, b[2], b[3]),
    ];
    [
        pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
        pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    ]
}

/// Whether a transform turns, skews or mirrors what it draws.
pub fn turned(m: &Matrix) -> bool {
    m[1].abs() > 1e-6 || m[2].abs() > 1e-6 || m[0] < 0. || m[3] < 0.
}

pub fn overlap(a: &Box4, b: &Box4) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// The text a reader sees in a label's Typst markup.
pub fn unescape(code: &str) -> String {
    static ESC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\\(.)").unwrap());
    ESC.replace_all(code, "$1").into_owned()
}

pub const MARKS: &[&str] = &[
    "bar",
    "line",
    "projection",
    "point",
    "dot",
    "segment",
    "slice",
    "area",
    "change-shaft",
    "change-head",
    "change-start",
    "step-line",
    "event-bar",
    "target-mark",
    "reference-line",
    "cell",
    "incomplete-mark",
    "tile",
    "cell-bar",
];
pub const FORBIDDEN: &[(&str, &str, &str, &str)] = &[
    (
        "chart-frame",
        "R1-03",
        "frame round the chart",
        "the chart is clear without it (G3.3)",
    ),
    (
        "plot-frame",
        "R1-03",
        "frame round the plot",
        "the gridlines already mark out the plot (G3.3)",
    ),
    (
        "mark-outline",
        "R1-03",
        "outlined mark",
        "marks are flat, with no outline (G3.3)",
    ),
    (
        "tick",
        "R1-04",
        "tick mark",
        "labels sit centred on their gridlines and bands, so ticks add nothing (G3.4)",
    ),
    (
        "axis-line",
        "R1-05",
        "axis line",
        "it adds nothing where gridlines are drawn (G3.5)",
    ),
];
pub const NON_DATA: &[&str] = &[
    "gridline",
    "time-divider",
    "inset-frame",
    "inset-source",
    "inset-link",
    "panel-divider",
];
pub fn text_modifier(c: &str) -> bool {
    matches!(
        c,
        "text-strong"
            | "inset-text"
            | "text-good"
            | "text-bad"
            | "text-good-cvd"
            | "text-bad-cvd"
            | "on-fill"
    ) || c
        .strip_prefix("text-group-")
        .and_then(|k| k.parse::<u32>().ok())
        .is_some_and(|k| (1..7).contains(&k))
}

fn local(tag: &str) -> &str {
    tag.rsplit('}').next().unwrap_or(tag)
}

pub struct Node<'a> {
    pub el: XNode<'a, 'a>,
    pub tag: String,
    pub classes: Vec<String>,
    pub parent: Option<usize>,
    pub order: usize,
    pub ctm: Matrix,
}

impl Node<'_> {
    pub fn has(&self, c: &str) -> bool {
        self.classes.iter().any(|x| x == c)
    }
    pub fn has_any(&self, cs: &[&str]) -> bool {
        cs.iter().any(|c| self.has(c))
    }
    pub fn get(&self, a: &str) -> Option<&str> {
        self.el.attribute(a)
    }
    pub fn kind(&self) -> Option<&'static str> {
        MARKS.iter().copied().find(|m| self.has(m))
    }
    pub fn role(&self) -> String {
        if self.tag == "text" {
            return self
                .classes
                .iter()
                .filter(|c| *c != "prose" && *c != "math" && !text_modifier(c))
                .last()
                .cloned()
                .unwrap_or_else(|| "text".into());
        }
        if let Some(k) = self.kind() {
            return k.into();
        }
        if let Some(f) = FORBIDDEN.iter().find(|f| self.has(f.0)) {
            return f.0.into();
        }
        if let Some(n) = NON_DATA.iter().find(|c| self.has(c)) {
            return (*n).into();
        }
        self.classes
            .first()
            .cloned()
            .unwrap_or_else(|| self.tag.clone())
    }
}

/// All text inside an element, in document order (ElementTree's itertext).
pub fn itertext(el: XNode) -> String {
    el.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
}

#[derive(Debug, Clone)]
pub struct TextItem {
    pub node: usize,
    pub role: String,
    pub mode: String,
    pub code: String,
    pub plain: String,
    pub size: f64,
    pub fill: String,
    pub weight: Option<Weight>,
    pub slant: Option<String>,
    pub x: f64,
    pub y: f64,
    pub anchor: String,
    pub central: bool,
    pub extent: Option<Extent>,
    pub bx: Option<Box4>,
}

/// A text the host measures: the label as bake typesets it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TextQuery {
    pub mode: String,
    pub code: String,
    pub size: f64,
    pub weight: Option<String>,
    pub slant: Option<String>,
}

pub struct Rule {
    pub sel: Selector,
    pub order: usize,
    pub decls: BTreeMap<String, String>,
}

/// A node as the cascade sees it: a real one, or a ghost with other classes (paint_as).
#[derive(Clone)]
pub struct View<'v> {
    pub idx: Option<usize>,
    pub real: usize,
    pub classes: &'v [String],
    pub ghost: bool,
}

pub struct Drawing<'a> {
    pub nodes: Vec<Node<'a>>,
    pub by_id: HashMap<String, XNode<'a, 'a>>,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub page: String,
    pub em: f64,
    pub css: String,
    pub sheet: css::Sheet,
    pub tokens: BTreeMap<String, String>,
    pub rules: Vec<Rule>,
    pub drawn: Vec<usize>,
    pub marks: Vec<usize>,
    pub texts: Vec<TextItem>,
    pub chart: Vec<String>,
    pub orientation: Option<&'static str>,
    values: RefCell<HashMap<(usize, String), Option<String>>>,
    matched: RefCell<HashMap<usize, std::rc::Rc<Vec<usize>>>>,
    paints: RefCell<HashMap<(usize, bool), Vec<String>>>,
    boxes: RefCell<HashMap<usize, Option<Box4>>>,
}

/// The style the drawing will be baked under: the flattened stylesheet text and its tokens.
pub struct Style<'s> {
    pub css: &'s str,
    pub tokens: &'s BTreeMap<String, String>,
}

impl<'s> Style<'s> {
    pub fn metric(&self, name: &str) -> Option<f64> {
        css::float(self.tokens.get(name).map(String::as_str))
    }
}

impl<'a> Drawing<'a> {
    pub fn new(doc: &'a Document<'a>, style: Option<&Style>, final_width: Option<f64>) -> Self {
        let root = doc.root_element();
        let vb: Vec<String> = root
            .attribute("viewBox")
            .unwrap_or("")
            .replace(',', " ")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let (width, height) = if vb.len() == 4 {
            (vb[2].parse().unwrap_or(0.), vb[3].parse().unwrap_or(0.))
        } else {
            (
                css::float(root.attribute("width")).unwrap_or(0.),
                css::float(root.attribute("height")).unwrap_or(0.),
            )
        };
        let scale = match final_width {
            Some(fw) if fw != 0. && width != 0. => fw / width,
            _ => 1.,
        };
        let page = style
            .and_then(|s| s.tokens.get("--page").cloned())
            .unwrap_or_else(|| "#ffffff".into());
        let em = style.and_then(|s| s.metric("--label-size")).unwrap_or(10.);
        let by_id = doc
            .descendants()
            .filter(|n| n.is_element())
            .filter_map(|n| n.attribute("id").map(|id| (id.to_owned(), n)))
            .collect();
        let mut d = Drawing {
            nodes: vec![],
            by_id,
            width,
            height,
            scale,
            page,
            em,
            css: String::new(),
            sheet: css::Sheet::new(""),
            tokens: BTreeMap::new(),
            rules: vec![],
            drawn: vec![],
            marks: vec![],
            texts: vec![],
            chart: vec![],
            orientation: None,
            values: RefCell::new(HashMap::new()),
            matched: RefCell::new(HashMap::new()),
            paints: RefCell::new(HashMap::new()),
            boxes: RefCell::new(HashMap::new()),
        };
        d.build(root, None, IDENTITY);
        let mut sheets = vec![];
        let styles: Vec<XNode> = doc
            .descendants()
            .filter(|n| n.is_element() && local(n.tag_name().name()) == "style")
            .collect();
        for s in &styles {
            match style {
                Some(st) if s.attribute("id") == Some("house") => sheets.push(st.css.to_owned()),
                _ => sheets.push(itertext_direct(*s)),
            }
        }
        if let Some(st) = style {
            if !styles.iter().any(|s| s.attribute("id") == Some("house")) {
                sheets.insert(0, st.css.to_owned());
            }
            d.tokens = st.tokens.clone();
        }
        d.css = sheets.join("\n");
        d.sheet = css::Sheet::new(&d.css);
        for (order, (sel, decls)) in css::parse(&css::strip_comments(&d.css))
            .into_iter()
            .enumerate()
        {
            let mut map = BTreeMap::new();
            for (p, v) in decls {
                let v = css::re(r"\s*!important\s*$").replace(&v, "").into_owned();
                map.insert(p, v);
            }
            if sel.trim() == ":root" {
                for (p, v) in &map {
                    if p.starts_with("--") {
                        d.tokens.insert(p.clone(), v.clone());
                    }
                }
            }
            for one in sel.split(',') {
                if let Some(s) = css::selector(one) {
                    d.rules.push(Rule {
                        sel: s,
                        order,
                        decls: map.clone(),
                    });
                }
            }
        }
        const DRAWN_TAGS: &[&str] = &[
            "rect", "path", "line", "circle", "ellipse", "use", "polyline", "polygon",
        ];
        d.drawn = (0..d.nodes.len())
            .filter(|&i| {
                let t = d.nodes[i].tag.as_str();
                (DRAWN_TAGS.contains(&t) || t == "text" || t == "g") && d.visible(i)
            })
            .collect();
        d.marks = d
            .drawn
            .iter()
            .copied()
            .filter(|&i| {
                DRAWN_TAGS.contains(&d.nodes[i].tag.as_str()) && d.nodes[i].kind().is_some()
            })
            .collect();
        d.texts = d
            .drawn
            .clone()
            .into_iter()
            .filter(|&i| d.nodes[i].tag == "text")
            .map(|i| d.text(i))
            .collect();
        d.chart = d
            .nodes
            .iter()
            .find(|n| n.has("chart"))
            .map(|n| n.classes.clone())
            .unwrap_or_default();
        d.orientation = if d.chart.iter().any(|c| c == "vertical") {
            Some("vertical")
        } else if d.chart.iter().any(|c| c == "horizontal") {
            Some("horizontal")
        } else {
            None
        };
        d
    }

    fn build(&mut self, el: XNode<'a, 'a>, parent: Option<usize>, ctm: Matrix) {
        if !el.is_element() {
            return;
        }
        let tag = local(el.tag_name().name()).to_owned();
        let m = mul(&ctm, &transform(el.attribute("transform")));
        let idx = self.nodes.len();
        self.nodes.push(Node {
            el,
            tag: tag.clone(),
            classes: el
                .attribute("class")
                .unwrap_or("")
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
            parent,
            order: idx,
            ctm: m,
        });
        const LEAVES: &[&str] = &[
            "defs",
            "style",
            "title",
            "desc",
            "metadata",
            "symbol",
            "clipPath",
            "mask",
            "text",
            "linearGradient",
            "radialGradient",
            "pattern",
            "filter",
            "marker",
        ];
        if LEAVES.contains(&tag.as_str()) {
            return;
        }
        for c in el.children() {
            self.build(c, Some(idx), m);
        }
    }

    /// Self and every ancestor, innermost first.
    pub fn up(&self, i: usize) -> Vec<usize> {
        let mut out = vec![i];
        let mut n = self.nodes[i].parent;
        while let Some(p) = n {
            out.push(p);
            n = self.nodes[p].parent;
        }
        out
    }
    pub fn within(&self, i: usize, cls: &str) -> bool {
        let mut n = Some(i);
        while let Some(k) = n {
            if self.nodes[k].has(cls) {
                return true;
            }
            n = self.nodes[k].parent;
        }
        false
    }
    /// An attribute, or the nearest ancestor's.
    pub fn attr(&self, i: usize, name: &str) -> Option<String> {
        self.up(i)
            .into_iter()
            .find_map(|a| self.nodes[a].get(name).map(str::to_owned))
    }

    pub fn reference(&self, i: usize) -> String {
        let n = &self.nodes[i];
        if let Some(id) = n.get("id") {
            return format!("#{id}");
        }
        let mut s = format!(
            "{}.{}",
            n.tag,
            if n.tag == "g" && !n.classes.is_empty() {
                n.classes.join(".")
            } else {
                n.role()
            }
        );
        if n.kind().is_some() {
            if let Some(v) = self.attr(i, "data-series") {
                s.push_str(&format!("[data-series={v}]"));
            }
        }
        for a in ["data-x", "data-value"] {
            if let Some(v) = n.get(a) {
                s.push_str(&format!("[{a}={v}]"));
                break;
            }
        }
        if n.tag == "text" {
            let t = itertext(n.el);
            s.push_str(&format!(
                " “{}”",
                unescape(&t.split_whitespace().collect::<Vec<_>>().join(" "))
            ));
        }
        s
    }

    // ---- the cascade ----
    fn compound_matches(&self, c: &css::Compound, i: usize, classes: &[String]) -> bool {
        let n = &self.nodes[i];
        c.tag.as_ref().is_none_or(|t| *t == n.tag)
            && c.id.as_ref().is_none_or(|id| n.get("id") == Some(id))
            && c.classes.iter().all(|x| classes.contains(x))
            && (!c.root || n.parent.is_none())
    }
    fn matches(&self, sel: &Selector, k: usize, i: usize, classes: &[String]) -> bool {
        if !self.compound_matches(&sel.parts[k].1, i, classes) {
            return false;
        }
        if k == 0 {
            return true;
        }
        let child = sel.parts[k].0;
        let mut p = self.nodes[i].parent;
        if child {
            return p.is_some_and(|p| self.matches(sel, k - 1, p, &self.nodes[p].classes));
        }
        while let Some(q) = p {
            if self.matches(sel, k - 1, q, &self.nodes[q].classes) {
                return true;
            }
            p = self.nodes[q].parent;
        }
        false
    }

    fn view(&self, i: usize) -> View<'_> {
        View {
            idx: Some(i),
            real: i,
            classes: &self.nodes[i].classes,
            ghost: false,
        }
    }

    /// The rules matching a node, by specificity and order (cached for real nodes).
    fn rules_for(&self, v: &View) -> std::rc::Rc<Vec<usize>> {
        if let Some(idx) = v.idx {
            if let Some(m) = self.matched.borrow().get(&idx) {
                return m.clone();
            }
        }
        let mut found: Vec<usize> = (0..self.rules.len())
            .filter(|&k| {
                let r = &self.rules[k];
                self.matches(&r.sel, r.sel.parts.len() - 1, v.real, v.classes)
            })
            .collect();
        found.sort_by_key(|&k| (self.rules[k].sel.spec, self.rules[k].order));
        let found = std::rc::Rc::new(found);
        if let Some(idx) = v.idx {
            self.matched.borrow_mut().insert(idx, found.clone());
        }
        found
    }

    pub fn value(&self, i: usize, prop: &str) -> Option<String> {
        self.value_of(&self.view(i), prop)
    }

    fn value_of(&self, v: &View, prop: &str) -> Option<String> {
        if let Some(idx) = v.idx {
            if let Some(c) = self.values.borrow().get(&(idx, prop.to_owned())) {
                return c.clone();
            }
        }
        let n = &self.nodes[v.real];
        let mut val = if v.ghost {
            None
        } else {
            css::inline(n.get("style"), prop)
        };
        if val.is_none() {
            for k in self.rules_for(v).iter() {
                if let Some(x) = self.rules[*k].decls.get(prop) {
                    val = Some(x.clone());
                }
            }
        }
        if val.is_none() && !(v.ghost && matches!(prop, "style" | "fill" | "stroke")) {
            val = n.get(prop).map(str::to_owned);
        }
        if val.as_deref().map(str::trim) == Some("inherit") {
            val = None;
        }
        const INHERITED: &[&str] = &[
            "fill",
            "stroke",
            "stroke-width",
            "stroke-dasharray",
            "fill-opacity",
            "stroke-opacity",
            "color",
            "font-size",
            "font-weight",
            "font-style",
            "font-family",
            "writing-mode",
            "visibility",
            "glyph-orientation-vertical",
            "text-orientation",
        ];
        if val.is_none() && INHERITED.contains(&prop) {
            if let Some(p) = n.parent {
                val = self.value(p, prop);
            }
        }
        if val.is_none() {
            val = match prop {
                "fill" => Some("#000000"),
                "stroke" => Some("none"),
                "stroke-width" | "opacity" | "fill-opacity" | "stroke-opacity" => Some("1"),
                "visibility" => Some("visible"),
                "display" => Some("inline"),
                _ => None,
            }
            .map(str::to_owned);
        }
        if let Some(x) = &val {
            if x.contains("var(") {
                let mut s = x.clone();
                for _ in 0..10 {
                    let Some(c) = VAR.captures(&s) else { break };
                    let m = c.get(0).unwrap();
                    let rep = self
                        .tokens
                        .get(&c[1])
                        .cloned()
                        .filter(|t| !t.is_empty())
                        .or_else(|| {
                            c.get(2)
                                .map(|f| f.as_str().to_owned())
                                .filter(|f| !f.is_empty())
                        })
                        .unwrap_or_default();
                    s = format!("{}{}{}", &s[..m.start()], rep, &s[m.end()..]);
                }
                val = Some(s);
            }
        }
        if let Some(idx) = v.idx {
            self.values
                .borrow_mut()
                .insert((idx, prop.to_owned()), val.clone());
        }
        val
    }

    pub fn number(&self, i: usize, prop: &str) -> f64 {
        css::float(self.value(i, prop).as_deref()).unwrap_or(0.)
    }

    pub fn visible(&self, i: usize) -> bool {
        self.up(i)
            .iter()
            .all(|&a| self.value(a, "display").as_deref() != Some("none"))
            && !matches!(
                self.value(i, "visibility").as_deref(),
                Some("hidden" | "collapse")
            )
    }

    fn alpha_of(&self, v: &View, prop: &str) -> f64 {
        let mut a =
            css::float(self.value_of(v, &format!("{prop}-opacity")).as_deref()).unwrap_or(1.);
        a *= css::float(self.value_of(v, "opacity").as_deref()).unwrap_or(1.);
        for x in self.up(v.real).into_iter().skip(1) {
            a *= css::float(self.value(x, "opacity").as_deref()).unwrap_or(1.);
        }
        a.clamp(0., 1.)
    }

    fn paint_of(&self, v: &View, prop: &str) -> Vec<String> {
        let mut val = self.value_of(v, prop).unwrap_or_default().trim().to_owned();
        if val.to_lowercase() == "currentcolor" {
            val = self
                .value_of(v, "color")
                .unwrap_or_else(|| "#000000".into());
        }
        let a = self.alpha_of(v, prop);
        if let Some(c) = URL.captures(&val) {
            return self
                .stops(&c[1], 0)
                .into_iter()
                .filter_map(|(c, sa)| mix(&c, &self.page, 1. - a * sa))
                .collect();
        }
        match css::colour(Some(&val)) {
            Some((c, ca)) if ca * a > 0. => mix(&c, &self.page, 1. - ca * a).into_iter().collect(),
            _ => vec![],
        }
    }

    /// The colours a paint property shows, blended with the page by opacity.
    pub fn paint(&self, i: usize, prop: &str) -> Vec<String> {
        if let Some(p) = self.paints.borrow().get(&(i, prop == "fill")) {
            return p.clone();
        }
        let p = self.paint_of(&self.view(i), prop);
        if matches!(prop, "fill" | "stroke") {
            self.paints
                .borrow_mut()
                .insert((i, prop == "fill"), p.clone());
        }
        p
    }

    pub fn gradient(&self, i: usize, prop: &str) -> bool {
        let v = self.value(i, prop).unwrap_or_default();
        URL_ID
            .captures(v.trim())
            .and_then(|c| self.by_id.get(&c[1]))
            .is_some_and(|el| {
                matches!(
                    local(el.tag_name().name()),
                    "linearGradient" | "radialGradient"
                )
            })
    }

    fn stops(&self, ident: &str, depth: usize) -> Vec<(String, f64)> {
        let Some(el) = self.by_id.get(ident) else {
            return vec![];
        };
        if depth > 5 {
            return vec![];
        }
        let mut out = vec![];
        for s in el
            .children()
            .filter(|c| c.is_element() && local(c.tag_name().name()) == "stop")
        {
            let c = css::colour(Some(
                &css::style_value(
                    s.attribute("style"),
                    s.attribute("stop-color"),
                    "stop-color",
                )
                .unwrap_or_else(|| "#000000".into()),
            ));
            if let Some((c, ca)) = c {
                let so = css::float(
                    css::style_value(
                        s.attribute("style"),
                        s.attribute("stop-opacity"),
                        "stop-opacity",
                    )
                    .as_deref(),
                )
                .unwrap_or(1.);
                out.push((c, ca * so));
            }
        }
        let href = el
            .attribute("href")
            .or_else(|| el.attribute(("http://www.w3.org/1999/xlink", "href")));
        if out.is_empty() {
            if let Some(h) = href.and_then(|h| h.strip_prefix('#')) {
                return self.stops(h, depth + 1);
            }
        }
        out
    }

    /// The data colours a mark shows: its stroke for a line, its fill otherwise.
    pub fn colours(&self, i: usize) -> Vec<String> {
        const STROKED: &[&str] = &[
            "line",
            "projection",
            "step-line",
            "change-shaft",
            "reference-line",
            "incomplete-mark",
        ];
        if self.nodes[i].kind().is_some_and(|k| STROKED.contains(&k)) {
            return self.paint(i, "stroke");
        }
        let f = self.paint(i, "fill");
        if !f.is_empty() {
            return f;
        }
        if self.number(i, "stroke-width") > 0. {
            self.paint(i, "stroke")
        } else {
            vec![]
        }
    }

    /// The paint a mark would show with other classes and none of its own.
    pub fn paint_as(&self, i: usize, classes: &[String], prop: &str) -> Vec<String> {
        let v = View {
            idx: None,
            real: i,
            classes,
            ghost: true,
        };
        self.paint_of(&v, prop)
    }

    // ---- geometry ----
    pub fn bx(&self, i: usize) -> Option<Box4> {
        if let Some(b) = self.boxes.borrow().get(&i) {
            return *b;
        }
        let b = self.compute_box(i);
        self.boxes.borrow_mut().insert(i, b);
        b
    }
    fn compute_box(&self, i: usize) -> Option<Box4> {
        let n = &self.nodes[i];
        let f = |a: &str| css::float(n.get(a)).unwrap_or(0.);
        let b = match n.tag.as_str() {
            "rect" | "use" => [f("x"), f("y"), f("x") + f("width"), f("y") + f("height")],
            "circle" => [
                f("cx") - f("r"),
                f("cy") - f("r"),
                f("cx") + f("r"),
                f("cy") + f("r"),
            ],
            "ellipse" => [
                f("cx") - f("rx"),
                f("cy") - f("ry"),
                f("cx") + f("rx"),
                f("cy") + f("ry"),
            ],
            "line" => [
                f("x1").min(f("x2")),
                f("y1").min(f("y2")),
                f("x1").max(f("x2")),
                f("y1").max(f("y2")),
            ],
            _ => {
                let pts = self.points(i);
                if pts.is_empty() {
                    return None;
                }
                return Some([
                    pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
                    pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
                    pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
                    pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
                ]);
            }
        };
        Some(tbox(&n.ctm, b))
    }

    /// The points of a line, polyline, polygon or path, in the SVG's frame.
    pub fn points(&self, i: usize) -> Vec<(f64, f64)> {
        let n = &self.nodes[i];
        let nums = |s: &str| -> Vec<f64> {
            NUM.find_iter(s)
                .filter_map(|m| m.as_str().parse().ok())
                .collect()
        };
        let raw: Vec<(f64, f64)> = match n.tag.as_str() {
            "line" => {
                let f = |a: &str| css::float(n.get(a)).unwrap_or(0.);
                vec![(f("x1"), f("y1")), (f("x2"), f("y2"))]
            }
            "polyline" | "polygon" => {
                let v = nums(n.get("points").unwrap_or(""));
                v.chunks_exact(2).map(|c| (c[0], c[1])).collect()
            }
            "path" => {
                let mut raw = vec![];
                let (mut x, mut y) = (0., 0.);
                for c in PATH_CMD.captures_iter(n.get("d").unwrap_or("")) {
                    let cmd = c[1].chars().next().unwrap();
                    let v = nums(&c[2]);
                    let rel = cmd.is_lowercase();
                    match cmd.to_ascii_uppercase() {
                        'M' | 'L' | 'T' => {
                            let mut k = 0;
                            while k + 1 < v.len() {
                                (x, y) = if rel {
                                    (x + v[k], y + v[k + 1])
                                } else {
                                    (v[k], v[k + 1])
                                };
                                raw.push((x, y));
                                k += 2;
                            }
                        }
                        'H' => {
                            for &h in &v {
                                x = if rel { x + h } else { h };
                                raw.push((x, y));
                            }
                        }
                        'V' => {
                            for &h in &v {
                                y = if rel { y + h } else { h };
                                raw.push((x, y));
                            }
                        }
                        c @ ('C' | 'S' | 'Q') => {
                            let step = if c == 'C' { 6 } else { 4 };
                            let mut k = 0;
                            while k + step <= v.len() {
                                let (bx, by) = (x, y);
                                for j in (0..step).step_by(2) {
                                    raw.push(if rel {
                                        (bx + v[k + j], by + v[k + j + 1])
                                    } else {
                                        (v[k + j], v[k + j + 1])
                                    });
                                }
                                (x, y) = *raw.last().unwrap();
                                k += step;
                            }
                        }
                        'A' => {
                            let mut k = 0;
                            while v.len() >= 7 && k < v.len() - 6 {
                                (x, y) = if rel {
                                    (x + v[k + 5], y + v[k + 6])
                                } else {
                                    (v[k + 5], v[k + 6])
                                };
                                raw.push((x, y));
                                k += 7;
                            }
                        }
                        _ => {}
                    }
                }
                raw
            }
            _ => return vec![],
        };
        raw.into_iter()
            .map(|(px, py)| apply(&n.ctm, px, py))
            .collect()
    }

    // ---- texts ----
    fn text(&self, i: usize) -> TextItem {
        let n = &self.nodes[i];
        let mut classes = n.classes.clone();
        let mode = if classes.iter().any(|c| c == "math") {
            "math"
        } else {
            "prose"
        };
        let code = itertext(n.el)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let own = |p: &str| css::style_value(n.get("style"), n.get(p), p);
        if !classes.iter().any(|c| c == "prose" || c == "math") {
            classes.push("prose".into());
        }
        let (size, fill, weight, slant) = css::label_style(&self.sheet, &classes, &own);
        let tspan =
            n.el.descendants()
                .find(|t| t.is_element() && local(t.tag_name().name()) == "tspan");
        let pick = |a: &str| {
            n.get(a)
                .filter(|v| !v.is_empty())
                .or_else(|| tspan.and_then(|t| t.attribute(a)))
        };
        let anchor = css::style_value(n.get("style"), n.get("text-anchor"), "text-anchor")
            .unwrap_or_else(|| "start".into());
        let central = matches!(
            css::style_value(
                n.get("style"),
                n.get("dominant-baseline"),
                "dominant-baseline"
            )
            .as_deref(),
            Some("central" | "middle")
        );
        TextItem {
            node: i,
            role: n.role(),
            mode: mode.into(),
            plain: unescape(&code),
            code,
            size,
            fill,
            weight,
            slant,
            x: css::float(pick("x")).unwrap_or(0.),
            y: css::float(pick("y")).unwrap_or(0.),
            anchor,
            central,
            extent: None,
            bx: None,
        }
    }

    pub fn set_extents(&mut self, extents: &[Option<Extent>]) {
        for (t, e) in self.texts.iter_mut().zip(extents) {
            let Some(e) = e else { continue };
            t.extent = Some(*e);
            let h = e.ascent + e.descent;
            let x0 = t.x
                - match t.anchor.as_str() {
                    "middle" => e.width / 2.,
                    "end" => e.width,
                    _ => 0.,
                };
            let top = t.y - if t.central { h / 2. } else { e.ascent };
            t.bx = Some(tbox(
                &self.nodes[t.node].ctm,
                [x0, top, x0 + e.width, top + h],
            ));
        }
    }

    /// The text's anchor in the SVG's frame (its box centre when measured).
    pub fn point(&self, t: &TextItem) -> (f64, f64) {
        if let Some(b) = t.bx {
            return ((b[0] + b[2]) / 2., (b[1] + b[3]) / 2.);
        }
        apply(
            &self.nodes[t.node].ctm,
            t.x,
            t.y - if t.central { 0. } else { 0.35 * t.size },
        )
    }

    pub fn style_key(t: &TextItem) -> (String, String, Option<String>, Option<String>) {
        (
            format!("{}", crate::py_round(t.size, 2)),
            t.fill.clone(),
            t.weight.as_ref().map(|w| w.to_string()),
            t.slant.clone(),
        )
    }

    // ---- groupings ----
    pub fn panel_of(&self, i: usize) -> Option<usize> {
        self.up(i)
            .into_iter()
            .find(|&a| self.nodes[a].has_any(&["panel", "inset-panel"]))
    }
    pub fn panels(&self) -> Vec<(Option<usize>, Vec<usize>)> {
        let ps: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].has_any(&["panel", "inset-panel"]))
            .collect();
        if ps.is_empty() {
            return vec![(None, self.drawn.clone())];
        }
        ps.into_iter()
            .map(|p| {
                (
                    Some(p),
                    self.drawn
                        .iter()
                        .copied()
                        .filter(|&n| self.panel_of(n) == Some(p))
                        .collect(),
                )
            })
            .collect()
    }
    pub fn series_key(&self, i: usize) -> Option<String> {
        if let Some(s) = self.attr(i, "data-series") {
            return Some(s);
        }
        if let Some(g) = self
            .up(i)
            .into_iter()
            .find(|&a| self.nodes[a].has("series"))
        {
            return Some(
                self.nodes[g]
                    .get("id")
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("series@{}", self.nodes[g].order)),
            );
        }
        self.nodes[i].has("single").then(|| "single".into())
    }
    pub fn texts_in(&self, nodes: &[usize], roles: &[&str]) -> Vec<usize> {
        let set: std::collections::HashSet<usize> = nodes.iter().copied().collect();
        (0..self.texts.len())
            .filter(|&k| {
                set.contains(&self.texts[k].node)
                    && (roles.is_empty() || roles.contains(&self.texts[k].role.as_str()))
            })
            .collect()
    }
}

/// The text of a style element (its own text, as ElementTree's `.text`).
fn itertext_direct(el: XNode) -> String {
    el.children()
        .take_while(|c| c.is_text())
        .filter_map(|c| c.text())
        .collect()
}
