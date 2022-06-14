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
from typing import Callable, List, Optional

from nautilus_trader.adapters.binance.common.functions import format_symbol
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.network.websocket import WebSocketClient


class BinanceWebSocketClient(WebSocketClient):
    """
    Provides a `Binance` streaming WebSocket client.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        handler: Callable[[bytes], None],
        base_url: Optional[str] = None,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
            handler=handler,
            max_retry_connection=6,
        )

        self._base_url = base_url

        self._clock = clock
        self._streams: List[str] = []

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

    async def connect(
        self,
        key: Optional[str] = None,
        start: bool = True,
        **ws_kwargs,
    ) -> None:
        if not self._streams:
            raise RuntimeError("no subscriptions for connection.")

        # Always connecting combined streams for consistency
        ws_url = self._base_url + "/stream?streams=" + "/".join(self._streams)
        if key is not None:
            ws_url += f"&listenKey={key}"

        self._log.info(f"Connecting to {ws_url}")
        await super().connect(ws_url=ws_url, start=start, **ws_kwargs)

    def _add_stream(self, stream: str):
        if stream not in self._streams:
            self._streams.append(stream)

    def subscribe(self, key: str):
        """
        Subscribe to the user data stream.

        Parameters
        ----------
        key : str
            The listen key for the subscription.

        """
        self._add_stream(key)

    def subscribe_agg_trades(self, symbol: str):
        """
        Aggregate Trade Streams.

        The Aggregate Trade Streams push trade information that is aggregated for a single taker order.
        Stream Name: <symbol>@aggTrade
        Update Speed: Real-time

        """
        self._add_stream(f"{format_symbol(symbol).lower()}@aggTrade")

    def subscribe_trades(self, symbol: str):
        """
        Trade Streams.

        The Trade Streams push raw trade information; each trade has a unique buyer and seller.
        Stream Name: <symbol>@trade
        Update Speed: Real-time

        """
        self._add_stream(f"{format_symbol(symbol).lower()}@trade")

    def subscribe_bars(self, symbol: str, interval: str):
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
        self._add_stream(f"{format_symbol(symbol).lower()}@kline_{interval}")

    def subscribe_mini_ticker(self, symbol: str = None):
        """
        Individual symbol or all symbols mini ticker.

        24hr rolling window mini-ticker statistics.
        These are NOT the statistics of the UTC day, but a 24hr rolling window for the previous 24hrs
        Stream Name: <symbol>@miniTicker or
        Stream Name: !miniTicker@arr
        Update Speed: 1000ms

        """
        if symbol is None:
            self._add_stream("!miniTicker@arr")
        else:
            self._add_stream(f"{format_symbol(symbol).lower()}@miniTicker")

    def subscribe_ticker(self, symbol: str = None):
        """
        Individual symbol or all symbols ticker.

        24hr rolling window ticker statistics for a single symbol.
        These are NOT the statistics of the UTC day, but a 24hr rolling window for the previous 24hrs.
        Stream Name: <symbol>@ticker or
        Stream Name: !ticker@arr
        Update Speed: 1000ms

        """
        if symbol is None:
            self._add_stream("!ticker@arr")
        else:
            self._add_stream(f"{format_symbol(symbol).lower()}@ticker")

    def subscribe_book_ticker(self, symbol: str = None):
        """
        Individual symbol or all book ticker.

        Pushes any update to the best bid or ask's price or quantity in real-time for a specified symbol.
        Stream Name: <symbol>@bookTicker or
        Stream Name: !bookTicker
        Update Speed: realtime

        """
        if symbol is None:
            self._add_stream("!bookTicker")
        else:
            self._add_stream(f"{format_symbol(symbol).lower()}@bookTicker")

    def subscribe_partial_book_depth(self, symbol: str, depth: int, speed: int):
        """
        Partial Book Depth Streams.

        Top bids and asks, Valid are 5, 10, or 20.
        Stream Names: <symbol>@depth<levels> OR <symbol>@depth<levels>@100ms.
        Update Speed: 1000ms or 100ms

        """
        self._add_stream(f"{format_symbol(symbol).lower()}@depth{depth}@{speed}ms")

    def subscribe_diff_book_depth(self, symbol: str, speed: int):
        """
        Diff book depth stream.

        Stream Name: <symbol>@depth OR <symbol>@depth@100ms
        Update Speed: 1000ms or 100ms
        Order book price and quantity depth updates used to locally manage an order book.

        """
        self._add_stream(f"{format_symbol(symbol).lower()}@depth@{speed}ms")

    def subscribe_mark_price(self, symbol: str = None, speed: int = None):
        """
        Aggregate Trade Streams.

        The Aggregate Trade Streams push trade information that is aggregated for a single taker order.
        Stream Name: <symbol>@aggTrade
        Update Speed: 3000ms or 1000ms

        """
        assert speed in (1000, 3000), "`speed` options are 1000ms or 3000ms only"
        if symbol is None:
            self._add_stream("!markPrice@arr")
        else:
            self._add_stream(f"{format_symbol(symbol).lower()}@markPrice@{int(speed / 1000)}s")
