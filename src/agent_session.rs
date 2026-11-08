// SPDX-License-Identifier: GPL-3.0-only

//! The header's agent-session button (Tom, 8 Nov): a click opens a popover to choose what the new
//! agent may edit and which AI tool it is; "Open agent session" then grants that scope to an id
//! Deductum gives the agent (`deductum-<session>-<n>`, passed as `DEDUCTUM_AGENT_ID`, which the
//! MCP adapter already reads as the agent's identity) and runs
//! `cosmic-term --new-thread <folder> [--host <machine>] --agent <tool> --prompt <context>
//! --env DEDUCTUM_AGENT_ID=<id>` (Terminal Agents' draft PR #183) on the host system, as the
//! folder, Zed and terminal openers are. Not in the Windows build.

use cosmic::iced::{Alignment, Border, Color, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, button};
use cosmic::{iced, theme};

use super::settings::Rights;
use super::shell::{Msg, icon_handle, sp};
use cosmic::Application as _;
use super::{App, Message, file_name, link};

/// The AI tools a thread can run
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
}

impl Tool {
    pub const ALL: [Tool; 2] = [Tool::Claude, Tool::Codex];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Claude => "Claude",
            Tool::Codex => "Codex",
        }
    }

    /// What `--agent` is given
    pub fn arg(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
        }
    }
}

/// The popover: open or not, and what it has chosen so far
#[derive(Clone, Debug)]
pub struct Popover {
    pub open: bool,
    pub rights: Rights,
    pub tool: Tool,
    /// The number in the agent's id, fixed when the popover opens
    pub n: u64,
}

impl Default for Popover {
    fn default() -> Self {
        Self { open: false, rights: Rights::default(), tool: Tool::Claude, n: 0 }
    }
}

impl Popover {
    /// Opened with the default rights of Settings ▸ Agents, Claude, and a fresh number
    pub fn opened(default_rights: Rights, n: u64) -> Self {
        Self { open: true, rights: default_rights, tool: Tool::Claude, n }
    }
}

/// What the new agent is attached to
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub session_id: String,
    pub session_name: String,
    /// The session's folder, on the machine it runs on
    pub folder: String,
    /// The machine, as the person knows it ("neo", or this computer's name)
    pub machine: String,
    /// What `--host` is given: the machine's alias destination, none for this computer
    pub host: Option<String>,
    pub theory_path: Option<String>,
    pub lemma: Option<String>,
    /// The line the caret is on, from 1
    pub line: Option<usize>,
}

impl Context {
    pub fn theory_name(&self) -> Option<&str> {
        self.theory_path.as_deref().map(file_name)
    }

    /// "In Timed_Circus on neo · attached to Stop.thy, lemma map_tock_length"
    pub fn summary(&self) -> String {
        let folder = file_name(&self.folder);
        let mut s = format!("In {folder} on {}", self.machine);
        if let Some(theory) = self.theory_name() {
            s.push_str(&format!(" \u{b7} attached to {theory}"));
            if let Some(lemma) = &self.lemma {
                s.push_str(&format!(", lemma {lemma}"));
            }
        }
        s
    }
}

/// The machine's destination for `--host`: the ssh destination Deductum's link uses, which is the
/// machine's alias (`twright@neo`); a tailnet name would bypass the alias's proxy, so the machine's
/// own name stands in for one
pub fn host_arg(target: &link::Target) -> Option<String> {
    let destination = target.destination.as_ref()?;
    let host = destination.rsplit('@').next().unwrap_or(destination);
    if host.ends_with(".ts.net") || host.contains(".tail") {
        Some(target.key.clone())
    } else {
        Some(destination.clone())
    }
}

/// What the open session and theory say about where a new agent would be attached
pub fn context(app: &App) -> Option<Context> {
    let ws = app.workspace.as_ref()?;
    let folder = ws.info.as_ref()?["folder"].as_str()?.to_string();
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let link = app.link(&ws.machine)?;
    let (mut lemma, mut line) = (None, None);
    if let Some(t) = &ws.theory {
        lemma = t.blocks.iter().find(|b| b.start <= t.cursor && t.cursor < b.end.max(b.start + 1)).and_then(|b| super::shell::block_name(t, b));
        line = Some(t.local.chars().take(t.cursor).filter(|c| *c == '\n').count() + 1);
    }
    Some(Context {
        session_id: ws.sid.clone(),
        session_name: summary.and_then(|s| s["name"].as_str()).unwrap_or(&ws.sid).to_string(),
        folder,
        machine: link.name.clone(),
        host: host_arg(&link.target),
        theory_path: ws.theory.as_ref().map(|t| t.path.clone()),
        lemma,
        line,
    })
}

/// The agent's id: `deductum-<session>-<n>`
pub fn agent_id(session: &str, n: u64) -> String {
    format!("deductum-{session}-{n}")
}

/// The key the host knows the agent by: the MCP adapter reads the id as provider and conversation
/// both (`DEDUCTUM_AGENT_ID` has no colon), and the host joins them
pub fn agent_key(id: &str) -> String {
    format!("{id}:{id}")
}

/// The host's scope for the rights chosen, none for Ask first
pub fn scope(rights: Rights) -> Option<&'static str> {
    match rights {
        Rights::Ask => None,
        Rights::Proof => Some("proof"),
        Rights::Theory => Some("theory"),
        Rights::Folder => Some("folder"),
    }
}

/// The first message of the new thread: where it is attached, and to use the deductum tools
pub fn prompt(ctx: &Context, rights: Rights) -> String {
    let mut lines = vec![
        "You are attached to a live Deductum proof session. Use the deductum MCP tools (proof_sessions, proof_read, proof_state, proof_check, proof_run_tool, proof_search) to read and check the proof in the session the human has open; do not edit its files directly.".to_string(),
        format!("Deductum session: {} (id {}), on {}.", ctx.session_name, ctx.session_id, ctx.machine),
        format!("Session folder: {}", ctx.folder),
    ];
    if let Some(path) = &ctx.theory_path {
        lines.push(format!("Theory file: {} ({path})", file_name(path)));
    }
    match (&ctx.lemma, ctx.line) {
        (Some(lemma), Some(line)) => lines.push(format!("Lemma: {lemma}, at line {line}.")),
        (None, Some(line)) => lines.push(format!("The human's caret is at line {line}.")),
        _ => {}
    }
    lines.push(
        match rights {
            Rights::Ask => "Editing rights: none yet. Ask for them with proof_request_access, saying why; the human answers in Deductum.",
            Rights::Proof => "Editing rights: you may edit this proof.",
            Rights::Theory => "Editing rights: you may edit this theory.",
            Rights::Folder => "Editing rights: you may edit any document in the session's folder.",
        }
        .to_string(),
    );
    lines.join("\n")
}

/// The command: what `cosmic-term` is given, the program first. `--host` is left out for this
/// computer
pub fn command(ctx: &Context, tool: Tool, id: &str, prompt: &str) -> Vec<String> {
    let mut args = vec!["cosmic-term".to_string(), "--new-thread".to_string(), ctx.folder.clone()];
    if let Some(host) = &ctx.host {
        args.push("--host".into());
        args.push(host.clone());
    }
    args.extend(["--agent".to_string(), tool.arg().to_string(), "--prompt".to_string(), prompt.to_string(), "--env".to_string(), format!("DEDUCTUM_AGENT_ID={id}")]);
    args
}

/// What runs on the host to start `command`'s program: Terminal Agents' `cosmic-term`, else its
/// Flatpak, and a plain error if neither is there or this one does not know `--new-thread`
const LAUNCH: &str = r#"if command -v cosmic-term >/dev/null 2>&1; then t="cosmic-term"
elif flatpak info dev.twright.CosmicTermAgents >/dev/null 2>&1; then t="flatpak run dev.twright.CosmicTermAgents"
else echo "Terminal Agents (cosmic-term) is not installed on this computer" >&2; exit 127; fi
shift
if ! $t --help 2>&1 | grep -q -e --new-thread; then echo "This Terminal Agents does not know --new-thread yet: update it" >&2; exit 126; fi
exec setsid -f $t "$@""#;

/// The arguments the host's shell is given: the script, then the command's arguments as they are
pub fn launch_args(command: &[String]) -> Vec<String> {
    let mut args = vec!["-c".to_string(), LAUNCH.to_string(), "sh".to_string()];
    args.extend(command.iter().cloned());
    args
}

/// Start the thread's page in Terminal Agents, on the host system
pub async fn launch(command: Vec<String>) -> Result<(), String> {
    let mut run = link::host_command("sh", &launch_args(&command));
    run.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), run.output())
        .await
        .map_err(|_| "Terminal Agents did not answer in time".to_string())?
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if said.is_empty() { "Terminal Agents could not be started".to_string() } else { said })
    }
}

// ----- the update

impl App {
    pub fn agent_update(&mut self, msg: Msg) -> cosmic::app::Task<Message> {
        use cosmic::app::Task;
        match msg {
            Msg::AgentOpen(open) => {
                self.shell.agent = if open && context(self).is_some() {
                    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_secs());
                    Popover::opened(self.shell.settings.agents.default_rights, n)
                } else {
                    Popover::default()
                };
            }
            Msg::AgentRights(r) => self.shell.agent.rights = r,
            Msg::AgentTool(t) => self.shell.agent.tool = t,
            Msg::AgentStart => {
                let Some(ctx) = context(self) else { return Task::none() };
                let (rights, tool, n) = (self.shell.agent.rights, self.shell.agent.tool, self.shell.agent.n);
                self.shell.agent = Popover::default();
                let id = agent_id(&ctx.session_id, n);
                let mut tasks = Vec::new();
                // The rights are in place before the agent starts: the host takes the grant for an id it has not seen
                if let Some(scope) = scope(rights) {
                    tasks.push(self.update(Message::Grant(agent_key(&id), tool.label().to_string(), scope)));
                }
                let argv = command(&ctx, tool, &id, &prompt(&ctx, rights));
                tasks.push(cosmic::task::future(async move { Message::Shell(Msg::AgentLaunched(launch(argv).await)) }));
                return Task::batch(tasks);
            }
            Msg::AgentLaunched(Err(error)) => return self.toast(error, false),
            Msg::AgentLaunched(Ok(())) => {}
            _ => {}
        }
        Task::none()
    }
}

// ----- the button and the popover

/// A bar of choices, sized to its parts, the chosen one filled with the muted accent
fn choices<'a>(parts: Vec<(String, Msg)>, chosen: usize) -> Element<'a, Message> {
    let mut bar = widget::row::with_capacity(parts.len()).align_y(Alignment::Center);
    for (i, (label, msg)) in parts.into_iter().enumerate() {
        bar = bar.push(
            widget::button::text(label)
                .class(if i == chosen { super::tones::muted_button(theme::Button::Text) } else { theme::Button::Text })
                .on_press(Message::Shell(msg)),
        );
    }
    widget::container(bar)
        .class(theme::Container::custom(|t| {
            let c = t.cosmic();
            iced::widget::container::Style {
                border: Border { radius: c.corner_radii.radius_xl.into(), width: 1.0, color: c.bg_divider().into() },
                ..Default::default()
            }
        }))
        .into()
}

/// The popover's body (t22): what the agent is attached to, what it may edit, which tool, and the
/// two buttons
pub fn popover_body<'a>(app: &'a App, ctx: &Context) -> Element<'a, Message> {
    let s = sp();
    let pop = &app.shell.agent;
    let label = |t: &'static str| widget::container(widget::text::body(t)).width(Length::Fixed(80.0));
    let rights = choices(
        Rights::ALL.iter().map(|r| (r.label().to_string(), Msg::AgentRights(*r))).collect(),
        Rights::ALL.iter().position(|r| *r == pop.rights).unwrap_or(0),
    );
    let tools = choices(Tool::ALL.iter().map(|t| (t.label().to_string(), Msg::AgentTool(*t))).collect(), Tool::ALL.iter().position(|t| *t == pop.tool).unwrap_or(0));
    let note = match pop.rights {
        Rights::Ask => "It may read and run proof tools, and asks you before it edits.",
        _ => "It may always read and run proof tools. Change this later in the agent\u{2019}s row.",
    };
    let body = widget::column::with_capacity(8)
        .spacing(s.space_s)
        .push(widget::text::heading("New agent session"))
        .push(widget::text::caption(ctx.summary()))
        .push(widget::row::with_capacity(2).spacing(s.space_s).align_y(Alignment::Center).push(label("May edit")).push(rights))
        .push(widget::row::with_capacity(2).push(widget::Space::new().width(Length::Fixed(80.0 + s.space_s as f32))).push(widget::text::caption(note)))
        .push(widget::row::with_capacity(2).spacing(s.space_s).align_y(Alignment::Center).push(label("AI tool")).push(tools))
        .push(
            widget::row::with_capacity(2)
                .spacing(s.space_xs)
                .push(widget::button::suggested("Open agent session").on_press(Message::Shell(Msg::AgentStart)))
                .push(super::view::secondary("Cancel", Some(Message::Shell(Msg::AgentOpen(false))))),
        );
    widget::container(body)
        .padding(s.space_m)
        .width(Length::Fixed(580.0))
        .class(theme::Container::custom(|t| {
            let c = t.cosmic();
            iced::widget::container::Style {
                background: Some(Color::from(c.background(t.transparent).component.base).into()),
                border: Border { radius: c.corner_radii.radius_m.into(), width: 1.0, color: c.bg_divider().into() },
                text_color: Some(c.on_bg_color().into()),
                ..Default::default()
            }
        }))
        .into()
}

/// The split button: the agent-session icon (a click opens the popover) and a ⌄ that opens the menu
/// of the agent session, Files, Zed and a terminal; the popover hangs under it while it is open
pub fn split(app: &App) -> Element<'_, Message> {
    let ctx = context(app);
    let [tl, _tr, _br, bl] = theme::active().cosmic().corner_radii.radius_xl;
    let [_, tr, br, _] = theme::active().cosmic().corner_radii.radius_xl;
    let space = sp();
    let icon = button::custom(widget::icon(icon_handle("ai-session-symbolic")).size(16))
        .class(super::tones::toggled_rounded(app.shell.agent.open, [tl, 0.0, 0.0, bl]))
        .padding([space.space_xxs, space.space_xs])
        .on_press_maybe(ctx.as_ref().map(|_| Message::Shell(Msg::AgentOpen(!app.shell.agent.open))));
    let tip = if ctx.is_some() { "New agent session: choose what it may edit" } else { "Open a session to start an agent in its folder" };
    let icon = household_widgets::tooltip_with(icon, widget::text(tip), widget::tooltip::Position::Bottom);

    let folder = ctx.is_some();
    let row = |label: &str, opener: link::Opener| {
        if folder {
            super::menubar::Row::Item { label: label.into(), keys: "", message: Message::OpenFolder(opener), bold: false }
        } else {
            super::menubar::Row::Off { label: label.into(), keys: "", why: "Open a session first".into() }
        }
    };
    let first = if folder {
        super::menubar::Row::Item { label: "New agent session".into(), keys: "", message: Message::Shell(Msg::AgentOpen(true)), bold: true }
    } else {
        super::menubar::Row::Off { label: "New agent session".into(), keys: "", why: "Open a session first".into() }
    };
    let rows = vec![first, row("Open in Files", link::Opener::Files), row("Open in Zed", link::Opener::Zed), row("Open a terminal here", link::Opener::Terminal)];
    let arrow = button::custom(widget::icon(icon_handle("pan-down-symbolic")).size(12))
        .class(super::tones::toggled_rounded(false, [0.0, tr, br, 0.0]))
        .padding([space.space_xxs, space.space_xxs]);
    let arrow = household_widgets::tooltip_with(arrow, widget::text("Open in\u{2026}: the agent session, Files, Zed or a terminal"), widget::tooltip::Position::Bottom);
    let menu = super::menubar::dropdown(arrow.into(), rows);

    let joined = widget::container(widget::row::with_capacity(2).align_y(Alignment::Center).push(icon).push(menu))
        .class(theme::Container::custom(|t| {
            let c = t.cosmic();
            iced::widget::container::Style {
                border: Border { radius: c.corner_radii.radius_xl.into(), width: 1.0, color: c.bg_divider().into() },
                ..Default::default()
            }
        }));
    let mut popover = widget::popover(joined).position(widget::popover::Position::Bottom).on_close(Message::Shell(Msg::AgentOpen(false)));
    if app.shell.agent.open
        && let Some(ctx) = &ctx
    {
        popover = popover.popup(popover_body(app, ctx));
    }
    popover.into()
}
