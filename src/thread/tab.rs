// SPDX-License-Identifier: GPL-3.0-only

//! A thread tab: its conversation, its CLI, its composer and its selection.
//!
//! Memory is spent where the user is looking. A thread not shown is
//! *unloaded* when more than `loaded_threads` others are loaded: its entries
//! go, and are read back from the CLI's transcript when it is shown again.
//! Its CLI, a Node process of a few hundred megabytes, is stopped once idle
//! for `idle_minutes`, or when more than `live_agents` are running, and is
//! started again with `--resume` when the next message is sent.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use cosmic::app::Task;
use cosmic::iced::widget::scrollable::{self, Viewport};
use cosmic::widget::{self, text_editor};
use tokio::sync::mpsc::UnboundedSender;

use super::claude::{self, Decision, Launch};
use super::protocol::Command;
use crate::config::Agent;
use super::markdown::{self, Doc};
use super::model::{Activity, Entries, Item, ItemKind, ToolCall};
use super::rich::{self, Pos, Select};
use super::session::{self, History, Output};
use crate::Message;
use crate::config::ProfileId;

pub type ThreadId = u64;

/// Which rich text of an item: its body, or a tool's input or output
pub type Part = u8;
pub const BODY: Part = 0;
pub const INPUT: Part = 1;
pub const OUTPUT: Part = 2;

/// One end of a selection
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub item: u64,
    pub part: Part,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Loaded(u64, Arc<Result<History, String>>),
    Cli(u64, Output),
    Edit(text_editor::Action),
    Send,
    Stop,
    Answer(String, Decision),
    Toggle(u64),
    Select(u64, Part, Select),
    LoadEarlier,
    Scrolled(Viewport),
    SetModel(usize),
    SetMode(usize),
    /// Scroll a page up (negative) or down
    Page(f32),
    Attach,
    Attached(Vec<PathBuf>),
    Detach(usize),
    CopyItem(u64),
}

/// What a thread asks of the app
pub enum Request {
    Copy(String),
    PickImages(ThreadId),
    Retitle,
}

enum Cli {
    Stopped,
    Starting,
    Running(UnboundedSender<Command>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    Ready,
    Loading,
    Failed(String),
}

/// The permission modes the CLI takes, and what the composer calls them
pub const MODES: [(&str, &str); 5] = [
    ("default", "Ask"),
    ("acceptEdits", "Accept edits"),
    // The CLI's classifier approves what looks safe and asks about the rest
    ("auto", "Auto"),
    ("plan", "Plan"),
    ("bypassPermissions", "Bypass"),
];

pub struct ThreadTab {
    pub id: ThreadId,
    pub profile_id: Option<ProfileId>,
    /// Whose agent: Claude or Codex
    pub agent: Agent,
    pub host: String,
    pub cwd: String,
    pub args: String,
    pub title: String,
    /// `None` while unloaded
    pub entries: Option<Entries>,
    pub load: Load,
    /// User turns not loaded, before the first one shown
    pub earlier: usize,
    /// User turns asked for
    pub turns: usize,
    pub session_id: Option<String>,
    cli: Cli,
    /// Each start of the CLI, so that a stopped one's output is ignored
    generation: u64,
    /// Lines for the CLI, waiting for it to start
    pending: Vec<Command>,
    pub last_active: Instant,
    pub last_shown: Instant,
    pub composer: text_editor::Content,
    pub composer_id: widget::Id,
    pub attachments: Vec<PathBuf>,
    pub selection: Option<(Anchor, Anchor)>,
    pub dragging: bool,
    pub expanded: HashSet<u64>,
    pub scroll_id: widget::Id,
    /// Whether the view follows new output, as it does while scrolled to
    /// the end
    pub follow: bool,
    /// The height of the scrolled view, for paging
    pub page_height: f32,
    pub model: String,
    pub mode: String,
    /// Models the CLI offers, as value and name
    pub models: Vec<(String, String)>,
    /// What the CLI last wrote to stderr, when it failed
    pub failure: Option<String>,
    /// The view's colours for the agent to match, if it should; set by the
    /// app
    pub colours: Option<claude::Colours>,
    /// The colours the running CLI was told
    launched_colours: Option<claude::Colours>,
    /// Rich text built for items, by item and part, with the hash of what
    /// it was built from
    docs: RefCell<HashMap<(u64, Part), (u64, Arc<Doc>)>>,
}

impl ThreadTab {
    pub fn new(id: ThreadId, profile_id: Option<ProfileId>, host: String, cwd: String, args: String) -> Self {
        Self {
            id,
            agent: Agent::Claude,
            profile_id,
            host,
            title: title_for(&cwd),
            cwd,
            args,
            entries: Some(Entries::default()),
            load: Load::Ready,
            earlier: 0,
            turns: 20,
            session_id: None,
            cli: Cli::Stopped,
            generation: 0,
            pending: Vec::new(),
            last_active: Instant::now(),
            last_shown: Instant::now(),
            composer: text_editor::Content::new(),
            // Unique, not named: libcosmic moves named widgets' state about,
            // which here took the header's text with it
            composer_id: widget::Id::unique(),
            attachments: Vec::new(),
            selection: None,
            dragging: false,
            expanded: HashSet::new(),
            scroll_id: widget::Id::unique(),
            follow: true,
            page_height: 0.0,
            model: String::new(),
            // The CLI approves what is safe and asks about the rest
            mode: "auto".to_string(),
            models: Vec::new(),
            failure: None,
            colours: None,
            launched_colours: None,
            docs: RefCell::new(HashMap::new()),
        }
    }

    /// A thread of an existing conversation, read back from its transcript
    pub fn resuming(
        id: ThreadId,
        profile_id: Option<ProfileId>,
        host: String,
        cwd: String,
        args: String,
        session_id: String,
        turns: usize,
    ) -> (Self, Task<Message>) {
        let mut tab = Self::new(id, profile_id, host, cwd, args);
        tab.session_id = Some(session_id);
        tab.turns = turns;
        tab.entries = None;
        let task = tab.reload();
        (tab, task)
    }

    pub fn is_loaded(&self) -> bool {
        self.entries.is_some()
    }

    pub fn is_working(&self) -> bool {
        matches!(&self.entries, Some(entries) if entries.activity == Activity::Working)
            || matches!(self.cli, Cli::Starting)
            || !self.pending.is_empty()
    }

    pub fn has_cli(&self) -> bool {
        !matches!(self.cli, Cli::Stopped)
    }

    /// Bytes its entries hold, roughly
    pub fn weight(&self) -> usize {
        self.entries.as_ref().map_or(0, Entries::weight)
            + self.docs.borrow().values().map(|(_, doc)| doc_weight(doc)).sum::<usize>()
    }

    /// Read the conversation back from its transcript
    pub fn reload(&mut self) -> Task<Message> {
        let Some(session_id) = self.session_id.clone() else {
            self.entries.get_or_insert_with(Entries::default);
            return Task::none();
        };
        self.load = Load::Loading;
        let (host, turns, id, generation) = (self.host.clone(), self.turns, self.id, self.generation);
        cosmic::task::future(async move {
            let result = tokio::task::spawn_blocking(move || session::load(&host, &session_id, turns))
                .await
                .unwrap_or_else(|err| Err(err.to_string()));
            Message::Thread(id, Msg::Loaded(generation, Arc::new(result)))
        })
    }

    /// Let go of the entries until shown again
    pub fn unload(&mut self) {
        if self.session_id.is_some() && !self.is_working() {
            self.entries = None;
            self.selection = None;
            self.docs.borrow_mut().clear();
        }
    }

    /// Shown: load it again if it was unloaded
    pub fn shown(&mut self) -> Task<Message> {
        self.last_shown = Instant::now();
        if self.entries.is_none() && self.load != Load::Loading {
            return self.reload();
        }
        Task::none()
    }

    /// Stop the CLI; it starts again, resuming, when needed
    pub fn stop_cli(&mut self) {
        if !self.is_working() {
            self.cli = Cli::Stopped;
            self.generation += 1;
        }
    }

    fn launch(&self) -> Launch {
        Launch {
            agent: self.agent,
            host: self.host.clone(),
            cwd: self.cwd.clone(),
            resume: self.session_id.clone(),
            model: Some(self.model.clone()).filter(|m| !m.is_empty()),
            permission_mode: Some(self.mode.clone()).filter(|m| !m.is_empty()),
            args: self.args.clone(),
            colours: self.colours.clone(),
        }
    }

    /// Send `line` to the CLI, starting it if need be
    fn send_line(&mut self, line: Command) -> Task<Message> {
        self.last_active = Instant::now();
        match &self.cli {
            Cli::Running(tx) => {
                if tx.send(line.clone()).is_ok() {
                    return Task::none();
                }
                self.cli = Cli::Stopped;
                self.pending.push(line);
                self.start_cli()
            }
            Cli::Starting => {
                self.pending.push(line);
                Task::none()
            }
            Cli::Stopped => {
                self.pending.push(line);
                self.start_cli()
            }
        }
    }

    fn start_cli(&mut self) -> Task<Message> {
        self.generation += 1;
        self.cli = Cli::Starting;
        self.failure = None;
        self.launched_colours = self.colours.clone();
        let (id, generation) = (self.id, self.generation);
        Task::run(session::run(self.launch()), move |output| {
            cosmic::Action::App(Message::Thread(id, Msg::Cli(generation, output)))
        })
    }

    pub fn update(&mut self, msg: Msg, requests: &mut Vec<Request>) -> Task<Message> {
        match msg {
            Msg::Loaded(generation, result) => {
                if generation != self.generation && self.has_cli() {
                    return Task::none();
                }
                match Arc::try_unwrap(result).unwrap_or_else(|arc| match &*arc {
                    Ok(_) => Err("loaded twice".into()),
                    Err(err) => Err(err.clone()),
                }) {
                    Ok(history) => {
                        let mut entries = history.entries;
                        if let Some(old) = &self.entries {
                            entries.models = old.models.clone();
                        }
                        self.earlier = history.earlier;
                        if self.session_id.is_none() {
                            self.session_id = entries.session_id.clone();
                        }
                        self.entries = Some(entries);
                        self.load = Load::Ready;
                        self.selection = None;
                        self.docs.borrow_mut().clear();
                        requests.push(Request::Retitle);
                        if self.follow {
                            return scroll_to_end(&self.scroll_id);
                        }
                    }
                    Err(err) => {
                        self.load = Load::Failed(err);
                        self.entries.get_or_insert_with(Entries::default);
                    }
                }
            }
            Msg::Cli(generation, output) => {
                if generation != self.generation {
                    return Task::none();
                }
                match output {
                    Output::Ready(tx) => {
                        for line in self.pending.drain(..) {
                            let _ = tx.send(line);
                        }
                        self.cli = Cli::Running(tx);
                    }
                    Output::Events(events) => {
                        self.last_active = Instant::now();
                        let entries = self.entries.get_or_insert_with(Entries::default);
                        let had_title = self.session_id.is_some();
                        for event in events {
                            entries.apply(event);
                        }
                        if entries.session_id.is_some() && self.session_id != entries.session_id {
                            self.session_id = entries.session_id.clone();
                        }
                        if !entries.model.is_empty() && self.model.is_empty() {
                            self.model = entries.model.clone();
                        }
                        if !entries.permission_mode.is_empty() {
                            self.mode = entries.permission_mode.clone();
                        }
                        if !entries.models.is_empty() {
                            self.models = entries.models.clone();
                        }
                        if !had_title {
                            requests.push(Request::Retitle);
                        }
                        if self.follow {
                            return scroll_to_end(&self.scroll_id);
                        }
                    }
                    Output::Exited(code, stderr) => {
                        self.cli = Cli::Stopped;
                        let was_working = self.is_working();
                        if let Some(entries) = &mut self.entries {
                            entries.stopped();
                            if was_working || code.is_some_and(|code| code != 0) {
                                let detail = stderr.trim();
                                entries.notice(
                                    if detail.is_empty() {
                                        format!("Claude stopped (exit status {})", code.map_or("unknown".into(), |c| c.to_string()))
                                    } else {
                                        format!("Claude stopped: {detail}")
                                    },
                                    true,
                                );
                            }
                        }
                        self.pending.clear();
                        if code.is_some_and(|code| code != 0) {
                            self.failure = Some(stderr);
                        }
                    }
                    // Only from a relayed CLI (session::run_relayed)
                    Output::Hello { .. } | Output::Input { .. } | Output::Disconnected => {}
                }
            }
            Msg::Edit(action) => {
                if matches!(action, text_editor::Action::Click(_) | text_editor::Action::Drag(_)) {
                    self.selection = None;
                }
                self.composer.perform(action);
            }
            Msg::Send => {
                let text = self.composer.text().trim_end().to_string();
                if text.trim().is_empty() && self.attachments.is_empty() {
                    return Task::none();
                }
                let mut images = Vec::new();
                for path in &self.attachments {
                    match super::images::read_for_sending(path) {
                        Ok(image) => images.push(image),
                        Err(err) => {
                            if let Some(entries) = &mut self.entries {
                                entries.notice(err, true);
                            }
                            return Task::none();
                        }
                    }
                }
                // Its system prompt holds the colours, so a CLI told others
                // starts again, resuming, to learn the new ones
                if matches!(self.cli, Cli::Running(_)) && self.session_id.is_some() && self.colours != self.launched_colours {
                    self.stop_cli();
                }
                let line = Command::User { text: text.clone(), images };
                let refs = self.attachments.drain(..).map(|path| super::model::ImageRef { path }).collect();
                self.entries.get_or_insert_with(Entries::default).user(text, refs);
                self.composer = text_editor::Content::new();
                self.follow = true;
                return Task::batch([self.send_line(line), scroll_to_end(&self.scroll_id)]);
            }
            Msg::Stop => {
                if let Cli::Running(tx) = &self.cli {
                    let _ = tx.send(Command::Interrupt);
                }
            }
            Msg::Answer(request_id, decision) => {
                let Some(entries) = &mut self.entries else { return Task::none() };
                let Some(request) = entries.permissions.iter().find(|r| r.request_id == request_id).cloned() else {
                    return Task::none();
                };
                entries.answered(&request_id, decision != Decision::Deny);
                if let Cli::Running(tx) = &self.cli {
                    let _ = tx.send(Command::Answer { request, decision });
                }
            }
            Msg::Toggle(item) => {
                if !self.expanded.remove(&item) {
                    self.expanded.insert(item);
                }
            }
            Msg::Select(item, part, select) => {
                let anchor = |pos| Anchor { item, part, pos };
                match select {
                    Select::Press { pos, extend } => {
                        match (&mut self.selection, extend) {
                            (Some((_, head)), true) => *head = anchor(pos),
                            _ => self.selection = Some((anchor(pos), anchor(pos))),
                        }
                        self.dragging = true;
                    }
                    Select::Drag(pos) => {
                        if self.dragging
                            && let Some((_, head)) = &mut self.selection
                        {
                            *head = anchor(pos);
                        }
                    }
                    Select::Range(start, end) => {
                        self.selection = Some((anchor(start), anchor(end)));
                        self.dragging = false;
                    }
                }
            }
            Msg::LoadEarlier => {
                self.turns += self.turns.max(10);
                self.follow = false;
                return self.reload();
            }
            Msg::Scrolled(viewport) => {
                let bounds = viewport.bounds();
                let content = viewport.content_bounds();
                let offset = viewport.absolute_offset();
                self.follow = offset.y + bounds.height >= content.height - 24.0;
                self.page_height = bounds.height;
            }
            Msg::Page(pages) => {
                // A little of the last page stays in sight, to read on from
                let page = if self.page_height > 0.0 { self.page_height * 0.9 } else { 400.0 };
                return scrollable::scroll_by(self.scroll_id.clone(), scrollable::AbsoluteOffset { x: 0.0, y: pages * page });
            }
            Msg::SetModel(index) => {
                if let Some((value, _)) = self.models.get(index) {
                    self.model = value.clone();
                    if let Cli::Running(tx) = &self.cli {
                        let _ = tx.send(Command::SetModel(value.clone()));
                    }
                }
            }
            Msg::SetMode(index) => {
                if let Some((value, _)) = MODES.get(index) {
                    self.mode = value.to_string();
                    if let Cli::Running(tx) = &self.cli {
                        let _ = tx.send(Command::SetMode(value.to_string()));
                    }
                }
            }
            Msg::Attach => requests.push(Request::PickImages(self.id)),
            Msg::Attached(paths) => self.attachments.extend(paths),
            Msg::Detach(index) => {
                if index < self.attachments.len() {
                    self.attachments.remove(index);
                }
            }
            Msg::CopyItem(item) => {
                if let Some(item) = self.item(item) {
                    requests.push(Request::Copy(item.copy_text()));
                }
            }
        }
        Task::none()
    }

    pub fn item(&self, id: u64) -> Option<&Item> {
        self.entries.as_ref()?.items.iter().find(|item| item.id == id)
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.entries.as_ref()?.items.iter().position(|item| item.id == id)
    }

    /// The selection, start first, as item indices
    fn ordered(&self) -> Option<((usize, Part, Pos), (usize, Part, Pos))> {
        let (a, b) = self.selection?;
        let a = (self.index_of(a.item)?, a.part, a.pos);
        let b = (self.index_of(b.item)?, b.part, b.pos);
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// The part of the selection in one rich text, start first
    pub fn selection_in(&self, item_index: usize, part: Part) -> Option<(Pos, Pos)> {
        let (start, end) = self.ordered()?;
        let here = (item_index, part);
        if here < (start.0, start.1) || here > (end.0, end.1) {
            return None;
        }
        let from = if here == (start.0, start.1) { start.2 } else { Pos::START };
        let to = if here == (end.0, end.1) { end.2 } else { Pos::END };
        (from != to).then_some((from, to))
    }

    pub fn has_selection(&self) -> bool {
        self.ordered().is_some_and(|(a, b)| a != b)
    }

    /// The selected text, as it would be pasted
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.ordered()?;
        let entries = self.entries.as_ref()?;
        let mut parts = Vec::new();
        for index in start.0..=end.0 {
            let item = entries.items.get(index)?;
            for part in [BODY, INPUT, OUTPUT] {
                let Some((from, to)) = self.selection_in(index, part) else { continue };
                if let Some(doc) = self.doc(item, part) {
                    let text = rich::copy(&doc, from, to);
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
            }
        }
        Some(parts.join("\n\n")).filter(|text| !text.is_empty())
    }

    pub fn select_all(&mut self) {
        let Some(entries) = &self.entries else { return };
        let (Some(first), Some(last)) = (entries.items.first(), entries.items.last()) else { return };
        self.selection = Some((
            Anchor { item: first.id, part: BODY, pos: Pos::START },
            Anchor { item: last.id, part: OUTPUT, pos: Pos::END },
        ));
    }

    /// The rich text of part `part` of `item`, built when first needed and
    /// kept while the item is unchanged
    pub fn doc(&self, item: &Item, part: Part) -> Option<Arc<Doc>> {
        let (hash, build): (u64, Box<dyn Fn() -> Doc>) = match (&item.kind, part) {
            (ItemKind::Text { doc, .. }, BODY) => return Some(doc.clone()),
            (ItemKind::User { text, .. }, BODY) => (hash_of(text), Box::new(|| markdown::parse(text))),
            (ItemKind::Thinking { text, .. }, BODY) => (hash_of(text), Box::new(|| markdown::parse(text))),
            (ItemKind::Notice { text, .. }, BODY) => (hash_of(text), Box::new(|| markdown::plain(text))),
            (ItemKind::Tool(tool), INPUT) => (
                hash_of(&(tool.input.to_string(), &tool.partial_input)),
                Box::new(|| tool_input_doc(tool)),
            ),
            (ItemKind::Tool(tool), OUTPUT) => {
                if tool.output.is_empty() && tool.children.is_empty() {
                    return None;
                }
                (hash_of(&(&tool.output, &tool.children)), Box::new(|| tool_output_doc(tool)))
            }
            _ => return None,
        };
        let key = (item.id, part);
        if let Some((built, doc)) = self.docs.borrow().get(&key)
            && *built == hash
        {
            return Some(doc.clone());
        }
        let doc = Arc::new(build());
        self.docs.borrow_mut().insert(key, (hash, doc.clone()));
        Some(doc)
    }
}

fn hash_of(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::hash::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn doc_weight(doc: &Doc) -> usize {
    rich::copy(doc, Pos::START, rich::end_of(doc)).len() * 2
}

fn scroll_to_end(id: &widget::Id) -> Task<Message> {
    scrollable::snap_to(id.clone(), scrollable::RelativeOffset { x: None, y: Some(1.0) })
}

/// A thread's title before the agent names it: its directory's name
fn title_for(cwd: &str) -> String {
    let name = cwd.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    if name.is_empty() { "Claude".to_string() } else { format!("Claude · {name}") }
}

/// What a tool was asked to do, as rich text: a command, a diff, a list
fn tool_input_doc(tool: &ToolCall) -> Doc {
    let field = |name: &str| tool.input.get(name).and_then(serde_json::Value::as_str).unwrap_or_default();
    let fenced = |lang: &str, body: &str| {
        let fence = if body.contains("```") { "~~~~" } else { "```" };
        format!("{fence}{lang}\n{body}\n{fence}")
    };
    if !tool.partial_input.is_empty() {
        return markdown::parse(&fenced("json", &tool.partial_input));
    }
    let source = match tool.name.as_str() {
        "Bash" => {
            let mut out = fenced("sh", field("command"));
            let description = field("description");
            if !description.is_empty() {
                out = format!("{description}\n\n{out}");
            }
            out
        }
        "Edit" | "MultiEdit" | "Write" => {
            let mut diff = String::new();
            for (old, new) in tool.edits() {
                for line in old.lines() {
                    diff.push_str(&format!("-{line}\n"));
                }
                for line in new.lines() {
                    diff.push_str(&format!("+{line}\n"));
                }
                diff.push_str("@@\n");
            }
            let diff = diff.trim_end_matches("@@\n").trim_end();
            fenced("diff", diff)
        }
        "TodoWrite" => tool
            .todos()
            .iter()
            .map(|(text, status)| {
                let mark = match status.as_str() {
                    "completed" => "- [x] ",
                    "in_progress" => "- [ ] **",
                    _ => "- [ ] ",
                };
                let end = if status == "in_progress" { "**" } else { "" };
                format!("{mark}{text}{end}\n")
            })
            .collect(),
        "Task" | "Agent" => field("prompt").to_string(),
        _ => {
            let pretty = serde_json::to_string_pretty(&tool.input).unwrap_or_default();
            if pretty == "null" { String::new() } else { fenced("json", &pretty) }
        }
    };
    markdown::parse(&source)
}

/// What a tool gave back
fn tool_output_doc(tool: &ToolCall) -> Doc {
    let mut blocks = Vec::new();
    if !tool.children.is_empty() {
        let mut lines = String::new();
        if tool.child_count > tool.children.len() {
            lines.push_str(&format!("… {} earlier steps\n", tool.child_count - tool.children.len()));
        }
        for child in &tool.children {
            lines.push_str(child);
            lines.push('\n');
        }
        blocks.push(markdown::Block::Code { lang: String::new(), text: lines.trim_end().to_string(), indent: 0, quote: 0 });
    }
    if !tool.output.is_empty() {
        // An agent's answer is prose; anything else is shown as it came
        if matches!(tool.name.as_str(), "Task" | "Agent" | "WebFetch" | "WebSearch") {
            blocks.extend(markdown::parse(&tool.output).blocks);
        } else {
            blocks.push(markdown::Block::Code {
                lang: String::new(),
                text: tool.output.clone(),
                indent: 0,
                quote: 0,
            });
        }
    }
    Doc { blocks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::model::Event;

    fn tab_with(texts: &[&str]) -> ThreadTab {
        let mut tab = ThreadTab::new(1, None, String::new(), "/tmp/p".into(), String::new());
        let entries = tab.entries.as_mut().unwrap();
        for (n, text) in texts.iter().enumerate() {
            entries.apply(Event::Assistant {
                message_id: format!("m{n}"),
                content: serde_json::json!([{"type": "text", "text": text}]),
                parent: None,
            });
        }
        tab
    }

    #[test]
    fn copies_a_selection_across_items() {
        let mut tab = tab_with(&["First $x$ para", "Second para"]);
        let ids: Vec<u64> = tab.entries.as_ref().unwrap().items.iter().map(|i| i.id).collect();
        let mut requests = Vec::new();
        let _ = tab.update(Msg::Select(ids[0], BODY, Select::Press { pos: Pos { block: 0, offset: 6 }, extend: false }), &mut requests);
        let _ = tab.update(Msg::Select(ids[1], BODY, Select::Drag(Pos { block: 0, offset: 6 })), &mut requests);
        assert_eq!(tab.selected_text().as_deref(), Some("$x$ para\n\nSecond"));
        // Backwards gives the same
        tab.selection = tab.selection.map(|(a, b)| (b, a));
        assert_eq!(tab.selected_text().as_deref(), Some("$x$ para\n\nSecond"));
    }

    #[test]
    fn messages_wait_for_the_cli_and_a_stopped_one_is_resumed() {
        let mut tab = ThreadTab::new(1, None, String::new(), "/tmp".into(), String::new());
        let mut requests = Vec::new();
        tab.composer = text_editor::Content::with_text("What is $\\pi$?");
        let _ = tab.update(Msg::Send, &mut requests);
        assert!(tab.has_cli() && tab.is_working());
        assert_eq!(tab.pending.len(), 1, "queued until the CLI is ready");
        assert!(matches!(&tab.entries.as_ref().unwrap().items[0].kind, ItemKind::User { text, .. } if text == "What is $\\pi$?"));

        // The CLI starts: what waited goes to it
        let generation = tab.generation;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _ = tab.update(Msg::Cli(generation, Output::Ready(tx)), &mut requests);
        assert!(matches!(rx.try_recv().unwrap(), Command::User { text, .. } if text.contains("What is")));
        let _ = tab.update(
            Msg::Cli(
                generation,
                Output::Events(vec![
                    super::super::model::Event::Started {
                        session_id: "s1".into(),
                        model: "m".into(),
                        cwd: "/tmp".into(),
                        permission_mode: "default".into(),
                    },
                    super::super::model::Event::TurnDone {
                        error: false,
                        message: String::new(),
                        duration_ms: 1,
                        context_tokens: Some(1000),
                    },
                ]),
            ),
            &mut requests,
        );
        assert_eq!(tab.session_id.as_deref(), Some("s1"));
        assert!(!tab.is_working());

        // Idle, it is stopped; its last words are ignored
        tab.stop_cli();
        assert!(!tab.has_cli());
        assert!(rx.try_recv().is_err());
        let _ = tab.update(Msg::Cli(generation, Output::Exited(Some(0), String::new())), &mut requests);
        assert_eq!(tab.entries.as_ref().unwrap().items.len(), 1, "no notice for a stop we asked for");

        // The next message starts it again, resuming the conversation
        tab.composer = text_editor::Content::with_text("And $e$?");
        let _ = tab.update(Msg::Send, &mut requests);
        assert!(tab.has_cli());
        assert!(tab.launch().script().contains("'--resume=s1'"));
    }

    #[test]
    fn unloading_keeps_the_session_and_frees_the_entries() {
        let mut tab = tab_with(&["text"]);
        tab.session_id = Some("abc".into());
        tab.unload();
        assert!(tab.entries.is_none());
        assert_eq!(tab.weight(), 0);
    }

    #[test]
    fn tool_inputs_render_as_diffs_and_commands() {
        let tool = ToolCall {
            id: "t".into(),
            name: "Edit".into(),
            partial_input: String::new(),
            input: serde_json::json!({"file_path": "/a", "old_string": "a\nb", "new_string": "c"}),
            status: super::super::model::ToolStatus::Done,
            output: String::new(),
            images: Vec::new(),
            children: Vec::new(),
            child_count: 0,
        };
        let doc = tool_input_doc(&tool);
        assert!(matches!(&doc.blocks[0], markdown::Block::Code { lang, text, .. } if lang == "diff" && text == "-a\n-b\n+c"));
    }
}
