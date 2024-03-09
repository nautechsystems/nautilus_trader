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
import json
from collections.abc import Awaitable
from collections.abc import Callable
from typing import Any

from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig


class BinanceWebSocketClient:
    """
    Provides a `Binance` streaming WebSocket client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    base_url : str
        The base URL for the WebSocket connection.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    handler_reconnect : Callable[..., Awaitable[None]], optional
        The callback handler to be called on reconnect.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#websocket-market-streams

    """

    def __init__(
        self,
        clock: LiveClock,
        base_url: str,
        handler: Callable[[bytes], None],
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        loop: asyncio.AbstractEventLoop,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(type(self).__name__)

        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop

        self._streams: list[str] = []
        self._inner: WebSocketClient | None = None
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
        return self._base_url

    @property
    def subscriptions(self) -> list[str]:
        """
        Return the current active subscriptions for the client.

        Returns
        -------
        str

        """
        return self._streams.copy()

    @property
    def has_subscriptions(self) -> bool:
        """
        Return whether the client has subscriptions.

        Returns
        -------
        bool

        """
        return bool(self._streams)

    async def connect(self) -> None:
        """
        Connect a websocket client to the server.
        """
        if not self._streams:
            self._log.error("Cannot connect: no streams for initial connection.")
            return

        # Binance expects at least one stream for the initial connection
        initial_stream = self._streams[0]
        ws_url = self._base_url + f"/stream?streams={initial_stream}"

        self._log.debug(f"Connecting to {ws_url}...")
        self._is_connecting = True

        config = WebSocketConfig(
            url=ws_url,
            handler=self._handler,
            heartbeat=60,
            headers=[],
            ping_handler=self._handle_ping,
        )

        self._inner = await WebSocketClient.connect(
            config=config,
            post_reconnection=self.reconnect,
        )
        self._is_connecting = False
        self._log.info(f"Connected to {self._base_url}.", LogColor.BLUE)
        self._log.info(f"Subscribed to {initial_stream}.", LogColor.BLUE)

    def _handle_ping(self, raw: bytes) -> None:
        self._loop.create_task(self.send_pong(raw))

    async def send_pong(self, raw: bytes) -> None:
        """
        Send the given raw payload to the server as a PONG message.
        """
        if self._inner is None:
            return

        await self._inner.send_pong(raw)

    # TODO: Temporarily synch
    def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        if not self._streams:
            self._log.error("Cannot reconnect: no streams for initial connection.")
            return

        self._log.warning(f"Reconnected to {self._base_url}.")

        # Re-subscribe to all streams
        self._loop.create_task(self._subscribe_all())

        if self._handler_reconnect is not None:
            self._loop.create_task(self._handler_reconnect())  # type: ignore

    async def disconnect(self) -> None:
        """
        Disconnect the client from the server.
        """
        if self._inner is None:
            self._log.warning("Cannot disconnect: not connected.")
            return

        self._log.debug("Disconnecting...")
        await self._inner.disconnect()
        self._inner = None

        self._log.info("Disconnected.")

    async def subscribe_listen_key(self, listen_key: str) -> None:
        """
        Subscribe to user data stream.
        """
        await self._subscribe(listen_key)

    async def unsubscribe_listen_key(self, listen_key: str) -> None:
        """
        Unsubscribe from user data stream.
        """
        await self._unsubscribe(listen_key)

    async def subscribe_agg_trades(self, symbol: str) -> None:
        """
        Subscribe to aggregate trade stream.

        The Aggregate Trade Streams push trade information that is aggregated for a single taker order.
        Stream Name: <symbol>@aggTrade
        Update Speed: Real-time

        """
        stream = f"{BinanceSymbol(symbol).lower()}@aggTrade"
        await self._subscribe(stream)

    async def unsubscribe_agg_trades(self, symbol: str) -> None:
        """
        Unsubscribe from aggregate trade stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@aggTrade"
        await self._unsubscribe(stream)

    async def subscribe_trades(self, symbol: str) -> None:
        """
        Subscribe to trade stream.

        The Trade Streams push raw trade information; each trade has a unique buyer and seller.
        Stream Name: <symbol>@trade
        Update Speed: Real-time

        """
        stream = f"{BinanceSymbol(symbol).lower()}@trade"
        await self._subscribe(stream)

    async def unsubscribe_trades(self, symbol: str) -> None:
        """
        Unsubscribe from trade stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@trade"
        await self._unsubscribe(stream)

    async def subscribe_bars(
        self,
        symbol: str,
        interval: str,
    ) -> None:
        """
        Subscribe to bar (kline/candlestick) stream.

        The Kline/Candlestick Stream push updates to the current klines/candlestick every second.
        Stream Name: <symbol>@kline_<interval>
        interval:
        m -> minutes; h -> hours; d -> days; w -> weeks; M -> months
        - 1m
        - 3m
        - 5m
        - 15m
        - 30m
        - 1h
        - 2h
        - 4h
        - 6h
        - 8h
        - 12h
        - 1d
        - 3d
        - 1w
        - 1M
        Update Speed: 2000ms

        """
        stream = f"{BinanceSymbol(symbol).lower()}@kline_{interval}"
        await self._subscribe(stream)

    async def unsubscribe_bars(
        self,
        symbol: str,
        interval: str,
    ) -> None:
        """
        Unsubscribe from bar (kline/candlestick) stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@kline_{interval}"
        await self._unsubscribe(stream)

    async def subscribe_mini_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Subscribe to individual symbol or all symbols mini ticker stream.

        24hr rolling window mini-ticker statistics.
        These are NOT the statistics of the UTC day, but a 24hr rolling window for the previous 24hrs
        Stream Name: <symbol>@miniTicker or
        Stream Name: !miniTicker@arr
        Update Speed: 1000ms

        """
        if symbol is None:
            stream = "!miniTicker@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@miniTicker"
        await self._subscribe(stream)

    async def unsubscribe_mini_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Unsubscribe to individual symbol or all symbols mini ticker stream.
        """
        if symbol is None:
            stream = "!miniTicker@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@miniTicker"
        await self._unsubscribe(stream)

    async def subscribe_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Subscribe to individual symbol or all symbols ticker stream.

        24hr rolling window ticker statistics for a single symbol.
        These are NOT the statistics of the UTC day, but a 24hr rolling window for the previous 24hrs.
        Stream Name: <symbol>@ticker or
        Stream Name: !ticker@arr
        Update Speed: 1000ms

        """
        if symbol is None:
            stream = "!ticker@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@ticker"
        await self._subscribe(stream)

    async def unsubscribe_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Unsubscribe from individual symbol or all symbols ticker stream.
        """
        if symbol is None:
            stream = "!ticker@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@ticker"
        await self._unsubscribe(stream)

    async def subscribe_book_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Subscribe to individual symbol or all book tickers stream.

        Pushes any update to the best bid or ask's price or quantity in real-time for a specified symbol.
        Stream Name: <symbol>@bookTicker or
        Stream Name: !bookTicker
        Update Speed: realtime

        """
        if symbol is None:
            stream = "!bookTicker"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@bookTicker"
        await self._subscribe(stream)

    async def unsubscribe_book_ticker(
        self,
        symbol: str | None = None,
    ) -> None:
        """
        Unsubscribe from individual symbol or all book tickers.
        """
        if symbol is None:
            stream = "!bookTicker"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@bookTicker"
        await self._unsubscribe(stream)

    async def subscribe_partial_book_depth(
        self,
        symbol: str,
        depth: int,
        speed: int,
    ) -> None:
        """
        Subscribe to partial book depth stream.

        Top bids and asks, Valid are 5, 10, or 20.
        Stream Names: <symbol>@depth<levels> OR <symbol>@depth<levels>@100ms.
        Update Speed: 1000ms or 100ms

        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth{depth}@{speed}ms"
        await self._subscribe(stream)

    async def unsubscribe_partial_book_depth(
        self,
        symbol: str,
        depth: int,
        speed: int,
    ) -> None:
        """
        Unsubscribe from partial book depth stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth{depth}@{speed}ms"
        await self._subscribe(stream)

    async def subscribe_diff_book_depth(
        self,
        symbol: str,
        speed: int,
    ) -> None:
        """
        Subscribe to diff book depth stream.

        Stream Name: <symbol>@depth OR <symbol>@depth@100ms
        Update Speed: 1000ms or 100ms
        Order book price and quantity depth updates used to locally manage an order book.

        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth@{speed}ms"
        await self._subscribe(stream)

    async def unsubscribe_diff_book_depth(
        self,
        symbol: str,
        speed: int,
    ) -> None:
        """
        Unsubscribe from diff book depth stream.
        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth@{speed}ms"
        await self._unsubscribe(stream)

    async def subscribe_mark_price(
        self,
        symbol: str | None = None,
        speed: int | None = None,
    ) -> None:
        """
        Subscribe to aggregate mark price stream.
        """
        if speed not in (1000, 3000):
            raise ValueError(f"`speed` options are 1000ms or 3000ms only, was {speed}")
        if symbol is None:
            stream = "!markPrice@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@markPrice@{int(speed / 1000)}s"
        await self._subscribe(stream)

    async def unsubscribe_mark_price(
        self,
        symbol: str | None = None,
        speed: int | None = None,
    ) -> None:
        """
        Unsubscribe from aggregate mark price stream.
        """
        if speed not in (1000, 3000):
            raise ValueError(f"`speed` options are 1000ms or 3000ms only, was {speed}")
        if symbol is None:
            stream = "!markPrice@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@markPrice@{int(speed / 1000)}s"
        await self._unsubscribe(stream)

    async def _subscribe(self, stream: str) -> None:
        if stream in self._streams:
            self._log.warning(f"Cannot subscribe to {stream}: already subscribed.")
            return  # Already subscribed

        self._streams.append(stream)

        while self._is_connecting and not self._inner:
            await asyncio.sleep(0.01)

        if self._inner is None:
            # Make initial connection
            await self.connect()
            return

        message = self._create_subscribe_msg(streams=[stream])
        self._log.debug(f"SENDING: {message}")

        await self._inner.send_text(json.dumps(message))
        self._log.info(f"Subscribed to {stream}.", LogColor.BLUE)

    async def _subscribe_all(self) -> None:
        if self._inner is None:
            self._log.error("Cannot subscribe all: no connected.")
            return

        message = self._create_subscribe_msg(streams=self._streams)
        self._log.debug(f"SENDING: {message}")

        await self._inner.send_text(json.dumps(message))
        for stream in self._streams:
            self._log.info(f"Subscribed to {stream}.", LogColor.BLUE)

    async def _unsubscribe(self, stream: str) -> None:
        if stream not in self._streams:
            self._log.warning(f"Cannot unsubscribe from {stream}: never subscribed.")
            return  # Not subscribed

        self._streams.remove(stream)

        if self._inner is None:
            self._log.error(f"Cannot unsubscribe from {stream}: not connected.")
            return

        message = self._create_unsubscribe_msg(streams=[stream])
        self._log.debug(f"SENDING: {message}")

        await self._inner.send_text(json.dumps(message))
        self._log.info(f"Unsubscribed from {stream}.", LogColor.BLUE)

    def _create_subscribe_msg(self, streams: list[str]) -> dict[str, Any]:
        message = {
            "method": "SUBSCRIBE",
            "params": streams,
            "id": self._msg_id,
        }
        self._msg_id += 1
        return message

    def _create_unsubscribe_msg(self, streams: list[str]) -> dict[str, Any]:
        message = {
            "method": "UNSUBSCRIBE",
            "params": streams,
            "id": self._msg_id,
        }
        self._msg_id += 1
        return message
