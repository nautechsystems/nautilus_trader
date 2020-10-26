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
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
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

    cdef ClientOrderId _cl_ord_id
    cdef StrategyId _strategy_id
    cdef OrderId _id
    cdef AccountId _account_id
    cdef ExecutionId _execution_id
    cdef PositionId _position_id
    cdef Symbol _symbol
    cdef OrderSide _side
    cdef OrderType _type
    cdef Quantity _quantity
    cdef datetime _timestamp
    cdef TimeInForce _time_in_force
    cdef Quantity _filled_qty
    cdef datetime _filled_timestamp
    cdef Decimal _avg_price
    cdef Decimal _slippage
    cdef UUID _init_id

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
    cdef Price _price
    cdef LiquiditySide _liquidity_side
    cdef datetime _expire_time

    cdef void _set_slippage(self) except *


cdef class MarketOrder(Order):
    @staticmethod
    cdef MarketOrder create(OrderInitialized event)


cdef class StopMarketOrder(PassiveOrder):
    @staticmethod
    cdef StopMarketOrder create(OrderInitialized event)


cdef class LimitOrder(PassiveOrder):
    cdef bint _is_post_only
    cdef bint _is_hidden

    @staticmethod
    cdef LimitOrder create(OrderInitialized event)


cdef class BracketOrder:
    cdef BracketOrderId _id
    cdef Order _entry
    cdef StopMarketOrder _stop_loss
    cdef PassiveOrder _take_profit
    cdef datetime _timestamp
