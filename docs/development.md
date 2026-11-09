# Developing Deductum

This guide is for building, testing and maintaining Deductum. Commands run from
the repository root. For installation and everyday use, see [the README](../README.md).

## Build on Linux

The supported packaged build uses Freedesktop SDK 25.08. Install
`org.flatpak.Builder`, `org.freedesktop.Sdk//25.08` and
`org.freedesktop.Sdk.Extension.rust-stable//25.08`, then run:

```sh
scripts/build-flatpak.sh
# Build and install locally instead:
scripts/build-flatpak.sh --user --install
```

The dependency manifest includes pinned downloads, including the household
libcosmic fork and household-kit over HTTPS. No credentials are needed.
A source build uses `cargo build --locked --release --bin deductum` with the
native dependencies required by libcosmic.

## Build the Windows package

The Windows support uses a Linux cross-build with Rust 1.99.0 and MinGW-w64:

```sh
rustup target add --toolchain 1.99.0 x86_64-pc-windows-gnu
cargo +1.99.0 build --locked --release --bin deductum --no-default-features \
  --features wgpu --target x86_64-pc-windows-gnu
python3 scripts/test-package-windows.py
python3 scripts/package-windows.py
```

The packaging script downloads and verifies the pinned embedded Python runtime
and writes the portable ZIP and checksum/provenance JSON under `dist/windows`.
Release packaging requires a clean checkout and a binary built from that exact
revision. Development packages made with `--allow-dirty` are marked as such.
The Windows CI jobs test the native host, app and real Isabelle/Lean backends;
cross-compilation alone does not establish that the package works on Windows.

The hosted Windows gate uses PowerShell 7:

```powershell
./scripts/run-windows-native-ci.ps1
```

It creates disposable standard accounts and runs the complete host and Rust
suites plus both real prover integrations with isolated configuration and a
private temporary directory. `scripts/install-windows-test-provers.ps1`
installs the pinned test dependencies. The package's embedded Python is run
with `-X utf8`, because it ignores `PYTHONUTF8`. Separate VM acceptance also
covers visible GUI use, second-user isolation and Windows-to-Linux SSH.
`deductum.exe --build-info` reports the package's embedded source identity.

## Build the macOS package

The macOS package is built on a Mac, from both architectures, and joined into
one universal binary with `lipo`:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --locked --release --bin deductum --target aarch64-apple-darwin
cargo build --locked --release --bin deductum --target x86_64-apple-darwin
python3 scripts/test-package-macos.py
python3 scripts/package-macos.py
```

The packaging script downloads and verifies the pinned Python 3.12 runtimes of
`packaging/macos/python-runtime.json` (python-build-standalone, one per
architecture) and writes the ZIP and its provenance JSON under `dist/macos`.
As for Windows, release packaging requires a clean checkout and binaries built
from that exact revision, and `--allow-dirty` marks a development package.
The package holds `deductum`, `share/deductum`, `python/arm64`,
`python/x86_64`, the `Deductum.command` launcher and `README.txt`. The launcher
puts the bundled Python and `~/.elan/bin` on `PATH`, because an app opened from
Finder gets no shell `PATH`. The binary needs macOS 11 or later.

The binary has an ad-hoc signature only. Without signing and notarisation with
an Apple Developer ID, Gatekeeper blocks the downloaded package until its
quarantine attribute is removed, as `packaging/macos/README.txt` explains. No
CI job builds the macOS package yet.

## Automated versioned downloads

`.github/workflows/release-binaries.yml` runs after the **entire** main-branch
CI run succeeds, including native Windows and Linux prover tests. It publishes
the tested Windows ZIP and Linux `.flatpak`, their source/checksum receipts,
`SHA256SUMS` and generated release notes to a permanent GitHub Release, which
has the repository's visibility.
It downloads CI artifacts instead of recompiling either app. Download filenames
include the release version, such as `deductum-v0.2.0-linux-x86_64.flatpak`;
renaming preserves the tested package and receipt bytes.

Versions follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/)
and [SemVer](https://semver.org/spec/v2.0.0.html). The highest change since the
previous reachable final `vMAJOR.MINOR.PATCH` tag determines one bump:

| Commit | Version change |
| --- | --- |
| `fix(editor): preserve the caret`, `perf: reduce allocations` | Patch |
| `feat: add a proof tool` | Minor |
| `feat!: change the protocol`, or a `BREAKING CHANGE:` / `BREAKING-CHANGE:` footer | Major (minor before 1.0) |
| A `Release-As: 1.0.0` footer | That version, if it is above the previous one and the automatic bump |
| Only `docs`, `test`, `chore`, and other changes without a breaking marker | No versioned release |

Use these subjects for commits and for a PR's title when squash-merging it.

Before 1.0, breaking changes advance the minor version, so Deductum never
reaches 1.0.0 by itself. To release 1.0.0, merge a commit whose message ends with
the footer `Release-As: 1.0.0` (for example
`chore: declare the stable release`); from then on breaking changes advance the
major version.
`release-config.json` starts the distribution version at `0.1.0` and excludes
history at or before the adoption commit. Older tags such as
`epoch-*`, prereleases and unrelated branches do not set the version. The
source Cargo package version is retained in the Windows build receipt; the
SemVer tag identifies the downloadable Deductum release.

The publisher accepts only successful, completed CI pushes from `main` of the
repository named in `release-config.json`. It verifies the package source
revision, architecture, size, checksum and Linux run identity. It reserves each version tag atomically
on that exact tested commit before uploading. Assets are immutable, and an
interrupted upload can resume without replacing existing files. A newer main
push supersedes an older candidate that has not reserved a version; its
release includes those still-unreleased commits. Already reserved releases
can finish. No shared concurrency group drops pending revisions, and delayed
runs never move a shared latest pointer backwards.

A public repository adds three rules. `public_signing` in `release-config.json`
must record the signing decision (`github-attestation` is the one implemented);
until then CI runs normally and nothing is released, and the next release covers
the waiting commits. `scripts/release_scan.py` must find no household network
names in the Windows package, the Linux payload (scanned when CI bundles it and
recorded in the receipt) or the receipts, and generated notes omit any commit
subject naming household systems. And `publish-release.py stage` uploads to a
draft, the workflow attests every file with `actions/attest`, and
`publish-release.py finish` verifies each attestation with `gh attestation
verify` before making the release visible. A failure at any point leaves a draft
that the retry below resumes. The attestation identifies the release workflow;
it signs the JSON receipts too, and those bind each package to its source
revision and CI run.

No human tag or separate release approval is needed. The publisher uses the
job-scoped `GITHUB_TOKEN`; it creates no version-bump commits, so publication
does not create a loop of new source builds. To retry a successful run while
its Actions artifacts are still retained (seven days):

```sh
gh workflow run release-binaries.yml --ref main -f run_id=CI_RUN_ID
```

The `linux-x86_64` and `windows-x86_64` Actions artifacts are also available
for direct downloads during that retention window. The Windows package jobs
come from the Windows support change; merge that support before this publisher.

The standalone Linux bundle comes from `ci-artifact/build.tar.gz`, including
when Neo reuses a previously validated build. Run
`python3 scripts/package-flatpak-bundle.py ci-artifact` with the CI identity
environment to reproduce it. The bundler verifies the input digest and source,
rechecks desktop/AppStream/catalog metadata, exports that payload to a local
repository and runs `flatpak build-bundle` with Flathub as the runtime source.
The bundle contains no `home-apps` repository URL. It has no repository
signature: no signing key is exposed to CI. Users install new bundles explicitly using the commands
in [the README](../README.md#linux-downloaded-flatpak).

The real Flatpak export/install/update test runs on the build runner, outside
the SDK build sandbox. Its installation and XDG data/config/cache directories
are temporary, and `--no-deps` keeps the fixture test offline. It leaves the
user's normal Flatpak installation alone. The policy and publisher unit tests
also run inside the SDK test manifest.

Every successful main push still publishes the signed private `home-apps`
Flatpak below, including docs-only pushes. That publication remains on Neo;
the versioned GitHub downloads neither change its ordering nor repository
visibility.

## CI and private releases

Branch pushes and pull requests run `.github/workflows/ci.yml`. The SDK job checks
all targets and tests the complete Rust workspace with all features, including
Deductum's headless UI/unit tests, and runs all Python host unit-test modules:

```sh
cargo --offline --locked test --workspace --all-targets --all-features
python3 -m unittest discover -s host/tests -p "test_*.py"
python3 scripts/test-publish-release.py
python3 scripts/test-submit-home-apps.py
python3 scripts/test-release-policy.py
python3 scripts/test-package-flatpak-bundle.py
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
python3 -m unittest discover -s host/tests -p "test_responsiveness.py"
python3 -m unittest discover -s host/tests -p "test_background_io.py"
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
python3 host/tests/integration_lean.py --skip-dsrv \
  --core-fixture "$PWD/scripts/ci-fixtures/lean"
DEDUCTUM_ISABELLE=/path/to/Isabelle2025/bin/isabelle \
  DEDUCTUM_STATE_DIR=/disposable/state \
  python3 host/tests/integration_host.py
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
records its Git SHA, run ID and archive digest. Chartgen and a patched cosmic-text
(`vendor/cosmic-text/README.md`) are vendored in this repository;
libcosmic and household-tones are fetched from their public household
repositories. No sibling checkout is needed.
The successful default-branch push preserves its artifact for seven days.
The release workflow waits for the entire CI workflow, including both prover
integrations, to succeed before submitting to Neo with
`scripts/submit-home-apps.py`. A public repository cannot call the household's
private reusable workflow, so Neo's policy pins this repository's own
`release.yml` on `main` instead. Other branches and pull requests do not publish
stable.

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
are regenerated from Cargo.lock with `flatpak-cargo-generator.py`:

    flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json

Validate the finished build with the same package checks CI uses:

```sh
python3 scripts/check-flatpak-catalog build-flatpak
desktop-file-validate build-flatpak/files/share/applications/dev.twright.Deductum.desktop
appstreamcli validate --no-net build-flatpak/files/share/metainfo/dev.twright.Deductum.metainfo.xml
```

`python3 scripts/package-ci-release.py build-flatpak` additionally verifies
the executable, runtime, app identity and clean source revision, then records
the archive digest and CI provenance. Run that command in CI with its
`GITHUB_SHA`, `GITHUB_REPOSITORY` and `GITHUB_RUN_ID` environment.

## Text interaction and recovery

Editor input keeps its source or proof-window origin and waits for fresh layout
when a cell or its displayed markup changes. Clipboard and page-navigation
continuations retain input order; prover events and Stop continue independently.
Document and session switches wait for journal-backed edit acknowledgements.
The bundled host records per-edit identities for idempotent retry and bounded
recovery after a lost reply. When history or receipts cannot establish the
outcome, the app retains the local buffer instead of guessing.

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

## Existing Isabellum sessions

Deductum has its own settings, host bundle and state. Installing or publishing
it does not delete Isabellum or migrate sessions. The optional explicit
`python3 ~/.local/share/deductum/host/deductum_host.py migrate-from-isabellum`
command requires the Isabellum host to be stopped first;
never allow two hosts to autosave the same session folder.

