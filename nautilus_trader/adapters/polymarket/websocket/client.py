# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from collections.abc import Awaitable
from collections.abc import Callable
from enum import Enum
from enum import unique
from typing import Any
from weakref import WeakSet

import msgspec

from nautilus_trader.adapters.polymarket.common.credentials import PolymarketWebSocketAuth
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout


@unique
class PolymarketWebSocketChannel(Enum):
    MARKET = "market"
    USER = "user"


class PolymarketWebSocketClient:
    """
    Provides a Polymarket streaming WebSocket client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    base_url : str, optional
        The base URL for the WebSocket connection.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    handler_reconnect : Callable[..., Awaitable[None]], optional
        The callback handler to be called on reconnect.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.

    References
    ----------
    https://docs.polymarket.com/?python#websocket-api

    """

    def __init__(
        self,
        clock: LiveClock,
        base_url: str | None,
        channel: PolymarketWebSocketChannel,
        handler: Callable[[bytes], None],
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        loop: asyncio.AbstractEventLoop,
        auth: PolymarketWebSocketAuth | None = None,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(type(self).__name__)

        self._channel = channel
        self._base_url: str = base_url or "wss://ws-subscriptions-clob.polymarket.com/ws/"
        self._ws_url = self._base_url + channel.value
        self._auth = auth
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop
        self._tasks: WeakSet[asyncio.Task] = WeakSet()

        self._markets: list[str] = []
        self._assets: list[str] = []
        self._client: WebSocketClient | None = None
        self._is_connecting = False
        self._msg_id: int = 0

    @property
    def url(self) -> str:
        """
        Return the server URL being used by the client.

        Returns
        -------
        str

        """
        return self._ws_url

    def is_connected(self) -> bool:
        """
        Return whether the client is connected.

        Returns
        -------
        bool

        """
        return self._client is not None and self._client.is_active()

    def is_disconnected(self) -> bool:
        """
        Return whether the client is disconnected.

        Returns
        -------
        bool

        """
        return not self.is_connected()

    def market_subscriptions(self) -> list[str]:
        """
        Return the current active market (condition_id) subscriptions for the client.

        Returns
        -------
        str

        """
        return self._markets.copy()

    def asset_subscriptions(self) -> list[str]:
        """
        Return the current active asset (token_id) subscriptions for the client.

        Returns
        -------
        str

        """
        return self._assets.copy()

    async def connect(self) -> None:
        """
        Connect a websocket client to the server.
        """
        self._log.debug(f"Connecting to {self._ws_url}")
        self._is_connecting = True

        config = WebSocketConfig(
            url=self._ws_url,
            handler=self._handler,
            heartbeat=10,
            headers=[],
        )

        self._client = await WebSocketClient.connect(
            config=config,
            post_reconnection=self.reconnect,
        )
        self._is_connecting = False
        self._log.info(f"Connected to {self._ws_url}", LogColor.BLUE)

        await self._subscribe_all()

    # TODO: Temporarily sync
    def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        if self._channel == PolymarketWebSocketChannel.USER and not self._markets:
            self._log.error("Cannot reconnect: no market streams for USER channel")
            return

        if self._channel == PolymarketWebSocketChannel.MARKET and not self._assets:
            self._log.error("Cannot reconnect: no asset streams for MARKET channel")
            return

        self._log.warning(f"Reconnected to {self._ws_url}")

        # Re-subscribe to all streams
        task = self._loop.create_task(self._subscribe_all())
        self._tasks.add(task)

        if self._handler_reconnect:
            task = self._loop.create_task(self._handler_reconnect())  # type: ignore
            self._tasks.add(task)

    async def disconnect(self) -> None:
        """
        Disconnect the client from the server.
        """
        await cancel_tasks_with_timeout(self._tasks, self._log)

        if self._client is None:
            self._log.warning("Cannot disconnect: not connected")
            return

        self._log.debug("Disconnecting...")
        await self._client.disconnect()
        self._client = None  # Dispose (will go out of scope)

        self._log.info(f"Disconnected from {self._ws_url}", LogColor.BLUE)

    def subscribe_market(self, condition_id: str) -> None:
        if condition_id in self._markets:
            self._log.warning(f"Cannot subscribe to market {condition_id}: already subscribed")
            return  # Already subscribed

        self._markets.append(condition_id)

    def subscribe_book(self, asset: str) -> None:
        self._subscribe_asset(asset)

    def _subscribe_asset(self, asset: str) -> None:
        if asset in self._assets:
            self._log.warning(f"Cannot subscribe to asset {asset}: already subscribed")
            return  # Already subscribed

        self._assets.append(asset)
        self._log.debug(f"Subscribed to asset {asset}")

    async def _subscribe_all(self) -> None:
        if self._channel == PolymarketWebSocketChannel.USER:
            msg = self._create_subscribe_user_channel_msg(markets=self._markets)
        else:  # MARKET
            msg = self._create_subscribe_market_channel_msg(assets=self._assets)

        await self._send(msg)

    def _create_subscribe_market_channel_msg(self, assets: list[str]) -> dict[str, Any]:
        message = {
            "type": "market",
            "assets_ids": assets,
        }
        return message

    def _create_subscribe_user_channel_msg(self, markets: list[str]) -> dict[str, Any]:
        message = {
            "auth": self._auth,
            "type": "user",
            "markets": markets,
        }
        return message

    async def _send(self, msg: dict[str, Any]) -> None:
        if self._client is None:
            self._log.error(f"Cannot send message {msg}: not connected")
            return

        self._log.debug(f"SENDING: {msg}")

        try:
            await self._client.send_text(msgspec.json.encode(msg))
        except WebSocketClientError as e:
            self._log.error(str(e))
