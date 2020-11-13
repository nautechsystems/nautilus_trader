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

from nautilus_trader.core.fsm cimport FiniteStateMachine
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
    cdef list _events
    cdef list _execution_ids
    cdef FiniteStateMachine _fsm

    cdef readonly ClientOrderId cl_ord_id
    """The orders client order identifier.\n\n:returns: `ClientOrderId`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the order.\n\n:returns: `StrategyId`"""
    cdef readonly OrderId id
    """The order identifier (exchange/broker).\n\n:returns: `OrderId`"""
    cdef readonly AccountId account_id
    """The account identifier associated with the order.\n\n:returns: `AccountId`"""
    cdef readonly ExecutionId execution_id
    """The orders last execution identifier.\n\n:returns: `ExecutionId`"""
    cdef readonly PositionId position_id
    """The position identifier associated with the order.\n\n:returns: `PositionId`"""
    cdef readonly Symbol symbol
    """The order symbol.\n\n:returns: `Symbol`"""
    cdef readonly OrderSide side
    """The order side.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType type
    """The order type.\n\n:returns: `OrderType`"""
    cdef readonly Quantity quantity
    """The order quantity.\n\n:returns: `Quantity`"""
    cdef readonly datetime timestamp
    """The order initialization timestamp.\n\n:returns: `datetime`"""
    cdef readonly TimeInForce time_in_force
    """The order time-in-force.\n\n:returns: `TimeInForce`"""
    cdef readonly Quantity filled_qty
    """The order total filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly datetime filled_timestamp
    """The order last filled timestamp.\n\n:returns: `datetime`"""
    cdef readonly object avg_price
    """The order average fill price.\n\n:returns: `decimal.Decimal`"""
    cdef readonly object slippage
    """The order total price slippage.\n\n:returns: `decimal.Decimal`"""
    cdef readonly UUID init_id
    """The identifier of the `OrderInitialized` event.\n\n:returns: `UUID`"""

    cdef OrderState state_c(self) except *
    cdef OrderEvent last_event_c(self)
    cdef list events_c(self)
    cdef list execution_ids_c(self)
    cdef int event_count_c(self)
    cdef str state_string_c(self)
    cdef str status_string_c(self)
    cdef bint is_buy_c(self) except *
    cdef bint is_sell_c(self) except *
    cdef bint is_working_c(self) except *
    cdef bint is_completed_c(self) except *

    @staticmethod
    cdef inline OrderSide opposite_side_c(OrderSide side) except *

    @staticmethod
    cdef inline OrderSide flatten_side_c(PositionSide side) except *

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
    """The order price (STOP or LIMIT).\n\n:returns: `Price`"""
    cdef readonly LiquiditySide liquidity_side
    """The order liquidity size.\n\n:returns: `LiquiditySide`"""
    cdef readonly datetime expire_time
    """The order expire time (optional).\n\n:returns: `datetime` or `None`"""

    cdef void _set_slippage(self) except *


cdef class MarketOrder(Order):
    @staticmethod
    cdef MarketOrder create(OrderInitialized event)


cdef class StopMarketOrder(PassiveOrder):
    @staticmethod
    cdef StopMarketOrder create(OrderInitialized event)


cdef class LimitOrder(PassiveOrder):
    cdef readonly bint is_post_only
    """If the order is `post_only`, meaning it will only make liquidity.\n\n:returns: `bool`"""
    cdef readonly bint is_hidden
    """If the order is marked to display on the public order book.\n\n:returns: `bool`"""

    @staticmethod
    cdef LimitOrder create(OrderInitialized event)


cdef class BracketOrder:
    cdef readonly BracketOrderId id
    """The bracket order identifier.\n\n:returns: `BracketOrderId`"""
    cdef readonly Order entry
    """The entry order.\n\n:returns: `Order`"""
    cdef readonly StopMarketOrder stop_loss
    """The stop-loss order.\n\n:returns: `StopMarketOrder`"""
    cdef readonly PassiveOrder take_profit
    """The take-profit order (optional).\n\n:returns: `PassiveOrder` or `None`"""
    cdef readonly datetime timestamp
    """If the bracket order has a take-profit.\n\n:returns: `datetime`"""
