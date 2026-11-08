// SPDX-License-Identifier: GPL-3.0-only

//! The start page: what the window shows with no session open (Tom, 8 Nov, t27 (b)). Centred, the
//! app's icon, "Deductum", one sentence and a Primary "Open a folder"; under it the sessions
//! last used as rows, the latest highlighted. A click opens a row; right-click offers Open and
//! Forget. The sidebar starts closed at launch, and the header has no chip, pause or stop while
//! no session is open.

use std::collections::HashMap;

use cosmic::iced::{Alignment, Length, Padding};
use cosmic::prelude::*;
use cosmic::widget::{self, menu};

use super::shell::Msg;
use super::sidebar::{mark_of, mark_widget, name_text, nav_style, short_folder};
use super::{App, Message};

/// Rows the page lists
const SHOWN: usize = 6;

pub const SENTENCE: &str = "Open a folder of Isabelle theories or a Lean project to start a session.";

/// One row of the list: the session, and what is remembered of its last use
#[derive(Clone, Debug)]
pub struct Row {
    pub machine: String,
    pub sid: String,
    pub name: String,
    pub folder: String,
    pub lifecycle: String,
    pub file: Option<String>,
    pub at: Option<u64>,
}

/// The sessions, the one used last first (then by when each was created); forks of a session
/// are its agents' work and not listed
pub fn rows(app: &App) -> Vec<Row> {
    let mut rows: Vec<(u64, Row)> = app
        .sessions
        .iter()
        .filter(|(_, v)| v["parent"].is_null())
        .map(|((machine, sid), v)| {
            let seen = super::recent::get(machine, sid);
            let order = seen.as_ref().map_or(0, |s| s.at).max(v["created"].as_f64().unwrap_or(0.0) as u64);
            let row = Row {
                machine: machine.clone(),
                sid: sid.clone(),
                name: v["name"].as_str().unwrap_or(sid).to_string(),
                folder: v["folder"].as_str().unwrap_or("").to_string(),
                lifecycle: v["lifecycle"].as_str().unwrap_or("").to_string(),
                file: seen.as_ref().map(|s| s.file.clone()),
                at: seen.map(|s| s.at),
            };
            (order, row)
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    rows.into_iter().take(SHOWN).map(|(_, r)| r).collect()
}

/// A menu's action is `Copy`, so the ids it carries are interned: each distinct one is kept once
fn intern(text: &str) -> &'static str {
    static SEEN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> = std::sync::LazyLock::new(Default::default);
    let mut seen = SEEN.lock().unwrap();
    match seen.get(text) {
        Some(known) => known,
        None => {
            let leaked: &'static str = Box::leak(text.to_string().into_boxed_str());
            seen.insert(leaked);
            leaked
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowAction {
    Open(&'static str, &'static str),
    Forget(&'static str, &'static str),
}

impl menu::Action for RowAction {
    type Message = Message;

    fn message(&self) -> Message {
        match self {
            RowAction::Open(m, s) => Message::OpenSession(m.to_string(), s.to_string()),
            RowAction::Forget(m, s) => Message::Shell(Msg::ForgetSession(m.to_string(), s.to_string())),
        }
    }
}

fn row_view<'a>(app: &'a App, row: &Row, latest: bool, now: u64) -> Element<'a, Message> {
    let machine_label = app.links.iter().find(|l| l.target.key == row.machine).map_or(row.machine.as_str(), |l| l.name.as_str()).to_string();
    let home = app.links.iter().find(|l| l.target.key == row.machine).and_then(|l| l.hello.as_ref()).and_then(|h| h["home"].as_str());
    let folder = short_folder(&row.folder, home);
    let caption = format!("{machine_label}  \u{b7}  {folder}  \u{b7}  {}", row.lifecycle.to_lowercase());
    let left = widget::column::with_capacity(2)
        .width(Length::Fill)
        .push(name_text(&row.name, 14.0, latest))
        .push(widget::text::caption(caption).class(super::view::muted()).wrapping(cosmic::iced::widget::text::Wrapping::None));
    let mut line = widget::row::with_capacity(3).spacing(8).align_y(Alignment::Start).push(
        widget::container(mark_widget(mark_of(&row.lifecycle, false))).width(Length::Fixed(14.0)).padding(Padding { top: 4.0, ..Padding::ZERO }),
    );
    line = line.push(left);
    if let (Some(file), Some(at)) = (&row.file, row.at) {
        line = line.push(widget::text::caption(format!("{file}  \u{b7}  {}", super::recent::ago(at, now))).class(super::view::muted()));
    }
    let button = widget::button::custom(line)
        .width(Length::Fill)
        .padding([6, 8])
        .class(nav_style(latest))
        .on_press(Message::OpenSession(row.machine.clone(), row.sid.clone()));
    let tip = household_widgets::tooltip_with(button, widget::text(format!("{machine_label}: {}", row.folder)), widget::tooltip::Position::Bottom);
    let forget = (matches!(row.lifecycle.as_str(), "Stopped" | "Failed")).then(|| menu::Item::Button("Forget", None, RowAction::Forget(intern(&row.machine), intern(&row.sid))));
    let forget = forget.unwrap_or_else(|| menu::Item::ButtonDisabled("Forget", None, RowAction::Forget(intern(&row.machine), intern(&row.sid))));
    let tree = menu::items(&HashMap::new(), vec![menu::Item::Button("Open", None, RowAction::Open(intern(&row.machine), intern(&row.sid))), forget]);
    let mut with_menu = widget::context_menu(tip, Some(tree));
    with_menu.item_width = menu::ItemWidth::Uniform(160);
    with_menu.into()
}

pub fn icon<'a>(size: u16) -> Element<'a, Message> {
    widget::icon(widget::icon::from_svg_bytes(include_bytes!("../res/dev.twright.Deductum.svg").as_slice())).size(size).into()
}

pub fn page(app: &App) -> Element<'_, Message> {
    page_at(app, super::recent::now())
}

pub fn page_at(app: &App, now: u64) -> Element<'_, Message> {
    let sp = super::view::spacing();
    let mut col = widget::column::with_capacity(8)
        .spacing(sp.space_s)
        .align_x(Alignment::Center)
        .push(icon(72))
        .push(widget::text::title3("Deductum"))
        .push(widget::text::body(SENTENCE))
        .push(widget::button::suggested("Open a folder").on_press(Message::NewSession(true)));
    let rows = rows(app);
    if !rows.is_empty() {
        let mut list = widget::column::with_capacity(rows.len() + 1).spacing(2).width(Length::Fixed(640.0));
        list = list.push(widget::container(widget::text::heading("Recent sessions")).padding(Padding { top: sp.space_s as f32, bottom: 4.0, left: 8.0, right: 0.0 }));
        for (i, row) in rows.iter().enumerate() {
            list = list.push(row_view(app, row, i == 0, now));
        }
        col = col.push(list);
    }
    widget::scrollable(widget::container(col).width(Length::Fill).padding([sp.space_xl as f32, 16.0]).center_x(Length::Fill)).into()
}
