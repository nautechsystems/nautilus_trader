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
from nautilus_trader.model.functions cimport contingency_type_from_str
from nautilus_trader.model.functions cimport contingency_type_to_str
from nautilus_trader.model.functions cimport liquidity_side_to_str
from nautilus_trader.model.functions cimport order_side_from_str
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.functions cimport order_type_to_str
from nautilus_trader.model.functions cimport time_in_force_from_str
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


cdef class LimitOrder(Order):
    """
    Represents a `Limit` order.

    A Limit order is an order to BUY (or SELL) at a specified price or better.
    The Limit order ensures that if the order fills, it will not fill at a price
    less favorable than your limit price, but it does not guarantee a fill.

    - A `Limit-On-Open (LOO)` order can be represented using a time in force of ``AT_THE_OPEN``.
    - A `Limit-On-Close (LOC)` order can be represented using a time in force of ``AT_THE_CLOSE``.

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
    price : Price
        The order limit price.
    init_id : UUID4
        The order initialization event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
        The order time in force.
    expire_time_ns : uint64_t, default 0 (no expiry)
        UNIX timestamp (nanoseconds) when the order will expire.
    post_only : bool, default False
        If the order will only provide liquidity (make a market).
    reduce_only : bool, default False
        If the order carries the 'reduce-only' execution instruction.
    quote_quantity : bool, default False
        If the order quantity is denominated in the quote currency.
    display_qty : Quantity, optional
        The quantity of the order to display on the public book (iceberg).
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
        If `time_in_force` is ``GTD`` and `expire_time_ns` <= UNIX epoch.
    ValueError
        If `display_qty` is negative (< 0) or greater than `quantity`.

    References
    ----------
    https://www.interactivebrokers.com/en/trading/orders/limit.php
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price price not None,
        UUID4 init_id not None,
        uint64_t ts_init,
        TimeInForce time_in_force = TimeInForce.GTC,
        uint64_t expire_time_ns = 0,
        bint post_only = False,
        bint reduce_only = False,
        bint quote_quantity = False,
        Quantity display_qty = None,
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
        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.is_true(expire_time_ns > 0, "`expire_time_ns` cannot be <= UNIX epoch.")
        else:
            # Should not have an expire time
            Condition.is_true(expire_time_ns == 0, "`expire_time_ns` was set when `time_in_force` not GTD.")
        Condition.is_true(
            display_qty is None or 0 <= display_qty <= quantity,
            fail_msg="`display_qty` was negative or greater than `quantity`",
        )

        # Set options
        cdef dict options = {
            "price": str(price),
            "display_qty": str(display_qty) if display_qty is not None else None,
            "expire_time_ns": expire_time_ns,
        }

        # Create initialization event
        cdef OrderInitialized init = OrderInitialized(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=order_side,
            order_type=OrderType.LIMIT,
            quantity=quantity,
            time_in_force=time_in_force,
            post_only=post_only,
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

        self.price = price
        self.expire_time_ns = expire_time_ns
        self.display_qty = display_qty

    cdef void _updated(self, OrderUpdated event):
        if self.venue_order_id is not None and event.venue_order_id is not None and self.venue_order_id != event.venue_order_id:
            self._venue_order_ids.append(self.venue_order_id)
            self.venue_order_id = event.venue_order_id

        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity.from_raw_c(self.quantity._mem.raw - self.filled_qty._mem.raw, self.quantity._mem.precision)

        if event.price is not None:
            self.price = event.price

    cdef void _set_slippage(self):
        if self.side == OrderSide.BUY:
            self.slippage = self.avg_px - self.price.as_f64_c()
        elif self.side == OrderSide.SELL:
            self.slippage = self.price.as_f64_c() - self.avg_px
    cdef bint has_price_c(self):
        return True

    cdef bint has_trigger_price_c(self):
        return False

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
            f"{order_type_to_str(self.order_type)} @ {self.price.to_formatted_str()} "
            f"{time_in_force_to_str(self.time_in_force)}{expiration_str}"
            f"{emulation_str}"
        )

    @staticmethod
    cdef LimitOrder from_pyo3_c(pyo3_order):
        return LimitOrder(
            trader_id=TraderId(str(pyo3_order.trader_id)),
            strategy_id=StrategyId(str(pyo3_order.strategy_id)),
            instrument_id=InstrumentId.from_str_c(str(pyo3_order.instrument_id)),
            client_order_id=ClientOrderId(str(pyo3_order.client_order_id)),
            order_side=order_side_from_str(str(pyo3_order.side)),
            quantity=Quantity.from_raw_c(pyo3_order.quantity.raw, pyo3_order.quantity.precision),
            price=Price.from_raw_c(pyo3_order.price.raw, pyo3_order.price.precision),
            init_id=UUID4.from_str_c(str(pyo3_order.init_id)),
            ts_init=pyo3_order.ts_init,
            time_in_force=time_in_force_from_str(str(pyo3_order.time_in_force)),
            expire_time_ns=int(pyo3_order.expire_time_ns) if pyo3_order.expire_time_ns is not None else 0,
            post_only=pyo3_order.is_post_only,
            reduce_only=pyo3_order.is_reduce_only,
            quote_quantity=pyo3_order.is_quote_quantity,
            display_qty=Quantity.from_str_c(pyo3_order.display_qty) if pyo3_order.display_qty is not None else None,
            emulation_trigger=trigger_type_from_str(str(pyo3_order.emulation_trigger)) if pyo3_order.emulation_trigger is not None else TriggerType.NO_TRIGGER,
            trigger_instrument_id=InstrumentId.from_str_c(str(pyo3_order.trigger_instrument_id)) if pyo3_order.trigger_instrument_id is not None else None,
            contingency_type=contingency_type_from_str(str(pyo3_order.contingency_type)) if pyo3_order.contingency_type is not None else ContingencyType.NO_CONTINGENCY,
            order_list_id=OrderListId(str(pyo3_order.order_list_id)) if pyo3_order.order_list_id is not None else None,
            linked_order_ids=[ClientOrderId(str(o)) for o in pyo3_order.linked_order_ids] if pyo3_order.linked_order_ids is not None else None,
            parent_order_id=ClientOrderId(str(pyo3_order.parent_order_id)) if pyo3_order.parent_order_id is not None else None,
            exec_algorithm_id=ExecAlgorithmId(str(pyo3_order.exec_algorithm_id)) if pyo3_order.exec_algorithm_id is not None else None,
            exec_algorithm_params=pyo3_order.exec_algorithm_params,
            exec_spawn_id=ClientOrderId(str(pyo3_order.exec_spawn_id)) if pyo3_order.exec_spawn_id is not None else None,
            tags=pyo3_order.tags if pyo3_order.tags is not None else None,
        )

    @staticmethod
    def from_pyo3(pyo3_order):
        return LimitOrder.from_pyo3_c(pyo3_order)

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
            "price": str(self.price),
            "time_in_force": time_in_force_to_str(self.time_in_force),
            "expire_time_ns": self.expire_time_ns if self.expire_time_ns > 0 else None,
            "filled_qty": str(self.filled_qty),
            "liquidity_side": liquidity_side_to_str(self.liquidity_side),
            "avg_px": self.avg_px if self.filled_qty.as_f64_c() > 0.0 else None,
            "slippage": self.slippage if self.filled_qty.as_f64_c() > 0.0 else None,
            "commissions": [str(c) for c in self.commissions()] if self._commissions else None,
            "status": self._fsm.state_string_c(),
            "is_post_only": self.is_post_only,
            "is_reduce_only": self.is_reduce_only,
            "is_quote_quantity": self.is_quote_quantity,
            "display_qty": str(self.display_qty) if self.display_qty is not None else None,
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
    cdef LimitOrder create_c(OrderInitialized init):
        """
        Return a `Limit` order from the given initialized event.

        Parameters
        ----------
        init : OrderInitialized
            The event to initialize with.

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If `init.order_type` is not equal to ``LIMIT``.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.order_type, OrderType.LIMIT, "init.order_type", "OrderType")

        cdef str display_qty_str = init.options.get("display_qty")

        return LimitOrder(
            trader_id=init.trader_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            client_order_id=init.client_order_id,
            order_side=init.side,
            quantity=init.quantity,
            price=Price.from_str_c(init.options["price"]),
            init_id=init.id,
            ts_init=init.ts_init,
            time_in_force=init.time_in_force,
            expire_time_ns=init.options["expire_time_ns"],
            post_only=init.post_only,
            reduce_only=init.reduce_only,
            quote_quantity=init.quote_quantity,
            display_qty=Quantity.from_str_c(display_qty_str) if display_qty_str is not None else None,
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
    def create(OrderInitialized init):
        return LimitOrder.create_c(init)

    @staticmethod
    cdef LimitOrder transform(Order order, uint64_t ts_init, Price price = None):
        """
        Transform the given order to a `limit` order.

        All existing events will be prepended to the orders internal events
        prior to the new `OrderInitialized` event.

        Parameters
        ----------
        order : Order
            The order to transform from.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the object was initialized.
        price : Price, optional
            The price to assign to the order (will override any existing price on `order`).

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If `price` is ``None`` and `order` does not have a `price` attribute.

        """
        Condition.not_none(order, "order")
        Condition.is_true(price or hasattr(order, "price"), "`order` has no price")

        cdef LimitOrder transformed = LimitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            order_side=order.side,
            quantity=order.quantity,
            price=price or order.price,
            time_in_force=order.time_in_force,
            expire_time_ns=order.expire_time_ns if hasattr(order, "expire_time_ns") else 0,
            init_id=UUID4(),
            ts_init=ts_init,
            post_only=order.is_post_only if hasattr(order, "is_post_only") else False,
            reduce_only=order.is_reduce_only,
            quote_quantity=order.is_quote_quantity,
            display_qty=order.display_qty if hasattr(order, "display_qty") else None,
            contingency_type=order.contingency_type,
            order_list_id=order.order_list_id,
            linked_order_ids=order.linked_order_ids,
            parent_order_id=order.parent_order_id,
            exec_algorithm_id=order.exec_algorithm_id,
            exec_algorithm_params=order.exec_algorithm_params,
            exec_spawn_id=order.exec_spawn_id,
            tags=order.tags,
        )
        transformed.liquidity_side = order.liquidity_side
        cdef Price triggered_price = order.get_triggered_price_c()
        if triggered_price:
            transformed.set_triggered_price_c(triggered_price)

        # Use original order initialization timestamp
        transformed.ts_init = order.ts_init

        Order._hydrate_initial_events(original=order, transformed=transformed)

        return transformed

    @staticmethod
    def transform_py(Order order, uint64_t ts_init, Price price = None) -> LimitOrder:
        return LimitOrder.transform(order, ts_init, price)
