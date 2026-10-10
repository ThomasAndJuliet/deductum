// SPDX-License-Identifier: GPL-3.0-only

//! The app's side of threads: opening them, routing their messages, keeping
//! within the memory limits, and the windows their images open in.

use std::path::PathBuf;
use std::time::Duration;

use cosmic::app::Task;
use cosmic::ApplicationExt;
use cosmic::iced::{clipboard, window};
use cosmic::widget::{pane_grid, segmented_button};
use cosmic_files::dialog::{Dialog, DialogKind, DialogResult, DialogSettings};

use super::tab::{Msg, Request, ThreadId, ThreadTab};
use super::viewer::{ImageWindow, Zoom};
use crate::config::{PaperFont, PaperTone, ProfileId};
use crate::{App, Message, TabMark, ThreadSetting};

/// Where a thread runs, for another beside it
pub(crate) struct ThreadOrigin {
    pub profile_id: Option<ProfileId>,
    pub host: String,
    pub cwd: String,
    pub args: String,
    pub agent: crate::config::Agent,
}

impl ThreadOrigin {
    pub(crate) fn of(tab: &ThreadTab) -> Self {
        Self {
            profile_id: tab.profile_id,
            host: tab.host.clone(),
            cwd: tab.cwd.clone(),
            args: tab.args.clone(),
            agent: tab.agent,
        }
    }
}

impl App {
    /// Every thread tab, with its pane and whether it is shown
    pub(crate) fn thread_tabs(&self) -> Vec<(pane_grid::Pane, segmented_button::Entity, bool)> {
        let mut out = Vec::new();
        for (pane, tab_model) in self.pane_model.panes.iter() {
            let active = tab_model.active();
            for entity in tab_model.iter() {
                if tab_model.data::<ThreadTab>(entity).is_some() {
                    out.push((*pane, entity, entity == active));
                }
            }
        }
        out
    }

    /// A new tab in `pane`: beside a thread, another in the same place; else
    /// a thread, a launcher or a terminal, as the default profile says
    pub(crate) fn new_tab_in(&mut self, pane: pane_grid::Pane, from: Option<ThreadOrigin>) -> Task<Message> {
        if let Some(from) = from {
            let open = self.open_thread_as(pane, from.profile_id, from.cwd.clone(), None, None, Some(from.agent), Some(from.host));
            if let Some(tab) = self.thread_mut(self.next_thread - 1) {
                tab.args = from.args;
            }
            return open;
        }
        if let Some(profile_id) = self.get_default_profile()
            && let Some(dir) = self.thread_profile_dir(profile_id)
        {
            return self.open_thread(pane, Some(profile_id), dir, None, None);
        }
        if let Some(profile_id) = self.get_default_profile()
            && self.wants_launcher(Some(profile_id))
        {
            return self.open_launcher(pane, profile_id);
        }
        self.create_and_focus_new_terminal(pane, self.get_default_profile(), self.config.tab_new_inherit_working_directory)
    }

    /// Move a thread's tab to another pane, with its conversation and agent
    pub(crate) fn move_thread(
        &mut self,
        from: pane_grid::Pane,
        entity: segmented_button::Entity,
        to: pane_grid::Pane,
    ) -> Task<Message> {
        let Some(source) = self.pane_model.panes.get_mut(from) else { return Task::none() };
        let Some(slot) = source.data_mut::<ThreadTab>(entity) else { return Task::none() };
        let tab = std::mem::replace(slot, ThreadTab::new(ThreadId::MAX, None, String::new(), String::new(), String::new()));
        let mark = source.data::<TabMark>(entity).cloned();
        let title = source.text(entity).unwrap_or_default().to_string();
        source.remove(entity);
        if source.iter().next().is_none() {
            // The pane it leaves empty goes too
            if let Some((_, sibling)) = self.pane_model.panes.close(from) {
                self.pane_model.set_focus(sibling);
            }
        }
        let composer_id = tab.composer_id.clone();
        let Some(target) = self.pane_model.panes.get_mut(to) else { return Task::none() };
        let mut builder = target.insert().text(title);
        if let Some(icon) = mark.as_ref().and_then(|mark| mark.icon(16)) {
            builder = builder.icon(icon);
        }
        let moved = builder.closable().activate().id();
        if let Some(mark) = mark {
            target.data_set::<TabMark>(moved, mark);
        }
        target.data_set::<ThreadTab>(moved, tab);
        self.pane_model.set_focus(to);
        Task::batch([self.update_title(Some(to)), cosmic::widget::text_input::focus(composer_id)])
    }

    /// Open a thread's relay session in `pane` as a thread: from what the
    /// sidebar knows of it, else from its meta.json on its host
    pub(crate) fn open_thread_session(&mut self, pane: pane_grid::Pane, session: crate::agent::Session) -> Task<Message> {
        let known = self.sidebar.find(crate::sidebar::session_key(&session)).and_then(|(_, summary)| {
            let thread = summary.thread.clone()?;
            let cwd = thread
                .meta
                .get("cwd")
                .and_then(|c| c.as_str())
                .map(str::to_string)
                .or_else(|| summary.cwd.clone())
                .unwrap_or_default();
            Some((cwd, thread.conversation))
        });
        match known {
            Some((cwd, conversation)) => self.open_thread_attached(pane, session, cwd, conversation),
            None => cosmic::task::future(async move {
                let meta = crate::agent::thread_meta(session.clone()).await;
                Message::ThreadSessionFound(pane, session, meta)
            }),
        }
    }

    /// A thread tab in `pane` joined to the relay of `session`
    pub(crate) fn open_thread_attached(
        &mut self,
        pane: pane_grid::Pane,
        session: crate::agent::Session,
        cwd: String,
        conversation: Option<String>,
    ) -> Task<Message> {
        let agent = session.agent().unwrap_or(crate::config::Agent::Claude);
        let profile_id = self.profile_on_host(&session.host, Some(agent), true);
        let id = self.next_thread;
        let has_history = conversation.is_some();
        let host = session.host.clone();
        let opened = self.open_thread_as(pane, profile_id, cwd, conversation, None, Some(agent), Some(host));
        let mut attach = Task::none();
        if let Some(tab) = self.thread_mut(id) {
            tab.relay = Some(session);
            tab.relay_live = true;
            // After its history, or now if it has none yet
            tab.attach_on_load = has_history;
            if !has_history {
                attach = tab.attach();
            }
        }
        Task::batch([opened, attach])
    }

    /// Tell every thread what the view shows of thinking and tool calls
    pub(crate) fn sync_thread_shown(&mut self) {
        let shown = super::tab::Shown { thinking: self.config.thread_show_thinking, tools: self.config.thread_show_tools };
        for id in self.thread_ids() {
            if let Some(tab) = self.thread_mut(id) {
                tab.shown = shown;
            }
        }
    }

    /// Every thread, by id
    pub(crate) fn thread_ids(&self) -> Vec<ThreadId> {
        self.pane_model
            .panes
            .iter()
            .flat_map(|(_, tab_model)| tab_model.iter().filter_map(|e| tab_model.data::<ThreadTab>(e).map(|t| t.id)))
            .collect()
    }

    /// Stop the idle agents of threads working in or around `dir` on `host`,
    /// which would keep a transfer of that worktree from starting; they
    /// start again, resuming, at the next message
    pub(crate) fn stop_idle_threads_in(&mut self, host: &str, dir: &str) -> Task<Message> {
        let dir = std::path::Path::new(dir);
        let mut ended = Vec::new();
        for id in self.thread_ids() {
            if let Some(tab) = self.thread_mut(id)
                && tab.host == host
                && (std::path::Path::new(&tab.cwd).starts_with(dir) || dir.starts_with(&tab.cwd))
            {
                ended.push(tab.end_cli());
            }
        }
        Task::batch(ended)
    }

    pub(crate) fn thread(&self, id: ThreadId) -> Option<&ThreadTab> {
        self.pane_model.panes.iter().find_map(|(_, tab_model)| {
            tab_model.iter().find_map(|entity| tab_model.data::<ThreadTab>(entity).filter(|t| t.id == id))
        })
    }

    pub(crate) fn thread_mut(&mut self, id: ThreadId) -> Option<&mut ThreadTab> {
        let (pane, entity) = self.pane_model.panes.iter().find_map(|(pane, tab_model)| {
            tab_model
                .iter()
                .find(|entity| tab_model.data::<ThreadTab>(*entity).is_some_and(|t| t.id == id))
                .map(|entity| (*pane, entity))
        })?;
        self.pane_model.panes.get_mut(pane)?.data_mut::<ThreadTab>(entity)
    }

    /// The thread in front in the focused pane
    pub(crate) fn active_thread(&self) -> Option<&ThreadTab> {
        let tab_model = self.pane_model.active()?;
        tab_model.data::<ThreadTab>(tab_model.active())
    }

    fn active_thread_id(&self) -> Option<ThreadId> {
        self.active_thread().map(|t| t.id)
    }

    /// Open a thread tab for an agent profile, in place of `replace` if
    /// given: in `cwd`, continuing conversation `resume` if given
    pub(crate) fn open_thread(
        &mut self,
        pane: pane_grid::Pane,
        profile_id: Option<ProfileId>,
        cwd: String,
        resume: Option<String>,
        replace: Option<segmented_button::Entity>,
    ) -> Task<Message> {
        self.open_thread_as(pane, profile_id, cwd, resume, replace, None, None)
    }

    /// [`Self::open_thread`], with another agent or machine than the
    /// profile's, as a thread restored or opened beside another has
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open_thread_as(
        &mut self,
        pane: pane_grid::Pane,
        profile_id: Option<ProfileId>,
        cwd: String,
        resume: Option<String>,
        replace: Option<segmented_button::Entity>,
        agent: Option<crate::config::Agent>,
        host: Option<String>,
    ) -> Task<Message> {
        let mut profile = profile_id.and_then(|id| self.config.profiles.get(&id)).cloned().unwrap_or_default();
        if let Some(agent) = agent {
            profile.agent = agent;
        }
        // Without one, a Claude thread, marked as Claude's are
        if profile.agent == crate::config::Agent::Shell {
            profile.agent = crate::config::Agent::Claude;
        }
        let host = host.unwrap_or_else(|| profile.host.trim().to_string());
        let mark = TabMark { color: profile.tab_color(), remote: profile.is_remote(), disconnected: false };
        let id = self.next_thread;
        self.next_thread += 1;
        let turns = usize::from(self.config.threads.history_turns.max(1));
        let (mut tab, load) = match resume {
            Some(session) => ThreadTab::resuming(id, profile_id, profile.agent, host, cwd, profile.args.clone(), session, turns),
            None => {
                let mut tab = ThreadTab::new(id, profile_id, host, cwd, profile.args.clone());
                tab.set_agent(profile.agent);
                (tab, Task::none())
            }
        };
        tab.turns = turns;
        tab.colours = self.thread_colours();
        tab.shown = super::tab::Shown { thinking: self.config.thread_show_thinking, tools: self.config.thread_show_tools };
        let title = tab.title.clone();
        let composer_id = tab.composer_id.clone();
        self.pane_model.set_focus(pane);
        let Some(tab_model) = self.pane_model.panes.get_mut(pane) else { return Task::none() };
        let position = replace.and_then(|entity| tab_model.position(entity));
        if let Some(entity) = replace {
            tab_model.remove(entity);
        }
        let mut builder = tab_model.insert().text(title);
        if let Some(icon) = mark.icon(16) {
            builder = builder.icon(icon);
        }
        let entity = builder.closable().activate().id();
        if let Some(position) = position {
            tab_model.position_set(entity, position);
        }
        tab_model.data_set::<TabMark>(entity, mark);
        tab_model.data_set::<ThreadTab>(entity, tab);
        Task::batch([
            load,
            self.update_title(Some(pane)),
            cosmic::widget::text_input::focus(composer_id),
            self.enforce_thread_limits(),
        ])
    }

    /// Route a thread's message, and do what it asks of the app
    pub(crate) fn thread_message(&mut self, id: ThreadId, msg: Msg) -> Task<Message> {
        let mut requests = Vec::new();
        let busy_before = self.thread(id).is_some_and(ThreadTab::has_cli);
        let colours = self.thread_colours();
        // After its controls the keys go back to the composer, as they do
        // to a CLI's prompt
        let refocus = matches!(
            msg,
            Msg::Send | Msg::Stop | Msg::SetModel(_) | Msg::SetMode(_) | Msg::Answer(..) | Msg::Attached(_) | Msg::Detach(_)
        );
        let Some(tab) = self.thread_mut(id) else { return Task::none() };
        tab.colours = colours;
        let task = tab.update(msg, &mut requests);
        let started = !busy_before && tab.has_cli();
        let mut tasks = vec![task];
        if refocus {
            tasks.push(cosmic::widget::text_input::focus(tab.composer_id.clone()));
        }
        for request in requests {
            match request {
                Request::Copy(text) => tasks.push(clipboard::write(text)),
                Request::PickImages(id) => tasks.push(self.pick_images(id)),
                Request::Retitle => tasks.push(self.retitle_thread(id)),
            }
        }
        if started {
            tasks.push(self.enforce_thread_limits());
        }
        Task::batch(tasks)
    }

    /// Name a thread's tab after its first message, once it has one
    fn retitle_thread(&mut self, id: ThreadId) -> Task<Message> {
        let Some(tab) = self.thread(id).filter(|tab| !tab.named) else { return Task::none() };
        // The name it was given, before its first message
        if let Some(title) = tab.conversation_title.clone() {
            return self.set_thread_title(id, title);
        }
        let Some(first) = tab.entries.as_ref().and_then(|entries| {
            entries.items.iter().find_map(|item| match &item.kind {
                super::model::ItemKind::User { text, .. } => Some(text.clone()),
                _ => None,
            })
        }) else {
            return Task::none();
        };
        let line = first.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string();
        let mut title: String = line.chars().take(48).collect();
        if line.chars().count() > 48 {
            title.push('…');
        }
        if title.is_empty() {
            return Task::none();
        }
        self.set_thread_title(id, title)
    }

    /// Rename a thread: its tab, and its conversation where the agent keeps
    /// names, as Claude's `/rename` does
    pub(crate) fn rename_thread_to(&mut self, id: ThreadId, name: String) -> Task<Message> {
        if name.is_empty() {
            return self.update_focus();
        }
        let Some(tab) = self.thread_mut(id) else { return Task::none() };
        tab.named = true;
        let record = (tab.agent == crate::config::Agent::Claude)
            .then(|| tab.session_id.clone().map(|session| (tab.host.clone(), session)))
            .flatten();
        let retitle = self.set_thread_title(id, name.clone());
        let save = match record {
            Some((host, session)) => {
                let line = serde_json::json!({"type": "custom-title", "customTitle": name, "sessionId": session}).to_string();
                let script = format!(
                    "for f in \"$HOME\"/.claude/projects/*/{}.jsonl; do [ -f \"$f\" ] && printf '%s\\n' {} >> \"$f\"; done\n",
                    crate::agent::sh_quote(&session),
                    crate::agent::sh_quote(&line),
                );
                cosmic::iced::Task::future(async move {
                    if let Err(err) = crate::agent::run(crate::agent::session_exec(&host, &script)).await {
                        log::warn!("saving the thread's name: {err}");
                    }
                })
                .discard()
            }
            None => Task::none(),
        };
        Task::batch([retitle, save, self.update_focus()])
    }

    /// Name a thread's tab, and the window after it if it is in front
    pub(crate) fn set_thread_title(&mut self, id: ThreadId, title: String) -> Task<Message> {
        for (pane, entity, _) in self.thread_tabs() {
            if let Some(tab_model) = self.pane_model.panes.get_mut(pane)
                && let Some(tab) = tab_model.data_mut::<ThreadTab>(entity)
                && tab.id == id
            {
                tab.title = title.clone();
                tab_model.text_set(entity, title);
                return self.update_title(None);
            }
        }
        Task::none()
    }

    /// Keep within the limits: unload threads not shown beyond
    /// `loaded_threads`, stop agents idle for `idle_minutes` or beyond
    /// `live_agents`, and load threads that are shown but unloaded
    pub(crate) fn enforce_thread_limits(&mut self) -> Task<Message> {
        let settings = self.config.threads.clone();
        let tabs = self.thread_tabs();
        let mut tasks = Vec::new();
        // Shown threads come back
        for (pane, entity, shown) in &tabs {
            if *shown
                && let Some(tab) = self.pane_model.panes.get_mut(*pane).and_then(|m| m.data_mut::<ThreadTab>(*entity))
            {
                tasks.push(tab.shown());
            }
        }
        let collect = |app: &Self| -> Vec<(pane_grid::Pane, segmented_button::Entity, bool, std::time::Instant, std::time::Instant, bool, bool, bool)> {
            tabs.iter()
                .filter_map(|(pane, entity, shown)| {
                    let tab = app.pane_model.panes.get(*pane)?.data::<ThreadTab>(*entity)?;
                    Some((*pane, *entity, *shown, tab.last_shown, tab.last_active, tab.is_loaded(), tab.has_cli(), tab.is_working()))
                })
                .collect()
        };
        // Unload the least recently shown, keeping the shown ones
        let mut loaded: Vec<_> = collect(self).into_iter().filter(|t| t.5 && !t.2 && !t.7).collect();
        loaded.sort_by_key(|t| std::cmp::Reverse(t.3));
        for t in loaded.into_iter().skip(usize::from(settings.loaded_threads)) {
            if let Some(tab) = self.pane_model.panes.get_mut(t.0).and_then(|m| m.data_mut::<ThreadTab>(t.1)) {
                tab.unload();
            }
        }
        // Stop idle agents
        let idle = Duration::from_secs(u64::from(settings.idle_minutes) * 60);
        let mut live: Vec<_> = collect(self).into_iter().filter(|t| t.6).collect();
        live.sort_by_key(|t| std::cmp::Reverse(t.4));
        let keep = usize::from(settings.live_agents.max(1));
        for (n, t) in live.into_iter().enumerate() {
            if !t.7 && (t.4.elapsed() > idle || n >= keep)
                && let Some(tab) = self.pane_model.panes.get_mut(t.0).and_then(|m| m.data_mut::<ThreadTab>(t.1))
            {
                tab.stop_cli();
            }
        }
        Task::batch(tasks)
    }

    /// The memory threads hold, for the settings page: entries, formulas
    pub(crate) fn thread_memory(&self) -> (usize, usize, usize) {
        let mut loaded = 0;
        let mut bytes = 0;
        let mut agents = 0;
        for (pane, entity, _) in self.thread_tabs() {
            if let Some(tab) = self.pane_model.panes.get(pane).and_then(|m| m.data::<ThreadTab>(entity)) {
                loaded += usize::from(tab.is_loaded());
                agents += usize::from(tab.has_cli());
                bytes += tab.weight();
            }
        }
        (loaded, agents, bytes + super::math::cache_usage().0 + super::diagram::cache_usage().0)
    }

    fn pick_images(&mut self, id: ThreadId) -> Task<Message> {
        if self.dialog_opt.is_some() {
            return Task::none();
        }
        let (dialog, command) = Dialog::new(
            DialogSettings::new().kind(DialogKind::OpenMultipleFiles),
            |msg| Message::DialogMessage(Box::new(msg)),
            move |result| Message::ThreadImagesPicked(id, result),
        );
        self.dialog_opt = Some(dialog);
        command
    }

    pub(crate) fn images_picked(&mut self, id: ThreadId, result: DialogResult) -> Task<Message> {
        self.dialog_opt = None;
        if let DialogResult::Open(paths) = result {
            return self.thread_message(id, Msg::Attached(paths));
        }
        Task::none()
    }

    /// Copy the thread's selection, if the active tab is a thread with one
    pub(crate) fn thread_copy(&self) -> Option<Task<Message>> {
        let text = self.active_thread()?.selected_text()?;
        Some(clipboard::write(text))
    }

    /// Keys no widget took go to the thread's composer, as every key goes to
    /// a terminal agent's prompt: text is typed into it, and Ctrl+V pastes
    /// there. Gives `None` for keys it leaves to the rest of the app
    pub(crate) fn thread_type(
        &mut self,
        modifiers: cosmic::iced::keyboard::Modifiers,
        key: &cosmic::iced::keyboard::Key,
        text: Option<&str>,
    ) -> Option<Task<Message>> {
        use cosmic::widget::text_editor::{Action, Edit};
        if self.dialog_opt.is_some()
            || self.rename.is_some()
            || self.core.window.show_context
            || self.find
            || self.sidebar.searching
            || self.new_tab_menu_open
        {
            return None;
        }
        let (id, composer_id) = self.active_thread().map(|t| (t.id, t.composer_id.clone()))?;
        let focus = cosmic::widget::text_input::focus(composer_id);
        if modifiers.control() || modifiers.alt() || modifiers.logo() {
            let paste = modifiers.control()
                && !modifiers.alt()
                && matches!(key, cosmic::iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("v"));
            if !paste {
                return None;
            }
            let read = clipboard::read().map(move |text| match text {
                Some(text) => cosmic::Action::App(Message::Thread(
                    id,
                    Msg::Edit(Action::Edit(Edit::Paste(std::sync::Arc::new(text)))),
                )),
                None => cosmic::Action::None,
            });
            return Some(Task::batch([focus, read]));
        }
        let text = text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control))?;
        let tab = self.thread_mut(id)?;
        for c in text.chars() {
            tab.composer.perform(Action::Edit(Edit::Insert(c)));
        }
        Some(focus)
    }

    /// Find the search box's text in the thread in front, and scroll to it
    pub(crate) fn thread_find(&mut self, forward: bool, typing: bool) -> Task<Message> {
        let needle = self.find_search_value.clone();
        let Some(id) = self.active_thread().map(|t| t.id) else { return Task::none() };
        let Some(tab) = self.thread_mut(id) else { return Task::none() };
        if typing {
            // Typing more of the word keeps the match where it is
            if let Some((start, _)) = tab.selection {
                tab.selection = Some((start, start));
            }
        }
        let Some(at) = tab.find(&needle, forward) else { return Task::none() };
        // The match in the middle of the view, once the thread's height is
        // known
        if tab.content_height > tab.page_height && tab.page_height > 0.0 {
            let y = (at * tab.content_height - tab.page_height / 2.0).clamp(0.0, tab.content_height - tab.page_height);
            return cosmic::iced::widget::scrollable::scroll_to(
                tab.scroll_id.clone(),
                cosmic::iced::widget::scrollable::AbsoluteOffset { x: None, y: Some(y) },
            );
        }
        cosmic::iced::widget::scrollable::snap_to(
            tab.scroll_id.clone(),
            cosmic::iced::widget::scrollable::RelativeOffset { x: None, y: Some(at) },
        )
    }

    pub(crate) fn thread_select_all(&mut self) {
        if let Some(id) = self.active_thread_id()
            && let Some(tab) = self.thread_mut(id)
        {
            tab.select_all();
        }
    }

    /// The mouse button came up: drags end, and the selection becomes the
    /// primary selection, as in a terminal
    pub(crate) fn thread_drag_ended(&mut self) -> Option<Task<Message>> {
        let mut text = None;
        let active = self.active_thread_id();
        for (pane, entity, _) in self.thread_tabs() {
            if let Some(tab) = self.pane_model.panes.get_mut(pane).and_then(|m| m.data_mut::<ThreadTab>(entity))
                && tab.dragging
            {
                tab.dragging = false;
                if Some(tab.id) == active {
                    text = tab.selected_text();
                }
            }
        }
        text.map(clipboard::write_primary)
    }

    pub(crate) fn open_diagram(&mut self, source: String) -> Task<Message> {
        // On white or sepia paper, in the paper's colours
        let style = super::view::ThreadStyle::new(&self.config, &cosmic::theme::active());
        let paper = style.page.map(|_| style.body.diagram_palette());
        let diagram = ImageWindow::diagram(source, paper);
        let (id, open) = window::open(window::Settings {
            size: diagram.initial_size(),
            min_size: Some(cosmic::iced::Size::new(560.0, 240.0)),
            decorations: true,
            ..Default::default()
        });
        let title = diagram.title();
        self.image_windows.insert(id, diagram);
        Task::batch([open.map(|_| cosmic::Action::None), self.set_window_title(title, id)])
    }

    pub(crate) fn open_image(&mut self, path: PathBuf) -> Task<Message> {
        let image = ImageWindow::new(path);
        let (id, open) = window::open(window::Settings {
            size: image.initial_size(),
            min_size: Some(cosmic::iced::Size::new(560.0, 240.0)),
            decorations: true,
            ..Default::default()
        });
        let title = image.title();
        self.image_windows.insert(id, image);
        Task::batch([open.map(|_| cosmic::Action::None), self.set_window_title(title, id)])
    }

    pub(crate) fn image_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ImageZoom(id, zoom) => {
                if let Some(window) = self.image_windows.get_mut(&id) {
                    window.zoom(zoom);
                }
            }
            Message::Wheel(id, delta) => {
                if self.modifiers.control()
                    && let Some(window) = self.image_windows.get_mut(&id)
                {
                    let lines = match delta {
                        cosmic::iced::mouse::ScrollDelta::Lines { y, .. } => y,
                        cosmic::iced::mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
                    };
                    window.zoom(Zoom::Scroll(lines));
                }
            }
            Message::ImageCopyPath(id) => {
                if let Some(window) = self.image_windows.get(&id) {
                    return clipboard::write(window.path.display().to_string());
                }
            }
            Message::ImageOpenExternally(id) => {
                if let Some(window) = self.image_windows.get(&id)
                    && let Err(err) = open::that_detached(&window.path)
                {
                    log::warn!("failed to open {}: {err}", window.path.display());
                }
            }
            Message::ImageWindowClosed(id) => {
                self.image_windows.remove(&id);
            }
            _ => {}
        }
        Task::none()
    }

    pub(crate) fn thread_setting(&mut self, setting: ThreadSetting) -> Task<Message> {
        use crate::*;
        let mut threads = self.config.threads.clone();
        let pick = |list: &[u16], index: usize, current: u16| list.get(index).copied().unwrap_or(current);
        match setting {
            ThreadSetting::TextSize(i) => threads.text_size = pick(&THREAD_TEXT_SIZES, i, threads.text_size),
            ThreadSetting::PaperFont(i) => threads.paper_font = PaperFont::ALL.get(i).copied().unwrap_or_default(),
            ThreadSetting::PaperSize(i) => threads.paper_size = pick(&PAPER_SIZES, i, threads.paper_size),
            ThreadSetting::PaperLineHeight(i) => {
                threads.paper_line_height = pick(&PAPER_LINE_HEIGHTS, i, threads.paper_line_height)
            }
            ThreadSetting::PaperMeasure(i) => threads.paper_measure = pick(&PAPER_MEASURES, i, threads.paper_measure),
            ThreadSetting::PaperJustify(on) => threads.paper_justify = on,
            ThreadSetting::PaperTone(i) => threads.paper_tone = PaperTone::ALL.get(i).copied().unwrap_or_default(),
            ThreadSetting::MathScale(i) => threads.math_scale = pick(&MATH_SCALES, i, threads.math_scale),
            ThreadSetting::IdleMinutes(i) => threads.idle_minutes = pick(&IDLE_MINUTES, i, threads.idle_minutes),
            ThreadSetting::LiveAgents(i) => threads.live_agents = pick(&LIVE_AGENTS, i, threads.live_agents),
            ThreadSetting::LoadedThreads(i) => threads.loaded_threads = pick(&LOADED_THREADS, i, threads.loaded_threads),
        }
        self.save_threads_config(threads);
        self.enforce_thread_limits()
    }

    /// Threads' text a pixel larger or smaller, or with `None` its usual
    /// size: the paper's when threads are set as papers
    pub(crate) fn zoom_threads(&mut self, step: Option<i16>) {
        let mut threads = self.config.threads.clone();
        let defaults = crate::config::ThreadSettings::default();
        let (size, default) = if self.config.thread_paper {
            (&mut threads.paper_size, defaults.paper_size)
        } else {
            (&mut threads.text_size, defaults.text_size)
        };
        *size = match step {
            Some(step) => size.saturating_add_signed(step).clamp(9, 40),
            None => default,
        };
        self.save_threads_config(threads);
    }

    pub(crate) fn save_threads_config(&mut self, threads: crate::config::ThreadSettings) {
        match &self.config_handler {
            Some(handler) => {
                if let Err(err) = self.config.set_threads(handler, threads) {
                    log::warn!("failed to save thread settings: {err}");
                }
            }
            None => self.config.threads = threads,
        }
    }

    /// The colours threads are drawn in, for the agent's images to match,
    /// unless that is off
    pub(crate) fn thread_colours(&self) -> Option<super::claude::Colours> {
        if !self.config.thread_match_colours {
            return None;
        }
        let theme = cosmic::theme::active();
        let style = super::view::ThreadStyle::new(&self.config, &theme);
        let background = style.page.unwrap_or_else(|| theme.cosmic().bg_color().into());
        let css = |color: cosmic::iced::Color| {
            let [r, g, b, _] = color.into_rgba8();
            format!("#{r:02x}{g:02x}{b:02x}")
        };
        Some(super::claude::Colours {
            dark: background.relative_luminance() < 0.25,
            background: css(background),
            text: css(style.body.text),
            accent: css(style.body.link),
        })
    }

    pub(crate) fn set_thread_paper(&mut self, paper: bool) {
        match &self.config_handler {
            Some(handler) => {
                if let Err(err) = self.config.set_thread_paper(handler, paper) {
                    log::warn!("failed to save thread setting: {err}");
                }
            }
            None => self.config.thread_paper = paper,
        }
    }
}
