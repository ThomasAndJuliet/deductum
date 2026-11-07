"""Edit identities are durable, idempotent and tied to one immutable intent."""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
os.environ.setdefault("DEDUCTUM_STATE_DIR", tempfile.mkdtemp())
import deductum_host as H


class EditIdentityTest(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = os.path.join(self.tmp.name, "T.thy")
        with open(self.path, "w") as f:
            f.write("abc")
        self.host = H.Host()
        self.session = H.Session(self.host, "identity", {"folder": self.tmp.name, "runtime": "/bin/true", "logic": "HOL", "lifecycle": "Stopped"})
        self.session.dir.mkdir(parents=True, exist_ok=True)
        class Client:
            kind = "app"
            key = None
            id = "client"
            label = "test"
            def author(self): return H.HUMAN
            def note_position(self, *args): pass
            def attach(self, session): pass
            open_docs = set()
        self.client = Client()
        self.doc = self.session.get_doc(self.path)
        self.host.sessions[self.session.id] = self.session

    def edit(self, text="A", identity="unique", revision=0):
        return self.session.client_edit(self.client, self.path, revision,
            [{"start": 0, "end": 0, "text": text}], "merge", "group", 1, identity)

    async def test_retry_is_idempotent_and_reuse_for_other_intent_is_rejected(self):
        first = self.edit()
        self.assertEqual(self.doc.text, "Aabc")
        repeated = self.edit()
        self.assertTrue(repeated["duplicate"])
        self.assertEqual(repeated["entry"], first["entry"])
        self.assertEqual(self.doc.revision, 1)
        with self.assertRaises(H.HostError):
            self.edit(text="B")
        with self.assertRaises(H.HostError):
            self.session.client_edit(self.client, self.path, 0, [{"start": 0, "end": 0, "text": "A"}], "strict", "group", 1, "unique")
        self.assertEqual(self.doc.text, "Aabc")
        self.assertEqual(self.doc.entries[0]["edit_id"], "unique")
        self.assertEqual(self.doc.to_json()["entries"][0]["edit_intent"]["revision"], 0)

    async def test_read_returns_bounded_history_with_its_snapshot(self):
        self.edit()
        result = await self.host.handle(self.client, {"op": "read", "session": self.session.id, "path": self.path, "since_revision": 0})
        self.assertEqual(result["doc"]["text"], "Aabc")
        self.assertTrue(result["edit_history"]["complete"])
        self.assertEqual(result["edit_history"]["entries"][0]["edit_id"], "unique")
        self.doc.entries.clear()
        result = await self.host.handle(self.client, {"op": "read", "session": self.session.id, "path": self.path, "since_revision": 0})
        self.assertFalse(result["edit_history"]["complete"])

    async def test_oversized_history_is_not_returned(self):
        for i in range(129):
            self.session.client_edit(self.client, self.path, self.doc.revision,
                [{"start": 0, "end": 0, "text": "x"}], "merge", "g", i, f"id{i}")
        result = await self.host.handle(self.client, {"op": "read", "session": self.session.id, "path": self.path, "since_revision": 0})
        self.assertFalse(result["edit_history"]["complete"])
        self.assertEqual(result["edit_history"]["entries"], [])

    async def test_pruned_base_and_noop_do_not_mutate(self):
        self.edit()
        self.doc.entries.clear()
        with self.assertRaises(H.HostError) as error:
            self.edit(identity="other")
        self.assertEqual(error.exception.code, "stale_base")
        with self.assertRaises(H.HostError):
            self.session.client_edit(self.client, self.path, 1, [], "merge", "group", 2, "empty")
        self.assertEqual(self.doc.revision, 1)


if __name__ == "__main__":
    unittest.main()
