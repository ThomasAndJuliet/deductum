"""Real endpoint/process tests; Windows security is exercised on Windows only."""
import asyncio
import contextlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import types
import re
import hmac
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import platform_support as P
import drivers

FIXTURE = str(Path(__file__).with_name("process_fixture.py"))


class EndpointRecords(unittest.TestCase):
    def test_malformed_endpoint_records_fail_without_disclosing_the_token(self):
        with tempfile.TemporaryDirectory() as temp:
            state = Path(temp) / "state"
            P.ensure_state_dir(state)
            valid = {"version": 2, "port": 12345, "token": "a" * 64, "bundle_id": "test-bundle"}
            cases = [None, [], "invalid", {}, {**valid, "version": True}]
            for key, values in {"version": [None, 1, 3, "2"],
                                "port": [True, "12", 0, 65536, None],
                                "token": ["a" * 63, "A" * 64, "g" * 64, 1, None],
                                "bundle_id": ["", "b" * 129, 1, None]}.items():
                cases += [{**valid, key: value} for value in values]
                cases.append({k: v for k, v in valid.items() if k != key})
            for index, case in enumerate(cases):
                with self.subTest(case=index):
                    P.endpoint_path(state).write_text(json.dumps(case), encoding="utf-8")
                    with self.assertRaisesRegex(OSError, "Invalid Deductum host endpoint") as caught:
                        P.read_endpoint(state)
                    self.assertNotIn(valid["token"], str(caught.exception))
            P.endpoint_path(state).write_text("{", encoding="utf-8")
            with self.assertRaises(OSError):
                P.read_endpoint(state)


class AuthenticatedEndpoint(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.state = Path(self.tmp.name) / "state"
        self.addCleanup(self.tmp.cleanup)
        # On Unix this runs the actual TCP protocol, while retaining native
        # directory security. Native Windows discovery runs the security too.
        if os.name != "nt":
            self.enterContext(patch.object(P, "ensure_state_dir", lambda p: Path(p).mkdir(parents=True, exist_ok=True)))
            self.enterContext(patch.dict(sys.modules, {"_windows": types.SimpleNamespace(
                reject_reparse=lambda p: None, secure_file=lambda p: None)}))
            self.enterContext(patch.object(P, "WINDOWS", True))
        self.calls = []
        self.writers = []
        async def dispatch(reader, writer):
            self.calls.append(True)
            self.writers.append(writer)
            try:
                data = await reader.read()
                writer.write(b"response:" + data)
                await writer.drain()
            finally:
                writer.close()
                await writer.wait_closed()
        self.server = await P.listen(dispatch, self.state, self.state / "unused.sock")
        self.record = P.read_endpoint(self.state)

    async def asyncTearDown(self):
        for writer in self.writers:
            writer.close()
        self.server.close()
        await self.server.wait_closed()
        P.cleanup_endpoint(self.state, self.state / "unused.sock")

    async def raw(self):
        reader, writer = await asyncio.open_connection("127.0.0.1", self.record["port"])
        self.writers.append(writer)
        return reader, writer

    def proof(self, role, client_nonce, server_nonce):
        # This is the specified v2 wire construction, independently of the
        # production parser and proof helpers. Do not print proof/key bytes.
        message = b"deductum-host-v2/" + role + b"\0" + client_nonce + server_nonce
        return hmac.digest(bytes.fromhex(self.record["token"]), message, "sha256").hex().encode("ascii")

    async def challenge(self, reader, writer, client_nonce=b"c" * 64):
        writer.write(client_nonce + b"\n")
        await writer.drain()
        response = await asyncio.wait_for(reader.readexactly(130), 5)
        self.assertEqual((response[64:65], response[129:130]), (b"\n", b"\n"))
        nonce, proof = response[:64], response[65:129]
        self.assertTrue(hmac.compare_digest(proof, self.proof(b"server", client_nonce, nonce)),
                        "endpoint did not prove possession of the state secret")
        return nonce, self.proof(b"client", client_nonce, nonce)

    async def test_authentication_precedes_dispatch_and_half_close_drains_replies(self):
        reader, writer = await self.raw()
        _, proof = await self.challenge(reader, writer)
        writer.write(proof + b"\nhello\x00\xff\r\n")
        await writer.drain()
        writer.write_eof()
        self.assertEqual(await asyncio.wait_for(reader.read(), 5), b"OK\nresponse:hello\x00\xff\r\n")
        self.assertEqual(len(self.calls), 1)

    async def test_wrong_missing_and_partial_prefaces_never_dispatch(self):
        for preface in (b"z" * 64 + b"\n", b"A" * 64 + b"\n", b"a" * 64 + b"!", b"\xff" * 64 + b"\n",
                        b'{"op":"hello"}\n', b""):
            reader, writer = await self.raw()
            writer.write(preface)
            await writer.drain()
            writer.write_eof()
            self.assertEqual(await asyncio.wait_for(reader.read(), 5), b"", "rejected client received protocol data")
            self.assertEqual(self.calls, [], "an unauthenticated stream reached protocol dispatch")
        reader, writer = await self.raw()
        writer.write(b"prefix")
        await writer.drain()
        self.assertEqual(await asyncio.wait_for(reader.read(), P.AUTH_TIMEOUT + 2), b"")
        self.assertEqual(self.calls, [])

    async def test_wrong_client_proofs_and_reflected_server_proofs_never_dispatch(self):
        for wrong in (b"0" * 64, b"g" * 64, self.record["token"].encode(), "reflect"):
            reader, writer = await self.raw()
            nonce, _ = await self.challenge(reader, writer)
            proof = self.proof(b"server", b"c" * 64, nonce) if wrong == "reflect" else wrong
            writer.write(proof + b"\n")
            await writer.drain()
            writer.write_eof()
            self.assertEqual(await asyncio.wait_for(reader.read(), 5), b"", "unauthorized proof received protocol data")
            self.assertEqual(self.calls, [], "invalid or reflected proof reached protocol dispatch")

    async def test_a_valid_public_nonce_without_client_proof_is_closed_on_timeout(self):
        reader, writer = await self.raw()
        await self.challenge(reader, writer)
        writer.write(b"partial")
        await writer.drain()
        self.assertEqual(await asyncio.wait_for(reader.read(), P.AUTH_TIMEOUT + 2), b"")
        self.assertEqual(self.calls, [])

    async def test_a_captured_client_proof_cannot_authenticate_a_new_challenge(self):
        reader, writer = await self.raw()
        first_nonce, captured = await self.challenge(reader, writer)
        writer.write(captured + b"\n")
        await writer.drain()
        writer.write_eof()
        self.assertEqual(await asyncio.wait_for(reader.read(), 5), b"OK\nresponse:")
        reader, writer = await self.raw()
        new_nonce, _ = await self.challenge(reader, writer)
        self.assertNotEqual(first_nonce, new_nonce, "independent connections reused a server nonce")
        writer.write(captured + b"\n")
        await writer.drain()
        writer.write_eof()
        self.assertEqual(await asyncio.wait_for(reader.read(), 5), b"")
        self.assertEqual(len(self.calls), 1, "a replayed client proof reached dispatch")

    async def test_authentication_slots_recover_after_saturation(self):
        held = await asyncio.to_thread(P.connect, self.state, "unused")
        self.addCleanup(held.close)
        pending = [await self.raw() for _ in range(P.MAX_AUTHENTICATING)]
        reader, writer = await self.raw()
        self.assertEqual(await asyncio.wait_for(reader.read(), 2), b"")
        for _, writer in pending:
            writer.close()
            await writer.wait_closed()
        # Wait for the server to observe all closures before consuming a slot.
        await asyncio.gather(*(asyncio.wait_for(r.read(), 2) for r, _ in pending))
        sock = await asyncio.to_thread(P.connect, self.state, "unused")
        try:
            await asyncio.to_thread(sock.shutdown, socket.SHUT_WR)
            self.assertEqual(await asyncio.to_thread(sock.recv, 100), b"response:")
        finally:
            sock.close()
            held.shutdown(socket.SHUT_WR)
            self.assertEqual(await asyncio.to_thread(held.recv, 100), b"response:")
            held.close()
        self.assertEqual(len(self.calls), 2, "an already authenticated stream occupied an authentication slot")

    async def rejected_server(self, response):
        finished = asyncio.Event()
        hellos, extra, errors = [], [], []
        async def fake(reader, writer):
            try:
                hello = await asyncio.wait_for(reader.readexactly(65), 5)
                hellos.append(hello[:64])
                writer.write(response(hello[:64]))
                await writer.drain()
                extra.append(await asyncio.wait_for(reader.read(), P.AUTH_TIMEOUT + 2))
            except Exception as error:
                errors.append(type(error).__name__)
            finally:
                writer.close()
                with contextlib.suppress(ConnectionError):
                    await writer.wait_closed()
                finished.set()
        server = await asyncio.start_server(fake, "127.0.0.1", 0)
        record = {**self.record, "port": server.sockets[0].getsockname()[1]}
        P.endpoint_path(self.state).write_text(json.dumps(record), encoding="utf-8")
        accepted = False
        try:
            try:
                sock = await asyncio.to_thread(P.connect, self.state, "unused")
            except OSError:
                pass
            else:
                accepted = True
                sock.sendall(b'{"op":"hello"}\n')
                sock.close()
            await asyncio.wait_for(finished.wait(), P.AUTH_TIMEOUT + 3)
            self.assertEqual(errors, [], "fake server could not observe the rejected handshake")
            self.assertEqual([len(value) for value in extra], [0],
                             "client released a proof or protocol bytes before validating server identity")
            self.assertFalse(accepted, "an invalid server proof authenticated the endpoint")
            return hellos[0]
        finally:
            server.close()
            await server.wait_closed()

    async def test_bad_server_proofs_never_release_a_client_proof_or_protocol_data(self):
        nonce = b"d" * 64
        responses = [lambda client: nonce + b"\n" + b"0" * 64 + b"\n",
                     lambda client: nonce + b"\n" + self.proof(b"client", client, nonce) + b"\n",
                     lambda client: b"D" * 64 + b"\n" + self.proof(b"server", client, b"D" * 64) + b"\n",
                     lambda client: nonce + b"\n" + b"g" * 64 + b"\n"]
        for index, response in enumerate(responses):
            with self.subTest(case=index):
                await self.rejected_server(response)

    async def test_a_captured_server_proof_cannot_authenticate_to_a_fresh_client(self):
        reader, writer = await self.raw()
        old_client_nonce = b"c" * 64
        writer.write(old_client_nonce + b"\n")
        await writer.drain()
        captured_response = await asyncio.wait_for(reader.readexactly(130), 5)
        writer.close()
        await writer.wait_closed()
        nonce = await self.rejected_server(lambda _: captured_response)
        self.assertNotEqual(nonce, old_client_nonce, "the client reused an authentication nonce")
        self.assertEqual(self.calls, [])

    async def test_bundle_mismatch_requires_explicit_stop_connection_bypass(self):
        record = {**self.record, "bundle_id": "another-installed-bundle"}
        P.endpoint_path(self.state).write_text(json.dumps(record), encoding="utf-8")
        with self.assertRaisesRegex(P.HostBundleMismatch, "stop-host"):
            await asyncio.to_thread(P.connect, self.state, "unused")
        sock = await asyncio.to_thread(P.connect, self.state, "unused", allow_bundle_mismatch=True)
        sock.shutdown(socket.SHUT_WR)
        self.assertEqual(await asyncio.to_thread(sock.recv, 100), b"response:")
        sock.close()

    async def test_failed_security_setup_never_publishes_an_endpoint(self):
        state = self.state / "denied"
        with patch.object(P, "ensure_state_dir", side_effect=PermissionError("fixture denial")):
            with self.assertRaises(PermissionError):
                await P.listen(lambda *a: None, state, state / "unused")
        self.assertFalse(P.endpoint_path(state).exists())

    async def test_a_stale_port_squatter_learns_neither_the_host_token_nor_protocol_data(self):
        # Reuse the actual released port, keeping the owner's stale record.
        # The attacker sees only a public endpoint and cannot read its token.
        self.server.close()
        await self.server.wait_closed()
        captured = bytearray()
        finished = asyncio.Event()
        async def attacker(reader, writer):
            try:
                writer.write(b"OK\n")  # Public acknowledgement is not server identity.
                await writer.drain()
                captured.extend(await asyncio.wait_for(reader.read(), P.AUTH_TIMEOUT + 2))
            finally:
                writer.close()
                with contextlib.suppress(ConnectionError):
                    await writer.wait_closed()
                finished.set()
        squatter = await asyncio.start_server(attacker, "127.0.0.1", self.record["port"])
        accepted = False
        probe = b'{"op":"hello","client":"confidential-fixture"}\n'
        try:
            try:
                sock = await asyncio.to_thread(P.connect, self.state, "unused")
            except OSError:
                pass
            else:
                accepted = True
                sock.sendall(probe)
                sock.close()
            await asyncio.wait_for(finished.wait(), P.AUTH_TIMEOUT + 3)
            # Boolean assertions keep any captured authentication bytes out of
            # failure diagnostics, including the intentionally broken case.
            self.assertFalse(self.record["token"].encode() in captured, "stale-port attacker learned the bearer credential")
            self.assertFalse(probe in captured, "protocol data reached an unauthenticated server")
            self.assertFalse(accepted, "a public OK authenticated the squatted endpoint")
        finally:
            squatter.close()
            await squatter.wait_closed()

    async def test_wrong_or_truncated_authentication_acknowledgement_is_rejected(self):
        for reply in (b"NO\n", b"O", b""):
            authenticated = []
            finished = asyncio.Event()
            async def fake(reader, writer):
                try:
                    client_nonce = (await reader.readexactly(65))[:64]
                    nonce = b"d" * 64
                    writer.write(nonce + b"\n" + self.proof(b"server", client_nonce, nonce) + b"\n")
                    await writer.drain()
                    proof = await reader.readexactly(65)
                    authenticated.append(hmac.compare_digest(proof[:64], self.proof(b"client", client_nonce, nonce)))
                    writer.write(reply)
                    await writer.drain()
                finally:
                    writer.close()
                    await writer.wait_closed()
                    finished.set()
            server = await asyncio.start_server(fake, "127.0.0.1", 0)
            record = {**self.record, "port": server.sockets[0].getsockname()[1]}
            P.endpoint_path(self.state).write_text(json.dumps(record), encoding="utf-8")
            try:
                with self.assertRaisesRegex(OSError, "authentication failed"):
                    await asyncio.to_thread(P.connect, self.state, "unused")
                await asyncio.wait_for(finished.wait(), 5)
                self.assertEqual(authenticated, [True], "acknowledgement test did not reach a mutually authenticated connection")
            finally:
                server.close()
                await server.wait_closed()


class HostLock(unittest.TestCase):
    def test_simultaneous_processes_select_exactly_one_live_lock_owner(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "state" / "host.lock"
            listener = socket.socket()
            listener.bind(("127.0.0.1", 0))
            listener.listen(2)
            listener.settimeout(10)
            code = '''
import socket,sys
sys.path.insert(0,sys.argv[1])
import platform_support as p
with socket.create_connection(('127.0.0.1',int(sys.argv[3])),timeout=10) as gate:
    gate.sendall(b'ready')
    gate.recv(1)
    try:
        lock=p.acquire_host_lock(sys.argv[2])
        result=b'owned'
    except p.HostLockBusy:
        result=b'busy'
    gate.sendall(result)
    gate.recv(1)
'''
            children, gates = [], []
            try:
                for _ in range(2):
                    children.append(subprocess.Popen([sys.executable, "-c", code, str(Path(P.__file__).parent),
                                                      str(path), str(listener.getsockname()[1])],
                                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE))
                for _ in range(2):
                    gate, _ = listener.accept()
                    gate.settimeout(10)
                    gates.append(gate)
                    self.assertEqual(gate.recv(5), b"ready")
                for gate in gates:
                    gate.sendall(b"g")
                outcomes = [gate.recv(5) for gate in gates]
                self.assertEqual(sorted(outcomes), [b"busy", b"owned"], "concurrent first launches did not select one host owner")
                for gate in gates:
                    gate.sendall(b"x")
                for child in children:
                    _, error = child.communicate(timeout=10)
                    self.assertEqual(child.returncode, 0, error.decode(errors="replace"))
            finally:
                for gate in gates:
                    gate.close()
                listener.close()
                for child in children:
                    if child.poll() is None:
                        child.kill()
                        child.wait(10)
                    if child.stdout:
                        child.stdout.close()
                    if child.stderr:
                        child.stderr.close()

    def test_a_second_process_cannot_own_the_live_lock_but_can_succeed_its_owner(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "state" / "host.lock"
            code = ("import sys; sys.path.insert(0,sys.argv[1]); import platform_support as p; "
                    "lock=p.acquire_host_lock(sys.argv[2]); print('owned',flush=True)")
            args = [sys.executable, "-c", code, str(Path(P.__file__).parent), str(path)]
            lock = P.acquire_host_lock(path)
            try:
                contender = subprocess.run(args, capture_output=True, timeout=10)
                self.assertNotEqual(contender.returncode, 0, "another process acquired the live host lock")
                self.assertNotIn(b"owned", contender.stdout)
            finally:
                lock.close()
            successor = subprocess.run(args, capture_output=True, timeout=10)
            self.assertEqual(successor.returncode, 0, successor.stderr.decode(errors="replace"))
            self.assertEqual(successor.stdout.strip(), b"owned")


class ProcessStreams(unittest.IsolatedAsyncioTestCase):
    async def test_managed_process_preserves_binary_streams_and_exit_status(self):
        proc = await P.create_subprocess_exec(sys.executable, FIXTURE, "echo", stdin=asyncio.subprocess.PIPE,
                                             stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        data = b"\x00\xff\n\r\n\x1a" + bytes(range(256))
        stdout, stderr = await asyncio.wait_for(proc.communicate(data), 10)
        self.assertEqual(stdout, data, "managed launcher changed binary standard input/output")
        self.assertEqual(stderr, b"stderr\x00\xff\r\n")
        self.assertEqual(proc.returncode, 17)

    async def test_eof_cooperative_process_stops_without_forced_termination(self):
        proc = await P.create_subprocess_exec(sys.executable, "-c", "import sys; sys.stdin.buffer.read()",
                                             stdin=asyncio.subprocess.PIPE)
        with patch.object(P, "terminate_process_tree", side_effect=AssertionError("EOF should suffice")):
            await asyncio.wait_for(drivers.stop_process(proc), 10)
            await drivers.stop_process(proc)
        self.assertEqual(proc.returncode, 0)


class StdioBridge(unittest.TestCase):
    def test_input_eof_keeps_the_bridge_alive_for_the_last_reply(self):
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(10)
        code = ("import socket,sys; sys.path.insert(0,sys.argv[1]); import platform_support as p; "
                "sys.exit(p.bridge_stdio(socket.create_connection(('127.0.0.1',int(sys.argv[2])))))")
        proc = subprocess.Popen([sys.executable, "-c", code, str(Path(P.__file__).parent), str(listener.getsockname()[1])],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            conn, _ = listener.accept()
            conn.settimeout(10)
            data = b"request\x00\xff\r\n\x1a"
            proc.stdin.write(data)
            proc.stdin.close()
            got = bytearray()
            while True:
                chunk = conn.recv(100)
                if not chunk:
                    break
                got.extend(chunk)
            self.assertEqual(bytes(got), data)
            self.assertIsNone(proc.poll(), "input EOF killed bridge before pending response")
            conn.sendall(b"final\x00\xff\r\n")
            conn.close()
            proc.stdin = None  # Already half-closed; communicate must not flush it.
            stdout, stderr = proc.communicate(timeout=10)
            self.assertEqual(stdout, b"final\x00\xff\r\n")
            self.assertEqual(proc.returncode, 0, stderr.decode(errors="replace"))
        finally:
            listener.close()
            if proc.poll() is None:
                proc.kill()
                proc.wait()
            proc.stdout.close()
            proc.stderr.close()


class AuxiliaryCommands(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "Native complete command-line UTF-16 limit")
    def test_wrapped_command_accepts_the_utf16_boundary_and_rejects_one_more_unit_before_spawn(self):
        arguments = [sys.executable, "-c", "pass"]
        prefix, _ = P.managed_argv(arguments)
        remaining = 32766 - len(subprocess.list2cmdline(prefix).encode("utf-16-le")) // 2 - 1
        self.assertGreater(remaining, 0)
        value = "😀" * (remaining // 2) + "x" * (remaining % 2)
        wrapped, _ = P.managed_argv([*arguments, value])
        self.assertEqual(len(subprocess.list2cmdline(wrapped).encode("utf-16-le")) // 2, 32766)
        result = P.run([*arguments, value], capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        with patch.object(subprocess, "Popen", side_effect=AssertionError("overlong command was spawned")):
            with self.assertRaises(OSError) as caught:
                P.run([*arguments, value + "x"], capture_output=True, timeout=30)
        self.assertIn("32767", str(caught.exception))
        self.assertIn("UTF-16", str(caught.exception))

    def test_strict_text_decoding_failure_reaches_the_caller_without_a_reader_thread_traceback(self):
        for stream in (1, 2):
            for cancel in (None, threading.Event()):
                with self.subTest(stream=stream, cancellable=cancel is not None):
                    thread_errors = []
                    code = f"import os; os.write({stream}, bytes([255]))"
                    with patch.object(threading, "excepthook", side_effect=thread_errors.append):
                        with self.assertRaises(UnicodeDecodeError):
                            P.run([sys.executable, "-c", code], cancel_event=cancel,
                                  capture_output=True, text=True, encoding="utf-8", errors="strict", timeout=10)
                    self.assertEqual(thread_errors, [], "text decoding escaped in a subprocess reader thread")

    def test_cancellable_text_capture_decodes_both_streams_and_normalizes_newlines(self):
        code = ("import os,sys; data=sys.stdin.buffer.read(); os.write(1,data); "
                "os.write(2,'Ω\\r\\nβ\\rγ\\n'.encode('utf-8'))")
        result = P.run([sys.executable, "-c", code], cancel_event=threading.Event(),
                       input="Ω\nβ\n", capture_output=True, text=True, encoding="utf-8", timeout=10, check=True)
        self.assertEqual(result.stdout, "Ω\nβ\n")
        self.assertEqual(result.stderr, "Ω\nβ\nγ\n")
        self.assertEqual(result.returncode, 0)

    def test_cancellable_checked_failure_retains_decoded_stdout_and_stderr(self):
        code = "import os; os.write(1,'Ω'.encode('utf-8')); os.write(2,'β'.encode('utf-8')); raise SystemExit(7)"
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            P.run([sys.executable, "-c", code], cancel_event=threading.Event(),
                  capture_output=True, text=True, encoding="utf-8", timeout=10, check=True)
        self.assertEqual(caught.exception.returncode, 7)
        self.assertEqual(caught.exception.stdout, "Ω")
        self.assertEqual(caught.exception.stderr, "β")

    def test_auxiliary_binary_input_output_and_exit_status_are_preserved(self):
        data = b"\x00\xff\r\n\x1a" + bytes(range(256))
        result = P.run([sys.executable, FIXTURE, "echo"], input=data, capture_output=True, timeout=10)
        self.assertEqual(result.stdout, data)
        self.assertEqual(result.stderr, b"stderr\x00\xff\r\n")
        self.assertEqual(result.returncode, 17)

    def test_already_canceled_auxiliary_work_never_starts_a_backend(self):
        canceled = threading.Event()
        canceled.set()
        with tempfile.TemporaryDirectory() as temp:
            marker = Path(temp) / "started"
            code = "from pathlib import Path; import sys; Path(sys.argv[1]).touch()"
            with self.assertRaises(InterruptedError):
                P.run([sys.executable, "-c", code, str(marker)], cancel_event=canceled, timeout=10)
            self.assertFalse(marker.exists(), "canceled startup ran backend code")


@unittest.skipUnless(os.name == "nt", "Native Windows state DACL and reparse semantics")
class WindowsStateSecurity(unittest.IsolatedAsyncioTestCase):
    def test_broad_state_roots_are_rejected_before_any_acl_mutation(self):
        import _windows as W
        for root in (Path.home(), Path(tempfile.gettempdir()), Path(Path.home().anchor)):
            with self.subTest(root=str(root)), patch.object(W, "set_security", side_effect=AssertionError("broad ACL mutated")):
                with self.assertRaises(OSError):
                    P.ensure_state_dir(root)

    async def test_state_and_endpoint_grant_access_only_to_owner_and_system(self):
        import ctypes as C
        from ctypes import wintypes as T
        import _windows as W
        convert = W.api(W.a, "ConvertSecurityDescriptorToStringSecurityDescriptorW", T.BOOL,
                        T.LPVOID, T.DWORD, T.DWORD, C.POINTER(T.LPWSTR), C.POINTER(T.DWORD))
        with tempfile.TemporaryDirectory() as temp:
            state = Path(temp) / "state"
            state.mkdir()  # A previously ordinary directory must be secured too.
            (state / "host.lock").write_bytes(b"\0")
            permissive = T.LPVOID()
            W.checked(W.convert_sd(f"D:P(A;;FA;;;{W.current_sid()})(A;;FA;;;WD)", 1, C.byref(permissive), None))
            try:
                W.checked(W.set_security(str(state / "host.lock"), 4 | 0x80000000, permissive))
            finally:
                W.local_free(permissive)
            lock = P.acquire_host_lock(state / "host.lock")
            server = await P.listen(lambda *a: None, state, state / "unused")
            try:
                for path in (state, state / "host.lock", P.endpoint_path(state)):
                    owner, descriptor, text = T.LPVOID(), T.LPVOID(), T.LPWSTR()
                    error = W.get_security(str(path), 1, 1 | 4, C.byref(owner), None, None, None, C.byref(descriptor))
                    self.assertEqual(error, 0, "could not inspect fixture owner/DACL")
                    try:
                        W.checked(convert(descriptor, 1, 1 | 4, C.byref(text), None))
                        sddl = text.value
                        grants = re.findall(r"\(([^()]+)\)", sddl)
                        self.assertEqual(len(grants), 2, "private state has additional access grants")
                        self.assertEqual({ace.split(";")[-1] for ace in grants}, {W.current_sid(), "SY"})
                        self.assertTrue(all(ace.split(";")[0] == "A" and ace.split(";")[2] == "FA" for ace in grants))
                        if path == state:
                            self.assertIn("D:P", sddl, "state DACL inherited ancestor permissions")
                    finally:
                        if text:
                            W.local_free(C.cast(text, T.HANDLE))
                        W.local_free(descriptor)
            finally:
                server.close()
                await server.wait_closed()
                lock.close()

    async def test_junction_state_root_is_rejected_before_endpoint_publication(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            target, junction = root / "target", root / "junction"
            target.mkdir()
            result = subprocess.run([os.environ.get("COMSPEC", "cmd.exe"), "/d", "/c", "mklink", "/J",
                                     str(junction), str(target)], capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, "fixture junction setup failed: " + result.stderr)
            try:
                with self.assertRaisesRegex(OSError, "reparse"):
                    await P.listen(lambda *a: None, junction, junction / "unused")
                self.assertFalse(P.endpoint_path(target).exists())
            finally:
                junction.rmdir()

    async def test_another_standard_user_cannot_read_or_replace_private_state(self):
        user = os.environ.get("DEDUCTUM_TEST_OTHER_USER")
        password = os.environ.get("DEDUCTUM_TEST_OTHER_PASSWORD")
        if not user or not password:
            self.skipTest("Cross-user acceptance needs disposable DEDUCTUM_TEST_OTHER_USER/PASSWORD credentials")
        import ctypes as C
        from ctypes import wintypes as T
        import _windows as W
        logon = W.api(W.a, "LogonUserW", T.BOOL, T.LPCWSTR, T.LPCWSTR, T.LPCWSTR,
                      T.DWORD, T.DWORD, C.POINTER(T.HANDLE))
        impersonate = W.api(W.a, "ImpersonateLoggedOnUser", T.BOOL, T.HANDLE)
        revert = W.api(W.a, "RevertToSelf", T.BOOL)
        with tempfile.TemporaryDirectory() as temp:
            state = Path(temp) / "state"
            lock = P.acquire_host_lock(state / "host.lock")
            server = await P.listen(lambda *a: None, state, state / "unused")
            token = T.HANDLE()
            try:
                W.checked(logon(user, ".", password, 2, 0, C.byref(token)))
                def attempt():
                    W.checked(impersonate(token))
                    try:
                        actions = [lambda: list(state.iterdir()), lambda: P.endpoint_path(state).read_bytes(),
                                   lambda: P.endpoint_path(state).write_bytes(b"replacement"),
                                   lambda: open(state / "host.lock", "rb")]
                        denied = []
                        for action in actions:
                            try:
                                result = action()
                                if hasattr(result, "close"):
                                    result.close()
                                denied.append(False)
                            except PermissionError:
                                denied.append(True)
                        return denied
                    finally:
                        W.checked(revert())
                self.assertEqual(await asyncio.to_thread(attempt), [True] * 4,
                                 "another standard user accessed owner state")
            finally:
                if token:
                    W.close(token)
                server.close()
                await server.wait_closed()
                lock.close()


@unittest.skipUnless(os.name == "nt", "Native Windows Job Objects, security and process identities")
class WindowsProcesses(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="Deductum Å proof ")
        self.root = Path(self.tmp.name)
        self.processes, self.handles = [], []

    async def asyncTearDown(self):
        import _windows as W
        (self.root / "release").touch()
        for proc in self.processes:
            if proc.returncode is None:
                P.terminate_process_tree(proc, force=True)
            await proc.wait()
        for handle in self.handles:
            W.close(handle)
        self.tmp.cleanup()

    async def tree(self, mode="tree"):
        import ctypes as C
        from ctypes import wintypes
        import _windows as W
        proc = await P.create_subprocess_exec(sys.executable, FIXTURE, mode, str(self.root),
                                             stdin=asyncio.subprocess.PIPE)
        self.processes.append(proc)
        deadline = time.monotonic() + 10
        while not all((self.root / f"{n}.json").exists() for n in range(3)):
            self.assertLess(time.monotonic(), deadline, "fixture did not publish descendant readiness")
            await asyncio.sleep(.02)
        open_process = W.api(W.k, "OpenProcess", wintypes.HANDLE, wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
        handles = []
        for n in range(3):
            pid = json.loads((self.root / f"{n}.json").read_text())["pid"]
            handle = open_process(0x100000 | 0x410, False, pid)
            self.assertTrue(handle, "could not retain fixture process identity")
            handles.append(handle)
        self.handles.extend(handles)
        return proc, handles

    async def assert_exited(self, handles):
        from ctypes import wintypes
        import _windows as W
        wait = W.api(W.k, "WaitForSingleObject", wintypes.DWORD, wintypes.HANDLE, wintypes.DWORD)
        deadline = time.monotonic() + 10
        while any(wait(handle, 0) == 258 for handle in handles):
            self.assertLess(time.monotonic(), deadline, "owned child remained alive after job teardown")
            await asyncio.sleep(.02)

    async def test_stop_terminates_the_owned_tree_and_preserves_an_unrelated_process(self):
        sentinel = subprocess.Popen([sys.executable, "-c", "import sys; sys.stdin.buffer.read()"], stdin=subprocess.PIPE)
        try:
            proc, handles = await self.tree()
            await asyncio.wait_for(drivers.stop_process(proc), 40)
            await self.assert_exited(handles)
            await drivers.stop_process(proc)
            self.assertIsNone(sentinel.poll(), "Stop terminated an unrelated process")
        finally:
            sentinel.stdin.close()
            sentinel.wait(10)

    async def test_sampling_handles_do_not_keep_descendants_alive_after_launcher_death(self):
        proc, handles = await self.tree()
        import ctypes as C
        from ctypes import wintypes as T
        import _windows as W
        memory = W.api(W.k, "K32GetProcessMemoryInfo", T.BOOL, T.HANDLE, T.LPVOID, T.DWORD)
        open_process = W.api(W.k, "OpenProcess", T.HANDLE, T.DWORD, T.BOOL, T.DWORD)
        launcher = W.checked(open_process(0x410, False, proc.pid))
        self.handles.append(launcher)
        observed_rss = 0
        for handle in [launcher, *handles]:
            counters = W.MEMORY_COUNTERS()
            counters.cb = C.sizeof(counters)
            W.checked(memory(handle, C.byref(counters), counters.cb))
            observed_rss += counters.WorkingSetSize
        samples = [P.process_group_usage(proc.pid) for _ in range(10)]
        self.assertTrue(all(s and s[1] > 32 * 1024 * 1024 for s in samples), "tree RSS omitted allocated descendant")
        self.assertLessEqual(abs(samples[-1][1] - observed_rss), max(observed_rss // 4, 8 * 1024 * 1024),
                             "tree RSS disagrees with independently sampled owned process working sets")
        self.assertEqual(P.clock_ticks(), 10_000_000)
        self.assertEqual(P.page_size(), 1)
        self.assertTrue(all(a[0] <= b[0] for a, b in zip(samples, samples[1:])), "job CPU ticks decreased")
        proc.kill()
        await proc.wait()
        await self.assert_exited(handles)

    async def test_parent_exit_terminates_surviving_children(self):
        # Capture descendants before asking the parent to exit, avoiding PID reuse.
        proc, handles = await self.tree()
        (self.root / "exit-parent").touch()
        await asyncio.wait_for(proc.wait(), 10)
        self.assertEqual(proc.returncode, 0, "ordinary parent exit status was lost")
        await self.assert_exited(handles)

    async def test_owner_crash_terminates_the_tree_even_when_the_launcher_is_alive(self):
        import _windows as W
        from ctypes import wintypes as T
        code = '''
import asyncio, sys
sys.path.insert(0, sys.argv[1])
import platform_support as p
async def main():
    process = await p.create_subprocess_exec(sys.executable, sys.argv[2], 'tree', sys.argv[3])
    await process.wait()
asyncio.run(main())
'''
        owner = subprocess.Popen([sys.executable, "-c", code, str(Path(P.__file__).parent), FIXTURE, str(self.root)])
        handles = []
        try:
            deadline = time.monotonic() + 10
            while not all((self.root / f"{n}.json").exists() for n in range(3)):
                self.assertIsNone(owner.poll(), "fixture owner exited before descendant readiness")
                self.assertLess(time.monotonic(), deadline, "owned tree never became ready")
                await asyncio.sleep(.02)
            open_process = W.api(W.k, "OpenProcess", T.HANDLE, T.DWORD, T.BOOL, T.DWORD)
            for n in range(3):
                pid = json.loads((self.root / f"{n}.json").read_text())["pid"]
                handle = W.checked(open_process(0x100000 | 0x411, False, pid))
                handles.append(handle)
                self.handles.append(handle)
            owner.kill()
            owner.wait(10)
            await self.assert_exited(handles)
        finally:
            if owner.poll() is None:
                owner.kill()
                owner.wait(10)

    async def test_auxiliary_command_timeout_terminates_its_descendants(self):
        await self.auxiliary_termination(cancel=False)

    async def test_auxiliary_command_cancellation_terminates_its_descendants(self):
        await self.auxiliary_termination(cancel=True)

    async def auxiliary_termination(self, cancel):
        import _windows as W
        from ctypes import wintypes as T
        canceled = threading.Event() if cancel else None
        work = asyncio.create_task(asyncio.to_thread(P.run, [sys.executable, FIXTURE, "tree", str(self.root)],
                                                      capture_output=True, timeout=8 if not cancel else 30,
                                                      cancel_event=canceled))
        handles = []
        try:
            deadline = time.monotonic() + 6
            while not all((self.root / f"{n}.json").exists() for n in range(3)):
                self.assertFalse(work.done(), "auxiliary fixture ended before readiness")
                self.assertLess(time.monotonic(), deadline, "auxiliary tree never became ready")
                await asyncio.sleep(.02)
            open_process = W.api(W.k, "OpenProcess", T.HANDLE, T.DWORD, T.BOOL, T.DWORD)
            for n in range(3):
                pid = json.loads((self.root / f"{n}.json").read_text())["pid"]
                handle = W.checked(open_process(0x100000 | 0x410, False, pid))
                handles.append(handle)
                self.handles.append(handle)
            if canceled:
                canceled.set()
            with self.assertRaises(InterruptedError if cancel else subprocess.TimeoutExpired):
                await asyncio.wait_for(asyncio.shield(work), 12)
            await self.assert_exited(handles)
        finally:
            if canceled:
                canceled.set()
            await asyncio.gather(work, return_exceptions=True)

    async def test_failed_backend_start_reports_failure_without_creating_descendants(self):
        missing = str(self.root / "not-an-installed-backend.exe")
        proc = await P.create_subprocess_exec(missing, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        self.processes.append(proc)
        _, error = await asyncio.wait_for(proc.communicate(), 10)
        self.assertNotEqual(proc.returncode, 0, "missing backend startup unexpectedly succeeded")
        self.assertTrue(error, "launcher hid the backend startup failure")
        self.assertFalse(list(self.root.glob("*.json")), "failed startup created fixture descendants")



@unittest.skipIf(os.name == "nt", "Windows counts a job, not a process group")
class ProverUsageTests(unittest.TestCase):
    def test_a_child_in_a_session_of_its_own_is_counted(self):
        # Isabelle's ML process runs outside the JVM's group, and that is where the proofs happen
        child = ("import time; b = bytearray(64 * 1024 * 1024); "
                 "[b.__setitem__(i, 1) for i in range(0, len(b), 4096)]; print('up', flush=True); time.sleep(60)")
        parent = ("import subprocess, sys, time; "
                  f"subprocess.Popen([sys.executable, '-c', {child!r}], start_new_session=True); time.sleep(60)")
        proc = subprocess.Popen([sys.executable, "-c", parent], start_new_session=True, stdout=subprocess.PIPE)
        descendants = []
        try:
            self.assertEqual(proc.stdout.readline().strip(), b"up")
            with open(f"/proc/{proc.pid}/task/{proc.pid}/children") as f:
                descendants = [int(p) for p in f.read().split()]
            self.assertTrue(descendants)
            with open(f"/proc/{descendants[0]}/stat") as f:
                self.assertNotEqual(int(f.read().rsplit(")", 1)[1].split()[2]), proc.pid, "the child has its own group")
            _, pages = P.process_group_usage(proc.pid)
            self.assertGreater(pages * P.page_size(), 64 * 1024 * 1024)
        finally:
            for pid in descendants:
                os.kill(pid, signal.SIGKILL)
            proc.kill()
            proc.wait()
            proc.stdout.close()

if __name__ == "__main__":
    unittest.main()
