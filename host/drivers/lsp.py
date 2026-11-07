"""A small Language Server Protocol client over a subprocess's stdio (standard library only).

JSON-RPC 2.0 with ``Content-Length`` framing: requests with timeouts and cancellation,
notifications, and handlers for what the server sends (notifications and requests). Also
the position arithmetic LSP needs: the host counts code points, LSP counts UTF-16 units
of a ``{line, character}`` pair.
"""

from __future__ import annotations

import asyncio
import bisect
import contextlib
import json
import sys
from typing import Any, Awaitable, Callable, Dict, List, Optional, Tuple

# JSON-RPC error codes the client answers with
METHOD_NOT_FOUND = -32601
REQUEST_CANCELLED = -32800
CONTENT_MODIFIED = -32801
SERVER_CANCELLED = -32802


class LspError(Exception):
    """The server answered a request with an error"""

    def __init__(self, code: int, message: str, data: Any = None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.data = data


class LspClosed(Exception):
    """The server's connection is gone"""


Handler = Callable[[str, Any], Any]

#: What an ``on_request`` handler returns for a request it leaves to the defaults
UNHANDLED = object()


class LspClient:
    """One connection to a language server process.

    ``on_notification(method, params)`` and ``on_request(method, params)`` may be plain
    functions or coroutines; ``on_request`` returns the result (raise ``LspError`` to
    answer with an error, return ``UNHANDLED`` to leave it to the defaults). The defaults
    answer the housekeeping requests every client must (configuration, capability
    registration, progress creation, ``*/refresh``) and refuse the rest."""

    def __init__(self, proc: asyncio.subprocess.Process,
                 on_notification: Optional[Handler] = None,
                 on_request: Optional[Handler] = None,
                 on_exit: Optional[Callable[[Optional[int]], Any]] = None,
                 on_stderr: Optional[Callable[[str], Any]] = None):
        self.proc = proc
        self.on_notification = on_notification
        self.on_request = on_request
        self.on_exit = on_exit
        self.on_stderr = on_stderr
        self.next_id = 1
        self.pending: Dict[int, asyncio.Future] = {}
        self.closed = False
        self.tasks: List[asyncio.Task] = []
        self.reader = asyncio.get_running_loop().create_task(self._read_loop())
        self.err_reader = asyncio.get_running_loop().create_task(self._stderr_loop())

    # ----- sending

    def _write(self, obj: Dict[str, Any]) -> None:
        if self.closed or not self.proc.stdin or self.proc.returncode is not None:
            raise LspClosed("the language server is not running")
        body = json.dumps(obj, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        self.proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)

    def notify(self, method: str, params: Any = None) -> None:
        msg: Dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        with contextlib.suppress(LspClosed):
            self._write(msg)

    def start_request(self, method: str, params: Any = None) -> Tuple[int, asyncio.Future]:
        """Send a request; the id (for ``cancel``) and the future of its result"""
        rid = self.next_id
        self.next_id += 1
        fut = asyncio.get_running_loop().create_future()
        self.pending[rid] = fut
        msg: Dict[str, Any] = {"jsonrpc": "2.0", "id": rid, "method": method}
        if params is not None:
            msg["params"] = params
        try:
            self._write(msg)
        except LspClosed:
            self.pending.pop(rid, None)
            raise
        return rid, fut

    async def request(self, method: str, params: Any = None, timeout: Optional[float] = 30.0) -> Any:
        """A request and its result; on timeout the server is told to cancel it"""
        rid, fut = self.start_request(method, params)
        try:
            return await asyncio.wait_for(fut, timeout)
        except asyncio.TimeoutError:
            self.cancel(rid)
            raise
        except asyncio.CancelledError:
            self.cancel(rid)
            raise
        finally:
            self.pending.pop(rid, None)

    def cancel(self, rid: int) -> None:
        """``$/cancelRequest``: the server need not answer, so the future is dropped too"""
        self.notify("$/cancelRequest", {"id": rid})
        fut = self.pending.pop(rid, None)
        if fut and not fut.done():
            fut.cancel()

    # ----- receiving

    async def _read_message(self) -> Optional[Dict[str, Any]]:
        assert self.proc.stdout
        length = None
        while True:
            line = await self.proc.stdout.readline()
            if not line:
                return None
            line = line.strip()
            if not line:
                if length is None:
                    continue
                break
            name, _, value = line.partition(b":")
            if name.strip().lower() == b"content-length":
                length = int(value.strip())
        body = await self.proc.stdout.readexactly(length)
        return json.loads(body.decode("utf-8"))

    async def _read_loop(self) -> None:
        try:
            while True:
                try:
                    msg = await self._read_message()
                except (ValueError, UnicodeDecodeError):
                    continue
                if msg is None:
                    break
                self._dispatch(msg)
        except (asyncio.IncompleteReadError, ConnectionError):
            pass
        finally:
            self.closed = True
            code = None
            with contextlib.suppress(Exception):
                code = await asyncio.wait_for(self.proc.wait(), 5)
            for fut in list(self.pending.values()):
                if not fut.done():
                    fut.set_exception(LspClosed("the language server stopped"))
            self.pending.clear()
            if self.on_exit:
                self._call(self.on_exit, code)

    async def _stderr_loop(self) -> None:
        assert self.proc.stderr
        while True:
            line = await self.proc.stderr.readline()
            if not line:
                break
            if self.on_stderr:
                self._call(self.on_stderr, line.decode("utf-8", "replace").rstrip())

    def _call(self, fn: Callable[..., Any], *args: Any) -> None:
        """Run a handler (sync or async) without letting its errors stop the reader"""
        try:
            res = fn(*args)
            if asyncio.iscoroutine(res):
                self._spawn(res)
        except Exception as exn:
            print("lsp handler error", repr(exn), file=sys.stderr, flush=True)

    def _spawn(self, coro: Awaitable[Any]) -> asyncio.Task:
        task = asyncio.get_running_loop().create_task(self._guard(coro))
        self.tasks.append(task)
        task.add_done_callback(lambda t: self.tasks.remove(t) if t in self.tasks else None)
        return task

    async def _guard(self, coro: Awaitable[Any]) -> None:
        try:
            await coro
        except asyncio.CancelledError:
            raise
        except Exception as exn:
            print("lsp handler error", repr(exn), file=sys.stderr, flush=True)

    def _dispatch(self, msg: Dict[str, Any]) -> None:
        method = msg.get("method")
        if method is None:  # a response
            fut = self.pending.pop(msg.get("id"), None)
            if fut is None or fut.done():
                return
            if "error" in msg and msg["error"] is not None:
                e = msg["error"]
                fut.set_exception(LspError(e.get("code", 0), e.get("message", ""), e.get("data")))
            else:
                fut.set_result(msg.get("result"))
        elif "id" in msg:
            self._spawn(self._answer(msg))
        elif self.on_notification:
            self._call(self.on_notification, method, msg.get("params"))

    async def _answer(self, msg: Dict[str, Any]) -> None:
        method, params = msg["method"], msg.get("params")
        reply: Dict[str, Any] = {"jsonrpc": "2.0", "id": msg["id"]}
        try:
            res: Any = UNHANDLED
            if self.on_request:
                res = self.on_request(method, params)
                if asyncio.iscoroutine(res):
                    res = await res
            if res is not UNHANDLED:
                reply["result"] = res
            elif method == "workspace/configuration":
                reply["result"] = [None for _ in (params or {}).get("items", [])]
            elif method in ("client/registerCapability", "client/unregisterCapability",
                            "window/workDoneProgress/create") or method.endswith("/refresh"):
                reply["result"] = None
            else:
                raise LspError(METHOD_NOT_FOUND, "Method not found: " + method)
        except LspError as e:
            reply["error"] = {"code": e.code, "message": e.message}
        except Exception as e:
            reply["error"] = {"code": -32603, "message": repr(e)}
        with contextlib.suppress(LspClosed):
            self._write(reply)

    # ----- ending

    async def shutdown(self, timeout: float = 5.0) -> None:
        """``shutdown`` and ``exit``, then stop the process (and its group) if it lingers"""
        import platform_support as platform
        if not self.closed:
            with contextlib.suppress(Exception):
                await self.request("shutdown", None, timeout)
            self.notify("exit")
        with contextlib.suppress(Exception):
            if self.proc.stdin:
                self.proc.stdin.close()
        try:
            await asyncio.wait_for(self.proc.wait(), timeout)
        except asyncio.TimeoutError:
            for force in (False, True):
                with contextlib.suppress(ProcessLookupError):
                    platform.terminate_process_tree(self.proc, force=force)
                try:
                    await asyncio.wait_for(self.proc.wait(), 5)
                    break
                except asyncio.TimeoutError:
                    continue
        for t in [self.reader, self.err_reader] + list(self.tasks):
            t.cancel()


# ---------------------------------------------------------------------------
# Positions: code points of the host's text <-> LSP (line, UTF-16 character)


def utf16_len(s: str) -> int:
    """Length of ``s`` in UTF-16 code units: code points above U+FFFF count twice"""
    n = len(s)
    if s.isascii():
        return n
    return n + sum(1 for c in s if c > "￿")


class TextIndex:
    """Line starts of one text, for converting between code-point offsets and LSP positions.
    Built once per text version; lookups are a bisection plus a scan of one line."""

    def __init__(self, text: str):
        self.text = text
        starts = [0]
        i = text.find("\n")
        while i >= 0:
            starts.append(i + 1)
            i = text.find("\n", i + 1)
        self.starts = starts

    def line_of(self, offset: int) -> int:
        return bisect.bisect_right(self.starts, max(0, min(offset, len(self.text)))) - 1

    def line_end(self, line: int) -> int:
        """Offset of the end of the line, before its newline"""
        if line + 1 < len(self.starts):
            return self.starts[line + 1] - 1
        return len(self.text)

    def to_lsp(self, offset: int) -> Dict[str, int]:
        offset = max(0, min(offset, len(self.text)))
        line = self.line_of(offset)
        return {"line": line, "character": utf16_len(self.text[self.starts[line]:offset])}

    def to_offset(self, line: int, character: int) -> int:
        """The offset of an LSP position; positions past the end of a line or of the text clamp"""
        if line >= len(self.starts):
            return len(self.text)
        if line < 0:
            return 0
        start, end = self.starts[line], self.line_end(line)
        seg = self.text[start:end]
        if seg.isascii():
            return start + min(character, len(seg))
        units = 0
        for i, c in enumerate(seg):
            if units >= character:
                return start + i
            units += 2 if c > "￿" else 1
        return end

    def range_to_lsp(self, start: int, end: int) -> Dict[str, Any]:
        return {"start": self.to_lsp(start), "end": self.to_lsp(end)}

    def range_to_offsets(self, rng: Dict[str, Any]) -> Tuple[int, int]:
        s, e = rng["start"], rng["end"]
        return self.to_offset(s["line"], s["character"]), self.to_offset(e["line"], e["character"])
