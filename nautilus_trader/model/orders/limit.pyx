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

from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport PassiveOrder


cdef class LimitOrder(PassiveOrder):
    """
    Limit orders are used to specify a maximum or minimum price the trader is
    willing to buy or sell at. Traders use this order type to minimise their
    trading cost, however they are sacrificing guaranteed execution as there is
    a chance the order may not be executed if it is placed deep out of the
    market.
    """
    def __init__(
        self,
        ClientOrderId client_order_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price price not None,
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        UUID init_id not None,
        int64_t timestamp_ns,
        bint post_only=True,
        bint reduce_only=False,
        bint hidden=False,
    ):
        """
        Initialize a new instance of the ``LimitOrder`` class.

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
        price : Price
            The order limit price.
        time_in_force : TimeInForce
            The order time-in-force.
        expire_time : datetime, optional
            The order expiry time.
        init_id : UUID
            The order initialization event identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the order initialization.
        post_only : bool, optional
            If the order will only make a market.
        reduce_only : bool, optional
            If the order will only reduce an open position.
        hidden : bool, optional
            If the order will be hidden from the public book.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is GTD and expire_time is None.
        ValueError
            If post_only and hidden.
        ValueError
            If hidden and post_only.

        """
        if post_only:
            Condition.false(hidden, "A post-only order is not hidden")
        if hidden:
            Condition.false(post_only, "A hidden order is not post-only")
        super().__init__(
            client_order_id=client_order_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            order_side=order_side,
            order_type=OrderType.LIMIT,
            quantity=quantity,
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            init_id=init_id,
            timestamp_ns=timestamp_ns,
            options={
                POST_ONLY: post_only,
                REDUCE_ONLY: reduce_only,
                HIDDEN: hidden,
            },
        )

        self.is_post_only = post_only
        self.is_reduce_only = reduce_only
        self.is_hidden = hidden

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
            "price": str(self.price),
            "liquidity_side": LiquiditySideParser.to_str(self.liquidity_side),
            "expire_time": self.expire_time,
            "ts_expire_time": self.expire_time_ns,
            "timestamp_ns": self.timestamp_ns,
            "time_in_force": TimeInForceParser.to_str(self.time_in_force),
            "filled_qty": str(self.filled_qty),
            "ts_filled_ns": self.ts_filled_ns,
            "avg_px": str(self.avg_px) if self.avg_px else None,
            "slippage": str(self.slippage),
            "init_id": str(self.init_id),
            "state": self._fsm.state_string_c(),
            "is_post_only": self.is_post_only,
            "is_reduce_only": self.is_reduce_only,
            "is_hidden": self.is_hidden,
        }

    @staticmethod
    cdef LimitOrder create(OrderInitialized init):
        """
        Return a limit order from the given initialized event.

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
            If init.order_type is not equal to LIMIT.

        """
        Condition.not_none(init, "init")
        Condition.equal(init.order_type, OrderType.LIMIT, "init.order_type", "OrderType")

        return LimitOrder(
            client_order_id=init.client_order_id,
            strategy_id=init.strategy_id,
            instrument_id=init.instrument_id,
            order_side=init.order_side,
            quantity=init.quantity,
            price=Price.from_str_c(init.options[PRICE]),
            time_in_force=init.time_in_force,
            expire_time=init.options.get(EXPIRE_TIME),
            init_id=init.id,
            timestamp_ns=init.timestamp_ns,
            post_only=init.options[POST_ONLY],
            reduce_only=init.options[REDUCE_ONLY],
            hidden=init.options[HIDDEN],
        )
