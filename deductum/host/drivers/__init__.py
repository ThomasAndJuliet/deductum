"""Prover drivers for the Deductum session host (design: docs/deductum-provers.md §4).

A driver owns one session's prover and speaks to the host core in the **backend
protocol**: the JSON objects the Isabelle backend (``isabelle deductum_backend``,
``backend/Isabelle2025/src/backend.scala``) reads and writes as JSON lines. The
Isabelle driver passes them to that process; the Lean driver answers them itself
by talking LSP to ``lake serve``. The host core never knows which prover it has.

Offsets everywhere are code points of the LF-normalised text, as the host keeps
it. ``rev``/``seq`` is the host's revision of the document.

Operations the core sends (``send`` for none-reply ops, ``request`` for the rest)
-------------------------------------------------------------------------------
- ``open {file, text, rev}``; ``edit {file, rev, edits: [{start, end, text}]}``
  (edits are sequential: each applies to the text the previous one left);
  ``close {file}``; ``disk_changed {files: [path]}`` (files not open in the
  session changed on disk).
- ``checking {on: bool}``: Pause (off) and Resume (on) checking; the prover
  stays loaded.
- ``cancel`` → ``{canceled: n}``: interrupt everything running.
- ``cancel_tools`` → ``{}``: interrupt only proof tools typed in the text
  (Isabelle: sledgehammer, nitpick, ...).
- ``state {file, offset, margin}`` → ``{command: {start, end, name, status} |
  null, messages: [Message], outdated: bool}`` or ``{error}``: the command at
  the offset and its output. A message of kind ``state`` is the proof state as
  text.
- ``query {query, file, offset, operation, args}``: starts an asynchronous
  query; progress comes as ``query`` events (below). ``cancel_query {query}``.
  Operations:
    - ``deductum_goals`` (args ``[margin, types]``): the proof state at the
      offset in parts (see "Goal parts").
    - ``deductum_command`` (args ``[text]``): run ``text`` at the offset
      without editing the document (Isabelle: a diagnostic command or a proof
      in place of the command there; Lean: a tactic or command in a scratch
      copy). Errors come as messages of kind ``error``; ``finished`` with no
      error means it worked.
    - ``deductum_entity`` (args ``[name]``): what a name is (Isabelle only).
    - prover tools by name: ``sledgehammer``, ``find_theorems`` (Isabelle).
- ``syntax`` → ``{syntax: {kinds: {command_name: kind}}}``: keyword kinds, as
  Isabelle names them (``thy_goal...`` for statements that open a proof,
  ``thy_decl``, ``prf_*`` for proof commands, ``document_*``). Lean maps its
  commands onto these (``theorem``/``lemma``/``example`` → ``thy_goal``).
- ``symbols`` → ``{symbols: [...]}`` (Isabelle only; others reply ``[]``).
- ``info {file, offset}`` → ``{info: {tooltip: {start, end, items: [{kind,
  raw, text, spans}]} | null, definition: {file, line, column?, start, end,
  name, kind} | null}}``: hover and the definition.
- ``options {set: ["key=value"]}`` → ``{}``: prover options (Isabelle:
  ``sledgehammer_timeout``, ``sledgehammer_provers``).
- ``ping`` → ``{}``.

Events the driver emits (``self.session.on_backend_event(obj)``)
----------------------------------------------------------------
- ``ready {identifier, home}``: the prover is ready; set ``self.ready = True``
  first. ``identifier`` names the version (``Isabelle2025``,
  ``Lean (version 4.19.0, ...)``).
- ``phase {phase: "building"|"failed", message}``, ``build {text}``,
  ``log {text}``: progress while starting.
- ``node {file, theory, seq, status, header_errors, imports, commands,
  messages, colours}``: the prover's report on one document, covering
  revision ``seq``. ``status`` is ``{total, unprocessed, running, warned,
  failed, finished, canceled, consolidated, percentage}``. ``commands`` is
  ``[[start, end, name, status]]`` with status ``unprocessed|running|failed|
  canceled|warned|finished``; ``name`` is the command's keyword (``sorry``
  for a placeholder proof matters to the host). ``messages`` are Messages
  with ``start`` and ``end``. ``colours`` is ``[[start, end, class]]`` with
  Isabelle's colour class names (``keyword1``, ``keyword2``, ``keyword3``,
  ``free``, ``bound``, ``var``, ``skolem``, ``tfree``, ``tvar``, ``inner_quoted``,
  ``inner_comment``, ``comment1``, ``dynamic``, ``class_parameter``,
  ``antiquoted``, ``operator``, ``main``...).
- ``query {query, status: waiting|running|finished|failed|canceled, messages,
  start?, end?, command?, replace?: {start, end}, error?}``.

A **Message** is ``{kind, text, raw, spans: [[s, e, class]], tips: [[s, e,
text]], sendbacks: [...]}``: ``kind`` is ``error``, ``warning``, ``writeln``,
``information``, ``legacy`` or ``state``; ``text`` is what is shown (Isabelle
symbols decoded), ``raw`` the source form; ``spans`` colour ``text`` by code
point; ``tips`` are hover texts for ranges of it.

Goal parts (the ``deductum_goals`` query)
-----------------------------------------
Each part is one message whose ``raw`` (and ``text``) starts with a head line
``#deductum:KIND N``; the body follows on the next lines. Kinds: ``text`` (the
whole state as the prover prints it), ``fact`` (a chained fact), ``count``
(N = number of goals), ``nogoal`` (in a proof, but no goal), ``case`` (N's
case tag, Lean), ``param`` (a fixed variable of goal N; in Lean every
hypothesis, ``h : P n``), ``prem`` (an assumption of goal N), ``concl`` (goal
N's conclusion). Spans and tips count from the start of ``text``, head line
included. A final ``#deductum:done`` may end the list.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import os
import signal
from typing import Any, Dict, List, Optional


class Driver:
    """One session's prover. Subclasses set ``backend`` and implement ``start``
    and either a process speaking the backend protocol (``Process``) or
    ``send``/``request`` themselves."""

    backend = "?"
    label = "?"
    #: File extensions of this backend's documents, and its project files
    documents: tuple = ()
    project_files: tuple = ()

    def __init__(self, session: Any):
        self.session = session
        self.ready = False
        # The prover's process; its process group is sampled for the Activity view
        self.proc: Optional[asyncio.subprocess.Process] = None

    # ----- discovery, without a session

    @classmethod
    def toolchains(cls) -> List[Dict[str, Any]]:
        """Installed versions on this machine: ``{id, label, version, path, supported,
        problem}``; ``id`` is what a session records as its toolchain"""
        return []

    @classmethod
    def detect(cls, folder: str) -> Optional[Dict[str, Any]]:
        """Whether the folder is a project of this backend: ``{backend, evidence: [str],
        project: {...}, toolchain, note}``, or None"""
        return None

    @classmethod
    def is_document(cls, path: str) -> bool:
        return path.endswith(cls.documents) or os.path.basename(path) in cls.project_files

    # ----- what the core asks of the backend

    def capabilities(self) -> Dict[str, Any]:
        """What this session's prover can do (design §5); the app shows features by it"""
        return {}

    # ----- the protocol

    async def start(self) -> None:
        raise NotImplementedError

    def send(self, obj: Dict[str, Any]) -> None:
        raise NotImplementedError

    async def request(self, obj: Dict[str, Any], timeout: float = 120.0) -> Dict[str, Any]:
        raise NotImplementedError

    async def stop(self) -> None:
        raise NotImplementedError

    def emit(self, obj: Dict[str, Any]) -> None:
        """An event to the host core"""
        try:
            self.session.on_backend_event(obj)
        except Exception as exn:  # keep going whatever an event does
            import sys
            print("driver event error", repr(exn), file=sys.stderr, flush=True)


async def stop_process(proc: Optional[asyncio.subprocess.Process], close_stdin: bool = True) -> None:
    """End a prover process and its process group: politely, then by signal"""
    if not proc or proc.returncode is not None:
        return
    if close_stdin:
        with contextlib.suppress(Exception):
            assert proc.stdin
            proc.stdin.close()
    try:
        await asyncio.wait_for(proc.wait(), 20)
    except asyncio.TimeoutError:
        with contextlib.suppress(ProcessLookupError):
            os.killpg(proc.pid, signal.SIGTERM)
        try:
            await asyncio.wait_for(proc.wait(), 10)
        except asyncio.TimeoutError:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(proc.pid, signal.SIGKILL)


def registry() -> Dict[str, type]:
    """The backends this host knows, by name"""
    out: Dict[str, type] = {}
    with contextlib.suppress(ImportError):
        from .isabelle import IsabelleDriver
        out["isabelle"] = IsabelleDriver
    with contextlib.suppress(ImportError):
        from .lean import LeanDriver
        out["lean"] = LeanDriver
    return out


def message(kind: str, text: str, raw: Optional[str] = None, spans: Optional[list] = None,
            tips: Optional[list] = None, **extra: Any) -> Dict[str, Any]:
    """A Message, as the backend protocol has them"""
    return dict({"kind": kind, "text": text, "raw": text if raw is None else raw, "spans": spans or [],
                 "tips": tips or [], "sendbacks": []}, **extra)


def dumps(obj: Any) -> str:
    return json.dumps(obj, ensure_ascii=False)
