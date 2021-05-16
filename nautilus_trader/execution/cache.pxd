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

from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.execution.base cimport ExecutionCacheFacade
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionCache(ExecutionCacheFacade):
    cdef LoggerAdapter _log
    cdef ExecutionDatabase _database
    cdef dict _cached_currencies
    cdef dict _cached_instruments
    cdef dict _cached_accounts
    cdef dict _cached_orders
    cdef dict _cached_positions

    cdef dict _index_venue_account
    cdef dict _index_venue_order_ids
    cdef dict _index_order_position
    cdef dict _index_order_strategy
    cdef dict _index_position_strategy
    cdef dict _index_position_orders
    cdef dict _index_instrument_orders
    cdef dict _index_instrument_positions
    cdef dict _index_strategy_orders
    cdef dict _index_strategy_positions
    cdef set _index_orders
    cdef set _index_orders_working
    cdef set _index_orders_completed
    cdef set _index_positions
    cdef set _index_positions_open
    cdef set _index_positions_closed
    cdef set _index_strategies

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void cache_currencies(self) except *
    cpdef void cache_instruments(self) except *
    cpdef void cache_accounts(self) except *
    cpdef void cache_orders(self) except *
    cpdef void cache_positions(self) except *
    cpdef void build_index(self) except *
    cpdef bint check_integrity(self) except *
    cpdef bint check_residuals(self) except *
    cpdef void reset(self) except *
    cpdef void clear_cache(self) except *
    cpdef void clear_index(self) except *
    cpdef void flush_db(self) except *

    cpdef Instrument load_instrument(self, InstrumentId instrument_id)
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef void load_strategy(self, TradingStrategy strategy) except *
    cpdef void delete_strategy(self, TradingStrategy strategy) except *

    cpdef void add_currency(self, Currency currency) except *
    cpdef void add_instrument(self, Instrument instrument) except *
    cpdef void add_account(self, Account account) except *
    cpdef void add_order(self, Order order, PositionId position_id) except *
    cpdef void add_position_id(self, PositionId position_id, ClientOrderId client_order_id, StrategyId strategy_id) except *
    cpdef void add_position(self, Position position) except *

    cpdef void update_account(self, Account account) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_position(self, Position position) except *
    cpdef void update_strategy(self, TradingStrategy strategy) except *

    cdef void _build_index_venue_account(self) except *
    cdef void _cache_venue_account_id(self, AccountId account_id) except *
    cdef void _build_indexes_from_orders(self) except *
    cdef void _build_indexes_from_positions(self) except *
    cdef inline set _build_ord_query_filter_set(self, InstrumentId instrument_id, StrategyId strategy_id)
    cdef inline set _build_pos_query_filter_set(self, InstrumentId instrument_id, StrategyId strategy_id)
