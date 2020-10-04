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
from nautilus_trader.execution.base cimport ExecutionCacheReadOnly
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Decimal
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionCache(ExecutionCacheReadOnly):
    cdef LoggerAdapter _log
    cdef ExecutionDatabase _database
    cdef dict _cached_accounts
    cdef dict _cached_orders
    cdef dict _cached_positions

    cdef dict _index_order_position
    cdef dict _index_order_strategy
    cdef dict _index_position_strategy
    cdef dict _index_position_orders
    cdef dict _index_symbol_orders
    cdef dict _index_symbol_positions
    cdef dict _index_strategy_orders
    cdef dict _index_strategy_positions
    cdef set _index_orders
    cdef set _index_orders_working
    cdef set _index_orders_completed
    cdef set _index_stop_loss_ids
    cdef set _index_take_profit_ids
    cdef set _index_positions
    cdef set _index_positions_open
    cdef set _index_positions_closed
    cdef set _index_strategies

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void cache_accounts(self) except *
    cpdef void cache_orders(self) except *
    cpdef void cache_positions(self) except *
    cpdef void build_index(self) except *
    cdef void _build_indexes_from_orders(self) except *
    cdef void _build_indexes_from_positions(self) except *
    cdef void _build_indexes_from_registered_order_ids(self) except *
    cpdef void integrity_check(self) except *
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef void load_strategy(self, TradingStrategy strategy)
    cpdef void delete_strategy(self, TradingStrategy strategy) except *

    cpdef void add_account(self, Account account) except *
    cpdef void add_order(self, Order order, PositionId position_id, StrategyId strategy_id) except *
    cpdef void add_stop_loss_id(self, ClientOrderId cl_ord_id) except *
    cpdef void add_take_profit_id(self, ClientOrderId cl_ord_id) except *
    cpdef void add_position_id(self, PositionId position_id, ClientOrderId cl_ord_id, StrategyId strategy_id) except *
    cpdef void add_position(self, Position position, StrategyId strategy_id) except *

    cpdef void update_account(self, Account account) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_position(self, Position position) except *
    cpdef void update_strategy(self, TradingStrategy strategy) except *

    cpdef void discard_stop_loss_id(self, ClientOrderId cl_ord_id) except *
    cpdef void discard_take_profit_id(self, ClientOrderId cl_ord_id) except *
    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *
    cpdef void flush_db(self) except *
    cdef void _clear_indexes(self) except *

    cdef inline set _build_ord_query_filter_set(self, Symbol symbol, StrategyId strategy_id)
    cdef inline set _build_pos_query_filter_set(self, Symbol symbol, StrategyId strategy_id)
    cdef inline Decimal _sum_net_position(self, Symbol symbol, StrategyId strategy_id)
