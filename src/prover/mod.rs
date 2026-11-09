// SPDX-License-Identifier: GPL-3.0-only

//! What the app knows about each backend (design: docs/deductum-provers.md §7).
//!
//! Two sources of backend knowledge drive the app. A **profile**, compiled in,
//! one per backend, holds what the app must know before the prover answers or
//! without it: file types, how a document splits into blocks, prose syntax and
//! its conversion to Markdown, the symbol codec, colours found lexically,
//! indentation, the construct the goal is for, completions, palette entries and
//! block templates. The **capabilities**, sent by the host per session
//! ([`Capabilities`]), say what this session's driver and project can do; the
//! UI shows a panel, setting, button or palette entry only when they offer it.
//!
//! A document's profile is that of its session, chosen by the session's
//! `backend`. The generic code (the replica and its undo steps, the block
//! editors, the layout, vim, collaboration) asks the profile and never names a
//! prover. Isabelle is the only profile so far; [`profile`] is where the next
//! one is registered.

pub mod capabilities;
pub mod isabelle;
pub mod lean;

use std::collections::HashMap;
use std::sync::Arc;

pub use capabilities::{Capabilities, Setting, Tool};

use crate::codec::{Codec, Decoded, Symbols};
use crate::editor::{Completion, Goal, Hl, Markup, Sem};
use crate::goals::Subgoal;
use crate::theory::{Block, Command};

/// The Isabelle profile
pub static ISABELLE: isabelle::Isabelle = isabelle::Isabelle;

/// The Lean 4 profile
pub static LEAN: lean::Lean = lean::Lean;

/// The profile for a session's `backend`. A session summary without one comes from a host
/// older than backends, which ran Isabelle only
pub fn profile(backend: Option<&str>) -> &'static dyn Profile {
    match backend.unwrap_or("isabelle") {
        "lean" => &LEAN,
        _ => &ISABELLE,
    }
}

/// Every profile, for what is decided before a session is known (a path on the command line)
pub fn profiles() -> [&'static dyn Profile; 2] {
    [&ISABELLE, &LEAN]
}

/// Colour ```lean and ```isabelle code in prose with the profiles' lexers (once, at startup)
pub fn register_highlighters() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        prose_render::highlight::register("lean", &["lean4"], lean::highlight);
        prose_render::highlight::register("isabelle", &["isar", "thy"], isabelle::highlight);
    });
}

/// The symbol table a host sent (`symbol_table`), kept per machine. Only Isabelle's driver has
/// one (its runtime's `etc/symbols`); other backends' hosts reply with none
pub fn host_symbols(symbols: &serde_json::Value) -> Option<Arc<dyn Symbols>> {
    isabelle::symbols::from_host(symbols)
}

/// What the caret is in, for the insert palette: maths or a term (symbols), prose (text
/// templates), or a proof
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsertContext {
    Symbols,
    Prose,
    Proof,
}

/// An entry of the insert palette: its label, and what it puts before and after the selection
/// (or at the caret, the caret then between)
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteEntry {
    pub label: String,
    pub before: String,
    pub after: String,
}

impl PaletteEntry {
    pub fn new(label: &str, before: &str, after: &str) -> Self {
        Self { label: label.into(), before: before.into(), after: after.into() }
    }
}

/// A block the page's `+ Text`, `+ Section`, `+ Lemma` and `+ Definition` buttons add
#[derive(Clone, Copy, Debug)]
pub struct BlockTemplate {
    /// What the buttons ask for (`text`, `section`, `lemma`, `definition`)
    pub kind: &'static str,
    pub label: &'static str,
    pub body: &'static str,
    /// Where the caret goes in the new block (line, byte column), else at its text's end
    pub caret: Option<(usize, usize)>,
}

/// A subgoal laid out for reading: a heading, then sections of parts, each with a label in
/// the column on its left or none
pub struct GoalPresentation<'a> {
    /// A line above the sections (Lean's `case succ`)
    pub heading: Option<String>,
    pub sections: Vec<GoalSection<'a>>,
}

pub struct GoalSection<'a> {
    /// What the section is (Isabelle's `fix`, `assume`, `show`; Lean's `⊢`)
    pub label: Option<&'static str>,
    pub body: GoalBody<'a>,
}

/// Parts of a subgoal, as the host sends them (messages shown as code)
pub enum GoalBody<'a> {
    /// One part on its own (a conclusion)
    One(&'a serde_json::Value),
    /// Parts one under another (variables, assumptions)
    Many(&'a [serde_json::Value]),
}

/// Where each piece of a prose block's Markdown came from: the Markdown, and verbatim runs of it
/// that are copies of the source (Isabelle's conversion rewrites too much to have one; Lean's only
/// deletes delimiters and indentation, so it does). Offsets in the Markdown are bytes, in the source
/// (the block's body) code points
#[derive(Clone, Debug, PartialEq)]
pub struct ProseMap {
    pub markdown: String,
    /// (Markdown byte start, byte length, source code point start), in order
    pub runs: Vec<(usize, usize, usize)>,
}

impl ProseMap {
    /// The source offset standing for Markdown byte `at` (the end of the last run at the end)
    pub fn source_char(&self, at: usize) -> usize {
        let at = at.min(self.markdown.len());
        let i = self.runs.partition_point(|r| r.0 <= at);
        match i.checked_sub(1).map(|i| self.runs[i]) {
            Some((md, len, src)) => src + self.markdown[md..(md + len).min(at)].chars().count(),
            None => 0,
        }
    }

    /// The Markdown byte offset of source offset `src`: in source the Markdown lacks, the next
    /// Markdown character
    pub fn markdown_byte(&self, src: usize) -> usize {
        for &(md, len, s) in &self.runs {
            let n = self.markdown[md..md + len].chars().count();
            if src < s {
                return md;
            }
            if src < s + n {
                return md + self.markdown[md..md + len].char_indices().nth(src - s).map_or(len, |(b, _)| b);
            }
        }
        self.markdown.len()
    }
}

/// A backend's lexical and presentational knowledge (design §7.1). Offsets are code points of
/// the source text unless a method says otherwise.
pub trait Profile: Send + Sync {
    /// The backend's name, as session summaries give it (`isabelle`, `lean`)
    fn backend(&self) -> &'static str;

    /// The extensions of this backend's documents (`.thy`)
    fn documents(&self) -> &'static [&'static str];

    /// Whether a path is one of this backend's documents
    fn is_document(&self, path: &str) -> bool {
        self.documents().iter().any(|ext| path.ends_with(ext))
    }

    /// A document's file name without its extension, as the sessions list names it
    fn document_name<'a>(&self, file: &'a str) -> &'a str {
        self.documents().iter().find_map(|ext| file.strip_suffix(ext)).unwrap_or(file)
    }

    // ----- blocks

    /// The document's blocks: from the prover's command spans where it has them (text it has
    /// not seen split lexically), else lexically. `kinds` are the prover's keyword kinds
    fn blocks(&self, text: &str, commands: &[Command], kinds: &HashMap<String, String>) -> Vec<Block>;

    /// Commands found lexically, for keyword colours until the prover's spans arrive
    fn lexical_commands(&self, text: &str, kinds: &HashMap<String, String>) -> Vec<Command>;

    /// Whether a formal block states something to prove, for the Theorems list
    fn is_statement(&self, block: &Block, kinds: &HashMap<String, String>) -> bool;

    /// Commands that leave a proof unfinished (`sorry`, `oops`), listed as Problems
    fn unfinished(&self) -> &'static [&'static str];

    /// The placeholder proof (`sorry`); the prover sees a `claude` line as it (§9.4)
    fn placeholder(&self) -> &'static str;

    // ----- characters

    /// The symbols a session's documents are decoded and typed with, given the table the
    /// session's host sent (if any)
    fn symbols(&self, from_host: Option<&Arc<dyn Symbols>>) -> Arc<dyn Symbols>;

    /// Whether there is a table of symbols worth a panel of its own
    fn has_symbol_table(&self) -> bool;

    /// The name at a source offset, to look up what it is (a library theory's names)
    fn name_at(&self, text: &str, at: usize) -> Option<String>;

    // ----- prose (§8)

    /// The Markdown for a prose block's text (inside its delimiters)
    fn prose(&self, body: &str, codec: &dyn Codec) -> String;

    /// The Markdown of a prose block's text with the map back to the source, for backends whose
    /// conversion keeps the text (so that an offset in the rendered Markdown, such as where a
    /// click landed or a spelling mark, is an offset of the source). `None` where it cannot
    fn prose_map(&self, body: &str) -> Option<ProseMap> {
        let _ = body;
        None
    }

    /// The maths or markup around a character offset of prose being edited, if the offset is
    /// in some: what to show rendered beside the source
    fn snippet_at(&self, text: &str, at: usize) -> Option<String>;

    /// Prose as its editor shows it with the caret at `caret` (decoded offsets): one shown
    /// character for each decoded one, so offsets stay the source's
    fn shown_prose(&self, text: &str, caret: Option<usize>, symbols: &dyn Symbols) -> String {
        let _ = (caret, symbols);
        crate::editor::shown_text(text, true)
    }

    /// Markup in prose that reads as it renders unless the caret is in it
    fn prose_markup(&self, decoded: &Decoded, caret: Option<usize>) -> Vec<Markup> {
        let _ = (decoded, caret);
        Vec::new()
    }

    /// The code in prose being edited (maths source, references), coloured as it reads
    fn prose_code_classes(&self, text: &str) -> Vec<Option<Hl>> {
        vec![None; text.chars().count()]
    }

    /// Whether a prose block's text around its body (`text ‹`, `›`) is only its delimiters, so
    /// that the block is empty when its body is
    fn bare_prose(&self, around: &str) -> bool;

    // ----- colours

    /// Comments, found lexically: coloured whatever the prover says
    fn comments(&self, text: &str) -> Vec<(usize, usize)>;

    /// Colours found from the characters alone, painted under the prover's: what a document
    /// looks like before the prover has answered (Lean's lexer; Isabelle needs none)
    fn lexical_colours(&self, text: &str) -> Vec<(usize, usize, Hl)> {
        let _ = text;
        Vec::new()
    }

    /// Colours the profile adds to the prover's (Isabelle: the other half of a coloured bracket)
    fn derived_colours(&self, text: &str, colours: &[(usize, usize, Sem)]) -> Vec<(usize, usize, Sem)> {
        let _ = (text, colours);
        Vec::new()
    }

    // ----- editing proofs

    /// The indentation for a new line after `line`
    fn next_indent(&self, line: &str) -> String;

    /// The construct the caret (at `at`) is in, among a proof's commands: the goal goes level
    /// with it, and the editor tints it
    fn construct_at(&self, text: &str, commands: &[&Command], at: usize) -> Option<(usize, usize)>;

    /// The first subgoal in the prover's text of a proof state, for completions from it
    fn parse_goal(&self, state: &str) -> Option<Goal>;

    /// Steps built from the goal, for the word being typed at a line's start
    fn goal_completions(&self, prefix: &str, replace: usize, goal: &Goal) -> Vec<Completion>;

    /// Whether the caret, after `before` in a formal block, is inside a term (symbols then)
    fn in_term(&self, before: &str) -> bool;

    /// What the caret, after `before` (the decoded text of a formal block up to it), is in, for
    /// the insert palette and the insert bar above the goal: a term (symbols), a place for a
    /// proof step, or a comment that is prose. By default a term where [`Self::in_term`] says so,
    /// a proof step elsewhere (Isabelle, where everything outside quotes and cartouches is Isar)
    fn formal_context(&self, before: &str) -> InsertContext {
        if self.in_term(before) { InsertContext::Symbols } else { InsertContext::Proof }
    }

    /// The insert palette's entries for a context, as the session's capabilities allow
    fn palette(&self, context: InsertContext, caps: &Capabilities) -> Vec<PaletteEntry>;

    /// The entries the insert bar above the goal shows before anything is searched for, most
    /// wanted first (the bar shows as many as fit on its one row). By default the palette's own
    fn quick_entries(&self, context: InsertContext, caps: &Capabilities) -> Vec<PaletteEntry> {
        self.palette(context, caps)
    }

    /// The palette's list entries for prose whose caret line is `line`: in a list its next
    /// item, elsewhere a list to begin
    fn lists(&self, line: &str, symbols: &dyn Symbols) -> Vec<PaletteEntry>;

    /// The blocks `+ Text`, `+ Section` and `+ Lemma` add
    fn block_templates(&self) -> &'static [BlockTemplate];

    /// Whether a line asks an LLM or a listening agent for a proof (`claude`, `codex`, `@name`)
    fn keyword_line(&self, line: &str) -> bool {
        keywords::is_keyword_line(line)
    }

    /// Where such lines' keywords are (with a model in brackets), to colour them
    fn keyword_tokens(&self, text: &str) -> Vec<(usize, usize)> {
        keywords::tokens(text)
    }

    /// Facts an LLM suggests, written as a step that uses them and a tool to finish with,
    /// indented like the hole on the next line (§9.4)
    fn facts_line(&self, facts: &[String], indent: &str) -> String;

    /// The tool [`Self::facts_line`] finishes with
    fn facts_tool(&self) -> &'static str;

    /// A goal's statement as a step that states it (a `have`), on one line: what "Copy as a have"
    /// copies. Isabelle: `have "P"`; Lean: `have : P`
    fn have_line(&self, statement: &str) -> String;

    /// The same step with a placeholder for its proof, as "Use as the next step" inserts it; the
    /// lines after the first are indented by `indent` and the proof's own step. Isabelle: `have
    /// "P"` then `sorry` under it; Lean: `have : P := by` then `sorry` under it
    fn have_step(&self, statement: &str, indent: &str) -> String;

    /// A tool's text run at the caret with options the human typed
    fn tool_invocation(&self, name: &str, params: &str) -> String;

    /// Whether an error only follows from a proof tool typed in the proof, still running (what
    /// closes the proof fails while the tool leaves it unfinished)
    fn tool_waiting_error(&self, text: &str) -> bool;

    /// Whether a message says a run was interrupted (a tool stopped by the human)
    fn interrupted(&self, text: &str) -> bool;

    /// An error or warning as the page shows it under a block: without what the goal view shows
    fn message_summary(&self, text: &str) -> String {
        text.trim_end().to_string()
    }

    /// The quote mark that sets a name or term off in this prover's messages (Isabelle: `"`;
    /// Lean: `'`), for the summary's code-font terms where its markup says none
    fn message_quote(&self) -> char {
        '"'
    }

    // ----- goals (§9.1)

    /// How a subgoal reads: labels and layout
    fn present_goal<'a>(&self, subgoal: &'a Subgoal) -> GoalPresentation<'a>;

    /// What a goal of the state is called in its panel (`subgoal`; Lean says `goal`)
    fn goal_noun(&self) -> &'static str {
        "subgoal"
    }

    /// Whether `c` can be typed after the `\` of a symbol being completed (Isabelle's names are
    /// letters, digits and `^`; Lean's abbreviations also use punctuation: `\<`, `\.`, `\->`)
    fn symbol_query_char(&self, c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '^'
    }

    /// Whether `\<name>` is the form of a symbol (Isabelle's), so that `\<` begins a name rather
    /// than being a symbol of its own (Lean's `⟨`)
    fn angle_escapes(&self) -> bool {
        true
    }

    /// Whether the first word of each command is a keyword to colour (Isabelle). Lean's commands
    /// include every tactic, whose words are coloured by the lexer where they are keywords
    fn commands_are_keywords(&self) -> bool {
        true
    }

    /// Whether the goal panel offers the prover's `show types` option (Isabelle's)
    fn goal_types_option(&self) -> bool {
        true
    }

    /// The label of the chained facts above the subgoals
    fn facts_label(&self) -> &'static str;

    // ----- capabilities

    /// What a session of this backend can do when its host does not say (a host from before
    /// capabilities), and how the tools the profile knows are presented
    fn default_capabilities(&self) -> Capabilities;
}

/// The `claude`, `codex` and `@name` lines (§9.4): the same in every backend whose strings are
/// in double quotes and whose tactics need no closing period
pub mod keywords {
    /// Whether a line reads `claude` or `codex` (a model may follow in brackets, a hint after
    /// it) or `@name`, outside strings: alone, or ending a proof's start
    pub fn is_keyword_line(line: &str) -> bool {
        let mut quoted = false;
        let mut at_word = true;
        let lc: Vec<char> = line.chars().collect();
        for (i, c) in lc.iter().enumerate() {
            if *c == '"' {
                quoted = !quoted;
            }
            if !quoted && at_word {
                let rest: String = lc[i..].iter().collect();
                // (`@name` asks a listening agent)
                let at_name = rest.strip_prefix('@').map(|r| r.chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-')).count()).filter(|n| *n > 0);
                if let Some(n) = at_name
                    && rest[1 + n..].chars().next().is_none_or(char::is_whitespace)
                    && !rest.contains('"')
                {
                    return true;
                }
                for kw in ["claude", "codex"] {
                    // (a model may follow in brackets: `codex[astra]`)
                    if rest.starts_with(kw) && rest[kw.len()..].chars().next().is_none_or(|c| c.is_whitespace() || c == '[') && !rest.contains('"') {
                        return true;
                    }
                }
            }
            at_word = c.is_whitespace();
        }
        false
    }

    /// Where `claude` or `codex` (and a model in brackets) asks for a proof, as the host finds
    /// such lines: the keyword ending a line or followed by a hint, outside strings (char offsets)
    pub fn tokens(text: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut offset = 0;
        for line in text.split('\n') {
            let chars: Vec<char> = line.chars().collect();
            for i in 0..chars.len() {
                if chars[..i].contains(&'"') || (i > 0 && !matches!(chars[i - 1], ' ' | '\t')) {
                    continue;
                }
                let rest: String = chars[i..].iter().collect();
                let name_len = rest.strip_prefix('@').filter(|r| r.starts_with(|c: char| c.is_ascii_alphabetic())).map(|r| 1 + r.chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-')).count());
                let Some(kw_len) = ["claude", "codex"].into_iter().find(|kw| rest.starts_with(kw)).map(str::len).or(name_len) else { continue };
                let mut end = i + kw_len;
                if chars.get(end) == Some(&'[')
                    && let Some(close) = (end + 1..chars.len()).find(|&k| chars[k] == ']')
                    && chars[end + 1..close].iter().all(|c| c.is_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
                {
                    end = close + 1;
                }
                let after: String = chars[end..].iter().collect();
                if (after.is_empty() || after.starts_with([' ', '\t'])) && !after.contains('"') {
                    out.push((offset + i, offset + end));
                    break;
                }
            }
            offset += chars.len() + 1;
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn keyword_lines_are_found_as_the_host_finds_them() {
            let text = "lemma \"claude\"\n  show ?thesis unfolding f_def claude\n  codex[astra] use foo\n  claudette\n";
            let words: Vec<String> = tokens(text).into_iter().map(|(s, e)| text.chars().skip(s).take(e - s).collect()).collect();
            assert_eq!(words, vec!["claude", "codex[astra]"]);
            assert!(is_keyword_line("  show ?thesis unfolding f_def claude"));
            assert!(is_keyword_line("  @tock try this"));
            assert!(!is_keyword_line("lemma \"claude\""));
            assert!(!is_keyword_line("  claudette"));
        }
    }
}
