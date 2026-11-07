// SPDX-License-Identifier: GPL-3.0-only

//! The Isabelle profile: theories (`.thy`), their blocks, Isabelle's document
//! text and symbols, Isar, and how Isabelle's tools and goals are presented.
//! Everything here is what Deductum did before it had backends; the defaults
//! in [`Isabelle::default_capabilities`] are what an Isabelle session could do
//! then, so that a host which does not send capabilities looks as it did.

pub mod blocks;
pub mod isar;
pub mod literate;
pub mod prose;
pub mod symbols;

use std::collections::HashMap;
use std::sync::Arc;

use super::capabilities::{Capabilities, Goals, ProjectAction, Search, Setting, Tool};
use super::{BlockTemplate, GoalBody, GoalPresentation, GoalSection, InsertContext, PaletteEntry, Profile};
use crate::codec::{Codec, Decoded, Symbols};
use crate::editor::{Completion, Goal, Hl, Markup, Sem};
use crate::goals::Subgoal;
use crate::theory::{Block, Command};

pub struct Isabelle;

/// Isabelle code coloured by prose-render (for ```isabelle in prose): commands, comments, quoted
/// text and cartouches, found lexically, as byte ranges
pub fn highlight(code: &str) -> Vec<(std::ops::Range<usize>, prose_render::highlight::Class)> {
    use prose_render::highlight::Class;
    let byte_of: Vec<usize> = code.char_indices().map(|(b, _)| b).chain(std::iter::once(code.len())).collect();
    let chars: Vec<char> = code.chars().collect();
    let mut out: Vec<(usize, usize, Class)> = isar::comments(code).into_iter().map(|(s, e)| (s, e, Class::Comment)).collect();
    for c in blocks::lexical_commands(code, &HashMap::new()) {
        let end = c.start + c.name.chars().count();
        if !out.iter().any(|(s, e, _)| *s <= c.start && c.start < *e) {
            out.push((c.start, end, Class::Keyword));
        }
    }
    // (quoted text outside comments)
    let mut i = 0;
    while i < chars.len() {
        if out.iter().any(|(s, e, c)| *c == Class::Comment && *s <= i && i < *e) {
            i += 1;
        } else if chars[i] == '"' {
            let end = (i + 1..chars.len()).find(|&k| chars[k] == '"').map_or(chars.len(), |k| k + 1);
            out.push((i, end, Class::String));
            i = end;
        } else {
            i += 1;
        }
    }
    out.sort_by_key(|(s, _, _)| *s);
    out.into_iter().map(|(s, e, c)| (byte_of[s.min(chars.len())]..byte_of[e.min(chars.len())], c)).collect()
}

const BLOCK_TEMPLATES: &[BlockTemplate] = &[
    BlockTemplate { kind: "text", label: "+ Text", body: "text \\<open>\\<close>", caret: None },
    BlockTemplate { kind: "section", label: "+ Section", body: "section \\<open>\\<close>", caret: None },
    // (the caret between the quotes)
    BlockTemplate { kind: "lemma", label: "+ Lemma", body: "lemma \"\"\n  sorry", caret: Some((0, 7)) },
];

impl Profile for Isabelle {
    fn backend(&self) -> &'static str {
        "isabelle"
    }

    fn documents(&self) -> &'static [&'static str] {
        &[".thy"]
    }

    fn blocks(&self, text: &str, commands: &[Command], kinds: &HashMap<String, String>) -> Vec<Block> {
        blocks::blocks(text, commands, kinds)
    }

    fn lexical_commands(&self, text: &str, kinds: &HashMap<String, String>) -> Vec<Command> {
        blocks::lexical_commands(text, kinds)
    }

    fn is_statement(&self, block: &Block, kinds: &HashMap<String, String>) -> bool {
        match kinds.get(&block.name) {
            Some(k) => k.starts_with("thy_goal"),
            None => matches!(block.name.as_str(), "lemma" | "theorem" | "corollary" | "proposition" | "schematic_goal" | "lemmas"),
        }
    }

    fn unfinished(&self) -> &'static [&'static str] {
        &["sorry", "oops"]
    }

    fn placeholder(&self) -> &'static str {
        "sorry"
    }

    fn symbols(&self, from_host: Option<&Arc<dyn Symbols>>) -> Arc<dyn Symbols> {
        symbols::for_session(from_host)
    }

    fn has_symbol_table(&self) -> bool {
        true
    }

    fn name_at(&self, text: &str, at: usize) -> Option<String> {
        isar::name_at(text, at)
    }

    fn prose(&self, body: &str, codec: &dyn Codec) -> String {
        literate::to_markdown(body, codec)
    }

    fn snippet_at(&self, text: &str, at: usize) -> Option<String> {
        literate::snippet_at(text, at)
    }

    fn shown_prose(&self, text: &str, caret: Option<usize>, symbols: &dyn Symbols) -> String {
        prose::shown_text_at(text, caret, symbols)
    }

    fn prose_markup(&self, decoded: &Decoded, caret: Option<usize>) -> Vec<Markup> {
        prose::prose_markup(decoded, caret)
    }

    fn prose_code_classes(&self, text: &str) -> Vec<Option<Hl>> {
        prose::prose_code_classes(text)
    }

    fn bare_prose(&self, around: &str) -> bool {
        prose::bare(around)
    }

    fn comments(&self, text: &str) -> Vec<(usize, usize)> {
        isar::comments(text)
    }

    fn derived_colours(&self, text: &str, colours: &[(usize, usize, Sem)]) -> Vec<(usize, usize, Sem)> {
        isar::paired_brackets(text, colours)
    }

    fn next_indent(&self, line: &str) -> String {
        isar::next_indent(line)
    }

    fn construct_at(&self, text: &str, commands: &[&Command], at: usize) -> Option<(usize, usize)> {
        isar::construct_at(text, commands, at)
    }

    fn parse_goal(&self, state: &str) -> Option<Goal> {
        isar::parse_goal(state)
    }

    fn goal_completions(&self, prefix: &str, replace: usize, goal: &Goal) -> Vec<Completion> {
        isar::goal_completions(prefix, replace, goal)
    }

    fn in_term(&self, before: &str) -> bool {
        isar::in_term(before)
    }

    fn quick_entries(&self, context: InsertContext, caps: &Capabilities) -> Vec<PaletteEntry> {
        match context {
            InsertContext::Symbols => isar::QUICK_SYMBOLS.iter().map(|(l, b, a)| PaletteEntry::new(l, b, a)).collect(),
            // On the bar's one row, the proof tools first: they are what is reached for while
            // a goal is shown (sledgehammer, nitpick, …), and the steps follow as room allows
            InsertContext::Proof => {
                let (tools, steps): (Vec<_>, Vec<_>) = isar::PROOF_TEMPLATES
                    .iter()
                    .filter(|(_, _, _, tool)| tool.is_none_or(|t| caps.typed_tool(t).is_some()))
                    .partition(|(_, _, _, tool)| tool.is_some());
                tools.into_iter().chain(steps).map(|(l, b, a, _)| PaletteEntry::new(l, b, a)).collect()
            }
            _ => self.palette(context, caps),
        }
    }

    fn palette(&self, context: InsertContext, caps: &Capabilities) -> Vec<PaletteEntry> {
        match context {
            InsertContext::Symbols => isar::COMMON_SYMBOLS.iter().map(|c| PaletteEntry::new(c, c, "")).collect(),
            InsertContext::Prose => prose::TEMPLATES.iter().map(|(l, b, a)| PaletteEntry::new(l, b, a)).collect(),
            // (a tool's step only when the session has the tool)
            InsertContext::Proof => isar::PROOF_TEMPLATES
                .iter()
                .filter(|(_, _, _, tool)| tool.is_none_or(|t| caps.typed_tool(t).is_some()))
                .map(|(l, b, a, _)| PaletteEntry::new(l, b, a))
                .collect(),
        }
    }

    fn lists(&self, line: &str, symbols: &dyn Symbols) -> Vec<PaletteEntry> {
        prose::lists(line, symbols)
    }

    fn block_templates(&self) -> &'static [BlockTemplate] {
        BLOCK_TEMPLATES
    }

    fn facts_line(&self, facts: &[String], indent: &str) -> String {
        format!("using {}\n{indent}sledgehammer", facts.join(" "))
    }

    fn facts_tool(&self) -> &'static str {
        "sledgehammer"
    }

    fn tool_invocation(&self, name: &str, params: &str) -> String {
        if params.is_empty() { name.to_string() } else { format!("{name} [{params}]") }
    }

    fn tool_waiting_error(&self, text: &str) -> bool {
        isar::waits_on_tool(text)
    }

    fn interrupted(&self, text: &str) -> bool {
        text.contains("Interrupt")
    }

    fn message_summary(&self, text: &str) -> String {
        isar::message_summary(text)
    }

    fn present_goal<'a>(&self, sg: &'a Subgoal) -> GoalPresentation<'a> {
        // As an Isar proof would name them: the variables it fixes, what it assumes, what it shows
        let mut sections = Vec::new();
        if !sg.params.is_empty() {
            sections.push(GoalSection { label: Some("fix"), body: GoalBody::Many(&sg.params) });
        }
        if !sg.prems.is_empty() {
            sections.push(GoalSection { label: Some("assume"), body: GoalBody::Many(&sg.prems) });
        }
        if let Some(c) = &sg.concl {
            sections.push(GoalSection { label: Some("show"), body: GoalBody::One(c) });
        }
        GoalPresentation { heading: None, sections }
    }

    fn facts_label(&self) -> &'static str {
        "using"
    }

    fn default_capabilities(&self) -> Capabilities {
        let s = |s: &str| s.to_string();
        let tool = |name: &str, kind: &str, label: &str| Tool { name: s(name), typed: s(name), kind: s(kind), stop: true, label: s(label), ..Default::default() };
        let setting = |key: &str, kind: &str, label: &str, unit: &str, placeholder: &str, default: &str, option: &str| Setting {
            key: s(key),
            kind: s(kind),
            label: s(label),
            unit: s(unit),
            placeholder: s(placeholder),
            default: s(default),
            option: s(option),
        };
        let sledgehammer = Tool {
            settings: vec![
                setting("provers", "text", "", "", "provers (default: cvc5 verit z3 e spass vampire zipperposition)", "", "sledgehammer_provers"),
                setting("timeout", "number", "Timeout", "seconds per prover", "30", "", "sledgehammer_timeout"),
                setting("isar", "bool", "Isar proofs", "", "", "false", ""),
                setting("try0", "bool", "Try simple methods first", "", "", "true", ""),
            ],
            args: vec![s("provers"), s("isar"), s("try0")],
            running: s("Sledgehammering…"),
            panel_note: s("Runs on the goal at the caret; on a sorry or oops, on the goal before it."),
            typed_note: s("For finer control write sledgehammer [timeout = 120, max_facts = 1000] in the proof; its proofs appear in Output with Insert."),
            settings_note: s("These hold for sledgehammer typed in the text. For one run, write sledgehammer [timeout = 120, isar_proofs] there."),
            ..tool("sledgehammer", "prove", "Sledgehammer")
        };
        Capabilities {
            checking: s("continuous"),
            pause: s("perspective"),
            interrupt: true,
            goals: Goals { structured: true, term_goal: false, facts: true, shelved: false },
            hover: true,
            definition: true,
            spans: s("prover"),
            tools: vec![
                sledgehammer,
                tool("try", "prove", "Try"),
                Tool { quick: true, ..tool("try0", "prove", "Try methods") },
                Tool { quick: true, ..tool("solve_direct", "check", "Solve direct") },
                tool("nitpick", "counterexample", "Nitpick"),
                Tool { params: s("options, e.g. timeout = 60, size = 8, tester = random"), ..tool("quickcheck", "counterexample", "Quickcheck") },
                tool("nunchaku", "counterexample", "Nunchaku"),
            ],
            counterexamples: vec![s("quickcheck"), s("nitpick")],
            search: Some(Search {
                label: s("Find theorems"),
                tab: s("Find"),
                syntax: s("isabelle-find"),
                placeholder: s("\"_ + _ = _ + _\" name: comm"),
                note: s("Theorems in the context at the caret."),
                operation: s("find_theorems"),
                args: vec![s("40"), s("false")],
                ..Default::default()
            }),
            command: ["value", "term", "thm", "typ", "find_consts"].map(s).to_vec(),
            check_candidates: s("query"),
            project_actions: vec![ProjectAction { name: s("build"), label: s("Build heap") }],
            semantic_colours: true,
            documents: vec![s(".thy")],
            project_files: vec![s("ROOT")],
        }
    }
}
