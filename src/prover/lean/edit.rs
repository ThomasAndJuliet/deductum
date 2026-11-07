// SPDX-License-Identifier: GPL-3.0-only

//! What the editor needs to know about writing Lean (design 7.3), as `editor.rs` and `view.rs`
//! know it for Isar: where a new line is indented to, which construct the goal beside the code is
//! for, which brackets pair, where a `claude`/`codex`/`@name` line asks for a proof, what the goal
//! suggests to type next, and what the insert palette offers.
//!
//! Lean is layout sensitive: a tactic block is the lines at one column after `by`, a line more
//! indented continues the tactic above it, and `·`, `case … =>` and `calc` steps open blocks of
//! their own. The functions here read that layout from the text alone, with no prover, and treat
//! comments and strings as the lexer does. Offsets are in code points.
//!
//! `claude` and `codex` lines work as in Isabelle: the keyword (and a model in brackets, or a
//! `@name`) ends a line or is followed by a hint, and the prover sees `sorry` of the same length
//! (design 9.4), which Lean accepts as a tactic and as a term. `claude` and `codex` may follow a
//! tactic prefix (`intro h; claude`). In Lean `@name` also writes an explicit application
//! (`exact @foo _ _`), so it counts as a keyword line only where a tactic can start, and
//! only with at least four characters after the `@`, so that `sorry` fits in its length.

/// A suggestion to put at the caret: what to replace (code points before the caret), what to
/// insert, and how to show it. As `editor::Completion`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub replace: usize,
    pub text: String,
    pub label: String,
}

// ---------------------------------------------------------------- lines and layout

/// The text without a trailing `--` comment (outside strings), and without trailing space
fn code_of(line: &str) -> &str {
    let mut in_str = false;
    let mut escaped = false;
    let mut prev_dash = None;
    for (i, c) in line.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '-' => {
                if let Some(p) = prev_dash {
                    return line[..p].trim_end();
                }
                prev_dash = Some(i);
                continue;
            }
            _ => {}
        }
        prev_dash = None;
    }
    line.trim_end()
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// The first word of a code line (`intro`, `theorem`, `·`, `_`)
fn first_word(code: &str) -> &str {
    let t = code.trim_start();
    let end = t.char_indices().find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '_' | '\'' | '.' | '!' | '?'))).map_or(t.len(), |(i, _)| i);
    if end == 0 { t.chars().next().map_or("", |c| &t[..c.len_utf8()]) } else { &t[..end] }
}

fn is_bullet(code: &str) -> bool {
    let t = code.trim_start();
    t.starts_with("· ") || t == "·" || t.starts_with(". ") || t == "."
}

/// Whether the code ends in a token that opens a block of lines below it
fn ends_with_opener(code: &str) -> bool {
    let t = code.trim_end();
    [" by", ":= by", "where", " do", "=>", ":=", " from", " then", " else", " calc"].iter().any(|s| t.ends_with(s))
        || ["by", "where", "do", "=>", ":=", "from", "then", "else", "calc"].contains(&t)
}

/// How many brackets the code leaves open
fn open_brackets(code: &str) -> i32 {
    let mut depth = 0;
    let mut in_str = false;
    for c in code.chars() {
        match c {
            '"' => in_str = !in_str,
            '(' | '[' | '{' | '⟨' | '⦃' | '⟦' if !in_str => depth += 1,
            ')' | ']' | '}' | '⟩' | '⦄' | '⟧' if !in_str => depth -= 1,
            _ => {}
        }
    }
    depth
}

const DECLARATIONS: &[&str] = &["theorem", "lemma", "def", "example", "instance", "abbrev", "inductive", "structure", "class", "axiom", "opaque", "macro", "syntax", "notation"];

/// The indentation for a new line after the text before the caret, the Lean way. The last
/// line with code decides:
///
/// * a line ending in `by`, `:= by`, `where`, `do`, `=>`, `:=`, `from`, `then`, `else` opens a block:
///   two columns in from the line's content (after a `·`, from the text after it);
/// * a line of `·` leaves the next tactic aligned with the text after the bullet;
/// * a `calc` line puts its steps (`_ = …`) two columns in;
/// * brackets left open, or a trailing `,` or connective (`→`, `∧`, `=` …), hang two columns in;
/// * a declaration header that has not reached its `:=` hangs four columns in, as Mathlib writes it;
/// * anything else continues at the line's own indentation.
pub fn next_indent(before: &str) -> String {
    let line = before.lines().rev().map(code_of).find(|l| !l.trim().is_empty()).unwrap_or("");
    let indent = indent_of(line);
    let code = line.trim_start();
    if code.is_empty() {
        return String::new();
    }
    // (the text after a bullet is where this line's content starts)
    let bullet = is_bullet(code);
    let content = if bullet { indent + 2 } else { indent };
    let rest = if bullet { code[code.char_indices().nth(1).map_or(code.len(), |(i, _)| i)..].trim_start() } else { code };
    let first = first_word(rest);
    // (the line's own indentation characters are kept: tabs stay tabs)
    let own: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let spaces = |n: usize| if n >= indent { format!("{own}{}", " ".repeat(n - indent)) } else { " ".repeat(n) };
    if open_brackets(code) > 0 || ends_with_opener(rest) {
        return spaces(content + 2);
    }
    if first == "calc" {
        return spaces(content + 2);
    }
    if bullet {
        return spaces(content);
    }
    let modifiers = ["private", "protected", "noncomputable", "partial", "unsafe", "nonrec", "@[simp]"];
    let decl_word = if modifiers.contains(&first) { first_word(rest.trim_start()[first.len()..].trim_start()) } else { first };
    if DECLARATIONS.contains(&decl_word) && indent == 0 && !rest.contains(":=") && !rest.ends_with("where") {
        let needs_where = matches!(decl_word, "structure" | "inductive" | "class") && !rest.contains(':');
        return spaces(if needs_where { 2 } else { 4 });
    }
    const CONNECTIVES: &[&str] = &[",", "→", "↔", "∧", "∨", "∀", "∃", "+", "*", "=", "<|", "|>", "<;>", "$", "∘", "∈", "->"];
    if CONNECTIVES.iter().any(|c| rest.ends_with(c)) {
        return spaces(content + 2);
    }
    spaces(indent)
}

/// Which lines are code, as (char start, char end, indent, code), skipping blank lines, comments
/// that fill a line and the lines of block comments
struct Line {
    start: usize,
    /// The end of the code, without a trailing comment or space
    end: usize,
    indent: usize,
    code: String,
}

fn code_lines(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut depth = 0u32;
    for raw in text.split('\n') {
        let len = raw.chars().count();
        let c: Vec<char> = raw.chars().collect();
        // block comments: lines in them or opened and closed on them hold no code, unless text
        // other than the comment is on the line
        let mut code = String::new();
        let mut i = 0;
        let mut in_str = false;
        while i < c.len() {
            if depth > 0 {
                if i + 1 < c.len() && c[i] == '/' && c[i + 1] == '-' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < c.len() && c[i] == '-' && c[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        code.push(' ');
                    }
                } else {
                    i += 1;
                }
                continue;
            }
            if in_str {
                code.push(c[i]);
                if c[i] == '\\' && i + 1 < c.len() {
                    code.push(c[i + 1]);
                    i += 1;
                } else if c[i] == '"' {
                    in_str = false;
                }
                i += 1;
                continue;
            }
            if i + 1 < c.len() && c[i] == '/' && c[i + 1] == '-' {
                depth = 1;
                i += 2;
                continue;
            }
            if i + 1 < c.len() && c[i] == '-' && c[i + 1] == '-' {
                break;
            }
            if c[i] == '"' {
                in_str = true;
            }
            code.push(c[i]);
            i += 1;
        }
        // (keep the original columns: a comment replaced by nothing at the line's start still counts its indent)
        let trimmed = code.trim_end().to_string();
        if !trimmed.trim().is_empty() {
            let first_col = c.iter().position(|ch| !ch.is_whitespace()).unwrap_or(0);
            let lead = trimmed.chars().take_while(|ch| ch.is_whitespace()).count();
            let col = if lead == 0 { first_col } else { lead.max(first_col) };
            let code_text = trimmed.trim_start().to_string();
            let end_col = col + code_text.chars().count();
            out.push(Line { start: at + col, end: at + end_col.min(len), indent: col, code: code_text });
        }
        at += len + 1;
    }
    out
}

/// The source range (code points) of the tactic block the goal at `offset` is for.
///
/// Inside a proof (after `:= by`), the caret's line is a tactic, or a continuation of one (a line
/// indented below a line that does not open a block). The range is that tactic with its lines:
/// for `have h : _ := by` the `have` with its whole block, for `· …` the focused block, for
/// `case … =>` and `| ctor =>` the alternative, for a `calc` step the step; for any other tactic,
/// the tactic and its continuation lines. A block longer than twelve lines is reduced to its
/// first line. In the declaration's header, up to the `by`, the range is the header. The caret may
/// be on a blank line below the tactic.
pub fn construct_at(text: &str, offset: usize) -> Option<(usize, usize)> {
    let lines = code_lines(text);
    // the line of the caret, or the last line with code before it
    let cl = lines.iter().rposition(|l| l.start <= offset)?;
    // the declaration: the nearest line at column 0, and where its proof starts
    let d = (0..=cl).rev().find(|&i| lines[i].indent == 0)?;
    let region_end = (cl + 1..lines.len()).find(|&i| lines[i].indent == 0).unwrap_or(lines.len());
    let k = (d..region_end).find(|&i| ends_with_by(&lines[i].code));
    let Some(k) = k else {
        return Some((lines[d].start, lines[region_end - 1].end));
    };
    // (`:= by simp`: the tactic on the header's line belongs to the proof, but is the header here)
    if cl <= k {
        return Some((lines[d].start, lines[k].end));
    }
    let statement = statement_of(&lines, k, cl);
    let last = descendants_end(&lines, statement, region_end);
    let n_lines = last - statement + 1;
    let opener = |s: usize| {
        let w = first_word(&lines[s].code);
        matches!(w, "have" | "show" | "suffices" | "·" | "." | "case" | "next" | "calc" | "|" | "_") || ends_with_opener(&lines[s].code)
    };
    let end_line = if n_lines > 12 && opener(statement) { statement } else { last };
    Some((lines[statement].start, lines[end_line].end))
}

fn ends_with_by(code: &str) -> bool {
    let t = code.trim_end();
    t.ends_with(":= by") || t == "by" || t.ends_with(" by")
}

/// The index of the last line belonging to line `s`: those more indented below it
fn descendants_end(lines: &[Line], s: usize, region_end: usize) -> usize {
    let mut last = s;
    for i in s + 1..region_end {
        if lines[i].indent > lines[s].indent || (lines[i].indent == lines[s].indent && lines[i].code.starts_with("<;>")) {
            last = i;
        } else {
            break;
        }
    }
    last
}

/// The tactic line that the line `l` of a proof (below the header line `k`) belongs to
fn statement_of(lines: &[Line], k: usize, l: usize) -> usize {
    // the parent: the nearest line above with less indentation
    let parent = (k + 1..l).rev().find(|&i| lines[i].indent < lines[l].indent);
    let Some(p) = parent else { return l };
    let code = &lines[p].code;
    let bullet = is_bullet(code);
    let opens = if bullet {
        // (the first tactic of the bullet is on its line; later ones are indented to its text)
        lines[l].indent <= lines[p].indent + 2
    } else {
        ends_with_opener(code) || first_word(code) == "calc"
    };
    if opens { l } else { statement_of(lines, k, p) }
}

// ---------------------------------------------------------------- brackets

const PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('⟨', '⟩'), ('⦃', '⦄'), ('⟦', '⟧'), ('‹', '›')];

/// Every matching pair of brackets `( ) [ ] { } ⟨ ⟩ ⦃ ⦄ ⟦ ⟧ ‹ ›` outside comments, strings and
/// character literals, as (open, close) code point offsets ordered by the opening one. Unmatched
/// brackets are left out
pub fn paired_brackets(text: &str) -> Vec<(usize, usize)> {
    let c: Vec<char> = text.chars().collect();
    let n = c.len();
    let mut stack: Vec<(usize, char)> = Vec::new();
    let mut out = Vec::new();
    let mut i = 0;
    let mut depth = 0u32;
    while i < n {
        let two = |a: char, b: char| c[i] == a && c.get(i + 1) == Some(&b);
        if depth > 0 {
            if two('/', '-') {
                depth += 1;
                i += 2;
            } else if two('-', '/') {
                depth -= 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if two('/', '-') {
            depth = 1;
            i += 2;
            continue;
        }
        if two('-', '-') {
            while i < n && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c[i] == '"' {
            i += 1;
            while i < n && c[i] != '"' {
                i += if c[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        if c[i] == '\'' {
            // a character literal: 'x' or '\n'
            let end = if c.get(i + 1) == Some(&'\\') { (i + 3..(i + 12).min(n)).find(|&k| c[k] == '\'') } else if c.get(i + 2) == Some(&'\'') { Some(i + 2) } else { None };
            if let Some(e) = end {
                i = e + 1;
                continue;
            }
        }
        if let Some((_, close)) = PAIRS.iter().find(|(o, _)| *o == c[i]) {
            stack.push((i, *close));
        } else if PAIRS.iter().any(|(_, cl)| *cl == c[i]) {
            if let Some(&(o, want)) = stack.last()
                && want == c[i]
            {
                stack.pop();
                out.push((o, i));
            }
        }
        i += 1;
    }
    out.sort();
    out
}

/// The pair of brackets next to the caret: the one ending or starting just before it, or else
/// the one starting at it. As (open, close)
pub fn bracket_at(text: &str, caret: usize) -> Option<(usize, usize)> {
    let pairs = paired_brackets(text);
    let before = caret.checked_sub(1);
    pairs
        .iter()
        .copied()
        .find(|&(o, c)| Some(c) == before || Some(o) == before)
        .or_else(|| pairs.iter().copied().find(|&(o, c)| o == caret || c == caret))
}

// ---------------------------------------------------------------- claude / codex / @name lines

/// The keyword of a `claude`, `codex` or `@name` line, with a model in brackets if it has one, as
/// code points of the line: the keyword ends the line or is followed by a hint, is not inside a
/// string or a comment, and is preceded by a space or the start of the line. `claude` and `codex`
/// may follow any tactic prefix; `@name` only a place where a tactic starts
pub fn keyword_line(line: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    for i in 0..chars.len() {
        if chars[..i].contains(&'"') || (i > 0 && !matches!(chars[i - 1], ' ' | '\t')) {
            continue;
        }
        if chars[..i].windows(2).any(|w| w == ['-', '-']) {
            return None;
        }
        let rest: String = chars[i..].iter().collect();
        let word = ["claude", "codex"].into_iter().find(|kw| rest.starts_with(kw)).map(str::len);
        let name_len = rest
            .strip_prefix('@')
            .filter(|r| r.starts_with(|c: char| c.is_ascii_alphabetic()))
            .map(|r| 1 + r.chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-')).count())
            .filter(|len| *len >= 5);
        let kw_len = match (word, name_len) {
            (Some(w), _) => w,
            (None, Some(l)) if starts_tactic_prefix(&chars[..i]) => l,
            _ => continue,
        };
        let mut end = i + kw_len;
        if chars.get(end) == Some(&'[')
            && let Some(close) = (end + 1..chars.len()).find(|&k| chars[k] == ']')
            && chars[end + 1..close].iter().all(|c| c.is_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
        {
            end = close + 1;
        }
        let after: String = chars[end..].iter().collect();
        if (after.is_empty() || after.starts_with([' ', '\t'])) && !after.contains('"') {
            return Some((i, end));
        }
    }
    None
}

/// Whether text before a token on a line leaves a tactic able to start there
fn starts_tactic_prefix(before: &[char]) -> bool {
    let s: String = before.iter().collect();
    let t = s.trim_end();
    t.is_empty() || ["·", ".", ";", "<;>", "by", "=>", "|", "try", "<|>"].iter().any(|p| t.ends_with(p))
}

/// Where the keyword lines of a text are (code points of the text), skipping block comments
pub fn keyword_tokens(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut offset = 0;
    let mut depth = 0i32;
    for line in text.split('\n') {
        let start_depth = depth;
        let c: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i + 1 < c.len() {
            if c[i] == '/' && c[i + 1] == '-' {
                depth += 1;
                i += 2;
            } else if c[i] == '-' && c[i + 1] == '/' && depth > 0 {
                depth -= 1;
                i += 2;
            } else if c[i] == '-' && c[i + 1] == '-' && depth == 0 {
                break;
            } else {
                i += 1;
            }
        }
        // (a line that starts in, or has, a block comment is left alone)
        if start_depth == 0 && !line.contains("/-")
            && let Some((s, e)) = keyword_line(line)
        {
            out.push((offset + s, offset + e));
        }
        offset += c.len() + 1;
    }
    out
}

/// The text as the prover reads it: each keyword line, from its keyword to the end of the line,
/// is `sorry` and spaces of the same length. The keyword itself holds `sorry` (it is at least
/// five characters), so no offset changes
pub fn placeholder_text(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = c.clone();
    for (s, _) in keyword_tokens(text) {
        let end = (s..c.len()).find(|&k| c[k] == '\n').unwrap_or(c.len());
        let end = (s..end).rev().find(|&k| !c[k].is_whitespace()).map_or(s, |k| k + 1);
        for (k, slot) in out[s..end].iter_mut().enumerate() {
            *slot = "sorry".chars().nth(k).unwrap_or(' ');
        }
    }
    out.into_iter().collect()
}

// ---------------------------------------------------------------- goals

/// A goal as the infoview shows it, for suggestions from its shape
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Goal {
    /// `case` tag, if any
    pub case: Option<String>,
    /// Hypotheses `(name, type)`, a line `x y : Nat` giving two
    pub hyps: Vec<(String, String)>,
    pub target: String,
}

impl Goal {
    /// The first goal of `case tag`, `x : T`, `⊢ target` text, as Lean prints it. Types may run over
    /// indented continuation lines. `None` without a `⊢`
    pub fn parse(state: &str) -> Option<Goal> {
        let mut goal = Goal::default();
        let mut cur: Option<(Vec<String>, String)> = None;
        let mut target: Option<String> = None;
        let flush = |goal: &mut Goal, cur: &mut Option<(Vec<String>, String)>| {
            if let Some((names, ty)) = cur.take() {
                for n in names {
                    goal.hyps.push((n, ty.trim().to_string()));
                }
            }
        };
        for line in state.lines() {
            if line.trim().is_empty() {
                if target.is_some() {
                    break;
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("case ") {
                goal.case = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix('⊢') {
                flush(&mut goal, &mut cur);
                target = Some(rest.trim().to_string());
            } else if line.starts_with([' ', '\t']) {
                if let Some(t) = target.as_mut() {
                    t.push(' ');
                    t.push_str(line.trim());
                } else if let Some((_, ty)) = cur.as_mut() {
                    ty.push(' ');
                    ty.push_str(line.trim());
                }
            } else if let Some(at) = line.find(" : ") {
                flush(&mut goal, &mut cur);
                let names: Vec<String> = line[..at].split_whitespace().map(str::to_string).collect();
                cur = Some((names, line[at + 3..].to_string()));
            }
        }
        flush(&mut goal, &mut cur);
        goal.target = target?;
        Some(goal)
    }
}

/// How a proposition begins, by its outermost connective
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Forall,
    Exists,
    Iff,
    Imp,
    Or,
    And,
    Not,
    Eq,
    Ne,
    Order,
    True,
    False,
    Other,
}

/// The positions of `pats` at bracket depth 0 of `s`
fn top_level(s: &str, pats: &[&str]) -> Vec<usize> {
    let c: Vec<char> = s.chars().collect();
    let mut depth = 0i32;
    let mut out = Vec::new();
    for i in 0..c.len() {
        match c[i] {
            '(' | '[' | '{' | '⟨' | '⦃' | '⟦' => depth += 1,
            ')' | ']' | '}' | '⟩' | '⦄' | '⟧' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            let rest: String = c[i..].iter().take(3).collect();
            if pats.iter().any(|p| rest.starts_with(p)) {
                out.push(i);
            }
        }
    }
    out
}

fn shape_of(target: &str) -> Shape {
    let t = target.trim();
    if t.starts_with('∀') {
        return Shape::Forall;
    }
    if t.starts_with('∃') {
        return Shape::Exists;
    }
    if t == "True" {
        return Shape::True;
    }
    if t == "False" {
        return Shape::False;
    }
    let arrows = |t: &str| top_level(t, &["→", "->"]);
    if !top_level(t, &["↔"]).is_empty() {
        Shape::Iff
    } else if !arrows(t).is_empty() {
        Shape::Imp
    } else if !top_level(t, &["∨"]).is_empty() {
        Shape::Or
    } else if !top_level(t, &["∧"]).is_empty() {
        Shape::And
    } else if t.starts_with('¬') {
        Shape::Not
    } else if !top_level(t, &["≠"]).is_empty() {
        Shape::Ne
    } else if !top_level(t, &[" = "]).is_empty() {
        Shape::Eq
    } else if !top_level(t, &["≤", "≥", " < ", " > "]).is_empty() {
        Shape::Order
    } else {
        Shape::Other
    }
}

/// The names bound by a quantifier prefix `∀ x y : T,` / `∃ (a b : T) (c : U),` / `∀ x ∈ s,`
fn binder_names(target: &str) -> Vec<String> {
    let t = target.trim().chars().skip(1).collect::<String>();
    let comma = top_level(&t, &[","]).first().copied();
    let head: String = match comma {
        Some(k) => t.chars().take(k).collect(),
        None => t.clone(),
    };
    let mut names = Vec::new();
    let mut take = |group: &str| {
        let upto = group.find([':', '∈', '<', '>', '≤', '≥', '≠']).map_or(group.len(), |i| i);
        for w in group[..upto].split_whitespace() {
            let w = w.trim_matches(|c| matches!(c, '(' | ')' | '{' | '}' | '⦃' | '⦄'));
            if !w.is_empty() && w.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '\'')) {
                names.push(w.to_string());
            }
        }
    };
    if head.contains(['(', '{', '⦃']) {
        for part in head.split(['(', '{', '⦃']).filter(|p| !p.trim().is_empty()) {
            take(part.split([')', '}', '⦄']).next().unwrap_or(""));
        }
    } else {
        take(&head);
    }
    names
}

/// The text after a quantifier prefix
fn quantifier_body(target: &str) -> String {
    let t = target.trim();
    match top_level(t, &[","]).first() {
        Some(&k) => t.chars().skip(k + 1).collect::<String>().trim().to_string(),
        None => String::new(),
    }
}

fn hypothesis_names(count: usize) -> String {
    const SUB: [&str; 4] = ["h₁", "h₂", "h₃", "h₄"];
    if count <= 1 { "h".to_string() } else { SUB[..count.min(4)].join(" ") }
}

fn is_arithmetic(goal: &Goal, target: &str) -> bool {
    let numeric_ops = target.contains(['+', '-', '*', '/', '%']) || target.chars().any(|c| c.is_ascii_digit());
    let inexact = ["ℝ", "ℚ", "ℂ", "Real", "Rat", "Float"].iter().any(|t| target.contains(t) || goal.hyps.iter().any(|(_, ty)| ty.contains(t)));
    let exact = ["ℕ", "ℤ", "Nat", "Int", "Fin"].iter().any(|t| target.contains(t) || goal.hyps.iter().any(|(_, ty)| ty.contains(t)));
    numeric_ops && exact && !inexact
}

/// Tactics from the shape of the goal and its hypotheses, for the word being typed (at least
/// two letters): `intro` for `∀` and `→`, `constructor` or `refine ⟨?_, ?_⟩` for `∧` and `↔`,
/// `use`/`refine ⟨_, ?_⟩` for `∃`, `left`/`right` for `∨`, `rfl`, `simp` or `omega` for equations
/// (`omega` for integer arithmetic only), `obtain`/`rcases` for hypotheses of those shapes, and
/// `exact h` for a hypothesis that is the goal. As `editor::goal_completions` does for Isar
pub fn goal_completions(prefix: &str, replace: usize, goal: &Goal) -> Vec<Completion> {
    let mut steps: Vec<String> = Vec::new();
    let target = goal.target.trim();
    match shape_of(target) {
        Shape::Forall => {
            let names = binder_names(target);
            let body = quantifier_body(target);
            let hyps = top_level(&body, &["→", "->"]).len();
            let mut intro = if names.is_empty() { "x".to_string() } else { names.join(" ") };
            if hyps > 0 {
                intro.push(' ');
                intro.push_str(&hypothesis_names(hyps));
            }
            steps.push(format!("intro {intro}"));
        }
        Shape::Imp => steps.push(format!("intro {}", hypothesis_names(top_level(target, &["→", "->"]).len()))),
        Shape::Not | Shape::Ne => steps.push("intro h".into()),
        Shape::Iff => {
            steps.push("constructor".into());
            steps.push("refine ⟨fun h => ?_, fun h => ?_⟩".into());
        }
        Shape::And => {
            steps.push("constructor".into());
            steps.push("refine ⟨?_, ?_⟩".into());
        }
        Shape::Or => {
            steps.push("left".into());
            steps.push("right".into());
        }
        Shape::Exists => {
            steps.push("use _".into());
            steps.push("refine ⟨_, ?_⟩".into());
        }
        Shape::Eq => {
            steps.push("rfl".into());
            steps.push("simp".into());
            if is_arithmetic(goal, target) {
                steps.push("omega".into());
            }
        }
        Shape::Order => {
            steps.push("simp".into());
            if is_arithmetic(goal, target) {
                steps.push("omega".into());
            }
        }
        Shape::True => steps.push("trivial".into()),
        Shape::False | Shape::Other => {}
    }
    for (name, ty) in &goal.hyps {
        if ty.trim() == target && !target.is_empty() {
            steps.push(format!("exact {name}"));
        }
        match shape_of(ty) {
            Shape::Exists => {
                let names = binder_names(ty);
                let last = names.last().map_or("h".to_string(), |n| format!("h{n}"));
                let mut pat = if names.is_empty() { vec!["x".to_string()] } else { names };
                pat.push(last);
                steps.push(format!("obtain ⟨{}⟩ := {name}", pat.join(", ")));
            }
            Shape::And => {
                let count = top_level(ty, &["∧"]).len() + 1;
                let pat: Vec<String> = (1..=count).map(|i| format!("h{}", subscript(i))).collect();
                steps.push(format!("obtain ⟨{}⟩ := {name}", pat.join(", ")));
            }
            Shape::Iff => {
                steps.push(format!("obtain ⟨hmp, hmpr⟩ := {name}"));
                steps.push(format!("rw [{name}]"));
            }
            Shape::Or => steps.push(format!("rcases {name} with h₁ | h₂")),
            Shape::False => {
                steps.push(format!("exact {name}.elim"));
                steps.push("contradiction".into());
            }
            Shape::Imp => steps.push(format!("apply {name}")),
            Shape::Eq => {
                steps.push(format!("rw [{name}]"));
                let sides: Vec<&str> = ty.split(" = ").collect();
                if sides.len() == 2 && sides.iter().any(|s| !s.trim().is_empty() && s.trim().chars().all(|c| c.is_alphanumeric() || c == '_')) {
                    steps.push(format!("subst {name}"));
                }
            }
            _ => {}
        }
    }
    let mut seen = Vec::new();
    steps
        .into_iter()
        .filter(|text| {
            let kw = text.split_whitespace().next().unwrap_or("");
            prefix.chars().count() >= 2 && kw.starts_with(prefix) && !seen.contains(text) && {
                seen.push(text.clone());
                true
            }
        })
        .map(|text| Completion { replace, label: format!("{text}   (from the goal)"), text })
        .collect()
}

fn subscript(i: usize) -> char {
    char::from_u32('₀' as u32 + (i % 10) as u32).unwrap_or('₁')
}

// ---------------------------------------------------------------- insert palette

/// What the caret is in, for the insert palette and the insert bar above the goal
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A term: a declaration's type or body, a binder, `have h : _`, `exact _`, a bracket
    Term,
    /// Where a tactic starts: at a line's start in a `by` block, after `by`, `·`, `;`, `<;>` or
    /// a case's `=>`
    Tactic,
    /// A docstring or module doc, which are Markdown
    Doc,
}

/// Where the caret is, after `before` (a declaration's text up to it), read lexically as the
/// lexer reads Lean: comments (`--`, `/- … -/`, which nest), strings and brackets.
///
/// Inside a docstring or a module doc it is Markdown. Inside a string, a bracket or another
/// comment it is a term (symbols are what is typed there). Otherwise it is a tactic's start
/// when a `by` came before it outside brackets (the declaration's proof is a tactic block) and
/// nothing but space stands between the last thing that ends a tactic (a line break, `by`, `·`,
/// `;`, `<;>`, `=>`) and the caret; anything else, a declaration's header or body or a tactic's
/// argument, is a term. The caret just after a whole tactic on its line is a term too: a tactic
/// put there would join the word before it
pub fn place(before: &str) -> Place {
    let chars: Vec<char> = before.chars().collect();
    let n = chars.len();
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'' || c == '.' || c == '!' || c == '?';
    // (each open comment: whether it is a doc)
    let mut comments: Vec<bool> = Vec::new();
    let (mut in_str, mut line_comment) = (false, false);
    let mut brackets = 0usize;
    let mut tactics = false;
    // (where the text since the last tactic boundary starts)
    let mut segment = 0;
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if line_comment {
            if c == '\n' {
                line_comment = false;
                segment = i + 1;
            }
            i += 1;
            continue;
        }
        if !comments.is_empty() {
            if c == '-' && next == Some('/') {
                comments.pop();
                i += 2;
                segment = i;
                continue;
            }
            if c == '/' && next == Some('-') {
                comments.push(false);
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => in_str = true,
            '-' if next == Some('-') => {
                line_comment = true;
                i += 2;
                continue;
            }
            '/' if next == Some('-') => {
                let doc = matches!(chars.get(i + 2), Some('-' | '!')) && chars.get(i + 3) != Some(&'/');
                comments.push(doc);
                i += 2;
                continue;
            }
            '(' | '[' | '{' | '⟨' => brackets += 1,
            ')' | ']' | '}' | '⟩' => brackets = brackets.saturating_sub(1),
            '\n' | ';' | '·' if brackets == 0 => segment = i + 1,
            '<' if brackets == 0 && chars[i..].starts_with(&['<', ';', '>']) => {
                i += 3;
                segment = i;
                continue;
            }
            // (a case's `=>` begins its tactics; a `fun`'s or a `match` arm's in a term does not)
            '=' if brackets == 0 && next == Some('>') => {
                let arm: String = chars[segment.min(i)..i].iter().collect();
                let arm = arm.trim_start();
                if arm.starts_with('|') || arm.starts_with("case ") || arm.starts_with("next") {
                    segment = i + 2;
                }
                i += 2;
                continue;
            }
            'b' if brackets == 0 && next == Some('y') && (i == 0 || !word(chars[i - 1])) && chars.get(i + 2).is_none_or(|c| !word(*c)) => {
                tactics = true;
                i += 2;
                segment = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if let Some(doc) = comments.last() {
        return if *doc { Place::Doc } else { Place::Term };
    }
    if in_str || line_comment || brackets > 0 {
        return Place::Term;
    }
    if tactics && chars[segment.min(n)..].iter().all(|c| c.is_whitespace()) { Place::Tactic } else { Place::Term }
}


/// What can be inserted in a proof: a label, what goes before the selection (or the caret), what
/// goes after it. A newline in these is followed by the indentation of the line it is on: use
/// [`indented`]
pub const PROOF_TEMPLATES: &[(&str, &str, &str)] = &[
    ("have", "have h : ", " := by\n  "),
    ("show", "show ", ""),
    ("calc", "calc ", " = _ := by\n    sorry\n  _ = _ := by\n    sorry"),
    ("·", "· ", ""),
    ("intro", "intro ", ""),
    ("constructor", "constructor", ""),
    ("exact", "exact ", ""),
    ("apply", "apply ", ""),
    ("rw", "rw [", "]"),
    ("simp only", "simp only [", "]"),
    ("exact?", "exact?", ""),
    ("apply?", "apply?", ""),
    ("simp?", "simp?", ""),
    ("omega", "omega", ""),
    ("decide", "decide", ""),
    ("sorry", "sorry", ""),
    ("claude", "claude", ""),
];

/// What can be inserted in a docstring or module doc, which are Markdown
pub const PROSE_TEMPLATES: &[(&str, &str, &str)] = &[
    ("emphasis", "*", "*"),
    ("bold", "**", "**"),
    ("code", "`", "`"),
    ("maths", "$", "$"),
    ("display maths", "$$\n", "\n$$"),
    ("link", "[", "](https://)"),
    ("code block", "```lean\n", "\n```"),
    ("bullet item", "\n- ", ""),
    ("numbered item", "\n1. ", ""),
];

/// `text` with each newline followed by `indent`: a template put at a line indented so
pub fn indented(text: &str, indent: &str) -> String {
    text.replace('\n', &format!("\n{indent}"))
}

/// Whether a line of prose is an item of a Markdown list: the marker to put on the next item
/// (`- `, `* `, `3. ` becoming `4. `), and the line's indentation
pub fn list_item(line: &str) -> Option<(String, String)> {
    let indent: String = line.chars().take_while(|c| *c == ' ').collect();
    let rest = &line[indent.len()..];
    for bullet in ["- ", "* ", "+ "] {
        if rest.starts_with(bullet) {
            return Some((bullet.to_string(), indent));
        }
    }
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() && digits.len() < 9 && (rest[digits.len()..].starts_with(". ") || rest[digits.len()..].starts_with(") ")) {
        let sep = &rest[digits.len()..digits.len() + 2];
        return Some((format!("{}{sep}", digits.parse::<u64>().ok()? + 1), indent));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indentation_follows_tactic_blocks() {
        assert_eq!(next_indent("theorem foo : P := by"), "  ");
        assert_eq!(next_indent("theorem foo : P :="), "  ");
        assert_eq!(next_indent("  have h : x = y := by"), "    ");
        assert_eq!(next_indent("  intro h"), "  ");
        assert_eq!(next_indent("  simp -- by"), "  ");
        assert_eq!(next_indent("structure S where"), "  ");
        assert_eq!(next_indent("instance : Foo Bar where"), "  ");
        assert_eq!(next_indent("  | succ k ih =>"), "    ");
        assert_eq!(next_indent("  | inl h => exact h"), "  ");
        assert_eq!(next_indent("  fun x =>"), "    ");
        assert_eq!(next_indent("def f : IO Unit := do"), "  ");
        assert_eq!(next_indent("namespace Foo"), "");
        assert_eq!(next_indent("end Foo"), "");
        assert_eq!(next_indent(""), "");
        // blank lines and comments after the line do not matter
        assert_eq!(next_indent("  have h : P := by\n  -- note\n"), "    ");
        assert_eq!(next_indent("\tintro x\n"), "\t");
    }

    #[test]
    fn indentation_after_bullets_calc_and_hanging_lines() {
        assert_eq!(next_indent("  · simp"), "    ");
        assert_eq!(next_indent("  ·"), "    ");
        assert_eq!(next_indent("  · have h : P := by"), "      ");
        assert_eq!(next_indent("  calc a = b := by simp"), "    ");
        assert_eq!(next_indent("    _ = c := by simp"), "    ");
        assert_eq!(next_indent("  calc"), "    ");
        assert_eq!(next_indent("  rw [foo,"), "    ");
        assert_eq!(next_indent("  exact foo (bar"), "    ");
        assert_eq!(next_indent("  refine ⟨?_,"), "    ");
        assert_eq!(next_indent("theorem foo (a b : Nat)"), "    ");
        assert_eq!(next_indent("theorem foo (a b : Nat) :"), "    ");
        assert_eq!(next_indent("private theorem foo (a b : Nat)"), "    ");
        assert_eq!(next_indent("def x := 1"), "");
        assert_eq!(next_indent("  ∀ x, P x →"), "    ");
        assert_eq!(next_indent("inductive T"), "  ");
    }

    const PROOF: &str = "theorem foo (a b : Nat) : a = b := by\n  intro h\n  have h2 : a = a := by\n    rfl\n    simp\n  · simp\n    omega\n  calc a = a := rfl\n    _ = a := by\n      simp\n  exact h\n\ndef next := 1\n";

    fn text_of(range: Option<(usize, usize)>, text: &str) -> String {
        let (s, e) = range.expect("a construct");
        text.chars().skip(s).take(e - s).collect()
    }

    fn at(text: &str, needle: &str) -> usize {
        text[..text.find(needle).unwrap()].chars().count()
    }

    #[test]
    fn the_construct_is_the_block_the_goal_is_for() {
        let t = PROOF;
        // on a have: the have with its block
        assert_eq!(text_of(construct_at(t, at(t, "have h2") + 3), t), "have h2 : a = a := by\n    rfl\n    simp");
        // inside it: the tactic line
        assert_eq!(text_of(construct_at(t, at(t, "rfl") + 1), t), "rfl");
        assert_eq!(text_of(construct_at(t, at(t, "simp\n  ·")), t), "simp");
        // a bullet: the focus block
        assert_eq!(text_of(construct_at(t, at(t, "· simp") + 1), t), "· simp\n    omega");
        assert_eq!(text_of(construct_at(t, at(t, "omega")), t), "omega");
        // a plain tactic
        assert_eq!(text_of(construct_at(t, at(t, "intro h") + 2), t), "intro h");
        assert_eq!(text_of(construct_at(t, at(t, "exact h") + 7), t), "exact h");
    }

    #[test]
    fn calc_steps_and_headers() {
        let t = PROOF;
        assert_eq!(text_of(construct_at(t, at(t, "_ = a") + 1), t), "_ = a := by\n      simp");
        assert_eq!(text_of(construct_at(t, at(t, "simp\n  exact")), t), "simp");
        // the calc itself: the calc with its steps
        assert!(text_of(construct_at(t, at(t, "calc") + 1), t).starts_with("calc a = a := rfl\n    _ = a"));
        // the header
        assert_eq!(text_of(construct_at(t, at(t, "(a b") + 1), t), "theorem foo (a b : Nat) : a = b := by");
        // a term-mode declaration
        assert_eq!(text_of(construct_at(t, at(t, "def next") + 4), t), "def next := 1");
        // a caret on the blank line below the proof belongs to its last tactic
        assert_eq!(text_of(construct_at(t, at(t, "\n\ndef") + 1), t), "exact h");
        assert_eq!(construct_at("", 0), None);
        assert_eq!(construct_at("-- only a comment\n", 3), None);
    }

    #[test]
    fn continuation_lines_belong_to_their_tactic_and_comments_are_skipped() {
        let t = "theorem x : P := by\n  simp [foo,\n    bar] -- trailing\n  -- a comment line\n  /- block\n  comment -/\n  exact baz\n";
        assert_eq!(text_of(construct_at(t, at(t, "bar") + 1), t), "simp [foo,\n    bar]");
        assert_eq!(text_of(construct_at(t, at(t, "comment line")), t), "simp [foo,\n    bar]");
        assert_eq!(text_of(construct_at(t, at(t, "baz")), t), "exact baz");
        // a block of more than twelve lines shows its first line only
        let mut long = String::from("theorem y : P := by\n  have h : Q := by\n");
        for _ in 0..13 {
            long.push_str("    simp\n");
        }
        long.push_str("  exact h\n");
        assert_eq!(text_of(construct_at(&long, at(&long, "have")), &long), "have h : Q := by");
    }

    #[test]
    fn brackets_pair_across_kinds_and_skip_comments_and_strings() {
        let t = "f (a [b, ⟨c⟩]) -- )\n \"(\" '(' ⦃x⦄ {y} /- ) -/ (z";
        let pairs = paired_brackets(t);
        let chars: Vec<char> = t.chars().collect();
        let shown: Vec<String> = pairs.iter().map(|&(o, c)| format!("{}{}", chars[o], chars[c])).collect();
        assert_eq!(shown, vec!["()", "[]", "⟨⟩", "⦃⦄", "{}"]);
        assert_eq!(pairs[0], (2, 13));
        assert_eq!(bracket_at(t, 14), Some((2, 13)), "just after the closer");
        assert_eq!(bracket_at(t, 3), Some((2, 13)), "just after the opener");
        assert_eq!(bracket_at(t, 2), Some((2, 13)), "before the opener");
        assert_eq!(bracket_at(t, 0), None);
        // mismatched closers do not pair
        assert_eq!(paired_brackets("(]"), vec![]);
        assert_eq!(paired_brackets("‹h› ⟦a⟧"), vec![(0, 2), (4, 6)]);
    }

    #[test]
    fn keyword_lines_are_found_as_the_host_finds_them() {
        let text = "theorem t : \"claude\" := by\n  intro h; claude\n  codex[astra] use foo\n  claudette\n  claude -- not a comment word\n  -- claude\n  exact claude \"x\"\n";
        let words: Vec<String> = keyword_tokens(text).into_iter().map(|(s, e)| text.chars().skip(s).take(e - s).collect()).collect();
        assert_eq!(words, vec!["claude", "codex[astra]", "claude"]);
        assert_eq!(keyword_line("  claude"), Some((2, 8)));
        assert_eq!(keyword_line("  claude[opus] use lia"), Some((2, 14)));
        assert_eq!(keyword_line("  claudette"), None);
        assert_eq!(keyword_line("  -- claude"), None);
        assert_eq!(keyword_line("  simp -- claude"), None);
        assert_eq!(keyword_line("  exact foo claude"), Some((12, 18)));
    }

    #[test]
    fn at_names_only_where_a_tactic_starts() {
        assert_eq!(keyword_line("  @tock try this"), Some((2, 7)));
        assert_eq!(keyword_line("  · @tock"), Some((4, 9)));
        assert_eq!(keyword_line("  simp <;> @tock"), Some((11, 16)));
        assert_eq!(keyword_line("  exact @foo _ _"), None, "explicit application");
        assert_eq!(keyword_line("  exact @Nat.add_comm a b"), None);
        assert_eq!(keyword_line("  @ab"), None, "too short for sorry");
        assert_eq!(keyword_line("  @abcd"), Some((2, 7)));
    }

    #[test]
    fn comments_hide_keyword_lines() {
        let t = "/-- Ask claude\n  claude\n-/\ntheorem t : P := by\n  claude\n";
        let toks = keyword_tokens(t);
        assert_eq!(toks.len(), 1);
        assert_eq!(t.chars().skip(toks[0].0).take(6).collect::<String>(), "claude");
        assert!(toks[0].0 > 30);
    }

    #[test]
    fn the_prover_sees_sorry_of_the_same_length() {
        let t = "theorem t : P := by\n  intro h\n  claude[opus] use lia\n  codex\n";
        let p = placeholder_text(t);
        assert_eq!(p.chars().count(), t.chars().count());
        assert_eq!(p, "theorem t : P := by\n  intro h\n  sorry               \n  sorry\n");
        assert_eq!(placeholder_text("a\nαβγ claude\n"), "a\nαβγ sorry \n");
        assert_eq!(placeholder_text("no keywords"), "no keywords");
    }

    #[test]
    fn a_goal_parses_as_the_infoview_prints_it() {
        let g = Goal::parse("case succ\nn : Nat\na b : ℤ\nh : ∃ x, x > 0 ∧\n    x < 5\n⊢ n + 1 = n + 1\n\ncase zero\n⊢ False").unwrap();
        assert_eq!(g.case.as_deref(), Some("succ"));
        assert_eq!(g.hyps, vec![("n".into(), "Nat".into()), ("a".into(), "ℤ".into()), ("b".into(), "ℤ".into()), ("h".into(), "∃ x, x > 0 ∧ x < 5".into())]);
        assert_eq!(g.target, "n + 1 = n + 1");
        assert_eq!(Goal::parse("no goals"), None);
        assert_eq!(Goal::parse("⊢ p ∧\n  q").unwrap().target, "p ∧ q");
    }

    fn goal(hyps: &[(&str, &str)], target: &str) -> Goal {
        Goal { case: None, hyps: hyps.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), target: target.into() }
    }

    fn texts(prefix: &str, g: &Goal) -> Vec<String> {
        goal_completions(prefix, prefix.len(), g).into_iter().map(|c| c.text).collect()
    }

    #[test]
    fn the_goal_suggests_how_to_start() {
        assert_eq!(texts("in", &goal(&[], "∀ x y : Nat, x ≤ y → x < y + 1")), vec!["intro x y h"]);
        assert_eq!(texts("in", &goal(&[], "∀ (a : Nat) (b : Nat), a = b")), vec!["intro a b"]);
        assert_eq!(texts("in", &goal(&[], "∀ x ∈ s, p x")), vec!["intro x"]);
        assert_eq!(texts("in", &goal(&[], "p → q → r")), vec!["intro h₁ h₂"]);
        assert_eq!(texts("in", &goal(&[], "¬ p")), vec!["intro h"]);
        assert_eq!(texts("in", &goal(&[], "a ≠ b")), vec!["intro h"]);
        assert_eq!(texts("co", &goal(&[], "p ∧ q")), vec!["constructor"]);
        assert_eq!(texts("re", &goal(&[], "p ∧ q")), vec!["refine ⟨?_, ?_⟩"]);
        assert_eq!(texts("re", &goal(&[], "p ↔ q")), vec!["refine ⟨fun h => ?_, fun h => ?_⟩"]);
        assert_eq!(texts("us", &goal(&[], "∃ x, x > 0 ∧ x < 5")), vec!["use _"], "the ∧ is inside the ∃");
        assert_eq!(texts("re", &goal(&[], "∃ x, p x")), vec!["refine ⟨_, ?_⟩"]);
        assert_eq!(texts("le", &goal(&[], "p ∨ q")), vec!["left"]);
        assert_eq!(texts("tr", &goal(&[], "True")), vec!["trivial"]);
        // the head connective is the loosest one
        assert_eq!(texts("in", &goal(&[], "p ∧ q → r")), vec!["intro h"]);
        assert_eq!(texts("co", &goal(&[], "(p ∧ q) → r")), Vec::<String>::new());
    }

    #[test]
    fn equations_get_rfl_simp_and_omega_for_integers() {
        assert_eq!(texts("rf", &goal(&[], "a = a")), vec!["rfl"]);
        assert_eq!(texts("si", &goal(&[], "a = a")), vec!["simp"]);
        assert_eq!(texts("om", &goal(&[("n", "Nat")], "n + 1 = 1 + n")), vec!["omega"]);
        assert_eq!(texts("om", &goal(&[("n", "ℤ")], "n - 1 < n")), vec!["omega"]);
        assert_eq!(texts("om", &goal(&[("x", "ℝ")], "x + 1 = 1 + x")), Vec::<String>::new());
        assert_eq!(texts("om", &goal(&[], "a = b")), Vec::<String>::new(), "no sign of arithmetic");
    }

    #[test]
    fn hypotheses_suggest_how_to_take_them_apart() {
        let g = goal(&[("h", "∃ x, p x"), ("k", "a ∧ b ∧ c"), ("o", "a ∨ b"), ("e", "n = 3"), ("f", "False"), ("i", "p → q"), ("j", "q")], "q");
        assert_eq!(texts("ob", &g), vec!["obtain ⟨x, hx⟩ := h", "obtain ⟨h₁, h₂, h₃⟩ := k"]);
        assert_eq!(texts("rc", &g), vec!["rcases o with h₁ | h₂"]);
        assert_eq!(texts("ex", &g), vec!["exact f.elim", "exact j"]);
        assert_eq!(texts("rw", &g), vec!["rw [e]"]);
        assert_eq!(texts("su", &g), vec!["subst e"]);
        assert_eq!(texts("ap", &g), vec!["apply i"]);
        assert_eq!(texts("ob", &goal(&[("h", "∃ x y, p x y")], "q")), vec!["obtain ⟨x, y, hy⟩ := h"]);
        // short prefixes list nothing; labels say where the text comes from
        assert!(texts("o", &g).is_empty() && texts("", &g).is_empty());
        let c = goal_completions("ob", 2, &g);
        assert_eq!(c[0].replace, 2);
        assert!(c[0].label.ends_with("(from the goal)"));
    }

    #[test]
    fn palettes_wrap_a_selection() {
        let wrap = |t: &(&str, &str, &str), sel: &str| format!("{}{}{}", t.1, sel, t.2);
        let have = PROOF_TEMPLATES.iter().find(|t| t.0 == "have").unwrap();
        assert_eq!(wrap(have, "a = a"), "have h : a = a := by\n  ");
        let calc = PROOF_TEMPLATES.iter().find(|t| t.0 == "calc").unwrap();
        assert_eq!(wrap(calc, "a"), "calc a = _ := by\n    sorry\n  _ = _ := by\n    sorry");
        for label in ["have", "show", "calc", "·", "intro", "constructor", "exact?", "apply?", "simp?", "omega", "decide", "sorry", "claude"] {
            assert!(PROOF_TEMPLATES.iter().any(|t| t.0 == label), "{label}");
        }
        for label in ["emphasis", "bold", "code", "maths", "link", "bullet item"] {
            assert!(PROSE_TEMPLATES.iter().any(|t| t.0 == label), "{label}");
        }
        let maths = PROSE_TEMPLATES.iter().find(|t| t.0 == "maths").unwrap();
        assert_eq!(wrap(maths, "x^2"), "$x^2$");
        // the claude template is a keyword line
        assert!(keyword_line("  claude").is_some());
        assert_eq!(indented("a\n  b", "    "), "a\n      b");
        assert_eq!(indented(&wrap(have, "P"), "  "), "have h : P := by\n    ");
    }

    #[test]
    fn the_caret_is_in_a_term_a_tactic_or_a_doc() {
        let at = |text: &str| place(text);
        // a declaration's header and body are terms
        assert_eq!(at("def merge (left right : List Nat) : "), Place::Term);
        assert_eq!(at("def merge (left right : List Nat) : List Nat := "), Place::Term);
        assert_eq!(at("def merge (left right : List Nat) : List Nat :=\n  "), Place::Term);
        assert_eq!(at("theorem t (n : Nat) :\n    "), Place::Term);
        assert_eq!(at("theorem t (n "), Place::Term);
        // a tactic's start: after `by`, at a line's start in the block, after `·`, `;`, `=>`
        assert_eq!(at("theorem t : p := by\n  "), Place::Tactic);
        assert_eq!(at("theorem t : p := by "), Place::Tactic);
        assert_eq!(at("theorem t : p ∧ q := by\n  constructor\n  · "), Place::Tactic);
        assert_eq!(at("theorem t : p := by\n  intro h; "), Place::Tactic);
        assert_eq!(at("theorem t : p := by\n  induction xs with\n  | nil => "), Place::Tactic);
        assert_eq!(at("theorem t : p := by\n  cases h <;> "), Place::Tactic);
        // in a tactic: its arguments are terms (have's statement, exact's term, a bracket)
        assert_eq!(at("theorem t : p := by\n  have h : "), Place::Term);
        assert_eq!(at("theorem t : p := by\n  exact "), Place::Term);
        assert_eq!(at("theorem t : p := by\n  show "), Place::Term);
        assert_eq!(at("theorem t : p := by\n  rw [h, "), Place::Term);
        assert_eq!(at("theorem t : p := by\n  exact (fun x =>\n    "), Place::Term);
        assert_eq!(at("theorem t : p := by\n  simp"), Place::Term, "just after a tactic");
        assert_eq!(at("theorem t : p := by\n  exact fun x => "), Place::Term);
        assert_eq!(at("theorem t : p := by\n  case succ n => "), Place::Tactic);
        // `by` is a word: `decreasing_by`, `bygone` are not
        assert_eq!(at("def f := bygone\n  "), Place::Term);
        // docstrings and module docs are Markdown; other comments and strings hold terms
        assert_eq!(at("/-- Adds "), Place::Doc);
        assert_eq!(at("/-! # Intro\n\nSome "), Place::Doc);
        assert_eq!(at("/-- Adds. -/\ndef add := by\n  "), Place::Tactic);
        assert_eq!(at("theorem t : p := by\n  -- by "), Place::Term);
        assert_eq!(at("theorem t : p := by\n  -- a note\n  "), Place::Tactic);
        assert_eq!(at("theorem t : p := by\n  /- note -/ "), Place::Tactic);
        assert_eq!(at("def s := \"by "), Place::Term);
    }

    #[test]
    fn list_items_continue() {
        assert_eq!(list_item("- one"), Some(("- ".into(), "".into())));
        assert_eq!(list_item("  * nested"), Some(("* ".into(), "  ".into())));
        assert_eq!(list_item("3. three"), Some(("4. ".into(), "".into())));
        assert_eq!(list_item("  9) nine"), Some(("10) ".into(), "  ".into())));
        assert_eq!(list_item("plain"), None);
        assert_eq!(list_item("-no"), None);
    }

    #[test]
    fn real_proofs_have_constructs_everywhere() {
        let Ok(t) = std::fs::read_to_string("/tmp/dsrv-lean-copy/DSRV/CausalSemantics.lean") else { return };
        let n = t.chars().count();
        let mut found = 0;
        for off in (0..n).step_by(7) {
            if let Some((s, e)) = construct_at(&t, off) {
                assert!(s < e && e <= n);
                found += 1;
            }
        }
        assert!(found > n / 7 / 2);
        // every pair of brackets in a real file is ordered and nested
        for (o, c) in paired_brackets(&t) {
            assert!(o < c && c < n);
        }
    }
}
