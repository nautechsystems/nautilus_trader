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

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.datetime cimport unix_nanos_to_iso8601
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.functions cimport contingency_type_to_str
from nautilus_trader.model.functions cimport liquidity_side_to_str
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.functions cimport order_type_to_str
from nautilus_trader.model.functions cimport time_in_force_to_str
from nautilus_trader.model.functions cimport trigger_type_from_str
from nautilus_trader.model.functions cimport trigger_type_to_str
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class StopMarketOrder(Order):
    """
    Represents a `Stop-Market` conditional order.

    A Stop-Market order is an instruction to submit a BUY (or SELL) market order
    if and when the specified stop trigger price is attained or penetrated.
    A Stop-Market order is not guaranteed a specific execution price and may execute
    significantly away from its stop price.

    A SELL Stop-Market order is always placed below the current market price,
    and is typically used to limit a loss or protect a profit on a long position.

    A BUY Stop-Market order is always placed above the current market price,
    and is typically used to limit a loss or protect a profit on a short position.

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
    init_id : UUID4
        The order initialization event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
        The order time in force.
    expire_time_ns : uint64_t, default 0 (no expiry)
        UNIX timestamp (nanoseconds) when the order will expire.
    reduce_only : bool, default False
        If the order carries the 'reduce-only' execution instruction.
    quote_quantity : bool, default False
        If the order quantity is denominated in the quote currency.
    emulation_trigger : TriggerType, default ``NO_TRIGGER``
        The type of market price trigger to use for local order emulation.
        - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
        - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
        Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
    trigger_instrument_id : InstrumentId, optional
        The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
    contingency_type : ContingencyType, default ``NO_CONTINGENCY``
        The order contingency type.
    order_list_id : OrderListId, optional
        The order list ID associated with the order.
    linked_order_ids : list[ClientOrderId], optional
        The order linked client order ID(s).
    parent_order_id : ClientOrderId, optional
        The order parent client order ID.
    exec_algorithm_id : ExecAlgorithmId, optional
        The execution algorithm ID for the order.
    exec_algorithm_params : dict[str, Any], optional
        The execution algorithm parameters for the order.
    exec_spawn_id : ClientOrderId, optional
        The execution algorithm spawning primary client order ID.
    tags : list[str], optional
        The custom user tags for the order.

    Raises
    ------
    ValueError
        If `order_side` is ``NO_ORDER_SIDE``.
    ValueError
        If `quantity` is not positive (> 0).
    ValueError
        If `trigger_type` is ``NO_TRIGGER``.
    ValueError
        If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
    ValueError
        If `time_in_force` is ``GTD`` and `expire_time_ns` <= UNIX epoch.

    References
    ----------
    https://www.interactivebrokers.com/en/trading/orders/stop.php
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
        UUID4 init_id not None,
        uint64_t ts_init,
        TimeInForce time_in_force = TimeInForce.GTC,
        uint64_t expire_time_ns = 0,
        bint reduce_only = False,
        bint quote_quantity = False,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        InstrumentId trigger_instrument_id = None,
        ContingencyType contingency_type = ContingencyType.NO_CONTINGENCY,
        OrderListId order_list_id = None,
        list linked_order_ids = None,
        ClientOrderId parent_order_id = None,
        ExecAlgorithmId exec_algorithm_id = None,
        dict exec_algorithm_params = None,
        ClientOrderId exec_spawn_id = None,
        list[str] tags = None,
    ):
        Condition.not_equal(order_side, OrderSide.NO_ORDER_SIDE, "order_side", "NO_ORDER_SIDE")
        Condition.not_equal(trigger_type, TriggerType.NO_TRIGGER, "trigger_type", "NO_TRIGGER")
        Condition.not_equal(time_in_force, TimeInForce.AT_THE_OPEN, "time_in_force", "AT_THE_OPEN`")
        Condition.not_equal(time_in_force, TimeInForce.AT_THE_CLOSE, "time_in_force", "AT_THE_CLOSE`")

        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.is_true(expire_time_ns > 0, "`expire_time_ns` cannot be <= UNIX epoch.")
        else:
            # Should not have an expire time
            Condition.is_true(expire_time_ns == 0, "`expire_time_ns` was set when `time_in_force` not GTD.")

        # Set options
        cdef dict options = {
            "trigger_price": str(trigger_price),
            "trigger_type": trigger_type_to_str(trigger_type),
            "expire_time_ns": expire_time_ns,
        }

        # Create initialization event
        cdef OrderInitialized init = OrderInitialized(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=order_side,
            order_type=OrderType.STOP_MARKET,
            quantity=quantity,
            time_in_force=time_in_force,
            post_only=False,
            reduce_only=reduce_only,
            quote_quantity=quote_quantity,
            options=options,
            emulation_trigger=emulation_trigger,
            trigger_instrument_id=trigger_instrument_id,
            contingency_type=contingency_type,
            order_list_id=order_list_id,
            linked_order_ids=linked_order_ids,
            parent_order_id=parent_order_id,
            exec_algorithm_id=exec_algorithm_id,
            exec_algorithm_params=exec_algorithm_params,
            exec_spawn_id=exec_spawn_id,
            tags=tags,
            event_id=init_id,
            ts_init=ts_init,
        )
        super().__init__(init=init)

        self.trigger_price = trigger_price
        self.trigger_type = trigger_type
        self.expire_time_ns = expire_time_ns

    cdef void _updated(self, OrderUpdated event):
        if self.venue_order_id is not None and event.venue_order_id is not None and self.venue_order_id != event.venue_order_id:
            self._venue_order_ids.append(self.venue_order_id)
            self.venue_order_id = event.venue_order_id
        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity.from_raw_c(self.quantity._mem.raw - self.filled_qty._mem.raw, self.quantity._mem.precision)
        if event.trigger_price is not None:
            self.trigger_price = event.trigger_price

    cdef void _set_slippage(self):
        if self.side == OrderSide.BUY:
            self.slippage = self.avg_px - self.trigger_price.as_f64_c()
        elif self.side == OrderSide.SELL:
            self.slippage = self.trigger_price.as_f64_c() - self.avg_px

    cdef bint has_price_c(self):
        return False

    cdef bint has_trigger_price_c(self):
        return True

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
        cdef str expiration_str = "" if self.expire_time_ns == 0 else f" {unix_nanos_to_iso8601(self.expire_time_ns, nanos_precision=False)}"
        cdef str emulation_str = "" if self.emulation_trigger == TriggerType.NO_TRIGGER else f" EMULATED[{trigger_type_to_str(self.emulation_trigger)}]"
        return (
            f"{order_side_to_str(self.side)} {self.quantity.to_formatted_str()} {self.instrument_id} "
            f"{order_type_to_str(self.order_type)} @ {self.trigger_price.to_formatted_str()}"
            f"[{trigger_type_to_str(self.trigger_type)}] "
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
            "venue_order_id": self.venue_order_id.to_str() if self.venue_order_id is not None else None,
            "position_id": self.position_id.to_str() if self.position_id is not None else None,
            "account_id": self.account_id.to_str() if self.account_id is not None else None,
            "last_trade_id": self.last_trade_id.to_str() if self.last_trade_id is not None else None,
            "type": order_type_to_str(self.order_type),
            "side": order_side_to_str(self.side),
            "quantity": str(self.quantity),
            "trigger_price": str(self.trigger_price),
            "trigger_type": trigger_type_to_str(self.trigger_type),
            "expire_time_ns": self.expire_time_ns if self.expire_time_ns > 0 else None,
            "time_in_force": time_in_force_to_str(self.time_in_force),
            "filled_qty": str(self.filled_qty),
            "liquidity_side": liquidity_side_to_str(self.liquidity_side),
            "avg_px": self.avg_px if self.filled_qty.as_f64_c() > 0.0 else None,
            "slippage": self.slippage if self.filled_qty.as_f64_c() > 0.0 else None,
            "commissions": [str(c) for c in self.commissions()] if self._commissions else None,
            "status": self._fsm.state_string_c(),
            "is_reduce_only": self.is_reduce_only,
            "is_quote_quantity": self.is_quote_quantity,
            "emulation_trigger": trigger_type_to_str(self.emulation_trigger),
            "trigger_instrument_id": self.trigger_instrument_id.to_str() if self.trigger_instrument_id is not None else None,
            "contingency_type": contingency_type_to_str(self.contingency_type),
            "order_list_id": self.order_list_id.to_str() if self.order_list_id is not None else None,
            "linked_order_ids": [o.to_str() for o in self.linked_order_ids] if self.linked_order_ids is not None else None,  # noqa
            "parent_order_id": self.parent_order_id.to_str() if self.parent_order_id is not None else None,
            "exec_algorithm_id": self.exec_algorithm_id.to_str() if self.exec_algorithm_id is not None else None,
            "exec_algorithm_params": self.exec_algorithm_params,
            "exec_spawn_id": self.exec_spawn_id.to_str() if self.exec_spawn_id is not None else None,
            "tags": self.tags,
            "init_id": str(self.init_id),
            "ts_init": self.ts_init,
            "ts_last": self.ts_last,
        }

    @staticmethod
    cdef StopMarketOrder create_c(OrderInitialized init):
        """
        Return a `Stop-Market` order from the given initialized event.

        Parameters
        ----------
        init : OrderInitialized
            The event to initialize with.

        Returns
        -------
        StopMarketOrder

        Raises
        ------
        ValueError
            If `init.order_type` is not equal to ``STOP_MARKET``.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.order_type, OrderType.STOP_MARKET, "init.order_type", "OrderType")
        return StopMarketOrder(
            trader_id=init.trader_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            client_order_id=init.client_order_id,
            order_side=init.side,
            quantity=init.quantity,
            trigger_price=Price.from_str_c(init.options["trigger_price"]),
            trigger_type=trigger_type_from_str(init.options["trigger_type"]),
            init_id=init.id,
            ts_init=init.ts_init,
            time_in_force=init.time_in_force,
            expire_time_ns=init.options["expire_time_ns"] if init.options["expire_time_ns"] is not None else 0,
            reduce_only=init.reduce_only,
            quote_quantity=init.quote_quantity,
            emulation_trigger=init.emulation_trigger,
            trigger_instrument_id=init.trigger_instrument_id,
            contingency_type=init.contingency_type,
            order_list_id=init.order_list_id,
            linked_order_ids=init.linked_order_ids,
            parent_order_id=init.parent_order_id,
            exec_algorithm_id=init.exec_algorithm_id,
            exec_algorithm_params=init.exec_algorithm_params,
            exec_spawn_id=init.exec_spawn_id,
            tags=init.tags,
        )

    @staticmethod
    def create(init):
        return StopMarketOrder.create_c(init)
