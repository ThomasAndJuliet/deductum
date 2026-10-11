// SPDX-License-Identifier: GPL-3.0-only

//! Isabellum's window: sessions on the left, the theory in the middle in its
//! literate or source view (design C14), proof output, Sledgehammer, agents
//! and history on the right.

use std::sync::{Arc, LazyLock};

use cosmic::iced::{Alignment, Border, Color, Length, Padding};
use cosmic::prelude::*;
use cosmic::widget::{self, button, text_editor};
use cosmic::{iced, theme};
use serde_json::Value;

use super::editor::{Mode, Theory, highlight_format};
use super::theory::{Block, Kind, Status};
use super::thread::rich::{Typography, rich_text};
use super::{App, LeftMode, Message, Panel, Workspace, file_name, link};

static BLOCK_EDITOR: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);
static SOURCE_EDITOR: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);
static LITERATE_SCROLL: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);

pub fn source_editor_id() -> widget::Id {
    SOURCE_EDITOR.clone()
}

pub fn literate_scroll_id() -> widget::Id {
    LITERATE_SCROLL.clone()
}

pub fn block_editor_id() -> widget::Id {
    BLOCK_EDITOR.clone()
}

fn icon(name: &'static str) -> widget::icon::Icon {
    widget::icon::from_name(name).size(16).icon()
}

fn icon_button(name: &'static str, tip: &'static str, message: Option<Message>) -> Element<'static, Message> {
    let b = button::icon(widget::icon::from_name(name)).on_press_maybe(message);
    widget::tooltip(b, widget::text(tip), widget::tooltip::Position::Bottom).into()
}

fn spacing() -> cosmic::cosmic_theme::Spacing {
    theme::active().cosmic().spacing
}

fn status_color(status: Status, theme: &cosmic::Theme) -> Color {
    let c = theme.cosmic();
    match status {
        Status::Finished => Color { a: 0.35, ..Color::from(c.success_color()) },
        Status::Warned => c.warning_color().into(),
        Status::Unprocessed => Color { a: 0.3, ..Color::from(c.on_bg_color()) },
        Status::Running => c.accent_color().into(),
        Status::Failed => c.destructive_color().into(),
    }
}

pub fn typography(theme: &cosmic::Theme) -> Typography {
    let cosmic = theme.cosmic();
    let text: Color = cosmic.on_bg_color().into();
    let dark = cosmic.is_dark;
    Typography {
        body: "NewComputerModern".into(),
        mono: String::new(),
        size: 17.0,
        line_height: 1.5,
        math_scale: 1.1,
        justify: false,
        italic: false,
        text,
        muted: Color { a: 0.68, ..text },
        link: cosmic.accent_text_color().into(),
        code: text,
        code_background: cosmic.bg_component_color().into(),
        page: cosmic.bg_color().into(),
        rule: Color { a: 0.25, ..text },
        selection: Color { a: 0.32, ..Color::from(cosmic.accent.base) },
        error: cosmic.destructive_text_color().into(),
        added: if dark { Color::from_rgb8(0x8f, 0xd6, 0x8f) } else { Color::from_rgb8(0x1d, 0x7a, 0x2a) },
        removed: if dark { Color::from_rgb8(0xf0, 0x8a, 0x8a) } else { Color::from_rgb8(0xb0, 0x22, 0x22) },
        image_width: 520.0,
    }
}

// ----- header

pub fn header_start(app: &App) -> Vec<Element<'_, Message>> {
    let mut items: Vec<Element<'_, Message>> =
        vec![icon_button("sidebar-show-symbolic", "Show or hide the navigation", Some(Message::ToggleSidebar))];
    if let Some(t) = app.workspace.as_ref().and_then(|w| w.theory.as_ref()) {
        // A fixed width: the control would otherwise take the header's whole free width
        items.push(
            widget::container(widget::segmented_control::horizontal(&app.mode_model).on_activate(Message::SetModeEntity))
                .width(Length::Fixed(230.0))
                .into(),
        );
        items.push(icon_button("edit-undo-symbolic", "Undo your last edit (Ctrl+Z)", t.can_undo.then_some(Message::Undo)));
        items.push(icon_button("edit-redo-symbolic", "Redo (Ctrl+Shift+Z)", t.can_redo.then_some(Message::Redo)));
        if !app.back.is_empty() {
            items.push(icon_button("go-previous-symbolic", "Back to where Ctrl+click came from (Alt+Left)", Some(Message::Back)));
        }
    }
    items
}

pub fn header_end(app: &App) -> Vec<Element<'_, Message>> {
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    let Some(ws) = &app.workspace else {
        items.push(widget::button::suggested("New session").on_press(Message::NewSession(true)).into());
        return items;
    };
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
            items.push(icon_button("user-trash-symbolic", "Forget this session (theories are saved; an agent's unmerged work is refused)", Some(Message::Lifecycle("discard"))));
        }
        _ => {}
    }
    if ws.info.is_some() {
        items.push(icon_button("folder-open-symbolic", "Open the session's folder in Files", Some(Message::OpenFolder(link::Opener::Files))));
        items.push(icon_button("text-editor-symbolic", "Open the session's folder in Zed", Some(Message::OpenFolder(link::Opener::Zed))));
        items.push(icon_button("utilities-terminal-symbolic", "Open a terminal in the session's folder", Some(Message::OpenFolder(link::Opener::Terminal))));
    }
    items.push(
        widget::tooltip(
            widget::button::text("Vim")
                .class(if app.vim_enabled { theme::Button::Suggested } else { theme::Button::Text })
                .on_press(Message::ToggleVim),
            widget::text("Vim keys in the editors"),
            widget::tooltip::Position::Bottom,
        )
        .into(),
    );
    // The goal beside the text when there is room (off: under the construct, the page centred)
    items.push(
        widget::tooltip(
            widget::button::icon(widget::icon::from_name(if goal_beside_on() { "view-dual-symbolic" } else { "view-paged-symbolic" }))
                .class(if goal_beside_on() { theme::Button::Suggested } else { theme::Button::Icon })
                .on_press(Message::ToggleGoalBeside),
            widget::text(if goal_beside_on() {
                "The goal beside the text when there is room: click to keep it under the proof, the page centred"
            } else {
                "The goal under the proof, the page centred: click to put it beside the text when there is room"
            }),
            widget::tooltip::Position::Bottom,
        )
        .into(),
    );
    items.push(icon_button("sidebar-show-right-symbolic", "Show or hide proof output and tools", Some(Message::ToggleRight)));
    items
}

/// Whether the goal goes beside the text when there is room (a toolbar toggle)
static GOAL_BESIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn goal_beside_on() -> bool {
    GOAL_BESIDE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_goal_beside(on: bool) {
    GOAL_BESIDE.store(on, std::sync::atomic::Ordering::Relaxed);
}

// ----- the window

pub fn view(app: &App) -> Element<'_, Message> {
    // The editors' keys follow vim's mode, whoever draws the view
    set_vim_mode(app.vim_enabled.then_some(app.vim.mode));
    set_placement_override(app.goal_placement_override);
    widget::responsive(move |size| {
        set_view_height(size.height);
        layout(app, size.width)
    })
    .into()
}

/// Whether the right sidebar shows at this window width
pub fn right_shown(app: &App, width: f32) -> bool {
    app.show_right && app.workspace.is_some() && app.new_session.is_none() && width >= 960.0
}

fn layout(app: &App, width: f32) -> Element<'_, Message> {
    let sp = spacing();
    // Narrow windows give the theory the room: the sidebars fold away
    let left = app.show_sidebar && width >= 640.0;
    let right = right_shown(app, width);
    let mut row = widget::row::with_capacity(5).spacing(0).height(Length::Fill);
    if left {
        row = row.push(sidebar(app)).push(splitter(true));
    }
    let main: Element<'_, Message> = if let Some(ns) = &app.new_session {
        new_session(app, ns)
    } else if let Some(ws) = &app.workspace {
        let main_width = width - if left { app.left_width + 6.0 } else { 0.0 } - if right { app.right_width + 6.0 } else { 0.0 };
        workspace(app, ws, right, main_width)
    } else {
        welcome(app)
    };
    row = row.push(widget::container(main).width(Length::Fill).height(Length::Fill));
    if right && let Some(ws) = &app.workspace {
        row = row.push(splitter(false)).push(side_panel(app, ws));
    }
    // While a sidebar's edge is dragged, the pointer shows it (the drag itself follows
    // window events, so a release anywhere ends it)
    let row: Element<'_, Message> = if app.dragging.is_some() {
        widget::mouse_area(row).interaction(cosmic::iced::mouse::Interaction::ResizingHorizontally).into()
    } else {
        row.into()
    };
    let mut col = widget::column::with_capacity(2).push(row);
    // The status bar, always there with a theory open (so the page's height never changes for
    // it): what editing the open block means, and the app's last word
    let editing = editing_status(app);
    if editing.is_some() || app.status.is_some() || app.workspace.as_ref().is_some_and(|w| w.theory.is_some()) {
        let bar = widget::row::with_capacity(2)
            .spacing(sp.space_m)
            .push(widget::text::caption(editing.unwrap_or_default()).width(Length::Fill))
            .push(widget::text::caption(app.status.clone().unwrap_or_default()));
        col = col.push(widget::container(bar).padding([sp.space_xxs, sp.space_s]).width(Length::Fill));
    }
    // An opaque background of the theme's colour (with no inset content card, the window itself
    // would show through to the desktop)
    widget::container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color { a: 1.0, ..Color::from(theme.cosmic().bg_color()) }.into()),
            text_color: Some(theme.cosmic().on_bg_color().into()),
            ..Default::default()
        }))
        .into()
}

/// A sidebar's draggable edge
fn splitter<'a>(left: bool) -> Element<'a, Message> {
    widget::mouse_area(
        widget::container(widget::Space::new())
            .width(Length::Fixed(6.0))
            .height(Length::Fill)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(Color { a: 0.12, ..Color::from(theme.cosmic().on_bg_color()) }.into()),
                ..Default::default()
            })),
    )
    .on_press(Message::DragStart(left))
    .interaction(cosmic::iced::mouse::Interaction::ResizingHorizontally)
    .into()
}

fn welcome(app: &App) -> Element<'_, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(6)
        .spacing(sp.space_s)
        .push(widget::text::title1("Isabellum"))
        .push(widget::text::body("Isabelle theories as literate documents, worked on together with agents in one live session."))
        .push(widget::button::suggested("Open a theory folder…").on_press(Message::NewSession(true)));
    let any = !app.sessions.is_empty();
    if any {
        col = col.push(widget::text::body("Or pick a session on the left."));
    }
    widget::container(col.max_width(640)).center(Length::Fill).into()
}

fn lifecycle_badge(s: &Value) -> String {
    let lc = s["lifecycle"].as_str().unwrap_or("?");
    let act = &s["activity"];
    let running = act["running"].as_u64().unwrap_or(0);
    let failed = act["failed"].as_u64().unwrap_or(0);
    let mut out = lc.to_string();
    if lc == "Running" && running > 0 {
        out.push_str(" · checking");
    }
    if failed > 0 {
        out.push_str(&format!(" · {failed} failing"));
    }
    out
}

fn sidebar(app: &App) -> Element<'_, Message> {
    let sp = spacing();
    let theory = app.workspace.as_ref().and_then(|w| w.theory.as_ref());
    let tabs = [
        (LeftMode::Sessions, "network-server-symbolic", "Sessions"),
        (LeftMode::Outline, "view-list-bullet-symbolic", "Outline"),
        (LeftMode::Theorems, "emblem-ok-symbolic", "Theorems"),
        (LeftMode::Problems, "dialog-warning-symbolic", "Problems"),
    ];
    let mut bar = widget::row::with_capacity(tabs.len() + 1).spacing(2).align_y(Alignment::Center);
    for (mode, icon_name, label) in tabs {
        let enabled = mode == LeftMode::Sessions || theory.is_some();
        let b = widget::button::icon(widget::icon::from_name(icon_name))
            .class(if app.left == mode { theme::Button::Suggested } else { theme::Button::Icon })
            .on_press_maybe(enabled.then_some(Message::LeftMode(mode)));
        bar = bar.push(widget::tooltip(b, widget::text(label), widget::tooltip::Position::Bottom));
    }
    let title = tabs.iter().find(|t| t.0 == app.left).map_or("", |t| t.2);
    bar = bar.push(widget::text::heading(title).width(Length::Fill)).push(widget::Space::new().width(Length::Fixed(4.0)));
    let content: Element<'_, Message> = match (app.left, theory) {
        (LeftMode::Outline, Some(t)) => outline(app, t),
        (LeftMode::Theorems, Some(t)) => theorems(app, t),
        (LeftMode::Problems, Some(t)) => problems(app, t),
        _ => sessions_list(app),
    };
    widget::container(
        widget::column::with_capacity(2)
            .spacing(sp.space_xs)
            .padding([sp.space_xs, sp.space_xs])
            .push(bar)
            .push(widget::scrollable(content).height(Length::Fill)),
    )
    .width(Length::Fixed(app.left_width))
    .height(Length::Fill)
    .class(theme::Container::Card)
    .into()
}

fn nav_item<'a>(label: String, detail: Option<String>, indent: f32, color: Option<Status>, message: Message) -> Element<'a, Message> {
    let mut row = widget::row::with_capacity(3).spacing(6).align_y(Alignment::Center);
    if let Some(status) = color {
        row = row.push(
            widget::container(widget::Space::new())
                .width(Length::Fixed(8.0))
                .height(Length::Fixed(8.0))
                .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                    background: Some(status_color(status, theme).into()),
                    border: Border { radius: 4.0.into(), ..Default::default() },
                    ..Default::default()
                })),
        );
    }
    let mut col = widget::column::with_capacity(2).push(widget::text::body(label));
    if let Some(d) = detail {
        col = col.push(widget::text::caption(d));
    }
    row = row.push(col);
    widget::button::custom(row)
        .width(Length::Fill)
        .padding(Padding { left: 6.0 + indent, right: 6.0, top: 3.0, bottom: 3.0 })
        .class(theme::Button::MenuItem)
        .on_press(message)
        .into()
}

fn block_title(t: &Theory, table: &super::symbols::Table, block: &Block, max: usize) -> String {
    let text = match block.body {
        Some((s, e)) => t.source_text(s, e),
        None => t.source_text(block.start, block.end),
    };
    let first = table.decode(&text).text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    if first.chars().count() > max { format!("{}…", first.chars().take(max).collect::<String>()) } else { first }
}

fn outline<'a>(app: &'a App, t: &'a Theory) -> Element<'a, Message> {
    let table = app.table(&app.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default());
    let mut col = widget::column::with_capacity(t.blocks.len()).spacing(1);
    for block in &t.blocks {
        if let Kind::Heading(level) = block.kind {
            let title = block_title(t, &table, block, 60).replace("\\<open>", "").replace("\\<close>", "");
            col = col.push(nav_item(title, None, 12.0 * (level.saturating_sub(1) as f32), None, Message::Goto(block.start)));
        }
    }
    col.into()
}

fn is_goal(block: &Block, kinds: &std::collections::HashMap<String, String>) -> bool {
    match kinds.get(&block.name) {
        Some(k) => k.starts_with("thy_goal"),
        None => matches!(block.name.as_str(), "lemma" | "theorem" | "corollary" | "proposition" | "schematic_goal" | "lemmas"),
    }
}

fn theorems<'a>(app: &'a App, t: &'a Theory) -> Element<'a, Message> {
    let table = app.table(&app.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default());
    let mut col = widget::column::with_capacity(t.blocks.len()).spacing(1);
    for block in t.blocks.iter().filter(|b| b.kind == Kind::Formal && is_goal(b, &t.kinds)) {
        let title = block_title(t, &table, block, 48);
        let sorry = block.commands.iter().any(|c| c.name == "sorry" || c.name == "oops");
        let status = if sorry && block.status == Status::Finished { Status::Warned } else { block.status };
        col = col.push(nav_item(title, sorry.then(|| "unfinished".to_string()), 0.0, Some(status), Message::Goto(block.start)));
    }
    col.into()
}

fn problems<'a>(app: &'a App, t: &'a Theory) -> Element<'a, Message> {
    let table = app.table(&app.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default());
    let mut col = widget::column::with_capacity(32).spacing(1);
    // Imported theories that cannot load hold this one up
    if let Some(info) = app.workspace.as_ref().and_then(|w| w.info.as_ref()) {
        for th in info["theories"].as_array().into_iter().flatten() {
            if let Some(errors) = th["status"]["header_errors"].as_array().filter(|e| !e.is_empty())
                && let Some(path) = th["path"].as_str()
            {
                let msg = errors.iter().filter_map(|e| e.as_str()).collect::<Vec<_>>().join("; ");
                col = col.push(nav_item(format!("{} cannot load", file_name(path)), Some(msg), 0.0, Some(Status::Failed), Message::OpenTheory(path.to_string())));
            }
        }
    }
    for m in t.messages_in(0, usize::MAX / 2) {
        if m.kind != "error" {
            continue;
        }
        let line = super::ot::line_col(&t.local, m.start).0;
        let text: String = table.decode(&m.text).text.lines().next().unwrap_or("").chars().take(80).collect();
        col = col.push(nav_item(format!("Error, line {line}"), Some(text), 0.0, Some(Status::Failed), Message::Goto(m.start)));
    }
    for block in &t.blocks {
        for c in block.commands.iter().filter(|c| c.name == "sorry" || c.name == "oops") {
            let line = super::ot::line_col(&t.local, c.start).0;
            col = col.push(nav_item(format!("{}, line {line}", c.name), Some(block_title(t, &table, block, 48)), 0.0, Some(Status::Warned), Message::Goto(c.start)));
        }
    }
    col.into()
}

fn sessions_list(app: &App) -> Element<'_, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(32).spacing(sp.space_xxs);
    col = col.push(
        widget::row::with_capacity(2)
            .align_y(Alignment::Center)
            .push(widget::Space::new().width(Length::Fill))
            .push(widget::button::text("New session").on_press(Message::NewSession(true))),
    );
    for link in &app.links {
        let connected = link.sender.is_some();
        col = col.push(
            widget::text::caption_heading(format!("{}{}", link.name, if connected { String::new() } else { format!(" — {}", link.status) })),
        );
        for ((machine, sid), s) in app.sessions.iter().filter(|((m, _), _)| *m == link.target.key) {
            let current = app.workspace.as_ref().is_some_and(|w| w.machine == *machine && w.sid == *sid);
            let name = s["name"].as_str().unwrap_or(sid).to_string();
            let owner = s["owner"]["label"].as_str().map(|l| format!(" ({l})")).unwrap_or_default();
            let clients = s["clients"].as_array().map_or(0, |c| c.iter().filter(|c| c["kind"] == "agent").count());
            // A session: its name, and a line on its prover and agents; the open one marked by
            // weight and a quiet background, not a filled button
            let label = widget::column::with_capacity(2)
                .push(
                    widget::text(format!("{name}{owner}"))
                        .size(z(14.0))
                        .font(cosmic::iced::Font { weight: if current { cosmic::iced::font::Weight::Bold } else { cosmic::iced::font::Weight::Normal }, ..cosmic::font::default() })
                        .wrapping(iced::widget::text::Wrapping::None),
                )
                .push(widget::text::caption(format!(
                    "{}{}",
                    lifecycle_badge(s),
                    if clients > 0 { format!(" · {clients} agent{}", if clients == 1 { "" } else { "s" }) } else { String::new() }
                )));
            col = col.push(
                widget::button::custom(label)
                    .width(Length::Fill)
                    .padding([4, 8])
                    .class(if current { theme::Button::Standard } else { theme::Button::MenuItem })
                    .on_press(Message::OpenSession(machine.clone(), sid.clone())),
            );
            if current && let Some(ws) = &app.workspace {
                col = col.push(theories(app, ws));
            }
        }
    }
    col.into()
}

fn theories<'a>(app: &'a App, ws: &'a Workspace) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(64).spacing(1).padding(Padding { left: sp.space_xs as f32, ..Padding::ZERO });
    let Some(info) = &ws.info else { return widget::text::caption("Loading…").into() };
    let folder = info["folder"].as_str().unwrap_or("");
    let all: Vec<&Value> = info["theories"].as_array().into_iter().flatten().filter(|t| t["path"].as_str().is_some_and(|p| p.ends_with(".thy"))).collect();
    // A big folder gets a filter
    if all.len() > 10 {
        col = col.push(
            widget::container(widget::text_input(format!("Filter {} theories", all.len()), &ws.theory_filter).on_input(Message::TheoryFilter))
                .padding(Padding { bottom: 4.0, right: 4.0, ..Padding::ZERO }),
        );
    }
    let filter = ws.theory_filter.to_lowercase();
    // By folder, then the name less what all names in the folder start with
    let mut groups: std::collections::BTreeMap<String, Vec<(String, &Value)>> = std::collections::BTreeMap::new();
    for t in &all {
        let path = t["path"].as_str().unwrap_or("");
        let rel = path.strip_prefix(folder).unwrap_or(path).trim_start_matches('/');
        if !filter.is_empty() && super::symbols::fuzzy(&filter, &rel.to_lowercase()).is_none() {
            continue;
        }
        let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        groups.entry(dir.to_string()).or_default().push((name.trim_end_matches(".thy").to_string(), *t));
    }
    if groups.is_empty() {
        col = col.push(widget::text::caption("No theory matches"));
    }
    for (dir, items) in groups {
        let head = dir.clone();
        let indent = if head.is_empty() { 0.0 } else { 10.0 };
        if !head.is_empty() {
            col = col.push(widget::container(widget::text::caption(head)).padding(Padding { top: 4.0, left: 6.0, ..Padding::ZERO }));
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
            let current = ws.theory.as_ref().is_some_and(|th| th.path == path);
            let dirty = if t["dirty"].as_bool() == Some(true) { " •" } else { "" };
            let mut row = widget::row::with_capacity(2).spacing(6).align_y(Alignment::Center);
            row = row.push(
                widget::container(widget::Space::new())
                    .width(Length::Fixed(7.0))
                    .height(Length::Fixed(7.0))
                    .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                        background: status.map(|s| status_color(s, theme).into()),
                        border: Border { radius: 4.0.into(), ..Default::default() },
                        ..Default::default()
                    })),
            );
            row = row.push(widget::text(format!("{name}{dirty}")).size(z(13.0)).wrapping(iced::widget::text::Wrapping::None));
            col = col.push(
                widget::tooltip(
                    widget::button::custom(row)
                        .width(Length::Fill)
                        .padding(Padding { left: 6.0 + indent, right: 6.0, top: 2.0, bottom: 2.0 })
                        .class(if current { theme::Button::Standard } else { theme::Button::MenuItem })
                        .on_press(Message::OpenTheory(path.clone())),
                    widget::text::caption(format!("{name}.thy")),
                    widget::tooltip::Position::Right,
                ),
            );
        }
    }
    let _ = app;
    col.into()
}


fn new_session<'a>(app: &'a App, ns: &'a super::NewSession) -> Element<'a, Message> {
    let sp = spacing();
    let names: Vec<String> = app.links.iter().map(|l| l.name.clone()).collect();
    let mut col = widget::column::with_capacity(12)
        .spacing(sp.space_s)
        .padding(sp.space_l)
        .push(widget::text::title3("New session"))
        .push(widget::text::body("A session is one shared Isabelle prover for a folder of theories. It keeps running when you close the window; agents can attach to it."))
        .push(widget::settings::item("Machine", widget::dropdown(names, Some(ns.machine), Message::NewMachine)))
        .push(widget::settings::item(
            "Folder",
            widget::text_input("/path/to/theories", &ns.folder).on_input(Message::NewFolder).on_submit(|f| Message::NewBrowse(f)).width(Length::Fixed(480.0)),
        ));
    if let Some(listing) = &ns.listing {
        let mut list = widget::column::with_capacity(40).spacing(1);
        if let Some(parent) = listing["parent"].as_str() {
            list = list.push(widget::button::custom(widget::text::body("..")).class(theme::Button::MenuItem).width(Length::Fill).on_press(Message::NewBrowse(parent.to_string())));
        }
        let base = listing["path"].as_str().unwrap_or("");
        for e in listing["entries"].as_array().into_iter().flatten() {
            let name = e["name"].as_str().unwrap_or("");
            let dir = e["dir"].as_bool() == Some(true);
            let full = format!("{}/{}", base.trim_end_matches('/'), name);
            let label = if dir { format!("{name}/") } else { name.to_string() };
            let b = widget::button::custom(widget::text::body(label)).class(theme::Button::MenuItem).width(Length::Fill);
            list = list.push(if dir { b.on_press(Message::NewBrowse(full)) } else { b });
        }
        col = col.push(widget::container(widget::scrollable(list)).height(Length::Fixed(260.0)).class(theme::Container::Card));
    }
    let detail = ns.suggestion.as_ref().map(|s| {
        if let Some(note) = s["note"].as_str() {
            return note.to_string();
        }
        s["session"].as_str().map(|n| format!("The folder belongs to session {n}; its parent is the suggested base logic.")).unwrap_or_else(|| "No ROOT found; HOL is suggested.".into())
    });
    col = col.push(widget::settings::item("Base logic", widget::text_input("HOL", &ns.logic).on_input(Message::NewLogic).width(Length::Fixed(240.0))));
    if let Some(d) = detail {
        col = col.push(widget::text::caption(d));
    }
    if let Some(err) = &ns.error {
        col = col.push(widget::text::body(err.clone()).class(theme::Text::Accent));
    }
    col = col.push(
        widget::row::with_capacity(2)
            .spacing(sp.space_s)
            .push(widget::button::standard("Cancel").on_press(Message::NewSession(false)))
            .push(widget::button::suggested("Start session").on_press_maybe((!ns.folder.is_empty()).then_some(Message::NewCreate))),
    );
    widget::scrollable(col.max_width(820)).into()
}

// ----- the workspace

fn workspace<'a>(app: &'a App, ws: &'a Workspace, right: bool, main_width: f32) -> Element<'a, Message> {
    let sp = spacing();
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let mut center = widget::column::with_capacity(6).spacing(sp.space_xs).padding([sp.space_xs, sp.space_s]);
    // (the theory's page outside the padded column: its scroll view, and so its scroll bar, at the
    // window's edges)
    let mut page_body: Option<Element<'a, Message>> = None;
    VIEW_WIDTH.store(main_width.to_bits(), std::sync::atomic::Ordering::Relaxed);
    if let Some(s) = summary {
        let lc = s["lifecycle"].as_str().unwrap_or("");
        if let Some(msg) = s["message"].as_str() {
            center = center.push(banner(msg.to_string(), None));
        } else if lc == "Paused" {
            center = center.push(banner("Paused: the prover stays loaded, checking is suspended. Earlier output is historical.".into(), Some(("Resume", Message::Lifecycle("resume")))));
        }
        for r in s["requests"].as_array().into_iter().flatten() {
            center = center.push(request_banner(r));
        }
        if let Some(offer) = &ws.show_offer {
            let msg = offer["message"].as_str().unwrap_or("");
            let text = format!(
                "{} wants to show you line {} of {}{}",
                offer["from"]["label"].as_str().unwrap_or("An agent"),
                offer["line"],
                file_name(offer["path"].as_str().unwrap_or("")),
                if msg.is_empty() { String::new() } else { format!(": “{msg}”") }
            );
            let row = widget::row::with_capacity(3)
                .spacing(sp.space_s)
                .align_y(Alignment::Center)
                .push(widget::text::body(text).width(Length::Fill))
                .push(widget::button::standard("Dismiss").on_press(Message::ShowOffer(false)))
                .push(widget::button::suggested("Go there").on_press(Message::ShowOffer(true)));
            center = center.push(widget::container(row).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill));
        }
        if s["parent"].is_object() && s["owner"].is_object() {
            center = center.push(banner(
                format!("Independent session of {} on branch {}", s["owner"]["label"].as_str().unwrap_or("an agent"), s["parent"]["branch"].as_str().unwrap_or("")),
                Some(("Review and merge…", Message::Merge("preview".into()))),
            ));
        }
    }
    if let Some(files) = &ws.merge {
        let mut col = widget::column::with_capacity(files.len() + 2).spacing(sp.space_xxs);
        col = col.push(widget::text::heading(format!("{} changed theor{}", files.len(), if files.len() == 1 { "y" } else { "ies" })));
        for f in files {
            col = col.push(widget::text(f["diff"].as_str().unwrap_or("").to_string()).font(cosmic::font::mono()).size(12));
        }
        col = col.push(widget::row::with_capacity(2).spacing(sp.space_s).push(widget::button::suggested("Merge into the shared session").on_press(Message::Merge("apply".into()))));
        center = center.push(widget::container(widget::scrollable(col)).max_height(360.0).padding(sp.space_s).class(theme::Container::Card));
    }
    match &ws.theory {
        Some(t) => {
            if t.conflict.is_some() {
                center = center.push(conflict_banner());
            }
            if let Some(err) = &t.save_error {
                center = center.push(banner(format!("Saving failed: {err}"), Some(("Retry", Message::Save))));
            }
            for alt in t.alternatives.iter().take(3) {
                center = center.push(alternative_banner(alt));
            }

            // Imports that cannot load leave this theory waiting: say so
            if let Some(info) = &ws.info {
                let broken: Vec<String> = info["theories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|th| th["status"]["header_errors"].as_array().is_some_and(|e| !e.is_empty()))
                    .filter_map(|th| th["path"].as_str().map(|p| file_name(p).to_string()))
                    .collect();
                if !broken.is_empty() {
                    center = center.push(banner(
                        format!("{} cannot load (a bad import or header); theories importing it wait. See Problems.", broken.join(", ")),
                        Some(("Problems", Message::LeftMode(LeftMode::Problems))),
                    ));
                }
            }
            // The proof state (and what the tools typed in the text say) goes beside the block it
            // belongs to when there is room for both columns, else under it
            let preferred_goal = (main_width * 0.38).clamp(z(320.0), z(560.0));
            // The text column as wide as the code blocks (their code and status strip), within a
            // readable measure: long code lines wrap rather than push the goal underneath, and
            // the goal sits just past the blocks, a little apart
            let beside_width = (code_width(t.code_columns) + 8.0).clamp(z(440.0), z(680.0));
            // Beside when the text column, the gap and a goal of at least its minimum width fit
            // (room for the scroll bar kept); the side margins go first, before the goal goes under
            let room = main_width - SCROLLBAR_ROOM - beside_width - GOAL_GAP;
            let goal_width = preferred_goal.min(room).max(z(320.0));
            let fits = room >= z(320.0) && goal_beside_on();
            let gutter = ((room - goal_width) / 2.0).clamp(0.0, sp.space_l as f32);
            PAGE_GUTTER.store(gutter.to_bits(), std::sync::atomic::Ordering::Relaxed);
            // The goal's column is kept whenever it fits, whether or not there is a goal now:
            // moving between blocks never changes the layout. Without it, the page is a wider
            // column, centred, the code blocks as wide as their lines allow
            let beside = fits;
            let text_width = if fits { beside_width } else { (code_width(t.code_columns) + 8.0).clamp(z(560.0), z(880.0)) };
            COLUMN_CAP.store(text_width.to_bits(), std::sync::atomic::Ordering::Relaxed);
            WIDE.store(fits, std::sync::atomic::Ordering::Relaxed);
            let inline = if beside { None } else { Some(ws) };
            let _ = right;
            let mut body: Element<'_, Message> = match t.mode {
                Mode::Literate if beside && goal_placement(true, 0) == GoalPlacement::Pinned => {
                    // the goal column beside the page, at the top of the view, not scrolling
                    let goal: Element<'_, Message> = match inline_goal(Some(ws), &app.table(&ws.machine)) {
                        Some(g) => widget::container(widget::scrollable(g)).width(Length::Fixed(goal_width)).into(),
                        None => widget::Space::new().width(Length::Fixed(goal_width)).into(),
                    };
                    widget::row::with_capacity(2)
                        .spacing(sp.space_m)
                        .push(widget::container(literate(app, t, None, None, text_width)).width(Length::Fill).height(Length::Fill))
                        .push(goal)
                        .into()
                }
                Mode::Literate => literate(app, t, inline, beside.then_some((ws, goal_width, text_width)), text_width),
                Mode::Source => {
                    // The source as wide as its lines (up to a readable measure), the goal beside the
                    // caret's line when there is room for both
                    let columns = t.decoded.text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
                    let source_width = source_width(columns);
                    // (the goal column narrows to the room there is, down to a readable width)
                    let room = (main_width - source_width - z(70.0)).min(goal_width);
                    let fits = (ws.output.is_some() || fill_here(ws)) && room >= z(320.0) && goal_beside_on();
                    source(ws, t, if fits { None } else { Some(ws) }, &app.table(&ws.machine), fits.then_some((room, source_width)), source_width.min(main_width - z(24.0)))
                }
            };
            let _ = &mut body;
            page_body = Some(body);
        }
        None => {
            center = center.push(widget::container(widget::text::body(if ws.opening.is_some() { "Opening…" } else { "Choose a theory on the left." })).center(Length::Fill));
        }
    }
    let mut outer = widget::column::with_capacity(2).push(center);
    if let Some(body) = page_body {
        outer = outer.push(body);
    }
    widget::container(outer).width(Length::Fill).height(Length::Fill).into()
}

/// The workspace's width, as the view last laid it out
static VIEW_WIDTH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn view_width() -> f32 {
    f32::from_bits(VIEW_WIDTH.load(std::sync::atomic::Ordering::Relaxed))
}

/// What an agent's request (or grant) covers, in words
fn scope_words(r: &Value) -> String {
    match r["scope"].as_str() {
        Some("folder") => "the whole folder".to_string(),
        Some("theory") => file_name(r["path"].as_str().unwrap_or("")).to_string(),
        Some("proof") => format!("the proof at line {} of {}", r["line"], file_name(r["path"].as_str().unwrap_or(""))),
        _ => "this session".into(),
    }
}

/// An agent asks for editing rights: Allow or Deny
fn request_banner<'a>(r: &Value) -> Element<'a, Message> {
    let sp = spacing();
    let id = r["id"].as_str().unwrap_or("").to_string();
    let reason = r["reason"].as_str().unwrap_or("");
    let mut text = widget::column::with_capacity(2).spacing(2).width(Length::Fill).push(widget::text::body(format!(
        "{} asks to edit {}",
        r["label"].as_str().unwrap_or("An agent"),
        scope_words(r)
    )));
    if !reason.is_empty() {
        text = text.push(widget::text::caption(format!("“{reason}”")));
    }
    let row = widget::row::with_capacity(3)
        .spacing(sp.space_s)
        .align_y(Alignment::Center)
        .push(text)
        .push(widget::button::standard("Deny").on_press(Message::AnswerRequest(id.clone(), false)))
        .push(widget::button::suggested("Allow").on_press(Message::AnswerRequest(id, true)));
    widget::container(row).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill).into()
}

/// An agent's note on the text: who, what, the replies so far, and a reply or Resolve
fn note_card<'a>(ws: &'a Workspace, n: &Value) -> Element<'a, Message> {
    let sp = spacing();
    let id = n["id"].as_str().unwrap_or("").to_string();
    let mut col = widget::column::with_capacity(6).spacing(4);
    col = col.push(widget::text::caption(format!("{} · note on line {}", n["label"].as_str().unwrap_or("Agent"), n["line"])));
    col = col.push(widget::text::body(n["text"].as_str().unwrap_or("").to_string()));
    for r in n["replies"].as_array().into_iter().flatten() {
        col = col.push(widget::text::caption(format!("{}: {}", r["by"].as_str().unwrap_or(""), r["text"].as_str().unwrap_or(""))));
    }
    let reply = ws.note_reply.get(&id).cloned().unwrap_or_default();
    let reply_id = id.clone();
    col = col.push(
        widget::row::with_capacity(3)
            .spacing(sp.space_xs)
            .align_y(Alignment::Center)
            .push(
                widget::text_input("Reply", reply)
                    .on_input(move |t| Message::NoteReplyInput(reply_id.clone(), t))
                    .on_submit({
                        let id = id.clone();
                        move |_| Message::NoteReply(id.clone(), false)
                    })
                    .width(Length::Fill),
            )
            .push(widget::button::text("Reply").on_press(Message::NoteReply(id.clone(), false)))
            .push(widget::button::text("Resolve").on_press(Message::NoteReply(id, true))),
    );
    widget::container(col)
        .padding([sp.space_xxs, sp.space_s])
        .width(Length::Fill)
        .max_width(code_width(60) + 8.0)
        .class(theme::Container::custom(|theme| {
            let c = theme.cosmic();
            iced::widget::container::Style {
                background: Some(blend(Color::from(c.bg_component_color()), Color::from(c.warning_color()), 0.10).into()),
                border: Border { radius: 6.0.into(), width: 0.0, color: Color::TRANSPARENT },
                ..Default::default()
            }
        }))
        .into()
}

fn banner<'a>(text: String, action: Option<(&'static str, Message)>) -> Element<'a, Message> {
    let sp = spacing();
    let mut row = widget::row::with_capacity(2).spacing(sp.space_s).align_y(Alignment::Center).push(widget::text::body(text).width(Length::Fill));
    if let Some((label, msg)) = action {
        row = row.push(widget::button::standard(label).on_press(msg));
    }
    widget::container(row).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill).into()
}

fn conflict_banner<'a>() -> Element<'a, Message> {
    let sp = spacing();
    widget::container(
        widget::row::with_capacity(4)
            .spacing(sp.space_s)
            .align_y(Alignment::Center)
            .push(widget::text::body("This theory changed on disk while it had unsaved edits. Autosave is holding.").width(Length::Fill))
            .push(widget::button::standard("Reload from disk").on_press(Message::Resolve("disk")))
            .push(widget::button::standard("Keep mine").on_press(Message::Resolve("buffer"))),
    )
    .padding(sp.space_xs)
    .class(theme::Container::Card)
    .width(Length::Fill)
    .into()
}

fn alternative_banner<'a>(alt: &'a Value) -> Element<'a, Message> {
    let sp = spacing();
    let id = alt["id"].as_str().unwrap_or("").to_string();
    let mut row = widget::row::with_capacity(4).spacing(sp.space_s).align_y(Alignment::Center);
    let n = alt["pieces"].as_array().map_or(0, Vec::len);
    row = row.push(
        widget::text::body(format!(
            "{} of a {} could not be applied over later edits; the text is kept here",
            if n == 1 { "One part".to_string() } else { format!("{n} parts") },
            alt["kind"].as_str().unwrap_or("change")
        ))
        .width(Length::Fill),
    );
    for (i, p) in alt["pieces"].as_array().into_iter().flatten().enumerate().take(3) {
        let restore: String = p["restore"].as_str().unwrap_or("").chars().take(24).collect();
        let current: String = p["current"].as_str().unwrap_or("").chars().take(24).collect();
        let label = match (restore.is_empty(), current.is_empty()) {
            (true, false) => format!("Remove “{current}” (line {})", p["line"]),
            (false, true) => format!("Insert “{restore}” (line {})", p["line"]),
            _ => format!("Put back “{restore}” for “{current}” (line {})", p["line"]),
        };
        row = row.push(widget::button::standard(label).on_press(Message::RestoreAlternative(id.clone(), i)));
    }
    row = row.push(widget::button::text("Dismiss").on_press(Message::DismissAlternative(id)));
    widget::container(row).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill).into()
}

/// The prover's state in the header bar, as COSMIC apps show work in progress: never an
/// idle-looking editor while the prover is busy. Clicking it opens the details
pub fn header_center(app: &App) -> Vec<Element<'_, Message>> {
    let Some(ws) = &app.workspace else { return Vec::new() };
    let lifecycle = app
        .sessions
        .get(&(ws.machine.clone(), ws.sid.clone()))
        .and_then(|s| s["lifecycle"].as_str())
        .unwrap_or("")
        .to_string();
    match (&ws.theory, lifecycle.as_str()) {
        (Some(t), "Running") => prover_status(ws, t).into_iter().collect(),
        (_, "") => Vec::new(),
        (_, lc) => vec![
            widget::button::custom(widget::text::body(format!("Prover {}", lc.to_lowercase())))
                .class(theme::Button::Text)
                .on_press(Message::Panel(Panel::Log))
                .into(),
        ],
    }
}

fn prover_status<'a>(ws: &'a Workspace, t: &'a Theory) -> Option<Element<'a, Message>> {
    let act = ws.activity.as_ref()?;
    let theories = act["theories"].as_array()?;
    let find = |path: &str| theories.iter().find(|th| th["path"].as_str() == Some(path));
    // The current theory's imports, transitively, that are not checked yet
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
    let running = act["running"].as_array().cloned().unwrap_or_default();
    let busy_theories = theories.iter().filter(|th| th["running"].as_u64().unwrap_or(0) > 0).count();
    let (finished, unprocessed, run_here, failed, consolidated) = t.status();
    let total = (finished + unprocessed + run_here + failed).max(1);
    let prover = &act["prover"];
    let busy = !running.is_empty() || busy_theories > 0;
    let mut parts: Vec<String> = Vec::new();
    if !waiting.is_empty() {
        let blocked = waiting.iter().filter(|w| w.2).count();
        parts.push(if blocked > 0 {
            format!("{blocked} import{} cannot load", if blocked == 1 { "" } else { "s" })
        } else {
            format!("waiting for {} import{}", waiting.len(), if waiting.len() == 1 { "" } else { "s" })
        });
    } else if busy || (!consolidated && !t.prover.status.is_null()) {
        parts.push(format!("checking {}%", (finished + failed) * 100 / total));
    } else if !t.prover.status.is_null() {
        parts.push("checked".into());
    }
    if failed > 0 {
        parts.push(format!("{failed} failing"));
    }
    let is_tool = |r: &Value| r["name"].as_str().is_some_and(|n| super::editor::TOOLS.contains(&n));
    if let Some(longest) = running.iter().find(|r| !is_tool(r))
        && longest["seconds"].as_u64().unwrap_or(0) >= 5
    {
        parts.push(format!("{} {} s", longest["name"].as_str().unwrap_or(""), longest["seconds"]));
    }
    if let Some(cpu) = prover["cpu"].as_u64().filter(|c| *c >= 5) {
        parts.push(format!("CPU {cpu}%"));
    }
    if let Some(mb) = prover["rss_mb"].as_u64() {
        parts.push(format!("{:.1} GB", mb as f64 / 1000.0));
    }
    let mut row = widget::row::with_capacity(4).spacing(6).align_y(Alignment::Center);
    if busy {
        row = row.push(widget::progress_bar::circular::Circular::new().size(14.0));
    }
    row = row.push(widget::text::body(parts.join(" · ")));
    let tip = if waiting.is_empty() {
        "Prover activity: click for details".to_string()
    } else {
        let mut w = waiting.clone();
        w.sort_by_key(|w| (!w.2, w.1));
        format!(
            "Waiting for: {}",
            w.iter().take(8).map(|(n, p, f)| if *f { format!("{n} (cannot load)") } else { format!("{n} {p}%") }).collect::<Vec<_>>().join(", ")
        )
    };
    let status = widget::tooltip(
        widget::button::custom(row).class(theme::Button::Text).on_press(Message::Panel(Panel::Log)),
        widget::text(tip),
        widget::tooltip::Position::Bottom,
    );
    let mut out = widget::row::with_capacity(3).spacing(2).align_y(Alignment::Center).push(status);
    // A proof tool typed in the text, running: stoppable on its own
    // A proof tool typed in the text: its own entry, with its own Stop
    if let Some(tool) = running.iter().find(|r| is_tool(r)) {
        let label = format!("{} {} s", tool["name"].as_str().unwrap_or(""), tool["seconds"].as_u64().unwrap_or(0));
        if ws.tools_stopping {
            out = out.push(widget::text::body(format!("· {label} · stopping…")));
        } else {
            out = out
                .push(widget::text::body(format!("· {label}")))
                .push(icon_button("process-stop-symbolic", "Stop it (as if its time ran out)", Some(Message::StopTools)));
        }
    }
    // Interrupt: for proofs other than the tools
    let others = running.iter().any(|r| !is_tool(r));
    if busy && others {
        out = out.push(icon_button("process-stop-symbolic", "Interrupt running proofs", Some(Message::Lifecycle("cancel"))));
    }
    Some(out.into())
}

fn activity_panel(ws: &Workspace) -> Element<'_, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(64).spacing(sp.space_xs);
    if let Some(act) = &ws.activity {
        let prover = &act["prover"];
        col = col.push(widget::text::heading(format!(
            "Prover {}{}{}",
            act["lifecycle"].as_str().unwrap_or(""),
            prover["cpu"].as_u64().map(|c| format!(" · CPU {c}%")).unwrap_or_default(),
            prover["rss_mb"].as_u64().map(|m| format!(" · {:.1} GB", m as f64 / 1000.0)).unwrap_or_default()
        )));
        col = col.push(widget::button::standard("Interrupt running proofs").on_press(Message::Lifecycle("cancel")));
        let running = act["running"].as_array().cloned().unwrap_or_default();
        if !running.is_empty() {
            col = col.push(widget::text::caption_heading("Running"));
            for r in running {
                let path = r["path"].as_str().unwrap_or("").to_string();
                col = col.push(widget::text::body(format!(
                    "{} at {} line {} — {} s",
                    r["name"].as_str().unwrap_or(""),
                    file_name(&path),
                    r["line"],
                    r["seconds"]
                )));
            }
        }
        col = col.push(widget::text::caption_heading("Theories"));
        let mut theories = act["theories"].as_array().cloned().unwrap_or_default();
        theories.sort_by_key(|th| (th["consolidated"].as_bool() == Some(true), th["percentage"].as_u64().unwrap_or(0)));
        for th in theories.iter().take(80) {
            let name = th["theory"].as_str().unwrap_or("").to_string();
            let pct = th["percentage"].as_u64().unwrap_or(0);
            let failed = th["failed"].as_u64().unwrap_or(0) > 0 || th["header_errors"].as_array().is_some_and(|e| !e.is_empty());
            let label = if th["consolidated"].as_bool() == Some(true) && !failed {
                format!("{name} ✓")
            } else if failed {
                format!("{name} ✗ {pct}%")
            } else {
                format!("{name} {pct}%{}", if th["running"].as_u64().unwrap_or(0) > 0 { " · running" } else { "" })
            };
            col = col.push(widget::text::body(label));
            col = col.push(widget::progress_bar::linear::Linear::new().progress(pct as f32 / 100.0).width(Length::Fill).girth(4.0));
        }
    } else {
        col = col.push(widget::text::caption("No prover activity yet."));
    }
    col = col.push(widget::text::caption_heading("Log"));
    col = col.push(widget::text(ws.log.iter().rev().take(80).rev().cloned().collect::<Vec<_>>().join("\n")).font(cosmic::font::mono()).size(z(12.0)));
    widget::scrollable(col).height(Length::Fill).into()
}

/// Quickcheck, Nitpick and the simple methods at the caret, without editing the theory
fn quickcheck_panel<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    let q = &ws.cex;
    let mut col = widget::column::with_capacity(12).spacing(sp.space_xs);
    col = col.push(widget::text::caption("Counterexamples and quick proofs for the goal at the caret. Typing quickcheck or nitpick in the proof works too."));
    col = col.push(
        widget::row::with_capacity(4)
            .spacing(4)
            .push(widget::button::suggested("Quickcheck").on_press(Message::RunCommand("quickcheck")))
            .push(widget::button::standard("Nitpick").on_press(Message::RunCommand("nitpick")))
            .push(widget::button::standard("Try methods").on_press(Message::RunCommand("try0")))
            .push(widget::button::standard("Solve direct").on_press(Message::RunCommand("solve_direct"))),
    );
    col = col.push(widget::text_input("options, e.g. timeout = 60, size = 8, tester = random", &ws.cex_params).on_input(Message::CommandParams));
    if !q.status.is_empty() {
        col = col.push(
            widget::row::with_capacity(2)
                .spacing(sp.space_s)
                .align_y(Alignment::Center)
                .push(widget::text::heading(if q.status == "finished" { "Finished".to_string() } else { q.status.clone() }).width(Length::Fill))
                .push(widget::button::text("Cancel").on_press_maybe((q.status != "finished" && q.status != "failed").then_some(Message::CancelQuery))),
        );
    }
    if let Some(err) = &q.error {
        col = col.push(widget::text::body(err.clone()));
    }
    for m in &q.messages {
        col = col.push(message_view(ws, m, table));
        for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
            col = col.push(
                widget::row::with_capacity(2)
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .push(widget::text(table.decode(sb).text).font(cosmic::font::mono()).size(z(13.0)).width(Length::Fill))
                    .push(widget::button::suggested("Insert").on_press(Message::InsertOutput(sb.to_string()))),
            );
        }
    }
    widget::scrollable(col).height(Length::Fill).into()
}

fn theory_status(t: &Theory) -> Element<'_, Message> {
    let (finished, unprocessed, running, failed, consolidated) = t.status();
    let total = (finished + unprocessed + running + failed).max(1);
    let text = if consolidated && failed == 0 {
        "Checked".to_string()
    } else if t.prover.status.is_null() {
        "Not checked yet".into()
    } else {
        format!("{}% checked{}{}", (finished + failed) * 100 / total, if running > 0 { format!(" · {running} running") } else { String::new() }, if failed > 0 { format!(" · {failed} failed") } else { String::new() })
    };
    widget::row::with_capacity(3)
        .spacing(spacing().space_s)
        .push(widget::text::caption(file_name(&t.path).to_string()))
        .push(widget::text::caption(text))
        .push(widget::text::caption(if t.dirty { "unsaved" } else { "saved" }))
        .into()
}

fn mono_block<'a>(text: String, status: Status) -> Element<'a, Message> {
    let strip = widget::container(widget::Space::new())
        .width(Length::Fixed(4.0))
        .height(Length::Fill)
        .class(theme::Container::custom(move |theme| iced::widget::container::Style {
            background: Some(status_color(status, theme).into()),
            border: Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }));
    let body = widget::container(widget::text(text).font(cosmic::font::mono()).size(13))
        .padding(Padding { left: 8.0, right: 6.0, top: 4.0, bottom: 4.0 })
        .width(Length::Fill)
        .class(theme::Container::custom(move |theme| iced::widget::container::Style {
            background: Some(Color::from(theme.cosmic().bg_component_color()).into()),
            border: Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        }));
    widget::row::with_capacity(2).spacing(4).height(Length::Shrink).push(strip).push(body).into()
}

/// A formal block as the source view colours it
/// Add a text, section or lemma after a block (or at the end), and delete the block
fn block_actions<'a>(after: Option<usize>, delete: bool) -> Element<'a, Message> {
    let small = |label: &'static str, msg: Message| widget::button::text(label).font_size(z(12.0) as u16).padding([0, 6]).on_press(msg);
    let mut row = widget::row::with_capacity(5)
        .spacing(2)
        .align_y(Alignment::Center)
        .push(small("+ Text", Message::InsertBlock(after, "text")))
        .push(small("+ Section", Message::InsertBlock(after, "section")))
        .push(small("+ Lemma", Message::InsertBlock(after, "lemma")));
    if let Some(i) = after.filter(|_| delete) {
        row = row.push(widget::Space::new().width(Length::Fixed(12.0))).push(small("Delete", Message::DeleteBlock(i)));
    }
    row.into()
}

/// The line of the block editor's text a source offset is on
fn block_line(be: &super::editor::BlockEditor, offset: usize) -> usize {
    let shown = be.shown();
    super::editor::LineIndex::new(&shown).position(&shown, be.decoded.to_decoded(offset.saturating_sub(be.start))).line
}

/// A line of code in a block
fn code_line_height() -> f32 {
    z(13.0) * CODE_LINE
}

/// Padding of code in a block, shown or edited: the same, so that opening a block moves nothing
const CODE_PADDING: Padding = Padding { left: 10.0, right: 10.0, top: 5.0, bottom: 5.0 };

/// The width of a block of code with this many columns: its text and a little room, not the page
/// (the page column caps it, and long lines wrap)
pub fn code_width(columns: usize) -> f32 {
    (columns.max(30) as f32 + 4.0) * z(13.0) * 0.6 + CODE_PADDING.left + CODE_PADDING.right
}

/// The width of a block of code on the page: as [`code_width`], but never wider than the text
/// column can be (its status strip beside it), so nothing on the page runs into the goal drawn
/// past the column; longer lines wrap
pub fn block_code_width(columns: usize) -> f32 {
    let cap = f32::from_bits(COLUMN_CAP.load(std::sync::atomic::Ordering::Relaxed));
    code_width(columns).min(if cap > 0.0 { cap } else { z(680.0) } - 8.0)
}

/// The text column's width as the layout last chose it (blocks are never wider)
static COLUMN_CAP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// How a block of code looks, shown or being edited: the current one outlined in the accent
fn code_style(marked: bool) -> impl Fn(&cosmic::Theme, iced::widget::text_editor::Status) -> iced::widget::text_editor::Style {
    move |theme: &cosmic::Theme, _status| {
        let c = theme.cosmic();
        let accent = Color::from(c.accent_color());
        iced::widget::text_editor::Style {
            // The current block: darker in a dark theme; in a light one a wash of the accent (a
            // darker grey reads as dull there, not as current); its strip takes the accent
            background: if marked {
                if c.is_dark {
                    blend(Color::from(c.bg_component_color()), Color::BLACK, 0.32).into()
                } else {
                    blend(Color::from(c.bg_component_color()), accent, 0.07).into()
                }
            } else {
                Color::from(c.bg_component_color()).into()
            },
            border: Border { radius: 4.0.into(), width: 0.0, color: Color::TRANSPARENT },
            placeholder: Color { a: 0.5, ..Color::from(c.on_bg_color()) },
            value: c.on_bg_color().into(),
            selection: Color { a: 0.32, ..Color::from(c.accent.base) },
        }
    }
}

fn blend(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a }
}

/// A block's status strip, then its code
fn framed<'a>(code: Element<'a, Message>, status: Status, marked: bool) -> Element<'a, Message> {
    let strip = widget::container(widget::Space::new())
        .width(Length::Fixed(4.0))
        .height(Length::Fill)
        .class(theme::Container::custom(move |theme| iced::widget::container::Style {
            background: Some(if marked { Color::from(theme.cosmic().accent_color()) } else { status_color(status, theme) }.into()),
            border: Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }));
    // (the height after the children: a pushed Fill child would make the row fill)
    widget::row::with_capacity(2).spacing(4).push(strip).push(code).height(Length::Shrink).into()
}

/// An editor that says what is under the pointer: the pointer's place (less the editor's
/// padding) goes to the app, and what the prover says about it shows beside the pointer
fn hoverable<'a>(ws: Option<&'a Workspace>, target: super::HoverTarget, pad: Padding, el: Element<'a, Message>) -> Element<'a, Message> {
    hoverable_press(ws, target, pad, el, None)
}

/// As [`hoverable`]; with `press`, a layer over the editor takes clicks (a shown block opens at
/// the click without its read-only editor taking focus, and so a caret)
fn hoverable_press<'a>(ws: Option<&'a Workspace>, target: super::HoverTarget, pad: Padding, el: Element<'a, Message>, press: Option<Message>) -> Element<'a, Message> {
    let el: Element<'a, Message> = match press {
        Some(msg) => iced::widget::stack![el, widget::mouse_area(widget::Space::new().width(Length::Fill).height(Length::Fill)).on_press(msg)].into(),
        None => el,
    };
    let area = widget::mouse_area(el)
        .on_move(move |p| Message::HoverMove(target, iced::Point::new(p.x - pad.left, p.y - pad.top)))
        .on_exit(Message::HoverExit);
    // Always in a popover, the card added when there is one: the editor inside keeps its state
    // (focus, caret) when a tooltip comes and goes
    let pop = widget::popover(area);
    let Some(ws) = ws else { return pop.into() };
    let Some(h) = ws.hover.as_ref().filter(|h| h.target == target) else { return pop.into() };
    let Some(card) = hover_card(ws, h) else { return pop.into() };
    pop.popup(card)
        .position(widget::popover::Position::Point(iced::Point::new(h.point.x + pad.left + 8.0, h.point.y + pad.top + z(20.0))))
        .into()
}

/// The keys a read-only view (the goal, a tool's output) takes: copying and selecting, so that
/// every other key goes on to the page's block navigation even when a click left it focused
pub fn read_only_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    use text_editor::Binding as B;
    B::from_key_press(press).filter(|b| matches!(b, B::Copy | B::Select(_) | B::SelectWord | B::SelectLine | B::SelectAll | B::Unfocus))
}

/// An error or warning without the goal Isabelle lists after it (the goal view shows that) or
/// its position marker
pub fn message_summary_pub(text: &str) -> String {
    message_summary(text)
}

fn message_summary(text: &str) -> String {
    let t = text.replace("\\<^here>", "").replace('\u{2302}', "");
    let cut = ["\ngoal (", "\ngoal:", "\nGoal ("].iter().filter_map(|g| t.find(g)).min().unwrap_or(t.len());
    t[..cut].trim_end().trim_end_matches(':').to_string()
}

/// Text with Isabelle symbols, in the code font: scripts with no Unicode form are drawn in the
/// scripts font (as the editors draw them), bold symbols bold
fn symbol_text<'a>(text: &str, table: &super::symbols::Table, size: f32) -> Element<'a, Message> {
    let d = table.decode(text);
    let style_at: std::collections::HashMap<usize, super::symbols::Style> = d.styles.iter().map(|(i, st)| (*i, *st)).collect();
    let mut runs: Vec<(String, Option<super::symbols::Style>)> = Vec::new();
    for (i, ch) in d.text.chars().enumerate() {
        let st = style_at.get(&i).copied();
        match runs.last_mut() {
            Some((t, k)) if *k == st => t.push(ch),
            _ => runs.push((ch.to_string(), st)),
        }
    }
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = runs
        .into_iter()
        .map(|(t, st)| iced::widget::span(t).font(symbol_font(st)))
        .collect();
    iced::widget::rich_text(spans).size(size).into()
}

fn symbol_font(st: Option<super::symbols::Style>) -> cosmic::iced::Font {
    match st {
        Some(super::symbols::Style::Script) => cosmic::iced::Font::with_name("Isabellum Scripts"),
        Some(super::symbols::Style::Bold) => cosmic::iced::Font { weight: cosmic::iced::font::Weight::Bold, ..cosmic::font::mono() },
        _ => cosmic::font::mono(),
    }
}

/// A message as code, as wide as its text: coloured spans in the code font, wrapping when long
fn code_text<'a>(m: &Value) -> Element<'a, Message> {
    let theme = theme::active();
    let raw = m["raw"].as_str().or_else(|| m["text"].as_str()).unwrap_or("");
    let d = super::symbols::Table::fallback().decode(raw.trim_end());
    let n = d.src_of.len().saturating_sub(1);
    let mut class: Vec<Option<super::editor::Sem>> = vec![None; n];
    for sp in m["spans"].as_array().into_iter().flatten() {
        let (Some(s), Some(e), Some(sem)) = (sp[0].as_u64(), sp[1].as_u64(), sp[2].as_str().and_then(super::editor::Sem::parse)) else { continue };
        for c in class.iter_mut().take(d.to_decoded(e as usize).min(n)).skip(d.to_decoded(s as usize)) {
            *c = Some(sem);
        }
    }
    let style_at: std::collections::HashMap<usize, super::symbols::Style> = d.styles.iter().map(|(i, st)| (*i, *st)).collect();
    let mut runs: Vec<(String, Option<super::editor::Sem>, Option<super::symbols::Style>)> = Vec::new();
    for (i, (ch, c)) in d.text.chars().zip(class.iter().copied().chain(std::iter::repeat(None))).enumerate() {
        let st = style_at.get(&i).copied();
        match runs.last_mut() {
            Some((t, k, s)) if *k == c && *s == st => t.push(ch),
            _ => runs.push((ch.to_string(), c, st)),
        }
    }
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = runs
        .into_iter()
        .map(|(t, c, st)| {
            let mut span = iced::widget::span(t).font(symbol_font(st));
            if let Some(sem) = c
                && let Some(color) = highlight_format(&super::editor::Hl::Sem(sem), &theme).color
            {
                span = span.color(color);
            }
            span
        })
        .collect();
    iced::widget::rich_text(spans).size(z(13.0)).into()
}

/// What Isabelle says about the place under the pointer: types, what a name is, messages there
fn hover_card<'a>(ws: &'a Workspace, h: &super::Hover) -> Option<Element<'a, Message>> {
    let info = h.info.as_ref()?;
    let items: Vec<&Value> = info["tooltip"]["items"].as_array().into_iter().flatten().collect();
    let definition = info["definition"].as_object();
    if items.is_empty() && definition.is_none() {
        return None;
    }
    let sp = spacing();
    let mut col = widget::column::with_capacity(items.len() + 1).spacing(4);
    for item in items {
        col = col.push(code_text(item));
    }
    if let Some(d) = definition {
        let name = d.get("name").and_then(Value::as_str).unwrap_or("");
        let file = file_name(d.get("file").and_then(Value::as_str).unwrap_or(""));
        col = col.push(widget::text::caption(format!("Ctrl+click: go to the definition of {name} ({file}, line {})", d.get("line").cloned().unwrap_or_default())));
    }
    Some(
        widget::container(col)
            .max_width(z(520.0))
            .padding([sp.space_xxs, sp.space_xs])
            .class(theme::Container::custom(|theme| {
                let c = theme.cosmic();
                iced::widget::container::Style {
                    background: Some(Color::from(c.bg_component_color()).into()),
                    border: Border { radius: 6.0.into(), width: 1.0, color: Color { a: 0.2, ..Color::from(c.on_bg_color()) } },
                    shadow: iced::Shadow { color: Color { a: 0.35, ..Color::BLACK }, offset: iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
                    ..Default::default()
                }
            }))
            .into(),
    )
}

/// A formal block, shown: read-only text that a click opens for editing at the click
fn block_code<'a>(ws: Option<&'a Workspace>, t: &'a Theory, i: usize, block: &Block, marked: bool) -> Element<'a, Message> {
    let Some(Some(v)) = t.block_views.get(i) else { return highlighted_block(t, block) };
    let editor = iced::widget::text_editor(&v.content)
        .font(cosmic::font::mono())
        .size(z(13.0))
        .line_height(iced::widget::text::LineHeight::Relative(CODE_LINE))
        .padding(CODE_PADDING)
        .width(block_code_width(t.code_columns.max(v.columns)))
        .on_action(move |a| Message::BlockViewAction(i, a))
        .key_binding(|_| None)
        .style(code_style(marked))
        .highlight_with::<super::editor::Highlighter>(v.highlight.themed(), highlight_format);
    framed(
        hoverable_press(ws, super::HoverTarget::View(i), CODE_PADDING, editor.into(), Some(Message::ViewPress(i))),
        t.shown_status(block.start, block.end, block.status),
        marked,
    )
}

fn highlighted_block<'a>(t: &Theory, block: &Block) -> Element<'a, Message> {
    let theme = theme::active();
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = t
        .highlighted(block.start, block.end)
        .into_iter()
        .map(|(text, hl)| {
            let mut span = iced::widget::span(text).font(cosmic::font::mono());
            if let Some(hl) = hl {
                let format = highlight_format(&hl, &theme);
                if let Some(color) = format.color {
                    span = span.color(color);
                }
                if let Some(font) = format.font {
                    span = span.font(font);
                }
            }
            span
        })
        .collect();
    let text: Element<'a, Message> = iced::widget::rich_text(spans).size(13).into();
    let status = block.status;
    let strip = widget::container(widget::Space::new())
        .width(Length::Fixed(4.0))
        .height(Length::Fill)
        .class(theme::Container::custom(move |theme| iced::widget::container::Style {
            background: Some(status_color(status, theme).into()),
            border: Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }));
    let body = widget::container(text)
        .padding(Padding { left: 8.0, right: 6.0, top: 4.0, bottom: 4.0 })
        .width(Length::Fill)
        .class(theme::Container::custom(move |theme| iced::widget::container::Style {
            background: Some(Color::from(theme.cosmic().bg_component_color()).into()),
            border: Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        }));
    widget::row::with_capacity(2).spacing(4).height(Length::Shrink).push(strip).push(body).into()
}

/// The text zoom, set with Ctrl+= and Ctrl+-
static ZOOM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

pub fn set_zoom(z: f32) {
    ZOOM.store(z.clamp(0.5, 3.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub fn zoom() -> f32 {
    f32::from_bits(ZOOM.load(std::sync::atomic::Ordering::Relaxed))
}

/// The vim mode the editors are in, or none without vim keys
static VIM_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn set_vim_mode(mode: Option<super::vim::Mode>) {
    let v = match mode {
        None => 0,
        Some(super::vim::Mode::Normal) => 1,
        Some(super::vim::Mode::Insert) => 2,
        Some(super::vim::Mode::Visual) => 3,
        Some(super::vim::Mode::VisualLine) => 4,
    };
    VIM_MODE.store(v, std::sync::atomic::Ordering::Relaxed);
}

fn vim_mode_now() -> Option<super::vim::Mode> {
    match VIM_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(super::vim::Mode::Normal),
        2 => Some(super::vim::Mode::Insert),
        3 => Some(super::vim::Mode::Visual),
        4 => Some(super::vim::Mode::VisualLine),
        _ => None,
    }
}

/// A size at the current zoom
pub fn z(px: f32) -> f32 {
    (px * zoom()).round()
}

/// Messages under a block: all for the proof at the caret, errors for failing ones
fn messages_view<'a>(t: &Theory, block: &Block, table: &super::symbols::Table, current: bool) -> Option<Element<'a, Message>> {
    let all = t.messages_in(block.start, block.end);
    let waiting = all.iter().any(|m| t.expected_error(m));
    let msgs: Vec<_> = all
        .into_iter()
        .filter(|m| !t.expected_error(m))
        .filter(|m| current || m.kind == "error" || m.text.contains("counterexample"))
        .collect();
    if msgs.is_empty() {
        return waiting.then(|| widget::text::caption("Unfinished: pick a proof the tool finds (beside the proof state)").into());
    }
    let mut col = widget::column::with_capacity(msgs.len()).spacing(2);
    for m in msgs.into_iter().take(6) {
        let kind = m.kind.clone();
        let line = super::ot::line_col(&t.local, m.start).0;
        // (what went wrong: the goal it lists is the one the goal view shows)
        col = col.push(
            widget::container(symbol_text(&format!("{kind} (line {line}): {}", message_summary(&m.text)), table, z(12.0)))
                .padding([2, 8])
                .width(Length::Fill)
                .class(theme::Container::custom(move |theme| {
                    let c = theme.cosmic();
                    let color: Color = if kind == "error" { c.destructive_color().into() } else { c.warning_color().into() };
                    iced::widget::container::Style { background: Some(Color { a: 0.15, ..color }.into()), text_color: Some(c.on_bg_color().into()), ..Default::default() }
                })),
        );
    }
    // As wide as the block it is about
    Some(col.width(Length::Fixed(block_code_width(t.code_columns) + 8.0)).into())
}

/// The goal at the caret, under the proof being edited, when the right sidebar is hidden
/// A claude or codex job's proofs belong to the cell its line is in: shown there only
pub fn fill_here(ws: &Workspace) -> bool {
    let (Some(_), Some((start, _)), Some(t)) = (&ws.fill.job, ws.fill.replace, &ws.theory) else { return false };
    let at = super::App::local_offset_pub(t, start);
    t.blocks.iter().find(|b| b.start <= t.cursor && t.cursor <= b.end).is_none_or(|b| b.start <= at && at <= b.end)
}

fn inline_goal<'a>(ws: Option<&'a Workspace>, table: &super::symbols::Table) -> Option<Element<'a, Message>> {
    let ws = ws?;
    if ws.output.is_none() && !fill_here(ws) {
        return None;
    }
    Some(
        widget::container(state_view(ws, table))
            .padding([6, 10])
            .width(Length::Fill)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(Color { a: 0.05, ..Color::from(theme.cosmic().accent_color()) }.into()),
                border: Border { radius: 6.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .into(),
    )
}

#[allow(dead_code)]
fn inline_goal_text<'a>(ws: Option<&'a Workspace>, table: &super::symbols::Table) -> Option<Element<'a, Message>> {
    let ws = ws?;
    let out = ws.output.as_ref()?;
    let msgs = out["messages"].as_array()?;
    let mut col = widget::column::with_capacity(msgs.len() + 1).spacing(4);
    if let Some(c) = out.get("command").filter(|c| c.is_object()) {
        col = col.push(widget::text::caption(format!("After {} ({})", c["name"].as_str().unwrap_or(""), c["status"].as_str().unwrap_or(""))));
    }
    if msgs.is_empty() {
        col = col.push(widget::text::caption("No goal here."));
    }
    for m in msgs {
        col = col.push(message_view(ws, m, table));
        for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
            col = col.push(
                widget::row::with_capacity(2)
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .push(widget::text(table.decode(sb).text).font(cosmic::font::mono()).size(z(13.0)).width(Length::Fill))
                    .push(widget::button::suggested("Insert").on_press(Message::InsertOutput(sb.to_string()))),
            );
        }
    }
    Some(
        widget::container(col)
            .padding([6, 10])
            .width(Length::Fill)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(Color { a: 0.08, ..Color::from(theme.cosmic().accent_color()) }.into()),
                border: Border { radius: 4.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .into(),
    )
}

fn literate<'a>(app: &'a App, t: &'a Theory, inline: Option<&'a Workspace>, beside: Option<(&'a Workspace, f32, f32)>, text_width: f32) -> Element<'a, Message> {
    let sp = spacing();
    let table = app.table(&app.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default());
    // With room, the goal sits beside the block it belongs to, level with it; the text keeps
    // its column on the left
    // Each item of the page: what it shows in the text column, then the goal's reserved column
    // (the goal itself is drawn by the page, past whatever is level with it)
    let side_at = |el: Element<'a, Message>, _goal: bool, _top: f32| -> Element<'a, Message> {
        match beside {
            Some((_, width, text_width)) => widget::row::with_capacity(2)
                .spacing(GOAL_GAP)
                .push(widget::container(el).width(Length::Fixed(text_width)).height(Length::Shrink))
                .push(widget::Space::new().width(Length::Fixed(width)))
                .height(Length::Shrink)
                .into(),
            None => widget::row::with_capacity(2)
                .push(widget::container(el).width(Length::Fill).height(Length::Shrink))
                .push(widget::Space::new().width(Length::Fixed(0.0)))
                .height(Length::Shrink)
                .into(),
        }
    };
    let mut anchor: Option<(usize, f32)> = None;
    let mut open_item: Option<usize> = None;
    let side = |el: Element<'a, Message>, goal: bool| side_at(el, goal, 0.0);
    let mut typo = typography(&theme::active());
    typo.size = z(17.0);
    let typo = Arc::new(typo);
    // (no spacing: the gaps between blocks are hover areas of their own)
    // (side margins as the room allows: small in a narrow window, where the text needs the width)
    let gutter = if beside.is_some() {
        f32::from_bits(PAGE_GUTTER.load(std::sync::atomic::Ordering::Relaxed)) as u16
    } else if view_width() > 0.0 && view_width() < z(900.0) {
        sp.space_s
    } else {
        sp.space_l
    };
    let mut col = widget::column::with_capacity(t.blocks.len() * 2).spacing(0).padding([sp.space_s, gutter]);
    let editing = t.block_editor.as_ref().map(|b| b.start);
    for (i, block) in t.blocks.iter().enumerate() {
        // Each block is one element of the page, whatever it shows, so that the open block's
        // editor keeps its place (and so its focus and caret) as things around it come and go
        let mut item = widget::column::with_capacity(4);
        // The open block shows as its editor (which may edit only its text, inside it)
        let opens_here = editing.is_some_and(|s| block.start <= s && s <= block.end) && t.blocks.get(i.wrapping_sub(1)).is_none_or(|p| !(p.start <= editing.unwrap_or(0) && editing.unwrap_or(0) <= p.end));
        let covered = t.block_editor.as_ref().is_some_and(|b| b.start < block.start && block.start < b.end);
        if opens_here || covered {
            if opens_here {
                if app.popout.is_some() {
                    item = item.push(side(banner("This proof is open in its own window.".into(), Some(("Bring it back", Message::PopIn))), false));
                } else {
                    let formal = t.block_editor.as_ref().is_some_and(|b| b.kind == Kind::Formal);
                    // (level with the proof's top, or with the construct at the caret in a long one)
                    let lines = t.block_editor.as_ref().map_or(0, |be| be.content.line_count());
                    let top = match goal_placement(true, lines) {
                        GoalPlacement::Construct => t
                            .block_editor
                            .as_ref()
                            .zip(t.block_editor.as_ref().and_then(|be| t.construct_at(be.start, be.end)))
                            .map_or(0.0, |(be, (cs, _))| CODE_PADDING.top + block_line(be, cs) as f32 * code_line_height()),
                        _ => 0.0,
                    };
                    if formal && beside.is_some() {
                        anchor = Some((i, top));
                    }
                    open_item = Some(i);
                    item = item.push(side_at(widget::container(block_editor_in(app.workspace.as_ref(), t, false, inline, &table)).id(selected_block_id()).into(), formal, top));
                }
            }
            // (its notes stay under it while it is open: nothing after it moves)
            if opens_here && let Some(ws) = app.workspace.as_ref() {
                for n in ws.notes.iter().filter(|n| {
                    n["start"].as_u64().is_some_and(|s| {
                        let s = super::App::local_offset_pub(t, s as usize);
                        block.start <= s && s < block.end.max(block.start + 1)
                    })
                }) {
                    item = item.push(side(note_card(ws, n), false));
                }
            }
            col = col.push(item);
            continue;
        }
        let current = block.start <= t.cursor && t.cursor <= block.end;
        // The block j and k selected, else the one the caret is in: outlined
        let marked = t.selected.map_or(block.start <= t.cursor && t.cursor < block.end, |s| s == i);
        let content: Element<'a, Message> = match block.kind {
            Kind::Header => {
                let src = table.decode(&t.source_text(block.start, block.end)).text;
                widget::column::with_capacity(2)
                    .push(
                        widget::text(src.lines().next().unwrap_or("").replacen("theory", "Theory", 1))
                            .size(z(26.0))
                            .font(cosmic::iced::Font { weight: cosmic::iced::font::Weight::Bold, ..cosmic::font::default() }),
                    )
                    .push(widget::text(src.lines().skip(1).map(str::trim).collect::<Vec<_>>().join(" ")).size(z(13.0)))
                    .into()
            }
            Kind::Heading(_) | Kind::Text => match t.docs.get(i).cloned().flatten() {
                Some(doc) => rich_text(doc, typo.clone()).into(),
                None => mono_block(table.decode(&t.source_text(block.start, block.end)).text, block.status),
            },
            Kind::Formal => {
                let mut c = widget::column::with_capacity(3).spacing(4).push(block_code(app.workspace.as_ref(), t, i, block, marked));
                if let Some(m) = messages_view(t, block, &table, current) {
                    c = c.push(m);
                }
                if current && let Some(g) = inline_goal(inline, &table) {
                    c = c.push(g);
                }
                c.into()
            }
            Kind::Comment => widget::text(table.decode(&t.source_text(block.start, block.end)).text).font(cosmic::font::mono()).size(z(12.0)).class(theme::Text::Default).into(),
            // (the theory's `end` is source, not reading)
            Kind::End => continue,
        };
        // The block j and k selected, found by its id to centre it; a prose block shows it with a bar
        let content: Element<'a, Message> = if t.selected == Some(i) && t.block_editor.is_none() {
            let bar = block.kind != Kind::Formal;
            widget::container(content)
                .id(selected_block_id())
                .padding(Padding { left: if bar { 8.0 } else { 0.0 }, ..Padding::ZERO })
                .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                    background: bar.then(|| Color { a: 0.07, ..Color::from(theme.cosmic().accent_color()) }.into()),
                    border: Border { radius: 4.0.into(), ..Default::default() },
                    ..Default::default()
                }))
                .into()
        } else {
            content
        };
        // The goal beside the selected proof when none is open (as beside the open one)
        if marked && editing.is_none() && block.kind == Kind::Formal && beside.is_some() {
            anchor = Some((i, 0.0));
        }
        // Formal blocks take their own clicks (the caret goes where the click lands)
        if block.kind == Kind::Formal && t.block_views.get(i).is_some_and(Option::is_some) {
            item = item.push(side(content, marked && editing.is_none()));
        } else {
            let area = widget::mouse_area(content).on_press(Message::EditBlock(i));
            let area = if matches!(block.kind, Kind::Heading(_) | Kind::Text) { area.on_move(move |p| Message::ProsePointer(i, p)) } else { area };
            item = item.push(side(area.into(), marked && editing.is_none() && block.kind == Kind::Formal));
        }
        // The selected block: what can be added after it, and Delete
        let selected_here = t.selected == Some(i) && editing.is_none() && block.kind != Kind::Header;
        if selected_here {
            item = item.push(side(block_actions(Some(i), true), false));
        }
        // Agents' notes on this block, under it
        if let Some(ws) = app.workspace.as_ref() {
            for n in ws.notes.iter().filter(|n| {
                n["start"].as_u64().is_some_and(|s| {
                    let s = super::App::local_offset_pub(t, s as usize);
                    block.start <= s && s < block.end.max(block.start + 1)
                })
            }) {
                item = item.push(side(note_card(ws, n), false));
            }
        }
        // The gap after it: hovered, it offers what can be added there
        if !selected_here && block.kind != Kind::End {
            let hovered = app.workspace.as_ref().is_some_and(|w| w.gap_hover == Some(i));
            let inner: Element<'a, Message> = if hovered { block_actions(Some(i), false) } else { widget::Space::new().into() };
            let gap = widget::mouse_area(widget::container(inner).height(Length::Fixed(z(20.0))).width(Length::Fill).center_y(Length::Fixed(z(20.0))))
                .on_enter(Message::GapHover(Some(i)))
                .on_exit(Message::GapLeave(i));
            item = item.push(side(gap.into(), false));
        } else {
            item = item.push(widget::Space::new().height(Length::Fixed(sp.space_xs as f32)));
        }
        col = col.push(item);
    }
    // At the end: add a block
    col = col.push(side(block_actions(None, false), false));
    // (room at the end, for a goal beside the last block to be scrolled into sight)
    col = col.push(widget::Space::new().height(Length::Fixed(if beside.is_some() { view_height() * 0.5 } else { 0.0 })));
    // Clicking outside every block finishes the one being edited
    // (its side margins included, so that the page is centred, the same room on either side)
    // (with the goal under the proof, the readable text column alone, centred)
    let page_width = beside.map_or(text_width, |(_, w, tw)| tw + w + GOAL_GAP) + 2.0 * gutter as f32;
    // A click no block takes (between them, beside them, below the end) finishes editing
    // With no goal to show beside the block being edited (prose, or a proof with no state yet),
    // that room holds what can be inserted there
    let palette = match (beside, open_item) {
        (Some((ws, width, _)), Some(i)) if anchor.is_none() || inline_goal(Some(ws), &table).is_none() => {
            anchor = Some((i, 0.0));
            Some(insert_palette(ws, t, &table, width))
        }
        _ => None,
    };
    let goal: Element<'a, Message> = match (beside, anchor) {
        _ if palette.is_some() => palette.unwrap_or_else(|| widget::Space::new().into()),
        (Some((ws, width, _)), Some(_)) => match inline_goal(Some(ws), &table) {
            Some(g) => widget::container(g)
                .width(Length::Fixed(width))
                .class(theme::Container::custom(|theme| iced::widget::container::Style {
                    background: Some(Color::from(theme.cosmic().bg_color()).into()),
                    border: Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                }))
                .into(),
            None => widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into(),
        },
        _ => widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into(),
    };
    let laid = super::gap::GoalLayer::new(col, goal, anchor, GOAL_GAP);
    let page = widget::container(widget::container(laid).max_width(page_width).width(Length::Fill)).center_x(Length::Fill);
    widget::mouse_area(widget::scrollable(page).id(literate_scroll_id()).height(Length::Fill)).on_press(Message::CloseBlock).into()
}

/// Where a scroll view is (the source view's, the proof window's), to keep the caret in sight there
struct MeasureSource {
    id: widget::Id,
    view: Option<(iced::Rectangle, iced::Rectangle, iced::Vector)>,
}

impl iced::advanced::widget::Operation<(iced::Rectangle, iced::Rectangle, iced::Vector)> for MeasureSource {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<(iced::Rectangle, iced::Rectangle, iced::Vector)>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&widget::Id>,
        bounds: iced::Rectangle,
        content_bounds: iced::Rectangle,
        translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&self.id) {
            self.view = Some((bounds, content_bounds, translation));
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<(iced::Rectangle, iced::Rectangle, iced::Vector)> {
        match self.view {
            Some(v) => iced::advanced::widget::operation::Outcome::Some(v),
            None => iced::advanced::widget::operation::Outcome::None,
        }
    }
}

/// Scroll the source view so that the caret's line is in sight
pub fn keep_source_caret(line: usize) -> cosmic::app::Task<Message> {
    keep_caret_in(source_scroll_id(), line, source_line_height(), spacing().space_s as f32)
}

/// Scroll a scroll view holding an editor at its full height so that the caret's line is in sight
pub fn keep_caret_in(id: widget::Id, line: usize, lh: f32, top: f32) -> cosmic::app::Task<Message> {
    iced::advanced::widget::operate(MeasureSource { id: id.clone(), view: None }).then(move |(view, content, translation)| {
        let y = top + line as f32 * lh;
        let margin = lh * 3.0;
        let target = if y < translation.y + margin {
            Some(y - margin)
        } else if y + lh > translation.y + view.height - margin {
            Some(y + lh + margin - view.height)
        } else {
            None
        };
        match target {
            Some(t) => iced::widget::scrollable::scroll_to(
                id.clone(),
                iced::widget::scrollable::AbsoluteOffset { x: None, y: Some(t.clamp(0.0, (content.height - view.height).max(0.0))) },
            ),
            None => cosmic::app::Task::none(),
        }
    })
}

pub fn selected_block_id() -> widget::Id {
    widget::Id::new("isabellum-selected-block")
}

/// Where the selected block and the literate view's viewport are, for centring it
#[derive(Default)]
struct MeasureSelected {
    block: Option<iced::Rectangle>,
    view: Option<(iced::Rectangle, iced::Rectangle)>,
}

type Measured = (iced::Rectangle, iced::Rectangle, iced::Rectangle);

impl iced::advanced::widget::Operation<Measured> for MeasureSelected {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<Measured>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
        if id == Some(&selected_block_id()) {
            self.block = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&widget::Id>,
        bounds: iced::Rectangle,
        content_bounds: iced::Rectangle,
        _translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&literate_scroll_id()) {
            self.view = Some((bounds, content_bounds));
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<Measured> {
        match (self.block, self.view) {
            (Some(b), Some((v, c))) => iced::advanced::widget::operation::Outcome::Some((b, v, c)),
            _ => iced::advanced::widget::operation::Outcome::None,
        }
    }
}

/// Scroll the literate view so that the selected block is in the middle (a block taller
/// than the view starts at its top)
pub fn centre_selected() -> cosmic::app::Task<Message> {
    iced::advanced::widget::operate(MeasureSelected::default()).then(|(block, view, content)| {
        let top = block.y - content.y;
        let offset = if block.height > view.height * 0.9 { top - 12.0 } else { top + block.height / 2.0 - view.height / 2.0 };
        let offset = offset.clamp(0.0, (content.height - view.height).max(0.0));
        iced::widget::scrollable::scroll_to(literate_scroll_id(), iced::widget::scrollable::AbsoluteOffset { x: None, y: Some(offset) })
    })
}

/// What editing the open block means, for the status bar: vim's mode, the block's checking, how
/// to finish
fn editing_status(app: &App) -> Option<String> {
    let t = app.workspace.as_ref()?.theory.as_ref()?;
    let be = t.block_editor.as_ref().filter(|_| t.mode == Mode::Literate)?;
    let mut parts = Vec::new();
    if let Some(m) = vim_mode_now() {
        parts.push(format!("-- {} --", m.label()));
    }
    if be.wrap.is_none() {
        parts.push(status_caption(t.status_in(be.start, be.end)).to_string());
    }
    if be.preview.is_some() {
        parts.push("preview follows".to_string());
    }
    parts.push("Esc or click outside to finish".to_string());
    Some(parts.join(" · "))
}

fn status_caption(status: Option<Status>) -> &'static str {
    match status {
        Some(Status::Failed) => "✗ failing",
        Some(Status::Running) => "… checking",
        Some(Status::Unprocessed) => "… waiting to be checked",
        Some(Status::Warned) => "⚠ checked with warnings",
        Some(Status::Finished) => "✓ checked",
        None => "",
    }
}

fn block_editor_in<'a>(ws: Option<&'a Workspace>, t: &'a Theory, window: bool, inline: Option<&'a Workspace>, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    let Some(be) = &t.block_editor else { return widget::Space::new().into() };
    let vim_mode = vim_mode_now();
    // The block's source, coloured as the source view colours it, without a box
    // Looks as the block did shown, outlined as the current one, so opening it moves nothing
    let columns = be.content.text().lines().map(|l| l.chars().count()).max().unwrap_or(0);
    if be.wrap.is_some() && !window {
        return prose_editor(t, be, vim_mode);
    }
    let editor = iced::widget::text_editor(&be.content)
        .id(block_editor_id())
        .font(cosmic::font::mono())
        .size(z(13.0))
        .line_height(iced::widget::text::LineHeight::Relative(CODE_LINE))
        .padding(CODE_PADDING)
        .on_action(Message::BlockAction)
        .key_binding(key_bindings(!t.completions.is_empty(), vim_mode))
        .style(code_style(true))
        .highlight_with::<super::editor::Highlighter>(t.block_highlight.themed(), highlight_format);
    // A long proof in a narrow window: the goal opens a gap in the editor just after the construct
    // the caret is in (the proof stays one editor; what follows moves down, nothing is covered)
    let lines = be.content.line_count();
    let after = (!window && be.kind == Kind::Formal && inline.is_some() && goal_placement(false, lines) == GoalPlacement::AfterConstruct)
        .then(|| t.construct_at(be.start, be.end))
        .flatten()
        .and_then(|(cs, ce)| {
            // indented as the construct is
            let first = be.shown_text.split('\n').nth(block_line(be, cs)).unwrap_or("");
            let indent = first.chars().take_while(|c| *c == ' ').count();
            Some((block_line(be, ce), indent, inline_goal(inline, table)?))
        });
    let floated = after.is_some();
    // The construct the proof state is for, tinted
    let band = (be.kind == Kind::Formal).then(|| t.construct_at(be.start, be.end)).flatten().map(|(cs, ce)| {
        let (l0, l1) = (block_line(be, cs), block_line(be, ce.saturating_sub(1).max(cs)));
        (CODE_PADDING.top + l0 as f32 * code_line_height(), CODE_PADDING.top + (l1 + 1) as f32 * code_line_height())
    });
    let tint = construct_tint();
    let banded = move |g: super::gap::Gap<'a, Message, cosmic::Theme, cosmic::Renderer>| match band {
        Some((from, to)) => g.band(from, to, tint),
        None => g,
    };
    let editor: Element<'_, Message> = if window {
        // At its full height in a scroll view that keeps the caret in sight, so that where its
        // lines are drawn is known for the tint
        let code = banded(super::gap::Gap::new(editor.height(Length::Shrink), widget::Space::new().height(Length::Fixed(0.0)), 0.0));
        widget::scrollable(code).id(popout_scroll_id()).height(Length::Fill).into()
    } else {
        let width = block_code_width(t.code_columns.max(columns));
        let code = hoverable(ws, super::HoverTarget::Block, CODE_PADDING, editor.width(width).into());
        // (always in a gap, empty when there is nothing to put there: the editor keeps its state)
        let code: Element<'_, Message> = match after {
            Some((line, indent, goal)) => {
                // Set off from the code: indented as its construct, an accent rule on its left,
                // its own background
                let rule = widget::container(widget::Space::new())
                    .width(Length::Fixed(3.0))
                    .height(Length::Fill)
                    .class(theme::Container::custom(|theme| iced::widget::container::Style {
                        background: Some(Color::from(theme.cosmic().accent_color()).into()),
                        border: Border { radius: 2.0.into(), ..Default::default() },
                        ..Default::default()
                    }));
                let card = widget::container(goal).width(Length::Fill).class(theme::Container::custom(|theme| iced::widget::container::Style {
                    background: Some(Color::from(theme.cosmic().bg_color()).into()),
                    border: Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                }));
                let inset = widget::row::with_capacity(2).spacing(6).push(rule).push(card).height(Length::Shrink);
                let left = CODE_PADDING.left + indent as f32 * z(13.0) * 0.6;
                banded(super::gap::Gap::new(
                    code,
                    widget::container(inset).padding(Padding { top: 6.0, bottom: 8.0, left, right: 6.0 }).width(Length::Fixed(width)),
                    CODE_PADDING.top + (line + 1) as f32 * code_line_height(),
                ))
                .into()
            }
            None => banded(super::gap::Gap::new(code, widget::Space::new().height(Length::Fixed(0.0)), 0.0)).into(),
        };
        // Pop out: a small button floating at the top right of the code, as in the source view
        let pop: Element<'_, Message> = if be.kind == Kind::Formal {
            widget::button::icon(widget::icon::from_name("window-pop-out-symbolic"))
                .tooltip("Pop out (the proof in a window of its own)")
                .on_press(Message::PopOut)
                .into()
        } else {
            widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into()
        };
        let code: Element<'_, Message> = super::gap::Beside::new(code, pop, iced::Vector::new(4.0, 0.0)).right().fixed().into();
        framed(code, t.shown_status(be.start, be.end, t.status_in(be.start, be.end).unwrap_or(Status::Finished)), true)
    };
    let mut col = widget::column::with_capacity(6).spacing(4).push(editor);
    if let Some(bar) = completion_bar(t) {
        col = col.push(bar);
    }
    // Live feedback: the block's checking status and messages, and the goal at the caret
    let status = t.status_in(be.start, be.end);
    // (errors and warnings: the rest shows with the proof state)
    let all = t.messages_in(be.start, be.end);
    if all.iter().any(|m| t.expected_error(m)) {
        col = col.push(widget::text::caption("Unfinished while the tool runs: pick a proof it finds"));
    }
    let msgs: Vec<_> = all.into_iter().filter(|m| (m.kind == "error" || m.kind == "warning") && !t.expected_error(m)).collect();
    for m in msgs.into_iter().take(6) {
        let kind = m.kind.clone();
        let line = super::ot::line_col(&t.local, m.start).0;
        col = col.push(
            widget::container(symbol_text(&format!("{kind} (line {line}): {}", message_summary(&m.text)), table, z(12.0)))
                .padding([2, 8])
                .width(Length::Fill)
                .class(theme::Container::custom(move |theme| {
                    let c = theme.cosmic();
                    let color: Color = if kind == "error" { c.destructive_color().into() } else { c.warning_color().into() };
                    iced::widget::container::Style { background: Some(Color { a: 0.15, ..color }.into()), ..Default::default() }
                })),
        );
    }
    // Prose has no proof state
    if !window && !floated && be.kind == Kind::Formal && let Some(g) = inline_goal(inline, table) {
        col = col.push(g);
    }
    if let Some(doc) = &be.preview {
        let mut typo = typography(&theme::active());
        typo.size = z(17.0);
        col = col.push(rich_text(doc.clone(), Arc::new(typo)));
    }
    let controls = widget::row::with_capacity(4)
        .spacing(sp.space_s)
        .align_y(Alignment::Center)
        .push(widget::text::caption(vim_mode.map(|m| format!("-- {} --", m.label())).unwrap_or_default()))
        .push(widget::text::caption(status_caption(status)))
        .push(widget::text::caption(if be.preview.is_some() { "· preview follows · Esc or click outside to finish" } else { "· Esc or click outside to finish" }).width(Length::Fill));
    // (on the page, what the controls say is in the status bar at the window's foot: a row
    // under the open block would move everything after it as the open block changes)
    if window {
        return col.push(controls).into();
    }
    let _ = controls;
    col.width(Length::Fixed(block_code_width(t.code_columns.max(columns)) + 8.0)).into()
}

/// A heading or text being edited: its text alone, set in the font, size and width it reads in,
/// on the page rather than in a box; code inside it (antiquotations) keeps code colours, and
/// what only renders (maths, antiquotations) shows rendered underneath
fn prose_editor<'a>(t: &'a Theory, be: &'a super::editor::BlockEditor, vim_mode: Option<super::vim::Mode>) -> Element<'a, Message> {
    let sp = spacing();
    let (scale, bold) = match be.kind {
        Kind::Heading(level) => (match level { 1 => 1.55, 2 => 1.32, 3 => 1.16, 4 => 1.05, _ => 1.0 }, true),
        _ => (1.0, false),
    };
    let serif = cosmic::iced::Font {
        weight: if bold { cosmic::iced::font::Weight::Bold } else { cosmic::iced::font::Weight::Normal },
        ..cosmic::iced::Font::with_name("NewComputerModern")
    };
    let editor = iced::widget::text_editor(&be.content)
        .id(block_editor_id())
        .placeholder(if bold { "Section title" } else { "Text" })
        .font(serif)
        .size(z(17.0) * scale)
        .line_height(iced::widget::text::LineHeight::Relative(if bold { 1.25 } else { 1.5 }))
        .padding(Padding { left: 0.0, right: 0.0, top: 2.0, bottom: 2.0 })
        .on_action(Message::BlockAction)
        .key_binding(key_bindings(!t.completions.is_empty(), vim_mode))
        .style(|theme: &cosmic::Theme, _status| {
            let c = theme.cosmic();
            iced::widget::text_editor::Style {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                placeholder: Color { a: 0.5, ..Color::from(c.on_bg_color()) },
                value: c.on_bg_color().into(),
                selection: Color { a: 0.32, ..Color::from(c.accent.base) },
            }
        })
        .highlight_with::<super::editor::Highlighter>(t.block_highlight.themed(), prose_format);
    // Exactly where the text was, on the page; only the caret says it is being edited. The block's
    // other paragraphs stay as they read, above and below the one being edited
    let (before, after) = t
        .blocks
        .iter()
        .find(|b| b.start <= be.start && be.start <= b.end)
        .and_then(|b| b.body)
        .map_or((String::new(), String::new()), |(bs, bend)| (t.source_text(bs, be.start), t.source_text(be.end, bend)));
    let rendered = |src: &str| -> Option<Element<'a, Message>> {
        (!src.trim().is_empty()).then(|| {
            let mut typo = typography(&theme::active());
            typo.size = z(17.0);
            let md = super::literate::to_markdown(src.trim(), &super::symbols::Table::fallback());
            rich_text(Arc::new(super::markdown::parse(&md)), Arc::new(typo)).into()
        })
    };
    // (both places always there, empty or not: the editor keeps its place, and so its focus)
    let mut col = widget::column::with_capacity(5).spacing(4);
    col = col.push(rendered(&before).unwrap_or_else(|| widget::Space::new().height(Length::Fixed(0.0)).into()));
    col = col.push(editor);
    col = col.push(rendered(&after).unwrap_or_else(|| widget::Space::new().height(Length::Fixed(0.0)).into()));
    if let Some(bar) = completion_bar(t) {
        col = col.push(bar);
    }
    // The maths or antiquotation the caret is in, set as it will read
    if let Some(snippet) = super::literate::snippet_at(&be.shown(), {
        let shown = be.shown();
        super::editor::LineIndex::new(&shown).offset(&shown, be.content.cursor().position)
    }) {
        let md = super::literate::to_markdown(&snippet, &super::symbols::Table::fallback());
        let mut typo = typography(&theme::active());
        typo.size = z(17.0);
        col = col.push(
            widget::container(rich_text(Arc::new(super::markdown::parse(&md)), Arc::new(typo)))
                .padding([2, 8])
                .class(theme::Container::custom(|theme| iced::widget::container::Style {
                    background: Some(Color { a: 0.05, ..Color::from(theme.cosmic().accent_color()) }.into()),
                    border: Border { radius: 4.0.into(), ..Default::default() },
                    ..Default::default()
                })),
        );
    }
    let _ = (sp, vim_mode);
    col.into()
}

/// Prose being edited: plain text in the editor's own font; code inside it in mono and colour
pub fn prose_format(hl: &super::editor::Hl, theme: &cosmic::Theme) -> iced::advanced::text::highlighter::Format<cosmic::iced::Font> {
    use super::editor::Hl;
    match hl {
        // (checking status would grey the whole text: prose shows none)
        Hl::Unprocessed | Hl::Running | Hl::Comment => iced::advanced::text::highlighter::Format { color: None, font: None },
        // The prover marks a document text's body itself (as comment, raw or cartouche text):
        // that is the prose, set as prose
        Hl::Sem(super::editor::Sem::Comment | super::editor::Sem::Raw | super::editor::Sem::Inner) => {
            iced::advanced::text::highlighter::Format { color: None, font: None }
        }
        Hl::Sem(_) | Hl::Keyword => {
            let mut f = highlight_format(hl, theme);
            f.font = f.font.or(Some(cosmic::font::mono()));
            f
        }
        _ => highlight_format(hl, theme),
    }
}

/// A definition in a window of its own (Alt+click): its text, and a way to it in the main window
pub fn reference_window(app: &App, id: iced::window::Id) -> Element<'_, Message> {
    let sp = spacing();
    let Some((_, r)) = app.references.iter().find(|(w, _)| *w == id) else { return widget::text("").into() };
    let mut header = widget::row::with_capacity(4).padding([sp.space_xxs, sp.space_s]).spacing(sp.space_xxs).align_y(Alignment::Center);
    if !r.back.is_empty() {
        header = header.push(widget::button::icon(widget::icon::from_name("go-previous-symbolic")).tooltip("Back").on_press(Message::RefBack(id)));
    }
    header = header
        .push(widget::text::heading(r.title.clone()).width(Length::Fill))
        .push(widget::text::caption("Ctrl+click: go there · Alt+click: new window"))
        .push(widget::button::text("Open here").on_press(Message::RefOpenHere(id)));
    // The whole theory, read-only, as the source view draws it: the definition tinted, tooltips
    // under the pointer
    let content = widget::responsive(move |size| {
        let t = &r.theory;
        let pad = Padding::from(sp.space_s as f32);
        let code = iced::widget::text_editor(&t.source)
            .font(cosmic::font::mono())
            .size(z(13.0))
            .line_height(iced::widget::text::LineHeight::Relative(CODE_LINE))
            .padding(pad)
            .height(Length::Shrink)
            .key_binding(|press| {
                use iced::widget::text_editor::Binding as B;
                B::from_key_press(press).filter(|b| matches!(b, B::Copy | B::Move(_) | B::Select(_) | B::SelectWord | B::SelectLine | B::SelectAll | B::Unfocus))
            })
            .on_action(move |a| Message::RefAction(id, a))
            .style(|theme: &cosmic::Theme, _status| {
                let c = theme.cosmic();
                iced::widget::text_editor::Style {
                    background: Color::TRANSPARENT.into(),
                    border: Border::default(),
                    placeholder: Color { a: 0.5, ..Color::from(c.on_bg_color()) },
                    value: c.on_bg_color().into(),
                    selection: Color { a: 0.32, ..Color::from(c.accent.base) },
                }
            })
            .highlight_with::<super::editor::Highlighter>(t.highlight.themed(), highlight_format);
        let (l0, l1) = reference_lines(r);
        let row_top = reference_rows(r, size.width - pad.left - pad.right);
        let band = construct_tint();
        let gap = super::gap::Gap::new(code, widget::Space::new().height(Length::Fixed(0.0)), 0.0).band(pad.top + row_top(l0), pad.top + row_top(l1 + 1), band);
        let el: Element<'_, Message> =
            widget::scrollable(hoverable(app.workspace.as_ref(), super::HoverTarget::Ref(id), pad, gap.into())).id(reference_scroll_id(id)).height(Length::Fill).into();
        el
    });
    widget::container(widget::column::with_capacity(2).push(header).push(content))
        .width(Length::Fill)
        .height(Length::Fill)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color { a: 1.0, ..Color::from(theme.cosmic().bg_color()) }.into()),
            text_color: Some(theme.cosmic().on_bg_color().into()),
            ..Default::default()
        }))
        .into()
}

/// The lines of a reference's definition: from its line to the next blank one (at most 40)
fn reference_lines(r: &super::Reference) -> (usize, usize) {
    let lines: Vec<&str> = r.theory.decoded.text.split('\n').collect();
    let mut end = r.line.min(lines.len().saturating_sub(1));
    while end + 1 < lines.len() && !lines[end + 1].trim().is_empty() && end - r.line < 40 {
        end += 1;
    }
    (r.line, end)
}

/// Where a reference's lines are drawn, below its text's top, wrapped at this width
fn reference_rows(r: &super::Reference, width: f32) -> impl Fn(usize) -> f32 {
    let per_row = (width / (z(13.0) * 0.6)).floor().max(20.0) as usize;
    let rows: Vec<usize> = r.theory.decoded.text.split('\n').map(|l| l.chars().count().div_ceil(per_row).max(1)).collect();
    move |line| rows.iter().take(line).sum::<usize>() as f32 * code_line_height()
}

pub fn reference_scroll_id(id: iced::window::Id) -> widget::Id {
    widget::Id::new(format!("isabellum-reference-{id:?}"))
}

/// Scroll a reference window to its definition, a few lines above it in sight
pub fn scroll_reference(id: iced::window::Id, r: &super::Reference) -> cosmic::app::Task<Message> {
    // (the window's width is not known here: wrapping is estimated at its usual width)
    let y = reference_rows(r, z(780.0))(r.line) - 3.0 * code_line_height();
    iced::widget::scrollable::scroll_to(reference_scroll_id(id), iced::widget::scrollable::AbsoluteOffset { x: None, y: Some(y.max(0.0)) })
}

/// The proof workspace in its own window (design C4): the block's source, the
/// goal at its caret and Sledgehammer, on the same buffer as the main window
pub fn proof_window(app: &App) -> Element<'_, Message> {
    let sp = spacing();
    let Some(ws) = &app.workspace else { return widget::text("No session").into() };
    let Some(t) = &ws.theory else { return widget::text("No theory").into() };
    if t.block_editor.is_none() {
        return widget::container(widget::text::body("The proof was closed in the main window."))
            .center(Length::Fill)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(Color::from(theme.cosmic().bg_color()).into()),
                ..Default::default()
            }))
            .into();
    }
    // As the main window: the state beside the proof when there is room, else under it
    let content = widget::responsive(move |size| {
        let table = app.table(&ws.machine);
        let state = widget::container(widget::scrollable(state_view(ws, &table))).padding(sp.space_s).class(theme::Container::Card);
        let editor = widget::container(block_editor_in(Some(ws), t, true, None, &table));
        let wide = size.width >= z(900.0);
        let el: Element<'_, Message> = if wide {
            widget::row::with_capacity(2)
                .spacing(sp.space_s)
                .padding(sp.space_s)
                .push(editor.width(Length::FillPortion(3)).height(Length::Fill))
                .push(state.width(Length::FillPortion(2)).height(Length::Fill))
                .into()
        } else {
            widget::column::with_capacity(2)
                .spacing(sp.space_s)
                .padding(sp.space_s)
                .push(editor.height(Length::FillPortion(3)).width(Length::Fill))
                .push(state.height(Length::FillPortion(2)).width(Length::Fill))
                .into()
        };
        el
    });
    // An opaque window: its own header and background, whatever the theme's transparency
    let header = widget::row::with_capacity(3)
        .padding([sp.space_xxs, sp.space_s])
        .align_y(Alignment::Center)
        .push(widget::text::heading(format!("Proof — {}", file_name(&t.path))).width(Length::Fill))
        .push(widget::button::text("Bring back").on_press(Message::PopIn));
    widget::container(widget::column::with_capacity(2).push(header).push(content))
        .width(Length::Fill)
        .height(Length::Fill)
        .class(theme::Container::custom(|theme| {
            let cosmic = theme.cosmic();
            let on: Color = cosmic.on_bg_color().into();
            iced::widget::container::Style {
                background: Some(Color { a: 1.0, ..Color::from(cosmic.bg_color()) }.into()),
                text_color: Some(Color { a: 1.0, ..on }),
                icon_color: Some(Color { a: 1.0, ..on }),
                ..Default::default()
            }
        }))
        .into()
}

fn key_bindings(completing: bool, vim: Option<super::vim::Mode>) -> impl Fn(text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    move |press| key_binding(press, completing, vim)
}

fn key_binding(press: text_editor::KeyPress, completing: bool, vim: Option<super::vim::Mode>) -> Option<text_editor::Binding<Message>> {
    use cosmic::iced::keyboard::Key;
    use cosmic::iced::keyboard::key::Named;
    let m = press.modifiers;
    if m.command() && matches!(press.key.as_ref(), Key::Character("=" | "+" | "-" | "0")) {
        return None;
    }
    // Vim: outside insert mode every key goes to the vim machine
    if let Some(mode) = vim {
        let custom = |k: &str, ctrl: bool| Some(text_editor::Binding::Custom(Message::Vim(k.to_string(), ctrl)));
        // Copy, cut and paste stay the desktop's, in every mode
        if m.control() {
            match press.key.as_ref() {
                Key::Character("c") => return Some(text_editor::Binding::Custom(Message::CopySelection(false))),
                Key::Character("x") => return Some(text_editor::Binding::Custom(Message::CopySelection(true))),
                Key::Character("v") => return Some(text_editor::Binding::Paste),
                _ => {}
            }
        }
        if mode != super::vim::Mode::Insert {
            return match press.key.as_ref() {
                Key::Named(Named::Escape) => custom("Escape", false),
                Key::Named(Named::Enter) => custom("Enter", false),
                Key::Named(Named::Backspace) => custom("Backspace", false),
                Key::Named(Named::ArrowLeft) => custom("h", false),
                Key::Named(Named::ArrowRight) => custom("l", false),
                Key::Named(Named::ArrowUp) => custom("k", false),
                Key::Named(Named::ArrowDown) => custom("j", false),
                Key::Character(c) if m.control() => custom(c, true),
                _ => match press.text.as_deref() {
                    Some(t) if !t.is_empty() && !m.control() && !m.alt() => custom(t, false),
                    _ => None,
                },
            };
        }
        if let Key::Named(Named::Escape) = press.key.as_ref() {
            return custom("Escape", false);
        }
    }
    match press.key.as_ref() {
        Key::Character("c") if m.control() && !m.shift() => Some(text_editor::Binding::Custom(Message::CopySelection(false))),
        Key::Character("x") if m.control() && !m.shift() => Some(text_editor::Binding::Custom(Message::CopySelection(true))),
        Key::Named(Named::Tab) if completing && !m.shift() => Some(text_editor::Binding::Custom(Message::Complete(0))),
        // Tab indents, as in any code editor, instead of moving the focus
        Key::Named(Named::Tab) if m.shift() => Some(text_editor::Binding::Custom(Message::Unindent)),
        Key::Named(Named::Tab) => Some(text_editor::Binding::Custom(Message::InsertSymbol("  ".into()))),
        Key::Character("z") if m.command() && m.shift() => Some(text_editor::Binding::Custom(Message::Redo)),
        Key::Character("z") if m.command() => Some(text_editor::Binding::Custom(Message::Undo)),
        Key::Character("y") if m.command() => Some(text_editor::Binding::Custom(Message::Redo)),
        Key::Character("s") if m.command() => Some(text_editor::Binding::Custom(Message::Save)),
        // Zoom is the window's: decline it, so the window-wide shortcut sees it
        Key::Character("=" | "+" | "-" | "0") if m.command() => None,
        Key::Named(Named::Escape) => Some(text_editor::Binding::Custom(Message::CloseBlock)),
        // Up and down run on past a block's ends into its neighbours, as j and k do
        Key::Named(Named::ArrowUp) if !m.shift() && !m.control() && !m.alt() => Some(text_editor::Binding::Custom(Message::LineMove(false))),
        Key::Named(Named::ArrowDown) if !m.shift() && !m.control() && !m.alt() => Some(text_editor::Binding::Custom(Message::LineMove(true))),
        Key::Named(Named::Enter) if !m.shift() && !m.command() && !m.alt() => Some(text_editor::Binding::Custom(Message::Newline)),
        _ => text_editor::Binding::from_key_press(press),
    }
}

fn completion_bar(t: &Theory) -> Option<Element<'_, Message>> {
    if t.completions.is_empty() {
        return None;
    }
    let mut row = widget::row::with_capacity(t.completions.len() + 1).spacing(4).align_y(Alignment::Center);
    row = row.push(widget::text::caption("Complete:"));
    for (i, c) in t.completions.iter().enumerate() {
        let label = if i == 0 { format!("{}  ⇥", c.label) } else { c.label.clone() };
        row = row.push(
            widget::button::text(label)
                .class(if i == 0 { theme::Button::Suggested } else { theme::Button::Text })
                .on_press(Message::Complete(i)),
        );
    }
    Some(widget::scrollable::horizontal(row).into())
}

/// The source view's width for lines this long: their measure, within limits (longer lines wrap)
pub fn source_width(columns: usize) -> f32 {
    (columns.clamp(60, 100) as f32 + 4.0) * z(14.0) * 0.6 + 2.0 * spacing().space_s as f32
}

/// Where the proof state goes relative to the proof it belongs to
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalPlacement {
    /// Beside the text, at the top of what is in view
    Pinned,
    /// Beside the text, level with the proof's top
    Proof,
    /// Beside the text, level with the construct (have, show, case, ... or the step) at the caret
    Construct,
    /// Under the whole proof
    UnderProof,
    /// Just after the construct at the caret
    AfterConstruct,
}

static VIEW_HEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static PLACEMENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn set_view_height(h: f32) {
    VIEW_HEIGHT.store(h.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn view_height() -> f32 {
    f32::from_bits(VIEW_HEIGHT.load(std::sync::atomic::Ordering::Relaxed)).max(400.0)
}

fn set_placement_override(p: Option<GoalPlacement>) {
    let v = match p {
        None => 0,
        Some(GoalPlacement::Pinned) => 1,
        Some(GoalPlacement::Proof) => 2,
        Some(GoalPlacement::Construct) => 3,
        Some(GoalPlacement::UnderProof) => 4,
        Some(GoalPlacement::AfterConstruct) => 5,
    };
    PLACEMENT.store(v, std::sync::atomic::Ordering::Relaxed);
}

static WIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether the last layout had room for the goal beside the text
pub fn last_wide() -> bool {
    WIDE.load(std::sync::atomic::Ordering::Relaxed)
}

/// The rule: a proof that fits comfortably in view keeps its goal with the whole proof (level
/// with its top beside it, or under it); a longer one keeps it with the construct at the caret,
/// so that it stays near (level with it beside, or just after it)
pub fn goal_placement(wide: bool, proof_lines: usize) -> GoalPlacement {
    match PLACEMENT.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return GoalPlacement::Pinned,
        2 => return GoalPlacement::Proof,
        3 => return GoalPlacement::Construct,
        4 => return GoalPlacement::UnderProof,
        5 => return GoalPlacement::AfterConstruct,
        _ => {}
    }
    let height = proof_lines as f32 * code_line_height();
    match wide {
        // (level with the construct, moving only as far as it must to stay in sight)
        true => GoalPlacement::Construct,
        false if height <= view_height() * 0.35 => GoalPlacement::UnderProof,
        false => GoalPlacement::AfterConstruct,
    }
}

/// What the caret is in, for the insert palette: maths or a term (symbols), prose (text
/// templates), or a proof (Isar)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsertContext {
    Symbols,
    Prose,
    Proof,
}

pub fn insert_context(t: &Theory) -> Option<InsertContext> {
    let be = t.block_editor.as_ref()?;
    // (the source as decoded, which the editor shows one character for one: maths shown set
    // still has its `$` here)
    let before: String = be.decoded.text.chars().take(be.caret()).collect();
    if be.wrap.is_some() {
        // (an unclosed `$` or `\(` before the caret: inside maths)
        let dollars = before.matches('$').count() - before.matches("\\$").count();
        let parens = before.matches("\\(").count() > before.matches("\\)").count();
        return Some(if dollars % 2 == 1 || parens { InsertContext::Symbols } else { InsertContext::Prose });
    }
    if be.kind != Kind::Formal {
        return None;
    }
    // (inside a quoted term)
    Some(if before.matches('"').count() % 2 == 1 { InsertContext::Symbols } else { InsertContext::Proof })
}

const COMMON_SYMBOLS: &[&str] = &[
    "∧", "∨", "¬", "⟶", "⟷", "∀", "∃", "⟹", "⇒", "→", "↦", "∈", "∉", "⊆", "⊂", "∪", "∩", "∅", "≤", "≥", "≠", "≡", "×", "∘", "λ", "α",
    "β", "γ", "δ", "σ", "τ", "φ", "ψ", "ω", "⦇", "⦈", "⟦", "⟧", "‹", "›", "⇩", "⇧",
];

const PROSE_TEMPLATES: &[(&str, &str)] = &[
    ("term", "@{term \"\"}"),
    ("theorem", "@{thm }"),
    ("constant", "@{const }"),
    ("type", "@{typ \"\"}"),
    ("maths", "$ $"),
    ("emphasis", "\\<^emph>‹›"),
    ("bold", "\\<^bold>‹›"),
    ("citation", "\\cite{}"),
    ("list item", "\\<^item> "),
    ("cartouche", "‹›"),
    ("footnote", "\\footnote{}"),
];

const PROOF_TEMPLATES: &[(&str, &str)] = &[
    ("have", "have \"\"\n  "),
    ("show ?thesis", "show ?thesis\n  "),
    ("proof … qed", "proof -\n  \nqed"),
    ("using", "using "),
    ("unfolding", "unfolding "),
    ("by simp", "by simp"),
    ("by auto", "by auto"),
    ("sledgehammer", "sledgehammer"),
    ("claude", "claude"),
    ("nitpick", "nitpick"),
    ("quickcheck", "quickcheck"),
];

/// Beside the block being edited, when there is no goal: what can be inserted at the caret, by
/// context (symbols in maths or a term, templates in prose, Isar in a proof), searchable
fn insert_palette<'a>(ws: &'a Workspace, t: &'a Theory, table: &super::symbols::Table, width: f32) -> Element<'a, Message> {
    let sp = spacing();
    let context = insert_context(t).unwrap_or(InsertContext::Prose);
    let query = ws.palette_query.trim().to_lowercase();
    let mut col = widget::column::with_capacity(4).spacing(sp.space_xs);
    col = col.push(widget::text::caption(match context {
        InsertContext::Symbols => "Insert a symbol",
        InsertContext::Prose => "Insert in the text",
        InsertContext::Proof => "Insert in the proof",
    }));
    col = col.push(widget::text_input("Search", &ws.palette_query).on_input(Message::PaletteQuery));
    let mut items: Vec<(String, String)> = Vec::new();
    match context {
        InsertContext::Symbols if !query.is_empty() => {
            for (c, sym, how) in table.search(&query, 40) {
                items.push((format!("{c}  {}", if how.is_empty() { sym.clone() } else { format!("{sym} {how}") }), c.to_string()));
            }
        }
        InsertContext::Symbols => {
            for c in COMMON_SYMBOLS {
                items.push((c.to_string(), c.to_string()));
            }
        }
        InsertContext::Prose | InsertContext::Proof => {
            let list = if context == InsertContext::Prose { PROSE_TEMPLATES } else { PROOF_TEMPLATES };
            for (label, text) in list.iter().filter(|(l, _)| query.is_empty() || l.contains(&query)) {
                items.push((label.to_string(), text.to_string()));
            }
            // (and symbols matching the search)
            if !query.is_empty() {
                for (c, sym, _) in table.search(&query, 12) {
                    items.push((format!("{c}  {sym}"), c.to_string()));
                }
            }
        }
    }
    let buttons: Vec<Element<'a, Message>> =
        items.into_iter().map(|(label, text)| widget::button::text(label).class(theme::Button::Standard).on_press(Message::InsertSymbol(text)).into()).collect();
    col = col.push(widget::flex_row(buttons).row_spacing(sp.space_xxs).column_spacing(sp.space_xxs));
    widget::container(col)
        .padding(sp.space_s)
        .width(Length::Fixed(width))
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color { a: 0.05, ..Color::from(theme.cosmic().accent_color()) }.into()),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

/// Between a block and the goal beside it
const GOAL_GAP: f32 = 10.0;

/// Room kept for the page's scroll bar
const SCROLLBAR_ROOM: f32 = 16.0;

/// The page's side margins when the goal is beside it, as the layout computed them
static PAGE_GUTTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub const GOAL_GAP_PUB: f32 = GOAL_GAP;

/// The tint over the construct the goal is for
fn construct_tint() -> Color {
    // (fainter on a light theme, where the text over it is dark and needs the contrast)
    let c = theme::active();
    Color { a: if c.cosmic().is_dark { 0.14 } else { 0.08 }, ..Color::from(c.cosmic().accent_color()) }
}

/// Code's line height, set on its editors and used to place things level with lines
const CODE_LINE: f32 = 1.4;

/// The source view's line height
pub fn source_line_height() -> f32 {
    z(14.0) * CODE_LINE
}

pub fn popout_scroll_id() -> widget::Id {
    widget::Id::new("isabellum-popout-scroll")
}

/// Keep the proof window's caret in sight
pub fn keep_popout_caret(line: usize) -> cosmic::app::Task<Message> {
    keep_caret_in(popout_scroll_id(), line, code_line_height(), CODE_PADDING.top)
}

pub fn source_scroll_id() -> widget::Id {
    widget::Id::new("isabellum-source-scroll")
}

fn source<'a>(ws: &'a Workspace, t: &'a Theory, inline: Option<&'a Workspace>, table: &super::symbols::Table, beside: Option<(f32, f32)>, width: f32) -> Element<'a, Message> {
    let mut col = widget::column::with_capacity(4).spacing(2).height(Length::Fill);
    let pad = Padding::from(spacing().space_s as f32);
    // The editor at its full height in a scroll view (shared with the goal when that is beside
    // it), so that where its lines are drawn is known: the construct the caret is in (the
    // statement around it, or the step if that is long) is tinted, and the goal sits level with it
    let source_width = beside.map_or(width, |(_, w)| w);
    let construct = t
        .blocks
        .iter()
        .find(|b| b.start <= t.cursor && t.cursor <= b.end)
        .and_then(|b| t.construct_at(b.start, b.end))
        .map(|(cs, ce)| {
            let line = |o: usize| t.lines.position(&t.decoded.text, t.decoded.to_decoded(o)).line;
            (line(cs), line(ce.saturating_sub(1).max(cs)))
        });
    // (in rows as drawn: a long line wraps onto several)
    let per_row = ((source_width - pad.left - pad.right) / (z(14.0) * 0.6)).floor().max(20.0) as usize;
    let rows: Vec<usize> = t.decoded.text.split('\n').map(|l| l.chars().count().div_ceil(per_row).max(1)).collect();
    let row_top = |line: usize| pad.top + rows.iter().take(line).sum::<usize>() as f32 * source_line_height();
    let line = construct.map_or(t.source.cursor().position.line, |(l0, _)| l0);
    let top = row_top(line);
    let mut code = super::gap::Gap::new(source_editor(t, Length::Shrink), widget::Space::new().height(Length::Fixed(0.0)), 0.0);
    if let Some((l0, l1)) = construct {
        code = code.band(row_top(l0), row_top(l1 + 1), construct_tint());
    }
    let editor = widget::container(hoverable(Some(ws), super::HoverTarget::Source, pad, code.into())).width(Length::Fixed(source_width));
    // The goal level with the construct, over its reserved column (moving only as far as it
    // must to stay in sight)
    let (goal_width, goal): (f32, Element<'a, Message>) = match beside.and_then(|(w, _)| Some((w, inline_goal(Some(ws), table)?))) {
        Some((w, g)) => (w, widget::container(g).width(Length::Fixed(w)).class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color::from(theme.cosmic().bg_color()).into()),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        })).into()),
        None => (beside.map_or(0.0, |(w, _)| w), widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into()),
    };
    let row = widget::row::with_capacity(2).spacing(spacing().space_m).push(editor).push(widget::Space::new().width(Length::Fixed(goal_width)));
    let at = iced::Vector::new(source_width + spacing().space_m as f32, top);
    // Pop out: a small button floating at the top right of the proof the caret is in
    let proof = t.blocks.iter().find(|b| b.start <= t.cursor && t.cursor <= b.end && b.kind == Kind::Formal);
    let (pop, pop_at): (Element<'a, Message>, iced::Vector) = match proof {
        Some(b) => (
            widget::button::icon(widget::icon::from_name("window-pop-out-symbolic"))
                .tooltip("Pop out (the proof in a window of its own)")
                .on_press(Message::PopOut)
                .into(),
            iced::Vector::new(source_width - z(40.0), row_top(t.lines.position(&t.decoded.text, t.decoded.to_decoded(b.start)).line)),
        ),
        None => (widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into(), iced::Vector::ZERO),
    };
    let page = widget::container(super::gap::Beside::new(super::gap::Beside::new(row, goal, at), pop, pop_at).fixed()).center_x(Length::Fill);
    col = col.push(widget::scrollable(page).id(source_scroll_id()).height(Length::Fill));
    if let Some(m) = vim_mode_now() {
        col = col.push(widget::text::caption(format!("-- {} --", m.label())));
    }
    if let Some(bar) = completion_bar(t) {
        col = col.push(bar);
    }
    // With no room beside it, the goal at the caret shows under the editor
    if let Some(g) = inline_goal(inline, table) {
        col = col.push(widget::container(widget::scrollable(g)).max_height(z(220.0)));
    }
    col.into()
}

fn source_editor(t: &Theory, height: Length) -> Element<'_, Message> {
    let sp = spacing();
    let vim_mode = vim_mode_now();
    iced::widget::text_editor(&t.source)
        .id(source_editor_id())
        .font(cosmic::font::mono())
        .size(z(14.0))
        .line_height(iced::widget::text::LineHeight::Relative(CODE_LINE))
        .padding(sp.space_s)
        .height(height)
        .on_action(Message::SourceAction)
        .key_binding(key_bindings(!t.completions.is_empty(), vim_mode))
        .style(|theme: &cosmic::Theme, _status| {
            let c = theme.cosmic();
            iced::widget::text_editor::Style {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                placeholder: Color { a: 0.5, ..Color::from(c.on_bg_color()) },
                value: c.on_bg_color().into(),
                selection: Color { a: 0.32, ..Color::from(c.accent.base) },
            }
        })
        .highlight_with::<super::editor::Highlighter>(t.highlight.themed(), highlight_format)
        .into()
}

// ----- the side panel

fn side_panel<'a>(app: &'a App, ws: &'a Workspace) -> Element<'a, Message> {
    let sp = spacing();
    let tabs = [
        (Panel::Quickcheck, "Quickcheck"),
        (Panel::Find, "Find"),
        (Panel::Symbols, "Symbols"),
        (Panel::Agents, "Agents"),
        (Panel::History, "History"),
        (Panel::Log, "Activity"),
    ];
    let mut rows = [widget::row::with_capacity(4).spacing(2), widget::row::with_capacity(4).spacing(2)];
    for (i, (panel, label)) in tabs.into_iter().enumerate() {
        let r = std::mem::replace(&mut rows[i / 4], widget::row::with_capacity(0));
        rows[i / 4] = r.push(
            widget::button::text(label)
                .class(if ws.panel == panel { theme::Button::Suggested } else { theme::Button::Text })
                .on_press(Message::Panel(panel)),
        );
    }
    let [r0, r1] = rows;
    let bar = widget::column::with_capacity(2).push(r0).push(r1);
    let table = app.table(&ws.machine);
    let content: Element<'a, Message> = match ws.panel {
        Panel::Output => output_panel(ws, &table),
        Panel::Sledgehammer => sledgehammer_panel(ws, &table),
        Panel::Find => find_panel(ws, &table),
        Panel::Symbols => symbols_panel(ws, &table),
        Panel::Agents => agents_panel(app, ws),
        Panel::History => history_panel(ws),
        Panel::Log => activity_panel(ws),
        Panel::Quickcheck => quickcheck_panel(ws, &table),
    };
    widget::container(widget::column::with_capacity(2).spacing(sp.space_xs).push(bar).push(content))
        .padding(sp.space_s)
        .width(Length::Fixed(app.right_width))
        .height(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn message_text<'a>(m: &Value, table: &super::symbols::Table) -> Element<'a, Message> {
    let kind = m["kind"].as_str().unwrap_or("").to_string();
    let text = table.decode(m["text"].as_str().unwrap_or("")).text;
    let w = widget::text(text).font(cosmic::font::mono()).size(z(13.0));
    tinted(kind, w.into())
}

/// Errors and warnings on a tinted background
fn tinted<'a>(kind: String, inner: Element<'a, Message>) -> Element<'a, Message> {
    match kind.as_str() {
        "error" | "warning" | "legacy" => widget::container(inner)
            .padding([2, 6])
            .width(Length::Fill)
            .class(theme::Container::custom(move |theme| {
                let c = theme.cosmic();
                let color: Color = if kind == "error" { c.destructive_color().into() } else { c.warning_color().into() };
                iced::widget::container::Style { background: Some(Color { a: 0.15, ..color }.into()), ..Default::default() }
            }))
            .into(),
        _ => inner,
    }
}

/// A message as code: highlighted with the prover's markup, selectable and copyable
fn message_view<'a>(ws: &'a Workspace, m: &Value, table: &super::symbols::Table) -> Element<'a, Message> {
    let key = super::message_key(m);
    let kind = m["kind"].as_str().unwrap_or("").to_string();
    match ws.views.get(&key) {
        Some(v) => tinted(
            kind,
            hoverable(Some(ws), super::HoverTarget::Message(key), Padding::from(2.0), iced::widget::text_editor(&v.content)
                .font(cosmic::font::mono())
                .size(z(13.0))
                .padding(2)
                .on_action(move |a| Message::ViewAction(key, a))
                // Read-only: copying and selecting only, so that every other key (j, k, the
                // arrows, d, Enter) goes on to the page's block navigation even when a click
                // has left this view focused
                .key_binding(read_only_binding)
                .style(|theme: &cosmic::Theme, _status| {
                    let c = theme.cosmic();
                    iced::widget::text_editor::Style {
                        background: Color::TRANSPARENT.into(),
                        border: Border::default(),
                        placeholder: Color::TRANSPARENT,
                        value: c.on_bg_color().into(),
                        selection: Color { a: 0.32, ..Color::from(c.accent.base) },
                    }
                })
                .highlight_with::<super::editor::Highlighter>(v.highlight.themed(), highlight_format)
                .into()),
        ),
        None => message_text(m, table),
    }
}

fn output_panel<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    widget::scrollable(state_view(ws, table)).height(Length::Fill).into()
}

/// The proof state: as parts (the chained facts, then each subgoal as fix, assume and show,
/// with what the command changed), or as Isabelle prints it; then any other output
fn state_view<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    // While Isabelle has yet to check what changed at the caret, output about another command
    // (the one that was there before the edit) is not shown as if it were this one's
    if let (Some(t), Some(out)) = (&ws.theory, &ws.output)
        && t.unchecked_from().is_some_and(|from| from <= t.cursor)
        && let (Some(s), Some(e)) = (out["command"]["start"].as_u64(), out["command"]["end"].as_u64())
    {
        let (s, e) = (super::App::local_offset_pub(t, s as usize), super::App::local_offset_pub(t, e as usize));
        let at = t.cursor.saturating_sub(1);
        if at < s || at >= e.max(s + 1) {
            return widget::text::caption("Checking…").into();
        }
    }
    let g = super::goals::Goals::parse(&ws.goals.messages);
    let have_parts = g.count.is_some() || g.nogoal || !g.facts.is_empty();
    let toggle = |label: &'static str, on: bool, msg: Message| {
        widget::button::text(label).class(if on { theme::Button::Suggested } else { theme::Button::Text }).on_press(msg)
    };
    let controls = widget::row::with_capacity(4)
        .spacing(sp.space_xxs)
        .align_y(Alignment::Center)
        .push(toggle("Goals", !ws.state_text, Message::StateText(false)))
        .push(toggle("Text", ws.state_text, Message::StateText(true)))
        .push(widget::Space::new().width(Length::Fill))
        .push(widget::checkbox(ws.show_types).label("Types").on_toggle(Message::ShowTypes));
    // On a proof tool typed in the text (sledgehammer, nitpick, …): its findings, as they come,
    // in place of the state, with its settings
    let tool = ws.output.as_ref().and_then(|o| o["command"]["name"].as_str()).filter(|n| super::editor::TOOLS.contains(n));
    let mut tool_section: Option<Element<'a, Message>> = None;
    if let Some(name) = tool {
        let running = ws.theory.as_ref().is_some_and(|t| t.caret_signature().2 == Status::Running);
        let mut head = widget::row::with_capacity(2).spacing(sp.space_s).align_y(Alignment::Center).push(widget::text::heading(name.to_string()));
        if running {
            let secs = ws.tool_started.map_or(0, |s| s.elapsed().as_secs());
            head = head
                .push(widget::text::caption(format!("running · {secs} s")).width(Length::Fill))
                .push(widget::button::text("Stop").on_press(Message::StopTools));
        }
        let stopped = !running
            && ws.output.as_ref().and_then(|o| o["messages"].as_array()).is_some_and(|ms| ms.iter().any(|m| m["kind"] == "error" && m["text"].as_str().is_some_and(|t| t.contains("Interrupt"))));
        if stopped {
            head = head.push(widget::text::caption("stopped (as if its time ran out)"));
        }
        let mut col = widget::column::with_capacity(4).spacing(sp.space_xs).push(head);
        if running {
            col = col.push(iced::widget::progress_bar(0.0..=1.0, ((ws.tool_started.map_or(0.0, |s| s.elapsed().as_secs_f32())) / ws.sh_timeout.trim().parse::<f32>().unwrap_or(30.0)).min(1.0)).girth(4.0));
        }
        col = col.push(tool_output(ws, table));
        if name == "sledgehammer" {
            col = col.push(sledgehammer_settings(ws));
        }
        // (above the goal it works on, which shows as usual)
        tool_section = Some(
            widget::container(col)
                .padding([sp.space_xxs, sp.space_xs])
                .width(Length::Fill)
                .class(theme::Container::custom(|theme| iced::widget::container::Style {
                    background: Some(Color { a: 0.06, ..Color::from(theme.cosmic().accent_color()) }.into()),
                    border: Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                }))
                .into(),
        );
    }
    // Only a message (no proof here, or the session stopped): just the message
    let only_message = !have_parts
        && tool_section.is_none()
        && !fill_here(ws)
        && ws.output.as_ref().is_none_or(|o| o.get("error").is_some() || o["messages"].as_array().is_none_or(|m| m.is_empty()));
    if only_message {
        // (while Isabelle has yet to check what changed, there is a state coming, not none)
        let checking = ws.theory.as_ref().is_some_and(|t| t.unchecked_from().is_some_and(|from| from <= t.cursor));
        let text = match ws.output.as_ref().and_then(|o| o["error"].as_str()) {
            Some(e) => e.to_string(),
            None if checking => "Checking…".to_string(),
            None => "No proof state here.".to_string(),
        };
        return widget::text::caption(text).into();
    }
    let mut col = widget::column::with_capacity(16).spacing(sp.space_xs).push(controls);
    let in_tool = tool_section.is_some();
    if let Some(section) = tool_section {
        col = col.push(section);
    }
    if ws.state_text && let Some(text) = &g.text {
        // Isabelle's own text of the state, printed with the Types setting
        col = col.push(tool_output(ws, table));
        col = col.push(message_view(ws, text, table));
        return col.into();
    }
    if ws.state_text || !have_parts {
        col = col.push(state_text(ws, table));
        if fill_here(ws) {
            col = col.push(fill_results(ws, table));
        }
        return col.into();
    }
    // What the command did: subgoals solved, and new ones
    let prev = super::goals::Goals::parse(&ws.goals_prev.messages);
    let before: Vec<String> = prev.subgoals.iter().map(|s| s.key()).collect();
    let now: Vec<String> = g.subgoals.iter().map(|s| s.key()).collect();
    let compared = prev.count.is_some();
    let solved = before.iter().filter(|k| !now.contains(k)).count();
    let added = now.iter().filter(|k| !before.contains(k)).count();
    let n = g.count.unwrap_or(0);
    let headline = if g.nogoal {
        "No goal: stating facts (forward mode)".to_string()
    } else if n == 0 {
        "No subgoals left: the proof can be finished".to_string()
    } else {
        format!("{n} subgoal{}", if n == 1 { "" } else { "s" })
    };
    // The command's own output (a theorem it proved, a value, a warning) comes first: it is
    // what the step did
    if !in_tool {
        col = col.push(tool_output(ws, table));
    }
    let mut head = widget::row::with_capacity(3).spacing(sp.space_s).align_y(Alignment::Center).push(widget::text::heading(headline));
    if matches!(ws.goals.status.as_str(), "waiting" | "running") {
        head = head.push(widget::text::caption("updating…"));
    }
    if compared && (solved > 0 || added > 0) {
        let mut parts = Vec::new();
        if solved > 0 {
            parts.push(format!("{solved} solved"));
        }
        if added > 0 {
            parts.push(format!("{added} new"));
        }
        head = head.push(widget::text::caption(format!("by this step: {}", parts.join(", "))));
    }
    col = col.push(head);
    if !g.facts.is_empty() {
        let mut facts = widget::column::with_capacity(g.facts.len()).spacing(2);
        for f in &g.facts {
            facts = facts.push(message_view(ws, f, table));
        }
        col = col.push(labelled("using", facts.into()));
    }
    let shown = if ws.all_goals { g.subgoals.len() } else { g.subgoals.len().min(4) };
    for (i, sg) in g.subgoals.iter().take(shown).enumerate() {
        let new = compared && !before.contains(&sg.key());
        col = col.push(subgoal_card(ws, sg, i == 0, new, table));
    }
    if g.subgoals.len() > shown {
        col = col.push(widget::button::text(format!("Show {} more", g.subgoals.len() - shown)).on_press(Message::AllGoals(true)));
    } else if ws.all_goals && g.subgoals.len() > 4 {
        col = col.push(widget::button::text("Show fewer").on_press(Message::AllGoals(false)));
    }
    col.into()
}

/// Sledgehammer's settings, folded away under the proof state: sledgehammer itself is typed in
/// the text (where its proofs come with Insert)
fn sledgehammer_settings<'a>(ws: &'a Workspace) -> Element<'a, Message> {
    let sp = spacing();
    let head = widget::button::text(if ws.sh_open { "▾ Sledgehammer settings" } else { "▸ Sledgehammer settings" })
        .on_press(Message::ShSettings(!ws.sh_open));
    let mut col = widget::column::with_capacity(4).spacing(sp.space_xxs).push(head);
    if ws.sh_open {
        col = col.push(
            widget::text_input("provers (default: cvc5 verit z3 e spass vampire zipperposition)", &ws.provers)
                .on_input(Message::Provers)
                .on_submit(|_| Message::ShApply),
        );
        col = col.push(
            widget::row::with_capacity(4)
                .spacing(sp.space_xs)
                .align_y(Alignment::Center)
                .push(widget::text::body("Timeout"))
                .push(widget::text_input("30", &ws.sh_timeout).on_input(Message::ShTimeout).on_submit(|_| Message::ShApply).width(Length::Fixed(64.0)))
                .push(widget::text::body("seconds per prover").width(Length::Fill))
                .push(widget::button::standard("Apply").on_press(Message::ShApply)),
        );
        col = col.push(widget::text::caption("These hold for sledgehammer typed in the text. For one run, write sledgehammer [timeout = 120, isar_proofs] there."));
    }
    col.into()
}

/// What the command at the caret printed besides the state (a theorem, sledgehammer's or
/// nitpick's findings typed in the text, a warning), with Insert for the proofs it offers; then
/// an LLM's proofs for a hole, when one was asked for (typing `claude` or `codex`)
fn tool_output<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(8).spacing(sp.space_xxs);
    if let Some(out) = &ws.output {
        // (a tool's Interrupt is its being stopped: said in its heading, not as an error)
        for m in out["messages"].as_array().into_iter().flatten().filter(|m| m["kind"] != "state" && !(m["kind"] == "error" && m["text"].as_str().is_some_and(|t| t.contains("Interrupt")))) {
            col = col.push(message_view(ws, m, table));
            for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
                col = col.push(
                    widget::row::with_capacity(2)
                        .spacing(6)
                        .align_y(Alignment::Center)
                        .push(widget::text(table.decode(sb).text).font(cosmic::font::mono()).size(z(13.0)).width(Length::Fill))
                        .push(widget::button::suggested("Insert").on_press(Message::InsertOutput(sb.to_string()))),
                );
            }
        }
    }
    // A `claude`, `codex` or `@name` line not asked yet: when it will be
    if !fill_here(ws)
        && let Some(t) = &ws.theory
        && let Some(line) = super::keyword_line(t)
    {
        let who = line.split_whitespace().find(|w| w.starts_with("claude") || w.starts_with("codex") || w.starts_with('@')).unwrap_or("claude").to_string();
        col = col.push(widget::text::caption(format!("{who}: asks when you pause typing (Enter asks now)")));
    }
    if fill_here(ws) && !ws.fill.status.is_empty() {
        // As sledgehammer shows: the tool, how long it has been at it, Stop
        let busy = matches!(ws.fill.status.as_str(), "starting" | "asking" | "checking");
        let secs = ws.fill.started.map_or(0, |s| s.elapsed().as_secs());
        let mut head = widget::row::with_capacity(3)
            .spacing(sp.space_s)
            .align_y(Alignment::Center)
            .push(widget::text::heading(match &ws.fill.asked {
                Some(name) => format!("@{name}"),
                None => format!(
                    "{}{}",
                    if ws.fill.agent == 1 { "codex" } else { "claude" },
                    ws.fill.model.as_ref().map_or(String::new(), |m| format!(" · {m}"))
                ),
            }));
        head = head.push(widget::text::caption(if busy { format!("{} · {secs} s", ws.fill.status) } else { ws.fill.status.clone() }).width(Length::Fill));
        head = head.push(widget::button::text(if busy { "Stop" } else { "Dismiss" }).on_press(Message::FillDismiss));
        col = col.push(head);
        if busy {
            col = col.push(iced::widget::progress_bar(0.0..=1.0, (secs as f32 / 120.0).min(1.0)).girth(4.0));
        }
        if let Some(m) = &ws.fill.message {
            col = col.push(widget::text::body(m.clone()));
        }
        col = col.push(fill_results(ws, table));
        col = col.push(query_route_row(ws));
    }
    col.into()
}

/// Where `claude` and `codex` queries go: the session's helper conversation (it remembers), a
/// fresh request each time, or a listening agent; the helper's conversation, to open or start again
fn query_route_row<'a>(ws: &'a Workspace) -> Element<'a, Message> {
    let sp = spacing();
    let route = ws.activity.as_ref().and_then(|a| a["route"].as_str()).unwrap_or("helper").to_string();
    let listeners: Vec<(String, String)> = ws
        .activity
        .as_ref()
        .and_then(|a| a["listeners"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l["key"].as_str()?.to_string(), l["label"].as_str()?.to_string())))
        .collect();
    let mut row = widget::row::with_capacity(8).spacing(sp.space_xxs).align_y(Alignment::Center).push(widget::text::caption("Queries go to"));
    let choice = |label: String, value: String| {
        let on = route == value;
        widget::button::text(label).class(if on { theme::Button::Suggested } else { theme::Button::Text }).on_press(Message::QueryRoute(value))
    };
    row = row.push(choice("the helper".into(), "helper".into())).push(choice("a fresh request".into(), "fresh".into()));
    for (key, label) in listeners {
        row = row.push(choice(format!("@{label}"), format!("agent:{key}")));
    }
    row = row.push(widget::Space::new().width(Length::Fill));
    if route == "helper" && ws.fill.asked.is_none() {
        if let Some(conv) = &ws.fill.conversation {
            let resume = conv["resume"].as_str().unwrap_or("").to_string();
            let machine = conv["machine"].as_str().unwrap_or("").to_string();
            row = row.push(
                widget::button::text("Copy resume command")
                    .tooltip(format!("Talk to the helper: run this on {machine}"))
                    .on_press(Message::CopyText(resume)),
            );
        }
        row = row.push(widget::button::text("New conversation").on_press(Message::HelperReset));
    }
    widget::container(row).padding([sp.space_xxs, 0]).into()
}

/// A label in a narrow column, then what it labels
fn labelled<'a>(label: &'static str, body: Element<'a, Message>) -> Element<'a, Message> {
    widget::row::with_capacity(2)
        .spacing(spacing().space_xs)
        .push(
            widget::container(widget::text(label).font(cosmic::font::mono()).size(z(12.0)).class(theme::Text::Accent))
                .padding(Padding { top: 3.0, ..Padding::ZERO })
                .width(Length::Fixed(z(52.0))),
        )
        .push(widget::container(body).width(Length::Fill))
        .into()
}

/// One subgoal as an Isar user reads it: the variables it fixes, what it assumes, what it shows
fn subgoal_card<'a>(ws: &'a Workspace, sg: &super::goals::Subgoal, first: bool, new: bool, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    let mut head = widget::row::with_capacity(2).spacing(sp.space_xs).push(widget::text::caption(format!("Subgoal {}", sg.n)));
    if new {
        head = head.push(widget::text::caption("· new").class(theme::Text::Accent));
    }
    let mut col = widget::column::with_capacity(4).spacing(2).push(head);
    let stack = |parts: &[Value]| -> Element<'a, Message> {
        let mut c = widget::column::with_capacity(parts.len()).spacing(2);
        for p in parts {
            c = c.push(message_view(ws, p, table));
        }
        c.into()
    };
    if !sg.params.is_empty() {
        col = col.push(labelled("fix", stack(&sg.params)));
    }
    if !sg.prems.is_empty() {
        col = col.push(labelled("assume", stack(&sg.prems)));
    }
    if let Some(c) = &sg.concl {
        col = col.push(labelled("show", message_view(ws, c, table)));
    }
    widget::container(col)
        .padding([sp.space_xxs, sp.space_xs])
        .width(Length::Fill)
        .class(theme::Container::custom(move |theme| {
            let c = theme.cosmic();
            let base = Color::from(c.bg_component_color());
            iced::widget::container::Style {
                background: Some(if first { blend(base, Color::from(c.accent_color()), 0.08) } else { base }.into()),
                border: Border { radius: 6.0.into(), ..Default::default() },
                ..Default::default()
            }
        }))
        .into()
}

/// The state as Isabelle prints it, with the command's other output
fn state_text<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    let mut col = widget::column::with_capacity(8).spacing(6);
    match &ws.output {
        None => col = col.push(widget::text::caption("Move the caret into a proof to see its state.")),
        Some(out) if out.get("error").is_some() => col = col.push(widget::text::body(out["error"].as_str().unwrap_or("").to_string())),
        Some(out) => {
            if let Some(c) = out.get("command").filter(|c| c.is_object()) {
                col = col.push(widget::text::caption(format!(
                    "After {} ({}){}",
                    c["name"].as_str().unwrap_or(""),
                    c["status"].as_str().unwrap_or(""),
                    if out["outdated"].as_bool() == Some(true) { " — prover catching up" } else { "" }
                )));
            }
            let msgs = out["messages"].as_array().cloned().unwrap_or_default();
            if msgs.is_empty() {
                col = col.push(widget::text::caption("No output."));
            }
            for m in msgs {
                col = col.push(message_view(ws, &m, table));
                for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
                    col = col.push(
                        widget::row::with_capacity(2)
                            .spacing(6)
                            .align_y(Alignment::Center)
                            .push(widget::text(table.decode(sb).text).font(cosmic::font::mono()).size(13).width(Length::Fill))
                            .push(widget::button::suggested("Insert").on_press(Message::InsertOutput(sb.to_string()))),
                    );
                }
            }
        }
    }
    col.into()
}

fn proofs(q: &super::QueryState) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in &q.messages {
        for sb in m["sendbacks"].as_array().into_iter().flatten() {
            if let Some(s) = sb.as_str()
                && !out.iter().any(|p| p == s)
            {
                out.push(s.to_string());
            }
        }
    }
    out
}

fn sledgehammer_panel<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    let q = &ws.sledgehammer;
    let mut col = widget::column::with_capacity(12).spacing(sp.space_xs);
    col = col.push(widget::text::caption("Runs on the goal at the caret; on a sorry or oops, on the goal before it."));
    col = col.push(
        widget::row::with_capacity(3)
            .spacing(sp.space_xs)
            .align_y(Alignment::Center)
            .push(widget::text_input("provers (default: cvc5 verit z3 e spass vampire zipperposition)", &ws.provers).on_input(Message::Provers).width(Length::Fill))
            .push(widget::button::suggested("Run").on_press(Message::Sledgehammer))
            .push(widget::button::standard("Cancel").on_press_maybe((q.status == "running" || q.status == "waiting").then_some(Message::CancelQuery))),
    );
    col = col.push(
        widget::row::with_capacity(3)
            .spacing(sp.space_xs)
            .align_y(Alignment::Center)
            .push(widget::text::body("Timeout"))
            .push(widget::text_input("30", &ws.sh_timeout).on_input(Message::ShTimeout).width(Length::Fixed(64.0)))
            .push(widget::text::body("seconds per prover")),
    );
    col = col.push(
        widget::row::with_capacity(2)
            .spacing(sp.space_m)
            .push(widget::checkbox(ws.sh_isar).label("Isar proofs").on_toggle(Message::ShIsar))
            .push(widget::checkbox(ws.sh_try0).label("Try simple methods first").on_toggle(Message::ShTry0)),
    );
    col = col.push(widget::text::caption("For finer control write sledgehammer [timeout = 120, max_facts = 1000] in the proof; its proofs appear in Output with Insert."));
    if !q.status.is_empty() {
        let status = match q.status.as_str() {
            "waiting" | "starting" => "Waiting for the goal to be checked…",
            "running" => "Sledgehammering…",
            "finished" => "Finished",
            "canceled" => "Canceled",
            "failed" => "Failed",
            other => other,
        };
        col = col.push(widget::text::heading(status.to_string()));
    }
    if let Some(err) = &q.error {
        col = col.push(widget::text::body(err.clone()));
    }
    let found = proofs(q);
    if !found.is_empty() {
        col = col.push(widget::text::caption(if q.replace.is_some() { "Found proofs — Insert replaces the sorry:" } else { "Found proofs — Insert adds the proof after the command:" }));
        for p in found {
            col = col.push(
                widget::row::with_capacity(2)
                    .spacing(sp.space_xs)
                    .align_y(Alignment::Center)
                    .push(widget::text(table.decode(&p).text).font(cosmic::font::mono()).size(13).width(Length::Fill))
                    .push(widget::button::suggested("Insert").on_press(Message::InsertProof(p.clone()))),
            );
        }
    }
    for m in &q.messages {
        let text = m["text"].as_str().unwrap_or("");
        if text.contains("found a proof") || m["sendbacks"].as_array().is_some_and(|s| !s.is_empty()) {
            continue;
        }
        col = col.push(message_view(ws, m, table));
    }
    // An LLM's proofs for the hole, each checked by the prover before it is offered
    let f = &ws.fill;
    col = col.push(widget::divider::horizontal::default());
    col = col.push(widget::text::heading("Fill the hole with an LLM"));
    col = col.push(widget::text::caption("Claude or Codex on the session's machine proposes proofs for the sorry at or after the caret; each is checked by the prover first."));
    col = col.push(widget::text::caption("Type claude or codex (with a hint after it, if you like) on a line of its own in a proof and press Enter."));
    let _ = f;
    col = col.push(fill_results(ws, table));
    widget::scrollable(col).height(Length::Fill).into()
}

/// An LLM fill's progress and its proofs, each with an Insert button
fn fill_results<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    let f = &ws.fill;
    let mut col = widget::column::with_capacity(f.candidates.len() + 2).spacing(sp.space_xs);
    if !f.status.is_empty() {
        let status = match f.status.as_str() {
            "starting" | "asking" => "Asking for proofs…",
            "checking" => "Checking the proposed proofs…",
            "done" => "Done",
            "failed" => "Failed",
            other => other,
        };
        col = col.push(widget::text::body(status.to_string()));
    }
    if let Some(err) = &f.error {
        col = col.push(widget::text::body(err.clone()));
    }
    if !f.using.is_empty() {
        col = col.push(
            widget::row::with_capacity(2)
                .spacing(sp.space_xs)
                .align_y(Alignment::Center)
                .push(
                    widget::column::with_capacity(2)
                        .push(widget::text(format!("using {}", f.using.iter().map(|u| table.decode(u).text).collect::<Vec<_>>().join(" "))).font(cosmic::font::mono()).size(z(13.0)))
                        .push(widget::text::caption("Facts to guide Sledgehammer"))
                        .width(Length::Fill),
                )
                .push(widget::button::standard("Insert with sledgehammer").on_press(Message::InsertUsing)),
        );
    }
    for c in &f.candidates {
        let text = c["text"].as_str().unwrap_or("").to_string();
        let mark = match c["ok"].as_bool() {
            Some(true) => "✓ ",
            Some(false) => "✗ ",
            None => "… ",
        };
        let mut row = widget::row::with_capacity(2).spacing(sp.space_xs).align_y(Alignment::Center).push(
            widget::column::with_capacity(2)
                .push(widget::text(format!("{mark}{}", table.decode(&text).text)).font(cosmic::font::mono()).size(z(13.0)))
                .push(widget::text::caption(c["message"].as_str().unwrap_or("").to_string()))
                .width(Length::Fill),
        );
        row = row.push(if c["ok"].as_bool() == Some(true) {
            widget::button::suggested("Insert").on_press(Message::InsertFill(text))
        } else {
            widget::button::standard("Insert anyway").on_press(Message::InsertFill(text))
        });
        col = col.push(row);
    }
    col.into()
}

fn find_panel<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    let q = &ws.find;
    let mut col = widget::column::with_capacity(6).spacing(sp.space_xs);
    col = col.push(
        widget::row::with_capacity(2)
            .spacing(sp.space_xs)
            .push(widget::text_input("\"_ + _ = _ + _\" name: comm", &ws.find_input).on_input(Message::FindInput).on_submit(|_| Message::Find).width(Length::Fill))
            .push(widget::button::suggested("Find").on_press(Message::Find)),
    );
    col = col.push(widget::text::caption("Theorems in the context at the caret."));
    if q.status == "running" || q.status == "waiting" || q.status == "starting" {
        col = col.push(widget::text::caption("Searching…"));
    }
    for m in &q.messages {
        col = col.push(message_text(m, table));
    }
    widget::scrollable(col).height(Length::Fill).into()
}

/// Isabelle symbols to insert, by group or by search (name, LaTeX name or abbreviation)
fn symbols_panel<'a>(ws: &'a Workspace, table: &super::symbols::Table) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(24).spacing(sp.space_xs);
    col = col.push(widget::text_input("search: forall, leq, -->, sub…", &ws.symbol_search).on_input(Message::SymbolSearch));
    col = col.push(widget::text::caption("In the text, type \\ and a name (or a LaTeX name) and press Tab."));
    let button = |c: char, sym: String, how: String| -> Element<'a, Message> {
        let tip = if how.is_empty() { sym.clone() } else { format!("{sym}   {how}") };
        widget::tooltip(
            widget::button::text(c.to_string()).padding([2, 6]).on_press(Message::InsertSymbol(c.to_string())),
            widget::text(tip),
            widget::tooltip::Position::Bottom,
        )
        .into()
    };
    let wrap = |items: Vec<Element<'a, Message>>| -> Element<'a, Message> {
        let mut rows = widget::column::with_capacity(items.len() / 10 + 1).spacing(2);
        let mut row = widget::row::with_capacity(10).spacing(2);
        let mut n = 0;
        for item in items {
            row = row.push(item);
            n += 1;
            if n == 10 {
                rows = rows.push(std::mem::replace(&mut row, widget::row::with_capacity(10).spacing(2)));
                n = 0;
            }
        }
        if n > 0 {
            rows = rows.push(row);
        }
        rows.into()
    };
    let q = ws.symbol_search.trim().trim_start_matches('\\');
    if !q.is_empty() {
        let found = table.search(q, 40);
        col = col.push(wrap(found.into_iter().map(|(c, sym, how)| button(c, sym, how)).collect()));
    } else {
        for (group, syms) in table.groups() {
            col = col.push(widget::text::caption_heading(group));
            col = col.push(wrap(syms.into_iter().map(|(c, sym)| button(c, sym, String::new())).collect()));
        }
    }
    widget::scrollable(col).height(Length::Fill).into()
}

fn agents_panel<'a>(app: &'a App, ws: &'a Workspace) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(24).spacing(sp.space_xs);
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let grants: Vec<Value> = summary.and_then(|s| s["grants"].as_array().cloned()).unwrap_or_default();
    let clients: Vec<Value> = summary.and_then(|s| s["clients"].as_array().cloned()).unwrap_or_default();
    let seen = ws.info.as_ref().and_then(|i| i["seen"].as_object().cloned()).unwrap_or_default();
    col = col.push(widget::text::caption("Agents use this session through Isabellum's MCP tools. They edit only where you grant rights; grants last until you revoke them."));
    if seen.is_empty() {
        col = col.push(widget::text::body("No agent has used this session yet."));
    }
    for (key, info) in seen {
        let label = info["label"].as_str().unwrap_or(&key).to_string();
        let live = clients.iter().any(|c| c["key"].as_str() == Some(key.as_str()));
        let pos = info["position"].clone();
        let where_ = pos["path"].as_str().map(|p| format!("{} line {}", file_name(p), pos["line"])).unwrap_or_default();
        let mut card = widget::column::with_capacity(6).spacing(4);
        card = card.push(widget::text::heading(format!("{label}{}", if live { " · attached" } else { "" })));
        card = card.push(widget::text::caption(format!("{key}{}", if where_.is_empty() { String::new() } else { format!(" · at {where_}") })));
        let mine: Vec<&Value> = grants.iter().filter(|g| g["agent"].as_str() == Some(key.as_str())).collect();
        for g in &mine {
            let scope = match g["scope"].as_str() {
                Some("folder") => "May edit the whole folder".to_string(),
                Some("theory") => format!("May edit {}", file_name(g["path"].as_str().unwrap_or(""))),
                Some("proof") => format!("May edit the proof at line {} of {}", g["line"], file_name(g["path"].as_str().unwrap_or(""))),
                _ => "May edit".into(),
            };
            card = card.push(
                widget::row::with_capacity(2)
                    .spacing(sp.space_xs)
                    .align_y(Alignment::Center)
                    .push(widget::text::body(scope).width(Length::Fill))
                    .push(widget::button::destructive("Revoke").on_press(Message::Revoke(g["id"].as_str().unwrap_or("").to_string()))),
            );
        }
        card = card.push(
            widget::row::with_capacity(3)
                .spacing(4)
                .push(widget::button::standard("Grant folder").on_press(Message::Grant(key.clone(), label.clone(), "folder")))
                .push(widget::button::standard("This theory").on_press_maybe(ws.theory.as_ref().map(|_| Message::Grant(key.clone(), label.clone(), "theory"))))
                .push(widget::button::standard("Proof at caret").on_press_maybe(ws.theory.as_ref().map(|_| Message::Grant(key.clone(), label.clone(), "proof")))),
        );
        let following = ws.follow.as_deref() == Some(key.as_str());
        card = card.push(
            widget::row::with_capacity(2)
                .spacing(sp.space_xs)
                .align_y(Alignment::Center)
                .push(widget::toggler(following).on_toggle(move |on| Message::Follow(on.then(|| key.clone()))))
                .push(widget::text::body("Follow this agent")),
        );
        col = col.push(widget::container(card).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill));
    }
    widget::scrollable(col).height(Length::Fill).into()
}

fn history_panel(ws: &Workspace) -> Element<'_, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(ws.history.len() + 1).spacing(4);
    col = col.push(widget::text::caption("Ctrl+Z undoes your own edits. Agents' changes are reverted one by one; later edits are kept."));
    for h in &ws.history {
        let author = h["author"]["label"].as_str().unwrap_or("?").to_string();
        let kind = h["kind"].as_str().unwrap_or("").to_string();
        let reverted = h["reverted_by"].is_number();
        let mut row = widget::row::with_capacity(2).spacing(sp.space_xs).align_y(Alignment::Center).push(
            widget::column::with_capacity(2)
                .push(widget::text::body(format!("{author} — {kind} at line {}{}", h["line"], if reverted { " (reverted)" } else { "" })))
                .push(widget::text::caption(h["summary"].as_str().unwrap_or("").to_string()))
                .width(Length::Fill),
        );
        if h["author"]["kind"] == "agent" && !reverted
            && let Some(id) = h["id"].as_u64()
        {
            row = row.push(widget::button::standard("Revert").on_press(Message::Revert(id)));
        }
        col = col.push(row);
    }
    widget::scrollable(col).height(Length::Fill).into()
}
