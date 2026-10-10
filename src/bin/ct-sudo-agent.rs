// SPDX-License-Identifier: GPL-3.0-only

//! `ct-sudo-agent`: the terminal's secrets agent. It keeps the secrets store
//! (`cosmic_term::vault`): saved passwords, and one key per machine that
//! approves sudo there. sudo's PAM module asks it to sign a request; the
//! user approves in a terminal window. The design is in `AGENTS-PLAN.md`,
//! under "Secrets and sudo approvals".
//!
//! systemd starts it from user socket units with two sockets: the one PAM
//! modules ask on (`/run/user/<uid>/cosmic-term-sudo.sock`, which the
//! terminal forwards to the other machines) and the control socket that
//! windows connect to. Each request goes to every window, and whichever the
//! user answers in sends the answer.
//!
//! The policy (`sudo_control::Policy`): the store locks after its idle time
//! and when the screen locks or the computer sleeps. A sudo request needs
//! the store password when the store is locked or, per session, when that
//! session has not given it yet; otherwise a touch on the fingerprint
//! reader, when it can be used, or a click. Pasting a secret needs only the
//! user's choosing it, unless the entry asks for the password each time.
//!
//! Without systemd's sockets (`LISTEN_FDS`) it opens them itself, for trying
//! it out.

use cosmic_term::{
    sudo_control::{
        self as control, Approval, ApprovalRule, EntryEdit, EntryInfo, FromAgent, KeyInfo, Outcome,
        Policy, Request, Scope, State, ToAgent,
    },
    vault::{self, Contents, Entry, Key},
};
use ct_sudo_protocol::{self as protocol, Challenge, Reply, keys, ssh_key};

/// The agent is linked statically against an SDK without a static libm, and
/// tokio's scheduler asks for pow; this one is pure Rust
#[unsafe(no_mangle)]
pub extern "C" fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}
use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::OpenOptionsExt,
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// How long a fingerprint check waits for a finger
const FINGER_WAIT: Duration = Duration::from_secs(60);
/// The answer must reach the module before it gives up
const MARGIN: Duration = Duration::from_secs(5);

enum Answer {
    Approve(Option<String>),
    Deny,
    Finger,
}

struct Pending {
    request: Request,
    deadline: Instant,
    answers: mpsc::Sender<Answer>,
}

/// The open store: its contents and key, when it was last used, and the
/// sessions that have given the store password since it was opened
struct Open {
    contents: Contents,
    key: Key,
    used: Instant,
    tickets: HashMap<String, Instant>,
}

struct Agent {
    store_path: PathBuf,
    policy_path: PathBuf,
    user: String,
    store: Mutex<Option<Open>>,
    policy: Mutex<Policy>,
    pending: Mutex<BTreeMap<u64, Pending>>,
    windows: Mutex<Vec<(u64, UnixStream)>>,
    next: AtomicU64,
    /// One fingerprint check at a time, as the reader serves one
    finger_busy: AtomicBool,
    /// The request the reader is waiting for, and how to stop it waiting
    finger_for: Mutex<Option<(u64, Arc<AtomicBool>)>>,
}

fn now_idle(open: &Open, policy: &Policy) -> bool {
    open.used.elapsed() >= Duration::from_secs(policy.idle_lock)
}

impl Agent {
    fn send(stream: &mut UnixStream, message: &FromAgent) -> bool {
        let Ok(mut line) = serde_json::to_string(message) else {
            return false;
        };
        line.push('\n');
        stream.write_all(line.as_bytes()).is_ok()
    }

    fn broadcast(&self, message: &FromAgent) {
        self.windows
            .lock()
            .unwrap()
            .retain_mut(|(_, stream)| Self::send(stream, message));
    }

    fn reply_to(&self, window: u64, message: FromAgent) {
        let mut windows = self.windows.lock().unwrap();
        if let Some((_, stream)) = windows.iter_mut().find(|(w, _)| *w == window) {
            Self::send(stream, &message);
        }
    }

    fn policy(&self) -> Policy {
        *self.policy.lock().unwrap()
    }

    /// The machines with sudo keys, which a locked store does not show: kept
    /// beside the store, as they are not secret
    fn index_path(&self) -> PathBuf {
        self.store_path.with_extension("machines")
    }

    fn machines(&self) -> Vec<String> {
        std::fs::read_to_string(self.index_path())
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn state(&self) -> State {
        let policy = self.policy();
        let store = self.store.lock().unwrap();
        let (unlocked, locks_in, entries) = match store.as_ref() {
            Some(open) => (
                true,
                Duration::from_secs(policy.idle_lock)
                    .saturating_sub(open.used.elapsed())
                    .as_secs(),
                open.contents
                    .entries
                    .iter()
                    .map(|e| EntryInfo {
                        id: e.id,
                        host: e.host.clone(),
                        label: e.label.clone(),
                        note: e.note.clone(),
                        always_ask: e.always_ask,
                    })
                    .collect(),
            ),
            None => (false, 0, Vec::new()),
        };
        State {
            exists: self.store_path.exists(),
            unlocked,
            locks_in,
            machines: self.machines(),
            entries,
            policy: Some(policy),
            fingers: fingerprint::fingers(&self.user).unwrap_or_default(),
            reader: reader::unusable(),
        }
    }

    fn tell_state(&self) {
        let state = self.state();
        self.broadcast(&FromAgent::State(state));
    }

    /// Seal and write the open store
    fn save(&self, open: &Open) -> Result<(), String> {
        let sealed = vault::seal(&open.contents, &open.key)?;
        write_private(&self.store_path, &sealed)?;
        let machines: String = open.contents.sudo_keys.keys().map(|h| format!("{h}\n")).collect();
        write_private(&self.index_path(), machines.as_bytes())
    }

    /// Open the store with its password, or check it if open already
    fn unlock(&self, password: &str) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        if let Some(open) = store.as_mut() {
            if open.key.matches(password) {
                open.used = Instant::now();
                return Ok(());
            }
            return Err("the password is not right".to_string());
        }
        let file = std::fs::read(&self.store_path).map_err(|_| "there is no store yet".to_string())?;
        let (contents, key) = vault::open(&file, password)?;
        *store = Some(Open {
            contents,
            key,
            used: Instant::now(),
            tickets: HashMap::new(),
        });
        Ok(())
    }

    fn lock(&self) {
        // Dropping wipes the contents and the key
        *self.store.lock().unwrap() = None;
    }

    /// How a request from a session would be approved now
    fn approval(&self, session: &str) -> (Approval, Option<String>) {
        let policy = self.policy();
        let store = self.store.lock().unwrap();
        let Some(open) = store.as_ref() else {
            return (Approval::Password, None);
        };
        let ticket = policy.sudo.scope == Scope::Store || open.tickets.contains_key(session);
        if !ticket || policy.sudo.approval == ApprovalRule::Password {
            return (Approval::Password, None);
        }
        match policy.sudo.approval {
            ApprovalRule::Fingerprint => match reader::unusable() {
                None => (Approval::Fingerprint, None),
                Some(why) => (Approval::Click, Some(why)),
            },
            _ => (Approval::Click, None),
        }
    }

    /// Sign a challenge with a machine's key, with the password if given,
    /// giving the session its ticket
    fn sign(&self, host: &str, session: &str, password: Option<&str>, bytes: &[u8]) -> Result<String, String> {
        if let Some(password) = password {
            self.unlock(password)?;
        }
        let mut store = self.store.lock().unwrap();
        let open = store.as_mut().ok_or("the store is locked")?;
        let policy = self.policy();
        if password.is_none()
            && (policy.sudo.approval == ApprovalRule::Password
                || (policy.sudo.scope == Scope::Session && !open.tickets.contains_key(session)))
        {
            return Err("the store password is needed".to_string());
        }
        let private = open
            .contents
            .sudo_keys
            .get(host)
            .ok_or_else(|| format!("{host} is not set up"))?;
        let key = ssh_key::PrivateKey::from_openssh(private).map_err(|err| err.to_string())?;
        let signature = keys::sign(&key, bytes).map_err(|err| err.0)?;
        open.used = Instant::now();
        open.tickets.insert(session.to_string(), Instant::now());
        Ok(signature)
    }

    /// Ask the user about one request from a PAM module, and answer it
    fn serve_module(self: &Arc<Self>, mut conn: UnixStream) -> Result<(), String> {
        conn.set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|err| err.to_string())?;
        conn.set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|err| err.to_string())?;
        let mut version = [0];
        // SSH forwarding checks connect and close without sending a request.
        // A clean EOF before the first byte is not a truncated PAM message.
        if conn.read(&mut version).map_err(|err| err.to_string())? == 0 {
            return Ok(());
        }
        let bytes = match protocol::read_request(&mut version.as_slice().chain(&mut conn)) {
            Ok(bytes) => bytes,
            Err(err) => {
                let _ = protocol::write_request_refusal(&mut conn, version[0], err.0.clone());
                return Err(err.0);
            }
        };
        let refuse = |conn: &mut UnixStream, reason: String| {
            let _ = protocol::write_reply(conn, &Reply::Refused(reason.clone()));
            Err(reason)
        };
        let challenge = match Challenge::decode(&bytes) {
            Ok(challenge) => challenge,
            Err(err) => return refuse(&mut conn, err.0),
        };
        if challenge.module_version != protocol::MODULE_VERSION {
            return refuse(&mut conn, format!(
                "sudo module version mismatch: PAM uses {}, agent expects {}; update the PAM module",
                challenge.module_version, protocol::MODULE_VERSION
            ));
        }
        if !self.machines().contains(&challenge.host) {
            return refuse(&mut conn, format!("{} is not set up in this agent's store ({}); check that the sudo socket forwards to the terminal with this machine's key", challenge.host, self.store_path.display()));
        }
        let session = challenge.session.clone().unwrap_or_default();
        let command: Vec<String> = challenge.command().into_iter().map(str::to_string).collect();
        // The policy may approve by itself while the store is open; the
        // windows are told, after
        if self.policy().sudo.approval == ApprovalRule::Nothing
            && let Ok(signature) = self.sign(&challenge.host, &session, None, &bytes)
        {
            let _ = protocol::write_reply(&mut conn, &Reply::Signed(signature));
            self.broadcast(&FromAgent::AutoApproved(Request {
                id: self.next.fetch_add(1, Ordering::Relaxed),
                host: challenge.host.clone(),
                user: challenge.user.clone(),
                target: challenge.target.clone(),
                cwd: challenge.cwd.clone(),
                command,
                session: challenge.session.clone(),
                approval: Approval::Click,
                note: None,
                seconds_left: 0,
            }));
            self.tell_state();
            return Ok(());
        }
        if self.windows.lock().unwrap().is_empty() {
            return refuse(&mut conn, "no terminal window to ask".to_string());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let deadline = Instant::now() + protocol::TIMEOUT.saturating_sub(MARGIN);
        let (answers, answered) = mpsc::channel();
        let (approval, note) = self.approval(&session);
        let request = Request {
            id,
            host: challenge.host.clone(),
            user: challenge.user.clone(),
            target: challenge.target.clone(),
            cwd: challenge.cwd.clone(),
            command,
            session: challenge.session.clone(),
            approval,
            note,
            seconds_left: deadline.saturating_duration_since(Instant::now()).as_secs(),
        };
        self.pending.lock().unwrap().insert(
            id,
            Pending {
                request: request.clone(),
                deadline,
                answers,
            },
        );
        self.broadcast(&FromAgent::Request(request));

        let outcome = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let password = match answered.recv_timeout(left) {
                Err(_) => break Outcome::Expired,
                Ok(Answer::Deny) => break Outcome::Denied,
                // While the reader is offered, a click alone is not enough:
                // the touch, or the store password
                Ok(Answer::Approve(None)) if self.approval(&session).0 == Approval::Fingerprint => {
                    self.broadcast(&FromAgent::Retry {
                        id,
                        error: "touch the reader, or type the store password".to_string(),
                    });
                    continue;
                }
                Ok(Answer::Approve(password)) => password,
                // Only offered once the session holds a ticket
                Ok(Answer::Finger) => None,
            };
            match self.sign(&challenge.host, &session, password.as_deref(), &bytes) {
                Ok(signature) => {
                    let _ = protocol::write_reply(&mut conn, &Reply::Signed(signature));
                    break Outcome::Approved;
                }
                Err(error) => {
                    self.refresh(id);
                    self.broadcast(&FromAgent::Retry { id, error });
                }
            }
        };
        self.pending.lock().unwrap().remove(&id);
        // Let the reader go, if it waits for this one
        if let Some((waiting, stop)) = self.finger_for.lock().unwrap().as_ref()
            && *waiting == id
        {
            stop.store(true, Ordering::SeqCst);
        }
        match outcome {
            Outcome::Denied => {
                let _ = protocol::write_reply(&mut conn, &Reply::Refused("denied".into()));
            }
            Outcome::Expired => {
                let _ = protocol::write_reply(&mut conn, &Reply::Refused("no answer".into()));
            }
            Outcome::Approved => {}
        }
        self.broadcast(&FromAgent::Done { id, outcome });
        self.tell_state();
        Ok(())
    }

    /// Tell the windows how a waiting request is approved now
    fn refresh(&self, id: u64) {
        let request = {
            let pending = self.pending.lock().unwrap();
            let Some(p) = pending.get(&id) else {
                return;
            };
            let (approval, note) = self.approval(p.request.session.as_deref().unwrap_or_default());
            Request {
                approval,
                note,
                seconds_left: p.deadline.saturating_duration_since(Instant::now()).as_secs(),
                ..p.request.clone()
            }
        };
        self.broadcast(&FromAgent::Request(request));
    }

    fn refresh_all(&self) {
        let ids: Vec<u64> = self.pending.lock().unwrap().keys().copied().collect();
        for id in ids {
            self.refresh(id);
        }
    }

    fn answer(&self, id: u64, answer: Answer) {
        if let Some(pending) = self.pending.lock().unwrap().get(&id) {
            let _ = pending.answers.send(answer);
        }
    }

    /// Change the open store and save it
    fn change<T>(&self, change: impl FnOnce(&mut Contents) -> Result<T, String>) -> Result<T, String> {
        let mut store = self.store.lock().unwrap();
        let open = store.as_mut().ok_or("the store is locked")?;
        let result = change(&mut open.contents)?;
        open.used = Instant::now();
        self.save(open)?;
        Ok(result)
    }

    fn handle(self: &Arc<Self>, window: u64, message: ToAgent) -> Result<(), String> {
        match message {
            ToAgent::Approve { id, password } => self.answer(id, Answer::Approve(password)),
            ToAgent::Deny { id } => self.answer(id, Answer::Deny),
            ToAgent::Arm { id } => self.arm(id),
            ToAgent::Create { password } => {
                if self.store_path.exists() {
                    return Err("there is a store already".to_string());
                }
                if password.is_empty() {
                    return Err("the password is empty".to_string());
                }
                let open = Open {
                    contents: Contents::default(),
                    key: Key::new(&password, vault::Params::default())?,
                    used: Instant::now(),
                    tickets: HashMap::new(),
                };
                self.save(&open)?;
                *self.store.lock().unwrap() = Some(open);
            }
            ToAgent::Unlock { password } => {
                self.unlock(&password)?;
                self.refresh_all();
            }
            ToAgent::Lock => {
                self.lock();
                self.refresh_all();
            }
            ToAgent::SetUpMachine { host } => {
                if !valid_host(&host) {
                    return Err(format!("odd host name: {host}"));
                }
                let comment = format!("cosmic-term sudo key for {host}");
                let (private, public) = generate(&comment)?;
                self.change(|contents| {
                    contents.sudo_keys.insert(host.clone(), private);
                    Ok(())
                })?;
                self.reply_to(window, FromAgent::Generated(KeyInfo { host, public }));
            }
            ToAgent::ForgetMachine { host } => {
                self.change(|contents| {
                    contents.sudo_keys.remove(&host);
                    Ok(())
                })?;
            }
            ToAgent::AddEntry(edit) => {
                self.change(|contents| {
                    contents.add(entry_from(edit, None));
                    Ok(())
                })?;
            }
            ToAgent::EditEntry { id, edit } => {
                self.change(|contents| {
                    let entry = contents
                        .entries
                        .iter_mut()
                        .find(|e| e.id == id)
                        .ok_or("no such entry")?;
                    *entry = entry_from(edit, Some(entry));
                    entry.id = id;
                    Ok(())
                })?;
            }
            ToAgent::DeleteEntry { id } => {
                self.change(|contents| {
                    contents.entries.retain(|e| e.id != id);
                    Ok(())
                })?;
            }
            ToAgent::Reveal { id, password } => {
                if let Some(password) = &password {
                    self.unlock(password)?;
                }
                let policy = self.policy();
                let mut store = self.store.lock().unwrap();
                let open = store.as_mut().ok_or("the store is locked")?;
                let entry = open.contents.entry(id).ok_or("no such entry")?;
                let asks = entry.always_ask || policy.paste.approval == ApprovalRule::Password;
                if asks && password.is_none() {
                    return Err("this one needs the store password".to_string());
                }
                let secret = entry.secret.clone();
                open.used = Instant::now();
                drop(store);
                self.reply_to(window, FromAgent::Secret { id, secret });
                return Ok(());
            }
            ToAgent::SetPolicy(policy) => {
                // Not while locked, so that nothing can weaken it unseen
                if self.store.lock().unwrap().is_none() {
                    return Err("unlock the store to change the policy".to_string());
                }
                *self.policy.lock().unwrap() = policy;
                let json = serde_json::to_vec_pretty(&policy).map_err(|err| err.to_string())?;
                write_private(&self.policy_path, &json)?;
                self.refresh_all();
            }
            ToAgent::Status => {}
            ToAgent::Enrol { finger } => {
                let agent = Arc::clone(self);
                thread::spawn(move || {
                    let tell = |status: String, done: bool| {
                        agent.reply_to(window, FromAgent::Enrolment { status, done });
                    };
                    if let Err(err) = fingerprint::enrol(&finger, &tell) {
                        tell(err, true);
                    }
                    agent.tell_state();
                });
                return Ok(());
            }
        }
        self.tell_state();
        Ok(())
    }

    /// Talk to one terminal window until it goes
    fn serve_window(self: &Arc<Self>, conn: UnixStream) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let Ok(mut writer) = conn.try_clone() else {
            return;
        };
        if !Self::send(&mut writer, &FromAgent::State(self.state())) {
            return;
        }
        let waiting: Vec<u64> = self.pending.lock().unwrap().keys().copied().collect();
        self.windows.lock().unwrap().push((id, writer));
        for request in waiting {
            self.refresh(request);
        }
        for line in BufReader::new(conn).lines() {
            let Ok(line) = line else {
                break;
            };
            let result = serde_json::from_str::<ToAgent>(&line)
                .map_err(|err| err.to_string())
                .and_then(|message| self.handle(id, message));
            if let Err(message) = result {
                self.reply_to(id, FromAgent::Error { message });
            }
        }
        self.windows.lock().unwrap().retain(|(w, _)| *w != id);
    }

    /// The request's bar is in front: wait for a finger, if that is how it
    /// is approved now
    fn arm(self: &Arc<Self>, id: u64) {
        let Some(session) = self
            .pending
            .lock()
            .unwrap()
            .get(&id)
            .map(|p| p.request.session.clone().unwrap_or_default())
        else {
            return;
        };
        if self.approval(&session).0 != Approval::Fingerprint {
            return;
        }
        if self.finger_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *self.finger_for.lock().unwrap() = Some((id, Arc::clone(&stop)));
        let agent = Arc::clone(self);
        thread::spawn(move || {
            // Answer as soon as it matches; the reader is let go after
            let result = fingerprint::verify(
                FINGER_WAIT,
                &stop,
                &|| agent.broadcast(&FromAgent::FingerReading { id }),
                &|| agent.answer(id, Answer::Finger),
            );
            if let Err(error) = result
                && !stop.load(Ordering::SeqCst)
            {
                agent.broadcast(&FromAgent::FingerprintFailed { id, error });
            }
            agent.finger_busy.store(false, Ordering::SeqCst);
        });
    }

    /// Lock when unused for the policy's time; tell the windows the time left
    fn watch_idle(self: Arc<Self>) {
        loop {
            thread::sleep(Duration::from_secs(5));
            let policy = self.policy();
            let idle = self.store.lock().unwrap().as_ref().is_some_and(|open| now_idle(open, &policy));
            if idle {
                self.lock();
                self.refresh_all();
                self.tell_state();
            }
        }
    }
}

fn entry_from(edit: EntryEdit, old: Option<&Entry>) -> Entry {
    let secret = if edit.secret.is_empty() {
        old.map(|e| e.secret.clone()).unwrap_or_default()
    } else {
        edit.secret
    };
    Entry {
        id: 0,
        host: edit.host.filter(|h| !h.is_empty()),
        label: edit.label,
        note: edit.note,
        secret,
        always_ask: edit.always_ask,
    }
}

/// A new Ed25519 key: its OpenSSH private key, unencrypted as the store
/// seals it, and its public key line
fn generate(comment: &str) -> Result<(String, String), String> {
    use ssh_key::{Algorithm, LineEnding, PrivateKey, rand_core::OsRng};
    let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).map_err(|err| err.to_string())?;
    key.set_comment(comment);
    let public = key.public_key().to_openssh().map_err(|err| err.to_string())?;
    let private = key.to_openssh(LineEnding::LF).map_err(|err| err.to_string())?;
    Ok((private.to_string(), public))
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// Write a file only its owner can read, replacing it whole
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    let tmp = path.with_extension("new");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|err| err.to_string())?;
    file.write_all(bytes).map_err(|err| err.to_string())?;
    file.sync_all().map_err(|err| err.to_string())?;
    std::fs::rename(&tmp, path).map_err(|err| err.to_string())
}

/// Whether the fingerprint reader can be used now
mod reader {
    /// Vendors of fingerprint readers, to find them among USB devices
    const VENDORS: [&str; 7] = ["06cb", "27c6", "04f3", "138a", "10a5", "1c7a", "2541"];

    fn lid_closed() -> bool {
        std::fs::read_dir("/proc/acpi/button/lid")
            .into_iter()
            .flatten()
            .flatten()
            .any(|lid| {
                std::fs::read_to_string(lid.path().join("state"))
                    .is_ok_and(|s| s.contains("closed"))
            })
    }

    fn masked() -> bool {
        ["/etc/systemd/system/fprintd.service", "/run/systemd/system/fprintd.service"]
            .iter()
            .any(|p| std::fs::read_link(p).is_ok_and(|t| t == std::path::Path::new("/dev/null")))
    }

    /// Fingerprint readers on USB, and whether each can be unplugged
    fn readers() -> Vec<bool> {
        let mut found = Vec::new();
        for dev in std::fs::read_dir("/sys/bus/usb/devices").into_iter().flatten().flatten() {
            let path = dev.path();
            let vendor = std::fs::read_to_string(path.join("idVendor")).unwrap_or_default();
            if VENDORS.contains(&vendor.trim()) {
                let removable = std::fs::read_to_string(path.join("removable")).unwrap_or_default();
                found.push(removable.trim() == "removable");
            }
        }
        found
    }

    /// Why the reader cannot be used now, or None if it can
    pub fn unusable() -> Option<String> {
        if masked() {
            return Some("fingerprint service off".to_string());
        }
        let readers = readers();
        if readers.is_empty() {
            return Some("no fingerprint reader".to_string());
        }
        if lid_closed() && !readers.iter().any(|removable| *removable) {
            return Some("lid closed".to_string());
        }
        None
    }
}

/// fprintd, on the system bus. Only fprintd's own answers count: they come
/// from its name on the bus, which no other program can take
mod fingerprint {
    use futures_util::StreamExt;
    use std::time::Duration;
    use zbus::zvariant::OwnedObjectPath;

    const SERVICE: &str = "net.reactivated.Fprint";

    fn runtime() -> Result<tokio::runtime::Runtime, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| err.to_string())
    }

    async fn device(conn: &zbus::Connection) -> Result<zbus::Proxy<'static>, String> {
        let manager = zbus::Proxy::new(conn, SERVICE, "/net/reactivated/Fprint/Manager", "net.reactivated.Fprint.Manager")
            .await
            .map_err(|err| err.to_string())?;
        let path: OwnedObjectPath = manager
            .call("GetDefaultDevice", &())
            .await
            .map_err(|_| "no fingerprint reader".to_string())?;
        zbus::Proxy::new(conn, SERVICE, path, "net.reactivated.Fprint.Device")
            .await
            .map_err(|err| err.to_string())
    }

    /// The user's enrolled fingers, without starting fprintd when it is off
    pub fn fingers(user: &str) -> Result<Vec<String>, String> {
        if super::reader::unusable().as_deref() == Some("fingerprint service off") {
            return Ok(Vec::new());
        }
        runtime()?.block_on(async {
            let conn = zbus::Connection::system().await.map_err(|err| err.to_string())?;
            let device = device(&conn).await?;
            device
                .call("ListEnrolledFingers", &(user,))
                .await
                .or_else(|_| Ok::<_, String>(Vec::new()))
        })
    }

    /// Wait for a finger that matches one enrolled, telling when one is on
    /// the reader and, before letting the reader go, when it matches
    /// Stops waiting, and lets the reader go, once `stop` is set
    pub fn verify(
        wait: Duration,
        stop: &std::sync::atomic::AtomicBool,
        reading: &dyn Fn(),
        matched: &dyn Fn(),
    ) -> Result<(), String> {
        runtime()?.block_on(async {
            let conn = zbus::Connection::system().await.map_err(|err| err.to_string())?;
            let device = device(&conn).await?;
            let _: () = device.call("Claim", &("",)).await.map_err(|err| err.to_string())?;
            let mut status = device
                .receive_signal("VerifyStatus")
                .await
                .map_err(|err| err.to_string())?;
            // fprintd since 1.92 says when a finger is on the reader
            let mut present = device.receive_property_changed::<bool>("finger-present").await;
            let result = async {
                let _: () = device.call("VerifyStart", &("any",)).await.map_err(|err| err.to_string())?;
                let outcome = tokio::time::timeout(wait, async {
                    loop {
                        tokio::select! {
                            message = status.next() => {
                                let Some(message) = message else {
                                    return Err("fprintd went away".to_string());
                                };
                                let Ok((result, done)) = message.body().deserialize::<(String, bool)>() else {
                                    continue;
                                };
                                if result == "verify-match" {
                                    matched();
                                    return Ok(());
                                }
                                if done {
                                    return Err(result.trim_start_matches("verify-").replace('-', " "));
                                }
                            }
                            Some(change) = present.next() => {
                                if change.get().await.unwrap_or(false) {
                                    reading();
                                }
                            }
                            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                                if stop.load(std::sync::atomic::Ordering::SeqCst) {
                                    return Err("answered otherwise".to_string());
                                }
                            }
                        }
                    }
                })
                .await
                .unwrap_or_else(|_| Err("no finger in time".to_string()));
                let _: Result<(), _> = device.call("VerifyStop", &()).await;
                outcome
            }
            .await;
            let _: Result<(), _> = device.call("Release", &()).await;
            result
        })
    }

    /// Enrol a finger, telling each step
    pub fn enrol(finger: &str, tell: &dyn Fn(String, bool)) -> Result<(), String> {
        runtime()?.block_on(async {
            let conn = zbus::Connection::system().await.map_err(|err| err.to_string())?;
            let device = device(&conn).await?;
            let _: () = device.call("Claim", &("",)).await.map_err(|err| err.to_string())?;
            let mut status = device
                .receive_signal("EnrollStatus")
                .await
                .map_err(|err| err.to_string())?;
            let result = async {
                let _: () = device
                    .call("EnrollStart", &(finger,))
                    .await
                    .map_err(|err| err.to_string())?;
                let outcome = tokio::time::timeout(Duration::from_secs(120), async {
                    while let Some(message) = status.next().await {
                        let Ok((result, done)) = message.body().deserialize::<(String, bool)>() else {
                            continue;
                        };
                        tell(result.clone(), done);
                        if done {
                            return if result == "enroll-completed" { Ok(()) } else { Err(result) };
                        }
                    }
                    Err("fprintd went away".to_string())
                })
                .await
                .unwrap_or_else(|_| Err("timed out".to_string()));
                let _: Result<(), _> = device.call("EnrollStop", &()).await;
                outcome
            }
            .await;
            let _: Result<(), _> = device.call("Release", &()).await;
            result
        })
    }
}

/// Lock the store when the screen locks or the computer is about to sleep,
/// as logind announces
fn watch_logind(agent: Arc<Agent>) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
        return;
    };
    runtime.block_on(async move {
        use futures_util::StreamExt;
        let Ok(conn) = zbus::Connection::system().await else {
            return;
        };
        let rules = [
            "type='signal',interface='org.freedesktop.login1.Session',member='Lock'",
            "type='signal',interface='org.freedesktop.login1.Manager',member='PrepareForSleep'",
        ];
        let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
            return;
        };
        for rule in rules {
            if let Ok(rule) = zbus::MatchRule::try_from(rule) {
                let _ = dbus.add_match_rule(rule).await;
            }
        }
        let mut messages = zbus::MessageStream::from(&conn);
        while let Some(Ok(message)) = messages.next().await {
            let header = message.header();
            let member = header.member().map(|m| m.as_str().to_string()).unwrap_or_default();
            let going = match member.as_str() {
                "Lock" => true,
                "PrepareForSleep" => message.body().deserialize::<bool>().unwrap_or(false),
                _ => false,
            };
            if going && agent.policy().lock_with_screen && agent.store.lock().unwrap().is_some() {
                agent.lock();
                agent.refresh_all();
                agent.tell_state();
            }
        }
    });
}

/// Whether a window is the terminal's own program, as far as can be told:
/// the user's, running a `cosmic-term`
fn trusted_window(conn: &UnixStream, uid: u32) -> bool {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: cred and len are sized for SO_PEERCRED
    let rc = unsafe {
        libc::getsockopt(
            conn.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc != 0 || cred.uid != uid {
        return false;
    }
    std::fs::read_link(format!("/proc/{}/exe", cred.pid))
        .ok()
        .and_then(|exe| exe.file_name().map(|n| n == "cosmic-term"))
        .unwrap_or(false)
}

/// The sockets systemd passed, by the names in the socket units
fn systemd_sockets() -> HashMap<String, UnixListener> {
    let mut found = HashMap::new();
    let pid_ok = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
        .is_some_and(|p| p == std::process::id());
    let count: i32 = std::env::var("LISTEN_FDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    if !pid_ok || count <= 0 {
        return found;
    }
    let names = std::env::var("LISTEN_FDNAMES").unwrap_or_default();
    let names: Vec<&str> = names.split(':').collect();
    for i in 0..count {
        let name = names.get(i as usize).copied().unwrap_or("").to_string();
        // SAFETY: systemd passed these descriptors from 3 on, for us
        let listener = unsafe { UnixListener::from_raw_fd(3 + i) };
        found.insert(name, listener);
    }
    found
}

/// Listen at a path ourselves, replacing a dead socket left there
fn listen(path: &Path) -> std::io::Result<UnixListener> {
    cosmic_term::sudo_socket::listen(path)
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("ct-sudo-agent {} (sudo protocol {}, PAM module {})", env!("CARGO_PKG_VERSION"), protocol::VERSION, protocol::MODULE_VERSION);
        return;
    }
    // SAFETY: a plain call
    let uid = unsafe { libc::getuid() };
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/state"))
        .join("cosmic-term");
    let policy_path = state.join("vault-policy.json");
    let policy = std::fs::read(&policy_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let agent = Arc::new(Agent {
        store_path: state.join("vault"),
        policy_path,
        user: std::env::var("USER").unwrap_or_default(),
        store: Mutex::new(None),
        policy: Mutex::new(policy),
        pending: Mutex::new(BTreeMap::new()),
        windows: Mutex::new(Vec::new()),
        next: AtomicU64::new(1),
        finger_busy: AtomicBool::new(false),
        finger_for: Mutex::new(None),
    });

    let mut sockets = systemd_sockets();
    let open = |name: &str, path: PathBuf, sockets: &mut HashMap<String, UnixListener>| {
        sockets.remove(name).map(Ok).unwrap_or_else(|| listen(&path)).unwrap_or_else(|err| {
            eprintln!("ct-sudo-agent: cannot listen at {}: {err}", path.display());
            std::process::exit(1);
        })
    };
    let pam = open("pam", protocol::socket_path(uid), &mut sockets);
    let windows = open("control", control::control_path(uid), &mut sockets);

    thread::spawn({
        let agent = Arc::clone(&agent);
        move || agent.watch_idle()
    });
    thread::spawn({
        let agent = Arc::clone(&agent);
        move || watch_logind(agent)
    });
    let for_windows = Arc::clone(&agent);
    thread::spawn(move || {
        for conn in windows.incoming().flatten() {
            if !trusted_window(&conn, uid) {
                continue;
            }
            let agent = Arc::clone(&for_windows);
            thread::spawn(move || agent.serve_window(conn));
        }
    });
    for conn in pam.incoming().flatten() {
        let agent = Arc::clone(&agent);
        thread::spawn(move || {
            if let Err(err) = agent.serve_module(conn) {
                eprintln!("ct-sudo-agent: {err}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> Arc<Agent> {
        Arc::new(Agent {
            store_path: std::env::temp_dir().join(cosmic_term::random_hex(16)).join("vault"),
            policy_path: PathBuf::new(),
            user: "test".into(),
            store: Mutex::new(None),
            policy: Mutex::new(Policy::default()),
            pending: Mutex::new(BTreeMap::new()),
            windows: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
            finger_busy: AtomicBool::new(false),
            finger_for: Mutex::new(None),
        })
    }

    #[test]
    fn connect_and_close_probe_is_silent() {
        let (server, client) = UnixStream::pair().unwrap();
        drop(client);
        assert!(agent().serve_module(server).is_ok());
    }

    #[test]
    fn legacy_pam_gets_the_version_mismatch_in_its_own_framing() {
        let (server, mut client) = UnixStream::pair().unwrap();
        let serving = thread::spawn(move || agent().serve_module(server));
        client.write_all(&[1, 0, 0, 0, 3, b'a', b'b', b'c']).unwrap();
        let mut head = [0; 5];
        client.read_exact(&mut head).unwrap();
        assert_eq!(head[0], 0);
        let mut body = vec![0; u32::from_be_bytes(head[1..].try_into().unwrap()) as usize];
        client.read_exact(&mut body).unwrap();
        let reason = String::from_utf8(body).unwrap();
        assert!(reason.contains("PAM module uses 1, local uses 2"));
        assert_eq!(serving.join().unwrap().unwrap_err(), reason);
    }

    #[test]
    fn module_version_and_missing_machine_refusals_explain_what_to_update() {
        for module_version in [1, protocol::MODULE_VERSION] {
            let challenge = Challenge {
                nonce: [7; 32], host: "neo".into(), user: "twright".into(),
                target: "root".into(), cwd: "/tmp".into(),
                argv: vec!["sudo".into(), "true".into()],
                session: Some("ct-claude-53b3".into()), cgroup: String::new(),
                issued_at: 0, module_version,
            };
            let (server, mut client) = UnixStream::pair().unwrap();
            let serving = thread::spawn(move || agent().serve_module(server));
            protocol::write_request(&mut client, &challenge.encode()).unwrap();
            let Reply::Refused(reason) = protocol::read_reply(&mut client).unwrap() else {
                panic!("an incompatible or unconfigured module must not get a signature");
            };
            if module_version == 1 {
                assert!(reason.contains("PAM uses 1, agent expects 2"));
            } else {
                assert!(reason.contains("neo is not set up in this agent's store"));
                assert!(reason.contains("check that the sudo socket forwards"));
            }
            assert_eq!(serving.join().unwrap().unwrap_err(), reason);
        }
    }
}
