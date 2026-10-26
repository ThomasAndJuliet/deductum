# Deductum

Deductum is the household application for shared Isabelle and Lean proof work.
It replaces Isabellum and publishes privately to home-apps. Its standalone
source repository is `ThomasAndJuliet/deductum`; development starts from the
complete `multi-prover` source history, preserving the vendored COSMIC toolkit.
Tom and Juliet administer this repository through the household-admins team.

## CI and private releases

Every push and pull request runs `.github/workflows/ci.yml`. The SDK job checks
all targets and tests the complete Rust workspace with all features, including
Deductum's headless UI/unit tests, and runs all Python host unit-test modules:

```sh
cargo --offline --locked test --workspace --all-targets --all-features
python3 -m unittest discover -s deductum/host/tests -p "test_*.py"
```

The app opts into concurrent proof-state and hover/definition reads with
`concurrent_reads: true` in its `hello` request. The host dispatches these reads
independently of ordered edits and control requests, allowing four pending
reads per opted-in connection; excess requests receive `busy`. Legacy clients
retain serial dispatch. Concurrent replies carry their request IDs and may
arrive out of order. A read whose document or prover changed receives
`stale_snapshot` instead of offsets labelled with a newer revision. Closing the
input half of a connection still drains pending replies.

The app reads host events and writes commands independently, so a full UI event
queue does not prevent outgoing edits or cancellation. Views share one prover
report mapped to the local text. Prover progress retains unchanged prose maps,
rendered documents and highlighting; source edits explicitly refresh them.
Current proof-state requests retry after saturation or stale results, and
delayed navigation replies retain their source context through file reads.
The focused regressions are:

```sh
python3 -m unittest discover -s deductum/host/tests -p "test_responsiveness.py"
cargo --offline --locked test --bin deductum link::tests
cargo --offline --locked test --bin deductum prover_view_tests
```

A separate hosted job installs Lean 4.19.0 and the checksum-pinned official
Isabelle2025 distribution. It runs the existing real backend integrations:

```sh
python3 deductum/host/tests/integration_lean.py --skip-dsrv \
  --core-fixture "$PWD/scripts/ci-fixtures/lean"
DEDUCTUM_ISABELLE=/path/to/Isabelle2025/bin/isabelle \
  DEDUCTUM_STATE_DIR=/disposable/state \
  python3 deductum/host/tests/integration_host.py
```

The Lean driver runs its generated tiny project and the existing host-core
assertions run against a self-contained checked-in fixture. CI needs no personal
research checkout, SSH access, agent login or server credential. Existing opt-in
live UI, mixed-machine, agent and research-project acceptance harnesses remain
separate; the unattended suite reports ignored tests rather than claiming those
live runs occurred. Prover component/state changes affect only the disposable
hosted runner.

After the SDK tests, CI builds `deductum` in Freedesktop SDK 25.08 and packages
that exact tested checkout. It validates desktop/AppStream/catalog/icons and
records its Git SHA, run ID and archive digest. Dependency sources, including
chartgen revision `50b5b9773dbcbd1e8c9975c9bc3e107f00496bb6`, are vendored in this
repository; no sibling checkout or private dependency credentials are needed.
The successful default-branch push preserves its artifact for seven days.
The release workflow waits for the entire CI workflow, including both prover
integrations, to succeed before submitting to Neo's trusted reusable publisher.
Other branches and pull requests do not publish stable.

CI cancels superseded runs for the same pull request or feature branch to save
hosted runner minutes. Each push to `main` uses its own concurrency group and
never automatically cancels another release-branch push, including a pending
run. Every passing default-branch push retains its publication opportunity.

Neo independently verifies the repository, source, successful CI run and immutable
artifact before its offline publisher signs the private stable release. GitHub
holds no signing key. The repository variable `NEO_CI_MINIMUM_RUN_NUMBER` and Neo's
matching policy must be set before enabling publication. Missed successful releases
are retried twice daily; signed source provenance is the acceptance criterion.

Local Flatpak builds use `scripts/build-flatpak.sh`. To reproduce the SDK tests,
run `python3 scripts/prepare-ci-manifest.py` and build the generated
`packaging/flatpak/dev.twright.Deductum.ci.yml` manifest. Frozen Cargo downloads
are regenerated from Cargo.lock with `flatpak-cargo-generator.py`, followed by
`scripts/fix-cargo-sources-ssh.py` for the libcosmic fork, which is fetched over
ssh:

    flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
    python3 scripts/fix-cargo-sources-ssh.py packaging/flatpak/cargo-sources.json

## Existing sessions

Deductum has its own settings, host bundle and state. Installing or publishing
it does not delete Isabellum or migrate sessions. The optional explicit
`migrate-from-isabellum` command requires the Isabellum host to be stopped first;
never allow two hosts to autosave the same session folder.

The preserved terminal-family documentation follows for shared internals.

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
