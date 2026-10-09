// SPDX-License-Identifier: GPL-3.0-only

//! The machines' files mounted on each other, with sshfs, by `ct-mount`
//! (`ct-mount.sh`), which the terminal installs on every machine it works
//! with. This computer mounts a remote machine when something here first
//! needs its files, and the remote machine mounts this one when one of its
//! agents asks, using a way back here found when the terminal starts.
//!
//! A mount is kept while anything on its machine needs it: a running session,
//! or a lease. A window holds a lease, a locked file, on each machine it shows
//! a tab or launcher for, so a file chooser or file manager opened there finds
//! the mount in place. Whatever needs it no more is unmounted by `ct-mount
//! --gc`, which runs in the background when a lease is let go, when a session
//! ends, and with every sidebar scan. Nothing here blocks the interface.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::fd::AsRawFd,
    path::PathBuf,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use super::{run, session_exec, sh_quote};

const SCRIPT: &str = include_str!("ct-mount.sh");
const REMOTE_MCP: &str = include_str!("ct-remote-mcp.py");

/// Where `ct-mount` is on every machine, as a shell word
pub const COMMAND: &str = "\"$HOME/.config/cosmic-term-agents/ct-mount\"";

/// How long a lease that could not be taken waits before another try
const LEASE_RETRY: Duration = Duration::from_secs(15);

/// Install or update `ct-mount` on a host, or on this computer if it is empty
pub async fn install(host: String) -> Result<(), String> {
    let script = format!(
        "d=\"$HOME/.config/cosmic-term-agents\"\n\
         mkdir -p \"$d\"\n\
         cat > \"$d/ct-mount.tmp\" <<'CT_MOUNT_EOF'\n{SCRIPT}CT_MOUNT_EOF\n\
         chmod 755 \"$d/ct-mount.tmp\"\n\
         if cmp -s \"$d/ct-mount.tmp\" \"$d/ct-mount\"; then rm \"$d/ct-mount.tmp\"; \
         else mv \"$d/ct-mount.tmp\" \"$d/ct-mount\"; fi\n\
         cat > \"$d/ct-remote-mcp.tmp\" <<'CT_MCP_EOF'\n{REMOTE_MCP}CT_MCP_EOF\n\
         chmod 755 \"$d/ct-remote-mcp.tmp\"\n\
         if cmp -s \"$d/ct-remote-mcp.tmp\" \"$d/ct-remote-mcp\"; then rm \"$d/ct-remote-mcp.tmp\"; \
         else mv \"$d/ct-remote-mcp.tmp\" \"$d/ct-remote-mcp\"; fi\n"
    );
    run(session_exec(&host, &script)).await.map(|_| ())
}

/// This computer's own install, made once
async fn install_here() -> Result<(), String> {
    static DONE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    DONE.get_or_try_init(|| install(String::new())).await.map(|_| ())
}

/// A machine's name as a mount's directory: letters, digits and `._@-`
pub fn sanitize(name: &str) -> String {
    let name: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._@-".contains(c) { c } else { '-' })
        .collect();
    name.trim_start_matches('.').to_string()
}

/// The machines' host names, by ssh destination: the registrations of
/// `ct-mount` here, read once, and those `connect` makes
static NAMES: LazyLock<Mutex<Option<BTreeMap<String, String>>>> = LazyLock::new(Mutex::default);

/// `ct-mount`'s registrations on this computer, as destination → name. They
/// are the host's, which a sandbox reaches under the same home
fn registrations() -> BTreeMap<String, String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let dir = home.join(".local/state/cosmic-term/mounts");
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if let (Some(name), Ok(dest)) = (
            path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".dest")),
            fs::read_to_string(&path),
        ) {
            found.insert(dest.trim().to_string(), name.to_string());
        }
    }
    found
}

/// The mount name of a host: its host name, else the destination itself
pub fn name_for(host: &str) -> String {
    let mut names = NAMES.lock().unwrap();
    names
        .get_or_insert_with(registrations)
        .get(host)
        .cloned()
        .unwrap_or_else(|| sanitize(host))
}

/// This computer's name, as other machines mount it
pub fn local_name() -> String {
    let name = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    sanitize(&name)
}

/// Ways other machines may reach this one over ssh, the likeliest first
async fn local_destinations() -> Vec<String> {
    let host = local_name();
    let user = std::env::var("USER").unwrap_or_default();
    let tailscale = run(session_exec("", "tailscale status --json 2>/dev/null || true\n"))
        .await
        .ok()
        .and_then(|json| crate::machines::parse_tailscale(&json))
        .map(|(me, _)| me.dns_name)
        .unwrap_or_default();
    let mut found = Vec::new();
    for name in [host.as_str(), tailscale.as_str()] {
        if name.is_empty() {
            continue;
        }
        found.push(name.to_string());
        if !user.is_empty() {
            found.push(format!("{user}@{name}"));
        }
    }
    found.dedup();
    found
}

/// Let this computer and a remote machine mount each other: install
/// `ct-mount` on both, record the machine here by its host name, and record
/// this computer there by the first destination that works from there.
/// Nothing is mounted until it is asked for
pub async fn connect(host: String) -> Result<(), String> {
    install_here().await?;
    install(host.clone()).await?;
    let name = sanitize(run(session_exec(&host, "cat /proc/sys/kernel/hostname\n")).await?.trim());
    if name.is_empty() {
        return Err(format!("{host} has no host name"));
    }
    NAMES
        .lock()
        .unwrap()
        .get_or_insert_with(registrations)
        .insert(host.clone(), name.clone());
    let register = format!(
        "{COMMAND} --register {} {}\n",
        sh_quote(&name),
        sh_quote(&host)
    );
    run(session_exec("", &register)).await?;
    let local = local_name();
    let candidates: Vec<String> = local_destinations()
        .await
        .iter()
        .map(|c| sh_quote(c))
        .collect();
    if local.is_empty() || candidates.is_empty() {
        return Err("this computer has no name to be reached by".to_string());
    }
    // Known host keys only: an unknown one is for the user to accept
    let back = format!(
        "for c in {}; do\n\
         \tif ssh -n -o BatchMode=yes -o ConnectTimeout=5 \"$c\" true 2>/dev/null; then\n\
         \t\texec {COMMAND} --register {} \"$c\"\n\
         \tfi\n\
         done\n\
         echo \"cannot reach {local} from $(cat /proc/sys/kernel/hostname) over ssh\" >&2\n\
         exit 1\n",
        candidates.join(" "),
        sh_quote(&local),
    );
    run(session_exec(&host, &back)).await.map(|_| ())
}

/// Where a host's files are mounted here, mounting them first if needed
pub async fn mount(host: String) -> Result<PathBuf, String> {
    install_here().await?;
    let name = name_for(&host);
    let script = format!(
        "{COMMAND} --register {name} {host} && exec {COMMAND} {name}\n",
        name = sh_quote(&name),
        host = sh_quote(&host),
    );
    let output = run(session_exec("", &script)).await?;
    Ok(PathBuf::from(output.trim_end()))
}

/// The line a session script ends with, to unmount in the background what
/// nothing needs any more: after a moment, since the session is still
/// running then
pub fn collect_later() -> String {
    format!(
        "[ -x {COMMAND} ] && setsid -f sh -c 'sleep 3; exec \"$0\" --gc' {COMMAND} >/dev/null 2>&1 </dev/null\n"
    )
}

enum Lease {
    Taking,
    Held { file: fs::File, path: PathBuf },
    Failed(Instant),
}

/// This window's leases, by machine name
static LEASES: LazyLock<Mutex<BTreeMap<String, Lease>>> = LazyLock::new(Mutex::default);

/// This window, among the holders of a lease
static WINDOW: LazyLock<String> = LazyLock::new(|| cosmic_term::random_hex(12));

/// Hold leases on the mounts of exactly these machines. Cheap when nothing
/// changed, so it can be called with every redraw; taking a lease and
/// letting one go happen on a thread of their own
pub fn set_needed(names: &BTreeSet<String>) {
    let mut leases = LEASES.lock().unwrap();
    let gone: Vec<String> = leases
        .keys()
        .filter(|name| !names.contains(*name))
        .cloned()
        .collect();
    let mut released = false;
    for name in gone {
        if let Some(Lease::Held { file, path }) = leases.remove(&name) {
            let _ = fs::remove_file(&path);
            drop(file);
            released = true;
        }
    }
    if released {
        std::thread::spawn(|| {
            let _ = crate::host::std_command("timeout")
                .args(["-k", "5s", "30s", "sh", "-c", &format!("{COMMAND} --gc")])
                .output();
        });
    }
    for name in names {
        let due = match leases.get(name) {
            None => true,
            Some(Lease::Failed(at)) => at.elapsed() >= LEASE_RETRY,
            Some(_) => false,
        };
        if !due {
            continue;
        }
        leases.insert(name.clone(), Lease::Taking);
        let name = name.clone();
        std::thread::spawn(move || {
            let taken = take(&name);
            let mut leases = LEASES.lock().unwrap();
            match (taken, leases.get(&name)) {
                (Ok((file, path)), Some(Lease::Taking)) => {
                    leases.insert(name, Lease::Held { file, path });
                }
                // Let go while being taken
                (Ok((_file, path)), _) => {
                    let _ = fs::remove_file(path);
                }
                (Err(err), Some(Lease::Taking)) => {
                    log::debug!("no lease on the mount of {name} yet: {err}");
                    leases.insert(name, Lease::Failed(Instant::now()));
                }
                (Err(_), _) => {}
            }
        });
    }
}

/// A locked lease file for this window on a machine's mount
fn take(name: &str) -> Result<(fs::File, PathBuf), String> {
    // The lease directory is the host's, which a sandbox sees elsewhere
    let output = crate::host::std_command("timeout")
        .args(["-k", "5s", "30s", "sh", "-c", &format!("{COMMAND} --lease {}", sh_quote(name))])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        if output.status.code() == Some(124) {
            return Err("taking the mount lease timed out".into());
        }
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let path = dir.join(WINDOW.as_str());
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    // SAFETY: a plain syscall on a descriptor we own
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(format!(
            "{}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok((file, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe_as_directories() {
        assert_eq!(sanitize("neo"), "neo");
        assert_eq!(sanitize("My laptop/2"), "My-laptop-2");
        assert_eq!(sanitize("..evil"), "evil");
        assert_eq!(sanitize("twright@neo"), "twright@neo");
    }

    #[test]
    fn the_script_installs_through_a_here_document() {
        assert!(SCRIPT.ends_with('\n'));
        assert!(!SCRIPT.lines().any(|line| line == "CT_MOUNT_EOF"));
    }
}
