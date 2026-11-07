"""The Lean driver's pure parts: positions, the command splitter, goal parts and reports.
Nothing here needs Lean."""
import asyncio
import os
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from drivers import lean  # noqa: E402
from drivers.lsp import TextIndex, utf16_len  # noqa: E402


class ImportInvalidation(unittest.IsolatedAsyncioTestCase):
    """The server can report checked before registering its import closure."""

    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / "Tiny").mkdir()
        self.events, self.notifications = [], []
        self.changed = asyncio.Event()
        case = self
        class Session:
            meta = {}
            def on_backend_event(self, obj):
                case.events.append(obj)
                case.changed.set()
        class ServerWithoutImportGraph:
            def __init__(self):
                self.reporter_finished = asyncio.Event()
            def notify(self, method, params):
                # This is the real protocol boundary: a watched-file change
                # arriving before Lean's importClosure has no stale reply.
                case.notifications.append((method, params))
            async def request(self, method, params, **kwargs):
                if method == "textDocument/waitForDiagnostics":
                    self.reporter_finished.set()
                    return None
                return {}
            async def shutdown(self):
                pass
        self.driver = lean.LeanDriver(Session())
        self.driver.lake_root = str(self.root)
        self.driver.client = ServerWithoutImportGraph()
        self.gates = []

    async def asyncTearDown(self):
        for gate in self.gates:
            gate.set()
        await self.driver.stop()
        self.tmp.cleanup()

    def source(self, module, text):
        path = self.root / (module.replace(".", "/") + ".lean")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return str(path)

    def opened(self, path, text=None):
        text = Path(path).read_text() if text is None else text
        self.driver.send({"op": "open", "file": path, "text": text, "rev": 1})
        doc = self.driver.docs[path]
        self.driver._on_diagnostics({"uri": doc.uri, "version": 1, "diagnostics": []})
        self.driver._on_progress({"textDocument": {"uri": doc.uri, "version": 1}, "processing": []})
        return doc

    async def stale(self, path):
        async def wait():
            while True:
                self.changed.clear()
                for event in self.events:
                    if (event.get("event") == "node" and event["file"] == path
                            and lean.STALE_HEADER_ERROR in event["header_errors"]):
                        return event
                await self.changed.wait()
        return await asyncio.wait_for(wait(), 1)

    async def test_changed_direct_import_is_stale_without_server_graph(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        use = self.source("Tiny.Use", "import Tiny.Dep\n\n#eval n\n")
        doc = self.opened(use)
        await asyncio.wait_for(self.driver.client.reporter_finished.wait(), 1)
        self.assertTrue(self.driver.node_for(doc)["status"]["consolidated"])
        self.driver.send({"op": "disk_changed", "files": [dep]})
        node = self.driver.node_for(doc)
        self.assertEqual(node["header_errors"], [lean.STALE_HEADER_ERROR])
        self.assertFalse(any("out of date" in message["text"] for message in node["messages"]))
        self.assertIn(("workspace/didChangeWatchedFiles", {"changes": [{"uri": Path(dep).as_uri(), "type": 2}]}), self.notifications)

    async def test_transitive_local_import_invalidates_only_affected_documents(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        self.source("Tiny.Middle", "import Tiny.Dep\n")
        use = self.source("Tiny.Use", "import Tiny.Middle\n")
        unrelated = self.source("Tiny.Other", "/- import Tiny.Dep -/\ndef other := 1\n")
        self.opened(use)
        other = self.opened(unrelated)
        self.driver.send({"op": "disk_changed", "files": [dep]})
        self.assertEqual((await self.stale(use))["header_errors"], [lean.STALE_HEADER_ERROR])
        self.assertFalse(other.stale)

    async def test_transitive_scan_uses_open_module_text_and_handles_cycles(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        middle = self.source("Tiny.Middle", "def unrelated := 1\n")
        use = self.source("Tiny.Use", "import Tiny.Middle\n")
        self.opened(middle, "import Tiny.Use Tiny.Dep\n")
        self.opened(use)
        self.driver.send({"op": "disk_changed", "files": [dep]})
        await self.stale(use)

    async def test_late_scan_does_not_invalidate_a_reopened_or_removed_document(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        middle = self.source("Tiny.Middle", "import Tiny.Dep\n")
        use = self.source("Tiny.Use", "import Tiny.Middle\n")
        doc = self.opened(use)
        entered, release = asyncio.Event(), threading.Event()
        self.gates.append(release)
        loop = asyncio.get_running_loop()
        original = lean.read_import_source
        def read(path, remaining):
            if path == middle:
                loop.call_soon_threadsafe(entered.set)
                release.wait(2)
            return original(path, remaining)
        with patch.object(lean, "read_import_source", read):
            self.driver.send({"op": "disk_changed", "files": [dep]})
            await asyncio.wait_for(entered.wait(), 1)
            self.driver._reopen(doc)
            release.set()
            await self.driver.disk_task
        self.assertFalse(doc.stale)
        # Stop also cancels the coalesced graph task and prevents late apply.
        self.assertIsNone(self.driver.disk_task)

    async def test_unrelated_edit_does_not_delay_a_confirmed_transitive_change(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        middle = self.source("Tiny.Middle", "import Tiny.Dep\n")
        use = self.source("Tiny.Use", "import Tiny.Middle\n")
        other = self.source("Tiny.Other", "def unrelated := 1\n")
        doc = self.opened(use)
        self.opened(other)
        entered, release = asyncio.Event(), threading.Event()
        self.gates.append(release)
        loop, original = asyncio.get_running_loop(), lean.read_import_source
        calls = []
        def read(path, remaining):
            if path == middle:
                calls.append(path)
                loop.call_soon_threadsafe(entered.set)
                release.wait(2)
            return original(path, remaining)
        with patch.object(lean, "read_import_source", read):
            self.driver.send({"op": "disk_changed", "files": [dep]})
            await asyncio.wait_for(entered.wait(), 1)
            self.assertTrue(doc.import_pending)
            self.assertIn(lean.UNVERIFIED_IMPORTS, self.driver.node_for(doc)["header_errors"])
            self.driver.send({"op": "edit", "file": other, "rev": 2, "edits": [{"start": 0, "end": 0, "text": "-- typing\n"}]})
            release.set()
            await self.stale(use)
        self.assertEqual(calls, [middle])
        self.assertFalse(doc.import_pending)

    async def test_real_dependency_buffer_edit_rescans_without_losing_change(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        middle = self.source("Tiny.Middle", "import Tiny.Dep\n")
        use = self.source("Tiny.Use", "import Tiny.Middle\n")
        self.opened(use)
        entered, release = asyncio.Event(), threading.Event()
        self.gates.append(release)
        loop, original = asyncio.get_running_loop(), lean.read_import_source
        def read(path, remaining):
            if path == middle:
                loop.call_soon_threadsafe(entered.set)
                release.wait(2)
            return original(path, remaining)
        with patch.object(lean, "read_import_source", read):
            self.driver.send({"op": "disk_changed", "files": [dep]})
            await asyncio.wait_for(entered.wait(), 1)
            # Middle was a closed disk source. Its newly opened buffer must
            # participate in the rescan, rather than publishing the old graph.
            self.opened(middle, "-- current buffer\nimport Tiny.Dep\n")
            release.set()
            await self.stale(use)

    async def test_graph_read_failure_and_limits_are_explicitly_unknown(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        middle = self.source("Tiny.Middle", "import Tiny.Dep\n")
        use = self.source("Tiny.Use", "import Tiny.Middle\n")
        other = self.source("Tiny.Other", "def other := 1\n")
        sources = {use: Path(use).read_text(), other: Path(other).read_text()}
        original = lean.read_import_source
        def failed(path, remaining):
            if path == middle:
                raise PermissionError("unreadable local dependency")
            return original(path, remaining)
        with patch.object(lean, "read_import_source", failed):
            result, errors, _ = lean.local_import_effects(str(self.root), sources, {use: {dep}, other: {dep}})
        self.assertIsNone(result[use])
        self.assertFalse(result[other])
        self.assertTrue(errors)
        result, errors, readsets = lean.local_import_effects(str(self.root), {use: "import Tiny.A Tiny.B Tiny.C\n"},
                                                            {use: {dep}}, module_limit=2)
        self.assertIsNone(result[use])
        self.assertLessEqual(len(readsets[use]), 2)
        self.assertTrue(errors)
        unicode_source = self.source("Tiny.Unicode", "🙂" * 10)
        with self.assertRaises(lean.ImportBudgetExceeded):
            lean.read_import_source(unicode_source, 4)

    async def test_progress_metadata_does_not_touch_disk_on_the_event_loop(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        use = self.source("Tiny.Use", "import Tiny.Dep\n")
        doc = self.opened(use)
        entered, release = asyncio.Event(), threading.Event()
        self.gates.append(release)
        loop, original = asyncio.get_running_loop(), os.stat
        def slow(path, *args, **kwargs):
            if path == dep:
                self.assertIsNot(threading.current_thread(), threading.main_thread())
                loop.call_soon_threadsafe(entered.set)
                release.wait(2)
            return original(path, *args, **kwargs)
        with patch.object(lean.os, "stat", slow):
            self.driver._emit_node(doc)
            await asyncio.wait_for(entered.wait(), 1)
            self.assertFalse(self.events)
            self.assertEqual(self.driver.node_for(doc)["imports"], [])
            release.set()
            await doc.import_task
        self.driver._emit_node(doc)
        self.assertEqual(self.events[-1]["imports"], [dep])
        with patch.object(lean.os, "stat", side_effect=AssertionError("progress performed filesystem I/O")):
            self.assertEqual(self.driver.node_for(doc)["imports"], [dep])

    async def test_overflow_is_unknown_and_stop_suppresses_late_apply(self):
        dep = self.source("Tiny.Dep", "def n := 1\n")
        middle = self.source("Tiny.Middle", "import Tiny.Dep\n")
        use = self.source("Tiny.Use", "import Tiny.Middle\n")
        doc = self.opened(use)
        self.driver.send({"op": "disk_changed", "files": [str(self.root / f"Tiny/X{i}.lean") for i in range(129)]})
        self.assertTrue(doc.import_error)
        self.assertFalse(self.driver.disk_pending)
        entered, release = asyncio.Event(), threading.Event()
        self.gates.append(release)
        loop, original = asyncio.get_running_loop(), lean.read_import_source
        def read(path, remaining):
            if path == middle:
                loop.call_soon_threadsafe(entered.set)
                release.wait(2)
            return original(path, remaining)
        with patch.object(lean, "read_import_source", read):
            self.driver.send({"op": "disk_changed", "files": [dep]})
            await asyncio.wait_for(entered.wait(), 1)
            await asyncio.wait_for(self.driver.stop(), .5)
            release.set()
            await asyncio.sleep(.01)
        self.assertFalse(doc.stale)
        self.assertIsNone(self.driver.disk_task)


class DiagnosticCompletion(unittest.IsolatedAsyncioTestCase):
    """Protocol responses, not elapsed time, establish the final diagnostic set."""

    TEXT = "theorem t : True := by\n  sorry\n"

    async def asyncSetUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.events, self.requests = [], []
        self.started = asyncio.Queue()
        case = self

        class Session:
            meta = {}
            def on_backend_event(self, event):
                case.events.append(event)

        class Server:
            def notify(self, method, params):
                pass
            async def shutdown(self):
                pass
            async def request(self, method, params, **kwargs):
                if method != "textDocument/waitForDiagnostics":
                    return {}
                request = {"params": params, "reply": asyncio.get_running_loop().create_future(),
                           "returned": asyncio.Event(), "canceled": asyncio.Event(),
                           "ready": asyncio.Event(),
                           "task": asyncio.current_task()}
                case.requests.append(request)
                case.started.put_nowait(request)
                try:
                    try:
                        await request["ready"].wait()
                    except asyncio.CancelledError:
                        # An obsolete response can still arrive from a transport
                        # that has already queued it. Exercise the identity fence.
                        request["canceled"].set()
                        await request["ready"].wait()
                    return request["reply"].result()
                finally:
                    request["returned"].set()

        self.driver = lean.LeanDriver(Session())
        self.driver.lake_root = str(self.root)
        self.driver.client = Server()

    async def asyncTearDown(self):
        await self.driver.stop()
        for request in self.requests:
            if not request["reply"].done():
                request["reply"].set_result(None)
            request["ready"].set()
        await asyncio.gather(*(request["task"] for request in self.requests), return_exceptions=True)
        self.temp.cleanup()

    def opened(self, name="Main"):
        path = str(self.root / (name + ".lean"))
        Path(path).write_text(self.TEXT, encoding="utf-8")
        self.driver.send({"op": "open", "file": path, "text": self.TEXT, "rev": 1})
        return self.driver.docs[path]

    def diagnostics(self, doc, warning=False, version=None):
        items = [{"severity": 2, "message": "declaration uses 'sorry'", "range": {
            "start": {"line": 0, "character": 8}, "end": {"line": 0, "character": 9}}}] if warning else []
        self.driver._on_diagnostics({"uri": doc.uri, "version": doc.version if version is None else version,
                                     "diagnostics": items})

    def progress(self, doc, pending=False):
        tasks = [{"range": {"start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 0}}}] if pending else []
        self.driver._on_progress({"textDocument": {"uri": doc.uri, "version": doc.version}, "processing": tasks})

    async def barrier(self, doc):
        try:
            request = await asyncio.wait_for(self.started.get(), 1)
        except asyncio.TimeoutError:
            self.fail(f"No new diagnostic reporter request for {Path(doc.path).name} revision {doc.version}; "
                      f"{len(self.requests)} previous requests recorded")
        self.assertEqual(request["params"], {"uri": doc.uri, "version": doc.version})
        return request

    async def reply(self, request, error=None):
        if error is None:
            request["reply"].set_result(None)
        else:
            request["reply"].set_exception(error)
        request["ready"].set()
        await asyncio.wait_for(request["returned"].wait(), 1)

    def assert_pending(self, doc):
        node = self.driver.node_for(doc)
        self.assertFalse(node["status"]["consolidated"], "final diagnostics were claimed before the reporter fence")
        return node

    async def test_zero_width_tasks_and_empty_intermediate_diagnostics_do_not_finish_a_file(self):
        doc = self.opened()
        self.progress(doc, pending=True)
        self.diagnostics(doc)
        self.assert_pending(doc)
        self.diagnostics(doc, warning=True)
        node = self.assert_pending(doc)
        self.assertEqual(node["status"]["warned"], 1, "pending reporter hid an already published warning")
        self.assertEqual(node["messages"][0]["text"], "declaration uses 'sorry'")
        self.assertEqual(self.requests, [], "zero-width pending work was mistaken for an empty task list")
        self.progress(doc)
        request = await self.barrier(doc)
        self.assert_pending(doc)
        await self.reply(request)
        node = self.driver.node_for(doc)
        self.assertTrue(node["status"]["consolidated"])
        self.assertEqual(node["status"]["warned"], 1)
        self.assertEqual(node["messages"][0]["start"], 8)
        self.assertEqual(node["messages"][0]["end"], 9)

    async def test_final_warning_is_included_whether_progress_or_diagnostics_arrives_first(self):
        for warning_first in (False, True):
            with self.subTest(warning_first=warning_first):
                doc = self.opened("First" if warning_first else "Second")
                self.diagnostics(doc)
                if warning_first:
                    self.diagnostics(doc, warning=True)
                self.progress(doc)
                request = await self.barrier(doc)
                if not warning_first:
                    self.diagnostics(doc, warning=True)
                self.assert_pending(doc)
                await self.reply(request)
                node = self.driver.node_for(doc)
                self.assertTrue(node["status"]["consolidated"])
                self.assertEqual(node["status"]["warned"], 1)
                self.assertEqual(node["messages"][0]["kind"], "warning")

    async def test_empty_final_diagnostics_can_finish_after_the_reporter_reply(self):
        doc = self.opened()
        self.diagnostics(doc)
        self.progress(doc)
        request = await self.barrier(doc)
        self.assert_pending(doc)
        await self.reply(request)
        node = self.driver.node_for(doc)
        self.assertTrue(node["status"]["consolidated"])
        self.assertEqual(node["messages"], [])
        self.assertEqual(node["status"]["finished"], node["status"]["total"])

    async def test_reply_after_an_edit_cannot_finish_the_new_revision(self):
        doc = self.opened()
        self.diagnostics(doc)
        self.progress(doc)
        old = await self.barrier(doc)
        self.driver.send({"op": "edit", "file": doc.path, "rev": 2,
                          "edits": [{"start": 0, "end": 0, "text": "-- new revision\n"}]})
        self.diagnostics(doc)
        self.progress(doc)
        current = await self.barrier(doc)
        await asyncio.wait_for(old["canceled"].wait(), 1)
        await self.reply(old)
        self.assertEqual(self.assert_pending(doc)["seq"], 2)
        await self.reply(current)
        self.assertTrue(self.driver.node_for(doc)["status"]["consolidated"])

    async def test_reopen_and_cancel_each_fence_a_reply_for_the_same_document_version(self):
        for action in ("reopen", "cancel"):
            with self.subTest(action=action):
                doc = self.opened(action)
                self.diagnostics(doc)
                self.progress(doc)
                old = await self.barrier(doc)
                if action == "reopen":
                    self.driver._reopen(doc)
                else:
                    result = await self.driver.request({"op": "cancel"})
                    self.assertEqual(result["canceled"], 1, "pending diagnostics were treated as an idle file")
                self.diagnostics(doc)
                self.progress(doc)
                current = await self.barrier(doc)
                await asyncio.wait_for(old["canceled"].wait(), 1)
                await self.reply(old)
                self.assert_pending(doc)
                await self.reply(current)
                self.assertTrue(self.driver.node_for(doc)["status"]["consolidated"])
                self.driver.send({"op": "close", "file": doc.path})

    async def test_close_and_replacement_fence_a_reply_for_the_same_path_and_revision(self):
        old_doc = self.opened()
        self.diagnostics(old_doc)
        self.progress(old_doc)
        old = await self.barrier(old_doc)
        self.driver.send({"op": "close", "file": old_doc.path})
        new_doc = self.opened()
        self.assertIsNot(old_doc, new_doc)
        self.diagnostics(new_doc)
        self.progress(new_doc)
        current = await self.barrier(new_doc)
        await self.reply(old)
        self.assert_pending(new_doc)
        await self.reply(current)
        self.assertTrue(self.driver.node_for(new_doc)["status"]["consolidated"])

    async def test_closed_document_emits_nothing_when_its_canceled_reporter_replies(self):
        doc = self.opened()
        self.diagnostics(doc)
        self.progress(doc)
        request = await self.barrier(doc)
        self.driver.send({"op": "close", "file": doc.path})
        self.events.clear()
        await self.reply(request)
        self.assertNotIn(doc.path, self.driver.docs)
        self.assertEqual(self.events, [], "a closed document emitted a late reporter result")

    async def test_error_from_an_old_revision_is_not_reported_as_a_current_failure(self):
        doc = self.opened()
        self.diagnostics(doc)
        self.progress(doc)
        old = await self.barrier(doc)
        self.driver.send({"op": "edit", "file": doc.path, "rev": 2,
                          "edits": [{"start": 0, "end": 0, "text": "-- new revision\n"}]})
        self.events.clear()
        await self.reply(old, error=lean.LspError(-32603, "obsolete reporter failure"))
        self.assertFalse(any("obsolete reporter failure" in event.get("text", "") for event in self.events))
        self.assertEqual(self.assert_pending(doc)["seq"], 2)

    async def test_new_pending_work_invalidates_a_reporter_reply_for_the_same_version(self):
        doc = self.opened()
        self.diagnostics(doc)
        self.progress(doc)
        old = await self.barrier(doc)
        self.progress(doc, pending=True)
        await self.reply(old)
        self.assert_pending(doc)
        self.progress(doc)
        current = await self.barrier(doc)
        await self.reply(current)
        self.assertTrue(self.driver.node_for(doc)["status"]["consolidated"])

    async def test_a_new_progress_cycle_gets_its_own_barrier_before_the_old_reporter_replies(self):
        for error in (None, lean.LspError(-32603, "obsolete progress-cycle failure")):
            with self.subTest(reply="success" if error is None else "error"):
                doc = self.opened("FreshCycle" if error is None else "FailedCycle")
                self.diagnostics(doc)
                self.progress(doc)
                old = await self.barrier(doc)
                self.progress(doc, pending=True)
                self.progress(doc)
                current = await self.barrier(doc)
                self.assertIsNot(current, old)
                await asyncio.wait_for(old["canceled"].wait(), 1)
                self.events.clear()
                await self.reply(old, error=error)
                self.assert_pending(doc)
                self.assertFalse(any("obsolete progress-cycle failure" in event.get("text", "") for event in self.events))
                await self.reply(current)
                self.assertTrue(self.driver.node_for(doc)["status"]["consolidated"])

    async def test_stop_cancels_pending_diagnostics_and_a_late_reply_emits_no_new_node(self):
        doc = self.opened()
        self.diagnostics(doc)
        self.progress(doc)
        request = await self.barrier(doc)
        await asyncio.wait_for(self.driver.stop(), 1)
        await asyncio.wait_for(request["canceled"].wait(), 1)
        self.events.clear()
        await self.reply(request)
        self.assertEqual(self.events, [], "a stopped driver emitted a node after a stale reporter reply")
        self.assert_pending(doc)

    async def test_failed_or_canceled_reporter_requests_never_manufacture_completion(self):
        for index, error in enumerate((lean.LspError(-32603, "reporter failed"), lean.LspClosed(),
                                       asyncio.TimeoutError(), asyncio.CancelledError())):
            with self.subTest(error=type(error).__name__):
                doc = self.opened("Error" + str(index))
                self.diagnostics(doc)
                self.progress(doc)
                request = await self.barrier(doc)
                await self.reply(request, error=error)
                self.assert_pending(doc)


def split(text):
    return [(text[s:e], n) for s, e, n in lean.split_commands(text)]


def names(text):
    return [n for _, n in split(text)]


class Positions(unittest.TestCase):
    def test_ascii(self):
        ix = TextIndex("ab\ncde\n\nf")
        self.assertEqual(ix.to_lsp(0), {"line": 0, "character": 0})
        self.assertEqual(ix.to_lsp(4), {"line": 1, "character": 1})
        self.assertEqual(ix.to_lsp(7), {"line": 2, "character": 0})
        self.assertEqual(ix.to_lsp(9), {"line": 3, "character": 1})
        self.assertEqual(ix.to_offset(1, 1), 4)
        self.assertEqual(ix.to_offset(3, 5), 9)  # past the end clamps
        self.assertEqual(ix.to_offset(9, 0), 9)

    def test_non_bmp_counts_twice(self):
        text = "a🙂b\n𝔸x"
        self.assertEqual(utf16_len("🙂"), 2)
        ix = TextIndex(text)
        self.assertEqual(ix.to_lsp(2), {"line": 0, "character": 3})  # before b
        self.assertEqual(ix.to_offset(0, 3), 2)
        self.assertEqual(ix.to_offset(0, 2), 2)  # inside the pair: before the next whole character
        self.assertEqual(ix.to_lsp(len("a🙂b\n𝔸x") - 1), {"line": 1, "character": 2})
        self.assertEqual(ix.to_offset(1, 2), len(text) - 1)

    def test_combining_marks_count_per_code_point(self):
        text = "éx"  # e + combining acute: two code points, two units
        ix = TextIndex(text)
        self.assertEqual(ix.to_lsp(2), {"line": 0, "character": 2})
        self.assertEqual(ix.to_offset(0, 2), 2)

    def test_round_trip_everywhere(self):
        text = "∀ x, 🙂 x\n\n  naïve 𝔸𝔹 é\nlast"
        ix = TextIndex(text)
        for off in range(len(text) + 1):
            p = ix.to_lsp(off)
            self.assertEqual(ix.to_offset(p["line"], p["character"]), off)

    def test_range(self):
        ix = TextIndex("x\n🙂y")
        self.assertEqual(ix.range_to_lsp(2, 4), {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 3}})
        self.assertEqual(ix.range_to_offsets(ix.range_to_lsp(2, 4)), (2, 4))


class Regions(unittest.TestCase):
    def kinds(self, text):
        return bytes(lean.scan_regions(text))

    def test_comments_nest(self):
        t = "a /- x /- y -/ z -/ b"
        m = lean.scan_regions(t)
        self.assertEqual(m[t.index("b")], lean.CODE)
        self.assertEqual(m[t.index("z")], lean.COMMENT)

    def test_line_comment_stops_at_newline(self):
        t = "a -- c\nb"
        m = lean.scan_regions(t)
        self.assertEqual(m[t.index("c")], lean.COMMENT)
        self.assertEqual(m[t.index("b")], lean.CODE)

    def test_strings_and_chars(self):
        t = 'x "a -- b \\" c" y \'a\' h\' z'
        m = lean.scan_regions(t)
        self.assertEqual(m[t.index("--")], lean.STRING)
        self.assertEqual(m[t.index("y")], lean.CODE)
        self.assertEqual(m[t.index("'a'")], lean.STRING)
        self.assertEqual(m[t.index("h'") + 1], lean.CODE)  # a prime, not a literal
        self.assertEqual(m[t.index("z")], lean.CODE)

    def test_docstrings(self):
        t = "/-- doc -/\n/-! mod -/\n/- plain -/"
        m = lean.scan_regions(t)
        self.assertEqual(m[0], lean.DOC)
        self.assertEqual(m[t.index("mod")], lean.MODDOC)
        self.assertEqual(m[t.index("plain")], lean.COMMENT)


class TopLevel(unittest.TestCase):
    def test_keywords_and_names(self):
        t = "import Foo\n\nnamespace A\n\nopen B\n\ndef f := 1\n\nend A\n"
        self.assertEqual(names(t), ["import", "namespace", "open", "def", "end"])

    def test_module_doc_is_its_own_command(self):
        t = "/-!\n# Title\n\ntheorem not_a_command := 1\n-/\n\ndef f := 1\n"
        self.assertEqual(split(t)[0][1], "/-!")
        self.assertTrue(split(t)[0][0].endswith("-/"))
        self.assertEqual(names(t), ["/-!", "def"])

    def test_docstring_belongs_to_the_declaration(self):
        t = "/-- Adds.\nsecond line at column 0\n-/\ntheorem foo : True := trivial\n\ndef g := 2\n"
        cs = split(t)
        self.assertEqual([n for _, n in cs], ["theorem", "def"])
        self.assertTrue(cs[0][0].startswith("/-- Adds."))

    def test_attributes_and_modifiers_are_part_of_the_declaration(self):
        t = "@[simp]\ntheorem a : True := trivial\n\n@[simp] lemma b : True := trivial\n\nprivate def c := 1\n\nnoncomputable def d := 2\n"
        self.assertEqual(names(t), ["theorem", "lemma", "def", "def"])

    def test_open_in_prefixes_the_next_command(self):
        t = "open Nat in\ntheorem a : True := trivial\n\nset_option maxRecDepth 100 in\ndef b := 1\n"
        self.assertEqual(names(t), ["theorem", "def"])

    def test_diagnostic_commands(self):
        t = "#check Nat\n#eval 1 + 1\n#print axioms foo\n"
        self.assertEqual(names(t), ["#check", "#eval", "#print"])

    def test_keywords_in_comments_and_strings_do_not_split(self):
        t = ('def f : String :=\n  "\ntheorem inside string\n"\n\n'
             "-- theorem in a comment\n/- def in a block\ntheorem too\n-/\ntheorem real : True := trivial\n")
        self.assertEqual(names(t), ["def", "theorem"])

    def test_commands_end_before_trailing_comments(self):
        t = "def f := 1 -- trailing\n-- between\ndef g := 2\n"
        cs = split(t)
        self.assertEqual(cs[0][0], "def f := 1")
        self.assertEqual(cs[1][0], "def g := 2")

    def test_continuation_lines_at_column_zero(self):
        t = "structure P where\n  x : Nat\nderiving Repr\n\ninstance : Foo P := ⟨⟩\n"
        self.assertEqual(names(t), ["structure", "instance"])

    def test_deriving_instance_is_a_command(self):
        self.assertEqual(names("inductive T | a\nderiving instance Repr for T\n"), ["inductive", "deriving"])

    def test_mutual(self):
        t = "mutual\ndef a := 1\ndef b := 2\nend\n"
        self.assertEqual(names(t), ["mutual", "def", "def", "end"])

    def test_commands_tile_in_order(self):
        t = "/-! a -/\n\n/-- d -/\ntheorem t (n : Nat) : n = n := by\n  cases n\n  · rfl\n  · rfl\n\ndef g := 1\n"
        cs = lean.split_commands(t)
        for (s, e, _), (s2, e2, _) in zip(cs, cs[1:]):
            self.assertTrue(s < e <= s2 < e2)
        self.assertEqual(t[cs[-1][0]:cs[-1][1]], "def g := 1")

    def test_toplevel_covers_the_proof(self):
        t = "theorem t : True := by\n  trivial\n\ndef g := 1\n"
        sp = lean.Splitter(t)
        self.assertEqual([(t[s:e], n) for s, e, n in sp.toplevel],
                         [("theorem t : True := by\n  trivial", "theorem"), ("def g := 1", "def")])


class Tactics(unittest.TestCase):
    def test_header_ends_at_by(self):
        t = "theorem t (a b : Nat)\n    (h : a = b) : b = a := by\n  omega\n"
        self.assertEqual(split(t), [("theorem t (a b : Nat)\n    (h : a = b) : b = a := by", "theorem"), ("omega", "omega")])

    def test_by_on_the_same_line_splits(self):
        t = "theorem t : True := by trivial\n"
        self.assertEqual(split(t), [("theorem t : True := by", "theorem"), ("trivial", "trivial")])

    def test_by_in_brackets_does_not_split(self):
        t = "def f : Fin 3 := ⟨0, by decide⟩\n"
        self.assertEqual(split(t), [("def f : Fin 3 := ⟨0, by decide⟩", "def")])

    def test_term_proof_stays_one_command(self):
        t = "theorem t : 1 = 1 :=\n  rfl\n"
        self.assertEqual(names(t), ["theorem"])

    def test_sorry_term_proof_is_split_off(self):
        t = "theorem t : 1 = 1 := sorry\n"
        self.assertEqual(split(t), [("theorem t : 1 = 1 :=", "theorem"), ("sorry", "sorry")])

    def test_tactic_lines_and_continuations(self):
        t = ("theorem t (p q : Prop) (hp : p) (hq : q) : p ∧ q := by\n"
             "  constructor\n"
             "  · exact hp\n"
             "  · apply And.intro\n"
             "      hq\n"
             "      hq\n")
        cs = split(t)
        self.assertEqual([n for _, n in cs], ["theorem", "constructor", "·", "exact", "·", "apply"])
        self.assertEqual(cs[-1][0], "apply And.intro\n      hq\n      hq")

    def test_bullet_blocks_with_several_tactics(self):
        t = ("theorem t : True ∧ True := by\n"
             "  constructor\n"
             "  · intro x\n"
             "    simp\n"
             "    exact foo\n"
             "  · sorry\n")
        self.assertEqual(names(t), ["theorem", "constructor", "·", "intro", "simp", "exact", "·", "sorry"])

    def test_nested_by_and_have(self):
        t = ("theorem t (n : Nat) : n = n := by\n"
             "  have h : n + 0 = n := by\n"
             "    simp\n"
             "  have h2 : n = n := by simp\n"
             "  exact rfl\n")
        cs = split(t)
        self.assertEqual([n for _, n in cs], ["theorem", "have", "simp", "have", "simp", "exact"])
        self.assertEqual(cs[1][0], "have h : n + 0 = n := by")

    def test_induction_alternatives(self):
        t = ("theorem t (n : Nat) : n + 0 = n := by\n"
             "  induction n with\n"
             "  | zero => simp\n"
             "  | succ k ih =>\n"
             "    simp at ih\n"
             "    omega\n")
        self.assertEqual(names(t), ["theorem", "induction", "|", "simp", "|", "simp", "omega"])

    def test_case_and_next(self):
        t = ("theorem t : True ∧ True := by\n  constructor\n  case left => trivial\n  next => trivial\n")
        self.assertEqual(names(t), ["theorem", "constructor", "case", "trivial", "next", "trivial"])

    def test_calc_steps(self):
        t = ("theorem t (a b c : Nat) (h1 : a = b) (h2 : b = c) : a = c := by\n"
             "  calc a = b := by rw [h1]\n"
             "    _ = c := by\n"
             "      exact h2\n")
        cs = split(t)
        self.assertEqual([n for _, n in cs], ["theorem", "calc", "rw", "_", "exact"])
        self.assertEqual(cs[1][0], "calc a = b := by")

    def test_calc_on_following_lines(self):
        t = ("theorem t (a b : Nat) (h : a = b) : a = b := by\n"
             "  calc\n"
             "    a = b := h\n"
             "    _ = b := rfl\n")
        self.assertEqual(names(t), ["theorem", "calc", "a", "_"])

    def test_tool_tactics_and_placeholders(self):
        t = "example : True := by\n  exact?\n  simp?\n  sorry\n"
        self.assertEqual(names(t), ["example", "exact?", "simp?", "sorry"])

    def test_keywords_inside_tactic_comments_and_strings(self):
        t = ("theorem t : True := by\n"
             "  -- theorem def lemma\n"
             "  /- by\n   sorry -/\n"
             "  trace \"by sorry\"\n"
             "  trivial\n")
        self.assertEqual(names(t), ["theorem", "trace", "trivial"])

    def test_function_arrow_does_not_open_a_block(self):
        t = ("theorem t : True := by\n"
             "  let f : Nat → Nat := fun x =>\n"
             "    x + 1\n"
             "  trivial\n")
        self.assertEqual(names(t), ["theorem", "let", "trivial"])

    def test_term_match_inside_parentheses_is_one_tactic(self):
        t = ("theorem t (o : Option Nat) : True := by\n"
             "  change True = (match o with\n"
             "    | some _ => True\n"
             "    | none => True)\n"
             "  trivial\n")
        self.assertEqual(names(t), ["theorem", "change", "trivial"])

    def test_placeholder_in_term_and_tactic(self):
        self.assertEqual(names("theorem t : True := by sorry\n"), ["theorem", "sorry"])
        self.assertEqual(names("theorem t : True := by\n  sorry\n"), ["theorem", "sorry"])

    def test_def_by_block(self):
        t = "def f (n : Nat) : Nat := by\n  exact n\n"
        self.assertEqual(names(t), ["def", "exact"])

    def test_padded_keyword_line_reads_as_sorry(self):
        # (the host's prover_text turns `claude try simp` into `sorry` plus spaces)
        t = "theorem t : True := by\n  sorry          \n\ndef g := 1\n"
        self.assertEqual(split(t)[1], ("sorry", "sorry"))

    def test_nonbmp_characters(self):
        t = "theorem t : True := by\n  -- 🙂\n  trivial\n"
        cs = lean.split_commands(t)
        self.assertEqual(t[cs[1][0]:cs[1][1]], "trivial")


class Goals(unittest.TestCase):
    TT = {"append": [{"tag": [{"info": 1}, {"text": "a"}]}, {"text": " + "}, {"text": "b"}]}

    def test_tagged_text(self):
        self.assertEqual(lean.tagged_text(self.TT), "a + b")
        self.assertEqual(lean.tagged_text(None), "")

    def interactive(self):
        return {"userName": "succ", "goalPrefix": "⊢ ", "type": self.TT,
                "hyps": [{"names": ["n", "m"], "type": {"text": "Nat"}},
                         {"names": ["x"], "type": {"text": "Nat"}, "val": {"text": "n + 1"}},
                         {"names": ["h"], "type": {"text": "n = m"}}]}

    def test_goal_text_as_the_infoview(self):
        g = lean.goal_from_interactive(self.interactive())
        self.assertEqual(lean.goal_text(g), "case succ\nn m : Nat\nx : Nat := n + 1\nh : n = m\n⊢ a + b")

    def test_parts(self):
        g = lean.goal_from_interactive(self.interactive())
        parts = lean.goal_parts([g, {"tag": None, "hyps": [], "target": "True", "prefix": "⊢ "}])
        raws = [p["raw"] for p in parts]
        self.assertEqual(raws[0], "#deductum:count 2\n")
        self.assertIn("#deductum:case 1\nsucc", raws)
        self.assertIn("#deductum:param 1\nn m : Nat", raws)
        self.assertIn("#deductum:param 1\nx : Nat := n + 1", raws)
        self.assertIn("#deductum:concl 1\na + b", raws)
        self.assertIn("#deductum:concl 2\nTrue", raws)
        self.assertEqual(sum(r.startswith("#deductum:case") for r in raws), 1)
        self.assertTrue(raws[-1].startswith("#deductum:text 0\ncase succ\n"))
        self.assertTrue(all(p["kind"] == "writeln" and p["text"] == p["raw"] for p in parts))

    def test_no_goals(self):
        parts = lean.goal_parts([], nogoal=True)
        self.assertEqual(parts[0]["raw"], "#deductum:nogoal 0\nNo goals")
        self.assertFalse(any(p["raw"].startswith("#deductum:count") for p in parts))

    def test_parse_plain_goal(self):
        g = lean.parse_goal_text("case right\np q : Prop\nhp : p\nlong : A →\n    B\n⊢ q\n  ∧ r")
        self.assertEqual(g["tag"], "right")
        self.assertEqual(g["hyps"][0], {"names": ["p", "q"], "type": "Prop", "value": None})
        self.assertEqual(g["hyps"][2]["type"], "A →\n    B")
        self.assertEqual(g["target"], "q\n  ∧ r")

    def test_parse_term_goal(self):
        g = lean.parse_goal_text("n : Nat\n⊢ Nat")
        self.assertIsNone(g["tag"])
        self.assertEqual(lean.goal_text(g), "n : Nat\n⊢ Nat")


class Reports(unittest.TestCase):
    CMDS = [(0, 10, "theorem"), (11, 20, "simp"), (21, 30, "omega"), (31, 40, "def")]

    def diag(self, sev, start, end, msg="m"):
        return {"severity": sev, "start": start, "end": end, "message": msg}

    def test_done_statuses(self):
        rep = lean.build_report(self.CMDS, [self.diag(1, 22, 25), self.diag(2, 1, 5)], [])
        self.assertEqual([c[3] for c in rep["commands"]], ["warned", "finished", "failed", "finished"])
        st = rep["status"]
        self.assertEqual((st["total"], st["warned"], st["failed"], st["finished"], st["percentage"]), (4, 1, 1, 2, 100))
        self.assertTrue(st["consolidated"])

    def test_error_wins_over_warning(self):
        rep = lean.build_report(self.CMDS, [self.diag(2, 1, 5), self.diag(1, 2, 3)], [])
        self.assertEqual(rep["commands"][0][3], "failed")

    def test_wide_error_is_anchored_at_its_start(self):
        rep = lean.build_report(self.CMDS, [self.diag(1, 11, 40)], [])
        self.assertEqual([c[3] for c in rep["commands"]], ["finished", "failed", "finished", "finished"])

    def test_running_and_unprocessed(self):
        rep = lean.build_report(self.CMDS, [], [(15, 40)])
        self.assertEqual([c[3] for c in rep["commands"]], ["finished", "running", "unprocessed", "unprocessed"])
        st = rep["status"]
        self.assertEqual((st["running"], st["unprocessed"], st["percentage"]), (1, 2, 25))
        self.assertFalse(st["consolidated"])

    def test_zero_width_tasks_are_pending_even_when_no_command_range_is_marked_running(self):
        rep = lean.build_report(self.CMDS, [], [(5, 5), (25, 25)])
        self.assertTrue(all(c[3] == "finished" for c in rep["commands"]))
        self.assertFalse(rep["status"]["consolidated"], "zero-width tasks still represent pending work")

    def test_nothing_known_is_unprocessed(self):
        rep = lean.build_report(self.CMDS, [], None)
        self.assertEqual([c[3] for c in rep["commands"]], ["running", "unprocessed", "unprocessed", "unprocessed"])

    def test_diagnostic_in_a_gap_belongs_to_the_next_command(self):
        self.assertEqual(lean.anchor_command(self.CMDS, 10), 0)
        self.assertEqual(lean.anchor_command(self.CMDS, 15), 1)
        self.assertEqual(lean.anchor_command([], 3), None)

    def test_messages(self):
        for sev, kind in ((1, "error"), (2, "warning"), (3, "information"), (4, "writeln")):
            m = lean.diag_message(self.diag(sev, 3, 7, "text"))
            self.assertEqual((m["kind"], m["text"], m["start"], m["end"]), (kind, "text", 3, 7))

    def test_try_this_sendbacks(self):
        self.assertEqual(lean.try_this("Try this: exact hq"), ["exact hq"])
        self.assertEqual(lean.try_this("Try this: rw [Nat.add_one]\n-- a < b.succ"), ["rw [Nat.add_one]"])
        self.assertEqual(lean.try_this("Try this: simp only [foo]\n  [bar]"), ["simp only [foo]\n  [bar]"])
        self.assertEqual(lean.try_this("Try these:\n  • simp\n  • [apply] exact h\n"), ["simp", "exact h"])
        self.assertEqual(lean.try_this("unsolved goals"), [])
        m = lean.diag_message(self.diag(3, 0, 5, "Try this: exact hq"))
        self.assertEqual(m["sendbacks"], ["exact hq"])


class Maps(unittest.TestCase):
    def test_text_map(self):
        tm = lean.TextMap("abcdef", "abXYcdef")
        self.assertEqual(tm.pos(1), 1)
        self.assertEqual(tm.pos(4), 6)
        self.assertEqual(tm.span(4, 6), (6, 8))
        self.assertIsNone(tm.pos(1) if False else lean.TextMap("abcdef", "abXef").pos(3))

    def test_text_map_deletion_and_same(self):
        self.assertEqual(lean.TextMap("same", "same").span(1, 3), (1, 3))
        tm = lean.TextMap("abcdef", "abef")
        self.assertEqual(tm.span(4, 6), (2, 4))
        self.assertEqual(tm.span(2, 4), (2, 2))  # the deleted region collapses
        self.assertIsNone(tm.span(3, 4))

    def test_colours_from_tokens(self):
        text = "theorem x\n  🙂 y"
        ix = TextIndex(text)
        legend = ["keyword", "variable"]
        # theorem at 0:0 len 7 (keyword); y on line 1 after 🙂 and space: char 5, len 1
        data = [0, 0, 7, 0, 0, 1, 5, 1, 1, 0]
        cols = lean.decode_tokens(data, legend, ix)
        self.assertEqual(cols[0], [0, 7, "keyword1"])
        self.assertEqual(text[cols[1][0]:cols[1][1]], "y")
        self.assertEqual(cols[1][2], "free")

    def test_lexical_colours(self):
        t = 'a -- c\n"s" b'
        cols = lean.lexical_colours(lean.scan_regions(t))
        self.assertEqual([(t[s:e], k) for s, e, k in cols], [("-- c", "comment1"), ('"s"', "inner_quoted")])


class Discovery(unittest.TestCase):
    def test_header_of(self):
        h = lean.LeanDriver.header_of("import A\nimport B\n\nopen X\n\ntheorem t : True := trivial\n")
        self.assertEqual(h, "import A\nimport B\n\nopen X")

    def test_fact_check_text(self):
        self.assertEqual(lean.LeanDriver.fact_check_text(["f", "g.h"]), "#check @f\n#check @g.h")
        self.assertIsNone(lean.LeanDriver.fact_check_text([]))

    def test_version_tuple(self):
        self.assertEqual(lean.version_tuple("leanprover/lean4:v4.19.0"), (4, 19, 0))
        self.assertEqual(lean.version_tuple("leanprover/lean4:v4.22.0-rc1"), (4, 22, 0))
        self.assertIsNone(lean.version_tuple("stable"))

    def test_detect_and_project(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory(dir=os.path.expanduser("~")) as d:
            os.makedirs(os.path.join(d, "Sub", "Deep"))
            for name, content in (("lakefile.toml", 'name = "x"\n'), ("lean-toolchain", "leanprover/lean4:v4.99.0\n"),
                                  ("lake-manifest.json", json.dumps({"packages": [{"name": "mathlib"}, {"name": "aesop"}]}))):
                with open(os.path.join(d, name), "w") as f:
                    f.write(content)
            found = lean.LeanDriver.detect(os.path.join(d, "Sub", "Deep"))
            self.assertEqual(found["project"]["lake_root"], os.path.realpath(d))
            self.assertEqual(found["toolchain"], "leanprover/lean4:v4.99.0")
            self.assertTrue(found["project"]["mathlib"])
            self.assertEqual(found["project"]["packages"], ["mathlib", "aesop"])
            self.assertIn("elan will download", found["note"])
            self.assertEqual(lean.LeanDriver.project_key(found["project"]), os.path.realpath(d))
            moved = lean.LeanDriver.relocate_project(found["project"], lambda p: "/w" + p)
            self.assertEqual(moved["lake_root"], "/w" + os.path.realpath(d))

    def test_capabilities(self):
        drv = lean.LeanDriver(object())
        drv.version = "4.19.0"
        caps = drv.capabilities()
        names_ = [t["name"] for t in caps["tools"]]
        self.assertIn("exact?", names_)
        self.assertNotIn("grind", names_)
        self.assertEqual(caps["counterexamples"], [])
        self.assertEqual([a["name"] for a in caps["project_actions"]], ["build", "rebuild_imports"])
        drv.version = "4.22.0"
        drv.project = {"mathlib": True, "packages": ["mathlib"]}
        caps = drv.capabilities()
        self.assertIn("grind", [t["name"] for t in caps["tools"]])
        self.assertEqual(caps["counterexamples"], ["plausible"])
        self.assertIn("cache_get", [a["name"] for a in caps["project_actions"]])
        self.assertEqual(caps["facts_line"], "simp [{facts}]")
        self.assertEqual(lean.LeanDriver.placeholder_commands, ("sorry",))

    def test_scratch_text(self):
        class S:
            meta = {}
        drv = lean.LeanDriver(S())
        text = "theorem a : True := by\n  trivial\n\ntheorem t (p q : Prop) (hp : p) (hq : q) : p ∧ q := by\n  constructor\n  · exact hp\n  · sorry\n\ndef z := 1\n"
        doc = lean.LDoc(os.path.abspath(os.path.join("x", "A.lean")), text, 1)
        at = text.index("sorry")
        body, lo, hi, as_command = drv.scratch_text(doc, at, "exact hq")
        self.assertFalse(as_command)
        self.assertTrue(body.endswith("  · exact hq"))
        self.assertNotIn("def z", body)
        self.assertEqual(body[lo:lo + 7], "theorem")
        self.assertTrue(body.startswith("theorem a"))
        body, lo, hi, _ = drv.scratch_text(doc, at, "constructor\nexact hq")
        self.assertTrue(body.endswith("  · constructor\n    exact hq"))
        body, lo, hi, as_command = drv.scratch_text(doc, at, "#check @t")
        self.assertTrue(as_command)
        self.assertTrue(body.endswith("  · sorry\n#check @t\n"))
        self.assertEqual(body[lo:], "#check @t\n")


class Loogle(unittest.TestCase):
    ANSWER = {"header": "Found 2 declarations", "count": 2, "hits": [
        {"name": "Nat.add_comm", "type": "∀ (n m : ℕ), n + m = m + n", "module": "Init.Core", "doc": None},
        {"name": "Nat.add_comm'", "type": "∀ (n m : ℕ), m + n = n + m", "module": "Init.Core"}]}

    def test_hits_become_messages_with_the_name_to_insert(self):
        msgs = lean.loogle_messages(self.ANSWER, "public")
        self.assertEqual(msgs[0]["text"], "2 declarations from loogle.lean-lang.org")
        self.assertEqual(msgs[1]["name"], "Nat.add_comm")
        self.assertEqual(msgs[1]["module"], "Init.Core")
        self.assertEqual(msgs[1]["sendbacks"], ["Nat.add_comm"])
        self.assertEqual(msgs[1]["text"], "Nat.add_comm : ∀ (n m : ℕ), n + m = m + n\nInit.Core")
        self.assertEqual(len(msgs), 3)

    def test_an_error_answer_is_an_error_message(self):
        msgs = lean.loogle_messages({"error": "Unknown identifier 'foo'", "suggestions": ["Foo.bar"]}, "public")
        self.assertEqual([m["kind"] for m in msgs], ["error", "writeln"])
        self.assertIn("Foo.bar", msgs[1]["text"])

    def test_the_limit_is_said(self):
        hits = [{"name": f"n{i}", "type": "t", "module": "M"} for i in range(60)]
        msgs = lean.loogle_messages({"count": 600, "hits": hits}, "local")
        self.assertEqual(len(msgs), lean.LOOGLE_LIMIT + 1)
        self.assertIn("showing 40", msgs[0]["text"])
        self.assertIn("local loogle", msgs[0]["text"])

    def test_off_and_unknown_sources_and_empty_queries_fail_clearly(self):
        from drivers import HostError
        for query, source, fragment in [("x", "off", "off in the settings"), ("", "public", "Type a name"),
                                       ("x", "bogus", "Unknown search source")]:
            with self.assertRaises(HostError) as cm:
                lean.loogle_fetch(query, source)
            self.assertIn(fragment, str(cm.exception))

    def test_local_needs_a_binary_on_the_path(self):
        from drivers import HostError
        saved = os.environ.get("PATH")
        os.environ["PATH"] = "/nonexistent"
        try:
            with self.assertRaises(HostError) as cm:
                lean.loogle_fetch("Nat.add_comm", "local")
            self.assertIn("no loogle program", str(cm.exception))
        finally:
            os.environ["PATH"] = saved or ""

    def test_local_runs_the_binary(self):
        import tempfile
        from fixture_helpers import json_executable
        with tempfile.TemporaryDirectory() as d:
            json_executable(d, "loogle", self.ANSWER)
            saved = os.environ.get("PATH", "")
            os.environ["PATH"] = d + os.pathsep + saved
            try:
                res = lean.loogle_fetch("Nat.add_comm", "local")
            finally:
                os.environ["PATH"] = saved
            self.assertEqual(res["count"], 2)

    def test_public_asks_the_json_endpoint(self):
        import io, json
        seen = {}

        class Res(io.BytesIO):
            def __enter__(self): return self
            def __exit__(self, *a): return False

        def fake(req, timeout=None):
            seen["url"], seen["timeout"] = req.full_url, timeout
            return Res(json.dumps(self.ANSWER).encode())

        real = lean.urllib.request.urlopen
        lean.urllib.request.urlopen = fake
        try:
            res = lean.loogle_fetch("?a + ?b = ?b + ?a", "public")
        finally:
            lean.urllib.request.urlopen = real
        self.assertEqual(seen["url"], "https://loogle.lean-lang.org/json?q=%3Fa+%2B+%3Fb+%3D+%3Fb+%2B+%3Fa")
        self.assertLessEqual(seen["timeout"], 30)
        self.assertEqual(res["hits"][0]["name"], "Nat.add_comm")

    def test_an_unreachable_server_is_a_clear_error(self):
        from drivers import HostError
        real = lean.urllib.request.urlopen

        def down(req, timeout=None):
            raise lean.urllib.error.URLError("no route")

        lean.urllib.request.urlopen = down
        try:
            with self.assertRaises(HostError) as cm:
                lean.loogle_fetch("x", "public")
        finally:
            lean.urllib.request.urlopen = real
        self.assertIn("could not be reached", str(cm.exception))


class Hover(unittest.TestCase):
    def test_a_signature_and_its_docstring_are_two_items_without_fences(self):
        md = "```lean\nNat.add_comm (n m : Nat) : n + m = m + n\n```\n***\nAddition is commutative.\n\nSee `Nat.add_assoc`."
        self.assertEqual(lean.tooltip_items(md), ["Nat.add_comm (n m : Nat) : n + m = m + n",
                                                 "Addition is commutative.\n\nSee `Nat.add_assoc`."])

    def test_plain_and_empty_hovers(self):
        self.assertEqual(lean.tooltip_items("just text"), ["just text"])
        self.assertEqual(lean.tooltip_items("  \n"), [])
        self.assertEqual(lean.tooltip_items("```lean\nx : Nat\n```"), ["x : Nat"])


if __name__ == "__main__":
    unittest.main()
