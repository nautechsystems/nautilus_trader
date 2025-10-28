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

import pickle
import time
import uuid
from collections import deque
from decimal import Decimal

from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.rust.model import PriceType as PriceType_py

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.cache.facade cimport CacheDatabaseFacade
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.model.book cimport should_handle_own_book_order
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport BarSpecification
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport FundingRateUpdate
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.data cimport MarkPriceUpdate
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.functions cimport order_status_to_pyo3
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.crypto_perpetual cimport CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair cimport CurrencyPair
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.trading.strategy cimport Strategy


cdef class Cache(CacheFacade):
    """
    Provides a common object cache for market and execution related data.

    Parameters
    ----------
    database : CacheDatabaseFacade, optional
        The database adapter for the cache. If ``None`` then will bypass persistence.
    config : CacheConfig, optional
        The cache configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `CacheConfig`.
    """

    def __init__(
        self,
        CacheDatabaseFacade database: CacheDatabaseFacade | None = None,
        config: CacheConfig | None = None,
    ) -> None:
        if config is None:
            config = CacheConfig()

        Condition.type(config, CacheConfig, "config")

        self._database = database
        self._log = Logger(name=type(self).__name__)

        # Configuration
        self._drop_instruments_on_reset = config.drop_instruments_on_reset
        self.has_backing = database is not None
        self.persist_account_events = config.persist_account_events
        self.tick_capacity = config.tick_capacity
        self.bar_capacity = config.bar_capacity
        self._specific_venue = None

        # Caches
        self._general: dict[str, bytes] = {}
        self._currencies: dict[str, Currency] = {}
        self._instruments: dict[InstrumentId, Instrument] = {}
        self._synthetics: dict[InstrumentId, SyntheticInstrument] = {}
        self._order_books: dict[InstrumentId, OrderBook] = {}
        self._own_order_books: dict[InstrumentId, nautilus_pyo3.OwnOrderBook] = {}
        self._quote_ticks: dict[InstrumentId, deque[QuoteTick]] = {}
        self._trade_ticks: dict[InstrumentId, deque[TradeTick]] = {}
        self._xrate_symbols: dict[InstrumentId, str] = {}
        self._mark_xrates: dict[tuple[Currency, Currency], double] = {}
        self._mark_prices: dict[InstrumentId, deque[MarkPriceUpdate]] = {}
        self._index_prices: dict[InstrumentId, deque[IndexPriceUpdate]] = {}
        self._funding_rates: dict[InstrumentId, FundingRateUpdate] = {}
        self._bars: dict[BarType, deque[Bar]] = {}
        self._bars_bid: dict[InstrumentId, Bar] = {}
        self._bars_ask: dict[InstrumentId, Bar] = {}
        self._accounts: dict[AccountId, Account] = {}
        self._orders: dict[ClientOrderId, Order] = {}
        self._order_lists: dict[OrderListId, OrderList] = {}
        self._positions: dict[PositionId, Position] = {}
        self._position_snapshots: dict[PositionId, list[bytes]] = {}
        self._greeks: dict[InstrumentId, object] = {}
        self._yield_curves: dict[str, object] = {}

        # Cache index
        self._index_venue_account: dict[Venue, AccountId] = {}
        self._index_venue_orders: dict[Venue, set[ClientOrderId]] = {}
        self._index_venue_positions: dict[Venue, set[PositionId]] = {}
        self._index_venue_order_ids: dict[VenueOrderId, ClientOrderId] = {}
        self._index_client_order_ids: dict[ClientOrderId, VenueOrderId] = {}
        self._index_order_position: dict[ClientOrderId, PositionId] = {}
        self._index_order_strategy: dict[ClientOrderId, StrategyId] = {}
        self._index_order_client: dict[ClientOrderId, ClientId] = {}
        self._index_position_strategy: dict[PositionId, StrategyId] = {}
        self._index_position_orders: dict[PositionId, set[ClientOrderId]] = {}
        self._index_instrument_orders: dict[InstrumentId, set[ClientOrderId]] = {}
        self._index_instrument_positions: dict[InstrumentId, set[PositionId]] = {}
        self._index_instrument_position_snapshots: dict[InstrumentId, set[PositionId]] = {}
        self._index_strategy_orders: dict[StrategyId, set[ClientOrderId]] = {}
        self._index_strategy_positions: dict[StrategyId, set[PositionId]] = {}
        self._index_exec_algorithm_orders: dict[ExecAlgorithmId, set[ClientOrderId]] = {}
        self._index_exec_spawn_orders: dict[ClientOrderId: set[ClientOrderId]] = {}
        self._index_orders: set[ClientOrderId] = set()
        self._index_orders_open: set[ClientOrderId] = set()
        self._index_orders_open_pyo3: set[nautilus_pyo3.ClientOrderId] = set()
        self._index_orders_closed: set[ClientOrderId] = set()
        self._index_orders_emulated: set[ClientOrderId] = set()
        self._index_orders_inflight: set[ClientOrderId] = set()
        self._index_orders_pending_cancel: set[ClientOrderId] = set()
        self._index_positions: set[PositionId] = set()
        self._index_positions_open: set[PositionId] = set()
        self._index_positions_closed: set[PositionId] = set()
        self._index_actors: set[ComponentId] = set()
        self._index_strategies: set[StrategyId] = set()
        self._index_exec_algorithms: set[ExecAlgorithmId] = set()

        self._log.info("READY")

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void set_specific_venue(self, Venue venue):
        """
        Set a specific venue that the cache will use for subsequent `account_for_venue` calls.

        Primarily for Interactive Brokers, a multi-venue brokerage where account updates
        are not tied to a single venue.

        Parameters
        ----------
        venue : Venue
            The specific venue to set.

        """
        Condition.not_none(venue, "venue")

        self._specific_venue = venue

    cpdef void cache_all(self):
        """
        Clears and loads the currencies, instruments, synthetics, accounts, orders, and positions.
        from the cache database.
        """
        self._log.debug(f"Loading currencies, instruments, synthetics, accounts, orders, and positions cache from database")

        if self._database is not None:
            result = self._database.load_all()
            self._currencies = result.get("currencies", {})
            self._instruments = result.get("instruments", {})
            self._synthetics = result.get("synthetics", {})
            self._accounts = result.get("accounts", {})
            self._orders = result.get("orders", {})
            self._positions = result.get("positions", {})
        else:
            self._currencies = {}
            self._instruments = {}
            self._synthetics = {}
            self._accounts = {}
            self._orders = {}
            self._positions = {}

        # Register currencies with internal `CURRENCY_MAP`
        cdef Currency currency

        for currency in self._currencies.values():
            Currency.register_c(currency, overwrite=False)

        # Assign position IDs to contingent orders
        cdef Order order

        for order in self._orders.values():
            if order.contingency_type == ContingencyType.OTO and order.position_id is not None:
                self._assign_position_id_to_contingencies(order)

        cdef int currencies_count = len(self._currencies)
        self._log.info(
            f"Cached {currencies_count} currenc{'y' if currencies_count == 1 else 'ies'} from database",
            color=LogColor.BLUE if self._currencies else LogColor.NORMAL,
        )

        cdef int instruments_count = len(self._instruments)
        self._log.info(
            f"Cached {instruments_count} instrument{'' if instruments_count == 1 else 's'} from database",
            color=LogColor.BLUE if self._instruments else LogColor.NORMAL,
        )

        cdef int synthetics_count = len(self._synthetics)
        self._log.info(
            f"Cached {synthetics_count} synthetic instrument{'' if synthetics_count == 1 else 's'} from database",
            color=LogColor.BLUE if self._synthetics else LogColor.NORMAL,
        )

        cdef int accounts_count = len(self._accounts)
        self._log.info(
            f"Cached {accounts_count} account{'' if accounts_count == 1 else 's'} from database",
            color=LogColor.BLUE if self._accounts else LogColor.NORMAL,
        )

        cdef int orders_count = len(self._orders)
        self._log.info(
            f"Cached {orders_count} order{'' if orders_count == 1 else 's'} from database",
            color=LogColor.BLUE if self._orders else LogColor.NORMAL,
        )

        cdef int positions_count = len(self._positions)
        self._log.info(
            f"Cached {positions_count} position{'' if positions_count == 1 else 's'} from database",
            color=LogColor.BLUE if self._positions else LogColor.NORMAL
        )

    cpdef void cache_general(self):
        """
        Clear the current general cache and load the general objects from the
        cache database.
        """
        cdef double ts_start = time.time()
        cdef double ts_end

        self._log.debug(f"Loading general cache from database")

        if self._database is not None:
            ts_end = time.time()
            self._log.debug(f"cache_general: Before database.load() took {(ts_end - ts_start) * 1000:.2f}ms")

            ts_start = time.time()
            self._general = self._database.load()
            ts_end = time.time()
            self._log.debug(f"cache_general: database.load() took {(ts_end - ts_start) * 1000:.2f}ms")
        else:
            self._general = {}

        cdef int count = len(self._general)
        self._log.info(
            f"Cached {count} general object{'' if count == 1 else 's'} from database",
            color=LogColor.BLUE if self._general else LogColor.NORMAL,
        )

    cpdef void cache_currencies(self):
        """
        Clear the current currencies cache and load currencies from the cache
        database.
        """
        self._log.debug(f"Loading currencies from database")

        if self._database is not None:
            self._currencies = self._database.load_currencies()
        else:
            self._currencies = {}

        # Register currencies with internal `CURRENCY_MAP`
        cdef Currency currency

        for currency in self._currencies.values():
            Currency.register_c(currency, overwrite=False)

        cdef int count = len(self._currencies)
        self._log.info(
            f"Cached {count} currenc{'y' if count == 1 else 'ies'} from database",
            color=LogColor.BLUE if self._currencies else LogColor.NORMAL,
        )

    cpdef void cache_instruments(self):
        """
        Clear the current instruments cache and load instruments from the cache
        database.
        """
        self._log.debug(f"Loading instruments from database")

        if self._database is not None:
            self._instruments = self._database.load_instruments()
        else:
            self._instruments = {}

        cdef int count = len(self._instruments)
        self._log.info(
            f"Cached {count} instrument{'' if count == 1 else 's'} from database",
            color=LogColor.BLUE if self._instruments else LogColor.NORMAL,
        )

    cpdef void cache_synthetics(self):
        """
        Clear the current synthetic instruments cache and load synthetic instruments from the cache
        database.
        """
        self._log.debug(f"Loading synthetic instruments from database")

        if self._database is not None:
            self._synthetics = self._database.load_synthetics()
        else:
            self._synthetics = {}

        cdef int count = len(self._synthetics)
        self._log.info(
            f"Cached {count} synthetic instrument{'' if count == 1 else 's'} from database",
            color=LogColor.BLUE if self._synthetics else LogColor.NORMAL,
        )

    cpdef void cache_accounts(self):
        """
        Clear the current accounts cache and load accounts from the cache
        database.
        """
        self._log.debug(f"Loading accounts from database")

        if self._database is not None:
            self._accounts = self._database.load_accounts()
        else:
            self._accounts = {}

        cdef int count = len(self._accounts)
        self._log.info(
            f"Cached {count} account{'' if count == 1 else 's'} from database",
            color=LogColor.BLUE if self._accounts else LogColor.NORMAL,
        )

    cpdef void cache_orders(self):
        """
        Clear the current orders cache and load orders from the cache database.
        """
        self._log.debug(f"Loading orders from database")

        if self._database is not None:
            self._orders = self._database.load_orders()
            self._index_order_position = self._database.load_index_order_position()
            self._index_order_client = self._database.load_index_order_client()
        else:
            self._orders = {}

        # Assign position IDs to contingent orders
        cdef Order order

        for order in self._orders.values():
            if order.contingency_type == ContingencyType.OTO and order.position_id is not None:
                self._assign_position_id_to_contingencies(order)

        cdef int count = len(self._orders)
        self._log.info(
            f"Cached {count} order{'' if count == 1 else 's'} from database",
            color=LogColor.BLUE if self._orders else LogColor.NORMAL,
        )

    cpdef void cache_order_lists(self):
        """
        Clear the current order lists cache and load order lists using cached orders.
        """
        self._log.debug(f"Loading order lists")
        cdef dict order_list_index = {}  # type: dict[OrderListId, list[Order]]

        # Collect all orders common to an OrderListId
        cdef:
            Order order
            list orders
        for order in self._orders.values():
            if order.order_list_id is not None:
                orders = order_list_index.get(order.order_list_id)

                if orders is None:
                    orders = []
                    order_list_index[order.order_list_id] = orders

                orders.append(order)

        # Rebuild and cache order lists
        cdef:
            OrderListId order_list_id
            OrderList order_list
        for order_list_id, orders in order_list_index.items():
            order_list = OrderList(
                order_list_id=order_list_id,
                orders=orders,
            )
            self._order_lists[order_list_id] = order_list

        cdef int count = len(self._order_lists)
        self._log.info(
            f"Cached {count} order list{'' if count == 1 else 's'} from database",
            color=LogColor.BLUE if self._order_lists else LogColor.NORMAL,
        )

    cpdef void cache_positions(self):
        """
        Clear the current positions cache and load positions from the cache
        database.
        """
        self._log.debug(f"Loading positions from database")

        if self._database is not None:
            self._positions = self._database.load_positions()
        else:
            self._positions = {}

        cdef int count = len(self._positions)
        self._log.info(
            f"Cached {count} position{'' if count == 1 else 's'} from database",
            color=LogColor.BLUE if self._positions else LogColor.NORMAL
        )

    cpdef void build_index(self):
        """
        Build the cache index from objects currently held in memory.
        """
        self._log.debug(f"Building index")
        cdef double ts = time.time()

        self._build_index_venue_account()
        self._build_indexes_from_orders()
        self._build_indexes_from_positions()

        self._log.debug(f"Index built in {time.time() - ts:.3f}s")

    cpdef bint check_integrity(self):
        """
        Check integrity of data within the cache.

        All data should be loaded from the database prior to this call. If an
        error is found then a log error message will also be produced.

        Returns
        -------
        bool
            True if checks pass, else False.

        """
        cdef int error_count = 0
        cdef str failure = "Integrity failure"

        # As there should be a bi-directional one-to-one relationship between
        # caches and indexes, each cache and index must be checked individually

        cdef uint64_t timestamp_us = time.time_ns() // 1000
        self._log.info("Checking data integrity")

        cdef:
            AccountId account_id
            Order order
            Position position
        for account_id in self._accounts:
            if Venue(account_id.get_issuer()) not in self._index_venue_account:
                self._log.error(
                    f"{failure} in _cached_accounts: "
                    f"{repr(account_id)} not found in self._index_venue_account"
                )
                error_count += 1

        for client_order_id, order in self._orders.items():
            if client_order_id not in self._index_order_strategy:
                self._log.error(
                    f"{failure} in _cached_orders: "
                    f"{repr(client_order_id)} not found in self._index_order_strategy"
                )
                error_count += 1
            if client_order_id not in self._index_orders:
                self._log.error(
                    f"{failure} in _cached_orders: "
                    f"{repr(client_order_id)} not found in self._index_orders"
                )
                error_count += 1
            if order.is_inflight_c() and client_order_id not in self._index_orders_inflight:
                self._log.error(
                    f"{failure} in _cached_orders: "
                    f"{repr(client_order_id)} not found in self._index_orders_inflight"
                )
                error_count += 1
            if order.is_open_c() and client_order_id not in self._index_orders_open:
                self._log.error(
                    f"{failure} in _cached_orders: "
                    f"{repr(client_order_id)} not found in self._index_orders_open"
                )
                error_count += 1
            if order.is_closed_c() and client_order_id not in self._index_orders_closed:
                self._log.error(
                    f"{failure} in _cached_orders "
                    f"{repr(client_order_id)} not found in self._index_orders_closed"
                )
                error_count += 1
            if order.exec_algorithm_id is not None and order.exec_algorithm_id not in self._index_exec_algorithm_orders:
                self._log.error(
                    f"{failure} in _cached_orders "
                    f"{repr(order.exec_algorithm_id)} not found in self._index_exec_algorithm_orders"
                )
                error_count += 1
            if order.exec_algorithm_id is not None and order.exec_spawn_id is None and order.client_order_id not in self._index_exec_spawn_orders:
                self._log.error(
                    f"{failure} in _cached_orders "
                    f"{repr(order.exec_algorithm_id)} not found in self._index_exec_spawn_orders"
                )
                error_count += 1

        for position_id, position in self._positions.items():
            if position_id not in self._index_position_strategy:
                self._log.error(
                    f"{failure} in _cached_positions: "
                    f"{repr(position_id)} not found in self._index_position_strategy"
                )
                error_count += 1
            if position_id not in self._index_position_orders:
                self._log.error(
                    f"{failure} in _cached_positions: "
                    f"{repr(position_id)} not found in self._index_position_orders"
                )
                error_count += 1
            if position_id not in self._index_positions:
                self._log.error(
                    f"{failure} in _cached_positions: "
                    f"{repr(position_id)} not found in self._index_positions"
                )
                error_count += 1
            if position.is_open_c() and position_id not in self._index_positions_open:
                self._log.error(
                    f"{failure} in _cached_positions: "
                    f"{repr(position_id)} not found in self._index_positions_open"
                )
                error_count += 1
            if position.is_closed_c() and position_id not in self._index_positions_closed:
                self._log.error(
                    f"{failure} in _cached_positions: "
                    f"{repr(position_id)} not found in self._index_positions_closed"
                )
                error_count += 1

        # Check indexes
        # -------------
        for account_id in self._index_venue_account.values():
            if account_id not in self._accounts:
                self._log.error(
                    f"{failure} in _index_venue_account: "
                    f"{repr(account_id)} not found in self._cached_accounts"
                )
                error_count += 1

        for client_order_id in self._index_venue_order_ids.values():
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_venue_order_ids: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for client_order_id in self._index_client_order_ids:
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_client_order_ids: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for client_order_id in self._index_order_position:
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_order_position: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for client_order_id in self._index_order_strategy:
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_order_strategy: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for position_id in self._index_position_strategy:
            if position_id not in self._positions:
                self._log.error(
                    f"{failure} in _index_position_strategy: "
                    f"{repr(position_id)} not found in self._cached_positions"
                )
                error_count += 1

        for position_id in self._index_position_orders:
            if position_id not in self._positions:
                self._log.error(
                    f"{failure} in _index_position_orders: "
                    f"{repr(position_id)} not found in self._cached_positions"
                )
                error_count += 1

        for instrument_id, client_order_ids in self._index_instrument_orders.items():
            for client_order_id in client_order_ids:
                if client_order_id not in self._orders:
                    self._log.error(
                        f"{failure} in _index_instrument_orders: "
                        f"{repr(instrument_id)} not found in self._cached_orders"
                    )
                    error_count += 1

        for instrument_id in self._index_instrument_positions:
            if instrument_id not in self._index_instrument_orders:
                self._log.error(
                    f"{failure} in _index_instrument_positions: "
                    f"{repr(instrument_id)} not found in self._index_instrument_orders"
                )
                error_count += 1

        for client_order_ids in self._index_strategy_orders.values():
            for client_order_id in client_order_ids:
                if client_order_id not in self._orders:
                    self._log.error(
                        f"{failure} in _index_strategy_orders: "
                        f"{repr(client_order_id)} not found in self._cached_orders"
                    )
                    error_count += 1

        for position_ids in self._index_strategy_positions.values():
            for position_id in position_ids:
                if position_id not in self._positions:
                    self._log.error(
                        f"{failure} in _index_strategy_positions: "
                        f"{repr(position_id)} not found in self._caches_positions"
                    )
                    error_count += 1

        for client_order_id in self._index_orders:
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_orders: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for client_order_id in self._index_orders_emulated:
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_orders_emulated: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for client_order_id in self._index_orders_inflight:
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_orders_inflight: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for client_order_id in self._index_orders_open:
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_orders_open: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for client_order_id in self._index_orders_closed:
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_orders_closed: "
                    f"{repr(client_order_id)} not found in self._cached_orders"
                )
                error_count += 1

        for position_id in self._index_positions:
            if position_id not in self._positions:
                self._log.error(
                    f"{failure} in _index_positions: "
                    f"{repr(position_id)} not found in self._cached_positions"
                )
                error_count += 1

        for position_id in self._index_positions_open:
            if position_id not in self._positions:
                self._log.error(
                    f"{failure} in _index_positions_open: "
                    f"{repr(position_id)} not found in self._cached_positions"
                )
                error_count += 1

        for position_id in self._index_positions_closed:
            if position_id not in self._positions:
                self._log.error(
                    f"{failure} in _index_positions_closed: "
                    f"{repr(position_id)} not found in self._cached_positions"
                )
                error_count += 1

        for strategy_id in self._index_strategies:
            if strategy_id not in self._index_strategy_orders:
                self._log.error(
                    f"{failure} in _index_strategies: "
                    f"{repr(strategy_id)} not found in self._index_strategy_orders"
                )
                error_count += 1

        for exec_algorithm_id in self._index_exec_algorithms:
            if exec_algorithm_id not in self._index_exec_algorithm_orders:
                self._log.error(
                    f"{failure} in _index_exec_algorithms: "
                    f"{repr(exec_algorithm_id)} not found in self._index_exec_algorithm_orders"
                )
                error_count += 1

        # Finally
        cdef uint64_t total_us = round((time.time_ns() // 1000) - timestamp_us)
        if error_count == 0:
            self._log.info(
                f"Integrity check passed in {total_us}μs",
                color=LogColor.GREEN
            )
            return True
        else:
            self._log.error(
                f"Integrity check failed with "
                f"{error_count} error{'' if error_count == 1 else 's'} "
                f"in {total_us}μs"
            )
            return False

    cpdef bint check_residuals(self):
        """
        Check for any residual open state and log warnings if any are found.

        'Open state' is considered to be open orders and open positions.

        Returns
        -------
        bool
            True if residuals exist, else False.

        """
        self._log.debug("Checking residuals")

        cdef bint residuals = False

        # Check for any residual active orders and log warnings if any are found
        for order in self.orders_open():
            residuals = True
            self._log.warning(f"Residual {order}")

        for position in self.positions_open():
            residuals = True
            self._log.warning(f"Residual {position}")

        return residuals


    cpdef void purge_closed_orders(
        self,
        uint64_t ts_now,
        uint64_t buffer_secs = 0,
        bint purge_from_database = False,
    ):
        """
        Purge all closed orders from the cache.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).
        buffer_secs : uint64_t, default 0
            The purge buffer (seconds) from when the order was closed.
            Only orders that have been closed for at least this amount of time will be purged.
            A value of 0 means purge all closed orders regardless of when they were closed.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        cdef str buffer_secs_str = f" with {buffer_secs=:_}" if buffer_secs else ""
        self._log.debug(f"Purging closed orders{buffer_secs_str}", LogColor.MAGENTA)

        cdef uint64_t buffer_ns = nautilus_pyo3.secs_to_nanos(buffer_secs)

        cdef:
            ClientOrderId client_order_id
            ClientOrderId linked_order_id
            Order order
            Order linked_order
        for client_order_id in self._index_orders_closed.copy():
            order = self._orders.get(client_order_id)

            if order is not None and order.is_closed_c() and order.ts_closed + buffer_ns <= ts_now:
                # Check any linked orders (contingency orders)
                if order.linked_order_ids is not None:
                    for linked_order_id in order.linked_order_ids:
                        linked_order = self._orders.get(linked_order_id)
                        if linked_order is not None and linked_order.is_open_c():
                            break  # Do not purge if linked order still open
                    else:
                        self.purge_order(client_order_id, purge_from_database)
                else:
                    self.purge_order(client_order_id, purge_from_database)

    cpdef void purge_closed_positions(
        self,
        uint64_t ts_now,
        uint64_t buffer_secs = 0,
        bint purge_from_database = False,
    ):
        """
        Purge all closed positions from the cache.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).
        buffer_secs : uint64_t, default 0
            The purge buffer (seconds) from when the position was closed.
            Only positions that have been closed for at least this amount of time will be purged.
            A value of 0 means purge all closed positions regardless of when they were closed.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        cdef str buffer_secs_str = f" with {buffer_secs=:_}" if buffer_secs else ""
        self._log.debug(f"Purging closed positions{buffer_secs_str}", LogColor.MAGENTA)
        cdef uint64_t buffer_ns = nautilus_pyo3.secs_to_nanos(buffer_secs)

        cdef:
            PositionId position_id
            Position position
        for position_id in self._index_positions_closed.copy():
            position = self._positions.get(position_id)

            if position is not None and position.is_closed_c() and position.ts_closed + buffer_ns <= ts_now:
                self.purge_position(position_id, purge_from_database)

    cpdef void purge_order(self, ClientOrderId client_order_id, bint purge_from_database = False):
        """
        Purge the order for the given client order ID from the cache (if found).

        For safety, an order is prevented from being purged if it's open.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to purge.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        Condition.not_none(client_order_id, "client_order_id")

        # Check if order exists and is safe to purge before popping
        cdef Order order = self._orders.get(client_order_id)

        if order is not None and order.is_open_c():
            self._log.warning(f"Order {client_order_id} found open when purging, skipping purge")
            return

        if order is None:
            self._log.warning(f"Order {client_order_id} not found when purging")
        else:
            # Safe to purge
            self._orders.pop(client_order_id, None)
            self._index_venue_orders[order.instrument_id.venue].discard(client_order_id)
            self._index_venue_order_ids.pop(order.venue_order_id, None)
            self._index_instrument_orders[order.instrument_id].discard(client_order_id)

            if order.position_id is not None:
                self._index_position_orders[order.position_id].discard(client_order_id)

            if order.exec_algorithm_id is not None:
                self._index_exec_algorithm_orders[order.exec_algorithm_id].discard(client_order_id)

            # Clean up strategy orders reverse index
            strategy_orders = self._index_strategy_orders.get(order.strategy_id)
            if strategy_orders is not None:
                strategy_orders.discard(client_order_id)
                if not strategy_orders:
                    self._index_strategy_orders.pop(order.strategy_id, None)

            # Clean up exec spawn reverse index (if this order is a spawned child)
            if order.exec_spawn_id is not None:
                spawn_orders = self._index_exec_spawn_orders.get(order.exec_spawn_id)
                if spawn_orders is not None:
                    spawn_orders.discard(client_order_id)
                    if not spawn_orders:
                        self._index_exec_spawn_orders.pop(order.exec_spawn_id, None)

            self._log.info(f"Purged order {client_order_id}", LogColor.BLUE)

        # Always clean up order indices (even if order was not in cache)
        self._index_order_position.pop(client_order_id, None)
        self._index_order_client.pop(client_order_id, None)
        self._index_client_order_ids.pop(client_order_id, None)
        strategy_id = self._index_order_strategy.pop(client_order_id, None)

        # Clean up reverse index when order not in cache (using forward index)
        if strategy_id is not None:
            strategy_orders = self._index_strategy_orders.get(strategy_id)
            if strategy_orders is not None:
                strategy_orders.discard(client_order_id)
                if not strategy_orders:
                    self._index_strategy_orders.pop(strategy_id, None)

        # Remove spawn parent entry if this order was a spawn root
        self._index_exec_spawn_orders.pop(client_order_id, None)

        self._index_orders.discard(client_order_id)
        self._index_orders_closed.discard(client_order_id)
        self._index_orders_emulated.discard(client_order_id)
        self._index_orders_inflight.discard(client_order_id)
        self._index_orders_pending_cancel.discard(client_order_id)

        # Delete from database if requested
        if purge_from_database and self._database is not None:
            self._database.delete_order(client_order_id)

    cpdef void purge_position(self, PositionId position_id, bint purge_from_database = False):
        """
        Purge the position for the given position ID from the cache (if found).

        For safety, a position is prevented from being purged if it's open.

        Parameters
        ----------
        position_id : PositionId
            The position ID to purge.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        Condition.not_none(position_id, "position_id")

        # Check if position exists and is safe to purge before popping
        cdef Position position = self._positions.get(position_id)

        if position is not None and position.is_open_c():
            self._log.warning(f"Position {position_id} found open when purging, skipping purge")
            return

        if position is None:
            self._log.warning(f"Position {position_id} not found when purging")
        else:
            # Safe to purge
            self._positions.pop(position_id, None)
            self._index_venue_positions[position.instrument_id.venue].discard(position_id)
            self._index_instrument_positions[position.instrument_id].discard(position_id)
            self._index_strategy_positions[position.strategy_id].discard(position_id)

            for client_order_id in position.client_order_ids_c():
                self._index_order_position.pop(client_order_id, None)

            self._log.info(f"Purged position {position_id}", LogColor.BLUE)

        # Always clean up position indices (even if position not in cache)
        self._index_position_strategy.pop(position_id, None)
        self._index_position_orders.pop(position_id, None)
        self._index_positions.discard(position_id)
        self._index_positions_open.discard(position_id)
        self._index_positions_closed.discard(position_id)

        # Remove position snapshots and clean up index
        cdef set[PositionId] snapshot_position_ids
        cdef list[bytes] snapshots = self._position_snapshots.pop(position_id, None)

        if snapshots is not None and position is not None:
            snapshot_position_ids = self._index_instrument_position_snapshots.get(position.instrument_id)
            if snapshot_position_ids:
                snapshot_position_ids.discard(position_id)

                # Clean up
                if not snapshot_position_ids:
                    self._index_instrument_position_snapshots.pop(position.instrument_id, None)

        # Delete from database if requested
        if purge_from_database and self._database is not None:
            self._database.delete_position(position_id)

    cpdef void purge_account_events(
        self,
        uint64_t ts_now,
        uint64_t lookback_secs = 0,
        bint purge_from_database = False,
    ):
        """
        Purge all account state events which are outside the lookback window.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).
        lookback_secs : uint64_t, default 0
            The purge lookback window (seconds) from when the account state event occurred.
            Only events which are outside the lookback window will be purged.
            A value of 0 means purge all account state events.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        cdef str lookback_secs_str = f" with {lookback_secs=:_}" if lookback_secs else ""
        self._log.debug(f"Purging account events{lookback_secs_str}", LogColor.MAGENTA)

        cdef Account account
        for account in self._accounts.values():
            event_count = account.event_count_c()

            # Track events before purging if database deletion is enabled
            events_before = None
            if purge_from_database and self._database is not None:
                events_before = account.events.copy()  # Copy, not alias

            account.purge_account_events(ts_now, lookback_secs)
            count_diff = event_count - account.event_count_c()

            if count_diff > 0:
                self._log.info(f"Purged {count_diff} event(s) from account {account.id}", LogColor.BLUE)

                # Delete from database if enabled
                if purge_from_database and self._database is not None and events_before is not None:
                    events_after = account.events
                    event_ids_before = {event.id for event in events_before}
                    event_ids_after = {event.id for event in events_after}
                    removed_event_ids = event_ids_before - event_ids_after

                    for event_id in removed_event_ids:
                        self._database.delete_account_event(account.id, event_id.value)

    cpdef void clear_index(self):
        self._log.debug(f"Clearing index")

        self._index_venue_account.clear()
        self._index_venue_orders.clear()
        self._index_venue_positions.clear()
        self._index_venue_order_ids.clear()
        self._index_client_order_ids.clear()
        self._index_order_position.clear()
        self._index_order_strategy.clear()
        self._index_order_client.clear()
        self._index_position_strategy.clear()
        self._index_position_orders.clear()
        self._index_instrument_orders.clear()
        self._index_instrument_positions.clear()
        self._index_instrument_position_snapshots.clear()
        self._index_strategy_orders.clear()
        self._index_strategy_positions.clear()
        self._index_exec_algorithm_orders.clear()
        self._index_exec_spawn_orders.clear()
        self._index_orders.clear()
        self._index_orders_open.clear()
        self._index_orders_open_pyo3.clear()
        self._index_orders_closed.clear()
        self._index_orders_emulated.clear()
        self._index_orders_inflight.clear()
        self._index_orders_pending_cancel.clear()
        self._index_positions.clear()
        self._index_positions_open.clear()
        self._index_positions_closed.clear()
        self._index_actors.clear()
        self._index_strategies.clear()
        self._index_exec_algorithms.clear()

        self._log.debug(f"Cleared index")

    cpdef void reset(self):
        """
        Reset the cache.

        All stateful fields are reset to their initial value.
        """
        self._log.debug("Resetting cache")

        self._general.clear()
        self._currencies.clear()
        self._synthetics.clear()
        self._order_books.clear()
        self._own_order_books.clear()
        self._quote_ticks.clear()
        self._trade_ticks.clear()
        self._xrate_symbols.clear()
        self._mark_xrates.clear()
        self._mark_prices.clear()
        self._index_prices.clear()
        self._funding_rates.clear()
        self._bars.clear()
        self._bars_bid.clear()
        self._bars_ask.clear()
        self._accounts.clear()
        self._orders.clear()
        self._order_lists.clear()
        self._positions.clear()
        self._position_snapshots.clear()
        self.clear_index()

        if self._drop_instruments_on_reset:
            self._instruments.clear()

        self._log.info(f"Reset")

    cpdef void dispose(self):
        """
        Dispose of the cache which will close any underlying database adapter.

        """
        if self._database is not None:
            self._database.close()

    cpdef void flush_db(self):
        """
        Flush the caches database which permanently removes all persisted data.

        Warnings
        --------
        Permanent data loss.

        """
        self._log.debug("Flushing cache database")

        if self._database is not None:
            self._database.flush()

        self._log.info("Cache database flushed")

    cdef void _build_index_venue_account(self):
        cdef AccountId account_id

        for account_id in self._accounts.keys():
            self._cache_venue_account_id(account_id)

    cdef void _cache_venue_account_id(self, AccountId account_id):
        self._index_venue_account[Venue(account_id.get_issuer())] = account_id

    cdef void _build_indexes_from_orders(self):
        cdef ClientOrderId client_order_id
        cdef Order order

        for client_order_id, order in self._orders.items():
            # 1: Build _index_venue_orders -> {Venue, {ClientOrderId}}
            if order.instrument_id.venue not in self._index_venue_orders:
                self._index_venue_orders[order.instrument_id.venue] = set()

            self._index_venue_orders[order.instrument_id.venue].add(client_order_id)

            # 2: Build _index_venue_order_ids -> {VenueOrderId, ClientOrderId}
            if order.venue_order_id is not None:
                self._index_venue_order_ids[order.venue_order_id] = order.client_order_id
                self._index_client_order_ids[order.client_order_id] = order.venue_order_id

            # 3: Build _index_order_position -> {ClientOrderId, PositionId}
            if order.position_id is not None:
                self._index_order_position[client_order_id] = order.position_id

            # 4: Build _index_order_strategy -> {ClientOrderId, StrategyId}
            self._index_order_strategy[client_order_id] = order.strategy_id

            # 5: Build _index_instrument_orders -> {InstrumentId, {ClientOrderId}}
            if order.instrument_id not in self._index_instrument_orders:
                self._index_instrument_orders[order.instrument_id] = set()

            self._index_instrument_orders[order.instrument_id].add(client_order_id)

            # 6: Build _index_strategy_orders -> {StrategyId, {ClientOrderId}}
            if order.strategy_id not in self._index_strategy_orders:
                self._index_strategy_orders[order.strategy_id] = set()

            self._index_strategy_orders[order.strategy_id].add(client_order_id)

            # 7: Build _index_exec_algorithm_orders -> {ExecAlgorithmId, {ClientOrderId}}
            if order.exec_algorithm_id is not None:
                if order.exec_algorithm_id not in self._index_exec_algorithm_orders:
                    self._index_exec_algorithm_orders[order.exec_algorithm_id] = set()

                self._index_exec_algorithm_orders[order.exec_algorithm_id].add(order.client_order_id)

            # 8: Build _index_exec_spawn_orders -> {ClientOrderId, {ClientOrderId}}
            if order.exec_algorithm_id is not None:
                if order.exec_spawn_id not in self._index_exec_spawn_orders:
                    self._index_exec_spawn_orders[order.exec_spawn_id] = set()

                self._index_exec_spawn_orders[order.exec_spawn_id].add(order.client_order_id)

            # 9: Build _index_orders -> {ClientOrderId}
            self._index_orders.add(client_order_id)

            # 10: Build _index_orders_open -> {ClientOrderId}
            if order.is_open_c():
                self._index_orders_open.add(client_order_id)

                if self._own_order_books:
                    self._index_orders_open_pyo3.add(nautilus_pyo3.ClientOrderId(client_order_id.value))

            # 11: Build _index_orders_closed -> {ClientOrderId}
            if order.is_closed_c():
                self._index_orders_closed.add(client_order_id)

            # 12: Build _index_orders_emulated -> {ClientOrderId}
            if order.emulation_trigger != TriggerType.NO_TRIGGER and not order.is_closed_c():
                self._index_orders_emulated.add(client_order_id)

            # 13: Build _index_orders_inflight -> {ClientOrderId}
            if order.is_inflight_c():
                self._index_orders_inflight.add(client_order_id)

            # 14: Build _index_strategies -> {StrategyId}
            self._index_strategies.add(order.strategy_id)

            # 15: Build _index_strategies -> {ExecAlgorithmId}
            if order.exec_algorithm_id is not None:
                self._index_exec_algorithms.add(order.exec_algorithm_id)

    cdef void _build_indexes_from_positions(self):
        cdef ClientOrderId client_order_id
        cdef PositionId position_id
        cdef Position position
        for position_id, position in self._positions.items():
            # 1: Build _index_venue_positions -> {Venue, {PositionId}}
            if position.instrument_id.venue not in self._index_venue_positions:
                self._index_venue_positions[position.instrument_id.venue] = set()

            self._index_venue_positions[position.instrument_id.venue].add(position_id)

            # 2: Build _index_position_strategy -> {PositionId, StrategyId}
            if position.strategy_id is not None:
                self._index_position_strategy[position_id] = position.strategy_id

            # 3: Build _index_position_orders -> {PositionId, {ClientOrderId}}
            if position_id not in self._index_position_orders:
                self._index_position_orders[position_id] = set()

            index_position_orders = self._index_position_orders[position_id]

            for client_order_id in position.client_order_ids_c():
                index_position_orders.add(client_order_id)

            # 4: Build _index_instrument_positions -> {InstrumentId, {PositionId}}
            if position.instrument_id not in self._index_instrument_positions:
                self._index_instrument_positions[position.instrument_id] = set()

            self._index_instrument_positions[position.instrument_id].add(position_id)

            # 5: Build _index_strategy_positions -> {StrategyId, {PositionId}}
            if position.strategy_id is not None and position.strategy_id not in self._index_strategy_positions:
                self._index_strategy_positions[position.strategy_id] = set()

            self._index_strategy_positions[position.strategy_id].add(position.id)

            # 6: Build _index_positions -> {PositionId}
            self._index_positions.add(position_id)

            # 7: Build _index_positions_open -> {PositionId}
            if position.is_open_c():
                self._index_positions_open.add(position_id)
            # 8: Build _index_positions_closed -> {PositionId}
            elif position.is_closed_c():
                self._index_positions_closed.add(position_id)

            # 9: Build _index_strategies -> {StrategyId}
            self._index_strategies.add(position.strategy_id)

    cdef void _assign_position_id_to_contingencies(self, Order order):
        cdef:
            ClientOrderId client_order_id
            Order contingent_order
        for client_order_id in order.linked_order_ids or []:
            contingent_order = self._orders.get(client_order_id)

            if contingent_order is None:
                self._log.error(f"Contingency order {client_order_id!r} not found")
                continue

            if contingent_order.position_id is None:
                # Assign the parents position ID
                contingent_order.position_id = order.position_id

                self.add_position_id(
                    order.position_id,
                    order.instrument_id.venue,
                    contingent_order.client_order_id,
                    order.strategy_id,
                )
                self._log.info(f"Assigned {order.position_id!r} to {client_order_id!r}")

    cpdef Money calculate_unrealized_pnl(self, Position position):
        cdef QuoteTick quote = self.quote_tick(position.instrument_id)

        if quote is None:
            self._log.warning(
                f"Cannot calculate unrealized PnL for {position.id!r}, "
                f"no quotes for {position.instrument_id}",
            )
            return None

        cdef Price last

        if position.side == PositionSide.FLAT:
            return Money(0.0, position.settlement_currency)
        elif position.side == PositionSide.LONG:
            last = quote.ask_price
        else:
            last = quote.bid_price

        return position.unrealized_pnl(last)

    cpdef void load_actor(self, Actor actor):
        """
        Load the state dictionary into the given actor.

        Parameters
        ----------
        actor : Actor
            The actor to load.

        """
        Condition.not_none(actor, "actor")

        cdef dict state = None

        if self._database is not None:
            state = self._database.load_actor(actor.id)

        if state:
            for key, value in state.items():
                self._log.debug(f"Loading {actor.id}) state {{ {key}: {value} }}")

            actor.load(state)
        else:
            self._log.info(f"No previous state found for {repr(actor.id)}")

    cpdef void load_strategy(self, Strategy strategy):
        """
        Load the state dictionary into the given strategy.

        Parameters
        ----------
        strategy : Strategy
            The strategy to load.

        """
        Condition.not_none(strategy, "strategy")

        cdef dict state = None

        if self._database is not None:
            state = self._database.load_strategy(strategy.id)

        if state:
            for key, value in state.items():
                self._log.debug(f"Loading {strategy.id}) state {{ {key}: {value} }}")

            strategy.load(state)
        else:
            self._log.info(f"No previous state found for {repr(strategy.id)}")

    cpdef Instrument load_instrument(self, InstrumentId instrument_id):
        """
        Load the instrument associated with the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.

        Returns
        -------
        Instrument or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Instrument instrument = self._instruments.get(instrument_id)

        if instrument is None and self._database is not None:
            instrument = self._database.load_instrument(instrument_id)

            if instrument is not None:
                self._instruments[instrument.id] = instrument

        return instrument

    cpdef SyntheticInstrument load_synthetic(self, InstrumentId instrument_id):
        """
        Load the synthetic instrument associated with the given `instrument_id` (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The synthetic instrument ID to load.

        Returns
        -------
        SyntheticInstrument or ``None``

        Raises
        ------
        ValueError
            If `instrument_id` is not a synthetic instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(instrument_id.is_synthetic(), "instrument_id was not a synthetic")

        cdef SyntheticInstrument synthetic = self._synthetics.get(instrument_id)

        if synthetic is None and self._database is not None:
            synthetic = self._database.load_synthetic(instrument_id)

            if synthetic is not None:
                self._synthetics[synthetic.id] = synthetic

        return synthetic

    cpdef Account load_account(self, AccountId account_id):
        """
        Load the account associated with the given account_id (if found).

        Parameters
        ----------
        account_id : AccountId
            The account ID to load.

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(account_id, "account_id")

        return self._accounts.get(account_id)

    cpdef Order load_order(self, ClientOrderId client_order_id):
        """
        Load the order associated with the given ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to load.

        Returns
        -------
        Order or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        return self._orders.get(client_order_id)

    cpdef Position load_position(self, PositionId position_id):
        """
        Load the position associated with the given ID (if found).

        Parameters
        ----------
        position_id : PositionId
            The position ID to load.

        Returns
        -------
        Position or ``None``

        """
        Condition.not_none(position_id, "position_id")

        return self._positions.get(position_id)

    cpdef void add(self, str key, bytes value):
        """
        Add the given general object `value` to the cache.

        The cache is agnostic to what the object actually is (and how it may
        be serialized), offering maximum flexibility.

        Parameters
        ----------
        key : str
            The cache key for the object.
        value : bytes
            The object value to write.

        """
        Condition.not_none(key, "key")
        Condition.not_none(value, "value")

        self._general[key] = value

        # Update database
        if self._database is not None:
            self._database.add(key, value)

    cpdef void add_order_book(self, OrderBook order_book):
        """
        Add the given order book to the cache.

        Parameters
        ----------
        order_book : OrderBook
            The order book to add.

        """
        Condition.not_none(order_book, "order_book")

        self._order_books[order_book.instrument_id] = order_book

    cpdef void add_own_order_book(self, own_order_book):
        """
        Add the given own order book to the cache.

        Parameters
        ----------
        own_order_book : nautilus_pyo3.OwnOrderBook
            The own order book to add.

        """
        Condition.not_none(own_order_book, "own_order_book")

        cdef InstrumentId instrument_id = InstrumentId.from_str(own_order_book.instrument_id.value)
        self._own_order_books[instrument_id] = own_order_book

    cpdef void add_quote_tick(self, QuoteTick tick):
        """
        Add the given quote tick to the cache.

        Parameters
        ----------
        tick : QuoteTick
            The tick to add.

        """
        Condition.not_none(tick, "tick")

        cdef InstrumentId instrument_id = tick.instrument_id
        ticks = self._quote_ticks.get(instrument_id)

        if not ticks:
            # The instrument_id was not registered
            ticks = deque(maxlen=self.tick_capacity)
            self._quote_ticks[instrument_id] = ticks

        ticks.appendleft(tick)

    cpdef void add_trade_tick(self, TradeTick tick):
        """
        Add the given trade tick to the cache.

        Parameters
        ----------
        tick : TradeTick
            The tick to add.

        """
        Condition.not_none(tick, "tick")

        cdef InstrumentId instrument_id = tick.instrument_id
        ticks = self._trade_ticks.get(instrument_id)

        if not ticks:
            # The instrument_id was not registered
            ticks = deque(maxlen=self.tick_capacity)
            self._trade_ticks[instrument_id] = ticks

        ticks.appendleft(tick)

    cpdef void add_mark_price(self, MarkPriceUpdate mark_price):
        """
        Add the given mark price update to the cache.

        Parameters
        ----------
        mark_price : MarkPriceUpdate
            The mark price update to add.

        """
        Condition.not_none(mark_price, "mark_price")

        mark_prices = self._mark_prices.get(mark_price.instrument_id)

        if not mark_prices:
            # The instrument_id was not registered
            mark_prices = deque(maxlen=self.tick_capacity)
            self._mark_prices[mark_price.instrument_id] = mark_prices

        mark_prices.appendleft(mark_price)

    cpdef void add_index_price(self, IndexPriceUpdate index_price):
        """
        Add the given index price update to the cache.

        Parameters
        ----------
        index_price : IndexPriceUpdate
            The index price update to add.

        """
        Condition.not_none(index_price, "index_price")

        index_prices = self._index_prices.get(index_price.instrument_id)

        if not index_prices:
            # The instrument_id was not registered
            index_prices = deque(maxlen=self.tick_capacity)
            self._index_prices[index_price.instrument_id] = index_prices

        index_prices.appendleft(index_price)

    cpdef void add_funding_rate(self, FundingRateUpdate funding_rate):
        """
        Add the given funding rate update to the cache.

        Parameters
        ----------
        funding_rate : FundingRateUpdate
            The funding rate update to add.

        """
        Condition.not_none(funding_rate, "funding_rate")

        self._funding_rates[funding_rate.instrument_id] = funding_rate

    cpdef void add_bar(self, Bar bar):
        """
        Add the given bar to the cache.

        Parameters
        ----------
        bar : Bar
            The bar to add.

        """
        Condition.not_none(bar, "bar")

        bars = self._bars.get(bar.bar_type)

        if not bars:
            # The bar type was not registered
            bars = deque(maxlen=self.bar_capacity)
            self._bars[bar.bar_type] = bars

        bars.appendleft(bar)
        cdef PriceType price_type = bar.bar_type.spec.price_type

        if price_type == PriceType.BID:
            self._bars_bid[bar.bar_type.instrument_id] = bar
        elif price_type == PriceType.ASK:
            self._bars_ask[bar.bar_type.instrument_id] = bar

    cpdef void add_quote_ticks(self, list ticks):
        """
        Add the given quotes to the cache.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The ticks to add.

        """
        Condition.not_none(ticks, "ticks")

        cdef int length = len(ticks)
        cdef InstrumentId instrument_id

        if length > 0:
            instrument_id = ticks[0].instrument_id
            self._log.debug(f"Received <QuoteTick[{length}]> data for {instrument_id}")
        else:
            self._log.debug("Received <QuoteTick[]> data with no ticks")
            return

        cached_ticks = self._quote_ticks.get(instrument_id)

        if not cached_ticks:
            # The instrument_id was not registered
            cached_ticks = deque(maxlen=self.tick_capacity)
            self._quote_ticks[instrument_id] = cached_ticks

        cdef QuoteTick tick

        for tick in ticks:
            if cached_ticks and tick.ts_event <= cached_ticks[0].ts_event:
                # Only add more recent data to cache
                continue

            cached_ticks.appendleft(tick)

    cpdef void add_trade_ticks(self, list ticks):
        """
        Add the given trades to the cache.

        Parameters
        ----------
        ticks : list[TradeTick]
            The ticks to add.

        """
        Condition.not_none(ticks, "ticks")

        cdef int length = len(ticks)
        cdef InstrumentId instrument_id

        if length > 0:
            instrument_id = ticks[0].instrument_id
            self._log.debug(f"Received <TradeTick[{length}]> data for {instrument_id}")
        else:
            self._log.debug("Received <TradeTick[]> data with no ticks")
            return

        cached_ticks = self._trade_ticks.get(instrument_id)

        if not cached_ticks:
            cached_ticks = deque(maxlen=self.tick_capacity)
            self._trade_ticks[instrument_id] = cached_ticks

        cdef TradeTick tick

        for tick in ticks:
            if cached_ticks and tick.ts_event <= cached_ticks[0].ts_event:
                # Only add more recent data to cache
                continue

            cached_ticks.appendleft(tick)

    cpdef void add_bars(self, list bars):
        """
        Add the given bars to the cache.

        Parameters
        ----------
        bars : list[Bar]
            The bars to add.

        """
        Condition.not_none(bars, "bars")

        cdef int length = len(bars)
        cdef BarType bar_type

        if length > 0:
            bar_type = bars[0].bar_type
            self._log.debug(f"Received <Bar[{length}]> data for {bar_type}")
        else:
            self._log.debug("Received <Bar[]> data with no ticks")
            return

        cached_bars = self._bars.get(bar_type)

        if not cached_bars:
            cached_bars = deque(maxlen=self.bar_capacity)
            self._bars[bar_type] = cached_bars

        cdef Bar bar

        for bar in bars:
            if cached_bars and bar.ts_event <= cached_bars[0].ts_event:
                # Only add more recent data to cache
                continue

            cached_bars.appendleft(bar)

        bar = bars[-1]
        cdef PriceType price_type = bar.bar_type.spec.price_type

        if price_type == PriceType.BID:
            self._bars_bid[bar.bar_type.instrument_id] = bar
        elif price_type == PriceType.ASK:
            self._bars_ask[bar.bar_type.instrument_id] = bar

    cpdef void add_currency(self, Currency currency):
        """
        Add the given currency to the cache.

        Parameters
        ----------
        currency : Currency
            The currency to add.

        """
        Condition.not_none(currency, "currency")

        self._currencies[currency.code] = currency
        Currency.register_c(currency, overwrite=False)

        self._log.debug(f"Added currency {currency.code}")

        # Update database
        if self._database is not None:
            self._database.add_currency(currency)

    cpdef void add_instrument(self, Instrument instrument):
        """
        Add the given instrument to the cache.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        self._instruments[instrument.id] = instrument

        if isinstance(instrument, (CurrencyPair, CryptoPerpetual)):
            self._xrate_symbols[instrument.id] = (
                f"{instrument.base_currency}/{instrument.quote_currency}"
            )

        self._log.debug(f"Added instrument {instrument.id}")

        # Update database
        if self._database is not None:
            self._database.add_instrument(instrument)

    cpdef void add_synthetic(self, SyntheticInstrument synthetic):
        """
        Add the given synthetic instrument to the cache.

        Parameters
        ----------
        synthetic : SyntheticInstrument
            The synthetic instrument to add.

        """
        self._synthetics[synthetic.id] = synthetic

        self._log.debug(f"Added synthetic instrument {synthetic.id}")

        # Update database
        if self._database is not None:
            self._database.add_synthetic(synthetic)

    cpdef void add_account(self, Account account):
        """
        Add the given account to the cache.

        Parameters
        ----------
        account : Account
            The account to add.

        Raises
        ------
        ValueError
            If `account_id` is already contained in the cache.

        """
        Condition.not_none(account, "account")
        Condition.not_in(account.id, self._accounts, "account.id", "cached_accounts")

        self._accounts[account.id] = account
        self._cache_venue_account_id(account.id)

        self._log.debug(f"Added Account(id={account.id.to_str()})")
        self._log.debug(f"Indexed {repr(account.id)}")

        # Update database
        if self._database is not None and self.persist_account_events:
            self._database.add_account(account)

    cpdef void add_venue_order_id(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        bint overwrite=False,
    ):
        """
        Index the given client order ID with the given venue order ID.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to index.
        venue_order_id : VenueOrderId
            The venue order ID to index.
        overwrite : bool, default False
            If the venue order ID will 'overwrite' any existing indexing and replace
            it in the cache. This is currently used for updated orders where the venue
            order ID may change.

        Raises
        ------
        ValueError
            If `overwrite` is False and the `client_order_id` is already indexed with a different `venue_order_id`.

        """
        Condition.not_none(client_order_id, "client_order_id")
        Condition.not_none(venue_order_id, "venue_order_id")

        cdef VenueOrderId existing_venue_order_id = self._index_client_order_ids.get(client_order_id)

        if not overwrite and existing_venue_order_id is not None and venue_order_id != existing_venue_order_id:
            raise ValueError(
                f"Existing {existing_venue_order_id!r} for {client_order_id!r} "
                f"did not match the given {venue_order_id!r}. "
                "If you are writing a test then try a different `venue_order_id`, "
                "otherwise this is probably a bug."
            )

        self._index_client_order_ids[client_order_id] = venue_order_id
        self._index_venue_order_ids[venue_order_id] = client_order_id

        self._log.debug(
            f"Indexed {client_order_id!r} with {venue_order_id!r}",
        )

    cpdef void add_order(
        self,
        Order order,
        PositionId position_id = None,
        ClientId client_id = None,
        bint overwrite = False,
    ):
        """
        Add the given order to the cache indexed with the given position
        ID.

        Parameters
        ----------
        order : Order
            The order to add.
        position_id : PositionId, optional
            The position ID to index for the order.
        client_id : ClientId, optional
            The execution client ID for order routing.
        overwrite : bool, default False
            If the added order should 'overwrite' any existing order and replace
            it in the cache. This is currently used for emulated orders which are
            being released and transformed into another type.

        Raises
        ------
        ValueError
            If `order.client_order_id` is already contained in the cache.

        """
        Condition.not_none(order, "order")

        if not overwrite:
            Condition.not_in(order.client_order_id, self._orders, "order.client_order_id", "_orders")
            Condition.not_in(order.client_order_id, self._index_orders, "order.client_order_id", "_index_orders")
            Condition.not_in(order.client_order_id, self._index_order_position, "order.client_order_id", "_index_order_position")
            Condition.not_in(order.client_order_id, self._index_order_strategy, "order.client_order_id", "_index_order_strategy")

        self._orders[order.client_order_id] = order
        self._index_orders.add(order.client_order_id)
        self._index_order_strategy[order.client_order_id] = order.strategy_id
        self._index_strategies.add(order.strategy_id)

        # Index: Venue -> set[ClientOrderId]
        cdef set venue_orders = self._index_venue_orders.get(order.instrument_id.venue)

        if not venue_orders:
            self._index_venue_orders[order.instrument_id.venue] = {order.client_order_id}
        else:
            venue_orders.add(order.client_order_id)

        # Index: InstrumentId -> set[ClientOrderId]
        cdef set instrument_orders = self._index_instrument_orders.get(order.instrument_id)

        if not instrument_orders:
            self._index_instrument_orders[order.instrument_id] = {order.client_order_id}
        else:
            instrument_orders.add(order.client_order_id)

        # Index: StrategyId -> set[ClientOrderId]
        cdef set strategy_orders = self._index_strategy_orders.get(order.strategy_id)

        if not strategy_orders:
            self._index_strategy_orders[order.strategy_id] = {order.client_order_id}
        else:
            strategy_orders.add(order.client_order_id)

        # Index: ExecAlgorithmId -> set[ClientOrderId]
        # Index: ClientOrderId -> set[ClientOrderId]
        cdef set exec_algorithm_orders
        cdef set exec_spawn_orders

        if order.exec_algorithm_id is not None:
            self._index_exec_algorithms.add(order.exec_algorithm_id)

            # Set exec_algorithm_orders index
            exec_algorithm_orders = self._index_exec_algorithm_orders.get(order.exec_algorithm_id)

            if not exec_algorithm_orders:
                self._index_exec_algorithm_orders[order.exec_algorithm_id] = {order.client_order_id}
            else:
                exec_algorithm_orders.add(order.client_order_id)

            # Set exec_spawn_id index
            exec_spawn_orders = self._index_exec_spawn_orders.get(order.exec_spawn_id)

            if not exec_spawn_orders:
                self._index_exec_spawn_orders[order.exec_spawn_id] = {order.client_order_id}
            else:
                self._index_exec_spawn_orders[order.exec_spawn_id].add(order.client_order_id)

        # Update emulation
        if order.emulation_trigger == TriggerType.NO_TRIGGER:
            self._index_orders_emulated.discard(order.client_order_id)
        else:
            self._index_orders_emulated.add(order.client_order_id)

        self._log.debug(f"Added {order}")

        if position_id is not None:
            # Index position ID
            self.add_position_id(
                position_id,
                order.instrument_id.venue,
                order.client_order_id,
                order.strategy_id,
            )

        # Index: ClientOrderId -> ClientId (execution client routing)
        if client_id is not None:
            self._index_order_client[order.client_order_id] = client_id
            self._log.debug(f"Indexed {client_id!r}")

        if self._database is None:
            return

        # Update database
        self._database.add_order(order, position_id, client_id)

    cpdef void add_order_list(self, OrderList order_list):
        """
        Add the given order list to the cache.

        Parameters
        ----------
        order_list : OrderList
            The order_list to add.

        Raises
        ------
        ValueError
            If `order_list.id` is already contained in the cache.

        """
        Condition.not_none(order_list, "order_list")
        Condition.not_in(order_list.id, self._order_lists, "order_list.id", "_order_lists")

        self._order_lists[order_list.id] = order_list
        self._log.debug(f"Added {order_list}")

    cpdef void add_position_id(
        self,
        PositionId position_id,
        Venue venue,
        ClientOrderId client_order_id,
        StrategyId strategy_id,
    ):
        """
        Index the given position ID with the other given IDs.

        Parameters
        ----------
        position_id : PositionId
            The position ID to index.
        venue : Venue
            The venue ID to index with the position ID.
        client_order_id : ClientOrderId
            The client order ID to index with the position ID.
        strategy_id : StrategyId
            The strategy ID to index with the position ID.

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(venue, "venue")
        Condition.not_none(client_order_id, "client_order_id")
        Condition.not_none(strategy_id, "strategy_id")

        # Index: ClientOrderId -> PositionId
        self._index_order_position[client_order_id] = position_id

        if self._database is not None:
            self._database.index_order_position(client_order_id, position_id)

        # Index: PositionId -> StrategyId
        self._index_position_strategy[position_id] = strategy_id

        # Index: PositionId -> set[ClientOrderId]
        cdef set position_orders = self._index_position_orders.get(position_id)

        if not position_orders:
            self._index_position_orders[position_id] = {client_order_id}
        else:
            position_orders.add(client_order_id)

        # Index: StrategyId -> set[PositionId]
        cdef set strategy_positions = self._index_strategy_positions.get(strategy_id)

        if not strategy_positions:
            self._index_strategy_positions[strategy_id] = {position_id}
        else:
            strategy_positions.add(position_id)

        self._log.debug(
            f"Indexed {position_id!r}, "
            f"client_order_id={client_order_id}, "
            f"strategy_id={strategy_id})",
        )

    cpdef void add_position(self, Position position, OmsType oms_type):
        """
        Add the given position to the cache.

        Parameters
        ----------
        position : Position
            The position to add.
        oms_type : OmsType
            The order management system type for the position.

        Raises
        ------
        ValueError
            If `oms_type` is ``HEDGING`` and a virtual `position.id` is already contained in the cache.

        """
        Condition.not_none(position, "position")

        if oms_type == OmsType.HEDGING and position.id.is_virtual_c():
            Condition.not_in(position.id, self._positions, "position.id", "_positions")
            Condition.not_in(position.id, self._index_positions, "position.id", "_index_positions")
            Condition.not_in(position.id, self._index_positions_open, "position.id", "_index_positions_open")

        self._positions[position.id] = position
        self._index_positions.add(position.id)
        self._index_positions_open.add(position.id)

        self.add_position_id(
            position.id,
            position.instrument_id.venue,
            position.opening_order_id,
            position.strategy_id,
        )

        # Index: Venue -> set[PositionId]
        cdef Venue venue = position.instrument_id.venue
        cdef set venue_positions = self._index_venue_positions.get(venue)

        if not venue_positions:
            self._index_venue_positions[venue] = {position.id}
        else:
            venue_positions.add(position.id)

        # Index: InstrumentId -> set[PositionId]
        cdef InstrumentId instrument_id = position.instrument_id
        cdef set instrument_positions = self._index_instrument_positions.get(instrument_id)

        if not instrument_positions:
            self._index_instrument_positions[instrument_id] = {position.id}
        else:
            instrument_positions.add(position.id)

        self._log.debug(f"Added Position(id={position.id.to_str()}, strategy_id={position.strategy_id.to_str()})")

        if self._database is None:
            return

        # Update database
        self._database.add_position(position)

    cpdef void add_greeks(self, object greeks):
        """
        Add greeks to the cache.

        Parameters
        ----------
        greeks : GreeksData
            The greeks to add.

        """
        self._greeks[greeks.instrument_id] = greeks

    cpdef void add_yield_curve(self, object yield_curve):
        """
        Add a yield curve to the cache.

        Parameters
        ----------
        yield_curve : YieldCurveData
            The yield curve to add.

        """
        self._yield_curves[yield_curve.curve_name] = yield_curve

    cpdef object greeks(self, InstrumentId instrument_id):
        """
        Return the latest cached greeks for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to get the greeks for.

        Returns
        -------
        GreeksData
            The greeks for the given instrument ID.

        """
        return self._greeks.get(instrument_id)

    cpdef object yield_curve(self, str curve_name):
        """
        Return the latest cached yield curve for the given curve name.

        Parameters
        ----------
        curve_name : str
            The name of the yield curve to get.

        Returns
        -------
        YieldCurveData
            The interest rate curve for the given currency.

        """
        return self._yield_curves.get(curve_name)

    cpdef void snapshot_position(self, Position position):
        """
        Snapshot the given position in its current state.

        The position ID will be appended with a UUID v4 string.

        Parameters
        ----------
        position : Position
            The position to snapshot.

        """
        cdef PositionId position_id = position.id
        cdef list[bytes] snapshots = self._position_snapshots.get(position_id)

        # Create snapshot with modified position ID
        cdef Position copied_position = pickle.loads(pickle.dumps(position))
        copied_position.id = PositionId(f"{position_id.to_str()}-{uuid.uuid4()}")
        cdef bytes position_pickled = pickle.dumps(copied_position)

        if snapshots is not None:
            snapshots.append(position_pickled)
        else:
            self._position_snapshots[position_id] = [position_pickled]

        # Update snapshot index
        cdef InstrumentId instrument_id = position.instrument_id
        cdef set position_ids = self._index_instrument_position_snapshots.get(instrument_id)
        if position_ids is not None:
            position_ids.add(position_id)
        else:
            self._index_instrument_position_snapshots[instrument_id] = {position_id}

        self._log.debug(f"Snapshot {repr(copied_position)}")

    cpdef void snapshot_position_state(
        self,
        Position position,
        uint64_t ts_snapshot,
        Money unrealized_pnl=None,
        bint open_only=True,
    ):
        """
        Snapshot the state dictionary for the given `position`.

        This method will persist to the backing cache database.

        Parameters
        ----------
        position : Position
            The position to snapshot the state for.
        ts_snapshot : uint64_t
            UNIX timestamp (nanoseconds) when the snapshot was taken.
        unrealized_pnl : Money, optional
            The current unrealized PnL for the position.
        open_only : bool, default True
            If only open positions should be snapshot, this flag helps to avoid race conditions
            where a position is snapshot when no longer open.

        """
        Condition.not_none(position, "position")

        if open_only and not position.is_open_c():
            return  # Only snapshot open positions

        if self._database is None:
            self._log.warning(
                "Cannot snapshot position state for {position.id:r!} (no database configured)",
            )
            return

        self._database.snapshot_position_state(
            position,
            ts_snapshot,
            unrealized_pnl,
        )

    cpdef void snapshot_order_state(self, Order order):
        """
        Snapshot the state dictionary for the given `order`.

        This method will persist to the backing cache database.

        Parameters
        ----------
        order : Order
            The order to snapshot the state for.

        """
        Condition.not_none(order, "order")

        if self._database is None:
            self._log.warning(
                "Cannot snapshot order state for {order.client_order_id:r!} (no database configured)",
            )
            return

        self._database.snapshot_order_state(order)

    cpdef void update_account(self, Account account):
        """
        Update the given account in the cache.

        Parameters
        ----------
        account : The account to update (from last event).

        """
        Condition.not_none(account, "account")

        # Update database
        if self._database is not None and self.persist_account_events:
            self._database.update_account(account)

    cpdef void update_order(self, Order order):
        """
        Update the given order in the cache.

        Parameters
        ----------
        order : Order
            The order to update (from last event).

        """
        Condition.not_none(order, "order")

        # Update venue order ID
        if order.venue_order_id is not None and order.venue_order_id not in self._index_venue_order_ids:
            # If the order is being modified then we allow a changing `VenueOrderId` to accommodate
            # venues which use a cancel+replace update strategy.
            self.add_venue_order_id(
                order.client_order_id,
                order.venue_order_id,
                overwrite=isinstance(order._events[-1], OrderUpdated),
            )

        # Update in-flight state
        if order.is_inflight_c():
            self._index_orders_inflight.add(order.client_order_id)
        else:
            self._index_orders_inflight.discard(order.client_order_id)

        # Update open/closed state
        if order.is_open_c():
            self._index_orders_closed.discard(order.client_order_id)
            self._index_orders_open.add(order.client_order_id)

            if self._own_order_books:
                self._index_orders_open_pyo3.add(nautilus_pyo3.ClientOrderId(order.client_order_id.value))
        elif order.is_closed_c():
            self._index_orders_open.discard(order.client_order_id)
            self._index_orders_pending_cancel.discard(order.client_order_id)
            self._index_orders_closed.add(order.client_order_id)

            if self._own_order_books:
                self._index_orders_open_pyo3.discard(nautilus_pyo3.ClientOrderId(order.client_order_id.value))

        # Update emulation
        if order.is_closed_c() or order.emulation_trigger == TriggerType.NO_TRIGGER:
            self._index_orders_emulated.discard(order.client_order_id)
        else:
            self._index_orders_emulated.add(order.client_order_id)

        # Update own book
        if self._own_order_books:
            own_book = self._own_order_books.get(order.instrument_id)
            # Only bypass should_handle check for closed orders (to ensure cleanup)
            if (own_book is not None and order.is_closed_c()) or should_handle_own_book_order(order):
                self.update_own_order_book(order)

        if self._database is None:
            return

        # Update database
        self._database.update_order(order)

    cpdef void update_order_pending_cancel_local(self, Order order):
        """
        Update the given `order` as pending cancel locally.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        Condition.not_none(order, "order")

        self._index_orders_pending_cancel.add(order.client_order_id)

    cpdef void update_own_order_book(self, Order order):
        """
        Update the own order book for the given order.

        Orders without prices (MARKET, etc.) are skipped as they cannot be
        represented in own books.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        Condition.not_none(order, "order")

        if not order.has_price_c():
            return

        own_book = self._own_order_books.get(order.instrument_id)

        if own_book is None:
            if order.is_closed_c():
                # Don't create own book for closed orders
                return
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
            own_book = nautilus_pyo3.OwnOrderBook(pyo3_instrument_id)
            self._own_order_books[order.instrument_id] = own_book
            self._log.debug(f"Initialized {own_book!r}", LogColor.MAGENTA)

        own_book_order = order.to_own_book_order()

        if order.is_closed_c():
            try:
                own_book.delete(own_book_order)
                self._log.debug(f"Deleted order {order.client_order_id} from own book")
            except RuntimeError as e:
                self._log.debug(f"Failed to delete order {order.client_order_id} from own book: {e}")
        else:
            try:
                own_book.update(own_book_order)
            except RuntimeError as e:
                self._log.debug(f"Failed to update order {order.client_order_id} in own book: {e}; inserting instead")
                own_book.add(own_book_order)
            self._log.debug(f"Updated order {order.client_order_id} in own book")

    cpdef void update_position(self, Position position):
        """
        Update the given position in the cache.

        Parameters
        ----------
        position : Position
            The position to update (from last event).

        """
        Condition.not_none(position, "position")

        if position.is_open_c():
            self._index_positions_open.add(position.id)
            self._index_positions_closed.discard(position.id)
        elif position.is_closed_c():
            self._index_positions_closed.add(position.id)
            self._index_positions_open.discard(position.id)

        if self._database is None:
            return

        # Update database
        self._database.update_position(position)

    cpdef void update_actor(self, Actor actor):
        """
        Update the given actor state in the cache.

        Parameters
        ----------
        actor : Actor
            The actor to update.
        """
        Condition.not_none(actor, "actor")

        self._index_actors.add(actor.id)

        # Update database
        if self._database is not None:
            self._database.update_actor(actor)

    cpdef void update_strategy(self, Strategy strategy):
        """
        Update the given strategy state in the cache.

        Parameters
        ----------
        strategy : Strategy
            The strategy to update.
        """
        Condition.not_none(strategy, "strategy")

        self._index_strategies.add(strategy.id)

        # Update database
        if self._database is not None:
            self._database.update_strategy(strategy)

    cpdef void delete_actor(self, Actor actor):
        """
        Delete the given actor from the cache.

        Parameters
        ----------
        actor : Actor
            The actor to deregister.

        Raises
        ------
        ValueError
            If `actor` is not contained in the actors index.

        """
        Condition.not_none(actor, "actor")
        Condition.is_in(actor.id, self._index_actors, "actor.id", "actors")

        self._index_actors.discard(actor.id)

        # Update database
        if self._database is not None:
            self._database.delete_actor(actor.id)
            self._log.debug(f"Deleted Actor(id={actor.id.value})")

    cpdef void delete_strategy(self, Strategy strategy):
        """
        Delete the given strategy from the cache.

        Parameters
        ----------
        strategy : Strategy
            The strategy to deregister.

        Raises
        ------
        ValueError
            If `strategy` is not contained in the strategies index.

        """
        Condition.not_none(strategy, "strategy")
        Condition.is_in(strategy.id, self._index_strategies, "strategy.id", "strategies")

        self._index_strategies.discard(strategy.id)

        if strategy.id in self._index_strategy_orders:
            del self._index_strategy_orders[strategy.id]

        if strategy.id in self._index_strategy_positions:
            del self._index_strategy_positions[strategy.id]

        # Update database
        if self._database is not None:
            self._database.delete_strategy(strategy.id)
            self._log.debug(f"Deleted Strategy(id={strategy.id.value})")

# -- DATA QUERIES ---------------------------------------------------------------------------------

    cpdef bytes get(self, str key):
        """
        Return the general object for the given `key`.

        The cache is agnostic to what the object actually is (and how it may
        be serialized), offering maximum flexibility.

        Parameters
        ----------
        key : str
            The cache key for the object.

        Returns
        -------
        bytes or ``None``

        """
        Condition.not_none(key, "key")

        return self._general.get(key)

    cpdef list quote_ticks(self, InstrumentId instrument_id):
        """
        Return the quotes for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks to get.

        Returns
        -------
        list[QuoteTick]

        """
        Condition.not_none(instrument_id, "instrument_id")

        return list(self._quote_ticks.get(instrument_id, []))

    cpdef list trade_ticks(self, InstrumentId instrument_id):
        """
        Return trades for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks to get.

        Returns
        -------
        list[TradeTick]

        """
        Condition.not_none(instrument_id, "instrument_id")

        return list(self._trade_ticks.get(instrument_id, []))

    cpdef list mark_prices(self, InstrumentId instrument_id):
        """
        Return mark prices for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark prices to get.

        Returns
        -------
        list[MarkPriceUpdate]

        """
        Condition.not_none(instrument_id, "instrument_id")

        return list(self._mark_prices.get(instrument_id, []))

    cpdef list index_prices(self, InstrumentId instrument_id):
        """
        Return index prices for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark prices to get.

        Returns
        -------
        list[IndexPriceUpdate]

        """
        Condition.not_none(instrument_id, "instrument_id")

        return list(self._index_prices.get(instrument_id, []))

    cpdef list bars(self, BarType bar_type):
        """
        Return bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for bars to get.

        Returns
        -------
        list[Bar]

        """
        Condition.not_none(bar_type, "bar_type")

        return list(self._bars.get(bar_type, []))

    cpdef Price price(self, InstrumentId instrument_id, PriceType price_type):
        """
        Return the price for the given instrument ID and price type.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.
        price_type : PriceType
            The price type for the query.

        Returns
        -------
        Price or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef TradeTick trade_tick
        cdef QuoteTick quote_tick

        if price_type == PriceType.LAST:
            trade_tick = self.trade_tick(instrument_id)

            if trade_tick is not None:
                return trade_tick.price
        elif price_type == PriceType.BID or price_type == PriceType.ASK or price_type == PriceType.MID:
            quote_tick = self.quote_tick(instrument_id)

            if quote_tick is not None:
                return quote_tick.extract_price(price_type)
        elif price_type == PriceType.MARK:
            mark_price = self.mark_price(instrument_id)

            if mark_price is not None:
                return mark_price.value

        # Fall back to bar pricing for bid, ask and last
        cdef Bar bar
        cdef list bar_types = self.bar_types(instrument_id, price_type, AggregationSource.EXTERNAL)

        if bar_types:
            bar = self.bar(bar_types[0])  # Bar with smallest timedelta

            if bar is not None:
                return bar.close

    cpdef dict[InstrumentId, Price] prices(self, PriceType price_type):
        """
        Return a map of latest prices per instrument ID for the given price type.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.
        price_type : PriceType
            The price type for the query.

        Returns
        -------
        dict[InstrumentId, Price]
            Includes key value pairs for prices which exist.

        """
        cdef set[InstrumentId] instrument_ids = {b.instrument_id for b in self._bars.keys()}

        if price_type == PriceType.LAST:
            instrument_ids.update(self._trade_ticks.keys())
        elif price_type == PriceType.BID or price_type == PriceType.ASK or price_type == PriceType.MID:
            instrument_ids.update(self._quote_ticks.keys())
        elif price_type == PriceType.MARK:
            instrument_ids.update(self._mark_prices.keys())
        else:
            # Unreachable unless code changes
            raise ValueError(f"Invalid `PriceType`, was {price_type}")

        cdef dict[InstrumentId, Price] prices_map = {}

        cdef:
            InstrumentId instrument_id
            Price price
        for instrument_id in sorted(instrument_ids):
            price = self.price(instrument_id, price_type)

            if price is not None:
                prices_map[instrument_id] = price

        return prices_map

    cpdef OrderBook order_book(self, InstrumentId instrument_id):
        """
        Return the order book for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the order book to get.

        Returns
        -------
        OrderBook or ``None``
            If book not found for the instrument ID then returns ``None``.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._order_books.get(instrument_id)

    cpdef object own_order_book(self, InstrumentId instrument_id):
        """
        Return the own order book for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the own order book to get.
            Note this is the standard Cython `InstrumentId`.

        Returns
        -------
        nautilus_pyo3.OwnOrderBook or ``None``
            If own book not found for the instrument ID then returns ``None``.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._own_order_books.get(instrument_id)

    cpdef dict[Decimal, list[Order]] own_bid_orders(
        self,
        InstrumentId instrument_id,
        set[OrderStatus] status = None,
        uint64_t accepted_buffer_ns = 0,
        uint64_t ts_now = 0,
    ):
        """
        Return own bid orders for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the own orders to get.
            Note this is the standard Cython `InstrumentId`.
        status : set[OrderStatus], optional
            The order status to filter for. Empty price levels after filtering are excluded from the result.
        accepted_buffer_ns : uint64_t, optional
            The minimum time in nanoseconds that must have elapsed since the order was accepted.
            Orders accepted less than this time ago will be filtered out.
        ts_now : uint64_t, optional
            The current time in nanoseconds. Required if accepted_buffer_ns > 0.

        Returns
        -------
        dict[Decimal, list[Order]] or ``None``
            If own book not found for the instrument ID then returns ``None``.

        Raises
        ------
        ValueError
            If `accepted_buffer_ns` > 0 and `ts_now` == 0.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if accepted_buffer_ns > 0 and ts_now == 0:
            raise ValueError("ts_now must be provided when accepted_buffer_ns > 0")

        own_order_book = self._own_order_books.get(instrument_id)
        if own_order_book is None:
            return None

        return process_own_order_map(
            own_order_book.bids_to_dict(
                {order_status_to_pyo3(s) for s in status} if status is not None else None,
                accepted_buffer_ns if accepted_buffer_ns > 0 else None,
                ts_now if ts_now > 0 else None,
            ),
            self._orders,
        )

    cpdef dict[Decimal, list[Order]] own_ask_orders(
        self,
        InstrumentId instrument_id,
        set[OrderStatus] status = None,
        uint64_t accepted_buffer_ns = 0,
        uint64_t ts_now = 0,
    ):
        """
        Return own ask orders for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the own orders to get.
            Note this is the standard Cython `InstrumentId`.
        status : set[OrderStatus], optional
            The order status to filter for. Empty price levels after filtering are excluded from the result.
        accepted_buffer_ns : uint64_t, optional
            The minimum time in nanoseconds that must have elapsed since the order was accepted.
            Orders accepted less than this time ago will be filtered out.
        ts_now : uint64_t, optional
            The current time in nanoseconds. Required if accepted_buffer_ns > 0.

        Returns
        -------
        dict[Decimal, list[Order]] or ``None``
            If own book not found for the instrument ID then returns ``None``.

        Raises
        ------
        ValueError
            If `accepted_buffer_ns` > 0 and `ts_now` == 0.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if accepted_buffer_ns > 0 and ts_now == 0:
            raise ValueError("ts_now must be provided when accepted_buffer_ns > 0")

        own_order_book = self._own_order_books.get(instrument_id)
        if own_order_book is None:
            return None

        return process_own_order_map(
            own_order_book.asks_to_dict(
                {order_status_to_pyo3(s) for s in status} if status is not None else None,
                accepted_buffer_ns if accepted_buffer_ns > 0 else None,
                ts_now if ts_now > 0 else None,
            ),
            self._orders,
        )

    cpdef QuoteTick quote_tick(self, InstrumentId instrument_id, int index = 0):
        """
        Return the quote tick for the given instrument ID at the given index (if found).

        Last quote tick if no index specified.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the tick to get.
        index : int, optional
            The index for the tick to get.

        Returns
        -------
        QuoteTick or ``None``
            If no ticks or no tick at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent tick at index 0).

        """
        Condition.not_none(instrument_id, "instrument_id")

        ticks = self._quote_ticks.get(instrument_id)

        if not ticks:
            return None

        try:
            return ticks[index]
        except IndexError:
            return None

    cpdef TradeTick trade_tick(self, InstrumentId instrument_id, int index = 0):
        """
        Return the trade tick for the given instrument ID at the given index (if found).

        Last trade tick if no index specified.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the tick to get.
        index : int, optional
            The index for the tick to get.

        Returns
        -------
        TradeTick or ``None``
            If no ticks or no tick at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent tick at index 0).

        """
        Condition.not_none(instrument_id, "instrument_id")

        ticks = self._trade_ticks.get(instrument_id)

        if not ticks:
            return None

        try:
            return ticks[index]
        except IndexError:
            return None

    cpdef MarkPriceUpdate mark_price(self, InstrumentId instrument_id, int index = 0):
        """
        Return the mark price for the given instrument ID at the given index (if found).

        Last mark price if no index specified.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark price to get.
        index : int, optional
            The index for the mark price to get.

        Returns
        -------
        MarkPriceUpdate or ``None``
            If no mark prices or no mark price at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent mark price at index 0).

        """
        Condition.not_none(instrument_id, "instrument_id")

        mark_prices = self._mark_prices.get(instrument_id)

        if not mark_prices:
            return None

        try:
            return mark_prices[index]
        except IndexError:
            return None

    cpdef IndexPriceUpdate index_price(self, InstrumentId instrument_id, int index = 0):
        """
        Return the index price for the given instrument ID at the given index (if found).

        Last index price if no index specified.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the index price to get.
        index : int, optional
            The index for the index price to get.

        Returns
        -------
        IndexPriceUpdate or ``None``
            If no index prices or no index price at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent index price at index 0).

        """
        Condition.not_none(instrument_id, "instrument_id")

        index_prices = self._index_prices.get(instrument_id)

        if not index_prices:
            return None

        try:
            return index_prices[index]
        except IndexError:
            return None

    cpdef FundingRateUpdate funding_rate(self, InstrumentId instrument_id):
        """
        Return the funding rate for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the funding rate to get.

        Returns
        -------
        FundingRateUpdate or ``None``
            If no funding rate then returns ``None``.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._funding_rates.get(instrument_id)

    cpdef Bar bar(self, BarType bar_type, int index = 0):
        """
        Return the bar for the given bar type at the given index (if found).

        Last bar if no index specified.

        Parameters
        ----------
        bar_type : BarType
            The bar type to get.
        index : int, optional
            The index for the bar to get.

        Returns
        -------
        Bar or ``None``
            If no bars or no bar at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent bar at index 0).

        """
        Condition.not_none(bar_type, "bar_type")

        bars = self._bars.get(bar_type)

        if not bars:
            return None

        try:
            return bars[index]
        except IndexError:
            return None

    cpdef int book_update_count(self, InstrumentId instrument_id):
        """
        The count of order book updates for the given instrument ID.

        Will return zero if there is no book for the instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the book.

        Returns
        -------
        int

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderBook book = self._order_books.get(instrument_id)

        if book is None:
            return 0
        else:
            return book.update_count

    cpdef int quote_tick_count(self, InstrumentId instrument_id):
        """
        The count of quotes for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks.

        Returns
        -------
        int

        """
        Condition.not_none(instrument_id, "instrument_id")

        return len(self._quote_ticks.get(instrument_id, []))

    cpdef int trade_tick_count(self, InstrumentId instrument_id):
        """
        The count of trades for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks.

        Returns
        -------
        int

        """
        Condition.not_none(instrument_id, "instrument_id")

        return len(self._trade_ticks.get(instrument_id, []))

    cpdef int mark_price_count(self, InstrumentId instrument_id):
        """
        The count of mark prices for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark prices.

        Returns
        -------
        int

        """
        Condition.not_none(instrument_id, "instrument_id")

        return len(self._mark_prices.get(instrument_id, []))

    cpdef int index_price_count(self, InstrumentId instrument_id):
        """
        The count of index prices for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the index prices.

        Returns
        -------
        int

        """
        Condition.not_none(instrument_id, "instrument_id")

        return len(self._index_prices.get(instrument_id, []))

    cpdef int bar_count(self, BarType bar_type):
        """
        The count of bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to count.

        Returns
        -------
        int

        """
        Condition.not_none(bar_type, "bar_type")

        return len(self._bars.get(bar_type, []))

    cpdef bint has_order_book(self, InstrumentId instrument_id):
        """
        Return a value indicating whether the cache has an order book snapshot
        for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the order book snapshot.

        Returns
        -------
        bool

        """
        return instrument_id in self._order_books

    cpdef bint has_quote_ticks(self, InstrumentId instrument_id):
        """
        Return a value indicating whether the cache has quotes for the
        given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks.

        Returns
        -------
        bool

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self.quote_tick_count(instrument_id) > 0

    cpdef bint has_trade_ticks(self, InstrumentId instrument_id):
        """
        Return a value indicating whether the cache has trades for the
        given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks.

        Returns
        -------
        bool

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self.trade_tick_count(instrument_id) > 0

    cpdef bint has_mark_prices(self, InstrumentId instrument_id):
        """
        Return a value indicating whether the cache has mark prices for the
        given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark prices.

        Returns
        -------
        bool

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self.mark_price_count(instrument_id) > 0

    cpdef bint has_index_prices(self, InstrumentId instrument_id):
        """
        Return a value indicating whether the cache has index prices for the
        given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the index prices.

        Returns
        -------
        bool

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self.index_price_count(instrument_id) > 0

    cpdef bint has_bars(self, BarType bar_type):
        """
        Return a value indicating whether the cache has bars for the given bar
        type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the bars.

        Returns
        -------
        bool

        """
        Condition.not_none(bar_type, "bar_type")

        return self.bar_count(bar_type) > 0

    cpdef get_xrate(
        self,
        Venue venue,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=PriceType.MID,
    ):
        """
        Return the calculated exchange rate.

        If the exchange rate cannot be calculated then returns ``None``.

        Parameters
        ----------
        venue : Venue
            The venue for the exchange rate.
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.
        price_type : PriceType
            The price type for the exchange rate.

        Returns
        -------
        float or ``None``

        Raises
        ------
        ValueError
            If `price_type` is ``LAST`` or ``MARK``.

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")

        if from_currency == to_currency:
            # When the source and target currencies are identical,
            # no conversion is needed; return an exchange rate of 1.0.
            return 1.0

        cdef tuple quotes = self._build_quote_table(venue)

        try:
            return nautilus_pyo3.get_exchange_rate(
                from_currency=from_currency.code,
                to_currency=to_currency.code,
                price_type=nautilus_pyo3.PriceType.from_int(price_type),
                quotes_bid=quotes[0],  # Bid
                quotes_ask=quotes[1],  # Ask
            )
        except ValueError as e:
            self._log.error(f"Cannot calculate exchange rate: {e!r}")

    cdef tuple _build_quote_table(self, Venue venue):
        cdef dict bid_quotes = {}
        cdef dict ask_quotes = {}

        cdef:
            InstrumentId instrument_id
            str base_quote
            Price bid_price
            Price ask_price
            Bar bid_bar
            Bar ask_bar
        for instrument_id, base_quote in self._xrate_symbols.items():
            if instrument_id.venue != venue:
                continue

            ticks = self._quote_ticks.get(instrument_id)

            if ticks:
                bid_price = ticks[0].bid_price
                ask_price = ticks[0].ask_price
            else:
                # No quotes for instrument_id
                bid_bar = self._bars_bid.get(instrument_id)
                ask_bar = self._bars_ask.get(instrument_id)

                if bid_bar is None or ask_bar is None:
                    continue # No prices for instrument_id

                bid_price = bid_bar.close
                ask_price = ask_bar.close

            bid_quotes[base_quote] = bid_price.as_f64_c()
            ask_quotes[base_quote] = ask_price.as_f64_c()

        return bid_quotes, ask_quotes

    cpdef get_mark_xrate(
        self,
        Currency from_currency,
        Currency to_currency,
    ):
        """
        Return the exchange rate based on mark price.

        Will return ``None`` if an exchange rate has not been set.

        Parameters
        ----------
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.

        Returns
        -------
        float or ``None``

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")

        if from_currency == to_currency:
            # When the source and target currencies are identical,
            # no conversion is needed; return an exchange rate of 1.0.
            return 1.0

        cdef tuple[Currency, Currency] key = (from_currency, to_currency)
        return self._mark_xrates.get(key)

    cpdef void set_mark_xrate(
        self,
        Currency from_currency,
        Currency to_currency,
        double xrate,
    ):
        """
        Set the exchange rate based on mark price.

        Will also set the inverse xrate automatically.

        Parameters
        ----------
        from_currency : Currency
            The base currency for the exchange rate to set.
        to_currency : Currency
            The quote currency for the exchange rate to set.
        xrate : double
            The exchange rate based on mark price.

        Raises
        ------
        ValueError
            If `xrate` is zero.

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")
        Condition.not_equal(xrate, 0.0, "xrate", "zero")

        self._mark_xrates[(from_currency, to_currency)] = xrate
        self._mark_xrates[(to_currency, from_currency)] = 1.0 / xrate

    cpdef void clear_mark_xrate(
        self,
        Currency from_currency,
        Currency to_currency,
    ):
        """
        Clear the exchange rate based on mark price.

        Parameters
        ----------
        from_currency : Currency
            The base currency for the exchange rate to clear.
        to_currency : Currency
            The quote currency for the exchange rate to clear.

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")

        self._mark_xrates.pop((from_currency, to_currency), None)

    cpdef void clear_mark_xrates(self):
        """
        Clear the exchange rates based on mark price.

        """
        self._mark_xrates.clear()

# -- INSTRUMENT QUERIES ---------------------------------------------------------------------------

    cpdef Instrument instrument(self, InstrumentId instrument_id):
        """
        Return the instrument corresponding to the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID of the instrument to return.

        Returns
        -------
        Instrument or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._instruments.get(instrument_id)

    cpdef list instrument_ids(self, Venue venue = None):
        """
        Return all instrument IDs held by the cache.

        Parameters
        ----------
        venue : Venue, optional
            The venue filter for the query.

        Returns
        -------
        list[InstrumentId]

        """
        cdef InstrumentId x

        return sorted([x for x in self._instruments.keys() if venue is None or venue == x.venue])

    cpdef list instruments(self, Venue venue = None, str underlying = None):
        """
        Return all instruments held by the cache.

        Parameters
        ----------
        venue : Venue, optional
            The venue filter for the query.
        underlying : str, optional
            The underlying root symbol for the query.

        Returns
        -------
        list[Instrument]

        """
        cdef Instrument x

        return [
            x for x in self._instruments.values()
            if (venue is None or venue == x.id.venue) and
            (underlying is None or (hasattr(x, "underlying") and underlying == x.underlying))
        ]

    cdef timedelta _get_timedelta(self, BarType bar_type):
        # Helper method to get the timedelta from a BarType
        cdef BarSpecification bar_spec = bar_type.spec

        if bar_spec.aggregation == BarAggregation.MONTH:
            return timedelta(days=bar_spec.step * 30)  # Reasonable value to fix sorting

        return bar_spec.timedelta

    cpdef list bar_types(
        self,
        InstrumentId instrument_id = None,
        object price_type = None,
        aggregation_source = None,
    ):
        """
        Return all bar types with the given query filters.

        If a filter parameter is ``None``, then no filtering occurs for that parameter.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        price_type : PriceType, optional
            The price type query filter.
        aggregation_source : AggregationSource, optional
            The aggregation source query filter.

        Returns
        -------
        list[BarType]

        """
        Condition.type_or_none(instrument_id, InstrumentId, "instrument_id")
        Condition.type_or_none(price_type, PriceType_py, "price_type")

        cdef list[BarType] bar_types = list(self._bars.keys())

        if instrument_id is not None:
            bar_types = [bar_type for bar_type in bar_types if bar_type.instrument_id == instrument_id]

        if price_type is not None:
            bar_types = [bar_type for bar_type in bar_types if bar_type.spec.price_type == price_type]

        if aggregation_source is not None:
            bar_types = [bar_type for bar_type in bar_types if bar_type.aggregation_source == aggregation_source]

        if instrument_id and price_type:
            bar_types.sort(key=self._get_timedelta)

        return bar_types

# -- SYNTHETIC QUERIES ----------------------------------------------------------------------------

    cpdef SyntheticInstrument synthetic(self, InstrumentId instrument_id):
        """
        Return the synthetic instrument corresponding to the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID of the synthetic instrument to return.

        Returns
        -------
        SyntheticInstrument or ``None``

        Raises
        ------
        ValueError
            If `instrument_id` is not a synthetic instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(instrument_id.is_synthetic(), "instrument_id was not a synthetic")

        return self._synthetics.get(instrument_id)

    cpdef list synthetic_ids(self):
        """
        Return all synthetic instrument IDs held by the cache.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(self._synthetics.keys())

    cpdef list synthetics(self):
        """
        Return all synthetic instruments held by the cache.

        Returns
        -------
        list[SyntheticInstrument]

        """
        return list(self._synthetics.values())

# -- ACCOUNT QUERIES ------------------------------------------------------------------------------

    cpdef Account account(self, AccountId account_id):
        """
        Return the account matching the given ID (if found).

        Parameters
        ----------
        account_id : AccountId
            The account ID.

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(account_id, "account_id")

        return self._accounts.get(account_id)

    cpdef Account account_for_venue(self, Venue venue):
        """
        Return the account matching the given client ID (if found).

        If unique_venue is set, it will be used instead of the provided venue.

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(venue, "venue")

        # Use unique_venue if it's set
        cdef Venue used_venue = self._specific_venue or venue
        cdef AccountId account_id = self._index_venue_account.get(used_venue)

        if account_id is None:
            return None

        return self._accounts.get(account_id)

    cpdef AccountId account_id(self, Venue venue):
        """
        Return the account ID for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the account ID.

        Returns
        -------
        AccountId or ``None``

        """
        Condition.not_none(venue, "venue")

        return self._index_venue_account.get(venue)

    cpdef list accounts(self):
        """
        Return all accounts in the cache.

        Returns
        -------
        list[Account]

        """
        return list(self._accounts.values())

# -- IDENTIFIER QUERIES ---------------------------------------------------------------------------

    cdef set _build_order_query_filter_set(
        self,
        Venue venue,
        InstrumentId instrument_id,
        StrategyId strategy_id,
    ):
        cdef set query = None

        # Build potential query set
        if venue is not None:
            query = self._index_venue_orders.get(venue, set())

        if instrument_id is not None:
            if query is None:
                query = self._index_instrument_orders.get(instrument_id, set())
            else:
                query = query.intersection(self._index_instrument_orders.get(instrument_id, set()))

        if strategy_id is not None:
            if query is None:
                query = self._index_strategy_orders.get(strategy_id, set())
            else:
                query = query.intersection(self._index_strategy_orders.get(strategy_id, set()))

        return query

    cdef set _build_position_query_filter_set(
        self,
        Venue venue,
        InstrumentId instrument_id,
        StrategyId strategy_id,
    ):
        cdef set query = None

        # Build potential query set
        if venue is not None:
            query = self._index_venue_positions.get(venue, set())

        if instrument_id is not None:
            if query is None:
                query = self._index_instrument_positions.get(instrument_id, set())
            else:
                query = query.intersection(self._index_instrument_positions.get(instrument_id, set()))

        if strategy_id is not None:
            if query is None:
                query = self._index_strategy_positions.get(strategy_id, set())
            else:
                query = query.intersection(self._index_strategy_positions.get(strategy_id, set()))

        return query

    cdef list _get_orders_for_ids(self, set client_order_ids, OrderSide side):
        cdef list orders = []

        if not client_order_ids:
            return orders

        cdef:
            ClientOrderId client_order_id
            Order order

        try:
            for client_order_id in sorted(client_order_ids):
                order = self._orders[client_order_id]

                if side == OrderSide.NO_ORDER_SIDE or side == order.side:
                    orders.append(order)
        except KeyError as e:
            self._log.error(f"Cannot find `Order` object in cached orders {e}")

        return orders

    cdef list _get_positions_for_ids(self, set position_ids, PositionSide side):
        cdef list positions = []

        cdef:
            PositionId position_id
            Position position

        try:
            for position_id in sorted(position_ids):
                position = self._positions[position_id]

                if side == PositionSide.NO_POSITION_SIDE or side == position.side:
                    positions.append(position)
        except KeyError as e:
            self._log.error(f"Cannot find `Position` object in cached positions {e}")

        return positions

    cpdef set client_order_ids(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        cdef set query = self._build_order_query_filter_set(venue, instrument_id, strategy_id)

        if query is None:
            return self._index_orders
        else:
            return self._index_orders.intersection(query)

    cpdef set client_order_ids_open(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all open client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        cdef set query = self._build_order_query_filter_set(venue, instrument_id, strategy_id)

        if query is None:
            return self._index_orders_open
        else:
            return self._index_orders_open.intersection(query)

    cpdef set client_order_ids_closed(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all closed client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        cdef set query = self._build_order_query_filter_set(venue, instrument_id, strategy_id)

        if query is None:
            return self._index_orders_closed
        else:
            return self._index_orders_closed.intersection(query)

    cpdef set client_order_ids_emulated(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all emulated client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        cdef set query = self._build_order_query_filter_set(venue, instrument_id, strategy_id)

        if query is None:
            return self._index_orders_emulated
        else:
            return self._index_orders_emulated.intersection(query)

    cpdef set client_order_ids_inflight(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all in-flight client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        cdef set query = self._build_order_query_filter_set(venue, instrument_id, strategy_id)

        if query is None:
            return self._index_orders_inflight
        else:
            return self._index_orders_inflight.intersection(query)

    cpdef set order_list_ids(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all order list IDs.

        Returns
        -------
        set[OrderListId]

        """
        cdef list order_lists = self.order_lists(venue, instrument_id, strategy_id)
        cdef OrderList ol

        return {ol.id for ol in order_lists}

    cpdef set position_ids(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all position IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[PositionId]

        """
        cdef set query = self._build_position_query_filter_set(venue, instrument_id, strategy_id)

        if query is None:
            return self._index_positions
        else:
            return self._index_positions.intersection(query)

    cpdef set position_open_ids(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all open position IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[PositionId]

        """
        cdef set query = self._build_position_query_filter_set(venue, instrument_id, strategy_id)

        if query is None:
            return self._index_positions_open
        else:
            return self._index_positions_open.intersection(query)

    cpdef set position_closed_ids(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all closed position IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[PositionId]

        """
        cdef set query = self._build_position_query_filter_set(venue, instrument_id, strategy_id)

        if query is None:
            return self._index_positions_closed
        else:
            return self._index_positions_closed.intersection(query)

    cpdef set actor_ids(self):
        """
        Return all actor IDs.

        Returns
        -------
        set[ComponentId]

        """
        return self._index_actors.copy()

    cpdef set strategy_ids(self):
        """
        Return all strategy IDs.

        Returns
        -------
        set[StrategyId]

        """
        return self._index_strategies.copy()

    cpdef set exec_algorithm_ids(self):
        """
        Return all execution algorithm IDs.

        Returns
        -------
        set[ExecAlgorithmId]

        """
        return self._index_exec_algorithms.copy()

# -- ORDER QUERIES --------------------------------------------------------------------------------

    cpdef Order order(self, ClientOrderId client_order_id):
        """
        Return the order matching the given client order ID (if found).

        Returns
        -------
        Order or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        return self._orders.get(client_order_id)

    cpdef ClientOrderId client_order_id(self, VenueOrderId venue_order_id):
        """
        Return the client order ID matching the given venue order ID (if found).

        Parameters
        ----------
        venue_order_id : VenueOrderId
            The venue assigned order ID.

        Returns
        -------
        ClientOrderId or ``None``

        """
        Condition.not_none(venue_order_id, "venue_order_id")

        return self._index_venue_order_ids.get(venue_order_id)

    cpdef VenueOrderId venue_order_id(self, ClientOrderId client_order_id):
        """
        Return the order ID matching the given client order ID (if found).

        Returns
        -------
        VenueOrderId or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        return self._index_client_order_ids.get(client_order_id)

    cpdef ClientId client_id(self, ClientOrderId client_order_id):
        """
        Return the specific execution client ID matching the given client order ID (if found).

        Returns
        -------
        ClientId or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        return self._index_order_client.get(client_order_id)

    cpdef list orders(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return all orders matching the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        cdef set client_order_ids = self.client_order_ids(venue, instrument_id, strategy_id)

        return self._get_orders_for_ids(client_order_ids, side)

    cpdef list orders_open(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return all open orders with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        cdef set client_order_ids = self.client_order_ids_open(venue, instrument_id, strategy_id)

        return self._get_orders_for_ids(client_order_ids, side)

    cpdef list orders_closed(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return all closed orders with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        cdef set client_order_ids = self.client_order_ids_closed(venue, instrument_id, strategy_id)

        return self._get_orders_for_ids(client_order_ids, side)

    cpdef list orders_emulated(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return all emulated orders with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        cdef set client_order_ids = self.client_order_ids_emulated(venue, instrument_id, strategy_id)

        return self._get_orders_for_ids(client_order_ids, side)

    cpdef list orders_inflight(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return all in-flight orders with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        cdef set client_order_ids = self.client_order_ids_inflight(venue, instrument_id, strategy_id)

        return self._get_orders_for_ids(client_order_ids, side)

    cpdef list orders_for_position(self, PositionId position_id):
        """
        Return all orders for the given position ID.

        Parameters
        ----------
        position_id : PositionId
            The position ID for the orders.

        Returns
        -------
        list[Order]

        """
        Condition.not_none(position_id, "position_id")

        cdef set client_order_ids = self._index_position_orders.get(position_id)

        if not client_order_ids:
            return []

        return [self._orders[client_order_id] for client_order_id in client_order_ids]

    cpdef bint order_exists(self, ClientOrderId client_order_id):
        """
        Return a value indicating whether an order with the given ID exists.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        Condition.not_none(client_order_id, "client_order_id")

        return client_order_id in self._index_orders

    cpdef bint is_order_open(self, ClientOrderId client_order_id):
        """
        Return a value indicating whether an order with the given ID is open.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        Condition.not_none(client_order_id, "client_order_id")

        return client_order_id in self._index_orders_open

    cpdef bint is_order_closed(self, ClientOrderId client_order_id):
        """
        Return a value indicating whether an order with the given ID is closed.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        Condition.not_none(client_order_id, "client_order_id")

        return client_order_id in self._index_orders_closed

    cpdef bint is_order_emulated(self, ClientOrderId client_order_id):
        """
        Return a value indicating whether an order with the given ID is emulated.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        Condition.not_none(client_order_id, "client_order_id")

        return client_order_id in self._index_orders_emulated

    cpdef bint is_order_inflight(self, ClientOrderId client_order_id):
        """
        Return a value indicating whether an order with the given ID is in-flight.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        Condition.not_none(client_order_id, "client_order_id")

        return client_order_id in self._index_orders_inflight

    cpdef bint is_order_pending_cancel_local(self, ClientOrderId client_order_id):
        """
        Return a value indicating whether an order with the given ID is pending cancel locally.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        Condition.not_none(client_order_id, "client_order_id")

        return client_order_id in self._index_orders_pending_cancel

    cpdef int orders_open_count(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return the count of open orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        return len(self.orders_open(venue, instrument_id, strategy_id, side))

    cpdef int orders_closed_count(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return the count of closed orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        return len(self.orders_closed(venue, instrument_id, strategy_id, side))

    cpdef int orders_emulated_count(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return the count of emulated orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        return len(self.orders_emulated(venue, instrument_id, strategy_id, side))

    cpdef int orders_inflight_count(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return the count of in-flight orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        return len(self.orders_inflight(venue, instrument_id, strategy_id, side))

    cpdef int orders_total_count(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return the total count of orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        return len(self.orders(venue, instrument_id, strategy_id, side))

# -- ORDER LIST QUERIES ---------------------------------------------------------------------------

    cpdef OrderList order_list(self, OrderListId order_list_id):
        """
        Return the order list matching the given order list ID (if found).

        Returns
        -------
        OrderList or ``None``

        """
        Condition.not_none(order_list_id, "order_list_id")

        return self._order_lists.get(order_list_id)

    cpdef list order_lists(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all order lists matching the given query filters.

        *No particular order of list elements is guaranteed.*

        Returns
        -------
        list[OrderList]

        """
        cdef list order_lists = list(self._order_lists.values())
        cdef OrderList ol

        if venue is not None:
            order_lists = [ol for ol in order_lists if ol.instrument_id.venue == venue]

        if instrument_id is not None:
            order_lists = [ol for ol in order_lists if ol.instrument_id == instrument_id]

        if strategy_id is not None:
            order_lists = [ol for ol in order_lists if ol.strategy_id == strategy_id]

        return order_lists

    cpdef bint order_list_exists(self, OrderListId order_list_id):
        """
        Return a value indicating whether an order list with the given ID exists.

        Parameters
        ----------
        order_list_id : OrderListId
            The order list ID to check.

        Returns
        -------
        bool

        """
        Condition.not_none(order_list_id, "order_list_id")

        return order_list_id in self._order_lists

# -- EXEC ALGORITHM QUERIES -----------------------------------------------------------------------

    cpdef list orders_for_exec_algorithm(
        self,
        ExecAlgorithmId exec_algorithm_id,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        OrderSide side = OrderSide.NO_ORDER_SIDE,
    ):
        """
        Return all execution algorithm orders for the given query filters.

        Parameters
        ----------
        exec_algorithm_id : ExecAlgorithmId
            The execution algorithm ID.
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        Condition.not_none(exec_algorithm_id, "exec_algorithm_id")

        cdef set query = self._build_order_query_filter_set(venue, instrument_id, strategy_id)
        cdef set exec_algorithm_order_ids = self._index_exec_algorithm_orders.get(exec_algorithm_id)

        if query is not None and exec_algorithm_order_ids is not None:
            exec_algorithm_order_ids = query.intersection(exec_algorithm_order_ids)

        return self._get_orders_for_ids(exec_algorithm_order_ids, side)

    cpdef list orders_for_exec_spawn(self, ClientOrderId exec_spawn_id):
        """
        Return all orders for the given execution spawn ID (if found).

        Will also include the primary (original) order.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution algorithm spawning primary (original) client order ID.

        Returns
        -------
        list[Order]

        """
        Condition.not_none(exec_spawn_id, "exec_spawn_id")

        return self._get_orders_for_ids(self._index_exec_spawn_orders.get(exec_spawn_id), OrderSide.NO_ORDER_SIDE)

    cpdef Quantity exec_spawn_total_quantity(self, ClientOrderId exec_spawn_id, bint active_only=False):
        """
        Return the total quantity for the given execution spawn ID (if found).

        If no execution spawn ID matches then returns ``None``.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution algorithm spawning primary (original) client order ID.
        active_only : bool, default False
            The flag to filter for active execution spawn orders only.

        Returns
        -------
        Quantity or ``None``

        Notes
        -----
        An "active" order is defined as one which is *not closed*.

        """
        Condition.not_none(exec_spawn_id, "exec_spawn_id")

        cdef list exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id)

        if not exec_spawn_orders:
            return None

        cdef:
            Order spawn_order
            uint8_t precision = 0
            uint64_t raw_total_quantity = 0
        for spawn_order in exec_spawn_orders:
            precision = spawn_order.quantity._mem.precision

            if not active_only or not spawn_order.is_closed_c():
                raw_total_quantity += spawn_order.quantity._mem.raw

        return Quantity.from_raw_c(raw_total_quantity, precision)

    cpdef Quantity exec_spawn_total_filled_qty(self, ClientOrderId exec_spawn_id, bint active_only=False):
        """
        Return the total filled quantity for the given execution spawn ID (if found).

        If no execution spawn ID matches then returns ``None``.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution algorithm spawning primary (original) client order ID.
        active_only : bool, default False
            The flag to filter for active execution spawn orders only.

        Returns
        -------
        Quantity or ``None``

        Notes
        -----
        An "active" order is defined as one which is *not closed*.

        """
        Condition.not_none(exec_spawn_id, "exec_spawn_id")

        cdef list exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id)

        if not exec_spawn_orders:
            return None

        cdef:
            Order spawn_order
            uint8_t precision = 0
            uint64_t raw_filled_qty = 0
        for spawn_order in exec_spawn_orders:
            precision = spawn_order.filled_qty._mem.precision

            if not active_only or not spawn_order.is_closed_c():
                raw_filled_qty += spawn_order.filled_qty._mem.raw

        return Quantity.from_raw_c(raw_filled_qty, precision)

    cpdef Quantity exec_spawn_total_leaves_qty(self, ClientOrderId exec_spawn_id, bint active_only=False):
        """
        Return the total leaves quantity for the given execution spawn ID (if found).

        If no execution spawn ID matches then returns ``None``.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution algorithm spawning primary (original) client order ID.
        active_only : bool, default False
            The flag to filter for active execution spawn orders only.

        Returns
        -------
        Quantity or ``None``

        Notes
        -----
        An "active" order is defined as one which is *not closed*.

        """
        Condition.not_none(exec_spawn_id, "exec_spawn_id")

        cdef list exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id)

        if not exec_spawn_orders:
            return None

        cdef:
            Order spawn_order
            uint8_t precision = 0
            uint64_t raw_leaves_qty = 0
        for spawn_order in exec_spawn_orders:
            precision = spawn_order.leaves_qty._mem.precision

            if not active_only or not spawn_order.is_closed_c():
                raw_leaves_qty += spawn_order.leaves_qty._mem.raw

        return Quantity.from_raw_c(raw_leaves_qty, precision)

# -- POSITION QUERIES -----------------------------------------------------------------------------

    cpdef Position position(self, PositionId position_id):
        """
        Return the position associated with the given ID (if found).

        Parameters
        ----------
        position_id : PositionId
            The position ID.

        Returns
        -------
        Position or ``None``

        """
        Condition.not_none(position_id, "position_id")

        return self._positions.get(position_id)

    cpdef Position position_for_order(self, ClientOrderId client_order_id):
        """
        Return the position associated with the given client order ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID.

        Returns
        -------
        Position or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        cdef PositionId position_id = self._index_order_position.get(client_order_id)

        if position_id is None:
            return None

        return self._positions.get(position_id)

    cpdef PositionId position_id(self, ClientOrderId client_order_id):
        """
        Return the position ID associated with the given client order ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID associated with the position.

        Returns
        -------
        PositionId or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        return self._index_order_position.get(client_order_id)

    cpdef list position_snapshots(self, PositionId position_id = None):
        """
        Return all position snapshots with the given optional identifier filter.

        Parameters
        ----------
        position_id : PositionId, optional
            The position ID query filter.

        Returns
        -------
        list[Position]

        """
        cdef list snapshot_list
        cdef list snapshots

        if position_id is not None:
            snapshots = self._position_snapshots.get(position_id, [])
        else:
            snapshots = []

            for snapshot_list in self._position_snapshots.values():
                snapshots += snapshot_list

        cdef bytes s

        return [pickle.loads(s) for s in snapshots]

    cpdef set position_snapshot_ids(self, InstrumentId instrument_id = None):
        """
        Return all position IDs for position snapshots with the given instrument filter.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.

        Returns
        -------
        set[PositionId]

        """
        if instrument_id is not None:
            return self._index_instrument_position_snapshots.get(instrument_id, set())
        else:
            # Return all position IDs that have snapshots
            return set(self._position_snapshots.keys())

    cpdef list position_snapshot_bytes(self, PositionId position_id):
        """
        Return the raw pickled snapshot bytes for the given position ID.

        Parameters
        ----------
        position_id : PositionId
            The position ID to get snapshot bytes for.

        Returns
        -------
        list[bytes]
            The list of pickled snapshot bytes, or empty list if no snapshots exist.

        """
        Condition.not_none(position_id, "position_id")
        return self._position_snapshots.get(position_id, [])

    cpdef list positions(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        PositionSide side = PositionSide.NO_POSITION_SIDE,
    ):
        """
        Return all positions with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : PositionSide, default ``NO_POSITION_SIDE`` (no filter)
            The position side query filter.

        Returns
        -------
        list[Position]

        """
        cdef set position_ids = self.position_ids(venue, instrument_id, strategy_id)

        return self._get_positions_for_ids(position_ids, side)

    cpdef list positions_open(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        PositionSide side = PositionSide.NO_POSITION_SIDE,
    ):
        """
        Return all open positions with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : PositionSide, default ``NO_POSITION_SIDE`` (no filter)
            The position side query filter.

        Returns
        -------
        list[Position]

        """
        cdef set position_ids = self.position_open_ids(venue, instrument_id, strategy_id)

        return self._get_positions_for_ids(position_ids, side)

    cpdef list positions_closed(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return all closed positions with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        list[Position]

        """
        cdef set position_ids = self.position_closed_ids(venue, instrument_id, strategy_id)

        return self._get_positions_for_ids(position_ids, PositionSide.NO_POSITION_SIDE)

    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given ID exists.

        Parameters
        ----------
        position_id : PositionId
            The position ID.

        Returns
        -------
        int

        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions

    cpdef bint is_position_open(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given ID exists
        and is open.

        Parameters
        ----------
        position_id : PositionId
            The position ID.

        Returns
        -------
        bool

        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions_open

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given ID exists
        and is closed.

        Parameters
        ----------
        position_id : PositionId
            The position ID.

        Returns
        -------
        bool

        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions_closed

    cpdef int positions_open_count(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        PositionSide side = PositionSide.NO_POSITION_SIDE,
    ):
        """
        Return the count of open positions with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : PositionSide, default ``NO_POSITION_SIDE`` (no filter)
            The position side query filter.

        Returns
        -------
        int

        """
        return len(self.positions_open(venue, instrument_id, strategy_id, side))

    cpdef int positions_closed_count(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
    ):
        """
        Return the count of closed positions with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        int

        """
        return len(self.positions_closed(venue, instrument_id, strategy_id))

    cpdef int positions_total_count(
        self,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        PositionSide side = PositionSide.NO_POSITION_SIDE,
    ):
        """
        Return the total count of positions with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : PositionSide, default ``NO_POSITION_SIDE`` (no filter)
            The position side query filter.

        Returns
        -------
        int

        """
        return len(self.positions(venue, instrument_id, strategy_id, side))

# -- STRATEGY QUERIES -----------------------------------------------------------------------------

    cpdef StrategyId strategy_id_for_order(self, ClientOrderId client_order_id):
        """
        Return the strategy ID associated with the given ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID associated with the strategy.

        Returns
        -------
        StrategyId or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        return self._index_order_strategy.get(client_order_id)

    cpdef StrategyId strategy_id_for_position(self, PositionId position_id):
        """
        Return the strategy ID associated with the given ID (if found).

        Parameters
        ----------
        position_id : PositionId
            The position ID associated with the strategy.

        Returns
        -------
        StrategyId or ``None``

        """
        Condition.not_none(position_id, "position_id")

        return self._index_position_strategy.get(position_id)

    cpdef void heartbeat(self, datetime timestamp):
        """
        Add a heartbeat at the given `timestamp`.

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the heartbeat.

        """
        Condition.not_none(timestamp, "timestamp")

        if self._database is None:
            self._log.warning(f"Cannot set heartbeat {timestamp} (no database configured)")
            return

        self._database.heartbeat(timestamp)

    cpdef void force_remove_from_own_order_book(self, ClientOrderId client_order_id):
        """
        Force removal of an order from own order books and clean up all indexes.

        This method is used when order.apply() fails and we need to ensure terminal
        orders are properly cleaned up from own books and all relevant indexes.
        Replicates the index cleanup that update_order performs for closed orders.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to remove.

        """
        Condition.not_none(client_order_id, "client_order_id")

        cdef Order order = self._orders.get(client_order_id)
        if order is None:
            return

        # Remove from all open/active indexes (mirrors update_order for closed orders)
        self._index_orders_open.discard(client_order_id)
        self._index_orders_pending_cancel.discard(client_order_id)
        self._index_orders_inflight.discard(client_order_id)
        self._index_orders_emulated.discard(client_order_id)

        if self._own_order_books:
            self._index_orders_open_pyo3.discard(nautilus_pyo3.ClientOrderId(client_order_id.value))

            own_book = self._own_order_books.get(order.instrument_id)
            if own_book is not None:
                try:
                    own_book_order = order.to_own_book_order()
                    own_book.delete(own_book_order)
                    self._log.debug(
                        f"Force deleted {client_order_id!r} from own book",
                        LogColor.MAGENTA,
                    )
                except Exception as e:
                    self._log.debug(
                        f"Could not force delete {client_order_id!r} from own book: {e}",
                        LogColor.MAGENTA,
                    )

        self._index_orders_closed.add(client_order_id)

    cpdef void audit_own_order_books(self):
        """
        Audit all own order books against open and inflight order indexes.

        Ensures closed orders are removed from own order books. This includes both
        orders tracked in _index_orders_open (ACCEPTED, TRIGGERED, PENDING_*, PARTIALLY_FILLED)
        and _index_orders_inflight (INITIALIZED, SUBMITTED) to prevent false positives
        during venue latency windows.

        Logs all failures as errors.

        """
        self._log.debug("Starting own books audit", LogColor.MAGENTA)
        cdef double start_us = time.time() * 1_000_000

        # Build union of open and inflight orders for audit,
        # this prevents false positives for SUBMITTED orders during venue latency.
        cdef set valid_order_ids = set()
        for client_order_id in self._index_orders_open:
            if self._own_order_books:
                valid_order_ids.add(nautilus_pyo3.ClientOrderId(client_order_id.value))

        for client_order_id in self._index_orders_inflight:
            if self._own_order_books:
                valid_order_ids.add(nautilus_pyo3.ClientOrderId(client_order_id.value))

        for own_book in self._own_order_books.values():
            own_book.audit_open_orders(valid_order_ids)

        cdef double audit_us = (time.time() * 1_000_000) - start_us
        self._log.debug(f"Completed own books audit in {int(audit_us)}us", LogColor.MAGENTA)


cdef inline dict[Decimal, list[Order]] process_own_order_map(
    dict[Decimal, list[nautilus_pyo3.OwnBookOrder]] own_order_map,
    dict[ClientOrderId, Order] order_cache,
):
    cdef dict[Decimal, Order] order_map = {}

    cdef:
        list[Order] orders = []
        ClientOrderId client_order_id
        Order order
    for level_price, own_orders in own_order_map.items():
        orders = []

        for own_order in own_orders:
            client_order_id = ClientOrderId(own_order.client_order_id.value)
            order = order_cache.get(client_order_id)

            if order is None:
                raise RuntimeError(f"{client_order_id!r} from own book not found in cache")

            orders.append(order)

        order_map[level_price] = orders

    return order_map
