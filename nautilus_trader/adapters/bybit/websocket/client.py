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

import hashlib
import hmac
import json
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

    """

    def __init__(
        self,
        clock: LiveClock,
        base_url: str,
        handler: Callable[[bytes], None],
        api_key: str | None = None,
        api_secret: str | None = None,
        is_private: bool | None = False,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(name=type(self).__name__)
        self._url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._client: WebSocketClient | None = None
        self._is_private = is_private
        self._api_key = api_key
        self._api_secret = api_secret

        self._streams_connecting: set[str] = set()
        self._subscriptions: list[str] = []

    @property
    def subscriptions(self) -> list[str]:
        return self._subscriptions

    def has_subscriptions(self, item: str) -> bool:
        return item in self._subscriptions

    ################################################################################
    # Public
    ################################################################################

    async def subscribe_trades(self, symbol: str) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected.")
            return

        subscription = f"publicTrade.{symbol}"
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))
        self._subscriptions.append(subscription)

    async def subscribe_tickers(self, symbol: str) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected.")
            return

        subscription = f"tickers.{symbol}"
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))
        self._subscriptions.append(subscription)

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
            self._log.warning("Cannot subscribe: not connected.")
            return

        subscription = "order"
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))
        self._subscriptions.append(subscription)

    async def subscribe_executions_update(self) -> None:
        if self._client is None:
            self._log.warning("Cannot subscribe: not connected.")
            return

        subscription = "execution"
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))
        self._subscriptions.append(subscription)

    async def connect(self) -> None:
        self._log.debug(f"Connecting to {self._url} websocket stream")
        config = WebSocketConfig(
            url=self._url,
            handler=self._handler,
            heartbeat=20,
            heartbeat_msg=json.dumps({"op": "ping"}),
            headers=[],
        )
        client = await WebSocketClient.connect(
            config=config,
        )
        self._client = client
        self._log.info(f"Connected to {self._url}.", LogColor.BLUE)
        ## authenticate
        if self._is_private:
            signature = self._get_signature()
            self._client.send_text(json.dumps(signature))

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

    async def disconnect(self) -> None:
        if self._client is None:
            self._log.warning("Cannot disconnect: not connected.")
            return

        await self._client.send_text(json.dumps({"op": "unsubscribe", "args": self._subscriptions}))
        await self._client.disconnect()
        self._log.info(f"Disconnected from {self._url}.", LogColor.BLUE)
