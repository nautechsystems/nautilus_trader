# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import orjson
from aiohttp import WSMessage

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition

from nautilus_trader.network.error import MaxRetriesExceeded


cpdef enum WSMsgType:
    # websocket spec types
    CONTINUATION = 0x0
    TEXT = 0x1
    BINARY = 0x2
    PING = 0x9
    PONG = 0xA
    CLOSE = 0x8
    # aiohttp specific types
    CLOSING = 0x100
    CLOSED = 0x101
    ERROR = 0x102


cdef class WebSocketClient:
    """
    Provides a low-level web socket base client.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    logger : LoggerAdapter
        The logger adapter for the client.
    handler : Callable[[bytes], None]
        The handler for receiving raw data.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Logger logger not None: Logger,
        handler not None: Callable[[bytes], None],
        max_retry_connection=0,
    ):
        self._loop = loop
        self._log = LoggerAdapter(component_name=type(self).__name__, logger=logger)
        self._ws_url = None
        self._ws_kwargs = {}
        self._handler = handler

        self._session: Optional[aiohttp.ClientSession] = None
        self._ws: Optional[aiohttp.ClientWebSocketResponse] = None
        self._tasks: List[asyncio.Task] = []
        self._stopped = False
        self._stopping = False

        self.is_connected = False
        self.max_retry_connection = max_retry_connection
        self.connection_retry_count = 0
        self.unknown_message_count = 0

    async def connect(
        self,
        str ws_url,
        bint start=True,
        **ws_kwargs,
    ) -> None:
        Condition.valid_string(ws_url, "ws_url")

        self._log.debug(f"Connecting WebSocket to {ws_url}")
        self._session = aiohttp.ClientSession(loop=self._loop)
        self._ws_url = ws_url
        self._ws_kwargs = ws_kwargs
        self._ws = await self._session.ws_connect(url=ws_url, **ws_kwargs)
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
        self._log.debug("Closing WebSocket...")
        self._stopping = True
        await self._ws.close()
        while not self._stopped:
            await self._sleep0()
        self.is_connected = False
        self._log.debug("WebSocket closed.")

    async def send_json(self, dict msg) -> None:
        await self.send(orjson.dumps(msg))

    async def send(self, bytes raw) -> None:
        self._log.debug(f"[SEND] {raw}")
        await self._ws.send_bytes(raw)

    async def receive(self) -> Optional[bytes]:
        cdef WSMsgType msg_type
        try:
            msg: WSMessage = await self._ws.receive()
            msg_type = msg.type
            if msg_type == TEXT:
                return msg.data.encode()  # Current workaround to always return bytes
            elif msg_type == BINARY:
                return msg.data
            elif msg_type == ERROR:  # aiohttp specific
                if self._stopping is True:
                    return
                self._log.warning(f"Received {msg}.")
                raise ConnectionAbortedError("websocket aiohttp error")
            elif msg_type == CLOSE:  # Received CLOSE from server
                if self._stopping is True:
                    return
                self._log.warning(f"Received {msg}.")
                raise ConnectionAbortedError("websocket closed by server")
            elif msg_type == CLOSING or msg_type == CLOSED:  # aiohttp specific
                if self._stopping is True:
                    return
                self._log.warning(f"Received {msg}.")
                raise ConnectionAbortedError("websocket aiohttp closing or closed")
            else:
                self._log.warning(
                    f"Received unknown data type: {msg.type}, data: {msg.data}.",
                )
                self.unknown_message_count += 1
                if self.unknown_message_count > 20:
                    self.unknown_message_count = 0  # Reset counter
                    # This shouldn't be happening, trigger a reconnection
                    raise ConnectionAbortedError("Too many unknown messages")
                return b""
        except (asyncio.IncompleteReadError, ConnectionAbortedError, RuntimeError) as ex:
            self._log.warning(
                f"{ex.__class__.__name__}: Reconnecting {self.connection_retry_count=}, "
                f"{self.max_retry_connection=}",
            )
            if self.max_retry_connection == 0:
                raise
            if self.connection_retry_count > self.max_retry_connection:
                raise MaxRetriesExceeded(f"Max retries of {self.max_retry_connection} exceeded.")
            await self._reconnect_backoff()
            self.connection_retry_count += 1
            self._log.debug(
                f"Attempting reconnect "
                f"(attempt: {self.connection_retry_count}) after exception.",
            )
            self._ws = await self._session.ws_connect(url=self._ws_url, **self._ws_kwargs)

    async def _reconnect_backoff(self):
        cdef int backoff = 2 ** self.connection_retry_count
        self._log.debug(
            f"Exponential backoff attempt "
            f"{self.connection_retry_count}, sleeping for {backoff}",
        )
        await asyncio.sleep(backoff)

    async def start(self) -> None:
        self._log.debug("Starting recv loop...")
        cdef bytes raw
        while not self._stopping:
            try:
                raw = await self.receive()
                if raw is None:
                    continue
                if raw is not None:
                    self._handler(raw)
            except Exception as ex:
                self._log.exception(ex)
                break
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
