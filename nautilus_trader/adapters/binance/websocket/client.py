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

from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.nautilus_pyo3.network import WebSocketClient


class BinanceWebSocketClient:
    """
    Provides a `Binance` streaming WebSocket client.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    base_url : str
        The base URL for the WebSocket connection.
    handler : Callable[[bytes], None]
        The callback handler for message events.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#websocket-market-streams

    """

    def __init__(
        self,
        clock: LiveClock,
        logger: Logger,
        base_url: str,
        handler: Callable[[bytes], None],
    ) -> None:
        self._clock = clock
        self._logger = logger
        self._log: LoggerAdapter = LoggerAdapter(type(self).__name__, logger=logger)

        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler

        self._streams_connecting: set[str] = set()
        self._streams: dict[str, WebSocketClient] = {}

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
        return list(self._streams.keys())

    @property
    def has_subscriptions(self) -> bool:
        """
        Return whether the client has subscriptions.

        Returns
        -------
        bool

        """
        return bool(self._streams)

    async def _connect(self, stream: str) -> None:
        if stream not in self._streams and stream not in self._streams_connecting:
            self._streams_connecting.add(stream)
            await self.connect(stream)

    async def connect(self, stream: str) -> None:
        """
        Connect a websocket client to the server for the given `stream`.
        """
        ws_url = self._base_url + f"/streams?streams={stream}"

        self._log.debug(f"Connecting to {ws_url}...")
        client = await WebSocketClient.connect(
            url=ws_url,
            handler=self._handler,
            heartbeat=60,
        )
        self._log.info(f"Connected to {ws_url}.", LogColor.BLUE)

        self._streams[stream] = client
        self._streams_connecting.discard(stream)

    async def disconnect(self) -> None:
        """
        Disconnect the client from the server.
        """
        client_disconnects = []
        for stream, client in self._streams.items():
            self._log.info(f"Disconnecting {stream}...")
            client_disconnects.append(client.disconnect())

        await asyncio.gather(*client_disconnects)

    async def subscribe_agg_trades(self, symbol: str) -> None:
        """
        Aggregate Trade Streams.

        The Aggregate Trade Streams push trade information that is aggregated for a single taker order.
        Stream Name: <symbol>@aggTrade
        Update Speed: Real-time

        """
        stream = f"{BinanceSymbol(symbol).lower()}@aggTrade"
        await self._connect(stream)

    async def subscribe_trades(self, symbol: str) -> None:
        """
        Trade Streams.

        The Trade Streams push raw trade information; each trade has a unique buyer and seller.
        Stream Name: <symbol>@trade
        Update Speed: Real-time

        """
        stream = f"{BinanceSymbol(symbol).lower()}@trade"
        await self._connect(stream)

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
        await self._connect(stream)

    async def subscribe_mini_ticker(
        self,
        symbol: Optional[str] = None,
    ) -> None:
        """
        Individual symbol or all symbols mini ticker.

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
        await self._connect(stream)

    async def subscribe_ticker(
        self,
        symbol: Optional[str] = None,
    ) -> None:
        """
        Individual symbol or all symbols ticker.

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
        await self._connect(stream)

    async def subscribe_book_ticker(
        self,
        symbol: Optional[str] = None,
    ) -> None:
        """
        Individual symbol or all book ticker.

        Pushes any update to the best bid or ask's price or quantity in real-time for a specified symbol.
        Stream Name: <symbol>@bookTicker or
        Stream Name: !bookTicker
        Update Speed: realtime

        """
        if symbol is None:
            stream = "!bookTicker"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@bookTicker"
        await self._connect(stream)

    async def subscribe_partial_book_depth(
        self,
        symbol: str,
        depth: int,
        speed: int,
    ) -> None:
        """
        Partial Book Depth Streams.

        Top bids and asks, Valid are 5, 10, or 20.
        Stream Names: <symbol>@depth<levels> OR <symbol>@depth<levels>@100ms.
        Update Speed: 1000ms or 100ms

        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth{depth}@{speed}ms"
        await self._connect(stream)

    async def subscribe_diff_book_depth(
        self,
        symbol: str,
        speed: int,
    ) -> None:
        """
        Diff book depth stream.

        Stream Name: <symbol>@depth OR <symbol>@depth@100ms
        Update Speed: 1000ms or 100ms
        Order book price and quantity depth updates used to locally manage an order book.

        """
        stream = f"{BinanceSymbol(symbol).lower()}@depth@{speed}ms"
        await self._connect(stream)

    async def subscribe_mark_price(
        self,
        symbol: Optional[str] = None,
        speed: Optional[int] = None,
    ) -> None:
        """
        Aggregate Trade Streams.

        The Aggregate Trade Streams push trade information that is aggregated for a single taker order.
        Stream Name: <symbol>@aggTrade
        Update Speed: 3000ms or 1000ms

        """
        if speed not in (1000, 3000):
            raise ValueError(f"`speed` options are 1000ms or 3000ms only, was {speed}")
        if symbol is None:
            stream = "!markPrice@arr"
        else:
            stream = f"{BinanceSymbol(symbol).lower()}@markPrice@{int(speed / 1000)}s"
        await self._connect(stream)
