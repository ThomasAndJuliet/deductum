// SPDX-License-Identifier: GPL-3.0-only

//! Isabelle symbols: theory sources write `\<forall>`, both views show `∀`
//! (design C14). The table comes from the runtime's `etc/symbols`, through the
//! session host. Decoding keeps a map between the two texts' offsets, so an
//! edit made in the decoded text lands on the right source characters and
//! leaves every other escape as it was.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    pub symbol: String,
    #[serde(default)]
    pub code: u32,
    #[serde(default)]
    pub abbrevs: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
}

/// LaTeX names for Isabelle symbols, for completing `\leq` as `≤` (by symbol name)
pub const LATEX: &[(&str, &[&str])] = &[
    ("le", &["leq", "le"]), ("ge", &["geq", "ge"]), ("noteq", &["neq", "ne"]),
    ("longrightarrow", &["to", "rightarrow", "implies", "longrightarrow"]),
    ("Longrightarrow", &["Longrightarrow", "Rightarrow", "implies"]), ("rightarrow", &["rightarrow", "to"]),
    ("Rightarrow", &["Rightarrow"]), ("longleftrightarrow", &["iff", "leftrightarrow", "longleftrightarrow"]),
    ("and", &["land", "wedge"]), ("or", &["lor", "vee"]), ("not", &["neg", "lnot"]),
    ("forall", &["forall"]), ("exists", &["exists"]), ("nexists", &["nexists"]),
    ("in", &["in"]), ("notin", &["notin"]), ("subseteq", &["subseteq"]), ("subset", &["subset"]),
    ("supseteq", &["supseteq"]), ("union", &["cup"]), ("inter", &["cap"]), ("Union", &["bigcup"]),
    ("Inter", &["bigcap"]), ("emptyset", &["emptyset", "varnothing"]), ("lambda", &["lambda"]),
    ("times", &["times"]), ("circ", &["circ", "compose"]), ("equiv", &["equiv"]), ("bottom", &["bot"]),
    ("top", &["top"]), ("langle", &["langle"]), ("rangle", &["rangle"]), ("lbrakk", &["llbracket"]),
    ("rbrakk", &["rrbracket"]), ("mapsto", &["mapsto"]), ("turnstile", &["vdash"]), ("Turnstile", &["models"]),
    ("sqsubseteq", &["sqsubseteq"]), ("sqsubset", &["sqsubset"]), ("squnion", &["sqcup"]), ("sqinter", &["sqcap"]),
    ("Squnion", &["bigsqcup"]), ("Sqinter", &["bigsqcap"]), ("oplus", &["oplus"]), ("otimes", &["otimes"]),
    ("cdot", &["cdot"]), ("bullet", &["bullet"]), ("star", &["star"]), ("infinity", &["infty"]),
    ("Sum", &["sum"]), ("Prod", &["prod"]), ("partial", &["partial"]), ("nabla", &["nabla"]),
    ("nat", &["mathbb", "naturals", "N"]), ("int", &["integers", "Z"]), ("real", &["reals", "R"]),
    ("rat", &["rationals", "Q"]), ("complex", &["C"]), ("open", &["lcartouche"]), ("close", &["rcartouche"]),
    ("And", &["bigwedge"]), ("Or", &["bigvee"]), ("box", &["box", "square"]), ("diamond", &["diamond"]),
    ("leadsto", &["leadsto"]), ("approx", &["approx"]), ("sim", &["sim"]), ("simeq", &["simeq"]), ("cong", &["cong"]),
    ("prec", &["prec"]), ("succ", &["succ"]), ("preceq", &["preceq"]), ("succeq", &["succeq"]), ("parallel", &["parallel"]),
    ("Colon", &["Colon"]), ("dagger", &["dagger"]), ("ddagger", &["ddagger"]), ("ominus", &["ominus"]),
    ("lfloor", &["lfloor"]), ("rfloor", &["rfloor"]), ("lceil", &["lceil"]), ("rceil", &["rceil"]),
];

/// Unicode subscripts and superscripts, for `\<^sub>` and `\<^sup>` as jEdit sets them
const SUBSCRIPTS: &[(char, char)] = &[
    ('0', '₀'), ('1', '₁'), ('2', '₂'), ('3', '₃'), ('4', '₄'), ('5', '₅'), ('6', '₆'), ('7', '₇'), ('8', '₈'),
    ('9', '₉'), ('+', '₊'), ('-', '₋'), ('=', '₌'), ('(', '₍'), (')', '₎'), ('a', 'ₐ'), ('e', 'ₑ'), ('h', 'ₕ'),
    ('i', 'ᵢ'), ('j', 'ⱼ'), ('k', 'ₖ'), ('l', 'ₗ'), ('m', 'ₘ'), ('n', 'ₙ'), ('o', 'ₒ'), ('p', 'ₚ'), ('r', 'ᵣ'),
    ('s', 'ₛ'), ('t', 'ₜ'), ('u', 'ᵤ'), ('v', 'ᵥ'), ('x', 'ₓ'),
];
const SUPERSCRIPTS: &[(char, char)] = &[
    ('0', '⁰'), ('1', '¹'), ('2', '²'), ('3', '³'), ('4', '⁴'), ('5', '⁵'), ('6', '⁶'), ('7', '⁷'), ('8', '⁸'),
    ('9', '⁹'), ('+', '⁺'), ('-', '⁻'), ('=', '⁼'), ('(', '⁽'), (')', '⁾'), ('a', 'ᵃ'), ('b', 'ᵇ'), ('c', 'ᶜ'),
    ('d', 'ᵈ'), ('e', 'ᵉ'), ('f', 'ᶠ'), ('g', 'ᵍ'), ('h', 'ʰ'), ('i', 'ⁱ'), ('j', 'ʲ'), ('k', 'ᵏ'), ('l', 'ˡ'),
    ('m', 'ᵐ'), ('n', 'ⁿ'), ('o', 'ᵒ'), ('p', 'ᵖ'), ('r', 'ʳ'), ('s', 'ˢ'), ('t', 'ᵗ'), ('u', 'ᵘ'), ('v', 'ᵛ'),
    ('w', 'ʷ'), ('x', 'ˣ'), ('y', 'ʸ'), ('z', 'ᶻ'), ('A', 'ᴬ'), ('B', 'ᴮ'), ('D', 'ᴰ'), ('E', 'ᴱ'), ('G', 'ᴳ'),
    ('H', 'ᴴ'), ('I', 'ᴵ'), ('J', 'ᴶ'), ('K', 'ᴷ'), ('L', 'ᴸ'), ('M', 'ᴹ'), ('N', 'ᴺ'), ('O', 'ᴼ'), ('P', 'ᴾ'),
    ('R', 'ᴿ'), ('T', 'ᵀ'), ('U', 'ᵁ'), ('V', 'ⱽ'), ('W', 'ᵂ'),
];

/// Forms shown but not typed: typing `*` must stay a plain `*`
const SUB_SHOWN: &[(char, char)] = &[
    ('*', '⁎'), ('<', '˱'), ('>', '˲'), ('β', 'ᵦ'), ('γ', 'ᵧ'), ('ρ', 'ᵨ'), ('φ', 'ᵩ'), ('χ', 'ᵪ'), ('\'', '′'),
];
const SUP_SHOWN: &[(char, char)] = &[
    ('*', '*'), ('<', '˂'), ('>', '˃'), ('~', '˜'), ('!', 'ꜝ'), ('.', '·'), ('/', 'ᐟ'), ('#', '#'), ('α', 'ᵅ'), ('β', 'ᵝ'), ('γ', 'ᵞ'), ('δ', 'ᵟ'), ('ε', 'ᵋ'), ('θ', 'ᶿ'), ('ι', 'ᶥ'),
    ('φ', 'ᵠ'), ('χ', 'ᵡ'), ('\'', '′'), ('⇩', '⇩'), ('∗', '*'), ('⊤', 'ᵀ'), ('†', '†'), ('⊥', 'ᗮ'),
];

/// Isabellum Scripts (`res/deductum/fonts`) has a raised and a lowered small glyph
/// for each printable ASCII and Greek character, in the private use area
pub fn script_glyph(superscript: bool, c: char) -> Option<char> {
    let c = c as u32;
    let code = match c {
        0x21..=0x7E => (if superscript { 0xE000 } else { 0xE100 }) + c,
        0x391..=0x3C9 => (if superscript { 0xE200 } else { 0xE300 }) + (c - 0x380),
        _ => return None,
    };
    char::from_u32(code)
}

fn in_scripts_font(c: char) -> bool {
    matches!(c as u32, 0x21..=0x7E | 0x391..=0x3C9)
}

/// How `c` shows as a superscript in running text, which can't change fonts per
/// character: a Unicode form where there is one
pub fn unicode_script(superscript: bool, c: char) -> Option<char> {
    let (table, shown) = if superscript { (SUPERSCRIPTS, SUP_SHOWN) } else { (SUBSCRIPTS, SUB_SHOWN) };
    table.iter().chain(shown.iter()).find(|(b, _)| *b == c).map(|(_, s)| *s)
}

/// How `c` shows as a superscript (or subscript): the scripts font's glyph, else a
/// Unicode form where there is one
pub fn script(superscript: bool, c: char) -> Option<char> {
    if in_scripts_font(c) {
        return script_glyph(superscript, c);
    }
    let (table, shown) = if superscript { (SUPERSCRIPTS, SUP_SHOWN) } else { (SUBSCRIPTS, SUB_SHOWN) };
    table.iter().chain(shown.iter()).find(|(b, _)| *b == c).map(|(_, s)| *s)
}

/// How a decoded code point is set, beyond its character
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    /// A glyph of the scripts font, which must be drawn in it
    Script,
    /// A control symbol shown as a marker (⇩, ⇧, ❙ …) because the next character has no
    /// subscript or superscript form
    Control,
}

/// How well `query` matches `name`: prefix, then word starts, then a subsequence
pub fn fuzzy(query: &str, name: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase();
    let n = name.to_lowercase();
    if n == q {
        return Some(10_000);
    }
    if name.starts_with(query) {
        return Some(5000 - name.len() as i32);
    }
    if n.starts_with(&q) {
        return Some(4500 - name.len() as i32);
    }
    if let Some(i) = n.find(&q) {
        return Some(3000 - i as i32 * 10 - name.len() as i32);
    }
    // a subsequence, scored by its gaps
    let mut gaps = 0;
    let mut chars = n.chars();
    let mut last = 0;
    for (k, qc) in q.chars().enumerate() {
        let mut pos = last;
        loop {
            match chars.next() {
                Some(c) if c == qc => break,
                Some(_) => pos += 1,
                None => return None,
            }
        }
        if k > 0 {
            gaps += pos - last;
        }
        last = pos + 1;
    }
    Some(1000 - gaps as i32 * 20 - name.len() as i32)
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub entries: Vec<Entry>,
    decode: HashMap<String, char>,
    encode: HashMap<char, String>,
}

impl Table {
    pub fn new(entries: Vec<Entry>) -> Self {
        let mut decode = HashMap::new();
        let mut encode = HashMap::new();
        for e in &entries {
            if let Some(c) = char::from_u32(e.code).filter(|_| e.code > 0x7f) {
                decode.insert(e.symbol.clone(), c);
                encode.entry(c).or_insert_with(|| e.symbol.clone());
            }
        }
        for (base, sub) in SUBSCRIPTS {
            encode.insert(*sub, format!("\\<^sub>{base}"));
        }
        for (base, sup) in SUPERSCRIPTS {
            encode.insert(*sup, format!("\\<^sup>{base}"));
        }
        // The scripts font's glyphs, back to their escapes
        for c in (0x21u32..=0x7E).chain(0x391..=0x3C9) {
            let base = char::from_u32(c).unwrap();
            let sym = |ch: char| self_symbol(&decode, ch);
            if let Some(g) = script_glyph(true, base) {
                encode.insert(g, format!("\\<^sup>{}", sym(base)));
            }
            if let Some(g) = script_glyph(false, base) {
                encode.insert(g, format!("\\<^sub>{}", sym(base)));
            }
        }
        Self { entries, decode, encode }
    }

    /// A table good enough until the runtime's arrives
    pub fn fallback() -> Self {
        let pairs: &[(&str, u32)] = &[
            ("forall", 0x2200), ("exists", 0x2203), ("not", 0x00ac), ("and", 0x2227), ("or", 0x2228),
            ("longrightarrow", 0x27f6), ("Longrightarrow", 0x27f9), ("rightarrow", 0x2192),
            ("Rightarrow", 0x21d2), ("leftrightarrow", 0x2194), ("longleftrightarrow", 0x27f7),
            ("equiv", 0x2261), ("noteq", 0x2260), ("le", 0x2264), ("ge", 0x2265), ("in", 0x2208),
            ("notin", 0x2209), ("subseteq", 0x2286), ("subset", 0x2282), ("union", 0x222a),
            ("inter", 0x2229), ("lambda", 0x03bb), ("alpha", 0x03b1), ("beta", 0x03b2),
            ("gamma", 0x03b3), ("delta", 0x03b4), ("sigma", 0x03c3), ("tau", 0x03c4), ("phi", 0x03c6),
            ("psi", 0x03c8), ("omega", 0x03c9), ("Sigma", 0x03a3), ("open", 0x2039), ("close", 0x203a),
            ("lbrakk", 0x27e6), ("rbrakk", 0x27e7), ("And", 0x22c0), ("times", 0x00d7),
            ("circ", 0x2218), ("bottom", 0x22a5), ("top", 0x22a4), ("comment", 0x2014),
            ("^sub", 0x21e9), ("^sup", 0x21e7), ("^bold", 0x2759), ("^emph", 0x2223),
            ("cdot", 0x22c5), ("sqsubseteq", 0x2291), ("Union", 0x22c3), ("Inter", 0x22c2),
            ("turnstile", 0x22a2), ("Turnstile", 0x22a8), ("langle", 0x27e8), ("rangle", 0x27e9),
            ("mapsto", 0x21a6), ("Colon", 0x2237), ("nat", 0x2115), ("int", 0x2124), ("real", 0x211d),
        ];
        Self::new(
            pairs
                .iter()
                .map(|(n, c)| Entry { symbol: format!("\\<{n}>"), code: *c, abbrevs: Vec::new(), groups: Vec::new() })
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.decode.is_empty()
    }

    /// Symbols for completion: (name without `\<…>`, character, abbreviations)
    pub fn completions(&self) -> impl Iterator<Item = (&str, char, &[String])> {
        self.entries.iter().filter_map(move |e| {
            let c = *self.decode.get(&e.symbol)?;
            let name = e.symbol.strip_prefix("\\<")?.strip_suffix('>')?;
            Some((name, c, e.abbrevs.as_slice()))
        })
    }

    /// Symbols matching a `\` query by name, LaTeX name or abbreviation, best first:
    /// (character, symbol, what matched)
    pub fn search(&self, query: &str, limit: usize) -> Vec<(char, String, String)> {
        let latex: HashMap<&str, &[&str]> = LATEX.iter().copied().collect();
        let mut scored: Vec<(i32, char, String, String)> = Vec::new();
        for (name, c, abbrevs) in self.completions() {
            let mut best: Option<(i32, String)> = fuzzy(query, name).map(|s| (s + 50, String::new()));
            for alias in latex.get(name).copied().unwrap_or(&[]) {
                if let Some(sc) = fuzzy(query, alias)
                    && best.as_ref().is_none_or(|b| sc > b.0)
                {
                    best = Some((sc, format!("\\{alias}")));
                }
            }
            for a in abbrevs {
                if a.chars().count() >= 2
                    && let Some(sc) = fuzzy(query, a).map(|s| s - 200)
                    && best.as_ref().is_none_or(|b| sc > b.0)
                {
                    best = Some((sc, a.clone()));
                }
            }
            if let Some((sc, how)) = best {
                scored.push((sc, c, format!("\\<{name}>"), how));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.len().cmp(&b.2.len())));
        scored.dedup_by(|a, b| a.1 == b.1);
        scored.into_iter().take(limit).map(|(_, c, sym, how)| (c, sym, how)).collect()
    }

    /// The symbols by group, as Isabelle/jEdit's Symbols panel has them
    pub fn groups(&self) -> Vec<(String, Vec<(char, String)>)> {
        let mut groups: Vec<(String, Vec<(char, String)>)> = Vec::new();
        for e in &self.entries {
            let Some(c) = self.decode.get(&e.symbol).copied() else { continue };
            let group = e.groups.first().cloned().unwrap_or_else(|| "other".into());
            if group == "control" || group == "icon" {
                continue;
            }
            match groups.iter_mut().find(|(g, _)| *g == group) {
                Some((_, v)) => v.push((c, e.symbol.clone())),
                None => groups.push((group, vec![(c, e.symbol.clone())])),
            }
        }
        groups
    }

    pub fn char_of(&self, symbol: &str) -> Option<char> {
        self.decode.get(symbol).copied()
    }

    /// The source for a character typed or pasted into a decoded view
    pub fn encode(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            match self.encode.get(&c) {
                Some(sym) => out.push_str(sym),
                None => out.push(c),
            }
        }
        out
    }

    /// `source` with symbols shown as Unicode, and the map between offsets.
    /// `\<^sub>c` and `\<^sup>c` become subscript and superscript characters where
    /// Unicode has them, and `\<^bold>c` a bold c, each standing for its whole escape
    pub fn decode(&self, source: &str) -> Decoded {
        let mut text = String::with_capacity(source.len());
        let mut src_of = Vec::with_capacity(source.len() + 1);
        let mut styles = Vec::new();
        let chars: Vec<char> = source.chars().collect();
        let mut i = 0;
        // The character at `j`, decoded, and where it ends
        let one = |j: usize| -> Option<(char, usize)> {
            if j >= chars.len() {
                return None;
            }
            if chars[j] == '\\' && chars.get(j + 1) == Some(&'<') && let Some(len) = symbol_len(&chars[j..]) {
                let sym: String = chars[j..j + len].iter().collect();
                return self.decode.get(&sym).map(|c| (*c, j + len));
            }
            Some((chars[j], j + 1))
        };
        // A symbol at `j` and its length: an escape, or the Unicode a prover's output already
        // carries for the script markers (⇩ ⇧ ⇘ ⇙ ⇗ ⇖)
        let marker = |j: usize| -> Option<(String, usize)> {
            let named = |n: &str| Some((format!("\\<^{n}>"), 1));
            match chars.get(j)? {
                '⇩' => named("sub"),
                '⇧' => named("sup"),
                '⇘' => named("bsub"),
                '⇙' => named("esub"),
                '⇗' => named("bsup"),
                '⇖' => named("esup"),
                '\\' if chars.get(j + 1) == Some(&'<') => symbol_len(&chars[j..]).map(|l| (chars[j..j + l].iter().collect(), l)),
                _ => None,
            }
        };
        while i < chars.len() {
            if let Some((sym, len)) = marker(i) {
                let sup = match sym.as_str() {
                    "\\<^sub>" => Some(false),
                    "\\<^sup>" => Some(true),
                    _ => None,
                };
                if let Some(sup) = sup
                    && let Some((c, end)) = one(i + len)
                    && let Some(sc) = script(sup, c)
                {
                    if in_scripts_font(c) {
                        styles.push((src_of.len(), Style::Script));
                    }
                    text.push(sc);
                    src_of.push(i);
                    i = end;
                    continue;
                }
                // \<^bsup>…\<^esup>, \<^bsub>…\<^esub>: as scripts when every character has a form
                let block = match sym.as_str() {
                    "\\<^bsup>" => Some((true, "\\<^esup>")),
                    "\\<^bsub>" => Some((false, "\\<^esub>")),
                    _ => None,
                };
                if let Some((sup, close)) = block {
                    let mut j = i + len;
                    let mut inner: Vec<(char, usize)> = Vec::new();
                    let mut closed = None;
                    while j < chars.len() && inner.len() < 64 {
                        if let Some((s2, l)) = marker(j)
                            && s2 == close
                        {
                            closed = Some(j + l);
                            break;
                        }
                        match one(j) {
                            Some((c, e)) if c != '\n' => {
                                inner.push((c, j));
                                j = e;
                            }
                            _ => break,
                        }
                    }
                    if let Some(end) = closed
                        && !inner.is_empty()
                        && inner.iter().all(|(c, _)| script(sup, *c).is_some())
                    {
                        for (k, (c, at)) in inner.iter().enumerate() {
                            if in_scripts_font(*c) {
                                styles.push((src_of.len(), Style::Script));
                            }
                            text.push(script(sup, *c).unwrap());
                            // the opening marker goes with the first character, the closing
                            // one with the last (the next character starts after it)
                            src_of.push(if k == 0 { i } else { *at });
                        }
                        i = end;
                        continue;
                    }
                }
                if sym == "\\<^bold>"
                    && let Some((c, end)) = one(i + len)
                    && c != '\n'
                {
                    styles.push((src_of.len(), Style::Bold));
                    text.push(c);
                    src_of.push(i);
                    i = end;
                    continue;
                }
                if let Some(c) = self.decode.get(&sym) {
                    if sym.starts_with("\\<^") {
                        styles.push((src_of.len(), Style::Control));
                    }
                    text.push(*c);
                    src_of.push(i);
                    i += len;
                    continue;
                }
            }
            text.push(chars[i]);
            src_of.push(i);
            i += 1;
        }
        src_of.push(chars.len());
        Decoded { text, src_of, styles }
    }
}

/// A character as source: its symbol if it is one (Greek letters are `\<alpha>`)
fn self_symbol(decode: &HashMap<String, char>, c: char) -> String {
    decode.iter().find(|(_, v)| **v == c).map_or_else(|| c.to_string(), |(k, _)| k.clone())
}

/// Length of the symbol `\<name>` or `\<^name>` at the start of `chars`
fn symbol_len(chars: &[char]) -> Option<usize> {
    let mut j = 2;
    if chars.get(j) == Some(&'^') {
        j += 1;
    }
    let start = j;
    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '\'') {
        j += 1;
    }
    (j > start && chars.get(j) == Some(&'>')).then_some(j + 1)
}

/// A decoded text and, for each of its code points (and its end), where it
/// starts in the source
#[derive(Clone, Debug, Default)]
pub struct Decoded {
    pub text: String,
    pub src_of: Vec<usize>,
    /// Decoded offsets set bold, or shown as control markers
    pub styles: Vec<(usize, Style)>,
}

impl Decoded {
    pub fn to_source(&self, decoded: usize) -> usize {
        self.src_of[decoded.min(self.src_of.len() - 1)]
    }

    /// The decoded offset at or before source offset `source`
    pub fn to_decoded(&self, source: usize) -> usize {
        match self.src_of.binary_search(&source) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    /// The source change for a change of the decoded text from `old` to the
    /// edited text `new`: the source between the mapped ends of the edited
    /// span is replaced by the encoded new text, everything else untouched
    pub fn source_change(&self, table: &Table, new: &str) -> super::ot::Change {
        let change = super::ot::diff(&self.text, new);
        change
            .into_iter()
            .map(|(s, e, t)| (self.to_source(s), self.to_source(e), table.encode(&t)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_script_markers_render_as_scripts() {
        let table = Table::fallback();
        let d = table.decode("tr⇩v a ∧ ($st)⇧< = x⇘ij⇙");
        assert!(!d.text.contains('⇩') && !d.text.contains('⇧') && !d.text.contains('⇘'), "{}", d.text);
        assert_eq!(d.src_of.len(), d.text.chars().count() + 1);
    }

    #[test]
    fn decodes_and_maps_offsets() {
        let table = Table::fallback();
        let src = "lemma \"\\<forall>x. P x \\<longrightarrow> Q\"";
        let d = table.decode(src);
        assert_eq!(d.text, "lemma \"∀x. P x ⟶ Q\"");
        let i = d.text.chars().position(|c| c == '∀').unwrap();
        assert_eq!(d.to_source(i), 7);
        assert_eq!(d.to_source(i + 1), 7 + "\\<forall>".len());
        assert_eq!(d.to_decoded(9), i);
    }

    #[test]
    fn edits_keep_other_escapes() {
        let table = Table::fallback();
        let src = "\\<forall>x. P x";
        let d = table.decode(src);
        let edited = "∀x. P y ∧ Q";
        let change = d.source_change(&table, edited);
        let out = super::super::ot::apply(src, &change);
        assert_eq!(out, "\\<forall>x. P y \\<and> Q");
    }

    #[test]
    fn searches_by_latex_and_fuzzily() {
        let table = Table::fallback();
        assert_eq!(table.search("leq", 1)[0].0, '≤');
        assert_eq!(table.search("to", 3).iter().filter(|r| r.0 == '⟶').count(), 1);
        assert_eq!(table.search("forall", 1)[0].0, '∀');
        assert_eq!(table.search("longright", 2)[0].0, '⟶');
        assert!(fuzzy("sbsq", "subseteq").is_some());
    }

    #[test]
    fn control_symbols_render_as_scripts_and_bold() {
        let table = Table::fallback();
        let d = table.decode("x\\<^sub>1 + y\\<^sup>2 + \\<^bold>A + z\\<^sub>\\<forall>");
        let sub = |c: char| script_glyph(false, c).unwrap();
        let sup = |c: char| script_glyph(true, c).unwrap();
        assert_eq!(d.text, format!("x{} + y{} + A + z⇩∀", sub('1'), sup('2')));
        assert_eq!(d.styles.iter().filter(|s| s.1 == Style::Bold).count(), 1);
        assert!(d.styles.iter().any(|s| s.1 == Style::Control));
        // Editing around a subscript keeps its escape
        let change = d.source_change(&table, &format!("{} + w{}", d.text, sub('2')));
        let out = super::super::ot::apply("x\\<^sub>1 + y\\<^sup>2 + \\<^bold>A + z\\<^sub>\\<forall>", &change);
        assert_eq!(out, "x\\<^sub>1 + y\\<^sup>2 + \\<^bold>A + z\\<^sub>\\<forall> + w\\<^sub>2");
    }

    #[test]
    fn stars_greek_and_script_blocks() {
        let table = Table::fallback();
        let d = table.decode("R\\<^sup>* ; x\\<^sup>\\<alpha> ; y\\<^bsup>n+1\\<^esup> ; z\\<^bsub>ij\\<^esub>");
        let sub = |c: char| script_glyph(false, c).unwrap();
        let sup = |c: char| script_glyph(true, c).unwrap();
        assert_eq!(d.text, format!("R{} ; x{} ; y{}{}{} ; z{}{}", sup('*'), sup('α'), sup('n'), sup('+'), sup('1'), sub('i'), sub('j')));
        assert_eq!(table.encode(&d.text), "R\\<^sup>* ; x\\<^sup>\\<alpha> ; y\\<^sup>n\\<^sup>+\\<^sup>1 ; z\\<^sub>i\\<^sub>j");
        // UTP's before and after markers
        assert_eq!(table.decode("tr\\<^sup>> = tr\\<^sup><").text, format!("tr{} = tr{}", sup('>'), sup('<')));
        // typing a plain star stays plain
        assert_eq!(table.encode("a*b"), "a*b");
    }

    #[test]
    fn unknown_and_partial_symbols_stay() {
        let table = Table::fallback();
        let d = table.decode("\\<foo> \\<for");
        assert_eq!(d.text, "\\<foo> \\<for");
    }
}

#[cfg(test)]
mod real_table_probe {
    use super::*;

    /// `DEDUCTUM_SYMBOLS=/tmp/isa-symbols.json`: how lines of a UTP theory decode with the
    /// prover's real symbol table
    #[test]
    fn probe_utp_lines() {
        let Ok(path) = std::env::var("DEDUCTUM_SYMBOLS") else { return };
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let entries: Vec<Entry> = serde_json::from_value(v["symbols"].clone()).unwrap();
        let table = Table::new(entries);
        for src in [
            "\"\\<lceil>\\<sigma>\\<rceil>\\<^sub>S\\<^sub>\\<sigma> \\<circ>\\<^sub>s x\"",
            "[pred]: \"lift_cond_srea b = \\<lceil>b\\<rceil>\\<^sub>S\\<^sub><\"",
            "(\"\\<lceil>_\\<rceil>\\<^sub>S\\<^sub>\\<leftarrow>\")",
            "lemma foo: \"x\\<^sup>> = y\\<^bsub>ab\\<^esub>\"",
        ] {
            let d = table.decode(src);
            let chars: Vec<char> = d.text.chars().collect();
            eprintln!("{src}\n  -> {:?}\n  codes {:?}", d.text, chars.iter().map(|c| format!("{:04x}", *c as u32)).collect::<Vec<_>>());
            eprintln!("  src_of {:?}", d.src_of);
        }
    }
}
