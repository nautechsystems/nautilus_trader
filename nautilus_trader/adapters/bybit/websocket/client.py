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

import asyncio
import hashlib
import hmac
import json
from collections.abc import Awaitable
from collections.abc import Callable

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig


class BybitWebsocketClient:
    """
    Provides a `Bybit` streaming WebSocket client.

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
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        api_key: str,
        api_secret: str,
        loop: asyncio.AbstractEventLoop,
        is_private: bool | None = False,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(name=type(self).__name__)

        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop

        self._client: WebSocketClient | None = None
        self._is_private = is_private
        self._api_key = api_key
        self._api_secret = api_secret

        self._subscriptions: list[str] = []

    @property
    def subscriptions(self) -> list[str]:
        return self._subscriptions

    def has_subscription(self, item: str) -> bool:
        return item in self._subscriptions

    async def connect(self) -> None:
        self._log.debug(f"Connecting to {self._base_url} websocket stream")
        config = WebSocketConfig(
            url=self._base_url,
            handler=self._handler,
            heartbeat=20,
            heartbeat_msg=json.dumps({"op": "ping"}),
            headers=[],
        )
        client = await WebSocketClient.connect(
            config=config,
            post_reconnection=self.reconnect,
        )
        self._client = client
        self._log.info(f"Connected to {self._base_url}", LogColor.BLUE)

        ## Authenticate
        if self._is_private:
            signature = self._get_signature()
            self._client.send_text(json.dumps(signature))

    # TODO: Temporarily sync
    def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        self._log.warning(f"Reconnected to {self._base_url}")

        # Re-subscribe to all streams
        self._loop.create_task(self._subscribe_all())

        if self._handler_reconnect:
            self._loop.create_task(self._handler_reconnect())  # type: ignore

    async def disconnect(self) -> None:
        if self._client is None:
            self._log.warning("Cannot disconnect: not connected.")
            return

        await self._client.disconnect()
        self._client = None  # Dispose (will go out of scope)

        self._log.info(f"Disconnected from {self._base_url}", LogColor.BLUE)

    ################################################################################
    # Public
    ################################################################################

    async def subscribe_order_book(self, symbol: str, depth: int) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = f"orderbook.{depth}.{symbol}"
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.append(subscription)
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    async def subscribe_trades(self, symbol: str) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = f"publicTrade.{symbol}"
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.append(subscription)
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    async def subscribe_tickers(self, symbol: str) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = f"tickers.{symbol}"
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.append(subscription)
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    async def subscribe_klines(self, symbol: str, interval: str) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = f"kline.{interval}.{symbol}"
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.append(subscription)
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    async def unsubscribe_order_book(self, symbol: str, depth: int) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = f"orderbook.{depth}.{symbol}"
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        sub = {"op": "unsubscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    async def unsubscribe_trades(self, symbol: str) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = f"publicTrade.{symbol}"
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        sub = {"op": "unsubscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    async def unsubscribe_tickers(self, symbol: str) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = f"tickers.{symbol}"
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        sub = {"op": "unsubscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    async def unsubscribe_klines(self, symbol: str, interval: str) -> None:
        if self._client is None:
            self._log.warning("Cannot unsubscribe: not connected")
            return

        subscription = f"kline.{interval}.{symbol}"
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        sub = {"op": "unsubscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    ################################################################################
    # Private
    ################################################################################
    # async def subscribe_account_position_update(self) -> None:
    #     subsscription = "position"
    #     sub = {"op": "subscribe", "args": [subsscription]}
    #     await self._client.send_text(json.dumps(sub))
    #     self._subscriptions.append(subsscription)

    async def subscribe_orders_update(self) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = "order"
        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    async def subscribe_executions_update(self) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected")
            return

        subscription = "execution"
        if subscription in self._subscriptions:
            return

        self._subscriptions.append(subscription)
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))

    def _get_signature(self):
        timestamp = self._clock.timestamp_ms() + 1000
        sign = f"GET/realtime{timestamp}"
        signature = hmac.new(
            self._api_secret.encode("utf-8"),
            sign.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
        return {
            "op": "auth",
            "args": [self._api_key, timestamp, signature],
        }

    async def _subscribe_all(self) -> None:
        if self._client is None:
            self._log.error("Cannot subscribe all: not connected")
            return

        sub = {"op": "subscribe", "args": self._subscriptions}
        await self._client.send_text(json.dumps(sub))
