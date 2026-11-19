// SPDX-License-Identifier: GPL-3.0-only

//! Indentation as it is typed: Tab, Shift+Tab and Enter through the editor's own key bindings, in
//! an open cell and in the source view, Isabelle and Lean, headless. In the texts `§` is the caret,
//! `⟪…⟫` a selection with the caret at its end and `⟬…⟭` one with the caret at its start. Each case
//! is checked in both views, and an open cell's text is checked to be in the source.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic::iced::Size;
use cosmic::iced::keyboard::{Key, Modifiers, key::Named};
use cosmic::widget::text_editor::Cursor;
use serde_json::json;

use super::editor::{LineIndex, Mode, Theory};
use super::prover::isabelle::symbols::Table;
use super::snapshot::{Harness, demo_app};
use super::{App, Message};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Be { Isabelle, Lean }

const ISA_HEAD: &str = "theory T\n  imports Main\nbegin\n\n";
const ISA_TAIL: &str = "\n\nend\n";
const LEAN_HEAD: &str = "import Mathlib\n\n";
const LEAN_TAIL: &str = "\n";

struct Marked { text: String, anchor: usize, head: usize }

fn parse(marked: &str) -> Marked {
    let (mut text, mut anchor, mut head) = (String::new(), None, None);
    let mut n = 0;
    for c in marked.chars() {
        match c {
            '§' => { anchor = Some(n); head = Some(n); }
            '⟪' | '⟭' => anchor = Some(n),
            '⟫' | '⟬' => head = Some(n),
            _ => { text.push(c); n += 1; }
        }
    }
    Marked { text, anchor: anchor.expect("a marker"), head: head.expect("a marker") }
}

fn render(text: &str, anchor: usize, head: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    for i in 0..=chars.len() {
        if anchor == head {
            if i == head { out.push('§'); }
        } else if anchor < head {
            if i == anchor { out.push('⟪'); }
            if i == head { out.push('⟫'); }
        } else {
            if i == head { out.push('⟬'); }
            if i == anchor { out.push('⟭'); }
        }
        if i < chars.len() { out.push(chars[i]); }
    }
    out
}

struct Rig { app: App, h: Harness, mode: Mode, be: Be, base: usize }

impl Rig {
    fn new(be: Be, mode: Mode, marked: &str) -> Rig { Rig::wrapped(be, mode, marked, 0, 0) }

    /// (a prose cell shows the text inside `skip` leading and `skip_end` trailing characters)
    fn wrapped(be: Be, mode: Mode, marked: &str, skip: usize, skip_end: usize) -> Rig {
        let m = parse(marked);
        let (head, tail) = if be == Be::Isabelle { (ISA_HEAD, ISA_TAIL) } else { (LEAN_HEAD, LEAN_TAIL) };
        // (the source holds what the probe's text says; the editor shows Isabelle's symbols decoded)
        let full = format!("{head}{}{tail}", m.text);
        let mut app = match be {
            Be::Isabelle => isabelle_app(mode, &full),
            Be::Lean => super::snapshot_lean::lean_app_with(mode, &full),
        };
        let mut h = Harness::new(Size::new(1100.0, 800.0));
        if mode == Mode::Literate {
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            let first = m.text.lines().next().unwrap().trim_start();
            let i = t.blocks.iter().position(|b| t.source_text(b.start, b.end).contains(first)).unwrap_or_else(|| panic!("a block with {first:?}: {:?}", t.blocks.iter().map(|b| t.source_text(b.start, b.end)).collect::<Vec<_>>()));
            let _ = cosmic::Application::update(&mut app, Message::EditBlock(i));
            h.focus(&app, super::view::block_editor_id());
        } else {
            h.focus(&app, super::view::source_editor_id());
        }
        h.frame(&app);
        let mut rig = Rig { app, h, mode, be, base: 0 };
        // (marked text is in source form; the editor's is decoded: find where it starts)
        let table = Table::fallback();
        let decoded_body = if be == Be::Isabelle { table.decode(&m.text).text } else { m.text.clone() };
        let inner: String = { let c: Vec<char> = decoded_body.chars().collect(); c[skip..c.len() - skip_end].iter().collect() };
        let text = rig.editor_text();
        let at = text.find(inner.as_str()).unwrap_or_else(|| panic!("{decoded_body:?} in the editor's {text:?}"));
        rig.base = text[..at].chars().count();
        // (offsets of the markers, in the decoded body)
        let dec = |n: usize| -> usize {
            if be == Be::Isabelle {
                let upto: String = m.text.chars().take(n).collect();
                table.decode(&upto).text.chars().count()
            } else { n }
        };
        let (a, hd) = (dec(m.anchor), dec(m.head));
        rig.set(a.saturating_sub(skip) + rig.base, hd.saturating_sub(skip) + rig.base);
        rig
    }

    fn theory(&self) -> &Theory { self.app.workspace.as_ref().unwrap().theory.as_ref().unwrap() }

    fn content(&self) -> &cosmic::widget::text_editor::Content {
        let t = self.theory();
        if self.mode == Mode::Literate { &t.block_editor.as_ref().unwrap().content } else { &t.source }
    }

    fn editor_text(&self) -> String { self.content().text() }

    fn set(&mut self, anchor: usize, head: usize) {
        let text = self.editor_text();
        let li = LineIndex::new(&text);
        let (a, hd) = (li.position(&text, anchor), li.position(&text, head));
        let lit = self.mode == Mode::Literate;
        let t = self.app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let c = if lit { &mut t.block_editor.as_mut().unwrap().content } else { &mut t.source };
        c.move_to(Cursor { position: hd, selection: if a == hd { None } else { Some(a) } });
        if lit { t.sync_block_cursor(); }
    }

    /// The editor's text from the probe's start to before the document's footer, with the caret
    /// and selection marked
    fn shown(&self) -> String {
        let text = self.editor_text();
        let li = LineIndex::new(&text);
        let c = self.content().cursor();
        let head = li.offset(&text, c.position);
        let anchor = c.selection.map_or(head, |s| li.offset(&text, s));
        let chars: Vec<char> = text.chars().collect();
        let tail = if self.mode == Mode::Source { if self.be == Be::Isabelle { ISA_TAIL } else { LEAN_TAIL } } else { "" };
        let end = if text.ends_with(tail) { chars.len() - tail.chars().count() } else { chars.len() };
        let body: String = chars[self.base..end.max(self.base)].iter().collect();
        let (a, hh) = (anchor.saturating_sub(self.base), head.saturating_sub(self.base));
        render(&body, a, hh).trim_end_matches('\n').to_string()
    }

    fn txn_state(&self) -> (String, usize) { let t = self.theory(); (t.txn.clone(), t.steps.len()) }

    fn apply(&mut self, messages: Vec<Message>) {
        for m in messages {
            let task = cosmic::Application::update(&mut self.app, m);
            self.h.run_task_with_updates(&mut self.app, task);
        }
        self.h.frame(&self.app);
    }

    fn press(&mut self, key: Key, text: Option<&str>, mods: Modifiers) {
        let msgs = self.h.key_with(&self.app, key, text, mods);
        self.apply(msgs);
    }
    fn enter(&mut self) { self.press(Key::Named(Named::Enter), None, Modifiers::empty()) }
    fn tab(&mut self) { self.press(Key::Named(Named::Tab), None, Modifiers::empty()) }
    fn shift_tab(&mut self) { self.press(Key::Named(Named::Tab), None, Modifiers::SHIFT) }
    fn type_text(&mut self, s: &str) {
        let msgs = self.h.type_text(&self.app, s);
        self.apply(msgs);
    }

    /// In the literate view: the source carries what the cell shows (written back)
    fn written_back(&self) -> Result<(), String> {
        if self.mode != Mode::Literate { return Ok(()); }
        let t = self.theory();
        let shown = t.block_editor.as_ref().unwrap().content.text();
        let shown = shown.trim_end();
        let raw = &t.local;
        let table = Table::fallback();
        let decoded = if self.be == Be::Isabelle { table.decode(raw).text } else { raw.clone() };
        if !decoded.contains(shown) {
            return Err(format!("source {decoded:?} lacks the cell's {shown:?}"));
        }
        Ok(())
    }
}

fn isabelle_app(mode: Mode, text: &str) -> App {
    let mut app = demo_app(mode);
    let table = Arc::new(Table::fallback());
    let cmds = super::prover::isabelle::blocks::lexical_commands(text, &HashMap::new());
    let commands: Vec<serde_json::Value> = cmds.iter().map(|c| json!([c.start, c.end, c.name, "finished"])).collect();
    let doc = json!({"text": text, "revision": 1, "dirty": false, "can_undo": true, "prover": {
        "revision": 1, "status": {"finished": 14, "unprocessed": 0, "running": 0, "failed": 0, "consolidated": true},
        "commands": commands, "messages": []}});
    let theory = Theory::new("/home/me/Demo/Demo.thy".into(), &doc, &*table, Arc::new(HashMap::new()), mode, crate::prover::profile(None));
    app.workspace.as_mut().unwrap().theory = Some(theory);
    app
}

#[derive(Clone, Copy)]
enum K { Enter, Tab, ShiftTab, Type(&'static str) }

/// Run `keys` from `start`; the result must be `expect`; the cell writes it back; `steps`, when
/// given, is how many undo steps the keys begin
fn run(be: Be, mode: Mode, start: &str, keys: &[K], expect: &str, steps: Option<usize>) {
    let mut r = Rig::new(be, mode, start);
    let shown_form = |s: &str| if be == Be::Isabelle { Table::fallback().decode(s).text } else { s.to_string() };
    let (start_d, expect_d) = (shown_form(start), shown_form(expect));
    assert_eq!(r.shown(), start_d.trim_end_matches('\n'), "setup: the caret is where the probe says");
    let s0 = r.txn_state();
    for k in keys {
        match k {
            K::Enter => r.enter(),
            K::Tab => r.tab(),
            K::ShiftTab => r.shift_tab(),
            K::Type(s) => r.type_text(s),
        }
    }
    let got = r.shown();
    let mut problems = Vec::new();
    if got != expect_d.trim_end_matches('\n') { problems.push(format!("text/caret differ\n  start : {start_d:?}\n  expect: {expect_d:?}\n  got   : {got:?}")); }
    if let Err(e) = r.written_back() { problems.push(e); }
    if let Some(n) = steps {
        let s1 = r.txn_state();
        let began = if s1.0 != s0.0 { s1.1 - s0.1 + 1 } else { s1.1 - s0.1 };
        if began != n { problems.push(format!("undo steps begun: {began}, expected {n} (steps {} -> {}, txn changed {})", s0.1, s1.1, s1.0 != s0.0)); }
    }
    assert!(problems.is_empty(), "{be:?}/{mode:?}: {}", problems.join("\n"));
}

macro_rules! both {
    ($name:ident, $be:expr, $start:expr, $keys:expr, $expect:expr) => {
        mod $name {
            use super::*;
            #[test] fn cell() { run($be, Mode::Literate, $start, $keys, $expect, None) }
            #[test] fn source() { run($be, Mode::Source, $start, $keys, $expect, None) }
        }
    };
}

// ----- Tab and Shift+Tab with no selection (an indent is 2 columns)

both!(tab_at_the_start_of_a_line_indents_it, Be::Isabelle, "lemma a: \"P\"\n§apply simp", &[K::Tab], "lemma a: \"P\"\n  §apply simp");
both!(tab_goes_to_the_next_tab_stop, Be::Isabelle, "lemma a: \"P\"\n §apply simp", &[K::Tab], "lemma a: \"P\"\n  §apply simp");
both!(tab_in_text_goes_to_the_next_tab_stop, Be::Lean, "theorem t : P := by\n  i§ntro h", &[K::Tab], "theorem t : P := by\n  i §ntro h");
both!(shift_tab_keeps_the_caret_on_its_text, Be::Isabelle, "lemma a: \"P\"\n    app§ly simp", &[K::ShiftTab], "lemma a: \"P\"\n  app§ly simp");
both!(shift_tab_with_the_caret_in_the_indent, Be::Isabelle, "lemma a: \"P\"\n  §  apply simp", &[K::ShiftTab], "lemma a: \"P\"\n§  apply simp");
both!(shift_tab_takes_less_than_an_indent, Be::Lean, "theorem t : P := by\n intro h§", &[K::ShiftTab], "theorem t : P := by\nintro h§");
both!(shift_tab_takes_a_tab, Be::Isabelle, "lemma a: \"P\"\n\tapply simp§", &[K::ShiftTab], "lemma a: \"P\"\napply simp§");
both!(shift_tab_on_a_line_with_symbols, Be::Isabelle, "lemma a: \"P\"\n    \\<forall>x. x = §x", &[K::ShiftTab], "lemma a: \"P\"\n  \\<forall>x. x = §x");

// ----- Tab and Shift+Tab with a selection: every line it is on, and the selection kept

both!(tab_indents_the_selected_lines, Be::Isabelle, "lemma a: \"P\"\n  app⟪ly simp\n  apply a⟫uto\n  done", &[K::Tab], "lemma a: \"P\"\n    app⟪ly simp\n    apply a⟫uto\n  done");
both!(tab_indents_the_lines_of_a_backward_selection, Be::Lean, "theorem t : P := by\n  ⟬intro h\n  exact h⟭", &[K::Tab], "theorem t : P := by\n    ⟬intro h\n    exact h⟭");
both!(a_selection_ending_at_a_lines_start_leaves_that_line, Be::Isabelle, "lemma a: \"P\"\n⟪  apply simp\n  apply auto\n⟫  done", &[K::Tab], "lemma a: \"P\"\n⟪    apply simp\n    apply auto\n⟫  done");
both!(tab_leaves_blank_lines_blank, Be::Isabelle, "lemma a: \"P\"\n  ⟪apply simp\n\n  apply auto⟫", &[K::Tab], "lemma a: \"P\"\n    ⟪apply simp\n\n    apply auto⟫");
both!(tab_with_part_of_a_line_selected_indents_the_line, Be::Isabelle, "lemma a: \"P\"\n  apply ⟪si⟫mp", &[K::Tab], "lemma a: \"P\"\n    apply ⟪si⟫mp");
both!(shift_tab_unindents_the_selected_lines, Be::Isabelle, "lemma a: \"P\"\n⟪ apply simp\n  apply auto\n    done⟫", &[K::ShiftTab], "lemma a: \"P\"\n⟪apply simp\napply auto\n  done⟫");
both!(shift_tab_unindents_the_lines_of_a_backward_selection, Be::Isabelle, "lemma a: \"P\"\n    ⟬apply simp\n    apply auto⟭", &[K::ShiftTab], "lemma a: \"P\"\n  ⟬apply simp\n  apply auto⟭");

#[test]
fn indenting_lines_is_one_undo_step() {
    run(Be::Isabelle, Mode::Source, "lemma a: \"P\"\n  ⟪apply simp\n  apply auto⟫", &[K::Tab], "lemma a: \"P\"\n    ⟪apply simp\n    apply auto⟫", Some(1));
    run(Be::Isabelle, Mode::Source, "lemma a: \"P\"\n    ⟪apply simp\n    apply auto⟫", &[K::ShiftTab], "lemma a: \"P\"\n  ⟪apply simp\n  apply auto⟫", Some(1));
}

#[test]
fn typing_a_tab_and_typing_again_are_three_undo_steps() {
    run(Be::Isabelle, Mode::Source, "lemma a: \"P\"\n  apply simp§", &[K::Type("x"), K::Tab, K::Type("y")], "lemma a: \"P\"\n  apply simpx y§", Some(3));
}

// ----- Enter

both!(enter_keeps_the_indent, Be::Isabelle, "lemma a: \"P\"\n  apply simp§", &[K::Enter], "lemma a: \"P\"\n  apply simp\n  §");
both!(enter_after_proof_steps_in, Be::Isabelle, "lemma a: \"P\"\nproof§", &[K::Enter], "lemma a: \"P\"\nproof\n  §");
both!(enter_after_by_steps_in, Be::Lean, "theorem t : P := by§", &[K::Enter], "theorem t : P := by\n  §");
