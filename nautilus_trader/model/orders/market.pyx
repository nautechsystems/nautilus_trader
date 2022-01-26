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
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyType
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyTypeParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef set _MARKET_ORDER_VALID_TIF = {
    TimeInForce.GTC,
    TimeInForce.IOC,
    TimeInForce.FOK,
    TimeInForce.FAK,
    TimeInForce.OC,
}


cdef class MarketOrder(Order):
    """
    Represents a market order.

    A market order is an order to buy or sell an instrument immediately. This
    type of order guarantees that the order will be executed, but does not
    guarantee the execution price. A market order generally will execute at or
    near the current bid (for a sell order) or ask (for a buy order) price. The
    last-traded price is not necessarily the price at which a market order will
    be executed.

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
    contingency_type : ContingencyType
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
        If `time_in_force` is other than ``GTC``, ``IOC`` or ``FOK``.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        TimeInForce time_in_force,
        UUID4 init_id not None,
        int64_t ts_init,
        bint reduce_only=False,
        OrderListId order_list_id=None,
        ClientOrderId parent_order_id=None,
        list child_order_ids=None,
        ContingencyType contingency_type=ContingencyType.NONE,
        list contingency_ids=None,
        str tags=None,
    ):
        Condition.true(
            time_in_force in _MARKET_ORDER_VALID_TIF,
            fail_msg="time_in_force was != GTC, IOC or FOK",
        )

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
            order_list_id=order_list_id,
            parent_order_id=parent_order_id,
            child_order_ids=child_order_ids,
            contingency_type=contingency_type,
            contingency_ids=contingency_ids,
            tags=tags,
            event_id=init_id,
            ts_init=ts_init,
        )
        super().__init__(init=init)

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
            "venue_order_id": self.venue_order_id.value if self.venue_order_id else None,
            "position_id": self.position_id.value if self.position_id else None,
            "account_id": self.account_id.value if self.account_id else None,
            "last_trade_id": self.last_trade_id.value if self.last_trade_id else None,
            "type": OrderTypeParser.to_str(self.type),
            "side": OrderSideParser.to_str(self.side),
            "quantity": str(self.quantity),
            "time_in_force": TimeInForceParser.to_str(self.time_in_force),
            "reduce_only": self.is_reduce_only,
            "filled_qty": str(self.filled_qty),
            "avg_px": str(self.avg_px) if self.avg_px else None,
            "slippage": str(self.slippage),
            "status": self._fsm.state_string_c(),
            "order_list_id": self.order_list_id,
            "parent_order_id": self.parent_order_id,
            "child_order_ids": ",".join([o.value for o in self.child_order_ids]) if self.child_order_ids is not None else None,  # noqa
            "contingency_type": ContingencyTypeParser.to_str(self.contingency_type),
            "contingency_ids": ",".join([o.value for o in self.contingency_ids]) if self.contingency_ids is not None else None,  # noqa
            "tags": self.tags,
            "ts_last": self.ts_last,
            "ts_init": self.ts_init,
        }

    @staticmethod
    cdef MarketOrder create(OrderInitialized init):
        """
        Return an order from the given initialized event.

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
            If `init.type` is not equal to ``MARKET``.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.type, OrderType.MARKET, "init.type", "OrderType")

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
            order_list_id=init.order_list_id,
            parent_order_id=init.parent_order_id,
            child_order_ids=init.child_order_ids,
            contingency_type=init.contingency_type,
            contingency_ids=init.contingency_ids,
            tags=init.tags,
        )

    cpdef str info(self):
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        return (
            f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
            f"{OrderTypeParser.to_str(self.type)} "
            f"{TimeInForceParser.to_str(self.time_in_force)}"
        )
