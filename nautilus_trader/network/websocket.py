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
from typing import Any, Callable, Optional

import msgspec

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.nautilus_pyo3.network import WebSocketClient as RustWebSocketClient
from nautilus_trader.network.error import MaxRetriesExceeded


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
        self._last_url: Optional[str] = None

        self._handler: Callable[[bytes], None] = handler
        self._heartbeat: Optional[int] = heartbeat
        self._max_retries: int = max_retries
        self._check_interval: float = 1.0
        self._check_task: Optional[asyncio.Task] = None
        self._client: Optional[WebSocketClient] = None

        self._connection_retry_count = 0

    async def post_connection(self) -> None:
        """
        Actions to be performed post connection.

        """
        # Override to implement additional connection related behaviour
        # (sending other messages etc.).

    async def post_reconnection(self) -> None:
        """
        Actions to be performed post reconnection.

        """
        # Override to implement additional reconnection related behaviour
        # (resubscribing etc.).

    async def post_disconnection(self) -> None:
        """
        Actions to be performed post disconnection.

        """
        # Override to implement additional disconnection related behaviour
        # (canceling ping tasks etc.).

    async def _check_connection(self):
        self._log.info(
            f"Scheduled auto-reconnects (max_retries={self._max_retries} with exponential backoff).",
        )
        try:
            while True:
                await asyncio.sleep(self._check_interval)
                if self.is_connected:
                    continue

                self._log.warning(
                    f"Reconnecting {self._connection_retry_count=}, {self._max_retries=} ...",
                )
                if self._max_retries == 0:
                    raise MaxRetriesExceeded("disconnected with no retries configured")
                if self._connection_retry_count > self._max_retries:
                    raise MaxRetriesExceeded(f"max retries of {self._max_retries} exceeded.")
                await self._reconnect_backoff()
                self._connection_retry_count += 1
                self._log.debug(
                    f"Attempting reconnect ({self._connection_retry_count}/{self._max_retries}).",
                )

                await self.reconnect()
                self._connection_retry_count = 0
        except asyncio.CancelledError:
            self._log.debug("`_check_connection` task was canceled.")

    async def _reconnect_backoff(self) -> None:
        if self._connection_retry_count == 0:
            return  # Immediately attempt first reconnect
        backoff = 1.5**self._connection_retry_count
        self._log.debug(f"Exponential backoff: sleeping for {backoff}")
        await asyncio.sleep(backoff)

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
        self._last_url = url
        self._log.info(f"Connecting to {url}")

        # TODO: Raise exception on connection failure and log error
        self._client = await RustWebSocketClient.connect(
            url=url,
            handler=self._handler,
            heartbeat=self._heartbeat,
        )

        self._log.info("Connected.")
        await self.post_connection()
        if self._check_task is None:
            self._check_task = asyncio.create_task(self._check_connection())

    async def reconnect(self) -> None:
        """
        Reconnect the client to the server.
        """
        await self.connect(self._last_url)
        await self.post_reconnection()

    async def disconnect(self) -> None:
        """
        Disconnect the client from the server.
        """
        if not self.is_connected:
            self._log.error("Cannot disconnect websocket, not connected.")
            return
        assert self._client is not None  # Type checking

        self._log.info("Disconnecting...")
        if self._check_task is not None:
            self._check_task.cancel()
            self._check_task = None

        await self._client.disconnect()
        self._log.info("Disconnected.")
        await self.post_disconnection()

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

    async def send_json(self, message: dict[str, Any]) -> None:
        """
        Send the given `message` as JSON format bytes to the server.

        Parameters
        ----------
        message : dict[str, Any]
            The message to send.

        """
        if not self.is_connected:
            self._log.error("Cannot send websocket message, not connected.")
            return
        assert self._client is not None  # Type checking

        await self._client.send(msgspec.json.encode(message))
