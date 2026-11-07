// SPDX-License-Identifier: GPL-3.0-only

//! A Lean file as the literate view lays it out: blocks of source, in order, each one prose or
//! one declaration (design 7.3, as `theory.rs` does for Isabelle). Lean has no command spans from
//! the prover until the server answers, so the split is lexical: it looks at the first token of
//! each line at column 0, outside comments and strings, and never drops or reorders source.
//!
//! * `/-! … -/` (a module doc) is a prose block. It is a [`Kind::Heading`] when it holds nothing
//!   but one Markdown heading, otherwise a [`Kind::Text`] that remembers the level of its first
//!   heading.
//! * `/-- … -/` (a docstring) belongs to the declaration after it: one formal block, with the
//!   docstring's range in [`Block::doc`] so that the view can draw it as prose above the code,
//!   which is [`Block::code`]. A docstring that precedes nothing is a text block.
//! * A command at column 0 (`theorem`, `def`, `instance`, `namespace`, `open`, `#eval`, `@[simp]`,
//!   …) starts a formal block that runs to the next such line. Lines in between, indented or not
//!   (`| ctor`, `deriving Repr`, `termination_by`), belong to it, and so do blank lines inside it.
//!   Attributes and modifiers on a line of their own (`@[simp]`, `private`), and `open Foo in`,
//!   continue into the next command. A `mutual` block runs to its `end`. Consecutive `import`s
//!   are one header block.
//! * A comment at column 0 between blocks is a [`Kind::Comment`] block; so is any other stray text.
//!
//! Blocks are trimmed: the text between two blocks is whitespace only (a test checks it). All
//! offsets are code points. Each block also carries its prover-like commands ([`commands`]).
//!
//! [`commands`] splits a declaration the way the host's Lean driver does, so that a status can be
//! shown per step: the header up to and including `:= by` is one command (named by its keyword,
//! `theorem`), then each tactic of the block is one, from its first token to the end of its
//! continuation lines (the lines indented further), named by that token (`intro`, `·`, `have`).
//! A declaration without `:= by` is one command.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The `import`s at the top of a file
    Header,
    /// A module doc that is only a Markdown heading of this level
    Heading(u8),
    /// A module doc, or a docstring on nothing
    Text,
    /// A declaration or other command, with its docstring if it has one
    Formal,
    /// A comment, or stray source, between blocks
    Comment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub start: usize,
    pub end: usize,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    /// The whole block, docstring included, without surrounding blank space
    pub start: usize,
    pub end: usize,
    /// For prose blocks and for a docstring: the span between the delimiters
    pub body: Option<(usize, usize)>,
    /// A formal block's docstring, delimiters included (to be drawn as prose above the code)
    pub doc: Option<(usize, usize)>,
    /// The source of the code: the whole block, or what follows the docstring
    pub code: (usize, usize),
    /// The command keyword of a formal block (`theorem`, after any attributes and modifiers)
    pub name: String,
    /// What the declaration declares (`foo` in `theorem foo : …`), when it has a name
    pub title: String,
    /// The level of the first Markdown heading of a prose block
    pub level: Option<u8>,
    pub commands: Vec<Command>,
}

const COMMANDS: &[&str] = &[
    "theorem", "lemma", "def", "example", "instance", "structure", "inductive", "coinductive", "class", "abbrev",
    "namespace", "section", "end", "open", "export", "variable", "universe", "set_option", "import", "prelude",
    "attribute", "notation", "macro", "macro_rules", "syntax", "elab", "elab_rules", "mutual", "noncomputable",
    "private", "protected", "partial", "unsafe", "nonrec", "opaque", "axiom", "infix", "infixl", "infixr", "prefix",
    "postfix", "declare_syntax_cat", "initialize", "builtin_initialize", "local", "scoped", "omit", "include",
    "alias", "irreducible_def", "run_cmd", "proof_wanted", "suppress_compilation", "assert_not_exists",
    "add_decl_doc", "compile_inductive", "register_simp_attr", "unseal", "seal", "library_note",
];

/// Words that go before a command, on its line or the line above
const MODIFIERS: &[&str] = &["private", "protected", "noncomputable", "partial", "unsafe", "nonrec", "local", "scoped"];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '\'' | '.' | '!' | '?')
}

/// The source as characters, with the scanning helpers the splitters share
struct Src {
    c: Vec<char>,
}

impl Src {
    fn new(text: &str) -> Self {
        Src { c: text.chars().collect() }
    }

    fn n(&self) -> usize {
        self.c.len()
    }

    fn starts(&self, i: usize, s: &str) -> bool {
        s.chars().enumerate().all(|(k, sc)| self.c.get(i + k) == Some(&sc))
    }

    fn line_start(&self, i: usize) -> usize {
        self.c[..i.min(self.n())].iter().rposition(|c| *c == '\n').map_or(0, |k| k + 1)
    }

    fn line_end(&self, i: usize) -> usize {
        (i..self.n()).find(|&k| self.c[k] == '\n').unwrap_or(self.n())
    }

    fn word_at(&self, i: usize) -> String {
        self.c[i.min(self.n())..].iter().take_while(|c| is_word_char(**c)).collect()
    }

    /// Past spaces, tabs and newlines
    fn skip_ws(&self, mut i: usize) -> usize {
        while i < self.n() && self.c[i].is_whitespace() {
            i += 1;
        }
        i
    }

    /// The end of the block comment starting at `i` (past its `-/`), nested comments counted
    fn comment_end(&self, i: usize) -> usize {
        let mut depth = 0;
        let mut k = i;
        while k < self.n() {
            if self.starts(k, "/-") {
                depth += 1;
                k += 2;
            } else if self.starts(k, "-/") {
                depth -= 1;
                k += 2;
                if depth == 0 {
                    return k;
                }
            } else {
                k += 1;
            }
        }
        self.n()
    }

    fn string_end(&self, i: usize) -> usize {
        let mut k = i + 1;
        while k < self.n() {
            match self.c[k] {
                '"' => return k + 1,
                '\\' => k += 2,
                _ => k += 1,
            }
        }
        self.n()
    }

    /// A character literal at `i` (`'a'`, `'\n'`), or its end
    fn char_end(&self, i: usize) -> Option<usize> {
        if self.c.get(i + 1) == Some(&'\\') {
            (i + 3..(i + 12).min(self.n())).find(|&k| self.c[k] == '\'').map(|k| k + 1)
        } else if self.c.get(i + 2) == Some(&'\'') && self.c.get(i + 1).is_some_and(|x| *x != '\'' && *x != '\n') {
            Some(i + 3)
        } else {
            None
        }
    }

    /// The end of the logical line from `i`: the next newline outside comments and strings
    /// (which can run over several physical lines), and where its code ends (before a trailing
    /// comment, `None` if it has no code)
    fn logical_line(&self, mut i: usize) -> (usize, Option<usize>) {
        let mut code_end = None;
        while i < self.n() {
            let ch = self.c[i];
            if ch == '\n' {
                break;
            }
            if self.starts(i, "--") {
                i = self.line_end(i);
                break;
            }
            if self.starts(i, "/-") {
                i = self.comment_end(i);
                continue;
            }
            if ch == '"' {
                i = self.string_end(i);
                code_end = Some(i);
                continue;
            }
            if ch == '\'' && let Some(e) = self.char_end(i) {
                i = e;
                code_end = Some(i);
                continue;
            }
            if !ch.is_whitespace() {
                code_end = Some(i + 1);
            }
            i += 1;
        }
        (i, code_end)
    }

    /// The words of `from..to` outside comments, attributes (`@[…]`) and strings
    fn words(&self, from: usize, to: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = from;
        while i < to {
            if self.starts(i, "--") {
                i = self.line_end(i);
            } else if self.starts(i, "/-") {
                i = self.comment_end(i);
            } else if self.starts(i, "@[") {
                let mut depth = 0;
                while i < to {
                    match self.c[i] {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                out.push("@[]".to_string());
            } else if self.c[i] == '"' {
                i = self.string_end(i);
                out.push("\"\"".to_string());
            } else if self.c[i] == '#' && self.c.get(i + 1).is_some_and(|c| c.is_alphabetic()) {
                let w = format!("#{}", self.word_at(i + 1));
                i += w.chars().count();
                out.push(w);
            } else if is_word_char(self.c[i]) {
                let w = self.word_at(i);
                i += w.chars().count();
                out.push(w);
            } else {
                i += 1;
            }
        }
        out
    }
}

/// Whether the line at `i` starts a command: its keyword, or `@[`, or `#word`
fn starts_command(s: &Src, i: usize) -> bool {
    if s.starts(i, "@[") {
        return true;
    }
    if s.c.get(i) == Some(&'#') && s.c.get(i + 1).is_some_and(|c| c.is_alphabetic()) {
        return true;
    }
    let w = s.word_at(i);
    if w == "deriving" {
        // `deriving instance …` is a command; `deriving Repr` ends a structure
        return s.word_at(s.skip_ws(i + 8)) == "instance";
    }
    COMMANDS.contains(&w.as_str())
}

/// The command keyword of a declaration's source, and its name
fn keyword_and_title(s: &Src, from: usize, to: usize) -> (String, String) {
    let words = s.words(from, to);
    let mut it = words.into_iter().skip_while(|w| w == "@[]" || MODIFIERS.contains(&w.as_str()));
    let kw = it.next().unwrap_or_default();
    let title = if kw.starts_with('#') { String::new() } else { it.next().filter(|w| w.starts_with(|c: char| c.is_alphabetic() || c == '_') && !matches!(w.as_str(), "where" | "in")).unwrap_or_default() };
    (kw, title)
}

/// Whether source so far leaves the command unfinished: only attributes and modifiers, or a
/// trailing `in` (`open Foo in`)
fn unfinished(s: &Src, from: usize, to: usize) -> bool {
    let words = s.words(from, to);
    words.iter().all(|w| w == "@[]" || MODIFIERS.contains(&w.as_str())) || words.last().is_some_and(|w| w == "in")
}

/// The level of a Markdown heading line (`## Title`)
fn heading_level(line: &str) -> Option<u8> {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&n).then_some(())?;
    t[n..].starts_with([' ', '\t']).then_some(n as u8)
}

/// The Markdown headings of a text outside fenced code: (level, byte offset of the line, title)
pub fn headings(markdown: &str) -> Vec<(u8, usize, String)> {
    let mut out = Vec::new();
    let mut fence = false;
    let mut at = 0;
    for line in markdown.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
        } else if !fence && let Some(level) = heading_level(line) {
            out.push((level, at, line.trim().trim_start_matches('#').trim_end_matches('#').trim().to_string()));
        }
        at += line.len();
    }
    out
}

/// A Text or Heading block for a module doc or a stray docstring
fn prose_block(s: &Src, start: usize, end: usize) -> Block {
    let body_start = start + 3;
    let closed = end >= body_start + 2 && s.starts(end - 2, "-/");
    let body_end = if closed { end - 2 } else { end }.max(body_start);
    let body: String = s.c[body_start.min(s.n())..body_end.min(s.n())].iter().collect();
    let hs = headings(&body);
    let nonblank = body.lines().filter(|l| !l.trim().is_empty()).count();
    let kind = match hs.first() {
        Some((level, _, _)) if nonblank == 1 => Kind::Heading(*level),
        _ => Kind::Text,
    };
    Block {
        kind,
        start,
        end,
        body: Some((body_start, body_end)),
        doc: None,
        code: (start, end),
        name: String::new(),
        title: String::new(),
        level: hs.first().map(|h| h.0),
        commands: Vec::new(),
    }
}

/// The end of the logical line at `p`, without trailing space (a trailing comment is part of it)
fn trimmed_line_end(s: &Src, p: usize) -> usize {
    let (e, _) = s.logical_line(p);
    (p..e).rev().find(|&k| !s.c[k].is_whitespace()).map_or(p, |k| k + 1)
}

/// The blocks of a Lean file, in order
pub fn blocks(text: &str) -> Vec<Block> {
    let s = Src::new(text);
    let n = s.n();
    let mut out: Vec<Block> = Vec::new();
    // The formal block being built, whether a `mutual` is open in it
    let mut cur: Option<Block> = None;
    let mut mutual = false;
    let flush = |out: &mut Vec<Block>, cur: &mut Option<Block>, s: &Src| {
        if let Some(mut b) = cur.take() {
            let (kw, title) = keyword_and_title(s, b.code.0, b.end);
            b.name = kw;
            b.title = title;
            b.commands = split_commands(s, b.code.0, b.end);
            out.push(b);
        }
    };
    let mut p = 0;
    while p < n {
        p = s.skip_ws(p);
        if p >= n {
            break;
        }
        let col = p - s.line_start(p);
        let continuing = cur.is_some() && col > 0;
        if col == 0 || cur.is_none() {
            // Something that may start a block
            if s.starts(p, "/-!") {
                flush(&mut out, &mut cur, &s);
                let e = s.comment_end(p);
                out.push(prose_block(&s, p, e));
                p = e;
                continue;
            }
            if s.starts(p, "/--") && !s.starts(p, "/--/") {
                flush(&mut out, &mut cur, &s);
                let e = s.comment_end(p);
                let q = s.skip_ws(e);
                if q < n && starts_command(&s, q) && !s.starts(q, "import") {
                    let end = trimmed_line_end(&s, q);
                    let body_end = if s.starts(e.saturating_sub(2), "-/") { e - 2 } else { e }.max(p + 3);
                    cur = Some(Block {
                        kind: Kind::Formal,
                        start: p,
                        end,
                        body: Some((p + 3, body_end)),
                        doc: Some((p, e)),
                        code: (q, end),
                        name: String::new(),
                        title: String::new(),
                        level: None,
                        commands: Vec::new(),
                    });
                    mutual = s.word_at(q) == "mutual";
                    p = s.logical_line(q).0;
                } else {
                    let mut b = prose_block(&s, p, e);
                    b.kind = Kind::Text;
                    out.push(b);
                    p = e;
                }
                continue;
            }
            let in_open_mutual = mutual && cur.is_some();
            if (s.starts(p, "--") || s.starts(p, "/-")) && !in_open_mutual && !continuing {
                flush(&mut out, &mut cur, &s);
                // a run of line comments is one block, a block comment another
                let start = p;
                let mut end;
                if s.starts(p, "--") {
                    let mut q = p;
                    loop {
                        end = s.line_end(q);
                        let next = end + 1;
                        if next < n && s.starts(next, "--") {
                            q = next;
                        } else {
                            break;
                        }
                    }
                } else {
                    end = s.comment_end(p);
                }
                let end_trim = (start..end).rev().find(|&k| !s.c[k].is_whitespace()).map_or(start, |k| k + 1);
                out.push(Block { kind: Kind::Comment, start, end: end_trim, body: None, doc: None, code: (start, end_trim), name: String::new(), title: String::new(), level: None, commands: Vec::new() });
                p = end;
                continue;
            }
            if starts_command(&s, p) && !continuing {
                let word = s.word_at(p);
                let joins = match &cur {
                    Some(b) if mutual => {
                        if word == "end" {
                            mutual = false;
                        }
                        true
                    }
                    Some(b) => unfinished(&s, b.code.0, b.end) || (word == "import" && s.words(b.code.0, b.end).first().is_some_and(|w| w == "import" || w == "prelude")),
                    None => false,
                };
                if !joins {
                    flush(&mut out, &mut cur, &s);
                    let end = trimmed_line_end(&s, p);
                    let kind = if word == "import" || word == "prelude" { Kind::Header } else { Kind::Formal };
                    cur = Some(Block { kind, start: p, end, body: None, doc: None, code: (p, end), name: String::new(), title: String::new(), level: None, commands: Vec::new() });
                    mutual = word == "mutual";
                    p = s.logical_line(p).0;
                    continue;
                }
                // (continues the block: fall through to extend it over this line)
            } else if cur.is_none() {
                // stray source
                let (e, code_end) = s.logical_line(p);
                out.push(Block { kind: Kind::Comment, start: p, end: code_end.unwrap_or(e).max(p + 1).min(n), body: None, doc: None, code: (p, code_end.unwrap_or(e).max(p + 1).min(n)), name: String::new(), title: String::new(), level: None, commands: Vec::new() });
                p = e.max(p + 1);
                continue;
            }
        }
        // A line of the current block
        let (e, code_end) = s.logical_line(p);
        if let Some(b) = cur.as_mut() {
            // (a trailing comment, or a comment line inside the block, stays in it)
            let _ = code_end;
            let trimmed = (p..e).rev().find(|&k| !s.c[k].is_whitespace()).map_or(b.end, |k| k + 1);
            b.end = b.end.max(trimmed);
            b.code.1 = b.end;
        }
        p = e.max(p + 1);
    }
    flush(&mut out, &mut cur, &s);
    out
}

/// A declaration's commands: the header to `:= by`, then each tactic; or the whole thing
fn split_commands(s: &Src, from: usize, to: usize) -> Vec<Command> {
    let words = s.words(from, to);
    let (kw, _) = keyword_and_title(s, from, to);
    let kw = if kw.is_empty() { words.first().cloned().unwrap_or_default() } else { kw };
    let code_end = {
        let mut at = from;
        let mut last = from;
        while at < to {
            let (e, ce) = s.logical_line(at);
            last = ce.map_or(last, |c| c.min(to));
            at = e + 1;
        }
        last.max(from)
    };
    let whole = || vec![Command { start: from, end: code_end, name: kw.clone() }];
    if to <= from {
        return Vec::new();
    }
    if kw == "import" || kw == "prelude" || kw == "mutual" {
        return whole();
    }
    // The first `by` right after a `:=`, outside brackets and before any `where` or alternative
    let mut depth = 0i32;
    let mut prev = String::new();
    let mut i = from;
    let mut by_end = None;
    while i < to {
        let ch = s.c[i];
        if s.starts(i, "--") {
            i = s.line_end(i);
            continue;
        }
        if s.starts(i, "/-") {
            i = s.comment_end(i);
            continue;
        }
        if ch == '"' {
            i = s.string_end(i);
            prev = "\"".into();
            continue;
        }
        if matches!(ch, '(' | '[' | '{' | '⟨' | '⦃') {
            depth += 1;
        } else if matches!(ch, ')' | ']' | '}' | '⟩' | '⦄') {
            depth -= 1;
        }
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if is_word_char(ch) {
            let w = s.word_at(i);
            let len = w.chars().count();
            if depth <= 0 && w == "by" && prev == ":=" {
                by_end = Some(i + len);
                break;
            }
            if depth <= 0 && w == "where" {
                break;
            }
            i += len;
            prev = w;
            continue;
        }
        if depth <= 0 && ch == '|' && s.line_start(i) == s.c[s.line_start(i)..i].iter().take_while(|c| c.is_whitespace()).count() + s.line_start(i) {
            break;
        }
        if s.starts(i, ":=") {
            prev = ":=".into();
            i += 2;
            continue;
        }
        prev = ch.to_string();
        i += 1;
    }
    let Some(by_end) = by_end else { return whole() };
    let mut out = vec![Command { start: from, end: by_end, name: kw }];
    // The tactic block: its column is that of its first token
    let q = {
        let mut k = by_end;
        loop {
            k = s.skip_ws(k);
            if s.starts(k, "--") {
                k = s.line_end(k);
            } else if s.starts(k, "/-") {
                k = s.comment_end(k);
            } else {
                break k;
            }
        }
    };
    if q >= to {
        return out;
    }
    let tcol = q - s.line_start(q);
    // Go through the lines from the first tactic's: a line whose first token is at or left of the
    // block's column starts a command, deeper lines continue it
    let mut cmd: Option<Command> = None;
    let mut line = if s.line_start(q) >= by_end { s.line_start(q) } else { q };
    let mut first_line = true;
    while line < to {
        let (e, code_end) = s.logical_line(line);
        let tok = s.skip_ws(line).min(e);
        let phys_start = if first_line && line == q { q } else { line };
        // the first token of the line
        let first = if first_line { q } else { (phys_start..e).find(|&k| !s.c[k].is_whitespace()).unwrap_or(e) };
        let indent = if first_line { tcol } else { first - s.line_start(first) };
        let _ = tok;
        if let Some(ce) = code_end.filter(|ce| *ce > first && first < to) {
            let word_start = first;
            let continuation = !first_line && (indent > tcol || s.starts(word_start, "<;>") || s.starts(word_start, "|") && indent >= tcol);
            if continuation && cmd.is_some() {
                cmd.as_mut().unwrap().end = ce.min(to);
            } else {
                if let Some(c) = cmd.take() {
                    out.push(c);
                }
                let name = if s.c[word_start].is_alphabetic() || s.c[word_start] == '_' {
                    let w = s.word_at(word_start);
                    w
                } else {
                    s.c[word_start].to_string()
                };
                let name = if name == "." { "·".to_string() } else { name };
                cmd = Some(Command { start: word_start, end: ce.min(to), name });
            }
        }
        first_line = false;
        line = e + 1;
        if e >= s.n() {
            break;
        }
    }
    if let Some(c) = cmd {
        out.push(c);
    }
    out
}

/// Every command of a Lean file, in order: the commands of its formal blocks
pub fn commands(text: &str) -> Vec<Command> {
    blocks(text).into_iter().flat_map(|b| b.commands).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(text: &str, s: usize, e: usize) -> String {
        text.chars().skip(s).take(e - s).collect()
    }

    /// Blocks are in order and between them there is only whitespace
    fn check_covering(text: &str, bs: &[Block]) {
        let c: Vec<char> = text.chars().collect();
        let mut at = 0;
        for b in bs {
            assert!(b.start >= at, "overlap or disorder at {b:?}");
            assert!(c[at..b.start].iter().all(|x| x.is_whitespace()), "dropped source {:?} before {b:?}", slice(text, at, b.start));
            assert!(b.end > b.start && b.end <= c.len(), "{b:?}");
            assert!(!c[b.end - 1].is_whitespace(), "untrimmed {b:?}");
            at = b.end;
        }
        assert!(c[at..].iter().all(|x| x.is_whitespace()), "dropped source at the end: {:?}", slice(text, at, c.len()));
    }

    const FILE: &str = "import Mathlib.Data.Nat\nimport DSRV.Domain\n\n/-!\n# Intro\n\nSome $x$ text.\n-/\n\nnamespace Foo\n\n/-!\n## Sub\n-/\n\n-- a line comment\n-- continued\n\n/-- Adds. -/\ndef add (a b : Nat) : Nat := a + b\n\n@[simp]\ntheorem add_zero' (a : Nat) :\n    add a 0 = a := by\n  unfold add\n  simp\n\ninductive T where\n  | a\n  | b\nderiving Repr\n\nopen Foo in\n#eval add 1 2\n\nend Foo\n";

    #[test]
    fn a_file_is_split_into_blocks() {
        let bs = blocks(FILE);
        check_covering(FILE, &bs);
        let kinds: Vec<(Kind, String)> = bs.iter().map(|b| (b.kind, slice(FILE, b.start, b.end).lines().next().unwrap().to_string())).collect();
        let expect = [
            (Kind::Header, "import Mathlib.Data.Nat"),
            (Kind::Text, "/-!"),
            (Kind::Formal, "namespace Foo"),
            (Kind::Heading(2), "/-!"),
            (Kind::Comment, "-- a line comment"),
            (Kind::Formal, "/-- Adds. -/"),
            (Kind::Formal, "@[simp]"),
            (Kind::Formal, "inductive T where"),
            (Kind::Formal, "open Foo in"),
            (Kind::Formal, "end Foo"),
        ];
        assert_eq!(kinds.len(), expect.len(), "{kinds:?}");
        for (got, want) in kinds.iter().zip(expect) {
            assert_eq!((got.0, got.1.as_str()), want);
        }
        // the header holds both imports
        assert!(slice(FILE, bs[0].start, bs[0].end).ends_with("DSRV.Domain"));
        // the comment block holds both lines
        assert_eq!(slice(FILE, bs[4].start, bs[4].end), "-- a line comment\n-- continued");
    }

    #[test]
    fn module_docs_are_prose_with_a_body_and_level() {
        let bs = blocks(FILE);
        let intro = &bs[1];
        assert_eq!(slice(FILE, intro.start, intro.end).chars().take(3).collect::<String>(), "/-!");
        let (s, e) = intro.body.unwrap();
        assert_eq!(slice(FILE, s, e).trim(), "# Intro\n\nSome $x$ text.");
        assert_eq!(intro.kind, Kind::Text);
        assert_eq!(intro.level, Some(1));
    }

    #[test]
    fn a_docstring_belongs_to_its_declaration() {
        let bs = blocks(FILE);
        let add = bs.iter().find(|b| b.title == "add").unwrap();
        assert_eq!(add.name, "def");
        let (ds, de) = add.doc.unwrap();
        assert_eq!(slice(FILE, ds, de), "/-- Adds. -/");
        assert_eq!(slice(FILE, add.code.0, add.code.1), "def add (a b : Nat) : Nat := a + b");
        assert_eq!(add.start, ds);
        let (bs_, be) = add.body.unwrap();
        assert_eq!(slice(FILE, bs_, be), " Adds. ");
        // a docstring on nothing is text
        let t = "/-- Alone. -/\n\n/-! m -/\n";
        let b = blocks(t);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].kind, Kind::Text);
        check_covering(t, &b);
    }

    #[test]
    fn attributes_modifiers_and_in_continue_into_the_next_command() {
        let bs = blocks(FILE);
        let simp = bs.iter().find(|b| b.title == "add_zero'").unwrap();
        assert_eq!(simp.name, "theorem");
        assert!(slice(FILE, simp.start, simp.end).starts_with("@[simp]\ntheorem"));
        let ev = bs.iter().find(|b| b.name == "open").unwrap();
        assert!(slice(FILE, ev.start, ev.end).ends_with("#eval add 1 2"));
        let t = "private\nnoncomputable def f := 1\n\n#check f\n#check g\n";
        let b = blocks(t);
        assert_eq!(b.len(), 3, "{b:?}");
        assert_eq!(b[0].name, "def");
        assert_eq!(b[1].name, "#check");
    }

    #[test]
    fn continuation_lines_at_column_zero_stay_in_the_block() {
        let bs = blocks(FILE);
        let ind = bs.iter().find(|b| b.title == "T").unwrap();
        assert!(slice(FILE, ind.start, ind.end).ends_with("deriving Repr"));
        let t = "structure S where\n  x : Nat\nderiving Repr\n\nderiving instance BEq for S\n";
        let b = blocks(t);
        assert_eq!(b.len(), 2);
        check_covering(t, &b);
    }

    #[test]
    fn mutual_runs_to_its_end_and_blank_lines_stay_inside_blocks() {
        let t = "mutual\ninductive A where\n  | a (b : B)\n\ninductive B where\n  | b\nend\n\ntheorem x : True := by\n  trivial\n\n  -- note\n\n  rfl\n\ndef y := 1\n";
        let b = blocks(t);
        check_covering(t, &b);
        assert_eq!(b.len(), 3, "{b:?}");
        assert!(slice(t, b[0].start, b[0].end).ends_with("end"));
        assert!(slice(t, b[1].start, b[1].end).ends_with("rfl"));
    }

    #[test]
    fn comments_and_strings_hide_commands() {
        let t = "def s := \"a\ntheorem not_a_command\"\n\n/- start\ntheorem hidden\n-/\n\ndef t := 1 /- inline -/\n";
        let b = blocks(t);
        check_covering(t, &b);
        let kinds: Vec<Kind> = b.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, vec![Kind::Formal, Kind::Comment, Kind::Formal], "{b:?}");
        assert_eq!(slice(t, b[2].start, b[2].end), "def t := 1 /- inline -/", "a trailing comment stays");
        assert_eq!(b[2].commands[0].end - b[2].commands[0].start, "def t := 1".len(), "but is not part of the command");
    }

    #[test]
    fn stray_and_unusual_input_is_never_dropped() {
        for t in ["", "\n\n", "hello", "  indented stray\ndef x := 1\n", "/-! open", "/-- doc", "def", "@[", "theorem t : True := by\n  sorry -- done", "\u{feff}def x := 1"] {
            let bs = blocks(t);
            check_covering(t, &bs);
        }
        assert_eq!(blocks("hello")[0].kind, Kind::Comment);
    }

    #[test]
    fn headings_in_markdown() {
        let h = headings("# A\n\ntext\n```\n# not\n```\n## B ##\n#no\n");
        assert_eq!(h.iter().map(|x| (x.0, x.2.as_str())).collect::<Vec<_>>(), vec![(1, "A"), (2, "B")]);
        let t = "/-!\n# Title\n\nand body\n-/\n";
        let b = blocks(t);
        assert_eq!((b[0].kind, b[0].level), (Kind::Text, Some(1)));
    }

    fn names(text: &str, cs: &[Command]) -> Vec<(String, String)> {
        cs.iter().map(|c| (c.name.clone(), slice(text, c.start, c.end))).collect()
    }

    #[test]
    fn a_proof_is_split_into_header_and_tactics() {
        let t = "theorem foo (a b : Nat)\n    (h : a = b) : b = a := by\n  intro_x\n  have h2 : a = a := by\n    rfl\n  · simp\n    omega\n  exact h.symm\n";
        let cs = commands(t);
        let got = names(t, &cs);
        assert_eq!(got[0], ("theorem".into(), "theorem foo (a b : Nat)\n    (h : a = b) : b = a := by".into()));
        assert_eq!(got[1].1, "intro_x");
        assert_eq!(got[2], ("have".into(), "have h2 : a = a := by\n    rfl".into()));
        assert_eq!(got[3], ("·".into(), "· simp\n    omega".into()));
        assert_eq!(got[4], ("exact".into(), "exact h.symm".into()));
        assert_eq!(got.len(), 5);
    }

    #[test]
    fn same_line_by_and_comments_between_tactics() {
        let t = "example : True := by trivial\n\nexample (p : Prop) : p → p := by\n  -- start\n  intro h -- keep\n  /- block -/\n  exact h\n  <;> rfl\n";
        let cs = commands(t);
        let got = names(t, &cs);
        assert_eq!(got[0], ("example".into(), "example : True := by".into()));
        assert_eq!(got[1], ("trivial".into(), "trivial".into()));
        assert_eq!(got[2].0, "example");
        assert_eq!(got[3], ("intro".into(), "intro h".into()));
        assert_eq!(got[4], ("exact".into(), "exact h\n  <;> rfl".into()));
        assert_eq!(got.len(), 5);
    }

    #[test]
    fn declarations_without_by_are_one_command() {
        let t = "def f (n : Nat) : Nat :=\n  match n with\n  | 0 => 1\n  | n + 1 => f n\n\ninstance : Foo Bar where\n  x := by simp\n\nnamespace A\n";
        let cs = commands(t);
        let got = names(t, &cs);
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(got[0].0, "def");
        assert!(got[0].1.ends_with("f n"));
        assert_eq!(got[1].0, "instance");
        assert_eq!(got[2], ("namespace".into(), "namespace A".into()));
        // a `by` inside brackets is not the proof
        let t = "theorem x (h : 1 = 1 := by rfl) : True := by\n  trivial\n";
        let got = names(t, &commands(t));
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].0, "trivial");
    }

    #[test]
    fn match_alternatives_and_termination_are_handled() {
        let t = "theorem t (n : Nat) : n = n := by\n  induction n with\n  | zero => rfl\n  | succ k ih => rfl\n  done\n\ndef g (n : Nat) : Nat := by\n  exact n\ntermination_by n\n";
        let got = names(t, &commands(t));
        assert_eq!(got[1].0, "induction");
        assert!(got[1].1.ends_with("| succ k ih => rfl"));
        assert_eq!(got[2].0, "done");
        let g: Vec<&str> = got.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(g, vec!["theorem", "induction", "done", "def", "exact", "termination_by"]);
    }

    #[test]
    fn real_dsrv_files_split_cleanly() {
        let Ok(rd) = std::fs::read_dir("/tmp/dsrv-lean-copy/DSRV") else { return };
        let mut theorems = 0;
        let mut tactics = 0;
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "lean") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            let bs = blocks(&text);
            check_covering(&text, &bs);
            for b in &bs {
                if b.kind == Kind::Formal && b.commands.is_empty() {
                    panic!("{p:?}: a formal block without commands: {b:?}");
                }
                let mut at = b.code.0;
                for c in &b.commands {
                    assert!(c.start >= at && c.end > c.start && c.end <= b.end, "{p:?}: {c:?} in {b:?}");
                    at = c.end;
                }
                if matches!(b.name.as_str(), "theorem" | "lemma") {
                    theorems += 1;
                    tactics += b.commands.len().saturating_sub(1);
                }
            }
        }
        assert!(theorems > 50, "{theorems}");
        assert!(tactics > theorems, "{tactics} tactics in {theorems} theorems");
    }
}
