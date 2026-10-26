"""Connection dispatch with delayed prover reads, without a prover or live host."""
import asyncio
import json
import os
import socket
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
import deductum_host as H


class Writer:
    def __init__(self):
        self.messages = []
        self.changed = asyncio.Event()
        self.closed = asyncio.get_running_loop().create_future()
        self.drains = 0

    def write(self, data):
        self.messages.append(json.loads(data))
        self.changed.set()

    async def drain(self):
        self.drains += 1

    async def wait_closed(self):
        await self.closed

    def close(self):
        if not self.closed.done():
            self.closed.set_result(None)

    def disconnect(self):
        self.closed.set_exception(ConnectionResetError("peer disconnected"))

    async def reply(self, rid):
        async def wait():
            while True:
                self.changed.clear()
                for message in self.messages:
                    if message.get("id") == rid:
                        return message
                await self.changed.wait()
        return await asyncio.wait_for(wait(), 1)


class Backend:
    ready = True
    proc = None

    def __init__(self):
        self.calls = []
        self.release = asyncio.Event()
        self.changed = asyncio.Event()
        self.canceled = []
        self.failure = None
        self.result = {"messages": [], "info": {"tooltip": None}}

    def send(self, obj):
        self.calls.append(obj)
        self.changed.set()

    async def request(self, obj):
        self.send(obj)
        if obj["op"] in ("state", "info"):
            try:
                await self.release.wait()
            except asyncio.CancelledError:
                self.canceled.append(obj["op"])
                raise
            if self.failure:
                raise self.failure
        return dict(self.result)

    async def entered(self, count):
        async def wait():
            while len([c for c in self.calls if c["op"] in ("state", "info")]) < count:
                self.changed.clear()
                await self.changed.wait()
        await asyncio.wait_for(wait(), 1)


class Responsiveness(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.state = patch.object(H, "STATE_DIR", Path(self.tmp.name) / "state")
        self.state.start()
        self.runtimes = patch.object(H, "find_runtimes", return_value=[])
        self.runtimes.start()
        self.host = H.Host()
        self.host.backends_info = lambda *args: []
        self.broadcasts = []
        self.host.broadcast_session = lambda s: self.broadcasts.append(s.id)
        self.session = H.Session(self.host, "s", {
            "folder": self.tmp.name, "backend": "isabelle", "toolchain": "/unused",
            "lifecycle": "Running", "epoch": 3,
        })
        self.backend = Backend()
        self.session.driver = self.backend
        self.host.sessions["s"] = self.session
        self.path = str(Path(self.tmp.name) / "T.thy")
        self.doc = H.Doc(self.session, self.path)
        self.doc.text = "abc"
        self.doc.opened = True
        self.doc._sent_open = True
        self.doc._prover_text = self.doc.text
        self.session.docs[self.path] = self.doc
        self.saves = 0
        def save_all():
            self.saves += 1
        self.session.save_all = save_all
        self.reader = asyncio.StreamReader()
        self.writer = Writer()
        self.serving = asyncio.create_task(self.host.serve_client(self.reader, self.writer))
        self.request(0, "hello", client="cli", concurrent_reads=True)
        self.assertTrue((await self.writer.reply(0))["ok"])

    async def asyncTearDown(self):
        self.serving.cancel()
        await asyncio.gather(self.serving, return_exceptions=True)
        for handle in (self.session.persist_handle, self.session.status_handle, self.doc.save_handle):
            if handle:
                handle.cancel()
        self.runtimes.stop()
        self.state.stop()
        self.tmp.cleanup()

    def request(self, rid, op, **params):
        req = {"id": rid, "op": op, "session": "s", "path": self.path, **params}
        self.reader.feed_data((json.dumps(req) + "\n").encode())

    async def test_slow_output_does_not_hold_edits_or_cancellation(self):
        blocked, release = asyncio.Event(), asyncio.Event()
        async def drain():
            blocked.set()
            await release.wait()
        self.writer.drain = drain
        self.host.clients[0].push({"event": "activity", "session": "s", "activity": {}})
        await asyncio.wait_for(blocked.wait(), 1)
        self.request(1, "edit", revision=0, edits=[{"start": 3, "end": 3, "text": "d"}])
        self.request(2, "cancel_tools")
        async def dispatched():
            while not any(c["op"] == "cancel_tools" for c in self.backend.calls):
                self.backend.changed.clear()
                await self.backend.changed.wait()
        await asyncio.wait_for(dispatched(), 1)
        self.assertEqual(self.doc.text, "abcd")
        self.assertNotIn(2, [m.get("id") for m in self.writer.messages])
        release.set()
        self.assertTrue((await self.writer.reply(2))["ok"])
        self.assertTrue((await self.writer.reply(1))["ok"])

    async def test_snapshot_coalescing_does_not_cross_an_edit_fence(self):
        blocked, release = asyncio.Event(), asyncio.Event()
        async def drain():
            blocked.set()
            await release.wait()
        self.writer.drain = drain
        client = self.host.clients[0]
        client.push({"event": "activity", "session": "s", "value": 0})
        await asyncio.wait_for(blocked.wait(), 1)
        for value in [1, 2]:
            client.push({"event": "activity", "session": "s", "value": value})
        client.push({"event": "doc_changed", "session": "s", "revision": 1})
        for value in [3, 4]:
            client.push({"event": "activity", "session": "s", "value": value})
        release.set()
        await client.finish_writer()
        messages = [m for m in self.writer.messages if m.get("event") in ("activity", "doc_changed")]
        self.assertEqual([m.get("value", "edit") for m in messages], [0, 2, "edit", 4])

    async def test_lossless_output_overflow_disconnects_instead_of_dropping_edits(self):
        blocked, release = asyncio.Event(), asyncio.Event()
        async def drain():
            blocked.set()
            await release.wait()
        self.writer.drain = drain
        client = self.host.clients[0]
        client.push({"event": "activity", "session": "s"})
        await asyncio.wait_for(blocked.wait(), 1)
        with patch.object(H, "CLIENT_OUTPUT_MESSAGES", 2):
            for revision in range(3):
                client.push({"event": "doc_changed", "session": "s", "revision": revision})
        await asyncio.wait_for(self.serving, 1)
        self.assertTrue(client.closed)
        self.assertTrue(self.writer.closed.done())

    async def test_legacy_hello_keeps_reads_and_mutations_in_request_order(self):
        # A legacy GUI's hello has no opt-in. Its existing navigation assumptions
        # require the state reply before the following edit is applied.
        self.request(10, "hello", client="app")
        self.assertTrue((await self.writer.reply(10))["ok"])
        self.assertFalse(self.host.clients[0].concurrent_reads)
        self.request(1, "state", offset=1, revision=0)
        await self.backend.entered(1)
        self.request(2, "edit", revision=0, edits=[{"start": 3, "end": 3, "text": "d"}])
        self.request(3, "cancel_tools")
        await asyncio.sleep(0)
        self.assertEqual(self.doc.text, "abc")
        self.assertNotIn(2, [m.get("id") for m in self.writer.messages])
        self.assertNotIn("cancel_tools", [c["op"] for c in self.backend.calls])
        self.backend.release.set()
        self.assertTrue((await self.writer.reply(3))["ok"])
        self.assertTrue((await self.writer.reply(1))["ok"])
        self.assertTrue((await self.writer.reply(2))["ok"])
        replies = [m["id"] for m in self.writer.messages if m.get("id") in (1, 2, 3)]
        self.assertEqual(replies, [1, 2, 3])
        self.assertEqual(self.doc.text, "abcd")

    async def test_pending_state_does_not_block_fifo_edits_or_cancel(self):
        self.request(1, "state", offset=1, revision=0)
        await self.backend.entered(1)
        self.request(2, "edit", revision=0, edits=[{"start": 3, "end": 3, "text": "d"}])
        self.request(3, "edit", revision=1, edits=[{"start": 4, "end": 4, "text": "e"}])
        self.request(4, "cancel_tools")
        self.assertTrue((await self.writer.reply(4))["ok"])
        self.assertTrue((await self.writer.reply(2))["ok"])
        self.assertTrue((await self.writer.reply(3))["ok"])
        self.assertEqual(self.doc.text, "abcde")
        self.assertNotIn(1, [m.get("id") for m in self.writer.messages])
        self.backend.release.set()
        state = await self.writer.reply(1)
        self.assertEqual(state["code"], "stale_snapshot")
        self.assertEqual(state["revision"], 0)

    async def test_info_does_not_block_control_and_retains_captured_revision(self):
        self.request(1, "info", offset=1, revision=0)
        await self.backend.entered(1)
        self.request(2, "cancel_tools")
        self.assertTrue((await self.writer.reply(2))["ok"])
        self.backend.release.set()
        result = await self.writer.reply(1)
        self.assertTrue(result["ok"])
        self.assertEqual(result["revision"], 0)
        self.assertEqual(result["info"], {"tooltip": None})

    async def test_snapshot_bound_rejects_excess_without_blocking_next_request(self):
        # More reads than the bound must not accumulate semaphore waiters or block control.
        for rid in range(1, 9):
            self.request(rid, "state" if rid % 2 else "info", offset=1)
        self.request(9, "cancel_tools")
        self.assertTrue((await self.writer.reply(9))["ok"])
        await self.backend.entered(4)
        calls = [c for c in self.backend.calls if c["op"] in ("state", "info")]
        self.assertEqual(len(calls), 4)
        for rid in range(5, 9):
            self.assertEqual((await self.writer.reply(rid))["code"], "busy")
        self.backend.release.set()
        for rid in range(1, 5):
            self.assertTrue((await self.writer.reply(rid))["ok"])
        self.request(10, "state")
        self.assertTrue((await self.writer.reply(10))["ok"], "finished reads release capacity")

    async def test_read_eof_keeps_pending_replies_for_half_closed_clients(self):
        self.request(1, "state", offset=1)
        await self.backend.entered(1)
        self.reader.feed_eof()
        await asyncio.sleep(0)
        self.assertFalse(self.serving.done(), "read EOF is not proof that the peer cannot receive")
        self.backend.release.set()
        self.assertTrue((await self.writer.reply(1))["ok"])
        await asyncio.wait_for(self.serving, 1)
        self.assertEqual(self.host.clients, [])
        self.assertTrue(self.writer.closed.done())
        self.assertGreater(self.writer.drains, 0)

    async def test_real_socket_half_close_delivers_the_pending_reply(self):
        self.serving.cancel()
        await asyncio.gather(self.serving, return_exceptions=True)
        server_socket, client_socket = socket.socketpair()
        server_reader, server_writer = await asyncio.open_connection(sock=server_socket)
        client_reader, client_writer = await asyncio.open_connection(sock=client_socket)
        self.serving = asyncio.create_task(self.host.serve_client(server_reader, server_writer))
        try:
            hello = {"id": 0, "op": "hello", "concurrent_reads": True}
            client_writer.write((json.dumps(hello) + "\n").encode())
            await client_writer.drain()
            self.assertTrue(json.loads(await asyncio.wait_for(client_reader.readline(), 1))["ok"])
            req = {"id": 1, "op": "state", "session": "s", "path": self.path}
            client_writer.write((json.dumps(req) + "\n").encode())
            await client_writer.drain()
            await self.backend.entered(1)
            client_writer.write_eof()
            self.backend.release.set()
            reply = json.loads(await asyncio.wait_for(client_reader.readline(), 1))
            self.assertEqual(reply["id"], 1)
            self.assertTrue(reply["ok"])
            await asyncio.wait_for(self.serving, 1)
            self.assertEqual(await asyncio.wait_for(client_reader.read(), 1), b"")
        finally:
            client_writer.close()
            await client_writer.wait_closed()
            server_writer.close()

    async def test_transport_loss_cancels_reads_and_detaches_client(self):
        self.request(1, "state")
        self.request(2, "info")
        await self.backend.entered(2)
        self.host.clients[0].kind = "app"
        self.writer.disconnect()
        await asyncio.wait_for(self.serving, 1)
        self.assertCountEqual(self.backend.canceled, ["state", "info"])
        self.assertEqual(self.host.clients, [])
        self.assertEqual(self.saves, 1)
        self.assertNotIn(1, [m.get("id") for m in self.writer.messages])

    async def test_canceling_connection_cleans_up_all_snapshot_tasks(self):
        self.request(1, "state")
        self.request(2, "info")
        await self.backend.entered(2)
        self.serving.cancel()
        await asyncio.gather(self.serving, return_exceptions=True)
        self.assertCountEqual(self.backend.canceled, ["state", "info"])
        self.assertEqual(self.host.clients, [])

    async def test_changed_epoch_rejects_reply_from_previous_prover(self):
        self.request(1, "state")
        await self.backend.entered(1)
        self.session.meta["epoch"] += 1
        self.backend.release.set()
        self.assertEqual((await self.writer.reply(1))["code"], "stale_snapshot")

    async def test_replaced_driver_rejects_old_info(self):
        self.request(1, "info")
        await self.backend.entered(1)
        self.session.driver = Backend()
        self.backend.release.set()
        self.assertEqual((await self.writer.reply(1))["code"], "stale_snapshot")

    async def test_replaced_session_rejects_old_state(self):
        self.request(1, "state")
        await self.backend.entered(1)
        self.host.sessions.pop("s")
        self.backend.release.set()
        self.assertEqual((await self.writer.reply(1))["code"], "stale_snapshot")

    async def test_replaced_document_rejects_old_offsets(self):
        self.request(1, "info")
        await self.backend.entered(1)
        self.session.docs[self.path] = H.Doc(self.session, self.path)
        self.backend.release.set()
        self.assertEqual((await self.writer.reply(1))["code"], "stale_snapshot")

    async def test_pausing_session_rejects_pending_snapshot(self):
        self.request(1, "state")
        await self.backend.entered(1)
        self.session.meta["lifecycle"] = "Paused"
        self.backend.release.set()
        self.assertEqual((await self.writer.reply(1))["code"], "stale_snapshot")

    async def test_info_rejects_changed_definition_target_with_unchanged_source(self):
        target_path = str(Path(self.tmp.name) / "Other.thy")
        target = H.Doc(self.session, target_path)
        target.text = "one\ntwo"
        self.session.docs[target_path] = target
        self.backend.result = {"info": {"definition": {"file": target_path, "line": 2, "column": 1}}}
        self.request(1, "info")
        await self.backend.entered(1)
        target.text = "inserted\none\ntwo"
        target.revision += 1
        self.backend.release.set()
        result = await self.writer.reply(1)
        self.assertEqual(result["code"], "stale_snapshot")
        self.assertEqual(self.doc.revision, 0, "the source did not change")

    async def test_info_maps_unchanged_definition_target(self):
        target_path = str(Path(self.tmp.name) / "Other.thy")
        target = H.Doc(self.session, target_path)
        target.text = "one\ntwo"
        self.session.docs[target_path] = target
        self.backend.result = {"info": {"definition": {"file": target_path, "line": 2, "column": 1}}}
        self.request(1, "info")
        await self.backend.entered(1)
        self.backend.release.set()
        result = await self.writer.reply(1)
        self.assertTrue(result["ok"])
        self.assertEqual(result["info"]["definition"]["offset"], 5)

    async def test_info_rejects_newly_opened_definition_target(self):
        target_path = str(Path(self.tmp.name) / "Other.thy")
        self.backend.result = {"info": {"definition": {"file": target_path, "line": 1, "column": 1}}}
        self.request(1, "info")
        await self.backend.entered(1)
        self.session.docs[target_path] = H.Doc(self.session, target_path)
        self.backend.release.set()
        self.assertEqual((await self.writer.reply(1))["code"], "stale_snapshot")

    async def test_snapshot_errors_keep_request_ids_and_free_capacity(self):
        self.backend.failure = H.HostError("test backend rejected", "test_failure")
        self.backend.release.set()
        self.request(1, "state")
        result = await self.writer.reply(1)
        self.assertFalse(result["ok"])
        self.assertEqual(result["code"], "test_failure")
        self.assertEqual(result["error"], "test backend rejected")
        self.request(2, "info", path=str(Path(self.tmp.name) / "missing.thy"))
        self.assertFalse((await self.writer.reply(2))["ok"])
        self.backend.failure = None
        self.request(3, "state")
        self.assertTrue((await self.writer.reply(3))["ok"])


if __name__ == "__main__":
    unittest.main()
