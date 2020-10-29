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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.commands cimport Connect
from nautilus_trader.common.commands cimport Disconnect
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport Command
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderEvent
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionEngine:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef TraderId _trader_id
    cdef ExecutionCache _cache
    cdef Portfolio _portfolio
    cdef PositionIdGenerator _pos_id_generator
    cdef dict _clients
    cdef dict _strategies
    cdef int _command_count
    cdef int _event_count

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient exec_client) except *
    cpdef void deregister_client(self, ExecutionClient exec_client) except *
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef void deregister_strategy(self, TradingStrategy strategy) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void load_cache(self) except *
    cpdef void integrity_check(self) except *
    cpdef void _set_position_symbol_counts(self) except *
    cpdef void execute(self, Command command) except *
    cpdef void process(self, Event event) except *
    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void flush_db(self) except *

# -- COMMAND-HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, Command command) except *
    cdef inline void _handle_connect(self, Connect command) except *
    cdef inline void _handle_disconnect(self, Disconnect command) except *
    cdef inline void _handle_submit_order(self, SubmitOrder command) except *
    cdef inline void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *
    cdef inline void _handle_modify_order(self, ModifyOrder command) except *
    cdef inline void _handle_cancel_order(self, CancelOrder command) except *
    cdef inline void _invalidate_order(self, Order order, str reason) except *
    cdef inline void _deny_order(self, Order order, str reason) except *

# -- EVENT-HANDLERS --------------------------------------------------------------------------------

    cdef inline void _handle_event(self, Event event) except *
    cdef inline void _handle_account_event(self, AccountState event) except *
    cdef inline void _handle_position_event(self, PositionEvent event) except *
    cdef inline void _handle_order_event(self, OrderEvent event) except *
    cdef inline void _handle_order_cancel_reject(self, OrderCancelReject event) except *
    cdef inline void _handle_order_fill(self, OrderFilled event) except *
    cdef inline void _fill_system_assigned_ids(self, PositionId position_id, OrderFilled fill, StrategyId strategy_id) except *
    cdef inline void _fill_exchange_assigned_ids(self, PositionId position_id, OrderFilled fill, StrategyId strategy_id) except *
    cdef inline void _open_position(self, OrderFilled event) except *
    cdef inline void _update_position(self, OrderFilled event) except *
    cdef inline void _flip_position(self, Position position, OrderFilled fill) except *
    cdef inline PositionOpened _pos_opened_event(self, Position position, OrderFilled fill)
    cdef inline PositionModified _pos_modified_event(self, Position position, OrderFilled fill)
    cdef inline PositionClosed _pos_closed_event(self, Position position, OrderFilled fill)
    cdef inline void _send_to_strategy(self, Event event, StrategyId strategy_id) except *
