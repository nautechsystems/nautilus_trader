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

from nautilus_trader.common.component cimport Component
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport TradingCommand
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class RiskEngine(Component):
    cdef Portfolio _portfolio
    cdef ExecutionEngine _exec_engine

    cdef readonly TraderId trader_id
    """The trader identifier associated with the engine.\n\n:returns: `TraderId`"""
    cdef readonly ExecutionCache cache
    """The engines execution cache.\n\n:returns: `ExecutionCache`"""
    cdef readonly int command_count
    """The total count of commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int event_count
    """The total count of events received by the engine.\n\n:returns: `int`"""
    cdef readonly bint block_all_orders
    """If all orders are blocked from being sent.\n\n:returns: `bool`"""

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *
    cpdef void _on_stop(self) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void execute(self, Command command) except *
    cpdef void process(self, Event event) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, Command command) except *
    cdef inline void _handle_trading_command(self, TradingCommand command) except *
    cdef inline void _handle_submit_order(self, SubmitOrder command) except *
    cdef inline void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *
    cdef inline void _handle_update_order(self, UpdateOrder command) except *
    cdef inline void _handle_cancel_order(self, CancelOrder command) except *
    cdef inline void _invalidate_order(self, ClientOrderId client_order_id, str reason) except *
    cdef inline void _invalidate_bracket_order(self, BracketOrder bracket_order) except *

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef inline void _handle_event(self, Event event) except *

# -- RISK MANAGEMENT -------------------------------------------------------------------------------

    cdef list _check_submit_order_risk(self, SubmitOrder command)
    cdef list _check_submit_bracket_order_risk(self, SubmitBracketOrder command)
    cdef void _deny_order(self, Order order, str reason) except *

# -- TEMP ------------------------------------------------------------------------------------------

    cpdef void set_block_all_orders(self, bint value=*) except *
