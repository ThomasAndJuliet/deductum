"""Private real daemon lifetime and durable retry across its native endpoint."""
import contextlib
import json
import os
from pathlib import Path
import signal
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import deductum_host as H
import platform_support as P


class DetachedHost(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="Deductum Å endpoint ")
        self.root = Path(self.tmp.name)
        self.state, self.config = self.root / "state", self.root / "config"
        self.state.mkdir()
        self.config.mkdir()
        (self.config / "host.json").write_text('{"isabelle":[],"lean":[]}', encoding="utf-8")
        self.folder = self.root / "Å proof 😀"
        self.folder.mkdir()
        self.path = self.folder / "Main.lean"
        self.path.write_bytes(b"abc\r\n")
        meta = self.state / "sessions" / "fixture" / "meta.json"
        meta.parent.mkdir(parents=True)
        meta.write_text(json.dumps({"id": "fixture", "name": "fixture", "backend": "lean", "folder": str(self.folder),
                                    "toolchain": "unused", "project": {}, "lifecycle": "Stopped", "desired": "stopped"}),
                        encoding="utf-8")
        self.env = {**os.environ, "DEDUCTUM_STATE_DIR": str(self.state), "DEDUCTUM_CONFIG_DIR": str(self.config),
                    "PYTHONUTF8": "1"}
        self.wrapper = self.root / "private-host.py"
        self.wrapper.write_text("import os,sys\nfrom pathlib import Path\n"
                                f"sys.path.insert(0,{str(Path(H.__file__).parent)!r})\n"
                                "import deductum_host as h\n"
                                "h.AUTOSAVE_DELAY=600\n"
                                f"Path({str(self.root / 'host.pid')!r}).write_text(str(os.getpid()))\n"
                                "sys.exit(h.run_daemon())\n", encoding="utf-8")
        self.clients = []
        self.pid = None
        self.process_handles = {}
        self.addCleanup(self.cleanup)

    def start(self):
        # The client process exits immediately after invoking the production
        # detached launcher. The daemon must survive that caller's exit.
        code = ("import sys; sys.path.insert(0,sys.argv[1]); import platform_support as p; "
                "p.spawn_detached(sys.argv[2],sys.argv[3])")
        result = subprocess.run([sys.executable, "-c", code, str(Path(P.__file__).parent), str(self.wrapper),
                                 str(self.root / "host.log")], env=self.env, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        deadline = time.monotonic() + 15
        while True:
            try:
                sock = P.connect(self.state, self.state / "host.sock")
                break
            except OSError:
                if time.monotonic() >= deadline:
                    self.fail("private daemon did not publish its endpoint; log: " + (self.root / "host.log").read_text(errors="replace"))
                time.sleep(.02)
        self.pid = int((self.root / "host.pid").read_text())
        if os.name == "nt":
            import ctypes as C
            from ctypes import wintypes as W
            kernel = C.WinDLL("kernel32", use_last_error=True)
            kernel.OpenProcess.argtypes = [W.DWORD, W.BOOL, W.DWORD]
            kernel.OpenProcess.restype = W.HANDLE
            handle = kernel.OpenProcess(0x100000 | 0x1000, False, self.pid)
            self.assertTrue(handle, "could not retain the private host process identity")
            self.process_handles[self.pid] = handle
        return self.client(sock)

    def client(self, sock=None):
        client = H.SyncClient(sock=sock or P.connect(self.state, self.state / "host.sock"))
        client.sock.settimeout(10)
        self.clients.append(client)
        client.request("hello", client="app", label="private daemon test", concurrent_reads=True)
        return client

    def wait_stopped(self):
        deadline = time.monotonic() + 15
        while True:
            try:
                lock = P.acquire_host_lock(self.state / "host.lock")
                lock.close()
                if os.name == "nt" and self.pid in self.process_handles:
                    import ctypes as C
                    from ctypes import wintypes as W
                    kernel = C.WinDLL("kernel32", use_last_error=True)
                    kernel.WaitForSingleObject.argtypes = [W.HANDLE, W.DWORD]
                    kernel.WaitForSingleObject.restype = W.DWORD
                    self.assertEqual(kernel.WaitForSingleObject(self.process_handles[self.pid], 15_000), 0,
                                     "private host released lock but kept owned process/log handles alive")
                return
            except P.HostLockBusy:
                self.assertLess(time.monotonic(), deadline, "private host did not release its owned lock")
                time.sleep(.02)

    def cleanup(self):
        try:
            if self.pid:
                with contextlib.suppress(OSError, H.HostError, ConnectionError):
                    client = self.client()
                    client.request("shutdown")
                self.wait_stopped()
        finally:
            for client in self.clients:
                with contextlib.suppress(OSError):
                    client.sock.close()
            if os.name == "nt":
                import ctypes as C
                from ctypes import wintypes as W
                kernel = C.WinDLL("kernel32", use_last_error=True)
                kernel.CloseHandle.argtypes = [W.HANDLE]
                for handle in self.process_handles.values():
                    kernel.CloseHandle(handle)
            self.tmp.cleanup()

    def snapshot(self, label):
        artifacts = os.environ.get("DEDUCTUM_TEST_ARTIFACTS_DIR")
        if artifacts:
            destination = Path(artifacts) / (self.root.name + "-" + label)
            destination.mkdir(parents=True, exist_ok=True)
            shutil.copytree(self.state / "sessions", destination / "sessions", dirs_exist_ok=True)
            shutil.copy2(self.path, destination / "disk-Main.lean")
            with contextlib.suppress(OSError):
                shutil.copy2(self.root / "host.log", destination / "host.log")

    def crash(self):
        # Windows SIGTERM calls TerminateProcess; POSIX SIGTERM is handled by
        # the host and flushes state, so POSIX needs SIGKILL for this oracle.
        os.kill(self.pid, signal.SIGTERM if os.name == "nt" else signal.SIGKILL)
        self.wait_stopped()

    def edit(self, client, text="Å😀\n", edit_id="durable-intent"):
        return client.request("edit", session="fixture", path=str(self.path), revision=0,
                              edits=[{"start": 0, "end": 0, "text": text}], edit_id=edit_id,
                              policy="merge", txn="fixture", request=1)

    def read(self, client):
        return client.request("read", session="fixture", path=str(self.path), since_revision=0)

    def test_the_first_acknowledged_edit_survives_a_crash_before_periodic_checkpoint(self):
        client = self.start()
        receipt = self.edit(client)
        self.assertNotIn("Å".encode(), self.path.read_bytes(), "fixture edit was already saved to disk")
        self.crash()
        self.snapshot("first-edit-after-crash")
        restarted = self.start()
        recovered = self.read(restarted)
        self.assertEqual(recovered["doc"]["text"], "Å😀\nabc\n")
        self.assertEqual(recovered["doc"]["revision"], 1)
        duplicate = self.edit(restarted)
        self.assertTrue(duplicate["duplicate"])
        self.assertEqual(duplicate["entry"], receipt["entry"])

    def test_acknowledged_edits_survive_client_exit_and_abrupt_host_restart(self):
        first = self.start()
        self.assertEqual(self.read(first)["doc"]["text"], "abc\n")
        receipt = self.edit(first)
        self.assertTrue(receipt["ok"])
        first.sock.close()
        reconnect = self.client()
        current = self.read(reconnect)
        self.assertEqual(current["doc"]["text"], "Å😀\nabc\n")
        self.assertEqual(current["doc"]["revision"], 1)
        crash_receipt = reconnect.request("edit", session="fixture", path=str(self.path), revision=1,
            edits=[{"start": 0, "end": 0, "text": "crash\n"}], edit_id="crash-intent", policy="merge",
            txn="crash", request=2)
        # Autosave's configured fixture delay leaves this acknowledged revision
        # journal-only. The real endpoint still provides the durability fence.
        self.assertNotIn(b"crash", self.path.read_bytes())
        previous_pid = self.pid
        self.crash()
        self.snapshot("acknowledged-after-crash-before-restart")
        restarted = self.start()
        self.assertNotEqual(self.pid, previous_pid)
        recovered = self.read(restarted)
        self.snapshot("reopened-after-crash")
        self.assertEqual(recovered["doc"]["text"], "crash\nÅ😀\nabc\n")
        self.assertEqual(recovered["doc"]["revision"], 2)
        crash_retry = restarted.request("edit", session="fixture", path=str(self.path), revision=1,
            edits=[{"start": 0, "end": 0, "text": "crash\n"}], edit_id="crash-intent", policy="merge",
            txn="crash", request=2)
        self.assertTrue(crash_retry["duplicate"])
        self.assertEqual(crash_retry["entry"], crash_receipt["entry"])
        retried = self.edit(restarted)
        self.assertTrue(retried["duplicate"], "restarted host did not retain the durable receipt")
        self.assertEqual(retried["entry"], receipt["entry"])
        self.assertEqual(self.read(restarted)["doc"]["revision"], 2)

    def test_a_reply_lost_after_durable_acknowledgement_is_retried_once(self):
        initial = self.start()
        self.read(initial)
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(10)
        observed, failures = [], []
        acknowledged = threading.Event()
        def proxy():
            try:
                downstream, _ = listener.accept()
                upstream = P.connect(self.state, self.state / "host.sock")
                downstream.settimeout(10)
                upstream.settimeout(10)
                with downstream, upstream, downstream.makefile("rb") as incoming, upstream.makefile("rb") as replies:
                    for line in incoming:
                        request = json.loads(line)
                        upstream.sendall(line)
                        while True:
                            response = replies.readline()
                            if not response:
                                raise ConnectionError("private host closed before reply")
                            message = json.loads(response)
                            if request.get("op") == "edit" and message.get("id") == request["id"]:
                                observed.append(message)
                                acknowledged.set()
                                return  # Lose this durable response before the caller receives it.
                            downstream.sendall(response)
                            if message.get("id") == request["id"]:
                                break
            except Exception as error:
                failures.append(type(error).__name__)
                acknowledged.set()
        worker = threading.Thread(target=proxy, daemon=True)
        worker.start()
        try:
            client = self.client(socket.create_connection(listener.getsockname(), timeout=10))
            with self.assertRaises(ConnectionError):
                self.edit(client)
            self.assertTrue(acknowledged.wait(10), "proxy never observed the host's durable reply")
            self.assertEqual(failures, [])
            self.assertTrue(observed and observed[0]["ok"], "the dropped reply was not a successful durable acknowledgement")
            retry = self.client()
            result = self.edit(retry)
            self.assertTrue(result["duplicate"])
            self.assertEqual(result["entry"], observed[0]["entry"])
            self.assertEqual(self.read(retry)["doc"]["revision"], 1)
            self.assertEqual(self.read(retry)["doc"]["text"], "Å😀\nabc\n")
            with self.assertRaises(H.HostError):
                self.edit(retry, text="changed intent")
        finally:
            listener.close()
            worker.join(10)
            self.assertFalse(worker.is_alive(), "fixture proxy did not stop")


if __name__ == "__main__":
    unittest.main()
