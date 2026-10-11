// SPDX-License-Identifier: GPL-3.0-only

//! The Lean 4 profile (provers §7.3): `.lean` files, how they split into blocks, docstrings as
//! prose, the `\` input abbreviations, colours found lexically, indentation, the construct the
//! goal is for, completions and the insert palette, and how the VS Code infoview shows goals
//! (C17d: `case tag`, the hypotheses, `⊢ target`).
//!
//! Lean has no prover spans: the host's driver sends lexical commands, and [`blocks`] and
//! [`Lean::lexical_commands`] find the same ones. The pieces are standalone modules
//! ([`lexer`], [`blocks`], [`docs`], [`abbrev`], [`edit`], [`templates`]) that depend on `std`
//! only; this file joins them to the app's types.
//!
//! * A module doc is a prose block (a [`Kind::Heading`] when it is only a heading, its body then
//!   the title, so that the view's `#`s are not doubled). A docstring is a prose block of its own
//!   above the declaration it documents. The `import`s are a formal block, not a header: the view
//!   titles a header "Theory".
//! * Source is Unicode as written: the codec is the identity, and `\` completion inserts the
//!   character (the table is `abbrev`'s).
//! * The lexer's classes map onto the colours the app has for Isabelle's markup: commands are
//!   `keyword1`, tactics and term keywords `keyword2`, strings `inner_quoted`, numbers `skolem`,
//!   types `tfree`, attributes `quasi_keyword`, and `sorry` a warning. The prover's own colours
//!   (semantic tokens) replace them when they arrive.

pub mod abbrev;
#[cfg(test)]
pub mod demo;
pub mod blocks;
pub mod docs;
pub mod edit;
pub mod lexer;
pub mod templates;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, LazyLock};

use super::capabilities::{Capabilities, Goals, ProjectAction, Search, Tool};
use super::{BlockTemplate, GoalBody, GoalPresentation, GoalSection, InsertContext, PaletteEntry, Profile};
use crate::codec::{Codec, Decoded, Plain, Symbols};
use crate::editor::{Completion, Goal, Hl, Sem};
use crate::goals::Subgoal;
use crate::input::Input;
use crate::theory::{Block, Command, Kind, Status};

pub struct Lean;

const BLOCK_TEMPLATES: &[BlockTemplate] = &[
    BlockTemplate { kind: "text", label: "+ Text", body: "/-!\n\n-/", caret: Some((1, 0)) },
    BlockTemplate { kind: "section", label: "+ Section", body: "/-!\n# Section\n-/", caret: None },
    // (the caret at the lemma's name)
    BlockTemplate { kind: "lemma", label: "+ Lemma", body: "theorem name : _ := by\n  sorry", caret: Some((0, 8)) },
];

// ---------------------------------------------------------------- symbols

/// Lean's characters: written as they are shown, typed with the `\` abbreviations
pub struct LeanSymbols;

static SYMBOLS: LazyLock<Arc<dyn Symbols>> = LazyLock::new(|| Arc::new(LeanSymbols));

impl Codec for LeanSymbols {
    fn decode(&self, source: &str) -> Decoded {
        Plain.decode(source)
    }

    fn encode(&self, text: &str) -> String {
        Plain.encode(text)
    }
}

/// The groups of the Symbols panel: a name and the abbreviations of its characters
const GROUPS: &[(&str, &[&str])] = &[
    ("logic", &["forall", "exists", "and", "or", "not", "iff", "to", "ne", "top", "bot", "vdash", "models", "nexists"]),
    (
        "greek",
        &[
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu", "xi", "pi", "rho", "sigma", "tau",
            "upsilon", "phi", "chi", "psi", "omega", "Gamma", "Delta", "Theta", "Lambda", "Xi", "Pi", "Sigma", "Phi", "Psi", "Omega",
        ],
    ),
    ("relations", &["le", "ge", "ne", "approx", "sim", "simeq", "cong", "prec", "succ", "in", "notin"]),
    ("sets", &["in", "notin", "sub", "cup", "cap", "empty", "setminus", "powerset"]),
    ("arrows", &["r", "l", "lr", "mapsto", "=>", "<=>", "uparrow", "downarrow", "hookrightarrow"]),
    ("numbers", &["N", "Z", "Q", "R", "C", "sqrt", "sum", "prod"]),
    ("operators", &["comp", ".", "oplus", "otimes", "times", "pm", "dagger", "star"]),
    ("brackets", &["<", ">", "[[", "]]"]),
];

impl Input for LeanSymbols {
    fn search(&self, query: &str, limit: usize) -> Vec<(char, String, String)> {
        // (only single characters: a pair such as ⟨⟩ is typed with `\<` and `\>`)
        abbrev::search(query, limit * 3)
            .into_iter()
            .filter_map(|h| {
                let mut it = h.text.chars();
                let c = it.next().filter(|_| it.next().is_none())?;
                let how = h.aliases.iter().take(3).map(|a| format!("\\{a}")).collect::<Vec<_>>().join(" ");
                Some((c, format!("\\{}", h.abbrev), how))
            })
            .take(limit)
            .collect()
    }

    fn search_text(&self, query: &str, limit: usize) -> Vec<crate::input::Hit> {
        abbrev::search(query, limit)
            .into_iter()
            .map(|h| {
                let how = h.aliases.iter().take(3).map(|a| format!("\\{a}")).collect::<Vec<_>>().join(" ");
                crate::input::Hit { text: h.text, caret: h.cursor, name: format!("\\{}", h.abbrev), how }
            })
            .collect()
    }

    fn abbreviations(&self) -> Vec<(char, &str)> {
        // (Lean source is typed with `\`; ASCII like `->` is Lean syntax, not a shortcut)
        Vec::new()
    }

    fn groups(&self) -> Vec<(String, Vec<(char, String)>)> {
        GROUPS
            .iter()
            .map(|(name, abbrevs)| {
                let mut seen = Vec::new();
                let chars = abbrevs
                    .iter()
                    .filter_map(|a| {
                        let e = abbrev::expand(a)?;
                        let mut it = e.text.chars();
                        let c = it.next().filter(|_| it.next().is_none())?;
                        let fresh = !seen.contains(&c);
                        seen.push(c);
                        fresh.then(|| (c, format!("\\{a}")))
                    })
                    .collect();
                (name.to_string(), chars)
            })
            .collect()
    }
}

// ---------------------------------------------------------------- colours

/// How the lexer's classes are coloured
fn colour_of(class: lexer::Class) -> Option<Hl> {
    use lexer::Class as C;
    Some(match class {
        C::Command | C::HashCommand => Hl::Sem(Sem::Keyword1),
        C::Keyword => Hl::Sem(Sem::Keyword2),
        C::Sorry => Hl::Warning,
        C::Comment | C::Doc => Hl::Comment,
        C::Str => Hl::Sem(Sem::Inner),
        C::Number => Hl::Sem(Sem::Skolem),
        C::Type => Hl::Sem(Sem::TFree),
        C::Attribute => Hl::Sem(Sem::QuasiKeyword),
        C::Operator | C::Name => return None,
    })
}

/// Code in Lean coloured by prose-render (for ```lean in a docstring): byte ranges and classes
pub fn highlight(code: &str) -> Vec<(Range<usize>, prose_render::highlight::Class)> {
    use lexer::Class as C;
    use prose_render::highlight::Class as H;
    let byte_of: Vec<usize> = code.char_indices().map(|(b, _)| b).chain(std::iter::once(code.len())).collect();
    lexer::lex(code)
        .into_iter()
        .filter_map(|t| {
            let class = match t.class {
                C::Command | C::HashCommand | C::Keyword => H::Keyword,
                C::Sorry => H::Constant,
                C::Comment | C::Doc => H::Comment,
                C::Str => H::String,
                C::Number => H::Number,
                C::Type => H::Type,
                C::Attribute => H::Attribute,
                C::Operator | C::Name => return None,
            };
            Some((byte_of[t.start]..byte_of[t.end], class))
        })
        .collect()
}

// ---------------------------------------------------------------- blocks

fn command(c: &blocks::Command) -> Command {
    Command { start: c.start, end: c.end, name: c.name.clone(), status: Status::Unprocessed }
}

fn worst(commands: &[Command]) -> Status {
    commands.iter().map(|c| c.status).max().unwrap_or(Status::Unprocessed)
}

/// The title of a module doc that is only a heading: its level and the span of the title in `text`
fn heading_title(chars: &[char], body: (usize, usize)) -> Option<(u8, (usize, usize))> {
    let line_start = (body.0..body.1).find(|&i| !chars[i].is_whitespace())?;
    let hashes = chars[line_start..body.1].iter().take_while(|c| **c == '#').count();
    let s = (line_start + hashes..body.1).find(|&i| !matches!(chars[i], ' ' | '\t'))?;
    let e = (s..body.1).rev().find(|&i| !chars[i].is_whitespace()).map_or(s, |i| i + 1);
    Some((hashes as u8, (s, e)))
}

fn lean_blocks(text: &str, prover: &[Command]) -> Vec<Block> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for b in blocks::blocks(text) {
        let prose = |kind: Kind, start: usize, end: usize, body: Option<(usize, usize)>| Block {
            kind,
            start,
            end,
            body,
            name: String::new(),
            status: Status::Finished,
            commands: Vec::new(),
        };
        match b.kind {
            blocks::Kind::Heading(_) | blocks::Kind::Text => {
                let body = b.body.unwrap_or((b.start, b.end));
                // (a module doc of one heading: the title is its body, and the view adds the #s)
                match (b.kind, heading_title(&chars, body)) {
                    (blocks::Kind::Heading(_), Some((level, title))) if b.doc.is_none() && chars.get(b.start + 2) == Some(&'!') => {
                        out.push(prose(Kind::Heading(level.min(6)), b.start, b.end, Some(title)));
                    }
                    _ => out.push(prose(Kind::Text, b.start, b.end, Some(body))),
                }
            }
            blocks::Kind::Comment => out.push(prose(Kind::Comment, b.start, b.end, None)),
            blocks::Kind::Header | blocks::Kind::Formal => {
                if let Some((ds, de)) = b.doc {
                    out.push(prose(Kind::Text, ds, de, b.body));
                }
                let (start, end) = b.code;
                let mut commands: Vec<Command> = b.commands.iter().map(command).collect();
                // The prover's commands, with their statuses, where it has them in the block
                let known: Vec<Command> = prover.iter().filter(|c| c.start >= start && c.end <= end).cloned().collect();
                if !known.is_empty() {
                    commands = known;
                }
                let name = if b.kind == blocks::Kind::Header { "import".to_string() } else { b.name.clone() };
                out.push(Block { kind: Kind::Formal, start, end, body: None, name, status: worst(&commands), commands });
            }
        }
    }
    out
}

// ---------------------------------------------------------------- goals

/// The hypotheses and target of a Lean goal state, as the editor's goal: every hypothesis is a
/// `vars` entry `name : type`, which [`Lean::goal_completions`] reads back
fn parse_state(state: &str) -> Option<Goal> {
    let g = edit::Goal::parse(state)?;
    Some(Goal { vars: g.hyps.iter().map(|(n, t)| format!("{n} : {t}")).collect(), premises: Vec::new(), conclusion: g.target })
}

/// Markdown as the paragraph editor shows it: line breaks as spaces (the paragraph wraps as the
/// page does), except in fenced code, where every line break matters. One character for one
fn shown_markdown(text: &str) -> String {
    let shown: Vec<char> = crate::editor::shown_text(text, true).chars().collect();
    let chars: Vec<char> = text.chars().collect();
    let mut out = shown.clone();
    let mut fence = false;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let n = line.chars().count();
        let t = line.trim_start();
        let marker = t.starts_with("```") || t.starts_with("~~~");
        if fence || marker {
            out[at..at + n].copy_from_slice(&chars[at..at + n]);
        }
        if marker {
            fence = !fence;
        }
        at += n;
    }
    out.into_iter().collect()
}

/// A Lean message without the goals it lists, which the goal view shows: the text before the first
/// goal, and for "unsolved goals" how many there are and their case names, as the infoview's first
/// line says it. A goal is its `case` line, its hypotheses (`x : T`, with continuation lines
/// indented) and its `⊢` line
fn summarise(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let Some(first_turnstile) = lines.iter().position(|l| l.starts_with('⊢')) else { return text.trim_end().to_string() };
    // The goal's start: back over its hypotheses and case line
    let mut start = first_turnstile;
    while start > 0 {
        let l = lines[start - 1];
        let hypothesis = l.starts_with(char::is_whitespace) || l.contains(" : ") || l.starts_with("case ");
        if hypothesis && !l.trim().is_empty() {
            start -= 1;
        } else {
            break;
        }
    }
    let head = lines[..start].join("\n").trim().to_string();
    let goals = lines.iter().filter(|l| l.starts_with('⊢')).count();
    let cases: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("case ")).map(str::trim).collect();
    let detail = match (goals, cases.len()) {
        (1, 0) => String::new(),
        (1, _) => format!(" ({})", cases.iter().map(|c| format!("case {c}")).collect::<Vec<_>>().join(", ")),
        (n, 0) => format!(" ({n} goals)"),
        (n, _) => format!(" ({n} goal{}: {})", if n == 1 { "" } else { "s" }, cases.iter().map(|c| format!("case {c}")).collect::<Vec<_>>().join(", ")),
    };
    if head.is_empty() { format!("{} goal{}{detail}", goals, if goals == 1 { "" } else { "s" }) } else { format!("{head}{detail}") }
}

fn name_at(text: &str, at: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let is = |c: char| c.is_alphanumeric() || matches!(c, '_' | '\'' | '.' | '!' | '?') || ('\u{2080}'..='\u{209c}').contains(&c);
    let at = at.min(chars.len());
    let mut s = at;
    while s > 0 && is(chars[s - 1]) {
        s -= 1;
    }
    let mut e = at;
    while e < chars.len() && is(chars[e]) {
        e += 1;
    }
    let word: String = chars[s..e].iter().collect();
    let word = word.trim_matches('.');
    (!word.is_empty() && !word.chars().all(|c| c.is_ascii_digit())).then(|| word.to_string())
}

impl Profile for Lean {
    fn backend(&self) -> &'static str {
        "lean"
    }

    fn documents(&self) -> &'static [&'static str] {
        &[".lean"]
    }

    fn blocks(&self, text: &str, commands: &[Command], _kinds: &HashMap<String, String>) -> Vec<Block> {
        lean_blocks(text, commands)
    }

    fn lexical_commands(&self, text: &str, _kinds: &HashMap<String, String>) -> Vec<Command> {
        blocks::commands(text).iter().map(command).collect()
    }

    fn is_statement(&self, block: &Block, _kinds: &HashMap<String, String>) -> bool {
        matches!(block.name.as_str(), "theorem" | "lemma" | "example" | "proposition" | "corollary")
    }

    fn unfinished(&self) -> &'static [&'static str] {
        &["sorry", "admit"]
    }

    fn placeholder(&self) -> &'static str {
        "sorry"
    }

    fn symbols(&self, _from_host: Option<&Arc<dyn Symbols>>) -> Arc<dyn Symbols> {
        SYMBOLS.clone()
    }

    fn has_symbol_table(&self) -> bool {
        true
    }

    fn name_at(&self, text: &str, at: usize) -> Option<String> {
        name_at(text, at)
    }

    fn prose(&self, body: &str, _codec: &dyn Codec) -> String {
        // (the Markdown as written, without the common indentation)
        docs::to_markdown(body).markdown
    }

    fn prose_map(&self, body: &str) -> Option<super::ProseMap> {
        let d = docs::to_markdown(body);
        let runs = d.segments();
        Some(super::ProseMap { markdown: d.markdown, runs })
    }

    fn snippet_at(&self, text: &str, at: usize) -> Option<String> {
        docs::snippet_at(text, at)
    }

    fn shown_prose(&self, text: &str, _caret: Option<usize>, _symbols: &dyn Symbols) -> String {
        shown_markdown(text)
    }

    fn prose_code_classes(&self, text: &str) -> Vec<Option<Hl>> {
        let mut out = vec![None; text.chars().count()];
        for s in docs::snippets(text) {
            let hl = match s.kind {
                docs::Kind::Math { .. } => Hl::Sem(Sem::Skolem),
                docs::Kind::Code => Hl::Sem(Sem::Free),
            };
            for slot in &mut out[s.start..s.end] {
                *slot = Some(hl);
            }
        }
        out
    }

    fn bare_prose(&self, around: &str) -> bool {
        around.split_whitespace().all(|w| matches!(w, "/-!" | "/--" | "-/") || (w.starts_with('#') && w.chars().all(|c| c == '#')))
    }

    fn comments(&self, text: &str) -> Vec<(usize, usize)> {
        lexer::lex(text).into_iter().filter(|t| matches!(t.class, lexer::Class::Comment | lexer::Class::Doc)).map(|t| (t.start, t.end)).collect()
    }

    fn lexical_colours(&self, text: &str) -> Vec<(usize, usize, Hl)> {
        lexer::lex(text).into_iter().filter_map(|t| colour_of(t.class).map(|hl| (t.start, t.end, hl))).collect()
    }

    fn next_indent(&self, line: &str) -> String {
        edit::next_indent(line)
    }

    fn construct_at(&self, text: &str, commands: &[&Command], at: usize) -> Option<(usize, usize)> {
        // Only the proof's own lines: from its first command to its last
        let lo = commands.iter().map(|c| c.start).min()?;
        let hi = commands.iter().map(|c| c.end).max()?;
        let sub: String = text.chars().skip(lo).take(hi.saturating_sub(lo)).collect();
        edit::construct_at(&sub, at.saturating_sub(lo)).map(|(s, e)| (s + lo, e + lo))
    }

    fn parse_goal(&self, state: &str) -> Option<Goal> {
        parse_state(state)
    }

    fn goal_completions(&self, prefix: &str, replace: usize, goal: &Goal) -> Vec<Completion> {
        let hyps = goal
            .vars
            .iter()
            .filter_map(|v| v.split_once(" : ").map(|(n, t)| (n.to_string(), t.to_string())))
            .collect();
        let g = edit::Goal { case: None, hyps, target: goal.conclusion.clone() };
        edit::goal_completions(prefix, replace, &g).into_iter().map(|c| Completion { back: 0, replace: c.replace, text: c.text, label: c.label }).collect()
    }

    fn in_term(&self, _before: &str) -> bool {
        false
    }

    fn palette(&self, context: InsertContext, caps: &Capabilities) -> Vec<PaletteEntry> {
        let entry = |(l, b, a): &(&str, &str, &str)| PaletteEntry::new(l, b, a);
        match context {
            InsertContext::Symbols => ["∀", "∃", "∧", "∨", "¬", "→", "↔", "≠", "≤", "≥", "∈", "∉", "⊆", "∪", "∩", "∘", "×", "λ", "α", "β", "γ", "ℕ", "ℤ", "ℝ", "⟨", "⟩", "·", "⊢"]
                .iter()
                .map(|c| PaletteEntry::new(c, c, ""))
                .collect(),
            InsertContext::Prose => edit::PROSE_TEMPLATES.iter().map(entry).collect(),
            // (a tactic of the toolchain only when the session offers it)
            InsertContext::Proof => edit::PROOF_TEMPLATES
                .iter()
                .filter(|(l, _, _)| !matches!(*l, "exact?" | "apply?" | "simp?" | "omega" | "decide") || caps.tools.is_empty() || caps.typed_tool(l).is_some())
                .map(entry)
                .collect(),
        }
    }

    fn lists(&self, line: &str, _symbols: &dyn Symbols) -> Vec<PaletteEntry> {
        match edit::list_item(line) {
            Some((marker, indent)) => {
                let bullet = marker.starts_with(|c: char| c.is_ascii_digit()).then(|| "1. ".to_string()).unwrap_or_else(|| marker.clone());
                vec![
                    PaletteEntry::new("next item", &format!("\n{indent}{marker}"), ""),
                    PaletteEntry::new("nested item", &format!("\n{indent}  {bullet}"), ""),
                ]
            }
            None => vec![PaletteEntry::new("bullet list", "\n- ", ""), PaletteEntry::new("numbered list", "\n1. ", "")],
        }
    }

    fn block_templates(&self) -> &'static [BlockTemplate] {
        BLOCK_TEMPLATES
    }

    fn keyword_line(&self, line: &str) -> bool {
        edit::keyword_line(line).is_some()
    }

    fn keyword_tokens(&self, text: &str) -> Vec<(usize, usize)> {
        edit::keyword_tokens(text)
    }

    fn facts_line(&self, facts: &[String], _indent: &str) -> String {
        format!("simp [{}]", facts.join(", "))
    }

    fn facts_tool(&self) -> &'static str {
        "simp"
    }

    fn tool_invocation(&self, name: &str, params: &str) -> String {
        if params.is_empty() { name.to_string() } else { format!("{name} {params}") }
    }

    fn tool_waiting_error(&self, _text: &str) -> bool {
        false
    }

    fn interrupted(&self, text: &str) -> bool {
        text.contains("interrupted") || text.contains("cancelled")
    }

    fn message_summary(&self, text: &str) -> String {
        summarise(text)
    }

    fn present_goal<'a>(&self, sg: &'a Subgoal) -> GoalPresentation<'a> {
        // As the VS Code infoview shows a goal: `case tag`, the hypotheses, `⊢ target`
        let heading = sg.case.as_ref().and_then(|c| c["raw"].as_str().or_else(|| c["text"].as_str())).map(|t| format!("case {}", t.trim())).filter(|h| h.len() > 5);
        let mut sections = Vec::new();
        if !sg.params.is_empty() {
            sections.push(GoalSection { label: None, body: GoalBody::Many(&sg.params) });
        }
        if let Some(c) = &sg.concl {
            sections.push(GoalSection { label: Some("⊢"), body: GoalBody::One(c) });
        }
        GoalPresentation { heading, sections }
    }

    fn facts_label(&self) -> &'static str {
        "facts"
    }

    fn commands_are_keywords(&self) -> bool {
        false
    }

    fn symbol_query_char(&self, c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '<' | '>' | '.' | '-' | '=' | '|' | ':' | '[' | ']' | '(' | ')' | '{' | '}' | '_' | '*' | '+' | '/' | '~' | '!' | '?' | '@' | '^' | '&' | '#' | '$' | '%' | '\'' | '"' | ',' | ';' | '`')
    }

    fn angle_escapes(&self) -> bool {
        false
    }

    fn goal_noun(&self) -> &'static str {
        "goal"
    }

    fn goal_types_option(&self) -> bool {
        false
    }

    fn default_capabilities(&self) -> Capabilities {
        let s = |s: &str| s.to_string();
        let tool = |name: &str| Tool { name: s(name), typed: s(name), kind: s("prove"), stop: false, label: s(name), ..Default::default() };
        Capabilities {
            checking: s("continuous"),
            pause: s("close_documents"),
            interrupt: true,
            goals: Goals { structured: true, term_goal: true, facts: false, shelved: false },
            hover: true,
            definition: true,
            spans: s("lexical"),
            tools: ["exact?", "apply?", "rw?", "simp?", "decide", "omega"].map(tool).to_vec(),
            counterexamples: Vec::new(),
            search: Some(Search {
                label: s("Search"),
                tab: s("Search"),
                syntax: s("loogle"),
                placeholder: s("Nat.add_comm   or   ?a + ?b = ?b + ?a   or   \"comm\""),
                note: s("Loogle: by name, by a pattern of the type (?a, ?b stand for anything), or by a word of the name. Its source is a setting."),
                operation: s("loogle"),
                args: Vec::new(),
                sources: vec![s("exact?"), s("apply?"), s("loogle")],
            }),
            command: ["#check", "#eval", "#print", "#reduce"].map(s).to_vec(),
            check_candidates: s("scratch"),
            project_actions: vec![
                ProjectAction { name: s("build"), label: s("Build (lake build)") },
                ProjectAction { name: s("rebuild_imports"), label: s("Rebuild imports") },
            ],
            semantic_colours: true,
            documents: vec![s(".lean")],
            project_files: ["lakefile.lean", "lakefile.toml", "lean-toolchain", "lake-manifest.json"].map(s).to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::demo::{DEMO, goal_parts};
    use super::*;
    use crate::goals::Goals;
    use crate::prover::{self, profile};
    use serde_json::json;

    fn text_of(text: &str, s: usize, e: usize) -> String {
        text.chars().skip(s).take(e - s).collect()
    }

    fn demo_blocks() -> Vec<Block> {
        profile(Some("lean")).blocks(DEMO, &[], &HashMap::new())
    }

    #[test]
    fn the_profile_is_registered() {
        assert_eq!(profile(Some("lean")).backend(), "lean");
        assert_eq!(profile(None).backend(), "isabelle");
        assert!(prover::profiles().iter().any(|p| p.is_document("/x/DSRV/A.lean")));
        assert!(!profile(Some("lean")).is_document("/x/A.thy"));
        assert_eq!(profile(Some("lean")).document_name("Causal.lean"), "Causal");
    }

    #[test]
    fn the_demo_splits_into_blocks_that_read_right() {
        let bs = demo_blocks();
        let kinds: Vec<(Kind, String)> = bs.iter().map(|b| (b.kind, b.name.clone())).collect();
        // the imports are formal code, not a "Theory" header
        assert_eq!(kinds[0], (Kind::Formal, "import".into()));
        assert!(bs.iter().all(|b| b.kind != Kind::Header));
        // the module doc is prose with a heading in it
        assert_eq!(bs[1].kind, Kind::Text);
        let (s, e) = bs[1].body.unwrap();
        assert!(text_of(DEMO, s, e).trim_start().starts_with("# Timed provenance"));
        // a docstring is a prose block above its declaration
        let st = bs.iter().position(|b| b.name == "structure").unwrap();
        assert_eq!(bs[st - 1].kind, Kind::Text);
        let (s, e) = bs[st - 1].body.unwrap();
        assert!(text_of(DEMO, s, e).contains("```lean"));
        assert_eq!(text_of(DEMO, bs[st - 1].start, bs[st - 1].end).chars().take(4).collect::<String>(), "/-- ");
        assert!(text_of(DEMO, bs[st].start, bs[st].end).ends_with("deriving Repr, DecidableEq"));
        // blocks are ordered and only whitespace is between them
        let chars: Vec<char> = DEMO.chars().collect();
        let mut at = 0;
        for b in &bs {
            assert!(b.start >= at);
            assert!(chars[at..b.start].iter().all(|c| c.is_whitespace()), "dropped {:?}", text_of(DEMO, at, b.start));
            at = b.end;
        }
        assert!(bs.iter().any(|b| b.kind == Kind::Comment));
        // theorems are statements
        let p = profile(Some("lean"));
        let theorems: Vec<&Block> = bs.iter().filter(|b| p.is_statement(b, &HashMap::new())).collect();
        assert_eq!(theorems.len(), 5);
    }

    #[test]
    fn a_module_doc_of_one_heading_is_a_heading_whose_body_is_the_title() {
        let text = "/-!\n## The section\n-/\n\ndef x := 1\n";
        let bs = profile(Some("lean")).blocks(text, &[], &HashMap::new());
        assert_eq!(bs[0].kind, Kind::Heading(2));
        let (s, e) = bs[0].body.unwrap();
        assert_eq!(text_of(text, s, e), "The section");
        assert!(profile(Some("lean")).bare_prose(&text_of(text, bs[0].start, s)));
        assert!(profile(Some("lean")).bare_prose("/-!\n"));
        assert!(!profile(Some("lean")).bare_prose("/-! hello"));
    }

    #[test]
    fn commands_are_the_lexical_ones_and_the_provers_win() {
        let p = profile(Some("lean"));
        let lexical = p.lexical_commands(DEMO, &HashMap::new());
        let names: Vec<&str> = lexical.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"theorem") && names.contains(&"induction") && names.contains(&"sorry") && names.contains(&"·"));
        // statuses from the prover are the block's
        let mut known = lexical.clone();
        for c in &mut known {
            c.status = if c.name == "sorry" { Status::Warned } else { Status::Finished };
        }
        let bs = p.blocks(DEMO, &known, &HashMap::new());
        let later = bs.iter().find(|b| b.name == "theorem" && text_of(DEMO, b.start, b.end).contains("theorem later")).unwrap();
        assert_eq!(later.status, Status::Warned);
        assert_eq!(later.commands.len(), 2);
        let first = bs.iter().find(|b| b.name == "import").unwrap();
        assert_eq!(first.status, Status::Finished);
        // without the prover's, blocks are unprocessed
        assert!(demo_blocks().iter().filter(|b| b.kind == Kind::Formal).all(|b| b.status == Status::Unprocessed));
    }

    #[test]
    fn the_goal_goes_with_the_tactic_among_the_blocks_commands() {
        let p = profile(Some("lean"));
        let lexical = p.lexical_commands(DEMO, &HashMap::new());
        let bs = demo_blocks();
        let b = bs.iter().find(|b| text_of(DEMO, b.start, b.end).starts_with("theorem and_swap'")).unwrap();
        let cmds: Vec<&Command> = lexical.iter().filter(|c| c.start >= b.start && c.end <= b.end).collect();
        let at = DEMO.find("exact hq").map(|i| DEMO[..i].chars().count()).unwrap();
        let (s, e) = p.construct_at(DEMO, &cmds, at + 2).unwrap();
        assert_eq!(text_of(DEMO, s, e), "· exact hq");
        let (s, e) = p.construct_at(DEMO, &cmds, b.start + 3).unwrap();
        assert!(text_of(DEMO, s, e).ends_with(":= by"));
    }

    #[test]
    fn goals_are_shown_as_the_infoview_shows_them() {
        let msgs = goal_parts(&[("cons", &["x : Nat", "xs : List Nat", "ih : xs ++ [] = xs"], "x :: xs ++ [] = x :: xs")]);
        let goals = Goals::parse(&msgs);
        let sg = &goals.subgoals[0];
        let shown = Lean.present_goal(sg);
        assert_eq!(shown.heading.as_deref(), Some("case cons"));
        assert_eq!(shown.sections.len(), 2);
        assert_eq!(shown.sections[0].label, None, "hypotheses are unlabelled");
        assert!(matches!(shown.sections[0].body, GoalBody::Many(p) if p.len() == 3));
        assert_eq!(shown.sections[1].label, Some("⊢"));
        // a goal without a case tag has no heading
        let goals = Goals::parse(&goal_parts(&[("", &["h : p"], "p")]));
        assert_eq!(Lean.present_goal(&goals.subgoals[0]).heading, None);
    }

    #[test]
    fn goal_completions_read_the_hypotheses_back() {
        let g = Lean.parse_goal("case succ\nn : Nat\nh : ∃ k, n = 2 * k\n⊢ n + 1 ≠ 0").unwrap();
        let got: Vec<String> = Lean.goal_completions("ob", 2, &g).into_iter().map(|c| c.text).collect();
        assert_eq!(got, vec!["obtain ⟨k, hk⟩ := h"]);
        let got: Vec<String> = Lean.goal_completions("in", 2, &g).into_iter().map(|c| c.text).collect();
        assert_eq!(got, vec!["intro h"]);
        assert!(Lean.parse_goal("no goals").is_none());
    }

    #[test]
    fn symbols_complete_with_the_lean_abbreviations() {
        let s = Lean.symbols(None);
        let found = s.search("forall", 5);
        assert_eq!(found[0].0, '∀');
        assert_eq!(found[0].1, "\\forall");
        assert_eq!(s.search("to", 3)[0].0, '→');
        assert!(s.search("<", 5).iter().all(|(c, _, _)| *c != '\u{0}'));
        // source is shown as written
        let d = s.decode("∀ x, α → β");
        assert_eq!(d.text, "∀ x, α → β");
        assert_eq!(d.to_source(3), 3);
        assert_eq!(s.encode("→"), "→");
        assert!(Lean.has_symbol_table());
        let groups = s.groups();
        assert!(groups.iter().any(|(g, cs)| g == "logic" && cs.iter().any(|(c, _)| *c == '∀')));
        assert!(groups.iter().all(|(_, cs)| !cs.is_empty()));
        assert!(s.abbreviations().is_empty());
    }

    #[test]
    fn prose_is_markdown_without_the_indent() {
        let md = Lean.prose("\n  # Title\n\n  Some `code` and $x^2$.\n", &Plain);
        assert_eq!(md, "# Title\n\nSome `code` and $x^2$.");
        assert_eq!(Lean.snippet_at("so $x + 0 = x$ here", 5).as_deref(), Some("$x + 0 = x$"));
        let c = Lean.prose_code_classes("a `b` and $c$");
        assert_eq!(c[3], Some(Hl::Sem(Sem::Free)));
        assert_eq!(c[11], Some(Hl::Sem(Sem::Skolem)));
        assert_eq!(c[0], None);
    }

    #[test]
    fn fenced_code_keeps_its_line_breaks_in_the_paragraph_editor() {
        let text = "Some prose\nover two lines.\n\n```lean\ndef a := 1\n  def b := 2\n```\nafter";
        let shown = Lean.shown_prose(text, None, &LeanSymbols);
        assert_eq!(shown.chars().count(), text.chars().count());
        assert!(shown.starts_with("Some prose over two lines."), "{shown:?}");
        assert!(shown.contains("```lean\ndef a := 1\n  def b := 2\n```"), "{shown:?}");
    }

    #[test]
    fn prose_offsets_map_back_to_the_source() {
        let body = "\n  # Title\n\n  Some `code`, $x^2$ and ∀.\n";
        let map = Lean.prose_map(body).unwrap();
        assert_eq!(map.markdown, "# Title\n\nSome `code`, $x^2$ and ∀.");
        for needle in ["Title", "x^2", "∀", "`code`"] {
            let md = map.markdown.find(needle).unwrap();
            let src = map.source_char(md);
            assert_eq!(body.chars().skip(src).take(needle.chars().count()).collect::<String>(), needle);
            assert_eq!(map.markdown_byte(src), md);
        }
        assert_eq!(map.source_char(map.markdown.len()), body.trim_end().chars().count());
        // Isabelle's conversion rewrites too much to have a map
        assert!(prover::ISABELLE.prose_map("x").is_none());
    }

    #[test]
    fn a_theory_maps_a_rendered_offset_into_the_file() {
        use crate::editor::{Mode, Theory};
        let doc = json!({"text": DEMO, "revision": 1, "prover": {}});
        let t = Theory::new("/x.lean".into(), &doc, &LeanSymbols, Arc::new(HashMap::new()), Mode::Literate, &Lean);
        let i = t.blocks.iter().position(|b| b.kind == Kind::Text && t.prose_maps[t.blocks.iter().position(|x| x == b).unwrap()].as_ref().is_some_and(|m| m.markdown.contains("enabling"))).unwrap();
        let md = t.prose_maps[i].as_ref().unwrap().markdown.find("enabling").unwrap();
        let at = t.source_of_markdown(i, md).unwrap();
        assert_eq!(t.local.chars().skip(at).take(8).collect::<String>(), "enabling");
        assert_eq!(t.source_of_markdown(0, 0), None, "the imports are not prose");
    }

    #[test]
    fn messages_do_not_list_their_goals() {
        assert_eq!(Lean.message_summary("unsolved goals\nn : Nat\n⊢ n + 0 = n"), "unsolved goals");
        assert_eq!(
            Lean.message_summary("unsolved goals\ncase zero\n⊢ P 0\n\ncase succ\nn : Nat\nih : P n\n⊢ P (n + 1)"),
            "unsolved goals (2 goals: case zero, case succ)"
        );
        assert_eq!(Lean.message_summary("unsolved goals\na : Nat\n⊢ p\n\nb : Nat\n⊢ q"), "unsolved goals (2 goals)");
        // a hypothesis that runs over lines
        assert_eq!(Lean.message_summary("unsolved goals\nh : ∃ x,\n    x > 0\n⊢ False"), "unsolved goals");
        // an error with its own explanation first
        assert_eq!(Lean.message_summary("type mismatch\n  h\nhas type\n  p : Prop\nbut is expected to have type\n  q : Prop"), "type mismatch\n  h\nhas type\n  p : Prop\nbut is expected to have type\n  q : Prop");
        assert_eq!(Lean.message_summary("linarith failed to find a contradiction\ncase a\nx : ℝ\n⊢ False"), "linarith failed to find a contradiction (case a)");
        assert_eq!(Lean.message_summary("declaration uses 'sorry'"), "declaration uses 'sorry'");
    }

    #[test]
    fn lexical_colours_and_comments() {
        let text = "theorem t : Nat := by\n  sorry -- later\n/-- doc -/\n";
        let cs = Lean.lexical_colours(text);
        let at = |w: &str| text[..text.find(w).unwrap()].chars().count();
        assert!(cs.contains(&(0, 7, Hl::Sem(Sem::Keyword1))));
        assert!(cs.contains(&(at("Nat"), at("Nat") + 3, Hl::Sem(Sem::TFree))));
        assert!(cs.contains(&(at("sorry"), at("sorry") + 5, Hl::Warning)));
        let comments = Lean.comments(text);
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].0, at("--"));
    }

    #[test]
    fn keyword_lines_and_the_palette() {
        assert!(Lean.keyword_line("  claude"));
        assert!(Lean.keyword_line("  intro h; claude[opus]"));
        assert!(!Lean.keyword_line("  exact @foo _"));
        assert_eq!(Lean.keyword_tokens("  codex\n"), vec![(2, 7)]);
        let caps = Lean.default_capabilities();
        let proof = Lean.palette(InsertContext::Proof, &caps);
        assert!(proof.iter().any(|e| e.label == "have" && e.before == "have h : " && e.after == " := by\n  "));
        assert!(proof.iter().any(|e| e.label == "exact?"));
        let mut no_omega = caps.clone();
        no_omega.tools.retain(|t| t.name != "omega");
        assert!(!Lean.palette(InsertContext::Proof, &no_omega).iter().any(|e| e.label == "omega"));
        assert!(Lean.palette(InsertContext::Prose, &caps).iter().any(|e| e.label == "maths" && e.before == "$"));
        assert!(Lean.palette(InsertContext::Symbols, &caps).iter().any(|e| e.label == "∀"));
        let in_list = Lean.lists("  3. three", &LeanSymbols);
        assert_eq!(in_list[0].before, "\n  4. ");
        assert_eq!(Lean.lists("plain", &LeanSymbols)[0].label, "bullet list");
        assert_eq!(Lean.facts_line(&["a".into(), "b".into()], "  "), "simp [a, b]");
        assert_eq!(Lean.name_at("exact Nat.add_comm a b", 9).as_deref(), Some("Nat.add_comm"));
        assert_eq!(Lean.name_at("x := 12", 6), None);
    }

    #[test]
    fn templates_match_the_template_module() {
        let t = Lean.block_templates();
        assert_eq!(t[0].body, templates::text().text);
        assert_eq!(t[1].body, templates::section(1).text);
        assert_eq!(t[2].body, templates::lemma("name").text);
        assert_eq!(t[2].caret, Some((0, "theorem ".len())));
    }

    #[test]
    fn the_hosts_capabilities_and_the_defaults_agree() {
        // what LeanDriver.capabilities() sends for a project without Mathlib
        let summary = json!({"capabilities": {
            "checking": "continuous", "pause": "close_documents", "interrupt": true,
            "goals": {"structured": true, "term_goal": true, "facts": false, "case_tags": true},
            "hover": true, "definition": true, "spans": "lexical",
            "tools": [{"name": "exact?", "typed": "exact?", "kind": "prove", "stop": false}],
            "counterexamples": [],
            "search": {"label": "Search", "syntax": "loogle", "operation": "loogle", "sources": ["exact?", "apply?", "loogle"]},
            "command": ["#check"], "check_candidates": "scratch",
            "project_actions": [{"name": "build", "label": "Build (lake build)"}],
            "semantic_colours": true, "documents": [".lean"], "project_files": ["lakefile.toml"],
            "placeholder": "sorry", "facts_line": "simp [{facts}]", "goal_style": "lean"}});
        let caps = Capabilities::from_summary(Some(&summary), Lean.default_capabilities());
        assert_eq!(caps.spans, "lexical");
        assert!(caps.goals.term_goal && !caps.goals.facts);
        assert_eq!(caps.typed_tools(), vec!["exact?".to_string()]);
        let search = caps.search.unwrap();
        assert_eq!(search.operation, "loogle");
        assert_eq!(search.short(), "Search");
        assert!(search.sources.contains(&"apply?".to_string()));
        assert!(!search.placeholder.is_empty(), "presentation from the profile");
        // a tool's settings from the host are a list, or ignored
        let odd = json!({"capabilities": {"tools": [{"name": "exact?", "settings": {"type": "object"}}]}});
        let caps = Capabilities::from_summary(Some(&odd), Lean.default_capabilities());
        assert!(caps.tool("exact?").unwrap().settings.is_empty());
    }

    #[test]
    fn code_in_docstrings_is_coloured_by_the_lexer() {
        use prose_render::highlight::Class;
        let code = "theorem t : Nat := by\n  simp -- é\n";
        let got = highlight(code);
        assert_eq!(got[0], (0..7, Class::Keyword));
        assert!(got.iter().any(|(r, c)| *c == Class::Type && &code[r.clone()] == "Nat"));
        assert!(got.iter().any(|(r, c)| *c == Class::Comment && code[r.clone()].starts_with("-- é")));
        assert!(!got.is_empty());
        prover::register_highlighters();
        assert!(prose_render::highlight::knows("lean"));
        assert!(prose_render::highlight::knows("Isabelle"));
        let thy = prover::isabelle::highlight("lemma \"a = a\" (* c *)\n  by simp");
        assert!(thy.iter().any(|(_, c)| *c == Class::Keyword) && thy.iter().any(|(_, c)| *c == Class::Comment) && thy.iter().any(|(_, c)| *c == Class::String));
    }
}
