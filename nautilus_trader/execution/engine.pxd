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
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport TradingCommand
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderEvent
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport PositionChanged
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order.bracket cimport BracketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionEngine(Component):
    cdef dict _clients
    cdef dict _strategies
    cdef PositionIdGenerator _pos_id_generator
    cdef Portfolio _portfolio
    cdef RiskEngine _risk_engine

    cdef readonly TraderId trader_id
    """The trader identifier associated with the engine.\n\n:returns: `TraderId`"""
    cdef readonly ExecutionCache cache
    """The engines execution cache.\n\n:returns: `ExecutionCache`"""
    cdef readonly int command_count
    """The total count of commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int event_count
    """The total count of events received by the engine.\n\n:returns: `int`"""

    cpdef int position_id_count(self, StrategyId strategy_id) except *
    cpdef bint check_portfolio_equal(self, Portfolio portfolio) except *
    cpdef bint check_integrity(self) except *
    cpdef bint check_connected(self) except *
    cpdef bint check_disconnected(self) except *
    cpdef bint check_residuals(self) except *

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient client) except *
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef void register_risk_engine(self, RiskEngine engine) except *
    cpdef void deregister_client(self, ExecutionClient client) except *
    cpdef void deregister_strategy(self, TradingStrategy strategy) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *
    cpdef void _on_stop(self) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void load_cache(self) except *
    cpdef void execute(self, TradingCommand command) except *
    cpdef void process(self, Event event) except *
    cpdef void flush_db(self) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, TradingCommand command) except *
    cdef inline void _handle_submit_order(self, ExecutionClient client, SubmitOrder command) except *
    cdef inline void _handle_submit_bracket_order(self, ExecutionClient client, SubmitBracketOrder command) except *
    cdef inline void _handle_amend_order(self, ExecutionClient client, AmendOrder command) except *
    cdef inline void _handle_cancel_order(self, ExecutionClient client, CancelOrder command) except *
    cdef inline void _invalidate_order(self, ClientOrderId cl_ord_id, str reason) except *
    cdef inline void _invalidate_bracket_order(self, BracketOrder bracket_order) except *

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef inline void _handle_event(self, Event event) except *
    cdef inline void _handle_account_event(self, AccountState event) except *
    cdef inline void _handle_position_event(self, PositionEvent event) except *
    cdef inline void _handle_order_event(self, OrderEvent event) except *
    cdef inline void _confirm_strategy_id(self, OrderFilled fill) except *
    cdef inline void _confirm_position_id(self, OrderFilled fill) except *
    cdef inline void _handle_order_cancel_reject(self, OrderCancelReject event) except *
    cdef inline void _handle_order_fill(self, OrderFilled event) except *
    cdef inline void _open_position(self, OrderFilled event) except *
    cdef inline void _update_position(self, Position position, OrderFilled event) except *
    cdef inline void _flip_position(self, Position position, OrderFilled fill) except *
    cdef inline PositionOpened _pos_opened_event(self, Position position, OrderFilled fill)
    cdef inline PositionChanged _pos_changed_event(self, Position position, OrderFilled fill)
    cdef inline PositionClosed _pos_closed_event(self, Position position, OrderFilled fill)
    cdef inline void _send_to_strategy(self, Event event, StrategyId strategy_id) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _set_position_id_counts(self) except *
