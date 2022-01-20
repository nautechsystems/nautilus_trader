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

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.datetime cimport maybe_unix_nanos_to_dt
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyType
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyTypeParser
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetTypeParser
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class TrailingStopMarketOrder(Order):
    """
    Represents a trailing stop-market trigger order.

    A sell trailing stop order sets the stop price at a fixed amount below the
    market price with an attached "trailing" offset. As the market price rises,
    the stop price rises by the trail amount, but if the stock price falls, the
    stop loss price doesn't change, and a market order is submitted when the
    stop price is hit. This technique is designed to allow a trader to
    specify a limit on the maximum possible loss, without setting a limit on
    the maximum possible gain. "Buy" trailing stop orders are the mirror image
    of sell trailing stop orders, and are most appropriate for use in falling
    markets.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the order.
    strategy_id : StrategyId
        The strategy ID associated with the order.
    instrument_id : InstrumentId
        The order instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    order_side : OrderSide {``BUY``, ``SELL``}
        The order side.
    quantity : Quantity
        The order quantity (> 0).
    trigger_price : Price
        The order trigger price (STOP).
    trigger_type : TriggerType
        The order trigger type.
    trailing_offset : Decimal
        The trailing offset for the trigger (STOP) price.
    offset_type : TrailingOffsetType
        The order trailing offset type.
    time_in_force : TimeInForce
        The order time-in-force.
    expiration : datetime, optional
        The order expiration.
    init_id : UUID4
        The order initialization event ID.
    ts_init : int64
        The UNIX timestamp (nanoseconds) when the object was initialized.
    reduce_only : bool, optional
        If the order carries the 'reduce-only' execution instruction.
    order_list_id : OrderListId, optional
        The order list ID associated with the order.
    parent_order_id : ClientOrderId, optional
        The order parent client order ID.
    child_order_ids : list[ClientOrderId], optional
        The order child client order ID(s).
    contingency : ContingencyType
        The order contingency type.
    contingency_ids : list[ClientOrderId], optional
        The order contingency client order ID(s).
    tags : str, optional
        The custom user tags for the order. These are optional and can
        contain any arbitrary delimiter if required.

    Raises
    ------
    ValueError
        If `quantity` is not positive (> 0).
    ValueError
        If `time_in_force` is ``GTD`` and `expiration` is ``None`` or <= UNIX epoch.
    """
    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price trigger_price not None,
        TriggerType trigger_type,
        trailing_offset: Decimal,
        TrailingOffsetType offset_type,
        TimeInForce time_in_force,
        datetime expiration,  # Can be None
        UUID4 init_id not None,
        int64_t ts_init,
        bint reduce_only=False,
        OrderListId order_list_id=None,
        ClientOrderId parent_order_id=None,
        list child_order_ids=None,
        ContingencyType contingency=ContingencyType.NONE,
        list contingency_ids=None,
        str tags=None,
    ):
        cdef int64_t expiration_ns = 0
        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.not_none(expiration, "expiration")
            expiration_ns = dt_to_unix_nanos(expiration)
            Condition.true(expiration_ns > 0, "expiration cannot be <= UNIX epoch.")
        else:
            # Should not have an expire time
            Condition.none(expiration, "expiration")

        # Set options
        cdef dict options = {
            "trigger_price": str(trigger_price),
            "trigger_type": TriggerTypeParser.to_str(trigger_type),
            "trailing_offset": str(trailing_offset),
            "offset_type": TrailingOffsetTypeParser.to_str(offset_type),
            "expiration_ns": expiration_ns if expiration_ns > 0 else None,
        }

        # Create initialization event
        cdef OrderInitialized init = OrderInitialized(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=order_side,
            order_type=OrderType.TRAILING_STOP_MARKET,
            quantity=quantity,
            time_in_force=time_in_force,
            post_only=False,
            reduce_only=reduce_only,
            options=options,
            order_list_id=order_list_id,
            parent_order_id=parent_order_id,
            child_order_ids=child_order_ids,
            contingency=contingency,
            contingency_ids=contingency_ids,
            tags=tags,
            event_id=init_id,
            ts_init=ts_init,
        )
        super().__init__(init=init)

        self.trigger_price = trigger_price
        self.trigger_type = trigger_type
        self.trailing_offset = trailing_offset
        self.offset_type = offset_type
        self.expiration = expiration
        self.expiration_ns = expiration_ns

    cpdef str info(self):
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        cdef str expiration_str = "" if self.expiration is None else f" {format_iso8601(self.expiration)}"
        return (
            f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
            f"{OrderTypeParser.to_str(self.type)} @ {self.trigger_price}"
            f"[{TriggerTypeParser.to_str(self.trigger_type)}] "
            f"{self.trailing_offset}-TRAILING_OFFSET[{TrailingOffsetTypeParser.to_str(self.offset_type)}] "
            f"{TimeInForceParser.to_str(self.time_in_force)}{expiration_str}"
        )

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "trader_id": self.trader_id.value,
            "strategy_id": self.strategy_id.value,
            "instrument_id": self.instrument_id.value,
            "client_order_id": self.client_order_id.value,
            "venue_order_id": self.venue_order_id if self.venue_order_id else None,
            "position_id": self.position_id.value if self.position_id else None,
            "account_id": self.account_id.value if self.account_id else None,
            "last_trade_id": self.last_trade_id.value if self.last_trade_id else None,
            "type": OrderTypeParser.to_str(self.type),
            "side": OrderSideParser.to_str(self.side),
            "quantity": str(self.quantity),
            "trigger_price": str(self.trigger_price),
            "trigger_type": TriggerTypeParser.to_str(self.trigger_type),
            "trailing_offset": str(self.trailing_offset),
            "offset_type": TrailingOffsetTypeParser.to_str(self.offset_type),
            "expiration_ns": self.expiration_ns if self.expiration_ns > 0 else None,
            "time_in_force": TimeInForceParser.to_str(self.time_in_force),
            "filled_qty": str(self.filled_qty),
            "liquidity_side": LiquiditySideParser.to_str(self.liquidity_side),
            "avg_px": str(self.avg_px) if self.avg_px else None,
            "slippage": str(self.slippage),
            "status": self._fsm.state_string_c(),
            "is_reduce_only": self.is_reduce_only,
            "order_list_id": self.order_list_id,
            "parent_order_id": self.parent_order_id,
            "child_order_ids": ",".join([o.value for o in self.child_order_ids]) if self.child_order_ids is not None else None,  # noqa
            "contingency": ContingencyTypeParser.to_str(self.contingency),
            "contingency_ids": ",".join([o.value for o in self.contingency_ids]) if self.contingency_ids is not None else None,  # noqa
            "tags": self.tags,
            "ts_last": self.ts_last,
            "ts_init": self.ts_init,
        }

    @staticmethod
    cdef TrailingStopMarketOrder create(OrderInitialized init):
        """
        Return a stop-market order from the given initialized event.

        Parameters
        ----------
        init : OrderInitialized
            The event to initialize with.

        Returns
        -------
        TrailingStopMarketOrder

        Raises
        ------
        ValueError
            If `init.type` is not equal to ``STOP_MARKET``.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.type, OrderType.TRAILING_STOP_MARKET, "init.type", "OrderType")

        return TrailingStopMarketOrder(
            trader_id=init.trader_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            client_order_id=init.client_order_id,
            order_side=init.side,
            quantity=init.quantity,
            trigger_price=Price.from_str_c(init.options["trigger_price"]),
            trigger_type=TriggerTypeParser.from_str(init.options["trigger_type"]),
            trailing_offset=Decimal(init.options["trailing_offset"]),
            offset_type=TrailingOffsetTypeParser.from_str(init.options["offset_type"]),
            time_in_force=init.time_in_force,
            expiration=maybe_unix_nanos_to_dt(init.options["expiration_ns"]),
            init_id=init.id,
            ts_init=init.ts_init,
            reduce_only=init.reduce_only,
            order_list_id=init.order_list_id,
            parent_order_id=init.parent_order_id,
            child_order_ids=init.child_order_ids,
            contingency=init.contingency,
            contingency_ids=init.contingency_ids,
            tags=init.tags,
        )

    cdef void _updated(self, OrderUpdated event) except *:
        if self.venue_order_id != event.venue_order_id:
            self._venue_order_ids.append(self.venue_order_id)
            self.venue_order_id = event.venue_order_id
        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity(self.quantity - self.filled_qty, self.quantity.precision)
        if event.trigger_price is not None:
            self.trigger_price = event.trigger_price

    cdef void _set_slippage(self) except *:
        if self.side == OrderSide.BUY:
            self.slippage = self.avg_px - self.trigger_price
        elif self.side == OrderSide.SELL:
            self.slippage = self.trigger_price - self.avg_px
