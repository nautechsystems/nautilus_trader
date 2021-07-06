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
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderTriggered
from nautilus_trader.model.events cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport PassiveOrder


cdef class StopLimitOrder(PassiveOrder):
    """
    Represents a stop-limit order.

    A stop-limit order is an instruction to submit a buy or sell limit order
    when the user-specified stop trigger price is attained or penetrated. The
    order has two basic components: the stop price and the limit price. When a
    trade has occurred at or through the stop price, the order becomes
    executable and enters the market as a limit order, which is an order to buy
    or sell at a specified price or better.

    A stop-limit eliminates the price risk associated with a stop order where
    the execution price cannot be guaranteed, but exposes the trader to the
    risk that the order may never fill even if the stop price is reached. The
    trader could "miss the market" altogether.
    """
    def __init__(
        self,
        ClientOrderId client_order_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price price not None,
        Price trigger not None,
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        UUID init_id not None,
        int64_t timestamp_ns,
        bint post_only=False,
        bint reduce_only=False,
        bint hidden=False,
    ):
        """
        Initialize a new instance of the ``StopLimitOrder`` class.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID.
        strategy_id : StrategyId
            The strategy ID associated with the order.
        instrument_id : InstrumentId
            The order instrument_id.
        order_side : OrderSide
            The order side (BUY or SELL).
        quantity : Quantity
            The order quantity (> 0).
        price : Price
            The order limit price.
        trigger : Price
            The order stop trigger price.
        time_in_force : TimeInForce
            The order time-in-force.
        expire_time : datetime, optional
            The order expiry time.
        init_id : UUID
            The order initialization event ID.
        timestamp_ns : int64
            The UNIX timestamp (nanoseconds) of the order initialization.
        post_only : bool, optional
            If the order will only make a market (once triggered).
        reduce_only : bool, optional
            If the order will only reduce an open position (once triggered).
        hidden : bool, optional
            If the order will be hidden from the public book (once triggered).


        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is GTD and the expire_time is None.
        ValueError
            If post_only and hidden.

        """
        if post_only:
            Condition.false(hidden, "A post-only order cannot be hidden")
        super().__init__(
            client_order_id=client_order_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            order_side=order_side,
            order_type=OrderType.STOP_LIMIT,
            quantity=quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=init_id,
            timestamp_ns=timestamp_ns,
            options={
                "trigger": str(trigger),
                "post_only": post_only,
                "reduce_only": reduce_only,
                "hidden": hidden,
            },
        )

        self.trigger = trigger
        self.is_triggered = False
        self.is_post_only = post_only
        self.is_reduce_only = reduce_only
        self.is_hidden = hidden

    def __repr__(self) -> str:
        cdef str id_string = f", id={self.venue_order_id.value})" if self.venue_order_id.not_null() else ")"
        return (f"{type(self).__name__}("
                f"{self.status_string_c()}, "
                f"trigger={self.trigger}, "
                f"state={self._fsm.state_string_c()}, "
                f"client_order_id={self.client_order_id.value}"
                f"{id_string}")

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "client_order_id": self.client_order_id.value,
            "venue_order_id": self.venue_order_id.value,
            "position_id": self.position_id.value,
            "strategy_id": self.strategy_id.value,
            "account_id": self.account_id.value if self.account_id else None,
            "execution_id": self.execution_id.value if self.execution_id else None,
            "instrument_id": self.instrument_id.value,
            "type": OrderTypeParser.to_str(self.type),
            "side": OrderSideParser.to_str(self.side),
            "quantity": str(self.quantity),
            "trigger": str(self.trigger),
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
            "is_post_only": self.is_post_only,
            "is_reduce_only": self.is_reduce_only,
            "is_hidden": self.is_hidden,
        }

    @staticmethod
    cdef StopLimitOrder create(OrderInitialized init):
        """
        Return a stop-limit order from the given initialized event.

        Parameters
        ----------
        init : OrderInitialized
            The event to initialize with.

        Returns
        -------
        StopLimitOrder

        Raises
        ------
        ValueError
            If init.order_type is not equal to STOP_LIMIT.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.order_type, OrderType.STOP_LIMIT, "init.order_type", "OrderType")

        return StopLimitOrder(
            client_order_id=init.client_order_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            order_side=init.order_side,
            quantity=init.quantity,
            price=Price.from_str_c(init.options["price"]),
            trigger=Price.from_str_c(init.options["trigger"]),
            time_in_force=init.time_in_force,
            expire_time=maybe_nanos_to_unix_dt(init.options.get("expire_time")),
            init_id=init.id,
            timestamp_ns=init.timestamp_ns,
            post_only=init.options["post_only"],
            reduce_only=init.options["reduce_only"],
            hidden=init.options["hidden"],
        )

    cdef void _updated(self, OrderUpdated event) except *:
        self.venue_order_id = event.venue_order_id
        self.quantity = event.quantity
        if self.is_triggered:
            self.price = event.price
        else:
            self.trigger = event.price

    cdef void _triggered(self, OrderTriggered event) except *:
        self.is_triggered = True
