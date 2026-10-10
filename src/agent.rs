// SPDX-License-Identifier: GPL-3.0-only

//! Tabs as Zellij sessions, on this computer or a remote host.
//!
//! Every tab a profile opens runs in its own Zellij session, so the session
//! outlives the tab: a tab can be closed and reattached, or moved to another
//! window, which is another process, by attaching there and detaching here.

use std::{
    fmt::Write as _,
    hash::{BuildHasher, Hasher, RandomState},
    process::Stdio,
    time::Duration,
};

use crate::host::{self, HostCommand};
use crate::config::{Agent, Config, Profile, ProfileId};

/// Prefix of the Zellij sessions this terminal creates, so they can be told
/// apart from the user's other sessions
pub const SESSION_PREFIX: &str = "ct-";

/// Prefix of the sessions that keep a rich-text thread's CLI, which run a
/// relay rather than a terminal: `ct-thread-<agent>-<dir>-<hex>`
pub const THREAD_PREFIX: &str = "ct-thread-";

/// Where each host keeps its threads' relays: see `cosmic_term::relay`
const THREADS_DIR: &str = "${XDG_STATE_HOME:-$HOME/.local/state}/cosmic-term/threads";

/// How long a thread's CLI is kept, idle, with no window showing it
pub const THREAD_LINGER: Duration = Duration::from_secs(3600);

/// Where the Zellij config and layout are written, on each host
const CONFIG_DIR: &str = "$HOME/.config/cosmic-term-agents";

/// Every script's PATH: first the programs settled for this machine (see
/// [`zellij_resolved`] and [`settle_tools`]), links only to those otherwise
/// missed, then tools installed in the user's home, which login shells add
const PATH_EXPORT: &str =
    "export PATH=\"$HOME/.config/cosmic-term-agents/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\"\n";

/// Zellij config for agent sessions: no key bindings, bars, frames or tips,
/// so that the agent receives every key and the tab bar is the only UI
const ZELLIJ_CONFIG: &str = include_str!("agent/zellij-config.kdl");

/// Prints a tab's project directories; see the script for details
pub mod mounts;
pub mod sudo;

const PROJECT_DIRS: &str = include_str!("agent/project-dirs.py");

/// Prints an agent's recent projects and conversations as JSON; see the
/// script for details
const LAUNCHER: &str = include_str!("agent/launcher.py");

/// Rounds of built-in profiles. Each round is added once, so deleting one of
/// its profiles is permanent
const PROFILE_SEED_VERSION: u32 = 4;

/// Codex's full access mode: no approval prompts and no sandbox
pub const CODEX_FULL_ACCESS: &str = "--dangerously-bypass-approvals-and-sandbox";

/// Add any built-in profiles this config has not had yet, returning whether
/// anything changed
pub fn seed_profiles(config: &mut Config) -> bool {
    // Configs from before rounds were counted recorded the first with a flag
    let done = if config.agent_profiles_seeded {
        config.profile_seed_version.max(1)
    } else {
        config.profile_seed_version
    };
    if done >= PROFILE_SEED_VERSION {
        return false;
    }
    // Profiles for other computers are offered when a machine is added
    let mut seeds = Vec::new();
    if done < 1 {
        seeds.extend([
            Profile::agent("Claude", Agent::Claude, ""),
            Profile::agent("Codex", Agent::Codex, ""),
        ]);
    }
    if done < 2 {
        seeds.push(Profile::agent("Terminal", Agent::Shell, ""));
    }
    // Round 4: Claude in a rich-text thread
    if done < 4 {
        seeds.push(Profile { thread: true, ..Profile::agent("Claude thread", Agent::Claude, "") });
    }
    let mut changed = !seeds.is_empty();
    for mut profile in seeds {
        if profile.agent == Agent::Codex {
            profile.args = CODEX_FULL_ACCESS.to_string();
        }
        let profile_id = config
            .profiles
            .last_key_value()
            .map(|(id, _)| ProfileId(id.0 + 1))
            .unwrap_or_default();
        config.profiles.insert(profile_id, profile);
    }
    // Round 3: Codex runs in full access mode, unless the user has given
    // its profiles arguments of their own
    if done < 3 {
        for profile in config.profiles.values_mut() {
            if profile.agent == Agent::Codex && profile.args.trim().is_empty() {
                profile.args = CODEX_FULL_ACCESS.to_string();
                changed = true;
            }
        }
    }
    config.agent_profiles_seeded = true;
    config.profile_seed_version = PROFILE_SEED_VERSION;
    changed || done < PROFILE_SEED_VERSION
}

/// A Zellij session on this computer or a remote host
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Session {
    /// An ssh destination, or empty for this computer
    pub host: String,
    pub name: String,
}

impl Session {
    pub fn is_local(&self) -> bool {
        self.host.is_empty()
    }

    /// `name` or `name@host`, for passing a session to another window
    pub fn descriptor(&self) -> String {
        if self.is_local() {
            self.name.clone()
        } else {
            format!("{}@{}", self.name, self.host)
        }
    }

    /// The inverse of [`Session::descriptor`]. Session names cannot contain
    /// `@`, but a host can (`user@host`)
    pub fn from_descriptor(descriptor: &str) -> Option<Self> {
        let (name, host) = descriptor.split_once('@').unwrap_or((descriptor, ""));
        (!name.is_empty()).then(|| Self {
            host: host.to_string(),
            name: name.to_string(),
        })
    }

    /// Whether this terminal created the session, rather than the user
    pub fn is_managed(&self) -> bool {
        self.name.starts_with(SESSION_PREFIX)
    }

    /// Whether the session keeps a rich-text thread's CLI
    pub fn is_thread(&self) -> bool {
        self.name.starts_with(THREAD_PREFIX)
    }

    /// The agent a session runs, recovered from its name
    pub fn agent(&self) -> Option<Agent> {
        let rest = self.name.strip_prefix(SESSION_PREFIX)?;
        let rest = rest.strip_prefix("thread-").unwrap_or(rest);
        let (kind, _) = rest.split_once('-')?;
        Agent::ALL
            .into_iter()
            .find(|agent| agent.session_kind() == kind)
    }

    /// A short label for menus and tabs, like `Claude · myproject`
    pub fn label(&self) -> String {
        let rest = self.name.strip_prefix(SESSION_PREFIX).unwrap_or(&self.name);
        let rest = rest.strip_prefix("thread-").unwrap_or(rest);
        match self.agent() {
            Some(agent) => {
                // Drop the random suffix, then the agent
                let detail = rest.rsplit_once('-').map_or(rest, |(d, _)| d);
                let detail = detail
                    .strip_prefix(agent.session_kind())
                    .unwrap_or(detail)
                    .trim_start_matches('-');
                if detail.is_empty() {
                    agent.label()
                } else {
                    format!("{} · {}", agent.label(), detail)
                }
            }
            None => rest.to_string(),
        }
    }
}

/// Start a new session for a profile, returning the tab's command. `dir`
/// overrides the profile's working directory
pub fn launch(profile: &Profile, dir: Option<&str>) -> (HostCommand, Session) {
    launch_resuming(profile, dir, None)
}

/// Like [`launch`], resuming the agent's conversation `resume`
pub fn launch_resuming(
    profile: &Profile,
    dir: Option<&str>,
    resume: Option<&str>,
) -> (HostCommand, Session) {
    let mut args = resume_args(profile.agent, resume);
    args.extend(shlex::split(&profile.args).unwrap_or_default());
    let dir = dir.unwrap_or(&profile.working_directory);
    let session = Session {
        name: new_session_name(profile.agent, dir),
        host: profile.host.trim().to_string(),
    };
    let command = match profile.agent.program() {
        Some(program) => Some((program.to_string(), args)),
        None => HostCommand::parse(&profile.command).map(|c| (c.program, c.args)),
    };
    (session_shell(&session, Some((command, dir))), session)
}

/// A tab on a host, or on this computer if it is empty, that runs one
/// shell command and waits for Enter before it closes, so its output can
/// be read: a short session of its own, as every tab is
pub fn command_tab(host: &str, command: &str) -> (HostCommand, Session) {
    let session = Session {
        name: new_session_name(Agent::Shell, "setup"),
        host: host.trim().to_string(),
    };
    let script = format!("{command}\necho\nprintf 'Press Enter to close this tab. '\nread _\n");
    let pane = Some(("sh".to_string(), vec!["-c".to_string(), script]));
    (session_shell(&session, Some((pane, ""))), session)
}

/// The arguments that make an agent resume a conversation
fn resume_args(agent: Agent, resume: Option<&str>) -> Vec<String> {
    match (agent, resume) {
        (Agent::Claude, Some(id)) => vec!["--resume".into(), id.into()],
        // A subcommand, so it comes before the profile's arguments
        (Agent::Codex, Some(id)) => vec!["resume".into(), id.into()],
        _ => Vec::new(),
    }
}

/// The command for a tab that attaches to an existing session
pub fn attach(session: &Session) -> HostCommand {
    session_shell(session, None)
}

fn new_session_name(agent: Agent, dir: &str) -> String {
    let kind = agent.session_kind();
    let dir = dir
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let dir: String = dir
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .take(24)
        .collect();
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    let suffix = hasher.finish() & 0xffff;
    if dir.is_empty() {
        format!("{SESSION_PREFIX}{kind}-{suffix:04x}")
    } else {
        format!("{SESSION_PREFIX}{kind}-{dir}-{suffix:04x}")
    }
}

/// A program and its arguments, or `None` for the user's shell
type PaneCommand = Option<(String, Vec<String>)>;

/// Quote for POSIX `sh`
pub(crate) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Turn a directory into a `sh` expression, expanding a leading `~`
fn sh_dir(dir: &str) -> String {
    let dir = dir.trim();
    if dir.is_empty() || dir == "~" {
        "\"$HOME\"".to_string()
    } else if let Some(rest) = dir.strip_prefix("~/") {
        format!("\"$HOME\"/{}", sh_quote(rest))
    } else {
        sh_quote(dir)
    }
}

/// The start of every session script: PATH, the client's machine, and
/// Zellij's config and layout in `$d`
fn session_prelude(session: &Session) -> String {
    let mut script = String::new();
    // Login shells set PATH for tools installed in the user's home
    script.push_str(PATH_EXPORT);
    // A terminal started from inside a Zellij session passes that on, and
    // Zellij would then attach from within it, which it cannot do
    script.push_str("unset ZELLIJ ZELLIJ_SESSION_NAME ZELLIJ_PANE_ID\n");
    // Which machine this terminal is on, for agents to open windows on the
    // one the user is at (`ct-mount --where`): carried by the `zellij attach`
    // below, and recorded as the session's last client
    script.push_str(&client_record(session));
    let _ = writeln!(script, "d={CONFIG_DIR}");
    script.push_str("mkdir -p \"$d\"\n");
    let _ = writeln!(
        script,
        "cat > \"$d/config.kdl\" <<'CT_EOF'\n{ZELLIJ_CONFIG}CT_EOF"
    );
    // A single borderless pane, without the tab and status bars. A session
    // created with a command (`attach --create NAME -- CMD`) ignores
    // default_layout and takes the layout named "default" from the layout
    // directory, falling back to Zellij's own, which has the bars. Zellij
    // needs absolute paths here
    script.push_str(
        "mkdir -p \"$d/layouts\"\n\
         printf 'layout {\\n    pane borderless=true\\n}\\n' > \"$d/layouts/default.kdl\"\n\
         printf 'layout_dir \"%s/layouts\"\\ndefault_layout \"default\"\\n' \"$d\" >> \"$d/config.kdl\"\n",
    );
    script
}

/// Export `CT_CLIENT`, this machine's name, and record it as the session's
/// last client and the latest
fn client_record(session: &Session) -> String {
    format!(
        "export CT_CLIENT={me}\n\
         c=\"${{XDG_STATE_HOME:-$HOME/.local/state}}/cosmic-term/clients\"\n\
         mkdir -p \"$c\" && printf '%s\\n' {me} > \"$c\"/{name} && printf '%s\\n' {me} > \"$c/latest\"\n",
        me = sh_quote(&mounts::local_name()),
        name = sh_quote(&session.name),
    )
}

/// The `sh` script that starts or attaches to a session.
///
/// Zellij's config is given on the command line, so the user's own Zellij
/// config is left alone.
fn session_script(session: &Session, launch: Option<(PaneCommand, &str)>) -> String {
    let mut script = session_prelude(session);
    let name = sh_quote(&session.name);
    match launch {
        Some((command, dir)) => {
            let mut inner = format!("cd {} || cd\n", sh_dir(dir));
            match command {
                Some((program, args)) => {
                    let mut command_line = sh_quote(&program);
                    for arg in &args {
                        command_line.push(' ');
                        command_line.push_str(&sh_quote(arg));
                    }
                    // Ctrl+Z would otherwise leave the agent stopped with
                    // nothing to resume it; see ct-supervise. Until the
                    // supervisor is on the host, the command runs directly,
                    // and Ctrl+Z is lost but does not hang
                    match supervisor_name() {
                        Some(name) => {
                            let _ = write!(
                                inner,
                                "s={CONFIG_DIR}/{name}\n\
                                 if [ -t 0 ] && [ -x \"$s\" ]; then\n\
                                 \"$s\" {command_line}\n\
                                 else\n\
                                 {command_line}\n\
                                 fi"
                            );
                        }
                        None => inner.push_str(&command_line),
                    }
                }
                // $SHELL is not set everywhere, for instance under Flatpak
                None => inner.push_str(
                    "shell=$(getent passwd \"$(id -un)\" | cut -d: -f7)\n\
                     \"${shell:-/bin/sh}\" -l",
                ),
            }
            // Without Zellij the tab still works, but cannot be reattached
            let _ = writeln!(
                script,
                "command -v zellij >/dev/null 2>&1 || {{ echo 'cosmic-term: no Zellij here, so this tab ends when closed' >&2; exec sh -c {}; }}",
                sh_quote(&inner)
            );
            // --close-on-exit alone leaves an empty session behind when the
            // command exits while no client is attached
            // This session no longer needs the machines' mounts
            inner.push('\n');
            inner.push_str(&mounts::collect_later());
            inner.push_str("zellij kill-session \"$ZELLIJ_SESSION_NAME\"\n");
            let _ = writeln!(
                script,
                "exec zellij --config \"$d/config.kdl\" attach --create --close-on-exit {name} -- sh -c {}",
                sh_quote(&inner)
            );
        }
        // The user's own sessions keep their own config
        None if !session.is_managed() => {
            let _ = writeln!(script, "exec zellij attach {name}");
        }
        None => {
            let _ = writeln!(
                script,
                "exec zellij --config \"$d/config.kdl\" attach {name}"
            );
        }
    }
    script
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Wrap a `sh` script so that any remote login shell (fish, bash, zsh…) runs
/// it unchanged: the only characters it sees are inside single quotes and
/// none of them are special there
fn remote_command(script: &str) -> String {
    format!(
        "sh -c 'eval \"$(echo {} | base64 -d)\"'",
        base64(script.as_bytes())
    )
}

/// ssh options shared by every connection to a host, so later tabs and
/// session listings reuse the first connection
fn ssh_options() -> Vec<String> {
    let control_path = dirs_runtime().join("cosmic-term-ssh-%C");
    vec![
        "-o".into(),
        "ControlMaster=auto".into(),
        "-o".into(),
        format!("ControlPath={}", control_path.display()),
        "-o".into(),
        "ControlPersist=10m".into(),
        // Notice a dead connection within about 15 seconds
        "-o".into(),
        "ServerAliveInterval=5".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
    ]
}

fn dirs_runtime() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(Into::into)
        .unwrap_or_else(std::env::temp_dir)
}

/// The tab command that runs a session script on the session's host
fn session_shell(session: &Session, launch: Option<(PaneCommand, &str)>) -> HostCommand {
    let script = session_script(session, launch);
    if session.is_local() {
        return HostCommand::new("sh", vec!["-c".into(), script]);
    }
    let mut args = ssh_options();
    args.push("-t".into());
    args.push(session.host.clone());
    args.push(remote_command(&script));
    HostCommand::new("ssh", args)
}

/// A command running `script` on `host`, or on this computer if it is empty,
/// without a terminal
pub(crate) fn session_exec(host: &str, script: &str) -> tokio::process::Command {
    let script = format!("{PATH_EXPORT}{script}");
    if host.is_empty() {
        let mut command = host::command("sh");
        command.args(["-c", &script]);
        return command;
    }
    let mut command = host::command("ssh");
    command
        .args(ssh_options())
        .args(["-o", "BatchMode=yes", "-T", host])
        .arg(remote_command(&script));
    command
}

/// Whether a host accepts ssh connections again. A fresh connection is made,
/// since the shared one may be the one that died
pub async fn probe_host(host: String) -> bool {
    let mut command = host::command("ssh");
    command
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5"])
        .args(["-o", "ControlMaster=no", "-o", "ControlPath=none", "-T"])
        .arg(&host)
        .arg("true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    matches!(
        tokio::time::timeout(Duration::from_secs(15), command.status()).await,
        Ok(Ok(status)) if status.success()
    )
}

/// Tailscale's status and the ssh config on this computer, for finding
/// machines to add. Either may be empty
pub async fn discovery_sources() -> Result<(String, String), String> {
    const SPLIT: &str = "--cosmic-term-ssh-config--";
    let script = format!(
        "tailscale status --json 2>/dev/null\necho {SPLIT}\ncat \"$HOME/.ssh/config\" 2>/dev/null\ntrue\n"
    );
    let output = run(session_exec("", &script)).await?;
    let (tailscale, ssh) = output.split_once(SPLIT).unwrap_or((&output, ""));
    Ok((tailscale.trim().to_string(), ssh.trim().to_string()))
}

/// This computer, as [`probe_machine`] sees another
pub async fn local_probe() -> Result<crate::machines::Probe, String> {
    let output = run(session_exec("", crate::machines::PROBE_SCRIPT)).await?;
    crate::machines::parse_probe(&output).ok_or_else(|| "no answer".to_string())
}

/// Log in to a machine once, over a fresh connection, and read what it is
pub async fn probe_machine(destination: String) -> Result<crate::machines::Probe, String> {
    let mut command = host::command("ssh");
    command
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"])
        .args(["-o", "ControlMaster=no", "-o", "ControlPath=none", "-T"])
        .arg(&destination)
        .arg(remote_command(crate::machines::PROBE_SCRIPT))
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| format!("{destination} did not answer"))?
        .map_err(|err| format!("ssh: {err}"))?;
    if !output.status.success() {
        return Err(crate::machines::explain_ssh_error(
            &destination,
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    crate::machines::parse_probe(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| format!("{destination} answered, but not as expected"))
}


/// than the remote command ending
pub const SSH_CONNECTION_LOST: i32 = 255;

/// A session found on a host
#[derive(Clone, Debug)]
pub struct ListedSession {
    pub session: Session,
    /// The session has ended and would be resurrected on attach
    pub exited: bool,
}

/// List the Zellij sessions on a host, or on this computer if it is empty
pub async fn list_sessions(host: String) -> Result<Vec<ListedSession>, String> {
    let command = session_exec(&host, "zellij list-sessions -n 2>/dev/null || true\n");
    let output = run(command).await?;
    Ok(parse_sessions(&host, &output))
}

fn parse_sessions(host: &str, output: &str) -> Vec<ListedSession> {
    output
        .lines()
        .filter_map(|line| {
            let name = line.split_whitespace().next()?;
            Some(ListedSession {
                session: Session {
                    host: host.to_string(),
                    name: name.to_string(),
                },
                exited: line.contains("(EXITED"),
            })
        })
        .collect()
}

/// Rename a session's conversation by typing the agent's own command into
/// it: Claude's `/rename [name]`, which picks a name itself when given none,
/// or Codex's `/rename`, which then asks for the name
pub async fn rename_conversation(
    session: Session,
    agent: Agent,
    name: Option<String>,
) -> Result<(), String> {
    let name = name.map(|n| n.split_whitespace().collect::<Vec<_>>().join(" "));
    let target = format!("zellij --session {} action", sh_quote(&session.name));
    let write = |text: &str| format!("{target} write-chars -p terminal_0 {}
", sh_quote(text));
    let enter = format!("{target} write -p terminal_0 13
");
    let script = match (agent, name) {
        (Agent::Claude, Some(name)) => format!("{}{enter}", write(&format!("/rename {name}"))),
        (Agent::Claude, None) => format!("{}{enter}", write("/rename")),
        // The command opens a prompt for the name, which takes a moment.
        // Codex takes keys that come in a burst for a paste, in which Enter
        // is a new line, so Enter follows the name after a pause
        (Agent::Codex, Some(name)) => format!(
            "{}{enter}sleep 1\n{}sleep 0.5\n{enter}",
            write("/rename"),
            write(&name)
        ),
        _ => return Err("this agent cannot name a conversation by itself".to_string()),
    };
    // The agents retitle the terminal only when they redraw, as when the
    // tab is clicked away from and back to: a focus out and in, as the
    // terminal reports them, has the same effect in every window showing it
    let refocus = format!("sleep 1\n{target} write -p terminal_0 27 91 79 27 91 73\n");
    run(session_exec(&session.host, &(script + &refocus))).await.map(|_| ())
}

/// End a session
pub async fn kill_session(session: Session) -> Result<(), String> {
    let script = format!(
        "zellij kill-session {name}; zellij delete-session --force {name}\n{}",
        mounts::collect_later(),
        name = sh_quote(&session.name)
    );
    run(session_exec(&session.host, &script)).await.map(|_| ())
}

/// A new session for a thread of `agent` working in `dir`, on `host`
pub fn new_thread_session(agent: Agent, dir: &str, host: &str) -> Session {
    // The terminal's own naming, with the thread's prefix and a longer
    // suffix, as a thread's directory outlives its session
    let name = new_session_name(agent, dir);
    let rest = name.strip_prefix(SESSION_PREFIX).unwrap_or(&name);
    let rest = rest.rsplit_once('-').map_or(rest, |(rest, _)| rest);
    Session {
        host: host.trim().to_string(),
        name: format!("{THREAD_PREFIX}{rest}-{}", cosmic_term::random_hex(8)),
    }
}

/// A thread's relay directory, as a `sh` expression
fn thread_dir(session: &Session) -> String {
    format!("\"{THREADS_DIR}\"/{}", sh_quote(&session.name))
}

/// Whether this program can keep threads' CLIs in relays; without the
/// supervisor, a thread runs its CLI directly
pub fn relay_available() -> bool {
    supervisor_name().is_some()
}

/// The `sh` script that starts a thread's relay and connects to it,
/// installed beforehand with [`ensure_helper`] `(…, Helper::Supervise)`.
/// The relay runs `launch` (a `sh` script) with the same session setup as a
/// terminal's agent: in a Zellij session of its own when Zellij is there,
/// else alone. `done` is the text of the CLI's line that ends a turn; `meta`
/// is written as the thread's `meta.json`, and its `conversation`, if any,
/// is claimed for this relay alone. The connection's output is the relay
/// protocol's (see `cosmic_term::relay`), from the start of the log.
/// `None` without the supervisor
pub fn thread_start_script(
    session: &Session,
    launch: &str,
    done: &str,
    linger: Duration,
    meta: &serde_json::Value,
) -> Option<String> {
    Some(start_script(supervisor_name()?, session, launch, done, linger, meta))
}

/// [`thread_start_script`], with the supervisor's file name
fn start_script(
    supervisor: &str,
    session: &Session,
    launch: &str,
    done: &str,
    linger: Duration,
    meta: &serde_json::Value,
) -> String {
    let name = sh_quote(&session.name);
    let mut script = session_prelude(session);
    let _ = write!(
        script,
        "s=\"$d\"/{supervisor}\n\
         t={dir}\n\
         [ -x \"$s\" ] || {{ echo 'cosmic-term: the thread supervisor is not installed' >&2; exit 127; }}\n\
         mkdir -p \"$t\" && chmod 700 \"$t\" || exit 1\n\
         printf '%s\\n' \"$s\" > \"$t/relay-bin\"\n\
         printf '%s\\n' {meta} > \"$t/meta.json\"\n\
         printf '%s' {launch} > \"$t/launch.sh\"\n",
        dir = thread_dir(session),
        meta = sh_quote(&meta.to_string()),
        launch = sh_quote(launch),
    );
    // The relay's own script, given the thread's directory and the
    // supervisor; in Zellij, the session ends with it
    let mut relay = format!(
        "t=$1 s=$2\n\
         \"$s\" --relay \"$t\" --done {done} --linger {linger}",
        done = sh_quote(done),
        linger = linger.as_secs(),
    );
    if let Some(id) = meta.get("conversation").and_then(serde_json::Value::as_str) {
        relay.push_str(" --conversation ");
        relay.push_str(&sh_quote(id));
    }
    relay.push_str(" -- sh \"$t/launch.sh\" 2>>\"$t/relay.err\"\n");
    relay.push_str(&mounts::collect_later());
    relay.push_str("[ -n \"${ZELLIJ:-}\" ] && zellij kill-session \"$ZELLIJ_SESSION_NAME\"\nexit 0\n");
    let _ = write!(
        script,
        "r={relay}\n\
         if command -v zellij >/dev/null 2>&1; then\n\
         zellij --config \"$d/config.kdl\" attach --create-background {name} -- sh -c \"$r\" sh \"$t\" \"$s\" </dev/null >/dev/null 2>&1\n\
         i=0\n\
         while [ ! -e \"$t/state\" ] && [ \"$i\" -lt 30 ]; do sleep 0.1; i=$((i + 1)); done\n\
         fi\n\
         if [ ! -e \"$t/state\" ]; then\n\
         export ZELLIJ_SESSION_NAME={name}\n\
         if command -v setsid >/dev/null 2>&1; then\n\
         setsid -f sh -c \"$r\" sh \"$t\" \"$s\" </dev/null >/dev/null 2>&1\n\
         else\n\
         nohup sh -c \"$r\" sh \"$t\" \"$s\" </dev/null >/dev/null 2>&1 &\n\
         fi\n\
         fi\n\
         exec \"$s\" --connect \"$t\" --wait 10 --from 0 --client \"$CT_CLIENT\"\n",
        relay = sh_quote(&relay),
    );
    script
}

/// The `sh` script that connects to a thread's relay, reading its log from
/// `from`; once the relay has gone, it gives the rest of the log and how
/// the CLI ended, or `#ct lost`
pub fn thread_attach_script(session: &Session, from: cosmic_term::relay::Cursor) -> String {
    format!(
        "{client}t={dir}\n\
         s=$(cat \"$t/relay-bin\" 2>/dev/null)\n\
         if [ -z \"$s\" ] || [ ! -x \"$s\" ]; then printf '#ct hello 1 pid=0 head=0 turn=0 end=0 busy=0\\n#ct lost\\n'; exit 0; fi\n\
         exec \"$s\" --connect \"$t\" --wait 2 --from {from} --client \"$CT_CLIENT\"\n",
        client = client_record(session),
        dir = thread_dir(session),
    )
}

/// End a thread's CLI, as gently as it will go, and its relay
pub async fn thread_stop(session: Session) -> Result<(), String> {
    run(session_exec(&session.host, &thread_stop_script(&session))).await.map(|_| ())
}

fn thread_stop_script(session: &Session) -> String {
    format!(
        "t={dir}\n\
         s=$(cat \"$t/relay-bin\" 2>/dev/null)\n\
         [ -n \"$s\" ] && [ -x \"$s\" ] && \"$s\" --connect \"$t\" --stop\n\
         zellij kill-session {name} >/dev/null 2>&1\n\
         zellij delete-session --force {name} >/dev/null 2>&1\n\
         true\n",
        dir = thread_dir(session),
        name = sh_quote(&session.name),
    )
}

/// Where a tab's processes run
#[derive(Clone, Debug)]
pub enum TabProcess {
    /// Directly on this computer, tagged with this [`host::TAB_VAR`] token
    Local(String),
    Session(Session),
}

/// The project directories of a tab, and the host they are on if remote:
/// see `project-dirs.py`
async fn project_dirs(process: &TabProcess) -> Result<(Option<String>, Vec<String>), String> {
    let (host, output) = match process {
        TabProcess::Local(token) => {
            let mut command = host::command("python3");
            command.arg("-c").arg(PROJECT_DIRS).args(["env", token]);
            (None, run(command).await?)
        }
        TabProcess::Session(session) => {
            let script = format!(
                "python3 -c {} zellij {}\n",
                sh_quote(PROJECT_DIRS),
                sh_quote(&session.name)
            );
            let host = (!session.is_local()).then(|| session.host.clone());
            (host, run(session_exec(&session.host, &script)).await?)
        }
    };
    let dirs: Vec<String> = output
        .lines()
        .filter(|l| l.starts_with('/'))
        .map(str::to_string)
        .collect();
    if dirs.is_empty() {
        return Err("no project directory found".to_string());
    }
    Ok((host, dirs))
}

/// Open the project directories of a tab in the file manager, through the
/// host's sshfs mount if it is remote
pub async fn open_in_files(process: TabProcess) -> Result<(), String> {
    let (host, dirs) = project_dirs(&process).await?;
    let mount = match &host {
        Some(host) => Some(ensure_mount(host.clone()).await?),
        None => None,
    };
    for dir in dirs {
        let local = match &mount {
            Some(mount) => mount.join(dir.trim_start_matches('/')),
            None => std::path::PathBuf::from(&dir),
        };
        log::info!("opening {} in the file manager", local.display());
        let mut command = host::command("xdg-open");
        command.arg(&local);
        run(command).await?;
    }
    Ok(())
}

/// Open the project directories of a tab in Zed
pub async fn open_in_zed(process: TabProcess) -> Result<(), String> {
    let (host, dirs) = project_dirs(&process).await?;

    // Desktop sessions often lack ~/.local/bin, where Zed's installer puts
    // the CLI, on PATH; some distributions call it zeditor, and Flathub's
    // Zed is a Flatpak
    let mut command = host::command("sh");
    command.args([
        "-c",
        "PATH=\"$PATH:$HOME/.local/bin\"\n\
         for zed in zed zeditor; do command -v \"$zed\" >/dev/null 2>&1 && exec \"$zed\" \"$@\"; done\n\
         flatpak info dev.zed.Zed >/dev/null 2>&1 && exec setsid -f flatpak run dev.zed.Zed \"$@\"\n\
         echo 'Zed is not installed' >&2; exit 127\n",
        "zed",
        // A project gets its own window, whatever the user's default
        "--new",
    ]);
    for dir in dirs {
        match &host {
            // Zed merges URLs for the same host into one remote project
            Some(host) => command.arg(format!("ssh://{host}{dir}")),
            None => command.arg(dir),
        };
    }
    log::info!("opening in Zed: {:?}", command.as_std());
    run(command).await.map(|_| ())
}

/// A coding agent running in a tab, as `project-dirs.py --json` finds it
#[derive(Clone, Debug, serde::Deserialize)]
pub struct TabAgent {
    /// `claude` or `codex`
    pub kind: String,
    pub pid: u32,
    /// Its conversation, if found
    #[serde(default)]
    pub conversation: Option<String>,
    /// The conversation is only the newest one begun where the agent started
    #[serde(default)]
    pub guessed: bool,
    #[serde(default)]
    pub started_in: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// The projects it has been working in lately, most used first
    #[serde(default)]
    pub focus: Vec<String>,
    #[serde(default)]
    pub status: String,
}

impl TabAgent {
    /// Where the agent is working: its main project lately, else its directory
    pub fn working_directory(&self) -> Option<&str> {
        self.focus
            .first()
            .or(self.cwd.as_ref())
            .or(self.started_in.as_ref())
            .map(String::as_str)
    }

    pub fn agent(&self) -> Option<Agent> {
        match self.kind.as_str() {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }
}

/// What runs in a tab
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct TabAgents {
    /// The deepest process's directory
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agents: Vec<TabAgent>,
}

/// The coding agents running in a tab, with their conversations and where
/// they are working
pub async fn tab_agents(process: TabProcess) -> Result<TabAgents, String> {
    let output = match &process {
        TabProcess::Local(token) => {
            let mut command = host::command("python3");
            command.arg("-c").arg(PROJECT_DIRS).args(["env", token, "--json"]);
            run(command).await?
        }
        TabProcess::Session(session) => {
            let script = format!(
                "python3 -c {} zellij {} --json\n",
                sh_quote(PROJECT_DIRS),
                sh_quote(&session.name)
            );
            run(session_exec(&session.host, &script)).await?
        }
    };
    serde_json::from_str(&output).map_err(|err| err.to_string())
}

/// A running session this terminal made, as the sidebar shows it
#[derive(Clone, Debug, serde::Deserialize)]
pub struct SessionSummary {
    pub name: String,
    /// Windows attached to it, here or elsewhere
    #[serde(default)]
    pub clients: usize,
    /// Where it works: its agent's project, else its directory
    #[serde(default)]
    pub cwd: Option<String>,
    /// The main worktree of the repository it works in
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// A terminal's foreground command, unless it is at its shell
    #[serde(default)]
    pub command: Option<String>,
    /// When that command started, in seconds since the epoch
    #[serde(default)]
    pub since: Option<f64>,
    /// A terminal's last lines on screen
    #[serde(default)]
    pub screen: Vec<String>,
    #[serde(default)]
    pub agents: Vec<AgentSummary>,
    /// For a thread's relay: see [`Session::is_thread`]
    #[serde(default)]
    pub thread: Option<ThreadSummary>,
}

/// A thread's live relay, as the sidebar shows it
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ThreadSummary {
    /// `claude` or `codex`
    #[serde(default)]
    pub agent: String,
    /// A turn is in progress
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub conversation: Option<String>,
    /// The thread's `meta.json`, as written when it started and since
    #[serde(default)]
    pub meta: serde_json::Value,
    /// The relay's log: where the latest turn began, and its end
    #[serde(default)]
    pub turn_start: u64,
    #[serde(default)]
    pub end: u64,
    /// When it started, in seconds since the epoch
    #[serde(default)]
    pub created: f64,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct AgentSummary {
    /// `claude` or `codex`
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub conversation: Option<String>,
    #[serde(default)]
    pub title: String,
    /// `working` or `idle`
    #[serde(default)]
    pub state: String,
    /// Its latest message
    #[serde(default)]
    pub last: String,
    /// When its conversation last changed, in seconds since the epoch
    #[serde(default)]
    pub updated: f64,
}

impl SessionSummary {
    pub fn working(&self) -> bool {
        self.agents.iter().any(|agent| agent.state == "working")
    }

    /// When it last did something, if known
    pub fn active_at(&self) -> Option<f64> {
        self.agents
            .iter()
            .map(|agent| agent.updated)
            .filter(|&t| t > 0.0)
            .reduce(f64::max)
            .or(self.since)
    }
}

/// The running sessions this terminal made on a host, with what they are doing
pub async fn session_summaries(host: String) -> Result<Vec<SessionSummary>, String> {
    // A scan is also the time to let go of mounts nothing needs any more
    let script = format!(
        "python3 -c {} sessions\n[ -x {cmd} ] && setsid -f {cmd} --gc >/dev/null 2>&1 </dev/null\ntrue\n",
        sh_quote(PROJECT_DIRS),
        cmd = mounts::COMMAND
    );
    let output = run(session_exec(&host, &script)).await?;
    serde_json::from_str(&output).map_err(|err| err.to_string())
}

/// A program the terminal copies to every host it works with
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Helper {
    /// `ct-supervise`, which runs a session's command
    Supervise,
    /// `ct-transfer`, which does the steps of a session transfer
    Transfer,
    /// The PAM module that lets sudo take approvals from the terminal,
    /// installed into PAM by `ct-sudo-setup`
    PamModule,
    /// `ct-sudo-agent`, which answers it; only on this computer
    SudoAgent,
    /// Zellij, for machines without their own
    Zellij,
    /// `ct-chartgen`, with which agents in threads check their charts
    Chartgen,
}

impl Helper {
    /// The helpers every host gets
    pub const ALL: [Self; 5] = [Self::Supervise, Self::Transfer, Self::PamModule, Self::Zellij, Self::Chartgen];

    fn program(self) -> &'static str {
        match self {
            Self::Supervise => "ct-supervise",
            Self::Transfer => "ct-transfer",
            Self::PamModule => "pam_cosmic_term_sudo.so",
            Self::SudoAgent => "ct-sudo-agent",
            Self::Zellij => "zellij",
            Self::Chartgen => "ct-chartgen",
        }
    }

    /// Other names the program may have beside this one, as in a Cargo
    /// build directory
    fn other_name(self) -> Option<&'static str> {
        match self {
            Self::PamModule => Some("libpam_cosmic_term_sudo.so"),
            _ => None,
        }
    }

    /// The variable naming the program's file instead of the one beside
    /// this program
    fn variable(self) -> &'static str {
        match self {
            Self::Supervise => "CT_SUPERVISE",
            Self::Transfer => "CT_TRANSFER",
            Self::PamModule => "CT_PAM_MODULE",
            Self::SudoAgent => "CT_SUDO_AGENT",
            Self::Zellij => "CT_ZELLIJ",
            Self::Chartgen => "CT_CHARTGEN",
        }
    }
}

/// A helper's file name on hosts, and its contents
type Installable = Option<(String, Vec<u8>)>;

/// A helper's file name on hosts and its contents, if this program has it
fn helper(helper: Helper) -> Option<&'static (String, Vec<u8>)> {
    static HELPERS: [std::sync::OnceLock<Installable>; 6] = [
        std::sync::OnceLock::new(),
        std::sync::OnceLock::new(),
        std::sync::OnceLock::new(),
        std::sync::OnceLock::new(),
        std::sync::OnceLock::new(),
        std::sync::OnceLock::new(),
    ];
    HELPERS[helper as usize]
        .get_or_init(|| {
            let bytes = match std::env::var_os(helper.variable()) {
                Some(path) => std::fs::read(path).ok()?,
                None => {
                    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
                    std::iter::once(helper.program())
                        .chain(helper.other_name())
                        .find_map(|name| std::fs::read(dir.join(name)).ok())?
                }
            };
            // Named by its contents, so that each build installs its own
            let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
            });
            Some((format!("{}-{hash:016x}", helper.program()), bytes))
        })
        .as_ref()
}

/// A helper's file name on hosts, if this program has it
pub fn helper_name(which: Helper) -> Option<&'static str> {
    helper(which).map(|(name, _)| name.as_str())
}

/// The supervisor's file name on hosts, if this program has one
fn supervisor_name() -> Option<&'static str> {
    helper(Helper::Supervise).map(|(name, _)| name.as_str())
}

/// Install the helpers on a host, or on this computer if `host` is empty,
/// unless they are already there, and settle the programs it cannot bring;
/// gives those still missing
pub async fn ensure_helpers(host: String) -> Result<Vec<String>, String> {
    ensure_helpers_progress(host, |_| {}).await
}

/// Report which installation step is currently running. `None` means the
/// initial tool check; each helper is reported before its network call.
pub async fn ensure_helpers_progress(
    host: String,
    progress: impl Fn(Option<Helper>),
) -> Result<Vec<String>, String> {
    progress(None);
    let missing = settle_tools(&host).await?;
    for which in Helper::ALL {
        progress(Some(which));
        ensure_helper(host.clone(), which).await?;
    }
    Ok(missing)
}

/// Check remote-file tools on the actual host, not inside the Flatpak.
pub async fn mount_setup(host: String) -> Result<cosmic_term::mount_setup::Status, String> {
    let output = run(session_exec(&host, cosmic_term::mount_setup::PROBE)).await
        .map_err(|error| if host.is_empty() { error } else { crate::machines::explain_ssh_error(&host, &error) })?;
    cosmic_term::mount_setup::Status::parse(&output)
}

/// The programs the terminal needs on a machine but cannot bring, each as
/// the names it goes by
pub const TOOLS: [&str; 8] = [
    "python3",
    "git",
    "sshfs",
    "fusermount3 fusermount",
    "flock",
    "setsid",
    "mountpoint",
    "timeout",
];

/// Make each of [`TOOLS`] found on every script's PATH: one the scripts'
/// PATH misses but the user's login shell finds is linked into `bin` of the
/// terminal's directory, first on that PATH. Gives the first name of each
/// still missing
async fn settle_tools(host: &str) -> Result<Vec<String>, String> {
    let tools = TOOLS.map(sh_quote).join(" ");
    let script = format!(
        "d={CONFIG_DIR}\n\
         mkdir -p \"$d/bin\"\n\
         shell=$(getent passwd \"$(id -un)\" 2>/dev/null | cut -d: -f7)\n\
         for names in {tools}; do\n\
           found=\n\
           for name in $names; do\n\
             [ -L \"$d/bin/$name\" ] && [ ! -x \"$d/bin/$name\" ] && rm -f \"$d/bin/$name\"\n\
             command -v \"$name\" >/dev/null 2>&1 && {{ found=1; break; }}\n\
           done\n\
           [ -n \"$found\" ] && continue\n\
           for name in $names; do\n\
             own=$(\"${{shell:-/bin/sh}}\" -lc \"command -v $name\" </dev/null 2>/dev/null | tail -n 1)\n\
             case \"$own\" in /*) [ -x \"$own\" ] && ln -sfn \"$own\" \"$d/bin/$name\" && {{ found=1; break; }} ;; esac\n\
           done\n\
           [ -n \"$found\" ] || echo \"${{names%% *}}\"\n\
         done\n"
    );
    let mut command = session_exec(host, &script);
    command.stdin(Stdio::null());
    let missing = tokio::time::timeout(Duration::from_secs(60), run(command))
        .await
        .map_err(|_| "timed out finding programs".to_string())??;
    Ok(missing.lines().map(str::to_string).filter(|l| !l.is_empty()).collect())
}

/// Settle which Zellij a host's sessions use, as `bin/zellij` in the
/// terminal's directory there, first on every script's PATH, so that every
/// caller gets the same one whatever its own PATH: the one the user's login
/// shell finds, else one on the scripts' PATH. Once settled it stays, as a
/// client attaches only to a server of its own version. False when the host
/// has none, and needs the one this terminal brings
async fn zellij_resolved(host: &str) -> Result<bool, String> {
    let script = format!(
        "d={CONFIG_DIR}\n\
         link=\"$d/bin/zellij\"\n\
         [ -x \"$link\" ] && exit 0\n\
         shell=$(getent passwd \"$(id -un)\" | cut -d: -f7)\n\
         own=$(\"${{shell:-/bin/sh}}\" -lc 'command -v zellij' </dev/null 2>/dev/null | tail -n 1)\n\
         case \"$own\" in /*) ;; *) own=$(command -v zellij) ;; esac\n\
         case \"$own\" in \"$link\" | '') exit 3 ;; esac\n\
         [ -x \"$own\" ] || exit 3\n\
         mkdir -p \"$d/bin\" && ln -sfn \"$own\" \"$link\"\n"
    );
    let mut command = session_exec(host, &script);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| "timed out finding Zellij".to_string())?
        .map_err(|err| err.to_string())?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(3) => Ok(false),
        _ => Err(format!(
            "finding Zellij: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

pub async fn ensure_helper(host: String, which: Helper) -> Result<(), String> {
    if which == Helper::Zellij && zellij_resolved(&host).await? {
        return Ok(());
    }
    let Some((name, bytes)) = tokio::task::spawn_blocking(move || helper(which))
        .await
        .map_err(|err| format!("loading {which:?}: {err}"))?
    else {
        return Ok(());
    };
    let script = if which == Helper::Zellij {
        // The machine has none: bring this one
        format!(
            "d={CONFIG_DIR}\n\
             mkdir -p \"$d/bin\" && cat > \"$d\"/{name}.tmp && chmod 755 \"$d\"/{name}.tmp \
             && mv \"$d\"/{name}.tmp \"$d\"/{name} && ln -sfn ../{name} \"$d/bin/zellij\"\n"
        )
    } else if which == Helper::Chartgen {
        // Under its own name too, which agents are told
        format!(
            "d={CONFIG_DIR}\n\
             [ -x \"$d\"/{name} ] || {{ mkdir -p \"$d\" && cat > \"$d\"/{name}.tmp && chmod 755 \"$d\"/{name}.tmp \
             && mv \"$d\"/{name}.tmp \"$d\"/{name}; }} && ln -sfn {name} \"$d\"/ct-chartgen\n"
        )
    } else {
        format!(
            "d={CONFIG_DIR}\n\
             [ -x \"$d\"/{name} ] && exit 0\n\
             mkdir -p \"$d\" && cat > \"$d\"/{name}.tmp && chmod 755 \"$d\"/{name}.tmp \
             && mv \"$d\"/{name}.tmp \"$d\"/{name}\n"
        )
    };
    let mut command = session_exec(&host, &script);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Zellij is some 50 MB, which a slow link takes a while to carry
    let wait = Duration::from_secs(if which == Helper::Zellij { 600 } else { 60 });
    let output = tokio::time::timeout(wait, async {
        let mut child = command.spawn().map_err(|err| err.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            // A host that already has it exits without reading
            let _ = stdin.write_all(bytes).await;
        }
        child.wait_with_output().await.map_err(|err| err.to_string())
    })
        .await
        .map_err(|_| format!("installing {which:?} timed out after {} seconds", wait.as_secs()))??;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// A computer a transfer works on, reached through its installed
/// `ct-transfer`: this one, or an ssh destination
#[derive(Clone, Debug)]
pub struct HostMachine {
    /// An ssh destination, or empty for this computer
    pub host: String,
}

impl cosmic_term::transfer::Machine for HostMachine {
    fn call(
        &self,
        request: cosmic_term::helper::Request,
    ) -> Result<cosmic_term::helper::Reply, String> {
        use tokio::io::AsyncWriteExt as _;
        let (name, _) = helper(Helper::Transfer)
            .ok_or("ct-transfer was not found beside the terminal")?;
        let input = serde_json::to_vec(&request).map_err(|err| err.to_string())?;
        // The coordinator runs on a dedicated blocking thread. Give the
        // complete request (including stdin upload) a deadline, so a stuck
        // sshfs stat or git operation cannot strand the launcher forever.
        let limit = match request {
            cosmic_term::helper::Request::Repositories { .. } => std::time::Duration::from_secs(20),
            cosmic_term::helper::Request::Fetch { .. }
            | cosmic_term::helper::Request::Push { .. } => std::time::Duration::from_secs(600),
            _ => std::time::Duration::from_secs(120),
        };
        // Bound the helper on its own host too. Otherwise killing the local
        // ssh client on timeout can leave a remote Git operation running.
        // The local watchdog below still applies if `timeout` is unavailable.
        let soft_limit = limit.as_secs().saturating_sub(5);
        let script = format!(
            "if command -v timeout >/dev/null 2>&1; then exec timeout -k 5s {soft_limit}s {CONFIG_DIR}/{name}; else exec {CONFIG_DIR}/{name}; fi\n"
        );
        let mut command = session_exec_std(&self.host, &script);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("ct-transfer runtime: {err}"))?;
        let output = runtime.block_on(async {
            tokio::time::timeout(limit, async {
                let mut command = tokio::process::Command::from(command);
                command.kill_on_drop(true);
                let mut child = command.spawn().map_err(|err| format!("ct-transfer: {err}"))?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(&input).await.map_err(|err| format!("ct-transfer: {err}"))?;
                }
                child.wait_with_output().await.map_err(|err| format!("ct-transfer: {err}"))
            })
            .await
            .map_err(|_| format!("ct-transfer timed out after {} seconds", limit.as_secs()))?
        })?;
        let where_ = if self.host.is_empty() { "this computer" } else { self.host.as_str() };
        if output.status.code() == Some(SSH_CONNECTION_LOST) && !self.host.is_empty() {
            return Err(format!("lost the connection to {where_}"));
        }
        if matches!(output.status.code(), Some(124 | 137 | 143)) {
            return Err(format!("ct-transfer on {where_} timed out after {soft_limit} seconds"));
        }
        serde_json::from_slice(&output.stdout).map_err(|_| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!(
                "ct-transfer on {where_} failed: {}",
                stderr.trim().lines().last().unwrap_or("no reply")
            )
        })
    }

    fn git_url(&self, path: &std::path::Path) -> String {
        if self.host.is_empty() {
            path.display().to_string()
        } else {
            format!("{}:{}", self.host, path.display())
        }
    }

    fn ssh_command(&self) -> Option<String> {
        if self.host.is_empty() {
            return None;
        }
        let mut command = "ssh".to_string();
        for option in ssh_options().iter().map(String::as_str).chain(["-o", "BatchMode=yes"]) {
            command.push(' ');
            command.push_str(&sh_quote(option));
        }
        Some(command)
    }

    fn is_local(&self) -> bool {
        self.host.is_empty()
    }
}

/// Like [`session_exec`], for code that blocks
pub(crate) fn session_exec_std(host: &str, script: &str) -> std::process::Command {
    let script = format!("{PATH_EXPORT}{script}");
    if host.is_empty() {
        let mut command = host::std_command("sh");
        command.args(["-c", &script]);
        return command;
    }
    let mut command = host::std_command("ssh");
    command
        .args(ssh_options())
        .args(["-o", "BatchMode=yes", "-T", host])
        .arg(remote_command(&script));
    command
}

/// Start a profile's agent in `dir`, as a transfer or handoff says
pub fn launch_start(
    profile: &Profile,
    dir: &str,
    start: &cosmic_term::transfer::Start,
) -> (HostCommand, Session) {
    use cosmic_term::transfer::Start;
    let mut args = Vec::new();
    match start {
        Start::Resume { id, note } => {
            args.extend(resume_args(profile.agent, Some(id)));
            if profile.agent == Agent::Claude && !note.is_empty() {
                args.extend(["--append-system-prompt".to_string(), note.clone()]);
            }
            args.extend(shlex::split(&profile.args).unwrap_or_default());
        }
        Start::Seeded { prompt } => {
            args.extend(shlex::split(&profile.args).unwrap_or_default());
            // The first message, after any options
            args.push(prompt.clone());
        }
    }
    let session = Session {
        name: new_session_name(profile.agent, dir),
        host: profile.host.trim().to_string(),
    };
    let command = profile
        .agent
        .program()
        .map(|program| (program.to_string(), args));
    (session_shell(&session, Some((command, dir))), session)
}

/// A directory an agent has worked in
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Project {
    pub path: String,
    /// Seconds since the epoch, or 0 if unknown
    #[serde(default)]
    pub last_used: f64,
}

/// A past conversation of an agent, which can be resumed
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Conversation {
    pub id: String,
    pub cwd: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub last_used: f64,
}

/// One agent's projects and conversations on a host
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct AgentListing {
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub conversations: Vec<Conversation>,
}

/// What a launcher offers from a host: both agents' projects and
/// conversations
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct HostListing {
    pub home: String,
    #[serde(default)]
    pub hostname: String,
    /// The host's id in its project registry, or empty if it has none yet
    #[serde(default)]
    pub host_id: String,
    #[serde(default)]
    pub agents: std::collections::BTreeMap<String, AgentListing>,
}

impl HostListing {
    /// What tells this computer apart from others, whatever ssh name it
    /// was reached by
    pub fn identity(&self) -> &str {
        if self.host_id.is_empty() { &self.hostname } else { &self.host_id }
    }

    pub fn agent(&self, agent: Agent) -> Option<&AgentListing> {
        self.agents.get(agent.program()?)
    }
}

/// Find both agents' recent projects and conversations on a host
pub async fn launcher_data(host: String) -> Result<HostListing, String> {
    let script = format!("python3 -c {} all\n", sh_quote(LAUNCHER));
    let output = run(session_exec(&host, &script)).await?;
    serde_json::from_str(&output).map_err(|err| err.to_string())
}

/// Where a host's root is mounted with sshfs, mounting it first if needed
pub async fn ensure_mount(host: String) -> Result<std::path::PathBuf, String> {
    mounts::mount(host).await
}

/// Turn a link or a word under the pointer into a path: a `file:` URL's
/// path, or a path-like word without a trailing `:line:column`. `None` if it
/// does not look like a path
pub fn link_path(link: &str) -> Option<String> {
    let link = link.trim();
    if let Some(rest) = link.strip_prefix("file:") {
        // file:///path, or file://host/path from programs naming their host
        let rest = match rest.strip_prefix("//") {
            Some(authority_and_path) => &authority_and_path[authority_and_path.find('/')?..],
            None => rest,
        };
        let path = url::Url::parse(&format!("file://{rest}"))
            .ok()?
            .to_file_path()
            .ok()?;
        return Some(path.to_string_lossy().into_owned());
    }
    if link.contains("://") {
        return None;
    }
    let mut path = link
        .trim_start_matches(['(', '[', '\'', '"', '`'])
        .trim_end_matches(['.', ',', ';', ')', ']', '\'', '"', '`']);
    // A position such as src/main.rs:42:7
    for _ in 0..2 {
        if let Some((head, tail)) = path.rsplit_once(':')
            && !tail.is_empty()
            && tail.bytes().all(|b| b.is_ascii_digit())
        {
            path = head;
        }
    }
    let path = path.trim_end_matches(':');
    let file_name = path.rsplit('/').next().unwrap_or_default();
    let looks_like_path = path.contains('/')
        || file_name
            .rsplit_once('.')
            .is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty());
    (looks_like_path && !path.is_empty()).then(|| path.to_string())
}

/// Make a path absolute, given the remote home and working directory
fn resolve_path(path: &str, home: &str, cwd: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else if path == "~" {
        home.to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{rest}", home.trim_end_matches('/'))
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), path.trim_start_matches("./"))
    }
}

/// Open a file named in a remote session's output with a local application,
/// through an sshfs mount of the remote host's root that is made when first
/// needed and kept for later
pub async fn open_locally(session: Session, link: String) -> Result<(), String> {
    let path = link_path(&link).ok_or_else(|| format!("not a path: {link}"))?;
    let path = if path.starts_with('/') {
        path
    } else {
        // Relative to the session's working directory on its host
        let script = format!(
            "echo \"$HOME\"\npython3 -c {} zellij {} --cwd\n",
            sh_quote(PROJECT_DIRS),
            sh_quote(&session.name)
        );
        let output = run(session_exec(&session.host, &script)).await?;
        let mut lines = output.lines();
        let home = lines.next().unwrap_or_default();
        let cwd = lines.next().unwrap_or(home);
        resolve_path(&path, home, cwd)
    };

    let mount = ensure_mount(session.host.clone()).await?;
    let local = mount.join(path.trim_start_matches('/'));
    if !host::path_exists(&local).await {
        return Err(format!("no such file: {path}"));
    }
    log::info!("opening {path} from {} locally", session.host);
    let mut command = host::command("xdg-open");
    command.arg(&local);
    run(command).await.map(|_| ())
}

/// Run a command to completion, returning its stdout
async fn run(mut command: tokio::process::Command) -> Result<String, String> {
    command.stdin(Stdio::null()).kill_on_drop(true);
    let program = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| format!("{program} timed out"))?
        .map_err(|err| format!("{program}: {err}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{program}: {}",
            stderr.lines().last().unwrap_or("failed")
        ))
    }
}

/// Remove the session name Zellij puts before a pane's title: `name | title`
/// in current versions, `Zellij (name) - title` in older ones
pub fn strip_zellij_title<'a>(title: &'a str, session: &str) -> &'a str {
    title
        .strip_prefix(session)
        .and_then(|rest| rest.strip_prefix(" | "))
        .or_else(|| {
            title
                .strip_prefix("Zellij (")?
                .strip_prefix(session)?
                .strip_prefix(") - ")
        })
        .unwrap_or(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settling Zellij on this computer, in a home of its own: run alone,
    /// as it changes HOME, with
    /// `cargo test --bin cosmic-term zellij_is_settled -- --ignored`
    #[test]
    #[ignore]
    fn zellij_is_settled_once_preferring_the_users_own() {
        let home = std::env::temp_dir().join(format!("ct-zellij-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let brought = home.join("brought-zellij");
        std::fs::write(&brought, "#!/bin/sh\necho brought\n").unwrap();
        std::fs::set_permissions(&brought, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        // SAFETY: this test runs alone
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("PATH", "/usr/bin:/bin");
            std::env::set_var("CT_ZELLIJ", &brought);
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let link = home.join(".config/cosmic-term-agents/bin/zellij");
        let run = |link: &std::path::Path| {
            String::from_utf8(std::process::Command::new(link).output().unwrap().stdout).unwrap()
        };

        // Without a Zellij of its own the machine gets the one brought
        assert!(!rt.block_on(zellij_resolved("")).unwrap());
        rt.block_on(ensure_helper(String::new(), Helper::Zellij)).unwrap();
        assert_eq!(run(&link), "brought\n");
        // and keeps it, even once the user installs one
        let own = home.join(".local/bin/zellij");
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        std::fs::write(&own, "#!/bin/sh\necho own\n").unwrap();
        std::fs::set_permissions(&own, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        assert!(rt.block_on(zellij_resolved("")).unwrap());
        assert_eq!(run(&link), "brought\n");

        // A machine with its own uses that one, found on the scripts' PATH
        std::fs::remove_file(&link).unwrap();
        rt.block_on(ensure_helper(String::new(), Helper::Zellij)).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), own);
        assert_eq!(run(&link), "own\n");
        // and one gone is settled again
        std::fs::remove_file(&own).unwrap();
        rt.block_on(ensure_helper(String::new(), Helper::Zellij)).unwrap();
        assert_eq!(run(&link), "brought\n");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Run a script on a host, failing the test if it fails
    fn sh(host: &str, script: &str) -> String {
        let output = session_exec_std(host, script).output().unwrap();
        assert!(output.status.success(), "{host}: {script}\n{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// A transfer's Git steps over a real ssh connection, both ways. Uses
    /// throwaway repositories under /tmp on both computers and removes them;
    /// installs ct-transfer on the host as the terminal would.
    #[test]
    #[ignore = "needs CT_TEST_HOST (an ssh destination) and CT_TRANSFER (a static ct-transfer)"]
    fn transfers_over_ssh() {
        use cosmic_term::{
            helper::{Reply, Request},
            transfer::Machine,
            workspace::{Placement, Snapshot},
        };
        use std::io::Write as _;
        let host = std::env::var("CT_TEST_HOST").expect("CT_TEST_HOST");
        let (name, bytes) = helper(Helper::Transfer).expect("CT_TRANSFER");
        for target in ["", host.as_str()] {
            let mut child = session_exec_std(
                target,
                &format!("d={CONFIG_DIR}; mkdir -p \"$d\" && cat > \"$d\"/{name}.tmp && chmod 755 \"$d\"/{name}.tmp && mv \"$d\"/{name}.tmp \"$d\"/{name}\n"),
            )
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            assert!(child.wait().unwrap().success(), "installing on {target:?}");
        }
        let local = HostMachine { host: String::new() };
        let remote = HostMachine { host: host.clone() };
        let info = |m: &HostMachine| match m.call(Request::Info).unwrap() {
            Reply::Info(info) => info,
            other => panic!("{other:?}"),
        };
        assert_ne!(info(&local).host_id, info(&remote).host_id);

        let tag = cosmic_term::random_hex(8);
        let git = "git -c user.name=t -c user.email=t@t -c commit.gpgsign=false";
        let remote_repo = sh(&host, &format!(
            "r=/tmp/ct-ssh-test-{tag}; mkdir -p $r && cd $r && git init -q -b main && echo one > a.txt && {git} add a.txt && {git} commit -q -m first \
             && git checkout -q -b feature && echo wip > a.txt && echo new > b.txt && echo $r"
        ));
        let local_dir = std::env::temp_dir().join(format!("ct-ssh-test-{tag}"));
        let local_repo = local_dir.join("repo");
        sh("", &format!("mkdir -p {0} && git init -q {0}", local_repo.display()));
        let snapshot = |m: &HostMachine, top: &str, id: &str| match m.call(Request::Snapshot { top: top.into(), id: id.into() }).unwrap() {
            Reply::Snapshot(s) => s,
            other => panic!("{other:?}"),
        };
        let done = |reply: Result<Reply, String>| assert!(matches!(reply, Ok(Reply::Done)), "{reply:?}");

        // From the host: fetched over ssh
        let out: Snapshot = snapshot(&remote, &remote_repo, "out");
        done(local.call(Request::Fetch {
            repo: local_repo.clone(),
            url: remote.git_url(std::path::Path::new(&remote_repo)),
            id: "out".into(),
            ssh: remote.ssh_command(),
        }));
        let worktree = local_dir.join("wt");
        match local.call(Request::Prepare {
            repo: local_repo.clone(),
            placement: Placement::New(worktree.clone()),
            snapshot: out.clone(),
            id: "out".into(),
        }) {
            Ok(Reply::Prepared(_)) => {}
            other => panic!("{other:?}"),
        }
        done(local.call(Request::Commit { path: worktree.clone(), snapshot: out }));
        assert_eq!(std::fs::read_to_string(worktree.join("a.txt")).unwrap(), "wip\n");
        assert_eq!(std::fs::read_to_string(worktree.join("b.txt")).unwrap(), "new\n");

        // To the host: pushed over ssh
        std::fs::write(worktree.join("a.txt"), "back\n").unwrap();
        let back = snapshot(&local, worktree.to_str().unwrap(), "back");
        done(local.call(Request::Push {
            repo: local_repo.clone(),
            url: remote.git_url(std::path::Path::new(&remote_repo)),
            id: "back".into(),
            ssh: remote.ssh_command(),
        }));
        let arrived = sh(&host, &format!("git -C {remote_repo} rev-parse refs/cosmic-term/transfer/back"));
        assert_eq!(arrived, back.worktree);

        sh(&host, &format!("rm -rf {remote_repo}"));
        let _ = std::fs::remove_dir_all(&local_dir);
    }

    #[test]
    fn base64_matches_rfc_4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn remote_command_has_no_characters_special_inside_single_quotes() {
        let command = remote_command("echo 'it''s' \\ \"$HOME\"\n");
        let inner = command
            .strip_prefix("sh -c '")
            .and_then(|c| c.strip_suffix('\''))
            .unwrap();
        assert!(!inner.contains('\'') && !inner.contains('\\'));
    }

    #[test]
    fn session_label_drops_prefix_and_suffix() {
        let session = |name: &str| Session {
            host: "h".into(),
            name: name.into(),
        };
        assert_eq!(session("ct-claude-proj-1a2b").label(), "Claude · proj");
        assert_eq!(session("ct-codex-9f00").label(), "Codex");
        assert_eq!(session("ct-claude-proj-1a2b").agent(), Some(Agent::Claude));
        assert_eq!(session("ct-shell-srv-0003").label(), "Terminal · srv");
        assert_eq!(session("ct-shell-0003").agent(), Some(Agent::Shell));
    }

    #[test]
    fn parses_sessions() {
        let listed = parse_sessions(
            "h",
            "arcadian-peach [Created 1h ago] \nct-claude-x-0001 [Created 2m ago] (EXITED - attach to resurrect)\nct-codex-0002 [Created 1m ago] \n",
        );
        let names: Vec<_> = listed.iter().map(|l| l.session.name.as_str()).collect();
        assert_eq!(
            names,
            ["arcadian-peach", "ct-claude-x-0001", "ct-codex-0002"]
        );
        assert_eq!(
            listed.iter().map(|l| l.exited).collect::<Vec<_>>(),
            [false, true, false]
        );
        assert!(!listed[0].session.is_managed());
        assert!(listed[1].session.is_managed());
    }

    #[test]
    fn user_sessions_attach_with_their_own_config() {
        let attach = |name: &str| {
            session_script(
                &Session {
                    host: "h".into(),
                    name: name.into(),
                },
                None,
            )
        };
        assert!(attach("arcadian-peach").contains("\nexec zellij attach 'arcadian-peach'\n"));
        assert!(
            attach("ct-shell-0001").contains("--config \"$d/config.kdl\" attach 'ct-shell-0001'")
        );
    }

    fn launch_script(args: &[&str], dir: &str) -> String {
        let session = Session {
            host: "h".into(),
            name: "ct-claude-p-0001".into(),
        };
        let args = args.iter().map(|a| a.to_string()).collect();
        session_script(&session, Some((Some(("claude".into(), args)), dir)))
    }

    #[test]
    fn remote_script_is_valid_sh() {
        for script in [
            launch_script(&["--model", "it's \"x\" $HOME \\"], "~/my dir"),
            session_script(
                &Session {
                    host: "h".into(),
                    name: "ct-shell-0001".into(),
                },
                None,
            ),
        ] {
            let status = std::process::Command::new("sh")
                .args(["-n", "-c", &script])
                .status()
                .unwrap();
            assert!(status.success(), "{script}");
        }
    }

    fn thread_session() -> Session {
        Session { host: "h".into(), name: "ct-thread-claude-p-0123abcd".into() }
    }

    #[test]
    fn thread_sessions_are_named_apart() {
        let session = new_thread_session(Agent::Codex, "/srv/my proj/", " h ");
        assert!(session.is_thread() && session.is_managed(), "{}", session.name);
        assert!(session.name.starts_with("ct-thread-codex-myproj-"), "{}", session.name);
        assert_eq!(session.name.len(), "ct-thread-codex-myproj-".len() + 8);
        assert_eq!(session.host, "h");
        assert_eq!(session.agent(), Some(Agent::Codex));
        assert_eq!(session.label(), "Codex · myproj");
        assert!(!Session { host: String::new(), name: "ct-claude-p-0001".into() }.is_thread());
    }

    #[test]
    fn thread_scripts_are_valid_sh() {
        let meta = serde_json::json!({"cwd": "it's \"x\"", "conversation": "c-1"});
        let start = start_script(
            "ct-supervise-0011",
            &thread_session(),
            "cd 'it'\\''s'\nexec claude --x \"$HOME\"\n",
            "\"type\":\"result\"",
            Duration::from_secs(60),
            &meta,
        );
        let check = |script: &str| {
            let status = std::process::Command::new("sh").args(["-n", "-c", script]).status().unwrap();
            assert!(status.success(), "{script}");
        };
        check(&start);
        check(&thread_attach_script(&thread_session(), cosmic_term::relay::Cursor::Live));
        check(&thread_stop_script(&thread_session()));
        // The relay's own script, as its shell is given it
        let (_, rest) = start.split_once("\nr=").unwrap();
        let (quoted, _) = rest.split_once("\nif command -v zellij").unwrap();
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("printf '%s' {quoted}")])
            .output()
            .unwrap();
        let relay = String::from_utf8(output.stdout).unwrap();
        check(&relay);
        assert!(
            relay.contains("--done '\"type\":\"result\"' --linger 60 --conversation 'c-1' -- sh \"$t/launch.sh\""),
            "{relay}"
        );
        assert!(start.contains("attach --create-background 'ct-thread-claude-p-0123abcd' -- sh -c \"$r\""));
        assert!(start.ends_with("exec \"$s\" --connect \"$t\" --wait 10 --from 0 --client \"$CT_CLIENT\"\n"));
    }

    /// A thread's relay started, joined and stopped by its scripts, as on a
    /// machine without Zellij, with the `ct-supervise` built beside the
    /// tests and a stand-in CLI
    #[test]
    fn threads_outlive_their_connections() {
        use std::io::{BufRead, Write};
        let Some(supervisor) = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("ct-supervise")))
            .filter(|path| path.exists())
        else {
            eprintln!("no ct-supervise built beside the tests: skipped");
            return;
        };
        let home = std::env::temp_dir().join(format!("ct-thread-test-{}", cosmic_term::random_hex(8)));
        let agents = home.join(".config/cosmic-term-agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::copy(&supervisor, agents.join("ct-supervise-test")).unwrap();
        // Where Zellij is, to try its path too: by default, none
        let path = std::env::var("CT_TEST_PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        let zellij = |args: &[&str]| {
            std::process::Command::new("zellij").args(args).env("PATH", &path).env("HOME", &home).output().ok()
        };
        let in_zellij = |name: &str| {
            zellij(&["list-sessions", "-n"])
                .is_some_and(|out| String::from_utf8_lossy(&out.stdout).lines().any(|l| l.starts_with(&format!("{name} "))))
        };
        let session = Session { host: String::new(), name: format!("ct-thread-claude-x-{}", cosmic_term::random_hex(8)) };
        let fake = "while IFS= read -r line; do\n\
                    printf '{\"got\":%s}\\n' \"$line\"\n\
                    case \"$line\" in *turn*) printf '{\"type\":\"result\"}\\n' ;; esac\n\
                    done\n";
        let meta = serde_json::json!({"agent": "claude", "cwd": "/tmp", "created": 1});
        let start = start_script("ct-supervise-test", &session, fake, "\"type\":\"result\"", Duration::from_secs(60), &meta);
        let sh = |script: &str| {
            let mut command = std::process::Command::new("sh");
            command
                .arg("-c")
                .arg(format!("{PATH_EXPORT}{script}"))
                .env("HOME", &home)
                .env("PATH", &path)
                .env_remove("XDG_STATE_HOME")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
            command.spawn().unwrap()
        };
        let read_until = |lines: &mut dyn Iterator<Item = std::io::Result<String>>, want: &str| {
            let mut got = Vec::new();
            for line in lines {
                let line = line.unwrap();
                let done = line.contains(want);
                got.push(line);
                if done {
                    return got;
                }
            }
            panic!("no {want:?} in {got:#?}");
        };

        let mut child = sh(&start);
        let mut stdin = child.stdin.take().unwrap();
        let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
        stdin.write_all(b"turn t1 {\"turn\":1}\n").unwrap();
        let got = read_until(&mut lines, "\"type\":\"result\"");
        assert!(got[0].starts_with("#ct hello 1 "), "{got:#?}");
        assert_eq!(&got[1..], ["#ct< t1 turn {\"turn\":1}", "{\"got\":{\"turn\":1}}", "{\"type\":\"result\"}"]);
        // Letting go leaves the relay running
        drop(stdin);
        assert!(child.wait().unwrap().success());
        let dir = home.join(".local/state/cosmic-term/threads").join(&session.name);
        assert!(cosmic_term::relay::is_live(&dir));
        let written: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("meta.json")).unwrap()).unwrap();
        assert_eq!(written, meta);
        let zellij_used = in_zellij(&session.name);
        eprintln!("relay in Zellij: {zellij_used}");

        // The sidebar's scan lists it
        let scan = std::process::Command::new("python3")
            .args(["-c", PROJECT_DIRS, "sessions"])
            .env("HOME", &home)
            .env_remove("XDG_STATE_HOME")
            .output();
        if let Some(scan) = scan.ok().filter(|scan| scan.status.success()) {
            let rows: Vec<SessionSummary> = serde_json::from_slice(&scan.stdout).unwrap();
            let row = rows.iter().find(|row| row.name == session.name).expect("the thread's row");
            let thread = row.thread.as_ref().unwrap();
            assert_eq!((thread.agent.as_str(), thread.busy, row.clients), ("claude", false, 0));
            assert_eq!(row.cwd.as_deref(), Some("/tmp"));
        }

        // Joining it again replays the log from the offset asked for
        let log = std::fs::read_to_string(dir.join("out.log")).unwrap();
        let mut child = sh(&thread_attach_script(&session, cosmic_term::relay::Cursor::Offset(0)));
        let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let got = read_until(&mut lines, "\"type\":\"result\"");
        assert_eq!(got[1..].join("\n") + "\n", log);
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());

        // Stopped, it ends, and joining gives the rest of its log
        let mut child = sh(&thread_stop_script(&session));
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
        assert!(!cosmic_term::relay::is_live(&dir));
        // Its Zellij session ends with it
        if zellij_used {
            std::thread::sleep(Duration::from_millis(500));
            assert!(!in_zellij(&session.name));
        }
        let mut child = sh(&thread_attach_script(&session, cosmic_term::relay::Cursor::Offset(log.len() as u64)));
        drop(child.stdin.take());
        let output = child.wait_with_output().unwrap();
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.starts_with("#ct hello 1 pid=0 "), "{output}");
        assert!(output.lines().nth(1).is_some_and(|line| line.starts_with("#ct exited 0 ")), "{output}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn remote_command_decodes_to_the_script() {
        let script = launch_script(&["a b"], "/srv");
        let command = remote_command(&script);
        let encoded = command
            .strip_prefix("sh -c 'eval \"$(echo ")
            .and_then(|c| c.strip_suffix(" | base64 -d)\"'"))
            .unwrap();
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("echo {encoded} | base64 -d")])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), script);
    }

    #[test]
    fn remote_script_runs_the_agent_in_its_directory() {
        let script = launch_script(&["--add-dir", "it's"], "~/my dir");
        // The pane's command is the quoted `sh -c` argument, which runs to
        // the end of the script
        let (_, quoted) = script.split_once(" -- sh -c ").unwrap();
        let inner = std::process::Command::new("sh")
            .args(["-c", &format!("printf '%s' {}", quoted.trim_end())])
            .output()
            .unwrap()
            .stdout;
        let inner = String::from_utf8(inner).unwrap();
        assert!(
            inner.starts_with("cd \"$HOME\"/'my dir' || cd\n"),
            "{inner}"
        );
        assert!(
            inner.contains("'claude' '--add-dir' 'it'\\''s'\n"),
            "{inner}"
        );
        assert!(inner.ends_with("\nzellij kill-session \"$ZELLIJ_SESSION_NAME\"\n"));
    }

    #[test]
    fn agents_are_followed_to_where_they_work() {
        let root = std::env::temp_dir().join(format!("ct-focus-test-{}", std::process::id()));
        let home = root.join("home");
        let start = root.join("start");
        let worktree = root.join("worktree");
        let other = root.join("other");
        for dir in [&home, &start, &worktree.join("src"), &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        for repo in [&worktree, &other] {
            assert!(std::process::Command::new("git").args(["init", "-q"]).current_dir(repo).status().unwrap().success());
        }
        let run = |pid: u32| -> TabAgents {
            let output = std::process::Command::new("python3")
                .args(["-c", PROJECT_DIRS, "pid", &pid.to_string(), "--json"])
                .env("HOME", &home)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            serde_json::from_slice(&output.stdout).unwrap()
        };

        // A Claude started in `start` that has since worked in the worktree
        let mut claude = std::process::Command::new("sleep").arg("30").current_dir(&start).spawn().unwrap();
        let pid = claude.id();
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        std::fs::create_dir_all(home.join(".claude/projects/-start")).unwrap();
        std::fs::write(
            home.join(format!(".claude/sessions/{pid}.json")),
            format!(r#"{{"pid":{pid},"sessionId":"c1","cwd":"{}","status":"idle"}}"#, start.display()),
        )
        .unwrap();
        let tool = |input: String| {
            format!(
                r#"{{"type":"assistant","cwd":"{}","message":{{"content":[{{"type":"tool_use","name":"t","input":{input}}}]}}}}"#,
                start.display()
            )
        };
        let lines = [
            tool(format!(r#"{{"file_path":"{}/x.txt"}}"#, other.display())),
            tool(format!(r#"{{"command":"cd {} && cargo test"}}"#, worktree.join("src").display())),
            tool(format!(r#"{{"command":"git -C {} status"}}"#, worktree.display())),
            tool(format!(r#"{{"file_path":"{}/src/lib.rs"}}"#, worktree.display())),
            tool(r#"{"command":"ls /tmp"}"#.to_string()),
        ];
        std::fs::write(home.join(".claude/projects/-start/c1.jsonl"), lines.join("\n") + "\n").unwrap();
        let found = run(pid);
        claude.kill().ok();
        let agent = &found.agents[0];
        assert_eq!(agent.kind, "claude");
        assert_eq!(agent.conversation.as_deref(), Some("c1"));
        assert_eq!(agent.started_in.as_deref(), Some(start.to_str().unwrap()));
        assert_eq!(agent.focus, [worktree.display().to_string(), other.display().to_string()]);
        assert_eq!(agent.working_directory(), Some(worktree.to_str().unwrap()));

        // A Codex holding its thread's lock, working through `workdir`
        let lock = home.join(".codex/thread-writer-locks/0199aaaa-bbbb-7ccc-8ddd-eeeeeeeeeeee.lock");
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        std::fs::write(&lock, "").unwrap();
        let rollouts = home.join(".codex/sessions/2026/10/03");
        std::fs::create_dir_all(&rollouts).unwrap();
        let call = |args: String| {
            format!(r#"{{"type":"response_item","payload":{{"type":"function_call","name":"shell","arguments":{}}}}}"#, serde_json::to_string(&args).unwrap())
        };
        std::fs::write(
            rollouts.join("rollout-2026-10-03T10-00-00-0199aaaa-bbbb-7ccc-8ddd-eeeeeeeeeeee.jsonl"),
            [
                format!(r#"{{"type":"session_meta","payload":{{"id":"0199aaaa-bbbb-7ccc-8ddd-eeeeeeeeeeee","cwd":"{}"}}}}"#, start.display()),
                format!(r#"{{"type":"turn_context","payload":{{"cwd":"{}"}}}}"#, start.display()),
                call(format!(r#"{{"cmd":["ls"],"workdir":"{}"}}"#, other.display())),
                call(format!(r#"{{"cmd":["cargo","test"],"workdir":"{}"}}"#, worktree.display())),
                call(format!(r#"{{"cmd":["git","status"],"workdir":"{}"}}"#, worktree.display())),
            ]
            .join("\n"),
        )
        .unwrap();
        let mut codex = std::process::Command::new("python3")
            .args(["-c", "import sys, time; f = open(sys.argv[1]); time.sleep(30)", lock.to_str().unwrap()])
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let found = run(codex.id());
        codex.kill().ok();
        let _ = std::fs::remove_dir_all(&root);
        let agent = &found.agents[0];
        assert_eq!(agent.kind, "codex");
        assert_eq!(agent.conversation.as_deref(), Some("0199aaaa-bbbb-7ccc-8ddd-eeeeeeeeeeee"));
        assert!(!agent.guessed);
        assert_eq!(agent.focus.first().map(String::as_str), Some(worktree.to_str().unwrap()));
    }

    #[test]
    fn project_dirs_include_claude_added_directories() {
        let root = std::env::temp_dir().join(format!("ct-dirs-test-{}", std::process::id()));
        let home = root.join("home");
        let work = root.join("work");
        let repo = root.join("repo");
        let other = root.join("other");
        let added = root.join("added");
        for dir in [&home, &work, &repo.join("sub"), &other, &added] {
            std::fs::create_dir_all(dir).unwrap();
        }
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
        // A tagged process standing in for a tab's Claude
        let token = format!("test-{}", std::process::id());
        let mut claude = std::process::Command::new("sleep")
            .arg("30")
            .env(host::TAB_VAR, &token)
            .spawn()
            .unwrap();
        let pid = claude.id();
        let sessions = home.join(".claude/sessions");
        let project = home.join(".claude/projects/-work");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            sessions.join(format!("{pid}.json")),
            format!(
                r#"{{"pid":{pid},"sessionId":"s1","cwd":"{}"}}"#,
                work.display()
            ),
        )
        .unwrap();
        let snapshot = |dirs: &[&std::path::Path]| {
            let dirs: Vec<String> = dirs
                .iter()
                .map(|d| format!("\"{}\"", d.display()))
                .collect();
            format!(
                r#"{{"type":"attachment","attachment":{{"type":"environment","snapshot":{{"workingDirectory":"{}","additionalWorkingDirectories":[{}]}}}}}}"#,
                work.display(),
                dirs.join(",")
            )
        };
        // What /add-dir writes, with the path in bold; a failed attempt adds
        // nothing
        let add_dir = |outcome: &str| {
            format!(
                r#"{{"type":"user","message":{{"role":"user","content":"<local-command-stdout>{outcome}</local-command-stdout>"}}}}"#
            )
        };
        // The latest snapshot wins
        std::fs::write(
            project.join("s1.jsonl"),
            format!(
                "{}\n{{\"type\":\"user\"}}\n{}\n{}\n{}\n",
                snapshot(&[]),
                add_dir("Did not add a working directory."),
                add_dir(&format!(
                    "Added \\u001b[1m{}\\u001b[22m as a working directory for this session",
                    added.display()
                )),
                snapshot(&[&repo.join("sub"), &other])
            ),
        )
        .unwrap();

        let output = std::process::Command::new("python3")
            .arg("-c")
            .arg(PROJECT_DIRS)
            .args(["env", &token])
            .env("HOME", &home)
            .output()
            .unwrap();
        let _ = claude.kill();
        let _ = claude.wait();
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let dirs: Vec<String> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        // Where Claude started (work) is not a project of its own; the
        // directories it was given are, whatever they are
        assert_eq!(
            dirs,
            [repo, other, added]
                .iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn descriptors_round_trip() {
        for session in [
            Session {
                host: "twright@neo".into(),
                name: "ct-claude-x-0001".into(),
            },
            Session {
                host: String::new(),
                name: "ct-shell-0002".into(),
            },
        ] {
            assert_eq!(Session::from_descriptor(&session.descriptor()), Some(session));
        }
        assert_eq!(Session::from_descriptor(""), None);
    }

    #[test]
    fn local_sessions_run_without_ssh() {
        let profile = Profile::agent("t", Agent::Claude, "");
        let (command, session) = launch(&profile, Some("/tmp"));
        assert!(session.is_local());
        assert_eq!(command.program, "sh");
        assert!(session.name.starts_with("ct-claude-tmp-"), "{}", session.name);
        assert!(command.args[1].contains("attach --create --close-on-exit"));
    }

    #[test]
    fn runs_directly_without_zellij() {
        // A PATH without zellij; the script's own PATH additions point at
        // an empty HOME
        let home = std::env::temp_dir().join(format!("ct-nozellij-{}", std::process::id()));
        std::fs::create_dir_all(home.join("work")).unwrap();
        let profile = Profile {
            command: "pwd".into(),
            ..Profile::agent("t", Agent::Shell, "")
        };
        let (command, _) = launch(&profile, Some(home.join("work").to_str().unwrap()));
        let output = std::process::Command::new(&command.program)
            .args(&command.args)
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&home);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.trim_end().ends_with("/work"), "{stdout}");
    }

    #[test]
    fn finds_paths_in_links_and_words() {
        let cases = [
            ("file:///home/t/a%20b.rs", Some("/home/t/a b.rs")),
            ("file://neo/home/t/x.rs", Some("/home/t/x.rs")),
            ("src/main.rs:42:7", Some("src/main.rs")),
            ("src/main.rs:42", Some("src/main.rs")),
            ("`README.md`.", Some("README.md")),
            ("(src/lib.rs)", Some("src/lib.rs")),
            ("/etc/hosts,", Some("/etc/hosts")),
            ("~/notes.txt", Some("~/notes.txt")),
            ("https://example.com/a.rs", None),
            ("hello", None),
            ("v1.", None),
        ];
        for (link, path) in cases {
            assert_eq!(link_path(link).as_deref(), path, "{link}");
        }
    }

    #[test]
    fn resolves_relative_paths() {
        assert_eq!(resolve_path("src/a.rs", "/h", "/h/p/"), "/h/p/src/a.rs");
        assert_eq!(resolve_path("./a.rs", "/h", "/h/p"), "/h/p/a.rs");
        assert_eq!(resolve_path("~/n.txt", "/h/", "/x"), "/h/n.txt");
        assert_eq!(resolve_path("/abs", "/h", "/x"), "/abs");
    }

    #[test]
    fn launcher_script_output_parses() {
        let output = std::process::Command::new("python3")
            .args(["-c", LAUNCHER, "all", "3"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let data: HostListing = serde_json::from_slice(&output.stdout).unwrap();
        assert!(!data.home.is_empty());
        assert!(!data.identity().is_empty());
        for agent in [Agent::Claude, Agent::Codex] {
            assert!(data.agent(agent).unwrap().conversations.len() <= 3);
        }
    }

    #[test]
    fn resumes_before_the_profile_arguments() {
        let profile = Profile {
            args: "--model x".into(),
            ..Profile::agent("t", Agent::Codex, "")
        };
        let (command, _) = launch_resuming(&profile, Some("/tmp"), Some("abc"));
        let (_, quoted) = command.args[1].split_once(" -- sh -c ").unwrap();
        let inner = std::process::Command::new("sh")
            .args(["-c", &format!("printf '%s' {}", quoted.trim_end())])
            .output()
            .unwrap()
            .stdout;
        let inner = String::from_utf8(inner).unwrap();
        assert!(inner.contains("'codex' 'resume' 'abc' '--model' 'x'\n"), "{inner}");
    }

    #[test]
    fn codex_profiles_get_full_access_once() {
        let mut config = Config::default();
        assert!(seed_profiles(&mut config));
        let codex_args: Vec<_> = config
            .profiles
            .values()
            .filter(|p| p.agent == Agent::Codex)
            .map(|p| p.args.clone())
            .collect();
        assert_eq!(codex_args, [CODEX_FULL_ACCESS]);
        assert!(config.profiles.values().all(|p| !p.is_remote()));
        assert!(!seed_profiles(&mut config));

        // An older config gets it, except where the user set arguments
        let mut config = Config {
            profile_seed_version: 2,
            agent_profiles_seeded: true,
            ..Config::default()
        };
        config.profiles.insert(ProfileId(0), Profile::agent("a", Agent::Codex, ""));
        config.profiles.insert(
            ProfileId(1),
            Profile {
                args: "--model x".into(),
                ..Profile::agent("b", Agent::Codex, "")
            },
        );
        assert!(seed_profiles(&mut config));
        assert_eq!(config.profiles[&ProfileId(0)].args, CODEX_FULL_ACCESS);
        assert_eq!(config.profiles[&ProfileId(1)].args, "--model x");
    }

    #[test]
    fn strips_zellij_title_prefix() {
        assert_eq!(
            strip_zellij_title("Zellij (ct-claude-1) - ✳ Fix bug", "ct-claude-1"),
            "✳ Fix bug"
        );
        assert_eq!(strip_zellij_title("ct-shell-b3ad | ~", "ct-shell-b3ad"), "~");
        assert_eq!(strip_zellij_title("plain | x", "ct-shell-b3ad"), "plain | x");
        assert_eq!(strip_zellij_title("plain", "ct-shell-b3ad"), "plain");
    }
}

/// For trying a tab's command by hand:
/// `ARGV_OUT=file cargo test write_local_argv -- --ignored`
/// writes the NUL-separated argv of a local terminal tab starting in /tmp,
/// running `ARGV_COMMAND` if set, on `ARGV_HOST` if set
#[cfg(test)]
#[test]
#[ignore]
fn write_local_argv() {
    let host = std::env::var("ARGV_HOST").unwrap_or_default();
    let profile = Profile {
        command: std::env::var("ARGV_COMMAND").unwrap_or_default(),
        ..Profile::agent("t", Agent::Shell, &host)
    };
    let (command, session) = launch(&profile, Some("/tmp"));
    let mut argv = vec![command.program];
    argv.extend(command.args);
    let out = std::env::var("ARGV_OUT").expect("ARGV_OUT");
    std::fs::write(out, argv.join("\0")).unwrap();
    println!("SESSION={}", session.name);
    println!("SUPERVISOR={}", supervisor_name().unwrap_or_default());
}
