//! Plain text as Typst markup (the original's figkit/typst.py `escape`), the one
//! implementation every host uses (through the protocol's `escape` operation).
//!
//! - whitespace runs (Python's `str.split()` set) collapse to one space;
//! - a backslash goes before `\ # $ * _ @ < > [ ] ` ~ /`;
//! - a double hyphen becomes `-\-`; a leading `-`, `+` or `=` is escaped;
//! - the full stop of a leading `1. ` (decimal digits, then a full stop and a space) is escaped.
use regex::Regex;
use std::sync::LazyLock;

const SPECIAL: &str = "\\#$*_@<>[]`~/";
static NUMBERED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)\.(\s)").expect("valid numbered-list pattern"));

/// Whitespace as Python's `str.isspace` (which `str.split()` uses): Rust's set plus U+001C–U+001F.
fn python_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Plain text as Typst markup that typesets to exactly that text.
pub fn escape(text: &str) -> String {
    let text = text
        .split(python_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if SPECIAL.contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    let mut out = out.replace("--", "-\\-");
    if out.starts_with(['-', '+', '=']) {
        out.insert(0, '\\');
    }
    NUMBERED.replace(&out, "${1}\\.${2}").into_owned()
}
