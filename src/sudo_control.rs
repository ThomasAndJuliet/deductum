// SPDX-License-Identifier: GPL-3.0-only

//! What terminal windows and `ct-sudo-agent` say to each other, as JSON
//! lines on the agent's control socket. The agent tells every window about
//! each sudo request, and takes its answer from one; the design is in
//! `AGENTS-PLAN.md`, under "Sudo from agents".
//!
//! The socket is in the app's runtime directory, which the Flatpak sandbox
//! shares with the host, and the agent only listens to the terminal's own
//! program on it. Answers never go over the session bus, which any program
//! of the user's can watch.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The terminal's app id, naming its runtime directory
pub const APP_ID: &str = "dev.twright.CosmicTermAgents";

/// The control socket, for a user
pub fn control_path(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/user/{uid}/app/{APP_ID}/sudo-control.sock"))
}

/// How a request is approved, as the policy and the hardware decide now
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    /// The store password, as the store is locked or the session has not
    /// given it yet
    Password,
    Fingerprint,
    Click,
}

/// A sudo request waiting for the user
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Request {
    pub id: u64,
    /// The machine sudo runs on, as it names itself
    pub host: String,
    pub user: String,
    pub target: String,
    pub cwd: String,
    /// The command, without sudo and its options
    pub command: Vec<String>,
    /// The Zellij session that asked, if any
    pub session: Option<String>,
    pub approval: Approval,
    /// Why the fingerprint reader is not used, when the policy would
    pub note: Option<String>,
    /// Seconds left before the request gives up
    pub seconds_left: u64,
}

/// A saved secret, without the secret
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EntryInfo {
    pub id: u64,
    /// The machine it is for, or none for Other
    pub host: Option<String>,
    /// The account, or the name of an Other entry
    pub label: String,
    pub note: String,
    pub always_ask: bool,
}

/// What a saved secret is, for adding or editing one
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct EntryEdit {
    pub host: Option<String>,
    pub label: String,
    pub note: String,
    /// Left as it is when editing and empty
    pub secret: String,
    pub always_ask: bool,
}

/// What each use needs while the store is open
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRule {
    /// For sudo: approved without asking while the store is open
    Nothing,
    Click,
    /// The reader when it can be used, else a click
    Fingerprint,
    Password,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// One unlock serves every use
    Store,
    /// A session's first use needs the store password, as sudo's ticket is
    /// per terminal
    Session,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct UseRule {
    pub approval: ApprovalRule,
    pub scope: Scope,
}

/// When the store locks, and what each use needs
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Policy {
    /// Seconds unused after which the store locks
    pub idle_lock: u64,
    /// Lock when the screen locks or the computer sleeps
    pub lock_with_screen: bool,
    pub sudo: UseRule,
    pub paste: UseRule,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            idle_lock: 15 * 60,
            lock_with_screen: true,
            // Once the store is open, a touch or a click; the password
            // only unlocks it
            sudo: UseRule {
                approval: ApprovalRule::Fingerprint,
                scope: Scope::Store,
            },
            paste: UseRule {
                approval: ApprovalRule::Nothing,
                scope: Scope::Store,
            },
        }
    }
}

/// The store, as the windows show it
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct State {
    /// Whether there is a store yet
    pub exists: bool,
    pub unlocked: bool,
    /// Seconds until it locks itself, when unlocked
    pub locks_in: u64,
    /// The machines set up to approve sudo, by host name
    pub machines: Vec<String>,
    /// Empty while locked
    pub entries: Vec<EntryInfo>,
    pub policy: Option<Policy>,
    pub fingers: Vec<String>,
    /// Why the reader cannot be used now, if it cannot
    pub reader: Option<String>,
}

/// From a window to the agent
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToAgent {
    /// Approve a request, with the store password when it asks for it
    Approve { id: u64, password: Option<String> },
    Deny { id: u64 },
    /// The request's bar is in front: start waiting for a fingerprint
    Arm { id: u64 },
    /// Make the store, with its password
    Create { password: String },
    Unlock { password: String },
    Lock,
    /// Make a sudo key for a machine; the store must be open
    SetUpMachine { host: String },
    ForgetMachine { host: String },
    AddEntry(EntryEdit),
    EditEntry { id: u64, edit: EntryEdit },
    DeleteEntry { id: u64 },
    /// An entry's secret, for pasting; with the store password when the
    /// entry or the policy asks for it
    Reveal { id: u64, password: Option<String> },
    SetPolicy(Policy),
    Status,
    /// Enrol a finger with fprintd, such as "right-index-finger"
    Enrol { finger: String },
}

/// From the agent to the windows
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FromAgent {
    /// A request to show, sent to every window
    Request(Request),
    /// A request is over
    Done { id: u64, outcome: Outcome },
    /// The password was wrong, or another step failed; the request waits
    Retry { id: u64, error: String },
    FingerprintFailed { id: u64, error: String },
    /// A finger is on the reader, for a request
    FingerReading { id: u64 },
    /// A request the policy approved by itself, the store being open
    AutoApproved(Request),
    /// The store changed, sent to every window
    State(State),
    /// A machine's new sudo key, for its key file
    Generated(KeyInfo),
    /// An entry's secret, to the window that asked
    Secret { id: u64, secret: String },
    /// Progress of an enrolment: fprintd's status, and whether it is over
    Enrolment { status: String, done: bool },
    Error { message: String },
}

/// A machine's new sudo key
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct KeyInfo {
    pub host: String,
    /// Its public key line, for the machine's key file
    pub public: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Approved,
    Denied,
    Expired,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_one_json_line_each() {
        let messages = [
            serde_json::to_string(&ToAgent::Approve { id: 3, password: None }).unwrap(),
            serde_json::to_string(&FromAgent::Done { id: 3, outcome: Outcome::Approved }).unwrap(),
        ];
        for line in messages {
            assert!(!line.contains('\n'));
        }
        let back: ToAgent = serde_json::from_str(r#"{"deny":{"id":7}}"#).unwrap();
        assert_eq!(back, ToAgent::Deny { id: 7 });
        assert_eq!(control_path(1000).to_str().unwrap(), "/run/user/1000/app/dev.twright.CosmicTermAgents/sudo-control.sock");
    }
}
