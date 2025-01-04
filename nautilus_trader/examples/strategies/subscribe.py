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

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY ***


class SubscribeStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``SubscribeStrategy`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.

    """

    instrument_id: InstrumentId
    book_type: BookType | None = None
    snapshots: bool = False
    trade_ticks: bool = False
    quote_ticks: bool = False
    bars: bool = False


class SubscribeStrategy(Strategy):
    """
    A strategy that simply subscribes to data and logs it (typically for testing
    adapters)

    Parameters
    ----------
    config : OrderbookImbalanceConfig
        The configuration for the instance.

    """

    def __init__(self, config: SubscribeStrategyConfig) -> None:
        super().__init__(config)
        self.book: OrderBook | None = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        if self.config.book_type:
            self.book = OrderBook(
                instrument_id=self.instrument.id,
                book_type=self.config.book_type,
            )
            if self.config.snapshots:
                self.subscribe_order_book_at_interval(
                    instrument_id=self.config.instrument_id,
                    book_type=self.config.book_type,
                )
            else:
                self.subscribe_order_book_deltas(
                    instrument_id=self.config.instrument_id,
                    book_type=self.config.book_type,
                )

        if self.config.trade_ticks:
            self.subscribe_trade_ticks(instrument_id=self.config.instrument_id)
        if self.config.quote_ticks:
            self.subscribe_quote_ticks(instrument_id=self.config.instrument_id)
        if self.config.bars:
            bar_type: BarType = BarType(
                instrument_id=self.config.instrument_id,
                bar_spec=BarSpecification(
                    step=5,
                    aggregation=BarAggregation.SECOND,
                    price_type=PriceType.LAST,
                ),
                aggregation_source=AggregationSource.EXTERNAL,
            )
            self.subscribe_bars(bar_type)

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        if not self.book:
            self.log.error("No book being maintained")
            return

        self.book.apply_deltas(deltas)
        self.log.info(str(self.book))

    def on_order_book(self, order_book: OrderBook) -> None:
        self.book = order_book
        self.log.info(str(self.book))

    def on_trade_tick(self, tick: TradeTick) -> None:
        self.log.info(str(tick))

    def on_quote_tick(self, tick: QuoteTick) -> None:
        self.log.info(str(tick))

    def on_bar(self, bar: Bar) -> None:
        self.log.info(str(bar))
