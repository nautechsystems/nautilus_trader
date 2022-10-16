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

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyType
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyTypeParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class MarketOrder(Order):
    """
    Represents a `Market` order.

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
        If `time_in_force` is ``GTD``.
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
        ContingencyType contingency_type = ContingencyType.NONE,
        OrderListId order_list_id = None,
        list linked_order_ids = None,
        ClientOrderId parent_order_id = None,
        str tags = None,
    ):
        Condition.not_equal(order_side, OrderSide.NONE, "order_side", "NONE")
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
            options={},
            emulation_trigger=TriggerType.NONE,
            contingency_type=contingency_type,
            order_list_id=order_list_id,
            linked_order_ids=linked_order_ids,
            parent_order_id=parent_order_id,
            tags=tags,
            event_id=init_id,
            ts_init=ts_init,
        )
        super().__init__(init=init)

    cdef bint has_price_c(self) except *:
        return False

    cdef bint has_trigger_price_c(self) except *:
        return False

    cpdef str info(self):
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        return (
            f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
            f"{OrderTypeParser.to_str(self.order_type)} "
            f"{TimeInForceParser.to_str(self.time_in_force)}"
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
            "type": OrderTypeParser.to_str(self.order_type),
            "side": OrderSideParser.to_str(self.side),
            "quantity": str(self.quantity),
            "time_in_force": TimeInForceParser.to_str(self.time_in_force),
            "reduce_only": self.is_reduce_only,
            "filled_qty": str(self.filled_qty),
            "avg_px": str(self.avg_px),
            "slippage": str(self.slippage),
            "status": self._fsm.state_string_c(),
            "contingency_type": ContingencyTypeParser.to_str(self.contingency_type),
            "order_list_id": self.order_list_id.to_str() if self.order_list_id is not None else None,
            "linked_order_ids": ",".join([o.to_str() for o in self.linked_order_ids]) if self.linked_order_ids is not None else None,  # noqa
            "parent_order_id": self.parent_order_id.to_str() if self.parent_order_id is not None else None,
            "tags": self.tags,
            "ts_last": self.ts_last,
            "ts_init": self.ts_init,
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
            init_id=init.id,
            ts_init=init.ts_init,
            contingency_type=init.contingency_type,
            order_list_id=init.order_list_id,
            linked_order_ids=init.linked_order_ids,
            parent_order_id=init.parent_order_id,
            tags=init.tags,
        )

    cdef void _updated(self, OrderUpdated event) except *:
        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity.from_raw_c(self.quantity._mem.raw - self.filled_qty._mem.raw, self.quantity._mem.precision)
