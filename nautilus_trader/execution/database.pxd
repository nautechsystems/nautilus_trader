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

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionDatabase:
    cdef LoggerAdapter _log
    cdef dict _cached_accounts
    cdef dict _cached_orders
    cdef dict _cached_positions

    cdef readonly TraderId trader_id

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void add_account(self, Account account) except *
    cpdef void add_order(self, Order order, PositionId position_id, StrategyId strategy_id) except *
    cpdef void add_position(self, Position position, StrategyId strategy_id) except *
    cdef void index_position_id(self, PositionId position_id, ClientOrderId cl_ord_id, StrategyId strategy_id) except *
    cpdef void update_account(self, Account account) except *
    cpdef void update_strategy(self, TradingStrategy strategy) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_position(self, Position position) except *
    cpdef void load_strategy(self, TradingStrategy strategy) except *
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef void delete_strategy(self, TradingStrategy strategy) except *
    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *
    cpdef void flush(self) except *
    cdef void _reset(self) except *

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef dict get_symbol_position_counts(self)
    cpdef Account get_account(self, AccountId account_id)
    cpdef set get_strategy_ids(self)
    cpdef set get_order_ids(self, StrategyId strategy_id=*)
    cpdef set get_order_working_ids(self, StrategyId strategy_id=*)
    cpdef set get_order_completed_ids(self, StrategyId strategy_id=*)
    cpdef set get_position_ids(self, StrategyId strategy_id=*)
    cpdef set get_position_open_ids(self, StrategyId strategy_id=*)
    cpdef set get_position_closed_ids(self, StrategyId strategy_id=*)
    cpdef StrategyId get_strategy_for_order(self, ClientOrderId cl_ord_id)
    cpdef StrategyId get_strategy_for_position(self, PositionId position_id)
    cpdef Order get_order(self, ClientOrderId cl_ord_id)
    cpdef dict get_orders(self, StrategyId strategy_id=*)
    cpdef dict get_orders_working(self, StrategyId strategy_id=*)
    cpdef dict get_orders_completed(self, StrategyId strategy_id=*)
    cpdef Position get_position(self, PositionId position_id)
    cpdef PositionId get_position_id(self, ClientOrderId cl_ord_id)
    cpdef dict get_positions(self, StrategyId strategy_id=*)
    cpdef dict get_positions_open(self, StrategyId strategy_id=*)
    cpdef dict get_positions_closed(self, StrategyId strategy_id=*)
    cpdef bint order_exists(self, ClientOrderId cl_ord_id)
    cpdef bint is_order_working(self, ClientOrderId cl_ord_id)
    cpdef bint is_order_completed(self, ClientOrderId cl_ord_id)
    cpdef int orders_total_count(self, StrategyId strategy_id=*)
    cpdef int orders_working_count(self, StrategyId strategy_id=*)
    cpdef int orders_completed_count(self, StrategyId strategy_id=*)
    cpdef bint position_exists(self, PositionId position_id)
    cpdef bint position_exists_for_order(self, ClientOrderId cl_ord_id)
    cpdef bint position_indexed_for_order(self, ClientOrderId cl_ord_id)
    cpdef bint is_position_open(self, PositionId position_id)
    cpdef bint is_position_closed(self, PositionId position_id)
    cpdef int positions_total_count(self, StrategyId strategy_id=*)
    cpdef int positions_open_count(self, StrategyId strategy_id=*)
    cpdef int positions_closed_count(self, StrategyId strategy_id=*)

# -------------------------------------------------------------------------------------------------"

cdef class InMemoryExecutionDatabase(ExecutionDatabase):
    cdef set _strategies
    cdef dict _index_order_position
    cdef dict _index_order_strategy
    cdef dict _index_position_strategy
    cdef dict _index_position_orders
    cdef dict _index_strategy_orders
    cdef dict _index_strategy_positions
    cdef set _index_orders
    cdef set _index_orders_working
    cdef set _index_orders_completed
    cdef set _index_positions
    cdef set _index_positions_open
    cdef set _index_positions_closed
