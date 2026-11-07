"""Recovery must use durable journal suffixes, without periodic checkpoints."""
import asyncio
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import deductum_host as H


class Client:
    kind, key, id, label = "app", None, "journal-client", "test"
    open_docs = set()
    def author(self): return H.HUMAN
    def note_position(self, *args): pass
    def attach(self, session): pass


class JournalRecovery(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.enterContext(patch.object(H, "STATE_DIR", self.root / "state"))
        self.enterContext(patch.object(H.drivers, "read_config", return_value={}))
        self.host = H.Host()
        self.host.enable_background_io()
        self.meta = {"folder": self.tmp.name, "backend": "lean", "toolchain": "unused", "lifecycle": "Stopped"}
        self.session = H.Session(self.host, "recovery", copy.deepcopy(self.meta))
        self.host.sessions[self.session.id] = self.session
        self.path = self.root / "Å 😀.lean"
        self.path.write_bytes(b"abc\r\n")
        self.doc = self.session.get_doc(str(self.path))
        self.client = Client()
        self.cancel_timers(self.session)

    def cancel_timers(self, session):
        if session.persist_handle:
            session.persist_handle.cancel()
            session.persist_handle = None
        for doc in session.docs.values():
            if doc.save_handle:
                doc.save_handle.cancel()
                doc.save_handle = None

    async def asyncTearDown(self):
        self.host.stopping.set()
        self.cancel_timers(self.session)
        for broker in (self.host.state_io, self.host.project_io, self.host.sample_io):
            try:
                await broker.flush()
            finally:
                broker.close()
        self.tmp.cleanup()

    def edit(self, text, edit_id, txn=None):
        result = self.session.client_edit(self.client, str(self.path), self.doc.revision,
            [{"start": 0, "end": 0, "text": text}], "merge", txn or edit_id, edit_id, edit_id)
        self.cancel_timers(self.session)
        return result

    async def durable(self):
        await self.host.state_io.flush()

    def recover(self):
        session = H.Session(self.host, "recovery", copy.deepcopy(self.meta))
        session.load_docs()
        return session, session.docs[str(self.path)]

    async def test_first_journaled_edit_establishes_its_recoverable_baseline(self):
        self.assertFalse(self.doc.state_path().exists(), "fixture already has a periodic checkpoint")
        receipt = self.edit("∀😀\n", "first")
        await self.durable()
        self.assertEqual(self.path.read_bytes(), b"abc\r\n", "fixture persisted the edit to the document")
        recovered, doc = self.recover()
        self.assertEqual((doc.text, doc.revision), ("∀😀\nabc\n", 1))
        duplicate = recovered.client_edit(self.client, str(self.path), 0,
            [{"start": 0, "end": 0, "text": "∀😀\n"}], "merge", "first", "first", "first")
        self.assertTrue(duplicate["duplicate"])
        self.assertEqual(duplicate["entry"], receipt["entry"])
        self.assertEqual(doc.revision, 1)

    async def test_newer_journal_suffix_replays_once_over_an_older_checkpoint(self):
        self.edit("A", "first")
        await self.durable()
        self.session.persist_now()
        await self.durable()
        checkpoint = self.doc.state_path().read_bytes()
        self.edit("B", "second")
        self.edit("C", "third")
        await self.durable()
        self.assertEqual(self.doc.state_path().read_bytes(), checkpoint, "periodic checkpoint escaped fixture barrier")
        _, doc = self.recover()
        self.assertEqual((doc.text, doc.revision), ("CBAabc\n", 3))
        self.assertEqual([entry["edit_id"] for entry in doc.entries], ["first", "second", "third"])
        _, again = self.recover()
        self.assertEqual((again.text, again.revision), ("CBAabc\n", 3))
        self.assertEqual(len(again.entries), 3)

    async def test_undo_redo_and_edit_receipts_remain_usable_after_recovery(self):
        self.edit("A", "first")
        self.edit("B", "second")
        self.session.undo(self.client, str(self.path))
        self.cancel_timers(self.session)
        await self.durable()
        recovered, doc = self.recover()
        self.assertEqual(doc.text, "Aabc\n")
        self.assertEqual(doc.revision, 3)
        result = recovered.redo(self.client, str(self.path))
        self.cancel_timers(recovered)
        await self.durable()
        self.assertTrue(result["redone"], "recovery lost the redo transaction")
        self.assertEqual(doc.text, "BAabc\n")
        self.assertEqual(doc.revision, 4)
        _, after_redo = self.recover()
        self.assertEqual((after_redo.text, after_redo.revision), ("BAabc\n", 4))
        self.assertTrue(after_redo.undo_stack)
        self.assertFalse(after_redo.redo_stack)

    async def test_corrupt_or_noncontiguous_journal_is_reported_instead_of_loading_disk_text(self):
        self.edit("A", "first")
        await self.durable()
        original = self.doc.log_path().read_bytes()
        record = json.loads(original)
        cases = [b"{truncated", json.dumps({**record, "rev": 99}).encode(),
                 json.dumps({**record, "inverse": []}).encode(),
                 json.dumps({**record, "base_sha256": "0" * 64}).encode(),
                 json.dumps({**record, "recovery": {**record["recovery"], "text_sha256": "0" * 64}}).encode()]
        try:
            for index, raw in enumerate(cases):
                with self.subTest(case=index):
                    self.doc.log_path().write_bytes(raw + b"\n")
                    with self.assertRaises(H.HostError) as caught:
                        self.recover()
                    self.assertEqual(caught.exception.code, "io_error")
                    self.assertIn("preserve", str(caught.exception))
                    self.assertEqual(self.path.read_bytes(), b"abc\r\n", "failed recovery changed the original file")
        finally:
            self.doc.log_path().write_bytes(original)

    async def test_an_orphaned_journal_cannot_silently_reopen_the_disk_file(self):
        self.edit("A", "first")
        await self.durable()
        self.doc.state_path().unlink()
        with self.assertRaisesRegex(H.HostError, "without document checkpoints") as caught:
            self.recover()
        self.assertEqual(caught.exception.code, "io_error")
        self.assertEqual(self.path.read_bytes(), b"abc\r\n")


if __name__ == "__main__":
    unittest.main()
