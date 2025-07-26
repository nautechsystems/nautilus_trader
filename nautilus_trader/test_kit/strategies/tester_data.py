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

from typing import Any

import pandas as pd

from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import PositiveInt
from nautilus_trader.common.enums import LogColor
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


class DataTesterConfig(ActorConfig, frozen=True):
    """
    Configuration for ``DataTester`` instances.
    """

    instrument_ids: list[InstrumentId]
    bar_types: list[BarType] | None = None
    subscribe_book_deltas: bool = False
    subscribe_book_depth: bool = False
    subscribe_book_at_interval: bool = False
    subscribe_quotes: bool = False
    subscribe_trades: bool = False
    subscribe_mark_prices: bool = False
    subscribe_index_prices: bool = False
    subscribe_bars: bool = False
    subscribe_instrument_status: bool = False
    subscribe_instrument_close: bool = False
    can_unsubscribe: bool = True
    request_instruments: bool = False
    request_quotes: bool = False
    request_trades: bool = False
    request_bars: bool = False
    requests_start_delta: pd.Timedelta | None = None
    book_type: BookType = BookType.L2_MBP
    book_depth: PositiveInt | None = None
    book_interval_ms: PositiveInt = 1000
    book_levels_to_print: PositiveInt = 10
    client_id: ClientId | None = None


class DataTester(Actor):
    """
    An actor for testing data functionality for integration adapters.

    Parameters
    ----------
    config : DataTesterConfig
        The configuration for the instance.

    """

    def __init__(self, config: DataTesterConfig) -> None:
        super().__init__(config)

    def on_start(self) -> None:  # noqa: C901 (too complex)
        """
        Actions to be performed when the actor is started.
        """
        # Determine requests start
        requests_start_delta = self.config.requests_start_delta or pd.Timedelta(hours=1)
        requests_start = self.clock.utc_now() - requests_start_delta

        client_id = self.config.client_id

        for instrument_id in self.config.instrument_ids or []:
            if self.config.subscribe_book_deltas:
                self.subscribe_order_book_deltas(
                    instrument_id=instrument_id,
                    book_type=self.config.book_type,
                    client_id=client_id,
                )

            if self.config.subscribe_book_depth:
                self.subscribe_order_book_depth(
                    instrument_id=instrument_id,
                    book_type=self.config.book_type,
                    depth=self.config.book_depth or 10,
                    client_id=client_id,
                )

            if self.config.subscribe_book_at_interval:
                self.subscribe_order_book_at_interval(
                    instrument_id=instrument_id,
                    book_type=self.config.book_type,
                    depth=self.config.book_depth or 0,
                    interval_ms=self.config.book_interval_ms,
                    client_id=client_id,
                )

            if self.config.subscribe_quotes:
                self.subscribe_quote_ticks(instrument_id, client_id=client_id)

            if self.config.subscribe_trades:
                self.subscribe_trade_ticks(instrument_id, client_id=client_id)

            if self.config.subscribe_instrument_status:
                self.subscribe_instrument_status(instrument_id, client_id=client_id)

            if self.config.subscribe_instrument_close:
                self.subscribe_instrument_close(instrument_id, client_id=client_id)

            if self.config.request_trades:
                self.request_trade_ticks(instrument_id, start=requests_start, client_id=client_id)

        for bar_type in self.config.bar_types or []:
            if self.config.subscribe_bars:
                self.subscribe_bars(bar_type, client_id=client_id)

            if self.config.request_bars:
                self.request_bars(bar_type, start=requests_start, client_id=client_id)

    def on_stop(self) -> None:  # noqa: C901 (too complex)
        """
        Actions to be performed when the actor is stopped.
        """
        if not self.config.can_unsubscribe:
            return  # Unsubscribe not supported

        client_id = self.config.client_id

        for instrument_id in self.config.instrument_ids or []:
            if self.config.subscribe_book_deltas:
                self.unsubscribe_order_book_deltas(
                    instrument_id=instrument_id,
                    client_id=client_id,
                )

            if self.config.subscribe_book_depth:
                self.unsubscribe_order_book_depth(
                    instrument_id=instrument_id,
                    client_id=client_id,
                )

            if self.config.subscribe_book_at_interval:
                self.unsubscribe_order_book_at_interval(
                    instrument_id=instrument_id,
                    interval_ms=self.config.book_interval_ms,
                    client_id=client_id,
                )

            if self.config.subscribe_quotes:
                self.unsubscribe_quote_ticks(instrument_id, client_id=client_id)

            if self.config.subscribe_trades:
                self.unsubscribe_trade_ticks(instrument_id, client_id=client_id)

            if self.config.subscribe_instrument_status:
                self.unsubscribe_instrument_status(instrument_id, client_id=client_id)

            if self.config.subscribe_instrument_close:
                self.unsubscribe_instrument_close(instrument_id, client_id=client_id)

        for bar_type in self.config.bar_types or []:
            if self.config.subscribe_bars:
                self.unsubscribe_bars(bar_type, client_id=client_id)

    def on_historical_data(self, data: Any) -> None:
        """
        Actions to be performed when the actor is running and receives historical data.
        """
        self.log.info(repr(data), LogColor.CYAN)

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when the actor is running and receives order book
        deltas.
        """
        self.log.info(repr(deltas), LogColor.CYAN)

    def on_order_book_depth(self, depth: OrderBookDepth10) -> None:
        """
        Actions to be performed when the actor is running and receives order book depth.
        """
        self.log.info(repr(depth), LogColor.CYAN)

    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when an order book update is received.
        """
        num_levels = 10
        self.log.info(
            f"\n{order_book.instrument_id}\n{order_book.pprint(num_levels)}",
            LogColor.CYAN,
        )

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the actor is running and receives a quote.
        """
        self.log.info(repr(tick), LogColor.CYAN)

    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Actions to be performed when the actor is running and receives a trade.
        """
        self.log.info(repr(tick), LogColor.CYAN)

    def on_bar(self, bar: Bar) -> None:
        """
        Actions to be performed when the actor is running and receives a bar.
        """
        self.log.info(repr(bar), LogColor.CYAN)
