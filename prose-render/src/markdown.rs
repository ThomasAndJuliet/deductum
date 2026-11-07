// SPDX-License-Identifier: GPL-3.0-only

//! Assistant markdown, parsed into the blocks the thread view lays out.
//!
//! Math is parsed with pulldown-cmark's `$…$` and `$$…$$`. Claude also
//! writes `\(…\)`, `\[…\]` and bare `\begin{align}…\end{align}`, which
//! CommonMark would read as escapes, so [`normalize_math`] rewrites them to
//! dollars first, leaving code alone.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    /// The run is LaTeX, set as inline math
    pub math: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Run {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextKind {
    Paragraph,
    Heading(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextBlock {
    pub kind: TextKind,
    /// List nesting
    pub indent: u16,
    /// Blockquote nesting
    pub quote: u16,
    /// A list item's bullet or number, on its first block
    pub marker: Option<String>,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Block {
    Text(TextBlock),
    Code {
        lang: String,
        text: String,
        indent: u16,
        quote: u16,
    },
    /// Display math
    Math {
        latex: String,
        indent: u16,
        quote: u16,
        marker: Option<String>,
    },
    Rule,
    /// Rows of cells; the first row is the header
    Table {
        rows: Vec<Vec<Vec<Run>>>,
    },
    Image {
        url: String,
        alt: String,
        indent: u16,
    },
    /// A chart: a `chartgen` code block's specification
    Chart {
        spec: String,
        indent: u16,
        quote: u16,
    },
    /// A Mermaid diagram: a closed `mermaid` code block's source. One whose
    /// fence is still open, as while it streams in, stays code
    Diagram {
        source: String,
        indent: u16,
        quote: u16,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

impl Doc {
    /// Every formula, inline or display, with whether it is display math
    pub fn formulas(&self) -> impl Iterator<Item = (&str, bool)> {
        self.blocks
            .iter()
            .flat_map(|block| -> Box<dyn Iterator<Item = (&str, bool)>> {
                match block {
                    Block::Math { latex, .. } => Box::new(std::iter::once((latex.as_str(), true))),
                    Block::Text(text) => Box::new(
                        text.runs
                            .iter()
                            .filter(|r| r.style.math)
                            .map(|r| (r.text.as_str(), false)),
                    ),
                    Block::Table { rows } => Box::new(
                        rows.iter()
                            .flatten()
                            .flatten()
                            .filter(|r| r.style.math)
                            .map(|r| (r.text.as_str(), false)),
                    ),
                    _ => Box::new(std::iter::empty()),
                }
            })
    }
}

/// Parse plain text, such as a user's message, as one paragraph per
/// paragraph of the text, keeping its line breaks
pub fn plain(text: &str) -> Doc {
    let blocks = text
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            Block::Text(TextBlock {
                kind: TextKind::Paragraph,
                indent: 0,
                quote: 0,
                marker: None,
                runs: vec![Run {
                    text: p.trim_end().to_string(),
                    style: Style::default(),
                    link: None,
                }],
            })
        })
        .collect();
    Doc { blocks }
}

/// Markdown as a chat takes it: each line break is meant (an agent's
/// reply), a soft break kept as a line break
pub fn parse(markdown: &str) -> Doc {
    parse_with(markdown, false, false)
}

/// Markdown as CommonMark has it, for documents: a soft break (a line
/// ending inside a paragraph) is a space; hard breaks still break. Front
/// matter (a `---` … `---` YAML block at the very start, see
/// [`front_matter_end`]) is left out
pub fn parse_document(markdown: &str) -> Doc {
    parse_with(markdown, true, true)
}

/// [`parse_document`] of a part of a document that does not start it:
/// there is no front matter to leave out, as front matter is only ever at
/// a document's start
pub fn parse_document_part(markdown: &str) -> Doc {
    parse_with(markdown, true, false)
}

fn parse_with(markdown: &str, soft_space: bool, front: bool) -> Doc {
    let source = normalize_math(markdown);
    let options = Options::ENABLE_MATH
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        // Footnotes: a reference shows as its raised label, a definition
        // starts with it
        | Options::ENABLE_FOOTNOTES;
    // A document's front matter is not its text: read as Cerea's own
    // analysis reads it, only a block at the very start ([`front_matter_end`]),
    // and left out, as Export leaves it out, rather than set as a rule and a
    // heading. The same lines further on are read as CommonMark reads them
    let source = match front_matter_end(&source) {
        Some(end) if front => &source[end..],
        _ => &source[..],
    };
    let mut builder = Builder {
        soft_space,
        ..Builder::default()
    };
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) = &event {
            builder.fence_closed = fence_closed(&source[range]);
        }
        builder.event(event);
    }
    builder.flush();
    Doc {
        blocks: builder.blocks,
    }
}

/// Where a document's front matter ends, if it starts with some: the byte
/// after its closing line. Front matter is only a block at the very start
/// of a document, read as pulldown-cmark's YAML metadata scanner reads one:
/// a `---` line (exactly three dashes, then only white space), a next line
/// neither blank nor closing, and a closing `---` or `...` line after it.
/// The same lines further on are not front matter
pub fn front_matter_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let line_end = |at: usize| {
        bytes[at..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |i| at + i)
    };
    let first = line_end(0);
    let line = &bytes[..first];
    if !line.starts_with(b"---")
        || line.get(3) == Some(&b'-')
        || !line[3..].iter().all(|b| b.is_ascii_whitespace())
    {
        return None;
    }
    let next = bytes.get(first + 1..).filter(|n| !n.is_empty())?;
    let blank = next
        .iter()
        .position(|b| !matches!(b, b'\t' | 0x0b | 0x0c | b' '))
        .is_none_or(|i| matches!(next[i], b'\n' | b'\r'));
    if blank || is_metadata_closer(next) {
        return None;
    }
    let mut at = line_end(first + 1);
    while at < bytes.len() {
        at += 1;
        if is_metadata_closer(&bytes[at..]) {
            return Some((line_end(at) + 1).min(bytes.len()));
        }
        at = line_end(at);
    }
    None
}

/// Whether `line` (and what follows it) starts with a line closing front
/// matter, as pulldown-cmark's scanner reads one: `---` or `...`, spaces,
/// then a line's end (`\n`, `\r`) or the text's
pub fn is_metadata_closer(line: &[u8]) -> bool {
    if !(line.starts_with(b"---") || line.starts_with(b"...")) || line.get(3) == Some(&line[0]) {
        return false;
    }
    let after = line[3..]
        .iter()
        .position(|b| *b != b' ')
        .map_or(line.len(), |i| i + 3);
    matches!(line.get(after), None | Some(b'\n' | b'\r'))
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    runs: Vec<Run>,
    kind: Option<TextKind>,
    style: Style,
    link: Option<String>,
    /// Each open list's next number, or `None` for bullets
    lists: Vec<Option<u64>>,
    quote: u16,
    marker: Option<String>,
    code: Option<(String, String)>,
    table: Option<Vec<Vec<Vec<Run>>>>,
    /// A soft break is a space (documents), not a line break (chats)
    soft_space: bool,
    image: Option<(String, String)>,
    /// Whether the fenced code block being built has its closing fence
    fence_closed: bool,
    /// A footnote definition's label, put before its first paragraph
    footnote: Option<String>,
}

impl Builder {
    fn indent(&self) -> u16 {
        self.lists.len() as u16
    }

    fn push_text(&mut self, text: &str, style: Style) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(text);
            return;
        }
        if let Some(rows) = &mut self.table {
            if let Some(cell) = rows.last_mut().and_then(|row| row.last_mut()) {
                cell.push(Run {
                    text: text.to_string(),
                    style,
                    link: self.link.clone(),
                });
            }
            return;
        }
        if let Some(last) = self.runs.last_mut()
            && last.style == style
            && last.link == self.link
            && !style.math
        {
            last.text.push_str(text);
            return;
        }
        self.runs.push(Run {
            text: text.to_string(),
            style,
            link: self.link.clone(),
        });
    }

    /// End the text block being built, if it has any text
    fn flush(&mut self) {
        let runs = std::mem::take(&mut self.runs);
        if runs.iter().all(|run| run.text.trim().is_empty()) {
            return;
        }
        self.blocks.push(Block::Text(TextBlock {
            kind: self.kind.unwrap_or(TextKind::Paragraph),
            indent: self.indent(),
            quote: self.quote,
            marker: self.marker.take(),
            runs,
        }));
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    self.flush();
                    self.kind = Some(TextKind::Paragraph);
                    if let Some(label) = self.footnote.take() {
                        self.push_text(&format!("{label} "), Style::default());
                    }
                }
                Tag::FootnoteDefinition(name) => {
                    self.flush();
                    self.footnote = Some(superscript(&name).unwrap_or_else(|| format!("[{name}]")));
                }
                Tag::Heading { level, .. } => {
                    self.flush();
                    self.kind = Some(TextKind::Heading(match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    }));
                }
                Tag::BlockQuote(_) => {
                    self.flush();
                    self.quote += 1;
                }
                Tag::CodeBlock(kind) => {
                    self.flush();
                    let lang = match kind {
                        CodeBlockKind::Fenced(lang) => lang
                            .split_whitespace()
                            .next()
                            .unwrap_or_default()
                            .to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    self.code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    self.flush();
                    self.lists.push(start);
                }
                Tag::Item => {
                    self.flush();
                    self.kind = Some(TextKind::Paragraph);
                    let marker = match self.lists.last_mut() {
                        Some(Some(n)) => {
                            let marker = format!("{n}.");
                            *n += 1;
                            marker
                        }
                        _ => if self.lists.len() > 1 { "◦" } else { "•" }.to_string(),
                    };
                    self.marker = Some(marker);
                }
                Tag::Emphasis => self.style.italic = true,
                Tag::Strong => self.style.bold = true,
                Tag::Strikethrough => self.style.strike = true,
                Tag::Link { dest_url, .. } => self.link = Some(dest_url.to_string()),
                Tag::Image { dest_url, .. } => {
                    self.image = Some((dest_url.to_string(), String::new()))
                }
                // An HTML block is a paragraph of its own text: not run
                // together with the blocks around it
                Tag::HtmlBlock => self.flush(),
                Tag::Table(_) => {
                    self.flush();
                    self.table = Some(Vec::new());
                }
                Tag::TableHead | Tag::TableRow => {
                    if let Some(rows) = &mut self.table {
                        rows.push(Vec::new());
                    }
                }
                Tag::TableCell => {
                    if let Some(row) = self.table.as_mut().and_then(|rows| rows.last_mut()) {
                        row.push(Vec::new());
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) => {
                    self.flush();
                    self.kind = None;
                }
                TagEnd::BlockQuote(_) => {
                    self.flush();
                    self.quote = self.quote.saturating_sub(1);
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, text)) = self.code.take() {
                        let text = text.strip_suffix('\n').unwrap_or(&text).to_string();
                        let (indent, quote) = (self.indent(), self.quote);
                        self.blocks.push(if lang == "chartgen" {
                            Block::Chart {
                                spec: text,
                                indent,
                                quote,
                            }
                        } else if lang == "mermaid" && self.fence_closed {
                            Block::Diagram {
                                source: text,
                                indent,
                                quote,
                            }
                        } else {
                            Block::Code {
                                lang,
                                text,
                                indent,
                                quote,
                            }
                        });
                    }
                }
                TagEnd::List(_) => {
                    self.flush();
                    self.lists.pop();
                }
                TagEnd::HtmlBlock => self.flush(),
                TagEnd::Item => {
                    self.flush();
                    // An empty item's bullet is its own: not given to the
                    // text after the list (a heading took it)
                    self.marker = None;
                }
                TagEnd::Emphasis => self.style.italic = false,
                TagEnd::Strong => self.style.bold = false,
                TagEnd::Strikethrough => self.style.strike = false,
                TagEnd::Link => self.link = None,
                TagEnd::Image => {
                    if let Some((url, alt)) = self.image.take() {
                        self.flush();
                        self.blocks.push(Block::Image {
                            url,
                            alt,
                            indent: self.indent(),
                        });
                    }
                }
                TagEnd::Table => {
                    if let Some(rows) = self.table.take() {
                        self.blocks.push(Block::Table { rows });
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some((_, code)) = &mut self.code {
                    code.push_str(&text);
                } else {
                    self.push_text(&text, self.style);
                }
            }
            Event::Code(text) => self.push_text(
                &text,
                Style {
                    code: true,
                    ..self.style
                },
            ),
            Event::InlineMath(latex) => self.push_text(
                latex.trim(),
                Style {
                    math: true,
                    ..Style::default()
                },
            ),
            Event::DisplayMath(latex) => {
                if let Some(rows) = &mut self.table {
                    if let Some(cell) = rows.last_mut().and_then(|row| row.last_mut()) {
                        cell.push(Run {
                            text: latex.trim().to_string(),
                            style: Style {
                                math: true,
                                ..Style::default()
                            },
                            link: None,
                        });
                    }
                    return;
                }
                // Display math breaks its paragraph in two. A list item that
                // starts with it keeps its bullet on the formula
                let has_text = self.runs.iter().any(|run| !run.text.trim().is_empty());
                let marker = if has_text {
                    self.flush();
                    None
                } else {
                    self.runs.clear();
                    self.marker.take()
                };
                self.blocks.push(Block::Math {
                    latex: latex.trim().to_string(),
                    indent: self.indent(),
                    quote: self.quote,
                    marker,
                });
            }
            // An agent means its line breaks, as chats take them
            Event::SoftBreak => {
                if self.code.is_none() {
                    self.push_text(if self.soft_space { " " } else { "\n" }, self.style);
                }
            }
            Event::HardBreak => self.push_text("\n", self.style),
            Event::Rule => {
                self.flush();
                self.blocks.push(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                self.marker = Some(if done { "☑" } else { "☐" }.to_string());
            }
            Event::Html(html) | Event::InlineHtml(html) => self.push_text(&html, self.style),
            // A raised label, as the Paper and Text views show it: the
            // name in superscript letters where each has one, else as before
            Event::FootnoteReference(name) => match superscript(&name) {
                Some(raised) => self.push_text(&raised, self.style),
                None => self.push_text(&format!("[{name}]"), self.style),
            },
        }
    }
}

/// `text` in Unicode's superscript letters, if every one has one (rich
/// text sets every run on one baseline, so a raised label is written
/// with them)
fn superscript(text: &str) -> Option<String> {
    const FROM: &str = "0123456789+-=()abcdefghijklmnoprstuvwxyzABDEGHIJKLMNOPRTUVW";
    const TO: [char; 59] = [
        '⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹', '⁺', '⁻', '⁼', '⁽', '⁾', 'ᵃ', 'ᵇ', 'ᶜ',
        'ᵈ', 'ᵉ', 'ᶠ', 'ᵍ', 'ʰ', 'ⁱ', 'ʲ', 'ᵏ', 'ˡ', 'ᵐ', 'ⁿ', 'ᵒ', 'ᵖ', 'ʳ', 'ˢ', 'ᵗ', 'ᵘ', 'ᵛ',
        'ʷ', 'ˣ', 'ʸ', 'ᶻ', 'ᴬ', 'ᴮ', 'ᴰ', 'ᴱ', 'ᴳ', 'ᴴ', 'ᴵ', 'ᴶ', 'ᴷ', 'ᴸ', 'ᴹ', 'ᴺ', 'ᴼ', 'ᴾ',
        'ᴿ', 'ᵀ', 'ᵁ', 'ⱽ', 'ᵂ',
    ];
    if text.is_empty() {
        return None;
    }
    text.chars()
        .map(|c| FROM.chars().position(|f| f == c).map(|i| TO[i]))
        .collect()
}

/// Whether a fenced code block, as its source runs, ends with a closing
/// fence: pulldown-cmark ends an unclosed one at the end of the document
fn fence_closed(block: &str) -> bool {
    let strip = |line: &str| {
        line.trim_start_matches(|c: char| c.is_whitespace() || c == '>')
            .to_string()
    };
    let mut lines = block.trim_end().lines();
    let Some(open) = lines.next().map(strip) else {
        return false;
    };
    let fence: String = open
        .chars()
        .take_while(|c| *c == '`' || *c == '~')
        .collect();
    let Some(close) = lines.last().map(strip) else {
        return false;
    };
    !fence.is_empty()
        && close.starts_with(&fence)
        && close.trim_end().chars().all(|c| fence.starts_with(c))
}

/// Rewrite `\(…\)` as `$…$`, `\[…\]` as `$$…$$`, and wrap bare display
/// environments such as `\begin{align}` in `$$`, outside code
pub fn normalize_math(text: &str) -> String {
    normalize_math_mapped(text).0
}

/// Where a rewritten piece of text came from: `out` in the rewritten text
/// stands for `src` in the original
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rewrite {
    pub src: std::ops::Range<usize>,
    pub out: std::ops::Range<usize>,
}

/// [`normalize_math`], with each rewrite it made, in order, so that offsets
/// into the rewritten text map back to the original: an editor parses the
/// rewritten text and edits the original
pub fn normalize_math_mapped(text: &str) -> (String, Vec<Rewrite>) {
    let (out, rewrites, _) = normalize_math_scan(text);
    (out, rewrites)
}

/// [`normalize_math_mapped`], with the lines it starts in the middle of
/// something — a code fence or a `\[` not yet closed — as ranges of the
/// original: a line starting in none of them is read the same whatever
/// came before it. An incremental parser restarts only at such a line
pub fn normalize_math_scan(text: &str) -> (String, Vec<Rewrite>, Vec<std::ops::Range<usize>>) {
    const ENVIRONMENTS: [&str; 8] = [
        "align",
        "align*",
        "equation",
        "equation*",
        "gather",
        "gather*",
        "multline",
        "eqnarray",
    ];
    let mut out = String::with_capacity(text.len() + 16);
    let mut rewrites = Vec::new();
    let mut insert =
        |out: &mut String, rewrites: &mut Vec<Rewrite>, src: std::ops::Range<usize>, with: &str| {
            let start = out.len();
            out.push_str(with);
            rewrites.push(Rewrite {
                src,
                out: start..out.len(),
            });
        };
    let mut in_fence: Option<String> = None;
    let mut in_display = false;
    let mut line_start = 0;
    let mut held: Vec<std::ops::Range<usize>> = Vec::new();
    for line in text.split_inclusive('\n') {
        let at = line_start;
        line_start += line.len();
        if in_fence.is_some() || in_display {
            match held.last_mut() {
                Some(h) if h.end == at => h.end = line_start,
                _ => held.push(at..line_start),
            }
        }
        let trimmed = line.trim_start();
        if let Some(fence) = &in_fence {
            if trimmed.starts_with(fence.as_str()) {
                in_fence = None;
            }
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let fence: String = trimmed
                .chars()
                .take_while(|c| *c == '`' || *c == '~')
                .collect();
            in_fence = Some(fence);
            out.push_str(line);
            continue;
        }
        // Environments on lines of their own
        let begins = ENVIRONMENTS
            .iter()
            .any(|env| trimmed.starts_with(&format!("\\begin{{{env}}}")));
        if begins && !in_display {
            insert(&mut out, &mut rewrites, at..at, "$$\n");
        }
        let mut chars = line.char_indices().peekable();
        let mut in_code = 0usize;
        while let Some((i, c)) = chars.next() {
            if c == '`' {
                let mut run = 1;
                while chars.peek().is_some_and(|(_, c)| *c == '`') {
                    chars.next();
                    run += 1;
                }
                if in_code == 0 {
                    in_code = run;
                } else if in_code == run {
                    in_code = 0;
                }
                out.extend(std::iter::repeat_n('`', run));
                continue;
            }
            if c == '\\' && in_code == 0 {
                match chars.peek().map(|(_, c)| *c) {
                    Some('(') | Some(')') => {
                        chars.next();
                        insert(&mut out, &mut rewrites, at + i..at + i + 2, "$");
                        continue;
                    }
                    Some('[') => {
                        chars.next();
                        insert(&mut out, &mut rewrites, at + i..at + i + 2, "$$");
                        in_display = true;
                        continue;
                    }
                    Some(']') if in_display => {
                        chars.next();
                        insert(&mut out, &mut rewrites, at + i..at + i + 2, "$$");
                        in_display = false;
                        continue;
                    }
                    Some(next) => {
                        out.push(c);
                        out.push(next);
                        chars.next();
                        continue;
                    }
                    None => {}
                }
            }
            out.push(c);
        }
        let ends = ENVIRONMENTS
            .iter()
            .any(|env| trimmed.trim_end().ends_with(&format!("\\end{{{env}}}")));
        if ends && !in_display {
            // The line's own break stands for the closing fence's line, so
            // the maths ends where its last line does
            let end = at + line.len();
            if out.ends_with('\n') {
                out.pop();
                insert(&mut out, &mut rewrites, end - 1..end, "\n$$\n");
            } else {
                insert(&mut out, &mut rewrites, end..end, "\n$$\n");
            }
        }
    }
    (out, rewrites, held)
}

/// Maps offsets into a rewritten text back to the original
pub struct OffsetMap<'a> {
    rewrites: &'a [Rewrite],
}

impl<'a> OffsetMap<'a> {
    pub fn new(rewrites: &'a [Rewrite]) -> Self {
        OffsetMap { rewrites }
    }

    /// The original offset of rewritten offset `at`. Inside a rewrite, its
    /// start; at its end, the end of what it replaced
    pub fn source(&self, at: usize) -> usize {
        // The last rewrite starting at or before `at`
        let i = self.rewrites.partition_point(|r| r.out.start <= at);
        let Some(r) = i.checked_sub(1).map(|i| &self.rewrites[i]) else {
            return at;
        };
        if at >= r.out.end {
            r.src.end + (at - r.out.end)
        } else if at == r.out.start {
            r.src.start
        } else {
            r.src.start
        }
    }

    pub fn range(&self, range: &std::ops::Range<usize>) -> std::ops::Range<usize> {
        let start = self.source(range.start);
        start..self.source(range.end).max(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn math_runs(doc: &Doc) -> Vec<(String, bool)> {
        doc.formulas().map(|(l, d)| (l.to_string(), d)).collect()
    }

    fn text_of(doc: &Doc) -> String {
        let Block::Text(block) = &doc.blocks[0] else {
            panic!()
        };
        block.runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn soft_breaks_are_line_breaks_in_chats_and_spaces_in_documents() {
        let md = "Written before the `~/shelf`\nmigration, wrapped.  \nA hard break.";
        assert_eq!(
            text_of(&parse(md)),
            "Written before the ~/shelf\nmigration, wrapped.\nA hard break."
        );
        assert_eq!(
            text_of(&parse_document(md)),
            "Written before the ~/shelf migration, wrapped.\nA hard break."
        );
    }

    #[test]
    fn dollars_make_inline_and_display_math() {
        let doc = parse("Let $x^2$ be.\n\n$$\\int_0^1 f$$\n\nDone, costs $5 and $6.");
        assert_eq!(
            math_runs(&doc),
            vec![
                ("x^2".to_string(), false),
                ("\\int_0^1 f".to_string(), true)
            ]
        );
    }

    #[test]
    fn brackets_and_parentheses_become_math_outside_code() {
        let doc = parse("Take \\(a+b\\) and `\\(code\\)`:\n\\[\nE = mc^2\n\\]");
        let runs = math_runs(&doc);
        assert_eq!(runs[0], ("a+b".to_string(), false));
        assert_eq!(runs[1], ("E = mc^2".to_string(), true));
        let Block::Text(block) = &doc.blocks[0] else {
            panic!()
        };
        assert!(
            block
                .runs
                .iter()
                .any(|r| r.style.code && r.text == "\\(code\\)")
        );
    }

    #[test]
    fn bare_align_environments_are_display_math() {
        let doc = parse("So:\n\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\nhence.");
        let runs = math_runs(&doc);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].1);
        assert!(runs[0].0.starts_with("\\begin{align}"));
    }

    #[test]
    fn display_math_inside_a_paragraph_splits_it() {
        let doc = parse("Before $$x$$ after");
        assert_eq!(doc.blocks.len(), 3, "{doc:#?}");
        assert!(matches!(doc.blocks[1], Block::Math { .. }));
    }

    #[test]
    fn rewritten_offsets_map_back() {
        let text = "a \\(x\\) b\n\\[y\\]\n\\begin{align}\nz\n\\end{align}\nc\n";
        let (out, rewrites) = normalize_math_mapped(text);
        assert_eq!(
            out,
            "a $x$ b\n$$y$$\n$$\n\\begin{align}\nz\n\\end{align}\n$$\nc\n"
        );
        assert_eq!(out, normalize_math(text));
        let map = OffsetMap::new(&rewrites);
        // "x" and " b"
        assert_eq!(&text[map.range(&(3..4))], "x");
        assert_eq!(&text[map.range(&(2..5))], "\\(x\\)");
        assert_eq!(&text[map.source(5)..map.source(7)], " b");
        // The display maths, and the environment with its inserted fences
        let env = out.find("$$\n\\begin").unwrap()..out.rfind("$$").unwrap() + 2;
        assert_eq!(&text[map.range(&env)], "\\begin{align}\nz\n\\end{align}");
        assert_eq!(&text[map.source(out.len() - 2)..], "c\n");
    }

    #[test]
    fn closed_mermaid_fences_are_diagrams_and_open_ones_code() {
        let source = "flowchart TD\n    A --> B";
        let doc = parse(&format!("See:\n\n```mermaid\n{source}\n```\n\nDone."));
        assert!(
            matches!(&doc.blocks[1], Block::Diagram { source: s, .. } if s == source),
            "{doc:#?}"
        );
        // Still streaming: the fence is open
        let doc = parse(&format!("See:\n\n```mermaid\n{source}\n"));
        assert!(
            matches!(&doc.blocks[1], Block::Code { lang, .. } if lang == "mermaid"),
            "{doc:#?}"
        );
        // In a list and a quote, and with a longer fence
        let doc = parse(&format!(
            "- item\n\n  ````mermaid\n  {source}\n  ````\n\n> ```mermaid\n> graph LR\n> ```\n"
        ));
        assert!(
            matches!(&doc.blocks[1], Block::Diagram { indent: 1, .. }),
            "{doc:#?}"
        );
        assert!(
            matches!(&doc.blocks[2], Block::Diagram { quote: 1, .. }),
            "{doc:#?}"
        );
        // A shorter fence does not close a longer one
        let doc = parse("````mermaid\ngraph LR\n```\n");
        assert!(matches!(&doc.blocks[0], Block::Code { .. }), "{doc:#?}");
    }

    #[test]
    fn lists_headings_code_and_tables() {
        let doc = parse(
            "# Title\n\n1. one\n2. two\n   - nested\n\n```rust\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| $1$ | 2 |\n",
        );
        let Block::Text(heading) = &doc.blocks[0] else {
            panic!()
        };
        assert_eq!(heading.kind, TextKind::Heading(1));
        let Block::Text(item) = &doc.blocks[2] else {
            panic!()
        };
        assert_eq!(item.marker.as_deref(), Some("2."));
        let Block::Text(nested) = &doc.blocks[3] else {
            panic!()
        };
        assert_eq!((nested.indent, nested.marker.as_deref()), (2, Some("◦")));
        assert!(
            matches!(&doc.blocks[4], Block::Code { lang, text, .. } if lang == "rust" && text == "fn x() {}")
        );
        let Block::Table { rows } = &doc.blocks[5] else {
            panic!()
        };
        assert_eq!(rows.len(), 2);
        assert!(rows[1][0][0].style.math);
    }

    fn all_text(doc: &Doc) -> String {
        doc.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text(t) => Some(t.runs.iter().map(|r| r.text.as_str()).collect::<String>()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    /// The Source view's preview leaves a document's front matter out, as
    /// Export does, rather than setting it as a rule and a heading
    /// (follow-up to item 2); a chat's text is read as before
    #[test]
    fn a_documents_front_matter_is_left_out_of_its_preview_but_not_a_chats() {
        let md = "---\ntitle: Running the house server\nauthor: Juliet\n---\n\n# Running the house server\n\nThe server sits in the hall cupboard.\n";
        let doc = parse_document(md);
        assert_eq!(doc.blocks.len(), 2, "{doc:#?}");
        assert!(
            !doc.blocks.iter().any(|b| matches!(b, Block::Rule)),
            "no rule"
        );
        let Block::Text(first) = &doc.blocks[0] else {
            panic!("{doc:#?}")
        };
        assert_eq!(
            (first.kind, first.runs[0].text.as_str()),
            (TextKind::Heading(1), "Running the house server"),
            "the first block is the heading"
        );
        assert!(!all_text(&doc).contains("author") && !all_text(&doc).contains("title:"));
        // A chat's reply is not a document: a rule and text, as before
        let chat = parse(md);
        assert!(
            chat.blocks.iter().any(|b| matches!(b, Block::Rule)),
            "{chat:#?}"
        );
        assert!(all_text(&chat).contains("author"));
        // Only at the start: a rule between two paragraphs is a rule
        let later = parse_document("Intro.\n\n---\n\nMore.\n");
        assert!(
            later.blocks.iter().any(|b| matches!(b, Block::Rule)),
            "{later:#?}"
        );
        // Unclosed front matter is not front matter
        assert!(all_text(&parse_document("---\ntitle: x\n\nBody.\n")).contains("Body."));
        // Without any, nothing changes
        assert_eq!(parse_document("# T\n\nBody.\n"), parse("# T\n\nBody.\n"));
    }

    /// Front matter is only a block at the very start: the same block in
    /// the middle of a document is shown as CommonMark reads it, a rule and
    /// then the lines down to the second `---` as a heading
    #[test]
    fn a_front_matter_block_mid_document_is_shown() {
        assert_eq!(front_matter_end("---\ntitle: x\n---\n\nBody.\n"), Some(17));
        assert_eq!(front_matter_end("Intro.\n\n---\ntitle: x\n---\n"), None);
        assert_eq!(
            front_matter_end("---\n\ntitle: x\n---\n"),
            None,
            "a blank line first"
        );
        assert_eq!(front_matter_end("---\ntitle: x\n"), None, "no closing line");
        let mid = "Intro.\n\n---\ntitle: x\nauthor: J\n---\n\nMore.\n";
        let doc = parse_document(mid);
        assert!(matches!(doc.blocks[1], Block::Rule), "{doc:#?}");
        let Block::Text(heading) = &doc.blocks[2] else {
            panic!("{doc:#?}")
        };
        assert_eq!(heading.kind, TextKind::Heading(2), "{doc:#?}");
        assert_eq!(
            heading
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
            "title: x author: J"
        );
        assert_eq!(all_text(&doc), "Intro.|title: x author: J|More.");
        // With front matter at the start as well, only that is left out
        let both = parse_document(&format!("---\nk: v\n---\n\n{mid}"));
        assert_eq!(both, doc);
    }

    /// Item 11: a footnote reference is its name in superscript letters,
    /// where each has one; a definition starts with its label; a name that
    /// cannot be raised keeps its brackets
    #[test]
    fn footnotes_are_superscript_labels_with_the_definition_starting_with_it() {
        let doc = parse_document(
            "Disks are encrypted.[^keys] And more.[^1] Odd.[^no_way!]\n\n[^keys]: The keys are on paper.\n\n[^1]: One.\n\n[^no_way!]: Odd one.\n",
        );
        let first = text_of(&doc);
        assert_eq!(first, "Disks are encrypted.ᵏᵉʸˢ And more.¹ Odd.[no_way!]");
        assert_eq!(superscript("keys").as_deref(), Some("ᵏᵉʸˢ"));
        assert_eq!(superscript("1").as_deref(), Some("¹"));
        assert_eq!(superscript("q"), None, "no superscript q in Unicode");
        assert_eq!(superscript(""), None);
        assert_eq!(superscript("a-b").as_deref(), Some("ᵃ⁻ᵇ"));
        // The definitions: the label first, then the words
        let texts: Vec<String> = doc
            .blocks
            .iter()
            .filter_map(|b| {
                if let Block::Text(t) = b {
                    Some(t.runs.iter().map(|r| r.text.as_str()).collect())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            texts.contains(&"ᵏᵉʸˢ The keys are on paper.".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"¹ One.".to_string()), "{texts:?}");
        assert!(
            texts.contains(&"[no_way!] Odd one.".to_string()),
            "{texts:?}"
        );
    }
}
