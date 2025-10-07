# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.cache.facade cimport CacheDatabaseFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport FundingRateUpdate
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.data cimport MarkPriceUpdate
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport Strategy


cdef class Cache(CacheFacade):
    cdef Logger _log
    cdef CacheDatabaseFacade _database

    cdef dict _general
    cdef dict _currencies
    cdef dict _instruments
    cdef dict _synthetics
    cdef dict _order_books
    cdef dict _own_order_books
    cdef dict _quote_ticks
    cdef dict _trade_ticks
    cdef dict _xrate_symbols
    cdef dict _mark_prices
    cdef dict _index_prices
    cdef dict _funding_rates
    cdef dict _bars
    cdef dict _bars_bid
    cdef dict _bars_ask
    cdef dict _accounts
    cdef dict _orders
    cdef dict _order_lists
    cdef dict _positions
    cdef dict _position_snapshots
    cdef dict _greeks
    cdef dict _yield_curves
    cdef dict[tuple[Currency, Currency], double] _mark_xrates

    cdef dict _index_venue_account
    cdef dict _index_venue_orders
    cdef dict _index_venue_positions
    cdef dict _index_venue_order_ids
    cdef dict _index_client_order_ids
    cdef dict _index_order_position
    cdef dict _index_order_strategy
    cdef dict _index_order_client
    cdef dict _index_position_strategy
    cdef dict _index_position_orders
    cdef dict _index_instrument_orders
    cdef dict _index_instrument_positions
    cdef dict _index_instrument_position_snapshots
    cdef dict _index_strategy_orders
    cdef dict _index_strategy_positions
    cdef dict _index_exec_algorithm_orders
    cdef dict _index_exec_spawn_orders
    cdef set _index_orders
    cdef set _index_orders_open
    cdef set _index_orders_open_pyo3
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
    cdef bint _drop_instruments_on_reset
    cdef Venue _specific_venue

    cdef readonly bint has_backing
    """If the cache has a database backing.\n\n:returns: `bool`"""
    cdef readonly bint persist_account_events
    """If account state events are written to the backing database.\n\n:returns: `bool`"""
    cdef readonly int tick_capacity
    """The caches tick capacity.\n\n:returns: `int`"""
    cdef readonly int bar_capacity
    """The caches bar capacity.\n\n:returns: `int`"""

    cpdef void set_specific_venue(self, Venue venue)
    cpdef void cache_all(self)
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
    cpdef void purge_closed_orders(self, uint64_t ts_now, uint64_t buffer_secs=*, bint purge_from_database=*)
    cpdef void purge_closed_positions(self, uint64_t ts_now, uint64_t buffer_secs=*, bint purge_from_database=*)
    cpdef void purge_order(self, ClientOrderId client_order_id, bint purge_from_database=*)
    cpdef void purge_position(self, PositionId position_id, bint purge_from_database=*)
    cpdef void purge_account_events(self, uint64_t ts_now, uint64_t lookback_secs=*, bint purge_from_database=*)
    cpdef void clear_index(self)
    cpdef void reset(self)
    cpdef void dispose(self)
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
    cpdef Money calculate_unrealized_pnl(self, Position position)

    cpdef Instrument load_instrument(self, InstrumentId instrument_id)
    cpdef SyntheticInstrument load_synthetic(self, InstrumentId instrument_id)
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef void load_actor(self, Actor actor)
    cpdef void load_strategy(self, Strategy strategy)

    cpdef void add_order_book(self, OrderBook order_book)
    cpdef void add_own_order_book(self, own_order_book)
    cpdef void add_quote_tick(self, QuoteTick tick)
    cpdef void add_trade_tick(self, TradeTick tick)
    cpdef void add_mark_price(self, MarkPriceUpdate mark_price)
    cpdef void add_index_price(self, IndexPriceUpdate index_price)
    cpdef void add_funding_rate(self, FundingRateUpdate funding_rate)
    cpdef void add_bar(self, Bar bar)
    cpdef void add_quote_ticks(self, list ticks)
    cpdef void add_trade_ticks(self, list ticks)
    cpdef void add_bars(self, list bars)
    cpdef void add_currency(self, Currency currency)
    cpdef void add_instrument(self, Instrument instrument)
    cpdef void add_synthetic(self, SyntheticInstrument synthetic)
    cpdef void add_account(self, Account account)
    cpdef void add_venue_order_id(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, bint overwrite=*)
    cpdef void add_order(self, Order order, PositionId position_id=*, ClientId client_id=*, bint overwrite=*)
    cpdef void add_order_list(self, OrderList order_list)
    cpdef void add_position_id(self, PositionId position_id, Venue venue, ClientOrderId client_order_id, StrategyId strategy_id)
    cpdef void add_position(self, Position position, OmsType oms_type)

    cpdef void snapshot_position(self, Position position)
    cpdef void snapshot_position_state(self, Position position, uint64_t ts_snapshot, Money unrealized_pnl=*, bint open_only=*)
    cpdef void snapshot_order_state(self, Order order)

    cpdef void update_account(self, Account account)
    cpdef void update_order(self, Order order)
    cpdef void update_order_pending_cancel_local(self, Order order)
    cpdef void update_own_order_book(self, Order order)
    cpdef void update_position(self, Position position)
    cpdef void update_actor(self, Actor actor)
    cpdef void update_strategy(self, Strategy strategy)
    cpdef void delete_actor(self, Actor actor)
    cpdef void delete_strategy(self, Strategy strategy)

    cpdef void heartbeat(self, datetime timestamp)
    cpdef void force_remove_from_own_order_book(self, ClientOrderId client_order_id)
    cpdef void audit_own_order_books(self)

    cdef timedelta _get_timedelta(self, BarType bar_type)

    cpdef list bar_types(
        self,
        InstrumentId instrument_id=*,
        object price_type=*,
        aggregation_source=*,
    )


cdef dict[Decimal, list[Order]] process_own_order_map(
    dict[Decimal, list[nautilus_pyo3.OwnBookOrder]] own_order_map,
    dict[ClientOrderId, Order] order_cache,
)
