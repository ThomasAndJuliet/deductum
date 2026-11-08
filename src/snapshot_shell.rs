// SPDX-License-Identifier: GPL-3.0-only

//! The shell drawn headless (see `snapshot`): the settings page, the header and the status bar in
//! several states, the left sidebar with Isabelle and Lean sessions side by side, the new-session
//! cards for a folder both backends recognise, and the Project tab. `CT_SNAPSHOTS=<dir>` keeps the
//! frames. The pictures are for looking at; the assertions check what the widgets send.

use std::collections::HashMap;

use cosmic::iced::{Length, Point, Size};
use cosmic::prelude::*;
use cosmic::widget;
use serde_json::{Value, json};

use super::editor::Mode;
use super::prover::capabilities::ProjectAction;
use super::shell::{self, Msg, Run};
use super::snapshot::{demo_app, render, ui_turn};
use super::{App, LeftMode, Link, Message, link};

/// A frame in the given theme, with the turn held so that no other test draws meanwhile; the
/// element is built in that theme, as the app builds its view in the theme in use
fn draw_in<'a>(light: bool, build: impl FnOnce() -> Element<'a, Message>, name: &str, size: Size) -> Vec<u8> {
    let _turn = ui_turn();
    cosmic::theme::set_active(if light { cosmic::theme::ThemeType::Light } else { cosmic::theme::ThemeType::Dark });
    let frame = render(build(), name, size);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    frame
}

/// The header as the window draws it: start, centre and end, in a bar
fn header(app: &App) -> Element<'_, Message> {
    let row = widget::row::with_capacity(5)
        .spacing(4)
        .align_y(cosmic::iced::Alignment::Center)
        .push(widget::row::with_children(shell::header_start(app)).spacing(2).align_y(cosmic::iced::Alignment::Center))
        .push(widget::Space::new().width(Length::Fill))
        .push(widget::row::with_children(shell::header_center(app)).align_y(cosmic::iced::Alignment::Center))
        .push(widget::Space::new().width(Length::Fill))
        .push(widget::row::with_children(shell::header_end(app)).spacing(2).align_y(cosmic::iced::Alignment::Center));
    widget::container(row)
        .padding([4, 8])
        .height(Length::Fixed(48.0))
        .width(Length::Fill)
        .class(cosmic::theme::Container::custom(|t| cosmic::iced::widget::container::Style {
            background: Some(cosmic::iced::Color::from(t.cosmic().bg_color()).into()),
            ..Default::default()
        }))
        .into()
}

/// The whole window: the header over the view
fn window(app: &App) -> Element<'_, Message> {
    widget::column::with_capacity(2).push(header(app)).push(super::view::view(app)).into()
}

fn connected(name: &str, key: &str, hello: Option<Value>) -> Link {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    std::mem::forget(receiver);
    Link {
        target: link::Target { key: key.into(), destination: (key != "local").then(|| key.to_string()) },
        name: name.into(),
        sender: Some(sender),
        status: "connected".into(),
        hello,
        client_id: None,
        symbols: None,
    }
}

fn hello() -> Value {
    json!({"backends": [
        {"backend": "isabelle", "label": "Isabelle", "documents": [".thy"], "project_files": ["ROOT", "ROOTS"],
         "toolchains": [{"id": "/opt/Isabelle2025-CyPhyAssure", "label": "Isabelle2025-CyPhyAssure", "ok": true},
                        {"id": "/opt/Isabelle2024", "label": "Isabelle2024", "ok": false, "problem": "No Deductum backend for Isabelle2024"}]},
        {"backend": "lean", "label": "Lean", "documents": [".lean"], "project_files": ["lakefile.toml", "lean-toolchain"],
         "toolchains": [{"id": "leanprover/lean4:v4.19.0", "label": "Lean v4.19.0", "ok": true}]}],
        "memory_budget_gb": 24, "home": "/home/me", "version": "0.1.0"})
}

/// The demo app with sessions of both backends on two machines: Isabelle and Lean on one folder,
/// a fork under its session, a paused and a stopped one, and a failed one on a machine that is
/// not connected
fn mixed_app() -> App {
    let mut app = demo_app(Mode::Literate);
    let symbols = app.links[0].symbols.clone();
    app.links = vec![connected("au-laptop", "local", Some(hello())), {
        let mut neo = connected("neo", "neo", None);
        neo.sender = None;
        neo.status = "disconnected: ssh: connect to host neo port 22: No route to host".into();
        neo
    }];
    app.links[0].symbols = symbols;
    let s = &mut app.sessions;
    s.clear();
    let mut put = |machine: &str, v: Value| {
        s.insert((machine.to_string(), v["id"].as_str().unwrap().to_string()), v);
    };
    put("local", json!({"id": "s1", "name": "Demo", "folder": "/home/me/Research/Demo", "backend": "isabelle", "lifecycle": "Running",
        "toolchain": {"id": "/opt/Isabelle2025-CyPhyAssure", "label": "Isabelle2025"}, "documents": [".thy"],
        "activity": {"running": 1, "failed": 1}, "clients": [{"kind": "agent", "label": "Claude"}, {"kind": "app"}]}));
    put("local", json!({"id": "s3", "name": "DSRV", "folder": "/home/me/Research/Demo", "backend": "lean", "lifecycle": "Running",
        "toolchain": {"id": "leanprover/lean4:v4.19.0", "label": "Lean v4.19.0"}, "documents": [".lean"],
        "activity": {"running": 0, "failed": 0}, "memory_mb": 1840, "clients": []}));
    put("local", json!({"id": "s4", "name": "Demo (Claude's copy)", "folder": "/home/me/.local/state/deductum/forks/s4", "backend": "isabelle", "lifecycle": "Running",
        "toolchain": {"label": "Isabelle2025"}, "parent": {"session": "s1", "branch": "deductum/s4"}, "owner": {"label": "Claude"},
        "activity": {"running": 0, "failed": 0}, "clients": [{"kind": "agent", "label": "Claude"}]}));
    put("local", json!({"id": "s2", "name": "UTP", "folder": "/home/me/Isabelle/UTP", "backend": "isabelle", "lifecycle": "Paused",
        "toolchain": {"label": "Isabelle2025"}, "activity": {}, "clients": []}));
    put("local", json!({"id": "s5", "name": "CausalSemantics with a long name", "folder": "/home/me/Research/Repositories/DSRVLean", "backend": "lean", "lifecycle": "Stopped",
        "toolchain": {"label": "Lean v4.32.1"}, "activity": {}, "clients": []}));
    put("neo", json!({"id": "n1", "name": "tock-Circus", "folder": "/home/twright/Isabelle/tock-Circus", "backend": "isabelle", "lifecycle": "Failed",
        "toolchain": {"label": "Isabelle2025"}, "activity": {"failed": 3}, "clients": []}));
    let ws = app.workspace.as_mut().unwrap();
    ws.info.as_mut().unwrap()["folder"] = json!("/home/me/Research/Demo");
    ws.info.as_mut().unwrap()["theories"] = json!([
        {"path": "/home/me/Research/Demo/ROOT", "status": null},
        {"path": "/home/me/Research/Demo/Demo.thy", "status": {"failed": 1}},
        {"path": "/home/me/Research/Demo/More.thy", "status": {"consolidated": true}, "dirty": true},
        {"path": "/home/me/Research/Demo/lib/Base.thy", "status": {"running": 2}}]);
    ws.caps.project_files = vec!["ROOT".into(), "ROOTS".into()];
    ws.theory.as_mut().unwrap().path = "/home/me/Research/Demo/Demo.thy".into();
    app.left_width = 300.0;
    app
}

// ----- the settings page

#[test]
fn the_settings_page_in_both_themes_narrow_and_wide() {
    let mut app = demo_app(Mode::Literate);
    app.links = vec![connected("au-laptop", "local", None), {
        let mut neo = connected("neo", "neo", None);
        neo.sender = None;
        neo.status = "disconnected: No route to host".into();
        neo
    }];
    app.shell.settings_open = true;
    app.shell.settings.llm.claude_model = "opus".into();
    app.workspace.as_mut().unwrap().activity.as_mut().unwrap()["listeners"] = json!([{"key": "claude:c1", "label": "tock"}]);
    for (light, theme) in [(false, "dark"), (true, "light")] {
        app.show_sidebar = false;
        draw_in(light, || window(&app), &format!("shell-settings-{theme}-narrow"), Size::new(800.0, 900.0));
        app.show_sidebar = true;
        draw_in(light, || window(&app), &format!("shell-settings-{theme}-wide"), Size::new(1400.0, 900.0));
    }
    // Scrolled to its foot: the Lean, Machines and About sections
    app.show_sidebar = false;
    let mut h = super::snapshot::Harness::new(Size::new(800.0, 900.0));
    let _ = h.move_to(&app, Point::new(400.0, 500.0));
    let _ = h.events(&app, &[cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::WheelScrolled { delta: cosmic::iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -3000.0 } })]);
    h.draw(&app, "shell-settings-dark-foot");
}

#[test]
fn settings_change_at_once_and_vim_has_left_the_header() {
    let mut app = demo_app(Mode::Literate);
    app.shell.settings_open = true;
    // (the settings are written and published for the whole process: no other test changes them meanwhile)
    let turn = super::snapshot::ui_turn();
    // Its togglers send what they set
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::Vim(true)));
    assert!(app.vim_enabled && app.shell.settings.editing.vim);
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::IndentWidth(4)));
    assert_eq!(app.shell.settings.editing.indent_width, 4);
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::KeywordPause(4000)));
    assert_eq!(app.shell.settings.keyword_pause(), std::time::Duration::from_millis(4000));
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::Loogle(super::settings::Loogle::Local)));
    // written at once, where tests keep theirs
    let written = super::settings::Settings::load();
    assert_eq!(written.lean.loogle, super::settings::Loogle::Local);
    assert!(written.editing.vim);
    assert_eq!(super::settings::loogle(), super::settings::Loogle::Local, "and published for the Lean search");
    drop(turn);
    // the header has no Vim or Settings button: Settings is in the Edit menu (Ctrl+,)
    app.shell.settings_open = false;
    let found: Vec<Message> = (0..1400).step_by(8).flat_map(|x| clicks(header(&app), Point::new(x as f32, 24.0), Size::new(1400.0, 48.0))).collect();
    assert!(!found.iter().any(|m| matches!(m, Message::ToggleVim | Message::Shell(Msg::ToggleSettings))), "{found:?}");
    let menus = super::menubar::menus(&app);
    let edit = &menus.iter().find(|(t, _)| *t == "Edit").unwrap().1;
    assert!(matches!(super::menubar::find(edit, "Settings"), Some(super::menubar::Row::Item { message: Message::Shell(Msg::ToggleSettings), keys: "Ctrl+,", .. })));
    // Opening a session leaves the settings for the document
    app.shell.settings_open = true;
    let _ = cosmic::Application::update(&mut app, Message::OpenSession("local".into(), "s2".into()));
    assert!(!app.shell.settings_open);
}

/// Messages an element sends for a click
fn clicks(element: Element<'_, Message>, at: Point, size: Size) -> Vec<Message> {
    use cosmic::iced::advanced::clipboard;
    use cosmic::iced::advanced::renderer::Headless;
    use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
    use cosmic::iced::{Event, Font, Pixels, mouse};
    let _turn = ui_turn();
    super::load_fonts();
    let mut renderer = cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia"))).unwrap();
    let mut ui = UserInterface::build(element, size, Cache::default(), &mut renderer);
    let mut messages = Vec::new();
    for event in [
        Event::Mouse(mouse::Event::CursorMoved { position: at }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ] {
        let _ = ui.update(&[event], mouse::Cursor::Available(at), &mut renderer, &mut clipboard::Null, &mut messages);
    }
    messages
}

// ----- the header and the status bar

#[test]
fn the_header_and_status_bar_in_their_states() {
    let mut app = mixed_app();
    app.show_right = false;
    // checking, with a failing proof and a long-running one
    draw_in(false, || header(&app), "shell-header-checking", Size::new(1400.0, 48.0));
    app.window_width = 1000.0;
    draw_in(false, || header(&app), "shell-header-checking-narrow", Size::new(1000.0, 48.0));
    app.window_width = 1400.0;
    // checked, nothing failing
    {
        let ws = app.workspace.as_mut().unwrap();
        ws.activity = Some(json!({"prover": {"cpu": 2, "rss_mb": 3120}, "running": [], "theories": []}));
        let t = ws.theory.as_mut().unwrap();
        t.prover.status = json!({"finished": 15, "unprocessed": 0, "running": 0, "failed": 0, "consolidated": true});
    }
    draw_in(false, || header(&app), "shell-header-checked", Size::new(1400.0, 48.0));
    // paused, in the light theme too
    app.sessions.get_mut(&("local".to_string(), "s1".to_string())).unwrap()["lifecycle"] = json!("Paused");
    draw_in(false, || header(&app), "shell-header-paused", Size::new(1400.0, 48.0));
    draw_in(true, || header(&app), "shell-header-paused-light", Size::new(1400.0, 48.0));
    app.sessions.get_mut(&("local".to_string(), "s1".to_string())).unwrap()["lifecycle"] = json!("Running");

    // The status bar: nothing open, a block open without and with vim, a message and a lost machine
    // (on the page, as the window draws it)
    fn bar(app: &App) -> Element<'_, Message> {
        widget::container(shell::status_bar(app).unwrap_or_else(|| widget::Space::new().into()))
            .width(Length::Fill)
            .height(Length::Fill)
            .class(cosmic::theme::Container::custom(|t| cosmic::iced::widget::container::Style { background: Some(super::view::page_color(t).into()), ..Default::default() }))
            .into()
    }
    draw_in(false, || bar(&app), "shell-status-idle", Size::new(1400.0, 28.0));
    let lemma = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, Message::EditBlock(lemma));
    draw_in(false, || bar(&app), "shell-status-editing", Size::new(1400.0, 28.0));
    app.vim_enabled = true;
    app.vim.mode = super::vim::Mode::Insert;
    draw_in(false, || bar(&app), "shell-status-vim-insert", Size::new(1400.0, 28.0));
    app.status = Some("Not sent: neo is not connected".into());
    app.links[0].sender = None;
    app.links[0].status = "disconnected: broken pipe".into();
    draw_in(false, || bar(&app), "shell-status-message-offline", Size::new(1400.0, 28.0));
    draw_in(true, || bar(&app), "shell-status-message-offline-light", Size::new(1400.0, 28.0));
    // and in the window, narrow
    app.links[0].sender = connected("x", "local", None).sender;
    app.vim.mode = super::vim::Mode::Normal;
    app.window_width = 1100.0;
    draw_in(false, || window(&app), "shell-window-vim-normal", Size::new(1100.0, 760.0));
    app.window_width = 1400.0;
    app.vim_enabled = false;
    app.status = None;
    draw_in(false, || window(&app), "shell-window", Size::new(1400.0, 860.0));
}

// ----- the left sidebar

#[test]
fn the_sidebar_with_isabelle_and_lean_sessions() {
    let mut app = mixed_app();
    for w in [230.0, 300.0, 420.0] {
        app.left_width = w;
        draw_in(false, || super::sidebar::sidebar(&app), &format!("shell-sidebar-sessions-{w}"), Size::new(w, 760.0));
    }
    app.left_width = 300.0;
    draw_in(true, || super::sidebar::sidebar(&app), "shell-sidebar-sessions-light", Size::new(300.0, 760.0));
    // The tabs: parts the size of their icons, each a section (the last click is past the bar)
    let tabs: Vec<Message> = [30.0, 70.0, 110.0, 150.0, 190.0, 290.0].iter().flat_map(|x| clicks(super::sidebar::sidebar(&app), Point::new(*x, 24.0), Size::new(300.0, 760.0))).collect();
    let modes: Vec<LeftMode> = tabs.iter().filter_map(|m| if let Message::LeftMode(l) = m { Some(*l) } else { None }).collect();
    assert_eq!(modes.first(), Some(&LeftMode::Sessions), "{tabs:?}");
    assert!(modes.contains(&LeftMode::Problems) && modes.contains(&LeftMode::Outline), "{modes:?}");
    assert!(modes.contains(&LeftMode::Project), "Isabelle offers building its heap: {modes:?}");
    // A project file opens in the source view
    app.left = LeftMode::Outline;
    draw_in(false, || super::sidebar::sidebar(&app), "shell-sidebar-outline", Size::new(300.0, 500.0));
}

#[test]
fn a_project_file_opens_in_the_source_view_and_a_document_again_literate() {
    let mut app = mixed_app();
    let ws = app.workspace.as_ref().unwrap();
    assert!(super::sidebar::is_project_file(&app, ws, "/home/me/Research/Demo/ROOT"));
    assert!(!super::sidebar::is_project_file(&app, ws, "/home/me/Research/Demo/Demo.thy"));
    let read = |path: &str| json!({"ok": true, "doc": {"path": path, "text": "session Demo = HOL +\n  theories Demo\n", "revision": 1, "prover": null}});
    let _ = app.on_reply("local".into(), super::Pending::Read("s1".into(), "/home/me/Research/Demo/ROOT".into(), None), read("/home/me/Research/Demo/ROOT"));
    assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().mode, Mode::Source);
    assert_eq!(app.mode_model.active(), app.mode_entities[1]);
    let _ = app.on_reply("local".into(), super::Pending::Read("s1".into(), "/home/me/Research/Demo/Demo.thy".into(), None), read("/home/me/Research/Demo/Demo.thy"));
    assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().mode, Mode::Literate);
    assert_eq!(app.mode_model.active(), app.mode_entities[0]);
}

// ----- the project tab

fn lean_project_app() -> App {
    let mut app = mixed_app();
    app.sessions.get_mut(&("local".to_string(), "s3".to_string())).unwrap()["project"] =
        json!({"lake_root": "/home/me/Research/Demo", "toolchain": "leanprover/lean4:v4.19.0", "packages": ["batteries", "aesop"], "mathlib": false});
    let ws = app.workspace.as_mut().unwrap();
    ws.sid = "s3".into();
    ws.caps.project_actions = vec![
        ProjectAction { name: "build".into(), label: "Build (lake build)".into() },
        ProjectAction { name: "rebuild_imports".into(), label: "Rebuild imports".into() },
        ProjectAction { name: "cache_get".into(), label: "Get Mathlib cache".into() },
    ];
    ws.caps.project_files = vec!["lakefile.toml".into(), "lean-toolchain".into()];
    app.shell.runs = HashMap::from([
        (("s3".to_string(), "build".to_string()), Run { running: false, ok: Some(true), output: "✔ [12/14] Built DSRV.Basic\n✔ [13/14] Built DSRV.CausalSemantics\nBuild completed successfully.".into() }),
        (("s3".to_string(), "cache_get".to_string()), Run { running: true, ok: None, output: String::new() }),
        (("s3".to_string(), "rebuild_imports".to_string()), Run { running: false, ok: Some(false), output: "error: unknown module prefix 'Mathlib'".into() }),
    ]);
    app.left = LeftMode::Project;
    app
}

#[test]
fn the_project_tab_lists_actions_with_their_output() {
    let mut app = lean_project_app();
    draw_in(false, || super::sidebar::sidebar(&app), "shell-sidebar-project", Size::new(320.0, 760.0));
    draw_in(true, || super::sidebar::sidebar(&app), "shell-sidebar-project-light", Size::new(320.0, 760.0));
    draw_in(false, || window(&app), "shell-window-project", Size::new(1400.0, 860.0));
    // Run sends the action, and the reply becomes its last output
    let runs: Vec<Message> = (60..700).step_by(6).flat_map(|y| clicks(super::sidebar::sidebar(&app), Point::new(235.0, y as f32), Size::new(320.0, 760.0))).collect();
    assert!(runs.iter().any(|m| matches!(m, Message::Shell(Msg::ProjectAction(n)) if n == "build")), "{runs:?}");
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::ProjectAction("build".into())));
    assert!(app.shell.runs[&("s3".to_string(), "build".to_string())].running);
    app.project_action_done("s3".into(), "build".into(), &json!({"ok": true, "result": {"ok": false, "output_tail": "error: build failed"}}));
    let run = &app.shell.runs[&("s3".to_string(), "build".to_string())];
    assert_eq!((run.running, run.ok, run.output.as_str()), (false, Some(false), "error: build failed"));
}

// ----- the new-session flow

fn new_session_app(backend: Option<&str>, detections: Value) -> App {
    let mut app = mixed_app();
    app.workspace = None;
    app.show_right = false;
    let mut ns = super::NewSession { machine: 0, folder: "/home/me/Research/Mixed".into(), ..Default::default() };
    ns.listing = Some(json!({"path": "/home/me/Research/Mixed", "parent": "/home/me/Research", "entries": [
        {"name": "DSRV", "dir": true}, {"name": "Theories", "dir": true}, {"name": "Main.lean", "dir": false}, {"name": "Semantics.thy", "dir": false}, {"name": "ROOT", "dir": false}, {"name": "lakefile.toml", "dir": false}]}));
    ns.suggested(&json!({"suggestion": {"logic": "HOL"}, "detections": detections}));
    ns.backend = backend.map(str::to_string);
    app.new_session = Some(ns);
    app
}

fn both() -> Value {
    json!([
        {"backend": "isabelle", "evidence": ["ROOT in /home/me/Research/Mixed", "7 .thy files"], "project": {"logic": "HOL-Library", "dirs": [], "include_sessions": [], "session": "Mixed"}, "toolchain": "/opt/Isabelle2025-CyPhyAssure", "note": null},
        {"backend": "lean", "evidence": ["lakefile.toml in /home/me/Research/Mixed", "lean-toolchain: leanprover/lean4:v4.21.0", "packages: batteries"], "project": {"lake_root": "/home/me/Research/Mixed", "toolchain": "leanprover/lean4:v4.21.0", "packages": ["batteries"], "mathlib": false}, "toolchain": "leanprover/lean4:v4.21.0", "note": "elan will download leanprover/lean4:v4.21.0 when the project starts"}])
}

#[test]
fn new_session_cards_for_a_folder_of_both_backends() {
    let app = new_session_app(None, both());
    draw_in(false, || window(&app), "shell-new-session-cards", Size::new(1400.0, 1100.0));
    let app = new_session_app(Some("isabelle"), both());
    draw_in(false, || window(&app), "shell-new-session-isabelle", Size::new(1400.0, 1300.0));
    draw_in(true, || window(&app), "shell-new-session-isabelle-light", Size::new(1000.0, 1300.0));
    let app = new_session_app(Some("lean"), both());
    draw_in(false, || window(&app), "shell-new-session-lean", Size::new(1400.0, 1300.0));
    let app = new_session_app(None, json!([]));
    draw_in(false, || window(&app), "shell-new-session-nothing-found", Size::new(1000.0, 1200.0));
}

#[test]
fn a_card_picks_its_backend_and_start_sends_it() {
    let mut app = new_session_app(None, both());
    let picks: Vec<Message> = (500..1100).step_by(8).flat_map(|y| clicks(window(&app), Point::new(800.0, y as f32), Size::new(1400.0, 1100.0))).collect();
    let backends: Vec<String> = picks.iter().filter_map(|m| if let Message::Shell(Msg::NewBackend(b)) = m { Some(b.clone()) } else { None }).collect();
    assert!(backends.contains(&"isabelle".to_string()) && backends.contains(&"lean".to_string()), "{picks:?}");
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::NewBackend("lean".into())));
    let params = app.new_session.as_ref().unwrap().create_params();
    assert_eq!(params["backend"], "lean");
    assert_eq!(params["project"]["lake_root"], "/home/me/Research/Mixed");
    // A Lean document clicked in the browser of a folder whose Lean session exists opens there
    let mut app = new_session_app(None, both());
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::OpenDocument("/home/me/Research/Demo/Main.lean".into())));
    assert!(app.new_session.is_none());
    assert_eq!(app.workspace.as_ref().map(|w| w.sid.as_str()), Some("s3"), "the Lean session of that folder");
    assert_eq!(app.want_theory.as_deref(), Some("/home/me/Research/Demo/Main.lean"));
    // one in a folder without a session of its backend: the form, its backend picked, says why
    let mut app = new_session_app(None, both());
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::OpenDocument("/home/me/Elsewhere/Scratch.lean".into())));
    let ns = app.new_session.as_ref().unwrap();
    assert_eq!(ns.backend.as_deref(), Some("lean"));
    assert!(ns.reason.as_deref().is_some_and(|r| r.contains("Scratch.lean is a Lean document")), "{:?}", ns.reason);
}


/// The sidebar, the status bar and the settings read a machine's connection from one place, so
/// they never disagree in one frame
#[test]
fn every_part_of_the_window_says_the_same_about_a_connection() {
    let mut l = connected("au-laptop", "local", None);
    assert_eq!(shell::connection(&l), shell::Conn::Connected);
    l.sender = None;
    assert_eq!(shell::connection(&l), shell::Conn::Connected, "the status says connected");
    l.status = "starting".into();
    assert_eq!(shell::connection(&l).word(), "connecting…");
    assert_eq!(shell::connection(&l).detail(), None, "nothing to add while starting");
    l.status = "disconnected: broken pipe".into();
    assert_eq!(shell::connection(&l), shell::Conn::Offline("broken pipe".into()));
    // the open session's machine: said in the status bar only when it is not connected
    let mut app = demo_app(Mode::Literate);
    app.links[0].status = "connected".into();
    assert!(shell::status_bar(&app).is_some(), "a document is open");
    let _ = cosmic::Application::update(&mut app, Message::Save);
    assert_eq!(app.status, None, "no \"Not connected to local\" for the machine the bar speaks for");
}

/// `CT_SNAPSHOTS`: whole windows (the header over the view) where the shell meets the document:
/// the settings page narrow, the new-session form at 1000 px, both sidebars round a Lean document
/// at 1100 px, and the header with a running tool at 1000 px, in both themes
#[test]
fn shell_frame_matrix() {
    if std::env::var("CT_SNAPSHOTS").is_err() {
        return;
    }
    for (light, theme) in [(true, "light"), (false, "dark")] {
        // the settings page at 800 px, alone and beside the sidebar
        let mut app = mixed_app();
        app.shell.settings_open = true;
        app.window_width = 800.0;
        app.show_sidebar = false;
        draw_in(light, || window(&app), &format!("matrix-shell-{theme}-800-settings"), Size::new(800.0, 860.0));
        app.show_sidebar = true;
        app.left_width = 260.0;
        draw_in(light, || window(&app), &format!("matrix-shell-{theme}-800-settings-sidebar"), Size::new(800.0, 860.0));
        // the new-session form at 1000 px
        let mut app = new_session_app(Some("isabelle"), both());
        app.window_width = 1000.0;
        draw_in(light, || window(&app), &format!("matrix-shell-{theme}-1000-new-session"), Size::new(1000.0, 1200.0));
        // a Lean document between both sidebars at 1100 px, a theorem open
        let mut app = super::snapshot_lean::lean_app(Mode::Literate);
        app.show_sidebar = true;
        app.show_right = true;
        app.window_width = 1100.0;
        app.left_width = 240.0;
        app.right_width = 300.0;
        {
            let s = app.sessions.get_mut(&("local".to_string(), "s1".to_string())).unwrap();
            s["toolchain"] = json!({"id": "leanprover/lean4:v4.19.0", "label": "Lean v4.19.0"});
            s["documents"] = json!([".lean"]);
        }
        app.workspace.as_mut().unwrap().activity = Some(json!({"prover": {"cpu": 120, "rss_mb": 1840}, "running": [], "theories": []}));
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "theorem").unwrap();
        let _ = cosmic::Application::update(&mut app, Message::EditBlock(i));
        draw_in(light, || window(&app), &format!("matrix-shell-{theme}-1100-lean-sidebars"), Size::new(1100.0, 860.0));
        // a document with the sidebars at narrow widths: the left one at 800 px, both at 1000 px
        for (width, right) in [(800.0, false), (1000.0, true)] {
            let mut app = mixed_app();
            app.window_width = width;
            app.left_width = 240.0;
            app.right_width = 300.0;
            app.show_right = right;
            let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
            let _ = cosmic::Application::update(&mut app, Message::EditBlock(i));
            draw_in(light, || window(&app), &format!("matrix-shell-{theme}-{}-sidebars", width as u32), Size::new(width, 760.0));
        }
        // the header with a running tool at 1000 px
        let mut app = mixed_app();
        app.window_width = 1000.0;
        app.show_sidebar = false;
        app.show_right = false;
        app.workspace.as_mut().unwrap().activity = Some(json!({"prover": {"cpu": 390, "rss_mb": 3200},
            "running": [{"name": "sledgehammer", "seconds": 12, "line": 9}, {"name": "by", "seconds": 7, "line": 12}], "theories": []}));
        draw_in(light, || window(&app), &format!("matrix-shell-{theme}-1000-tool"), Size::new(1000.0, 700.0));
    }
}

#[test]
fn a_project_s_documents_collapse_all_at_once_or_by_folder() {
    let mut app = mixed_app();
    // A bigger project: documents at the top and in two folders
    let mut theories = vec![json!({"path": "/home/me/Research/Demo/ROOT", "status": null})];
    for n in ["Demo", "More", "Extra", "Laws"] {
        theories.push(json!({"path": format!("/home/me/Research/Demo/{n}.thy"), "status": {"consolidated": true}}));
    }
    for d in ["lib", "examples"] {
        for n in ["Base", "Lists", "Sets"] {
            theories.push(json!({"path": format!("/home/me/Research/Demo/{d}/{n}.thy"), "status": {"consolidated": true}}));
        }
    }
    app.workspace.as_mut().unwrap().info.as_mut().unwrap()["theories"] = json!(theories);
    let rows = |app: &App| {
        let png = draw_in(false, || super::sidebar::sidebar(app), "probe", Size::new(300.0, 900.0));
        png.len()
    };
    let _ = rows(&app);
    draw_in(false, || super::sidebar::sidebar(&app), "shell-sidebar-documents-expanded", Size::new(300.0, 760.0));
    // A folder's heading collapses it
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::ToggleDocFolder("lib".into())));
    assert!(app.shell.collapsed_dirs.iter().any(|(_, _, d)| d == "lib"));
    draw_in(false, || super::sidebar::sidebar(&app), "shell-sidebar-documents-folder-collapsed", Size::new(300.0, 760.0));
    // The list's header collapses everything but the open document, and expands it again
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::ToggleDocuments));
    let key = { let ws = app.workspace.as_ref().unwrap(); (ws.machine.clone(), ws.sid.clone()) };
    assert!(app.shell.collapsed.contains(&key));
    draw_in(false, || super::sidebar::sidebar(&app), "shell-sidebar-documents-collapsed", Size::new(300.0, 760.0));
    draw_in(true, || super::sidebar::sidebar(&app), "shell-sidebar-documents-collapsed-light", Size::new(300.0, 760.0));
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::ToggleDocuments));
    assert!(!app.shell.collapsed.contains(&key));
}

// ----- the household sidebar: tabs, rows, marks

use cosmic::iced::Color;

fn theme_of(light: bool) -> cosmic::Theme {
    if light { cosmic::Theme::light() } else { cosmic::Theme::dark() }
}

fn px(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * width + x) * 4) as usize;
    [frame[i], frame[i + 1], frame[i + 2]]
}

/// How many pixels of a frame are `c`, to `tol` in each channel
fn count_of(frame: &[u8], c: Color, tol: u8) -> usize {
    let [r, g, b, _] = c.into_rgba8();
    frame.chunks(4).filter(|p| [r, g, b].iter().zip(&p[..3]).all(|(a, b)| a.abs_diff(*b) <= tol)).count()
}

/// The rows of a frame that differ from its top-left pixel, and the bands of rows that have ink
fn ink(frame: &[u8], width: u32) -> (Vec<Vec<bool>>, Vec<(u32, u32)>) {
    let bg = px(frame, width, 0, 0);
    let height = frame.len() as u32 / 4 / width;
    let rows: Vec<Vec<bool>> = (0..height).map(|y| (0..width).map(|x| px(frame, width, x, y).iter().zip(bg).any(|(a, b)| a.abs_diff(b) > 40)).collect()).collect();
    let mut bands = Vec::new();
    let mut start = None;
    for (y, row) in rows.iter().enumerate() {
        match (row.iter().any(|b| *b), start) {
            (true, None) => start = Some(y as u32),
            (false, Some(s)) => {
                bands.push((s, y as u32));
                start = None;
            }
            _ => {}
        }
    }
    // (an underscore sits apart from the letters above it: one line, not two)
    let mut lines: Vec<(u32, u32)> = Vec::new();
    for (from, to) in bands {
        match lines.last_mut() {
            Some(last) if from - last.1 <= 3 => last.1 = to,
            _ => lines.push((from, to)),
        }
    }
    (rows, lines)
}

/// The ink of the rows `from..to`, cropped to its own bounding box
fn cropped(rows: &[Vec<bool>], from: u32, to: u32) -> Vec<Vec<bool>> {
    let part: Vec<&Vec<bool>> = rows[from as usize..to as usize].iter().collect();
    let (x0, x1) = (0..part[0].len()).fold((usize::MAX, 0), |(a, b), x| if part.iter().any(|r| r[x]) { (a.min(x), b.max(x)) } else { (a, b) });
    part.iter().map(|r| r[x0..=x1].to_vec()).collect()
}

/// Whether two bitmaps of ink are the same text: the same size, and all but a few edge pixels the
/// same (a line is placed at a fraction of a pixel, so its edges are not always drawn alike)
fn same_ink(a: &[Vec<bool>], b: &[Vec<bool>]) -> bool {
    a.len() == b.len()
        && a[0].len() == b[0].len()
        && a.iter().flatten().zip(b.iter().flatten()).filter(|(x, y)| x != y).count() * 25 <= a.len() * a[0].len()
}

/// A name drawn as a row's name is, in a box of the given width
fn name_frame(light: bool, name: &str, width: u32, height: u32) -> Vec<u8> {
    let tag = if width < 900 { "sidebar-name-wrapped" } else { "sidebar-name-single" };
    let name = name.to_string();
    draw_in(light, move || widget::container(super::sidebar::name_text(&name, 14.0, false)).width(Length::Fixed(width as f32)).into(), tag, Size::new(width as f32, height as f32))
}

/// The current session and the current file are rows with COSMIC's wash and their text in the
/// accent, bold; nothing else is washed, and the tint the old rows used is gone
#[test]
fn the_current_row_has_the_wash_and_accent_bold_text() {
    assert_eq!(super::sidebar::row_font(true), cosmic::font::bold());
    assert_eq!(super::sidebar::row_font(false), cosmic::font::default());
    for light in [true, false] {
        let mut app = mixed_app();
        app.left_width = 300.0;
        let (w, h) = (300u32, 760u32);
        let frame = draw_in(light, || super::sidebar::sidebar(&app), &format!("sidebar-current-{}", if light { "light" } else { "dark" }), Size::new(w as f32, h as f32));
        let t = theme_of(light);
        let p = px(&frame, w, 20, h - 10);
        let page = Color::from_rgb8(p[0], p[1], p[2]);
        let wash = super::tones::over(t.cosmic().icon_button.hover.into(), page);
        let washed = count_of(&frame, wash, 1);
        assert!(washed > 6_000, "{washed} washed pixels");
        // The text in the accent, and none of the old accent tint
        let accent: Color = t.cosmic().accent_color().into();
        assert!(count_of(&frame, accent, 24) > 40, "accent text");
        let old_tint = super::tones::over(Color { a: 0.16, ..accent }, page);
        assert_eq!(count_of(&frame, old_tint, 1), 0, "the old accent tint");
    }
}

/// A long name wraps after an underscore, on lines that are whole words, and nothing is clipped
#[test]
fn a_long_name_wraps_after_an_underscore_without_clipping() {
    assert_eq!(super::sidebar::wrappable("A_b__c_"), "A_\u{200b}b__\u{200b}c_");
    let name = "Timed_Circus_Contract_Automation_Refinement";
    let pieces: Vec<&str> = name.split_inclusive('_').collect();
    let width = 150u32;
    for light in [true, false] {
        let frame = name_frame(light, name, width, 100);
        let (rows, bands) = ink(&frame, width);
        assert!(bands.len() == 3, "{bands:?}: the name wraps");
        // No ink in the last column: no glyph is cut off at the edge
        assert!(rows.iter().all(|r| !r[width as usize - 1]), "a clipped glyph");
        // Each line is what a single line of whole pieces looks like
        let mut at = 0;
        for (from, to) in &bands {
            let line = cropped(&rows, *from, *to);
            let found = (at + 1..=pieces.len()).find(|k| {
                let single = name_frame(light, &pieces[at..*k].concat(), 900, 40);
                let (single_rows, single_bands) = ink(&single, 900);
                single_bands.len() == 1 && same_ink(&cropped(&single_rows, single_bands[0].0, single_bands[0].1), &line)
            });
            at = found.unwrap_or_else(|| panic!("a line that is not whole pieces, after piece {at}: {bands:?} {}x{}", line.len(), line[0].len()));
        }
        assert_eq!(at, pieces.len(), "every piece is on a line");
    }
}

/// Status is a mark and words, never ▶ ⏸ ■: a filled green dot, a hollow amber ring, a grey dot for
/// a stopped session, the error icon for a failed one, a spinner while it checks
#[test]
fn a_running_session_shows_a_dot_and_the_word_running() {
    use super::sidebar::{Mark, caption_line, mark_of};
    assert_eq!(mark_of("Running", false), Mark::Dot);
    assert_eq!(mark_of("Running", true), Mark::Spinner);
    assert_eq!(mark_of("Paused", false), Mark::Ring);
    assert_eq!(mark_of("Stopped", false), Mark::Grey);
    assert_eq!(mark_of("Failed", false), Mark::Error);
    assert_eq!(caption_line("neo", "Running"), "neo \u{b7} running");
    assert_eq!(caption_line("this computer", "Paused"), "this computer \u{b7} paused");
    assert!(!include_str!("sidebar.rs").contains("media-playback"), "no play, pause or stop icon is a status");
    for light in [true, false] {
        let app = mixed_app();
        let frame = draw_in(light, || super::sidebar::sidebar(&app), "sidebar-marks", Size::new(300.0, 760.0));
        let t = theme_of(light);
        let green = super::tones::muted_status(t.cosmic().success_color().into(), !light);
        let amber = super::tones::muted_status(super::view::sorry_color(&t), !light);
        let red = super::tones::muted_status(super::view::error_color(&t), !light);
        // (the DSRV session is running and idle; UTP is paused; tock-Circus has failed)
        assert!(count_of(&frame, green, 2) >= 30, "the running dot");
        assert!(count_of(&frame, amber, 30) >= 8, "the paused ring");
        assert!(count_of(&frame, red, 40) >= 8, "the failed icon");
        // (and CausalSemantics is stopped: a grey dot of its own, not nothing)
        let grey = super::sidebar::muted_grey(&t);
        assert!(count_of(&frame, grey, 6) >= 20, "the stopped dot");
    }
}

/// The tabs are the size of their icons, at the bar's left, the chosen one in the muted accent
#[test]
fn the_tabs_are_no_wider_than_their_content() {
    for light in [true, false] {
        let app = mixed_app();
        let w = 300u32;
        let frame = draw_in(light, || super::sidebar::sidebar(&app), "sidebar-tabs", Size::new(w as f32, 200.0));
        let right_bg = px(&frame, w, w - 1, 24);
        let edge = (0..w).rev().find(|x| px(&frame, w, *x, 24) != right_bg).expect("a bar");
        // Five tabs of a 16 px icon and 12 px either side, inside the card's padding
        assert!(edge < 12 + 5 * 40 + 8, "the bar ends at {edge}");
        assert!(edge > 100, "the bar ends at {edge}");
        let fill = super::tones::muted(&theme_of(light)).0;
        assert!(count_of(&frame, fill, 1) > 500, "the chosen tab is the muted accent");
    }
}

/// The sidebar as the review's mock-up draws it (mockups/t25-sidebar.png), light and dark, at 300 px
/// (frames only, with `CT_SNAPSHOTS`)
#[test]
fn sidebar_frames_for_the_review() {
    if std::env::var("CT_SNAPSHOTS").is_err() {
        return;
    }
    let mut app = mixed_app();
    app.links = vec![connected("this computer", "local", Some(hello())), connected("neo", "neo", None)];
    app.sessions.clear();
    let mut put = |machine: &str, v: Value| {
        app.sessions.insert((machine.to_string(), v["id"].as_str().unwrap().to_string()), v);
    };
    put("neo", json!({"id": "t1", "name": "Timed_Circus", "folder": "/home/twright/Isabelle/Timed_Circus", "backend": "isabelle", "lifecycle": "Running",
        "toolchain": {"label": "Isabelle2025"}, "activity": {"running": 0, "failed": 0}, "clients": []}));
    put("local", json!({"id": "d1", "name": "DSRVLean", "folder": "/home/me/DSRVLean", "backend": "lean", "lifecycle": "Paused",
        "toolchain": {"label": "Lean v4.19.0"}, "activity": {}, "clients": []}));
    put("neo", json!({"id": "t2", "name": "Sequential_Counter_Refinement", "folder": "/home/twright/Isabelle/Timed_Circus", "backend": "isabelle", "lifecycle": "Stopped",
        "toolchain": {"label": "Isabelle2025"}, "activity": {}, "clients": []}));
    let folder = "/home/twright/Isabelle/Timed_Circus";
    let ws = app.workspace.as_mut().unwrap();
    ws.machine = "neo".into();
    ws.sid = "t1".into();
    ws.info.as_mut().unwrap()["folder"] = json!(folder);
    ws.info.as_mut().unwrap()["theories"] = json!([
        {"path": format!("{folder}/Stop.thy"), "status": {"consolidated": true}},
        {"path": format!("{folder}/Skip.thy"), "status": {"consolidated": true}},
        {"path": format!("{folder}/Timed_Circus_Contract_Automation.thy"), "status": {"consolidated": true}}]);
    ws.theory.as_mut().unwrap().path = format!("{folder}/Stop.thy");
    app.left_width = 300.0;
    for (light, name) in [(true, "light"), (false, "dark")] {
        draw_in(light, || super::sidebar::sidebar(&app), &format!("sidebar-review-{name}"), Size::new(300.0, 640.0));
    }
}

// ----- the outline, theorems and problems rows

/// The Theorems rows (the demo's caret is in `add_zero`): the current row has COSMIC's wash and its
/// name in the accent, bold; the others are clear, in the text colour; hovering a row is COSMIC's
/// wash, and a name wraps after an underscore
#[test]
fn the_theorems_rows_use_the_sidebar_row_style() {
    use cosmic::widget::button::Catalog as _;
    for light in [true, false] {
        let app = demo_app(Mode::Literate);
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let (w, h) = (320u32, 220u32);
        let frame = draw_in(light, || super::view::theorems(&app, t), &format!("theorems-{}", if light { "light" } else { "dark" }), Size::new(w as f32, h as f32));
        let theme = theme_of(light);
        let bg = px(&frame, w, w - 3, h - 3);
        let page = Color::from_rgb8(bg[0], bg[1], bg[2]);
        let wash = super::tones::over(theme.cosmic().icon_button.hover.into(), page);
        // the current row is washed (a row's width and a row's height); no other is
        let rows: Vec<usize> = (0..h).filter(|y| (0..w).filter(|x| { let p = px(&frame, w, *x, *y); [wash.into_rgba8()[0], wash.into_rgba8()[1], wash.into_rgba8()[2]].iter().zip(p).all(|(a, b)| a.abs_diff(b) <= 1) }).count() > 200).map(|y| y as usize).collect();
        assert!(!rows.is_empty(), "light={light}: no washed row");
        assert!(rows.windows(2).all(|p| p[1] == p[0] + 1) && rows.len() < 60, "light={light}: the wash is on one row: {} rows, {:?}..", rows.len(), rows.first());
        // its name is in the accent, and bold: more accent ink than a name in the accent at the regular weight
        let accent: Color = theme.cosmic().accent_color().into();
        assert!(count_of(&frame, accent, 24) > 40, "light={light}: the current name is not in the accent");
        // the old chrome is gone: the MenuItem wash is not behind any other row
        let washed = count_of(&frame, wash, 1);
        assert!(washed < (w * 60) as usize, "light={light}: {washed} washed pixels: more than the one row");
        // the states of a row's button: clear at rest, COSMIC's wash on hover, the current row washed at rest
        let rest = theme.active(false, false, &super::sidebar::nav_style(false));
        assert!(rest.background.is_none(), "a row at rest has a fill");
        let hover = theme.hovered(false, false, &super::sidebar::nav_style(false));
        assert_eq!(hover.background, Some(cosmic::iced::Background::Color(theme.cosmic().icon_button.hover.into())), "light={light}: hover is not COSMIC's wash");
        let current = theme.active(false, false, &super::sidebar::nav_style(true));
        assert_eq!(current.text_color, Some(accent), "light={light}: the current row's text");
        assert!(current.background.is_some(), "the current row keeps the wash");
    }
    assert_eq!(super::sidebar::row_font(true), cosmic::font::bold());
}

/// The outline's current row is the heading the caret is under
#[test]
fn the_outline_marks_the_heading_the_caret_is_under() {
    for light in [true, false] {
        let app = demo_app(Mode::Literate);
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let (w, h) = (320u32, 80u32);
        let frame = draw_in(light, || super::view::outline(&app, t), &format!("outline-{}", if light { "light" } else { "dark" }), Size::new(w as f32, h as f32));
        let theme = theme_of(light);
        let bg = px(&frame, w, w - 3, h - 3);
        let page = Color::from_rgb8(bg[0], bg[1], bg[2]);
        let wash = super::tones::over(theme.cosmic().icon_button.hover.into(), page);
        assert!(count_of(&frame, wash, 1) > 3_000, "light={light}: the current heading is not washed");
        assert!(count_of(&frame, theme.cosmic().accent_color().into(), 24) > 40, "light={light}: the current heading is not in the accent");
    }
}

// ----- the status chip, pause and stop

/// The first x on the header's row where a click sends a message `wanted` accepts
fn x_where(app: &App, wanted: impl Fn(&Message) -> bool) -> Option<u32> {
    (0..1400u32).step_by(2).find(|x| clicks(header(app), Point::new(*x as f32, 24.0), Size::new(1400.0, 48.0)).iter().any(&wanted))
}

fn set_lifecycle(app: &mut App, lifecycle: &str) {
    app.sessions.get_mut(&("local".to_string(), "s1".to_string())).unwrap()["lifecycle"] = json!(lifecycle);
}

/// A scene with the prover running and checked, changed by `f`
fn chip_scene(f: impl FnOnce(&mut App)) -> App {
    let mut app = mixed_app();
    app.show_right = false;
    set_lifecycle(&mut app, "Running");
    let ws = app.workspace.as_mut().unwrap();
    ws.activity = Some(json!({"prover": {"cpu": 2, "rss_mb": 1300}, "running": [], "theories": []}));
    ws.theory.as_mut().unwrap().prover.status = json!({"finished": 15, "unprocessed": 0, "running": 0, "failed": 0, "consolidated": true});
    f(&mut app);
    app
}

#[test]
fn the_chip_says_its_state_with_a_mark_in_each() {
    use super::chip::{State, state_of};
    use super::sidebar::Mark;
    let path = "/home/me/Research/Demo/Demo.thy";
    let cases: Vec<(&str, App, State, &str, Mark)> = vec![
        ("no session", chip_scene(|a| a.workspace = None), State::NoSession, "No session", Mark::Blank),
        ("starting", chip_scene(|a| a.workspace.as_mut().unwrap().activity = None), State::Starting, "Starting\u{2026}", Mark::Spinner),
        (
            "loading imports",
            chip_scene(|a| {
                a.workspace.as_mut().unwrap().activity = Some(json!({"prover": {"rss_mb": 900}, "running": [], "theories": [
                    {"path": path, "imports": ["/x/A.thy", "/x/B.thy", "/x/C.thy"], "consolidated": false},
                    {"path": "/x/A.thy", "consolidated": true, "percentage": 100},
                    {"path": "/x/B.thy", "consolidated": true, "percentage": 100},
                    {"path": "/x/C.thy", "consolidated": false, "percentage": 40}]}));
            }),
            State::Loading { done: 2, total: 3 },
            "Loading imports \u{b7} 2 of 3",
            Mark::Spinner,
        ),
        (
            "checking",
            chip_scene(|a| a.workspace.as_mut().unwrap().theory.as_mut().unwrap().prover.status = json!({"finished": 6, "unprocessed": 6, "running": 0, "failed": 0, "consolidated": false})),
            State::Checking(50),
            "Checking \u{b7} 50 %",
            Mark::Spinner,
        ),
        ("problems", chip_scene(|_| {}), State::Problems { failed: 1, unfinished: 1 }, "1 failed \u{b7} 1 unfinished", Mark::RedDot),
        ("paused", chip_scene(|a| set_lifecycle(a, "Paused")), State::Paused, "Paused", Mark::Ring),
        ("stopped", chip_scene(|a| set_lifecycle(a, "Stopped")), State::Stopped, "Stopped", Mark::Grey),
        ("crashed", chip_scene(|a| set_lifecycle(a, "Failed")), State::Crashed, "The prover stopped unexpectedly", Mark::Error),
    ];
    for (name, app, state, label, mark) in &cases {
        assert_eq!(&state_of(app), state, "{name}");
        assert_eq!(state.label(), *label, "{name}");
        assert_eq!(state.mark(), *mark, "{name}");
        assert!(!state.label().contains('\u{25b6}') && !state.label().contains('\u{23f8}'), "no play or pause sign as a status: {name}");
    }
    // nothing failed or unfinished: Checked, with a check
    let mut app = chip_scene(|_| {});
    {
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        for b in &mut t.blocks {
            b.status = super::theory::Status::Finished;
            b.commands.retain(|c| c.name != "sorry");
        }
    }
    assert_eq!(state_of(&app), State::Checked);
    assert_eq!((State::Checked.label().as_str(), State::Checked.mark()), ("Checked", Mark::Check));
    // a failed proof alone says only that
    {
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let i = t.blocks.iter().position(|b| b.name == "lemma").unwrap();
        t.blocks[i].status = super::theory::Status::Failed;
    }
    assert_eq!(state_of(&app).label(), "1 failed");
    // frames, in both themes
    for (light, theme) in [(true, "light"), (false, "dark")] {
        for (name, app, ..) in &cases {
            draw_in(light, || header(app), &format!("chip-{}-{theme}", name.replace(' ', "-")), Size::new(1400.0, 48.0));
        }
    }
}

#[test]
fn pause_resume_restart_and_stop_are_always_there() {
    use super::chip::{controls_for, state_of};
    let mut app = chip_scene(|_| {});
    // running: pause and stop
    let pause = x_where(&app, |m| matches!(m, Message::Lifecycle("pause"))).expect("pause");
    let stop = x_where(&app, |m| matches!(m, Message::Lifecycle("stop"))).expect("stop");
    assert!(stop > pause);
    // (the chip is as wide as its words, so the first button is where the chip ends, whatever the
    // state: right after it, and no longer at one fixed place)
    let right_after_chip = |app: &App, x: u32| {
        let end = span_of(&header_hits(app), |m| matches!(m, Message::Panel(super::Panel::Log) | Message::Shell(Msg::NextProblem(_)))).expect("the chip").1;
        x > end && x - end <= 14
    };
    assert!(right_after_chip(&app, pause));
    // paused: the first button, right after the chip, now resumes, and stop follows it
    set_lifecycle(&mut app, "Paused");
    let resume = x_where(&app, |m| matches!(m, Message::Lifecycle("resume"))).expect("resume");
    assert!(right_after_chip(&app, resume));
    assert!(x_where(&app, |m| matches!(m, Message::Lifecycle("stop"))).is_some_and(|s| s > resume));
    assert_eq!(x_where(&app, |m| matches!(m, Message::Lifecycle("pause"))), None);
    // stopped: resume (start again), stop disabled
    set_lifecycle(&mut app, "Stopped");
    assert!(x_where(&app, |m| matches!(m, Message::Lifecycle("resume"))).is_some_and(|x| right_after_chip(&app, x)));
    assert_eq!(x_where(&app, |m| matches!(m, Message::Lifecycle("stop"))), None);
    assert!(!controls_for(&state_of(&app)).stop.1);
    // crashed: Restart (the circular arrow), stop disabled
    set_lifecycle(&mut app, "Failed");
    let c = controls_for(&state_of(&app));
    assert_eq!((c.first.0, c.first.2, c.stop.1), ("view-refresh-symbolic", Some("resume"), false));
    assert!(x_where(&app, |m| matches!(m, Message::Lifecycle("resume"))).is_some_and(|x| right_after_chip(&app, x)));
    // no session: both are there and disabled, not hidden
    let none = chip_scene(|a| a.workspace = None);
    let c = controls_for(&state_of(&none));
    assert_eq!((c.first.2, c.stop.1), (None, false));
    assert_eq!(super::chip::controls(&none).len(), 2, "both buttons are drawn");
    assert_eq!(x_where(&none, |m| matches!(m, Message::Lifecycle(_))), None);
    // and the chip's state goes to the next failure when there is one
    let app = chip_scene(|_| {});
    let hits = (0..1400u32).step_by(2).flat_map(|x| clicks(header(&app), Point::new(x as f32, 24.0), Size::new(1400.0, 48.0))).collect::<Vec<_>>();
    assert!(hits.iter().any(|m| matches!(m, Message::Shell(Msg::NextProblem(super::attention::Problem::Failed)))), "{hits:?}");
}

/// Where the header's clickable parts are, by message: every second pixel of the bar's middle row
/// with what a click there sends
fn header_hits(app: &App) -> Vec<(u32, Vec<Message>)> {
    (0..1400u32).step_by(2).map(|x| (x, clicks(header(app), Point::new(x as f32, 24.0), Size::new(1400.0, 48.0)))).collect()
}

/// The first and last x of the parts whose message `wanted` accepts
fn span_of(hits: &[(u32, Vec<Message>)], wanted: impl Fn(&Message) -> bool) -> Option<(u32, u32)> {
    let xs: Vec<u32> = hits.iter().filter(|(_, m)| m.iter().any(&wanted)).map(|(x, _)| *x).collect();
    Some((*xs.first()?, *xs.last()?))
}

/// The chip is as wide as its content and pause and stop sit right after it, so they shift when
/// the state changes (Tom's decided exception to rule 3, 8 Nov); the header's other groups, at its
/// edges, do not move, and the group of chip, pause and stop stays centred as a whole
#[test]
fn the_chip_hugs_its_content_and_only_its_own_group_shifts_between_states() {
    let scenes: Vec<(&str, App)> = vec![
        ("checked", chip_scene(|_| {})),
        ("starting", chip_scene(|a| a.workspace.as_mut().unwrap().activity = None)),
        ("checking", chip_scene(|a| a.workspace.as_mut().unwrap().theory.as_mut().unwrap().prover.status = json!({"finished": 6, "unprocessed": 6, "consolidated": false}))),
        ("paused", chip_scene(|a| set_lifecycle(a, "Paused"))),
        ("stopped", chip_scene(|a| set_lifecycle(a, "Stopped"))),
        ("crashed", chip_scene(|a| set_lifecycle(a, "Failed"))),
    ];
    let chip = |m: &Message| matches!(m, Message::Panel(super::Panel::Log) | Message::Shell(Msg::NextProblem(_)));
    let pause = |m: &Message| matches!(m, Message::Lifecycle("pause" | "resume"));
    let stop = |m: &Message| matches!(m, Message::Lifecycle("stop"));
    let measured: Vec<_> = scenes.iter().map(|(name, app)| {
        let hits = header_hits(app);
        let get = |wanted: &dyn Fn(&Message) -> bool| span_of(&hits, wanted);
        let sidebar = get(&|m| matches!(m, Message::ToggleSidebar)).unwrap_or_else(|| panic!("{name}: the sidebar toggle"));
        let right = get(&|m| matches!(m, Message::ToggleRight)).unwrap_or_else(|| panic!("{name}: the right panel toggle"));
        let beside = get(&|m| matches!(m, Message::ToggleGoalBeside)).unwrap_or_else(|| panic!("{name}: the goal toggle"));
        let chip = get(&chip).unwrap_or_else(|| panic!("{name}: the chip"));
        let pause = get(&pause).unwrap_or_else(|| panic!("{name}: pause"));
        // (stop is disabled when stopped or crashed: it sits where it would, 2 buttons after the chip)
        let stop = get(&stop);
        (*name, sidebar, right, beside, chip, pause, stop)
    }).collect();
    let first = &measured[0];
    for (name, sidebar, right, beside, chip, pause, _) in &measured {
        // the left and right groups do not move
        assert_eq!((*sidebar, *right, *beside), (first.1, first.2, first.3), "{name}: the header's other groups moved");
        // pause is right after the chip: a few pixels, not a fixed width's worth
        assert!(pause.0 > chip.1 && pause.0 - chip.1 <= 14, "{name}: pause starts at {} and the chip ends at {}", pause.0, chip.1);
    }
    // the chip is as wide as its content: no two states of different words are the same width, and
    // the shortest (starting) is well under the longest (checked, with the memory)
    let width = |s: &(&str, (u32, u32), (u32, u32), (u32, u32), (u32, u32), (u32, u32), Option<(u32, u32)>)| s.4.1 - s.4.0;
    let widths: Vec<(&str, u32)> = measured.iter().map(|s| (s.0, width(s))).collect();
    let (least, most) = (widths.iter().map(|w| w.1).min().unwrap(), widths.iter().map(|w| w.1).max().unwrap());
    assert!(most > least + 60, "the chip is not as wide as its content: {widths:?}");
    // pause and stop shift with it
    let pauses: std::collections::BTreeSet<u32> = measured.iter().map(|s| s.5.0).collect();
    assert!(pauses.len() > 3, "pause stays where it is: {pauses:?}");
    // the group (chip to the end of the last of pause and stop) stays centred on the same place
    // (stop, disabled when stopped or crashed, takes no clicks: it is as far past pause as when it does)
    let stop_past_pause = first.6.expect("stop when checked").1 - first.5.1;
    let centres: Vec<(&str, f32)> = measured.iter().map(|s| (s.0, (s.4.0 + s.5.1 + stop_past_pause) as f32 / 2.0)).collect();
    for (name, c) in &centres {
        assert!((c - centres[0].1).abs() <= 6.0, "{name}: the group is centred at {c}, as against {} when checked: {centres:?}", centres[0].1);
    }
    // no session: still there, and the sidebar toggle stays first
    let none = chip_scene(|a| a.workspace = None);
    let hits = header_hits(&none);
    assert_eq!(span_of(&hits, |m| matches!(m, Message::ToggleSidebar)), Some((first.1.0, first.1.1)));
}

/// The padding inside the chip is even: as far from the pill's left edge to the logo as from the
/// last of its words to its right edge
#[test]
fn the_chip_has_even_padding_at_both_ends() {
    for (name, app) in [("checked", chip_scene(|_| {})), ("paused", chip_scene(|a| set_lifecycle(a, "Paused")))] {
        let hits = header_hits(&app);
        let (from, to) = span_of(&hits, |m| matches!(m, Message::Panel(super::Panel::Log) | Message::Shell(Msg::NextProblem(_)))).expect("the chip");
        let size = Size::new(1400.0, 48.0);
        let frame = draw_in(true, || header(&app), &format!("chip-padding-{name}"), size);
        let px = |x: u32, y: u32| {
            let i = ((y * 1400 + x) * 4) as usize;
            [frame[i], frame[i + 1], frame[i + 2]]
        };
        let bar = px(4, 2);
        // the pill's own extent along the middle row: from the first pixel that is not the bar,
        // a little before the chip's first part, to the last
        let left = (from.saturating_sub(24)..from + 8).find(|&x| px(x, 24) != bar).expect("the pill's left edge");
        let right = (to.saturating_sub(8)..to + 24).rev().find(|&x| px(x, 24) != bar).and_then(|_| (to.saturating_sub(8)..to + 24).take_while(|&x| px(x, 24) != bar).last()).expect("the pill's right edge");
        let fill = px(left + 3, 24);
        // ink: any pixel inside the pill, between its top and bottom border, that is not its fill
        let ink = |x: u32| (21..28).any(|y| {
            let p = px(x, y);
            p.iter().zip(fill).any(|(a, b)| a.abs_diff(b) > 40)
        });
        let first = (left + 4..right - 4).find(|&x| ink(x)).expect("ink in the chip");
        let last = (left + 4..right - 4).rev().find(|&x| ink(x)).expect("ink in the chip");
        let (before, after) = (first - left, right - last);
        assert!((before as i32 - after as i32).abs() <= 4, "{name}: {before} px before the first mark and {after} after the last (the pill {left}..{right})");
        assert!(before >= 6 && before <= 16, "{name}: {before} px of padding at the start");
    }
}

// ----- the sidebar and the side panel's surface

/// The sidebar and the side panel are the plan's lightish, as the header is: COSMIC's background
/// colour in light (#d7d7d7), `chrome_color` (#272727) in dark; edge to edge, square, with nothing
/// of the window behind showing at their corners. Rows, the chosen tab and the disabled tabs still
/// read on it
#[test]
fn the_sidebar_and_the_side_panel_are_the_lightish_surface() {
    use cosmic::iced::Color;
    let lum = |c: Color| {
        let f = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    };
    let contrast = |a: Color, b: Color| {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    let over = |top: Color, under: Color| Color {
        r: top.r * top.a + under.r * (1.0 - top.a),
        g: top.g * top.a + under.g * (1.0 - top.a),
        b: top.b * top.a + under.b * (1.0 - top.a),
        a: 1.0,
    };
    for (light, theme_name) in [(true, "light"), (false, "dark")] {
        let theme = if light { cosmic::Theme::light() } else { cosmic::Theme::dark() };
        let chrome = super::view::chrome_color(&theme);
        assert_eq!(super::view::chrome_color(&theme).into_rgba8()[..3], if light { [0xd7, 0xd7, 0xd7] } else { [0x27, 0x27, 0x27] });
        let mut app = mixed_app();
        app.window_width = 1000.0;
        app.left_width = 240.0;
        app.right_width = 300.0;
        app.show_right = true;
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
        let _ = cosmic::Application::update(&mut app, Message::EditBlock(i));
        let size = Size::new(1000.0, 760.0);
        let frame = draw_in(light, || window(&app), &format!("surface-{theme_name}"), size);
        let px = |x: u32, y: u32| {
            let i = ((y * 1000 + x) * 4) as usize;
            [frame[i], frame[i + 1], frame[i + 2]]
        };
        let same = |p: [u8; 3]| p.iter().zip(chrome.into_rgba8()).all(|(a, b)| a.abs_diff(b) <= 1);
        // the sidebar's and the panel's corners and edges:
        // under the header, at the window's edge, and above the status bar
        // (the header is COSMIC's background colour: the same in light, #1b1b1b and not #272727 in dark)
        assert!(!light || same(px(500, 3)), "{theme_name}: the header is not chrome: {:?}", px(500, 3));
        for (what, x, y) in [
            ("the sidebar's top left corner", 0, 49),
            ("the sidebar's bottom left corner", 0, 726),
            ("the sidebar's top right corner", 239, 49),
            ("the sidebar's bottom right corner", 239, 726),
            ("the side panel's top left corner", 706, 49),
            ("the side panel's bottom left corner", 706, 726),
            ("the side panel's top right corner", 999, 49),
            ("the side panel's bottom right corner", 999, 726),
        ] {
            assert!(same(px(x, y)), "{theme_name}: {what} is {:?}, not the lightish surface", px(x, y));
        }
        // (an empty stretch of each, below its content)
        assert!(same(px(120, 722)) || same(px(120, 700)), "{theme_name}: the sidebar's body: {:?}", px(120, 700));
        assert!(same(px(850, 722)), "{theme_name}: the side panel's body: {:?}", px(850, 722));

        // the selected row's wash (COSMIC's hover wash) shows on it
        let wash = over(theme.cosmic().icon_button.hover.into(), chrome);
        assert!(contrast(wash, chrome) >= 1.08, "{theme_name}: the selected row's wash is {:?} on {:?}: contrast {:.2}", wash, chrome, contrast(wash, chrome));
        let text: Color = theme.cosmic().on_bg_color().into();
        assert!(contrast(text, wash) >= 7.0, "{theme_name}: text on the wash: {:.2}", contrast(text, wash));
        let accent: Color = theme.cosmic().accent_color().into();
        assert!(contrast(accent, wash) >= 3.0, "{theme_name}: the current row's accent text on the wash: {:.2}", contrast(accent, wash));

        // the tab bar is a pill of COSMIC's component fill on the surface; the chosen tab is the
        // muted accent on it; a disabled tab's icon is the text, faded to 35 %, on it
        let bar: Color = theme.cosmic().bg_component_color().into();
        assert!(contrast(bar, chrome) >= 1.05, "{theme_name}: the tab bar {:?} does not stand out from {:?}", bar, chrome);
        let (chosen, label) = super::tones::muted(&theme);
        assert!(contrast(chosen, bar) >= 1.3, "{theme_name}: the chosen tab {:?} on the bar {:?}: {:.2}", chosen, bar, contrast(chosen, bar));
        assert!(contrast(label, chosen) >= 4.5, "{theme_name}: the chosen tab's icon: {:.2}", contrast(label, chosen));
        let dim = over(shell::dimmed(&theme), bar);
        assert!(contrast(dim, bar) >= 1.8, "{theme_name}: a disabled tab's icon {:?} on the bar {:?}: {:.2}", dim, bar, contrast(dim, bar));
    }
}

/// What menus, dropdowns and tooltips are now (the plan's lightest is #f5f5f5 light and #343434
/// dark): recorded, with the frame
#[test]
fn menus_and_tooltips_are_recorded_against_the_lightest() {
    for (light, name, lightest) in [(true, "light", 0xf5f5f5u32), (false, "dark", 0x343434)] {
        let theme = if light { cosmic::Theme::light() } else { cosmic::Theme::dark() };
        let hex = |c: cosmic::iced::Color| {
            let [r, g, b, _] = c.into_rgba8();
            format!("#{r:02x}{g:02x}{b:02x}")
        };
        // (a menu is COSMIC's component colour of its layer; a tooltip is the palette's neutral_2)
        let menu = hex(theme.cosmic().bg_component_color().into());
        let tooltip = hex(theme.cosmic().palette.neutral_2.into());
        eprintln!("{name}: menus {menu}, tooltips {tooltip}; the plan's lightest #{lightest:06x}");
        if light {
            assert_eq!(menu, "#f5f5f5", "light menus are the lightest already");
        }
    }
}

// ----- the status bar and toasts

/// The status bar on the page, as the window draws it
fn bar_of(app: &App) -> Element<'_, Message> {
    widget::container(shell::status_bar(app).unwrap_or_else(|| widget::Space::new().into()))
        .width(Length::Fill)
        .height(Length::Fill)
        .class(cosmic::theme::Container::custom(|t| cosmic::iced::widget::container::Style { background: Some(super::view::page_color(t).into()), ..Default::default() }))
        .into()
}

fn bar_hits(app: &App) -> Vec<(u32, Message)> {
    (0..1400u32).step_by(2).flat_map(|x| clicks(bar_of(app), Point::new(x as f32, 14.0), Size::new(1400.0, 28.0)).into_iter().map(move |m| (x, m))).collect()
}

#[test]
fn the_status_bar_has_back_and_links_to_what_needs_attention() {
    use super::attention::Problem;
    let mut app = mixed_app();
    // no jump yet: only the links, "1 failed" then "1 unfinished"
    let hits = bar_hits(&app);
    let failed = hits.iter().find(|(_, m)| matches!(m, Message::Shell(Msg::NextProblem(Problem::Failed)))).map(|h| h.0).expect("a link to the next failure");
    let unfinished = hits.iter().find(|(_, m)| matches!(m, Message::Shell(Msg::NextProblem(Problem::Unfinished)))).map(|h| h.0).expect("a link to the next unfinished");
    assert!(failed < unfinished);
    assert!(!hits.iter().any(|(_, m)| matches!(m, Message::Back)), "no Back before a jump");
    draw_in(false, || bar_of(&app), "status-links-dark", Size::new(1400.0, 28.0));
    // after a Ctrl+click jump: Back to the cell it came from, before the links
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let at = t.local.find("lemma add_zero").map(|b| t.local[..b].chars().count()).unwrap();
    app.back.push((t.path.clone(), at));
    assert_eq!(shell::back_name(&app).as_deref(), Some("add_zero"));
    let hits = bar_hits(&app);
    let back = hits.iter().find(|(_, m)| matches!(m, Message::Back)).map(|h| h.0).expect("Back");
    assert!(back < failed + 40 && hits.iter().any(|(x, m)| matches!(m, Message::Shell(Msg::NextProblem(Problem::Failed))) && *x > back));
    draw_in(false, || bar_of(&app), "status-back-dark", Size::new(1400.0, 28.0));
    draw_in(true, || bar_of(&app), "status-back-light", Size::new(1400.0, 28.0));
    // a jump into another file is named by the file
    app.back = vec![("/elsewhere/Other.thy".into(), 0)];
    assert_eq!(shell::back_name(&app).as_deref(), Some("Other.thy"));
    // all checked: nothing at the left
    app.back.clear();
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    for b in &mut t.blocks {
        b.status = super::theory::Status::Finished;
        b.commands.retain(|c| c.name != "sorry");
    }
    assert!(bar_hits(&app).is_empty(), "nothing needs attention: nothing to click");
    // the prover's state is not here: no block status, whatever is open
    let lemma = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, Message::EditBlock(lemma));
    assert!(bar_hits(&app).is_empty());
}

#[test]
fn a_link_goes_to_the_next_cell_of_its_kind_round_to_the_first() {
    use super::attention::{Problem, next};
    let mut app = mixed_app();
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let broken = t.local.find("lemma broken").map(|b| t.local[..b].chars().count()).unwrap();
    let comm = t.local.find("lemma add_comm").map(|b| t.local[..b].chars().count()).unwrap();
    assert_eq!(next(t, Problem::Failed), Some(broken));
    assert_eq!(next(t, Problem::Unfinished), Some(comm));
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::NextProblem(Problem::Unfinished)));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(t.cursor, comm);
    assert_eq!(t.selected.map(|i| t.blocks[i].start), Some(comm));
    // from the last one it goes round to the first
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::NextProblem(Problem::Failed)));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(t.cursor, broken);
    assert_eq!(next(t, Problem::Failed), Some(broken), "the only one: round to itself");
}

#[tokio::test]
async fn a_toast_appears_and_goes_by_itself() {
    use cosmic::iced::futures::StreamExt;
    let mut app = demo_app(Mode::Literate);
    assert!(app.shell.toasts.is_empty());
    let task = app.shell.toasts.push("Copied", false, std::time::Duration::from_millis(40));
    assert_eq!(app.shell.toasts.live.iter().map(|l| (l.text.as_str(), l.undo)).collect::<Vec<_>>(), [("Copied", false)]);
    draw_in(false, || app.shell.toasts.over(window(&app)), "toast-dark", Size::new(1400.0, 860.0));
    draw_in(true, || app.shell.toasts.over(window(&app)), "toast-light", Size::new(1400.0, 860.0));
    // its time runs out: the task says so, and it is gone
    let mut stream = cosmic::iced::runtime::task::into_stream(task).expect("a timer");
    let mut gone = None;
    while let Some(action) = stream.next().await {
        if let cosmic::iced::runtime::Action::Output(cosmic::Action::App(Message::Shell(Msg::ToastGone(id)))) = action {
            gone = Some(id);
            break;
        }
    }
    let id = gone.expect("the toast's time ran out");
    assert_eq!(app.shell.toasts.live[0].id, id);
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::ToastGone(id)));
    assert!(app.shell.toasts.is_empty());
    // the real time is a few seconds
    assert!((3..=8).contains(&super::toast::STAY.as_secs()));
}

#[test]
fn deleting_a_block_says_so_in_a_toast_with_an_undo() {
    let mut app = demo_app(Mode::Literate);
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let before = t.local.clone();
    let i = t.blocks.iter().position(|b| b.kind == super::theory::Kind::Formal).unwrap();
    let _ = cosmic::Application::update(&mut app, Message::DeleteBlock(i));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_ne!(t.local, before, "the block is gone");
    assert!(app.status.is_none(), "not in the status bar");
    let live = app.shell.toasts.live.clone();
    assert_eq!(live.iter().map(|l| (l.text.as_str(), l.undo)).collect::<Vec<_>>(), [("Block deleted", true)]);
    // its Undo is Ctrl+Z's step: it asks the host to undo, and the toast goes
    let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::ToastUndo(live[0].id)));
    assert!(app.shell.toasts.is_empty());
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert!(
        t.queued.iter().any(|op| *op == "undo") || app.status.as_deref().is_some_and(|s| s.contains("not connected")) || t.inflight.is_some(),
        "an undo went out (queued {:?}, status {:?})",
        t.queued,
        app.status
    );
    // other passing words are toasts too
    let _ = cosmic::Application::update(&mut app, Message::CopyText("x".into()));
    assert_eq!(app.shell.toasts.live.last().map(|l| (l.text.as_str(), l.undo)), Some(("Copied", false)));
    assert!(app.status.is_none());
}

// ----- the header's order and the menu bar

#[test]
fn the_header_is_sidebar_menus_chip_pause_stop_then_open_in_views_goal_and_panel() {
    let mut app = chip_scene(|a| a.back.push(("/home/me/Research/Demo/Demo.thy".into(), 0)));
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().can_undo = true;
    for width in [1400.0, 1100.0, 900.0] {
        app.window_width = width;
        let hits: Vec<(u32, Message)> = (0..1400u32).step_by(2).flat_map(|x| clicks(header(&app), Point::new(x as f32, 24.0), Size::new(1400.0, 48.0)).into_iter().map(move |m| (x, m))).collect();
        let first = |wanted: &dyn Fn(&Message) -> bool| hits.iter().find(|(_, m)| wanted(m)).map(|h| h.0).unwrap_or_else(|| panic!("no such button at {width}: {hits:?}"));
        let order = [
            first(&|m| matches!(m, Message::ToggleSidebar)),
            first(&|m| matches!(m, Message::Lifecycle("pause"))),
            first(&|m| matches!(m, Message::Lifecycle("stop"))),
            first(&|m| matches!(m, Message::SetMode(Mode::Literate))),
            first(&|m| matches!(m, Message::SetMode(Mode::Source))),
            first(&|m| matches!(m, Message::ToggleGoalBeside)),
            first(&|m| matches!(m, Message::ToggleRight)),
        ];
        assert!(order.windows(2).all(|w| w[0] < w[1]), "at {width}: {order:?}");
        // none of what moved to the menus is a button any more
        for (x, m) in &hits {
            assert!(
                !matches!(m, Message::Undo | Message::Redo | Message::Back | Message::OpenFolder(_) | Message::Shell(Msg::ToggleSettings) | Message::NewSession(_)),
                "{m:?} at {x} is still in the header"
            );
        }
        // the menu bar sits between the sidebar toggle and the chip
        assert!(order[0] + 40 < order[1] - 400, "room for the menus: {order:?}");
        draw_in(false, || header(&app), &format!("header-{}-dark", width as u32), Size::new(1400.0, 48.0));
        draw_in(true, || header(&app), &format!("header-{}-light", width as u32), Size::new(1400.0, 48.0));
    }
    // no session: the same places, the views and the buttons dimmed and not gone
    let none = chip_scene(|a| a.workspace = None);
    let hits: Vec<Message> = (0..1400u32).step_by(2).flat_map(|x| clicks(header(&none), Point::new(x as f32, 24.0), Size::new(1400.0, 48.0))).collect();
    assert!(hits.iter().any(|m| matches!(m, Message::ToggleSidebar)) && hits.iter().any(|m| matches!(m, Message::ToggleRight)));
    assert!(!hits.iter().any(|m| matches!(m, Message::SetMode(_) | Message::Lifecycle(_))), "no session: views, pause and stop are disabled");
    draw_in(false, || header(&none), "header-no-session-dark", Size::new(1400.0, 48.0));
    draw_in(true, || header(&none), "header-no-session-light", Size::new(1400.0, 48.0));
}

#[test]
fn the_menus_hold_what_the_header_gave_up() {
    use super::menubar::{Row, find, labels, menus};
    let app = chip_scene(|a| {
        a.back.push(("/home/me/Research/Demo/Demo.thy".into(), 0));
        a.workspace.as_mut().unwrap().theory.as_mut().unwrap().can_undo = true;
        a.workspace.as_mut().unwrap().caps.interrupt = true;
        a.workspace.as_mut().unwrap().activity = Some(json!({"prover": {"rss_mb": 1300}, "running": [{"name": "by", "seconds": 9}], "theories": []}));
    });
    let all = menus(&app);
    assert_eq!(all.iter().map(|(t, _)| *t).collect::<Vec<_>>(), ["File", "Edit", "View", "Prover", "Help"]);
    let menu = |title: &str| all.iter().find(|(t, _)| *t == title).unwrap().1.clone();
    let msg = |title: &str, label: &str| match find(&menu(title), label) {
        Some(Row::Item { message, .. } | Row::Check { message, .. }) => message.clone(),
        other => panic!("{title} > {label}: {other:?}"),
    };
    // wired to what exists
    assert!(matches!(msg("File", "New session\u{2026}"), Message::NewSession(true)));
    assert!(matches!(msg("File", "Save"), Message::Save));
    assert!(matches!(msg("File", "Files"), Message::OpenFolder(super::link::Opener::Files)));
    assert!(matches!(msg("File", "Zed"), Message::OpenFolder(super::link::Opener::Zed)));
    assert!(matches!(msg("File", "A terminal"), Message::OpenFolder(super::link::Opener::Terminal)));
    assert!(matches!(msg("File", "Close session"), Message::CloseWorkspace));
    assert!(matches!(msg("File", "Quit"), Message::Shell(Msg::Quit)));
    assert!(matches!(msg("Edit", "Undo"), Message::Undo));
    assert!(matches!(msg("Edit", "Copy"), Message::CopySelection(false)));
    assert!(matches!(msg("Edit", "Cut"), Message::CopySelection(true)));
    assert!(matches!(msg("Edit", "Settings"), Message::Shell(Msg::ToggleSettings)));
    assert!(matches!(msg("View", "Sidebar"), Message::ToggleSidebar));
    assert!(matches!(msg("View", "Literate"), Message::SetMode(Mode::Literate)));
    assert!(matches!(msg("View", "Source"), Message::SetMode(Mode::Source)));
    assert!(matches!(msg("View", "Larger text"), Message::Zoom(1)));
    assert!(matches!(msg("View", "Back"), Message::Back));
    assert!(matches!(msg("Prover", "Pause"), Message::Lifecycle("pause")));
    assert!(matches!(msg("Prover", "Stop"), Message::Lifecycle("stop")));
    assert!(matches!(msg("Prover", "Next failure"), Message::Shell(Msg::NextProblem(super::attention::Problem::Failed))));
    assert!(matches!(msg("Prover", "Search theorems\u{2026}"), Message::Panel(super::Panel::Search)));
    assert!(matches!(msg("Prover", "Interrupt running proofs"), Message::Lifecycle("cancel")));
    assert!(matches!(msg("Help", "About Deductum"), Message::Shell(Msg::SettingsAt(super::settings_page::Sub::About))));
    // the shortcuts shown are the ones that exist
    let keys = |title: &str, label: &str| match find(&menu(title), label) {
        Some(Row::Item { keys, .. } | Row::Check { keys, .. } | Row::Off { keys, .. }) => *keys,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        [keys("View", "Literate"), keys("View", "Source"), keys("View", "Sidebar"), keys("View", "Back"), keys("Prover", "Next failure"), keys("Prover", "Next unfinished")],
        ["Ctrl+1", "Ctrl+2", "F9", "Alt+Left", "F8", "Shift+F8"]
    );
    // features not built yet are there, disabled, each saying why
    let not_built = [("Edit", "Copy as Markdown"), ("Edit", "Find"), ("Edit", "Find and replace"), ("Edit", "Paste"), ("Edit", "Select all"), ("File", "Save all"), ("View", "Full screen")];
    for (title, label) in not_built {
        match find(&menu(title), label) {
            Some(Row::Off { why, .. }) => assert!(why.len() > 10, "{label}: {why}"),
            other => panic!("{title} > {label} should be disabled with a reason: {other:?}"),
        }
    }
    assert!(labels(&menu("Edit")).contains(&"(Copy as Markdown)".to_string()));
    // and what cannot be used in this state says why
    let stopped = chip_scene(|a| set_lifecycle(a, "Stopped"));
    let prover = menus(&stopped).into_iter().find(|(t, _)| *t == "Prover").unwrap().1;
    assert!(matches!(find(&prover, "Stop"), Some(Row::Off { .. })));
    assert!(matches!(find(&prover, "Restart"), Some(Row::Item { message: Message::Lifecycle("resume"), .. })));
    assert!(matches!(find(&prover, "Start"), Some(Row::Item { message: Message::Lifecycle("resume"), .. })));
    let none = chip_scene(|a| a.workspace = None);
    let file = menus(&none).into_iter().find(|(t, _)| *t == "File").unwrap().1;
    assert!(matches!(find(&file, "Save"), Some(Row::Off { .. })) && matches!(find(&file, "Close session"), Some(Row::Off { .. })));
    // Ctrl+1, Ctrl+2 and F9 are the app's own keys, F8 and Shift+F8 too: nothing else binds them
    let main = include_str!("main.rs");
    for key in ["Character(\"1\") if modifiers.command()", "Character(\"2\") if modifiers.command()", "Named::F9", "Named::F8) if modifiers.shift()"] {
        assert_eq!(main.matches(key).count(), 1, "{key}");
    }
}

#[test]
fn the_menu_bar_folds_into_one_menu_when_the_window_is_narrow() {
    let mut app = chip_scene(|_| {});
    app.window_width = 1400.0;
    let wide = draw_in(false, || super::menubar::bar(&app), "menubar-wide-dark", Size::new(500.0, 48.0));
    app.window_width = super::menubar::FOLD_WIDTH - 1.0;
    let narrow = draw_in(false, || super::menubar::bar(&app), "menubar-folded-dark", Size::new(500.0, 48.0));
    assert_ne!(wide, narrow, "the folded bar is another drawing");
    // folded, the bar is narrower than the five words
    let width_of = |frame: &[u8]| {
        let (w, h) = (500usize, 48usize);
        let bg = &frame[0..4];
        (0..w).rev().find(|x| (0..h).any(|y| &frame[(y * w + x) * 4..(y * w + x) * 4 + 4] != bg)).unwrap_or(0)
    };
    assert!(width_of(&narrow) < width_of(&wide), "{} < {}", width_of(&narrow), width_of(&wide));
}

/// A frame drawn after the pointer has clicked at `at` (a menu opened, a popover shown)
fn draw_clicked<'a>(light: bool, build: impl FnOnce() -> Element<'a, Message>, at: &[Point], name: &str, size: Size) -> Vec<u8> {
    use cosmic::iced::advanced::clipboard;
    use cosmic::iced::advanced::renderer::{Headless, Style};
    use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
    use cosmic::iced::{Event, Font, Pixels, mouse};
    let _turn = ui_turn();
    cosmic::theme::set_active(if light { cosmic::theme::ThemeType::Light } else { cosmic::theme::ThemeType::Dark });
    super::load_fonts();
    let mut renderer = cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia"))).unwrap();
    let theme = cosmic::theme::active();
    let mut ui = UserInterface::build(build(), size, Cache::default(), &mut renderer);
    let mut messages = Vec::new();
    let mut cursor = mouse::Cursor::Unavailable;
    for p in at {
        cursor = mouse::Cursor::Available(*p);
        for event in [
            Event::Mouse(mouse::Event::CursorMoved { position: *p }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Event::Window(cosmic::iced::window::Event::RedrawRequested(std::time::Instant::now())),
        ] {
            let _ = ui.update(&[event], cursor, &mut renderer, &mut clipboard::Null, &mut messages);
        }
    }
    let text: cosmic::iced::Color = theme.cosmic().on_bg_color().into();
    ui.draw(&mut renderer, &theme, &Style { icon_color: text, text_color: text, scale_factor: 1.0 }, cursor);
    let physical = Size::new(size.width as u32, size.height as u32);
    let rgba = renderer.screenshot(physical, 1.0, theme.cosmic().bg_color().into());
    if let Some(dir) = std::env::var_os("CT_SNAPSHOTS").filter(|d| !d.is_empty()) {
        image::RgbaImage::from_raw(physical.width, physical.height, rgba.clone()).expect("a frame").save(std::path::Path::new(&dir).join(format!("{name}.png"))).expect("saved");
    }
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    rgba
}

#[test]
fn each_menu_opens_under_its_word() {
    let app = chip_scene(|a| a.workspace.as_mut().unwrap().theory.as_mut().unwrap().can_undo = true);
    for (light, theme) in [(true, "light"), (false, "dark")] {
        let closed = draw_in(light, || header(&app), &format!("menu-closed-{theme}"), Size::new(1400.0, 520.0));
        for (name, x) in [("file", 65.0), ("edit", 114.0), ("view", 167.0), ("prover", 229.0), ("help", 292.0)] {
            let open = draw_clicked(light, || header(&app), &[Point::new(x, 24.0)], &format!("menu-{name}-{theme}"), Size::new(1400.0, 520.0));
            assert_ne!(open, closed, "the {name} menu opened ({theme})");
        }
    }
}

// ----- the agent-session button and its popover

#[cfg(not(windows))]
mod agent {
    use super::*;
    use super::super::agent_session::{self as ag, Context, Tool};
    use super::super::settings::Rights;

    const PROMPT: &str = "You are attached.\nDeductum session: Demo (id s1), on neo.\nLemma: add_zero, at line 11.\nA \"quote\", 'another', `tick`, $HOME and \\n.";

    fn ctx(host: Option<&str>) -> Context {
        Context {
            session_id: "s1".into(),
            session_name: "Demo".into(),
            folder: "/home/twright/Proofs/Timed Circus".into(),
            machine: host.map_or("au-laptop".into(), |_| "neo".into()),
            host: host.map(str::to_string),
            theory_path: Some("/home/twright/Proofs/Timed Circus/Stop.thy".into()),
            lemma: Some("map_tock_length".into()),
            line: Some(17),
        }
    }

    #[test]
    fn the_command_for_this_computer_and_for_another_machine() {
        // this computer: no --host
        assert_eq!(
            ag::command(&ctx(None), Tool::Claude, "deductum-s1-7", PROMPT),
            ["cosmic-term", "--new-thread", "/home/twright/Proofs/Timed Circus", "--agent", "claude", "--prompt", PROMPT, "--env", "DEDUCTUM_AGENT_ID=deductum-s1-7"]
        );
        // neo, by its alias, with Codex
        assert_eq!(
            ag::command(&ctx(Some("twright@neo")), Tool::Codex, "deductum-s1-8", PROMPT),
            ["cosmic-term", "--new-thread", "/home/twright/Proofs/Timed Circus", "--host", "twright@neo", "--agent", "codex", "--prompt", PROMPT, "--env", "DEDUCTUM_AGENT_ID=deductum-s1-8"]
        );
        // the prompt stays one argument, whole, through the host's shell
        let args = ag::launch_args(&ag::command(&ctx(None), Tool::Claude, "i", PROMPT));
        assert_eq!(&args[..3], ["-c", args[1].as_str(), "sh"]);
        assert_eq!(args.iter().filter(|a| a.as_str() == PROMPT).count(), 1);
        assert_eq!(ag::agent_id("s1", 7), "deductum-s1-7");
        assert_eq!(ag::agent_key("deductum-s1-7"), "deductum-s1-7:deductum-s1-7", "the key the host gives that id");
        // a tailnet name is not the alias: the machine's own name stands in for it
        let target = |d: Option<&str>| super::super::link::Target { key: "neo".into(), destination: d.map(str::to_string) };
        assert_eq!(ag::host_arg(&target(None)), None);
        assert_eq!(ag::host_arg(&target(Some("twright@neo"))).as_deref(), Some("twright@neo"));
        assert_eq!(ag::host_arg(&target(Some("twright@neo.tail57c02a.ts.net"))).as_deref(), Some("neo"));
    }

    /// The host-side script, run for real with a stand-in `cosmic-term` that records its arguments
    #[test]
    fn the_host_script_passes_the_arguments_whole_and_says_plainly_what_is_missing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("deductum-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for tool in ["grep", "setsid", "cat", "tr"] {
            let found = ["/usr/bin", "/bin"].iter().map(|d| std::path::Path::new(d).join(tool)).find(|p| p.exists()).unwrap();
            std::os::unix::fs::symlink(found, dir.join(tool)).unwrap();
        }
        let run = |args: &[String]| {
            std::process::Command::new("/bin/sh").args(super::super::agent_session::launch_args(args)).env("PATH", &dir).output().unwrap()
        };
        let command = ag::command(&ctx(Some("twright@neo")), Tool::Claude, "deductum-s1-7", PROMPT);
        // not installed: a plain sentence
        let out = run(&command);
        assert_eq!(out.status.code(), Some(127));
        assert!(String::from_utf8_lossy(&out.stderr).contains("is not installed"), "{}", String::from_utf8_lossy(&out.stderr));
        // installed, but it does not know --new-thread
        let fake = dir.join("cosmic-term");
        std::fs::write(&fake, "#!/bin/sh\necho 'Usage: cosmic-term [--thread DIR]'\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = run(&command);
        assert_eq!(out.status.code(), Some(126));
        assert!(String::from_utf8_lossy(&out.stderr).contains("--new-thread"), "{}", String::from_utf8_lossy(&out.stderr));
        // one that does: it is started with the arguments as given, the prompt intact
        let record = dir.join("args");
        std::fs::write(&fake, format!("#!/bin/sh\nif [ \"$1\" = --help ]; then echo '--new-thread DIR'; exit 0; fi\nfor a in \"$@\"; do printf '%s\\0' \"$a\"; done > {}\n", record.display())).unwrap();
        let out = run(&command);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        for _ in 0..100 {
            if record.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let got: Vec<String> = std::fs::read(&record).unwrap().split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).to_string()).collect();
        assert_eq!(got, command[1..], "cosmic-term got exactly its arguments");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_popover_opens_with_the_default_rights_and_claude_and_closes_on_esc_or_cancel() {
        let mut app = demo_app(Mode::Literate);
        assert!(!app.shell.agent.open);
        let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentOpen(true)));
        assert!(app.shell.agent.open);
        assert_eq!((app.shell.agent.rights, app.shell.agent.tool), (Rights::Theory, Tool::Claude), "This theory, Claude");
        // the default is Settings > Agents'
        let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentOpen(false)));
        let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::Setting(super::super::settings::Change::DefaultRights(Rights::Folder))));
        let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentOpen(true)));
        assert_eq!(app.shell.agent.rights, Rights::Folder);
        let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::Setting(super::super::settings::Change::DefaultRights(Rights::Theory))));
        // what it says it is attached to
        let c = ag::context(&app).unwrap();
        assert_eq!(c.theory_name(), Some("Demo.thy"));
        assert_eq!(c.lemma.as_deref(), Some("add_zero"));
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        assert_eq!(c.line, Some(t.local.chars().take(t.cursor).filter(|c| *c == '\n').count() + 1));
        assert_eq!(c.summary(), "In Demo on au-laptop \u{b7} attached to Demo.thy, lemma add_zero");
        assert_eq!(c.host, None, "this computer: no --host");
        // Esc closes it
        let _ = cosmic::Application::update(&mut app, Message::Escape);
        assert!(!app.shell.agent.open);
        // the buttons: four rights, two tools, Open agent session and Cancel
        let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentOpen(true)));
        fn body(app: &App) -> Element<'_, Message> {
            ag::popover_body(app, &ag::context(app).unwrap())
        }
        let hits: Vec<Message> = (0..580).step_by(10).flat_map(|x| (0..300).step_by(12).map(move |y| (x, y))).flat_map(|(x, y)| clicks(body(&app), Point::new(x as f32, y as f32), Size::new(580.0, 300.0))).collect();
        let has = |f: &dyn Fn(&Msg) -> bool| hits.iter().any(|m| matches!(m, Message::Shell(msg) if f(msg)));
        for r in Rights::ALL {
            assert!(has(&|m| matches!(m, Msg::AgentRights(x) if *x == r)), "{r:?}");
        }
        assert!(has(&|m| matches!(m, Msg::AgentTool(Tool::Claude))) && has(&|m| matches!(m, Msg::AgentTool(Tool::Codex))));
        assert!(has(&|m| matches!(m, Msg::AgentStart)) && has(&|m| matches!(m, Msg::AgentOpen(false))));
        draw_in(false, || body(&app), "agent-popover-body-dark", Size::new(580.0, 300.0));
        draw_in(true, || body(&app), "agent-popover-body-light", Size::new(580.0, 300.0));
        // Cancel closes
        let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentOpen(false)));
        assert!(!app.shell.agent.open);
    }

    #[test]
    fn open_agent_session_grants_the_chosen_scope_to_its_id_unless_ask_first() {
        for (rights, scope) in [(Rights::Ask, None), (Rights::Proof, Some("proof")), (Rights::Theory, Some("theory")), (Rights::Folder, Some("folder"))] {
            let mut app = demo_app(Mode::Literate);
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            app.links[0].sender = Some(tx);
            let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentOpen(true)));
            let n = app.shell.agent.n;
            let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentRights(rights)));
            let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentTool(Tool::Codex)));
            let _task = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentStart));
            assert!(!app.shell.agent.open, "the popover closes");
            let mut sent = Vec::new();
            while let Ok(line) = rx.try_recv() {
                sent.push(serde_json::from_str::<Value>(&line).unwrap());
            }
            let grants: Vec<&Value> = sent.iter().filter(|v| v["op"] == "grant").collect();
            match scope {
                None => assert!(grants.is_empty(), "Ask first grants nothing: {sent:?}"),
                Some(scope) => {
                    assert_eq!(grants.len(), 1, "{sent:?}");
                    let id = format!("deductum-s1-{n}");
                    assert_eq!((grants[0]["agent"].as_str(), grants[0]["scope"].as_str(), grants[0]["label"].as_str()), (Some(format!("{id}:{id}").as_str()), Some(scope), Some("Codex")));
                    assert_eq!(grants[0]["session"], "s1");
                }
            }
        }
    }

    #[test]
    fn the_prompt_names_the_session_theory_file_lemma_and_line_and_the_deductum_tools() {
        let p = ag::prompt(&ctx(Some("twright@neo")), Rights::Theory);
        for needle in ["Demo (id s1)", "on neo", "Stop.thy", "/home/twright/Proofs/Timed Circus/Stop.thy", "map_tock_length", "line 17", "deductum MCP tools", "edit this theory"] {
            assert!(p.contains(needle), "{needle}: {p}");
        }
        assert!(p.lines().count() >= 5, "several lines: {p}");
        assert!(ag::prompt(&ctx(None), Rights::Ask).contains("proof_request_access"));
    }

    #[test]
    fn the_split_button_is_the_icon_and_an_arrow_and_the_arrow_menu_has_four_items() {
        let app = demo_app(Mode::Literate);
        for (light, theme) in [(true, "light"), (false, "dark")] {
            draw_in(light, || widget::container(ag::split(&app)).padding(20).into(), &format!("agent-split-{theme}"), Size::new(300.0, 80.0));
            // the icon opens the popover; the arrow's menu has the session first, in bold
            let hits: Vec<Message> = (0..300).step_by(2).flat_map(|x| clicks(widget::container(ag::split(&app)).padding(20).into(), Point::new(x as f32, 40.0), Size::new(300.0, 80.0))).collect();
            assert!(hits.iter().any(|m| matches!(m, Message::Shell(Msg::AgentOpen(true)))), "{hits:?}");
            // the menu, opened: its picture differs from the closed one
            let closed = draw_in(light, || widget::container(ag::split(&app)).padding(20).into(), "agent-split-closed", Size::new(300.0, 260.0));
            let menu = draw_clicked(light, || widget::container(ag::split(&app)).padding(20).into(), &[Point::new(70.0, 38.0)], &format!("agent-menu-{theme}"), Size::new(300.0, 260.0));
            assert_ne!(closed, menu, "the arrow opens a menu ({theme})");
        }
    }

    #[test]
    fn the_popover_hangs_under_the_button_in_the_header() {
        let mut app = chip_scene(|_| {});
        let _ = cosmic::Application::update(&mut app, Message::Shell(Msg::AgentOpen(true)));
        for (light, theme) in [(true, "light"), (false, "dark")] {
            draw_in(light, || window(&app), &format!("agent-popover-{theme}"), Size::new(1400.0, 600.0));
        }
    }
}
#[test]
fn agent_items_are_not_in_the_windows_build() {
    use super::menubar::{agent_file_rows, agent_prover_rows};
    assert!(agent_file_rows(true, true).is_empty() && agent_prover_rows(true, true).is_empty(), "no agent items on Windows");
    // (the rest is cfg: the agent session's module, messages and button are compiled out there)
    let main = include_str!("main.rs");
    assert!(main.contains("#[cfg(not(windows))]\nmod agent_session;"), "the agent session module is not built on Windows");
    #[cfg(not(windows))]
    {
        assert!(!agent_file_rows(false, true).is_empty());
        assert!(!agent_prover_rows(false, true).is_empty());
    }
}


/// The frames for the review of the header (stages/8-header), named as the "before" frames are
#[test]
fn frames_for_the_header_review() {
    let mut app = mixed_app();
    app.show_right = true;
    for (light, theme) in [(true, "light"), (false, "dark")] {
        set_lifecycle(&mut app, "Running");
        app.workspace.as_mut().unwrap().activity = Some(json!({"prover": {"cpu": 385, "rss_mb": 3120}, "running": [{"name": "by", "seconds": 41}], "theories": []}));
        app.workspace.as_mut().unwrap().theory.as_mut().unwrap().prover.status = json!({"finished": 9, "unprocessed": 6, "running": 1, "failed": 0, "consolidated": false});
        draw_in(light, || header(&app), &format!("header-checking-{theme}"), Size::new(1400.0, 48.0));
        {
            let ws = app.workspace.as_mut().unwrap();
            ws.activity = Some(json!({"prover": {"cpu": 2, "rss_mb": 3120}, "running": [], "theories": []}));
            let t = ws.theory.as_mut().unwrap();
            t.prover.status = json!({"finished": 15, "unprocessed": 0, "running": 0, "failed": 0, "consolidated": true});
            for b in &mut t.blocks {
                b.status = super::theory::Status::Finished;
            }
            t.blocks.iter_mut().for_each(|b| b.commands.retain(|c| c.name != "sorry"));
        }
        draw_in(light, || header(&app), &format!("header-checked-{theme}"), Size::new(1400.0, 48.0));
        set_lifecycle(&mut app, "Paused");
        draw_in(light, || header(&app), &format!("header-paused-{theme}"), Size::new(1400.0, 48.0));
        set_lifecycle(&mut app, "Stopped");
        draw_in(light, || header(&app), &format!("header-stopped-{theme}"), Size::new(1400.0, 48.0));
        set_lifecycle(&mut app, "Running");
        // a toast, with its Undo, over the window
        let task = app.shell.toasts.push("Block deleted", true, std::time::Duration::from_secs(5));
        drop(task);
        draw_in(light, || app.shell.toasts.over(window(&app)), &format!("window-toast-{theme}"), Size::new(1400.0, 860.0));
        let ids: Vec<_> = app.shell.toasts.live.iter().map(|l| l.id).collect();
        ids.into_iter().for_each(|id| app.shell.toasts.gone(id));
        app.workspace.as_mut().unwrap().activity = None;
        draw_in(light, || super::sidebar::sidebar(&app), &format!("sidebar-marks-{theme}"), Size::new(300.0, 760.0));
    }
}

#[test]
fn the_chip_logo_is_the_symbolic_icon_in_the_text_colour_and_rocq_keeps_its_letter() {
    let mut frames = Vec::new();
    for backend in ["isabelle", "lean", "rocq"] {
        let mut app = chip_scene(|_| {});
        app.sessions.get_mut(&("local".to_string(), "s1".to_string())).unwrap()["backend"] = json!(backend);
        for (light, theme) in [(true, "light"), (false, "dark")] {
            let frame = draw_in(light, || header(&app), &format!("chip-logo-{backend}-{theme}"), Size::new(1400.0, 48.0));
            if light {
                frames.push(frame);
            }
        }
    }
    assert!(frames[0] != frames[1] && frames[1] != frames[2] && frames[0] != frames[2], "each backend has its own logo");
    // Isabelle's and Lean's are not coloured: in the light theme every pixel of the logo is grey
    let width = 1400usize;
    for frame in &frames[..2] {
        // (the chip is centred: its logo is the first mark after the pill's left edge, 10 to 40 px in)
        let mut coloured = 0;
        for y in 14..34 {
            for x in 520..560 {
                let p = &frame[(y * width + x) * 4..(y * width + x) * 4 + 3];
                let spread = p.iter().max().unwrap() - p.iter().min().unwrap();
                if spread > 24 {
                    coloured += 1;
                }
            }
        }
        assert_eq!(coloured, 0, "a symbolic logo is in the text's colour");
    }
}
