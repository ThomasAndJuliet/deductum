# Deductum

Deductum is a desktop app for interactive theorem development and proof
assistance. Theorem provers such as [Isabelle](https://isabelle.in.tum.de/) and
[Lean](https://lean-lang.org/) verify mathematics and software one logical step
at a time. Their files are written to be read, yet they are usually edited like
code. Deductum shows a file as a document, with formatted prose, typeset
mathematics and proofs, and checks it continuously as you type. Click anything
to edit its source; the file on disk stays the source of truth. You can share the
same live prover session with AI agents, deciding what each one may change and
seeing who changed what. Deductum runs on Linux, Windows and macOS, with projects on your
own computer or on a remote Linux machine.

## Features

- **Literate and source views.** Read Isabelle text blocks and Lean doc comments
  as formatted prose with typeset mathematics, alongside editable proof code.
  Switch to the source view to work on the complete file.
- **Live proof feedback.** See goals, errors and warnings at the caret, follow
  definitions and use completion while editing.
- **Prover tools.** Use Sledgehammer and Find Theorems for Isabelle, or
  `exact?`/`apply?` suggestions and Loogle search for Lean.
- **Persistent sessions.** Reopen a session after closing the window. Pause,
  stop and resume provers from the app; edits are autosaved, with a recovery
  journal for interrupted work.
- **Work with agents.** Agents can attach to the same documents and prover
  through MCP. Control where they may edit, undo your own edits independently,
  and revert their changes individually.
- **Local and remote projects.** Keep Isabelle and Lean sessions on several
  machines in one workspace, with the prover running where the project lives.
- **Reading and editing preferences.** Choose fonts, text size, spacing, page
  width and theme, and optionally use Vim keys. New Computer Modern and
  Libertinus Serif are bundled.

## Install

Download prebuilt Linux, Windows and macOS packages from GitHub Releases. All are built
by GitHub Actions; you do not need Rust or a source checkout to install them.

### Release downloads

1. Open [Deductum's releases](https://github.com/ThomasAndJuliet/deductum/releases).
2. Choose a version (`vMAJOR.MINOR.PATCH`). Download the Linux `.flatpak`,
   Windows `…-windows-x86_64.zip` or macOS `…-macos-universal.zip` file from its
   **Assets**.
3. Follow the installation steps for your platform below.

GitHub's **Source code** archives contain source, rather than an installable app.

### Linux: downloaded Flatpak

Install [Flatpak](https://flatpak.org/setup/) for your distribution. Download
the `.flatpak` asset above and open a terminal in its download folder. In the
commands below, replace `deductum-VERSION-linux-x86_64.flatpak` with the filename
you downloaded:

```sh
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user --bundle ./deductum-VERSION-linux-x86_64.flatpak
flatpak run dev.twright.Deductum
```

The first installation needs internet access, to download the runtime from
Flathub. You can also launch **Deductum** from your application menu afterwards.

For an update, download the newer `.flatpak` asset and install it over the
existing app:

```sh
flatpak install --user --or-update --bundle ./deductum-VERSION-linux-x86_64.flatpak
```

Install each new app version this way; `flatpak update` alone does not fetch
new Deductum bundles. Settings, proof files and session state are retained.

Local provers run on your Linux system, so install Python 3 and the prover you
need there, outside the Flatpak.

### Windows: portable app

Use the **Windows x64** package; it runs natively and does not require WSL or a
separate Python installation.

1. Download the prebuilt Windows package as described in
   [Release downloads](#release-downloads).
2. Extract the entire Deductum ZIP into a folder you can write to, such as
   `%LOCALAPPDATA%\Programs\Deductum\VERSION`.
3. Run `deductum.exe`. Keep the included `python` and `share` folders beside it.
4. Install Lean or Isabelle separately if you want to run local proofs, as
   described below. For remote proofs, install the Windows OpenSSH client;
   `ssh` and `tar` must be available on PATH. Git must be on PATH to use Git
   project actions.

To update, extract the new package into a different folder. Close Deductum's
windows, run `stop-host.cmd` from the **old** package to flush and stop its
session host, then launch the new package. Keep the old package until you have
checked the update. Settings and session data are stored outside the package.

### macOS: app

Use the **macOS universal** package; it runs natively on Apple Silicon and Intel
Macs with macOS 11 or later and carries its own Python.

1. Download the prebuilt macOS package as described in
   [Release downloads](#release-downloads) and extract the ZIP.
2. Move `Deductum.app` to `/Applications` or `~/Applications`.
3. The app is not signed with an Apple Developer ID, so macOS blocks it
   after a download. In Terminal, run
   `xattr -dr com.apple.quarantine /Applications/Deductum.app`
   (use `~/Applications` if you put it there).
4. Open Deductum like any other app.

For an update, quit Deductum and replace the app with the new one. Settings and
sessions are stored outside the app and are kept. To build the package yourself,
see [the development guide](docs/development.md#build-the-macos-package).

## Set up a prover

Install provers on the machine that will run the project. A computer used only
to connect to remote sessions does not need a local prover.

- **Lean 4:** install [elan](https://github.com/leanprover/elan), then open a
  Lake project containing `lakefile.toml` or `lakefile.lean`. Deductum uses the
  project's `lean-toolchain`; elan downloads that toolchain if needed. Prepare
  the project's dependencies as you would for any other Lean editor.
- **Isabelle:** install [Isabelle2025](https://isabelle.in.tum.de/website-Isabelle2025/).
  On Windows, use the official Windows distribution. Other Isabelle versions
  are not supported yet.

If Deductum cannot find Isabelle, add its `bin/isabelle` path to `host.json`
in the [configuration directory](#settings-and-data) on the prover machine:

```json
{"isabelle": [{"path": "/opt/Isabelle2025/bin/isabelle"}]}
```

On Windows, an equivalent path is `C:/Isabelle2025/bin/isabelle`. If a folder
path causes a problem, the error message says what to do.

## Open your first project

1. Choose **New session**, select the machine and browse to your project folder.
2. Select Isabelle or Lean if both are detected. For Isabelle, check the runtime,
   base logic and any additional session directories. For Lean, Deductum reads
   the Lake project and toolchain automatically.
3. Choose **Start session**, then open a `.thy` or `.lean` file. Switch between
   **Literate** and **Source** views and select a proof position to inspect its
   goals.

Closing a window leaves its proof sessions running. Reopen them from the
Sessions sidebar, or use **Stop** to stop a prover explicitly. Open Settings
with **Ctrl+,** (**⌘,** on macOS) to adjust the display and editor preferences.

You can also open a folder from a terminal:

```sh
flatpak run dev.twright.Deductum /path/to/project
```

In PowerShell, from the extracted Windows package:

```powershell
.\deductum.exe C:\Proofs\Example
```

On macOS, in Terminal:

```sh
/Applications/Deductum.app/Contents/MacOS/deductum ~/Proofs/Example
```

## Remote Linux projects

The remote machine needs Python 3, `tar` and the project's prover installed.
Set up SSH key authentication and confirm the server's host key with your
normal SSH client before connecting in Deductum. Deductum installs its bundled
session-host code under your remote user account automatically.

Add machines to `machines.json` in Deductum's
[configuration directory](#settings-and-data), then restart the app:

```json
[{"name": "proof-server", "host": "proof-server", "user": "my-user"}]
```

Choose that machine when creating a session. Folder paths refer to the
**remote** filesystem.

```sh
flatpak run dev.twright.Deductum --machine proof-server /srv/proofs/Example
```

```powershell
.\deductum.exe --machine proof-server /srv/proofs/Example
```

```sh
/Applications/Deductum.app/Contents/MacOS/deductum --machine proof-server /srv/proofs/Example
```

## Settings and data

| Data | Linux default | Windows default | macOS default |
| --- | --- | --- | --- |
| App configuration (`settings.json`, `machines.json`) | `~/.config/deductum`¹ | `%APPDATA%\deductum` | `~/.config/deductum` |
| Host configuration (`host.json`) | `~/.config/deductum` | `%APPDATA%\deductum` | `~/.config/deductum` |
| Sessions and recovery journal | `~/.local/state/deductum` | `%LOCALAPPDATA%\deductum\state` | `~/.local/state/deductum` |

¹ The Flatpak uses `~/.var/app/dev.twright.Deductum/config/deductum` for app
configuration. Linux and macOS paths follow the corresponding XDG directory settings.
Remote sessions keep their state on the remote machine. Your proof files stay
in their project folders; back those up along with session state.

## Development

For source builds, backend integrations and packaging, see
[the development guide](docs/development.md). The standard Linux test commands
are:

```sh
cargo --offline --locked test --workspace --all-targets --all-features
python3 -m unittest discover -s host/tests -p "test_*.py"
python3 scripts/test-ci-trust.py
python3 scripts/test-publish-release.py
python3 scripts/test-submit-home-apps.py
python3 scripts/test-release-policy.py
python3 scripts/test-package-flatpak-bundle.py
python3 scripts/test-package-windows.py
python3 scripts/test-package-macos.py
python3 scripts/prepare-ci-manifest.py
```

`scripts/test-ci-trust.py` needs PyYAML (`python3-yaml`).

CI also validates the finished Flatpak with
`python3 scripts/check-flatpak-catalog build-flatpak`, `desktop-file-validate`
and `appstreamcli validate --no-net`; the guide gives the complete commands.
