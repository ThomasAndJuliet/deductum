// SPDX-License-Identifier: GPL-3.0-only

//! Deductum's window drawn headless, as `src/thread/snapshot.rs` draws
//! threads: the real view, laid out and rendered with tiny-skia. Set
//! `CT_SNAPSHOTS` to a directory to keep the frames as PNG files.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use cosmic::iced::advanced::clipboard;
use cosmic::iced::advanced::renderer::{Headless, Style};
use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
use cosmic::iced::{Event, Font, Pixels, Size, mouse};
use serde_json::json;

use super::editor::{Mode, Theory};
use super::prover::Profile;
use super::prover::isabelle::symbols::Table;
use super::{App, Link, Panel, QueryState, Workspace, link};

pub const DEMO: &str = r#"theory Demo
  imports Main
begin

section \<open>Natural numbers\<close>

text \<open>We prove that addition with zero is the identity, so $x + 0 = x$ for every
  @{typ nat}, and that @{term "a + b = b + a"}. The proof of the first is
  \<^emph>\<open>by simplification\<close>; see \cite{Nipkow2002}.\<close>

lemma add_zero: "\<forall>x::nat. x + 0 = x"
  apply simp
  done

(* the second is left for Sledgehammer *)

lemma add_comm: "(a::nat) + b = b + a"
  sorry

lemma broken: "(x::nat) < x"
  by simp

end
"#;

fn prover_json(text: &str) -> serde_json::Value {
    let cmds = super::prover::isabelle::blocks::lexical_commands(text, &HashMap::new());
    let chars: Vec<char> = text.chars().collect();
    let commands: Vec<serde_json::Value> = cmds
        .iter()
        .map(|c| {
            let src: String = chars[c.start..c.end].iter().collect();
            let status = if src.contains("broken") || (c.name == "by" && src.contains("by simp") && text[..].contains("broken")) && c.start > text.find("broken").map(|b| text[..b].chars().count()).unwrap_or(usize::MAX) { "failed" } else if c.name == "sorry" { "warned" } else { "finished" };
            json!([c.start, c.end, c.name, status])
        })
        .collect();
    let by = text.rfind("by simp").map(|b| text[..b].chars().count()).unwrap();
    json!({
        "revision": 1,
        "status": {"finished": 14, "unprocessed": 0, "running": 0, "failed": 1, "consolidated": true},
        "commands": commands,
        "messages": [{"start": by, "end": by + 7, "kind": "error", "text": "Failed to finish proof\ngoal (1 subgoal):\n 1. False"}],
    })
}

pub fn demo_app(mode: Mode) -> App {
    let table = Arc::new(Table::fallback());
    let doc = json!({"text": DEMO, "revision": 1, "prover": prover_json(DEMO), "dirty": false, "can_undo": true});
    let mut theory = Theory::new("/home/me/Demo/Demo.thy".into(), &doc, &*table, Arc::new(HashMap::new()), mode, crate::prover::profile(None));
    theory.cursor = DEMO.find("apply simp").unwrap() + 3;
    let mut sessions = BTreeMap::new();
    sessions.insert(
        ("local".to_string(), "s1".to_string()),
        json!({"id": "s1", "name": "Demo", "folder": "/home/me/Demo", "logic": "HOL", "lifecycle": "Running",
               "activity": {"running": 0, "failed": 1}, "clients": [{"kind": "agent", "key": "claude:c1", "label": "Claude"}],
               "grants": [{"id": "g1", "agent": "claude:c1", "label": "Claude", "scope": "theory", "path": "/home/me/Demo/Demo.thy"}]}),
    );
    sessions.insert(
        ("local".to_string(), "s2".to_string()),
        json!({"id": "s2", "name": "UTP", "folder": "/home/me/UTP", "logic": "Shallow-Expressions-Z", "lifecycle": "Paused",
               "activity": {}, "clients": [], "grants": []}),
    );
    let info = json!({
        "folder": "/home/me/Demo",
        "theories": [{"path": "/home/me/Demo/Demo.thy", "status": {"failed": 1}, "dirty": false},
                     {"path": "/home/me/Demo/More.thy", "status": {"consolidated": true}, "dirty": true}],
        "seen": {"claude:c1": {"label": "Claude", "position": {"path": "/home/me/Demo/Demo.thy", "line": 18}}},
    });
    App {
        core: cosmic::app::Core::default(),
        links: vec![Link {
            target: link::Target { key: "local".into(), destination: None },
            name: "au-laptop".into(),
            sender: None,
            status: "connected".into(),
            hello: None,
            client_id: None,
            symbols: Some(table),
        }],
        ctrl: false,
        alt: false,
        back: Vec::new(),
        references: Vec::new(),
        entity: None,
        hover_gen: 0,
        keyword_gen: 0,
        prose_pointer: None,
        prose_hover: None,
        vim_block: false,
        goal_placement_override: None,
        sessions,
        next_id: 0,
        pending: HashMap::new(),
        workspace: Some(Workspace {
            machine: "local".into(),
            sid: "s1".into(),
            profile: super::prover::profile(None),
            caps: super::prover::profile(None).default_capabilities(),
            info: Some(info),
            theory: Some(theory),
            opening: None,
            panel: Panel::Symbols,
            output: Some(json!({"command": {"name": "apply", "status": "finished"}, "messages": [
                {"kind": "state", "text": "proof (prove)\ngoal (1 subgoal):\n 1. \\<forall>x. x + 0 = x"}]})),
            tool_query: QueryState { operation: "tool", ..Default::default() },
            goals: QueryState {
                operation: "deductum_goals",
                status: "finished".into(),
                messages: [
                    "#deductum:fact 0\nlength xs = n",
                    "#deductum:count 2",
                    "#deductum:param 1\nxs :: 'a list",
                    "#deductum:prem 1\n0 < n",
                    "#deductum:concl 1\nxs \\<noteq> []",
                    "#deductum:param 2\nxs :: 'a list",
                    "#deductum:concl 2\nP n",
                ]
                .iter()
                .map(|r| json!({"kind": "writeln", "raw": r, "text": r, "spans": []}))
                .collect(),
                ..Default::default()
            },
            goals_prev: QueryState { operation: "deductum_goals", ..Default::default() },
            state_text: false,
            show_types: false,
            all_goals: false,
            theory_filter: String::new(),
            notes: vec![json!({"id": "n1", "label": "Claude", "start": 330, "end": 360, "line": 18, "text": "This needs induction on a; simp alone will not do.", "replies": []})],
            note_reply: Default::default(),
            show_offer: None,
            pending_goto: None,
            hover: None,
            settings_open: false,
            tool_started: None,
            gap_hover: None,
            tools_stopping: false,
            palette_query: String::new(),
            insert_open: false,
            find: QueryState { operation: "find_theorems", ..Default::default() },
            find_input: String::new(),
            cex: QueryState { operation: "deductum_command", ..Default::default() },
            fill: super::FillState {
                status: "done".into(),
                candidates: vec![json!({"text": "by (simp add: add.commute)", "ok": true, "message": ""}),
                                 json!({"text": "by auto", "ok": false, "message": "Failed to finish proof"})],
                ..Default::default()
            },
            cex_params: String::new(),
            views: HashMap::new(),
            activity: Some(json!({"lifecycle": "Running", "prover": {"cpu": 385, "rss_mb": 3120},
                "running": [{"path": "/home/me/Demo/Demo.thy", "line": 21, "name": "by", "seconds": 41}],
                "theories": [{"path": "/home/me/Demo/Demo.thy", "theory": "Draft.Demo", "percentage": 80, "running": 1, "failed": 1, "consolidated": false, "total": 16, "imports": ["/home/me/Demo/Base.thy"]},
                             {"path": "/home/me/Demo/Base.thy", "theory": "Draft.Base", "percentage": 45, "running": 2, "failed": 0, "consolidated": false, "total": 120, "imports": []}]})),
            tool_settings: HashMap::new(),
            symbol_search: String::new(),
            history: vec![json!({"id": 3, "author": {"kind": "agent", "label": "Claude"}, "kind": "edit", "line": 18, "summary": "−5 +'by simp'"}),
                          json!({"id": 2, "author": {"kind": "human", "label": "You"}, "kind": "edit", "line": 7, "summary": "+'zero'"})],
            follow: None,
            log: vec!["Welcome".into()],
            merge: None,
        }),
        new_session: None,
        show_sidebar: true,
        status: None,
        state_gen: 0,
        request: None,
        kinds: Arc::new(HashMap::new()),
        popout: None,
        want_theory: None,
        left: super::LeftMode::Sessions,
        show_right: true,
        left_width: 290.0,
        right_width: 420.0,
        dragging: None,
        window_width: 1400.0,
        vim: super::vim::Vim::default(),
        vim_enabled: false,
        mode_model: cosmic::widget::segmented_button::SingleSelectModel::default(),
        mode_entities: [cosmic::widget::segmented_button::Entity::default(); 2],
        shell: Default::default(),
    }
}

pub fn draw(app: &App, name: &str, size: Size) -> Vec<u8> {
    let turn = ui_turn();
    draw_locked(super::view::view(app), name, size, turn)
}

/// Views publish process-wide state as they are built (vim's mode, the zoom): tests that draw or
/// drive the widgets take turns, so that one test's view does not change another's keys
static UI: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn ui_turn() -> std::sync::MutexGuard<'static, ()> {
    UI.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn draw_element(element: cosmic::Element<'_, super::Message>, name: &str, size: Size) -> Vec<u8> {
    draw_locked(element, name, size, ui_turn())
}

/// As [`draw_element`], with the UI's turn already taken (before the element was built, which
/// may publish process-wide state too)
fn draw_locked(element: cosmic::Element<'_, super::Message>, name: &str, size: Size, _turn: std::sync::MutexGuard<'static, ()>) -> Vec<u8> {
    render(element, name, size)
}

/// A frame, drawn by a caller that already has its turn (to set the theme for it, say)
pub fn render(element: cosmic::Element<'_, super::Message>, name: &str, size: Size) -> Vec<u8> {
    super::load_fonts();
    let mut renderer = cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia"))).expect("a headless renderer");
    let theme = cosmic::theme::active();
    let mut ui = UserInterface::build(element, size, Cache::default(), &mut renderer);
    let _ = ui.update(
        &[Event::Window(cosmic::iced::window::Event::RedrawRequested(std::time::Instant::now()))],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut Vec::new(),
    );
    let text: cosmic::iced::Color = theme.cosmic().on_bg_color().into();
    ui.draw(&mut renderer, &theme, &Style { icon_color: text, text_color: text, scale_factor: 1.0 }, mouse::Cursor::Unavailable);
    let physical = cosmic::iced::Size::new(size.width as u32, size.height as u32);
    let rgba = renderer.screenshot(physical, 1.0, theme.cosmic().bg_color().into());
    if let Some(dir) = std::env::var_os("CT_SNAPSHOTS").filter(|d| !d.is_empty()) {
        image::RgbaImage::from_raw(physical.width, physical.height, rgba.clone())
            .expect("a frame")
            .save(std::path::Path::new(&dir).join(format!("{name}.png")))
            .expect("saved");
    }
    rgba
}

#[test]
fn draws_the_literate_view() {
    let app = demo_app(Mode::Literate);
    let frame = draw(&app, "deductum-literate", Size::new(1400.0, 900.0));
    assert!(frame.chunks(4).any(|p| p[0] > 200));
}

#[test]
fn draws_the_source_view() {
    let app = demo_app(Mode::Source);
    draw(&app, "deductum-source", Size::new(1400.0, 900.0));
}

#[test]
fn draws_the_panels() {
    let mut app = demo_app(Mode::Literate);
    let ws = app.workspace.as_mut().unwrap();
    ws.panel = Panel::Tool;
    ws.tool_query = QueryState {
        operation: "sledgehammer",
        status: "finished".into(),
        messages: vec![
            json!({"kind": "writeln", "text": "e found a proof...", "sendbacks": []}),
            json!({"kind": "writeln", "text": "e: Try this: by simp (6 ms)", "sendbacks": ["by simp"]}),
            json!({"kind": "writeln", "text": "cvc5: Try this: by presburger (1 ms)", "sendbacks": ["by presburger"]}),
        ],
        replace: Some((10, 15)),
        ..Default::default()
    };
    draw(&app, "deductum-sledgehammer", Size::new(1400.0, 900.0));
    app.workspace.as_mut().unwrap().panel = Panel::Agents;
    draw(&app, "deductum-agents", Size::new(1400.0, 900.0));
    app.workspace.as_mut().unwrap().panel = Panel::History;
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    t.open_block(4, &Table::fallback());
    draw(&app, "deductum-history-editing", Size::new(1400.0, 900.0));
    // Editing prose shows its rendered preview; a proof pops out
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    t.open_block(2, &Table::fallback());
    draw(&app, "deductum-prose-editing", Size::new(1400.0, 900.0));
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    t.open_block(3, &Table::fallback());
    let element = super::view::proof_window(&app);
    let _ = element;
}

#[test]
fn draws_the_proof_window() {
    let mut app = demo_app(Mode::Literate);
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let i = t.blocks.iter().position(|b| b.name == "lemma").unwrap();
    t.open_block(i, &Table::fallback());
    draw_element(super::view::proof_window(&app), "deductum-proof-window", Size::new(1000.0, 700.0));
}

#[test]
fn draws_a_prose_preview_alone() {
    let app = demo_app(Mode::Literate);
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let i = t.blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
    let doc = t.docs[i].clone().unwrap();
    let typo = std::sync::Arc::new(super::view::typography(&cosmic::theme::active()));
    let el: cosmic::Element<'_, super::Message> = prose_render::rich::rich_text(doc.clone(), typo.clone()).into();
    draw_element(el, "deductum-preview-alone", Size::new(700.0, 200.0));
    let el2: cosmic::Element<'_, super::Message> = cosmic::widget::column::with_capacity(2)
        .push(cosmic::widget::text_editor::text_editor(&t.source).height(cosmic::iced::Length::Fixed(80.0)))
        .push(prose_render::rich::rich_text(doc, typo))
        .into();
    draw_element(el2, "deductum-preview-under-editor", Size::new(700.0, 300.0));
    let el3: cosmic::Element<'_, super::Message> = cosmic::widget::column::with_capacity(2)
        .push(cosmic::iced::widget::stack![cosmic::widget::Space::new(), cosmic::widget::text_editor::text_editor(&t.source).height(cosmic::iced::Length::Fixed(80.0))])
        .push(prose_render::rich::rich_text(t.docs[i].clone().unwrap(), std::sync::Arc::new(super::view::typography(&cosmic::theme::active()))))
        .into();
    draw_element(el3, "deductum-preview-editor-layered", Size::new(700.0, 300.0));
    let el4: cosmic::Element<'_, super::Message> = cosmic::widget::column::with_capacity(2)
        .push(cosmic::iced::widget::text_editor(&t.source).height(cosmic::iced::Length::Fixed(80.0)))
        .push(prose_render::rich::rich_text(t.docs[i].clone().unwrap(), std::sync::Arc::new(super::view::typography(&cosmic::theme::active()))))
        .into();
    draw_element(el4, "deductum-preview-iced-editor", Size::new(700.0, 300.0));
}

/// Messages the view produces for a click at `at`
pub fn click(app: &App, at: cosmic::iced::Point, size: Size) -> Vec<super::Message> {
    let _turn = ui_turn();
    super::load_fonts();
    let mut renderer = cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia"))).unwrap();
    let mut ui = UserInterface::build(super::view::view(app), size, Cache::default(), &mut renderer);
    let mut messages = Vec::new();
    let cursor = mouse::Cursor::Available(at);
    for event in [
        Event::Mouse(mouse::Event::CursorMoved { position: at }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ] {
        let _ = ui.update(&[event], cursor, &mut renderer, &mut clipboard::Null, &mut messages);
    }
    messages
}

#[test]
fn clicking_blocks_opens_their_source() {
    let app = demo_app(Mode::Literate);
    // the prose paragraph, and the first lemma, as drawn in deductum-literate.png
    let prose = click(&app, cosmic::iced::Point::new(500.0, 215.0), Size::new(1400.0, 900.0));
    assert!(prose.iter().any(|m| matches!(m, super::Message::EditBlock(_))), "{prose:?}");
    // A lemma takes the click itself, as text: it opens with the caret where the click landed
    let mut app = app;
    let lemma = click(&app, cosmic::iced::Point::new(500.0, 300.0), Size::new(1400.0, 900.0));
    // the pointer's place (hover), then the press, which the shown block's own editor never sees
    draw(&app, "deductum-literate-click", Size::new(1400.0, 900.0));
    for m in lemma.iter().filter(|m| matches!(m, super::Message::HoverMove(..))) {
        let _ = cosmic::Application::update(&mut app, m.clone());
    }
    let Some(msg) = lemma.iter().find(|m| matches!(m, super::Message::ViewPress(_))).cloned() else { panic!("{lemma:?}") };
    assert!(!lemma.iter().any(|m| matches!(m, super::Message::BlockViewAction(_, cosmic::widget::text_editor::Action::Click(_)))), "the view itself takes no click");
    let _ = cosmic::Application::update(&mut app, msg);
    let t = app.workspace.as_ref().and_then(|w| w.theory.as_ref()).unwrap();
    let be = t.block_editor.as_ref().expect("the block opens");
    let pos = be.content.cursor().position;
    assert!(pos.line >= 1 && pos.column > 0, "caret at {pos:?}");
    // Between blocks: finishes editing
    let outside = click(&app, cosmic::iced::Point::new(900.0, 820.0), Size::new(1400.0, 900.0));
    assert!(outside.iter().any(|m| matches!(m, super::Message::CloseBlock)), "{outside:?}");
}

#[test]
fn draws_navigation_and_inline_goals() {
    let mut app = demo_app(Mode::Literate);
    app.left = super::LeftMode::Theorems;
    draw(&app, "deductum-theorems", Size::new(1400.0, 900.0));
    app.left = super::LeftMode::Problems;
    app.show_right = false;
    draw(&app, "deductum-problems-inline-goal", Size::new(1100.0, 900.0));
    let mut app = demo_app(Mode::Source);
    app.show_right = false;
    draw(&app, "deductum-source-inline-goal", Size::new(1100.0, 900.0));
    // Wide enough: the goal sits beside the theory
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    draw(&app, "deductum-goal-beside", Size::new(1700.0, 900.0));
}

#[test]
fn sidebars_resize_and_the_header_shows_the_prover() {
    let app = demo_app(Mode::Literate);
    let pressed = click(&app, cosmic::iced::Point::new(292.0, 500.0), Size::new(1400.0, 900.0));
    assert!(pressed.iter().any(|m| matches!(m, super::Message::DragStart(true))), "{pressed:?}");
    let header: cosmic::Element<'_, super::Message> =
        cosmic::widget::row::with_children(super::view::header_center(&app)).padding(8).into();
    draw_element(header, "deductum-header-status", Size::new(700.0, 48.0));
}

#[test]
fn header_groups_keep_their_width() {
    let mut app = demo_app(Mode::Literate);
    let lit = app.mode_model.insert().text("Literate").id();
    app.mode_model.insert().text("Source");
    app.mode_model.activate(lit);
    let start: cosmic::Element<'_, super::Message> = cosmic::widget::row::with_children(super::view::header_start(&app)).into();
    draw_element(start, "deductum-header-start", Size::new(900.0, 48.0));
}

/// `DEDUCTUM_THY=path`: a real theory through both views
#[test]
fn draws_a_real_theory() {
    let Ok(path) = std::env::var("DEDUCTUM_THY") else { return };
    let text = std::fs::read_to_string(&path).unwrap();
    for (mode, name) in [(Mode::Source, "deductum-real-source"), (Mode::Literate, "deductum-real-literate")] {
        let mut app = demo_app(mode);
        let table = Arc::new(Table::fallback());
        let doc = json!({"text": text, "revision": 1, "prover": {}});
        let t = Theory::new(path.clone(), &doc, &*table, Arc::new(HashMap::new()), mode, crate::prover::profile(None));
        app.workspace.as_mut().unwrap().theory = Some(t);
        draw(&app, name, Size::new(1400.0, 900.0));
        if mode == Mode::Literate {
            // and its first prose block, being edited
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            if let Some(i) = t.blocks.iter().position(|b| b.kind == super::theory::Kind::Text) {
                eprintln!("block {i}: {:?} body {:?}", t.blocks[i].kind, t.blocks[i].body);
                let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
                let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
                eprintln!("wrap {:?}\nshown {:?}", be.wrap, be.shown());
                draw(&app, "deductum-real-prose-editing", Size::new(1400.0, 900.0));
            }
        }
    }
}

#[test]
fn compares_superscript_glyphs() {
    let rows = ["A  tr˃ = tr˂   ok˃  st˃", "B  trᐳ = trᐸ   okᐳ  stᐳ", "C  tr⪧ = tr⪦   ok⪧  st⪧", "D  tr﹥ = tr﹤   ok﹥  st﹥", "E  tr⃗ trᵃ  x¹ x²", "F  tr' = tr   ok'  st'", "G  tr⁾ tr⁽ trᵒ"];
    let col = cosmic::widget::column::with_children(rows.iter().map(|r| cosmic::widget::text(r.to_string()).font(cosmic::font::mono()).size(16.0).into()).collect::<Vec<_>>()).padding(10);
    draw_element(col.into(), "deductum-sup-glyphs", Size::new(600.0, 220.0));
}

#[test]
fn scripts_font_loads() {
    super::load_fonts();
    let mut fs = cosmic::iced::advanced::graphics::text::font_system().write().unwrap();
    let found: Vec<String> = fs.raw().db().faces().flat_map(|f| f.families.iter().map(|(n, _)| n.clone())).filter(|n| n.contains("Isabellum Scripts")).collect();
    println!("faces: {found:?}");
    let ncm: Vec<String> = fs.raw().db().faces().flat_map(|f| f.families.iter().map(|(n, _)| n.clone())).filter(|n| n.contains("NewComputer")).collect();
    println!("ncm: {}", ncm.len());
    let data = include_bytes!("../../../res/deductum/fonts/IsabellumScripts.ttf");
    println!("ttf-parser: {:?}", cosmic_text::fontdb::Database::new().len());
    let mut db = cosmic_text::fontdb::Database::new();
    db.load_font_data(data.to_vec());
    println!("own db faces: {:?}", db.faces().map(|f| f.families.clone()).collect::<Vec<_>>());
    assert!(!found.is_empty(), "the scripts font is not in the database");
}


#[test]
fn clicking_prose_puts_the_caret_there() {
    let mut app = demo_app(Mode::Literate);
    let size = Size::new(1400.0, 900.0);
    // The middle of the paragraph's second line
    let msgs = click(&app, cosmic::iced::Point::new(560.0, 187.0), size);
    let pointer = msgs.iter().rev().find(|m| matches!(m, super::Message::ProsePointer(..))).cloned().expect("pointer over prose");
    let open = msgs.iter().find(|m| matches!(m, super::Message::EditBlock(_))).cloned().expect("opens");
    let _ = cosmic::Application::update(&mut app, pointer.clone());
    let _ = cosmic::Application::update(&mut app, open);
    // The editor is laid out when drawn; then the click is replayed
    draw(&app, "deductum-prose-click", size);
    let super::Message::ProsePointer(_, p) = pointer else { unreachable!() };
    let _ = cosmic::Application::update(&mut app, super::Message::BlockClickAt(cosmic::iced::Point::new(p.x, p.y - 2.0)));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let be = t.block_editor.as_ref().unwrap();
    let pos = be.content.cursor().position;
    let len = be.content.text().chars().count();
    let li = super::editor::LineIndex::new(&be.content.text());
    let at = li.offset(&be.content.text(), pos);
    assert!(at > 10 && at + 10 < len, "caret at {at} of {len}");
}

#[test]
fn inserts_and_deletes_blocks() {
    let mut app = demo_app(Mode::Literate);
    let before = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    let _ = cosmic::Application::update(&mut app, super::Message::InsertBlock(Some(1), "section"));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert!(t.local.contains("section \\<open>\\<close>"), "{}", t.local);
    let be = t.block_editor.as_ref().expect("the new section opens");
    assert!(be.wrap.is_some(), "edited as its title");
    let i = t.blocks.iter().position(|b| b.start <= be.start && be.start <= b.end).unwrap();
    draw(&app, "deductum-new-section", Size::new(1400.0, 900.0));
    let _ = cosmic::Application::update(&mut app, super::Message::DeleteBlock(i));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(t.local, before);
}

#[test]
fn vim_normal_mode_shows_a_block_cursor() {
    let mut app = demo_app(Mode::Literate);
    app.vim_enabled = true;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Formal).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    app.vim.mode = super::vim::Mode::Normal;
    // (an editor moves by its laid-out text: drawn, as in the app)
    draw(&app, "deductum-vim-block", Size::new(1400.0, 900.0));
    let _ = cosmic::Application::update(&mut app, super::Message::Vim("0".into(), false));
    draw(&app, "deductum-vim-block", Size::new(1400.0, 900.0));
    let _ = cosmic::Application::update(&mut app, super::Message::Vim("l".into(), false));
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    let c = be.content.cursor();
    assert_eq!(c.selection.map(|s| (s.line, s.column)), Some((0, 1)), "{c:?}");
    assert_eq!((c.position.line, c.position.column), (0, 2));
    // the next motion starts from the caret, not the block's end
    draw(&app, "deductum-vim-block", Size::new(1400.0, 900.0));
    let _ = cosmic::Application::update(&mut app, super::Message::Vim("l".into(), false));
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    assert_eq!(be.content.cursor().selection.map(|s| s.column), Some(2));
    // insert mode: a thin caret
    let _ = cosmic::Application::update(&mut app, super::Message::Vim("i".into(), false));
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    assert!(be.content.cursor().selection.is_none());
}

#[test]
fn a_typed_sledgehammer_leaves_the_proof_unfinished_not_failed() {
    let text = "theory T imports Main begin\n\nlemma \"x = x\"\nproof -\n  show ?thesis\n    sledgehammer\nqed\n\nend\n";
    let at = |s: &str| text[..text.find(s).unwrap()].chars().count();
    let len = |s: &str| s.chars().count();
    let cmd = |s: &str, name: &str, status: &str| json!([at(s), at(s) + len(s), name, status]);
    let prover = json!({
        "revision": 1,
        "status": {"consolidated": true},
        "commands": [
            cmd("theory T imports Main begin", "theory", "finished"),
            cmd("lemma \"x = x\"", "lemma", "finished"),
            cmd("proof -", "proof", "finished"),
            cmd("show ?thesis", "show", "finished"),
            cmd("sledgehammer", "sledgehammer", "finished"),
            cmd("qed", "qed", "failed"),
            cmd("end", "end", "finished"),
        ],
        "messages": [{"start": at("qed"), "end": at("qed") + 3, "kind": "error", "text": "Illegal application of proof command in \"prove\" mode"}],
    });
    let doc = json!({"text": text, "revision": 1, "prover": prover});
    let table = Arc::new(Table::fallback());
    let t = Theory::new("/tmp/T.thy".into(), &doc, &*table, Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None));
    let b = t.blocks.iter().find(|b| b.name == "lemma").expect("the lemma");
    assert_eq!(b.status, super::theory::Status::Failed);
    assert_eq!(t.shown_status(b.start, b.end, b.status), super::theory::Status::Warned);
    assert!(t.messages_in(b.start, b.end).iter().all(|m| t.expected_error(m)));
}

/// The window as the app runs it, without a compositor: the real view, laid out and drawn by the
/// real renderer, with its widget state (focus, carets, scrolling) kept between frames, and
/// mouse and keyboard events going through the widgets as they do in the app
pub struct Harness {
    _turn: std::sync::MutexGuard<'static, ()>,
    renderer: cosmic::Renderer,
    cache: Option<Cache>,
    pub size: Size,
    cursor: mouse::Cursor,
}

impl Harness {
    pub fn new(size: Size) -> Self {
        super::load_fonts();
        let renderer = cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia"))).expect("a headless renderer");
        Self { _turn: ui_turn(), renderer, cache: None, size, cursor: mouse::Cursor::Unavailable }
    }

    /// Events through the widgets; the messages they publish
    pub fn events(&mut self, app: &App, events: &[Event]) -> Vec<super::Message> {
        let mut ui = UserInterface::build(super::view::view(app), self.size, self.cache.take().unwrap_or_default(), &mut self.renderer);
        let mut messages = Vec::new();
        let _ = ui.update(events, self.cursor, &mut self.renderer, &mut clipboard::Null, &mut messages);
        self.cache = Some(ui.into_cache());
        messages
    }

    /// What a focus task does in the app
    pub fn focus(&mut self, app: &App, id: cosmic::widget::Id) {
        let mut ui = UserInterface::build(super::view::view(app), self.size, self.cache.take().unwrap_or_default(), &mut self.renderer);
        ui.operate(&self.renderer, &mut cosmic::iced::advanced::widget::operation::focusable::focus::<()>(id));
        self.cache = Some(ui.into_cache());
    }

    /// An operation over the widgets as laid out (to find where something is)
    pub fn operate(&mut self, app: &App, operation: &mut dyn cosmic::iced::advanced::widget::Operation<()>) {
        let mut ui = UserInterface::build(super::view::view(app), self.size, self.cache.take().unwrap_or_default(), &mut self.renderer);
        ui.operate(&self.renderer, operation);
        self.cache = Some(ui.into_cache());
    }

    /// What an update's task does to the widgets, as the app's runtime would do it now: its
    /// widget operations (a focus, a scroll) run on the view; anything that waits (a timer, the
    /// host) is left. How many operations ran
    pub fn run_task(&mut self, app: &App, task: cosmic::app::Task<super::Message>) -> usize {
        use cosmic::iced::futures::StreamExt;
        let Some(mut stream) = cosmic::iced::runtime::task::into_stream(task) else { return 0 };
        // (as the runtime would: on Tokio, whose timers some tasks wait on; whatever is not ready
        // within a moment is left)
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        let mut ran = 0;
        while let Ok(Some(action)) = runtime.block_on(async { tokio::time::timeout(std::time::Duration::from_millis(30), stream.next()).await }) {
            if let cosmic::iced::runtime::Action::Widget(mut operation) = action {
                let mut ui = UserInterface::build(super::view::view(app), self.size, self.cache.take().unwrap_or_default(), &mut self.renderer);
                ui.operate(&self.renderer, operation.as_mut());
                self.cache = Some(ui.into_cache());
                ran += 1;
            }
        }
        ran
    }

    pub fn move_to(&mut self, app: &App, at: cosmic::iced::Point) -> Vec<super::Message> {
        self.cursor = mouse::Cursor::Available(at);
        self.events(app, &[Event::Mouse(mouse::Event::CursorMoved { position: at })])
    }

    pub fn click(&mut self, app: &App, at: cosmic::iced::Point) -> Vec<super::Message> {
        let mut out = self.move_to(app, at);
        out.extend(self.events(app, &[Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]));
        out.extend(self.events(app, &[Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))]));
        out
    }

    /// A key as the keyboard sends it (a character, or a named key such as Escape)
    pub fn key(&mut self, app: &App, key: cosmic::iced::keyboard::Key, text: Option<&str>) -> Vec<super::Message> {
        use cosmic::iced::keyboard::{Event as K, Location, Modifiers, key::{NativeCode, Physical}};
        let physical = Physical::Unidentified(NativeCode::Unidentified);
        let mut out = self.events(app, &[Event::Keyboard(K::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: physical,
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: text.map(Into::into),
            repeat: false,
        })]);
        out.extend(self.events(app, &[Event::Keyboard(K::KeyReleased { key: key.clone(), modified_key: key, physical_key: physical, location: Location::Standard, modifiers: Modifiers::empty() })]));
        out
    }

    pub fn type_text(&mut self, app: &App, text: &str) -> Vec<super::Message> {
        let mut out = Vec::new();
        for c in text.chars() {
            let s = c.to_string();
            out.extend(self.key(app, cosmic::iced::keyboard::Key::Character(s.clone().into()), Some(&s)));
        }
        out
    }

    /// A frame, saved under `CT_SNAPSHOTS` as `name.png`
    pub fn draw(&mut self, app: &App, name: &str) -> Vec<u8> {
        let theme = cosmic::theme::active();
        let mut ui = UserInterface::build(super::view::view(app), self.size, self.cache.take().unwrap_or_default(), &mut self.renderer);
        // (a moment before any editor took focus: its caret is in the visible half of its blink
        // whatever the time, so a busy machine does not change the frame)
        let earlier = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(3600)).unwrap_or_else(std::time::Instant::now);
        let _ = ui.update(
            &[Event::Window(cosmic::iced::window::Event::RedrawRequested(earlier))],
            self.cursor,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut Vec::new(),
        );
        let text: cosmic::iced::Color = theme.cosmic().on_bg_color().into();
        ui.draw(&mut self.renderer, &theme, &Style { icon_color: text, text_color: text, scale_factor: 1.0 }, self.cursor);
        self.cache = Some(ui.into_cache());
        let physical = cosmic::iced::Size::new(self.size.width as u32, self.size.height as u32);
        let rgba = self.renderer.screenshot(physical, 1.0, theme.cosmic().bg_color().into());
        if let Some(dir) = std::env::var_os("CT_SNAPSHOTS").filter(|d| !d.is_empty()) {
            image::RgbaImage::from_raw(physical.width, physical.height, rgba.clone())
                .expect("a frame")
                .save(std::path::Path::new(&dir).join(format!("{name}.png")))
                .expect("saved");
        }
        rgba
    }
}

#[test]
fn vim_visual_mode_selects_through_the_widgets() {
    let mut app = demo_app(Mode::Literate);
    app.vim_enabled = true;
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Formal).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    h.focus(&app, super::view::block_editor_id());
    h.draw(&app, "vim-visual-0");
    let mut feed = |app: &mut App, h: &mut Harness, keys: &str| {
        for m in h.type_text(app, keys) {
            let _ = cosmic::Application::update(app, m);
        }
        h.draw(app, "vim-visual");
    };
    feed(&mut app, &mut h, "0vll");
    // (the selection itself, kept: its text keeps its syntax colours under the selection's tint)
    h.draw(&app, "vim-visual-1-selected");
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    let sel = be.content.selection();
    eprintln!("mode {:?} cursor {:?} selection {sel:?}", app.vim.mode, be.content.cursor());
    assert_eq!(app.vim.mode, super::vim::Mode::Visual);
    assert_eq!(sel.as_deref(), Some("lem"), "v selects the character under the cursor, as vim does");
    feed(&mut app, &mut h, "y");
    assert_eq!(app.vim.mode, super::vim::Mode::Normal);
    assert_eq!(app.vim.register, "lem");
    feed(&mut app, &mut h, "0vlld");
    let text = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap().content.text();
    assert!(!text.starts_with("lem"), "deleted the selection: {text:?}");
}

#[test]
fn a_tooltip_does_not_take_the_editors_focus() {
    let mut app = demo_app(Mode::Literate);
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Formal).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    h.focus(&app, super::view::block_editor_id());
    h.draw(&app, "focus-0");
    // the prover's tooltip arrives for the open block
    let generation = 7;
    app.workspace.as_mut().unwrap().hover = Some(super::Hover {
        target: super::HoverTarget::Block,
        point: cosmic::iced::Point::new(40.0, 10.0),
        offset: 0,
        generation,
        info: Some(json!({"tooltip": {"items": [{"raw": ":: nat", "text": ":: nat", "spans": []}]}})),
    });
    h.draw(&app, "focus-1-tooltip");
    let before = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    for m in h.type_text(&app, "xyz") {
        let _ = cosmic::Application::update(&mut app, m);
    }
    let after = &app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local;
    assert!(after.contains("xyz") && *after != before, "typing still reaches the editor");
}

#[test]
fn the_open_editor_keeps_focus_as_the_page_changes_around_it() {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    let output = app.workspace.as_mut().unwrap().output.take();
    let mut h = Harness::new(Size::new(1800.0, 900.0));
    let blocks = &app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks;
    let formal: Vec<usize> = blocks.iter().enumerate().filter(|(_, b)| b.kind == super::theory::Kind::Formal).map(|(i, _)| i).collect();
    // another block selected above, with its actions row
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().selected = Some(1);
    h.draw(&app, "keep-0");
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(formal[1]));
    h.focus(&app, super::view::block_editor_id());
    h.draw(&app, "keep-1-open");
    // the proof state arrives: it goes beside the block (the page's shape around the editor changes)
    app.workspace.as_mut().unwrap().output = output;
    h.draw(&app, "keep-2-goal-beside");
    for m in h.type_text(&app, "qq") {
        let _ = cosmic::Application::update(&mut app, m);
    }
    assert!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("qq"), "typing still reaches the editor");
}

#[test]
fn draws_the_goal_beside_the_source() {
    let mut app = demo_app(Mode::Source);
    app.show_right = false;
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let line = t.decoded.text.lines().position(|l| l.contains("apply simp")).unwrap();
    super::editor::place(&mut t.source, cosmic::widget::text_editor::Position { line, column: 4 });
    draw(&app, "deductum-source-goal-beside", Size::new(1800.0, 900.0));
}

#[test]
fn places_the_goal_by_the_construct_in_a_long_proof() {
    let text = "theory Long imports Main begin\n\nlemma long: \"(a::nat) + b + c = c + b + a\"\nproof -\n  have one: \"a + b = b + a\"\n    by simp\n  have two: \"(b + a) + c = c + (b + a)\"\n    by simp\n  have three: \"c + (b + a) = c + b + a\"\n    by simp\n  have four: \"a + b + c = (b + a) + c\"\n    using one\n    by simp\n  have five: \"True\"\n    by simp\n  have six: \"True\"\n    by simp\n  show ?thesis\n    using one two three four\n    by simp\nqed\n\nend\n";
    let mut app = demo_app(Mode::Literate);
    let doc = json!({"text": text, "revision": 1, "prover": prover_json(text)});
    let t = Theory::new("/tmp/Long.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None));
    app.workspace.as_mut().unwrap().theory = Some(t);
    app.show_right = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let at = text[..text.find("have three").unwrap()].chars().count() + 3;
    t.cursor = at;
    let (cs, ce) = t.construct_at(t.block_editor.as_ref().unwrap().start, t.block_editor.as_ref().unwrap().end).expect("a construct");
    let construct: String = text.chars().skip(cs).take(ce - cs).collect();
    assert!(construct.starts_with("have three") && construct.contains("by simp") && !construct.contains("four"), "{construct:?}");
    draw(&app, "goal-long-narrow", Size::new(1100.0, 900.0));
    draw(&app, "goal-long-wide", Size::new(1800.0, 900.0));
}

#[test]
fn prose_is_edited_a_paragraph_at_a_time() {
    let text = "theory P imports Main begin\n\ntext \\<open>The first paragraph says one thing\n  over two lines.\n\n  The second says $x^2$ another.\n\n  The third ends it.\\<close>\n\nlemma \"True\"\n  by simp\n\nend\n";
    let mut app = demo_app(Mode::Literate);
    let doc = json!({"text": text, "revision": 1, "prover": prover_json(text)});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/P.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    t.cursor = text[..text.find("second").unwrap()].chars().count();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    assert_eq!(be.content.text(), "The third ends it.", "opened at the end: the last paragraph");
    // j at the top of the third paragraph goes up to the second, then the first
    app.vim_enabled = true;
    h.focus(&app, super::view::block_editor_id());
    for m in h.type_text(&app, "k") {
        let _ = cosmic::Application::update(&mut app, m);
    }
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    // its maths reads as Unicode, the caret being elsewhere
    assert_eq!(be.content.text().replace('\u{200B}', ""), "The second says x² another.");
    h.draw(&app, "prose-paragraph");
    // the caret moved into the formula: its source, to edit
    for m in h.type_text(&app, "0") {
        let _ = cosmic::Application::update(&mut app, m);
    }
    for _ in 0.."The second says $".len() {
        let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(cosmic::widget::text_editor::Action::Move(cosmic::widget::text_editor::Motion::Right)));
    }
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    assert!(be.content.text().starts_with("The second says $x^2$"), "{:?}", be.content.text());
    h.draw(&app, "prose-paragraph-in-maths");
    // typing in it changes that paragraph only, in the source
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(cosmic::widget::text_editor::Action::Move(cosmic::widget::text_editor::Motion::DocumentStart)));
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(cosmic::widget::text_editor::Action::Edit(cosmic::widget::text_editor::Edit::Insert('Q'))));
    let local = &app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local;
    assert!(local.contains("QThe second") && local.contains("The first paragraph says one thing\n  over two lines."), "{local}");
}

#[test]
fn hovering_a_shown_block_follows_the_pointer() {
    let mut app = demo_app(Mode::Literate);
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    h.draw(&app, "hover-shown");
    // along a row of a shown block, wherever the page puts it: the first row the pointer hovers
    let mut offsets = Vec::new();
    for y in (200..500).step_by(6) {
        offsets.clear();
        for x in (300..1100).step_by(20) {
            for m in h.move_to(&app, cosmic::iced::Point::new(x as f32, y as f32)) {
                let _ = cosmic::Application::update(&mut app, m);
            }
            offsets.push(app.workspace.as_ref().unwrap().hover.as_ref().filter(|h| matches!(h.target, super::HoverTarget::View(_))).map(|h| (h.target, h.offset)));
        }
        if offsets.iter().flatten().count() >= 5 {
            break;
        }
    }
    eprintln!("{offsets:?}");
    let distinct: std::collections::HashSet<_> = offsets.iter().flatten().map(|(_, o)| *o).collect();
    assert!(distinct.len() >= 3, "{offsets:?}");
}

#[test]
fn the_header_stop_button_stops_a_running_sledgehammer() {
    let _turn = ui_turn();
    super::load_fonts();
    let mut app = demo_app(Mode::Literate);
    app.workspace.as_mut().unwrap().activity = Some(json!({
        "theories": [], "prover": {"cpu": 90},
        "running": [{"name": "sledgehammer", "seconds": 14, "path": "/home/me/Demo/Demo.thy"}],
    }));
    app.sessions.values_mut().for_each(|s| s["lifecycle"] = json!("Running"));
    let size = Size::new(900.0, 40.0);
    let mut renderer = cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia"))).unwrap();
    let mut found = None;
    for x in (0..900).step_by(6) {
        let header = cosmic::widget::row::with_children(super::view::header_center(&app));
        let mut ui = UserInterface::build(header, size, Cache::default(), &mut renderer);
        let at = cosmic::iced::Point::new(x as f32, 20.0);
        let mut msgs = Vec::new();
        for e in [Event::Mouse(mouse::Event::CursorMoved { position: at }), Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))] {
            let _ = ui.update(&[e], mouse::Cursor::Available(at), &mut renderer, &mut clipboard::Null, &mut msgs);
        }
        if msgs.iter().any(|m| matches!(m, super::Message::StopTools)) {
            found = Some(x);
            break;
        }
        assert!(!msgs.iter().any(|m| matches!(m, super::Message::Lifecycle("cancel"))), "only sledgehammer running: no general Interrupt");
    }
    assert!(found.is_some(), "a stop button for the running sledgehammer in the header");
    // pressing it: stopping, asked again until it has stopped
    let _ = cosmic::Application::update(&mut app, super::Message::StopTools);
    assert!(app.workspace.as_ref().unwrap().tools_stopping);
}

#[test]
#[ignore]
fn goal_placement_pictures() {
    let text = "theory Long imports Main begin\n\ntext \\<open>A short proof and a long one.\\<close>\n\nlemma short: \"(a::nat) + 0 = a\"\n  by simp\n\nlemma long: \"(a::nat) + b + c = c + b + a\"\nproof -\n  have one: \"a + b = b + a\"\n    by simp\n  have two: \"(b + a) + c = c + (b + a)\"\n    by simp\n  have three: \"c + (b + a) = c + b + a\"\n    by simp\n  have four: \"a + b + c = (b + a) + c\"\n    using one\n    by simp\n  have five: \"True\"\n    by simp\n  have six: \"True\"\n    by simp\n  have seven: \"True\"\n    by simp\n  have eight: \"True\"\n    by simp\n  have nine: \"True\"\n    by simp\n  have ten: \"True\"\n    by simp\n  have eleven: \"True\"\n    by simp\n  show ?thesis\n    using one two three four\n    by simp\nqed\n\nend\n";
    let shot = |name: &str, lemma: &str, at: &str, size: Size, placement: Option<super::view::GoalPlacement>| {
        let mut app = demo_app(Mode::Literate);
        let doc = json!({"text": text, "revision": 1, "prover": prover_json(text)});
        app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/Long.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
        app.show_right = false;
        app.show_sidebar = false;
        app.goal_placement_override = placement;
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.source_start_is(text, lemma)).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        t.cursor = text[..text.find(at).unwrap()].chars().count() + 2;
        draw(&app, name, size);
    };
    let wide = Size::new(1700.0, 900.0);
    let narrow = Size::new(1000.0, 900.0);
    shot("place-1-wide-short", "lemma short", "by simp", wide, None);
    shot("place-2-wide-long", "lemma long", "have eight", wide, None);
    shot("place-3-narrow-short", "lemma short", "by simp", narrow, None);
    shot("place-4-narrow-long", "lemma long", "have eight", narrow, None);
    shot("place-5-wide-long-pinned", "lemma long", "have eight", wide, Some(super::view::GoalPlacement::Pinned));
}

#[test]
fn the_goal_opens_a_gap_in_a_long_proof_and_clicks_below_it_land_right() {
    let text = "theory Long imports Main begin\n\nlemma long: \"(a::nat) + b + c = c + b + a\"\nproof -\n  have one: \"a + b = b + a\"\n    by simp\n  have two: \"(b + a) + c = c + (b + a)\"\n    by simp\n  have three: \"c + (b + a) = c + b + a\"\n    by simp\n  have four: \"True\"\n    by simp\n  have five: \"True\"\n    by simp\n  have six: \"True\"\n    by simp\n  have seven: \"True\"\n    by simp\n  have eight: \"True\"\n    by simp\n  have nine: \"True\"\n    by simp\n  have ten: \"True\"\n    by simp\n  have eleven: \"True\"\n    by simp\n  show ?thesis\n    using one two three\n    by simp\nqed\n\nend\n";
    let mut app = demo_app(Mode::Literate);
    let doc = json!({"text": text, "revision": 1, "prover": prover_json(text)});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/Long.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
    app.show_right = false;
    app.show_sidebar = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = text[..text.find("have three").unwrap()].chars().count() + 3;
    let mut h = Harness::new(Size::new(1000.0, 900.0));
    h.focus(&app, super::view::block_editor_id());
    h.draw(&app, "gap-narrow-long");
    // Click down the left of the proof until the caret lands on "have ten"'s line (below the gap)
    let want = text.lines().skip(2).position(|l| l.contains("have ten")).unwrap();
    let mut landed = None;
    for y in (150..880).step_by(6) {
        let msgs = h.click(&app, cosmic::iced::Point::new(130.0, y as f32));
        // (only what reaches the open editor: a click outside the proof would close it)
        for m in msgs.into_iter().filter(|m| matches!(m, super::Message::BlockAction(_))) {
            let _ = cosmic::Application::update(&mut app, m);
        }
        let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
        if be.content.cursor().position.line == want {
            landed = Some(y);
            break;
        }
        // (keep the caret in "have three" for the gap to stay where it is)
        app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = text[..text.find("have three").unwrap()].chars().count() + 3;
    }
    let y = landed.expect("a click lands on have ten's line");
    // drawn where the click was: the row of "have ten" in the picture is below the gap
    eprintln!("have ten clicked at y = {y}");
    assert!(y > 450, "below the goal's gap, not where it would be without it (y = {y})");
}

/// A reference window: the whole theory read-only at the definition, which is tinted; Ctrl+click
/// in it moves it (Back returns), names in a library theory are looked up by name
#[test]
fn a_reference_window_shows_its_theory_and_moves_on_ctrl_click() {
    use super::{Pending, RefUse};
    let mut text = String::from("theory Lib imports Main begin\n\n");
    text.push_str("datatype 'a tree = Leaf | Node \"'a tree\" 'a \"'a tree\"\n\n");
    text.push_str("primrec mirror :: \"'a tree \\<Rightarrow> 'a tree\" where\n  \"mirror Leaf = Leaf\"\n| \"mirror (Node l x r) = Node (mirror r) x (mirror l)\"\n\n");
    for i in 0..3000 {
        text.push_str(&format!("lemma filler_{i}: \"mirror (mirror t) = t\"\n  by (induct t) simp_all\n\n"));
    }
    text.push_str("end\n");
    let mut app = demo_app(Mode::Source);
    let doc = json!({"ok": true, "doc": {"path": "/isa/src/HOL/Lib.thy", "text": text, "revision": 0, "prover": null, "library": true}});
    let d = json!({"file": "/isa/src/HOL/Lib.thy", "line": 5, "name": "Lib.mirror"});
    let started = std::time::Instant::now();
    let _ = app.on_reply("local".into(), Pending::RefRead(d, None), doc.clone());
    eprintln!("a 9000-line theory read for its window in {:?}", started.elapsed());
    assert_eq!(app.references.len(), 1);
    let id = app.references[0].0;
    let r = &app.references[0].1;
    assert!(r.library);
    assert_eq!((r.line, r.title.as_str()), (4, "Lib.mirror — Lib.thy, line 5"));
    assert!(r.theory.local[r.start..].starts_with("primrec mirror"));
    draw_element(super::view::reference_window(&app, id), "deductum-reference", Size::new(820.0, 420.0));
    // The name under the pointer (the hover looks it up by it)
    let hover_at = r.theory.local.find("Node (mirror r)").unwrap() + 7;
    assert_eq!(super::prover::profile(None).name_at(&r.theory.local, hover_at).as_deref(), Some("mirror"));
    // Ctrl+click on a name found elsewhere: this window moves there, and Back knows where it was
    let info = super::editor::entity_info(&[json!({"raw": "#deductum:entity type\nLib.tree\n/isa/src/HOL/Lib.thy\n3\ntype tree"})]);
    let _ = app.ref_found(id, RefUse::Goto(false), info);
    let before = app.references[0].1.theory.highlight.0.generation;
    let d2 = json!({"file": "/isa/src/HOL/Lib.thy", "line": 3, "name": "Lib.tree"});
    let _ = app.on_reply("local".into(), Pending::RefRead(d2, Some(id)), doc.clone());
    // (its editor keeps its highlighter while the number is the same: a new one, so the new
    // theory's colours, not the old one's)
    assert_ne!(app.references[0].1.theory.highlight.0.generation, before);
    assert_eq!(app.references.len(), 1, "Ctrl+click moves the window, it opens no other");
    let r = &app.references[0].1;
    assert_eq!((r.line, r.back.len()), (2, 1));
    assert_eq!(r.back[0]["name"], "Lib.mirror");
    // Alt+click: another window
    let d3 = json!({"file": "/isa/src/HOL/Lib.thy", "line": 12, "name": "Lib.filler_1"});
    let _ = app.on_reply("local".into(), Pending::RefRead(d3, None), doc);
    assert_eq!(app.references.len(), 2);
    // Typing in it does nothing
    let before = app.references[0].1.theory.local.clone();
    let _ = cosmic::Application::update(&mut app, super::Message::RefAction(id, cosmic::widget::text_editor::Action::Edit(cosmic::widget::text_editor::Edit::Insert('x'))));
    assert_eq!(app.references[0].1.theory.local, before);
}

/// The arrow keys run on past a block's last line into the next block (and back up), as j and k
/// do, without vim
#[test]
fn arrow_keys_move_between_blocks() {
    use cosmic::iced::keyboard::{Key, key::Named};
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    let blocks = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.clone();
    let i = blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let mut h = Harness::new(Size::new(1000.0, 900.0));
    h.focus(&app, super::view::block_editor_id());
    let open = |app: &App| {
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let be = t.block_editor.as_ref().unwrap();
        t.blocks.iter().position(|b| b.start <= be.start && be.start <= b.end).unwrap()
    };
    let press = |h: &mut Harness, app: &mut App, named: Named| {
        for m in h.key(app, Key::Named(named), None) {
            let _ = cosmic::Application::update(app, m);
        }
    };
    let lines = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap().content.line_count();
    for _ in 0..lines {
        press(&mut h, &mut app, Named::ArrowDown);
    }
    assert!(open(&app) > i, "down off the last line opens the next block (still in {})", open(&app));
    let next = open(&app);
    press(&mut h, &mut app, Named::ArrowUp);
    assert!(open(&app) < next, "up off the first line goes back");
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    assert_eq!(be.content.cursor().position.line + 1, be.content.line_count(), "onto the last line of the block above");
}

/// Beside the text, the goal is level with the construct the caret is in, and moves up only as
/// far as it must to stay in the window (not to the top of the window, as an overlay did)
#[test]
fn the_goal_beside_stays_level_with_its_construct() {
    let mut text = String::from("theory Low imports Main begin\n\n");
    for i in 0..7 {
        text.push_str(&format!("lemma filler_{i}: \"(a::nat) + {i} = {i} + a\"\n  by simp\n\n"));
    }
    text.push_str("lemma low: \"(a::nat) + b + c = c + b + a\"\nproof -\n  have one: \"a + b = b + a\"\n    by simp\n  show ?thesis\n    using one\n    by simp\nqed\n\nend\n");
    let mut app = demo_app(Mode::Literate);
    let doc = json!({"text": text, "revision": 1, "prover": prover_json(&text)});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/Low.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
    app.show_right = false;
    app.show_sidebar = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().rposition(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = text[..text.find("show ?thesis").unwrap()].chars().count() + 3;
    let mut h = Harness::new(Size::new(1460.0, 966.0));
    h.focus(&app, super::view::block_editor_id());
    let png = h.draw(&app, "goal-beside-low");
    // Where the goal card's top is: the first row, right of the text column, that is not the
    // page's background
    let img = image::RgbaImage::from_raw(1460, 966, png).expect("the frame's pixels");
    let bg = *img.get_pixel(1440, 5);
    let x = 1100;
    let top = (0..img.height()).find(|&y| img.get_pixel(x, y) != &bg);
    eprintln!("goal card top at {top:?}");
    let top = top.expect("a goal beside") as f32;
    assert!(top > 300.0, "not jumped to the top of the window (top = {top})");
}

/// The source view has a pop-out button at the top right of the proof the caret is in, and the
/// proof window tints the construct the goal is for
#[test]
fn the_source_view_pops_out_its_proof_with_the_construct_tinted() {
    let text = "theory Pop imports Main begin\n\nlemma pop: \"(a::nat) + b + c = c + b + a\"\nproof -\n  have one: \"a + b = b + a\"\n    by simp\n  show ?thesis\n    using one\n    by simp\nqed\n\nend\n";
    let mut app = demo_app(Mode::Source);
    let doc = json!({"text": text, "revision": 1, "prover": prover_json(text)});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/Pop.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Source, crate::prover::profile(None)));
    app.show_right = false;
    app.show_sidebar = false;
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = text[..text.find("have one").unwrap()].chars().count() + 3;
    let mut h = Harness::new(Size::new(1300.0, 700.0));
    h.draw(&app, "source-popout-button");
    // Click about where the button is drawn (at the right of the proof's first line)
    let mut popped = false;
    'find: for y in (70..96).step_by(4) {
        for x in (630..672).step_by(6) {
            if h.click(&app, cosmic::iced::Point::new(x as f32, y as f32)).iter().any(|m| matches!(m, super::Message::PopOut)) {
                popped = true;
                break 'find;
            }
        }
    }
    assert!(popped, "a pop-out button by the proof");
    drop(h);
    let _ = cosmic::Application::update(&mut app, super::Message::PopOut);
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert!(app.popout.is_some() && t.block_editor.as_ref().is_some_and(|be| be.start == t.blocks.iter().find(|b| b.name == "lemma").unwrap().start));
    let px = draw_element(super::view::proof_window(&app), "popout-tinted", Size::new(1000.0, 600.0));
    // The tint: a row in the editor's area that is neither the window's background nor text
    let img = image::RgbaImage::from_raw(1000, 600, px).unwrap();
    let bg = *img.get_pixel(5, 300);
    let tinted = (60..400).filter(|&y| { let p = img.get_pixel(400, y); p != &bg && p[3] == 255 && (p[1] as i32 - bg[1] as i32).abs() < 40 }).count();
    assert!(tinted > 10, "a tinted band in the proof window ({tinted} rows)");
}

/// Vim's block cursor on a character of several bytes (Isabelle symbols, the zero-width spaces
/// prose maths is shown with) covers the whole character: it crashed the window before
#[test]
fn the_vim_block_cursor_covers_a_whole_character() {
    let text = "theory Z imports Main begin\n\nlemma z: \"x \\<Rightarrow> \u{200b}y\"\n  by simp\n\nend\n";
    let mut app = demo_app(Mode::Source);
    let doc = json!({"text": text, "revision": 1, "prover": prover_json(text)});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/Z.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Source, crate::prover::profile(None)));
    app.vim_enabled = true;
    app.vim.mode = super::vim::Mode::Normal;
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let line = t.decoded.text.split('\n').nth(2).unwrap().to_string();
    for (byte, c) in line.char_indices().filter(|(_, c)| c.len_utf8() > 1) {
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        super::editor::place(&mut t.source, cosmic::widget::text_editor::Position { line: 2, column: byte });
        app.vim_caret(true);
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let cur = t.source.cursor();
        assert_eq!(cur.position.column, byte + c.len_utf8(), "the block over {c:?} ends after it");
        // and the caret's offset maps without panicking
        let _ = t.lines.offset(&t.decoded.text, cur.position);
    }
}

/// A claude/codex job's proofs and a tool's findings belong to their cell: another cell does not
/// show them (moving back shows the job's proofs again)
#[test]
fn results_stay_with_their_cell() {
    let mut app = demo_app(Mode::Literate);
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let lemmas: Vec<(usize, usize)> = t.blocks.iter().filter(|b| b.name == "lemma").map(|b| (b.start, b.end)).collect();
    assert!(lemmas.len() >= 2);
    let (a, b) = (lemmas[0], lemmas[1]);
    let ws = app.workspace.as_mut().unwrap();
    ws.fill = super::FillState { job: Some("j".into()), replace: Some((a.0 + 2, a.0 + 8)), status: "checking".into(), ..Default::default() };
    ws.output = Some(json!({"command": {"start": a.0 + 1, "end": a.0 + 5, "name": "sledgehammer"}, "messages": [{"kind": "writeln", "text": "Try this: by simp"}]}));
    ws.theory.as_mut().unwrap().cursor = a.0 + 3;
    assert!(super::view::fill_here(app.workspace.as_ref().unwrap()));
    // to the other lemma
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = b.0 + 3;
    assert!(!super::view::fill_here(app.workspace.as_ref().unwrap()), "not the other cell's proofs");
    let _ = app.schedule_state();
    assert!(app.workspace.as_ref().unwrap().output.is_none(), "not the other cell's findings");
    // and back: the job's proofs again
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = a.0 + 3;
    assert!(super::view::fill_here(app.workspace.as_ref().unwrap()));
}

/// `DEDUCTUM_SYMBOLS=/tmp/isa-symbols.json`: a UTP library theory in a reference window, with
/// the prover's real symbol table (scripts with no Unicode form are private-use characters)
#[test]
fn a_utp_reference_window_with_the_real_symbols() {
    let (Ok(sym), Ok(text)) = (std::env::var("DEDUCTUM_SYMBOLS"), std::fs::read_to_string("/tmp/isa-utp_rea_prog.thy")) else { return };
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(sym).unwrap()).unwrap();
    let table = Arc::new(Table::new(serde_json::from_value(v["symbols"].clone()).unwrap()));
    let mut app = demo_app(Mode::Source);
    if let Some(l) = app.links.first_mut() {
        l.symbols = Some(table.clone());
    }
    let doc = json!({"ok": true, "doc": {"path": "/isa/utp_rea_prog.thy", "text": text, "revision": 0, "prover": null, "library": true}});
    let d = json!({"file": "/isa/utp_rea_prog.thy", "line": 203, "name": "utp_rea_prog.lift_cond_srea_def"});
    let _ = app.on_reply("local".into(), super::Pending::RefRead(d, None), doc);
    let (id, r) = (app.references[0].0, &app.references[0].1);
    let t = &r.theory;
    // the keyword `lemma` of `lemma unrest_lift_cond_srea`: classed as a keyword at its own characters
    let at = t.local.find("lemma unrest_lift_cond_srea").unwrap();
    let at = t.local[..at].chars().count();
    let d0 = t.decoded.to_decoded(at);
    let shown: String = t.decoded.text.chars().skip(d0).take(5).collect();
    eprintln!("decoded there: {shown:?}; classes {:?}", (d0..d0 + 7).map(|i| t.classes.get(i).copied().flatten()).collect::<Vec<_>>());
    // a reference scrolled to line 196 for the picture
    let mut r2 = std::mem::replace(&mut app.references[0].1.line, 0);
    r2 = r2.max(0);
    let _ = r2;
    draw_element(super::view::reference_window(&app, id), "utp-reference", Size::new(960.0, 4400.0));
}

/// Double-click selects a word in the open editor (vim off, or in insert mode), and the selection
/// stays: moving the pointer after it (hover) and the state that follows do not clear it
#[test]
fn double_click_selects_a_word_and_it_stays() {
  for width in [1700.0f32, 1000.0] {
    eprintln!("window {width} wide");
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let mut h = Harness::new(Size::new(width, 900.0));
    h.focus(&app, super::view::block_editor_id());
    let selection = |app: &App| app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap().content.selection();
    // find a point on a word: down the first line until a click lands in the editor
    let mut at = None;
    'find: for y in (60..400).step_by(6) {
        for x in (100..(width as i32 - 100)).step_by(50).map(|x| x as f32) {
            let msgs = h.click(&app, cosmic::iced::Point::new(x, y as f32));
            if msgs.iter().any(|m| matches!(m, super::Message::BlockAction(cosmic::widget::text_editor::Action::Click(_)))) {
                for m in msgs {
                    eprintln!("first: {m:?}");
                    let _ = cosmic::Application::update(&mut app, m);
                }
                at = Some(cosmic::iced::Point::new(x, y as f32));
                break 'find;
            }
        }
    }
    let at = at.expect("a point in the open editor");
    // the second click of a double click
    for m in h.click(&app, at) {
        eprintln!("double: {m:?}");
        let _ = cosmic::Application::update(&mut app, m);
    }
    let picked = selection(&app);
    eprintln!("selected after the double click: {picked:?}");
    assert!(picked.as_ref().is_some_and(|s| !s.trim().is_empty()), "a word selected");
    // the pointer moves on (hover), the state comes
    for dx in [3.0, 6.0, 30.0] {
        for m in h.move_to(&app, cosmic::iced::Point::new(at.x + dx, at.y)) {
            let label = format!("{m:?}");
            let _ = cosmic::Application::update(&mut app, m);
            eprintln!("after {label}: {:?}", selection(&app));
        }
        let g = app.hover_gen;
        let _ = cosmic::Application::update(&mut app, super::Message::HoverTick(g));
        eprintln!("after hover tick: {:?}", selection(&app));
    }
    let _ = app.schedule_state();
    assert_eq!(selection(&app), picked, "the selection stays");
  }
}

/// An error under a block says what went wrong without the goal it lists (the goal view shows
/// that), and its scripts are drawn in the scripts font, not as missing glyphs
#[test]
fn block_errors_are_short_and_draw_their_scripts() {
    assert_eq!(
        super::prover::profile(None).message_summary("Failed to finish proof\\<^here>:\ngoal (2 subgoals):\n 1. \\<And>ok. ok\\<^sup>> = ok"),
        "Failed to finish proof"
    );
    assert_eq!(super::prover::profile(None).message_summary("Undefined fact: \"foo\"\\<^here>"), "Undefined fact: \"foo\"");
}


/// The views in COSMIC's light theme: every text readable on its background (a picture of each;
/// the syntax colours' contrast is checked in the editor's tests)
#[test]
fn the_views_in_light_mode() {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let mut h = Harness::new(Size::new(1500.0, 900.0));
    cosmic::theme::set_active(cosmic::theme::ThemeType::Light);
    h.focus(&app, super::view::block_editor_id());
    h.draw(&app, "light-literate-open");
    let _ = cosmic::Application::update(&mut app, super::Message::CloseBlock);
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().selected = Some(i);
    h.draw(&app, "light-literate-selected");
    let _ = cosmic::Application::update(&mut app, super::Message::SetMode(Mode::Source));
    h.draw(&app, "light-source");
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
}

/// Vim's normal mode shows a block cursor (the character under the caret selected) as soon as a
/// block opens and after a click, not only after a vim key
#[test]
fn vim_normal_mode_shows_a_block_cursor_after_opening_and_clicking() {
    let mut app = demo_app(Mode::Literate);
    app.vim_enabled = true;
    app.vim.mode = super::vim::Mode::Normal;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let block = |app: &App| {
        let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
        be.content.selection().map(|s| s.chars().count())
    };
    assert_eq!(block(&app), Some(1), "a block cursor when the block opens");
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(cosmic::widget::text_editor::Action::Click(cosmic::iced::Point::new(40.0, 4.0))));
    assert_eq!(block(&app), Some(1), "and after a click");
}

/// Beside the text, the goal never runs into it: whatever the page shows (long paragraphs and a
/// heading around a small lemma, a note card, a block with a very long line), nothing reaches
/// past the text column into the gap before the goal, on any row of the window
#[test]
fn nothing_on_the_page_runs_into_the_goal_beside_it() {
    let para = "We prove that the reactive design is healthy, and that its refinement follows from the refinement of its pre and postconditions, which is the main theorem of this section and is used throughout the rest of the development without further comment.";
    let long = "lemma long_line: \"(a::nat) + b + c + d + e + f + g + h + i + j + k + l + m + n + o + p + q + r + s + t + u + v + w + x + y + z = z + y + x + w + v + u + t + s + r + q + p\"\n  by simp\n\n";
    for (name, with_long) in [("narrow-code", false), ("long-line", true)] {
        let mut text = format!("theory Fit imports Main begin\n\nsection \\<open>A rather long heading for a section about the healthiness of reactive designs and contracts\\<close>\n\ntext \\<open>{para}\\<close>\n\nlemma small: \"True\"\n  by simp\n\ntext \\<open>{para} {para}\\<close>\n\n");
        if with_long {
            text.push_str(long);
        }
        text.push_str(&format!("text \\<open>{para}\\<close>\n\nend\n"));
        let mut app = demo_app(Mode::Literate);
        let doc = json!({"text": text, "revision": 1, "prover": prover_json(&text)});
        app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/Fit.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
        app.show_right = false;
        app.show_sidebar = false;
        let small = text.find("lemma small").unwrap();
        app.workspace.as_mut().unwrap().notes = vec![json!({"id": "n1", "label": "Claude", "line": 7, "start": small, "end": small + 5, "text": "This could also be done by auto; it is here to show a note card next to a small lemma in a narrow column.", "replies": []})];
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.start == small || (b.start <= small && small < b.end)).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        let (w, h) = (1500u32, 1400u32);
        let mut hh = Harness::new(Size::new(w as f32, h as f32));
        hh.focus(&app, super::view::block_editor_id());
        let png = hh.draw(&app, &format!("fit-{name}"));
        drop(hh);
        let img = image::RgbaImage::from_raw(w, h, png).unwrap();
        let bg = *img.get_pixel(w - 30, h - 5);
        // the goal card: left of its accent "Goals" button (the leftmost accent pixel in the
        // right half), less its padding
        let accent = |p: &image::Rgba<u8>| p[2] > 170 && p[1] > 150 && p[0] < 150;
        // (the pill filled with the accent, not accent-coloured text such as a note's Resolve)
        let pill = |x: u32, y: u32| (x..x + 20).all(|xx| accent(img.get_pixel(xx, y)));
        let button = (w / 2..w - 40).find(|&x| (0..h).any(|y| pill(x, y))).expect("the goal beside");
        let by = (0..h).find(|&y| pill(button, y)).unwrap();
        // (left from the button, to where the card's background ends)
        let card = (0..button).rev().find(|&x| img.get_pixel(x, by) == &bg).map_or(button, |x| x + 1);
        // the gap before it: background on every row
        let strip = card - super::view::GOAL_GAP_PUB as u32 + 1..card - 1;
        let crossing: Vec<(u32, u32)> = strip.clone().flat_map(|x| (0..h).map(move |y| (x, y))).filter(|&(x, y)| img.get_pixel(x, y) != &bg).take(5).collect();
        eprintln!("{name}: goal card at x = {card}; strip {strip:?}; crossing {crossing:?}");
        assert!(crossing.is_empty(), "{name}: something on the page reaches into the goal's gap at {crossing:?}");
    }
}

/// `CT_SNAPSHOTS`: the literate view in a narrow window and a wide one, for the margins
#[test]
fn margins_pictures() {
    if std::env::var("CT_SNAPSHOTS").is_err() {
        return;
    }
    for (w, name) in [(720.0f32, "margins-narrow"), (1500.0, "margins-wide")] {
        let mut app = demo_app(Mode::Literate);
        app.show_right = false;
        app.show_sidebar = false;
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        let mut h = Harness::new(Size::new(w, 700.0));
        h.draw(&app, name);
    }
}

/// The user's window (about 1344 px wide, code about 85 columns): the margins give way so that
/// the goal goes beside the code, and they give way before the goal does in narrower windows
#[test]
fn the_margins_collapse_before_the_goal_goes_under() {
    let wide_line = "lemma wide: \"timed_contract_wf true\\<^sub>r skip_unstable (timed_assign_final f) \\<and> timed_reactive P\"\n  by simp\n\n";
    let text = format!("theory W imports Main begin\n\n{wide_line}lemma low: \"(a::nat) + b = b + a\"\nproof -\n  show ?thesis\n    by simp\nqed\n\nend\n");
    for (w, beside) in [(1320.0f32, true), (1100.0, true), (1000.0, false)] {
        let mut app = demo_app(Mode::Literate);
        let doc = json!({"text": text, "revision": 1, "prover": prover_json(&text)});
        app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/W.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
        app.show_right = false;
        app.show_sidebar = false;
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().rposition(|b| b.name == "lemma").unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        let mut h = Harness::new(Size::new(w, 900.0));
        h.draw(&app, &format!("collapse-{}", w as u32));
        // (read while this test has the UI's turn: other tests' views set it too)
        assert_eq!(super::view::last_wide(), beside, "{w} px: goal beside = {beside}");
        drop(h);
    }
}

/// Blocks go when there is nothing left of them: Backspace in an emptied block, `dd` on a block's
/// only line (vim), and an empty block left behind
#[test]
fn empty_blocks_go() {
    use cosmic::widget::text_editor::{Action as A, Edit as E};
    let text = "theory G imports Main begin\n\nlemma one: \"True\" by simp\n\nlemma two: \"(a::nat) + b = b + a\"\n  by simp\n\nend\n";
    let fresh = |text: &str| {
        let mut app = demo_app(Mode::Literate);
        let doc = json!({"text": text, "revision": 1, "prover": prover_json(text)});
        app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/G.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
        app
    };
    let local = |app: &App| app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    let index = |app: &App, name: &str| {
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        t.blocks.iter().position(|b| t.local.chars().skip(b.start).take(name.len()).collect::<String>() == name).unwrap()
    };
    // 1. Backspace in an emptied block: it goes, and the caret is at the end of the one before
    let mut app = fresh(text);
    let i = index(&app, "lemma two");
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(A::SelectAll));
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(A::Edit(E::Delete)));
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(A::Edit(E::Backspace)));
    assert_eq!(local(&app), "theory G imports Main begin\n\nlemma one: \"True\" by simp\n\nend\n", "the emptied block and its blank line gone");
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let be = t.block_editor.as_ref().expect("the block before it open");
    assert!(t.local[..].chars().skip(be.start).collect::<String>().starts_with("lemma one"));
    // 2. dd on a block's only line (vim normal mode)
    let mut app = fresh(text);
    app.vim_enabled = true;
    app.vim.mode = super::vim::Mode::Normal;
    let i = index(&app, "lemma one");
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let _ = cosmic::Application::update(&mut app, super::Message::Vim("d".into(), false));
    let _ = cosmic::Application::update(&mut app, super::Message::Vim("d".into(), false));
    assert!(!local(&app).contains("lemma one"), "dd took the one-line block away: {:?}", local(&app));
    assert!(local(&app).contains("lemma two"));
    // 3. An empty block, left, goes: a new text block, left at once
    let mut app = fresh(text);
    let before = local(&app);
    let i = index(&app, "lemma one");
    let _ = cosmic::Application::update(&mut app, super::Message::InsertBlock(Some(i), "text"));
    assert_ne!(local(&app), before);
    let _ = cosmic::Application::update(&mut app, super::Message::CloseBlock);
    assert_eq!(local(&app), before, "the empty text block went when left");
    // and an emptied proof block, left for another block
    let mut app = fresh(text);
    let i = index(&app, "lemma two");
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(A::SelectAll));
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(A::Edit(E::Delete)));
    let j = index(&app, "lemma one");
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(j));
    assert_eq!(local(&app), "theory G imports Main begin\n\nlemma one: \"True\" by simp\n\nend\n");
}


/// Clicking a shown block opens it with the caret where the click was; in vim's normal mode that
/// caret is the block cursor (placing it at the click used to leave an ordinary one)
#[test]
fn a_block_opened_by_a_click_has_vim_block_cursor() {
    let mut app = demo_app(Mode::Literate);
    app.vim_enabled = true;
    app.vim.mode = super::vim::Mode::Normal;
    let lemma = click(&app, cosmic::iced::Point::new(500.0, 300.0), Size::new(1400.0, 900.0));
    for m in lemma.iter().filter(|m| matches!(m, super::Message::HoverMove(..))) {
        let _ = cosmic::Application::update(&mut app, m.clone());
    }
    let Some(press) = lemma.iter().find(|m| matches!(m, super::Message::ViewPress(_))).cloned() else { panic!("{lemma:?}") };
    let _ = cosmic::Application::update(&mut app, press);
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().expect("the block opens");
    assert_eq!(be.content.selection().map(|s| s.chars().count()), Some(1), "a block cursor where the click was");
    let _ = cosmic::Application::update(&mut app, super::Message::BlockClickAt(cosmic::iced::Point::new(30.0, 4.0)));
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    assert_eq!(be.content.selection().map(|s| s.chars().count()), Some(1), "and after the click is placed in the open block");
}

/// Down from a text block into the code block after it, and up back again: with vim's j/k, and
/// with the arrow keys
#[test]
fn moving_from_a_text_block_into_a_code_block() {
    for vim in [true, false] {
        let mut app = demo_app(Mode::Literate);
        app.vim_enabled = vim;
        app.vim.mode = super::vim::Mode::Normal;
        let blocks = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.clone();
        let text = blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(text));
        let open = |app: &App| {
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            t.block_editor.as_ref().and_then(|be| t.blocks.iter().position(|b| b.start <= be.start && be.start <= b.end))
        };
        let down = |app: &mut App| {
            let m = if vim { super::Message::Vim("j".into(), false) } else { super::Message::LineMove(true) };
            let _ = cosmic::Application::update(app, m);
        };
        let mut reached = None;
        for n in 0..12 {
            down(&mut app);
            eprintln!("vim {vim}: after {} down: open block {:?}", n + 1, open(&app));
            if open(&app).is_some_and(|i| i > text) {
                reached = open(&app);
                break;
            }
        }
        assert!(reached.is_some_and(|i| blocks[i].kind == super::theory::Kind::Formal), "vim {vim}: down from the text block reaches the code block ({reached:?})");
    }
}

/// As the window does it: a long paragraph that wraps, drawn, and the keys pressed
#[test]
fn moving_from_a_wrapped_text_block_into_a_code_block_with_keys() {
    use cosmic::iced::keyboard::{Key, key::Named};
    let para = "A is the reactive assumption, Q the intermediate commitment, and R the final commitment. The intended well-formed inputs satisfy the existing RC/RR conditions, and Q must not constrain the final program state. These requirements are recorded separately rather than silently normalising the supplied predicates.";
    let text = format!("theory P imports Main begin\n\nlemma before: \"True\"\n  by simp\n\ntext \\<open>{para}\\<close>\n\nlemma after: \"(a::nat) + b = b + a\"\n  by simp\n\nend\n");
    for (vim, up) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut app = demo_app(Mode::Literate);
        let doc = json!({"text": text, "revision": 1, "prover": prover_json(&text)});
        app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/P.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
        app.show_right = false;
        app.show_sidebar = false;
        app.vim_enabled = vim;
        app.vim.mode = super::vim::Mode::Normal;
        let ti = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(ti));
        let mut h = Harness::new(Size::new(900.0, 800.0));
        h.focus(&app, super::view::block_editor_id());
        let open = |app: &App| {
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            t.block_editor.as_ref().and_then(|be| t.blocks.iter().position(|b| b.start <= be.start && be.start <= b.end))
        };
        let mut reached = false;
        for n in 0..14 {
            let msgs = match (vim, up) {
                (true, false) => h.key(&app, Key::Character("j".into()), Some("j")),
                (true, true) => h.key(&app, Key::Character("k".into()), Some("k")),
                (false, false) => h.key(&app, Key::Named(Named::ArrowDown), None),
                (false, true) => h.key(&app, Key::Named(Named::ArrowUp), None),
            };
            let names: Vec<String> = msgs.iter().map(|m| format!("{m:?}").chars().take(40).collect()).collect();
            for m in msgs {
                let _ = cosmic::Application::update(&mut app, m);
            }
            h.draw(&app, "wrapped-move");
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            let caret = t.block_editor.as_ref().map(|be| be.content.cursor().position);
            eprintln!("vim {vim} up {up}: {}: {names:?} -> block {:?} caret {caret:?}", n + 1, open(&app));
            if open(&app).is_some_and(|i| if up { i < ti } else { i > ti }) {
                reached = true;
                break;
            }
        }
        assert!(reached, "vim {vim} up {up}: from the wrapped paragraph to the lemma");
    }
}

/// A text block of two paragraphs, opened by a click on its first (so narrowed to it): down
/// goes through the second paragraph and on into the lemma after the block
#[test]
fn moving_from_a_paragraph_through_the_next_into_a_code_block() {
    use cosmic::iced::keyboard::{Key, key::Named};
    let para = "A is the reactive assumption, Q the intermediate commitment, and R the final commitment. The intended well-formed inputs satisfy the existing RC/RR conditions.";
    let text = format!("theory P imports Main begin\n\ntext \\<open>{para}\n\n{para}\\<close>\n\nlemma after: \"(a::nat) + b = b + a\"\n  by simp\n\nend\n");
    for vim in [false, true] {
        let mut app = demo_app(Mode::Literate);
        let doc = json!({"text": text, "revision": 1, "prover": prover_json(&text)});
        app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/P.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
        app.show_right = false;
        app.show_sidebar = false;
        app.vim_enabled = vim;
        app.vim.mode = super::vim::Mode::Normal;
        let ti = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(ti));
        let _ = cosmic::Application::update(&mut app, super::Message::BlockClickAt(cosmic::iced::Point::new(5.0, 2.0)));
        let wrap = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap().wrap.is_some();
        eprintln!("vim {vim}: narrowed to a paragraph: {wrap}");
        let mut h = Harness::new(Size::new(900.0, 800.0));
        h.focus(&app, super::view::block_editor_id());
        let open = |app: &App| {
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            t.block_editor.as_ref().and_then(|be| t.blocks.iter().position(|b| b.start <= be.start && be.start <= b.end))
        };
        let mut reached = false;
        for n in 0..16 {
            let msgs = if vim { h.key(&app, Key::Character("j".into()), Some("j")) } else { h.key(&app, Key::Named(Named::ArrowDown), None) };
            for m in msgs {
                let _ = cosmic::Application::update(&mut app, m);
            }
            h.draw(&app, "para-move");
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            let be = t.block_editor.as_ref();
            eprintln!("vim {vim}: {}: block {:?} start {:?} caret {:?}", n + 1, open(&app), be.map(|b| b.start), be.map(|b| b.content.cursor().position));
            if open(&app).is_some_and(|i| i > ti) {
                reached = true;
                break;
            }
        }
        assert!(reached, "vim {vim}: through the paragraphs into the lemma");
    }
}


/// A read-only view (the goal, a tool's output) focused by a click takes copying and selecting
/// only: j, k, the arrows, d and Enter go on to the page's block navigation
#[test]
fn a_focused_goal_view_leaves_navigation_keys_alone() {
    use cosmic::iced::keyboard::{Key, Modifiers, key::{Named, NativeCode, Physical}};
    use cosmic::widget::text_editor::{Binding as B, KeyPress, Status};
    let press = |key: Key, modifiers: Modifiers, text: Option<&str>| KeyPress {
        key: key.clone(),
        modified_key: key,
        physical_key: Physical::Unidentified(NativeCode::Unidentified),
        modifiers,
        text: text.map(Into::into),
        status: Status::Focused { is_hovered: false },
    };
    for (key, text) in [(Key::Character("j".into()), Some("j")), (Key::Character("k".into()), Some("k")), (Key::Character("d".into()), Some("d")), (Key::Named(Named::ArrowDown), None), (Key::Named(Named::ArrowUp), None), (Key::Named(Named::Enter), None)] {
        assert!(super::view::read_only_binding(press(key.clone(), Modifiers::empty(), text)).is_none(), "{key:?} goes on to the page");
    }
    assert!(matches!(super::view::read_only_binding(press(Key::Character("c".into()), Modifiers::CTRL, Some("c"))), Some(B::Copy)), "Ctrl+C copies");
    assert!(matches!(super::view::read_only_binding(press(Key::Named(Named::ArrowDown), Modifiers::SHIFT, None)), Some(B::Select(_))), "Shift+Down selects");
}

/// The toolbar's toggle: off, the goal is under the proof even with room beside, and the page
/// is centred
#[test]
fn the_goal_beside_can_be_turned_off() {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let mut h = Harness::new(Size::new(1320.0, 800.0));
    h.draw(&app, "beside-on");
    assert!(super::view::last_wide(), "beside with room for it");
    let _ = cosmic::Application::update(&mut app, super::Message::ToggleGoalBeside);
    let png = h.draw(&app, "beside-off");
    let off = super::view::last_wide();
    let _ = cosmic::Application::update(&mut app, super::Message::ToggleGoalBeside);
    let on_png = h.draw(&app, "beside-on-again");
    let again = super::view::last_wide();
    assert!(!off, "under the proof when turned off");
    assert!(again, "beside again when turned back on");
    // and drawn beside: something right of the text column
    let img2 = image::RgbaImage::from_raw(1320, 800, on_png).unwrap();
    let bg2 = *img2.get_pixel(12, 400);
    let right_of_text = (1000..1290).any(|x| (60..780).step_by(4).any(|y| img2.get_pixel(x, y) != &bg2));
    assert!(right_of_text, "the goal drawn beside the text again");
    // centred: as much room left of the page as right of it
    let img = image::RgbaImage::from_raw(1320, 800, png).unwrap();
    let bg = *img.get_pixel(12, 400);
    // (inside the window's edges and its scroll bar)
    let cols: Vec<u32> = (8..1290).filter(|&x| (40..780).step_by(2).any(|y| img.get_pixel(x, y) != &bg)).collect();
    let (l, r) = (cols[0], 1306 - cols[cols.len() - 1]);
    eprintln!("page from {} to {}: left {l}, right {r}", cols[0], cols[cols.len() - 1]);
    assert!((l as i32 - r as i32).abs() < 60, "centred (left {l}, right {r})");
}


/// With no block open, the goal is beside the selected proof (and comes back there when the
/// toggle is turned on again)
#[test]
fn the_goal_is_beside_the_selected_block_with_none_open() {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    {
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        t.selected = Some(i);
        t.cursor = t.blocks[i].start + 2;
    }
    let mut h = Harness::new(Size::new(1320.0, 800.0));
    let has_goal_beside = |png: Vec<u8>| {
        let img = image::RgbaImage::from_raw(1320, 800, png).unwrap();
        let bg = *img.get_pixel(12, 400);
        (1000..1290).any(|x| (60..780).step_by(4).any(|y| img.get_pixel(x, y) != &bg))
    };
    assert!(has_goal_beside(h.draw(&app, "selected-beside")), "the goal beside the selected proof");
    let _ = cosmic::Application::update(&mut app, super::Message::ToggleGoalBeside);
    let off = has_goal_beside(h.draw(&app, "selected-off"));
    let _ = cosmic::Application::update(&mut app, super::Message::ToggleGoalBeside);
    let again = has_goal_beside(h.draw(&app, "selected-again"));
    assert!(!off, "nothing beside when turned off");
    assert!(again, "beside again when turned back on");
}

/// With the goal beside on, the page keeps the goal's column whether or not there is a goal
/// (moving onto prose never changes the layout); off, the centred column is a readable width
#[test]
fn the_layout_keeps_its_place_and_the_centred_column_is_wide_enough() {
    let edges = |png: Vec<u8>| {
        let img = image::RgbaImage::from_raw(1320, 800, png).unwrap();
        let bg = *img.get_pixel(12, 400);
        let cols: Vec<u32> = (8..1290).filter(|&x| (40..780).step_by(2).any(|y| img.get_pixel(x, y) != &bg)).collect();
        (cols[0], cols[cols.len() - 1])
    };
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let mut h = Harness::new(Size::new(1320.0, 800.0));
    let with_goal = edges(h.draw(&app, "keep-with-goal"));
    let output = app.workspace.as_mut().unwrap().output.take();
    let without = edges(h.draw(&app, "keep-without-goal"));
    assert_eq!(with_goal.0, without.0, "the page starts in the same place with and without a goal");
    app.workspace.as_mut().unwrap().output = output;
    let _ = cosmic::Application::update(&mut app, super::Message::ToggleGoalBeside);
    let centred = edges(h.draw(&app, "keep-centred"));
    // (the column as laid out: what is drawn in it need not reach its edge, a ragged paragraph
    // or a note as wide as its cell)
    let column = super::view::last_column();
    let _ = cosmic::Application::update(&mut app, super::Message::ToggleGoalBeside);
    eprintln!("with goal {with_goal:?}, without {without:?}, centred {centred:?}, column {column}");
    assert!(column >= 540.0, "the centred column is wide enough ({column}, drawn {centred:?})");
}

/// Beside the block being edited, with no goal: what can be inserted there, by context
#[test]
fn the_insert_palette_follows_the_context() {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let ti = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(ti));
    let ctx = |app: &App| super::insert::insert_context(app.workspace.as_ref().unwrap().theory.as_ref().unwrap());
    assert_eq!(ctx(&app), Some(super::prover::InsertContext::Prose));
    let mut h = Harness::new(Size::new(1320.0, 800.0));
    h.draw(&app, "palette-prose");
    // the caret inside the maths
    {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        let src = be.decoded.text.clone();
        let at = src[..src.find('$').expect("maths in the demo's prose") + 2].chars().count();
        let text = be.content.text();
        let li = super::editor::LineIndex::new(&text);
        super::editor::place(&mut be.content, li.position(&text, at));
    }
    assert_eq!(ctx(&app), Some(super::prover::InsertContext::Symbols), "inside $…$: symbols");
    let _ = cosmic::Application::update(&mut app, super::Message::PaletteQuery("forall".into()));
    h.draw(&app, "palette-symbols");
    // a template, inserted at the caret, in the source
    let before = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    let _ = cosmic::Application::update(&mut app, super::Message::InsertSymbol("@{term \"\"}".into()));
    let after = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    assert!(after.contains("@{term \"\"}") && after.len() > before.len(), "inserted in the source");
}

/// The lemma's top, with the text before it open, then with the lemma itself open (moving down
/// from the text into it): no jump (the editing hints are in the status bar, not under the
/// open block, and notes stay under an open block)
#[test]
fn opening_the_next_block_does_not_move_it() {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let blocks = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.clone();
    let ti = blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
    let li = blocks.iter().position(|b| b.name == "lemma").unwrap();
    let mut h = Harness::new(Size::new(1320.0, 900.0));
    // the lemma's top: the first height, down its left, where the pointer is over it (its shown
    // view, or its open editor)
    let top = |h: &mut Harness, app: &App, open: bool| -> Option<u32> {
        h.draw(app, if open { "jump-lemma-open" } else { "jump-text-open" });
        (150..800).step_by(2).find(|&y| {
            (150..700).step_by(30).any(|x| {
                h.move_to(app, cosmic::iced::Point::new(x as f32, y as f32)).iter().any(|m| match m {
                    super::Message::HoverMove(super::HoverTarget::View(i), _) => !open && *i == li,
                    super::Message::HoverMove(super::HoverTarget::Block, _) => open,
                    _ => false,
                })
            })
        })
    };
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(ti));
    let with_text = top(&mut h, &app, false);
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(li));
    let with_lemma = top(&mut h, &app, true);
    eprintln!("lemma top: text open {with_text:?}, lemma open {with_lemma:?}");
    let (a, b) = (with_text.expect("the lemma drawn"), with_lemma.expect("the lemma drawn"));
    // (within a little: the paragraph being edited shows its antiquotations as source, so it can
    // be a line longer than rendered; the row of editing hints under the open block, which moved
    // it 23 px, is gone)
    assert!(a.abs_diff(b) <= 12, "the lemma stays where it was as it opens ({a} then {b})");
}

/// Up from a block whose first line wraps: one press per row, then out to the block above (the
/// editor's remembered column must not cost an extra press on the top row); down likewise
#[test]
fn up_and_down_leave_a_block_from_its_edge_rows() {
    use cosmic::iced::keyboard::{Key, key::Named};
    let long = "lemma wrapped: \"timed_contract_wf true\\<^sub>r skip_unstable (timed_assign_final f) \\<and> timed_reactive P \\<and> timed_reactive Q \\<and> timed_reactive R\"";
    let text = format!("theory U imports Main begin\n\nlemma above: \"True\"\n  by simp\n\n{long}\n  by simp\n\nlemma below: \"True\"\n  by simp\n\nend\n");
    for up in [true, false] {
        let mut app = demo_app(Mode::Literate);
        let doc = json!({"text": text, "revision": 1, "prover": prover_json(&text)});
        app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/U.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
        app.show_right = false;
        app.show_sidebar = false;
        let wi = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma" && b.start > text.find("lemma wrapped").unwrap().saturating_sub(1)).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(wi));
        let mut h = Harness::new(Size::new(900.0, 800.0));
        h.focus(&app, super::view::block_editor_id());
        h.draw(&app, "edge-rows");
        // the caret: on "by simp" (going up), or at the start of the wrapped first line (going down)
        {
            let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
            let pos = if up { cosmic::widget::text_editor::Position { line: 1, column: 6 } } else { cosmic::widget::text_editor::Position { line: 0, column: 3 } };
            super::editor::place(&mut be.content, pos);
        }
        let open = |app: &App| {
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            t.block_editor.as_ref().and_then(|be| t.blocks.iter().position(|b| b.start <= be.start && be.start <= b.end))
        };
        let mut presses = 0;
        for _ in 0..8 {
            presses += 1;
            let msgs = h.key(&app, Key::Named(if up { Named::ArrowUp } else { Named::ArrowDown }), None);
            for m in msgs {
                let _ = cosmic::Application::update(&mut app, m);
            }
            h.draw(&app, "edge-rows");
            if open(&app) != Some(wi) {
                break;
            }
        }
        eprintln!("up {up}: left the block after {presses} presses");
        // up: by simp -> row 2 -> row 1 -> out; down: row 1 -> row 2 -> by simp -> out
        assert!(presses <= 3 + usize::from(!up), "up {up}: out of the block within a press per row ({presses} presses)");
        assert!(open(&app) != Some(wi), "up {up}: left the block");
    }
}

/// The palette wraps the selection (emphasis around a selected word), or inserts at the caret
/// with the caret between (a term to type)
#[test]
fn the_palette_wraps_the_selection() {
    use cosmic::widget::text_editor::{Action as A, Cursor, Position};
    let para = "We prove that addition with zero is the identity\n  for every natural number.";
    let text = format!("theory W imports Main begin\n\ntext \\<open>{para}\\<close>\n\nend\n");
    let fresh = || {
        let mut app = demo_app(Mode::Literate);
        let doc = json!({"text": text, "revision": 1, "prover": {}});
        app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/W.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
        let ti = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(ti));
        app
    };
    let local = |app: &App| app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    let shown = |app: &App| app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap().content.text();
    // 1. a selected word, emphasised
    let mut app = fresh();
    {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        let t = be.content.text();
        let s = t.find("addition").unwrap();
        let li = super::editor::LineIndex::new(&t);
        let (a, b) = (li.position(&t, t[..s].chars().count()), li.position(&t, t[..s].chars().count() + 8));
        be.content.move_to(Cursor { position: b, selection: Some(a) });
    }
    let _ = cosmic::Application::update(&mut app, super::Message::Wrap("\\<^emph>‹".into(), "›".into()));
    assert!(local(&app).contains("\\<^emph>\\<open>addition\\<close> with zero"), "{:?}", local(&app));
    assert!(local(&app).contains("identity\n  for every"), "the rest of the paragraph as it was: {:?}", local(&app));
    // 2. at the caret: both, the caret between
    let mut app = fresh();
    {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        super::editor::place(&mut be.content, Position { line: 0, column: 3 });
    }
    let _ = cosmic::Application::update(&mut app, super::Message::Wrap("@{term \"".into(), "\"}".into()));
    assert!(local(&app).contains("We @{term \"\"}prove"), "{:?}", local(&app));
    let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
    let t = shown(&app);
    let at = super::editor::LineIndex::new(&t).offset(&t, be.content.cursor().position);
    assert_eq!(t.chars().skip(at).take(2).collect::<String>(), "\"}", "the caret between the quotes");
    // 3. a word selected as a double click selects it (the cursor cannot say where it is)
    let mut app = fresh();
    {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        super::editor::place(&mut be.content, Position { line: 0, column: 16 });
        be.content.perform(A::SelectWord);
    }
    let sel = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap().content.selection();
    eprintln!("word selection: {sel:?}");
    if sel.as_deref().is_some_and(|s| !s.trim().is_empty()) {
        let _ = cosmic::Application::update(&mut app, super::Message::Wrap("\\<^bold>‹".into(), "›".into()));
        assert!(local(&app).contains(&format!("\\<^bold>\\<open>{}\\<close>", sel.unwrap().trim())), "{:?}", local(&app));
    }
}


/// Each click in the palette is one entry of the history: a wrap's two insertions (after, then
/// before the selection) are one step, apart from the typing around it
#[test]
fn a_palette_click_is_one_history_entry() {
    use cosmic::widget::text_editor::{Action as A, Cursor, Edit as E};
    let text = "theory W imports Main begin\n\ntext \\<open>We prove that addition with zero is the identity.\\<close>\n\nend\n";
    let mut app = demo_app(Mode::Literate);
    let doc = json!({"text": text, "revision": 1, "prover": {}});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/W.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
    let ti = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(ti));
    {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        let t = be.content.text();
        let s = t[..t.find("addition").unwrap()].chars().count();
        let li = super::editor::LineIndex::new(&t);
        be.content.move_to(Cursor { position: li.position(&t, s + 8), selection: Some(li.position(&t, s)) });
    }
    let _ = cosmic::Application::update(&mut app, super::Message::Wrap("\\<^emph>‹".into(), "›".into()));
    // then a typed character, which begins the next step
    let _ = cosmic::Application::update(&mut app, super::Message::BlockAction(A::Edit(E::Insert('!'))));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let texts: Vec<&String> = t.steps.iter().map(|(_, text)| text).collect();
    // no step holds only half the wrap
    for text in &texts {
        let after = text.contains("addition\\<close>");
        let before = text.contains("\\<^emph>\\<open>addition");
        assert_eq!(after, before, "a step with half the wrap: {text:?}");
    }
    assert!(texts.iter().any(|text| text.contains("\\<^emph>\\<open>addition\\<close>") && !text.contains('!')), "the wrap, a step of its own: {texts:?}");
}

/// In the paragraph being edited, markup reads as it renders (bold, emphasis, an antiquotation
/// as what it holds) unless the caret is inside it, where it is source again
#[test]
fn markup_renders_unless_the_caret_is_in_it() {
    let table = Table::fallback();
    let src = "The \\<^bold>\\<open>finite\\<close> case and \\<^emph>\\<open>this\\<close> one, @{term \"x + y\"} too.";
    let d = table.decode(src);
    let text: Vec<char> = d.text.chars().collect();
    let far = Some(0);
    let shown: Vec<char> = super::editor::shown_with_markup(super::prover::profile(None), &d, true, far, &table).chars().collect();
    assert_eq!(shown.len(), text.len(), "one shown character for each");
    let visible: String = shown.iter().filter(|c| **c != '\u{200B}').collect();
    assert!(visible.contains("The finite case and this one, x + y too."), "rendered: {visible:?}");
    let marks = super::prover::profile(None).prose_markup(&d, far);
    assert!(marks.iter().any(|m| m.style.2 == super::editor::Hl::Bold), "bold");
    assert!(marks.iter().any(|m| m.style.2 == super::editor::Hl::Emph), "emphasis");
    // the caret inside the bold: that one is source again, the others still rendered
    let at = d.text.find("finite").map(|b| d.text[..b].chars().count() + 2);
    let inside: String = super::editor::shown_with_markup(super::prover::profile(None), &d, true, at, &table).chars().filter(|c| *c != '\u{200B}').collect();
    assert!(inside.contains("‹finite›") && inside.contains("and this one"), "source where the caret is: {inside:?}");
}

/// Lists are contextual: on an item's line the palette offers the next item, elsewhere a list
#[test]
fn list_items_are_offered_in_a_list() {
    let text = "theory L imports Main begin\n\ntext \\<open>Some prose.\n\n\\<^item> one\n\\<^item> two\\<close>\n\nend\n";
    let mut app = demo_app(Mode::Literate);
    let doc = json!({"text": text, "revision": 1, "prover": {}});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/L.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
    let ti = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(ti));
    let table = Table::fallback();
    let at = |app: &mut App, line: usize| {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        let shown = be.content.text();
        eprintln!("line {line}: {:?}", shown.split('\n').nth(line));
        super::editor::place(&mut be.content, cosmic::widget::text_editor::Position { line, column: 3 });
        super::insert::list_entries(app.workspace.as_ref().unwrap().theory.as_ref().unwrap(), &table)
    };
    let end = |app: &mut App| {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        be.content.perform(cosmic::widget::text_editor::Action::Move(cosmic::widget::text_editor::Motion::DocumentEnd));
        super::insert::list_entries(app.workspace.as_ref().unwrap().theory.as_ref().unwrap(), &table)
    };
    let _ = at;
    // (the block opens on its last paragraph, the items)
    let second = end(&mut app);
    let _ = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().adjacent_paragraph(false, &table);
    let first = end(&mut app);
    eprintln!("first {first:?}, second {second:?}");
    assert!(first.first().is_some_and(|e| e.label == "bullet list"), "prose is not a list");
    assert!(second.first().is_some_and(|e| e.label == "next item" && e.before.contains("\\<^item>")), "an item's line is in a list");
}

/// The last block selected: its own actions add a block after it, and the page's closing row of
/// the same actions is not drawn as well
#[test]
fn no_duplicate_actions_after_the_last_block() {
    use cosmic::iced::Point;
    let text = "theory One imports Main begin\n\ntext \\<open>Only prose here.\\<close>\n\nend\n";
    let mut app = demo_app(Mode::Literate);
    let doc = json!({"text": text, "revision": 1, "prover": {}});
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/tmp/One.thy".into(), &doc, &Table::fallback(), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None)));
    app.show_right = false;
    app.show_sidebar = false;
    let ti = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().selected = Some(ti);
    let mut h = Harness::new(Size::new(900.0, 600.0));
    h.draw(&app, "last-selected");
    // the "+ Text" actions: count the rows that offer them, down the page
    let mut ys = Vec::new();
    for y in (40..590).step_by(4) {
        if (20..400).step_by(12).any(|x| h.click(&app, Point::new(x as f32, y as f32)).iter().any(|m| matches!(m, super::Message::InsertBlock(_, "text")))) {
            ys.push(y);
        }
    }
    drop(h);
    // (rows: runs of heights a row apart at most)
    let rows = ys.windows(2).filter(|w| w[1] - w[0] > 12).count() + usize::from(!ys.is_empty());
    eprintln!("heights offering + Text: {ys:?} ({rows} rows)");
    assert_eq!(rows, 1, "one row of actions after the last block ({ys:?})");
}


/// `CT_SNAPSHOTS`: the window across its modes, for looking at: the literate and source views,
/// dark and light, the goal beside on and off, widths from a laptop's half screen to a wide
/// monitor, the sidebars open and closed, vim on and off, with a paragraph open and with a proof
/// open (its goal shown), the demo's note and error present throughout. Named
/// `matrix-<view>-<theme>-<width>-<open>[-sidebars][-under][-vim]`
#[test]
fn frame_matrix() {
    if std::env::var("CT_SNAPSHOTS").is_err() {
        return;
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Open {
        Proof,
        Prose,
    }
    struct Case {
        mode: Mode,
        dark: bool,
        width: f32,
        open: Open,
        sidebars: bool,
        beside: bool,
        vim: bool,
    }
    let case = |mode, dark, width, open| Case { mode, dark, width, open, sidebars: false, beside: true, vim: false };
    let mut cases = Vec::new();
    for dark in [true, false] {
        for width in [800.0, 1000.0, 1100.0, 1400.0, 1800.0] {
            cases.push(case(Mode::Literate, dark, width, Open::Proof));
        }
        for width in [1100.0, 1400.0] {
            cases.push(case(Mode::Literate, dark, width, Open::Prose));
        }
        for width in [1400.0, 1800.0] {
            cases.push(Case { sidebars: true, ..case(Mode::Literate, dark, width, Open::Proof) });
        }
        cases.push(Case { beside: false, ..case(Mode::Literate, dark, 1400.0, Open::Proof) });
        for width in [1000.0, 1400.0] {
            cases.push(case(Mode::Source, dark, width, Open::Proof));
        }
        cases.push(Case { sidebars: true, ..case(Mode::Source, dark, 1800.0, Open::Proof) });
    }
    cases.push(Case { vim: true, ..case(Mode::Literate, true, 1400.0, Open::Proof) });
    cases.push(Case { vim: true, ..case(Mode::Literate, true, 1400.0, Open::Prose) });
    for c in cases {
        let mut app = demo_app(Mode::Literate);
        app.show_sidebar = c.sidebars;
        app.show_right = c.sidebars;
        app.window_width = c.width;
        app.vim_enabled = c.vim;
        let blocks = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.clone();
        let i = match c.open {
            Open::Proof => blocks.iter().position(|b| b.name == "lemma").unwrap(),
            Open::Prose => blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap(),
        };
        if c.mode == Mode::Source {
            let _ = cosmic::Application::update(&mut app, super::Message::SetMode(Mode::Source));
        } else {
            let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        }
        let mut h = Harness::new(Size::new(c.width, 860.0));
        cosmic::theme::set_active(if c.dark { cosmic::theme::ThemeType::Dark } else { cosmic::theme::ThemeType::Light });
        super::view::set_goal_beside(c.beside);
        if c.mode == Mode::Literate {
            h.focus(&app, super::view::block_editor_id());
        }
        let name = format!(
            "matrix-{}-{}-{}-{}{}{}{}",
            if c.mode == Mode::Source { "source" } else { "literate" },
            if c.dark { "dark" } else { "light" },
            c.width as u32,
            if c.open == Open::Proof { "proof" } else { "prose" },
            if c.sidebars { "-sidebars" } else { "" },
            if c.beside { "" } else { "-under" },
            if c.vim { "-vim" } else { "" },
        );
        h.draw(&app, &name);
        super::view::set_goal_beside(true);
        cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    }
}

/// A tool typed where the prover has not got to yet (its imports still being checked, as when a
/// session has just started) says what it waits for, not that there is no proof state
#[test]
fn a_tool_waiting_for_imports_says_so() {
    let mut app = demo_app(Mode::Literate);
    {
        let ws = app.workspace.as_mut().unwrap();
        let t = ws.theory.as_mut().unwrap();
        let at = t.local.find("sorry").expect("the demo has a sorry");
        let text = format!("{}sledgehammer{}", &t.local[..at], &t.local[at + 5..]);
        let len = text.chars().count();
        t.local = text.clone();
        t.confirmed = text;
        let start = t.local[..at].chars().count();
        t.prover.commands = vec![super::theory::Command { start, end: start + 12, name: "sledgehammer".into(), status: super::theory::Status::Unprocessed }];
        t.cursor = (start + 12).min(len);
        ws.output = None;
        ws.goals.messages.clear();
    }
    let ws = app.workspace.as_ref().unwrap();
    let note = super::view::waiting_note(ws).expect("a note");
    assert!(note.starts_with("sledgehammer starts when the prover reaches this line."), "{note}");
    assert!(note.contains("Base.thy 45%"), "{note}");
    draw(&app, "tool-waiting-for-imports", Size::new(1400.0, 900.0));
}

/// The caret in the open block's editor, `plus` characters after the first `needle` (as the
/// editor shows the text, decoded)
pub fn caret_in_block(app: &mut App, needle: &str, plus: usize) {
    let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
    let text = be.content.text();
    let at = text[..text.find(needle).unwrap_or_else(|| panic!("{needle:?} in {text:?}"))].chars().count() + plus;
    let li = super::editor::LineIndex::new(&text);
    super::editor::place(&mut be.content, li.position(&text, at));
}

/// Where the insert bar above the goal is (the page unscrolled), if it is drawn: where it is laid
/// out, moved as far as the goal beside the text is moved to its place
pub fn insert_bar_bounds(h: &mut Harness, app: &App) -> Option<cosmic::iced::Rectangle> {
    struct Find(Option<cosmic::iced::Rectangle>);
    impl cosmic::iced::advanced::widget::Operation<()> for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn cosmic::iced::advanced::widget::Operation<()>)) {
            operate(self);
        }
        fn container(&mut self, id: Option<&cosmic::widget::Id>, bounds: cosmic::iced::Rectangle) {
            if id == Some(&super::insert::insert_bar_id()) {
                self.0 = Some(bounds);
            }
        }
    }
    let mut find = Find(None);
    h.operate(app, &mut find);
    let beside = app.workspace.as_ref().unwrap().theory.as_ref().is_some_and(|t| t.mode == Mode::Literate) && super::view::goal_beside_on() && super::view::last_wide();
    find.0.map(|b| if beside { b + super::gap::goal_moved() } else { b })
}

/// The bar's chip that inserts `before`, clicked through the widgets (found along the bar's row,
/// left to right): the messages the click sent
pub fn click_chip(h: &mut Harness, app: &App, before: &str) -> Vec<super::Message> {
    let bar = insert_bar_bounds(h, app).expect("an insert bar");
    // (its row: the search field's height, at the bar's top)
    let y = bar.y + 4.0 + 16.0;
    let mut x = bar.x + 4.0;
    while x < bar.x + bar.width {
        let msgs = h.click(app, cosmic::iced::Point::new(x, y));
        if msgs.iter().any(|m| matches!(m, super::Message::Wrap(b, _) if b == before)) {
            return msgs;
        }
        x += 4.0;
    }
    panic!("no chip for {before:?} on the bar at {bar:?}");
}

/// The insert bar above the goal, in an Isabelle proof: symbols inside the statement's quotes,
/// proof steps at a step's place, the whole palette when opened; dark and light, the goal beside
/// and under, 800 and 1400 px wide; in the source view and the proof window too
#[test]
fn the_insert_bar_sits_above_the_isabelle_goal() {
    use super::prover::InsertContext;
    let ctx = |app: &App| super::insert::insert_context(app.workspace.as_ref().unwrap().theory.as_ref().unwrap());
    for dark in [true, false] {
        for (w, h, beside) in [(1400.0, 900.0, true), (1400.0, 900.0, false), (800.0, 760.0, true), (800.0, 760.0, false)] {
            let mut harness = Harness::new(Size::new(w, h));
            cosmic::theme::set_active(if dark { cosmic::theme::ThemeType::Dark } else { cosmic::theme::ThemeType::Light });
            super::view::set_goal_beside(beside);
            let mut app = demo_app(Mode::Literate);
            app.show_right = false;
            app.show_sidebar = false;
            app.window_width = w;
            let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
            let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
            let name = |what: &str| format!("bar-isabelle-{what}-{}-{}{}", if dark { "dark" } else { "light" }, w as u32, if beside { "-beside" } else { "-under" });
            // inside the statement's quotes: symbols
            caret_in_block(&mut app, "x::nat", 0);
            assert_eq!(ctx(&app), Some(InsertContext::Symbols));
            harness.draw(&app, &name("term"));
            assert!(insert_bar_bounds(&mut harness, &app).is_some(), "{}: a bar above the goal", name("term"));
            // at a step's place: steps
            caret_in_block(&mut app, "apply", 0);
            assert_eq!(ctx(&app), Some(InsertContext::Proof));
            harness.draw(&app, &name("step"));
            // opened: the whole palette, the goals below it
            let _ = cosmic::Application::update(&mut app, super::Message::InsertBar(true));
            harness.draw(&app, &name("step-open"));
            caret_in_block(&mut app, "x::nat", 0);
            harness.draw(&app, &name("term-open"));
            // a search: by name, and by the character itself
            let _ = cosmic::Application::update(&mut app, super::Message::PaletteQuery("subseteq".into()));
            harness.draw(&app, &name("term-search"));
            let _ = cosmic::Application::update(&mut app, super::Message::InsertBar(false));
            harness.draw(&app, &name("term-search-row"));
            let _ = cosmic::Application::update(&mut app, super::Message::PaletteQuery(String::new()));
            cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
            super::view::set_goal_beside(true);
        }
    }
    // The source view's goal pane, and the proof window
    // (the theme and the toggle are the process's: set while this test has the UI's turn)
    let mut harness = Harness::new(Size::new(1400.0, 900.0));
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    super::view::set_goal_beside(true);
    let mut app = demo_app(Mode::Source);
    app.show_right = false;
    app.show_sidebar = false;
    {
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let line = t.decoded.text.lines().position(|l| l.contains("lemma add_zero")).unwrap();
        let column = t.decoded.text.lines().nth(line).unwrap().find("x::").unwrap();
        super::editor::place(&mut t.source, cosmic::widget::text_editor::Position { line, column });
        t.cursor = DEMO.find("x::nat").unwrap();
    }
    assert_eq!(ctx(&app), Some(InsertContext::Symbols), "in the source view, inside the quotes");
    harness.draw(&app, "bar-isabelle-source-1400");
    assert!(insert_bar_bounds(&mut harness, &app).is_some(), "a bar above the source view's goal");
    drop(harness);
    let _ = cosmic::Application::update(&mut app, super::Message::PopOut);
    assert!(app.popout.is_some());
    caret_in_block(&mut app, "apply", 0);
    draw_element(super::view::proof_window(&app), "bar-isabelle-proof-window", Size::new(1000.0, 600.0));
    assert_eq!(ctx(&app), Some(InsertContext::Proof), "the proof window's block, in the source view");
}

/// A chip on the bar inserts at the caret (its symbol, in the source as its `\<name>`), wraps a
/// selection with a pair (a cartouche), is one entry of the history, and leaves the editor
/// focused with its caret after what was inserted: typing goes on there
#[test]
fn a_chip_on_the_bar_inserts_at_the_caret_and_keeps_the_focus() {
    use cosmic::widget::text_editor::{Cursor, Position};
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    super::view::set_goal_beside(true);
    h.focus(&app, super::view::block_editor_id());
    caret_in_block(&mut app, "x + 0", 0);
    h.draw(&app, "chip-0");
    let local = |app: &App| app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    // 1. a symbol at the caret, clicked through the widgets
    let msgs = click_chip(&mut h, &app, "∃");
    for m in msgs {
        let task = cosmic::Application::update(&mut app, m);
        h.run_task(&app, task);
    }
    assert!(local(&app).contains("x::nat. \\<exists>x + 0"), "the symbol at the caret, as its name in the source: {:?}", local(&app));
    // typing goes on in the editor, after the symbol
    for m in h.type_text(&app, "y") {
        let _ = cosmic::Application::update(&mut app, m);
    }
    assert!(local(&app).contains("\\<exists>yx + 0"), "focus and caret kept: {:?}", local(&app));
    h.draw(&app, "chip-1-inserted");
    // 2. a pair around a selection: one step of the history
    {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        let text = be.content.text();
        let s = text[..text.find("x + 0").unwrap()].chars().count();
        let li = super::editor::LineIndex::new(&text);
        let line = li.position(&text, s).line;
        let col = li.position(&text, s).column;
        be.content.move_to(Cursor { position: Position { line, column: col + 5 }, selection: Some(Position { line, column: col }) });
    }
    // (found by searching for its character: the pair is the bar's first match)
    let _ = cosmic::Application::update(&mut app, super::Message::PaletteQuery("‹".into()));
    h.draw(&app, "chip-2-search");
    let msgs = click_chip(&mut h, &app, "‹");
    for m in msgs {
        let task = cosmic::Application::update(&mut app, m);
        h.run_task(&app, task);
    }
    assert!(local(&app).contains("\\<open>x + 0\\<close>"), "the selection wrapped: {:?}", local(&app));
    for m in h.type_text(&app, "z") {
        let _ = cosmic::Application::update(&mut app, m);
    }
    assert!(local(&app).contains("\\<open>x + 0\\<close>z"), "the caret after the wrapped selection, focused: {:?}", local(&app));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    for (_, text) in &t.steps {
        assert_eq!(text.contains("\\<open>x + 0"), text.contains("x + 0\\<close>"), "a step with half the wrap: {text:?}");
    }
    h.draw(&app, "chip-2-wrapped");
    super::view::set_goal_beside(true);
}

/// Scroll the literate page with the wheel until the open block's top is a third of the way
/// down the window, as a reader would to work on it
pub fn scroll_to_open_block(h: &mut Harness, app: &App) {
    struct Find(Option<cosmic::iced::Rectangle>, cosmic::iced::Vector);
    impl cosmic::iced::advanced::widget::Operation<()> for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn cosmic::iced::advanced::widget::Operation<()>)) {
            operate(self);
        }
        fn focusable(&mut self, id: Option<&cosmic::widget::Id>, bounds: cosmic::iced::Rectangle, _state: &mut dyn cosmic::iced::advanced::widget::operation::Focusable) {
            if id == Some(&super::view::block_editor_id()) {
                self.0 = Some(bounds);
            }
        }
        fn scrollable(&mut self, id: Option<&cosmic::widget::Id>, _b: cosmic::iced::Rectangle, _c: cosmic::iced::Rectangle, translation: cosmic::iced::Vector, _s: &mut dyn cosmic::iced::advanced::widget::operation::Scrollable) {
            if id == Some(&super::view::literate_scroll_id()) {
                self.1 = translation;
            }
        }
    }
    let mut find = Find(None, cosmic::iced::Vector::ZERO);
    h.operate(app, &mut find);
    let Some(block) = find.0 else { return };
    let by = block.y - find.1.y - h.size.height / 3.0;
    h.move_to(app, cosmic::iced::Point::new(h.size.width / 4.0, h.size.height / 2.0));
    h.events(app, &[cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::WheelScrolled { delta: cosmic::iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -by } })]);
}

/// Opening a selected block does not move its goal down: the insert bar's row is there, saying
/// how to use it, while the block is only selected, and is filled when it opens
#[test]
fn opening_a_selected_block_keeps_the_goal_where_it_was() {
    for beside in [true, false] {
        let mut harness = Harness::new(Size::new(1400.0, 900.0));
        super::view::set_goal_beside(beside);
        let mut app = demo_app(Mode::Literate);
        app.show_right = false;
        app.show_sidebar = false;
        app.window_width = 1400.0;
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
        app.workspace.as_mut().unwrap().theory.as_mut().unwrap().selected = Some(i);
        harness.draw(&app, &format!("bar-selected-{}", if beside { "beside" } else { "under" }));
        let closed = insert_bar_bounds(&mut harness, &app).expect("the bar's row is there while the block is only selected");
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        harness.draw(&app, &format!("bar-opened-{}", if beside { "beside" } else { "under" }));
        let open = insert_bar_bounds(&mut harness, &app).expect("the bar once the block is open");
        assert!((closed.y - open.y).abs() <= 2.0 && (closed.height - open.height).abs() <= 2.0, "beside={beside}: {closed:?} → {open:?}");
    }
    super::view::set_goal_beside(true);
}
