# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import asyncio
from typing import Generic, TypeVar
from weakref import WeakSet

from nautilus_trader.common.component import Clock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3 import NANOSECONDS_IN_SECOND


T = TypeVar("T")


class ThrottledEnqueuer(Generic[T]):
    """
    Manages enqueuing messages of type T onto an internal asynchronous queue.

    Parameters
    ----------
    qname : str
        The name of the inner queue  (e.g., "data_queue").
    queue : asyncio.Queue
        The inner asyncio queue to manage.
    loop : asyncio.AbstractEventLoop
        The event loop used for scheduling queue operations.
    clock : Clock
        The clock for throttling log messages.
    logger : Logger
        The logger to use for capacity warning logs.

    """

    def __init__(
        self,
        qname: str,
        queue: asyncio.Queue,
        loop: asyncio.AbstractEventLoop,
        clock: Clock,
        logger: Logger,
    ) -> None:
        self._qname = qname
        self._queue = queue
        self._loop = loop
        self._clock = clock
        self._log = logger
        self._ts_last_logged: int = 0
        self._pending_tasks: WeakSet[asyncio.Task] = WeakSet()

    @property
    def qname(self) -> str:
        """
        Return the name of the inner queue.

        Returns
        -------
        str

        """
        return self._qname

    @property
    def size(self) -> int:
        """
        Return the current inner queue size.

        Returns
        -------
        int

        """
        return self._queue.qsize()

    @property
    def capacity(self) -> int:
        """
        Return the inner queue maximum capacity.

        Returns
        -------
        int

        """
        return self._queue.maxsize

    def enqueue(self, msg: T) -> None:
        """
        Enqueue a message and logs a throttled warning if the queue is at capacity.

        This method ensures that the message is always queued, even if the queue is
        momentarily full (it schedules an asynchronous put).

        Parameters
        ----------
        msg : T
            The message to enqueue.

        """
        # Do not allow None through (None is a sentinel value which stops the queue)
        assert msg is not None, "message was `None` when a value was expected"

        if self._queue.qsize() < self._queue.maxsize:
            self._loop.call_soon_threadsafe(self._enqueue_nowait_safely, self._queue, msg)
            return

        task = self._loop.create_task(self._queue.put(msg))
        task.add_done_callback(self._handle_task_exception)
        self._pending_tasks.add(task)

        # Throttle logging to once per second
        now_ns = self._clock.timestamp_ns()
        if now_ns > self._ts_last_logged + NANOSECONDS_IN_SECOND:
            self._log.warning(
                f"{self._qname} at capacity ({self._queue.qsize():_}/{self._queue.maxsize}), "
                "scheduled asynchronous put() onto queue",
            )
            self._ts_last_logged = now_ns

    def cancel_pending_tasks(self) -> None:
        """
        Cancel all pending async put tasks.

        This should be called during shutdown to prevent "Task was destroyed but it is
        pending!" warnings.

        """
        for task in list(self._pending_tasks):
            if not task.done():
                task.cancel()

    def _handle_task_exception(self, task: asyncio.Task) -> None:
        if task.cancelled():
            return

        exc = task.exception()
        if exc is not None:
            self._log.error(f"Error putting message on {self._qname}: {exc!r}")

    def _enqueue_nowait_safely(self, queue: asyncio.Queue, msg: T) -> None:
        # Attempt put_nowait(msg) and if the queue is full,
        # schedule an async put() as a fallback.
        try:
            queue.put_nowait(msg)
        except asyncio.QueueFull:
            task = asyncio.create_task(queue.put(msg))
            task.add_done_callback(self._handle_task_exception)
            self._pending_tasks.add(task)
