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
from asyncio import Task
from typing import Callable, Optional

import aiohttp
import msgspec
from aiohttp import WSMessage

from nautilus_trader.core.asynchronous import sleep0
from nautilus_trader.network.error import MaxRetriesExceeded

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition


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
    max_retry_connection : int, default 0
        The number of times to attempt a reconnection.
    pong_msg : bytes, optional
        The pong message expected from the server (used to filter).
    log_send : bool, default False
        If the raw sent bytes for each message should be logged.
    log_recv : bool, default False
        If the raw recv bytes for each message should be logged.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Logger logger not None: Logger,
        handler not None: Callable[[bytes], None],
        int max_retry_connection = 0,
        bytes pong_msg = None,
        bint log_send = False,
        bint log_recv = False,
    ):
        self._loop = loop
        self._log = LoggerAdapter(component_name=type(self).__name__, logger=logger)
        self._ws_url = None
        self._ws_kwargs = {}
        self._handler = handler

        self._session: Optional[aiohttp.ClientSession] = None
        self._ws: Optional[aiohttp.ClientWebSocketResponse] = None
        self._tasks: list[asyncio.Task] = []
        self._pong_msg = pong_msg
        self._log_send = log_send
        self._log_recv = log_recv

        self.is_stopping = False
        self.is_running = False
        self.is_connected = False
        self.max_retry_connection = max_retry_connection
        self.connection_retry_count = 0
        self.unknown_message_count = 0

    async def connect(self, str ws_url, bint start=True, **ws_kwargs) -> None:
        """
        Connect the WebSocket client.

        Will call `post_connection()` prior to starting receive loop.

        Parameters
        ----------
        ws_url : str
            The endpoint URL to connect to.
        start : bool, default True
            If the WebSocket should start its receive loop.
        ws_kwargs : dict
            The optional kwargs for connection.

        Raises
        ------
        ValueError
            If `ws_url` is not a valid string.

        """
        Condition.valid_string(ws_url, "ws_url")

        self._log.debug(f"Connecting WebSocket to {ws_url}")
        self._session = aiohttp.ClientSession(loop=self._loop)
        self._ws_url = ws_url
        self._ws_kwargs = ws_kwargs
        self._ws = await self._session.ws_connect(url=ws_url, **ws_kwargs)
        await self.post_connection()
        if start:
            task: Task = self._loop.create_task(self.start())
            self._tasks.append(task)
            self._log.debug("WebSocket connected.")
        self.is_connected = True

    async def post_connection(self) -> None:
        """
        Actions to be performed post connection.

        """
        # Override to implement additional connection related behaviour
        # (sending other messages etc.).
        pass

    async def reconnect(self) -> None:
        """
        Reconnect the WebSocket client session.

        Will call `post_reconnection()` following connection.

        """
        self._log.debug(f"Reconnecting WebSocket to {self._ws_url}")

        self._ws = await self._session.ws_connect(url=self._ws_url, **self._ws_kwargs)
        await self.post_reconnection()
        self._log.debug("WebSocket reconnected.")

    async def post_reconnection(self) -> None:
        """
        Actions to be performed post reconnection.

        """
        # Override to implement additional reconnection related behaviour
        # (resubscribing etc.).
        pass

    async def disconnect(self) -> None:
        """
        Disconnect the WebSocket client session.

        Will call `post_disconnection()`.

        """
        self._log.debug("Closing WebSocket...")
        self.is_stopping = True
        await self._ws.close()
        while self.is_running:
            await sleep0()
        self.is_connected = False
        await self.post_disconnection()
        self._log.debug("WebSocket closed.")

    async def post_disconnection(self) -> None:
        """
        Actions to be performed post disconnection.

        """
        # Override to implement additional disconnection related behaviour
        # (canceling ping tasks etc.).
        pass

    async def send_json(self, dict msg) -> None:
        await self.send(msgspec.json.encode(msg))

    async def send(self, bytes raw) -> None:
        if self._log_send:
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
                if self.is_stopping is True:
                    return
                self._log.warning(f"[RECV] {msg}.")
                raise ConnectionAbortedError("websocket aiohttp error")
            elif msg_type == CLOSE:  # Received CLOSE from server
                if self.is_stopping is True:
                    return
                self._log.warning(f"[RECV] {msg}.")
                raise ConnectionAbortedError("websocket closed by server")
            elif msg_type == CLOSING or msg_type == CLOSED:  # aiohttp specific
                if self.is_stopping is True:
                    return
                self._log.warning(f"[RECV] {msg}.")
                raise ConnectionAbortedError("websocket aiohttp closing or closed")
            else:
                self._log.warning(
                    f"[RECV] unknown data type: {msg.type}, data: {msg.data}.",
                )
                self.unknown_message_count += 1
                if self.unknown_message_count > 20:
                    self.unknown_message_count = 0  # Reset counter
                    # This shouldn't be happening, trigger a reconnection
                    raise ConnectionAbortedError("Too many unknown messages")
                return b""
        except (asyncio.IncompleteReadError, ConnectionAbortedError, RuntimeError) as e:
            self._log.warning(
                f"{e.__class__.__name__}: Reconnecting {self.connection_retry_count=}, "
                f"{self.max_retry_connection=}",
            )
            if self.max_retry_connection == 0:
                raise
            if self.connection_retry_count > self.max_retry_connection:
                raise MaxRetriesExceeded(f"Max retries of {self.max_retry_connection} exceeded.")
            await self._reconnect_backoff()
            self.connection_retry_count += 1
            self._log.debug(
                f"Attempting reconnect (attempt: {self.connection_retry_count}).",
            )
            try:
                await self.reconnect()
            except aiohttp.ClientConnectorError:
                # Robust to connection errors during reconnect attempts
                pass

    async def _reconnect_backoff(self) -> None:
        if self.connection_retry_count == 0:
            return  # Immediately attempt first reconnect
        cdef double backoff = 1.5 ** self.connection_retry_count
        self._log.debug(
            f"Exponential backoff attempt "
            f"{self.connection_retry_count}, sleeping for {backoff}",
        )
        await asyncio.sleep(backoff)

    async def start(self) -> None:
        self._log.debug("Starting recv loop...")
        self.is_running = True
        cdef bytes raw
        while not self.is_stopping:
            try:
                raw = await self.receive()
                if self._log_recv:
                    self._log.debug(f"[RECV] {raw}.")
                if raw is None:
                    continue
                if self._pong_msg is not None and raw == self._pong_msg:
                    continue  # Filter pong message
                self._handler(raw)
                self.connection_retry_count = 0
            except Exception as e:
                self._log.exception(f"Error on receive", e)
                break
        self._log.debug("Stopped.")
        self.is_running = False

    async def close(self):
        for task in self._tasks:
            self._log.debug(f"Canceling {task}...")
            task.cancel()
