// SPDX-License-Identifier: GPL-3.0-only

//! The blocks the "+ Text", "+ Section" and "+ Lemma" actions add to a Lean file (design 7.3, as
//! `theory.rs` has `text \<open>…\<close>`, `section` and `lemma` for Isabelle). Text is a module doc,
//! `/-! … -/`; a section is a module doc holding a Markdown heading; a lemma is a `theorem` with
//! `sorry` for its proof, which the prover accepts with a warning until the proof is written.
//!
//! A [`Template`] holds the text and the span to select once it is in, so that the user can type
//! over the placeholder ("Section", the lemma's name). [`Template::insertion`] places it at an
//! offset of a file with a blank line either side. Offsets are in code points.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub text: String,
    /// The span of `text` to select, as code points
    pub select: (usize, usize),
}

/// What [`Template::insertion`] puts in a file
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insertion {
    /// Where to insert (code points)
    pub at: usize,
    /// The text to insert: the template, with blank lines around it as needed
    pub text: String,
    /// The span to select, as code points of the file after the insertion
    pub select: (usize, usize),
}

fn template(before: &str, placeholder: &str, after: &str) -> Template {
    let start = before.chars().count();
    Template { text: format!("{before}{placeholder}{after}"), select: (start, start + placeholder.chars().count()) }
}

/// "+ Text": an empty module doc with the caret in it
pub fn text() -> Template {
    template("/-!\n", "", "\n-/")
}

/// "+ Section": a module doc with a heading of `level` (1 is `#`), its title selected
pub fn section(level: u8) -> Template {
    let hashes = "#".repeat(level.clamp(1, 6) as usize);
    template(&format!("/-!\n{hashes} "), "Section", "\n-/")
}

/// "+ Lemma": `theorem name : _ := by\n  sorry`, the name selected
pub fn lemma(name: &str) -> Template {
    template("theorem ", if name.is_empty() { "name" } else { name }, " : _ := by\n  sorry")
}

/// A name for a new lemma that no word of `source` uses: `base`, or `base_1`, `base_2`, …
pub fn fresh_name(source: &str, base: &str) -> String {
    let used = |name: &str| {
        source.match_indices(name).any(|(i, _)| {
            let before = source[..i].chars().next_back();
            let after = source[i + name.len()..].chars().next();
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '\'' | '.'));
            !word(before) && !word(after)
        })
    };
    if !used(base) {
        return base.to_string();
    }
    (1..).map(|n| format!("{base}_{n}")).find(|n| !used(n)).unwrap()
}

impl Template {
    /// Where and what to insert so that the template is a block of its own at `at` in `source`:
    /// separated from the text before and after by one blank line (a file's start and end need
    /// none before and after, and an existing blank line is not doubled)
    pub fn insertion(&self, source: &str, at: usize) -> Insertion {
        let chars: Vec<char> = source.chars().collect();
        let at = at.min(chars.len());
        let before_blank = if at == 0 || chars[..at].iter().all(|c| c.is_whitespace()) {
            0
        } else {
            // newlines at the end of the text before `at`, ignoring other blanks on the last line
            let trailing = chars[..at].iter().rev().take_while(|c| c.is_whitespace()).filter(|c| **c == '\n').count();
            2usize.saturating_sub(trailing)
        };
        let after_blank = if chars[at..].iter().all(|c| c.is_whitespace()) {
            // (at the end of the file: one final newline)
            if chars[at..].iter().filter(|c| **c == '\n').count() >= 1 { 0 } else { 1 }
        } else {
            let leading = chars[at..].iter().take_while(|c| c.is_whitespace()).filter(|c| **c == '\n').count();
            2usize.saturating_sub(leading)
        };
        let head = "\n".repeat(before_blank);
        let text = format!("{head}{}{}", self.text, "\n".repeat(after_blank));
        let shift = at + head.chars().count();
        Insertion { at, text, select: (shift + self.select.0, shift + self.select.1) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(source: &str, ins: &Insertion) -> String {
        let chars: Vec<char> = source.chars().collect();
        let mut out: String = chars[..ins.at].iter().collect();
        out.push_str(&ins.text);
        out.extend(&chars[ins.at..]);
        out
    }

    fn selected(text: &str, ins: &Insertion) -> String {
        text.chars().skip(ins.select.0).take(ins.select.1 - ins.select.0).collect()
    }

    #[test]
    fn templates_have_their_placeholder_selected() {
        let t = text();
        assert_eq!(t.text, "/-!\n\n-/");
        assert_eq!(t.select, (4, 4), "the caret on the empty line");
        let s = section(1);
        assert_eq!(s.text, "/-!\n# Section\n-/");
        assert_eq!(&s.text[s.select.0..s.select.1], "Section");
        assert_eq!(section(2).text, "/-!\n## Section\n-/");
        assert_eq!(section(9).text, "/-!\n###### Section\n-/");
        let l = lemma("foo_bar");
        assert_eq!(l.text, "theorem foo_bar : _ := by\n  sorry");
        assert_eq!(&l.text[l.select.0..l.select.1], "foo_bar");
        assert_eq!(lemma("").text, "theorem name : _ := by\n  sorry");
    }

    #[test]
    fn insertion_adds_blank_lines_only_where_needed() {
        let src = "def a := 1\n\ndef b := 2\n";
        // between two declarations, at the blank line's end
        let at = "def a := 1\n\n".chars().count();
        let ins = lemma("x").insertion(src, at);
        let out = apply(src, &ins);
        assert_eq!(out, "def a := 1\n\ntheorem x : _ := by\n  sorry\n\ndef b := 2\n");
        assert_eq!(selected(&out, &ins), "x");
        // straight after a line's end
        let at = "def a := 1".chars().count();
        let out = apply(src, &text().insertion(src, at));
        assert_eq!(out, "def a := 1\n\n/-!\n\n-/\n\ndef b := 2\n");
        // at the start of a file, and into an empty one
        let ins = section(1).insertion(src, 0);
        let out = apply(src, &ins);
        assert_eq!(out, "/-!\n# Section\n-/\n\ndef a := 1\n\ndef b := 2\n");
        assert_eq!(selected(&out, &ins), "Section");
        let out = apply("", &lemma("t").insertion("", 0));
        assert_eq!(out, "theorem t : _ := by\n  sorry\n");
        // at the end of a file
        let at = src.chars().count();
        let ins = lemma("end_of_file").insertion(src, at);
        let out = apply(src, &ins);
        assert_eq!(out, "def a := 1\n\ndef b := 2\n\ntheorem end_of_file : _ := by\n  sorry\n");
        assert_eq!(selected(&out, &ins), "end_of_file");
    }

    #[test]
    fn selection_counts_code_points() {
        let src = "-- αβγ ∀\n";
        let at = src.chars().count();
        let ins = lemma("λ").insertion(src, at);
        let out = apply(src, &ins);
        assert_eq!(selected(&out, &ins), "λ");
        assert!(out.starts_with("-- αβγ ∀\n\ntheorem λ"));
    }

    #[test]
    fn fresh_names_avoid_what_the_file_uses() {
        assert_eq!(fresh_name("def a := 1", "lemma_1"), "lemma_1");
        assert_eq!(fresh_name("theorem lemma_1 : True := trivial", "lemma_1"), "lemma_1_1");
        assert_eq!(fresh_name("theorem t : True := lemma_1_1", "lemma_1"), "lemma_1", "a longer word is not a use");
        assert_eq!(fresh_name("x lemma_1 lemma_1_1 lemma_1_2", "lemma_1"), "lemma_1_3");
        assert_eq!(fresh_name("Foo.lemma_1", "lemma_1"), "lemma_1");
    }
}
