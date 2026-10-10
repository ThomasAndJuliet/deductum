# cosmic-term
COSMIC terminal emulator, built using [alacritty\_terminal](https://docs.rs/alacritty_terminal) that is provided by the [alacritty](https://github.com/alacritty/alacritty) project. `cosmic-term` provides bidirectional rendering and ligatures with a custom renderer based on [cosmic-text](https://github.com/pop-os/cosmic-text).

The `wgpu` feature, enabled by default, supports GPU rendering using `glyphon`
and `wgpu`. If `wgpu` is not enabled or fails to initialize, then rendering falls
back to using `softbuffer` and `tiny-skia`.

## Color Schemes

Custom color schemes can be imported from the `View -> Color schemes...` menu item.
You can find templates for color schemes in the [color-schemes](color-schemes) folder.

## Agent threads

A Claude or Codex profile can open its conversations as rich-text threads
instead of terminals: the built-in **Claude thread** and **Codex thread**
profiles do. Replies are set with their LaTeX typeset, `chartgen` charts and
Mermaid diagrams drawn in the thread's colours, and text can be selected and
copied anywhere in the thread; images and diagrams open in their own windows.
**View → Maths typesetting** sets the thread as a paper, and **Settings →
Threads** holds its fonts and its memory limits. Threads run the `claude` and
`codex` command-line tools, so they use your subscriptions, and they run in a
relay that outlives the window: a turn finishes with the window closed, and
the window reopens its threads. See `AGENTS-PLAN.md`, "Threads".

## Fedora COSMIC Atomic

Install COSMIC Terminal Agents as a user Flatpak. Its terminals and agent helpers
run on the host, so install any agent command-line tools on the host as well.

Remote home-directory mounts require SSHFS. On Fedora COSMIC Atomic, install it
with `sudo rpm-ostree install fuse-sshfs`, then reboot into the new deployment.
Local terminals and worktrees do not require SSHFS. **File → Remote files**
checks the tools on this computer and each configured computer. It shows the
appropriate installation command, offers **Install tools…** to run it in a
terminal on the named computer, and explains when a reboot is needed. Return to
that page and use **Check again** after installation or reboot. Missing-tool
warnings also offer **Help set up**. SSH connections must already work with your
keys; the setup page helps diagnose connection failures.

The **Passwords and sudo** setup installs its PAM module in
`/usr/local/lib/cosmic-term` and its configuration in `/etc`. These locations are
writable on Atomic; setup works with SELinux enforcing and does not require
unlocking `/usr`. If an approval is unavailable or denied, sudo still uses the
system's password authentication.

## Flatpak builds and publication

Run `scripts/build-flatpak.sh` for a complete local build (`--install` to install it too).
The Flatpak module explicitly composes the Store catalog after installing its
MetaInfo, desktop entry, and icons, so module-only rebuilds include the catalog
as well. Do not publish a cached build after running only its Rust commands.
On neo, `neo-infrastructure/scripts/check-flatpak-catalog BUILD_DIR` checks the
finished build; the publisher also runs this check before signing or exporting.
A missing or invalid catalog stops publication instead of removing the app
from COSMIC Store.
