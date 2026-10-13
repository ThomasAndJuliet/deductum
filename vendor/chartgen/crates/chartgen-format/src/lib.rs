//! Set-wise number, scale label, and date formatting from the original fmt.py.
use chartgen_model::{Decimal, Error, Result, Spec, TrainedScale};
use chrono::{Datelike, NaiveDate};
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locale {
    pub group: &'static str,
    pub decimal: &'static str,
    pub minus: &'static str,
}
impl Default for Locale {
    fn default() -> Self {
        Self {
            group: ",",
            decimal: ".",
            minus: "−",
        }
    }
}
pub fn locale(name: &str) -> Locale {
    match name {
        "de-DE" => Locale {
            group: ".",
            decimal: ",",
            minus: "−",
        },
        "fr-FR" => Locale {
            group: "\u{202f}",
            decimal: ",",
            minus: "−",
        },
        _ => Locale::default(),
    }
}
fn err(s: &str) -> Error {
    Error::new("format", s)
}
fn dec(v: &Value) -> Result<Option<Decimal>> {
    if v.is_null() {
        Ok(None)
    } else {
        let raw = v.to_string();
        Decimal::from_str(&raw)
            .or_else(|_| Decimal::from_scientific(&raw))
            .map(Some)
            .map_err(|_| err("invalid decimal"))
    }
}
fn pow10(k: i32) -> Decimal {
    if k < 0 {
        Decimal::from_i128_with_scale(1, (-k) as u32)
    } else {
        let mut n = Decimal::ONE;
        for _ in 0..k {
            n *= Decimal::from(10)
        }
        n
    }
}
fn shift(v: Decimal, e: i32) -> Decimal {
    if e >= 0 { v * pow10(e) } else { v / pow10(-e) }
}
pub fn decimals(v: Decimal) -> u32 {
    if v.is_zero() {
        0
    } else {
        v.normalize().scale()
    }
}
pub fn round_half_away(v: Decimal, d: u32) -> Decimal {
    v.round_dp_with_strategy(d, RoundingStrategy::MidpointAwayFromZero)
}
fn fixed(v: Decimal, d: u32) -> String {
    let mut x = round_half_away(v, d);
    x.rescale(d);
    x.to_string()
}
fn integer_digits(v: Decimal) -> usize {
    let s = v.abs().trunc().to_string();
    s.len()
}
pub fn number(v: Decimal, d: u32, loc: Locale, prefix: &str, plus: bool) -> String {
    let rounded = round_half_away(v, d);
    let negative = rounded < Decimal::ZERO;
    let text = fixed(rounded.abs(), d);
    let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push_str(loc.group)
        }
        grouped.push(c)
    }
    let sign = if negative {
        loc.minus
    } else if plus && rounded > Decimal::ZERO {
        "+"
    } else {
        ""
    };
    format!(
        "{sign}{prefix}{grouped}{}",
        if d > 0 {
            format!("{}{}", loc.decimal, frac)
        } else {
            String::new()
        }
    )
}
pub fn censored_text(status: &str, limit: Decimal, d: u32, loc: Locale, prefix: &str) -> String {
    let sign = if status == "below" { "<" } else { ">" };
    format!(
        "{sign}{}",
        number(limit, d.max(decimals(limit)), loc, prefix, false)
    )
}

#[derive(Debug, Clone, Default)]
pub struct Unit {
    pub prefix: String,
    pub suffix: String,
    pub noun: String,
    pub what: String,
    pub magnitude: i32,
    pub fallback: String,
    pub scale_title: String,
}
fn text(v: &Value) -> String {
    v.as_str()
        .or_else(|| {
            ["alternative", "typst", "tex", "latex"]
                .iter()
                .find_map(|k| v[*k].as_str())
        })
        .unwrap_or("")
        .into()
}
impl Unit {
    pub fn from_spec(spec: &Spec) -> Self {
        Self::from_value(spec.get("/data/measure/unit"))
    }
    pub fn from_value(u: &Value) -> Self {
        let given = match u["given_in"].as_str().unwrap_or("") {
            "thousands" => 3,
            "millions" => 6,
            "billions" => 9,
            "trillions" => 12,
            _ => 0,
        };
        Self {
            prefix: u["prefix"].as_str().unwrap_or("").into(),
            suffix: u["suffix"].as_str().unwrap_or("").into(),
            noun: u["noun"].as_str().unwrap_or("").into(),
            what: u["what"].as_str().unwrap_or("").into(),
            magnitude: u["magnitude"]
                .as_i64()
                .map(|x| x as i32)
                .filter(|x| *x != 0)
                .unwrap_or(given),
            fallback: text(&u["fallback"]),
            scale_title: text(&u["scale_title"]),
        }
    }
}
fn currency(u: &Unit) -> Option<String> {
    const SIGNS: &str = "$£€¥₹₩₽₺₪₫฿₦₱₴₸₡₲₵¢";
    const WORDS: &[&str] = &[
        "USD", "GBP", "EUR", "CHF", "CAD", "AUD", "NZD", "SEK", "NOK", "DKK", "PLN", "CZK", "HUF",
        "ZAR", "SGD", "HKD", "CNY", "INR", "BRL", "MXN", "JPY", "KRW", "ISK", "KR", "ZŁ", "KČ",
        "FR.",
    ];
    for part in [&u.prefix, &u.suffix] {
        if let Some(c) = part.chars().find(|c| SIGNS.contains(*c)) {
            return Some(c.to_string());
        }
        let upper = part.trim().to_uppercase();
        if WORDS.contains(&upper.as_str()) {
            return Some(upper);
        }
    }
    None
}
fn money_decimals(d: u32, u: &Unit, e: i32) -> u32 {
    let c = currency(u);
    if e != 0
        || d == 0
        || d >= 2
        || c.as_deref()
            .is_none_or(|x| ["¥", "₩", "JPY", "KRW", "ISK"].contains(&x))
    {
        d
    } else {
        2
    }
}
/// The decimals of a set of numbers read together (fmt.set_decimals, G6.16): the precision's,
/// else the most any value needs, then money's (two decimals for amounts of money in their own
/// unit that need any).
pub fn set_decimals(values: &[Decimal], precision: Option<Decimal>, unit: &Unit, e: i32) -> u32 {
    let d = match precision {
        Some(p) => decimals(p),
        None => values.iter().map(|v| decimals(*v)).max().unwrap_or(0),
    };
    money_decimals(d, unit, e)
}
pub fn choose_magnitude(values: &[Decimal], step: Option<Decimal>, setting: Option<i32>) -> i32 {
    if let Some(e) = setting {
        return e;
    }
    let top = values
        .iter()
        .map(|v| v.abs())
        .max()
        .unwrap_or(Decimal::ZERO);
    if top.is_zero() || integer_digits(top) < 5 {
        return 0;
    }
    let mut e = ((integer_digits(top) - 1) / 3 * 3).min(12) as i32;
    while e > 0 && step.is_some_and(|s| decimals(s / pow10(e)) > 1) {
        e -= 3
    }
    e
}
pub fn magnitude_word(e: i32, abbr: &Value, style: &str) -> (String, bool) {
    if e == 0 {
        return (String::new(), false);
    }
    let default = match e {
        3 => "K",
        6 => "million",
        9 => "billion",
        12 => "trillion",
        _ => "",
    };
    let short = match e {
        3 => "K",
        6 => "M",
        9 => "B",
        12 => "T",
        _ => "",
    };
    let key = match e {
        3 => "thousand",
        6 => "million",
        9 => "billion",
        12 => "trillion",
        _ => "",
    };
    let custom = abbr
        .get(e.to_string())
        .or_else(|| abbr.get(key))
        .and_then(Value::as_str);
    let word = if style == "suffix" {
        custom.unwrap_or(short)
    } else {
        custom.unwrap_or(default)
    }
    .to_owned();
    let attached = style == "suffix"
        || !word.chars().next().is_some_and(char::is_alphabetic)
        || word.len() <= 2;
    (word, attached)
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopLabel {
    pub number: String,
    pub attached: String,
    pub phrase: String,
    pub kept: String,
}
impl TopLabel {
    pub fn inline(&self) -> String {
        format!(
            "{}{}{}",
            self.number,
            self.attached,
            if self.phrase.is_empty() {
                String::new()
            } else {
                format!(" {}", self.phrase)
            }
        )
    }
    pub fn short(&self) -> String {
        format!("{}{}", self.number, self.attached)
    }
    pub fn kept_text(&self) -> String {
        format!(
            "{}{}",
            self.attached,
            if self.kept.is_empty() {
                String::new()
            } else {
                format!(" {}", self.kept)
            }
        )
        .trim()
        .into()
    }
    pub fn fallback(&self) -> String {
        let k = self.kept_text();
        format!(
            "{}{}",
            self.number,
            if k.is_empty() {
                String::new()
            } else if self.attached.is_empty() {
                format!(" {k}")
            } else {
                k
            }
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScaleLabels {
    pub ticks: Vec<Decimal>,
    pub labels: Vec<String>,
    pub top: TopLabel,
    pub magnitude: i32,
    pub decimals: u32,
    pub style: String,
    pub unit_line: String,
    pub bare: Vec<String>,
    pub title: String,
    pub notes: Vec<String>,
}
impl ScaleLabels {
    pub fn top_index(&self) -> usize {
        self.labels.len().saturating_sub(1)
    }
}
fn unit_words(u: &Unit, word: &str) -> (String, String) {
    let mut kept = Vec::new();
    if !word.is_empty() {
        kept.push(word.to_owned())
    }
    if u.suffix.starts_with(' ') && !u.suffix.trim().is_empty() {
        kept.push(u.suffix.trim().into())
    }
    let mut described = Vec::new();
    if !u.noun.is_empty() {
        described.push(u.noun.clone())
    }
    if !u.what.is_empty() {
        described.push(format!("({})", u.what))
    }
    (kept.join(" "), described.join(" "))
}
fn unit_phrase(u: &Unit, word: &str) -> String {
    let (k, d) = unit_words(u, word);
    [k, d]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn scale_title(name: &str, u: &Unit, e: i32, kind: &str, abbr: &Value) -> String {
    if !u.scale_title.is_empty() {
        return u.scale_title.clone();
    }
    let quantity = if !u.what.is_empty() { &u.what } else { name };
    let quantity = format!(
        "{}{}",
        quantity.chars().next().unwrap_or(' ').to_uppercase(),
        quantity.chars().skip(1).collect::<String>()
    )
    .trim()
    .to_owned();
    let suffix = if u.suffix.is_empty() && kind == "percent" {
        "%"
    } else {
        u.suffix.trim()
    };
    let mut parts = Vec::new();
    if !u.prefix.is_empty() {
        let (w, attached) = magnitude_word(e, abbr, "word_on_top");
        parts.push(format!(
            "{}{}",
            u.prefix,
            if attached {
                w
            } else if w.is_empty() {
                String::new()
            } else {
                format!(" {w}")
            }
        ))
    } else if e > 0 {
        let base = match e {
            3 => "thousand",
            6 => "million",
            9 => "billion",
            12 => "trillion",
            _ => "",
        };
        parts.push(format!(
            "{}{}",
            base,
            if !u.noun.is_empty() && suffix.is_empty() {
                ""
            } else {
                "s"
            }
        ))
    }
    if !suffix.is_empty() && suffix != "%" {
        parts.push(suffix.into())
    }
    if !u.noun.is_empty() && u.prefix.is_empty() && suffix.is_empty() {
        parts.push(u.noun.clone())
    }
    let inside = parts.join(" ");
    if !inside.is_empty() && !quantity.is_empty() {
        format!("{quantity} ({inside})")
    } else if !quantity.is_empty() {
        quantity
    } else {
        inside
    }
}

pub fn format_scale(
    ticks: &[Decimal],
    step: Decimal,
    kind: &str,
    unit: &Unit,
    magnitude: Option<i32>,
    style: &str,
    abbreviations: &Value,
    loc: Locale,
    name: &str,
) -> Result<ScaleLabels> {
    if ticks.is_empty() {
        return Err(err("scale has no ticks"));
    }
    if step <= Decimal::ZERO {
        return Err(err("step must be positive"));
    }
    let actual: Vec<_> = ticks.iter().map(|t| shift(*t, unit.magnitude)).collect();
    let step_actual = shift(step, unit.magnitude);
    let e = choose_magnitude(&actual, Some(step_actual), magnitude);
    let style = if style == "auto" {
        "word_on_top"
    } else {
        style
    };
    if !["word_on_top", "suffix"].contains(&style) {
        return Err(err("unknown magnitude style"));
    }
    let d = money_decimals(decimals(step_actual / pow10(e)), unit, e);
    let suffix = if kind == "percent" && unit.suffix.is_empty() {
        "%"
    } else {
        &unit.suffix
    };
    let plus = kind == "change" || kind == "index";
    let (w, attached) = magnitude_word(e, abbreviations, style);
    let pct = if suffix == "%" { "%" } else { "" };
    let tight = if suffix != "%" && !suffix.starts_with(' ') {
        suffix
    } else {
        ""
    };
    let mut labels = Vec::new();
    for t in &actual {
        let scaled = *t / pow10(e);
        labels.push(if style == "suffix" {
            format!(
                "{}{}{}",
                number(scaled, d, loc, &unit.prefix, plus),
                if attached { &w } else { "" },
                suffix
            )
        } else {
            format!("{}{}", number(scaled, d, loc, "", plus), pct)
        })
    }
    let (kept, desc) = unit_words(unit, if attached { "" } else { &w });
    let top = TopLabel {
        number: format!(
            "{}{}",
            number(
                *actual.last().unwrap() / pow10(e),
                d,
                loc,
                &unit.prefix,
                plus
            ),
            pct
        ),
        attached: format!("{}{}", if attached { &w } else { "" }, tight),
        phrase: unit_phrase(unit, if attached { "" } else { &w }),
        kept,
    };
    *labels.last_mut().unwrap() = top.short();
    let unit_line = if !unit.fallback.is_empty() {
        unit.fallback.clone()
    } else {
        desc.strip_prefix('(')
            .and_then(|x| x.strip_suffix(')'))
            .filter(|_| desc.matches('(').count() == 1)
            .unwrap_or(&desc)
            .to_owned()
    };
    let bare = actual
        .iter()
        .map(|t| format!("{}{}", number(*t / pow10(e), d, loc, "", plus), pct))
        .collect();
    let title = scale_title(name, unit, e, kind, abbreviations);
    Ok(ScaleLabels {
        ticks: ticks.to_vec(),
        labels,
        top,
        magnitude: e,
        decimals: d,
        style: style.into(),
        unit_line,
        bare,
        title,
        notes: vec![format!(
            "labels formatted as a set: magnitude 10^{e}, {d} decimals, style {style}"
        )],
    })
}
pub fn scale_labels(spec: &Spec, scale: &TrainedScale) -> Result<ScaleLabels> {
    scale_labels_with_measure(spec, spec.get("/data/measure"), scale)
}
pub fn scale_labels_with_measure(
    spec: &Spec,
    measure: &Value,
    scale: &TrainedScale,
) -> Result<ScaleLabels> {
    let unit = Unit::from_value(&measure["unit"]);
    let kind = measure["kind"].as_str().unwrap_or("count");
    let magnitude = match spec.get("/scale/magnitude") {
        Value::String(x) if x == "none" => Some(0),
        Value::Number(_) => {
            dec(spec.get("/scale/magnitude"))?.and_then(|v| v.to_string().parse().ok())
        }
        _ => None,
    };
    format_scale(
        &scale.ticks,
        scale.step,
        kind,
        &unit,
        magnitude,
        &spec.string("/scale/magnitude_style", "auto"),
        spec.get("/audience/abbreviations"),
        locale(&spec.string("/audience/locale", "en-GB")),
        measure["name"].as_str().unwrap_or(""),
    )
}
pub fn format_values(spec: &Spec, values: &[Option<Decimal>]) -> Result<Vec<Option<String>>> {
    format_values_with_measure(spec, spec.get("/data/measure"), values)
}
pub fn format_values_with_measure(
    spec: &Spec,
    measure: &Value,
    values: &[Option<Decimal>],
) -> Result<Vec<Option<String>>> {
    let unit = Unit::from_value(&measure["unit"]);
    let kind = measure["kind"].as_str().unwrap_or("count");
    let loc = locale(&spec.string("/audience/locale", "en-GB"));
    let abbr = spec.get("/audience/abbreviations");
    let actual: Vec<_> = values
        .iter()
        .map(|v| v.map(|x| shift(x, unit.magnitude)))
        .collect();
    let known: Vec<_> = actual.iter().flatten().copied().collect();
    let mut e = choose_magnitude(&known, None, None);
    let prefix = &unit.prefix;
    let suffix = if unit.suffix.is_empty() && kind == "percent" {
        "%"
    } else {
        &unit.suffix
    };
    let plus = kind == "change" || kind == "index";
    let precision = dec(&measure["precision"])?;
    let d = loop {
        let scaled: Vec<_> = known.iter().map(|v| *v / pow10(e)).collect();
        let limit = precision.map_or(6, |p| 6.min(decimals(shift(p, unit.magnitude) / pow10(e))));
        let distinct = scaled
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let mut d = 0;
        while d < limit
            && scaled
                .iter()
                .map(|v| round_half_away(*v, d))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                < distinct
        {
            d += 1
        }
        if d <= 1 || e == 0 {
            break money_decimals(d, &unit, e);
        }
        e -= 3
    };
    let (w, attached) = magnitude_word(e, abbr, "word_on_top");
    let after = format!(
        "{}{}",
        if attached {
            w
        } else if w.is_empty() {
            String::new()
        } else {
            format!(" {w}")
        },
        suffix
    );
    Ok(actual
        .iter()
        .map(|v| v.map(|x| format!("{}{}", number(x / pow10(e), d, loc, prefix, plus), after)))
        .collect())
}
const MONTHS_INLINE: [&str; 12] = [
    "Jan.", "Feb.", "Mar.", "Apr.", "May", "June", "July", "Aug.", "Sept.", "Oct.", "Nov.", "Dec.",
];
const MONTHS_COLUMN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub fn date_inline(day: NaiveDate, audience_format: Option<&str>) -> String {
    let month = MONTHS_INLINE[(day.month() - 1) as usize];
    match audience_format {
        Some("D Mon. YYYY") => format!("{} {month} {}", day.day(), day.year()),
        Some("D-Mon-YYYY") => format!(
            "{}-{}-{}",
            day.day(),
            MONTHS_COLUMN[(day.month() - 1) as usize],
            day.year()
        ),
        _ => format!("{month} {}, {}", day.day(), day.year()),
    }
}
pub fn date_column(day: NaiveDate) -> String {
    format!(
        "{:02}-{}-{:04}",
        day.day(),
        MONTHS_COLUMN[(day.month() - 1) as usize],
        day.year()
    )
}
pub fn month_year(year: i32, month: u32) -> Result<String> {
    let m = MONTHS_INLINE
        .get(month.checked_sub(1).ok_or_else(|| err("invalid month"))? as usize)
        .ok_or_else(|| err("invalid month"))?;
    Ok(format!("{m} {year}"))
}
pub fn parse_date(text: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| err("invalid ISO date"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    fn ticks(lo: &str, hi: &str, step: &str) -> Vec<Decimal> {
        let (mut x, end, s) = (d(lo), d(hi), d(step));
        let mut out = Vec::new();
        while x <= end {
            out.push(x);
            x += s
        }
        out
    }
    fn labels(lo: &str, hi: &str, step: &str, kind: &str, unit: Unit) -> ScaleLabels {
        format_scale(
            &ticks(lo, hi, step),
            d(step),
            kind,
            &unit,
            None,
            "auto",
            &Value::Null,
            Locale::default(),
            "",
        )
        .unwrap()
    }
    #[test]
    fn numbers() {
        assert_eq!(round_half_away(d("2.5"), 0), d("3"));
        assert_eq!(round_half_away(d("-2.5"), 0), d("-3"));
        assert_eq!(
            number(d("5477210"), 0, Locale::default(), "", false),
            "5,477,210"
        );
        assert_eq!(number(d("-20"), 0, Locale::default(), "$", false), "−$20");
        assert_eq!(
            number(d("1234.5"), 1, locale("de-DE"), "", false),
            "1.234,5"
        );
    }
    #[test]
    fn source_scale_cases() {
        let x = labels(
            "0",
            "0.25",
            "0.05",
            "quantity",
            Unit {
                suffix: " lbs.".into(),
                what: "weight of product".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            x.labels,
            vec!["0.00", "0.05", "0.10", "0.15", "0.20", "0.25"]
        );
        assert_eq!(x.top.inline(), "0.25 lbs. (weight of product)");
        let x = labels(
            "0",
            "25000000",
            "5000000",
            "count",
            Unit {
                noun: "inhabitants".into(),
                ..Default::default()
            },
        );
        assert_eq!(x.labels[..5], ["0", "5", "10", "15", "20"]);
        assert_eq!(x.top.inline(), "25 million inhabitants");
        let x = labels(
            "0",
            "300",
            "50",
            "amount",
            Unit {
                prefix: "$".into(),
                noun: "in donations".into(),
                magnitude: 3,
                ..Default::default()
            },
        );
        assert_eq!(x.top.inline(), "$300K in donations");
        let x = labels(
            "-20",
            "20",
            "10",
            "change",
            Unit {
                suffix: "%".into(),
                ..Default::default()
            },
        );
        assert_eq!(x.labels, vec!["−20%", "−10%", "0%", "+10%", "+20%"]);
    }
    #[test]
    fn source_titles() {
        let cases = [
            (
                "300",
                "50",
                "amount",
                Unit {
                    prefix: "$".into(),
                    noun: "in donations".into(),
                    magnitude: 3,
                    ..Default::default()
                },
                "donations",
                "Donations ($K)",
            ),
            (
                "20",
                "5",
                "amount",
                Unit {
                    prefix: "$".into(),
                    magnitude: 6,
                    ..Default::default()
                },
                "funds raised",
                "Funds raised ($ million)",
            ),
            (
                "10",
                "2",
                "score",
                Unit {
                    noun: "on 10".into(),
                    ..Default::default()
                },
                "weirdness score",
                "Weirdness score (on 10)",
            ),
            (
                "0.25",
                "0.05",
                "quantity",
                Unit {
                    suffix: " lbs.".into(),
                    what: "weight of product".into(),
                    ..Default::default()
                },
                "weight",
                "Weight of product (lbs.)",
            ),
        ];
        for (hi, step, kind, unit, name, want) in cases {
            let got = format_scale(
                &ticks("0", hi, step),
                d(step),
                kind,
                &unit,
                None,
                "auto",
                &Value::Null,
                Locale::default(),
                name,
            )
            .unwrap();
            assert_eq!(got.title, want)
        }
    }
    #[test]
    fn data_values() {
        let spec = Spec {
            raw: json!({"data":{"measure":{"kind":"amount","unit":{"prefix":"$"}}}}),
        };
        let vals = [
            "728491.65",
            "582793.32",
            "466234.66",
            "326364.26",
            "195818.56",
        ]
        .map(|x| Some(d(x)));
        let got = format_values(&spec, &vals).unwrap();
        assert_eq!(
            got,
            vec!["$728K", "$583K", "$466K", "$326K", "$196K"]
                .into_iter()
                .map(|x| Some(x.into()))
                .collect::<Vec<_>>()
        );
        let spec = Spec {
            raw: json!({"data":{"measure":{"kind":"score"}}}),
        };
        assert_eq!(
            format_values(&spec, &[Some(d("8.8")), Some(d("8.84"))]).unwrap(),
            vec![Some("8.80".into()), Some("8.84".into())]
        );
    }
    #[test]
    fn percent_suffix_and_currency_cases() {
        let x = labels(
            "0",
            "100",
            "20",
            "percent",
            Unit {
                noun: "of employees working remotely".into(),
                ..Default::default()
            },
        );
        assert_eq!(x.labels, vec!["0%", "20%", "40%", "60%", "80%", "100%"]);
        assert_eq!(x.top.inline(), "100% of employees working remotely");
        let x = format_scale(
            &ticks("0", "600000", "100000"),
            d("100000"),
            "amount",
            &Unit {
                prefix: "$".into(),
                ..Default::default()
            },
            None,
            "suffix",
            &Value::Null,
            Locale::default(),
            "",
        )
        .unwrap();
        assert_eq!(
            x.labels,
            vec!["$0K", "$100K", "$200K", "$300K", "$400K", "$500K", "$600K"]
        );
        let x = labels(
            "0",
            "500",
            "100",
            "amount",
            Unit {
                prefix: "£".into(),
                ..Default::default()
            },
        );
        assert_eq!(x.decimals, 0);
        let x = labels(
            "0",
            "0.5",
            "0.1",
            "amount",
            Unit {
                prefix: "£".into(),
                ..Default::default()
            },
        );
        assert_eq!(x.decimals, 2);
    }
    #[test]
    fn date_forms() {
        let day = NaiveDate::from_ymd_opt(2007, 2, 10).unwrap();
        assert_eq!(date_inline(day, None), "Feb. 10, 2007");
        assert_eq!(date_column(day), "10-Feb-2007");
        assert_eq!(month_year(2022, 7).unwrap(), "July 2022");
        assert_eq!(month_year(2022, 10).unwrap(), "Oct. 2022");
    }
    #[test]
    fn derived_measure_uses_prepared_unit() {
        let spec = Spec {
            raw: json!({"chartspec":1,"id":"ratio","data":{"x":{"kind":"nominal","values":["A"]},"measure":{"name":"accidents","kind":"count"},"series":[{"id":"s","values":[20]}]},"transform":{"kind":"ratio","exposure":{"values":[4],"noun":"work hours"},"per":100000}}),
        };
        let data = chartgen_data::transform(&spec, &chartgen_data::parse(&spec).unwrap()).unwrap();
        let values: Vec<_> = data.series[0].values.iter().map(|d| d.value).collect();
        let labels = format_values_with_measure(&spec, &data.measure, &values).unwrap();
        assert_eq!(labels, vec![Some("500K".into())]);
        assert_eq!(
            data.measure["unit"]["noun"],
            "accidents per 100K work hours"
        );
    }
}
