# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
import types
from asyncio import Task
from typing import Callable, List, Optional

import aiohttp
from aiohttp import WSMessage
from aiohttp import WSMsgType

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition


cdef class WebSocketClient:
    """
    Provides a low-level web socket base client.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Logger logger not None: Logger,
        handler not None: Callable[[bytes], None],
        max_retry_connection=0,
    ):
        """
        Initialize a new instance of the ``WebSocketClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        logger : LoggerAdapter
            The logger adapter for the client.
        handler : Callable[[bytes], None]
            The handler for rece    ived raw data.

        """
        self._loop = loop
        self._log = LoggerAdapter(component_name=type(self).__name__, logger=logger)
        self._handler = handler
        self._ws_url = None
        self._ws_kwargs = {}

        self._session: Optional[aiohttp.ClientSession] = None
        self._socket: Optional[aiohttp.ClientWebSocketResponse] = None
        self._tasks: List[asyncio.Task] = []
        self._stopped = False
        self._trigger_stop = False
        self._connection_retry_count = 0
        self._unknown_message_count = 0
        self._max_retry_connection = max_retry_connection
        self.is_connected = False

    async def connect(
        self,
        str ws_url,
        bint start=True,
        **ws_kwargs,
    ) -> None:
        Condition.valid_string(ws_url, "ws_url")

        self._log.debug(f"Connecting to {ws_url}")
        self._session = aiohttp.ClientSession(loop=self._loop)
        self._socket = await self._session.ws_connect(url=ws_url, **ws_kwargs)
        self._ws_url = ws_url
        self._ws_kwargs = ws_kwargs
        await self.post_connect()
        if start:
            task: Task = self._loop.create_task(self.start())
            self._tasks.append(task)
        self.is_connected = True
        self._log.debug("WebSocket connected.")

    async def post_connect(self):
        """
        Actions to be performed post connection.

        This method is called before start(), override to implement additional
        connection related behaviour (sending other messages etc.).
        """
        pass

    async def disconnect(self) -> None:
        self._trigger_stop = True
        await self._socket.close()
        while not self._stopped:
            await self._sleep0()
        self.is_connected = False
        self._log.debug("WebSocket closed.")

    async def send(self, raw: bytes) -> None:
        self._log.debug(f"[SEND] {raw}")
        await self._socket.send_bytes(raw)

    async def recv(self) -> Optional[bytes]:
        try:
            msg: WSMessage = await self._socket.receive()
            if msg.type == WSMsgType.TEXT:
                return msg.data.encode()
            elif msg.type == WSMsgType.BINARY:
                return msg.data
            elif msg.type in (WSMsgType.ERROR, WSMsgType.CLOSE, WSMsgType.CLOSING, WSMsgType.CLOSED):
                if self._trigger_stop is True:
                    return
                self._log.warning(f"Received closing msg {msg}.")
                raise ConnectionAbortedError("WebSocket error or closed")
            else:
                self._log.warning(
                    f"Received unknown data type: {msg.type} data: {msg.data}.",
                )
                self._unknown_message_count += 1
                if self._unknown_message_count > 20:
                    # This shouldn't be happening and we don't want to spam the logger, trigger a reconnection
                    raise ConnectionAbortedError("Too many unknown messages")
                return b""
        except (asyncio.IncompleteReadError, ConnectionAbortedError, RuntimeError) as ex:
            self._log.exception(ex)
            self._log.debug(
                f"Error, attempting reconnection {self._connection_retry_count=} "
                f"{self._max_retry_connection=}",
            )
            if self._connection_retry_count > self._max_retry_connection:
                raise MaxRetriesExceeded(f"Max retries of {self._max_retry_connection} exceeded.")
            await self._reconnect_backoff()
            self._connection_retry_count += 1
            self._log.debug(
                f"Attempting reconnect "
                f"(attempt: {self._connection_retry_count}) after exception.",
            )
            await self.connect(ws_url=self._ws_url, start=False, **self._ws_kwargs)

    async def _reconnect_backoff(self):
        backoff = 2 ** self._connection_retry_count
        self._log.debug(
            f"Exponential backoff attempt "
            f"{self._connection_retry_count}, sleeping for {backoff}",
        )
        await asyncio.sleep(backoff)

    async def start(self) -> None:
        self._log.debug("Starting recv loop...")
        while not self._trigger_stop:
            try:
                raw = await self.recv()
                if raw is None:
                    continue
                self._log.debug(f"[RECV] {raw}")
                if raw is not None:
                    self._handler(raw)
            except Exception as ex:
                # TODO - Handle disconnect? Should we reconnect or throw?
                self._log.exception(ex)
        self._log.debug("Stopped.")
        self._stopped = True

    async def close(self):
        for task in self._tasks:
            self._log.debug(f"Canceling {task}...")
            task.cancel()

    @types.coroutine
    def _sleep0(self):
        # Skip one event loop run cycle.
        #
        # This is equivalent to `asyncio.sleep(0)` however avoids the overhead
        # of the pure Python function call and integer comparison <= 0.
        #
        # Uses a bare 'yield' expression (which Task.__step knows how to handle)
        # instead of creating a Future object.
        yield


class MaxRetriesExceeded(ConnectionError):
    pass
