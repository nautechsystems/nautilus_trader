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

from typing import Callable, Optional

from libc.limits cimport INT_MAX
from libc.limits cimport INT_MIN
from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class MatchingCore:
    """
    Provides a generic order matching core.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the matching core.
    trigger_stop_order : Callable[[Order], None]
        The callable when a stop order is triggered.
    fill_market_order : Callable[[Order, LiquiditySide], None]
        The callable when a market order is filled.
    fill_limit_order : Callable[[Order, LiquiditySide], None]
        The callable when a limit order is filled.
    """

    def __init__(
        self,
        Instrument instrument not None,
        trigger_stop_order not None: Callable,
        fill_market_order not None: Callable,
        fill_limit_order not None: Callable,
    ):
        self._instrument = instrument

        # Market
        self.bid_raw = 0
        self.ask_raw = 0
        self.last_raw = 0
        self.is_bid_initialized = False
        self.is_ask_initialized = False
        self.is_last_initialized = False

        # Event handlers
        self._trigger_stop_order = trigger_stop_order
        self._fill_market_order = fill_market_order
        self._fill_limit_order = fill_limit_order

        # Orders
        self._orders: dict[ClientOrderId, Order] = {}
        self._orders_bid: list[Order] = []
        self._orders_ask: list[Order] = []

    @property
    def bid(self) -> Optional[Price]:
        """
        Return the current bid price for the matching core.

        Returns
        -------
        Price or ``None``

        """
        if not self.is_bid_initialized:
            return None
        else:
            return Price.from_raw_c(self.bid_raw, self._instrument.price_precision)

    @property
    def ask(self) -> Optional[Price]:
        """
        Return the current ask price for the matching core.

        Returns
        -------
        Price or ``None``

        """
        if not self.is_ask_initialized:
            return None
        else:
            return Price.from_raw_c(self.ask_raw, self._instrument.price_precision)

    @property
    def last(self) -> Optional[Price]:
        """
        Return the current last price for the matching core.

        Returns
        -------
        Price or ``None``

        """
        if not self.is_last_initialized:
            return None
        else:
            return Price.from_raw_c(self.last_raw, self._instrument.price_precision)

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Order get_order(self, ClientOrderId client_order_id):
        return self._orders.get(client_order_id)

    cpdef bint order_exists(self, ClientOrderId client_order_id) except *:
        return client_order_id in self._orders

    cpdef list get_orders(self):
        return self._orders_bid + self._orders_ask

    cpdef list get_orders_bid(self):
        return self._orders_bid

    cpdef list get_orders_ask(self):
        return self._orders_ask

# -- COMMANDS -------------------------------------------------------------------------------------

    cdef void set_bid(self, Price_t bid) except *:
        self.is_bid_initialized = True
        self.bid_raw = bid.raw

    cdef void set_ask(self, Price_t ask) except *:
        self.is_ask_initialized = True
        self.ask_raw = ask.raw

    cdef void set_last(self, Price_t last) except *:
        self.is_last_initialized = True
        self.last_raw = last.raw

    cpdef void reset(self) except *:
        self._orders.clear()
        self._orders_bid.clear()
        self._orders_ask.clear()
        self.bid_raw = 0
        self.ask_raw = 0
        self.last_raw = 0
        self.is_bid_initialized = False
        self.is_ask_initialized = False
        self.is_last_initialized = False

    cpdef void add_order(self, Order order) except *:
        # Needed as closures not supported in cpdef functions
        self._add_order(order)

    cdef void _add_order(self, Order order) except *:
        # Index order
        self._orders[order.client_order_id] = order

        if order.side == OrderSide.BUY:
            self._orders_bid.append(order)
            self._orders_bid.sort(key=lambda o: o.price if (o.order_type == OrderType.LIMIT or o.order_type == OrderType.MARKET_TO_LIMIT) or (o.order_type == OrderType.STOP_LIMIT and o.is_triggered) else o.trigger_price or INT_MIN, reverse=True)  # noqa  TODO(cs): Will refactor!
        elif order.side == OrderSide.SELL:
            self._orders_ask.append(order)
            self._orders_ask.sort(key=lambda o: o.price if (o.order_type == OrderType.LIMIT or o.order_type == OrderType.MARKET_TO_LIMIT) or (o.order_type == OrderType.STOP_LIMIT and o.is_triggered) else o.trigger_price or INT_MAX)  # noqa  TODO(cs): Will refactor!
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)

    cpdef void delete_order(self, Order order) except *:
        self._orders.pop(order.client_order_id, None)

        if order.side == OrderSide.BUY:
            if order in self._orders_bid:
                self._orders_bid.remove(order)
        elif order.side == OrderSide.SELL:
            if order in self._orders_ask:
                self._orders_ask.remove(order)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)

    cpdef void iterate(self, uint64_t timestamp_ns) except *:
        cdef Order order
        for order in self._orders_bid + self._orders_ask:  # Lists implicitly copied
            if order.is_closed_c():
                continue  # Orders state has changed since the loop started
            self.match_order(order)

# -- MATCHING -------------------------------------------------------------------------------------

    cpdef void match_order(self, Order order) except *:
        if order.order_type == OrderType.LIMIT or order.order_type == OrderType.MARKET_TO_LIMIT:
            self.match_limit_order(order)
        elif (
            order.order_type == OrderType.STOP_MARKET
            or order.order_type == OrderType.MARKET_IF_TOUCHED
            or order.order_type == OrderType.TRAILING_STOP_MARKET
        ):
            self.match_stop_market_order(order)
        elif (
            order.order_type == OrderType.STOP_LIMIT
            or order.order_type == OrderType.LIMIT_IF_TOUCHED
            or order.order_type == OrderType.TRAILING_STOP_LIMIT
        ):
            self.match_stop_limit_order(order)
        else:
            raise ValueError(f"invalid `OrderType` was {order.order_type}")  # pragma: no cover (design-time error)

    cpdef void match_limit_order(self, Order order) except *:
        if self.is_limit_matched(order.side, order.price):
            self._fill_limit_order(order, LiquiditySide.MAKER)

    cpdef void match_stop_market_order(self, Order order) except *:
        if self.is_stop_triggered(order.side, order.trigger_price):
            # Triggered stop places market order
            self._fill_market_order(order, LiquiditySide.TAKER)

    cpdef void match_stop_limit_order(self, Order order) except *:
        if order.is_triggered:
            if self.is_limit_matched(order.side, order.price):
                self._fill_limit_order(order, LiquiditySide.MAKER)
            return

        if self.is_stop_triggered(order.side, order.trigger_price):
            self._trigger_stop_order(order)
            # Check if immediately marketable
            if self.is_limit_matched(order.side, order.price):
                self._fill_limit_order(order, LiquiditySide.TAKER)

    cpdef bint is_limit_matched(self, OrderSide side, Price price) except *:
        if side == OrderSide.BUY:
            if not self.is_ask_initialized:
                return False  # No market
            return self.ask_raw <= price._mem.raw
        elif side == OrderSide.SELL:
            if not self.is_bid_initialized:
                return False  # No market
            return self.bid_raw >= price._mem.raw
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef bint is_stop_triggered(self, OrderSide side, Price trigger_price) except *:
        if side == OrderSide.BUY:
            if not self.is_ask_initialized:
                return False  # No market
            return self.ask_raw >= trigger_price._mem.raw
        elif side == OrderSide.SELL:
            if not self.is_bid_initialized:
                return False  # No market
            return self.bid_raw <= trigger_price._mem.raw
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)
