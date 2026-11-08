"""Deterministic blocked-I/O scenarios; no real provers or production state."""
import asyncio
import builtins
import json
import os
import sys
import tempfile
import threading
import subprocess
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, patch

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
import deductum_host as H
from background import OrderedIO, Busy
from test_responsiveness import Backend, Writer


class OrderedWork(unittest.IsolatedAsyncioTestCase):
    async def test_flush_allows_completed_task_removal_callbacks_to_run(self):
        # A completed operation may still be in the broker's pending set until
        # its scheduled callback executes. Exercise flush in that exact window.
        code = '''
import asyncio, sys
sys.path.insert(0, sys.argv[1])
from background import OrderedIO
async def check():
    io = OrderedIO("flush-callback", workers=1)
    loop = asyncio.get_running_loop()
    completed = loop.create_future()
    completed.set_result("persisted")
    io.tasks.add(completed)
    loop.call_soon(io.tasks.discard, completed)
    try:
        await io.flush()
        assert not io.tasks, "flush returned before callbacks drained"
    finally:
        io.close()
asyncio.run(check())
'''
        # The broken implementation blocks the event loop itself, so its
        # watchdog must live outside that loop.
        result = await asyncio.to_thread(subprocess.run, [sys.executable, "-c", code, str(Path(H.__file__).parent)],
                                         capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)

    async def test_deadline_keeps_write_order_and_other_mount_progress(self):
        io = OrderedIO("test", workers=2)
        entered, release = asyncio.Event(), threading.Event()
        loop = asyncio.get_running_loop()
        written = []
        def first():
            loop.call_soon_threadsafe(entered.set)
            release.wait(5)
            written.append("old")
        try:
            one = io.submit("remote", first)
            two = io.submit("remote", lambda: written.append("new"))
            await asyncio.wait_for(entered.wait(), 1)
            with self.assertRaises(asyncio.TimeoutError):
                await io.wait(one, "remote", .01)
            self.assertFalse(one.cancelled())
            with self.assertRaises(Busy):
                io.submit("remote", lambda: None)
            self.assertEqual(await asyncio.wait_for(io.submit("local", lambda: "responsive"), 1), "responsive")
            self.assertEqual(written, [])
            release.set()
            await asyncio.wait_for(io.flush(), 1)
            self.assertTrue(two.done())
            self.assertEqual(written, ["old", "new"])
        finally:
            release.set()
            io.close()

    async def test_flush_waits_for_other_lanes_after_a_failure(self):
        io = OrderedIO("test", workers=2)
        release = threading.Event()
        def fail():
            raise OSError("journal unavailable")
        try:
            io.submit("bad", fail, strict=True)
            good = io.submit("good", lambda: release.wait(5))
            flushing = asyncio.create_task(io.flush())
            await asyncio.sleep(.02)
            self.assertFalse(flushing.done())
            release.set()
            with self.assertRaises(OSError):
                await flushing
            self.assertTrue(good.done())
            with self.assertRaises(OSError):
                io.submit("bad", lambda: None, strict=True)
        finally:
            release.set()
            io.close()


class BackgroundHost(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.patches = [patch.object(H, "STATE_DIR", Path(self.tmp.name) / "state"),
                        patch.object(H, "find_runtimes", return_value=[]),
                        patch.object(H.drivers, "read_config", return_value={})]
        for p in self.patches:
            p.start()
        self.host = H.Host()
        self.host.backends_info = lambda *args: []
        self.host.enable_background_io()
        self.sessions, self.docs, self.backends = [], [], []
        self.connections, self.gates = [], []
        for sid in ("one", "two"):
            session = H.Session(self.host, sid, {"folder": self.tmp.name, "backend": "isabelle",
                                "toolchain": "/unused", "lifecycle": "Running", "epoch": 3})
            self.host.sessions[sid] = session
            backend = Backend()
            backend.capabilities = lambda: {}
            backend.stop = AsyncMock()
            session.driver = backend
            path = Path(self.tmp.name) / (sid + ".thy")
            path.write_text("abc")
            doc = H.Doc(session, str(path))
            doc.load_from_disk()
            doc.opened = doc._sent_open = True
            doc._prover_text = doc.text
            session.docs[doc.path] = doc
            self.sessions.append(session)
            self.docs.append(doc)
            self.backends.append(backend)

    async def asyncTearDown(self):
        self.host.stopping.set()
        for gate in self.gates:
            gate.set()
        for reader, writer, serving in self.connections:
            serving.cancel()
        await asyncio.gather(*(t for _, _, t in self.connections), return_exceptions=True)
        for broker in (self.host.project_io, self.host.state_io, self.host.sample_io):
            try:
                await asyncio.wait_for(broker.flush(), 2)
            except Exception:
                pass
            broker.close()
        jobs = [j for s in self.sessions for j in [s._watch_job, *s._save_jobs.values()] if j]
        if self.host.sample_job:
            jobs.append(self.host.sample_job)
        for job in jobs:
            job.cancel()
        await asyncio.gather(*jobs, return_exceptions=True)
        for session in self.sessions:
            for handle in [session.persist_handle, session.status_handle, *(d.save_handle for d in session.docs.values())]:
                if handle:
                    handle.cancel()
        for p in reversed(self.patches):
            p.stop()
        self.tmp.cleanup()

    async def connect(self, kind="cli"):
        reader, writer = asyncio.StreamReader(), Writer()
        serving = asyncio.create_task(self.host.serve_client(reader, writer))
        self.connections.append((reader, writer, serving))
        self.send(reader, 0, "hello", client=kind, concurrent_reads=True)
        self.assertTrue((await writer.reply(0))["ok"])
        return reader, writer

    def send(self, reader, rid, op, session=0, **args):
        reader.feed_data((json.dumps({"id": rid, "op": op, "session": self.sessions[session].id,
                                      "path": self.docs[session].path, **args}) + "\n").encode())

    def blocked(self):
        entered, release = asyncio.Event(), threading.Event()
        self.gates.append(release)
        loop = asyncio.get_running_loop()
        def wait():
            loop.call_soon_threadsafe(entered.set)
            if not release.wait(5):
                raise TimeoutError("test worker was not released")
        return entered, release, wait

    async def test_blocked_project_journal_and_socket_allow_other_edit_and_stop(self):
        reader, writer = await self.connect()
        other, output = await self.connect()
        journal_entered, journal_release, block_journal = self.blocked()
        disk_entered, disk_release, block_disk = self.blocked()
        slow_entered, slow_release = asyncio.Event(), asyncio.Event()
        original_open, original_write = builtins.open, H.write_file_atomic
        def opened(path, mode="r", *args, **kwargs):
            if str(path) == str(self.docs[0].log_path()) and mode == "a":
                block_journal()
            return original_open(path, mode, *args, **kwargs)
        def written(path, data):
            if path == self.docs[0].path:
                block_disk()
            return original_write(path, data)
        async def slow():
            slow_entered.set()
            await slow_release.wait()
        writer.drain = slow
        with patch.object(H, "open", opened, create=True), patch.object(H, "write_file_atomic", written):
            self.send(reader, 1, "edit", revision=0, edits=[{"start": 3, "end": 3, "text": "d"}])
            await asyncio.wait_for(journal_entered.wait(), 1)
            self.assertEqual(self.docs[0].text, "abcd")
            # A status starts a deliberately stalled socket drain.
            self.host.clients[0].push({"event": "activity", "session": "one", "activity": {}})
            await asyncio.wait_for(slow_entered.wait(), 1)
            self.send(reader, 2, "cancel_tools")
            self.send(reader, 3, "stop")
            await asyncio.wait_for(disk_entered.wait(), 1)
            self.assertEqual(self.sessions[0].lifecycle, "Stopped")
            self.backends[0].stop.assert_awaited_once()
            self.assertTrue(any(c["op"] == "cancel_tools" for c in self.backends[0].calls))
            self.send(other, 4, "edit", session=1, revision=0, edits=[{"start": 3, "end": 3, "text": "e"}])
            self.assertTrue((await output.reply(4))["ok"])
            self.assertEqual(self.docs[1].text, "abce")
            slow_release.set()
            self.assertTrue((await writer.reply(3))["ok"])
            self.assertFalse(any(m.get("id") == 1 for m in writer.messages))
            journal_release.set()
            self.assertTrue((await writer.reply(1))["ok"])
            disk_release.set()
            await asyncio.wait_for(asyncio.gather(*self.sessions[0]._save_jobs.values()), 1)
            self.assertEqual(Path(self.docs[0].path).read_text(), "abcd")

    async def test_recovery_read_and_duplicate_wait_for_original_journal(self):
        for fail in (False, True):
            reader, writer = await self.connect("app")
            other, output = await self.connect("app")
            entered, release, block = self.blocked()
            original = builtins.open
            def opened(path, mode="r", *args, **kwargs):
                if str(path) == str(self.docs[0].log_path()) and mode == "a":
                    block()
                    if fail:
                        raise OSError("journal failed")
                return original(path, mode, *args, **kwargs)
            rev = self.docs[0].revision
            identity = "failure" if fail else "success"
            edit = {"revision": rev, "edits": [{"start": 0, "end": 0, "text": identity}], "txn": identity, "edit_id": identity}
            with patch.object(H, "open", opened, create=True):
                self.send(reader, 10, "edit", **edit)
                await asyncio.wait_for(entered.wait(), 1)
                self.send(other, 11, "read", since_revision=rev, receipt_ids=[identity])
                self.send(other, 12, "edit", **edit)
                self.send(other, 13, "cancel_tools")
                await asyncio.wait_for(output.reply(13), 1)
                self.assertFalse(any(m.get("id") in (11, 12) for m in output.messages))
                release.set()
                read, duplicate = await output.reply(11), await output.reply(12)
                self.assertEqual(read["ok"], not fail)
                self.assertEqual(duplicate["ok"], not fail)
                if not fail:
                    self.assertTrue(duplicate["duplicate"])
                    self.assertEqual(read["edit_receipts"], [identity])
                    self.assertEqual(read["edit_history"]["entries"][0]["edit_id"], identity)
                else:
                    self.assertEqual(read["code"], "io_error")
                    self.assertEqual(duplicate["code"], "io_error")

    async def test_queue_overflow_precedes_authoritative_edit(self):
        self.host.state_io.limit = 0
        doc = self.docs[0]
        with self.assertRaises(H.HostError) as caught:
            self.sessions[0].commit(doc, H.HUMAN, "edit", [(3, 3, "d")])
        self.assertEqual(caught.exception.code, "busy")
        self.assertEqual((doc.text, doc.revision, doc.entries, doc.next_entry), ("abc", 0, [], 1))

    async def test_late_journal_failure_errors_ack_and_prevents_a_gap(self):
        reader, writer = await self.connect()
        entered, release, block = self.blocked()
        original = builtins.open
        def opened(path, mode="r", *args, **kwargs):
            if str(path) == str(self.docs[0].log_path()) and mode == "a":
                block()
                raise OSError("simulated unavailable journal")
            return original(path, mode, *args, **kwargs)
        with patch.object(H, "open", opened, create=True):
            self.send(reader, 1, "edit", revision=0, edits=[{"start": 3, "end": 3, "text": "d"}])
            await asyncio.wait_for(entered.wait(), 1)
            self.send(reader, 2, "ping")
            self.assertTrue((await writer.reply(2))["ok"])
            self.assertFalse(any(m.get("id") == 1 for m in writer.messages))
            release.set()
            reply = await writer.reply(1)
            self.assertFalse(reply["ok"])
            self.assertEqual(reply["code"], "io_error")
            self.send(reader, 3, "edit", revision=1, edits=[{"start": 4, "end": 4, "text": "e"}])
            self.assertEqual((await writer.reply(3))["code"], "io_error")
            self.assertEqual((self.docs[0].text, self.docs[0].revision), ("abcd", 1))

    async def test_late_save_leaves_newer_edit_dirty_then_saves_it(self):
        session, doc = self.sessions[0], self.docs[0]
        session.commit(doc, H.HUMAN, "edit", [(3, 3, "d")])
        entered, release, block = self.blocked()
        original = H.write_file_atomic
        def write(path, data):
            block()
            return original(path, data)
        with patch.object(H, "write_file_atomic", write):
            self.assertTrue(session.save_doc(doc))
            await asyncio.wait_for(entered.wait(), 1)
            session.commit(doc, H.HUMAN, "edit", [(4, 4, "e")])
            release.set()
            await asyncio.wait_for(asyncio.gather(*session._save_jobs.values()), 1)
        self.assertEqual((doc.text, doc.disk_text, doc.saved_revision, doc.dirty), ("abcde", "abcd", 1, True))
        self.assertTrue(session.save_doc(doc))
        await asyncio.wait_for(asyncio.gather(*session._save_jobs.values()), 1)
        self.assertEqual((Path(doc.path).read_text(), doc.saved_revision, doc.dirty), ("abcde", 2, False))

    async def test_external_change_is_detected_by_save_lane(self):
        session, doc = self.sessions[0], self.docs[0]
        session.commit(doc, H.HUMAN, "edit", [(3, 3, "mine")])
        Path(doc.path).write_text("external change")
        session.save_doc(doc)
        await asyncio.wait_for(asyncio.gather(*session._save_jobs.values()), 1)
        self.assertEqual(Path(doc.path).read_text(), "external change")
        self.assertEqual(doc.conflict["disk_text"], "external change")
        self.assertEqual(doc.text, "abcmine")

    async def test_explicit_save_waits_for_revision_after_an_inflight_save(self):
        reader, writer = await self.connect()
        session, doc = self.sessions[0], self.docs[0]
        session.commit(doc, H.HUMAN, "edit", [(3, 3, "d")])
        entered, release, block = self.blocked()
        original = H.write_file_atomic
        def write(path, data):
            block()
            return original(path, data)
        with patch.object(H, "write_file_atomic", write):
            session.save_doc(doc)
            await asyncio.wait_for(entered.wait(), 1)
            session.commit(doc, H.HUMAN, "edit", [(4, 4, "e")])
            self.send(reader, 1, "save")
            release.set()
            reply = await writer.reply(1)
        self.assertEqual(reply["failed"], [])
        self.assertEqual((Path(doc.path).read_text(), doc.saved_revision, doc.dirty), ("abcde", 2, False))

    async def test_shutdown_save_fence_and_full_recovery_queue_retain_latest(self):
        session, doc = self.sessions[0], self.docs[0]
        session.commit(doc, H.HUMAN, "edit", [(3, 3, "d")])
        entered, release, block = self.blocked()
        original = H.write_file_atomic
        def write(path, data):
            block()
            return original(path, data)
        with patch.object(H, "write_file_atomic", write):
            session.save_doc(doc)
            await asyncio.wait_for(entered.wait(), 1)
            session.commit(doc, H.HUMAN, "edit", [(4, 4, "e")])
            # Shutdown prevents the ordinary timer from rescheduling a newer save.
            self.host.stopping.set()
            saving = asyncio.create_task(session.save_through(doc))
            release.set()
            self.assertTrue(await asyncio.wait_for(saving, 1))
        await self.host.state_io.flush()
        # More sessions than queue slots must be flushed in bounded batches.
        self.host.state_io.limit = 1
        await asyncio.wait_for(self.host.flush_recovery(), 1)
        recovered = H.read_json(doc.state_path())
        self.assertEqual((Path(doc.path).read_text(), recovered["text"], recovered["saved_revision"]),
                         ("abcde", "abcde", 2))
        self.assertTrue(all((s.dir / "meta.json").exists() for s in self.sessions))

    async def test_watch_and_process_sampling_do_not_block_edits(self):
        reader, writer = await self.connect()
        entered, release, block = self.blocked()
        original = H.list_documents
        def scan(*args, **kwargs):
            block()
            return original(*args, **kwargs)
        with patch.object(H, "list_documents", scan):
            self.sessions[0].watch()
            await asyncio.wait_for(entered.wait(), 1)
            self.send(reader, 1, "edit", session=1, revision=0, edits=[{"start": 3, "end": 3, "text": "d"}])
            self.assertTrue((await writer.reply(1))["ok"])
            release.set()
            await asyncio.wait_for(self.sessions[0]._watch_job, 1)
        from types import SimpleNamespace
        self.backends[0].proc = SimpleNamespace(pid=123, returncode=None)
        sampled, sample_release, block_sample = self.blocked()
        def sample(pid):
            block_sample()
            return 100, 200
        with patch.object(H.Session, "process_group_usage", staticmethod(sample)):
            self.host.sample_resources()
            await asyncio.wait_for(sampled.wait(), 1)
            self.assertIsNone(self.sessions[0].activity()["prover"])
            self.send(reader, 2, "ping")
            self.assertTrue((await writer.reply(2))["ok"])
            sample_release.set()
            await asyncio.wait_for(self.host.sample_job, 1)
        self.assertEqual(self.sessions[0].group_usage(), (100, 200))

    async def test_superseded_gui_reads_reply_to_every_request_id(self):
        reader, writer = await self.connect("app")
        self.send(reader, 1, "state", offset=1, revision=0)
        await self.backends[0].entered(1)
        self.send(reader, 2, "state", offset=2, revision=0)
        await self.backends[0].entered(2)
        reply = await writer.reply(1)
        self.assertFalse(reply["ok"])
        self.assertEqual(reply["code"], "stale_snapshot")
        self.assertEqual(self.backends[0].canceled, ["state"])
        self.backends[0].release.set()
        self.assertTrue((await writer.reply(2))["ok"])
        self.assertEqual(len([m for m in writer.messages if m.get("id") == 1]), 1)

    async def test_configured_job_and_memory_reservations_cover_starting_provers(self):
        self.host.resource_policy = {"max_running_provers": 2}
        self.sessions[0].meta["lifecycle"] = "Starting"
        with self.assertRaises(H.HostError) as error:
            self.host.check_memory_budget()
        self.assertEqual(error.exception.code, "prover_limit")
        self.host.resource_policy = {"memory_budget_gb": 1, "prover_start_reserve_mb": 400}
        with self.assertRaises(H.HostError) as error:
            self.host.check_memory_budget()
        self.assertEqual(error.exception.code, "memory_budget")
        self.sessions[1].meta["lifecycle"] = "Stopped"
        self.host.check_memory_budget()
        self.host.resource_policy = {}
        self.host.check_memory_budget()


if __name__ == "__main__":
    unittest.main()
