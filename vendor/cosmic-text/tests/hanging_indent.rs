// SPDX-License-Identifier: MIT OR Apache-2.0
// (Deductum's patch to cosmic-text 0.19.0: see vendor/cosmic-text/README.md. Deductum's own test
// binary runs this file too, so that its usual suite covers the patch.)

//! A hanging indent, with the monospace font from cosmic-text's own test fonts

use cosmic_text::{Attrs, Buffer, Family, FontSystem, HangingIndent, Metrics, Shaping, Wrap, fontdb};

const FONT_SIZE: f32 = 20.0;
const LINE: &str = "    alpha beta gamma delta epsilon zeta eta theta iota kappa";

fn font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.load_font_data(include_bytes!("../fonts/FiraMono-Medium.ttf").to_vec());
    FontSystem::new_with_locale_and_db("en-GB".into(), db)
}

fn attrs() -> Attrs<'static> {
    Attrs::new().family(Family::Name("Fira Mono"))
}

fn advance(fs: &mut FontSystem) -> f32 {
    let mut buffer = Buffer::new(fs, Metrics::new(FONT_SIZE, FONT_SIZE));
    buffer.set_text("xxxxxxxxxx", &attrs(), Shaping::Advanced, None);
    buffer.shape_until_scroll(fs, false);
    buffer.layout_runs().next().unwrap().line_w / 10.0
}

fn buffer(fs: &mut FontSystem, text: &str, width: f32, hanging: Option<HangingIndent>) -> Buffer {
    let mut buffer = Buffer::new(fs, Metrics::new(FONT_SIZE, FONT_SIZE));
    buffer.set_hanging_indent(hanging);
    buffer.set_wrap(Wrap::WordOrGlyph);
    buffer.set_size(Some(width), None);
    buffer.set_text(text, &attrs(), Shaping::Advanced, None);
    buffer.shape_until_scroll(fs, false);
    buffer
}

/// A row's text, and where its first glyph starts
fn rows(buffer: &Buffer) -> Vec<(String, f32)> {
    buffer
        .layout_runs()
        .map(|run| {
            let (start, end) = (run.glyphs[0].start, run.glyphs[run.glyphs.len() - 1].end);
            (run.text[start..end].to_string(), run.glyphs[0].x)
        })
        .collect()
}

const TWO: Option<HangingIndent> = Some(HangingIndent { columns: 2.0 });

/// The first row is where it was; each row after it starts at the line's indentation and two columns
#[test]
fn wrapped_rows_start_at_the_indentation_and_two_columns() {
    let mut fs = font_system();
    let column = advance(&mut fs);
    let width = 24.5 * column;
    let hanging = rows(&buffer(&mut fs, LINE, width, TWO));
    assert!(hanging.len() >= 3, "{hanging:?}");
    assert!(hanging[0].0.starts_with("    alpha") && hanging[0].1.abs() < 1e-3, "{hanging:?}");
    for (text, x) in &hanging[1..] {
        assert!((x - 6.0 * column).abs() < 1e-3, "{text:?} starts at {x}, not 6 columns of {column}");
        assert!(!text.starts_with(' '), "{text:?}: a row does not start with the blank it wrapped at");
    }
}

/// Wrapped rows wrap at the width less the indent: nothing goes past the width, and every word is kept
#[test]
fn wrapped_rows_wrap_at_the_width_less_the_indent() {
    let mut fs = font_system();
    let column = advance(&mut fs);
    let width = 24.5 * column;
    let hanging = buffer(&mut fs, LINE, width, TWO);
    for run in hanging.layout_runs() {
        assert!(run.line_w <= width + 1e-3, "{} is wider than {width}", run.line_w);
        let last = run.glyphs.last().unwrap();
        assert!(last.x + last.w <= width + 1e-3, "the row ends at {}", last.x + last.w);
    }
    let text: Vec<String> = rows(&hanging).into_iter().map(|(t, _)| t).collect();
    assert_eq!(text.join(" "), LINE);
    // at the same width without the indent, more fits on the wrapped rows
    let plain = rows(&buffer(&mut fs, LINE, width, None));
    assert!(plain.iter().all(|(_, x)| x.abs() < 1e-3), "{plain:?}");
    assert!(plain.len() <= text.len());
}

/// A word too long for a wrapped row breaks to fit the row, not the width
#[test]
fn a_word_too_long_for_a_wrapped_row_breaks_inside_it() {
    let mut fs = font_system();
    let column = advance(&mut fs);
    let width = 20.5 * column;
    let long = format!("  {}", "x".repeat(70));
    let runs = buffer(&mut fs, &long, width, TWO);
    for run in runs.layout_runs() {
        let last = run.glyphs.last().unwrap();
        assert!(last.x + last.w <= width + 1e-3, "the row ends at {}", last.x + last.w);
    }
}

/// Very deep indentation leaves room for text: the indent is at most half the width
#[test]
fn the_indent_is_at_most_half_the_width() {
    let mut fs = font_system();
    let column = advance(&mut fs);
    let width = 20.5 * column;
    let deep = format!("{}alpha beta gamma delta", " ".repeat(16));
    let wrapped = rows(&buffer(&mut fs, &deep, width, TWO));
    assert!(wrapped.len() >= 2, "{wrapped:?}");
    assert!((wrapped[1].1 - width / 2.0).abs() < 1e-3, "{wrapped:?}");
}

/// Without the setting, and for a line that fits, nothing changes
#[test]
fn a_line_that_fits_and_a_buffer_without_the_setting_are_as_they_were() {
    let mut fs = font_system();
    let column = advance(&mut fs);
    assert_eq!(rows(&buffer(&mut fs, "    fits", 40.0 * column, TWO)), rows(&buffer(&mut fs, "    fits", 40.0 * column, None)));
    assert!(rows(&buffer(&mut fs, LINE, 24.5 * column, None)).iter().all(|(_, x)| x.abs() < 1e-3));
}

/// A click on a wrapped row lands on the character drawn there, and one left of the indent on the
/// row's first character; the caret's position there is the glyph's
#[test]
fn clicks_and_the_caret_follow_the_indent() {
    let mut fs = font_system();
    let column = advance(&mut fs);
    let buffer = buffer(&mut fs, LINE, 24.5 * column, TWO);
    let runs: Vec<_> = buffer.layout_runs().collect();
    let second = &runs[1];
    let y = second.line_top + 1.0;
    // the third character of the wrapped row
    let glyph = &second.glyphs[2];
    let hit = buffer.hit(glyph.x + 1.0, y).unwrap();
    assert_eq!(hit.index, glyph.start);
    assert!((second.cursor_position(&hit).unwrap() - glyph.x).abs() < 1e-3);
    // in the indent: the row's first character, not the end of the row
    let hit = buffer.hit(1.0, y).unwrap();
    assert_eq!(hit.index, second.glyphs[0].start);
    assert!((second.cursor_position(&hit).unwrap() - 6.0 * column).abs() < 1e-3);
}

/// A selection across the wrap: on the wrapped row it starts at the indent, not the left edge
#[test]
fn a_selection_across_the_wrap_starts_at_the_indent() {
    let mut fs = font_system();
    let column = advance(&mut fs);
    let buffer = buffer(&mut fs, LINE, 24.5 * column, TWO);
    let runs: Vec<_> = buffer.layout_runs().collect();
    let (from, to) = (cosmic_text::Cursor::new(0, 6), cosmic_text::Cursor::new(0, runs[1].glyphs[3].start));
    let spans: Vec<(f32, f32)> = runs[1].highlight(from, to).collect();
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert!((spans[0].0 - 6.0 * column).abs() < 1e-3, "{spans:?}");
    assert!((spans[0].1 - 3.0 * column).abs() < 1e-3, "{spans:?}");
}

/// Buffers made inside the scope start with the setting; those made after do not
#[test]
fn a_new_buffer_takes_the_setting_of_its_scope() {
    let metrics = Metrics::new(FONT_SIZE, FONT_SIZE);
    assert_eq!(Buffer::new_empty(metrics).hanging_indent(), None);
    let inside = cosmic_text::with_hanging_indent(TWO, || Buffer::new_empty(metrics));
    assert_eq!(inside.hanging_indent(), TWO);
    assert_eq!(inside.clone().hanging_indent(), TWO);
    assert_eq!(Buffer::new_empty(metrics).hanging_indent(), None);
}

/// A justified buffer's rows but the last of each line end at the width, whatever the line's
/// origin (set with the buffer's text or pushed after); turned off, the alignment is cleared
#[test]
fn justified_buffer_justifies_all_its_lines_but_their_last_rows() {
    let mut fs = font_system();
    let width = 24.5 * advance(&mut fs);
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    let mut b = buffer(&mut fs, text, width, None);
    b.set_justified(true);
    b.lines.push(cosmic_text::BufferLine::new(text, cosmic_text::LineEnding::None, cosmic_text::AttrsList::new(&attrs()), Shaping::Advanced));
    b.shape_until_scroll(&mut fs, false);
    let ends: Vec<(usize, f32)> = b.layout_runs().map(|run| (run.line_i, run.glyphs.last().map_or(0.0, |g| g.x + g.w))).collect();
    for line in 0..2 {
        let rows: Vec<f32> = ends.iter().filter(|(i, _)| *i == line).map(|(_, e)| *e).collect();
        assert!(rows.len() >= 2, "{rows:?}");
        assert!(rows[..rows.len() - 1].iter().all(|e| (width - e).abs() < 0.5), "line {line}: {rows:?} against {width}");
        assert!(width - rows[rows.len() - 1] > 1.0, "line {line}'s last row is ragged: {rows:?}");
    }
    b.set_justified(false);
    b.shape_until_scroll(&mut fs, false);
    assert!(b.layout_runs().all(|run| (width - run.glyphs.last().map_or(0.0, |g| g.x + g.w)).abs() > 0.5 || run.glyphs.len() > 100));
    assert!(b.lines.iter().all(|l| l.align().is_none()));
}
