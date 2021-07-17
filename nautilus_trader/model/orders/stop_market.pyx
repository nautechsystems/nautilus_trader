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

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport maybe_nanos_to_unix_dt
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport PassiveOrder


cdef class StopMarketOrder(PassiveOrder):
    """
    Represents a stop-market order.

    A stop-market order is an instruction to submit a buy or sell market order
    if and when the user-specified stop trigger price is attained or penetrated.
    A stop-market order is not guaranteed a specific execution price and may
    execute significantly away from its stop price. A Sell Stop order is always
    placed below the current market price and is typically used to limit a loss
    or protect a profit on a long stock position. A Buy Stop order is always
    placed above the current market price. It is typically used to limit a loss
    or help protect a profit on a short sale.
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
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        UUID init_id not None,
        int64_t timestamp_ns,
        bint reduce_only=False,
    ):
        """
        Initialize a new instance of the ``StopMarketOrder`` class.

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
        order_side : OrderSide
            The order side (BUY or SELL).
        quantity : Quantity
            The order quantity (> 0).
        price : Price
            The order stop price.
        time_in_force : TimeInForce
            The order time-in-force.
        expire_time : datetime, optional
            The order expiry time.
        init_id : UUID
            The order initialization event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the order initialization.
        reduce_only : bool, optional
            If the order will only reduce an open position.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is GTD and the expire_time is None.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=order_side,
            order_type=OrderType.STOP_MARKET,
            quantity=quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=init_id,
            timestamp_ns=timestamp_ns,
            options={"reduce_only": reduce_only},
        )

        self.is_reduce_only = reduce_only

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
            "venue_order_id": self.venue_order_id.value,
            "position_id": self.position_id.value,
            "account_id": self.account_id.value if self.account_id else None,
            "execution_id": self.execution_id.value if self.execution_id else None,
            "type": OrderTypeParser.to_str(self.type),
            "side": OrderSideParser.to_str(self.side),
            "quantity": str(self.quantity),
            "price": str(self.price),
            "liquidity_side": LiquiditySideParser.to_str(self.liquidity_side),
            "expire_time_ns": self.expire_time_ns,
            "timestamp_ns": self.timestamp_ns,
            "time_in_force": TimeInForceParser.to_str(self.time_in_force),
            "filled_qty": str(self.filled_qty),
            "ts_filled_ns": self.ts_filled_ns,
            "avg_px": str(self.avg_px) if self.avg_px else None,
            "slippage": str(self.slippage),
            "state": self._fsm.state_string_c(),
            "is_reduce_only": self.is_reduce_only,
        }

    @staticmethod
    cdef StopMarketOrder create(OrderInitialized init):
        """
        Return a stop-market order from the given initialized event.

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
            If init.type is not equal to STOP_MARKET.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.type, OrderType.STOP_MARKET, "init.type", "OrderType")

        return StopMarketOrder(
            trader_id=init.trader_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            client_order_id=init.client_order_id,
            order_side=init.side,
            quantity=init.quantity,
            price=Price.from_str_c(init.options["price"]),
            time_in_force=init.time_in_force,
            expire_time=maybe_nanos_to_unix_dt(init.options.get("expire_time")),
            init_id=init.id,
            timestamp_ns=init.timestamp_ns,
            reduce_only=init.options["reduce_only"],
        )
