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

from typing import Callable

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.model.functions cimport order_type_to_str
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class MatchingCore:
    """
    Provides a generic order matching core.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the matching core.
    price_increment : Price
        The minimum price increment (tick size) for the matching core.
    trigger_stop_order : Callable[[Order], None]
        The callable when a stop order is triggered.
    fill_market_order : Callable[[Order], None]
        The callable when a market order is filled.
    fill_limit_order : Callable[[Order], None]
        The callable when a limit order is filled.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Price price_increment not None,
        trigger_stop_order not None: Callable,
        fill_market_order not None: Callable,
        fill_limit_order not None: Callable,
    ):
        self._instrument_id = instrument_id
        self._price_increment = price_increment
        self._price_precision = price_increment.precision

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
    def instrument_id(self) -> InstrumentId:
        """
        Return the instrument ID for the matching core.

        Returns
        -------
        InstrumentId

        """
        return self._instrument_id

    @property
    def price_precision(self) -> int:
        """
        Return the instruments price precision for the matching core.

        Returns
        -------
        int

        """
        return self._price_increment.precision

    @property
    def price_increment(self) -> Price:
        """
        Return the instruments minimum price increment (tick size) for the matching core.

        Returns
        -------
        Price

        """
        return self._price_increment

    @property
    def bid(self) -> Price | None:
        """
        Return the current bid price for the matching core.

        Returns
        -------
        Price or ``None``

        """
        if not self.is_bid_initialized:
            return None
        else:
            return Price.from_raw_c(self.bid_raw, self._price_precision)

    @property
    def ask(self) -> Price | None:
        """
        Return the current ask price for the matching core.

        Returns
        -------
        Price or ``None``

        """
        if not self.is_ask_initialized:
            return None
        else:
            return Price.from_raw_c(self.ask_raw, self._price_precision)

    @property
    def last(self) -> Price | None:
        """
        Return the current last price for the matching core.

        Returns
        -------
        Price or ``None``

        """
        if not self.is_last_initialized:
            return None
        else:
            return Price.from_raw_c(self.last_raw, self._price_precision)

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Order get_order(self, ClientOrderId client_order_id):
        Condition.not_none(client_order_id, "client_order_id")
        return self._orders.get(client_order_id)

    cpdef bint order_exists(self, ClientOrderId client_order_id):
        Condition.not_none(client_order_id, "client_order_id")
        return client_order_id in self._orders

    cpdef list get_orders(self):
        return self._orders_bid + self._orders_ask

    cpdef list get_orders_bid(self):
        return self._orders_bid

    cpdef list get_orders_ask(self):
        return self._orders_ask

# -- COMMANDS -------------------------------------------------------------------------------------

    cdef void set_bid_raw(self, PriceRaw bid_raw):
        self.is_bid_initialized = True
        self.bid_raw = bid_raw

    cdef void set_ask_raw(self, PriceRaw ask_raw):
        self.is_ask_initialized = True
        self.ask_raw = ask_raw

    cdef void set_last_raw(self, PriceRaw last_raw):
        self.is_last_initialized = True
        self.last_raw = last_raw

    cpdef void reset(self):
        self._orders.clear()
        self._orders_bid.clear()
        self._orders_ask.clear()
        self.bid_raw = 0
        self.ask_raw = 0
        self.last_raw = 0
        self.is_bid_initialized = False
        self.is_ask_initialized = False
        self.is_last_initialized = False

    cpdef void add_order(self, Order order):
        Condition.not_none(order, "order")

        # Needed as closures not supported in cpdef functions
        self._add_order(order)

    cdef void _add_order(self, Order order):
        # Index order
        self._orders[order.client_order_id] = order

        if order.side == OrderSide.BUY:
            self._orders_bid.append(order)
            self.sort_bid_orders()
        elif order.side == OrderSide.SELL:
            self._orders_ask.append(order)
            self.sort_ask_orders()
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)

    cdef void sort_bid_orders(self):
        self._orders_bid.sort(key=order_sort_key, reverse=True)

    cdef void sort_ask_orders(self):
        self._orders_ask.sort(key=order_sort_key)

    cpdef void delete_order(self, Order order):
        Condition.not_none(order, "order")

        self._orders.pop(order.client_order_id, None)

        if order.side == OrderSide.BUY:
            if order in self._orders_bid:
                self._orders_bid.remove(order)
        elif order.side == OrderSide.SELL:
            if order in self._orders_ask:
                self._orders_ask.remove(order)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)

    cpdef void iterate(self, uint64_t timestamp_ns):
        cdef Order order
        for order in self._orders_bid + self._orders_ask:  # Lists implicitly copied
            if order.is_closed_c():
                continue  # Orders state has changed since iteration started  # pragma: no cover
            self.match_order(order)

# -- MATCHING -------------------------------------------------------------------------------------

    cpdef void match_order(self, Order order, bint initial = False):
        """
        Match the given order.

        Parameters
        ----------
        order : Order
            The order to match.
        initial : bool, default False
            If this is an initial match.

        Raises
        ------
        TypeError
            If the `order.order_type` is an invalid type for the core (e.g. `MARKET`).

        """
        Condition.not_none(order, "order")

        if (
            order.order_type == OrderType.LIMIT
            or order.order_type == OrderType.MARKET_TO_LIMIT
        ):
            self.match_limit_order(order)
        elif (
            order.order_type == OrderType.STOP_LIMIT
            or order.order_type == OrderType.TRAILING_STOP_LIMIT
        ):
            self.match_stop_limit_order(order, initial)
        elif (
            order.order_type == OrderType.STOP_MARKET
            or order.order_type == OrderType.TRAILING_STOP_MARKET
        ):
            self.match_stop_market_order(order)
        elif order.order_type == OrderType.LIMIT_IF_TOUCHED:
            self.match_limit_if_touched_order(order, initial)
        elif order.order_type == OrderType.MARKET_IF_TOUCHED:
            self.match_market_if_touched_order(order)
        else:
            raise TypeError(f"invalid `OrderType` was {order.order_type}")  # pragma: no cover (design-time error)

    cpdef void match_limit_order(self, Order order):
        Condition.not_none(order, "order")

        if self.is_limit_matched(order.side, order.price):
            order.liquidity_side = LiquiditySide.MAKER
            self._fill_limit_order(order)

    cpdef void match_stop_market_order(self, Order order):
        Condition.not_none(order, "order")

        if self.is_stop_triggered(order.side, order.trigger_price):
            order.set_triggered_price_c(order.trigger_price)
            # Triggered stop places market order
            self._fill_market_order(order)

    cpdef void match_stop_limit_order(self, Order order, bint initial):
        Condition.not_none(order, "order")

        if order.is_triggered:
            if self.is_limit_matched(order.side, order.price):
                order.liquidity_side = LiquiditySide.MAKER
                self._fill_limit_order(order)
            return

        cdef LiquiditySide liquidity_side
        if self.is_stop_triggered(order.side, order.trigger_price):
            order.set_triggered_price_c(order.trigger_price)
            order.liquidity_side = self._determine_order_liquidity(
                initial,
                order.side,
                order.price,
                order.trigger_price,
            )
            self._trigger_stop_order(order)
            # Check if immediately marketable
            if self.is_limit_matched(order.side, order.price):
                order.liquidity_side = LiquiditySide.TAKER
                self._fill_limit_order(order)

    cpdef void match_market_if_touched_order(self, Order order):
        Condition.not_none(order, "order")

        if self.is_touch_triggered(order.side, order.trigger_price):
            order.set_triggered_price_c(order.trigger_price)
            # Triggered stop places market order
            self._fill_market_order(order)

    cpdef void match_limit_if_touched_order(self, Order order, bint initial):
        Condition.not_none(order, "order")

        if order.is_triggered:
            if self.is_limit_matched(order.side, order.price):
                order.liquidity_side = LiquiditySide.MAKER
                self._fill_limit_order(order)
            return

        cdef LiquiditySide liquidity_side
        if self.is_touch_triggered(order.side, order.trigger_price):
            if not initial:
                order.set_triggered_price_c(order.trigger_price)
            order.liquidity_side = self._determine_order_liquidity(
                initial,
                order.side,
                order.price,
                order.trigger_price,
            )
            self._trigger_stop_order(order)
            # Check if immediately marketable
            if self.is_limit_matched(order.side, order.price):
                order.liquidity_side = LiquiditySide.TAKER
                self._fill_limit_order(order)

    cpdef bint is_limit_matched(self, OrderSide side, Price price):
        Condition.not_none(price, "price")

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

    cpdef bint is_stop_triggered(self, OrderSide side, Price trigger_price):
        Condition.not_none(trigger_price, "trigger_price")

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

    cpdef bint is_touch_triggered(self, OrderSide side, Price trigger_price):
        Condition.not_none(trigger_price, "trigger_price")

        if side == OrderSide.BUY:
            if not self.is_ask_initialized:
                return False  # No market
            return self.ask_raw <= trigger_price._mem.raw
        elif side == OrderSide.SELL:
            if not self.is_bid_initialized:
                return False  # No market
            return self.bid_raw >= trigger_price._mem.raw
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cdef LiquiditySide _determine_order_liquidity(
        self,
        bint initial,
        OrderSide side,
        Price price,
        Price trigger_price,
    ):
        if initial:
            return LiquiditySide.TAKER

        if side == OrderSide.BUY and trigger_price._mem.raw > price._mem.raw:
            return LiquiditySide.MAKER
        elif side == OrderSide.SELL and trigger_price._mem.raw < price._mem.raw:
            return LiquiditySide.MAKER

        return LiquiditySide.TAKER


cdef inline int64_t order_sort_key(Order order):
    cdef Price trigger_price
    cdef Price price
    if order.order_type == OrderType.LIMIT:
        price = order.price
        return price._mem.raw
    elif order.order_type == OrderType.MARKET_TO_LIMIT:
        price = order.price
        return price._mem.raw
    elif order.order_type == OrderType.STOP_MARKET:
        trigger_price = order.trigger_price
        return trigger_price._mem.raw
    elif order.order_type == OrderType.STOP_LIMIT:
        trigger_price = order.trigger_price
        price = order.price
        return price._mem.raw if order.is_triggered else trigger_price._mem.raw
    elif order.order_type == OrderType.MARKET_IF_TOUCHED:
        trigger_price = order.trigger_price
        return trigger_price._mem.raw
    elif order.order_type == OrderType.LIMIT_IF_TOUCHED:
        trigger_price = order.trigger_price
        price = order.price
        return price._mem.raw if order.is_triggered else trigger_price._mem.raw
    elif order.order_type == OrderType.TRAILING_STOP_MARKET:
        trigger_price = order.trigger_price
        return trigger_price._mem.raw
    elif order.order_type == OrderType.TRAILING_STOP_LIMIT:
        trigger_price = order.trigger_price
        price = order.price
        return price._mem.raw if order.is_triggered else trigger_price._mem.raw
    else:
        raise RuntimeError(  # pragma: no cover (design-time error)
            f"invalid order type to sort in book, "  # pragma: no cover (design-time error)
            f"was {order_type_to_str(order.order_type)}",  # pragma: no cover (design-time error)
        )
