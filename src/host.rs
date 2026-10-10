// SPDX-License-Identifier: GPL-3.0-only

//! Running programs on the host when the terminal is sandboxed by Flatpak.
//!
//! Inside a Flatpak, the runtime has none of the user's shells or tools, so
//! every child runs on the host through `flatpak-spawn --host`. The sandbox has
//! its own PID namespace, so a tab's host processes are found by the
//! `COSMIC_TERM_TAB` variable each tab sets, not by PID.

use alacritty_terminal::tty;
use std::{
    hash::{BuildHasher, Hasher, RandomState},
    path::Path,
    sync::LazyLock,
};

/// Set in each tab's environment to a token unique to the tab
pub const TAB_VAR: &str = "COSMIC_TERM_TAB";

static SANDBOXED: LazyLock<bool> = LazyLock::new(|| Path::new("/.flatpak-info").exists());

pub fn is_sandboxed() -> bool {
    *SANDBOXED
}

/// A new token for [`TAB_VAR`]
pub fn tab_token() -> String {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    format!("{:016x}", hasher.finish())
}

/// A program and its arguments, to run in a tab
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl HostCommand {
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }

    /// Split a command line, or `None` if it is empty
    pub fn parse(command_line: &str) -> Option<Self> {
        let mut argv = shlex::split(command_line)?;
        if argv.is_empty() {
            return None;
        }
        let program = argv.remove(0);
        Some(Self::new(program, argv))
    }
}

/// Set a tab's command, tagging it with the tab's token. When sandboxed the
/// command is sent to the host, with `None` meaning the user's login shell
pub fn tab_options(
    mut options: tty::Options,
    command: Option<HostCommand>,
    token: &str,
) -> tty::Options {
    options.env.insert(TAB_VAR.to_string(), token.to_string());
    if !is_sandboxed() {
        // `tty::new` waits for its child to chdir before returning. An
        // inherited directory on an offline sshfs mount can therefore freeze
        // the entire UI. Let the child shell do that chdir after spawn.
        options.shell = match (options.working_directory.take(), command) {
            (Some(dir), Some(command)) => {
                let mut args = vec![
                    "-c".to_string(),
                    "cd \"$1\" || cd; shift; exec \"$@\"".to_string(),
                    "sh".to_string(),
                    dir.display().to_string(),
                    command.program,
                ];
                args.extend(command.args);
                Some(tty::Shell::new("sh".to_string(), args))
            }
            (Some(dir), None) => Some(tty::Shell::new(
                "sh".to_string(),
                vec![
                    "-c".to_string(),
                    "cd \"$1\" || cd; exec \"${SHELL:-/bin/sh}\" -l".to_string(),
                    "sh".to_string(),
                    dir.display().to_string(),
                ],
            )),
            (None, command) => command.map(|c| tty::Shell::new(c.program, c.args)),
        };
        return options;
    }

    let mut args = vec!["--host".to_string(), "--watch-bus".to_string()];
    // The host process gets the portal's environment, not ours
    let mut env: Vec<(String, String)> = options.env.drain().collect();
    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("COLORTERM".into(), "truecolor".into()));
    env.sort();
    for (key, value) in env {
        args.push(format!("--env={key}={value}"));
    }
    // The directory is a host path, which may not exist in the sandbox
    if let Some(dir) = options.working_directory.take() {
        args.push(format!("--directory={}", dir.display()));
    }
    match command {
        Some(command) => {
            args.push(command.program);
            args.extend(command.args);
        }
        // The runtime's $SHELL is not the user's, so ask the host
        None => args.extend([
            "sh".to_string(),
            "-c".to_string(),
            "shell=$(getent passwd \"$(id -un)\" | cut -d: -f7)\n\
             exec \"${shell:-/bin/sh}\" -l"
                .to_string(),
        ]),
    }
    // Start through ourselves, to hand the terminal over; see `host_exec`
    let exe = std::env::current_exe()
        .map(|exe| exe.display().to_string())
        .unwrap_or_else(|_| "cosmic-term".to_string());
    args.insert(0, HOST_EXEC_ARG.to_string());
    options.shell = Some(tty::Shell::new(exe, args));
    options
}

/// The argument that makes this program run [`host_exec`]
pub const HOST_EXEC_ARG: &str = "--host-exec";

/// Run `flatpak-spawn` with `args`, first giving up our controlling terminal.
///
/// The terminal starts its child as the leader of a session that controls
/// the tab's pty. While the pty belongs to that session, the host process
/// cannot take it, and would run without job control, Ctrl+C or resize
/// signals. Once released, Flatpak's session helper makes it the controlling
/// terminal of the host process, as if the host had started it.
pub fn host_exec(args: Vec<std::ffi::OsString>) -> std::io::Error {
    // SAFETY: plain syscalls on this process, before any threads start
    unsafe {
        // Releasing the terminal hangs up our own process group
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::ioctl(0, libc::TIOCNOTTY);
        libc::signal(libc::SIGHUP, libc::SIG_DFL);
    }
    use std::os::unix::process::CommandExt;
    std::process::Command::new("flatpak-spawn").args(args).exec()
}

/// Whether `path` exists on the host. A mount made on the host after the
/// sandbox started may not be visible inside it
pub async fn path_exists(path: &Path) -> bool {
    let mut command = command("test");
    command.arg("-e").arg(path);
    command.status().await.is_ok_and(|status| status.success())
}

/// A command that runs `program` on the host
pub fn command(program: &str) -> tokio::process::Command {
    if is_sandboxed() {
        let mut command = tokio::process::Command::new("flatpak-spawn");
        command.args(["--host", "--watch-bus", program]);
        command
    } else {
        tokio::process::Command::new(program)
    }
}

/// Like [`command`], for code that blocks
pub fn std_command(program: &str) -> std::process::Command {
    if is_sandboxed() {
        let mut command = std::process::Command::new("flatpak-spawn");
        command.args(["--host", "--watch-bus", program]);
        command
    } else {
        std::process::Command::new(program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_lines() {
        assert_eq!(
            HostCommand::parse("ssh -t 'a b'"),
            Some(HostCommand::new("ssh", vec!["-t".into(), "a b".into()]))
        );
        assert_eq!(HostCommand::parse("  "), None);
    }

    #[test]
    fn tags_tabs_with_their_token() {
        let options = tab_options(tty::Options::default(), None, "t1");
        // In the environment, or, from the Flatpak, passed to the host
        let tagged = options.env.get(TAB_VAR).map(String::as_str) == Some("t1")
            || options
                .shell
                .as_ref()
                .is_some_and(|shell| format!("{shell:?}").contains(&format!("--env={TAB_VAR}=t1")));
        assert!(tagged, "{options:?}");
    }
}
