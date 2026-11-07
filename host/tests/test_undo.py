# Unit test of content-based undo without a prover: a host with a doc, vim-like edits
import asyncio, os, sys, tempfile
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
os.environ["DEDUCTUM_STATE_DIR"] = tempfile.mkdtemp()
import deductum_host as H

async def main():
    host = H.Host()
    folder = tempfile.mkdtemp(); path = os.path.join(folder, "T.thy")
    open(path, "w").write("lemma x: True\n  by simp\n")
    s = H.Session(host, "s1", {"folder": folder, "runtime": "/bin/true", "logic": "HOL", "lifecycle": "Stopped"})
    s.dir.mkdir(parents=True, exist_ok=True)
    class C: kind="app"; key=None; id="c"; label="t"
    def author(self): return H.HUMAN
    def note_position(self, *a): pass
    C.author = author; C.note_position = note_position
    c = C()
    doc = s.get_doc(path)
    base = doc.text
    # type "this" in txn A, then delete it char by char in txn B (vim x x x x)
    for i, ch in enumerate("this"):
        s.client_edit(c, path, doc.revision, [{"start": 9 + i, "end": 9 + i, "text": ch}], "merge", "A", None)
    for _ in range(4):
        s.client_edit(c, path, doc.revision, [{"start": 9, "end": 10, "text": ""}], "merge", "B", None)
    assert doc.text == base, doc.text
    s.undo(c, path); assert doc.text == base[:9] + "this" + base[9:], doc.text
    s.undo(c, path); assert doc.text == base, repr(doc.text)
    s.redo(c, path); assert doc.text == base[:9] + "this" + base[9:], doc.text
    assert not doc.alternatives, doc.alternatives
    print("content-based undo ok")

    # The human types a word; an agent then rewrites part of it: undo keeps the agent's
    # text, says so, and leaves no pieces to restore
    class A(C): kind = "agent"; key = "claude:x"; label = "Claude"
    def agent_author(self): return {"kind": "agent", "key": "claude:x", "label": "Claude"}
    A.author = agent_author
    s.agent_may_edit = lambda *a: True
    a = A()
    s.client_edit(c, path, doc.revision, [{"start": 9, "end": 9, "text": "hello "}], "merge", "W", None)
    s.client_edit(a, path, doc.revision, [{"start": 11, "end": 14, "text": "LLO"}], "strict", None, None)
    r = s.undo(c, path)
    assert "Claude" in r["note"] and r["at"] is not None, r
    assert "LLO" in doc.text and "he" not in doc.text[:12], repr(doc.text)
    assert not doc.alternatives
    print("undo keeps later agent edits ok:", r["note"])

asyncio.run(main())
