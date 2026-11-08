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

use super::shell::{self, Msg, backend_mark, badge, blend, destructive, sp, spinner, success, tinted_icon};
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
    // Cerea's icon bar: the parts sized to their icons, at the bar's left, rounded at its ends only
    let mut bar = widget::row::with_capacity(tabs.len()).align_y(Alignment::Center);
    let last = tabs.len() - 1;
    for (i, (mode, icon_name, label)) in tabs.iter().enumerate() {
        let enabled = matches!(mode, LeftMode::Sessions | LeftMode::Project) || theory.is_some();
        // (a section with nothing to show yet is dimmed, as well as not pressable)
        let icon: Element<'_, Message> = if enabled { widget::icon::from_name(*icon_name).size(16).icon().into() } else { tinted_icon(icon_name, 16, shell::dimmed) };
        let b: Element<'_, Message> = if enabled {
            button::custom(icon)
                .width(Length::Shrink)
                .height(Length::Fixed(32.0))
                .padding([8, 12])
                .class(tab_style(current == *mode, i == 0, i == last))
                .on_press(Message::LeftMode(*mode))
                .into()
        } else {
            // (a disabled button draws its icon in the button's colour, whatever the icon's own)
            widget::container(icon).padding([8, 12]).center_y(Length::Fixed(32.0)).height(Length::Fixed(32.0)).into()
        };
        let tip = if enabled { label.to_string() } else { format!("{label} (open a document first)") };
        bar = bar.push(household_widgets::tooltip_with(b, widget::text(tip), widget::tooltip::Position::Bottom));
    }
    let bar = widget::container(bar).width(Length::Shrink).class(theme::Container::custom(|theme| iced::widget::container::Style {
        background: Some(Color::from(theme.cosmic().bg_component_color()).into()),
        border: Border { radius: theme.cosmic().radius_xl().into(), ..Default::default() },
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
    let card = widget::container(
        widget::column::with_capacity(3)
            .spacing(s.space_xxs)
            .padding(Padding { top: s.space_xs as f32, bottom: s.space_xs as f32, left: s.space_xs as f32, right: s.space_xs as f32 })
            .push(bar)
            .push(widget::container(heading).padding(Padding { left: 6.0, right: 0.0, top: 2.0, bottom: 0.0 }))
            .push(widget::scrollable(widget::container(content).padding(Padding { right: 14.0, ..Padding::ZERO })).height(Length::Fill)),
    )
    .width(Length::Fixed(app.left_width))
    .height(Length::Fill)
    .class(super::view::chrome_panel());
    inset(card.into(), true)
}

/// COSMIC's `space_xxs` (8 px), the margin a card keeps from the window's edges and the header
pub fn card_margin() -> f32 {
    super::view::spacing().space_xxs as f32
}

/// What an inset card adds to its width besides the 6 px splitter beside it: the margin outside
/// (8 px) and the 2 px inside that, with the splitter, make the gap to the page 8 px as well
pub const INSET_SPAN: f32 = 10.0;

/// `card` inset on the page: `space_xxs` from the window's edge, the header above and the status
/// bar below, and the splitter's 6 px and 2 more between it and the page. `left` is the left
/// sidebar's side
pub fn inset<'a>(card: Element<'a, Message>, left: bool) -> Element<'a, Message> {
    let m = card_margin();
    let (l, r) = if left { (m, INSET_SPAN - m) } else { (INSET_SPAN - m, m) };
    widget::container(card).padding(Padding { top: m, bottom: m, left: l, right: r }).height(Length::Fill).into()
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

/// (with a no-break space, so that a wrapped line never parts the number from its unit)
fn gigabytes(mb: u64) -> String {
    format!("{:.1}\u{a0}GB", mb as f64 / 1000.0)
}

/// A folder as a heading: under the machine's home as `~/…`, and no more than its last two parts
/// (the whole path in its tooltip)
pub(crate) fn short_folder(folder: &str, home: Option<&str>) -> String {
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
        let connected = shell::connection(link) == shell::Conn::Connected;
        let total: u64 = on_machine.iter().filter_map(|(id, v)| memory_mb(app, key, id, v)).sum();
        let budget = link.hello.as_ref().and_then(|h| h["memory_budget_gb"].as_f64());
        // (its name, and under it whether it is connected and what its provers use: a line that
        // still fits a narrow sidebar)
        let memory = (total > 0).then(|| match budget {
            Some(b) => format!("{} of {b:.0}\u{a0}GB", gigabytes(total).trim_end_matches("\u{a0}GB")),
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
            col = col.push(household_widgets::tooltip_with(
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

/// A machine's connection: a dot and a word (as the status bar and the settings say it), then
/// what its provers use
fn connection<'a>(link: &super::Link, memory: Option<String>) -> Element<'a, Message> {
    let conn = shell::connection(link);
    let word = conn.word().to_string();
    let tip = conn.detail().map_or_else(|| word.clone(), |d| format!("{word}: {d}"));
    household_widgets::tooltip_with(
        widget::row::with_capacity(2)
            .spacing(4)
            .align_y(Alignment::Center)
            .push(conn.dot(7.0))
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
        detail.push(format!("{agents}\u{a0}agent{}", if agents == 1 { "" } else { "s" }));
    }
    // (its memory where the sidebar has room for it; the machine's total is above in any case)
    if let Some(mb) = memory_mb(app, machine, sid, v).filter(|_| app.left_width >= 280.0) {
        detail.push(gigabytes(mb));
    }
    // The caption: the machine and the state in words, then the toolchain, agents and memory
    let s = sp();
    let machine_label = machine_name(app, machine);
    let mut text = widget::column::with_capacity(3)
        .push(name_text(&format!("{name}{owner}"), 14.0, current))
        .push(widget::text::caption(caption_line(machine_label, lifecycle)).class(super::view::muted()).wrapping(iced::widget::text::Wrapping::Word));
    // The details on one line where they fit (about 6.5 px a character of a caption, in the room
    // left beside the mark), else the toolchain on a line of its own: a wrapped line never begins
    // or ends with a separator
    let room = app.left_width - 80.0 - indent;
    let caption = |t: String| widget::text::caption(t).class(super::view::muted()).wrapping(iced::widget::text::Wrapping::Word);
    let joined = detail.join("  \u{b7}  ");
    if detail.len() > 1 && joined.chars().count() as f32 * 6.5 > room {
        text = text.push(caption(detail[0].clone())).push(caption(detail[1..].join("  \u{b7}  ")));
    } else if !detail.is_empty() {
        text = text.push(caption(joined));
    }
    let text = text.width(Length::Fill);
    // Its state: a mark (a spinner while it checks), and what fails
    let mark = mark_of(lifecycle, checking);
    let mut tip = vec![format!("{name}{owner}"), caption_line(machine_label, lifecycle)];
    if mark == Mark::Spinner {
        tip.push("checking".into());
    }
    let mut row = widget::row::with_capacity(4).spacing(s.space_xs).align_y(Alignment::Start).push(mark_widget(mark));
    if mark == Mark::Word {
        row = row.push(widget::text::caption(lifecycle.to_string()));
    }
    row = row.push(text);
    if failing > 0 {
        row = row.push(badge(failing.to_string(), destructive));
        tip.push(format!("{failing} document{} failing", if failing == 1 { "" } else { "s" }));
    }
    let row = household_widgets::tooltip_with(
        button::custom(row).width(Length::Fill).padding([s.space_xxs, s.space_xs]).class(nav_style(current)).on_press(Message::OpenSession(machine.to_string(), sid.to_string())),
        widget::text(tip.join("\n")),
        widget::tooltip::Position::Bottom,
    );
    // (a right press on a session's row opens its menu: Open, the folder's path, Forget)
    let folder = v["folder"].as_str();
    let row = super::context_menu::area(row, super::context_menu::Target::Session(machine.to_string(), sid.to_string()), || super::context_menu::session_rows(machine, sid, folder));
    widget::container(row).padding(Padding { left: indent, ..Padding::ZERO }).into()
}

/// A tab of the icon bar: filled with the muted accent when it is the chosen one (hovered and
/// pressed with COSMIC's washes over the fill), else clear with COSMIC's wash on hover; rounded
/// (`radius_xl`) only at the bar's ends
fn tab_style(on: bool, first: bool, last: bool) -> theme::Button {
    fn style(theme: &cosmic::Theme, on: bool, ends: (bool, bool), wash: Option<Color>) -> button::Style {
        let c = theme.cosmic();
        let r = c.radius_xl()[0];
        let (left, right) = (if ends.0 { r } else { 0.0 }, if ends.1 { r } else { 0.0 });
        let mut s = button::Style::new();
        s.border_radius = [left, right, right, left].into();
        let (fill, label) = super::tones::muted(theme);
        let text: Color = c.on_bg_color().into();
        if on {
            s.background = Some(wash.map_or(fill, |w| super::tones::muted_washed(theme, w)).into());
            s.text_color = Some(label);
            s.icon_color = Some(label);
        } else {
            s.background = wash.map(Into::into);
            s.text_color = Some(text);
            s.icon_color = Some(text);
        }
        s
    }
    let ends = (first, last);
    theme::Button::Custom {
        active: Box::new(move |_, t| style(t, on, ends, None)),
        disabled: Box::new(move |t| style(t, on, ends, None)),
        hovered: Box::new(move |_, t| style(t, on, ends, Some(t.cosmic().icon_button.hover.into()))),
        pressed: Box::new(move |_, t| style(t, on, ends, Some(t.cosmic().icon_button.pressed.into()))),
    }
}

/// The one row style of a navigation list (household-ui rule 6a): clear at rest, COSMIC's
/// `icon_button` hover wash on hover, its pressed wash when pressed; the current row keeps the
/// hover wash, its text and icon in the accent (bold: [`row_font`])
pub fn nav_style(current: bool) -> theme::Button {
    fn style(theme: &cosmic::Theme, current: bool, wash: Option<Color>) -> button::Style {
        let c = theme.cosmic();
        let ink: Color = if current { c.accent_color().into() } else { c.on_bg_color().into() };
        let mut s = button::Style::new();
        s.border_radius = c.radius_s().into();
        s.background = wash.or_else(|| current.then(|| c.icon_button.hover.into())).map(Into::into);
        s.text_color = Some(ink);
        s.icon_color = Some(ink);
        s
    }
    theme::Button::Custom {
        active: Box::new(move |_, t| style(t, current, None)),
        disabled: Box::new(move |t| style(t, current, None)),
        hovered: Box::new(move |_, t| style(t, current, Some(t.cosmic().icon_button.hover.into()))),
        pressed: Box::new(move |_, t| style(t, current, Some(t.cosmic().icon_button.pressed.into()))),
    }
}

/// The font of a row's name: bold for the current one
pub(crate) fn row_font(current: bool) -> cosmic::iced::Font {
    if current { cosmic::font::bold() } else { cosmic::font::default() }
}

/// A name that wraps onto further lines where it must: a break opportunity is put after each
/// underscore (the font's own are at spaces), so never inside a word
pub(crate) fn wrappable(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 8);
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '_' && chars.peek().is_some_and(|n| *n != '_') {
            out.push('\u{200b}');
        }
    }
    out
}

/// A row's name: wrapped, bold when it is the current row
pub(crate) fn name_text<'a>(name: &str, size: f32, current: bool) -> widget::Text<'a, cosmic::Theme> {
    widget::text(wrappable(name)).size(size).font(row_font(current)).wrapping(iced::widget::text::Wrapping::Word).width(Length::Fill)
}

/// What marks a session's state beside its name
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Mark {
    /// A filled dot: running
    Dot,
    /// A hollow ring: paused
    Ring,
    /// The error icon: failed
    Error,
    /// A spinner: running, and checking
    Spinner,
    /// A grey dot: stopped (Tom, 8 Nov, option 1 of t33)
    Grey,
    /// A check: checked (the status chip's)
    Check,
    /// A red dot: checked, with failures or unfinished proofs (the status chip's)
    RedDot,
    /// Nothing at all
    Blank,
    /// Any other state: its word alone
    Word,
}

pub(crate) fn mark_of(lifecycle: &str, checking: bool) -> Mark {
    match lifecycle {
        "Running" if checking => Mark::Spinner,
        "Running" => Mark::Dot,
        "Paused" => Mark::Ring,
        "Stopped" => Mark::Grey,
        "Failed" => Mark::Error,
        _ => Mark::Word,
    }
}

/// The caption line: the machine, then the state in words
pub(crate) fn caption_line(machine: &str, lifecycle: &str) -> String {
    format!("{machine} \u{b7} {}", lifecycle.to_lowercase())
}

// The cell palette's muted tones (`tones::muted_status`) of the success, unfinished and error colours
pub(crate) fn muted_green(theme: &cosmic::Theme) -> Color {
    super::tones::muted_status(theme.cosmic().success_color().into(), theme.cosmic().is_dark)
}
pub(crate) fn muted_amber(theme: &cosmic::Theme) -> Color {
    super::tones::muted_status(super::view::sorry_color(theme), theme.cosmic().is_dark)
}
pub(crate) fn muted_red(theme: &cosmic::Theme) -> Color {
    super::tones::muted_status(super::view::error_color(theme), theme.cosmic().is_dark)
}

/// The grey of a stopped session's dot: the text's own colour, muted as the status colours are
pub(crate) fn muted_grey(theme: &cosmic::Theme) -> Color {
    super::tones::muted_status(theme.cosmic().on_bg_color().into(), theme.cosmic().is_dark)
}

/// A mark's widget, in a 16 px column so that names line up
pub(crate) fn mark_widget<'a>(mark: Mark) -> Element<'a, Message> {
    const D: f32 = 8.0;
    let dot = |filled: bool| -> Element<'a, Message> {
        widget::container(widget::Space::new())
            .width(Length::Fixed(D))
            .height(Length::Fixed(D))
            .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                background: filled.then(|| muted_green(theme).into()),
                border: Border { radius: (D / 2.0).into(), width: if filled { 0.0 } else { 1.5 }, color: muted_amber(theme) },
                ..Default::default()
            }))
            .into()
    };
    let inner: Element<'a, Message> = match mark {
        Mark::Dot => dot(true),
        Mark::Ring => dot(false),
        Mark::Spinner => spinner(12.0),
        Mark::Error => widget::icon::from_name("dialog-error-symbolic")
            .size(14)
            .icon()
            .class(theme::Svg::custom(|theme| iced::widget::svg::Style { color: Some(muted_red(theme)) }))
            .into(),
        Mark::Grey => widget::container(widget::Space::new())
            .width(Length::Fixed(D))
            .height(Length::Fixed(D))
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(muted_grey(theme).into()),
                border: Border { radius: (D / 2.0).into(), ..Default::default() },
                ..Default::default()
            }))
            .into(),
        Mark::RedDot => widget::container(widget::Space::new())
            .width(Length::Fixed(D))
            .height(Length::Fixed(D))
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(muted_red(theme).into()),
                border: Border { radius: (D / 2.0).into(), ..Default::default() },
                ..Default::default()
            }))
            .into(),
        Mark::Check => widget::icon::from_name("emblem-ok-symbolic")
            .size(14)
            .icon()
            .class(theme::Svg::custom(|theme| iced::widget::svg::Style { color: Some(muted_green(theme)) }))
            .into(),
        Mark::Blank | Mark::Word => widget::Space::new().into(),
    };
    widget::container(inner).width(Length::Fixed(16.0)).height(Length::Fixed(20.0)).center_x(Length::Fixed(16.0)).center_y(Length::Fixed(20.0)).into()
}

/// A file's icon, in the accent when it is the current row
fn file_icon<'a>(current: bool) -> Element<'a, Message> {
    if current {
        widget::icon::from_name("text-x-generic-symbolic")
            .size(14)
            .icon()
            .class(theme::Svg::custom(|theme| iced::widget::svg::Style { color: Some(theme.cosmic().accent_color().into()) }))
            .into()
    } else {
        tinted_icon("text-x-generic-symbolic", 14, shell::muted)
    }
}

/// The name of a machine by its key
fn machine_name<'a>(app: &'a App, key: &'a str) -> &'a str {
    app.links.iter().find(|l| l.target.key == key).map_or(key, |l| l.name.as_str())
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
    // A header that collapses the whole list (the open document stays in sight) or expands it
    let key = (ws.machine.clone(), ws.sid.clone());
    let collapsed = app.shell.collapsed.contains(&key);
    let header = widget::row::with_capacity(3)
        .spacing(6)
        .align_y(Alignment::Center)
        .push(tinted_icon(if collapsed { "go-next-symbolic" } else { "go-down-symbolic" }, 14, shell::muted))
        .push(widget::text::caption(format!("{} document{}", all.len(), if all.len() == 1 { "" } else { "s" })));
    col = col.push(household_widgets::tooltip_with(
        button::custom(header).width(Length::Fill).padding([2, 6]).class(nav_style(false)).on_press(Message::Shell(shell::Msg::ToggleDocuments)),
        widget::text(if collapsed { "Show all the documents" } else { "Collapse the list (the open document stays)" }),
        widget::tooltip::Position::Bottom,
    ));
    let open_path = ws.theory.as_ref().map(|t| t.path.as_str());
    // A big folder gets a filter
    if all.len() > 10 && !collapsed {
        col = col.push(
            widget::container(widget::text_input(format!("Filter {} documents", all.len()), &ws.theory_filter).on_input(Message::TheoryFilter))
                .padding(Padding { bottom: 4.0, right: 4.0, ..Padding::ZERO }),
        );
    }
    let open = ws.theory.as_ref().map(|t| t.path.as_str());
    let machine = machine_name(app, &ws.machine);
    for p in projects.iter().filter(|_| !collapsed) {
        let rel = p.strip_prefix(folder).unwrap_or(p).trim_start_matches('/');
        let is_open = open == Some(p.as_str());
        let row = widget::row::with_capacity(2).spacing(6).align_y(Alignment::Start).push(file_icon(is_open)).push(name_text(rel, 13.0, is_open));
        col = col.push(household_widgets::tooltip_with(
            button::custom(row).width(Length::Fill).padding([2, 6]).class(nav_style(is_open)).on_press(Message::OpenTheory(p.clone())),
            widget::text(format!("{rel}\n{machine}\nA project file: opens in the source view")),
            widget::tooltip::Position::Right,
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
        if !collapsed {
            col = col.push(widget::container(widget::text::caption(if all.is_empty() { "No documents here yet" } else { "No document matches" })).padding([2, 6]));
        }
    }
    for (dir, items) in groups {
        let indent = if dir.is_empty() || collapsed { 0.0 } else { 10.0 };
        // A folder's heading collapses its documents (all but the open one); a filter shows them all
        let dir_collapsed = !dir.is_empty() && filter.is_empty() && app.shell.collapsed_dirs.contains(&(ws.machine.clone(), ws.sid.clone(), dir.clone()));
        if !dir.is_empty() && !collapsed {
            let heading = widget::row::with_capacity(2)
                .spacing(4)
                .align_y(Alignment::Center)
                .push(tinted_icon(if dir_collapsed { "go-next-symbolic" } else { "go-down-symbolic" }, 12, shell::muted))
                .push(widget::text::caption(format!("{dir}{}", if dir_collapsed { format!(" ({})", items.len()) } else { String::new() })));
            col = col.push(
                widget::container(button::custom(heading).width(Length::Fill).padding([1, 4]).class(nav_style(false)).on_press(Message::Shell(shell::Msg::ToggleDocFolder(dir.clone()))))
                    .padding(Padding { top: 4.0, left: 2.0, ..Padding::ZERO }),
            );
        }
        for (name, t) in items {
            if (collapsed || dir_collapsed) && t["path"].as_str() != open_path {
                continue;
            }
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
            let is_open = open == Some(path.as_str());
            let row = widget::row::with_capacity(2)
                .spacing(6)
                .align_y(Alignment::Start)
                .push(
                    widget::container(
                        widget::container(widget::Space::new())
                            .width(Length::Fixed(7.0))
                            .height(Length::Fixed(7.0))
                            .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                                background: status.map(|s| super::view::status_color(s, theme).into()),
                                border: Border { radius: 4.0.into(), ..Default::default() },
                                ..Default::default()
                            })),
                    )
                    .height(Length::Fixed(18.0))
                    .center_y(Length::Fixed(18.0)),
                )
                .push(name_text(&format!("{name}{dirty}"), 13.0, is_open));
            col = col.push(household_widgets::tooltip_with(
                button::custom(row)
                    .width(Length::Fill)
                    .padding(Padding { left: 6.0 + indent, right: 6.0, top: 2.0, bottom: 2.0 })
                    .class(nav_style(is_open))
                    .on_press(Message::OpenTheory(path.clone())),
                widget::text(format!("{}\n{machine}", file_name(&path))),
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
            let is_open = open == Some(p.as_str());
            let row = widget::row::with_capacity(2).spacing(6).align_y(Alignment::Start).push(file_icon(is_open)).push(name_text(file_name(&p), 13.0, is_open));
            col = col.push(household_widgets::tooltip_with(
                button::custom(row).width(Length::Fill).padding([3, 6]).class(nav_style(is_open)).on_press(Message::OpenTheory(p.clone())),
                widget::text(format!("{}\n{}", file_name(&p), machine_name(app, &ws.machine))),
                widget::tooltip::Position::Right,
            ));
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
