// SPDX-License-Identifier: GPL-3.0-only

//! A theory as the literate view lays it out: blocks of source, each a run of
//! commands (design C4, P1). Document commands (`section`, `text`, …) become
//! prose; a theorem and its proof, or a definition, one formal block. The
//! prover's command spans are used when it has parsed the text; until then a
//! lexical scan stands in, which never drops or reorders source. Every block
//! keeps its exact source span, so edits go back to the `.thy` buffer.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Header,
    Heading(u8),
    Text,
    Formal,
    Comment,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Finished,
    Warned,
    Unprocessed,
    Running,
    Failed,
}

impl Status {
    pub fn parse(s: &str) -> Self {
        match s {
            "finished" => Status::Finished,
            "warned" => Status::Warned,
            "running" => Status::Running,
            "failed" | "canceled" => Status::Failed,
            _ => Status::Unprocessed,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub status: Status,
}

impl Block {
    /// Whether the block's source starts with `prefix` (in `text`, the theory)
    pub fn source_start_is(&self, text: &str, prefix: &str) -> bool {
        text.chars().skip(self.start).take(prefix.chars().count()).collect::<String>() == prefix
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: Kind,
    /// Source span in code points, without surrounding blank space
    pub start: usize,
    pub end: usize,
    /// The span of a document command's text, inside its delimiters
    pub body: Option<(usize, usize)>,
    pub name: String,
    pub status: Status,
    pub commands: Vec<Command>,
}

const DOCUMENT: &[(&str, Kind)] = &[
    ("chapter", Kind::Heading(1)),
    ("section", Kind::Heading(2)),
    ("subsection", Kind::Heading(3)),
    ("subsubsection", Kind::Heading(4)),
    ("paragraph", Kind::Heading(5)),
    ("subparagraph", Kind::Heading(6)),
    ("text", Kind::Text),
    ("txt", Kind::Text),
    ("text_raw", Kind::Text),
];

/// Commands that start a block when the prover hasn't said what kind they are
const THEORY_LEVEL: &[&str] = &[
    "theory", "end", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph",
    "text", "txt", "text_raw", "lemma", "theorem", "corollary", "proposition", "schematic_goal",
    "definition", "abbreviation", "fun", "function", "primrec", "datatype", "codatatype",
    "type_synonym", "typedef", "record", "locale", "context", "interpretation",
    "global_interpretation", "sublocale", "instantiation", "instance", "class", "consts",
    "axiomatization", "notation", "no_notation", "syntax", "no_syntax", "translations", "declare",
    "lemmas", "named_theorems", "method", "ML", "ML_file", "setup", "bundle", "unbundle",
    "inductive", "inductive_set", "coinductive", "termination", "nonterminal", "type_notation",
    "hide_const", "hide_fact", "hide_type", "print_theorems", "value", "term", "thm", "typ",
    "find_theorems", "alphabet", "dataspace", "zstore", "zschema", "zoperation", "zmachine",
    "interpretation", "overloading", "specification", "lift_definition", "setup_lifting",
    "experiment", "notepad", "subsection", "adhoc_overloading", "no_adhoc_overloading",
    "simproc_setup", "attribute_setup", "method_setup", "ML_val", "SML_file", "code_datatype",
    "export_code", "quickcheck_params", "nitpick_params", "default_sort", "lemmas", "corollary",
];

const PROOF: &[&str] = &[
    "proof", "qed", "by", "apply", "apply_end", "done", "sorry", "oops", "have", "show", "then",
    "from", "with", "using", "unfolding", "next", "fix", "assume", "presume", "obtain", "note",
    "moreover", "ultimately", "finally", "also", "case", "hence", "thus", "supply", "subgoal",
    "defer", "prefer", "back", "let", "define", "consider", "guess", "include", "including",
    "{", "}", ".", "..", "where", "if", "for", "is", "and", "interpret", "write",
];

fn theory_level(name: &str, kinds: &HashMap<String, String>) -> bool {
    if name == "theory" || name == "end" {
        return true;
    }
    match kinds.get(name) {
        Some(kind) => kind.starts_with("thy_") || kind.starts_with("document_"),
        None => THEORY_LEVEL.contains(&name),
    }
}

/// A scanner state for the lexical fallback: what the text is inside of
struct Scan<'a> {
    chars: &'a [char],
}

impl<'a> Scan<'a> {
    fn starts(&self, i: usize, s: &str) -> bool {
        let mut j = i;
        for c in s.chars() {
            if self.chars.get(j) != Some(&c) {
                return false;
            }
            j += 1;
        }
        true
    }

    /// The end of a comment, cartouche, string or verbatim text starting at `i`
    fn skip(&self, i: usize) -> Option<usize> {
        let n = self.chars.len();
        if self.starts(i, "(*") {
            let mut depth = 0;
            let mut j = i;
            while j < n {
                if self.starts(j, "(*") {
                    depth += 1;
                    j += 2;
                } else if self.starts(j, "*)") {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        return Some(j);
                    }
                } else {
                    j += 1;
                }
            }
            return Some(n);
        }
        if self.starts(i, "\\<open>") || self.chars[i] == '‹' {
            return Some(self.cartouche_end(i));
        }
        if self.chars[i] == '"' || self.chars[i] == '`' {
            let q = self.chars[i];
            let mut j = i + 1;
            while j < n && self.chars[j] != q {
                if self.chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            return Some((j + 1).min(n));
        }
        if self.starts(i, "{*") {
            let mut j = i + 2;
            while j < n && !self.starts(j, "*}") {
                j += 1;
            }
            return Some((j + 2).min(n));
        }
        None
    }

    fn cartouche_end(&self, i: usize) -> usize {
        let n = self.chars.len();
        let mut depth = 0;
        let mut j = i;
        while j < n {
            if self.starts(j, "\\<open>") {
                depth += 1;
                j += 7;
            } else if self.chars[j] == '‹' {
                depth += 1;
                j += 1;
            } else if self.starts(j, "\\<close>") {
                depth -= 1;
                j += 8;
                if depth == 0 {
                    return j;
                }
            } else if self.chars[j] == '›' {
                depth -= 1;
                j += 1;
                if depth == 0 {
                    return j;
                }
            } else {
                j += 1;
            }
        }
        n
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// Commands found lexically: keyword starts outside comments, cartouches and strings
pub fn lexical_commands(source: &str, kinds: &HashMap<String, String>) -> Vec<Command> {
    let chars: Vec<char> = source.chars().collect();
    let scan = Scan { chars: &chars };
    let n = chars.len();
    let known = |w: &str| kinds.contains_key(w) || THEORY_LEVEL.contains(&w) || PROOF.contains(&w);
    // Command starts, and for each the end of its last token that is not a comment
    let mut starts: Vec<(usize, String)> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let mut last_end = 0;
    let mut i = 0;
    let mut line_start = true;
    while i < n {
        let c = chars[i];
        if c == '\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if c == ' ' || c == '\t' {
            i += 1;
            continue;
        }
        if let Some(j) = scan.skip(i) {
            if !scan.starts(i, "(*") {
                last_end = j;
            }
            line_start = false;
            i = j;
            continue;
        }
        if is_word(c) && (i == 0 || !is_word(chars[i - 1])) {
            let mut j = i;
            while j < n && is_word(chars[j]) {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            if known(&word) && line_start {
                if !starts.is_empty() {
                    ends.push(last_end);
                }
                starts.push((i, word));
            }
            last_end = j;
            line_start = false;
            i = j;
            continue;
        }
        last_end = i + 1;
        line_start = false;
        i += 1;
    }
    if !starts.is_empty() {
        ends.push(last_end);
    }
    starts
        .into_iter()
        .zip(ends)
        .map(|((s, name), e)| Command { start: s, end: e.max(s), name, status: Status::Unprocessed })
        .collect()
}

/// The body of a document command: its cartouche or string, without delimiters
fn document_body(chars: &[char], cmd: &Command) -> Option<(usize, usize)> {
    let scan = Scan { chars };
    let mut i = cmd.start + cmd.name.chars().count();
    while i < cmd.end {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '%' {
            // a tag such as %invisible
            i += 1;
            while i < cmd.end && (is_word(chars[i]) || chars[i] == '.') {
                i += 1;
            }
        } else if scan.starts(i, "(*") {
            i = scan.skip(i)?;
        } else if scan.starts(i, "\\<open>") {
            let end = scan.cartouche_end(i);
            return Some((i + 7, end.saturating_sub(8).max(i + 7)));
        } else if c == '‹' {
            let end = scan.cartouche_end(i);
            return Some((i + 1, end.saturating_sub(1).max(i + 1)));
        } else if c == '"' {
            let end = scan.skip(i)?;
            return Some((i + 1, end.saturating_sub(1).max(i + 1)));
        } else {
            return None;
        }
    }
    None
}

/// The theory's blocks. `commands` are the prover's, or empty for a lexical scan
/// The prover's commands, with text it has not seen yet (just typed or inserted) split where a
/// theory-level command starts a line in it: between its commands, or inside a command whose span
/// an edit grew. What the prover has not seen is not checked yet
fn refine(source: &str, commands: &[Command], kinds: &HashMap<String, String>) -> Vec<Command> {
    let chars: Vec<char> = source.chars().collect();
    let lex_in = |from: usize, to: usize| -> Vec<Command> {
        let text: String = chars[from.min(to)..to].iter().collect();
        lexical_commands(&text, kinds)
            .into_iter()
            .filter(|c| theory_level(&c.name, kinds))
            .map(|c| Command { start: c.start + from, end: c.end + from, name: c.name, status: Status::Unprocessed })
            .collect()
    };
    let mut out: Vec<Command> = Vec::with_capacity(commands.len());
    let mut covered = 0;
    for cmd in commands {
        if cmd.start > covered {
            out.extend(lex_in(covered, cmd.start.min(chars.len())));
        }
        let end = cmd.end.min(chars.len());
        let start = cmd.start.min(end);
        let lexed = lex_in(start, end);
        // Does the span still start with its command? (an insert at its start moves it)
        let name_at_start = chars[start..end].iter().take_while(|c| is_word(**c)).collect::<String>() == cmd.name;
        if !name_at_start && !lexed.is_empty() {
            out.extend(lexed);
        } else {
            let inside: Vec<Command> = lexed.into_iter().filter(|c| c.start > start).collect();
            match inside.first() {
                Some(first) => {
                    let mut head = cmd.clone();
                    head.end = chars[..first.start].iter().rposition(|c| !c.is_whitespace()).map_or(start, |i| (i + 1).max(start));
                    out.push(head);
                    out.extend(inside);
                }
                None => out.push(cmd.clone()),
            }
        }
        covered = covered.max(cmd.end);
    }
    if covered < chars.len() {
        out.extend(lex_in(covered, chars.len()));
    }
    out
}

pub fn blocks(source: &str, commands: &[Command], kinds: &HashMap<String, String>) -> Vec<Block> {
    let chars: Vec<char> = source.chars().collect();
    let commands = if commands.is_empty() { lexical_commands(source, kinds) } else { refine(source, commands, kinds) };
    let mut out: Vec<Block> = Vec::new();
    let mut covered = 0;
    let push_gap = |out: &mut Vec<Block>, from: usize, to: usize| {
        // Text between commands: comments and stray source, kept visible
        let mut s = from;
        let mut e = to;
        while s < e && chars[s].is_whitespace() {
            s += 1;
        }
        while e > s && chars[e - 1].is_whitespace() {
            e -= 1;
        }
        if e > s {
            out.push(Block {
                kind: Kind::Comment,
                start: s,
                end: e,
                body: None,
                name: String::new(),
                status: Status::Finished,
                commands: Vec::new(),
            });
        }
    };
    for cmd in commands {
        if cmd.start < covered {
            continue;
        }
        let level = theory_level(&cmd.name, kinds);
        let attach = !level && out.last().is_some_and(|b| matches!(b.kind, Kind::Formal | Kind::Header));
        if !attach {
            push_gap(&mut out, covered, cmd.start);
        }
        if attach {
            let last = out.last_mut().unwrap();
            last.end = cmd.end;
            last.status = last.status.max(cmd.status);
            last.commands.push(cmd.clone());
        } else {
            let kind = if cmd.name == "theory" {
                Kind::Header
            } else if cmd.name == "end" {
                Kind::End
            } else if let Some((_, k)) = DOCUMENT.iter().find(|(n, _)| *n == cmd.name) {
                *k
            } else {
                Kind::Formal
            };
            let body = if matches!(kind, Kind::Heading(_) | Kind::Text) { document_body(&chars, &cmd) } else { None };
            out.push(Block {
                kind,
                start: cmd.start,
                end: cmd.end,
                body,
                name: cmd.name.clone(),
                status: cmd.status,
                commands: vec![cmd.clone()],
            });
        }
        covered = cmd.end;
    }
    push_gap(&mut out, covered, chars.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const THY: &str = "theory Demo\n  imports Main\nbegin\n\nsection \\<open>Intro\\<close>\n\ntext \\<open>Some $x^2$ text with \\<open>nested\\<close>.\\<close>\n\n(* a comment *)\n\nlemma foo: \"x = x\"\n  apply simp\n  done\n\ndefinition f where \"f = 0\"\n\nend\n";

    #[test]
    fn lexical_blocks_cover_the_theory() {
        let kinds = HashMap::new();
        let bs = blocks(THY, &[], &kinds);
        let kinds_seen: Vec<Kind> = bs.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds_seen,
            vec![Kind::Header, Kind::Heading(2), Kind::Text, Kind::Comment, Kind::Formal, Kind::Formal, Kind::End]
        );
        let chars: Vec<char> = THY.chars().collect();
        let text = &bs[2];
        let (s, e) = text.body.unwrap();
        assert_eq!(chars[s..e].iter().collect::<String>(), "Some $x^2$ text with \\<open>nested\\<close>.");
        let lemma = &bs[4];
        assert!(chars[lemma.start..lemma.end].iter().collect::<String>().ends_with("done"));
    }

    #[test]
    fn inserted_text_is_split_before_the_prover_sees_it() {
        let kinds = HashMap::new();
        let old = blocks(THY, &[], &kinds);
        let commands: Vec<Command> = lexical_commands(THY, &kinds);
        let at = old[1].end;
        let mut text = THY.to_string();
        let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
        text.insert_str(byte, "\n\nsection \\<open>New\\<close>");
        // the prover's spans as they were, the later ones moved past the insert
        let n = "\n\nsection \\<open>New\\<close>".chars().count();
        let moved: Vec<Command> = commands.iter().map(|c| if c.start >= at { Command { start: c.start + n, end: c.end + n, ..c.clone() } } else { c.clone() }).collect();
        let bs = blocks(&text, &moved, &kinds);
        assert!(bs.iter().any(|b| b.kind == Kind::Heading(2) && b.body.is_some_and(|(s, e)| text.chars().skip(s).take(e - s).collect::<String>() == "New")), "{bs:?}");
    }

    #[test]
    fn keywords_inside_strings_do_not_split() {
        let thy = "lemma \"a\nlemma b\" by simp\n";
        let bs = blocks(thy, &[], &HashMap::new());
        assert_eq!(bs.len(), 1);
    }
}
