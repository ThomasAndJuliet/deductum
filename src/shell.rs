// SPDX-License-Identifier: GPL-3.0-only

//! The app's shell around the document: the header bar, the status bar at the window's foot, the
//! settings and what changing them does, and project actions (design: provers §5.2, §6.4).
//!
//! The document area and the right sidebar's panels are `view`'s; the left sidebar is
//! `sidebar`'s, the settings page `settings_page`'s and the new-session flow `new_session`'s.
//! What the shell keeps between frames is [`Shell`], one field of the app, and what it is asked
//! to do arrives as [`Msg`] inside `Message::Shell`, so that the main update loop only hands it on.

use std::collections::HashMap;

use cosmic::iced::{Alignment, Border, Color, Length, Padding};
use cosmic::Application;
use cosmic::app::Task;
use cosmic::prelude::*;
use cosmic::widget::{self, button};
use cosmic::{iced, theme};
use serde_json::{Value, json};

use super::editor::Mode;
use super::settings::{Loogle, Settings, ThemeChoice};
use super::theory::Status;
use super::{App, Message, Panel, Pending, Workspace, file_name, link};

/// What the shell keeps between frames
#[derive(Default)]
pub struct Shell {
    pub settings: Settings,
    /// The settings page is shown instead of the document
    pub settings_open: bool,
    /// Project actions' runs, by session and action name: running, or how the last one ended
    pub runs: HashMap<(String, String), Run>,
    /// Sessions whose document list is collapsed in the sidebar (only the open document shows),
    /// by machine and session
    pub collapsed: std::collections::HashSet<(String, String)>,
    /// Folders collapsed in a session's document list, by machine, session and folder
    pub collapsed_dirs: std::collections::HashSet<(String, String, String)>,
}

/// A project action's run (`lake build`, building a heap)
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub running: bool,
    pub ok: Option<bool>,
    /// The last lines it printed
    pub output: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Show the settings page (true) or the document again
    Settings(bool),
    ToggleSettings,
    Theme(ThemeChoice),
    /// The text size, in percent
    TextSize(i32),
    GoalBeside(bool),
    Vim(bool),
    IndentWidth(u8),
    CompleteWhileTyping(bool),
    Keywords(bool),
    /// The pause before a keyword line asks, in milliseconds
    KeywordPause(u32),
    ShowTypes(bool),
    ShowChanges(bool),
    /// Where this session's keyword lines go: the route's index among those offered
    Route(usize),
    ClaudeModel(String),
    CodexModel(String),
    Loogle(Loogle),
    /// Run one of the session's project actions, by name
    ProjectAction(String),
    /// The new-session flow: the backend card picked, the toolchain picked (index among the
    /// backend's), the session directories typed, a document clicked in the folder browser
    NewBackend(String),
    NewToolchain(usize),
    NewDirs(String),
    OpenDocument(String),
    /// Collapse or expand the open session's whole document list
    ToggleDocuments,
    /// Collapse or expand one folder of the open session's document list
    ToggleDocFolder(String),
}

impl App {
    /// Settings as they are now, with what lives elsewhere while the app runs gathered in
    pub fn gather_settings(&mut self) {
        let s = &mut self.shell.settings;
        s.editing.vim = self.vim_enabled;
        s.appearance.zoom = super::view::zoom();
        s.appearance.goal_beside = super::view::goal_beside_on();
        s.layout.left_width = self.left_width;
        s.layout.right_width = self.right_width;
        s.layout.show_left = self.show_sidebar;
        s.layout.show_right = self.show_right;
    }

    /// Keep the settings: gathered, published to the code that reads them, and written
    pub fn save_settings(&mut self) {
        self.gather_settings();
        super::settings::publish(&self.shell.settings);
        self.shell.settings.save();
    }

    /// The settings read at start, applied: the window's fields, the view's, and the theme
    pub fn apply_loaded_settings(&mut self, settings: Settings) -> Task<Message> {
        self.vim_enabled = settings.editing.vim;
        super::view::set_zoom(settings.appearance.zoom);
        super::view::set_goal_beside(settings.appearance.goal_beside);
        self.left_width = settings.layout.left_width;
        self.right_width = settings.layout.right_width;
        self.show_sidebar = settings.layout.show_left;
        self.show_right = settings.layout.show_right;
        let theme = settings.appearance.theme;
        super::settings::publish(&settings);
        self.shell.settings = settings;
        // (the desktop's theme is libcosmic's own default: nothing to set for it)
        if theme == ThemeChoice::System { Task::none() } else { cosmic::command::set_theme(theme.theme()) }
    }

    /// Tell a machine's host what this app wants of keyword lines and searches (the models to
    /// ask, whether keyword lines ask at all, Loogle)
    pub fn tell_host(&mut self, machine: &str) {
        if self.link(machine).is_some_and(|l| l.sender.is_some()) {
            let params = self.shell.settings.host_json();
            self.send(machine, "client_settings", params, Pending::Plain);
        }
    }

    fn tell_hosts(&mut self) {
        let keys: Vec<String> = self.links.iter().filter(|l| l.sender.is_some()).map(|l| l.target.key.clone()).collect();
        for k in keys {
            self.tell_host(&k);
        }
    }

    pub fn shell_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Settings(open) => {
                self.shell.settings_open = open;
                if open {
                    self.new_session = None;
                }
            }
            Msg::ToggleSettings => return self.shell_update(Msg::Settings(!self.shell.settings_open)),
            Msg::Theme(choice) => {
                self.shell.settings.appearance.theme = choice;
                self.save_settings();
                return cosmic::command::set_theme(choice.theme());
            }
            Msg::TextSize(percent) => {
                super::view::set_zoom(percent as f32 / 100.0);
                self.save_settings();
            }
            Msg::GoalBeside(on) => {
                super::view::set_goal_beside(on);
                self.save_settings();
            }
            Msg::Vim(on) => {
                if on != self.vim_enabled {
                    return self.update(Message::ToggleVim);
                }
            }
            Msg::IndentWidth(n) => {
                self.shell.settings.editing.indent_width = n.clamp(1, 8);
                self.save_settings();
            }
            Msg::CompleteWhileTyping(on) => {
                self.shell.settings.editing.complete_while_typing = on;
                self.save_settings();
            }
            Msg::Keywords(on) => {
                self.shell.settings.editing.keywords = on;
                self.save_settings();
                self.tell_hosts();
            }
            Msg::KeywordPause(ms) => {
                self.shell.settings.editing.keyword_pause_ms = ms.clamp(500, 10_000);
                self.save_settings();
            }
            Msg::ShowTypes(on) => {
                self.shell.settings.proof_state.show_types = on;
                self.save_settings();
            }
            Msg::ShowChanges(on) => {
                self.shell.settings.proof_state.show_changes = on;
                self.save_settings();
                return self.schedule_state();
            }
            Msg::Route(i) => {
                let routes = self.workspace.as_ref().map(routes).unwrap_or_default();
                if let Some((value, _)) = routes.get(i) {
                    return self.update(Message::QueryRoute(value.clone()));
                }
            }
            Msg::ClaudeModel(m) => {
                self.shell.settings.llm.claude_model = m;
                self.save_settings();
                self.tell_hosts();
            }
            Msg::CodexModel(m) => {
                self.shell.settings.llm.codex_model = m;
                self.save_settings();
                self.tell_hosts();
            }
            Msg::Loogle(l) => {
                self.shell.settings.lean.loogle = l;
                self.save_settings();
                self.tell_hosts();
            }
            Msg::ToggleDocuments => {
                if let Some(key) = self.workspace_ids() {
                    if !self.shell.collapsed.remove(&key) {
                        self.shell.collapsed.insert(key);
                    }
                }
            }
            Msg::ToggleDocFolder(dir) => {
                if let Some((machine, sid)) = self.workspace_ids() {
                    let key = (machine, sid, dir);
                    if !self.shell.collapsed_dirs.remove(&key) {
                        self.shell.collapsed_dirs.insert(key);
                    }
                }
            }
            Msg::ProjectAction(name) => {
                let Some((machine, sid)) = self.workspace_ids() else { return Task::none() };
                let file = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).map(|t| t.path.clone());
                let mut args = json!({});
                if let Some(f) = file {
                    args["file"] = json!(f);
                }
                self.shell.runs.insert((sid.clone(), name.clone()), Run { running: true, ..Default::default() });
                self.send(&machine, "project_action", json!({"session": sid, "name": name, "args": args}), Pending::ProjectAction(sid.clone(), name));
            }
            Msg::NewBackend(b) => return self.new_session_backend(b),
            Msg::NewToolchain(i) => {
                let toolchains = self.new_session.as_ref().map(|ns| self.toolchains(ns.machine, ns.backend.as_deref().unwrap_or("isabelle"))).unwrap_or_default();
                if let (Some(ns), Some(t)) = (&mut self.new_session, toolchains.get(i)) {
                    ns.toolchain = t["id"].as_str().map(str::to_string);
                }
            }
            Msg::NewDirs(d) => {
                if let Some(ns) = &mut self.new_session {
                    ns.dirs = d;
                }
            }
            Msg::OpenDocument(path) => return self.open_document(path),
        }
        Task::none()
    }

    /// A project action's reply
    pub fn project_action_done(&mut self, sid: String, name: String, v: &Value) {
        let result = &v["result"];
        let run = self.shell.runs.entry((sid, name)).or_default();
        run.running = false;
        run.ok = Some(v["ok"] == json!(true) && result["ok"] != json!(false));
        run.output = result["output_tail"].as_str().or(v["error"].as_str()).unwrap_or("").to_string();
    }
}

/// The routes this session's keyword lines can take, with their labels: the persistent helper, a
/// fresh request each time, or a listening agent
pub fn routes(ws: &Workspace) -> Vec<(String, String)> {
    let mut out = vec![
        ("helper".to_string(), "The session's helper conversation".to_string()),
        ("fresh".to_string(), "A fresh request each time".to_string()),
    ];
    for l in ws.activity.as_ref().and_then(|a| a["listeners"].as_array()).into_iter().flatten() {
        if let (Some(key), Some(label)) = (l["key"].as_str(), l["label"].as_str()) {
            out.push((format!("agent:{key}"), format!("@{label}, listening")));
        }
    }
    out
}

/// The route the session's keyword lines take now
pub fn current_route(ws: &Workspace) -> String {
    ws.activity.as_ref().and_then(|a| a["route"].as_str()).unwrap_or("helper").to_string()
}

// ----- small pieces shared by the shell's views

pub fn sp() -> cosmic::cosmic_theme::Spacing {
    theme::active().cosmic().spacing
}

/// An icon button with a tooltip under it
pub fn icon_button<'a>(name: &'static str, tip: impl Into<String>, message: Option<Message>) -> Element<'a, Message> {
    let b = button::icon(icon_handle(name)).on_press_maybe(message);
    widget::tooltip(b, widget::text(tip.into()), widget::tooltip::Position::Bottom).into()
}

/// The sidebars' toggles have no icon in COSMIC's theme (nor in what it inherits, as installed):
/// these are drawn here, a window with its left or right side filled
const SIDEBAR_LEFT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="#000" fill-rule="evenodd" d="M3 1.5h10A2.5 2.5 0 0 1 15.5 4v8a2.5 2.5 0 0 1-2.5 2.5H3A2.5 2.5 0 0 1 .5 12V4A2.5 2.5 0 0 1 3 1.5zM2 4v8a1 1 0 0 0 1 1h3V3H3a1 1 0 0 0-1 1zm5.5-1v10H13a1 1 0 0 0 1-1V4a1 1 0 0 0-1-1z"/><path fill="#000" fill-opacity=".4" d="M3 3h3v10H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/></svg>"##;
const SIDEBAR_RIGHT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="#000" fill-rule="evenodd" d="M3 1.5h10A2.5 2.5 0 0 1 15.5 4v8a2.5 2.5 0 0 1-2.5 2.5H3A2.5 2.5 0 0 1 .5 12V4A2.5 2.5 0 0 1 3 1.5zM2 4v8a1 1 0 0 0 1 1h5.5V3H3a1 1 0 0 0-1 1zm8-1v10h3a1 1 0 0 0 1-1V4a1 1 0 0 0-1-1z"/><path fill="#000" fill-opacity=".4" d="M10 3h3a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-3z"/></svg>"##;

/// An icon by its freedesktop name, or one of the app's own (`sidebar-left`, `sidebar-right`)
pub fn icon_handle(name: &'static str) -> widget::icon::Handle {
    match name {
        "sidebar-left" => widget::icon::from_svg_bytes(SIDEBAR_LEFT).symbolic(true),
        "sidebar-right" => widget::icon::from_svg_bytes(SIDEBAR_RIGHT).symbolic(true),
        _ => widget::icon::from_name(name).handle(),
    }
}

/// A thin vertical line between groups of a bar
pub fn divider<'a>(height: f32) -> Element<'a, Message> {
    widget::container(widget::Space::new())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(height))
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color { a: 0.2, ..Color::from(theme.cosmic().on_bg_color()) }.into()),
            ..Default::default()
        }))
        .into()
}

/// A small spinner, for work in progress. Its faint ring shows that it is one even in a still frame
pub fn spinner<'a>(size: f32) -> Element<'a, Message> {
    let text: Color = theme::active().cosmic().on_bg_color().into();
    widget::progress_bar::circular::Circular::new().size(size).bar_height(2.0).track_color(Color { a: 0.18, ..text }).into()
}

/// A short label on a rounded tinted background: a count, a state
pub fn badge<'a>(text: String, tint: fn(&cosmic::Theme) -> Color) -> Element<'a, Message> {
    widget::container(widget::text::caption(text).font(cosmic::font::semibold()))
        .padding([0, 6])
        .class(theme::Container::custom(move |theme| {
            let c = tint(theme);
            iced::widget::container::Style {
                background: Some(Color { a: 0.22, ..c }.into()),
                text_color: Some(readable_on(theme, c)),
                border: Border { radius: 8.0.into(), ..Default::default() },
                ..Default::default()
            }
        }))
        .into()
}

/// A colour's own hue, deep enough to read on the theme's page (lighter in a dark theme, darker
/// in a light one)
pub fn readable_on(theme: &cosmic::Theme, c: Color) -> Color {
    let text: Color = theme.cosmic().on_bg_color().into();
    if theme.cosmic().is_dark { blend(c, Color { a: c.a, ..text }, 0.35) } else { blend(c, Color { a: c.a, ..Color::BLACK }, 0.3) }
}

pub fn blend(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

pub fn destructive(theme: &cosmic::Theme) -> Color {
    theme.cosmic().destructive_color().into()
}

pub fn warning(theme: &cosmic::Theme) -> Color {
    theme.cosmic().warning_color().into()
}

pub fn success(theme: &cosmic::Theme) -> Color {
    theme.cosmic().success_color().into()
}

pub fn accent(theme: &cosmic::Theme) -> Color {
    theme.cosmic().accent_color().into()
}

/// An icon drawn in a colour of the theme. An icon's tint has no transparency of its own (only
/// its colour is taken), so a faint colour is mixed with the page here
pub fn tinted_icon<'a>(name: &'static str, size: u16, tint: fn(&cosmic::Theme) -> Color) -> Element<'a, Message> {
    widget::icon::from_name(name)
        .size(size)
        .icon()
        .class(theme::Svg::custom(move |theme| {
            let c = readable_on(theme, tint(theme));
            let page: Color = theme.cosmic().bg_color().into();
            iced::widget::svg::Style { color: Some(Color { a: 1.0, ..blend(page, Color { a: 1.0, ..c }, c.a) }) }
        }))
        .into()
}

/// The text's colour, faded towards the page (for icons that go with secondary text)
pub fn muted(theme: &cosmic::Theme) -> Color {
    Color { a: 0.7, ..Color::from(theme.cosmic().on_bg_color()) }
}

/// The text's colour, faded further (for what cannot be used yet)
pub fn dimmed(theme: &cosmic::Theme) -> Color {
    Color { a: 0.35, ..Color::from(theme.cosmic().on_bg_color()) }
}

/// A backend's mark: a small rounded square with its initial, in its colour
pub fn backend_mark<'a>(backend: &str) -> Element<'a, Message> {
    let (letter, rgb): (&str, [u8; 3]) = match backend {
        "lean" => ("L", [0x3a, 0x8f, 0xe8]),
        "rocq" | "coq" => ("R", [0xe8, 0x7a, 0x2c]),
        _ => ("I", [0xd9, 0x9a, 0x1e]),
    };
    let base = Color::from_rgb8(rgb[0], rgb[1], rgb[2]);
    widget::tooltip(
        widget::container(widget::text(letter).size(11.0).font(cosmic::font::bold()))
            .width(Length::Fixed(18.0))
            .height(Length::Fixed(18.0))
            .center_x(Length::Fixed(18.0))
            .center_y(Length::Fixed(18.0))
            .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                background: Some(Color { a: 0.24, ..base }.into()),
                text_color: Some(readable_on(theme, base)),
                border: Border { radius: 4.0.into(), width: 1.0, color: Color { a: 0.55, ..base } },
                ..Default::default()
            })),
        widget::text(backend_label(backend)),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

pub fn backend_label(backend: &str) -> &'static str {
    match backend {
        "lean" => "Lean",
        "rocq" | "coq" => "Rocq",
        _ => "Isabelle",
    }
}

// ----- the header bar

/// The header's start: the sidebar's toggle, the view switch, then undo and redo apart from it
pub fn header_start(app: &App) -> Vec<Element<'_, Message>> {
    let mut items: Vec<Element<'_, Message>> = vec![icon_button("sidebar-left", "Show or hide the sidebar", Some(Message::ToggleSidebar))];
    if app.shell.settings_open {
        return items;
    }
    if let Some(t) = app.workspace.as_ref().and_then(|w| w.theory.as_ref()) {
        items.push(widget::Space::new().width(Length::Fixed(4.0)).into());
        items.push(mode_switch(t.mode, app.window_width < 1100.0));
        items.push(widget::Space::new().width(Length::Fixed(sp().space_s as f32)).into());
        items.push(icon_button("edit-undo-symbolic", "Undo your last edit (Ctrl+Z)", t.can_undo.then_some(Message::Undo)));
        items.push(icon_button("edit-redo-symbolic", "Redo (Ctrl+Shift+Z)", t.can_redo.then_some(Message::Redo)));
        if !app.back.is_empty() {
            items.push(icon_button("go-previous-symbolic", "Back to where Ctrl+click came from (Alt+Left)", Some(Message::Back)));
        }
    }
    items
}

/// Literate or Source: two equal segments in one rounded frame, the current one filled
/// (`narrow`: segments as wide as their words, for a narrow window's header)
pub fn mode_switch<'a>(mode: Mode, narrow: bool) -> Element<'a, Message> {
    let segment = |label: &'static str, this: Mode| {
        let on = mode == this;
        button::custom(widget::text::body(label).width(Length::Fill).align_x(Alignment::Center))
            .width(Length::Fixed(if narrow { 76.0 } else { 92.0 }))
            .padding([5, 10])
            .class(segment_style(on))
            .on_press(Message::SetMode(this))
    };
    widget::container(widget::row::with_capacity(2).spacing(2).push(segment("Literate", Mode::Literate)).push(segment("Source", Mode::Source)))
        .padding(2)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color::from(theme.cosmic().bg_component_color()).into()),
            border: Border { radius: 16.0.into(), width: 1.0, color: Color { a: 0.12, ..Color::from(theme.cosmic().on_bg_color()) } },
            ..Default::default()
        }))
        .into()
}

/// A segment of a switch or a tab: filled with the accent when it is the current one
pub fn segment_style(on: bool) -> theme::Button {
    fn style(theme: &cosmic::Theme, on: bool, hover: bool) -> button::Style {
        let c = theme.cosmic();
        let text: Color = c.on_bg_color().into();
        let mut s = button::Style::new();
        s.border_radius = 14.0.into();
        if on {
            s.background = Some(Color::from(c.accent_color()).into());
            s.text_color = Some(c.on_accent_color().into());
            s.icon_color = Some(c.on_accent_color().into());
        } else {
            s.background = hover.then(|| Color { a: 0.08, ..text }.into());
            s.text_color = Some(text);
            s.icon_color = Some(text);
        }
        s
    }
    theme::Button::Custom {
        active: Box::new(move |_, t| style(t, on, false)),
        disabled: Box::new(move |t| {
            let mut s = style(t, on, false);
            s.text_color = s.text_color.map(|c| Color { a: 0.4, ..c });
            s.icon_color = s.icon_color.map(|c| Color { a: 0.4, ..c });
            s
        }),
        hovered: Box::new(move |_, t| style(t, on, true)),
        pressed: Box::new(move |_, t| style(t, on, true)),
    }
}

/// The header's end: the session's lifecycle, its folder in other apps, the goal's place, the
/// settings and the right sidebar
pub fn header_end(app: &App) -> Vec<Element<'_, Message>> {
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    let settings = icon_button("preferences-system-symbolic", "Settings (Ctrl+,)", Some(Message::Shell(Msg::ToggleSettings)));
    let Some(ws) = &app.workspace else {
        if app.new_session.is_none() && !app.shell.settings_open {
            items.push(widget::button::suggested("New session").on_press(Message::NewSession(true)).into());
        }
        items.push(settings);
        return items;
    };
    if !app.shell.settings_open {
        let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
        let lifecycle = summary.and_then(|s| s["lifecycle"].as_str()).unwrap_or("?").to_string();
        match lifecycle.as_str() {
            "Running" => {
                items.push(icon_button("media-playback-pause-symbolic", "Pause: keep the prover loaded, stop checking", Some(Message::Lifecycle("pause"))));
                items.push(icon_button("system-shutdown-symbolic", "Stop the prover (buffers are kept)", Some(Message::Lifecycle("stop"))));
            }
            "Paused" => {
                items.push(icon_button("media-playback-start-symbolic", "Resume checking", Some(Message::Lifecycle("resume"))));
                items.push(icon_button("system-shutdown-symbolic", "Stop the prover (buffers are kept)", Some(Message::Lifecycle("stop"))));
            }
            "Stopped" | "Failed" => {
                items.push(widget::button::suggested("Resume").on_press(Message::Lifecycle("resume")).into());
                items.push(icon_button("user-trash-symbolic", "Forget this session (documents are saved; an agent's unmerged work is refused)", Some(Message::Lifecycle("discard"))));
            }
            _ => {}
        }
        // (a narrow window keeps its room for the document's own controls)
        if ws.info.is_some() && app.window_width >= 1250.0 {
            items.push(widget::Space::new().width(Length::Fixed(sp().space_xxs as f32)).into());
            items.push(icon_button("folder-open-symbolic", "Open the session's folder in Files", Some(Message::OpenFolder(link::Opener::Files))));
            items.push(icon_button("accessories-text-editor-symbolic", "Open the session's folder in Zed", Some(Message::OpenFolder(link::Opener::Zed))));
            items.push(icon_button("utilities-terminal-symbolic", "Open a terminal in the session's folder", Some(Message::OpenFolder(link::Opener::Terminal))));
        }
        items.push(widget::Space::new().width(Length::Fixed(sp().space_xxs as f32)).into());
        // The goal beside the text when there is room (off: under the construct, the page centred)
        let beside = super::view::goal_beside_on();
        items.push(
            widget::tooltip(
                widget::button::icon(widget::icon::from_name(if beside { "view-dual-symbolic" } else { "view-paged-symbolic" }))
                    .selected(beside)
                    .on_press(Message::ToggleGoalBeside),
                widget::text(if beside {
                    "The goal beside the text when there is room: click to keep it under the proof, the page centred"
                } else {
                    "The goal under the proof, the page centred: click to put it beside the text when there is room"
                }),
                widget::tooltip::Position::Bottom,
            )
            .into(),
        );
    }
    items.push(settings);
    if !app.shell.settings_open {
        items.push(icon_button("sidebar-right", "Show or hide proof output and tools", Some(Message::ToggleRight)));
    }
    items
}

/// The prover's state in the header bar, as COSMIC apps show work in progress: never an idle-looking
/// editor while the prover is busy. Its parts are apart (the toolchain, the state, what fails,
/// what it uses), a running tool has its own Stop, and Interrupt shows when the session can
pub fn header_center(app: &App) -> Vec<Element<'_, Message>> {
    let Some(ws) = &app.workspace else { return Vec::new() };
    if app.shell.settings_open {
        return Vec::new();
    }
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let lifecycle = summary.and_then(|s| s["lifecycle"].as_str()).unwrap_or("").to_string();
    let toolchain = summary.and_then(|s| s["toolchain"]["label"].as_str()).filter(|l| !l.is_empty()).map(str::to_string);
    let backend = summary.and_then(|s| s["backend"].as_str()).unwrap_or("isabelle").to_string();
    match (&ws.theory, lifecycle.as_str()) {
        (Some(t), "Running") => prover_status(ws, t, &backend, toolchain, app.window_width < 1250.0, app.window_width < 1100.0).into_iter().collect(),
        (_, "") => Vec::new(),
        (_, lc) => {
            let (icon_name, tint): (&'static str, fn(&cosmic::Theme) -> Color) = match lc {
                "Paused" => ("media-playback-pause-symbolic", warning),
                "Failed" => ("dialog-error-symbolic", destructive),
                "Stopped" => ("media-playback-stop-symbolic", |t: &cosmic::Theme| Color::from(t.cosmic().on_bg_color())),
                _ => ("media-playback-start-symbolic", success),
            };
            let mut row = widget::row::with_capacity(5).spacing(sp().space_xs).align_y(Alignment::Center);
            row = row.push(backend_mark(&backend));
            if let Some(tc) = toolchain {
                row = row.push(widget::text::caption(tc)).push(divider(14.0));
            }
            row = row.push(tinted_icon(icon_name, 16, tint)).push(widget::text::body(match lc {
                "Paused" => "Paused".to_string(),
                "Stopped" => "Stopped".to_string(),
                "Failed" => "Failed".to_string(),
                other => other.to_string(),
            }));
            vec![chip(row.into(), "Prover activity and log", Some(Message::Panel(Panel::Log)))]
        }
    }
}

/// A rounded chip in the header, pressable, with a tooltip
fn chip<'a>(content: Element<'a, Message>, tip: impl Into<String>, press: Option<Message>) -> Element<'a, Message> {
    widget::tooltip(
        button::custom(content).padding([3, 12]).class(chip_style()).on_press_maybe(press),
        widget::text(tip.into()),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

fn chip_style() -> theme::Button {
    fn style(theme: &cosmic::Theme, hover: bool) -> button::Style {
        let c = theme.cosmic();
        let text: Color = c.on_bg_color().into();
        let mut s = button::Style::new();
        s.border_radius = 16.0.into();
        s.background = Some(blend(Color::from(c.bg_component_color()), text, if hover { 0.08 } else { 0.0 }).into());
        s.border_width = 1.0;
        s.border_color = Color { a: 0.10, ..text };
        s.text_color = Some(text);
        s.icon_color = Some(text);
        s
    }
    theme::Button::Custom {
        active: Box::new(|_, t| style(t, false)),
        disabled: Box::new(|t| style(t, false)),
        hovered: Box::new(|_, t| style(t, true)),
        pressed: Box::new(|_, t| style(t, true)),
    }
}

/// The theory's imports, transitively, that the prover has not finished checking: each one's
/// file name, how far it is, and whether it cannot load. The theory's own commands wait for
/// these, so the header and the goal panel both say so
pub fn pending_imports(ws: &Workspace, t: &super::editor::Theory) -> Vec<(String, u64, bool)> {
    let Some(theories) = ws.activity.as_ref().and_then(|a| a["theories"].as_array()) else { return Vec::new() };
    let find = |path: &str| theories.iter().find(|th| th["path"].as_str() == Some(path));
    let mut waiting: Vec<(String, u64, bool)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<String> = find(&t.path)
        .and_then(|th| th["imports"].as_array())
        .map(|i| i.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    while let Some(p) = stack.pop() {
        if !seen.insert(p.clone()) {
            continue;
        }
        if let Some(th) = find(&p) {
            if th["consolidated"].as_bool() != Some(true) {
                let failed = th["failed"].as_u64().unwrap_or(0) > 0 || th["header_errors"].as_array().is_some_and(|e| !e.is_empty());
                waiting.push((file_name(&p).to_string(), th["percentage"].as_u64().unwrap_or(0), failed));
            }
            for i in th["imports"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
                stack.push(i.to_string());
            }
        }
    }
    waiting
}

/// (`compact`, in a narrow window: the state and what fails only)
/// (`narrow`, narrower still: no backend mark, and a running tool without its time)
fn prover_status<'a>(ws: &'a Workspace, t: &'a super::editor::Theory, backend: &str, toolchain: Option<String>, compact: bool, narrow: bool) -> Option<Element<'a, Message>> {
    let act = ws.activity.as_ref()?;
    let theories = act["theories"].as_array()?;
    let waiting = pending_imports(ws, t);
    let running = act["running"].as_array().cloned().unwrap_or_default();
    let busy_theories = theories.iter().filter(|th| th["running"].as_u64().unwrap_or(0) > 0).count();
    let (finished, unprocessed, run_here, failed, consolidated) = t.status();
    let total = (finished + unprocessed + run_here + failed).max(1);
    let prover = &act["prover"];
    let busy = !running.is_empty() || busy_theories > 0;
    let s = sp();
    // The state, in words
    let state = if !waiting.is_empty() {
        let blocked = waiting.iter().filter(|w| w.2).count();
        if blocked > 0 {
            format!("{blocked} import{} cannot load", if blocked == 1 { "" } else { "s" })
        } else {
            format!("Waiting for {} import{}", waiting.len(), if waiting.len() == 1 { "" } else { "s" })
        }
    } else if busy || (!consolidated && !t.prover.status.is_null()) {
        format!("Checking {}%", (finished + failed) * 100 / total)
    } else if !t.prover.status.is_null() {
        "Checked".to_string()
    } else {
        "Starting".to_string()
    };
    let mut row = widget::row::with_capacity(10).spacing(s.space_xs).align_y(Alignment::Center);
    if !narrow {
        row = row.push(backend_mark(backend));
    }
    if let Some(tc) = toolchain.filter(|_| !compact) {
        row = row.push(widget::text::caption(tc).wrapping(iced::widget::text::Wrapping::None)).push(divider(14.0));
    }
    if busy {
        row = row.push(spinner(14.0));
    } else if waiting.is_empty() && !t.prover.status.is_null() && failed == 0 {
        row = row.push(tinted_icon("emblem-ok-symbolic", 14, success));
    }
    row = row.push(widget::text::body(state).wrapping(iced::widget::text::Wrapping::None));
    if failed > 0 {
        row = row.push(badge(format!("{failed} failing"), destructive));
    }
    let is_tool = |r: &Value| r["name"].as_str().is_some_and(|n| ws.caps.typed_tool(n).is_some());
    // The command that has run longest, by its name as typed
    if let Some(longest) = running.iter().find(|r| !is_tool(r))
        && longest["seconds"].as_u64().unwrap_or(0) >= 5
        && !compact
    {
        row = row
            .push(widget::text(longest["name"].as_str().unwrap_or("").to_string()).size(12.0).font(cosmic::font::mono()).wrapping(iced::widget::text::Wrapping::None))
            .push(widget::text::caption(format!("{} s", longest["seconds"])).wrapping(iced::widget::text::Wrapping::None));
    }
    let mut resources = Vec::new();
    if let Some(cpu) = prover["cpu"].as_u64().filter(|c| *c >= 5) {
        resources.push(format!("CPU {cpu}%"));
    }
    if let Some(mb) = prover["rss_mb"].as_u64() {
        resources.push(format!("{:.1} GB", mb as f64 / 1000.0));
    }
    if !resources.is_empty() && !compact {
        row = row.push(divider(14.0));
        for r in resources {
            row = row.push(widget::text::caption(r).wrapping(iced::widget::text::Wrapping::None));
        }
    }
    let tip = if waiting.is_empty() {
        "Prover activity and log".to_string()
    } else {
        let mut w = waiting.clone();
        w.sort_by_key(|w| (!w.2, w.1));
        format!(
            "Waiting for: {}",
            w.iter().take(8).map(|(n, p, f)| if *f { format!("{n} (cannot load)") } else { format!("{n} {p}%") }).collect::<Vec<_>>().join(", ")
        )
    };
    let mut out = widget::row::with_capacity(4).spacing(s.space_xxs).align_y(Alignment::Center).push(chip(row.into(), tip, Some(Message::Panel(Panel::Log))));
    // A proof tool typed in the text, running: its own chip, its Stop inside it where it has one
    if let Some(tool) = running.iter().find(|r| is_tool(r)) {
        let name = tool["name"].as_str().unwrap_or("").to_string();
        let seconds = tool["seconds"].as_u64().unwrap_or(0);
        let mut trow = widget::row::with_capacity(3).spacing(s.space_xs).align_y(Alignment::Center).push(spinner(14.0));
        if ws.tools_stopping {
            trow = trow.push(widget::text::body(format!("{name}: stopping…")));
        } else {
            trow = trow.push(widget::text(name.clone()).size(13.0).font(cosmic::font::mono()));
            if !narrow {
                trow = trow.push(widget::text::body(format!("{seconds} s")));
            }
            if ws.caps.typed_tool(&name).is_some_and(|t| t.stop) {
                trow = trow.push(
                    widget::tooltip(
                        button::text("Stop").on_press(Message::StopTools),
                        widget::text(format!("Stop {name} (as if its time ran out)")),
                        widget::tooltip::Position::Bottom,
                    ),
                );
            }
        }
        out = out.push(
            widget::container(trow)
                .padding(Padding { left: 12.0, right: 4.0, top: 2.0, bottom: 2.0 })
                .height(Length::Fixed(30.0))
                .center_y(Length::Fixed(30.0))
                .class(theme::Container::custom(|theme| {
                    let c = theme.cosmic();
                    let text: Color = c.on_bg_color().into();
                    iced::widget::container::Style {
                        background: Some(blend(Color::from(c.bg_component_color()), Color::from(c.accent_color()), 0.12).into()),
                        border: Border { radius: 16.0.into(), width: 1.0, color: Color { a: 0.10, ..text } },
                        text_color: Some(text),
                        ..Default::default()
                    }
                })),
        );
    }
    // Interrupt: for proofs other than the tools, where the session's prover can
    let others = running.iter().any(|r| !is_tool(r));
    if busy && others && ws.caps.interrupt {
        out = out.push(icon_button("process-stop-symbolic", "Interrupt the running proofs (the prover keeps going with the rest)", Some(Message::Lifecycle("cancel"))));
    }
    Some(out.into())
}

// ----- a machine's connection, as every part of the window says it

/// Whether a machine's host is reached: connected, being connected (with what is being tried), or
/// not (with why)
#[derive(Clone, Debug, PartialEq)]
pub enum Conn {
    Connected,
    Connecting(String),
    Offline(String),
}

/// A link's connection. The app sets its sender and its status together (`connected`,
/// `disconnected: why`, or what it is trying); the status alone decides, so that a link read
/// from either says the same everywhere
pub fn connection(link: &super::Link) -> Conn {
    if link.sender.is_some() || link.status == "connected" {
        Conn::Connected
    } else if let Some(why) = link.status.strip_prefix("disconnected") {
        Conn::Offline(why.trim_start_matches(':').trim().to_string())
    } else {
        Conn::Connecting(link.status.clone())
    }
}

impl Conn {
    pub fn word(&self) -> &'static str {
        match self {
            Conn::Connected => "connected",
            Conn::Connecting(_) => "connecting…",
            Conn::Offline(_) => "not connected",
        }
    }

    /// What is tried, or why it failed, when there is something to say
    pub fn detail(&self) -> Option<&str> {
        match self {
            Conn::Connected => None,
            Conn::Connecting(what) | Conn::Offline(what) => Some(what.as_str()).filter(|w| !w.is_empty() && *w != "starting"),
        }
    }

    pub fn tint(&self) -> fn(&cosmic::Theme) -> Color {
        match self {
            Conn::Connected => success,
            Conn::Connecting(_) => warning,
            Conn::Offline(_) => destructive,
        }
    }

    /// A small dot in the connection's colour
    pub fn dot<'a>(&self, size: f32) -> Element<'a, Message> {
        let tint = self.tint();
        widget::container(widget::Space::new())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                background: Some(tint(theme).into()),
                border: Border { radius: (size / 2.0).into(), ..Default::default() },
                ..Default::default()
            }))
            .into()
    }
}

// ----- the status bar

/// The status bar's height: fixed, so that the page above never moves for it
pub const STATUS_BAR_HEIGHT: f32 = 28.0;

/// The bar at the window's foot, always there with a document open: vim's mode as a badge (with
/// vim keys on), what editing the open block means, then the app's last word and the connection
pub fn status_bar(app: &App) -> Option<Element<'_, Message>> {
    let theory = app.workspace.as_ref().and_then(|w| w.theory.as_ref()).filter(|_| !app.shell.settings_open);
    // (the open session's machine, when it is not connected; connected, it goes unsaid)
    let disconnected = app.workspace.as_ref().and_then(|w| app.link(&w.machine)).map(|l| (l, connection(l))).filter(|(_, c)| *c != Conn::Connected);
    if theory.is_none() && app.status.is_none() && disconnected.is_none() {
        return None;
    }
    let s = sp();
    let mut fields: Vec<Element<'_, Message>> = Vec::new();
    if app.vim_enabled && theory.is_some() {
        fields.push(vim_badge(app.vim.mode));
    }
    if let Some(t) = theory
        && let Some(be) = t.block_editor.as_ref().filter(|_| t.mode == Mode::Literate)
    {
        if be.wrap.is_none()
            && let Some(st) = t.status_in(be.start, be.end)
        {
            fields.push(block_status(st));
        }
        if be.preview.is_some() {
            fields.push(widget::text::caption("The preview follows your typing").into());
        }
        // (what the keys do for the tool at the caret, when there is one: Esc stops it, Alt+Enter
        // takes what it found)
        let hint = app.workspace.as_ref().and_then(super::flow::key_hint).unwrap_or_else(|| "Esc or a click outside finishes".into());
        fields.push(widget::text::caption(hint).into());
    } else if let Some(hint) = theory.and(app.workspace.as_ref()).and_then(super::flow::key_hint) {
        fields.push(widget::text::caption(hint).into());
    }
    let mut row = fields_row(fields).push(widget::Space::new().width(Length::Fill));
    if let Some(status) = &app.status {
        row = row.push(widget::text::caption(status.clone()).wrapping(iced::widget::text::Wrapping::None));
    }
    if let Some((l, conn)) = disconnected {
        if app.status.is_some() {
            row = row.push(divider(14.0));
        }
        let text = format!("{} {}", l.name, conn.word());
        let mut field = widget::row::with_capacity(2).spacing(6).align_y(Alignment::Center).push(conn.dot(8.0)).push(widget::text::caption(text).wrapping(iced::widget::text::Wrapping::None));
        if let Some(why) = conn.detail() {
            field = field.push(widget::text::caption(format!("({why})")).wrapping(iced::widget::text::Wrapping::None));
        }
        row = row.push(field);
    }
    Some(
        widget::container(row)
            .padding(Padding { left: s.space_s as f32, right: s.space_s as f32, top: 0.0, bottom: 0.0 })
            .width(Length::Fill)
            .height(Length::Fixed(STATUS_BAR_HEIGHT))
            .center_y(Length::Fixed(STATUS_BAR_HEIGHT))
            // (no background of its own: the window's page shows through, in either theme, so the
            // bar is set apart by its fixed place and height and the page's pixels stay the page's)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                text_color: Some(theme.cosmic().on_bg_color().into()),
                ..Default::default()
            }))
            .into(),
    )
}

/// Fields of a status bar, a thin line between each two (never before the first)
fn fields_row<'a>(fields: Vec<Element<'a, Message>>) -> iced::widget::Row<'a, Message, cosmic::Theme, cosmic::Renderer> {
    let mut row = widget::row::with_capacity(2 * fields.len() + 4).spacing(sp().space_s).align_y(Alignment::Center);
    for (i, f) in fields.into_iter().enumerate() {
        if i > 0 {
            row = row.push(divider(14.0));
        }
        row = row.push(f);
    }
    row
}

/// Vim's mode, as a badge
fn vim_badge<'a>(mode: super::vim::Mode) -> Element<'a, Message> {
    widget::container(widget::text::caption(mode.label()).font(cosmic::font::semibold()))
        .padding([1, 8])
        .class(theme::Container::custom(|theme| {
            let c = theme.cosmic();
            iced::widget::container::Style {
                background: Some(Color::from(c.accent_color()).into()),
                text_color: Some(c.on_accent_color().into()),
                border: Border { radius: 4.0.into(), ..Default::default() },
                ..Default::default()
            }
        }))
        .into()
}

/// The status bar of a window of its own (the proof window, a reference window): the same fields
/// as the main window's, under a line
pub fn window_status_bar<'a>(vim: Option<super::vim::Mode>, status: Option<Status>, hints: &[&str]) -> Element<'a, Message> {
    let mut fields: Vec<Element<'a, Message>> = Vec::new();
    fields.extend(vim.map(vim_badge));
    fields.extend(status.map(block_status));
    fields.extend(hints.iter().map(|h| widget::text::caption(h.to_string()).into()));
    let s = sp();
    widget::column::with_capacity(2)
        .push(widget::divider::horizontal::light())
        .push(
            widget::container(fields_row(fields))
                .padding(Padding { left: s.space_s as f32, right: s.space_s as f32, top: 0.0, bottom: 0.0 })
                .width(Length::Fill)
                .height(Length::Fixed(STATUS_BAR_HEIGHT))
                .center_y(Length::Fixed(STATUS_BAR_HEIGHT)),
        )
        .into()
}

/// The open block's checking, with an icon (a spinner while it is being checked)
fn block_status<'a>(status: Status) -> Element<'a, Message> {
    let (icon, text): (Element<'a, Message>, &str) = match status {
        Status::Failed => (tinted_icon("dialog-error-symbolic", 14, destructive), "Failing"),
        Status::Running => (spinner(12.0), "Checking"),
        Status::Unprocessed => (spinner(12.0), "Waiting to be checked"),
        Status::Warned => (tinted_icon("dialog-warning-symbolic", 14, warning), "Checked, with warnings"),
        Status::Finished => (tinted_icon("emblem-ok-symbolic", 14, success), "Checked"),
    };
    widget::row::with_capacity(2).spacing(6).align_y(Alignment::Center).push(icon).push(widget::text::caption(text)).into()
}
