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
from typing import Callable, Optional

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition


cdef bytes DEFAULT_CRLF = b"\r\n"


cdef class SocketClient:
    """
    Provides a low level generic socket base client.
    """

    def __init__(
        self,
        host,
        port,
        loop not None: asyncio.AbstractEventLoop,
        handler not None: Callable,
        Logger logger not None: Logger,
        bint ssl=True,
        str encoding="utf-8",
        bytes crlf=None,
    ):
        """
        Initialize a new instance of the ``WebSocketClient`` class.

        Parameters
        ----------
        host : str
            The host for the client.
        port : int
            The port for the client.
        logger : Logger
            The logger for the client.
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        handler : Callable
            The handler to process the raw bytes read.
        ssl : bool
            If SSL should be used for socket connection.
        encoding : str. optional
            The encoding to use when sending messages.
        crlf : bytes, optional
            The carriage return, line feed delimiter on which to split messages.

        Raises
        ------
        ValueError
            If host is not a valid string.
        ValueError
            If port is not in range [0, 65535].

        """
        # Condition.valid_string(host, "host")  # TODO(cs): Temporary
        # Condition.valid_port(port, "port")  # TODO(cs): Temporary

        self.host = host
        self.port = port
        self.ssl = ssl
        self._loop = loop
        self._reader: Optional[asyncio.StreamReader] = None
        self._writer: Optional[asyncio.StreamWriter] = None
        self._handler = handler
        self._log = LoggerAdapter(
            component_name=type(self).__name__,
            logger=logger,
        )

        self._crlf = crlf or DEFAULT_CRLF
        self._encoding = encoding
        self._running = False
        self._stopped = False
        self.is_connected = False

    async def connect(self):
        if not self.is_connected:
            self._reader, self._writer = await asyncio.open_connection(
                host=self.host,
                port=self.port,
                loop=self._loop,
                ssl=self.ssl,
            )
            await self.post_connection()
            self._loop.create_task(self.start())
            self._running = True
            self.is_connected = True

    async def disconnect(self):
        self.stop()
        while not self._stopped:
            await asyncio.sleep(0.01)
        self._writer.close()
        await self._writer.wait_closed()
        self._reader = None
        self._writer = None
        self.is_connected = False

    def stop(self):
        self._running = False

    async def reconnect(self):
        await self.disconnect()
        await self.connect()

    async def post_connection(self):
        """
        The actions to perform post-connection. i.e. sending further connection messages.
        """
        await asyncio.sleep(0)

    async def send(self, bytes raw):
        self._log.debug("[SEND] " + raw.decode())
        self._writer.write(raw + self._crlf)
        await self._writer.drain()

    async def start(self):
        cdef bytes partial = b""
        cdef bytes raw

        self._log.debug("Starting recv loop")

        while self._running:
            try:
                raw = await self._reader.readuntil(separator=self._crlf)
                if partial:
                    raw = partial + raw
                    partial = b""
                self._log.debug("[RECV] " + raw.decode())
                self._handler(raw.rstrip(self._crlf))
                await asyncio.sleep(0)
            except asyncio.IncompleteReadError as ex:
                partial = ex.partial
                self._log.warning(str(ex))
                await asyncio.sleep(0)
                continue
            except ConnectionResetError:
                await self.connect()
        self._stopped = True
