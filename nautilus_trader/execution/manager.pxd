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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID4
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
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class OrderManager:
    cdef Clock _clock
    cdef Logger _log
    cdef MessageBus _msgbus
    cdef Cache _cache

    cdef readonly bint active_local
    cdef readonly bint debug
    cdef readonly bint log_events
    cdef readonly bint log_commands

    cdef dict _submit_order_commands
    cdef object _submit_order_handler
    cdef object _cancel_order_handler
    cdef object _modify_order_handler

    cpdef dict get_submit_order_commands(self)
    cpdef void cache_submit_order_command(self, SubmitOrder command)
    cpdef SubmitOrder pop_submit_order_command(self, ClientOrderId client_order_id)
    cpdef void reset(self)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void cancel_order(self, Order order)
    cpdef void modify_order_quantity(self, Order order, Quantity new_quantity)
    cpdef void create_new_submit_order(self, Order order, PositionId position_id=*, ClientId client_id=*)
    cpdef bint should_manage_order(self, Order order)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void handle_event(self, Event event)
    cpdef void handle_order_rejected(self, OrderRejected rejected)
    cpdef void handle_order_canceled(self, OrderCanceled canceled)
    cpdef void handle_order_expired(self, OrderExpired expired)
    cpdef void handle_order_updated(self, OrderUpdated updated)
    cpdef void handle_order_filled(self, OrderFilled filled)
    cpdef void handle_contingencies(self, Order order)
    cpdef void handle_contingencies_update(self, Order order)
    cpdef void handle_position_event(self, PositionEvent event)

# -- EGRESS ---------------------------------------------------------------------------------------

    cpdef void send_emulator_command(self, TradingCommand command)
    cpdef void send_algo_command(self, TradingCommand command, ExecAlgorithmId exec_algorithm_id)
    cpdef void send_risk_command(self, TradingCommand command)
    cpdef void send_exec_command(self, TradingCommand command)
    cpdef void send_risk_event(self, OrderEvent event)
    cpdef void send_exec_event(self, OrderEvent event)
