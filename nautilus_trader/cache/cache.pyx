# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import copy
import pickle
import time
import uuid
from collections import deque
from decimal import Decimal

from nautilus_trader.cache.config import CacheConfig

from cpython.datetime cimport datetime
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.calculators cimport ExchangeRateCalculator
from nautilus_trader.cache.facade cimport CacheDatabaseFacade
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
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
    snapshot_orders : bool, default False
        If order state snapshots should be persisted.
    snapshot_positions : bool, default False
        If position state snapshots should be persisted.

    Raises
    ------
    TypeError
        If `config` is not of type `CacheConfig`.
    """

    def __init__(
        self,
        CacheDatabaseFacade database: CacheDatabaseFacade | None = None,
        bint snapshot_orders: bool = False,
        bint snapshot_positions: bool = False,
        config: CacheConfig | None = None,
    ):
        if config is None:
            config = CacheConfig()
        Condition.type(config, CacheConfig, "config")

        self._database = database
        self._log = Logger(name=type(self).__name__)
        self._xrate_calculator = ExchangeRateCalculator()

        # Configuration
        self._drop_instruments_on_reset = config.drop_instruments_on_reset
        self.tick_capacity = config.tick_capacity
        self.bar_capacity = config.bar_capacity
        self.snapshot_orders = snapshot_orders
        self.snapshot_positions = snapshot_positions

        # Caches
        self._general: dict[str, bytes] = {}
        self._xrate_symbols: dict[InstrumentId, str] = {}
        self._quote_ticks: dict[InstrumentId, deque[QuoteTick]] = {}
        self._trade_ticks: dict[InstrumentId, deque[TradeTick]] = {}
        self._order_books: dict[InstrumentId, OrderBook] = {}
        self._bars: dict[BarType, deque[Bar]] = {}
        self._bars_bid: dict[InstrumentId, Bar] = {}
        self._bars_ask: dict[InstrumentId, Bar] = {}
        self._currencies: dict[str, Currency] = {}
        self._instruments: dict[InstrumentId, Instrument] = {}
        self._synthetics: dict[InstrumentId, SyntheticInstrument] = {}
        self._accounts: dict[AccountId, Account] = {}
        self._orders: dict[ClientOrderId, Order] = {}
        self._order_lists: dict[OrderListId, OrderList] = {}
        self._positions: dict[PositionId, Position] = {}
        self._position_snapshots: dict[PositionId, list[bytes]] = {}

        # Cache index
        self._index_venue_account: dict[Venue, AccountId] = {}
        self._index_venue_orders: dict[Venue, set[ClientOrderId]] = {}
        self._index_venue_positions: dict[Venue, set[PositionId]] = {}
        self._index_order_ids: dict[VenueOrderId, ClientOrderId] = {}
        self._index_order_position: dict[ClientOrderId, PositionId] = {}
        self._index_order_strategy: dict[ClientOrderId, StrategyId] = {}
        self._index_order_client: dict[ClientOrderId, ClientId] = {}
        self._index_position_strategy: dict[PositionId, StrategyId] = {}
        self._index_position_orders: dict[PositionId, set[ClientOrderId]] = {}
        self._index_instrument_orders: dict[InstrumentId, set[ClientOrderId]] = {}
        self._index_instrument_positions: dict[InstrumentId, set[PositionId]] = {}
        self._index_strategy_orders: dict[StrategyId, set[ClientOrderId]] = {}
        self._index_strategy_positions: dict[StrategyId, set[PositionId]] = {}
        self._index_exec_algorithm_orders: dict[ExecAlgorithmId, set[ClientOrderId]] = {}
        self._index_exec_spawn_orders: dict[ClientOrderId: set[ClientOrderId]] = {}
        self._index_orders: set[ClientOrderId] = set()
        self._index_orders_open: set[ClientOrderId] = set()
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

        self._log.info("READY.")

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void cache_general(self):
        """
        Clear the current general cache and load the general objects from the
        cache database.
        """
        self._log.debug(f"Loading general cache from database...")

        if self._database is not None:
            self._general = self._database.load()
        else:
            self._general = {}

        cdef int count = len(self._general)
        self._log.info(
            f"Cached {count} general object{'' if count == 1 else 's'} from database.",
            color=LogColor.BLUE if self._general else LogColor.NORMAL,
        )

    cpdef void cache_currencies(self):
        """
        Clear the current currencies cache and load currencies from the cache
        database.
        """
        self._log.debug(f"Loading currencies from database...")

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
            f"Cached {count} currenc{'y' if count == 1 else 'ies'} from database.",
            color=LogColor.BLUE if self._currencies else LogColor.NORMAL,
        )

    cpdef void cache_instruments(self):
        """
        Clear the current instruments cache and load instruments from the cache
        database.
        """
        self._log.debug(f"Loading instruments from database...")

        if self._database is not None:
            self._instruments = self._database.load_instruments()
        else:
            self._instruments = {}

        cdef int count = len(self._instruments)
        self._log.info(
            f"Cached {count} instrument{'' if count == 1 else 's'} from database.",
            color=LogColor.BLUE if self._instruments else LogColor.NORMAL,
        )

    cpdef void cache_synthetics(self):
        """
        Clear the current synthetic instruments cache and load synthetic instruments from the cache
        database.
        """
        self._log.debug(f"Loading synthetic instruments from database...")

        if self._database is not None:
            self._synthetics = self._database.load_synthetics()
        else:
            self._synthetics = {}

        cdef int count = len(self._synthetics)
        self._log.info(
            f"Cached {count} synthetic instrument{'' if count == 1 else 's'} from database.",
            color=LogColor.BLUE if self._synthetics else LogColor.NORMAL,
        )

    cpdef void cache_accounts(self):
        """
        Clear the current accounts cache and load accounts from the cache
        database.
        """
        self._log.debug(f"Loading accounts from database...")

        if self._database is not None:
            self._accounts = self._database.load_accounts()
        else:
            self._accounts = {}

        cdef int count = len(self._accounts)
        self._log.info(
            f"Cached {count} account{'' if count == 1 else 's'} from database.",
            color=LogColor.BLUE if self._accounts else LogColor.NORMAL,
        )

    cpdef void cache_orders(self):
        """
        Clear the current orders cache and load orders from the cache database.
        """
        self._log.debug(f"Loading orders from database...")

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
            f"Cached {count} order{'' if count == 1 else 's'} from database.",
            color=LogColor.BLUE if self._orders else LogColor.NORMAL,
        )

    cpdef void cache_order_lists(self):
        """
        Clear the current order lists cache and load order lists using cached orders.
        """
        self._log.debug(f"Loading order lists...")

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
            f"Cached {count} order list{'' if count == 1 else 's'} from database.",
            color=LogColor.BLUE if self._order_lists else LogColor.NORMAL,
        )

    cpdef void cache_positions(self):
        """
        Clear the current positions cache and load positions from the cache
        database.
        """
        self._log.debug(f"Loading positions from database...")

        if self._database is not None:
            self._positions = self._database.load_positions()
        else:
            self._positions = {}

        cdef int count = len(self._positions)
        self._log.info(
            f"Cached {count} position{'' if count == 1 else 's'} from database.",
            color=LogColor.BLUE if self._positions else LogColor.NORMAL
        )

    cpdef void build_index(self):
        """
        Clear the current cache index and re-build.
        """
        self.clear_index()

        self._log.debug(f"Building index...")
        cdef double ts = time.time()

        self._build_index_venue_account()
        self._build_indexes_from_orders()
        self._build_indexes_from_positions()

        self._log.debug(f"Index built in {time.time() - ts:.3f}s.")

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
        self._log.info("Checking data integrity...")

        # Needed type defs
        # ----------------
        cdef:
            AccountId account_id
            Order order
            Position position

        # Check object caches
        # -------------------
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

        for client_order_id in self._index_order_ids.values():
            if client_order_id not in self._orders:
                self._log.error(
                    f"{failure} in _index_venue_order_ids: "
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
                f"Integrity check passed in {total_us}μs.",
                color=LogColor.GREEN
            )
            return True
        else:
            self._log.error(
                f"Integrity check failed with "
                f"{error_count} error{'' if error_count == 1 else 's'} "
                f"in {total_us}μs."
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
        self._log.debug("Checking residuals...")

        cdef bint residuals = False

        # Check for any residual active orders and log warnings if any are found
        for order in self.orders_open():
            residuals = True
            self._log.warning(f"Residual {order}")

        for position in self.positions_open():
            residuals = True
            self._log.warning(f"Residual {position}")

        return residuals

    cpdef void clear_index(self):
        self._log.debug(f"Clearing index...")

        self._index_venue_account.clear()
        self._index_venue_orders.clear()
        self._index_venue_positions.clear()
        self._index_order_ids.clear()
        self._index_order_position.clear()
        self._index_order_strategy.clear()
        self._index_order_client.clear()
        self._index_position_strategy.clear()
        self._index_position_orders.clear()
        self._index_instrument_orders.clear()
        self._index_instrument_positions.clear()
        self._index_strategy_orders.clear()
        self._index_strategy_positions.clear()
        self._index_exec_algorithm_orders.clear()
        self._index_exec_spawn_orders.clear()
        self._index_orders.clear()
        self._index_orders_open.clear()
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

        self._log.debug(f"Cleared index.")

    cpdef void reset(self):
        """
        Reset the cache.

        All stateful fields are reset to their initial value.
        """
        self._log.info("Resetting cache...")

        self._general.clear()
        self._xrate_symbols.clear()
        self._quote_ticks.clear()
        self._trade_ticks.clear()
        self._order_books.clear()
        self._bars.clear()
        self._bars_bid.clear()
        self._bars_ask.clear()
        self._currencies.clear()
        self._synthetics.clear()
        self._accounts.clear()
        self._orders.clear()
        self._order_lists.clear()
        self._positions.clear()
        self._position_snapshots.clear()
        self.clear_index()

        if self._drop_instruments_on_reset:
            self._instruments.clear()

        self._log.debug(f"Reset cache.")

    cpdef void flush_db(self):
        """
        Flush the caches database which permanently removes all persisted data.

        Warnings
        --------
        Permanent data loss.

        """
        self._log.debug("Flushing execution database...")

        if self._database is not None:
            self._database.flush()

        self._log.info("Execution database flushed.")

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

            # 2: Build _index_order_ids -> {VenueOrderId, ClientOrderId}
            if order.venue_order_id is not None:
                self._index_order_ids[order.venue_order_id] = order.client_order_id

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
                self._log.error(f"Contingency order {client_order_id!r} not found.")
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
                self._log.info(f"Assigned {order.position_id!r} to {client_order_id!r}.")

    cpdef Money calculate_unrealized_pnl(self, Position position):
        cdef QuoteTick quote = self.quote_tick(position.instrument_id)
        if quote is None:
            self._log.warning(
                f"Cannot calculate unrealized PnL for {position.id!r}, "
                f"no quotes for {position.instrument_id}.",
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
        Load the instrument associated with the given instrument_id (if found).

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
        Condition.true(instrument_id.is_synthetic(), "instrument_id was not a synthetic")

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
        Add the given general object to the cache.

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

        cdef PriceType price_type = <PriceType>bar._mem.bar_type.spec.price_type
        if price_type == PriceType.BID:
            self._bars_bid[bar.bar_type.instrument_id] = bar
        elif price_type == PriceType.ASK:
            self._bars_ask[bar.bar_type.instrument_id] = bar

    cpdef void add_quote_ticks(self, list ticks):
        """
        Add the given quote ticks to the cache.

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
            self._log.debug(f"Received <QuoteTick[{length}]> data for {instrument_id}.")
        else:
            self._log.debug("Received <QuoteTick[]> data with no ticks.")
            return

        cached_ticks = self._quote_ticks.get(instrument_id)

        if not cached_ticks:
            # The instrument_id was not registered
            cached_ticks = deque(maxlen=self.tick_capacity)
            self._quote_ticks[instrument_id] = cached_ticks
        elif len(cached_ticks) > 0:
            # Currently the simple solution for multiple consumers requesting
            # ticks at system spool up is just to add only if the cache is empty.
            self._log.debug("Cache already contains ticks.")
            return

        cdef QuoteTick tick
        for tick in ticks:
            cached_ticks.appendleft(tick)

    cpdef void add_trade_ticks(self, list ticks):
        """
        Add the given trade ticks to the cache.

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
            self._log.debug(f"Received <TradeTick[{length}]> data for {instrument_id}.")
        else:
            self._log.debug("Received <TradeTick[]> data with no ticks.")
            return

        cached_ticks = self._trade_ticks.get(instrument_id)

        if not cached_ticks:
            # The instrument_id was not registered
            cached_ticks = deque(maxlen=self.tick_capacity)
            self._trade_ticks[instrument_id] = cached_ticks
        elif len(cached_ticks) > 0:
            # Currently the simple solution for multiple consumers requesting
            # ticks at system spool up is just to add only if the cache is empty.
            self._log.debug("Cache already contains ticks.")
            return

        cdef TradeTick tick
        for tick in ticks:
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
            self._log.debug(f"Received <Bar[{length}]> data for {bar_type}.")
        else:
            self._log.debug("Received <Bar[]> data with no ticks.")
            return

        cached_bars = self._bars.get(bar_type)

        if not cached_bars:
            # The instrument_id was not registered
            cached_bars = deque(maxlen=self.bar_capacity)
            self._bars[bar_type] = cached_bars
        elif len(cached_bars) > 0:
            # Currently the simple solution for multiple consumers requesting
            # bars at system spool up is just to add only if the cache is empty.
            self._log.debug("Cache already contains bars.")
            return

        cdef Bar bar
        for bar in bars:
            cached_bars.appendleft(bar)

        bar = bars[-1]
        cdef PriceType price_type = <PriceType>bar._mem.bar_type.spec.price_type
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

        self._log.debug(f"Added currency {currency.code}.")

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

        self._log.debug(f"Added instrument {instrument.id}.")

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

        self._log.debug(f"Added synthetic instrument {synthetic.id}.")

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

        self._log.debug(f"Added Account(id={account.id.to_str()}).")
        self._log.debug(f"Indexed {repr(account.id)}.")

        # Update database
        if self._database is not None:
            self._database.add_account(account)

    cpdef void add_order(
        self,
        Order order,
        PositionId position_id = None,
        ClientId client_id = None,
        bint override = False,
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
        override : bool, default False
            If the added order should 'override' any existing order and replace
            it in the cache. This is currently used for emulated orders which are
            being released and transformed into another type.

        Raises
        ------
        ValueError
            If `order.client_order_id` is already contained in the cache.

        """
        Condition.not_none(order, "order")
        if not override:
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

        self._log.debug(f"Added {order}.")

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
            self._log.debug(f"Indexed {client_id!r}.")

        if self._database is None:
            return

        # Update database
        self._database.add_order(order, position_id, client_id)
        if self.snapshot_orders:
            self._database.snapshot_order_state(order)

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

        self._log.debug(f"Added {order_list}.")

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
            f"strategy_id={strategy_id}).",
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

        self._log.debug(f"Added Position(id={position.id.to_str()}, strategy_id={position.strategy_id.to_str()}).")

        if self._database is None:
            return

        # Update database
        self._database.add_position(position)
        if self.snapshot_positions:
            self._database.snapshot_position_state(
                position,
                position.ts_last,
                self.calculate_unrealized_pnl(position),
            )

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
        cdef list snapshots = self._position_snapshots.get(position_id)

        # Reassign position ID
        cdef Position copied_position = copy.deepcopy(position)
        copied_position.id = PositionId(f"{position.id.to_str()}-{uuid.uuid4()}")
        cdef bytes position_pickled = pickle.dumps(copied_position)

        if snapshots is not None:
            snapshots.append(position_pickled)
        else:
            self._position_snapshots[position_id] = [position_pickled]

        self._log.debug(f"Snapshot {repr(copied_position)}.")

    cpdef void snapshot_position_state(
        self,
        Position position,
        uint64_t ts_snapshot,
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
            The UNIX timestamp (nanoseconds) when the snapshot was taken.
        open_only : bool, default True
            If only open positions should be snapshot, this flag helps to avoid race conditions
            where a position is snapshot when no longer open.

        """
        Condition.not_none(position, "position")

        if open_only and not position.is_open_c():
            return  # Only snapshot open positions

        if self._database is None:
            self._log.warning(
                "Cannot snapshot position state for {position.id:r!} (no database configured).",
            )
            return

        self._database.snapshot_position_state(
            position,
            ts_snapshot,
            self.calculate_unrealized_pnl(position),
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
                "Cannot snapshot order state for {order.client_order_id:r!} (no database configured).",
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
        if self._database is not None:
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
        if order.venue_order_id is not None:
            # Assumes order_id does not change
            self._index_order_ids[order.venue_order_id] = order.client_order_id

        # Update in-flight state
        if order.is_inflight_c():
            self._index_orders_inflight.add(order.client_order_id)
        else:
            self._index_orders_inflight.discard(order.client_order_id)

        # Update open/closed state
        if order.is_open_c():
            self._index_orders_closed.discard(order.client_order_id)
            self._index_orders_open.add(order.client_order_id)
        elif order.is_closed_c():
            self._index_orders_open.discard(order.client_order_id)
            self._index_orders_pending_cancel.discard(order.client_order_id)
            self._index_orders_closed.add(order.client_order_id)

        # Update emulation
        if order.is_closed_c() or order.emulation_trigger == TriggerType.NO_TRIGGER:
            self._index_orders_emulated.discard(order.client_order_id)
        else:
            self._index_orders_emulated.add(order.client_order_id)

        if self._database is None:
            return

        # Update database
        self._database.update_order(order)
        if self.snapshot_orders:
            self._database.snapshot_order_state(order)

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
        if self.snapshot_positions:
            self._database.snapshot_position_state(
                position,
                position.ts_last,
                self.calculate_unrealized_pnl(position),
            )

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
            self._log.debug(f"Deleted Actor(id={actor.id.value}).")

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
            self._log.debug(f"Deleted Strategy(id={strategy.id.value}).")

# -- DATA QUERIES ---------------------------------------------------------------------------------

    cpdef bytes get(self, str key):
        """
        Add the given general object to the cache.

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
        Return the quote ticks for the given instrument ID.

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
        Return trade ticks for the given instrument ID.

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
            return trade_tick.price if trade_tick is not None else None
        else:
            quote_tick = self.quote_tick(instrument_id)
            return quote_tick.extract_price(price_type) if quote_tick is not None else None

    cpdef OrderBook order_book(self, InstrumentId instrument_id):
        """
        Return the order book for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId

        Returns
        -------
        OrderBook or ``None``

        """
        return self._order_books.get(instrument_id)

    cpdef QuoteTick quote_tick(self, InstrumentId instrument_id, int index = 0):
        """
        Return the quote tick for the given instrument ID at the given index.

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
        Return the trade tick for the given instrument ID at the given index

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

    cpdef Bar bar(self, BarType bar_type, int index = 0):
        """
        Return the bar for the given bar type at the given index.

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
            return book.count

    cpdef int quote_tick_count(self, InstrumentId instrument_id):
        """
        The count of quote ticks for the given instrument ID.

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
        The count of trade ticks for the given instrument ID.

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
        Return a value indicating whether the cache has quote ticks for the
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
        Return a value indicating whether the cache has trade ticks for the
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

    cpdef double get_xrate(
        self,
        Venue venue,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=PriceType.MID,
    ):
        """
        Return the calculated exchange rate.

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
        double

        Raises
        ------
        ValueError
            If `price_type` is ``LAST``.

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")

        if from_currency == to_currency:
            return Decimal(1)  # No conversion necessary

        cdef tuple quotes = self._build_quote_table(venue)

        return self._xrate_calculator.get_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type,
            bid_quotes=quotes[0],  # Bid
            ask_quotes=quotes[1],  # Ask
        )

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
        return sorted([x for x in self._instruments.keys() if venue is None or venue == x.venue])

    cpdef list instruments(self, Venue venue = None):
        """
        Return all instruments held by the cache.

        Parameters
        ----------
        venue : Venue, optional
            The venue filter for the query.

        Returns
        -------
        list[Instrument]

        """
        return [x for x in self._instruments.values() if venue is None or venue == x.id.venue]

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
        Condition.true(instrument_id.is_synthetic(), "instrument_id was not a synthetic")

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

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(venue, "venue")

        cdef AccountId account_id = self._index_venue_account.get(venue)
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

        return self._index_order_ids.get(venue_order_id)

    cpdef VenueOrderId venue_order_id(self, ClientOrderId client_order_id):
        """
        Return the order ID matching the given client order ID (if found).

        Returns
        -------
        VenueOrderId or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        cdef Order order = self._orders.get(client_order_id)
        if order is None:
            return None
        return order.venue_order_id

    cpdef ClientId client_id(self, ClientOrderId client_order_id):
        """
        Return the specific execution client ID matching the given client order ID (if found).

        Returns
        -------
        ClientId or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        self._index_order_client.get(client_order_id)

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
            self._log.warning(f"Cannot set heartbeat {timestamp} (no database configured).")
            return

        self._database.heartbeat(timestamp)
