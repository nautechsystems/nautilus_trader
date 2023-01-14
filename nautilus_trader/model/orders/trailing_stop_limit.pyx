# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.enums_c cimport contingency_type_to_str
from nautilus_trader.model.enums_c cimport liquidity_side_to_str
from nautilus_trader.model.enums_c cimport order_side_to_str
from nautilus_trader.model.enums_c cimport order_type_to_str
from nautilus_trader.model.enums_c cimport time_in_force_to_str
from nautilus_trader.model.enums_c cimport trailing_offset_type_from_str
from nautilus_trader.model.enums_c cimport trailing_offset_type_to_str
from nautilus_trader.model.enums_c cimport trigger_type_from_str
from nautilus_trader.model.enums_c cimport trigger_type_to_str
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class TrailingStopLimitOrder(Order):
    """
    Represents a `Trailing-Stop-Limit` conditional order.

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
    price : Price, optional with no default so ``None`` must be passed explicitly
        The order price (LIMIT). If ``None`` then will typically default to the
        delta of market price and `limit_offset`.
    trigger_price : Price, optional with no default so ``None`` must be passed explicitly
        The order trigger price (STOP). If ``None`` then will typically default
        to the delta of market price and `trailing_offset`.
    trigger_type : TriggerType
        The order trigger type.
    limit_offset : Decimal
        The trailing offset for the order price (LIMIT).
    trailing_offset : Decimal
        The trailing offset for the order trigger price (STOP).
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
    post_only : bool, default False
        If the ``LIMIT`` order will only provide liquidity (once triggered).
    reduce_only : bool, default False
        If the ``LIMIT`` order carries the 'reduce-only' execution instruction.
    display_qty : Quantity, optional
        The quantity of the ``LIMIT`` order to display on the public book (iceberg).
    emulation_trigger : TriggerType, default ``NO_TRIGGER``
        The order emulation trigger.
    contingency_type : ContingencyType, default ``NO_CONTINGENCY``
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
        If `order_side` is ``NO_ORDER_SIDE``.
    ValueError
        If `quantity` is not positive (> 0).
    ValueError
        If `trigger_type` is ``NO_TRIGGER``.
    ValueError
        If `trailing_offset_type` is ``NO_TRAILING_OFFSET``.
    ValueError
        If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
    ValueError
        If `time_in_force` is ``GTD`` and `expire_time_ns` <= UNIX epoch.
    ValueError
        If `display_qty` is negative (< 0) or greater than `quantity`.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price price: Optional[Price],
        Price trigger_price: Optional[Price],
        TriggerType trigger_type,
        limit_offset: Decimal,
        trailing_offset: Decimal,
        TrailingOffsetType trailing_offset_type,
        UUID4 init_id not None,
        uint64_t ts_init,
        TimeInForce time_in_force = TimeInForce.GTC,
        uint64_t expire_time_ns = 0,
        bint post_only = False,
        bint reduce_only = False,
        Quantity display_qty = None,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        ContingencyType contingency_type = ContingencyType.NO_CONTINGENCY,
        OrderListId order_list_id = None,
        list linked_order_ids = None,
        ClientOrderId parent_order_id = None,
        str tags = None,
    ):
        Condition.not_equal(order_side, OrderSide.NO_ORDER_SIDE, "order_side", "NO_ORDER_SIDE")
        Condition.not_equal(trigger_type, TriggerType.NO_TRIGGER, "trigger_type", "NO_TRIGGER")
        Condition.not_equal(trailing_offset_type, TrailingOffsetType.NO_TRAILING_OFFSET, "trailing_offset_type", "NO_TRAILING_OFFSET")
        Condition.not_equal(time_in_force, TimeInForce.AT_THE_OPEN, "time_in_force", "AT_THE_OPEN`")
        Condition.not_equal(time_in_force, TimeInForce.AT_THE_CLOSE, "time_in_force", "AT_THE_CLOSE`")

        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.true(expire_time_ns > 0, "`expire_time_ns` cannot be <= UNIX epoch.")
        else:
            # Should not have an expire time
            Condition.true(expire_time_ns == 0, "`expire_time_ns` was set when `time_in_force` not GTD.")
        Condition.true(
            display_qty is None or 0 <= display_qty <= quantity,
            fail_msg="`display_qty` was negative or greater than order quantity",
        )

        # Set options
        cdef dict options = {
            "price": str(price) if price is not None else None,
            "trigger_price": str(trigger_price) if trigger_price is not None else None,
            "trigger_type": trigger_type_to_str(trigger_type),
            "limit_offset": str(limit_offset),
            "trailing_offset": str(trailing_offset),
            "trailing_offset_type": trailing_offset_type_to_str(trailing_offset_type),
            "expire_time_ns": expire_time_ns,
            "display_qty": str(display_qty) if display_qty is not None else None,
        }

        # Create initialization event
        cdef OrderInitialized init = OrderInitialized(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=order_side,
            order_type=OrderType.TRAILING_STOP_LIMIT,
            quantity=quantity,
            time_in_force=time_in_force,
            post_only=post_only,
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

        self.price = price
        self.trigger_price = trigger_price
        self.trigger_type = trigger_type
        self.limit_offset = limit_offset
        self.trailing_offset = trailing_offset
        self.trailing_offset_type = trailing_offset_type
        self.expire_time_ns = expire_time_ns
        self.display_qty = display_qty
        self.is_triggered = False
        self.ts_triggered = 0

    cdef void _updated(self, OrderUpdated event) except *:
        if self.venue_order_id is not None and event.venue_order_id is not None and self.venue_order_id != event.venue_order_id:
            self._venue_order_ids.append(self.venue_order_id)
            self.venue_order_id = event.venue_order_id
        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity.from_raw_c(self.quantity._mem.raw - self.filled_qty._mem.raw, self.quantity._mem.precision)
        if event.price is not None:
            self.price = event.price
        if event.trigger_price is not None:
            self.trigger_price = event.trigger_price

    cdef void _triggered(self, OrderTriggered event) except *:
        self.is_triggered = True
        self.ts_triggered = event.ts_event

    cdef void _set_slippage(self) except *:
        if self.side == OrderSide.BUY:
            self.slippage = self.avg_px - self.price.as_f64_c()
        elif self.side == OrderSide.SELL:
            self.slippage = self.price.as_f64_c() - self.avg_px

    cdef bint has_price_c(self) except *:
        return self.price is not None

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
        cdef str emulation_str = "" if self.emulation_trigger == TriggerType.NO_TRIGGER else f" EMULATED[{trigger_type_to_str(self.emulation_trigger)}]"
        return (
            f"{order_side_to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
            f"{order_type_to_str(self.order_type)}[{trigger_type_to_str(self.trigger_type)}] "
            f"{'@ ' + str(self.trigger_price) + '-STOP ' if self.trigger_price else ''}"
            f"[{trigger_type_to_str(self.trigger_type)}] {self.price}-LIMIT "
            f"{self.trailing_offset}-TRAILING_OFFSET[{trailing_offset_type_to_str(self.trailing_offset_type)}] "
            f"{self.limit_offset}-LIMIT_OFFSET[{trailing_offset_type_to_str(self.trailing_offset_type)}] "
            f"{time_in_force_to_str(self.time_in_force)}{expiration_str}"
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
            "venue_order_id": self.venue_order_id.to_str() if self.venue_order_id else None,
            "position_id": self.position_id.to_str() if self.position_id else None,
            "account_id": self.account_id.to_str() if self.account_id else None,
            "last_trade_id": self.last_trade_id.to_str() if self.last_trade_id else None,
            "type": order_type_to_str(self.order_type),
            "side": order_side_to_str(self.side),
            "quantity": str(self.quantity),
            "price": str(self.price) if self.price is not None else None,
            "trigger_price": str(self.trigger_price) if self.trigger_price is not None else None,
            "trigger_type": trigger_type_to_str(self.trigger_type),
            "limit_offset": str(self.limit_offset),
            "trailing_offset": str(self.trailing_offset),
            "trailing_offset_type": trailing_offset_type_to_str(self.trailing_offset_type),
            "expire_time_ns": self.expire_time_ns,
            "time_in_force": time_in_force_to_str(self.time_in_force),
            "filled_qty": str(self.filled_qty),
            "liquidity_side": liquidity_side_to_str(self.liquidity_side),
            "avg_px": str(self.avg_px),
            "slippage": str(self.slippage),
            "status": self._fsm.state_string_c(),
            "is_post_only": self.is_post_only,
            "is_reduce_only": self.is_reduce_only,
            "display_qty": str(self.display_qty) if self.display_qty is not None else None,
            "emulation_trigger": trigger_type_to_str(self.emulation_trigger),
            "contingency_type": contingency_type_to_str(self.contingency_type),
            "order_list_id": self.order_list_id.to_str() if self.order_list_id is not None else None,
            "linked_order_ids": ",".join([o.to_str() for o in self.linked_order_ids]) if self.linked_order_ids is not None else None,  # noqa
            "parent_order_id": self.parent_order_id.to_str() if self.parent_order_id is not None else None,
            "tags": self.tags,
            "ts_last": self.ts_last,
            "ts_init": self.ts_init,
        }

    @staticmethod
    cdef TrailingStopLimitOrder create(OrderInitialized init):
        """
        Return a `Trailing-Stop-Limit` order from the given initialized event.

        Parameters
        ----------
        init : OrderInitialized
            The event to initialize with.

        Returns
        -------
        TrailingStopLimitOrder

        Raises
        ------
        ValueError
            If `init.order_type` is not equal to ``TRAILING_STOP_LIMIT``.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.order_type, OrderType.TRAILING_STOP_LIMIT, "init.order_type", "OrderType")

        cdef str price_str = init.options.get("price")
        cdef str trigger_price_str = init.options.get("trigger_price")
        cdef str display_qty_str = init.options.get("display_qty")

        return TrailingStopLimitOrder(
            trader_id=init.trader_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            client_order_id=init.client_order_id,
            order_side=init.side,
            quantity=init.quantity,
            price=Price.from_str_c(price_str) if price_str is not None else None,
            trigger_price=Price.from_str_c(trigger_price_str) if trigger_price_str is not None else None,
            trigger_type=trigger_type_from_str(init.options["trigger_type"]),
            limit_offset=Decimal(init.options["limit_offset"]),
            trailing_offset=Decimal(init.options["trailing_offset"]),
            trailing_offset_type=trailing_offset_type_from_str(init.options["trailing_offset_type"]),
            time_in_force=init.time_in_force,
            expire_time_ns=init.options["expire_time_ns"],
            init_id=init.id,
            ts_init=init.ts_init,
            post_only=init.post_only,
            reduce_only=init.reduce_only,
            display_qty=Quantity.from_str_c(display_qty_str) if display_qty_str is not None else None,
            emulation_trigger=init.emulation_trigger,
            contingency_type=init.contingency_type,
            order_list_id=init.order_list_id,
            linked_order_ids=init.linked_order_ids,
            parent_order_id=init.parent_order_id,
            tags=init.tags,
        )
