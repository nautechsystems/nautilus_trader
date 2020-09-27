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

from nautilus_trader.core.decimal cimport Decimal64
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderType
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
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Order:
    cdef list _execution_ids
    cdef list _events
    cdef FiniteStateMachine _fsm

    cdef readonly ClientOrderId cl_ord_id
    cdef readonly OrderId id
    cdef readonly AccountId account_id
    cdef readonly ExecutionId execution_id
    cdef readonly PositionId position_id

    cdef readonly Symbol symbol
    cdef readonly OrderSide side
    cdef readonly OrderType type
    cdef readonly Quantity quantity
    cdef readonly datetime timestamp
    cdef readonly TimeInForce time_in_force
    cdef readonly Quantity filled_quantity
    cdef readonly datetime filled_timestamp
    cdef readonly Price average_price
    cdef readonly Decimal64 slippage
    cdef readonly UUID init_id

    cpdef bint equals(self, Order other)
    cpdef OrderState state(self)
    cpdef Event last_event(self)
    cpdef list get_execution_ids(self)
    cpdef list get_events(self)
    cpdef int event_count(self)
    cpdef bint is_buy(self)
    cpdef bint is_sell(self)
    cpdef bint is_working(self)
    cpdef bint is_completed(self)
    cpdef str status_string(self)
    cpdef str state_as_string(self)
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
    cdef readonly LiquiditySide liquidity_side
    cdef readonly datetime expire_time

    cdef void _set_slippage(self) except *


cdef class MarketOrder(Order):
    @staticmethod
    cdef MarketOrder create(OrderInitialized event)


cdef class StopOrder(PassiveOrder):
    @staticmethod
    cdef StopOrder create(OrderInitialized event)


cdef class LimitOrder(PassiveOrder):
    cdef readonly bint is_post_only
    cdef readonly bint is_hidden

    @staticmethod
    cdef LimitOrder create(OrderInitialized event)


cdef class BracketOrder:
    cdef readonly BracketOrderId id
    cdef readonly Order entry
    cdef readonly StopOrder stop_loss
    cdef readonly PassiveOrder take_profit
    cdef readonly bint has_take_profit
    cdef readonly datetime timestamp

    cpdef bint equals(self, BracketOrder other)
