// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use alacritty_terminal::{event::Event as TermEvent, term, term::color::Colors as TermColors, tty};
use cosmic::iced::clipboard::dnd::DndAction;
use cosmic::iced::core::keyboard::key::Named;
use cosmic::iced::keyboard::key::Physical;
use cosmic::widget::menu::action::MenuAction;
use cosmic::widget::menu::key_bind::KeyBind;
use cosmic::widget::pane_grid::Pane;
use cosmic::widget::segmented_button::ReorderEvent;
use cosmic::{
    Application, ApplicationExt, Element, action,
    app::{Core, Settings, Task, context_drawer},
    cosmic_config::{self, ConfigSet, CosmicConfigEntry},
    cosmic_theme, executor,
    iced::{
        self, Alignment, Color, Event, Length, Limits, Padding, Subscription,
        advanced::graphics::text::font_system,
        clipboard, event,
        futures::SinkExt,
        keyboard::{Event as KeyEvent, Key, Modifiers},
        mouse::{Button as MouseButton, Event as MouseEvent},
        stream, window,
    },
    style,
    widget::{self, DndDestination, PaneGrid, about::About, button, pane_grid, segmented_button},
};
use cosmic::{Apply, surface};
use cosmic_files::dialog::{Dialog, DialogKind, DialogMessage, DialogResult, DialogSettings};
use cosmic_text::{Family, Stretch, Weight, fontdb::FaceInfo};
use localize::LANGUAGE_SORTER;
use std::{
    any::TypeId,
    cell::Cell,
    cmp,
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    error::Error,
    fs,
    path::PathBuf,
    process,
    rc::Rc,
    sync::{LazyLock, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use config::{
    AppTheme, CONFIG_VERSION, ColorScheme, ColorSchemeId, ColorSchemeKind, Config, MachineId,
    Profile, ProfileId, TabColor,
};
mod agent;
mod restore;
mod thread;
mod host;
mod launcher_forms;
mod machines;
mod peers;
mod sidebar;
mod secrets;
mod mount_setup_view;
use cosmic_term::transfer;
use launcher_forms::{LauncherForm, TransferForm, WorktreeField, WorktreeForm, blocking};
mod config;
mod mouse_reporter;

use icon_cache::IconCache;
mod icon_cache;

use key_bind::key_binds;
mod key_bind;

mod shortcuts;

mod localize;

use menu::menu_bar;
mod menu;

use terminal::{Terminal, TerminalPaneGrid, TerminalScroll};
mod terminal;

use terminal_box::terminal_box;

use crate::dnd::DndDrop;
use cosmic::iced::clipboard::dnd::{DndEvent, OfferEvent, SourceEvent};
use crate::menu::MenuState;
mod terminal_box;

#[cfg(feature = "password_manager")]
mod password_manager;
mod terminal_theme;

mod dnd;

use clap_lex::RawArgs;

static ICON_CACHE: LazyLock<Mutex<IconCache>> = LazyLock::new(|| Mutex::new(IconCache::new()));

pub fn icon_cache_get(name: &'static str, size: u16) -> widget::icon::Icon {
    let mut icon_cache = ICON_CACHE.lock().unwrap();
    icon_cache.get(name, size)
}

/// A tab whose agent has not started yet. It offers where to start: the home
/// directory, a recent project or a new worktree of one, a folder chosen in
/// a picker, a past conversation of either agent on either computer to
/// resume, continue or move here, or a running session to attach to
pub struct Launcher {
    id: String,
    profile_id: ProfileId,
    filter: String,
    filter_id: widget::Id,
    /// Each host's projects and conversations, by ssh destination, with
    /// this computer as ""
    listings: BTreeMap<String, Listing>,
    /// The agent's sessions on the profile's host
    running: Vec<agent::Session>,
    /// A form shown instead of the list
    form: Option<LauncherForm>,
    /// A user-triggered launch step that has not returned yet.
    pending_action: Option<(String, String)>,
    /// Transfers that stopped part way, to resume or roll back
    unfinished: Vec<transfer::Record>,
}

/// What a launcher knows of one host's projects and conversations
pub enum Listing {
    Loading,
    Ready(agent::HostListing),
    /// The host could not be reached; its last listing, if there was one,
    /// is shown but cannot be launched from
    Offline(Option<agent::HostListing>, String),
}

impl Listing {
    fn listing(&self) -> Option<&agent::HostListing> {
        match self {
            Self::Ready(listing) | Self::Offline(Some(listing), _) => Some(listing),
            _ => None,
        }
    }
}

/// Where an agent tab's agent is working, as last found. Agents seldom stay
/// where they started: new tabs and Open in Zed follow this instead
#[derive(Default)]
pub struct TabFocus {
    pub dir: Option<String>,
    pub checked: Option<Instant>,
    pub pending: bool,
}

/// How often an agent tab's directory is looked up, at most
const FOCUS_INTERVAL: Duration = Duration::from_secs(10);

/// A field of a machine in the settings
#[derive(Clone, Copy, Debug)]
pub enum MachineField {
    Name,
    Host,
    User,
}

/// A machine's check in the settings
pub enum MachineCheck {
    Running,
    Done(Result<machines::Probe, String>),
}

/// Where a launcher starts its agent
#[derive(Clone, Debug)]
pub enum LaunchChoice {
    Dir(String),
    Resume { id: String, cwd: String },
    Attach(agent::Session),
    Browse,
    /// A new worktree of the project in this directory
    NewWorktree(String),
    /// Another agent's conversation on this computer, continued from its
    /// transcript
    Continue { conversation: transfer::ConversationRef, cwd: String },
    /// A conversation on the other computer, brought here
    Transfer {
        conversation: transfer::ConversationRef,
        cwd: String,
        title: String,
        mode: transfer::Mode,
    },
}

/// A row of a launcher
struct LauncherRow {
    label: String,
    detail: String,
    when: String,
    /// What pressing the row does; nothing for an offline host's rows
    choice: Option<LaunchChoice>,
    /// Buttons at the end of the row
    actions: Vec<(String, LaunchChoice)>,
}

use cosmic_term::provider::Kind as ConversationKind;

/// The kind of conversation an agent keeps
fn agent_kind(agent: config::Agent) -> Option<ConversationKind> {
    match agent {
        config::Agent::Claude => Some(ConversationKind::Claude),
        config::Agent::Codex => Some(ConversationKind::Codex),
        config::Agent::Shell => None,
    }
}

/// Where launchers keep each host's last listing, for when it is offline
fn listing_cache(host: &str) -> PathBuf {
    let name = if host.is_empty() { "local".to_string() } else { cosmic_term::project::slug(host) };
    cosmic_term::Env::from_process()
        .state_dir
        .join("launcher")
        .join(format!("{name}.json"))
}

/// Shorten `text` to at most `max` characters by replacing its middle, which
/// keeps both ends of a path readable
fn ellipsize_middle(text: &str, max: usize) -> String {
    let len = text.chars().count();
    if len <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    let head: String = text.chars().take(keep / 2).collect();
    let tail: String = text.chars().skip(len - (keep - keep / 2)).collect();
    format!("{head}…{tail}")
}

/// How long ago `epoch_secs` was, briefly
fn time_ago(epoch_secs: f64) -> String {
    if epoch_secs <= 0.0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |now| now.as_secs_f64());
    let minutes = ((now - epoch_secs).max(0.0) / 60.0) as i64;
    let (hours, days) = (minutes / 60, minutes / 1440);
    match minutes {
        0 => fl!("time-just-now"),
        1..60 => fl!("time-minutes-ago", n = minutes),
        60..1440 => fl!("time-hours-ago", n = hours),
        _ => fl!("time-days-ago", n = days),
    }
}

/// Before a tab's title while its session waits for sudo approval
/// A field's id, named and numbered: a field that may take the place of
/// another in the widget tree, as a tab's launcher does another tab's, needs
/// one, as the tree gives an unnamed field the id of the one it replaces,
/// and the focus would then be asked of a field no longer there
pub(crate) fn field_id(kind: &str) -> widget::Id {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    widget::Id::new(format!("{kind}-{n}"))
}

/// What goes without a program a machine lacks
fn tool_needed(tool: &str) -> String {
    match tool {
        "python3" => fl!("tool-python3"),
        "git" => fl!("tool-git"),
        _ => fl!("tool-mounts"),
    }
}

/// How long the pointer rests on a tab before its preview shows
const TAB_PREVIEW_DELAY: Duration = Duration::from_millis(400);

const SUDO_LOCK: &str = "🔒 ";

/// A tab whose session is open in another window too, whose bus name this is
pub struct TabShared {
    peer: String,
}

/// The value of [`host::TAB_VAR`] in a tab's processes
pub struct TabToken(String);

/// How a tab is marked: its colour, and whether it runs on a remote host
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TabMark {
    color: Option<TabColor>,
    remote: bool,
    /// Lost its connection to the remote host, and drawn in grey
    disconnected: bool,
}

/// Grey, for tabs whose remote host is out of reach
const DISCONNECTED_GREY: TabColor = TabColor([0x80, 0x80, 0x80]);
/// The accent of tabs without a colour: grey, which [`App::theme`] brings to
/// the lightness of the theme's own accent, so light on a dark theme and dark
/// on a light one
const NEUTRAL_ACCENT: TabColor = TabColor([0x80, 0x80, 0x80]);

/// How a remote tab was started, to reconnect it after its connection drops
pub struct TabConnection {
    launch: host::HostCommand,
    started: Instant,
    /// When the connection dropped, if it has
    lost: Option<Instant>,
}

impl TabMark {
    /// A computer or server in the tab's colour. Unmarked local tabs have no
    /// icon, as in upstream
    fn icon(self, size: u16) -> Option<widget::icon::Icon> {
        let name = if self.remote {
            "network-server-symbolic"
        } else {
            "computer-symbolic"
        };
        match self.strip_color() {
            Some(color) => Some(ICON_CACHE.lock().unwrap().get_colored(name, color.0, size)),
            None => self.remote.then(|| icon_cache_get(name, size)),
        }
    }

    /// The colour of the tab's icon, and of its session's strip in the sidebar
    fn strip_color(self) -> Option<TabColor> {
        if self.disconnected {
            Some(DISCONNECTED_GREY)
        } else {
            self.color
        }
    }
}

/// Runs application with these settings
#[rustfmt::skip]
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(std::ffi::OsStr::new(host::HOST_EXEC_ARG)) {
        return Err(host::host_exec(args.collect()).into());
    }

    let raw_args = RawArgs::from_args();
    let mut cursor = raw_args.cursor();

    let mut shell_program_opt = None;
    let mut shell_args = Vec::new();
    let mut daemonize = true;
    let mut working_directory = None;
    let mut startup_attach = None;
    let mut startup_thread: Option<StartupThread> = None;
    // Parse the arguments using clap_lex
    while let Some(arg) = raw_args.next_os(&mut cursor) {
        match arg.to_str() {
            Some("--help") | Some("-h") => {
                print_help();
                return Ok(());
            }
            Some("--version") | Some("-V") => {
                println!(
                    "cosmic-term {}",
                    env!("CARGO_PKG_VERSION"),
                );
                return Ok(());
            }
            Some(arg_str @ "--working-directory") | Some(arg_str @ "-w") => {
                if let Some(dir_arg) = raw_args.next_os(&mut cursor) {
                    working_directory = Some(PathBuf::from(dir_arg));
                } else {
                    eprintln!("Missing argument for {arg_str}");
                    process::exit(1);
                }
            }
            Some(arg_str @ "--attach") => {
                match raw_args
                    .next_os(&mut cursor)
                    .and_then(|arg| agent::Session::from_descriptor(arg.to_str()?))
                {
                    Some(session) => startup_attach = Some(session),
                    None => {
                        eprintln!("Missing or invalid session for {arg_str}");
                        process::exit(1);
                    }
                }
            }
            Some(arg_str @ "--thread") => match raw_args.next_os(&mut cursor) {
                Some(dir) => {
                    startup_thread.get_or_insert_with(StartupThread::default).dir =
                        dir.to_string_lossy().into_owned();
                }
                None => {
                    eprintln!("Missing directory for {arg_str}");
                    process::exit(1);
                }
            },
            Some(arg_str @ "--host") => match raw_args.next_os(&mut cursor) {
                Some(host) => {
                    startup_thread.get_or_insert_with(StartupThread::default).host =
                        Some(host.to_string_lossy().into_owned());
                }
                None => {
                    eprintln!("Missing destination for {arg_str}");
                    process::exit(1);
                }
            },
            Some(arg_str @ "--agent") => match raw_args.next_os(&mut cursor).map(|a| a.to_string_lossy().into_owned()) {
                Some(agent) if agent == "claude" || agent == "codex" => {
                    startup_thread.get_or_insert_with(StartupThread::default).codex = agent == "codex";
                }
                _ => {
                    eprintln!("{arg_str} takes claude or codex");
                    process::exit(1);
                }
            },
            Some(arg_str @ ("--resume" | "--prompt")) => match raw_args.next_os(&mut cursor) {
                Some(value) => {
                    let thread = startup_thread.get_or_insert_with(StartupThread::default);
                    let value = value.to_string_lossy().into_owned();
                    if arg_str == "--resume" {
                        thread.resume = Some(value);
                    } else {
                        thread.prompt = Some(value);
                    }
                }
                None => {
                    eprintln!("Missing argument for {arg_str}");
                    process::exit(1);
                }
            },
            Some("--no-daemon") => {
                daemonize = false;
            }
            Some("-e") | Some("--command") | Some("--") => {
                // Handle the '--command' or '-e' flag
                break;
            }
            _ => {
                //TODO: should this throw an error?
                log::warn!("ignored argument {:?}", arg);
            }
        }
    }
    // After flags, process remaining shell program and args
    while let Some(arg) = raw_args.next_os(&mut cursor) {
        if shell_program_opt.is_some() {
            shell_args.push(arg.to_string_lossy().to_string());
        } else {
            shell_program_opt = Some(arg.to_string_lossy().to_string());
        }
    }

    // Platform-specific daemonization logic

    // In a Flatpak the sandbox ends with the process it started, taking a
    // forked daemon with it
    #[cfg(all(unix, not(target_os = "redox")))]
    if daemonize && !host::is_sandboxed() {
        match fork::daemon(true, true) {
            Ok(fork::Fork::Child) => (),
            Ok(fork::Fork::Parent(_child_pid)) => process::exit(0),
            Err(err) => {
                eprintln!("failed to daemonize: {:?}", err);
                process::exit(1);
            }
        }
    }

    localize::localize();

    let (config_handler, config) = match cosmic_config::Config::new(App::APP_ID, CONFIG_VERSION) {
        Ok(config_handler) => {
            let config = match Config::get_entry(&config_handler) {
                Ok(ok) => ok,
                Err((errs, config)) => {
                    log::info!("errors loading config: {:?}", errs);
                    config
                }
            };
            (Some(config_handler), config)
        }
        Err(err) => {
            log::error!("failed to create config handler: {}", err);
            (None, Config::default())
        }
    };

    let mut config = config;
    if agent::seed_profiles(&mut config)
        && let Some(config_handler) = &config_handler
    {
        for result in [
            config_handler.set("profiles", &config.profiles),
            config_handler.set("agent_profiles_seeded", true),
            config_handler.set("profile_seed_version", config.profile_seed_version),
        ] {
            if let Err(err) = result {
                log::error!("failed to save agent profiles: {}", err);
            }
        }
    }
    // Profiles' hosts become machines, once
    if config.adopt_machines()
        && let Some(config_handler) = &config_handler
    {
        for result in [
            config_handler.set("machines", &config.machines),
            config_handler.set("profiles", &config.profiles),
        ] {
            if let Err(err) = result {
                log::error!("failed to save machines: {}", err);
            }
        }
    }

    let shortcuts_config = shortcuts::ShortcutsConfig::new(config.shortcuts_custom.clone());

    let startup_command =
        shell_program_opt.map(|shell_program| host::HostCommand::new(shell_program, shell_args));
    let startup_options = Some(tty::Options {
        working_directory,
        ..tty::Options::default()
    });

    // Terminal config setup
    let term_config = term::Config {
        scrolling_history: 100_000,
        ..term::Config::default()
    };
    // Set up environmental variables for terminal
    tty::setup_env();
    // Override TERM for better compatibility
    unsafe {
        env::set_var("TERM", "xterm-256color");
    }

    // Set settings
    let mut settings = Settings::default();
    settings = settings.theme(config.app_theme.theme());
    settings = settings.size_limits(Limits::NONE.min_width(360.0).min_height(180.0));

    // Flags
    // Started plainly, a window reopens the tabs a closed one left
    let restore = std::env::args().skip(1).all(|arg| arg == "--no-daemon");
    let flags = Flags {
        restore,
        config_handler,
        config,
        shortcuts_config,
        startup_options,
        startup_command,
        startup_attach,
        startup_thread,
        term_config,
    };

    // Run the cosmic app
    cosmic::app::run::<App>(settings, flags)?;

    Ok(())
}

fn print_help() {
    println!(
        r#"COSMIC Terminal
Designed for the COSMIC™ desktop environment, cosmic-term is a libcosmic-based terminal emulator.

Project home page: https://github.com/pop-os/cosmic-term
Options:
  --help                          Show this message
  --version                       Show the version of cosmic-term
  -w, --working-directory <dir>   Set the working directory for the terminal
  --attach <session>[@<host>]     Open a tab attached to a Zellij session
  --thread <dir>                  Open an agent thread working in <dir>
  --agent claude|codex            ... with this agent (Claude by default)
  --host <destination>            ... on this machine, over ssh
  --resume <id>                   ... continuing conversation <id>
  --prompt <text>                 ... and send <text> as its first message"#
    );
}

/// A thread to open at startup, from `--thread`, `--resume` and `--prompt`
#[derive(Clone, Debug, Default)]
pub struct StartupThread {
    dir: String,
    resume: Option<String>,
    prompt: Option<String>,
    /// Codex's rather than Claude's
    codex: bool,
    /// The machine it runs on, an ssh destination, if not this one
    host: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Flags {
    /// Reopen a closed window's tabs
    restore: bool,
    config_handler: Option<cosmic_config::Config>,
    config: Config,
    shortcuts_config: shortcuts::ShortcutsConfig,
    startup_options: Option<tty::Options>,
    startup_command: Option<host::HostCommand>,
    startup_attach: Option<agent::Session>,
    startup_thread: Option<StartupThread>,
    term_config: term::Config,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    About,
    DarkModeToggle,
    ThreadMatchColours,
    ThreadCopy,
    ThreadPaper,
    ThreadSelectAll,
    ClearScrollback,
    ColorSchemes(ColorSchemeKind),
    Copy,
    CopyUrlByMenu,
    CopyOrSigint,
    CopyPrimary,
    Find,
    KeyboardShortcuts,
    LaunchUrlByMenu,
    PaneFocusDown,
    PaneFocusLeft,
    PaneFocusRight,
    PaneFocusUp,
    PaneSplitHorizontal,
    PaneSplitVertical,
    PaneToggleMaximized,
    Paste,
    PastePrimary,
    ProfileOpen(ProfileId),
    Profiles,
    SelectAll,
    Settings,
    #[cfg(feature = "password_manager")]
    PasswordManager,
    ShowHeaderBar(bool),
    TabActivate0,
    TabActivate1,
    TabActivate2,
    TabActivate3,
    TabActivate4,
    TabActivate5,
    TabActivate6,
    TabActivate7,
    TabActivate8,
    TabClose,
    TabNew,
    NewTabMenu,
    TabNewNoProfile,
    RemoteEnd,
    TabPopOut,
    /// Move or fork an agent tab's conversation to this profile's computer
    TabMoveTo(ProfileId),
    TabForkTo(ProfileId),
    OpenLocallyByMenu,
    OpenInZed,
    OpenInFiles,
    /// Rename the conversation of the tab in front
    SessionRename,
    SidebarToggle,
    SecretsToggle,
    RemoteFiles,
    /// Paste a saved password from the secrets store
    PastePassword(u64),
    TabNext,
    TabPrev,
    ToggleFullscreen,
    WindowClose,
    WindowNew,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

impl Action {
    fn message(&self, entity_opt: Option<segmented_button::Entity>) -> Message {
        match self {
            Self::About => Message::ToggleContextPage(ContextPage::About),
            Self::DarkModeToggle => Message::ToggleDarkMode,
            Self::ThreadMatchColours => Message::ThreadMatchColours(None),
            Self::ThreadCopy => Message::ThreadCopy,
            Self::ThreadPaper => Message::ThreadPaperToggle,
            Self::ThreadSelectAll => Message::ThreadSelectAll,
            Self::ClearScrollback => Message::ClearScrollback(entity_opt),
            Self::ColorSchemes(color_scheme_kind) => {
                Message::ToggleContextPage(ContextPage::ColorSchemes(*color_scheme_kind))
            }
            Self::Copy => Message::Copy(entity_opt),
            Self::CopyUrlByMenu => Message::CopyUrlByMenu,
            Self::CopyOrSigint => Message::CopyOrSigint(entity_opt),
            Self::CopyPrimary => Message::CopyPrimary(entity_opt),
            Self::Find => Message::Find(true),
            Self::KeyboardShortcuts => Message::ToggleContextPage(ContextPage::KeyboardShortcuts),
            Self::LaunchUrlByMenu => Message::LaunchUrlByMenu,
            Self::PaneFocusDown => Message::PaneFocusAdjacent(pane_grid::Direction::Down),
            Self::PaneFocusLeft => Message::PaneFocusAdjacent(pane_grid::Direction::Left),
            Self::PaneFocusRight => Message::PaneFocusAdjacent(pane_grid::Direction::Right),
            Self::PaneFocusUp => Message::PaneFocusAdjacent(pane_grid::Direction::Up),
            Self::PaneSplitHorizontal => Message::PaneSplit(pane_grid::Axis::Horizontal),
            Self::PaneSplitVertical => Message::PaneSplit(pane_grid::Axis::Vertical),
            Self::PaneToggleMaximized => Message::PaneToggleMaximized,
            #[cfg(feature = "password_manager")]
            // The Secrets pane took the password manager's place
            Self::PasswordManager => Message::SecretsToggle,
            Self::Paste => Message::Paste(entity_opt),
            Self::PastePrimary => Message::PastePrimary(entity_opt),
            Self::ProfileOpen(profile_id) => Message::ProfileOpen(*profile_id),
            Self::RemoteFiles => Message::RemoteFilesOpen(String::new()),
            Self::Profiles => Message::ToggleContextPage(ContextPage::Profiles),
            Self::SelectAll => Message::SelectAll(entity_opt),
            Self::Settings => Message::ToggleContextPage(ContextPage::Settings),
            Self::ShowHeaderBar(show_headerbar) => Message::ShowHeaderBar(*show_headerbar),
            Self::TabActivate0 => Message::TabActivateJump(0),
            Self::TabActivate1 => Message::TabActivateJump(1),
            Self::TabActivate2 => Message::TabActivateJump(2),
            Self::TabActivate3 => Message::TabActivateJump(3),
            Self::TabActivate4 => Message::TabActivateJump(4),
            Self::TabActivate5 => Message::TabActivateJump(5),
            Self::TabActivate6 => Message::TabActivateJump(6),
            Self::TabActivate7 => Message::TabActivateJump(7),
            Self::TabActivate8 => Message::TabActivateJump(8),
            Self::TabClose => Message::TabClose(entity_opt),
            Self::TabNew => Message::TabNew,
            Self::NewTabMenu => Message::NewTabMenuToggle,
            Self::TabNewNoProfile => Message::TabNewNoProfile,
            Self::RemoteEnd => Message::RemoteEnd(entity_opt),
            Self::TabPopOut => Message::TabPopOut(entity_opt),
            Self::OpenInZed => Message::OpenInZed,
            Self::OpenInFiles => Message::OpenInFiles,
            Self::SessionRename => Message::SessionRename(entity_opt),
            Self::SidebarToggle => Message::SidebarToggle,
            Self::SecretsToggle => Message::SecretsToggle,
            Self::PastePassword(id) => Message::SecretsPaste(*id),
            Self::TabMoveTo(profile_id) => Message::TabTransfer(entity_opt, *profile_id, transfer::Mode::Move),
            Self::TabForkTo(profile_id) => Message::TabTransfer(entity_opt, *profile_id, transfer::Mode::Fork),
            Self::OpenLocallyByMenu => Message::OpenLocallyByMenu,
            Self::TabNext => Message::TabNext,
            Self::TabPrev => Message::TabPrev,
            Self::ToggleFullscreen => Message::ToggleFullscreen,
            Self::WindowClose => Message::WindowClose,
            Self::WindowNew => Message::WindowNew,
            Self::ZoomIn => Message::ZoomIn,
            Self::ZoomOut => Message::ZoomOut,
            Self::ZoomReset => Message::ZoomReset,
        }
    }
}

impl MenuAction for Action {
    type Message = Message;

    fn message(&self) -> Message {
        self.message(None)
    }
}

/// Messages that are used specifically by our [`App`].
#[derive(Clone, Debug)]
pub enum Message {
    AppTheme(AppTheme),
    /// The sessions of a reopened window that still run, to attach again
    RestoreSessions(Vec<agent::Session>),
    /// A conversation whose agent has ended, to continue in a thread or a
    /// terminal: host, agent, directory and conversation
    ContinueConversation { host: String, agent: config::Agent, cwd: String, conversation: String, thread: bool },
    /// A diagram's Mermaid source, to open in a window of its own
    OpenDiagram(String),
    /// A control in an image or diagram window
    Viewer(window::Id, thread::viewer::Action),
    /// A thread tab's own message
    Thread(thread::tab::ThreadId, thread::tab::Msg),
    /// Images chosen to attach to a thread's next message
    ThreadImagesPicked(thread::tab::ThreadId, DialogResult),
    /// Set threads as papers, or not
    ThreadPaper(bool),
    ThreadPaperToggle,
    /// Set, or with `None` toggle, whether images match the view's colours
    ThreadMatchColours(Option<bool>),
    ThreadCopy,
    ThreadSelectAll,
    /// A thread setting changed
    ThreadSetting(ThreadSetting),
    /// Stop idle agents and unload threads beyond the limits
    ThreadTick,
    ToggleDarkMode,
    OpenImage(PathBuf),
    ImageZoom(window::Id, thread::viewer::Zoom),
    ImageCopyPath(window::Id),
    ImageOpenExternally(window::Id),
    ImageWindowClosed(window::Id),
    /// The wheel turned in a window, for Ctrl+scroll zoom
    Wheel(window::Id, iced::mouse::ScrollDelta),
    /// A step of the debug driver; see `thread::debug`
    #[cfg(debug_assertions)]
    DebugStep(usize),
    #[cfg(debug_assertions)]
    DebugShot(String, window::Screenshot, usize),
    ClearScrollback(Option<segmented_button::Entity>),
    ColorSchemeCollapse,
    ColorSchemeDelete(ColorSchemeKind, ColorSchemeId),
    ColorSchemeExpand(ColorSchemeKind, Option<ColorSchemeId>),
    ColorSchemeExport(ColorSchemeKind, Option<ColorSchemeId>),
    ColorSchemeExportResult(ColorSchemeKind, Option<ColorSchemeId>, DialogResult),
    ColorSchemeExported(Result<(), String>),
    ColorSchemeImport(ColorSchemeKind),
    ColorSchemeImportResult(ColorSchemeKind, DialogResult),
    ColorSchemeImported(ColorSchemeKind, Vec<Result<ColorScheme, String>>),
    ColorSchemeIoSlow(u64),
    ColorSchemeRename(ColorSchemeKind, ColorSchemeId, String),
    ColorSchemeRenameSubmit,
    ColorSchemeTabActivate(widget::segmented_button::Entity),
    Config(Box<Config>),
    Copy(Option<segmented_button::Entity>),
    CopyOrSigint(Option<segmented_button::Entity>),
    CopyPrimary(Option<segmented_button::Entity>),
    CopyUrlByMenu,
    DefaultBoldFontWeight(usize),
    DefaultDimFontWeight(usize),
    DefaultFont(usize),
    DefaultFontSize(usize),
    DefaultFontStretch(usize),
    DefaultFontWeight(usize),
    DefaultZoomStep(usize),
    DialogMessage(Box<DialogMessage>), // DialogMessage is huge, so we use a box to make the size of this enum smaller on the stack
    Drop(Option<(pane_grid::Pane, segmented_button::Entity, DndDrop)>),
    Find(bool),
    FindNext,
    FindPrevious,
    FindSearchValueChanged(String),
    MiddleClick(pane_grid::Pane, Option<segmented_button::Entity>),
    FocusFollowMouse(bool),
    Key(Modifiers, Physical, Key),
    /// A key no widget took, with the text it types
    KeyUnclaimed(Modifiers, Physical, Key, Option<String>),
    LaunchUrl(String),
    LaunchUrlByMenu,
    Modifiers(Modifiers),
    ShortcutCaptureCancel,
    ShortcutCaptureStart(shortcuts::KeyBindAction),
    ShortcutConflictCancel,
    ShortcutConflictReplace,
    ShortcutRemove(shortcuts::Binding, shortcuts::BindingSource),
    ShortcutReset(shortcuts::KeyBindAction),
    ShortcutSearch(String),
    MouseEnter(pane_grid::Pane),
    CloseToast(widget::ToastId),
    LauncherBrowse(segmented_button::Entity, Option<PathBuf>, DialogResult),
    LauncherChoose(segmented_button::Entity, LaunchChoice),
    LauncherData(segmented_button::Entity, String, Result<agent::HostListing, String>),
    LauncherFormCancel(segmented_button::Entity),
    /// A field of a tab's launcher took the focus
    LauncherFocus(segmented_button::Entity, widget::Id),
    LauncherRecover(segmented_button::Entity, String, bool),
    LauncherRecovered(segmented_button::Entity, String, String, Result<Option<transfer::Launch>, String>),
    LauncherStart(segmented_button::Entity, String, String, Result<transfer::Start, String>),
    LauncherTransferChoose(segmented_button::Entity, transfer::RepoChoice),
    LauncherTransferDone(segmented_button::Entity, String, String, Result<transfer::Launch, String>),
    // Boxed, as they are large
    LauncherTransferDraft(segmented_button::Entity, String, Box<Result<transfer::Draft, String>>),
    LauncherTransferPath(segmented_button::Entity, String),
    LauncherTransferProgress(segmented_button::Entity, String, transfer::Step),
    LauncherTransferSlow(segmented_button::Entity, String, transfer::Step),
    LauncherTransferPlace(
        segmented_button::Entity,
        String,
        u64,
        Box<Result<cosmic_term::helper::PlaceInfo, String>>,
    ),
    LauncherTransferStart(segmented_button::Entity),
    LauncherWorktreeBase(segmented_button::Entity, usize),
    LauncherWorktreeCreate(segmented_button::Entity),
    LauncherWorktreeCreated(segmented_button::Entity, String, Result<PathBuf, String>),
    LauncherWorktreeEdit(segmented_button::Entity, WorktreeField, String),
    LauncherWorktreeOptions(
        segmented_button::Entity,
        String,
        Result<cosmic_term::helper::WorktreeOptions, String>,
    ),
    LauncherFilter(segmented_button::Entity, String),
    LauncherMounted(segmented_button::Entity, String, Result<PathBuf, String>),
    LauncherSessions(segmented_button::Entity, Result<Vec<agent::ListedSession>, String>),
    LauncherSubmit(segmented_button::Entity),
    LauncherToggleSection(config::LauncherSection),
    HostProbed(String, u64, bool),
    OpenInZed,
    OpenInFiles,
    OpenInFilesResult(Result<(), String>),
    /// Show or hide the sidebar of sessions
    SidebarToggle,
    /// Scan a host's sessions again, unless the sidebar was hidden or
    /// shown again since this was asked for
    SidebarRefresh(String, u64),
    SidebarSummaries(String, u64, Result<Vec<agent::SessionSummary>, String>),
    /// Bring a session's tab to the front, or open one for it
    SidebarOpen(agent::Session),
    /// Rename the conversation of a tab, or of the one in front
    SessionRename(Option<segmented_button::Entity>),
    /// The tab under the pointer, and its centre from the tab bar's left
    TabHover(pane_grid::Pane, Option<(segmented_button::Entity, f32)>),
    /// Time to show a hovered tab's preview
    TabPreviewDue,
    /// Open or close the new tab menu from the sidebar's New button
    SidebarNewMenu,
    /// Another window has the session, under this bus name, if any
    SidebarPeerFound(agent::Session, Result<Option<String>, String>),
    /// For a tab also open in another window: go to that window instead,
    /// move the session here, or keep both and hide the bar saying so
    SharedGoThere(segmented_button::Entity),
    SharedMoveHere(segmented_button::Entity),
    SharedDismiss(segmented_button::Entity),
    RenameInput(String),
    RenameSubmit,
    RenameCancel,
    Renamed(Result<(), String>),
    /// An activation token for raising another window
    PeerFocusToken(String, String, Option<String>),
    PeerAsked(Result<bool, String>),
    /// Another window asks something of this one
    PeerRequest(peers::Request),
    SudoConnected(tokio::sync::mpsc::UnboundedSender<cosmic_term::sudo_control::ToAgent>),
    SudoDisconnected,
    SudoEvent(cosmic_term::sudo_control::FromAgent),
    SudoPassword(u64, String),
    SudoApprove(u64),
    SudoDeny(u64),
    /// Redraw, as when a click-approval becomes armed
    SudoRedraw,
    /// Set a machine up: its ssh destination (empty for this computer) and
    /// host name
    SudoSetup(String, String),
    SudoPrepared(String, Result<String, String>),
    SecretsToggle,
    /// A field of the Secrets pane or a sudo prompt lost the focus
    SecretsUnfocus(widget::Id),
    /// A click gave a pane's terminal the keys
    TerminalClickFocus(pane_grid::Pane),
    /// A field of the Secrets pane took the focus: the terminal gives it up
    SecretsFocus(widget::Id),
    SecretsPassword(String),
    SecretsAgain(String),
    SecretsCreate,
    SecretsUnlock,
    SecretsLock,
    SecretsPaste(u64),
    SecretsAskPassword(String),
    SecretsAskSubmit,
    /// Add a secret, for a machine by host name or for Other
    SecretsAdd(Option<String>),
    /// Edit a secret, or stop editing
    SecretsEdit(Option<u64>),
    SecretsFormLabel(String),
    SecretsFormSecret(String),
    SecretsFormNote(String),
    SecretsFormAlwaysAsk(bool),
    SecretsSave,
    SecretsDelete(u64),
    SecretsPolicy(cosmic_term::sudo_control::Policy),
    SecretsImport,
    SecretsImported(Result<Vec<(String, String)>, String>),
    SudoFinger(usize),
    SudoEnrol,
    SudoResult(String, Result<(), String>),
    /// This computer and a machine can now mount each other
    MountsConnected(String, Result<(), String>),
    RemoteFilesOpen(String),
    RemoteFilesCheck(String),
    RemoteFilesChecked(String, String, Result<cosmic_term::mount_setup::Status, String>),
    RemoteFilesConnectionChecked(String, String, Result<(), String>),
    RemoteFilesInstall(String),
    RemoteFilesCopy(String),
    /// A row's context menu, on the session with this key
    SidebarAction(u64, sidebar::Action),
    SidebarFilter(String),
    SidebarCompact(bool),
    /// Open the first session the filter matches
    SidebarFilterSubmit,
    /// The filter gained or lost focus
    SidebarSearching(bool),
    /// Start a session in a project's main worktree on a host, or a new
    /// worktree of it, with the agent of a profile for that host
    SidebarNew {
        host: String,
        repository: String,
        agent: Option<config::Agent>,
        worktree: bool,
        /// In a thread rather than a terminal
        thread: bool,
    },
    ReconnectNow(String),
    ReconnectSessions(String, Result<Vec<agent::ListedSession>, String>),
    /// The helpers are on a host, and these programs are still missing
    SupervisorInstalled(String, Result<Vec<String>, String>),
    SupervisorProgress(String, Option<agent::Helper>),
    SupervisorSlow(String, Option<agent::Helper>),
    OpenLocallyByMenu,
    OpenLocallyResult(Result<(), String>),
    OpenInZedResult(Result<(), String>),
    NewTabMenuClose,
    NewTabMenuToggle,
    Opacity(u8),
    PaneClicked(pane_grid::Pane),
    PaneDragged(pane_grid::DragEvent),
    PaneFocusAdjacent(pane_grid::Direction),
    PaneResized(pane_grid::ResizeEvent),
    PaneSplit(pane_grid::Axis),
    PaneToggleMaximized,
    #[cfg(feature = "password_manager")]
    PasswordManager(password_manager::PasswordManagerMessage),
    #[cfg(feature = "password_manager")]
    PasswordPaste(secstr::SecUtf8, pane_grid::Pane),
    Paste(Option<segmented_button::Entity>),
    PastePrimary(Option<segmented_button::Entity>),
    PasteValue(Option<segmented_button::Entity>, String),
    ProfileAgent(ProfileId, usize),
    ProfileArgs(ProfileId, String),
    /// Whether a Claude profile opens rich-text threads
    ProfileThread(ProfileId, bool),
    ProfileCollapse(ProfileId),
    ProfileColor(ProfileId, usize),
    ProfileCommand(ProfileId, String),
    ProfileDirectory(ProfileId, String),
    ProfileExpand(ProfileId),
    ProfileHold(ProfileId, bool),
    ProfileMachine(ProfileId, usize),
    MachineAdd,
    MachineAddBlank,
    MachineAddCandidate(usize),
    MachineAddProfiles(MachineId),
    MachineCheck(MachineId),
    MachineChecked(MachineId, String, Result<machines::Probe, String>),
    MachineCollapse(MachineId),
    MachineDiscovered(Result<(String, String), String>),
    MachineDiscoveryClose,
    MachineEdit(MachineId, MachineField, String),
    MachineExpand(MachineId),
    MachineRemove(MachineId),
    LocalProbed(Result<machines::Probe, String>),
    ProfileName(ProfileId, String),
    ProfileNew,
    ProfileOpen(ProfileId),
    ProfileRemove(ProfileId),
    ProfileSyntaxTheme(ProfileId, ColorSchemeKind, usize),
    ProfileTabTitle(ProfileId, String),
    RemoteAttach(agent::Session),
    RemoteEnd(Option<segmented_button::Entity>),
    RemoteEnded(Option<segmented_button::Entity>, Result<(), String>),
    ReorderTab(Pane, ReorderEvent),
    Surface(surface::Action<Message>),
    SelectAll(Option<segmented_button::Entity>),
    ShowAdvancedFontSettings(bool),
    ShowHeaderBar(bool),
    ShowPaneBorders(bool),
    AccentFollowsTab(bool),
    SyntaxTheme(ColorSchemeKind, usize),
    SystemThemeChange,
    TabNewInheritWorkingDirectory(bool),
    TabActivate(segmented_button::Entity),
    TabActivateJump(usize),
    TabClose(Option<segmented_button::Entity>),
    TabContextAction(segmented_button::Entity, Action),
    /// Where an agent tab's agent is working, found
    TabFocusFound(segmented_button::Entity, Result<agent::TabAgents, String>),
    /// Move or fork an agent tab's conversation to a profile's computer
    TabTransfer(Option<segmented_button::Entity>, ProfileId, transfer::Mode),
    TabTransferFound(
        pane_grid::Pane,
        segmented_button::Entity,
        String,
        ProfileId,
        transfer::Mode,
        String,
        Result<agent::TabAgents, String>,
    ),
    TabContextMenu(pane_grid::Pane, Option<MenuState>),
    TabNew,
    TabNewNoProfile,
    TabBarContext(pane_grid::Pane, segmented_button::Entity),
    TabDragEnded(bool),
    TabDragOverWindow(bool),
    TabDragStart(pane_grid::Pane, segmented_button::Entity),
    TabDropped(pane_grid::Pane, Option<DndDrop>),
    TabPopOut(Option<segmented_button::Entity>),
    TabNext,
    TabPrev,
    TermEvent(pane_grid::Pane, segmented_button::Entity, TermEvent),
    TermEventTx(mpsc::UnboundedSender<(pane_grid::Pane, segmented_button::Entity, TermEvent)>),
    ToggleFullscreen,
    ToggleContextPage(ContextPage),
    UpdateDefaultProfile((bool, ProfileId)),
    UseBrightBold(bool),
    WindowClose,
    WindowNew,
    WindowFocused,
    WindowUnfocused,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// A change on the settings page's Threads section
#[derive(Clone, Copy, Debug)]
pub enum ThreadSetting {
    TextSize(usize),
    PaperFont(usize),
    PaperSize(usize),
    PaperLineHeight(usize),
    PaperMeasure(usize),
    PaperJustify(bool),
    PaperTone(usize),
    MathScale(usize),
    IdleMinutes(usize),
    LiveAgents(usize),
    LoadedThreads(usize),
}

/// The choices the Threads settings offer
const THREAD_TEXT_SIZES: [u16; 7] = [12, 13, 14, 15, 16, 17, 18];
const PAPER_SIZES: [u16; 8] = [14, 15, 16, 17, 18, 19, 20, 22];
const PAPER_LINE_HEIGHTS: [u16; 5] = [120, 130, 140, 145, 160];
const PAPER_MEASURES: [u16; 6] = [28, 32, 36, 40, 48, 60];
const MATH_SCALES: [u16; 6] = [90, 100, 110, 121, 130, 150];
const IDLE_MINUTES: [u16; 6] = [1, 2, 5, 10, 30, 60];
const LIVE_AGENTS: [u16; 5] = [1, 2, 3, 5, 8];
const LOADED_THREADS: [u16; 5] = [1, 2, 4, 8, 16];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewTabEntryKind {
    Header,
    Item,
}

/// A row of the new tab menu
pub struct NewTabEntry {
    kind: NewTabEntryKind,
    label: String,
    mark: TabMark,
    is_default: bool,
    /// Set for items only
    message: Option<Message>,
}

impl NewTabEntry {
    fn header(label: String) -> Self {
        Self {
            kind: NewTabEntryKind::Header,
            label,
            mark: TabMark::default(),
            is_default: false,
            message: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextPage {
    About,
    ColorSchemes(ColorSchemeKind),
    KeyboardShortcuts,
    Profiles,
    Settings,
    #[cfg(feature = "password_manager")]
    PasswordManager,
    Secrets,
    RemoteFiles,
}

#[derive(Clone, Debug)]
struct ShortcutConflict {
    binding: shortcuts::Binding,
    existing_action: shortcuts::KeyBindAction,
    new_action: shortcuts::KeyBindAction,
}

/// The [`App`] stores application-specific state.
pub struct App {
    core: Core,
    /// The next thread tab's id
    next_thread: thread::tab::ThreadId,
    /// Images opened from threads, each in its window
    image_windows: HashMap<window::Id, thread::viewer::ImageWindow>,
    about: About,
    pane_model: TerminalPaneGrid,
    config_handler: Option<cosmic_config::Config>,
    config: Config,
    shortcuts_config: shortcuts::ShortcutsConfig,
    key_binds: HashMap<KeyBind, Action>,
    app_themes: Vec<String>,
    font_names: Vec<String>,
    font_size_names: Vec<String>,
    font_sizes: Vec<u16>,
    font_name_faces_map: BTreeMap<String, Vec<FaceInfo>>,
    all_font_weights_vals_names_map: BTreeMap<u16, String>,
    all_font_stretches_vals_names_map: BTreeMap<Stretch, String>,
    curr_font_weight_names: Vec<String>,
    curr_font_weights: Vec<u16>,
    curr_font_stretch_names: Vec<String>,
    curr_font_stretches: Vec<Stretch>,
    zoom_step_names: Vec<String>,
    zoom_steps: Vec<u16>,
    theme_names_dark: Vec<String>,
    theme_names_light: Vec<String>,
    themes: HashMap<(String, ColorSchemeKind), TermColors>,
    context_page: ContextPage,
    dialog_opt: Option<Dialog<Message>>,
    agent_names: Vec<String>,
    /// What a profile's machine dropdown offers: this computer, then each
    /// machine, in `Config::machines` order
    host_names: Vec<String>,
    machine_expanded: Option<MachineId>,
    /// This computer, as a machine check would see it, to recognise it
    /// among the machines
    local: Option<machines::Probe>,
    /// Machines being checked, or what the last check found
    machine_checks: HashMap<MachineId, MachineCheck>,
    /// The add-machine list: `None` when closed, `Some(None)` while looking
    discovery: Option<Option<Result<Vec<machines::Candidate>, String>>>,
    color_names: Vec<String>,
    new_tab_menu_open: bool,
    /// The new tab menu hangs from the sidebar's New button, not the header's
    new_tab_menu_in_sidebar: bool,
    toasts: widget::toaster::Toasts<Message>,
    sidebar: sidebar::Sidebar,
    /// Transfer records owned by work still running in this window. This
    /// also prevents two launchers from recovering the same journal at once.
    active_transfers: BTreeSet<String>,
    transfer_lookups: HashMap<(pane_grid::Pane, segmented_button::Entity), String>,
    /// A session open in another window, whose bus name is given, and what
    /// to do about it
    /// The tab the pointer is over, for its preview
    /// The tab under the pointer, its centre, and since when
    hovered_tab: Option<(pane_grid::Pane, segmented_button::Entity, f32, Instant)>,
    /// Sudo requests from agents, and the connection to the agent
    secrets: secrets::Secrets,
    /// The pane a secret being fetched is pasted into
    secrets_paste_pane: Option<pane_grid::Pane>,
    /// A field of the Secrets pane has the focus, so it keeps it
    /// The field of the Secrets pane or a sudo prompt that has the keys
    secrets_focus: Option<widget::Id>,
    /// Whether this window shows the sidebar; the config holds the last
    /// choice, for new windows, so that other windows do not follow it
    show_sidebar: bool,
    /// Machines whose missing programs have been told
    tools_reported: BTreeSet<String>,
    mount_setup: BTreeMap<String, mount_setup_view::Check>,
    mount_errors: BTreeMap<String, String>,
    /// Installation currently running on each host, updated only when a
    /// real setup step begins.
    host_setup: BTreeMap<String, Option<agent::Helper>>,
    host_setup_stalled: BTreeSet<String>,
    pending_commands: BTreeMap<&'static str, widget::ToastId>,
    host_toasts: BTreeMap<String, widget::ToastId>,
    window_focused: bool,
    /// When each machine's sudo forward was last renewed
    sudo_forwarded: HashMap<String, Instant>,
    /// A conversation being renamed, and the name typed so far
    rename: Option<(agent::Session, String)>,
    /// The thread being renamed, when [`Self::rename`] is a thread's
    rename_thread: Option<thread::tab::ThreadId>,
    /// Whether to reopen a closed window's tabs at startup
    restore_wanted: bool,
    /// This window's record of its tabs, for reopening them
    restore: Option<restore::Claim>,
    rename_id: widget::Id,
    /// The tab colour the theme's accent is set to, if any
    accent: Option<TabColor>,
    terminal_ids: HashMap<pane_grid::Pane, widget::Id>,
    find: bool,
    find_search_id: widget::Id,
    find_search_value: String,
    term_event_tx_opt:
        Option<mpsc::UnboundedSender<(pane_grid::Pane, segmented_button::Entity, TermEvent)>>,
    startup_options: Option<tty::Options>,
    /// The command given on the command line, for the first tab
    startup_command: Option<host::HostCommand>,
    /// A session given on the command line, for the first tab
    startup_attach: Option<agent::Session>,
    startup_thread: Option<StartupThread>,
    /// Remote hosts being reconnected to, with the generation of the pending
    /// probe and the number of attempts so far
    reconnecting: BTreeMap<String, (u64, u32)>,
    /// The tab being dragged from this window, if any, and when the drag
    /// started
    tab_drag: Option<(pane_grid::Pane, segmented_button::Entity, Instant)>,
    /// Whether the dragged tab has left this window, as the pointer must
    /// before a cancelled drag pops the tab out
    tab_drag_left_window: bool,
    term_config: term::Config,
    color_scheme_errors: Vec<String>,
    color_scheme_io: Option<String>,
    color_scheme_io_generation: u64,
    color_scheme_io_slow: bool,
    color_scheme_expanded: Option<(ColorSchemeKind, Option<ColorSchemeId>)>,
    color_scheme_renaming: Option<(ColorSchemeKind, ColorSchemeId, String)>,
    color_scheme_rename_id: widget::Id,
    color_scheme_tab_model: widget::segmented_button::SingleSelectModel,
    profile_expanded: Option<ProfileId>,
    show_advanced_font_settings: bool,
    shortcut_capture: Option<shortcuts::KeyBindAction>,
    shortcut_conflict: Option<ShortcutConflict>,
    shortcut_conflict_overlay_restore: Option<bool>,
    shortcut_search_focus: Cell<bool>,
    shortcut_search_id: widget::Id,
    shortcut_search_regex: Option<regex::Regex>,
    shortcut_search_value: String,
    modifiers: Modifiers,
    #[cfg(feature = "password_manager")]
    password_mgr: password_manager::PasswordManager,
}

fn setup_host_task(host: String) -> Task<Message> {
    cosmic::task::stream(stream::channel(16, move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
        let (send, mut received) = tokio::sync::mpsc::unbounded_channel();
        let progress_host = host.clone();
        let setup = agent::ensure_helpers_progress(host.clone(), move |step| {
            let _ = send.send(step);
        });
        tokio::pin!(setup);
        loop {
            tokio::select! {
                Some(step) = received.recv() => {
                    if output.send(Message::SupervisorProgress(progress_host.clone(), step)).await.is_err() {
                        break;
                    }
                }
                result = &mut setup => {
                    let _ = output.send(Message::SupervisorInstalled(host, result)).await;
                    break;
                }
            }
        }
    }))
}

impl App {
    fn theme_names(&self, color_scheme_kind: ColorSchemeKind) -> &Vec<String> {
        match color_scheme_kind {
            ColorSchemeKind::Dark => &self.theme_names_dark,
            ColorSchemeKind::Light => &self.theme_names_light,
        }
    }

    fn update_color_schemes(&mut self) {
        self.themes = terminal_theme::terminal_themes();
        for &color_scheme_kind in &[ColorSchemeKind::Dark, ColorSchemeKind::Light] {
            for (color_scheme_name, color_scheme_id) in
                self.config.color_scheme_names(color_scheme_kind)
            {
                if let Some(color_scheme) = self
                    .config
                    .color_schemes(color_scheme_kind)
                    .get(&color_scheme_id)
                    && self
                        .themes
                        .insert(
                            (color_scheme_name.clone(), color_scheme_kind),
                            color_scheme.into(),
                        )
                        .is_some()
                {
                    log::warn!(
                        "custom {:?} color scheme {:?} replaces builtin one",
                        color_scheme_kind,
                        color_scheme_name
                    );
                }
            }
        }

        self.theme_names_dark.clear();
        self.theme_names_light.clear();
        for (name, color_scheme_kind) in self.themes.keys() {
            match *color_scheme_kind {
                ColorSchemeKind::Dark => {
                    self.theme_names_dark.push(name.clone());
                }
                ColorSchemeKind::Light => {
                    self.theme_names_light.push(name.clone());
                }
            }
        }
        self.theme_names_dark
            .sort_by(|a, b| LANGUAGE_SORTER.compare(a, b));
        self.theme_names_light
            .sort_by(|a, b| LANGUAGE_SORTER.compare(a, b));
    }

    fn reset_terminal_panes_zoom(&mut self) {
        for (_pane, tab_model) in self.pane_model.panes.iter() {
            for entity in tab_model.iter() {
                if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                    let mut terminal = terminal.lock().unwrap();
                    terminal.set_zoom_adj(0);
                }
            }
        }
    }

    fn reset_active_pane_zoom(&mut self) {
        if let Some(tab_model) = self.pane_model.active() {
            for entity in tab_model.iter() {
                if tab_model.is_active(entity)
                    && let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity)
                {
                    terminal.lock().unwrap().set_zoom_adj(0);
                }
            }
        }
    }

    fn save_shortcuts_custom(&mut self) {
        self.config.shortcuts_custom = self.shortcuts_config.custom.clone();
        match &self.config_handler {
            Some(config_handler) => {
                if let Err(err) =
                    config_handler.set("shortcuts_custom", &self.config.shortcuts_custom)
                {
                    log::warn!("failed to save shortcuts custom config: {}", err);
                }
            }
            None => {
                log::warn!("failed to save shortcuts custom config: no config handler");
            }
        }
        self.key_binds = key_binds(&self.shortcuts_config);
    }

    fn apply_shortcut_binding(
        &mut self,
        binding: shortcuts::Binding,
        action: shortcuts::KeyBindAction,
    ) {
        self.shortcuts_config.custom.0.insert(binding, action);
        self.save_shortcuts_custom();
    }

    fn set_context_overlay(&mut self, overlay: bool) {
        if self.core.window.context_is_overlay != overlay {
            self.core.window.context_is_overlay = overlay;
            self.core.set_show_context(self.core.window.show_context);
        }
    }

    fn begin_shortcut_conflict(&mut self, conflict: ShortcutConflict) {
        if self.shortcut_conflict.is_none() {
            self.shortcut_conflict_overlay_restore = Some(self.core.window.context_is_overlay);
            self.set_context_overlay(false);
        }
        self.shortcut_conflict = Some(conflict);
    }

    fn clear_shortcut_conflict(&mut self) {
        self.shortcut_conflict = None;
        if let Some(overlay) = self.shortcut_conflict_overlay_restore.take() {
            self.set_context_overlay(overlay);
        }
    }

    fn shortcut_page_toggle(&mut self) {
        self.shortcut_capture = None;
        self.clear_shortcut_conflict();
        self.shortcut_search_focus
            .set(self.core.window.show_context);
        self.shortcut_search_regex = None;
        self.shortcut_search_value.clear();
    }

    fn update_config(&mut self) -> Task<Message> {
        self.refresh_host_names();
        self.accent = self.tab_accent();
        let theme = self.theme();

        // Update color schemes
        self.update_color_schemes();

        // Update terminal window background color
        {
            let color = Color::from(theme.cosmic().background(theme.transparent).base);
            let bytes = color.into_rgba8();
            let data = u32::from(bytes[2])
                | (u32::from(bytes[1]) << 8)
                | (u32::from(bytes[0]) << 16)
                | 0xFF000000;
            terminal::WINDOW_BG_COLOR.store(data, Ordering::SeqCst);
        }

        // Set config of all tabs
        let color_scheme_kind = self.config.color_scheme_kind(&theme);
        for (_pane, tab_model) in self.pane_model.panes.iter() {
            for entity in tab_model.iter() {
                if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                    let mut terminal = terminal.lock().unwrap();
                    terminal.set_config(&self.config, color_scheme_kind, &self.themes);
                }
            }
        }

        // Set headerbar state
        self.core.window.show_headerbar = self.config.show_headerbar;

        // Update application theme
        cosmic::command::set_theme(theme)
    }

    /// The colour of the tab in front of the focused pane, for the accent.
    /// A tab without one, such as a plain terminal's, gets a neutral accent
    /// rather than the desktop's, which may look like another tab's colour
    fn tab_accent(&self) -> Option<TabColor> {
        if !self.config.accent_follows_tab {
            return None;
        }
        let tab_model = self.pane_model.active()?;
        Some(
            tab_model
                .data::<TabMark>(tab_model.active())
                .and_then(|mark| mark.color)
                .unwrap_or(NEUTRAL_ACCENT),
        )
    }

    /// The configured theme, with the accent of [`Self::accent`]. It is a
    /// custom theme then, since a system one would take the desktop's accent
    fn theme(&self) -> cosmic::theme::Theme {
        let theme = self.config.app_theme.theme();
        match self.accent {
            Some(TabColor([r, g, b])) => {
                let accent = cosmic::cosmic_theme::palette::Srgba::new(
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    1.0,
                );
                let base = theme.cosmic();
                let mut tinted = base.with_accent(accent);
                if tinted.is_dark != base.is_dark {
                    // `with_accent` starts again from the desktop's saved
                    // builder, which can lack the very fields `AppTheme::theme`
                    // stood in for; COSMIC's own builder takes its place
                    use cosmic::cosmic_theme::palette::{IntoColor, Oklcha, Srgb};
                    let mut oklcha: Oklcha = accent.into_color();
                    let current: Oklcha = base.accent_color().into_color();
                    oklcha.l = current.l;
                    let accent: Srgb = oklcha.into_color();
                    let builder = if base.is_dark {
                        cosmic::cosmic_theme::ThemeBuilder::dark()
                    } else {
                        cosmic::cosmic_theme::ThemeBuilder::light()
                    };
                    tinted = builder.accent(accent).build();
                }
                cosmic::theme::Theme::custom(std::sync::Arc::new(tinted))
            }
            None => theme,
        }
    }

    /// Set the accent to the tab in front's colour, if that changed
    fn apply_theme(&mut self) -> Task<Message> {
        let accent = self.tab_accent();
        if accent == self.accent {
            return Task::none();
        }
        self.accent = accent;
        cosmic::command::set_theme(self.theme())
    }

    fn update_render_active_pane_zoom(&mut self, zoom_message: Message) -> Task<Message> {
        // skip writing config to fs when zoom in/ out
        // recalculate the pane due to the changes of zoom_adj value
        // but only for the active pane/tab
        let color_scheme_kind = self.config.color_scheme_kind(self.core.system_theme());
        if let Some(tab_model) = self.pane_model.active() {
            for entity in tab_model.iter() {
                if tab_model.is_active(entity)
                    && let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity)
                {
                    let mut terminal = terminal.lock().unwrap();
                    let current_zoom_adj = terminal.zoom_adj();
                    match zoom_message {
                        Message::ZoomIn => {
                            terminal.set_zoom_adj(current_zoom_adj.saturating_add(1))
                        }
                        Message::ZoomOut => {
                            terminal.set_zoom_adj(current_zoom_adj.saturating_sub(1))
                        }
                        _ => {}
                    }
                    terminal.set_config(&self.config, color_scheme_kind, &self.themes);
                }
            }
        }
        Task::none()
    }

    fn save_color_schemes(&mut self, color_scheme_kind: ColorSchemeKind) -> Task<Message> {
        // Optimized for just saving color_schemes
        if let Some(ref config_handler) = self.config_handler
            && let Err(err) = config_handler.set(
                match color_scheme_kind {
                    ColorSchemeKind::Dark => "color_schemes_dark",
                    ColorSchemeKind::Light => "color_schemes_light",
                },
                self.config.color_schemes(color_scheme_kind),
            )
        {
            log::error!("failed to save config: {}", err);
        }
        self.update_color_schemes();
        Task::none()
    }

    /// Save the machines, and the profiles, whose hosts follow them
    fn save_machines(&mut self) -> Task<Message> {
        self.config.adopt_machines();
        self.refresh_host_names();
        if let Some(ref config_handler) = self.config_handler
            && let Err(err) = config_handler.set("machines", &self.config.machines)
        {
            log::error!("failed to save machines: {}", err);
        }
        self.save_profiles()
    }

    /// What profiles' machine dropdowns offer
    fn refresh_host_names(&mut self) {
        self.host_names = std::iter::once(fl!("this-computer"))
            .chain(self.config.machines.values().map(|m| m.name.clone()))
            .collect();
    }

    fn save_profiles(&mut self) -> Task<Message> {
        // Optimized for just saving profiles
        if let Some(ref config_handler) = self.config_handler {
            match config_handler.set("profiles", &self.config.profiles) {
                Ok(()) => {}
                Err(err) => {
                    log::error!("failed to save config: {}", err);
                }
            }
        }
        Task::none()
    }

    fn update_focus(&self) -> Task<Message> {
        // Typing in the sidebar's filter, the Secrets pane or a dialog keeps
        // the focus there
        if self.rename.is_some()
            || self.secrets_focus.is_some()
            || (self.sidebar.searching && !self.new_tab_menu_open)
        {
            return Task::none();
        }
        let launcher = self
            .pane_model
            .active()
            .and_then(|tab_model| tab_model.data::<Launcher>(tab_model.active()));
        if self.new_tab_menu_open {
            Task::none()
        } else if let Some(launcher) = launcher
            && !self.core.window.show_context
            && self.dialog_opt.is_none()
        {
            // A form's fields keep the focus they have
            if launcher.form.is_some() {
                Task::none()
            } else {
                widget::text_input::focus(launcher.filter_id.clone())
            }
        } else if self.find {
            widget::text_input::focus(self.find_search_id.clone())
        } else if self.core.window.show_context {
            // Right now we only care about the KeyboardShortcuts context page, so we use a simple if.
            // In the future if we are to care about other conext pages, we could switch this to a match
            // statement instead to be cleaner.
            if self.context_page == ContextPage::KeyboardShortcuts
                && self.shortcut_search_focus.get()
            {
                self.shortcut_search_focus.set(false);
                return widget::text_input::focus(self.shortcut_search_id.clone());
            }

            Task::none()
        } else if let Some(thread) = self.active_thread() {
            // The pane's terminal isn't drawn behind a thread: focusing its
            // id would only take the focus from the composer
            widget::text_input::focus(thread.composer_id.clone())
        } else if let Some(terminal_id) = self.terminal_ids.get(&self.pane_model.focused()).cloned()
        {
            widget::text_input::focus(terminal_id)
        } else {
            Task::none()
        }
    }

    // Call this any time the tab changes
    fn update_title(&mut self, pane: Option<pane_grid::Pane>) -> Task<Message> {
        let pane = pane.unwrap_or(self.pane_model.focused());
        if let Some(tab_model) = self.pane_model.panes.get(pane) {
            let (header_title, window_title) = match tab_model.text(tab_model.active()) {
                Some(tab_title) => (
                    tab_title.to_string(),
                    format!("{tab_title} — {}", fl!("cosmic-terminal")),
                ),
                None => (String::new(), fl!("cosmic-terminal")),
            };
            self.set_header_title(header_title);
            Task::batch([
                if let Some(window_id) = self.core.main_window_id() {
                    self.set_window_title(window_title, window_id)
                } else {
                    Task::none()
                },
                self.update_focus(),
                self.apply_theme(),
            ])
        } else {
            log::error!("Failed to get the specific pane");
            Task::batch([
                if let Some(window_id) = self.core.main_window_id() {
                    self.set_window_title(fl!("cosmic-terminal"), window_id)
                } else {
                    Task::none()
                },
                self.update_focus(),
            ])
        }
    }

    fn set_curr_font_weights_and_stretches(&mut self) {
        // check if config font_name is available first, if not, set it to first name in list
        if !self.font_names.contains(&self.config.font_name) {
            log::error!("'{}' is not in the font list", self.config.font_name);
            log::error!("setting font name to '{}'", self.font_names[0]);
            let _ = self.update(Message::DefaultFont(0));
        }

        let curr_font_faces = &self.font_name_faces_map[&self.config.font_name];

        self.curr_font_stretches = curr_font_faces
            .iter()
            .map(|face| face.stretch)
            .collect::<BTreeSet<_>>() // remove duplicates and sort
            .into_iter()
            .collect();

        self.curr_font_stretch_names = self
            .curr_font_stretches
            .iter()
            .map(|stretch| &self.all_font_stretches_vals_names_map[stretch])
            .cloned()
            .collect::<Vec<_>>();

        if !self
            .curr_font_stretches
            .contains(&self.config.typed_font_stretch())
        {
            self.config.font_stretch = Stretch::Normal.to_number();
        }

        let curr_weights = |conf_stretch| {
            curr_font_faces
                .iter()
                .filter(|face| face.stretch == conf_stretch)
                .map(|face| face.weight.0)
                .collect::<BTreeSet<_>>() // remove duplicates and sort
                .into_iter()
                .collect()
        };

        self.curr_font_weights = curr_weights(self.config.typed_font_stretch());

        if self.curr_font_weights.is_empty() {
            // stretch fallback
            self.config.font_stretch = Stretch::Normal.to_number();
        }

        self.curr_font_weights = curr_weights(self.config.typed_font_stretch());
        assert!(!self.curr_font_weights.is_empty());

        self.curr_font_weight_names = self
            .curr_font_weights
            .iter()
            .map(|weight| &self.all_font_weights_vals_names_map[weight])
            .cloned()
            .collect::<Vec<_>>();

        if !self.curr_font_weights.contains(&self.config.font_weight) {
            self.config.font_weight = Weight::NORMAL.0;
        }

        if !self
            .curr_font_weights
            .contains(&self.config.dim_font_weight)
        {
            self.config.dim_font_weight = Weight::NORMAL.0;
        }

        if !self
            .curr_font_weights
            .contains(&self.config.bold_font_weight)
        {
            self.config.bold_font_weight = Weight::BOLD.0;
        }
    }

    fn color_schemes(&self, color_scheme_kind: ColorSchemeKind) -> Element<'_, Message> {
        let cosmic_theme::Spacing { space_xxxs, .. } = self.core().system_theme().cosmic().spacing;

        let mut sections = Vec::with_capacity(3 + self.color_scheme_errors.len());

        sections.push(
            widget::tab_bar::horizontal(&self.color_scheme_tab_model)
                .on_activate(Message::ColorSchemeTabActivate)
                .into(),
        );

        let mut section = widget::settings::section();
        let builtin_name = format!("COSMIC {:?}", color_scheme_kind);
        let color_scheme_names = self.config.color_scheme_names(color_scheme_kind);
        for (color_scheme_name, color_scheme_id_opt) in std::iter::once((builtin_name, None)).chain(
            color_scheme_names
                .into_iter()
                .map(|(name, id)| (name, Some(id))),
        ) {
            let expanded =
                self.color_scheme_expanded == Some((color_scheme_kind, color_scheme_id_opt));
            let renaming = match &self.color_scheme_renaming {
                Some((kind, id, value))
                    if kind == &color_scheme_kind && Some(id) == color_scheme_id_opt.as_ref() =>
                {
                    Some(value)
                }
                _ => None,
            };

            let button = if expanded {
                widget::button::custom(icon_cache_get("view-more-symbolic", 16))
                    .on_press(Message::ColorSchemeCollapse)
            } else {
                widget::button::custom(icon_cache_get("view-more-symbolic", 16)).on_press(
                    Message::ColorSchemeExpand(color_scheme_kind, color_scheme_id_opt),
                )
            }
            .class(style::Button::Icon);

            let mut popover = widget::popover(button);
            if expanded {
                let menu = menu::color_scheme_menu(
                    color_scheme_kind,
                    color_scheme_id_opt,
                    &color_scheme_name,
                );
                popover = popover
                    .popup(menu)
                    .position(widget::popover::Position::Bottom);
            }

            let item = match renaming {
                Some(value) => widget::settings::item_row(vec![
                    widget::text_input("", value)
                        .id(self.color_scheme_rename_id.clone())
                        .on_input(move |value| {
                            Message::ColorSchemeRename(
                                color_scheme_kind,
                                color_scheme_id_opt.expect("trying to rename builtin color scheme"),
                                value,
                            )
                        })
                        .on_submit(|_| Message::ColorSchemeRenameSubmit)
                        .into(),
                    popover.into(),
                ]),
                None => widget::settings::item::builder(color_scheme_name).control(popover),
            };
            section = section.add(item);
        }
        sections.push(section.into());

        sections.push(
            widget::row::with_children(vec![
                widget::space::horizontal().into(),
                widget::button::standard(fl!("import"))
                    .on_press_maybe(self.color_scheme_io.is_none().then_some(Message::ColorSchemeImport(color_scheme_kind)))
                    .into(),
            ])
            .into(),
        );
        if let Some(status) = &self.color_scheme_io {
            sections.push(widget::text::caption(status.clone()).into());
            if self.color_scheme_io_slow {
                sections.push(widget::text::caption(fl!("color-scheme-waiting")).into());
            }
        }

        for error in &self.color_scheme_errors {
            sections.push(
                widget::row::with_children(vec![
                    icon_cache_get("dialog-error-symbolic", 16)
                        .class(style::Svg::Custom(Rc::new(|theme| {
                            let cosmic = theme.cosmic();
                            widget::svg::Style {
                                color: Some(cosmic.destructive_text_color().into()),
                            }
                        })))
                        .into(),
                    widget::text::body(error)
                        .class(style::Text::Custom(|theme| {
                            let cosmic = theme.cosmic();
                            //TODO: re-export in libcosmic
                            iced::widget::text::Style {
                                color: Some(cosmic.destructive_text_color().into()),
                                ..Default::default()
                            }
                        }))
                        .into(),
                ])
                .spacing(space_xxxs)
                .into(),
            );
        }

        widget::settings::view_column(sections).into()
    }

    fn keyboard_shortcuts(&self) -> Element<'_, Message> {
        let cosmic_theme::Spacing {
            space_xxs,
            space_s,
            space_m,
            space_l,
            space_xl,
            ..
        } = self.core().system_theme().cosmic().spacing;

        let pad_action = [space_xxs, space_m];
        let div_action = space_s;
        let pad_binding = [space_xxs, space_xl];
        let div_binding = space_l;

        let mut groups = Vec::new();
        //TODO: fix text input focus going outside bounds
        groups.push(widget::space::horizontal().into());
        groups.push(
            widget::text_input::search_input(fl!("type-to-search"), &self.shortcut_search_value)
                .id(self.shortcut_search_id.clone())
                .on_input(Message::ShortcutSearch)
                .into(),
        );

        for group in shortcuts::shortcut_groups() {
            let mut list = widget::list::list_column();

            let mut found_actions = false;
            for action in group.actions {
                let action_label = shortcuts::action_label(action);
                if let Some(regex) = &self.shortcut_search_regex
                    && regex.find(&action_label).is_none()
                {
                    continue;
                }
                found_actions = true;

                let (bindings, changed) = self.shortcuts_config.bindings_for_action(action);

                let mut buttons = widget::row::with_capacity(2);
                if changed {
                    buttons = buttons.push(widget::tooltip(
                        widget::button::custom(icon_cache_get("edit-undo-symbolic", 16))
                            .class(style::Button::Icon)
                            .on_press(Message::ShortcutReset(action)),
                        widget::text::body(fl!("reset-to-default")),
                        widget::tooltip::Position::Top,
                    ));
                }
                buttons = buttons.push(widget::tooltip(
                    widget::button::custom(icon_cache_get("list-add-symbolic", 16))
                        .class(style::Button::Icon)
                        .on_press(Message::ShortcutCaptureStart(action)),
                    widget::text::body(fl!("add-another-keybinding")),
                    widget::tooltip::Position::Top,
                ));

                list = list.list_item_padding(pad_action);
                list = list.divider_padding(div_action);
                list = list.add(widget::settings::item_row(vec![
                    widget::text::heading(action_label)
                        .width(Length::Fill)
                        .into(),
                    buttons.into(),
                ]));

                if bindings.is_empty() {
                    list = list.list_item_padding(pad_binding);
                    list = list.add(widget::text::body(fl!("no-shortcuts")));
                    list = list.divider_padding(div_binding);
                } else {
                    for resolved in bindings {
                        list = list.list_item_padding(pad_binding);
                        list = list.add(
                            widget::settings::item::builder(shortcuts::binding_display(
                                &resolved.binding,
                            ))
                            .control(
                                widget::button::custom(icon_cache_get("edit-delete-symbolic", 16))
                                    .class(style::Button::Icon)
                                    .on_press(Message::ShortcutRemove(
                                        resolved.binding.clone(),
                                        resolved.source,
                                    )),
                            ),
                        );
                        list = list.divider_padding(div_binding);
                    }
                }

                if self.shortcut_capture == Some(action) {
                    list = list.list_item_padding(pad_binding);
                    list = list.add(
                        widget::settings::item_row(vec![
                            widget::text::body(fl!("shortcut-capture-hint"))
                                .width(Length::Fill)
                                .into(),
                            widget::button::text(fl!("cancel"))
                                .on_press(Message::ShortcutCaptureCancel)
                                .into(),
                        ])
                        .spacing(space_xxs),
                    );
                    list = list.divider_padding(div_binding);
                }
            }

            if found_actions {
                groups.push(
                    widget::settings::section::with_column(list)
                        .title(group.title)
                        .into(),
                );
            }
        }

        widget::settings::view_column(groups).into()
    }

    fn profiles(&self) -> Element<'_, Message> {
        let cosmic_theme::Spacing {
            space_s,
            space_xs,
            space_xxs,
            space_xxxs,
            ..
        } = self.core().system_theme().cosmic().spacing;

        let mut sections = Vec::with_capacity(4);
        sections.push(self.machines_view());

        if !self.config.profiles.is_empty() {
            let mut profiles_section = widget::settings::section().title(fl!("profiles"));
            for (profile_name, profile_id) in self.config.profile_names() {
                let Some(profile) = self.config.profiles.get(&profile_id) else {
                    continue;
                };

                let expanded = self.profile_expanded == Some(profile_id);

                profiles_section = profiles_section.add(
                    widget::settings::item::builder(profile_name).control(
                        widget::row::with_children(vec![
                            widget::button::custom(icon_cache_get("edit-delete-symbolic", 16))
                                .on_press(Message::ProfileRemove(profile_id))
                                .class(style::Button::Icon)
                                .into(),
                            if expanded {
                                widget::button::custom(icon_cache_get("go-up-symbolic", 16))
                                    .on_press(Message::ProfileCollapse(profile_id))
                            } else {
                                widget::button::custom(icon_cache_get("go-down-symbolic", 16))
                                    .on_press(Message::ProfileExpand(profile_id))
                            }
                            .class(style::Button::Icon)
                            .into(),
                        ])
                        .align_y(Alignment::Center)
                        .spacing(space_xxs),
                    ),
                );

                if expanded {
                    let dark_selected = self
                        .theme_names_dark
                        .iter()
                        .position(|theme_name| theme_name == &profile.syntax_theme_dark);
                    let light_selected = self
                        .theme_names_light
                        .iter()
                        .position(|theme_name| theme_name == &profile.syntax_theme_light);

                    let expanded_section = widget::settings::section()
                        .add(
                            widget::column::with_children(vec![
                                widget::column::with_children(vec![
                                    widget::text(fl!("name")).into(),
                                    widget::text_input("", &profile.name)
                                        .on_input(move |text| {
                                            Message::ProfileName(profile_id, text)
                                        })
                                        .on_paste(move |text| {
                                            Message::ProfileName(profile_id, text)
                                        })
                                        .into(),
                                ])
                                .spacing(space_xxxs)
                                .into(),
                                widget::column::with_children(vec![
                                    widget::text(fl!("agent")).into(),
                                    widget::dropdown(
                                        &self.agent_names,
                                        config::Agent::ALL
                                            .iter()
                                            .position(|agent| *agent == profile.agent),
                                        move |agent_i| Message::ProfileAgent(profile_id, agent_i),
                                    )
                                    .into(),
                                ])
                                .spacing(space_xxxs)
                                .into(),
                                widget::column::with_children(vec![
                                    widget::text(fl!("machine")).into(),
                                    widget::dropdown(
                                        &self.host_names,
                                        match profile.machine {
                                            None => Some(0),
                                            Some(machine) => self
                                                .config
                                                .machines
                                                .keys()
                                                .position(|id| *id == machine)
                                                .map(|i| i + 1),
                                        },
                                        move |i| Message::ProfileMachine(profile_id, i),
                                    )
                                    .into(),
                                    widget::text::caption(fl!("machine-description")).into(),
                                ])
                                .spacing(space_xxxs)
                                .into(),
                                widget::column::with_children(vec![
                                    widget::text(fl!("tab-color")).into(),
                                    widget::dropdown(
                                        &self.color_names,
                                        if profile.color.is_empty() {
                                            Some(0)
                                        } else {
                                            TabColor::parse(&profile.color).and_then(|color| {
                                                TabColor::PALETTE
                                                    .iter()
                                                    .position(|(_, hex)| {
                                                        TabColor::parse(hex) == Some(color)
                                                    })
                                                    .map(|i| i + 1)
                                            })
                                        },
                                        move |color_i| Message::ProfileColor(profile_id, color_i),
                                    )
                                    .into(),
                                ])
                                .spacing(space_xxxs)
                                .into(),
                                if profile.agent == config::Agent::Shell {
                                    widget::column::with_children(vec![
                                        widget::text(fl!("command-line")).into(),
                                        widget::text_input("", &profile.command)
                                            .on_input(move |text| {
                                                Message::ProfileCommand(profile_id, text)
                                            })
                                            .on_paste(move |text| {
                                                Message::ProfileCommand(profile_id, text)
                                            })
                                            .into(),
                                    ])
                                    .spacing(space_xxxs)
                                    .into()
                                } else {
                                    let mut children = vec![
                                        widget::text(fl!("agent-arguments")).into(),
                                        widget::text_input("", &profile.args)
                                            .on_input(move |text| {
                                                Message::ProfileArgs(profile_id, text)
                                            })
                                            .on_paste(move |text| {
                                                Message::ProfileArgs(profile_id, text)
                                            })
                                            .into(),
                                    ];
                                    // Agents' conversations can be rich-text threads
                                    if profile.agent != config::Agent::Shell {
                                        children.push(
                                            widget::toggler(profile.thread)
                                                .label(fl!("profile-thread"))
                                                .on_toggle(move |on| Message::ProfileThread(profile_id, on))
                                                .into(),
                                        );
                                    }
                                    widget::column::with_children(children)
                                    .spacing(space_xxxs)
                                    .into()
                                },
                                widget::column::with_children(vec![
                                    widget::text(fl!("working-directory")).into(),
                                    widget::text_input("", &profile.working_directory)
                                        .on_input(move |text| {
                                            Message::ProfileDirectory(profile_id, text)
                                        })
                                        .on_paste(move |text| {
                                            Message::ProfileDirectory(profile_id, text)
                                        })
                                        .into(),
                                ])
                                .spacing(space_xxxs)
                                .into(),
                                widget::column::with_children(vec![
                                    widget::text(fl!("tab-title")).into(),
                                    widget::text_input("", &profile.tab_title)
                                        .on_input(move |text| {
                                            Message::ProfileTabTitle(profile_id, text)
                                        })
                                        .on_paste(move |text| {
                                            Message::ProfileTabTitle(profile_id, text)
                                        })
                                        .into(),
                                    widget::text::caption(fl!("tab-title-description")).into(),
                                ])
                                .spacing(space_xxxs)
                                .into(),
                            ])
                            .padding([0, space_s])
                            .spacing(space_xs),
                        )
                        .add(
                            //TODO: rename to color-scheme-dark?
                            widget::settings::item::builder(fl!("syntax-dark")).control(
                                widget::dropdown::popup_dropdown(
                                    &self.theme_names_dark,
                                    dark_selected,
                                    move |theme_i| {
                                        Message::ProfileSyntaxTheme(
                                            profile_id,
                                            ColorSchemeKind::Dark,
                                            theme_i,
                                        )
                                    },
                                    self.core.main_window_id().unwrap_or(window::Id::RESERVED),
                                    Message::Surface,
                                    |a| a,
                                ),
                            ),
                        )
                        .add(
                            //TODO: rename to color-scheme-light?
                            widget::settings::item::builder(fl!("syntax-light")).control(
                                widget::dropdown(
                                    &self.theme_names_light,
                                    light_selected,
                                    move |theme_i| {
                                        Message::ProfileSyntaxTheme(
                                            profile_id,
                                            ColorSchemeKind::Light,
                                            theme_i,
                                        )
                                    },
                                ),
                            ),
                        )
                        .add(
                            widget::settings::item::builder(fl!("make-default")).control(
                                widget::toggler(
                                    self.get_default_profile().is_some_and(|p| p == profile_id),
                                )
                                .on_toggle(move |t| Message::UpdateDefaultProfile((t, profile_id))),
                            ),
                        )
                        .add(
                            widget::row::with_children(vec![
                                widget::column::with_children(vec![
                                    widget::text(fl!("hold")).into(),
                                    widget::text::caption(fl!("remain-open")).into(),
                                ])
                                .spacing(space_xxxs)
                                .into(),
                                widget::space::horizontal().into(),
                                widget::toggler(profile.drain_on_exit)
                                    .on_toggle(move |t| Message::ProfileHold(profile_id, t))
                                    .into(),
                            ])
                            .align_y(Alignment::Center)
                            .padding([0, space_s]),
                        );

                    let padding = Padding {
                        top: 0.0,
                        bottom: 0.0,
                        left: space_s.into(),
                        right: space_s.into(),
                    };
                    profiles_section =
                        profiles_section.add(widget::container(expanded_section).padding(padding))
                }
            }
            sections.push(profiles_section.into());
        }

        let add_profile = widget::row::with_children(vec![
            widget::space::horizontal().into(),
            widget::button::standard(fl!("add-profile"))
                .on_press(Message::ProfileNew)
                .into(),
        ]);
        sections.push(add_profile.into());

        widget::settings::view_column(sections).into()
    }

    /// One bottom toast per pending command, removed when it finishes.
    fn start_progress(&mut self, key: &'static str, message: String) {
        let toast = widget::toaster::Toast::new(message);
        if let Some(&id) = self.pending_commands.get(key) {
            self.toasts.update(id, toast);
        } else {
            let id = self.toasts.push_persistent(toast);
            self.pending_commands.insert(key, id);
        }
    }

    fn finish_progress(&mut self, key: &'static str) {
        if let Some(id) = self.pending_commands.remove(key) {
            self.toasts.remove(id);
        }
    }

    /// Update setup progress in place; a user-dismissed toast stays dismissed.
    fn host_progress(&mut self, host: &str) {
        let Some(step) = self.host_setup.get(host) else { return; };
        let machine = if host.is_empty() {
            fl!("this-computer")
        } else {
            self.config.machine_for_host(host).map_or_else(|| host.to_string(), |m| m.name.clone())
        };
        let (detail, progress) = match step {
            None => (fl!("host-setup-tools"), 0.0),
            Some(agent::Helper::Supervise) => (fl!("host-setup-supervisor"), 0.2),
            Some(agent::Helper::Transfer) => (fl!("host-setup-transfer"), 0.4),
            Some(agent::Helper::PamModule | agent::Helper::SudoAgent) => (fl!("host-setup-sudo"), 0.6),
            Some(agent::Helper::Zellij) => (fl!("host-setup-zellij"), 0.75),
            Some(agent::Helper::Chartgen) => (fl!("host-setup-chartgen"), 0.9),
        };
        let mut message = fl!("host-setup", host = machine, step = detail);
        if self.host_setup_stalled.contains(host) {
            message.push('\n');
            message.push_str(&fl!("host-setup-waiting"));
        }
        let toast = widget::toaster::Toast::new(message).progress(progress);
        if let Some(&id) = self.host_toasts.get(host) {
            self.toasts.update(id, toast);
        } else {
            let id = self.toasts.push_persistent(toast);
            self.host_toasts.insert(host.to_string(), id);
        }
    }

    fn mount_connection_result(&mut self, host: String, result: Result<(), String>) -> Task<Message> {
        match result {
            Ok(()) => { self.mount_errors.remove(&host); Task::none() }
            Err(err) => {
                log::warn!("{host} and this computer cannot mount each other: {err}");
                self.mount_errors.insert(host.clone(), err);
                let machine = self.config.machine_for_host(&host).map_or_else(|| host.clone(), |m| m.name.clone());
                self.toasts.push(widget::toaster::Toast::new(fl!("remote-files-connect-failed", machine = machine.as_str()))
                    .action(fl!("remote-files-help"), move |_| Message::RemoteFilesOpen(host.clone())))
                    .map(cosmic::Action::App)
            }
        }
    }

    fn file_open_error(&mut self, title: String, error: String) -> Task<Message> {
        let mut toast = widget::toaster::Toast::new(title);
        if error.contains("ct-mount:") || error.contains("sshfs") {
            self.mount_errors.insert(String::new(), error);
            self.mount_setup.remove("");
            toast = toast.action(fl!("remote-files-help"), |_| Message::RemoteFilesOpen(String::new()));
        }
        self.toasts.push(toast).map(cosmic::Action::App)
    }

    fn mount_setup_machines(&self) -> Vec<(String, String)> {
        let mut names = BTreeMap::new();
        names.insert(String::new(), self.local.as_ref().map_or_else(
            || fl!("this-computer"),
            |local| fl!("this-computer-named", name = local.hostname.as_str()),
        ));
        for host in self.session_hosts().into_iter().filter(|host| !host.is_empty()) {
            names.insert(host.clone(), host);
        }
        for machine in self.config.machines.values() {
            names.insert(machine.destination(), machine.name.clone());
        }
        for host in self.mount_errors.keys().chain(self.mount_setup.keys()) {
            names.entry(host.clone()).or_insert_with(|| host.clone());
        }
        names.into_iter().collect()
    }

    /// The machines part of the profiles page
    fn machines_view(&self) -> Element<'_, Message> {
        let cosmic_theme::Spacing {
            space_s,
            space_xs,
            space_xxs,
            space_xxxs,
            ..
        } = self.core().system_theme().cosmic().spacing;
        let mut section = widget::settings::section().title(fl!("machines"));
        for (&id, machine) in &self.config.machines {
            let expanded = self.machine_expanded == Some(id);
            let mut detail = machine.destination();
            if !machine.hostname.is_empty() && machine.hostname != machine.name {
                detail = fl!("machine-detail", destination = detail, hostname = machine.hostname.as_str());
            }
            section = section.add(
                widget::settings::item::builder(machine.name.clone())
                    .description(detail)
                    .control(
                        widget::row::with_children(vec![
                            widget::button::custom(icon_cache_get("edit-delete-symbolic", 16))
                                .on_press(Message::MachineRemove(id))
                                .class(style::Button::Icon)
                                .into(),
                            if expanded {
                                widget::button::custom(icon_cache_get("go-up-symbolic", 16))
                                    .on_press(Message::MachineCollapse(id))
                            } else {
                                widget::button::custom(icon_cache_get("go-down-symbolic", 16))
                                    .on_press(Message::MachineExpand(id))
                            }
                            .class(style::Button::Icon)
                            .into(),
                        ])
                        .align_y(Alignment::Center)
                        .spacing(space_xxs),
                    ),
            );
            if !expanded {
                continue;
            }
            let field = |label: String, value: &str, which: MachineField, help: Option<String>| -> Element<'_, Message> {
                let mut column = widget::column::with_capacity(3)
                    .spacing(space_xxxs)
                    .push(widget::text(label))
                    .push(
                        widget::text_input("", value.to_string())
                            .on_input(move |text| Message::MachineEdit(id, which, text))
                            .on_paste(move |text| Message::MachineEdit(id, which, text)),
                    );
                if let Some(help) = help {
                    column = column.push(widget::text::caption(help));
                }
                column.into()
            };
            let status = match self.machine_checks.get(&id) {
                None => None,
                Some(MachineCheck::Running) => Some(fl!("machine-checking")),
                Some(MachineCheck::Done(Ok(probe))) => Some(self.probe_summary(id, probe)),
                Some(MachineCheck::Done(Err(error))) => Some(error.clone()),
            };
            let checking = matches!(self.machine_checks.get(&id), Some(MachineCheck::Running));
            let mut buttons = vec![
                widget::button::standard(fl!("machine-check"))
                    .on_press_maybe((!checking).then_some(Message::MachineCheck(id)))
                    .into(),
            ];
            if !self.config.profiles.values().any(|p| p.machine == Some(id)) {
                buttons.push(
                    widget::button::standard(fl!("machine-add-profiles", name = machine.name.as_str()))
                        .on_press(Message::MachineAddProfiles(id))
                        .into(),
                );
            }
            let mut column = widget::column::with_capacity(6)
                .spacing(space_xs)
                .padding([0, space_s])
                .push(field(fl!("name"), &machine.name, MachineField::Name, None))
                .push(field(fl!("machine-host"), &machine.host, MachineField::Host, Some(fl!("machine-host-description"))))
                .push(field(fl!("machine-user"), &machine.user, MachineField::User, Some(fl!("machine-user-description"))));
            if let Some(status) = status {
                column = column.push(widget::text::caption(status));
            }
            column = column.push(widget::row::with_children(buttons).spacing(space_xs));
            section = section.add(column);
        }

        let mut column = widget::column::with_capacity(5).spacing(space_xs).push(section)
            .push(widget::button::standard(fl!("remote-files-title"))
                .on_press(Message::RemoteFilesOpen(String::new())));
        match &self.discovery {
            None => {
                column = column.push(widget::row::with_children(vec![
                    widget::space::horizontal().into(),
                    widget::button::standard(fl!("machine-add")).on_press(Message::MachineAdd).into(),
                ]));
            }
            Some(found) => {
                let mut list = widget::settings::section().title(fl!("machine-found"));
                match found {
                    None => list = list.add(widget::text::caption(fl!("machine-looking"))),
                    Some(Err(error)) => list = list.add(widget::text::caption(error.clone())),
                    Some(Ok(candidates)) if candidates.is_empty() => {
                        list = list.add(widget::text::caption(fl!("machine-none-found")));
                    }
                    Some(Ok(candidates)) => {
                        for (i, candidate) in candidates.iter().enumerate() {
                            list = list.add(
                                widget::settings::item::builder(candidate.name.clone())
                                    .description(self.candidate_detail(candidate))
                                    .control(
                                        widget::button::standard(fl!("machine-add-this"))
                                            .on_press(Message::MachineAddCandidate(i)),
                                    ),
                            );
                        }
                    }
                }
                column = column.push(list).push(widget::row::with_children(vec![
                    widget::space::horizontal().into(),
                    widget::button::standard(fl!("cancel")).on_press(Message::MachineDiscoveryClose).into(),
                    widget::button::standard(fl!("machine-add-blank")).on_press(Message::MachineAddBlank).into(),
                ]).spacing(space_xs));
            }
        }
        column.into()
    }

    /// One line on what a machine's check found
    fn probe_summary(&self, id: MachineId, probe: &machines::Probe) -> String {
        let this = self.local.as_ref().is_some_and(|local| {
            (!probe.host_id.is_empty() && probe.host_id == local.host_id)
                || (probe.hostname == local.hostname && probe.home == local.home)
        });
        let mut text = fl!(
            "machine-checked",
            user = probe.user.as_str(),
            hostname = probe.hostname.as_str(),
            home = probe.home.as_str()
        );
        if this {
            text.push(' ');
            text.push_str(&fl!("machine-is-this-computer"));
        } else if let Some((_, other)) = self
            .config
            .machines
            .iter()
            .find(|(other, m)| **other != id && !probe.host_id.is_empty() && m.host_id == probe.host_id)
        {
            text.push(' ');
            text.push_str(&fl!("machine-same-as", name = other.name.as_str()));
        }
        text
    }

    fn candidate_detail(&self, candidate: &machines::Candidate) -> String {
        let mut parts = vec![candidate.host.clone()];
        if !candidate.user.is_empty() {
            parts[0] = format!("{}@{}", candidate.user, candidate.host);
        }
        for source in &candidate.sources {
            parts.push(match source {
                machines::Source::Tailscale => fl!("machine-from-tailscale"),
                machines::Source::SshConfig => fl!("machine-from-ssh-config"),
            });
        }
        if candidate.online == Some(false) {
            parts.push(fl!("machine-offline"));
        }
        parts.join(" · ")
    }

    fn settings(&self) -> Element<'_, Message> {
        let t = self.core().system_theme();
        let app_theme_selected = match self.config.app_theme {
            AppTheme::Dark => 1,
            AppTheme::Light => 2,
            AppTheme::System => 0,
        };
        let dark_selected = self
            .theme_names_dark
            .iter()
            .position(|theme_name| theme_name == &self.config.syntax_theme_dark);
        let light_selected = self
            .theme_names_light
            .iter()
            .position(|theme_name| theme_name == &self.config.syntax_theme_light);
        let font_selected = {
            let mut font_system = font_system().write().unwrap();
            let current_font_name = font_system.raw().db().family_name(&Family::Monospace);
            self.font_names
                .iter()
                .position(|font_name| font_name == current_font_name)
        };
        let font_size_selected = self
            .font_sizes
            .iter()
            .position(|font_size| font_size == &self.config.font_size);
        let font_stretch_selected = self
            .curr_font_stretches
            .iter()
            .position(|font_stretch| font_stretch == &self.config.typed_font_stretch());
        let font_weight_selected = self
            .curr_font_weights
            .iter()
            .position(|font_weight| font_weight == &self.config.font_weight);
        let dim_font_weight_selected = self
            .curr_font_weights
            .iter()
            .position(|font_weight| font_weight == &self.config.dim_font_weight);
        let bold_font_weight_selected = self
            .curr_font_weights
            .iter()
            .position(|font_weight| font_weight == &self.config.bold_font_weight);
        let zoom_step_selected = self
            .zoom_steps
            .iter()
            .position(|zoom_step| zoom_step == &self.config.font_size_zoom_step_mul_100);

        let appearance_section = widget::settings::section()
            .title(fl!("appearance"))
            .add(
                widget::settings::item::builder(fl!("theme")).control(widget::dropdown(
                    &self.app_themes,
                    Some(app_theme_selected),
                    move |index| {
                        Message::AppTheme(match index {
                            1 => AppTheme::Dark,
                            2 => AppTheme::Light,
                            _ => AppTheme::System,
                        })
                    },
                )),
            )
            .add(
                widget::settings::item::builder(fl!("accent-follows-tab"))
                    .description(fl!("accent-follows-tab-description"))
                    .toggler(self.config.accent_follows_tab, Message::AccentFollowsTab),
            )
            .add(
                //TODO: rename to color-scheme-dark?
                widget::settings::item::builder(fl!("syntax-dark")).control(widget::dropdown(
                    &self.theme_names_dark,
                    dark_selected,
                    move |index| Message::SyntaxTheme(ColorSchemeKind::Dark, index),
                )),
            )
            .add(
                //TODO: rename to color-scheme-light?
                widget::settings::item::builder(fl!("syntax-light")).control(widget::dropdown(
                    &self.theme_names_light,
                    light_selected,
                    move |index| Message::SyntaxTheme(ColorSchemeKind::Light, index),
                )),
            )
            .add(
                widget::settings::item::builder(fl!("default-zoom-step")).control(
                    widget::dropdown(&self.zoom_step_names, zoom_step_selected, |index| {
                        Message::DefaultZoomStep(index)
                    }),
                ),
            )
            .add_maybe((!t.transparent).then(|| {
                widget::settings::item::builder(fl!("opacity"))
                    .description(format!("{}%", self.config.opacity))
                    .control(widget::slider(0..=100, self.config.opacity, |opacity| {
                        Message::Opacity(opacity)
                    }))
            }));

        let mut font_section = widget::settings::section()
            .title(fl!("font"))
            .add(
                widget::settings::item::builder(fl!("default-font")).control(widget::dropdown(
                    &self.font_names,
                    font_selected,
                    Message::DefaultFont,
                )),
            )
            .add(
                widget::settings::item::builder(fl!("default-font-size")).control(
                    widget::dropdown(&self.font_size_names, font_size_selected, |index| {
                        Message::DefaultFontSize(index)
                    }),
                ),
            )
            .add(
                widget::settings::item::builder(fl!("advanced-font-settings")).control(
                    if self.show_advanced_font_settings {
                        widget::button::custom(icon_cache_get("go-up-symbolic", 16))
                            .on_press(Message::ShowAdvancedFontSettings(false))
                    } else {
                        widget::button::custom(icon_cache_get("go-down-symbolic", 16))
                            .on_press(Message::ShowAdvancedFontSettings(true))
                    }
                    .class(style::Button::Icon),
                ),
            );

        let advanced_font_settings = || {
            let section = widget::settings::section()
                .add(
                    widget::settings::item::builder(fl!("default-font-stretch")).control(
                        widget::dropdown(
                            &self.curr_font_stretch_names,
                            font_stretch_selected,
                            Message::DefaultFontStretch,
                        ),
                    ),
                )
                .add(
                    widget::settings::item::builder(fl!("default-font-weight")).control(
                        widget::dropdown(
                            &self.curr_font_weight_names,
                            font_weight_selected,
                            Message::DefaultFontWeight,
                        ),
                    ),
                )
                .add(
                    widget::settings::item::builder(fl!("default-dim-font-weight")).control(
                        widget::dropdown(
                            &self.curr_font_weight_names,
                            dim_font_weight_selected,
                            Message::DefaultDimFontWeight,
                        ),
                    ),
                )
                .add(
                    widget::settings::item::builder(fl!("default-bold-font-weight")).control(
                        widget::dropdown(
                            &self.curr_font_weight_names,
                            bold_font_weight_selected,
                            Message::DefaultBoldFontWeight,
                        ),
                    ),
                )
                .add(
                    widget::settings::item::builder(fl!("use-bright-bold"))
                        .toggler(self.config.use_bright_bold, Message::UseBrightBold),
                );
            let padding = Padding {
                top: 0.0,
                bottom: 0.0,
                left: 12.0,
                right: 12.0,
            };
            widget::container(section).padding(padding)
        };

        if self.show_advanced_font_settings {
            font_section = font_section.add(advanced_font_settings());
        }

        let splits_section = widget::settings::section()
            .title(fl!("splits"))
            .add(
                widget::settings::item::builder(fl!("focus-follow-mouse"))
                    .toggler(self.config.focus_follow_mouse, Message::FocusFollowMouse),
            )
            .add(
                widget::settings::item::builder(fl!("show-pane-borders"))
                    .toggler(self.config.show_pane_borders, Message::ShowPaneBorders),
            );

        let advanced_section = widget::settings::section()
            .title(fl!("advanced"))
            .add(
                widget::settings::item::builder(fl!("show-headerbar"))
                    .description(fl!("show-header-description"))
                    .toggler(self.config.show_headerbar, Message::ShowHeaderBar),
            )
            .add(
                widget::settings::item::builder(fl!("tab-new-inherit-working-directory"))
                    .description(fl!("tab-new-inherit-working-directory-description"))
                    .toggler(
                        self.config.tab_new_inherit_working_directory,
                        Message::TabNewInheritWorkingDirectory,
                    ),
            );

        widget::settings::view_column(vec![
            appearance_section.into(),
            font_section.into(),
            self.threads_section(),
            splits_section.into(),
            advanced_section.into(),
        ])
        .into()
    }

    /// Settings for agent threads: how they are set, and how much of them
    /// is kept in memory
    fn threads_section(&self) -> Element<'_, Message> {
        let threads = &self.config.threads;
        let dropdown = |labels: Vec<String>, selected: Option<usize>, on: fn(usize) -> ThreadSetting| {
            widget::dropdown(labels, selected, move |i| Message::ThreadSetting(on(i)))
        };
        let numbers = |list: &[u16], unit: &dyn Fn(u16) -> String| list.iter().map(|n| unit(*n)).collect::<Vec<_>>();
        let position = |list: &[u16], value: u16| list.iter().position(|n| *n == value);
        let px = |n: u16| format!("{n} px");
        let (loaded, agents, bytes) = self.thread_memory();
        widget::settings::section()
            .title(fl!("threads"))
            .add(
                widget::settings::item::builder(fl!("thread-paper"))
                    .description(fl!("thread-paper-description"))
                    .toggler(self.config.thread_paper, Message::ThreadPaper),
            )
            .add(
                widget::settings::item::builder(fl!("thread-match-colours"))
                    .description(fl!("thread-match-colours-description"))
                    .toggler(self.config.thread_match_colours, |on| Message::ThreadMatchColours(Some(on))),
            )
            .add(widget::settings::item::builder(fl!("thread-paper-font")).control(dropdown(
                vec![
                    fl!("thread-font-libertinus"),
                    fl!("thread-font-new-computer-modern"),
                    fl!("thread-font-serif"),
                    fl!("thread-font-sans"),
                ],
                config::PaperFont::ALL.iter().position(|f| *f == threads.paper_font),
                ThreadSetting::PaperFont,
            )))
            .add(widget::settings::item::builder(fl!("thread-paper-size")).control(dropdown(
                numbers(&PAPER_SIZES, &px),
                position(&PAPER_SIZES, threads.paper_size),
                ThreadSetting::PaperSize,
            )))
            .add(widget::settings::item::builder(fl!("thread-paper-line-height")).control(dropdown(
                numbers(&PAPER_LINE_HEIGHTS, &|n| format!("{:.2}", f32::from(n) / 100.0)),
                position(&PAPER_LINE_HEIGHTS, threads.paper_line_height),
                ThreadSetting::PaperLineHeight,
            )))
            .add(widget::settings::item::builder(fl!("thread-paper-measure")).control(dropdown(
                numbers(&PAPER_MEASURES, &|n| fl!("thread-ems", n = n)),
                position(&PAPER_MEASURES, threads.paper_measure),
                ThreadSetting::PaperMeasure,
            )))
            .add(
                widget::settings::item::builder(fl!("thread-paper-justify"))
                    .toggler(threads.paper_justify, |on| Message::ThreadSetting(ThreadSetting::PaperJustify(on))),
            )
            .add(widget::settings::item::builder(fl!("thread-paper-tone")).control(dropdown(
                vec![fl!("thread-tone-theme"), fl!("thread-tone-white"), fl!("thread-tone-sepia")],
                config::PaperTone::ALL.iter().position(|t| *t == threads.paper_tone),
                ThreadSetting::PaperTone,
            )))
            .add(
                widget::settings::item::builder(fl!("thread-math-scale"))
                    .description(fl!("thread-math-scale-description"))
                    .control(dropdown(
                        numbers(&MATH_SCALES, &|n| format!("{n}%")),
                        position(&MATH_SCALES, threads.math_scale),
                        ThreadSetting::MathScale,
                    )),
            )
            .add(widget::settings::item::builder(fl!("thread-text-size")).control(dropdown(
                numbers(&THREAD_TEXT_SIZES, &px),
                position(&THREAD_TEXT_SIZES, threads.text_size),
                ThreadSetting::TextSize,
            )))
            .add(
                widget::settings::item::builder(fl!("thread-idle"))
                    .description(fl!("thread-idle-description"))
                    .control(dropdown(
                        numbers(&IDLE_MINUTES, &|n| fl!("thread-minutes", n = n)),
                        position(&IDLE_MINUTES, threads.idle_minutes),
                        ThreadSetting::IdleMinutes,
                    )),
            )
            .add(widget::settings::item::builder(fl!("thread-live-agents")).control(dropdown(
                numbers(&LIVE_AGENTS, &|n| n.to_string()),
                position(&LIVE_AGENTS, threads.live_agents),
                ThreadSetting::LiveAgents,
            )))
            .add(
                widget::settings::item::builder(fl!("thread-loaded"))
                    .description(fl!(
                        "thread-memory",
                        loaded = loaded,
                        agents = agents,
                        size = format!("{:.1}", bytes as f64 / 1_048_576.0)
                    ))
                    .control(dropdown(
                        numbers(&LOADED_THREADS, &|n| n.to_string()),
                        position(&LOADED_THREADS, threads.loaded_threads),
                        ThreadSetting::LoadedThreads,
                    )),
            )
            .into()
    }
    /// The popover listing profiles and remote sessions
    fn new_tab_menu(&self) -> Element<'_, Message> {
        let cosmic_theme::Spacing {
            space_xxxs,
            space_xxs,
            space_xs,
            ..
        } = self.core().system_theme().cosmic().spacing;

        let entries = self.new_tab_entries();
        let mut rows = widget::column::with_capacity(entries.len() + 2).spacing(space_xxxs);
        if !entries
            .iter()
            .any(|entry| entry.kind == NewTabEntryKind::Item)
        {
            rows = rows.push(
                widget::container(widget::text::body(fl!("no-matching-results")))
                    .padding(space_xs)
                    .center_x(Length::Fill),
            );
        }
        for entry in entries {
            match entry.kind {
                NewTabEntryKind::Header => {
                    rows = rows.push(
                        widget::container(widget::text::caption_heading(entry.label))
                            .padding([space_xxs, space_xs, space_xxxs, space_xs]),
                    );
                }
                NewTabEntryKind::Item => {
                    let mut content = Vec::with_capacity(4);
                    let icon = entry
                        .mark
                        .icon(16)
                        .unwrap_or_else(|| icon_cache_get("computer-symbolic", 16));
                    content.push(widget::container(icon).padding([0, space_xs, 0, 0]).into());
                    content.push(
                        widget::text::body(entry.label)
                            .width(Length::Fill)
                            .wrapping(cosmic::iced::widget::text::Wrapping::None)
                            .into(),
                    );
                    if entry.is_default {
                        content.push(widget::text::caption(fl!("default-profile-marker")).into());
                    }
                    let mut button = widget::menu::menu_button(content);
                    if let Some(message) = entry.message {
                        button = button.on_press(message);
                    }
                    rows = rows.push(button);
                }
            }
        }
        rows = rows.push(widget::divider::horizontal::light());
        rows = rows.push(
            widget::menu::menu_button(vec![
                widget::text::body(fl!("menu-profiles"))
                    .width(Length::Fill)
                    .into(),
            ])
            .on_press(Message::ToggleContextPage(ContextPage::Profiles)),
        );

        // Running sessions, and searching them, are in the sidebar
        widget::container(widget::scrollable(rows).height(Length::Shrink))
        .padding(space_xxxs)
        .max_height(520.0)
        .class(style::Container::Dropdown)
        .width(Length::Fixed(320.0))
        .into()
    }

    /// The remote hosts named by profiles, without duplicates
    /// The hosts whose sessions the new tab menu lists: this computer (as an
    /// empty string), then the remote hosts named by profiles
    fn session_hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self
            .config
            .profiles
            .values()
            .filter(|profile| profile.is_remote())
            .map(|profile| profile.host.trim().to_string())
            .chain(self.config.machines.values().map(config::Machine::destination))
            .filter(|host| !host.is_empty())
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts.insert(0, String::new());
        hosts
    }

    /// Whether a profile opens a launcher rather than starting straight away:
    /// an agent without a working directory, unless one was given on the
    /// command line
    fn wants_launcher(&self, profile_id: Option<ProfileId>) -> bool {
        let given_dir = self
            .startup_options
            .as_ref()
            .is_some_and(|options| options.working_directory.is_some());
        profile_id
            .and_then(|profile_id| self.config.profiles.get(&profile_id))
            .is_some_and(|profile| {
                profile.agent != config::Agent::Shell
                    && profile.working_directory.trim().is_empty()
                    && !given_dir
                    && self.startup_command.is_none()
            })
    }

    /// A thread profile's directory, when it opens straight into a thread
    /// rather than a launcher
    fn thread_profile_dir(&self, profile_id: ProfileId) -> Option<String> {
        let profile = self.config.profiles.get(&profile_id)?;
        let dir = profile.working_directory.trim();
        (profile.thread && profile.agent != config::Agent::Shell && !dir.is_empty()).then(|| dir.to_string())
    }

    /// Open a launcher tab for an agent profile, and look up what it offers
    fn open_launcher(&mut self, pane: pane_grid::Pane, profile_id: ProfileId) -> Task<Message> {
        let Some(profile) = self.config.profiles.get(&profile_id) else {
            return Task::none();
        };
        let mark = TabMark {
            color: profile.tab_color(),
            remote: profile.is_remote(),
            disconnected: false,
        };
        let (name, host) = (profile.name.clone(), profile.host.trim().to_string());
        self.pane_model.set_focus(pane);
        let Some(tab_model) = self.pane_model.active_mut() else {
            return Task::none();
        };
        let mut builder = tab_model.insert().text(name);
        if let Some(icon) = mark.icon(16) {
            builder = builder.icon(icon);
        }
        let entity = builder.closable().activate().id();
        tab_model.data_set::<TabMark>(entity, mark);
        let hosts = self.session_hosts();
        let unfinished = transfer::Journal::new(&cosmic_term::Env::from_process().state_dir).unfinished();
        let Some(tab_model) = self.pane_model.active_mut() else {
            return Task::none();
        };
        tab_model.data_set::<Launcher>(
            entity,
            Launcher {
                id: cosmic_term::random_hex(16),
                profile_id,
                filter: String::new(),
                filter_id: field_id("launcher-filter"),
                listings: hosts.iter().map(|host| (host.clone(), Listing::Loading)).collect(),
                running: Vec::new(),
                form: None,
                pending_action: None,
                unfinished,
            },
        );
        let sessions_host = host.clone();
        let listings = hosts.into_iter().map(|host| {
            cosmic::task::future(async move {
                let result = agent::launcher_data(host.clone()).await;
                Message::LauncherData(entity, host, result)
            })
        });
        Task::batch(
            [
                cosmic::task::future(async move {
                    Message::LauncherSessions(entity, agent::list_sessions(sessions_host).await)
                }),
                self.update_title(Some(pane)),
            ]
            .into_iter()
            .chain(listings),
        )
    }

    /// The pane holding a tab
    fn tab_pane(&self, entity: segmented_button::Entity) -> Option<pane_grid::Pane> {
        self.pane_model
            .panes
            .iter()
            .find(|(_pane, tab_model)| tab_model.contains_item(entity))
            .map(|(pane, _)| *pane)
    }

    fn launcher(&self, entity: segmented_button::Entity) -> Option<&Launcher> {
        let pane = self.tab_pane(entity)?;
        self.pane_model.panes.get(pane)?.data::<Launcher>(entity)
    }

    fn launcher_mut(&mut self, entity: segmented_button::Entity) -> Option<&mut Launcher> {
        let pane = self.tab_pane(entity)?;
        self.pane_model.panes.get_mut(pane)?.data_mut::<Launcher>(entity)
    }

    fn launcher_locked(&self, entity: segmented_button::Entity) -> bool {
        self.launcher(entity).is_some_and(|launcher| {
            launcher.pending_action.is_some()
                || matches!(&launcher.form, Some(LauncherForm::Worktree(form)) if form.busy)
                || matches!(&launcher.form, Some(LauncherForm::Transfer(form)) if form.busy)
        })
    }

    /// A launcher's rows, filtered: running sessions, then projects, then
    /// conversations
    fn launcher_rows(
        &self,
        launcher: &Launcher,
    ) -> Vec<(config::LauncherSection, String, Vec<LauncherRow>)> {
        let filter = launcher.filter.trim().to_lowercase();
        let matches = |text: &str| filter.is_empty() || text.to_lowercase().contains(&filter);
        let agent = self
            .config
            .profiles
            .get(&launcher.profile_id)
            .map(|profile| profile.agent);
        let open = self.open_sessions();
        let mut sections = Vec::new();

        let running: Vec<LauncherRow> = launcher
            .running
            .iter()
            .filter(|session| session.agent() == agent && !open.contains(session) && !session.is_thread())
            .filter(|session| matches(&session.label()))
            .map(|session| LauncherRow {
                label: session.label(),
                detail: session.name.clone(),
                when: String::new(),
                choice: Some(LaunchChoice::Attach(session.clone())),
                actions: Vec::new(),
            })
            .collect();
        sections.push((
            config::LauncherSection::Running,
            fl!("launcher-running"),
            running,
        ));

        let target = self
            .config
            .profiles
            .get(&launcher.profile_id)
            .map(|profile| profile.host.trim().to_string())
            .unwrap_or_default();
        let target_kind = agent.and_then(agent_kind);
        let target_identity = launcher
            .listings
            .get(&target)
            .and_then(Listing::listing)
            .map(|listing| listing.identity().to_string());
        let shortener = |home: &str, path: &str| match path.strip_prefix(home) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        };

        // The target computer's projects, from both agents
        if let Some(state) = launcher.listings.get(&target)
            && let Some(listing) = state.listing()
        {
            let offline = !matches!(state, Listing::Ready(_));
            let mut used: BTreeMap<&str, f64> = BTreeMap::new();
            for project in listing.agents.values().flat_map(|a| &a.projects) {
                let entry = used.entry(project.path.as_str()).or_default();
                *entry = entry.max(project.last_used);
            }
            let mut found: Vec<(&str, f64)> = used.into_iter().collect();
            found.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
            let projects = found
                .into_iter()
                .filter(|(path, _)| matches(path))
                .map(|(path, last_used)| LauncherRow {
                    label: path.rsplit('/').next().unwrap_or(path).to_string(),
                    detail: shortener(&listing.home, path),
                    when: time_ago(last_used),
                    choice: (!offline).then(|| LaunchChoice::Dir(path.to_string())),
                    actions: if offline {
                        Vec::new()
                    } else {
                        vec![(fl!("launcher-new-worktree"), LaunchChoice::NewWorktree(path.to_string()))]
                    },
                })
                .collect();
            sections.push((
                config::LauncherSection::Projects,
                fl!("launcher-projects"),
                projects,
            ));
        }

        // Every computer's conversations, from both agents, newest first;
        // not those open in a thread here, whose agent would have two CLIs
        let in_threads: std::collections::HashSet<(String, String)> = self
            .thread_ids()
            .into_iter()
            .filter_map(|id| self.thread(id))
            .filter_map(|t| t.session_id.clone().map(|s| (t.host.clone(), s)))
            .collect();
        let mut conversations: Vec<(f64, LauncherRow)> = Vec::new();
        for (host, state) in &launcher.listings {
            let Some(listing) = state.listing() else {
                continue;
            };
            let offline = !matches!(state, Listing::Ready(_));
            let same_computer = host == &target || target_identity.as_deref() == Some(listing.identity());
            // The target computer reached under another name
            if host != &target && same_computer {
                continue;
            }
            let host_label = match self.config.machine_for_host(host) {
                Some(machine) => machine.name.clone(),
                None if listing.hostname.is_empty() => host.clone(),
                None => listing.hostname.clone(),
            };
            for (kind, config_agent) in [
                (ConversationKind::Claude, config::Agent::Claude),
                (ConversationKind::Codex, config::Agent::Codex),
            ] {
                let Some(found) = listing.agent(config_agent) else {
                    continue;
                };
                for conversation in &found.conversations {
                    if in_threads.contains(&(host.clone(), conversation.id.clone())) {
                        continue;
                    }
                    if !(matches(&conversation.title) || matches(&conversation.cwd) || (!same_computer && matches(&host_label))) {
                        continue;
                    }
                    let mut detail = shortener(&listing.home, &conversation.cwd);
                    if !same_computer {
                        detail = fl!("launcher-on-host", detail = detail, agent = config_agent.label(), host = host_label.as_str());
                    } else if Some(config_agent) != agent {
                        detail = fl!("launcher-other-agent", detail = detail, agent = config_agent.label());
                    }
                    let reference = transfer::ConversationRef {
                        host: host.clone(),
                        kind,
                        id: conversation.id.clone(),
                    };
                    let (choice, actions) = if offline {
                        (None, Vec::new())
                    } else if same_computer && Some(config_agent) == agent {
                        let choice = LaunchChoice::Resume {
                            id: conversation.id.clone(),
                            cwd: conversation.cwd.clone(),
                        };
                        (Some(choice), Vec::new())
                    } else if target_kind.is_none() {
                        (None, Vec::new())
                    } else if same_computer {
                        let choice = LaunchChoice::Continue {
                            conversation: reference,
                            cwd: conversation.cwd.clone(),
                        };
                        (Some(choice), Vec::new())
                    } else {
                        let transfer = |mode| LaunchChoice::Transfer {
                            conversation: reference.clone(),
                            cwd: conversation.cwd.clone(),
                            title: conversation.title.clone(),
                            mode,
                        };
                        let actions = vec![
                            (fl!("launcher-move-here"), transfer(transfer::Mode::Move)),
                            (fl!("launcher-fork-here"), transfer(transfer::Mode::Fork)),
                        ];
                        (None, actions)
                    };
                    let mut when = time_ago(conversation.last_used);
                    if offline {
                        when = fl!("launcher-offline", when = when);
                    }
                    conversations.push((
                        conversation.last_used,
                        LauncherRow {
                            label: conversation.title.clone(),
                            detail,
                            when,
                            choice,
                            actions,
                        },
                    ));
                }
            }
        }
        conversations.sort_by(|a, b| b.0.total_cmp(&a.0));
        sections.push((
            config::LauncherSection::Conversations,
            fl!("launcher-conversations"),
            conversations.into_iter().map(|(_, row)| row).collect(),
        ));
        sections.retain(|(_, _, rows)| !rows.is_empty());
        sections
    }

    /// Whether a launcher section's rows are shown: when expanded, or when
    /// filtering, so that matches are never hidden
    fn launcher_section_open(&self, launcher: &Launcher, section: config::LauncherSection) -> bool {
        !self.config.launcher_collapsed.contains(&section) || !launcher.filter.trim().is_empty()
    }

    /// The contents of a launcher tab
    fn launcher_view<'a>(
        &'a self,
        entity: segmented_button::Entity,
        launcher: &'a Launcher,
    ) -> Element<'a, Message> {
        let cosmic_theme::Spacing {
            space_xxs,
            space_xs,
            space_s,
            ..
        } = self.core().system_theme().cosmic().spacing;
        let name = self
            .config
            .profiles
            .get(&launcher.profile_id)
            .map(|profile| profile.name.clone())
            .unwrap_or_default();
        let locked = self.launcher_locked(entity);

        let mut column = widget::column::with_capacity(8)
            .spacing(space_s)
            .push(widget::text::title3(name))
            .push(
                widget::row::with_children(vec![
                    widget::button::suggested(fl!("launcher-home"))
                        .on_press_maybe((!locked).then(|| Message::LauncherChoose(entity, LaunchChoice::Dir(String::new()))))
                        .into(),
                    widget::button::standard(fl!("launcher-browse"))
                        .on_press_maybe((!locked).then_some(Message::LauncherChoose(entity, LaunchChoice::Browse)))
                        .into(),
                ])
                .spacing(space_xs),
            )
            // A form replaces the list the filter narrows
            .push_maybe(launcher.form.is_none().then(|| {
                widget::text_input::search_input(fl!("launcher-filter"), &launcher.filter)
                    .id(launcher.filter_id.clone())
                    .on_focus(Message::LauncherFocus(entity, launcher.filter_id.clone()))
                    .on_input(move |text| Message::LauncherFilter(entity, text))
                    .on_paste(move |text| Message::LauncherFilter(entity, text))
                    .on_submit(move |_| Message::LauncherSubmit(entity))
            }));
        if let Some((_, status)) = &launcher.pending_action {
            column = column.push(widget::text::caption(status.clone()));
        }

        for record in &launcher.unfinished {
            let id = record.plan.draft.id.clone();
            let active = self.active_transfers.contains(&id);
            column = column.push(
                widget::row::with_children(vec![
                    widget::text::body(launcher_forms::describe(record))
                        .width(Length::Fill)
                        .into(),
                    widget::button::standard(fl!("transfer-resume"))
                        .on_press_maybe((!active).then(|| Message::LauncherRecover(entity, id.clone(), true)))
                        .into(),
                    widget::button::standard(fl!("transfer-roll-back"))
                        .on_press_maybe((!active).then(|| Message::LauncherRecover(entity, id, false)))
                        .into(),
                ])
                .spacing(space_xs)
                .align_y(Alignment::Center),
            );
            if active {
                column = column.push(widget::text::caption(fl!("transfer-recovering")));
            }
        }
        match &launcher.form {
            Some(LauncherForm::Worktree(form)) => {
                column = column.push(form.view(entity, space_s));
                return self.launcher_frame(column.into());
            }
            Some(LauncherForm::Transfer(form)) => {
                column = column.push(widget::scrollable(form.view(entity, space_s)).height(Length::Fill));
                return self.launcher_frame(column.into());
            }
            None => {}
        }

        let mut list = widget::column::with_capacity(32).spacing(space_xxs);
        let target = self
            .config
            .profiles
            .get(&launcher.profile_id)
            .map(|profile| profile.host.trim().to_string())
            .unwrap_or_default();
        for (host, state) in &launcher.listings {
            let message = match state {
                Listing::Loading if host == &target => fl!("launcher-loading"),
                Listing::Offline(_, error) if host == &target => {
                    fl!("launcher-failed", error = error.as_str())
                }
                // A host never reached has nothing to show, as when a
                // profile names this computer by an ssh name it refuses
                Listing::Offline(Some(_), error) => fl!(
                    "launcher-host-offline",
                    host = host.as_str(),
                    error = error.as_str()
                ),
                _ => continue,
            };
            list = list.push(widget::text::caption(message));
        }
        for (section, title, rows) in self.launcher_rows(launcher) {
            let open = self.launcher_section_open(launcher, section);
            let collapsed = self.config.launcher_collapsed.contains(&section);
            let header = widget::row::with_children(vec![
                widget::text::heading(format!("{title} ({})", rows.len()))
                    .width(Length::Fill)
                    .into(),
                icon_cache_get(if collapsed { "go-down-symbolic" } else { "go-up-symbolic" }, 16)
                    .into(),
            ])
            .align_y(Alignment::Center);
            list = list.push(
                widget::container(
                    widget::button::custom(header)
                        .class(style::Button::MenuItem)
                        .width(Length::Fill)
                        .padding([space_xxs, space_s])
                        .on_press(Message::LauncherToggleSection(section)),
                )
                .padding([space_xs, 0, 0, 0]),
            );
            if !open {
                continue;
            }
            for row in rows {
                let content = widget::row::with_children(vec![
                    widget::column::with_children(vec![
                        widget::text::body(ellipsize_middle(&row.label, 90))
                            .wrapping(cosmic::iced::widget::text::Wrapping::None)
                            .into(),
                        widget::text::caption(ellipsize_middle(&row.detail, 90))
                            .wrapping(cosmic::iced::widget::text::Wrapping::None)
                            .into(),
                    ])
                    .width(Length::Fill)
                    .into(),
                    widget::text::caption(row.when).into(),
                ])
                .align_y(Alignment::Center)
                .spacing(space_s);
                let pressable = widget::button::custom(content)
                    .class(style::Button::MenuItem)
                    .width(Length::Fill)
                    .padding([space_xxs, space_s])
                    .on_press_maybe((!locked).then(|| row.choice).flatten().map(|choice| Message::LauncherChoose(entity, choice)));
                if row.actions.is_empty() {
                    list = list.push(pressable);
                } else {
                    let mut line = widget::row::with_capacity(1 + row.actions.len())
                        .spacing(space_xxs)
                        .align_y(Alignment::Center)
                        .push(pressable);
                    for (label, choice) in row.actions {
                        line = line.push(
                            widget::button::text(label)
                                .on_press_maybe((!locked).then_some(Message::LauncherChoose(entity, choice))),
                        );
                    }
                    list = list.push(line);
                }
            }
        }
        column = column.push(widget::scrollable(list).height(Length::Fill));
        self.launcher_frame(column.into())
    }

    /// A launcher's contents, centred and painted like a terminal
    fn launcher_frame<'a>(&'a self, column: Element<'a, Message>) -> Element<'a, Message> {
        let cosmic_theme::Spacing { space_m, .. } = self.core().system_theme().cosmic().spacing;

        // Painted like a terminal, which draws its own background
        let t = self.core().system_theme();
        let alpha = if t.transparent {
            t.cosmic().alpha_map.blurred_alpha(t.cosmic().frosted)
        } else {
            self.config.opacity_ratio()
        };
        widget::container(
            widget::container(column)
                .max_width(760.0)
                .padding(space_m)
                .height(Length::Fill),
        )
        .center_x(Length::Fill)
        .height(Length::Fill)
        .class(style::Container::Custom(Box::new(move |theme| {
            let cosmic = theme.cosmic();
            let mut background: Color = cosmic.background(theme.transparent).base.into();
            background.a *= alpha;
            cosmic::iced::widget::container::Style {
                background: Some(iced::Background::Color(background)),
                text_color: Some(cosmic.background(theme.transparent).on.into()),
                ..Default::default()
            }
        })))
        .into()
    }

    /// Look up where an agent tab's agent is working, unless that was done
    /// in the last few seconds
    fn refresh_focus(&mut self, entity: segmented_button::Entity) -> Task<Message> {
        let Some(pane) = self.tab_pane(entity) else {
            return Task::none();
        };
        let Some(tab_model) = self.pane_model.panes.get_mut(pane) else {
            return Task::none();
        };
        let Some(session) = tab_model
            .data::<agent::Session>(entity)
            .filter(|s| s.agent().is_some_and(|a| a != config::Agent::Shell))
            .cloned()
        else {
            return Task::none();
        };
        if tab_model.data::<TabFocus>(entity).is_none() {
            tab_model.data_set::<TabFocus>(entity, TabFocus::default());
        }
        let Some(focus) = tab_model.data_mut::<TabFocus>(entity) else {
            return Task::none();
        };
        if focus.pending || focus.checked.is_some_and(|t| t.elapsed() < FOCUS_INTERVAL) {
            return Task::none();
        }
        focus.pending = true;
        cosmic::task::future(async move {
            Message::TabFocusFound(entity, agent::tab_agents(agent::TabProcess::Session(session)).await)
        })
    }

    /// Whether two ssh destinations ("" for this computer) are one computer
    fn same_computer(&self, a: &str, b: &str) -> bool {
        let identity = |host: &str| -> Option<String> {
            if host.trim().is_empty() {
                self.local.as_ref().map(|l| if l.host_id.is_empty() { l.hostname.clone() } else { l.host_id.clone() })
            } else {
                self.config
                    .machine_for_host(host)
                    .map(|m| if m.host_id.is_empty() { m.hostname.clone() } else { m.host_id.clone() })
                    .filter(|id| !id.is_empty())
            }
        };
        a.trim() == b.trim() || matches!((identity(a), identity(b)), (Some(x), Some(y)) if x == y)
    }

    /// For each agent tab, the agent profiles on other computers it can be
    /// moved or forked to
    fn transfer_destinations(
        &self,
        tab_model: &segmented_button::Model<segmented_button::SingleSelect>,
    ) -> HashMap<segmented_button::Entity, Vec<(String, ProfileId)>> {
        let mut destinations = HashMap::new();
        for entity in tab_model.iter() {
            // A thread's conversation moves as a session's does, once it has one
            if let Some(thread) = tab_model.data::<thread::tab::ThreadTab>(entity) {
                if thread.session_id.is_some() {
                    destinations.insert(entity, self.destinations_from(&thread.host));
                }
                continue;
            }
            let Some(session) = tab_model.data::<agent::Session>(entity) else {
                continue;
            };
            if !session.agent().is_some_and(|a| a != config::Agent::Shell) {
                continue;
            }
            destinations.insert(entity, self.session_destinations(session));
        }
        destinations
    }

    /// The agent profiles on other computers, which a session's
    /// conversation could move or fork to
    fn session_destinations(&self, session: &agent::Session) -> Vec<(String, ProfileId)> {
        if !session.agent().is_some_and(|a| a != config::Agent::Shell) {
            return Vec::new();
        }
        self.destinations_from(&session.host)
    }

    /// The agent profiles on computers other than `host`'s
    fn destinations_from(&self, host: &str) -> Vec<(String, ProfileId)> {
        let mut found: Vec<(String, ProfileId)> = self
            .config
            .profiles
            .iter()
            .filter(|(_, p)| agent_kind(p.agent).is_some() && !self.same_computer(&p.host, host))
            .map(|(id, p)| (p.name.clone(), *id))
            .collect();
        found.sort();
        found
    }

    /// A tab also open in another window: its pane, that window's bus name,
    /// and its session
    fn shared_tab(
        &self,
        entity: segmented_button::Entity,
    ) -> Option<(pane_grid::Pane, String, agent::Session)> {
        let pane = self.tab_pane(entity)?;
        let tab_model = self.pane_model.panes.get(pane)?;
        let shared = tab_model.data::<TabShared>(entity)?;
        let session = tab_model.data::<agent::Session>(entity)?;
        Some((pane, shared.peer.clone(), session.clone()))
    }

    /// Name agents' tabs after their conversations, as the sidebar does,
    /// unless the user named the tab
    fn retitle_agent_tabs(&mut self) -> Task<Message> {
        let mut changed = false;
        for (_pane, tab_model) in self.pane_model.panes.iter_mut() {
            let entities: Vec<_> = tab_model.iter().collect();
            for entity in entities {
                let overridden = tab_model
                    .data::<Mutex<Terminal>>(entity)
                    .is_some_and(|t| t.lock().unwrap().tab_title_override.is_some());
                let Some(session) = tab_model.data::<agent::Session>(entity) else {
                    continue;
                };
                let current = tab_model.text(entity).unwrap_or_default();
                let base = current.strip_prefix(SUDO_LOCK).unwrap_or(current);
                let mut title = self
                    .sidebar
                    .title(session)
                    .map_or_else(|| base.to_string(), str::to_string);
                // A lock while the session waits for sudo approval
                if self.secrets.for_session(&session.name).next().is_some() {
                    title = format!("{SUDO_LOCK}{title}");
                }
                if !overridden && tab_model.text(entity) != Some(title.as_str()) {
                    tab_model.text_set(entity, title);
                    changed = true;
                }
            }
        }
        if changed { self.update_title(None) } else { Task::none() }
    }

    /// The pane and tab showing a session in this window
    fn session_tab(&self, session: &agent::Session) -> Option<(pane_grid::Pane, segmented_button::Entity)> {
        self.pane_model.panes.iter().find_map(|(pane, tab_model)| {
            tab_model
                .iter()
                .find(|&entity| {
                    tab_model.data::<agent::Session>(entity) == Some(session)
                        // A thread, by the session of its relay
                        || tab_model
                            .data::<thread::tab::ThreadTab>(entity)
                            .is_some_and(|t| t.relay_live && t.relay.as_ref() == Some(session))
                })
                .map(|entity| (*pane, entity))
        })
    }

    /// The ssh destination of a launcher's profile, or "" for this computer
    fn launcher_host(&self, entity: segmented_button::Entity) -> String {
        self.launcher(entity)
            .and_then(|launcher| self.config.profiles.get(&launcher.profile_id))
            .map(|profile| profile.host.trim().to_string())
            .unwrap_or_default()
    }

    /// A profile for the agent and computer a transfer was for
    fn profile_for(&self, launch: &transfer::Launch) -> Option<ProfileId> {
        self.config
            .profiles
            .iter()
            .find(|(_, profile)| {
                profile.host.trim() == launch.host && agent_kind(profile.agent) == Some(launch.kind)
            })
            .map(|(id, _)| *id)
    }

    fn launcher_error(&mut self, err: String) -> Task<Message> {
        self.toasts
            .push(widget::toaster::Toast::new(fl!("launcher-failed", error = err.as_str())))
            .map(cosmic::Action::App)
    }

    /// Replace a launcher with its agent in `dir`, started as a transfer or
    /// handoff says, with the launcher's profile unless another is given
    fn launch_started(
        &mut self,
        entity: segmented_button::Entity,
        profile_id: Option<ProfileId>,
        dir: &str,
        start: &transfer::Start,
    ) -> Task<Message> {
        let Some(pane) = self.tab_pane(entity) else {
            return Task::none();
        };
        let Some(profile_id) = profile_id.or_else(|| self.launcher(entity).map(|l| l.profile_id)) else {
            return Task::none();
        };
        let Some(profile) = self.config.profiles.get(&profile_id).cloned() else {
            return Task::none();
        };
        if profile.thread && profile.agent != config::Agent::Shell {
            // As a thread: resumed with the note, or opened with the message
            let (resume, note, prompt) = match start {
                transfer::Start::Resume { id, note } => (Some(id.clone()), Some(note.clone()), None),
                transfer::Start::Seeded { prompt } => (None, None, Some(prompt.clone())),
            };
            let open = self.open_thread(pane, Some(profile_id), dir.to_string(), resume, Some(entity));
            let id = self.next_thread - 1;
            if let Some(tab) = self.thread_mut(id) {
                tab.note = note.filter(|note| !note.is_empty());
                if let Some(prompt) = &prompt {
                    tab.composer = widget::text_editor::Content::with_text(prompt);
                }
            }
            let send = if prompt.is_some() { self.thread_message(id, thread::tab::Msg::Send) } else { Task::none() };
            return Task::batch([open, send]);
        }
        let prepared = agent::launch_start(&profile, dir, start);
        self.replace_tab(pane, entity, Some(profile_id), prepared)
    }

    /// Replace a launcher with its agent, started as `choice` says
    fn launch_from(
        &mut self,
        entity: segmented_button::Entity,
        choice: LaunchChoice,
    ) -> Task<Message> {
        let Some(pane) = self.tab_pane(entity) else {
            return Task::none();
        };
        let Some(launcher) = self.launcher(entity) else {
            return Task::none();
        };
        let profile_id = launcher.profile_id;
        let Some(profile) = self.config.profiles.get(&profile_id).cloned() else {
            return Task::none();
        };
        if profile.thread && profile.agent != config::Agent::Shell {
            match &choice {
                LaunchChoice::Dir(dir) => {
                    return self.open_thread(pane, Some(profile_id), dir.clone(), None, Some(entity));
                }
                LaunchChoice::Resume { id, cwd } => {
                    return self.open_thread(pane, Some(profile_id), cwd.clone(), Some(id.clone()), Some(entity));
                }
                _ => {}
            }
        }
        let prepared = match choice {
            LaunchChoice::Dir(dir) => {
                agent::launch(&profile, Some(dir.as_str()).filter(|dir| !dir.is_empty()))
            }
            LaunchChoice::Resume { id, cwd } => {
                agent::launch_resuming(&profile, Some(&cwd), Some(&id))
            }
            LaunchChoice::Attach(session) => (agent::attach(&session), session),
            LaunchChoice::NewWorktree(dir) => {
                let host = profile.host.trim().to_string();
                let form = WorktreeForm::new();
                let request_id = form.request_id.clone();
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.form = Some(LauncherForm::Worktree(Box::new(form)));
                }
                return cosmic::task::future(async move {
                    let result = async {
                        agent::ensure_helper(host.clone(), agent::Helper::Transfer).await?;
                        blocking(move || {
                            let request = cosmic_term::helper::Request::WorktreeOptions { dir: dir.into(), fetch: true };
                            match transfer::Machine::call(&agent::HostMachine { host }, request)? {
                                cosmic_term::helper::Reply::WorktreeOptions(options) => Ok(options),
                                cosmic_term::helper::Reply::Error { message } => Err(message),
                                other => Err(format!("unexpected reply: {other:?}")),
                            }
                        })
                        .await
                    }
                    .await;
                    Message::LauncherWorktreeOptions(entity, request_id, result)
                });
            }
            LaunchChoice::Continue { conversation, cwd } => {
                let request_id = cosmic_term::random_hex(16);
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.pending_action = Some((request_id.clone(), fl!("launcher-continuing")));
                }
                return cosmic::task::future(async move {
                    let result = async {
                        agent::ensure_helper(conversation.host.clone(), agent::Helper::Transfer).await?;
                        let machine = agent::HostMachine { host: conversation.host.clone() };
                        blocking(move || transfer::continue_with(&machine, &conversation)).await
                    }
                    .await;
                    Message::LauncherStart(entity, request_id, cwd, result)
                });
            }
            LaunchChoice::Transfer { ref conversation, ref cwd, ref title, mode } => {
                let retry = choice.clone();
                let (conversation, cwd, title) = (conversation.clone(), cwd.clone(), title.clone());
                let destination = profile.host.trim().to_string();
                let Some(target) = agent_kind(profile.agent) else {
                    return Task::none();
                };
                // A relayed agent ends first, as its CLI would block the plan
                let ended = self.stop_idle_threads_in(&conversation.host, &cwd);
                let form = TransferForm::new(mode, title, Some(retry));
                let request_id = form.request_id.clone();
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.form = Some(LauncherForm::Transfer(Box::new(form)));
                }
                return ended.chain(cosmic::task::future(async move {
                    let result = async {
                        agent::ensure_helper(conversation.host.clone(), agent::Helper::Transfer).await?;
                        agent::ensure_helper(destination.clone(), agent::Helper::Transfer).await?;
                        blocking(move || {
                            let source = agent::HostMachine { host: conversation.host.clone() };
                            let dest = agent::HostMachine { host: destination.clone() };
                            transfer::plan(&source, &dest, conversation, &cwd, &destination, target, mode)
                        })
                        .await
                    }
                    .await;
                    Message::LauncherTransferDraft(entity, request_id, Box::new(result))
                }));
            }
            LaunchChoice::Browse => {
                let request_id = cosmic_term::random_hex(16);
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.pending_action = Some((request_id.clone(), fl!("launcher-mounting")));
                }
                if !profile.is_remote() {
                    return self.update(Message::LauncherMounted(entity, request_id, Ok(PathBuf::new())));
                }
                let host = profile.host.trim().to_string();
                return cosmic::task::future(async move {
                    Message::LauncherMounted(entity, request_id, agent::ensure_mount(host).await)
                });
            }
        };
        // The agent takes the launcher's place in the tab bar
        self.replace_tab(pane, entity, Some(profile_id), prepared)
    }

    /// Replace a tab with a new terminal running `prepared`, in the same
    /// place in the tab bar
    fn replace_tab(
        &mut self,
        pane: pane_grid::Pane,
        entity: segmented_button::Entity,
        profile_id: Option<ProfileId>,
        prepared: (host::HostCommand, agent::Session),
    ) -> Task<Message> {
        let Some(tab_model) = self.pane_model.panes.get_mut(pane) else {
            return Task::none();
        };
        let position = tab_model.position(entity);
        let was_active = tab_model.active() == entity;
        let active = tab_model.active();
        tab_model.remove(entity);
        let task = self.create_and_focus_new_terminal_with(pane, profile_id, false, Some(prepared));
        if let Some(tab_model) = self.pane_model.panes.get_mut(pane) {
            let started = tab_model.active();
            if let Some(position) = position {
                tab_model.position_set(started, position);
            }
            // A tab reconnecting in the background stays in the background
            if !was_active && tab_model.contains_item(active) {
                tab_model.activate(active);
            }
        }
        task
    }

    /// Mark a remote tab whose connection dropped, and start reconnecting
    /// to its host
    fn tab_disconnected(
        &mut self,
        pane: pane_grid::Pane,
        entity: segmented_button::Entity,
    ) -> Task<Message> {
        let Some(tab_model) = self.pane_model.panes.get_mut(pane) else {
            return Task::none();
        };
        let Some(host) = tab_model
            .data::<agent::Session>(entity)
            .filter(|session| !session.is_local())
            .map(|session| session.host.clone())
        else {
            return Task::none();
        };
        let Some(connection) = tab_model.data_mut::<TabConnection>(entity) else {
            return Task::none();
        };
        connection.lost = Some(Instant::now());
        if let Some(mark) = tab_model.data_mut::<TabMark>(entity) {
            mark.disconnected = true;
            let mark = *mark;
            if let Some(icon) = mark.icon(16) {
                tab_model.icon_set(entity, icon);
            }
        }
        if let Some(title) = tab_model.text(entity).map(str::to_string) {
            tab_model.text_set(entity, fl!("tab-disconnected", title = title.as_str()));
        }
        if self.reconnecting.contains_key(&host) {
            return Task::none();
        }
        self.probe_host(host, 0, 5)
    }

    /// Probe a host after `delay_secs`, replacing any probe already pending
    fn probe_host(&mut self, host: String, attempt: u32, delay_secs: u64) -> Task<Message> {
        let generation = self.reconnecting.get(&host).map_or(0, |(g, _)| g + 1);
        self.reconnecting.insert(host.clone(), (generation, attempt));
        cosmic::task::future(async move {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
            let reachable = agent::probe_host(host.clone()).await;
            Message::HostProbed(host, generation, reachable)
        })
    }

    /// The disconnected tabs on a host
    fn disconnected_tabs(&self, host: &str) -> Vec<(pane_grid::Pane, segmented_button::Entity)> {
        self.pane_model
            .panes
            .iter()
            .flat_map(|(pane, tab_model)| {
                tab_model
                    .iter()
                    .filter(move |entity| {
                        tab_model
                            .data::<TabConnection>(*entity)
                            .is_some_and(|connection| connection.lost.is_some())
                            && tab_model
                                .data::<agent::Session>(*entity)
                                .is_some_and(|session| session.host == host)
                    })
                    .map(move |entity| (*pane, entity))
            })
            .collect()
    }

    /// Open a tab dropped on `pane`, from this window or another
    fn receive_tab(&mut self, pane: pane_grid::Pane, session: agent::Session) -> Task<Message> {
        if let Some((drag_pane, drag_entity, _)) = self.tab_drag {
            let dragged = self
                .pane_model
                .panes
                .get(drag_pane)
                .and_then(|tab_model| tab_model.data::<agent::Session>(drag_entity));
            // Dropped back where it came from. Dropped on another pane, the
            // original closes when the drag ends
            if dragged == Some(&session) && drag_pane == pane {
                self.tab_drag = None;
                return Task::none();
            }
        }
        self.pane_model.set_focus(pane);
        self.create_and_focus_new_terminal_with(
            pane,
            None,
            false,
            Some((agent::attach(&session), session)),
        )
    }

    /// The hosts the sidebar scans, with their labels: this computer, then
    /// the other machines, leaving out any that is this computer by another
    /// name
    fn sidebar_hosts(&self) -> Vec<(String, String)> {
        let local = self.local.as_ref();
        let this_computer = |machine: &config::Machine| {
            local.is_some_and(|local| {
                (!machine.host_id.is_empty() && machine.host_id == local.host_id)
                    || (!machine.hostname.is_empty() && machine.hostname == local.hostname)
            })
        };
        self.session_hosts()
            .into_iter()
            .filter_map(|host| {
                if host.is_empty() {
                    let label = local
                        .map(|l| l.hostname.clone())
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| fl!("this-computer"));
                    return Some((host, label));
                }
                let machine = self.config.machines.values().find(|m| m.destination() == host);
                if machine.is_some_and(this_computer) {
                    return None;
                }
                let label = machine.map_or_else(|| host.clone(), |m| m.name.clone());
                Some((host, label))
            })
            .collect()
    }

    /// Start scanning every host for the sidebar and the tabs' previews,
    /// which are wanted with the sidebar hidden too
    fn sidebar_start(&mut self) -> Task<Message> {
        self.sidebar.generation += 1;
        let generation = self.sidebar.generation;
        let hosts = self.sidebar_hosts();
        self.sidebar.hosts.retain(|host, _| hosts.iter().any(|(h, _)| h == host));
        let mut tasks = Vec::new();
        for (host, label) in hosts {
            let state = self.sidebar.hosts.entry(host.clone()).or_default();
            state.label = label;
            state.scanning = false;
            tasks.push(self.update(Message::SidebarRefresh(host, generation)));
        }
        Task::batch(tasks)
    }

    /// A profile running `agent` on `host`, else the default profile if it
    /// runs there, else any profile that does
    fn profile_on_host(&self, host: &str, agent: Option<config::Agent>, thread: bool) -> Option<ProfileId> {
        let on_host = |p: &&Profile| p.host.trim() == host;
        let profiles = &self.config.profiles;
        // The agent in the view asked for, else in the other
        agent
            .and_then(|agent| {
                profiles
                    .iter()
                    .filter(|(_, p)| on_host(p) && p.agent == agent)
                    .min_by_key(|(_, p)| p.thread != thread)
                    .map(|(id, _)| *id)
            })
            .or_else(|| {
                self.get_default_profile()
                    .filter(|id| profiles.get(id).is_some_and(|p| on_host(&p)))
            })
            .or_else(|| profiles.iter().find(|(_, p)| on_host(p)).map(|(id, _)| *id))
    }

    /// Where the processes of the tab in front of the focused pane run
    fn active_process(&self) -> Option<agent::TabProcess> {
        let tab_model = self.pane_model.active()?;
        let entity = tab_model.active();
        if let Some(session) = tab_model.data::<agent::Session>(entity) {
            return Some(agent::TabProcess::Session(session.clone()));
        }
        if let Some(thread) = tab_model.data::<thread::tab::ThreadTab>(entity) {
            return Some(agent::TabProcess::Dir { host: thread.host.clone(), dir: thread.cwd.clone() });
        }
        let TabToken(token) = tab_model.data::<TabToken>(entity)?;
        Some(agent::TabProcess::Local(token.clone()))
    }

    /// The remote hosts of this window's launchers and threads, which want
    /// the sudo agent and mounts as sessions do
    fn launcher_hosts(&self) -> Vec<&str> {
        self.pane_model
            .panes
            .iter()
            .flat_map(|(_, tab_model)| {
                tab_model
                    .iter()
                    .filter_map(|entity| {
                        if let Some(thread) = tab_model.data::<thread::tab::ThreadTab>(entity) {
                            return Some(thread.host.trim());
                        }
                        let launcher = tab_model.data::<Launcher>(entity)?;
                        self.config.profiles.get(&launcher.profile_id).map(|profile| profile.host.trim())
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Whether a sudo request is in front of the user: its tab is in front
    /// of the focused pane of a focused window, or, when no tab here shows
    /// its session, the window is focused
    fn sudo_in_front(&self, request: &cosmic_term::sudo_control::Request) -> bool {
        if !self.window_focused {
            return false;
        }
        let Some(session) = request.session.as_deref() else {
            return true;
        };
        let shown_here = self.pane_model.panes.iter().any(|(_, tab_model)| {
            tab_model
                .iter()
                .any(|e| tab_model.data::<agent::Session>(e).is_some_and(|s| s.name == session))
        });
        !shown_here || self.active_session().is_some_and(|s| s.name == session)
    }

    /// Each machine for the Secrets pane: its name, ssh destination (empty
    /// for this computer) and host name, which its sudo knows itself by
    fn secret_machines(&self) -> Vec<(String, String, String)> {
        let here = agent::mounts::local_name();
        let mut machines = vec![(fl!("this-computer-named", name = here.as_str()), String::new(), here.clone())];
        for machine in self.config.machines.values() {
            let dest = machine.destination();
            let name = agent::mounts::name_for(&dest);
            if name != here {
                machines.push((machine.name.clone(), dest, name));
            }
        }
        machines
    }

    /// The saved passwords the right-click menu offers to paste, by name and
    /// machine; none while the store is locked, to offer unlocking instead
    fn paste_passwords(&self) -> Option<Vec<(u64, String)>> {
        let state = &self.secrets.state;
        if self.secrets.agent.is_none() || !state.exists {
            return Some(Vec::new());
        }
        if !state.unlocked {
            return None;
        }
        Some(
            state
                .entries
                .iter()
                .map(|entry| {
                    let label = match &entry.host {
                        Some(host) => fl!("paste-password-on", account = entry.label.as_str(), machine = host.as_str()),
                        None => entry.label.clone(),
                    };
                    (entry.id, label)
                })
                .collect(),
        )
    }

    /// Start a fingerprint check for each request now in front that is
    /// approved by fingerprint
    fn sudo_arm(&mut self) -> Task<Message> {
        let ids: Vec<u64> = self
            .secrets
            .pending
            .values()
            .filter(|p| p.request.approval == cosmic_term::sudo_control::Approval::Fingerprint && !p.armed)
            .filter(|p| self.sudo_in_front(&p.request))
            .map(|p| p.request.id)
            .collect();
        for id in ids {
            if self.secrets.send(cosmic_term::sudo_control::ToAgent::Arm { id })
                && let Some(p) = self.secrets.pending.get_mut(&id)
            {
                p.armed = true;
            }
        }
        Task::none()
    }

    /// A desktop notification for a request out of view
    fn sudo_notify(&self, id: u64) -> Task<Message> {
        let Some(pending) = self.secrets.pending.get(&id) else {
            return Task::none();
        };
        if self.sudo_in_front(&pending.request) {
            return Task::none();
        }
        let request = &pending.request;
        let title = fl!("sudo-notify-title", host = request.host.as_str());
        let body = request.command.join(" ");
        cosmic::task::future(async move {
            let mut command = host::command("notify-send");
            command.args(["-a", "COSMIC Agent Terminal", "-i", "dialog-password", &title, &body]);
            let result = command
                .status()
                .await
                .map(|_| ())
                .map_err(|err| err.to_string());
            Message::SudoResult("notification".to_string(), result)
        })
    }

    fn sudo_toast(&mut self, error: String) -> Task<Message> {
        self.toasts
            .push(widget::toaster::Toast::new(fl!("sudo-failed", error = error.as_str())))
            .map(cosmic::Action::App)
    }

    /// What the Session menu offers for the tab in front
    fn session_menu(&self) -> menu::SessionMenu {
        if let Some(thread) = self.active_thread() {
            return menu::SessionMenu {
                session: false,
                agent: true,
                destinations: if thread.session_id.is_some() { self.destinations_from(&thread.host) } else { Vec::new() },
                thread: true,
            };
        }
        let session = self.active_session();
        menu::SessionMenu {
            session: session.is_some(),
            agent: session.is_some_and(|s| s.agent().is_some_and(|a| a != config::Agent::Shell)),
            destinations: session.map(|s| self.session_destinations(s)).unwrap_or_default(),
            thread: false,
        }
    }

    /// The session of the tab in front of the focused pane
    fn active_session(&self) -> Option<&agent::Session> {
        let tab_model = self.pane_model.active()?;
        tab_model.data::<agent::Session>(tab_model.active())
    }

    /// Sessions open in a tab of this window
    fn open_sessions(&self) -> Vec<agent::Session> {
        self.pane_model
            .panes
            .iter()
            .flat_map(|(_pane, tab_model)| {
                tab_model.iter().filter_map(|entity| {
                    tab_model.data::<agent::Session>(entity).cloned().or_else(|| {
                        // A thread's relay, which it shows
                        tab_model
                            .data::<thread::tab::ThreadTab>(entity)
                            .filter(|t| t.relay_live)
                            .and_then(|t| t.relay.clone())
                    })
                })
            })
            .collect()
    }

    /// The rows of the new tab menu: the profiles, by computer
    fn new_tab_entries(&self) -> Vec<NewTabEntry> {
        let default_profile = self.get_default_profile();
        let profile_names = self.config.profile_names();

        let mut entries = Vec::new();
        let section = |entries: &mut Vec<NewTabEntry>, title: String, items: Vec<NewTabEntry>| {
            if !items.is_empty() {
                entries.push(NewTabEntry::header(title));
                entries.extend(items);
            }
        };
        let profile_item = |name: &str, profile_id: ProfileId| NewTabEntry {
            kind: NewTabEntryKind::Item,
            label: name.to_string(),
            mark: self
                .config
                .profiles
                .get(&profile_id)
                .map(|profile| TabMark {
                    color: profile.tab_color(),
                    remote: profile.is_remote(),
                    disconnected: false,
                })
                .unwrap_or_default(),
            is_default: default_profile == Some(profile_id),
            message: Some(Message::ProfileOpen(profile_id)),
        };

        let local = profile_names
            .iter()
            .filter(|(_, id)| self.config.profiles.get(id).is_some_and(|p| !p.is_remote()))
            .map(|(name, id)| profile_item(name, *id))
            .collect();
        section(&mut entries, fl!("this-computer"), local);

        // Running sessions are in the sidebar; the menu offers profiles
        for host in self.session_hosts() {
            let profiles = profile_names
                .iter()
                .filter(|(_, id)| {
                    self.config
                        .profiles
                        .get(id)
                        .is_some_and(|p| p.host.trim() == host)
                })
                .map(|(name, id)| profile_item(name, *id))
                .collect();
            // Local profiles are listed first, under "This computer"
            if !host.is_empty() {
                section(&mut entries, fl!("on-host", host = host.as_str()), profiles);
            }
        }
        entries
    }

    /// The colour for a remote session: that of a profile for the same host
    /// and agent, or the agent's default. The user's own sessions are
    /// unmarked
    fn session_color(&self, session: &agent::Session) -> Option<TabColor> {
        let agent = session.agent().filter(|_| session.is_managed())?;
        self.config
            .profiles
            .values()
            .find(|p| p.agent == agent && p.host.trim() == session.host)
            .and_then(|p| p.tab_color())
            .or_else(|| agent.default_color())
    }

    fn get_default_profile(&self) -> Option<ProfileId> {
        self.config.default_profile
    }

    fn active_terminal_working_directory(&self) -> Option<PathBuf> {
        let tab_model = self.pane_model.active()?;
        let entity = tab_model.active();
        if let Some(thread) = tab_model.data::<thread::tab::ThreadTab>(entity) {
            return thread.host.is_empty().then(|| PathBuf::from(&thread.cwd));
        }
        // An agent works where it has moved to, not where its tab started
        if tab_model
            .data::<agent::Session>(entity)
            .is_some_and(agent::Session::is_local)
            && let Some(dir) = tab_model.data::<TabFocus>(entity).and_then(|f| f.dir.clone())
        {
            return Some(PathBuf::from(dir));
        }
        let terminal = tab_model.data::<Mutex<Terminal>>(entity)?;
        let terminal = terminal.lock().unwrap();
        terminal.working_directory()
    }

    fn create_and_focus_new_terminal(
        &mut self,
        pane: pane_grid::Pane,
        profile_id_opt: Option<ProfileId>,
        inherit_working_directory: bool,
    ) -> Task<Message> {
        self.create_and_focus_new_terminal_with(
            pane,
            profile_id_opt,
            inherit_working_directory,
            None,
        )
    }

    /// Create a terminal, running `prepared`, a session's command, if given
    /// instead of starting the profile
    fn create_and_focus_new_terminal_with(
        &mut self,
        pane: pane_grid::Pane,
        profile_id_opt: Option<ProfileId>,
        inherit_working_directory: bool,
        prepared: Option<(host::HostCommand, agent::Session)>,
    ) -> Task<Message> {
        let inherited_working_directory = inherit_working_directory
            .then(|| self.active_terminal_working_directory())
            .flatten();
        // Every tab runs in a session, so it can be reattached or moved to
        // another window, except a command given on the command line
        let (launch_command, session) = match prepared {
            Some((command, session)) => (Some(command), Some(session)),
            None if self.startup_command.is_some() => (None, None),
            None => {
                let profile = profile_id_opt
                    .and_then(|profile_id| self.config.profiles.get(&profile_id))
                    .cloned()
                    .unwrap_or_else(|| config::Profile::agent("", config::Agent::Shell, ""));
                // A local tab starts where asked on the command line, or
                // where the active tab is
                let dir = (!profile.is_remote())
                    .then(|| {
                        self.startup_options
                            .as_ref()
                            .and_then(|options| options.working_directory.clone())
                            .or_else(|| inherited_working_directory.clone())
                    })
                    .flatten();
                let (command, session) =
                    agent::launch(&profile, dir.as_deref().and_then(|dir| dir.to_str()));
                (Some(command), Some(session))
            }
        };
        let remote_session = session;
        let mark = TabMark {
            color: match (
                profile_id_opt.and_then(|id| self.config.profiles.get(&id)),
                &remote_session,
            ) {
                (Some(profile), _) => profile.tab_color(),
                (None, Some(session)) => self.session_color(session),
                (None, None) => None,
            },
            remote: remote_session
                .as_ref()
                .is_some_and(|session| !session.is_local()),
            disconnected: false,
        };
        self.pane_model.set_focus(pane);
        match &self.term_event_tx_opt {
            Some(term_event_tx) => {
                let color_scheme_kind = self.config.color_scheme_kind(self.core.system_theme());
                let colors = self
                    .themes
                    .get(&self.config.syntax_theme(color_scheme_kind, profile_id_opt))
                    .or_else(|| match color_scheme_kind {
                        ColorSchemeKind::Dark => self
                            .themes
                            .get(&(config::COSMIC_THEME_DARK.to_string(), ColorSchemeKind::Dark)),
                        ColorSchemeKind::Light => self.themes.get(&(
                            config::COSMIC_THEME_LIGHT.to_string(),
                            ColorSchemeKind::Light,
                        )),
                    });
                match colors {
                    Some(colors) => {
                        let current_pane = self.pane_model.focused();
                        if let Some(tab_model) = self.pane_model.active_mut() {
                            let command;
                            let (options, tab_title_override) = if let Some(profile) =
                                profile_id_opt
                                    .and_then(|profile_id| self.config.profiles.get(&profile_id))
                            {
                                // Merge profile and startup options, preferring startup options
                                let startup_options =
                                    self.startup_options.take().unwrap_or_default();
                                let is_remote = remote_session.is_some();
                                command = self
                                    .startup_command
                                    .take()
                                    .or(launch_command)
                                    .or_else(|| host::HostCommand::parse(&profile.command));
                                let options = tty::Options {
                                    // A remote profile's directory is on the remote host
                                    working_directory: startup_options.working_directory.or_else(
                                        || {
                                            (!is_remote)
                                                .then(|| {
                                                    inherited_working_directory.clone().or_else(
                                                        || {
                                                            (!profile.working_directory.is_empty())
                                                                .then(|| {
                                                                    profile
                                                                        .working_directory
                                                                        .clone()
                                                                        .into()
                                                                })
                                                        },
                                                    )
                                                })
                                                .flatten()
                                        },
                                    ),
                                    drain_on_exit: startup_options.drain_on_exit
                                        || profile.drain_on_exit,
                                    ..startup_options
                                };
                                let tab_title_override = if profile.tab_title.is_empty() {
                                    None
                                } else {
                                    Some(profile.tab_title.clone())
                                };
                                (options, tab_title_override)
                            } else {
                                let mut options = self.startup_options.take().unwrap_or_default();
                                command = self.startup_command.take().or(launch_command);
                                if remote_session.is_some() {
                                    options.working_directory = None;
                                } else if options.working_directory.is_none() {
                                    options.working_directory = inherited_working_directory.clone();
                                }
                                (options, None)
                            };
                            // Until the agent sets a title, name the tab after
                            // what it runs
                            let initial_title =
                                tab_title_override.clone().unwrap_or_else(|| {
                                    match (&remote_session, profile_id_opt) {
                                        (Some(session), _) if session.is_managed() => {
                                            session.label()
                                        }
                                        (Some(session), None) => session.label(),
                                        (_, Some(profile_id)) => self
                                            .config
                                            .profiles
                                            .get(&profile_id)
                                            .filter(|p| {
                                                p.agent != config::Agent::Shell || p.is_remote()
                                            })
                                            .map(|p| p.name.clone())
                                            .unwrap_or_else(|| fl!("new-terminal")),
                                        _ => fl!("new-terminal"),
                                    }
                                });

                            // A tab beside others will be drawn at their size
                            let initial_pixels = tab_model
                                .iter()
                                .find_map(|e| tab_model.data::<Mutex<Terminal>>(e))
                                .map(|terminal| {
                                    let size = terminal.lock().unwrap().size();
                                    (size.width, size.height)
                                })
                                .filter(|(width, height)| *width > 0 && *height > 0);
                            let mut builder = tab_model.insert().text(initial_title);
                            if let Some(icon) = mark.icon(16) {
                                builder = builder.icon(icon);
                            }
                            let entity = builder.closable().activate().id();
                            tab_model.data_set::<TabMark>(entity, mark);
                            let token = host::tab_token();
                            if let (Some(session), Some(launch)) = (&remote_session, &command)
                                && !session.is_local()
                            {
                                tab_model.data_set::<TabConnection>(
                                    entity,
                                    TabConnection {
                                        launch: launch.clone(),
                                        started: Instant::now(),
                                        lost: None,
                                    },
                                );
                            }
                            let options = host::tab_options(options, command, &token);
                            tab_model.data_set::<TabToken>(entity, TabToken(token));
                            if let Some(session) = remote_session.clone() {
                                tab_model.data_set::<agent::Session>(entity, session);
                            }
                            match Terminal::new(
                                current_pane,
                                entity,
                                term_event_tx.clone(),
                                self.term_config.clone(),
                                options,
                                &self.config,
                                *colors,
                                profile_id_opt,
                                tab_title_override,
                                initial_pixels,
                            ) {
                                Ok(mut terminal) => {
                                    terminal.set_config(
                                        &self.config,
                                        color_scheme_kind,
                                        &self.themes,
                                    );
                                    tab_model
                                        .data_set::<Mutex<Terminal>>(entity, Mutex::new(terminal));
                                }
                                Err(err) if profile_id_opt.is_some() => {
                                    // Create a tab without a profile if the selected
                                    // profile doesn't work
                                    let name = profile_id_opt
                                        .and_then(|id| self.config.profiles.get(&id))
                                        .map(|profile| profile.name.as_str())
                                        .unwrap_or_default();
                                    log::error!(
                                        "failed to open terminal with profile `{}`: {}",
                                        name,
                                        err
                                    );

                                    // TabClose focuses the nearest tab which would be incorrect
                                    // in this specific case as it would unfocus the new tab
                                    // created by TabNewNoProfile
                                    // TabClose can also cause the terminal app to close if it
                                    // closes the only open tab. This would close cosmic term
                                    // if launched with an invalid profile (issue #274)
                                    tab_model.remove(entity);
                                    return self.update(Message::TabNewNoProfile);
                                }
                                Err(err) => {
                                    log::error!("failed to open terminal: {}", err);
                                    // Clean up partially created tab
                                    return self.update(Message::TabClose(Some(entity)));
                                }
                            }
                        } else {
                            log::error!("Found no active pane");
                        }
                    }
                    None => {
                        log::error!(
                            "failed to find terminal theme {:?}",
                            self.config.syntax_theme(color_scheme_kind, profile_id_opt)
                        );
                        //TODO: fall back to known good theme
                    }
                }
            }
            None => {
                log::warn!("tried to create new tab before having event channel");
            }
        }
        self.update_title(Some(pane))
    }
}

/// Implement [`Application`] to integrate with COSMIC.
impl Application for App {
    /// Default async executor to use with the app.
    type Executor = executor::Default;

    /// Argument received
    type Flags = Flags;

    /// Message type specific to our [`App`].
    type Message = Message;

    /// The unique application ID to supply to the window manager.
    /// Distinct from upstream so this fork keeps its own config and can be
    /// installed alongside COSMIC Terminal
    const APP_ID: &'static str = "dev.twright.CosmicTermAgents";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    /// Creates the application, and optionally emits command on initialize.
    fn init(mut core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        core.window.content_container = false;
        core.window.show_headerbar = flags.config.show_headerbar;

        // Update font name from config
        {
            let mut font_system = font_system().write().unwrap();
            font_system
                .raw()
                .db_mut()
                .set_monospace_family(&flags.config.font_name);
            // The fonts threads are set in as papers
            for data in thread::FONTS {
                font_system.raw().db_mut().load_font_data(data.to_vec());
            }
        }
        // Thread images nobody has looked at for a while
        std::thread::spawn(thread::images::prune);

        let app_themes = vec![fl!("match-desktop"), fl!("dark"), fl!("light")];

        let font_name_faces_map = {
            let mut font_name_faces_map = BTreeMap::<_, Vec<_>>::new();
            let mut font_system = font_system().write().unwrap();
            //TODO: do not repeat, used in Tab::new
            for face in font_system.raw().db().faces() {
                // only monospace fonts and weights that match named constants.
                let weight = face.weight.0;
                if face.monospaced && { 1..9 }.contains(&{ weight / 100 }) && weight % 100 == 0 {
                    //TODO: get localized name if possible
                    let font_name = face
                        .families
                        .first()
                        .map_or_else(|| face.post_script_name.to_string(), |x| x.0.to_string());
                    font_name_faces_map
                        .entry(font_name)
                        .or_default()
                        .push(face.clone());
                }
            }

            // only keep fonts that have both NORMAL and BOLD weights with both having
            // a `Stretch::Normal` face.
            // This is important for fallbacks.
            font_name_faces_map.retain(|_, v| {
                let has_normal = v
                    .iter()
                    .any(|face| face.weight == Weight::NORMAL && face.stretch == Stretch::Normal);
                let has_bold = v
                    .iter()
                    .any(|face| face.weight == Weight::BOLD && face.stretch == Stretch::Normal);
                has_normal && has_bold
            });
            font_name_faces_map
        };

        if font_name_faces_map.is_empty() {
            log::error!(
                "at least one monospace font with normal/bold weights and default stretch is required"
            );
            log::error!("no monospace fonts to select from, exiting");
            process::exit(1);
        }

        let font_names = font_name_faces_map.keys().cloned().collect();

        let mut font_size_names = Vec::new();
        let mut font_sizes = Vec::new();
        for font_size in 4..=32 {
            font_size_names.push(format!("{font_size}px"));
            font_sizes.push(font_size);
        }

        let mut all_font_weights_vals_names_map = BTreeMap::new();

        macro_rules! populate_font_weights {
            ($($weight:ident,)+) => {
                // all weights
                paste::paste!{
                    $(
                        all_font_weights_vals_names_map
                            .insert(Weight::$weight.0, stringify!([<$weight:camel>]).into());
                    )+
                }
            };
        }

        populate_font_weights! {
            THIN, EXTRA_LIGHT, LIGHT, NORMAL, MEDIUM,
            SEMIBOLD, BOLD, EXTRA_BOLD, BLACK,
        };

        let mut all_font_stretches_vals_names_map = BTreeMap::new();

        macro_rules! populate_font_stretches {
            ($($stretch:ident,)+) => {
                // all stretches
                $(
                    all_font_stretches_vals_names_map
                        .insert(Stretch::$stretch, stringify!($stretch).into());
                )+
            };
        }

        populate_font_stretches! {
            UltraCondensed, ExtraCondensed, Condensed, SemiCondensed,
            Normal, SemiExpanded, Expanded, ExtraExpanded, UltraExpanded,
        };

        let mut zoom_step_names = Vec::new();
        let mut zoom_steps = Vec::new();
        for zoom_step in [25, 50, 75, 100, 150, 200] {
            zoom_step_names.push(format!("{}px", f32::from(zoom_step) / 100.0));
            zoom_steps.push(zoom_step);
        }

        let pane_model = TerminalPaneGrid::new(segmented_button::ModelBuilder::default().build());
        let mut terminal_ids = HashMap::new();
        terminal_ids.insert(pane_model.focused(), field_id("terminal"));

        let about = About::default()
            .name(fl!("cosmic-terminal"))
            .icon(widget::icon::from_name(Self::APP_ID))
            .version(env!("CARGO_PKG_VERSION"))
            .author("System76")
            .comments(fl!("comment"))
            .license("GPL-3.0-only")
            .license_url("https://spdx.org/licenses/GPL-3.0-only")
            .developers([("Jeremy Soller", "jeremy@system76.com")])
            .links([
                (fl!("repository"), "https://github.com/pop-os/cosmic-term"),
                (
                    fl!("support"),
                    "https://github.com/pop-os/cosmic-term/issues",
                ),
            ]);

        let key_binds = key_binds(&flags.shortcuts_config);
        let mut app = Self {
            core,
            about,
            pane_model,
            config_handler: flags.config_handler,
            config: flags.config.clone(),
            shortcuts_config: flags.shortcuts_config,
            key_binds,
            app_themes,
            font_names,
            font_size_names,
            font_sizes,
            font_name_faces_map,
            all_font_weights_vals_names_map,
            all_font_stretches_vals_names_map,
            curr_font_weight_names: Vec::new(),
            curr_font_weights: Vec::new(),
            curr_font_stretch_names: Vec::new(),
            curr_font_stretches: Vec::new(),
            zoom_step_names,
            zoom_steps,
            theme_names_dark: Vec::new(),
            theme_names_light: Vec::new(),
            themes: HashMap::new(),
            context_page: ContextPage::Settings,
            dialog_opt: None,
            agent_names: config::Agent::ALL.iter().map(|a| a.label()).collect(),
            host_names: Vec::new(),
            machine_expanded: None,
            local: None,
            machine_checks: HashMap::new(),
            discovery: None,
            color_names: std::iter::once(fl!("tab-color-default"))
                .chain(TabColor::PALETTE.iter().map(|(name, _)| name.to_string()))
                .collect(),
            new_tab_menu_open: false,
            new_tab_menu_in_sidebar: false,
            toasts: widget::toaster::Toasts::new(Message::CloseToast),
            sidebar: sidebar::Sidebar::default(),
            active_transfers: BTreeSet::new(),
            transfer_lookups: HashMap::new(),
            hovered_tab: None,
            secrets: secrets::Secrets::default(),
            secrets_paste_pane: None,
            secrets_focus: None,
            show_sidebar: flags.config.show_sidebar,
            tools_reported: BTreeSet::new(),
            mount_setup: BTreeMap::new(),
            mount_errors: BTreeMap::new(),
            host_setup: BTreeMap::new(),
            host_setup_stalled: BTreeSet::new(),
            pending_commands: BTreeMap::new(),
            host_toasts: BTreeMap::new(),
            window_focused: true,
            sudo_forwarded: HashMap::new(),
            rename: None,
            rename_thread: None,
            rename_id: field_id("rename"),
            accent: None,
            terminal_ids,
            find: false,
            find_search_id: field_id("find"),
            find_search_value: String::new(),
            startup_options: flags.startup_options,
            startup_command: flags.startup_command,
            startup_attach: flags.startup_attach,
            startup_thread: flags.startup_thread,
            restore_wanted: flags.restore,
            restore: None,
            tab_drag: None,
            reconnecting: BTreeMap::new(),
            tab_drag_left_window: false,
            term_config: flags.term_config,
            term_event_tx_opt: None,
            color_scheme_errors: Vec::new(),
            color_scheme_io: None,
            color_scheme_io_generation: 0,
            color_scheme_io_slow: false,
            color_scheme_expanded: None,
            color_scheme_renaming: None,
            color_scheme_rename_id: field_id("color-scheme-rename"),
            color_scheme_tab_model: widget::segmented_button::Model::default(),
            profile_expanded: None,
            show_advanced_font_settings: false,
            shortcut_capture: None,
            shortcut_conflict: None,
            shortcut_conflict_overlay_restore: None,
            shortcut_search_focus: Cell::new(true),
            shortcut_search_id: field_id("shortcut-search"),
            shortcut_search_regex: None,
            shortcut_search_value: String::new(),
            modifiers: Modifiers::empty(),
            next_thread: 1,
            image_windows: HashMap::new(),
            #[cfg(feature = "password_manager")]
            password_mgr: Default::default(),
        };

        app.set_curr_font_weights_and_stretches();
        // Sessions need the supervisor on their host for Ctrl+Z to work, and
        // transfers the transfer helper
        let setup_hosts = app.session_hosts();
        app.host_setup = setup_hosts.iter().map(|host| (host.clone(), None)).collect();
        for host in &setup_hosts { app.host_progress(host); }
        let supervisors = setup_hosts.into_iter().map(setup_host_task);
        let local = cosmic::task::future(async {
            Message::LocalProbed(agent::local_probe().await)
        });
        let sidebar = app.sidebar_start();
        // The sudo agent and its units, which systemd starts when needed
        let sudo_agent = cosmic::task::future(async {
            Message::SudoResult("installing the sudo agent".to_string(), agent::sudo::install_agent().await)
        });
        // Each machine and this computer learn how to mount each other, in
        // the background; mounts are made when something needs them
        let mounts = app.session_hosts().into_iter().filter(|host| !host.is_empty()).map(|host| {
            cosmic::task::future(async move {
                let result = agent::mounts::connect(host.clone()).await;
                Message::MountsConnected(host, result)
            })
        });
        let command = Task::batch(
            [app.update_config(), app.update_title(None), local, sidebar]
                .into_iter()
                .chain(supervisors)
                .chain(mounts)
                .chain([sudo_agent]),
        );

        (app, command)
    }

    //TODO: currently the first escape unfocuses, and the second calls this function
    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        self.image_windows.contains_key(&id).then_some(Message::ImageWindowClosed(id))
    }

    fn on_escape(&mut self) -> Task<Message> {
        if self.rename.take().is_some() {
            self.rename_thread = None;
            return self.update_focus();
        }
        if self.new_tab_menu_open {
            return self.update(Message::NewTabMenuClose);
        }
        if self.sidebar.searching {
            self.sidebar.searching = false;
            self.sidebar.filter.clear();
            return self.update_focus();
        }
        if self.core.window.show_context {
            // Handle keyboard shortcut page escape
            if let ContextPage::KeyboardShortcuts = self.context_page {
                // Cancel shortcut capture
                if self.shortcut_capture.take().is_some() {
                    return Task::none();
                }

                // Cancel shortcut conflict dialog
                if self.shortcut_conflict.take().is_some() {
                    return Task::none();
                }
            }

            return self.update(Message::ToggleContextPage(self.context_page));
        } else if self.find {
            // Close find if open
            self.find = false;
            self.find_search_value.clear();
        } else if let Some(id) = self.active_thread().filter(|t| t.is_working() && self.window_focused).map(|t| t.id) {
            // As in the CLI, Escape stops the agent
            return Task::batch([self.thread_message(id, thread::tab::Msg::Stop), self.update_focus()]);
        }

        // Focus correct widget
        self.update_focus()
    }

    fn on_context_drawer(&mut self) -> Task<Message> {
        if self.core.window.show_context {
            Task::none()
        } else {
            self.secrets_focus = None;
            #[cfg(feature = "password_manager")]
            if self.context_page == ContextPage::PasswordManager {
                self.password_mgr.clear();
            }
            self.update_focus()
        }
    }

    /// Handle application events here.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        // Helper for updating config values efficiently
        macro_rules! config_set {
            ($name: ident, $value: expr) => {
                match &self.config_handler {
                    Some(config_handler) => {
                        if let Err(err) =
                            paste::paste! { self.config.[<set_ $name>](config_handler, $value) }
                        {
                            log::warn!("failed to save config {:?}: {}", stringify!($name), err);
                        }
                    }
                    None => {
                        self.config.$name = $value;
                        log::warn!(
                            "failed to save config {:?}: no config handler",
                            stringify!($name)
                        );
                    }
                }
            };
        }
        match message {
            Message::AppTheme(app_theme) => {
                config_set!(app_theme, app_theme);
                return self.update_config();
            }
            Message::Thread(id, msg) => return self.thread_message(id, msg),
            Message::ContinueConversation { host, agent, cwd, conversation, thread } => {
                let pane = self.pane_model.focused();
                let Some(profile_id) = self.profile_on_host(&host, Some(agent), thread) else {
                    return self
                        .toasts
                        .push(widget::toaster::Toast::new(fl!("sidebar-no-profile", host = host.as_str())))
                        .map(cosmic::Action::App);
                };
                if thread {
                    return self.open_thread_as(pane, Some(profile_id), cwd, Some(conversation), None, Some(agent), Some(host));
                }
                // A terminal, through the launcher, which resumes it
                let opened = self.open_launcher(pane, profile_id);
                let Some(entity) = self.pane_model.active().map(|tab_model| tab_model.active()) else {
                    return opened;
                };
                return Task::batch([
                    opened,
                    self.update(Message::LauncherChoose(entity, LaunchChoice::Resume { id: conversation, cwd })),
                ]);
            }
            Message::RestoreSessions(sessions) => {
                let tasks: Vec<_> = sessions.into_iter().map(|s| self.update(Message::RemoteAttach(s))).collect();
                return Task::batch(tasks);
            }
            #[cfg(debug_assertions)]
            Message::DebugStep(index) => return self.debug_step(index),
            #[cfg(debug_assertions)]
            Message::DebugShot(name, shot, next) => return self.debug_shot(name, shot, next),
            Message::ThreadImagesPicked(id, result) => return self.images_picked(id, result),
            Message::ThreadPaper(paper) => self.set_thread_paper(paper),
            Message::ThreadPaperToggle => self.set_thread_paper(!self.config.thread_paper),
            Message::ThreadMatchColours(on) => {
                config_set!(thread_match_colours, on.unwrap_or(!self.config.thread_match_colours));
            }
            Message::ThreadCopy => {
                if let Some(task) = self.thread_copy() {
                    return task;
                }
            }
            Message::ThreadSelectAll => self.thread_select_all(),
            Message::ThreadSetting(setting) => return self.thread_setting(setting),
            Message::ThreadTick => {
                self.save_tabs();
                return self.enforce_thread_limits();
            }
            Message::ToggleDarkMode => {
                let dark = cosmic::theme::is_dark();
                config_set!(app_theme, if dark { AppTheme::Light } else { AppTheme::Dark });
                return self.update_config();
            }
            Message::OpenImage(path) => return self.open_image(path),
            Message::OpenDiagram(source) => return self.open_diagram(source),
            Message::Viewer(id, action) => {
                if let Some(window) = self.image_windows.get_mut(&id) {
                    return window.act(id, action);
                }
            }
            message @ (Message::ImageZoom(..)
            | Message::ImageCopyPath(_)
            | Message::ImageOpenExternally(_)
            | Message::ImageWindowClosed(_)
            | Message::Wheel(..)) => return self.image_message(message),
            Message::ClearScrollback(entity_opt) => {
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let terminal = terminal.lock().unwrap();
                        let mut term = terminal.term.lock();
                        term.grid_mut().clear_history();
                    }
                }
            }
            Message::ColorSchemeCollapse => {
                self.color_scheme_expanded = None;
            }
            Message::ColorSchemeDelete(color_scheme_kind, color_scheme_id) => {
                self.color_scheme_expanded = None;
                self.config
                    .color_schemes_mut(color_scheme_kind)
                    .remove(&color_scheme_id);
                return self.save_color_schemes(color_scheme_kind);
            }
            Message::ColorSchemeExport(color_scheme_kind, color_scheme_id_opt) => {
                if self.color_scheme_io.is_some() {
                    return Task::none();
                }
                self.color_scheme_expanded = None;
                if let Some(color_scheme_name) = match color_scheme_id_opt {
                    Some(color_scheme_id) => self
                        .config
                        .color_schemes(color_scheme_kind)
                        .get(&color_scheme_id)
                        .map(|color_scheme| color_scheme.name.clone()),
                    None => Some(format!("COSMIC {:?}", color_scheme_kind)),
                } && self.dialog_opt.is_none()
                {
                    let (dialog, command) = Dialog::new(
                        DialogSettings::new().kind(DialogKind::SaveFile {
                            filename: format!("{}.ron", color_scheme_name),
                        }),
                        |msg| Message::DialogMessage(Box::new(msg)),
                        move |result| {
                            Message::ColorSchemeExportResult(
                                color_scheme_kind,
                                color_scheme_id_opt,
                                result,
                            )
                        },
                    );
                    self.dialog_opt = Some(dialog);
                    return command;
                }
            }
            Message::ColorSchemeExportResult(color_scheme_kind, color_scheme_id_opt, result) => {
                self.dialog_opt = None;
                let DialogResult::Open(paths) = result else { return Task::none(); };
                let Some(path) = paths.into_iter().next() else { return Task::none(); };
                let scheme = match color_scheme_id_opt {
                    Some(id) => self.config.color_schemes(color_scheme_kind).get(&id).cloned(),
                    None => {
                        let name = format!("COSMIC {:?}", color_scheme_kind);
                        Some(match color_scheme_kind {
                            ColorSchemeKind::Dark => ColorScheme::from((name.as_str(), &terminal_theme::cosmic_dark())),
                            ColorSchemeKind::Light => ColorScheme::from((name.as_str(), &terminal_theme::cosmic_light())),
                        })
                    }
                };
                let Some(scheme) = scheme else { return Task::none(); };
                self.color_scheme_io = Some(fl!("color-scheme-exporting"));
                self.color_scheme_io_generation += 1;
                self.color_scheme_io_slow = false;
                let generation = self.color_scheme_io_generation;
                let work = cosmic::task::future(async move {
                    let result = blocking(move || {
                        let ron = ron::ser::to_string_pretty(&scheme, ron::ser::PrettyConfig::new())
                            .map_err(|err| err.to_string())?;
                        fs::write(&path, ron).map_err(|err| format!("{}: {err}", path.display()))
                    }).await;
                    Message::ColorSchemeExported(result)
                });
                return Task::batch([work, cosmic::task::future(async move {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    Message::ColorSchemeIoSlow(generation)
                })]);
            }
            Message::ColorSchemeExported(result) => {
                self.color_scheme_io = None;
                self.color_scheme_io_slow = false;
                if let Err(err) = result {
                    self.color_scheme_errors.push(err);
                }
            }
            Message::ColorSchemeExpand(color_scheme_kind, color_scheme_id_opt) => {
                self.color_scheme_expanded = Some((color_scheme_kind, color_scheme_id_opt));
            }
            Message::ColorSchemeImport(color_scheme_kind) => {
                if self.dialog_opt.is_none() && self.color_scheme_io.is_none() {
                    self.color_scheme_errors.clear();
                    let (dialog, command) = Dialog::new(
                        DialogSettings::new().kind(DialogKind::OpenMultipleFiles),
                        |msg| Message::DialogMessage(Box::new(msg)),
                        move |result| Message::ColorSchemeImportResult(color_scheme_kind, result),
                    );
                    self.dialog_opt = Some(dialog);
                    return command;
                }
            }
            Message::ColorSchemeImportResult(color_scheme_kind, result) => {
                self.dialog_opt = None;
                let DialogResult::Open(paths) = result else { return Task::none(); };
                self.color_scheme_errors.clear();
                self.color_scheme_io = Some(fl!("color-scheme-importing"));
                self.color_scheme_io_generation += 1;
                self.color_scheme_io_slow = false;
                let generation = self.color_scheme_io_generation;
                let work = cosmic::task::future(async move {
                    let results = blocking(move || {
                        Ok(paths.into_iter().map(|path| {
                            let file = fs::File::open(&path)
                                .map_err(|err| format!("Failed to open {path:?}: {err}"))?;
                            ron::de::from_reader::<_, ColorScheme>(file)
                                .map_err(|err| format!("Failed to parse {path:?}: {err}"))
                        }).collect::<Vec<_>>())
                    }).await.unwrap_or_else(|err| vec![Err(err)]);
                    Message::ColorSchemeImported(color_scheme_kind, results)
                });
                return Task::batch([work, cosmic::task::future(async move {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    Message::ColorSchemeIoSlow(generation)
                })]);
            }
            Message::ColorSchemeImported(color_scheme_kind, results) => {
                self.color_scheme_io = None;
                self.color_scheme_io_slow = false;
                for result in results {
                    match result {
                        Ok(color_scheme) => {
                            let id = self.config.color_schemes(color_scheme_kind)
                                .last_key_value()
                                .map(|(id, _)| ColorSchemeId(id.0 + 1))
                                .unwrap_or_default();
                            self.config.color_schemes_mut(color_scheme_kind).insert(id, color_scheme);
                        }
                        Err(err) => self.color_scheme_errors.push(err),
                    }
                }
                return self.save_color_schemes(color_scheme_kind);
            }
            Message::ColorSchemeIoSlow(generation) => {
                if self.color_scheme_io.is_some() && self.color_scheme_io_generation == generation {
                    self.color_scheme_io_slow = true;
                }
            }
            Message::ColorSchemeRename(color_scheme_kind, color_scheme_id, color_scheme_name) => {
                self.color_scheme_expanded = None;
                let focus = self.color_scheme_renaming.is_none();
                self.color_scheme_renaming =
                    Some((color_scheme_kind, color_scheme_id, color_scheme_name));
                if focus {
                    return widget::text_input::focus(self.color_scheme_rename_id.clone());
                }
            }
            Message::ColorSchemeRenameSubmit => {
                if let Some((color_scheme_kind, color_scheme_id, color_scheme_name)) =
                    self.color_scheme_renaming.take()
                    && let Some(color_scheme) = self
                        .config
                        .color_schemes_mut(color_scheme_kind)
                        .get_mut(&color_scheme_id)
                {
                    color_scheme.name = color_scheme_name;
                    return self.save_color_schemes(color_scheme_kind);
                }
            }
            Message::ColorSchemeTabActivate(entity) => {
                if let Some(color_scheme_kind) =
                    self.color_scheme_tab_model.data::<ColorSchemeKind>(entity)
                {
                    let context_page = ContextPage::ColorSchemes(*color_scheme_kind);
                    if self.context_page != context_page {
                        return self.update(Message::ToggleContextPage(context_page));
                    }
                }
            }
            Message::Config(config) => {
                if *config != self.config {
                    let shortcuts_changed = config.shortcuts_custom != self.config.shortcuts_custom;
                    log::info!("update config");
                    //TODO: update syntax theme by clearing tabs, only if needed
                    self.config = *config;
                    if shortcuts_changed {
                        self.shortcuts_config =
                            shortcuts::ShortcutsConfig::new(self.config.shortcuts_custom.clone());
                        self.key_binds = key_binds(&self.shortcuts_config);
                    }
                    return self.update_config();
                }
            }
            Message::Copy(entity_opt) => {
                if let Some(task) = self.thread_copy() {
                    return task;
                }
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let terminal = terminal.lock().unwrap();
                        let term = terminal.term.lock();
                        if let Some(text) = term.selection_to_string() {
                            return Task::batch([clipboard::write(text), self.update_focus()]);
                        }
                    }
                } else {
                    log::warn!("Failed to get focused pane");
                }
                return self.update_focus();
            }
            Message::CopyOrSigint(entity_opt) => {
                // A thread copies its selection, else stops its agent, as ^C would
                if let Some(id) = self.active_thread().filter(|_| entity_opt.is_none()).map(|t| t.id) {
                    if let Some(task) = self.thread_copy() {
                        return task;
                    }
                    if self.active_thread().is_some_and(|t| t.is_working()) {
                        return self.thread_message(id, thread::tab::Msg::Stop);
                    }
                    return Task::none();
                }
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let mut terminal = terminal.lock().unwrap();
                        let mut term = terminal.term.lock();
                        if let Some(text) = term.selection_to_string() {
                            // Clear selection (to allow next Ctrl+C to signal)
                            term.selection = None;
                            drop(term);
                            // Mark as dirty
                            terminal.needs_update = true;
                            drop(terminal);
                            return Task::batch([clipboard::write(text), self.update_focus()]);
                        } else {
                            // Drop the lock for term so that input_scroll doesn't block forever
                            drop(term);
                            // 0x03 is ^C
                            terminal.input_scroll(b"\x03".as_slice());
                        }
                    }
                } else {
                    log::warn!("Failed to get focused pane");
                }
                return self.update_focus();
            }
            Message::ToggleFullscreen => {
                if let Some(window_id) = self.core.main_window_id() {
                    return cosmic::command::toggle_maximize(window_id);
                }
            }
            Message::CopyPrimary(entity_opt) => {
                if entity_opt.is_none()
                    && let Some(task) = self.thread_drag_ended()
                {
                    return task;
                }
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let terminal = terminal.lock().unwrap();
                        let term = terminal.term.lock();
                        if let Some(text) = term.selection_to_string() {
                            return Task::batch([
                                clipboard::write_primary(text),
                                self.update_focus(),
                            ]);
                        }
                    }
                } else {
                    log::warn!("Failed to get focused pane");
                }
            }
            Message::DefaultFont(index) => {
                match self.font_names.get(index) {
                    Some(font_name) => {
                        if font_name != &self.config.font_name {
                            // Update font name from config
                            {
                                let mut font_system = font_system().write().unwrap();
                                font_system.raw().db_mut().set_monospace_family(font_name);
                            }
                            let panes: Vec<_> = self.pane_model.panes.iter().collect();
                            for (_pane, tab_model) in panes {
                                let entities: Vec<_> = tab_model.iter().collect();
                                for entity in entities {
                                    if let Some(terminal) =
                                        tab_model.data::<Mutex<Terminal>>(entity)
                                    {
                                        let mut terminal = terminal.lock().unwrap();
                                        terminal.update_cell_size();
                                    }
                                }
                            }

                            config_set!(font_name, font_name.to_string());
                            self.set_curr_font_weights_and_stretches();

                            return self.update_config();
                        }
                    }
                    None => {
                        log::warn!("failed to find font with index {}", index);
                    }
                }
            }
            Message::DefaultFontSize(index) => match self.font_sizes.get(index) {
                Some(font_size) => {
                    config_set!(font_size, *font_size);
                    self.reset_terminal_panes_zoom(); // reset zoom
                    return self.update_config();
                }
                None => {
                    log::warn!("failed to find font with index {}", index);
                }
            },
            Message::DefaultFontStretch(index) => match self.curr_font_stretches.get(index) {
                Some(font_stretch) => {
                    config_set!(font_stretch, font_stretch.to_number());
                    self.set_curr_font_weights_and_stretches();
                    return self.update_config();
                }
                None => {
                    log::warn!("failed to find font weight with index {}", index);
                }
            },
            Message::DefaultFontWeight(index) => match self.curr_font_weights.get(index) {
                Some(font_weight) => {
                    config_set!(font_weight, *font_weight);
                    return self.update_config();
                }
                None => {
                    log::warn!("failed to find font weight with index {}", index);
                }
            },
            Message::DefaultDimFontWeight(index) => match self.curr_font_weights.get(index) {
                Some(font_weight) => {
                    config_set!(dim_font_weight, *font_weight);
                    return self.update_config();
                }
                None => {
                    log::warn!("failed to find dim font weight with index {}", index);
                }
            },
            Message::DefaultBoldFontWeight(index) => match self.curr_font_weights.get(index) {
                Some(font_weight) => {
                    config_set!(bold_font_weight, *font_weight);
                    return self.update_config();
                }
                None => {
                    log::warn!("failed to find bold font weight with index {}", index);
                }
            },
            Message::DefaultZoomStep(index) => match self.zoom_steps.get(index) {
                Some(zoom_step) => {
                    config_set!(font_size_zoom_step_mul_100, *zoom_step);
                    self.reset_terminal_panes_zoom(); // reset zoom
                    return self.update_config();
                }
                None => {
                    log::warn!("failed to find zoom step with index {}", index);
                }
            },
            Message::DialogMessage(dialog_message) => {
                if let Some(dialog) = &mut self.dialog_opt {
                    // DialogMessage is boxed, so we need to dereference it before updating
                    return dialog.update(*dialog_message);
                }
            }
            Message::Drop(Some((pane, _entity, DndDrop {
                session: Some(session),
                ..
            }))) => {
                return self.receive_tab(pane, session);
            }
            Message::Drop(Some((pane, entity, data))) => {
                self.pane_model.set_focus(pane);
                // A thread attaches the images and is given the other paths
                let thread = self.pane_model.panes.get(pane).and_then(|t| t.data::<thread::tab::ThreadTab>(entity)).map(|t| t.id);
                if let Some(id) = thread {
                    let (images, others): (Vec<_>, Vec<_>) = data.paths.iter().cloned().partition(|path| {
                        path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                            matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif")
                        })
                    });
                    let mut tasks = vec![self.thread_message(id, thread::tab::Msg::Attached(images))];
                    if let Ok(value) = shlex::try_join(others.iter().filter_map(|p| p.to_str()))
                        && !value.is_empty()
                    {
                        tasks.push(self.update(Message::PasteValue(Some(entity), value)));
                    }
                    return Task::batch(tasks);
                }
                if let Ok(value) = shlex::try_join(data.paths.iter().filter_map(|p| p.to_str())) {
                    return Task::batch([
                        self.update_focus(),
                        cosmic::task::message(action::app(Message::PasteValue(
                            Some(entity),
                            value,
                        ))),
                    ]);
                }
            }
            Message::Drop(None) => {}
            Message::Find(find) => {
                self.find = find;
                if find {
                    if let Some(tab_model) = self.pane_model.active() {
                        let entity = tab_model.active();
                        if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                            let terminal = terminal.lock().unwrap();
                            let term = terminal.term.lock();
                            if let Some(text) = term.selection_to_string() {
                                self.find_search_value = text;
                            }
                        }
                    } else {
                        log::warn!("Failed to get focused pane");
                    }
                } else {
                    self.find_search_value.clear();
                }

                // Focus correct input
                return self.update_focus();
            }
            Message::FindNext if self.active_thread().is_some() => return self.thread_find(true, false),
            Message::FindPrevious if self.active_thread().is_some() => return self.thread_find(false, false),
            Message::FindNext => {
                if !self.find_search_value.is_empty()
                    && let Some(tab_model) = self.pane_model.active()
                {
                    let entity = tab_model.active();
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let mut terminal = terminal.lock().unwrap();
                        terminal.search(&self.find_search_value, true);
                    }
                }

                // Focus correct input
                return self.update_focus();
            }
            Message::FindPrevious => {
                if !self.find_search_value.is_empty()
                    && let Some(tab_model) = self.pane_model.active()
                {
                    let entity = tab_model.active();
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let mut terminal = terminal.lock().unwrap();
                        terminal.search(&self.find_search_value, false);
                    }
                }

                // Focus correct input
                return self.update_focus();
            }
            Message::FindSearchValueChanged(value) => {
                self.find_search_value = value;
                // A thread finds as the words are typed
                if self.active_thread().is_some() {
                    return self.thread_find(true, true);
                }
            }
            Message::MiddleClick(pane, entity_opt) => {
                self.pane_model.set_focus(pane);
                return Task::batch([
                    self.update_focus(),
                    clipboard::read_primary().map(move |value_opt| match value_opt {
                        Some(value) => action::app(Message::PasteValue(entity_opt, value)),
                        None => action::none(),
                    }),
                ]);
            }
            Message::FocusFollowMouse(focus_follow_mouse) => {
                config_set!(focus_follow_mouse, focus_follow_mouse);
            }
            Message::KeyUnclaimed(modifiers, physical, key, text) => {
                if let Some(task) = self.thread_type(modifiers, &key, text.as_deref()) {
                    return task;
                }
                return self.update(Message::Key(modifiers, physical, key));
            }
            Message::Key(modifiers, physical, key) => {
                // A thread copies its selection with Ctrl+C, as a document
                // does, and selects all of itself with Ctrl+Shift+A
                if self.active_thread().is_some()
                    && modifiers.control()
                    && let Key::Character(c) = &key
                {
                    if c.as_str().eq_ignore_ascii_case("c")
                        && let Some(task) = self.thread_copy()
                    {
                        return task;
                    }
                    if c.as_str().eq_ignore_ascii_case("a") && modifiers.shift() {
                        self.thread_select_all();
                        return Task::none();
                    }
                }
                // Page Up and Down page through a thread, wherever the keys are
                if let Some(id) = self.active_thread().map(|t| t.id)
                    && let Key::Named(named @ (Named::PageUp | Named::PageDown)) = key
                    && !modifiers.control()
                    && !modifiers.alt()
                {
                    let pages = if named == Named::PageUp { -1.0 } else { 1.0 };
                    return self.thread_message(id, thread::tab::Msg::Page(pages));
                }
                // Hard-coded keys
                match key {
                    Key::Named(Named::Copy) => {
                        return self.update(Message::Copy(None));
                    }
                    Key::Named(Named::Paste) => {
                        return self.update(Message::Paste(None));
                    }
                    Key::Named(Named::Escape) => {
                        // Handled by on_escape
                        return Task::none();
                    }
                    _ => {}
                }

                // Handle shortcut capture
                if let Some(action) = self.shortcut_capture {
                    if let Some(binding) = shortcuts::binding_from_key(modifiers, key) {
                        self.shortcut_capture = None;
                        if let Some(existing_action) =
                            self.shortcuts_config.action_for_binding(&binding)
                        {
                            if existing_action != action {
                                self.begin_shortcut_conflict(ShortcutConflict {
                                    binding,
                                    existing_action,
                                    new_action: action,
                                });
                                return Task::none();
                            }
                            return Task::none();
                        }
                        self.apply_shortcut_binding(binding, action);
                    }
                    return Task::none();
                }

                // Handle configurable keys
                for (key_bind, action) in &self.key_binds {
                    if key_bind.matches(modifiers, &key, Some(&physical)) {
                        return self.update(action.message(None));
                    }
                }
            }
            Message::LaunchUrl(url) => {
                if let Err(err) = open::that_detached(&url) {
                    log::warn!("failed to open {:?}: {}", url, err);
                }
            }
            Message::CopyUrlByMenu => {
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = tab_model.active();
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let mut terminal = terminal.lock().unwrap();
                        if let Some(url) =
                            terminal.context_menu.as_ref().and_then(|m| m.link.as_ref())
                        {
                            let url = url.to_owned();
                            terminal.context_menu = None;
                            terminal.active_regex_match = None;
                            terminal.needs_update = true;

                            return Task::batch([clipboard::write(url), self.update_focus()]);
                        }
                    }
                }
            }
            Message::LaunchUrlByMenu => {
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = tab_model.active();
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let mut terminal = terminal.lock().unwrap();
                        if let Some(url) =
                            terminal.context_menu.as_ref().and_then(|m| m.link.as_ref())
                            && let Err(err) = open::that_detached(url)
                        {
                            log::warn!("failed to open {:?}: {}", url, err);
                        }
                        terminal.context_menu = None;
                        terminal.active_regex_match = None;
                        terminal.needs_update = true;
                    }
                }
            }
            Message::OpenLocallyByMenu => {
                if self.pending_commands.contains_key("open-locally") {
                    return Task::none();
                }
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = tab_model.active();
                    let session = tab_model.data::<agent::Session>(entity).cloned();
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let mut terminal = terminal.lock().unwrap();
                        let target = terminal
                            .context_menu
                            .as_ref()
                            .and_then(|state| state.link.clone().or_else(|| state.word.clone()));
                        terminal.context_menu = None;
                        terminal.active_regex_match = None;
                        terminal.needs_update = true;
                        drop(terminal);
                        if let (Some(session), Some(target)) = (session, target) {
                            self.start_progress("open-locally", fl!("opening-locally"));
                            return cosmic::task::future(async move {
                                Message::OpenLocallyResult(
                                    agent::open_locally(session, target).await,
                                )
                            });
                        }
                    }
                }
            }
            Message::OpenLocallyResult(result) => {
                self.finish_progress("open-locally");
                if let Err(err) = result {
                    log::warn!("failed to open locally: {}", err);
                    return self.file_open_error(fl!("open-locally-failed", error = err.as_str()), err);
                }
            }
            Message::Modifiers(modifiers) => {
                self.modifiers = modifiers;
                thread::rich::set_shift(modifiers.shift());
            }
            Message::MouseEnter(pane) => {
                self.pane_model.set_focus(pane);
                return self.update_focus();
            }
            Message::ShortcutCaptureCancel => {
                self.shortcut_capture = None;
            }
            Message::ShortcutCaptureStart(action) => {
                self.shortcut_capture = Some(action);
            }
            Message::ShortcutConflictCancel => {
                self.clear_shortcut_conflict();
            }
            Message::ShortcutConflictReplace => {
                if let Some(conflict) = self.shortcut_conflict.clone() {
                    self.apply_shortcut_binding(conflict.binding, conflict.new_action);
                }
                self.clear_shortcut_conflict();
            }
            Message::ShortcutRemove(binding, source) => {
                match source {
                    shortcuts::BindingSource::Default => {
                        self.shortcuts_config
                            .custom
                            .0
                            .insert(binding, shortcuts::KeyBindAction::Disable);
                    }
                    shortcuts::BindingSource::Custom => {
                        self.shortcuts_config.custom.0.remove(&binding);
                    }
                }
                self.save_shortcuts_custom();
            }
            Message::ShortcutReset(reset_action) => {
                self.shortcuts_config.reset_action(reset_action);
                self.save_shortcuts_custom();
            }
            Message::ShortcutSearch(search) => {
                self.shortcut_search_focus.set(true);
                self.shortcut_search_regex = None;
                if !search.is_empty() {
                    let pattern = regex::escape(&search);
                    match regex::RegexBuilder::new(&pattern)
                        .case_insensitive(true)
                        .build()
                    {
                        Ok(regex) => {
                            self.shortcut_search_regex = Some(regex);
                        }
                        Err(err) => {
                            log::warn!("failed to parse regex {:?}: {}", pattern, err);
                        }
                    };
                }
                self.shortcut_search_value = search;
                return self.update_focus();
            }
            Message::Opacity(opacity) => {
                config_set!(opacity, cmp::min(100, opacity));
            }
            Message::PaneClicked(pane) => {
                // A click in the thread in front has put the focus where it
                // belongs, in the composer or out of it to select the
                // thread's text; refocusing would undo it
                if pane == self.pane_model.focused() && self.active_thread().is_some() {
                    return Task::none();
                }
                self.pane_model.set_focus(pane);
                let title_task = self.update_title(Some(pane));
                return Task::batch([self.update_focus(), title_task]);
            }
            Message::PaneSplit(axis) => {
                let from = self.active_thread().map(thread::app::ThreadOrigin::of);
                let result = self.pane_model.panes.split(
                    axis,
                    self.pane_model.focused(),
                    segmented_button::ModelBuilder::default().build(),
                );
                if let Some((pane, _)) = result {
                    self.terminal_ids.insert(pane, field_id("terminal"));
                    self.pane_model.panes_created += 1;
                    // What a new tab would be: beside a thread, another
                    let command = self.new_tab_in(pane, from);
                    return command;
                }
            }
            Message::PaneToggleMaximized => {
                if self.pane_model.panes.maximized().is_some() {
                    self.pane_model.panes.restore();
                } else {
                    self.pane_model.panes.maximize(self.pane_model.focused());
                }
                return self.update_focus();
            }
            Message::PaneFocusAdjacent(direction) => {
                if let Some(adjacent) = self
                    .pane_model
                    .panes
                    .adjacent(self.pane_model.focused(), direction)
                {
                    self.pane_model.set_focus(adjacent);
                    return self.update_title(Some(adjacent));
                }
            }
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.pane_model.panes.resize(split, ratio);
            }
            Message::PaneDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.pane_model.panes.drop(pane, target);
            }
            Message::PaneDragged(_) => {}
            #[cfg(feature = "password_manager")]
            Message::PasswordManager(msg) => {
                return self.password_mgr.update(msg);
            }
            #[cfg(feature = "password_manager")]
            Message::PasswordPaste(password, pane) => {
                if let Some(tab_model) = self.pane_model.panes.get(pane) {
                    let entity = tab_model.active();
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let terminal = terminal.lock().unwrap();
                        terminal.paste(password.into_unsecure());
                        terminal.input_scroll(b"\n".as_slice());
                        self.core.window.show_context = false;
                        self.password_mgr.clear();
                    }
                }
            }
            Message::Paste(entity_opt) => {
                return clipboard::read().map(move |value_opt| match value_opt {
                    Some(value) => action::app(Message::PasteValue(entity_opt, value)),
                    None => action::none(),
                });
            }
            Message::PastePrimary(entity_opt) => {
                return clipboard::read_primary().map(move |value_opt| match value_opt {
                    Some(value) => action::app(Message::PasteValue(entity_opt, value)),
                    None => action::none(),
                });
            }
            Message::PasteValue(entity_opt, value) => {
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    // Into a thread's composer
                    if let Some(id) = tab_model.data::<thread::tab::ThreadTab>(entity).map(|t| t.id) {
                        use widget::text_editor::{Action, Edit};
                        return self.thread_message(
                            id,
                            thread::tab::Msg::Edit(Action::Edit(Edit::Paste(std::sync::Arc::new(value)))),
                        );
                    }
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let terminal = terminal.lock().unwrap();
                        terminal.paste(value);
                    }
                }
                return self.update_focus();
            }
            Message::ProfileCollapse(_profile_id) => {
                self.profile_expanded = None;
            }
            Message::ProfileCommand(profile_id, text) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    profile.command = text;
                    return self.save_profiles();
                }
            }
            Message::ProfileDirectory(profile_id, text) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    profile.working_directory = text;
                    return self.save_profiles();
                }
            }
            Message::ProfileExpand(profile_id) => {
                self.profile_expanded = Some(profile_id);
            }
            Message::ProfileHold(profile_id, drain_on_exit) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    profile.drain_on_exit = drain_on_exit;
                    return self.save_profiles();
                }
            }
            Message::ProfileName(profile_id, text) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    profile.name = text;
                    return self.save_profiles();
                }
            }
            Message::ProfileNew => {
                // Get next profile ID
                let profile_id = self
                    .config
                    .profiles
                    .last_key_value()
                    .map(|(id, _)| ProfileId(id.0 + 1))
                    .unwrap_or_default();
                self.config.profiles.insert(profile_id, Profile::default());
                self.profile_expanded = Some(profile_id);
                return self.save_profiles();
            }
            Message::SidebarNewMenu => {
                let open = self.new_tab_menu_open && self.new_tab_menu_in_sidebar;
                self.new_tab_menu_open = false;
                if open {
                    return self.update(Message::NewTabMenuClose);
                }
                let task = self.update(Message::NewTabMenuToggle);
                self.new_tab_menu_in_sidebar = true;
                return task;
            }
            Message::SidebarPeerFound(session, found) => match found {
                // Open it here straight away, with a bar offering the other
                // window instead
                Ok(Some(peer)) => {
                    let attached = self.update(Message::RemoteAttach(session.clone()));
                    if let Some((pane, entity)) = self.session_tab(&session)
                        && let Some(tab_model) = self.pane_model.panes.get_mut(pane)
                    {
                        tab_model.data_set::<TabShared>(entity, TabShared { peer });
                    }
                    return attached;
                }
                // Attached on another computer, or in a window that cannot
                // say: open it here too
                result => {
                    if let Err(err) = result {
                        log::warn!("could not ask the other windows: {err}");
                    }
                    return self.update(Message::RemoteAttach(session));
                }
            },
            Message::SharedGoThere(entity) => {
                let Some((pane, peer, session)) = self.shared_tab(entity) else {
                    return Task::none();
                };
                let descriptor = session.descriptor();
                // The copy here goes, leaving the session to the other window
                self.pane_model.set_focus(pane);
                let closed = self.update(Message::TabClose(Some(entity)));
                // Wayland raises a window only with a token from the one in
                // front
                let raise = cosmic::iced::platform_specific::shell::commands::activation::request_token(
                    Some(Self::APP_ID.to_string()),
                    self.core.main_window_id(),
                )
                .map(move |token| {
                    cosmic::Action::App(Message::PeerFocusToken(peer.clone(), descriptor.clone(), token))
                });
                return Task::batch([raise, closed]);
            }
            Message::SharedMoveHere(entity) => {
                let Some((pane, peer, session)) = self.shared_tab(entity) else {
                    return Task::none();
                };
                if let Some(tab_model) = self.pane_model.panes.get_mut(pane) {
                    tab_model.data_remove::<TabShared>(entity);
                }
                let descriptor = session.descriptor();
                return cosmic::task::future(async move {
                    Message::PeerAsked(peers::release(peer, descriptor).await)
                });
            }
            Message::SharedDismiss(entity) => {
                if let Some(pane) = self.tab_pane(entity)
                    && let Some(tab_model) = self.pane_model.panes.get_mut(pane)
                {
                    tab_model.data_remove::<TabShared>(entity);
                }
            }
            Message::PeerFocusToken(peer, descriptor, token) => {
                return cosmic::task::future(async move {
                    Message::PeerAsked(peers::focus(peer, descriptor, token.unwrap_or_default()).await)
                });
            }
            Message::PeerAsked(result) => {
                if let Err(err) = result {
                    log::warn!("another window did not answer: {err}");
                }
            }
            Message::SessionRename(entity_opt) => {
                let thread = self.pane_model.active().and_then(|tab_model| {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    tab_model.data::<thread::tab::ThreadTab>(entity).map(|t| (t.id, t.host.clone(), t.title.clone()))
                });
                if let Some((id, host, title)) = thread {
                    self.rename_thread = Some(id);
                    self.rename = Some((agent::Session { host, name: String::new() }, title));
                    return Task::batch([
                        widget::text_input::focus(self.rename_id.clone()),
                        widget::text_input::select_all(self.rename_id.clone()),
                    ]);
                }
                let session = self.pane_model.active().and_then(|tab_model| {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    tab_model.data::<agent::Session>(entity).cloned()
                });
                if let Some(session) = session.filter(|s| s.agent().is_some_and(|a| a != config::Agent::Shell)) {
                    let current = self.sidebar.title(&session).unwrap_or_default().to_string();
                    self.rename = Some((session, current));
                    return Task::batch([
                        widget::text_input::focus(self.rename_id.clone()),
                        widget::text_input::select_all(self.rename_id.clone()),
                    ]);
                }
            }
            Message::RenameInput(name) => {
                if let Some((_, draft)) = &mut self.rename {
                    *draft = name;
                }
            }
            Message::RenameSubmit => {
                if self.pending_commands.contains_key("rename") {
                    return Task::none();
                }
                let Some((session, name)) = self.rename.take() else {
                    return Task::none();
                };
                let name = name.trim().to_string();
                if let Some(id) = self.rename_thread.take() {
                    return self.rename_thread_to(id, name);
                }
                let Some(kind) = session.agent().filter(|_| !name.is_empty()) else {
                    return self.update_focus();
                };
                self.sidebar.set_title(&session, &name);
                self.start_progress("rename", fl!("renaming-conversation"));
                // The agent retitles its tab too, a moment later
                let retitle = match self.session_tab(&session) {
                    Some((pane, entity)) => {
                        if let Some(tab_model) = self.pane_model.panes.get_mut(pane) {
                            tab_model.text_set(entity, name.clone());
                        }
                        self.update_title(None)
                    }
                    None => Task::none(),
                };
                return Task::batch([
                    retitle,
                    self.update_focus(),
                    cosmic::task::future(async move {
                        Message::Renamed(agent::rename_conversation(session, kind, Some(name)).await)
                    }),
                ]);
            }
            Message::RenameCancel => {
                self.rename = None;
                self.rename_thread = None;
                return self.update_focus();
            }
            Message::Renamed(result) => {
                self.finish_progress("rename");
                if let Err(err) = result {
                    return self
                        .toasts
                        .push(widget::toaster::Toast::new(fl!(
                            "rename-failed",
                            error = err.as_str()
                        )))
                        .map(cosmic::Action::App);
                }
            }
            Message::SudoConnected(agent) => {
                self.secrets.agent = Some(agent);
                self.secrets.send(cosmic_term::sudo_control::ToAgent::Status);
            }
            Message::SudoDisconnected => {
                self.secrets.agent = None;
                self.secrets_focus = None;
                self.secrets.pending.clear();
                self.secrets.state = Default::default();
                return self.retitle_agent_tabs();
            }
            Message::SudoEvent(event) => {
                let was_unlocked = self.secrets.state.unlocked;
                let new = self.secrets.receive(&event);
                let mut tasks = vec![self.retitle_agent_tabs(), self.sudo_arm()];
                // A field that goes away takes the focus with it, without
                // saying so: give the keys back to the terminal
                let gone = match &event {
                    cosmic_term::sudo_control::FromAgent::Done { .. } => self.secrets_focus.as_ref().is_some_and(|focus| {
                        !self.secrets.pending.values().any(|p| &p.input == focus)
                            && ![&self.secrets.password_id, &self.secrets.again_id, &self.secrets.ask_id]
                                .into_iter()
                                .chain(&self.secrets.form_ids)
                                .any(|id| id == focus)
                    }),
                    cosmic_term::sudo_control::FromAgent::State(state) => {
                        state.unlocked != was_unlocked
                            && self.secrets_focus.as_ref().is_some_and(|focus| {
                                [&self.secrets.password_id, &self.secrets.again_id].contains(&focus)
                            })
                    }
                    // A wrong password: its field is back, to type again
                    cosmic_term::sudo_control::FromAgent::Retry { id, .. } => {
                        if let Some(p) = self.secrets.pending.get(id) {
                            tasks.push(self.update(Message::SecretsFocus(p.input.clone())));
                        }
                        false
                    }
                    _ => false,
                };
                if gone {
                    self.secrets_focus = None;
                    tasks.push(self.update_focus());
                }
                if let Some(id) = new {
                    tasks.push(self.sudo_notify(id));
                    // A request from a session no tab here shows: its popup's
                    // field takes the keys
                    if let Some(p) = self.secrets.pending.get(&id) {
                        let shown = p.request.session.as_ref().is_some_and(|session| {
                            self.open_sessions().iter().any(|s| &s.name == session)
                        });
                        if !shown && p.request.approval == cosmic_term::sudo_control::Approval::Password {
                            tasks.push(self.update(Message::SecretsFocus(p.input.clone())));
                        }
                    }
                }
                match event {
                    cosmic_term::sudo_control::FromAgent::Generated(info) => {
                        if let Some((dest, machine)) = self.secrets.setting_up.take()
                            && machine == info.host
                        {
                            tasks.push(cosmic::task::future(async move {
                                let result = agent::sudo::prepare_setup(dest.clone(), info.public).await;
                                Message::SudoPrepared(dest, result)
                            }));
                        }
                    }
                    // Said, as it was asked of no one
                    cosmic_term::sudo_control::FromAgent::AutoApproved(request) => {
                        let command = request.command.join(" ");
                        tasks.push(
                            self.toasts
                                .push(widget::toaster::Toast::new(fl!(
                                    "sudo-auto-approved",
                                    host = request.host.as_str(),
                                    command = command.as_str()
                                )))
                                .map(cosmic::Action::App),
                        );
                    }
                    cosmic_term::sudo_control::FromAgent::Secret { secret, .. } => {
                        if let Some(pane) = self.secrets_paste_pane.take() {
                            tasks.push(self.update(Message::PasswordPaste(secret.into(), pane)));
                        }
                    }
                    _ => {}
                }
                return Task::batch(tasks);
            }
            Message::SudoPassword(id, password) => {
                if let Some(p) = self.secrets.pending.get_mut(&id) {
                    p.password = password;
                }
            }
            Message::SudoApprove(id) => {
                if let Some(p) = self.secrets.pending.get_mut(&id) {
                    // The store password, when one is typed, whatever else
                    // the request offers
                    let password = (!p.password.is_empty()).then(|| std::mem::take(&mut p.password));
                    p.error = None;
                    p.checking = password.is_some();
                    self.secrets.send(cosmic_term::sudo_control::ToAgent::Approve { id, password });
                }
            }
            Message::SudoDeny(id) => {
                self.secrets.send(cosmic_term::sudo_control::ToAgent::Deny { id });
            }
            Message::SudoRedraw => {}
            Message::SudoSetup(dest, machine) => {
                self.secrets.setting_up = Some((dest, machine.clone()));
                self.secrets.send(cosmic_term::sudo_control::ToAgent::SetUpMachine { host: machine });
            }
            Message::SudoPrepared(dest, result) => match result {
                // The one ordinary sudo: in a tab on the machine, where the
                // user types the password for real
                Ok(command) => {
                    let tab = agent::command_tab(&dest, &command);
                    return self.create_and_focus_new_terminal_with(
                        self.pane_model.focused(),
                        None,
                        false,
                        Some(tab),
                    );
                }
                Err(err) => return self.sudo_toast(err),
            },
            Message::SudoFinger(index) => {
                self.secrets.finger = index;
            }
            Message::SudoEnrol => {
                let finger = secrets::FINGERS[self.secrets.finger.min(secrets::FINGERS.len() - 1)];
                self.secrets.enrolment = Some(fl!("sudo-enrol-touch"));
                self.secrets.send(cosmic_term::sudo_control::ToAgent::Enrol {
                    finger: finger.to_string(),
                });
            }
            Message::SecretsToggle => {
                let task = self.update(Message::ToggleContextPage(ContextPage::Secrets));
                if self.core.window.show_context && self.context_page == ContextPage::Secrets {
                    self.secrets.send(cosmic_term::sudo_control::ToAgent::Status);
                    // Only a locked store has a field to type in
                    if !self.secrets.state.unlocked {
                        self.secrets_focus = Some(self.secrets.password_id.clone());
                        return Task::batch([task, widget::text_input::focus(self.secrets.password_id.clone())]);
                    }
                } else {
                    self.secrets_focus = None;
                }
                return task;
            }
            Message::TerminalClickFocus(pane) => {
                // Whatever field had the keys has lost them, and so has every
                // other terminal: the widget took them itself, so the focus
                // is moved properly here, as a click on the pane does
                self.secrets_focus = None;
                self.pane_model.set_focus(pane);
                return self.update_focus();
            }
            Message::SecretsUnfocus(id) => {
                // Another field may have the focus already, as the field
                // that lost it can say so after
                if self.secrets_focus.as_ref() == Some(&id) {
                    self.secrets_focus = None;
                }
            }
            Message::SecretsFocus(id) => {
                // Focusing the field through the app takes the focus from
                // the terminal, which a click on the field alone does not
                self.secrets_focus = Some(id.clone());
                return widget::text_input::focus(id);
            }
            Message::SecretsPassword(password) => self.secrets.password = password,
            Message::SecretsAgain(again) => self.secrets.again = again,
            Message::SecretsCreate => {
                if !self.secrets.password.is_empty() && self.secrets.password == self.secrets.again {
                    let password = std::mem::take(&mut self.secrets.password);
                    self.secrets.again.clear();
                    self.secrets.error = None;
                    self.secrets.send(cosmic_term::sudo_control::ToAgent::Create { password });
                }
            }
            Message::SecretsUnlock => {
                let password = std::mem::take(&mut self.secrets.password);
                self.secrets.error = None;
                self.secrets.send(cosmic_term::sudo_control::ToAgent::Unlock { password });
            }
            Message::SecretsLock => {
                self.secrets.send(cosmic_term::sudo_control::ToAgent::Lock);
            }
            Message::SecretsPaste(id) => {
                self.secrets_paste_pane = Some(self.pane_model.focused());
                self.secrets.error = None;
                let entry = self.secrets.state.entries.iter().find(|e| e.id == id);
                let asks = entry.is_some_and(|e| e.always_ask)
                    || self.secrets.policy().paste.approval == cosmic_term::sudo_control::ApprovalRule::Password;
                if asks {
                    self.secrets.asking = Some(secrets::Asking { id, password: String::new() });
                    // As from the right-click menu: the pane shows the ask
                    let mut tasks = Vec::new();
                    if !(self.core.window.show_context && self.context_page == ContextPage::Secrets) {
                        tasks.push(self.update(Message::ToggleContextPage(ContextPage::Secrets)));
                    }
                    tasks.push(self.update(Message::SecretsFocus(self.secrets.ask_id.clone())));
                    return Task::batch(tasks);
                } else {
                    self.secrets.send(cosmic_term::sudo_control::ToAgent::Reveal { id, password: None });
                }
            }
            Message::SecretsAskPassword(password) => {
                if let Some(asking) = &mut self.secrets.asking {
                    asking.password = password;
                }
            }
            Message::SecretsAskSubmit => {
                if let Some(asking) = self.secrets.asking.take() {
                    self.secrets.send(cosmic_term::sudo_control::ToAgent::Reveal {
                        id: asking.id,
                        password: Some(asking.password),
                    });
                }
            }
            Message::SecretsAdd(host) => {
                self.secrets.form = Some(secrets::Form {
                    id: None,
                    edit: cosmic_term::sudo_control::EntryEdit { host, ..Default::default() },
                });
            }
            Message::SecretsEdit(id) => {
                self.secrets.form = id.and_then(|id| {
                    let entry = self.secrets.state.entries.iter().find(|e| e.id == id)?;
                    Some(secrets::Form {
                        id: Some(id),
                        edit: cosmic_term::sudo_control::EntryEdit {
                            host: entry.host.clone(),
                            label: entry.label.clone(),
                            note: entry.note.clone(),
                            secret: String::new(),
                            always_ask: entry.always_ask,
                        },
                    })
                });
            }
            Message::SecretsFormLabel(text) => {
                if let Some(form) = &mut self.secrets.form {
                    form.edit.label = text;
                }
            }
            Message::SecretsFormSecret(text) => {
                if let Some(form) = &mut self.secrets.form {
                    form.edit.secret = text;
                }
            }
            Message::SecretsFormNote(text) => {
                if let Some(form) = &mut self.secrets.form {
                    form.edit.note = text;
                }
            }
            Message::SecretsFormAlwaysAsk(on) => {
                if let Some(form) = &mut self.secrets.form {
                    form.edit.always_ask = on;
                }
            }
            Message::SecretsSave => {
                if let Some(form) = self.secrets.form.take() {
                    self.secrets.send(match form.id {
                        Some(id) => cosmic_term::sudo_control::ToAgent::EditEntry { id, edit: form.edit },
                        None => cosmic_term::sudo_control::ToAgent::AddEntry(form.edit),
                    });
                }
            }
            Message::SecretsDelete(id) => {
                self.secrets.send(cosmic_term::sudo_control::ToAgent::DeleteEntry { id });
            }
            Message::SecretsPolicy(policy) => {
                self.secrets.send(cosmic_term::sudo_control::ToAgent::SetPolicy(policy));
            }
            Message::SecretsImport => {
                #[cfg(feature = "password_manager")]
                return cosmic::task::future(async {
                    Message::SecretsImported(
                        password_manager::store::export_all().await.map_err(|err| err.to_string()),
                    )
                });
            }
            Message::SecretsImported(result) => match result {
                Ok(found) => {
                    for (name, secret) in found {
                        self.secrets.send(cosmic_term::sudo_control::ToAgent::AddEntry(
                            cosmic_term::sudo_control::EntryEdit {
                                host: None,
                                label: name,
                                note: fl!("secrets-imported"),
                                secret,
                                always_ask: false,
                            },
                        ));
                    }
                }
                Err(err) => self.secrets.error = Some(err),
            },
            Message::SudoResult(what, result) => {
                if let Err(err) = result {
                    log::warn!("{what}: {err}");
                }
            }
            Message::RemoteFilesOpen(host) => {
                let show = if self.core.window.show_context && self.context_page == ContextPage::RemoteFiles {
                    Task::none()
                } else {
                    self.update(Message::ToggleContextPage(ContextPage::RemoteFiles))
                };
                let mut tasks = vec![show];
                let mut hosts: BTreeSet<_> = self.mount_setup_machines().into_iter().map(|(host, _)| host).collect();
                hosts.insert(host);
                for host in hosts {
                    if !self.mount_setup.contains_key(&host) {
                        tasks.push(self.update(Message::RemoteFilesCheck(host)));
                    }
                }
                return Task::batch(tasks);
            }
            Message::RemoteFilesCheck(host) => {
                let request_id = cosmic_term::random_hex(12);
                self.mount_setup.insert(host.clone(), mount_setup_view::Check {
                    request_id: request_id.clone(), result: None,
                });
                let check_host = host.clone();
                let connection_id = request_id.clone();
                let check = cosmic::task::future(async move {
                    let result = agent::mount_setup(check_host.clone()).await;
                    Message::RemoteFilesChecked(check_host, request_id, result)
                });
                if !host.is_empty() {
                    return Task::batch([check, cosmic::task::future(async move {
                        let result = agent::mounts::connect(host.clone()).await;
                        Message::RemoteFilesConnectionChecked(host, connection_id, result)
                    })]);
                }
                return check;
            }
            Message::RemoteFilesChecked(host, request_id, result) => {
                if let Some(check) = self.mount_setup.get_mut(&host)
                    && check.request_id == request_id
                {
                    if result.as_ref().is_ok_and(|status| status.ready()) {
                        self.tools_reported.remove(&host);
                        if self.mount_errors.get(&host).is_some_and(|error| error.contains("sshfs is not installed")) {
                            self.mount_errors.remove(&host);
                        }
                    }
                    check.result = Some(result);
                }
            }
            Message::RemoteFilesCopy(ref host) | Message::RemoteFilesInstall(ref host) => {
                let install = matches!(&message, Message::RemoteFilesInstall(_));
                let status = self.mount_setup.get(host)
                    .and_then(|check| check.result.as_ref())
                    .and_then(|result| result.as_ref().ok());
                if let Some(command) = status.and_then(|status| status.install_command()) {
                    if !install { return clipboard::write(command); }
                    // This tab runs on the explicitly named computer. Sudo
                    // asks there; no system reboot is issued by the app.
                    let name = self.mount_setup_machines().into_iter().find(|(h, _)| h == host)
                        .map(|(_, name)| name).unwrap_or_else(|| host.clone());
                    let after = if status.is_some_and(|status| status.atomic) {
                        fl!("remote-files-reboot", machine = name.as_str())
                    } else {
                        fl!("remote-files-after-install")
                    };
                    let command = format!("{command}\nif [ $? -eq 0 ]; then printf '%s\\n' {}; fi", agent::sh_quote(&after));
                    let tab = agent::command_tab(host, &command);
                    let close = self.update(Message::ToggleContextPage(ContextPage::RemoteFiles));
                    let terminal = self.create_and_focus_new_terminal_with(
                        self.pane_model.focused(), None, false, Some(tab),
                    );
                    return Task::batch([close, terminal]);
                }
            }
            Message::RemoteFilesConnectionChecked(host, request_id, result) => {
                if self.mount_setup.get(&host).is_some_and(|check| check.request_id == request_id) {
                    return self.mount_connection_result(host, result);
                }
            }
            Message::MountsConnected(host, result) => {
                // An explicit check supersedes the connection started at
                // startup. Its tagged result owns the current diagnosis.
                if !self.mount_setup.contains_key(&host) {
                    return self.mount_connection_result(host, result);
                }
            }
            Message::PeerRequest(request) => match request {
                peers::Request::Focus(descriptor, token) => {
                    let Some(session) = agent::Session::from_descriptor(&descriptor) else {
                        return Task::none();
                    };
                    let Some((pane, entity)) = self.session_tab(&session) else {
                        return Task::none();
                    };
                    self.pane_model.set_focus(pane);
                    if let Some(tab_model) = self.pane_model.panes.get_mut(pane) {
                        tab_model.activate(entity);
                    }
                    let raise = match self.core.main_window_id() {
                        Some(window) if !token.is_empty() => {
                            cosmic::iced::platform_specific::shell::commands::activation::activate(
                                window, token,
                            )
                        }
                        _ => Task::none(),
                    };
                    return Task::batch([raise, self.update_title(Some(pane))]);
                }
                peers::Request::Release(descriptor) => {
                    let Some(session) = agent::Session::from_descriptor(&descriptor) else {
                        return Task::none();
                    };
                    if let Some((pane, entity)) = self.session_tab(&session) {
                        self.pane_model.set_focus(pane);
                        return self.update(Message::TabClose(Some(entity)));
                    }
                }
            },
            Message::SidebarAction(key, action) => {
                let Some((session, summary)) = self.sidebar.find(key) else {
                    return Task::none();
                };
                let agent = summary.agents.first().cloned();
                let cwd = summary.cwd.clone().unwrap_or_default();
                let tab = self.session_tab(&session);
                // Tab actions act on the focused pane's tabs
                if let Some((pane, _)) = tab {
                    self.pane_model.set_focus(pane);
                }
                // The conversation, continued in the other view once its agent
                // here has ended
                if action == sidebar::Action::SwitchView {
                    let (Some(kind), Some(conversation)) =
                        (session.agent(), agent.as_ref().and_then(|a| a.conversation.clone()))
                    else {
                        return Task::none();
                    };
                    if summary.working() {
                        return self
                            .toasts
                            .push(widget::toaster::Toast::new(fl!("sidebar-switch-busy")))
                            .map(cosmic::Action::App);
                    }
                    let thread = !session.is_thread();
                    let mut closing = Vec::new();
                    if let Some((_, entity)) = tab {
                        closing.push(self.update(Message::TabClose(Some(entity))));
                    }
                    let host = session.host.clone();
                    let end = cosmic::task::future(async move {
                        let ended = if session.is_thread() {
                            agent::thread_stop(session).await
                        } else {
                            agent::kill_session(session).await
                        };
                        if let Err(err) = ended {
                            log::warn!("ending a session to switch views: {err}");
                        }
                        Message::ContinueConversation { host, agent: kind, cwd, conversation, thread }
                    });
                    closing.push(end);
                    return Task::batch(closing);
                }
                // A thread's conversation is renamed and moved from its tab,
                // opened first if need be
                if session.is_thread()
                    && matches!(
                        action,
                        sidebar::Action::Rename | sidebar::Action::RenameAuto | sidebar::Action::MoveTo(_) | sidebar::Action::ForkTo(_)
                    )
                {
                    let Some((_, entity)) = tab else {
                        return self.update(Message::SidebarOpen(session));
                    };
                    return self.update(match action {
                        sidebar::Action::MoveTo(profile_id) => Message::TabTransfer(Some(entity), profile_id, transfer::Mode::Move),
                        sidebar::Action::ForkTo(profile_id) => Message::TabTransfer(Some(entity), profile_id, transfer::Mode::Fork),
                        _ => Message::SessionRename(Some(entity)),
                    });
                }
                match action {
                    // Handled above
                    sidebar::Action::SwitchView => {}
                    sidebar::Action::Open => return self.update(Message::SidebarOpen(session)),
                    sidebar::Action::CloseTab => {
                        if let Some((_, entity)) = tab {
                            return self.update(Message::TabClose(Some(entity)));
                        }
                    }
                    sidebar::Action::Rename => {
                        let current = agent.map(|a| a.title).unwrap_or_default();
                        self.rename = Some((session, current));
                        return Task::batch([
                            widget::text_input::focus(self.rename_id.clone()),
                            widget::text_input::select_all(self.rename_id.clone()),
                        ]);
                    }
                    sidebar::Action::RenameAuto => {
                        if self.pending_commands.contains_key("rename") {
                            return Task::none();
                        }
                        let Some(kind) = session.agent() else {
                            return Task::none();
                        };
                        self.start_progress("rename", fl!("renaming-conversation"));
                        return cosmic::task::future(async move {
                            Message::Renamed(agent::rename_conversation(session, kind, None).await)
                        });
                    }
                    sidebar::Action::End => {
                        if self.pending_commands.contains_key("end-session") {
                            return Task::none();
                        }
                        // Gone from the list now, rather than at the next scan
                        if let Some(Some(Ok(summaries))) =
                            self.sidebar.hosts.get_mut(&session.host).map(|h| h.summaries.as_mut())
                        {
                            summaries.retain(|s| s.name != session.name);
                        }
                        if let Some((_, entity)) = tab {
                            return self.update(Message::RemoteEnd(Some(entity)));
                        }
                        self.start_progress("end-session", fl!("ending-session"));
                        return cosmic::task::future(async move {
                            Message::RemoteEnded(None, agent::kill_session(session).await)
                        });
                    }
                    sidebar::Action::MoveTo(profile_id) | sidebar::Action::ForkTo(profile_id) => {
                        let mode = if matches!(action, sidebar::Action::MoveTo(_)) {
                            transfer::Mode::Move
                        } else {
                            transfer::Mode::Fork
                        };
                        if let Some((_, entity)) = tab {
                            return self.update(Message::TabTransfer(Some(entity), profile_id, mode));
                        }
                        let (Some(agent), Some(kind)) =
                            (agent, session.agent().and_then(agent_kind))
                        else {
                            return Task::none();
                        };
                        let Some(id) = agent.conversation.clone() else {
                            return self.launcher_error(fl!(
                                "tab-transfer-no-agent",
                                error = "its conversation was not found"
                            ));
                        };
                        let choice = LaunchChoice::Transfer {
                            conversation: transfer::ConversationRef {
                                host: session.host.clone(),
                                kind,
                                id,
                            },
                            cwd,
                            title: agent.title.clone(),
                            mode,
                        };
                        // The destination's launcher, with the transfer form open
                        let pane = self.pane_model.focused();
                        let opened = self.open_launcher(pane, profile_id);
                        let Some(launcher) = self.pane_model.panes.get(pane).map(|t| t.active()) else {
                            return opened;
                        };
                        return Task::batch([opened, self.launch_from(launcher, choice)]);
                    }
                }
            }
            Message::TabHover(pane, hovered) => {
                self.hovered_tab = hovered.map(|(entity, x)| (pane, entity, x, Instant::now()));
                if hovered.is_some() {
                    return cosmic::task::future(async {
                        tokio::time::sleep(TAB_PREVIEW_DELAY).await;
                        Message::TabPreviewDue
                    });
                }
            }
            Message::TabPreviewDue => {}
            Message::SidebarCompact(compact) => {
                config_set!(sidebar_compact, compact);
            }
            Message::SidebarFilter(filter) => {
                self.sidebar.filter = filter;
            }
            Message::SidebarFilterSubmit => {
                if let Some(session) = self.sidebar.first_match() {
                    self.sidebar.filter.clear();
                    self.sidebar.searching = false;
                    return self.update(Message::SidebarOpen(session));
                }
            }
            Message::SidebarSearching(searching) => {
                self.sidebar.searching = searching;
                // As for the Secrets pane's fields: the terminal gives up
                // the focus
                if searching {
                    return widget::text_input::focus(self.sidebar.filter_id.clone());
                }
            }
            Message::NewTabMenuToggle => {
                if self.new_tab_menu_open {
                    return self.update(Message::NewTabMenuClose);
                }
                self.new_tab_menu_open = true;
                self.new_tab_menu_in_sidebar = false;
            }
            Message::NewTabMenuClose => {
                self.new_tab_menu_open = false;
                return self.update_focus();
            }
            // A thread's relay opens as a thread, never in a terminal
            Message::RemoteAttach(session) if session.is_thread() => {
                return self.update(Message::SidebarOpen(session));
            }
            Message::RemoteAttach(session) => {
                self.new_tab_menu_open = false;
                return self.create_and_focus_new_terminal_with(
                    self.pane_model.focused(),
                    None,
                    false,
                    Some((agent::attach(&session), session)),
                );
            }
            Message::RemoteEnd(entity_opt) => {
                // A thread's agent ends, relay and all
                let thread = self.pane_model.active().and_then(|tab_model| {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    tab_model.data::<thread::tab::ThreadTab>(entity).map(|t| t.id)
                });
                if let Some(id) = thread {
                    return self.thread_mut(id).map_or_else(Task::none, |tab| tab.end_cli());
                }
                if self.pending_commands.contains_key("end-session") {
                    return Task::none();
                }
                let session = self.pane_model.active().and_then(|tab_model| {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    tab_model.data::<agent::Session>(entity).cloned()
                });
                if let Some(session) = session {
                    self.start_progress("end-session", fl!("ending-session"));
                    let entity = entity_opt.or_else(|| self.pane_model.active().map(|tab_model| tab_model.active()));
                    return cosmic::task::future(async move {
                        Message::RemoteEnded(entity, agent::kill_session(session).await)
                    });
                }
            }
            Message::CloseToast(id) => {
                self.toasts.remove(id);
            }
            Message::LauncherData(entity, host, result) => {
                let cache = listing_cache(&host);
                let state = match result {
                    Ok(listing) => {
                        // Kept for when the host is offline
                        if let Some(dir) = cache.parent()
                            && fs::create_dir_all(dir).is_ok()
                            && let Ok(bytes) = serde_json::to_vec(&listing)
                        {
                            let _ = fs::write(&cache, bytes);
                        }
                        Listing::Ready(listing)
                    }
                    Err(error) => {
                        log::info!("listing {host:?} failed: {error}");
                        let cached = fs::read(&cache)
                            .ok()
                            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
                        Listing::Offline(cached, error)
                    }
                };
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.listings.insert(host, state);
                }
            }
            Message::LauncherFocus(entity, id) => {
                // Its pane is the one in front, so that the focus stays where
                // it was put rather than going back to the other pane, and
                // the field takes the keys from any terminal holding them
                if let Some(pane) = self.tab_pane(entity)
                    && pane != self.pane_model.focused()
                {
                    self.pane_model.set_focus(pane);
                }
                self.secrets_focus = None;
                return widget::text_input::focus(id);
            }
            Message::LauncherFormCancel(entity) => {
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.form = None;
                }
                return self.update_focus();
            }
            Message::LauncherStart(entity, request_id, dir, result) => {
                if !matches!(self.launcher(entity).and_then(|l| l.pending_action.as_ref()), Some((id, _)) if id == &request_id) {
                    return Task::none();
                }
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.pending_action = None;
                }
                match result {
                Ok(start) => return self.launch_started(entity, None, &dir, &start),
                Err(err) => return self.launcher_error(err),
                }
            }
            Message::LauncherWorktreeOptions(entity, request_id, result) => {
                if let Some(launcher) = self.launcher_mut(entity)
                    && let Some(LauncherForm::Worktree(form)) = &mut launcher.form
                    && form.request_id == request_id
                {
                    form.set_options(result);
                    // Typing replaces the suggested name
                    return Task::batch([
                        widget::text_input::focus(form.branch_id.clone()),
                        widget::text_input::select_all(form.branch_id.clone()),
                    ]);
                }
            }
            Message::LauncherWorktreeEdit(entity, field, value) => {
                if let Some(launcher) = self.launcher_mut(entity)
                    && let Some(LauncherForm::Worktree(form)) = &mut launcher.form
                {
                    form.edit(field, value);
                }
            }
            Message::LauncherWorktreeBase(entity, index) => {
                if let Some(launcher) = self.launcher_mut(entity)
                    && let Some(LauncherForm::Worktree(form)) = &mut launcher.form
                {
                    form.base = index;
                }
            }
            Message::LauncherWorktreeCreate(entity) => {
                let host = self.launcher_host(entity);
                let Some(launcher) = self.launcher_mut(entity) else {
                    return Task::none();
                };
                let Some(LauncherForm::Worktree(form)) = &mut launcher.form else {
                    return Task::none();
                };
                if form.busy {
                    return Task::none();
                }
                let Some(launcher_forms::WorktreeRequest {
                    repository,
                    branch,
                    base,
                    path,
                    from,
                }) = form.request()
                else {
                    return Task::none();
                };
                form.busy = true;
                form.error = None;
                let request_id = form.request_id.clone();
                return cosmic::task::future(async move {
                    let result = blocking(move || {
                        let request = cosmic_term::helper::Request::CreateWorktree {
                            repository,
                            branch,
                            base,
                            path,
                            from,
                        };
                        match transfer::Machine::call(&agent::HostMachine { host }, request)? {
                            cosmic_term::helper::Reply::Path { path } => Ok(path),
                            cosmic_term::helper::Reply::Error { message } => Err(message),
                            other => Err(format!("unexpected reply: {other:?}")),
                        }
                    })
                    .await;
                    Message::LauncherWorktreeCreated(entity, request_id, result)
                });
            }
            Message::LauncherWorktreeCreated(entity, request_id, result) => {
                if !matches!(self.launcher(entity).and_then(|l| l.form.as_ref()),
                    Some(LauncherForm::Worktree(form)) if form.request_id == request_id && form.busy)
                {
                    return Task::none();
                }
                match result {
                Ok(path) => {
                    return self.launch_from(entity, LaunchChoice::Dir(path.display().to_string()));
                }
                Err(err) => {
                    if let Some(launcher) = self.launcher_mut(entity)
                        && let Some(LauncherForm::Worktree(form)) = &mut launcher.form
                    {
                        form.busy = false;
                        form.error = Some(err);
                    }
                }
                }
            }
            Message::LauncherTransferDraft(entity, request_id, result) => {
                let result = *result;
                if !matches!(self.launcher(entity).and_then(|l| l.form.as_ref()),
                    Some(LauncherForm::Transfer(form)) if form.request_id == request_id && form.draft.is_none())
                {
                    return Task::none();
                }
                let bound = result
                    .as_ref()
                    .ok()
                    .and_then(|draft| draft.repositories.bound.clone());
                if let Some(launcher) = self.launcher_mut(entity)
                    && let Some(LauncherForm::Transfer(form)) = &mut launcher.form
                {
                    form.draft = Some(result);
                }
                // A known repository needs no choice
                if let Some(bound) = bound {
                    return self.update(Message::LauncherTransferChoose(
                        entity,
                        transfer::RepoChoice::Existing(bound),
                    ));
                }
            }
            Message::LauncherTransferChoose(entity, choice) => {
                let destination = self.launcher_host(entity);
                let Some(launcher) = self.launcher_mut(entity) else {
                    return Task::none();
                };
                let Some(LauncherForm::Transfer(form)) = &mut launcher.form else {
                    return Task::none();
                };
                if form.busy {
                    return Task::none();
                }
                let Some(Ok(draft)) = form.draft.clone() else {
                    return Task::none();
                };
                form.choice = Some(choice.clone());
                form.place = None;
                form.place_generation += 1;
                let generation = form.place_generation;
                let request_id = form.request_id.clone();
                return cosmic::task::future(async move {
                    let result = blocking(move || {
                        transfer::place(&agent::HostMachine { host: destination }, &draft, &choice)
                    })
                    .await;
                    Message::LauncherTransferPlace(entity, request_id, generation, Box::new(result))
                });
            }
            Message::LauncherTransferPlace(entity, request_id, generation, result) => {
                let result = *result;
                if let Some(launcher) = self.launcher_mut(entity)
                    && let Some(LauncherForm::Transfer(form)) = &mut launcher.form
                    && form.request_id == request_id
                    && form.place_generation == generation
                {
                    if let Ok(place) = &result {
                        form.path = place.new_path.display().to_string();
                    }
                    form.place = Some(result);
                }
            }
            Message::LauncherTransferPath(entity, path) => {
                if let Some(launcher) = self.launcher_mut(entity)
                    && let Some(LauncherForm::Transfer(form)) = &mut launcher.form
                {
                    form.path = path;
                }
            }
            Message::LauncherTransferStart(entity) => {
                let plan = self.launcher(entity)
                    .and_then(|launcher| launcher.form.as_ref())
                    .and_then(|form| match form {
                        LauncherForm::Transfer(form) if !form.busy => form.plan(),
                        _ => None,
                    });
                let Some(plan) = plan else {
                    return Task::none();
                };
                let transfer_id = plan.draft.id.clone();
                if self.active_transfers.contains(&transfer_id) {
                    return Task::none();
                }
                let Some(launcher) = self.launcher_mut(entity) else {
                    return Task::none();
                };
                let Some(LauncherForm::Transfer(form)) = &mut launcher.form else {
                    return Task::none();
                };
                form.busy = true;
                form.progress = transfer::Step::Started;
                form.stalled = false;
                form.error = None;
                let request_id = form.request_id.clone();
                self.active_transfers.insert(transfer_id.clone());
                let (send, receive) = iced::futures::channel::mpsc::unbounded();
                std::thread::spawn(move || {
                    let progress_send = send.clone();
                    let progress_id = request_id.clone();
                    let result = (|| -> Result<transfer::Launch, String> {
                        let source = agent::HostMachine { host: plan.draft.conversation.host.clone() };
                        let destination = agent::HostMachine { host: plan.draft.destination.clone() };
                        let journal = transfer::Journal::new(&cosmic_term::Env::from_process().state_dir);
                        let _lock = journal.try_lock(&plan.draft.id)?;
                        transfer::start_with_progress(&source, &destination, &journal, plan,
                            &|step| { let _ = progress_send.unbounded_send(
                                Message::LauncherTransferProgress(entity, progress_id.clone(), step)); })
                    })();
                    let _ = send.unbounded_send(Message::LauncherTransferDone(entity, request_id, transfer_id, result));
                });
                let slow_id = self.launcher(entity).and_then(|launcher| match launcher.form.as_ref() {
                    Some(LauncherForm::Transfer(form)) => Some(form.request_id.clone()),
                    _ => None,
                }).unwrap_or_default();
                return Task::batch([
                    cosmic::task::stream(receive),
                    cosmic::task::future(async move {
                        tokio::time::sleep(Duration::from_secs(15)).await;
                        Message::LauncherTransferSlow(entity, slow_id, transfer::Step::Started)
                    }),
                ]);
            }
            Message::LauncherTransferProgress(entity, request_id, step) => {
                if let Some(launcher) = self.launcher_mut(entity)
                    && let Some(LauncherForm::Transfer(form)) = &mut launcher.form
                    && form.request_id == request_id && form.busy
                {
                    form.progress = step;
                    form.stalled = false;
                    return cosmic::task::future(async move {
                        tokio::time::sleep(Duration::from_secs(15)).await;
                        Message::LauncherTransferSlow(entity, request_id, step)
                    });
                }
            }
            Message::LauncherTransferSlow(entity, request_id, step) => {
                if let Some(launcher) = self.launcher_mut(entity)
                    && let Some(LauncherForm::Transfer(form)) = &mut launcher.form
                    && form.request_id == request_id && form.busy && form.progress == step
                {
                    form.stalled = true;
                }
            }
            Message::LauncherTransferDone(entity, request_id, transfer_id, result) => {
                self.active_transfers.remove(&transfer_id);
                if !matches!(self.launcher(entity).and_then(|l| l.form.as_ref()),
                    Some(LauncherForm::Transfer(form)) if form.request_id == request_id && form.busy)
                {
                    return Task::none();
                }
                match result {
                Ok(launch) => {
                    let dir = launch.dir.display().to_string();
                    return self.launch_started(entity, None, &dir, &launch.start);
                }
                Err(err) => {
                    let unfinished = transfer::Journal::new(&cosmic_term::Env::from_process().state_dir).unfinished();
                    if let Some(launcher) = self.launcher_mut(entity) {
                        launcher.unfinished = unfinished;
                        if let Some(LauncherForm::Transfer(form)) = &mut launcher.form {
                            form.busy = false;
                            form.error = Some(err);
                        }
                    }
                }
                }
            }
            Message::LauncherRecover(entity, id, resume) => {
                if self.active_transfers.contains(&id) {
                    return Task::none();
                }
                let Some(mut record) = self
                    .launcher(entity)
                    .and_then(|launcher| launcher.unfinished.iter().find(|r| r.plan.draft.id == id).cloned())
                else {
                    return Task::none();
                };
                let launcher_id = self.launcher(entity).map(|launcher| launcher.id.clone()).unwrap_or_default();
                self.active_transfers.insert(id.clone());
                return cosmic::task::future(async move {
                    let result = async {
                        for host in [&record.plan.draft.conversation.host, &record.plan.draft.destination] {
                            agent::ensure_helper(host.clone(), agent::Helper::Transfer).await?;
                        }
                        blocking(move || {
                            let source = agent::HostMachine { host: record.plan.draft.conversation.host.clone() };
                            let destination = agent::HostMachine { host: record.plan.draft.destination.clone() };
                            let journal = transfer::Journal::new(&cosmic_term::Env::from_process().state_dir);
                            let _lock = journal.try_lock(&record.plan.draft.id)?;
                            if resume {
                                transfer::resume(&source, &destination, &journal, &mut record).map(Some)
                            } else {
                                transfer::rollback(&source, &destination, &journal, &mut record).map(|()| None)
                            }
                        })
                        .await
                    }
                    .await;
                    Message::LauncherRecovered(entity, id, launcher_id, result)
                });
            }
            Message::LauncherRecovered(entity, id, launcher_id, result) => {
                if !self.active_transfers.remove(&id) {
                    return Task::none();
                }
                if self.launcher(entity).is_none_or(|launcher| launcher.id != launcher_id) {
                    return Task::none();
                }
                let unfinished = transfer::Journal::new(&cosmic_term::Env::from_process().state_dir).unfinished();
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.unfinished = unfinished;
                }
                match result {
                    Ok(Some(launch)) => {
                        // The agent the transfer was for, on its computer
                        let profile_id = self.profile_for(&launch);
                        let dir = launch.dir.display().to_string();
                        return self.launch_started(entity, profile_id, &dir, &launch.start);
                    }
                    Ok(None) => {}
                    Err(err) => return self.launcher_error(err),
                }
            }
            Message::LauncherSessions(entity, result) => {
                if let (Some(launcher), Ok(listed)) = (self.launcher_mut(entity), result) {
                    launcher.running = listed
                        .into_iter()
                        .filter(|listed| listed.session.is_managed() && !listed.exited)
                        .map(|listed| listed.session)
                        .collect();
                }
            }
            Message::LauncherFilter(entity, filter) => {
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.filter = filter;
                }
            }
            Message::LauncherSubmit(entity) => {
                if self.launcher_locked(entity) {
                    return Task::none();
                }
                // The first row, or home when nothing matches an empty filter
                let choice = self.launcher(entity).and_then(|launcher| {
                    let first = self
                        .launcher_rows(launcher)
                        .into_iter()
                        .filter(|(section, _, _)| self.launcher_section_open(launcher, *section))
                        .flat_map(|(_, _, rows)| rows)
                        .find_map(|row| row.choice);
                    first.or_else(|| {
                        launcher
                            .filter
                            .trim()
                            .is_empty()
                            .then(|| LaunchChoice::Dir(String::new()))
                    })
                });
                if let Some(choice) = choice {
                    return self.launch_from(entity, choice);
                }
            }
            Message::LauncherToggleSection(section) => {
                let mut collapsed = self.config.launcher_collapsed.clone();
                if !collapsed.remove(&section) {
                    collapsed.insert(section);
                }
                config_set!(launcher_collapsed, collapsed);
            }
            Message::LauncherChoose(entity, choice) => {
                if self.launcher_locked(entity) {
                    return Task::none();
                }
                return self.launch_from(entity, choice);
            }
            Message::LauncherMounted(entity, request_id, result) => {
                if !matches!(self.launcher(entity).and_then(|l| l.pending_action.as_ref()), Some((id, _)) if id == &request_id) {
                    return Task::none();
                }
                if let Some(launcher) = self.launcher_mut(entity) {
                    launcher.pending_action = None;
                }
                match result {
                // Local folders are chosen directly; remote ones through the
                // host's sshfs mount
                Ok(mount) if self.dialog_opt.is_none() => {
                    let host = self.launcher_host(entity);
                    let home = self
                        .launcher(entity)
                        .and_then(|launcher| launcher.listings.get(&host)?.listing())
                        .map(|listing| listing.home.clone())
                        .unwrap_or_default();
                    let start = if mount.as_os_str().is_empty() {
                        PathBuf::from(&home)
                    } else {
                        mount.join(home.trim_start_matches('/'))
                    };
                    let mount = (!mount.as_os_str().is_empty()).then_some(mount);
                    let (dialog, command) = Dialog::new(
                        DialogSettings::new()
                            .kind(DialogKind::OpenFolder)
                            .path(start),
                        |msg| Message::DialogMessage(Box::new(msg)),
                        move |result| Message::LauncherBrowse(entity, mount.clone(), result),
                    );
                    self.dialog_opt = Some(dialog);
                    return command;
                }
                Ok(_) => {}
                Err(err) => {
                    self.mount_errors.insert(String::new(), err);
                    self.mount_setup.remove("");
                    return self.update(Message::RemoteFilesOpen(String::new()));
                }
                }
            }
            Message::LauncherBrowse(entity, mount, result) => {
                self.dialog_opt = None;
                if let DialogResult::Open(paths) = result
                    && let Some(path) = paths.into_iter().next()
                {
                    // Back from the mount to the path on the host
                    let dir = match &mount {
                        Some(mount) => match path.strip_prefix(mount) {
                            Ok(rest) => format!("/{}", rest.display()),
                            Err(_) => return Task::none(),
                        },
                        None => path.display().to_string(),
                    };
                    return self.launch_from(entity, LaunchChoice::Dir(dir));
                }
            }
            Message::HostProbed(host, generation, reachable) => {
                // Only the latest probe counts
                let Some(&(current, attempt)) = self.reconnecting.get(&host) else {
                    return Task::none();
                };
                if generation != current {
                    return Task::none();
                }
                if self.disconnected_tabs(&host).is_empty() {
                    self.reconnecting.remove(&host);
                    return Task::none();
                }
                if !reachable {
                    // 10 seconds, doubling to a minute
                    let delay = (10u64 << attempt.min(3)).min(60);
                    return self.probe_host(host, attempt + 1, delay);
                }
                self.reconnecting.remove(&host);
                let sessions_host = host.clone();
                return cosmic::task::future(async move {
                    Message::ReconnectSessions(host, agent::list_sessions(sessions_host).await)
                });
            }
            Message::ReconnectNow(host) => {
                return self.probe_host(host, 0, 0);
            }
            Message::ReconnectSessions(host, result) => {
                let listed = match result {
                    Ok(listed) => listed,
                    // Reachable, but not yet usable; keep trying
                    Err(_) => return self.probe_host(host, 1, 10),
                };
                let mut tasks = Vec::new();
                for (pane, entity) in self.disconnected_tabs(&host) {
                    let Some(tab_model) = self.pane_model.panes.get(pane) else {
                        continue;
                    };
                    let (Some(session), Some(connection)) = (
                        tab_model.data::<agent::Session>(entity).cloned(),
                        tab_model.data::<TabConnection>(entity),
                    ) else {
                        continue;
                    };
                    let running = listed
                        .iter()
                        .any(|listed| listed.session == session && !listed.exited);
                    // A drop just after starting may have come before the
                    // session was created
                    let never_started = connection.lost.is_some_and(|lost| {
                        lost.duration_since(connection.started) < Duration::from_secs(10)
                    });
                    let prepared = if running {
                        (agent::attach(&session), session)
                    } else if never_started {
                        (connection.launch.clone(), session)
                    } else {
                        // It ended while out of reach; close, as it would have
                        self.pane_model.set_focus(pane);
                        tasks.push(self.update(Message::TabClose(Some(entity))));
                        tasks.push(
                            self.toasts
                                .push(widget::toaster::Toast::new(fl!(
                                    "session-ended-while-disconnected",
                                    host = host.as_str()
                                )))
                                .map(cosmic::Action::App),
                        );
                        continue;
                    };
                    tasks.push(self.replace_tab(pane, entity, None, prepared));
                }
                return Task::batch(tasks);
            }
            Message::SupervisorProgress(host, step) => {
                self.host_setup.insert(host.clone(), step);
                self.host_setup_stalled.remove(&host);
                self.host_progress(&host);
                return cosmic::task::future(async move {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    Message::SupervisorSlow(host, step)
                });
            }
            Message::SupervisorSlow(host, step) => {
                if self.host_setup.get(&host) == Some(&step) {
                    self.host_setup_stalled.insert(host.clone());
                    self.host_progress(&host);
                }
            }
            Message::SupervisorInstalled(host, result) => {
                self.host_setup.remove(&host);
                self.host_setup_stalled.remove(&host);
                if let Some(id) = self.host_toasts.remove(&host) { self.toasts.remove(id); }
                match result {
                Err(err) => log::warn!("failed to install the helpers on {:?}: {}", host, err),
                // Said once per machine and run, rather than failing quietly
                Ok(missing) if !missing.is_empty() && self.tools_reported.insert(host.clone()) => {
                    let machine = if host.is_empty() {
                        fl!("this-computer")
                    } else {
                        self.config.machine_for_host(&host).map_or_else(|| host.clone(), |m| m.name.clone())
                    };
                    let what = missing
                        .iter()
                        .map(|tool| fl!("tool-missing", tool = tool.as_str(), needed = tool_needed(tool)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    log::warn!("missing on {host:?}: {missing:?}");
                    let mut toast = widget::toaster::Toast::new(fl!(
                        "tools-missing", machine = machine.as_str(), what = what.as_str()
                    ));
                    if missing.iter().any(|tool| !matches!(tool.as_str(), "python3" | "git")) {
                        toast = toast.action(fl!("remote-files-help"), move |_| Message::RemoteFilesOpen(host.clone()));
                    }
                    return self
                        .toasts
.push(toast)
                        .map(cosmic::Action::App);
                }
                Ok(_) => {}
                }
            }
            Message::SidebarToggle => {
                self.show_sidebar = !self.show_sidebar;
                config_set!(show_sidebar, self.show_sidebar);
                return self.sidebar_start();
            }
            Message::SidebarRefresh(host, generation) => {
                if generation != self.sidebar.generation {
                    return Task::none();
                }
                let Some(state) = self.sidebar.hosts.get_mut(&host) else {
                    return Task::none();
                };
                if state.scanning {
                    return Task::none();
                }
                state.scanning = true;
                return cosmic::task::future(async move {
                    let result = agent::session_summaries(host.clone()).await;
                    Message::SidebarSummaries(host, generation, result)
                });
            }
            Message::SidebarSummaries(host, generation, result) => {
                if let Err(err) = &result {
                    log::debug!("could not list the sessions on {host:?}: {err}");
                }
                if let Some(state) = self.sidebar.hosts.get_mut(&host) {
                    state.scanning = false;
                    // A host that stops answering keeps showing what it had
                    if result.is_ok() || !matches!(state.summaries, Some(Ok(_))) {
                        state.summaries = Some(result);
                    }
                }
                let mut retitled = self.retitle_agent_tabs();
                // Keep this computer's sudo agent reachable from machines
                // that this window shows, renewing the forward now and then
                let shown = !host.is_empty()
                    && (self.open_sessions().iter().any(|s| s.host == host)
                        || self.launcher_hosts().contains(&host.as_str()));
                let due = self
                    .sudo_forwarded
                    .get(&host)
                    .is_none_or(|at| at.elapsed() > Duration::from_secs(60));
                if shown && due {
                    self.sudo_forwarded.insert(host.clone(), Instant::now());
                    let forward_host = host.clone();
                    retitled = Task::batch([
                        retitled,
                        cosmic::task::future(async move {
                            let result = agent::sudo::forward(forward_host.clone()).await;
                            Message::SudoResult(format!("forwarding sudo to {forward_host}"), result)
                        }),
                    ]);
                }
                if generation != self.sidebar.generation {
                    return retitled;
                }
                return Task::batch([retitled, cosmic::task::future(async move {
                    tokio::time::sleep(sidebar::REFRESH).await;
                    Message::SidebarRefresh(host, generation)
                })]);
            }
            Message::SidebarOpen(session) if session.is_thread() && self.session_tab(&session).is_none() => {
                self.sidebar.searching = false;
                // A thread joins its relay, in a thread tab
                let found = self.sidebar.find(sidebar::session_key(&session)).map(|(_, summary)| {
                    let thread = summary.thread.clone();
                    let cwd = thread
                        .as_ref()
                        .and_then(|t| t.meta.get("cwd").and_then(|c| c.as_str()).map(str::to_string))
                        .or_else(|| summary.cwd.clone())
                        .unwrap_or_default();
                    (cwd, thread.and_then(|t| t.conversation))
                });
                let Some((cwd, conversation)) = found else { return Task::none() };
                let agent = session.agent().unwrap_or(config::Agent::Claude);
                let profile_id = self.profile_on_host(&session.host, Some(agent), true);
                let id = self.next_thread;
                let has_history = conversation.is_some();
                let opened = self.open_thread_as(
                    self.pane_model.focused(),
                    profile_id,
                    cwd,
                    conversation,
                    None,
                    Some(agent),
                    Some(session.host.clone()),
                );
                let mut attach = Task::none();
                if let Some(tab) = self.thread_mut(id) {
                    tab.relay = Some(session);
                    tab.relay_live = true;
                    tab.attach_on_load = has_history;
                    if !has_history {
                        attach = tab.attach();
                    }
                }
                return Task::batch([opened, attach]);
            }
            Message::SidebarOpen(session) => {
                self.sidebar.searching = false;
                let elsewhere = self
                    .sidebar
                    .find(sidebar::session_key(&session))
                    .is_some_and(|(_, summary)| summary.clients > 0);
                if elsewhere && self.session_tab(&session).is_none() {
                    let descriptor = session.descriptor();
                    return cosmic::task::future(async move {
                        Message::SidebarPeerFound(session, peers::find(descriptor).await)
                    });
                }
                if let Some((pane, entity)) = self.session_tab(&session) {
                    self.pane_model.set_focus(pane);
                    if let Some(tab_model) = self.pane_model.panes.get_mut(pane) {
                        tab_model.activate(entity);
                    }
                    return Task::batch([
                        self.update_title(Some(pane)),
                        self.update_focus(),
                        self.refresh_focus(entity),
                    ]);
                }
                return self.update(Message::RemoteAttach(session));
            }
            Message::SidebarNew {
                host,
                repository,
                agent,
                worktree,
                thread,
            } => {
                let Some(profile_id) = self.profile_on_host(&host, agent, thread) else {
                    return self
                        .toasts
                        .push(widget::toaster::Toast::new(fl!(
                            "sidebar-no-profile",
                            host = host.as_str()
                        )))
                        .map(cosmic::Action::App);
                };
                // Through a launcher, which starts the session in its place
                // or shows the new worktree form
                let pane = self.pane_model.focused();
                let opened = self.open_launcher(pane, profile_id);
                let Some(entity) = self.pane_model.active().map(|tab_model| tab_model.active())
                else {
                    return opened;
                };
                let choice = if worktree {
                    LaunchChoice::NewWorktree(repository)
                } else {
                    LaunchChoice::Dir(repository)
                };
                return Task::batch([opened, self.update(Message::LauncherChoose(entity, choice))]);
            }
            Message::OpenInFiles => {
                if self.pending_commands.contains_key("open-files") {
                    return Task::none();
                }
                if let Some(process) = self.active_process() {
                    self.start_progress("open-files", fl!("opening-files"));
                    return cosmic::task::future(async move {
                        Message::OpenInFilesResult(agent::open_in_files(process).await)
                    });
                }
            }
            Message::OpenInFilesResult(result) => {
                self.finish_progress("open-files");
                if let Err(err) = result {
                    log::warn!("failed to open in the file manager: {}", err);
                    return self.file_open_error(fl!("open-in-files-failed", error = err.as_str()), err);
                }
            }
            Message::OpenInZed => {
                if self.pending_commands.contains_key("open-zed") {
                    return Task::none();
                }
                if let Some(process) = self.active_process() {
                    self.start_progress("open-zed", fl!("opening-zed"));
                    return cosmic::task::future(async move {
                        Message::OpenInZedResult(agent::open_in_zed(process).await)
                    });
                }
            }
            Message::OpenInZedResult(result) => {
                self.finish_progress("open-zed");
                if let Err(err) = result {
                    log::warn!("failed to open in Zed: {}", err);
                    return self.file_open_error(fl!("open-in-zed-failed", error = err.as_str()), err);
                }
            }
            Message::RemoteEnded(entity, result) => {
                self.finish_progress("end-session");
                match result {
                    Ok(()) => {
                        if let Some(entity) = entity {
                            return self.update(Message::TabClose(Some(entity)));
                        }
                    }
                    Err(err) => log::warn!("failed to end remote session: {}", err),
                }
            }
            Message::ProfileAgent(profile_id, agent_i) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id)
                    && let Some(agent) = config::Agent::ALL.get(agent_i)
                {
                    profile.agent = *agent;
                    return self.save_profiles();
                }
            }
            Message::ProfileThread(profile_id, on) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    profile.thread = on;
                    return self.save_profiles();
                }
            }
            Message::ProfileArgs(profile_id, text) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    profile.args = text;
                    return self.save_profiles();
                }
            }
            Message::ProfileColor(profile_id, color_i) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    // The first choice is the agent's default
                    profile.color = color_i
                        .checked_sub(1)
                        .and_then(|i| TabColor::PALETTE.get(i))
                        .map(|(_, hex)| hex.to_string())
                        .unwrap_or_default();
                    return self.save_profiles();
                }
            }
            Message::ProfileMachine(profile_id, index) => {
                let machine = index
                    .checked_sub(1)
                    .and_then(|i| self.config.machines.keys().nth(i).copied());
                let host = machine
                    .and_then(|m| self.config.machines.get(&m))
                    .map(config::Machine::destination)
                    .unwrap_or_default();
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    profile.machine = machine;
                    profile.host = host;
                    return self.save_profiles();
                }
            }
            Message::LocalProbed(result) => match result {
                Ok(probe) => {
                    self.local = Some(probe);
                    // This computer may be among the machines by another name
                    return self.sidebar_start();
                }
                Err(err) => log::info!("could not check this computer: {err}"),
            },
            Message::MachineAdd => {
                self.discovery = Some(None);
                return cosmic::task::future(async {
                    Message::MachineDiscovered(agent::discovery_sources().await)
                });
            }
            Message::MachineDiscovered(result) => {
                if self.discovery.is_some() {
                    let known: Vec<config::Machine> = self.config.machines.values().cloned().collect();
                    self.discovery = Some(Some(result.map(|(tailscale, ssh)| {
                        machines::candidates(
                            &machines::parse_ssh_config(&ssh),
                            machines::parse_tailscale(&tailscale).as_ref(),
                            &known,
                        )
                    })));
                }
            }
            Message::MachineDiscoveryClose => self.discovery = None,
            Message::MachineAddCandidate(index) => {
                let Some(Some(Ok(candidates))) = &self.discovery else {
                    return Task::none();
                };
                let Some(candidate) = candidates.get(index) else {
                    return Task::none();
                };
                let id = self.config.add_machine(candidate.machine());
                self.discovery = None;
                self.machine_expanded = Some(id);
                return Task::batch([self.save_machines(), self.update(Message::MachineCheck(id))]);
            }
            Message::MachineAddBlank => {
                let id = self.config.add_machine(config::Machine {
                    name: fl!("new-machine"),
                    ..config::Machine::default()
                });
                self.discovery = None;
                self.machine_expanded = Some(id);
                return self.save_machines();
            }
            Message::MachineEdit(id, field, text) => {
                let Some(machine) = self.config.machines.get_mut(&id) else {
                    return Task::none();
                };
                match field {
                    MachineField::Name => machine.name = text,
                    MachineField::Host => machine.host = text.trim().to_string(),
                    MachineField::User => machine.user = text.trim().to_string(),
                }
                if !matches!(field, MachineField::Name) {
                    // Another computer, perhaps: what was found no longer holds
                    machine.hostname.clear();
                    machine.home.clear();
                    machine.host_id.clear();
                    self.machine_checks.remove(&id);
                }
                return self.save_machines();
            }
            Message::MachineExpand(id) => self.machine_expanded = Some(id),
            Message::MachineCollapse(id) => {
                if self.machine_expanded == Some(id) {
                    self.machine_expanded = None;
                }
            }
            Message::MachineRemove(id) => {
                let users: Vec<String> = self
                    .config
                    .profiles
                    .values()
                    .filter(|p| p.machine == Some(id))
                    .map(|p| p.name.clone())
                    .collect();
                if !users.is_empty() {
                    return self
                        .toasts
                        .push(widget::toaster::Toast::new(fl!(
                            "machine-in-use",
                            profiles = users.join(", ")
                        )))
                        .map(cosmic::Action::App);
                }
                self.config.machines.remove(&id);
                self.machine_checks.remove(&id);
                return self.save_machines();
            }
            Message::MachineCheck(id) => {
                let Some(destination) = self
                    .config
                    .machines
                    .get(&id)
                    .map(config::Machine::destination)
                    .filter(|d| !d.is_empty())
                else {
                    return Task::none();
                };
                self.machine_checks.insert(id, MachineCheck::Running);
                return cosmic::task::future(async move {
                    let result = agent::probe_machine(destination.clone()).await;
                    Message::MachineChecked(id, destination, result)
                });
            }
            Message::MachineChecked(id, destination, result) => {
                // Unless the machine was changed meanwhile
                let Some(machine) = self
                    .config
                    .machines
                    .get_mut(&id)
                    .filter(|m| m.destination() == destination)
                else {
                    return Task::none();
                };
                let mut task = Task::none();
                if let Ok(probe) = &result {
                    if machine.user.is_empty() {
                        machine.user = probe.user.clone();
                    }
                    machine.hostname = probe.hostname.clone();
                    machine.home = probe.home.clone();
                    machine.host_id = probe.host_id.clone();
                    task = self.save_machines();
                }
                self.machine_checks.insert(id, MachineCheck::Done(result));
                return task;
            }
            Message::MachineAddProfiles(id) => {
                let Some(machine) = self.config.machines.get(&id).cloned() else {
                    return Task::none();
                };
                for (label, agent, thread) in [
                    ("Claude", config::Agent::Claude, false),
                    ("Codex", config::Agent::Codex, false),
                    ("Claude thread", config::Agent::Claude, true),
                    ("Codex thread", config::Agent::Codex, true),
                    ("Terminal", config::Agent::Shell, false),
                ] {
                    let mut profile = Profile::agent(
                        &fl!("machine-profile-name", agent = label, name = machine.name.as_str()),
                        agent,
                        &machine.destination(),
                    );
                    profile.machine = Some(id);
                    profile.thread = thread;
                    if agent == config::Agent::Codex && !thread {
                        profile.args = agent::CODEX_FULL_ACCESS.to_string();
                    }
                    let profile_id = self
                        .config
                        .profiles
                        .last_key_value()
                        .map(|(id, _)| ProfileId(id.0 + 1))
                        .unwrap_or_default();
                    self.config.profiles.insert(profile_id, profile);
                }
                let host = machine.destination();
                self.host_setup.insert(host.clone(), None);
                self.host_progress(&host);
                return Task::batch([
                    self.save_profiles(),
                    setup_host_task(host),
                ]);
            }
            Message::ProfileOpen(profile_id) => {
                self.new_tab_menu_open = false;
                if let Some(dir) = self.thread_profile_dir(profile_id) {
                    return self.open_thread(self.pane_model.focused(), Some(profile_id), dir, None, None);
                }
                if self.wants_launcher(Some(profile_id)) {
                    return self.open_launcher(self.pane_model.focused(), profile_id);
                }
                return self.create_and_focus_new_terminal(
                    self.pane_model.focused(),
                    Some(profile_id),
                    false,
                );
            }
            Message::ProfileRemove(profile_id) => {
                // Reset matching terminals to default profile
                for (_pane, tab_model) in self.pane_model.panes.iter() {
                    for entity in tab_model.iter() {
                        if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                            let mut terminal = terminal.lock().unwrap();
                            if terminal.profile_id_opt == Some(profile_id) {
                                terminal.profile_id_opt = None;
                            }
                        }
                    }
                }
                if Some(profile_id) == self.get_default_profile() {
                    config_set!(default_profile, None);
                }
                self.config.profiles.remove(&profile_id);
                return self.save_profiles();
            }
            Message::ProfileSyntaxTheme(profile_id, color_scheme_kind, theme_i) => {
                match self
                    .theme_names(color_scheme_kind)
                    .get(theme_i)
                    .map(|x| x.to_string())
                {
                    Some(theme_name) => {
                        if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                            match color_scheme_kind {
                                ColorSchemeKind::Dark => {
                                    profile.syntax_theme_dark = theme_name;
                                }
                                ColorSchemeKind::Light => {
                                    profile.syntax_theme_light = theme_name;
                                }
                            }
                            return self.save_profiles();
                        }
                    }
                    None => {
                        log::warn!("failed to find syntax theme with index {}", theme_i);
                    }
                }
            }
            Message::ProfileTabTitle(profile_id, text) => {
                if let Some(profile) = self.config.profiles.get_mut(&profile_id) {
                    profile.tab_title = text;
                    return self.save_profiles();
                }
            }
            Message::SelectAll(entity_opt) => {
                if entity_opt.is_none() && self.active_thread().is_some() {
                    self.thread_select_all();
                    return Task::none();
                }
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        let mut terminal = terminal.lock().unwrap();
                        terminal.select_all();
                    }
                }
                return self.update_focus();
            }
            Message::ShowHeaderBar(show_headerbar) => {
                if show_headerbar != self.config.show_headerbar {
                    config_set!(show_headerbar, show_headerbar);
                    return self.update_config();
                }
            }
            Message::TabNewInheritWorkingDirectory(tab_new_inherit_working_directory) => {
                config_set!(
                    tab_new_inherit_working_directory,
                    tab_new_inherit_working_directory
                );
            }
            Message::AccentFollowsTab(follows) => {
                config_set!(accent_follows_tab, follows);
                return self.apply_theme();
            }
            Message::ShowPaneBorders(show_pane_borders) => {
                if show_pane_borders != self.config.show_pane_borders {
                    config_set!(show_pane_borders, show_pane_borders);
                }
            }
            Message::UseBrightBold(use_bright_bold) => {
                if use_bright_bold != self.config.use_bright_bold {
                    config_set!(use_bright_bold, use_bright_bold);
                    return self.update_config();
                }
            }
            Message::ShowAdvancedFontSettings(show) => {
                self.show_advanced_font_settings = show;
            }
            Message::SystemThemeChange => {
                return self.update_config();
            }
            Message::SyntaxTheme(color_scheme_kind, index) => {
                match self.theme_names(color_scheme_kind).get(index) {
                    Some(theme_name) => {
                        match color_scheme_kind {
                            ColorSchemeKind::Dark => {
                                config_set!(syntax_theme_dark, theme_name.to_string());
                            }
                            ColorSchemeKind::Light => {
                                config_set!(syntax_theme_light, theme_name.to_string());
                            }
                        }
                        return self.update_config();
                    }
                    None => {
                        log::warn!("failed to find syntax theme with index {}", index);
                    }
                }
            }
            Message::TabActivate(entity) => {
                if let Some(tab_model) = self.pane_model.active_mut() {
                    tab_model.activate(entity);
                }
                return Task::batch([
                    self.update_title(None),
                    self.refresh_focus(entity),
                    self.enforce_thread_limits(),
                ]);
            }
            Message::TabFocusFound(entity, result) => {
                let dir = match result {
                    Ok(found) => found
                        .agents
                        .first()
                        .and_then(|agent| agent.working_directory().map(str::to_string)),
                    Err(err) => {
                        log::debug!("could not find where a tab's agent works: {err}");
                        None
                    }
                };
                if let Some(pane) = self.tab_pane(entity)
                    && let Some(tab_model) = self.pane_model.panes.get_mut(pane)
                    && let Some(focus) = tab_model.data_mut::<TabFocus>(entity)
                {
                    focus.pending = false;
                    focus.checked = Some(Instant::now());
                    if dir.is_some() {
                        focus.dir = dir;
                    }
                }
            }
            Message::TabTransfer(entity_opt, profile_id, mode) => {
                let entity = entity_opt.or_else(|| self.pane_model.active().map(|t| t.active()));
                let Some(entity) = entity else {
                    return Task::none();
                };
                let Some(pane) = self.tab_pane(entity) else {
                    return Task::none();
                };
                if self.transfer_lookups.contains_key(&(pane, entity)) {
                    return Task::none();
                }
                let Some(tab_model) = self.pane_model.panes.get(pane) else {
                    return Task::none();
                };
                // A thread knows its conversation: no need to look for it
                if let Some(thread) = tab_model.data::<thread::tab::ThreadTab>(entity) {
                    let (Some(kind), Some(id)) = (agent_kind(thread.agent), thread.session_id.clone()) else {
                        return Task::none();
                    };
                    let choice = LaunchChoice::Transfer {
                        conversation: transfer::ConversationRef { host: thread.host.clone(), kind, id },
                        cwd: thread.cwd.clone(),
                        title: thread.title.clone(),
                        mode,
                    };
                    let pane = self.pane_model.focused();
                    let opened = self.open_launcher(pane, profile_id);
                    let Some(launcher) = self.pane_model.panes.get(pane).map(|t| t.active()) else {
                        return opened;
                    };
                    return Task::batch([opened, self.launch_from(launcher, choice)]);
                }
                let Some(session) = tab_model.data::<agent::Session>(entity).cloned() else {
                    return Task::none();
                };
                let title = tab_model.text(entity).unwrap_or_default().to_string();
                let request_id = cosmic_term::random_hex(16);
                self.transfer_lookups.insert((pane, entity), request_id.clone());
                return cosmic::task::future(async move {
                    let found = agent::tab_agents(agent::TabProcess::Session(session)).await;
                    Message::TabTransferFound(pane, entity, request_id, profile_id, mode, title, found)
                });
            }
            Message::TabTransferFound(pane, entity, request_id, profile_id, mode, title, found) => {
                if self.transfer_lookups.get(&(pane, entity)) != Some(&request_id) {
                    return Task::none();
                }
                self.transfer_lookups.remove(&(pane, entity));
                let Some(session) = self
                    .tab_pane(entity)
                    .and_then(|pane| self.pane_model.panes.get(pane))
                    .and_then(|tab_model| tab_model.data::<agent::Session>(entity).cloned())
                else {
                    return Task::none();
                };
                let wanted = session.agent();
                let agent = found.map_err(|err| fl!("tab-transfer-no-agent", error = err.as_str())).and_then(|found| {
                    found
                        .agents
                        .into_iter()
                        .find(|a| wanted.is_none() || a.agent() == wanted)
                        .ok_or_else(|| fl!("tab-transfer-no-agent", error = "no Claude or Codex is running in the tab"))
                });
                let agent = match agent {
                    Ok(agent) => agent,
                    Err(message) => return self.launcher_error(message),
                };
                let (Some(kind), Some(id)) = (agent.agent().and_then(agent_kind), agent.conversation.clone()) else {
                    return self.launcher_error(fl!("tab-transfer-no-agent", error = "its conversation was not found"));
                };
                let cwd = agent.working_directory().unwrap_or_default().to_string();
                let choice = LaunchChoice::Transfer {
                    conversation: transfer::ConversationRef { host: session.host.clone(), kind, id },
                    cwd,
                    title,
                    mode,
                };
                // The destination's launcher, with the transfer form open
                let pane = self.pane_model.focused();
                let opened = self.open_launcher(pane, profile_id);
                let Some(launcher) = self.pane_model.panes.get(pane).map(|t| t.active()) else {
                    return opened;
                };
                return Task::batch([opened, self.launch_from(launcher, choice)]);
            }
            Message::TabActivateJump(pos) => {
                if let Some(tab_model) = self.pane_model.active() {
                    // Length is always at least one so there shouldn't be a division by zero
                    let len = tab_model.iter().count();
                    // The typical pattern is that 1-8 selects tabs 1-8 while 9 selects the last tab
                    let pos = if pos >= 8 || pos > len - 1 {
                        len - 1
                    } else {
                        pos % len
                    };

                    let entity = tab_model.iter().nth(pos);
                    if let Some(entity) = entity {
                        return self.update(Message::TabActivate(entity));
                    }
                }
            }
            Message::TabClose(entity_opt) => {
                if let Some(tab_model) = self.pane_model.active() {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                    if self.transfer_lookups.contains_key(&(self.pane_model.focused(), entity)) {
                        return self.toasts.push(widget::toaster::Toast::new(fl!("launcher-busy-close")))
                            .map(cosmic::Action::App);
                    }
                    if self.launcher_locked(entity) {
                        return self.toasts.push(widget::toaster::Toast::new(fl!("launcher-busy-close")))
                            .map(cosmic::Action::App);
                    }
                }
                if let Some(tab_model) = self.pane_model.active_mut() {
                    let entity = entity_opt.unwrap_or_else(|| tab_model.active());

                    // Activate closest item if closing active tab
                    if entity == tab_model.active()
                        && let Some(position) = tab_model.position(entity)
                    {
                        if position > 0 {
                            tab_model.activate_position(position - 1);
                        } else {
                            tab_model.activate_position(position + 1);
                        }
                    }

                    // Remove item
                    tab_model.remove(entity);

                    // If that was the last tab, close current pane
                    if tab_model.iter().next().is_none() {
                        if let Some((_state, sibling)) =
                            self.pane_model.panes.close(self.pane_model.focused())
                        {
                            self.terminal_ids.remove(&self.pane_model.focused());
                            self.pane_model.set_focus(sibling);
                        } else {
                            //Last pane, closing window
                            if let Some(window_id) = self.core.main_window_id() {
                                return window::close(window_id);
                            }
                        }
                    }
                }

                return self.update_title(None);
            }
            Message::TabContextAction(entity, action) => {
                let mut tasks = Vec::new();
                // Close terminal context menu state
                if let Some(tab_model) = self.pane_model.active()
                    && let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity)
                {
                    let mut terminal = terminal.lock().unwrap();
                    //Some actions need the menu_state,
                    //so only clear the position for them.
                    match action {
                        // these read the link from the menu state and clear it themselves
                        Action::LaunchUrlByMenu
                        | Action::CopyUrlByMenu
                        | Action::OpenLocallyByMenu => {}
                        _ => {
                            terminal.context_menu = None;
                        }
                    }
                }
                tasks.push(self.update(action.message(Some(entity))));
                return cosmic::Task::batch(tasks);
            }
            Message::TabContextMenu(pane, menu_state) => {
                // Clear all terminal context_menu state
                for (_, tab_model) in self.pane_model.panes.iter() {
                    for entity in tab_model.iter() {
                        if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                            let mut terminal = terminal.lock().unwrap();
                            if terminal.context_menu.take().is_some() {
                                terminal.active_regex_match = None;
                                terminal.needs_update = true;
                            }
                        }
                    }
                }

                // A right press records what is under the cursor
                if let Some(menu_state) = menu_state
                    && let Some(tab_model) = self.pane_model.panes.get(pane)
                {
                    let entity = tab_model.active();
                    if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                        terminal.lock().unwrap().context_menu = Some(menu_state);
                    }
                    self.pane_model.set_focus(pane);
                }
            }
            Message::TabNew => {
                self.new_tab_menu_open = false;
                let from = self.active_thread().map(thread::app::ThreadOrigin::of);
                return self.new_tab_in(self.pane_model.focused(), from);
            }
            Message::TabBarContext(pane, _entity) => {
                // The menu's actions act on the focused pane
                self.pane_model.set_focus(pane);
            }
            Message::TabDragStart(pane, entity) => {
                self.tab_drag = Some((pane, entity, Instant::now()));
                self.tab_drag_left_window = false;
            }
            Message::TabDragOverWindow(over) => {
                if self.tab_drag.is_some() {
                    self.tab_drag_left_window = !over;
                }
            }
            Message::TabDragEnded(finished) => {
                // A reorder, or a drop back on this window, has already
                // cleared the drag
                let Some((pane, entity, started)) = self.tab_drag.take() else {
                    return Task::none();
                };
                // Pop out only a tab carried outside the window: not one whose
                // drag the compositor refused straight away, nor one released
                // over this window, such as by a click in the terminal
                if !finished
                    && (!self.tab_drag_left_window
                        || started.elapsed() < Duration::from_millis(500))
                {
                    return Task::none();
                }
                // A thread carried out of the window opens in one of its own
                if !finished
                    && self.pane_model.panes.get(pane).is_some_and(|t| t.data::<thread::tab::ThreadTab>(entity).is_some())
                {
                    self.pane_model.set_focus(pane);
                    return self.update(Message::TabPopOut(Some(entity)));
                }
                let has_session = self
                    .pane_model
                    .panes
                    .get(pane)
                    .is_some_and(|tab_model| tab_model.data::<agent::Session>(entity).is_some());
                if has_session {
                    self.pane_model.set_focus(pane);
                    return self.update(if finished {
                        // Another window has attached to the session
                        Message::TabClose(Some(entity))
                    } else {
                        // Dropped where nothing accepted it, such as the
                        // desktop
                        Message::TabPopOut(Some(entity))
                    });
                }
            }
            Message::TabDropped(pane, data) => {
                // A thread dragged to another pane of this window moves there
                if let Some((from, entity, _)) = self.tab_drag
                    && from != pane
                    && self.pane_model.panes.get(from).is_some_and(|t| t.data::<thread::tab::ThreadTab>(entity).is_some())
                {
                    self.tab_drag = None;
                    return self.move_thread(from, entity, pane);
                }
                if let Some(session) = data.and_then(|data| data.session) {
                    return self.receive_tab(pane, session);
                }
            }
            Message::TabPopOut(entity_opt) => {
                let Some(tab_model) = self.pane_model.active() else {
                    return Task::none();
                };
                let entity = entity_opt.unwrap_or_else(|| tab_model.active());
                // A thread reopens in the new window, continuing its conversation
                if let Some(thread) = tab_model.data::<thread::tab::ThreadTab>(entity) {
                    if thread.is_working() {
                        return Task::none();
                    }
                    let mut command = process::Command::new(match env::current_exe() {
                        Ok(exe) => exe,
                        Err(_) => return Task::none(),
                    });
                    command.arg("--thread").arg(&thread.cwd);
                    command.arg("--agent").arg(if thread.agent == config::Agent::Codex { "codex" } else { "claude" });
                    if !thread.host.is_empty() {
                        command.arg("--host").arg(&thread.host);
                    }
                    if let Some(session) = &thread.session_id {
                        command.arg("--resume").arg(session);
                    }
                    match command.spawn() {
                        Ok(_child) => return self.update(Message::TabClose(Some(entity))),
                        Err(err) => log::error!("failed to open a window for a thread: {err}"),
                    }
                    return Task::none();
                }
                let Some(session) = tab_model.data::<agent::Session>(entity).cloned() else {
                    return Task::none();
                };
                // The new window attaches before this one detaches
                match env::current_exe().and_then(|exe| {
                    process::Command::new(exe)
                        .arg("--attach")
                        .arg(session.descriptor())
                        .spawn()
                }) {
                    Ok(_child) => return self.update(Message::TabClose(Some(entity))),
                    Err(err) => log::error!("failed to open a window for {:?}: {}", session, err),
                }
            }
            Message::TabNewNoProfile => {
                return self.create_and_focus_new_terminal(
                    self.pane_model.focused(),
                    None,
                    self.config.tab_new_inherit_working_directory,
                );
            }
            Message::TabNext => {
                if let Some(tab_model) = self.pane_model.active() {
                    let len = tab_model.iter().count();
                    // Next tab position. Wraps around to 0 (first tab) if the last tab is active.
                    let pos = tab_model
                        .position(tab_model.active())
                        .map(|i| (i as usize + 1) % len)
                        .expect("at least one tab is always open");

                    let entity = tab_model.iter().nth(pos);
                    if let Some(entity) = entity {
                        return self.update(Message::TabActivate(entity));
                    }
                }
            }
            Message::TabPrev => {
                if let Some(tab_model) = self.pane_model.active() {
                    let pos = tab_model
                        .position(tab_model.active())
                        .and_then(|i| (i as usize).checked_sub(1))
                        .unwrap_or_else(|| tab_model.iter().count().saturating_sub(1));

                    let entity = tab_model.iter().nth(pos);
                    if let Some(entity) = entity {
                        return self.update(Message::TabActivate(entity));
                    }
                }
            }
            Message::TermEvent(pane, entity, event) => {
                match event {
                    TermEvent::Bell => {
                        //TODO: audible or visible bell options?
                    }
                    TermEvent::ClipboardLoad(kind, callback) => {
                        match kind {
                            term::ClipboardType::Clipboard => {
                                log::info!("clipboard load");
                                return clipboard::read().map(move |data_opt| {
                                    //TODO: what to do when data_opt is None?
                                    callback(&data_opt.unwrap_or_default());
                                    // We don't need to do anything else
                                    action::none()
                                });
                            }
                            term::ClipboardType::Selection => {
                                log::info!("TODO: load selection");
                            }
                        }
                    }
                    TermEvent::ClipboardStore(kind, data) => match kind {
                        term::ClipboardType::Clipboard => {
                            log::info!("clipboard store");
                            return clipboard::write(data);
                        }
                        term::ClipboardType::Selection => {
                            log::info!("TODO: store selection");
                        }
                    },
                    TermEvent::ColorRequest(index, f) => {
                        if let Some(tab_model) = self.pane_model.panes.get(pane)
                            && let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity)
                        {
                            let terminal = terminal.lock().unwrap();
                            let rgb = terminal.effective_color(index);
                            let text = f(rgb);
                            terminal.input_no_scroll(text.into_bytes());
                        }
                    }
                    TermEvent::CursorBlinkingChange => {
                        //TODO: should we blink the cursor?
                    }
                    TermEvent::Exit => {
                        // A disconnected tab stays, to be reconnected
                        let disconnected =
                            self.pane_model.panes.get(pane).is_some_and(|tab_model| {
                                tab_model
                                    .data::<TabConnection>(entity)
                                    .is_some_and(|connection| connection.lost.is_some())
                            });
                        if disconnected {
                            return Task::none();
                        }
                        return self.update(Message::TabClose(Some(entity)));
                    }
                    TermEvent::PtyWrite(text) => {
                        if let Some(tab_model) = self.pane_model.panes.get(pane)
                            && let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity)
                        {
                            let terminal = terminal.lock().unwrap();
                            terminal.input_no_scroll(text.into_bytes());
                        }
                    }
                    TermEvent::ResetTitle => {
                        if let Some(tab_model) = self.pane_model.panes.get_mut(pane) {
                            let tab_title_override =
                                if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                                    let terminal = terminal.lock().unwrap();
                                    terminal.tab_title_override.clone()
                                } else {
                                    None
                                };
                            let remote_label = tab_model
                                .data::<agent::Session>(entity)
                                .map(agent::Session::label);
                            tab_model.text_set(
                                entity,
                                tab_title_override
                                    .or(remote_label)
                                    .unwrap_or_else(|| fl!("new-terminal")),
                            );
                        }
                        return self.update_title(Some(pane));
                    }
                    TermEvent::TextAreaSizeRequest(f) => {
                        if let Some(tab_model) = self.pane_model.panes.get(pane)
                            && let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity)
                        {
                            let terminal = terminal.lock().unwrap();
                            let text = f(terminal.size().into());
                            terminal.input_no_scroll(text.into_bytes());
                        }
                    }
                    TermEvent::Title(title) => {
                        if let Some(tab_model) = self.pane_model.panes.get_mut(pane) {
                            let has_override =
                                if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                                    let terminal = terminal.lock().unwrap();
                                    terminal.tab_title_override.is_some()
                                } else {
                                    false
                                };
                            if !has_override {
                                let title = match tab_model.data::<agent::Session>(entity) {
                                    // An agent's tab is named after its conversation,
                                    // as in the sidebar, whatever title the agent sets
                                    Some(session) => match self.sidebar.title(session) {
                                        Some(conversation) => conversation.to_string(),
                                        None => agent::strip_zellij_title(&title, &session.name)
                                            .to_string(),
                                    },
                                    None => title,
                                };
                                let locked = tab_model
                                    .data::<agent::Session>(entity)
                                    .is_some_and(|s| self.secrets.for_session(&s.name).next().is_some());
                                let title = if locked { format!("{SUDO_LOCK}{title}") } else { title };
                                tab_model.text_set(entity, title);
                            }
                        }
                        // Agents retitle their tab as they work: a cue to see
                        // where they are working now
                        // The header follows the focused pane, whichever was retitled
                        return Task::batch([self.update_title(None), self.refresh_focus(entity)]);
                    }
                    TermEvent::MouseCursorDirty | TermEvent::Wakeup => {
                        if let Some(tab_model) = self.pane_model.panes.get(pane)
                            && let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity)
                        {
                            let mut terminal = terminal.lock().unwrap();
                            terminal.needs_update = true;
                        }
                    }
                    TermEvent::ChildExit(code) => {
                        // ssh lost the connection, rather than the session
                        // ending or the tab detaching
                        if code == agent::SSH_CONNECTION_LOST {
                            return self.tab_disconnected(pane, entity);
                        }
                    }
                }
            }
            Message::TermEventTx(term_event_tx) => {
                // Check if the terminal event channel was reset
                if self.term_event_tx_opt.is_some() {
                    // Close tabs using old terminal event channel
                    log::warn!("terminal event channel reset, closing tabs");

                    // First, close other panes
                    while let Some((_state, sibling)) =
                        self.pane_model.panes.close(self.pane_model.focused())
                    {
                        self.terminal_ids.remove(&self.pane_model.focused());
                        self.pane_model.set_focus(sibling);
                    }

                    // Next, close all tabs in the active pane
                    if let Some(tab_model) = self.pane_model.active_mut() {
                        let entities: Vec<_> = tab_model.iter().collect();
                        for entity in entities {
                            tab_model.remove(entity);
                        }
                    }
                }

                // Set new terminal event channel
                self.term_event_tx_opt = Some(term_event_tx);

                // Spawn first tab: a closed window's, when started plainly
                if self.restore_wanted
                    && let Some((claim, record)) = restore::Claim::previous()
                {
                    self.restore = Some(claim);
                    let reopened = self.reopen(record);
                    #[cfg(debug_assertions)]
                    let reopened = Task::batch([reopened, cosmic::task::message(Message::DebugStep(0))]);
                    if self.pane_model.active().is_some_and(|tabs| tabs.iter().next().is_some()) {
                        return reopened;
                    }
                    // Only sessions, which attach once found running
                    return Task::batch([reopened, self.update(Message::TabNew)]);
                }
                if self.restore.is_none() {
                    self.restore = restore::Claim::new();
                }
                if let Some(session) = self.startup_attach.take() {
                    return self.update(Message::RemoteAttach(session));
                }
                if let Some(start) = self.startup_thread.take() {
                    // A local thread profile of the agent, if there is one, for
                    // its arguments
                    let agent = if start.codex { config::Agent::Codex } else { config::Agent::Claude };
                    let profile_id = self
                        .config
                        .profiles
                        .iter()
                        .find(|(_, p)| p.thread && p.agent == agent && !p.is_remote())
                        .map(|(id, _)| *id);
                    let id = self.next_thread;
                    let mut opened = vec![self.open_thread_as(
                        self.pane_model.focused(),
                        profile_id,
                        start.dir,
                        start.resume,
                        None,
                        Some(agent),
                        start.host,
                    )];
                    if let Some(prompt) = start.prompt
                        && let Some(tab) = self.thread_mut(id)
                    {
                        tab.composer = widget::text_editor::Content::with_text(&prompt);
                        opened.push(self.thread_message(id, thread::tab::Msg::Send));
                    }
                    #[cfg(debug_assertions)]
                    opened.push(cosmic::task::message(Message::DebugStep(0)));
                    return Task::batch(opened);
                }
                return self.update(Message::TabNew);
            }
            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                    self.pane_model.update_terminal_focus();

                    if let ContextPage::KeyboardShortcuts = context_page {
                        self.shortcut_page_toggle();
                    }

                    #[cfg(feature = "password_manager")]
                    if ContextPage::PasswordManager == context_page {
                        if self.core.window.show_context {
                            self.password_mgr.pane = Some(self.pane_model.focused());
                            return self.password_mgr.refresh_password_list();
                        } else {
                            self.password_mgr.clear();
                        }
                    }
                    return self.update_focus();
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                    self.pane_model.unfocus_all_terminals();
                }

                // Extra work to do to prepare context pages
                if let ContextPage::ColorSchemes(color_scheme_kind) = self.context_page {
                    self.color_scheme_errors.clear();
                    self.color_scheme_expanded = None;
                    self.color_scheme_renaming = None;
                    self.color_scheme_tab_model = widget::segmented_button::Model::default();
                    let dark_entity = self
                        .color_scheme_tab_model
                        .insert()
                        .text(fl!("dark"))
                        .data(ColorSchemeKind::Dark)
                        .id();
                    let light_entity = self
                        .color_scheme_tab_model
                        .insert()
                        .text(fl!("light"))
                        .data(ColorSchemeKind::Light)
                        .id();
                    self.color_scheme_tab_model
                        .activate(match color_scheme_kind {
                            ColorSchemeKind::Dark => dark_entity,
                            ColorSchemeKind::Light => light_entity,
                        });
                }

                if let ContextPage::KeyboardShortcuts = context_page {
                    self.shortcut_page_toggle();
                    return self.update_focus();
                }

                #[cfg(feature = "password_manager")]
                if ContextPage::PasswordManager == context_page {
                    self.password_mgr.pane = Some(self.pane_model.focused());
                    return self.password_mgr.refresh_password_list();
                }
            }
            Message::UpdateDefaultProfile((default, profile_id)) => {
                config_set!(default_profile, default.then_some(profile_id));
            }
            Message::WindowClose => {
                self.close_tabs();
                if let Some(window_id) = self.core.main_window_id() {
                    return window::close(window_id);
                }
            }
            Message::WindowNew => match env::current_exe() {
                Ok(exe) => {
                    let mut command = process::Command::new(&exe);
                    if self.config.tab_new_inherit_working_directory
                        && let Some(dir) = self.active_terminal_working_directory()
                    {
                        command.arg("--working-directory");
                        command.arg(dir);
                    }
                    match command.spawn() {
                        Ok(_child) => {}
                        Err(err) => {
                            log::error!("failed to execute {:?}: {}", exe, err);
                        }
                    }
                }
                Err(err) => {
                    log::error!("failed to get current executable path: {}", err);
                }
            },
            Message::WindowFocused => {
                self.window_focused = true;
                if !self.core.window.show_context {
                    self.pane_model.update_terminal_focus();
                }
                return Task::batch([self.update_focus(), self.sudo_arm()]);
            }
            Message::WindowUnfocused => {
                self.window_focused = false;
                self.pane_model.unfocus_all_terminals();
            }
            Message::ZoomIn | Message::ZoomOut | Message::ZoomReset if self.active_thread().is_some() => {
                self.zoom_threads(match message {
                    Message::ZoomIn => Some(1),
                    Message::ZoomOut => Some(-1),
                    _ => None,
                });
            }
            Message::ZoomIn => {
                return self.update_render_active_pane_zoom(message);
            }
            Message::ZoomOut => {
                return self.update_render_active_pane_zoom(message);
            }
            Message::ZoomReset => {
                self.reset_active_pane_zoom();
                return self.update_config();
            }
            Message::Surface(a) => {
                return cosmic::task::message(cosmic::Action::Surface(a));
            }
            Message::ReorderTab(
                pane,
                ReorderEvent {
                    dragged,
                    target,
                    position,
                },
            ) => {
                // The tab stayed in this window
                self.tab_drag = None;
                let Some(p) = self.pane_model.panes.get_mut(pane) else {
                    log::error!("Failed to find reordered tab model.");
                    return Task::none();
                };
                _ = p.reorder(dragged, target, position);
            }
        }

        Task::none()
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |s| Message::LaunchUrl(s.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
            ContextPage::ColorSchemes(color_scheme_kind) => context_drawer::context_drawer(
                self.color_schemes(color_scheme_kind),
                Message::ToggleContextPage(ContextPage::ColorSchemes(color_scheme_kind)),
            )
            .title(fl!("color-schemes")),
            ContextPage::KeyboardShortcuts => context_drawer::context_drawer(
                self.keyboard_shortcuts(),
                Message::ToggleContextPage(ContextPage::KeyboardShortcuts),
            )
            .title(fl!("keyboard-shortcuts")),
            ContextPage::Profiles => context_drawer::context_drawer(
                self.profiles(),
                Message::ToggleContextPage(ContextPage::Profiles),
            )
            .title(fl!("profiles")),
            ContextPage::Settings => context_drawer::context_drawer(
                self.settings(),
                Message::ToggleContextPage(ContextPage::Settings),
            )
            .title(fl!("settings")),
            ContextPage::RemoteFiles => context_drawer::context_drawer(
                mount_setup_view::pane(self.mount_setup_machines(), &self.mount_setup, &self.mount_errors),
                Message::ToggleContextPage(ContextPage::RemoteFiles),
            ).title(fl!("remote-files-title")),
            ContextPage::Secrets => context_drawer::context_drawer(
                secrets::pane(&self.secrets, self.secret_machines()),
                Message::ToggleContextPage(ContextPage::Secrets),
            )
            .title(fl!("secrets-title")),
            #[cfg(feature = "password_manager")]
            ContextPage::PasswordManager => context_drawer::context_drawer(
                self.password_mgr.context_page(self.core.system_theme()),
                Message::ToggleContextPage(ContextPage::PasswordManager),
            )
            .title(fl!("passwords-title")),
        })
    }

    fn dialog(&self) -> Option<Element<'_, Message>> {
        // A sudo request whose session has no tab here
        let open = self.open_sessions();
        if let Some(pending) = self.secrets.pending.values().find(|p| {
            p.request
                .session
                .as_ref()
                .is_none_or(|session| !open.iter().any(|o| &o.name == session))
        }) {
            let session = pending.request.session.as_ref().map(|name| {
                self.sidebar
                    .title_by_name(name)
                    .map_or_else(|| name.clone(), str::to_string)
            });
            return Some(secrets::popup(pending, session));
        }
        if let Some((_, name)) = &self.rename {
            return Some(
                widget::dialog()
                    .title(fl!("rename-title"))
                    .body(fl!("rename-body"))
                    .control(
                        widget::text_input("", name)
                            .id(self.rename_id.clone())
                            .on_input(Message::RenameInput)
                            .on_submit(|_| Message::RenameSubmit),
                    )
                    .primary_action(
                        widget::button::suggested(fl!("rename"))
                            .on_press(Message::RenameSubmit),
                    )
                    .secondary_action(
                        widget::button::standard(fl!("cancel")).on_press(Message::RenameCancel),
                    )
                    .into(),
            );
        }
        let conflict = self.shortcut_conflict.as_ref()?;
        let binding = shortcuts::binding_display(&conflict.binding);
        let existing = shortcuts::action_label(conflict.existing_action);
        let new_action = shortcuts::action_label(conflict.new_action);
        let body = fl!(
            "shortcut-replace-body",
            binding = binding.as_str(),
            existing = existing.as_str(),
            new_action = new_action.as_str()
        );

        Some(
            widget::dialog()
                .title(fl!("shortcut-replace-title"))
                .body(body)
                .primary_action(
                    widget::button::suggested(fl!("replace"))
                        .on_press(Message::ShortcutConflictReplace),
                )
                .secondary_action(
                    widget::button::standard(fl!("cancel"))
                        .on_press(Message::ShortcutConflictCancel),
                )
                .into(),
        )
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let sidebar = widget::tooltip(
            widget::button::custom(icon_cache_get("sidebar-show-symbolic", 16))
                .on_press(Message::SidebarToggle)
                .padding(8)
                .class(if self.show_sidebar {
                    style::Button::Standard
                } else {
                    style::Button::Icon
                }),
            widget::text(fl!("sidebar-toggle")),
            widget::tooltip::Position::Bottom,
        );
        vec![
            sidebar.into(),
            menu_bar(&self.core, &self.config, &self.key_binds, &self.session_menu()),
        ]
    }

    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        // A split button, as in Ptyxis: the main part opens the default
        // profile and the arrow chooses a profile or session
        let new_tab = widget::tooltip(
            widget::button::custom(icon_cache_get("list-add-symbolic", 16))
                .on_press(Message::TabNew)
                .padding([8, 4, 8, 8])
                .class(style::Button::Icon),
            widget::text(fl!("new-tab")),
            widget::tooltip::Position::Bottom,
        );
        let arrow = widget::button::custom(icon_cache_get("pan-down-symbolic", 16))
            .on_press(Message::NewTabMenuToggle)
            .padding([8, 6, 8, 2])
            .class(if self.new_tab_menu_open && !self.new_tab_menu_in_sidebar {
                style::Button::Standard
            } else {
                style::Button::Icon
            });
        let mut popover = widget::popover(arrow)
            .on_close(Message::NewTabMenuClose)
            .position(widget::popover::Position::Bottom);
        if self.new_tab_menu_open && !self.new_tab_menu_in_sidebar {
            popover = popover.popup(self.new_tab_menu());
        }
        let open_in_zed = widget::tooltip(
            widget::button::custom(icon_cache_get("accessories-text-editor-symbolic", 16))
                .on_press(Message::OpenInZed)
                .padding(8)
                .class(style::Button::Icon),
            widget::text(fl!("open-in-zed")),
            widget::tooltip::Position::Bottom,
        );
        let open_in_files = widget::tooltip(
            widget::button::custom(icon_cache_get("folder-symbolic", 16))
                .on_press(Message::OpenInFiles)
                .padding(8)
                .class(style::Button::Icon),
            widget::text(fl!("open-in-files")),
            widget::tooltip::Position::Bottom,
        );
        vec![
            open_in_files.into(),
            open_in_zed.into(),
            widget::row::with_children(vec![new_tab.into(), popover.into()])
                .align_y(Alignment::Center)
                .into(),
        ]
    }

    fn view_window(&self, window_id: window::Id) -> Element<'_, Message> {
        if let Some(image) = self.image_windows.get(&window_id) {
            return thread::viewer::view_with(image, move |action| Some(Message::Viewer(window_id, action)));
        }
        match &self.dialog_opt {
            Some(dialog) => dialog.view(window_id),
            None => widget::text("Unknown window ID").into(),
        }
    }

    /// Creates a view after each update.
    fn view(&self) -> Element<'_, Self::Message> {
        let thread_style = thread::view::ThreadStyle::new(&self.config, &cosmic::theme::active());
        let t = self.core().system_theme();
        let cosmic = t.cosmic();
        let cosmic_theme::Spacing {
            space_xxxs,
            space_xxs,
            ..
        } = cosmic.spacing;

        let show_pane_borders =
            self.config.show_pane_borders && self.pane_model.panes.panes.len() > 1;
        let pane_corner_radius: iced::border::Radius = {
            let pad = f32::from(space_xxxs) / 2.0;
            cosmic
                .radius_s()
                .map(|r| if r > 0.0 { r + pad } else { 0.0 })
                .into()
        };
        let pane_grid = PaneGrid::new(&self.pane_model.panes, |pane, tab_model, _is_maximized| {
            let mut tab_column = widget::column::with_capacity(1);

            // Always shown, so that a window's only tab can be dragged to
            // another window
            {
                // A dragged tab carries its session, for the window it is
                // dropped on
                let drag_data: HashMap<segmented_button::Entity, Vec<u8>> = tab_model
                    .iter()
                    .filter_map(|entity| {
                        let session = tab_model.data::<agent::Session>(entity)?;
                        Some((entity, session.descriptor().into_bytes()))
                    })
                    .collect();
                let tab_bar = widget::container(
                        widget::tab_bar::horizontal(tab_model)
                            .enable_tab_drag(dnd::TAB_MIME.to_string())
                            .on_reorder(move |event| Message::ReorderTab(pane, event))
                            .tab_drag_data(move |entity| {
                                drag_data.get(&entity).cloned().unwrap_or_default()
                            })
                            .on_tab_drag_start(move |entity| Message::TabDragStart(pane, entity))
                            .on_tab_hover(move |entity| Message::TabHover(pane, entity))
                            .on_context(move |entity| Message::TabBarContext(pane, entity))
                            .context_menu(Some(menu::tab_menu(tab_model, &self.transfer_destinations(tab_model))))
                            .window_id_maybe(self.core.main_window_id())
                            .on_surface_action(Message::Surface)
                            .on_dnd_drop::<DndDrop>(move |_entity, data, _action| {
                                Message::TabDropped(pane, data)
                            })
                            .tab_drag_threshold(25.)
                            .button_height(32)
                            .button_spacing(space_xxs)
                            .on_activate(Message::TabActivate)
                            .on_close(|entity| Message::TabClose(Some(entity))),
                    )
                    .class(style::Container::Custom(Box::new(|theme| {
                        let cosmic = theme.cosmic();
                        cosmic::iced::widget::container::Style {
                            icon_color: Some(Color::from(cosmic.background(theme.transparent).on)),
                            text_color: Some(Color::from(cosmic.background(theme.transparent).on)),
                            background: Some(iced::Background::Color(
                                cosmic.background(theme.transparent).base.into(),
                            )),
                            border: iced::Border::default(),
                            shadow: iced::Shadow::default(),
                            snap: true,
                        }
                    })))
                    .width(Length::Fill);
                // The hovered tab's preview, as in the sidebar, centred below
                // that tab once the pointer has rested on it
                let preview = self
                    .hovered_tab
                    .filter(|(hovered_pane, _, _, since)| {
                        *hovered_pane == pane && since.elapsed() >= TAB_PREVIEW_DELAY
                    })
                    .and_then(|(_, entity, x, _)| {
                        let session = tab_model.data::<agent::Session>(entity)?;
                        let mark = tab_model.data::<TabMark>(entity).copied().unwrap_or_default();
                        Some((self.sidebar.tab_preview(session, mark)?, x))
                    });
                // Always in a popover, so that the tab bar keeps its state
                // (and the hover it reports) as the preview comes and goes
                let mut popover = widget::popover(tab_bar);
                if let Some((preview, x)) = preview {
                    popover = popover
                        .popup(preview)
                        .position(widget::popover::Position::BelowAt(x));
                }
                tab_column = tab_column.push(popover);
            }

            let entity = tab_model.active();
            if self.transfer_lookups.contains_key(&(pane, entity)) {
                tab_column = tab_column.push(widget::text::caption(fl!("tab-transfer-checking")));
            }
            if tab_model
                .data::<TabConnection>(entity)
                .is_some_and(|connection| connection.lost.is_some())
                && let Some(session) = tab_model.data::<agent::Session>(entity)
            {
                let host = session.host.clone();
                tab_column = tab_column.push(
                    widget::layer_container(
                        widget::row::with_children(vec![
                            widget::text::body(fl!("disconnected-banner", host = host.as_str()))
                                .width(Length::Fill)
                                .into(),
                            widget::button::standard(fl!("reconnect-now"))
                                .on_press(Message::ReconnectNow(host))
                                .into(),
                        ])
                        .align_y(Alignment::Center)
                        .spacing(space_xxs)
                        .padding(space_xxs),
                    )
                    .layer(cosmic_theme::Layer::Primary),
                );
            }
            // The tab's sudo requests: its session's, or its thread's relay's
            let sudo_session = tab_model.data::<agent::Session>(entity).cloned().or_else(|| {
                tab_model.data::<thread::tab::ThreadTab>(entity).and_then(|t| t.relay.clone())
            });
            if let Some(session) = sudo_session {
                for pending in self.secrets.for_session(&session.name) {
                    tab_column = tab_column.push(secrets::bar(pending));
                }
            }
            if tab_model.data::<TabShared>(entity).is_some() {
                tab_column = tab_column.push(secrets::banner(
                    None,
                    fl!("shared-banner"),
                    vec![
                            widget::button::standard(fl!("shared-go-there"))
                                .on_press(Message::SharedGoThere(entity))
                                .into(),
                            widget::button::standard(fl!("shared-move-here"))
                                .on_press(Message::SharedMoveHere(entity))
                                .into(),
                            widget::tooltip(
                                button::custom(icon_cache_get("window-close-symbolic", 16))
                                    .on_press(Message::SharedDismiss(entity))
                                    .padding(space_xxs)
                                    .class(style::Button::Icon),
                                widget::text::body(fl!("shared-keep-both")),
                                widget::tooltip::Position::Bottom,
                            )
                            .into(),
                    ],
                ));
            }
            let entity_middle_click = tab_model.active();
            let terminal_id = self
                .terminal_ids
                .get(&pane)
                .cloned()
                .unwrap_or_else(|| field_id("terminal"));
            if let Some(terminal) = tab_model.data::<Mutex<Terminal>>(entity) {
                let mut terminal_box = terminal_box(terminal, &self.key_binds)
                    .id(terminal_id)
                    .disabled(self.core.window.show_context)
                    .on_context_menu(move |menu_state| Message::TabContextMenu(pane, menu_state))
                    .on_middle_click(move || Message::MiddleClick(pane, Some(entity_middle_click)))
                    .on_open_hyperlink(Some(Box::new(Message::LaunchUrl)))
                    .on_window_focused(|| Message::WindowFocused)
                    .on_window_unfocused(|| Message::WindowUnfocused)
                    .on_click_focus(move || Message::TerminalClickFocus(pane))
                    .opacity(if t.transparent {
                        t.cosmic().alpha_map.blurred_alpha(t.cosmic().frosted)
                    } else {
                        self.config.opacity_ratio()
                    })
                    .padding(space_xxs)
                    .sharp_corners(self.core.window.sharp_corners)
                    .show_headerbar(self.config.show_headerbar)
                    .pane_border_radius(show_pane_borders.then_some(pane_corner_radius))
                    .border(pane_border(cosmic, t.transparent, show_pane_borders));

                if self.config.focus_follow_mouse {
                    terminal_box = terminal_box.on_mouse_enter(move || Message::MouseEnter(pane));
                }

                // The terminal records what was under the right press; the widget opens the
                // menu on release, so build it from that state
                let context_state = terminal.lock().unwrap().context_menu.clone();
                let session = tab_model.data::<agent::Session>(entity);
                // Files named by a remote session can be opened here
                let open_locally = session.is_some_and(|session| !session.is_local())
                    && context_state
                        .as_ref()
                        .is_some_and(|state| state.path().is_some());
                let context_link = context_state.map(|state| state.link);
                if context_link.is_some() {
                    terminal_box = terminal_box.context_menu_open(true);
                }
                let mut context_menu = widget::context_menu(
                    terminal_box,
                    context_link.map(|link| {
                        menu::context_menu(
                            &self.config,
                            &self.key_binds,
                            entity,
                            link,
                            session.is_some(),
                            open_locally,
                            self.paste_passwords(),
                        )
                    }),
                )
                .item_width(widget::menu::ItemWidth::Uniform(360))
                .on_close(Message::TabContextMenu(pane, None))
                .on_surface_action(Message::Surface);
                if let Some(window_id) = self.core.main_window_id() {
                    context_menu = context_menu.window_id(window_id);
                }
                tab_column = tab_column.push(context_menu);
            }

            if let Some(launcher) = tab_model.data::<Launcher>(entity) {
                tab_column = tab_column.push(self.launcher_view(entity, launcher));
            } else if let Some(thread) = tab_model.data::<thread::tab::ThreadTab>(entity) {
                tab_column = tab_column.push(thread::view::view(thread, &thread_style, cosmic::theme::is_dark()));
            }

            //Only draw find in the currently focused pane
            let in_thread = tab_model.data::<thread::tab::ThreadTab>(entity).is_some();
            if self.find && pane == self.pane_model.focused() && tab_model.data::<Launcher>(entity).is_none() {
                let find_input = widget::text_input::text_input(
                    fl!("find-placeholder"),
                    &self.find_search_value,
                )
                .id(self.find_search_id.clone())
                .on_input(Message::FindSearchValueChanged)
                // This is inverted for ease of use, usually in terminals you want to search
                // upwards, which is FindPrevious
                .on_submit(move |_| {
                    // A thread reads down, as a document does
                    if self.modifiers.contains(Modifiers::SHIFT) != in_thread {
                        Message::FindNext
                    } else {
                        Message::FindPrevious
                    }
                })
                .width(Length::Fixed(320.0))
                .trailing_icon(
                    button::custom(icon_cache_get("edit-clear-symbolic", 16))
                        .on_press(Message::FindSearchValueChanged(String::new()))
                        .class(style::Button::Icon)
                        .into(),
                );
                let find_widget = widget::row::with_children(vec![
                    find_input.into(),
                    widget::tooltip(
                        button::custom(icon_cache_get("go-up-symbolic", 16))
                            .on_press(Message::FindPrevious)
                            .padding(space_xxs)
                            .class(style::Button::Icon),
                        widget::text::body(fl!("find-previous")),
                        widget::tooltip::Position::Top,
                    )
                    .into(),
                    widget::tooltip(
                        button::custom(icon_cache_get("go-down-symbolic", 16))
                            .on_press(Message::FindNext)
                            .padding(space_xxs)
                            .class(style::Button::Icon),
                        widget::text::body(fl!("find-next")),
                        widget::tooltip::Position::Top,
                    )
                    .into(),
                    widget::space::horizontal().into(),
                    button::custom(icon_cache_get("window-close-symbolic", 16))
                        .on_press(Message::Find(false))
                        .padding(space_xxs)
                        .class(style::Button::Icon)
                        .into(),
                ])
                .align_y(Alignment::Center)
                .padding(space_xxs)
                .spacing(space_xxs);

                tab_column = tab_column
                    .push(widget::layer_container(find_widget).layer(cosmic_theme::Layer::Primary));
            }

            DndDestination::for_data::<DndDrop>(tab_column, move |data, action| {
                if let Some(data) = data {
                    if action == DndAction::Move {
                        Message::Drop(Some((pane, entity, data)))
                    } else {
                        log::warn!("unsuppported action: {:?}", action);
                        Message::Drop(None)
                    }
                } else {
                    Message::Drop(None)
                }
            })
            .apply(pane_grid::Content::new)
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .on_click(Message::PaneClicked)
        .on_resize(space_xxs, Message::PaneResized)
        .on_drag(Message::PaneDragged);

        //TODO: apply window border radius xs at bottom of window
        let content: Element<_> = if show_pane_borders {
            // Each pane draws its own border stroke. Painting a filled
            // container behind the grid instead would stack its alpha with the
            // translucent panes and wash out the blurred backdrop.
            pane_grid.spacing(space_xxxs).into()
        } else {
            pane_grid.into()
        };
        let open = self.open_sessions();
        // What other windows are told this one shows
        peers::set_open(open.iter().map(agent::Session::descriptor).collect());
        // The mounts this window may want: of the machines it shows
        let shown: BTreeSet<String> = open
            .iter()
            .map(|session| session.host.as_str())
            .chain(self.launcher_hosts())
            .filter(|host| !host.is_empty())
            .map(agent::mounts::name_for)
            .collect();
        agent::mounts::set_needed(&shown);
        if !self.show_sidebar {
            return widget::toaster(&self.toasts, content);
        }
        let menu = (self.new_tab_menu_open && self.new_tab_menu_in_sidebar)
            .then(|| self.new_tab_menu());
        let sidebar = self.sidebar.view(
            &open,
            self.active_session(),
            menu,
            |session| TabMark {
                color: self.session_color(session),
                remote: !session.is_local(),
                disconnected: false,
            },
            |session| self.session_destinations(session),
            self.core.main_window_id(),
            self.config.sidebar_compact,
            |session| self.secrets.for_session(&session.name).next().is_some(),
        );
        widget::toaster(
            &self.toasts,
            widget::row::with_children(vec![sidebar, widget::divider::vertical::default().into(), content]),
        )
    }

    fn system_theme_update(
        &mut self,
        _keys: &[&'static str],
        _new_theme: &cosmic::cosmic_theme::Theme,
    ) -> Task<Self::Message> {
        self.update(Message::SystemThemeChange)
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        struct ConfigSubscription;
        struct TerminalEventSubscription;
        struct PeersSubscription;
        struct SudoSubscription;

        Subscription::batch([
            // Idle agents stop, and threads unload, on time
            iced::time::every(Duration::from_secs(30)).map(|_| Message::ThreadTick),
            event::listen_with(|event, status, _window_id| match event {
                Event::Keyboard(KeyEvent::KeyPressed {
                    key,
                    physical_key,
                    modifiers,
                    text,
                    ..
                }) => Some(if status == event::Status::Ignored {
                    Message::KeyUnclaimed(modifiers, physical_key, key, text.map(|t| t.to_string()))
                } else {
                    Message::Key(modifiers, physical_key, key)
                }),
                Event::Keyboard(KeyEvent::ModifiersChanged(modifiers)) => {
                    Some(Message::Modifiers(modifiers))
                }
                Event::Mouse(MouseEvent::ButtonReleased(MouseButton::Left)) => {
                    Some(Message::CopyPrimary(None))
                }
                Event::Mouse(MouseEvent::WheelScrolled { delta }) => Some(Message::Wheel(_window_id, delta)),
                // How a tab dragged from this window was dropped
                Event::Dnd(DndEvent::Source(SourceEvent::Finished)) => {
                    Some(Message::TabDragEnded(true))
                }
                Event::Dnd(DndEvent::Source(SourceEvent::Cancelled)) => {
                    Some(Message::TabDragEnded(false))
                }
                // Whether a drag is over this window
                Event::Dnd(DndEvent::Offer(
                    _,
                    OfferEvent::Enter { .. } | OfferEvent::Motion { .. },
                )) => Some(Message::TabDragOverWindow(true)),
                Event::Dnd(DndEvent::Offer(_, OfferEvent::Leave)) => {
                    Some(Message::TabDragOverWindow(false))
                }
                _ => None,
            }),
            Subscription::run_with(TypeId::of::<SudoSubscription>(), |_| {
                stream::channel(
                    16,
                    |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                        // SAFETY: a plain call
                        let path = cosmic_term::sudo_control::control_path(unsafe { libc::getuid() });
                        loop {
                            // systemd starts the agent on connecting
                            if let Ok(stream) = tokio::net::UnixStream::connect(&path).await {
                                let (read, mut write) = stream.into_split();
                                let (agent, mut to_agent) = tokio::sync::mpsc::unbounded_channel();
                                let _ = output.send(Message::SudoConnected(agent)).await;
                                let mut lines = tokio::io::BufReader::new(read).lines();
                                loop {
                                    tokio::select! {
                                        line = lines.next_line() => match line {
                                            Ok(Some(line)) => {
                                                if let Ok(event) = serde_json::from_str(&line) {
                                                    let _ = output.send(Message::SudoEvent(event)).await;
                                                }
                                            }
                                            _ => break,
                                        },
                                        message = to_agent.recv() => match message {
                                            Some(message) => {
                                                let Ok(mut line) = serde_json::to_string(&message) else { continue };
                                                line.push('\n');
                                                if write.write_all(line.as_bytes()).await.is_err() {
                                                    break;
                                                }
                                            }
                                            None => break,
                                        },
                                    }
                                }
                                let _ = output.send(Message::SudoDisconnected).await;
                            }
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    },
                )
            }),
            Subscription::run_with(TypeId::of::<PeersSubscription>(), |_| {
                stream::channel(
                    16,
                    |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                        let (requests, mut received) = mpsc::unbounded_channel();
                        // Kept, to stay on the bus
                        let _connection = match peers::serve(requests).await {
                            Ok(connection) => Some(connection),
                            Err(err) => {
                                log::warn!("other windows cannot reach this one: {err}");
                                None
                            }
                        };
                        while let Some(request) = received.recv().await {
                            if output.send(Message::PeerRequest(request)).await.is_err() {
                                break;
                            }
                        }
                        std::future::pending::<()>().await;
                    },
                )
            }),
            Subscription::run_with(TypeId::of::<TerminalEventSubscription>(), |_| {
                stream::channel(
                    100,
                    |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
                        output.send(Message::TermEventTx(event_tx)).await.unwrap();

                        while let Some((pane, entity, event)) = event_rx.recv().await {
                            output
                                .send(Message::TermEvent(pane, entity, event))
                                .await
                                .unwrap();
                        }

                        panic!("terminal event channel closed");
                    },
                )
            }),
            cosmic_config::config_subscription(
                TypeId::of::<ConfigSubscription>(),
                Self::APP_ID.into(),
                CONFIG_VERSION,
            )
            .map(|update| {
                if !update.errors.is_empty() {
                    log::debug!(
                        "errors loading config {:?}: {:?}",
                        update.keys,
                        update.errors
                    );
                }
                Message::Config(Box::new(update.config))
            }),
            match &self.dialog_opt {
                Some(dialog) => dialog.subscription(),
                None => Subscription::none(),
            },
        ])
    }
}

/// Divider color painted behind the pane grid to form pane borders.
///
/// When blur is active the transparent container must be used, otherwise the
/// fill is opaque and hides the blurred desktop behind the window.
fn pane_divider_color(
    cosmic: &cosmic_theme::Theme,
    transparent: bool,
) -> cosmic::cosmic_theme::palette::Srgba {
    cosmic.background(transparent).divider
}

/// Border stroke drawn around each terminal pane.
///
/// The stroke sits on the pane edge rather than behind it, so the pane
/// interior keeps its own opacity and the blurred desktop stays visible.
fn pane_border(cosmic: &cosmic_theme::Theme, transparent: bool, show: bool) -> iced::Border {
    if show {
        iced::Border {
            color: Color::from(pane_divider_color(cosmic, transparent)),
            width: 1.0,
            ..Default::default()
        }
    } else {
        iced::Border::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{pane_border, pane_divider_color};

    #[test]
    fn disconnected_tabs_are_grey() {
        let mark = crate::TabMark {
            color: crate::TabColor::parse("#d97757"),
            remote: true,
            disconnected: false,
        };
        assert_eq!(mark.strip_color(), crate::TabColor::parse("#d97757"));
        let lost = crate::TabMark {
            disconnected: true,
            ..mark
        };
        assert_eq!(lost.strip_color(), Some(crate::DISCONNECTED_GREY));
    }

    #[test]
    fn long_text_keeps_both_ends() {
        assert_eq!(crate::ellipsize_middle("short", 10), "short");
        let shortened = crate::ellipsize_middle("~/Documents/Research/Repositories/project/src", 20);
        assert_eq!(shortened, "~/Documen…roject/src");
        assert_eq!(shortened.chars().count(), 20);
    }

    #[test]
    fn pane_border_is_drawn_only_when_enabled() {
        // With borders off the pane must render exactly as before, so the
        // stroke has to be zero width rather than a transparent hairline.
        let theme = cosmic::cosmic_theme::Theme::dark_default();
        assert_eq!(pane_border(&theme, true, false).width, 0.0);
        assert!(pane_border(&theme, true, true).width > 0.0);
    }

    #[test]
    fn pane_border_uses_the_divider_color() {
        // The stroke is the only thing marking the pane edge now that nothing
        // is painted behind the grid, so it must match the theme divider.
        let theme = cosmic::cosmic_theme::Theme::dark_default();
        let expected = cosmic::iced::Color::from(pane_divider_color(&theme, true));
        assert_eq!(pane_border(&theme, true, true).color, expected);
    }

    #[test]
    fn pane_divider_is_translucent_when_blur_is_active() {
        // The divider is painted as a full-window container behind the pane
        // grid. With blur active it must keep alpha < 1 so the frosted
        // backdrop still shows through instead of a flat opaque fill.
        let theme = cosmic::cosmic_theme::Theme::dark_default();
        let color = pane_divider_color(&theme, true);
        assert!(
            color.alpha < 1.0,
            "expected translucent divider, got alpha {}",
            color.alpha
        );
    }

    #[test]
    fn pane_divider_is_opaque_without_blur() {
        // Without blur there is nothing to show through, so the divider keeps
        // the fully opaque background color.
        let theme = cosmic::cosmic_theme::Theme::dark_default();
        let color = pane_divider_color(&theme, false);
        assert!(
            (color.alpha - 1.0).abs() < f32::EPSILON,
            "expected opaque divider, got alpha {}",
            color.alpha
        );
    }
}
