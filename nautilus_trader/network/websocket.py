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

from typing import Callable, Optional

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.nautilus_pyo3.network import WebSocketClient as RustWebSocketClient


class WebSocketClient:
    """
    Provides a base class for asynchronous WebSocket clients.

    The client is capable of automatic heartbeating and reconnects (with max retries).

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    base_url : str
        The base URL for the WebSocket connection.
    heartbeat : int, optional
        The heartbeat interval (seconds), if `None` then no heartbeats will be sent to the server,
        however the client will respond to ping frames with pongs.
    max_retries : int, default 6
        The maximum number of times the client will auto-reconnect before raising an exception.
    name : str, optional
        The custom name for the client.
    """

    def __init__(
        self,
        clock: LiveClock,
        logger: Logger,
        url: str,
        handler: Callable[[bytes], None],
        heartbeat: Optional[int] = None,
        max_retries: int = 6,
        name: Optional[str] = None,
    ) -> None:
        self._clock: LiveClock = clock
        self._log: LoggerAdapter = LoggerAdapter(name or type(self).__name__, logger=logger)
        self._url: str = url

        self._handler = handler
        self._heartbeat = heartbeat
        self._max_retries = max_retries
        self._client: Optional[WebSocketClient] = None

    @property
    def url(self) -> Optional[str]:
        """
        Return the server URL being used by the client.

        Returns
        -------
        str

        """
        return self._url

    @property
    def is_connected(self) -> bool:
        """
        Return whether the client is connected.

        Returns
        -------
        bool

        """
        return self._client is not None and self._client.is_connected

    async def connect(self, url: Optional[str] = None) -> None:
        """
        Connect the client to the server.

        Parameters
        ----------
        url : str, optional
            The URL path override.

        """
        url = url or self._url
        self._log.info(f"Connecting to {url}")

        # TODO: Raise exception on connection failure and log error
        self._client = await RustWebSocketClient.connect(
            url=url,
            handler=self._handler,
            heartbeat=self._heartbeat,
        )

        self._log.info("Connected.")

    async def disconnect(self) -> None:
        """
        Disconnect the client from the server.
        """
        if not self.is_connected:
            self._log.error("Cannot disconnect websocket, not connected.")
            return
        assert self._client is not None  # Type checking

        self._log.info("Closing...")
        await self._client.disconnect()
        self._log.info("Closed.")

    async def send(self, data: bytes) -> None:
        """
        Send the given `data` bytes to the server.

        Parameters
        ----------
        data : bytes
            The message data to send.

        """
        if not self.is_connected:
            self._log.error("Cannot send websocket message, not connected.")
            return
        assert self._client is not None  # Type checking

        await self._client.send(data)
