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
    ("logic", &["forall", "exists", "and", "or", "not", "iff", "to", "ne", "top", "bot", "vdash", "models", "neg", "nexists"]),
    ("greek", &["a", "b", "g", "d", "e", "z", "h", "th", "i", "k", "l", "m", "n", "x", "o", "p", "r", "s", "t", "u", "f", "ch", "psi", "w", "G", "D", "Th", "L", "Xi", "Pi", "S", "Phi", "Psi", "W"]),
    ("relations", &["le", "ge", "ne", "equiv", "approx", "sim", "simeq", "cong", "prec", "succ", "<", ">"]),
    ("sets", &["in", "notin", "sub", "sup", "subeq", "supeq", "cup", "cap", "empty", "setminus", "union", "inter", "powerset"]),
    ("arrows", &["r", "l", "lr", "r|", "mapsto", "=>", "<=>", "uparrow", "downarrow", "hookrightarrow"]),
    ("numbers", &["N", "Z", "Q", "R", "C", "inf", "sqrt", "sum", "prod", "int"]),
    ("operators", &["x", "o", "comp", "circ", ".", "cdot", "oplus", "otimes", "times", "pm", "dagger", "star", "bullet", "<>", "[[]]", "{{}}"]),
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

    fn snippet_at(&self, text: &str, at: usize) -> Option<String> {
        docs::snippet_at(text, at)
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
        edit::goal_completions(prefix, replace, &g).into_iter().map(|c| Completion { replace: c.replace, text: c.text, label: c.label }).collect()
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
