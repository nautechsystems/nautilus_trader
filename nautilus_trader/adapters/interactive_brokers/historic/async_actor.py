#!/usr/bin/env python3
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

import asyncio
import functools
from collections.abc import Callable

# fmt: off
from collections.abc import Coroutine

import async_timeout

from nautilus_trader.common.actor import Actor
from nautilus_trader.common.actor import ActorConfig
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.config.common import Environment
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.core.uuid import UUID4


# fmt: on


class AsyncActor(Actor):
    def __init__(self, config: ActorConfig):
        super().__init__(config)

        self.environment: Environment | None = Environment.BACKTEST

        # Hot Cache
        self._pending_async_requests: dict[UUID4, asyncio.Event] = {}

        # Initialized in on_start
        self._loop: asyncio.AbstractEventLoop | None = None

    def on_start(self):
        if isinstance(self.clock, LiveClock):
            self.environment = Environment.LIVE

        if self.environment == Environment.LIVE:
            self._loop = asyncio.get_running_loop()
            self.create_task(self._on_start())
        else:
            asyncio.run(self._on_start())

    async def _on_start(self):
        raise NotImplementedError(  # pragma: no cover
            "implement the `_on_start` coroutine",  # pragma: no cover
        )

    def _finish_response(self, request_id: UUID4):
        super()._finish_response(request_id)
        if request_id in self._pending_async_requests:
            self._pending_async_requests[request_id].set()

    async def await_request(self, request_id: UUID4, timeout: int = 30):
        self._pending_async_requests[request_id] = asyncio.Event()
        try:
            async with async_timeout.timeout(timeout):
                await self._pending_async_requests[request_id].wait()
        except asyncio.TimeoutError:
            self.log.error(f"Failed to download data for {request_id}")
        del self._pending_async_requests[request_id]

    def create_task(
        self,
        coro: Coroutine,
        log_msg: str | None = None,
        actions: Callable | None = None,
        success: str | None = None,
    ) -> asyncio.Task:
        """
        Run the given coroutine with error handling and optional callback actions when
        done.

        Parameters
        ----------
        coro : Coroutine
            The coroutine to run.
        log_msg : str, optional
            The log message for the task.
        actions : Callable, optional
            The actions callback to run when the coroutine is done.
        success : str, optional
            The log message to write on actions success.

        Returns
        -------
        asyncio.Task

        """
        log_msg = log_msg or coro.__name__
        self._log.debug(f"Creating task {log_msg}.")
        task = self._loop.create_task(
            coro,
            name=coro.__name__,
        )
        task.add_done_callback(
            functools.partial(
                self._on_task_completed,
                actions,
                success,
            ),
        )
        return task

    def _on_task_completed(
        self,
        actions: Callable | None,
        success: str | None,
        task: asyncio.Task,
    ) -> None:
        if task.exception():
            self._log.error(
                f"Error on `{task.get_name()}`: " f"{task.exception()!r}",
            )
        else:
            if actions:
                try:
                    actions()
                except Exception as e:
                    self._log.error(
                        f"Failed triggering action {actions.__name__} on `{task.get_name()}`: "
                        f"{e!r}",
                    )
            if success:
                self._log.info(success, LogColor.GREEN)
