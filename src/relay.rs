// SPDX-License-Identifier: GPL-3.0-only

//! A thread's agent CLI, kept running apart from the window showing it.
//!
//! `ct-supervise --relay DIR -- PROGRAM ARGS…` starts the CLI with pipes, in
//! a session of its own, and keeps it: everything it writes is appended to
//! `DIR/out.log`, and windows come and go through `DIR/sock`, each reading
//! the log from where it left off. `ct-supervise --connect DIR --from N`
//! bridges a window's pipes (over ssh, for a remote thread) to that socket,
//! or, once the relay is gone, replays the rest of its log.
//!
//! The log is the CLI's stdout, line by line, with the relay's own records
//! among it, each a line beginning `#ct ` (never valid JSON):
//! `#ct< ID KIND JSON` for each input accepted, and
//! `#ct exited CODE STDERR` at the end, with the base64 end of its stderr.
//! Byte offsets into the log are the windows' cursors.
//!
//! On the socket, lines go each way. The client opens with
//! `attach FROM|live [NAME]` and is sent `#ct hello 1 pid=P head=H turn=T
//! end=E busy=0|1`, then the log from FROM, then what is appended to it.
//! `live` is the turn in progress, else the end. Then the client may send
//! `turn ID JSON` (a user's turn: the agent is busy until a line with the
//! done pattern), `line ID JSON` (anything else for the CLI), `meta JSON`
//! (merged into `DIR/meta.json`) and `stop`. Inputs whose ID was seen lately
//! are dropped, so a client may resend what it is unsure arrived.
//!
//! `DIR/lock` is held with flock for the relay's life, so a free lock with a
//! `DIR/state` is a relay that has ended. `DIR/state` is its JSON summary,
//! replaced as it changes: `pid`, `busy`, `clients`, `head` (where the log
//! stood before the first turn), `turn_start` (where the latest began),
//! `end`, `exited` and `code`.

use std::collections::{HashSet, VecDeque};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{FileExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub const LOG: &str = "out.log";
pub const STATE: &str = "state";
pub const META: &str = "meta.json";
pub const LOCK: &str = "lock";
/// The supervisor to connect with, written by whoever starts the relay
pub const RELAY_BIN: &str = "relay-bin";
/// Where a thread's relay names the conversations it has, beside the threads
pub const BY_CONVERSATION: &str = "by-conversation";
/// The exit code of a relay whose conversation another relay has
pub const CLAIMED: i32 = 75;
/// The exit code of a relay that found another in its directory
pub const RUNNING: i32 = 3;
/// Input ids remembered, to drop repeats
const REMEMBERED: usize = 1000;
const STDERR_TAIL: usize = 4096;

/// How a relay runs
#[derive(Clone, Debug)]
pub struct Options {
    pub dir: PathBuf,
    /// Text in the CLI's line that ends a turn
    pub done: Option<String>,
    /// Stop once idle this long with no client
    pub linger: Option<Duration>,
    /// Stop once busy this long with no client
    pub max_blocked: Duration,
    /// The conversation the CLI resumes, claimed for this relay alone
    pub conversation: Option<String>,
    /// Between closing the CLI's stdin, SIGTERM and SIGKILL
    pub grace: Duration,
    pub program: Vec<OsString>,
}

impl Options {
    pub fn new(dir: impl Into<PathBuf>, program: Vec<OsString>) -> Self {
        Self {
            dir: dir.into(),
            done: None,
            linger: None,
            max_blocked: Duration::from_secs(86400),
            conversation: None,
            grace: Duration::from_secs(5),
            program,
        }
    }
}

/// Where a client starts reading the log
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cursor {
    Offset(u64),
    /// The turn in progress, else the end
    Live,
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Offset(n) => write!(f, "{n}"),
            Self::Live => f.write_str("live"),
        }
    }
}

impl std::str::FromStr for Cursor {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        if s == "live" {
            return Ok(Self::Live);
        }
        s.parse()
            .map(Self::Offset)
            .map_err(|_| format!("bad offset: {s}"))
    }
}

/// What `#ct hello` says
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Hello {
    pub pid: u32,
    pub head: u64,
    pub turn_start: u64,
    pub end: u64,
    pub busy: bool,
}

impl Hello {
    pub fn line(&self) -> String {
        format!(
            "#ct hello 1 pid={} head={} turn={} end={} busy={}\n",
            self.pid,
            self.head,
            self.turn_start,
            self.end,
            u8::from(self.busy)
        )
    }

    pub fn parse(line: &str) -> Option<Self> {
        let rest = line.trim_end().strip_prefix("#ct hello ")?;
        let mut words = rest.split(' ');
        if words.next()? != "1" {
            return None;
        }
        let mut hello = Self::default();
        for word in words {
            let (key, value) = word.split_once('=')?;
            match key {
                "pid" => hello.pid = value.parse().ok()?,
                "head" => hello.head = value.parse().ok()?,
                "turn" => hello.turn_start = value.parse().ok()?,
                "end" => hello.end = value.parse().ok()?,
                "busy" => hello.busy = value == "1",
                _ => {}
            }
        }
        Some(hello)
    }
}

/// A relay's record in the log
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Record {
    Hello(Hello),
    Input {
        id: String,
        kind: String,
        json: String,
    },
    Exited {
        code: i32,
        stderr: String,
    },
    /// The relay ended without saying how the CLI did
    Lost,
}

impl Record {
    /// The record in a log line, or `None` for the CLI's own lines
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.strip_suffix('\n').unwrap_or(line);
        if let Some(rest) = line.strip_prefix("#ct< ") {
            let mut parts = rest.splitn(3, ' ');
            let (id, kind, json) = (
                parts.next()?,
                parts.next()?,
                parts.next().unwrap_or_default(),
            );
            return Some(Self::Input {
                id: id.into(),
                kind: kind.into(),
                json: json.into(),
            });
        }
        if let Some(rest) = line.strip_prefix("#ct exited ") {
            let (code, tail) = rest.split_once(' ').unwrap_or((rest, ""));
            let stderr = crate::b64::decode(tail).unwrap_or_default();
            return Some(Self::Exited {
                code: code.parse().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
            });
        }
        if line == "#ct lost" {
            return Some(Self::Lost);
        }
        if line.starts_with("#ct hello ") {
            return Hello::parse(line).map(Self::Hello);
        }
        None
    }
}

fn exit_line(code: i32, stderr: &[u8]) -> String {
    format!("#ct exited {code} {}\n", crate::b64::encode(stderr))
}

/// The socket of the relay in `dir`. Socket paths are short, so a long
/// `dir` gets one under /tmp named by its hash
pub fn socket_path(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join("sock");
    if path.as_os_str().len() <= 100 {
        return Ok(path);
    }
    // SAFETY: a plain syscall
    let uid = unsafe { libc::getuid() };
    let base = PathBuf::from(format!("/tmp/ct-relay-{uid}"));
    match fs::create_dir(&base) {
        Ok(()) => fs::set_permissions(&base, fs::Permissions::from_mode(0o700))?,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    let meta = fs::symlink_metadata(&base)?;
    if !meta.is_dir()
        || std::os::unix::fs::MetadataExt::uid(&meta) != uid
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(format!(
            "{} is not this user's own",
            base.display()
        )));
    }
    let hash = dir
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
    Ok(base.join(format!("{hash:016x}")))
}

fn flock(file: &File, operation: libc::c_int) -> bool {
    // SAFETY: a plain syscall on an open file
    unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) == 0 }
}

/// Take `file`'s lock for good. A moment's tries, as [`is_live`] takes it
/// shared for a moment to look
fn lock_exclusive(file: &File) -> bool {
    for _ in 0..50 {
        if flock(file, libc::LOCK_EX) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Whether a relay holds `dir`'s lock
pub fn is_live(dir: &Path) -> bool {
    let Ok(file) = File::open(dir.join(LOCK)) else {
        return false;
    };
    if flock(&file, libc::LOCK_SH) {
        flock(&file, libc::LOCK_UN);
        false
    } else {
        true
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Claim conversation `id` for the relay in `dir`, unless a live relay
/// beside it has it: `threads/by-conversation/ID` links to the relay's
/// directory. Gives the holder's directory name when taken
pub fn claim_conversation(dir: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("bad conversation id: {id:?}"));
    }
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
        return Err("no thread directory".into());
    };
    let claims = parent.join(BY_CONVERSATION);
    fs::create_dir_all(&claims).map_err(|err| err.to_string())?;
    let guard = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(claims.join(".lock"))
        .map_err(|err| err.to_string())?;
    // SAFETY: a plain syscall; released when `guard` closes
    unsafe { libc::flock(guard.as_raw_fd(), libc::LOCK_EX) };
    let link = claims.join(id);
    if let Ok(target) = fs::read_link(&link) {
        let holder = claims.join(&target);
        if target.file_name() != Some(name) && is_live(&holder) {
            return Err(target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned());
        }
    }
    let tmp = claims.join(format!(".{id}.{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    std::os::unix::fs::symlink(Path::new("..").join(name), &tmp).map_err(|err| err.to_string())?;
    fs::rename(&tmp, &link).map_err(|err| err.to_string())
}

/// The thread directory holding conversation `id`, if a live relay has it
pub fn conversation_holder(threads: &Path, id: &str) -> Option<PathBuf> {
    if !valid_id(id) {
        return None;
    }
    let claims = threads.join(BY_CONVERSATION);
    let target = claims.join(fs::read_link(claims.join(id)).ok()?);
    is_live(&target).then_some(target)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

fn set_nonblocking(fd: RawFd) {
    // SAFETY: plain syscalls on an open descriptor
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

struct Client {
    stream: UnixStream,
    /// Sent before the log: the hello
    prefix: Vec<u8>,
    /// The next log offset to send, once it has attached
    pos: Option<u64>,
    input: Vec<u8>,
    name: Option<String>,
    /// Gone, or done with
    closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stop {
    No,
    Closing(Instant),
    Termed(Instant),
    Killed,
}

struct Relay {
    options: Options,
    log: File,
    end: u64,
    head: Option<u64>,
    turn_start: Option<u64>,
    busy: bool,
    child: Child,
    stdin: Option<ChildStdin>,
    /// Bytes for the CLI not yet written
    to_cli: Vec<u8>,
    close_stdin: bool,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    partial: Vec<u8>,
    stderr_tail: Vec<u8>,
    err_log: Option<File>,
    clients: Vec<Client>,
    seen: HashSet<String>,
    order: VecDeque<String>,
    stop: Stop,
    status: Option<std::process::ExitStatus>,
    exited_at: Option<Instant>,
    /// When it last did anything, or a client last left
    active_at: Instant,
    alone_since: Instant,
    state_dirty: bool,
    state_at: Instant,
}

/// Run the relay until its CLI has ended, giving the CLI's exit code.
/// `signals` is read from when a signal asks it to stop
pub fn relay(options: Options, signals: Option<RawFd>) -> Result<i32, String> {
    let dir = options.dir.clone();
    fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK))
        .map_err(|err| err.to_string())?;
    if !lock_exclusive(&lock) {
        return Ok(RUNNING);
    }
    // A relay restarted here begins a new log, as clients' offsets were
    // into the old one
    let log_path = dir.join(LOG);
    if fs::metadata(&log_path).is_ok_and(|m| m.len() > 0) {
        let _ = fs::rename(&log_path, dir.join(format!("{LOG}.1")));
    }
    let log = OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&log_path)
        .map_err(|err| err.to_string())?;
    let socket = socket_path(&dir).map_err(|err| err.to_string())?;
    let _ = fs::remove_file(&socket);
    let listener =
        UnixListener::bind(&socket).map_err(|err| format!("{}: {err}", socket.display()))?;
    listener
        .set_nonblocking(true)
        .map_err(|err| err.to_string())?;
    let err_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("err.log"))
        .ok();

    let fail = |mut log: &File, code: i32, message: String| {
        let _ = log.write_all(exit_line(code, message.as_bytes()).as_bytes());
        let end = log.metadata().map_or(0, |m| m.len());
        let state = json!({"pid": std::process::id(), "busy": false, "clients": 0, "head": end,
            "turn_start": end, "end": end, "exited": true, "code": code, "updated": crate::now()});
        let _ = write_atomic(&dir.join(STATE), state.to_string().as_bytes());
        let _ = fs::remove_file(&socket);
        Ok(code)
    };
    if let Some(id) = &options.conversation
        && let Err(holder) = claim_conversation(&dir, id)
    {
        return fail(
            &log,
            CLAIMED,
            format!("conversation {id} is open in thread {holder}"),
        );
    }
    let mut command = std::process::Command::new(options.program.first().ok_or("no program")?);
    command
        .args(&options.program[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // In a session of its own, apart from any terminal the relay has
    // SAFETY: setsid is async-signal-safe
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            return fail(
                &log,
                127,
                format!("{}: {err}", options.program[0].to_string_lossy()),
            );
        }
    };
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    for fd in [
        stdin.as_ref().map(AsRawFd::as_raw_fd),
        stdout.as_ref().map(AsRawFd::as_raw_fd),
        stderr.as_ref().map(AsRawFd::as_raw_fd),
    ]
    .into_iter()
    .flatten()
    {
        set_nonblocking(fd);
    }
    let now = Instant::now();
    let mut relay = Relay {
        options,
        log,
        end: 0,
        head: None,
        turn_start: None,
        busy: false,
        child,
        stdin,
        to_cli: Vec::new(),
        close_stdin: false,
        stdout,
        stderr,
        partial: Vec::new(),
        stderr_tail: Vec::new(),
        err_log,
        clients: Vec::new(),
        seen: HashSet::new(),
        order: VecDeque::new(),
        stop: Stop::No,
        status: None,
        exited_at: None,
        active_at: now,
        alone_since: now,
        state_dirty: true,
        state_at: now,
    };
    // Inputs the last relay here had, as a client may send them again
    if let Ok(old) = File::open(relay.options.dir.join(format!("{LOG}.1"))) {
        for line in io::BufReader::new(old).split(b'\n').map_while(Result::ok) {
            if line.starts_with(b"#ct< ")
                && let Some(Record::Input { id, .. }) =
                    Record::parse(&String::from_utf8_lossy(&line))
            {
                relay.remember(&id);
            }
        }
    }
    relay.write_state();
    let code = relay.run(&listener, signals);
    drop(listener);
    let _ = fs::remove_file(&socket);
    relay.finish(code);
    drop(lock);
    Ok(code)
}

impl Relay {
    fn hello(&self) -> Hello {
        let head = self.head.unwrap_or(self.end);
        Hello {
            pid: std::process::id(),
            head,
            turn_start: self.turn_start.unwrap_or(head),
            end: self.end,
            busy: self.busy,
        }
    }

    fn attached(&self) -> usize {
        self.clients
            .iter()
            .filter(|c| c.pos.is_some() && !c.closed)
            .count()
    }

    fn write_state(&mut self) {
        let hello = self.hello();
        let names: Vec<&str> = self
            .clients
            .iter()
            .filter(|c| c.pos.is_some())
            .filter_map(|c| c.name.as_deref())
            .collect();
        let code = self.status.map(exit_code);
        let state = json!({
            "pid": hello.pid,
            "cli": self.child.id(),
            "busy": hello.busy,
            "clients": self.attached(),
            "names": names,
            "head": hello.head,
            "turn_start": hello.turn_start,
            "end": hello.end,
            "exited": code.is_some(),
            "code": code,
            "updated": crate::now(),
        });
        let _ = write_atomic(&self.options.dir.join(STATE), state.to_string().as_bytes());
        self.state_dirty = false;
        self.state_at = Instant::now();
    }

    fn append(&mut self, bytes: &[u8]) {
        if self.log.write_all(bytes).is_ok() {
            self.end += bytes.len() as u64;
        }
        self.state_dirty = true;
    }

    fn remember(&mut self, id: &str) -> bool {
        if !self.seen.insert(id.to_string()) {
            return false;
        }
        self.order.push_back(id.to_string());
        if self.order.len() > REMEMBERED
            && let Some(old) = self.order.pop_front()
        {
            self.seen.remove(&old);
        }
        true
    }

    fn begin_stop(&mut self) {
        if self.stop == Stop::No {
            self.stop = Stop::Closing(Instant::now());
            self.close_stdin = true;
        }
    }

    fn signal(&self, signal: libc::c_int) {
        if self.status.is_none() {
            // SAFETY: the CLI leads its own session, so its group is its pid
            unsafe { libc::killpg(self.child.id() as libc::pid_t, signal) };
        }
    }

    /// A client's line
    fn command(&mut self, client: usize, line: &[u8]) {
        let line = String::from_utf8_lossy(line);
        let line = line.trim_end_matches('\r');
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "attach" => {
                let (from, name) = rest.split_once(' ').unwrap_or((rest, ""));
                let hello = self.hello();
                let pos = match from.parse::<Cursor>() {
                    Ok(Cursor::Offset(n)) => n.min(self.end),
                    Ok(Cursor::Live) if hello.busy => hello.turn_start,
                    _ => self.end,
                };
                let c = &mut self.clients[client];
                if c.pos.is_none() {
                    c.prefix = hello.line().into_bytes();
                    c.pos = Some(pos);
                    c.name = (!name.is_empty()).then(|| name.to_string());
                    self.state_dirty = true;
                    self.write_state();
                }
            }
            "turn" | "line" => {
                let (id, json) = rest.split_once(' ').unwrap_or((rest, ""));
                if !valid_id(id) || !self.remember(id) {
                    return;
                }
                self.active_at = Instant::now();
                if word == "turn" {
                    self.head.get_or_insert(self.end);
                    self.turn_start = Some(self.end);
                    self.busy = true;
                }
                self.append(format!("#ct< {id} {word} {json}\n").as_bytes());
                if self.stdin.is_some() && !self.close_stdin {
                    self.to_cli.extend_from_slice(json.as_bytes());
                    self.to_cli.push(b'\n');
                }
                if word == "turn" {
                    self.write_state();
                }
            }
            "meta" => self.meta(rest),
            "stop" => self.begin_stop(),
            _ => {}
        }
    }

    fn meta(&mut self, json: &str) {
        let Ok(Value::Object(update)) = serde_json::from_str::<Value>(json) else {
            return;
        };
        let path = self.options.dir.join(META);
        let mut meta = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| match v {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default();
        if let Some(id) = update.get("conversation").and_then(Value::as_str)
            && meta.get("conversation").and_then(Value::as_str) != Some(id)
            && let Err(holder) = claim_conversation(&self.options.dir, id)
        {
            self.note(&format!(
                "conversation {id} is also open in thread {holder}\n"
            ));
        }
        meta.extend(update);
        let _ = write_atomic(&path, Value::Object(meta).to_string().as_bytes());
    }

    fn note(&mut self, text: &str) {
        if let Some(log) = &mut self.err_log {
            let _ = log.write_all(text.as_bytes());
        }
    }

    fn read_stdout(&mut self) {
        let Some(stdout) = &mut self.stdout else {
            return;
        };
        let mut buf = [0u8; 65536];
        let mut eof = false;
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(n) => self.partial.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    eof = true;
                    break;
                }
            }
            if self.partial.len() > 1 << 20 {
                break;
            }
        }
        if eof {
            self.stdout = None;
            if !self.partial.is_empty() && !self.partial.ends_with(b"\n") {
                self.partial.push(b'\n');
            }
        }
        let Some(cut) = self.partial.iter().rposition(|&b| b == b'\n') else {
            return;
        };
        let lines: Vec<u8> = self.partial.drain(..=cut).collect();
        self.active_at = Instant::now();
        if self.busy
            && let Some(done) = &self.options.done
            && lines
                .split(|&b| b == b'\n')
                .any(|line| contains(line, done.as_bytes()))
        {
            self.busy = false;
            self.append(&lines);
            self.write_state();
            return;
        }
        self.append(&lines);
    }

    fn read_stderr(&mut self) {
        let Some(stderr) = &mut self.stderr else {
            return;
        };
        let mut buf = [0u8; 16384];
        loop {
            match stderr.read(&mut buf) {
                Ok(0) => {
                    self.stderr = None;
                    break;
                }
                Ok(n) => {
                    self.stderr_tail.extend_from_slice(&buf[..n]);
                    if let Some(log) = &mut self.err_log {
                        let _ = log.write_all(&buf[..n]);
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.stderr = None;
                    break;
                }
            }
        }
        if self.stderr_tail.len() > 2 * STDERR_TAIL {
            self.stderr_tail
                .drain(..self.stderr_tail.len() - STDERR_TAIL);
        }
    }

    fn write_cli(&mut self) {
        if let Some(stdin) = &mut self.stdin {
            while !self.to_cli.is_empty() {
                match stdin.write(&self.to_cli) {
                    Ok(n) => {
                        self.to_cli.drain(..n);
                    }
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        self.stdin = None;
                        self.to_cli.clear();
                        break;
                    }
                }
            }
        }
        if self.close_stdin && self.to_cli.is_empty() {
            self.stdin = None;
        }
    }

    fn read_client(&mut self, index: usize) {
        let mut buf = [0u8; 65536];
        loop {
            let client = &mut self.clients[index];
            match client.stream.read(&mut buf) {
                Ok(0) => {
                    client.closed = true;
                    break;
                }
                Ok(n) => client.input.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    client.closed = true;
                    break;
                }
            }
        }
        while let Some(cut) = self.clients[index].input.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.clients[index].input.drain(..=cut).collect();
            self.command(index, &line[..line.len() - 1]);
        }
        // A client that left still had its say
        if self.clients[index].closed {
            self.clients[index].pos = None;
        }
    }

    /// Send a client what it has not had, as far as it takes it
    fn write_client(&mut self, index: usize) {
        let mut buf = vec![0u8; 65536];
        let end = self.end;
        let client = &mut self.clients[index];
        while !client.closed {
            if !client.prefix.is_empty() {
                match client.stream.write(&client.prefix) {
                    Ok(n) => {
                        client.prefix.drain(..n);
                        continue;
                    }
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        client.closed = true;
                        break;
                    }
                }
            }
            let Some(pos) = client.pos.filter(|&pos| pos < end) else {
                break;
            };
            let want = ((end - pos) as usize).min(buf.len());
            let Ok(got) = self.log.read_at(&mut buf[..want], pos) else {
                break;
            };
            if got == 0 {
                break;
            }
            match client.stream.write(&buf[..got]) {
                Ok(n) => client.pos = Some(pos + n as u64),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => client.closed = true,
            }
        }
    }

    fn wants_write(&self, client: &Client) -> bool {
        !client.closed
            && (!client.prefix.is_empty() || client.pos.is_some_and(|pos| pos < self.end))
    }

    /// Timers, and whether the CLI is done with
    fn tick(&mut self) -> bool {
        let now = Instant::now();
        if self.status.is_none()
            && let Ok(Some(status)) = self.child.try_wait()
        {
            self.status = Some(status);
            self.exited_at = Some(now);
            self.busy = false;
        }
        if let Some(at) = self.exited_at {
            // Its output, unless something it left behind holds the pipe
            if (self.stdout.is_none() && self.stderr.is_none()) || now - at > Duration::from_secs(1)
            {
                return true;
            }
        }
        if self.attached() == 0 && self.stop == Stop::No && self.status.is_none() {
            let idle = now.saturating_duration_since(self.active_at.max(self.alone_since));
            if !self.busy && self.options.linger.is_some_and(|linger| idle >= linger) {
                self.note("stopping: idle with no window\n");
                self.begin_stop();
            } else if self.busy
                && now.saturating_duration_since(self.alone_since) >= self.options.max_blocked
            {
                self.note("stopping: busy too long with no window\n");
                self.begin_stop();
            }
        }
        let grace = self.options.grace;
        match self.stop {
            Stop::Closing(at) if now - at >= grace => {
                self.signal(libc::SIGTERM);
                self.stop = Stop::Termed(now);
            }
            Stop::Termed(at) if now - at >= grace => {
                self.signal(libc::SIGKILL);
                self.stop = Stop::Killed;
            }
            _ => {}
        }
        if self.state_dirty && now - self.state_at >= Duration::from_secs(1) {
            self.write_state();
        }
        false
    }

    fn run(&mut self, listener: &UnixListener, signals: Option<RawFd>) -> i32 {
        loop {
            self.write_cli();
            if self.tick() {
                break;
            }
            let mut fds = vec![
                libc::pollfd {
                    fd: listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: signals.unwrap_or(-1),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.stdout.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.stderr.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self
                        .stdin
                        .as_ref()
                        .filter(|_| !self.to_cli.is_empty())
                        .map_or(-1, AsRawFd::as_raw_fd),
                    events: libc::POLLOUT,
                    revents: 0,
                },
            ];
            const CLIENTS: usize = 5;
            for client in &self.clients {
                let events = libc::POLLIN
                    | if self.wants_write(client) {
                        libc::POLLOUT
                    } else {
                        0
                    };
                fds.push(libc::pollfd {
                    fd: client.stream.as_raw_fd(),
                    events,
                    revents: 0,
                });
            }
            let timeout = if self.exited_at.is_some() || self.stop != Stop::No {
                100
            } else {
                1000
            };
            // SAFETY: `fds` is a live array of pollfds of its length
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
            if n < 0 {
                if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                    std::thread::sleep(Duration::from_millis(10));
                }
                continue;
            }
            let ready = |i: usize| fds[i].revents != 0;
            if ready(1)
                && let Some(fd) = signals
            {
                let mut buf = [0u8; 64];
                // SAFETY: reading into a buffer of its length
                unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
                self.begin_stop();
            }
            if ready(2) {
                self.read_stdout();
            }
            if ready(3) {
                self.read_stderr();
            }
            for index in 0..self.clients.len() {
                let revents = fds[CLIENTS + index].revents;
                if revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                    self.read_client(index);
                }
            }
            if ready(0) {
                while let Ok((stream, _)) = listener.accept() {
                    let _ = stream.set_nonblocking(true);
                    self.clients.push(Client {
                        stream,
                        prefix: Vec::new(),
                        pos: None,
                        input: Vec::new(),
                        name: None,
                        closed: false,
                    });
                }
            }
            for index in 0..self.clients.len() {
                if self.wants_write(&self.clients[index]) {
                    self.write_client(index);
                }
            }
            let had = self.clients.len();
            self.clients.retain(|c| !c.closed);
            if self.clients.len() != had {
                if self.attached() == 0 {
                    self.alone_since = Instant::now();
                }
                self.write_state();
            }
        }
        self.status.map_or(-1, exit_code)
    }

    /// Record the end, and see the clients have it
    fn finish(&mut self, code: i32) {
        self.read_stdout();
        if !self.partial.is_empty() {
            let mut rest = std::mem::take(&mut self.partial);
            rest.push(b'\n');
            self.append(&rest);
        }
        self.read_stderr();
        let mut tail = &self.stderr_tail[self.stderr_tail.len().saturating_sub(STDERR_TAIL)..];
        if self.stderr_tail.len() > STDERR_TAIL
            && let Some(cut) = tail.iter().position(|&b| b == b'\n')
        {
            tail = &tail[cut + 1..];
        }
        let line = exit_line(code, tail);
        self.append(line.as_bytes());
        self.busy = false;
        self.write_state();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            for index in 0..self.clients.len() {
                self.write_client(index);
            }
            if !self.clients.iter().any(|c| self.wants_write(c)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        for client in &self.clients {
            let _ = client.stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// How to reach a relay
#[derive(Clone, Debug)]
pub struct Connect {
    pub dir: PathBuf,
    pub from: Cursor,
    /// How long to wait for the relay to start
    pub wait: Duration,
    pub client: Option<String>,
}

/// Bridge `input` and `output` to the relay in `dir`, or replay its log
/// once it is gone: see the module's description
pub fn connect(
    options: &Connect,
    input: impl Read + Send + 'static,
    mut output: impl Write,
) -> io::Result<()> {
    let deadline = Instant::now() + options.wait;
    loop {
        if let Some(mut stream) = socket_path(&options.dir)
            .ok()
            .and_then(|path| UnixStream::connect(path).ok())
        {
            let mut attach = format!("attach {}", options.from);
            if let Some(name) = options
                .client
                .as_deref()
                .filter(|n| !n.contains(char::is_whitespace))
            {
                attach.push(' ');
                attach.push_str(name);
            }
            attach.push('\n');
            if stream.write_all(attach.as_bytes()).is_ok() {
                let mut reader = io::BufReader::new(stream.try_clone()?);
                let mut hello = Vec::new();
                if reader.read_until(b'\n', &mut hello).is_ok() && hello.starts_with(b"#ct hello ")
                {
                    output.write_all(&hello)?;
                    output.flush()?;
                    return bridge(stream, reader, input, output);
                }
            }
        }
        // Not there yet, or ending
        let started = options.dir.join(STATE).exists();
        if (started && !is_live(&options.dir)) || Instant::now() >= deadline {
            return replay(options, &mut output);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn bridge(
    stream: UnixStream,
    mut reader: io::BufReader<UnixStream>,
    mut input: impl Read + Send + 'static,
    mut output: impl Write,
) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 65536];
        loop {
            match input.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if writer.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            }
        }
        // Detached: the relay lets this client go, and its end follows
        let _ = writer.shutdown(std::net::Shutdown::Write);
    });
    let mut buf = vec![0u8; 65536];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        output.write_all(&buf[..n])?;
        output.flush()?;
    }
}

/// What a relay that has gone left: a hello, its log from the offset, and
/// how it ended
fn replay(options: &Connect, output: &mut impl Write) -> io::Result<()> {
    let state = fs::read(options.dir.join(STATE))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .unwrap_or(Value::Null);
    let log = File::open(options.dir.join(LOG)).ok();
    let end = log
        .as_ref()
        .and_then(|l| l.metadata().ok())
        .map_or(0, |m| m.len());
    let get = |key: &str| state.get(key).and_then(Value::as_u64).unwrap_or(0).min(end);
    let hello = Hello {
        pid: 0,
        head: get("head"),
        turn_start: get("turn_start"),
        end,
        busy: false,
    };
    output.write_all(hello.line().as_bytes())?;
    let from = match options.from {
        Cursor::Offset(n) => n.min(end),
        Cursor::Live => end,
    };
    let mut ended = false;
    if let Some(log) = &log {
        let mut buf = vec![0u8; 65536];
        let mut pos = from;
        while pos < end {
            let n = log.read_at(&mut buf[..((end - pos) as usize).min(65536)], pos)?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n])?;
            pos += n as u64;
        }
        // Whether the log ends with the relay's account of the end, which
        // is given again to a client that has read past its start
        let start = end.saturating_sub(STDERR_TAIL as u64 * 3);
        let mut tail = vec![0u8; (end - start) as usize];
        if log.read_exact_at(&mut tail, start).is_ok() {
            let body = tail.strip_suffix(b"\n").unwrap_or(&tail);
            let last = body.rsplit(|&b| b == b'\n').next().unwrap_or_default();
            ended = last.starts_with(b"#ct exited ");
            let last_start = end - (tail.len() - (body.len() - last.len())) as u64;
            if ended && from > last_start {
                output.write_all(last)?;
                output.write_all(b"\n")?;
            }
        }
    }
    if !ended {
        output.write_all(b"#ct lost\n")?;
    }
    output.flush()
}

/// Ask the relay in `dir` to stop, and wait up to `wait` for it to
pub fn stop(dir: &Path, wait: Duration) -> bool {
    if let Ok(mut stream) = socket_path(dir).and_then(UnixStream::connect) {
        let _ = stream.write_all(b"stop\n");
        let _ = stream.shutdown(std::net::Shutdown::Write);
    }
    let deadline = Instant::now() + wait;
    while is_live(dir) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

static SIGNAL_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn on_signal(_: libc::c_int) {
    let fd = SIGNAL_FD.load(std::sync::atomic::Ordering::Relaxed);
    // SAFETY: write is async-signal-safe
    unsafe { libc::write(fd, b"s".as_ptr().cast(), 1) };
}

/// A pipe that SIGHUP, SIGINT and SIGTERM write to, giving its read end
fn stop_signals() -> Option<RawFd> {
    let mut fds = [0; 2];
    // SAFETY: plain syscalls; the handler only writes to the pipe
    unsafe {
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) != 0 {
            return None;
        }
        SIGNAL_FD.store(fds[1], std::sync::atomic::Ordering::Relaxed);
        for signal in [libc::SIGHUP, libc::SIGINT, libc::SIGTERM] {
            libc::signal(signal, on_signal as *const () as libc::sighandler_t);
        }
    }
    Some(fds[0])
}

fn seconds(value: Option<OsString>, flag: &str) -> Result<Duration, String> {
    value
        .and_then(|v| v.to_str().and_then(|v| v.parse::<f64>().ok()))
        .filter(|v| *v >= 0.0)
        .map(Duration::from_secs_f64)
        .ok_or_else(|| format!("{flag} needs a number of seconds"))
}

/// `ct-supervise --relay …`, given the arguments after `--relay`
pub fn relay_main(args: Vec<OsString>) -> i32 {
    let mut args = args.into_iter();
    let Some(dir) = args.next() else {
        eprintln!(
            "usage: ct-supervise --relay DIR [--done PATTERN] [--linger SECS] [--max-blocked SECS] [--conversation ID] -- PROGRAM ARGS..."
        );
        return 2;
    };
    let mut options = Options::new(dir, Vec::new());
    let parsed = (|| {
        while let Some(arg) = args.next() {
            match arg.to_str().unwrap_or_default() {
                "--done" => {
                    options.done = Some(
                        args.next()
                            .ok_or("--done needs a pattern")?
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
                "--linger" => options.linger = Some(seconds(args.next(), "--linger")?),
                "--max-blocked" => options.max_blocked = seconds(args.next(), "--max-blocked")?,
                "--grace" => options.grace = seconds(args.next(), "--grace")?,
                "--conversation" => {
                    options.conversation = Some(
                        args.next()
                            .ok_or("--conversation needs an id")?
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
                "--" => break,
                other => return Err(format!("unknown option {other}")),
            }
        }
        options.program = args.by_ref().collect();
        if options.program.is_empty() {
            Err("no program".to_string())
        } else {
            Ok(())
        }
    })();
    if let Err(err) = parsed {
        eprintln!("ct-supervise --relay: {err}");
        return 2;
    }
    match relay(options, stop_signals()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ct-supervise --relay: {err}");
            1
        }
    }
}

/// `ct-supervise --connect …`, given the arguments after `--connect`
pub fn connect_main(args: Vec<OsString>) -> i32 {
    let mut args = args.into_iter();
    let Some(dir) = args.next() else {
        eprintln!(
            "usage: ct-supervise --connect DIR [--from OFFSET|live] [--wait SECS] [--client NAME] | --connect DIR --stop"
        );
        return 2;
    };
    let mut options = Connect {
        dir: dir.into(),
        from: Cursor::Offset(0),
        wait: Duration::ZERO,
        client: None,
    };
    let mut stopping = false;
    let parsed = (|| {
        while let Some(arg) = args.next() {
            match arg.to_str().unwrap_or_default() {
                "--from" => {
                    options.from = args
                        .next()
                        .ok_or("--from needs an offset")?
                        .to_string_lossy()
                        .parse()?
                }
                "--wait" => options.wait = seconds(args.next(), "--wait")?,
                "--client" => {
                    options.client = Some(
                        args.next()
                            .ok_or("--client needs a name")?
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
                "--stop" => stopping = true,
                other => return Err(format!("unknown option {other}")),
            }
        }
        Ok::<_, String>(())
    })();
    if let Err(err) = parsed {
        eprintln!("ct-supervise --connect: {err}");
        return 2;
    }
    if stopping {
        return if stop(&options.dir, Duration::from_secs(20)) {
            0
        } else {
            1
        };
    }
    match connect(&options, io::stdin(), io::stdout()) {
        Ok(()) => 0,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(err) => {
            eprintln!("ct-supervise --connect: {err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in CLI: each line it reads comes back as `{"got":LINE}`, and
    /// a line with `"turn"` in it is answered with a result line. `quit`
    /// makes it exit with 3, `slow` makes it take a second
    const FAKE: &str = r#"
trap 'echo terminated >&2; exit 143' TERM
while IFS= read -r line; do
  case "$line" in *quit*) echo "bye" >&2; exit 3 ;; esac
  case "$line" in *slow*) sleep 1 ;; esac
  printf '{"got":%s}\n' "$line"
  case "$line" in *'"turn"'*) printf '{"type":"result"}\n' ;; esac
done
echo "stdin closed" >&2
"#;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ct-relay-{name}-{}-{}",
            std::process::id(),
            crate::random_hex(6)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn start(
        threads: &Path,
        name: &str,
        configure: impl FnOnce(&mut Options),
    ) -> (PathBuf, std::thread::JoinHandle<Result<i32, String>>) {
        start_with(threads, name, FAKE, configure)
    }

    fn start_with(
        threads: &Path,
        name: &str,
        script: &str,
        configure: impl FnOnce(&mut Options),
    ) -> (PathBuf, std::thread::JoinHandle<Result<i32, String>>) {
        let dir = threads.join(name);
        let mut options = Options::new(&dir, vec!["sh".into(), "-c".into(), script.into()]);
        options.done = Some("\"type\":\"result\"".into());
        configure(&mut options);
        let handle = std::thread::spawn(move || relay(options, None));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.join(STATE).exists() {
            assert!(Instant::now() < deadline, "the relay did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        (dir, handle)
    }

    /// A client on the socket, reading lines in the background
    struct TestClient {
        stream: UnixStream,
        lines: std::sync::mpsc::Receiver<String>,
    }

    impl Drop for TestClient {
        fn drop(&mut self) {
            let _ = self.stream.shutdown(std::net::Shutdown::Both);
        }
    }

    impl TestClient {
        fn attach(dir: &Path, from: &str) -> Self {
            let mut stream = UnixStream::connect(socket_path(dir).unwrap()).unwrap();
            stream
                .write_all(format!("attach {from} test\n").as_bytes())
                .unwrap();
            let (tx, lines) = std::sync::mpsc::channel();
            let reader = io::BufReader::new(stream.try_clone().unwrap());
            std::thread::spawn(move || {
                for line in reader.lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
            Self { stream, lines }
        }

        fn send(&mut self, line: &str) {
            self.stream
                .write_all(format!("{line}\n").as_bytes())
                .unwrap();
        }

        /// Lines until one contains `want`
        fn until(&self, want: &str) -> Vec<String> {
            let mut got = Vec::new();
            loop {
                match self.lines.recv_timeout(Duration::from_secs(10)) {
                    Ok(line) => {
                        let done = line.contains(want);
                        got.push(line);
                        if done {
                            return got;
                        }
                    }
                    Err(_) => panic!("no {want:?} in {got:#?}"),
                }
            }
        }
    }

    fn state(dir: &Path) -> Value {
        serde_json::from_slice(&fs::read(dir.join(STATE)).unwrap()).unwrap()
    }

    fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !check() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn relay_replays_from_an_offset_and_runs_inputs_once() {
        let threads = tempdir("replay");
        let (dir, handle) = start(&threads, "t1", |_| {});
        let mut a = TestClient::attach(&dir, "0");
        let hello = Hello::parse(&a.until("#ct hello")[0]).unwrap();
        assert_eq!((hello.end, hello.busy), (0, false));
        a.send("line l1 {\"n\":1}");
        a.send("turn t1 {\"turn\":1}");
        // A repeat, as after a reconnect, is dropped
        a.send("turn t1 {\"turn\":1}");
        a.send("line l2 {\"n\":2}");
        let got = a.until("{\"got\":{\"n\":2}}");
        assert_eq!(
            got.iter().filter(|l| l.contains("\"turn\":1")).count(),
            2,
            "{got:#?}"
        );
        wait_for("idle", || !state(&dir)["busy"].as_bool().unwrap());
        let log = fs::read(dir.join(LOG)).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&log)
                .matches("#ct< t1 turn")
                .count(),
            1
        );

        // Reattaching at an offset gives exactly the log's bytes from there
        let offset = String::from_utf8_lossy(&log).find("#ct< t1 ").unwrap() as u64;
        let b = TestClient::attach(&dir, &offset.to_string());
        let hello = Hello::parse(&b.until("#ct hello")[0]).unwrap();
        assert_eq!(hello.end, log.len() as u64);
        assert_eq!(hello.head, offset);
        let replayed = b.until("{\"got\":{\"n\":2}}").join("\n") + "\n";
        assert_eq!(replayed.as_bytes(), &log[offset as usize..]);
        assert_eq!(state(&dir)["clients"], 2);

        // `live` starts at the turn in progress, else the end
        let c = TestClient::attach(&dir, "live");
        let hello = Hello::parse(&c.until("#ct hello")[0]).unwrap();
        assert!(c.lines.recv_timeout(Duration::from_millis(200)).is_err());
        assert_eq!(hello.end, log.len() as u64);

        a.send("stop");
        assert_eq!(handle.join().unwrap(), Ok(0));
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn relay_broadcasts_to_every_client() {
        let threads = tempdir("broadcast");
        let (dir, handle) = start(&threads, "t1", |_| {});
        let mut a = TestClient::attach(&dir, "0");
        let b = TestClient::attach(&dir, "0");
        a.until("#ct hello");
        b.until("#ct hello");
        a.send("turn x1 {\"turn\":\"hi\"}");
        for client in [&a, &b] {
            let got = client.until("\"type\":\"result\"");
            assert!(got[0].starts_with("#ct< x1 turn "), "{got:#?}");
            assert!(
                got.iter().any(|l| l == "{\"got\":{\"turn\":\"hi\"}}"),
                "{got:#?}"
            );
        }
        a.send("stop");
        handle.join().unwrap().unwrap();
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn relay_stops_gracefully_and_connect_replays_after() {
        let threads = tempdir("stop");
        let (dir, handle) = start(&threads, "t1", |_| {});
        assert!(is_live(&dir));
        let mut a = TestClient::attach(&dir, "0");
        a.send("line l1 {\"n\":1}");
        a.until("\"got\"");
        let started = Instant::now();
        assert!(stop(&dir, Duration::from_secs(10)));
        // Closing stdin was enough: no SIGTERM
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(handle.join().unwrap(), Ok(0));
        let got = a.until("#ct exited");
        let Some(Record::Exited { code, stderr }) = Record::parse(got.last().unwrap()) else {
            panic!("{got:#?}")
        };
        assert_eq!((code, stderr.trim()), (0, "stdin closed"));
        assert!(!is_live(&dir));
        assert!(!socket_path(&dir).unwrap().exists());
        assert_eq!(state(&dir)["exited"], true);

        // After the end, connecting replays the rest, with the exit record
        let log = fs::read(dir.join(LOG)).unwrap();
        let mut out = Vec::new();
        let options = Connect {
            dir: dir.clone(),
            from: Cursor::Offset(5),
            wait: Duration::from_secs(1),
            client: None,
        };
        connect(&options, io::empty(), &mut out).unwrap();
        let (hello, rest) = out.split_at(out.iter().position(|&b| b == b'\n').unwrap() + 1);
        let hello = Hello::parse(&String::from_utf8_lossy(hello)).unwrap();
        assert_eq!((hello.pid, hello.end), (0, log.len() as u64));
        assert_eq!(rest, &log[5..]);
        // and one that had read it all is told again how it ended
        let mut out = Vec::new();
        connect(
            &Connect {
                from: Cursor::Live,
                ..options
            },
            io::empty(),
            &mut out,
        )
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        let last = String::from_utf8_lossy(&log)
            .lines()
            .last()
            .unwrap()
            .to_string();
        assert_eq!(out.lines().nth(1), Some(last.as_str()));
        assert_eq!(out.lines().count(), 2);
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn relay_terminates_a_cli_that_ignores_stdin() {
        let threads = tempdir("term");
        let script = "trap 'echo terminated >&2; exit 143' TERM\nwhile :; do sleep 0.05; done\n";
        let (dir, handle) = start_with(&threads, "t1", script, |o| {
            o.grace = Duration::from_millis(300)
        });
        assert!(stop(&dir, Duration::from_secs(10)));
        assert_eq!(handle.join().unwrap(), Ok(143));
        let log = fs::read_to_string(dir.join(LOG)).unwrap();
        assert_eq!(
            Record::parse(log.lines().last().unwrap()),
            Some(Record::Exited {
                code: 143,
                stderr: "terminated\n".into()
            })
        );
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn relay_lingers_only_while_idle() {
        let threads = tempdir("linger");
        let (dir, handle) = start(&threads, "t1", |o| {
            o.linger = Some(Duration::from_millis(600))
        });
        let mut a = TestClient::attach(&dir, "0");
        // A slow turn: busy, so leaving does not stop it
        a.send("turn t1 {\"turn\":\"slow\"}");
        a.until("#ct< t1");
        drop(a);
        wait_for("busy", || state(&dir)["clients"] == 0);
        assert!(state(&dir)["busy"].as_bool().unwrap());
        // Once the turn is done, it waits for the linger time and stops
        let started = Instant::now();
        assert_eq!(handle.join().unwrap(), Ok(0));
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "{:?}",
            started.elapsed()
        );
        let log = fs::read_to_string(dir.join(LOG)).unwrap();
        assert!(
            log.contains("{\"type\":\"result\"}\n#ct exited 0 "),
            "{log}"
        );
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn relay_records_how_the_cli_exited() {
        let threads = tempdir("exit");
        let (dir, handle) = start(&threads, "t1", |_| {});
        let mut a = TestClient::attach(&dir, "0");
        a.send("line q {\"quit\":1}");
        let got = a.until("#ct exited");
        assert_eq!(
            Record::parse(got.last().unwrap()),
            Some(Record::Exited {
                code: 3,
                stderr: "bye\n".into()
            })
        );
        assert_eq!(handle.join().unwrap(), Ok(3));
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn connect_waits_for_the_relay_and_bridges() {
        let threads = tempdir("connect");
        let dir = threads.join("t1");
        let (to_relay, input) = std::os::unix::net::UnixStream::pair().unwrap();
        let (output, from_relay) = std::os::unix::net::UnixStream::pair().unwrap();
        let options = Connect {
            dir: dir.clone(),
            from: Cursor::Offset(0),
            wait: Duration::from_secs(10),
            client: Some("me".into()),
        };
        let bridge = std::thread::spawn(move || connect(&options, input, output));
        // Started after the connector
        std::thread::sleep(Duration::from_millis(200));
        let (_, handle) = start(&threads, "t1", |_| {});
        let mut to_relay = to_relay;
        to_relay.write_all(b"turn a {\"turn\":1}\n").unwrap();
        let mut lines = io::BufReader::new(from_relay).lines();
        assert!(
            lines
                .next()
                .unwrap()
                .unwrap()
                .starts_with("#ct hello 1 pid=")
        );
        assert_eq!(lines.next().unwrap().unwrap(), "#ct< a turn {\"turn\":1}");
        assert_eq!(lines.next().unwrap().unwrap(), "{\"got\":{\"turn\":1}}");
        wait_for("the client's name", || {
            state(&dir)["names"] == json!(["me"])
        });
        // Closing the input detaches, and the bridge ends
        drop(to_relay);
        bridge.join().unwrap().unwrap();
        wait_for("no clients", || state(&dir)["clients"] == 0);
        assert!(is_live(&dir));
        assert!(stop(&dir, Duration::from_secs(10)));
        handle.join().unwrap().unwrap();
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn connect_to_a_relay_that_never_came_says_lost() {
        let threads = tempdir("lost");
        let options = Connect {
            dir: threads.join("none"),
            from: Cursor::Offset(0),
            wait: Duration::from_millis(200),
            client: None,
        };
        let mut out = Vec::new();
        connect(&options, io::empty(), &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "#ct hello 1 pid=0 head=0 turn=0 end=0 busy=0\n#ct lost\n"
        );
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn one_relay_a_directory_and_a_conversation() {
        let threads = tempdir("claim");
        let (dir, handle) = start(&threads, "t1", |o| o.conversation = Some("c-1".into()));
        assert!(is_live(&dir));
        // Its lock is its own
        let again = relay(Options::new(&dir, vec!["true".into()]), None);
        assert_eq!(again, Ok(RUNNING));
        assert_eq!(
            conversation_holder(&threads, "c-1"),
            Some(threads.join(BY_CONVERSATION).join("../t1"))
        );
        // A second relay for the conversation ends at once, saying why
        let (other, second) = start(&threads, "t2", |o| o.conversation = Some("c-1".into()));
        assert_eq!(second.join().unwrap(), Ok(CLAIMED));
        let log = fs::read_to_string(other.join(LOG)).unwrap();
        assert_eq!(
            Record::parse(&log),
            Some(Record::Exited {
                code: CLAIMED,
                stderr: "conversation c-1 is open in thread t1".into()
            })
        );
        assert!(stop(&dir, Duration::from_secs(10)));
        handle.join().unwrap().unwrap();
        assert_eq!(conversation_holder(&threads, "c-1"), None);
        // Once that relay has gone the conversation is free, also by meta
        let (third, handle) = start(&threads, "t3", |_| {});
        let mut a = TestClient::attach(&third, "0");
        a.send("meta {\"conversation\":\"c-1\",\"title\":\"x\"}");
        a.send("line l {}");
        a.until("\"got\"");
        assert!(conversation_holder(&threads, "c-1").is_some_and(|d| d.ends_with("t3")));
        let meta: Value = serde_json::from_slice(&fs::read(third.join(META)).unwrap()).unwrap();
        assert_eq!(meta, json!({"conversation": "c-1", "title": "x"}));
        a.send("stop");
        handle.join().unwrap().unwrap();
        let _ = fs::remove_dir_all(threads);
    }

    #[test]
    fn long_directories_get_short_sockets() {
        let long = PathBuf::from("/home").join("x".repeat(120));
        let path = socket_path(&long).unwrap();
        assert!(path.as_os_str().len() < 60, "{}", path.display());
        assert_eq!(path, socket_path(&long).unwrap());
        assert_eq!(socket_path(Path::new("/s")).unwrap(), Path::new("/s/sock"));
    }

    #[test]
    fn records_parse() {
        assert_eq!(
            Record::parse("#ct< ab-1 turn {\"x\":\"a b\"}\n"),
            Some(Record::Input {
                id: "ab-1".into(),
                kind: "turn".into(),
                json: "{\"x\":\"a b\"}".into()
            })
        );
        assert_eq!(
            Record::parse(&exit_line(2, b"oops")),
            Some(Record::Exited {
                code: 2,
                stderr: "oops".into()
            })
        );
        assert_eq!(Record::parse("{\"type\":\"result\"}"), None);
        let hello = Hello {
            pid: 7,
            head: 1,
            turn_start: 2,
            end: 3,
            busy: true,
        };
        assert_eq!(Hello::parse(&hello.line()), Some(hello));
        assert_eq!("live".parse(), Ok(Cursor::Live));
        assert_eq!("12".parse(), Ok(Cursor::Offset(12)));
    }
}
