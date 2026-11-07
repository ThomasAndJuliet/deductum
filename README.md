# Deductum

Deductum is the household application for shared Isabelle and Lean proof work.
It replaces Isabellum and publishes privately to home-apps. Its standalone
source repository is `ThomasAndJuliet/deductum`; development starts from the
complete `multi-prover` source history, preserving the vendored COSMIC toolkit.
Tom and Juliet administer this repository through the household-admins team.

## Native Windows

The Windows x64 build runs the GUI, session host and local provers natively.
It also connects to Linux proof hosts over Windows OpenSSH. Local Lean uses
elan; local Isabelle uses the official Isabelle2025 Windows distribution and
its bundled Cygwin. WSL is not required. Git must be on PATH for Git project
actions; SSH connections also require the Windows `ssh` and `tar` commands.

Isabelle2025's bundled Java narrows some Unicode command-line paths. The
adapter uses existing Windows short-path aliases for those CLI arguments and
keeps original document paths in the protocol. On a volume without usable
short names, an error identifies the path that must use an ASCII location
for that Isabelle operation. Lean and Linux remote connections keep their
native Unicode paths. Local Isabelle logic names and command-line option
values must use ASCII.

The backend builds in a protected cache under the state directory, so the
portable package can live in a path containing emoji. The bundled JDK cannot
load jars from canonical paths containing supplementary Unicode characters.
Keep Isabelle's own installation and the state/cache location in paths without
emoji or other supplementary characters; Greek letters such as β and Ω work.
An unsupported cache location gives `DEDUCTUM_STATE_DIR` guidance. Cached components are keyed by their source contents and retained
across updates.

Extract the complete private portable ZIP into a new directory and run
`deductum.exe`. Python is included. Keep its `python` and `share` directories
beside the executable. Add remote machines to
`%APPDATA%\deductum\machines.json`:

```json
[{"name":"proof-server","host":"proof-server","user":"my-user"}]
```

Configure authentication and host trust with the normal Windows SSH client.
Open a local project with `deductum.exe C:\Proofs\Example`, or a server project
with `deductum.exe --machine proof-server /srv/proofs/Example`. Remote paths
are interpreted on the server. Settings live in `%APPDATA%\deductum` and
session/recovery data in `%LOCALAPPDATA%\deductum\state`.

Closing a window leaves sessions running. To update, close the windows, run
the old package's `stop-host.cmd` to flush and stop its host, then start the
new package from a different directory. A package never silently replaces a
running host from another version. Keep the previous package until the update
has been checked; state lives outside both package directories.

Cross-build and package on Linux with Rust1.99 and MinGW-w64:

```sh
rustup target add x86_64-pc-windows-gnu
cargo --locked build --release --bin deductum --no-default-features \
  --features wgpu --target x86_64-pc-windows-gnu
python3 scripts/test-package-windows.py
python3 scripts/package-windows.py
```

Release packaging requires a clean Git checkout and a binary stamped with its
exact commit. The script verifies the pinned official embedded Python download
and writes an immutable ZIP plus a checksum/provenance receipt under
`dist/windows`. Use `--allow-dirty` only for explicitly marked development
packages. `deductum.exe --build-info` reports the embedded source identity.

Windows CI executes the shared Python host suite, the cross-built Rust app
tests and both existing real prover integrations. The portable package's
interpreter runs them with `-X utf8`; embedded Python ignores `PYTHONUTF8`.
`scripts/install-windows-test-provers.ps1` installs the pinned native test
dependencies into a disposable runner directory. The Windows VM acceptance
also runs as a standard user with a second account for isolation checks,
and covers visible GUI use and Windows-to-Linux SSH connections. Tests that
need disposable second-account credentials report an explicit skip when those
fixtures are unavailable; CI does not claim that this replaces VM acceptance.
The hosted Windows gate creates disposable standard accounts and runs the
same suites with a private temporary directory and isolated configuration.
Its PowerShell 7 command is `./scripts/run-windows-native-ci.ps1`; the default
gate runs the complete host and Rust suites plus both real prover integrations.

Lean reports a file as checked after its final diagnostics arrive for that
document revision, so completion includes its warnings and errors.

## CI and private releases

Every push and pull request runs `.github/workflows/ci.yml`. The SDK job checks
all targets and tests the complete Rust workspace with all features, including
Deductum's headless UI/unit tests, and runs all Python host unit-test modules:

```sh
cargo --offline --locked test --workspace --all-targets --all-features
python3 -m unittest discover -s deductum/host/tests -p "test_*.py"
python3 scripts/test-package-windows.py
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
The host gives each real socket a bounded output queue. Replaceable prover
snapshots can coalesce between lossless message fences; edits and replies are
never dropped. Slow clients disconnect and resynchronise instead of blocking
other clients. Recovery journals and routine project reads, listings, watches and autosaves use separate
bounded worker lanes. Edit acknowledgements still wait for journal completion,
while opted-in connections can dispatch Stop during that wait. Autosave keeps
an immutable revision and checks disk identity before writing; an older save
cannot clear a newer edit's dirty state.

Long literate pages retain block heights and construct widget trees near the
viewport, with the active editor and selected navigation target pinned. Small
pages retain exact eager layout. Height corrections preserve the visible source
anchor. The shared `prose-render` crate owns retained block geometry, indexed
viewport/hit queries, exact Markdown provenance and immutable prepared resource
and layout APIs. The app owns source revisions, edits, selection policy, undo
and prover state. Preparing resources/layout on workers is explicit; existing
synchronous callers remain supported.

The focused regressions are:

```sh
python3 -m unittest discover -s deductum/host/tests -p "test_responsiveness.py"
python3 -m unittest discover -s deductum/host/tests -p "test_background_io.py"
cargo --offline --locked test --bin deductum link::tests
cargo --offline --locked test --bin deductum prover_view_tests
cargo --offline --locked test --bin deductum ui_responsiveness
cargo --offline --locked test -p prose-render
```

Machine-local `~/.config/deductum/host.json` can set `max_running_provers`,
`memory_budget_gb`, `prover_start_reserve_mb`, `prover_threads` and
`scratch_checks`. Admission includes starting sessions before their RSS is
available, using the reservation (256 MB by default). Optional thread and job
limits apply to new backend work; existing session options remain available.
Process sampling and configuration refresh run off the host event loop. These
settings do not change machine policy or stop existing sessions. Cold session resolution, independent fork/merge and git/discard cleanup still perform some synchronous filesystem work.

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
The release workflow waits for the entire CI workflow, including Linux and
Windows prover integrations, to succeed before submitting to Neo's trusted reusable publisher.
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
`scripts/fix-cargo-sources-ssh.py` for the libcosmic fork and household-kit (its
household-tones crate), which are fetched over ssh:

    flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
    python3 scripts/fix-cargo-sources-ssh.py packaging/flatpak/cargo-sources.json

## Text interaction and recovery

Editor input keeps its source or proof-window origin and waits for fresh layout
when a cell or its displayed markup changes. Clipboard and page-navigation
continuations retain input order; prover events and Stop continue independently.
Document and session switches wait for journal-backed edit acknowledgements.
The bundled host records per-edit identities for idempotent retry and bounded
recovery after a lost reply. When history or receipts cannot establish the
outcome, the app retains the local buffer instead of guessing.

## Display and reading fonts

Settings includes Cerea's reading controls: New Computer Modern, Libertinus
Serif, system serif and interface sans-serif; prose size, line spacing,
paragraph spacing, maths size, justification, column width and centring.
Interface sans-serif uses the same generic sans family as Terminal's normal
threads. The two named serif families are bundled with the app.

Changes apply immediately to rendered prose and the prose editor and are kept
in `~/.config/deductum/settings.json`. Text size sets the prose size; Zoom scales
the document and proof editors together. Automatic text width follows the
proof layout, while an explicit width is a maximum in ems that fits the window.
Existing settings retain the original New Computer Modern appearance.

## Isolated graphical UI review

`scripts/build-vnc-fixture.py` builds a test-only `deductum-vnc` executable
with the real COSMIC window and fixture documents. It uses a distinct binary
name so it cannot overwrite the normal test executable. Build it through the
machine's build coordinator, then run it inside a disposable VNC container
with `DEDUCTUM_VNC_FIXTURE=1`; it does not connect to a live session host.

The fixture writes caret/source state to `/tmp/deductum-vnc-state.json` for
checking actual keyboard and mouse interactions. `DEDUCTUM_VNC_LEAN=1`
selects Lean, `DEDUCTUM_VNC_SOURCE=1` starts in Source, and
`DEDUCTUM_VNC_LONG=1` selects a long document. Set `DEDUCTUM_CONFIG_DIR` to a
container-local directory to check display-setting persistence, or use
`DEDUCTUM_VNC_SCRIPTS=1` for Isabelle scripts in prose, antiquotations and maths.
Debug KaTeX fonts must be copied into the container at their build-time resource path;
release binaries embed them. UI review uses separate network-free containers
with no host desktop, session sockets or user-data mounts.

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
