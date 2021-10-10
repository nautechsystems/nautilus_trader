# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

import asyncio
from typing import Callable

from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


class BinanceSpotWebSocket(BinanceWebSocketClient):
    """
    Provides access to the `Binance SPOT` streaming WebSocket API.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        handler: Callable[[bytes], None],
        ws_url="wss://stream.binance.com:9443",
    ):
        super().__init__(
            loop=loop,
            clock=clock,
            logger=logger,
            handler=handler,
            ws_url=ws_url,
        )

    def agg_trade(self, symbol: str, id: int, **kwargs):
        """
        Aggregate Trade Streams.

        The Aggregate Trade Streams push trade information that is aggregated for a single taker order.
        Stream Name: <symbol>@aggTrade
        Update Speed: Real-time

        """
        self.live_subscribe(f"{symbol.lower()}@aggTrade", id, **kwargs)

    def trade(self, symbol: str, id: int, **kwargs):
        """
        Trade Streams.

        The Trade Streams push raw trade information; each trade has a unique buyer and seller.
        Stream Name: <symbol>@trade
        Update Speed: Real-time

        """
        self.live_subscribe(f"{symbol.lower()}@trade", id, **kwargs)

    def kline(self, symbol: str, id: int, interval: str, **kwargs):
        """
        Kline/Candlestick Streams.

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
        self.live_subscribe(f"{symbol.lower()}@kline_{interval}", id, **kwargs)

    def mini_ticker(self, id: int, symbol=None, **kwargs):
        """
        Individual symbol or all symbols mini ticker.

        24hr rolling window mini-ticker statistics.
        These are NOT the statistics of the UTC day, but a 24hr rolling window for the previous 24hrs
        Stream Name: <symbol>@miniTicker or
        Stream Name: !miniTicker@arr
        Update Speed: 1000ms

        """
        if symbol is None:
            self.live_subscribe("!miniTicker@arr", id, **kwargs)
        else:
            self.live_subscribe(f"{symbol.lower()}@miniTicker", id, **kwargs)

    def ticker(self, id: int, symbol=None, **kwargs):
        """
        Individual symbol or all symbols ticker.

        24hr rolling window ticker statistics for a single symbol.
        These are NOT the statistics of the UTC day, but a 24hr rolling window for the previous 24hrs.
        Stream Name: <symbol>@ticker or
        Stream Name: !ticker@arr
        Update Speed: 1000ms

        """
        if symbol is None:
            self.live_subscribe("!ticker@arr", id, **kwargs)
        else:
            self.live_subscribe(f"{symbol.lower()}@ticker", id, **kwargs)

    def book_ticker(self, id: int, symbol=None, **kwargs):
        """
        Individual symbol or all book ticker.

        Pushes any update to the best bid or ask's price or quantity in real-time for a specified symbol.
        Stream Name: <symbol>@bookTicker or
        Stream Name: !bookTicker
        Update Speed: realtime

        """
        if symbol is None:
            self.live_subscribe("!bookTicker", id, **kwargs)
        else:
            self.live_subscribe(f"{symbol.lower()}@bookTicker", id, **kwargs)

    def partial_book_depth(self, symbol: str, id: int, level, speed, **kwargs):
        """
        Partial Book Depth Streams.

        Top bids and asks, Valid are 5, 10, or 20.
        Stream Names: <symbol>@depth<levels> OR <symbol>@depth<levels>@100ms.
        Update Speed: 1000ms or 100ms

        """
        self.live_subscribe(f"{symbol.lower()}@depth{level}@{speed}ms", id, **kwargs)

    def diff_book_depth(self, symbol: str, id: int, speed, **kwargs):
        """
        Diff book depth stream.

        Stream Name: <symbol>@depth OR <symbol>@depth@100ms
        Update Speed: 1000ms or 100ms
        Order book price and quantity depth updates used to locally manage an order book.

        """
        self.live_subscribe(f"{symbol.lower()}@depth@{speed}ms", id, **kwargs)

    def user_data(self, listen_key: str, id: int, **kwargs):
        """
        Listen to user data by provided `listenkey`.
        """
        self.live_subscribe(listen_key, id, **kwargs)
