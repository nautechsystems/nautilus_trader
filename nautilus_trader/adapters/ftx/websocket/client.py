# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import hmac
from typing import Callable, Dict, List, Optional

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.network.websocket import WebSocketClient


class FTXWebSocketClient(WebSocketClient):
    """
    Provides a `FTX` streaming WebSocket client.
    """

    BASE_URL = "wss://ftx.com/ws/"

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        msg_handler: Callable[[bytes], None],
        reconnect_handler: Callable[[], None],
        key: Optional[str] = None,
        secret: Optional[str] = None,
        base_url: Optional[str] = None,
        us: bool = False,
        auto_ping_interval: Optional[float] = None,
        log_send: bool = False,
        log_recv: bool = False,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
            handler=msg_handler,
            max_retry_connection=6,
            pong_msg=b'{"type":"pong"}',
            log_send=log_send,
            log_recv=log_recv,
        )

        self._clock = clock
        self._base_url = base_url or self.BASE_URL
        if self._base_url == self.BASE_URL and us:
            self._base_url = self._base_url.replace("com", "us")
        self._key = key
        self._secret = secret

        self._reconnect_handler = reconnect_handler
        self._streams: List[Dict] = []

        # Tasks
        self._auto_ping_interval = auto_ping_interval
        self._task_auto_ping: Optional[asyncio.Task] = None

    @property
    def base_url(self) -> str:
        return self._base_url

    @property
    def subscriptions(self):
        return self._streams.copy()

    @property
    def has_subscriptions(self):
        if self._streams:
            return True
        else:
            return False

    async def connect(self, start: bool = True, **ws_kwargs) -> None:
        await super().connect(ws_url=self._base_url, start=start, **ws_kwargs)

    async def post_connection(self):
        if self._key is None or self._secret is None:
            self._log.info("Unauthenticated session (no credentials provided).")
            return

        time: int = self._clock.timestamp_ms()
        sign: str = hmac.new(
            self._secret.encode(),
            f"{time}websocket_login".encode(),
            "sha256",
        ).hexdigest()

        login = {
            "op": "login",
            "args": {
                "key": self._key,
                "sign": sign,
                "time": time,
            },
        }

        await self.send_json(login)

        if self._auto_ping_interval and self._task_auto_ping is None:
            self._task_auto_ping = self._loop.create_task(self._auto_ping())

        self._log.info("Session authenticated.")

    async def post_reconnection(self):
        # Re-login and authenticate
        await self.post_connection()

        # Resubscribe to all streams
        for subscription in self._streams:
            await self.send_json({"op": "subscribe", **subscription})

        self._reconnect_handler()

    async def post_disconnection(self) -> None:
        if self._task_auto_ping is not None:
            self._task_auto_ping.cancel()
            self._task_auto_ping = None  # Clear canceled task

    async def _auto_ping(self) -> None:
        while True:
            await asyncio.sleep(self._auto_ping_interval)
            await self._ping()

    async def _ping(self) -> None:
        await self.send_json({"op": "ping"})

    async def _subscribe(self, subscription: Dict) -> None:
        if subscription not in self._streams:
            await self.send_json({"op": "subscribe", **subscription})
            self._streams.append(subscription)

    async def _unsubscribe(self, subscription: Dict) -> None:
        if subscription in self._streams:
            await self.send_json({"op": "unsubscribe", **subscription})
            self._streams.remove(subscription)

    async def subscribe_markets(self) -> None:
        subscription = {"channel": "markets"}
        await self._subscribe(subscription)

    async def subscribe_ticker(self, market: str) -> None:
        subscription = {"channel": "ticker", "market": market}
        await self._subscribe(subscription)

    async def subscribe_trades(self, market: str) -> None:
        subscription = {"channel": "trades", "market": market}
        await self._subscribe(subscription)

    async def subscribe_fills(self) -> None:
        subscription = {"channel": "fills"}
        await self._subscribe(subscription)

    async def subscribe_orders(self) -> None:
        subscription = {"channel": "orders"}
        await self._subscribe(subscription)

    async def subscribe_orderbook(self, market: str) -> None:
        subscription = {"channel": "orderbook", "market": market}
        await self._subscribe(subscription)

    async def unsubscribe_markets(self) -> None:
        subscription = {"channel": "markets"}
        await self._unsubscribe(subscription)

    async def unsubscribe_ticker(self, market: str) -> None:
        subscription = {"channel": "ticker", "market": market}
        await self._unsubscribe(subscription)

    async def unsubscribe_trades(self, market: str) -> None:
        subscription = {"channel": "trades", "market": market}
        await self._unsubscribe(subscription)

    async def unsubscribe_fills(self) -> None:
        subscription = {"channel": "fills"}
        await self._unsubscribe(subscription)

    async def unsubscribe_orders(self) -> None:
        subscription = {"channel": "orders"}
        await self._unsubscribe(subscription)

    async def unsubscribe_orderbook(self, market: str) -> None:
        subscription = {"channel": "orderbook", "market": market}
        await self._unsubscribe(subscription)
