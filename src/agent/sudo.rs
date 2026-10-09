// SPDX-License-Identifier: GPL-3.0-only

//! The machines' side of sudo approvals (see `AGENTS-PLAN.md`, "Sudo from
//! agents"): `ct-sudo-setup` and the PAM module on every machine, the agent
//! and its systemd user units on this one, and the agent's socket forwarded
//! to each remote machine the windows show, so that its sudo can ask here.

use super::{CONFIG_DIR, Helper, ensure_helper, helper_name, run, session_exec, sh_quote, ssh_options};

const SETUP: &str = include_str!("ct-sudo-setup.sh");

/// Where `ct-sudo-setup` is on every machine, as a shell word
pub const SETUP_COMMAND: &str = "\"$HOME/.config/cosmic-term-agents/ct-sudo-setup\"";

/// Install or update `ct-sudo-setup` and the PAM module on a host, or on
/// this computer if it is empty
pub async fn install_setup(host: String) -> Result<(), String> {
    let script = format!(
        "d={CONFIG_DIR}\n\
         mkdir -p \"$d\"\n\
         cat > \"$d/ct-sudo-setup.tmp\" <<'CT_SUDO_EOF'\n{SETUP}CT_SUDO_EOF\n\
         chmod 755 \"$d/ct-sudo-setup.tmp\"\n\
         if cmp -s \"$d/ct-sudo-setup.tmp\" \"$d/ct-sudo-setup\"; then rm \"$d/ct-sudo-setup.tmp\"; \
         else mv \"$d/ct-sudo-setup.tmp\" \"$d/ct-sudo-setup\"; fi\n"
    );
    run(session_exec(&host, &script)).await?;
    ensure_helper(host, Helper::PamModule).await
}

/// The agent and its units on this computer, started by systemd when a
/// window or a PAM module connects. A new build replaces the running agent
pub async fn install_agent() -> Result<(), String> {
    ensure_helper(String::new(), Helper::SudoAgent).await?;
    let Some(agent) = helper_name(Helper::SudoAgent) else {
        return Err("this build has no ct-sudo-agent".to_string());
    };
    let service = format!(
        "[Unit]\n\
         Description=COSMIC Terminal's sudo agent\n\
         \n\
         [Service]\n\
         ExecStart=%h/.config/cosmic-term-agents/{agent}\n"
    );
    let socket = |name: &str, path: &str| {
        format!(
            "[Unit]\n\
             Description=COSMIC Terminal's sudo agent ({name})\n\
             \n\
             [Socket]\n\
             ListenStream={path}\n\
             SocketMode=0600\n\
             DirectoryMode=0700\n\
             FileDescriptorName={name}\n\
             Service=ct-sudo-agent.service\n\
             \n\
             [Install]\n\
             WantedBy=sockets.target\n"
        )
    };
    let pam = socket("pam", &format!("%t/{}", ct_sudo_protocol::SOCKET));
    let control = socket(
        "control",
        &format!("%t/app/{}/sudo-control.sock", cosmic_term::sudo_control::APP_ID),
    );
    let mut script = String::from(
        "u=\"${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user\"\n\
         mkdir -p \"$u\"\n\
         changed=\n\
         put() {\n\
         \tif ! printf '%s' \"$2\" | cmp -s - \"$u/$1\"; then printf '%s' \"$2\" > \"$u/$1\"; changed=1; fi\n\
         }\n",
    );
    for (unit, text) in [
        ("ct-sudo-agent.service", service),
        ("ct-sudo-agent.socket", pam),
        ("ct-sudo-agent-control.socket", control),
    ] {
        script.push_str(&format!("put {unit} {}\n", sh_quote(&text)));
    }
    script.push_str(
        "if [ -n \"$changed\" ]; then\n\
         \tsystemctl --user daemon-reload\n\
         \tsystemctl --user stop ct-sudo-agent.service 2>/dev/null\n\
         \tsystemctl --user restart ct-sudo-agent.socket ct-sudo-agent-control.socket 2>/dev/null\n\
         fi\n\
         systemctl --user enable --now ct-sudo-agent.socket ct-sudo-agent-control.socket 2>&1\n",
    );
    run(session_exec("", &script)).await.map(|_| ())
}

/// Forward this computer's agent socket to the same path on a host, over
/// the shared ssh connection, unless a live one is there already. Done
/// again from time to time, since the shared connection, and the forward
/// with it, go when nothing has used it for a while
pub async fn forward(host: String) -> Result<(), String> {
    // A dead socket left by an earlier forward would block this one
    let prepare = format!(
        "p=/run/user/$(id -u)/{socket}\n\
         if [ -S \"$p\" ]; then\n\
         \tif python3 -c 'import socket,sys; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])' \"$p\" 2>/dev/null; then\n\
         \t\techo live; exit 0\n\
         \tfi\n\
         \trm -f \"$p\"\n\
         fi\n\
         echo \"$p\"\n",
        socket = ct_sudo_protocol::SOCKET
    );
    let remote = run(session_exec(&host, &prepare)).await?;
    let remote = remote.trim();
    if remote == "live" || remote.is_empty() {
        return Ok(());
    }
    // SAFETY: a plain call
    let local = ct_sudo_protocol::socket_path(unsafe { libc::getuid() });
    let mut command = crate::host::command("ssh");
    command
        .args(ssh_options())
        .args(["-O", "forward", "-R"])
        .arg(format!("{remote}:{}", local.display()))
        .arg(&host);
    run(command).await.map(|_| ())
}

/// Prepare a host to be set up, and return the command that sets it up,
/// for a tab on it to run under sudo: `ct-sudo-setup` and the module are
/// installed, and the public key is written beside them
pub async fn prepare_setup(host: String, public: String) -> Result<String, String> {
    install_setup(host.clone()).await?;
    let Some(module) = helper_name(Helper::PamModule) else {
        return Err("this build has no PAM module".to_string());
    };
    let script = format!(
        "printf '%s\\n' {} > {CONFIG_DIR}/sudo-key.pub\n",
        sh_quote(&public)
    );
    run(session_exec(&host, &script)).await?;
    Ok(format!(
        "sudo {SETUP_COMMAND} install {CONFIG_DIR}/{module} {CONFIG_DIR}/sudo-key.pub"
    ))
}
