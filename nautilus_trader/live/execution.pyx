# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import queue
import threading
import redis
import zmq

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport MessageType, Message, Response
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.identifiers cimport (
    AccountId,
    TraderId,
    StrategyId,
    OrderId,
    PositionId,
    PositionIdBroker)
from nautilus_trader.model.commands cimport (
    Command,
    AccountInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    ModifyOrder,
    CancelOrder)
from nautilus_trader.model.events cimport Event, AccountStateEvent, OrderFillEvent, OrderInitialized
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger
from nautilus_trader.common.execution cimport ExecutionDatabase, ExecutionEngine, ExecutionClient
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.network.workers cimport RequestWorker, SubscriberWorker
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer, MsgPackResponseSerializer
from nautilus_trader.live.logger cimport LiveLogger
from nautilus_trader.serialization.serializers cimport EventSerializer, MsgPackEventSerializer
from nautilus_trader.trade.strategy cimport TradingStrategy

cdef str UTF8 = 'utf-8'

cdef str INDEX = 'Index'
cdef str TRADER = 'Trader'
cdef str CONFIG = 'Config'
cdef str ACCOUNTS = 'Accounts'
cdef str ORDER = 'Order'
cdef str ORDERS = 'Orders'
cdef str BROKER = 'BrokerId'
cdef str POSITION = 'Position'
cdef str POSITIONS = 'Positions'
cdef str STRATEGY = 'Strategy'
cdef str STRATEGIES = 'Strategies'
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
                 Logger logger,
                 bint option_load_cache=True):
        """
        Initializes a new instance of the RedisExecutionEngine class.

        :param trader_id: The trader_id.
        :param host: The redis host for the database connection.
        :param port: The redis port for the database connection.
        :param command_serializer: The command serializer for database transactions.
        :param event_serializer: The event serializer for database transactions.
        :param option_load_cache: The option flag to load caches from Redis on instantiation.
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
        self.key_index_order_position     = f'{self.key_trader}:{INDEX}:{ORDER}{POSITION}'      # HASH
        self.key_index_order_strategy     = f'{self.key_trader}:{INDEX}:{ORDER}{STRATEGY}'      # HASH
        self.key_index_broker_position    = f'{self.key_trader}:{INDEX}:{BROKER}{POSITION}'     # HASH
        self.key_index_position_strategy  = f'{self.key_trader}:{INDEX}:{POSITION}{STRATEGY}'   # HASH
        self.key_index_position_orders    = f'{self.key_trader}:{INDEX}:{POSITION}{ORDERS}:'    # SET
        self.key_index_strategy_orders    = f'{self.key_trader}:{INDEX}:{STRATEGY}{ORDERS}:'    # SET
        self.key_index_strategy_positions = f'{self.key_trader}:{INDEX}:{STRATEGY}{POSITIONS}:' # SET
        self.key_index_orders             = f'{self.key_trader}:{INDEX}:{ORDERS}'               # SET
        self.key_index_orders_working     = f'{self.key_trader}:{INDEX}:{ORDERS}:{WORKING}'     # SET
        self.key_index_orders_completed   = f'{self.key_trader}:{INDEX}:{ORDERS}:{COMPLETED}'   # SET
        self.key_index_positions          = f'{self.key_trader}:{INDEX}:{POSITIONS}'            # SET
        self.key_index_positions_open     = f'{self.key_trader}:{INDEX}:{POSITIONS}:{OPEN}'     # SET
        self.key_index_positions_closed   = f'{self.key_trader}:{INDEX}:{POSITIONS}:{CLOSED}'   # SET

        # Serializers
        self._command_serializer = command_serializer
        self._event_serializer = event_serializer

        # Redis client
        self._redis = redis.Redis(host=host, port=port, db=0)

        # Options
        self.OPTION_LOAD_CACHES = option_load_cache

        if self.OPTION_LOAD_CACHES:
            self._log.info(f"The OPTION_LOAD_CACHES is {self.OPTION_LOAD_CACHES}")
            # Load cache
            self.load_accounts_cache()
            self.load_orders_cache()
            self.load_positions_cache()
        else:
            self._log.warning(f"The OPTION_LOAD_CACHE is {self.OPTION_LOAD_CACHES} "
                              f"(this should only be done in a testing environment).")


# -- COMMANDS -------------------------------------------------------------------------------------"

    cpdef void load_accounts_cache(self) except *:
        """
        Clear the current accounts cache and load accounts from the database.
        """
        self._log.info("Re-caching accounts from the database...")
        self._cached_accounts.clear()

        cdef list account_keys = self._redis.keys(f'{self.key_accounts}*')
        if len(account_keys) == 0:
            self._log.info('No accounts found in database.')
            return

        cdef bytes key_bytes
        cdef AccountId account_id
        cdef Account account
        for key_bytes in account_keys:
            account_id = AccountId.from_string(key_bytes.decode(UTF8).rsplit(':', maxsplit=1)[1])
            account = self.load_account(account_id)

            if account:
                self._cached_accounts[account.id] = account

        self._log.info(f"Cached {len(self._cached_accounts)} account(s).")

    cpdef void load_orders_cache(self) except *:
        """
        Clear the current order cache and load orders from the database.
        """
        self._log.info("Re-caching orders from the database...")
        self._cached_orders.clear()

        cdef list order_keys = self._redis.keys(f'{self.key_orders}*')
        if len(order_keys) == 0:
            self._log.info('No orders found in database.')
            return

        cdef bytes key_bytes
        cdef OrderId order_id
        cdef Order order
        for key_bytes in order_keys:
            order_id = OrderId(key_bytes.decode(UTF8).rsplit(':', maxsplit=1)[1])
            order = self.load_order(order_id)

            if order:
                self._cached_orders[order.id] = order

        self._log.info(f"Cached {len(self._cached_orders)} order(s).")

    cpdef void load_positions_cache(self) except *:
        """
        Clear the current order cache and load orders from the database.
        """
        self._log.info("Re-caching positions from the database...")
        self._cached_positions.clear()

        cdef list position_keys = self._redis.keys(f'{self.key_positions}*')
        if len(position_keys) == 0:
            self._log.info('No positions found in database.')
            return

        cdef bytes key_bytes
        cdef PositionId position_id
        cdef Position position

        for key_bytes in position_keys:
            position_id = PositionId(key_bytes.decode(UTF8).rsplit(':', maxsplit=1)[1])
            position = self.load_position(position_id)

            if position:
                self._cached_positions[position.id] = position

        self._log.info(f"Cached {len(self._cached_positions)} position(s).")

    cpdef void add_account(self, Account account) except *:
        """
        Add the given account to the execution database.

        :param account: The account to add.
        :raises ConditionFailed: If the account_id is already contained in the cached_accounts.
        """
        Condition.not_in(account.id, self._cached_accounts, 'account.id', 'cached_accounts')

        self._cached_accounts[account.id] = account

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_accounts + account.id.value, self._event_serializer.serialize(account.last_event))
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        if reply[0] > 1:  # Reply = The length of the list after the push operation
            self._log.error(f"The {account.id} already existed in the accounts and was appended to.")

        self._log.debug(f"Added Account(id={account.id.value}).")

    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id) except *:
        """
        Add the given order to the execution database indexed with the given strategy and position
        identifiers.

        :param order: The order to add.
        :param strategy_id: The strategy_id to index for the order.
        :param position_id: The position_id to index for the order.
        :raises ConditionFailed: If the order_id is already contained in the cached_orders.
        """
        Condition.not_in(order.id, self._cached_orders, 'order.id', 'cached_orders')

        self._cached_orders[order.id] = order

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_orders + order.id.value, self._event_serializer.serialize(order.last_event))  # 0
        pipe.hset(name=self.key_index_order_position, key=order.id.value, value=position_id.value)        # 1
        pipe.hset(name=self.key_index_order_strategy, key=order.id.value, value=strategy_id.value)        # 2
        pipe.hset(name=self.key_index_position_strategy, key=position_id.value, value=strategy_id.value)  # 3
        pipe.sadd(self.key_index_orders, order.id.value)                                                  # 4
        pipe.sadd(self.key_index_position_orders + position_id.value, order.id.value)                     # 5
        pipe.sadd(self.key_index_strategy_orders + strategy_id.value, order.id.value)                     # 6
        pipe.sadd(self.key_index_strategy_positions + strategy_id.value, position_id.value)               # 7
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        if reply[0] > 1:  # Reply = The length of the list after the push operation
            self._log.error(f"The {order.id} already existed in the orders and was appended to.")
        if reply[1] == 0:  # Reply = 0 if field already exists in the hash and the value was updated
            self._log.error(f"The {order.id} already existed in index_order_position and was overwritten.")
        if reply[2] == 0:  # Reply = 0 if field already exists in the hash and the value was updated
            self._log.error(f"The {order.id} already existed in index_order_strategy and was overwritten.")
        # reply[3] index_position_strategy does not need to be checked as there will be multiple writes for atomic orders
        if reply[4] == 0:  # Reply = 0 if the element was already a member of the set
            self._log.error(f"The {order.id} already existed in index_orders.")
        if reply[5] == 0:  # Reply = 0 if the element was already a member of the set
            self._log.error(f"The {order.id} already existed in index_position_orders.")
        if reply[6] == 0:  # Reply = 0 if the element was already a member of the set
            self._log.error(f"The {order.id} already existed in index_strategy_orders.")
        # reply[7] index_strategy_positions does not need to be checked as there will be multiple writes for atomic orders

        self._log.debug(f"Added Order(id={order.id.value}).")

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        """
        Add the given position associated with the given strategy_id.
        
        :param position: The position to add.
        :param strategy_id: The strategy_id to associate with the position.
        :raises ConditionFailed: If the position_id is already contained in the cached_positions.
        """
        Condition.not_in(position.id, self._cached_positions, 'position.id', 'cached_positions')

        self._cached_positions[position.id] = position

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_positions + position.id.value, self._event_serializer.serialize(position.last_event))
        pipe.hset(name=self.key_index_broker_position, key=position.id_broker.value, value=position.id.value)
        pipe.sadd(self.key_index_positions, position.id.value)
        pipe.sadd(self.key_index_positions_open, position.id.value)
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        if reply[0] > 1:  # Reply = The length of the list after the push operation
            self._log.error(f"The {position.id_broker} already existed in the index_broker_position and was overwritten.")
        if reply[1] == 0:  # Reply = 0 if the element was already a member of the set
            self._log.error(f"The {position.id} already existed in index_positions.")
        if reply[2] == 0:  # Reply = 0 if the element was already a member of the set
            self._log.error(f"The {position.id} already existed in index_positions.")
        if reply[3] == 0:  # Reply = 0 if the element was already a member of the set
            self._log.error(f"The {position.id} already existed in index_positions_open.")

        self._log.debug(f"Added Position(id={position.id.value}).")

    cpdef void update_account(self, Account account) except *:
        """
        Update the given account in the execution database by persisting its
        last event.

        :param account: The account to update (from last event).
        """
        self._redis.rpush(self.key_accounts + account.id.value, self._event_serializer.serialize(account.last_event))
        self._log.debug(f"Updated Account(id={account.id}).")

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        """
        Update the given strategy state in the execution database.
        
        :param strategy: The strategy to update.
        """
        cdef dict state = strategy.save()

        pipe = self._redis.pipeline()

        for entry in state['StateLog']:
            if entry:
                pipe.rpush(self.key_strategies + strategy.id.value + ':StateLog', entry)

        for key, value in state.items():
            if key == 'StateLog':
                continue # Already persisted (cannot directly persist list)
            pipe.hset(name=self.key_strategies + strategy.id.value + ':State', key=key, value=value)
            self._log.debug(f"Saving {strategy.id} state (key='{key}', value={value})...")
        cdef list reply = pipe.execute()

        strategy.update_state_log(strategy.clock.time_now(), "SAVED")

        self._log.info(f"Saved strategy state for {strategy.id.value}.")

    cpdef void update_order(self, Order order) except *:
        """
        Update the given order in the execution database.

        :param order: The order to update (from last event).
        """
        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_orders + order.id.value, self._event_serializer.serialize(order.last_event))
        if order.is_working:
            pipe.sadd(self.key_index_orders_working, order.id.value)
            pipe.srem(self.key_index_orders_completed, order.id.value)
        elif order.is_completed:
            pipe.sadd(self.key_index_orders_completed, order.id.value)
            pipe.srem(self.key_index_orders_working, order.id.value)
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        if reply[0] == 1:  # Reply = The length of the list after the push operation
            self._log.error(f"The updated Order(id={order.id.value}) did not already exist.")

        self._log.debug(f"Updated Order(id={order.id.value}).")

    cpdef void update_position(self, Position position) except *:
        """
        Update the given position in the execution database.

        :param position: The position to update (from last event).
        """
        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_positions + position.id.value, self._event_serializer.serialize(position.last_event))
        if position.is_closed:
            pipe.sadd(self.key_index_positions_closed, position.id.value)
            pipe.srem(self.key_index_positions_open, position.id.value)
        else:
            pipe.sadd(self.key_index_positions_open, position.id.value)
            pipe.srem(self.key_index_positions_closed, position.id.value)
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        if reply[0] == 1:  # Reply = The length of the list after the push operation
            self._log.error(f"The updated Position(id={position.id.value}) did not already exist.")

        self._log.debug(f"Updated Position(id={position.id.value}).")

    cpdef void load_strategy(self, TradingStrategy strategy) except *:
        """
        Load the state for the given strategy from the execution database.
        
        :param strategy: The strategy to load.
        """
        cdef list state_log = self._redis.lrange(name=self.key_strategies + strategy.id.value + ':StateLog', start=0, end=-1)
        cdef dict state = self._redis.hgetall(name=self.key_strategies + strategy.id.value + ':State')

        if state_log:
            state[b'StateLog'] = state_log

        if len(state) == 0:
            self._log.info(f"No previous state found for Strategy(id={strategy.id.value}).")
            return

        for key, value in state.items():
            self._log.debug(f"Loading Strategy(id={strategy.id.value}) state (key='{key}', value={value})...")
        strategy.load(state)

        self._log.info(f"Loaded Strategy(id={strategy.id.value}) state.")

    cpdef Account load_account(self, AccountId account_id):
        """
        Load the account associated with the given account_id (if found).
        
        :param account_id: The account identifier to load.
        :return: Account or None.
        """
        cdef list events = self._redis.lrange(name=self.key_accounts + account_id.value, start=0, end=-1)
        if len(events) == 0:
            self._log.error(f"Cannot load Account(id={account_id.value}) from database (not found).")
            return None

        cdef AccountStateEvent last_event = self._event_serializer.deserialize(events.pop())
        return Account(event=last_event)

    cpdef Order load_order(self, OrderId order_id):
        """
        Load the order associated with the given order_id (if found).
        
        :param order_id: The order_id to load.
        :return: Order or None.
        """
        cdef list events = self._redis.lrange(name=self.key_orders + order_id.value, start=0, end=-1)

        # Check there is at least one event to pop
        if len(events) == 0:
            self._log.error(f"Cannot load Order(id={order_id.value}) from database (not found).")
            return None

        cdef OrderInitialized initial = self._event_serializer.deserialize(events.pop(0))
        cdef Order order = Order.create(event=initial)

        cdef bytes event_bytes
        for event_bytes in events:
            order.apply(self._event_serializer.deserialize(event_bytes))
        return order

    cpdef Position load_position(self, PositionId position_id):
        """
        Load the position associated with the given position_id (if found).
        
        :param position_id: The position_id to load.
        :return: Position or None.
        """
        cdef list events = self._redis.lrange(name=self.key_positions + position_id.value, start=0, end=-1)

        # Check there is at least one event to pop
        if len(events) == 0:
            self._log.error(f"Cannot load Position(id={position_id.value}) from database (not found).")
            return None

        cdef OrderFillEvent initial = self._event_serializer.deserialize(events.pop(0))
        cdef Position position = Position(position_id=position_id, event=initial)

        cdef bytes event_bytes
        for event_bytes in events:
            position.apply(self._event_serializer.deserialize(event_bytes))
        return position

    cpdef void delete_strategy(self, TradingStrategy strategy) except *:
        """
        Delete the given strategy from the execution database.

        :param strategy: The strategy to deregister.
        """
        pipe = self._redis.pipeline()
        pipe.delete(self.key_strategies + strategy.id.value)
        pipe.execute()

        self._log.info(f"Deleted Strategy(id={strategy.id.value}).")

    # noinspection PyUnresolvedReferences
    cpdef void check_residuals(self) except *:
        # Check for any residual active orders and log warnings if any are found
        for order_id, order in self.get_orders_working().items():
            self._log.warning(f"Residual working {order}")

        for position_id, position in self.get_positions_open().items():
            self._log.warning(f"Residual open {position}")

    cpdef void reset(self) except *:
        """
        Reset the execution database by clearing the cache.
        """
        self._reset()

    cpdef void flush(self) except *:
        """
        Flush the database which clears all data.
        """
        self._log.debug('Flushing database....')
        self._redis.flushdb()
        self._log.info('Flushed database.')

    cdef set _decode_set_to_order_ids(self, set original):
        return {OrderId(element.decode(UTF8)) for element in original}

    cdef set _decode_set_to_position_ids(self, set original):
        return {PositionId(element.decode(UTF8)) for element in original}

    cdef set _decode_set_to_strategy_ids(self, list original):
        return {StrategyId.from_string(element.decode(UTF8).rsplit(':', 2)[1]) for element in original}

# -- QUERIES --------------------------------------------------------------------------------------"

    cpdef Account get_account(self, AccountId account_id):
        """
        Return the order matching the given identifier (if found).

        :param account_id: The account_id.
        :return Account or None.
        """
        return self._cached_accounts.get(account_id)

    cpdef set get_strategy_ids(self):
        """
        Return a set of all strategy_ids.
         
        :return Set[StrategyId].
        """
        return  self._decode_set_to_strategy_ids(self._redis.keys(pattern=f'{self.key_strategies}*'))

    cpdef set get_order_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all order_ids.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Set[OrderId].
        """
        if strategy_id is None:
            return self._decode_set_to_order_ids(self._redis.smembers(name=self.key_index_orders))
        return self._decode_set_to_order_ids(self._redis.smembers(name=self.key_index_strategy_orders + strategy_id.value))

    cpdef set get_order_working_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all working order_ids.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Set[OrderId].
        """
        if strategy_id is None:
            return self._decode_set_to_order_ids(self._redis.smembers(name=self.key_index_orders_working))
        return self._decode_set_to_order_ids(self._redis.sinter(keys=(self.key_index_orders_working, self.key_index_strategy_orders + strategy_id.value)))

    cpdef set get_order_completed_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all completed order_ids.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Set[OrderId].
        """
        if strategy_id is None:
            return self._decode_set_to_order_ids(self._redis.smembers(name=self.key_index_orders_completed))
        return self._decode_set_to_order_ids(self._redis.sinter(keys=(self.key_index_orders_completed, self.key_index_strategy_orders + strategy_id.value)))

    cpdef set get_position_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all position_ids.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Set[PositionId].
        """
        if strategy_id is None:
            return self._decode_set_to_position_ids(self._redis.smembers(name=self.key_index_positions))
        return self._decode_set_to_position_ids(self._redis.smembers(name=self.key_index_strategy_positions + strategy_id.value))

    cpdef set get_position_open_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all open position_ids.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Set[PositionId].
        """
        if strategy_id is None:
            return self._decode_set_to_position_ids(self._redis.smembers(name=self.key_index_positions_open))
        return self._decode_set_to_position_ids(self._redis.sinter(keys=(self.key_index_positions_open, self.key_index_strategy_positions + strategy_id.value)))

    cpdef set get_position_closed_ids(self, StrategyId strategy_id=None):
        """
        Return a set of all closed position_ids.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Set[PositionId].
        """
        if strategy_id is None:
            return self._decode_set_to_position_ids(self._redis.smembers(name=self.key_index_positions_closed))
        return self._decode_set_to_position_ids(self._redis.sinter(keys=(self.key_index_positions_closed, self.key_index_strategy_positions + strategy_id.value)))

    cpdef StrategyId get_strategy_for_order(self, OrderId order_id):
        """
        Return the strategy_id associated with the given order_id (if found).
        
        :param order_id: The order_id associated with the strategy.
        :return StrategyId or None: 
        """
        return StrategyId.from_string(self._redis.hget(name=self.key_index_order_strategy, key=order_id.value).decode(UTF8))

    cpdef StrategyId get_strategy_for_position(self, PositionId position_id):
        """
        Return the strategy_id associated with the given position_id (if found).
        
        :param position_id: The position_id associated with the strategy.
        :return StrategyId or None: 
        """
        return StrategyId.from_string(self._redis.hget(name=self.key_index_position_strategy, key=position_id.value).decode(UTF8))

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
        cdef dict orders = {}

        try:
            orders = {order_id: self._cached_orders[order_id] for order_id in order_ids}
        except KeyError as ex:
            self._log.error("Cannot find Order object in cache " + str(ex))

        return orders

    cpdef dict get_orders_working(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all working orders.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[OrderId, Order].
        """
        cdef set order_ids = self.get_order_working_ids(strategy_id)
        cdef dict orders = {}

        try:
            orders = {order_id: self._cached_orders[order_id] for order_id in order_ids}
        except KeyError as ex:
            self._log.error("Cannot find Order object in the cache " + str(ex))

        return orders

    cpdef dict get_orders_completed(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all completed orders.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[OrderId, Order].
        """
        cdef set order_ids = self.get_order_completed_ids(strategy_id)
        cdef dict orders = {}

        try:
            orders = {order_id: self._cached_orders[order_id] for order_id in order_ids}
        except KeyError as ex:
            self._log.error("Cannot find Order object in cache " + str(ex))

        return orders

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
            self._log.warning(f"Cannot get Position for {order_id} (no matching PositionId found in database).")
            return None

        return self._cached_positions.get(position_id)

    cpdef PositionId get_position_id(self, OrderId order_id):
        """
        Return the position associated with the given order_id (if found, else None).
        
        :param order_id: The order_id associated with the position.
        :return PositionId or None.
        """
        cdef bytes position_id_bytes = self._redis.hget(name=self.key_index_order_position, key=order_id.value)
        if position_id_bytes is None:
            self._log.warning(f"Cannot get PositionId for {order_id} (no matching PositionId found in database).")
            return position_id_bytes

        return PositionId(position_id_bytes.decode(UTF8))

    cpdef PositionId get_position_id_for_broker_id(self, PositionIdBroker position_id_broker):
        """
        Return the position associated with the given order_id (if found, else None).
        
        :param position_id_broker: The broker position_id.
        :return PositionId or None.
        """
        cdef bytes position_id_bytes = self._redis.hget(name=self.key_index_broker_position, key=position_id_broker.value)
        if position_id_bytes is None:
            self._log.warning(f"Cannot get PositionId for {position_id_broker} (no matching PositionId found in database).")
            return position_id_bytes

        return PositionId(position_id_bytes.decode(UTF8))

    cpdef dict get_positions(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all positions.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_ids(strategy_id)
        cdef dict positions = {}

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            # This should never happen
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef dict get_positions_open(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all open positions.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_open_ids(strategy_id)
        cdef dict positions = {}

        try:
            positions = {position_id: self._cached_positions[position_id] for position_id in position_ids}
        except KeyError as ex:
            # This should never happen
            self._log.error("Cannot find Position object in cache " + str(ex))

        return positions

    cpdef dict get_positions_closed(self, StrategyId strategy_id=None):
        """
        Return a dictionary of all closed positions.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return Dict[PositionId, Position].
        """
        cdef set position_ids = self.get_position_closed_ids(strategy_id)
        cdef dict positions = {}

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
        return self._redis.sismember(name=self.key_index_orders, value=order_id.value)

    cpdef bint is_order_working(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is working.

        :param order_id: The order_id to check.
        :return bool.
        """
        return self._redis.sismember(name=self.key_index_orders_working, value=order_id.value)

    cpdef bint is_order_completed(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is completed.

        :param order_id: The order_id to check.
        :return bool.
        """
        return self._redis.sismember(name=self.key_index_orders_completed, value=order_id.value)

    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.
        
        :param position_id: The position_id.
        :return bool.
        """
        return self._redis.sismember(name=self.key_index_positions, value=position_id.value)

    cpdef bint position_exists_for_order(self, OrderId order_id):
        """
        Return a value indicating whether there is a position associated with the given
        order_id.
        
        :param order_id: The order_id.
        :return bool.
        """
        cdef bytes position_id = self._redis.hget(name=self.key_index_order_position, key=order_id.value)

        if position_id is None:
            return False
        return self._redis.sismember(name=self.key_index_positions, value=position_id)

    cpdef bint position_indexed_for_order(self, OrderId order_id):
        """
        Return a value indicating whether there is a position_id indexed for the 
        given order_id.

        :param order_id: The order_id to check.
        :return bool.
        """
        return self._redis.hexists(name=self.key_index_order_position, key=order_id.value)

    cpdef bint is_position_open(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is open.

        :param position_id: The position_id.
        :return bool.
        """
        return self._redis.sismember(name=self.key_index_positions_open, value=position_id.value)

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists
        and is closed.

        :param position_id: The position_id.
        :return bool.
        """
        return self._redis.sismember(name=self.key_index_positions_closed, value=position_id.value)

    cpdef int count_orders_total(self, StrategyId strategy_id=None):
        """
        Return the count of order_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        if strategy_id is None:
            return self._redis.scard(self.key_index_orders)

        return len(self._redis.sinter(keys=(self.key_index_orders, self.key_index_strategy_orders + strategy_id.value)))

    cpdef int count_orders_working(self, StrategyId strategy_id=None):
        """
        Return the count of working order_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        if strategy_id is None:
            return self._redis.scard(self.key_index_orders_working)

        return len(self._redis.sinter(keys=(self.key_index_orders_working, self.key_index_strategy_orders + strategy_id.value)))

    cpdef int count_orders_completed(self, StrategyId strategy_id=None):
        """
        Return the count of completed order_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        if strategy_id is None:
            return self._redis.scard(self.key_index_orders_completed)

        return len(self._redis.sinter(keys=(self.key_index_orders_completed, self.key_index_strategy_orders + strategy_id.value)))

    cpdef int count_positions_total(self, StrategyId strategy_id=None):
        """
        Return the count of position_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        if strategy_id is None:
            return self._redis.scard(self.key_index_positions)

        return len(self._redis.sinter(keys=(self.key_index_positions, self.key_index_strategy_positions + strategy_id.value)))

    cpdef int count_positions_open(self, StrategyId strategy_id=None):
        """
        Return the count of open position_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        if strategy_id is None:
            return self._redis.scard(self.key_index_positions_open)

        return len(self._redis.sinter(keys=(self.key_index_positions_open, self.key_index_strategy_positions + strategy_id.value)))

    cpdef int count_positions_closed(self, StrategyId strategy_id=None):
        """
        Return the count of closed position_ids held by the execution database.
        
        :param strategy_id: The strategy_id query filter (optional can be None).
        :return int.
        """
        if strategy_id is None:
            return self._redis.scard(self.key_index_positions_closed)

        return len(self._redis.sinter(keys=(self.key_index_positions_closed, self.key_index_strategy_positions + strategy_id.value)))


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a process and thread safe execution engine utilizing Redis.
    """

    def __init__(self,
                 TraderId trader_id,
                 AccountId account_id,
                 ExecutionDatabase database,
                 Portfolio portfolio,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the RedisExecutionEngine class.

        :param trader_id: The trader_id for the engine.
        :param account_id: The account_id for the engine.
        :param database: The execution database for the engine.
        :param portfolio: The portfolio for the engine.
        :param clock: The clock for the engine.
        :param guid_factory: The guid factory for the engine.
        :param logger: The logger for the engine.
        """
        super().__init__(
            trader_id=trader_id,
            account_id=account_id,
            database=database,
            portfolio=portfolio,
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)

        self._message_bus = queue.Queue()
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

    cpdef void execute_command(self, Command command):
        """
        Execute the given command by inserting it into the message bus for processing.
        
        :param command: The command to execute.
        """
        self._message_bus.put(command)

    cpdef void handle_event(self, Event event):
        """
        Handle the given event by inserting it into the message bus for processing.
        
        :param event: The event to handle
        """
        self._message_bus.put(event)

    cpdef void _consume_messages(self):
        self._log.info("Running...")

        cdef Message message
        while True:
            message = self._message_bus.get()

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
            zmq_context: zmq.Context,
            str service_name='NautilusExecutor',
            str service_address='localhost',
            str events_topic='EVENTS',
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

    cpdef void _command_handler(self, Command command) except *:
        self._log.debug(f"Sending command {command}...")
        cdef bytes command_bytes = self._command_serializer.serialize(command)
        cdef bytes response_bytes = self._commands_worker.send(command_bytes)
        cdef Response response =  self._response_serializer.deserialize(response_bytes)
        self._log.debug(f"Received response {response}")

    cpdef void _event_handler(self, str topic, bytes event_bytes) except *:
        cdef Event event = self._event_serializer.deserialize(event_bytes)
        self._exec_engine.handle_event(event)
