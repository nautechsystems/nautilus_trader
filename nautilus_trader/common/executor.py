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

from __future__ import annotations

import asyncio
import functools
import threading
from asyncio import Future
from asyncio import Queue
from collections.abc import Callable
from concurrent.futures import Executor
from dataclasses import dataclass
from typing import Any

from nautilus_trader.common.component import Logger
from nautilus_trader.core.uuid import UUID4


@dataclass(frozen=True)
class TaskId:
    """
    Represents a unique identifier for a task managed by the `ActorExecutor`.

    This ID can be associated with a task that is either queued for execution or
    actively executing as an `asyncio.Future`.

    """

    value: str

    def __repr__(self) -> str:
        return f"{self.__class__.__name__}('{self.value}')"

    @classmethod
    def create(cls) -> TaskId:
        """
        Create and return a new task identifier with a UUID v4 value.

        Returns
        -------
        TaskId

        """
        return TaskId(str(UUID4()))


class ActorExecutor:
    """
    Provides an executor for `Actor` and `Strategy` classes.

    The executor is designed to handle asynchronous tasks for `Actor` and `Strategy` classes.
    This custom executor queues and executes tasks within a given event loop and is tailored for
    single-threaded applications.

    The `ActorExecutor` maintains its internal state to manage both queued and active tasks,
    providing facilities for scheduling, cancellation, and monitoring. It can be used to create
    more controlled execution flows within complex asynchronous systems.

    Parameters
    ----------
    loop : AbstractEventLoop
        The event loop for the application.
    executor : Executor
        The inner executor to register.
    logger : Logger, optional
        The logger for the executor.

    Warnings
    --------
    This executor is not fully thread-safe. Only `queue_for_executor` can be safely called
    from other threads. All other methods (`cancel_task`, `get_future`, `reset`, etc.) must
    be invoked from the same thread in which the executor was created (the event loop thread).

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        executor: Executor,
        logger: Logger | None = None,
    ):
        self._loop = loop
        self._executor: Executor = executor
        self._log: Logger = logger or Logger(name=type(self).__name__)

        self._active_tasks: dict[TaskId, Future[Any]] = {}
        self._future_index: dict[Future[Any], TaskId] = {}
        self._queued_tasks: set[TaskId] = set()

        self._queue: Queue = Queue()
        self._worker_task = self._loop.create_task(self._worker())

    def reset(self) -> None:
        """
        Reset the executor.

        This will cancel all queued and active tasks, and drain the internal queue
        without executing those tasks.

        """
        self.cancel_all_tasks()

        self._active_tasks.clear()
        self._future_index.clear()
        self._queued_tasks.clear()

    def get_future(self, task_id: TaskId) -> Future | None:
        """
        Return the executing `Future` with the given task_id (if found).

        Parameters
        ----------
        task_id : TaskId
            The task identifier for the future.

        Returns
        -------
        asyncio.Future or ``None``

        """
        return self._active_tasks.get(task_id)

    async def shutdown(self) -> None:
        """
        Shutdown the executor in an async context.

        This will cancel the inner worker task and shutdown the underlying executor.

        """
        self._worker_task.cancel()
        try:
            await asyncio.wait_for(self._worker_task, timeout=2.0)
        except asyncio.CancelledError:
            pass  # Ignore the exception since we intentionally cancelled the task
        except TimeoutError:
            self._log.error("Executor: TimeoutError shutting down worker")

        # Use a dedicated thread to avoid self-join issue when the executor
        # is also the loop's default executor
        def _shutdown_executor():
            self._executor.shutdown(wait=True)

        shutdown_thread = threading.Thread(target=_shutdown_executor, daemon=False)
        shutdown_thread.start()

        while shutdown_thread.is_alive():
            await asyncio.sleep(0.01)

    def _drain_queue(self) -> None:
        # Drain the internal task queue (this will not execute the tasks)
        while not self._queue.empty():
            task_id, _, _, _ = self._queue.get_nowait()
            self._log.debug(f"Executor: Dequeued {task_id} prior to execution")
        self._queued_tasks.clear()

    def _add_active_task(self, task_id: TaskId, task: Future[Any]) -> None:
        self._active_tasks[task_id] = task
        self._future_index[task] = task_id

    async def _worker(self) -> None:
        try:
            while True:
                task_id = None
                item_dequeued = False

                try:
                    task_id, func, args, kwargs = await self._queue.get()
                    item_dequeued = True

                    if task_id not in self._queued_tasks:
                        continue  # Already canceled

                    self._queued_tasks.discard(task_id)

                    task = self._submit_to_executor(func, *args, **kwargs)

                    self._add_active_task(task_id, task)
                    self._log.debug(f"Executor: Scheduled {task_id}, {task}")

                    await asyncio.wrap_future(self._active_tasks[task_id])
                except asyncio.CancelledError:
                    current_task = asyncio.current_task()
                    if current_task and current_task.cancelling() > 0:
                        raise  # Worker shutdown, propagate up
                    self._log.debug(f"Executor: Task {task_id} cancelled during execution")
                finally:
                    # Only call task_done if we actually dequeued an item
                    if item_dequeued:
                        self._queue.task_done()
        except asyncio.CancelledError:
            self._log.debug("Executor: Canceled inner worker task")

    def _remove_done_task(self, task: Future[Any]) -> None:
        task_id = self._future_index.pop(task, None)
        if not task_id:
            self._log.error(f"Executor: {task} not found on done callback")
            return

        self._active_tasks.pop(task_id, None)
        self._queued_tasks.discard(task_id)

        if task.done():
            try:
                if task.exception() is not None:
                    self._log.exception(f"Executor: Exception in {task_id}", task.exception())
                    return
            except asyncio.CancelledError:
                self._log.warning(f"Executor: Canceled {task_id}")
                return

            self._log.debug(f"Executor: Completed {task_id}")

    def _submit_to_executor(
        self,
        func: Callable[..., Any],
        *args: Any,
        **kwargs: Any,
    ) -> Future[Any]:
        partial_func = functools.partial(func, *args, **kwargs)
        task: Future[Any] = self._loop.run_in_executor(self._executor, partial_func)
        task.add_done_callback(self._remove_done_task)
        return task

    def queue_for_executor(
        self,
        func: Callable[..., Any],
        *args: Any,
        **kwargs: Any,
    ) -> TaskId:
        """
        Enqueue the given `func` to be executed sequentially.

        Parameters
        ----------
        func : Callable
            The function to be executed.
        args : positional arguments
            The positional arguments for the call to `func`.
        kwargs : arbitrary keyword arguments
            The keyword arguments for the call to `func`.

        Returns
        -------
        TaskId

        """
        task_id = TaskId.create()
        self._loop.call_soon_threadsafe(self._queue.put_nowait, (task_id, func, args, kwargs))
        self._queued_tasks.add(task_id)

        return task_id

    def run_in_executor(
        self,
        func: Callable[..., Any],
        *args: Any,
        **kwargs: Any,
    ) -> TaskId:
        """
        Arrange for the given `func` to be called in the executor.

        Parameters
        ----------
        func : Callable
            The function to be executed.
        args : positional arguments
            The positional arguments for the call to `func`.
        kwargs : arbitrary keyword arguments
            The keyword arguments for the call to `func`.

        Returns
        -------
        TaskId

        """
        self._log.debug(f"Executor: {type(func).__name__}({args=}, {kwargs=})")
        task: Future = self._submit_to_executor(func, *args, **kwargs)

        task_id = TaskId.create()
        self._active_tasks[task_id] = task
        self._future_index[task] = task_id
        self._log.debug(f"Executor: Scheduled {task_id}, {task}")

        return task_id

    def queued_task_ids(self) -> list[TaskId]:
        """
        Return the queued task identifiers.

        Returns
        -------
        list[TaskId]

        """
        return list(self._queued_tasks)

    def active_task_ids(self) -> list[TaskId]:
        """
        Return the active task identifiers.

        Returns
        -------
        list[TaskId]

        """
        return list(self._active_tasks.keys())

    def has_queued_tasks(self) -> bool:
        """
        Return a value indicating whether there are any queued tasks.

        Returns
        -------
        bool

        """
        return bool(self._queued_tasks)

    def has_active_tasks(self) -> bool:
        """
        Return a value indicating whether there are any active tasks.

        Returns
        -------
        bool

        """
        return bool(self._active_tasks)

    def cancel_task(self, task_id: TaskId) -> None:
        """
        Cancel the task with the given `task_id` (if queued or active).

        If the task is not found then a warning is logged.

        Parameters
        ----------
        task_id : TaskId
            The active task identifier.

        """
        if task_id in self._queued_tasks:
            self._queued_tasks.discard(task_id)
            self._log.debug(f"Executor: Canceled {task_id} prior to execution")
            return

        task: Future | None = self._active_tasks.pop(task_id, None)
        if not task:
            self._log.warning(f"Executor: {task_id} not found")
            return

        self._future_index.pop(task, None)

        result = task.cancel()
        self._log.debug(f"Executor: Canceled {task_id} with result {result}")

    def cancel_all_tasks(self) -> None:
        """
        Cancel all active and queued tasks.
        """
        self._drain_queue()

        if self._worker_task is not None:
            self._worker_task.cancel()

        for task_id in self._active_tasks.copy():
            self.cancel_task(task_id)

        self._worker_task = self._loop.create_task(self._worker())
