"""OS mechanisms for the common host; the protocol and session state stay portable."""
from __future__ import annotations

import asyncio
import contextlib
import errno
import hmac
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import sys
import threading
import time

WINDOWS = os.name == "nt"
AUTH_TIMEOUT = 3.0
MAX_AUTHENTICATING = 16
AUTH_ACK = b"OK\n"
ENDPOINT_VERSION = 2
_jobs = {}


class HostBundleMismatch(OSError):
    """Explicit shutdown is required before selecting a different host bundle."""


class HostLockBusy(OSError):
    """Another host holds the per-user lifetime lock."""


def bundle_id():
    share = Path(__file__).resolve().parent.parent
    try:
        value = json.loads((share / "RELEASE.json").read_text(encoding="utf-8"))["bundle_sha256"]
        if isinstance(value, str) and len(value) == 64 and all(c in "0123456789abcdef" for c in value):
            return value
        raise OSError("Invalid Deductum release manifest")
    except FileNotFoundError:
        return "dev-" + hashlib.sha256(str(share).encode("utf-8")).hexdigest()


def state_dir():
    base = Path(os.environ.get("LOCALAPPDATA") or Path.home() / "AppData/Local") if WINDOWS else \
        Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")
    default = base / "deductum" / "state" if WINDOWS else base / "deductum"
    return Path(os.environ.get("DEDUCTUM_STATE_DIR") or default)


def config_dir():
    if os.environ.get("DEDUCTUM_CONFIG_DIR"):
        return Path(os.environ["DEDUCTUM_CONFIG_DIR"])
    base = Path(os.environ.get("APPDATA") or Path.home() / "AppData/Roaming") if WINDOWS else \
        Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    return base / "deductum"


def ensure_state_dir(path):
    if WINDOWS:
        from _windows import secure_directory
        secure_directory(path)
    else:
        Path(path).mkdir(parents=True, exist_ok=True)


def acquire_host_lock(path):
    ensure_state_dir(Path(path).parent)
    if WINDOWS:
        from _windows import reject_reparse, secure_file
        reject_reparse(Path(path))
        if Path(path).exists():
            secure_file(path)
    lock = open(path, "a+b")
    try:
        if WINDOWS:
            import msvcrt
            secure_file(path)
            if os.fstat(lock.fileno()).st_size == 0:
                lock.write(b"\0")
                lock.flush()
            lock.seek(0)
            msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return lock
    except OSError as error:
        lock.close()
        if error.errno in (errno.EACCES, errno.EAGAIN):
            raise HostLockBusy("Deductum host already running") from None
        raise
    except BaseException:
        lock.close()
        raise


def endpoint_path(state):
    return Path(state) / "host-endpoint.json"


def read_endpoint(state):
    ensure_state_dir(state)
    if WINDOWS:
        from _windows import reject_reparse, secure_file
        reject_reparse(endpoint_path(state))
        secure_file(endpoint_path(state))
    try:
        record = json.loads(endpoint_path(state).read_text(encoding="utf-8"))
        if type(record.get("version")) is not int or record["version"] != ENDPOINT_VERSION or \
                type(record.get("port")) is not int or not 1 <= record["port"] <= 65535:
            raise ValueError()
        token = record.get("token")
        if not isinstance(token, str) or len(token) != 64 or any(c not in "0123456789abcdef" for c in token):
            raise ValueError()
        if not isinstance(record.get("bundle_id"), str) or not 1 <= len(record["bundle_id"]) <= 128:
            raise ValueError()
        return record
    except (ValueError, AttributeError, TypeError) as error:
        raise OSError("Invalid Deductum host endpoint record") from None


def _auth_field(value):
    if len(value) != 65 or value[-1:] != b"\n" or any(c not in b"0123456789abcdef" for c in value[:64]):
        raise ValueError("Invalid authentication field")
    return value[:64]


def _auth_proof(key, role, client_nonce, server_nonce):
    return hmac.digest(key, b"deductum-host-v2/" + role + b"\0" + client_nonce + server_nonce, "sha256").hex().encode("ascii")


async def listen(callback, state, unix_path, limit=64 * 1024 * 1024):
    ensure_state_dir(state)
    if not WINDOWS:
        with contextlib.suppress(FileNotFoundError):
            Path(unix_path).unlink()
        server = await asyncio.start_unix_server(callback, path=str(unix_path), limit=limit)
        os.chmod(unix_path, 0o600)
        return server
    token = secrets.token_hex(32)
    key = bytes.fromhex(token)
    pending = 0

    async def authenticate(reader, writer):
        nonlocal pending
        if pending >= MAX_AUTHENTICATING:
            writer.close()
            return
        pending += 1
        accepted = False
        try:
            client_nonce = _auth_field(await asyncio.wait_for(reader.readexactly(65), AUTH_TIMEOUT))
            server_nonce = secrets.token_hex(32).encode("ascii")
            writer.write(server_nonce + b"\n" + _auth_proof(key, b"server", client_nonce, server_nonce) + b"\n")
            await asyncio.wait_for(writer.drain(), AUTH_TIMEOUT)
            proof = _auth_field(await asyncio.wait_for(reader.readexactly(65), AUTH_TIMEOUT))
            if not hmac.compare_digest(proof, _auth_proof(key, b"client", client_nonce, server_nonce)):
                return
            writer.write(AUTH_ACK)
            await asyncio.wait_for(writer.drain(), AUTH_TIMEOUT)
            accepted = True
        except (asyncio.TimeoutError, asyncio.IncompleteReadError, ConnectionError, ValueError):
            pass
        finally:
            pending -= 1
            if not accepted:
                writer.close()
        if accepted:
            await callback(reader, writer)

    server = await asyncio.start_server(authenticate, "127.0.0.1", 0, limit=limit)
    record = {"version": ENDPOINT_VERSION, "port": server.sockets[0].getsockname()[1], "token": token,
              "bundle_id": bundle_id()}
    path = endpoint_path(state)
    temp = path.with_name(".host-endpoint-" + secrets.token_hex(8))
    try:
        with open(temp, "x", encoding="utf-8") as f:
            json.dump(record, f)
            f.flush()
            os.fsync(f.fileno())
        os.replace(temp, path)
    except BaseException:
        server.close()
        await server.wait_closed()
        raise
    finally:
        with contextlib.suppress(FileNotFoundError):
            temp.unlink()
    return server


def connect(state, unix_path, allow_bundle_mismatch=False):
    if not WINDOWS:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            sock.connect(str(unix_path))
            return sock
        except BaseException:
            sock.close()
            raise
    record = read_endpoint(state)
    sock = socket.create_connection(("127.0.0.1", record["port"]), AUTH_TIMEOUT)
    try:
        def receive(size):
            data = b""
            while len(data) < size:
                part = sock.recv(size - len(data))
                if not part:
                    raise OSError("Deductum host authentication failed")
                data += part
            return data
        try:
            # A stale port can belong to another user after a crash. Prove
            # server identity before releasing a client proof or application data.
            key = bytes.fromhex(record["token"])
            client_nonce = secrets.token_hex(32).encode("ascii")
            sock.sendall(client_nonce + b"\n")
            response = receive(130)
            server_nonce, proof = _auth_field(response[:65]), _auth_field(response[65:])
            if not hmac.compare_digest(proof, _auth_proof(key, b"server", client_nonce, server_nonce)):
                raise ValueError("Invalid server proof")
            sock.sendall(_auth_proof(key, b"client", client_nonce, server_nonce) + b"\n")
            if receive(len(AUTH_ACK)) != AUTH_ACK:
                raise ValueError("Invalid authentication acknowledgement")
        except (OSError, ValueError):
            raise OSError("Deductum host authentication failed") from None
        if not allow_bundle_mismatch and record["bundle_id"] != bundle_id():
            raise HostBundleMismatch("Another Deductum version owns the running host. Run deductum_host.py "
                                     "stop-host to safely flush and stop it, then launch this version again.")
        sock.settimeout(None)
        return sock
    except BaseException:
        sock.close()
        raise


def cleanup_endpoint(state, unix_path):
    path = endpoint_path(state) if WINDOWS else Path(unix_path)
    with contextlib.suppress(FileNotFoundError):
        path.unlink()


def spawn_detached(script, log_path):
    with open(log_path, "ab") as log:
        options = {"creationflags": subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP} if WINDOWS else \
            {"start_new_session": True}
        python_flags = ["-X", "utf8"] if WINDOWS else []
        subprocess.Popen([sys.executable, *python_flags, script, "daemon"], stdin=subprocess.DEVNULL,
                         stdout=log, stderr=log, close_fds=True, **options)


def bridge_stdio(sock):
    """Portable half-close bridge; Windows selectors cannot watch anonymous pipes."""
    if WINDOWS:
        import msvcrt
        msvcrt.setmode(sys.stdin.fileno(), os.O_BINARY)
        msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
    def send():
        try:
            while True:
                data = os.read(sys.stdin.fileno(), 65536)
                if not data:
                    break
                sock.sendall(data)
        except OSError:
            pass
        finally:
            with contextlib.suppress(OSError):
                sock.shutdown(socket.SHUT_WR)
    threading.Thread(target=send, name="host-stdin", daemon=True).start()
    try:
        while True:
            data = sock.recv(1 << 20)
            if not data:
                return 0
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
    finally:
        sock.close()


def managed_argv(argv):
    from _windows import creation_time
    name = "Local\\Deductum-" + secrets.token_hex(24)
    return [sys.executable, "-X", "utf8", str(Path(__file__).with_name("managed_process.py")), name,
            str(os.getpid()), str(creation_time()), *map(str, argv)], name


async def create_subprocess_exec(*argv, **kwargs):
    if WINDOWS:
        kwargs.pop("start_new_session", None)
        argv, job = managed_argv(argv)
        kwargs["creationflags"] = kwargs.get("creationflags", 0) | subprocess.CREATE_NO_WINDOW
        proc = await asyncio.create_subprocess_exec(*argv, **kwargs)
        _jobs[proc.pid] = job
        async def forget_job():
            await proc.wait()
            if _jobs.get(proc.pid) == job:
                _jobs.pop(proc.pid, None)
        asyncio.create_task(forget_job())
        return proc
    kwargs.setdefault("start_new_session", True)
    return await asyncio.create_subprocess_exec(*argv, **kwargs)


def run(argv, cancel_event=None, **kwargs):
    """Synchronous auxiliary prover commands, including compiler builds."""
    if cancel_event is None and not WINDOWS:
        return subprocess.run(argv, **kwargs)
    job = None
    if WINDOWS:
        argv, job = managed_argv(argv)
        kwargs["creationflags"] = kwargs.get("creationflags", 0) | subprocess.CREATE_NO_WINDOW
    if cancel_event is None:
        return subprocess.run(argv, **kwargs)  # timeout kills launcher and its owned descendants
    if cancel_event.is_set():
        raise InterruptedError("Prover command canceled")
    timeout = kwargs.pop("timeout", None)
    check = kwargs.pop("check", False)
    data = kwargs.pop("input", None)
    if kwargs.pop("capture_output", False):
        kwargs["stdout"] = kwargs["stderr"] = subprocess.PIPE
    if data is not None:
        kwargs["stdin"] = subprocess.PIPE
    if not WINDOWS:
        kwargs.setdefault("start_new_session", True)
    proc = subprocess.Popen(argv, **kwargs)
    if job:
        _jobs[proc.pid] = job
    deadline = time.monotonic() + timeout if timeout is not None else None
    try:
        while True:
            if cancel_event.is_set():
                raise InterruptedError("Prover command canceled")
            if deadline is not None and time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired(argv, timeout)
            try:
                out, err = proc.communicate(data, timeout=0.1)
                break
            except subprocess.TimeoutExpired:
                data = None
        result = subprocess.CompletedProcess(argv, proc.returncode, out, err)
        if check:
            result.check_returncode()
        return result
    finally:
        if proc.poll() is None:
            terminate_process_tree(proc, force=True)
            proc.wait()
        _jobs.pop(proc.pid, None)


def terminate_process_tree(proc, force=False):
    if WINDOWS:
        from _windows import kill_job
        name = _jobs.get(proc.pid)
        if name:
            kill_job(name)
            # A Stop can race the launcher's first instructions, before job creation.
            # Killing the launcher closes its job handle if it has joined already.
            with contextlib.suppress(ProcessLookupError):
                proc.kill()
        elif proc.returncode is None:
            raise RuntimeError("Prover was not launched with Windows process containment")
    else:
        with contextlib.suppress(ProcessLookupError):
            os.killpg(proc.pid, signal.SIGKILL if force else signal.SIGTERM)


def clock_ticks():
    return 10_000_000 if WINDOWS else os.sysconf("SC_CLK_TCK")


def page_size():
    return 1 if WINDOWS else os.sysconf("SC_PAGE_SIZE")


def process_group_usage(pid):
    if WINDOWS:
        from _windows import job_usage
        name = _jobs.get(pid)
        return job_usage(name) if name else None
    ticks = rss_pages = 0
    try:
        for entry in os.listdir("/proc"):
            if not entry.isdigit():
                continue
            try:
                with open(f"/proc/{entry}/stat") as f:
                    fields = f.read().rsplit(")", 1)[1].split()
                if int(fields[2]) == pid:
                    ticks += int(fields[11]) + int(fields[12])
                    rss_pages += int(fields[21])
            except (OSError, ValueError, IndexError):
                continue
    except OSError:
        return None
    return ticks, rss_pages
