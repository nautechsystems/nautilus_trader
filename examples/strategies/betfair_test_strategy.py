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

from nautilus_trader.adapters.betfair.data import InstrumentSearch
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.message import Event
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.model.data import Data
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.trading.strategy import TradingStrategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class BetfairTestStrategy(TradingStrategy):
    """
    A simple quoting strategy

    Cancels all orders and flattens all positions on stop.
    """

    def __init__(
        self,
        instrument_filter: dict,
        trade_size: Decimal,
        order_id_tag: str,  # Must be unique at 'trader level'
        theo_change_threshold=Decimal(0.01),  # noqa (B008)
        market_width=Decimal(0.05),  # noqa (B008)
    ):
        """
        Initialize a new instance of the ``BetfairTestStrategy`` class.

        Parameters
        ----------
        instrument_filter: dict
            The dict of filters to search for an instrument
        theo_change_threshold: Decimal
        trade_size : Decimal
            The position size per trade.
        order_id_tag : str
            The unique order identifier tag for the strategy. Must be unique
            amongst all running strategies for a particular trader identifier.

        """
        super().__init__(order_id_tag=order_id_tag)
        self.instrument_filter = instrument_filter
        self.theo_change_threshold = theo_change_threshold
        self.instrument_id = None
        self.midpoint = None
        self.trade_size = trade_size
        self.market_width = market_width
        self._in_flight = set()
        self._state = "START"

    def on_start(self):
        """Actions to be performed on strategy start."""
        self.request_data(
            "BETFAIR", DataType(InstrumentSearch, metadata=self.instrument_filter)
        )

    def on_event(self, event: Event):
        self.log.info(f"on_event: {event}")
        if isinstance(event, (OrderAccepted, OrderUpdated, OrderCanceled)):
            self._in_flight.remove(event.client_order_id)
        self.trigger()

    def on_data(self, data: Data):
        # self.log.debug(str(data))
        if isinstance(data, InstrumentSearch):
            # Find and set instrument
            self.log.info(f"Received {len(data.instruments)} from instrument search")
            self.instrument_id = data.instruments[0].id
            # Subscribe to live data
            self.subscribe_order_book_deltas(
                instrument_id=self.instrument_id,
                level=OrderBookLevel.L2,
            )
            self.book = OrderBook(
                instrument_id=self.instrument_id,
                level=OrderBookLevel.L2,
            )

    def on_order_book_delta(self, delta: OrderBookDelta):
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        delta : OrderBookDelta
            The order book received.

        """
        # self.log.debug(
        #     f"Received {repr(order_book)}"
        # )  # For debugging (must add a subscription)
        self.book.apply(delta)
        if self.book.spread():
            self.update_midpoint(order_book=self.book)
            self.log.info(f"on_order_book {self._in_flight}")
            self.trigger()

    def on_order_book(self, order_book: OrderBook):
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        """
        # self.log.debug(
        #     f"Received {repr(order_book)}"
        # )  # For debugging (must add a subscription)
        if order_book.spread():
            self.update_midpoint(order_book=order_book)
            self.log.info(f"on_order_book {self._in_flight}")
            self.trigger()

    def trigger(self):
        if self._state == "START" and self.midpoint:
            self.log.info("Sending orders", color=LogColor.YELLOW)
            self.send_orders(midpoint=self.midpoint)
        elif self._state == "UPDATE" and not self._in_flight:
            self.log.info("Sending order update...", color=LogColor.YELLOW)
            for client_order_id in self.cache.client_order_ids_working():
                order = self.cache.order(client_order_id)
                new_price = (
                    order.price * 0.90
                    if order.side == OrderSide.BUY
                    else order.price * 1.10
                )
                self._in_flight.add(order.client_order_id)
                self.update_order(order=order, price=Price(new_price, precision=5))
            self._state = "CANCEL"
            self.log.info(f"Trigger cancel {self._in_flight}", color=LogColor.YELLOW)
        elif self._state == "CANCEL" and not self._in_flight:
            orders = self.cache.orders()
            self.log.info(f"Sending cancel for orders: {orders}", color=LogColor.YELLOW)
            for order in orders:
                self.cancel_order(order=order)
                self._in_flight.add(order.client_order_id)
            self._state = "COMPLETE"
        elif self._state == "COMPLETE":
            self.log.info("Complete - shutting down", color=LogColor.YELLOW)
            self.stop()

    def update_midpoint(self, order_book: OrderBook):
        """
        Check if midpoint has moved more than threshold, if so , update quotes.
        """

        midpoint = Decimal(
            order_book.best_ask_price() + order_book.best_bid_price()
        ) / Decimal(2.0)
        self.log.info(f"midpoint: {midpoint}, prev: {self.midpoint}")
        if (
            abs(midpoint - (self.midpoint or Decimal(-1e15)))
            > self.theo_change_threshold
        ):
            self.log.info("Theo updating", LogColor.BLUE)
            self.midpoint = midpoint

    def send_orders(self, midpoint):
        # sell_price = midpoint + (self.market_width / Decimal(2.0))
        buy_price = midpoint - (self.market_width / Decimal(2.0))
        self.buy(price=buy_price)
        # self.sell(price=sell_price)
        self._state = "UPDATE"

    def buy(self, price):
        """
        Users simple buy method (example).
        """
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            price=Price(price, precision=5),
            order_side=OrderSide.BUY,
            quantity=Quantity(self.trade_size, precision=0),
            time_in_force=TimeInForce.GTC,
        )
        self._in_flight.add(order.client_order_id)
        self.submit_order(order)

    def sell(self, price):
        """
        Users simple sell method (example).
        """
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            price=Price(price + (self.market_width / Decimal(2.0)), precision=5),
            quantity=Quantity(self.trade_size, precision=0),
            time_in_force=TimeInForce.GTC,
        )
        self._in_flight.add(order.client_order_id)
        self.submit_order(order)

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.instrument_id)
        self.flatten_all_positions(self.instrument_id)
