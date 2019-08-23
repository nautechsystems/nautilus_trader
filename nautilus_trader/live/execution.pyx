# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import queue
import threading

from redis import Redis
from zmq import Context

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport MessageType, Message, Command, Event, Response
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.identifiers cimport TraderId, StrategyId, OrderId, PositionId
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
    OrderInitialized,
    PositionEvent,
    AccountEvent,
    OrderModified,
    OrderRejected,
    OrderCancelled,
    OrderCancelReject,
    PositionOpened,
    PositionModified,
    PositionClosed)
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger
from nautilus_trader.common.execution cimport ExecutionDatabase, ExecutionEngine, ExecutionClient
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.network.workers import RequestWorker, SubscriberWorker
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer
from nautilus_trader.serialization.serializers cimport (
    MsgPackCommandSerializer,
    MsgPackResponseSerializer
)
from nautilus_trader.live.logger cimport LiveLogger
from nautilus_trader.serialization.serializers cimport EventSerializer, MsgPackEventSerializer
from nautilus_trader.trade.strategy cimport TradingStrategy

cdef str UTF8 = 'utf-8'

cdef str TRADER = 'Trader'
cdef str INDEX = 'Index'
cdef str CONFIG = 'Config'
cdef str ACCOUNTS = 'Accounts'
cdef str ORDERS = 'Orders'
cdef str POSITIONS = 'Positions'
cdef str STRATEGIES = 'Strategies'
cdef str ORDER_POSITION = 'OrderPosition'
cdef str ORDER_STRATEGY = 'OrderStrategy'
cdef str POSITION_ORDERS = 'PositionOrders'
cdef str POSITION_STRATEGY = 'PositionStrategy'
cdef str STRATEGY_ORDERS = 'StrategyOrders'
cdef str STRATEGY_POSITIONS = 'StrategyPositions'
cdef str WORKING = 'Working'
cdef str COMPLETED = 'Completed'
cdef str OPEN = 'Open'
cdef str CLOSED = 'Closed'


cdef class RedisExecutionDatabase(ExecutionDatabase):
    """
    Provides an execution database utilizing Redis.
    """

    def __init__(self,
                 TraderId trader_id,
                 str host,
                 int port,
                 CommandSerializer command_serializer,
                 EventSerializer event_serializer,
                 Logger logger):
        """
        Initializes a new instance of the RedisExecutionEngine class.

        :param trader_id: The trader identifier.
        :param port: The redis host for the database connection.
        :param port: The redis port for the database connection.
        :param command_serializer: The command serializer for database transactions.
        :param event_serializer: The event serializer for database transactions.
        :raises ConditionFailed: If the host is not a valid string.
        :raises ConditionFailed: If the port is not in range [0, 65535].
        """
        Condition.valid_string(host, 'host')
        Condition.in_range(port, 'port', 0, 65535)

        super().__init__(trader_id, logger)

        # Database keys
        self.key_trader                   = f'{TRADER}-{trader_id.value}'
        self.key_accounts                 = f'{self.key_trader}:{ACCOUNTS}:'
        self.key_orders                   = f'{self.key_trader}:{ORDERS}:'
        self.key_positions                = f'{self.key_trader}:{POSITIONS}:'
        self.key_strategies               = f'{self.key_trader}:{STRATEGIES}:'
        self.key_index_order_position     = f'{self.key_trader}:{INDEX}:{ORDER_POSITION}'      # HASH
        self.key_index_order_strategy     = f'{self.key_trader}:{INDEX}:{ORDER_STRATEGY}'      # HASH
        self.key_index_position_strategy  = f'{self.key_trader}:{INDEX}:{POSITION_STRATEGY}'   # HASH
        self.key_index_position_orders    = f'{self.key_trader}:{INDEX}:{POSITION_ORDERS}:'    # SET
        self.key_index_strategy_orders    = f'{self.key_trader}:{INDEX}:{STRATEGY_ORDERS}:'    # SET
        self.key_index_strategy_positions = f'{self.key_trader}:{INDEX}:{STRATEGY_POSITIONS}:' # SET
        self.key_index_orders_working     = f'{self.key_trader}:{INDEX}:{ORDERS}:{WORKING}'    # SET
        self.key_index_orders_completed   = f'{self.key_trader}:{INDEX}:{ORDERS}:{COMPLETED}'  # SET
        self.key_index_positions_open     = f'{self.key_trader}:{INDEX}:{POSITIONS}:{OPEN}'    # SET
        self.key_index_positions_closed   = f'{self.key_trader}:{INDEX}:{POSITIONS}:{CLOSED}'  # SET

        # Serializers
        self._command_serializer = command_serializer
        self._event_serializer = event_serializer

        # Redis client
        self._redis = Redis(host=host, port=port, db=0)

        self.load_cache = True
        self.check_integrity = True

        # Load cache
        if self.load_cache:
            self.load_orders_cache()
            self.load_positions_cache()


# -- COMMANDS -------------------------------------------------------------------------------------"

    cpdef void load_orders_cache(self):
        """
        Clear the current order cache and load orders from the database.
        """
        self._cached_orders.clear()

        cdef bytes key_bytes
        cdef bytes event_bytes
        cdef list events
        cdef Order order
        cdef OrderEvent initial

        cdef list order_keys = self._redis.keys(f'{self.key_orders}*')

        for key_bytes in order_keys:
            key = key_bytes.decode(UTF8)
            events = self._redis.lrange(name=key, start=0, end=-1)
            initial = self._event_serializer.deserialize(events.pop(0))
            assert isinstance(initial, OrderInitialized)
            order = Order.create(event=initial)

            for event_bytes in events:
                order.apply(self._event_serializer.deserialize(event_bytes))

            self._cached_orders[order.id] = order

    cpdef void load_positions_cache(self):
        """
        Clear the current order cache and load orders from the database.
        """
        self._cached_positions.clear()

        cdef str key
        cdef PositionId position_id
        cdef Position position
        cdef list events
        cdef OrderFillEvent event

        cdef list position_keys = [key.decode(UTF8) for key in self._redis.keys(f'{self.key_positions}*')]

        for key in position_keys:
            position_id = key.rsplit(':', maxsplit=1)[1]
            events = [self._event_serializer.deserialize(event) for event in self._redis.lrange(name=key, start=0, end=-1)]
            initial = events.pop(0)
            assert isinstance(initial, OrderFillEvent)
            position = Position(position_id=position_id, event=initial)

            for event in events:
                position.apply(event)

            self._cached_positions[position.id] = position

    cpdef void reset(self):
        """
        Reset the execution database by clearing the cache.
        """
        self._reset()

    cpdef void add_strategy(self, TradingStrategy strategy):
        """
        Add the given strategy to the execution database.

        :param strategy: The strategy to add.
        """
        pipe = self._redis.pipeline()
        pipe.hset(self.key_strategies + f'{strategy.id.value}:{CONFIG}', 'some_value', 1)
        pipe.execute()

        self._log.debug(f"Added strategy (id={strategy.id.value}).")

    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id):
        """
        Add the given order to the execution database.

        :param order: The order to add.
        :param strategy_id: The strategy identifier to associate with the order.
        :param position_id: The position identifier to associate with the order.
        """
        Condition.true(order.id not in self._cached_orders, 'order.id not in order_book')

        self._cached_orders[order.id] = order

        cdef str key_order =  self.key_orders + order.id.value

        if self.check_integrity:
            if self._redis.exists(key_order):
                self._log.warning(f'The {key_order} already exists.')

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(key_order, self._event_serializer.serialize(order.last_event))
        pipe.hset(name=self.key_index_order_position, key=order.id.value, value=position_id.value)
        pipe.hset(name=self.key_index_order_strategy, key=order.id.value, value=strategy_id.value)
        pipe.hset(name=self.key_index_position_strategy, key=position_id.value, value=strategy_id.value)
        pipe.sadd(self.key_index_position_orders + position_id.value, order.id.value)
        pipe.sadd(self.key_index_strategy_orders + strategy_id.value, order.id.value)
        pipe.sadd(self.key_index_strategy_positions + strategy_id.value, position_id.value)
        pipe.execute()

        self._log.debug(f"Added order (id={order.id.value}, strategy_id={strategy_id.value}, position_id={position_id.value}).")

    cpdef void add_position(self, Position position, StrategyId strategy_id):
        """
        Add the given position associated with the given strategy identifier.

        :param position: The position to add.
        :param strategy_id: The strategy identifier to associate with the position.
        """
        Condition.true(position.id not in self._cached_positions, 'position.id not in self._cached_positions')

        self._cached_positions[position.id] = position

        cdef str key_position = self.key_positions + position.id.value

        if self.check_integrity:
            if self._redis.exists(key_position):
                self._log.warning(f'The {key_position} already exists.')

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(key_position, self._event_serializer.serialize(position.last_event))
        pipe.sadd(self.key_index_positions_open, position.id.value)
        pipe.execute()

        self._log.debug(f"Added position (id={position.id.value}) .")

    cpdef void add_order_event(self, Order order, OrderEvent event):
        """
        Add the last event of the given order to the execution database.

        :param order: The order for the event to add (last event).
        :param event: The event to add.
        """
        Condition.equal(order.id, event.order_id)

        cdef str key_order = self.key_orders + order.id.value

        if self.check_integrity:
            if not self._redis.exists(key_order):
                self._log.warning(f'The {key_order} did not already exist.')

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(key_order, self._event_serializer.serialize(event))
        if order.is_working:
            pipe.sadd(self.key_index_orders_working, order.id.value)
            pipe.srem(self.key_index_orders_completed, order.id.value)
        elif order.is_completed:
            pipe.sadd(self.key_index_orders_completed, order.id.value)
            pipe.srem(self.key_index_orders_working, order.id.value)
        pipe.execute()

    cpdef void add_position_event(self, Position position, OrderFillEvent event):
        """
        Add the given position event to the execution database.

        :param position: The position for the event to add (last event).
        :param event: The event to add.
        """

        cdef str key_position = self.key_positions + position.id.value

        if self.check_integrity:
            if not self._redis.exists(key_position):
                self._log.warning(f'The {key_position} did not already exist.')

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(key_position, self._event_serializer.serialize(event))
        if position.is_closed:
            pipe.sadd(self.key_index_positions_closed, position.id.value)
            pipe.srem(self.key_index_positions_open, position.id.value)
        pipe.execute()

    cpdef void add_account_event(self, AccountEvent event):
        """
        Add the given account event to the execution database.

        :param event: The account event to add.
        """
        cdef str key_account = self.key_accounts + event.account_id.value

        self._redis.rpush(key_account, self._event_serializer.serialize(event))

#
#     cpdef void delete_strategy(self, TradingStrategy strategy):
#         """
#         Deregister the given strategy with the execution client.
#
#         :param strategy: The strategy to deregister.
#         :raises ConditionFailed: If the strategy is not registered with the execution client.
#         """
#         Condition.true(strategy.id in self._strategies, 'strategy in strategies')
#         Condition.true(strategy.id in self._orders_working, 'strategy in orders_active')
#         Condition.true(strategy.id in self._orders_completed, 'strategy in orders_completed')
#
#         self._strategies.remove(strategy.id)
#         del self._orders_working[strategy.id]
#         del self._orders_completed[strategy.id]
#         del self._positions_open[strategy.id]
#         del self._positions_closed[strategy.id]
#
#         self._log.debug(f"Deleted strategy (id={strategy.id.value}).")
#
    cpdef void check_residuals(self):
        # Check for any residual active orders and log warnings if any are found
        for working_orders in self._redis.smembers(self.key_index_orders_working):
            for order_id in working_orders:
                self._log.warning(f"Residual working order {order_id}")

        for positions_open in self._redis.smembers(self.key_index_positions_open):
            for position_id in positions_open:
                self._log.warning(f"Residual open position {position_id}")
#
#     cpdef void reset(self):
#         # Reset the execution database by returning all stateful internal values to their initial value
#         self._log.debug(f"Resetting...")
#         self._index_order_strategy = {}   # type: Dict[OrderId, StrategyId]
#         self._index_order_position = {}   # type: Dict[OrderId, PositionId]
#
#         # Reset all active orders
#         for strategy_id in self._orders_working.keys():
#             self._orders_working[strategy_id] = {}     # type: Dict[OrderId, Order]
#
#         # Reset all completed orders
#         for strategy_id in self._orders_completed.keys():
#             self._orders_completed[strategy_id] = {}  # type: Dict[OrderId, Order]
#
#         # Reset all active positions
#         for strategy_id in self._positions_open.keys():
#             self._positions_open[strategy_id] = {}  # type: Dict[PositionId, Position]
#
#         # Reset all closed positions
#         for strategy_id in self._positions_closed.keys():
#             self._positions_closed[strategy_id] = {}  # type: Dict[PositionId, Position]
#
#         self._reset()
#

# -- QUERIES --------------------------------------------------------------------------------------"

    cpdef list get_strategy_ids(self):
        """
        Return a list of all registered strategy identifiers.

        :return: List[StrategyId].
        """
        return  self._redis.keys(pattern=f'{self.key_strategies}*')

    cpdef list get_order_ids(self):
        """
        Return a list of all registered order identifiers.

        :return: List[OrderId].
        """
        return self._redis.keys(pattern=f'{self.key_orders}*')

    cpdef list get_position_ids(self):
        """
        Return a list of the cached position identifiers.

        :return: List[PositionId].
        """
        return self._redis.keys(pattern=f'{self.key_positions}*')

    cpdef StrategyId get_strategy_id(self, OrderId order_id):
        """
        Return the strategy identifier associated with the given order identifier.

        :param order_id: The order identifier associated with the strategy.
        :return StrategyId or None:
        """
        return self._redis.hget(name=self.key_index_order_strategy, key=order_id.value)

    cpdef Order get_order(self, OrderId order_id):
        """
        Return the order matching the given identifier (if found).

        :return: Order or None.
        """
        cdef Order order = self._cached_orders.get(order_id)
        if order is None:
            self._log_cannot_find_order(order_id)
        return order

    cpdef dict get_orders_all(self):
        """
        Return all orders in the execution engines order book.

        :return: Dict[OrderId, Order].
        """
        return self._cached_orders.copy()

    cpdef dict get_orders_working_all(self):
        """
        Return all active orders in the execution engines order book.

        :return: Dict[OrderId, Order].
        """
        cdef set working_order_ids = self._redis.smembers(self.key_index_orders_working)

        cdef dict orders_working = {}
        cdef Order order
        for order_id in working_order_ids:
            order = self._cached_orders.get(order_id)
            if order is None:
                self._log_cannot_find_order(order_id)
            orders_working[order_id] = order

        return orders_working

    cpdef dict get_orders_completed_all(self):
        """
        Return all completed orders in the execution engines order book.

        :return: Dict[OrderId, Order].
        """
        cdef set completed_order_ids = self._redis.smembers(self.key_index_orders_completed)

        cdef dict orders_completed = {}
        cdef Order order
        for order_id in orders_completed:
            order = self._cached_orders.get(order_id)
            if order is None:
                self._log_cannot_find_order(order_id)
            orders_completed[order_id] = order

        return orders_completed

    # cpdef dict get_orders(self, StrategyId strategy_id):
    #     """
    #     Return all orders associated with the strategy identifier.
    #
    #     :param strategy_id: The strategy identifier associated with the orders.
    #     :return: Dict[OrderId, Order].
    #     :raises ConditionFailed: If the strategy identifier is not registered with the execution client.
    #     """
    #     # Condition.true(strategy_id in self._orders_active, 'strategy_id in orders_active')
    #     # Condition.true(strategy_id in self._orders_completed, 'strategy_id in orders_completed')
    #
    #     return {**self._orders_working[strategy_id], **self._orders_completed[strategy_id]}
#
#     cpdef dict get_orders_working(self, StrategyId strategy_id):
#         """
#         Return all active orders associated with the strategy identifier.
#
#         :param strategy_id: The strategy identifier associated with the orders.
#         :return: Dict[OrderId, Order].
#         :raises ConditionFailed: If the strategy identifier is not registered with the execution client.
#         """
#         # Condition.true(strategy_id in self._orders_active, 'strategy_id in orders_active')
#
#         return self._orders_working[strategy_id].copy()
#
#     cpdef dict get_orders_completed(self, StrategyId strategy_id):
#         """
#         Return all completed orders associated with the strategy identifier.
#
#         :param strategy_id: The strategy identifier associated with the orders.
#         :return: Dict[OrderId, Order].
#         :raises ConditionFailed: If the strategy identifier is not registered with the execution client.
#         """
#         # Condition.true(strategy_id in self._orders_completed, 'strategy_id in orders_completed')
#
#         return self._orders_completed[strategy_id].copy()
#
#     cpdef bint order_exists(self, OrderId order_id):
#         """
#         Return a value indicating whether an order with the given identifier exists.
#
#         :param order_id: The order identifier to check.
#         :return: True if the order exists, else False.
#         """
#         return order_id in self._cached_orders
#
#     cpdef bint is_order_working(self, OrderId order_id):
#         """
#         Return a value indicating whether an order with the given identifier is active.
#
#         :param order_id: The order identifier to check.
#         :return: True if the order is found and active, else False.
#         """
#         return order_id in self._cached_orders and self._cached_orders[order_id].is_working
#
#     cpdef bint is_order_complete(self, OrderId order_id):
#         """
#         Return a value indicating whether an order with the given identifier is complete.
#
#         :param order_id: The order identifier to check.
#         :return: True if the order is found and complete, else False.
#         """
#         return order_id in self._cached_orders and self._cached_orders[order_id].is_completed
#
#     cpdef Position get_position(self, PositionId position_id):
#         """
#         Return the position associated with the given position identifier (if found, else None).
#
#         :param position_id: The position identifier.
#         :return: Position or None.
#         """
#         cdef Position position = self._cached_positions.get(position_id)
#         if position is None:
#             self._log_cannot_find_position(position_id)
#         return position
#
#     cpdef Position get_position_for_order(self, OrderId order_id):
#         """
#         Return the position associated with the given order identifier (if found, else None).
#
#         :param order_id: The order identifier for the position.
#         :return: Position or None.
#         """
#         cdef PositionId position_id = self.get_position_id(order_id)
#         if position_id is None:
#             self._log.error(f"Cannot get position for {order_id} (no matching position id found).")
#             return None
#
#         return self._cached_positions.get(position_id)
#
#     cpdef PositionId get_position_id(self, OrderId order_id):
#         """
#         Return the position associated with the given order identifier (if found, else None).
#
#         :param order_id: The order identifier associated with the position.
#         :return: PositionId or None.
#         """
#         cdef PositionId position_id = self._index_order_position.get(order_id)
#         if position_id is None:
#             self._log.error(f"Cannot get position id for {order_id} (no matching position id found).")
#
#         return position_id
#
#     cpdef dict get_positions_all(self):
#         """
#         Return a dictionary of all positions held by the portfolio.
#
#         :return: Dict[PositionId, Position].
#         """
#         return self._cached_positions.copy()
#
#     cpdef dict get_positions_open_all(self):
#         """
#         Return a dictionary of all active positions held by the portfolio.
#
#         :return: Dict[PositionId, Position].
#         """
#         return self._positions_open.copy()
#
#     cpdef dict get_positions_closed_all(self):
#         """
#         Return a dictionary of all closed positions held by the portfolio.
#
#         :return: Dict[PositionId, Position].
#         """
#         return self._positions_closed.copy()
#
#     cpdef dict get_positions(self, StrategyId strategy_id):
#         """
#         Return a list of all positions associated with the given strategy identifier.
#
#         :param strategy_id: The strategy identifier associated with the positions.
#         :return: Dict[PositionId, Position].
#         :raises ConditionFailed: If the strategy identifier is not registered with the portfolio.
#         """
#         Condition.is_in(strategy_id, self._positions_open, 'strategy_id', 'positions_active')
#         Condition.is_in(strategy_id, self._positions_closed, 'strategy_id', 'positions_closed')
#
#         return {**self._positions_open[strategy_id], **self._positions_closed[strategy_id]}  # type: Dict[PositionId, Position]
#
#     cpdef dict get_positions_open(self, StrategyId strategy_id):
#         """
#         Return a list of all active positions associated with the given strategy identifier.
#
#         :param strategy_id: The strategy identifier associated with the positions.
#         :return: Dict[PositionId, Position].
#         :raises ConditionFailed: If the strategy identifier is not registered with the portfolio.
#         """
#         Condition.is_in(strategy_id, self._positions_open, 'strategy_id', 'positions_active')
#
#         return self._positions_open[strategy_id].copy()
#
#     cpdef dict get_positions_closed(self, StrategyId strategy_id):
#         """
#         Return a list of all active positions associated with the given strategy identifier.
#
#         :param strategy_id: The strategy identifier associated with the positions.
#         :return: Dict[PositionId, Position].
#         :raises ConditionFailed: If the strategy identifier is not registered with the portfolio.
#         """
#         Condition.is_in(strategy_id, self._positions_closed, 'strategy_id', 'positions_closed')
#
#         return self._positions_closed[strategy_id].copy()
#
    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.
        :param position_id: The position identifier.
        :return: True if the position exists, else False.
        """
        return position_id in self._cached_positions

    cpdef bint position_exists_for_order(self, OrderId order_id):
        """
        Return a value indicating whether there is a position associated with the given
        order identifier.

        :param order_id: The order identifier.
        :return: True if an associated position exists, else False.
        """
        cdef PositionId position_id = self._redis.hget(name=self.key_index_order_position, key=order_id)

        return position_id in self._cached_positions

    cpdef bint is_position_open(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is entered (active).

        :param position_id: The position identifier.
        :return: True if the position exists and is exited, else False.
        """
        return position_id in self._cached_positions and not self._cached_positions[position_id].is_flat

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is exited (closed).

        :param position_id: The position identifier.
        :return: True if the position does not exist or is closed, else False.
        """
        return position_id in self._cached_positions and self._cached_positions[position_id].is_closed

    cpdef int positions_count(self):
        """
        Return the total count of positions held by the database.

        :return: int.
        """
        return len(self._redis.keys(f'{self.key_positions}*'))

    cpdef int positions_open_count(self):
        """
        Return the count of open positions held by the execution database.

        :return: int.
        """
        return len(self._redis.smembers(self.key_index_positions_open))

    cpdef int positions_closed_count(self):
        """
        Return the count of closed positions held by the execution database.

        :return: int.
        """
        return len(self._redis.smembers(self.key_index_positions_closed))


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a process and thread safe execution engine utilizing Redis.
    """

    def __init__(self,
                 ExecutionDatabase database,
                 Account account,
                 Portfolio portfolio,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the RedisExecutionEngine class.

        :param database: The execution database for the engine.
        :param account: The account for the engine.
        :param portfolio: The portfolio for the engine.
        :param clock: The clock for the engine.
        :param guid_factory: The guid factory for the engine.
        :param logger: The logger for the engine.
        """
        super().__init__(
            database=database,
            account=account,
            portfolio=portfolio,
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)

        self._queue = queue.Queue()
        self._thread = threading.Thread(target=self._process_queue, daemon=True)
        self._thread.start()

    cpdef void execute_command(self, Command command):
        """
        Execute the given command by inserting it into the message bus for processing.
        
        :param command: The command to execute.
        """
        self._queue.put(command)

    cpdef void handle_event(self, Event event):
        """
        Handle the given event by inserting it into the message bus for processing.
        
        :param event: The event to handle
        """
        self._queue.put(event)

    cpdef void _process_queue(self):
        self._log.info("Running...")

        # Process the queue one item at a time
        cdef Message message
        while True:
            message = self._queue.get()

            if message.message_type == MessageType.EVENT:
                self._handle_event(message)
            elif message.message_type == MessageType.COMMAND:
                self._execute_command(message)
            else:
                raise RuntimeError(f"Invalid message type on queue ({repr(message)}).")


cdef class LiveExecClient(ExecutionClient):
    """
    Provides an execution client for live trading utilizing a ZMQ transport
    to the execution service.
    """

    def __init__(
            self,
            ExecutionEngine exec_engine,
            zmq_context: Context,
            str service_name='NautilusExecutor',
            str service_address='localhost',
            str events_topic='NAUTILUS:EVENTS',
            int commands_port=55555,
            int events_port=55556,
            CommandSerializer command_serializer=MsgPackCommandSerializer(),
            ResponseSerializer response_serializer=MsgPackResponseSerializer(),
            EventSerializer event_serializer=MsgPackEventSerializer(),
            Logger logger=LiveLogger()):
        """
        Initializes a new instance of the LiveExecClient class.

        :param exec_engine: The execution engine for the component.
        :param zmq_context: The ZMQ context.
        :param service_name: The name of the service.
        :param service_address: The execution service host IP address (default='localhost').
        :param events_topic: The execution service events topic (default='NAUTILUS:EXECUTION').
        :param commands_port: The execution service commands port (default=55555).
        :param events_port: The execution service events port (default=55556).
        :param command_serializer: The command serializer for the client.
        :param response_serializer: The response serializer for the client.
        :param event_serializer: The event serializer for the client.

        :param logger: The logger for the component (can be None).
        :raises ConditionFailed: If the service_address is not a valid string.
        :raises ConditionFailed: If the events_topic is not a valid string.
        :raises ConditionFailed: If the commands_port is not in range [0, 65535].
        :raises ConditionFailed: If the events_port is not in range [0, 65535].
        """
        Condition.valid_string(service_address, 'service_address')
        Condition.valid_string(events_topic, 'events_topic')
        Condition.in_range(commands_port, 'commands_port', 0, 65535)
        Condition.in_range(events_port, 'events_port', 0, 65535)

        super().__init__(exec_engine, logger)
        self._zmq_context = zmq_context

        self._commands_worker = RequestWorker(
            f'{self.__class__.__name__}.CommandRequester',
            f'{service_name}.CommandRouter',
            service_address,
            commands_port,
            self._zmq_context,
            logger)

        self._events_worker = SubscriberWorker(
            f'{self.__class__.__name__}.EventSubscriber',
            f'{service_name}.EventPublisher',
            service_address,
            events_port,
            self._zmq_context,
            self._event_handler,
            logger)

        self._command_serializer = command_serializer
        self._response_serializer = response_serializer
        self._event_serializer = event_serializer

        self.events_topic = events_topic

    cpdef void connect(self):
        """
        Connect to the execution service.
        """
        self._events_worker.connect()
        self._commands_worker.connect()
        self._events_worker.subscribe(self.events_topic)

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        self._events_worker.unsubscribe(self.events_topic)
        self._commands_worker.disconnect()
        self._events_worker.disconnect()

    cpdef void dispose(self):
        """
        Disposes of the execution client.
        """
        self._commands_worker.dispose()
        self._events_worker.dispose()

    cpdef void reset(self):
        """
        Reset the execution client.
        """
        self._reset()

    cpdef void account_inquiry(self, AccountInquiry command):
        self._command_handler(command)

    cpdef void submit_order(self, SubmitOrder command):
        self._command_handler(command)

    cpdef void submit_atomic_order(self, SubmitAtomicOrder command):
        self._command_handler(command)

    cpdef void modify_order(self, ModifyOrder command):
        self._command_handler(command)

    cpdef void cancel_order(self, CancelOrder command):
        self._command_handler(command)

    cdef void _command_handler(self, Command command):
        self._log.debug(f"Sending {command} ...")
        cdef bytes response_bytes = self._commands_worker.send(self._command_serializer.serialize(command))
        cdef Response response =  self._response_serializer.deserialize(response_bytes)
        self._log.debug(f"Received response {response}")

    cdef void _event_handler(self, str topic, bytes event_bytes):
        cdef Event event = self._event_serializer.deserialize(event_bytes)
        self._exec_engine.handle_event(event)
