// SPDX-License-Identifier: GPL-3.0-only

//! Exact parser provenance for positions in rendered Markdown.
//!
//! Offsets here are UTF-8 bytes. Prover character offsets belong to the caller,
//! which can compose this map with its own source conversion. Hidden markup
//! creates two valid source boundaries at a rendered boundary; affinity chooses
//! between them. Formulas and figures are atomic source replacements.

use std::ops::Range;
use std::sync::Arc;

use crate::markdown::{Doc, OffsetMap, Rewrite};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Affinity {
    Before,
    #[default]
    After,
}

/// A parsed document and provenance for its rendered positions.
#[derive(Clone, Debug)]
pub struct MappedDoc {
    pub doc: Doc,
    pub map: SourceMap,
}

#[derive(Clone, Debug)]
struct Span {
    display: Range<usize>,
    source: Range<usize>,
    literal: bool,
}

#[derive(Clone, Debug, Default)]
struct CellMap {
    spans: Vec<Span>,
    len: usize,
    trailing_breaks: usize,
    anchor: usize,
}

impl CellMap {
    fn push(&mut self, len: usize, source: Range<usize>, literal: bool) {
        if len == 0 {
            return;
        }
        if self.spans.is_empty() {
            self.anchor = source.start;
        }
        if literal
            && let Some(previous) = self.spans.last_mut()
            && previous.literal
            && previous.source.end == source.start
        {
            previous.display.end += len;
            previous.source.end = source.end;
        } else {
            self.spans.push(Span { display: self.len..self.len + len, source, literal });
        }
        self.len += len;
    }

    fn literal(&mut self, shown: &str, range: Range<usize>, source: &str, code: bool) {
        let mut range = range;
        if code && source[range.clone()].starts_with('`') {
            let ticks = source[range.clone()].bytes().take_while(|&c| c == b'`').count();
            if range.len() >= ticks * 2 && source[range.end - ticks..range.end].bytes().all(|c| c == b'`') {
                range.start += ticks;
                range.end -= ticks;
                let content = &source[range.clone()];
                if content.starts_with(' ') && content.ends_with(' ') && content.bytes().any(|c| c != b' ') {
                    range.start += 1;
                    range.end -= 1;
                }
            }
        }
        let raw = &source[range.clone()];
        if !code && raw.starts_with("[^") && shown.starts_with('[') && raw[2..] == shown[1..] {
            self.push(1, range.start..range.start + 1, true);
            self.push(shown.len() - 1, range.start + 2..range.end, true);
            self.trailing_breaks = 0;
            return;
        }
        if raw == shown {
            self.push(shown.len(), range, true);
        } else if !code && shown == "\n" {
            self.push(1, range, false);
        } else {
            let mut at = range.start;
            let mut displayed = 0;
            while displayed < shown.len() {
                let rest = &shown[displayed..];
                let Some(c) = rest.chars().next() else { break };
                if at >= range.end {
                    self.push(rest.len(), range.end..range.end, false);
                    break;
                }
                let raw_rest = &source[at..range.end];
                let raw_c = raw_rest.chars().next().unwrap();
                if !code
                    && raw_c == '&'
                    && let Some(end) = raw_rest.find(';').filter(|end| *end < 40)
                {
                    let entity = &raw_rest[..=end];
                    let decoded: String = pulldown_cmark::Parser::new(entity)
                        .filter_map(|e| match e {
                            pulldown_cmark::Event::Text(t) => Some(t.to_string()),
                            _ => None,
                        })
                        .collect();
                    if !decoded.is_empty() && decoded != entity && rest.starts_with(&decoded) {
                        self.push(decoded.len(), at..at + entity.len(), false);
                        at += entity.len();
                        displayed += decoded.len();
                        continue;
                    }
                }
                if !code && raw_c == '\\' && raw_rest[1..].starts_with(c) && c.is_ascii_punctuation() {
                    self.push(c.len_utf8(), at..at + 1 + c.len_utf8(), false);
                    at += 1 + c.len_utf8();
                } else if raw_c == c {
                    self.push(c.len_utf8(), at..at + c.len_utf8(), true);
                    at += c.len_utf8();
                } else if code && c == ' ' && matches!(raw_c, '\n' | '\r') {
                    let len = if raw_rest.starts_with("\r\n") { 2 } else { raw_c.len_utf8() };
                    self.push(1, at..at + len, false);
                    at += len;
                } else if (raw_c.is_whitespace() || raw_c == '>')
                    && raw_rest.lines().next().is_some_and(|line| line.ends_with(rest.lines().next().unwrap_or(rest)))
                {
                    // Container prefixes/indentation removed from code by cmark.
                    at += raw_c.len_utf8();
                    continue;
                } else {
                    // A parser replacement has no source byte for its interior.
                    // Keep its exact provenance, choosing its ends by affinity.
                    self.push(rest.len(), at..range.end, false);
                    break;
                }
                displayed += c.len_utf8();
            }
        }
        self.trailing_breaks = if shown.ends_with('\n') {
            let n = shown.bytes().rev().take_while(|&c| c == b'\n').count();
            if n == shown.len() { self.trailing_breaks + n } else { n }
        } else {
            0
        };
    }

    fn math(&mut self, latex: &str, range: Range<usize>, source: &str) {
        if crate::math::parses(latex) {
            self.push(1, range, false);
        } else {
            self.literal(&format!("${latex}$"), range, source, false);
        }
        self.trailing_breaks = 0;
    }

    fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
        self.spans.retain(|span| span.display.start < self.len);
        if let Some(last) = self.spans.last_mut()
            && last.display.end > self.len
        {
            if last.literal {
                last.source.end -= last.display.end - self.len;
            }
            last.display.end = self.len;
        }
        self.trailing_breaks = 0;
    }

    fn trim_breaks(&mut self) {
        self.truncate(self.len.saturating_sub(self.trailing_breaks));
    }

    fn append(&mut self, mut other: CellMap) {
        if self.spans.is_empty() {
            self.anchor = other.anchor;
        }
        for span in &mut other.spans {
            span.display.start += self.len;
            span.display.end += self.len;
        }
        self.len += other.len;
        self.spans.extend(other.spans);
    }

    fn source(&self, offset: usize, affinity: Affinity) -> usize {
        let offset = offset.min(self.len);
        let next = self.spans.partition_point(|span| span.display.end <= offset);
        if affinity == Affinity::Before && next > 0 && self.spans[next - 1].display.end == offset {
            return self.spans[next - 1].source.end;
        }
        let Some(span) = self.spans.get(next) else {
            return self.spans.last().map_or(self.anchor, |span| span.source.end);
        };
        if span.literal {
            span.source.start + offset.saturating_sub(span.display.start).min(span.source.len())
        } else if offset > span.display.start && affinity == Affinity::After {
            span.source.end
        } else {
            span.source.start
        }
    }
}

#[derive(Clone, Debug)]
enum BlockMap {
    Text(CellMap),
    Object(Range<usize>),
}

/// The source of every rendered block and cell, retained independently of the
/// layout/font cache. Copies of a map share their backing storage.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    source: Arc<str>,
    rewrites: Arc<[Rewrite]>,
    blocks: Arc<[BlockMap]>,
}

impl SourceMap {
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Original Markdown byte for a rendered block/linear-text byte. Hidden
    /// delimiters select the preceding/following boundary by `affinity`.
    /// Positions inside generated figures select the source object's boundary.
    pub fn source_byte(&self, block: usize, offset: usize, affinity: Affinity) -> Option<usize> {
        let normalized = match self.blocks.get(block)? {
            BlockMap::Text(cell) => cell.source(offset, affinity),
            BlockMap::Object(range) => {
                if offset == 0 {
                    range.start
                } else {
                    range.end
                }
            }
        };
        let mut at = OffsetMap::new(&self.rewrites).source(normalized).min(self.source.len());
        match affinity {
            Affinity::Before => {
                while !self.source.is_char_boundary(at) {
                    at -= 1;
                }
            }
            Affinity::After => {
                while !self.source.is_char_boundary(at) {
                    at += 1;
                }
            }
        }
        Some(at)
    }
}

pub(crate) struct SourceTracker {
    source: Arc<str>,
    pub range: Range<usize>,
    text: CellMap,
    code: CellMap,
    code_range: Range<usize>,
    image_range: Range<usize>,
    table: Option<Vec<Vec<CellMap>>>,
    blocks: Vec<BlockMap>,
}

impl SourceTracker {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.into(),
            range: 0..0,
            text: CellMap::default(),
            code: CellMap::default(),
            code_range: 0..0,
            image_range: 0..0,
            table: None,
            blocks: Vec::new(),
        }
    }

    pub fn push(&mut self, text: &str, math: bool, code: bool) {
        let cell = self
            .table
            .as_mut()
            .and_then(|rows| rows.last_mut())
            .and_then(|row| row.last_mut())
            .unwrap_or(&mut self.text);
        if math {
            cell.math(text, self.range.clone(), &self.source);
        } else {
            cell.literal(text, self.range.clone(), &self.source, code);
        }
    }

    pub fn flush(&mut self, keep: bool) {
        let mut text = std::mem::take(&mut self.text);
        if keep {
            text.trim_breaks();
            self.blocks.push(BlockMap::Text(text));
        }
    }

    pub fn begin_code(&mut self) {
        self.code = CellMap::default();
        self.code_range = self.range.clone();
    }

    pub fn code_text(&mut self, text: &str) {
        // Code-block events already exclude fences and indentation. Only an
        // inline Code event includes backtick delimiters in its source range.
        self.code.literal(text, self.range.clone(), &self.source, false);
    }

    pub fn end_code(&mut self, len: usize, figure: bool) {
        let mut code = std::mem::take(&mut self.code);
        code.truncate(len);
        self.blocks.push(if figure { BlockMap::Object(self.code_range.clone()) } else { BlockMap::Text(code) });
    }

    pub fn begin_image(&mut self) {
        self.image_range = self.range.clone();
    }

    pub fn end_image(&mut self) {
        self.blocks.push(BlockMap::Object(self.image_range.clone()));
    }

    pub fn object(&mut self) {
        self.blocks.push(BlockMap::Object(self.range.clone()));
    }

    pub fn begin_table(&mut self) {
        self.table = Some(Vec::new());
    }
    pub fn row(&mut self) {
        if let Some(rows) = &mut self.table {
            rows.push(Vec::new());
        }
    }
    pub fn cell(&mut self) {
        if let Some(row) = self.table.as_mut().and_then(|rows| rows.last_mut()) {
            row.push(CellMap { anchor: self.range.start, ..CellMap::default() });
        }
    }
    pub fn end_table(&mut self) {
        let mut table = CellMap::default();
        for (r, row) in self.table.take().unwrap_or_default().into_iter().enumerate() {
            for (c, mut cell) in row.into_iter().enumerate() {
                cell.trim_breaks();
                if r > 0 && c == 0 || c > 0 {
                    let before = table.spans.last().map_or(cell.anchor, |span| span.source.end);
                    table.push(1, before..cell.anchor.max(before), false);
                }
                table.append(cell);
            }
        }
        self.blocks.push(BlockMap::Text(table));
    }

    pub fn finish(self, source: &str, rewrites: Vec<Rewrite>) -> SourceMap {
        SourceMap { source: source.into(), rewrites: rewrites.into(), blocks: self.blocks.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse_mapped;

    #[test]
    fn hidden_markup_has_two_exact_boundaries() {
        let source = "same **same** [same](target) same";
        let mapped = parse_mapped(source);
        assert_eq!(mapped.map.len(), mapped.doc.blocks.len());
        for (display, original) in [(0, 0), (5, 7), (10, 15), (15, 29)] {
            assert_eq!(mapped.map.source_byte(0, display, Affinity::After), Some(original));
        }
        assert_eq!(mapped.map.source_byte(0, 9, Affinity::Before), Some(11));
        assert_eq!(mapped.map.source_byte(0, 9, Affinity::After), Some(13));
    }

    #[test]
    fn entities_escapes_and_unicode_keep_parser_provenance() {
        let source = "é &amp; \\*界 [é](elsewhere)";
        let mapped = parse_mapped(source);
        for (display, original) in [(0, 0), (3, 3), (4, 8), (5, 10), (6, 11), (10, 16)] {
            assert_eq!(mapped.map.source_byte(0, display, Affinity::After), Some(original), "display {display}");
        }
        assert_eq!(mapped.map.source_byte(0, 1, Affinity::Before), Some(0));
        assert_eq!(mapped.map.source_byte(0, 1, Affinity::After), Some(2));
    }

    #[test]
    fn normalized_math_uses_rendered_placeholder_bytes() {
        let source = "before \\(x+1\\) after";
        let mapped = parse_mapped(source);
        assert_eq!(mapped.map.source_byte(0, 7, Affinity::After), Some(7));
        assert_eq!(mapped.map.source_byte(0, 8, Affinity::Before), Some(14));
        assert_eq!(mapped.map.source_byte(0, 9, Affinity::After), Some(15));
        let copied = crate::rich::copy(&mapped.doc, crate::rich::Pos::START, crate::rich::Pos::END);
        assert_eq!(copied, "before $x+1$ after");
    }

    #[test]
    fn tables_code_and_objects_stay_aligned_with_blocks() {
        let source = "| α | **β** |\n|---|---|\n| γ | `δ` |\n\n> ```rust\n> let x = 1;\n> ```\n\n![alt](image.png)\n\n$$x$$\n\n---";
        let mapped = parse_mapped(source);
        assert_eq!(mapped.map.len(), mapped.doc.blocks.len());
        assert_eq!(mapped.map.len(), 5);
        for (offset, needle) in [(0, "α"), (3, "β"), (6, "γ"), (9, "δ")] {
            assert_eq!(mapped.map.source_byte(0, offset, Affinity::After), source.find(needle));
        }
        assert_eq!(mapped.map.source_byte(1, 0, Affinity::After), source.find("let x"));
        assert_eq!(mapped.map.source_byte(1, 4, Affinity::After), source.find("x ="));
        assert_eq!(mapped.map.source_byte(2, 0, Affinity::After), source.find("!["));
        assert_eq!(mapped.map.source_byte(3, 1, Affinity::Before), source.find("$$x$$").map(|i| i + 5));
    }

    #[test]
    fn inline_code_normalization_and_literal_code_backticks_are_distinct() {
        let source = "` a\nb `\n\n```\n`literal`\n```";
        let mapped = parse_mapped(source);
        for (offset, original) in [(0, 2), (1, 3), (2, 4), (3, 5)] {
            assert_eq!(mapped.map.source_byte(0, offset, Affinity::After), Some(original));
        }
        let start = source.find("`literal`").unwrap();
        for offset in 0..=9 {
            assert_eq!(mapped.map.source_byte(1, offset, Affinity::After), Some(start + offset));
        }
    }
}
