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
from typing import Callable, Optional

from nautilus_trader.core.asynchronous import sleep0

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition


cdef class SocketClient:
    """
    Provides a low-level generic socket base client.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    logger : Logger
        The logger for the client.
    host : str
        The host for the client.
    port : int
        The port for the client.
    handler : Callable
        The handler to process the raw bytes read.
    ssl : bool
        If SSL should be used for socket connection.
    crlf : bytes, optional
        The carriage return, line feed delimiter on which to split messages.
    encoding : str, optional
        The encoding to use when sending messages.

    Raises
    ------
    ValueError
        If `host` is not a valid string.
    ValueError
        If `port` is not positive (> 0).
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Logger logger not None: Logger,
        host,
        port,
        handler not None: Callable,
        bint ssl = True,
        bytes crlf = None,
        str encoding = "utf-8",
    ):
        Condition.valid_string(host, "host")
        Condition.positive_int(port, "port")

        self.host = host
        self.port = port
        self.ssl = ssl
        self._loop = loop
        self._log = LoggerAdapter(
            component_name=type(self).__name__,
            logger=logger,
        )
        self._reader: Optional[asyncio.StreamReader] = None
        self._writer: Optional[asyncio.StreamWriter] = None
        self._handler = handler

        self._crlf = crlf or b"\r\n"
        self._encoding = encoding
        self.is_running = False
        self._incomplete_read_count = 0
        self.is_running = False
        self.is_stopped = False
        self.reconnection_count = 0
        self.is_connected = False

    async def connect(self):
        self._log.info("Attempting Connection ..")
        if self.is_connected:
            self._log.info("Already connected.")
            return

        self._log.debug("Opening connections")
        self._reader, self._writer = await asyncio.open_connection(
            host=self.host,
            port=self.port,
            ssl=self.ssl,
        )
        self._log.debug("Running post connect")
        await self.post_connection()
        self._log.debug("Starting main loop")
        self._loop.create_task(self.start())
        self.is_running = True
        self.is_connected = True
        self._log.info("Connected.")

    async def disconnect(self):
        self._log.info("Disconnecting .. ")
        self.stop()
        self._log.debug("Main loop stop triggered.")
        while not self.is_stopped:
            self._log.debug("Waiting for stop")
            await asyncio.sleep(0.25)
        self._log.debug("Stopped, closing connections")
        self._writer.close()
        await self._writer.wait_closed()
        self._log.debug("Connections closed")
        self._reader = None
        self._writer = None
        self.is_connected = False
        self._log.info("Disconnected.")

    def stop(self):
        self.is_running = False

    async def reconnect(self):
        self._log.info("Reconnecting")
        await self.disconnect()
        await self.connect()

    async def post_connection(self):
        """
        The actions to perform post-connection. i.e. sending further connection messages.
        """
        await sleep0()

    async def send(self, bytes raw):
        self._log.debug("[SEND] " + raw.decode())
        self._writer.write(raw + self._crlf)
        await self._writer.drain()

    async def start(self):
        self._log.debug("Starting recv loop")

        cdef:
            bytes partial = b""
            bytes raw = b""
        while self.is_running:
            try:
                raw = await self._reader.readuntil(separator=self._crlf)
                if partial:
                    raw += partial
                    partial = b""
                self._log.debug("[RECV] " + raw.decode())
                self._handler(raw.rstrip(self._crlf))
                self._incomplete_read_count = 0
                await sleep0()
            except asyncio.IncompleteReadError as e:
                partial = e.partial
                self._log.warning(str(e))
                self._incomplete_read_count += 1
                await asyncio.sleep(0.010)
                if self._incomplete_read_count > 10:
                    # Something probably wrong; reconnect
                    self._log.warning(f"Incomplete read error ({self._incomplete_read_count=}), reconnecting.. ({self.reconnection_count=})")
                    self.is_running = False
                    self.reconnection_count += 1
                    self._loop.create_task(self.reconnect())
                    return
                await sleep0()
                continue
            except ConnectionResetError:
                self._loop.create_task(self.reconnect())
                return
        self.is_running = True
