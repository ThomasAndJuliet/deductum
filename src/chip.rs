// SPDX-License-Identifier: GPL-3.0-only

//! The header's status chip and the buttons beside it (Tom, 8 Nov, t32): the prover's logo and
//! toolchain, a mark and the state in words, and the memory, each with a tooltip of its own; then
//! pause (resume when paused or stopped, Restart after a crash) and stop. The chip is live status
//! (a spinner while it works, a check when checked), never a ▶ or ⏸. It and its buttons are
//! there in every state, at one width, so that nothing beside them moves.

use cosmic::iced::{Alignment, Border, Color, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, button};
use cosmic::{iced, theme};

use super::attention;
use super::shell::{backend_label, icon_button, sp};
use super::sidebar::{Mark, mark_widget, muted_red};
use super::{App, Message, Panel, Workspace};

/// What the prover is doing, as the chip says it
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// No session is open
    NoSession,
    Starting,
    /// Imports being loaded: how many are done, of how many
    Loading { done: usize, total: usize },
    /// The theory being checked, in percent
    Checking(u64),
    Checked,
    /// Checked, with these cells failed and unfinished
    Problems { failed: usize, unfinished: usize },
    Paused,
    Stopped,
    /// The prover stopped without being asked
    Crashed,
}

impl State {
    /// The words in the chip
    pub fn label(&self) -> String {
        match self {
            State::NoSession => "No session".into(),
            State::Starting => "Starting\u{2026}".into(),
            State::Loading { done, total } => format!("Loading imports \u{b7} {done} of {total}"),
            State::Checking(p) => format!("Checking \u{b7} {p} %"),
            State::Checked => "Checked".into(),
            State::Problems { failed, unfinished } => attention::words(*failed, *unfinished),
            State::Paused => "Paused".into(),
            State::Stopped => "Stopped".into(),
            State::Crashed => "The prover stopped unexpectedly".into(),
        }
    }

    /// The mark before the words: a spinner while it works, a check, a red dot for problems, an
    /// amber ring paused, a grey dot stopped, the error icon after a crash
    pub fn mark(&self) -> Mark {
        match self {
            State::NoSession => Mark::Blank,
            State::Starting | State::Loading { .. } | State::Checking(_) => Mark::Spinner,
            State::Checked => Mark::Check,
            State::Problems { .. } => Mark::RedDot,
            State::Paused => Mark::Ring,
            State::Stopped => Mark::Grey,
            State::Crashed => Mark::Error,
        }
    }

    fn tip(&self) -> String {
        match self {
            State::NoSession => "No session is open. Open a folder or pick a session in the sidebar.".into(),
            State::Starting => "The prover is starting. The document checks once it has.".into(),
            State::Loading { done, total } => format!("Loading the theory's imports ({done} of {total} loaded). Its own commands wait for them. Click for the prover's log."),
            State::Checking(p) => format!("Checking the theory: {p} % done. Click for the prover's log."),
            State::Checked => "Everything is checked. Click for the prover's log.".into(),
            State::Problems { failed, unfinished } => format!("{}. Click to go to the next failure.", attention::words(*failed, *unfinished)),
            State::Paused => "Paused: the prover is loaded but does not check. Resume with the \u{25b6} beside it.".into(),
            State::Stopped => "Stopped: the prover is not running and the buffers are kept. Start it again with the \u{25b6} beside it.".into(),
            State::Crashed => "The prover stopped without being asked. Restart it with the \u{21bb} beside it; the buffers are kept.".into(),
        }
    }
}

/// The theories this theory imports, transitively, as the prover reports them: how many have
/// finished checking, of how many
pub fn imports_progress(ws: &Workspace, t: &super::editor::Theory) -> (usize, usize) {
    let Some(theories) = ws.activity.as_ref().and_then(|a| a["theories"].as_array()) else { return (0, 0) };
    let find = |path: &str| theories.iter().find(|th| th["path"].as_str() == Some(path));
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<String> = find(&t.path)
        .and_then(|th| th["imports"].as_array())
        .map(|i| i.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let (mut done, mut total) = (0, 0);
    while let Some(p) = stack.pop() {
        if !seen.insert(p.clone()) {
            continue;
        }
        if let Some(th) = find(&p) {
            total += 1;
            if th["consolidated"].as_bool() == Some(true) {
                done += 1;
            }
            stack.extend(th["imports"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)));
        }
    }
    (done, total)
}

/// The state of the open session's prover
pub fn state_of(app: &App) -> State {
    let Some(ws) = &app.workspace else { return State::NoSession };
    let lifecycle = app.sessions.get(&(ws.machine.clone(), ws.sid.clone())).and_then(|s| s["lifecycle"].as_str()).unwrap_or("");
    match lifecycle {
        "Paused" => return State::Paused,
        "Stopped" => return State::Stopped,
        "Failed" => return State::Crashed,
        "Running" | "" => {}
        _ => return State::Starting,
    }
    let (Some(t), Some(act)) = (&ws.theory, &ws.activity) else { return State::Starting };
    let (finished, unprocessed, run_here, failed_cmds, consolidated) = t.status();
    let waiting = super::shell::pending_imports(ws, t);
    if !waiting.is_empty() && !waiting.iter().any(|w| w.2) {
        let (done, total) = imports_progress(ws, t);
        return State::Loading { done, total: total.max(done + waiting.len()) };
    }
    let busy = act["running"].as_array().is_some_and(|r| !r.is_empty())
        || act["theories"].as_array().is_some_and(|ts| ts.iter().any(|th| th["running"].as_u64().unwrap_or(0) > 0));
    if t.prover.status.is_null() {
        return State::Starting;
    }
    if busy || !consolidated {
        let total = (finished + unprocessed + run_here + failed_cmds).max(1);
        return State::Checking((finished + failed_cmds) * 100 / total);
    }
    let (failed, unfinished) = attention::counts(t);
    if failed + unfinished > 0 { State::Problems { failed, unfinished } } else { State::Checked }
}

/// Where the chip is: one width in every state, so that pause and stop, and the rest of the
/// header, stay where they are. A narrow window gets a narrower one without the memory
pub const CHIP_WIDTH: f32 = 410.0;
pub const CHIP_WIDTH_NARROW: f32 = 340.0;

pub fn chip_width(app: &App) -> f32 {
    if app.window_width < 1250.0 { CHIP_WIDTH_NARROW } else { CHIP_WIDTH }
}

/// The prover's logo (the backend's mark for now: the symbolic logos are not chosen yet, and
/// replacing this one function's body changes the chip, the sidebar and the new-session cards)
pub fn logo<'a>(backend: &str) -> Element<'a, Message> {
    super::shell::backend_glyph(backend)
}

fn memory_text(ws: &Workspace) -> Option<String> {
    ws.activity.as_ref().and_then(|a| a["prover"]["rss_mb"].as_u64()).map(|mb| format!("{:.1} GB", mb as f64 / 1000.0))
}

fn part_style() -> theme::Button {
    fn style(theme: &cosmic::Theme, hover: bool) -> button::Style {
        let c = theme.cosmic();
        let text: Color = c.on_bg_color().into();
        let mut s = button::Style::new();
        s.border_radius = 12.0.into();
        s.background = hover.then(|| Color { a: 0.08, ..text }.into());
        s.text_color = Some(text);
        s.icon_color = Some(text);
        s
    }
    theme::Button::Custom {
        active: Box::new(|_, t| style(t, false)),
        disabled: Box::new(|t| style(t, false)),
        hovered: Box::new(|_, t| style(t, true)),
        pressed: Box::new(|_, t| style(t, true)),
    }
}

fn part<'a>(content: Element<'a, Message>, tip: String, press: Option<Message>) -> Element<'a, Message> {
    widget::tooltip(
        button::custom(content).padding([3, 8]).class(part_style()).on_press_maybe(press),
        widget::text(tip),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

fn pill<'a>(width: f32, content: Element<'a, Message>) -> Element<'a, Message> {
    widget::container(content)
        .width(Length::Fixed(width))
        .height(Length::Fixed(32.0))
        .center_x(Length::Fixed(width))
        .center_y(Length::Fixed(32.0))
        .class(theme::Container::custom(|theme| {
            let c = theme.cosmic();
            let text: Color = c.on_bg_color().into();
            iced::widget::container::Style {
                background: Some(Color::from(c.bg_component_color()).into()),
                border: Border { radius: 16.0.into(), width: 1.0, color: Color { a: 0.10, ..text } },
                text_color: Some(text),
                ..Default::default()
            }
        }))
        .into()
}

/// The chip: logo and toolchain, mark and state, memory
pub fn chip(app: &App) -> Element<'_, Message> {
    let s = sp();
    let state = state_of(app);
    let width = chip_width(app);
    let Some(ws) = &app.workspace else {
        return widget::tooltip(
            pill(width, widget::text::body(state.label()).class(super::view::muted()).into()),
            widget::text(state.tip()),
            widget::tooltip::Position::Bottom,
        )
        .into();
    };
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let backend = summary.and_then(|s| s["backend"].as_str()).unwrap_or("isabelle").to_string();
    let toolchain = summary.and_then(|s| s["toolchain"]["label"].as_str()).filter(|l| !l.is_empty()).map(str::to_string);
    let machine = app.link(&ws.machine).map(|l| l.name.clone()).unwrap_or_else(|| ws.machine.clone());
    let narrow = app.window_width < 1250.0;
    let gap = s.space_xs;
    let row_of = || widget::row::with_capacity(5).spacing(gap).align_y(Alignment::Center);
    let log = Some(Message::Panel(Panel::Log));

    // The logo and the toolchain (a starting prover's toolchain is not said yet)
    let mut first = row_of().push(logo(&backend));
    let shown_toolchain = toolchain.clone().filter(|_| state != State::Starting && !(narrow && state == State::Crashed));
    if let Some(tc) = &shown_toolchain {
        first = first.push(widget::text::body(tc.clone()).wrapping(iced::widget::text::Wrapping::None));
    }
    let tool_tip = match &toolchain {
        Some(tc) => format!("{} {tc}, running on {machine}", backend_label(&backend)),
        None => format!("{} on {machine}", backend_label(&backend)),
    };

    // The mark and the state
    let mut second = row_of();
    if state.mark() != Mark::Blank {
        second = second.push(mark_widget(state.mark()));
    }
    let words = widget::text::body(state.label()).wrapping(iced::widget::text::Wrapping::None);
    let words = if state == State::Crashed {
        words.class(theme::Text::Custom(|t| iced::widget::text::Style { color: Some(muted_red(t)), ..Default::default() }))
    } else {
        words
    };
    second = second.push(words);
    let state_press = match state {
        State::Problems { failed, .. } if failed > 0 => Some(Message::Shell(super::shell::Msg::NextProblem(attention::Problem::Failed))),
        State::Problems { .. } => Some(Message::Shell(super::shell::Msg::NextProblem(attention::Problem::Unfinished))),
        _ => log.clone(),
    };

    let mut parts = row_of().spacing(2);
    parts = parts.push(part(first.into(), tool_tip, log.clone()));
    parts = parts.push(super::shell::divider(16.0));
    parts = parts.push(part(second.into(), state.tip(), state_press));
    if !narrow && !matches!(state, State::Starting | State::Stopped | State::Crashed) {
        if let Some(mem) = memory_text(ws) {
            parts = parts.push(super::shell::divider(16.0));
            parts = parts.push(part(
                widget::text::body(mem.clone()).wrapping(iced::widget::text::Wrapping::None).into(),
                format!("The prover is using {mem} of this machine's memory"),
                log,
            ));
        }
    }
    pill(width, parts.align_y(Alignment::Center).into())
}

/// What the buttons beside the chip are, by the state
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Controls {
    /// Pause, resume (▶) or Restart (↻): its icon, tooltip and the lifecycle op it sends (none:
    /// disabled)
    pub first: (&'static str, &'static str, Option<&'static str>),
    /// Stop: its tooltip, and whether it can be pressed
    pub stop: (&'static str, bool),
}

pub fn controls_for(state: &State) -> Controls {
    const PAUSE: &str = "Pause: keep the prover loaded, stop checking";
    const STOP: &str = "Stop the prover (buffers are kept)";
    match state {
        State::NoSession => Controls { first: ("media-playback-pause-symbolic", "Pause (no session is open)", None), stop: ("Stop (no session is open)", false) },
        State::Paused => Controls { first: ("media-playback-start-symbolic", "Resume checking", Some("resume")), stop: (STOP, true) },
        State::Stopped => Controls { first: ("media-playback-start-symbolic", "Start the prover again", Some("resume")), stop: ("The prover is stopped", false) },
        State::Crashed => Controls { first: ("view-refresh-symbolic", "Restart the prover", Some("resume")), stop: ("The prover is not running", false) },
        _ => Controls { first: ("media-playback-pause-symbolic", PAUSE, Some("pause")), stop: (STOP, true) },
    }
}

/// Pause (or resume or Restart) and stop, always there, disabled when they cannot act
pub fn controls(app: &App) -> Vec<Element<'_, Message>> {
    let c = controls_for(&state_of(app));
    vec![
        icon_button(c.first.0, c.first.1, c.first.2.map(Message::Lifecycle)),
        icon_button("system-shutdown-symbolic", c.stop.0, c.stop.1.then_some(Message::Lifecycle("stop"))),
    ]
}
