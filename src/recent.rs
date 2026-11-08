// SPDX-License-Identifier: GPL-3.0-only

//! What the start page remembers of each session: the document last opened in it and when. The
//! host does not report this, so the app keeps it beside its settings (`recent.json`), by machine
//! and session id. Forgetting a session on the start page discards it on its host; this record
//! of it goes with it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    /// The document's file name
    pub file: String,
    /// When it was opened (seconds since the epoch)
    pub at: u64,
}

fn path() -> PathBuf {
    super::settings::path().with_file_name("recent.json")
}

static STORE: LazyLock<Mutex<BTreeMap<String, Seen>>> = LazyLock::new(|| {
    let read = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok());
    Mutex::new(read.unwrap_or_default())
});

fn key(machine: &str, sid: &str) -> String {
    format!("{machine}/{sid}")
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A document was opened in this session
pub fn note(machine: &str, sid: &str, document_path: &str) {
    set(machine, sid, document_path, now());
}

pub(crate) fn set(machine: &str, sid: &str, document_path: &str, at: u64) {
    let file = document_path.rsplit(['/', '\\']).next().unwrap_or(document_path).to_string();
    let mut store = STORE.lock().unwrap();
    store.insert(key(machine, sid), Seen { file, at });
    write(&store);
}

/// This session is gone from the start page
pub fn forget(machine: &str, sid: &str) {
    let mut store = STORE.lock().unwrap();
    if store.remove(&key(machine, sid)).is_some() {
        write(&store);
    }
}

pub fn get(machine: &str, sid: &str) -> Option<Seen> {
    STORE.lock().unwrap().get(&key(machine, sid)).cloned()
}

fn write(store: &BTreeMap<String, Seen>) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(store) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// How long ago, as the start page says it
pub fn ago(at: u64, now: u64) -> String {
    let s = now.saturating_sub(at);
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match s {
        0..=59 => "just now".into(),
        60..=3599 => plural(s / 60, "minute"),
        3600..=86_399 => plural(s / 3600, "hour"),
        86_400..=172_799 => "yesterday".into(),
        _ => plural(s / 86_400, "day"),
    }
}

#[cfg(test)]
mod tests {
    use super::ago;

    #[test]
    fn times_are_said_as_the_start_page_does() {
        assert_eq!(ago(1000, 1010), "just now");
        assert_eq!(ago(0, 7300), "2 hours ago");
        assert_eq!(ago(0, 3700), "1 hour ago");
        assert_eq!(ago(0, 90_000), "yesterday");
        assert_eq!(ago(0, 86_400 * 3 + 5), "3 days ago");
    }
}
