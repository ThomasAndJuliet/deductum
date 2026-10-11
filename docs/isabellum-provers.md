# Isabellum with Isabelle, Lean and Rocq backends

Status: design, 2026-10-11, on branch `multi-prover`
(`~/Documents/cosmic-term-multi-prover`). No production code has changed for
it yet. The branch starts from the Isabellum branch `worktree-2026-10-10-isabelle`
at `4b6b42c3b`. Cerea's rendering split (`shared-prose`, `9a556ee36`) is merged
into it, and `prose/` has been pulled up to Cerea's `9f8c181d`.
`cargo check --bin isabellum` passes on the merged tree.

This document extends [isabellum-decisions.md](isabellum-decisions.md) and
uses its labels. **Specified** means the human decided it. **Chosen** means
the implementing agent decided it, under the standing authorisation. Behaviour
from C1–C15 is unchanged unless a section here says otherwise. Coq is called
**Rocq** below, its name since Rocq 9.0. The design accepts both Rocq 9 and
Coq 8.x projects.

## Contents

1. [What the human asked for](#1-what-the-human-asked-for-c16)
2. [Terms](#2-terms)
3. [Components](#3-components)
4. [The host: one core, three drivers](#4-the-host-one-core-three-drivers)
5. [Capabilities: what each backend offers](#5-capabilities-what-each-backend-offers)
6. [Projects and sessions across backends](#6-projects-and-sessions-across-backends)
7. [The app: profiles and capabilities](#7-the-app-profiles-and-capabilities)
8. [The literate view per backend, on prose-render](#8-the-literate-view-per-backend-on-prose-render)
9. [Proof state, tools and LLM help per backend](#9-proof-state-tools-and-llm-help-per-backend)
10. [Agents (MCP)](#10-agents-mcp)
11. [Order of work and evidence](#11-order-of-work-and-evidence)
12. [To verify before relying on it](#12-to-verify-before-relying-on-it)
13. [Decisions from the human](#13-decisions-from-the-human-c17-2026-10-11)

## 1. What the human asked for (C16)

| Id | Decision |
|---|---|
| C16a | A large extension, built in a new worktree, with **separate Isabelle, Lean and Coq/Rocq backends**. |
| C16b | The UI **exposes the features that are relevant to the current backend**. |
| C16c | The UI **manages projects and sessions across the backends**. |
| C16d | The design includes the recent Isabellum changes and Cerea's worktree that **separated out the rendering engine** (`prose/prose-render`, `prose/prose-spell`). |

C1–C15 remain the specification. Every backend keeps shared live sessions with
agents (C2), Stop, Pause and Resume (C3, C7), host-owned buffers (C11),
per-author undo (C5), grants (C6, C9), autosave (C13) and both views (C14).
Where a prover cannot do something as Isabelle does, the design says what
happens instead (§5). It never fakes the feature.

## 2. Terms

The code says *theory* throughout. From here on, prover-neutral code uses the
terms in this table.

| Term | Meaning | Isabelle | Lean | Rocq |
|---|---|---|---|---|
| **Backend** | One prover family and its driver | `isabelle` | `lean` | `rocq` |
| **Toolchain** | An installed prover version on a machine | a runtime, e.g. CyPhyAssure Isabelle2025 | an elan toolchain, e.g. `leanprover/lean4:v4.32.1` | a `coq-lsp` binary and its Rocq, e.g. coq-lsp 0.2.5 + Rocq 9.1 |
| **Project** | How a folder's files are built together | ROOT sessions, base logic (heap), session dirs | a Lake package (`lakefile.{toml,lean}`, `lean-toolchain`) | `_RocqProject`/`_CoqProject`, or dune `(rocq.theory …)`/`(coq.theory …)` |
| **Document** | One source file that a session holds | `.thy` | `.lean` | `.v`, `.mv` (Markdown with Rocq blocks) |
| **Session** | One live prover on one project, owned by the host (C2) | one PIDE session | one `lake serve` | one `coq-lsp` |

## 3. Components

```text
Isabellum app (Rust, libcosmic)  ──┐                                  ┌─ drivers/isabelle.py ── JSON lines ──> isabelle isabellum_backend
agent's MCP adapter (Python)     ──┼── JSON lines (unchanged) ──> host core ┤                                         (Scala, Isabelle2025 PIDE)
isabellum command, Terminal Agents ┘                     (sessions,   ├─ drivers/lean.py ─┐
                                                          buffers, log,│                   ├─ drivers/lsp.py ── LSP ──> lake serve (Lean 4)
                                                          grants, ...) └─ drivers/rocq.py ─┘                   └─ LSP ──> coq-lsp (Rocq 9 / Coq 8.x)

prose/prose-render  (subtree from Cerea): Markdown → Doc → RichText, LaTeX, diagrams, code colours
prose/prose-spell   (subtree from Cerea): Harper; prose in every backend's literate blocks
```

| Component | Path (planned) | Role |
|---|---|---|
| Host core | `isabellum/host/isabellum_host.py` | As now: sessions, buffers, edit log, OT, undo, grants, notices, notes, autosave, disk watch, independent sessions, LLM plumbing. It no longer knows any prover. |
| Driver interface | `isabellum/host/drivers/__init__.py` | The `Driver` base class and the registry of backends (§4). |
| Isabelle driver | `isabellum/host/drivers/isabelle.py` | The current Isabelle-specific host code, moved here: runtimes, component registration, ROOT and logic suggestion, symbol table, keyword kinds, the backend process. |
| LSP client | `isabellum/host/drivers/lsp.py` | JSON-RPC framing, UTF-16 positions, document versions, request cancellation. Standard library only, like the rest of the host. |
| Lean driver | `isabellum/host/drivers/lean.py` | `lake serve`, goals (`$/lean/plainGoal`, RPC interactive goals), `$/lean/fileProgress`, "Try this" suggestions, scratch-document checking, Lake actions. |
| Rocq driver | `isabellum/host/drivers/rocq.py` | `coq-lsp`, `proof/goals`, `coq/getDocument` sentence spans, `$/coq/fileProgress`, Petanque for checking without editing, `coq/saveVo`. |
| Backend sources | `isabellum/backend/Isabelle2025/` | Unchanged. Lean and Rocq need no server-side code for the first version (§12 lists an optional Lean REPL). |
| App | `src/bin/isabellum/` | The generic app, plus `prover/{isabelle,lean,rocq}/` profiles (§7). |
| Renderer | `prose/prose-render` | Shared with Cerea and Terminal Agents. One addition is needed: prover code colours (§8.5). |

**Chosen: Lean and Rocq drivers live inside the host process, not as separate
backend processes.** Isabelle needs a JVM process, because PIDE's API is
Scala. `lake serve` and `coq-lsp` are already separate processes that speak
LSP. A further process between them and the host would add a protocol and a
failure point, and buy nothing. A driver is a Python object that owns its
prover process and turns its output into the host's neutral events.

**Chosen: one LSP client per session, and that client is the host.** LSP
servers serve one client. The host is that single client and keeps the
multi-client semantics, exactly as with PIDE (C2, C11). Agents and app windows
never talk to `lake serve` or `coq-lsp` directly.

## 4. The host: one core, three drivers

### 4.1 Driver interface

The core calls a driver with neutral operations and receives neutral events.
These are the operations the Scala backend already serves. Offsets are code
points of LF-normalised text, as now. LSP drivers convert offsets to and from
UTF-16 `(line, character)` positions in `lsp.py`, so no other code sees LSP
positions.

```python
class Driver:
    backend: str                      # "isabelle" | "lean" | "rocq"

    # discovery (no session needed)
    @classmethod
    def toolchains(cls, config) -> list[Toolchain]: ...
    @classmethod
    def detect(cls, folder) -> Detection | None: ...          # evidence, project suggestion
    @classmethod
    def documents(cls, folder) -> list[str]: ...              # this backend's files

    # lifecycle
    def start(self, session, toolchain, project) -> None: ... # emits phase/build/ready
    def stop(self) -> None: ...
    def pause(self) -> None: ...                              # see §5, "Pause"
    def resume(self) -> None: ...
    def interrupt(self) -> None: ...
    def capabilities(self) -> dict: ...                       # §5, sent to clients

    # documents (rev = host revision; becomes the LSP version)
    def open(self, path, text, rev): ...
    def edit(self, path, edits, rev): ...
    def close(self, path): ...
    def disk_changed(self, paths): ...

    # queries
    def state(self, path, offset, margin) -> State: ...       # §9.1 goal model
    def info(self, path, offset) -> Info: ...                 # hover + definition
    def run_tool(self, path, offset, tool, args) -> QueryId   # async, emits "query" events
    def cancel_query(self, qid): ...
    def check_candidate(self, path, span, text) -> Check: ... # LLM proofs, §9.4
    def run_command(self, path, offset, text) -> Messages: ...# diagnostics without editing
    def search(self, path, offset, query) -> QueryId: ...
    def project_action(self, name, args) -> QueryId: ...      # build, cache get, ...

    # events → core: phase, build, log, node{commands, messages, colours,
    #   status, revision}, query{…}, stale{paths}
```

The **`node` event keeps its shape**: command spans with status, messages with
ranges, colour spans, and the revision the report covers. The app's "waiting,
not stale" logic (decisions, "Waiting, not stale") then works for every
backend. LSP drivers fill `revision` from the `version` of
`publishDiagnostics` and the file-progress notifications.

### 4.2 What moves out of the core

The coupling survey of the host found that about a fifth of
`isabellum_host.py` is Isabelle-specific. All of it moves into
`drivers/isabelle.py`, or becomes a driver call:

| Now in the core | Becomes |
|---|---|
| `find_runtimes`, `runtime_identifier`, `ensure_component`, `symbol_table` | `IsabelleDriver.toolchains` / `start` |
| `parse_root`, `suggest_logic`, `known_sessions` | `IsabelleDriver.detect` |
| `list_theories`, `list_dir` filtering `.thy`/`ROOT` | `Driver.documents`, and the project files from `detect` |
| `syntax_kinds`, use in `proof_range`, `_fill`, `human_view` | `Driver.structure(path)`: declaration and proof ranges (§9.2) |
| `KEYWORD_PATTERN`, `prover_text()` (`sorry` padding) | `Driver.placeholder(line)` (§9.4) |
| `TOOLS` tuple, `cancel_tools` | `capabilities.tools[]`, `Driver.cancel_query` |
| `FILL_PROMPT`, `FACTS_PROMPT`, `existing_facts` | `prompts/{isabelle,lean,rocq}.py` |
| `prover_options` allow-list (Sledgehammer) | tool settings schema in `capabilities.tools[].settings` |

The core keeps everything listed in the survey as neutral: buffers, the log,
OT, undo and revert, grants, notices, notes, autosave and disk watch,
independent sessions, lifecycle and epochs, activity sampling, path
translation, `find_sessions`, the persistent LLM helpers and external agents.

### 4.3 Protocol changes

The app–host protocol stays JSON lines and keeps every current operation. The
changes are additive:

- `hello` returns `backends`: each backend's available toolchains on that
  machine, as `{backend, toolchains[{id, label, version, path, ok, problem}]}`.
  This replaces the flat `runtimes`, which is still sent for older apps.
- `suggest {folder}` returns `detections[]`, one per backend that recognises
  the folder: `{backend, evidence[], project{…}, toolchain, documents, note}`.
- `session_create` takes `backend`, `toolchain` and `project` (a backend-specific
  object). Isabelle's `logic`, `dirs` and `include_sessions` move inside
  `project`. They are still accepted at the top level for compatibility.
- `session` summaries carry `backend`, `toolchain{id,label}`, `project` and
  `capabilities` (§5).
- `query {operation, args}` becomes `tool {tool, args}` for typed proof tools,
  `search {query}` and `command {text}`. The old operation names stay as
  aliases for Isabelle.
- `state` replies with the neutral goal model (§9.1). Isabelle's
  `#isabellum:KIND` message parts are parsed in the Isabelle driver instead of
  in `goals.rs`.
- New: `project_action {name, args}` and its `query` events for builds.

Version 2 of the protocol is announced in `hello`. An older app talking to a
new host sees only Isabelle sessions.

## 5. Capabilities: what each backend offers

**Chosen: the UI follows capabilities, not backend names.** A session's
`capabilities` object says what its driver can do. The app shows a panel,
setting, button or palette entry only when the capability is present. The app
never contains `if backend == "lean"` for features. Backend names choose only
the profile (§7), which holds lexical knowledge such as keywords and comment
syntax.

```json
{
  "checking": "continuous",
  "pause": "perspective" | "close_documents" | "on_request",
  "interrupt": true,
  "goals": {"structured": true, "term_goal": false, "facts": true, "shelved": false},
  "hover": true, "definition": true, "spans": "prover" | "lexical",
  "tools": [{"name": "sledgehammer", "typed": "sledgehammer", "kind": "prove",
             "stop": true, "settings": {…schema…}}, …],
  "counterexamples": ["quickcheck", "nitpick"],
  "search": {"label": "Find theorems", "syntax": "isabelle-find"},
  "command": ["value", "term", "thm", "typ", "find_consts", …],
  "check_candidates": "query" | "petanque" | "scratch",
  "project_actions": [{"name": "build", "label": "Build heap"}, …],
  "semantic_colours": true,
  "documents": [".thy"], "project_files": ["ROOT"]
}
```

### 5.1 The features of each backend

| Feature | Isabelle | Lean 4 | Rocq |
|---|---|---|---|
| Checking | Continuous; every opened theory is required (as now). | Continuous per open file. Each open file is a Lean worker process, so the host opens files with the prover only while some client views or checks them (§6.5). | Continuous (coq-lsp's default mode). |
| Command spans | PIDE commands (`spans: prover`). | None over LSP. A lexical split on top-level commands, checked against `textDocument/documentSymbol` and `foldingRange` (`spans: lexical`). | `coq/getDocument`: one span per sentence (`spans: prover`). |
| Status per span | PIDE status. | `$/lean/fileProgress` ranges and diagnostics. | `$/coq/fileProgress` and diagnostics. |
| Goals at any position | `isabellum_goals`: facts, then per subgoal its fixes, assumptions and conclusion. | `$/lean/plainGoal` for text; `Lean.Widget.getInteractiveGoals` over `$/lean/rpc/call` for hypotheses, case tags and hover on subterms. `$/lean/plainTermGoal` gives the expected type in term mode. | `proof/goals` with `pp_format`: goals with hypotheses, plus shelved, given-up and unfocused goals (the bullet stack). |
| Hover, definition | `info` (as now). | `textDocument/hover`, `definition`. | `textDocument/hover`; `definition` where coq-lsp provides it. |
| Typed proof tools (offered with Insert) | `sledgehammer`, `try0`, `try`, `solve_direct`, `nitpick`, `quickcheck`, `nunchaku` (as now). | `exact?`, `apply?`, `rw?`, `simp?`, `aesop?`, `hint`, `grind`, and `hammer` when the project depends on LeanHammer. Their "Try this" messages and code actions become findings with Insert. | `hammer` (CoqHammer, when loaded), `sauto`/`best` and the like; their "Replace … with" output becomes findings with Insert. |
| Counterexamples | Quickcheck, Nitpick. | `plausible` (when the project has it). | QuickChick (when the project has it). |
| Search | Find Theorems. | `exact?`/`apply?` at the caret; Loogle, public or local, chosen in settings (C17e). | `Search` at the caret. |
| Commands without editing (`command`) | `isabellum_command` (as now). | A scratch document (§9.4): `#check`, `#eval`, `#print`, `#reduce`. | Petanque at the caret's state, or a scratch document: `Check`, `Print`, `About`, `Compute`, `Search`. |
| Checking LLM candidates | `isabellum_command` (`check_candidates: query`). | Scratch document (`scratch`). | Petanque `get_state_at_pos` then `run` (`petanque`). |
| Pause | Empty perspective plus cancel (as now). | Close every document in the server, keeping the server; edits keep going to the host's buffers. Resume reopens and rechecks from each file's header (`close_documents`). | `check_only_on_request` through `workspace/didChangeConfiguration` (`on_request`). |
| Interrupt | `cancel_exec` (as now). | No "stop elaborating" request exists. Interrupt closes and reopens the files that are being checked. | `on_request` mode, then back to continuous. |
| Stop a typed tool | Cancels only the tool's executions (as now). | Deleting or changing the line, as now. There is no separate Stop button (`stop: false`); the tool section says so. | Same as Lean. |
| Colours | PIDE markup. | Semantic tokens plus the profile's lexer. | The profile's lexer. |
| Project actions | Build the heap for the chosen logic. | `lake build`; `lake exe cache get` when Mathlib is a dependency; Rebuild imports (`lake setup-file`, then restart the file) when `$/lean/staleDependency` arrives. `lake update` only with a warning. | Build with the project's system (dune, or `coq_makefile` and make); `coq/saveVo` after a file checks, so dependent files see it. |
| Prose | `text ‹…›`, `section ‹…›` and the like (as now). | Module docs `/-! … -/` and docstrings `/-- … -/`, which are already Markdown with `$…$`. | coqdoc comments `(** … *)` in `.v`; Markdown outside the code blocks in `.mv`. |
| Symbols | `\<forall>` escapes, decoded for display with an offset map (as now). | Unicode in the source. `\` completion inserts the character (Lean's abbreviation table). | Unicode or ASCII notations in the source. `\` completion as in Lean. Source shown as written, as CoqIDE and VsRocq show it (C17d). |

### 5.2 How the UI uses the capabilities

- **Header centre.** The prover's state and Interrupt, as now. The label names
  the backend and toolchain ("Lean v4.32.1 · checking 2 files"). Interrupt
  shows only when `interrupt` is true.
- **Proof tools.** The tool section beside the goal lists the tools in
  `tools[]`. Each tool's settings section is built from its `settings`
  schema. Sledgehammer's timeout and provers become one instance of this, not
  special code. Stop appears only for tools with `stop: true`.
- **The insert palette and `+ Lemma`.** Their entries come from the profile
  (§7.2), filtered by capabilities. For example, `sledgehammer` is offered only
  when that tool is present, and `hammer` in Lean only when the project has it.
- **Right sidebar.** Activity, Agents, History, Symbols as now. **Counterexamples**
  (Quickcheck now) appears when `counterexamples` is not empty. **Search** appears
  when `search` is present, with its label and a syntax hint from it.
- **Left sidebar.** Sessions, Outline, Theorems, Problems. Theorems lists
  declarations from the driver's `structure` (§9.2), so it works for every
  backend. A **Project** tab lists `project_actions` with their last output.
- **The tool keyword lines** (`claude`, `codex`, `@name`) work in every backend
  (§9.4).
- **Copy.** Copies source. Only Isabelle has escapes to convert.

## 6. Projects and sessions across backends

### 6.1 Toolchains per machine

`~/.config/isabellum/host.json` gets one list per backend. The existing
`runtimes` list is read as `isabelle`:

```json
{
  "isabelle": [{"path": "~/Documents/Software/Isabelle2025-CyPhyAssure"}],
  "lean":     {"elan": "~/.elan/bin/elan"},
  "rocq":     [{"coq_lsp": "~/.opam/rocq-9.1/bin/coq-lsp", "label": "Rocq 9.1"}],
  "memory_budget_gb": 24
}
```

- **Isabelle.** As now: a runtime is offered only if a matching
  `backend/<ISABELLE_IDENTIFIER>/` exists.
- **Lean.** elan is the toolchain manager. The project's `lean-toolchain` file
  decides the version, and `lake serve` is started through elan in the
  project's root. When that toolchain is not installed, the host says so and
  asks before letting elan download it, because that can be gigabytes. Found
  so far: v4.19.0 on neo and au-laptop, and v4.32.1 on au-laptop.
- **Rocq.** A toolchain is a `coq-lsp` binary and the Rocq it was built with.
  The host finds them in opam switches (named switches, and a project-local
  `_opam`) and in the configured list. Neither neo nor au-laptop has Rocq yet.
  It is installed on au-laptop with opam (C17c).

### 6.2 Detection

`suggest {folder}` asks every driver's `detect`. Each driver reports the
evidence it found:

| Backend | Evidence | Project suggestion |
|---|---|---|
| Isabelle | `ROOT`/`ROOTS`, `.thy` files | logic, session dirs, included sessions (as now) |
| Lean | `lakefile.toml`/`lakefile.lean`, `lean-toolchain`, `.lean` files; Mathlib in `lake-manifest.json` | Lake root (the nearest ancestor with a lakefile), toolchain, whether a Mathlib cache is needed |
| Rocq | `_RocqProject`/`_CoqProject`, `dune-project` with `(using rocq …)`/`(using coq …)`, `.v`/`.mv` files | project file, logical path mappings (`-R`/`-Q`), opam switch |

A folder may match several backends. A repository can hold Isabelle theories
and a Lean package side by side, for instance.

### 6.3 Session identity

A session is identified by `(machine, folder, backend, toolchain, project)`
plus its epoch. The current lookup key `(folder, runtime, logic)` is the
Isabelle case of this. `meta.json` gains `backend`, `toolchain` and
`project`. A record without `backend` is read as Isabelle, with its `logic`,
`dirs` and `include_sessions` moved into `project`. No session state is lost
in the migration.

**Chosen: one session per backend per folder.** Two backends on the same
folder are two sessions. Each claims only its own documents (`documents` in
§5). A `.lean` file in an Isabelle session's folder is not that session's
document. Opening it in the app offers the Lean session for the folder, or
offers to start one.

### 6.4 The Sessions sidebar and the new-session flow

- **Sidebar.** Sessions are grouped by machine, then by folder. Each row shows
  a small backend mark (Isabelle, Lean, Rocq), the name, the toolchain label,
  the lifecycle badge and attached agents, as now. Independent sessions stay
  under the session they were forked from (C8). The open session expands to
  its documents. Its project files (ROOT, lakefile, `_RocqProject`) are listed
  first and open in the source view.
- **Memory.** Each row shows its prover's memory, and each machine shows the
  total against `memory_budget_gb`.
- **New session.** Pick the machine, then browse to a folder. Detection then
  shows one card per backend that recognised it, with the evidence ("Lake
  package, Mathlib, toolchain v4.19.0"). Picking a card shows that backend's
  project form:
  - Isabelle: runtime (now shown; today the host picks the first), base logic,
    session dirs.
  - Lean: Lake root and toolchain, read-only and taken from the files, with a
    note if the toolchain or the Mathlib cache is missing, and the action that
    fixes it.
  - Rocq: toolchain (switch), project file, and the logical paths read from it.
  "Start session" is the same as now. A folder with no evidence offers all
  three backends.
- **`isabellum .`** works as now. When the folder's existing sessions or its
  detection are ambiguous, the app opens the new-session cards instead of
  guessing (C3: "multiple eligible sessions require selection").
- **Terminal Agents' "Open in Isabellum"** (C10) works for every backend,
  because `find_sessions` is in the core.

### 6.5 Resources

Isabelle heaps take several gigabytes. Each Lean file worker takes from
hundreds of megabytes to a few gigabytes with Mathlib. coq-lsp takes less. The
host already refuses provers beyond a per-machine limit (C8). That limit
becomes a **memory budget across all backends**, using the activity sampler's
RSS figures.

For Lean, **the host opens a document with the server only while some client
views it, an agent queries it, or it is checked explicitly**. Closing it frees
its worker. The host keeps the buffer and log either way, so this affects only
checking.

### 6.6 Independent sessions per backend

`fork_session` (a git worktree on `isabellum/<id>`) stays in the core. Two
backend details:

- **Lean.** A new worktree has no `.lake`. The fork links the parent's
  `.lake/packages` into the worktree, since dependencies are only read, and
  lets the worktree build its own `.lake/build`. Otherwise every fork of a
  Mathlib project would fetch the cache again.
- **Rocq.** A fork rebuilds `.vo` files in the worktree with the project's
  build system before starting coq-lsp. The new-session card shows that build
  as progress.

## 7. The app: profiles and capabilities

### 7.1 Two sources of backend knowledge

- **The profile**, compiled into the app, one per backend. It holds what the
  app must know before the prover answers, or without the prover at all: file
  types, the lexical block split, comment and prose syntax, the
  prose-to-Markdown converter, the symbol codec and input table, the lexer for
  colours, indentation, goal-construct detection, completion templates, palette
  entries, and block templates.
- **The capabilities**, from the host, per session (§5). They hold what this
  session's driver and project can do.

A document's profile is that of the session it belongs to. Two sessions with
different backends can be open in one window at the same time.

```rust
pub trait Profile: Send + Sync {
    fn backend(&self) -> Backend;
    fn documents(&self) -> &[&str];                         // extensions
    fn blocks(&self, text: &str, spans: Option<&[Command]>) -> Vec<Block>;
    fn prose(&self, block_source: &str) -> Prose;           // Markdown + source map (§8)
    fn codec(&self) -> &dyn Codec;                          // display ⇄ source offsets
    fn input(&self) -> &InputTable;                         // `\` completion entries
    fn lex(&self, line: &str, state: &mut LexState) -> Vec<(Range<usize>, Class)>;
    fn indent(&self, before: &str) -> Indent;
    fn construct_at(&self, text: &str, offset: usize) -> Option<Range<usize>>;
    fn completions(&self, goal: &Goal) -> Vec<Template>;
    fn palette(&self, context: Context, caps: &Capabilities) -> Vec<PaletteEntry>;
    fn block_templates(&self) -> BlockTemplates;            // + Text, + Section, + Lemma
    fn keyword_line(&self, line: &str) -> Option<KeywordLine>; // claude/codex/@name
    fn placeholder(&self) -> &str;                          // sorry / admit.
    fn present_goal(&self, goal: &Goal) -> GoalPresentation;// labels and layout (§9.1)
}
```

### 7.2 Where today's code goes

The survey estimates that 40–45% of the app's non-test code is
Isabelle-specific. It moves as follows, and every move is checked by the
existing tests before the next backend is added (§11):

| Today | Moves to |
|---|---|
| `theory.rs`: `DOCUMENT`, `THEORY_LEVEL`, `PROOF` lists, lexical scanner | `prover/isabelle/blocks.rs`; `Block`, `Command`, `Status` stay in `theory.rs` |
| `literate.rs` (Isabelle text to Markdown) | `prover/isabelle/literate.rs` |
| `symbols.rs`: the Isabelle table and scripts | `prover/isabelle/symbols.rs` |
| `symbols.rs`: the `Decoded` offset map and fuzzy search | `codec.rs`, `input.rs` (generic) |
| `goals.rs` (`#isabellum:` parts) | the Isabelle **host** driver; the app gets the neutral goal model |
| `editor.rs`: `TOOLS`, `parse_goal`, `goal_completions`, `next_indent`, `construct_at`, `prose_markup`, `keyword_tokens`, `paired_brackets`, `Sem` classes | `prover/isabelle/{isar.rs, prose.rs, colours.rs}`; the replica, undo grouping and block editors stay |
| `view.rs`: `PROOF_TEMPLATES`, `PROSE_TEMPLATES`, `subgoal_card` labels, the Sledgehammer panel, `sorry`/`oops` checks, `.thy` listing | profile palette and `present_goal`; a generic tool panel built from `tools[]`; `Profile::documents` |
| `main.rs`: `InsertBlock` templates, operation routing, keyword-line detection, `InsertUsing`, `InsertOutput` | `Profile::{block_templates, keyword_line}`; the `facts_line` of §9.4; generic `tool` routing |
| `gap.rs`, `vim.rs`, `ot.rs`, `link.rs`, `cli.rs` | unchanged (already generic) |

### 7.3 The profiles for Lean and Rocq

- **Lean.** Blocks start at top-level commands in column 0 (`theorem`,
  `lemma`, `def`, `instance`, `example`, `structure`, `inductive`, `class`,
  `abbrev`, `namespace`, `section`, `end`, `open`, `variable`, `set_option`,
  attributes and modifiers). A docstring `/-- … -/` belongs to the
  declaration after it. A module doc `/-! … -/` is a prose block. The lexical
  split is confirmed by document symbols when they arrive.
  - Constructs for the tint: `have … := by` with its indented block, focus
    dots `·`, `case`, `next`, `calc` steps.
  - Completions from the goal's head: `intro` for ∀ and →, `constructor` or
    `refine ⟨?_, ?_⟩` for ∧ and ↔, `use`/`refine ⟨_, ?_⟩` for ∃, `cases`/`rcases`
    for hypotheses of those shapes.
  - Indentation follows tactic blocks (`by` at a line's end, `·`).
- **Rocq.** Blocks group coq-lsp's sentences. A sentence that states a
  theorem or definition starts a block, and its `Proof.` … `Qed.`/`Defined.`/
  `Admitted.`/`Abort.` belong to it. Without spans, the profile splits on `.`
  followed by whitespace, outside comments and strings.
  - Constructs: bullets `-` `+` `*`, braces `{ }`, `assert (…).` with its
    proof.
  - Completions: `intros`, `split`, `exists`, `destruct`, `induction`, `lia`,
    `auto`.
  - Indentation follows bullets.
  - `.mv` files: blocks are Markdown paragraphs and ```` ```rocq ```` (or
    ```` ```coq ````) fences. coq-lsp checks only the fences.

## 8. The literate view per backend, on prose-render

The literate view is now built on Cerea's `prose-render`. It parses Markdown
into a `Doc` and draws it with `RichText`, including LaTeX through RaTeX,
Mermaid, chartgen charts, images and code colours. Today the view calls only
`markdown::parse` and `rich::rich_text`. It builds the `Typography` in
`view.rs::typography`.

### 8.1 One converter per profile, with a source map

`Profile::prose` turns a prose block's source into Markdown **and a map from
Markdown offsets back to source offsets**. Clicking rendered text opens the
paragraph with the caret where it landed (decisions, "Prose a paragraph at a
time"), and spelling marks land on source text. The map has the same form as
prose-render's `normalize_math` offset map (`markdown.rs`, `source`/`range`).

- **Isabelle**: `literate::to_markdown`, as now. prose-render now reads
  `\(…\)`, `\[…\]` and `align`-like environments itself. The Isabelle converter
  therefore no longer needs to rewrite those, and they pass through.
- **Lean**: strip the `/-!`, `/--` and `-/` delimiters and the common indent.
  The rest is Markdown already, with `$…$` and `$$…$$` as doc-gen4 reads them.
  Code spans in backticks are Lean.
- **Rocq `.v`**: coqdoc to Markdown. Headings `(** * T *)` and `(** ** T *)`,
  `[code]` to inline code, `[[ … ]]` to a code block, `$…$` maths, `_emph_`,
  `-` lists, `<<verbatim>>`, and `%…%`/`#…#` dropped with a visible marker.
- **Rocq `.mv`**: the identity, apart from the code fences, which are formal
  blocks.

### 8.2 Editing prose

The paragraph-at-a-time editor works unchanged for every backend. It needs
two things from the profile: the codec, and how markup shows while edited.
The existing "markup reads as it renders unless the caret is in it" rules are
Isabelle's. Lean docstrings and Rocq `.mv` files are plain Markdown, so their
rules are the Markdown ones that Cerea's editor already implements (still open, §13).

### 8.3 Spelling

`prose-spell` is now in the tree, but Isabellum does not use it yet. It
applies to prose blocks of every backend through `check_markdown` on the
converter's Markdown, with marks mapped back through the source map.

### 8.4 Formal text

Formal blocks are read-only editors coloured by the profile's lexer and,
where the backend has them, by prover colours (PIDE markup, Lean semantic
tokens). This is as now.

### 8.5 One change to prose-render

`prose-render::highlight` colours fenced code with syntect's default
grammars. Those have no Isabelle, Lean or Rocq grammar. Code in Lean
docstrings (```` ```lean ````), `.mv` prose and Isabelle text would then
stay uncoloured.

**Chosen:** add a registration hook to `highlight.rs`,
`register(lang, fn(&str) -> Tokens)`, so that an app supplies its own lexer
for a language. Isabellum registers each profile's `lex`. The change is made
in Cerea's `prose/` and pulled into this branch by the subtree workflow (see
Cerea's `docs/architecture.md`). Cerea and Terminal Agents are unaffected
unless they register something.

## 9. Proof state, tools and LLM help per backend

### 9.1 One goal model

```json
{
  "goals": [{"tag": "succ", "hyps": [{"names": ["n"], "type": "ℕ", "value": null, "kind": "fix"}],
             "target": "P (n + 1)", "tips": […]}],
  "facts": [],                        // Isabelle chained facts
  "stack": {"unfocused": [[1, 0]], "shelved": 0, "given_up": 0},  // Rocq
  "term_goal": null,                  // Lean expected type
  "text": "…",                        // the prover's own printout (the Text view)
  "messages": […]
}
```

- **Isabelle**: fixes become `hyps` with kind `fix`, assumptions become
  `assume`, and the conclusion is the `target`.
- **Lean**: each interactive hypothesis becomes `names` and `type`, with its
  value for `let`. The case tag becomes `tag`.
- **Rocq**: hypotheses come with their bodies, and the bullet stack goes in
  `stack`.

`Profile::present_goal` chooses the labels and layout:

- Isabelle shows `fix` / `assume` / `show`, as now.
- Lean shows `case tag`, then `x : T`, then `⊢ target`.
- Rocq shows the hypotheses above a rule and the goal below it, with the
  unfocused and shelved counts.

The Goals and Text toggle, "what the step changed", the Types switch (where
the prover can print with or without types) and goal placement beside the
text are generic. They stay as they are.

### 9.2 Structure

`Driver.structure(path)` returns declarations (name, kind, statement range,
proof range) for Theorems, `isabelle_human_view`'s lemma, proof grants (C6)
and LLM prompts.

- **Isabelle**: from keyword kinds (`thy_goal`, as now).
- **Lean**: from `documentSymbol`.
- **Rocq**: from sentence spans and their first words, or from the AST that
  `coq/getDocument` can return.

### 9.3 Typed proof tools

The mechanism stays as it is now. With the caret on a tool's text, its
findings replace the goal as they arrive, with Insert, elapsed time and
progress. It becomes generic through `tools[]`:

- **Isabelle**: queries and PIDE print functions, as now.
- **Lean**: the tools are ordinary tactics. The driver collects their
  "Try this:" messages and the matching code actions
  (`textDocument/codeAction`) as findings, each with its edit. Progress comes
  from `$/lean/fileProgress` over the tool's range.
- **Rocq**: tactic output. The driver parses CoqHammer's replacement message.

A tool that is still running keeps its proof shown as unfinished, not failed.
Isabelle already does this ("The `qed` that fails because a proof waits on
such a tool"). For Lean this is the error at the declaration's end. For Rocq
it is the error at `Qed.`.

### 9.4 `claude`, `codex` and `@name` lines

The feature stays as it is now. The prover sees the line as the profile's
placeholder, padded to the same length, so offsets do not change:

- **Isabelle**: `sorry`.
- **Lean**: `sorry`, which is valid both as a tactic and as a term.
- **Rocq**: `admit.`. A Rocq keyword line ends with a period, as Rocq tactics
  do (`codex.`, `claude[opus] use lia.`), so the shortest line, `codex.`,
  still has room for `admit.`.

Candidates are checked before they are offered, according to
`check_candidates`:

- `query` (Isabelle): `isabellum_command`, as now.
- `petanque` (Rocq): `petanque/get_state_at_pos` at the placeholder, then
  `petanque/run` with the candidate. It passes when the focused goal is
  closed. Nothing in the document changes.
- `scratch` (Lean): a second document, unknown to clients, opened in the same
  server. It holds the file up to the end of the declaration, with the
  candidate in place of the placeholder. It passes when there are no errors in
  the declaration. This re-elaborates the prefix, which is slow on long files.
  An optional faster path is a Lean REPL built for the toolchain (§12).

The prompts move to `prompts/<backend>.py`: Isar for Isabelle, Lean 4 tactic
style (with Mathlib's when the project uses it) for Lean, and Ltac for Rocq.
The suggested facts are written with the profile's `facts_line`:

- Isabelle: `using f₁ f₂` followed by `sledgehammer`, as now.
- Lean: `simp [f₁, f₂]`, or `hammer [f₁, f₂]` when LeanHammer is present.
- Rocq: `eauto using f₁, f₂.`.

The persistent helper, fresh requests and listening external agents are
unchanged.

## 10. Agents (MCP)

- **Neutral tool names.** Each `isabelle_*` tool gets a `proof_*` name
  (`proof_sessions`, `proof_open_session`, `proof_read`, `proof_edit`,
  `proof_check`, `proof_state`, `proof_human_view`, …).
- **The old names stay as aliases.** This keeps registered agents on neo and
  au-laptop, and the scripted MCP tests, working.
- **One tool per kind of backend feature.** The Isabelle-only tools become
  `proof_run_tool {tool, args}` (Sledgehammer, `exact?`, `hammer`, …),
  `proof_search {query}` and `proof_command {text}`. MCP clients do not
  reliably refresh their tool lists, so the tool list stays the same for every
  backend. A call names a tool and is checked against the session's
  capabilities. A refusal lists what the session offers.
- **Telling the agent the backend.** `proof_open_session` and `proof_sessions`
  return each session's backend, toolchain and capabilities.
  `proof_capabilities` returns them on their own. Tool descriptions say which
  backend has which tools.
- **Opening sessions.** `proof_open_session` takes `backend` and `project`, or
  uses detection when the folder is unambiguous.
- **Unchanged for every backend:** grants, access requests, notices, notes,
  `listen`/`propose`, independent sessions and merge preview.
- **The MCP server's name** follows the new app name (C17a).

## 11. Order of work and evidence

Each step keeps every existing test passing before the next starts:

- 43+ Rust tests (`cargo test --bin isabellum`);
- `test_ot.py`, `test_keywords.py`, `test_undo.py`;
- `integration_host.py`'s 28 checks against CyPhyAssure Isabelle2025 on au-laptop;
- the `live_*` tests.

| Step | Work | Evidence that it is done |
|---|---|---|
| 0 | Worktree, merge `shared-prose`, pull Cerea's `prose/` | Done: `cargo check --bin isabellum` passes. |
| 1 | Host: `Driver` interface; Isabelle code moved to `drivers/isabelle.py`; protocol v2 fields; `meta.json` migration | Every Isabelle test above passes unchanged. A host restart keeps existing sessions on neo and au-laptop. |
| 2 | App: `Profile` with the Isabelle implementation; modules moved (§7.2); neutral goal model; capability-driven panels | Snapshot PNGs of the literate and source views, goal, tools and panels are pixel-identical before and after (`CT_SNAPSHOTS`). |
| 3 | `lsp.py` and the Lean driver; Lean profile; Lake detection and the new-session cards | `integration_lean.py` against v4.19.0 on neo: open, edit, goals at positions, a stale edit rebased, a typed `exact?` inserted, a `claude` line checked by scratch, Pause and Resume, Stop and a new epoch. Snapshots of a Lean demo. |
| 4 | Rocq toolchain on au-laptop (opam), then the Rocq driver and profile, `.v` and `.mv` | `integration_rocq.py`: the same checks, with Petanque candidate checks and `check_only_on_request` Pause. Snapshots of a coqdoc demo and a `.mv` demo. |
| 5 | Sessions across backends: grouped sidebar, memory budget, mixed folders, the Project tab and actions | A `live_sessions` test with an Isabelle and a Lean session on one folder, each claiming only its own documents; a memory refusal. |
| 6 | MCP: neutral names, `proof_run_tool`, aliases | The scripted MCP tests per backend; a real Claude session proving one lemma in Lean and one in Rocq after a grant. |
| 7 | `prose-render` highlight hook (in Cerea) and `prose-spell` in prose blocks | Highlighted ```` ```lean ```` in a docstring snapshot; spelling marks on a coqdoc paragraph. |

Lean comes before Rocq for two reasons. Lean is installed on both machines
already, and the human's main Lean project (C17f) runs on it. It is also the backend least like PIDE, with no spans and no
checking without editing, so the abstractions are tested against it early.

## 12. To verify before relying on it

- **Lean protocol.** `$/lean/plainGoal`, `$/lean/plainTermGoal`,
  `$/lean/fileProgress`, `$/lean/staleDependency`, RPC `getInteractiveGoals`
  and semantic tokens on v4.19.0 and v4.32.1, including the session-keepalive
  rules of RPC.
- **Lean "Try this".** Whether code actions carry edits for every tool
  listed, or whether some tools only print messages.
- **Lean Interrupt.** Whether closing a document reliably stops its worker's
  elaboration and frees its memory.
- **Lean REPL.** Whether `leanprover-community/repl` built per toolchain
  makes candidate checks fast enough to replace scratch documents (an
  optional capability, `check_candidates: "repl"`).
- **coq-lsp `check_only_on_request`.** Whether turning it on also stops a
  check that is already running. The protocol document lists the option but
  does not describe its behaviour.
- **Petanque.** Whether Petanque can run vernacular queries (`Search`,
  `Check`) at a state, or only tactics. If only tactics, `command` uses a
  scratch document as Lean does.
- **coq-lsp details.** `textDocument/definition` coverage, and how it finds a
  dune project's load paths.
- **coq-lsp versions.** The newest packaged release is 0.2.5, built for Rocq
  9.0 and 9.1. Builds for Coq 8.17–8.19 return goals in a different form
  (`reified_pp`), so the driver must read both.

## 13. Decisions from the human (C17, 2026-10-11)

| Id | Decision | Effect on the design |
|---|---|---|
| C17a | **A new name** for the app. | Pending the human's choice. Identifiers (binary, app ID, command, Flatpak, config and state directories, MCP server name) change together. The state and config directories are migrated, so existing sessions survive. |
| C17b | **Testing runs headlessly on neo.** | Every test uses the snapshot harness (`snapshot::Harness`) or the host tests, never a window. Provers that live on au-laptop (CyPhyAssure Isabelle2025, Rocq) are reached from neo over ssh, as `live_*` tests already do with `ISABELLUM_LIVE=d59500`. |
| C17c | **Rocq may be installed on au-laptop.** | Step 4 installs Rocq 9.1 and coq-lsp there with opam. |
| C17d | **Use the notation users of each tool are used to.** | Each backend shows source and goals the way its usual editor does. Isabelle: as Isabelle/jEdit (symbols decoded). Lean: as the VS Code infoview (`case tag`, `x : T`, `⊢ goal`). Rocq: as CoqIDE and VsRocq (hypotheses above `============================`, source as written). The Rocq ASCII-to-Unicode display option is dropped. |
| C17e | **Public Loogle is allowed, as a setting.** | A Lean search setting: Off, Local, or Public (loogle.lean-lang.org), default Public. The Search panel names the source it used. |
| C17f | **The main Lean project is the formalisation of the causal semantics and DSRV**: `DSRVLean` (Lake package `dsrv`, library `DSRV`, toolchain v4.19.0, no Mathlib). | It is the real test for Lean, as tock-Circus is for Isabelle. The current copy is au-laptop's `~/Documents/Research/Repositories/DSRVLean` (3716 lines, including `CausalSemantics`, `CausalCorrectness`, `CausalPaperSemantics`). neo's `~/Documents/DSRVLean` is an older 757-line copy. Neither is a git repository, so independent sessions need the plain-copy fork (C8's "later extension"). Tests run on a fresh copy and never touch either original. |

Still open: Cerea's inline editor for Markdown-native prose (default: keep
Isabellum's paragraph-at-a-time editing for all three backends), and a Rocq
project to test on (default: a small demo).
