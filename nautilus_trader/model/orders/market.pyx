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

import msgspec

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
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
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class MarketOrder(Order):
    """
    Represents a `Market` order.

    A Market order is an order to BUY (or SELL) at the market bid or offer price.
    A market order may increase the likelihood of a fill and the speed of
    execution, but unlike the Limit order - a Market order provides no price
    protection and may fill at a price far lower/higher than the top of book
    bid/ask.

    - A `Market-On-Open (MOO)` order can be represented using a time in force of ``AT_THE_OPEN``.
    - A `Market-On-Close (MOC)` order can be represented using a time in force of ``AT_THE_CLOSE``.

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
    init_id : UUID4
        The order initialization event ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
        The order time in force.
    reduce_only : bool, default False
        If the order carries the 'reduce-only' execution instruction.
    quote_quantity : bool, default False
        If the order quantity is denominated in the quote currency.
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
        If `time_in_force` is ``GTD``.

    References
    ----------
    https://www.interactivebrokers.com/en/trading/orders/market.php
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        UUID4 init_id not None,
        uint64_t ts_init,
        TimeInForce time_in_force = TimeInForce.GTC,
        bint reduce_only = False,
        bint quote_quantity = False,
        ContingencyType contingency_type = ContingencyType.NO_CONTINGENCY,
        OrderListId order_list_id = None,
        list linked_order_ids = None,
        ClientOrderId parent_order_id = None,
        ExecAlgorithmId exec_algorithm_id = None,
        dict exec_algorithm_params = None,
        ClientOrderId exec_spawn_id = None,
        str tags = None,
    ):
        Condition.not_equal(order_side, OrderSide.NO_ORDER_SIDE, "order_side", "NO_ORDER_SIDE")
        Condition.not_equal(time_in_force, TimeInForce.GTD, "time_in_force", "GTD")

        # Create initialization event
        cdef OrderInitialized init = OrderInitialized(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=order_side,
            order_type=OrderType.MARKET,
            quantity=quantity,
            time_in_force=time_in_force,
            post_only=False,
            reduce_only=reduce_only,
            quote_quantity=quote_quantity,
            options={},
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
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

    cdef void _updated(self, OrderUpdated event):
        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity.from_raw_c(self.quantity._mem.raw - self.filled_qty._mem.raw, self.quantity._mem.precision)

    cdef bint has_price_c(self):
        return False

    cdef bint has_trigger_price_c(self):
        return False

    cpdef str info(self):
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        return (
            f"{order_side_to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
            f"{order_type_to_str(self.order_type)} "
            f"{time_in_force_to_str(self.time_in_force)}"
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
            "time_in_force": time_in_force_to_str(self.time_in_force),
            "is_reduce_only": self.is_reduce_only,
            "is_quote_quantity": self.is_quote_quantity,
            "filled_qty": str(self.filled_qty),
            "liquidity_side": liquidity_side_to_str(self.liquidity_side),
            "avg_px": str(self.avg_px) if self.filled_qty.as_f64_c() > 0.0 else None,
            "slippage": str(self.slippage) if self.filled_qty.as_f64_c() > 0.0 else None,
            "commissions": str([c.to_str() for c in self.commissions()]) if self._commissions else None,
            "status": self._fsm.state_string_c(),
            "contingency_type": contingency_type_to_str(self.contingency_type),
            "order_list_id": self.order_list_id.to_str() if self.order_list_id is not None else None,
            "linked_order_ids": ",".join([o.to_str() for o in self.linked_order_ids]) if self.linked_order_ids is not None else None,  # noqa
            "parent_order_id": self.parent_order_id.to_str() if self.parent_order_id is not None else None,
            "exec_algorithm_id": self.exec_algorithm_id.to_str() if self.exec_algorithm_id is not None else None,
            "exec_algorithm_params": msgspec.json.encode(self.exec_algorithm_params) if self.exec_algorithm_params is not None else None,  # noqa
            "exec_spawn_id": self.exec_spawn_id.to_str() if self.exec_spawn_id is not None else None,
            "tags": self.tags,
            "ts_init": self.ts_init,
            "ts_last": self.ts_last,
        }

    @staticmethod
    cdef MarketOrder create(OrderInitialized init):
        """
        Return a `market` order from the given initialized event.

        Parameters
        ----------
        init : OrderInitialized
            The event to initialize with.

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If `init.order_type` is not equal to ``MARKET``.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.order_type, OrderType.MARKET, "init.order_type", "OrderType")

        return MarketOrder(
            trader_id=init.trader_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            client_order_id=init.client_order_id,
            order_side=init.side,
            quantity=init.quantity,
            time_in_force=init.time_in_force,
            reduce_only=init.reduce_only,
            quote_quantity=init.quote_quantity,
            init_id=init.id,
            ts_init=init.ts_init,
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
    cdef MarketOrder transform(Order order, uint64_t ts_init):
        """
        Transform the given order to a `market` order.

        All existing events will be prepended to the orders internal events
        prior to the new `OrderInitialized` event.

        Parameters
        ----------
        order : Order
            The order to transform from.
        ts_init : uint64_t
            The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        MarketOrder

        """
        Condition.not_none(order, "order")

        cdef list original_events = order.events_c()
        cdef MarketOrder transformed = MarketOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            order_side=order.side,
            quantity=order.quantity,
            time_in_force=order.time_in_force if order.time_in_force != TimeInForce.GTD else TimeInForce.GTC,
            reduce_only=order.is_reduce_only,
            quote_quantity=order.is_quote_quantity,
            init_id=UUID4(),
            ts_init=ts_init,
            contingency_type=order.contingency_type,
            order_list_id=order.order_list_id,
            linked_order_ids=order.linked_order_ids,
            parent_order_id=order.parent_order_id,
            exec_algorithm_id=order.exec_algorithm_id,
            exec_algorithm_params=order.exec_algorithm_params,
            exec_spawn_id=order.exec_spawn_id,
            tags=order.tags,
        )

        Order._hydrate_initial_events(original=order, transformed=transformed)

        return transformed

    @staticmethod
    def transform_py(Order order, uint64_t ts_init) -> MarketOrder:
        return MarketOrder.transform(order, ts_init)
