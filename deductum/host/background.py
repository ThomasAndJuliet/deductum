"""Bounded ordered disk work, with daemon workers independent of asyncio's executor.

Timeouts stop waiting, not the filesystem call. The ordered task remains alive
until that call finishes, so a late old write cannot overtake a newer write.
Project work and recovery/journal work use separate instances.
"""
from __future__ import annotations

import asyncio
import queue
import threading
from typing import Any, Callable, Dict, Hashable, Optional


class Busy(Exception):
    pass


def weight(value: Any) -> int:
    """Conservative retained Python data size without serializing large strings."""
    if isinstance(value, str):
        return 64 + 4 * len(value)
    if isinstance(value, dict):
        return 64 + sum(weight(k) + weight(v) + 32 for k, v in value.items())
    if isinstance(value, (list, tuple)):
        return 64 + sum(weight(v) + 8 for v in value)
    return 32


class OrderedIO:
    def __init__(self, name: str, workers: int = 2, pending: int = 128,
                 byte_budget: int = 256 * 1024 * 1024):
        self.name = name
        self.limit = pending
        self.byte_budget = byte_budget
        self.bytes = 0
        self.closed = False
        self.tasks: set = set()
        self.tails: Dict[Hashable, asyncio.Task] = {}
        self.stalled: set = set()
        self.queue: queue.Queue = queue.Queue()
        self.workers = workers
        for i in range(workers):
            threading.Thread(target=self._worker, name=f"deductum-{name}-{i}", daemon=True).start()

    def _worker(self) -> None:
        while True:
            item = self.queue.get()
            if item is None:
                return
            loop, future, operation = item
            try:
                value, error = operation(), None
            except Exception as exn:
                value, error = None, exn
            def finish(future=future, value=value, error=error):
                if not future.done():
                    future.set_exception(error) if error else future.set_result(value)
            try:
                loop.call_soon_threadsafe(finish)
            except RuntimeError:
                # The process is shutting down with a stalled project call.
                pass

    def submit(self, group: Hashable, operation: Callable[[], Any], size: int = 0,
               strict: bool = False) -> asyncio.Task:
        if self.closed or group in self.stalled or len(self.tasks) >= self.limit or self.bytes + size > self.byte_budget:
            raise Busy(f"{self.name} work is busy; retry when pending I/O finishes")
        loop = asyncio.get_running_loop()
        previous = self.tails.get(group)
        if strict and previous is not None and previous.done():
            previous.result()
        self.bytes += size
        async def ordered():
            if previous is not None:
                if strict:
                    await asyncio.shield(previous)
                else:
                    try:
                        await asyncio.shield(previous)
                    except Exception:
                        pass
            future = loop.create_future()
            future.add_done_callback(lambda f: f.exception() if not f.cancelled() else None)
            self.queue.put((loop, future, operation))
            return await asyncio.shield(future)
        task = asyncio.create_task(ordered())
        self.tasks.add(task)
        self.tails[group] = task
        def done(finished):
            self.tasks.discard(finished)
            self.bytes -= size
            if self.tails.get(group) is finished:
                self.stalled.discard(group)
                # Retain a failed strict tail: later journal writes must not
                # silently skip the failed record and acknowledge a gap.
                if not strict or finished.cancelled() or finished.exception() is None:
                    self.tails.pop(group, None)
            if not finished.cancelled():
                finished.exception()  # errors are also observed by owners/fences
        task.add_done_callback(done)
        return task

    async def wait(self, task: asyncio.Task, group: Hashable, timeout: Optional[float] = None):
        try:
            return await asyncio.wait_for(asyncio.shield(task), timeout)
        except asyncio.TimeoutError:
            self.stalled.add(group)
            if task.done():
                self.stalled.discard(group)
            raise

    async def flush(self) -> None:
        errors = []
        while self.tasks:
            results = await asyncio.gather(*(asyncio.shield(task) for task in list(self.tasks)), return_exceptions=True)
            errors.extend(r for r in results if isinstance(r, BaseException))
        if errors:
            raise errors[0]

    def close(self) -> None:
        self.closed = True
        # Queued ordered tasks have not touched disk. Active calls continue on
        # their daemon worker, but no late call can enqueue after the sentinels.
        for task in list(self.tasks):
            task.cancel()
        # Daemon workers let process shutdown retain recovery state and exit
        # even when a remote filesystem call never returns.
        for _ in range(self.workers):
            self.queue.put(None)
