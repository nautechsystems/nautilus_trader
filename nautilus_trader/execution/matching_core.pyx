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
from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetType
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetTypeParser
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class MatchingCore:
    """
    Provides an order matching core.
    """

    def __init__(
        self,
        Instrument instrument not None,
        expire_order not None: Callable,
        trigger_stop_order not None: Callable,
        update_trailing_stop not None: Callable,
        fill_market_order not None: Callable,
        fill_limit_order not None: Callable,
    ):
        self._instrument = instrument

        # Event handlers
        self._expire_order = expire_order
        self._trigger_stop_order = trigger_stop_order
        self._update_trailing_stop = update_trailing_stop
        self._fill_market_order = fill_market_order
        self._fill_limit_order = fill_limit_order

        # Orders
        self._orders: dict[ClientOrderId, Order] = {}
        self._orders_bid: list[Order] = []
        self._orders_ask: list[Order] = []

        # Market
        self.bid: Optional[Price] = None
        self.ask: Optional[Price] = None
        self.last: Optional[Price] = None

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

    cpdef void reset(self) except *:
        self._orders.clear()
        self._orders_bid.clear()
        self._orders_ask.clear()
        self.bid = None
        self.ask = None
        self.last = None

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

            # Check expiry
            if order.expire_time_ns > 0 and timestamp_ns >= order.expire_time_ns:
                self.delete_order(order)
                self._expire_order(order)

            # Check for match
            self.match_order(order)

            if order.is_open_c() and (order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT):
                self.update_trailing_stop(order)

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

    cpdef bint is_limit_marketable(self, OrderSide side, Price order_price) except *:
        if side == OrderSide.BUY:
            if self.ask is None:
                return False  # No market
            return order_price._mem.raw >= self.ask._mem.raw  # Match with LIMIT sells
        elif side == OrderSide.SELL:
            if self.bid is None:  # No market
                return False
            return order_price._mem.raw <= self.bid._mem.raw  # Match with LIMIT buys
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef bint is_limit_matched(self, OrderSide side, Price price) except *:
        if side == OrderSide.BUY:
            if self.ask is None:
                return False  # No market
            return price._mem.raw >= self.ask._mem.raw
        elif side == OrderSide.SELL:
            if self.bid is None:
                return False  # No market
            return price._mem.raw <= self.bid._mem.raw
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef bint is_stop_marketable(self, OrderSide side, Price price) except *:
        if side == OrderSide.BUY:
            if self.ask is None:
                return False  # No market
            return self.ask._mem.raw >= price._mem.raw  # Match with LIMIT sells
        elif side == OrderSide.SELL:
            if self.bid is None:
                return False  # No market
            return self.bid._mem.raw <= price._mem.raw  # Match with LIMIT buys
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef bint is_stop_triggered(self, OrderSide side, Price price) except *:
        if side == OrderSide.BUY:
            if self.ask is None:
                return False  # No market
            return self.ask._mem.raw >= price._mem.raw
        elif side == OrderSide.SELL:
            if self.bid is None:
                return False  # No market
            return self.bid._mem.raw <= price._mem.raw
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

# -- TRAILING STOP --------------------------------------------------------------------------------

    cpdef void update_trailing_stop(self, Order order) except *:
        cdef int64_t trailing_offset_raw = int(order.trailing_offset * int(FIXED_SCALAR))
        cdef int64_t limit_offset_raw = 0

        cdef Price trigger_price = order.trigger_price
        cdef Price price = None
        cdef Price new_trigger_price = None
        cdef Price new_price = None

        if order.order_type == OrderType.TRAILING_STOP_LIMIT:
            price = order.price
            limit_offset_raw = int(order.limit_offset * int(FIXED_SCALAR))

        cdef:
            Price temp_trigger_price
            Price temp_price
        if (
            order.trigger_type == TriggerType.DEFAULT
            or order.trigger_type == TriggerType.LAST
            or order.trigger_type == TriggerType.MARK
        ):
            if self.last is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(please add trade ticks or use bars)",
                )
            if order.side == OrderSide.BUY:
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        elif order.trigger_type == TriggerType.BID_ASK:
            if self.bid is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no BID price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )
            if self.ask is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no ASK price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )

            if order.side == OrderSide.BUY:
                temp_trigger_price = self._calculate_new_trailing_price_bid_ask(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_bid_ask(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = self._calculate_new_trailing_price_bid_ask(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_bid_ask(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        elif order.trigger_type == TriggerType.LAST_OR_BID_ASK:
            if self.last is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(please add trade ticks or use bars)",
                )
            if self.bid is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no BID price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )
            if self.ask is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no ASK price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )

            if order.side == OrderSide.BUY:
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                    trigger_price = new_trigger_price  # Set trigger to new trigger
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
                        price = new_price  # Set price to new price

                temp_trigger_price = self._calculate_new_trailing_price_bid_ask(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                )
                if trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_bid_ask(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                    trigger_price = new_trigger_price  # Set trigger to new trigger
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
                        price = new_price  # Set price to new price

                temp_trigger_price = self._calculate_new_trailing_price_bid_ask(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                )
                if trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_bid_ask(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"`TriggerType.{TriggerTypeParser.to_str(order.trigger_type)}` "
                f"not currently supported",
            )

        if new_trigger_price is None and new_price is None:
            return  # No updates

        self._update_trailing_stop(
            order,
            order.quantity,
            new_price,
            new_trigger_price,
        )

    cdef Price _calculate_new_trailing_price_last(
        self,
        Order order,
        TrailingOffsetType trailing_offset_type,
        double offset,
    ):
        cdef double last_f64 = self.last.as_f64_c()

        if trailing_offset_type == TrailingOffsetType.DEFAULT or trailing_offset_type == TrailingOffsetType.PRICE:
            pass  # Offset already calculated
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            offset = last_f64 * (offset / 100) / 100
        elif trailing_offset_type == TrailingOffsetType.TICKS:
            offset *= self._instrument.price_increment.as_f64_c()
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"`TrailingOffsetType.{TrailingOffsetTypeParser.to_str(trailing_offset_type)}` "
                f"not currently supported",
            )

        if order.side == OrderSide.BUY:
            return Price(last_f64 + offset, precision=self.last._mem.precision)
        elif order.side == OrderSide.SELL:
            return Price(last_f64 - offset, precision=self.last._mem.precision)

    cdef Price _calculate_new_trailing_price_bid_ask(
        self,
        Order order,
        TrailingOffsetType trailing_offset_type,
        double offset,
    ):
        cdef double ask_f64 = self.ask.as_f64_c()
        cdef double bid_f64 = self.bid.as_f64_c()

        if trailing_offset_type == TrailingOffsetType.DEFAULT or trailing_offset_type == TrailingOffsetType.PRICE:
            pass  # Offset already calculated
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            if order.side == OrderSide.BUY:
                offset = ask_f64 * (offset / 100) / 100
            elif order.side == OrderSide.SELL:
                offset = bid_f64 * (offset / 100) / 100
        elif trailing_offset_type == TrailingOffsetType.TICKS:
            offset *= self._instrument.price_increment.as_f64_c()
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"`TrailingOffsetType.{TrailingOffsetTypeParser.to_str(trailing_offset_type)}` "
                f"not currently supported",
            )

        if order.side == OrderSide.BUY:
            return Price(ask_f64 + offset, precision=self.ask._mem.precision)
        elif order.side == OrderSide.SELL:
            return Price(bid_f64 - offset, precision=self.bid._mem.precision)
