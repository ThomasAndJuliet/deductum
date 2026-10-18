# Deductum (formerly Isabellum): decisions

Deductum opens a folder of Isabelle theories and shows it as a literate
document. You and any number of agents work in the same live prover session.
The app is a sibling of COSMIC Terminal Agents and lives in the same repository.

This app was called **Isabellum** until 2026-10-11 (C17a); the record below keeps its history under the new name. On branch `multi-prover` it is being extended to Lean and Rocq (Coq)
backends alongside Isabelle (C16). That design is in
[deductum-provers.md](deductum-provers.md). The rest of this record still
describes the Isabelle-only app as implemented.

This record keeps three things apart:

- **Specified**: behaviour the human decided.
- **Chosen**: architecture the implementing agent decided, under the human's
  standing authorisation.
- **Implemented**: what the code does now, and how it was verified.

The working ledger with the full reasoning is
`.local-design/isabelle-app.md`. It is developer-local and not tracked.

## Contents

1. [Components](#components)
2. [Specified behaviour](#specified-behaviour)
3. [Chosen architecture](#chosen-architecture)
4. [Implemented and verified](#implemented-and-verified)
5. [Not yet implemented](#not-yet-implemented)

## Components

```text
Deductum app (Rust, libcosmic)  ──┐
agent's MCP adapter (Python)     ──┼── JSON lines ──>  session host (Python, one per user per machine)
deductum command, Terminal Agents ┘   socket, or         │ edit log, undo, grants, notices,
                                       ssh + `connect`     │ autosave, disk watch, lifecycle
                                                           └── JSON lines ──> isabelle deductum_backend
                                                                              (Scala, Isabelle2025 PIDE)
```

| Component | Path | Role |
|---|---|---|
| Backend | `deductum/backend/Isabelle2025/src/backend.scala` | One PIDE session per prover epoch. Holds the prover's copy of each opened theory and reports command spans and status, messages, proof states at any offset, and query operations (Sledgehammer, Find Theorems). |
| Session host | `deductum/host/deductum_host.py` | Owns sessions, the authoritative buffers, the edit log, undo, grants, notices, autosave and disk watching. Starts and stops backends. Standard library only. |
| Transforms | `deductum/host/deductum_ot.py`, `src/bin/deductum/ot.rs` | Rebasing a change past later changes under the `strict`, `partial` and `merge` policies. |
| MCP adapter | `deductum/host/deductum_mcp.py` | The agent tools. Run with `deductum_host.py mcp [--ssh DEST]`. |
| App | `src/bin/deductum/` | Sessions sidebar, the literate and source views, panels, the `deductum` command. |
| Renderer | `prose/prose-render`, `prose/prose-spell` | Cerea's shared crates, as a git subtree. They replace the `#[path]` borrowing of `src/thread/{markdown,rich,math}.rs` described under "The literate view" (merged from `shared-prose`). |

## Specified behaviour

| Id | Decision |
|---|---|
| C1 | A distinct app named **Deductum**, with the command-line helper `deductum`. It opens a theory folder for interactive proof work, using literate programming. It works locally or remotely using Terminal Agents' machine list. |
| C2 | You and multiple agents (over MCP) use the **same live PIDE session**. Each agent has its own query position, and queries never move your caret. Edits are guarded by revision. The session persists after the app closes, with explicit Stop and Resume. |
| C3 | A Sessions sidebar. **Stop** keeps the buffers and recovery state, and Resume starts a new epoch. A query never resumes a stopped session. `deductum .`, `--machine`, `--session` work like `zed .`. |
| C4 | `.thy` files are authoritative. Prose in `text` blocks is editable. Clicking rendered LaTeX or Isabelle content reveals its own source. Proofs are edited inline, with an expanded proof workspace that can pop out into its own window. Agents may be granted direct editing rights. |
| C5 | Undo is per author: Ctrl+Z undoes your own edits, and agents' changes have a separate revert action. |
| C6 | Grant scopes: the whole folder, one theory, or a selected proof. The grant screen defaults to the folder. |
| C7 | A sibling app. Undo is handled intelligently, so that work is **hard to lose**. Agents are told when their edits conflict. **Pause** keeps the Isabelle server running but pauses work. |
| C8 | Both collaborative and **independent** agent work. Independent work means the agent's own session on its own copy, which you can watch and which comes back as a reviewed merge. |
| C9 | Editing rights last until revoked. Agents are identified by conversation ID, accepting that this is not a security boundary. |
| C10 | From Terminal Agents, the human can open, view and work on the theory an agent is working on. |
| C11 | The session host owns the buffer. The app types locally. |
| C12 | The main Isabelle version is the one isabelle-utp supports: CyPhyAssure's patched **Isabelle2025**. |
| C13 | **Autosave**, and agents can save. When a file changes on disk with no unsaved edits, it is reloaded; otherwise a Reload button is shown, or agents can resolve the conflict. |
| C14 | Two views per theory, **literate** and **source**, with Isabelle symbols rendered in both. **Sledgehammer** integration with a good UI is essential. |
| C16 | In a new worktree, a large extension with **separate Isabelle, Lean and Coq/Rocq backends**. The UI exposes the features that are relevant to the current backend, and manages projects and sessions across the backends. Builds on Cerea's split rendering engine (`prose/`). Design: [deductum-provers.md](deductum-provers.md). |

## Chosen architecture

- **Backend in Isabelle's JVM, host in Python.** The backend uses Isabelle's
  Scala session API directly. It does not wrap `vscode_server`, which flattens
  output and serves only one client. The host keeps the multi-client
  semantics, so they can be tested without a prover.
  - This refines C12's first proposal, which put the whole host in the JVM.
  - The backend is derived from Isabelle2025's VSCode sources and
    `Query_Operation`.
- **Version isolation.** Code that depends on the Isabelle API lives in
  `backend/<ISABELLE_IDENTIFIER>/`. The host offers only runtimes that have a
  matching directory. Supporting another release means adding a directory.
- **Continuous checking.** Every theory the host opens is required, with the
  whole text as perspective. `editor_output_state` is on, so every command's
  results include its proof state. Proof states at any position are therefore
  snapshot lookups, with no caret or overlay.
- **Pause.** Nothing is required and the perspective is empty, the mechanism
  jEdit uses when continuous checking is off. Running executions are
  cancelled. Interrupted commands are re-run on Resume by removing and
  restoring their last character, because PIDE keeps their failed executions.
- **The edit log.** Each theory's log is append-only and records the author
  of every entry. Entries are never removed, so earlier text stays
  recoverable. Undo, revert, disk reloads and merges are new entries in the
  log.
- **Rebasing policies.**
  - `strict`, for agents' edits: an edit made against an old revision is
    applied if the text it targets is unchanged, and refused with the current
    text if not.
  - `partial`, for undo and revert: overlapping parts are left out and kept as
    recoverable alternatives in the app. Before that, if the text a change
    inserted is still present unchanged, the change is reversed directly.
  - `merge`, for the human's typing: nothing is dropped. A deletion only
    removes text that is still there.
- **Notices.** The host tracks each agent change's current span. An edit by
  someone else that touches the span, or a revert of it, produces a notice for
  that agent, delivered at the end of its next tool result.
- **Grants.** Folder grants use real paths, so imported theories outside the
  folder are not covered. A proof grant is a tracked range that grows with
  edits at its ends, not a stale byte range.
- **App replica.** The app keeps at most one edit in flight: the span between
  the confirmed text and the local text. Remote changes are applied to the
  confirmed text and the local difference is merge-rebased over them.
- **Autosave.** About 2 s after the last edit, and on Pause, Stop and app
  detach. Files are written to a temporary file and renamed. Line endings and
  BOM are kept.
- **Independent sessions.** A git worktree on branch `deductum/<id>`, started
  from the human's live buffers. The merge is a line-level three-way rebase
  into the shared session. The merged text is attributed to the agent and
  can be reverted.
- **Linking agents to sessions (C10).** The MCP adapter reports the agent's
  conversation ID, Zellij session and tab. The host's `find_sessions` matches
  them. This is used for discovery only, not as a credential.
- **The literate view.** Theories are split into blocks using the prover's
  command spans, or a lexical scan until those arrive; the scan never drops
  source. Document text is converted to Markdown and set by Terminal Agents'
  renderer, sharing `src/thread/{markdown,rich,math}.rs` through `#[path]`.
  Formal blocks show decoded source with a status strip.

## Implemented and verified

| Area | Evidence |
|---|---|
| Transforms | `deductum/host/tests/test_ot.py`: 14 tests, including randomised properties. Strict rebasing equals applying both changes simultaneously; merging never drops inserted text; inverses round-trip. The Rust port has matching tests. |
| Host against a real prover | `deductum/host/tests/integration_host.py` passes 28 checks on au-laptop with CyPhyAssure Isabelle2025: proof states, refusing edits without a grant, rebasing stale edits, rejecting conflicting ones, overlap notices, per-author undo and redo, revert with notice, autosave, clean reload as a logged edit, disk conflicts holding autosave, agent resolution, Sledgehammer with the span to replace, Pause refusing queries immediately, Resume rechecking, Stop and Resume with a new epoch, agents unable to resume the human's session, `find_sessions` by Zellij session, and refusal after revoking. |
| MCP | A scripted agent session on au-laptop: open, check, read, proof state, Sledgehammer refused without rights and then inserting `by simp` after a grant, history, Find Theorems. |
| App views | `src/bin/deductum/snapshot.rs` renders the literate view, the source view, and the Sledgehammer, Agents and History panels headlessly with tiny-skia (`CT_SNAPSHOTS=dir`). |
| App against a real host | `src/bin/deductum/live.rs` (`DEDUCTUM_LIVE=d59500`, `--ignored`) drives the window's state machine over ssh. You type in the source view while an agent replaces a `sorry` against an older revision; the replica converges on both edits. The goal query, Sledgehammer and autosave are checked too. |
| Unit tests | 43 Rust tests (`cargo test --bin deductum`): symbols and offset maps, blocks, the literate conversion, the replica with concurrent edits, typed symbol escapes, the command line. |

Building and running:

```sh
cargo build --release --bin deductum                       # inside the Flatpak SDK on neo
python3 deductum/host/deductum_host.py runtimes           # the runtimes the host can use
claude mcp add deductum -- python3 ~/.local/share/deductum/host/deductum_host.py mcp
```

The app installs `deductum/host` and `deductum/backend` into
`~/.local/share/deductum` on each machine it connects to. The host builds
the backend jar with the runtime's `isabelle scala_build`.

## Also implemented

- **The pop-out proof window (C4).** "Pop out" on a formal block opens it in
  its own window: its source, the goal at the caret and Sledgehammer, on the
  same buffer. Closing the window or "Bring it back" returns it to the main
  window.
- **Editing prose (C4).** A prose block opens its source, and the block's
  rendered preview follows each keystroke below it.
- **"Open in Deductum" in Terminal Agents (C10).** It is in the tab menu of
  session and thread tabs, and in the Session menu.
  - `agent::open_in_deductum` asks the session host on the agent's computer
    (`deductum_host.py find --zellij NAME`, or `--folder DIR`) for the
    session the agent used.
  - It then runs `deductum --machine NAME --session ID`. Failing that, it
    opens the agent's project folder.
  - Deductum adds a link to a machine it is asked about but doesn't yet
    know.
  - The action is offered only when Deductum is installed, either as
    `deductum` on PATH or as the Flatpak. Terminal Agents checks once at
    startup.
- **A Flatpak package.** `packaging/flatpak/dev.twright.Deductum.yml`
  packages a release binary built in the same SDK, together with the host
  and backend sources. It builds and passes `check-flatpak-catalog`. It has
  not been published to `home-apps`.
- **A renderer fix.** In libcosmic's tiny-skia backend, raw text, fills and
  strokes reused the clip mask left behind by whatever was drawn before them.
  Rich text below a text editor therefore lost its glyphs. The fix is in
  `iced/tiny_skia/src/engine.rs` of the household libcosmic fork.

## Added after trying the preview (C15)

- **Diagnostic commands at any position.** Quickcheck, Nitpick, `try0` and `solve_direct` run without editing the text.
  - The backend's ML prelude (`backend/Isabelle2025/ML/deductum_prelude.ML`, loaded with `use_prelude`) registers a query operation, `deductum_command`. It parses the command with the theory's keywords at the print function's position, so the command's messages carry its execution id. The backend collects them with the query's own results.
  - The **Quickcheck** panel and the `isabelle_run_diagnostic` MCP tool use it.
- **LLM hole filling** (`fill_hole`).
  - The host asks Claude (`claude -p`) or Codex (`codex exec`) on the session's machine for candidate proofs for a `sorry`. The prompt holds the theory header, the preceding text, the lemma and the proof state.
  - The host checks each candidate with `deductum_command` before offering it with Insert.
- **Sledgehammer options.**
  - `sledgehammer_timeout` and `sledgehammer_provers` are set on the running prover (backend `options`, host `prover_options`) and kept with the session.
  - A `sledgehammer` typed in the text offers its proofs with Insert in Output, replacing the command.
- **Prover activity.**
  - The host samples the prover process group's CPU and memory, and times running commands.
  - The backend reports each theory's imports and header errors.
  - The header's centre shows the state, with Interrupt. The Activity panel lists theories with progress, running commands and the log.
  - Imports that cannot load are named.
- **Layout.** A three-column responsive layout:
  - a resizable left sidebar with Sessions, Outline, Theorems and Problems;
  - a resizable, collapsible right sidebar.
  - Settings are kept in `~/.config/deductum/settings.json`.
- **Editing.**
  - Unboxed editors.
  - Goal-aware completion (`fix`, `assume`, `show`, `have`, `proof (intro …)` from the first subgoal).
  - Fuzzy `\` completion with LaTeX names, and a Symbols panel.
  - Isar auto-indentation; Tab and Shift+Tab indentation.
  - Vim keys (`vim.rs`: a modal machine over the editors' own actions).
  - Ctrl+= and Ctrl+- zoom.
- **Symbols.**
  - `\<^sub>`, `\<^sup>`, `\<^bsub>…\<^esub>` and `\<^bsup>…\<^esup>` render as Unicode scripts where possible, each standing for its whole escape.
  - `\<^bold>` renders bold; other control markers are greyed out.

## Added in the second round of feedback (2026-10-11)

- **Undo, redesigned.**
  - *Granularity.* An undo step is a word of typing, a run of deletions, or one discrete edit (a paste, a completion, a symbol, an inserted proof, a vim command). A step also ends at a pause of a second, a caret jump, Enter, or opening or closing a block. In vim mode a whole insert session is one step, as in vim.
  - The app sends each step to the host as its own edit (`Theory::steps`), so a step never merges with the next while an edit is in flight. Undo and redo wait until earlier edits have reached the host.
  - *Semantics.* Undo always consumes its step. Text that someone else changed since is left as they changed it: undo ranges are cut at later edits' boundaries, so only the characters both touched are kept (`split_at`). There are no "could not be applied" cards. The reply carries a one-line note ("Undid at line 24; kept what Claude changed there since") and where the undo acted, and the app moves the caret there. A step that later edits replaced entirely is passed over and counted in the note.
- **Proof state** (prelude query `deductum_goals`). Isabelle splits the state into parts: the chained facts, then for each subgoal its fixed variables (with types), assumptions and conclusion, pretty-printed at the panel's width. It also returns the whole state as text with the same settings.
  - The **Goals** view shows each subgoal as `fix` / `assume` / `show`. It also shows what the step changed (subgoals solved and new, against the previous command's state) and a Types switch. The **Text** view is Isabelle's own printout.
  - The command's own output (a theorem, a value, a warning, typed `sledgehammer`, `nitpick` or `quickcheck` findings, proof outlines) comes first, with Insert for the proofs it offers. Outlines are inserted line by line under the command.
  - The state shows beside the block it belongs to when the window has room for both columns, else under it. The right sidebar's Output tab was removed, and the pop-out proof window follows the same rule.
  - Message text skips the bodies of `xml_elem` markup (tooltip types), which had been spliced into the text.
- **Typed keywords.** Enter after a line reading `claude` or `codex` (with an optional hint) turns the line into `sorry`, in the same undo step, and asks that LLM. Its proofs, each checked first, and the facts it suggests for `using … sledgehammer` show with the proof state. LLM filling has no button. A proof that closes a theorem PIDE already registered ends with a duplicate fact, which is counted as success.
- **Sledgehammer settings** are a folded section under the proof state. They are applied to the prover for `sledgehammer` typed in the text; there is no Run button.
- **Literate view.**
  - Formal blocks are read-only editors: a click opens the block with the caret where it landed. All formal blocks share one width (the widest one's), and the current block is darker with an accent strip.
  - Prose and section titles are edited as their text alone, in the font, size and width they read in. Single line breaks show as spaces (the source keeps them), so the paragraph wraps as on the page, and code inside keeps code colours.
  - j and k move by visual rows and cross into the next block at an edge, and the selected block is centred.
- **Hover and definitions.** The backend's `info` op returns PIDE's tooltips (types, what a name is, notation, messages) and the definition's location. Hovering an editor shows them; Ctrl+click goes to the definition, opening its theory.
- **Collaboration.**
  - Agents ask for editing rights with `isabelle_request_access`. The human sees Allow and Deny, and the agent gets a notice with the answer.
  - `isabelle_human_view` gives where the human is, the selection and the proof around the caret. `isabelle_show` offers the human a place, or goes there if they follow the agent.
  - `isabelle_add_note` leaves a note on lines. It moves with edits and shows under its block; the human replies or resolves, and the reply reaches the agent as a notice.
- **Files.** The folder is relisted every 5 s and the app is told when theories appear or disappear. The sidebar groups theories by folder, drops a shared name prefix, shows status as a dot, and gets a filter for large folders. The left sidebar starts hidden.
- **Header.** Open the session's folder in Files, Zed or a terminal (Terminal Agents, else COSMIC Terminal, else any), over sshfs or ssh for remote sessions.
- **Copy and paste.** Ctrl+C, Ctrl+X and Ctrl+V work in every vim mode. Copy writes Isabelle source, with symbols as escapes.

- **Adding and removing blocks.** Under the selected block, and at the end of the theory, are + Text, + Section and + Lemma (and Delete for the selected block). `dd` in vim block navigation deletes it. A new block opens for editing, and each insert or delete is one undo step. Until the prover has seen new text, `theory::blocks` splits it where a theory-level command starts a line, so a new block is a block at once.
- **Prose.** A click opens prose with the caret where it landed: the pointer's place is replayed in the editor once it is laid out in the same typography. The prover's marking of a text's body leaves it in the prose font. Rendered prose joins a paragraph's source lines (the shared renderer keeps a single line break, as chats do). The theory's `end` is not shown.
- **Hover in output.** The backend sends each message's hover tips (a term's type, what a name is), from the markup it otherwise strips. Goals and messages show them without asking the prover. Hover cards are as wide as their text.
- **Sidebar.** Theory names are shown in full; sessions are rows marked by weight rather than filled buttons.
- **The thin maths (C15).** This is the upstream fix the human recalled. In libcosmic's wgpu renderer (`iced/wgpu/src/image/vector.rs`), tiny-skia's premultiplied pixels went to a pipeline that blends straight alpha, so the alpha applied twice and the antialiased edges of light glyphs thinned. The fix divides it out. It is in this branch's libcosmic (not yet the main checkout's), and Deductum builds with wgpu.

- **Proof tools typed in the text.**
  - With the caret on `sledgehammer`, `nitpick`, `quickcheck` and the like, its findings replace the proof state as they come, with Insert, elapsed time, a progress bar and Stop. Sledgehammer's settings show there too, and the state refreshes whenever the command at the caret says more.
  - The `qed` that fails because a proof waits on such a tool is shown as unfinished, not failed.
  - Stop (also in the header, named after the tool) cancels only the tools' executions (backend `cancel_tools`). Deleting a tool's text stops it.
- **`claude` and `codex`** start when typing pauses on their line (1.5 s), as well as on Enter. They show as Sledgehammer does: elapsed time, a progress bar, Stop, then checked proofs with Insert.
- **Navigation.** Ctrl+click goes to a definition and Back (header, Alt+Left) returns. Alt+click opens the definition in a window of its own, with Open here.
- **Vim.** Normal mode has a block cursor (the character under the caret, selected). A mouse selection enters visual mode, and a click leaves it. Every caret placement clears the selection, so the next motion moves.
- **One caret.** Shown blocks take clicks through a layer over them, so their read-only editors never take focus. Closing a block or moving the selection unfocuses all editors. The proof window opens for proofs only, and closes when another block is opened.
- **Inserting.** Hovering the gap between two blocks offers + Text, + Section and + Lemma there. While prose is edited, the maths or antiquotation under the caret shows rendered beneath it.
- **Output text.** The Unicode script markers that prover output already carries (⇩ ⇧ ⇘ ⇙ ⇗ ⇖) render as scripts, as the escapes do.
- **End to end.** `live_literate` and `live_types` drive the app's own update loop against the real host and Isabelle on au-laptop. They add a lemma and type into it, read its goal in parts, take back one word with one undo, delete a block and restore it, check the autosaved file, and switch Types.

- **Proof tools beside their goal.** On `sledgehammer` and the like, the tool's progress and findings show above the goal it works on, which shows as usual. Sledgehammer's settings show only there. The header names a running tool with its own Stop, and Interrupt shows only for other proofs. Stop repeats until the tool has stopped, and a stopped tool (Isabelle's `Interrupt_Breakdown`) reads as if its time ran out, not as a failed proof.
- **The window, end to end.** `snapshot::Harness` draws the real view with the real renderer, keeps widget state (focus, carets) between frames, and sends mouse and keyboard events through the widgets. `live_ui` uses it against a real host: point at a shown lemma, click (it opens with the caret at the click), type through the editor widget (the text reaches the host), click outside (it closes), hover a name (the prover's tooltip), and use vim's normal mode (a block cursor). The live tests stop and discard their sessions on the host afterwards.

- **Agents, end to end.**
  - Deductum's MCP server is registered with Claude Code and Codex on au-laptop (local host) and neo (au-laptop's host over ssh, `mcp --ssh d59500`).
  - A client on another machine names files as that machine does; the host reaches them through COSMIC Terminal's sshfs mount of it (`translate_path`), and git there trusts mounted repositories.
  - Tested with real agents. Claude (`claude -p`) and Codex (`codex exec`) each opened a session on a neo folder, asked for rights, waited for the grant, proved a lemma, checked it, saved or left a note. A scripted MCP test covers the same, and another covers independent work: fork, prove in the copy without asking, preview, and the human merges it into the shared session as the agent's change.
  - Codex starts MCP servers without a conversation id: the adapter takes the Codex process as the conversation (so a grant holds for its run) and labels it Codex. Codex asks for approval of MCP tools itself.
  - Fixed along the way: tools with relative paths (request access, show, notes) now resolve them against the session folder, the edit tool takes a single `old_text`/`new_text`, and the goal at a `sorry` is the goal it stands in for.
- **Host restarts** resume the sessions that were running (a new epoch); an explicit Stop stays stopped.
- **Goal placement.** Beside the text (both views), the goal is level with the construct the caret is in: the Isar statement around it (`have`, `show`, `case`, …, with its proof), or the step if that is long. In a narrow window, a long proof's goal floats just after that construct rather than after the whole proof. The text column has a readable width (long code lines wrap), so two columns come sooner.
- **One caret, kept.** The page has one element per block and the same wrapping whether or not the goal is beside, and editors are always inside a popover (the tooltip added when there is one). So the open editor keeps its focus and caret when the proof state arrives, a tooltip comes, or the selection moves; a widget-level test reproduces the loss without this.
- **Keywords and vim.** `claude`/`codex` work wherever they end a line (after `show ?thesis unfolding …` too), not only alone. Visual mode starts with the character under the cursor selected, as in vim.

- **Prose a paragraph at a time.** A text block opens as the paragraph the caret is in (or that was clicked); the block's other paragraphs stay rendered above and below it. j and k move to the next paragraph before the next block. Single line breaks show as spaces, and the maths or antiquotation under the caret renders beneath the line.

- **On the real development.** `live_tock` runs a fresh copy of tock-Circus on UTP-Reactive, as the human's session does, and drives the window. It clicks into a lemma of `Timed_Circus_Assignment` and types; undoes word by word; hovers a constant (`Timed_Circus_Contract.timed_contract_wf`); Ctrl+clicks to its definition in `Timed_Circus_Contract.thy` and Backs out; then types `sledgehammer` in a proof, sees it beside the goal, and stops it (the proof reads as unfinished). It found and fixed three bugs:
  - hovering or clicking a shown block over its padding missed the text;
  - in source mode the goal disappeared when only the literate layout had room for two columns;
  - a stopped tool's own failed command made its proof read as failed.
  Code editors now set their line height explicitly (1.4), so the goal lines up with the code even past wrapped lines.

- **Collaboration, in the window.** `live_collaboration` has an agent connection point the human at a lemma and leave a note on another, while the app is drawn and clicked. The banner's Go there selects the lemma, and the note's Reply and Resolve buttons reach the agent as notices.

- **Maths in the line being edited.** In the paragraph being edited, a formula reads as Unicode (`$x \le y^2$` as `x ≤ y²`) through Isabelle's symbols and Unicode scripts, one shown character for each source character (the rest zero-width spaces), so offsets and edits map exactly. The formula the caret is in shows as its source, to edit (and renders beneath the line). Fractions read as `a⁄b`, roots as `√x`, and `\text{…}` and the like as what they hold. A formula with no such form (matrices, aligned displays) stays as source.

- **The goal's construct, tinted.** The editor tints (accent at 14%) the construct the goal is for (the Isar statement around the caret, or its step if that is long), in a block and in the source view. The tint is drawn by the gap widget that holds an inline goal, so its lower part moves down with the text when the gap opens inside the construct. The source view is now always laid out at full height in a scroll view (centred at a readable width), so where its lines are drawn is known.
- **Reference windows.** Alt+click opens a definition in a window of its own: the whole theory, read-only, scrolled to the definition, which is tinted, with tooltips under the pointer. In it, Ctrl+click moves that window to the next definition (Back returns) and Alt+click opens another window. Several can be open.
  - Theories of the session use the prover's markup.
  - A library theory, which the session's prover has not checked, is read from disk without being opened as a document of the session (`read` with `peek`). Names in it are looked up through the prelude's `deductum_entity` query, run in the main theory's context at the caret. It gives each kind of thing the name is (constant, fact, type, class, locale), with its full name, its defining file and line, and its statement or type. Verified live against HOL: `rev` → List.thy line 87; `append`, `rev_rev_ident`, `list`, `Nat.Suc`.
- **Bracket pairs.** Isabelle colours a delimiter with a letter in it as a literal, like UTP's `)\<^sub>e`, but not the plain `(` that opens it. The matching bracket now gets the same colour, so `(P ∨ Q)ₑ` reads as one pair.
- **The goal beside the text** is always level with the construct the caret is in, and moves only as far as it must to stay in sight. It moves up when it would run off the bottom of the window (never above its block's top or the window's), and down when the construct has scrolled up out of view. It is drawn by a widget (`Beside`) that takes no room in the page and scrolls with it. The popover it replaces was kept inside the window, and jumped to the top of the window when the goal did not fit below.
- **Waiting, not stale.** The app now uses the revision each prover report covers. From the earliest change the prover has not seen, the text shows as waiting (grey) until a report covers it, and the goal panel says "Checking…" rather than "No proof state". Before, the old statuses stayed up while the prover caught up.
- **Stop and delete for proof tools** cancel the executions the backend saw running, as well as those it can find in the current version. While the prover is busy, the current version's executions may not be assigned yet, and Stop then found nothing to cancel.
- **`claude` and `codex` work as Sledgehammer does.** The keyword line stays as typed. The prover sees it as a `sorry` of the same length, so the goal there is there and offsets are unchanged. The proofs come beside the goal, Insert replaces the line, and deleting or changing the line stops the job (and its LLM process) and drops its proofs. A model can be named in brackets: `claude[opus]`, `codex[astra]`.
- **Facts for Sledgehammer, early.** Alongside the request for proofs, the host asks the same LLM a short question for facts only. Facts from either answer are shown as soon as they are checked to exist, before any proof is checked. They matter most when the LLM cannot prove the goal itself. Live, with `claude[haiku]`: facts after 6 s, together with the proposals and ahead of the checking. The whole keyword (and its model) is coloured as the `sorry` the prover sees.
- **The keyword pause** is 2.5 s, so `claude[opus]` and a hint can be finished after a hesitation (Enter asks at once). A line changed while its job runs stops that job; typing `[` after `claude` already does.
- **Where queries go** (the human's choice): `claude` and `codex` lines go by default to the session's **persistent helper**. That is one real Claude or Codex conversation per session and model, run in the session folder and resumed for each query (one at a time), so it remembers the theory and earlier attempts. The human can open it (`claude --resume ID` / `codex resume ID`, copied from the tool section) to talk to it, or start a new one. The other choices are a fresh request each time, or a listening external agent, picked in the tool section.
- **External agents** listen with `isabelle_listen` (giving the name the human uses) and answer with `isabelle_propose` (proofs, facts, a message). Their proofs are checked by the prover and shown with Insert, as claude/codex results are. The human can also reply in the agent's own conversation, and the agent may edit directly if it has rights. A line `@name hint…` asks that agent directly; the names of listening agents are completed after `@`. A query whose line is changed or deleted is withdrawn, and the agent hears so the next time it listens. Verified live: proposals `by simp`, `by auto` and a nonsense method came back as passed, passed and failed; two helper queries shared one conversation.
- **`isabelle_human_view`** gives agents the lemma (name and statement) and the step the caret is in (the construct the editor tints), so "prove this lemma" and "this step" mean what the human sees.
- **Results belong to their cell.** A claude/codex job's proofs show only in the cell holding its line (and again on returning there). A tool's findings and the goal of another cell are dropped when the caret moves to a different cell. While a change is unchecked, output about the command that was there before reads "Checking…".
- **Highlighting numbers are unique across theories.** An editor keeps its highlighter while the number is the same, so a reference window moved to another theory showed the old theory's colours, shifted, with script characters in the wrong font.
- **Hover does not touch a selection.** Finding what is under the pointer clicked the editor and restored its cursor, but a double- or triple-click selection cannot be restored that way, so it was lost as soon as the pointer moved. There is no hover while text is selected (vim's block cursor excepted).
- **An edit the prover does not see** (typing in a keyword line's hint) keeps the report current, instead of leaving the theory waiting for a report that never comes.
- **Completion in prose kept its line breaks wrong** (fixed). Accepting a completion in a paragraph being edited wrote the paragraph as shown back to the source: a space for each line break and zero-width spaces for its indent. It is now taken against the text as shown, as typing is. Two theories had picked this up (10 line breaks in `Timed_Circus_Assignment.thy` and `…_Assignment_Traces.thy`); they were repaired on disk, with backups in neo:/tmp/deductum-zwsp-backup, and both hosts reloaded them.
- **Errors under a block** say what went wrong (the goal Isabelle lists after "Failed to finish proof" is the goal view's) and draw scripts in the scripts font. So do hover cards.
- **Proof method names** (Isabelle's "operator" markup) are in the text's colour, as jEdit sets them, not dimmed.
- **Floating buttons** (pop out) are drawn in a layer above the editor they float over. The literate block's button is anchored to the code's top right as laid out.
- **A keyword line not asked yet** says when it will be: when typing pauses, or now with Enter.
- **Light mode.** The light syntax palette keeps the dark one's hues (keywords purple, names amber, types teal, strings green, constants orange, comments grey), made deep enough for at least 4.5:1 contrast on COSMIC's light page (#d6d6d6) and more on the code cards (One Light's colours fell to 1.8–4.2:1 there). The current block is a wash of the accent in a light theme rather than a darker grey, and the construct's tint is fainter. Theme changes now apply at once: each editor's highlighting carries whether the theme is dark, so a change redoes its colours, which editors otherwise keep with their lines.
- **Machines inside the Flatpak.** Terminal Agents writes machine keys as ids, `(0)`; they parse now. Before, the sandboxed app found no machines, since the `ct-mount` fallback cannot run there, so neo never appeared.
- **Vim's block cursor** shows as soon as a block opens and after a click, not only after a vim key. At a line's end it rests on the last character, as vim does.
- **The goal beside a block is close to it.** The text column is as wide as the code blocks (their code and status strip, between 440 and 680 px), with prose following it, and the goal is 10 px past it. Before, the column was at least 560 px plus a margin and the medium spacing, which left a wide empty band. Because the column follows the blocks, the window needs less width before the goal goes beside them.
- **Where the goal goes beside the text** (the human's rule). The page draws the goal past whatever items are level with it, measured as laid out: just past the code block among code blocks, past the text column where a paragraph, heading or note wider than the blocks is beside it. It never covers text: a test renders a small lemma between long paragraphs, a note card and a block with a very long line, and checks the gap before the goal is empty on every row. It fails if the blocks' width cap is removed. Code blocks are capped at the text column's width, where longer lines wrap; note cards and banners stay in the column.
- **Beside or under** is decided from the widths: the text column, the gap, a goal of at least 320 px and room for the scroll bar. The side margins shrink to nothing before the goal goes under, and the page is centred with its margins included.
- **The window**: no inset content card, so the theory's scroll bar is at the window's edge; the view draws its own opaque background. Narrow windows have small side margins.
- **Blocks go when nothing is left of them.** Backspace in an empty block, or `dd` on a block's only line in vim's normal mode, takes the block away and goes to the end of the block before it, as joining lines does. An empty block that is left (Esc, a click outside, another block) goes too. Prose counts as empty when its paragraph is blank and nothing but its command and brackets surrounds it. Undo brings a block back.
- **The goal beside the text can be turned off** with a toolbar toggle, which is remembered. Off, the goal is under the proof and the page is the readable text column alone, centred.
- **The layout does not change as the caret moves.** With the goal beside on and room for it, the page keeps the goal's column whether or not there is a goal at the moment, so moving onto prose or a block without a goal leaves everything where it was. Without the goal beside (the toggle off, or a narrow window), the centred column is 560–880 px, following the code blocks' width, and the blocks' width cap follows the column in use.
- **An insert palette beside the text** when there is no goal to show there (prose being edited, or a proof with no state yet). It is searchable and follows the caret's context: in maths (`$…$`, `\(…\)`) or a quoted term, Isabelle symbols (the common ones, or a search by name or abbreviation); in prose, antiquotations and markup (term, theorem, constant, type, maths, emphasis, bold, citation, list item, cartouche, footnote); in a proof, Isar (have, show, proof … qed, using, unfolding, by simp/auto, sledgehammer, claude, nitpick, quickcheck). A click inserts at the caret.
- **The palette edits text.** Each entry puts something before and after: with text selected, it wraps the selection (emphasis, bold, code, term, theorem, constant, type, maths, cartouche, URL, citation, footnote); otherwise it inserts both with the caret between, ready to type. Wrapping inserts the closing part, then the opening part, so the selected text itself is not rewritten. A double-click selection, which the cursor cannot describe, is found around the caret.
- **Markup reads as it renders unless the caret is in it.** In the paragraph being edited, `\<^bold>‹…›` is bold and `\<^emph>‹…›` italic in the prose's own font, `\<^verbatim>‹…›` and antiquotations show as what they hold, set as code. Their delimiters are there but zero-width, so offsets and edits map exactly, and the construct the caret is in shows as source, as maths already did.
- **Lists are contextual.** In prose the palette offers a bullet or numbered list to begin; on an item's line it offers the next item and a nested one.
- **Each palette click is one entry of the history**: a wrap's two insertions are one step, apart from the typing around them; a symbol is a step of its own.
- **The goal under a cell** has the space between cells below it, as part of the cell.
- **Code in prose is coloured as code while it is edited**, lexically and at once: antiquotations (`@{name` and its brace as keywords, quoted or cartouche arguments as terms, names), maths source (`$`, LaTeX commands, braces and scripts) and raw LaTeX commands. The prover's colours take precedence where it has given them.
- **Up and down leave a block from its first and last rows as laid out**, in one press: a click probe at the row's end says whether the caret is on it. The editor's remembered column used to move the caret sideways there first, which cost an extra press.
- **No jump as the open block changes.** What editing means (vim's mode, the block's checking, how to finish) is in a status bar at the window's foot, always there with a theory open, instead of a row under the open block that moved everything after it. An open block keeps its notes under it. Moving from a text block into the lemma after it moved the lemma by 23 px; a test now holds it within 3. An open paragraph can still be a line longer than rendered, since antiquotations show as source while edited.
- **Read-only views take only copying and selecting** (the goal's Text view, a tool's output): Ctrl+C, Ctrl+A, Shift+arrows, Esc. A click that left one focused used to swallow j, k, the arrows, d and Enter, so block navigation (and `dd` there) stopped working.
- **The pop-out button stays on its block** as the page scrolls; only the goal moves to stay in sight.
- **Pop out in the literate view** is the same floating button as in the source view; the Pop out and Done buttons under the block are gone.
- **Pop out in the source view**: a small button at the top right of the proof the caret is in. The proof window tints the construct too.
- **One Isabelle component list.** Each host registers its backend there and removes others. A host now checks its registration again before each start.
- **Arrow keys** run on past a block's ends into the next block (and back), as j and k do.

## Added with the Lean backend and the settings (C16, C17; 2026-10-11)

- **Two provers.** A session is Isabelle or Lean (Rocq later). The app shows what the
  session's prover can do: Isabelle's Sledgehammer, Find Theorems, counterexamples and symbols;
  Lean's Search (Loogle), `exact?`-style suggestions and its goals as the VS Code infoview
  shows them. Design and evidence: [deductum-provers.md](deductum-provers.md) §14.
- **Settings.** A settings page (Ctrl+, or the header's gear) keeps the app's settings; Vim keys
  moved there from the header.
- **Sessions across provers.** Sessions are listed by machine and folder with the prover's mark;
  a new session starts from the folder's detected projects (a ROOT, a Lake package).
- **The window, reworked** after a review of every frame: notes and errors attached to their
  cells, prose that does not jump when it opens, the COSMIC light palette, margins at every
  width, the tool, symbols, agents and history panels, tab bars, footer and header.
- **An insert bar above the goal.** Inside a proof, a definition's body or a theorem's statement
  there is almost always a goal (or Lean's expected type) beside the text, so the palette that
  takes the goal's place was never there when a symbol or a step was wanted. The goal panel
  (beside, under, the source view's and the proof window's) now begins with one row: a search
  field, as many of the entries most wanted at the caret as fit (the profile's
  `quick_entries`: ∀ ∃ → ↔ ∧ ∨ ¬ ⟨⟩ … in a term, have/show/… or tactics at a step, markup in a
  docstring), and a chevron that opens the whole palette under the row, pushing the goals down.
  Closed, the goals stay where they were. The bar and the palette beside the text share their
  entries and chips (`insert.rs`); a click is `Wrap`, so it wraps a selection, is one undo step
  and gives the focus back to the editor with its caret after the insertion. Symbols show as
  their characters (Isabelle's go into the source as `\<name>`, Lean's as Unicode), their names
  in tooltips; a search finds them by name, abbreviation, LaTeX name or the character itself.
- **What the caret is in** comes from the profile (`formal_context`). Isabelle: a term inside
  quotes or cartouches (which nest), a step elsewhere in a proof. Lean, read lexically: a
  docstring is Markdown; a tactic's start is a blank place after `by`, a line break in a `by`
  block, `·`, `;`, `<;>` or a case's `=>`; everything else (a declaration's header or body,
  `have h : _`, `exact _`, a bracket, the end of a tactic just typed) is a term. In the source
  view the block at the caret is read the same way.
- **The source view keeps the goal's column as the caret moves.** It kept the column only while
  there was a goal, and a click in another declaration clears the goal until the prover's state
  there comes, so each click switched the page to one column and back. It now keeps it whenever
  it fits, as the literate view does; with no goal, prose has the palette there.
- **The pop-out button** is one 20 px icon button in both views, centred on the proof's first
  line and as far inside the right edge as from the top. It was a 32 px button flush with the
  cell's top (its icon drawn about 10 px off centre) in the literate view and 6 px above the
  proof's first line in the source view.

## The proof tools' flow (2026-10-12)

The human: the tools are for interactive use and need a good flow. Walking every tool through
the window against real provers found the friction below; these are the decisions made for it
(chosen, within the decisions above: tools typed in the proof, findings beside the goal and
belonging to their cell, settings folded under the tool, no Sledgehammer button).

- **Findings, not the tool's log.** A tool's section lists what it found, read by `flow.rs`
  from what the prover printed. Isabelle's progress lines (`e found a proof...`,
  `cvc5: Duplicate proof`, `Sledgehammering...`, `Done`) are left out; Sledgehammer printed 20
  lines for two proofs. Each proof is listed once with the provers that found it and the time
  its check took (`e · 5 ms`), in the order found, with try0's own choice and Lean's Try this
  first and proofs whose check failed or timed out last. The status beside the tool's name says
  how far it is (`running · 3 s`, `2 proofs found`, `counterexample found`); five proofs show
  at first and the rest are a click away.
- **The keyboard takes them.** At the tool's line, Alt+Enter takes the highlighted offer (the
  first) and Alt+Up or Alt+Down picks another; the panel and the footer say so. Insert, by key
  or click, replaces exactly the tool's text (or the hole of a `claude` line), as one undo step,
  with the caret just after the proof in the open block as in the source view, and the editor
  focused again, so typing goes on there. Esc while a tool (or an LLM) runs at the caret stops
  it and leaves the block open, saying `stopping…` at once; the next Esc finishes the block.
- **Outcomes said plainly, with what to do next.** A counterexample is a table of each part
  (Nitpick's free variables, Quickcheck's values and evaluated terms) with "the statement is
  false for these values". No counterexample, no proof, a timeout or a Lean error is a sentence,
  and the next step is offered the same way as a proof: Sledgehammer again with twice the time
  (`sledgehammer [timeout = 60]`, its other options kept) or asking `claude` in its place after a
  Sledgehammer that found nothing; Sledgehammer after try0, solve_direct, try or a counterexample
  search that found nothing; the tactic a Lean error suggests (`apply?` after `exact?`).
  `solve_direct`'s facts are offered as `by (rule fact)` (checked live). A stopped tool reads as
  stopped: a cancelled Isabelle2025 sledgehammer fails without an Interrupt, which had read as
  "closes the goal".
- **`claude` and `codex` lines show the same way**: a section with the model, its state and
  time, Stop (and Esc), its proofs with the checked ones first, then the facts for Sledgehammer
  as an offer of their own. Their proofs show with the caret at the end of the keyword: the
  prover's command for the line is the padded `sorry`, which ended before the keyword did, so
  the rest of the keyword had also shown as stray text after the cell. A Lean proposal in a
  tactic block loses a leading `by` (the host's `prompts.fit_candidate`): live, every proposal of
  `claude[haiku]` for a gap after `:= by` began with `by` and failed to parse.
- **Nothing stale or alarming on the way.** Until the prover has seen the tool's line, the
  section says it is waiting or checking; once a proof replaced the tool, the tool's old output
  is not shown. While a tool is the last step of a proof, the lemma after it (Isabelle's "Bad
  context for command") reads as waiting for that proof, not as failing. A tool started from the
  opened insert palette folds the palette back to its row, so its findings come where the goal
  was. Symbol chips on the insert bar set their glyphs at 19 px on squarish chips.
- **Evidence.** `snapshot_tools.rs` draws every panel state from recorded host messages (dark
  and light; beside, under and 800 px) and drives Alt+Enter, Alt+Down, a click on Insert, Esc
  and more time through the widgets. `live_tools.rs` drives each flow through the window against
  CyPhyAssure Isabelle2025 on neo (a host of its own): sledgehammer running 1.8 s after the last
  key and its first proof at 2.4 s, inserted and checked 1.4 s after Alt+Enter, undone in one
  step; from the insert palette with Alt+Down; solve_direct, try0, nitpick and quickcheck
  counterexamples in about 1 s; nothing found, more time, Esc; claude asked 0.5 s after
  Alt+Down, Alt+Enter, and a typed `claude[haiku]` line's checked proof inserted. And against
  Lean 4.19.0: `exact?`, `rw?`, `simp?`, `apply?` suggested 0.5 s after typing and checked
  0.6 s after Alt+Enter; `exact?` failing, then `apply?` run with Alt+Enter; omega's failure;
  decide closing its goal; a `claude[haiku]` line.

## Not yet implemented

- **Layout maths in the line being edited** (matrices, aligned displays) shows as source there; it renders in the rest of the block and beneath the line.
- **A Codex conversation ID** is taken from any `CODEX_*_ID` variable. Codex's
  real variable name is unverified.
