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

"""
The `ExecutionCache` provides an interface for querying on orders and positions.
"""

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.execution.base cimport ExecutionCacheFacade
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.order cimport Order
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionCache(ExecutionCacheFacade):
    """
    Provides a cache for the `ExecutionEngine`.
    """

    def __init__(
            self,
            ExecutionDatabase database not None,
            Logger logger not None):
        """
        Initialize a new instance of the `ExecutionCache` class.

        Parameters
        ----------
        database : ExecutionDatabase
            The execution database adapter.
        logger : Logger
            The logger for the cache.

        """
        super().__init__()

        self._log = LoggerAdapter(type(self).__name__, logger)
        self._database = database

        # Cached objects
        self._cached_accounts = {}            # type: {AccountId, Account}
        self._cached_orders = {}              # type: {ClientOrderId, Order}
        self._cached_positions = {}           # type: {PositionId, Position}

        # Cached indexes
        self._index_venue_account = {}        # type: {Venue, AccountId}
        self._index_order_position = {}       # type: {ClientOrderId, PositionId}
        self._index_order_strategy = {}       # type: {ClientOrderId, StrategyId}
        self._index_position_strategy = {}    # type: {PositionId, StrategyId}
        self._index_position_orders = {}      # type: {PositionId, {ClientOrderId}}
        self._index_symbol_orders = {}        # type: {Symbol, {ClientOrderId}}
        self._index_symbol_positions = {}     # type: {Symbol, {PositionId}}
        self._index_strategy_orders = {}      # type: {StrategyId, {ClientOrderId}}
        self._index_strategy_positions = {}   # type: {StrategyId, {PositionId}}
        self._index_orders = set()            # type: {ClientOrderId}
        self._index_orders_working = set()    # type: {ClientOrderId}
        self._index_orders_completed = set()  # type: {ClientOrderId}
        self._index_positions = set()         # type: {PositionId}
        self._index_positions_open = set()    # type: {PositionId}
        self._index_positions_closed = set()  # type: {PositionId}
        self._index_strategies = set()        # type: {StrategyId}

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void cache_accounts(self) except *:
        """
        Clear the current accounts cache and load accounts from the execution
        database.
        """
        self._log.debug(f"Loading accounts to cache...")

        self._cached_accounts = self._database.load_accounts()
        self._log.info(f"Cached {len(self._cached_accounts)} account(s).")

    cpdef void cache_orders(self) except *:
        """
        Clear the current orders cache and load orders from the execution
        database.
        """
        self._log.debug(f"Loading orders to cache...")

        self._cached_orders = self._database.load_orders()
        self._log.info(f"Cached {len(self._cached_orders)} order(s).")

    cpdef void cache_positions(self) except *:
        """
        Clear the current positions cache and load positions from the execution
        database.
        """
        self._log.debug(f"Loading positions to cache...")

        self._cached_positions = self._database.load_positions()
        self._log.info(f"Cached {len(self._cached_positions)} position(s).")

    cpdef void build_index(self) except *:
        """
        Clear the current cache index and re-build.
        """
        self._clear_indexes()
        self._log.debug(f"Building indexes...")

        self._build_index_venue_account()
        self._build_indexes_from_orders()
        self._build_indexes_from_positions()

        self._log.info(f"Indexes built.")

    cpdef void integrity_check(self) except *:
        pass  # TODO: Implement

    cdef void _build_index_venue_account(self) except *:
        cdef AccountId account_id
        for account_id in self._cached_accounts.keys():
            self._cache_venue_account_id(account_id)

    cdef void _cache_venue_account_id(self, AccountId account_id) except *:
        self._index_venue_account[account_id.issuer_as_venue()] = account_id

    cdef void _build_indexes_from_orders(self) except *:
        cdef ClientOrderId cl_ord_id
        cdef Order order
        for cl_ord_id, order in self._cached_orders.items():
            # 1- Build _index_order_position -> {ClientOrderId, PositionId}
            if order.position_id is not None:
                self._index_order_position[cl_ord_id] = order.position_id

            # 2- Build _index_order_strategy -> {ClientOrderId, StrategyId}
            if order.strategy_id.not_null:
                self._index_order_strategy[cl_ord_id] = order.strategy_id

            # 3- Build _index_symbol_orders -> {Symbol, {ClientOrderId}}
            if order.symbol not in self._index_symbol_orders:
                self._index_symbol_orders[order.symbol] = set()
            self._index_symbol_orders[order.symbol].add(cl_ord_id)

            # 4- Build _index_strategy_orders -> {StrategyId, {ClientOrderId}}
            if order.strategy_id not in self._index_strategy_orders:
                self._index_strategy_orders[order.strategy_id] = set()
            self._index_strategy_orders[order.strategy_id].add(cl_ord_id)

            # 5- Build _index_orders -> {ClientOrderId}
            self._index_orders.add(cl_ord_id)

            # 6- Build _index_orders_working -> {ClientOrderId}
            if order.is_working_c():
                self._index_orders_working.add(cl_ord_id)
            # 7- Build _index_orders_completed -> {ClientOrderId}
            elif order.is_completed_c():
                self._index_orders_completed.add(cl_ord_id)

            # 8- Build _index_strategies -> {StrategyId}
            self._index_strategies.add(order.strategy_id)

    cdef void _build_indexes_from_positions(self) except *:
        cdef PositionId position_id
        cdef Position position
        for position_id, position in self._cached_positions.items():
            # 1- Build _index_position_strategy -> {PositionId, StrategyId}
            if position.strategy_id is not None:
                self._index_position_strategy[position_id] = position.strategy_id

            # 2- Build _index_position_orders -> {PositionId, {ClientOrderId}}
            if position_id not in self._index_position_orders:
                self._index_position_orders[position_id] = set()
            self._index_position_orders[position_id].add(position.cl_ord_id)

            # 3- Build _index_symbol_positions -> {Symbol, {PositionId}}
            if position.symbol not in self._index_symbol_positions:
                self._index_symbol_positions[position_id] = set()
            self._index_symbol_positions[position.symbol].add(position_id)

            # 4- Build _index_strategy_positions -> {StrategyId, {PositionId}}
            if position.strategy_id is not None and position.strategy_id not in self._index_strategy_positions:
                self._index_strategy_positions[position.strategy_id] = set()
            self._index_strategy_positions[position.strategy_id].add(position.strategy_id)

            # 5- Build _index_positions -> {PositionId}
            self._index_positions.add(position_id)

            # 6- Build _index_positions_open -> {PositionId}
            if position.is_open:
                self._index_positions_open.add(position_id)
            # 6- Build _index_positions_closed -> {PositionId}
            elif position.is_closed:
                self._index_positions_closed.add(position_id)

            # 7- Build _index_strategies -> {StrategyId}
            self._index_strategies.add(position.strategy_id)

    cpdef void load_strategy(self, TradingStrategy strategy):
        """
        Load the state dictionary for the given strategy from the execution cache.

        Parameters
        ----------
        strategy : TradingStrategy
            The strategy to load.

        """
        Condition.not_none(strategy, "strategy")

        cdef dict state = self._database.load_strategy(strategy.id)

        if state is not None:
            strategy.load(state)
            for key, value in state.items():
                self._log.debug(f"Loading {repr(strategy.id)}) state (key='{key}', value={value})...")
        else:
            self._log.info(f"No previous state found for {repr(strategy.id)}")

    cpdef Account load_account(self, AccountId account_id):
        """
        Load the account associated with the given account_id (if found).

        Parameters
        ----------
        :param account_id: The account identifier to load.

        Returns
        -------
        Account or None

        """
        Condition.not_none(account_id, "account_id")

        return self._cached_accounts.get(account_id)

    cpdef Order load_order(self, ClientOrderId cl_ord_id):
        """
        Load the order associated with the given identifier (if found).

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier to load.

        Returns
        -------
        Order or None

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return self._cached_orders.get(cl_ord_id)

    cpdef Position load_position(self, PositionId position_id):
        """
        Load the position associated with the given identifier (if found).

        Parameters
        ----------
        position_id : PositionId
            The position identifier to load.

        Returns
        -------
        Position or None

        """
        Condition.not_none(position_id, "position_id")

        return self._cached_positions.get(position_id)

    cpdef void add_account(self, Account account) except *:
        """
        Add the given account to the execution cache.

        Parameters
        ----------
        account : Account
            The account to add.

        Raises
        ------
        ValueError
            If account_id is already contained in the cached_accounts.

        """
        Condition.not_none(account, "account")
        Condition.not_in(account.id, self._cached_accounts, "account.id", "cached_accounts")

        self._cached_accounts[account.id] = account
        self._cache_venue_account_id(account.id)

        self._log.debug(f"Added Account(id={account.id.value}).")
        self._log.debug(f"Indexed {repr(account.id)} for {account.id.issuer}.")

        # Update database
        self._database.add_account(account)

    cpdef void add_order(self, Order order, PositionId position_id) except *:
        """
        Add the given order to the execution cache indexed with the given
        identifiers.

        Parameters
        ----------
        order : Order
            The order to add.
        position_id : PositionId
            The position identifier to index for the order.

        Raises
        ------
        ValueError
            If order.id is already contained in the cached_orders.
        ValueError
            If order.id is already contained in the index_orders.
        ValueError
            If order.id is already contained in the index_order_position.
        ValueError
            If order.id is already contained in the index_order_strategy.

        """
        Condition.not_none(order, "order")
        Condition.not_none(position_id, "position_id")
        Condition.not_in(order.cl_ord_id, self._cached_orders, "order.cl_ord_id", "cached_orders")
        Condition.not_in(order.cl_ord_id, self._index_orders, "order.cl_ord_id", "index_orders")
        Condition.not_in(order.cl_ord_id, self._index_order_position, "order.cl_ord_id", "index_order_position")
        Condition.not_in(order.cl_ord_id, self._index_order_strategy, "order.cl_ord_id", "index_order_strategy")

        self._cached_orders[order.cl_ord_id] = order
        self._index_orders.add(order.cl_ord_id)
        self._index_order_strategy[order.cl_ord_id] = order.strategy_id

        # Index: Symbol -> Set[ClientOrderId]
        if order.symbol not in self._index_symbol_orders:
            self._index_symbol_orders[order.symbol] = {order.cl_ord_id}
        else:
            self._index_symbol_orders[order.symbol].add(order.cl_ord_id)

        # Index: StrategyId -> Set[ClientOrderId]
        if order.strategy_id not in self._index_strategy_orders:
            self._index_strategy_orders[order.strategy_id] = {order.cl_ord_id}
        else:
            self._index_strategy_orders[order.strategy_id].add(order.cl_ord_id)

        cdef str position_id_str = f", {position_id.value}" if position_id.not_null else ""
        self._log.debug(f"Added Order(id={order.cl_ord_id.value}{position_id_str}).")

        if position_id.is_null:
            return  # Do not index the NULL id

        self.add_position_id(position_id, order.cl_ord_id, order.strategy_id)

        # Update database
        self._database.add_order(order, position_id)  # Logs

    cpdef void add_position_id(self, PositionId position_id, ClientOrderId cl_ord_id, StrategyId strategy_id) except *:
        """
        Index the given position identifier with the other given identifiers.

        Parameters
        ----------
        position_id : PositionId
            The position identifier to index.
        cl_ord_id : ClientOrderId
            The client order identifier to index.
        strategy_id : StrategyId
            The strategy identifier to index.

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(cl_ord_id, "cl_ord_id")
        Condition.not_none(strategy_id, "strategy_id")

        # Index: ClientOrderId -> PositionId
        if cl_ord_id not in self._index_order_position:
            self._index_order_position[cl_ord_id] = position_id
        else:
            if not position_id == self._index_order_position[cl_ord_id]:
                self._log.error(f"Order indexing invalid for {position_id}.")

        # Index: PositionId -> StrategyId
        if position_id not in self._index_position_strategy:
            self._index_position_strategy[position_id] = strategy_id
        else:
            if not strategy_id == self._index_position_strategy[position_id]:
                self._log.error(f"Strategy indexing invalid for {position_id}.")

        # Index: PositionId -> Set[ClientOrderId]
        if position_id not in self._index_position_orders:
            self._index_position_orders[position_id] = {cl_ord_id}
        else:
            self._index_position_orders[position_id].add(cl_ord_id)

        # Index: StrategyId -> Set[PositionId]
        if strategy_id not in self._index_strategy_positions:
            self._index_strategy_positions[strategy_id] = {position_id}
        else:
            self._index_strategy_positions[strategy_id].add(position_id)

        self._log.debug(f"Indexed {repr(position_id)}, "
                        f"cl_ord_id={cl_ord_id}, "
                        f"strategy_id={strategy_id}).")

    cpdef void add_position(self, Position position) except *:
        """
        Add the given position associated with the given strategy identifier.

        Parameters
        ----------
        position : Position
            The position to add.

        Raises
        ------
        ValueError
            If position.id is already contained in the cached_positions.
        ValueError
            If position.id is already contained in the index_positions.
        ValueError
            If position.id is already contained in the index_positions_open.

        """
        Condition.not_none(position, "position")
        Condition.not_in(position.id, self._cached_positions, "position.id", "cached_positions")
        Condition.not_in(position.id, self._index_positions, "position.id", "index_positions")
        Condition.not_in(position.id, self._index_positions_open, "position.id", "index_positions_open")

        self._cached_positions[position.id] = position
        self._index_positions.add(position.id)
        self._index_positions_open.add(position.id)

        self.add_position_id(position.id, position.from_order, position.strategy_id)

        # Index: Symbol -> Set[PositionId]
        if position.symbol not in self._index_symbol_positions:
            self._index_symbol_positions[position.symbol] = {position.id}
        else:
            self._index_symbol_positions[position.symbol].add(position.id)

        self._log.debug(f"Added Position(id={position.id.value}, strategy_id={position.strategy_id}).")

        # Update database
        self._database.add_position(position)

    cpdef void update_account(self, Account account) except *:
        """
        Update the given account in the execution cache.

        Parameters
        ----------
        account : The account to update (from last event).

        """
        Condition.not_none(account, "account")

        # Update database
        self._database.update_account(account)

    cpdef void update_order(self, Order order) except *:
        """
        Update the given order in the execution cache.

        Parameters
        ----------
        order : Order
            The order to update (from last event).

        """
        Condition.not_none(order, "order")

        if order.is_working_c():
            self._index_orders_working.add(order.cl_ord_id)
            self._index_orders_completed.discard(order.cl_ord_id)
        elif order.is_completed_c():
            self._index_orders_completed.add(order.cl_ord_id)
            self._index_orders_working.discard(order.cl_ord_id)

        # Update database
        self._database.update_order(order)

    cpdef void update_position(self, Position position) except *:
        """
        Update the given position in the execution cache.

        Parameters
        ----------
        position : Position
            The position to update (from last event).

        """
        Condition.not_none(position, "position")

        if position.is_closed:
            self._index_positions_closed.add(position.id)
            self._index_positions_open.discard(position.id)

        # Update database
        self._database.update_position(position)

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        """
        Update the given strategy state in the execution cache.

        Parameters
        ----------
        strategy : TradingStrategy
            The strategy to update.
        """
        Condition.not_none(strategy, "strategy")

        self._index_strategies.add(strategy.id)
        self._log.info(f"Saving {strategy.id} (in-memory cache does nothing).")

        # Update database
        self._database.update_strategy(strategy)

    cpdef void delete_strategy(self, TradingStrategy strategy) except *:
        """
        Delete the given strategy from the execution cache.
        Logs error if strategy not found in the cache.

        Parameters
        ----------
        strategy : TradingStrategy
            The strategy to deregister.

        Raises
        ------
        ValueError
            If strategy is not contained in the strategies.

        """
        Condition.not_none(strategy, "strategy")
        Condition.is_in(strategy.id, self._index_strategies, "strategy.id", "strategies")

        self._index_strategies.discard(strategy.id)

        if strategy.id in self._index_strategy_orders:
            del self._index_strategy_orders[strategy.id]

        if strategy.id in self._index_strategy_positions:
            del self._index_strategy_positions[strategy.id]

        # Update database
        self._database.delete_strategy(strategy.id)
        self._log.debug(f"Deleted Strategy(id={strategy.id.value}).")

    cpdef void check_residuals(self) except *:
        # Check for any residual active orders and log warnings if any are found
        for order in self.orders_working():
            self._log.warning(f"Residual {order}")

        for position in self.positions_open():
            self._log.warning(f"Residual {position}")

    cpdef void reset(self) except *:
        """
        Reset the cache.

        All stateful values are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        self._cached_accounts.clear()
        self._cached_orders.clear()
        self._cached_positions.clear()
        self._clear_indexes()

        self._log.info(f"Reset.")

    cpdef void flush_db(self) except *:
        """
        Flush the execution database which permanently removes all persisted data.

        WARNING: Permanent data loss.

        """
        self._log.debug("Flushing execution database...")
        self._database.flush()
        self._log.info("Execution database flushed.")

    cdef void _clear_indexes(self) except *:
        self._log.debug(f"Clearing indexes...")
        self._index_venue_account.clear()
        self._index_order_position.clear()
        self._index_order_strategy.clear()
        self._index_position_strategy.clear()
        self._index_position_orders.clear()
        self._index_symbol_orders.clear()
        self._index_symbol_positions.clear()
        self._index_strategy_orders.clear()
        self._index_strategy_positions.clear()
        self._index_orders.clear()
        self._index_orders_working.clear()
        self._index_orders_completed.clear()
        self._index_positions.clear()
        self._index_positions_open.clear()
        self._index_positions_closed.clear()
        self._index_strategies.clear()
        self._log.info(f"Indexes cleared.")

# -- ACCOUNT QUERIES -------------------------------------------------------------------------------

    cpdef Account account(self, AccountId account_id):
        """
        Return the account matching the given identifier (if found).

        Parameters
        ----------
        account_id : AccountId
            The account identifier.

        Returns
        -------
        Account or None

        """
        Condition.not_none(account_id, "account_id")

        return self._cached_accounts.get(account_id)

    cpdef Account account_for_venue(self, Venue venue):
        """
        Return the account for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        Account or None

        """
        Condition.not_none(venue, "venue")

        cdef AccountId account_id = self._index_venue_account.get(venue)
        if account_id is None:
            return None

        return self._cached_accounts.get(account_id)

    cpdef AccountId account_id(self, Venue venue):
        """
        Return the account identifier for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        AccountId or None

        """
        Condition.not_none(venue, "venue")

        return self._index_venue_account.get(venue)

# -- IDENTIFIER QUERIES ----------------------------------------------------------------------------

    cdef inline set _build_ord_query_filter_set(self, Symbol symbol, StrategyId strategy_id):
        cdef set query = None

        # Build potential query set
        if symbol is not None:
            query = self._index_symbol_orders.get(symbol, set())
        if strategy_id is not None:
            if not query:
                query = self._index_strategy_orders.get(strategy_id, set())
            else:
                query = query.intersection(self._index_strategy_orders.get(strategy_id, set()))

        return query

    cdef inline set _build_pos_query_filter_set(self, Symbol symbol, StrategyId strategy_id):
        cdef set query = None

        # Build potential query set
        if symbol is not None:
            query = self._index_symbol_positions.get(symbol, set())
        if strategy_id is not None:
            if not query:
                query = self._index_strategy_positions.get(strategy_id, set())
            else:
                query = query.intersection(self._index_strategy_positions.get(strategy_id, set()))

        return query

    cpdef set order_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all client order identifiers with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        cdef set query = self._build_ord_query_filter_set(symbol, strategy_id)

        if not query:
            return self._index_orders
        else:
            return self._index_orders.intersection(query)

    cpdef set order_working_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all working client order identifiers with the given query
        filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        cdef set query = self._build_ord_query_filter_set(symbol, strategy_id)

        if not query:
            return self._index_orders_working
        else:
            return self._index_orders_working.intersection(query)

    cpdef set order_completed_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all completed client order identifiers with the given query
        filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        cdef set query = self._build_ord_query_filter_set(symbol, strategy_id)

        if not query:
            return self._index_orders_completed
        else:
            return self._index_orders_completed.intersection(query)

    cpdef set position_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all position identifiers with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        Set[PositionId]

        """
        cdef set query = self._build_pos_query_filter_set(symbol, strategy_id)

        if not query:
            return self._index_positions
        else:
            return self._index_positions.intersection(query)

    cpdef set position_open_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all open position identifiers with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        Set[PositionId]

        """
        cdef set query = self._build_pos_query_filter_set(symbol, strategy_id)

        if not query:
            return self._index_positions_open
        else:
            return self._index_positions_open.intersection(query)

    cpdef set position_closed_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all closed position identifiers with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        Set[PositionId]

        """
        cdef set query = self._build_pos_query_filter_set(symbol, strategy_id)

        if not query:
            return self._index_positions_closed
        else:
            return self._index_positions_closed.intersection(query)

    cpdef set strategy_ids(self):
        """
        Return all strategy identifiers.

        Returns
        -------
        Set[StrategyId]

        """
        return self._index_strategies.copy()

# -- ORDER QUERIES ---------------------------------------------------------------------------------

    cpdef Order order(self, ClientOrderId cl_ord_id):
        """
        Return the order matching the given identifier (if found).

        Returns
        -------
        Order or None

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return self._cached_orders.get(cl_ord_id)

    cpdef list orders(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all orders with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        list[Order]

        """
        cdef set cl_ord_ids = self.order_ids(symbol, strategy_id)

        cdef ClientOrderId cl_ord_id
        cdef list orders
        try:
            orders = [self._cached_orders[cl_ord_id] for cl_ord_id in cl_ord_ids]
        except KeyError as ex:
            self._log.error("Cannot find order object in cached orders " + str(ex))

        return orders

    cpdef list orders_working(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all working orders with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        list[Order]

        """
        cdef set cl_ord_ids = self.order_working_ids(symbol, strategy_id)

        cdef ClientOrderId cl_ord_id
        cdef list orders_working
        try:
            orders_working = [self._cached_orders[cl_ord_id] for cl_ord_id in cl_ord_ids]
        except KeyError as ex:
            self._log.error("Cannot find Order object in cache " + str(ex))

        return orders_working

    cpdef list orders_completed(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all completed orders with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        list[Order]

        """
        cdef set cl_ord_ids = self.order_completed_ids(symbol, strategy_id)

        cdef ClientOrderId cl_ord_id
        cdef list orders_completed
        try:
            orders_completed = [self._cached_orders[cl_ord_id] for cl_ord_id in cl_ord_ids]
        except KeyError as ex:
            self._log.error("Cannot find Order object in cache " + str(ex))

        return orders_completed

# -- POSITION QUERIES ------------------------------------------------------------------------------

    cpdef Position position(self, PositionId position_id):
        """
        Return the position associated with the given identifier (if found).

        Parameters
        ----------
        position_id : PositionId
            The position identifier.

        Returns
        -------
        Position or None

        """
        Condition.not_none(position_id, "position_id")

        return self._cached_positions.get(position_id)

    cpdef PositionId position_id(self, ClientOrderId cl_ord_id):
        """
        Return the position identifier associated with the given client order
        identifier (if found).

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier associated with the position.

        Returns
        -------
        PositionId or None

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return self._index_order_position.get(cl_ord_id)

    cpdef list positions(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all positions with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        list[Position]

        """
        cdef set position_ids = self.position_ids(symbol, strategy_id)

        cdef PositionId position_id
        cdef list positions
        try:
            positions = [self._cached_positions[position_id] for position_id in position_ids]
        except KeyError as ex:
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef list positions_open(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all open positions with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy_id query filter.

        Returns
        -------
        list[Position]

        """
        cdef set position_ids = self.position_open_ids(symbol, strategy_id)

        cdef PositionId position_id
        cdef list positions
        try:
            positions = [self._cached_positions[position_id] for position_id in position_ids]
        except KeyError as ex:
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef list positions_closed(self, Symbol symbol=None, StrategyId strategy_id=None):
        """
        Return all closed positions with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy_id query filter.

        Returns
        -------
        list[Position]

        """
        cdef set position_ids = self.position_closed_ids(symbol, strategy_id)

        cdef PositionId position_id
        cdef list positions
        try:
            positions = [self._cached_positions[position_id] for position_id in position_ids]
        except KeyError as ex:
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef bint order_exists(self, ClientOrderId cl_ord_id) except *:
        """
        Return a value indicating whether an order with the given identifier
        exists.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier to check.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_orders

    cpdef bint is_order_working(self, ClientOrderId cl_ord_id) except *:
        """
        Return a value indicating whether an order with the given identifier is
        working.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier to check.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_orders_working

    cpdef bint is_order_completed(self, ClientOrderId cl_ord_id) except *:
        """
        Return a value indicating whether an order with the given identifier is
        completed.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier to check.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_orders_completed

    cpdef int orders_total_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """
        Return the total count of orders with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy_id query filter.

        Returns
        -------
        int

        """
        return len(self.order_ids(symbol, strategy_id))

    cpdef int orders_working_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """
        Return the count of working orders with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy_id query filter.

        Returns
        -------
        int

        """
        return len(self.order_working_ids(symbol, strategy_id))

    cpdef int orders_completed_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """
        Return the count of completed orders with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy_id query filter.

        Returns
        -------
        int

        """
        return len(self.order_completed_ids(symbol, strategy_id))

    cpdef bint position_exists(self, PositionId position_id) except *:
        """
        Return a value indicating whether a position with the given identifier
        exists.

        Parameters
        ----------
        position_id : PositionId
            The position identifier.

        Returns
        -------
        int

        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions

    cpdef bint position_exists_for_order(self, ClientOrderId cl_ord_id) except *:
        """
        Return a value indicating whether there is a position associated with
        the given client order identifier.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        cdef PositionId position_id = self._index_order_position.get(cl_ord_id)
        if position_id is None:
            return False
        return position_id in self._index_positions

    cpdef bint position_indexed_for_order(self, ClientOrderId cl_ord_id) except *:
        """
        Return a value indicating whether there is a position identifier indexed
        for the given identifier.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_order_position

    cpdef bint is_position_open(self, PositionId position_id) except *:
        """
        Return a value indicating whether a position with the given identifier
        exists and is open.

        Parameters
        ----------
        position_id : PositionId
            The position identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions_open

    cpdef bint is_position_closed(self, PositionId position_id) except *:
        """
        Return a value indicating whether a position with the given identifier
        exists and is closed.

        Parameters
        ----------
        position_id : PositionId
            The position identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions_closed

    cpdef int positions_total_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """
        Return the total count of positions with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        int

        """
        return len(self.position_ids(symbol, strategy_id))

    cpdef int positions_open_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """
        Return the count of open positions with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        int

        """
        return len(self.position_open_ids(symbol, strategy_id))

    cpdef int positions_closed_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """
        Return the count of closed positions with the given query filters.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol identifier query filter.
        strategy_id : StrategyId, optional
            The strategy identifier query filter.

        Returns
        -------
        int

        """
        return len(self.position_closed_ids(symbol, strategy_id))

# -- STRATEGY QUERIES ------------------------------------------------------------------------------

    cpdef StrategyId strategy_id_for_order(self, ClientOrderId cl_ord_id):
        """
        Return the strategy identifier associated with the given identifier (if found).

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier associated with the strategy.

        Returns
        -------
        StrategyId or None

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return self._index_order_strategy.get(cl_ord_id)

    cpdef StrategyId strategy_id_for_position(self, PositionId position_id):
        """
        Return the strategy identifier associated with the given identifier (if found).

        Parameters
        ----------
        position_id : PositionId
            The position identifier associated with the strategy.

        Returns
        -------
        StrategyId or None

        """
        Condition.not_none(position_id, "position_id")

        return self._index_position_strategy.get(position_id)

    cdef inline Decimal _sum_net_position(self, Symbol symbol, StrategyId strategy_id):
        cdef list positions = self.positions_open(symbol, strategy_id)
        cdef Decimal net_quantity = Decimal()

        cdef Position position
        for position in positions:
            if position.is_long:
                net_quantity = Decimal(net_quantity + position.quantity)
            elif position.is_short:
                net_quantity = Decimal(net_quantity - position.quantity)

        return net_quantity
