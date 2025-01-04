# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.actor cimport Actor
from nautilus_trader.execution.manager cimport OrderManager
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class OrderEmulator(Actor):
    cdef OrderManager _manager
    cdef dict[InstrumentId, MatchingCore] _matching_cores

    cdef set[InstrumentId] _subscribed_quotes
    cdef set[InstrumentId] _subscribed_trades
    cdef set[StrategyId] _subscribed_strategies
    cdef set[PositionId] _monitored_positions

    cdef readonly bint debug
    """If debug mode is active (will provide extra debug logging).\n\n:returns: `bool`"""
    cdef readonly int command_count
    """The total count of commands received by the emulator.\n\n:returns: `int`"""
    cdef readonly int event_count
    """The total count of events received by the emulator.\n\n:returns: `int`"""

    cpdef void execute(self, TradingCommand command)
    cpdef MatchingCore create_matching_core(self, InstrumentId instrument_id, Price price_increment)
    cdef void _handle_submit_order(self, SubmitOrder command)
    cdef void _handle_submit_order_list(self, SubmitOrderList command)
    cdef void _handle_modify_order(self, ModifyOrder command)
    cdef void _handle_cancel_order(self, CancelOrder command)
    cdef void _handle_cancel_all_orders(self, CancelAllOrders command)

    cpdef void _check_monitoring(self, StrategyId strategy_id, PositionId position_id)
    cpdef void _cancel_order(self, Order order)
    cpdef void _update_order(self, Order order, Quantity new_quantity)

# -------------------------------------------------------------------------------------------------

    cpdef void _trigger_stop_order(self, Order order)
    cpdef void _fill_market_order(self, Order order)
    cpdef void _fill_limit_order(self, Order order)

    cdef void _iterate_orders(self, MatchingCore matching_core)
    cdef void _update_trailing_stop_order(self, MatchingCore matching_core, Order order)
