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

from adapters.betfair.data import InstrumentSearch
from nautilus_trader.common.enums import LogColor
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order.limit import LimitOrder
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.trading.strategy import TradingStrategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class DumbQuoter(TradingStrategy):
    """
    A simple quoting strategy

    Cancels all orders and flattens all positions on stop.
    """

    def __init__(
        self,
        instrument_filter: dict,
        trade_size: Decimal,
        order_id_tag: str,  # Must be unique at 'trader level'
        theo_change_threshold=Decimal(0.01),
        market_width=Decimal(0.05),
    ):
        """
        Initialize a new instance of the `EMACross` class.

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

        # Custom strategy variables
        self.trade_size = trade_size
        self.market_width = market_width

    def on_start(self):
        """Actions to be performed on strategy start."""
        # Get historical data
        self.request_data(
            "BETFAIR", DataType(InstrumentSearch, metadata=self.instrument_filter)
        )

    def on_data(self, data: GenericData):
        self.log.info(data)
        if data.data_type.data_type == InstrumentSearch:
            # Find and set instrument
            instruments = data.data
            self.log.info(f"Received {len(instruments)} from instrument search")
            self.instrument_id = instruments[0].id
            # Subscribe to live data
            self.subscribe_order_book(
                instrument_id=self.instrument_id,
                level=OrderBookLevel.L2,
            )
        else:
            super().on_data(data)

    def update_midpoint(self, order_book: OrderBook):
        """ Check if midpoint has moved more than threshold, if so , update quotes """
        midpoint = (order_book.best_ask_price() + order_book.best_bid_price()) / 2.0
        self.log.info(f"midpoint: {midpoint}, prev: {self.midpoint}")
        if abs(midpoint - (self.midpoint or -1e15)) > self.theo_change_threshold:
            self.log.info("Theo updating", LogColor.BLUE)
            self.buy(price=midpoint)
            self.sell(price=midpoint)
            self.midpoint = midpoint

    def on_order_book(self, order_book: OrderBook):
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        """
        self.log.info(
            f"Received {repr(order_book)}"
        )  # For debugging (must add a subscription)
        if order_book.spread():
            self.update_midpoint(order_book=order_book)

    def buy(self, price):
        """
        Users simple buy method (example).
        """
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            price=price - (self.market_width / 2.0),
            order_side=OrderSide.BUY,
            quantity=Quantity(self.trade_size),
            time_in_force=TimeInForce.GTC,
        )

        self.submit_order(order)

    def sell(self, price):
        """
        Users simple sell method (example).
        """
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            price=price + (self.market_width / 2.0),
            quantity=Quantity(self.trade_size),
            time_in_force=TimeInForce.GTC,
        )

        self.submit_order(order)

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.instrument_id)
        self.flatten_all_positions(self.instrument_id)
