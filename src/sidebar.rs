// SPDX-License-Identifier: GPL-3.0-only

//! The left sidebar (design: provers §5.2, §6.4): an icon tab bar, the section's name as a heading
//! under it, then the section.
//!
//! - **Sessions**, grouped by machine and then by folder, as a navigation list. Each row has its
//!   backend's mark, its name, its toolchain, its lifecycle and what fails, and the agents
//!   attached; the open session lists its documents, project files first. Independent sessions
//!   sit under the session they were forked from (C8).
//! - **Project**: the open session's project and its actions (build, fetch a cache), each with a
//!   Run button and the last output, when its capabilities offer actions.
//! - **Outline**, **Theorems** and **Problems** of the open document (drawn by `view`).

use std::collections::BTreeMap;

use cosmic::iced::{Alignment, Border, Color, Length, Padding};
use cosmic::prelude::*;
use cosmic::widget::{self, button};
use cosmic::{iced, theme};
use serde_json::Value;

use super::shell::{self, Msg, backend_mark, badge, blend, destructive, sp, spinner, success, tinted_icon, warning};
use super::theory::Status;
use super::{App, LeftMode, Message, Workspace, file_name};

/// The sections, in the tab bar's order: icon, name
fn tabs(app: &App) -> Vec<(LeftMode, &'static str, &'static str)> {
    let mut tabs = vec![(LeftMode::Sessions, "network-server-symbolic", "Sessions")];
    if app.workspace.as_ref().is_some_and(has_project) {
        tabs.push((LeftMode::Project, "package-x-generic-symbolic", "Project"));
    }
    tabs.extend([
        (LeftMode::Outline, "view-list-symbolic", "Outline"),
        (LeftMode::Theorems, "emblem-ok-symbolic", "Theorems"),
        (LeftMode::Problems, "dialog-warning-symbolic", "Problems"),
    ]);
    tabs
}

/// Whether the open session has a project to show: actions to run, or project files
pub fn has_project(ws: &Workspace) -> bool {
    !ws.caps.project_actions.is_empty()
}

/// The section shown: the one picked, else Sessions when the picked one has nothing to show
fn shown(app: &App) -> LeftMode {
    let theory = app.workspace.as_ref().and_then(|w| w.theory.as_ref());
    match app.left {
        LeftMode::Project if app.workspace.as_ref().is_some_and(has_project) => LeftMode::Project,
        LeftMode::Outline | LeftMode::Theorems | LeftMode::Problems if theory.is_some() => app.left,
        _ => LeftMode::Sessions,
    }
}

pub fn sidebar(app: &App) -> Element<'_, Message> {
    let s = sp();
    let theory = app.workspace.as_ref().and_then(|w| w.theory.as_ref());
    let current = shown(app);
    let tabs = tabs(app);
    // Equal hit areas across the sidebar's width, the current one filled
    let mut bar = widget::row::with_capacity(tabs.len()).spacing(2).align_y(Alignment::Center);
    for (mode, icon_name, label) in &tabs {
        let enabled = matches!(mode, LeftMode::Sessions | LeftMode::Project) || theory.is_some();
        // (a section with nothing to show yet is dimmed, as well as not pressable)
        let icon: Element<'_, Message> = if enabled { widget::icon::from_name(*icon_name).size(16).icon().into() } else { tinted_icon(icon_name, 16, shell::dimmed) };
        let b: Element<'_, Message> = if enabled {
            button::custom(widget::container(icon).center_x(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fixed(32.0))
                .padding([8, 0])
                .class(shell::segment_style(current == *mode))
                .on_press(Message::LeftMode(*mode))
                .into()
        } else {
            // (a disabled button draws its icon in the button's colour, whatever the icon's own)
            widget::container(icon).center_x(Length::Fill).center_y(Length::Fixed(32.0)).width(Length::Fill).height(Length::Fixed(32.0)).into()
        };
        let tip = if enabled { label.to_string() } else { format!("{label} (open a document first)") };
        bar = bar.push(widget::tooltip(b, widget::text(tip), widget::tooltip::Position::Bottom));
    }
    let bar = widget::container(bar).padding(3).class(theme::Container::custom(|theme| iced::widget::container::Style {
        background: Some(Color::from(theme.cosmic().bg_component_color()).into()),
        border: Border { radius: 18.0.into(), ..Default::default() },
        ..Default::default()
    }));
    let title = tabs.iter().find(|t| t.0 == current).map_or("", |t| t.2);
    let mut heading = widget::row::with_capacity(3).align_y(Alignment::Center).height(Length::Fixed(32.0)).push(widget::text::heading(title).width(Length::Fill));
    if current == LeftMode::Sessions {
        heading = heading.push(shell::icon_button("list-add-symbolic", "New session (pick a machine and a folder)", Some(Message::NewSession(true))));
    }
    let content: Element<'_, Message> = match (current, theory) {
        (LeftMode::Outline, Some(t)) => super::view::outline(app, t),
        (LeftMode::Theorems, Some(t)) => super::view::theorems(app, t),
        (LeftMode::Problems, Some(t)) => super::view::problems(app, t),
        (LeftMode::Project, _) => match &app.workspace {
            Some(ws) => project(app, ws),
            None => sessions(app),
        },
        _ => sessions(app),
    };
    widget::container(
        widget::column::with_capacity(3)
            .spacing(s.space_xxs)
            .padding(Padding { top: s.space_xs as f32, bottom: s.space_xs as f32, left: s.space_xs as f32, right: s.space_xs as f32 })
            .push(bar)
            .push(widget::container(heading).padding(Padding { left: 6.0, right: 0.0, top: 2.0, bottom: 0.0 }))
            .push(widget::scrollable(widget::container(content).padding(Padding { right: 14.0, ..Padding::ZERO })).height(Length::Fill)),
    )
    .width(Length::Fixed(app.left_width))
    .height(Length::Fill)
    .class(theme::Container::Card)
    .into()
}

// ----- sessions

/// A session's memory, when it is known: the open session's from its activity, others' if the
/// host reports it in their summary
fn memory_mb(app: &App, machine: &str, sid: &str, s: &Value) -> Option<u64> {
    if let Some(ws) = app.workspace.as_ref().filter(|w| w.machine == machine && w.sid == sid)
        && let Some(mb) = ws.activity.as_ref().and_then(|a| a["prover"]["rss_mb"].as_u64())
    {
        return Some(mb);
    }
    s["memory_mb"].as_u64().or_else(|| s["activity"]["rss_mb"].as_u64())
}

fn gigabytes(mb: u64) -> String {
    format!("{:.1} GB", mb as f64 / 1000.0)
}

/// A folder as a heading: under the machine's home as `~/…`, and no more than its last two parts
/// (the whole path in its tooltip)
fn short_folder(folder: &str, home: Option<&str>) -> String {
    let folder = folder.trim_end_matches('/');
    let (prefix, rest) = match home.map(|h| h.trim_end_matches('/')).filter(|h| !h.is_empty()).and_then(|h| folder.strip_prefix(h)) {
        Some(rest) if rest.starts_with('/') => ("~/", rest.trim_start_matches('/')),
        _ => ("", folder),
    };
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [.., parent, last] if parts.len() > 3 || (parts.len() == 3 && !prefix.is_empty()) => format!("…/{parent}/{last}"),
        _ if prefix.is_empty() => folder.to_string(),
        _ => format!("{prefix}{}", parts.join("/")),
    }
}

fn sessions(app: &App) -> Element<'_, Message> {
    let s = sp();
    let mut col = widget::column::with_capacity(32).spacing(2);
    for link in &app.links {
        let key = link.target.key.as_str();
        let on_machine: Vec<(&String, &Value)> = app.sessions.iter().filter(|((m, _), _)| m == key).map(|((_, id), v)| (id, v)).collect();
        // The machine: its name, whether it is connected, and its provers' memory against its budget
        let connected = link.sender.is_some();
        let total: u64 = on_machine.iter().filter_map(|(id, v)| memory_mb(app, key, id, v)).sum();
        let budget = link.hello.as_ref().and_then(|h| h["memory_budget_gb"].as_f64());
        // (its name, and under it whether it is connected and what its provers use: a line that
        // still fits a narrow sidebar)
        let memory = (total > 0).then(|| match budget {
            Some(b) => format!("{} of {b:.0} GB", gigabytes(total).trim_end_matches(" GB")),
            None => gigabytes(total),
        });
        let head = widget::row::with_capacity(2)
            .spacing(s.space_xs)
            .align_y(Alignment::Center)
            .push(tinted_icon("computer-symbolic", 16, |t| Color::from(t.cosmic().on_bg_color())))
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text::heading(link.name.clone()).wrapping(iced::widget::text::Wrapping::None))
                    .push(connection(link, memory))
                    .width(Length::Fill)
                    .clip(true),
            );
        col = col.push(widget::container(head).padding(Padding { top: 8.0, bottom: 2.0, left: 6.0, right: 4.0 }));
        if on_machine.is_empty() {
            col = col.push(
                widget::container(widget::text::caption(if connected { "No sessions" } else { "Sessions show once it is connected" }))
                    .padding(Padding { left: 30.0, top: 0.0, bottom: 4.0, right: 0.0 }),
            );
            continue;
        }
        // By folder; a fork under the session it came from
        let ids: Vec<&str> = on_machine.iter().map(|(id, _)| id.as_str()).collect();
        let parent_of = |v: &Value| v["parent"]["session"].as_str().filter(|p| ids.contains(p)).map(str::to_string);
        let mut folders: BTreeMap<String, Vec<(&String, &Value)>> = BTreeMap::new();
        for (id, v) in &on_machine {
            if parent_of(v).is_none() {
                folders.entry(v["folder"].as_str().unwrap_or("").to_string()).or_default().push((id, v));
            }
        }
        for (folder, list) in folders {
            col = col.push(widget::tooltip(
                widget::container(
                    widget::row::with_capacity(2)
                        .spacing(6)
                        .align_y(Alignment::Center)
                        .push(tinted_icon("folder-symbolic", 14, shell::muted))
                        .push(widget::text::caption(short_folder(&folder, link.hello.as_ref().and_then(|h| h["home"].as_str()))).wrapping(iced::widget::text::Wrapping::None)),
                )
                .padding(Padding { top: 4.0, bottom: 2.0, left: 10.0, right: 4.0 }),
                widget::text(folder.clone()),
                widget::tooltip::Position::Bottom,
            ));
            for (id, v) in list {
                col = col.push(session_row(app, key, id, v, 0.0));
                if let Some(ws) = app.workspace.as_ref().filter(|w| w.machine == key && w.sid == *id) {
                    col = col.push(documents(app, ws));
                }
                for (fid, fv) in on_machine.iter().filter(|(_, fv)| parent_of(fv).as_deref() == Some(id.as_str())) {
                    col = col.push(session_row(app, key, fid, fv, 16.0));
                    if let Some(ws) = app.workspace.as_ref().filter(|w| w.machine == key && w.sid == **fid) {
                        col = col.push(documents(app, ws));
                    }
                }
            }
        }
    }
    col.into()
}

/// A machine's connection: a dot and a word, then what its provers use
fn connection<'a>(link: &super::Link, memory: Option<String>) -> Element<'a, Message> {
    let (word, tint): (String, fn(&cosmic::Theme) -> Color) = if link.sender.is_some() {
        ("connected".into(), success)
    } else if link.status.starts_with("disconnected") {
        ("offline".into(), destructive)
    } else {
        ("connecting…".into(), warning)
    };
    let tip = if link.status.is_empty() { word.clone() } else { link.status.clone() };
    widget::tooltip(
        widget::row::with_capacity(2)
            .spacing(4)
            .align_y(Alignment::Center)
            .push(
                widget::container(widget::Space::new())
                    .width(Length::Fixed(7.0))
                    .height(Length::Fixed(7.0))
                    .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                        background: Some(tint(theme).into()),
                        border: Border { radius: 4.0.into(), ..Default::default() },
                        ..Default::default()
                    })),
            )
            .push(widget::text::caption(match memory {
                Some(m) => format!("{word}  ·  {m}"),
                None => word,
            }).wrapping(iced::widget::text::Wrapping::None)),
        widget::text(tip),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

/// One session: its backend's mark, name and toolchain, what it is doing and failing, its agents
/// and memory
fn session_row<'a>(app: &'a App, machine: &str, sid: &str, v: &'a Value, indent: f32) -> Element<'a, Message> {
    let current = app.workspace.as_ref().is_some_and(|w| w.machine == machine && w.sid == sid);
    let backend = v["backend"].as_str().unwrap_or("isabelle");
    let name = v["name"].as_str().unwrap_or(sid).to_string();
    // (whose independent session it is, unless its name says so)
    let owner = v["owner"]["label"].as_str().filter(|l| !name.contains(l)).map(|l| format!(" ({l})")).unwrap_or_default();
    let agents = v["clients"].as_array().map_or(0, |c| c.iter().filter(|c| c["kind"] == "agent").count());
    let lifecycle = v["lifecycle"].as_str().unwrap_or("");
    let checking = v["activity"]["running"].as_u64().unwrap_or(0) > 0;
    let failing = v["activity"]["failed"].as_u64().unwrap_or(0);
    // The line under the name: toolchain, agents, memory
    let mut detail = Vec::new();
    match v["toolchain"]["label"].as_str().filter(|l| !l.is_empty()) {
        Some(tc) => detail.push(tc.to_string()),
        None => {
            if let Some(logic) = v["logic"].as_str().filter(|l| !l.is_empty()) {
                detail.push(logic.to_string());
            }
        }
    }
    if agents > 0 {
        detail.push(format!("{agents} agent{}", if agents == 1 { "" } else { "s" }));
    }
    if let Some(mb) = memory_mb(app, machine, sid, v) {
        detail.push(gigabytes(mb));
    }
    let text = widget::column::with_capacity(2)
        .push(
            widget::text(format!("{name}{owner}"))
                .size(14.0)
                .font(if current { cosmic::font::semibold() } else { cosmic::font::default() })
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .push(widget::text::caption(detail.join("  ·  ")).wrapping(iced::widget::text::Wrapping::Word))
        .width(Length::Fill)
        .clip(true);
    // Its lifecycle: an icon (a spinner while it checks), a word unless it is simply running, and
    // what fails
    let mut state = widget::row::with_capacity(3).spacing(4).align_y(Alignment::Center);
    let mut tip = vec![lifecycle.to_string()];
    match lifecycle {
        "Running" if checking => {
            state = state.push(spinner(12.0));
            tip.push("checking".into());
        }
        "Running" => state = state.push(tinted_icon("media-playback-start-symbolic", 14, success)),
        "Paused" => state = state.push(tinted_icon("media-playback-pause-symbolic", 14, warning)),
        "Stopped" => state = state.push(tinted_icon("media-playback-stop-symbolic", 14, shell::muted)),
        "Failed" => state = state.push(tinted_icon("dialog-error-symbolic", 14, destructive)),
        other => state = state.push(widget::text::caption(other.to_string())),
    }
    // (the word too, where the sidebar has room for it beside the name)
    if matches!(lifecycle, "Paused" | "Stopped" | "Failed") && app.left_width >= 270.0 {
        state = state.push(widget::text::caption(lifecycle.to_string()));
    }
    if failing > 0 {
        state = state.push(badge(failing.to_string(), destructive));
        tip.push(format!("{failing} document{} failing", if failing == 1 { "" } else { "s" }));
    }
    let row = widget::row::with_capacity(3)
        .spacing(8)
        .align_y(Alignment::Center)
        .push(backend_mark(backend))
        .push(text)
        .push(widget::tooltip(state, widget::text(tip.join(" · ")), widget::tooltip::Position::Bottom));
    widget::container(
        button::custom(row)
            .width(Length::Fill)
            .padding([5, 8])
            .class(nav_style(current))
            .on_press(Message::OpenSession(machine.to_string(), sid.to_string())),
    )
    .padding(Padding { left: indent, ..Padding::ZERO })
    .into()
}

/// A row of a navigation list: the current one tinted with the accent
pub fn nav_style(current: bool) -> theme::Button {
    fn style(theme: &cosmic::Theme, current: bool, hover: bool) -> button::Style {
        let c = theme.cosmic();
        let text: Color = c.on_bg_color().into();
        let accent: Color = c.accent_color().into();
        let mut s = button::Style::new();
        s.border_radius = 8.0.into();
        s.background = match (current, hover) {
            (true, _) => Some(Color { a: if hover { 0.22 } else { 0.16 }, ..accent }.into()),
            (false, true) => Some(Color { a: 0.07, ..text }.into()),
            (false, false) => None,
        };
        s.text_color = Some(text);
        s.icon_color = Some(text);
        s
    }
    theme::Button::Custom {
        active: Box::new(move |_, t| style(t, current, false)),
        disabled: Box::new(move |t| style(t, current, false)),
        hovered: Box::new(move |_, t| style(t, current, true)),
        pressed: Box::new(move |_, t| style(t, current, true)),
    }
}

/// The extensions of the session's documents, and the names of its project files: as its host
/// says, else as its profile knows them
pub fn document_kinds(app: &App, ws: &Workspace) -> (Vec<String>, Vec<String>) {
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let from_summary: Vec<String> = summary.and_then(|s| s["documents"].as_array()).into_iter().flatten().filter_map(|d| d.as_str().map(str::to_string)).collect();
    let documents = if !from_summary.is_empty() {
        from_summary
    } else if !ws.caps.documents.is_empty() {
        ws.caps.documents.clone()
    } else {
        ws.profile.documents().iter().map(|d| d.to_string()).collect()
    };
    (documents, ws.caps.project_files.clone())
}

/// Whether a path is a project file of the session (a ROOT, a lakefile): opened in the source view
pub fn is_project_file(app: &App, ws: &Workspace, path: &str) -> bool {
    let (documents, project_files) = document_kinds(app, ws);
    let name = file_name(path);
    !documents.iter().any(|d| name.ends_with(d.as_str())) && project_files.iter().any(|p| p == name)
}

/// The open session's documents: its project files first, then the documents by folder
fn documents<'a>(app: &'a App, ws: &'a Workspace) -> Element<'a, Message> {
    let mut col = widget::column::with_capacity(64).spacing(1).padding(Padding { left: 26.0, right: 0.0, top: 2.0, bottom: 6.0 });
    let Some(info) = &ws.info else { return widget::container(widget::text::caption("Loading…")).padding(Padding { left: 30.0, ..Padding::ZERO }).into() };
    let folder = info["folder"].as_str().unwrap_or("");
    let (documents, project_files) = document_kinds(app, ws);
    let entries: Vec<&Value> = info["theories"].as_array().into_iter().flatten().collect();
    let path_of = |t: &&Value| t["path"].as_str().unwrap_or("").to_string();
    let is_doc = |p: &str| documents.iter().any(|d| p.ends_with(d.as_str()));
    let projects: Vec<String> = entries.iter().map(path_of).filter(|p| !is_doc(p) && project_files.iter().any(|f| f == file_name(p))).collect();
    let all: Vec<&Value> = entries.iter().filter(|t| is_doc(&path_of(t))).copied().collect();
    // A big folder gets a filter
    if all.len() > 10 {
        col = col.push(
            widget::container(widget::text_input(format!("Filter {} documents", all.len()), &ws.theory_filter).on_input(Message::TheoryFilter))
                .padding(Padding { bottom: 4.0, right: 4.0, ..Padding::ZERO }),
        );
    }
    let open = ws.theory.as_ref().map(|t| t.path.as_str());
    for p in &projects {
        let rel = p.strip_prefix(folder).unwrap_or(p).trim_start_matches('/');
        let row = widget::row::with_capacity(2)
            .spacing(6)
            .align_y(Alignment::Center)
            .push(tinted_icon("text-x-generic-symbolic", 14, shell::muted))
            .push(widget::text(rel.to_string()).size(13.0).wrapping(iced::widget::text::Wrapping::None));
        col = col.push(widget::tooltip(
            button::custom(row).width(Length::Fill).padding([2, 6]).class(nav_style(open == Some(p.as_str()))).on_press(Message::OpenTheory(p.clone())),
            widget::text("A project file: opens in the source view"),
            widget::tooltip::Position::Bottom,
        ));
    }
    let filter = ws.theory_filter.to_lowercase();
    // By folder, then the name less what all names in the folder start with
    let mut groups: BTreeMap<String, Vec<(String, &Value)>> = BTreeMap::new();
    for t in &all {
        let path = t["path"].as_str().unwrap_or("");
        let rel = path.strip_prefix(folder).unwrap_or(path).trim_start_matches('/');
        if !filter.is_empty() && super::input::fuzzy(&filter, &rel.to_lowercase()).is_none() {
            continue;
        }
        let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        let shown = documents.iter().find_map(|d| name.strip_suffix(d.as_str())).unwrap_or(name);
        groups.entry(dir.to_string()).or_default().push((shown.to_string(), *t));
    }
    if groups.is_empty() {
        col = col.push(widget::container(widget::text::caption(if all.is_empty() { "No documents here yet" } else { "No document matches" })).padding([2, 6]));
    }
    for (dir, items) in groups {
        let indent = if dir.is_empty() { 0.0 } else { 10.0 };
        if !dir.is_empty() {
            col = col.push(widget::container(widget::text::caption(dir)).padding(Padding { top: 4.0, left: 6.0, ..Padding::ZERO }));
        }
        for (name, t) in items {
            let path = t["path"].as_str().unwrap_or("").to_string();
            let st = &t["status"];
            let status = if t["conflict"].as_bool() == Some(true) || st["failed"].as_u64().unwrap_or(0) > 0 {
                Some(Status::Failed)
            } else if st["consolidated"].as_bool() == Some(true) {
                Some(Status::Finished)
            } else if st["running"].as_u64().unwrap_or(0) > 0 {
                Some(Status::Running)
            } else if st.is_object() {
                Some(Status::Unprocessed)
            } else {
                None
            };
            let dirty = if t["dirty"].as_bool() == Some(true) { " •" } else { "" };
            let row = widget::row::with_capacity(2)
                .spacing(6)
                .align_y(Alignment::Center)
                .push(
                    widget::container(widget::Space::new())
                        .width(Length::Fixed(7.0))
                        .height(Length::Fixed(7.0))
                        .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                            background: status.map(|s| super::view::status_color(s, theme).into()),
                            border: Border { radius: 4.0.into(), ..Default::default() },
                            ..Default::default()
                        })),
                )
                .push(widget::text(format!("{name}{dirty}")).size(13.0).wrapping(iced::widget::text::Wrapping::None));
            col = col.push(widget::tooltip(
                button::custom(row)
                    .width(Length::Fill)
                    .padding(Padding { left: 6.0 + indent, right: 6.0, top: 2.0, bottom: 2.0 })
                    .class(nav_style(open == Some(path.as_str())))
                    .on_press(Message::OpenTheory(path.clone())),
                widget::text::caption(file_name(&path).to_string()),
                widget::tooltip::Position::Right,
            ));
        }
    }
    col.into()
}

// ----- the project

/// The open session's project: what it is, its files, and its actions with Run and their output
fn project<'a>(app: &'a App, ws: &'a Workspace) -> Element<'a, Message> {
    let s = sp();
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let backend = summary.and_then(|v| v["backend"].as_str()).unwrap_or("isabelle");
    let mut col = widget::column::with_capacity(16).spacing(s.space_xs).padding(Padding { left: 6.0, right: 2.0, top: 2.0, bottom: 8.0 });
    // What the project is
    let mut facts: Vec<(&str, String)> = Vec::new();
    if let Some(tc) = summary.and_then(|v| v["toolchain"]["label"].as_str()).filter(|l| !l.is_empty()) {
        facts.push(("Toolchain", tc.to_string()));
    }
    let project = summary.map(|v| &v["project"]).filter(|p| p.is_object());
    if let Some(p) = project {
        if let Some(root) = p["lake_root"].as_str() {
            facts.push(("Lake root", root.to_string()));
        }
        if let Some(logic) = p["logic"].as_str().filter(|l| !l.is_empty()) {
            facts.push(("Base logic", logic.to_string()));
        }
        let packages: Vec<&str> = p["packages"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if !packages.is_empty() {
            facts.push(("Packages", packages.join(", ")));
        }
        let dirs: Vec<&str> = p["dirs"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if !dirs.is_empty() {
            facts.push(("Session dirs", dirs.join(", ")));
        }
    }
    col = col.push(
        widget::row::with_capacity(2)
            .spacing(8)
            .align_y(Alignment::Center)
            .push(backend_mark(backend))
            .push(widget::text::body(format!("{} project", shell::backend_label(backend))).font(cosmic::font::semibold())),
    );
    for (label, value) in facts {
        col = col.push(
            widget::column::with_capacity(2)
                .push(widget::text::caption(label).font(cosmic::font::semibold()))
                .push(widget::text::body(value).wrapping(iced::widget::text::Wrapping::WordOrGlyph)),
        );
    }
    // Its files (ROOT, the lakefile), which open in the source view
    let files: Vec<String> = ws
        .info
        .as_ref()
        .and_then(|i| i["theories"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|t| t["path"].as_str())
        .filter(|p| is_project_file(app, ws, p))
        .map(str::to_string)
        .collect();
    if !files.is_empty() {
        col = col.push(widget::container(widget::text::heading("Files")).padding(Padding { top: 8.0, ..Padding::ZERO }));
        let open = ws.theory.as_ref().map(|t| t.path.as_str());
        for p in files {
            let row = widget::row::with_capacity(2)
                .spacing(6)
                .align_y(Alignment::Center)
                .push(tinted_icon("text-x-generic-symbolic", 14, shell::muted))
                .push(widget::text(file_name(&p).to_string()).size(13.0).wrapping(iced::widget::text::Wrapping::None));
            col = col.push(button::custom(row).width(Length::Fill).padding([3, 6]).class(nav_style(open == Some(p.as_str()))).on_press(Message::OpenTheory(p.clone())));
        }
    }
    // Its actions
    col = col.push(widget::container(widget::text::heading("Actions")).padding(Padding { top: 8.0, ..Padding::ZERO }));
    for action in &ws.caps.project_actions {
        let run = app.shell.runs.get(&(ws.sid.clone(), action.name.clone()));
        let running = run.is_some_and(|r| r.running);
        let mut row = widget::row::with_capacity(3).spacing(s.space_xs).align_y(Alignment::Center).push(widget::text::body(action.label.clone()).width(Length::Fill));
        if running {
            row = row.push(spinner(14.0)).push(widget::text::caption("Running…"));
        } else {
            row = row.push(widget::button::standard("Run").on_press(Message::Shell(Msg::ProjectAction(action.name.clone()))));
        }
        let mut card = widget::column::with_capacity(3).spacing(s.space_xxs).push(row);
        if let Some(r) = run.filter(|r| !r.running) {
            let (icon, word): (Element<'a, Message>, &str) = match r.ok {
                Some(true) => (tinted_icon("emblem-ok-symbolic", 14, success), "The last run finished"),
                _ => (tinted_icon("dialog-error-symbolic", 14, destructive), "The last run failed"),
            };
            card = card.push(widget::row::with_capacity(2).spacing(6).align_y(Alignment::Center).push(icon).push(widget::text::caption(word)));
            if !r.output.trim().is_empty() {
                let tail: Vec<&str> = r.output.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect();
                card = card.push(
                    widget::container(widget::scrollable(widget::text(tail.join("\n")).font(cosmic::font::mono()).size(11.0).wrapping(iced::widget::text::Wrapping::WordOrGlyph)))
                        .max_height(180.0)
                        .padding([4, 6])
                        .width(Length::Fill)
                        .class(theme::Container::custom(|t| iced::widget::container::Style {
                            background: Some(blend(Color::from(t.cosmic().bg_color()), Color::from(t.cosmic().on_bg_color()), 0.05).into()),
                            border: Border { radius: 6.0.into(), ..Default::default() },
                            ..Default::default()
                        })),
                );
            }
        }
        col = col.push(
            widget::container(card)
                .padding([6, 8])
                .width(Length::Fill)
                .class(theme::Container::custom(|t| {
                    let text: Color = t.cosmic().on_bg_color().into();
                    iced::widget::container::Style {
                        background: Some(blend(Color::from(t.cosmic().bg_component_color()), text, 0.05).into()),
                        border: Border { radius: 8.0.into(), width: 1.0, color: Color { a: 0.08, ..text } },
                        ..Default::default()
                    }
                })),
        );
    }
    if ws.caps.project_actions.is_empty() {
        col = col.push(widget::text::caption("This session's backend offers no project actions."));
    }
    col.into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn folders_read_short() {
        use super::short_folder;
        assert_eq!(short_folder("/home/me/Demo", None), "/home/me/Demo");
        assert_eq!(short_folder("/home/me/Demo", Some("/home/me")), "~/Demo");
        assert_eq!(short_folder("/home/me/Research/Demo/", Some("/home/me/")), "~/Research/Demo");
        assert_eq!(short_folder("/home/me/Research/Repositories/DSRVLean", Some("/home/me")), "…/Repositories/DSRVLean");
        assert_eq!(short_folder("/srv/a/b/c", None), "…/b/c");
    }
}
