# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Provide a dYdX streaming WebSocket client.
"""

import asyncio
from collections.abc import Callable
from typing import Any

import msgspec

from nautilus_trader.adapters.dydx.common.enums import DYDXCandlesResolution
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig


class DYDXWebsocketClient:
    """
    Provide a dYdX streaming WebSocket client.

    Parameters
    ----------
    clock : LiveClock
        The clock instance.
    base_url : str
        The base URL for the WebSocket connection.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    handler_reconnect : Callable[..., Awaitable[None]], optional
        The callback handler to be called on reconnect.

    """

    def __init__(
        self,
        clock: LiveClock,
        base_url: str,
        handler: Callable[[bytes], None],
        loop: asyncio.AbstractEventLoop,
    ) -> None:
        """
        Provide a dYdX streaming WebSocket client.
        """
        self._clock = clock
        self._log: Logger = Logger(name=type(self).__name__)
        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._loop = loop
        self._client: WebSocketClient | None = None
        self._is_running = False
        self._subscriptions: set[tuple[str, str]] = set()

    @property
    def subscriptions(self) -> set[tuple[str, str]]:
        """
        Return the list of subscriptions.
        """
        return self._subscriptions

    def has_subscription(self, item: tuple[str, str]) -> bool:
        """
        Return true if the connection is already subscribed to this topic.
        """
        return item in self._subscriptions

    async def connect(self) -> None:
        """
        Connect to the websocket server.
        """
        self._is_running = True
        self._log.debug(f"Connecting to {self._base_url} websocket stream")
        config = WebSocketConfig(
            url=self._base_url,
            handler=self._handler,
            heartbeat=10,
            headers=[],
            ping_handler=self._handle_ping,
        )
        client = await WebSocketClient.connect(
            config=config,
            post_reconnection=self.reconnect,
        )
        self._client = client
        self._log.info(f"Connected to {self._base_url}", LogColor.BLUE)

    def _handle_ping(self, raw: bytes) -> None:
        self._loop.create_task(self.send_pong(raw))

    async def send_pong(self, raw: bytes) -> None:
        """
        Send the given raw payload to the server as a PONG message.
        """
        if self._client is None:
            return

        await self._client.send_pong(raw)

    def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        if not self._is_running:
            return

        self._log.warning(f"Reconnected to {self._base_url}")

        # Re-subscribe to all streams
        self._loop.create_task(self._subscribe_all())

    async def disconnect(self) -> None:
        """
        Close the websocket connection.
        """
        self._is_running = False

        if self._client is None:
            self._log.warning("Cannot disconnect: not connected.")
            return

        try:
            await self._client.disconnect()
        except WebSocketClientError as e:
            self._log.error(f"Failed to close websocket connection: {e}")

        self._client = None  # Dispose (will go out of scope)

        self._log.info(f"Disconnected from {self._base_url}", LogColor.BLUE)

    async def subscribe_trades(self, symbol: str) -> None:
        """
        Subscribe to trades messages.
        """
        if self._client is None:
            self._log.warning("Cannot subscribe to trades: not connected")
            return

        subscription = ("v4_trades", symbol)
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.add(subscription)
        msg = {"type": "subscribe", "channel": "v4_trades", "id": symbol}
        self._log.debug(f"Subscribe to {symbol} trade ticks")
        await self._send(msg)

    async def subscribe_order_book(self, symbol: str) -> None:
        """
        Subscribe to trades messages.
        """
        if self._client is None:
            self._log.warning("Cannot subscribe to order book: not connected")
            return

        subscription = ("v4_orderbook", symbol)
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.add(subscription)
        msg = {"type": "subscribe", "channel": "v4_orderbook", "id": symbol, "batched": True}
        self._log.debug(f"Subscribe to {symbol} order book")
        await self._send(msg)

    async def subscribe_klines(self, symbol: str, interval: DYDXCandlesResolution) -> None:
        """
        Subscribe to klines.
        """
        if self._client is None:
            self._log.warning("Cannot subscribe to klines: not connected")
            return

        subscription = ("v4_candles", f"{symbol}/{interval.value}")
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.add(subscription)
        msg = {"type": "subscribe", "channel": "v4_candles", "id": f"{symbol}/{interval.value}"}
        await self._send(msg)

    async def subscribe_markets(self) -> None:
        """
        Subscribe to instrument updates.
        """
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = "v4_markets"
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': not subscribed")
            return

        self._subscriptions.add((subscription, ""))
        msg = {"type": "subscribe", "channel": "v4_markets"}
        await self._send(msg)

    async def subscribe_account_update(self, wallet_address: str, subaccount_number: int) -> None:
        """
        Subscribe to realtime information about orders, fills, transfers, perpetual
        positions, and perpetual assets for a subaccount.
        """
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        channel = "v4_subaccounts"
        channel_id = f"{wallet_address}/{subaccount_number}"

        subscription = (channel, channel_id)
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': not subscribed")
            return

        self._subscriptions.add(subscription)
        msg = {"type": "subscribe", "channel": channel, "id": channel_id}
        await self._send(msg)

    async def unsubscribe_account_update(self, wallet_address: str, subaccount_number: int) -> None:
        """
        Unsubscribe from account messages.
        """
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        channel = "v4_subaccounts"
        channel_id = f"{wallet_address}/{subaccount_number}"

        subscription = (channel, channel_id)
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        msg = {"type": "unsubscribe", "channel": channel, "id": channel_id}
        await self._send(msg)

    async def unsubscribe_trades(self, symbol: str) -> None:
        """
        Unsubscribe to trades messages.
        """
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = ("v4_trades", symbol)
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        msg = {"type": "unsubscribe", "channel": "v4_trades", "id": symbol}
        await self._send(msg)

    async def unsubscribe_order_book(self, symbol: str) -> None:
        """
        Unsubscribe to trades messages.
        """
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = ("v4_orderbook", symbol)
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        msg = {"type": "unsubscribe", "channel": "v4_orderbook", "id": symbol}
        await self._send(msg)

    async def unsubscribe_klines(self, symbol: str, interval: DYDXCandlesResolution) -> None:
        """
        Unsubscribe to trades messages.
        """
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = ("v4_candles", f"{symbol}/{interval.value}")
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        msg = {"type": "unsubscribe", "channel": "v4_candles", "id": f"{symbol}/{interval.value}"}
        await self._send(msg)

    async def unsubscribe_markets(self) -> None:
        """
        Unsubscribe from instrument updates.
        """
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = ("v4_markets", "")
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        msg = {"type": "unsubscribe", "channel": "v4_markets"}
        await self._send(msg)

    async def _subscribe_all(self) -> None:
        """
        Resubscribe to all previous subscriptions.
        """
        if self._client is None:
            self._log.error("Cannot subscribe all: not connected")
            return

        for subscription in self._subscriptions:
            msg = {"type": "subscribe", "channel": subscription[0], "id": subscription[1]}
            await self._send(msg)

    async def _send(self, msg: dict[str, Any]) -> None:
        if self._client is None:
            self._log.error(f"Cannot send message {msg}: not connected")
            return

        self._log.debug(f"SENDING: {msg}")

        try:
            await self._client.send_text(msgspec.json.encode(msg))
        except WebSocketClientError as e:
            self._log.error(f"Failed to send websocket message: {e}")
