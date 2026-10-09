# cosmic-term
COSMIC terminal emulator, built using [alacritty\_terminal](https://docs.rs/alacritty_terminal) that is provided by the [alacritty](https://github.com/alacritty/alacritty) project. `cosmic-term` provides bidirectional rendering and ligatures with a custom renderer based on [cosmic-text](https://github.com/pop-os/cosmic-text).

The `wgpu` feature, enabled by default, supports GPU rendering using `glyphon`
and `wgpu`. If `wgpu` is not enabled or fails to initialize, then rendering falls
back to using `softbuffer` and `tiny-skia`.

## Color Schemes

Custom color schemes can be imported from the `View -> Color schemes...` menu item.
You can find templates for color schemes in the [color-schemes](color-schemes) folder.

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
