// SPDX-License-Identifier: GPL-3.0-only

//! The sidebar: every running session this terminal made, on this computer
//! and on the other machines, grouped by project, with a little of what each
//! is doing. Each host is scanned with `project-dirs.py sessions` in turn,
//! then again a few seconds after its answer, while the sidebar is shown.

use cosmic::{
    Element,
    iced::{Alignment, Length},
    style, theme,
    widget::{self},
};
use cosmic::widget::menu::{action::MenuAction, items, key_bind::KeyBind};
use std::{
    collections::{BTreeMap, HashMap},
    hash::{Hash, Hasher},
};

use crate::{
    Message, TabMark,
    agent::{self, SessionSummary},
    config::{ProfileId, TabColor},
    fl, icon_cache_get, time_ago,
};

/// How long after a host's answer it is scanned again
pub const REFRESH: std::time::Duration = std::time::Duration::from_secs(10);

pub const WIDTH: f32 = 300.0;

pub struct Sidebar {
    /// By host: empty for this computer, else an ssh destination
    pub hosts: BTreeMap<String, Host>,
    /// Bumped each time the sidebar is shown, so that the scans of an
    /// earlier showing stop
    pub generation: u64,
    pub filter: String,
    pub filter_id: widget::Id,
    /// The filter has the focus, so it keeps it
    pub searching: bool,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            hosts: BTreeMap::new(),
            generation: 0,
            filter: String::new(),
            filter_id: crate::field_id("sidebar-filter"),
            searching: false,
        }
    }
}

#[derive(Default)]
pub struct Host {
    /// This computer, or the machine's name
    pub label: String,
    pub summaries: Option<Result<Vec<SessionSummary>, String>>,
    pub scanning: bool,
}

/// What a row's context menu does
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Open,
    /// Close its tab in this window, leaving the session running
    CloseTab,
    End,
    /// Ask for a name for the conversation
    Rename,
    /// Have the agent name the conversation
    RenameAuto,
    MoveTo(ProfileId),
    ForkTo(ProfileId),
}

/// An [`Action`] on the session with this [`session_key`]. Menu actions
/// must be `Copy`, and a key stays right when the list is refreshed
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowAction(pub u64, pub Action);

impl MenuAction for RowAction {
    type Message = Message;

    fn message(&self) -> Message {
        Message::SidebarAction(self.0, self.1)
    }
}

pub fn session_key(session: &agent::Session) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    session.hash(&mut hasher);
    hasher.finish()
}

/// One session, as a row
struct Row<'a> {
    session: agent::Session,
    host: &'a str,
    summary: &'a SessionSummary,
}

impl Row<'_> {
    fn agent(&self) -> Option<&agent::AgentSummary> {
        self.summary.agents.first()
    }

    fn title(&self) -> String {
        if let Some(agent) = self.agent().filter(|a| !a.title.is_empty()) {
            return agent.title.clone();
        }
        if let Some(command) = &self.summary.command {
            return command.clone();
        }
        self.session.label()
    }

    /// What it is, where, and on which branch
    fn detail(&self) -> String {
        let mut parts = vec![self.session.agent().map_or_else(|| fl!("agent-shell"), |a| a.label())];
        parts.push(self.host.to_string());
        if let Some(cwd) = &self.summary.cwd {
            let folder = cwd.rsplit('/').next().unwrap_or(cwd);
            // A worktree's folder says which one it is
            let repository = self.summary.repository.as_deref();
            if repository.is_some_and(|r| r != cwd) || self.summary.branch.is_none() {
                parts.push(folder.to_string());
            }
        }
        if let Some(branch) = &self.summary.branch {
            parts.push(branch.clone());
        }
        parts.join(" · ")
    }

    /// A little of what it is doing: an agent's latest message, or a
    /// terminal's last lines
    fn snippet(&self) -> Option<String> {
        match self.agent() {
            Some(agent) => Some(agent.last.clone()).filter(|s| !s.is_empty()),
            None => {
                let lines = self.screen_lines();
                (!lines.is_empty()).then(|| lines.join("\n"))
            }
        }
    }

    /// A terminal's last two lines of text, leaving out what full-screen
    /// programs draw: borders, bars and graphs
    fn screen_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.summary.screen.iter().rev().filter_map(|l| readable(l)).take(2).collect();
        lines.reverse();
        lines
    }

    /// Whether every word of the filter is in what the row shows
    fn matches(&self, project: &str, words: &[String]) -> bool {
        let text = format!(
            "{} {} {} {} {}",
            self.title(),
            self.detail(),
            project,
            self.snippet().unwrap_or_default(),
            self.session.label()
        )
        .to_lowercase();
        words.iter().all(|word| text.contains(word.as_str()))
    }

    fn status(&self) -> Option<(String, bool)> {
        match self.agent() {
            Some(agent) if agent.state == "working" => Some((fl!("sidebar-working"), true)),
            Some(_) => Some((fl!("sidebar-ready"), false)),
            None if self.summary.command.is_some() => Some((fl!("sidebar-running"), true)),
            None => None,
        }
    }
}

/// The group a session goes in: its repository, else its folder
fn project_of(summary: &SessionSummary) -> String {
    summary
        .repository
        .as_deref()
        .or(summary.cwd.as_deref())
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .map_or_else(|| fl!("sidebar-no-project"), str::to_string)
}

impl Sidebar {
    /// The rows the filter matches, by project: the most recently active
    /// first, in each project and among projects
    fn groups(&self) -> Vec<(String, Vec<Row<'_>>)> {
        let words: Vec<String> = self
            .filter
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let mut groups: BTreeMap<String, Vec<Row<'_>>> = BTreeMap::new();
        for (host, state) in &self.hosts {
            let Some(Ok(summaries)) = &state.summaries else {
                continue;
            };
            for summary in summaries {
                let project = project_of(summary);
                let row = Row {
                    session: agent::Session {
                        host: host.clone(),
                        name: summary.name.clone(),
                    },
                    host: &state.label,
                    summary,
                };
                if row.matches(&project, &words) {
                    groups.entry(project).or_default().push(row);
                }
            }
        }
        let latest = |rows: &[Row]| {
            rows.iter()
                .filter_map(|row| row.summary.active_at())
                .fold(0.0, f64::max)
        };
        let mut groups: Vec<(String, Vec<Row>)> = groups.into_iter().collect();
        for (_, rows) in &mut groups {
            rows.sort_by(|a, b| {
                let (a, b) = (a.summary.active_at(), b.summary.active_at());
                b.unwrap_or(0.0).total_cmp(&a.unwrap_or(0.0))
            });
        }
        groups.sort_by(|a, b| latest(&b.1).total_cmp(&latest(&a.1)));
        groups
    }

    /// The session with a [`session_key`], and what was found of it
    pub fn find(&self, key: u64) -> Option<(agent::Session, &SessionSummary)> {
        self.hosts.iter().find_map(|(host, state)| {
            let Some(Ok(summaries)) = &state.summaries else {
                return None;
            };
            summaries.iter().find_map(|summary| {
                let session = agent::Session {
                    host: host.clone(),
                    name: summary.name.clone(),
                };
                (session_key(&session) == key).then_some((session, summary))
            })
        })
    }

    /// The title of a session's conversation, if it has one
    /// A session's title, by its name alone, on whichever machine runs it
    pub fn title_by_name(&self, name: &str) -> Option<&str> {
        self.hosts.values().find_map(|host| {
            let Some(Ok(summaries)) = &host.summaries else {
                return None;
            };
            summaries
                .iter()
                .find(|s| s.name == name)?
                .agents
                .first()
                .map(|agent| agent.title.as_str())
                .filter(|title| !title.is_empty())
        })
    }

    pub fn title(&self, session: &agent::Session) -> Option<&str> {
        let Some(Ok(summaries)) = &self.hosts.get(&session.host)?.summaries else {
            return None;
        };
        summaries
            .iter()
            .find(|s| s.name == session.name)?
            .agents
            .first()
            .map(|agent| agent.title.as_str())
            .filter(|title| !title.is_empty())
    }

    /// Show a conversation's new name until the next scan finds it
    pub fn set_title(&mut self, session: &agent::Session, title: &str) {
        if let Some(Some(Ok(summaries))) = self.hosts.get_mut(&session.host).map(|h| h.summaries.as_mut())
            && let Some(agent) = summaries
                .iter_mut()
                .find(|s| s.name == session.name)
                .and_then(|s| s.agents.first_mut())
        {
            agent.title = title.to_string();
        }
    }

    /// A session's preview, as its sidebar row shows it, for its tab
    pub fn tab_preview(&self, session: &agent::Session, mark: TabMark) -> Option<Element<'_, Message>> {
        let state = self.hosts.get(&session.host)?;
        let Some(Ok(summaries)) = &state.summaries else {
            return None;
        };
        let summary = summaries.iter().find(|s| s.name == session.name)?;
        let row = Row {
            session: session.clone(),
            host: &state.label,
            summary,
        };
        let spacing = theme::active().cosmic().spacing;
        // The bubble a compact row's tooltip is, which the tooltip widget
        // makes of its content with this class and padding. Its width is
        // bounded, as an overlay offers the whole window, which a preview
        // that fills would take, to be placed by its edge
        let bubble = widget::container(
            widget::container(preview(&row, mark, true, false))
                .padding([spacing.space_xs, spacing.space_s]),
        )
        .class(cosmic::theme::Container::Tooltip)
        .padding(spacing.space_xxs);
        // A gap below the tabs; the bound is on the outermost, as a preview
        // that fills makes the containers around it fill too
        Some(
            widget::container(bubble)
                .padding([4, 0, 0, 0])
                .max_width(WIDTH + 60.0 + 2.0 * f32::from(spacing.space_xxs))
                .into(),
        )
    }

    /// The session listed first
    pub fn first_match(&self) -> Option<agent::Session> {
        let groups = self.groups();
        let (_, rows) = groups.first()?;
        rows.first().map(|row| row.session.clone())
    }

    /// `open` are the sessions with a tab in this window, `active` the one
    /// in front, and `mark` how a session's tab is marked
    #[allow(clippy::too_many_arguments)]
    pub fn view<'a>(
        &'a self,
        open: &[agent::Session],
        active: Option<&agent::Session>,
        menu: Option<Element<'a, Message>>,
        mark: impl Fn(&agent::Session) -> TabMark,
        destinations: impl Fn(&agent::Session) -> Vec<(String, ProfileId)>,
        window_id: Option<cosmic::iced::window::Id>,
        compact: bool,
        locked: impl Fn(&agent::Session) -> bool,
    ) -> Element<'a, Message> {
        let spacing = theme::active().cosmic().spacing;
        let mut column = widget::column::with_capacity(16)
            .spacing(spacing.space_xs)
            .padding([spacing.space_xs, spacing.space_xs]);
        // A split button, as in the header: New opens the default profile,
        // and the arrow chooses a profile or session
        let new = widget::button::custom(
            widget::row::with_children(vec![
                icon_cache_get("list-add-symbolic", 16).into(),
                widget::text::body(fl!("sidebar-new")).into(),
            ])
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center),
        )
        .on_press(Message::TabNew)
        .class(style::Button::Standard)
        .padding([spacing.space_xxs, spacing.space_s])
        .width(Length::Fill);
        let arrow = widget::tooltip(
            widget::button::custom(icon_cache_get("pan-down-symbolic", 16))
                .on_press(Message::SidebarNewMenu)
                .class(style::Button::Standard)
                .padding([spacing.space_xxs, spacing.space_xxs]),
            widget::text::caption(fl!("sidebar-new-menu")),
            widget::tooltip::Position::Bottom,
        );
        let mut popover = widget::popover(arrow)
            .on_close(Message::NewTabMenuClose)
            .position(widget::popover::Position::Bottom);
        if let Some(menu) = menu {
            popover = popover.popup(menu);
        }
        column = column
            .push(
                widget::row::with_children(vec![new.into(), popover.into()])
                    .spacing(spacing.space_xxxs)
                    .align_y(Alignment::Center),
            )
            .push(
                widget::row::with_children(vec![
                    widget::text_input::search_input(fl!("sidebar-search"), &self.filter)
                        .id(self.filter_id.clone())
                        .on_input(Message::SidebarFilter)
                        .on_paste(Message::SidebarFilter)
                        .on_clear(Message::SidebarFilter(String::new()))
                        .on_focus(Message::SidebarSearching(true))
                        .on_unfocus(Message::SidebarSearching(false))
                        .on_submit(|_| Message::SidebarFilterSubmit)
                        .into(),
                    // Full previews, or one line per session
                    widget::tooltip(
                        widget::button::custom(icon_cache_get(
                            if compact {
                                "view-expanded-symbolic"
                            } else {
                                "view-compact-symbolic"
                            },
                            16,
                        ))
                        .on_press(Message::SidebarCompact(!compact))
                        .padding(spacing.space_xxs)
                        .class(style::Button::Icon),
                        widget::text::caption(if compact {
                            fl!("sidebar-expand")
                        } else {
                            fl!("sidebar-compact")
                        }),
                        widget::tooltip::Position::Bottom,
                    )
                    .into(),
                ])
                .spacing(spacing.space_xxxs)
                .align_y(Alignment::Center),
            );

        // New and the search stay put above the list, which scrolls
        let header = column;
        let mut column = widget::column::with_capacity(16)
            .spacing(spacing.space_xs)
            .padding([0, spacing.space_xs, spacing.space_xs, spacing.space_xs]);
        let mut notes = Vec::new();
        for state in self.hosts.values() {
            match &state.summaries {
                Some(Err(error)) => notes.push(fl!(
                    "sidebar-failed",
                    host = state.label.as_str(),
                    error = error.as_str()
                )),
                None => notes.push(fl!("sidebar-loading", host = state.label.as_str())),
                Some(Ok(_)) => {}
            }
            if state.scanning && state.summaries.is_some() {
                notes.push(fl!("sidebar-refreshing", host = state.label.as_str()));
            }
        }
        for note in notes {
            column = column.push(widget::text::caption(note));
        }
        let groups = self.groups();

        let loaded = self
            .hosts
            .values()
            .any(|host| matches!(host.summaries, Some(Ok(_))));
        if groups.is_empty() && loaded {
            column = column.push(widget::text::caption(if self.filter.trim().is_empty() {
                fl!("sidebar-empty")
            } else {
                fl!("sidebar-no-matches")
            }));
        }
        for (project, rows) in groups {
            let mut heading = widget::row::with_capacity(3)
                .align_y(Alignment::Center)
                .push(widget::text::heading(project.clone()).width(Length::Fill));
            // New sessions go where the project was used last, with its
            // latest agent
            if let Some(row) = rows.iter().find(|row| row.summary.repository.is_some()) {
                let new = |worktree: bool| Message::SidebarNew {
                    host: row.session.host.clone(),
                    repository: row.summary.repository.clone().unwrap_or_default(),
                    agent: rows.iter().find_map(|row| row.session.agent()),
                    worktree,
                };
                let color = mark(&row.session).color;
                heading = heading
                    .push(group_button(
                        "list-add-symbolic",
                        color,
                        fl!("sidebar-new-session", project = project.as_str()),
                        new(false),
                    ))
                    .push(group_button(
                        "worktree-new-symbolic",
                        color,
                        fl!("sidebar-new-worktree", project = project.as_str()),
                        new(true),
                    ));
            }
            let mut group = widget::column::with_capacity(rows.len() + 1)
                .spacing(spacing.space_xxxs)
                .push(heading);
            for row in rows {
                let here = open.contains(&row.session);
                let in_front = active == Some(&row.session);
                let row_menu = context_menu(&row, here, &destinations(&row.session));
                let mut entry = widget::context_menu(
                    row_view(&row, mark(&row.session), here, in_front, compact, locked(&row.session)),
                    Some(row_menu),
                )
                .on_surface_action(Message::Surface);
                if let Some(window_id) = window_id {
                    entry = entry.window_id(window_id);
                }
                group = group.push(entry);
            }
            column = column.push(group);
        }

        // Painted like the tab bar, since the window may be translucent
        widget::container(
            widget::column::with_children(vec![
                header.into(),
                widget::scrollable(column).height(Length::Fill).into(),
            ]),
        )
            .width(Length::Fixed(WIDTH))
            .height(Length::Fill)
            .class(style::Container::Custom(Box::new(|theme| {
                let cosmic = theme.cosmic();
                let background = cosmic.background(theme.transparent);
                widget::container::Style {
                    icon_color: Some(background.on.into()),
                    text_color: Some(background.on.into()),
                    background: Some(cosmic::iced::Background::Color(background.base.into())),
                    ..Default::default()
                }
            })))
            .into()
    }
}

/// Whether a character is drawing rather than text: box drawing, blocks,
/// braille dots, as full-screen programs use for borders, bars and graphs
fn drawing(c: char) -> bool {
    matches!(c, '\u{2500}'..='\u{259f}' | '\u{2800}'..='\u{28ff}' | '\u{25a0}'..='\u{25ff}')
}

/// A screen line as text, without the drawing at its ends and with runs of
/// spaces shortened, unless it is more drawing than letters and digits
fn readable(line: &str) -> Option<String> {
    let words = line.chars().filter(|c| c.is_alphanumeric()).count();
    if words == 0 || line.chars().filter(|c| drawing(*c)).count() > words {
        return None;
    }
    let line = line.trim_matches(|c: char| c.is_whitespace() || drawing(c) || "|+-=_.:".contains(c));
    let mut out = String::with_capacity(line.len());
    let mut spaces = 0;
    for c in line.chars() {
        if c.is_whitespace() {
            spaces += 1;
            if spaces <= 2 {
                out.push(' ');
            }
        } else {
            spaces = 0;
            out.push(if drawing(c) { ' ' } else { c });
        }
    }
    Some(out)
}

/// A row's full preview: its heading, what and where it is, and a little
/// of what it is doing
fn preview<'a>(row: &Row<'a>, mark: TabMark, here: bool, locked: bool) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let mut body = widget::column::with_capacity(3)
        .spacing(spacing.space_xxxs)
        .push(heading_view(row, mark, locked));
    let mut detail = row.detail();
    // Attached in another window, or on another computer
    if !here && row.summary.clients > 0 {
        detail.push_str(" · ");
        detail.push_str(&fl!("sidebar-open-elsewhere"));
    }
    body = body.push(widget::text::caption(detail));
    let faint = |text: String| {
        widget::text::caption(text).width(Length::Fill).class(theme::Text::Custom(|theme| {
            let mut on = theme.cosmic().background(theme.transparent).on;
            on.alpha = 0.7;
            cosmic::iced::widget::text::Style {
                color: Some(on.into()),
                ..Default::default()
            }
        }))
    };
    use cosmic::iced::advanced::text::{Ellipsize, EllipsizeHeightLimit};
    if row.agent().is_some() {
        if let Some(snippet) = row.snippet() {
            body = body.push(faint(snippet).ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(3))));
        }
    } else {
        // A screen's lines as it shows them, each cut short rather than
        // wrapped
        for line in row.screen_lines() {
            body = body.push(
                faint(line)
                    .font(cosmic::font::mono())
                    .wrapping(cosmic::iced::advanced::text::Wrapping::None)
                    .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1))),
            );
        }
    }
    body.into()
}

/// A row's icon, title, status and time
fn heading_view<'a>(row: &Row<'a>, mark: TabMark, locked: bool) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let color = mark.strip_color();
    // A computer or server, in the tab's colour, as on the tab itself
    let icon = mark.icon(16).unwrap_or_else(|| icon_cache_get("computer-symbolic", 16));
    let mut heading = widget::row::with_capacity(4)
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center)
        .push(icon)
        .push(widget::text::body(row.title()).width(Length::Fill));
    // Waiting for the user's sudo approval, else what it is doing
    if locked {
        heading = heading.push(widget::tooltip(
            icon_cache_get("dialog-password-symbolic", 16),
            widget::text::caption(fl!("sidebar-sudo-waiting")),
            widget::tooltip::Position::Top,
        ));
    } else if let Some((status, busy)) = row.status() {
        heading = heading.push(status_dot(busy, color, status));
    }
    heading
        .push(widget::text::caption(
            row.summary.active_at().map(time_ago).unwrap_or_default(),
        ))
        .into()
}

/// A row: its full preview, or in compact mode its heading alone, with the
/// preview shown on hover
fn row_view<'a>(
    row: &Row<'a>,
    mark: TabMark,
    here: bool,
    in_front: bool,
    compact: bool,
    locked: bool,
) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let color = mark.strip_color();
    let content: Element<_> = if compact {
        heading_view(row, mark, locked)
    } else {
        preview(row, mark, here, locked)
    };
    let button = widget::button::custom(content)
        .on_press(Message::SidebarOpen(row.session.clone()))
        .class(row_style(color, in_front))
        .padding(spacing.space_xxs)
        .width(Length::Fill);
    let button: Element<_> = if compact {
        widget::tooltip(
            button,
            widget::container(preview(row, mark, here, locked))
                .max_width(WIDTH + 60.0)
                .padding([spacing.space_xs, spacing.space_s]),
            widget::tooltip::Position::Right,
        )
        .into()
    } else {
        button.into()
    };
    // A strip in the tab's colour, as above its pane, and the row in front
    // tinted with it
    let strip = widget::container(widget::space::vertical())
        .width(Length::Fixed(3.0))
        .height(Length::Fill)
        .class(style::Container::Custom(Box::new(move |_theme| {
            widget::container::Style {
                background: color.map(|c| cosmic::iced::Background::Color(rgb(c, 1.0))),
                border: cosmic::iced::Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })));
    widget::row::with_children(vec![strip.into(), button])
        .spacing(spacing.space_xxxs)
        .height(Length::Shrink)
        .into()
}

/// A row's button: a rounded rectangle, washed with the tab's colour when it
/// is in front, hovered or pressed. Without a colour the wash is of the text
/// colour, so it shows on light and dark themes alike
fn row_style(color: Option<TabColor>, in_front: bool) -> style::Button {
    let style = move |strength: f32, theme: &cosmic::Theme| {
        let cosmic = theme.cosmic();
        let on = cosmic.background(theme.transparent).on;
        let wash = match color {
            Some(c) => rgb(c, strength),
            None => {
                let mut on: cosmic::iced::Color = on.into();
                on.a = strength * 0.6;
                on
            }
        };
        cosmic::widget::button::Style {
            shadow_offset: cosmic::iced::Vector::default(),
            background: (strength > 0.0).then_some(cosmic::iced::Background::Color(wash)),
            overlay: None,
            border_radius: cosmic.corner_radii.radius_s.into(),
            border_width: 0.0,
            border_color: cosmic::iced::Color::TRANSPARENT,
            outline_width: 0.0,
            outline_color: cosmic::iced::Color::TRANSPARENT,
            icon_color: None,
            text_color: Some(on.into()),
        }
    };
    let rest = if in_front { 0.2 } else { 0.0 };
    style::Button::Custom {
        active: Box::new(move |_focused, theme| style(rest, theme)),
        disabled: Box::new(move |theme| style(rest, theme)),
        hovered: Box::new(move |_focused, theme| style(rest + 0.1, theme)),
        pressed: Box::new(move |_focused, theme| style(rest + 0.18, theme)),
    }
}

/// A row's menu: as a tab's, for a session that may have none here
fn context_menu(
    row: &Row,
    here: bool,
    destinations: &[(String, ProfileId)],
) -> Vec<widget::menu::Tree<Message>> {
    use widget::menu::Item;
    let key = session_key(&row.session);
    let item = |label: String, action: Action| Item::Button(label, None, RowAction(key, action));
    let mut menu = vec![item(fl!("sidebar-open"), Action::Open), Item::Divider];
    if here {
        menu.push(item(fl!("detach-session"), Action::CloseTab));
    }
    menu.push(item(fl!("end-remote-session"), Action::End));
    // The agents rename their conversations themselves
    match row.session.agent() {
        Some(crate::config::Agent::Claude) => {
            menu.push(Item::Divider);
            menu.push(item(fl!("sidebar-rename"), Action::Rename));
            menu.push(item(fl!("sidebar-rename-auto"), Action::RenameAuto));
        }
        Some(crate::config::Agent::Codex) => {
            menu.push(Item::Divider);
            menu.push(item(fl!("sidebar-rename"), Action::Rename));
        }
        _ => {}
    }
    // An agent's conversation, and its worktree, to another computer
    let conversation = row.agent().and_then(|agent| agent.conversation.as_ref());
    if conversation.is_some() && !destinations.is_empty() {
        menu.push(Item::Divider);
        for (name, profile_id) in destinations {
            menu.push(item(fl!("move-to", profile = name.as_str()), Action::MoveTo(*profile_id)));
        }
        for (name, profile_id) in destinations {
            menu.push(item(fl!("fork-to", profile = name.as_str()), Action::ForkTo(*profile_id)));
        }
    }
    items(&HashMap::<KeyBind, RowAction>::new(), menu)
}

/// A project's + or new-worktree button, turning the project's colour, else
/// the accent, when hovered
fn group_button<'a>(
    icon: &'static str,
    color: Option<TabColor>,
    tooltip: String,
    message: Message,
) -> Element<'a, Message> {
    let style = move |lit: bool, theme: &cosmic::Theme| {
        let cosmic = theme.cosmic();
        let on: cosmic::iced::Color = cosmic.background(theme.transparent).on.into();
        let highlight = color.map_or_else(|| cosmic.accent_color().into(), |c| rgb(c, 1.0));
        let mut wash = highlight;
        wash.a = 0.15;
        cosmic::widget::button::Style {
            shadow_offset: cosmic::iced::Vector::default(),
            background: lit.then_some(cosmic::iced::Background::Color(wash)),
            overlay: None,
            border_radius: cosmic.corner_radii.radius_s.into(),
            border_width: 0.0,
            border_color: cosmic::iced::Color::TRANSPARENT,
            outline_width: 0.0,
            outline_color: cosmic::iced::Color::TRANSPARENT,
            icon_color: Some(if lit { highlight } else { on }),
            text_color: Some(if lit { highlight } else { on }),
        }
    };
    widget::tooltip(
        widget::button::custom(icon_cache_get(icon, 16))
            .on_press(message)
            .padding(4)
            .class(style::Button::Custom {
                active: Box::new(move |_, theme| style(false, theme)),
                disabled: Box::new(move |theme| style(false, theme)),
                hovered: Box::new(move |_, theme| style(true, theme)),
                pressed: Box::new(move |_, theme| style(true, theme)),
            }),
        widget::text::caption(tooltip),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

fn rgb(TabColor([r, g, b]): TabColor, alpha: f32) -> cosmic::iced::Color {
    cosmic::iced::Color::from_rgba8(r, g, b, alpha)
}

/// A dot in the tab's colour, else the accent, while it works; grey when it
/// waits
fn status_dot<'a>(busy: bool, color: Option<TabColor>, status: String) -> Element<'a, Message> {
    widget::tooltip(
        widget::container(widget::space::horizontal().width(Length::Fixed(8.0)))
            .height(Length::Fixed(8.0))
            .class(style::Container::Custom(Box::new(move |theme| {
                let cosmic = theme.cosmic();
                let color = match color {
                    Some(c) if busy => rgb(c, 1.0),
                    _ if busy => cosmic.accent_color().into(),
                    _ => cosmic.palette.neutral_6.into(),
                };
                widget::container::Style {
                    background: Some(cosmic::iced::Background::Color(color)),
                    border: cosmic::iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            }))),
        widget::text::caption(status),
        widget::tooltip::Position::Top,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::readable;

    #[test]
    fn screen_lines_keep_text_and_drop_drawing() {
        // From btop, as a preview showed it
        assert_eq!(readable("|205K ⣿⣿ ⣿⣿ ⣿⣿ ⣿ ⣿⣿ ⣿⣿ ⣿ ⣿⣿ ⣿ ⣿⣿⣿⣿ ⣿⣿ ⣿"), None);
        assert_eq!(readable("└───────────────────────────────────────┘"), None);
        assert_eq!(
            readable("│ 4089 gluetun-entrypo /gluetun-entrypoint        29 root  30M │").as_deref(),
            Some("4089 gluetun-entrypo /gluetun-entrypoint  29 root  30M")
        );
        // A prompt is mostly signs, and still worth showing
        assert_eq!(readable("λ ~/ ~> ").as_deref(), Some("λ ~/ ~>"));
        assert_eq!(readable("cargo build --release").as_deref(), Some("cargo build --release"));
    }
}
