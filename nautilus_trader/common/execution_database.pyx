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
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ClientPositionId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport Order
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionDatabase:
    """
    The base class for all execution databases.
    """

    def __init__(self, TraderId trader_id not None, Logger logger not None):
        """
        Initialize a new instance of the ExecutionDatabase class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the component.
        logger : Logger
            The logger for the component.

        """
        self.trader_id = trader_id
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self._cached_accounts = {}   # type: {AccountId, Account}
        self._cached_orders = {}     # type: {ClientOrderId, Order}
        self._cached_positions = {}  # type: {ClientPositionId, Position}

    # -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void add_account(self, Account account) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_order(self, Order order, StrategyId strategy_id, ClientPositionId cl_pos_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_account(self, Account event) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_order(self, Order order) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_position(self, Position position) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Account load_account(self, AccountId account_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Order load_order(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Position load_position(self, ClientPositionId cl_pos_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void load_strategy(self, TradingStrategy strategy) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void delete_strategy(self, TradingStrategy strategy) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void check_residuals(self) except *:
        # Check for any residual active orders and log warnings if any are found
        for cl_ord_id, order in self.get_orders_working().items():
            self._log.warning(f"Residual {order}")

        for cl_pos_id, position in self.get_positions_open().items():
            self._log.warning(f"Residual {position}")

    cpdef void reset(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void flush(self) except *:
        raise NotImplementedError(f"method must be implemented in the subclass")

    cdef void _reset(self) except *:
        # Reset the class to its initial state
        self._log.debug(f"Resetting...")

        self._cached_accounts.clear()
        self._cached_orders.clear()
        self._cached_positions.clear()

        self._log.info(f"Reset.")

    # -- QUERIES ---------------------------------------------------------------------------------------

    cpdef Account get_account(self, AccountId account_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_strategy_ids(self):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_order_ids(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_order_working_ids(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_order_completed_ids(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_position_ids(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_position_open_ids(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_position_closed_ids(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef StrategyId get_strategy_for_order(self, ClientOrderId order_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef StrategyId get_strategy_for_position(self, ClientPositionId cl_pos_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Order get_order(self, ClientOrderId order_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_orders(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_orders_working(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_orders_completed(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Position get_position(self, ClientPositionId cl_pos_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Position get_position_for_order(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef ClientPositionId get_client_position_id(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef ClientPositionId get_client_position_id_for_id(self, PositionId position_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_positions(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_positions_open(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_positions_closed(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint order_exists(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_working(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_completed(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_exists(self, ClientPositionId cl_pos_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_exists_for_order(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_indexed_for_order(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_position_open(self, ClientPositionId cl_pos_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_position_closed(self, ClientPositionId cl_pos_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_orders_total(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_orders_working(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_orders_completed(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_positions_total(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_positions_open(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_positions_closed(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class InMemoryExecutionDatabase(ExecutionDatabase):
    """
    Provides an in-memory execution database.
    """

    def __init__(self, TraderId trader_id not None, Logger logger not None):
        """
        Initialize a new instance of the InMemoryExecutionDatabase class.

        :param trader_id: The trader_id for the component.
        :param logger: The logger for the component.
        """
        super().__init__(trader_id, logger)

        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._strategies = set()              # type: {StrategyId}
        self._index_order_position = {}       # type: {ClientOrderId, ClientPositionId}
        self._index_order_strategy = {}       # type: {ClientOrderId, StrategyId}
        self._index_broker_position = {}      # type: {PositionId, ClientPositionId}
        self._index_position_strategy = {}    # type: {ClientPositionId, StrategyId}
        self._index_position_orders = {}      # type: {ClientPositionId, {ClientOrderId}}
        self._index_strategy_orders = {}      # type: {StrategyId, {ClientOrderId}}
        self._index_strategy_positions = {}   # type: {StrategyId, {ClientPositionId}}
        self._index_orders = set()            # type: {ClientOrderId}
        self._index_orders_working = set()    # type: {ClientOrderId}
        self._index_orders_completed = set()  # type: {ClientOrderId}
        self._index_positions = set()         # type: {ClientPositionId}
        self._index_positions_open = set()    # type: {ClientPositionId}
        self._index_positions_closed = set()  # type: {ClientPositionId}

    # -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void add_account(self, Account account) except *:
        """
        Add the given account to the execution database.

        :param account: The account to add.
        :raises ValueError: If account_id is already contained in the cached_accounts.
        """
        Condition.not_none(account, "account")
        Condition.not_in(account.id, self._cached_accounts, "account.id", "cached_accounts")

        self._cached_accounts[account.id] = account

        self._log.debug(f"Added Account(id={account.id.value}).")

    cpdef void add_order(self, Order order, StrategyId strategy_id, ClientPositionId position_id) except *:
        """
        Add the given order to the execution database indexed with the given strategy and position
        identifiers.

        :param order: The order to add.
        :param strategy_id: The strategy identifier to index for the order.
        :param position_id: The position identifier to index for the order.
        :raises ValueError: If order.id is already contained in the cached_orders.
        :raises ValueError: If order.id is already contained in the index_orders.
        :raises ValueError: If order.id is already contained in the index_order_strategy.
        :raises ValueError: If order.id is already contained in the index_order_position.
        """
        Condition.not_none(order, "order")
        Condition.not_none(strategy_id, "strategy_id")
        Condition.not_none(position_id, "position_id")
        Condition.not_in(order.cl_ord_id, self._cached_orders, "order.cl_ord_id", "cached_orders")
        Condition.not_in(order.cl_ord_id, self._index_orders, "order.cl_ord_id", "index_orders")
        Condition.not_in(order.cl_ord_id, self._index_order_strategy, "order.cl_ord_id", "index_order_strategy")
        Condition.not_in(order.cl_ord_id, self._index_order_position, "order.cl_ord_id", "index_order_position")

        self._cached_orders[order.cl_ord_id] = order

        self._index_orders.add(order.cl_ord_id)
        self._index_order_strategy[order.cl_ord_id] = strategy_id
        self._index_order_position[order.cl_ord_id] = position_id

        # Index: PositionId -> StrategyId
        if position_id not in self._index_position_strategy:
            self._index_position_strategy[position_id] = strategy_id
        else:
            assert strategy_id.equals(self._index_position_strategy[position_id])

        # Index: PositionId -> Set[OrderId]
        if position_id not in self._index_position_orders:
            self._index_position_orders[position_id] = {order.cl_ord_id}
        else:
            self._index_position_orders[position_id].add(order.cl_ord_id)

        # Index: StrategyId -> Set[OrderId]
        if strategy_id not in self._index_strategy_orders:
            self._index_strategy_orders[strategy_id] = {order.cl_ord_id}
        else:
            self._index_strategy_orders[strategy_id].add(order.cl_ord_id)

        # Index: StrategyId -> Set[PositionId]
        if strategy_id not in self._index_strategy_positions:
            self._index_strategy_positions[strategy_id] = {position_id}
        else:
            self._index_strategy_positions[strategy_id].add(position_id)

        self._log.debug(f"Added Order(cl_ord_id={order.cl_ord_id.value}).")

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        """
        Add the given position associated with the given strategy_id.

        :param position: The position to add.
        :param strategy_id: The strategy_id to associate with the position.
        :raises ValueError: If position.id is already contained in the cached_positions.
        :raises ValueError: If position.cl_pos_id is already contained in the index_broker_position.
        :raises ValueError: If position.id is already contained in the index_positions.
        :raises ValueError: If position.id is already contained in the index_positions_open.
        """
        Condition.not_none(position, "position")
        Condition.not_none(strategy_id, "strategy_id")
        Condition.not_in(position.cl_pos_id, self._cached_positions, "position.cl_pos_id", "cached_positions")
        Condition.not_in(position.id, self._index_broker_position, "position.id", "index_broker_position")
        Condition.not_in(position.cl_pos_id, self._index_positions, "position.cl_pos_id", "index_positions")
        Condition.not_in(position.cl_pos_id, self._index_positions_open, "position.cl_pos_id", "index_positions_open")

        self._cached_positions[position.cl_pos_id] = position

        self._index_broker_position[position.id] = position.cl_pos_id
        self._index_positions.add(position.cl_pos_id)
        self._index_positions_open.add(position.cl_pos_id)
        self._log.debug(f"Added Position(id={position.cl_pos_id.value}).")

    cpdef void update_account(self, Account account) except *:
        """
        Update the given account in the execution database.

        :param account: The account to update (from last event).
        """
        # Do nothing in memory
        pass

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        """
        Update the given strategy state in the execution database .

        :param strategy: The strategy to update.
        """
        Condition.not_none(strategy, "strategy")

        self._log.info(f"Saving {strategy.id} (in-memory database does nothing).")
        self._strategies.add(strategy.id)

    cpdef void update_order(self, Order order) except *:
        """
        Update the given order in the execution database.

        :param order: The order to update (from last event).
        """
        Condition.not_none(order, "order")

        if order.is_working():
            self._index_orders_working.add(order.cl_ord_id)
            self._index_orders_completed.discard(order.cl_ord_id)
        elif order.is_completed():
            self._index_orders_completed.add(order.cl_ord_id)
            self._index_orders_working.discard(order.cl_ord_id)

    cpdef void update_position(self, Position position) except *:
        """
        Update the given position in the execution database.

        :param position: The position to update (from last event).
        """
        Condition.not_none(position, "position")

        if position.is_closed():
            self._index_positions_closed.add(position.cl_pos_id)
            self._index_positions_open.discard(position.cl_pos_id)

    cpdef void load_strategy(self, TradingStrategy strategy) except *:
        """
        Load the state for the given strategy from the execution database.

        :param strategy: The strategy to load.
        """
        Condition.not_none(strategy, "strategy")

        self._log.info(f"Loading {strategy.id} (in-memory database does nothing).")
        # Do nothing in memory

    cpdef Account load_account(self, AccountId account_id):
        """
        Load the account associated with the given account_id (if found).

        :param account_id: The account identifier to load.
        :return: Account or None.
        """
        Condition.not_none(account_id, "account_id")

        return self._cached_accounts.get(account_id)

    cpdef Order load_order(self, ClientOrderId cl_ord_id):
        """
        Load the order associated with the given identifier (if found).

        :param cl_ord_id: The client order identifier to load.
        :return: Order or None.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return self._cached_orders.get(cl_ord_id)

    cpdef Position load_position(self, ClientPositionId cl_pos_id):
        """
        Load the position associated with the given identifier (if found).

        :param cl_pos_id: The client position identifier to load.
        :return: Position or None.
        """
        Condition.not_none(cl_pos_id, "cl_pos_id")

        return self._cached_positions.get(cl_pos_id)

    cpdef void delete_strategy(self, TradingStrategy strategy) except *:
        """
        Delete the given strategy from the execution database.

        :param strategy: The strategy to deregister.
        :raises ValueError: If strategy is not contained in the strategies.
        """
        Condition.not_none(strategy, "strategy")
        Condition.is_in(strategy.id, self._strategies, "strategy.id", "strategies")

        self._strategies.discard(strategy.id)

        if strategy.id in self._index_strategy_orders:
            del self._index_strategy_orders[strategy.id]

        if strategy.id in self._index_strategy_positions:
            del self._index_strategy_positions[strategy.id]

        self._log.debug(f"Deleted Strategy(id={strategy.id.value}).")

    cpdef void reset(self) except *:
        # Reset the execution database by clearing all stateful values
        self._log.debug(f"Resetting...")

        self._strategies.clear()
        self._index_order_position.clear()
        self._index_order_strategy.clear()
        self._index_broker_position.clear()
        self._index_position_strategy.clear()
        self._index_position_orders.clear()
        self._index_strategy_orders.clear()
        self._index_strategy_positions.clear()
        self._index_orders.clear()
        self._index_orders_working.clear()
        self._index_orders_completed.clear()
        self._index_positions.clear()
        self._index_positions_open.clear()
        self._index_positions_closed.clear()

        self._reset()

    cpdef void flush(self) except *:
        """
        Flush the database which clears all data.
        """
        self._log.info("Flushing database (in-memory database does nothing).")

    # -- QUERIES ---------------------------------------------------------------------------------------

    cpdef Account get_account(self, AccountId account_id):
        """
        Return the account matching the given identifier (if found).

        :param account_id: The account identifier.
        :return Account or None.
        """
        Condition.not_none(account_id, "account_id")

        return self._cached_accounts.get(account_id)

    cpdef set get_strategy_ids(self):
        """
        Return a set of all strategy_ids.

        :return Set[StrategyId].
        """
        return self._strategies.copy()

    cpdef set get_order_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all client order identifiers.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[ClientOrderId].
        """
        if strategy_id is None:
            return self._index_orders.copy()

        cdef set strategy_order_ids = self._index_strategy_orders.get(strategy_id)
        if strategy_order_ids is None:
            return set()  # Empty set

        return self._index_orders.intersection(strategy_order_ids)

    cpdef set get_order_working_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all working client order identifiers.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[ClientOrderId].
        """
        if strategy_id is None:
            return self._index_orders_working.copy()

        cdef set strategy_order_ids = self._index_strategy_orders.get(strategy_id)
        if strategy_order_ids is None:
            return set()  # Empty set

        return self._index_orders_working.intersection(strategy_order_ids)

    cpdef set get_order_completed_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all completed client order identifiers.

        :param strategy_id: The optional strategy identifier query filter.
        :return Set[ClientOrderId].
        """
        if strategy_id is None:
            return self._index_orders_completed.copy()

        cdef set strategy_order_ids = self._index_strategy_orders.get(strategy_id)
        if strategy_order_ids is None:
            return set()  # Empty set

        return self._index_orders_completed.intersection(strategy_order_ids)

    cpdef set get_position_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all client position identifiers.

        :param strategy_id: The optional strategy identifier query filter.
        :return Set[ClientPositionId].
        """
        if strategy_id is None:
            return self._index_positions.copy()

        cdef set strategy_position_ids = self._index_strategy_positions.get(strategy_id)
        if strategy_position_ids is None:
            return set()  # Empty set

        return self._index_positions.intersection(strategy_position_ids)

    cpdef set get_position_open_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all open client position identifiers.

        :param strategy_id: The optional strategy identifier query filter.
        :return Set[ClientPositionId].
        """
        if strategy_id is None:
            return self._index_positions_open.copy()

        cdef set strategy_position_ids = self._index_strategy_positions.get(strategy_id)

        if strategy_position_ids is None:
            return set()  # Empty set

        return self._index_positions_open.intersection(strategy_position_ids)

    cpdef set get_position_closed_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all closed position identifiers.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[ClientPositionId].
        """
        if strategy_id is None:
            return self._index_positions_closed.copy()

        cdef set strategy_position_ids = self._index_strategy_positions.get(strategy_id)
        if strategy_position_ids is None:
            return set()  # Empty set

        return self._index_positions_closed.intersection(strategy_position_ids)

    cpdef StrategyId get_strategy_for_order(self, ClientOrderId cl_ord_id):
        """
        Return the strategy_id associated with the given identifier (if found).

        :param cl_ord_id: The client order identifier associated with the strategy.
        :return StrategyId or None.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return self._index_order_strategy.get(cl_ord_id)

    cpdef StrategyId get_strategy_for_position(self, ClientPositionId cl_pos_id):
        """
        Return the strategy_id associated with the given identifier (if found).

        :param cl_pos_id: The client position identifier associated with the strategy.
        :return StrategyId or None.
        """
        Condition.not_none(cl_pos_id, "cl_pos_id")

        return self._index_position_strategy.get(cl_pos_id)

    cpdef Order get_order(self, ClientOrderId cl_ord_id):
        """
        Return the order matching the given identifier (if found).

        :return Order or None.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return self._cached_orders.get(cl_ord_id)

    cpdef dict get_orders(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all orders.

        :param strategy_id: The optional strategy_id query filter.
        :return Dict[OrderId, Order].
        """
        cdef set order_ids = self.get_order_ids(strategy_id)
        cdef dict orders

        try:
            orders = {order_id: self._cached_orders[order_id] for order_id in order_ids}
        except KeyError as ex:
            self._log.error("Cannot find order object in cached orders " + str(ex))

        return orders

    cpdef dict get_orders_working(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all working orders.

        :param strategy_id: The optional strategy_id query filter.
        :return Dict[OrderId, Order].
        """
        cdef set order_ids = self.get_order_working_ids(strategy_id)
        cdef dict orders_working

        try:
            orders_working = {order_id: self._cached_orders[order_id] for order_id in order_ids}
        except KeyError as ex:
            self._log.error("Cannot find Order object in cache " + str(ex))

        return orders_working

    cpdef dict get_orders_completed(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all completed orders.

        :param strategy_id: The optional strategy_id query filter.
        :return Dict[OrderId, Order].
        """
        cdef set order_ids = self.get_order_completed_ids(strategy_id)
        cdef dict orders_completed

        try:
            orders_completed = {order_id: self._cached_orders[order_id] for order_id in order_ids}
        except KeyError as ex:
            self._log.error("Cannot find Order object in cache " + str(ex))

        return orders_completed

    cpdef Position get_position(self, ClientPositionId cl_pos_id):
        """
        Return the position associated with the given identifier (if found, else None).

        :param cl_pos_id: The client position identifier.
        :return Position or None.
        """
        Condition.not_none(cl_pos_id, "cl_pos_id")

        return self._cached_positions.get(cl_pos_id)

    cpdef Position get_position_for_order(self, ClientOrderId cl_ord_id):
        """
        Return the position associated with the given client order identifier (if found, else None).

        :param cl_ord_id: The client order identifier for the position.
        :return Position or None.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        cdef ClientPositionId position_id = self.get_client_position_id(cl_ord_id)
        if position_id is None:
            self._log.warning(f"Cannot get Position for {cl_ord_id.to_string(with_class=True)} "
                              f"(no matching PositionId found).")
            return None

        return self._cached_positions.get(position_id)

    cpdef ClientPositionId get_client_position_id(self, ClientOrderId cl_ord_id):
        """
        Return the client position identifier associated with the given client order identifier
        (if found, else None).

        :param cl_ord_id: The client order identifier associated with the position.
        :return PositionId or None.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        cdef ClientPositionId position_id = self._index_order_position.get(cl_ord_id)
        if position_id is None:
            self._log.warning(f"Cannot get PositionId for {cl_ord_id.to_string(with_class=True)} "
                              f"(no matching PositionId found).")

        return position_id

    cpdef ClientPositionId get_client_position_id_for_id(self, PositionId position_id):
        """
        Return the position_id associated with the given position identifier (if found, else None).

        :param position_id: The broker/exchange position identifier.
        :return PositionId or None.
        """
        Condition.not_none(position_id, "position_id")

        cdef ClientPositionId cl_pos_id = self._index_broker_position.get(position_id)
        if cl_pos_id is None:
            self._log.warning(f"Cannot get ClientPositionId for {position_id.to_string(with_class=True)} "
                              f"(no matching PositionId found).")

        return position_id

    cpdef dict get_positions(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all positions.

        :param strategy_id: The optional strategy identifier query filter.
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_ids(strategy_id)
        cdef dict positions

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef dict get_positions_open(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all open positions.

        :param strategy_id: The optional strategy_id query filter.
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_open_ids(strategy_id)
        cdef dict positions

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef dict get_positions_closed(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all closed positions.

        :param strategy_id: The optional strategy_id query filter.
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_closed_ids(strategy_id)
        cdef dict positions

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef bint order_exists(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether an order with the given identifier exists.

        :param cl_ord_id: The client order identifier to check.
        :return bool.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_orders

    cpdef bint is_order_working(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether an order with the given identifier is working.

        :param cl_ord_id: The client order identifier to check.
        :return bool.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_orders_working

    cpdef bint is_order_completed(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether an order with the given identifier is completed.

        :param cl_ord_id: The client order identifier to check.
        :return bool.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_orders_completed

    cpdef bint position_exists(self, ClientPositionId cl_pos_id):
        """
        Return a value indicating whether a position with the given identifier exists.

        :param cl_pos_id: The client position identifier.
        :return bool.
        """
        Condition.not_none(cl_pos_id, "cl_pos_id")

        return cl_pos_id in self._index_positions  # Only open positions added here

    cpdef bint position_exists_for_order(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether there is a position associated with the given
        identifier.

        :param cl_ord_id: The client order identifier.
        :return bool.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        cdef ClientPositionId position_id = self._index_order_position.get(cl_ord_id)
        if position_id is None:
            return False
        return position_id in self._index_positions

    cpdef bint position_indexed_for_order(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether there is a position_id indexed for the
        given identifier.

        :param cl_ord_id: The client order identifier.
        :return bool.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_order_position

    cpdef bint is_position_open(self, ClientPositionId cl_ord_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is open.

        :param cl_ord_id: The position identifier.
        :return bool.
        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._index_positions_open

    cpdef bint is_position_closed(self, ClientPositionId cl_pos_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is closed.

        :param cl_pos_id: The client position identifier.
        :return bool.
        """
        Condition.not_none(cl_pos_id, "cl_pos_id")

        return cl_pos_id in self._index_positions_closed

    cpdef int count_orders_total(self, StrategyId strategy_id=None):
        """
        Return the count of order held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_order_ids(strategy_id))

    cpdef int count_orders_working(self, StrategyId strategy_id=None):
        """
        Return the count of working orders held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_order_working_ids(strategy_id))

    cpdef int count_orders_completed(self, StrategyId strategy_id=None):
        """
        Return the count of completed orders held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_order_completed_ids(strategy_id))

    cpdef int count_positions_total(self, StrategyId strategy_id=None):
        """
        Return the count of positions held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_position_ids(strategy_id))

    cpdef int count_positions_open(self, StrategyId strategy_id=None):
        """
        Return the count of open positions held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_position_open_ids(strategy_id))

    cpdef int count_positions_closed(self, StrategyId strategy_id=None):
        """
        Return the count of closed positions held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_position_closed_ids(strategy_id))
