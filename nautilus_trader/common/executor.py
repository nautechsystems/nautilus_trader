# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
import uuid
from asyncio import Future
from asyncio import Queue
from collections.abc import Callable
from concurrent.futures import Executor
from dataclasses import dataclass
from typing import Any

from nautilus_trader.common.logging import LoggerAdapter


@dataclass(frozen=True)
class TaskId:
    """
    Represents the identifier for a task executing as a `asyncio.Future`.

    This also corresponds to the future objects memory address, unless the task was
    queued, in which case it is a pre-assigned random integer.

    """

    value: int


class ActorExecutor:
    """
    Provides an executor for `Actor` and `Strategy` classes.

    Parameters
    ----------
    loop : AbstractEventLoop
        The event loop for the application.
    executor : Executor
        The inner executor to register.
    logger : LoggerAdatper
        The logger for the executor.

    Warnings
    --------
    This executor if not thread safe, and must be called from the same thread
    in which it was created.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        executor: Executor,
        logger: LoggerAdapter,
    ):
        self._loop = loop
        self._executor: Executor = executor
        self._log: LoggerAdapter = logger

        self._active_tasks: dict[TaskId, Future[Any]] = {}
        self._queued_tasks: set[TaskId] = set()

        self._queue: Queue = Queue()
        self._worker_task = self._loop.create_task(self._worker())

    async def _worker(self) -> None:
        try:
            while True:
                task_id, func, args, kwargs = await self._queue.get()
                if task_id not in self._queued_tasks:
                    continue  # Already canceled

                task = self._submit_to_executor(func, *args, **kwargs)

                # Use pre-assigned task_id
                self._active_tasks[task_id] = task
                self._log.debug(f"Executor: scheduled {task_id}, {task} ...")

                # Sequentially execute tasks
                await asyncio.wrap_future(self._active_tasks[task_id])
                self._queue.task_done()
        except asyncio.CancelledError:
            self._log.debug("Executor: worker task canceled.")

    def _remove_done_task(self, task: Future[Any]) -> None:
        task_id = TaskId(id(task))
        for active_task_id, active_task in self._active_tasks.items():
            if task == active_task:
                task_id = active_task_id

        if task.done():
            try:
                if task.exception() is not None:
                    self._log.error(f"Exception in {task_id}: {task.exception()}")
            except asyncio.CancelledError:
                self._log.info(f"Task {task_id} was canceled.")
            self._active_tasks.pop(task_id, None)

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
        Enqueue the given callable to be executed sequentially.

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
        task_id = TaskId(id(uuid.uuid4()))
        self._queue.put_nowait((task_id, func, args, kwargs))
        self._queued_tasks.add(task_id)

        return task_id

    def run_in_executor(
        self,
        func: Callable[..., Any],
        *args: Any,
        **kwargs: Any,
    ) -> TaskId:
        """
        Arrange for the given callable to be executed.

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
        self._log.info(f"Executor: {type(func).__name__}({args=}, {kwargs=})")
        task = self._submit_to_executor(func, *args, **kwargs)

        task_id = TaskId(id(task))
        self._active_tasks[task_id] = task
        self._log.debug(f"Executor: scheduled {task_id}, {task} ...")

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
            self._log.info(f"Executor: {task_id} canceled prior to execution.")
            return

        future: Future | None = self._active_tasks.get(task_id)
        if not future:
            self._log.warning(f"Executor: {task_id} not found.")
            return

        result = future.cancel()
        self._log.info(f"Executor: {task_id} canceled {result}.")

    def cancel_all_tasks(self) -> None:
        """
        Cancel all active and queued tasks.
        """
        # Drain queue
        while not self._queue.empty():
            task_id, _, _, _ = self._queue.get_nowait()
            self._log.info(f"Executor: {task_id} dequeued prior to execution.")

        if self._worker_task is not None:
            self._worker_task.cancel()

        for task in self._active_tasks:
            self.cancel_task(task)
