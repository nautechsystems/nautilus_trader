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
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from typing import Optional

from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.trading.strategy import TradingStrategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class OrderbookImbalance(TradingStrategy):
    """
    A simple strategy that sends FAK limit orders when there is a bid/ask
    imbalance in the order book.

    Cancels all orders and flattens all positions on stop.
    """

    def __init__(
        self,
        # instrument_filter: dict,
        instrument: Instrument,
        max_trade_size: Decimal,
        order_id_tag: str,  # Must be unique at 'trader level'
        trigger_min_size=100.0,
        trigger_imbalance_ratio=0.20,
    ):
        """
        Initialize a new instance of the ``OrderbookImbalance`` class.

        Parameters
        ----------
        instrument : Instrument
            The instrument to trade
        max_trade_size : Decimal
            The max position size per trade (volume on the level can be less).
        order_id_tag : str
            The unique order ID tag for the strategy. Must be unique
            amongst all running strategies for a particular trader ID.
        trigger_min_size : float
            The minimum size on the larger side to trigger an order.
        trigger_imbalance_ratio : float
            The ratio of bid:ask volume required to trigger an order (smaller
            value / larger value) ie given a trigger_imbalance_ratio=0.2, and a
            bid volume of 100, we will send a buy order if the ask volume is <
            20).

        """
        assert 0 < trigger_imbalance_ratio < 1
        super().__init__(order_id_tag=order_id_tag)
        # self.instrument_filter = instrument_filter
        self.instrument = instrument
        self.max_trade_size = max_trade_size
        self.trigger_min_size = trigger_min_size
        self.trigger_imbalance_ratio = trigger_imbalance_ratio
        self._book = None  # type: Optional[OrderBook]

    def on_start(self):
        """Actions to be performed on strategy start."""
        # self.request_data(
        #     "BETFAIR", DataType(InstrumentSearch, metadata=self.instrument_filter)
        # )
        self.subscribe_order_book_deltas(
            instrument_id=self.instrument.id,
            level=BookLevel.L2,
        )
        self._book = OrderBook.create(
            instrument=self.instrument,
            level=BookLevel.L2,
        )

    def on_order_book_delta(self, delta: OrderBookDelta):
        """Actions to be performed when a delta is received."""
        self._book.apply(delta)
        if self._book.spread():
            self.check_trigger()

    def on_order_book(self, order_book: OrderBook):
        """Actions to be performed when an order book update is received."""
        self._book = order_book
        if self._book.spread():
            self.check_trigger()

    def check_trigger(self):
        """Checking for trigger conditions."""
        bid_volume = self._book.best_bid_qty()
        ask_volume = self._book.best_ask_qty()
        if not (bid_volume and ask_volume):
            return
        smaller = min(bid_volume, ask_volume)
        larger = max(bid_volume, ask_volume)
        ratio = smaller / larger
        if larger > self.trigger_min_size and ratio < self.trigger_imbalance_ratio:
            if bid_volume > ask_volume:
                order = self.order_factory.limit(
                    instrument_id=self.instrument.id,
                    price=self.instrument.make_price(self._book.best_ask_price()),
                    order_side=OrderSide.BUY,
                    quantity=self.instrument.make_qty(ask_volume),
                    post_only=False,
                )
                self.submit_order(order)
            else:
                order = self.order_factory.limit(
                    instrument_id=self.instrument.id,
                    price=self.instrument.make_price(self._book.best_bid_price()),
                    order_side=OrderSide.SELL,
                    quantity=self.instrument.make_qty(bid_volume),
                    post_only=False,
                )
                self.submit_order(order)

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.instrument.id)
        self.flatten_all_positions(self.instrument.id)
