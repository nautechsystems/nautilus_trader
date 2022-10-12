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

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetType
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetTypeParser
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class TrailingStopCalculator:
    """
    Provides trailing stop calculation functionality
    """

    @staticmethod
    cdef tuple calculate(
        Instrument instrument,
        Order order,
        Price bid,
        Price ask,
        Price last,
    ):
        Condition.not_none(instrument, "instrument")
        if order.order_type not in (OrderType.TRAILING_STOP_MARKET, OrderType.TRAILING_STOP_LIMIT):
            raise TypeError(f"invalid `OrderType` for calculation, was {order.type_string_c()}")

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
            if last is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(please add trade ticks or use bars)",
                )
            if order.side == OrderSide.BUY:
                temp_trigger_price = TrailingStopCalculator.calculate_with_last(
                    instrument=instrument,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_last(
                        instrument=instrument,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = TrailingStopCalculator.calculate_with_last(
                    instrument=instrument,
                    trailing_offset_type=order.trailing_offset_type,
                    side=order.side,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.order_type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = TrailingStopCalculator.calculate_with_last(
                        instrument=instrument,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        elif order.trigger_type == TriggerType.BID_ASK:
            if bid is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no BID price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )
            if ask is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no ASK price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )

            if order.side == OrderSide.BUY:
                temp_trigger_price = TrailingStopCalculator.calculate_with_bid_ask(
                    instrument=instrument,
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
                        instrument=instrument,
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
                    instrument=instrument,
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
                        instrument=instrument,
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
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(please add trade ticks or use bars)",
                )
            if bid is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no BID price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )
            if ask is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no ASK price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )

            if order.side == OrderSide.BUY:
                temp_trigger_price = TrailingStopCalculator.calculate_with_last(
                    instrument=instrument,
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
                        instrument=instrument,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
                        price = new_price  # Set price to new price

                temp_trigger_price = TrailingStopCalculator.calculate_with_bid_ask(
                    instrument=instrument,
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
                        instrument=instrument,
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
                    instrument=instrument,
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
                        instrument=instrument,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
                        price = new_price  # Set price to new price

                temp_trigger_price = TrailingStopCalculator.calculate_with_bid_ask(
                    instrument=instrument,
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
                        instrument=instrument,
                        trailing_offset_type=order.trailing_offset_type,
                        side=order.side,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"`TriggerType.{TriggerTypeParser.to_str(order.trigger_type)}` "
                f"not currently supported",
            )

        return new_trigger_price, new_price

    @staticmethod
    cdef Price calculate_with_last(
        Instrument instrument,
        TrailingOffsetType trailing_offset_type,
        OrderSide side,
        double offset,
        Price last,
    ):
        cdef double last_f64 = last.as_f64_c()

        if trailing_offset_type == TrailingOffsetType.DEFAULT or trailing_offset_type == TrailingOffsetType.PRICE:
            pass  # Offset already calculated
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            offset = last_f64 * (offset / 100) / 100
        elif trailing_offset_type == TrailingOffsetType.TICKS:
            offset *= instrument.price_increment.as_f64_c()
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"`TrailingOffsetType` {TrailingOffsetTypeParser.to_str(trailing_offset_type)} "
                f"not currently supported",
            )

        if side == OrderSide.BUY:
            return Price(last_f64 + offset, precision=instrument.price_precision)
        elif side == OrderSide.SELL:
            return Price(last_f64 - offset, precision=instrument.price_precision)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    @staticmethod
    cdef Price calculate_with_bid_ask(
        Instrument instrument,
        TrailingOffsetType trailing_offset_type,
        OrderSide side,
        double offset,
        Price bid,
        Price ask,
    ):
        cdef double ask_f64 = ask.as_f64_c()
        cdef double bid_f64 = bid.as_f64_c()

        if trailing_offset_type == TrailingOffsetType.DEFAULT or trailing_offset_type == TrailingOffsetType.PRICE:
            pass  # Offset already calculated
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            if side == OrderSide.BUY:
                offset = ask_f64 * (offset / 100) / 100
            elif side == OrderSide.SELL:
                offset = bid_f64 * (offset / 100) / 100
        elif trailing_offset_type == TrailingOffsetType.TICKS:
            offset *= instrument.price_increment.as_f64_c()
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"cannot process trailing stop, "
                f"`TrailingOffsetType` {TrailingOffsetTypeParser.to_str(trailing_offset_type)} "
                f"not currently supported",
            )

        if side == OrderSide.BUY:
            return Price(ask_f64 + offset, precision=instrument.price_precision)
        elif side == OrderSide.SELL:
            return Price(bid_f64 - offset, precision=instrument.price_precision)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)
