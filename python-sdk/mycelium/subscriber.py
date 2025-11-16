from __future__ import annotations

import asyncio
import queue
from typing import Optional, Type


class Subscriber:
    """Blocking/async interface for consuming typed messages."""

    def __init__(self, message_cls: Type, message_queue: "queue.Queue") -> None:
        self._message_cls = message_cls
        self._queue = message_queue

    @property
    def message_cls(self) -> Type:
        return self._message_cls

    def recv(self, timeout: Optional[float] = None):
        """Blocking receive."""
        return self._queue.get(timeout=timeout)

    def recv_nowait(self):
        return self._queue.get_nowait()

    async def recv_async(self) -> object:
        loop = asyncio.get_running_loop()
        return await loop.run_in_executor(None, self.recv)


__all__ = ["Subscriber"]
