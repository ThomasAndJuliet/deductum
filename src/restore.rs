// SPDX-License-Identifier: GPL-3.0-only

//! Tabs reopened after a restart.
//!
//! Each window keeps a record of its tabs in
//! `$XDG_STATE_HOME/cosmic-term/windows/<id>.json`, holding a lock on the
//! file for as long as it runs. A window started without arguments claims
//! a record no running window holds and reopens its tabs: threads continue
//! their conversations, sessions are attached again. A window closed with
//! no tabs to keep removes its record.

use std::fs::File;
use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A window's tabs, in order
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Record {
    pub tabs: Vec<Tab>,
    /// The tab that was in front
    #[serde(default)]
    pub active: usize,
}

/// A tab worth reopening
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Tab {
    Thread {
        /// `claude` or `codex`
        agent: String,
        host: String,
        cwd: String,
        #[serde(default)]
        profile: Option<u64>,
        /// The conversation, once it has one
        #[serde(default)]
        conversation: Option<String>,
        #[serde(default)]
        model: String,
        #[serde(default)]
        mode: String,
        #[serde(default)]
        title: String,
        /// The session of the relay running its agent, to join again
        #[serde(default)]
        relay: Option<String>,
    },
    /// A Zellij session, by its descriptor (`name@host`)
    Session { descriptor: String },
}

fn dir() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("cosmic-term/windows")
}

/// A window's hold on its record
pub struct Claim {
    file: File,
    path: PathBuf,
    saved: Option<Record>,
}

fn lock(file: &File) -> bool {
    // SAFETY: flock on a descriptor this file owns
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

impl Claim {
    /// The record of a window no longer running, if there is one, claimed
    pub fn previous() -> Option<(Self, Record)> {
        let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir())
            .ok()?
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "json"))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        // The window closed last first
        found.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, path) in found {
            let Ok(mut file) = File::options().read(true).write(true).open(&path) else { continue };
            if !lock(&file) {
                continue;
            }
            let mut text = String::new();
            let record = file.read_to_string(&mut text).ok().and_then(|_| serde_json::from_str::<Record>(&text).ok());
            match record {
                Some(record) if !record.tabs.is_empty() => {
                    return Some((Self { file, path, saved: Some(record.clone()) }, record));
                }
                _ => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        None
    }

    /// A record of its own for a new window
    pub fn new() -> Option<Self> {
        let dir = dir();
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("{}.json", cosmic_term::random_hex(8)));
        let file = File::options().read(true).write(true).create_new(true).open(&path).ok()?;
        lock(&file).then_some(Self { file, path, saved: None })
    }

    /// Keep `record`, if it changed
    pub fn save(&mut self, record: &Record) {
        if self.saved.as_ref() == Some(record) {
            return;
        }
        let Ok(text) = serde_json::to_string(record) else { return };
        // In place: the lock is on this file
        let written = self.file.set_len(0).is_ok()
            && self.file.rewind().is_ok()
            && self.file.write_all(text.as_bytes()).is_ok()
            && self.file.flush().is_ok();
        if written {
            self.saved = Some(record.clone());
        }
    }

    /// The window closes with nothing to reopen
    pub fn forget(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_round_trips_and_is_claimed_once() {
        let tmp = std::env::temp_dir().join(format!("ct-restore-{}", std::process::id()));
        // SAFETY: only this test reads the variable
        unsafe { std::env::set_var("XDG_STATE_HOME", &tmp) };
        let record = Record {
            tabs: vec![
                Tab::Thread {
                    agent: "claude".into(),
                    host: String::new(),
                    cwd: "/tmp".into(),
                    profile: Some(3),
                    conversation: Some("abc".into()),
                    model: String::new(),
                    mode: "auto".into(),
                    title: "A thread".into(),
                    relay: Some("ct-thread-claude-tmp-0123abcd".into()),
                },
                Tab::Session { descriptor: "ct-claude-x@neo".into() },
            ],
            active: 1,
        };
        let mut claim = Claim::new().unwrap();
        claim.save(&record);
        // Held by its window, it is not anyone else's
        assert!(Claim::previous().is_none());
        drop(claim);
        let (claimed, found) = Claim::previous().unwrap();
        assert_eq!(found, record);
        assert!(Claim::previous().is_none());
        claimed.forget();
        assert!(Claim::previous().is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

impl crate::App {
    /// This window's tabs, as a record to reopen them from
    fn tab_record(&self) -> Record {
        use crate::thread::tab::ThreadTab;
        let mut record = Record::default();
        for (_, tab_model) in self.pane_model.panes.iter() {
            for entity in tab_model.iter() {
                let tab = if let Some(thread) = tab_model.data::<ThreadTab>(entity) {
                    Tab::Thread {
                        agent: if thread.agent == crate::config::Agent::Codex { "codex" } else { "claude" }.into(),
                        host: thread.host.clone(),
                        cwd: thread.cwd.clone(),
                        profile: thread.profile_id.map(|id| id.0),
                        conversation: thread.session_id.clone(),
                        model: thread.model.clone(),
                        mode: thread.mode.clone(),
                        title: if thread.named { thread.title.clone() } else { String::new() },
                        relay: thread.relay.as_ref().filter(|_| thread.relay_live).map(|r| r.name.clone()),
                    }
                } else if let Some(session) = tab_model.data::<crate::agent::Session>(entity) {
                    Tab::Session { descriptor: session.descriptor() }
                } else {
                    continue;
                };
                if self.pane_model.active().is_some_and(|active| std::ptr::eq(active, tab_model)) && tab_model.is_active(entity) {
                    record.active = record.tabs.len();
                }
                record.tabs.push(tab);
            }
        }
        record
    }

    /// Keep the record of this window's tabs up to date
    pub(crate) fn save_tabs(&mut self) {
        let record = self.tab_record();
        if let Some(claim) = &mut self.restore {
            claim.save(&record);
        }
    }

    /// Reopen a closed window's tabs: threads now, sessions once their
    /// hosts say they still run
    pub(crate) fn reopen(&mut self, record: Record) -> cosmic::app::Task<crate::Message> {
        let pane = self.pane_model.focused();
        let mut tasks = Vec::new();
        let mut sessions = Vec::new();
        for tab in record.tabs {
            match tab {
                Tab::Thread { agent, host, cwd, profile, conversation, model, mode, title, relay } => {
                    let profile_id = profile
                        .map(crate::config::ProfileId)
                        .filter(|id| self.config.profiles.contains_key(id));
                    let id = self.next_thread;
                    let agent = if agent == "codex" { crate::config::Agent::Codex } else { crate::config::Agent::Claude };
                    let has_history = conversation.is_some();
                    tasks.push(self.open_thread_as(pane, profile_id, cwd, conversation, None, Some(agent), Some(host)));
                    if let Some(tab) = self.thread_mut(id) {
                        // Its relay may still run, with a turn in progress
                        if let Some(relay) = relay {
                            tab.relay = Some(crate::agent::Session { host: tab.host.clone(), name: relay });
                            tab.relay_live = true;
                            // After its history, or now if it has none yet
                            tab.attach_on_load = has_history;
                            if !has_history {
                                tasks.push(tab.attach());
                            }
                        }
                        if !model.is_empty() {
                            tab.model = model;
                        }
                        if !mode.is_empty() {
                            tab.mode = mode;
                        }
                        tab.named = !title.is_empty();
                    }
                    if !title.is_empty() {
                        tasks.push(self.set_thread_title(id, title));
                    }
                }
                Tab::Session { descriptor } => sessions.extend(crate::agent::Session::from_descriptor(&descriptor)),
            }
        }
        if !sessions.is_empty() {
            tasks.push(cosmic::task::future(async move {
                let mut alive = Vec::new();
                let hosts: std::collections::BTreeSet<String> = sessions.iter().map(|s| s.host.clone()).collect();
                for host in hosts {
                    if let Ok(listed) = crate::agent::list_sessions(host).await {
                        alive.extend(
                            sessions
                                .iter()
                                .filter(|s| listed.iter().any(|l| !l.exited && &l.session == *s))
                                .cloned(),
                        );
                    }
                }
                crate::Message::RestoreSessions(alive)
            }));
        }
        cosmic::app::Task::batch(tasks)
    }
}
