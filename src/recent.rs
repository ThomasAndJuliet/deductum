// SPDX-License-Identifier: GPL-3.0-only

//! What the start page remembers of each session: the document last opened in it and when. The
//! host does not report this, so the app keeps it beside its settings (`recent.json`), by machine
//! and session id. Forgetting a session on the start page discards it on its host; this record
//! of it goes with it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Condvar, LazyLock, Mutex, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    /// The document's file name
    pub file: String,
    /// When it was opened (seconds since the epoch)
    pub at: u64,
    /// Most recently opened paths first, with folder names retained.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    paths: Vec<String>,
}

fn path() -> PathBuf {
    super::settings::path().with_file_name("recent.json")
}

#[derive(Default)]
struct Store {
    entries: BTreeMap<String, Seen>,
    changed: u64,
    written: u64,
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| {
    let read = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok());
    Mutex::new(Store { entries: read.unwrap_or_default(), ..Store::default() })
});
static WRITTEN: Condvar = Condvar::new();

static WRITER: LazyLock<mpsc::SyncSender<()>> = LazyLock::new(|| {
    let (send, ready) = mpsc::sync_channel(1);
    std::thread::Builder::new().name("recent-files".into()).spawn(move || {
        while ready.recv().is_ok() {
            let (entries, revision) = {
                let store = STORE.lock().unwrap();
                if store.changed == store.written { continue; }
                (store.entries.clone(), store.changed)
            };
            write(&entries);
            STORE.lock().unwrap().written = revision;
            WRITTEN.notify_all();
        }
    }).expect("start recent files writer");
    send
});

/// Load the initial cache before creating the window.
pub fn initialize() {
    LazyLock::force(&STORE);
}

/// Give normal window shutdown a bounded chance to record the last file.
pub fn flush() {
    let store = STORE.lock().unwrap();
    drop(WRITTEN.wait_timeout_while(store, Duration::from_millis(250),
        |store| store.written != store.changed));
}

fn changed(mut store: std::sync::MutexGuard<'_, Store>) {
    store.changed += 1;
    drop(store);
    // One pending wakeup is enough: it will take the latest complete snapshot.
    let _ = WRITER.try_send(());
}

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
    let seen = store.entries.entry(key(machine, sid)).or_default();
    seen.file = file;
    seen.at = at;
    seen.paths.retain(|path| path != document_path);
    seen.paths.insert(0, document_path.to_string());
    seen.paths.truncate(100);
    changed(store);
}

pub fn paths(machine: &str, sid: &str) -> Vec<String> {
    STORE.lock().unwrap().entries.get(&key(machine, sid)).map(|seen| seen.paths.clone()).unwrap_or_default()
}

/// This session is gone from the start page
pub fn forget(machine: &str, sid: &str) {
    let mut store = STORE.lock().unwrap();
    if store.entries.remove(&key(machine, sid)).is_some() {
        changed(store);
    }
}

pub fn get(machine: &str, sid: &str) -> Option<Seen> {
    STORE.lock().unwrap().entries.get(&key(machine, sid)).cloned()
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
