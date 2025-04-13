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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport TrailingOffsetType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.model.functions cimport trailing_offset_type_to_str
from nautilus_trader.model.functions cimport trigger_type_to_str
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class TrailingStopCalculator:
    """
    Provides trailing stop calculation functionality
    """

    @staticmethod
    cdef tuple calculate(
        Price price_increment,
        Order order,
        Price bid,
        Price ask,
        Price last,
    ):
        Condition.not_none(price_increment, "price_increment")
        if order.order_type not in (OrderType.TRAILING_STOP_MARKET, OrderType.TRAILING_STOP_LIMIT):
            raise TypeError(f"invalid `OrderType` for calculation, was {order.type_string_c()}")  # pragma: no cover (design-time error)

        cdef Price trigger_price = order.trigger_price
        cdef Price price = None
        cdef Price new_trigger_price = None
        cdef Price new_price = None

        if order.order_type == OrderType.TRAILING_STOP_LIMIT:
            price = order.price

        cdef:
            Price temp_trigger_price
            Price temp_price
        if (
            order.trigger_type == TriggerType.DEFAULT
            or order.trigger_type == TriggerType.LAST_PRICE
            or order.trigger_type == TriggerType.MARK_PRICE
        ):
            if last is None:
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(add trades or use bars)",
                )
            if order.side == OrderSide.BUY:
                temp_trigger_price = TrailingStopCalculator.calculate_with_last(
                    price_increment=price_increment,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_last(
                        price_increment=price_increment,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = TrailingStopCalculator.calculate_with_last(
                    price_increment=price_increment,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_last(
                        price_increment=price_increment,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        elif order.trigger_type == TriggerType.BID_ASK:
            if bid is None:
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"cannot process trailing stop, "
                    f"no BID price for {order.instrument_id} "
                    f"(add quotes or use bars)",
                )
            if ask is None:
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"cannot process trailing stop, "
                    f"no ASK price for {order.instrument_id} "
                    f"(add quotes or use bars)",
                )

            if order.side == OrderSide.BUY:
                temp_trigger_price = TrailingStopCalculator.calculate_with_bid_ask(
                    price_increment=price_increment,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    bid=bid,
                    ask=ask,
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_bid_ask(
                        price_increment=price_increment,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = TrailingStopCalculator.calculate_with_bid_ask(
                    price_increment=price_increment,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    bid=bid,
                    ask=ask,
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_bid_ask(
                        price_increment=price_increment,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        elif order.trigger_type == TriggerType.LAST_OR_BID_ASK:
            if last is None:
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(add trades or use bars)",
                )
            if bid is None:
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"cannot process trailing stop, "
                    f"no BID price for {order.instrument_id} "
                    f"(add quotes or use bars)",
                )
            if ask is None:
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"cannot process trailing stop, "
                    f"no ASK price for {order.instrument_id} "
                    f"(add quotes or use bars)",
                )

            if order.side == OrderSide.BUY:
                temp_trigger_price = TrailingStopCalculator.calculate_with_last(
                    price_increment=price_increment,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                    trigger_price = new_trigger_price  # Set trigger to new trigger
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_last(
                        price_increment=price_increment,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
                        price = new_price  # Set price to new price

                temp_trigger_price = TrailingStopCalculator.calculate_with_bid_ask(
                    price_increment=price_increment,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    bid=bid,
                    ask=ask,
                )
                if trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_bid_ask(
                        price_increment=price_increment,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = TrailingStopCalculator.calculate_with_last(
                    price_increment=price_increment,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                    trigger_price = new_trigger_price  # Set trigger to new trigger
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_last(
                        price_increment=price_increment,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
                        price = new_price  # Set price to new price

                temp_trigger_price = TrailingStopCalculator.calculate_with_bid_ask(
                    price_increment=price_increment,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    bid=bid,
                    ask=ask,
                )
                if trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_bid_ask(
                        price_increment=price_increment,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"cannot process trailing stop, "
                f"`TriggerType.{trigger_type_to_str(order.trigger_type)}` "
                f"not currently supported",
            )

        return new_trigger_price, new_price

    @staticmethod
    cdef Price calculate_with_last(
        Price price_increment,
        TrailingOffsetType trailing_offset_type,
        OrderSide side,
        double offset,
        Price last,
    ):
        cdef double last_f64 = last.as_f64_c()

        if trailing_offset_type == TrailingOffsetType.PRICE:
            pass  # Offset already calculated
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            offset = last_f64 * (offset / 100) / 100
        elif trailing_offset_type == TrailingOffsetType.TICKS:
            offset *= price_increment.as_f64_c()
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"cannot process trailing stop, "
                f"`TrailingOffsetType` {trailing_offset_type_to_str(trailing_offset_type)} "
                f"not currently supported",
            )

        if side == OrderSide.BUY:
            return Price(last_f64 + offset, precision=price_increment.precision)
        elif side == OrderSide.SELL:
            return Price(last_f64 - offset, precision=price_increment.precision)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    @staticmethod
    cdef Price calculate_with_bid_ask(
        Price price_increment,
        TrailingOffsetType trailing_offset_type,
        OrderSide side,
        double offset,
        Price bid,
        Price ask,
    ):
        cdef double ask_f64 = ask.as_f64_c()
        cdef double bid_f64 = bid.as_f64_c()

        if trailing_offset_type == TrailingOffsetType.PRICE:
            pass  # Offset already calculated
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            if side == OrderSide.BUY:
                offset = ask_f64 * (offset / 100) / 100
            elif side == OrderSide.SELL:
                offset = bid_f64 * (offset / 100) / 100
        elif trailing_offset_type == TrailingOffsetType.TICKS:
            offset *= price_increment.as_f64_c()
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"cannot process trailing stop, "  # pragma: no cover (design-time error)
                f"`TrailingOffsetType` {trailing_offset_type_to_str(trailing_offset_type)} "  # pragma: no cover (design-time error)  # noqa
                f"not currently supported",  # pragma: no cover (design-time error)
            )

        if side == OrderSide.BUY:
            return Price(ask_f64 + offset, precision=price_increment.precision)
        elif side == OrderSide.SELL:
            return Price(bid_f64 - offset, precision=price_increment.precision)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)
