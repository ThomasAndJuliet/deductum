// SPDX-License-Identifier: GPL-3.0-only

//! Links to session hosts (design C2, C11): one per machine, locally or over
//! ssh to a machine Terminal Agents knows (C1). The host speaks JSON lines;
//! `deductum_host.py connect` bridges stdio to its socket and starts it if
//! need be. Before connecting, the host and backend sources this app carries
//! are installed on the machine when missing or older.

use std::hash::Hash;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use cosmic::iced::futures::channel::mpsc::Sender;
use cosmic::iced::futures::{SinkExt, Stream};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// Where the host runs
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Target {
    /// `local`, or the machine's name in Terminal Agents
    pub key: String,
    /// The ssh destination, or none for this computer
    pub destination: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Connecting(String),
    Connected(mpsc::UnboundedSender<String>),
    Line(serde_json::Value),
    Disconnected(String),
}

/// A machine from Terminal Agents' configuration
#[derive(Clone, Debug, Default, Deserialize)]
struct Machine {
    name: String,
    host: String,
    #[serde(default)]
    user: String,
}

/// This computer, then the machines Terminal Agents has registered
pub fn targets() -> Vec<(String, Target)> {
    let mut out = vec![(local_name(), Target { key: "local".into(), destination: None })];
    // Deductum can run without Terminal Agents, including on Windows.
    let path = super::platform::config_dir().join("machines.json");
    if let Ok(text) = std::fs::read_to_string(path) {
        match serde_json::from_str::<Vec<Machine>>(&text) {
            Ok(machines) => {
                for machine in machines {
                    let destination = if machine.user.trim().is_empty() {
                        machine.host.trim().to_string()
                    } else {
                        format!("{}@{}", machine.user.trim(), machine.host.trim())
                    };
                    if !machine.name.trim().is_empty()
                        && machine.name != "local"
                        && !destination.is_empty()
                        && !destination.starts_with('-')
                        && !destination.chars().any(char::is_whitespace)
                        && !out.iter().any(|(_, target)| target.key == machine.name)
                    {
                        out.push((machine.name.clone(), Target { key: machine.name, destination: Some(destination) }));
                    }
                }
            }
            Err(error) => log::error!("could not read Deductum's machines.json: {error}"),
        }
    }
    // Extra machines, `name=destination,…`, for trying a host without registering it
    if let Ok(extra) = std::env::var("DEDUCTUM_MACHINES") {
        for item in extra.split(',').filter(|s| !s.trim().is_empty()) {
            let (name, dest) = item.split_once('=').unwrap_or((item, item));
            out.push((name.trim().to_string(), Target { key: name.trim().to_string(), destination: Some(dest.trim().to_string()) }));
        }
    }
    // In a Flatpak, XDG_CONFIG_HOME is the app's own; Terminal Agents' is under the real one
    let path = [home().join(".config"), config_dir()]
        .into_iter()
        .map(|d| d.join("cosmic/dev.twright.CosmicTermAgents/v1/machines"))
        .find(|p| p.is_file())
        .unwrap_or_default();
    // (keys as Terminal Agents writes them: a machine id, `(0)`, or a bare number)
    #[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    struct Id(u64);
    let parsed = std::fs::read_to_string(&path).ok().and_then(|text| {
        ron::from_str::<std::collections::BTreeMap<Id, Machine>>(&text)
            .map(|m| m.into_values().collect::<Vec<_>>())
            .or_else(|_| ron::from_str::<std::collections::BTreeMap<u64, Machine>>(&text).map(|m| m.into_values().collect()))
            .ok()
    });
    if let Some(machines) = parsed {
        let me = local_name();
        for machine in machines {
            if machine.name == me || machine.host == me {
                continue;
            }
            let destination =
                if machine.user.trim().is_empty() { machine.host.clone() } else { format!("{}@{}", machine.user.trim(), machine.host.trim()) };
            out.push((machine.name.clone(), Target { key: machine.name, destination: Some(destination) }));
        }
    }
    if out.len() == 1 {
        // No machines registered here: the fleet's, as the Terminal Agents mount helper lists them
        // (`NAME  ssh DESTINATION  MOUNT  STATUS`)
        let helper = home().join(".config/cosmic-term-agents/ct-mount");
        if let Ok(output) = std::process::Command::new(&helper).arg("--list").output() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let fields: Vec<&str> = line.split('\t').collect();
                if let [name, ssh, ..] = fields.as_slice()
                    && let Some(dest) = ssh.strip_prefix("ssh ")
                    && !out.iter().any(|(_, t)| t.key == *name)
                {
                    out.push((name.to_string(), Target { key: name.to_string(), destination: Some(dest.trim().to_string()) }));
                }
            }
        }
    }
    out
}

fn config_dir() -> PathBuf {
    super::platform::config_home()
}

fn home() -> PathBuf {
    super::platform::home()
}

pub fn local_name() -> String {
    #[cfg(windows)]
    if let Ok(name) = std::env::var("COMPUTERNAME") {
        return name;
    }
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "this computer".into())
}

fn sandboxed() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// A command run on this computer's host system, outside a Flatpak sandbox
pub(crate) fn host_command(program: &str, args: &[String]) -> tokio::process::Command {
    if sandboxed() {
        let mut cmd = tokio::process::Command::new("flatpak-spawn");
        cmd.arg("--host").arg(program).args(args);
        cmd
    } else {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW for connection helpers
        cmd
    }
}

/// The host and backend sources this app carries
pub fn bundle() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("DEDUCTUM_BUNDLE") {
        candidates.push(PathBuf::from(dir));
    }
    candidates.push(PathBuf::from("/app/share/deductum"));
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("share/deductum"));
        candidates.push(dir.join("../share/deductum"));
        // target/<profile>/deductum in a source checkout: the checkout root holds host/ and backend/
        candidates.push(dir.join("../.."));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    candidates.into_iter().find(|p| p.join("host/deductum_host.py").is_file())
}

pub fn bundle_version(bundle: &std::path::Path) -> String {
    // Every file that is installed (as `install` packs them), so that a change to any of them,
    // a driver or a prompt as much as the host itself, installs the new sources
    fn walk(dir: &std::path::Path, rel: &str, out: &mut Vec<(String, std::path::PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(name.as_str(), "__pycache__" | "lib" | "tests") {
                continue;
            }
            let path = entry.path();
            let rel = format!("{rel}/{name}");
            if path.is_dir() {
                walk(&path, &rel, out);
            } else {
                out.push((rel, path));
            }
        }
    }
    let mut files = Vec::new();
    walk(&bundle.join("host"), "host", &mut files);
    walk(&bundle.join("backend"), "backend", &mut files);
    files.sort();
    let mut hasher = std::hash::DefaultHasher::new();
    for (rel, path) in files {
        rel.hash(&mut hasher);
        std::fs::read(path).unwrap_or_default().hash(&mut hasher);
    }
    use std::hash::Hasher;
    format!("{:016x}", hasher.finish())
}

/// Where the host is installed on the target, and the environment it runs with: normally
/// `~/.local/share/deductum` and the user's state; `DEDUCTUM_REMOTE_ROOT` (for tests) installs
/// and runs a host of its own there, with its own state, beside the user's
fn remote_root(target: &Target) -> (String, String) {
    // (`DEDUCTUM_LOCAL_ROOT` is the same for this computer's host alone, so that a test can run a
    // host of its own here beside the real one it reaches on another machine)
    let var = if target.destination.is_none() && std::env::var_os("DEDUCTUM_LOCAL_ROOT").is_some() { "DEDUCTUM_LOCAL_ROOT" } else { "DEDUCTUM_REMOTE_ROOT" };
    match std::env::var(var) {
        Ok(root) if !root.is_empty() => (root.clone(), format!("DEDUCTUM_STATE_DIR={root}/state ")),
        _ => ("~/.local/share/deductum".to_string(), String::new()),
    }
}

/// Install the bundled sources on the target when its copy differs
/// What says which sources are installed: VERSION, but only if the host's main file is its whole
/// size, so that an install cut short by an older Deductum is repaired rather than taken as done
fn installed_check_script(root: &str, host_size: u64) -> String {
    format!("[ \"$(wc -c < {root}/host/deductum_host.py 2>/dev/null)\" = {host_size} ] && cat {root}/VERSION 2>/dev/null || true")
}

/// What unpacks the sources from stdin. They go to a directory beside their place and then move
/// in a file at a time, with VERSION last, so that two windows starting together, or one closed
/// half way, never leave a part of a file behind. Runs inside single quotes over ssh: it has none
fn unpack_script(root: &str, env: &str, version: &str) -> String {
    format!(
        "{{ [ -f {root}/host/deductum_host.py ] && {env}python3 {root}/host/deductum_host.py stop-host >/dev/null 2>&1; true; }} && mkdir -p {root} && r=$(cd {root} && pwd) \
         && {{ find \"$r\" -maxdepth 1 -name \".install-*\" -mmin +60 -exec rm -rf {{}} + ; t=$(mktemp -d \"$r/.install-XXXXXX\"); }} \
         && {{ tar -C \"$t\" -xf - || {{ rm -rf \"$t\"; exit 1; }}; }} && (cd \"$t\" && find . -type f) | while IFS= read -r f; do mkdir -p \"$(dirname \"$r/$f\")\" && mv -f \"$t/$f\" \"$r/$f\" || exit 1; done \
         && rm -rf \"$t\" && echo {version} > \"$r/VERSION\""
    )
}

async fn install(target: &Target) -> Result<(), String> {
    #[cfg(windows)]
    if target.destination.is_none() {
        return bundle().map(|_| ()).ok_or_else(|| "Deductum's bundled session host is missing; reinstall the complete Windows package".into());
    }
    let Some(bundle) = bundle() else { return Ok(()) };
    // Running from the installed copy itself: nothing to install
    let installed = home().join(".local/share/deductum");
    let (root, _) = remote_root(target);
    if target.destination.is_none()
        && root.starts_with('~')
        && std::fs::canonicalize(&bundle).ok().is_some_and(|b| std::fs::canonicalize(&installed).ok() == Some(b))
    {
        return Ok(());
    }
    let version = bundle_version(&bundle);
    let host_size = std::fs::metadata(bundle.join("host/deductum_host.py")).map_or(0, |m| m.len());
    let check = installed_check_script(&root, host_size);
    let output = match &target.destination {
        None => host_command("sh", &["-c".into(), check]).output().await,
        Some(dest) => {
            host_command("ssh", &["-o".into(), "BatchMode=yes".into(), "-o".into(), "ConnectTimeout=10".into(), dest.clone(), "sh".into(), "-c".into(), format!("'{check}'")])
                .output()
                .await
        }
    }
    .map_err(|e| e.to_string())?;
    if String::from_utf8_lossy(&output.stdout).trim() == version {
        return Ok(());
    }
    // tar the sources, without built jars, and unpack them there
    let mut tar = std::process::Command::new("tar");
    // Windows tar narrows non-ACP command-line paths. Set the directory through
    // CreateProcessW instead, keeping argv ASCII and preserving bundle spelling.
    tar.current_dir(&bundle).args(["-C", ".", "--exclude=__pycache__", "--exclude=lib", "--exclude=tests", "-cf", "-", "host", "backend"]);
    let archive = tar.output().map_err(|e| format!("tar: {e}"))?;
    if !archive.status.success() {
        return Err("could not pack the host sources".into());
    }
    // A host already running there runs the old sources: it is asked to stop first (its sessions
    // that were running start again, a new epoch, when the next client starts the new host)
    let (_, env) = remote_root(target);
    let unpack = unpack_script(&root, &env, &version);
    let mut child = match &target.destination {
        None => host_command("sh", &["-c".into(), unpack]),
        Some(dest) => host_command("ssh", &["-o".into(), "BatchMode=yes".into(), dest.clone(), "sh".into(), "-c".into(), format!("'{unpack}'")]),
    }
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&archive.stdout).await.map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("installing the host failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    // The deductum MCP server for Claude and Codex there, where it is missing. Agents that the app
    // starts on that machine find the proof tools (the result, per tool: added, updated, present, skipped,
    // unreadable, or failed, is only logged); a failure here does not stop the host's use
    #[cfg(not(windows))]
    {
        let script = register_mcp_script(&root, &env);
        let run = match &target.destination {
            None => host_command("sh", &["-c".into(), script]),
            Some(dest) => host_command("ssh", &["-o".into(), "BatchMode=yes".into(), "-o".into(), "ConnectTimeout=10".into(), dest.clone(), "sh".into(), "-c".into(), format!("'{script}'")]),
        }
        .stdin(Stdio::null())
        .output()
        .await;
        match run {
            Ok(o) if o.status.success() => log::info!("registering the deductum MCP server on {}: {}", target.key, String::from_utf8_lossy(&o.stdout).trim()),
            Ok(o) => log::warn!("registering the deductum MCP server on {} failed: {}", target.key, String::from_utf8_lossy(&o.stderr).trim()),
            Err(e) => log::warn!("registering the deductum MCP server on {} failed: {e}", target.key),
        }
    }
    Ok(())
}

/// What registers the deductum MCP server for Claude and Codex (at user level, where it is missing,
/// leaving what is there as it is) with the host installed in `root`: run on that machine
#[cfg(not(windows))]
fn register_mcp_script(root: &str, env: &str) -> String {
    format!("{env}python3 {root}/host/deductum_host.py register-mcp")
}

#[cfg(windows)]
fn native_python(bundle: &std::path::Path) -> PathBuf {
    if let Some(python) = std::env::var_os("DEDUCTUM_PYTHON").filter(|value| !value.is_empty()) {
        return PathBuf::from(python);
    }
    let python = bundle.join("../../python/python.exe");
    if python.is_file() { python } else { PathBuf::from("python.exe") }
}

fn connect_command(target: &Target) -> Result<tokio::process::Command, String> {
    #[cfg(windows)]
    if target.destination.is_none() {
        let bundle = bundle().ok_or_else(|| "Deductum's bundled session host is missing".to_string())?;
        let mut cmd = tokio::process::Command::new(native_python(&bundle));
        cmd.args(["-X", "utf8"]).arg(bundle.join("host/deductum_host.py")).arg("connect");
        cmd.env("PYTHONUTF8", "1");
        if let Some(root) = std::env::var_os("DEDUCTUM_LOCAL_ROOT") {
            cmd.env("DEDUCTUM_STATE_DIR", PathBuf::from(root).join("state"));
        }
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        return Ok(cmd);
    }
    let (root, env) = remote_root(target);
    let script = format!("{env}python3 {root}/host/deductum_host.py connect");
    let mut cmd = match &target.destination {
        None => host_command("sh", &["-c".into(), script.clone()]),
        Some(dest) => host_command(
            "ssh",
            &["-o".into(), "BatchMode=yes".into(), "-o".into(), "ServerAliveInterval=15".into(), "-T".into(), dest.clone(), "sh".into(), "-c".into(), format!("'{script}'")],
        ),
    };
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    Ok(cmd)
}

/// Read host events and write commands independently. Delivering an event can
/// wait for the UI's bounded queue without holding up edits or cancellation.
/// Both pumps belong to this future: ending either drops the other and closes
/// its pipe, including when the subscription itself is dropped.
async fn exchange<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    stdout: R,
    mut stdin: W,
    mut lines_out: mpsc::UnboundedReceiver<String>,
    key: &str,
    output: &mut Sender<(String, Event)>,
) -> std::io::Result<()> {
    let read = async {
        let mut lines = tokio::io::BufReader::with_capacity(1 << 20, stdout).lines();
        while let Some(line) = lines.next_line().await? {
            if let Ok(value) = serde_json::from_str(&line) {
                output.send((key.to_string(), Event::Line(value))).await.map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the UI connection closed")
                })?;
            }
        }
        Ok(())
    };
    let write = async {
        while let Some(mut line) = lines_out.recv().await {
            line.push('\n');
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await?;
        }
        Ok(())
    };
    tokio::select! {
        result = read => result,
        result = write => result,
    }
}

/// The link to `target`'s host, reconnecting after a pause when it drops
pub fn run(target: &Target) -> impl Stream<Item = (String, Event)> + use<> {
    let target = target.clone();
    cosmic::iced::stream::channel(64, move |mut output: Sender<(String, Event)>| async move {
        let key = target.key.clone();
        loop {
            if output.send((key.clone(), Event::Connecting("installing".into()))).await.is_err() {
                return;
            }
            if let Err(err) = install(&target).await {
                if output.send((key.clone(), Event::Disconnected(err))).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(20)).await;
                continue;
            }
            if output.send((key.clone(), Event::Connecting("connecting".into()))).await.is_err() {
                return;
            }
            let mut child = match connect_command(&target).and_then(|mut command| command.spawn().map_err(|error| error.to_string())) {
                Ok(child) => child,
                Err(err) => {
                    if output.send((key.clone(), Event::Disconnected(err.to_string()))).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };
            let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
                continue;
            };
            let (to_host, lines_out) = mpsc::unbounded_channel::<String>();
            if output.send((key.clone(), Event::Connected(to_host))).await.is_err() {
                return;
            }
            let mut tail = std::collections::VecDeque::new();
            let result = {
                // Keep stderr owned by the connection as well: a cancelled
                // subscription must not leave a detached reader behind.
                let drain_stderr = async {
                    let mut lines = tokio::io::BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        tail.push_back(line);
                        if tail.len() > 10 {
                            tail.pop_front();
                        }
                    }
                };
                let exchange = exchange(stdout, stdin, lines_out, &key, &mut output);
                tokio::pin!(drain_stderr, exchange);
                let mut stderr_finished = false;
                let result = loop {
                    tokio::select! {
                        result = &mut exchange => break result,
                        _ = &mut drain_stderr, if !stderr_finished => stderr_finished = true,
                    }
                };
                let _ = child.kill().await;
                // A descendant can inherit the pipe; it must not hold up a
                // reconnect forever after the connection process has exited.
                if !stderr_finished {
                    let _ = tokio::time::timeout(Duration::from_secs(1), &mut drain_stderr).await;
                }
                result
            };
            let tail = tail.into_iter().collect::<Vec<_>>().join("\n");
            let reason = if !tail.trim().is_empty() {
                tail
            } else if let Err(error) = result {
                format!("the connection closed: {error}")
            } else {
                "the connection closed".to_string()
            };
            if output.send((key.clone(), Event::Disconnected(reason))).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    })
}

/// Where to open a session's folder
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opener {
    Files,
    Zed,
    Terminal,
}

/// Open a session's folder, on the computer `target` names, in the file manager (through
/// Terminal Agents' sshfs mount if it is remote), Zed (as an ssh project) or a terminal
/// (ssh'd there), the way Terminal Agents opens a tab's project
pub async fn open_folder(opener: Opener, target: Target, folder: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        return open_folder_windows(opener, target, folder).await;
    }
    #[cfg(not(windows))]
    open_folder_unix(opener, target, folder).await
}

#[cfg(windows)]
async fn open_folder_windows(opener: Opener, target: Target, folder: String) -> Result<(), String> {
    let mut command = match (opener, target.destination) {
        (Opener::Files, None) => {
            return open::that_detached(folder).map_err(|error| error.to_string());
        }
        (Opener::Files, Some(_)) => return Err("A remote folder needs a mounted drive to open in Explorer. Use Open in Zed or Open terminal for this SSH connection.".into()),
        (Opener::Zed, destination) => {
            let mut command = tokio::process::Command::new("zed.exe");
            command.arg("--new").arg(destination.map_or(folder.clone(), |destination| format!("ssh://{destination}{folder}")));
            command
        }
        (Opener::Terminal, None) => {
            return tokio::task::spawn_blocking(move || super::platform::windows::launch_console(
                "powershell.exe", &["-NoLogo".into(), "-NoExit".into()], Some(&folder)))
                .await.map_err(|error| error.to_string())?.map_err(|error| error.to_string());
        }
        (Opener::Terminal, Some(destination)) => {
            let quote = format!("'{}'", folder.replace('\'', r"'\''"));
            return tokio::task::spawn_blocking(move || super::platform::windows::launch_console(
                "ssh.exe", &["-t".into(), destination, format!("cd {quote} && exec \"$SHELL\" -l")], None))
                .await.map_err(|error| error.to_string())?.map_err(|error| error.to_string());
        }
    };
    if opener == Opener::Zed {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    }
    command.spawn().map(|_| ()).map_err(|error| error.to_string())
}

#[cfg(not(windows))]
async fn open_folder_unix(opener: Opener, target: Target, folder: String) -> Result<(), String> {
    let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let script = match (opener, &target.destination) {
        (Opener::Files, None) => format!("exec xdg-open {}", quote(&folder)),
        (Opener::Files, Some(_)) => format!(
            "m=$(\"$HOME/.config/cosmic-term-agents/ct-mount\" {} | tail -n 1) && [ -d \"$m\" ] || {{ echo \"cannot mount {}: $m\" >&2; exit 1; }}\n\
             exec xdg-open \"$m\"{}",
            quote(&target.key),
            target.key,
            quote(&folder)
        ),
        (Opener::Zed, dest) => {
            let arg = match dest {
                Some(dest) => format!("ssh://{dest}{folder}"),
                None => folder.clone(),
            };
            // Desktop sessions often lack ~/.local/bin on PATH; some distributions call it
            // zeditor, and Flathub's Zed is a Flatpak
            format!(
                "PATH=\"$PATH:$HOME/.local/bin\"\n\
                 for zed in zed zeditor; do command -v \"$zed\" >/dev/null 2>&1 && exec \"$zed\" --new {a}; done\n\
                 flatpak info dev.zed.Zed >/dev/null 2>&1 && exec setsid -f flatpak run dev.zed.Zed --new {a}\n\
                 echo 'Zed is not installed' >&2; exit 127",
                a = quote(&arg)
            )
        }
        (Opener::Terminal, dest) => {
            // Terminal Agents if it is installed, else the plain cosmic-term, else any terminal
            let (dir, command) = match dest {
                None => (quote(&folder), String::new()),
                Some(dest) => (
                    "\"$HOME\"".to_string(),
                    format!(" -e ssh -t {} {}", quote(dest), quote(&format!("cd {} && exec \"$SHELL\" -l", quote(&folder)))),
                ),
            };
            format!(
                "if flatpak info dev.twright.CosmicTermAgents >/dev/null 2>&1; then exec setsid -f flatpak run dev.twright.CosmicTermAgents -w {dir}{command}\n\
                 elif command -v cosmic-term >/dev/null 2>&1; then exec setsid -f cosmic-term -w {dir}{command}\n\
                 else cd {dir} && exec setsid -f xdg-terminal-exec{command_plain}; fi",
                command_plain = command.strip_prefix(" -e").unwrap_or("")
            )
        }
    };
    let mut command = host_command("sh", &["-c".to_string(), script]);
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), command.output())
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}


#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    use std::task::{Context, Poll};

    use cosmic::iced::futures::{StreamExt, channel::mpsc, poll};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

    use super::{Event, exchange};

    struct Tracked<T> {
        inner: T,
        dropped: Arc<AtomicBool>,
    }

    impl<T> Drop for Tracked<T> {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    impl<T: AsyncRead + Unpin> AsyncRead for Tracked<T> {
        fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl<T: AsyncWrite + Unpin> AsyncWrite for Tracked<T> {
        fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
            Pin::new(&mut self.inner).poll_write(cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    #[tokio::test]
    async fn blocked_ui_delivery_does_not_hold_up_ordered_edits_and_cancel() {
        let (mut host, client) = tokio::io::duplex(4096);
        let (stdout, stdin) = tokio::io::split(client);
        let (commands, lines_out) = tokio::sync::mpsc::unbounded_channel();
        let (mut output, mut events) = mpsc::channel(1);
        let mut queued = 0;
        while output.try_send(("test".into(), Event::Connecting("queued".into()))).is_ok() {
            queued += 1;
        }
        assert!(queued > 0);
        host.write_all(b"{\"revision\":1}\nnot json\n{\"revision\":2}\n").await.unwrap();
        let transfer = exchange(stdout, stdin, lines_out, "test", &mut output);
        tokio::pin!(transfer);
        // Enter the read pump while the event queue is full, before there is
        // anything to write. The old shared select loop remains in send here.
        assert!(poll!(&mut transfer).is_pending());
        let expected = "{\"edit\":1}\n{\"edit\":2}\n{\"cancel\":3}\n";
        for line in expected.lines() {
            commands.send(line.to_string()).unwrap();
        }
        let outgoing = async {
            let mut actual = vec![0; expected.len()];
            host.read_exact(&mut actual).await.unwrap();
            assert_eq!(actual, expected.as_bytes(), "edits and cancellation must retain their order");
        };
        tokio::select! {
            result = &mut transfer => panic!("connection ended with commands pending: {result:?}"),
            result = tokio::time::timeout(std::time::Duration::from_secs(2), outgoing) => {
                result.expect("a full UI event queue must not block commands to the host");
            }
        }
        let incoming = async {
            for _ in 0..queued {
                assert!(matches!(events.next().await, Some((_, Event::Connecting(_)))));
            }
            for revision in [1, 2] {
                match events.next().await {
                    Some((key, Event::Line(value))) => {
                        assert_eq!(key, "test");
                        assert_eq!(value["revision"], revision);
                    }
                    other => panic!("expected host revision {revision}, got {other:?}"),
                }
            }
        };
        tokio::select! {
            result = &mut transfer => panic!("connection ended before queued events were delivered: {result:?}"),
            result = tokio::time::timeout(std::time::Duration::from_secs(2), incoming) => {
                result.expect("host events must be delivered in order after the UI catches up");
            }
        }
        drop(commands);
        tokio::time::timeout(std::time::Duration::from_secs(2), transfer).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn host_eof_closes_the_command_receiver_and_both_pipes() {
        let (host, client) = tokio::io::duplex(64);
        let (stdout, stdin) = tokio::io::split(client);
        let read_dropped = Arc::new(AtomicBool::new(false));
        let write_dropped = Arc::new(AtomicBool::new(false));
        let (commands, lines_out) = tokio::sync::mpsc::unbounded_channel();
        let (mut output, _events) = mpsc::channel(1);
        drop(host);
        exchange(
            Tracked { inner: stdout, dropped: read_dropped.clone() },
            Tracked { inner: stdin, dropped: write_dropped.clone() },
            lines_out, "test", &mut output,
        ).await.unwrap();
        assert!(commands.is_closed());
        assert!(read_dropped.load(Ordering::SeqCst));
        assert!(write_dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn ui_disconnect_stops_both_pumps() {
        let (mut host, client) = tokio::io::duplex(64);
        let (stdout, stdin) = tokio::io::split(client);
        let (commands, lines_out) = tokio::sync::mpsc::unbounded_channel();
        let (mut output, events) = mpsc::channel(1);
        drop(events);
        host.write_all(b"{}\n").await.unwrap();
        let error = exchange(stdout, stdin, lines_out, "test", &mut output).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        assert!(commands.is_closed());
        let mut byte = [0];
        assert_eq!(host.read(&mut byte).await.unwrap(), 0, "the writer must close when event delivery fails");
    }

    #[tokio::test]
    async fn failed_command_write_stops_the_pending_host_reader() {
        let (_host, client) = tokio::io::duplex(64);
        let (stdout, _stdin) = tokio::io::split(client);
        // A separate closed pipe fails writes without making the pending host
        // reader complete, so only the write failure can end this exchange.
        let (closed_host, broken_stdin) = tokio::io::duplex(64);
        drop(closed_host);
        let read_dropped = Arc::new(AtomicBool::new(false));
        let (commands, lines_out) = tokio::sync::mpsc::unbounded_channel();
        let (mut output, _events) = mpsc::channel(1);
        commands.send("cancel".to_string()).unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), exchange(
            Tracked { inner: stdout, dropped: read_dropped.clone() },
            broken_stdin, lines_out, "test", &mut output,
        )).await.expect("write failure must stop the pending reader");
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
        assert!(commands.is_closed());
        assert!(read_dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn dropping_connection_drops_the_pipes_and_queued_commands() {
        let (_host, client) = tokio::io::duplex(64);
        let (stdout, stdin) = tokio::io::split(client);
        let read_dropped = Arc::new(AtomicBool::new(false));
        let write_dropped = Arc::new(AtomicBool::new(false));
        let (commands, lines_out) = tokio::sync::mpsc::unbounded_channel();
        let (mut output, _events) = mpsc::channel(1);
        {
            let transfer = exchange(
                Tracked { inner: stdout, dropped: read_dropped.clone() },
                Tracked { inner: stdin, dropped: write_dropped.clone() },
                lines_out, "test", &mut output,
            );
            tokio::pin!(transfer);
            assert!(poll!(&mut transfer).is_pending());
        }
        assert!(commands.is_closed());
        assert!(read_dropped.load(Ordering::SeqCst));
        assert!(write_dropped.load(Ordering::SeqCst));
    }

    /// The unpack script, run for real on a tar stream: whole, cut short, and beside built files
    #[cfg(unix)]
    #[test]
    fn a_host_install_cut_short_leaves_the_old_files_whole() {
        use std::io::Write;
        let base = std::env::temp_dir().join(format!("deductum-unpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let source = base.join("source");
        std::fs::create_dir_all(source.join("host")).unwrap();
        std::fs::create_dir_all(source.join("backend/Isabelle2025/src")).unwrap();
        // large enough for a cut to land inside the file
        let host = "# the host\n".repeat(30_000);
        std::fs::write(source.join("host/deductum_host.py"), &host).unwrap();
        std::fs::write(source.join("backend/Isabelle2025/src/backend.scala"), "new").unwrap();
        let tar = std::process::Command::new("tar").current_dir(&source).args(["-cf", "-", "host", "backend"]).output().unwrap();
        assert!(tar.status.success());
        let root = base.join("root");
        std::fs::create_dir_all(root.join("host")).unwrap();
        std::fs::create_dir_all(root.join("backend/Isabelle2025/lib")).unwrap();
        std::fs::write(root.join("host/deductum_host.py"), "old\n").unwrap();
        std::fs::write(root.join("backend/Isabelle2025/lib/built.jar"), "kept").unwrap();
        std::fs::write(root.join("VERSION"), "old\n").unwrap();
        let run = |stream: &[u8]| {
            let script = super::unpack_script(root.to_str().unwrap(), "", "new-version");
            let mut child = std::process::Command::new("/bin/sh")
                .args(["-c", &script])
                .stdin(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(stream).unwrap();
            child.wait().unwrap()
        };
        assert!(!super::unpack_script("~/x", "", "v").contains('\''), "it goes inside single quotes over ssh");
        // cut short inside the host file, as when a window closes: nothing changes
        let status = run(&tar.stdout[..64_512]);
        assert!(!status.success());
        assert_eq!(std::fs::read_to_string(root.join("host/deductum_host.py")).unwrap(), "old\n");
        assert_eq!(std::fs::read_to_string(root.join("VERSION")).unwrap(), "old\n");
        // whole: the new files, VERSION last, what was built there kept, nothing left over
        assert!(run(&tar.stdout).success());
        assert_eq!(std::fs::read_to_string(root.join("host/deductum_host.py")).unwrap(), host);
        assert_eq!(std::fs::read_to_string(root.join("backend/Isabelle2025/src/backend.scala")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(root.join("backend/Isabelle2025/lib/built.jar")).unwrap(), "kept");
        assert_eq!(std::fs::read_to_string(root.join("VERSION")).unwrap().trim(), "new-version");
        assert_eq!(std::fs::read_dir(&root).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(".install-")).count(), 0, "nothing is left over");
        // an install already cut short: its VERSION is not believed, so the next start repairs it
        let check = |size: u64| {
            let out = std::process::Command::new("/bin/sh").args(["-c", &super::installed_check_script(root.to_str().unwrap(), size)]).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(check(host.len() as u64), "new-version");
        std::fs::write(root.join("host/deductum_host.py"), &host[..64_512]).unwrap();
        assert_eq!(check(host.len() as u64), "");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    #[cfg(not(windows))]
    fn installing_the_host_registers_the_mcp_server_in_a_home_of_its_own() {
        use std::process::Command;
        // The script as install runs it, with the checkout's own host in place of an installed one,
        // and a temporary HOME: the real configuration is never read
        let root = env!("CARGO_MANIFEST_DIR");
        let script = super::register_mcp_script(root, "");
        assert_eq!(script, format!("python3 {root}/host/deductum_host.py register-mcp"));
        assert_eq!(super::register_mcp_script("~/.local/share/deductum", "DEDUCTUM_STATE_DIR=/r/state "), "DEDUCTUM_STATE_DIR=/r/state python3 ~/.local/share/deductum/host/deductum_host.py register-mcp");
        let home = std::env::temp_dir().join(format!("deductum-mcp-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".claude.json"), r#"{"mcpServers":{"other":{"type":"stdio","command":"x","args":[],"env":{}}}}"#).unwrap();
        std::fs::write(home.join(".codex/config.toml"), "model = \"m\"\n").unwrap();
        let run = || {
            let out = Command::new("sh").arg("-c").arg(&script).env("HOME", &home).env_remove("CODEX_HOME").output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
        };
        assert_eq!(run(), serde_json::json!({"claude": "added", "codex": "added"}));
        let claude: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(claude["mcpServers"]["other"]["command"], "x", "what was there stays");
        assert_eq!(claude["mcpServers"]["deductum"]["args"], serde_json::json!([format!("{root}/host/deductum_host.py"), "mcp"]));
        let codex = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.starts_with("model = \"m\"\n") && codex.contains("[mcp_servers.deductum]"), "{codex}");
        // a second install changes nothing
        let (c1, t1) = (std::fs::read_to_string(home.join(".claude.json")).unwrap(), std::fs::read_to_string(home.join(".codex/config.toml")).unwrap());
        assert_eq!(run(), serde_json::json!({"claude": "present", "codex": "present"}));
        assert_eq!((std::fs::read_to_string(home.join(".claude.json")).unwrap(), std::fs::read_to_string(home.join(".codex/config.toml")).unwrap()), (c1, t1));
        let _ = std::fs::remove_dir_all(&home);
        // and it is only here, not in the Windows build
        assert!(include_str!("link.rs").contains("#[cfg(not(windows))]\n    {\n        let script = register_mcp_script"));
    }

    #[test]
    fn terminal_agents_machines_parse() {
        let text = "{\n    (0): (\n        name: \"neo\",\n        host: \"neo\",\n        user: \"twright\",\n        hostname: \"\",\n        home: \"\",\n        host_id: \"\",\n    ),\n}";
        #[derive(serde::Deserialize, PartialEq, Eq, PartialOrd, Ord)]
        struct Id(u64);
        let m: std::collections::BTreeMap<Id, super::Machine> = ron::from_str(text).expect("parses");
        assert_eq!(m.into_values().next().unwrap().host, "neo");
    }
}
