// SPDX-License-Identifier: GPL-3.0-only

//! A Lean 4 lexer for colouring. Lean's own parser is the prover's, and its semantic tokens
//! arrive later (design 8.4); until then, and for source the server has not seen, this lexer
//! colours what can be known from the characters alone: keywords, `sorry`, comments, doc
//! comments, strings, numbers, types, operators, attributes and `#commands`.
//!
//! Lean's tactic names are not reserved words: `left` and `right` are fine variable names (the
//! formalisation has `merge (left right : Annotation)`). So a tactic name is a keyword only where
//! a tactic can start (first on its line, after `by`, `·`, `<;>`, `;`, `=>`, `|`, `(`, or a
//! combinator such as `try`), and the few no-argument ones that are also common names (`left`,
//! `right`, `split`, `rfl`, …) only when nothing but the end of the line or a separator follows.
//! Words that are reserved in Lean (`theorem`, `by`, `fun`, `have`, `at`, …) are keywords
//! anywhere.
//!
//! Block comments nest and run over lines, as do strings. For line-by-line highlighting
//! [`lex_line`] takes the [`State`] the previous line ended in and returns the state it ends in;
//! [`lex`] does a whole text. Offsets are in code points (of the line, or of the text), as
//! everywhere in the app. Whitespace and plain punctuation produce no token, and tokens do not
//! overlap. A block comment over several lines gives one token per line.
//!
//! [`Class::colour`] maps the classes to the small [`Colour`] enum the app maps onto its palette.

/// What a token is, finely
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// A declaration or command keyword: `theorem`, `def`, `namespace`, `open`, `private`, …
    Command,
    /// Any other keyword or tactic: `by`, `fun`, `have`, `intro`, `simp`, `at`, …
    Keyword,
    /// `sorry` and `admit`: a placeholder for a proof not yet written
    Sorry,
    /// `-- …` and `/- … -/`
    Comment,
    /// `/-- … -/` and `/-! … -/`
    Doc,
    /// A string literal, or a character literal
    Str,
    Number,
    /// `Type`, `Prop`, `Sort u`, and the names of the common core types
    Type,
    /// `:=`, `=>`, `→`, `∀`, `∧`, `·`, …
    Operator,
    /// `@[simp]`, with its brackets
    Attribute,
    /// `#check`, `#eval`, …
    HashCommand,
    /// Any other name, including dotted ones such as `Nat.add_comm`
    Name,
}

/// The colour groups an app needs to tell apart
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colour {
    /// Declarations and commands: the major keywords (Isabelle's `keyword1`)
    Command,
    /// Tactics and term keywords (`keyword2`)
    Keyword,
    /// A proof left open (`sorry`): a warning colour
    Placeholder,
    Comment,
    /// Documentation comments, which are prose (Markdown)
    Doc,
    /// Strings, characters and numbers
    Literal,
    Type,
    Operator,
    Attribute,
}

impl Class {
    /// The colour group, or `None` for a plain name
    pub fn colour(self) -> Option<Colour> {
        Some(match self {
            Class::Command | Class::HashCommand => Colour::Command,
            Class::Keyword => Colour::Keyword,
            Class::Sorry => Colour::Placeholder,
            Class::Comment => Colour::Comment,
            Class::Doc => Colour::Doc,
            Class::Str | Class::Number => Colour::Literal,
            Class::Type => Colour::Type,
            Class::Operator => Colour::Operator,
            Class::Attribute => Colour::Attribute,
            Class::Name => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub class: Class,
}

/// What a line ends inside of: the state to start the next line with
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// How deeply nested in block comments (0: not in one)
    pub depth: u32,
    /// Whether that comment is a doc comment
    pub doc: bool,
    /// In a string that continues on the next line
    pub string: bool,
    /// How deeply inside the brackets of an `@[` attribute that continues
    pub attr: u32,
}

const COMMANDS: &[&str] = &[
    "theorem", "lemma", "def", "example", "instance", "structure", "inductive", "coinductive", "class", "abbrev",
    "namespace", "section", "end", "open", "export", "variable", "universe", "set_option", "import", "prelude",
    "attribute", "notation", "macro", "macro_rules", "syntax", "elab", "elab_rules", "mutual", "noncomputable",
    "private", "protected", "partial", "unsafe", "nonrec", "opaque", "axiom", "constant", "deriving", "extends",
    "where", "infix", "infixl", "infixr", "prefix", "postfix", "declare_syntax_cat", "initialize", "local",
    "scoped", "termination_by", "decreasing_by", "omit", "include", "alias", "irreducible_def", "run_cmd",
    "compile_inductive", "register_simp_attr", "suppress_compilation", "assert_not_exists",
];

const KEYWORDS: &[&str] = &[
    "by", "fun", "match", "nomatch", "nofun", "with", "have", "show", "from", "calc", "let", "in", "at", "if", "then",
    "else", "do", "return", "for", "while", "break", "continue", "mut", "using", "generalizing", "suffices", "try",
    "repeat", "first", "all_goals", "any_goals", "focus", "iterate", "case", "next", "unless", "assert!", "fun_prop",
    "termination_by", "decreasing_by", "at", "this", "forall",
];

/// Tactics, which are keywords only where a tactic can start
const TACTICS: &[&str] = &[
    "intro", "intros", "rintro", "exact", "exacts", "apply", "refine", "refine'", "simp", "simp_all", "dsimp", "simpa",
    "simp_arith", "rw", "rewrite", "rwa", "erw", "cases", "rcases", "obtain", "induction", "induction'", "cases'",
    "constructor", "use", "exists", "existsi", "omega", "decide", "aesop", "grind", "exfalso", "left", "right", "split",
    "unfold", "subst", "symm", "trans", "rfl", "trivial", "assumption", "contradiction", "revert", "clear",
    "specialize", "generalize", "injection", "ring", "ring_nf", "linarith", "nlinarith", "norm_num", "positivity",
    "field_simp", "ext", "funext", "congr", "rename_i", "norm_cast", "push_cast", "exact_mod_cast", "tauto",
    "split_ifs", "by_cases", "by_contra", "push_neg", "set", "skip", "done", "fail", "conv", "change", "nth_rewrite",
    "nth_rw", "ac_rfl", "simp_wf", "bound", "gcongr", "apply_fun", "lift", "rename", "subst_vars", "infer_instance",
    "inferInstance", "calc", "show", "admit", "exact?", "apply?", "rw?", "simp?", "decide!", "unfold_let", "delta",
    "native_decide", "cc", "hint", "exists", "constructorm", "casesm", "rcases'", "interval_cases", "fin_cases",
    "mod_cases", "rintros", "intro?", "with_reducible", "show_term", "wlog", "peel", "choose", "contrapose",
    "contrapose!", "absurd", "nofun", "simp_intro", "symm_saturate", "ofNat", "variable", "sorry", "exact_mod_cast",
    "dsimp only", "refine_lift", "borelize",
];

/// Tactics without arguments that are also common variable names: a keyword only if the line
/// (or separator) ends right after them
const AMBIGUOUS: &[&str] = &["left", "right", "split", "rfl", "skip", "symm", "trivial", "done", "this", "ext", "set"];

/// The common core types, coloured as types
const TYPE_NAMES: &[&str] = &[
    "Nat", "Int", "Bool", "List", "Option", "String", "Char", "Float", "Fin", "Array", "Unit", "Real", "Rat", "Set",
    "Finset", "Multiset", "Prod", "Sum", "Sigma", "Subtype", "Empty", "PUnit", "UInt8", "UInt16", "UInt32", "UInt64",
    "Std", "IO", "Id", "Except", "StateM",
];

/// Words after which `only` is a keyword
const SIMP_LIKE: &[&str] = &["simp", "simp_all", "dsimp", "simpa", "norm_num", "field_simp", "simp_arith", "push_cast", "aesop"];

fn is_letter_like(c: char) -> bool {
    // (Lean excludes λ, Π and Σ from identifiers)
    if matches!(c, 'λ' | 'Π' | 'Σ') {
        return false;
    }
    c.is_alphabetic() || ('\u{2100}'..='\u{214f}').contains(&c) || ('\u{1d49c}'..='\u{1d59f}').contains(&c)
}

fn is_id_start(c: char) -> bool {
    c == '_' || is_letter_like(c)
}

fn is_id_rest(c: char) -> bool {
    is_id_start(c) || c.is_ascii_digit() || matches!(c, '\'' | '!' | '?') || ('\u{2080}'..='\u{209c}').contains(&c) || ('\u{1d62}'..='\u{1d6a}').contains(&c) || (c.is_numeric() && !c.is_ascii())
}

fn is_bracket(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '⟨' | '⟩' | '⦃' | '⦄' | '⟦' | '⟧' | '‹' | '›' | '«' | '»' | ',')
}

/// A character that can be part of an operator such as `<;>`, `:=`, `→` or `≤`
fn is_op_char(c: char) -> bool {
    if c.is_ascii() {
        matches!(c, '+' | '-' | '*' | '/' | '<' | '>' | '=' | '!' | '&' | '|' | '^' | '%' | '~' | '$' | '?' | ':' | '\\' | '.' | ';' | '#' | '@')
    } else {
        !c.is_alphanumeric() && !c.is_whitespace() && !is_bracket(c) && !is_letter_like(c) && !matches!(c, '‹' | '›' | '«' | '»' | '“' | '”' | '‘' | '’')
    }
}

fn starts(c: &[char], i: usize, s: &str) -> bool {
    s.chars().enumerate().all(|(k, sc)| c.get(i + k) == Some(&sc))
}

/// The end of a block comment's rest from `i` (nested), and the depth it ends at
fn comment_rest(c: &[char], mut i: usize, mut depth: u32) -> (usize, u32) {
    while i < c.len() && depth > 0 {
        if starts(c, i, "/-") {
            depth += 1;
            i += 2;
        } else if starts(c, i, "-/") {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    (i, depth)
}

/// The end of a string's rest from `i`, and whether it continues on the next line
fn string_rest(c: &[char], mut i: usize) -> (usize, bool) {
    while i < c.len() {
        match c[i] {
            '"' => return (i + 1, false),
            '\\' => i += 2,
            _ => i += 1,
        }
    }
    (c.len(), true)
}

/// Whether the tactic names `word` may start a tactic after the token `prev`
fn starts_tactic(prev: Option<&str>) -> bool {
    match prev {
        None => true,
        Some(p) => matches!(
            p,
            "by" | "·" | "." | "<;>" | ";" | "=>" | "|" | "(" | "{" | "try" | "repeat" | "repeat'" | "first" | "all_goals" | "any_goals"
                | "iterate" | "focus" | "<|>" | "do" | "then" | "else" | "next" | "with"
        ),
    }
}

/// Lex one line (without its newline) starting in `state`: the tokens, and the state to start the next line in
pub fn lex_line(line: &str, mut state: State) -> (Vec<Token>, State) {
    let c: Vec<char> = line.chars().collect();
    let n = c.len();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    let mut prev: Option<String> = None;
    let push = |out: &mut Vec<Token>, s: usize, e: usize, class: Class| {
        if e > s {
            out.push(Token { start: s, end: e, class })
        }
    };
    // Continuations from the previous line
    if state.depth > 0 {
        let (e, d) = comment_rest(&c, 0, state.depth);
        let class = if state.doc { Class::Doc } else { Class::Comment };
        push(&mut out, 0, e, class);
        state.depth = d;
        i = e;
        if d > 0 {
            return (out, state);
        }
    } else if state.string {
        let (e, cont) = string_rest(&c, 0);
        push(&mut out, 0, e, Class::Str);
        state.string = cont;
        i = e;
        if cont {
            return (out, state);
        }
    } else if state.attr > 0 {
        let (e, d) = attribute_rest(&c, 0, state.attr);
        push(&mut out, 0, e, Class::Attribute);
        state.attr = d;
        i = e;
        if d > 0 {
            return (out, state);
        }
    }
    while i < n {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if starts(&c, i, "--") {
            push(&mut out, i, n, Class::Comment);
            break;
        }
        if starts(&c, i, "/-") {
            let doc = starts(&c, i, "/--") && !starts(&c, i, "/--/") || starts(&c, i, "/-!");
            let (e, d) = comment_rest(&c, i + 2, 1);
            push(&mut out, i, e, if doc { Class::Doc } else { Class::Comment });
            state.depth = d;
            state.doc = doc;
            i = e;
            continue;
        }
        if ch == '"' {
            let (e, cont) = string_rest(&c, i + 1);
            push(&mut out, i, e, Class::Str);
            state.string = cont;
            i = e;
            prev = Some("\"".into());
            continue;
        }
        if ch == '\'' {
            // 'a', '\n', '\x41', '\u{3b1}'
            let end = if c.get(i + 1) == Some(&'\\') {
                (i + 3..(i + 12).min(n)).find(|&k| c[k] == '\'').map(|k| k + 1)
            } else if c.get(i + 2) == Some(&'\'') && c.get(i + 1).is_some_and(|x| *x != '\'') {
                Some(i + 3)
            } else {
                None
            };
            if let Some(e) = end {
                push(&mut out, i, e, Class::Str);
                i = e;
                prev = Some("'".into());
                continue;
            }
        }
        if starts(&c, i, "@[") {
            let (e, d) = attribute_rest(&c, i + 2, 1);
            push(&mut out, i, e, Class::Attribute);
            state.attr = d;
            i = e;
            continue;
        }
        if ch == '#' && c.get(i + 1).is_some_and(|x| x.is_alphabetic()) {
            let mut e = i + 1;
            while e < n && is_id_rest(c[e]) {
                e += 1;
            }
            push(&mut out, i, e, Class::HashCommand);
            i = e;
            prev = Some("#".into());
            continue;
        }
        if ch.is_ascii_digit() {
            let e = number_end(&c, i);
            push(&mut out, i, e, Class::Number);
            i = e;
            prev = Some("0".into());
            continue;
        }
        if ch == '«' {
            let e = (i + 1..n).find(|&k| c[k] == '»').map_or(n, |k| k + 1);
            push(&mut out, i, e, Class::Name);
            i = e;
            prev = Some("x".into());
            continue;
        }
        // A name, a hole `?x`, or a constructor shorthand `.inl`
        let lead = if (ch == '?' || ch == '.') && c.get(i + 1).is_some_and(|x| is_id_start(*x)) { 1 } else { 0 };
        if is_id_start(ch) || lead == 1 {
            let (e, dotted) = name_end(&c, i + lead);
            let word: String = c[i..e].iter().collect();
            let class = if lead == 1 || dotted { Class::Name } else { classify(&word, &c, e, prev.as_deref()) };
            let (class, e) = if class == Class::Type && matches!(word.as_str(), "Type" | "Sort") { (class, universe_end(&c, e)) } else { (class, e) };
            push(&mut out, i, e, class);
            i = e;
            prev = Some(word);
            continue;
        }
        if ch == 'λ' {
            push(&mut out, i, i + 1, Class::Keyword);
            i += 1;
            prev = Some("fun".into());
            continue;
        }
        if ch == ';' && !c.get(i + 1).is_some_and(|x| is_op_char(*x) && *x != ';') {
            i += 1;
            prev = Some(";".into());
            continue;
        }
        if is_bracket(ch) {
            i += 1;
            prev = Some(ch.to_string());
            continue;
        }
        if is_op_char(ch) {
            let mut e = i + 1;
            while e < n && is_op_char(c[e]) && !starts(&c, e, "--") && !starts(&c, e, "/-") {
                e += 1;
            }
            let text: String = c[i..e].iter().collect();
            push(&mut out, i, e, Class::Operator);
            i = e;
            prev = Some(text);
            continue;
        }
        // anything else (a stray symbol): a plain character
        i += 1;
        prev = Some(ch.to_string());
    }
    (out, state)
}

fn attribute_rest(c: &[char], mut i: usize, mut depth: u32) -> (usize, u32) {
    while i < c.len() && depth > 0 {
        match c[i] {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    (i, depth)
}

fn number_end(c: &[char], i: usize) -> usize {
    let n = c.len();
    let mut e = i;
    if c[i] == '0' && matches!(c.get(i + 1), Some('x' | 'X' | 'b' | 'B' | 'o' | 'O')) {
        e = i + 2;
        while e < n && (c[e].is_ascii_hexdigit() || c[e] == '_') {
            e += 1;
        }
        return e;
    }
    while e < n && (c[e].is_ascii_digit() || c[e] == '_') {
        e += 1;
    }
    if c.get(e) == Some(&'.') && c.get(e + 1).is_some_and(|x| x.is_ascii_digit()) {
        e += 1;
        while e < n && c[e].is_ascii_digit() {
            e += 1;
        }
    }
    if matches!(c.get(e), Some('e' | 'E')) {
        let mut k = e + 1;
        if matches!(c.get(k), Some('+' | '-')) {
            k += 1;
        }
        if c.get(k).is_some_and(|x| x.is_ascii_digit()) {
            while k < n && c[k].is_ascii_digit() {
                k += 1;
            }
            e = k;
        }
    }
    e
}

/// The end of the name starting at `i`, and whether it has dots (`Nat.add_comm`, `h.1`)
fn name_end(c: &[char], mut i: usize) -> (usize, bool) {
    let n = c.len();
    let mut dotted = false;
    loop {
        while i < n && is_id_rest(c[i]) {
            i += 1;
        }
        if c.get(i) == Some(&'.') && c.get(i + 1).is_some_and(|x| is_id_start(*x) || x.is_ascii_digit() || *x == '«') {
            dotted = true;
            i += 1;
            continue;
        }
        return (i, dotted);
    }
}

/// `Type u`, `Type*`, `Sort _`, `Type 1`: the universe argument is part of the token
fn universe_end(c: &[char], e: usize) -> usize {
    if c.get(e) == Some(&'*') {
        return e + 1;
    }
    let mut k = e;
    while c.get(k).is_some_and(|x| *x == ' ') {
        k += 1;
    }
    if k == e {
        return e;
    }
    let mut m = k;
    while c.get(m).is_some_and(|x| x.is_ascii_alphanumeric() || matches!(x, '_' | '\'')) {
        m += 1;
    }
    if m == k {
        return e;
    }
    let w: String = c[k..m].iter().collect();
    let universe = w == "_" || w.chars().all(|x| x.is_ascii_digit()) || (w.starts_with(|x: char| x.is_ascii_lowercase()) && !is_reserved(&w) && !TACTICS.contains(&w.as_str()));
    if universe && !c.get(m).is_some_and(|x| matches!(x, '.' | '(' | ':')) { m } else { e }
}

fn is_reserved(w: &str) -> bool {
    COMMANDS.contains(&w) || KEYWORDS.contains(&w)
}

fn classify(word: &str, c: &[char], end: usize, prev: Option<&str>) -> Class {
    match word {
        "sorry" => return Class::Sorry,
        "Type" | "Prop" | "Sort" | "Bool" | "Type*" => return Class::Type,
        _ => {}
    }
    if COMMANDS.contains(&word) {
        return Class::Command;
    }
    if KEYWORDS.contains(&word) && word != "this" && word != "forall" {
        return Class::Keyword;
    }
    if word == "only" && prev.is_some_and(|p| SIMP_LIKE.contains(&p)) {
        return Class::Keyword;
    }
    if TYPE_NAMES.contains(&word) {
        return Class::Type;
    }
    let base = word.trim_end_matches(['?', '!']);
    if starts_tactic(prev) && (TACTICS.contains(&word) || TACTICS.contains(&base) && base != word) {
        if prev == Some("(") && matches!(base, "left" | "right" | "set" | "ext" | "rename" | "change" | "use" | "show") {
            return Class::Name;
        }
        if AMBIGUOUS.contains(&word) {
            // only when nothing but a separator or the end of the line follows
            let rest: Vec<char> = c[end..].iter().copied().skip_while(|x| *x == ' ').collect();
            let ok = rest.is_empty() || rest[0] == ';' || rest[0] == ')' || starts(&rest, 0, "<;>") || starts(&rest, 0, "--") || rest[0] == '|';
            return if ok { if word == "this" { Class::Name } else { Class::Keyword } } else { Class::Name };
        }
        if word == "admit" {
            return Class::Sorry;
        }
        return Class::Keyword;
    }
    Class::Name
}

/// All tokens of a text, with offsets in code points of the text
pub fn lex(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut state = State::default();
    let mut offset = 0;
    for line in text.split('\n') {
        let (tokens, next) = lex_line(line, state);
        out.extend(tokens.into_iter().map(|t| Token { start: t.start + offset, end: t.end + offset, class: t.class }));
        state = next;
        offset += line.chars().count() + 1;
    }
    out
}

/// The class of every code point of a text: what a highlighter indexed by position wants
pub fn classes(text: &str) -> Vec<Option<Class>> {
    let mut out = vec![None; text.chars().count()];
    for t in lex(text) {
        for slot in &mut out[t.start..t.end] {
            *slot = Some(t.class);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The (text, class) of every token
    fn toks(text: &str) -> Vec<(String, Class)> {
        let chars: Vec<char> = text.chars().collect();
        lex(text).into_iter().map(|t| (chars[t.start..t.end].iter().collect(), t.class)).collect()
    }

    fn class_of(text: &str, word: &str) -> Option<Class> {
        toks(text).into_iter().find(|(t, _)| t == word).map(|(_, c)| c)
    }

    #[test]
    fn a_theorem_is_coloured() {
        let t = toks("theorem foo (n : Nat) : n + 0 = n := by\n  simp\n");
        assert_eq!(t[0], ("theorem".into(), Class::Command));
        assert!(t.contains(&("Nat".into(), Class::Type)));
        assert!(t.contains(&(":=".into(), Class::Operator)));
        assert!(t.contains(&("by".into(), Class::Keyword)));
        assert!(t.contains(&("simp".into(), Class::Keyword)));
        assert!(t.contains(&("0".into(), Class::Number)));
        assert!(t.contains(&("foo".into(), Class::Name)));
    }

    #[test]
    fn sorry_is_its_own_class() {
        assert_eq!(class_of("  sorry", "sorry"), Some(Class::Sorry));
        assert_eq!(class_of("  admit", "admit"), Some(Class::Sorry));
        assert_eq!(class_of("x := by sorry", "sorry"), Some(Class::Sorry));
        assert_eq!(Class::Sorry.colour(), Some(Colour::Placeholder));
    }

    #[test]
    fn tactic_names_are_variables_elsewhere() {
        // from DSRV: binders called left and right
        let t = toks("def merge (left right : Annotation) : Annotation :=\n  ⟨left.causes, right.causes⟩");
        assert_eq!(class_of("def merge (left right : Annotation)", "left"), Some(Class::Name));
        assert_eq!(class_of("def merge (left right : Annotation)", "right"), Some(Class::Name));
        assert!(t.iter().all(|(w, c)| !(w == "left" && *c == Class::Keyword)));
        assert_eq!(class_of("  left", "left"), Some(Class::Keyword));
        assert_eq!(class_of("  left\n", "left"), Some(Class::Keyword));
        assert_eq!(class_of("  left + right", "left"), Some(Class::Name));
        assert_eq!(class_of("  exact foo", "exact"), Some(Class::Keyword));
        assert_eq!(class_of("x := foo exact", "exact"), Some(Class::Name));
        assert_eq!(class_of("  · simp", "simp"), Some(Class::Keyword));
        assert_eq!(class_of("  cases h <;> simp", "simp"), Some(Class::Keyword));
        assert_eq!(class_of("  refine ⟨_, by omega⟩", "omega"), Some(Class::Keyword));
        assert_eq!(class_of("  have := rfl", "rfl"), Some(Class::Name));
        assert_eq!(class_of("  rfl", "rfl"), Some(Class::Keyword));
    }

    #[test]
    fn suggestion_tactics_keep_their_question_mark() {
        let t = toks("  exact?\n  simp? [foo]\n");
        assert!(t.contains(&("exact?".into(), Class::Keyword)), "{t:?}");
        assert!(t.contains(&("simp?".into(), Class::Keyword)), "{t:?}");
        assert_eq!(class_of("  simp only [foo] at h", "only"), Some(Class::Keyword));
        assert_eq!(class_of("  simp only [foo] at h", "at"), Some(Class::Keyword));
    }

    #[test]
    fn comments_nest_and_cross_lines() {
        let text = "/- outer /- inner -/ still -/ def x := 1\n-- line comment theorem\n/-- doc\n   more -/\ntheorem t : True := trivial";
        let t = toks(text);
        assert_eq!(t[0], ("/- outer /- inner -/ still -/".into(), Class::Comment));
        assert_eq!(t[1], ("def".into(), Class::Command));
        assert!(t.contains(&("-- line comment theorem".into(), Class::Comment)));
        assert!(t.contains(&("/-- doc".into(), Class::Doc)));
        assert!(t.contains(&("   more -/".into(), Class::Doc)));
        assert!(t.contains(&("theorem".into(), Class::Command)));
    }

    #[test]
    fn module_docs_are_docs() {
        let t = toks("/-!\n# Title\n\nSome `code` and $x$.\n-/\nopen Foo");
        assert_eq!(t.iter().filter(|(_, c)| *c == Class::Doc).count(), 4);
        assert!(t.contains(&("open".into(), Class::Command)));
        assert!(!t.iter().any(|(w, _)| w == "Title"), "inside the doc comment there are no names: {t:?}");
    }

    #[test]
    fn resumable_state_over_lines() {
        let (t1, s1) = lex_line("/- start /- nested", State::default());
        assert_eq!(s1.depth, 2);
        assert_eq!(t1.len(), 1);
        let (t2, s2) = lex_line("end -/ more -/ theorem", s1);
        assert_eq!(s2.depth, 0);
        assert_eq!(t2[0], Token { start: 0, end: 14, class: Class::Comment });
        assert_eq!(t2[1].class, Class::Command);
        // a doc comment keeps its class on later lines
        let (_, s) = lex_line("/-- a", State::default());
        let (t, s) = lex_line("b", s);
        assert_eq!(t[0].class, Class::Doc);
        assert!(s.depth == 1 && s.doc);
        // line by line gives the same as the whole text
        let text = "/- a\n b -/ def f := \"x\ny\" + 1\n@[simp\n, inline] theorem t";
        let mut state = State::default();
        let mut all = Vec::new();
        let mut offset = 0;
        for line in text.split('\n') {
            let (ts, s) = lex_line(line, state);
            all.extend(ts.into_iter().map(|t| Token { start: t.start + offset, end: t.end + offset, class: t.class }));
            state = s;
            offset += line.chars().count() + 1;
        }
        assert_eq!(all, lex(text));
    }

    #[test]
    fn strings_chars_and_numbers() {
        let t = toks("#eval s!\"a\\\"b\" ++ 'x' ++ '\\n' ++ 0x1F + 3.14 + 1_000");
        assert!(t.contains(&("#eval".into(), Class::HashCommand)), "{t:?}");
        assert!(t.contains(&("\"a\\\"b\"".into(), Class::Str)), "{t:?}");
        assert!(t.contains(&("'x'".into(), Class::Str)));
        assert!(t.contains(&("'\\n'".into(), Class::Str)));
        assert!(t.contains(&("0x1F".into(), Class::Number)));
        assert!(t.contains(&("3.14".into(), Class::Number)));
        assert!(t.contains(&("1_000".into(), Class::Number)));
        // a multi-line string
        let (_, s) = lex_line("def s := \"abc", State::default());
        assert!(s.string);
        let (t, s) = lex_line("def\" theorem", s);
        assert_eq!(t[0], Token { start: 0, end: 4, class: Class::Str });
        assert!(!s.string);
        // a prime is part of a name
        assert_eq!(class_of("  exact h'", "h'"), Some(Class::Name));
    }

    #[test]
    fn types_and_universes() {
        assert_eq!(class_of("(α : Type u) (β : Type)", "Type u"), Some(Class::Type));
        assert_eq!(class_of("(α : Type u) (β : Type)", "Type"), Some(Class::Type));
        assert_eq!(class_of("def f : Sort _ := x", "Sort _"), Some(Class::Type));
        assert_eq!(class_of("structure S where", "where"), Some(Class::Command));
        assert_eq!(class_of("(α : Type*)", "Type*"), Some(Class::Type));
        assert_eq!(class_of("theorem t : Prop := p", "Prop"), Some(Class::Type));
        assert_eq!(class_of("example : Type x := y", "Type x"), Some(Class::Type));
        // a following keyword is not a universe
        assert_eq!(class_of("class C (α : Type) where", "Type"), Some(Class::Type));
        assert_eq!(class_of("inductive T : Type where", "where"), Some(Class::Command));
    }

    #[test]
    fn attributes_operators_and_dotted_names() {
        let t = toks("@[simp, inline] theorem Nat.foo : ∀ x, x ≤ y → x ∧ y ↔ h.1 := by omega");
        assert_eq!(t[0], ("@[simp, inline]".into(), Class::Attribute));
        assert!(t.contains(&("Nat.foo".into(), Class::Name)));
        assert!(t.contains(&("∀".into(), Class::Operator)));
        assert!(t.contains(&("≤".into(), Class::Operator)));
        assert!(t.contains(&("→".into(), Class::Operator)));
        assert!(t.contains(&("h.1".into(), Class::Name)));
        assert!(t.contains(&("omega".into(), Class::Keyword)));
        // `λ` is `fun`, a comment after an operator is still a comment
        assert!(toks("λ x => x --c").contains(&("--c".into(), Class::Comment)));
        assert_eq!(toks("λ x => x")[0], ("λ".into(), Class::Keyword));
        assert_eq!(toks("a <;> b")[1], ("<;>".into(), Class::Operator));
        // a multi-line attribute
        let (_, s) = lex_line("@[simp,", State::default());
        assert_eq!(s.attr, 1);
        let (t, s) = lex_line("  inline] def", s);
        assert_eq!(t[0].class, Class::Attribute);
        assert_eq!(s.attr, 0);
        assert_eq!(t[1].class, Class::Command);
    }

    #[test]
    fn colours_group_the_classes() {
        assert_eq!(Class::Command.colour(), Some(Colour::Command));
        assert_eq!(Class::HashCommand.colour(), Some(Colour::Command));
        assert_eq!(Class::Name.colour(), None);
        assert_eq!(Class::Str.colour(), Some(Colour::Literal));
        assert_eq!(Class::Number.colour(), Some(Colour::Literal));
        assert_eq!(Class::Doc.colour(), Some(Colour::Doc));
    }

    #[test]
    fn classes_index_by_code_point() {
        let c = classes("α := 'x' -- é");
        assert_eq!(c.len(), 13);
        assert_eq!(c[0], Some(Class::Name));
        assert_eq!(c[1], None);
        assert_eq!(c[2], Some(Class::Operator));
        assert_eq!(c[5], Some(Class::Str));
        assert_eq!(c[12], Some(Class::Comment));
    }

    #[test]
    fn whole_dsrv_files_lex_without_gaps_or_overlap() {
        let dir = "/tmp/dsrv-lean-copy/DSRV";
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "lean") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            let toks = lex(&text);
            let n = text.chars().count();
            let mut last = 0;
            for t in &toks {
                assert!(t.start >= last && t.end > t.start && t.end <= n, "{p:?}: {t:?} after {last}");
                last = t.end;
            }
            // module docs and declarations are found
            assert!(toks.iter().any(|t| t.class == Class::Command), "{p:?}");
        }
    }
}
