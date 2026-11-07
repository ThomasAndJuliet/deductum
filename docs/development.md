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

The dependency manifest includes pinned downloads. Fetching the household
libcosmic fork and household-kit requires read access to their private GitHub
repositories over SSH. CI uses read-only deploy keys through
`scripts/ci-private-crates.sh`; it does not use an administrator's login.
A source build uses `cargo build --locked --release --bin deductum` with the
native dependencies required by libcosmic. The package is still named
`cosmic-term` for historical reasons; select the `deductum` binary explicitly.

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

Users can download `windows-x86_64` from a successful `main` CI run instead of
building. These private Actions artifacts are retained for seven days.
`.github/workflows/release-binaries.yml` also publishes the portable ZIP and
its receipt to a permanent private GitHub Release after the **entire** CI run
succeeds. It downloads the tested artifact instead of rebuilding it.

The publisher accepts only this repository's successful `main` push runs and
verifies the package's clean source revision, architecture, size and checksum.
Releases use `build-FULL_GIT_SHA` tags, retain the source CI link, and never
replace existing assets or move a shared latest pointer. Interrupted draft
uploads can be retried with the same inputs. To retry a successful run while
its Actions artifact is still retained:

```sh
gh workflow run release-binaries.yml --ref main -f run_id=CI_RUN_ID
```

This supplements the signed Linux Flatpak publication below. It neither
changes repository visibility nor needs the Flatpak signing key. The Windows
package-producing CI jobs come from the Windows support change; merge that
support before enabling this publisher.

## CI and private releases

Branch pushes run `.github/workflows/ci.yml`. The SDK job checks
all targets and tests the complete Rust workspace with all features, including
Deductum's headless UI/unit tests, and runs all Python host unit-test modules:

```sh
cargo --offline --locked test --workspace --all-targets --all-features
python3 -m unittest discover -s deductum/host/tests -p "test_*.py"
python3 scripts/test-publish-windows-release.py
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
records its Git SHA, run ID and archive digest. Chartgen and terminal dependencies are vendored in this repository;
libcosmic and household-tones are fetched from private household repositories
using read-only deploy keys. No sibling checkout is needed.
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
`scripts/fix-cargo-sources-ssh.py` for the libcosmic fork and household-kit (its
household-tones crate), which are fetched over ssh:

    flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
    python3 scripts/fix-cargo-sources-ssh.py packaging/flatpak/cargo-sources.json

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

