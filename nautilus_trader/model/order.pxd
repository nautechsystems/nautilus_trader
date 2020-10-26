# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderEvent
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderModified
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderWorking
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport BracketOrderId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Order:
    cdef list _execution_ids
    cdef list _events
    cdef FiniteStateMachine _fsm

    cdef readonly ClientOrderId cl_ord_id
    """
    Returns
    -------
    ClientOrderId
        The client order identifier of the order.
    """

    cdef readonly StrategyId strategy_id
    """
    Returns
    -------
    StrategyId
        The strategy identifier associated with the order.
    """

    cdef readonly OrderId id
    """
    Returns
    -------
    OrderId or None
        The order identifier.
    """

    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId or None
        The account identifier associated with the order.

    """

    cdef readonly ExecutionId execution_id
    """
    Returns
    -------
    ExecutionId or None
        The last execution identifier of the order.

    """

    cdef readonly PositionId position_id
    """
    Returns
    -------
    PositionId or None
        The position identifier associated with the order.

    """

    cdef readonly Symbol symbol
    """
    Returns
    -------
    Symbol
        The order symbol.

    """

    cdef readonly OrderSide side
    """
    Returns
    -------
    OrderSide
        The order side.

    """

    cdef readonly OrderType type
    """
    Returns
    -------
    OrderType
        The order type.

    """

    cdef readonly Quantity quantity
    """
    Returns
    -------
    Quantity
        The order quantity.

    """

    cdef readonly datetime timestamp
    """
    Returns
    -------
    datetime
        The order initialization timestamp.

    """

    cdef readonly TimeInForce time_in_force
    """
    Returns
    -------
    TimeInForce
        The order time-in-force.

    """

    cdef readonly Quantity filled_qty
    """
    Returns
    -------
    Quantity
        The order total filled quantity.

    """

    cdef readonly datetime filled_timestamp
    """
    Returns
    -------
    datetime or None
        The order last filled timestamp.

    """

    cdef readonly Decimal avg_price
    """
    Returns
    -------
    Decimal or None
        The order average fill price.

    """

    cdef readonly Decimal slippage
    """
    Returns
    -------
    Decimal
        The order total price slippage.

    """

    cdef readonly UUID init_id
    """
    Returns
    -------
    UUID
        The identifier of the `OrderInitialized` event.

    """

    @staticmethod
    cdef inline OrderSide opposite_side_c(OrderSide side) except *

    @staticmethod
    cdef inline OrderSide flatten_side_c(PositionSide side) except *

    cdef str state_string(self)
    cdef str status_string(self)
    cpdef void apply(self, OrderEvent event) except *
    cdef void _invalid(self, OrderInvalid event) except *
    cdef void _denied(self, OrderDenied event) except *
    cdef void _submitted(self, OrderSubmitted event) except *
    cdef void _rejected(self, OrderRejected event) except *
    cdef void _accepted(self, OrderAccepted event) except *
    cdef void _working(self, OrderWorking event) except *
    cdef void _cancelled(self, OrderCancelled event) except *
    cdef void _expired(self, OrderExpired event) except *
    cdef void _modified(self, OrderModified event) except *
    cdef void _filled(self, OrderFilled event) except *


cdef class PassiveOrder(Order):
    cdef readonly Price price
    """
    Returns
    -------
    Price
        The order price.

    """

    cdef readonly LiquiditySide liquidity_side
    """
    Returns
    -------
    LiquiditySide
        The order liquidity size.

    """

    cdef readonly datetime expire_time
    """
    Returns
    -------
    datetime or None
        The order expire time.

    """

    cdef void _set_slippage(self) except *


cdef class MarketOrder(Order):
    @staticmethod
    cdef MarketOrder create(OrderInitialized event)


cdef class StopMarketOrder(PassiveOrder):
    @staticmethod
    cdef StopMarketOrder create(OrderInitialized event)


cdef class LimitOrder(PassiveOrder):
    cdef readonly bint is_post_only
    """
    Return a value indicating whether the order is `post_only`, meaning it will
    only make liquidity.

    Returns
    -------
    bool
        True if post only, else False.

    """

    cdef readonly bint is_hidden
    """
    Return a value indicating whether the order is displayed on the public order
    book.

    Returns
    -------
    bool
        True if hidden, else False.

    """

    @staticmethod
    cdef LimitOrder create(OrderInitialized event)


cdef class BracketOrder:
    cdef readonly BracketOrderId id
    """
    Returns
    -------
    BracketOrderId
        The bracket order identifier.

    """

    cdef readonly Order entry
    """
    Returns
    -------
    Order
        The entry order.

    """

    cdef readonly StopMarketOrder stop_loss
    """
    Returns
    -------
    StopMarketOrder
        The stop-loss order.

    """

    cdef readonly PassiveOrder take_profit
    """
    Returns
    -------
    PassiveOrder or None
        The take-profit order (optional).

    """

    cdef readonly bint has_take_profit
    """
    Return a value indicating whether the bracket order has a take-profit

    Returns
    -------
    bool
        True if has take-profit, else False.

    """

    cdef readonly datetime timestamp
    """
    Returns
    -------
    datetime
        The bracket order initialization timestamp.

    """
