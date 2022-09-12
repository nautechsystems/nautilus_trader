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

from nautilus_trader.core.message import Event
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.events.position import PositionChanged
from nautilus_trader.model.events.position import PositionClosed
from nautilus_trader.model.events.position import PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.trading.strategy import Strategy


class MarketMaker(Strategy):
    """
    Provides a market making strategy for testing.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    trade_size : Decimal
        The position size per trade.
    max_size : Decimal
        The maximum inventory size allowed.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        trade_size: Decimal,
        max_size: Decimal,
    ):
        super().__init__()

        # Configuration
        self.instrument_id = instrument_id
        self.trade_size = trade_size
        self.max_size = max_size

        self.instrument: Optional[Instrument] = None  # Initialized in on_start
        self._book: Optional[OrderBook] = None
        self._mid: Optional[Decimal] = None
        self._adj = Decimal(0)

    def on_start(self):
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        # Create orderbook
        self._book = OrderBook.create(instrument=self.instrument, book_type=BookType.L2_MBP)

        # Subscribe to live data
        self.subscribe_order_book_deltas(self.instrument_id)

    def on_order_book_delta(self, delta: OrderBookData):
        self._book.apply(delta)
        bid_price = self._book.best_bid_price()
        ask_price = self._book.best_ask_price()
        if bid_price and ask_price:
            mid = (bid_price + ask_price) / 2
            if mid != self._mid:
                self.cancel_all_orders(self.instrument_id)
                self._mid = Decimal(mid)
                val = self._mid + self._adj
                self.buy(price=val * Decimal(1.01))
                self.sell(price=val * Decimal(0.99))

    def on_event(self, event: Event):
        if isinstance(event, (PositionOpened, PositionChanged)):
            net_qty = event.quantity.as_decimal()
            if event.side == PositionSide.SHORT:
                net_qty = -net_qty
            self._adj = (net_qty / self.max_size) * Decimal(0.01)
        elif isinstance(event, PositionClosed):
            self._adj = Decimal(0)

    def buy(self, price: Decimal):
        """
        Users simple buy method (example).
        """
        order = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            price=Price(price, precision=self.instrument.price_precision),
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    def sell(self, price: Decimal):
        """
        Users simple sell method (example).
        """
        order = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            price=Price(price, precision=self.instrument.price_precision),
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.instrument_id)
        self.close_all_positions(self.instrument_id)
