# Agent sessions: design and plan

This fork of COSMIC Terminal adds first-class support for opening the Claude
and Codex CLIs, either locally or on a remote host (`twright@neo`). A remote
session runs inside a Zellij session so it survives the tab being closed or the
network dropping. The terminal's own tab bar manages sessions; Zellij's UI and
key bindings are hidden.

## Model

`Profile` gains these fields, all `#[serde(default)]` so existing configs
load unchanged:

| field     | meaning                                                            |
|-----------|--------------------------------------------------------------------|
| `agent`   | `Shell` (existing behaviour: run `command`), `Claude`, or `Codex`  |
| `machine` | the machine it runs on, or none for this computer                  |
| `host`    | the machine's ssh destination, such as `twright@neo`; empty for local. Kept equal to the machine's destination, and used wherever a session is opened |
| `args`    | extra arguments passed to the agent CLI                            |
| `color`   | the tab colour as `#rrggbb`. When it is empty, Claude tabs are orange, Codex tabs are green, and terminal tabs are unmarked |

`working_directory` is interpreted on the profile's machine.

Built-in profiles are added in numbered rounds, each exactly once, so deleting
one is permanent. Round 1 adds Claude and Codex, round 2 Terminal, all on this
computer. A remote terminal runs the user's login shell in a Zellij session,
in the same way as the agents. Configs from before machines existed also got
"… on neo" profiles; on loading, each profile's host becomes a machine.

## Machines

`Config::machines` lists the other computers: a name, the `host` ssh connects
to (an alias from `~/.ssh/config`, a host name or an address), and the `user`
to log in as, empty for ssh's own choice. Each profile picks one of them, or
this computer. Editing a machine's host or user updates every profile on it.
A machine in use by a profile can't be deleted.

**Add machine…** on the profiles page offers computers found in two places,
merged when they are the same computer:

- the online and offline Linux, macOS and Windows peers in
  `tailscale status --json`, by MagicDNS name;
- the concrete `Host` entries of `~/.ssh/config`, with their `HostName` and
  `User`, leaving out patterns and code forges.

An ssh alias wins over a Tailscale name, since it carries the user and other
options. This computer and machines already added are left out. **Add by
hand** starts from a blank machine.

**Check** logs in once, over a fresh connection with `BatchMode`, and reads
the user name, host name, home and project-registry id. It fills in an empty
user name, and says when the machine is this computer, or the same computer
as another machine. A failure explains itself. An unknown host key, for
example, needs one `ssh DESTINATION` from a terminal, since the terminal never
accepts host keys by itself. A newly added machine is checked straight away,
and offers to add "Claude on …", "Codex on …" and "Terminal on …" profiles.

The launcher queries every machine as well as every profile's host, and labels
conversations with the machine's name.

## Launching

Every tab a profile or the `+` button opens runs in its own Zellij session,
on this computer or on the profile's host, so that it outlives the tab. Only a
command given with `-e` runs directly. A local session runs the bootstrap
script below with `sh`, and a remote one with `ssh -t <host> <command>`. If
Zellij is missing, the script runs the command directly, and the tab cannot
be reattached or moved.

The bootstrap command is a POSIX `sh` script. For a remote session it is
base64-encoded so that the remote login shell (fish on neo) parses
only `sh -c 'eval "$(echo … | base64 -d)"'`. The script:

1. writes `~/.config/cosmic-term-agents/config.kdl` and
   `layouts/default.kdl`. A session created with a command ignores
   `default_layout` and takes the layout named `default` from the layout
   directory. Without one it gets Zellij's own default, which has bars;
2. runs `zellij --config … attach --create --close-on-exit <name> -- sh -c
   'cd <dir> || cd; <agent> <args>; zellij kill-session "$ZELLIJ_SESSION_NAME"'`.

An agent or command runs under `ct-supervise` (`src/bin/ct-supervise.rs`), a
small statically linked Rust program. The pane has no interactive shell, so
nothing would resume an agent that stops itself on Ctrl+Z. Shell job control
in a script is no answer: dash's `fg` hands over the terminal, but bash's
does not, and `/bin/sh` is bash on Fedora. The supervisor runs the command in
its own process group, which owns the terminal. When the command stops, the
supervisor takes the terminal back, shows `[Suspended. Press Enter to
resume.]`, then hands the terminal back and continues the command.

At startup the terminal copies the supervisor to
`~/.config/cosmic-term-agents/ct-supervise-<hash>` on each host, uploading it
over ssh for remote hosts. The name carries a hash of the contents, so every
build installs its own. Until it is installed, a pane runs its command
directly: Ctrl+Z is lost, but nothing hangs. The flatpak builds it with
`-C target-feature=+crt-static`, using an empty `libm.a` because the SDK has
no static libm.

Tabs have a right-click menu. It offers New tab, and either Close tab or,
for session tabs, Detach (keep running), Move to new window, and End
session.

The config clears every Zellij key binding (`default_mode "locked"` alone
still intercepts Ctrl+G and Ctrl+Q). It also turns off frames, tips and
session serialisation, and makes a single borderless pane the default layout.
`mouse_mode` is on so the wheel reaches Zellij's scrollback, so native
selection needs Shift. The trailing `kill-session` ends the session when the
agent exits while no client is attached, a case `--close-on-exit` misses.

Session names are `ct-<agent>-<dirname>-<4 hex>`. The prefix lets the fork
list its own sessions without touching the user's other Zellij sessions.

ssh connections share a control master (`ControlMaster=auto`,
`ControlPersist`), so opening further tabs and listing sessions does not
re-authenticate.

The fork's app ID is `dev.twright.CosmicTermAgents`, so its config is separate
from an installed COSMIC Terminal.

## Open in Zed

A header button opens the active tab's project in a new Zed window.
`src/agent/project-dirs.py` finds the directories, and runs the same way
locally and on the remote host. It starts from the tab's shell PID, or from
the Zellij server for a remote tab's session, and walks the descendant
processes:

- For a Claude process, it reads `~/.claude/sessions/<pid>.json` to get the
  session ID, then takes the last `environment` snapshot in the session's
  transcript. The directories are `workingDirectory` plus
  `additionalWorkingDirectories`, which include `--add-dir` directories.
- Otherwise it uses the working directory of the deepest process.

Each directory becomes its git root, and duplicates are removed. Local
directories are passed to `zed --new` as paths. Remote ones are passed as
`ssh://<host><path>` URLs, which Zed merges into one remote project.

## Flatpak

`scripts/build-flatpak.sh` builds and installs `dev.twright.CosmicTermAgents`
for the user from `packaging/flatpak/`. `cargo-sources.json` vendors the
dependencies, and needs regenerating when `Cargo.lock` changes.

The runtime has none of the user's tools, so every process runs on the host
through `flatpak-spawn --host`. That covers tabs, `ssh`, `python3` and `zed`
(`src/host.rs`). Outside a Flatpak the same code runs them directly.

- **Controlling terminal.** alacritty makes the tab's pty the controlling
  terminal of the process it starts. The host process could not then take
  it, and would have no job control, Ctrl+C or resize signals. So a tab first
  runs the terminal's own binary with `--host-exec`. That releases the pty
  (`TIOCNOTTY`) and execs `flatpak-spawn`. Flatpak's session helper then
  makes the pty the controlling terminal of the host process. This was
  checked with SIGINT and SIGWINCH in a sandbox.
- **Finding a tab's processes.** The sandbox has its own PID namespace, so
  each tab sets `COSMIC_TERM_TAB=<token>`, and the Open in Zed helper finds
  the tab's host processes by it.
- **Default shell.** The login shell comes from the host's passwd entry.
- **Daemonizing.** It is skipped, because the sandbox ends with its first
  process.
- **Inherited working directory.** "New tab inherits working directory" has
  no effect, because the sandbox cannot see a host process's directory.

## Launcher tabs

Opening a Claude or Codex profile with no working directory opens a
*launcher* in the tab rather than starting the agent in `~`. It offers:

- **Home** and **Browse…**. Browse opens the folder picker; for a remote
  host it browses the host's sshfs mount, and maps the folder back to the
  path on the host.
- **Running**: the agent's Zellij sessions on the host that aren't open in
  this window. Choosing one attaches to it.
- **Projects**: the directories either agent knows on the profile's host,
  from `~/.claude.json` and `~/.codex/config.toml`, plus the directories of
  their conversations, newest first. **New worktree** beside each one opens
  the form described under "New worktrees".
- **Recent conversations**: both agents' conversations on every host the
  profiles name, newest first, each marked with its agent and computer when
  they differ from the profile's. A conversation of the profile's agent on its
  host resumes in its own directory with `claude --resume <id>` or
  `codex resume <id>`. One of the other agent on the same computer continues
  as a new conversation from its transcript. One on the other computer offers
  **Move here** and **Fork here** (see "Transferring sessions").

`src/agent/launcher.py all` gathers the lists on each host, with its hostname
and its id in the project registry, which is how one computer reached under
two ssh names is recognised. Each host's last listing is kept under
`$XDG_STATE_HOME/cosmic-term/launcher/`; when a host can't be reached, its
rows are shown from there, marked offline, and can't be launched. A filter
box narrows them, and Enter chooses the first row that can be launched. The session that starts
replaces the launcher in the same place in the tab bar. A profile with a
working directory, or a directory given with `-w`, still starts straight
away.

## Transferring sessions

A conversation and the worktree it works in can move between the laptop and
the desktop, or be forked onto the other one. The source must be online; once
the transfer is done, the destination carries on with the source switched
off. The code is `src/lib.rs` and its modules, shared by the terminal and the
`ct-transfer` helper.

### Model

| Module | Owns |
|---|---|
| `project` | Each computer's registry, `$XDG_STATE_HOME/cosmic-term/projects.json`: its own `HostId`, and for each project (`ProjectId`, shared between computers) its clones there, where its worktrees go, the ignored files that travel with it, and its idle copies |
| `workspace` | Snapshots, worktree creation and reuse, the branch move, carried files, through `git` |
| `provider` | Claude and Codex conversation files: export, import with `cwd` rewritten, readable transcripts and handoff messages |
| `helper` | One `Request` per step on one computer, answered by `ct-transfer` |
| `transfer` | The order of the steps, the progress record, rollback and resume |

Each computer writes only its own registry, and other computers' facts are
asked for over ssh when needed. A newer registry format is never written by
an older build. Settings and profiles are unchanged.

The terminal reaches the other computer only by running `ssh` from wherever
its window is, so every step runs through a `transfer::Machine`:
`agent::HostMachine` runs the installed `ct-transfer` with `sh` locally, or
over ssh. `ct-transfer` is linked statically and installed on each host like
`ct-supervise`, named by a hash of its contents, so both computers run the
same code. Git objects go with `git fetch` when the destination is this
computer and `git push` when the source is, using the terminal's ssh options.

### Steps

1. Check the source: it must have a branch checked out, no other worktrees
   inside it (merge subagents' work first), and no Claude or Codex running in
   it (stop the agent by hand first).
2. Snapshot the source: the index as a commit `I` on `HEAD`, and every file
   that isn't ignored as a commit `W` on `I`, under
   `refs/cosmic-term/transfer/<id>`. No branch moves.
3. Bring the ref to the destination's repository, creating one if the project
   has no clone there.
4. Make the destination's worktree, detached at `HEAD`, then set its files
   from `W` and its index from `I`. Staged, unstaged and untracked changes
   arrive as they were.
5. Write the ignored files the project lists in `carry_ignored`, such as
   `.env` or `.local-design/`. An existing file is never overwritten, except
   in an idle copy where it hasn't changed since the copy went idle.
6. A Claude conversation continued with Claude is copied into the
   destination directory's folder under a new id, with only its `cwd` fields
   changed; everything else, signed thinking included, is copied byte for
   byte. It resumes with a note appended to the system prompt saying where
   paths moved. A Codex conversation, or one continued with the other agent,
   starts a new conversation: its transcript is written to
   `$XDG_STATE_HOME/cosmic-term/transcripts/`, and the first message points
   at it.
7. The point of no return: move the branch to the transferred commit and
   check it out there, record the project's clone on the destination, and,
   for a move, mark the source as an idle copy.
8. Delete the transfer refs.

Before each step the terminal saves a record under
`$XDG_STATE_HOME/cosmic-term/transfers/`. A failure before step 7 undoes
exactly what the record lists. After step 7, only the rest of the bookkeeping
is retried. A launcher lists records left by a crash, with **Resume** and
**Roll back**.

### Choosing where it goes

The first transfer of a project asks which of the destination's repositories
is its clone. Repositories sharing a remote with the source are offered
first, then ones with the same name, then a new repository under
`~/Documents`. The choice is remembered. A clone already recorded for the
project is used without asking.

A move reuses a worktree that has the branch checked out, usually the idle
copy left by the last transfer the other way. It is reused only if its index
and files are still exactly as they were when it went idle (Git tree hashes
recorded with the idle mark), or have no changes at all, and if its `HEAD` is
an ancestor of the incoming commit. Otherwise the transfer refuses rather
than merge. A new worktree goes in `<root>/<project>-<branch>`. The root is
`~/worktrees`, unless the registry or the repository sets another, and the
path can be edited in the form. A fork goes on a new branch
`<branch>-<source hostname>`, and leaves the source as it was.

## New worktrees

**New worktree** on a project row fetches the project's remote and opens a
form. It asks for a new branch name, a base (the remote's default branch,
like `origin/main`, or any local branch), and the folder, which is prefilled
as above. A repository without a remote starts from its checked-out branch.
If the fetch fails, the form says so, and the base is as last fetched. The
profile's agent, or shell, starts in the new worktree. Worktrees are not
removed by the terminal; use `git worktree remove`.

## Following an agent

An agent seldom stays where it started. Asked to make a worktree, or started
in `~` and pointed at a project, it carries on elsewhere, while its process's
working directory stays put. `src/agent/project-dirs.py` therefore reads each
running agent's *focus* from what it has been doing lately:
- the directories of its recent commands (`cd`, `git -C`, and Codex's
  `workdir`);
- the files it read, wrote or patched.

Each is replaced by its git repository root and ranked by how often and how
recently it was used. Scratch folders under `/tmp` count only when they hold a
repository.

How each agent is found:
- **Claude:** by `~/.claude/sessions/PID.json`, which also names its
  conversation and status.
- **Codex:** by the thread lock it holds in `~/.codex/thread-writer-locks`.
  Failing that, by `resume <id>` on its command line. Failing both, by the
  newest conversation begun where it started since it started, marked as a
  guess.

An agent tab looks this up when it becomes active, and when the agent
retitles it (agents do as they work), at most every ten seconds.
- **New tabs:** a new tab opened from an agent tab starts in the agent's
  focus, not where the agent started.
- **Open in Zed:** puts the focus first.

## Moving a tab to another computer

An agent tab's right-click menu offers **Move to ‹profile›…** and **Fork to
‹profile›…** for each Claude or Codex profile on another computer. Machines
are told apart by their registry id or hostname, so a profile naming this
computer under an ssh alias is left out.

Choosing one:
1. Finds the tab's conversation and focus.
2. Opens that profile's launcher with the transfer form already filled in: the
   tab's conversation, from its focus, which is the worktree to move.
3. If the agent is still running, the form says to stop it first (finish or
   interrupt the turn, then exit). **Check again** retries once that's done.

After a move, the old tab's session ends with its agent, and the
conversation carries on in the new tab on the destination.



Each window is a separate process, so a tab moves by attaching to its session
in the other window and detaching here. Zellij keeps the scrollback.

- **Move to new window.** This context menu item starts a new window with
  `--attach <session>[@<host>]`, then closes the tab here.
- **Dragging.** A dragged tab carries its session under
  `x-cosmic-term/tab`. A window that receives it opens a tab attached to the
  session. The source closes its tab when the drop finishes elsewhere, and
  pops the tab out into a new window when the drop is cancelled, such as on
  the desktop. The tab bar is always shown, so that a window's only tab can
  be dragged.
- **libcosmic change.** libcosmic's tab bar offered only a fixed payload and
  did not say which tab was dragged. libcosmic is therefore vendored as a
  squashed git subtree in `libcosmic/`, with its `iced` and `cosmic-icons`
  submodules as nested subtrees. `Cargo.toml` patches it in, and the tab bar
  gains `tab_drag_data` and `on_tab_drag_start`.

## Opening remote files locally

Right-clicking a file link, or a path-like word such as `src/main.rs:42`, in
a remote tab offers **Open locally**:

1. A relative path or `~` is resolved on the host, against the agent's
   working directory, using `project-dirs.py --cwd`.
2. The host's root is mounted with sshfs at
   `~/.cache/cosmic-term/mounts/<host>`, or remounted if the mount has gone
   stale.
3. The file is opened with `xdg-open` on the host.

The mount is kept for later links and other windows.

## Disconnected remote tabs

ssh exits with 255 when its connection fails or is lost, and with the remote
command's status otherwise. A remote tab whose ssh exits with 255 is kept
rather than closed:

- **Shown.** Its icon and strip turn grey, "(disconnected)" follows its
  title, and a bar above the terminal offers **Reconnect now**. The last
  screen stays visible.
- **Retried.** One probe loop per host makes fresh ssh connections, not
  through the shared connection, which may be the one that died. It tries
  after 5 seconds, then 10, doubling up to a minute.
- **Reconnected.** When the host answers, each disconnected tab reattaches
  in place. A tab whose session ended meanwhile closes with a note. A tab
  that dropped within 10 seconds of starting reruns its start command, since
  its session may never have been created.

Keep-alives every 5 seconds, three of which may go unanswered, notice a dead
connection in about 15 seconds.

## Tab lifecycle

| action                            | effect                                          |
|-----------------------------------|-------------------------------------------------|
| close tab, or Detach (tab menu)   | Zellij client exits, session keeps running       |
| agent exits                       | Zellij session ends, ssh exits, tab closes      |
| "End remote session" (tab menu)   | `zellij kill-session` then close the tab        |
| reattach (from the new-tab menu)  | new tab running `zellij attach <name>`          |

## UI

Following Ptyxis, the header's `+` becomes a split button. The main part opens
the default profile. The arrow opens a menu listing:

1. profiles, with the default marked;
2. this terminal's sessions on each remote host (fetched when the menu opens),
   except those open in this window;
3. the user's other Zellij sessions on the host, with exited ones marked.
   These attach with the user's own Zellij config, bars and key bindings;
4. "Profiles…", which opens the profile settings page.

Clicking a session opens a tab attached to it.

Tabs and menu rows show a computer (local) or server (remote) icon in the tab
colour, which also becomes the window's accent colour while the tab is in
front. A reattached
session takes its colour from a profile with the same host and agent. Titles set by the agent over OSC pass
through Zellij; the Zellij prefix is removed.

The profile editor gains an agent selector, a host field and an arguments
field.

## Secrets and sudo approvals

Claude's and Codex's tool calls run commands without a terminal, so `sudo`
cannot ask for a password. The user answers such requests in the terminal
instead, on this computer or on another machine, and no password is sent
anywhere: sudo on each machine accepts a signature from a key the terminal
holds. sudo and sudoers still decide what is allowed and run the command.
The same store keeps the user's saved passwords, which replace upstream's
password manager.

- **The store.** `~/.local/state/cosmic-term/vault` (`cosmic_term::vault`)
  holds saved passwords — each for a machine and account, or Other, such as
  the password of an encrypted file — and one Ed25519 key per machine that
  approves sudo there. It is sealed with XChaCha20-Poly1305 under a key
  Argon2id derives from the user's store password. Only `ct-sudo-agent`
  reads it; windows get a secret from the agent only to paste it.
- **The agent.** `ct-sudo-agent`, a static binary started by systemd user
  units on this computer, with two sockets: `/run/user/<uid>/cosmic-term-sudo.sock`
  for PAM modules, and a control socket in the app's runtime directory for
  windows, which it accepts only from the terminal's executable. It tells
  every window about a request (no secrets on the session bus) and takes the
  answer from whichever the user uses.
- **The policy** (`sudo_control::Policy`). The store locks after 15 minutes
  unused, and when the screen locks or the computer sleeps. A sudo request
  needs the store password when the store is locked, or when its session has
  not given it yet (as sudo's ticket is per terminal); after that, a touch on
  the fingerprint reader, or a click when the reader cannot be used: fprintd
  is off, or the reader is built in and the lid is closed. Pasting a secret
  needs nothing more than choosing it, unless the entry asks for the store
  password each time. The policy can be changed only while the store is
  open.
- **The PAM module.** `pam_cosmic_term_sudo.so` (crate `sudo/pam`, logic in
  `ct-sudo-protocol`) is listed first, as `auth sufficient`, in
  `/etc/pam.d/sudo` and `sudo-i`. It builds a challenge — a fresh nonce, the
  host, the user, the sudo process's directory and command line (read from
  `/proc/self`, so they are what sudo will run), its cgroup, the Zellij
  session it came from (for display and routing only), the time and the
  module's version — and sends it to the agent's socket after checking that
  the socket is the user's own. It accepts only a signature over those exact
  bytes by the key in the root-owned `/etc/cosmic-term/sudo-keys/<user>.pub`.
  Anything else, or no answer within 2 minutes, fails, and sudo falls through
  to its other methods.
- **Remote machines.** The agent's socket is forwarded to the same path on a
  machine, over the shared ssh connection, while windows show that machine.
- **The Secrets pane** (View → Secrets, on the right): the store's lock, the
  waiting requests, a section per machine with its sudo approvals and saved
  passwords, Other, and the policy with fingerprint enrolment. A request
  also shows as a bar on the tab that asked, with a lock on the tab and its
  sidebar row and a notification when it is out of view.
- **Setting up a machine.** In the pane: the agent makes the machine's key,
  and a tab on the machine runs `sudo ct-sudo-setup install`, the one
  ordinary password entry, which installs the module and the public key and
  adds the PAM lines between markers. `uninstall` removes everything.

The design and the alternatives set aside (a password relay through
askpass, a run0-style root service, existing modules, an agent inside the
windows, per-machine passwords for the store, cgroup or sandbox isolation
per tab) were worked through in a design dialogue;
`.local-design/sudo-auth.md` keeps its record locally.

## Where the user is, and the machines' files

Every machine the terminal works with gets `~/.config/cosmic-term-agents/ct-mount`,
which mounts another machine's files at `~/.cache/cosmic-term/mounts/NAME`
with sshfs when something asks, and unmounts them when nothing on the
machine needs them (no session running, no window's lease). Machines are
named by their host names, and each learns how to reach the others over ssh
when the terminal starts.

`ct-mount --where` says which machine the user is at, so that agents open
windows there: the machine of the terminal attached to the session that saw
input last (each `zellij attach` carries `CT_CLIENT`), else for a plain ssh
session the machine its connection comes from, else the one machine with a
local desktop in use, else the machine that last opened the session.
`--open FILE` opens a file there, through that machine's mount of this one,
and `ct-remote-mcp` runs an MCP server that shows windows — Marginalium —
there too, translating paths both ways. The `fleet` skill tells agents all
this.

## Threads

A Claude profile marked **Open as rich-text thread** (the built-in "Claude thread"
profile is one) shows its conversation as rich text in the tab, rather than
running the Claude TUI in a terminal. The code is `src/thread/`.

### Talking to Claude

The thread runs the `claude` CLI itself, headless, as the Claude Agent SDK
does: `claude --input-format stream-json --output-format stream-json
--verbose --include-partial-messages --permission-prompt-tool stdio`, with
`--resume=<id>`, `--model` and `--permission-mode` when they apply. The CLI
uses its own login, so turns count against the user's subscription and never
against API credits; `ANTHROPIC_API_KEY` is unset in its environment, as the
CLI would otherwise prefer it. It runs on the profile's machine through the
same `sh` or `ssh` command as other helpers (`agent::session_exec`), and so
through `flatpak-spawn --host` in the Flatpak.

The protocol is one JSON object per line (`thread::claude`):

- **In:** an `initialize` control request, then each user message, with any
  images as base64 blocks. `interrupt`, `set_model` and `set_permission_mode`
  are control requests.
- **Out:** `system`/`init` (the session id, model and permission mode),
  `stream_event`s carrying the API's deltas, a complete `assistant` message
  per content block, `user` messages with tool results, and a `result` per
  turn. A `control_request` with subtype `can_use_tool` asks permission; the
  thread answers allow (echoing the input), allow with the CLI's suggested
  rule for the session, or deny. Any other control request gets an error
  reply.

### The thread

As in Zed's agent panel, a thread is a flat list of entries
(`thread::model`): the user's messages, the assistant's prose, thinking,
tool calls and notices. Streamed deltas build an entry, and the complete
message that follows replaces its text, so what is shown is what was sent. A
tool call is patched in place as its input streams in, its permission is
asked and its result arrives. A subagent's steps appear inside its Task call.
The same events build a thread from the CLI's transcript
(`~/.claude/projects/<dir>/<id>.jsonl`).

The view (`thread::view`):

- **Entries.** User messages are cards, the assistant's prose is set as
  rich text, and thinking and tool calls are cards that open. Edits, writes
  and todo lists are open from the start, as Delta shows them: nothing that
  changed the code is hidden. An edit is shown as a diff.
- **Permissions** sit above the composer, as in t3code, so they stay in view.
- **The composer** sends with Enter (Shift+Enter adds a line), attaches
  images, and chooses the model and permission mode. Stop interrupts a turn.
- **Following.** The view follows new output while it is scrolled to the
  end, and stays put while the user reads further up.

### Rich text and maths

`thread::markdown` parses markdown with pulldown-cmark, with `$…$` and
`$$…$$` math. Claude also writes `\(…\)`, `\[…\]` and bare
`\begin{align}…`, which CommonMark reads as escapes; these are rewritten to
dollars first, outside code.

`thread::math` sets LaTeX with RaTeX, a pure-Rust port of KaTeX, as SVG whose
glyphs are paths. A formula takes well under a millisecond, so formulas are
set as they are laid out, from one cache bounded at 24 MB. Numbered
environments are set unnumbered, since each formula stands alone. A formula
RaTeX cannot parse is shown as its source in the error colour.

`thread::rich` lays the blocks out with cosmic-text and draws the buffers
itself, as the terminal draws its grid, so every glyph can be hit:

- **Selection.** Text is selected by dragging, double-clicking a word or
  triple-clicking a block, across blocks and across entries. Ctrl+C or the
  right-click menu copies; on release the selection also becomes the primary
  selection. Ctrl+Shift+A or Edit → Select all selects the thread.
- **Inline math** stands in the line as a transparent letter whose letter
  spacing makes it the formula's width, and the formula is drawn on the
  line's baseline. It wraps, selects and copies with the words around it,
  and copies as its LaTeX. (A no-break space would not do: cosmic-text hangs
  spaces past the end of a line.)
- **Cost.** While text streams in, only the block that changed is shaped
  again; the rest are moved. Only the blocks on screen are drawn.

### Maths typesetting mode

**View → Maths typesetting**, or the button in the thread's header, sets
threads as a paper: a serif text font, justified, in a column of fixed
measure. The fonts are Typst's default, Libertinus Serif, and LaTeX's, New
Computer Modern, both bundled (`res/fonts`), or the system's serif or
sans-serif. Settings → Threads has the font, the text size, line spacing and
width, justification, the formula size, and the paper's colours: the theme's,
white, or sepia. A white or sepia paper sets the whole thread in the light
theme. **View → Dark mode** switches the app between light and dark.

### Images

Images in messages and tool results never stay in memory as data. Where the
CLI's output is parsed, off the UI thread, each base64 image is written to
`$XDG_CACHE_HOME/cosmic-term/thread-images/` under a hash of its bytes, with
a thumbnail when it is larger than 960 pixels, and only the path is kept.
iced decodes the images on screen and drops them when they leave it. Images
in markdown are shown from their paths, relative to the thread's directory.
Files unused for two weeks are removed at startup.

Clicking an image opens it in a window of its own (`thread::viewer`), laid
out as Marginalium lays out a page: a toolbar with fit, actual size, zoom
out and in and the percentage, copy path and open in another app, and the
image on a neutral ground that scrolls when it is larger than the window.
Ctrl+scroll zooms.

### Memory

Memory goes where the user is looking:

- **Transcripts are read a line at a time.** Each line is cut down to what the
  view keeps before the next is read, and only the last 20 user turns are
  kept, as t3code pages its history; **Show earlier turns** reads further
  back. A transcript of hundreds of megabytes costs a few hundred kilobytes.
- **Tool input and output are cut** to 12 KB, keeping both ends. The
  assistant's prose is kept whole.
- **Threads not shown are unloaded** beyond `loaded_threads` (4), least
  recently shown first: their entries are dropped, and read back from the
  transcript when the tab is shown again. A thread mid-turn is never
  unloaded.
- **Idle agents are stopped.** Each CLI is a Node process of a few hundred
  megabytes. One idle for `idle_minutes` (10), or beyond `live_agents` (3)
  running at once, is stopped by closing its input; the next message starts
  it again with `--resume`.
- **Layout is cached per block, and drawing is culled** to the blocks on
  screen; formulas and images live in bounded caches.

Settings → Threads shows how many threads are loaded, how many agents are
running, and roughly how much memory their text and formulas take.

### Opening threads

- A thread profile's launcher opens a thread in the chosen folder, or
  resumes a chosen Claude conversation as a thread. Its other choices
  (attaching, moving, continuing with the other agent) open terminals.
- A thread profile with a working directory opens straight into a thread.
- **New tab** from a thread opens another thread in the same place.
- `cosmic-term --thread DIR [--resume ID] [--prompt TEXT]` opens one at
  startup.

### Testing

`thread::snapshot` builds the real view, sends it pointer events through
iced's `UserInterface`, and draws it with the tiny-skia renderer, with no
window or GPU: selection and copying are tested end to end, and with
`CT_SNAPSHOTS=<dir>` the frames are written out as PNG files.
`thread::session::live` (ignored by default) runs the real CLI: a turn that
streams math, and a permission prompt answered with Allow. In debug builds,
`COSMIC_TERM_DEBUG_STEPS` scripts the running app (`thread::debug`).

### Not yet done

- Codex threads: Codex profiles still run in a terminal. `codex app-server`
  speaks a similar JSON-RPC protocol and would fit the same model.
- Closing a thread's tab mid-turn ends the turn, as its CLI is a child of the
  window. The conversation itself is kept by the CLI and resumes.
- Pasting images into the composer; images are attached from files.
- Syntax highlighting in code blocks.

## Not yet verified

- That a copied Codex conversation resumes natively. Codex keeps its own
  SQLite index of conversations, so Codex conversations always continue as
  new ones from a transcript. A moved Claude conversation was tested: it
  resumes under its new id in the new directory.
- The launcher's transfer and new-worktree forms on screen. Their steps are
  tested against real Git repositories and over ssh between neo and
  au-laptop, but the forms themselves have not been tried by hand.

- That a `/add-dir` added during a session appears in the next environment
  snapshot. `--add-dir` does.
- That Zed opens several `ssh://` URLs for one host as a single multi-root
  remote workspace. Zed's CLI source suggests it does.

## Out of scope for the first version

- Renaming the desktop file, metainfo and icons to the new app ID. Until
  then, the running window has no matching desktop entry.
- Choosing the working directory at launch time (use one profile per project
  for now).
- Mapping one Zellij session with several Zellij tabs to several UI tabs. One
  session per UI tab is simpler and makes each tab independently detachable.
