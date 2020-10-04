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

import redis

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport LimitOrder
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport StopOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.serialization.base cimport CommandSerializer
from nautilus_trader.serialization.serializers cimport EventSerializer
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef str _UTF8 = 'utf-8'

cdef str _INDEX = 'Index'
cdef str _TRADER = 'Trader'
cdef str _CONFIG = 'Config'
cdef str _ACCOUNTS = 'Accounts'
cdef str _ORDER = 'Order'
cdef str _ORDERS = 'Orders'
cdef str _POSITION = 'Position'
cdef str _POSITIONS = 'Positions'
cdef str _SYMBOL = 'Symbol'
cdef str _STRATEGY = 'Strategy'
cdef str _STRATEGIES = 'Strategies'
cdef str _WORKING = 'Working'
cdef str _COMPLETED = 'Completed'
cdef str _OPEN = 'Open'
cdef str _CLOSED = 'Closed'
cdef str _SL = 'SL'
cdef str _TP = 'TP'


cdef class RedisExecutionDatabase(ExecutionDatabase):
    """
    Provides an execution cache utilizing Redis.
    """

    def __init__(
            self,
            TraderId trader_id not None,
            Logger logger not None,
            str host not None,
            int port,
            CommandSerializer command_serializer not None,
            EventSerializer event_serializer not None,
    ):
        """
        Initialize a new instance of the RedisExecutionDatabase class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the database.
        logger : Logger
            The logger for the database.
        host : str
            The redis host for the cache connection.
        port : int
            The redis port for the cache connection.
        command_serializer : CommandSerializer
            The command serializer for cache transactions.
        event_serializer : EventSerializer
            The event serializer for cache transactions.

        Raises
        ------
        ValueError
            If the host is not a valid string.
        ValueError
            If the port is not in range [0, 65535].

        """
        Condition.valid_string(host, "host")
        Condition.in_range_int(port, 0, 65535, "port")
        super().__init__(trader_id, logger)

        # Database keys
        self.key_trader                   = f"{_TRADER}-{trader_id.value}"                                 # noqa
        self.key_accounts                 = f"{self.key_trader}:{_ACCOUNTS}:"                              # noqa
        self.key_orders                   = f"{self.key_trader}:{_ORDERS}:"                                # noqa
        self.key_positions                = f"{self.key_trader}:{_POSITIONS}:"                             # noqa
        self.key_strategies               = f"{self.key_trader}:{_STRATEGIES}:"                            # noqa
        self.key_index_order_position     = f"{self.key_trader}:{_INDEX}:{_ORDER}{_POSITION}"      # HASH  # noqa
        self.key_index_order_strategy     = f"{self.key_trader}:{_INDEX}:{_ORDER}{_STRATEGY}"      # HASH  # noqa
        self.key_index_position_strategy  = f"{self.key_trader}:{_INDEX}:{_POSITION}{_STRATEGY}"   # HASH  # noqa
        self.key_index_position_orders    = f"{self.key_trader}:{_INDEX}:{_POSITION}{_ORDERS}:"    # SET   # noqa
        self.key_index_symbol_orders      = f"{self.key_trader}:{_INDEX}:{_SYMBOL}{_ORDERS}:"      # SET   # noqa
        self.key_index_symbol_positions   = f"{self.key_trader}:{_INDEX}:{_SYMBOL}{_POSITIONS}:"   # SET   # noqa
        self.key_index_strategy_orders    = f"{self.key_trader}:{_INDEX}:{_STRATEGY}{_ORDERS}:"    # SET   # noqa
        self.key_index_strategy_positions = f"{self.key_trader}:{_INDEX}:{_STRATEGY}{_POSITIONS}:" # SET   # noqa
        self.key_index_orders             = f"{self.key_trader}:{_INDEX}:{_ORDERS}"                # SET   # noqa
        self.key_index_orders_working     = f"{self.key_trader}:{_INDEX}:{_ORDERS}:{_WORKING}"     # SET   # noqa
        self.key_index_orders_completed   = f"{self.key_trader}:{_INDEX}:{_ORDERS}:{_COMPLETED}"   # SET   # noqa
        self.key_index_positions          = f"{self.key_trader}:{_INDEX}:{_POSITIONS}"             # SET   # noqa
        self.key_index_positions_open     = f"{self.key_trader}:{_INDEX}:{_POSITIONS}:{_OPEN}"     # SET   # noqa
        self.key_index_positions_closed   = f"{self.key_trader}:{_INDEX}:{_POSITIONS}:{_CLOSED}"   # SET   # noqa
        self.key_index_stop_loss_ids      = f"{self.key_trader}:{_INDEX}:{_SL}"                    # SET   # noqa
        self.key_index_take_profit_ids    = f"{self.key_trader}:{_INDEX}:{_TP}"                    # SET   # noqa

        # Serializers
        self._command_serializer = command_serializer
        self._event_serializer = event_serializer

        # Redis client
        self._redis = redis.Redis(host=host, port=port, db=0)

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void flush(self) except *:
        """
        Flush the database which clears all data.

        """
        self._log.debug("Flushing database....")
        self._redis.flushdb()
        self._log.info("Flushed database.")

    cpdef dict load_accounts(self):
        """
        Load all accounts from the execution database.

        Returns
        -------
        Dict[AccountId, Account]

        """
        cdef dict accounts = {}

        cdef list account_keys = self._redis.keys(f"{self.key_accounts}*")
        if not account_keys:
            return accounts

        cdef bytes key_bytes
        cdef AccountId account_id
        cdef Account account
        for key_bytes in account_keys:
            account_id = AccountId.from_string(key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1])
            account = self.load_account(account_id)

            if account:
                accounts[account.id] = account

        return accounts

    cpdef dict load_orders(self):
        """
        Load all orders from the execution database.

        Returns
        -------
        Dict[ClientOrderId, Order]

        """
        cdef dict orders = {}

        cdef list order_keys = self._redis.keys(f"{self.key_orders}*")
        if not order_keys:
            return orders

        cdef bytes key_bytes
        cdef ClientOrderId order_id
        cdef Order order
        for key_bytes in order_keys:
            cl_ord_id = ClientOrderId(key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1])
            order = self.load_order(cl_ord_id)

            if order:
                orders[order.cl_ord_id] = order

        return orders

    cpdef set load_stop_loss_ids(self):
        """
        Load all registered stop-loss identifiers from the execution database.

        """
        cdef set stop_loss_id_members = self._redis.smembers(self.key_index_stop_loss_ids)
        return {ClientOrderId(cl_ord_id.decode(_UTF8)) for cl_ord_id in stop_loss_id_members}

    cpdef set load_take_profit_ids(self):
        """
        Load all registered take-profit identifiers from the execution database.

        """
        cdef set take_profit_id_members = self._redis.smembers(self.key_index_take_profit_ids)
        return {ClientOrderId(cl_ord_id.decode(_UTF8)) for cl_ord_id in take_profit_id_members}

    cpdef dict load_positions(self):
        """
        Load all positions from the execution database.

        Returns
        -------
        Dict[PositionId, Position]

        """
        cdef dict positions = {}

        cdef list position_keys = self._redis.keys(f"{self.key_positions}*")
        if not position_keys:
            return positions

        cdef bytes key_bytes
        cdef PositionId position_id
        cdef Position position
        for key_bytes in position_keys:
            position_id = PositionId(key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1])
            position = self.load_position(position_id)

            if position:
                positions[position.id] = position

        return positions

    cpdef Account load_account(self, AccountId account_id):
        """
        Load the account associated with the given account_id (if found).

        Parameters
        ----------
        account_id : AccountId
            The account identifier to load.

        Returns
        -------
        Account or None

        """
        Condition.not_none(account_id, "account_id")

        cdef list events = self._redis.lrange(name=self.key_accounts + account_id.value, start=0, end=-1)
        if len(events) == 0:
            return None

        cdef bytes event
        cdef Account account = Account(self._event_serializer.deserialize(events[0]))
        for event in events[1:]:
            account.apply(self._event_serializer.deserialize(event))

        return account

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

        cdef list events = self._redis.lrange(name=self.key_orders + cl_ord_id.value, start=0, end=-1)

        # Check there is at least one event to pop
        if len(events) == 0:
            return None

        cdef OrderInitialized initial = self._event_serializer.deserialize(events.pop(0))

        cdef Order order
        if initial.order_type == OrderType.MARKET:
            order = MarketOrder.create(event=initial)
        elif initial.order_type == OrderType.LIMIT:
            order = LimitOrder.create(event=initial)
        elif initial.order_type == OrderType.STOP:
            order = StopOrder.create(event=initial)
        else:
            raise RuntimeError("Invalid order type")

        cdef bytes event_bytes
        for event_bytes in events:
            order.apply(self._event_serializer.deserialize(event_bytes))

        return order

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

        cdef list events = self._redis.lrange(name=self.key_positions + position_id.value, start=0, end=-1)

        # Check there is at least one event to pop
        if len(events) == 0:
            return None

        cdef OrderFilled initial = self._event_serializer.deserialize(events.pop(0))
        cdef Position position = Position(event=initial)

        cdef bytes event_bytes
        for event_bytes in events:
            position.apply(self._event_serializer.deserialize(event_bytes))

        return position

    cpdef dict load_strategy(self, StrategyId strategy_id):
        """
        Load the state for the given strategy.

        Parameters
        ----------
        strategy_id : StrategyId
            The identifier of the strategy state dictionary to load.

        """
        Condition.not_none(strategy_id, "strategy_id")

        return self._redis.hgetall(name=self.key_strategies + strategy_id.value + ":State")

    cpdef void delete_strategy(self, StrategyId strategy_id) except *:
        """
        Delete the given strategy from the execution cache.
        Logs error if strategy not found in the cache.

        Parameters
        ----------
        strategy_id : StrategyId
            The identifier of the strategy state dictionary to delete.

        """
        Condition.not_none(strategy_id, "strategy_id")

        pipe = self._redis.pipeline()
        pipe.delete(self.key_strategies + strategy_id.value)
        pipe.execute()

        self._log.info(f"Deleted {strategy_id.to_string(with_class=True)}.")

    cpdef void add_account(self, Account account) except *:
        """
        Add the given account to the execution cache.

        Parameters
        ----------
        account : Account
            The account to add.

        """
        Condition.not_none(account, "account")

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_accounts + account.id.value, self._event_serializer.serialize(account.last_event()))
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        if reply[0] > 1:  # Reply = The length of the list after the push operation
            self._log.error(f"The {account.id} already existed in the accounts and was appended to.")

        self._log.debug(f"Added Account(id={account.id.value}).")

    cpdef void add_order(self, Order order, PositionId position_id, StrategyId strategy_id) except *:
        """
        Add the given order to the execution cache indexed with the given
        identifiers.

        Parameters
        ----------
        order : Order
            The order to add.
        position_id : PositionId
            The position identifier to index for the order.
        strategy_id : StrategyId
            The strategy identifier to index for the order.

        """
        Condition.not_none(order, "order")
        Condition.not_none(position_id, "position_id")
        Condition.not_none(strategy_id, "strategy_id")

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_orders + order.cl_ord_id.value, self._event_serializer.serialize(order.last_event()))  # 0
        pipe.hset(name=self.key_index_order_strategy, key=order.cl_ord_id.value, value=strategy_id.value)          # 1
        pipe.sadd(self.key_index_orders, order.cl_ord_id.value)                                                    # 2
        pipe.sadd(self.key_index_symbol_orders + order.symbol.value, order.cl_ord_id.value)                        # 3
        pipe.sadd(self.key_index_strategy_orders + strategy_id.value, order.cl_ord_id.value)                       # 4

        if position_id.not_null():
            pipe.hset(name=self.key_index_order_position, key=order.cl_ord_id.value, value=position_id.value)
            pipe.hset(name=self.key_index_position_strategy, key=position_id.value, value=strategy_id.value)
            pipe.sadd(self.key_index_position_orders + position_id.value, order.cl_ord_id.value)
            pipe.sadd(self.key_index_strategy_positions + strategy_id.value, position_id.value)

        cdef list reply = pipe.execute()

        # Check data integrity of reply
        # TODO: Reorganize logging
        # if reply[0] > 1:  # Reply = The length of the list after the push operation
        #     self._log.error(f"The {order.cl_ord_id} already existed in the orders and was appended to.")
        # if reply[1] == 0:  # Reply = 0 if field already exists in the hash and the value was updated
        #     self._log.error(f"The {order.cl_ord_id} already existed in index_order_position and was overwritten.")
        # if reply[2] == 0:  # Reply = 0 if field already exists in the hash and the value was updated
        #     self._log.error(f"The {order.cl_ord_id} already existed in index_order_strategy and was overwritten.")
        # # reply[3] index_position_strategy does not need to be checked as there will be multiple writes for bracket orders
        # if reply[4] == 0:  # Reply = 0 if the element was already a member of the set
        #     self._log.error(f"The {order.cl_ord_id} already existed in index_orders.")
        # if reply[5] == 0:  # Reply = 0 if the element was already a member of the set
        #     self._log.error(f"The {order.cl_ord_id} already existed in index_position_orders.")
        # if reply[6] == 0:  # Reply = 0 if the element was already a member of the set
        #     self._log.error(f"The {order.cl_ord_id} already existed in index_strategy_orders.")
        # reply[7] index_strategy_positions does not need to be checked as there will be multiple writes for bracket orders

    cpdef void add_stop_loss_id(self, ClientOrderId cl_ord_id) except *:
        """
        Register the given client order identifier as a stop-loss.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The identifier to register.

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        self._redis.sadd(self.key_index_stop_loss_ids, cl_ord_id.value)

    cpdef void add_take_profit_id(self, ClientOrderId cl_ord_id) except *:
        """
        Register the given order to be managed as a take-profit.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The identifier to register.

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        self._redis.sadd(self.key_index_take_profit_ids, cl_ord_id.value)

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

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.hset(name=self.key_index_order_position, key=cl_ord_id.value, value=position_id.value)
        pipe.hset(name=self.key_index_position_strategy, key=position_id.value, value=strategy_id.value)
        pipe.sadd(self.key_index_position_orders + position_id.value, cl_ord_id.value)
        pipe.sadd(self.key_index_strategy_positions + strategy_id.value, position_id.value)
        pipe.execute()

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        """
        Add the given position associated with the given strategy identifier.

        Parameters
        ----------
        position : Position
            The position to add.
        strategy_id : StrategyId
            The strategy identifier to associate with the position.

        """
        Condition.not_none(position, "position")
        Condition.not_none(strategy_id, "strategy_id")

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_positions + position.id.value, self._event_serializer.serialize(position.last_event()))
        pipe.sadd(self.key_index_positions, position.id.value)
        pipe.sadd(self.key_index_positions_open, position.id.value)
        pipe.sadd(self.key_index_symbol_positions + position.symbol.value, position.id.value)
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        # TODO: Reorganize logging
        # if reply[0] > 1:  # Reply = The length of the list after the push operation
        #     self._log.error(f"The {position.id} already existed in the index_broker_position and was overwritten.")
        # if reply[1] == 0:  # Reply = 0 if the element was already a member of the set
        #     self._log.error(f"The {position.id} already existed in index_positions.")
        # if reply[2] == 0:  # Reply = 0 if the element was already a member of the set
        #     self._log.error(f"The {position.id} already existed in index_positions.")
        # if reply[3] == 0:  # Reply = 0 if the element was already a member of the set
        #     self._log.error(f"The {position.id} already existed in index_positions_open.")

        self._log.debug(f"Added Position(id={position.id.value}).")

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        """
        Update the given strategy state in the execution cache.

        Parameters
        ----------
        strategy : TradingStrategy
            The strategy to update.

        """
        Condition.not_none(strategy, "strategy")

        cdef dict state = strategy.save()  # Extract state dictionary from strategy

        # Command pipeline
        pipe = self._redis.pipeline()
        for key, value in state.items():
            pipe.hset(name=self.key_strategies + strategy.id.value + ":State", key=key, value=value)
            self._log.debug(f"Saving {strategy.id} state (key='{key}', value={value})...")
        cdef list reply = pipe.execute()

        self._log.info(f"Saved strategy state for {strategy.id.value}.")

    cpdef void update_account(self, Account account) except *:
        """
        Update the given account in the execution cache.

        Parameters
        ----------
        account : The account to update (from last event).

        """
        Condition.not_none(account, "account")

        self._redis.rpush(self.key_accounts + account.id.value, self._event_serializer.serialize(account.last_event()))
        self._log.debug(f"Updated Account(id={account.id}).")

    cpdef void update_order(self, Order order) except *:
        """
        Update the given order in the execution cache.

        Parameters
        ----------
        order : Order
            The order to update (from last event).

        """
        Condition.not_none(order, "order")

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_orders + order.cl_ord_id.value, self._event_serializer.serialize(order.last_event()))
        if order.is_working():
            pipe.sadd(self.key_index_orders_working, order.cl_ord_id.value)
            pipe.srem(self.key_index_orders_completed, order.cl_ord_id.value)
        elif order.is_completed():
            pipe.sadd(self.key_index_orders_completed, order.cl_ord_id.value)
            pipe.srem(self.key_index_orders_working, order.cl_ord_id.value)
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        if reply[0] == 1:  # Reply = The length of the list after the push operation
            self._log.error(f"The updated Order(id={order.cl_ord_id.value}) did not already exist.")

        self._log.debug(f"Updated Order(id={order.cl_ord_id.value}).")

    cpdef void update_position(self, Position position) except *:
        """
        Update the given position in the execution cache.

        Parameters
        ----------
        position : Position
            The position to update (from last event).

        """
        Condition.not_none(position, "position")

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self.key_positions + position.id.value, self._event_serializer.serialize(position.last_event()))
        if position.is_closed():
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
