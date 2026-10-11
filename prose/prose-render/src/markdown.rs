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
    Code { lang: String, text: String, indent: u16, quote: u16 },
    /// Display math
    Math { latex: String, indent: u16, quote: u16, marker: Option<String> },
    Rule,
    /// Rows of cells; the first row is the header
    Table { rows: Vec<Vec<Vec<Run>>> },
    Image { url: String, alt: String, indent: u16 },
    /// A chart: a `chartgen` code block's specification
    Chart { spec: String, indent: u16, quote: u16 },
    /// A Mermaid diagram: a closed `mermaid` code block's source. One whose
    /// fence is still open, as while it streams in, stays code
    Diagram { source: String, indent: u16, quote: u16 },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

impl Doc {
    /// Every formula, inline or display, with whether it is display math
    pub fn formulas(&self) -> impl Iterator<Item = (&str, bool)> {
        self.blocks.iter().flat_map(|block| -> Box<dyn Iterator<Item = (&str, bool)>> {
            match block {
                Block::Math { latex, .. } => Box::new(std::iter::once((latex.as_str(), true))),
                Block::Text(text) => Box::new(
                    text.runs.iter().filter(|r| r.style.math).map(|r| (r.text.as_str(), false)),
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
                runs: vec![Run { text: p.trim_end().to_string(), style: Style::default(), link: None }],
            })
        })
        .collect();
    Doc { blocks }
}

pub fn parse(markdown: &str) -> Doc {
    let source = normalize_math(markdown);
    let options = Options::ENABLE_MATH
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let mut builder = Builder::default();
    for (event, range) in Parser::new_ext(&source, options).into_offset_iter() {
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) = &event {
            builder.fence_closed = fence_closed(&source[range]);
        }
        builder.event(event);
    }
    builder.flush();
    Doc { blocks: builder.blocks }
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
    image: Option<(String, String)>,
    /// Whether the fenced code block being built has its closing fence
    fence_closed: bool,
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
                cell.push(Run { text: text.to_string(), style, link: self.link.clone() });
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
        self.runs.push(Run { text: text.to_string(), style, link: self.link.clone() });
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
                        CodeBlockKind::Fenced(lang) => lang.split_whitespace().next().unwrap_or_default().to_string(),
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
                Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
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
                            Block::Chart { spec: text, indent, quote }
                        } else if lang == "mermaid" && self.fence_closed {
                            Block::Diagram { source: text, indent, quote }
                        } else {
                            Block::Code { lang, text, indent, quote }
                        });
                    }
                }
                TagEnd::List(_) => {
                    self.flush();
                    self.lists.pop();
                }
                TagEnd::Item => self.flush(),
                TagEnd::Emphasis => self.style.italic = false,
                TagEnd::Strong => self.style.bold = false,
                TagEnd::Strikethrough => self.style.strike = false,
                TagEnd::Link => self.link = None,
                TagEnd::Image => {
                    if let Some((url, alt)) = self.image.take() {
                        self.flush();
                        self.blocks.push(Block::Image { url, alt, indent: self.indent() });
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
            Event::Code(text) => self.push_text(&text, Style { code: true, ..self.style }),
            Event::InlineMath(latex) => self.push_text(latex.trim(), Style { math: true, ..Style::default() }),
            Event::DisplayMath(latex) => {
                if let Some(rows) = &mut self.table {
                    if let Some(cell) = rows.last_mut().and_then(|row| row.last_mut()) {
                        cell.push(Run {
                            text: latex.trim().to_string(),
                            style: Style { math: true, ..Style::default() },
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
                    self.push_text("\n", self.style);
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
            Event::FootnoteReference(name) => self.push_text(&format!("[{name}]"), self.style),
        }
    }
}

/// Whether a fenced code block, as its source runs, ends with a closing
/// fence: pulldown-cmark ends an unclosed one at the end of the document
fn fence_closed(block: &str) -> bool {
    let strip = |line: &str| line.trim_start_matches(|c: char| c.is_whitespace() || c == '>').to_string();
    let mut lines = block.trim_end().lines();
    let Some(open) = lines.next().map(strip) else { return false };
    let fence: String = open.chars().take_while(|c| *c == '`' || *c == '~').collect();
    let Some(close) = lines.last().map(strip) else { return false };
    !fence.is_empty()
        && close.starts_with(&fence)
        && close.trim_end().chars().all(|c| fence.starts_with(c))
}

/// Rewrite `\(…\)` as `$…$`, `\[…\]` as `$$…$$`, and wrap bare display
/// environments such as `\begin{align}` in `$$`, outside code
pub fn normalize_math(text: &str) -> String {
    const ENVIRONMENTS: [&str; 8] =
        ["align", "align*", "equation", "equation*", "gather", "gather*", "multline", "eqnarray"];
    let mut out = String::with_capacity(text.len() + 16);
    let mut in_fence: Option<String> = None;
    let mut in_display = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(fence) = &in_fence {
            if trimmed.starts_with(fence.as_str()) {
                in_fence = None;
            }
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let fence: String = trimmed.chars().take_while(|c| *c == '`' || *c == '~').collect();
            in_fence = Some(fence);
            out.push_str(line);
            continue;
        }
        // Environments on lines of their own
        let begins = ENVIRONMENTS.iter().any(|env| trimmed.starts_with(&format!("\\begin{{{env}}}")));
        if begins && !in_display {
            out.push_str("$$\n");
        }
        let mut converted = String::with_capacity(line.len());
        let mut chars = line.char_indices().peekable();
        let mut in_code = 0usize;
        while let Some((_, c)) = chars.next() {
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
                converted.extend(std::iter::repeat_n('`', run));
                continue;
            }
            if c == '\\' && in_code == 0 {
                match chars.peek().map(|(_, c)| *c) {
                    Some('(') | Some(')') => {
                        chars.next();
                        converted.push('$');
                        continue;
                    }
                    Some('[') => {
                        chars.next();
                        converted.push_str("$$");
                        in_display = true;
                        continue;
                    }
                    Some(']') if in_display => {
                        chars.next();
                        converted.push_str("$$");
                        in_display = false;
                        continue;
                    }
                    Some(next) => {
                        converted.push(c);
                        converted.push(next);
                        chars.next();
                        continue;
                    }
                    None => {}
                }
            }
            converted.push(c);
        }
        out.push_str(&converted);
        let ends = ENVIRONMENTS.iter().any(|env| trimmed.trim_end().ends_with(&format!("\\end{{{env}}}")));
        if (ends || (begins && trimmed.trim_end().ends_with('}') && ends)) && !in_display {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("$$\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn math_runs(doc: &Doc) -> Vec<(String, bool)> {
        doc.formulas().map(|(l, d)| (l.to_string(), d)).collect()
    }

    #[test]
    fn dollars_make_inline_and_display_math() {
        let doc = parse("Let $x^2$ be.\n\n$$\\int_0^1 f$$\n\nDone, costs $5 and $6.");
        assert_eq!(
            math_runs(&doc),
            vec![("x^2".to_string(), false), ("\\int_0^1 f".to_string(), true)]
        );
    }

    #[test]
    fn brackets_and_parentheses_become_math_outside_code() {
        let doc = parse("Take \\(a+b\\) and `\\(code\\)`:\n\\[\nE = mc^2\n\\]");
        let runs = math_runs(&doc);
        assert_eq!(runs[0], ("a+b".to_string(), false));
        assert_eq!(runs[1], ("E = mc^2".to_string(), true));
        let Block::Text(block) = &doc.blocks[0] else { panic!() };
        assert!(block.runs.iter().any(|r| r.style.code && r.text == "\\(code\\)"));
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
    fn closed_mermaid_fences_are_diagrams_and_open_ones_code() {
        let source = "flowchart TD\n    A --> B";
        let doc = parse(&format!("See:\n\n```mermaid\n{source}\n```\n\nDone."));
        assert!(matches!(&doc.blocks[1], Block::Diagram { source: s, .. } if s == source), "{doc:#?}");
        // Still streaming: the fence is open
        let doc = parse(&format!("See:\n\n```mermaid\n{source}\n"));
        assert!(matches!(&doc.blocks[1], Block::Code { lang, .. } if lang == "mermaid"), "{doc:#?}");
        // In a list and a quote, and with a longer fence
        let doc = parse(&format!("- item\n\n  ````mermaid\n  {source}\n  ````\n\n> ```mermaid\n> graph LR\n> ```\n"));
        assert!(matches!(&doc.blocks[1], Block::Diagram { indent: 1, .. }), "{doc:#?}");
        assert!(matches!(&doc.blocks[2], Block::Diagram { quote: 1, .. }), "{doc:#?}");
        // A shorter fence does not close a longer one
        let doc = parse("````mermaid\ngraph LR\n```\n");
        assert!(matches!(&doc.blocks[0], Block::Code { .. }), "{doc:#?}");
    }

    #[test]
    fn lists_headings_code_and_tables() {
        let doc = parse("# Title\n\n1. one\n2. two\n   - nested\n\n```rust\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| $1$ | 2 |\n");
        let Block::Text(heading) = &doc.blocks[0] else { panic!() };
        assert_eq!(heading.kind, TextKind::Heading(1));
        let Block::Text(item) = &doc.blocks[2] else { panic!() };
        assert_eq!(item.marker.as_deref(), Some("2."));
        let Block::Text(nested) = &doc.blocks[3] else { panic!() };
        assert_eq!((nested.indent, nested.marker.as_deref()), (2, Some("◦")));
        assert!(matches!(&doc.blocks[4], Block::Code { lang, text, .. } if lang == "rust" && text == "fn x() {}"));
        let Block::Table { rows } = &doc.blocks[5] else { panic!() };
        assert_eq!(rows.len(), 2);
        assert!(rows[1][0][0].style.math);
    }
}
