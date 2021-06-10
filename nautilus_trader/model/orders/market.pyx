# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef set _MARKET_ORDER_VALID_TIF = {
    TimeInForce.GTC,
    TimeInForce.IOC,
    TimeInForce.FOK,
    TimeInForce.FAK,
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
    """
    def __init__(
        self,
        ClientOrderId client_order_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        TimeInForce time_in_force,
        UUID init_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``MarketOrder`` class.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        instrument_id : InstrumentId
            The order instrument_id.
        order_side : OrderSide
            The order side (BUY or SELL).
        quantity : Quantity
            The order quantity (> 0).
        init_id : UUID
            The order initialization event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the order initialization.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is other than GTC, IOC or FOK.

        """
        Condition.positive(quantity, "quantity")
        Condition.true(time_in_force in _MARKET_ORDER_VALID_TIF, "time_in_force was != GTC, IOC or FOK")

        cdef OrderInitialized init = OrderInitialized(
            client_order_id=client_order_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            order_side=order_side,
            order_type=OrderType.MARKET,
            quantity=quantity,
            time_in_force=time_in_force,
            event_id=init_id,
            timestamp_ns=timestamp_ns,
            options={},
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
            "type": type(self).__name__,
            "client_order_id": self.client_order_id.value,
            "venue_order_id": self.venue_order_id.value,
            "position_id": self.position_id.value,
            "strategy_id": self.strategy_id.value,
            "account_id": self.account_id.value if self.account_id else None,
            "execution_id": self.execution_id.value if self.execution_id else None,
            "instrument_id": self.instrument_id.value,
            "order_side": OrderSideParser.to_str(self.side),
            "order_type": OrderTypeParser.to_str(self.type),
            "quantity": str(self.quantity),
            "timestamp_ns": self.timestamp_ns,
            "time_in_force": TimeInForceParser.to_str(self.time_in_force),
            "filled_qty": str(self.filled_qty),
            "ts_filled_ns": self.ts_filled_ns,
            "avg_px": str(self.avg_px) if self.avg_px else None,
            "slippage": str(self.slippage),
            "init_id": str(self.init_id),
            "state": self._fsm.state_string_c(),
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
            If init.order_type is not equal to MARKET.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.order_type, OrderType.MARKET, "init.order_type", "OrderType")

        return MarketOrder(
            client_order_id=init.client_order_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            order_side=init.order_side,
            quantity=init.quantity,
            time_in_force=init.time_in_force,
            init_id=init.id,
            timestamp_ns=init.timestamp_ns,
        )

    cdef str status_string_c(self):
        return (f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
                f"{OrderTypeParser.to_str(self.type)} "
                f"{TimeInForceParser.to_str(self.time_in_force)}")

    cdef void _updated(self, OrderUpdated event) except *:
        raise NotImplemented("Cannot update a market order")

    cdef void _filled(self, OrderFilled fill) except *:
        self.venue_order_id = fill.venue_order_id
        self.position_id = fill.position_id
        self.strategy_id = fill.strategy_id
        self._execution_ids.append(fill.execution_id)
        self.execution_id = fill.execution_id
        self.filled_qty = Quantity(self.filled_qty + fill.last_qty, fill.last_qty.precision)
        self.ts_filled_ns = fill.ts_filled_ns
        self.avg_px = self._calculate_avg_px(fill.last_qty, fill.last_px)
