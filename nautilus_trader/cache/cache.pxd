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

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

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
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
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
    cdef dict _synthetics
    cdef dict _accounts
    cdef dict _orders
    cdef dict _order_lists
    cdef dict _positions
    cdef dict _position_snapshots

    cdef dict _index_venue_account
    cdef dict _index_venue_orders
    cdef dict _index_venue_positions
    cdef dict _index_order_ids
    cdef dict _index_order_position
    cdef dict _index_order_strategy
    cdef dict _index_order_client
    cdef dict _index_position_strategy
    cdef dict _index_position_orders
    cdef dict _index_instrument_orders
    cdef dict _index_instrument_positions
    cdef dict _index_strategy_orders
    cdef dict _index_strategy_positions
    cdef dict _index_exec_algorithm_orders
    cdef dict _index_exec_spawn_orders
    cdef set _index_orders
    cdef set _index_orders_open
    cdef set _index_orders_closed
    cdef set _index_orders_emulated
    cdef set _index_orders_inflight
    cdef set _index_orders_pending_cancel
    cdef set _index_positions
    cdef set _index_positions_open
    cdef set _index_positions_closed
    cdef set _index_actors
    cdef set _index_strategies
    cdef set _index_exec_algorithms

    cdef readonly int tick_capacity
    """The caches tick capacity.\n\n:returns: `int`"""
    cdef readonly int bar_capacity
    """The caches bar capacity.\n\n:returns: `int`"""
    cdef readonly bint snapshot_orders
    """If order state snapshots should be taken.\n\n:returns: `bool`"""
    cdef readonly bint snapshot_positions
    """If position state snapshots should be taken.\n\n:returns: `bool`"""

    cpdef void cache_general(self)
    cpdef void cache_currencies(self)
    cpdef void cache_instruments(self)
    cpdef void cache_synthetics(self)
    cpdef void cache_accounts(self)
    cpdef void cache_orders(self)
    cpdef void cache_order_lists(self)
    cpdef void cache_positions(self)
    cpdef void build_index(self)
    cpdef bint check_integrity(self)
    cpdef bint check_residuals(self)
    cpdef void clear_index(self)
    cpdef void reset(self)
    cpdef void flush_db(self)

    cdef tuple _build_quote_table(self, Venue venue)
    cdef void _build_index_venue_account(self)
    cdef void _cache_venue_account_id(self, AccountId account_id)
    cdef void _build_indexes_from_orders(self)
    cdef void _build_indexes_from_positions(self)
    cdef set _build_order_query_filter_set(self, Venue venue, InstrumentId instrument_id, StrategyId strategy_id)
    cdef set _build_position_query_filter_set(self, Venue venue, InstrumentId instrument_id, StrategyId strategy_id)
    cdef list _get_orders_for_ids(self, set client_order_ids, OrderSide side)
    cdef list _get_positions_for_ids(self, set position_ids, PositionSide side)
    cdef void _assign_position_id_to_contingencies(self, Order order)
    cdef Money _calculate_unrealized_pnl(self, Position position)

    cpdef Instrument load_instrument(self, InstrumentId instrument_id)
    cpdef SyntheticInstrument load_synthetic(self, InstrumentId instrument_id)
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef void load_actor(self, Actor actor)
    cpdef void load_strategy(self, Strategy strategy)

    cpdef void add_order_book(self, OrderBook order_book)
    cpdef void add_ticker(self, Ticker ticker)
    cpdef void add_quote_tick(self, QuoteTick tick)
    cpdef void add_trade_tick(self, TradeTick tick)
    cpdef void add_bar(self, Bar bar)
    cpdef void add_quote_ticks(self, list ticks)
    cpdef void add_trade_ticks(self, list ticks)
    cpdef void add_bars(self, list bars)
    cpdef void add_currency(self, Currency currency)
    cpdef void add_instrument(self, Instrument instrument)
    cpdef void add_synthetic(self, SyntheticInstrument synthetic)
    cpdef void add_account(self, Account account)
    cpdef void add_order(self, Order order, PositionId position_id=*, ClientId client_id=*, bint override=*)
    cpdef void add_order_list(self, OrderList order_list)
    cpdef void add_position_id(self, PositionId position_id, Venue venue, ClientOrderId client_order_id, StrategyId strategy_id)
    cpdef void add_position(self, Position position, OmsType oms_type)
    cpdef void snapshot_position(self, Position position)
    cpdef void snapshot_position_state(self, Position position, uint64_t ts_snapshot, bint open_only=*)
    cpdef void snapshot_order_state(self, Order order)

    cpdef void update_account(self, Account account)
    cpdef void update_order(self, Order order)
    cpdef void update_order_pending_cancel_local(self, Order order)
    cpdef void update_position(self, Position position)
    cpdef void update_actor(self, Actor actor)
    cpdef void delete_actor(self, Actor actor)
    cpdef void update_strategy(self, Strategy strategy)
    cpdef void delete_strategy(self, Strategy strategy)

    cpdef void heartbeat(self, datetime timestamp)
