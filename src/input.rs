// SPDX-License-Identifier: GPL-3.0-only

//! Typing characters that are not on the keyboard (provers §7.1): `\` and a
//! name completes to the character, abbreviations such as `==>` become one,
//! and the Symbols panel lists them by group. The table is the backend's
//! (Isabelle's `etc/symbols`, Lean's abbreviations); the fuzzy match is shared.

/// A backend's input table
pub trait Input: Send + Sync {
    /// Characters matching a `\` query by name, LaTeX name or abbreviation, best first:
    /// (character, its name as the backend writes it, what matched)
    fn search(&self, query: &str, limit: usize) -> Vec<(char, String, String)>;

    /// Abbreviations, in table order: (character, abbreviation)
    fn abbreviations(&self) -> Vec<(char, &str)>;

    /// The characters by group, as the Symbols panel shows them
    fn groups(&self) -> Vec<(String, Vec<(char, String)>)>;
}

/// How well `query` matches `name`: prefix, then word starts, then a subsequence
pub fn fuzzy(query: &str, name: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase();
    let n = name.to_lowercase();
    if n == q {
        return Some(10_000);
    }
    if name.starts_with(query) {
        return Some(5000 - name.len() as i32);
    }
    if n.starts_with(&q) {
        return Some(4500 - name.len() as i32);
    }
    if let Some(i) = n.find(&q) {
        return Some(3000 - i as i32 * 10 - name.len() as i32);
    }
    // a subsequence, scored by its gaps
    let mut gaps = 0;
    let mut chars = n.chars();
    let mut last = 0;
    for (k, qc) in q.chars().enumerate() {
        let mut pos = last;
        loop {
            match chars.next() {
                Some(c) if c == qc => break,
                Some(_) => pos += 1,
                None => return None,
            }
        }
        if k > 0 {
            gaps += pos - last;
        }
        last = pos + 1;
    }
    Some(1000 - gaps as i32 * 20 - name.len() as i32)
}
