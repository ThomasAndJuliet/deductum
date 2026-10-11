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
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
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
        // No machines registered here: the fleet's, as COSMIC Terminal's mount helper lists them
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
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn local_name() -> String {
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
fn host_command(program: &str, args: &[String]) -> tokio::process::Command {
    if sandboxed() {
        let mut cmd = tokio::process::Command::new("flatpak-spawn");
        cmd.arg("--host").arg(program).args(args);
        cmd
    } else {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
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
        candidates.push(dir.join("../share/deductum"));
        candidates.push(dir.join("../../deductum"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("deductum"));
    candidates.into_iter().find(|p| p.join("host/deductum_host.py").is_file())
}

fn bundle_version(bundle: &std::path::Path) -> String {
    let mut hasher = std::hash::DefaultHasher::new();
    for file in [
        "host/deductum_host.py",
        "host/deductum_ot.py",
        "host/deductum_mcp.py",
        "backend/Isabelle2025/src/backend.scala",
        "backend/Isabelle2025/ML/deductum_prelude.ML",
        "backend/Isabelle2025/etc/build.props",
        "backend/Isabelle2025/etc/settings",
    ] {
        std::fs::read(bundle.join(file)).unwrap_or_default().hash(&mut hasher);
    }
    use std::hash::Hasher;
    format!("{:016x}", hasher.finish())
}

/// Where the host is installed on the target, and the environment it runs with: normally
/// `~/.local/share/deductum` and the user's state; `DEDUCTUM_REMOTE_ROOT` (for tests) installs
/// and runs a host of its own there, with its own state, beside the user's
fn remote_root() -> (String, String) {
    match std::env::var("DEDUCTUM_REMOTE_ROOT") {
        Ok(root) if !root.is_empty() => (root.clone(), format!("DEDUCTUM_STATE_DIR={root}/state ")),
        _ => ("~/.local/share/deductum".to_string(), String::new()),
    }
}

/// Install the bundled sources on the target when its copy differs
async fn install(target: &Target) -> Result<(), String> {
    let Some(bundle) = bundle() else { return Ok(()) };
    // Running from the installed copy itself: nothing to install
    let installed = home().join(".local/share/deductum");
    let (root, _) = remote_root();
    if target.destination.is_none()
        && root.starts_with('~')
        && std::fs::canonicalize(&bundle).ok().is_some_and(|b| std::fs::canonicalize(&installed).ok() == Some(b))
    {
        return Ok(());
    }
    let version = bundle_version(&bundle);
    let check = format!("cat {root}/VERSION 2>/dev/null || true");
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
    tar.arg("-C").arg(&bundle).args(["--exclude=__pycache__", "--exclude=lib", "--exclude=tests", "-cf", "-", "host", "backend"]);
    let archive = tar.output().map_err(|e| format!("tar: {e}"))?;
    if !archive.status.success() {
        return Err("could not pack the host sources".into());
    }
    let unpack = format!(
        "mkdir -p {root} && tar -C {root} -xf - && echo {version} > {root}/VERSION"
    );
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
    Ok(())
}

fn connect_command(target: &Target) -> tokio::process::Command {
    let (root, env) = remote_root();
    let script = format!("{env}python3 {root}/host/deductum_host.py connect");
    let mut cmd = match &target.destination {
        None => host_command("sh", &["-c".into(), script.clone()]),
        Some(dest) => host_command(
            "ssh",
            &["-o".into(), "BatchMode=yes".into(), "-o".into(), "ServerAliveInterval=15".into(), "-T".into(), dest.clone(), "sh".into(), "-c".into(), format!("'{script}'")],
        ),
    };
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    cmd
}

/// The link to `target`'s host, reconnecting after a pause when it drops
pub fn run(target: &Target) -> impl Stream<Item = (String, Event)> + use<> {
    let target = target.clone();
    cosmic::iced::stream::channel(64, move |mut output: Sender<(String, Event)>| async move {
        let key = target.key.clone();
        loop {
            let _ = output.send((key.clone(), Event::Connecting("installing".into()))).await;
            if let Err(err) = install(&target).await {
                let _ = output.send((key.clone(), Event::Disconnected(err))).await;
                tokio::time::sleep(Duration::from_secs(20)).await;
                continue;
            }
            let _ = output.send((key.clone(), Event::Connecting("connecting".into()))).await;
            let mut child = match connect_command(&target).spawn() {
                Ok(child) => child,
                Err(err) => {
                    let _ = output.send((key.clone(), Event::Disconnected(err.to_string()))).await;
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };
            let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
                continue;
            };
            let (to_host, mut lines_out) = mpsc::unbounded_channel::<String>();
            let _ = output.send((key.clone(), Event::Connected(to_host))).await;
            let stderr_task = tokio::spawn(async move {
                let mut tail = Vec::new();
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tail.push(line);
                    if tail.len() > 10 {
                        tail.remove(0);
                    }
                }
                tail.join("\n")
            });
            let mut lines = tokio::io::BufReader::with_capacity(1 << 20, stdout).lines();
            loop {
                tokio::select! {
                    line = lines.next_line() => match line {
                        Ok(Some(line)) => {
                            if let Ok(value) = serde_json::from_str(&line) {
                                let _ = output.send((key.clone(), Event::Line(value))).await;
                            }
                        }
                        _ => break,
                    },
                    out = lines_out.recv() => match out {
                        Some(mut line) => {
                            line.push('\n');
                            if stdin.write_all(line.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    },
                }
            }
            let _ = child.kill().await;
            let tail = stderr_task.await.unwrap_or_default();
            let reason = if tail.trim().is_empty() { "the connection closed".to_string() } else { tail };
            let _ = output.send((key.clone(), Event::Disconnected(reason))).await;
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
/// COSMIC Terminal's sshfs mount if it is remote), Zed (as an ssh project) or a terminal
/// (ssh'd there), the way Terminal Agents opens a tab's project
pub async fn open_folder(opener: Opener, target: Target, folder: String) -> Result<(), String> {
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
            // Terminal Agents if it is installed, else COSMIC Terminal, else any terminal
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
    #[test]
    fn terminal_agents_machines_parse() {
        let text = "{\n    (0): (\n        name: \"neo\",\n        host: \"neo\",\n        user: \"twright\",\n        hostname: \"\",\n        home: \"\",\n        host_id: \"\",\n    ),\n}";
        #[derive(serde::Deserialize, PartialEq, Eq, PartialOrd, Ord)]
        struct Id(u64);
        let m: std::collections::BTreeMap<Id, super::Machine> = ron::from_str(text).expect("parses");
        assert_eq!(m.into_values().next().unwrap().host, "neo");
    }
}
