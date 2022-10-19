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

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
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
    Represents a `Trailing-Stop-Market` conditional order.

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
    trigger_price : Price, optional with no default so ``None`` must be passed explicitly
        The order trigger price (STOP). If ``None`` then will typically default
        to the delta of market price and `trailing_offset`.
    trigger_type : TriggerType
        The order trigger type.
    trailing_offset : Decimal
        The trailing offset for the trigger price (STOP).
    trailing_offset_type : TrailingOffsetType
        The order trailing offset type.
    init_id : UUID4
        The order initialization event ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
        The order time in force.
    expire_time_ns : uint64_t, default 0 (no expiry)
        The UNIX timestamp (nanoseconds) when the order will expire.
    reduce_only : bool, default False
        If the order carries the 'reduce-only' execution instruction.
    emulation_trigger : TriggerType, default ``NONE``
        The order emulation trigger.
    contingency_type : ContingencyType, default ``NONE``
        The order contingency type.
    order_list_id : OrderListId, optional
        The order list ID associated with the order.
    linked_order_ids : list[ClientOrderId], optional
        The order linked client order ID(s).
    parent_order_id : ClientOrderId, optional
        The order parent client order ID.
    tags : str, optional
        The custom user tags for the order. These are optional and can
        contain any arbitrary delimiter if required.

    Raises
    ------
    ValueError
        If `order_side` is ``NONE``.
    ValueError
        If `quantity` is not positive (> 0).
    ValueError
        If `trigger_type` is ``NONE``.
    ValueError
        If `trailing_offset_type` is ``NONE``.
    ValueError
        If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
    ValueError
        If `time_in_force` is ``GTD`` and `expire_time_ns` <= UNIX epoch.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price trigger_price: Optional[Price],
        TriggerType trigger_type,
        trailing_offset: Decimal,
        TrailingOffsetType trailing_offset_type,
        UUID4 init_id not None,
        uint64_t ts_init,
        TimeInForce time_in_force = TimeInForce.GTC,
        uint64_t expire_time_ns = 0,
        bint reduce_only = False,
        TriggerType emulation_trigger = TriggerType.NONE,
        ContingencyType contingency_type = ContingencyType.NONE,
        OrderListId order_list_id = None,
        list linked_order_ids = None,
        ClientOrderId parent_order_id = None,
        str tags = None,
    ):
        Condition.not_equal(order_side, OrderSide.NONE, "order_side", "NONE")
        Condition.not_equal(trigger_type, TriggerType.NONE, "trigger_type", "NONE")
        Condition.not_equal(trailing_offset_type, TrailingOffsetType.NONE, "trailing_offset_type", "NONE")
        Condition.not_equal(time_in_force, TimeInForce.AT_THE_OPEN, "time_in_force", "AT_THE_OPEN`")
        Condition.not_equal(time_in_force, TimeInForce.AT_THE_CLOSE, "time_in_force", "AT_THE_CLOSE`")

        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.true(expire_time_ns > 0, "`expire_time_ns` cannot be <= UNIX epoch.")
        else:
            # Should not have an expire time
            Condition.true(expire_time_ns == 0, "`expire_time_ns` was set when `time_in_force` not GTD.")

        # Set options
        cdef dict options = {
            "trigger_price": str(trigger_price) if trigger_price is not None else None,
            "trigger_type": TriggerTypeParser.to_str(trigger_type),
            "trailing_offset": str(trailing_offset),
            "trailing_offset_type": TrailingOffsetTypeParser.to_str(trailing_offset_type),
            "expire_time_ns": expire_time_ns,
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
            emulation_trigger=emulation_trigger,
            contingency_type=contingency_type,
            order_list_id=order_list_id,
            linked_order_ids=linked_order_ids,
            parent_order_id=parent_order_id,
            tags=tags,
            event_id=init_id,
            ts_init=ts_init,
        )
        super().__init__(init=init)

        self.trigger_price = trigger_price
        self.trigger_type = trigger_type
        self.trailing_offset = trailing_offset
        self.trailing_offset_type = trailing_offset_type
        self.expire_time_ns = expire_time_ns

    cdef bint has_price_c(self) except *:
        return False

    cdef bint has_trigger_price_c(self) except *:
        return self.trigger_price is not None

    @property
    def expire_time(self):
        """
        Return the expire time for the order (UTC).

        Returns
        -------
        datetime or ``None``

        """
        return None if self.expire_time_ns == 0 else unix_nanos_to_dt(self.expire_time_ns)

    cpdef str info(self):
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        cdef str expiration_str = "" if self.expire_time_ns == 0 else f" {format_iso8601(unix_nanos_to_dt(self.expire_time_ns))}"
        cdef str emulation_str = "" if self.emulation_trigger == TriggerType.NONE else f" EMULATED[{TriggerTypeParser.to_str(self.emulation_trigger)}]"
        return (
            f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
            f"{OrderTypeParser.to_str(self.order_type)}[{TriggerTypeParser.to_str(self.trigger_type)}] "
            f"{'@ ' + str(self.trigger_price) + '-STOP ' if self.trigger_price else ''}"
            f"{self.trailing_offset}-TRAILING_OFFSET[{TrailingOffsetTypeParser.to_str(self.trailing_offset_type)}] "
            f"{TimeInForceParser.to_str(self.time_in_force)}{expiration_str}"
            f"{emulation_str}"
        )

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        cdef ClientOrderId o
        return {
            "trader_id": self.trader_id.to_str(),
            "strategy_id": self.strategy_id.to_str(),
            "instrument_id": self.instrument_id.to_str(),
            "client_order_id": self.client_order_id.to_str(),
            "venue_order_id": self.venue_order_id if self.venue_order_id else None,
            "position_id": self.position_id.to_str() if self.position_id else None,
            "account_id": self.account_id.to_str() if self.account_id else None,
            "last_trade_id": self.last_trade_id.to_str() if self.last_trade_id else None,
            "type": OrderTypeParser.to_str(self.order_type),
            "side": OrderSideParser.to_str(self.side),
            "quantity": str(self.quantity),
            "trigger_price": str(self.trigger_price) if self.trigger_price is not None else None,
            "trigger_type": TriggerTypeParser.to_str(self.trigger_type),
            "trailing_offset": str(self.trailing_offset),
            "trailing_offset_type": TrailingOffsetTypeParser.to_str(self.trailing_offset_type),
            "expire_time_ns": self.expire_time_ns,
            "time_in_force": TimeInForceParser.to_str(self.time_in_force),
            "filled_qty": str(self.filled_qty),
            "liquidity_side": LiquiditySideParser.to_str(self.liquidity_side),
            "avg_px": str(self.avg_px),
            "slippage": str(self.slippage),
            "status": self._fsm.state_string_c(),
            "is_reduce_only": self.is_reduce_only,
            "emulation_trigger": TriggerTypeParser.to_str(self.emulation_trigger),
            "contingency_type": ContingencyTypeParser.to_str(self.contingency_type),
            "order_list_id": self.order_list_id.to_str() if self.order_list_id is not None else None,
            "linked_order_ids": ",".join([o.to_str() for o in self.linked_order_ids]) if self.linked_order_ids is not None else None,  # noqa
            "parent_order_id": self.parent_order_id.to_str() if self.parent_order_id is not None else None,
            "tags": self.tags,
            "ts_last": self.ts_last,
            "ts_init": self.ts_init,
        }

    @staticmethod
    cdef TrailingStopMarketOrder create(OrderInitialized init):
        """
        Return a `Trailing-Stop-Market` order from the given initialized event.

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
            If `init.order_type` is not equal to ``TRAILING_STOP_MARKET``.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.order_type, OrderType.TRAILING_STOP_MARKET, "init.order_type", "OrderType")

        cdef str trigger_price_str = init.options.get("trigger_price")

        return TrailingStopMarketOrder(
            trader_id=init.trader_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            client_order_id=init.client_order_id,
            order_side=init.side,
            quantity=init.quantity,
            trigger_price=Price.from_str_c(trigger_price_str) if trigger_price_str is not None else None,
            trigger_type=TriggerTypeParser.from_str(init.options["trigger_type"]),
            trailing_offset=Decimal(init.options["trailing_offset"]),
            trailing_offset_type=TrailingOffsetTypeParser.from_str(init.options["trailing_offset_type"]),
            time_in_force=init.time_in_force,
            expire_time_ns=init.options["expire_time_ns"],
            init_id=init.id,
            ts_init=init.ts_init,
            reduce_only=init.reduce_only,
            emulation_trigger=init.emulation_trigger,
            contingency_type=init.contingency_type,
            order_list_id=init.order_list_id,
            linked_order_ids=init.linked_order_ids,
            parent_order_id=init.parent_order_id,
            tags=init.tags,
        )

    cdef void _updated(self, OrderUpdated event) except *:
        if self.venue_order_id != event.venue_order_id:
            self._venue_order_ids.append(self.venue_order_id)
            self.venue_order_id = event.venue_order_id
        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity.from_raw_c(self.quantity._mem.raw - self.filled_qty._mem.raw, self.quantity._mem.precision)
        if event.trigger_price is not None:
            self.trigger_price = event.trigger_price

    cdef void _set_slippage(self) except *:
        if self.side == OrderSide.BUY:
            self.slippage = self.avg_px - self.trigger_price.as_f64_c()
        elif self.side == OrderSide.SELL:
            self.slippage = self.trigger_price.as_f64_c() - self.avg_px
