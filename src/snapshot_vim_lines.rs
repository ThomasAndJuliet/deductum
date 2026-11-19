// SPDX-License-Identifier: GPL-3.0-only

//! Vim's operators on whole lines (`dd`, `dj`, `dk`, `cc`, `yk`) in an open cell and in the source
//! view, against what Vim does: the lines as the text has them, however they wrap, into the
//! register; the caret on the first non-blank of the line that took their place. Keys go in as
//! `Message::Vim`, as the key routing sends them.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic::widget::text_editor::Position;
use serde_json::json;

use super::App;
use super::editor::{self, Mode, Theory};
use super::prover::isabelle::symbols::Table;
use super::snapshot::{demo_app, prover_json};

const T: &str = "theory G imports Main begin\n\nlemma foo: \"a = b\"\n  apply simp\n  apply auto\n  by simp\n\nlemma bar: \"True\" by simp\n\nend\n";

fn app(mode: Mode, text: &str) -> App {
    let mut app = demo_app(mode);
    // (the demo's prover state needs a proof in the text; plain lines have none)
    let prover = if text.contains("by simp") {
        prover_json(text)
    } else {
        json!({"revision": 1, "status": {"finished": 0, "unprocessed": 0, "running": 0, "failed": 0, "consolidated": true}, "commands": [], "messages": []})
    };
    let doc = json!({"text": text, "revision": 1, "prover": prover});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/G.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), mode, crate::prover::profile(None)));
    app.vim_enabled = true;
    app.vim.mode = super::vim::Mode::Normal;
    app
}

fn theory(app: &mut App) -> &mut Theory {
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap()
}

fn source(app: &mut App) -> String {
    theory(app).local.clone()
}

/// Open the cell that starts with `start`
fn open(app: &mut App, start: &str) {
    let i = {
        let t = theory(app);
        t.blocks.iter().position(|b| t.local.chars().skip(b.start).take(start.len()).collect::<String>() == start).expect("a cell")
    };
    let _ = cosmic::Application::update(app, super::Message::EditBlock(i));
}

fn content(app: &mut App) -> &mut cosmic::widget::text_editor::Content {
    let t = theory(app);
    match &mut t.block_editor {
        Some(be) if t.mode == Mode::Literate => &mut be.content,
        _ => &mut t.source,
    }
}

/// The caret as (line, column in characters); in normal mode the block caret is a one-character
/// selection, whose start is the caret
fn caret(app: &mut App) -> (usize, usize) {
    let c = content(app).cursor();
    let p = c.selection.filter(|s| (s.line, s.column) < (c.position.line, c.position.column)).unwrap_or(c.position);
    let line = content(app).line(p.line).map(|l| l.text.to_string()).unwrap_or_default();
    (p.line, line[..p.column.min(line.len())].chars().count())
}

fn caret_at(app: &mut App, line: usize, column: usize) {
    let c = content(app);
    let text = c.line(line).map(|l| l.text.to_string()).unwrap_or_default();
    let column = text.char_indices().nth(column).map_or(text.len(), |(b, _)| b);
    editor::place(c, Position { line, column });
}

fn keys(app: &mut App, keys: &str) {
    for key in keys.split(' ') {
        let _ = cosmic::Application::update(app, super::Message::Vim(key.to_string(), false));
    }
}

#[test]
fn dd_on_a_cells_last_line_takes_the_whole_line() {
    let mut app = app(Mode::Literate, T);
    open(&mut app, "lemma foo");
    caret_at(&mut app, 3, 4);
    keys(&mut app, "d d");
    assert_eq!(source(&mut app), T.replace("\n  by simp", ""));
    assert_eq!((app.vim.register.as_str(), app.vim.linewise), ("  by simp\n", true));
    assert_eq!(caret(&mut app), (2, 2), "on the first non-blank of the new last line");
}

#[test]
fn dd_on_the_last_line_of_the_source_takes_the_whole_line() {
    let mut app = app(Mode::Source, "a\nb\n  c");
    caret_at(&mut app, 2, 2);
    keys(&mut app, "d d");
    assert_eq!(source(&mut app), "a\nb");
    assert_eq!(app.vim.register, "  c\n");
    assert_eq!(caret(&mut app), (1, 0));
    keys(&mut app, "p");
    assert_eq!(source(&mut app), "a\nb\n  c", "p puts it back below");
}

#[test]
fn the_empty_line_after_the_final_line_break_is_not_a_line() {
    let mut app = app(Mode::Source, "a\nb\nc\n");
    caret_at(&mut app, 2, 0);
    keys(&mut app, "d d");
    assert_eq!(source(&mut app), "a\nb\n");
    assert_eq!(caret(&mut app), (1, 0), "on the new last line, not after it");
    keys(&mut app, "9 d d");
    assert_eq!(source(&mut app), "a\n");
}

#[test]
fn a_count_past_the_end_takes_the_lines_there_are() {
    let mut app = app(Mode::Literate, T);
    open(&mut app, "lemma foo");
    caret_at(&mut app, 1, 0);
    keys(&mut app, "9 d d");
    assert_eq!(source(&mut app), T.replace("\n  apply simp\n  apply auto\n  by simp", ""));
    assert_eq!(app.vim.register, "  apply simp\n  apply auto\n  by simp\n");
}

#[test]
fn dd_lands_on_the_first_non_blank() {
    let mut app = app(Mode::Literate, T);
    open(&mut app, "lemma foo");
    caret_at(&mut app, 1, 8);
    keys(&mut app, "d d");
    assert_eq!(caret(&mut app), (1, 2));
}

#[test]
fn dk_takes_this_line_and_the_one_above() {
    let mut app = app(Mode::Source, "a\nbbb\nccc\nd\ne\n");
    caret_at(&mut app, 2, 2);
    keys(&mut app, "d k");
    assert_eq!(source(&mut app), "a\nd\ne\n");
    assert_eq!(app.vim.register, "bbb\nccc\n");
    let mut app = self::app(Mode::Literate, T);
    open(&mut app, "lemma foo");
    caret_at(&mut app, 2, 4);
    keys(&mut app, "d k");
    assert_eq!(source(&mut app), T.replace("\n  apply simp\n  apply auto", ""));
}

#[test]
fn dj_and_dk_past_an_end_do_nothing() {
    let mut app = app(Mode::Source, "a\nb\n");
    caret_at(&mut app, 1, 0);
    app.vim.register = "kept".into();
    keys(&mut app, "d j");
    assert_eq!(source(&mut app), "a\nb\n");
    caret_at(&mut app, 0, 0);
    keys(&mut app, "d k");
    assert_eq!(source(&mut app), "a\nb\n");
    assert_eq!(app.vim.register, "kept", "nothing yanked either");
    keys(&mut app, "d j");
    assert_eq!(source(&mut app), "", "both lines, from the first");
}

#[test]
fn dd_on_a_wrapped_line_takes_all_of_it() {
    let long = format!("  have \"{}\" by simp", "x = x \\<and> ".repeat(40));
    let text = format!("theory G imports Main begin\n\nlemma foo: \"a = b\"\n{long}\n  by simp\n\nend\n");
    let mut app = app(Mode::Literate, &text);
    open(&mut app, "lemma foo");
    // (laid out, so that the line wraps into rows)
    let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(700.0, 700.0));
    h.draw(&app, "vim-lines-wrapped");
    caret_at(&mut app, 1, 300);
    keys(&mut app, "d d");
    assert_eq!(source(&mut app), format!("theory G imports Main begin\n\nlemma foo: \"a = b\"\n  by simp\n\nend\n"));
}

#[test]
fn cc_keeps_the_indent_and_starts_inserting() {
    let mut app = app(Mode::Literate, T);
    open(&mut app, "lemma foo");
    caret_at(&mut app, 1, 5);
    keys(&mut app, "c j");
    assert_eq!(source(&mut app), T.replace("  apply simp\n  apply auto\n", "  \n"));
    assert_eq!(app.vim.register, "  apply simp\n  apply auto\n");
    assert_eq!(app.vim.mode, super::vim::Mode::Insert);
    assert_eq!(caret(&mut app), (1, 2));
}

#[test]
fn yk_yanks_this_line_and_the_one_above_and_goes_up() {
    let mut app = app(Mode::Source, "a\n  b\nc\n");
    caret_at(&mut app, 2, 0);
    keys(&mut app, "y k");
    assert_eq!(source(&mut app), "a\n  b\nc\n");
    assert_eq!(app.vim.register, "  b\nc\n");
    assert_eq!(caret(&mut app), (1, 2));
}

#[test]
fn dd_on_a_cells_only_line_yanks_it_and_the_cell_goes() {
    let mut app = app(Mode::Literate, T);
    app.vim.register = "older".into();
    open(&mut app, "lemma bar");
    keys(&mut app, "d d");
    assert_eq!(source(&mut app), T.replace("lemma bar: \"True\" by simp\n\n", ""));
    assert_eq!((app.vim.register.as_str(), app.vim.linewise), ("lemma bar: \"True\" by simp\n", true));
    // (and it can be put back in another cell)
    open(&mut app, "lemma foo");
    keys(&mut app, "p");
    assert!(source(&mut app).contains("lemma foo: \"a = b\"\nlemma bar: \"True\" by simp\n"), "{:?}", source(&mut app));
}

#[test]
fn a_count_over_all_of_a_cells_lines_takes_the_cell() {
    let mut app = app(Mode::Literate, T);
    open(&mut app, "lemma foo");
    keys(&mut app, "4 d d");
    assert_eq!(source(&mut app), "theory G imports Main begin\n\nlemma bar: \"True\" by simp\n\nend\n");
    assert_eq!(app.vim.register, "lemma foo: \"a = b\"\n  apply simp\n  apply auto\n  by simp\n");
}

/// A `d` and then something other than vim keys: the `d` is forgotten, so the next `d` starts a
/// command of its own rather than deleting a line where the caret has gone
#[test]
fn a_click_an_arrow_key_or_another_cell_cancels_a_pending_d() {
    use cosmic::widget::text_editor::{Action, Motion};
    let mut app = app(Mode::Source, "a\nb\nc\n");
    keys(&mut app, "d");
    let _ = cosmic::Application::update(&mut app, super::Message::SourceAction(Action::Click(cosmic::iced::Point::new(10.0, 30.0))));
    keys(&mut app, "d");
    assert_eq!(source(&mut app), "a\nb\nc\n", "a click");
    let mut app = self::app(Mode::Source, "a\nb\nc\n");
    keys(&mut app, "d");
    let _ = cosmic::Application::update(&mut app, super::Message::SourceAction(Action::Move(Motion::Down)));
    keys(&mut app, "d");
    assert_eq!(source(&mut app), "a\nb\nc\n", "an arrow key");
    let mut app = self::app(Mode::Literate, T);
    open(&mut app, "lemma foo");
    keys(&mut app, "d");
    let _ = cosmic::Application::update(&mut app, super::Message::CloseBlock);
    open(&mut app, "lemma bar");
    keys(&mut app, "d");
    assert_eq!(source(&mut app), T, "another cell");
    keys(&mut app, "d");
    assert_eq!(source(&mut app), T.replace("lemma bar: \"True\" by simp\n\n", ""), "and dd there is its own");
}

#[test]
fn d3d_takes_three_lines_as_3dd_does() {
    let mut app = app(Mode::Source, "a\nb\nc\nd\ne\n");
    caret_at(&mut app, 1, 0);
    keys(&mut app, "d 3 d");
    assert_eq!(source(&mut app), "a\ne\n");
    assert_eq!(app.vim.register, "b\nc\nd\n");
}

#[test]
fn o_on_a_wrapped_line_opens_a_line_after_all_of_it() {
    let long = format!("  have \"{}\" by simp", "x = x \\<and> ".repeat(40));
    let text = format!("theory G imports Main begin\n\nlemma foo: \"a = b\"\n{long}\n  by simp\n\nend\n");
    let mut app = app(Mode::Literate, &text);
    open(&mut app, "lemma foo");
    let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(700.0, 700.0));
    h.draw(&app, "vim-lines-wrapped-o");
    caret_at(&mut app, 1, 10);
    keys(&mut app, "o");
    assert_eq!(source(&mut app), format!("theory G imports Main begin\n\nlemma foo: \"a = b\"\n{long}\n  \n  by simp\n\nend\n"));
}
