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

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.calculators cimport ExchangeRateCalculator
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.cache.database cimport CacheDatabase
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.ticker cimport Ticker
from nautilus_trader.model.enums_c cimport OmsType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport PositionSide
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport Strategy


cdef class Cache(CacheFacade):
    cdef LoggerAdapter _log
    cdef CacheDatabase _database
    cdef ExchangeRateCalculator _xrate_calculator

    cdef dict _general
    cdef dict _xrate_symbols
    cdef dict _tickers
    cdef dict _quote_ticks
    cdef dict _trade_ticks
    cdef dict _order_books
    cdef dict _bars
    cdef dict _bars_bid
    cdef dict _bars_ask
    cdef dict _currencies
    cdef dict _instruments
    cdef dict _accounts
    cdef dict _orders
    cdef dict _order_lists
    cdef dict _positions
    cdef dict _position_snapshots
    cdef dict _submit_order_commands
    cdef dict _submit_order_list_commands

    cdef dict _index_venue_account
    cdef dict _index_venue_orders
    cdef dict _index_venue_positions
    cdef dict _index_order_ids
    cdef dict _index_order_position
    cdef dict _index_order_strategy
    cdef dict _index_position_strategy
    cdef dict _index_position_orders
    cdef dict _index_instrument_orders
    cdef dict _index_instrument_positions
    cdef dict _index_strategy_orders
    cdef dict _index_strategy_positions
    cdef set _index_orders
    cdef set _index_orders_open
    cdef set _index_orders_closed
    cdef set _index_orders_emulated
    cdef set _index_orders_inflight
    cdef set _index_positions
    cdef set _index_positions_open
    cdef set _index_positions_closed
    cdef set _index_actors
    cdef set _index_strategies

    cdef readonly int tick_capacity
    """The caches tick capacity.\n\n:returns: `int`"""
    cdef readonly int bar_capacity
    """The caches bar capacity.\n\n:returns: `int`"""

    cpdef void cache_general(self) except *
    cpdef void cache_currencies(self) except *
    cpdef void cache_instruments(self) except *
    cpdef void cache_accounts(self) except *
    cpdef void cache_orders(self) except *
    cpdef void cache_order_lists(self) except *
    cpdef void cache_positions(self) except *
    cpdef void cache_commands(self) except *
    cpdef void build_index(self) except *
    cpdef bint check_integrity(self) except *
    cpdef bint check_residuals(self) except *
    cpdef void clear_index(self) except *
    cpdef void reset(self) except *
    cpdef void flush_db(self) except *

    cdef tuple _build_quote_table(self, Venue venue)
    cdef void _build_index_venue_account(self) except *
    cdef void _cache_venue_account_id(self, AccountId account_id) except *
    cdef void _build_indexes_from_orders(self) except *
    cdef void _build_indexes_from_positions(self) except *
    cdef set _build_order_query_filter_set(self, Venue venue, InstrumentId instrument_id, StrategyId strategy_id)
    cdef set _build_position_query_filter_set(self, Venue venue, InstrumentId instrument_id, StrategyId strategy_id)
    cdef list _get_orders_for_ids(self, set client_order_ids, OrderSide side)
    cdef list _get_positions_for_ids(self, set position_ids, PositionSide side)

    cpdef Instrument load_instrument(self, InstrumentId instrument_id)
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef void load_actor(self, Actor actor) except *
    cpdef void load_strategy(self, Strategy strategy) except *
    cpdef SubmitOrder load_submit_order_command(self, ClientOrderId client_order_id)
    cpdef SubmitOrderList load_submit_order_list_command(self, OrderListId order_list_id)

    cpdef void add_order_book(self, OrderBook order_book) except *
    cpdef void add_ticker(self, Ticker ticker) except *
    cpdef void add_quote_tick(self, QuoteTick tick) except *
    cpdef void add_trade_tick(self, TradeTick tick) except *
    cpdef void add_bar(self, Bar bar) except *
    cpdef void add_quote_ticks(self, list ticks) except *
    cpdef void add_trade_ticks(self, list ticks) except *
    cpdef void add_bars(self, list bars) except *
    cpdef void add_currency(self, Currency currency) except *
    cpdef void add_instrument(self, Instrument instrument) except *
    cpdef void add_account(self, Account account) except *
    cpdef void add_order(self, Order order, PositionId position_id, bint override=*) except *
    cpdef void add_order_list(self, OrderList order_list) except *
    cpdef void add_position_id(self, PositionId position_id, Venue venue, ClientOrderId client_order_id, StrategyId strategy_id) except *
    cpdef void add_position(self, Position position, OmsType oms_type) except *
    cpdef void snapshot_position(self, Position position) except *
    cpdef void add_submit_order_command(self, SubmitOrder command) except *
    cpdef void add_submit_order_list_command(self, SubmitOrderList command) except *

    cpdef void update_account(self, Account account) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_position(self, Position position) except *
    cpdef void update_actor(self, Actor actor) except *
    cpdef void delete_actor(self, Actor actor) except *
    cpdef void update_strategy(self, Strategy strategy) except *
    cpdef void delete_strategy(self, Strategy strategy) except *
