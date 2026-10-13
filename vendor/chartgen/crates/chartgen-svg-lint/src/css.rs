//! The CSS the lint reads: figkit's parse and strip_comments, the lint's selectors and
//! specificity (svglint._selector), colours (svglint._colour) and bake's label lookup
//! (bake.label_style), as the original reads them.
use regex::Regex;
use std::sync::LazyLock;

static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
static RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([^{}]+)\{([^}]*)\}").unwrap());
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)").unwrap());
static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[.#]?-?[_a-zA-Z][\w-]*|\*|:[\w-]+(?:\([^)]*\))?|\[[^\]]*\]").unwrap()
});

/// A compiled regex, cached per pattern (the lint builds them from property and class names).
pub fn re(pattern: &str) -> Regex {
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<String, Regex>> = Default::default();
    }
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(pattern.to_owned())
            .or_insert_with(|| Regex::new(pattern).unwrap())
            .clone()
    })
}

pub fn strip_comments(css: &str) -> String {
    COMMENT.replace_all(css, "").into_owned()
}

/// `[(selector, [(property, value)])]` of a comment-free stylesheet (figkit style.parse).
pub fn parse(css: &str) -> Vec<(String, Vec<(String, String)>)> {
    RULE.captures_iter(css)
        .map(|c| {
            let decls = c[2]
                .split(';')
                .filter_map(|part| {
                    let (p, v) = part.split_once(':')?;
                    Some((
                        p.trim().to_owned(),
                        v.split_whitespace().collect::<Vec<_>>().join(" "),
                    ))
                })
                .collect();
            (c[1].split_whitespace().collect::<Vec<_>>().join(" "), decls)
        })
        .collect()
}

/// The leading number of a value (svglint._float).
pub fn float(v: Option<&str>) -> Option<f64> {
    NUMBER.captures(v?).and_then(|c| c[1].parse::<f64>().ok())
}

#[derive(Debug, Clone, Default)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub root: bool,
}

fn compound(text: &str) -> Option<Compound> {
    let mut c = Compound::default();
    for m in TOKEN.find_iter(text) {
        let tok = m.as_str();
        if let Some(cls) = tok.strip_prefix('.') {
            if !c.classes.iter().any(|x| x == cls) {
                c.classes.push(cls.into());
            }
        } else if let Some(id) = tok.strip_prefix('#') {
            c.id = Some(id.into());
        } else if tok == ":root" {
            c.root = true;
        } else if tok.starts_with(':') || tok.starts_with('[') {
            return None; // pseudo-classes, attributes: never matched
        } else if tok != "*" {
            c.tag = Some(tok.into());
        }
    }
    Some(c)
}

/// A selector: `[(combinator, compound)]` left to right, and its specificity.
#[derive(Debug, Clone)]
pub struct Selector {
    pub parts: Vec<(bool, Compound)>, // (child combinator, compound)
    pub spec: (usize, usize, usize),
}

pub fn selector(text: &str) -> Option<Selector> {
    let spaced = re(r"\s*>\s*").replace_all(text.trim(), " > ").into_owned();
    let mut parts = vec![];
    let mut child = false;
    for tok in spaced.split_whitespace() {
        if tok == ">" {
            child = true;
            continue;
        }
        parts.push((child, compound(tok)?));
        child = false;
    }
    if parts.is_empty() {
        return None;
    }
    let ids = parts.iter().filter(|(_, c)| c.id.is_some()).count();
    let cls = parts
        .iter()
        .map(|(_, c)| c.classes.len() + usize::from(c.root))
        .sum();
    let tags = parts.iter().filter(|(_, c)| c.tag.is_some()).count();
    Some(Selector {
        parts,
        spec: (ids, cls, tags),
    })
}

static NAMED: &[(&str, &str)] = &[
    ("black", "#000000"),
    ("white", "#ffffff"),
    ("red", "#ff0000"),
    ("green", "#008000"),
    ("blue", "#0000ff"),
    ("yellow", "#ffff00"),
    ("orange", "#ffa500"),
    ("grey", "#808080"),
    ("gray", "#808080"),
    ("silver", "#c0c0c0"),
    ("lightgrey", "#d3d3d3"),
    ("lightgray", "#d3d3d3"),
    ("darkgrey", "#a9a9a9"),
    ("darkgray", "#a9a9a9"),
    ("dimgrey", "#696969"),
    ("dimgray", "#696969"),
    ("magenta", "#ff00ff"),
    ("cyan", "#00ffff"),
    ("navy", "#000080"),
    ("purple", "#800080"),
    ("teal", "#008080"),
    ("maroon", "#800000"),
    ("olive", "#808000"),
];

fn py_round(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2. != 0. {
        r - x.signum()
    } else {
        r
    }
}

/// (#rrggbb, alpha) of a CSS colour, or None for none, transparent or a form not understood.
pub fn colour(v: Option<&str>) -> Option<(String, f64)> {
    let v = v.unwrap_or("").trim().to_lowercase();
    if let Some((_, h)) = NAMED.iter().find(|(n, _)| *n == v) {
        return Some(((*h).into(), 1.));
    }
    if let Some(h) = v.strip_prefix('#') {
        if (h.len() == 3 || h.len() == 6) && h.chars().all(|c| c.is_ascii_hexdigit()) {
            let h: String = if h.len() == 3 {
                h.chars().flat_map(|c| [c, c]).collect()
            } else {
                h.into()
            };
            return Some((format!("#{h}"), 1.));
        }
        return None;
    }
    let inner = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = re(r"[\s,/]+")
        .split(inner.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let ch = |p: &str| -> Option<f64> {
        Some(match p.strip_suffix('%') {
            Some(q) => q.parse::<f64>().ok()? * 2.55,
            None => p.parse::<f64>().ok()?,
        })
    };
    let a = if parts.len() < 4 {
        1.
    } else {
        match parts[3].strip_suffix('%') {
            Some(q) => q.parse::<f64>().ok()? / 100.,
            None => parts[3].parse::<f64>().ok()?,
        }
    };
    let hex: String = parts[..3]
        .iter()
        .map(|p| ch(p).map(|c| format!("{:02x}", py_round(c.clamp(0., 255.)) as u8)))
        .collect::<Option<_>>()?;
    Some((format!("#{hex}"), a))
}

/// A presentation value from an element's inline style or attribute (bake.style_value).
pub fn style_value(style: Option<&str>, attr: Option<&str>, name: &str) -> Option<String> {
    let r = re(&format!(r"(?:^|;)\s*{}\s*:\s*([^;]+)", regex::escape(name)));
    match r.captures(style.unwrap_or("")) {
        Some(c) => Some(c[1].trim().to_owned()),
        None => attr.map(str::to_owned),
    }
}

/// A property of the element's own style attribute only (svglint._inline).
pub fn inline(style: Option<&str>, prop: &str) -> Option<String> {
    let r = re(&format!(r"(?:^|;)\s*{}\s*:\s*([^;]+)", regex::escape(prop)));
    r.captures(style.unwrap_or(""))
        .map(|c| c[1].trim().to_owned())
}

/// A stylesheet's rules as bake reads them (selector text, body), with its lookups memoised.
pub struct Sheet {
    rules: Vec<(String, String)>,
    memo: std::cell::RefCell<std::collections::HashMap<(String, String), Option<String>>>,
}

impl Sheet {
    pub fn new(css: &str) -> Self {
        Sheet {
            rules: RULE
                .captures_iter(css)
                .map(|c| (c[1].to_owned(), c[2].to_owned()))
                .collect(),
            memo: Default::default(),
        }
    }
    /// bake.stylesheet_value: the first rule whose selector mentions the class and sets the property.
    fn value(&self, cls: &str, prop: &str) -> Option<String> {
        let key = (cls.to_owned(), prop.to_owned());
        if let Some(v) = self.memo.borrow().get(&key) {
            return v.clone();
        }
        let sel = re(&format!(r"\.{}([^\w-]|$)", regex::escape(cls)));
        let decl = re(&format!(r"(^|[^\w-]){}\s*:\s*([^;]+)", regex::escape(prop)));
        let mut out = None;
        for (s, body) in &self.rules {
            if sel.is_match(s) {
                if let Some(m) = decl.captures(body) {
                    out = Some(m[2].trim().to_owned());
                    break;
                }
            }
        }
        self.memo.borrow_mut().insert(key, out.clone());
        out
    }
}

fn font_size(v: &str) -> Option<f64> {
    let n: f64 = re(r"^\s*([-+\d.eE]+)").captures(v)?[1].parse().ok()?;
    Some(if v.trim().ends_with("pt") {
        n * 4. / 3.
    } else {
        n
    })
}

fn colour_value(v: &str) -> Option<String> {
    let t = v.trim();
    let h = t.strip_prefix('#')?;
    if !(h.len() == 3 || h.len() == 6) || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let h: String = if h.len() == 3 {
        h.chars().flat_map(|c| [c, c]).collect()
    } else {
        h.into()
    };
    Some(format!("#{}", h.to_lowercase()))
}

/// A Typst text weight: `bold`, `light`, a number, or None (regular).
#[derive(Debug, Clone, PartialEq)]
pub enum Weight {
    Name(String),
    Number(u32),
}
impl std::fmt::Display for Weight {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Weight::Name(s) => write!(f, "{s}"),
            Weight::Number(n) => write!(f, "{n}"),
        }
    }
}

fn weight_value(v: &str) -> Option<Weight> {
    let v = v.trim().to_lowercase();
    match v.as_str() {
        "bold" | "bolder" => Some(Weight::Name("bold".into())),
        "lighter" => Some(Weight::Name("light".into())),
        _ if v.len() == 3
            && v.ends_with("00")
            && ('1'..='9').contains(&v.chars().next().unwrap()) =>
        {
            (v != "400").then(|| Weight::Number(v.parse().unwrap()))
        }
        _ => None,
    }
}

fn slant_value(v: &str) -> Option<String> {
    let v = v.trim().to_lowercase();
    matches!(v.as_str(), "italic" | "oblique").then_some(v)
}

/// The size, fill, weight and slant a label is typeset with (bake.label_style): its own value,
/// else the first rule mentioning each of its classes in turn (math or prose last), no cascade.
pub fn label_style(
    css: &Sheet,
    classes: &[String],
    own: &dyn Fn(&str) -> Option<String>,
) -> (f64, String, Option<Weight>, Option<String>) {
    let mode = if classes.iter().any(|c| c == "math") {
        Some("math")
    } else if classes.iter().any(|c| c == "prose") {
        Some("prose")
    } else {
        None
    };
    let mut order: Vec<&str> = classes
        .iter()
        .map(String::as_str)
        .filter(|c| Some(*c) != mode)
        .collect();
    if let Some(m) = mode {
        order.push(m);
    }
    fn lookup<T>(
        css: &Sheet,
        order: &[&str],
        own: &dyn Fn(&str) -> Option<String>,
        prop: &str,
        convert: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        std::iter::once(own(prop))
            .chain(order.iter().map(|c| css.value(c, prop)))
            .find_map(|v| v.filter(|v| !v.is_empty()).and_then(|v| convert(&v)))
    }
    let size = lookup(css, &order, own, "font-size", |v| {
        font_size(v).filter(|s| *s != 0.)
    })
    .unwrap_or(10.);
    let fill = lookup(css, &order, own, "fill", colour_value).unwrap_or_else(|| "#000000".into());
    let weight = lookup(css, &order, own, "font-weight", weight_value);
    let slant = lookup(css, &order, own, "font-style", slant_value);
    (size, fill, weight, slant)
}
