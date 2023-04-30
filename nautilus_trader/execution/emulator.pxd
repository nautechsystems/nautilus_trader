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

from nautilus_trader.common.actor cimport Actor
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
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class OrderEmulator(Actor):
    cdef dict _matching_cores
    cdef dict _commands_submit_order
    cdef dict _commands_submit_order_list

    cdef set _subscribed_quotes
    cdef set _subscribed_trades
    cdef set _subscribed_strategies
    cdef set _monitored_positions

    cpdef void execute(self, TradingCommand command)
    cpdef MatchingCore create_matching_core(self, Instrument instrument)
    cdef void _handle_submit_order(self, SubmitOrder command)
    cdef void _handle_submit_order_list(self, SubmitOrderList command)
    cdef void _handle_modify_order(self, ModifyOrder command)
    cdef void _handle_cancel_order(self, CancelOrder command)
    cdef void _handle_cancel_all_orders(self, CancelAllOrders command)

    cdef void _create_new_submit_order(self, Order order, PositionId position_id, ClientId client_id)
    cdef void _cancel_order(self, MatchingCore matching_core, Order order)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cdef void _handle_order_rejected(self, OrderRejected rejected)
    cdef void _handle_order_canceled(self, OrderCanceled canceled)
    cdef void _handle_order_expired(self, OrderExpired expired)
    cdef void _handle_order_updated(self, OrderUpdated updated)
    cdef void _handle_order_filled(self, OrderFilled filled)
    cdef void _handle_position_event(self, PositionEvent event)
    cdef void _handle_contingencies(self, Order order)
    cdef void _update_order_quantity(self, Order order, Quantity new_quantity)

# -------------------------------------------------------------------------------------------------

    cpdef void _trigger_stop_order(self, Order order)
    cpdef void _fill_market_order(self, Order order)
    cpdef void _fill_limit_order(self, Order order)

    cdef void _iterate_orders(self, MatchingCore matching_core)
    cdef void _update_trailing_stop_order(self, MatchingCore matching_core, Order order)

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_algo_command(self, TradingCommand command)
    cdef void _send_risk_command(self, TradingCommand command)
    cdef void _send_exec_command(self, TradingCommand command)
    cdef void _send_risk_event(self, OrderEvent event)
    cdef void _send_exec_event(self, OrderEvent event)
