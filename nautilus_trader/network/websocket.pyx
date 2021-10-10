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
from typing import Callable, Dict, List, Optional

import aiohttp
from aiohttp import WSMessage

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
        str ws_url not None,
        handler not None: Callable,
        kwargs: Optional[Dict] = None,
    ):
        """
        Initialize a new instance of the ``WebSocketClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        logger : LoggerAdapter
            The logger adapter for the client.
        ws_url : str
            The websocket url to connect to.
        handler : Callable
            The handler for received raw data.
        kwargs : dict, optional
            The additional kwargs to pass to aiohttp.ClientSession._ws_connect().

        Raises
        ------
        ValueError
            If ws_url is not a valid string.

        """
        Condition.valid_string(ws_url, "ws_url")

        self.ws_url = ws_url

        self._loop = loop
        self._log = LoggerAdapter(
            component_name=type(self).__name__,
            logger=logger,
        )
        self._handler = handler
        self._ws_connect_kwargs = kwargs or {}

        self._session: Optional[aiohttp.ClientSession] = None
        self._ws: Optional[aiohttp.ClientWebSocketResponse] = None
        self._tasks: List[asyncio.Task] = []
        self._running = False
        self._stopped = False
        self._trigger_stop = False

    async def connect(self, bint start=True) -> None:
        self._session = aiohttp.ClientSession(loop=self._loop)
        self._log.debug(f"Connecting to websocket: {self.ws_url}")
        self._ws = await self._session.ws_connect(url=self.ws_url, **self._ws_connect_kwargs)
        await self.post_connect()
        if start:
            self._running = True
            task = self._loop.create_task(self.start())
            self._tasks.append(task)

    async def post_connect(self):
        """ Optional post connect to send any connection messages or other. Called before start()"""
        pass

    async def disconnect(self) -> None:
        self._trigger_stop = True
        while not self._stopped:
            await self._sleep0()
        await self._ws.close()
        self._log.debug("Websocket closed")

    async def send(self, raw: bytes) -> None:
        self._log.debug("SEND:" + str(raw))
        await self._ws.send_bytes(raw)

    async def recv(self) -> bytes:
        try:
            resp: WSMessage = await self._ws.receive()
            return resp.data
        except asyncio.IncompleteReadError as ex:
            self._log.exception(ex)
            await self.connect(start=False)

    async def start(self) -> None:
        self._log.debug("Starting recv loop")
        while self._running:
            try:
                raw = await self.recv()
                self._log.debug("[RECV] {raw}")
                if raw is not None:
                    self._handler(raw)
            except Exception as ex:
                # TODO - Handle disconnect? Should we reconnect or throw?
                self._log.exception(ex)
                self._running = False
        self._log.debug("Stopped")
        self._stopped = True

    async def close(self):
        tasks = [task for task in asyncio.all_tasks() if task is not asyncio.current_task()]
        list(map(lambda task: task.cancel(), tasks))
        return await asyncio.gather(*tasks, return_exceptions=True)

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
