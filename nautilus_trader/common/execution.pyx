# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from typing import Dict

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.commands cimport Command, AccountInquiry
from nautilus_trader.model.commands cimport SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from nautilus_trader.model.events cimport Event, OrderEvent, PositionEvent, AccountEvent
from nautilus_trader.model.events cimport OrderModified, OrderRejected, OrderCancelled, OrderCancelReject
from nautilus_trader.model.events cimport OrderFilled, OrderPartiallyFilled
from nautilus_trader.model.events cimport PositionOpened, PositionModified, PositionClosed
from nautilus_trader.model.identifiers cimport TraderId, StrategyId, OrderId, PositionId
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.trade.strategy cimport TradingStrategy


cdef class ExecutionDatabase:
    """
    The abstract base class for all execution databases.
    """

    def __init__(self,
                 TraderId trader_id,
                 Logger logger):
        """
        Initializes a new instance of the ExecutionDatabase class.

        :param trader_id: The trader identifier for the component.
        :param logger: The logger for the component.
        """
        self.trader_id = trader_id
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self._order_book = {}     # type: Dict[OrderId, Order]
        self._position_book = {}  # type: Dict[PositionId, Position]

    cpdef list get_strategy_ids(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef list get_order_ids(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef list get_position_ids(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void add_strategy(self, TradingStrategy strategy):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void remove_strategy(self, TradingStrategy strategy):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void update_order_status(self, Order order):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void add_position(self, Position position, StrategyId strategy_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef StrategyId get_strategy_id(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Order load_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Order get_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders_all(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders_active_all(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders_completed_all(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders(self, StrategyId strategy_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders_active(self, StrategyId strategy_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_orders_completed(self, StrategyId strategy_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint does_order_exist(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_order_active(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_order_complete(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef Position load_position(self, PositionId position_id):
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

    cpdef dict get_positions_all(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_positions_active_all(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_positions_closed_all(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_positions(self, StrategyId strategy_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_positions_active(self, StrategyId strategy_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef dict get_positions_closed(self, StrategyId strategy_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint does_position_exist(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_position_active(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_position_closed(self, PositionId position_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_position_for_order(self, OrderId order_id):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int positions_count(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int positions_active_count(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef int positions_closed_count(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void check_residuals(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void reset(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _reset(self):
        self._order_book = {}     # type: Dict[OrderId, Order]
        self._position_book = {}  # type: Dict[PositionId, Position]
        self._log.debug(f"Reset.")


cdef class InMemoryExecutionDatabase(ExecutionDatabase):
    """
    Provides an in-memory execution database.
    """

    def __init__(self,
                 TraderId trader_id,
                 Logger logger):
        """
        Initializes a new instance of the ExecutionEngine class.

        :param logger: The logger for the component.
        """
        super().__init__(trader_id, logger)

        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._index_order_strategy = {}   # type: Dict[OrderId, StrategyId]
        self._index_order_position = {}   # type: Dict[OrderId, PositionId]
        self._orders_active = {}          # type: Dict[StrategyId, Dict[OrderId, Order]]
        self._orders_completed = {}       # type: Dict[StrategyId, Dict[OrderId, Order]]
        self._positions_active = {}       # type: Dict[StrategyId, Dict[PositionId, Position]]
        self._positions_closed = {}       # type: Dict[StrategyId, Dict[PositionId, Position]]

    cpdef list get_strategy_ids(self):
        return []

    cpdef list get_order_ids(self):
        """
        Return a list of registered order identifiers.

        :return: List[OrderId].
        """
        return [self._order_book.keys()]

    cpdef list get_position_ids(self):
        """
        Return a list of position identifiers registered with the portfolio.

        :return: List[PositionId].
        """
        return [self._position_book.keys()]

    cpdef void add_strategy(self, TradingStrategy strategy):
        """
        Register the given strategy with the execution engine.

        :param strategy: The strategy to register.
        :raises ConditionFailed: If the strategy is already registered with the execution engine.
        """
        Condition.true(strategy.id not in self._orders_active, 'strategy not in orders_active')
        Condition.true(strategy.id not in  self._orders_completed, 'strategy not in orders_completed')

        self._orders_active[strategy.id] = {}     # type: Dict[OrderId, Order]
        self._orders_completed[strategy.id] = {}  # type: Dict[OrderId, Order]
        self._positions_active[strategy.id] = {}  # type: Dict[PositionId, Position]
        self._positions_closed[strategy.id] = {}  # type: Dict[PositionId, Position]

        self._log.debug(f"Added {strategy}.")

    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id):
        Condition.true(order.id not in self._order_book, 'order.id not in order_book')
        Condition.true(order.id not in self._index_order_strategy, 'order.id not in order_index')

        self._order_book[order.id] = order
        self._index_order_strategy[order.id] = strategy_id
        self._index_order_position[order.id] = position_id

        self._log.debug(f"Added order strategy_id={strategy_id.value}, position_id={position_id.value} order_id={order.id.value}")

    cpdef void remove_strategy(self, TradingStrategy strategy):
        """
        Deregister the given strategy with the execution client.

        :param strategy: The strategy to deregister.
        :raises ConditionFailed: If the strategy is not registered with the execution client.
        """
        Condition.true(strategy.id in self._orders_active, 'strategy in orders_active')
        Condition.true(strategy.id in self._orders_completed, 'strategy in orders_completed')

        del self._orders_active[strategy.id]
        del self._orders_completed[strategy.id]
        del self._positions_active[strategy.id]
        del self._positions_closed[strategy.id]

        self._log.debug(f"De-registered {strategy}.")

    cpdef void update_order_status(self, Order order):
        """
        
        :param order: 
        :return: 
        """
        cdef StrategyId strategy_id = self.get_strategy_id(order.id)

        # Active order
        if order.is_active:
            if order.id not in self._orders_active:
                self._orders_active[strategy_id][order.id] = order

        # Completed order
        if order.is_complete:
            if order.id not in self._orders_completed[strategy_id]:
                self._orders_completed[strategy_id][order.id] = order
                if order.id in self._orders_active[strategy_id]:
                    del self._orders_active[strategy_id][order.id]

    cpdef void add_position(self, Position position, StrategyId strategy_id):
        """
        Add the given position associated with the given strategy identifier.
        
        :param position: The position to add.
        :param strategy_id: The strategy identifier to associate.
        """
        self._position_book[position.id] = position
        self._positions_active[strategy_id][position.id] = position
        self._log.debug(f"Added {position} to active positions.")

    cpdef StrategyId get_strategy_id(self, OrderId order_id):
        """
        
        :param order_id: 
        :return StrategyId or None: 
        """
        return self._index_order_strategy.get(order_id)

    cpdef Order load_order(self, OrderId order_id):
        """
        Return the order with the given identifier (if found, else None).

        :return: Order or None.
        """
        return self.get_order(order_id)

    cpdef Order get_order(self, OrderId order_id):
        """
        Return the order with the given identifier (if found, else None).

        :return: Order or None.
        """
        return self._order_book.get(order_id, None)

    cpdef dict get_orders_all(self):
        """
        Return all orders in the execution clients order book.

        :return: Dict[OrderId, Order].
        """
        return self._order_book.copy()

    cpdef dict get_orders_active_all(self):
        """
        Return all active orders in the execution clients order book.
        
        :return: Dict[OrderId, Order].
        """
        return self._orders_active.copy()

    cpdef dict get_orders_completed_all(self):
        """
        Return all completed orders in the execution clients order book.
        
        :return: Dict[OrderId, Order].
        """
        return self._orders_completed.copy()

    cpdef dict get_orders(self, StrategyId strategy_id):
        """
        Return all orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        :raises ConditionFailed: If the strategy identifier is not registered with the execution client.
        """
        # Condition.true(strategy_id in self._orders_active, 'strategy_id in orders_active')
        # Condition.true(strategy_id in self._orders_completed, 'strategy_id in orders_completed')

        return {**self._orders_active[strategy_id], **self._orders_completed[strategy_id]}

    cpdef dict get_orders_active(self, StrategyId strategy_id):
        """
        Return all active orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        :raises ConditionFailed: If the strategy identifier is not registered with the execution client.
        """
        # Condition.true(strategy_id in self._orders_active, 'strategy_id in orders_active')

        return self._orders_active[strategy_id].copy()

    cpdef dict get_orders_completed(self, StrategyId strategy_id):
        """
        Return all completed orders associated with the strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the orders.
        :return: Dict[OrderId, Order].
        :raises ConditionFailed: If the strategy identifier is not registered with the execution client.
        """
        # Condition.true(strategy_id in self._orders_completed, 'strategy_id in orders_completed')

        return self._orders_completed[strategy_id].copy()

    cpdef bint does_order_exist(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier exists.
        
        :param order_id: The order identifier to check.
        :return: True if the order exists, else False.
        """
        return order_id in self._order_book

    cpdef bint is_order_active(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is active.
         
        :param order_id: The order identifier to check.
        :return: True if the order is found and active, else False.
        """
        return order_id in self._order_book and self._order_book[order_id].is_active

    cpdef bint is_order_complete(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is complete.
         
        :param order_id: The order identifier to check.
        :return: True if the order is found and complete, else False.
        """
        return order_id in self._order_book and self._order_book[order_id].is_complete

    cpdef Position load_position(self, PositionId position_id):
        """
        Return the position associated with the given position identifier (if found, else None).
        
        :param position_id: The position identifier.
        :return: Position or None.
        """
        return self.get_position(position_id)

    cpdef Position get_position(self, PositionId position_id):
        """
        Return the position associated with the given position identifier (if found, else None).
        
        :param position_id: The position identifier.
        :return: Position or None.
        """
        return self._position_book.get(position_id)

    cpdef Position get_position_for_order(self, OrderId order_id):
        """
        Return the position associated with the given order identifier (if found, else None).
        
        :param order_id: The order identifier for the position.
        :return: Position or None.
        """
        cdef PositionId position_id = self.get_position_id(order_id)

        if position_id is None:
            self._log.error("BOOM") # TODO
            return

        return self._position_book.get(position_id)

    cpdef PositionId get_position_id(self, OrderId order_id):
        """
        Return the position associated with the given order identifier (if found, else None).
        
        :param order_id: The order identifier associated with the position.
        :return: PositionId or None.
        """
        return self._index_order_position.get(order_id)

    cpdef dict get_positions_all(self):
        """
        Return a dictionary of all positions held by the portfolio.
        
        :return: Dict[PositionId, Position].
        """
        return self._position_book.copy()

    cpdef dict get_positions_active_all(self):
        """
        Return a dictionary of all active positions held by the portfolio.
        
        :return: Dict[PositionId, Position].
        """
        return self._positions_active.copy()

    cpdef dict get_positions_closed_all(self):
        """
        Return a dictionary of all closed positions held by the portfolio.
        
        :return: Dict[PositionId, Position].
        """
        return self._positions_closed.copy()

    cpdef dict get_positions(self, StrategyId strategy_id):
        """
        Return a list of all positions associated with the given strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the positions.
        :return: Dict[PositionId, Position].
        :raises ConditionFailed: If the strategy identifier is not registered with the portfolio.
        """
        Condition.is_in(strategy_id, self._positions_active, 'strategy_id', 'positions_active')
        Condition.is_in(strategy_id, self._positions_closed, 'strategy_id', 'positions_closed')

        return {**self._positions_active[strategy_id], **self._positions_closed[strategy_id]}  # type: Dict[PositionId, Position]

    cpdef dict get_positions_active(self, StrategyId strategy_id):
        """
        Return a list of all active positions associated with the given strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the positions.
        :return: Dict[PositionId, Position].
        :raises ConditionFailed: If the strategy identifier is not registered with the portfolio.
        """
        Condition.is_in(strategy_id, self._positions_active, 'strategy_id', 'positions_active')

        return self._positions_active[strategy_id].copy()

    cpdef dict get_positions_closed(self, StrategyId strategy_id):
        """
        Return a list of all active positions associated with the given strategy identifier.
        
        :param strategy_id: The strategy identifier associated with the positions.
        :return: Dict[PositionId, Position].
        :raises ConditionFailed: If the strategy identifier is not registered with the portfolio.
        """
        Condition.is_in(strategy_id, self._positions_closed, 'strategy_id', 'positions_closed')

        return self._positions_closed[strategy_id].copy()

    cpdef bint does_position_exist(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.

        :param position_id: The position identifier.
        :return: True if the position exists, else False.
        """
        return position_id in self._position_book

    cpdef bint is_position_active(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is entered (active).

        :param position_id: The position identifier.
        :return: True if the position exists and is exited, else False.
        """
        return position_id in self._position_book and not self._position_book[position_id].is_flat

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is exited (closed).

        :param position_id: The position identifier.
        :return: True if the position does not exist or is closed, else False.
        """
        return position_id in self._position_book and self._position_book[position_id].is_exited

    cpdef bint is_position_for_order(self, OrderId order_id):
        """
        Return a value indicating whether there is a position associated with the given
        order identifier.
        
        :param order_id: The order identifier.
        :return: True if an associated position exists, else False.
        """
        return order_id in self._index_order_position and self._index_order_position[order_id] in self._position_book

    cpdef int positions_count(self):
        """
        Return the total count of active and closed positions.
        
        :return: int.
        """
        cdef int positions_total_count = 0

        positions_total_count += self.positions_active_count()
        positions_total_count += self.positions_closed_count()

        return positions_total_count

    cpdef int positions_active_count(self):
        """
        Return the count of active positions held by the portfolio.
        
        :return: int.
        """
        cdef int active_positions = 0

        for positions_list in self._positions_active.values():
            active_positions += len(positions_list)

        return active_positions

    cpdef int positions_closed_count(self):
        """
        Return the count of closed positions held by the portfolio.
        
        :return: int.
        """
        cdef int closed_count = 0

        for positions_list in self._positions_closed.values():
            closed_count += len(positions_list)

        return closed_count

    cpdef void check_residuals(self):
        # Check for any residual active orders and log warnings if any are found
        for orders in self._orders_active.values():
            for order in orders.values():
                self._log.warning(f"Residual active {order}")

        for positions in self._positions_active.values():
            for position in positions.values():
                self._log.warning(f"Residual position {position}")

    cpdef void reset(self):
        # Reset the execution database by returning all stateful internal values to their initial value
        self._log.debug(f"Resetting...")
        self._index_order_strategy = {}   # type: Dict[OrderId, StrategyId]
        self._index_order_position = {}   # type: Dict[OrderId, PositionId]

        # Reset all active orders
        for strategy_id in self._orders_active.keys():
            self._orders_active[strategy_id] = {}     # type: Dict[OrderId, Order]

        # Reset all completed orders
        for strategy_id in self._orders_completed.keys():
            self._orders_completed[strategy_id] = {}  # type: Dict[OrderId, Order]

        # Reset all active positions
        for strategy_id in self._positions_active.keys():
            self._positions_active[strategy_id] = {}  # type: Dict[PositionId, Position]

        # Reset all closed positions
        for strategy_id in self._positions_closed.keys():
            self._positions_closed[strategy_id] = {}  # type: Dict[PositionId, Position]

        self._reset()


cdef class ExecutionEngine:
    """
    The base class for all execution engines.
    """

    def __init__(self,
                 ExecutionDatabase database,
                 Account account,
                 Portfolio portfolio,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the ExecutionEngine class.

        :param logger: The logger for the component.
        """
        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self.database = database
        self._exec_client = None
        self._account = account
        self._portfolio = portfolio

        self._registered_strategies = {}  # type: Dict[StrategyId, TradingStrategy]

        self.trader_id = database.trader_id
        self.command_count = 0
        self.event_count = 0

    cpdef list registered_strategies(self):
        """
        Return a list of strategy identifiers registered with the execution engine.
        
        :return: List[StrategyId]
        """
        return [self._registered_strategies.keys()]

# -- COMMANDS -----------------------------------------------------------------#
    cpdef void register_client(self, ExecutionClient exec_client):
        """
        Register the given execution client with the execution engine.
        :param exec_client: The execution client to register.
        """
        self._exec_client = exec_client

    cpdef void register_strategy(self, TradingStrategy strategy):
        """
        Register the given strategy with the execution engine.

        :param strategy: The strategy to register.
        :raises ConditionFailed: If the strategy is already registered with the execution engine.
        """
        Condition.true(strategy.id not in self._registered_strategies, 'strategy in registered_strategies')

        self._registered_strategies[strategy.id] = strategy
        self.database.add_strategy(strategy)
        strategy.register_execution_engine(self)

    cpdef void deregister_strategy(self, TradingStrategy strategy):
        """
        Deregister the given strategy with the execution engine.
        
        :param strategy: The strategy to deregister.
        :raises ConditionFailed: If the strategy is not registered with the execution client.
        """
        self.database.remove_strategy(strategy)

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

    cpdef void reset(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_strategy_flat(self, StrategyId strategy_id):
        """
        Return a value indicating whether the strategy given identifier is flat 
        (all associated positions FLAT).
        
        :param strategy_id: The strategy identifier.
        :return: True if the strategy is flat, else False.
        """
        return not self.database.get_positions_active(strategy_id)

    cpdef bint is_flat(self):
        """
        Return a value indicating whether the entire portfolio is flat.
        
        :return: True if the portfolio is flat, else False.
        """
        for strategy_id in self._registered_strategies:
            if not self.is_strategy_flat(strategy_id):
                return False  # Therefore the portfolio is not flat
        return True

    cdef void _execute_command(self, Command command):
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
        self.event_count += 1

        cdef Order order
        cdef StrategyId strategy_id

        # Order events
        if isinstance(event, OrderEvent):
            order = self.database.get_order(event.order_id)
            if order is not None:
                order.apply(event)
            else:
                self._log.error(f"Order for {event.order_id} not found.")
                return # Cannot apply event to any order

            self.database.update_order_status(order)

            strategy_id = self.database.get_strategy_id(event.order_id)
            if strategy_id is None:
                self._log.error(f"BOOM")

            if isinstance(event, (OrderFilled, OrderPartiallyFilled)):
                self._log.debug(f'{event}')
                if strategy_id is not None:
                    self._handle_order_fill(event, strategy_id)
            elif isinstance(event, OrderModified):
                self._log.debug(f"{event} price to {event.modified_price}")
            elif isinstance(event, OrderCancelled):
                self._log.debug(str(event))
            # Warning Events
            elif isinstance(event, (OrderRejected, OrderCancelReject)):
                self._log.debug(f'{event}')  # Also logged as warning by strategy
            else:
                self._log.debug(f'{event}')

            # Send event to strategy
            if strategy_id is not None:
                self._send_to_strategy(event, strategy_id)

        # Account event
        elif isinstance(event, AccountEvent):
            self._log.debug(f'{event}')
            if not self._account.initialized or self._account.id == event.account_id:
                self._account.apply(event)
                self._portfolio.handle_transaction(event)
            else:
                self._log.warning(f"{event} ignored (event not for this account).")

    cdef void _handle_order_fill(self, OrderEvent event, StrategyId strategy_id):
        """
        Handle the order fill event associated with the given strategy identifier.
        
        :param event: The event to handle.
        :raises ConditionFailed: If the strategy identifier is not registered with the portfolio.
        """
        assert isinstance(event, (OrderFilled, OrderPartiallyFilled))

        cdef PositionId position_id = self.database.get_position_id(event.order_id)
        if position_id is None:
            self._log.error(f"Cannot handle {event} (position id for {event.order_id} not found).")
            # Cannot proceed with event processing
            return

        cdef Position position = self.database.get_position_for_order(event.order_id)

        if position is None:
            # Position does not exist yet - create position
            position = Position(
                event.symbol,
                position_id,
                event.execution_time)
            position.apply(event)
            self.database.add_position(position, strategy_id)
            self._position_opened(position, strategy_id, event)
            return

        position.apply(event)
        if position.is_exited:
            self._position_closed(position, strategy_id, event)
        else:
            self._position_modified(position, strategy_id, event)

    cdef void _position_opened(self, Position position, StrategyId strategy_id, OrderEvent order_fill):
        cdef PositionOpened event = PositionOpened(
            position,
            strategy_id,
            order_fill,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._send_to_strategy(event, strategy_id)

    cdef void _position_modified(self, Position position, StrategyId strategy_id, OrderEvent order_fill):
        cdef PositionModified event = PositionModified(
            position,
            strategy_id,
            order_fill,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._send_to_strategy(event, strategy_id)

    cdef void _position_closed(self, Position position, StrategyId strategy_id, OrderEvent order_fill):
        cdef datetime time_now = self._clock.time_now()
        cdef PositionClosed event = PositionClosed(
            position,
            strategy_id,
            order_fill,
            self._guid_factory.generate(),
            time_now)

        self._portfolio.analyzer.add_return(time_now, position.return_realized)
        self._send_to_strategy(event, strategy_id)

    cdef void _send_to_strategy(self, Event event, StrategyId strategy_id):
        if strategy_id in self._registered_strategies:
            self._registered_strategies[strategy_id].handle_event(event)
        else:
            self._log.error(f"{strategy_id} not found in registered strategies.")

    cdef void _reset(self):
        """
        Reset the execution engine by clearing all stateful internal values.
        """
        self._registered_strategies = {}  # type: Dict[StrategyId, TradingStrategy]
        self.command_count = 0
        self.event_count = 0


cdef class ExecutionClient:
    """
    The base class for all execution clients.
    """

    def __init__(self,
                 ExecutionEngine engine,
                 Logger logger):
        """
        Initializes a new instance of the ExecutionClient class.

        :param engine: The execution engine to connect to the client.
        :param logger: The logger for the component.
        """
        self._engine = engine
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
