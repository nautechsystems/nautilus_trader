# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t

from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport OrderStatus
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport PositionSide
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderSubmitted
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef tuple VALID_STOP_ORDER_TYPES
cdef tuple VALID_LIMIT_ORDER_TYPES


cdef class Order:
    cdef list _events
    cdef list _venue_order_ids
    cdef list _trade_ids
    cdef FiniteStateMachine _fsm
    cdef OrderStatus _previous_status
    cdef Price _triggered_price

    cdef readonly TraderId trader_id
    """The trader ID associated with the position.\n\n:returns: `TraderId`"""
    cdef readonly StrategyId strategy_id
    """The strategy ID associated with the order.\n\n:returns: `StrategyId`"""
    cdef readonly InstrumentId instrument_id
    """The order instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly ClientOrderId client_order_id
    """The client order ID.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue assigned order ID.\n\n:returns: `VenueOrderId`"""
    cdef readonly PositionId position_id
    """The position ID associated with the order.\n\n:returns: `PositionId`"""
    cdef readonly AccountId account_id
    """The account ID associated with the order.\n\n:returns: `AccountId` or ``None``"""
    cdef readonly TradeId last_trade_id
    """The orders last trade match ID.\n\n:returns: `TradeId` or ``None``"""
    cdef readonly OrderSide side
    """The order side.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType order_type
    """The order type.\n\n:returns: `OrderType`"""
    cdef readonly TimeInForce time_in_force
    """The order time in force.\n\n:returns: `TimeInForce`"""
    cdef readonly LiquiditySide liquidity_side
    """The order liquidity side.\n\n:returns: `LiquiditySide`"""
    cdef readonly bint is_post_only
    """If the order will only provide liquidity (make a market).\n\n:returns: `bool`"""
    cdef readonly bint is_reduce_only
    """If the order carries the 'reduce-only' execution instruction.\n\n:returns: `bool`"""
    cdef readonly Quantity quantity
    """The order quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity filled_qty
    """The order total filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity leaves_qty
    """The order total leaves quantity.\n\n:returns: `Quantity`"""
    cdef readonly double avg_px
    """The order average fill price.\n\n:returns: `double`"""
    cdef readonly double slippage
    """The order total price slippage.\n\n:returns: `double`"""
    cdef readonly TriggerType emulation_trigger
    """The order emulation trigger type.\n\n:returns: `TriggerType`"""
    cdef readonly ContingencyType contingency_type
    """The orders contingency type.\n\n:returns: `ContingencyType`"""
    cdef readonly OrderListId order_list_id
    """The order list ID associated with the order.\n\n:returns: `OrderListId` or ``None``"""
    cdef readonly list linked_order_ids
    """The orders linked client order ID(s).\n\n:returns: `list[ClientOrderId]` or ``None``"""
    cdef readonly ClientOrderId parent_order_id
    """The parent client order ID.\n\n:returns: `ClientOrderId` or ``None``"""
    cdef readonly str tags
    """The order custom user tags.\n\n:returns: `str` or ``None``"""
    cdef readonly UUID4 init_id
    """The event ID of the `OrderInitialized` event.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """The UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_last
    """The UNIX timestamp (nanoseconds) when the last fill occurred (0 for no fill).\n\n:returns: `uint64_t`"""

    cpdef str info(self)
    cpdef dict to_dict(self)

    cdef void set_triggered_price_c(self, Price triggered_price) except *
    cdef Price get_triggered_price_c(self)
    cdef OrderStatus status_c(self) except *
    cdef OrderInitialized init_event_c(self)
    cdef OrderEvent last_event_c(self)
    cdef list events_c(self)
    cdef list venue_order_ids_c(self)
    cdef list trade_ids_c(self)
    cdef int event_count_c(self) except *
    cdef str status_string_c(self)
    cdef str type_string_c(self)
    cdef str side_string_c(self)
    cdef str tif_string_c(self)
    cdef bint has_price_c(self) except *
    cdef bint has_trigger_price_c(self) except *
    cdef bint is_buy_c(self) except *
    cdef bint is_sell_c(self) except *
    cdef bint is_passive_c(self) except *
    cdef bint is_aggressive_c(self) except *
    cdef bint is_emulated_c(self) except *
    cdef bint is_contingency_c(self) except *
    cdef bint is_parent_order_c(self) except *
    cdef bint is_child_order_c(self) except *
    cdef bint is_open_c(self) except *
    cdef bint is_canceled_c(self) except *
    cdef bint is_closed_c(self) except *
    cdef bint is_inflight_c(self) except *
    cdef bint is_pending_update_c(self) except *
    cdef bint is_pending_cancel_c(self) except *

    @staticmethod
    cdef OrderSide opposite_side_c(OrderSide side) except *

    @staticmethod
    cdef OrderSide closing_side_c(PositionSide position_side) except *

    cpdef bint would_reduce_only(self, PositionSide position_side, Quantity position_qty) except *

    cpdef void apply(self, OrderEvent event) except *

    cdef void _denied(self, OrderDenied event) except *
    cdef void _submitted(self, OrderSubmitted event) except *
    cdef void _rejected(self, OrderRejected event) except *
    cdef void _accepted(self, OrderAccepted event) except *
    cdef void _updated(self, OrderUpdated event) except *
    cdef void _triggered(self, OrderTriggered event) except *
    cdef void _canceled(self, OrderCanceled event) except *
    cdef void _expired(self, OrderExpired event) except *
    cdef void _filled(self, OrderFilled event) except *
    cdef double _calculate_avg_px(self, double last_qty, double last_px)
    cdef void _set_slippage(self) except *

    @staticmethod
    cdef void _hydrate_initial_events(Order original, Order transformed) except *
