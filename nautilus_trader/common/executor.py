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
    queued, in which case it is a random integer.

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

        self._queue: Queue = Queue()
        self._worker_task = self._loop.create_task(self._worker())

    async def _worker(self) -> None:
        try:
            while True:
                task_id, func, args, kwargs = await self._queue.get()
                partial_func = functools.partial(func, *args, **kwargs)
                task: Future[Any] = self._loop.run_in_executor(self._executor, partial_func)
                task.add_done_callback(self._remove_done_task)
                self._log.debug(f"Executor: scheduled {task} ...")

                self._active_tasks[task_id] = task
                await asyncio.wrap_future(task)
                self._queue.task_done()
        except asyncio.CancelledError:
            self._log.debug("Executor: worker task canceled.")

    def queue_for_executor(
        self,
        func: Callable[..., Any],
        *args: Any,
        **kwargs: Any,
    ) -> TaskId:
        """
        Enqueue the given callable to be executed sequentially.
        """
        task_id = TaskId(id(uuid.uuid4()))
        self._queue.put_nowait((task_id, func, args, kwargs))
        return task_id

    def run_in_executor(
        self,
        func: Callable[..., Any],
        *args: Any,
        **kwargs: Any,
    ) -> TaskId:
        self._log.info(f"Executor: {type(func).__name__}({args=}, {kwargs=})")
        partial_func = functools.partial(func, *args, **kwargs)
        task: Future[Any] = self._loop.run_in_executor(self._executor, partial_func)
        task.add_done_callback(self._remove_done_task)
        self._log.debug(f"Executor: scheduled {task} ...")

        task_id = TaskId(id(task))
        self._active_tasks[task_id] = task

        return task_id

    def cancel_task(self, task_id: TaskId) -> None:
        future: Future | None = self._active_tasks.get(task_id)
        if not future:
            self._log.warning(f"Executor: {task_id} not found.")
            return
        result = future.cancel()
        self._log.info(f"Executor: {task_id} canceled {result}.")

    def cancel_all_tasks(self) -> None:
        if self._worker_task is not None:
            self._worker_task.cancel()

        for task in self._active_tasks:
            self.cancel_task(task)

    def active_task_ids(self) -> list[TaskId]:
        return list(self._active_tasks.keys())

    def has_active_tasks(self) -> bool:
        return bool(self._active_tasks)

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
                self._log.info(f"Task {task_id} was cancelled.")
            self._active_tasks.pop(task_id, None)
