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
    let at = |needle: &str| text[..text.rfind(needle).unwrap()].chars().count();
    let (sorry, omega) = (at("sorry"), at("omega"));
    let _ = &chars;
    json!({
        "text": text, "revision": 1, "dirty": false, "can_undo": true,
        "prover": {
            "revision": 1,
            "status": {"finished": 20, "unprocessed": 0, "running": 0, "failed": 1, "consolidated": true},
            "commands": commands,
            "messages": [
                {"start": sorry, "end": sorry + 5, "kind": "warning", "text": "declaration uses 'sorry'"},
                {"start": omega, "end": omega + 5, "kind": "error", "text": "omega could not prove the goal:\nNo usable constraints found. You may need to unfold definitions so `omega` can see linear arithmetic facts about `Nat` and `Int`, which may also involve multiplication, division, and modular remainder by constants."},
            ],
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
    let ctx = |app: &App| super::view::insert_context(theory(app));
    assert_eq!(ctx(&app), Some(super::prover::InsertContext::Proof));
    // (the palette takes the goal's place while there is none)
    app.workspace.as_mut().unwrap().goals = QueryState { operation: "deductum_goals", ..Default::default() };
    app.workspace.as_mut().unwrap().output = None;
    let mut h = Harness::new(Size::new(1320.0, 800.0));
    {
        use cosmic::widget::text_editor::{Action, Motion};
        update(&mut app, super::Message::BlockAction(Action::Move(Motion::DocumentEnd)));
    }
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
    let probe: Vec<i32> = (200..1200).step_by(20).filter(|x| click_at(*x as f32).1).collect();
    assert!(probe.len() > 6, "the paragraph is under the pointer somewhere: {probe:?}");
    let (lo, hi) = (probe[1] as f32, probe[probe.len() - 2] as f32);
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
