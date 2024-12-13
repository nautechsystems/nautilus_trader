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
from collections.abc import Awaitable
from collections.abc import Callable
from typing import Any

import msgspec

from nautilus_trader.adapters.bybit.schemas.ws import BybitWsSubscriptionMsg
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig
from nautilus_trader.core.nautilus_pyo3 import hmac_signature


MAX_ARGS_PER_SUBSCRIPTION_REQUEST = 10


class BybitWebSocketClient:
    """
    Provides a Bybit streaming WebSocket client.

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
    max_reconnection_tries: int, default 3
        The number of retries to reconnect the websocket connection if the
        connection is broken.

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
        max_reconnection_tries: int | None = 3,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(name=type(self).__name__)

        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop
        self._max_reconnection_tries = max_reconnection_tries

        self._client: WebSocketClient | None = None
        self._api_key = api_key
        self._api_secret = api_secret
        self._is_private = is_private
        self._is_running = False

        self._subscriptions: list[str] = []

        self._is_authenticated = False
        self._decoder_ws_subscription = msgspec.json.Decoder(BybitWsSubscriptionMsg)

    @property
    def subscriptions(self) -> list[str]:
        return self._subscriptions

    def has_subscription(self, item: str) -> bool:
        return item in self._subscriptions

    async def connect(self) -> None:
        self._is_running = True
        self._log.debug(f"Connecting to {self._base_url} websocket stream")
        config = WebSocketConfig(
            url=self._base_url,
            handler=self._msg_handler,
            heartbeat=20,
            heartbeat_msg=msgspec.json.encode({"op": "ping"}).decode(),
            headers=[],
            max_reconnection_tries=self._max_reconnection_tries,
        )
        client = await WebSocketClient.connect(
            config=config,
            post_reconnection=self.reconnect,
        )
        self._client = client
        self._log.info(f"Connected to {self._base_url}", LogColor.BLUE)

        # Authenticate
        if self._is_private:
            await self._authenticate()

    def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        if not self._is_running:
            return

        self._log.warning(f"Trying to reconnect to {self._base_url}")
        self._loop.create_task(self._reconnect_wrapper())

    async def _reconnect_wrapper(self) -> None:
        # Authenticate
        if self._is_private:
            await self._authenticate()

        # Re-subscribe to all streams
        await self._subscribe_all()

        if self._handler_reconnect:
            await self._handler_reconnect()

        self._log.warning(f"Reconnected to {self._base_url}")

    async def disconnect(self) -> None:
        self._is_running = False

        if self._client is None:
            self._log.warning("Cannot disconnect: not connected.")
            return

        try:
            await self._client.disconnect()
        except WebSocketClientError as e:
            self._log.error(str(e))

        self._client = None  # Dispose (will go out of scope)

        self._log.info(f"Disconnected from {self._base_url}", LogColor.BLUE)

    def _msg_handler(self, raw: bytes) -> None:
        """
        Handle pushed websocket messages.

        Parameters
        ----------
        raw : bytes
            The received message in bytes.

        """
        if self._is_private and not self._is_authenticated:
            msg = self._decoder_ws_subscription.decode(raw)
            if msg.op == "auth":
                if msg.success is True:
                    self._is_authenticated = True
                    self._log.info("Private channel authenticated")
                else:
                    raise RuntimeError(f"Private channel authentication failed: {msg}")

        self._handler(raw)

    async def _authenticate(self) -> None:
        self._is_authenticated = False
        signature = self._get_signature()
        await self._send(signature)

        while not self._is_authenticated:
            self._log.debug("Waiting for private channel authentication")
            await asyncio.sleep(0.1)

    async def _subscribe(self, subscription: str) -> None:
        if subscription in self._subscriptions:
            self._log.warning(f"Cannot subscribe '{subscription}': already subscribed")
            return

        self._subscriptions.append(subscription)
        msg = {"op": "subscribe", "args": [subscription]}
        await self._send(msg)

    async def _unsubscribe(self, subscription: str) -> None:
        if subscription not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe '{subscription}': not subscribed")
            return

        self._subscriptions.remove(subscription)
        msg = {"op": "unsubscribe", "args": [subscription]}
        await self._send(msg)

    ################################################################################
    # Public
    ################################################################################

    async def subscribe_order_book(self, symbol: str, depth: int) -> None:
        subscription = f"orderbook.{depth}.{symbol}"
        await self._subscribe(subscription)

    async def subscribe_trades(self, symbol: str) -> None:
        subscription = f"publicTrade.{symbol}"
        await self._subscribe(subscription)

    async def subscribe_tickers(self, symbol: str) -> None:
        subscription = f"tickers.{symbol}"
        await self._subscribe(subscription)

    async def subscribe_klines(self, symbol: str, interval: str) -> None:
        subscription = f"kline.{interval}.{symbol}"
        await self._subscribe(subscription)

    async def unsubscribe_order_book(self, symbol: str, depth: int) -> None:
        subscription = f"orderbook.{depth}.{symbol}"
        await self._unsubscribe(subscription)

    async def unsubscribe_trades(self, symbol: str) -> None:
        subscription = f"publicTrade.{symbol}"
        await self._unsubscribe(subscription)

    async def unsubscribe_tickers(self, symbol: str) -> None:
        subscription = f"tickers.{symbol}"
        await self._unsubscribe(subscription)

    async def unsubscribe_klines(self, symbol: str, interval: str) -> None:
        subscription = f"kline.{interval}.{symbol}"
        await self._unsubscribe(subscription)

    ################################################################################
    # Private
    ################################################################################

    async def subscribe_account_position_update(self) -> None:
        subscription = "position"
        await self._subscribe(subscription)

    async def subscribe_orders_update(self) -> None:
        subscription = "order"
        await self._subscribe(subscription)

    async def subscribe_executions_update(self) -> None:
        subscription = "execution"
        await self._subscribe(subscription)

    async def subscribe_executions_update_fast(self) -> None:
        subscription = "execution.fast"
        await self._subscribe(subscription)

    async def subscribe_wallet_update(self) -> None:
        subscription = "wallet"
        await self._subscribe(subscription)

    def _get_signature(self):
        expires = self._clock.timestamp_ms() + 5_000
        sign = f"GET/realtime{expires}"
        signature = hmac_signature(self._api_secret, sign)
        return {
            "op": "auth",
            "args": [self._api_key, expires, signature],
        }

    async def _subscribe_all(self) -> None:
        if self._client is None:
            self._log.error("Cannot subscribe all: not connected")
            return

        self._log.info("Resubscribe to all data streams")

        # You can input up to 10 args for each subscription request sent to one connection
        subscription_lists = [
            self._subscriptions[i : i + MAX_ARGS_PER_SUBSCRIPTION_REQUEST]
            for i in range(0, len(self._subscriptions), MAX_ARGS_PER_SUBSCRIPTION_REQUEST)
        ]

        for subscriptions in subscription_lists:
            msg = {"op": "subscribe", "args": subscriptions}
            await self._send(msg)

    async def _send(self, msg: dict[str, Any]) -> None:
        if self._client is None:
            self._log.error(f"Cannot send message {msg}: not connected")
            return

        self._log.debug(f"SENDING: {msg}")

        try:
            await self._client.send_text(msgspec.json.encode(msg))
        except WebSocketClientError as e:
            self._log.error(str(e))
