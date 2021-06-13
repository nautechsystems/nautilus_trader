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

from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCanceled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderEvent
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderTriggered
from nautilus_trader.model.events cimport OrderUpdated
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Order:
    cdef list _events
    cdef list _execution_ids
    cdef FiniteStateMachine _fsm
    cdef OrderState _rollback_state

    cdef readonly ClientOrderId client_order_id
    """The client order identifier.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue assigned order identifier.\n\n:returns: `VenueOrderId`"""
    cdef readonly PositionId position_id
    """The position identifier associated with the order.\n\n:returns: `PositionId`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the order.\n\n:returns: `StrategyId`"""
    cdef readonly AccountId account_id
    """The account identifier associated with the order.\n\n:returns: `AccountId` or None"""
    cdef readonly ExecutionId execution_id
    """The orders last execution identifier.\n\n:returns: `ExecutionId` or None"""
    cdef readonly InstrumentId instrument_id
    """The order instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly OrderSide side
    """The order side.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType type
    """The order type.\n\n:returns: `OrderType`"""
    cdef readonly Quantity quantity
    """The order quantity.\n\n:returns: `Quantity`"""
    cdef readonly int64_t timestamp_ns
    """The UNIX timestamp (nanos) of order initialization.\n\n:returns: `int64`"""
    cdef readonly TimeInForce time_in_force
    """The order time-in-force.\n\n:returns: `TimeInForce`"""
    cdef readonly Quantity filled_qty
    """The order total filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly int64_t ts_filled_ns
    """The UNIX timestamp (nanos) of the last execution (0 for no execution).\n\n:returns: `int64`"""
    cdef readonly object avg_px
    """The order average fill price.\n\n:returns: `Decimal` or None"""
    cdef readonly object slippage
    """The order total price slippage.\n\n:returns: `Decimal`"""
    cdef readonly UUID init_id
    """The identifier of the `OrderInitialized` event.\n\n:returns: `UUID`"""

    cpdef dict to_dict(self)

    cdef OrderState state_c(self) except *
    cdef OrderInitialized init_event_c(self)
    cdef OrderEvent last_event_c(self)
    cdef list events_c(self)
    cdef list execution_ids_c(self)
    cdef int event_count_c(self) except *
    cdef str state_string_c(self)
    cdef str status_string_c(self)
    cdef bint is_buy_c(self) except *
    cdef bint is_sell_c(self) except *
    cdef bint is_passive_c(self) except *
    cdef bint is_aggressive_c(self) except *
    cdef bint is_working_c(self) except *
    cdef bint is_completed_c(self) except *

    @staticmethod
    cdef OrderSide opposite_side_c(OrderSide side) except *

    @staticmethod
    cdef OrderSide flatten_side_c(PositionSide side) except *

    cpdef void apply(self, OrderEvent event) except *

    cdef void _invalid(self, OrderInvalid event) except *
    cdef void _denied(self, OrderDenied event) except *
    cdef void _submitted(self, OrderSubmitted event) except *
    cdef void _rejected(self, OrderRejected event) except *
    cdef void _accepted(self, OrderAccepted event) except *
    cdef void _updated(self, OrderUpdated event) except *
    cdef void _canceled(self, OrderCanceled event) except *
    cdef void _expired(self, OrderExpired event) except *
    cdef void _triggered(self, OrderTriggered event) except *
    cdef void _filled(self, OrderFilled event) except *
    cdef object _calculate_avg_px(self, Quantity last_qty, Price last_px)


cdef class PassiveOrder(Order):
    cdef list _venue_order_ids

    cdef readonly Price price
    """The order price (STOP or LIMIT).\n\n:returns: `Price`"""
    cdef readonly LiquiditySide liquidity_side
    """The order liquidity side.\n\n:returns: `LiquiditySide`"""
    cdef readonly datetime expire_time
    """The order expire time.\n\n:returns: `datetime` or None"""
    cdef readonly int64_t expire_time_ns
    """The order expire time (nanoseconds), zero for no expire time.\n\n:returns: `int64`"""

    cdef list venue_order_ids_c(self)

    cdef void _set_slippage(self) except *
