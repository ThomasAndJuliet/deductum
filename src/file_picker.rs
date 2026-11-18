// SPDX-License-Identifier: GPL-3.0-only

//! Go to file (Tom, 18 Nov 2026): Ctrl+P (⌘P on macOS) puts a search of the session's files in
//! the header, in place of the status chip (checking goes on; pause and stop stay). Its files are
//! the sidebar's (the backend's documents and project files in the session's folder on its host).
//! It opens at once with the files known already, filtering them as typed, while the host lists
//! the folder again in the background (`project_files`, off the edit path), so a slow filesystem
//! or connection never holds it up; the selected file stays selected when that list comes in.
//! With nothing typed, the files opened most recently in the session come first (kept across
//! restarts, see `recent`), the open one left out, so Ctrl+P, Enter is the file before; typed, an
//! exact name ranks first, then names starting so, then fuzzy matches of names and paths, the
//! more recent first among equals. ↑ and ↓ select, Enter or a click opens (the picker stays,
//! saying so, until the file has opened, and shows why beside it if it cannot), Esc returns to
//! the caret, and Back returns to the file and caret it was opened from.
//!
//! The same search finds the open document's symbols (`doc_symbols`): Ctrl+Shift+O, or `g s` in
//! vim's normal mode, opens it for them, and `@` typed first in Ctrl+P's search turns it to them.
//! With nothing typed they are listed in document order, the one the caret is in selected; ↑ and
//! ↓ show each in the document as it is selected, Esc puts the caret back, and Enter or a click
//! goes there, Back returning to where the caret was.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use cosmic::Application as _;
use cosmic::app::Task;
use cosmic::iced::{Alignment, Length, widget::{scrollable, text::Wrapping}};
use cosmic::widget::{self, button, popover};
use cosmic::{Element, theme};
use household_widgets::ShortTextBox;
use serde_json::{Value, json};

use crate::editor::Mode;
use crate::doc_symbols::Symbol;
use crate::fields::{self, Field};
use crate::{App, DeferredLeave, Message, Pending};

type Scope = (String, String);
const ROW: f32 = 34.0;
static SEARCH: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);
static RESULTS: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);

#[derive(Clone, Copy, Debug)]
pub enum Key {
    Up,
    Down,
    Escape,
}

#[derive(Clone, Debug)]
struct File {
    path: String,
    name: String,
    folder: String,
    relative: String,
    lower_name: String,
    lower_path: String,
}

#[derive(Default)]
struct Index(Vec<File>);

impl Index {
    fn new<'a>(folder: &str, paths: impl Iterator<Item = &'a str>) -> Self {
        let windows = folder.as_bytes().get(1) == Some(&b':') || folder.starts_with("\\\\");
        let normal = |s: &str| if windows { s.replace('\\', "/") } else { s.to_string() };
        let prefix = format!("{}/", normal(folder).trim_end_matches('/'));
        let mut seen = HashSet::new();
        Self(paths.filter_map(|path| {
            let normal_path = normal(path);
            let relative = normal_path.strip_prefix(&prefix)?.to_string();
            if !seen.insert(relative.clone()) { return None; }
            let (folder, name) = relative.rsplit_once('/').unwrap_or(("", &relative));
            Some(File {
                name: name.into(), folder: folder.into(), lower_name: name.to_lowercase(),
                lower_path: relative.to_lowercase(), relative, path: path.into(),
            })
        }).collect())
    }

    fn rank(&self, query: &str, current: Option<&str>, recent: &[String]) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        let recency: HashMap<&str, usize> = recent.iter().enumerate()
            .map(|(i, path)| (path.as_str(), i)).collect();
        let mut matches: Vec<_> = self.0.iter().enumerate().filter_map(|(i, f)| {
            let score = if query.is_empty() {
                if current == Some(f.path.as_str()) { return None; }
                (0, 0)
            } else if f.lower_name == query || f.lower_name.rsplit_once('.').is_some_and(|(stem, _)| stem == query) {
                // (the name typed with its extension or without it)
                (3, 0)
            } else if f.lower_name.starts_with(&query) {
                (2, 0)
            } else {
                let name_score = crate::input::fuzzy(&query, &f.lower_name);
                let path_score = crate::input::fuzzy(&query, &f.lower_path);
                (1, name_score.into_iter().chain(path_score).max()?)
            };
            Some((Reverse(score), recency.get(f.path.as_str()).copied().unwrap_or(usize::MAX), &f.lower_name, &f.relative, i))
        }).collect();
        matches.sort_unstable();
        matches.into_iter().map(|(_, _, _, _, i)| i).collect()
    }
}

#[derive(Default)]
pub struct Picker {
    pub open: Option<Open>,
    indexes: HashMap<Scope, Index>,
    generation: u64,
}

pub struct Open {
    scope: Scope,
    generation: u64,
    pub query: ShortTextBox,
    current: Option<String>,
    recent: Vec<String>,
    hits: Vec<usize>,
    selected: usize,
    pub scroll: f32,
    opening: Option<String>,
    issue: Option<(String, String)>,
    refreshing: bool,
    refresh_error: Option<String>,
    /// The host has been asked for the session's files since the picker opened
    listed: bool,
    /// Opened for symbols (Ctrl+Shift+O, `g s`): the query is a symbol's name. Otherwise `@`
    /// typed first turns the search to the document's symbols
    symbols_only: bool,
    /// The open document's symbols while the search is for them, and the ones shown
    symbols: Option<SymbolSearch>,
    /// Where the caret was when the picker opened: Esc returns there after a symbol was previewed
    origin: Option<Origin>,
}

struct SymbolSearch {
    list: Vec<Symbol>,
    hits: Vec<usize>,
}

struct Origin {
    path: String,
    cursor: usize,
    selected: Option<usize>,
    previewed: bool,
}

impl Open {
    /// What is searched for among the document's symbols, if the search is for them
    fn symbol_query(&self) -> Option<&str> {
        let query = self.query.value();
        if self.symbols_only { Some(query.strip_prefix('@').unwrap_or(query)) } else { query.strip_prefix('@') }
    }

    /// How many rows are shown
    fn shown(&self) -> usize {
        self.symbols.as_ref().map_or(self.hits.len(), |s| s.hits.len())
    }

    /// The selected symbol
    fn selected_symbol(&self) -> Option<&Symbol> {
        let search = self.symbols.as_ref()?;
        search.hits.get(self.selected).map(|i| &search.list[*i])
    }

    fn selected_path<'a>(&self, index: &'a Index) -> Option<&'a str> {
        let hit = *self.hits.get(self.selected)?;
        Some(&index.0[hit].path)
    }

    fn rank(&mut self, index: &Index, keep: Option<&str>) {
        self.hits = index.rank(self.query.value(), self.current.as_deref(), &self.recent);
        self.selected = keep.and_then(|path| self.hits.iter().position(|i| index.0[*i].path == path)).unwrap_or(0);
    }

    fn reveal_selection(&mut self, window_height: f32) -> Task<Message> {
        let height = list_height(window_height, self.shown());
        let top = self.selected as f32 * ROW;
        self.scroll = self.scroll.min(top).max(top + ROW - height).max(0.0);
        scrollable::scroll_to(RESULTS.clone(), scrollable::AbsoluteOffset { x: None, y: Some(self.scroll) })
    }
}

#[cfg(test)]
impl Open {
    /// The rows shown, by path from the folder
    pub fn hits_for_tests(&self, picker: &Picker) -> Vec<String> {
        let index = &picker.indexes[&self.scope];
        self.hits.iter().map(|i| index.0[*i].relative.clone()).collect()
    }

    pub fn selected_for_tests(&self, picker: &Picker) -> Option<String> {
        self.selected_path(picker.indexes.get(&self.scope)?).map(str::to_string)
    }

    pub fn issue_for_tests(&self) -> Option<&(String, String)> {
        self.issue.as_ref()
    }

    /// The symbols shown, by name, and the selected one's, while the search is for symbols
    pub fn symbols_for_tests(&self) -> Option<(Vec<String>, Option<String>)> {
        let search = self.symbols.as_ref()?;
        Some((search.hits.iter().map(|i| search.list[*i].name.clone()).collect(), self.selected_symbol().map(|s| s.name.clone())))
    }
}

impl Picker {
    pub fn forget(&mut self, machine: &str, sid: &str) {
        self.indexes.remove(&(machine.into(), sid.into()));
    }

    pub fn owns_opening(&self, machine: &str, sid: &str, path: &str, generation: u64) -> bool {
        self.open.as_ref().is_some_and(|open| {
            open.scope.0 == machine && open.scope.1 == sid && open.generation == generation
                && open.opening.as_deref() == Some(path)
        })
    }
}

impl App {
    pub fn file_picker_available(&self) -> bool {
        self.workspace.is_some() && !self.shell.settings_open && self.new_session.is_none()
    }

    /// The search typed changed: the files or the symbols matching it, the first selected (with
    /// nothing typed for symbols: the one the caret is in)
    pub fn file_picker_query_changed(&mut self) -> Task<Message> {
        self.rank_symbols();
        let picker = &mut self.shell.file_picker;
        let Some(open) = picker.open.as_mut() else { return Task::none() };
        if open.symbols.is_none() && let Some(index) = picker.indexes.get(&open.scope) {
            open.rank(index, None);
        }
        open.scroll = 0.0;
        let to_selection = open.reveal_selection(self.window_height);
        let refresh = if open.symbols.is_none() { self.refresh_project_files() } else { Task::none() };
        Task::batch([to_selection, refresh])
    }

    /// The document's symbols again, ranked for the query, while the search is for them
    fn rank_symbols(&mut self) {
        let table = self.workspace.as_ref().map(|w| self.table(&w.machine));
        let theory = self.workspace.as_ref().and_then(|w| w.theory.as_ref());
        let Some(open) = self.shell.file_picker.open.as_mut() else { return };
        let Some(query) = open.symbol_query().map(str::to_string) else {
            if open.symbols.take().is_some() { open.selected = 0; }
            return;
        };
        let list = match (theory, &table) {
            (Some(t), Some(table)) => crate::doc_symbols::symbols(t, &**table),
            _ => Vec::new(),
        };
        let hits = crate::doc_symbols::rank(&list, &query);
        open.selected = if query.trim().is_empty() {
            let here = theory.and_then(|t| crate::doc_symbols::at_caret(&list, open.origin.as_ref().map_or(t.cursor, |o| o.cursor)));
            here.and_then(|i| hits.iter().position(|h| *h == i)).unwrap_or(0)
        } else {
            0
        };
        open.symbols = Some(SymbolSearch { list, hits });
    }

    /// Go to symbol (Ctrl+Shift+O, `g s`): the picker, searching the open document's symbols
    pub fn open_symbol_picker(&mut self) -> Task<Message> {
        if !self.file_picker_available() || self.workspace.as_ref().is_none_or(|w| w.theory.is_none()) { return Task::none(); }
        let task = self.open_file_picker_for(true);
        Task::batch([task, self.file_picker_query_changed()])
    }

    /// Go to file (Ctrl+P): the picker, searching the session's files
    pub fn open_file_picker(&mut self) -> Task<Message> {
        let switching = self.shell.file_picker.open.as_ref().is_some_and(|o| o.symbols_only);
        let task = self.open_file_picker_for(false);
        if switching { Task::batch([task, self.file_picker_query_changed()]) } else { task }
    }

    fn open_file_picker_for(&mut self, symbols: bool) -> Task<Message> {
        if !self.file_picker_available() { return Task::none(); }
        let focus = widget::text_input::focus(SEARCH.clone());
        // (open already: Ctrl+P and Ctrl+Shift+O turn it to files or symbols, the query cleared)
        if let Some(open) = self.shell.file_picker.open.as_mut() {
            if open.symbols_only != symbols {
                open.symbols_only = symbols;
                open.query = ShortTextBox::new();
            }
            return focus;
        }
        let ws = self.workspace.as_ref().unwrap();
        let scope = (ws.machine.clone(), ws.sid.clone());
        let current = ws.theory.as_ref().map(|t| t.path.clone());
        let origin = ws.theory.as_ref().map(|t| Origin { path: t.path.clone(), cursor: t.cursor, selected: t.selected, previewed: false });
        let picker = &mut self.shell.file_picker;
        if !picker.indexes.contains_key(&scope)
            && let Some(info) = &ws.info
            && let Some(folder) = info["folder"].as_str()
        {
            let paths = info["theories"].as_array().into_iter().flatten().filter_map(|entry| entry["path"].as_str());
            picker.indexes.insert(scope.clone(), Index::new(folder, paths));
        }
        picker.generation = picker.generation.wrapping_add(1);
        let mut open = Open {
            scope: scope.clone(), generation: picker.generation, query: ShortTextBox::new(), current,
            recent: crate::recent::paths(&scope.0, &scope.1), hits: Vec::new(), selected: 0, scroll: 0.0,
            opening: None, issue: None, refreshing: false, refresh_error: None, listed: false,
            symbols_only: symbols, symbols: None, origin,
        };
        if let Some(index) = picker.indexes.get(&scope) { open.rank(index, None); }
        picker.open = Some(open);
        if symbols {
            return focus;
        }
        Task::batch([focus, self.refresh_project_files()])
    }

    /// The session's files listed again by its host, in the background (one request at a time)
    fn refresh_project_files(&mut self) -> Task<Message> {
        let Some(open) = self.shell.file_picker.open.as_mut() else { return Task::none() };
        let scope = open.scope.clone();
        if open.listed {
            return Task::none();
        }
        open.listed = true;
        open.refreshing = true;
        let in_flight = self.pending.values().any(|(machine, pending)| {
            machine == &scope.0 && matches!(pending, Pending::ProjectFiles(sid) if sid == &scope.1)
        });
        if !in_flight {
            let request = self.send(&scope.0, "project_files", json!({"session": scope.1}), Pending::ProjectFiles(scope.1.clone()));
            if !self.pending.contains_key(&request)
                && let Some(open) = self.shell.file_picker.open.as_mut()
            {
                open.refreshing = false;
                open.refresh_error = Some("The project host is disconnected".into());
            }
        }
        Task::none()
    }

    pub fn project_files_reply(&mut self, machine: String, sid: String, reply: &Value) -> Task<Message> {
        let scope = (machine, sid);
        let size = self.window_height;
        let picker = &mut self.shell.file_picker;
        let open = picker.open.as_mut().filter(|picker| picker.scope == scope);
        let (Some(folder), Some(files)) = (reply["folder"].as_str(), reply["files"].as_array()) else {
            if let Some(open) = open {
                open.refreshing = false;
                open.refresh_error = Some(reply["error"].as_str().unwrap_or("Could not refresh project files").into());
            }
            return Task::none();
        };
        let keep = open.as_ref().and_then(|open| open.selected_path(picker.indexes.get(&scope)?)).map(str::to_string);
        let index = Index::new(folder, files.iter().filter_map(Value::as_str));
        if let Some(open) = open {
            open.refreshing = false;
            open.refresh_error = None;
            open.rank(&index, keep.as_deref());
            let task = open.reveal_selection(size);
            picker.indexes.insert(scope, index);
            return task;
        }
        picker.indexes.insert(scope, index);
        Task::none()
    }

    pub fn file_picker_key(&mut self, key: Key) -> Task<Message> {
        if matches!(key, Key::Escape) { return self.escape_file_picker(); }
        let Some(open) = self.shell.file_picker.open.as_mut() else { return Task::none() };
        let shown = open.shown();
        if open.opening.is_some() || shown == 0 { return Task::none(); }
        open.selected = match key {
            Key::Up => open.selected.saturating_sub(1),
            Key::Down => (open.selected + 1).min(shown - 1),
            Key::Escape => unreachable!(),
        };
        let reveal = open.reveal_selection(self.window_height);
        // A symbol selected is shown in the document; the keys stay in the search
        let Some(start) = open.selected_symbol().map(|s| s.start) else { return reveal };
        if let Some(origin) = open.origin.as_mut() { origin.previewed = true; }
        let show = self.update(Message::Goto(start));
        Task::batch([reveal, show.chain(widget::text_input::focus(SEARCH.clone()))])
    }

    /// Esc: the picker goes and the caret is where it was (back from a symbol previewed)
    fn escape_file_picker(&mut self) -> Task<Message> {
        let back = self.shell.file_picker.open.as_ref().and_then(|o| o.origin.as_ref()).filter(|o| o.previewed)
            .map(|o| (o.path.clone(), o.cursor, o.selected));
        let close = self.close_file_picker();
        let Some((path, cursor, selected)) = back else { return close };
        let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()).filter(|t| t.path == path) else { return close };
        t.selected = selected;
        Task::batch([self.update(Message::Goto(cursor)), close])
    }

    /// Enter, or a click on a symbol's row (by where it starts): the caret goes to the symbol, and
    /// Back returns to where it was
    pub fn choose_symbol(&mut self, clicked: Option<usize>) -> Task<Message> {
        let Some(open) = self.shell.file_picker.open.as_ref() else { return Task::none() };
        let Some(start) = clicked.or_else(|| open.selected_symbol().map(|s| s.start)) else { return Task::none() };
        let origin = open.origin.as_ref().map(|o| (o.path.clone(), o.cursor));
        if let Some((path, cursor)) = origin
            && let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).filter(|t| t.path == path)
        {
            self.back.push((path, Self::host_offset(t, cursor)));
        }
        self.dismiss_file_picker();
        self.update(Message::Goto(start))
    }

    pub fn choose_project_file(&mut self, clicked: Option<String>) -> Task<Message> {
        if clicked.is_none() && self.shell.file_picker.open.as_ref().is_some_and(|o| o.symbols.is_some()) {
            return self.choose_symbol(None);
        }
        let picker = &mut self.shell.file_picker;
        let Some(open) = picker.open.as_mut().filter(|open| open.opening.is_none()) else { return Task::none() };
        let Some(index) = picker.indexes.get(&open.scope) else { return Task::none() };
        if let Some(path) = clicked {
            let Some(row) = open.hits.iter().position(|i| index.0[*i].path == path) else { return Task::none() };
            open.selected = row;
        }
        let Some(path) = open.selected_path(index).map(str::to_string) else { return Task::none() };
        if open.current.as_deref() == Some(path.as_str()) { return self.close_file_picker(); }
        open.issue = None;
        open.opening = Some(path.clone());
        let generation = open.generation;
        self.open_picked_file(path, generation)
    }

    pub fn close_file_picker(&mut self) -> Task<Message> {
        if !self.dismiss_file_picker() { return Task::none(); }
        match self.workspace.as_ref().and_then(|ws| ws.theory.as_ref()) {
            Some(t) if t.mode == Mode::Source => widget::text_input::focus(crate::view::source_editor_id()),
            Some(t) if t.block_editor.is_some() && self.popout.is_none() => widget::text_input::focus(crate::view::block_editor_id()),
            _ => Self::unfocus_all(),
        }
    }

    pub fn dismiss_file_picker(&mut self) -> bool {
        let Some(open) = self.shell.file_picker.open.take() else { return false };
        if matches!(&self.deferred_leave, Some(DeferredLeave::PickFile { generation, .. }) if *generation == open.generation) {
            self.deferred_leave = None;
            if self.status.as_deref() == Some("Waiting for edits to reach the host before switching documents") {
                self.status = None;
            }
        }
        if let Some(path) = open.opening
            && let Some(ws) = self.workspace.as_mut().filter(|ws| ws.machine == open.scope.0 && ws.sid == open.scope.1)
            && ws.opening.as_deref() == Some(&path)
        {
            ws.opening = None;
            ws.pending_goto = None;
        }
        true
    }

    pub fn file_picker_failed(&mut self, error: String) -> Task<Message> {
        if let Some(open) = self.shell.file_picker.open.as_mut() && let Some(path) = open.opening.take() {
            open.issue = Some((path.clone(), error));
            if let Some(ws) = self.workspace.as_mut().filter(|ws| ws.opening.as_ref() == Some(&path)) {
                ws.opening = None;
                ws.pending_goto = None;
            }
            return widget::text_input::focus(SEARCH.clone());
        }
        Task::none()
    }
}

fn list_height(window_height: f32, count: usize) -> f32 {
    ((window_height - 110.0).clamp(ROW, ROW * 9.0)).min(count.max(1) as f32 * ROW)
}

pub fn header(app: &App) -> Option<Element<'_, Message>> {
    let picker = &app.shell.file_picker;
    let open = picker.open.as_ref().filter(|_| app.file_picker_available())?;
    let field_width = (app.window_width - 415.0).clamp(95.0, 340.0);
    let placeholder = match app.workspace.as_ref().and_then(|w| w.theory.as_ref()) {
        Some(t) if open.symbols_only => format!("Go to a symbol in {}", crate::file_name(&t.path)),
        _ => "Search project files, or @ for symbols".to_string(),
    };
    let field = fields::search(&open.query, Field::ProjectFiles, placeholder, |field| {
        field.id(SEARCH.clone()).width(Length::Fixed(field_width)).on_submit(Message::FilePickerAccept(None))
    });
    let mut rows = widget::column::with_capacity(16);
    if let Some(search) = &open.symbols {
        rows = symbol_rows(app, open, search, rows);
    } else if let Some(index) = picker.indexes.get(&open.scope) && !open.hits.is_empty() {
        let len = open.hits.len();
        let height = list_height(app.window_height, len);
        let start = ((open.scroll / ROW) as usize).min(len.saturating_sub(1)).saturating_sub(2);
        let end = (start + (height / ROW).ceil() as usize + 5).min(len);
        rows = rows.push(widget::Space::new().height(Length::Fixed(start as f32 * ROW)));
        for (row, hit) in open.hits[start..end].iter().enumerate() {
            let file = &index.0[*hit];
            let issue = open.issue.as_ref().filter(|(path, _)| path == &file.path);
            let selected = start + row == open.selected;
            // (the sidebar's file rows: the selected one with the wash, its name bold in the accent)
            let mut row_label = widget::row::with_capacity(3).align_y(Alignment::Center).spacing(10).height(Length::Fill)
                .push(widget::text(file.name.as_str()).wrapping(Wrapping::None).size(14).font(crate::sidebar::row_font(selected)))
                .push(widget::text::caption(file.folder.as_str()).wrapping(Wrapping::None)
                    .class(crate::view::muted()).width(Length::Fill));
            if let Some((_, detail)) = issue {
                row_label = row_label.push(widget::text::caption(detail.as_str()).wrapping(Wrapping::None)
                    .class(theme::Text::Custom(|t| cosmic::iced::widget::text::Style { color: Some(crate::sidebar::muted_red(t)), ..Default::default() })));
            } else if open.opening.as_ref() == Some(&file.path) {
                row_label = row_label.push(widget::text::caption("Opening\u{2026}").class(crate::view::muted()));
            }
            let row = button::custom(row_label).width(Length::Fill).height(Length::Fixed(ROW)).padding([0, 10])
                .class(crate::sidebar::nav_style(selected))
                .on_press_maybe(open.opening.is_none().then(|| Message::FilePickerAccept(Some(file.path.clone()))));
            rows = rows.push(row);
        }
        rows = rows.push(widget::Space::new().height(Length::Fixed((len - end) as f32 * ROW)));
    } else {
        let message = if open.refreshing { "Loading project files…" } else { "No matching project files" };
        rows = rows.push(widget::container(widget::text::caption(message).class(crate::view::muted())).padding(10));
    }
    let error = open.refresh_error.as_deref().filter(|_| open.symbols.is_none());
    let mut content = widget::column::with_capacity(2).push(widget::scrollable(rows).id(RESULTS.clone())
        .height(Length::Fixed(list_height(app.window_height, open.shown())))
        .on_scroll(|v| Message::FilePickerScroll(v.absolute_offset().y)));
    if let Some(error) = error {
        content = content.push(widget::container(widget::text::caption(error)).padding(8));
    }
    if let Some((path, error)) = open.issue.as_ref().filter(|_| open.symbols.is_none())
        && !picker.indexes.get(&open.scope).is_some_and(|index| open.hits.iter().any(|i| index.0[*i].path == *path))
    {
        content = content.push(widget::container(widget::text::caption(error.as_str())).padding(8));
    }
    let popup = widget::container(content).width(Length::Fixed((app.window_width - 24.0).clamp(100.0, 560.0)))
        .padding(4).class(theme::Container::custom(|theme| {
            let c = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: Some(c.background(false).component.base.into()),
                border: cosmic::iced::Border { color: c.bg_divider().into(), width: 1.0, radius: c.corner_radii.radius_m.into() },
                ..Default::default()
            }
        }));
    // (a little below the field, so that the list does not cover its border)
    let popup = widget::container(popup).padding(cosmic::iced::Padding { top: 4.0, ..cosmic::iced::Padding::ZERO });
    Some(widget::popover(field).popup(popup).position(popover::Position::Bottom).on_close(Message::FilePickerClose).into())
}

/// The rows of the document's symbols: a heading indented by its level, a formal block with its
/// status (the Theorems list's dots), each with the command that makes it
fn symbol_rows<'a>(app: &App, open: &'a Open, search: &'a SymbolSearch, mut rows: widget::Column<'a, Message, cosmic::Theme>) -> widget::Column<'a, Message, cosmic::Theme> {
    let len = search.hits.len();
    if len == 0 {
        let why = if app.workspace.as_ref().is_some_and(|w| w.theory.is_none()) {
            "Open a document to go to its symbols"
        } else if search.list.is_empty() {
            "This document has no headings, statements or definitions yet"
        } else {
            "No matching symbols"
        };
        return rows.push(widget::container(widget::text::caption(why).class(crate::view::muted())).padding(10));
    }
    let height = list_height(app.window_height, len);
    let start = ((open.scroll / ROW) as usize).min(len.saturating_sub(1)).saturating_sub(2);
    let end = (start + (height / ROW).ceil() as usize + 5).min(len);
    rows = rows.push(widget::Space::new().height(Length::Fixed(start as f32 * ROW)));
    for (row, i) in search.hits[start..end].iter().enumerate() {
        let symbol = &search.list[*i];
        let selected = start + row == open.selected;
        let mark: Element<'a, Message> = match symbol.status {
            Some(status) => widget::container(widget::Space::new()).width(Length::Fixed(8.0)).height(Length::Fixed(8.0))
                .class(theme::Container::custom(move |theme| cosmic::iced::widget::container::Style {
                    background: Some(crate::view::status_color(status, theme).into()),
                    border: cosmic::iced::Border { radius: 4.0.into(), ..Default::default() },
                    ..Default::default()
                }))
                .into(),
            None => widget::Space::new().width(Length::Fixed(0.0)).into(),
        };
        // (a section's title where the dots are; a subsection and below indented under it)
        let indent = symbol.level.map_or(0.0, |l| 12.0 * l.saturating_sub(2).min(4) as f32);
        let label = widget::row::with_capacity(4).align_y(Alignment::Center).spacing(10).height(Length::Fill)
            .push_maybe((indent > 0.0).then(|| widget::Space::new().width(Length::Fixed(indent))))
            .push_maybe(symbol.status.is_some().then_some(mark))
            .push(widget::text(symbol.name.as_str()).wrapping(Wrapping::None).size(14).font(crate::sidebar::row_font(selected)))
            .push(widget::text::caption(symbol.kind.as_str()).wrapping(Wrapping::None).class(crate::view::muted()).width(Length::Fill));
        let row = button::custom(label).width(Length::Fill).height(Length::Fixed(ROW)).padding([0, 10])
            .class(crate::sidebar::nav_style(selected))
            .on_press(Message::FilePickerSymbol(symbol.start));
        rows = rows.push(row);
    }
    rows.push(widget::Space::new().height(Length::Fixed((len - end) as f32 * ROW)))
}

#[cfg(test)]
mod tests {
    use super::Index;

    fn index() -> Index {
        let paths = ["/p/ROOT", "/p/Demo.thy", "/p/More.thy", "/p/lib/Base.thy", "/p/lib/Demo_Lemmas.thy", "/p/Other/Moreover.thy", "/p/Ademo.thy"];
        Index::new("/p", paths.into_iter())
    }

    fn names(index: &Index, query: &str, current: Option<&str>, recent: &[&str]) -> Vec<String> {
        let recent: Vec<String> = recent.iter().map(|p| p.to_string()).collect();
        index.rank(query, current, &recent).into_iter().map(|i| index.0[i].relative.clone()).collect()
    }

    #[test]
    fn with_nothing_typed_the_recent_files_come_first_then_the_rest_by_name_and_never_the_open_one() {
        let index = index();
        assert_eq!(
            names(&index, "", Some("/p/Demo.thy"), &["/p/lib/Base.thy", "/p/Demo.thy", "/p/ROOT"]),
            ["lib/Base.thy", "ROOT", "Ademo.thy", "lib/Demo_Lemmas.thy", "More.thy", "Other/Moreover.thy"],
        );
        // (with no history: by name)
        assert_eq!(names(&index, "", None, &[])[..3], ["Ademo.thy", "lib/Base.thy", "Demo.thy"]);
    }

    #[test]
    fn an_exact_name_ranks_first_then_names_starting_so_then_fuzzy_names_and_paths() {
        let index = index();
        // (Demo with or without .thy is exact; Demo_Lemmas starts with it; Ademo only contains it)
        assert_eq!(names(&index, "demo", None, &[]), ["Demo.thy", "lib/Demo_Lemmas.thy", "Ademo.thy"]);
        assert_eq!(names(&index, "Demo.thy", None, &[])[0], "Demo.thy");
        // the open file is found when it is searched for
        assert_eq!(names(&index, "demo", Some("/p/Demo.thy"), &[])[0], "Demo.thy");
        // a path: the folder typed as well
        assert_eq!(names(&index, "lib/ba", None, &[]), ["lib/Base.thy"]);
        // a subsequence of the name
        assert_eq!(names(&index, "dml", None, &[]), ["lib/Demo_Lemmas.thy"]);
        assert!(names(&index, "zzz", None, &[]).is_empty());
    }

    #[test]
    fn among_equal_matches_the_more_recently_opened_comes_first() {
        let index = index();
        assert_eq!(names(&index, "more", None, &[]), ["More.thy", "Other/Moreover.thy"]);
        assert_eq!(names(&index, "mor", None, &["/p/Other/Moreover.thy"]), ["Other/Moreover.thy", "More.thy"]);
    }

    #[test]
    fn files_outside_the_folder_and_repeats_are_left_out() {
        let index = Index::new("/p/", ["/p/A.thy", "/q/B.thy", "/p/A.thy"].into_iter());
        assert_eq!(index.0.iter().map(|f| f.relative.as_str()).collect::<Vec<_>>(), ["A.thy"]);
        let windows = Index::new("C:\\proofs", ["C:\\proofs\\lib\\A.thy"].into_iter());
        assert_eq!((windows.0[0].relative.as_str(), windows.0[0].folder.as_str()), ("lib/A.thy", "lib"));
    }
}
