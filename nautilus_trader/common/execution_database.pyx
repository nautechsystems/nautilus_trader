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
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport PositionIdBroker
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

        :param trader_id: The trader_id for the component.
        :param logger: The logger for the component.
        """
        self.trader_id = trader_id
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self._cached_accounts = {}   # type: {AccountId, Account}
        self._cached_orders = {}     # type: {OrderId, Order}
        self._cached_positions = {}  # type: {PositionId, Position}


    # -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void add_account(self, Account account) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_account(self, Account event) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_order(self, Order order) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_position(self, Position position) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Account load_account(self, AccountId account_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Order load_order(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Position load_position(self, PositionId position_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void load_strategy(self, TradingStrategy strategy) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void delete_strategy(self, TradingStrategy strategy) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void check_residuals(self) except *:
        # Check for any residual active orders and log warnings if any are found
        for order_id, order in self.get_orders_working().items():
            self._log.warning(f"Residual {order}")

        for position_id, position in self.get_positions_open().items():
            self._log.warning(f"Residual {position}")

    cpdef void reset(self) except *:
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
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_strategy_ids(self):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_order_ids(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_order_working_ids(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_order_completed_ids(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_position_ids(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_position_open_ids(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set get_position_closed_ids(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef StrategyId get_strategy_for_order(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef StrategyId get_strategy_for_position(self, PositionId position_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Order get_order(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_orders(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_orders_working(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_orders_completed(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Position get_position(self, PositionId position_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Position get_position_for_order(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef PositionId get_position_id(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef PositionId get_position_id_for_broker_id(self, PositionIdBroker position_id_broker):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_positions(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_positions_open(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_positions_closed(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint order_exists(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_working(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_completed(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_exists(self, PositionId position_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_exists_for_order(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_indexed_for_order(self, OrderId order_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_position_open(self, PositionId position_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_position_closed(self, PositionId position_id):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_orders_total(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_orders_working(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_orders_completed(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_positions_total(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_positions_open(self, StrategyId strategy_id=None):
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int count_positions_closed(self, StrategyId strategy_id=None):
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
        self._index_order_position = {}       # type: {OrderId, PositionId}
        self._index_order_strategy = {}       # type: {OrderId, StrategyId}
        self._index_broker_position = {}      # type: {PositionIdBroker, PositionId}
        self._index_position_strategy = {}    # type: {PositionId, StrategyId}
        self._index_position_orders = {}      # type: {PositionId, {OrderId}}
        self._index_strategy_orders = {}      # type: {StrategyId, {OrderId}}
        self._index_strategy_positions = {}   # type: {StrategyId, {PositionId}}
        self._index_orders = set()            # type: {OrderId}
        self._index_orders_working = set()    # type: {OrderId}
        self._index_orders_completed = set()  # type: {OrderId}
        self._index_positions = set()         # type: {PositionId}
        self._index_positions_open = set()    # type: {PositionId}
        self._index_positions_closed = set()  # type: {PositionId}


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

    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id) except *:
        """
        Add the given order to the execution database indexed with the given strategy and position
        identifiers.

        :param order: The order to add.
        :param strategy_id: The strategy_id to index for the order.
        :param position_id: The position_id to index for the order.
        :raises ValueError: If order.id is already contained in the cached_orders.
        :raises ValueError: If order.id is already contained in the index_orders.
        :raises ValueError: If order.id is already contained in the index_order_strategy.
        :raises ValueError: If order.id is already contained in the index_order_position.
        """
        Condition.not_none(order, "order")
        Condition.not_none(strategy_id, "strategy_id")
        Condition.not_none(position_id, "position_id")
        Condition.not_in(order.id, self._cached_orders, "order.id", "cached_orders")
        Condition.not_in(order.id, self._index_orders, "order.id", "index_orders")
        Condition.not_in(order.id, self._index_order_strategy, "order.id", "index_order_strategy")
        Condition.not_in(order.id, self._index_order_position, "order.id", "index_order_position")

        self._cached_orders[order.id] = order

        self._index_orders.add(order.id)
        self._index_order_strategy[order.id] = strategy_id
        self._index_order_position[order.id] = position_id

        # Index: PositionId -> StrategyId
        if position_id not in self._index_position_strategy:
            self._index_position_strategy[position_id] = strategy_id
        else:
            assert strategy_id.equals(self._index_position_strategy[position_id])

        # Index: PositionId -> Set[OrderId]
        if position_id not in self._index_position_orders:
            self._index_position_orders[position_id] = {order.id}
        else:
            self._index_position_orders[position_id].add(order.id)

        # Index: StrategyId -> Set[OrderId]
        if strategy_id not in self._index_strategy_orders:
            self._index_strategy_orders[strategy_id] = {order.id}
        else:
            self._index_strategy_orders[strategy_id].add(order.id)

        # Index: StrategyId -> Set[PositionId]
        if strategy_id not in self._index_strategy_positions:
            self._index_strategy_positions[strategy_id] = {position_id}
        else:
            self._index_strategy_positions[strategy_id].add(position_id)

        self._log.debug(f"Added Order(id={order.id.value}).")

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        """
        Add the given position associated with the given strategy_id.

        :param position: The position to add.
        :param strategy_id: The strategy_id to associate with the position.
        :raises ValueError: If position.id is already contained in the cached_positions.
        :raises ValueError: If position.id_broker is already contained in the index_broker_position.
        :raises ValueError: If position.id is already contained in the index_positions.
        :raises ValueError: If position.id is already contained in the index_positions_open.
        """
        Condition.not_none(position, "position")
        Condition.not_none(strategy_id, "strategy_id")
        Condition.not_in(position.id, self._cached_positions, "position.id", "cached_positions")
        Condition.not_in(position.id_broker, self._index_broker_position, "position.id_broker", "index_broker_position")
        Condition.not_in(position.id, self._index_positions, "position.id", "index_positions")
        Condition.not_in(position.id, self._index_positions_open, "position.id", "index_positions_open")

        self._cached_positions[position.id] = position

        self._index_broker_position[position.id_broker] = position.id
        self._index_positions.add(position.id)
        self._index_positions_open.add(position.id)
        self._log.debug(f"Added Position(id={position.id.value}).")

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
            self._index_orders_working.add(order.id)
            self._index_orders_completed.discard(order.id)
        elif order.is_completed():
            self._index_orders_completed.add(order.id)
            self._index_orders_working.discard(order.id)

    cpdef void update_position(self, Position position) except *:
        """
        Update the given position in the execution database.

        :param position: The position to update (from last event).
        """
        Condition.not_none(position, "position")

        if position.is_closed():
            self._index_positions_closed.add(position.id)
            self._index_positions_open.discard(position.id)

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

    cpdef Order load_order(self, OrderId order_id):
        """
        Load the order associated with the given order_id (if found).

        :param order_id: The order_id to load.
        :return: Order or None.
        """
        Condition.not_none(order_id, "order_id")

        return self._cached_orders.get(order_id)

    cpdef Position load_position(self, PositionId position_id):
        """
        Load the position associated with the given position_id (if found).

        :param position_id: The position_id to load.
        :return: Position or None.
        """
        Condition.not_none(position_id, "position_id")

        return self._cached_positions.get(position_id)

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

        :param account_id: The account_id.
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
        Return a set of all order_ids.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[OrderId].
        """
        if strategy_id is None:
            return self._index_orders.copy()

        cdef set strategy_order_ids = self._index_strategy_orders.get(strategy_id)
        if strategy_order_ids is None:
            return set()  # Empty set

        return self._index_orders.intersection(strategy_order_ids)

    cpdef set get_order_working_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all working order_ids.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[OrderId].
        """
        if strategy_id is None:
            return self._index_orders_working.copy()

        cdef set strategy_order_ids = self._index_strategy_orders.get(strategy_id)
        if strategy_order_ids is None:
            return set()  # Empty set

        return self._index_orders_working.intersection(strategy_order_ids)

    cpdef set get_order_completed_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all completed order_ids.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[OrderId].
        """
        if strategy_id is None:
            return self._index_orders_completed.copy()

        cdef set strategy_order_ids = self._index_strategy_orders.get(strategy_id)
        if strategy_order_ids is None:
            return set()  # Empty set

        return self._index_orders_completed.intersection(strategy_order_ids)

    cpdef set get_position_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all position_ids.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[PositionId].
        """
        if strategy_id is None:
            return self._index_positions.copy()

        cdef set strategy_position_ids = self._index_strategy_positions.get(strategy_id)
        if strategy_position_ids is None:
            return set()  # Empty set

        return self._index_positions.intersection(strategy_position_ids)

    cpdef set get_position_open_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all open position_ids.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[PositionId].
        """
        if strategy_id is None:
            return self._index_positions_open.copy()

        cdef set strategy_position_ids = self._index_strategy_positions.get(strategy_id)

        if strategy_position_ids is None:
            return set()  # Empty set

        return self._index_positions_open.intersection(strategy_position_ids)

    cpdef set get_position_closed_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all closed position_ids.

        :param strategy_id: The optional strategy_id query filter.
        :return Set[PositionId].
        """
        if strategy_id is None:
            return self._index_positions_closed.copy()

        cdef set strategy_position_ids = self._index_strategy_positions.get(strategy_id)
        if strategy_position_ids is None:
            return set()  # Empty set

        return self._index_positions_closed.intersection(strategy_position_ids)

    cpdef StrategyId get_strategy_for_order(self, OrderId order_id):
        """
        Return the strategy_id associated with the given order_id (if found).

        :param order_id: The order_id associated with the strategy.
        :return StrategyId or None.
        """
        Condition.not_none(order_id, "order_id")

        return self._index_order_strategy.get(order_id)

    cpdef StrategyId get_strategy_for_position(self, PositionId position_id):
        """
        Return the strategy_id associated with the given position_id (if found).

        :param position_id: The position_id associated with the strategy.
        :return StrategyId or None.
        """
        Condition.not_none(position_id, "position_id")

        return self._index_position_strategy.get(position_id)

    cpdef Order get_order(self, OrderId order_id):
        """
        Return the order matching the given identifier (if found).

        :return Order or None.
        """
        Condition.not_none(order_id, "order_id")

        return self._cached_orders.get(order_id)

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

    cpdef Position get_position(self, PositionId position_id):
        """
        Return the position associated with the given position_id (if found, else None).

        :param position_id: The position_id.
        :return Position or None.
        """
        Condition.not_none(position_id, "position_id")

        return self._cached_positions.get(position_id)

    cpdef Position get_position_for_order(self, OrderId order_id):
        """
        Return the position associated with the given order_id (if found, else None).

        :param order_id: The order_id for the position.
        :return Position or None.
        """
        Condition.not_none(order_id, "order_id")

        cdef PositionId position_id = self.get_position_id(order_id)
        if position_id is None:
            self._log.warning(f"Cannot get Position for {order_id.to_string(with_class=True)} "
                              f"(no matching PositionId found).")
            return None

        return self._cached_positions.get(position_id)

    cpdef PositionId get_position_id(self, OrderId order_id):
        """
        Return the position_id associated with the given order_id (if found, else None).

        :param order_id: The order_id associated with the position.
        :return PositionId or None.
        """
        Condition.not_none(order_id, "order_id")

        cdef PositionId position_id = self._index_order_position.get(order_id)
        if position_id is None:
            self._log.warning(f"Cannot get PositionId for {order_id.to_string(with_class=True)} "
                              f"(no matching PositionId found).")

        return position_id

    cpdef PositionId get_position_id_for_broker_id(self, PositionIdBroker position_id_broker):
        """
        Return the position_id associated with the given broker position_id (if found, else None).

        :param position_id_broker: The broker position_id.
        :return PositionId or None.
        """
        Condition.not_none(position_id_broker, "position_id_broker")

        cdef PositionId position_id = self._index_broker_position.get(position_id_broker)
        if position_id is None:
            self._log.warning(f"Cannot get PositionId for {position_id_broker.to_string(with_class=True)} "
                              f"(no matching PositionId found).")

        return position_id

    cpdef dict get_positions(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all positions.

        :param strategy_id: The optional strategy_id query filter.
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_ids(strategy_id)
        cdef dict positions

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            # This should never happen
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
            # This should never happen
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
            # This should never happen
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef bint order_exists(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier exists.

        :param order_id: The order_id to check.
        :return bool.
        """
        Condition.not_none(order_id, "order_id")

        return order_id in self._index_orders

    cpdef bint is_order_working(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is working.

        :param order_id: The order_id to check.
        :return bool.
        """
        Condition.not_none(order_id, "order_id")

        return order_id in self._index_orders_working

    cpdef bint is_order_completed(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is completed.

        :param order_id: The order_id to check.
        :return bool.
        """
        Condition.not_none(order_id, "order_id")

        return order_id in self._index_orders_completed

    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.

        :param position_id: The position_id.
        :return bool.
        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions  # Only open positions added here

    cpdef bint position_exists_for_order(self, OrderId order_id):
        """
        Return a value indicating whether there is a position associated with the given
        order_id.

        :param order_id: The order_id.
        :return bool.
        """
        Condition.not_none(order_id, "order_id")

        cdef PositionId position_id = self._index_order_position.get(order_id)
        if position_id is None:
            return False
        return position_id in self._index_positions

    cpdef bint position_indexed_for_order(self, OrderId order_id):
        """
        Return a value indicating whether there is a position_id indexed for the
        given order_id.

        :param order_id: The order_id.
        :return bool.
        """
        Condition.not_none(order_id, "order_id")

        return order_id in self._index_order_position

    cpdef bint is_position_open(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is open.

        :param position_id: The position_id.
        :return bool.
        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions_open

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is closed.

        :param position_id: The position_id.
        :return bool.
        """
        Condition.not_none(position_id, "position_id")

        return position_id in self._index_positions_closed

    cpdef int count_orders_total(self, StrategyId strategy_id=None):
        """
        Return the count of order_ids held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_order_ids(strategy_id))

    cpdef int count_orders_working(self, StrategyId strategy_id=None):
        """
        Return the count of working order_ids held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_order_working_ids(strategy_id))

    cpdef int count_orders_completed(self, StrategyId strategy_id=None):
        """
        Return the count of completed order_ids held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_order_completed_ids(strategy_id))

    cpdef int count_positions_total(self, StrategyId strategy_id=None):
        """
        Return the count of position_ids held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_position_ids(strategy_id))

    cpdef int count_positions_open(self, StrategyId strategy_id=None):
        """
        Return the count of open position_ids held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_position_open_ids(strategy_id))

    cpdef int count_positions_closed(self, StrategyId strategy_id=None):
        """
        Return the count of closed position_ids held by the execution database.

        :param strategy_id: The optional strategy_id query filter.
        :return int.
        """
        return len(self.get_position_closed_ids(strategy_id))
