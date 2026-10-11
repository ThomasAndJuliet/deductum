"""MCP adapter: agents' access to Isabellum's shared Isabelle sessions.

``isabellum_host.py mcp [--ssh DESTINATION]`` speaks MCP (JSON-RPC over stdio) to the
agent and the host protocol to the session host, locally or on another machine over
ssh. The agent works in the same live session as the human (design C2): it reads the
unsaved buffers, edits with revision checks (C7), sees proof states at its own
positions without moving anyone's caret, and runs Sledgehammer. It is identified by
its conversation id (C9); notices about conflicting changes, grants and disk
conflicts are appended to every tool result.
"""

from __future__ import annotations

import json
import os
import re
import socket
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

import isabellum_host as H
import isabellum_ot as ot

SERVER_INFO = {"name": "isabellum", "version": H.VERSION}

INSTRUCTIONS = """\
Isabellum gives you the human's live Isabelle sessions: the same unsaved theory text
the human sees, checked continuously by one shared prover.

- Start with isabelle_sessions, or isabelle_open_session for a folder.
- isabelle_read_theory returns numbered lines and a revision. Pass that revision to
  isabelle_edit_theory; edits made against an older revision are rebased if the text
  you targeted is unchanged, and refused with the current text if it changed.
- Editing the human's theories needs editing rights the human grants in Isabellum.
  Ask with isabelle_request_access, saying why. Meanwhile, or instead, work in your own copy: isabelle_start_independent (the human reviews
  and merges it).
- isabelle_proof_state shows the goal after the command at a line; isabelle_check
  waits for checking and lists errors; isabelle_sledgehammer finds proofs.
- Notices at the end of results tell you when someone changed or reverted your edits,
  when your rights changed, or when a theory changed on disk. Re-read before retrying.
- Never restart or resume a session the human stopped or paused; tell the human.
- Working with the human: isabelle_human_view says where they are and what they selected
  ("this lemma"); isabelle_show points them at lines; isabelle_add_note leaves a note on lines
  for them to answer (replies come as notices).
"""


def identity_from_env() -> Optional[Dict[str, str]]:
    if os.environ.get("ISABELLUM_AGENT_ID"):
        provider, _, conv = os.environ["ISABELLUM_AGENT_ID"].partition(":")
        return {"provider": provider or "agent", "conversation": conv or provider}
    if os.environ.get("CLAUDE_CODE_SESSION_ID"):
        return {"provider": "claude", "conversation": os.environ["CLAUDE_CODE_SESSION_ID"]}
    for k, v in os.environ.items():
        if re.match(r"CODEX_.*(SESSION|THREAD|CONVERSATION).*ID$", k) and v:
            return {"provider": "codex", "conversation": v}
    # Codex starts its MCP servers with a filtered environment and no conversation id: the
    # Codex process itself stands for the conversation, so a grant holds for its whole run
    parent = parent_agent()
    if parent:
        return {"provider": parent[0], "conversation": f"process-{parent[1]}"}
    return None


def parent_agent() -> Optional[tuple]:
    """The agent CLI this adapter runs under (name, pid), from the process tree"""
    pid = os.getppid()
    for _ in range(4):
        try:
            comm = Path(f"/proc/{pid}/comm").read_text().strip().lower()
            stat = Path(f"/proc/{pid}/stat").read_text()
        except OSError:
            return None
        for name in ("codex", "claude"):
            if comm.startswith(name):
                return name, pid
        pid = int(stat.rsplit(")", 1)[1].split()[1])
        if pid <= 1:
            return None
    return None


def label_from_env() -> str:
    if os.environ.get("ISABELLUM_AGENT_LABEL"):
        return os.environ["ISABELLUM_AGENT_LABEL"]
    if os.environ.get("CLAUDE_CODE_SESSION_ID") or os.environ.get("CLAUDECODE"):
        return "Claude"
    parent = parent_agent()
    if parent:
        return parent[0].capitalize()
    if any(k.startswith("CODEX_") for k in os.environ):
        return "Codex"
    return "Agent"


def location_from_env() -> Dict[str, str]:
    loc = {"machine": socket.gethostname()}
    if os.environ.get("ZELLIJ_SESSION_NAME"):
        loc["zellij"] = os.environ["ZELLIJ_SESSION_NAME"]
    if os.environ.get("COSMIC_TERM_TAB"):
        loc["tab"] = os.environ["COSMIC_TERM_TAB"]
    loc["cwd"] = os.getcwd()
    return loc


def S(type_: str, desc: str, **extra: Any) -> Dict[str, Any]:
    return dict({"type": type_, "description": desc}, **extra)


SESSION = S("string", "Session id from isabelle_sessions")
PATH = S("string", "Theory file path (absolute, or relative to the session folder)")
LINE = S("integer", "1-based line number")

TOOLS: List[Dict[str, Any]] = [
    {"name": "isabelle_sessions", "description": "List Isabellum sessions on the host: folder, base logic, "
     "prover state (Running, Paused, Stopped...), who is attached, your editing rights.",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "isabelle_open_session", "description": "Attach to the shared session for a theory folder, "
     "creating it (and starting its prover) if there is none. Set independent=true for your own copy of "
     "the folder in a git worktree, with its own prover, that the human later reviews and merges.",
     "inputSchema": {"type": "object", "properties": {
         "folder": S("string", "Folder of theories"),
         "logic": S("string", "Base logic session (default: the parent of the folder's ROOT session)"),
         "independent": S("boolean", "Your own copy instead of the shared session"),
     }, "required": ["folder"]}},
    {"name": "isabelle_start_independent", "description": "Fork your own independent session from a "
     "shared one, starting from the human's current unsaved text, in a git worktree on a new branch.",
     "inputSchema": {"type": "object", "properties": {"session": SESSION}, "required": ["session"]}},
    {"name": "isabelle_session_info", "description": "A session's theories with checking status, dirty "
     "buffers, conflicts, grants and attached clients.",
     "inputSchema": {"type": "object", "properties": {"session": SESSION}, "required": ["session"]}},
    {"name": "isabelle_read_theory", "description": "Read a theory's live text (unsaved edits included) "
     "with line numbers and the revision to pass to isabelle_edit_theory, plus current errors.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "start_line": LINE, "end_line": LINE}, "required": ["session", "path"]}},
    {"name": "isabelle_edit_theory", "description": "Edit a theory. Each edit replaces old_text (which "
     "must occur exactly once, or give occurrence) with new_text, in the text of the given revision. "
     "Edits against an older revision are rebased when their text is unchanged since; otherwise the "
     "edit is refused with the current text.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH,
         "revision": S("integer", "Revision the edits refer to, from isabelle_read_theory"),
         "edits": {"type": "array", "items": {"type": "object", "properties": {
             "old_text": S("string", "Exact text to replace (empty to insert at line)"),
             "new_text": S("string", "Replacement"),
             "occurrence": S("integer", "1-based occurrence when old_text is not unique"),
             "line": S("integer", "With empty old_text: insert at the start of this line")},
             "required": ["new_text"]}}},
         "required": ["session", "path", "revision", "edits"]}},
    {"name": "isabelle_check", "description": "Wait until the prover has checked a theory (or the timeout) "
     "and list its errors and warnings with line numbers.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "timeout": S("number", "Seconds to wait (default 60)")},
         "required": ["session", "path"]}},
    {"name": "isabelle_proof_state", "description": "The proof state and messages after the command at a "
     "line (or after the first occurrence of a text), at your own position: nobody's caret moves.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "line": LINE,
         "column": S("integer", "0-based column (default: end of line)"),
         "after_text": S("string", "Instead of line: the end of the first occurrence of this text")},
         "required": ["session", "path"]}},
    {"name": "isabelle_sledgehammer", "description": "Run Sledgehammer on the goal at a line (on a sorry or "
     "oops line, the goal before it) and wait for the result. Returns the proofs found and the line "
     "range they would replace. apply=true inserts the first proof found (needs editing rights).",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "line": LINE,
         "provers": S("string", "Space-separated provers (default: Isabelle's sledgehammer_provers)"),
         "isar_proofs": S("boolean", "Ask for Isar proofs"), "try0": S("boolean", "Try simple methods first (default true)"),
         "timeout": S("number", "Seconds to wait (default 120)"),
         "apply": S("boolean", "Insert the first proof found")}, "required": ["session", "path", "line"]}},
    {"name": "isabelle_find_theorems", "description": "Find theorems in the context at a line, e.g. "
     "query '\"_ + _ = _ + _\" name: comm'.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "line": LINE, "query": S("string", "find_theorems criteria"),
         "limit": S("integer", "Maximum results (default 40)")}, "required": ["session", "path", "line", "query"]}},
    {"name": "isabelle_run_diagnostic", "description": "Run a diagnostic command on the goal at a line without "
     "editing the theory: quickcheck, nitpick, try0, solve_direct, try, value, term, thm, typ, print_cases, "
     "print_facts, find_consts. Options in brackets work, e.g. 'nitpick [timeout = 60, card = 1-8]'. Returns the "
     "command's output (counterexamples, found proofs).",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "line": LINE,
         "command": S("string", "The diagnostic command, e.g. 'quickcheck' or 'value \"rev [1,2,3::nat]\"'"),
         "timeout": S("number", "Seconds to wait (default 120)")}, "required": ["session", "path", "line", "command"]}},
    {"name": "isabelle_cancel", "description": "Interrupt running proofs in a session (e.g. a looping "
     "tactic).", "inputSchema": {"type": "object", "properties": {"session": SESSION}, "required": ["session"]}},
    {"name": "isabelle_save", "description": "Save a theory now (theories are also autosaved).",
     "inputSchema": {"type": "object", "properties": {"session": SESSION, "path": PATH}, "required": ["session"]}},
    {"name": "isabelle_history", "description": "Recent changes to a theory: who changed what, which were "
     "reverted. Use with isabelle_revert.",
     "inputSchema": {"type": "object", "properties": {"session": SESSION, "path": PATH,
                                                      "mine": S("boolean", "Only your changes")},
                     "required": ["session", "path"]}},
    {"name": "isabelle_revert", "description": "Revert one of your own changes by its id from "
     "isabelle_history; later edits by others are kept.",
     "inputSchema": {"type": "object", "properties": {"session": SESSION, "path": PATH,
                                                      "change": S("integer", "Change id")},
                     "required": ["session", "path", "change"]}},
    {"name": "isabelle_conflict", "description": "For a theory that changed on disk while it had unsaved "
     "edits: the last saved text, the disk text and the buffer text.",
     "inputSchema": {"type": "object", "properties": {"session": SESSION, "path": PATH}, "required": ["session", "path"]}},
    {"name": "isabelle_resolve_conflict", "description": "Resolve a disk conflict: take the disk version, "
     "keep the buffer (overwriting disk), or give merged text. Needs editing rights; the human can revert it.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "choice": S("string", "disk, buffer or merge",
                                                       enum=["disk", "buffer", "merge"]),
         "text": S("string", "The merged theory text, for choice=merge")},
         "required": ["session", "path", "choice"]}},
    {"name": "isabelle_listen", "description": "Listen for the human's queries in Isabellum: waits up to "
     "50 seconds for the next one for you and returns it (the theory, line, lemma, goal and what they asked), "
     "or says there is none yet; call it again to keep listening. The human asks you by typing `@name …` on a "
     "proof line (name: the one you give here), or by choosing you for their claude/codex queries. Answer each "
     "query with isabelle_propose; the human inserts a proof from Isabellum, or tells you which one in your "
     "conversation.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION,
         "name": S("string", "What the human calls you, as in @name (short, e.g. 'tock' or 'helper')")},
         "required": ["session"]}},
    {"name": "isabelle_propose", "description": "Answer a query from isabelle_listen: proofs for the gap "
     "(each complete proof text for that line, e.g. 'by (simp add: foo_def)' or an Isar block), and facts to "
     "guide Sledgehammer. Isabellum checks each proof with the prover and shows them to the human with "
     "Insert buttons. Check proofs first where you can (isabelle_check in your own copy).",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "query": S("string", "The query's id, from isabelle_listen"),
         "proofs": {"type": "array", "items": {"type": "string"}, "description": "Proofs, best first"},
         "using": {"type": "array", "items": {"type": "string"}, "description": "Facts for a `using` line in front of Sledgehammer"},
         "message": S("string", "A sentence for the human, shown with the proofs")},
         "required": ["session", "query"]}},
    {"name": "isabelle_human_view", "description": "What the human is looking at in Isabellum: the theory, "
     "the caret's line, any selected text, the lemma (name and statement), the step the caret is in (the Isar "
     "statement the goal they see is for) and the proof around the caret. Use it when the human says "
     "'this lemma', 'this step', 'here' or 'the selected part'; then isabelle_proof_state at that line.",
     "inputSchema": {"type": "object", "properties": {"session": SESSION}, "required": ["session"]}},
    {"name": "isabelle_show", "description": "Show the human a place in a theory: Isabellum offers to go there "
     "(or goes, if the human follows you), highlighting the lines, with your message. Use it to point at "
     "what you are talking about rather than quoting it.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "line": LINE,
         "end_line": S("integer", "Last line to highlight (default: line)"),
         "message": S("string", "What to look at there, in a sentence")},
         "required": ["session", "path", "line"]}},
    {"name": "isabelle_add_note", "description": "Leave a note on lines of a theory, shown beside them in "
     "Isabellum until the human resolves it (it moves with edits). For questions, explanations and "
     "suggestions that belong to a place; the human's replies come as notices.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "path": PATH, "line": LINE,
         "end_line": S("integer", "Last line the note is about (default: line)"),
         "text": S("string", "The note")}, "required": ["session", "path", "line", "text"]}},
    {"name": "isabelle_notes", "description": "The open notes on a session's theories, with replies.",
     "inputSchema": {"type": "object", "properties": {"session": SESSION, "path": PATH}, "required": ["session"]}},
    {"name": "isabelle_resolve_note", "description": "Reply to a note, or resolve (remove) one of yours.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "note": S("string", "The note's id"), "text": S("string", "A reply (optional)"),
         "resolve": S("boolean", "Remove the note")}, "required": ["session", "note"]}},
    {"name": "isabelle_request_access", "description": "Ask the human for editing rights in a shared session: "
     "the whole folder, one theory, or the proof at a line. Isabellum shows the request with your reason; "
     "the answer comes as a notice (granted or denied). Until then, read and query freely, or work in an "
     "independent session.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "scope": S("string", "folder, theory or proof", enum=["folder", "theory", "proof"]),
         "path": S("string", "The theory, for scope theory or proof"),
         "line": S("integer", "A line inside the proof, for scope proof"),
         "reason": S("string", "What you want to change and why, in a sentence or two")},
         "required": ["session", "scope", "reason"]}},
    {"name": "isabelle_notices", "description": "Notices for you: conflicting or reverted changes, grants, "
     "disk conflicts, failed saves.", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "isabelle_session_control", "description": "Stop, pause or resume the prover of YOUR OWN "
     "independent session, or discard it. The human controls shared sessions.",
     "inputSchema": {"type": "object", "properties": {
         "session": SESSION, "action": S("string", "stop, pause, resume or discard",
                                         enum=["stop", "pause", "resume", "discard"])},
         "required": ["session", "action"]}},
    {"name": "isabelle_merge_preview", "description": "The diff of your independent session against what it "
     "started from, as the human will see it when merging.",
     "inputSchema": {"type": "object", "properties": {"session": SESSION}, "required": ["session"]}},
]


class Adapter:
    def __init__(self, client: H.SyncClient):
        self.client = client
        self.identity = identity_from_env()
        self.hello = client.request("hello", client="agent", label=label_from_env(), identity=self.identity,
                                    location=location_from_env())
        self.texts: Dict[tuple, Dict[int, str]] = {}
        self.folders: Dict[str, str] = {}

    # ----- helpers

    def req(self, op: str, **params: Any) -> Dict[str, Any]:
        return self.client.request(op, **params)

    def resolve_path(self, session: str, path: str) -> str:
        if os.path.isabs(os.path.expanduser(path)):
            return os.path.expanduser(path)
        folder = self.folders.get(session)
        if folder is None:
            info = self.req("session_info", session=session)["session"]
            folder = self.folders[session] = info["folder"]
        return os.path.join(folder, path)

    def read(self, session: str, path: str) -> Dict[str, Any]:
        doc = self.req("read", session=session, path=path, subscribe=False)["doc"]
        self.texts.setdefault((session, doc["path"]), {})[doc["revision"]] = doc["text"]
        cache = self.texts[(session, doc["path"])]
        for r in sorted(cache)[:-20]:
            cache.pop(r, None)
        return doc

    @staticmethod
    def numbered(text: str, start: int = 1, end: Optional[int] = None) -> str:
        lines = text.split("\n")
        end = len(lines) if end is None else min(end, len(lines))
        width = len(str(end))
        return "\n".join(f"{i:>{width}}  {lines[i - 1]}" for i in range(max(1, start), end + 1))

    @staticmethod
    def diagnostics(text: str, prover: Dict[str, Any], limit: int = 40) -> str:
        msgs = prover.get("messages") or []
        out = []
        for m in msgs:
            if m["kind"] not in ("error", "warning", "legacy"):
                continue
            line, _ = ot.lines_cols(text, min(m["start"], len(text)))
            out.append(f"line {line}: {m['kind']}: {m['text'].strip()}")
            if len(out) >= limit:
                out.append("...")
                break
        status = prover.get("status") or {}
        head = (f"status: {status.get('finished', 0)} finished, {status.get('unprocessed', 0)} unprocessed, "
                f"{status.get('running', 0)} running, {status.get('failed', 0)} failed"
                + (", fully checked" if status.get("consolidated") else "")) if status else "status: not checked yet"
        return head + ("\n" + "\n".join(out) if out else "\nno errors or warnings")

    def offset_for(self, text: str, args: Dict[str, Any]) -> int:
        if args.get("after_text"):
            i = text.find(args["after_text"])
            if i < 0:
                raise H.HostError("after_text not found in the theory")
            return i + len(args["after_text"]) - 1
        line = int(args.get("line", 1))
        if "column" in args and args["column"] is not None:
            return ot.offset_of(text, line, int(args["column"]))
        start = ot.offset_of(text, line, 0)
        end = text.find("\n", start)
        end = len(text) if end < 0 else end
        # The last non-blank character of the line: the command there
        while end > start and text[end - 1] in " \t":
            end -= 1
        return max(start, end - 1)

    # ----- tools

    def call(self, name: str, args: Dict[str, Any]) -> str:
        session = args.get("session")
        path = self.resolve_path(session, args["path"]) if args.get("path") and session else args.get("path")
        if name == "isabelle_sessions":
            sessions = self.req("sessions")["sessions"]
            if not sessions:
                return "No sessions. Use isabelle_open_session with a theory folder."
            key = self.hello.get("key")
            lines = []
            for s in sessions:
                rights = [g for g in s["grants"] if g["agent"] == key]
                own = s.get("owner") and s["owner"].get("key") == key
                lines.append(
                    f"- {s['id']}: {s['name']} [{s['lifecycle']}] {s['folder']} (logic {s['logic']})"
                    + (f" — {s['message']}" if s.get("message") else "")
                    + ("; YOUR independent session" if own else "")
                    + ("; you may edit: " + ", ".join(g["scope"] + (" " + os.path.basename(g["path"]) if g.get("path") else "")
                                                      for g in rights) if rights else
                       ("" if own else "; read-only for you"))
                    + (f"; clients: {', '.join(c['label'] for c in s['clients'])}" if s["clients"] else ""))
            return "\n".join(lines)
        if name == "isabelle_open_session":
            r = self.req("session_create", folder=args["folder"], logic=args.get("logic"),
                         independent=bool(args.get("independent")), start=True, resume=False)
            s = r["session"]
            self.folders[s["id"]] = s["folder"]
            return (f"Session {s['id']} ({s['name']}) on {s['folder']}, logic {s['logic']}: {s['lifecycle']}"
                    + (f" — {s['message']}" if s.get("message") else "")
                    + ("\nThis is your independent copy; the human reviews and merges it." if s.get("owner") else ""))
        if name == "isabelle_start_independent":
            s = self.req("session_fork", session=session)["session"]
            self.folders[s["id"]] = s["folder"]
            return (f"Independent session {s['id']} in {s['folder']} (branch {s['parent']['branch']}): "
                    f"{s['lifecycle']}. Edit freely there; the human reviews and merges your work.")
        if name == "isabelle_session_info":
            s = self.req("session_info", session=session)["session"]
            self.folders[session] = s["folder"]
            out = [f"{s['name']} [{s['lifecycle']}] {s['folder']} logic {s['logic']} epoch {s['epoch']}"]
            if s.get("message"):
                out.append(s["message"])
            for t in s["theories"][:200]:
                st = t.get("status") or {}
                flag = " (unsaved)" if t["dirty"] else ""
                flag += " (CONFLICT)" if t["conflict"] else ""
                stat = (f"{st.get('percentage', 0)}% {'failed ' + str(st['failed']) if st.get('failed') else ''}"
                        if st else "not loaded")
                out.append(f"  {os.path.relpath(t['path'], s['folder'])}: {stat}{flag}")
            if s["grants"]:
                out.append("grants: " + "; ".join(f"{g['label']}: {g['scope']}" for g in s["grants"]))
            return "\n".join(out)
        if name == "isabelle_read_theory":
            doc = self.read(session, path)
            start = int(args.get("start_line") or 1)
            end = args.get("end_line")
            head = (f"{doc['path']} revision {doc['revision']}" + (" (unsaved edits)" if doc["dirty"] else "")
                    + (" — CHANGED ON DISK, see isabelle_conflict" if doc["conflict"] else ""))
            return head + "\n" + self.numbered(doc["text"], start, int(end) if end else None) + "\n\n" + \
                self.diagnostics(doc["text"], doc["prover"])
        if name == "isabelle_edit_theory":
            # (one edit given directly, or no revision: the current text, read now)
            if "edits" not in args and ("old_text" in args or "new_text" in args):
                args["edits"] = [{k: args[k] for k in ("old_text", "new_text", "line", "occurrence") if k in args}]
            if not args.get("edits"):
                raise H.HostError("Give edits: a list of {old_text, new_text}")
            if args.get("revision") is None:
                args["revision"] = self.read(session, path)["revision"]
            rev = int(args["revision"])
            cache = self.texts.get((session, os.path.realpath(path)), {})
            text = cache.get(rev)
            if text is None:
                doc = self.read(session, path)
                if doc["revision"] != rev:
                    raise H.HostError(f"Revision {rev} is not one you read; the theory is at revision "
                                      f"{doc['revision']}. Read it again.")
                text = doc["text"]
            edits = []
            for e in args["edits"]:
                old = e.get("old_text") or ""
                new = e.get("new_text", "")
                if not old:
                    pos = ot.offset_of(text, int(e.get("line", 1)), 0)
                    edits.append({"start": pos, "end": pos, "text": new})
                    continue
                occ = [m.start() for m in re.finditer(re.escape(old), text)]
                if not occ:
                    raise H.HostError(f"old_text not found in revision {rev}: {old[:80]!r}")
                n = int(e.get("occurrence") or 0)
                if len(occ) > 1 and not n:
                    raise H.HostError(f"old_text occurs {len(occ)} times; give occurrence or more context: {old[:80]!r}")
                pos = occ[(n or 1) - 1]
                edits.append({"start": pos, "end": pos + len(old), "text": new})
            r = self.req("edit", session=session, path=path, revision=rev, edits=edits, policy="strict")
            doc = self.read(session, path)
            first = r["change"][0][0] if r["change"] else 0
            line = ot.lines_cols(doc["text"], min(first, len(doc["text"])))[0]
            return (f"Edited: now revision {doc['revision']} (change {r.get('entry')}). Around line {line}:\n"
                    + self.numbered(doc["text"], max(1, line - 3), line + 8)
                    + "\nThe prover rechecks continuously; use isabelle_check for errors.")
        if name == "isabelle_check":
            timeout = float(args.get("timeout") or 60)
            t0 = time.time()
            doc = self.read(session, path)
            while time.time() - t0 < timeout:
                st = (doc["prover"] or {}).get("status") or {}
                if doc["prover"].get("revision") == doc["revision"] and st.get("consolidated") \
                        and not st.get("running"):
                    break
                time.sleep(1.0)
                doc = self.read(session, path)
            info = self.req("session_info", session=session)["session"]
            if info["lifecycle"] != "Running":
                return f"The session is {info['lifecycle']}" + (f": {info['message']}" if info.get("message") else "") + \
                    "\n" + self.diagnostics(doc["text"], doc["prover"])
            return f"{doc['path']} revision {doc['revision']}\n" + self.diagnostics(doc["text"], doc["prover"])
        if name == "isabelle_proof_state":
            doc = self.read(session, path)
            offset = self.offset_for(doc["text"], args)
            r = self.req("state", session=session, path=path, offset=offset, revision=doc["revision"], margin=100)
            if not r.get("command"):
                return "No command at that position."
            c = r["command"]
            # On a placeholder (sorry, oops): the goal it stands in for, before it
            if c["name"] in ("sorry", "oops") and c["start"] > 0:
                r2 = self.req("state", session=session, path=path, offset=c["start"] - 1, revision=doc["revision"], margin=100)
                if r2.get("command"):
                    r, c = r2, r2["command"]
            line = ot.lines_cols(doc["text"], min(c["start"], len(doc["text"])))[0]
            out = [f"After `{c['name']}` at line {line} ({c['status']}){' (prover catching up)' if r.get('outdated') else ''}:"]
            for m in r["messages"]:
                out.append(("" if m["kind"] == "state" else m["kind"] + ": ") + m["text"])
            return "\n".join(out) if r["messages"] else out[0] + "\n(no output)"
        if name in ("isabelle_sledgehammer", "isabelle_find_theorems"):
            doc = self.read(session, path)
            offset = self.offset_for(doc["text"], args)
            if name == "isabelle_sledgehammer":
                op, qargs = "sledgehammer", [args.get("provers") or "", "true" if args.get("isar_proofs") else "false",
                                             "false" if args.get("try0") is False else "true"]
                timeout = float(args.get("timeout") or 120)
            else:
                op, qargs = "find_theorems", [str(args.get("limit") or 40), "false", args["query"]]
                timeout = 60.0
            qid = self.req("query", session=session, path=path, offset=offset, revision=doc["revision"],
                           operation=op, args=qargs)["query"]
            t0 = time.time()
            q = {}
            while time.time() - t0 < timeout:
                q = self.req("query_status", session=session, query=qid)["query"]
                if q["status"] in ("finished", "failed", "canceled"):
                    break
                time.sleep(0.7)
            else:
                self.req("cancel_query", session=session, query=qid)
                q["status"] = "timed out"
            texts = [m["text"] for m in q.get("messages", [])]
            if op == "find_theorems":
                return "\n".join(texts) or f"No result ({q['status']})."
            proofs = []
            for m in q.get("messages", []):
                for sb in m.get("sendbacks", []):
                    if sb not in proofs:
                        proofs.append(sb)
            doc = self.read(session, path)
            out = [f"Sledgehammer {q['status']}" + (f": {q['error']}" if q.get("error") else "")]
            out += ["  " + t for t in texts if "found a proof" not in t]
            if proofs:
                rep = q.get("replace")
                where = (f"replacing the {doc['text'][rep['start']:rep['end']].strip()} at line "
                         f"{ot.lines_cols(doc['text'], rep['start'])[0]}" if rep else
                         f"after the command ending at line {ot.lines_cols(doc['text'], q.get('end', 0))[0]}")
                out.append("Proofs found (" + where + "):")
                out += [f"  {i + 1}. {p}" for i, p in enumerate(proofs)]
                if args.get("apply"):
                    proof = proofs[0]
                    if rep:
                        edit = {"start": rep["start"], "end": rep["end"], "text": proof}
                    else:
                        indent = "  "
                        edit = {"start": q["end"], "end": q["end"], "text": "\n" + indent + proof}
                    try:
                        r = self.req("edit", session=session, path=path, revision=doc["revision"], edits=[edit],
                                     policy="strict")
                        out.append(f"Inserted `{proof}` (change {r.get('entry')}, revision {r['revision']}).")
                    except H.HostError as exn:
                        out.append(f"Not inserted ({exn.code}): {exn}")
            return "\n".join(out)
        if name == "isabelle_run_diagnostic":
            cmd = args["command"].strip()
            allowed = ("quickcheck", "nitpick", "try0", "solve_direct", "try", "value", "term", "thm", "typ",
                       "print_cases", "print_facts", "find_consts", "find_theorems", "print_context", "print_state")
            if cmd.split()[0].split("[")[0] not in allowed:
                raise H.HostError("Only diagnostic commands can run this way: " + ", ".join(allowed))
            doc = self.read(session, path)
            offset = self.offset_for(doc["text"], args)
            qid = self.req("query", session=session, path=path, offset=offset, revision=doc["revision"],
                           operation="isabellum_command", args=[cmd])["query"]
            t0 = time.time()
            q: Dict[str, Any] = {}
            while time.time() - t0 < float(args.get("timeout") or 120):
                q = self.req("query_status", session=session, query=qid)["query"]
                if q["status"] in ("finished", "failed", "canceled"):
                    break
                time.sleep(0.7)
            else:
                self.req("cancel_query", session=session, query=qid)
                q["status"] = "timed out"
            out = [f"{cmd}: {q.get('status')}" + (f" ({q['error']})" if q.get("error") else "")]
            for m in q.get("messages", []):
                out.append(("" if m["kind"] == "writeln" else m["kind"] + ": ") + m["text"])
            return "\n".join(out)
        if name == "isabelle_cancel":
            n = self.req("cancel", session=session)["canceled"]
            return f"Interrupted {n} running execution(s)."
        if name == "isabelle_save":
            r = self.req("save", session=session, path=path) if path else self.req("save", session=session)
            return "Saved." if not r["failed"] else "Not saved: " + "; ".join(f"{f['path']}: {f['error']}" for f in r["failed"])
        if name == "isabelle_history":
            hist = self.req("history", session=session, path=path, limit=40,
                            author=self.hello.get("key") if args.get("mine") else None)["history"]
            return "\n".join(
                f"change {h['id']} (rev {h['rev']}) {h['kind']} by {h['author'].get('label')} at line {h['line']}: "
                f"{h['summary']}" + (f" [reverted by {h['reverted_by']}]" if h.get("reverted_by") else "")
                for h in hist) or "No changes yet."
        if name == "isabelle_revert":
            r = self.req("revert", session=session, path=path, entry=int(args["change"]))
            return f"Reverted change(s) {r['reverted']}; now revision {r['revision']}."
        if name == "isabelle_conflict":
            doc = self.req("read", session=session, path=path, subscribe=False, conflict_texts=True)["doc"]
            if not doc.get("conflict"):
                return "No disk conflict for this theory."
            t = doc["conflict_texts"]
            return ("=== last saved (base) ===\n" + (t["base"] or "") + "\n=== disk ===\n" +
                    (t["disk"] if t["disk"] is not None else "(deleted)") + "\n=== buffer (unsaved) ===\n" + t["buffer"])
        if name == "isabelle_resolve_conflict":
            r = self.req("resolve", session=session, path=path, choice=args["choice"], text=args.get("text"))
            return f"Resolved; revision {r['revision']}. It will be saved shortly."
        if name == "isabelle_listen":
            r = self.req("listen", session=session, name=args.get("name", ""), timeout=50)
            out = []
            for job in r.get("withdrawn") or []:
                out.append(f"Query {job} was withdrawn by the human (its line was changed or deleted).")
            q = r.get("query")
            if not q:
                out.append("No query yet. Call isabelle_listen again to keep listening.")
                return "\n".join(out)
            out.append(f"Query {q['job']} from the human, at line {q['line']} of {q['path']}:")
            out.append(f"They typed: {q['asked'].strip()}" + (f"\nTheir hint: {q['hint']}" if q.get("hint") else ""))
            out.append(f"The lemma up to that line:\n{q['lemma']}")
            if q.get("goal"):
                out.append(f"The goal there:\n{q['goal']}")
            out.append("Answer with isabelle_propose (query " + q["job"] + "), then listen again.")
            return "\n\n".join(out)
        if name == "isabelle_propose":
            self.req("propose", session=session, query=args["query"], proofs=args.get("proofs") or [],
                     using=args.get("using") or [], message=args.get("message", ""))
            return ("Sent. Isabellum checks the proofs and shows them to the human, who inserts one (or tells you "
                    "which). Listen again for the next query.")
        if name == "isabelle_human_view":
            v = self.req("human_view", session=session)["view"]
            if not v.get("open"):
                return "The human has no theory of this session open in Isabellum."
            out = [f"The human is at line {v.get('line')} of {v.get('path')} ({v.get('mode') or 'literate'} view)."]
            if v.get("selection"):
                out.append("Selected:\n" + v["selection"])
            if v.get("lemma"):
                lm = v["lemma"]
                out.append(f"This lemma: {lm['kind']} {lm.get('name') or '(unnamed)'}, line {lm['line']}:\n{lm['statement']}")
            if v.get("construct"):
                c = v["construct"]
                out.append(f"This step (lines {c['line']}-{c['end_line']}, the construct the goal shown to "
                           f"the human is for):\n{c['text']}")
            if v.get("proof"):
                out.append(f"The proof around the caret (from line {v['proof']['line']}):\n{v['proof']['text']}")
            return "\n\n".join(out)
        if name in ("isabelle_show", "isabelle_add_note"):
            doc = self.read(session, path)
            text = doc["text"]
            line = int(args["line"])
            end_line = int(args.get("end_line") or line)
            start = ot.offset_of(text, line, 0)
            nxt = ot.offset_of(text, end_line + 1, 0) if end_line < text.count("\n") + 1 else len(text)
            end = max(start, nxt - 1)
            if name == "isabelle_show":
                r = self.req("show", session=session, path=path, start=start, end=end,
                             revision=doc["revision"], message=args.get("message", ""))
                return (f"Offered the human line {r['line']}." if r.get("shown")
                        else "Isabellum is not open; nothing was shown.")
            r = self.req("add_note", session=session, path=path, start=start, end=end,
                         revision=doc["revision"], text=args["text"])["note"]
            return f"Note {r['id']} added at line {r['line']}. Replies come as notices."
        if name == "isabelle_notes":
            notes = self.req("notes", session=session, path=path)["notes"]
            if not notes:
                return "No open notes."
            out = []
            for n in notes:
                out.append(f"[{n['id']}] {Path(n['path']).name} line {n['line']} ({n['label']}): {n['text']}")
                out.extend(f"    {r['by']}: {r['text']}" for r in n["replies"])
            return "\n".join(out)
        if name == "isabelle_resolve_note":
            self.req("reply_note", session=session, note=args["note"], text=args.get("text", ""),
                     resolve=bool(args.get("resolve")))
            return "Done."
        if name == "isabelle_request_access":
            params: Dict[str, Any] = {"session": session, "scope": args["scope"], "reason": args.get("reason", "")}
            if path:
                params["path"] = path
                if args["scope"] == "proof":
                    doc = self.read(session, path)
                    params["offset"] = self.offset_for(doc["text"], args)
            r = self.req("request_access", **params)["request"]
            what = {"folder": "the folder", "theory": "the theory"}.get(r["scope"], f"the proof at line {r.get('line')}")
            return (f"Asked the human for editing rights to {what}. The answer comes as a notice "
                    "(isabelle_notices, or at the end of later results).")
        if name == "isabelle_notices":
            return ""  # appended below
        if name == "isabelle_session_control":
            action = args["action"]
            if action == "discard":
                self.req("discard", session=session)
                return "Discarded."
            s = self.req(action, session=session)["session"]
            return f"{s['name']}: {s['lifecycle']}"
        if name == "isabelle_merge_preview":
            files = self.req("merge_preview", session=session)["files"]
            return "\n".join(f["diff"] for f in files) or "No changes against the starting point."
        raise H.HostError(f"Unknown tool {name}")

    def notices_text(self) -> str:
        try:
            notices = self.req("notices", ack=True)["notices"]
        except H.HostError:
            return ""
        if not notices:
            return ""
        lines = ["", "--- Isabellum notices ---"]
        for n in notices:
            line = f"[{n['kind']}] {n.get('message', '')}"
            if n.get("text") and n["kind"] in ("overlap", "reverted"):
                line += f"\n    current text there: {n['text'][:300]!r}"
            lines.append(line)
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# MCP over stdio


def main(argv: List[str]) -> int:
    ssh = None
    i = 0
    while i < len(argv):
        if argv[i] == "--ssh" and i + 1 < len(argv):
            ssh = argv[i + 1]
            i += 2
        else:
            i += 1
    ssh = ssh or os.environ.get("ISABELLUM_HOST_SSH")
    adapter: Optional[Adapter] = None

    def get_adapter() -> Adapter:
        nonlocal adapter
        if adapter is None:
            client = H.SyncClient.over_ssh(ssh) if ssh else H.SyncClient.local()
            adapter = Adapter(client)
        return adapter

    def respond(rid: Any, result: Any = None, error: Optional[Dict[str, Any]] = None) -> None:
        msg: Dict[str, Any] = {"jsonrpc": "2.0", "id": rid}
        if error is not None:
            msg["error"] = error
        else:
            msg["result"] = result
        sys.stdout.write(json.dumps(msg, ensure_ascii=False) + "\n")
        sys.stdout.flush()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        method = msg.get("method")
        rid = msg.get("id")
        if rid is None:
            continue  # notifications
        if method == "initialize":
            version = (msg.get("params") or {}).get("protocolVersion") or "2025-06-18"
            respond(rid, {"protocolVersion": version, "capabilities": {"tools": {"listChanged": False}},
                          "serverInfo": SERVER_INFO, "instructions": INSTRUCTIONS})
        elif method == "ping":
            respond(rid, {})
        elif method == "tools/list":
            respond(rid, {"tools": TOOLS})
        elif method == "tools/call":
            params = msg.get("params") or {}
            name = params.get("name", "")
            args = params.get("arguments") or {}
            try:
                a = get_adapter()
                text = a.call(name, args)
                text += a.notices_text()
                respond(rid, {"content": [{"type": "text", "text": text or "(no notices)"}], "isError": False})
            except H.HostError as exn:
                detail = str(exn)
                if exn.data.get("current"):
                    detail += "\nCurrent text:\n" + "\n".join(
                        f"  line {c['line']}: {c['text']!r}" for c in exn.data["current"])
                extra = adapter.notices_text() if adapter else ""
                respond(rid, {"content": [{"type": "text", "text": f"Error ({exn.code}): {detail}{extra}"}],
                              "isError": True})
            except (ConnectionError, OSError) as exn:
                adapter = None
                respond(rid, {"content": [{"type": "text", "text": f"Cannot reach the Isabellum host: {exn}"}],
                              "isError": True})
            except Exception as exn:  # report, keep serving
                text = f"Missing argument: {exn}" if isinstance(exn, KeyError) else f"Internal error: {type(exn).__name__}: {exn}"
                respond(rid, {"content": [{"type": "text", "text": text}],
                              "isError": True})
        else:
            respond(rid, error={"code": -32601, "message": f"Unknown method {method}"})
    return 0
