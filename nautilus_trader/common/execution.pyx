# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from typing import Set, Dict

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.commands cimport (
    Command,
    AccountInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    ModifyOrder,
    CancelOrder)
from nautilus_trader.model.events cimport (
    Event,
    OrderEvent,
    OrderFillEvent,
    PositionEvent,
    AccountStateEvent,
    OrderModified,
    OrderRejected,
    OrderCancelled,
    OrderCancelReject,
    PositionOpened,
    PositionModified,
    PositionClosed)
from nautilus_trader.model.identifiers cimport (
    AccountId,
    TraderId,
    StrategyId,
    OrderId,
    PositionId,
    PositionIdBroker)
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger, LoggerAdapter, CMD, EVT, SENT, RECV
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.performance cimport PerformanceAnalyzer
from nautilus_trader.trade.strategy cimport TradingStrategy


cdef class ExecutionDatabase:
    """
    The abstract base class for all execution databases.
    """

    def __init__(self, TraderId trader_id, Logger logger):
        """
        Initializes a new instance of the ExecutionDatabase class.

        :param trader_id: The trader_id for the component.
        :param logger: The logger for the component.
        """
        self.trader_id = trader_id
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self._cached_accounts = {}   # type: Dict[AccountId, Account]
        self._cached_orders = {}     # type: Dict[OrderId, Order]
        self._cached_positions = {}  # type: Dict[PositionId, Position]


# -- COMMANDS -------------------------------------------------------------------------------------"

    cpdef void add_account(self, Account account) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void update_account(self, Account event) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void update_order(self, Order order) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void update_position(self, Position position) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Account load_account(self, AccountId account_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Order load_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Position load_position(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void load_strategy(self, TradingStrategy strategy) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void delete_strategy(self, TradingStrategy strategy) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void check_residuals(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void reset(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void flush(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _reset(self) except *:
        self._cached_accounts.clear()
        self._cached_orders.clear()
        self._cached_positions.clear()
        self._log.debug(f"Reset.")


# -- QUERIES -------------------------------------------------------------------------------------"

    cpdef Account get_account(self, AccountId account_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set get_strategy_ids(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set get_order_ids(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set get_order_working_ids(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set get_order_completed_ids(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set get_position_ids(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set get_position_open_ids(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set get_position_closed_ids(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef StrategyId get_strategy_for_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef StrategyId get_strategy_for_position(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Order get_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders_working(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders_completed(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Position get_position(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Position get_position_for_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef PositionId get_position_id(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef PositionId get_position_id_for_broker_id(self, PositionIdBroker position_id_broker):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_positions(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_positions_open(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_positions_closed(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint order_exists(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_order_working(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_order_completed(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint position_exists(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint position_exists_for_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint position_indexed_for_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_position_open(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_position_closed(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int count_orders_total(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int count_orders_working(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int count_orders_completed(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int count_positions_total(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int count_positions_open(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int count_positions_closed(self, StrategyId strategy_id=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class InMemoryExecutionDatabase(ExecutionDatabase):
    """
    Provides an in-memory execution database.
    """

    def __init__(self, TraderId trader_id, Logger logger):
        """
        Initializes a new instance of the InMemoryExecutionDatabase class.

        :param trader_id: The trader_id for the component.
        :param logger: The logger for the component.
        """
        super().__init__(trader_id, logger)

        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._strategies = set()              # type: Set[StrategyId]
        self._index_order_position = {}       # type: Dict[OrderId, PositionId]
        self._index_order_strategy = {}       # type: Dict[OrderId, StrategyId]
        self._index_broker_position = {}      # type: Dict[PositionIdBroker, PositionId]
        self._index_position_strategy = {}    # type: Dict[PositionId, StrategyId]
        self._index_position_orders = {}      # type: Dict[PositionId, Set[OrderId]]
        self._index_strategy_orders = {}      # type: Dict[StrategyId, Set[OrderId]]
        self._index_strategy_positions = {}   # type: Dict[StrategyId, Set[PositionId]]
        self._index_orders = set()            # type: Set[OrderId]
        self._index_orders_working = set()    # type: Set[OrderId]
        self._index_orders_completed = set()  # type: Set[OrderId]
        self._index_positions = set()         # type: Set[PositionId]
        self._index_positions_open = set()    # type: Set[PositionId]
        self._index_positions_closed = set()  # type: Set[PositionId]


# -- COMMANDS -------------------------------------------------------------------------------------"

    cpdef void add_account(self, Account account) except *:
        """
        Add the given account to the execution database.

        :param account: The account to add.
        :raises ConditionFailed: If the account_id is already contained in the cached_accounts.
        """
        Condition.not_in(account.id, self._cached_accounts, 'account.id', 'cached_accounts')

        self._cached_accounts[account.id] = account

        self._log.debug(f"Added Account(id={account.id.value}).")

    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id) except *:
        """
        Add the given order to the execution database indexed with the given strategy and position
        identifiers.

        :param order: The order to add.
        :param strategy_id: The strategy_id to index for the order.
        :param position_id: The position_id to index for the order.
        :raises ConditionFailed: If the order_id is already contained in the cached_orders.
        :raises ConditionFailed: If the order_id is already contained in the index_orders.
        :raises ConditionFailed: If the order_id is already contained in the index_order_strategy.
        :raises ConditionFailed: If the order_id is already contained in the index_order_position.
        """
        Condition.not_in(order.id, self._cached_orders, 'order.id', 'cached_orders')
        Condition.not_in(order.id, self._index_orders, 'order.id', 'index_orders')
        Condition.not_in(order.id, self._index_order_strategy, 'order.id', 'index_order_strategy')
        Condition.not_in(order.id, self._index_order_position, 'order.id', 'index_order_position')

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
        :raises ConditionFailed: If the position.id is already contained in the cached_positions.
        :raises ConditionFailed: If the position.id_broker is already contained in the index_broker_position.
        :raises ConditionFailed: If the position.id is already contained in the index_positions.
        :raises ConditionFailed: If the position.id is already contained in the index_positions_open.
        """
        Condition.not_in(position.id, self._cached_positions, 'position.id', 'cached_positions')
        Condition.not_in(position.id_broker, self._index_broker_position, 'position.id_broker', 'index_broker_position')
        Condition.not_in(position.id, self._index_positions, 'position.id', 'index_positions')
        Condition.not_in(position.id, self._index_positions_open, 'position.id', 'index_positions_open')

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
        self._log.info(f'Saving {strategy.id} (in-memory database does nothing).')
        self._strategies.add(strategy.id)

    cpdef void update_order(self, Order order) except *:
        """
        Update the given order in the execution database.

        :param order: The order to update (from last event).
        """
        if order.is_working:
            self._index_orders_working.add(order.id)
            self._index_orders_completed.discard(order.id)
        elif order.is_completed:
            self._index_orders_completed.add(order.id)
            self._index_orders_working.discard(order.id)

    cpdef void update_position(self, Position position) except *:
        """
        Update the given position in the execution database.

        :param position: The position to update (from last event).
        """
        if position.is_closed:
            self._index_positions_closed.add(position.id)
            self._index_positions_open.discard(position.id)

    cpdef void load_strategy(self, TradingStrategy strategy) except *:
        """
        Load the state for the given strategy from the execution database.
        
        :param strategy: The strategy to load.
        """
        self._log.info(f'Loading {strategy.id} (in-memory database does nothing).')
        # Do nothing in memory

    cpdef Account load_account(self, AccountId account_id):
        """
        Load the account associated with the given account_id (if found).
        
        :param account_id: The account identifier to load.
        :return: Account or None.
        """
        return self._cached_accounts.get(account_id)

    cpdef Order load_order(self, OrderId order_id):
        """
        Load the order associated with the given order_id (if found).
        
        :param order_id: The order_id to load.
        :return: Order or None.
        """
        return self._cached_orders.get(order_id)

    cpdef Position load_position(self, PositionId position_id):
        """
        Load the position associated with the given position_id (if found).
        
        :param position_id: The position_id to load.
        :return: Position or None.
        """
        return self._cached_positions.get(position_id)

    cpdef void delete_strategy(self, TradingStrategy strategy) except *:
        """
        Delete the given strategy from the execution database.

        :param strategy: The strategy to deregister.
        :raises ConditionFailed: If the strategy is not contained in the strategies.
        """
        Condition.is_in(strategy.id, self._strategies, 'strategy.id', 'strategies')

        self._strategies.discard(strategy.id)

        if strategy.id in self._index_strategy_orders:
            del self._index_strategy_orders[strategy.id]

        if strategy.id in self._index_strategy_positions:
            del self._index_strategy_positions[strategy.id]

        self._log.debug(f"Deleted Strategy(id={strategy.id.value}).")

    cpdef void check_residuals(self) except *:
        for order_id, order in self.get_orders_working().items():
            self._log.warning(f"Residual working {order}.")

        for position_id, position in self.get_positions_open().items():
            self._log.warning(f"Residual open {position}.")

    cpdef void reset(self) except *:
        # Reset the execution database by clearing all stateful values
        self._log.debug(f"Resetting...")

        self._strategies.clear()
        self._index_order_position.clear()
        self._index_order_strategy.clear()
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
        self._log.info('Flushing database (in-memory database does nothing).')


# -- QUERIES --------------------------------------------------------------------------------------"

    cpdef Account get_account(self, AccountId account_id):
        """
        Return the account matching the given identifier (if found).

        :param account_id: The account_id.
        :return Account or None.
        """
        return self._cached_accounts.get(account_id)

    cpdef set get_strategy_ids(self):
        """
        Return a set of all strategy_ids.
         
        :return Set[StrategyId].
        """
        return  self._strategies.copy()

    cpdef set get_order_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all order_ids.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
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
        
        :param strategy_id: The strategy_id query filter (optional can be None).
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
        
        :param strategy_id: The strategy_id query filter (optional can be None).
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
        
        :param strategy_id: The strategy_id query filter (optional can be None).
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
        
        :param strategy_id: The strategy_id query filter (optional can be None).
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
        
        :param strategy_id: The strategy_id query filter (optional can be None).
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
        :return StrategyId or None: 
        """
        return self._index_order_strategy.get(order_id)

    cpdef StrategyId get_strategy_for_position(self, PositionId position_id):
        """
        Return the strategy_id associated with the given position_id (if found).
        
        :param position_id: The position_id associated with the strategy.
        :return StrategyId or None: 
        """
        return self._index_position_strategy.get(position_id)

    cpdef Order get_order(self, OrderId order_id):
        """
        Return the order matching the given identifier (if found).

        :return Order or None.
        """
        return self._cached_orders.get(order_id)

    cpdef dict get_orders(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all orders.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
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
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[OrderId, Order].
        """
        cdef set order_ids = self.get_order_working_ids(strategy_id)
        cdef dict orders_working

        try:
            orders_working = {order_id: self._cached_orders[order_id] for order_id in order_ids}
        except KeyError as ex:
            self._log.error("Cannot find order object in cached orders " + str(ex))

        return orders_working

    cpdef dict get_orders_completed(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all completed orders.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[OrderId, Order].
        """
        cdef set order_ids = self.get_order_completed_ids(strategy_id)
        cdef dict orders_completed

        try:
            orders_completed = {order_id: self._cached_orders[order_id] for order_id in order_ids}
        except KeyError as ex:
            self._log.error("Cannot find order object in cached orders " + str(ex))

        return orders_completed

    cpdef Position get_position(self, PositionId position_id):
        """
        Return the position associated with the given position_id (if found, else None).
        
        :param position_id: The position_id.
        :return Position or None.
        """
        return self._cached_positions.get(position_id)

    cpdef Position get_position_for_order(self, OrderId order_id):
        """
        Return the position associated with the given order_id (if found, else None).
        
        :param order_id: The order_id for the position.
        :return Position or None.
        """
        cdef PositionId position_id = self.get_position_id(order_id)
        if position_id is None:
            self._log.warning(f"Cannot get position for {order_id} (no matching position id found).")
            return None

        return self._cached_positions.get(position_id)

    cpdef PositionId get_position_id(self, OrderId order_id):
        """
        Return the position_id associated with the given order_id (if found, else None).
        
        :param order_id: The order_id associated with the position.
        :return PositionId or None.
        """
        cdef PositionId position_id = self._index_order_position.get(order_id)
        if position_id is None:
            self._log.warning(f"Cannot get position id for {order_id} (no matching position id found).")

        return position_id

    cpdef PositionId get_position_id_for_broker_id(self, PositionIdBroker position_id_broker):
        """
        Return the position_id associated with the given broker position_id (if found, else None).
        
        :param position_id_broker: The broker position_id.
        :return PositionId or None.
        """
        cdef PositionId position_id = self._index_broker_position.get(position_id_broker)
        if position_id is None:
            self._log.warning(f"Cannot get position id for {position_id_broker} (no matching position id found).")

        return position_id

    cpdef dict get_positions(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all positions.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_ids(strategy_id)
        cdef dict positions

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            # This should never happen
            self._log.error("Cannot find position object in cached positions " + str(ex))

        return positions

    cpdef dict get_positions_open(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all open positions.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_open_ids(strategy_id)
        cdef dict positions

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            # This should never happen
            self._log.error("Cannot find position object in cached positions " + str(ex))

        return positions

    cpdef dict get_positions_closed(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all closed positions.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_closed_ids(strategy_id)
        cdef dict positions

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            # This should never happen
            self._log.error("Cannot find position object in cached positions " + str(ex))

        return positions

    cpdef bint order_exists(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier exists.
        
        :param order_id: The order_id to check.
        :return bool.
        """
        return order_id in self._index_orders

    cpdef bint is_order_working(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is working.

        :param order_id: The order_id to check.
        :return bool.
        """
        return order_id in self._index_orders_working

    cpdef bint is_order_completed(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is completed.

        :param order_id: The order_id to check.
        :return bool.
        """
        return order_id in self._index_orders_completed

    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.
        
        :param position_id: The position_id.
        :return bool.
        """
        return position_id in self._index_positions  # Only open positions added here

    cpdef bint position_exists_for_order(self, OrderId order_id):
        """
        Return a value indicating whether there is a position associated with the given
        order_id.
        
        :param order_id: The order_id.
        :return bool.
        """
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
        return order_id in self._index_order_position

    cpdef bint is_position_open(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is open.

        :param position_id: The position_id.
        :return bool.
        """
        return position_id in self._index_positions_open

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is closed.

        :param position_id: The position_id.
        :return bool.
        """
        return position_id in self._index_positions_closed

    cpdef int count_orders_total(self, StrategyId strategy_id=None):
        """
        Return the count of order_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        return len(self.get_order_ids(strategy_id))

    cpdef int count_orders_working(self, StrategyId strategy_id=None):
        """
        Return the count of working order_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        return len(self.get_order_working_ids(strategy_id))

    cpdef int count_orders_completed(self, StrategyId strategy_id=None):
        """
        Return the count of completed order_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        return len(self.get_order_completed_ids(strategy_id))

    cpdef int count_positions_total(self, StrategyId strategy_id=None):
        """
        Return the count of position_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        return len(self.get_position_ids(strategy_id))

    cpdef int count_positions_open(self, StrategyId strategy_id=None):
        """
        Return the count of open position_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        return len(self.get_position_open_ids(strategy_id))

    cpdef int count_positions_closed(self, StrategyId strategy_id=None):
        """
        Return the count of closed position_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        return len(self.get_position_closed_ids(strategy_id))


cdef class ExecutionEngine:
    """
    Provides a generic execution engine.
    """

    def __init__(self,
                 TraderId trader_id,
                 AccountId account_id,
                 ExecutionDatabase database,
                 Portfolio portfolio,
                 PerformanceAnalyzer analyzer,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the ExecutionEngine class.

        :param trader_id: The trader identifier for the engine.
        :param account_id: The account identifier for the engine.
        :param database: The execution database for the engine.
        :param portfolio: The portfolio for the engine.
        :param analyzer: The performance analyzer for the engine.
        :param clock: The clock for the engine.
        :param guid_factory: The guid_factory for the engine.
        :param logger: The logger for the engine.
        :raises ConditionFailed: If the trader_id is not equal to the database.trader_id.
        """
        Condition.equal(trader_id, database.trader_id)

        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self._registered_strategies = {}  # type: Dict[StrategyId, TradingStrategy]
        self._exec_client = None

        self.trader_id = trader_id
        self.account_id = account_id
        self.database = database
        self.account = self.database.get_account(self.account_id)
        self.portfolio = portfolio
        self.analyzer = analyzer

        self.command_count = 0
        self.event_count = 0


#-- COMMANDS --------------------------------------------------------------------------------------#

    cpdef void register_client(self, ExecutionClient exec_client):
        """
        Register the given execution client with the execution engine.
        :param exec_client: The execution client to register.
        """
        self._exec_client = exec_client
        self._log.info("Registered execution client.")

    cpdef void register_strategy(self, TradingStrategy strategy):
        """
        Register the given strategy with the execution engine.

        :param strategy: The strategy to register.
        :raises ConditionFailed: If the strategy is already registered with the execution engine.
        """
        Condition.not_in(strategy.id, self._registered_strategies, 'strategy.id', 'registered_strategies')

        self._registered_strategies[strategy.id] = strategy
        strategy.register_execution_engine(self)
        self._log.info(f"Registered strategy {strategy}.")

    cpdef void deregister_strategy(self, TradingStrategy strategy):
        """
        Deregister the given strategy with the execution engine.
        
        :param strategy: The strategy to deregister.
        :raises ConditionFailed: If the strategy is not registered with the execution client.
        """
        Condition.is_in(strategy.id, self._registered_strategies, 'strategy.id', 'registered_strategies')

        del self._registered_strategies[strategy.id]
        self._log.info(f"De-registered strategy {strategy}.")

    cpdef void execute_command(self, Command command):
        """
        Execute the given command.
        
        :param command: The command to execute.
        """
        self._execute_command(command)

    cpdef void handle_event(self, Event event):
        """
        Handle the given command.
        
        :param event: The event to handle.
        """
        self._handle_event(event)

    cpdef void check_residuals(self):
        """
        Check for residual working orders or open positions.
        """
        self.database.check_residuals()

    cpdef void reset(self):
        """
        Reset the execution engine by clearing all stateful values.
        """
        self.database.reset()


#-- QUERIES ---------------------------------------------------------------------------------------"

    cpdef list registered_strategies(self):
        """
        Return a list of strategy_ids registered with the execution engine.
        
        :return List[StrategyId].
        """
        return list(self._registered_strategies.keys())

    cpdef bint is_strategy_flat(self, StrategyId strategy_id):
        """
        Return a value indicating whether the strategy given identifier is flat 
        (all associated positions FLAT).
        
        :param strategy_id: The strategy_id.
        :return bool.
        """
        return self.database.count_positions_open(strategy_id) == 0

    cpdef bint is_flat(self):
        """
        Return a value indicating whether the execution engine is flat.
        
        :return bool.
        """
        return self.database.count_positions_open() == 0


#--------------------------------------------------------------------------------------------------"

    cdef void _execute_command(self, Command command):
        self._log.debug(f'{RECV}{CMD} {command}.')
        self.command_count += 1

        if isinstance(command, AccountInquiry):
            self._exec_client.account_inquiry(command)
        elif isinstance(command, SubmitOrder):
            self.database.add_order(command.order, command.strategy_id, command.position_id)
            self._exec_client.submit_order(command)
        elif isinstance(command, SubmitAtomicOrder):
            self.database.add_order(command.atomic_order.entry, command.strategy_id, command.position_id)
            self.database.add_order(command.atomic_order.stop_loss, command.strategy_id, command.position_id)
            if command.atomic_order.has_take_profit:
                self.database.add_order(command.atomic_order.take_profit, command.strategy_id, command.position_id)
            self._exec_client.submit_atomic_order(command)
        elif isinstance(command, ModifyOrder):
            self._exec_client.modify_order(command)
        elif isinstance(command, CancelOrder):
            self._exec_client.cancel_order(command)

    cdef void _handle_event(self, Event event):
        self._log.debug(f'{RECV}{EVT} {event}.')
        self.event_count += 1

        if isinstance(event, OrderEvent):
            if isinstance(event, OrderCancelReject):
                self._handle_order_cancel_reject(event)
            else:
                self._handle_order_event(event)
        elif isinstance(event, PositionEvent):
            self._handle_position_event(event)
        elif isinstance(event, AccountStateEvent):
            self._handle_account_event(event)

    cdef void _handle_order_cancel_reject(self, OrderCancelReject event):
        cdef StrategyId strategy_id = self.database.get_strategy_for_order(event.order_id)
        if strategy_id is None:
            self._log.error(f"Cannot process event {event} ({strategy_id} not found).")
            return  # Cannot process event further

        self._send_to_strategy(event, strategy_id)

    cdef void _handle_order_event(self, OrderEvent event):
        cdef Order order = self.database.get_order(event.order_id)
        if order is None:
            self._log.warning(f"Cannot apply event {event} to any order ({event.order_id} not found in cache).")
            return  # Cannot process event further
        else:
            order.apply(event)
            self.database.update_order(order)

        if isinstance(event, OrderFillEvent):
            self._handle_order_fill(event)
            return

        self._send_to_strategy(event, self.database.get_strategy_for_order(event.order_id))

    cdef void _handle_order_fill(self, OrderFillEvent event):
        cdef PositionId position_id = self.database.get_position_id(event.order_id)
        if position_id is None:
            position_id = self.database.get_position_id_for_broker_id(event.position_id_broker)

        if position_id is None:
            self._log.error(f"Cannot process event {event} (position_id for {event.order_id} not found).")
            return  # Cannot process event further

        cdef Position position = self.database.get_position_for_order(event.order_id)  # Could still be None here
        cdef StrategyId strategy_id = self.database.get_strategy_for_position(position_id)

        if strategy_id is None:
            self._log.error(f"Cannot process event {event} (strategy_id for {position_id} not found).")
            return  # Cannot process event further

        if position is None:
            # Position does not exist - create new position
            position = Position(position_id, event)
            self.database.add_position(position, strategy_id)
            self._position_opened(position, strategy_id, event)
        else:
            # Position exists - apply event
            position.apply(event)
            self.database.update_position(position)
            if position.is_closed:
                self._position_closed(position, strategy_id, event)
            else:
                self._position_modified(position, strategy_id, event)

    cdef void _handle_position_event(self, PositionEvent event):
        if isinstance(event, PositionOpened):
            self.portfolio.update(event)
        if isinstance(event, PositionClosed):
            self.portfolio.update(event)
            self.analyzer.add_return(event.timestamp, event.position.realized_return)

        self._send_to_strategy(event, event.strategy_id)

    cdef void _handle_account_event(self, AccountStateEvent event):
        cdef Account account = self.database.get_account(event.account_id)

        if account is None:
            account = Account(event)
            if self.account_id.equals(account.id):
                self.account = account
            self.database.add_account(self.account)
            self.analyzer.initialize_account_data(event)
        elif account.id == event.account_id:
            account.apply(event)
            self.database.update_account(account)
            self.analyzer.handle_transaction(event)
        else:
            self._log.warning(f"Cannot process event {event} "
                              f"(event account_id {event.account_id} does not match this account {account.id}).")

    cdef void _position_opened(self, Position position, StrategyId strategy_id, OrderEvent event):
        cdef PositionOpened position_opened = PositionOpened(
            position,
            strategy_id,
            event,
            self._guid_factory.generate(),
            event.timestamp)

        self._send_to_strategy(event, strategy_id)
        self.handle_event(position_opened)

    cdef void _position_modified(self, Position position, StrategyId strategy_id, OrderEvent event):
        cdef PositionModified position_modified = PositionModified(
            position,
            strategy_id,
            event,
            self._guid_factory.generate(),
            event.timestamp)

        self._send_to_strategy(event, strategy_id)
        self.handle_event(position_modified)

    cdef void _position_closed(self, Position position, StrategyId strategy_id, OrderEvent event):
        cdef datetime time_now = self._clock.time_now()
        cdef PositionClosed position_closed = PositionClosed(
            position,
            strategy_id,
            event,
            self._guid_factory.generate(),
            event.timestamp)

        self._send_to_strategy(event, strategy_id)
        self.handle_event(position_closed)

    cdef void _send_to_strategy(self, Event event, StrategyId strategy_id):
        if strategy_id is None:
            self._log.error(f"Cannot send event {event} to strategy ({strategy_id} not found).")
            return  # Cannot send to strategy

        cdef TradingStrategy strategy = self._registered_strategies.get(strategy_id)
        if strategy_id is None:
            self._log.error(f"Cannot send event {event} to strategy ({strategy_id} not registered).")
            return

        strategy.handle_event(event)

    cdef void _reset(self):
        """
        Reset the execution engine by clearing stateful values.
        """
        self._registered_strategies = {}  # type: Dict[StrategyId, TradingStrategy]
        self.command_count = 0
        self.event_count = 0


cdef class ExecutionClient:
    """
    The base class for all execution clients.
    """

    def __init__(self, ExecutionEngine exec_engine, Logger logger):
        """
        Initializes a new instance of the ExecutionClient class.

        :param exec_engine: The execution engine to connect to the client.
        :param logger: The logger for the component.
        """
        self._exec_engine = exec_engine
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self.command_count = 0
        self.event_count = 0

        self._log.info(f"Initialized.")


# -- ABSTRACT METHODS ---------------------------------------------------------#

    cpdef void connect(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void dispose(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void account_inquiry(self, AccountInquiry command):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void submit_order(self, SubmitOrder command):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void submit_atomic_order(self, SubmitAtomicOrder command):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void modify_order(self, ModifyOrder command):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void cancel_order(self, CancelOrder command):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void reset(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")
# -----------------------------------------------------------------------------#

    cdef void _reset(self):
        self.command_count = 0
        self.event_count = 0
