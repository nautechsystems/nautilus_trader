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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.core.message cimport Event
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport SubmitBracketOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.commands.trading cimport TradingCommand
from nautilus_trader.model.commands.trading cimport UpdateOrder
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.message_bus cimport MessageBus


cdef class ExecutionEngine(Component):
    cdef dict _clients
    cdef dict _routing_map
    cdef ExecutionClient _default_client
    cdef PositionIdGenerator _pos_id_generator
    cdef MessageBus _msgbus

    cdef readonly TraderId trader_id
    """The trader ID associated with the engine.\n\n:returns: `TraderId`"""
    cdef readonly Cache cache
    """The engines cache.\n\n:returns: `Cache`"""
    cdef readonly int command_count
    """The total count of commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int event_count
    """The total count of events received by the engine.\n\n:returns: `int`"""

    cpdef int position_id_count(self, StrategyId strategy_id) except *
    cpdef bint check_integrity(self) except *
    cpdef bint check_connected(self) except *
    cpdef bint check_disconnected(self) except *
    cpdef bint check_residuals(self) except *

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient client) except *
    cpdef void register_default_client(self, ExecutionClient client) except *
    cpdef void register_venue_routing(self, ExecutionClient client, Venue venue) except *
    cpdef void deregister_client(self, ExecutionClient client) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *
    cpdef void _on_stop(self) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef void _set_position_id_counts(self) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void load_cache(self) except *
    cpdef void execute(self, TradingCommand command) except *
    cpdef void process(self, Event event) except *
    cpdef void flush_db(self) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef void _execute_command(self, TradingCommand command) except *
    cdef void _handle_submit_order(self, ExecutionClient client, SubmitOrder command) except *
    cdef void _handle_submit_bracket_order(self, ExecutionClient client, SubmitBracketOrder command) except *
    cdef void _handle_update_order(self, ExecutionClient client, UpdateOrder command) except *
    cdef void _handle_cancel_order(self, ExecutionClient client, CancelOrder command) except *

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *
    cdef void _handle_account_event(self, AccountState event) except *
    cdef void _handle_order_event(self, OrderEvent event) except *
    cdef void _confirm_position_id(self, OrderFilled fill) except *
    cdef void _handle_order_fill(self, OrderFilled fill) except *
    cdef void _open_position(self, OrderFilled fill) except *
    cdef void _update_position(self, Position position, OrderFilled fill) except *
    cdef void _flip_position(self, Position position, OrderFilled fill) except *
