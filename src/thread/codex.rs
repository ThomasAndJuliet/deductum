// SPDX-License-Identifier: GPL-3.0-only

//! Driving `codex app-server`: JSON-RPC over stdio, one message a line.
//! Not yet implemented: see AGENTS-PLAN.md, "Codex threads".

use serde_json::Value;

use super::claude::Launch;
use super::model::Event;
use super::protocol::Command;

/// A conversation with `codex app-server`
pub struct Codex {
    cwd: String,
    resume: Option<String>,
}

impl Codex {
    pub fn new(launch: &Launch) -> Self {
        Self { cwd: launch.cwd.clone(), resume: launch.resume.clone() }
    }

    pub fn start(&mut self) -> Vec<String> {
        let _ = (&self.cwd, &self.resume);
        Vec::new()
    }

    pub fn command(&mut self, _command: Command) -> Vec<String> {
        Vec::new()
    }

    pub fn parse(&mut self, line: &str, _events: &mut Vec<Event>) -> Vec<String> {
        let _ = serde_json::from_str::<Value>(line);
        Vec::new()
    }
}

/// The `sh` script that starts the app-server in the launch's directory
pub fn script(launch: &Launch) -> String {
    let cwd = if launch.cwd.is_empty() { "$HOME".to_string() } else { crate::agent::sh_quote(&launch.cwd) };
    format!(
        "cd {cwd} 2>/dev/null || cd\n\
         unset OPENAI_API_KEY CODEX_API_KEY OPENAI_BASE_URL\n\
         exec codex app-server -c forced_login_method=\"chatgpt\"\n"
    )
}
