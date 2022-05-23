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

from typing import Optional

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY ***


class SubscribeStrategyConfig(StrategyConfig):
    """
    Configuration for ``SubscribeStrategy`` instances.
    """

    instrument_id: str
    book_type: Optional[BookType] = None
    snapshots: bool = False
    trade_ticks: bool = False
    quote_ticks: bool = False
    bars: bool = False


class SubscribeStrategy(Strategy):
    """
    A strategy that simply subscribes to data and logs it (typically for testing adapters)

    Parameters
    ----------
    config : OrderbookImbalanceConfig
        The configuration for the instance.
    """

    def __init__(self, config: SubscribeStrategyConfig):
        super().__init__(config)
        self.instrument_id = InstrumentId.from_str(self.config.instrument_id)
        self.book: Optional[OrderBook] = None

    def on_start(self):
        """Actions to be performed on strategy start."""
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        if self.config.book_type:
            self.book = OrderBook.create(
                instrument=self.instrument, book_type=self.config.book_type
            )
            if self.config.snapshots:
                self.subscribe_order_book_snapshots(
                    instrument_id=self.instrument_id, book_type=self.config.book_type
                )
            else:
                self.subscribe_order_book_deltas(
                    instrument_id=self.instrument_id, book_type=self.config.book_type
                )

        if self.config.trade_ticks:
            self.subscribe_trade_ticks(instrument_id=self.instrument_id)
        if self.config.quote_ticks:
            self.subscribe_quote_ticks(instrument_id=self.instrument_id)
        if self.config.bars:
            bar_type: BarType = BarType(
                instrument_id=self.instrument_id,
                bar_spec=BarSpecification(
                    step=1, aggregation=BarAggregation.SECOND, price_type=PriceType.LAST
                ),
                aggregation_source=AggregationSource.EXTERNAL,
            )
            self.subscribe_bars(bar_type)

    def on_order_book_delta(self, data: OrderBookData):
        self.book.apply(data)
        self.log.info(str(self.book))

    def on_order_book(self, order_book: OrderBook):
        self.book = order_book
        self.log.info(str(self.book))

    def on_trade_tick(self, tick: TradeTick):
        self.log.info(str(tick))

    def on_quote_tick(self, tick: QuoteTick):
        self.log.info(str(tick))

    def on_bar(self, bar: Bar):
        self.log.info(str(bar))
