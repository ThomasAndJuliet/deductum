// SPDX-License-Identifier: GPL-3.0-only

//! An open theory: the app's replica of the host's buffer (design C11) and the
//! two views onto it (C14). The human's keystrokes apply here at once; one
//! edit at a time goes to the host, as the span between the confirmed text and
//! the local one. Edits arriving from others are applied to the confirmed text
//! and the local difference rebased over them without dropping anything.
//! The source view and each block editor in the literate view edit symbol-
//! decoded text; their edits are mapped back onto the exact source characters.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use cosmic::iced::Color;
use cosmic::iced::advanced::text::highlighter;
use cosmic::widget::text_editor::{self, Action, Cursor, Position};

use super::ot::{self, Change};
use super::codec::{Decoded, Symbols};
use super::prover::Profile;
#[cfg(test)]
use super::prover::isabelle::symbols::Table;
use super::theory::{self, Block, Command, Status};
use prose_render::markdown::{self, Affinity, Doc, SourceMap};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProverMessage {
    pub start: usize,
    pub end: usize,
    pub kind: String,
    pub text: String,
    /// The message in the prover's own encoding, which its markup `spans` count code points of
    /// (empty when the prover sent none)
    pub raw: String,
    /// The prover's markup colours over `raw`: (start, end, class)
    pub spans: Vec<(usize, usize, Sem)>,
}

/// What the prover last said about the theory, in confirmed-text offsets
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prover {
    pub commands: Vec<Command>,
    pub messages: Vec<ProverMessage>,
    pub status: serde_json::Value,
    /// The prover's markup colours: (start, end, class)
    pub colours: Vec<(usize, usize, Sem)>,
}

impl Prover {
    pub fn from_json(v: &serde_json::Value) -> Self {
        let commands = v["commands"]
            .as_array()
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| {
                        Some(Command {
                            start: c[0].as_u64()? as usize,
                            end: c[1].as_u64()? as usize,
                            name: c[2].as_str()?.to_string(),
                            status: Status::parse(c[3].as_str().unwrap_or("")),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let messages = v["messages"]
            .as_array()
            .map(|ms| {
                ms.iter()
                    .filter_map(|m| {
                        Some(ProverMessage {
                            start: m["start"].as_u64()? as usize,
                            end: m["end"].as_u64()? as usize,
                            kind: m["kind"].as_str()?.to_string(),
                            text: m["text"].as_str().unwrap_or("").to_string(),
                            raw: m["raw"].as_str().unwrap_or("").to_string(),
                            spans: m["spans"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|c| Some((c[0].as_u64()? as usize, c[1].as_u64()? as usize, Sem::parse(c[2].as_str()?)?)))
                                .collect(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let colours = v["colours"]
            .as_array()
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| Some((c[0].as_u64()? as usize, c[1].as_u64()? as usize, Sem::parse(c[2].as_str()?)?)))
                    .collect()
            })
            .unwrap_or_default();
        Self { commands, messages, status: v["status"].clone(), colours }
    }

    fn mapped(&self, change: &Change) -> Self {
        Self {
            commands: self
                .commands
                .iter()
                .map(|c| Command {
                    start: ot::map_pos(c.start, change, false),
                    end: ot::map_pos(c.end, change, true),
                    ..c.clone()
                })
                .collect(),
            messages: self
                .messages
                .iter()
                .map(|m| ProverMessage { start: ot::map_pos(m.start, change, false), end: ot::map_pos(m.end, change, true), ..m.clone() })
                .collect(),
            status: self.status.clone(),
            colours: self
                .colours
                .iter()
                .map(|(s, e, k)| (ot::map_pos(*s, change, false), ot::map_pos(*e, change, true), *k))
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Literate,
    Source,
}

/// A block of the literate view being edited in place
pub struct BlockEditor {
    /// Changes whenever the editor's shaping/layout is discarded.
    pub layout_revision: u64,
    pub start: usize,
    pub end: usize,
    pub decoded: Decoded,
    pub content: text_editor::Content,
    /// For prose, the block as the literate view sets it, kept current while editing
    pub preview: Option<Arc<Doc>>,
    pub kind: theory::Kind,
    /// For prose, only the text is edited (as it reads): the command and delimiters around it
    pub wrap: Option<(String, String)>,
    /// The text exactly as the editor holds it (see [`shown_text_at`])
    pub shown_text: String,
}

impl BlockEditor {
    /// The text as the editor shows it (see [`shown_text`])
    pub fn shown(&self) -> String {
        self.shown_text.clone()
    }

    /// The decoded offset of the editor's caret
    pub fn caret(&self) -> usize {
        LineIndex::new(&self.shown_text).offset(&self.shown_text, self.content.cursor().position)
    }

    /// The text as shown for a caret at `caret` (decoded); if it changed, the editor gets it, the
    /// caret where it was. True if it changed
    pub fn reshow(&mut self, profile: &dyn Profile, caret: Option<usize>, table: &dyn Symbols) -> bool {
        let shown = shown_with_markup(profile, &self.decoded, self.wrap.is_some(), caret, table);
        if shown == self.shown_text {
            return false;
        }
        // Positions are byte columns in the displayed text. Hiding markup replaces a
        // one-byte delimiter with a three-byte zero-width character, so preserve both
        // endpoints as character offsets before rebuilding the editor.
        let cursor = self.content.cursor();
        let old = LineIndex::new(&self.shown_text);
        let position = old.offset(&self.shown_text, cursor.position);
        let selection = cursor.selection.map(|anchor| old.offset(&self.shown_text, anchor));
        let new = LineIndex::new(&shown);
        let cursor = Cursor {
            position: new.position(&shown, position),
            selection: selection.map(|anchor| new.position(&shown, anchor)),
        };
        self.content = text_editor::Content::with_text(&shown);
        self.layout_revision = self.layout_revision.wrapping_add(1);
        self.shown_text = shown;
        self.content.move_to(cursor);
        if cursor.selection.is_none() {
            self.content.perform(Action::ClearSelection);
        }
        true
    }
}

pub const ZERO_WIDTH: char = '\u{200B}';

/// Markup in a paragraph of prose, rendered where the caret is not inside it: the characters it
/// hides (its delimiters, shown zero-width) and how what it holds is set (the profile finds it)
pub struct Markup {
    pub hide: Vec<usize>,
    pub style: (usize, usize, Hl),
}

/// The text the editor shows: for prose, maths and markup rendered where the caret is not in them
/// (the profile's [`Profile::shown_prose`] and [`Profile::prose_markup`])
pub fn shown_with_markup(profile: &dyn Profile, decoded: &Decoded, prose: bool, caret: Option<usize>, table: &dyn Symbols) -> String {
    if !prose {
        return shown_text(&decoded.text, false);
    }
    let shown = profile.shown_prose(&decoded.text, caret, table);
    let mut chars: Vec<char> = shown.chars().collect();
    for m in profile.prose_markup(decoded, caret) {
        for k in m.hide {
            if let Some(c) = chars.get_mut(k) {
                *c = ZERO_WIDTH;
            }
        }
    }
    chars.into_iter().collect()
}

/// Prose as it reads while edited: a single line break is shown as a space (the editor wraps the
/// paragraph as the page does) and the indentation after it as nothing (zero-width spaces), one
/// character for one, so that offsets are the source's; blank lines stay
pub fn shown_text(text: &str, prose: bool) -> String {
    if !prose {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let blankish = |c: &&char| **c == ' ' || **c == '\t';
    let mut out = String::with_capacity(text.len());
    let mut indent = false;
    for (i, &c) in chars.iter().enumerate() {
        if c == '\n' {
            let next = chars[i + 1..].iter().find(|c| !blankish(c));
            let prev = chars[..i].iter().rev().find(|c| !blankish(c));
            let hard = matches!(next, None | Some('\n')) || matches!(prev, None | Some('\n'));
            if !hard {
                out.push(' ');
                indent = true;
                continue;
            }
        } else if indent && (c == ' ' || c == '\t') {
            out.push('\u{200B}');
            continue;
        }
        indent = false;
        out.push(c);
    }
    out
}

/// The rendered prose of a block's source, if it is prose
/// The heading level the rendered view sets a heading in: Markdown headings stop at four. The
/// prose editor uses the same level, so that opening a heading changes neither its size nor its
/// line height
pub(crate) fn rendered_heading_level(level: u8) -> u8 {
    level.min(4)
}

fn preview_of(profile: &dyn Profile, source: &str, kinds: &HashMap<String, String>, table: &dyn Symbols) -> (theory::Kind, Option<Arc<Doc>>) {
    let blocks = profile.blocks(source, &[], kinds);
    let Some(block) = blocks.first() else { return (theory::Kind::Formal, None) };
    let chars: Vec<char> = source.chars().collect();
    let md = match (block.kind, block.body) {
        (theory::Kind::Heading(level), Some((s, e))) if e <= chars.len() => Some(format!(
            "{} {}",
            "#".repeat(rendered_heading_level(level) as usize),
            profile.prose(&chars[s..e].iter().collect::<String>(), table).replace('\n', " ")
        )),
        (theory::Kind::Text, Some((s, e))) if e <= chars.len() => Some(profile.prose(&chars[s..e].iter().collect::<String>(), table)),
        _ => None,
    };
    (block.kind, md.map(|md| Arc::new(markdown::parse(&md))))
}

/// Lines of a text, for converting offsets and editor positions
pub struct LineIndex {
    /// Each line's start, in code points and bytes
    starts: Vec<(usize, usize)>,
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![(0, 0)];
        let mut cp = 0;
        for (b, c) in text.char_indices() {
            cp += 1;
            if c == '\n' {
                starts.push((cp, b + 1));
            }
        }
        Self { starts, len: cp }
    }

    /// The editor position (line, byte column) of code point `offset` in `text`
    pub fn position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(self.len);
        let line = match self.starts.binary_search_by_key(&offset, |s| s.0) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let (cp0, b0) = self.starts[line];
        let column = text[b0..].chars().take(offset - cp0).map(char::len_utf8).sum();
        Position { line, column }
    }

    /// The code point offset of an editor position
    pub fn offset(&self, text: &str, pos: Position) -> usize {
        let Some(&(cp0, b0)) = self.starts.get(pos.line) else { return self.len };
        let end = self.starts.get(pos.line + 1).map_or(text.len(), |s| s.1);
        let col = pos.column.min(end - b0);
        // (a column inside a character, from a position in an older text, is the character's start)
        let mut e = (b0 + col).min(text.len());
        while !text.is_char_boundary(e) {
            e -= 1;
        }
        cp0 + text[b0..e].chars().count()
    }
}

/// The classes of the prover's markup colours: Isabelle's Rendering.Color names, which the backend
/// protocol uses for every backend (a driver maps its own semantic classes onto them)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sem {
    Keyword1,
    Keyword2,
    Keyword3,
    QuasiKeyword,
    Improper,
    Operator,
    Free,
    Bound,
    Skolem,
    Var,
    TFree,
    TVar,
    Inner,
    Dynamic,
    ClassParameter,
    Antiquote,
    Comment,
    Raw,
}

impl Sem {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "keyword1" => Sem::Keyword1,
            "keyword2" => Sem::Keyword2,
            "keyword3" => Sem::Keyword3,
            "quasi_keyword" => Sem::QuasiKeyword,
            "improper" => Sem::Improper,
            "operator" => Sem::Operator,
            "free" => Sem::Free,
            "bound" => Sem::Bound,
            "skolem" => Sem::Skolem,
            "var" => Sem::Var,
            "tfree" => Sem::TFree,
            "tvar" => Sem::TVar,
            "inner_quoted" | "inner_cartouche" => Sem::Inner,
            "dynamic" => Sem::Dynamic,
            "class_parameter" => Sem::ClassParameter,
            "antiquote" | "antiquoted" => Sem::Antiquote,
            "comment1" | "comment2" | "comment3" => Sem::Comment,
            "raw_text" | "plain_text" => Sem::Raw,
            _ => return None,
        })
    }
}

/// Source-view highlighting: one class per code point of the decoded text
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hl {
    Sem(Sem),
    /// A sub- or superscript glyph, drawn in the scripts font, in its class's colour
    Script(Option<Sem>),
    Bold,
    /// Emphasis in prose (`\<^emph>‹…›` set as it reads)
    Emph,
    Control,
    Keyword,
    Error,
    Warning,
    Unprocessed,
    Running,
    Comment,
}

#[derive(Debug)]
pub struct HighlightData {
    pub generation: u64,
    /// Per line: byte ranges and their class
    pub lines: Vec<Vec<(Range<usize>, Hl)>>,
}

/// A highlighting, and whether the theme was dark when an editor was given it: the colours an
/// editor sets are kept with its lines until its highlighting changes, so a theme change must
/// change it
#[derive(Clone, Debug)]
pub struct HighlightSettings(pub Arc<HighlightData>, pub bool);

impl HighlightSettings {
    /// For an editor in the current theme
    pub fn themed(&self) -> Self {
        HighlightSettings(self.0.clone(), cosmic::theme::is_dark())
    }
}

impl PartialEq for HighlightSettings {
    fn eq(&self, other: &Self) -> bool {
        self.0.generation == other.0.generation && self.1 == other.1
    }
}

pub struct Highlighter {
    data: Arc<HighlightData>,
    line: usize,
}

impl highlighter::Highlighter for Highlighter {
    type Settings = HighlightSettings;
    type Highlight = Hl;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Hl)>;

    fn new(settings: &Self::Settings) -> Self {
        Self { data: settings.0.clone(), line: 0 }
    }

    fn update(&mut self, settings: &Self::Settings) {
        self.data = settings.0.clone();
        self.line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.line = self.line.min(line);
    }

    fn highlight_line(&mut self, _line: &str) -> Self::Iterator<'_> {
        let spans = self.data.lines.get(self.line).cloned().unwrap_or_default();
        self.line += 1;
        spans.into_iter()
    }

    fn current_line(&self) -> usize {
        self.line
    }
}

/// The colours of Isabelle's token classes, from the household's code palette
/// (`household_highlight`, rule 25b; Q3(a), Juliet 8 Nov) with Isabelle's classes mapped onto
/// its own: keywords (and quasi-keywords and antiquotations) are Keyword; free variables and
/// names Function; bound and inner syntax String; fixed (skolem) and schematic variables
/// Constant; type variables, dynamic facts and class parameters Type; comments the muted text,
/// darker on a light theme. Improper input is Tag, the palette's red. Every colour is the
/// palette's, as it is for every household app: none is Deductum's own
struct Scheme {
    keyword: Color,
    name: Color,
    kind: Color,
    string: Color,
    constant: Color,
    comment: Color,
    error: Color,
}

fn scheme(theme: &cosmic::Theme) -> Scheme {
    use household_highlight::{Class, Colour, colour};
    let dark = theme.cosmic().is_dark;
    let of = |class: Class| match colour(class) {
        Colour::Fixed { light, dark: on_dark } => {
            let hex = if dark { on_dark } else { light };
            Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
        }
        _ => super::tones::code_muted(theme),
    };
    Scheme {
        keyword: of(Class::Keyword),
        name: of(Class::Function),
        kind: of(Class::Type),
        string: of(Class::String),
        constant: of(Class::Constant),
        comment: super::tones::code_muted(theme),
        error: of(Class::Tag),
    }
}

/// How a class is set
pub fn highlight_format(hl: &Hl, theme: &cosmic::Theme) -> highlighter::Format<cosmic::iced::Font> {
    let cosmic = theme.cosmic();
    let text: Color = cosmic.on_bg_color().into();
    let k = scheme(theme);
    if let Hl::Script(sem) = hl {
        let base = sem.map_or(highlighter::Format { color: Some(text), font: None }, |s| highlight_format(&Hl::Sem(s), theme));
        return highlighter::Format { color: base.color, font: Some(cosmic::iced::Font::with_name("Isabellum Scripts")) };
    }
    let (color, weight, italic) = match hl {
        Hl::Sem(sem) => match sem {
            Sem::Keyword1 => (k.keyword, true, false),
            Sem::Keyword2 => (k.keyword, false, false),
            Sem::Keyword3 => (k.keyword, false, true),
            Sem::QuasiKeyword => (Color { a: 0.85, ..k.keyword }, false, false),
            Sem::Improper => (k.error, true, false),
            // (proof method names, as Isabelle marks them: in the text's colour, as jEdit sets them)
            Sem::Operator => (text, false, false),
            Sem::Free => (k.name, false, false),
            Sem::Bound => (k.string, false, false),
            Sem::Skolem => (k.constant, false, false),
            Sem::Var => (k.constant, false, true),
            Sem::TFree => (k.kind, false, false),
            Sem::TVar => (k.kind, false, true),
            Sem::Inner => (k.string, false, false),
            Sem::Dynamic => (k.kind, false, true),
            Sem::ClassParameter => (k.kind, false, false),
            Sem::Antiquote => (k.keyword, true, false),
            Sem::Comment => (k.comment, false, true),
            Sem::Raw => (text, false, false),
        },
        Hl::Bold => (text, true, false),
        Hl::Emph => (text, false, true),
        Hl::Script(_) => (text, false, false),
        Hl::Control => (Color { a: 0.45, ..text }, false, false),
        Hl::Keyword => (k.keyword, true, false),
        Hl::Error => (Color::from(cosmic.destructive_text_color()), false, false),
        Hl::Warning => (Color::from(cosmic.warning_text_color()), false, false),
        Hl::Unprocessed => (Color { a: 0.45, ..text }, false, false),
        Hl::Running => (Color { a: 0.75, ..Color::from(cosmic.accent_text_color()) }, false, false),
        Hl::Comment => (k.comment, false, true),
    };
    let font = (weight || italic).then(|| cosmic::iced::Font {
        weight: if weight { cosmic::iced::font::Weight::Bold } else { cosmic::iced::font::Weight::Normal },
        style: if italic { cosmic::iced::font::Style::Italic } else { cosmic::iced::font::Style::Normal },
        ..cosmic::font::mono()
    });
    highlighter::Format { color: Some(color), font }
}

/// Per-line byte ranges of a text's classes, for a highlighter
pub fn line_spans(text: &str, class: &[Option<Hl>]) -> Vec<Vec<(Range<usize>, Hl)>> {
    let mut lines: Vec<Vec<(Range<usize>, Hl)>> = vec![Vec::new()];
    let mut byte = 0;
    let mut current: Option<(usize, Hl)> = None;
    for (i, c) in text.chars().enumerate() {
        let hl = class.get(i).copied().flatten();
        if c == '\n' {
            if let Some((s, h)) = current.take() {
                lines.last_mut().unwrap().push((s..byte, h));
            }
            lines.push(Vec::new());
            byte = 0;
            continue;
        }
        match (current, hl) {
            (Some((_, h)), Some(x)) if h == x => {}
            (Some((s, h)), _) => {
                lines.last_mut().unwrap().push((s..byte, h));
                current = hl.map(|x| (byte, x));
            }
            (None, Some(x)) => current = Some((byte, x)),
            (None, None) => {}
        }
        byte += c.len_utf8();
    }
    if let Some((s, h)) = current {
        lines.last_mut().unwrap().push((s..byte, h));
    }
    lines
}

static VIEW_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);

/// A highlighting's number, unique among all theories and views: an editor keeps its highlighter
/// while the number is the same, so a number shared by two theories (a window moved from one to
/// another, the main window opening another theory) would keep the old one's colours on the new text
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn next_generation() -> u64 {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Prover output shown as code: highlighted with its markup, selectable and copyable
pub struct CodeView {
    pub content: text_editor::Content,
    pub highlight: HighlightSettings,
    /// What hovering a stretch of it says (a term's type, what a name is), by shown characters
    pub tips: Vec<(usize, usize, String)>,
}

impl CodeView {
    pub fn from_message(m: &serde_json::Value, table: &dyn Symbols) -> Self {
        let raw = m["raw"].as_str().or_else(|| m["text"].as_str()).unwrap_or("");
        let d = table.decode(raw.trim_end());
        let n = d.src_of.len() - 1;
        let mut class: Vec<Option<Hl>> = vec![None; n];
        for sp in m["spans"].as_array().into_iter().flatten() {
            let (Some(s), Some(e), Some(sem)) = (sp[0].as_u64(), sp[1].as_u64(), sp[2].as_str().and_then(Sem::parse)) else { continue };
            let (ds, de) = (d.to_decoded(s as usize), d.to_decoded(e as usize));
            for c in class.iter_mut().take(de.min(n)).skip(ds) {
                *c = Some(Hl::Sem(sem));
            }
        }
        for (i, style) in &d.styles {
            if let Some(c) = class.get_mut(*i) {
                let sem = match *c {
                    Some(Hl::Sem(s)) => Some(s),
                    _ => None,
                };
                *c = Some(match style {
                    super::codec::Style::Bold => Hl::Bold,
                    super::codec::Style::Control => Hl::Control,
                    super::codec::Style::Script => Hl::Script(sem),
                });
            }
        }
        let generation = VIEW_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tips = m["tips"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((d.to_decoded(t[0].as_u64()? as usize), d.to_decoded(t[1].as_u64()? as usize), t[2].as_str()?.to_string())))
            .collect();
        Self {
            content: text_editor::Content::with_text(&d.text),
            highlight: HighlightSettings(Arc::new(HighlightData { generation, lines: line_spans(&d.text, &class) }), false),
            tips,
        }
    }
}

/// A formal block as the literate view shows it: read-only text in an editor, so that a click
/// puts the caret where it lands and text can be selected, coloured like the source
pub struct BlockView {
    pub content: text_editor::Content,
    pub highlight: HighlightSettings,
    /// Characters in the longest line, for a block as wide as its text
    pub columns: usize,
    text_hash: u64,
}

#[derive(Clone)]
pub struct InflightEdit {
    pub identity: String,
    pub revision: u64,
    pub change: Change,
    pub txn: String,
    pub request: u64,
    pub supported: bool,
}

pub struct Theory {
    pub path: String,
    pub confirmed: String,
    pub confirmed_rev: u64,
    pub local: String,
    /// The request id of the edit the host hasn't confirmed yet
    pub inflight: Option<u64>,
    /// An edit was sent and may still need reconciliation after disconnect.
    pub inflight_unknown: bool,
    pub inflight_edit: Option<InflightEdit>,
    pub recovery_revision: u64,
    pub durability_pending: Vec<String>,
    pub durability_error: bool,
    /// Where the earliest change the prover has not seen yet is, in the confirmed text: from there
    /// on Isabelle checks again, so the text there shows as waiting until it reports on it
    pub stale_from: Option<usize>,
    pub txn: String,
    pub last_edit: Instant,
    /// Undo steps that ended before the one being typed, not yet sent: each goes to the host
    /// as its own edit, so that Ctrl+Z takes back one step (its transaction, the text then)
    pub steps: std::collections::VecDeque<(String, String)>,
    /// What the last edit was, and where it left the caret, to tell where a step ends
    last_kind: EditKind,
    last_caret: usize,
    /// Start a new step at the next edit
    force_step: bool,
    /// While set, edits join the step under way (a palette click's several insertions are one)
    group: bool,
    /// Vim keys: a whole insert session is one step, as in vim
    pub vim_session: bool,
    /// Undo and redo wait until the edits before them have reached the host
    pub queued: std::collections::VecDeque<&'static str>,
    pub decoded: Decoded,
    pub lines: LineIndex,
    pub source: text_editor::Content,
    pub prover: Prover,
    /// The report mapped to local text by refresh, shared by every view query.
    /// Source/prover mutations must refresh before exposing the theory again.
    local_prover: Arc<Prover>,
    /// The text delta is unchanged by prover progress, even with typing pending.
    local_change: Change,
    local_unchecked: Option<usize>,
    pub blocks: Vec<Block>,
    pub docs: Vec<Option<Arc<Doc>>>,
    pub page_rows: Arc<Vec<super::viewport::Row>>,
    pub page_scope: u64,
    /// The messages (by where they start) shown whole in their cell; the page's own state, never saved
    pub open_messages: std::collections::HashSet<usize>,
    pub page_viewport: std::cell::Cell<(f32, f32)>,
    /// Each prose block's Markdown offsets mapped to the source, where the profile can say
    pub prose_maps: Vec<Option<crate::prover::ProseMap>>,
    rendered_sources: Vec<Option<Arc<SourceMap>>>,
    doc_cache: HashMap<String, (Arc<Doc>, Arc<SourceMap>)>,
    pub block_editor: Option<BlockEditor>,
    /// Desired visual x during a run of vertical moves, including across cells.
    pub vertical_x: Option<f32>,
    /// Place the caret on the new cell's edge row after its first layout.
    pub vertical_entry: Option<bool>,
    pub mode: Mode,
    /// Source offset of the caret, for the proof state
    pub cursor: usize,
    pub highlight: HighlightSettings,
    generation: u64,
    pub dirty: bool,
    pub conflict: Option<serde_json::Value>,
    pub alternatives: Vec<serde_json::Value>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub save_error: Option<String>,
    pub kinds: Arc<HashMap<String, String>>,
    /// What the session's backend is like: blocks, prose, colours, indentation
    pub profile: &'static dyn Profile,
    /// What the session's typed proof tools are typed as (`sledgehammer`): a proof waiting on
    /// one is unfinished, not failed
    pub typed_tools: Vec<String>,
    /// Highlight class of each code point of the decoded text
    pub classes: Vec<Option<Hl>>,
    /// Completions for the word at the caret of the editor in use
    pub completions: Vec<Completion>,
    /// Highlighting for the block editor, from the same classes
    pub block_highlight: HighlightSettings,
    /// The first subgoal at the caret, for structure-driven completion
    pub goal: Option<Goal>,
    /// The block vim's j and k select in the literate view
    pub selected: Option<usize>,
    /// The formal blocks' views, by block index (None for other kinds)
    pub block_views: Vec<Option<BlockView>>,
    /// The widest formal block's columns: every block is drawn this wide, so they line up
    pub code_columns: usize,
}

/// A completion: replace the `replace` code points before the caret with `text`
#[derive(Clone, Debug, PartialEq)]
pub struct Completion {
    /// Characters before the end of `text` that the caret goes to (a bracket pair's middle)
    pub back: usize,
    pub replace: usize,
    pub text: String,
    pub label: String,
}

/// The first subgoal of a proof state, taken apart for structure-driven completion
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Goal {
    pub vars: Vec<String>,
    pub premises: Vec<String>,
    pub conclusion: String,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// Completions for the decoded text before the caret
static LISTENERS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The names of the agents listening for queries now, as the human types them after `@`
pub fn set_listeners(names: Vec<String>) {
    if let Ok(mut l) = LISTENERS.lock() {
        *l = names;
    }
}

/// An agent's name as typed after `@`: lower case, words joined by `-`
pub fn at_name(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

pub fn complete(profile: &dyn Profile, before: &str, table: &dyn Symbols, kinds: &HashMap<String, String>, words: &[String], goal: Option<&Goal>) -> Vec<Completion> {
    let chars: Vec<char> = before.chars().collect();
    let n = chars.len();
    let mut out: Vec<Completion> = Vec::new();
    // @name: the listening agents
    let mut j = n;
    while j > 0 && (chars[j - 1].is_alphanumeric() || matches!(chars[j - 1], '-' | '_' | '.')) {
        j -= 1;
    }
    if j > 0 && chars[j - 1] == '@' && (j == 1 || chars[j - 2].is_whitespace()) {
        let prefix: String = chars[j..].iter().collect();
        for name in LISTENERS.lock().map(|l| l.clone()).unwrap_or_default().into_iter().filter(|nm| nm.starts_with(&prefix)) {
            out.push(Completion { back: 0, replace: n - j, text: name.clone(), label: format!("@{name}  listening agent") });
        }
        return out;
    }
    // \<name or \name: symbols by name
    let mut i = n;
    while i > 0 && profile.symbol_query_char(chars[i - 1]) {
        i -= 1;
    }
    let lt = profile.angle_escapes() && i >= 2 && chars[i - 1] == '<' && chars[i - 2] == '\\';
    let bs = i >= 1 && chars[i - 1] == '\\';
    if lt || bs {
        let prefix: String = chars[i..].iter().collect();
        let start = if lt { i - 2 } else { i - 1 };
        if prefix.is_empty() && bs {
            return out;
        }
        for h in table.search_text(&prefix, 10) {
            let (c, sym, how) = (&h.text, &h.name, &h.how);
            let label = if how.is_empty() { format!("{c}  {sym}") } else { format!("{c}  {sym}  {how}") };
            out.push(Completion { back: h.text.chars().count().saturating_sub(h.caret), replace: n - start, text: h.text.clone(), label });
        }
        return out;
    }
    // Abbreviations such as ==> or -->, ending at the caret
    let mut abbrevs: Vec<(usize, char, &str)> = Vec::new();
    for (c, a) in table.abbreviations() {
        let ac: Vec<char> = a.chars().collect();
        if ac.len() >= 2 && ac.iter().all(|c| !is_word_char(*c)) && n >= ac.len() && chars[n - ac.len()..] == ac[..] {
            abbrevs.push((ac.len(), c, a));
        }
    }
    abbrevs.sort_by(|a, b| b.0.cmp(&a.0));
    for (len, c, a) in abbrevs.into_iter().take(4) {
        if !out.iter().any(|o| o.text == c.to_string()) {
            out.push(Completion { back: 0, replace: len, text: c.to_string(), label: format!("{c}  {a}") });
        }
    }
    if !out.is_empty() {
        return out;
    }
    // A word: keywords, then names used in the theory
    let mut j = n;
    while j > 0 && is_word_char(chars[j - 1]) {
        j -= 1;
    }
    if n - j < 2 {
        return out;
    }
    let prefix: String = chars[j..].iter().collect();
    // Steps built from the goal first, when typing at the start of a line
    let line_start = chars[..j].iter().rev().take_while(|c| **c != '\n').all(|c| c.is_whitespace());
    if line_start && let Some(goal) = goal {
        out.extend(profile.goal_completions(&prefix, n - j, goal));
    }
    let mut kws: Vec<&String> = kinds.keys().filter(|k| k.starts_with(&prefix) && k.len() > prefix.len()).collect();
    kws.sort_by_key(|k| (k.len(), k.to_string()));
    for k in kws.into_iter().take(4) {
        out.push(Completion { back: 0, replace: n - j, text: k.clone(), label: k.clone() });
    }
    for w in words.iter().filter(|w| w.starts_with(&prefix) && w.len() > prefix.len()).take(8 - out.len().min(8)) {
        if !out.iter().any(|o| &o.text == w) {
            out.push(Completion { back: 0, replace: n - j, text: w.clone(), label: w.clone() });
        }
    }
    out
}

/// The kind of an edit, for the undo steps: typing a word character, a space or punctuation,
/// a line break, deleting one character, or anything else (a paste, a completion, a command)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    Word,
    Space,
    Newline,
    Delete,
    Other,
}

/// Put an editor's caret at a position, with nothing selected (`move_to` alone leaves an empty
/// selection there, which the next motion would only clear)
/// Whether the caret is on the editor's first row as laid out (`last`: its last row): what a
/// click at that row's far end (start) lands on says where the row ends (starts). False when
/// the editor has not been laid out
pub fn on_edge_row(content: &mut text_editor::Content, last: bool) -> bool {
    let saved = content.cursor();
    let pos = saved.position;
    let lines = content.line_count();
    let probe = if last { cosmic::iced::Point::new(0.0, 1.0e7) } else { cosmic::iced::Point::new(1.0e7, 2.0) };
    content.perform(Action::Click(probe));
    let edge = content.cursor().position;
    content.move_to(saved);
    if saved.selection.is_none() {
        content.perform(Action::ClearSelection);
    }
    // (not laid out: every click lands at the start)
    if edge.line == 0 && edge.column == 0 && !(lines == 1 && pos.column == 0) {
        return false;
    }
    if last { pos.line == edge.line && pos.column >= edge.column } else { pos.line == 0 && edge.line == 0 && pos.column <= edge.column }
}

/// Where the selection is (code point offsets in the editor's text, start before end): from the
/// cursor and its anchor, or, for a selection the cursor cannot say (a double or triple click
/// selects the word or line around a point), the selected text found around the caret
pub fn selection_range(content: &text_editor::Content) -> Option<(usize, usize)> {
    let selected = content.selection().filter(|s| !s.is_empty())?;
    let text = content.text();
    let li = LineIndex::new(&text);
    let cursor = content.cursor();
    let at = li.offset(&text, cursor.position);
    let n = selected.chars().count();
    if let Some(anchor) = cursor.selection.filter(|a| *a != cursor.position) {
        let a = li.offset(&text, anchor);
        let (s, e) = (a.min(at), a.max(at));
        if e - s == n {
            return Some((s, e));
        }
    }
    // (the occurrence of the selected text nearest the caret)
    let chars: Vec<char> = text.chars().collect();
    let want: Vec<char> = selected.chars().collect();
    (0..=chars.len().saturating_sub(n))
        .filter(|&i| chars[i..i + n] == want[..])
        .min_by_key(|&i| if i <= at && at <= i + n { 0 } else { i.abs_diff(at).min((i + n).abs_diff(at)) + 1 })
        .map(|i| (i, i + n))
}

pub fn place(content: &mut text_editor::Content, position: text_editor::Position) {
    content.move_to(Cursor { position, selection: None });
    content.perform(Action::ClearSelection);
}

/// Recover the visual caret position through the laid-out editor's hit testing.
/// Content exposes source positions but no caret geometry. Hit regions are centred
/// on caret stops; their midpoint also works with proportional fonts and symbols.
pub fn caret_point(content: &mut text_editor::Content) -> cosmic::iced::Point {
    use cosmic::iced::Point;
    let saved = content.cursor();
    let target = (saved.position.line, saved.position.column);
    let hit = |content: &mut text_editor::Content, x, y| {
        content.perform(Action::Click(Point::new(x, y)));
        let pos = content.cursor().position;
        (pos.line, pos.column)
    };
    let mut top = 0.0;
    let mut bottom = 16.0;
    while hit(content, 1.0e7, bottom) < target && bottom < 1.0e7 {
        top = bottom;
        bottom *= 2.0;
    }
    while bottom - top > 0.125 {
        let middle = (top + bottom) / 2.0;
        if hit(content, 1.0e7, middle) < target { top = middle } else { bottom = middle }
    }
    let y = bottom;
    let at_start = hit(content, 0.0, y) == target;
    let mut left = 0.0;
    let mut right = 16.0;
    while hit(content, right, y) < target && right < 1.0e7 { right *= 2.0; }
    while right - left > 0.125 {
        let middle = (left + right) / 2.0;
        if hit(content, middle, y) < target { left = middle } else { right = middle }
    }
    let start = right;
    // The line's end has an unbounded hit region. Use the preceding stop's
    // region to recover its half-width instead of averaging with infinity.
    let end = if hit(content, 1.0e7, y) <= target {
        let previous = hit(content, (start - 0.25).max(0.0), y);
        left = 0.0;
        right = start;
        while right - left > 0.125 {
            let middle = (left + right) / 2.0;
            if hit(content, middle, y) < previous { left = middle } else { right = middle }
        }
        start + (start - right)
    } else {
        left = start;
        right = start.max(16.0) * 2.0;
        while hit(content, right, y) <= target && right < 1.0e7 { right *= 2.0; }
        while right - left > 0.125 {
            let middle = (left + right) / 2.0;
            if hit(content, middle, y) <= target { left = middle } else { right = middle }
        }
        right
    };
    content.move_to(saved);
    if saved.selection.is_none() { content.perform(Action::ClearSelection); }
    Point::new(if at_start { 0.0 } else { (start + end) / 2.0 }, y)
}

fn new_txn() -> String {
    format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()))
}

impl Theory {
    /// A document read from the host, of a session whose backend's profile is `profile` (its
    /// typed tools are the profile's until the session's capabilities say otherwise)
    pub fn new(path: String, doc: &serde_json::Value, table: &dyn Symbols, kinds: Arc<HashMap<String, String>>, mode: Mode, profile: &'static dyn Profile) -> Self {
        let text = doc["text"].as_str().unwrap_or("").to_string();
        let decoded = table.decode(&text);
        let mut theory = Self {
            path,
            confirmed: text.clone(),
            confirmed_rev: doc["revision"].as_u64().unwrap_or(0),
            local: text,
            inflight: None,
            inflight_unknown: false,
            inflight_edit: None,
            recovery_revision: 0,
            durability_pending: Vec::new(),
            durability_error: false,
            txn: new_txn(),
            last_edit: Instant::now(),
            steps: Default::default(),
            last_kind: EditKind::Other,
            last_caret: 0,
            force_step: false,
            group: false,
            vim_session: false,
            queued: Default::default(),
            lines: LineIndex::new(&decoded.text),
            source: text_editor::Content::with_text(&decoded.text),
            decoded,
            prover: Prover::from_json(&doc["prover"]),
            local_prover: Arc::new(Prover::default()),
            local_change: Vec::new(),
            local_unchecked: None,
            stale_from: None,
            blocks: Vec::new(),
            docs: Vec::new(),
            page_rows: Arc::new(Vec::new()),
            page_scope: { static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1); NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) },
            open_messages: std::collections::HashSet::new(),
            page_viewport: std::cell::Cell::new((0.0, 800.0)),
            prose_maps: Vec::new(),
            rendered_sources: Vec::new(),
            doc_cache: HashMap::new(),
            block_editor: None,
            vertical_x: None,
            vertical_entry: None,
            mode,
            cursor: 0,
            highlight: HighlightSettings(Arc::new(HighlightData { generation: 0, lines: Vec::new() }), false),
            generation: 0,
            dirty: doc["dirty"].as_bool().unwrap_or(false),
            conflict: doc.get("conflict").filter(|c| !c.is_null()).cloned(),
            alternatives: doc["alternatives"].as_array().cloned().unwrap_or_default(),
            can_undo: doc["can_undo"].as_bool().unwrap_or(false),
            can_redo: doc["can_redo"].as_bool().unwrap_or(false),
            save_error: None,
            kinds,
            profile,
            typed_tools: profile.default_capabilities().typed_tools(),
            classes: Vec::new(),
            completions: Vec::new(),
            block_highlight: HighlightSettings(Arc::new(HighlightData { generation: 0, lines: Vec::new() }), false),
            block_views: Vec::new(),
            code_columns: 0,
            goal: None,
            selected: None,
        };
        theory.refresh(table);
        theory
    }

    /// Names used in the theory, most frequent first, for completion
    pub fn words(&self) -> Vec<String> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for w in self.decoded.text.split(|c: char| !is_word_char(c)) {
            if w.chars().count() >= 3 && !w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                *counts.entry(w).or_default() += 1;
            }
        }
        let mut words: Vec<(&str, usize)> = counts.into_iter().collect();
        words.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        words.into_iter().take(4000).map(|(w, _)| w.to_string()).collect()
    }

    /// The text of the current line before the caret, in the editor in use
    pub fn line_before_caret(&self) -> String {
        let (text, pos) = match &self.block_editor {
            Some(be) if self.mode == Mode::Literate => (be.content.text(), be.content.cursor().position),
            _ => (self.decoded.text.clone(), self.source.cursor().position),
        };
        let line = text.split('\n').nth(pos.line).unwrap_or("");
        let mut end = pos.column.min(line.len());
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line[..end].to_string()
    }

    /// Recompute the completions at the caret of the editor in use
    pub fn update_completions(&mut self, table: &dyn Symbols, edited: bool) {
        if !edited {
            self.completions.clear();
            return;
        }
        let (text, pos) = match &self.block_editor {
            Some(be) if self.mode == Mode::Literate => (be.content.text(), be.content.cursor().position),
            _ => (self.decoded.text.clone(), self.source.cursor().position),
        };
        let li = LineIndex::new(&text);
        let caret = li.offset(&text, pos);
        let before: String = text.chars().take(caret).collect();
        let words = self.words();
        self.completions = complete(self.profile, &before, table, &self.kinds, &words, self.goal.as_ref());
        // Without completion while typing (a setting), only what is typed to be completed: `\`
        // symbols and `@names`, which have no other way in
        if !crate::settings::complete_while_typing() {
            let chars: Vec<char> = before.chars().collect();
            // (a symbol's replacement starts at its `\`, a name's just after its `@`)
            self.completions.retain(|c| {
                let Some(i) = chars.len().checked_sub(c.replace) else { return false };
                chars.get(i) == Some(&'\\') || (i > 0 && chars[i - 1] == '@')
            });
        }
    }

    /// Accept completion `i` in the editor in use
    pub fn accept_completion(&mut self, i: usize, table: &dyn Symbols) {
        let Some(c) = self.completions.get(i).cloned() else { return };
        self.completions.clear();
        let in_block = self.block_editor.is_some() && self.mode == Mode::Literate;
        let (text, pos) = match &self.block_editor {
            Some(be) if in_block => (be.content.text(), be.content.cursor().position),
            _ => (self.decoded.text.clone(), self.source.cursor().position),
        };
        let li = LineIndex::new(&text);
        let caret = li.offset(&text, pos);
        let chars: Vec<char> = text.chars().collect();
        let start = caret.saturating_sub(c.replace);
        let new: String = chars[..start].iter().chain(c.text.chars().collect::<Vec<_>>().iter()).chain(chars[caret..].iter()).collect();
        let caret_after = (start + c.text.chars().count()).saturating_sub(c.back);
        if in_block {
            let be = self.block_editor.as_mut().unwrap();
            be.content = text_editor::Content::with_text(&new);
            be.layout_revision = be.layout_revision.wrapping_add(1);
            let li2 = LineIndex::new(&new);
            place(&mut be.content, li2.position(&new, caret_after));
            // The editor shows `new`; apply it as an edit of the block, against the text as shown
            // (where a space stands for a line break and zero-width spaces for its indent, those
            // stay in the source as they were)
            let rel: Change = ot::diff(&be.shown(), &new)
                .into_iter()
                .map(|(s, e, t)| (be.decoded.to_source(s), be.decoded.to_source(e), table.encode(&t.replace('\u{200B}', ""))))
                .collect();
            let start_src = be.start;
            let change: Change = rel.into_iter().map(|(s, e, t)| (s + start_src, e + start_src, t)).collect();
            let grown: isize = change.iter().map(|(s, e, t)| t.chars().count() as isize - (*e as isize - *s as isize)).sum();
            let end_before = be.end;
            self.apply_local(&change, table);
            if let Some(be) = &mut self.block_editor {
                be.end = (end_before as isize + grown).max(be.start as isize) as usize;
                let src = self.local.chars().skip(be.start).take(be.end - be.start).collect::<String>();
                be.decoded = table.decode(&src);
                // (what the editor shows now, for the next edit to be taken against)
                let shown = shown_with_markup(self.profile, &be.decoded, be.wrap.is_some(), Some(caret_after), table);
                if shown != new {
                    let li = LineIndex::new(&shown);
                    be.content = text_editor::Content::with_text(&shown);
                    be.layout_revision = be.layout_revision.wrapping_add(1);
                    place(&mut be.content, li.position(&shown, caret_after));
                }
                be.shown_text = shown;
            }
            self.decoded = table.decode(&self.local);
            self.lines = LineIndex::new(&self.decoded.text);
            self.source = text_editor::Content::with_text(&self.decoded.text);
        } else {
            let change = self.decoded.source_change(table, &new);
            self.apply_local(&change, table);
            self.decoded = table.decode(&self.local);
            self.lines = LineIndex::new(&self.decoded.text);
            self.source = text_editor::Content::with_text(&self.decoded.text);
            let pos = self.lines.position(&self.decoded.text, caret_after);
            place(&mut self.source, pos);
            self.cursor = self.decoded.to_source(caret_after);
        }
        self.refresh(table);
    }

    /// Highlighting for the block being edited, as per-line byte ranges of its decoded text
    pub fn compute_block_highlight(&mut self) {
        let Some(be) = &self.block_editor else { return };
        let ds = self.decoded.to_decoded(be.start);
        let mut lines: Vec<Vec<(Range<usize>, Hl)>> = vec![Vec::new()];
        let mut byte = 0;
        let mut current: Option<(usize, Hl)> = None;
        // Prose being edited: the code in it coloured as code at once (antiquotations, maths
        // source, LaTeX commands), where the prover has not coloured it (yet)
        let shown = be.shown();
        let prose = be.wrap.as_ref().map(|_| self.profile.prose_code_classes(&shown));
        // (markup rendered where the caret is not in it: set bold, emphasised, or as code)
        let mut rendered: Vec<Option<Hl>> = vec![None; shown.chars().count()];
        if be.wrap.is_some() {
            for m in self.profile.prose_markup(&be.decoded, Some(be.caret())) {
                for c in rendered.iter_mut().take(m.style.1).skip(m.style.0) {
                    *c = Some(m.style.2);
                }
            }
        }
        let prose_class = |h: Option<Hl>| matches!(h, None | Some(Hl::Comment | Hl::Unprocessed | Hl::Running | Hl::Sem(Sem::Comment | Sem::Raw | Sem::Inner)));
        for (k, c) in shown.chars().enumerate() {
            let mut hl = self.classes.get(ds + k).copied().flatten();
            if let Some(code) = prose.as_ref().and_then(|p| p.get(k).copied().flatten())
                && prose_class(hl)
            {
                hl = Some(code);
            }
            if let Some(r) = rendered.get(k).copied().flatten() {
                hl = Some(r);
            }
            if c == '\n' {
                if let Some((s, h)) = current.take() {
                    lines.last_mut().unwrap().push((s..byte, h));
                }
                lines.push(Vec::new());
                byte = 0;
                continue;
            }
            match (current, hl) {
                (Some((_, h)), Some(x)) if h == x => {}
                (Some((s, h)), _) => {
                    lines.last_mut().unwrap().push((s..byte, h));
                    current = hl.map(|x| (byte, x));
                }
                (None, Some(x)) => current = Some((byte, x)),
                (None, None) => {}
            }
            byte += c.len_utf8();
        }
        if let Some((s, h)) = current {
            lines.last_mut().unwrap().push((s..byte, h));
        }
        if self.block_highlight.0.lines != lines {
            self.generation = next_generation();
            self.block_highlight = HighlightSettings(Arc::new(HighlightData { generation: self.generation, lines }), false);
        }
    }

    /// The source construct the caret is in, within `start..end` (a proof), as the profile finds
    /// it among the proof's commands. As source offsets; the goal goes level with or just after it
    pub fn construct_at(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        let prover = self.local_prover();
        let cmds: Vec<_> = prover.commands.iter().filter(|c| c.start >= start && c.end <= end.max(start + 1)).collect();
        let at = self.cursor.saturating_sub(1).max(start);
        self.profile.construct_at(&self.local, &cmds, at)
    }

    /// The status of the commands in a source span: worst first
    pub fn status_in(&self, start: usize, end: usize) -> Option<Status> {
        let prover = self.local_prover();
        prover.commands.iter().filter(|c| c.start < end && c.end > start).map(|c| c.status).max()
    }

    /// The decoded text of a source span, in runs of one highlight class
    pub fn highlighted(&self, start: usize, end: usize) -> Vec<(String, Option<Hl>)> {
        let ds = self.decoded.to_decoded(start);
        let de = self.decoded.to_decoded(end).max(ds);
        let mut out: Vec<(String, Option<Hl>)> = Vec::new();
        for (i, c) in self.decoded.text.chars().enumerate().skip(ds).take(de - ds) {
            let hl = self.classes.get(i).copied().flatten();
            match out.last_mut() {
                Some((text, h)) if *h == hl => text.push(c),
                _ => out.push((c.to_string(), hl)),
            }
        }
        out
    }

    /// The prover's data against the local text, retained until the next refresh.
    fn local_prover(&self) -> &Prover {
        &self.local_prover
    }

    fn map_prover(&mut self, prover_only: bool) {
        if !prover_only {
            self.local_change = ot::diff(&self.confirmed, &self.local);
        }
        let delta = &self.local_change;
        let typed = delta.first().map(|(s, _, _)| *s);
        let stale = self.stale_from.map(|s| ot::map_pos(s, delta, false));
        self.local_unchecked = typed.into_iter().chain(stale).min();
        let mut prover = if delta.is_empty() { self.prover.clone() } else { self.prover.mapped(delta) };
        // A keyword line (`claude[haiku] hint`) reaches the prover as a `sorry` padded to its
        // length, and the prover's command ends with the `sorry`: here it covers the line as typed
        // (else the rest of the keyword was text of its own after the cell, and the caret at the
        // line's end was in no command)
        let unfinished = self.profile.unfinished();
        if prover.commands.iter().any(|c| unfinished.contains(&c.name.as_str())) {
            let chars: Vec<char> = self.local.chars().collect();
            for c in prover.commands.iter_mut().filter(|c| unfinished.contains(&c.name.as_str()) && c.end <= chars.len() && c.start < c.end) {
                let typed: String = chars[c.start..c.end].iter().collect();
                if typed == c.name {
                    continue;
                }
                let line_start = chars[..c.start].iter().rposition(|&ch| ch == '\n').map_or(0, |p| p + 1);
                let line_end = chars[c.end..].iter().position(|&ch| ch == '\n').map_or(chars.len(), |p| c.end + p);
                let line: String = chars[line_start..line_end].iter().collect();
                if self.profile.keyword_line(&line) {
                    c.end = chars[..line_end].iter().rposition(|ch| !ch.is_whitespace()).map_or(c.end, |i| (i + 1).max(c.end));
                }
            }
        }
        // What the prover has not seen yet (typing not sent, or changes it has not reported on)
        // and everything after it is being checked again: waiting, not as it was
        if let Some(from) = self.local_unchecked {
            for c in prover.commands.iter_mut().filter(|c| c.end > from) {
                c.status = Status::Unprocessed;
            }
        }
        self.local_prover = Arc::new(prover);
    }

    /// Where, in the local text, the prover's report stops covering the text: the earliest change
    /// it has not seen
    pub fn unchecked_from(&self) -> Option<usize> {
        self.local_unchecked
    }

    /// The source offset standing for byte `md` of the Markdown that block `i` renders as, if the
    /// profile keeps the text (Lean's docstrings): where a click in the rendered paragraph, or a
    /// mark on it, is in the file
    pub fn source_of_markdown(&self, i: usize, md: usize) -> Option<usize> {
        let (start, _) = self.blocks.get(i)?.body?;
        Some(start + self.prose_maps.get(i)?.as_ref()?.source_char(md))
    }

    /// Compose rendered Markdown provenance with the profile's source mapping.
    /// Offsets reported by the renderer are UTF-8 bytes; the resulting prover
    /// position is a source character offset, including the body's prefix.
    pub fn source_of_hit(&self, i: usize, block: usize, offset: usize) -> Option<usize> {
        let markdown = self.rendered_sources.get(i)?.as_ref()?
            .source_byte(block, offset, Affinity::After)?;
        let map = self.prose_maps.get(i)?.as_ref()?;
        let (body_start, _) = self.blocks.get(i)?.body?;
        Some(body_start + map.source_char(markdown))
    }

    /// Open prose block `index` with the caret at source offset `at`, as the paragraph there
    pub fn open_prose_at(&mut self, index: usize, at: usize, table: &dyn Symbols) {
        self.open_block(index, table);
        let Some((block, body)) = self.prose_block() else { return };
        let at = at.clamp(body.0, body.1);
        let paras = self.paragraphs(body);
        let (ps, pe) = paras.iter().copied().find(|(s, e)| *s <= at && at <= *e).or_else(|| paras.iter().rev().copied().find(|(s, _)| *s <= at)).unwrap_or(body);
        self.open_range(&block, ps, pe, at, table);
    }

    /// Recompute blocks, rendered prose and highlighting after the text or the prover changed
    pub fn refresh(&mut self, table: &dyn Symbols) {
        self.refresh_views(table, false);
    }

    fn refresh_views(&mut self, table: &dyn Symbols, prover_only: bool) {
        let previous = self.local_prover.clone();
        self.map_prover(prover_only);
        let prover = self.local_prover.clone();
        let same_commands = prover_only && previous.commands == prover.commands;
        let blocks = (!same_commands).then(|| self.profile.blocks(&self.local, &prover.commands, &self.kinds));
        // The profiles own block splitting and status aggregation. Even when
        // command status changes, unchanged prose keeps its source maps and Doc.
        let keep_prose = prover_only && blocks.as_ref().is_none_or(|blocks| {
            blocks.len() == self.blocks.len() && blocks.iter().zip(&self.blocks).all(|(a, b)| {
                (a.kind, a.start, a.end, a.body) == (b.kind, b.start, b.end, b.body)
            })
        });
        if let Some(blocks) = blocks {
            self.blocks = blocks;
        }
        if !keep_prose {
            self.compute_prose(table);
        }
        let repaint = !same_commands || previous.messages != prover.messages || previous.colours != prover.colours;
        let colours_changed = repaint && self.compute_highlight(&prover);
        if !prover_only || !keep_prose || colours_changed {
            self.compute_block_views();
        }
    }

    fn compute_prose(&mut self, table: &dyn Symbols) {
        let chars: Vec<char> = self.local.chars().collect();
        use std::hash::{Hash, Hasher};
        // Repeated text has no unique source identity. Invalidate those rows
        // after a source refresh rather than moving retained state to a different
        // occurrence after a collaborative insertion. The active editor has its
        // own identity in the viewport widget.
        static NEXT_ROWS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let generation = NEXT_ROWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut counts = HashMap::<u64, usize>::new();
        let mut rows: Vec<_> = self.blocks.iter().map(|b| {
            let slice = chars.get(b.start..b.end).unwrap_or(&[]);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            slice.hash(&mut hasher);
            format!("{:?}", b.kind).hash(&mut hasher);
            let key = hasher.finish();
            *counts.entry(key).or_default() += 1;
            super::viewport::Row { key, characters: slice.len(), lines: 1 + slice.iter().filter(|c| **c == '\n').count(), kind: b.kind }
        }).collect();
        for (i, row) in rows.iter_mut().enumerate() {
            if counts[&row.key] > 1 {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                (row.key, generation, i).hash(&mut hasher);
                row.key = hasher.finish();
            }
        }
        self.page_rows = Arc::new(rows);
        let mut docs = Vec::with_capacity(self.blocks.len());
        let mut rendered_sources = Vec::with_capacity(self.blocks.len());
        let mut maps = Vec::with_capacity(self.blocks.len());
        let cache = std::mem::take(&mut self.doc_cache);
        let mut used = HashMap::new();
        for block in &self.blocks {
            maps.push(match (block.kind, block.body) {
                (theory::Kind::Text, Some((s, e))) if s <= e && e <= chars.len() => self.profile.prose_map(&chars[s..e].iter().collect::<String>()),
                _ => None,
            });
            let doc = match (block.kind, block.body) {
                (theory::Kind::Heading(level), Some((s, e))) | (theory::Kind::Heading(level), Some((s, e)))
                    if s <= e && e <= chars.len() =>
                {
                    let src: String = chars[s..e].iter().collect();
                    let md = format!("{} {}", "#".repeat(rendered_heading_level(level) as usize), self.profile.prose(&src, table).replace('\n', " "));
                    Some(md)
                }
                (theory::Kind::Text, Some((s, e))) if s <= e && e <= chars.len() => {
                    let src: String = chars[s..e].iter().collect();
                    Some(self.profile.prose(&src, table))
                }
                _ => None,
            }
            .map(|md| {
                let doc = cache.get(&md).or_else(|| used.get(&md)).cloned().unwrap_or_else(|| {
                    let parsed = markdown::parse_mapped(&md);
                    (Arc::new(parsed.doc), Arc::new(parsed.map))
                });
                used.insert(md, doc.clone());
                doc
            });
            rendered_sources.push(doc.as_ref().map(|(_, map)| map.clone()));
            docs.push(doc.map(|(doc, _)| doc));
        }
        self.doc_cache = used;
        self.docs = docs;
        self.rendered_sources = rendered_sources;
        self.prose_maps = maps;
    }

    /// The formal blocks' read-only editors: kept while their text is unchanged (a block's
    /// editor holds its laid-out text), with highlighting replaced only when it changed
    fn compute_block_views(&mut self) {
        use std::hash::{Hash, Hasher};
        let mut old: HashMap<u64, Vec<BlockView>> = HashMap::new();
        for v in std::mem::take(&mut self.block_views).into_iter().flatten() {
            old.entry(v.text_hash).or_default().push(v);
        }
        let dchars: Vec<char> = self.decoded.text.chars().collect();
        let mut views = Vec::with_capacity(self.blocks.len());
        for block in &self.blocks {
            if block.kind != theory::Kind::Formal {
                views.push(None);
                continue;
            }
            let ds = self.decoded.to_decoded(block.start).min(dchars.len());
            let mut de = self.decoded.to_decoded(block.end).clamp(ds, dchars.len());
            while de > ds && dchars[de - 1].is_whitespace() {
                de -= 1;
            }
            let text: String = dchars[ds..de].iter().collect();
            let class: Vec<Option<Hl>> = (ds..de).map(|i| self.classes.get(i).copied().flatten()).collect();
            let lines = line_spans(&text, &class);
            let mut hasher = std::hash::DefaultHasher::new();
            text.hash(&mut hasher);
            let text_hash = hasher.finish();
            let view = match old.get_mut(&text_hash).and_then(|vs| vs.pop()) {
                Some(mut v) => {
                    if v.highlight.0.lines != lines {
                        let generation = VIEW_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        v.highlight = HighlightSettings(Arc::new(HighlightData { generation, lines }), false);
                    }
                    v
                }
                None => {
                    let generation = VIEW_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    BlockView {
                        columns: text.lines().map(|l| l.chars().count()).max().unwrap_or(0),
                        content: text_editor::Content::with_text(&text),
                        highlight: HighlightSettings(Arc::new(HighlightData { generation, lines }), false),
                        text_hash,
                    }
                }
            };
            views.push(Some(view));
        }
        self.code_columns = views.iter().flatten().map(|v| v.columns).max().unwrap_or(0);
        self.block_views = views;
    }

    fn compute_highlight(&mut self, prover: &Prover) -> bool {
        let text = &self.decoded.text;
        let n = self.decoded.src_of.len() - 1;
        let mut class: Vec<Option<Hl>> = vec![None; n];
        let to_dec = |s: usize| self.decoded.to_decoded(s).min(n);
        // Higher classes win where they meet: errors over colours over status
        let prio = |h: Option<Hl>| match h {
            None => 0,
            Some(Hl::Unprocessed | Hl::Running) => 1,
            Some(Hl::Comment) => 2,
            Some(Hl::Sem(_)) => 3,
            Some(Hl::Keyword) => 4,
            Some(Hl::Bold | Hl::Emph | Hl::Control | Hl::Script(_)) => 5,
            Some(Hl::Warning) => 6,
            Some(Hl::Error) => 7,
        };
        let mut paint = |class: &mut Vec<Option<Hl>>, s: usize, e: usize, hl: Hl, _over: &[Hl]| {
            for c in &mut class[to_dec(s)..to_dec(e).max(to_dec(s))] {
                if prio(Some(hl)) >= prio(*c) {
                    *c = Some(hl);
                }
            }
        };
        // What the characters alone show, under the prover's colours
        for (s, e, hl) in self.profile.lexical_colours(&self.local) {
            paint(&mut class, s, e, hl, &[]);
        }
        for (s, e, sem) in prover.colours.iter().chain(&self.profile.derived_colours(&self.local, &prover.colours)) {
            paint(&mut class, *s, *e, Hl::Sem(*sem), &[]);
        }
        // A `claude` or `codex` line reads to the prover as a `sorry` of the same length: the
        // whole keyword (and its model) coloured as that `sorry` is
        for (s, e) in self.profile.keyword_tokens(&self.local) {
            let hl = prover.colours.iter().find(|(cs, ce, _)| *cs <= s && s < *ce).map_or(Hl::Keyword, |(_, _, sem)| Hl::Sem(*sem));
            paint(&mut class, s, e, hl, &[]);
        }
        for cmd in &prover.commands {
            match cmd.status {
                Status::Unprocessed => paint(&mut class, cmd.start, cmd.end, Hl::Unprocessed, &[]),
                Status::Running => paint(&mut class, cmd.start, cmd.end, Hl::Running, &[]),
                _ => {}
            }
            let kw_end = cmd.start + cmd.name.chars().count();
            if prover.colours.is_empty() && self.profile.commands_are_keywords() {
                paint(&mut class, cmd.start, kw_end, Hl::Keyword, &[Hl::Unprocessed, Hl::Running]);
            }
        }
        if prover.commands.is_empty() && self.profile.commands_are_keywords() {
            for cmd in self.profile.lexical_commands(&self.local, &self.kinds) {
                paint(&mut class, cmd.start, cmd.start + cmd.name.chars().count(), Hl::Keyword, &[]);
            }
        }
        for m in &prover.messages {
            let hl = match m.kind.as_str() {
                "error" => Hl::Error,
                "warning" | "legacy" => Hl::Warning,
                _ => continue,
            };
            paint(&mut class, m.start, m.end.max(m.start + 1), hl, &[Hl::Keyword, Hl::Unprocessed, Hl::Running, Hl::Warning]);
        }
        // Comments, lexically
        for (s, e) in self.profile.comments(&self.local) {
            paint(&mut class, s, e, Hl::Comment, &[Hl::Unprocessed]);
        }
        // Bold and control markers from the symbols
        for (i, style) in &self.decoded.styles {
            if let Some(c) = class.get_mut(*i) {
                let sem = match *c {
                    Some(Hl::Sem(s)) => Some(s),
                    _ => None,
                };
                match style {
                    super::codec::Style::Script => *c = Some(Hl::Script(sem)),
                    _ if matches!(c, Some(Hl::Error | Hl::Warning)) => {}
                    super::codec::Style::Bold => *c = Some(Hl::Bold),
                    super::codec::Style::Control => *c = Some(Hl::Control),
                }
            }
        }
        // Per line byte ranges
        let mut lines: Vec<Vec<(Range<usize>, Hl)>> = vec![Vec::new()];
        let mut byte_in_line = 0;
        let mut current: Option<(usize, Hl)> = None;
        for (i, c) in text.chars().enumerate() {
            let hl = class.get(i).copied().flatten();
            if c == '\n' {
                if let Some((s, h)) = current.take() {
                    lines.last_mut().unwrap().push((s..byte_in_line, h));
                }
                lines.push(Vec::new());
                byte_in_line = 0;
                continue;
            }
            match (current, hl) {
                (Some((s, h)), Some(x)) if h == x => {
                    let _ = s;
                }
                (Some((s, h)), _) => {
                    lines.last_mut().unwrap().push((s..byte_in_line, h));
                    current = hl.map(|x| (byte_in_line, x));
                }
                (None, Some(x)) => current = Some((byte_in_line, x)),
                (None, None) => {}
            }
            byte_in_line += c.len_utf8();
        }
        if let Some((s, h)) = current {
            lines.last_mut().unwrap().push((s..byte_in_line, h));
        }
        if self.highlight.0.lines != lines {
            self.generation = next_generation();
            self.highlight = HighlightSettings(Arc::new(HighlightData { generation: self.generation, lines }), false);
        }
        let changed = self.classes != class;
        self.classes = class;
        self.compute_block_highlight();
        changed
    }

    /// Messages of the local text in a source span
    pub fn messages_in(&self, start: usize, end: usize) -> Vec<ProverMessage> {
        let prover = self.local_prover();
        prover
            .messages
            .iter()
            .filter(|m| m.start < end.max(start + 1) && m.end > start || m.start == start)
            .cloned()
            .collect()
    }

    /// An error that only follows from a proof tool typed in the proof (`sledgehammer` and the
    /// like leave the proof unfinished while they run, so what closes it fails)
    pub fn expected_error(&self, m: &ProverMessage) -> bool {
        // A tool stopped by the human: as if it had run out of time
        if m.kind == "error"
            && self.profile.interrupted(&m.text)
            && self.local_prover().commands.iter().any(|c| self.typed_tools.contains(&c.name) && c.start <= m.start && m.start < c.end)
        {
            return true;
        }
        if m.kind != "error" || !self.profile.tool_waiting_error(&m.text) {
            return false;
        }
        let Some(block) = self.blocks.iter().find(|b| b.start <= m.start && m.start < b.end.max(b.start + 1)) else { return false };
        block.commands.iter().any(|c| self.typed_tools.contains(&c.name) && c.end <= m.start) || self.waits_on_previous(m)
    }

    /// Whether an error only says that the proof before its block is unfinished because a tool
    /// typed there is the proof's last step (Isabelle's "Bad context for command" on the next
    /// lemma, while sledgehammer runs in the one before it)
    pub fn waits_on_previous(&self, m: &ProverMessage) -> bool {
        if m.kind != "error" || !m.text.contains("Bad context for command") {
            return false;
        }
        let Some(i) = self.blocks.iter().position(|b| b.start <= m.start && m.start < b.end.max(b.start + 1)) else { return false };
        let previous = self.blocks[..i].iter().rev().find(|b| b.kind == theory::Kind::Formal);
        previous.and_then(|b| b.commands.last()).is_some_and(|c| self.typed_tools.contains(&c.name))
    }

    /// A block's status as shown: a proof waiting on a tool typed in it is unfinished, not failed
    pub fn shown_status(&self, start: usize, end: usize, status: Status) -> Status {
        if status != Status::Failed {
            return status;
        }
        let msgs = self.messages_in(start, end);
        let errors: Vec<_> = msgs.iter().filter(|m| m.kind == "error").collect();
        // A stopped tool is a failed command of its own, with or without a message
        let prover = self.local_prover();
        let failed: Vec<_> = prover.commands.iter().filter(|c| c.start >= start && c.end <= end.max(start + 1) && c.status == Status::Failed).collect();
        let only_tools = failed.iter().all(|c| self.typed_tools.contains(&c.name) || errors.iter().any(|m| m.start >= c.start && m.start < c.end && self.expected_error(m)));
        if errors.iter().all(|m| self.expected_error(m)) && only_tools { Status::Warned } else { status }
    }

    /// What the command at the caret is and has said, to refresh its state when it changes
    pub fn caret_signature(&self) -> (usize, usize, Status, usize) {
        let prover = self.local_prover();
        let at = self.cursor.saturating_sub(1);
        match prover.commands.iter().find(|c| c.start <= at && at < c.end) {
            Some(c) => (c.start, c.end, c.status, prover.messages.iter().filter(|m| m.start < c.end && m.end > c.start).count()),
            None => (0, 0, Status::Finished, 0),
        }
    }

    /// The command the caret is in, where it is now (local offsets), with its name
    pub fn caret_command(&self) -> Option<(usize, usize, String)> {
        let at = self.cursor.saturating_sub(1);
        self.local_prover().commands.iter().find(|c| c.start <= at && at < c.end).map(|c| (c.start, c.end, c.name.clone()))
    }

    /// Replace `start..end` with `text` as one undo step of its own, the caret just after it in
    /// every editor showing it (the open block's too, which otherwise keeps its caret where it
    /// was, in the middle of what came in)
    pub fn replace_step(&mut self, start: usize, end: usize, text: &str, table: &dyn Symbols) {
        self.break_step();
        self.replace(start, end, text, table);
        self.break_step();
        let cursor = self.cursor;
        if let Some(be) = &mut self.block_editor
            && be.start <= cursor
            && cursor <= be.end
        {
            let shown = be.shown();
            let li = LineIndex::new(&shown);
            place(&mut be.content, li.position(&shown, be.decoded.to_decoded(cursor - be.start)));
        }
    }

    pub fn source_text(&self, start: usize, end: usize) -> String {
        self.local.chars().skip(start).take(end.saturating_sub(start)).collect()
    }

    // ----- local edits

    /// Apply a source change made here; true if the views had to be rebuilt
    pub fn apply_local(&mut self, change: &Change, table: &dyn Symbols) {
        if change.is_empty() {
            return;
        }
        self.vertical_x = None;
        self.vertical_entry = None;
        // Where an undo step ends: a pause, the caret jumping, a change between typing and
        // deleting, the end of a word or line, and around anything that is not typing
        let (kind, at, after) = match change.as_slice() {
            [(s, e, t)] if s == e && t.chars().count() == 1 => {
                let c = t.chars().next().unwrap_or(' ');
                let kind = if c == '\n' {
                    EditKind::Newline
                } else if c.is_alphanumeric() || c == '_' || c == '\'' {
                    EditKind::Word
                } else {
                    EditKind::Space
                };
                (kind, *s, s + 1)
            }
            [(s, e, t)] if t.is_empty() && e - s == 1 => (EditKind::Delete, *e, *s),
            _ => (EditKind::Other, 0, 0),
        };
        let last = self.last_kind;
        let adjacent = match kind {
            EditKind::Delete => at == self.last_caret || after == self.last_caret,
            _ => at == self.last_caret,
        };
        let boundary = self.force_step
            || self.last_edit.elapsed().as_secs_f32() > 1.0
            || kind == EditKind::Other
            || last == EditKind::Other
            || !adjacent
            || (kind == EditKind::Delete) != (last == EditKind::Delete)
            || (!self.vim_session && (last == EditKind::Newline || (last == EditKind::Space && kind == EditKind::Word)));
        if boundary && !self.group {
            self.begin_step();
        }
        self.force_step = false;
        self.last_kind = kind;
        self.last_caret = after;
        self.last_edit = Instant::now();
        self.local = ot::apply(&self.local, change);
        if let Some(be) = &mut self.block_editor {
            be.start = ot::map_pos(be.start, change, false);
            be.end = ot::map_pos(be.end, change, true);
        }
        self.cursor = ot::map_pos(self.cursor, change, true);
        self.dirty = true;
    }

    /// The source view's edit or motion
    pub fn source_action(&mut self, action: Action, table: &dyn Symbols) -> bool {
        let edit = action.is_edit();
        self.source.perform(action);
        let cursor = self.source.cursor();
        if !edit {
            self.cursor = self.decoded.to_source(self.lines.offset(&self.decoded.text, cursor.position));
            return false;
        }
        let new = self.source.text();
        let change = self.decoded.source_change(table, &new);
        let caret_dec = self.lines_for(&new).offset(&new, cursor.position);
        let caret_src_old = self.decoded.to_source(caret_dec.min(self.decoded.src_of.len() - 1));
        let _ = caret_src_old;
        // where the caret is in the source after the change: just after the edit
        let caret_src = change.first().map_or(self.cursor, |(s, _, t)| s + t.chars().count());
        self.apply_local(&change, table);
        self.cursor = caret_src;
        self.redecode(table, Some(caret_dec), &new);
        self.refresh(table);
        true
    }

    fn lines_for(&self, text: &str) -> LineIndex {
        LineIndex::new(text)
    }

    /// Decode the local text again; when the decoded text differs from what
    /// the editor shows (a symbol was completed, an edit arrived), reset the
    /// editor and put the caret back
    fn redecode(&mut self, table: &dyn Symbols, caret_dec: Option<usize>, shown: &str) {
        self.decoded = table.decode(&self.local);
        self.lines = LineIndex::new(&self.decoded.text);
        if self.decoded.text != shown {
            let caret = self.decoded.to_decoded(self.cursor);
            let _ = caret_dec;
            self.source = text_editor::Content::with_text(&self.decoded.text);
            let pos = self.lines.position(&self.decoded.text, caret);
            place(&mut self.source, pos);
        }
        if let Some(be) = &mut self.block_editor {
            let src = self.local.chars().skip(be.start).take(be.end.saturating_sub(be.start)).collect::<String>();
            let dec = table.decode(&src);
            if dec.text != be.decoded.text {
                let caret = be.caret();
                be.decoded = dec;
                be.reshow(self.profile, Some(caret), table);
            } else {
                be.decoded = dec;
            }
        }
    }

    /// The source offset of the block editor's caret, for the proof state
    pub fn sync_block_cursor(&mut self) {
        let Some(be) = &self.block_editor else { return };
        let shown = be.shown();
        let li = LineIndex::new(&shown);
        self.cursor = be.start + be.decoded.to_source(li.offset(&shown, be.content.cursor().position));
    }

    /// The block editor's edit or motion
    pub fn block_action(&mut self, action: Action, table: &dyn Symbols) -> bool {
        let Some(be) = &mut self.block_editor else { return false };
        let edit = action.is_edit();
        let vertical = matches!(action, Action::Move(text_editor::Motion::Up | text_editor::Motion::Down));
        let before = be.content.cursor().position;
        if vertical {
            if self.vertical_x.is_none() {
                self.vertical_x = Some(caret_point(&mut be.content).x + super::view::block_text_inset(be));
            }
        } else {
            self.vertical_x = None;
            self.vertical_entry = None;
        }
        be.content.perform(action);
        if vertical && be.content.cursor().position != before {
            let y = caret_point(&mut be.content).y;
            let x = (self.vertical_x.unwrap_or(0.0) - super::view::block_text_inset(be)).max(0.0);
            be.content.perform(Action::Click(cosmic::iced::Point::new(x, y)));
        }
        let cursor = be.content.cursor();
        if !edit {
            // (a formula the caret moves into is shown as its source; one it leaves, set)
            let caret = be.caret();
            if be.reshow(self.profile, Some(caret), table) {
                self.compute_block_highlight();
            }
            let Some(be) = &self.block_editor else { return false };
            self.cursor = be.start + be.decoded.to_source(be.caret());
            return false;
        }
        let new = be.content.text();
        // (against the text as shown: a shown space for a line break is the line break)
        let rel: Change = ot::diff(&be.shown(), &new)
            .into_iter()
            .map(|(s, e, t)| (be.decoded.to_source(s), be.decoded.to_source(e), table.encode(&t.replace('\u{200B}', ""))))
            .collect();
        let start = be.start;
        let change: Change = rel.into_iter().map(|(s, e, t)| (s + start, e + start, t)).collect();
        let caret_src = change.first().map_or(self.cursor, |(s, _, t)| s + t.chars().count());
        // The block grows with what is typed at its end
        let end_before = be.end;
        self.apply_local(&change, table);
        if let Some(be) = &mut self.block_editor {
            let grown: isize = change.iter().map(|(s, e, t)| t.chars().count() as isize - (*e as isize - *s as isize)).sum();
            be.end = (end_before as isize + grown).max(be.start as isize) as usize;
            let src = self.local.chars().skip(be.start).take(be.end - be.start).collect::<String>();
            let dec = table.decode(&src);
            let caret = dec.to_decoded(caret_src.saturating_sub(be.start));
            let shown = shown_with_markup(self.profile, &dec, be.wrap.is_some(), Some(caret), table);
            if shown != new {
                let li = LineIndex::new(&shown);
                be.content = text_editor::Content::with_text(&shown);
                be.layout_revision = be.layout_revision.wrapping_add(1);
                place(&mut be.content, li.position(&shown, caret));
            }
            be.shown_text = shown;
            be.decoded = dec;
            let full = match &be.wrap {
                Some((before, after)) => format!("{before}{src}{after}"),
                None => src.clone(),
            };
            let (kind, preview) = preview_of(self.profile, &full, &self.kinds, table);
            be.kind = kind;
            be.preview = preview;
        }
        self.cursor = caret_src;
        let shown = self.decoded.text.clone();
        self.decoded = table.decode(&self.local);
        self.lines = LineIndex::new(&self.decoded.text);
        if self.decoded.text != shown {
            self.source = text_editor::Content::with_text(&self.decoded.text);
        }
        self.refresh(table);
        true
    }

    pub fn open_block(&mut self, index: usize, table: &dyn Symbols) {
        self.vertical_x = None;
        self.vertical_entry = None;
        let Some(block) = self.blocks.get(index) else { return };
        let full = self.source_text(block.start, block.end);
        let (kind, preview) = preview_of(self.profile, &full, &self.kinds, table);
        // A heading or text is edited as its text alone, set like the text it is
        let (start, end, wrap) = match (block.kind, block.body) {
            (theory::Kind::Heading(_) | theory::Kind::Text, Some((s, e))) if block.start <= s && s <= e && e <= block.end => {
                let (s, e) = self.prose_range(s, e);
                (s, e, Some((self.source_text(block.start, s), self.source_text(e, block.end))))
            }
            _ => (block.start, block.end, None),
        };
        let src = self.source_text(start, end);
        let decoded = table.decode(&src);
        let shown_text = shown_with_markup(self.profile, &decoded, wrap.is_some(), Some(decoded.text.chars().count()), table);
        let content = text_editor::Content::with_text(&shown_text);
        self.cursor = end;
        self.block_editor = Some(BlockEditor { layout_revision: 0, start, end, decoded, content, preview, kind, wrap, shown_text });
        self.compute_block_highlight();
    }

    /// The paragraphs of a text block's body: source ranges, split at blank lines
    pub fn paragraphs(&self, body: (usize, usize)) -> Vec<(usize, usize)> {
        let chars: Vec<char> = self.local.chars().collect();
        let (s, e) = (body.0.min(chars.len()), body.1.min(chars.len()));
        let mut out = Vec::new();
        let mut start = s;
        let mut k = s;
        while k < e {
            if chars[k] == '\n' {
                let mut j = k + 1;
                while j < e && (chars[j] == ' ' || chars[j] == '\t') {
                    j += 1;
                }
                if j < e && chars[j] == '\n' {
                    if k > start {
                        out.push((start, k));
                    }
                    let mut n = j;
                    while n < e && chars[n].is_whitespace() {
                        n += 1;
                    }
                    start = n;
                    k = n;
                    continue;
                }
            }
            k += 1;
        }
        if start < e {
            out.push((start, e));
        }
        out
    }

    /// The text block the prose editor is in, and its body
    fn prose_block(&self) -> Option<(theory::Block, (usize, usize))> {
        let be = self.block_editor.as_ref().filter(|be| be.wrap.is_some() && be.kind == theory::Kind::Text)?;
        let block = self.blocks.iter().find(|b| b.start <= be.start && be.start <= b.end)?.clone();
        let body = block.body?;
        Some((block, body))
    }

    /// Edit `s..e` of a block (a paragraph of its text) as its prose editor, the caret at `at`
    fn open_range(&mut self, block: &theory::Block, s: usize, e: usize, at: usize, table: &dyn Symbols) {
        let (s, e) = self.prose_range(s, e);
        let full = self.source_text(block.start, block.end);
        let (kind, preview) = preview_of(self.profile, &full, &self.kinds, table);
        let wrap = Some((self.source_text(block.start, s), self.source_text(e, block.end)));
        let decoded = table.decode(&self.source_text(s, e));
        let caret = decoded.to_decoded(at.clamp(s, e) - s);
        let shown = shown_with_markup(self.profile, &decoded, true, Some(caret), table);
        let mut content = text_editor::Content::with_text(&shown);
        let pos = LineIndex::new(&shown).position(&shown, caret);
        place(&mut content, pos);
        self.cursor = at.clamp(s, e);
        self.block_editor = Some(BlockEditor { layout_revision: 0, start: s, end: e, decoded, content, preview, kind, wrap, shown_text: shown });
        self.compute_block_highlight();
    }

    /// Delimiter spacing belongs to the wrapper, as in the rendered paragraph.
    /// Keep it in the source without introducing a leading space or empty row
    /// when the paragraph opens for editing.
    fn prose_range(&self, start: usize, end: usize) -> (usize, usize) {
        let text = self.source_text(start, end);
        let leading = text.chars().take_while(|c| c.is_whitespace()).count();
        let length = text.trim().chars().count();
        (start + leading, start + leading + length)
    }

    /// Prose is edited a paragraph at a time: the one the caret is in becomes source, the rest of
    /// the block stays as it reads
    pub fn narrow_to_paragraph(&mut self, table: &dyn Symbols) {
        let Some((block, body)) = self.prose_block() else { return };
        let paras = self.paragraphs(body);
        if paras.len() < 2 {
            return;
        }
        let at = self.cursor.clamp(body.0, body.1);
        let Some(&(ps, pe)) = paras.iter().find(|(s, e)| *s <= at && at <= *e).or_else(|| paras.iter().rev().find(|(s, _)| *s <= at)).or(paras.first()) else {
            return;
        };
        let be = self.block_editor.as_ref().unwrap();
        if (ps, pe) == (be.start, be.end) {
            return;
        }
        self.open_range(&block, ps, pe, at, table);
    }

    /// The next (or previous) paragraph of the text being edited, opened; false at the block's edge
    pub fn adjacent_paragraph(&mut self, down: bool, table: &dyn Symbols) -> bool {
        let Some((block, body)) = self.prose_block() else { return false };
        let paras = self.paragraphs(body);
        let be = self.block_editor.as_ref().unwrap();
        let Some(i) = paras.iter().position(|(s, e)| *s <= be.start && be.start <= *e) else { return false };
        let j = if down { i + 1 } else { i.wrapping_sub(1) };
        let Some(&(ps, pe)) = paras.get(j) else { return false };
        self.open_range(&block, ps, pe, if down { ps } else { pe }, table);
        true
    }

    /// Insert source text at a source offset, as the human
    pub fn insert(&mut self, at: usize, text: &str, table: &dyn Symbols) {
        self.replace(at, at, text, table);
    }

    /// Apply a whole change as local edits
    pub fn replace_change(&mut self, change: &Change, table: &dyn Symbols) {
        if change.is_empty() {
            return;
        }
        let cursor = self.cursor;
        self.apply_local(change, table);
        self.cursor = ot::map_pos(cursor, change, true);
        let shown = self.decoded.text.clone();
        self.redecode(table, None, &shown);
        self.refresh(table);
    }

    pub fn replace(&mut self, start: usize, end: usize, text: &str, table: &dyn Symbols) {
        let change = vec![(start, end, text.to_string())];
        self.apply_local(&change, table);
        self.cursor = start + text.chars().count();
        let shown = self.decoded.text.clone();
        self.redecode(table, None, &shown);
        self.refresh(table);
    }

    /// End the undo step being typed: the next edit starts another (and anything other than
    /// typing is a step of its own)
    pub fn break_step(&mut self) {
        self.force_step = true;
    }

    /// Begin a step whose every edit, until [`Self::end_group`], is one entry of the history
    pub fn begin_group(&mut self) {
        self.begin_step();
        self.group = true;
    }

    /// End it: the next edit begins another step
    pub fn end_group(&mut self) {
        self.group = false;
        self.force_step = true;
    }

    fn begin_step(&mut self) {
        let base = self.steps.back().map_or(&self.confirmed, |(_, text)| text);
        if *base != self.local {
            self.steps.push_back((self.txn.clone(), self.local.clone()));
        }
        self.txn = new_txn();
    }

    /// Whether every edit made here has reached the host
    pub fn settled(&self) -> bool {
        self.inflight.is_none() && !self.inflight_unknown && self.durability_pending.is_empty() && !self.durability_error && self.local == self.confirmed && self.steps.is_empty()
    }

    // ----- the host

    /// The next edit to send, if none is in flight and there is one: the oldest unsent undo
    /// step, each on its own
    pub fn outgoing(&mut self, id: u64) -> Option<(u64, Change, String)> {
        if self.inflight.is_some() {
            return None;
        }
        while let Some((txn, text)) = self.steps.pop_front() {
            if text != self.confirmed {
                self.inflight = Some(id);
                self.inflight_unknown = true;
                return Some((self.confirmed_rev, ot::diff(&self.confirmed, &text), txn));
            }
        }
        if self.local == self.confirmed {
            return None;
        }
        let change = ot::diff(&self.confirmed, &self.local);
        self.inflight = Some(id);
        self.inflight_unknown = true;
        Some((self.confirmed_rev, change, self.txn.clone()))
    }

    /// A change the host committed. Returns false when the replica is out of
    /// step and must be read again
    pub fn committed(&mut self, revision: u64, change: &Change, request: Option<u64>, mine: bool, table: &dyn Symbols) -> bool {
        if revision != self.confirmed_rev + 1 {
            return revision <= self.confirmed_rev;
        }
        let own = mine && request.is_some() && request == self.inflight;
        let confirmed = ot::apply(&self.confirmed, change);
        self.prover = self.prover.mapped(change);
        // (the prover reports on this revision later: until then, checked from here on again)
        if let Some(first) = change.first().map(|(s, _, _)| *s) {
            self.stale_from = Some(self.stale_from.map_or(first, |s| ot::map_pos(s, change, false).min(first)));
        }
        if own {
            self.inflight = None;
            self.inflight_unknown = false;
            self.inflight_edit = None;
            self.confirmed = confirmed;
            self.confirmed_rev = revision;
        } else {
            // Unsent steps move with the change as the text being typed does
            for (_, text) in &mut self.steps {
                let pending = ot::diff(&self.confirmed, text);
                *text = ot::apply(&confirmed, &ot::rebase_merge(&pending, change));
            }
            let pending = ot::diff(&self.confirmed, &self.local);
            let rebased = ot::rebase_merge(&pending, change);
            let local = ot::apply(&confirmed, &rebased);
            let moved = ot::diff(&self.local, &local);
            self.cursor = ot::map_pos(self.cursor, &moved, true);
            if let Some(be) = &mut self.block_editor {
                be.start = ot::map_pos(be.start, &moved, false);
                be.end = ot::map_pos(be.end, &moved, true);
            }
            self.confirmed = confirmed;
            self.confirmed_rev = revision;
            self.local = local;
            let shown = self.decoded.text.clone();
            self.redecode(table, None, &shown);
        }
        self.refresh(table);
        true
    }

    pub fn prover_update(&mut self, prover: &serde_json::Value, table: &dyn Symbols) {
        if prover["revision"].as_u64() == Some(self.confirmed_rev) {
            let report = Prover::from_json(prover);
            // A report on the text as it is now: nothing it has not seen
            let covered = prover["backend_revision"].as_u64().is_some_and(|b| b >= self.confirmed_rev);
            let stale_changed = covered && self.stale_from.take().is_some();
            if report == self.prover && !stale_changed {
                return;
            }
            self.prover = report;
            self.refresh_views(table, true);
        }
    }

    pub fn status(&self) -> (u64, u64, u64, u64, bool) {
        let s = &self.prover.status;
        (
            s["finished"].as_u64().unwrap_or(0),
            s["unprocessed"].as_u64().unwrap_or(0),
            s["running"].as_u64().unwrap_or(0),
            s["failed"].as_u64().unwrap_or(0),
            s["consolidated"].as_bool().unwrap_or(false),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> serde_json::Value {
        serde_json::json!({"text": text, "revision": 3, "prover": {}})
    }

    #[test]
    fn local_edits_and_remote_rebase() {
        let table = Table::fallback();
        let mut t = Theory::new("/x.thy".into(), &doc("lemma \"\\<forall>x. P x\"\n  sorry\n"), &table, Arc::new(HashMap::new()), Mode::Source, crate::prover::profile(None));
        assert!(t.decoded.text.contains('∀'));
        // the human types at the end of the first line
        let end = t.decoded.text.find('\n').unwrap();
        place(&mut t.source, Position { line: 0, column: end });
        t.source_action(Action::Edit(text_editor::Edit::Insert('!')), &table);
        assert!(t.local.starts_with("lemma \"\\<forall>x. P x\"!"));
        let (rev, change, _) = t.outgoing(7).unwrap();
        assert_eq!(rev, 3);
        assert_eq!(change.len(), 1);
        // an agent replaces the sorry before our edit is confirmed
        let pos = t.confirmed.find("sorry").unwrap();
        let cp = t.confirmed[..pos].chars().count();
        assert!(t.committed(4, &vec![(cp, cp + 5, "by simp".into())], None, false, &table));
        assert!(t.local.contains("by simp") && t.local.contains("\"!"));
        // our edit is confirmed, rebased by the host the same way
        let ours = ot::rebase_merge(&change, &vec![(cp, cp + 5, "by simp".into())]);
        assert!(t.committed(5, &ours, Some(7), true, &table));
        assert_eq!(t.confirmed, t.local);
    }

    #[test]
    fn typing_a_symbol_escape_shows_the_symbol() {
        let table = Table::fallback();
        let mut t = Theory::new("/x.thy".into(), &doc(""), &table, Arc::new(HashMap::new()), Mode::Source, crate::prover::profile(None));
        for c in "\\<and>".chars() {
            t.source_action(Action::Edit(text_editor::Edit::Insert(c)), &table);
        }
        assert_eq!(t.local, "\\<and>");
        assert_eq!(t.source.text(), "∧");
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn deep_isabelle_heading_previews_and_edits_at_one_level() {
        let table = Table::fallback();
        let src = "paragraph \\<open>Deep title\\<close>";
        let (kind, doc) = preview_of(crate::prover::profile(None), src, &HashMap::new(), &table);
        let theory::Kind::Heading(level) = kind else { panic!("a heading: {kind:?}") };
        assert_eq!(level, 5, "Isabelle paragraph is level 5");
        let doc = doc.expect("a preview");
        let rendered = format!("{doc:?}");
        assert!(rendered.contains("Heading(4)"), "rendered at level 4: {rendered}");
        let edited = crate::view::prose_heading_style(rendered_heading_level(level), 1.5);
        let as_four = crate::view::prose_heading_style(4, 1.5);
        assert_eq!(edited, as_four, "the editor sets level 5 as the rendered view does");
        assert_ne!(edited, crate::view::prose_heading_style(level, 1.5), "unclamped it differed");
    }

    #[test]
    fn previews_prose() {
        let table = Table::fallback();
        let src = "text \\<open>We prove that $x + 0 = x$ for every @{typ nat}.\\<close>";
        let (kind, doc) = preview_of(crate::prover::profile(None), src, &HashMap::new(), &table);
        assert_eq!(kind, theory::Kind::Text);
        let doc = doc.expect("a preview");
        let text = format!("{:?}", doc);
        assert!(text.contains("We prove"), "{text}");
    }
}

#[cfg(test)]
mod shown_tests {
    use super::shown_text;

    #[test]
    fn prose_line_breaks_show_as_spaces() {
        let src = "We prove\n  that it holds.\n\nNext para\n  here";
        let shown = shown_text(src, true);
        assert_eq!(shown.chars().count(), src.chars().count());
        assert_eq!(shown, "We prove \u{200B}\u{200B}that it holds.\n\nNext para \u{200B}\u{200B}here");
        assert_eq!(shown_text(src, false), src);
    }
}

/// A tooltip and a definition, as the host's `info` gives them, from the prelude's
/// `deductum_entity` query: parts headed "#deductum:entity KIND", then the full name, the
/// defining file and line, and what it is
pub fn entity_info(messages: &[serde_json::Value]) -> serde_json::Value {
    let mut items = Vec::new();
    let mut definition = serde_json::Value::Null;
    for m in messages {
        let raw = m["raw"].as_str().or_else(|| m["text"].as_str()).unwrap_or("");
        let Some(rest) = raw.strip_prefix("#deductum:entity ") else { continue };
        let mut parts = rest.splitn(5, '\n');
        let (kind, full, file, line, what) = (
            parts.next().unwrap_or("").trim(),
            parts.next().unwrap_or("").trim(),
            parts.next().unwrap_or("").trim(),
            parts.next().unwrap_or("0").trim().parse::<u64>().unwrap_or(0),
            parts.next().unwrap_or("").trim_end(),
        );
        let text = format!("{kind} {full}\n{what}");
        items.push(serde_json::json!({"kind": "writeln", "raw": text, "text": text, "spans": []}));
        if definition.is_null() && !file.is_empty() && line > 0 {
            definition = serde_json::json!({"file": file, "line": line, "name": full, "kind": kind});
        }
    }
    let tooltip = if items.is_empty() { serde_json::Value::Null } else { serde_json::json!({"items": items}) };
    serde_json::json!({"tooltip": tooltip, "definition": definition})
}

#[cfg(test)]
mod staleness_tests {
    use super::*;

    #[test]
    fn what_the_prover_has_not_seen_waits_until_it_reports() {
        let table = Table::fallback();
        let text = "lemma a: \"True\"\n  by simp\n\nlemma b: \"True\"\n  by simp\n";
        let b = text.find("lemma b").unwrap();
        let prover = serde_json::json!({"revision": 3, "backend_revision": 3, "commands": [
            [0, 16, "lemma", "finished"], [16, 26, "by", "finished"], [b, b + 16, "lemma", "finished"], [b + 16, text.len(), "by", "finished"]]});
        let doc = serde_json::json!({"text": text, "revision": 3, "prover": prover});
        let mut t = Theory::new("/x.thy".into(), &doc, &table, Arc::new(HashMap::new()), Mode::Source, crate::prover::profile(None));
        assert_eq!(t.status_in(b, text.len()), Some(Status::Finished));
        // Typed in the second lemma, not yet sent: it waits, the first does not
        t.replace(b + 6, b + 6, "x", &table);
        assert_eq!(t.status_in(b, b + 20), Some(Status::Unprocessed));
        assert_eq!(t.status_in(0, 26), Some(Status::Finished));
        // Sent and committed: still waiting, until the prover reports on that revision
        let (rev, change, _) = t.outgoing(7).unwrap();
        assert!(t.committed(rev + 1, &change, Some(7), true, &table));
        assert_eq!(t.status_in(b, b + 20), Some(Status::Unprocessed));
        // A report on the old revision (mapped by the host) does not end the wait
        let mut old = prover.clone();
        old["revision"] = serde_json::json!(4);
        t.prover_update(&old, &table);
        assert_eq!(t.status_in(b, b + 20), Some(Status::Unprocessed));
        let mut seen = old.clone();
        seen["backend_revision"] = serde_json::json!(4);
        t.prover_update(&seen, &table);
        assert_eq!(t.status_in(b, b + 20), Some(Status::Finished));
        assert!(t.unchecked_from().is_none());
    }
}

#[cfg(test)]
mod prover_view_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rendered_hits_compose_exact_markdown_bytes_with_source_characters() {
        let profile = &crate::prover::LEAN;
        let table = profile.symbols(None);
        let text = "/-! same **same** [é](target) $x$ end -/\n";
        let doc = json!({"text": text, "revision": 1, "prover": {}});
        let t = Theory::new("/mapped.lean".into(), &doc, &*table, Arc::new(HashMap::new()), Mode::Literate, profile);
        let prose = t.blocks.iter().position(|b| b.kind == theory::Kind::Text).unwrap();
        let source_at = |needle: &str, skip: usize| {
            let at = text.find(needle).unwrap() + skip;
            text[..at].chars().count()
        };
        assert_eq!(t.source_of_hit(prose, 0, 5), Some(source_at("**same", 2)));
        assert_eq!(t.source_of_hit(prose, 0, 10), Some(source_at("[é]", 1)));
        assert_eq!(t.source_of_hit(prose, 0, 13), Some(source_at("$x$", 0)));
        assert_eq!(t.source_of_hit(prose, 0, 14), Some(source_at("$x$", 3)));
        assert_eq!(t.source_of_hit(prose, 0, 18), Some(source_at("end", 3)));
        assert_eq!(t.source_of_hit(prose, 1, 0), None);
    }

    #[test]
    fn prover_reports_keep_the_prose_maps_and_unchanged_highlighting() {
        let profile = &crate::prover::LEAN;
        let table = profile.symbols(None);
        let text = "/-- A paragraph with **markup** and $x$. -/\ntheorem one : True := by\n  trivial\n";
        let commands: Vec<_> = profile.lexical_commands(text, &HashMap::new()).into_iter()
            .map(|c| json!([c.start, c.end, c.name, "finished"])).collect();
        let report = json!({"revision": 1, "backend_revision": 1, "commands": commands});
        let doc = json!({"text": text, "revision": 1, "prover": report});
        let mut t = Theory::new("/one.lean".into(), &doc, &*table, Arc::new(HashMap::new()), Mode::Literate, profile);
        let prose = t.blocks.iter().position(|b| b.kind == theory::Kind::Text).expect("a docstring");
        let rendered = t.docs[prose].as_ref().unwrap().clone();
        let maps = t.prose_maps.as_ptr();
        let runs = t.prose_maps[prose].as_ref().unwrap().runs.as_ptr();
        let highlight = t.highlight.0.clone();
        let formal = t.block_views.iter().flatten().next().unwrap().highlight.0.clone();
        let views = t.block_views.as_ptr();

        // Activity counters do not change the text, its mapping or its colours.
        let mut progress = report.clone();
        progress["status"] = json!({"finished": 1});
        t.prover_update(&progress, &*table);
        assert!(Arc::ptr_eq(&rendered, t.docs[prose].as_ref().unwrap()));
        assert_eq!(maps, t.prose_maps.as_ptr(), "a prover report must retain the prose maps");
        assert_eq!(runs, t.prose_maps[prose].as_ref().unwrap().runs.as_ptr());
        assert!(Arc::ptr_eq(&highlight, &t.highlight.0), "unchanged highlighting keeps its generation");
        assert!(Arc::ptr_eq(&formal, &t.block_views.iter().flatten().next().unwrap().highlight.0));
        assert_eq!(views, t.block_views.as_ptr(), "aggregate progress retains the formal views themselves");

        // A real colour/status change must still reach both editors.
        progress["commands"][0][3] = json!("running");
        t.prover_update(&progress, &*table);
        assert_eq!(maps, t.prose_maps.as_ptr());
        assert_eq!(runs, t.prose_maps[prose].as_ref().unwrap().runs.as_ptr());
        assert!(t.blocks.iter().any(|b| b.status == Status::Running));
        assert!(!Arc::ptr_eq(&highlight, &t.highlight.0));
        assert!(!Arc::ptr_eq(&formal, &t.block_views.iter().flatten().next().unwrap().highlight.0));
    }

    #[test]
    fn prover_messages_follow_remote_edits_while_local_edits_are_pending() {
        let table = Table::fallback();
        let text = "lemma a: \"True\"\n  by simp\n";
        let report = json!({"revision": 1, "backend_revision": 1,
            "commands": [[0, 16, "lemma", "finished"], [16, text.len(), "by", "finished"]],
            "messages": [{"start": 18, "end": 20, "kind": "warning", "text": "at by"}]});
        let doc = json!({"text": text, "revision": 1, "prover": report});
        let mut t = Theory::new("/a.thy".into(), &doc, &table, Arc::new(HashMap::new()), Mode::Source, crate::prover::profile(None));
        t.replace(0, 0, "(* local *)\n", &table);
        let local_shift = "(* local *)\n".chars().count();
        assert_eq!(t.messages_in(0, usize::MAX)[0].start, 18 + local_shift);
        assert_eq!(t.unchecked_from(), Some(0));

        // Another author adds text to the confirmed document before the same command.
        let remote = vec![(0, 0, "(* remote *)\n".to_string())];
        assert!(t.committed(2, &remote, None, false, &table));
        let remote_shift = "(* remote *)\n".chars().count();
        let messages = t.messages_in(0, usize::MAX);
        assert_eq!(messages[0].start, 18 + local_shift + remote_shift);
        assert!(t.local.contains("(* local *)") && t.local.contains("(* remote *)"));
        assert_eq!(t.caret_command().map(|c| c.2), Some("lemma".into()));

        // Repeated reads must keep reporting the same mapped positions.
        assert_eq!(messages, t.messages_in(0, usize::MAX));
        assert_eq!(t.status_in(0, usize::MAX), Some(Status::Unprocessed));
    }

    #[test]
    fn source_edits_refresh_duplicate_prose_and_drop_removed_cache_entries() {
        let profile = &crate::prover::LEAN;
        let table = profile.symbols(None);
        let text = "/-! Repeated paragraph. -/\ntheorem one : True := by trivial\n\n/-! Repeated paragraph. -/\ntheorem two : True := by trivial\n";
        let doc = json!({"text": text, "revision": 1, "prover": {}});
        let mut t = Theory::new("/duplicates.lean".into(), &doc, &*table, Arc::new(HashMap::new()), Mode::Literate, profile);
        let prose_indices = |t: &Theory| t.blocks.iter().enumerate().filter_map(|(i, b)| (b.kind == theory::Kind::Text).then_some(i)).collect::<Vec<_>>();
        let indices = prose_indices(&t);
        assert_eq!(indices.len(), 2);
        let repeated = t.docs[indices[0]].as_ref().unwrap().clone();
        assert!(Arc::ptr_eq(&repeated, t.docs[indices[1]].as_ref().unwrap()), "duplicate paragraphs share their parsed Doc");

        let start = text.find("Repeated").unwrap();
        t.replace(start, start + "Repeated".len(), "Changed and longer", &*table);
        let indices = prose_indices(&t);
        assert!(!Arc::ptr_eq(&repeated, t.docs[indices[0]].as_ref().unwrap()));
        assert!(Arc::ptr_eq(&repeated, t.docs[indices[1]].as_ref().unwrap()));
        let map = t.prose_maps[indices[0]].as_ref().unwrap();
        assert!(map.markdown.contains("Changed and longer"), "the source map must follow the edit");
        let md = map.markdown.find("longer").unwrap();
        let source = t.source_of_markdown(indices[0], md).unwrap();
        assert!(t.local.chars().skip(source).collect::<String>().starts_with("longer"));

        // Removing the other prose block changes topology and leaves no user of its Doc.
        let removed = t.blocks[indices[1]].clone();
        t.replace(removed.start, removed.end, "", &*table);
        assert_eq!(prose_indices(&t).len(), 1);
        assert!(!t.doc_cache.values().any(|(doc, _)| Arc::ptr_eq(doc, &repeated)), "removed prose must not stay in the document cache");
        assert_eq!(t.docs.len(), t.blocks.len());
        assert_eq!(t.prose_maps.len(), t.blocks.len());
        assert_eq!(t.block_views.len(), t.blocks.len());
    }

    #[test]
    fn messages_and_semantic_colours_refresh_views_without_command_changes() {
        let profile = &crate::prover::LEAN;
        let table = profile.symbols(None);
        let text = "theorem one : True := by trivial\n";
        let commands: Vec<_> = profile.lexical_commands(text, &HashMap::new()).into_iter()
            .map(|c| json!([c.start, c.end, c.name, "finished"])).collect();
        let mut report = json!({"revision": 1, "backend_revision": 1, "commands": commands});
        let doc = json!({"text": text, "revision": 1, "prover": report});
        let mut t = Theory::new("/colours.lean".into(), &doc, &*table, Arc::new(HashMap::new()), Mode::Literate, profile);
        let at = text.find("one").unwrap();
        let initial = t.highlight.0.clone();
        report["messages"] = json!([{"start": at, "end": at + 3, "kind": "error", "text": "a diagnostic"}]);
        t.prover_update(&report, &*table);
        assert_eq!(t.classes[t.decoded.to_decoded(at)], Some(Hl::Error));
        assert!(!Arc::ptr_eq(&initial, &t.highlight.0));
        let diagnostic = t.block_views.iter().flatten().next().unwrap().highlight.0.clone();

        report["messages"] = json!([]);
        report["colours"] = json!([[at, at + 3, "free"]]);
        t.prover_update(&report, &*table);
        assert_eq!(t.classes[t.decoded.to_decoded(at)], Some(Hl::Sem(Sem::Free)));
        assert!(!Arc::ptr_eq(&diagnostic, &t.block_views.iter().flatten().next().unwrap().highlight.0));
        assert!(t.messages_in(0, usize::MAX).is_empty());
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    #[test]
    fn a_column_inside_a_character_does_not_panic() {
        let text = "ab\u{200b}c\nx \u{21d2} y";
        let index = LineIndex::new(text);
        // byte 3 is inside the zero-width space (bytes 2..5)
        assert_eq!(index.offset(text, Position { line: 0, column: 3 }), 2);
        assert_eq!(index.offset(text, Position { line: 1, column: 3 }), index.starts[1].0 + 2);
        assert_eq!(index.offset(text, Position { line: 0, column: 5 }), 3);
    }
}

#[cfg(test)]
mod prose_completion_tests {
    use super::*;

    #[test]
    fn completing_in_a_wrapped_paragraph_keeps_its_line_breaks() {
        let table = Table::fallback();
        let text = "theory T imports Main begin\n\ntext \\<open>An assignment shares the unstable\n  observation. Its final commitment applies a\n  total substitution, assign\\<close>\n\nend\n";
        let doc = serde_json::json!({"text": text, "revision": 1, "prover": {}});
        let mut t = Theory::new("/x.thy".into(), &doc, &table, Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None));
        let i = t.blocks.iter().position(|b| b.kind == theory::Kind::Text).unwrap();
        t.open_block(i, &table);
        let be = t.block_editor.as_ref().unwrap();
        assert!(be.shown().contains('\u{200B}'), "line breaks are shown as spaces, indents as zero-width spaces");
        // the caret after "assign" (the paragraph's end), and a completion of it
        let shown = be.shown();
        let at = shown.rfind("assign").unwrap() + "assign".len();
        let at = shown[..at].chars().count();
        let li = LineIndex::new(&shown);
        let be = t.block_editor.as_mut().unwrap();
        place(&mut be.content, li.position(&shown, at));
        t.completions = vec![Completion { back: 0, replace: 6, text: "assignment".into(), label: "assignment".into() }];
        t.accept_completion(0, &table);
        assert!(!t.local.contains('\u{200B}'), "no zero-width spaces in the source: {:?}", t.local);
        assert!(t.local.contains("the unstable\n  observation. Its final commitment applies a\n  total substitution, assignment"), "{:?}", t.local);
    }
}

#[cfg(test)]
mod keyword_tests {
    use super::*;

    #[test]
    fn the_whole_keyword_is_coloured_as_its_sorry() {
        let table = Table::fallback();
        let text = "lemma \"True\"\n  show ?thesis unfolding f_def claude[opus]\n  @tock try this\n";
        let k = text.find("claude").unwrap();
        let a = text.find("@tock").unwrap();
        // the prover colours its `sorry`: five characters
        let prover = serde_json::json!({"revision": 1, "backend_revision": 1, "commands": [], "colours": [[k, k + 5, "keyword1"], [a, a + 5, "keyword1"]]});
        let doc = serde_json::json!({"text": text, "revision": 1, "prover": prover});
        for mode in [Mode::Source, Mode::Literate] {
            let t = Theory::new("/x.thy".into(), &doc, &table, Arc::new(HashMap::new()), mode, crate::prover::profile(None));
            let classes = |s: usize, n: usize| (s..s + n).map(|i| t.classes[t.decoded.to_decoded(i)]).collect::<Vec<_>>();
            let kw = classes(k, "claude[opus]".len());
            assert!(kw.iter().all(|c| *c == kw[0] && c.is_some()), "{mode:?}: claude[opus] coloured alike: {kw:?}");
            let at = classes(a, "@tock".len());
            assert!(at.iter().all(|c| *c == at[0] && c.is_some()), "{mode:?}: @tock coloured alike: {at:?}");
        }
    }
}

#[cfg(test)]
mod entity_tests {
    use super::*;

    #[test]
    fn entity_results_become_a_tooltip_and_a_definition() {
        let m = serde_json::json!({"raw": "#deductum:entity constant\nList.rev\n/isa/src/HOL/List.thy\n420\nrev :: 'a list \\<Rightarrow> 'a list"});
        let info = entity_info(&[m, serde_json::json!({"raw": "#deductum:done"})]);
        assert_eq!(info["definition"]["line"], 420);
        assert_eq!(info["definition"]["file"], "/isa/src/HOL/List.thy");
        assert!(info["tooltip"]["items"][0]["raw"].as_str().unwrap().starts_with("constant List.rev\nrev ::"));
    }
}
