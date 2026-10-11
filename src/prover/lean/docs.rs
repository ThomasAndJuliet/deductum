// SPDX-License-Identifier: GPL-3.0-only

//! Lean doc comments as Markdown, with a map back to the source (design 8.1). `/-! … -/` (a
//! module doc) and `/-- … -/` (a docstring) hold Markdown already, with `$…$` and `$$…$$` for
//! maths as doc-gen4 reads them, and backticks for Lean code. So the conversion keeps every
//! character of the text and only takes away what is not prose: the opening `/-!` or `/--`, the
//! closing `-/`, the line break that follows the opener or precedes the closer, and the common
//! indentation of the lines. Because it only deletes, each piece of Markdown stands for an
//! equal-length piece of source, and the map is a short list of such pieces ([`Docs`]).
//!
//! The Markdown is shown rendered and edited a paragraph at a time. A click in a rendered
//! paragraph opens it with the caret where it landed ([`Docs::source_char`]), and spelling marks
//! on the Markdown land on source text. The other direction ([`Docs::markdown_offset`]) places
//! the Markdown caret from a source caret; a source offset in text that was deleted (the
//! indentation, a delimiter) moves to the next Markdown character.
//!
//! Offsets into the Markdown are bytes, like prose-render's `Rewrite`. Source offsets are given
//! both as bytes and as code points: the app's source offsets are code points. All source offsets
//! are relative to the comment text passed in; add the comment's own start to get a position in
//! the file.
//!
//! [`snippet_at`] finds the maths or inline code around a caret in prose (the comment's source
//! or its Markdown alike), for the live preview shown while a paragraph is edited as text.

/// Which kind of comment the text was
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delimiter {
    /// `/-! … -/`
    Module,
    /// `/-- … -/`
    Doc,
    /// No opener: the text is taken as the Markdown body as it stands
    Bare,
}

/// A run of Markdown that is a verbatim copy of source
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seg {
    /// Start in the Markdown (bytes)
    md: usize,
    len: usize,
    /// Start in the source, in bytes and in code points
    byte: usize,
    ch: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Docs {
    pub markdown: String,
    pub delimiter: Delimiter,
    /// The source (bytes) between the delimiters, with the whitespace around the text left in it
    pub body: (usize, usize),
    segs: Vec<Seg>,
    /// Where the Markdown would start if it were empty
    origin: (usize, usize),
}

/// A comment's delimiter and the byte range of what is inside it
fn split_comment(comment: &str) -> (Delimiter, usize, usize) {
    let lead = comment.len() - comment.trim_start().len();
    let rest = &comment[lead..];
    let (delimiter, open) = if rest.starts_with("/-!") {
        (Delimiter::Module, 3)
    } else if rest.starts_with("/--") && !rest.starts_with("/--/") {
        (Delimiter::Doc, 3)
    } else {
        return (Delimiter::Bare, 0, comment.len());
    };
    let start = lead + open;
    let trimmed = comment.trim_end();
    let end = if trimmed.len() >= start + 2 && trimmed.ends_with("-/") { trimmed.len() - 2 } else { comment.len() };
    (delimiter, start, end.max(start))
}

/// The Markdown of a doc comment (given with its delimiters), and the map back to the source
pub fn to_markdown(comment: &str) -> Docs {
    let (delimiter, start, end) = split_comment(comment);
    // The body's lines: content start (after any `\r`-less end), content end, and where the newline is
    struct Line {
        from: usize,
        to: usize,
        newline: Option<usize>,
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut at = start;
    loop {
        match comment[at..end].find('\n') {
            Some(k) => {
                let nl = at + k;
                let to = if nl > at && comment.as_bytes()[nl - 1] == b'\r' { nl - 1 } else { nl };
                lines.push(Line { from: at, to, newline: Some(nl) });
                at = nl + 1;
            }
            None => {
                lines.push(Line { from: at, to: end, newline: None });
                break;
            }
        }
    }
    let blank = |l: &Line| comment[l.from..l.to].trim().is_empty();
    // (the text on the opener's line is trimmed on its own: its indent is the space after `/--`)
    while lines.first().is_some_and(blank) && lines.len() > 1 {
        lines.remove(0);
    }
    while lines.len() > 1 && lines.last().is_some_and(blank) {
        lines.pop();
    }
    if lines.len() == 1 && blank(&lines[0]) {
        lines.clear();
    }
    // The first line is "on the opener" only if nothing before it was dropped
    let first_is_opener_line = lines.first().is_some_and(|l| l.from == start);
    let indent_of = |l: &Line| comment[l.from..l.to].len() - comment[l.from..l.to].trim_start_matches([' ', '\t']).len();
    let common = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| !blank(l) && !(*i == 0 && first_is_opener_line))
        .map(|(_, l)| indent_of(l))
        .min()
        .unwrap_or(0);
    let mut markdown = String::new();
    let mut segs: Vec<Seg> = Vec::new();
    let char_of = |byte: usize| comment[..byte].chars().count();
    let count = lines.len();
    for (i, l) in lines.iter().enumerate() {
        let skip = if blank(l) {
            l.to - l.from
        } else if i == 0 && first_is_opener_line {
            indent_of(l)
        } else {
            common.min(indent_of(l))
        };
        let mut from = l.from + skip;
        let mut to = l.to;
        if i == count - 1 {
            // (the last line loses the space before `-/`)
            to = from + comment[from..to].trim_end().len();
        }
        if to < from {
            from = to;
        }
        if to > from {
            segs.push(Seg { md: markdown.len(), len: to - from, byte: from, ch: char_of(from) });
            markdown.push_str(&comment[from..to]);
        }
        if i + 1 < count {
            let nl = l.newline.unwrap_or(l.to);
            segs.push(Seg { md: markdown.len(), len: 1, byte: nl, ch: char_of(nl) });
            markdown.push('\n');
        }
    }
    let origin = lines.first().map_or((start, char_of(start)), |l| (l.from, char_of(l.from)));
    Docs { markdown, delimiter, body: (start, end), segs, origin }
}

impl Docs {
    /// The source byte offset standing for byte `at` of the Markdown. The end of the Markdown is
    /// the end of its last piece of source
    pub fn source_byte(&self, at: usize) -> usize {
        match self.seg_at(at) {
            Some((s, off)) => s.byte + off,
            None => self.origin.0,
        }
    }

    /// As [`Docs::source_byte`], in code points of the source
    pub fn source_char(&self, at: usize) -> usize {
        match self.seg_at(at) {
            Some((s, off)) => s.ch + self.markdown[s.md..s.md + off].chars().count(),
            None => self.origin.1,
        }
    }

    /// The verbatim pieces: (Markdown byte start, byte length, source code point start)
    pub fn segments(&self) -> Vec<(usize, usize, usize)> {
        self.segs.iter().map(|s| (s.md, s.len, s.ch)).collect()
    }

    /// The same with its soft line breaks read as Markdown reads them (see [`reflow`]): each is one
    /// space, and the indentation the next line has in the source is dropped, so that a wrapped
    /// list item does not show a gap. The map follows: the space stands for the line break, and
    /// the dropped indentation stands for nothing
    pub fn flowed(&self) -> Docs {
        let flowed = reflow(&self.markdown);
        let mut markdown = String::with_capacity(self.markdown.len());
        let mut segs: Vec<Seg> = Vec::new();
        let mut after_soft = false;
        for s in &self.segs {
            let text = &self.markdown[s.md..s.md + s.len];
            if text == "\n" {
                let soft = flowed.as_bytes()[s.md] == b' ';
                segs.push(Seg { md: markdown.len(), len: 1, byte: s.byte, ch: s.ch });
                markdown.push(if soft { ' ' } else { '\n' });
                after_soft = soft;
                continue;
            }
            let skip = if after_soft { text.len() - text.trim_start_matches([' ', '\t']).len() } else { 0 };
            after_soft = false;
            if skip < s.len {
                segs.push(Seg { md: markdown.len(), len: s.len - skip, byte: s.byte + skip, ch: s.ch + skip });
                markdown.push_str(&text[skip..]);
            }
        }
        Docs { markdown, segs, ..self.clone() }
    }

    /// The source range of a Markdown byte range, in code points
    pub fn source_chars(&self, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
        let start = self.source_char(range.start);
        start..self.source_char(range.end).max(start)
    }

    fn seg_at(&self, at: usize) -> Option<(&Seg, usize)> {
        let at = at.min(self.markdown.len());
        let i = self.segs.partition_point(|s| s.md <= at);
        let s = self.segs.get(i.checked_sub(1)?)?;
        Some((s, (at - s.md).min(s.len)))
    }

    /// The Markdown byte offset of source byte `byte`: in text that is not in the Markdown (the
    /// delimiters, indentation), the next Markdown character
    pub fn markdown_offset(&self, byte: usize) -> usize {
        let i = self.segs.partition_point(|s| s.byte + s.len <= byte);
        match self.segs.get(i) {
            Some(s) if byte >= s.byte => s.md + (byte - s.byte),
            Some(s) => s.md,
            None => self.markdown.len(),
        }
    }

    /// As [`Docs::markdown_offset`], from a code point offset of the source
    pub fn markdown_offset_of_char(&self, ch: usize) -> usize {
        let i = self.segs.partition_point(|s| s.ch + self.markdown[s.md..s.md + s.len].chars().count() <= ch);
        match self.segs.get(i) {
            Some(s) if ch >= s.ch => {
                let within = ch - s.ch;
                s.md + self.markdown[s.md..s.md + s.len].char_indices().nth(within).map_or(s.len, |(b, _)| b)
            }
            Some(s) => s.md,
            None => self.markdown.len(),
        }
    }
}

/// The Markdown with each soft line break (a single newline inside a paragraph) a space, as
/// Markdown reads it and as docstrings are written (wrapped at 80 columns). Hard breaks (two spaces
/// or a backslash before the newline), code fences, display maths, headings, list items, quotes
/// and tables keep their line breaks. Each newline becomes one space, so every offset is the same
pub fn reflow(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let block_start = |l: &str| {
        let t = l.trim_start();
        t.starts_with(['#', '>', '|', '-', '*', '+'])
            || t.starts_with("```")
            || t.starts_with("~~~")
            || t.starts_with("$$")
            || t.split_once(['.', ')']).is_some_and(|(n, rest)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && rest.starts_with(' '))
    };
    let mut out = String::with_capacity(markdown.len());
    let (mut fence, mut display) = (false, false);
    for (i, line) in lines.iter().enumerate() {
        out.push_str(line);
        let Some(next) = lines.get(i + 1) else { break };
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
        }
        if !fence && t.matches("$$").count() % 2 == 1 {
            display = !display;
        }
        let soft = !fence
            && !display
            && !line.trim().is_empty()
            && !next.trim().is_empty()
            && !line.ends_with("  ")
            && !line.ends_with('\\')
            && !t.starts_with(['#', '|'])
            && !t.starts_with("$$")
            && !t.trim_end().ends_with("$$")
            && !block_start(next)
            && !t.starts_with("```")
            && !next.starts_with("    ");
        out.push(if soft { ' ' } else { '\n' });
    }
    out
}

/// What a snippet is
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `$…$`, or `$$…$$` when `display`
    Math { display: bool },
    /// `` `…` `` (or a longer backtick run)
    Code,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    /// Code points, delimiters included
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
    pub text: String,
}

/// The maths or inline code around a character offset of prose, if the offset is in (or at the
/// edge of) one: what to show rendered while the prose is edited as source. Fenced code blocks
/// and backslash-escaped `$` are not snippets, and `$` that is plainly money (`$5 and $6`) is not
/// maths: as in Pandoc, an opening `$` needs a non-space after it, a closing one a non-space
/// before it and no digit after it.
pub fn snippet_span_at(text: &str, at: usize) -> Option<Snippet> {
    snippets(text).into_iter().find(|s| s.start <= at && at <= s.end)
}

/// Every maths and inline code snippet of a prose text, in order (see [`snippet_span_at`] for
/// what counts)
pub fn snippets(text: &str) -> Vec<Snippet> {
    let mut out = Vec::new();
    let c: Vec<char> = text.chars().collect();
    let n = c.len();
    // Lines inside fenced code blocks do not hold snippets
    let mut blocked = vec![false; n];
    let mut fence: Option<char> = None;
    let mut i = 0;
    while i < n {
        let end = (i..n).find(|&k| c[k] == '\n').unwrap_or(n);
        let line: String = c[i..end].iter().collect();
        let t = line.trim_start();
        let marker = if t.starts_with("```") { Some('`') } else if t.starts_with("~~~") { Some('~') } else { None };
        let in_fence = fence.is_some();
        match (fence, marker) {
            (None, Some(m)) => fence = Some(m),
            (Some(f), Some(m)) if f == m => fence = None,
            _ => {}
        }
        if in_fence || marker.is_some() {
            for b in &mut blocked[i..end] {
                *b = true;
            }
        }
        i = end + 1;
    }
    // A blank line ends a paragraph, and with it any snippet being looked for
    let paragraph_end = |from: usize| -> usize {
        let mut k = from;
        while k < n {
            if c[k] == '\n' {
                let mut m = k + 1;
                while m < n && (c[m] == ' ' || c[m] == '\t' || c[m] == '\r') {
                    m += 1;
                }
                if m >= n || c[m] == '\n' {
                    return k;
                }
            }
            k += 1;
        }
        n
    };
    let found = |start: usize, end: usize, kind: Kind| Snippet { start, end, kind, text: c[start..end].iter().collect() };
    let mut i = 0;
    while i < n {
        if blocked[i] {
            i += 1;
            continue;
        }
        match c[i] {
            '\\' => i += 2,
            '`' => {
                let run = (i..n).take_while(|&k| c[k] == '`').count();
                let limit = paragraph_end(i);
                let mut j = i + run;
                let mut close = None;
                while j < limit {
                    if c[j] == '`' {
                        let r = (j..n).take_while(|&k| c[k] == '`').count();
                        if r == run {
                            close = Some(j + r);
                            break;
                        }
                        j += r;
                    } else {
                        j += 1;
                    }
                }
                match close {
                    Some(e) => {
                        out.push(found(i, e, Kind::Code));
                        i = e;
                    }
                    None => i += run,
                }
            }
            '$' => {
                let display = c.get(i + 1) == Some(&'$');
                let open = if display { 2 } else { 1 };
                let limit = paragraph_end(i);
                let mut j = i + open;
                let mut close = None;
                if !display && c.get(j).is_none_or(|x| x.is_whitespace()) {
                    i += 1;
                    continue;
                }
                while j < limit {
                    match c[j] {
                        '\\' => j += 2,
                        '$' if display => {
                            if c.get(j + 1) == Some(&'$') {
                                close = Some(j + 2);
                                break;
                            }
                            j += 1;
                        }
                        '$' => {
                            let after_digit = c.get(j + 1).is_some_and(|x| x.is_ascii_digit());
                            if !c[j - 1].is_whitespace() && !after_digit {
                                close = Some(j + 1);
                                break;
                            }
                            j += 1;
                        }
                        _ => j += 1,
                    }
                }
                match close {
                    Some(e) => {
                        out.push(found(i, e, Kind::Math { display }));
                        i = e;
                    }
                    None => i += open,
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// [`snippet_span_at`]'s text
pub fn snippet_at(text: &str, at: usize) -> Option<String> {
    snippet_span_at(text, at).map(|s| s.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Markdown char boundary maps to the source and back to itself, and the characters
    /// either side agree
    fn check_round_trip(comment: &str, d: &Docs) {
        for (m, _) in d.markdown.char_indices().chain(std::iter::once((d.markdown.len(), ' '))) {
            let b = d.source_byte(m);
            assert!(comment.is_char_boundary(b), "{m} -> {b} in {comment:?}");
            assert_eq!(d.markdown_offset(b), m, "byte {b} <- md {m}: {comment:?} / {:?}", d.markdown);
            let c = d.source_char(m);
            assert_eq!(comment[..b].chars().count(), c);
            assert_eq!(d.markdown_offset_of_char(c), m);
            // the same character
            if m < d.markdown.len() {
                assert_eq!(d.markdown[m..].chars().next(), comment[b..].chars().next(), "at md {m}");
            }
        }
    }

    #[test]
    fn module_doc_loses_delimiters_and_indent() {
        let src = "/-!\n  # Title\n\n  Some $x^2$ and `code`.\n    indented more\n-/";
        let d = to_markdown(src);
        assert_eq!(d.delimiter, Delimiter::Module);
        assert_eq!(d.markdown, "# Title\n\nSome $x^2$ and `code`.\n  indented more");
        check_round_trip(src, &d);
        let m = d.markdown.find("x^2").unwrap();
        assert_eq!(&src[d.source_byte(m)..d.source_byte(m) + 3], "x^2");
    }

    #[test]
    fn one_line_docstring() {
        let src = "/-- Adds one. -/";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "Adds one.");
        assert_eq!(d.delimiter, Delimiter::Doc);
        assert_eq!(d.source_byte(0), 4);
        assert_eq!(d.source_byte(d.markdown.len()), 13);
        check_round_trip(src, &d);
        assert_eq!(to_markdown("/-- -/").markdown, "");
        assert_eq!(to_markdown("/--  -/").markdown, "");
        assert_eq!(to_markdown("/-!\n-/").markdown, "");
    }

    #[test]
    fn docstring_starting_on_the_opener_line() {
        let src = "/-- First line\n    continues here\n    and `more` -/";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "First line\ncontinues here\nand `more`");
        check_round_trip(src, &d);
        // the newline stands for the source newline
        let m = d.markdown.find('\n').unwrap();
        assert_eq!(&src[d.source_byte(m)..d.source_byte(m) + 1], "\n");
        // the start of "continues" is after the indent
        let m = d.markdown.find("continues").unwrap();
        assert_eq!(&src[d.source_byte(m)..d.source_byte(m) + 9], "continues");
    }

    #[test]
    fn indent_is_common_and_relative_indent_is_kept() {
        let src = "/--\n  - item\n    - nested\n  - item 2\n-/";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "- item\n  - nested\n- item 2");
        check_round_trip(src, &d);
        // a docstring written flush left
        let src = "/--\nflush\n\n  indented\n-/";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "flush\n\n  indented");
        check_round_trip(src, &d);
    }

    #[test]
    fn windows_line_ends_and_tabs() {
        let src = "/-!\r\n\t# T\r\n\r\n\ttext\r\n-/";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "# T\n\ntext");
        check_round_trip(src, &d);
    }

    #[test]
    fn unicode_offsets_in_bytes_and_code_points() {
        let src = "/-!\n# Größe ∀\n\nWir zeigen α → β, `f ⟨a, b⟩`.\n-/";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "# Größe ∀\n\nWir zeigen α → β, `f ⟨a, b⟩`.");
        check_round_trip(src, &d);
        let m = d.markdown.find('β').unwrap();
        let c = d.source_char(m);
        assert_eq!(src.chars().nth(c), Some('β'));
        assert_eq!(d.source_chars(m..m + 'β'.len_utf8()), c..c + 1);
        assert_eq!(d.markdown_offset_of_char(c), m);
    }

    #[test]
    fn text_without_a_closer_while_typing() {
        let src = "/-!\n# T\n\nstill typing";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "# T\n\nstill typing");
        check_round_trip(src, &d);
        let bare = to_markdown("plain\ntext");
        assert_eq!(bare.delimiter, Delimiter::Bare);
        assert_eq!(bare.markdown, "plain\ntext");
    }

    #[test]
    fn source_in_deleted_text_snaps_to_the_next_markdown_char() {
        let src = "/-!\n  # T\n-/";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "# T");
        // a caret in the indent, in the opener and in the closer
        assert_eq!(d.markdown_offset(0), 0);
        assert_eq!(d.markdown_offset(5), 0);
        assert_eq!(d.markdown_offset(src.len()), 3);
        assert_eq!(d.markdown_offset(src.len() - 3), 3);
    }

    #[test]
    fn empty_lines_and_trailing_whitespace_at_the_end() {
        let src = "/-!\n\n\n# T   \n\ntext  \n\n  -/";
        let d = to_markdown(src);
        assert_eq!(d.markdown, "# T   \n\ntext");
        check_round_trip(src, &d);
    }

    #[test]
    fn real_docs_of_dsrv_round_trip() {
        let Ok(rd) = std::fs::read_dir("/tmp/dsrv-lean-copy/DSRV") else { return };
        let mut seen = 0;
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "lean") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            let mut rest = text.as_str();
            while let Some(i) = rest.find("/-") {
                let after = &rest[i + 2..];
                let Some(j) = after.find("-/") else { break };
                let comment = &rest[i..i + 2 + j + 2];
                if comment.starts_with("/-!") || comment.starts_with("/--") {
                    let d = to_markdown(comment);
                    check_round_trip(comment, &d);
                    seen += 1;
                }
                rest = &rest[i + 2 + j + 2..];
            }
        }
        assert!(seen > 10, "{seen}");
    }

    #[test]
    fn flowed_text_keeps_a_map_and_drops_the_indent_of_wrapped_lines() {
        let src = "/--\n  - one item\n    that wraps\n  - two\n-/";
        let d = to_markdown(src).flowed();
        assert_eq!(d.markdown, "- one item that wraps\n- two");
        for needle in ["one", "wraps", "two"] {
            let m = d.markdown.find(needle).unwrap();
            let at = d.source_byte(m);
            assert_eq!(&src[at..at + needle.len()], needle, "{needle}");
            assert_eq!(d.markdown_offset(at), m);
        }
        // the space stands for the line break
        let m = d.markdown.find("item that").unwrap() + 4;
        assert_eq!(&src[d.source_byte(m)..d.source_byte(m) + 1], "\n");
    }

    #[test]
    fn soft_breaks_become_spaces_and_nothing_else_moves() {
        let md = "A wrapped\nparagraph with `code`\nover three lines.\n\n- item one\n  goes on\n- item two\n\n```lean\ndef a := 1\ndef b := 2\n```\n\n$$\nx = y\n$$\n\n$$a = b$$\nright after\n\nhard  \nbreak\n# Title\nafter";
        let r = reflow(md);
        assert_eq!(r.len(), md.len());
        assert!(r.starts_with("A wrapped paragraph with `code` over three lines.\n\n- item one   goes on\n- item two"), "{r:?}");
        assert!(r.contains("```lean\ndef a := 1\ndef b := 2\n```"));
        assert!(r.contains("$$\nx = y\n$$"));
        assert!(r.contains("$$a = b$$\nright after"), "{r:?}");
        assert!(r.contains("hard  \nbreak\n# Title\nafter"), "{r:?}");
    }

    #[test]
    fn snippets_are_maths_or_code() {
        assert_eq!(snippet_at("so $x + 0 = x$ for", 5).as_deref(), Some("$x + 0 = x$"));
        assert_eq!(snippet_at("so $x + 0 = x$ for", 3).as_deref(), Some("$x + 0 = x$"), "at the edge");
        assert_eq!(snippet_at("so $x + 0 = x$ for", 14).as_deref(), Some("$x + 0 = x$"), "at the far edge");
        assert_eq!(snippet_at("so $x + 0 = x$ for", 16), None);
        assert_eq!(snippet_at("see `Nat.add_comm a b` now", 10).as_deref(), Some("`Nat.add_comm a b`"));
        assert_eq!(snippet_at("a $$\\sum_i x_i$$ b", 6).as_deref(), Some("$$\\sum_i x_i$$"));
        assert_eq!(snippet_at("plain text", 3), None);
        let s = snippet_span_at("ab $x$ `c`", 8).unwrap();
        assert_eq!((s.start, s.end, s.kind), (7, 10, Kind::Code));
        let s = snippet_span_at("ab $x$", 4).unwrap();
        assert_eq!(s.kind, Kind::Math { display: false });
    }

    #[test]
    fn snippet_edge_cases() {
        // money is not maths
        assert_eq!(snippet_at("costs $5 and $6 total", 8), None);
        // a dollar in code, an escaped dollar
        assert_eq!(snippet_at("use `$x` here", 6).as_deref(), Some("`$x`"));
        assert_eq!(snippet_at("a \\$x$ b", 3), None);
        // double backticks hold a single one
        assert_eq!(snippet_at("a ``x ` y`` b", 5).as_deref(), Some("``x ` y``"));
        // maths over a line break, but not over a paragraph
        assert_eq!(snippet_at("a $x\n+ y$ b", 3).as_deref(), Some("$x\n+ y$"));
        assert_eq!(snippet_at("a $x\n\ny$ b", 3), None);
        // an unclosed $ is nothing; a fenced block is not inline
        assert_eq!(snippet_at("a $x b", 3), None);
        assert_eq!(snippet_at("```lean\nexample : `a` = $x$ := rfl\n```", 20), None);
        assert_eq!(snippet_at("```lean\ncode\n```\nthen `x`", 23).as_deref(), Some("`x`"));
        // code points, not bytes
        assert_eq!(snippet_at("α → $β$", 6).as_deref(), Some("$β$"));
    }
}
