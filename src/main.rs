// SPDX-License-Identifier: GPL-3.0-only

//! Deductum: proof documents (Isabelle theories now) as literate documents,
//! worked on together with agents in one live prover session. A sibling of COSMIC Terminal Agents
//! (design C7), sharing its rendering and its list of machines.
//!
//! The design record is `docs/deductum-decisions.md`. The app is a client of
//! the session host on each machine (`deductum/host`), which owns sessions,
//! buffers and provers; it keeps a replica of each open theory and shows it
//! in a literate view and a source view. What depends on the prover is in
//! `prover`: a profile per backend, and the capabilities each session's host
//! reports, which decide what the UI offers.

mod cli;
mod codec;
mod editor;
mod gap;
mod goals;
mod input;
mod link;
mod ot;
mod prover;
mod theory;
mod view;
mod vim;

#[cfg(test)]
mod snapshot;
#[cfg(test)]
mod live;

/// LaTeX's fonts, New Computer Modern, for the literate view's prose; see `res/fonts/README`
pub const FONTS: [&[u8]; 5] = [
    // Raised and lowered small glyphs for \<^sup> and \<^sub>; see res/deductum/make-scripts-font.py
    include_bytes!("../../../res/deductum/fonts/IsabellumScripts.ttf"),
    include_bytes!("../../../res/fonts/NewCM10-Regular.otf"),
    include_bytes!("../../../res/fonts/NewCM10-Bold.otf"),
    include_bytes!("../../../res/fonts/NewCM10-Italic.otf"),
    include_bytes!("../../../res/fonts/NewCM10-BoldItalic.otf"),
];

pub fn load_fonts() {
    let mut font_system = cosmic::iced::advanced::graphics::text::font_system().write().unwrap();
    for data in FONTS {
        font_system.raw().db_mut().load_font_data(data.to_vec());
    }
}

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{Limits, Size, Subscription};
use cosmic::prelude::*;
use cosmic::Application;
use cosmic::widget::text_editor::Action;
use cosmic::{executor, widget};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use codec::Symbols;
use editor::{Mode, Theory};
use prover::{Capabilities, Profile, Tool};

pub const APP_ID: &str = "dev.twright.Deductum";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger_init();
    prover::register_highlighters();
    let request = cli::parse(std::env::args().skip(1).collect());
    if let Some(request) = &request
        && cli::forward(request)
    {
        return Ok(());
    }
    let settings = Settings::default()
        .size(Size::new(1400.0, 900.0))
        .size_limits(Limits::NONE.min_width(640.0).min_height(400.0));
    cosmic::app::run::<App>(settings, Flags { request })?;
    Ok(())
}

fn env_logger_init() {
    // log goes to stderr; no logger crate is needed for a preview
}

pub struct Flags {
    request: Option<cli::OpenRequest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Output,
    /// The proof tool that runs from a panel (Isabelle's Sledgehammer) and LLM help
    Tool,
    Search,
    Counterexamples,
    Symbols,
    Agents,
    History,
    Log,
}

/// What the left sidebar shows, as Marginalium's navigation does
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftMode {
    Sessions,
    Outline,
    Theorems,
    Problems,
}

#[derive(Clone, Debug)]
pub enum Message {
    Link(String, link::Event),
    Open(cli::OpenRequest),
    OpenSession(String, String),
    CloseWorkspace,
    OpenTheory(String),
    SetMode(Mode),
    SourceAction(Action),
    BlockAction(Action),
    EditBlock(usize),
    CloseBlock,
    Lifecycle(&'static str),
    Undo,
    Redo,
    Save,
    Panel(Panel),
    StateTick(u64),
    /// Run the panel's proof tool at the caret, or at a source offset
    RunTool(String),
    RunToolAt(String, usize),
    /// A proof tool's setting changed (tool, setting, value)
    ToolSetting(String, String, String),
    CancelQuery,
    InsertProof(String),
    FindInput(String),
    Find,
    Grant(String, String, &'static str),
    Revoke(String),
    /// Answer an agent's request for editing rights: allow (true) or deny
    AnswerRequest(String, bool),
    Revert(u64),
    Follow(Option<String>),
    Resolve(&'static str),
    RestoreAlternative(String, usize),
    DismissAlternative(String),
    NewSession(bool),
    NewMachine(usize),
    NewFolder(String),
    NewBrowse(String),
    NewLogic(String),
    NewCreate,
    Merge(String),
    PopOut,
    PopIn,
    Complete(usize),
    /// Enter: a new line indented for the proof structure
    Newline,
    /// Shift+Tab: remove up to two spaces of indentation from the current line
    Unindent,
    /// A key for the vim machine (key, with Ctrl)
    Vim(String, bool),
    ToggleVim,
    /// Open the session's folder in the file manager, Zed or a terminal
    OpenFolder(link::Opener),
    OpenFolderResult(Result<(), String>),
    /// Copy (or cut, if true) the selection, as Isabelle source: symbols as escapes
    CopySelection(bool),
    /// A key nobody else took: vim's block navigation in the literate view
    WindowKey(String, bool),
    SetModeEntity(widget::segmented_button::Entity),
    SymbolSearch(String),
    /// A motion or selection in a message shown as code (it is read-only)
    ViewAction(u64, Action),
    /// A click in a formal block shown in the literate view: opens it there, caret at the click
    BlockViewAction(usize, Action),
    /// Run a tool once at the caret without editing (a counterexample finder, a quick proof)
    RunCommand(String),
    /// Ask an LLM (0 Claude, 1 Codex) to fill the hole at the caret; its proofs are checked
    Fill,
    FillAgent(usize),
    FillHint(String),
    InsertFill(String),
    FillDismiss,
    /// Replace the hole with the suggested facts and an inline Sledgehammer
    InsertUsing,
    /// The proof state as parts (false) or as Isabelle's text (true)
    StateText(bool),
    TheoryFilter(String),
    /// The pointer over an editor (where, in its coordinates), or off it
    HoverMove(HoverTarget, cosmic::iced::Point),
    HoverExit,
    /// Show or hide a typed tool's settings; apply a tool's settings (sent to the prover)
    ToolSettings(bool),
    ToolApply(String),
    HoverTick(u64),
    /// A new block after the one at this index (none: at the end): text, section or lemma
    InsertBlock(Option<usize>, &'static str),
    DeleteBlock(usize),
    ProsePointer(usize, cosmic::iced::Point),
    /// A click replayed in the block editor once it is laid out (opening prose where it was clicked)
    BlockClickAt(cosmic::iced::Point),
    /// Ctrl and Alt held or not, for Ctrl+click and Alt+click
    Modifiers(bool, bool),
    /// Back to where Ctrl+click came from
    Back,
    GapHover(Option<usize>),
    GapLeave(usize),
    /// A shown block pressed: it opens with the caret at the character under the pointer
    ViewPress(usize),
    /// A pause after typing on a line reading `claude …` or `codex …` (this generation of edits)
    KeywordTick(u64),
    /// Stop proof tools typed in the text (sledgehammer and the like)
    StopTools,
    /// Stop again if the tools still run (an external prover can take a moment to give up)
    StopToolsAgain(u8),
    /// Show the definition in its window in the main window; its window closed
    /// Up or down a line (true: down), past a block's ends into its neighbours
    LineMove(bool),
    /// The goal beside the text, or under the proof with the page centred
    ToggleGoalBeside,
    /// The insert palette's search
    PaletteQuery(String),
    /// Put this before and this after the selection, or at the caret with the caret between
    Wrap(String, String),
    /// Where `claude` and `codex` queries go: helper, fresh, or agent:KEY
    QueryRoute(String),
    /// Start the helper conversation again
    HelperReset,
    CopyText(String),
    RefOpenHere(cosmic::iced::window::Id),
    RefBack(cosmic::iced::window::Id),
    RefAction(cosmic::iced::window::Id, Action),
    RefScroll(cosmic::iced::window::Id),
    RefClosed(cosmic::iced::window::Id),
    /// Go to the place an agent offered to show (true), or dismiss it
    ShowOffer(bool),
    NoteReplyInput(String, String),
    /// Send the reply to a note; resolve it if true
    NoteReply(String, bool),
    ShowTypes(bool),
    AllGoals(bool),
    CommandParams(String),
    InsertSymbol(String),
    LeftMode(LeftMode),
    /// Dragging a sidebar's edge: start (true for the left one), move, end
    DragStart(bool),
    DragMove(cosmic::iced::Point),
    DragEnd,
    WindowWidth(f32),
    /// Zoom in (1), out (-1) or back to 100% (0)
    Zoom(i8),
    ToggleRight,
    Goto(usize),
    InsertOutput(String),
    PopOutClosed(cosmic::iced::window::Id),
    ToggleSidebar,
    Noop,
}

/// What a reply is for
#[derive(Clone, Debug)]
enum Pending {
    Hello,
    Sessions,
    Symbols,
    SessionInfo(String),
    Read(String, String),
    Edit(String),
    State(u64),
    Query(&'static str),
    Fill,
    History(String),
    ListDir,
    Suggest,
    Create,
    Plain,
    /// An undo or redo: says what it did, and the app goes there
    Undo,
    Notes,
    /// Hover information for this generation of the pointer's place
    Info(u64),
    /// The definition at the caret, to go to (Ctrl+click), or to show in a window (Alt+click)
    Definition(bool),
    /// A definition's theory, read for a window: a new one, or this one (Ctrl+click in it)
    RefRead(Value, Option<cosmic::iced::window::Id>),
    /// What the prover says about a place in a reference window's theory
    RefInfo(cosmic::iced::window::Id, RefUse),
    /// A name in a reference window's library theory, looked up in the main theory's context
    Entity(cosmic::iced::window::Id, RefUse),
}

/// What a look-up in a reference window is for: a tooltip (this generation of the pointer's
/// place), or going to the definition (in a new window when true)
#[derive(Clone, Copy, Debug)]
pub enum RefUse {
    Hover(u64),
    Goto(bool),
}

pub struct Link {
    pub target: link::Target,
    pub name: String,
    pub sender: Option<mpsc::UnboundedSender<String>>,
    pub status: String,
    pub hello: Option<Value>,
    pub client_id: Option<String>,
    /// The symbol table the host sent (Isabelle's runtime's), if it has one
    pub symbols: Option<Arc<dyn Symbols>>,
}

#[derive(Clone, Debug, Default)]
pub struct QueryState {
    pub id: Option<String>,
    pub operation: &'static str,
    pub status: String,
    pub messages: Vec<Value>,
    pub replace: Option<(usize, usize)>,
    pub end: Option<usize>,
    pub revision: u64,
    pub error: Option<String>,
}

/// A definition shown in its own window: its whole theory, read-only, at the definition
pub struct Reference {
    pub title: String,
    pub path: String,
    /// Where the definition starts, and its line
    pub start: usize,
    pub line: usize,
    pub theory: Theory,
    /// Not a theory of the session (a library theory, which the prover has not checked here):
    /// names in it are looked up in the main theory's context
    pub library: bool,
    /// What opened it, and where it was before Ctrl+clicks in it moved it (for Back)
    pub place: Value,
    pub back: Vec<Value>,
}

/// The editor the pointer is over
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverTarget {
    Source,
    Block,
    /// A formal block shown in the literate view, by index
    View(usize),
    /// A reference window's theory
    Ref(cosmic::iced::window::Id),
    /// A message or goal shown as code, by its key (its tips are its own: no prover asked)
    Message(u64),
}

#[derive(Clone, Debug)]
pub struct Hover {
    pub target: HoverTarget,
    /// Where the pointer is, in the editor's own coordinates
    pub point: cosmic::iced::Point,
    /// The source offset under it
    pub offset: usize,
    pub generation: u64,
    pub info: Option<Value>,
}

#[derive(Clone, Debug, Default)]
pub struct FillState {
    pub job: Option<String>,
    pub agent: usize,
    /// The model asked for in brackets after the keyword (`codex[astra]`)
    pub model: Option<String>,
    /// The listening agent asked (`@name`), what it says, and the helper conversation used
    pub asked: Option<String>,
    pub message: Option<String>,
    pub conversation: Option<Value>,
    pub hint: String,
    pub status: String,
    pub candidates: Vec<Value>,
    pub replace: Option<(usize, usize)>,
    pub revision: u64,
    pub error: Option<String>,
    /// Facts the LLM suggests for a `using` line in front of Sledgehammer
    pub using: Vec<String>,
    /// When it was asked
    pub started: Option<std::time::Instant>,
}

pub struct Workspace {
    pub machine: String,
    pub sid: String,
    /// The session's backend, and what its host says it can do (see [`App::sync_backend`])
    pub profile: &'static dyn Profile,
    pub caps: Capabilities,
    pub info: Option<Value>,
    pub theory: Option<Theory>,
    pub opening: Option<String>,
    pub panel: Panel,
    pub output: Option<Value>,
    /// The panel tool's run (Sledgehammer from its panel)
    pub tool_query: QueryState,
    /// The proof state in parts at the caret, and at the command before (for what changed)
    pub goals: QueryState,
    pub goals_prev: QueryState,
    /// Show Isabelle's own text of the state instead of the parts
    pub state_text: bool,
    pub show_types: bool,
    pub all_goals: bool,
    /// Narrows the session's list of theories
    pub theory_filter: String,
    /// Agents' notes on the open theory, a reply being written to each, and a place an agent
    /// offers to show
    pub notes: Vec<Value>,
    pub note_reply: HashMap<String, String>,
    pub show_offer: Option<Value>,
    /// Where to go once the theory being opened is read (host offset)
    pub pending_goto: Option<usize>,
    /// What the pointer is over, and what the prover says about it
    pub hover: Option<Hover>,
    /// The settings of the typed tool at the caret are shown (under the proof state)
    pub settings_open: bool,
    /// When the command at the caret was first seen running (a proof tool's progress)
    pub tool_started: Option<std::time::Instant>,
    /// The gap between blocks the pointer is over (after the block at this index)
    pub gap_hover: Option<usize>,
    /// Stop was pressed for the proof tools: asked again until they have stopped
    pub tools_stopping: bool,
    /// What the insert palette (beside the text, when there is no goal) is searched for
    pub palette_query: String,
    pub find: QueryState,
    pub find_input: String,
    pub cex: QueryState,
    pub fill: FillState,
    pub cex_params: String,
    pub activity: Option<Value>,
    /// Messages shown as code, by the hash of the message
    pub views: HashMap<u64, editor::CodeView>,
    /// Proof tools' settings as the human set them, by `tool.setting` (see [`Tool::value`])
    pub tool_settings: HashMap<String, String>,
    pub symbol_search: String,
    pub history: Vec<Value>,
    pub follow: Option<String>,
    pub log: Vec<String>,
    pub merge: Option<Vec<Value>>,
}

#[derive(Default)]
pub struct NewSession {
    pub machine: usize,
    pub folder: String,
    pub listing: Option<Value>,
    pub logic: String,
    pub suggestion: Option<Value>,
    pub error: Option<String>,
}

pub struct App {
    core: Core,
    pub links: Vec<Link>,
    /// Ctrl is held (Ctrl+click goes to a definition)
    pub ctrl: bool,
    /// Alt is held (Alt+click opens a definition in a window of its own)
    pub alt: bool,
    /// Where Ctrl+click came from, to go back (theory, host offset)
    pub back: Vec<(String, usize)>,
    /// A definition shown in a window of its own
    pub references: Vec<(cosmic::iced::window::Id, Reference)>,
    /// A running name look-up for a reference window: its query, the window and what for
    pub entity: Option<(String, cosmic::iced::window::Id, RefUse)>,
    pub hover_gen: u64,
    /// Counts edits, so that a keyword's pause is a pause
    pub keyword_gen: u64,
    /// Where the pointer last was over a prose block (its index), so that a click there opens
    /// it with the caret at the click
    pub prose_pointer: Option<(usize, cosmic::iced::Point)>,
    /// Vim's block cursor is showing: the character under the caret, selected
    pub vim_block: bool,
    /// A placement for the proof state instead of the rule (comparing them in pictures)
    pub goal_placement_override: Option<view::GoalPlacement>,
    pub sessions: BTreeMap<(String, String), Value>,
    next_id: u64,
    pending: HashMap<u64, (String, Pending)>,
    pub workspace: Option<Workspace>,
    pub new_session: Option<NewSession>,
    pub show_sidebar: bool,
    pub status: Option<String>,
    state_gen: u64,
    request: Option<cli::OpenRequest>,
    kinds: Arc<HashMap<String, String>>,
    /// The window a proof was popped out into
    pub popout: Option<cosmic::iced::window::Id>,
    /// A theory asked for on the command line, opened once its session is
    want_theory: Option<String>,
    pub left: LeftMode,
    pub show_right: bool,
    pub left_width: f32,
    pub right_width: f32,
    /// The sidebar being resized: true for the left one
    pub dragging: Option<bool>,
    pub window_width: f32,
    pub vim: vim::Vim,
    pub vim_enabled: bool,
    pub mode_model: widget::segmented_button::SingleSelectModel,
    mode_entities: [widget::segmented_button::Entity; 2],
}

/// Settings kept between runs
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct UiSettings {
    #[serde(default)]
    vim: bool,
    #[serde(default)]
    zoom: Option<f32>,
    #[serde(default)]
    left_width: Option<f32>,
    #[serde(default)]
    right_width: Option<f32>,
    /// Whether the sidebars were open (both start closed)
    #[serde(default)]
    show_left: Option<bool>,
    #[serde(default)]
    show_right: Option<bool>,
    /// Whether the goal goes beside the text when there is room
    #[serde(default)]
    goal_beside: Option<bool>,
}

fn settings_path() -> std::path::PathBuf {
    let base = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join(".config/deductum");
    base.join("settings.json")
}

fn load_settings() -> UiSettings {
    std::fs::read_to_string(settings_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

impl App {
    fn save_settings(&self) {
        let settings = UiSettings {
            vim: self.vim_enabled,
            zoom: Some(view::zoom()),
            left_width: Some(self.left_width),
            right_width: Some(self.right_width),
            show_left: Some(self.show_sidebar),
            show_right: Some(self.show_right),
            goal_beside: Some(view::goal_beside_on()),
        };
        let path = settings_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Apply the vim machine's ops to the editor in use
    fn vim_ops(&mut self, ops: Vec<vim::Op>) -> Task<Message> {
        use cosmic::widget::text_editor::{Action as A, Edit as E, Motion as M};
        let mut tasks = Vec::new();
        for op in ops {
            let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { break };
            let in_block = t.block_editor.is_some() && t.mode == Mode::Literate;
            let act = |a: A| if in_block { Message::BlockAction(a) } else { Message::SourceAction(a) };
            // In the literate view j and k run on past a block's ends into its neighbours
            let crossing = match (&op, &t.block_editor) {
                (vim::Op::Act(A::Move(m @ (M::Down | M::Up))), Some(be)) if in_block => Some((*m, be.content.cursor().position, be.content.line_count())),
                _ => None,
            };
            if let Some((m, before, line_count)) = crossing {
                // Move first: a wrapped paragraph is one line with many rows, so the edge is
                // where the caret no longer moves
                let last_line = before.line + 1 >= line_count;
                let maybe_edge = (m == M::Down && last_line) || (m == M::Up && before.line == 0);
                if !maybe_edge {
                    tasks.push(self.update(act(A::Move(m))));
                    continue;
                }
                // On the first (last) row as laid out, the block is left at once: the editor's
                // remembered column would otherwise move the caret sideways there first, which
                // looks like no edge and takes another press
                let on_edge_row = self
                    .workspace
                    .as_mut()
                    .and_then(|w| w.theory.as_mut())
                    .and_then(|t| t.block_editor.as_mut())
                    .is_some_and(|be| editor::on_edge_row(&mut be.content, m == M::Down));
                if !on_edge_row {
                    tasks.push(self.update(Message::BlockAction(A::Move(m))));
                    let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { break };
                    let Some(be) = &t.block_editor else { continue };
                    if be.content.cursor().position != before {
                        continue;
                    }
                }
                let m = &m;
                // (within a text block, the next paragraph before the next block)
                let machine = self.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default();
                let table = self.table(&machine);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut())
                    && t.adjacent_paragraph(*m == M::Down, &*table)
                {
                    tasks.push(view::centre_selected());
                    continue;
                }
                let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { break };
                let Some(be) = &t.block_editor else { continue };
                if let Some(i) = t.blocks.iter().position(|b| b.start <= be.start && be.start <= b.end) {
                    let target = if *m == M::Down { i + 1 } else { i.wrapping_sub(1) };
                    if target < t.blocks.len() {
                        let down = *m == M::Down;
                        tasks.push(self.update(Message::EditBlock(target)));
                        if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut())
                            && let Some(be) = &mut t.block_editor
                        {
                            be.content.perform(A::Move(if down { M::DocumentStart } else { M::DocumentEnd }));
                            if !down {
                                be.content.perform(A::Move(M::Home));
                            }
                        }
                        tasks.push(view::centre_selected());
                        continue;
                    }
                }
                // (the first or last block: the move was made, and was nowhere to go)
                continue;
            }
            match op {
                vim::Op::Act(a) => tasks.push(self.update(act(a))),
                vim::Op::YankSelection(linewise) => {
                    let sel = match &t.block_editor {
                        Some(be) if in_block => be.content.selection(),
                        _ => t.source.selection(),
                    };
                    self.vim.register = sel.unwrap_or_default();
                    self.vim.linewise = linewise;
                }
                vim::Op::YankLines(n) => {
                    let (text, line) = match &t.block_editor {
                        Some(be) if in_block => (be.content.text(), be.content.cursor().position.line),
                        _ => (t.decoded.text.clone(), t.source.cursor().position.line),
                    };
                    let lines: Vec<&str> = text.split('\n').skip(line).take(n).collect();
                    self.vim.register = format!("{}\n", lines.join("\n"));
                    self.vim.linewise = true;
                }
                vim::Op::Paste { after } => {
                    let reg = self.vim.register.clone();
                    if reg.is_empty() {
                        continue;
                    }
                    let paste = |s: String| A::Edit(E::Paste(Arc::new(s)));
                    if self.vim.linewise {
                        let body = reg.trim_end_matches('\n').to_string();
                        if after {
                            tasks.push(self.update(act(A::Move(M::End))));
                            tasks.push(self.update(act(paste(format!("\n{body}")))));
                        } else {
                            tasks.push(self.update(act(A::Move(M::Home))));
                            tasks.push(self.update(act(paste(format!("{body}\n")))));
                        }
                    } else {
                        if after {
                            tasks.push(self.update(act(A::Move(M::Right))));
                        }
                        tasks.push(self.update(act(paste(reg))));
                    }
                }
                vim::Op::OpenLine(below) => {
                    if below {
                        tasks.push(self.update(act(A::Move(M::End))));
                        tasks.push(self.update(Message::Newline));
                    } else {
                        tasks.push(self.update(act(A::Move(M::Home))));
                        tasks.push(self.update(act(A::Edit(E::Paste(Arc::new("\n".into()))))));
                        tasks.push(self.update(act(A::Move(M::Up))));
                    }
                }
                vim::Op::Undo => tasks.push(self.update(Message::Undo)),
                vim::Op::Redo => tasks.push(self.update(Message::Redo)),
                vim::Op::Save => tasks.push(self.update(Message::Save)),
                vim::Op::Escape => tasks.push(self.update(Message::CloseBlock)),
            }
        }
        Task::batch(tasks)
    }
}

impl App {
    fn link(&self, key: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.target.key == key)
    }

    fn link_mut(&mut self, key: &str) -> Option<&mut Link> {
        self.links.iter_mut().find(|l| l.target.key == key)
    }

    /// The symbols the open session's documents are decoded and typed with: its profile's, from
    /// the table this machine's host sent
    pub fn table(&self, machine: &str) -> Arc<dyn Symbols> {
        let profile = self.workspace.as_ref().map_or(prover::profile(None), |w| w.profile);
        profile.symbols(self.link(machine).and_then(|l| l.symbols.as_ref()))
    }

    /// The open session's profile and capabilities, from its summary: called whenever summaries
    /// change. A summary without `backend` is Isabelle's (a host older than backends)
    fn sync_backend(&mut self) {
        let Some(ws) = &mut self.workspace else { return };
        let summary = self.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
        let profile = prover::profile(summary.and_then(|s| s["backend"].as_str()));
        let caps = Capabilities::from_summary(summary, profile.default_capabilities());
        ws.profile = profile;
        if caps != ws.caps {
            let typed = caps.typed_tools();
            ws.caps = caps;
            if let Some(t) = &mut ws.theory {
                t.typed_tools = typed;
            }
        }
    }

    /// Send a request to a machine's host; the reply comes back as `pending`
    fn send(&mut self, machine: &str, op: &str, params: Value, pending: Pending) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        let mut obj = params;
        obj["op"] = json!(op);
        obj["id"] = json!(id);
        if let Some(link) = self.link(machine)
            && let Some(sender) = &link.sender
            && sender.send(obj.to_string()).is_ok()
        {
            self.pending.insert(id, (machine.to_string(), pending));
        } else {
            self.status = Some(format!("Not connected to {machine}"));
        }
        id
    }

    fn title(&self) -> String {
        match &self.workspace {
            Some(ws) => {
                let name = self.sessions.get(&(ws.machine.clone(), ws.sid.clone())).and_then(|s| s["name"].as_str()).unwrap_or("session");
                match &ws.theory {
                    Some(t) => format!("{}{} — {} — Deductum", if t.dirty { "• " } else { "" }, file_name(&t.path), name),
                    None => format!("{name} — Deductum"),
                }
            }
            None => "Deductum".into(),
        }
    }

    fn retitle(&mut self) -> Task<Message> {
        let title = self.title();
        self.set_header_title(title.clone());
        match self.core.main_window_id() {
            Some(id) => self.set_window_title(title, id),
            None => Task::none(),
        }
    }

    // ----- host messages

    fn on_link(&mut self, key: String, event: link::Event) -> Task<Message> {
        match event {
            link::Event::Connecting(what) => {
                if let Some(l) = self.link_mut(&key) {
                    l.status = what;
                }
            }
            link::Event::Connected(sender) => {
                if let Some(l) = self.link_mut(&key) {
                    l.sender = Some(sender);
                    l.status = "connected".into();
                }
                self.send(&key, "hello", json!({"client": "app", "label": "Deductum"}), Pending::Hello);
                self.send(&key, "sessions", json!({}), Pending::Sessions);
                self.send(&key, "symbol_table", json!({}), Pending::Symbols);
                // Reattach after a reconnect
                if let Some(ws) = &self.workspace
                    && ws.machine == key
                {
                    let (sid, path) = (ws.sid.clone(), ws.theory.as_ref().map(|t| t.path.clone()));
                    self.send(&key, "session_info", json!({"session": sid}), Pending::SessionInfo(sid.clone()));
                    if let Some(path) = path {
                        self.send(&key, "read", json!({"session": sid, "path": path}), Pending::Read(sid, path));
                    }
                }
            }
            link::Event::Disconnected(reason) => {
                if let Some(l) = self.link_mut(&key) {
                    l.sender = None;
                    l.status = format!("disconnected: {}", reason.lines().last().unwrap_or(""));
                }
                self.pending.retain(|_, (m, _)| *m != key);
                if let Some(t) = self.workspace.as_mut().filter(|w| w.machine == key).and_then(|w| w.theory.as_mut()) {
                    t.inflight = None;
                }
            }
            link::Event::Line(value) => {
                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                    if let Some((machine, pending)) = self.pending.remove(&id) {
                        return self.on_reply(machine, pending, value);
                    }
                } else if value.get("event").is_some() {
                    return self.on_event(key, value);
                }
            }
        }
        Task::none()
    }

    fn on_reply(&mut self, machine: String, pending: Pending, v: Value) -> Task<Message> {
        if v["ok"] != json!(true) {
            let error = v["error"].as_str().unwrap_or("error").to_string();
            match pending {
                Pending::Edit(_) => {
                    // The replica is out of step: read the theory again
                    if let Some(ws) = &self.workspace
                        && let Some(t) = &ws.theory
                    {
                        let (sid, path) = (ws.sid.clone(), t.path.clone());
                        self.send(&machine, "read", json!({"session": sid, "path": path}), Pending::Read(sid, path));
                    }
                    self.status = Some(error);
                }
                Pending::State(_) => {
                    if let Some(ws) = &mut self.workspace {
                        ws.output = Some(json!({"error": error, "code": v["code"]}));
                    }
                }
                Pending::Query(op) => {
                    if let Some(ws) = &mut self.workspace {
                        let q = match op {
                            "tool" => &mut ws.tool_query,
                            "deductum_command" => &mut ws.cex,
                            "deductum_goals" => &mut ws.goals,
                            "goals_prev" => &mut ws.goals_prev,
                            _ => &mut ws.find,
                        };
                        q.status = "failed".into();
                        q.error = Some(error);
                    }
                }
                Pending::Fill => {
                    if let Some(ws) = &mut self.workspace {
                        ws.fill.status = "failed".into();
                        ws.fill.error = Some(error);
                    }
                }
                Pending::Create | Pending::Suggest | Pending::ListDir => {
                    if let Some(ns) = &mut self.new_session {
                        ns.error = Some(error);
                    }
                }
                _ => self.status = Some(error),
            }
            return Task::none();
        }
        match pending {
            Pending::Hello => {
                if let Some(l) = self.link_mut(&machine) {
                    l.client_id = v["client"].as_str().map(str::to_string);
                    l.hello = Some(v);
                }
            }
            Pending::Sessions => {
                self.sessions.retain(|(m, _), _| *m != machine);
                for s in v["sessions"].as_array().cloned().unwrap_or_default() {
                    if let Some(id) = s["id"].as_str() {
                        self.sessions.insert((machine.clone(), id.to_string()), s);
                    }
                }
                self.sync_backend();
                return self.handle_request();
            }
            Pending::Symbols => {
                if let Some(table) = prover::host_symbols(&v["symbols"])
                    && let Some(l) = self.link_mut(&machine)
                {
                    l.symbols = Some(table);
                }
            }
            Pending::SessionInfo(sid) => {
                if let Some(ws) = self.workspace.as_mut().filter(|w| w.sid == sid) {
                    ws.log = v["session"]["logs"].as_array().map(|l| l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    ws.info = Some(v["session"].clone());
                }
                if let Some(s) = v["session"].as_object() {
                    let mut summary = Value::Object(s.clone());
                    summary.as_object_mut().map(|o| o.remove("theories"));
                    self.sessions.insert((machine.clone(), sid.clone()), summary);
                    self.sync_backend();
                }
                if let Some(want) = self.want_theory.take() {
                    return self.update(Message::OpenTheory(want));
                }
                // Open the first theory if none is open
                if let Some(ws) = self.workspace.as_ref().filter(|w| w.sid == sid && w.theory.is_none() && w.opening.is_none())
                    && let Some(first) = ws.info.as_ref().and_then(|i| {
                        i["theories"].as_array()?.iter().filter_map(|t| t["path"].as_str()).find(|p| ws.profile.is_document(p)).map(str::to_string)
                    })
                {
                    let _ = ws;
                    return self.update(Message::OpenTheory(first));
                }
            }
            Pending::Read(sid, path) => {
                let table = self.table(&machine);
                let kinds = self.kinds.clone();
                if let Some(ws) = self.workspace.as_mut().filter(|w| w.sid == sid) {
                    let ws_profile = ws.profile;
                    let mode = ws.theory.as_ref().map_or(Mode::Literate, |t| t.mode);
                    let doc = &v["doc"];
                    let real = doc["path"].as_str().unwrap_or(&path).to_string();
                    // Typing the host hasn't confirmed (a reconnect, a resync) is kept,
                    // rebased onto the text read now
                    let pending = ws
                        .theory
                        .as_ref()
                        .filter(|t| t.path == real && t.local != t.confirmed)
                        .map(|t| (t.confirmed.clone(), ot::diff(&t.confirmed, &t.local), t.cursor));
                    let mut theory = Theory::new(real, doc, &*table, kinds, mode, ws_profile);
                    theory.typed_tools = ws.caps.typed_tools();
                    if let Some((old_confirmed, delta, cursor)) = pending {
                        let remote = ot::diff(&old_confirmed, &theory.confirmed);
                        let rebased = ot::rebase_merge(&delta, &remote);
                        theory.cursor = cursor;
                        theory.replace_change(&rebased, &*table);
                    }
                    ws.theory = Some(theory);
                    ws.opening = None;
                    ws.output = None;
                    ws.notes.clear();
                    let goto = ws.pending_goto.take();
                    let sid = ws.sid.clone();
                    self.send(&machine, "history", json!({"session": sid, "path": path, "limit": 60}), Pending::History(sid.clone()));
                    self.send(&machine, "notes", json!({"session": sid, "path": path}), Pending::Notes);
                    if let Some(at) = goto {
                        return Task::batch([self.flush_edits(), self.retitle(), self.update(Message::Goto(at))]);
                    }
                }
                return Task::batch([self.flush_edits(), self.retitle()]);
            }
            Pending::Edit(_) => {}
            Pending::State(generation) => {
                if generation == self.state_gen
                    && let Some(ws) = &mut self.workspace
                {
                    // (decoded with the session's symbols, once its machine is known)
                    let table = self.links.iter().find(|l| l.target.key == ws.machine).map(|l| ws.profile.symbols(l.symbols.as_ref()));
                    if let Some(t) = &mut ws.theory {
                        let state: String = v["messages"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|m| m["kind"] == "state")
                            .filter_map(|m| m["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let state = table.as_ref().map_or(state.clone(), |tb| tb.decode(&state).text);
                        t.goal = t.profile.parse_goal(&state);
                    }
                    ws.output = Some(v);
                    if let Some(tb) = &table {
                        ws.rebuild_views(&**tb);
                    }
                }
            }
            Pending::Query(op) => {
                if let Some(ws) = &mut self.workspace {
                    let q = match op {
                        "tool" => &mut ws.tool_query,
                        "deductum_command" => &mut ws.cex,
                            "deductum_goals" => &mut ws.goals,
                            "goals_prev" => &mut ws.goals_prev,
                        _ => &mut ws.find,
                    };
                    q.id = v["query"].as_str().map(str::to_string);
                }
            }
            Pending::Fill => {
                if let Some(ws) = &mut self.workspace {
                    ws.fill.job = v["job"].as_str().map(str::to_string);
                    ws.fill.replace = v["replace"].as_object().and_then(|r| Some((r["start"].as_u64()? as usize, r["end"].as_u64()? as usize)));
                    ws.fill.revision = v["revision"].as_u64().unwrap_or(0);
                }
            }
            Pending::History(sid) => {
                if let Some(ws) = self.workspace.as_mut().filter(|w| w.sid == sid) {
                    ws.history = v["history"].as_array().cloned().unwrap_or_default();
                }
            }
            Pending::ListDir => {
                if let Some(ns) = &mut self.new_session {
                    ns.folder = v["path"].as_str().unwrap_or("").to_string();
                    ns.listing = Some(v);
                    let folder = ns.folder.clone();
                    let machine = self.links.get(ns.machine).map(|l| l.target.key.clone()).unwrap_or_default();
                    self.send(&machine, "suggest", json!({"folder": folder}), Pending::Suggest);
                }
            }
            Pending::Suggest => {
                if let Some(ns) = &mut self.new_session {
                    ns.logic = v["suggestion"]["logic"].as_str().unwrap_or("HOL").to_string();
                    ns.suggestion = Some(v["suggestion"].clone());
                }
            }
            Pending::Create => {
                self.new_session = None;
                if let Some(id) = v["session"]["id"].as_str() {
                    self.sessions.insert((machine.clone(), id.to_string()), v["session"].clone());
                    return self.update(Message::OpenSession(machine, id.to_string()));
                }
            }
            Pending::Info(generation) => {
                let table = self.table(&machine);
                if let Some(ws) = self.workspace.as_mut()
                    && let Some(h) = ws.hover.as_mut().filter(|h| h.generation == generation)
                {
                    h.info = Some(v["info"].clone());
                    ws.rebuild_views(&*table);
                }
            }
            Pending::Definition(window) => {
                let d = v["info"]["definition"].clone();
                if !(d.is_object() && d["file"].is_string()) {
                    self.status = Some("No definition there".into());
                    return Task::none();
                }
                if window {
                    if let Some(sid) = self.workspace.as_ref().map(|w| w.sid.clone()) {
                        self.send(&machine, "read", json!({"session": sid, "path": d["file"], "peek": true}), Pending::RefRead(d, None));
                    }
                    return Task::none();
                }
                // Remember where this was, for Back
                if let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) {
                    self.back.push((t.path.clone(), Self::host_offset(t, t.cursor)));
                }
                let place = json!({"path": d["file"], "start": d["offset"].as_u64().unwrap_or(0)});
                return self.go_to_place(&place);
            }
            Pending::RefRead(d, target) => {
                let doc = &v["doc"];
                let path = doc["path"].as_str().or(d["file"].as_str()).unwrap_or("").to_string();
                let table = self.table(&machine);
                // (a theory of the open session's backend)
                let ws_profile = self.workspace.as_ref().map_or(prover::profile(None), |w| w.profile);
                let mut theory = Theory::new(path.clone(), doc, &*table, self.kinds.clone(), Mode::Source, ws_profile);
                if let Some(ws) = &self.workspace {
                    theory.typed_tools = ws.caps.typed_tools();
                }
                let line = (d["line"].as_u64().unwrap_or(1).max(1) - 1) as usize;
                let start = d["offset"]
                    .as_u64()
                    .map(|o| o as usize)
                    .unwrap_or_else(|| theory.local.split('\n').take(line).map(|l| l.chars().count() + 1).sum());
                theory.cursor = start;
                editor::place(&mut theory.source, cosmic::widget::text_editor::Position { line, column: 0 });
                let name = d["name"].as_str().unwrap_or("").to_string();
                let title = format!("{name} — {}, line {}", file_name(&path), line + 1);
                let library = doc["library"].as_bool().unwrap_or(false);
                let r = Reference { title: title.clone(), path, start, line, theory, library, place: d.clone(), back: Vec::new() };
                if let Some(id) = target
                    && let Some((_, slot)) = self.references.iter_mut().find(|(w, _)| *w == id)
                {
                    let back = std::mem::take(&mut slot.back);
                    *slot = r;
                    slot.back = back;
                    return Task::batch([self.set_window_title(title, id), Task::done(cosmic::Action::App(Message::RefScroll(id)))]);
                }
                let (id, open) = cosmic::iced::window::open(cosmic::iced::window::Settings {
                    size: Size::new(820.0, 520.0),
                    min_size: Some(Size::new(360.0, 200.0)),
                    decorations: true,
                    ..Default::default()
                });
                self.references.push((id, r));
                // (scrolled to the definition once the window has drawn)
                let scroll = Task::perform(async { tokio::time::sleep(Duration::from_millis(150)).await }, move |_| cosmic::Action::App(Message::RefScroll(id)));
                return Task::batch([open.map(|_| cosmic::Action::None), self.set_window_title(title, id), scroll]);
            }
            Pending::RefInfo(id, how) => {
                return self.ref_found(id, how, v["info"].clone());
            }
            Pending::Entity(id, how) => {
                self.entity = v["query"].as_str().map(|q| (q.to_string(), id, how));
            }
            Pending::Notes => {
                if let Some(ws) = &mut self.workspace {
                    ws.notes = v["notes"].as_array().cloned().unwrap_or_default();
                }
            }
            Pending::Undo => {
                let mut task = Task::none();
                if let Some(ws) = &mut self.workspace
                    && let Some(t) = &mut ws.theory
                {
                    let note = v["note"].as_str().unwrap_or("").to_string();
                    self.status = (!note.is_empty()).then_some(note);
                    if let Some(at) = v["at"].as_u64() {
                        task = Task::done(cosmic::Action::App(Message::Goto(Self::local_offset(t, at as usize))));
                    }
                }
                return Task::batch([task, self.flush_edits()]);
            }
            Pending::Plain => {
                if let Some(files) = v.get("files")
                    && let Some(ws) = &mut self.workspace
                {
                    ws.merge = files.as_array().cloned();
                }
            }
        }
        Task::none()
    }

    fn on_event(&mut self, machine: String, v: Value) -> Task<Message> {
        let ev = v["event"].as_str().unwrap_or("");
        let table = self.table(&machine);
        let client_id = self.link(&machine).and_then(|l| l.client_id.clone());
        match ev {
            "session" => {
                if let Some(id) = v["session"]["id"].as_str() {
                    self.sessions.insert((machine.clone(), id.to_string()), v["session"].clone());
                    self.sync_backend();
                }
            }
            "session_removed" => {
                if let Some(id) = v["session"].as_str() {
                    self.sessions.remove(&(machine.clone(), id.to_string()));
                    if self.workspace.as_ref().is_some_and(|w| w.sid == id) {
                        self.workspace = None;
                        return self.retitle();
                    }
                }
            }
            "session_log" => {
                if let Some(ws) = self.workspace.as_mut().filter(|w| Some(w.sid.as_str()) == v["session"].as_str()) {
                    ws.log.push(v["text"].as_str().unwrap_or("").to_string());
                    if ws.log.len() > 300 {
                        ws.log.remove(0);
                    }
                }
            }
            "theory_status" => {
                if let Some(ws) = self.workspace.as_mut().filter(|w| Some(w.sid.as_str()) == v["session"].as_str())
                    && let Some(info) = &mut ws.info
                    && let Some(theories) = info["theories"].as_array_mut()
                {
                    for t in theories {
                        if let Some(p) = t["path"].as_str()
                            && let Some(st) = v["status"].get(p)
                        {
                            t["status"] = st.clone();
                        }
                    }
                }
            }
            "doc_changed" => {
                let mut resync = None;
                let mut refresh_history = None;
                if let Some(ws) = self.workspace.as_mut().filter(|w| Some(w.sid.as_str()) == v["session"].as_str())
                    && let Some(t) = &mut ws.theory
                    && Some(t.path.as_str()) == v["path"].as_str()
                {
                    let change: ot::Change = v["entry"]["change"]
                        .as_array()
                        .map(|cs| {
                            cs.iter()
                                .filter_map(|c| Some((c[0].as_u64()? as usize, c[1].as_u64()? as usize, c[2].as_str()?.to_string())))
                                .collect()
                        })
                        .unwrap_or_default();
                    let revision = v["revision"].as_u64().unwrap_or(0);
                    let mine = v["client"].as_str().is_some() && v["client"].as_str() == client_id.as_deref();
                    let request = v["request"].as_u64();
                    if !t.committed(revision, &change, request, mine, &*table) {
                        resync = Some((ws.sid.clone(), t.path.clone()));
                    }
                    t.dirty = v["dirty"].as_bool().unwrap_or(t.dirty);
                    t.can_undo = v["can_undo"].as_bool().unwrap_or(t.can_undo);
                    t.can_redo = v["can_redo"].as_bool().unwrap_or(t.can_redo);
                    if v["entry"]["author"]["kind"] != json!("human") || v["entry"]["kind"] != json!("edit") {
                        refresh_history = Some((ws.sid.clone(), t.path.clone()));
                    }
                }
                if let Some((sid, path)) = resync {
                    self.send(&machine, "read", json!({"session": sid, "path": path}), Pending::Read(sid, path));
                }
                if let Some((sid, path)) = refresh_history {
                    self.send(&machine, "history", json!({"session": sid, "path": path, "limit": 60}), Pending::History(sid));
                }
                return self.flush_edits();
            }
            "doc_prover" => {
                if let Some(ws) = self.workspace.as_mut().filter(|w| Some(w.sid.as_str()) == v["session"].as_str())
                    && let Some(t) = &mut ws.theory
                    && Some(t.path.as_str()) == v["path"].as_str()
                {
                    let before = t.caret_signature();
                    t.prover_update(&v["prover"], &*table);
                    // The command at the caret moved on (sledgehammer said more, a proof
                    // finished): its state again
                    if t.caret_signature() != before {
                        if t.caret_signature().2 == theory::Status::Running && ws.tool_started.is_none() {
                            ws.tool_started = Some(std::time::Instant::now());
                        }
                        return self.schedule_state();
                    }
                }
            }
            "doc_saved" => {
                if let Some(t) = self.theory_for(&v) {
                    t.dirty = v["dirty"].as_bool().unwrap_or(false);
                    t.save_error = v["error"].as_str().map(str::to_string);
                }
            }
            "doc_conflict" => {
                if let Some(t) = self.theory_for(&v) {
                    t.conflict = v.get("conflict").filter(|c| !c.is_null()).cloned();
                }
            }
            "doc_alternatives" => {
                if let Some(t) = self.theory_for(&v) {
                    t.alternatives = v["alternatives"].as_array().cloned().unwrap_or_default();
                }
            }
            "query" => {
                if let Some((q, window, how)) = self.entity.clone()
                    && v["id"].as_str() == Some(q.as_str())
                    && matches!(v["status"].as_str(), Some("finished" | "failed" | "canceled"))
                {
                    self.entity = None;
                    let messages = v["messages"].as_array().cloned().unwrap_or_default();
                    return self.ref_found(window, how, editor::entity_info(&messages));
                }
                if let Some(ws) = &mut self.workspace {
                    let id = v["id"].as_str();
                    for q in [&mut ws.tool_query, &mut ws.find, &mut ws.cex, &mut ws.goals, &mut ws.goals_prev] {
                        if q.id.as_deref() == id && id.is_some() {
                            q.status = v["status"].as_str().unwrap_or("").to_string();
                            // The state keeps showing the last one until the new one is ready
                            let pending = q.operation == "deductum_goals" && matches!(q.status.as_str(), "waiting" | "running");
                            if !pending {
                                q.messages = v["messages"].as_array().cloned().unwrap_or_default();
                            }
                            q.replace = v["replace"].as_object().and_then(|r| Some((r["start"].as_u64()? as usize, r["end"].as_u64()? as usize)));
                            q.end = v["end"].as_u64().map(|e| e as usize);
                            q.revision = v["revision"].as_u64().unwrap_or(0);
                            q.error = v["error"].as_str().map(str::to_string);
                        }
                    }
                    ws.rebuild_views(&*table);
                }
            }
            "notes" => {
                if let Some(ws) = self.workspace.as_mut().filter(|w| Some(w.sid.as_str()) == v["session"].as_str())
                    && ws.theory.as_ref().is_some_and(|t| Some(t.path.as_str()) == v["path"].as_str())
                {
                    ws.notes = v["notes"].as_array().cloned().unwrap_or_default();
                }
            }
            "show" => {
                // An agent points at a place: go if the human follows it, else offer
                let follow = self.workspace.as_ref().and_then(|w| w.follow.clone());
                if follow.is_some() && follow.as_deref() == v["from"]["key"].as_str() {
                    return self.go_to_place(&v);
                }
                if let Some(ws) = self.workspace.as_mut().filter(|w| Some(w.sid.as_str()) == v["session"].as_str()) {
                    ws.show_offer = Some(v);
                }
            }
            "theories_changed" => {
                // A theory was added or deleted on disk: the list again
                if let Some(sid) = self.workspace.as_ref().filter(|w| Some(w.sid.as_str()) == v["session"].as_str()).map(|w| w.sid.clone()) {
                    self.send(&machine, "session_info", json!({"session": sid}), Pending::SessionInfo(sid.clone()));
                }
            }
            "fill" => {
                // A fill the app did not ask for: the human typed `claude` or `codex` in the theory
                if let Some(ws) = self.workspace.as_mut().filter(|w| Some(w.sid.as_str()) == v["session"].as_str())
                    && ws.fill.job.as_deref() != v["job"].as_str()
                    && let Some(r) = v["replace"].as_object()
                    && let (Some(start), Some(end)) = (r["start"].as_u64(), r["end"].as_u64())
                {
                    ws.fill = FillState {
                        job: v["job"].as_str().map(str::to_string),
                        agent: usize::from(v["agent"] == json!("codex")),
                        model: v["model"].as_str().map(str::to_string),
                        hint: v["hint"].as_str().unwrap_or("").to_string(),
                        replace: Some((start as usize, end as usize)),
                        revision: v["revision"].as_u64().unwrap_or(0),
                        started: Some(std::time::Instant::now()),
                        ..Default::default()
                    };
                    ws.panel = Panel::Tool;
                }
                // Its keyword deleted (or a proof inserted for it): its proofs go
                if v["status"] == json!("canceled") {
                    if let Some(ws) = self.workspace.as_mut().filter(|w| w.fill.job.as_deref() == v["job"].as_str()) {
                        ws.fill = FillState { agent: ws.fill.agent, ..Default::default() };
                    }
                    return Task::none();
                }
                if let Some(ws) = self.workspace.as_mut().filter(|w| w.fill.job.as_deref() == v["job"].as_str()) {
                    if let Some(a) = v["asked"].as_str() {
                        ws.fill.asked = Some(a.to_string());
                    }
                    if let Some(m) = v["message"].as_str() {
                        ws.fill.message = Some(m.to_string());
                    }
                    if v["conversation"].is_object() {
                        ws.fill.conversation = Some(v["conversation"].clone());
                    }
                    ws.fill.status = v["status"].as_str().unwrap_or("").to_string();
                    ws.fill.candidates = v["candidates"].as_array().cloned().unwrap_or_default();
                    if let Some(using) = v["using"].as_array() {
                        ws.fill.using = using.iter().filter_map(|f| f.as_str().map(str::to_string)).collect();
                    }
                    ws.fill.error = v["error"].as_str().map(str::to_string);
                }
            }
            "activity" => {
                if let Some(ws) = self.workspace.as_mut().filter(|w| Some(w.sid.as_str()) == v["session"].as_str()) {
                    // (the listening agents' names, for completing `@name`)
                    editor::set_listeners(
                        v["activity"]["listeners"].as_array().into_iter().flatten().filter_map(|l| l["label"].as_str()).map(editor::at_name).collect(),
                    );
                    ws.activity = Some(v["activity"].clone());
                }
            }
            "agent_position" => {
                let follow = self.workspace.as_ref().and_then(|w| w.follow.clone());
                let key = v["client"]["key"].as_str().map(str::to_string);
                if follow.is_some() && follow == key
                    && let Some(path) = v["client"]["position"]["path"].as_str()
                {
                    let current = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).map(|t| t.path.clone());
                    if current.as_deref() != Some(path) {
                        return self.update(Message::OpenTheory(path.to_string()));
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn theory_for(&mut self, v: &Value) -> Option<&mut Theory> {
        self.workspace
            .as_mut()
            .filter(|w| Some(w.sid.as_str()) == v["session"].as_str())?
            .theory
            .as_mut()
            .filter(|t| Some(t.path.as_str()) == v["path"].as_str())
    }

    /// Send the next edit, if any
    fn flush_edits(&mut self) -> Task<Message> {
        let Some(ws) = &self.workspace else { return Task::none() };
        let machine = ws.machine.clone();
        let sid = ws.sid.clone();
        let id = self.next_id + 1;
        let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
        if let Some((revision, change, txn)) = t.outgoing(id) {
            let path = t.path.clone();
            let edits: Vec<Value> = change.iter().map(|(s, e, text)| json!({"start": s, "end": e, "text": text})).collect();
            let sent = self.send(
                &machine,
                "edit",
                json!({"session": sid, "path": path, "revision": revision, "edits": edits, "txn": txn, "request": id, "policy": "merge"}),
                Pending::Edit(path),
            );
            debug_assert_eq!(sent, id);
        } else if t.settled()
            && let Some(op) = t.queued.pop_front()
        {
            // An undo or redo waits for the edits typed before it
            let path = t.path.clone();
            self.send(&machine, op, json!({"session": sid, "path": path}), Pending::Undo);
        }
        Task::none()
    }

    /// A caret moved: ask for the proof state there shortly
    fn schedule_state(&mut self) -> Task<Message> {
        // What the prover said for another cell (a tool's findings, the goal) is not shown in
        // this one while its own state comes
        if let Some(ws) = &mut self.workspace
            && let Some(t) = &ws.theory
            && let Some(b) = t.blocks.iter().find(|b| b.start <= t.cursor && t.cursor <= b.end)
            && let Some(cmd) = ws.output.as_ref().and_then(|o| o["command"]["start"].as_u64())
        {
            let at = Self::local_offset(t, cmd as usize);
            if at < b.start || at > b.end {
                ws.output = None;
                ws.goals.messages.clear();
                ws.goals_prev.messages.clear();
            }
        }
        self.state_gen += 1;
        let generation = self.state_gen;
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(220)).await;
                generation
            },
            |generation| cosmic::Action::App(Message::StateTick(generation)),
        )
    }

    /// The caret's offset in the host's current text
    fn host_offset(t: &Theory, offset: usize) -> usize {
        let back = ot::diff(&t.local, &t.confirmed);
        ot::map_pos(offset, &back, false)
    }

    pub fn local_offset_pub(t: &Theory, offset: usize) -> usize {
        Self::local_offset(t, offset)
    }

    fn local_offset(t: &Theory, offset: usize) -> usize {
        let fwd = ot::diff(&t.confirmed, &t.local);
        ot::map_pos(offset, &fwd, false)
    }

    /// The source offset under a point of an editor: a click there, undone, says where it lands
    fn hit(&mut self, target: HoverTarget, point: cosmic::iced::Point) -> Option<usize> {
        use cosmic::widget::text_editor::Action as A;
        // (over the editor's padding the point is just outside its text, where nothing is hit)
        let point = cosmic::iced::Point::new(point.x.max(0.0), point.y.max(0.0));
        // (a selection made by double or triple click is held as "the word/line around this
        // point", which the cursor cannot say: probing would lose it, so there is no hover while
        // text is selected, but for vim's one-character block, which the cursor says exactly)
        let vim_block = self.vim_block;
        let selecting = |content: &cosmic::widget::text_editor::Content| content.cursor().selection.is_some() && !vim_block;
        if let HoverTarget::Ref(id) = target {
            let t = &mut self.references.iter_mut().find(|(w, _)| *w == id)?.1.theory;
            if selecting(&t.source) {
                return None;
            }
            let saved = t.source.cursor();
            t.source.perform(A::Click(point));
            let pos = t.source.cursor().position;
            t.source.move_to(saved);
            if saved.selection.is_none() {
                t.source.perform(A::ClearSelection);
            }
            return Some(t.decoded.to_source(t.lines.offset(&t.decoded.text, pos)));
        }
        let t = self.workspace.as_mut()?.theory.as_mut()?;
        let mut probe = |content: &mut cosmic::widget::text_editor::Content| {
            if selecting(content) {
                return None;
            }
            let saved = content.cursor();
            content.perform(A::Click(point));
            let pos = content.cursor().position;
            content.move_to(saved);
            if saved.selection.is_none() {
                content.perform(A::ClearSelection);
            }
            Some(pos)
        };
        match target {
            HoverTarget::Source => {
                let pos = probe(&mut t.source)?;
                Some(t.decoded.to_source(t.lines.offset(&t.decoded.text, pos)))
            }
            HoverTarget::Block => {
                let be = t.block_editor.as_mut()?;
                let pos = probe(&mut be.content)?;
                let shown = be.shown();
                Some(be.start + be.decoded.to_source(editor::LineIndex::new(&shown).offset(&shown, pos)))
            }
            HoverTarget::Message(key) => {
                let ws = self.workspace.as_mut()?;
                let v = ws.views.get_mut(&key)?;
                if selecting(&v.content) {
                    return None;
                }
                let saved = v.content.cursor();
                v.content.perform(A::Click(point));
                let pos = v.content.cursor().position;
                v.content.move_to(saved);
                if saved.selection.is_none() {
                    v.content.perform(A::ClearSelection);
                }
                let text = v.content.text();
                Some(editor::LineIndex::new(&text).offset(&text, pos))
            }
            HoverTarget::Ref(_) => None,
            HoverTarget::View(i) => {
                let start = t.blocks.get(i)?.start;
                let v = t.block_views.get_mut(i)?.as_mut()?;
                let pos = probe(&mut v.content)?;
                let text = v.content.text();
                let ds = t.decoded.to_decoded(start);
                Some(t.decoded.to_source(ds + editor::LineIndex::new(&text).offset(&text, pos)))
            }
        }
    }

    /// Vim's block cursor in normal mode: the character under the caret shown selected (a thin
    /// caret for insert mode and selections of its own). Taken down before each command
    fn vim_caret(&mut self, show: bool) {
        use cosmic::widget::text_editor::{Cursor, Position};
        let vim_on = self.vim_enabled;
        let normal = self.vim.mode == vim::Mode::Normal;
        let was = std::mem::replace(&mut self.vim_block, false);
        let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return };
        let in_block = t.block_editor.is_some() && t.mode == Mode::Literate;
        let content = match &mut t.block_editor {
            Some(be) if in_block => &mut be.content,
            _ => &mut t.source,
        };
        let cursor = content.cursor();
        if !show {
            if was && let Some(anchor) = cursor.selection {
                // (move_to leaves an empty selection, which the next motion would only clear)
                editor::place(content, anchor);
            }
            return;
        }
        // (a click leaves an empty selection at the caret: no selection, for this)
        if !vim_on || !normal || cursor.selection.is_some_and(|s| s != cursor.position) {
            return;
        }
        let mut pos = cursor.position;
        // (columns are bytes: the block covers the whole character under the caret)
        let text = content.text();
        let line = text.split('\n').nth(pos.line).unwrap_or("");
        // (past a line's end, as a click there leaves it, normal mode rests on its last character)
        if pos.column >= line.len()
            && let Some((last, _)) = line.char_indices().last()
        {
            pos.column = last;
        }
        if let Some(c) = line.get(pos.column..).and_then(|rest| rest.chars().next()) {
            content.move_to(Cursor { position: Position { line: pos.line, column: pos.column + c.len_utf8() }, selection: Some(pos) });
            self.vim_block = true;
        }
    }

    /// The mouse in vim keys: dragging a selection is visual mode, a click leaves it
    fn vim_mouse(&mut self, action: &Action) {
        use cosmic::widget::text_editor::Action as A;
        match action {
            A::Click(_) => {
                self.vim_block = false;
                if matches!(self.vim.mode, vim::Mode::Visual | vim::Mode::VisualLine) {
                    self.vim.mode = vim::Mode::Normal;
                }
            }
            A::Drag(_) | A::SelectWord | A::SelectLine if self.vim_enabled && self.vim.mode == vim::Mode::Normal => {
                self.vim_block = false;
                self.vim.mode = vim::Mode::Visual;
            }
            _ => {}
        }
    }

    /// No editor keeps a caret (read-only views take focus when clicked; only the open block's
    /// editor should show one)
    fn unfocus_all() -> Task<Message> {
        cosmic::iced::advanced::widget::operate(cosmic::iced::advanced::widget::operation::focusable::unfocus())
    }

    /// What is at a place in a reference window's theory: from the prover's markup when the
    /// session checks that theory, else the name there looked up in the main theory's context
    fn ref_lookup(&mut self, id: cosmic::iced::window::Id, at: usize, how: RefUse) -> Task<Message> {
        let Some(ws) = &self.workspace else { return Task::none() };
        let Some((_, r)) = self.references.iter().find(|(w, _)| *w == id) else { return Task::none() };
        let (machine, sid) = (ws.machine.clone(), ws.sid.clone());
        if !r.library {
            let (path, rev) = (r.path.clone(), r.theory.confirmed_rev);
            self.send(&machine, "info", json!({"session": sid, "path": path, "offset": at, "revision": rev}), Pending::RefInfo(id, how));
            return Task::none();
        }
        let Some(name) = r.theory.profile.name_at(&r.theory.local, at) else { return Task::none() };
        let Some(t) = &ws.theory else { return Task::none() };
        let (path, offset, rev) = (t.path.clone(), Self::host_offset(t, t.cursor), t.confirmed_rev);
        self.send(
            &machine,
            "query",
            json!({"session": sid, "path": path, "offset": offset, "revision": rev, "operation": "deductum_entity", "args": [name]}),
            Pending::Entity(id, how),
        );
        Task::none()
    }

    /// What a look-up in a reference window found: the tooltip, or the definition to go to
    fn ref_found(&mut self, id: cosmic::iced::window::Id, how: RefUse, info: Value) -> Task<Message> {
        match how {
            RefUse::Hover(generation) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                if let Some(ws) = self.workspace.as_mut()
                    && let Some(h) = ws.hover.as_mut().filter(|h| h.generation == generation)
                {
                    h.info = Some(info);
                    ws.rebuild_views(&*table);
                }
            }
            RefUse::Goto(window) => {
                let d = info["definition"].clone();
                if !(d.is_object() && d["file"].is_string()) {
                    self.status = Some("No definition there".into());
                    return Task::none();
                }
                let Some(ws) = &self.workspace else { return Task::none() };
                let (machine, sid) = (ws.machine.clone(), ws.sid.clone());
                // Alt+click: another window; Ctrl+click: this window moves there (Back returns)
                let target = (!window).then_some(id);
                if let Some((_, r)) = self.references.iter_mut().find(|(w, _)| Some(*w) == target) {
                    let here = r.place.clone();
                    r.back.push(here);
                }
                self.send(&machine, "read", json!({"session": sid, "path": d["file"], "peek": true}), Pending::RefRead(d, target));
            }
        }
        Task::none()
    }

    /// The open block has no text left (Backspace there takes the block away)
    fn open_block_empty(&self) -> bool {
        self.workspace
            .as_ref()
            .and_then(|w| w.theory.as_ref())
            .and_then(|t| t.block_editor.as_ref().filter(|_| t.mode == Mode::Literate).map(|be| (t, be)))
            .is_some_and(|(t, be)| {
                // (a text or heading block: empty when its paragraph is and nothing but its
                // command and brackets is around it)
                let bare = |s: &str| t.profile.bare_prose(s);
                be.content.text().trim().is_empty() && be.wrap.as_ref().is_none_or(|(before, after)| bare(before) && bare(after))
            })
    }

    /// Take the open block away (Backspace in it when empty, `dd` on its only line), and go to the
    /// end of the block before it, as joining lines does; Undo brings it back
    fn delete_open_block(&mut self) -> Task<Message> {
        self.remove_open_block(true)
    }

    /// Take the open block away; with `open_previous`, go to the end of the block before it.
    /// Returns what was removed (start, length) for indices taken before to be mapped
    fn remove_open_block(&mut self, open_previous: bool) -> Task<Message> {
        use cosmic::widget::text_editor::{Action as A, Motion as M};
        let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
        let table = self.table(&machine);
        let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
        let Some(be) = t.block_editor.as_ref() else { return Task::none() };
        let (start, end) = (be.start, be.end);
        let empty = be.content.text().trim().is_empty();
        let prose = be.wrap.is_some();
        // (a paragraph being edited starts inside its block)
        let index = t.blocks.iter().position(|b| if prose { b.start <= start && start <= b.end } else { b.start == start });
        let empty = empty && !prose;
        // (strictly before: an emptied block may stay in the list, with no length, where it was)
        let before = t.blocks.iter().rposition(|b| b.start < start && b.end <= start && b.kind != theory::Kind::Header);
        let mut tasks = Vec::new();
        match index.filter(|_| !empty) {
            Some(i) => tasks.push(self.update(Message::DeleteBlock(i))),
            None => {
                // (no text, so perhaps no block left there: the blank lines it leaves go)
                let chars: Vec<char> = t.local.chars().collect();
                let mut e = end.min(chars.len());
                while e < chars.len() && chars[e].is_whitespace() && e < start + 2 + (end - start) {
                    e += 1;
                }
                t.block_editor = None;
                t.break_step();
                t.replace(start, e, "", &*table);
                t.break_step();
                tasks.push(self.flush_edits());
            }
        }
        if let Some(b) = before.filter(|_| open_previous) {
            tasks.push(self.update(Message::EditBlock(b)));
            if let Some(be) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()).and_then(|t| t.block_editor.as_mut()) {
                be.content.perform(A::Move(M::DocumentEnd));
            }
            tasks.push(view::centre_selected());
        }
        Task::batch(tasks)
    }

    /// Ctrl+click: the definition of what is at the caret (Alt+click: in a window of its own)
    fn goto_definition(&mut self) -> Task<Message> {
        let window = self.alt;
        let Some(ws) = &self.workspace else { return Task::none() };
        let Some(t) = &ws.theory else { return Task::none() };
        let at = Self::host_offset(t, t.cursor);
        let (machine, sid, path, rev) = (ws.machine.clone(), ws.sid.clone(), t.path.clone(), t.confirmed_rev);
        self.send(&machine, "info", json!({"session": sid, "path": path, "offset": at, "revision": rev}), Pending::Definition(window));
        Task::none()
    }

    /// Go to a place a host event names (path, start as a host offset), opening its theory first
    fn go_to_place(&mut self, place: &Value) -> Task<Message> {
        let (Some(path), Some(start)) = (place["path"].as_str(), place["start"].as_u64()) else { return Task::none() };
        let Some(ws) = &mut self.workspace else { return Task::none() };
        if ws.theory.as_ref().is_some_and(|t| t.path == path) {
            let t = ws.theory.as_ref().unwrap();
            let at = Self::local_offset(t, start as usize);
            return self.update(Message::Goto(at));
        }
        ws.pending_goto = Some(start as usize);
        self.update(Message::OpenTheory(path.to_string()))
    }

    fn workspace_ids(&self) -> Option<(String, String)> {
        self.workspace.as_ref().map(|w| (w.machine.clone(), w.sid.clone()))
    }

    fn handle_request(&mut self) -> Task<Message> {
        let Some(request) = self.request.clone() else { return Task::none() };
        let wanted = request.machine.clone().unwrap_or_else(|| "local".into());
        // A machine named as Terminal Agents names it, or an ssh destination
        let machine = match self.links.iter().find(|l| l.target.key == wanted || l.target.destination.as_deref() == Some(wanted.as_str())) {
            Some(l) => l.target.key.clone(),
            None => {
                self.links.push(Link {
                    target: link::Target { key: wanted.clone(), destination: Some(wanted.clone()) },
                    name: wanted.clone(),
                    sender: None,
                    status: "starting".into(),
                    hello: None,
                    client_id: None,
                    symbols: None,
                });
                return Task::none();
            }
        };
        if self.link(&machine).and_then(|l| l.sender.as_ref()).is_none() {
            return Task::none();
        }
        self.request = None;
        if let Some(sid) = &request.session {
            return self.update(Message::OpenSession(machine, sid.clone()));
        }
        if let Some(path) = &request.path {
            // (a document of some backend: its folder's session, opened at it)
            let (folder, theory) = if prover::profiles().iter().any(|p| p.is_document(path)) {
                (std::path::Path::new(path).parent().map(|p| p.display().to_string()).unwrap_or_default(), Some(path.clone()))
            } else {
                (path.clone(), None)
            };
            self.want_theory = theory;
            self.send(&machine, "session_create", json!({"folder": folder, "start": true, "resume": true}), Pending::Create);
        }
        Task::none()
    }
}

/// The key of a message's code view
pub fn message_key(m: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    m["raw"].as_str().or_else(|| m["text"].as_str()).unwrap_or("").hash(&mut h);
    m["kind"].as_str().unwrap_or("").hash(&mut h);
    h.finish()
}

impl Workspace {
    /// Code views for the messages on show: the proof state and the queries' output
    pub fn rebuild_views(&mut self, table: &dyn Symbols) {
        let mut wanted: Vec<&Value> = Vec::new();
        if let Some(out) = &self.output {
            wanted.extend(out["messages"].as_array().into_iter().flatten());
        }
        for q in [&self.tool_query, &self.find, &self.cex] {
            wanted.extend(q.messages.iter());
        }
        let goals = goals::Goals::parse(&self.goals.messages);
        wanted.extend(goals.parts());
        if let Some(info) = self.hover.as_ref().and_then(|h| h.info.as_ref()) {
            wanted.extend(info["tooltip"]["items"].as_array().into_iter().flatten());
        }
        let keys: std::collections::HashSet<u64> = wanted.iter().map(|m| message_key(m)).collect();
        self.views.retain(|k, _| keys.contains(k));
        for m in wanted {
            self.views.entry(message_key(m)).or_insert_with(|| editor::CodeView::from_message(m, table));
        }
    }
}

/// The caret's line, if it reads `claude` or `codex` (and a hint): an LLM asked for a proof
/// How long typing must pause on a `claude` or `codex` line before it asks: long enough to finish
/// `claude[opus]` and a hint after a hesitation (Enter asks at once)
const KEYWORD_PAUSE: Duration = Duration::from_millis(2500);

pub fn keyword_line(t: &Theory) -> Option<String> {
    let chars: Vec<char> = t.local.chars().collect();
    let at = t.cursor.min(chars.len());
    let start = chars[..at].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
    let end = chars[at..].iter().position(|c| *c == '\n').map_or(chars.len(), |i| at + i);
    let line: String = chars[start..end].iter().collect();
    t.profile.keyword_line(&line).then_some(line)
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl cosmic::Application for App {
    type Executor = executor::Default;
    type Flags = Flags;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(mut core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        // (no inset card around the content: the theory's scroll bar at the window's edge, as in
        // the terminal)
        core.window.content_container = false;
        let links = link::targets()
            .into_iter()
            .map(|(name, target)| Link {
                target,
                name,
                sender: None,
                status: "starting".into(),
                hello: None,
                client_id: None,
                symbols: None,
            })
            .collect();
        let mut mode_model = widget::segmented_button::SingleSelectModel::default();
        let lit = mode_model.insert().text("Literate").id();
        let src = mode_model.insert().text("Source").id();
        mode_model.activate(lit);
        let mode_entities = [lit, src];
        let mut app = App {
            core,
            links,
            ctrl: false,
            alt: false,
            back: Vec::new(),
            references: Vec::new(),
            entity: None,
            hover_gen: 0,
            keyword_gen: 0,
            prose_pointer: None,
            vim_block: false,
            goal_placement_override: None,
            sessions: BTreeMap::new(),
            next_id: 0,
            pending: HashMap::new(),
            workspace: None,
            new_session: None,
            show_sidebar: false,
            status: None,
            state_gen: 0,
            request: flags.request,
            kinds: Arc::new(HashMap::new()),
            popout: None,
            want_theory: None,
            left: LeftMode::Sessions,
            show_right: false,
            left_width: 290.0,
            right_width: 420.0,
            dragging: None,
            window_width: 1400.0,
            vim: vim::Vim::default(),
            vim_enabled: false,
            mode_model,
            mode_entities,
        };
        let settings = load_settings();
        app.vim_enabled = settings.vim;
        if let Some(z) = settings.zoom {
            view::set_zoom(z);
        }
        app.left_width = settings.left_width.unwrap_or(app.left_width);
        app.right_width = settings.right_width.unwrap_or(app.right_width);
        app.show_sidebar = settings.show_left.unwrap_or(false);
        app.show_right = settings.show_right.unwrap_or(false);
        view::set_goal_beside(settings.goal_beside.unwrap_or(true));
        load_fonts();
        let title = app.retitle();
        (app, Task::batch([title, cli::listen()]))
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        view::header_start(self)
    }

    fn header_center(&self) -> Vec<Element<'_, Self::Message>> {
        view::header_center(self)
    }

    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        view::header_end(self)
    }

    fn view(&self) -> Element<'_, Self::Message> {
        view::set_vim_mode(self.vim_enabled.then_some(self.vim.mode));
        view::view(self)
    }

    fn view_window(&self, id: cosmic::iced::window::Id) -> Element<'_, Self::Message> {
        if self.popout == Some(id) {
            return view::proof_window(self);
        }
        if self.references.iter().any(|(r, _)| *r == id) {
            return view::reference_window(self, id);
        }
        widget::text("").into()
    }

    fn on_close_requested(&self, id: cosmic::iced::window::Id) -> Option<Self::Message> {
        if self.references.iter().any(|(r, _)| *r == id) {
            return Some(Message::RefClosed(id));
        }
        (self.popout == Some(id)).then_some(Message::PopOutClosed(id))
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let links = self.links.iter().map(|l| {
            Subscription::run_with(l.target.clone(), link::run).map(|(key, event)| Message::Link(key, event))
        });
        let keys = cosmic::iced::event::listen_with(|event, status, _window| {
            use cosmic::iced::keyboard::{Event as Key, Key as K};
            if let cosmic::iced::Event::Keyboard(Key::ModifiersChanged(m)) = event {
                return Some(Message::Modifiers(m.control(), m.alt()));
            }
            if status == cosmic::iced::event::Status::Captured
                && !matches!(event, cosmic::iced::Event::Window(_))
            {
                return None;
            }
            if let cosmic::iced::Event::Window(cosmic::iced::window::Event::Resized(size)) = event {
                return Some(Message::WindowWidth(size.width));
            }
            let cosmic::iced::Event::Keyboard(Key::KeyPressed { key, modifiers, .. }) = event else { return None };
            match key.as_ref() {
                K::Character("=" | "+") if modifiers.command() => Some(Message::Zoom(1)),
                K::Character("-") if modifiers.command() => Some(Message::Zoom(-1)),
                K::Character("0") if modifiers.command() => Some(Message::Zoom(0)),
                K::Named(cosmic::iced::keyboard::key::Named::ArrowLeft) if modifiers.alt() => Some(Message::Back),
                K::Character(c) if !modifiers.alt() && !modifiers.logo() => Some(Message::WindowKey(c.to_string(), modifiers.control())),
                K::Named(cosmic::iced::keyboard::key::Named::Enter) => Some(Message::WindowKey("Enter".into(), false)),
                K::Named(cosmic::iced::keyboard::key::Named::ArrowDown) => Some(Message::WindowKey("Down".into(), false)),
                K::Named(cosmic::iced::keyboard::key::Named::ArrowUp) => Some(Message::WindowKey("Up".into(), false)),
                _ => None,
            }
        });
        // While a sidebar's edge is dragged: the pointer, wherever it is, and its release,
        // whichever widget it lands on
        let drag = self.dragging.is_some().then(|| {
            cosmic::iced::event::listen_with(|event, _status, _window| match event {
                cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::CursorMoved { position }) => Some(Message::DragMove(position)),
                cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::ButtonReleased(_)) => Some(Message::DragEnd),
                cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::CursorLeft) => Some(Message::DragEnd),
                _ => None,
            })
        });
        Subscription::batch(links.chain(std::iter::once(keys)).chain(drag))
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Noop => {}
            Message::Link(key, event) => return self.on_link(key, event),
            Message::Open(request) => {
                self.request = Some(request);
                return self.handle_request();
            }
            Message::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                self.save_settings();
            }
            Message::OpenSession(machine, sid) => {
                if self.workspace.as_ref().is_some_and(|w| w.machine == machine && w.sid == sid) {
                    return Task::none();
                }
                if let Some((m, old)) = self.workspace_ids() {
                    self.send(&m, "detach", json!({"session": old}), Pending::Plain);
                }
                let summary = self.sessions.get(&(machine.clone(), sid.clone()));
                let profile = prover::profile(summary.and_then(|s| s["backend"].as_str()));
                let caps = Capabilities::from_summary(summary, profile.default_capabilities());
                self.workspace = Some(Workspace {
                    machine: machine.clone(),
                    sid: sid.clone(),
                    profile,
                    caps,
                    info: None,
                    theory: None,
                    opening: None,
                    panel: if profile.has_symbol_table() { Panel::Symbols } else { Panel::Agents },
                    output: None,
                    tool_query: QueryState { operation: "tool", ..Default::default() },
                    goals: QueryState { operation: "deductum_goals", ..Default::default() },
                    goals_prev: QueryState { operation: "deductum_goals", ..Default::default() },
                    state_text: false,
                    show_types: false,
                    all_goals: false,
                    theory_filter: String::new(),
                    notes: Vec::new(),
                    note_reply: HashMap::new(),
                    show_offer: None,
                    pending_goto: None,
                    hover: None,
                    settings_open: false,
                    tool_started: None,
                    gap_hover: None,
                    tools_stopping: false,
                    palette_query: String::new(),
                    find: QueryState { operation: "find_theorems", ..Default::default() },
                    find_input: String::new(),
                    cex: QueryState { operation: "deductum_command", ..Default::default() },
                    fill: FillState::default(),
                    cex_params: String::new(),
                    activity: None,
                    views: HashMap::new(),
                    tool_settings: HashMap::new(),
                    symbol_search: String::new(),
                    history: Vec::new(),
                    follow: None,
                    log: Vec::new(),
                    merge: None,
                });
                self.send(&machine, "session_info", json!({"session": sid}), Pending::SessionInfo(sid));
                return self.retitle();
            }
            Message::CloseWorkspace => {
                if let Some((m, sid)) = self.workspace_ids() {
                    self.send(&m, "detach", json!({"session": sid}), Pending::Plain);
                }
                self.workspace = None;
                return self.retitle();
            }
            Message::OpenTheory(path) => {
                let Some((machine, sid)) = self.workspace_ids() else { return Task::none() };
                if let Some(ws) = &mut self.workspace {
                    if let Some(old) = &ws.theory {
                        let old = old.path.clone();
                        self.send(&machine, "close", json!({"session": sid, "path": old}), Pending::Plain);
                    }
                }
                if let Some(ws) = &mut self.workspace {
                    ws.opening = Some(path.clone());
                }
                self.send(&machine, "read", json!({"session": sid, "path": path}), Pending::Read(sid, path));
            }
            Message::SetMode(mode) => {
                self.mode_model.activate(self.mode_entities[if mode == Mode::Source { 1 } else { 0 }]);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.mode = mode;
                    t.block_editor = None;
                }
            }
            Message::SourceAction(action) if (self.ctrl || self.alt) && matches!(action, cosmic::widget::text_editor::Action::Click(_)) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.source_action(action, &*table);
                }
                return self.goto_definition();
            }
            Message::BlockAction(action) if (self.ctrl || self.alt) && matches!(action, cosmic::widget::text_editor::Action::Click(_)) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.block_action(action, &*table);
                }
                return self.goto_definition();
            }
            Message::SourceAction(action) => {
                self.vim_mouse(&action);
                let clicked = matches!(action, cosmic::widget::text_editor::Action::Click(_));
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    let edited = t.source_action(action, &*table);
                    if clicked {
                        self.vim_caret(true);
                    }
                    let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
                    // (the side-by-side source view scrolls itself, keeping the caret in sight)
                    let keep = view::keep_source_caret(t.source.cursor().position.line);
                    t.update_completions(&*table, edited);
                    let state = self.schedule_state();
                    if edited {
                        // A line reading `claude …` or `codex …`: asked when the typing pauses
                        self.keyword_gen += 1;
                        let generation = self.keyword_gen;
                        let pause = Task::perform(
                            async move {
                                tokio::time::sleep(KEYWORD_PAUSE).await;
                                generation
                            },
                            |g| cosmic::Action::App(Message::KeywordTick(g)),
                        );
                        return Task::batch([self.flush_edits(), state, pause, keep]);
                    }
                    return Task::batch([state, keep]);
                }
            }
            Message::BlockAction(cosmic::widget::text_editor::Action::Edit(cosmic::widget::text_editor::Edit::Backspace)) if self.open_block_empty() => {
                return self.delete_open_block();
            }
            Message::BlockAction(action) => {
                self.vim_mouse(&action);
                let clicked = matches!(action, cosmic::widget::text_editor::Action::Click(_));
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    let edited = t.block_action(action, &*table);
                    t.update_completions(&*table, edited);
                    // (vim's block cursor where the click put the caret, in normal mode)
                    if clicked {
                        self.vim_caret(true);
                    }
                    let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
                    // (the proof window scrolls itself, keeping the caret in sight)
                    let keep = match (&self.popout, &t.block_editor) {
                        (Some(_), Some(be)) => view::keep_popout_caret(be.content.cursor().position.line),
                        _ => Task::none(),
                    };
                    let state = Task::batch([self.schedule_state(), keep]);
                    if edited {
                        // A line reading `claude …` or `codex …`: asked when the typing pauses
                        self.keyword_gen += 1;
                        let generation = self.keyword_gen;
                        let pause = Task::perform(
                            async move {
                                tokio::time::sleep(KEYWORD_PAUSE).await;
                                generation
                            },
                            |g| cosmic::Action::App(Message::KeywordTick(g)),
                        );
                        return Task::batch([self.flush_edits(), state, pause]);
                    }
                    return state;
                }
            }
            Message::EditBlock(index) => {
                // Leaving an empty block for another: it goes first, and the other is found again
                if self.open_block_empty()
                    && let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref())
                    && let Some(target) = t.blocks.get(index).map(|b| b.start)
                    && t.block_editor.as_ref().is_some_and(|be| !(t.blocks[index].start <= be.start && be.start <= t.blocks[index].end))
                {
                    let before = t.local.chars().count();
                    let open_start = t.block_editor.as_ref().map_or(0, |be| be.start);
                    let removed = self.remove_open_block(false);
                    let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { return removed };
                    let gone = before.saturating_sub(t.local.chars().count());
                    let start = if target > open_start { target.saturating_sub(gone) } else { target };
                    let Some(i) = t.blocks.iter().position(|b| b.start == start) else { return removed };
                    return Task::batch([removed, self.update(Message::EditBlock(i))]);
                }
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                // Another block while a proof is in its own window: the window goes, its proof
                // with it (the window shows the block being edited)
                let close_popout = self.popout.take().map(cosmic::iced::window::close::<cosmic::Action<Message>>);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.break_step();
                    t.open_block(index, &*table);
                    t.selected = Some(index);
                    // Prose opens in its own typography, laid out as it was shown: the click,
                    // replayed once the editor is laid out, lands where it was made
                    let replay = match (self.prose_pointer, &t.block_editor) {
                        (Some((i, p)), Some(be)) if i == index && be.wrap.is_some() => Some(p),
                        _ => None,
                    };
                    if replay.is_none() {
                        t.narrow_to_paragraph(&*table);
                    }
                    let mut tasks = vec![widget::text_input::focus(view::block_editor_id()), self.schedule_state()];
                    tasks.extend(close_popout);
                    if let Some(p) = replay {
                        tasks.push(Task::perform(
                            async move {
                                tokio::time::sleep(Duration::from_millis(40)).await;
                                p
                            },
                            |p| cosmic::Action::App(Message::BlockClickAt(cosmic::iced::Point::new(p.x, (p.y - 2.0).max(0.0)))),
                        ));
                    }
                    // (vim's block cursor in the block just opened, in normal mode)
                    self.vim_caret(true);
                    return Task::batch(tasks);
                }
            }
            Message::CloseBlock => {
                // (a proof open in its own window stays open there)
                if self.popout.is_some() {
                    return Task::none();
                }
                // (an empty block, left, goes)
                let removed = if self.open_block_empty() { self.remove_open_block(false) } else { Task::none() };
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.break_step();
                    t.block_editor = None;
                }
                return Task::batch([removed, Self::unfocus_all()]);
            }
            Message::Lifecycle(op) => {
                if let Some((machine, sid)) = self.workspace_ids() {
                    self.send(&machine, op, json!({"session": sid}), Pending::Plain);
                }
            }
            Message::Undo | Message::Redo => {
                let op = if matches!(message, Message::Undo) { "undo" } else { "redo" };
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.break_step();
                    t.queued.push_back(op);
                }
                return self.flush_edits();
            }
            Message::Save => {
                let op = "save";
                if let Some((machine, sid)) = self.workspace_ids()
                    && let Some(path) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).map(|t| t.path.clone())
                {
                    self.send(&machine, op, json!({"session": sid, "path": path}), Pending::Plain);
                }
            }
            Message::Panel(panel) => {
                if !self.show_right {
                    self.show_right = true;
                    self.save_settings();
                }
                if let Some(ws) = &mut self.workspace {
                    ws.panel = panel;
                    if panel == Panel::History
                        && let Some(path) = ws.theory.as_ref().map(|t| t.path.clone())
                    {
                        let (m, sid) = (ws.machine.clone(), ws.sid.clone());
                        self.send(&m, "history", json!({"session": sid, "path": path, "limit": 60}), Pending::History(sid));
                    }
                    if panel == Panel::Agents
                        && let Some((m, sid)) = self.workspace_ids()
                    {
                        self.send(&m, "session_info", json!({"session": sid}), Pending::SessionInfo(sid));
                    }
                }
            }
            Message::StateTick(generation) => {
                if generation != self.state_gen {
                    return Task::none();
                }
                if let Some(ws) = &mut self.workspace
                    && let Some(t) = &ws.theory
                {
                    let sig = t.caret_signature();
                    if sig.2 != theory::Status::Running {
                        ws.tool_started = None;
                    } else if ws.tool_started.is_none() {
                        ws.tool_started = Some(std::time::Instant::now());
                    }
                }
                let Some(ws) = &self.workspace else { return Task::none() };
                let Some(t) = &ws.theory else { return Task::none() };
                let offset = Self::host_offset(t, t.cursor.saturating_sub(1).max(0));
                let (machine, sid, path, rev) = (ws.machine.clone(), ws.sid.clone(), t.path.clone(), t.confirmed_rev);
                // The state in parts, printed as wide as the panel, and the state before the
                // caret's command, to say what the command changed
                let margin = ((self.right_width - 120.0) / (view::z(13.0) * 0.6)).clamp(40.0, 160.0) as usize;
                let types = if ws.show_types { "true" } else { "false" };
                let prev = t.prover.commands.iter().position(|c| c.start <= offset && offset < c.end).and_then(|i| i.checked_sub(1)).map(|i| t.prover.commands[i].end.saturating_sub(1));
                let running: Vec<(String, &'static str)> = [(&ws.goals, "deductum_goals"), (&ws.goals_prev, "goals_prev")]
                    .into_iter()
                    .filter(|(q, _)| matches!(q.status.as_str(), "waiting" | "running"))
                    .filter_map(|(q, tag)| Some((q.id.clone()?, tag)))
                    .collect();
                self.send(&machine, "state", json!({"session": sid, "path": path, "offset": offset, "revision": rev, "margin": 90}), Pending::State(generation));
                // Where the human is, for agents asked about "this"
                let selection = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).and_then(|t| match &t.block_editor {
                    Some(be) if t.mode == Mode::Literate => be.content.selection(),
                    _ => t.source.selection(),
                });
                let selection = selection.map(|s| self.table(&machine).encode(&s)).unwrap_or_default();
                let mode = if self.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| t.mode == Mode::Source) { "source" } else { "literate" };
                // and the construct the goal is for (as the editor tints it), for "this step"
                let construct = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).and_then(|t| {
                    let b = t.blocks.iter().find(|b| b.start <= t.cursor && t.cursor <= b.end)?;
                    let (cs, ce) = t.construct_at(b.start, b.end)?;
                    Some(json!([Self::host_offset(t, cs), Self::host_offset(t, ce)]))
                });
                self.send(&machine, "view", json!({"session": sid, "path": path, "offset": offset, "selection": selection, "mode": mode, "construct": construct}), Pending::Plain);
                for (id, _) in &running {
                    self.send(&machine, "cancel_query", json!({"session": sid, "query": id}), Pending::Plain);
                }
                for tag in ["deductum_goals", "goals_prev"] {
                    let at = if tag == "goals_prev" { prev } else { Some(offset) };
                    if let Some(at) = at {
                        self.send(&machine, "query", json!({"session": sid, "path": path, "offset": at, "revision": rev, "operation": "deductum_goals", "args": [margin.to_string(), types]}), Pending::Query(tag));
                    }
                }
                if prev.is_none()
                    && let Some(ws) = &mut self.workspace
                {
                    ws.goals_prev = QueryState { operation: "deductum_goals", ..Default::default() };
                }
            }
            Message::ToolSetting(tool, key, value) => {
                if let Some(ws) = &mut self.workspace {
                    // (a number's field takes digits only)
                    let number = ws.caps.tool(&tool).and_then(|t| t.settings.iter().find(|s| s.key == key)).is_some_and(|s| s.kind == "number");
                    let value = if number { value.chars().filter(char::is_ascii_digit).collect() } else { value };
                    ws.tool_settings.insert(Tool::setting_key(&tool, &key), value);
                }
            }
            Message::RunTool(name) => {
                let cursor = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).map(|t| t.cursor.saturating_sub(1));
                if let Some(c) = cursor {
                    return self.update(Message::RunToolAt(name, c));
                }
            }
            Message::RunToolAt(name, offset) => {
                let Some(ws) = &mut self.workspace else { return Task::none() };
                let Some(t) = &ws.theory else { return Task::none() };
                let Some(tool) = ws.caps.tool(&name).cloned() else { return Task::none() };
                let host = Self::host_offset(t, offset);
                let (machine, sid, path, rev) = (ws.machine.clone(), ws.sid.clone(), t.path.clone(), t.confirmed_rev);
                // Its settings: the prover options they set, then the ones it takes as arguments
                let options = tool.options(&ws.tool_settings);
                let args: Vec<String> = tool.args.iter().map(|k| tool.value(&ws.tool_settings, k).trim().to_string()).collect();
                ws.panel = Panel::Tool;
                ws.tool_query = QueryState { operation: "tool", status: "starting".into(), ..Default::default() };
                if !options.is_empty() {
                    self.send(&machine, "prover_options", json!({"session": sid, "set": options}), Pending::Plain);
                }
                self.send(
                    &machine,
                    "query",
                    json!({"session": sid, "path": path, "offset": host, "revision": rev, "operation": tool.name, "args": args}),
                    Pending::Query("tool"),
                );
            }
            Message::CancelQuery => {
                if let Some(ws) = &self.workspace {
                    let q = match ws.panel {
                        Panel::Search => &ws.find,
                        Panel::Counterexamples => &ws.cex,
                        _ => &ws.tool_query,
                    };
                    if let Some(id) = q.id.clone() {
                        let (m, sid) = (ws.machine.clone(), ws.sid.clone());
                        self.send(&m, "cancel_query", json!({"session": sid, "query": id}), Pending::Plain);
                    }
                }
            }
            Message::InsertProof(proof) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some(ws) = &mut self.workspace else { return Task::none() };
                let q = ws.tool_query.clone();
                let Some(t) = &mut ws.theory else { return Task::none() };
                if q.revision == t.confirmed_rev || q.replace.is_some() || q.end.is_some() {
                    if let Some((s, e)) = q.replace {
                        let (s, e) = (Self::local_offset(t, s), Self::local_offset(t, e));
                        t.replace(s, e, &proof, &*table);
                    } else if let Some(end) = q.end {
                        let end = Self::local_offset(t, end);
                        t.insert(end, &format!("\n  {proof}"), &*table);
                    }
                    return self.flush_edits();
                }
            }
            Message::FindInput(s) => {
                if let Some(ws) = &mut self.workspace {
                    ws.find_input = s;
                }
            }
            Message::Find => {
                let Some(ws) = &mut self.workspace else { return Task::none() };
                let Some(t) = &ws.theory else { return Task::none() };
                let host = Self::host_offset(t, t.cursor.saturating_sub(1));
                let Some(search) = ws.caps.search.clone() else { return Task::none() };
                let (machine, sid, path, rev, query) = (ws.machine.clone(), ws.sid.clone(), t.path.clone(), t.confirmed_rev, ws.find_input.clone());
                ws.find = QueryState { operation: "find_theorems", status: "starting".into(), ..Default::default() };
                let args: Vec<String> = search.args.iter().cloned().chain([query]).collect();
                self.send(
                    &machine,
                    "query",
                    json!({"session": sid, "path": path, "offset": host, "revision": rev, "operation": search.operation, "args": args}),
                    Pending::Query("find_theorems"),
                );
            }
            Message::Grant(agent, label, scope) => {
                let Some(ws) = &self.workspace else { return Task::none() };
                let (machine, sid) = (ws.machine.clone(), ws.sid.clone());
                let mut params = json!({"session": sid, "agent": agent, "label": label, "scope": scope});
                if let Some(t) = &ws.theory {
                    params["path"] = json!(t.path);
                    params["offset"] = json!(Self::host_offset(t, t.cursor.saturating_sub(1)));
                }
                self.send(&machine, "grant", params, Pending::Plain);
                self.send(&machine, "session_info", json!({"session": sid}), Pending::SessionInfo(sid.clone()));
            }
            Message::AnswerRequest(request, allow) => {
                if let Some((machine, sid)) = self.workspace_ids() {
                    self.send(&machine, "answer_request", json!({"session": sid, "request": request, "allow": allow}), Pending::Plain);
                    self.send(&machine, "session_info", json!({"session": sid}), Pending::SessionInfo(sid.clone()));
                }
            }
            Message::Revoke(grant) => {
                if let Some((machine, sid)) = self.workspace_ids() {
                    self.send(&machine, "revoke", json!({"session": sid, "grant": grant}), Pending::Plain);
                    self.send(&machine, "session_info", json!({"session": sid}), Pending::SessionInfo(sid.clone()));
                }
            }
            Message::Revert(entry) => {
                if let Some((machine, sid)) = self.workspace_ids()
                    && let Some(path) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).map(|t| t.path.clone())
                {
                    self.send(&machine, "revert", json!({"session": sid, "path": path, "entry": entry}), Pending::Plain);
                }
            }
            Message::Follow(key) => {
                if let Some(ws) = &mut self.workspace {
                    ws.follow = key;
                }
            }
            Message::Resolve(choice) => {
                if let Some((machine, sid)) = self.workspace_ids()
                    && let Some(path) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()).map(|t| t.path.clone())
                {
                    self.send(&machine, "resolve", json!({"session": sid, "path": path, "choice": choice}), Pending::Plain);
                }
            }
            Message::RestoreAlternative(id, piece) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut())
                    && let Some(alt) = t.alternatives.iter().find(|a| a["id"].as_str() == Some(id.as_str())).cloned()
                    && let Some(p) = alt["pieces"].get(piece)
                {
                    // Put back what the change would have: replace the span it collided with
                    let len = ot::char_len(&t.local);
                    let at = Self::local_offset(t, p["at"].as_u64().unwrap_or(0) as usize).min(len);
                    let end = p["end"].as_u64().map_or(at, |e| Self::local_offset(t, e as usize)).clamp(at, len);
                    let text = p["restore"].as_str().unwrap_or("").to_string();
                    t.replace(at, end, &text, &*table);
                    t.alternatives.retain(|a| a["id"].as_str() != Some(id.as_str()));
                    return self.flush_edits();
                }
            }
            Message::DismissAlternative(id) => {
                if let Some((machine, sid)) = self.workspace_ids()
                    && let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut())
                {
                    t.alternatives.retain(|a| a["id"].as_str() != Some(id.as_str()));
                    let path = t.path.clone();
                    self.send(&machine, "dismiss_alternative", json!({"session": sid, "path": path, "id": id}), Pending::Plain);
                }
            }
            Message::NewSession(open) => {
                if open {
                    let machine = self.workspace.as_ref().and_then(|w| self.links.iter().position(|l| l.target.key == w.machine)).unwrap_or(0);
                    self.new_session = Some(NewSession { machine, ..Default::default() });
                    let key = self.links.get(machine).map(|l| l.target.key.clone()).unwrap_or_default();
                    self.send(&key, "list_dir", json!({}), Pending::ListDir);
                } else {
                    self.new_session = None;
                }
            }
            Message::NewMachine(i) => {
                if let Some(ns) = &mut self.new_session {
                    ns.machine = i;
                    ns.listing = None;
                    let key = self.links.get(i).map(|l| l.target.key.clone()).unwrap_or_default();
                    self.send(&key, "list_dir", json!({}), Pending::ListDir);
                }
            }
            Message::NewFolder(folder) => {
                if let Some(ns) = &mut self.new_session {
                    ns.folder = folder;
                }
            }
            Message::NewBrowse(path) => {
                if let Some(ns) = &self.new_session {
                    let key = self.links.get(ns.machine).map(|l| l.target.key.clone()).unwrap_or_default();
                    self.send(&key, "list_dir", json!({"path": path}), Pending::ListDir);
                }
            }
            Message::NewLogic(logic) => {
                if let Some(ns) = &mut self.new_session {
                    ns.logic = logic;
                }
            }
            Message::NewCreate => {
                if let Some(ns) = &self.new_session {
                    let key = self.links.get(ns.machine).map(|l| l.target.key.clone()).unwrap_or_default();
                    let mut params = json!({"folder": ns.folder, "start": true, "resume": true});
                    if !ns.logic.trim().is_empty() {
                        params["logic"] = json!(ns.logic.trim());
                        if let Some(s) = &ns.suggestion
                            && s["logic"].as_str() == Some(ns.logic.trim())
                        {
                            params["dirs"] = s["dirs"].clone();
                            params["include_sessions"] = s["include_sessions"].clone();
                        }
                    }
                    self.send(&key, "session_create", params, Pending::Create);
                }
            }
            Message::PopOut => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
                // (in the source view, the proof the caret is in, whatever block was open before)
                if t.mode == Mode::Source && self.popout.is_none() {
                    t.block_editor = None;
                }
                if t.block_editor.is_none() {
                    // the block at the caret
                    let at = t.cursor.saturating_sub(1);
                    if let Some(i) = t.blocks.iter().position(|b| b.start <= at && at <= b.end) {
                        t.open_block(i, &*table);
                    }
                }
                // Proofs only: prose is edited on the page
                if t.block_editor.as_ref().is_none_or(|be| be.wrap.is_some() || be.kind != theory::Kind::Formal) || self.popout.is_some() {
                    return Task::none();
                }
                let title = format!("Proof — {}", file_name(&t.path));
                let (id, open) = cosmic::iced::window::open(cosmic::iced::window::Settings {
                    size: Size::new(1000.0, 760.0),
                    min_size: Some(Size::new(560.0, 360.0)),
                    decorations: true,
                    ..Default::default()
                });
                self.popout = Some(id);
                return Task::batch([open.map(|_| cosmic::Action::None), self.set_window_title(title, id)]);
            }
            Message::LeftMode(mode) => self.left = mode,
            Message::DragStart(left) => self.dragging = Some(left),
            Message::DragEnd => {
                self.dragging = None;
                self.save_settings();
            }
            Message::WindowWidth(w) => self.window_width = w,
            Message::DragMove(p) => match self.dragging {
                Some(true) => self.left_width = p.x.clamp(180.0, (self.window_width - 360.0).max(180.0)),
                Some(false) => {
                    self.right_width = (self.window_width - p.x).clamp(260.0, (self.window_width - 360.0).max(260.0));
                }
                None => {}
            },
            Message::Zoom(step) => {
                let z = match step {
                    0 => 1.0,
                    s if s > 0 => view::zoom() * 1.1,
                    _ => view::zoom() / 1.1,
                };
                view::set_zoom(z);
                self.save_settings();
            }
            Message::Wrap(before, after) => {
                use cosmic::widget::text_editor::{Action as A, Edit as E, Motion as M};
                let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { return Task::none() };
                let in_block = t.block_editor.is_some() && t.mode == Mode::Literate;
                let (content, focus) = match &t.block_editor {
                    Some(be) if in_block => (&be.content, view::block_editor_id()),
                    _ => (&t.source, view::source_editor_id()),
                };
                let act = |a: A| if in_block { Message::BlockAction(a) } else { Message::SourceAction(a) };
                let paste = |s: &str| A::Edit(E::Paste(Arc::new(s.to_string())));
                let text = content.text();
                let li = editor::LineIndex::new(&text);
                let range = editor::selection_range(content);
                let mut tasks = Vec::new();
                // (one click, one entry of the history: what goes before and after together)
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.begin_group();
                }
                match range {
                    // Around the selection: what goes after first, then what goes before, each
                    // an insertion of its own (the selected text is left as it is)
                    Some((s, e)) => {
                        let place = |app: &mut Self, at: usize| {
                            if let Some(t) = app.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                                let content = match &mut t.block_editor {
                                    Some(be) if in_block => &mut be.content,
                                    _ => &mut t.source,
                                };
                                let text = content.text();
                                let li = editor::LineIndex::new(&text);
                                editor::place(content, li.position(&text, at));
                            }
                        };
                        place(self, e);
                        if !after.is_empty() {
                            tasks.push(self.update(act(paste(&after))));
                        }
                        place(self, s);
                        if !before.is_empty() {
                            tasks.push(self.update(act(paste(&before))));
                        }
                        let end = e + before.chars().count() + after.chars().count();
                        place(self, end);
                    }
                    // At the caret: both, the caret between them
                    None => {
                        let _ = li;
                        tasks.push(self.update(act(paste(&format!("{before}{after}")))));
                        for _ in 0..after.chars().count() {
                            tasks.push(self.update(act(A::Move(M::Left))));
                        }
                    }
                }
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.end_group();
                }
                tasks.push(widget::text_input::focus(focus));
                return Task::batch(tasks);
            }
            Message::PaletteQuery(q) => {
                if let Some(ws) = &mut self.workspace {
                    ws.palette_query = q;
                }
            }
            Message::ToggleGoalBeside => {
                view::set_goal_beside(!view::goal_beside_on());
                self.save_settings();
            }
            Message::ToggleRight => {
                self.show_right = !self.show_right;
                self.save_settings();
            }
            Message::Goto(offset) => {
                let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
                t.cursor = offset;
                let state = self.schedule_state();
                let t = self.workspace.as_mut().and_then(|w| w.theory.as_mut()).unwrap();
                match t.mode {
                    Mode::Source => {
                        let dec = t.decoded.to_decoded(offset);
                        let pos = t.lines.position(&t.decoded.text, dec);
                        editor::place(&mut t.source, pos);
                        return Task::batch([widget::text_input::focus(view::source_editor_id()), state]);
                    }
                    Mode::Literate => {
                        // Into the open block, if it is there; else the block it is in, centred
                        if let Some(be) = &mut t.block_editor
                            && be.start <= offset
                            && offset <= be.end
                        {
                            let shown = be.shown();
                            let li = editor::LineIndex::new(&shown);
                            let pos = li.position(&shown, be.decoded.to_decoded(offset - be.start));
                            editor::place(&mut be.content, pos);
                            return Task::batch([view::centre_selected(), state]);
                        }
                        if let Some(i) = t.blocks.iter().position(|b| b.start <= offset && offset < b.end.max(b.start + 1)) {
                            t.selected = Some(i);
                            return Task::batch([view::centre_selected(), state]);
                        }
                        let total = ot::char_len(&t.local).max(1) as f32;
                        let y = (offset as f32 / total).clamp(0.0, 1.0);
                        return Task::batch([
                            cosmic::iced::widget::scrollable::snap_to(
                                view::literate_scroll_id(),
                                cosmic::iced::widget::scrollable::RelativeOffset { x: None, y: Some(y) },
                            ),
                            state,
                        ]);
                    }
                }
            }
            Message::FillAgent(i) => {
                if let Some(ws) = &mut self.workspace {
                    ws.fill.agent = i;
                }
            }
            Message::FillHint(h) => {
                if let Some(ws) = &mut self.workspace {
                    ws.fill.hint = h;
                }
            }
            Message::Fill => {
                let Some(ws) = &mut self.workspace else { return Task::none() };
                let Some(t) = &ws.theory else { return Task::none() };
                let host = Self::host_offset(t, t.cursor.saturating_sub(1));
                let (machine, sid, path, rev) = (ws.machine.clone(), ws.sid.clone(), t.path.clone(), t.confirmed_rev);
                let agent = if ws.fill.agent == 1 { "codex" } else { "claude" };
                let hint = ws.fill.hint.clone();
                ws.panel = Panel::Tool;
                ws.fill = FillState { agent: ws.fill.agent, hint: hint.clone(), status: "starting".into(), ..Default::default() };
                self.send(&machine, "fill_hole", json!({"session": sid, "path": path, "offset": host, "revision": rev, "agent": agent, "hint": hint}), Pending::Fill);
            }
            Message::ShowOffer(go) => {
                let Some(offer) = self.workspace.as_mut().and_then(|w| w.show_offer.take()) else { return Task::none() };
                if go {
                    return self.go_to_place(&offer);
                }
            }
            Message::NoteReplyInput(id, text) => {
                if let Some(ws) = &mut self.workspace {
                    ws.note_reply.insert(id, text);
                }
            }
            Message::NoteReply(id, resolve) => {
                if let Some(ws) = &mut self.workspace {
                    let text = ws.note_reply.remove(&id).unwrap_or_default();
                    let (machine, sid) = (ws.machine.clone(), ws.sid.clone());
                    self.send(&machine, "reply_note", json!({"session": sid, "note": id, "text": text, "resolve": resolve}), Pending::Plain);
                }
            }
            Message::ToolSettings(open) => {
                if let Some(ws) = &mut self.workspace {
                    ws.settings_open = open;
                }
            }
            Message::ToolApply(name) => {
                // For the tool typed in the text: the prover's own defaults
                let Some(ws) = &self.workspace else { return Task::none() };
                let Some(tool) = ws.caps.tool(&name) else { return Task::none() };
                let options = tool.options(&ws.tool_settings);
                if options.is_empty() {
                    return Task::none();
                }
                let label = tool.label.clone();
                let (machine, sid) = (ws.machine.clone(), ws.sid.clone());
                self.send(&machine, "prover_options", json!({"session": sid, "set": options}), Pending::Plain);
                self.status = Some(format!("{label} settings applied: they hold for {name} typed in the text"));
            }
            Message::Modifiers(ctrl, alt) => {
                self.ctrl = ctrl;
                self.alt = alt;
            }
            Message::ViewPress(i) => {
                let at = self.workspace.as_ref().and_then(|w| w.hover.as_ref()).filter(|h| h.target == HoverTarget::View(i)).map(|h| h.offset);
                let open = self.update(Message::EditBlock(i));
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut())
                    && let Some(at) = at
                {
                    if let Some(be) = &mut t.block_editor
                        && be.start <= at
                        && at <= be.end
                    {
                        let shown = be.shown();
                        let pos = editor::LineIndex::new(&shown).position(&shown, be.decoded.to_decoded(at - be.start));
                        editor::place(&mut be.content, pos);
                    }
                    t.sync_block_cursor();
                }
                // (placing the caret at the click clears vim's block cursor: again, there)
                self.vim_caret(true);
                if self.ctrl || self.alt {
                    return Task::batch([open, self.goto_definition()]);
                }
                return Task::batch([open, self.schedule_state()]);
            }
            Message::StopTools => {
                if let Some(ws) = &mut self.workspace {
                    ws.tools_stopping = true;
                }
                return self.update(Message::StopToolsAgain(0));
            }
            Message::StopToolsAgain(round) => {
                let still = self.workspace.as_ref().is_some_and(|w| {
                    w.activity.as_ref().is_some_and(|a| a["running"].as_array().into_iter().flatten().any(|r| r["name"].as_str().is_some_and(|n| w.caps.typed_tool(n).is_some())))
                });
                if round > 0 && !still || round >= 6 {
                    if let Some(ws) = &mut self.workspace {
                        ws.tools_stopping = false;
                    }
                    return Task::none();
                }
                if let Some((machine, sid)) = self.workspace_ids() {
                    self.send(&machine, "cancel_tools", json!({"session": sid}), Pending::Plain);
                }
                return Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        round + 1
                    },
                    |r| cosmic::Action::App(Message::StopToolsAgain(r)),
                );
            }
            Message::KeywordTick(generation) => {
                if generation != self.keyword_gen {
                    return Task::none();
                }
                let Some(ws) = &self.workspace else { return Task::none() };
                let Some(t) = &ws.theory else { return Task::none() };
                if !t.settled() || keyword_line(t).is_none() {
                    return Task::none();
                }
                let at = Self::host_offset(t, t.cursor);
                let (machine, sid, path, rev) = (ws.machine.clone(), ws.sid.clone(), t.path.clone(), t.confirmed_rev);
                self.send(&machine, "keyword", json!({"session": sid, "path": path, "offset": at, "revision": rev}), Pending::Plain);
            }
            Message::GapHover(gap) => {
                if let Some(ws) = &mut self.workspace {
                    ws.gap_hover = gap;
                }
            }
            Message::GapLeave(i) => {
                // (leaving one gap may arrive after entering the next: only its own)
                if let Some(ws) = self.workspace.as_mut().filter(|w| w.gap_hover == Some(i)) {
                    ws.gap_hover = None;
                }
            }
            Message::Back => {
                let Some((path, at)) = self.back.pop() else { return Task::none() };
                return self.go_to_place(&json!({"path": path, "start": at}));
            }
            Message::QueryRoute(route) => {
                if let Some((machine, sid)) = self.workspace_ids() {
                    self.send(&machine, "query_route", json!({"session": sid, "route": route}), Pending::Plain);
                    if let Some(a) = self.workspace.as_mut().and_then(|w| w.activity.as_mut()) {
                        a["route"] = json!(route);
                    }
                }
            }
            Message::HelperReset => {
                if let Some((machine, sid)) = self.workspace_ids() {
                    let (agent, model) = self.workspace.as_ref().map(|w| (if w.fill.agent == 1 { "codex" } else { "claude" }, w.fill.model.clone())).unwrap_or(("claude", None));
                    self.send(&machine, "helper_reset", json!({"session": sid, "agent": agent, "model": model}), Pending::Plain);
                    if let Some(ws) = &mut self.workspace {
                        ws.fill.conversation = None;
                    }
                    self.status = Some(format!("The next {agent} query starts a new conversation"));
                }
            }
            Message::CopyText(text) => {
                self.status = Some("Copied".into());
                return cosmic::iced::clipboard::write(text);
            }
            Message::LineMove(down) => {
                use cosmic::widget::text_editor::{Action as A, Motion as M};
                return self.vim_ops(vec![vim::Op::Act(A::Move(if down { M::Down } else { M::Up }))]);
            }
            Message::RefOpenHere(id) => {
                let Some((_, r)) = self.references.iter().find(|(w, _)| *w == id) else { return Task::none() };
                let place = json!({"path": r.path, "start": r.start});
                if let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) {
                    self.back.push((t.path.clone(), Self::host_offset(t, t.cursor)));
                }
                return self.go_to_place(&place);
            }
            Message::RefBack(id) => {
                let Some(ws) = &self.workspace else { return Task::none() };
                let (machine, sid) = (ws.machine.clone(), ws.sid.clone());
                let Some(d) = self.references.iter_mut().find(|(w, _)| *w == id).and_then(|(_, r)| r.back.pop()) else { return Task::none() };
                self.send(&machine, "read", json!({"session": sid, "path": d["file"], "peek": true}), Pending::RefRead(d, Some(id)));
            }
            Message::RefAction(id, action) => {
                // Read-only: the caret moves and text is selected and copied, nothing is typed
                if action.is_edit() {
                    return Task::none();
                }
                let click = matches!(action, cosmic::widget::text_editor::Action::Click(_));
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some((_, r)) = self.references.iter_mut().find(|(w, _)| *w == id) else { return Task::none() };
                r.theory.source_action(action, &*table);
                if click && (self.ctrl || self.alt) {
                    let at = r.theory.cursor;
                    return self.ref_lookup(id, at, RefUse::Goto(self.alt));
                }
            }
            Message::RefScroll(id) => {
                let Some((_, r)) = self.references.iter().find(|(w, _)| *w == id) else { return Task::none() };
                return view::scroll_reference(id, r);
            }
            Message::RefClosed(id) => {
                self.references.retain(|(r, _)| *r != id);
                if self.entity.as_ref().is_some_and(|(_, w, _)| *w == id) {
                    self.entity = None;
                }
            }
            Message::HoverMove(target, point) => {
                let Some(offset) = self.hit(target, point) else { return Task::none() };
                let Some(ws) = &mut self.workspace else { return Task::none() };
                if let Some(h) = &mut ws.hover
                    && h.target == target
                    && h.offset == offset
                {
                    h.point = point;
                    return Task::none();
                }
                self.hover_gen += 1;
                let generation = self.hover_gen;
                if let HoverTarget::Message(key) = target {
                    // Output carries its own tips: the innermost under the pointer
                    let tip = ws.views.get(&key).and_then(|v| {
                        v.tips.iter().filter(|(s, e, _)| *s <= offset && offset < *e).min_by_key(|(s, e, _)| e - s).map(|(_, _, t)| t.clone())
                    });
                    ws.hover = tip.map(|t| Hover {
                        target,
                        point,
                        offset,
                        generation,
                        info: Some(json!({"tooltip": {"items": [{"raw": t, "text": t, "spans": []}]}})),
                    });
                    return Task::none();
                }
                ws.hover = Some(Hover { target, point, offset, generation, info: None });
                return Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(450)).await;
                        generation
                    },
                    |g| cosmic::Action::App(Message::HoverTick(g)),
                );
            }
            Message::InsertBlock(after, kind) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
                // After the block, else before the theory's `end`
                let at = match after.and_then(|i| t.blocks.get(i)) {
                    Some(b) => b.end,
                    None => t.blocks.iter().rev().find(|b| b.kind == theory::Kind::End).map_or(ot::char_len(&t.local), |b| b.start),
                };
                let chars: Vec<char> = t.local.chars().collect();
                let at = at.min(chars.len());
                let before = match (at.checked_sub(1).map(|i| chars[i]), at.checked_sub(2).map(|i| chars[i])) {
                    (Some('\n'), Some('\n')) | (None, _) => "",
                    (Some('\n'), _) => "\n",
                    _ => "\n\n",
                };
                let templates = t.profile.block_templates();
                let Some(template) = templates.iter().find(|b| b.kind == kind).or(templates.first()).copied() else { return Task::none() };
                let body = template.body;
                let after_nl = if chars.get(at) == Some(&'\n') { "\n" } else { "\n\n" };
                t.break_step();
                t.replace(at, at, &format!("{before}{body}{after_nl}"), &*table);
                t.break_step();
                let start = at + before.chars().count();
                let index = t.blocks.iter().position(|b| b.start <= start && start < b.end.max(b.start + 1));
                let mut tasks = vec![self.flush_edits()];
                if let Some(i) = index {
                    tasks.push(self.update(Message::EditBlock(i)));
                    // (where the template puts the caret: a lemma's between its quotes)
                    if let Some((line, column)) = template.caret
                        && let Some(be) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()).and_then(|t| t.block_editor.as_mut())
                    {
                        editor::place(&mut be.content, cosmic::widget::text_editor::Position { line, column });
                    }
                    tasks.push(view::centre_selected());
                }
                return Task::batch(tasks);
            }
            Message::DeleteBlock(i) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
                let Some(b) = t.blocks.get(i).cloned() else { return Task::none() };
                // The block and the blank line that set it off (undo brings it back)
                let chars: Vec<char> = t.local.chars().collect();
                let mut end = b.end.min(chars.len());
                while end < chars.len() && chars[end] == '\n' {
                    end += 1;
                }
                t.block_editor = None;
                t.break_step();
                t.replace(b.start, end, "", &*table);
                t.break_step();
                t.selected = Some(i.min(t.blocks.len().saturating_sub(1)));
                self.status = Some("Block deleted (Ctrl+Z brings it back)".into());
                return Task::batch([self.flush_edits(), view::centre_selected(), Self::unfocus_all()]);
            }
            Message::ProsePointer(i, p) => {
                self.prose_pointer = Some((i, p));
            }
            Message::BlockClickAt(p) => {
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut())
                    && let Some(be) = &mut t.block_editor
                {
                    be.content.perform(cosmic::widget::text_editor::Action::Click(p));
                    t.sync_block_cursor();
                    // (then only the paragraph clicked is source)
                    let machine = self.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default();
                    let table = self.table(&machine);
                    if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                        t.narrow_to_paragraph(&*table);
                    }
                    self.vim_caret(true);
                }
                return self.schedule_state();
            }
            Message::HoverExit => {
                if let Some(ws) = &mut self.workspace {
                    ws.hover = None;
                }
            }
            Message::HoverTick(generation) => {
                let Some(ws) = &self.workspace else { return Task::none() };
                let Some(h) = ws.hover.as_ref().filter(|h| h.generation == generation) else { return Task::none() };
                if let HoverTarget::Ref(id) = h.target {
                    let at = h.offset;
                    return self.ref_lookup(id, at, RefUse::Hover(generation));
                }
                let Some(t) = &ws.theory else { return Task::none() };
                let at = Self::host_offset(t, h.offset);
                let (machine, sid, path, rev) = (ws.machine.clone(), ws.sid.clone(), t.path.clone(), t.confirmed_rev);
                self.send(&machine, "info", json!({"session": sid, "path": path, "offset": at, "revision": rev}), Pending::Info(generation));
            }
            Message::TheoryFilter(f) => {
                if let Some(ws) = &mut self.workspace {
                    ws.theory_filter = f;
                }
            }
            Message::StateText(on) => {
                if let Some(ws) = &mut self.workspace {
                    ws.state_text = on;
                }
            }
            Message::ShowTypes(on) => {
                if let Some(ws) = &mut self.workspace {
                    ws.show_types = on;
                }
                return self.schedule_state();
            }
            Message::AllGoals(on) => {
                if let Some(ws) = &mut self.workspace {
                    ws.all_goals = on;
                }
            }
            Message::InsertUsing => {
                let Some(ws) = &self.workspace else { return Task::none() };
                let (Some((s0, _)), Some(t)) = (ws.fill.replace, &ws.theory) else { return Task::none() };
                // The facts as the profile writes them, the tool on its own line under them,
                // indented like the hole
                let chars: Vec<char> = t.local.chars().collect();
                let s = Self::local_offset(t, s0).min(chars.len());
                let mut line_start = s;
                while line_start > 0 && chars[line_start - 1] != '\n' {
                    line_start -= 1;
                }
                let before: String = chars[line_start..s].iter().collect();
                let indent = if before.trim().is_empty() { before } else { String::new() };
                let text = t.profile.facts_line(&ws.fill.using, &indent);
                return self.update(Message::InsertFill(text));
            }
            Message::FillDismiss => {
                if let Some(ws) = &mut self.workspace {
                    if let (Some(job), Some(machine)) = (ws.fill.job.clone(), Some(ws.machine.clone()))
                        && matches!(ws.fill.status.as_str(), "asking" | "checking" | "starting")
                    {
                        let sid = ws.sid.clone();
                        ws.fill = FillState { agent: ws.fill.agent, ..Default::default() };
                        self.send(&machine, "cancel_fill", json!({"session": sid, "job": job}), Pending::Plain);
                        return Task::none();
                    }
                    ws.fill = FillState { agent: ws.fill.agent, ..Default::default() };
                }
            }
            Message::InsertFill(text) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some(ws) = &mut self.workspace else { return Task::none() };
                let f = ws.fill.clone();
                let Some(t) = &mut ws.theory else { return Task::none() };
                let Some((s0, e0)) = f.replace else { return Task::none() };
                if f.revision != t.confirmed_rev {
                    // map from the revision the hole was found in
                    self.status = Some("The theory changed since; the proof replaces the hole where it now is".into());
                }
                let (s, e) = (Self::local_offset(t, s0), Self::local_offset(t, e0));
                t.replace(s, e, &text, &*table);
                ws.fill = FillState { agent: f.agent, hint: f.hint, ..Default::default() };
                return self.flush_edits();
            }
            Message::CommandParams(p) => {
                if let Some(ws) = &mut self.workspace {
                    ws.cex_params = p;
                }
            }
            Message::RunCommand(cmd) => {
                let Some(ws) = &mut self.workspace else { return Task::none() };
                let Some(t) = &ws.theory else { return Task::none() };
                let host = Self::host_offset(t, t.cursor.saturating_sub(1));
                let (machine, sid, path, rev) = (ws.machine.clone(), ws.sid.clone(), t.path.clone(), t.confirmed_rev);
                let params = ws.cex_params.trim().trim_start_matches('[').trim_end_matches(']').trim().to_string();
                let text = t.profile.tool_invocation(&cmd, &params);
                ws.panel = Panel::Counterexamples;
                ws.cex = QueryState { operation: "deductum_command", status: format!("running {text}"), ..Default::default() };
                self.send(
                    &machine,
                    "query",
                    json!({"session": sid, "path": path, "offset": host, "revision": rev, "operation": "deductum_command", "args": [text]}),
                    Pending::Query("deductum_command"),
                );
            }
            Message::BlockViewAction(i, action) => {
                use cosmic::widget::text_editor::Action as A;
                if !matches!(action, A::Click(_)) {
                    return Task::none();
                }
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
                let Some(Some(v)) = t.block_views.get_mut(i) else { return Task::none() };
                v.content.perform(action.clone());
                let cursor = v.content.cursor();
                t.open_block(i, &*table);
                t.selected = Some(i);
                if let Some(be) = &mut t.block_editor {
                    editor::place(&mut be.content, cursor.position);
                }
                t.sync_block_cursor();
                if self.ctrl || self.alt {
                    return Task::batch([self.goto_definition(), self.schedule_state()]);
                }
                return Task::batch([widget::text_input::focus(view::block_editor_id()), self.schedule_state()]);
            }
            Message::ViewAction(key, action) => {
                if !action.is_edit()
                    && let Some(ws) = &mut self.workspace
                    && let Some(v) = ws.views.get_mut(&key)
                {
                    v.content.perform(action);
                }
            }
            Message::SymbolSearch(q) => {
                if let Some(ws) = &mut self.workspace {
                    ws.symbol_search = q;
                }
            }
            Message::InsertSymbol(text) => {
                // Into the editor in use, at its caret, as an entry of the history of its own
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.break_step();
                }
                let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { return Task::none() };
                let paste = cosmic::widget::text_editor::Action::Edit(cosmic::widget::text_editor::Edit::Paste(Arc::new(text)));
                if t.block_editor.is_some() && t.mode == Mode::Literate {
                    return Task::batch([self.update(Message::BlockAction(paste)), widget::text_input::focus(view::block_editor_id())]);
                }
                if t.mode == Mode::Source {
                    return Task::batch([self.update(Message::SourceAction(paste)), widget::text_input::focus(view::source_editor_id())]);
                }
                self.status = Some("Click into a block or the source first, then pick the symbol".into());
            }
            Message::Vim(key, ctrl) => {
                // Each normal-mode command is an undo step; an insert session is one
                let vim_on = self.vim_enabled;
                let normal = self.vim.mode != vim::Mode::Insert || key == "Escape";
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.vim_session = vim_on;
                    if normal {
                        t.break_step();
                    }
                }
                // `dd` on a block's only line: the block goes
                if key == "d"
                    && self.vim.pending == "d"
                    && self.vim.mode == vim::Mode::Normal
                    && self.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| {
                        t.mode == Mode::Literate && t.block_editor.as_ref().is_some_and(|be| be.wrap.is_none() && be.content.line_count() == 1)
                    })
                {
                    self.vim.pending.clear();
                    self.vim_caret(false);
                    return self.delete_open_block();
                }
                self.vim_caret(false);
                if key == "Escape" && self.vim.mode == vim::Mode::Insert {
                    self.vim.mode = vim::Mode::Normal;
                    self.vim_caret(true);
                    return Task::none();
                }
                let ops = self.vim.key(&key, ctrl);
                let task = self.vim_ops(ops);
                self.vim_caret(true);
                return task;
            }
            Message::WindowKey(key, ctrl) => {
                // Block navigation in the literate view with no block open: arrows always,
                // the letters with vim keys on
                if !self.vim_enabled && !matches!(key.as_str(), "Down" | "Up" | "Enter") {
                    return Task::none();
                }
                let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) else { return Task::none() };
                if t.mode != Mode::Literate || t.block_editor.is_some() {
                    return Task::none();
                }
                let n = t.blocks.len();
                if n == 0 {
                    return Task::none();
                }
                let cur = t.selected.unwrap_or_else(|| t.blocks.iter().position(|b| b.end >= t.cursor).unwrap_or(0));
                let pending = std::mem::take(&mut self.vim.pending);
                let pending_g = pending == "g";
                let pending_d = pending == "d";
                let next = match (key.as_str(), ctrl) {
                    ("r", true) => return self.update(Message::Redo),
                    ("u", false) => return self.update(Message::Undo),
                    ("j" | "Down", false) => Some((cur + 1).min(n - 1)),
                    ("k" | "Up", false) => Some(cur.saturating_sub(1)),
                    ("G", false) => Some(n - 1),
                    ("g", false) if pending_g => Some(0),
                    ("g", false) => {
                        self.vim.pending = "g".into();
                        None
                    }
                    ("d", false) if pending_d => return self.update(Message::DeleteBlock(cur)),
                    ("d", false) => {
                        self.vim.pending = "d".into();
                        None
                    }
                    ("Enter" | "i" | "a" | "o", false) => return Task::batch([self.update(Message::EditBlock(cur)), view::centre_selected()]),
                    _ => None,
                };
                if let Some(i) = next {
                    t.selected = Some(i);
                    t.cursor = t.blocks[i].start;
                    return Task::batch([view::centre_selected(), self.schedule_state(), Self::unfocus_all()]);
                }
            }
            Message::CopySelection(cut) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { return Task::none() };
                let in_block = t.block_editor.is_some() && t.mode == Mode::Literate;
                let sel = match &t.block_editor {
                    Some(be) if in_block => be.content.selection(),
                    _ => t.source.selection(),
                };
                let Some(sel) = sel else { return Task::none() };
                let mut tasks = vec![cosmic::iced::clipboard::write(table.encode(&sel.replace('\u{200B}', "")))];
                if cut {
                    let del = cosmic::widget::text_editor::Action::Edit(cosmic::widget::text_editor::Edit::Delete);
                    tasks.push(self.update(if in_block { Message::BlockAction(del) } else { Message::SourceAction(del) }));
                }
                return Task::batch(tasks);
            }
            Message::OpenFolder(opener) => {
                let Some(ws) = &self.workspace else { return Task::none() };
                let Some(folder) = ws.info.as_ref().and_then(|i| i["folder"].as_str()).map(str::to_string) else {
                    return Task::none();
                };
                let Some(target) = self.link(&ws.machine).map(|l| l.target.clone()) else { return Task::none() };
                return cosmic::task::future(async move { Message::OpenFolderResult(link::open_folder(opener, target, folder).await) });
            }
            Message::OpenFolderResult(result) => {
                if let Err(err) = result {
                    self.status = Some(format!("Could not open the folder: {err}"));
                }
            }
            Message::ToggleVim => {
                self.vim_enabled = !self.vim_enabled;
                self.vim = vim::Vim::default();
                self.save_settings();
            }
            Message::SetModeEntity(entity) => {
                let mode = if entity == self.mode_entities[1] { Mode::Source } else { Mode::Literate };
                return self.update(Message::SetMode(mode));
            }
            Message::Unindent => {
                use cosmic::widget::text_editor::{Action as A, Edit as E, Motion as M};
                let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { return Task::none() };
                let in_block = t.block_editor.is_some() && t.mode == Mode::Literate;
                let line = {
                    let (text, pos) = match &t.block_editor {
                        Some(be) if in_block => (be.content.text(), be.content.cursor().position),
                        _ => (t.decoded.text.clone(), t.source.cursor().position),
                    };
                    text.split('\n').nth(pos.line).unwrap_or("").to_string()
                };
                let n = line.chars().take_while(|c| *c == ' ').count().min(2);
                if n == 0 {
                    return Task::none();
                }
                let act = |a: A| if in_block { Message::BlockAction(a) } else { Message::SourceAction(a) };
                let mut tasks = vec![self.update(act(A::Move(M::Home)))];
                for _ in 0..n {
                    tasks.push(self.update(act(A::Edit(E::Delete))));
                }
                return Task::batch(tasks);
            }
            Message::Newline => {
                let Some(t) = self.workspace.as_ref().and_then(|w| w.theory.as_ref()) else { return Task::none() };
                let indent = t.profile.next_indent(&t.line_before_caret());
                let paste = cosmic::widget::text_editor::Action::Edit(cosmic::widget::text_editor::Edit::Paste(Arc::new(format!("\n{indent}"))));
                if t.block_editor.is_some() && t.mode == Mode::Literate {
                    return self.update(Message::BlockAction(paste));
                }
                return self.update(Message::SourceAction(paste));
            }
            Message::Complete(i) => {
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                if let Some(t) = self.workspace.as_mut().and_then(|w| w.theory.as_mut()) {
                    t.accept_completion(i, &*table);
                    return Task::batch([self.flush_edits(), self.schedule_state()]);
                }
            }
            Message::InsertOutput(proof) => {
                // A proof from the output at the caret: it replaces a proof-search
                // command (sledgehammer, try0, try), else follows the command
                let Some(machine) = self.workspace.as_ref().map(|w| w.machine.clone()) else { return Task::none() };
                let table = self.table(&machine);
                let Some(ws) = &mut self.workspace else { return Task::none() };
                let Some(out) = ws.output.clone() else { return Task::none() };
                let Some(t) = &mut ws.theory else { return Task::none() };
                let cmd = &out["command"];
                let (Some(start), Some(end)) = (cmd["start"].as_u64(), cmd["end"].as_u64()) else { return Task::none() };
                if out["revision"].as_u64() != Some(t.confirmed_rev) {
                    self.status = Some("The theory changed since that output; place the caret again".into());
                    return Task::none();
                }
                let (s, e) = (Self::local_offset(t, start as usize), Self::local_offset(t, end as usize));
                // (a tool that finds proofs, or the placeholder, is replaced by the proof)
                let name = cmd["name"].as_str().unwrap_or("");
                let replaced = ws.caps.typed_tool(name).is_some_and(|tool| tool.kind == "prove") || t.profile.unfinished().contains(&name);
                if replaced {
                    t.replace(s, e, &proof, &*table);
                } else {
                    // Lined up under the command: a one-line proof indented under it, an outline
                    // (case … next … qed) at its level, each line
                    let chars: Vec<char> = t.local.chars().collect();
                    let mut ls = s.min(chars.len());
                    while ls > 0 && chars[ls - 1] != '\n' {
                        ls -= 1;
                    }
                    let indent: String = chars[ls..].iter().take_while(|c| **c == ' ' || **c == '\t').collect();
                    let lines: Vec<&str> = proof.lines().collect();
                    let text: String = if lines.len() > 1 {
                        lines.iter().map(|l| format!("\n{indent}{l}")).collect()
                    } else {
                        format!("\n{indent}  {proof}")
                    };
                    t.insert(e, &text, &*table);
                }
                return self.flush_edits();
            }
            Message::PopIn => {
                if let Some(id) = self.popout.take() {
                    return cosmic::iced::window::close(id);
                }
            }
            Message::PopOutClosed(id) => {
                if self.popout == Some(id) {
                    self.popout = None;
                }
            }
            Message::Merge(op) => {
                if let Some((machine, sid)) = self.workspace_ids() {
                    if op == "preview" {
                        self.send(&machine, "merge_preview", json!({"session": sid}), Pending::Plain);
                    } else {
                        self.send(&machine, "merge_apply", json!({"session": sid}), Pending::Plain);
                        if let Some(ws) = &mut self.workspace {
                            ws.merge = None;
                        }
                    }
                }
            }
        }
        Task::none()
    }
}
