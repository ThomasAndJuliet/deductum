"""End-to-end check of the session host against a real Isabelle2025 runtime.

Run on a machine with a supported runtime:

    ISABELLUM_STATE_DIR=/tmp/isabellum-it python3 tests/integration_host.py

It starts a private host, creates a session on a scratch theory folder, and checks
editing, rebasing, undo, revert, notices, grants, autosave, external changes,
proof states, Sledgehammer, Pause/Resume and Stop/Resume.
"""

import json
import os
import shutil
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))
os.environ.setdefault("ISABELLUM_STATE_DIR", tempfile.mkdtemp(prefix="isabellum-it-"))

import isabellum_host as H  # noqa: E402

THY = """theory Demo
  imports Main
begin

text \\<open>A small demo with $x^2$ maths.\\<close>

lemma foo: "\\<forall>x::nat. x + 0 = x"
  apply simp
  done

lemma bar: "(a::nat) + b = b + a"
  sorry

end
"""

FAILURES = []


def check(cond, what):
    print(("ok   " if cond else "FAIL ") + what, flush=True)
    if not cond:
        FAILURES.append(what)


def wait_for(client, pred, timeout=240, what="condition"):
    t0 = time.time()
    while time.time() - t0 < timeout:
        for i, ev in enumerate(client.events):
            if pred(ev):
                del client.events[:i + 1]
                return ev
        client.events.clear()
        try:
            client.sock.settimeout(1.0)
            msg = client.read_message()
            client.events.append(msg)
        except (TimeoutError, OSError):
            pass
        finally:
            client.sock.settimeout(None)
    raise SystemExit(f"timed out waiting for {what}")


def main():
    # A stand-in for the claude CLI, for the typed-keyword fill
    fake = Path(tempfile.mkdtemp(prefix="isabellum-bin-"))
    (fake / "claude").write_text('#!/bin/sh\necho \'{"proofs": ["by simp", "by blast"], '
                                 '"using": ["mult_1_right", "no_such_fact_xyz"]}\'\n')
    (fake / "claude").chmod(0o755)
    os.environ["PATH"] = f"{fake}:{os.environ.get('PATH', '')}"
    folder = Path(tempfile.mkdtemp(prefix="isabellum-thy-"))
    thy = folder / "Demo.thy"
    thy.write_text(THY)
    app = H.SyncClient.local()
    app.request("hello", client="app", label="test app")
    agent = H.SyncClient.local()
    agent.request("hello", client="agent", label="Claude (test)",
                  identity={"provider": "claude", "conversation": "conv-1"},
                  location={"zellij": "zj-test", "tab": "tab-1"})
    other = H.SyncClient.local()
    other.request("hello", client="agent", label="Codex (test)",
                  identity={"provider": "codex", "conversation": "conv-2"})

    s = app.request("session_create", folder=str(folder), logic="HOL", new=True)["session"]
    sid = s["id"]
    check(s["lifecycle"] == "Starting", "session starts")
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    rev0 = doc["revision"]
    wait_for(app, lambda e: e.get("event") == "session" and e["session"]["id"] == sid
             and e["session"]["lifecycle"] == "Running", what="Running")
    ev = wait_for(app, lambda e: e.get("event") == "doc_prover" and e["prover"]["status"]
                  and e["prover"]["status"].get("consolidated"), what="checked theory")
    names = [c[2] for c in ev["prover"]["commands"]]
    check("lemma" in names and "sorry" in names, "prover reports command spans")

    # Hover information and definitions
    r = app.request("info", session=sid, path=str(thy), offset=THY.index("x + 0"))["info"]
    texts = [i["text"] for i in (r.get("tooltip") or {}).get("items", [])]
    check(any("nat" in t for t in texts), "hover shows the type of a variable: %s" % texts)
    r = app.request("info", session=sid, path=str(thy), offset=THY.index("+ 0"))["info"]
    d = r.get("definition") or {}
    check(bool(d.get("file")) and d.get("line"), "Ctrl+click finds the definition of +: %s" % d)

    st = app.request("state", session=sid, path=str(thy), offset=THY.index("lemma foo") + 5)
    tips = [t[2] for m in st["messages"] for t in m.get("tips", [])]
    check(any("nat" in t for t in tips) and any("constant" in t for t in tips), "output carries hover tips: %s" % tips[:6])

    st = app.request("state", session=sid, path=str(thy), offset=THY.index("apply simp"))
    check(any("No subgoals" in m["text"] for m in st["messages"]), "proof state after apply simp")

    # An agent without rights cannot edit
    try:
        agent.request("edit", session=sid, path=str(thy), revision=rev0,
                      edits=[{"start": THY.index("sorry"), "end": THY.index("sorry") + 5, "text": "by simp"}])
        check(False, "agent without grant is refused")
    except H.HostError as e:
        check(e.code == "forbidden", "agent without grant is refused")

    # Collaboration: the agent sees where the human is, shows a place, leaves a note
    app.request("view", session=sid, path=str(thy), offset=THY.index("apply simp"), selection="apply simp",
                mode="literate")
    v = agent.request("human_view", session=sid)["view"]
    check(v["open"] and v["selection"] == "apply simp" and v["line"] == 8 and "lemma foo" in v["proof"]["text"],
          "the agent sees the human's place, selection and proof")
    agent.request("show", session=sid, path=str(thy), start=THY.index("lemma bar"), end=THY.index("lemma bar") + 9,
                  message="this one")
    ev = wait_for(app, lambda e: e.get("event") == "show", timeout=10, what="show")
    check(ev["line"] == 11 and ev["message"] == "this one" and ev["from"]["label"] == "Claude (test)",
          "the agent shows the human a place")
    n = agent.request("add_note", session=sid, path=str(thy), start=THY.index("lemma bar"),
                      end=THY.index("lemma bar") + 9, text="needs commutativity")["note"]
    ev = wait_for(app, lambda e: e.get("event") == "notes", timeout=10, what="notes")
    check(ev["notes"] and ev["notes"][0]["text"] == "needs commutativity", "the app sees the agent's note")
    app.request("reply_note", session=sid, note=n["id"], text="use add.commute", resolve=True)
    notices = agent.request("notices")["notices"]
    check(any(x["kind"] == "note_resolved" and x["reply"] == "use add.commute" for x in notices)
          and not agent.request("notes", session=sid)["notes"], "the human's reply reaches the agent")

    # The agent asks for rights; the app sees the request; the human declines
    r = agent.request("request_access", session=sid, scope="theory", path=str(thy), reason="finish bar")["request"]
    ev = wait_for(app, lambda e: e.get("event") == "access_request", timeout=10, what="access request")
    check(ev["request"]["id"] == r["id"] and ev["request"]["reason"] == "finish bar", "the app sees the agent's request")
    try:
        agent.request("answer_request", session=sid, request=r["id"], allow=True)
        check(False, "agents cannot answer requests")
    except H.HostError as e:
        check(e.code == "forbidden", "agents cannot answer requests")
    app.request("answer_request", session=sid, request=r["id"], allow=False)
    notices = agent.request("notices")["notices"]
    info = app.request("session_info", session=sid)["session"]
    check(any(n["kind"] == "denied" for n in notices) and not info["requests"] and not info["grants"],
          "a declined request tells the agent and grants nothing")

    # Grant the whole folder; the agent is told
    g = app.request("grant", session=sid, agent="claude:conv-1", label="Claude (test)", scope="folder")["grant"]
    notices = agent.request("notices")["notices"]
    check(any(n["kind"] == "granted" for n in notices), "agent receives grant notice")

    # The human types at the top while the agent edits the proof against an older revision
    app.request("edit", session=sid, path=str(thy), revision=rev0,
                edits=[{"start": THY.index("text"), "end": THY.index("text"), "text": "(* human *)\n"}],
                txn="t1")
    r = agent.request("edit", session=sid, path=str(thy), revision=rev0,
                      edits=[{"start": THY.index("sorry"), "end": THY.index("sorry") + 5, "text": "by simp"}])
    text = app.request("read", session=sid, path=str(thy))["doc"]["text"]
    check("(* human *)" in text and "by simp" in text and "sorry" not in text,
          "stale agent edit is rebased past the human's edit")
    agent_entry = r["entry"]

    # A conflicting stale edit is rejected with the current text
    try:
        other_rev = r["revision"]
        agent.request("edit", session=sid, path=str(thy), revision=rev0,
                      edits=[{"start": THY.index("sorry"), "end": THY.index("sorry") + 5, "text": "by auto"}])
        check(False, "overlapping stale edit is rejected")
    except H.HostError as e:
        check(e.code == "conflict" and e.data.get("current"), "overlapping stale edit is rejected")

    # The human overwrites part of the agent's proof: the agent gets an overlap notice
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    pos = doc["text"].index("by simp")
    app.request("edit", session=sid, path=str(thy), revision=doc["revision"],
                edits=[{"start": pos + 3, "end": pos + 7, "text": "arith"}], txn="t2")
    notices = agent.request("notices")["notices"]
    check(any(n["kind"] == "overlap" for n in notices), "agent told when the human edits its change")

    # Ctrl+Z undoes the human's last transaction only
    app.request("undo", session=sid, path=str(thy))
    text = app.request("read", session=sid, path=str(thy))["doc"]["text"]
    check("by simp" in text and "(* human *)" in text, "undo restores the agent's text, keeps human t1")
    app.request("undo", session=sid, path=str(thy))
    text = app.request("read", session=sid, path=str(thy))["doc"]["text"]
    check("(* human *)" not in text and "by simp" in text, "second undo removes t1, keeps agent change")
    app.request("redo", session=sid, path=str(thy))
    text = app.request("read", session=sid, path=str(thy))["doc"]["text"]
    check("(* human *)" in text, "redo")

    # Revert the agent's change; the agent hears about it
    app.request("revert", session=sid, path=str(thy), entry=agent_entry)
    text = app.request("read", session=sid, path=str(thy))["doc"]["text"]
    check("sorry" in text and "by simp" not in text, "revert agent change")
    notices = agent.request("notices")["notices"]
    check(any(n["kind"] == "reverted" for n in notices), "agent told its change was reverted")

    # Autosave
    time.sleep(3.5)
    check("(* human *)" in thy.read_text(), "autosave writes the theory")

    # External change with no unsaved edits: reloaded as a logged edit
    thy.write_text(thy.read_text().replace("lemma bar", "lemma baz"))
    time.sleep(2.5)
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    check("lemma baz" in doc["text"] and not doc["conflict"], "clean external change reloads")
    hist = app.request("history", session=sid, path=str(thy), limit=5)["history"]
    check(hist[0]["kind"] == "reload" and hist[0]["author"]["kind"] == "disk", "reload is a logged edit")

    # External change while dirty: conflict, autosave holds, agent resolves
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    app.request("edit", session=sid, path=str(thy), revision=doc["revision"],
                edits=[{"start": 0, "end": 0, "text": "(* dirty *)\n"}], txn="t3")
    thy.write_text(thy.read_text().replace("lemma foo", "lemma foo2"))
    time.sleep(1.5)
    doc = app.request("read", session=sid, path=str(thy), conflict_texts=True)["doc"]
    check(bool(doc["conflict"]) and "foo2" in doc["conflict_texts"]["disk"], "dirty external change conflicts")
    time.sleep(2.5)
    check("(* dirty *)" not in thy.read_text(), "autosave holds during a conflict")
    notices = agent.request("notices")["notices"]
    check(any(n["kind"] == "disk_conflict" for n in notices), "agent told about the disk conflict")
    merged = doc["conflict_texts"]["disk"].replace("theory Demo", "(* dirty *)\ntheory Demo")
    agent.request("resolve", session=sid, path=str(thy), choice="merge", text=merged)
    time.sleep(1.5)
    disk = thy.read_text()
    check("(* dirty *)" in disk and "foo2" in disk, "agent's resolution is saved")

    # Sledgehammer on the sorry
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    wait_for(app, lambda e: e.get("event") == "doc_prover" and e["prover"]["status"]
             and e["prover"]["status"].get("consolidated") and e["prover"]["revision"] == doc["revision"],
             what="recheck")
    q = app.request("query", session=sid, path=str(thy), offset=doc["text"].index("sorry"),
                    operation="sledgehammer", args=["", "false", "true"])["query"]
    ev = wait_for(app, lambda e: e.get("event") == "query" and e["id"] == q and e["status"] == "finished",
                  timeout=300, what="sledgehammer")
    proofs = [sb for m in ev["messages"] for sb in m["sendbacks"]]
    check(bool(proofs) and ev.get("replace"), "sledgehammer finds proofs and the span to replace")

    # Pause: queries say so immediately, edits still accepted
    app.request("pause", session=sid)
    try:
        app.request("state", session=sid, path=str(thy), offset=10)
        check(False, "paused session refuses queries")
    except H.HostError as e:
        check(e.code == "paused", "paused session refuses queries")
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    app.request("edit", session=sid, path=str(thy), revision=doc["revision"],
                edits=[{"start": 0, "end": 0, "text": "(* while paused *)\n"}], txn="t4")
    app.request("resume", session=sid)
    st = None
    for _ in range(60):
        try:
            doc = app.request("read", session=sid, path=str(thy))["doc"]
            st = app.request("state", session=sid, path=str(thy), offset=doc["text"].index("apply simp"))
            if any("No subgoals" in m["text"] for m in st["messages"]):
                break
        except H.HostError:
            pass
        time.sleep(1)
    check(st is not None and any("No subgoals" in m["text"] for m in st["messages"]), "resume rechecks")

    # Stop and Resume: a new epoch with the same buffers
    epoch = app.request("session_info", session=sid)["session"]["epoch"]
    app.request("stop", session=sid)
    info = app.request("session_info", session=sid)["session"]
    check(info["lifecycle"] == "Stopped", "stop")
    try:
        agent.request("resume", session=sid)
        check(False, "agents cannot resume the human's session")
    except H.HostError as e:
        check(e.code == "forbidden", "agents cannot resume the human's session")
    app.request("resume", session=sid)
    wait_for(app, lambda e: e.get("event") == "session" and e["session"]["id"] == sid
             and e["session"]["lifecycle"] == "Running", what="Running again")
    info = app.request("session_info", session=sid)["session"]
    check(info["epoch"] == epoch + 1, "resume starts a new epoch")

    # C10: Terminal Agents finds the agent's session by its Zellij session
    found = app.request("find_sessions", zellij="zj-test")["sessions"]
    check(any(f["session"]["id"] == sid for f in found), "find sessions by agent location")

    # Typing `claude <hint>` and Enter asks Claude (here a stand-in) to fill that line
    wait_for(app, lambda e: e.get("event") == "doc_prover" and e["prover"]["status"]
             and e["prover"]["status"].get("consolidated"), what="checked again")
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    at = doc["text"].rindex("end")
    app.request("edit", session=sid, path=str(thy), revision=doc["revision"],
                edits=[{"start": at, "end": at, "text": 'lemma baz: "(x::nat) * 1 = x"\n  claude try simp\n\n'}],
                txn="kw")
    ev = wait_for(app, lambda e: e.get("event") == "fill" and e.get("replace"), timeout=30, what="keyword fill")
    text = app.request("read", session=sid, path=str(thy))["doc"]["text"]
    check("claude try simp" not in text and text[ev["replace"]["start"]:ev["replace"]["end"]] == "sorry",
          "a typed claude line becomes the hole it fills")
    check(ev["agent"] == "claude" and ev["hint"] == "try simp", "the keyword's agent and hint are used")
    ev = wait_for(app, lambda e: e.get("event") == "fill" and e.get("status") in ("done", "failed"), timeout=240,
                  what="keyword fill result")
    check(any(c.get("ok") for c in ev.get("candidates", [])), "the keyword fill's proofs are checked")
    check(ev.get("using") == ["mult_1_right"], "suggested facts for sledgehammer are kept only if they exist")
    if not any(c.get("ok") for c in ev.get("candidates", [])):
        print("     ", json.dumps(ev)[:600])

    # The same without Enter: the app asks when the typing pauses on the line
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    at = doc["text"].rindex("\nend")
    app.request("edit", session=sid, path=str(thy), revision=doc["revision"],
                edits=[{"start": at, "end": at, "text": 'lemma qux: "(y::nat) * 1 = y"\n  claude'}], txn="kw2")
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    r = app.request("keyword", session=sid, path=str(thy), offset=doc["text"].rindex("claude") + 3,
                    revision=doc["revision"])
    ev = wait_for(app, lambda e: e.get("event") == "fill" and e.get("status") in ("done", "failed"), timeout=240,
                  what="paused keyword fill")
    check(r["started"] and any(c.get("ok") for c in ev.get("candidates", [])), "a paused-on claude line is filled too")
    check("canceled" in app.request("cancel_tools", session=sid), "proof tools can be stopped on their own")

    # A typed sledgehammer, stopped while it runs: it stops, as if its time ran out
    doc = app.request("read", session=sid, path=str(thy))["doc"]
    at = doc["text"].rindex("\nend")
    app.request("edit", session=sid, path=str(thy), revision=doc["revision"],
                edits=[{"start": at, "end": at, "text": '\nlemma zz: "rev (rev (xs::nat list)) = xs"\n  sledgehammer\n  sorry\n'}], txn="sh")
    def sh_cmd(e):
        return [c for c in (e.get("prover") or {}).get("commands", []) if c[2] == "sledgehammer"]
    wait_for(app, lambda e: e.get("event") == "doc_prover" and any(c[3] == "running" for c in sh_cmd(e)), timeout=120,
             what="sledgehammer running")
    n = app.request("cancel_tools", session=sid).get("canceled", 0)
    ev = wait_for(app, lambda e: e.get("event") == "doc_prover" and sh_cmd(e) and all(c[3] != "running" for c in sh_cmd(e)),
                  timeout=60, what="sledgehammer stopped")
    c = sh_cmd(ev)[0]
    msgs = [m["text"][:60] for m in ev["prover"]["messages"] if m["start"] >= c[0] and m["end"] <= c[1]]
    print("     stopped sledgehammer:", c[3], msgs)
    check(n >= 1, "stop reaches a running sledgehammer (%d executions)" % n)

    app.request("revoke", session=sid, grant=g["id"])
    try:
        doc = agent.request("read", session=sid, path=str(thy))["doc"]
        agent.request("edit", session=sid, path=str(thy), revision=doc["revision"],
                      edits=[{"start": 0, "end": 0, "text": "x"}])
        check(False, "revoked agent is refused")
    except H.HostError as e:
        check(e.code == "forbidden", "revoked agent is refused")

    app.request("stop", session=sid)
    print("FAILURES:", FAILURES)
    shutil.rmtree(folder, ignore_errors=True)
    return 1 if FAILURES else 0


if __name__ == "__main__":
    code = 1
    try:
        code = main()
    finally:
        with __import__("contextlib").suppress(Exception):
            c = H.SyncClient.local()
            c.request("hello", client="cli")
            c.request("shutdown")
    sys.exit(code)
