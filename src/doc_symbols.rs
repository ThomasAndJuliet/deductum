// SPDX-License-Identifier: GPL-3.0-only

//! The symbols of the open document, for Go to symbol (Tom, 18 Nov 2026: Ctrl+Shift+O, `g s` in
//! vim's normal mode, or `@` typed first in Ctrl+P's search): its headings (the Outline's) and
//! its named formal blocks, the statements of the Theorems list and the definitions (as each
//! backend's profile names them). They come from the document's blocks, so they are there before
//! the prover has checked anything.

use std::cmp::Reverse;

use crate::codec::Symbols;
use crate::editor::Theory;
use crate::theory::{Kind, Status};

/// A symbol of the document
#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    /// The name it defines, or a heading's title (a statement with no name: its first line)
    pub name: String,
    /// The command that makes it (`section`, `lemma`, `definition`, `def`)
    pub kind: String,
    /// Where its block starts and ends in the document (offsets in code points)
    pub start: usize,
    pub end: usize,
    /// A heading's level (1 for a chapter); none for a formal block
    pub level: Option<u8>,
    /// A formal block's state as the prover has checked it (unfinished counted as a warning)
    pub status: Option<Status>,
}

/// The longest a name or title is shown
const LONGEST: usize = 80;

/// The document's symbols, in document order
pub fn symbols(t: &Theory, table: &dyn Symbols) -> Vec<Symbol> {
    let mut out = Vec::new();
    for block in &t.blocks {
        let text = |s: usize, e: usize| table.decode(&t.source_text(s, e)).text;
        match block.kind {
            Kind::Heading(level) => {
                let (s, e) = block.body.unwrap_or((block.start, block.end));
                let title = first_line(&text(s, e)).replace("\\<open>", "").replace("\\<close>", "");
                out.push(Symbol { name: shorten(&title), kind: block.name.clone(), start: block.start, end: block.end, level: Some(level), status: None });
            }
            Kind::Formal if t.profile.is_statement(block, &t.kinds) || t.profile.is_definition(block) => {
                let source = text(block.start, block.end);
                let name = defined_name(&source, &block.name).unwrap_or_else(|| shorten(&from_keyword(&source, &block.name)));
                let unfinished = block.commands.iter().any(|c| t.profile.unfinished().contains(&c.name.as_str()));
                let status = if unfinished && block.status == Status::Finished { Status::Warned } else { block.status };
                out.push(Symbol { name, kind: block.name.clone(), start: block.start, end: block.end, level: None, status: Some(status) });
            }
            _ => {}
        }
    }
    out
}

/// The symbol the caret is in: the last one starting at or before it
pub fn at_caret(symbols: &[Symbol], cursor: usize) -> Option<usize> {
    symbols.iter().rposition(|s| s.start <= cursor)
}

/// The symbols matching `query`, best first: an exact name, then names starting with it, then
/// fuzzy matches of names, in document order among equals. Nothing typed: all of them, in
/// document order
pub fn rank(symbols: &[Symbol], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut matches: Vec<_> = symbols
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let name = s.name.to_lowercase();
            let score = if query.is_empty() {
                (0, 0)
            } else if name == query {
                (3, 0)
            } else if name.starts_with(&query) {
                (2, 0)
            } else {
                (1, crate::input::fuzzy(&query, &name)?)
            };
            Some((Reverse(score), i))
        })
        .collect();
    matches.sort_unstable();
    matches.into_iter().map(|(_, i)| i).collect()
}

/// The name a block's command defines: the word after the command (`lemma foo:`, `def foo (x`,
/// `fun foo ::`), past attributes and modifiers before the command (`@[simp]`, `private`) and type
/// parameters or a target after it (`datatype ('a, 'b) tree`, `lemma (in group) one`); none when
/// the command names nothing (`lemma "x = x"`, `instance : Foo Nat`)
pub fn defined_name(source: &str, keyword: &str) -> Option<String> {
    let mut words = source.split_whitespace().skip_while(|w| *w != keyword).skip(1).peekable();
    // (type parameters and parenthesised groups before the name)
    while let Some(w) = words.peek() {
        if w.starts_with('\'') {
            words.next();
        } else if w.starts_with('(') {
            for w in words.by_ref() {
                if w.ends_with(')') {
                    break;
                }
            }
        } else {
            break;
        }
    }
    let word = words.next()?;
    let name = word.split(['[', ':', '(', '{', '=']).next().unwrap_or("");
    let names_nothing = name.is_empty() || name.starts_with(['"', '\u{2039}', '\\', '`']) || matches!(name, "where" | "for" | "and" | "is" | "begin");
    (!names_nothing).then(|| shorten(name))
}

fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string()
}

/// A block's text from its command on, on one line (an unnamed statement's label)
fn from_keyword(source: &str, keyword: &str) -> String {
    let at = source.split_whitespace().position(|w| w == keyword).unwrap_or(0);
    source.split_whitespace().skip(at).collect::<Vec<_>>().join(" ")
}

fn shorten(text: &str) -> String {
    if text.chars().count() > LONGEST { format!("{}\u{2026}", text.chars().take(LONGEST).collect::<String>()) } else { text.to_string() }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::editor::Mode;

    fn theory(path: &str, text: &str, profile: &'static dyn crate::prover::Profile) -> Theory {
        let table = profile.symbols(None);
        let doc = serde_json::json!({"text": text, "revision": 1});
        Theory::new(path.into(), &doc, &*table, Arc::new(HashMap::new()), Mode::Literate, profile)
    }

    fn listed(t: &Theory) -> Vec<(String, String)> {
        let table = t.profile.symbols(None);
        symbols(t, &*table).into_iter().map(|s| (s.kind, s.name)).collect()
    }

    #[test]
    fn an_isabelle_theory_s_headings_statements_and_definitions() {
        let text = concat!(
            "theory Demo imports Main begin\n\n",
            "section \\<open>Lists\\<close>\n\n",
            "text \\<open>Some prose.\\<close>\n\n",
            "datatype ('a, 'b) tree = Leaf | Node \"('a, 'b) tree\"\n\n",
            "fun size :: \"('a, 'b) tree \\<Rightarrow> nat\" where\n  \"size Leaf = 0\"\n\n",
            "subsection \\<open>Facts\\<close>\n\n",
            "lemma size_zero[simp]: \"size Leaf = 0\"\n  by simp\n\n",
            "lemma \"True\"\n  sorry\n\n",
            "notation size (\"|_|\")\n\n",
            "end\n",
        );
        let t = theory("/Demo.thy", text, crate::prover::profile(None));
        assert_eq!(
            listed(&t),
            [
                ("section".to_string(), "Lists".to_string()),
                ("datatype".into(), "tree".into()),
                ("fun".into(), "size".into()),
                ("subsection".into(), "Facts".into()),
                ("lemma".into(), "size_zero".into()),
                ("lemma".into(), "lemma \"True\" sorry".into()),
            ]
        );
        let table = t.profile.symbols(None);
        let all = symbols(&t, &*table);
        assert_eq!((all[0].level, all[3].level, all[4].level), (Some(2), Some(3), None));
        assert!(all[0].status.is_none() && all[4].status.is_some(), "headings have no status, formal blocks do");
    }

    #[test]
    fn a_lean_file_s_declarations_past_attributes_and_modifiers() {
        let text = "/-! # Basics -/\n\n@[simp]\ntheorem add_zero' (n : Nat) : n + 0 = n := rfl\n\nprivate\nnoncomputable def f := 1\n\nstructure Point where\n  x : Nat\n\ninstance : Inhabited Point := ⟨⟨0⟩⟩\n\n#check f\n";
        let t = theory("/Basics.lean", text, crate::prover::profile(Some("lean")));
        let named: Vec<_> = listed(&t).into_iter().filter(|(k, _)| k != "#check").collect();
        assert!(named.contains(&("theorem".into(), "add_zero'".into())), "{named:?}");
        assert!(named.contains(&("def".into(), "f".into())), "{named:?}");
        assert!(named.contains(&("structure".into(), "Point".into())), "{named:?}");
        assert!(named.iter().any(|(k, n)| k == "instance" && n.starts_with("instance : Inhabited")), "{named:?}");
    }

    #[test]
    fn names_are_found_after_the_command() {
        assert_eq!(defined_name("lemma foo: \"x\"", "lemma").as_deref(), Some("foo"));
        assert_eq!(defined_name("lemma (in group) one_unique[simp]:", "lemma").as_deref(), Some("one_unique"));
        assert_eq!(defined_name("definition sample::bool where", "definition").as_deref(), Some("sample"));
        assert_eq!(defined_name("datatype 'a list = Nil", "datatype").as_deref(), Some("list"));
        assert_eq!(defined_name("@[simp]\ntheorem foo (n : Nat) : n = n", "theorem").as_deref(), Some("foo"));
        assert_eq!(defined_name("lemma \"x = x\"", "lemma"), None);
        assert_eq!(defined_name("lemma [simp]: \"x = x\"", "lemma"), None);
        assert_eq!(defined_name("axiomatization where ax: \"False\"", "axiomatization"), None);
        assert_eq!(defined_name("instance : Foo Nat", "instance"), None);
    }

    fn symbol(name: &str, start: usize) -> Symbol {
        Symbol { name: name.into(), kind: "lemma".into(), start, end: start + 5, level: None, status: None }
    }

    #[test]
    fn an_exact_name_first_then_names_starting_so_then_fuzzy_ones_in_document_order() {
        let all = [symbol("size_tree", 0), symbol("tree_size", 10), symbol("size", 20), symbol("resize", 30), symbol("other", 40)];
        let names = |q: &str| rank(&all, q).into_iter().map(|i| all[i].name.clone()).collect::<Vec<_>>();
        assert_eq!(names("size"), ["size", "size_tree", "resize", "tree_size"], "a closer match first among fuzzy ones");
        assert_eq!(names(""), ["size_tree", "tree_size", "size", "resize", "other"]);
        assert_eq!(names("SZ"), ["size", "resize", "size_tree", "tree_size"], "a subsequence, the tightest first, whatever the case");
        assert!(names("qq").is_empty());
        assert_eq!(at_caret(&all, 25), Some(2));
        assert_eq!(at_caret(&all, 0), Some(0));
    }
}
