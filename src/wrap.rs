// SPDX-License-Identifier: GPL-3.0-only

//! How many rows the lines of code take once they wrap. A cell's lines wrap at the cell's width
//! rather than widening it, so what is placed level with a line (the band over the construct, the
//! caret's line, the goal) needs the rows above it, which the editor does not say. They are
//! measured with cosmic-text itself, as the editor lays them out, so that the two cannot differ.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use cosmic::iced::Font;
use cosmic::iced::advanced::graphics::text as graphics;

/// How code editors wrap: at words, and within a word too long for a row of its own, so that
/// nothing ever runs past the cell's edge
pub const WRAPPING: cosmic::iced::widget::text::Wrapping = cosmic::iced::widget::text::Wrapping::WordOrGlyph;

/// A wrapped line of code continues at its own indentation plus this: Tom, 8 Nov
pub const HANGING: cosmic_text::HangingIndent = cosmic_text::HangingIndent { columns: 2.0 };

/// What an editor holds: the text of a code editor, whose wrapped lines hang, or of prose, whose do
/// not. The setting goes with the editor's buffer from here on (iced makes the buffer inside
/// `Content` and gives no way to reach it after: vendor/cosmic-text/README.md). Nothing else in the
/// window is affected, and the document's text is not touched
pub fn editor_content(text: &str, prose: bool) -> cosmic::widget::text_editor::Content {
    cosmic_text::with_hanging_indent((!prose).then_some(HANGING), || cosmic::widget::text_editor::Content::with_text(text))
}

/// What a line costs to measure is its shaping, so results are kept (a window being resized asks
/// for many widths; this many lines are more than a screenful at each)
const CACHE_LIMIT: usize = 16_384;

thread_local! {
    static ROWS: RefCell<HashMap<u64, usize>> = RefCell::new(HashMap::new());
}

/// The rows of a line that cannot be measured cheaply, in this font at this width
fn measured(line: &str, width: f32, font: Font, size: f32) -> usize {
    let mut hasher = DefaultHasher::new();
    line.hash(&mut hasher);
    width.to_bits().hash(&mut hasher);
    size.to_bits().hash(&mut hasher);
    font.hash(&mut hasher);
    let key = hasher.finish();
    if let Some(rows) = ROWS.with(|r| r.borrow().get(&key).copied()) {
        return rows;
    }
    let rows = {
        let mut font_system = graphics::font_system().write().expect("Write font system");
        let mut buffer = cosmic_text::Buffer::new_empty(cosmic_text::Metrics::new(size, size));
        buffer.set_size(Some(width), None);
        buffer.set_wrap(graphics::to_wrap(WRAPPING));
        buffer.set_hanging_indent(Some(HANGING));
        buffer.set_text(line, &graphics::to_attributes(font), cosmic_text::Shaping::Advanced, None);
        buffer.line_layout(font_system.raw(), 0).map_or(1, |layout| layout.len().max(1))
    };
    ROWS.with(|r| {
        let mut r = r.borrow_mut();
        if r.len() >= CACHE_LIMIT {
            r.clear();
        }
        r.insert(key, rows);
    });
    rows
}

/// The rows each line of `text` takes in an editor whose text is `width` wide, one entry a line;
/// `advance` is about what a character of this font takes. A line that plainly fits is one row
/// without being shaped: an ASCII line by its characters, another with room for what a font
/// standing in for a symbol may add
pub fn rows(text: &str, width: f32, font: Font, size: f32, advance: f32) -> Vec<usize> {
    let line_rows = |line: &str| {
        let wide = line.chars().count() as f32 * advance;
        let fits = if line.is_ascii() { wide <= width } else { wide * 1.5 <= width };
        if fits { 1 } else { measured(line, width, font, size) }
    };
    let mut rows: Vec<usize> = text.split('\n').map(line_rows).collect();
    if rows.is_empty() {
        rows.push(1);
    }
    rows
}

/// How far below the first line's top this line starts, in rows
pub fn rows_above(rows: &[usize], line: usize) -> usize {
    rows.iter().take(line).sum()
}
