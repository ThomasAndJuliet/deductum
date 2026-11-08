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
    // the header no longer has a Vim button; it has Settings
    app.shell.settings_open = false;
    let found: Vec<Message> = (700..1400).step_by(8).flat_map(|x| clicks(header(&app), Point::new(x as f32, 24.0), Size::new(1400.0, 48.0))).collect();
    assert!(!found.iter().any(|m| matches!(m, Message::ToggleVim)), "{found:?}");
    assert!(found.iter().any(|m| matches!(m, Message::Shell(Msg::ToggleSettings))), "{found:?}");
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
    // a typed sledgehammer running, with its own Stop
    app.workspace.as_mut().unwrap().activity = Some(json!({"prover": {"cpu": 390, "rss_mb": 3200},
        "running": [{"name": "sledgehammer", "seconds": 12, "line": 9}, {"name": "by", "seconds": 7, "line": 12}], "theories": []}));
    draw_in(false, || header(&app), "shell-header-tool", Size::new(1400.0, 48.0));
    let stops = (600..1300).step_by(4).flat_map(|x| clicks(header(&app), Point::new(x as f32, 24.0), Size::new(1400.0, 48.0))).collect::<Vec<_>>();
    assert!(stops.iter().any(|m| matches!(m, Message::StopTools)), "the tool's Stop: {stops:?}");
    // no Interrupt where the session cannot interrupt
    app.workspace.as_mut().unwrap().caps.interrupt = false;
    let none = (700..1300).step_by(4).flat_map(|x| clicks(header(&app), Point::new(x as f32, 24.0), Size::new(1400.0, 48.0))).collect::<Vec<_>>();
    assert!(!none.iter().any(|m| matches!(m, Message::Lifecycle("cancel"))), "{none:?}");
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
    app.status = Some("Block deleted (Ctrl+Z brings it back)".into());
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

/// Status is a mark and words, never ▶ ⏸ ■: a filled green dot, a hollow amber ring, nothing for
/// a stopped session, the error icon for a failed one, a spinner while it checks
#[test]
fn a_running_session_shows_a_dot_and_the_word_running() {
    use super::sidebar::{Mark, caption_line, mark_of};
    assert_eq!(mark_of("Running", false), Mark::Dot);
    assert_eq!(mark_of("Running", true), Mark::Spinner);
    assert_eq!(mark_of("Paused", false), Mark::Ring);
    assert_eq!(mark_of("Stopped", false), Mark::Blank);
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
