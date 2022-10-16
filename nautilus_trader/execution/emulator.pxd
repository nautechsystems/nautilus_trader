# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved. https://nautechsystems.io
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
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.orders.base cimport Order


cdef class OrderEmulator(Actor):
    cdef dict _commands
    cdef dict _matching_cores

    cdef set _subscribed_quotes
    cdef set _subscribed_trades

    cpdef void execute(self, TradingCommand command) except *
    cdef void _handle_submit_order(self, SubmitOrder command) except *
    cdef void _handle_modify_order(self, ModifyOrder command) except *
    cdef void _handle_cancel_order(self, CancelOrder command) except *
    cdef void _handle_cancel_all_orders(self, CancelAllOrders command) except *

    cdef void _cancel_order(self, MatchingCore matching_core, Order order) except *

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void trigger_stop_order(self, Order order) except *
    cpdef void fill_market_order(self, Order order, LiquiditySide liquidity_side) except *
    cpdef void fill_limit_order(self, Order order, LiquiditySide liquidity_side) except *

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_risk_command(self, TradingCommand command) except *
    cdef void _send_exec_command(self, TradingCommand command) except *
    cdef void _send_exec_event(self, OrderEvent event) except*
