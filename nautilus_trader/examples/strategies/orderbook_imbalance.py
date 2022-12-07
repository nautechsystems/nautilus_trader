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

from decimal import Decimal
from typing import Optional

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.c_enums.book_type import BookTypeParser
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.data import BookOrder
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class OrderBookImbalanceConfig(StrategyConfig):
    """
    Configuration for ``OrderBookImbalance`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    max_trade_size : str
        The max position size per trade (volume on the level can be less).
    trigger_min_size : float
        The minimum size on the larger side to trigger an order.
    trigger_imbalance_ratio : float
        The ratio of bid:ask volume required to trigger an order (smaller
        value / larger value) ie given a trigger_imbalance_ratio=0.2, and a
        bid volume of 100, we will send a buy order if the ask volume is <
        20).
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OMSType
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).
    """

    instrument_id: str
    max_trade_size: Decimal
    trigger_min_size: float = 100.0
    trigger_imbalance_ratio: float = 0.20
    book_type: str = "L2_MBP"
    use_quote_ticks: bool = False


class OrderBookImbalance(Strategy):
    """
    A simple strategy that sends FOK limit orders when there is a bid/ask
    imbalance in the order book.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : OrderbookImbalanceConfig
        The configuration for the instance.
    """

    def __init__(self, config: OrderBookImbalanceConfig):
        assert 0 < config.trigger_imbalance_ratio < 1
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.max_trade_size = Decimal(config.max_trade_size)
        self.trigger_min_size = config.trigger_min_size
        self.trigger_imbalance_ratio = config.trigger_imbalance_ratio
        self.instrument: Optional[Instrument] = None
        if self.config.use_quote_ticks:
            assert self.config.book_type == "L1_TBBO"
        self.book_type: BookType = BookTypeParser.from_str_py(self.config.book_type)
        self._book = None  # type: Optional[OrderBook]

    def on_start(self):
        """Actions to be performed on strategy start."""
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        if self.config.use_quote_ticks:
            book_type = BookType.L1_TBBO
            self.subscribe_quote_ticks(instrument_id=self.instrument.id)
        else:
            book_type = BookTypeParser.from_str_py(self.config.book_type)
            self.subscribe_order_book_deltas(instrument_id=self.instrument.id, book_type=book_type)
        self._book = OrderBook.create(instrument=self.instrument, book_type=book_type)

    def on_order_book_delta(self, data: OrderBookData):
        """Actions to be performed when a delta is received."""
        if not self._book:
            self.log.error("No book being maintained.")
            return

        self._book.apply(data)
        if self._book.spread():
            self.check_trigger()

    def on_quote_tick(self, tick: QuoteTick):
        """Actions to be performed when a delta is received."""
        bid = BookOrder(
            price=tick.bid.as_double(),
            size=tick.bid_size.as_double(),
            side=OrderSide.BUY,
        )
        ask = BookOrder(
            price=tick.ask.as_double(),
            size=tick.ask_size.as_double(),
            side=OrderSide.SELL,
        )

        self._book.clear()
        self._book.update(bid)
        self._book.update(ask)
        if self._book.spread():
            self.check_trigger()

    def on_order_book(self, order_book: OrderBook):
        """Actions to be performed when an order book update is received."""
        self._book = order_book
        if self._book.spread():
            self.check_trigger()

    def check_trigger(self):
        """Check for trigger conditions."""
        if not self._book:
            self.log.error("No book being maintained.")
            return

        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        bid_volume = self._book.best_bid_qty()
        ask_volume = self._book.best_ask_qty()
        if not (bid_volume and ask_volume):
            return

        smaller = min(bid_volume, ask_volume)
        larger = max(bid_volume, ask_volume)
        ratio = smaller / larger
        self.log.info(
            f"Book: {self._book.best_bid_price()} @ {self._book.best_ask_price()} ({ratio=:0.2f})",
        )
        if larger > self.trigger_min_size and ratio < self.trigger_imbalance_ratio:
            if len(self.cache.orders_inflight(strategy_id=self.id)) > 0:
                pass
            elif bid_volume > ask_volume:
                order = self.order_factory.limit(
                    instrument_id=self.instrument.id,
                    price=self.instrument.make_price(self._book.best_ask_price()),
                    order_side=OrderSide.BUY,
                    quantity=self.instrument.make_qty(ask_volume),
                    post_only=False,
                    time_in_force=TimeInForce.FOK,
                )
                self.submit_order(order)
            else:
                order = self.order_factory.limit(
                    instrument_id=self.instrument.id,
                    price=self.instrument.make_price(self._book.best_bid_price()),
                    order_side=OrderSide.SELL,
                    quantity=self.instrument.make_qty(bid_volume),
                    post_only=False,
                    time_in_force=TimeInForce.FOK,
                )
                self.submit_order(order)

    def on_stop(self):
        """Actions to be performed when the strategy is stopped."""
        if self.instrument is None:
            return
        self.cancel_all_orders(self.instrument.id)
        self.close_all_positions(self.instrument.id)
