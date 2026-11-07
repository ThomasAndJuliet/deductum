// SPDX-License-Identifier: GPL-3.0-only

//! Selectable rich text: a parsed markdown [`Doc`] laid out with cosmic-text,
//! with LaTeX set by [`crate::math`], charts laid out by [`crate::chart`]
//! and Mermaid diagrams drawn by [`crate::diagram`].
//!
//! Each block is shaped into cosmic-text buffers the widget owns and draws
//! itself, as the terminal draws its grid, so that every glyph can be hit
//! and highlighted: text is selected by dragging, double-clicking a word or
//! triple-clicking a block, across blocks and across the widgets of a
//! thread. Inline math sits in the line as a no-break space whose letter
//! spacing makes room for the formula, so it wraps, selects and copies with
//! the words around it; copying gives its LaTeX back.
//!
//! A position is a [`Pos`]: a block, and a byte offset into that block's
//! *linear text* — its cells' text, with a tab between table cells and a
//! newline between rows, or one unit for a formula or an image. [`copy`]
//! turns a range back into text with the same rules, without fonts, so a
//! thread copies a selection that spans widgets from its model.

use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use cosmic::Renderer;
use cosmic::iced::advanced::graphics::text::{Raw, font_system};
use cosmic::iced::advanced::{
    Clipboard, Layout, Shell, Widget, image as adv_image, layout, mouse, renderer, svg as adv_svg,
    widget::{Tree, tree},
};
use cosmic::iced::core::text::Renderer as _;
use cosmic::iced::core::{image::Renderer as _, renderer::Renderer as _, svg::Renderer as _};
use cosmic::iced::{
    Background, Border, Color, Element, Event, Length, Point, Rectangle, Size, Vector, keyboard,
};
use cosmic_text::{
    Align, Attrs, Buffer, Cursor, Family, Metrics, Shaping, Style as FontStyle, Weight, Wrap,
};

use crate::markdown::{Block, Doc, Run, Style, TextKind};
use crate::{chart, diagram, math};

/// A place in a [`Doc`]: a block, and a byte offset into its linear text
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub block: usize,
    pub offset: usize,
}

impl Pos {
    pub const START: Pos = Pos {
        block: 0,
        offset: 0,
    };
    pub const END: Pos = Pos {
        block: usize::MAX,
        offset: usize::MAX,
    };
}

/// What the pointer did to the selection
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Select {
    /// Pressed here: a new selection starts, or with Shift the current one
    /// extends
    Press { pos: Pos, extend: bool },
    /// Dragged here
    Drag(Pos),
    /// A word or a block, chosen by a double or triple click
    Range(Pos, Pos),
}

/// How text is set: fonts, sizes and colours
#[derive(Clone, Debug, PartialEq)]
pub struct Typography {
    /// The body font's family, or empty for the theme's sans-serif
    pub body: String,
    pub mono: String,
    /// Pixels per em of body text
    pub size: f32,
    /// Line height, as a multiple of the size
    pub line_height: f32,
    /// Formulas' size against the text's
    pub math_scale: f32,
    pub justify: bool,
    /// Display formulas set at the start of the line rather than centred,
    /// as in a list, where every row starts on one edge
    pub display_start: bool,
    /// The whole text in italic, as for thinking
    pub italic: bool,
    /// Body text
    pub text: Color,
    /// Headings, bold text and labels: the darkest tone, where `text` is
    /// lighter on a light page
    pub strong: Color,
    pub muted: Color,
    /// Links in body text, in the accent's dark tone on a light page
    pub link: Color,
    /// Links in headings and bold, in its darkest
    pub link_strong: Color,
    pub code: Color,
    /// A card's tone: what diagrams' boxes are filled with. Code, inline
    /// code and tables have no ground (household rule 25f)
    pub code_background: Color,
    /// The page's colour, which charts' neutral colours start from
    pub page: Color,
    pub rule: Color,
    pub selection: Color,
    pub error: Color,
    pub added: Color,
    pub removed: Color,
    /// Width of the widest image thumbnail
    pub image_width: f32,
}

impl Typography {
    fn key(&self) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        (
            &self.body,
            &self.mono,
            self.justify,
            self.display_start,
            self.italic,
        )
            .hash(&mut hasher);
        for value in [
            self.size,
            self.line_height,
            self.math_scale,
            self.image_width,
        ] {
            value.to_bits().hash(&mut hasher);
        }
        for color in [
            self.text,
            self.strong,
            self.muted,
            self.link,
            self.link_strong,
            self.code,
            self.code_background,
            self.page,
            self.rule,
            self.error,
            self.added,
            self.removed,
        ] {
            color.into_rgba8().hash(&mut hasher);
        }
        hasher.finish()
    }

    /// The colours a diagram is drawn in on this page
    pub fn diagram_palette(&self) -> diagram::Palette {
        // Translucent colours as they show on the page
        let over = |color: Color| Color {
            r: self.page.r + (color.r - self.page.r) * color.a,
            g: self.page.g + (color.g - self.page.g) * color.a,
            b: self.page.b + (color.b - self.page.b) * color.a,
            a: 1.0,
        };
        diagram::Palette {
            page: Color {
                a: 1.0,
                ..self.page
            },
            surface: over(self.code_background),
            text: over(self.text),
            muted: over(self.muted),
            rule: over(self.rule),
            accent: over(self.link_strong),
            error: over(self.error),
        }
    }

    fn family(&self) -> Family<'_> {
        if self.body.is_empty() {
            Family::SansSerif
        } else {
            Family::Name(&self.body)
        }
    }

    fn mono_family(&self) -> Family<'_> {
        if self.mono.is_empty() {
            Family::Monospace
        } else {
            Family::Name(&self.mono)
        }
    }

    /// A heading's line spacing (household rule 23a, Juliet 15 Oct):
    /// shrinking as the heading grows, H4 1.4 · H3 1.35 · H2 1.3 · H1 1.2,
    /// and never looser than the prose's own
    pub fn heading_line_height(level: u8, prose: f32) -> f32 {
        let heading = match level {
            1 => 1.2,
            2 => 1.3,
            3 => 1.35,
            4 => 1.4,
            _ => prose,
        };
        heading.min(prose)
    }

    fn heading_scale(level: u8) -> f32 {
        match level {
            1 => 1.55,
            2 => 1.32,
            3 => 1.16,
            4 => 1.05,
            _ => 1.0,
        }
    }
}

fn rgb(color: Color) -> [u8; 3] {
    let [r, g, b, _] = color.into_rgba8();
    [r, g, b]
}

fn ct_color(color: Color) -> cosmic_text::Color {
    let [r, g, b, a] = color.into_rgba8();
    cosmic_text::Color::rgba(r, g, b, a)
}

// ---------------------------------------------------------------------------
// Linear text, shared by layout and copying

/// How a span of a cell is coloured, apart from its style
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Normal,
    /// A token of code, coloured by its class
    Code(crate::highlight::Class),
}

#[derive(Clone, Debug, PartialEq)]
enum SpanKind {
    Text {
        style: Style,
        link: Option<String>,
        tone: Tone,
    },
    /// A no-break space standing for a formula
    Math(String),
}

#[derive(Clone, Debug, PartialEq)]
struct Span {
    range: Range<usize>,
    kind: SpanKind,
}

/// One piece of text shaped as one buffer: a paragraph, a code block, a
/// table cell
#[derive(Clone, Debug, Default, PartialEq)]
struct Cell {
    text: String,
    spans: Vec<Span>,
}

/// What stands for an inline formula in the text: a letter, drawn
/// transparent, whose letter spacing makes it as wide as the formula. Not a
/// space, as cosmic-text hangs spaces past the end of a line and justifies
/// by them
const PLACEHOLDER: &str = "x";

fn cell_from_runs(runs: &[Run]) -> Cell {
    let mut cell = Cell::default();
    for run in runs {
        let start = cell.text.len();
        if run.style.math {
            // A formula that can't be set shows its source
            if math::parses(&run.text) {
                cell.text.push_str(PLACEHOLDER);
            } else {
                cell.text.push('$');
                cell.text.push_str(&run.text);
                cell.text.push('$');
            }
            cell.spans.push(Span {
                range: start..cell.text.len(),
                kind: SpanKind::Math(run.text.clone()),
            });
        } else {
            cell.text.push_str(&run.text);
            cell.spans.push(Span {
                range: start..cell.text.len(),
                kind: SpanKind::Text {
                    style: run.style,
                    link: run.link.clone(),
                    tone: Tone::Normal,
                },
            });
        }
    }
    // A trailing newline would add an empty line
    while cell.text.ends_with('\n') {
        cell.text.pop();
        if let Some(last) = cell.spans.last_mut() {
            last.range.end = last.range.end.min(cell.text.len());
        }
    }
    cell
}

fn code_cell(lang: &str, text: &str) -> Cell {
    let code = Style {
        code: true,
        ..Style::default()
    };
    let mut cell = Cell {
        text: text.to_string(),
        spans: Vec::new(),
    };
    // Coloured by its language, plain between tokens (a diff's added and
    // removed lines are classes too)
    let mut at = 0;
    let mut push = |range: Range<usize>, tone: Tone| {
        if !range.is_empty() {
            cell.spans.push(Span {
                range,
                kind: SpanKind::Text {
                    style: code,
                    link: None,
                    tone,
                },
            });
        }
    };
    for (range, class) in crate::highlight::tokens(text, lang).iter() {
        push(at..range.start, Tone::Normal);
        push(range.clone(), Tone::Code(*class));
        at = range.end;
    }
    push(at..text.len(), Tone::Normal);
    if text.is_empty() {
        cell.spans.push(Span {
            range: 0..0,
            kind: SpanKind::Text {
                style: code,
                link: None,
                tone: Tone::Normal,
            },
        });
    }
    cell
}

/// A block's cells, and where each starts in its linear text
fn block_cells(block: &Block) -> Vec<(usize, Cell)> {
    match block {
        Block::Text(text) => vec![(0, cell_from_runs(&text.runs))],
        Block::Code { lang, text, .. } => vec![(0, code_cell(lang, text))],
        Block::Chart { spec, .. } => chart_cells(spec),
        Block::Diagram { source, .. } => diagram_cells(source),
        Block::Table { rows } => {
            let mut out = Vec::new();
            let mut at = 0;
            for (r, row) in rows.iter().enumerate() {
                if r > 0 {
                    at += 1; // newline
                }
                for (c, runs) in row.iter().enumerate() {
                    if c > 0 {
                        at += 1; // tab
                    }
                    let cell = cell_from_runs(runs);
                    let len = cell.text.len();
                    out.push((at, cell));
                    at += len;
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The length of a block's linear text
fn block_len(block: &Block) -> usize {
    match block {
        Block::Math { .. } | Block::Image { .. } => 1,
        Block::Rule => 0,
        // Its labels once it has been drawn, else one unit, as a formula
        Block::Chart { .. } | Block::Diagram { .. } => block_cells(block)
            .last()
            .map_or(1, |(start, cell)| start + cell.text.len()),
        _ => block_cells(block)
            .last()
            .map_or(0, |(start, cell)| start + cell.text.len()),
    }
}

/// The text of `range` of a cell, with formulas as their LaTeX
fn cell_copy(cell: &Cell, range: Range<usize>) -> String {
    let mut out = String::new();
    let mut at = range.start;
    for span in &cell.spans {
        if span.range.end <= range.start || span.range.start >= range.end {
            continue;
        }
        match &span.kind {
            SpanKind::Math(latex) => {
                out.push('$');
                out.push_str(latex);
                out.push('$');
                at = span.range.end;
            }
            SpanKind::Text { .. } => {
                let start = span.range.start.max(at).max(range.start);
                let end = span.range.end.min(range.end);
                if let Some(text) = cell.text.get(start..end) {
                    out.push_str(text);
                }
                at = end;
            }
        }
    }
    out
}

/// The text of the part of `block` from `start` to `end` of its linear text
fn block_copy(block: &Block, start: usize, end: usize) -> String {
    match block {
        Block::Math { latex, .. } => {
            if start < 1 && end >= 1 {
                format!("$$\n{latex}\n$$")
            } else {
                String::new()
            }
        }
        Block::Image { url, alt, .. } => {
            if start < 1 && end >= 1 {
                format!("![{alt}]({url})")
            } else {
                String::new()
            }
        }
        Block::Rule => String::new(),
        // The whole chart copies as its specification
        Block::Chart { spec, .. } if start == 0 && end >= block_len(block) => {
            format!("```chartgen\n{spec}\n```")
        }
        // And a diagram as its source
        Block::Diagram { source, .. } if start == 0 && end >= block_len(block) => {
            format!("```mermaid\n{source}\n```")
        }
        _ => {
            let cells = block_cells(block);
            let mut out = String::new();
            let mut last_row_start = None;
            for (i, (cell_start, cell)) in cells.iter().enumerate() {
                let cell_end = cell_start + cell.text.len();
                if cell_end < start || *cell_start > end {
                    continue;
                }
                if let Some(previous) = last_row_start {
                    // Tab or newline: what lies between the two cells
                    let (prev_start, prev): &(usize, Cell) = &cells[previous];
                    out.push(
                        if cell_start - (prev_start + prev.text.len()) == 1
                            && is_row_break(block, i)
                        {
                            '\n'
                        } else {
                            '\t'
                        },
                    );
                }
                let local = start.saturating_sub(*cell_start).min(cell.text.len())
                    ..end.saturating_sub(*cell_start).min(cell.text.len());
                out.push_str(&cell_copy(cell, local));
                last_row_start = Some(i);
            }
            out
        }
    }
}

/// Whether table cell `i` starts a row
fn is_row_break(block: &Block, i: usize) -> bool {
    // A chart's or a diagram's labels are a line each
    if let Block::Chart { .. } | Block::Diagram { .. } = block {
        return true;
    }
    let Block::Table { rows } = block else {
        return false;
    };
    let mut n = 0;
    for row in rows {
        if n == i {
            return true;
        }
        n += row.len();
    }
    false
}

/// A block's linear text: its cells with what lies between them, and one
/// placeholder byte for a formula or an image
fn linear_text(block: &Block) -> String {
    match block {
        Block::Math { .. } | Block::Image { .. } => "\u{1}".to_string(),
        Block::Rule => String::new(),
        _ => {
            let cells = block_cells(block);
            let mut out = String::new();
            for (i, (start, cell)) in cells.iter().enumerate() {
                while out.len() < *start {
                    out.push(if is_row_break(block, i) { '\n' } else { '\t' });
                }
                out.push_str(&cell.text);
            }
            if out.is_empty() && block_len(block) == 1 {
                out.push('\u{1}');
            }
            out
        }
    }
}

/// How tall `doc` is, and how much of it comes before `pos`, roughly: in
/// characters, each line break counting as a line's worth, and each block
/// as two
pub fn text_before(doc: &Doc, pos: Pos) -> (usize, usize) {
    const LINE: usize = 60;
    let weigh = |text: &str| text.len() + text.matches('\n').count() * LINE;
    let mut total = 0;
    let mut before = 0;
    for (index, block) in doc.blocks.iter().enumerate() {
        let text = linear_text(block);
        let len = weigh(&text) + 2 * LINE;
        if index < pos.block {
            before += len;
        } else if index == pos.block {
            before += weigh(text.get(..pos.offset.min(text.len())).unwrap_or(&text)) + LINE;
        }
        total += len;
    }
    (total, before)
}

/// Where `needle` appears in `doc`, ignoring ASCII case, as ranges of
/// positions; a match never spans blocks
pub fn find(doc: &Doc, needle: &str) -> Vec<(Pos, Pos)> {
    let needle = needle.to_ascii_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (index, block) in doc.blocks.iter().enumerate() {
        let text = linear_text(block).to_ascii_lowercase();
        let mut from = 0;
        while let Some(at) = text.get(from..).and_then(|rest| rest.find(&needle)) {
            let start = from + at;
            out.push((
                Pos {
                    block: index,
                    offset: start,
                },
                Pos {
                    block: index,
                    offset: start + needle.len(),
                },
            ));
            from = start + needle.len().max(1);
        }
    }
    out
}

/// The text from `start` to `end` of `doc`, as the user would paste it
pub fn copy(doc: &Doc, start: Pos, end: Pos) -> String {
    let mut parts = Vec::new();
    let last = end.block.min(doc.blocks.len().saturating_sub(1));
    for (i, block) in doc
        .blocks
        .iter()
        .enumerate()
        .take(last + 1)
        .skip(start.block)
    {
        let from = if i == start.block { start.offset } else { 0 };
        let to = if i == end.block {
            end.offset
        } else {
            usize::MAX
        };
        let text = block_copy(block, from, to.min(block_len(block)));
        if !text.is_empty() || matches!(block, Block::Text(_)) {
            parts.push(text);
        }
    }
    parts.join("\n\n").trim_matches('\n').to_string()
}

/// The end of `doc`, for selecting all of it
pub fn end_of(doc: &Doc) -> Pos {
    match doc.blocks.last() {
        Some(block) => Pos {
            block: doc.blocks.len() - 1,
            offset: block_len(block),
        },
        None => Pos::START,
    }
}

// ---------------------------------------------------------------------------
// Images: their sizes, from their headers

/// The labels each chart was last drawn with, by its specification, for
/// copying and selecting its text without laying it out
static CHART_LABELS: LazyLock<Mutex<std::collections::HashMap<u64, Arc<Vec<String>>>>> =
    LazyLock::new(Default::default);

fn spec_key(spec: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    spec.hash(&mut hasher);
    hasher.finish()
}

/// A chart's labels as cells, a line each
fn chart_cells(spec: &str) -> Vec<(usize, Cell)> {
    let Some(labels) = CHART_LABELS.lock().unwrap().get(&spec_key(spec)).cloned() else {
        return Vec::new();
    };
    let mut at = 0;
    labels
        .iter()
        .map(|text| {
            let cell = plain_cell(text);
            let start = at;
            at += text.len() + 1;
            (start, cell)
        })
        .collect()
}

/// A diagram's lines of text as cells, a line each
fn diagram_cells(source: &str) -> Vec<(usize, Cell)> {
    let Some(labels) = diagram::labels_of(source) else {
        return Vec::new();
    };
    let mut at = 0;
    labels
        .iter()
        .map(|text| {
            let start = at;
            at += text.len() + 1;
            (start, plain_cell(text))
        })
        .collect()
}

fn plain_cell(text: &str) -> Cell {
    Cell {
        text: text.to_string(),
        spans: vec![Span {
            range: 0..text.len(),
            kind: SpanKind::Text {
                style: Style::default(),
                link: None,
                tone: Tone::Normal,
            },
        }],
    }
}

static IMAGE_SIZES: LazyLock<Mutex<std::collections::HashMap<PathBuf, Option<(u32, u32)>>>> =
    LazyLock::new(Default::default);

fn image_size(path: &Path) -> Option<(u32, u32)> {
    let mut sizes = IMAGE_SIZES.lock().unwrap();
    if let Some(size) = sizes.get(path) {
        return *size;
    }
    let size = crate::images::dimensions(path);
    if sizes.len() > 4096 {
        sizes.clear();
    }
    sizes.insert(path.to_path_buf(), size);
    size
}

/// What the view shows for an image: its thumbnail, if it has one
pub fn thumbnail_of(path: &Path) -> PathBuf {
    let thumb = crate::images::thumbnail_path(path);
    if thumb.is_file() {
        thumb
    } else {
        path.to_path_buf()
    }
}

// ---------------------------------------------------------------------------
// Layout

/// A shaped cell, placed
#[derive(Clone)]
struct Shaped {
    buffer: Arc<Buffer>,
    /// Relative to the widget
    origin: Vector,
    size: Size,
    /// Where it starts in its block's linear text
    start: usize,
    cell: Cell,
    /// Byte offset of each buffer line in the cell's text
    line_starts: Vec<usize>,
    /// Formulas in the text, and where they go, relative to the widget
    formulas: Vec<(Arc<math::Formula>, Point)>,
    color: Color,
}

#[derive(Clone)]
enum Laid {
    Cells(Vec<Shaped>),
    Math(Result<Arc<math::Formula>, Shaped>, Rectangle),
    Image(PathBuf, Rectangle),
    Rule(Rectangle),
}

/// What a block draws under its cells
#[derive(Clone)]
enum Figure {
    /// A chart's shapes
    Chart(Arc<chart::Chart>),
    /// A diagram's, and its source
    Diagram(Arc<diagram::Diagram>, Arc<str>),
}

#[derive(Clone)]
struct BlockLayout {
    top: f32,
    bottom: f32,
    laid: Laid,
    /// Panels behind it: code backgrounds, quote bars, table rules
    panels: Vec<(Rectangle, Color, f32)>,
    /// A list item's bullet or number
    marker: Option<Shaped>,
    /// A chart's or a diagram's shapes, under its labels
    figure: Option<(Figure, Rectangle)>,
    /// A table wider than its column: its box, and how wide the column
    /// shows it. It keeps its columns' widths and its text's size, and
    /// scrolls sideways (the plan: "Wide tables scroll sideways"), by
    /// [`State::table_dx`]
    wide: Option<(Rectangle, f32)>,
    len: usize,
}

/// A wide table's scrollbar: its height, and its gap below the table, as
/// the Paper and Text views' (`editor.rs`)
const TABLE_BAR: f32 = 4.0;

#[derive(Default)]
struct State {
    key: Option<(u64, u32)>,
    /// Each block's layout as last built, placed at the top, by what it was
    /// built from: while text streams in, only the last block changes, and
    /// only it is shaped again
    built: std::collections::HashMap<u64, BlockLayout>,
    style: Option<u64>,
    blocks: Vec<BlockLayout>,
    height: f32,
    last_click: Option<(Instant, Point, u8)>,
    /// Pressed in this widget, and where, for clicks on links and images
    pressed: Option<Point>,
    /// How far each table wider than its column is scrolled sideways, by
    /// its block
    table_x: std::collections::HashMap<usize, f32>,
    /// Shift held, for Shift with the wheel over a wide table
    shift: bool,
}

/// Selectable rich text
pub struct RichText<'a, Message> {
    doc: Arc<Doc>,
    typography: Arc<Typography>,
    selection: Option<(Pos, Pos)>,
    dragging: bool,
    base_dir: Option<&'a Path>,
    on_select: Option<Box<dyn Fn(Select) -> Message + 'a>>,
    on_link: Option<Box<dyn Fn(String) -> Message + 'a>>,
    on_image: Option<Box<dyn Fn(PathBuf) -> Message + 'a>>,
    on_diagram: Option<Box<dyn Fn(String) -> Message + 'a>>,
}

pub fn rich_text<'a, Message>(doc: Arc<Doc>, typography: Arc<Typography>) -> RichText<'a, Message> {
    RichText {
        doc,
        typography,
        selection: None,
        dragging: false,
        base_dir: None,
        on_select: None,
        on_link: None,
        on_image: None,
        on_diagram: None,
    }
}

impl<'a, Message> RichText<'a, Message> {
    /// The selected part of this text, from start to end
    pub fn selection(mut self, selection: Option<(Pos, Pos)>) -> Self {
        self.selection = selection;
        self
    }

    /// Whether a selection is being dragged, anywhere in the thread
    pub fn dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }

    /// Where relative image paths are
    pub fn base_dir(mut self, dir: Option<&'a Path>) -> Self {
        self.base_dir = dir;
        self
    }

    pub fn on_select(mut self, f: impl Fn(Select) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    pub fn on_link(mut self, f: impl Fn(String) -> Message + 'a) -> Self {
        self.on_link = Some(Box::new(f));
        self
    }

    pub fn on_image(mut self, f: impl Fn(PathBuf) -> Message + 'a) -> Self {
        self.on_image = Some(Box::new(f));
        self
    }

    /// Told a diagram's source when it is clicked, to open it on its own
    pub fn on_diagram(mut self, f: impl Fn(String) -> Message + 'a) -> Self {
        self.on_diagram = Some(Box::new(f));
        self
    }

    fn key(&self) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        (Arc::as_ptr(&self.doc) as usize).hash(&mut hasher);
        self.typography.key().hash(&mut hasher);
        hasher.finish()
    }

    fn resolve_image(&self, url: &str) -> Option<PathBuf> {
        let path = url.strip_prefix("file://").unwrap_or(url);
        if path.contains("://") {
            return None;
        }
        let path = Path::new(path);
        Some(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir?.join(path)
        })
    }

    fn build(
        &self,
        width: f32,
        built: &mut std::collections::HashMap<u64, BlockLayout>,
    ) -> (Vec<BlockLayout>, f32) {
        let mut reused = std::collections::HashMap::with_capacity(self.doc.blocks.len());
        let t = &*self.typography;
        let size = t.size;
        let mut font_system = font_system().write().unwrap();
        let font_system = font_system.raw();
        let mut blocks = Vec::with_capacity(self.doc.blocks.len());
        let mut y = 0.0f32;
        let mut previous: Option<&Block> = None;
        for block in &self.doc.blocks {
            if let Some(previous) = previous {
                let tight = matches!(
                    (previous, block),
                    (Block::Text(a), Block::Text(b)) if a.marker.is_some() && b.marker.is_some() && a.indent == b.indent
                );
                y += if tight { size * 0.25 } else { size * 0.7 };
                if let Block::Text(text) = block
                    && matches!(text.kind, TextKind::Heading(_))
                {
                    y += size * 0.5;
                }
            }
            previous = Some(block);
            // Unchanged since last time: placed again, not shaped again
            let key = {
                let mut hasher = std::hash::DefaultHasher::new();
                (block, width.to_bits()).hash(&mut hasher);
                hasher.finish()
            };
            if let Some(layout) = built.remove(&key).or_else(|| reused.get(&key).cloned()) {
                let layout = layout.shifted(y);
                y = layout.bottom;
                reused.insert(key, layout.shifted(-layout.top));
                blocks.push(layout);
                continue;
            }
            let (indent, quote, marker) = match block {
                Block::Text(text) => (text.indent, text.quote, text.marker.as_deref()),
                Block::Code { indent, quote, .. } => (*indent, *quote, None),
                Block::Math {
                    indent,
                    quote,
                    marker,
                    ..
                } => (*indent, *quote, marker.as_deref()),
                Block::Image { indent, .. } => (*indent, 0, None),
                Block::Chart { indent, quote, .. } | Block::Diagram { indent, quote, .. } => {
                    (*indent, *quote, None)
                }
                _ => (0, 0, None),
            };
            let x0 = f32::from(indent) * size * 1.5 + f32::from(quote) * size * 1.1;
            let available = (width - x0).max(size * 4.0);
            let mut panels = Vec::new();
            let mut figure = None;
            let mut wide = None;
            for q in 0..quote {
                panels.push((
                    Rectangle::new(
                        Point::new(f32::from(q) * size * 1.1, y),
                        Size::new(3.0, 0.0),
                    ),
                    t.rule,
                    1.5,
                ));
            }
            let top = y;
            let laid = match block {
                Block::Text(text) => {
                    let (scale, bold, line) = match text.kind {
                        TextKind::Heading(level) => (
                            Typography::heading_scale(level),
                            true,
                            Typography::heading_line_height(level, t.line_height),
                        ),
                        TextKind::Paragraph => (1.0, false, t.line_height),
                    };
                    let color = if quote > 0 {
                        t.muted
                    } else if matches!(text.kind, TextKind::Heading(_)) {
                        t.strong
                    } else {
                        t.text
                    };
                    let cell = cell_from_runs(&text.runs);
                    let shaped = shape_lined(
                        font_system,
                        t,
                        line,
                        cell,
                        0,
                        Point::new(x0, y),
                        available,
                        size * scale,
                        bold,
                        color,
                        t.justify && text.kind == TextKind::Paragraph,
                    );
                    y += shaped.size.height;
                    Laid::Cells(vec![shaped])
                }
                Block::Code { lang, text, .. } => {
                    let pad = size * 0.5;
                    let shaped = shape(
                        font_system,
                        t,
                        code_cell(lang, text),
                        0,
                        Point::new(x0 + pad, y + pad),
                        available - 2.0 * pad,
                        size * 0.88,
                        false,
                        t.code,
                        false,
                    );
                    // No card: code has no actions here, and its monospace
                    // and colours set it apart (household rule 25f)
                    let height = shaped.size.height + 2.0 * pad;
                    y += height;
                    Laid::Cells(vec![shaped])
                }
                Block::Math { latex, .. } => {
                    match math::render(latex, true, size * t.math_scale, rgb(t.text)) {
                        Ok(formula) => {
                            let scale = (available / formula.width).min(1.0);
                            let (w, h) = (formula.width * scale, formula.height * scale);
                            let x = if t.display_start {
                                x0
                            } else {
                                x0 + ((available - w) / 2.0).max(0.0)
                            };
                            let rect =
                                Rectangle::new(Point::new(x, y + size * 0.2), Size::new(w, h));
                            y += h + size * 0.4;
                            Laid::Math(Ok(formula), rect)
                        }
                        Err(_) => {
                            let pad = size * 0.5;
                            let shaped = shape(
                                font_system,
                                t,
                                code_cell("", latex),
                                0,
                                Point::new(x0 + pad, y + pad),
                                available - 2.0 * pad,
                                size * 0.88,
                                false,
                                t.error,
                                false,
                            );
                            let rect = Rectangle::new(
                                Point::new(x0, y),
                                Size::new(available, shaped.size.height + 2.0 * pad),
                            );
                            // Its source in the error colour, without a card
                            y += rect.height;
                            Laid::Math(Err(shaped), rect)
                        }
                    }
                }
                Block::Image { url, alt, .. } => match self.resolve_image(url).and_then(|path| {
                    let (w, h) = image_size(&path)?;
                    Some((path, w, h))
                }) {
                    Some((path, w, h)) => {
                        let max = available.min(t.image_width);
                        let scale = (max / w as f32).min(size * 24.0 / h as f32).min(1.0);
                        let rect = Rectangle::new(
                            Point::new(x0, y),
                            Size::new(w as f32 * scale, h as f32 * scale),
                        );
                        y += rect.height;
                        Laid::Image(path, rect)
                    }
                    None => {
                        let cell = cell_from_runs(&[Run {
                            text: format!("[{}]", if alt.is_empty() { url } else { alt }),
                            style: Style {
                                italic: true,
                                ..Style::default()
                            },
                            link: Some(url.clone()),
                        }]);
                        let shaped = shape(
                            font_system,
                            t,
                            cell,
                            0,
                            Point::new(x0, y),
                            available,
                            size,
                            false,
                            t.muted,
                            false,
                        );
                        y += shaped.size.height;
                        Laid::Cells(vec![shaped])
                    }
                },
                Block::Chart { spec, .. } => {
                    // A label's text, shaped as the chart will draw it
                    let label = |font_system: &mut cosmic_text::FontSystem,
                                 text: &str,
                                 family: &str,
                                 size: f32,
                                 bold: bool,
                                 italic: bool,
                                 color: Color| {
                        let lt = Typography {
                            body: family.to_string(),
                            italic,
                            line_height: 1.2,
                            justify: false,
                            ..t.clone()
                        };
                        shape(
                            font_system,
                            &lt,
                            plain_cell(text),
                            0,
                            Point::ORIGIN,
                            f32::MAX / 4.0,
                            size,
                            bold,
                            color,
                            false,
                        )
                    };
                    let first_line = |s: &Shaped| {
                        s.buffer
                            .layout_runs()
                            .next()
                            .map(|run| (run.line_w, run.line_height, run.line_y - run.line_top))
                    };
                    let width = available.min(size * 42.0);
                    let mut measure =
                        |text: &str, family: &str, size: f32, bold: bool, italic: bool| {
                            let shaped =
                                label(font_system, text, family, size, bold, italic, t.text);
                            first_line(&shaped).unwrap_or((0.0, size * 1.2, size))
                        };
                    match chart::render(spec, width, size * 0.8, t.page, t.text, &mut measure) {
                        Ok(drawn) => {
                            let x = x0 + ((available - drawn.size.width) / 2.0).max(0.0);
                            let rect = Rectangle::new(Point::new(x, y + size * 0.4), drawn.size);
                            let mut cells = Vec::with_capacity(drawn.labels.len());
                            let mut start = 0;
                            for l in &drawn.labels {
                                let mut shaped = label(
                                    font_system,
                                    &l.text,
                                    &l.family,
                                    l.size,
                                    l.bold,
                                    l.italic,
                                    l.color,
                                );
                                let line_y = shaped
                                    .buffer
                                    .layout_runs()
                                    .next()
                                    .map_or(0.0, |run| run.line_y);
                                shaped.origin =
                                    Vector::new(rect.x + l.x, rect.y + l.baseline - line_y);
                                shaped.start = start;
                                start += l.text.len() + 1;
                                cells.push(shaped);
                            }
                            let mut labels = CHART_LABELS.lock().unwrap();
                            if labels.len() > 256 {
                                labels.clear();
                            }
                            labels.insert(
                                spec_key(spec),
                                Arc::new(drawn.labels.iter().map(|l| l.text.clone()).collect()),
                            );
                            drop(labels);
                            y = rect.y + rect.height + size * 0.4;
                            figure = Some((Figure::Chart(Arc::new(drawn)), rect));
                            Laid::Cells(cells)
                        }
                        Err(err) => {
                            // Why it could not be drawn, and its source
                            let pad = size * 0.5;
                            let text = format!("chartgen: {err}\n\n{spec}");
                            let shaped = shape(
                                font_system,
                                t,
                                code_cell("", &text),
                                0,
                                Point::new(x0 + pad, y + pad),
                                available - 2.0 * pad,
                                size * 0.88,
                                false,
                                t.error,
                                false,
                            );
                            let rect = Rectangle::new(
                                Point::new(x0, y),
                                Size::new(available, shaped.size.height + 2.0 * pad),
                            );
                            // Its source in the error colour, without a card
                            y += rect.height;
                            Laid::Math(Err(shaped), rect)
                        }
                    }
                }
                Block::Diagram { source, .. } => {
                    diagram::prepare(font_system);
                    match diagram::render(source, &t.diagram_palette()) {
                        Ok(drawn) => {
                            // Scaled down to fit the column, and to about
                            // two thirds of a window's height
                            let scale = (available / drawn.width)
                                .min(size * 40.0 / drawn.height)
                                .min(1.0);
                            // In whole pixels: drawn at a fractional size, the
                            // GPU renderer loses its thinnest lines
                            let (w, h) = (
                                (drawn.width * scale).floor(),
                                (drawn.height * scale).floor(),
                            );
                            let x = x0 + ((available - w) / 2.0).max(0.0);
                            let rect =
                                Rectangle::new(Point::new(x, y + size * 0.4), Size::new(w, h));
                            let lt = Typography {
                                body: drawn.family.clone(),
                                italic: false,
                                line_height: 1.2,
                                justify: false,
                                ..t.clone()
                            };
                            let mut cells = Vec::with_capacity(drawn.labels.len());
                            let mut start = 0;
                            for l in &drawn.labels {
                                let mut lt = lt.clone();
                                lt.italic = l.italic;
                                let mut shaped = shape(
                                    font_system,
                                    &lt,
                                    plain_cell(&l.text),
                                    0,
                                    Point::ORIGIN,
                                    f32::MAX / 4.0,
                                    l.size * scale,
                                    l.bold,
                                    l.color,
                                    false,
                                );
                                let line_y = shaped
                                    .buffer
                                    .layout_runs()
                                    .next()
                                    .map_or(0.0, |run| run.line_y);
                                shaped.origin = Vector::new(
                                    rect.x + l.x * scale,
                                    rect.y + l.baseline * scale - line_y,
                                );
                                shaped.start = start;
                                start += l.text.len() + 1;
                                cells.push(shaped);
                            }
                            y = rect.y + rect.height + size * 0.4;
                            figure = Some((Figure::Diagram(drawn, source.as_str().into()), rect));
                            Laid::Cells(cells)
                        }
                        Err(err) => {
                            // Why it could not be drawn, and its source
                            let pad = size * 0.5;
                            let text = format!("mermaid: {err}\n\n{source}");
                            let shaped = shape(
                                font_system,
                                t,
                                code_cell("", &text),
                                0,
                                Point::new(x0 + pad, y + pad),
                                available - 2.0 * pad,
                                size * 0.88,
                                false,
                                t.error,
                                false,
                            );
                            let rect = Rectangle::new(
                                Point::new(x0, y),
                                Size::new(available, shaped.size.height + 2.0 * pad),
                            );
                            // Its source in the error colour, without a card
                            y += rect.height;
                            Laid::Math(Err(shaped), rect)
                        }
                    }
                }
                Block::Rule => {
                    let rect =
                        Rectangle::new(Point::new(0.0, y + size * 0.4), Size::new(width, 1.0));
                    y += size * 0.8;
                    Laid::Rule(rect)
                }
                Block::Table { rows } => {
                    let pad = size * 0.4;
                    let cells = block_cells(block);
                    let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                    // Natural widths, the text at its size. A table too
                    // wide for the column keeps them and scrolls sideways
                    // in it, rather than being squashed to fit (the plan,
                    // 16 Oct: "Wide tables scroll sideways")
                    let mut natural = vec![size * 2.0; columns];
                    let mut index = 0;
                    for (r, row) in rows.iter().enumerate() {
                        for (c, _) in row.iter().enumerate() {
                            let (_, cell) = &cells[index];
                            index += 1;
                            let probe = shape(
                                font_system,
                                t,
                                cell.clone(),
                                0,
                                Point::ORIGIN,
                                f32::MAX / 4.0,
                                size,
                                r == 0,
                                t.text,
                                false,
                            );
                            natural[c] = natural[c].max(probe.size.width.ceil() + 2.0 * pad + 1.0);
                        }
                    }
                    let total: f32 = natural.iter().sum();
                    let widths = natural;
                    let table_top = y;
                    let mut shaped_cells = Vec::new();
                    let mut index = 0;
                    for (r, row) in rows.iter().enumerate() {
                        let mut x = x0;
                        let mut row_height: f32 = 0.0;
                        let first = shaped_cells.len();
                        for (c, _) in row.iter().enumerate() {
                            let (start, cell) = cells[index].clone();
                            index += 1;
                            let shaped = shape(
                                font_system,
                                t,
                                cell,
                                start,
                                Point::new(x + pad, y + pad),
                                widths[c] - 2.0 * pad,
                                size,
                                r == 0,
                                t.text,
                                false,
                            );
                            row_height = row_height.max(shaped.size.height + 2.0 * pad);
                            x += widths[c];
                            shaped_cells.push(shaped);
                        }
                        // Each cell spans its row, for hits below its text
                        for shaped in &mut shaped_cells[first..] {
                            shaped.size.height = row_height - 2.0 * pad;
                        }
                        y += row_height;
                        // A rule under each row but the last; the
                        // heading's a stronger one, and no fill behind it
                        // (household rule 25f), as the conversations' tables
                        if r + 1 < rows.len() {
                            let (thickness, color) = if r == 0 {
                                (1.5, Color { a: 0.5, ..t.strong })
                            } else {
                                (1.0, t.rule)
                            };
                            panels.push((
                                Rectangle::new(
                                    Point::new(x0, y - thickness / 2.0),
                                    Size::new(total, thickness),
                                ),
                                color,
                                0.0,
                            ));
                        }
                    }
                    if total > available + 0.5 {
                        wide = Some((
                            Rectangle::new(
                                Point::new(x0, table_top),
                                Size::new(total, y - table_top),
                            ),
                            available,
                        ));
                        // Room for its scrollbar under it
                        y += 2.0 * TABLE_BAR;
                    }
                    Laid::Cells(shaped_cells)
                }
            };
            let bottom = y;
            for panel in panels.iter_mut().take(usize::from(quote)) {
                panel.0.height = bottom - top;
            }
            let marker = marker.map(|marker| {
                let cell = cell_from_runs(&[Run {
                    text: marker.to_string(),
                    style: Style::default(),
                    link: None,
                }]);
                let mut shaped = shape(
                    font_system,
                    t,
                    cell,
                    0,
                    Point::ORIGIN,
                    size * 3.0,
                    size,
                    false,
                    t.muted,
                    false,
                );
                // On the baseline of the item's first line, however tall
                let baseline_of =
                    |s: &Shaped| s.buffer.layout_runs().next().map_or(0.0, |run| run.line_y);
                let first = match &laid {
                    Laid::Cells(cells) => {
                        cells.first().map(|cell| cell.origin.y + baseline_of(cell))
                    }
                    Laid::Math(Ok(formula), rect) => {
                        Some(rect.y + formula.baseline * rect.height / formula.height)
                    }
                    _ => None,
                };
                let y = first.map_or(top, |line_y| line_y - baseline_of(&shaped));
                shaped.origin = Vector::new(x0 - shaped.size.width - size * 0.45, y);
                shaped
            });
            let layout = BlockLayout {
                top,
                bottom,
                laid,
                panels,
                marker,
                figure,
                wide,
                len: block_len(block),
            };
            reused.insert(key, layout.shifted(-top));
            blocks.push(layout);
        }
        *built = reused;
        (blocks, y)
    }
}

/// The advance of [`PLACEHOLDER`] in the body font, measured once per font
/// and size
fn placeholder_width(
    font_system: &mut cosmic_text::FontSystem,
    family: &str,
    bold: bool,
    italic: bool,
    size: f32,
) -> f32 {
    thread_local! {
        static WIDTHS: std::cell::RefCell<std::collections::HashMap<(String, bool, bool, u32), f32>> =
            Default::default();
    }
    let key = (family.to_string(), bold, italic, size.to_bits());
    if let Some(width) = WIDTHS.with(|w| w.borrow().get(&key).copied()) {
        return width;
    }
    let width = {
        let mut buffer = Buffer::new(font_system, Metrics::new(size, size));
        let attrs = Attrs::new()
            .family(if family.is_empty() {
                Family::SansSerif
            } else {
                Family::Name(family)
            })
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
            .style(if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            });
        buffer.set_text(PLACEHOLDER, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(font_system, false);
        buffer
            .layout_runs()
            .next()
            .map_or(size * 0.5, |run| run.line_w)
    };
    WIDTHS.with(|w| w.borrow_mut().insert(key, width));
    width
}

/// Shape a cell into a buffer at `origin`, `width` wide, at the prose's
/// line spacing
#[allow(clippy::too_many_arguments)]
fn shape(
    font_system: &mut cosmic_text::FontSystem,
    t: &Typography,
    cell: Cell,
    start: usize,
    origin: Point,
    width: f32,
    size: f32,
    bold: bool,
    color: Color,
    justify: bool,
) -> Shaped {
    shape_lined(
        font_system,
        t,
        t.line_height,
        cell,
        start,
        origin,
        width,
        size,
        bold,
        color,
        justify,
    )
}

/// [`shape`], its lines `line` of its size apart (a heading's)
#[allow(clippy::too_many_arguments)]
fn shape_lined(
    font_system: &mut cosmic_text::FontSystem,
    t: &Typography,
    line: f32,
    cell: Cell,
    start: usize,
    origin: Point,
    width: f32,
    size: f32,
    bold: bool,
    color: Color,
    justify: bool,
) -> Shaped {
    let line_height = size * line;
    let mut buffer = Buffer::new(font_system, Metrics::new(size, line_height));
    buffer.set_wrap(Wrap::WordOrGlyph);
    buffer.set_size(Some(width.max(1.0)), None);
    let base = Attrs::new()
        .family(t.family())
        .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
        .style(if t.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        })
        .color(ct_color(color));
    let mut formulas_set: Vec<Option<Arc<math::Formula>>> = Vec::new();
    let spans: Vec<(&str, Attrs)> = cell
        .spans
        .iter()
        .map(|span| {
            let text = &cell.text[span.range.clone()];
            match &span.kind {
                SpanKind::Text { style, link, tone } => {
                    let mut attrs = base.clone();
                    if style.code {
                        attrs = attrs.family(t.mono_family()).color(ct_color(t.code));
                        if size == t.size {
                            attrs = attrs.metrics(Metrics::new(size * 0.92, line_height));
                        }
                    }
                    if style.bold || bold {
                        attrs = attrs.weight(Weight::BOLD);
                    }
                    // Bold in running text, as a table's head, takes the
                    // darkest tone, as headings do
                    if (style.bold || bold) && !style.code && color == t.text {
                        attrs = attrs.color(ct_color(t.strong));
                    }
                    if style.italic != t.italic {
                        attrs = attrs.style(FontStyle::Italic);
                    }
                    if link.is_some() {
                        let strong = style.bold || bold || color == t.strong;
                        attrs = attrs.color(ct_color(if strong { t.link_strong } else { t.link }));
                    }
                    match tone {
                        Tone::Code(class) => {
                            attrs = attrs.color(ct_color(crate::highlight::color(*class, t)));
                            if crate::highlight::italic(*class) {
                                attrs = attrs.style(FontStyle::Italic);
                            }
                        }
                        Tone::Normal => {}
                    }
                    (text, attrs)
                }
                // Its source, as it could not be set
                SpanKind::Math(_) if text != PLACEHOLDER => (
                    text,
                    base.clone()
                        .family(t.mono_family())
                        .color(ct_color(t.error)),
                ),
                SpanKind::Math(latex) => {
                    let formula = math::render(latex, false, size * t.math_scale, rgb(color)).ok();
                    let placeholder = placeholder_width(font_system, &t.body, bold, t.italic, size);
                    let attrs = match &formula {
                        Some(formula) => {
                            // Room for the formula: its width, and a line tall
                            // enough to hold it about the text's baseline
                            let above = formula.baseline - size * 0.8;
                            let below = (formula.height - formula.baseline) - size * 0.25;
                            let extra = 2.0 * above.max(below).max(0.0);
                            base.clone()
                                .color(cosmic_text::Color::rgba(0, 0, 0, 0))
                                .letter_spacing((formula.width - placeholder) / size)
                                .metrics(Metrics::new(size, line_height + extra))
                                .metadata(formulas_set.len() + 1)
                        }
                        None => base.clone(),
                    };
                    formulas_set.push(formula);
                    (text, attrs)
                }
            }
        })
        .collect();
    buffer.set_rich_text(
        spans.iter().map(|(text, attrs)| (*text, attrs.clone())),
        &base,
        Shaping::Advanced,
        justify.then_some(Align::Justified),
    );
    buffer.shape_until_scroll(font_system, false);
    let mut measured = Size::new(0.0f32, 0.0f32);
    let mut formulas = Vec::new();
    let formulas_ok = formulas_set;
    for run in buffer.layout_runs() {
        measured.width = measured.width.max(run.line_w);
        measured.height = measured.height.max(run.line_top + run.line_height);
        for glyph in run.glyphs {
            if glyph.metadata > 0
                && let Some(Some(formula)) = formulas_ok.get(glyph.metadata - 1)
            {
                let x = origin.x + glyph.x + (glyph.w - formula.width) / 2.0;
                let y = origin.y + run.line_y - formula.baseline;
                formulas.push((formula.clone(), Point::new(x, y)));
            }
        }
    }
    if cell.text.is_empty() {
        measured.height = line_height;
    }
    let mut line_starts = Vec::new();
    let mut at = 0;
    for line in cell.text.split('\n') {
        line_starts.push(at);
        at += line.len() + 1;
    }
    Shaped {
        buffer: Arc::new(buffer),
        origin: Vector::new(origin.x, origin.y),
        size: measured,
        start,
        cell,
        line_starts,
        formulas,
        color,
    }
}

impl Shaped {
    fn rect(&self) -> Rectangle {
        Rectangle::new(Point::new(self.origin.x, self.origin.y), self.size)
    }

    /// The offset in the block's linear text nearest `point`
    fn hit(&self, point: Point) -> usize {
        let local = point - self.origin;
        if local.y < 0.0 {
            return self.start;
        }
        if local.y > self.size.height {
            return self.start + self.cell.text.len();
        }
        let cursor = self.buffer.hit(local.x.max(0.0), local.y);
        self.start + cursor.map_or(self.cell.text.len(), |cursor| self.offset_of(cursor))
    }

    fn offset_of(&self, cursor: Cursor) -> usize {
        self.line_starts.get(cursor.line).copied().unwrap_or(0) + cursor.index
    }

    fn cursor_at(&self, offset: usize) -> Cursor {
        let line = self
            .line_starts
            .iter()
            .rposition(|start| *start <= offset)
            .unwrap_or(0);
        Cursor::new(line, offset - self.line_starts[line])
    }

    fn link_at(&self, offset: usize) -> Option<&str> {
        let local = offset.checked_sub(self.start)?;
        self.cell.spans.iter().find_map(|span| match &span.kind {
            SpanKind::Text {
                link: Some(link), ..
            } if span.range.contains(&local) => Some(link.as_str()),
            _ => None,
        })
    }

    /// Highlight `range` of the block's linear text
    fn draw_selection(
        &self,
        renderer: &mut Renderer,
        at: Vector,
        range: Range<usize>,
        color: Color,
    ) {
        let start = range.start.max(self.start) - self.start;
        let end = range
            .end
            .min(self.start + self.cell.text.len())
            .saturating_sub(self.start);
        if start >= end
            && !(range.end > self.start + self.cell.text.len() && range.start <= self.start)
        {
            return;
        }
        let (c_start, c_end) = (self.cursor_at(start), self.cursor_at(end.max(start)));
        for run in self.buffer.layout_runs() {
            if run.line_i < c_start.line || run.line_i > c_end.line {
                continue;
            }
            for (x, w) in run.highlight(c_start, c_end) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(
                                at.x + self.origin.x + x,
                                at.y + self.origin.y + run.line_top,
                            ),
                            Size::new(w.max(2.0), run.line_height),
                        ),
                        ..renderer::Quad::default()
                    },
                    Background::Color(color),
                );
            }
        }
    }
}

impl Shaped {
    fn shift(&mut self, dy: f32) {
        self.origin.y += dy;
        for (_, point) in &mut self.formulas {
            point.y += dy;
        }
    }
}

impl BlockLayout {
    /// The same layout, `dy` lower
    fn shifted(&self, dy: f32) -> Self {
        let mut out = self.clone();
        out.top += dy;
        out.bottom += dy;
        for (rect, _, _) in &mut out.panels {
            rect.y += dy;
        }
        if let Some(marker) = &mut out.marker {
            marker.shift(dy);
        }
        if let Some((_, rect)) = &mut out.figure {
            rect.y += dy;
        }
        if let Some((rect, _)) = &mut out.wide {
            rect.y += dy;
        }
        match &mut out.laid {
            Laid::Cells(cells) => cells.iter_mut().for_each(|cell| cell.shift(dy)),
            Laid::Math(result, rect) => {
                rect.y += dy;
                if let Err(shaped) = result {
                    shaped.shift(dy);
                }
            }
            Laid::Image(_, rect) | Laid::Rule(rect) => rect.y += dy,
        }
        out
    }
}

impl State {
    /// The position nearest `point`, relative to the widget
    fn hit(&self, point: Point) -> Pos {
        if self.blocks.is_empty() {
            return Pos::START;
        }
        let index = self
            .blocks
            .iter()
            .position(|block| point.y < block.bottom)
            .unwrap_or(self.blocks.len() - 1);
        let block = &self.blocks[index];
        if point.y < block.top && index > 0 {
            // Between blocks: the start of the lower one
            return Pos {
                block: index,
                offset: 0,
            };
        }
        // Through a wide table's sideways scroll
        let point = point + Vector::new(self.table_dx(index), 0.0);
        let offset = match &block.laid {
            Laid::Cells(cells) => {
                // The cell under the point, else the nearest above it
                let cell = cells
                    .iter()
                    .rev()
                    .find(|cell| {
                        let rect = cell.rect();
                        point.y >= rect.y - 1.0
                            && point.x >= rect.x - cell.buffer.metrics().font_size
                    })
                    .or(cells.first());
                cell.map_or(0, |cell| cell.hit(point))
            }
            Laid::Math(Err(shaped), _) => shaped.hit(point),
            Laid::Math(Ok(_), rect) | Laid::Image(_, rect) => {
                usize::from(point.x > rect.center_x())
            }
            Laid::Rule(_) => 0,
        };
        Pos {
            block: index,
            offset,
        }
    }

    /// The word around `pos`, or the block's whole text for `whole`
    fn around(&self, pos: Pos, whole: bool) -> (Pos, Pos) {
        let Some(block) = self.blocks.get(pos.block) else {
            return (pos, pos);
        };
        if whole {
            return (
                Pos {
                    block: pos.block,
                    offset: 0,
                },
                Pos {
                    block: pos.block,
                    offset: block.len,
                },
            );
        }
        let Laid::Cells(cells) = &block.laid else {
            return (
                Pos {
                    block: pos.block,
                    offset: 0,
                },
                Pos {
                    block: pos.block,
                    offset: block.len,
                },
            );
        };
        let Some(cell) = cells.iter().find(|cell| {
            pos.offset >= cell.start && pos.offset <= cell.start + cell.cell.text.len()
        }) else {
            return (pos, pos);
        };
        let text = &cell.cell.text;
        let local = (pos.offset - cell.start).min(text.len());
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let start = text[..local]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(local, |(i, _)| i);
        let end = text[local..]
            .char_indices()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(local, |(i, c)| local + i + c.len_utf8());
        let (start, end) = if start == end && local < text.len() {
            (
                local,
                local + text[local..].chars().next().map_or(0, char::len_utf8),
            )
        } else {
            (start, end)
        };
        (
            Pos {
                block: pos.block,
                offset: cell.start + start,
            },
            Pos {
                block: pos.block,
                offset: cell.start + end,
            },
        )
    }

    /// How far block `index`'s table is scrolled sideways: 0 unless it is
    /// wider than its column
    fn table_dx(&self, index: usize) -> f32 {
        match self.blocks.get(index).and_then(|block| block.wide) {
            Some((rect, view)) => self
                .table_x
                .get(&index)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, (rect.width - view).max(0.0)),
            None => 0.0,
        }
    }

    /// The table wider than its column under `point` (relative to the
    /// widget), its scrollbar included: its block, and how far it can
    /// scroll
    fn wide_table_at(&self, point: Point) -> Option<(usize, f32)> {
        self.blocks.iter().enumerate().find_map(|(index, block)| {
            let (rect, view) = block.wide?;
            let shown = Rectangle {
                width: view,
                height: rect.height + TABLE_BAR * 3.0,
                ..rect
            };
            shown
                .contains(point)
                .then_some((index, (rect.width - view).max(0.0)))
        })
    }

    fn link_at(&self, point: Point) -> Option<&str> {
        let pos = self.hit(point);
        let block = self.blocks.get(pos.block)?;
        let point = point + Vector::new(self.table_dx(pos.block), 0.0);
        let Laid::Cells(cells) = &block.laid else {
            return None;
        };
        cells
            .iter()
            .find(|cell| cell.rect().contains(point))
            .and_then(|cell| cell.link_at(pos.offset))
    }

    /// The source of the diagram under `point`
    fn diagram_at(&self, point: Point) -> Option<&str> {
        self.blocks.iter().find_map(|block| match &block.figure {
            Some((Figure::Diagram(_, source), rect)) if rect.contains(point) => Some(&**source),
            _ => None,
        })
    }

    /// Whether `point` is on a diagram but off its text
    fn on_diagram_ground(&self, point: Point) -> bool {
        self.blocks
            .iter()
            .any(|block| match (&block.figure, &block.laid) {
                (Some((Figure::Diagram(..), rect)), Laid::Cells(cells)) => {
                    rect.contains(point) && !cells.iter().any(|cell| cell.rect().contains(point))
                }
                _ => false,
            })
    }

    fn image_at(&self, point: Point) -> Option<&Path> {
        self.blocks.iter().find_map(|block| match &block.laid {
            Laid::Image(path, rect) if rect.contains(point) => Some(path.as_path()),
            _ => None,
        })
    }
}

impl<'a, Message> Widget<Message, cosmic::Theme, Renderer> for RichText<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let width = limits.max().width;
        let key = (self.key(), width.to_bits());
        if state.key != Some(key) {
            // A new style changes every block
            if state.style != Some(self.typography.key()) {
                state.built.clear();
                state.style = Some(self.typography.key());
            }
            let (blocks, height) = self.build(width, &mut state.built);
            state.blocks = blocks;
            state.height = height;
            state.key = Some(key);
        }
        layout::Node::new(limits.resolve(
            Length::Fill,
            Length::Shrink,
            Size::new(width, state.height),
        ))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(point) = cursor.position_in(bounds) else {
                    return;
                };
                let Some(on_select) = &self.on_select else {
                    return;
                };
                let now = Instant::now();
                let count = match state.last_click {
                    Some((at, last, count))
                        if now - at < Duration::from_millis(450) && last.distance(point) < 6.0 =>
                    {
                        count % 3 + 1
                    }
                    _ => 1,
                };
                state.last_click = Some((now, point, count));
                state.pressed = Some(point);
                let pos = state.hit(point);
                let extend = shell_modifiers_shift();
                let message = match count {
                    1 => on_select(Select::Press { pos, extend }),
                    2 => {
                        let (start, end) = state.around(pos, false);
                        on_select(Select::Range(start, end))
                    }
                    _ => {
                        let (start, end) = state.around(pos, true);
                        on_select(Select::Range(start, end))
                    }
                };
                shell.publish(message);
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if self.dragging => {
                let Some(on_select) = &self.on_select else {
                    return;
                };
                let Some(point) = cursor.position() else {
                    return;
                };
                // Only the widget the pointer is level with follows it
                if point.y >= bounds.y && point.y < bounds.y + bounds.height {
                    let local = Point::new(point.x - bounds.x, point.y - bounds.y);
                    if let Some(pressed) = state.pressed
                        && pressed.distance(local) > 3.0
                    {
                        state.pressed = None;
                    }
                    shell.publish(on_select(Select::Drag(state.hit(local))));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let Some(pressed) = state.pressed.take() else {
                    return;
                };
                let Some(point) = cursor.position_in(bounds) else {
                    return;
                };
                if pressed.distance(point) > 3.0 || state.last_click.is_some_and(|(_, _, n)| n > 1)
                {
                    return;
                }
                if let Some(link) = state.link_at(point)
                    && let Some(on_link) = &self.on_link
                {
                    shell.publish(on_link(link.to_string()));
                } else if let Some(path) = state.image_at(point)
                    && let Some(on_image) = &self.on_image
                {
                    shell.publish(on_image(path.to_path_buf()));
                } else if let Some(source) = state.diagram_at(point)
                    && let Some(on_diagram) = &self.on_diagram
                {
                    shell.publish(on_diagram(source.to_string()));
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift = modifiers.shift()
            }
            // Across a table wider than its column: sideways, by a sideways
            // wheel or touchpad, or Shift with the wheel, as in the Paper
            // and Text views
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let Some(point) = cursor.position_in(bounds) else {
                    return;
                };
                let Some((index, most)) = state.wide_table_at(point) else {
                    return;
                };
                let shift = state.shift || shell_modifiers_shift();
                let (x, y, line) = match *delta {
                    mouse::ScrollDelta::Lines { x, y } => (x, y, self.typography.size * 3.0),
                    mouse::ScrollDelta::Pixels { x, y } => (x, y, 1.0),
                };
                if !(x.abs() > y.abs() || (shift && y != 0.0)) {
                    return;
                }
                let by = -(if x.abs() > y.abs() { x } else { y }) * line;
                let now = state
                    .table_x
                    .get(&index)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, most);
                state.table_x.insert(index, (now + by).clamp(0.0, most));
                shell.request_redraw();
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let Some(point) = cursor.position_in(layout.bounds()) else {
            return mouse::Interaction::default();
        };
        if state.link_at(point).is_some()
            || state.image_at(point).is_some()
            || (self.on_diagram.is_some() && state.on_diagram_ground(point))
        {
            mouse::Interaction::Pointer
        } else if self.on_select.is_some() {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &cosmic::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };
        let at = Vector::new(bounds.x, bounds.y);
        let t = &*self.typography;
        for (index, block) in state.blocks.iter().enumerate() {
            // Only blocks on screen
            if bounds.y + block.bottom < visible.y
                || bounds.y + block.top > visible.y + visible.height
            {
                continue;
            }
            // A table wider than its column: drawn scrolled sideways, within
            // the column, its scrollbar under it
            if let Some((rect, view)) = block.wide {
                self.draw_wide_table(renderer, state, index, block, rect, view, at, visible);
                continue;
            }
            for (rect, color, radius) in &block.panels {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: *rect + at,
                        border: Border {
                            radius: (*radius).into(),
                            ..Border::default()
                        },
                        ..renderer::Quad::default()
                    },
                    Background::Color(*color),
                );
            }
            // The selected part of this block
            let selected = self.selection.and_then(|(start, end)| {
                if index < start.block || index > end.block {
                    return None;
                }
                let from = if index == start.block {
                    start.offset
                } else {
                    0
                };
                let to = if index == end.block {
                    end.offset.min(block.len)
                } else {
                    block.len
                };
                (from < to || (index != end.block && block.len == 0)).then_some(from..to)
            });
            match &block.figure {
                Some((Figure::Chart(chart), rect)) => {
                    use cosmic::iced::advanced::graphics::geometry::Renderer as _;
                    let geometry = chart.geometry(renderer);
                    renderer.with_translation(
                        Vector::new(at.x + rect.x, at.y + rect.y),
                        |renderer| {
                            renderer.draw_geometry(geometry);
                        },
                    );
                }
                Some((Figure::Diagram(diagram, _), rect)) => {
                    renderer.draw_svg(
                        adv_svg::Svg::new(diagram.handle.clone()),
                        *rect + at,
                        visible,
                    );
                }
                None => {}
            }
            match &block.laid {
                Laid::Cells(cells) => {
                    let draw_cells = |renderer: &mut Renderer| {
                        for cell in cells {
                            if let Some(range) = &selected {
                                cell.draw_selection(renderer, at, range.clone(), t.selection);
                            }
                            draw_cell(renderer, cell, at, visible);
                        }
                    };
                    // Over a diagram, whose SVG would hide the highlight,
                    // as quads go under images in a layer
                    if let Some((Figure::Diagram(..), rect)) = &block.figure {
                        match (*rect + at).intersection(&visible) {
                            Some(clip) => renderer.with_layer(clip, draw_cells),
                            None => {}
                        }
                    } else {
                        draw_cells(renderer);
                    }
                }
                Laid::Math(result, rect) => {
                    if selected.is_some() {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: *rect + at,
                                ..renderer::Quad::default()
                            },
                            Background::Color(t.selection),
                        );
                    }
                    match result {
                        Ok(formula) => renderer.draw_svg(
                            adv_svg::Svg::new(formula.handle.clone()),
                            *rect + at,
                            visible,
                        ),
                        Err(shaped) => draw_cell(renderer, shaped, at, visible),
                    }
                }
                Laid::Image(path, rect) => {
                    if crate::images::is_svg(path) {
                        let handle = adv_svg::Handle::from_path(path);
                        renderer.draw_svg(adv_svg::Svg::new(handle), *rect + at, visible);
                    } else {
                        let handle = adv_image::Handle::from_path(thumbnail_of(path));
                        renderer.draw_image(
                            adv_image::Image {
                                border_radius: (t.size * 0.3).into(),
                                ..adv_image::Image::new(handle)
                            },
                            *rect + at,
                            visible,
                        );
                    }
                    if selected.is_some() {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: *rect + at,
                                ..renderer::Quad::default()
                            },
                            Background::Color(t.selection),
                        );
                    }
                }
                Laid::Rule(rect) => {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: *rect + at,
                            ..renderer::Quad::default()
                        },
                        Background::Color(t.rule),
                    );
                }
            }
            if let Some(marker) = &block.marker {
                draw_cell(renderer, marker, at, visible);
            }
        }
    }
}

impl<'a, Message> RichText<'a, Message> {
    /// Block `index`, a table wider than its column (`rect`, `view` of it
    /// shown): its rules and cells scrolled sideways by
    /// [`State::table_dx`] and cut at the column's edge, and under it a
    /// thin bar marking the part in view, in the Paper and Text views'
    /// tones (the rule's, the thumb muted)
    #[allow(clippy::too_many_arguments)]
    fn draw_wide_table(
        &self,
        renderer: &mut Renderer,
        state: &State,
        index: usize,
        block: &BlockLayout,
        rect: Rectangle,
        view: f32,
        at: Vector,
        visible: Rectangle,
    ) {
        let t = &*self.typography;
        let dx = state.table_dx(index);
        let table_at = at - Vector::new(dx, 0.0);
        let Some(clip) = Rectangle::new(
            Point::new(at.x + rect.x, visible.y),
            Size::new(view, visible.height),
        )
        .intersection(&visible) else {
            return;
        };
        let selected = self.selection.and_then(|(start, end)| {
            if index < start.block || index > end.block {
                return None;
            }
            let from = if index == start.block {
                start.offset
            } else {
                0
            };
            let to = if index == end.block {
                end.offset.min(block.len)
            } else {
                block.len
            };
            (from < to || (index != end.block && block.len == 0)).then_some(from..to)
        });
        renderer.with_layer(clip, |renderer| {
            for (rect, color, radius) in &block.panels {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: *rect + table_at,
                        border: Border {
                            radius: (*radius).into(),
                            ..Border::default()
                        },
                        ..renderer::Quad::default()
                    },
                    Background::Color(*color),
                );
            }
            if let Laid::Cells(cells) = &block.laid {
                for cell in cells {
                    if let Some(range) = &selected {
                        cell.draw_selection(renderer, table_at, range.clone(), t.selection);
                    }
                    draw_cell(renderer, cell, table_at, clip);
                }
            }
        });
        let track = Rectangle::new(
            Point::new(at.x + rect.x, at.y + rect.y + rect.height + TABLE_BAR),
            Size::new(view, TABLE_BAR),
        );
        let round = Border {
            radius: (TABLE_BAR / 2.0).into(),
            ..Border::default()
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: track,
                border: round,
                ..renderer::Quad::default()
            },
            Background::Color(t.rule),
        );
        // Its share of the track, kept within it
        let thumb_w = (view * view / rect.width).max(TABLE_BAR * 4.0).min(view);
        let most = (rect.width - view).max(1.0);
        let thumb = Rectangle::new(
            Point::new(track.x + dx / most * (view - thumb_w), track.y),
            Size::new(thumb_w, TABLE_BAR),
        );
        renderer.fill_quad(
            renderer::Quad {
                bounds: thumb,
                border: round,
                ..renderer::Quad::default()
            },
            Background::Color(t.muted),
        );
    }
}

fn draw_cell(renderer: &mut Renderer, cell: &Shaped, at: Vector, clip: Rectangle) {
    // On a whole pixel: glyph rows are cut toward zero from where the text
    // starts, so text starting above the window's top was drawn a pixel
    // lower, and jumped as it crossed the top while scrolling
    let position = Point::new(at.x + cell.origin.x, (at.y + cell.origin.y).round());
    renderer.fill_raw(Raw {
        buffer: Arc::downgrade(&cell.buffer),
        position,
        color: cell.color,
        clip_bounds: clip,
    });
    for (formula, point) in &cell.formulas {
        let bounds = Rectangle::new(
            Point::new(at.x + point.x, at.y + point.y),
            Size::new(formula.width, formula.height),
        );
        renderer.draw_svg(adv_svg::Svg::new(formula.handle.clone()), bounds, clip);
    }
}

/// Whether Shift is held, as the widget sees no modifier events of its own
fn shell_modifiers_shift() -> bool {
    SHIFT.load(std::sync::atomic::Ordering::Relaxed)
}

static SHIFT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Told by the app as modifiers change
pub fn set_shift(held: bool) {
    SHIFT.store(held, std::sync::atomic::Ordering::Relaxed);
}

impl<'a, Message: 'a> From<RichText<'a, Message>>
    for Element<'a, Message, cosmic::Theme, Renderer>
{
    fn from(widget: RichText<'a, Message>) -> Self {
        Element::new(widget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;

    #[test]
    fn copies_formulas_as_latex_and_tables_with_tabs() {
        let doc = parse("Let $x^2$ be.\n\n$$y$$\n\n| a | b |\n|---|---|\n| c | d |");
        let all = copy(&doc, Pos::START, end_of(&doc));
        assert_eq!(all, "Let $x^2$ be.\n\n$$\ny\n$$\n\na\tb\nc\td");
    }

    #[test]
    fn copies_part_of_a_paragraph() {
        let doc = parse("Hello brave world");
        assert_eq!(
            copy(
                &doc,
                Pos {
                    block: 0,
                    offset: 6
                },
                Pos {
                    block: 0,
                    offset: 11
                }
            ),
            "brave"
        );
    }

    /// Headings' lines close up as they grow (household rule 23a): H4 1.4,
    /// H3 1.35, H2 1.3, H1 1.2, never looser than the prose's
    #[test]
    fn headings_have_tighter_line_spacing_the_larger_they_are() {
        let lines: Vec<f32> = (1..=4)
            .map(|level| Typography::heading_line_height(level, 1.5))
            .collect();
        assert_eq!(lines, [1.2, 1.3, 1.35, 1.4]);
        assert_eq!(Typography::heading_line_height(4, 1.3), 1.3);
        assert_eq!(Typography::heading_line_height(0, 1.5), 1.5);
        let scales: Vec<f32> = (1..=4).map(Typography::heading_scale).collect();
        assert_eq!(scales, [1.55, 1.32, 1.16, 1.05]);
    }

    #[test]
    fn block_lengths_count_separators() {
        let doc = parse("| ab | c |\n|---|---|\n| d | e |");
        // "ab\tc\nd\te"
        assert_eq!(block_len(&doc.blocks[0]), 8);
    }

    fn typography() -> Typography {
        let ink = Color::from_rgb8(0x22, 0x22, 0x22);
        Typography {
            display_start: false,
            body: String::new(),
            mono: String::new(),
            size: 16.0,
            line_height: 1.5,
            math_scale: 1.1,
            justify: false,
            italic: false,
            text: ink,
            strong: ink,
            muted: ink,
            link: Color::from_rgb8(0x00, 0x5b, 0x3d),
            link_strong: Color::from_rgb8(0x00, 0x5b, 0x3d),
            code: ink,
            code_background: Color::WHITE,
            page: Color::WHITE,
            rule: Color::from_rgba8(0, 0, 0, 0.2),
            selection: Color::from_rgba8(0, 0, 255, 0.3),
            error: Color::from_rgb8(0xc0, 0x3a, 0x3a),
            added: Color::from_rgb8(0x3a, 0x9a, 0x4a),
            removed: Color::from_rgb8(0xc0, 0x3a, 0x3a),
            image_width: 640.0,
        }
    }

    /// A table of eight columns whose last cell is a link, between two paragraphs
    const WIDE: &str = "Before the table.\n\n| Disk | Model | Size | Hours | Errors | Temperature | Firmware | Notes |\n|---|---|---|---|---|---|---|---|\n\
        | sda | WD Red Plus | 8 TB | 21000 | 0 | 34 degrees | 83.00A83 | [the oldest](https://example.org/oldest) |\n\
        | sdb | WD Red Plus | 8 TB | 20950 | 1 | 35 degrees | 83.00A83 | one reallocated sector |\n\nAfter the table.\n";

    /// The widget laid out at `width`, as a window does, with the state it
    /// keeps and a way to send it events
    struct Rig {
        element: Element<'static, (), cosmic::Theme, Renderer>,
        tree: Tree,
        node: layout::Node,
        renderer: Renderer,
    }

    impl Rig {
        fn new(markdown: &str, width: f32) -> Rig {
            use cosmic::iced::advanced::renderer::Headless;
            let renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
                cosmic::iced::Font::default(),
                cosmic::iced::Pixels(14.0),
                Some("tiny-skia"),
            ))
            .expect("a headless renderer");
            let mut element: Element<'static, (), cosmic::Theme, Renderer> =
                rich_text::<()>(Arc::new(parse(markdown)), Arc::new(typography())).into();
            let mut tree = Tree::new(&element);
            let limits = layout::Limits::new(Size::ZERO, Size::new(width, 4000.0));
            let node = element
                .as_widget_mut()
                .layout(&mut tree, &renderer, &limits);
            Rig {
                element,
                tree,
                node,
                renderer,
            }
        }

        fn state(&self) -> &State {
            self.tree.state.downcast_ref::<State>()
        }

        /// The table (block 1)'s box and how much of it shows
        fn wide(&self) -> Option<(Rectangle, f32)> {
            self.state().blocks[1].wide
        }

        /// The events sent with the pointer at `at`; whether the widget took the last
        fn send(&mut self, at: Point, events: &[Event]) -> bool {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            let viewport = Rectangle::new(Point::ORIGIN, Size::new(4000.0, 4000.0));
            for event in events {
                self.element.as_widget_mut().update(
                    &mut self.tree,
                    event,
                    Layout::new(&self.node),
                    mouse::Cursor::Available(at),
                    &self.renderer,
                    &mut cosmic::iced::advanced::clipboard::Null,
                    &mut shell,
                    &viewport,
                );
            }
            shell.is_event_captured()
        }

        fn wheel(&mut self, at: Point, delta: mouse::ScrollDelta) -> bool {
            self.send(at, &[Event::Mouse(mouse::Event::WheelScrolled { delta })])
        }

        fn shift(&mut self, held: bool) {
            let modifiers = if held {
                keyboard::Modifiers::SHIFT
            } else {
                keyboard::Modifiers::empty()
            };
            self.send(
                Point::ORIGIN,
                &[Event::Keyboard(keyboard::Event::ModifiersChanged(
                    modifiers,
                ))],
            );
        }
    }

    /// A table wider than its column keeps its columns' widths and its
    /// text's size, whatever the column: it is cut at the column's edge
    /// (`wide` holds its box and the part shown) with room under it for
    /// its bar, where it was shrunk and its cells wrapped. A narrow table is
    /// laid out as before, and a table that fits is not wide
    #[test]
    fn a_table_wider_than_its_column_keeps_its_widths_and_scrolls() {
        let narrow = Rig::new(WIDE, 400.0);
        let (rect, view) = narrow.wide().expect("wide in a 400 px column");
        assert_eq!(view, 400.0, "the column shows 400 px of it");
        assert!(
            rect.width > 700.0,
            "its natural width, not squashed: {}",
            rect.width
        );
        // The same table in a column of another width: the same box
        let other = Rig::new(WIDE, 600.0);
        let (again, shown) = other.wide().expect("wide in a 600 px column");
        assert_eq!((again.width, shown), (rect.width, 600.0));
        assert_eq!(again.x, rect.x, "starts at the left edge, not centred");
        // Wide enough to hold it: nothing wide about it
        let room = Rig::new(WIDE, rect.width + 50.0);
        assert!(room.wide().is_none());
        assert_eq!(
            room.state().blocks[1].bottom - room.state().blocks[1].top,
            narrow.state().blocks[1].bottom - narrow.state().blocks[1].top - 2.0 * TABLE_BAR,
            "its bar takes room only when it shows"
        );
        // Its text at its size: the cells' rows are one line each, in a
        // 400 px column as in a wide one
        let (a, b) = (
            narrow.state().blocks[1].bottom - narrow.state().blocks[1].top - 2.0 * TABLE_BAR,
            room.state().blocks[1].bottom - room.state().blocks[1].top,
        );
        assert_eq!(a, b, "no cell wraps");
        // A narrow table (two columns) fits: not wide
        let small = Rig::new("| a | b |\n|---|---|\n| 1 | 2 |\n", 400.0);
        assert!(small.state().blocks[0].wide.is_none());
    }

    /// Across the wide table: a sideways wheel or touchpad, or Shift with
    /// the wheel, moves it, within its ends; a plain vertical wheel is left
    /// to the page; over the bar under it counts as over the table, and
    /// beside or below them does not
    #[test]
    fn the_wheel_scrolls_a_wide_table_sideways() {
        let mut rig = Rig::new(WIDE, 400.0);
        let (rect, view) = rig.wide().unwrap();
        let most = rect.width - view;
        let on = Point::new(100.0, rect.y + rect.height / 2.0);
        let dx = |rig: &Rig| rig.state().table_x.get(&1).copied().unwrap_or(0.0);
        // A plain vertical wheel is not taken, and moves nothing
        assert!(!rig.wheel(on, mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }));
        assert!(!rig.wheel(on, mouse::ScrollDelta::Pixels { x: 0.0, y: -40.0 }));
        assert_eq!(dx(&rig), 0.0);
        // A sideways wheel (lines are three of the text's size) and touchpad (pixels)
        assert!(rig.wheel(on, mouse::ScrollDelta::Lines { x: -1.0, y: 0.0 }));
        assert_eq!(dx(&rig), 16.0 * 3.0);
        assert!(rig.wheel(on, mouse::ScrollDelta::Pixels { x: -30.0, y: 2.0 }));
        assert_eq!(dx(&rig), 16.0 * 3.0 + 30.0);
        // The other way, and not past the left end
        assert!(rig.wheel(on, mouse::ScrollDelta::Lines { x: 1.0, y: 0.0 }));
        assert_eq!(dx(&rig), 30.0);
        assert!(rig.wheel(on, mouse::ScrollDelta::Pixels { x: 500.0, y: 0.0 }));
        assert_eq!(dx(&rig), 0.0);
        // Shift with the vertical wheel
        rig.shift(true);
        assert!(rig.wheel(on, mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 }));
        assert_eq!(dx(&rig), 96.0);
        rig.shift(false);
        assert!(
            !rig.wheel(on, mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 }),
            "Shift released: the page's again"
        );
        assert_eq!(dx(&rig), 96.0);
        // Not past the right end
        assert!(rig.wheel(on, mouse::ScrollDelta::Pixels { x: -5000.0, y: 0.0 }));
        assert_eq!(dx(&rig), most);
        assert!(rig.wheel(on, mouse::ScrollDelta::Pixels { x: -50.0, y: 0.0 }));
        assert_eq!(dx(&rig), most);
        // Over the bar under the table counts
        let bar = Point::new(100.0, rect.y + rect.height + TABLE_BAR * 1.5);
        assert!(rig.wheel(bar, mouse::ScrollDelta::Pixels { x: 30.0, y: 0.0 }));
        assert_eq!(dx(&rig), most - 30.0);
        // Beside the column's edge, and in the text below, it does not
        let beside = Point::new(view + 20.0, rect.y + rect.height / 2.0);
        assert!(!rig.wheel(beside, mouse::ScrollDelta::Pixels { x: 30.0, y: 0.0 }));
        let below = Point::new(100.0, rect.y + rect.height + TABLE_BAR * 4.0);
        assert!(!rig.wheel(below, mouse::ScrollDelta::Pixels { x: 30.0, y: 0.0 }));
        assert_eq!(dx(&rig), most - 30.0);
    }

    /// Where a click lands is through the table's scroll: the cell and the
    /// link under the pointer are the ones scrolled to, not the ones that
    /// were there before it scrolled
    #[test]
    fn clicks_and_links_in_a_wide_table_go_through_its_scroll() {
        let mut rig = Rig::new(WIDE, 400.0);
        let (rect, view) = rig.wide().unwrap();
        let cell = {
            let Laid::Cells(cells) = &rig.state().blocks[1].laid else {
                panic!("a table's cells")
            };
            cells
                .iter()
                .find(|c| c.cell.text.contains("the oldest"))
                .expect("the link's cell")
                .rect()
        };
        assert!(
            cell.x > view,
            "the link is in a column beyond the part shown: {cell:?} against {view}"
        );
        let url = "https://example.org/oldest";
        // Scrolled to the end, the cell is on screen where it was moved to:
        // the point there is over its link, the point it had before is not
        rig.wheel(
            Point::new(100.0, rect.y + 5.0),
            mouse::ScrollDelta::Pixels {
                x: -10000.0,
                y: 0.0,
            },
        );
        let dx = rig.state().table_dx(1);
        assert_eq!(dx, rect.width - view);
        let shown = Point::new(cell.center_x() - dx, cell.center_y());
        assert!(shown.x >= 0.0 && shown.x < view, "on screen: {shown:?}");
        assert_eq!(rig.state().link_at(shown), Some(url));
        let hit = rig.state().hit(shown);
        assert_eq!(hit.block, 1);
        // The offset is in that cell's text: the same as when the table is
        // laid out unscrolled and the point is where the cell is
        let still = Rig::new(WIDE, 4000.0);
        assert!(still.wide().is_none());
        let Laid::Cells(cells) = &still.state().blocks[1].laid else {
            panic!()
        };
        let same = cells
            .iter()
            .find(|c| c.cell.text.contains("the oldest"))
            .unwrap()
            .rect();
        assert_eq!(
            still
                .state()
                .hit(Point::new(same.center_x(), same.center_y())),
            hit
        );
        // Unscrolled, the point of the scrolled cell is over some other cell
        let rig = Rig::new(WIDE, 400.0);
        assert_ne!(rig.state().link_at(shown), Some(url));
    }
}
