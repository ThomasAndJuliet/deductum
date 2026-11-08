// SPDX-License-Identifier: GPL-3.0-only

//! The Lean profile through the real window, headless (see `snapshot.rs`): a document modelled on
//! DSRVLean in the literate and source views, dark and light, narrow and wide, with the goal
//! beside and without it; Lean's goal view; docstrings with maths and a ```lean block; the
//! palette; the Search panel; and a `sorry` and an error. Set `CT_SNAPSHOTS` to keep the frames.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic::iced::Size;
use serde_json::json;

use super::editor::{Mode, Theory};
use super::prover::Profile;
use super::prover::lean::demo::{DEMO, goal_parts, status};
use super::snapshot::{Harness, demo_app};
use super::{App, Panel, QueryState};

const PATH: &str = "/home/me/DSRVLean/DSRV/Demo.lean";

/// The Lean document as the host would send it: lexical commands with statuses, a warning on the
/// `sorry`, an error on the `omega` that cannot prove `n < n`
fn doc_json(text: &str) -> serde_json::Value {
    let chars: Vec<char> = text.chars().collect();
    let cmds = super::prover::LEAN.lexical_commands(text, &HashMap::new());
    let commands: Vec<serde_json::Value> = cmds.iter().map(|c| json!([c.start, c.end, c.name, status(text, c.start, &c.name)])).collect();
    let at = |needle: &str| text.rfind(needle).map(|b| text[..b].chars().count());
    let _ = &chars;
    let mut messages = Vec::new();
    if let Some(sorry) = at("sorry") {
        messages.push(json!({"start": sorry, "end": sorry + 5, "kind": "warning", "text": "declaration uses 'sorry'"}));
    }
    if let Some(omega) = at("omega") {
        messages.push(json!({"start": omega, "end": omega + 5, "kind": "error", "text": "omega could not prove the goal:\nNo usable constraints found. You may need to unfold definitions so `omega` can see linear arithmetic facts about `Nat` and `Int`, which may also involve multiplication, division, and modular remainder by constants."}));
    }
    json!({
        "text": text, "revision": 1, "dirty": false, "can_undo": true,
        "prover": {
            "revision": 1,
            "status": {"finished": 20, "unprocessed": 0, "running": 0, "failed": 1, "consolidated": true},
            "commands": commands,
            "messages": messages,
        },
    })
}

/// The demo window on a Lean session
pub fn lean_app(mode: Mode) -> App {
    lean_app_with(mode, DEMO)
}

/// The demo's imports and its proofs, without the prose before them: the first proof is near the
/// top of the page, so that a picture of it shows the goal beside
fn proofs_only() -> String {
    let from = DEMO.find("theorem append_nil_right").unwrap();
    format!("import DSRV.Semantics\n\nnamespace DSRV.Demo\n\n{}", &DEMO[from..])
}

pub fn lean_app_with(mode: Mode, text: &str) -> App {
    super::prover::register_highlighters();
    let mut app = demo_app(mode);
    let symbols = super::prover::LEAN.symbols(None);
    let doc = doc_json(text);
    let theory = Theory::new(PATH.into(), &doc, &*symbols, Arc::new(HashMap::new()), mode, &super::prover::LEAN);
    for ((_, sid), s) in app.sessions.iter_mut() {
        if sid == "s1" {
            s["name"] = json!("DSRVLean");
            s["folder"] = json!("/home/me/DSRVLean");
            s["backend"] = json!("lean");
            s["logic"] = json!("v4.19.0");
            s["grants"] = json!([]);
        }
    }
    let ws = app.workspace.as_mut().unwrap();
    ws.profile = &super::prover::LEAN;
    ws.caps = super::prover::LEAN.default_capabilities();
    ws.theory = Some(theory);
    ws.info = Some(json!({"folder": "/home/me/DSRVLean", "theories": [{"path": PATH, "status": {"failed": 1}, "dirty": false}], "seen": {}}));
    ws.panel = Panel::Output;
    ws.notes = Vec::new();
    ws.history = Vec::new();
    ws.activity = None;
    ws.output = Some(json!({"command": {"name": "induction", "status": "finished"}, "messages": []}));
    ws.fill = Default::default();
    ws.goals = QueryState {
        operation: "deductum_goals",
        status: "finished".into(),
        messages: goal_parts(&[
            ("nil", &["xs : List Nat"], "[] ++ [] = []"),
            ("cons", &["x : Nat", "xs : List Nat", "ih : xs ++ [] = xs"], "x :: xs ++ [] = x :: xs"),
        ]),
        ..Default::default()
    };
    ws.goals_prev = QueryState { operation: "deductum_goals", ..Default::default() };
    app.symbols_for_tests(symbols);
    app
}

impl App {
    /// The symbols of the demo machine are Lean's
    fn symbols_for_tests(&mut self, symbols: Arc<dyn super::codec::Symbols>) {
        for l in &mut self.links {
            l.symbols = Some(symbols.clone());
        }
    }
}

fn theory(app: &App) -> &Theory {
    app.workspace.as_ref().unwrap().theory.as_ref().unwrap()
}

fn block_named(app: &App, needle: &str) -> usize {
    let t = theory(app);
    t.blocks.iter().position(|b| t.local.chars().skip(b.start).take(b.end - b.start).collect::<String>().contains(needle)).unwrap_or_else(|| panic!("no block with {needle:?}"))
}

fn select(app: &mut App, needle: &str, caret_in: &str) {
    let i = block_named(app, needle);
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    t.selected = Some(i);
    let at = t.local.find(caret_in).map(|b| t.local[..b].chars().count()).unwrap();
    t.cursor = at + 1;
}

fn update(app: &mut App, m: super::Message) {
    let _ = cosmic::Application::update(app, m);
}

/// (tests hold the UI's turn through their Harness before changing the theme or toggles)
fn light(on: bool) {
    cosmic::theme::set_active(if on { cosmic::theme::ThemeType::Light } else { cosmic::theme::ThemeType::Dark });
}

/// The pixels differing from the background in a region: something was drawn there
fn ink(png: &[u8], w: u32, h: u32, x: std::ops::Range<u32>, y: std::ops::Range<u32>) -> usize {
    let img = image::RgbaImage::from_raw(w, h, png.to_vec()).unwrap();
    let bg = *img.get_pixel(w - 3, h - 3);
    x.flat_map(|x| y.clone().map(move |y| (x, y))).filter(|(x, y)| img.get_pixel(*x, *y) != &bg).count()
}

#[test]
fn the_lean_document_reads_as_blocks() {
    let app = lean_app(Mode::Literate);
    let t = theory(&app);
    assert_eq!(t.profile.backend(), "lean");
    let kinds: Vec<_> = t.blocks.iter().map(|b| b.kind).collect();
    assert_eq!(kinds[0], super::theory::Kind::Formal, "imports are code");
    assert!(kinds.contains(&super::theory::Kind::Text));
    assert!(t.blocks.iter().any(|b| b.name == "theorem" && b.status == super::theory::Status::Failed), "the broken theorem failed");
    assert!(t.blocks.iter().any(|b| b.name == "theorem" && b.status == super::theory::Status::Warned), "the sorry warns");
    // the docstring is prose above its structure
    let i = block_named(&app, "structure Origin");
    assert_eq!(t.blocks[i - 1].kind, super::theory::Kind::Text);
    assert!(t.docs.get(i - 1).is_some_and(Option::is_some), "the docstring is rendered");
}

#[test]
fn draws_lean_in_both_views_both_themes_and_two_widths() {
    for (mode, mode_name) in [(Mode::Literate, "literate"), (Mode::Source, "source")] {
        for dark in [true, false] {
            for (w, h) in [(800.0, 760.0), (1400.0, 900.0)] {
                for beside in [true, false] {
                    let mut harness = Harness::new(Size::new(w, h));
                    light(!dark);
                    super::view::set_goal_beside(beside);
                    let mut app = lean_app_with(mode, &proofs_only());
                    app.show_sidebar = false;
                    app.show_right = false;
                    app.window_width = w;
                    select(&mut app, "theorem append_nil_right", "induction");
                    if mode == Mode::Source && !beside {
                        super::view::set_goal_beside(true);
                        light(false);
                        continue;
                    }
                    let name = format!("lean-{mode_name}-{}-{}-{}", if dark { "dark" } else { "light" }, w as u32, if beside { "beside" } else { "under" });
                    let png = harness.draw(&app, &name);
                    assert!(ink(&png, w as u32, h as u32, 20..(w as u32 - 20), 40..(h as u32 - 20)) > 2000, "{name} is not blank");
                    light(false);
                    super::view::set_goal_beside(true);
                }
            }
        }
    }
}

#[test]
fn the_goal_view_shows_case_tags_hypotheses_and_the_target() {
    let mut app = lean_app(Mode::Literate);
    select(&mut app, "theorem append_nil_right", "induction");
    app.workspace.as_mut().unwrap().panel = Panel::Output;
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    let png = h.draw(&app, "lean-goal-view");
    assert!(ink(&png, 1400, 900, 980..1390, 80..700) > 3000, "the goal panel on the right has content");
    // the goal text as the host sends it, in the proof state's own Output
    let ws = app.workspace.as_ref().unwrap();
    let goals = super::goals::Goals::parse(&ws.goals.messages);
    assert_eq!(goals.subgoals.len(), 2);
    let shown = ws.profile.present_goal(&goals.subgoals[1]);
    assert_eq!(shown.heading.as_deref(), Some("case cons"));
}

#[test]
fn a_docstring_with_maths_and_a_lean_block() {
    let mut app = lean_app(Mode::Literate);
    app.show_sidebar = false;
    app.show_right = false;
    let i = block_named(&app, "A variable observation");
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().selected = Some(i);
    let mut h = Harness::new(Size::new(1100.0, 900.0));
    let png = h.draw(&app, "lean-docstring");
    assert!(ink(&png, 1100, 900, 100..1000, 60..400) > 2000);
    // opened for editing: the paragraph as source
    update(&mut app, super::Message::EditBlock(i));
    h.draw(&app, "lean-docstring-open");
    let md = theory(&app).docs.get(i).cloned().flatten();
    assert!(md.is_some());
}

#[test]
fn the_palette_in_a_proof_and_in_prose() {
    let mut app = lean_app_with(Mode::Literate, &(proofs_only().replace("theorem later", "/-! Some *prose* with $x$. -/\n\ntheorem later")));
    app.show_sidebar = false;
    app.show_right = false;
    let proof = block_named(&app, "theorem and_swap'");
    update(&mut app, super::Message::EditBlock(proof));
    let ctx = |app: &App| super::insert::insert_context(theory(app));
    // (at a tactic's start: the second line's indent, before `constructor`)
    {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        super::editor::place(&mut be.content, cosmic::widget::text_editor::Position { line: 1, column: 2 });
    }
    assert_eq!(ctx(&app), Some(super::prover::InsertContext::Proof));
    // (the palette takes the goal's place while there is none)
    app.workspace.as_mut().unwrap().goals = QueryState { operation: "deductum_goals", ..Default::default() };
    app.workspace.as_mut().unwrap().output = None;
    let mut h = Harness::new(Size::new(1320.0, 800.0));
    {
        use cosmic::widget::text_editor::{Action, Motion};
        update(&mut app, super::Message::BlockAction(Action::Move(Motion::DocumentEnd)));
    }
    // (just after the last tactic: a term, the tactic's argument being what may be typed there)
    assert_eq!(ctx(&app), Some(super::prover::InsertContext::Symbols));
    h.draw(&app, "lean-palette-proof");
    update(&mut app, super::Message::Wrap("\n  exact?".into(), String::new()));
    assert!(theory(&app).local.contains("exact hp\n  exact?"), "inserted at the caret: {:?}", theory(&app).local);
    let prose = block_named(&app, "Some *prose*");
    update(&mut app, super::Message::EditBlock(prose));
    assert_eq!(ctx(&app), Some(super::prover::InsertContext::Prose));
    h.draw(&app, "lean-palette-prose");
}

#[test]
fn the_search_panel_lists_hits_with_insert() {
    let mut app = lean_app(Mode::Literate);
    {
        let ws = app.workspace.as_mut().unwrap();
        ws.panel = Panel::Search;
        ws.find_input = "?a + ?b = ?b + ?a".into();
        ws.find = QueryState {
            operation: "find_theorems",
            status: "finished".into(),
            messages: vec![
                json!({"kind": "writeln", "text": "3 declarations from loogle.lean-lang.org", "raw": "", "spans": [], "sendbacks": []}),
                json!({"kind": "writeln", "text": "Nat.add_comm : ∀ (n m : ℕ), n + m = m + n\nInit.Core", "name": "Nat.add_comm", "type": "∀ (n m : ℕ), n + m = m + n", "module": "Init.Core", "sendbacks": ["Nat.add_comm"], "spans": []}),
                json!({"kind": "writeln", "text": "", "name": "Int.add_comm", "type": "∀ (a b : ℤ), a + b = b + a", "module": "Init.Data.Int.Basic", "sendbacks": ["Int.add_comm"], "spans": []}),
                json!({"kind": "writeln", "text": "", "name": "Nat.add_comm'", "type": "∀ (n m : ℕ), m + n = n + m", "module": "Mathlib.Algebra.Group.Nat", "sendbacks": ["Nat.add_comm'"], "spans": []}),
            ],
            ..Default::default()
        };
    }
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    let png = h.draw(&app, "lean-search");
    assert!(ink(&png, 1400, 900, 980..1390, 120..500) > 3000);
    // a hit's Insert puts its name at the caret
    select(&mut app, "theorem and_swap'", "constructor");
    update(&mut app, super::Message::Wrap("Nat.add_comm".into(), String::new()));
    assert!(theory(&app).local.contains("Nat.add_comm"));
    // a failed search says why
    app.workspace.as_mut().unwrap().find = QueryState { operation: "find_theorems", status: "failed".into(), error: Some("loogle.lean-lang.org could not be reached (no route)".into()), ..Default::default() };
    h.draw(&app, "lean-search-error");
    // (the source follows the settings, which other tests may change meanwhile)
    assert_eq!(app.workspace.as_ref().unwrap().loogle_source(), super::settings::loogle().as_str());
}

#[test]
fn a_sorry_warns_and_an_error_is_shown_under_its_proof() {
    let mut app = lean_app_with(Mode::Literate, &proofs_only());
    app.show_sidebar = false;
    app.show_right = false;
    select(&mut app, "theorem broken", "omega");
    let mut h = Harness::new(Size::new(1100.0, 900.0));
    let png = h.draw(&app, "lean-sorry-and-error");
    assert!(ink(&png, 1100, 900, 100..1000, 300..880) > 2000);
    let ws = app.workspace.as_ref().unwrap();
    let t = ws.theory.as_ref().unwrap();
    assert!(t.prover.messages.iter().any(|m| m.kind == "warning") && t.prover.messages.iter().any(|m| m.kind == "error"));
}

#[test]
fn typing_a_backslash_abbreviation_offers_the_character() {
    let app = lean_app(Mode::Source);
    let symbols = super::prover::LEAN.symbols(None);
    let t = theory(&app);
    let c = super::editor::complete(t.profile, "theorem t : p \\to", &*symbols, &HashMap::new(), &[], None);
    assert_eq!(c[0].text, "→", "{c:?}");
    assert_eq!(c[0].replace, 3);
    // the punctuation abbreviations: `\<` is ⟨ (not the start of an Isabelle name), `\.` is ·
    let c = super::editor::complete(t.profile, "exact \\<", &*symbols, &HashMap::new(), &[], None);
    assert_eq!(c[0].text, "⟨", "{c:?}");
    assert_eq!(c[0].replace, 2);
    let c = super::editor::complete(t.profile, "  \\.", &*symbols, &HashMap::new(), &[], None);
    assert_eq!(c[0].text, "·", "{c:?}");
    // Isabelle keeps its `\<name>` form
    let table = super::prover::isabelle::symbols::Table::fallback();
    let c = super::editor::complete(&super::prover::ISABELLE, "lemma \"\\<fora", &table, &HashMap::new(), &[], None);
    assert_eq!(c[0].text, "∀", "{c:?}");
    let c = super::editor::complete(t.profile, "  intro h\n  cases h with | inl h => ex", &*symbols, &HashMap::new(), &["exact".to_string()], None);
    assert!(c.iter().any(|c| c.text == "exact"), "words of the file");
}

#[test]
fn clicking_a_lean_paragraph_puts_the_caret_there() {
    let size = Size::new(1400.0, 900.0);
    let click_at = |x: f32| {
        let mut app = lean_app(Mode::Literate);
        app.show_sidebar = false;
        app.show_right = false;
        let mut h = Harness::new(size);
        h.draw(&app, "lean-prose-click-0");
        let msgs = h.click(&app, cosmic::iced::Point::new(x, 153.0));
        let opened = msgs.iter().any(|m| matches!(m, super::Message::EditBlock(_)));
        for m in msgs {
            let _ = cosmic::Application::update(&mut app, m);
        }
        (app, opened)
    };
    // The page's text column, by where a click opens the block
    let probe: Vec<i32> = (20..1200).step_by(20).filter(|x| click_at(*x as f32).1).collect();
    assert!(probe.len() > 6, "the paragraph is under the pointer somewhere: {probe:?}");
    // (the first half of the column: Paper sets the closed text justified, and the editor opened by
    // the click sets it ragged, so a click right of the editor's first row lands at its end)
    let lo = probe[1] as f32;
    let hi = lo + (probe[probe.len() - 2] as f32 - lo) / 2.0;
    // Clicks along the module doc's first line of text: the caret goes to the word clicked
    let mut results = Vec::new();
    for x in [lo, lo + (hi - lo) / 3.0, lo + 2.0 * (hi - lo) / 3.0, hi] {
        let (app, _) = click_at(x);
        let be = theory(&app).block_editor.as_ref().expect("open");
        let text = be.content.text();
        let li = super::editor::LineIndex::new(&text);
        let at = li.offset(&text, be.content.cursor().position);
        assert!(text.starts_with("Every observation"), "the paragraph clicked: {text:?}");
        let word: String = text.chars().skip(at).take_while(|c| c.is_alphanumeric()).collect();
        results.push((x, at, word));
    }
    println!("{results:?}");
    assert!(results.windows(2).all(|w| w[0].1 < w[1].1), "{results:?}");
    assert!(results[0].1 < 12 && results[3].1 > 30, "{results:?}");
}

#[test]
fn a_bracket_abbreviation_puts_the_caret_between() {
    let mut app = lean_app(Mode::Source);
    let symbols = super::prover::LEAN.symbols(None);
    let c = super::editor::complete(&super::prover::LEAN, "exact \\<>", &*symbols, &HashMap::new(), &[], None);
    assert_eq!((c[0].text.as_str(), c[0].back, c[0].replace), ("⟨⟩", 1, 3), "{c:?}");
    let c = super::editor::complete(&super::prover::LEAN, "x \\floor", &*symbols, &HashMap::new(), &[], None);
    assert_eq!((c[0].text.as_str(), c[0].back), ("⌊⌋", 1), "{c:?}");
    let c = super::editor::complete(&super::prover::LEAN, "x \\ennreal", &*symbols, &HashMap::new(), &[], None);
    assert_eq!((c[0].text.as_str(), c[0].back), ("ℝ≥0∞", 0), "{c:?}");
    // accepted in the source, the caret is between the brackets
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let at = t.local.find("exact hq").map(|b| t.local[..b].chars().count()).unwrap_or(t.local.chars().count());
    let text = t.decoded.text.clone();
    let c = super::editor::complete(&super::prover::LEAN, "exact \\<>", &*symbols, &HashMap::new(), &[], None);
    // (insert the typed abbreviation, then accept)
    t.insert(at, "\\<>", &*symbols);
    let pos = t.lines.position(&t.decoded.text.clone(), at + 3);
    super::editor::place(&mut t.source, pos);
    t.completions = c;
    t.accept_completion(0, &*symbols);
    assert!(t.local.contains("⟨⟩") && !t.local.contains("\\<>"), "{:?}", &t.local[..200.min(t.local.len())]);
    let _ = text;
    let caret = t.source.cursor().position;
    let line: String = t.decoded.text.lines().nth(caret.line).unwrap().to_string();
    assert_eq!(&line[..caret.column].chars().last(), &Some('⟨'), "the caret sits after ⟨: {line:?} col {}", caret.column);
}

#[test]
fn unsolved_goals_are_summarised_under_the_proof() {
    let mut app = lean_app_with(Mode::Literate, &proofs_only());
    app.show_sidebar = false;
    app.show_right = false;
    {
        let symbols = super::prover::LEAN.symbols(None);
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let at = t.local.find("constructor").map(|b| t.local[..b].chars().count()).unwrap();
        t.prover.messages.push(super::editor::ProverMessage {
            start: at,
            end: at + 11,
            kind: "error".into(),
            text: "unsolved goals\ncase left\np q : Prop\nhp : p\nhq : q\n⊢ q\n\ncase right\np q : Prop\nhp : p\nhq : q\n⊢ p".into(),
            ..Default::default()
        });
        t.refresh(&*symbols);
    }
    let t = theory(&app);
    let strip = t.profile.message_summary(&t.prover.messages.last().unwrap().text);
    assert_eq!(strip, "unsolved goals (2 goals: case left, case right)");
    let mut h = Harness::new(Size::new(1100.0, 760.0));
    let png = h.draw(&app, "lean-unsolved-summary");
    assert!(ink(&png, 1100, 760, 100..1000, 200..400) > 1000);
}

#[test]
fn long_hypotheses_wrap_inside_the_goal_card() {
    // Lean prints a goal at its own width, not the panel's: a long hypothesis or target must
    // wrap inside its card instead of running past its edge
    let mut app = lean_app(Mode::Literate);
    select(&mut app, "theorem append_nil_right", "induction");
    {
        let ws = app.workspace.as_mut().unwrap();
        ws.panel = Panel::Output;
        ws.goals.messages = goal_parts(&[(
            "",
            &[
                "fuel : Nat",
                "reported : InSupport origin (evalOne fuel static current expression time).annotation",
            ],
            "OperationalFootprint fuel static current expression time origin ∧ InSupport origin (evalOne fuel static current expression time).annotation",
        )]);
    }
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    h.draw(&app, "lean-goal-long-lines");
}

/// Prose as Lean files lay it out, each in a file of its own (so that it is at the top of the page):
/// a module doc with a heading, a list and a code fence; a module doc of plain paragraphs indented
/// by two; docstrings whose lines the file indents by 4 and by 2, the last with a list and a fence
const PROSE_CASES: &[(&str, &str, &str)] = &[
    ("module-doc-heading", "# The section", "/-!\n# The section\n\nThe first paragraph of the module doc, wrapped over\nseveral lines of source so that the editor must wrap\nthe text as the page does, with `code` and $x^2$ in it.\n\n- a list item that\n  goes on\n- another item\n\n```lean\ndef a := 1\n  def b := 2\n```\n-/\n"),
    ("module-doc-plain", "Plain paragraphs", "/-!\n  Plain paragraphs, indented by two in the file,\n  that run over\n  three lines of source.\n\n  A second paragraph.\n-/\n"),
    ("docstring-indented", "A docstring of a declaration", "namespace Foo\n\n/-- A docstring of a declaration in a namespace, indented\n    in the file by four spaces, wrapped over\n    three lines of source. -/\ndef f := 1\n\nend Foo\n"),
    ("docstring-list-fence", "A docstring with a list", "/-- A docstring with a list:\n  - first item,\n    which wraps,\n  - second item,\n\n  and a fence:\n\n  ```lean\n  def g := 2\n    def h := 3\n  ```\n-/\ndef g := 2\n"),
];

/// The left edge of the ink in a row band, and the top of the ink around it (pixels), in the page
/// column
fn ink_edges(png: &[u8], w: u32, h: u32, y: u32, from: u32) -> Option<(u32, u32)> {
    let img = image::RgbaImage::from_raw(w, h, png.to_vec()).unwrap();
    let bg = *img.get_pixel(w - 3, h - 3);
    let ink = |x: u32, y: u32| {
        let p = img.get_pixel(x, y);
        let d = |i: usize| (p[i] as i32 - bg[i] as i32).abs();
        d(0) + d(1) + d(2) > 90
    };
    let cols = from..w - 60;
    let top = (y.saturating_sub(10)..y + 40).find(|&yy| cols.clone().any(|x| ink(x, yy)))?;
    let left = cols.clone().find(|&x| (top..top + 14).any(|yy| ink(x, yy)))?;
    Some((left, top))
}

/// Where a prose block's text starts before and after it is opened by a click on its first line:
/// the left edge and the top of its first line must not move
fn prose_edges(doc: &str, text_of_block: &str, w: f32, dark: bool, tag: &str) -> ((u32, u32), (u32, u32)) {
    let size = Size::new(w, 900.0);
    let mut app = lean_app_with(Mode::Literate, &format!("import DSRV.Semantics\n\n{doc}"));
    app.show_sidebar = false;
    app.show_right = false;
    // (no goal under the first block: nothing to push the prose down)
    app.workspace.as_mut().unwrap().goals = QueryState { operation: "deductum_goals", ..Default::default() };
    app.workspace.as_mut().unwrap().output = None;
    let mut h = Harness::new(size);
    light(!dark);
    super::view::set_goal_beside(false);
    let i = theory(&app)
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.kind != super::theory::Kind::Formal && theory(&app).local.chars().skip(b.start).take(b.end - b.start).collect::<String>().contains(text_of_block))
        .map(|(i, _)| i)
        .next()
        .unwrap_or_else(|| panic!("a prose block with {text_of_block:?}"));
    let name = |s: &str| format!("lean-open-{tag}-{}-{}-{s}", w as u32, if dark { "dark" } else { "light" });
    let png = h.draw(&app, &name("closed"));
    // the block's top: where the pointer first is over it
    let mut top = None;
    'scan: for y in (30..880).step_by(2) {
        for x in [140.0f32, 220.0, 300.0, 400.0, 500.0] {
            if h.move_to(&app, cosmic::iced::Point::new(x, y as f32)).iter().any(|m| matches!(m, super::Message::ProseHover(b, _) if *b == i)) {
                top = Some((x, y as f32));
                break 'scan;
            }
        }
    }
    let (x, y) = top.expect("the block under the pointer");
    let closed = ink_edges(&png, size.width as u32, 900, y as u32, 10).unwrap_or((0, 0));
    // a click on its first line (a little in from the left)
    let msgs = h.click(&app, cosmic::iced::Point::new(x + 60.0, y + 6.0));
    assert!(msgs.iter().any(|m| matches!(m, super::Message::EditBlock(_))), "a click opens the block");
    for m in msgs {
        let _ = cosmic::Application::update(&mut app, m);
    }
    let png = h.draw(&app, &name("open"));
    // (the accent bar that marks the open block is left of the text; the faint region behind it is not ink)
    let open = ink_edges(&png, size.width as u32, 900, y as u32, closed.0.saturating_sub(5)).unwrap_or((0, 0));
    light(false);
    super::view::set_goal_beside(true);
    (closed, open)
}

#[test]
fn opening_lean_prose_does_not_move_its_text() {
    let mut report = Vec::new();
    for (tag, needle, doc) in PROSE_CASES {
        for w in [800.0f32, 1400.0] {
            for dark in [true, false] {
                let (closed, open) = prose_edges(doc, needle, w, dark, tag);
                report.push(format!("{tag} {w} {}: closed {closed:?} open {open:?}", if dark { "dark" } else { "light" }));
                if std::env::var_os("LEAN_MEASURE_ONLY").is_some() {
                    continue;
                }
                assert!(closed.0.abs_diff(open.0) <= 1, "{tag} at {w}: the left edge moves ({closed:?} then {open:?})");
                assert!(closed.1.abs_diff(open.1) <= 2, "{tag} at {w}: the first line moves vertically ({closed:?} then {open:?})");
            }
        }
    }
    println!("{}", report.join("\n"));
}

/// Where the source view's text begins, between `y` rows: the leftmost column with text on it
/// (far from the background, which a faint tint such as the construct's is not). It moves if
/// the page changes between one column and two
fn left_edge(png: &[u8], w: u32, h: u32, y: std::ops::Range<u32>) -> u32 {
    let img = image::RgbaImage::from_raw(w, h, png.to_vec()).unwrap();
    let bg = *img.get_pixel(w - 3, h - 3);
    let far = |p: &image::Rgba<u8>| (0..3).map(|i| (p[i] as i32 - bg[i] as i32).abs()).sum::<i32>() > 150;
    (0..w).find(|&x| y.clone().step_by(2).any(|yy| far(img.get_pixel(x, yy)))).unwrap_or(w)
}

/// "In two column mode source view for Lean every time I click the source it flicks in and out
/// of single column mode": clicks in the source view (in a tactic, a def's body, a blank line, a
/// docstring), each drawn before the prover's state there comes (a click in another block clears
/// the goal) and after; the page's text stays where it was in every frame. The same for Isabelle,
/// and for both literate views as blocks open
#[test]
fn the_layout_does_not_change_as_clicks_move_the_caret() {
    let (w, h) = (1400u32, 900u32);
    for lean in [true, false] {
        for mode in [Mode::Source, Mode::Literate] {
            let mut harness = Harness::new(Size::new(w as f32, h as f32));
            super::view::set_goal_beside(true);
            let mut app = if lean { lean_app(mode) } else { demo_app(mode) };
            app.show_sidebar = false;
            app.show_right = false;
            app.window_width = w as f32;
            let app_output = app.workspace.as_ref().unwrap().output.clone().unwrap();
            // (the state for a command at `at`: a click in another block clears it, as the app
            // does, until the prover's state there comes)
            let output = |at: usize| {
                let mut o = app_output.clone();
                o["command"]["start"] = json!(at);
                o["command"]["end"] = json!(at + 1);
                o
            };
            app.workspace.as_mut().unwrap().output = Some(output(0));
            let goals = app.workspace.as_ref().unwrap().goals.clone();
            let name = |what: &str| format!("flicker-{}-{}-{what}", if lean { "lean" } else { "isabelle" }, if mode == Mode::Source { "source" } else { "literate" });
            let first = harness.draw(&app, &name("0"));
            // (the layout's own decision, two columns or one; and, in the source view, where its
            // text is drawn: the literate view's open block has a marker reaching to its left)
            let columns = || if mode == Mode::Source { super::view::last_source_beside() } else { super::view::last_wide() };
            assert!(columns(), "{}: two columns to begin with", name(""));
            let source = mode == Mode::Source;
            let edge = left_edge(&first, w, h, 60..h - 60);
            let spots: &[&str] = if lean { &["induction xs with", "left ++ right", "\n\ntheorem and_swap'", "A variable observation", "exact hq"] } else { &["apply simp", "(a::nat) + b", "\n\n(* the second", "We prove that", "by simp"] };
            for (k, spot) in spots.iter().enumerate() {
                // The spot's line, as the source view draws it (or its block, opened, in the
                // literate view), clicked
                let t = theory(&app);
                let at = t.local.find(spot).map(|b| t.local[..b].chars().count()).unwrap() + if spot.starts_with('\n') { 1 } else { 2 };
                if mode == Mode::Source {
                    let line = t.lines.position(&t.decoded.text, t.decoded.to_decoded(at)).line;
                    // (where line 0 is: calibrated by a click, whose line says)
                    let probe = 300.0;
                    for m in harness.click(&app, cosmic::iced::Point::new(500.0, probe)) {
                        update(&mut app, m);
                    }
                    let probed = theory(&app).source.cursor().position.line as f32;
                    let lh = super::view::source_line_height();
                    let y = probe + (line as f32 - probed) * lh;
                    let x = 300.0;
                    for m in harness.click(&app, cosmic::iced::Point::new(x, y)) {
                        update(&mut app, m);
                    }
                    let got = theory(&app).source.cursor().position.line;
                    assert_eq!(got, line, "{}: the click at {y} lands on the line of {spot:?}", name(""));
                } else {
                    // (a blank line between blocks is no block to open: a click there closes the open one)
                    match theory(&app).blocks.iter().position(|b| b.start <= at && at <= b.end) {
                        Some(i) => update(&mut app, super::Message::EditBlock(i)),
                        None => update(&mut app, super::Message::CloseBlock),
                    }
                }
                // (a click in another block: the goal is gone until the prover answers)
                let png = harness.draw(&app, &name(&format!("{}-clicked", k + 1)));
                let now = left_edge(&png, w, h, 60..h - 60);
                assert!(columns(), "{}: one column after a click at {spot:?}", name(""));
                assert!(!source || now.abs_diff(edge) <= 2, "{}: the page's text moved from x {edge} to {now} after a click at {spot:?}", name(""));
                // the state there arrives
                let ws = app.workspace.as_mut().unwrap();
                ws.output = Some(output(at));
                ws.goals = goals.clone();
                let png = harness.draw(&app, &name(&format!("{}-state", k + 1)));
                let now = left_edge(&png, w, h, 60..h - 60);
                assert!(columns(), "{}: one column when the state came at {spot:?}", name(""));
                assert!(!source || now.abs_diff(edge) <= 2, "{}: the page's text moved from x {edge} to {now} when the state came at {spot:?}", name(""));
            }
        }
    }
}

/// Where the pop-out button and the editor it floats over are laid out, and how far the page is
/// scrolled (what is drawn is that much higher)
struct PopOutPlace {
    editor: cosmic::widget::Id,
    scroll: cosmic::widget::Id,
    pop: Option<cosmic::iced::Rectangle>,
    cell: Option<cosmic::iced::Rectangle>,
    scrolled: cosmic::iced::Vector,
}

impl cosmic::iced::advanced::widget::Operation<()> for PopOutPlace {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn cosmic::iced::advanced::widget::Operation<()>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&cosmic::widget::Id>, bounds: cosmic::iced::Rectangle) {
        if id == Some(&super::view::pop_out_button_id()) {
            self.pop = Some(bounds);
        }
    }

    fn focusable(&mut self, id: Option<&cosmic::widget::Id>, bounds: cosmic::iced::Rectangle, _state: &mut dyn cosmic::iced::advanced::widget::operation::Focusable) {
        if id == Some(&self.editor) {
            self.cell = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&cosmic::widget::Id>,
        _bounds: cosmic::iced::Rectangle,
        _content: cosmic::iced::Rectangle,
        translation: cosmic::iced::Vector,
        _state: &mut dyn cosmic::iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&self.scroll) {
            self.scrolled = translation;
        }
    }
}

/// "The pop-out button is in slightly the wrong position in both source view and literate view":
/// it must sit inside the top right corner of the code it belongs to (the open block's cell, or
/// the proof's first line in the source view), as far from the right edge as from the top,
/// centred on the first line, whatever the prover, theme, width and scrolling. Measured from the
/// layout, and checked against the frame: the icon is drawn inside the button's place
#[test]
fn the_pop_out_button_sits_in_the_corner_of_its_proof() {
    let fillers: String = (0..30).map(|i| format!("lemma filler{i}: \"True\"\n  by simp\n\n")).collect();
    let long_isabelle = super::snapshot::DEMO.replace("lemma broken", &format!("{fillers}lemma broken"));
    let mut report = Vec::new();
    for lean in [true, false] {
        for mode in [Mode::Literate, Mode::Source] {
            for dark in [true, false] {
                for (w, h) in [(800u32, 760u32), (1400, 900)] {
                    for scrolled in [false, true] {
                        let mut harness = Harness::new(Size::new(w as f32, h as f32));
                        light(!dark);
                        super::view::set_goal_beside(true);
                        // (near the top, or far down the document and scrolled to)
                        let (mut app, target) = match (lean, scrolled) {
                            (true, false) => (lean_app_with(mode, &proofs_only()), "theorem append_nil_right"),
                            (true, true) => (lean_app(mode), "theorem and_swap'"),
                            (false, false) => (demo_app(mode), "lemma add_zero"),
                            (false, true) => {
                                let mut app = demo_app(mode);
                                let doc = json!({"text": long_isabelle, "revision": 1, "prover": {}});
                                app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/Long.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), Arc::new(HashMap::new()), mode, super::prover::profile(None)));
                                (app, "lemma broken")
                            }
                        };
                        app.show_sidebar = false;
                        app.show_right = false;
                        app.window_width = w as f32;
                        let t = theory(&app);
                        let at = t.local.find(target).map(|b| t.local[..b].chars().count()).unwrap();
                        let i = t.blocks.iter().position(|b| b.start <= at && at <= b.end).unwrap();
                        if mode == Mode::Literate {
                            update(&mut app, super::Message::EditBlock(i));
                        } else {
                            let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
                            t.cursor = at + 3;
                            let pos = t.lines.position(&t.decoded.text, t.decoded.to_decoded(at + 3));
                            super::editor::place(&mut t.source, pos);
                        }
                        let (editor, scroll) = if mode == Mode::Literate { (super::view::block_editor_id(), super::view::literate_scroll_id()) } else { (super::view::source_editor_id(), super::view::source_scroll_id()) };
                        let mut place = PopOutPlace { editor: editor.clone(), scroll: scroll.clone(), pop: None, cell: None, scrolled: cosmic::iced::Vector::ZERO };
                        let name = format!("popout-{}-{}-{}-{w}{}", if lean { "lean" } else { "isabelle" }, if mode == Mode::Source { "source" } else { "literate" }, if dark { "dark" } else { "light" }, if scrolled { "-scrolled" } else { "" });
                        harness.draw(&app, &name);
                        harness.operate(&app, &mut place);
                        if scrolled {
                            // the page scrolled so that the proof is in the middle of the view
                            let pop = place.pop.expect("a pop-out button");
                            harness.move_to(&app, cosmic::iced::Point::new(w as f32 / 3.0, h as f32 / 2.0));
                            let by = pop.y - h as f32 / 2.0;
                            harness.events(&app, &[cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::WheelScrolled { delta: cosmic::iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -by } })]);
                            harness.draw(&app, &name);
                            harness.operate(&app, &mut place);
                        }
                        let png = harness.draw(&app, &name);
                        let (pop, cell) = (place.pop.expect("a pop-out button"), place.cell.expect("the editor"));
                        let drawn = cosmic::iced::Rectangle { y: pop.y - place.scrolled.y, x: pop.x - place.scrolled.x, ..pop };
                        // Where the proof's first line is: the cell's first line, or the source
                        // view's line of the proof's start
                        let lh = if mode == Mode::Literate { super::view::code_line_height() } else { super::view::source_line_height() };
                        let img = image::RgbaImage::from_raw(w, h, png.clone()).unwrap();
                        let (first_top, right) = if mode == Mode::Literate {
                            (cell.y + 5.0, cell.x + cell.width)
                        } else {
                            // (the source view's lines wrap: the proof's first row is where the
                            // construct's tint begins, the caret being on it, at the editor's left)
                            let x = (cell.x + 3.0) as u32;
                            let above = *img.get_pixel(x, (drawn.y - 25.0).max(0.0) as u32);
                            let tint = ((drawn.y - 15.0).max(0.0) as u32..(drawn.y + 15.0) as u32).find(|&y| img.get_pixel(x, y) != &above);
                            // (with no construct known, the line's place, no line above it wrapping)
                            let t = theory(&app);
                            let line = t.lines.position(&t.decoded.text, t.decoded.to_decoded(t.blocks[i].start)).line;
                            let top = tint.map_or(cell.y + cosmic::theme::active().cosmic().spacing.space_s as f32 + line as f32 * lh - place.scrolled.y, |y| y as f32);
                            (top + place.scrolled.y, cell.x + cell.width)
                        };
                        let (right_inset, centre_off) = (right - (pop.x + pop.width), (pop.y + pop.height / 2.0) - (first_top + lh / 2.0));
                        let top_inset = pop.y - if mode == Mode::Literate { cell.y } else { first_top };
                        // The icon drawn inside the button's place: ink there, centred
                        let bg = *img.get_pixel(drawn.x as u32 - 2, (drawn.y + drawn.height / 2.0) as u32);
                        let far = |p: &image::Rgba<u8>| (0..3).map(|k| (p[k] as i32 - bg[k] as i32).abs()).sum::<i32>() > 120;
                        let (mut xs, mut ys) = (Vec::new(), Vec::new());
                        // (inside the cell: its own border above is not the icon, and sits within 6 px of the
                        // button where the code's 14 px lines make the button's inset 4.8 px)
                        for y in (drawn.y as i32 - 6).max((cell.y - place.scrolled.y).ceil() as i32 + 1)..((drawn.y + drawn.height) as i32 + 6).min(h as i32) {
                            for x in (drawn.x as i32 - 6)..((drawn.x + drawn.width) as i32 + 6).min(w as i32).min(if mode == Mode::Literate { (cell.x + cell.width - place.scrolled.x) as i32 - 1 } else { i32::MAX }) {
                                if far(img.get_pixel(x as u32, y as u32)) {
                                    xs.push(x as f32);
                                    ys.push(y as f32);
                                }
                            }
                        }
                        assert!(xs.len() > 10, "{name}: the icon is drawn where the button is ({drawn:?})");
                        let ink = ((xs.iter().cloned().fold(f32::MAX, f32::min) + xs.iter().cloned().fold(0.0, f32::max)) / 2.0, (ys.iter().cloned().fold(f32::MAX, f32::min) + ys.iter().cloned().fold(0.0, f32::max)) / 2.0);
                        let off = (ink.0 - (drawn.x + drawn.width / 2.0), ink.1 - (drawn.y + drawn.height / 2.0));
                        report.push(format!("{name}: pop {pop:?} cell {cell:?} right inset {right_inset:.1} top inset {top_inset:.1} off the first line's centre {centre_off:.1} icon off its place {off:?}"));
                        assert!(off.0.abs() <= 2.5 && off.1.abs() <= 2.5, "{name}: the icon is drawn {off:?} from the button's place");
                        assert!(centre_off.abs() <= 1.0, "{name}: centred on the first line ({centre_off:.1} off)");
                        assert!((2.0..=8.0).contains(&right_inset), "{name}: inside the right edge ({right_inset:.1})");
                        if mode == Mode::Literate {
                            assert!(top_inset >= 1.0, "{name}: inside the cell's top ({top_inset:.1})");
                            assert!((right_inset - top_inset).abs() <= 1.0, "{name}: as far from the right as from the top ({right_inset:.1}, {top_inset:.1})");
                        } else {
                            assert!(top_inset >= -1.0, "{name}: within the first line's band ({top_inset:.1})");
                        }
                        light(false);
                    }
                }
            }
        }
    }
    eprintln!("{}", report.join("\n"));
}

/// The insert bar above Lean's goal: symbols (⟨⟩ and → among them) in a `def`'s body where the
/// goal is its expected type, tactics at a tactic's start, Markdown in a docstring, the whole
/// palette when opened; dark and light, beside and under, 800 and 1400 px; in the source view and
/// the proof window too
#[test]
fn the_insert_bar_sits_above_the_lean_goal() {
    use super::prover::InsertContext;
    use super::snapshot::{caret_in_block, insert_bar_bounds};
    let ctx = |app: &App| super::insert::insert_context(theory(app));
    let expected = |app: &mut App| {
        app.workspace.as_mut().unwrap().goals.messages = goal_parts(&[("", &["left right : List Nat"], "List Nat")]);
    };
    for dark in [true, false] {
        for (w, h, beside) in [(1400.0, 900.0, true), (1400.0, 900.0, false), (800.0, 760.0, true), (800.0, 760.0, false)] {
            let mut harness = Harness::new(Size::new(w, h));
            light(!dark);
            super::view::set_goal_beside(beside);
            let mut app = lean_app(Mode::Literate);
            app.show_sidebar = false;
            app.show_right = false;
            app.window_width = w;
            let name = |what: &str| format!("bar-lean-{what}-{}-{}{}", if dark { "dark" } else { "light" }, w as u32, if beside { "-beside" } else { "-under" });
            // a def's body, its expected type in the goal panel: symbols
            let i = block_named(&app, "def merge");
            update(&mut app, super::Message::EditBlock(i));
            // (the page scrolled to it)
            harness.draw(&app, &name("opening"));
            super::snapshot::scroll_to_open_block(&mut harness, &app);
            expected(&mut app);
            caret_in_block(&mut app, ":= left", 3);
            assert_eq!(ctx(&app), Some(InsertContext::Symbols), "a def's body is a term");
            harness.draw(&app, &name("def"));
            assert!(insert_bar_bounds(&mut harness, &app).is_some(), "{}: a bar above the goal", name("def"));
            let quick: Vec<String> = super::prover::LEAN.quick_entries(InsertContext::Symbols, &app.workspace.as_ref().unwrap().caps).into_iter().map(|e| e.label).collect();
            assert!(quick.iter().take(10).any(|l| l == "⟨⟩") && quick.iter().take(4).any(|l| l == "→"), "{quick:?}");
            update(&mut app, super::Message::InsertBar(true));
            harness.draw(&app, &name("def-open"));
            update(&mut app, super::Message::InsertBar(false));
            // a tactic's start in a proof: tactics
            let i = block_named(&app, "theorem and_swap'");
            update(&mut app, super::Message::EditBlock(i));
            // (the page scrolled to it)
            harness.draw(&app, &name("opening"));
            super::snapshot::scroll_to_open_block(&mut harness, &app);
            app.workspace.as_mut().unwrap().goals.messages = goal_parts(&[("", &["p q : Prop", "hp : p", "hq : q"], "q ∧ p")]);
            caret_in_block(&mut app, "constructor", 0);
            assert_eq!(ctx(&app), Some(InsertContext::Proof), "a tactic's start");
            harness.draw(&app, &name("tactic"));
            update(&mut app, super::Message::InsertBar(true));
            harness.draw(&app, &name("tactic-open"));
            // a search for tactics by label
            update(&mut app, super::Message::PaletteQuery("simp".into()));
            harness.draw(&app, &name("tactic-search"));
            update(&mut app, super::Message::PaletteQuery(String::new()));
            update(&mut app, super::Message::InsertBar(false));
            // in a tactic's argument (have's statement): symbols again
            caret_in_block(&mut app, "exact hq", 6);
            assert_eq!(ctx(&app), Some(InsertContext::Symbols), "exact's term");
            light(false);
            super::view::set_goal_beside(true);
        }
    }
    // The source view's goal pane (a def's body), and the proof window (a tactic's start)
    let mut app = lean_app(Mode::Source);
    app.show_sidebar = false;
    app.show_right = false;
    expected(&mut app);
    {
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let at = t.local.find(":= left").map(|b| t.local[..b].chars().count()).unwrap() + 3;
        t.cursor = at;
        let pos = t.lines.position(&t.decoded.text, at);
        super::editor::place(&mut t.source, pos);
    }
    assert_eq!(ctx(&app), Some(InsertContext::Symbols), "the source view: a def's body");
    let mut harness = Harness::new(Size::new(1400.0, 900.0));
    harness.draw(&app, "bar-lean-source-1400");
    assert!(insert_bar_bounds(&mut harness, &app).is_some(), "a bar above the source view's goal");
    super::view::set_goal_beside(false);
    harness.draw(&app, "bar-lean-source-1400-under");
    super::view::set_goal_beside(true);
    // a declaration's docstring (part of its block): Markdown
    {
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let at = t.local.find("keeps the causes").map(|b| t.local[..b].chars().count()).unwrap();
        t.cursor = at;
        let pos = t.lines.position(&t.decoded.text, at);
        super::editor::place(&mut t.source, pos);
    }
    assert_eq!(ctx(&app), Some(InsertContext::Prose), "a docstring is Markdown");
    app.workspace.as_mut().unwrap().output = Some(json!({"command": {"name": "theorem", "status": "finished"}, "messages": []}));
    harness.draw(&app, "bar-lean-source-docstring");
    drop(harness);
    {
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let at = t.local.find("  constructor").map(|b| t.local[..b].chars().count()).unwrap() + 2;
        t.cursor = at;
    }
    update(&mut app, super::Message::PopOut);
    assert!(app.popout.is_some());
    caret_in_block(&mut app, "constructor", 0);
    // Layout publishes process-wide insertion state. Keep the UI turn until the
    // assertion reads it, so another test cannot replace this proof's context.
    let _proof_turn = super::snapshot::ui_turn();
    super::snapshot::render(super::view::proof_window(&app), "bar-lean-proof-window", Size::new(1000.0, 600.0));
    assert_eq!(ctx(&app), Some(InsertContext::Proof), "the proof window: a tactic's start");
}

/// In a Lean `def`'s body: → from the bar's row at the caret, then ⟨⟩ (found by its character)
/// around a selection; each one entry of the history, Unicode in the source, the editor still
/// focused with its caret after the insertion
#[test]
fn a_chip_on_the_lean_bar_inserts_unicode_and_wraps_the_selection() {
    use super::snapshot::{caret_in_block, click_chip};
    use cosmic::widget::text_editor::{Cursor, Position};
    let mut app = lean_app(Mode::Literate);
    app.show_sidebar = false;
    app.show_right = false;
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    super::view::set_goal_beside(true);
    let i = block_named(&app, "def merge");
            update(&mut app, super::Message::EditBlock(i));
    h.focus(&app, super::view::block_editor_id());
    caret_in_block(&mut app, ":= left", 3);
    h.draw(&app, "lean-chip-0");
    let local = |app: &App| theory(app).local.clone();
    for m in click_chip(&mut h, &app, "→") {
        let task = cosmic::Application::update(&mut app, m);
        h.run_task(&app, task);
    }
    for m in h.type_text(&app, " ") {
        update(&mut app, m);
    }
    assert!(local(&app).contains(":= → left ++ right"), "→ at the caret, typing after it: {:?}", local(&app));
    h.draw(&app, "lean-chip-1-arrow");
    // ⟨⟩ around `left`
    {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        let text = be.content.text();
        let s = text[..text.find("left ++").unwrap()].chars().count();
        let li = super::editor::LineIndex::new(&text);
        let p = li.position(&text, s);
        be.content.move_to(Cursor { position: Position { line: p.line, column: p.column + 4 }, selection: Some(p) });
    }
    update(&mut app, super::Message::PaletteQuery("⟨".into()));
    h.draw(&app, "lean-chip-2-search");
    for m in click_chip(&mut h, &app, "⟨") {
        let task = cosmic::Application::update(&mut app, m);
        h.run_task(&app, task);
    }
    for m in h.type_text(&app, "!") {
        update(&mut app, m);
    }
    assert!(local(&app).contains(":= → ⟨left⟩! ++ right"), "the selection wrapped, the caret after it: {:?}", local(&app));
    let t = theory(&app);
    for (_, text) in &t.steps {
        assert_eq!(text.contains("⟨left"), text.contains("left⟩"), "a step with half the wrap: {text:?}");
    }
    h.draw(&app, "lean-chip-3-wrapped");
}
