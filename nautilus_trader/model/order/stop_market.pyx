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

from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport PassiveOrder


cdef class StopMarketOrder(PassiveOrder):
    """
    Represents a stop market order. Once the price crosses the predefined
    trigger price, the stop order becomes a market order.
    """
    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        StrategyId strategy_id not None,
        Symbol symbol not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price price not None,
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        UUID init_id not None,
        datetime timestamp not None,
        bint reduce_only=False,
    ):
        """
        Initialize a new instance of the `StopMarketOrder` class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide (Enum)
            The order side (BUY or SELL).
        quantity : Quantity
            The order quantity (> 0).
        price : Price
            The order stop price.
        time_in_force : TimeInForce (Enum)
            The order time-in-force.
        expire_time : datetime, optional
            The order expiry time.
        init_id : UUID
            The order initialization event identifier.
        timestamp : datetime
            The order initialization timestamp.
        reduce_only : bool, optional
            If the order will only reduce an open position.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If order_side is UNDEFINED.
        ValueError
            If time_in_force is UNDEFINED.
        ValueError
            If time_in_force is GTD and the expire_time is None.

        """
        super().__init__(
            cl_ord_id,
            strategy_id,
            symbol,
            order_side,
            OrderType.STOP_MARKET,
            quantity,
            price,
            time_in_force,
            expire_time,
            init_id,
            timestamp,
            options={REDUCE_ONLY: reduce_only},
        )

        self.is_reduce_only = reduce_only

    @staticmethod
    cdef StopMarketOrder create(OrderInitialized event):
        """
        Return a stop order from the given initialized event.

        Parameters
        ----------
        event : OrderInitialized
            The event to initialize with.

        Returns
        -------
        StopMarketOrder

        Raises
        ------
        ValueError
            If event.order_type is not equal to OrderType.STOP_MARKET.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.order_type, OrderType.STOP_MARKET, "event.order_type", "OrderType")

        return StopMarketOrder(
            cl_ord_id=event.cl_ord_id,
            strategy_id=event.strategy_id,
            symbol=event.symbol,
            order_side=event.order_side,
            quantity=event.quantity,
            price=Price(event.options.get(PRICE)),
            time_in_force=event.time_in_force,
            expire_time=event.options.get(EXPIRE_TIME),
            init_id=event.id,
            timestamp=event.timestamp,
            reduce_only=event.options[REDUCE_ONLY],
        )
