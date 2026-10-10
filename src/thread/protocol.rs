// SPDX-License-Identifier: GPL-3.0-only

//! What a thread asks of its agent, apart from how the agent's CLI speaks.
//!
//! A thread sends [`Command`]s; a [`Protocol`] turns them into the lines its
//! CLI reads, and the CLI's lines into [`Event`]s, keeping whatever state
//! the conversation with the CLI needs (request ids, the turn in progress).
//! Moving the lines is the transport's: a child process today
//! ([`super::session::run`]).

use super::claude::{self, Decision, Launch};
use super::codex::Codex;
use super::model::{Event, PermissionRequest};
use crate::config::Agent;

/// Something the user did that the agent must hear of
#[derive(Clone, Debug)]
pub enum Command {
    /// A turn: text, and images as media type and base64 data
    User { text: String, images: Vec<(String, String)> },
    /// Stop the turn in progress
    Interrupt,
    /// The answer to a permission prompt
    Answer { request: PermissionRequest, decision: Decision },
    SetModel(String),
    /// A permission mode, by the names the composer uses: `default`,
    /// `acceptEdits`, `auto`, `plan` and `bypassPermissions`
    SetMode(String),
}

/// How one agent's CLI speaks
pub enum Protocol {
    Claude,
    Codex(Box<Codex>),
}

impl Protocol {
    pub fn new(launch: &Launch) -> Self {
        match launch.agent {
            Agent::Codex => Self::Codex(Box::new(Codex::new(launch))),
            _ => Self::Claude,
        }
    }

    /// The lines that open the conversation
    pub fn start(&mut self) -> Vec<String> {
        match self {
            Self::Claude => vec![claude::initialize("init")],
            Self::Codex(codex) => codex.start(),
        }
    }

    /// Joining a CLI another client started, whose opening lines it never
    /// sees
    pub fn attached(&mut self) {
        if let Self::Codex(codex) = self {
            codex.attached();
        }
    }

    /// The lines for `command`
    pub fn command(&mut self, command: Command) -> Vec<String> {
        match self {
            Self::Claude => vec![match command {
                Command::User { text, images } => claude::user_message(&text, &images),
                Command::Interrupt => claude::control(&cosmic_term::random_hex(8), "interrupt", serde_json::json!({})),
                Command::Answer { request, decision } => claude::permission_response(&request, &decision),
                Command::SetModel(model) => claude::control(
                    &cosmic_term::random_hex(8),
                    "set_model",
                    serde_json::json!({"model": model}),
                ),
                Command::SetMode(mode) => claude::control(
                    &cosmic_term::random_hex(8),
                    "set_permission_mode",
                    serde_json::json!({"mode": mode}),
                ),
            }],
            Self::Codex(codex) => codex.command(command),
        }
    }

    /// The events in one line of the CLI's output, and any replies it needs
    pub fn parse(&mut self, line: &str, events: &mut Vec<Event>) -> Vec<String> {
        match self {
            Self::Claude => claude::parse_line(line, events).into_iter().collect(),
            Self::Codex(codex) => codex.parse(line, events),
        }
    }

    /// Text in the line that ends a turn, for a relay to tell when the agent
    /// is idle
    pub fn turn_done(&self) -> &'static str {
        match self {
            Self::Claude => "\"type\":\"result\"",
            Self::Codex(_) => "\"method\":\"turn/completed\"",
        }
    }
}
