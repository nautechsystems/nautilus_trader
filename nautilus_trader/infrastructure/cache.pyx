# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import warnings
from typing import Optional

from nautilus_trader.config import CacheDatabaseConfig

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.factory cimport AccountFactory
from nautilus_trader.cache.database cimport CacheDatabase
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.model.c_enums.currency_type cimport CurrencyTypeParser
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.unpacker cimport OrderUnpacker
from nautilus_trader.model.position cimport Position
from nautilus_trader.serialization.base cimport Serializer
from nautilus_trader.trading.strategy cimport Strategy


try:
    import redis
except ImportError:  # pragma: no cover
    redis = None


cdef str _UTF8 = "utf-8"
cdef str _CURRENCIES = "currencies"
cdef str _INSTRUMENTS = "instruments"
cdef str _ACCOUNTS = "accounts"
cdef str _TRADER = "trader"
cdef str _ORDERS = "orders"
cdef str _POSITIONS = "positions"
cdef str _STRATEGIES = "strategies"
cdef str _COMMANDS = "commands"


cdef class RedisCacheDatabase(CacheDatabase):
    """
    Provides a cache database backed by Redis.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the database.
    logger : Logger
        The logger for the database.
    serializer : Serializer
        The serializer for database operations.
    config : CacheDatabaseConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `CacheDatabaseConfig`.

    Warnings
    --------
    Redis can only accurately store int64 types to 17 digits of precision.
    Therefore nanosecond timestamp int64's with 19 digits will lose 2 digits of
    precision when persisted. One way to solve this is to ensure the serializer
    converts timestamp int64's to strings on the way into Redis, and converts
    timestamp strings back to int64's on the way out. One way to achieve this is
    to set the `timestamps_as_str` flag to true for the `MsgPackSerializer`, as
    per the default implementations for both `TradingNode` and `BacktestEngine`.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        Logger logger not None,
        Serializer serializer not None,
        config: Optional[CacheDatabaseConfig] = None,
    ):
        if redis is None:
            warnings.warn("redis is not available.")

        if config is None:
            config = CacheDatabaseConfig()
        Condition.type(config, CacheDatabaseConfig, "config")
        super().__init__(logger)

        # Database keys
        self._key_trader      = f"{_TRADER}-{trader_id}"              # noqa
        self._key_currencies  = f"{self._key_trader}:{_CURRENCIES}:"  # noqa
        self._key_instruments = f"{self._key_trader}:{_INSTRUMENTS}:" # noqa
        self._key_accounts    = f"{self._key_trader}:{_ACCOUNTS}:"    # noqa
        self._key_orders      = f"{self._key_trader}:{_ORDERS}:"      # noqa
        self._key_positions   = f"{self._key_trader}:{_POSITIONS}:"   # noqa
        self._key_strategies  = f"{self._key_trader}:{_STRATEGIES}:"  # noqa
        self._key_commands    = f"{self._key_trader}:{_COMMANDS}:"    # noqa

        # Serializers
        self._serializer = serializer

        # Redis client
        self._redis = redis.Redis(host=config.host, port=config.port, db=0)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void flush(self) except *:
        """
        Flush the database which clears all data.

        """
        self._log.debug("Flushing database....")
        self._redis.flushdb()
        self._log.info("Flushed database.")

    cpdef dict load_currencies(self):
        """
        Load all currencies from the database.

        Returns
        -------
        dict[str, Currency]

        """
        cdef dict currencies = {}

        cdef list currency_keys = self._redis.keys(f"{self._key_currencies}*")
        if not currency_keys:
            return currencies

        cdef bytes key_bytes
        cdef str currency_code
        cdef Currency currency
        for key_bytes in currency_keys:
            currency_code = key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1]
            currency = self.load_currency(currency_code)

            if currency is not None:
                currencies[currency.code] = currency

        return currencies

    cpdef dict load_instruments(self):
        """
        Load all instruments from the database.

        Returns
        -------
        dict[InstrumentId, Instrument]

        """
        cdef dict instruments = {}

        cdef list instrument_keys = self._redis.keys(f"{self._key_instruments}*")
        if not instrument_keys:
            return instruments

        cdef bytes key_bytes
        cdef str key_str
        cdef InstrumentId instrument_id
        cdef Instrument instrument
        for key_bytes in instrument_keys:
            key_str = key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1]
            instrument_id = InstrumentId.from_str_c(key_str)
            instrument = self.load_instrument(instrument_id)

            if instrument is not None:
                instruments[instrument.id] = instrument

        return instruments

    cpdef dict load_accounts(self):
        """
        Load all accounts from the database.

        Returns
        -------
        dict[AccountId, Account]

        """
        cdef dict accounts = {}

        cdef list account_keys = self._redis.keys(f"{self._key_accounts}*")
        if not account_keys:
            return accounts

        cdef bytes key_bytes
        cdef str account_str
        cdef AccountId account_id
        cdef Account account
        for key_bytes in account_keys:
            account_str = key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1]
            account_id = AccountId(account_str)
            account = self.load_account(account_id)

            if account is not None:
                accounts[account.id] = account

        return accounts

    cpdef dict load_orders(self):
        """
        Load all orders from the database.

        Returns
        -------
        dict[ClientOrderId, Order]

        """
        cdef dict orders = {}

        cdef list order_keys = self._redis.keys(f"{self._key_orders}*")
        if not order_keys:
            return orders

        cdef bytes key_bytes
        cdef str key_str
        cdef ClientOrderId client_order_id
        cdef Order order
        for key_bytes in order_keys:
            key_str = key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1]
            client_order_id = ClientOrderId(key_str)
            order = self.load_order(client_order_id)

            if order is not None:
                orders[order.client_order_id] = order

        return orders

    cpdef dict load_positions(self):
        """
        Load all positions from the database.

        Returns
        -------
        dict[PositionId, Position]

        """
        cdef dict positions = {}

        cdef list position_keys = self._redis.keys(f"{self._key_positions}*")
        if not position_keys:
            return positions

        cdef bytes key_bytes
        cdef str key_str
        cdef PositionId position_id
        cdef Position position
        for key_bytes in position_keys:
            key_str = key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1]
            position_id = PositionId(key_str)
            position = self.load_position(position_id)

            if position is not None:
                positions[position.id] = position

        return positions

    cpdef dict load_submit_order_commands(self):
        """
        Load all commands from the database.

        Returns
        -------
        dict[ClientOrderId, SubmitOrder]

        """
        cdef dict commands = {}

        cdef list command_keys = self._redis.keys(f"{self._key_commands}submit_order:*")
        if not command_keys:
            return commands

        cdef bytes key_bytes
        cdef str key_str
        cdef ClientOrderId client_order_id
        cdef SubmitOrder command
        for key_bytes in command_keys:
            key_str = key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1]
            client_order_id = ClientOrderId(key_str)
            command = self.load_submit_order_command(client_order_id)

            if command is not None:
                commands[client_order_id] = command

        return commands

    cpdef SubmitOrder load_submit_order_command(self, ClientOrderId client_order_id):
        """
        Load the command associated with the given client order ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID for the command to load.

        Returns
        -------
        SubmitOrder or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        cdef str key = f"{self._key_commands}submit_order:{client_order_id}"
        cdef bytes command_bytes = self._redis.get(name=key)
        if not command_bytes:
            return None

        return self._serializer.deserialize(command_bytes)

    cpdef Currency load_currency(self, str code):
        """
        Load the currency associated with the given currency code (if found).

        Parameters
        ----------
        code : str
            The currency code to load.

        Returns
        -------
        Currency or ``None``

        """
        Condition.not_none(code, "code")

        cdef dict c_hash = self._redis.hgetall(name=self._key_currencies + code)
        cdef dict c_map = {k.decode('utf-8'): v for k, v in c_hash.items()}
        if not c_map:
            return None

        return Currency(
            code=code,
            precision=int(c_map["precision"]),
            iso4217=int(c_map["iso4217"]),
            name=c_map["name"].decode(_UTF8),
            currency_type=CurrencyTypeParser.from_str(c_map["currency_type"].decode("utf-8")),
        )

    cpdef Instrument load_instrument(self, InstrumentId instrument_id):
        """
        Load the instrument associated with the given instrument ID
        (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.

        Returns
        -------
        Instrument or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef str key = self._key_instruments + instrument_id.to_str()
        cdef bytes instrument_bytes = self._redis.get(name=key)
        if not instrument_bytes:
            return None

        return self._serializer.deserialize(instrument_bytes)

    cpdef Account load_account(self, AccountId account_id):
        """
        Load the account associated with the given account ID (if found).

        Parameters
        ----------
        account_id : AccountId
            The account ID to load.

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(account_id, "account_id")

        cdef list events = self._redis.lrange(
            name=self._key_accounts + account_id.to_str(),
            start=0,
            end=-1,
        )

        # Check there is at least one event to pop
        if not events:
            return None

        cdef bytes event
        cdef Account account = AccountFactory.create_c(self._serializer.deserialize(events[0]))
        for event in events[1:]:
            account.apply(event=self._serializer.deserialize(event))

        return account

    cpdef Order load_order(self, ClientOrderId client_order_id):
        """
        Load the order associated with the given client order ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to load.

        Returns
        -------
        Order or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        cdef list events = self._redis.lrange(
            name=self._key_orders + client_order_id.to_str(),
            start=0,
            end=-1,
        )

        # Check there is at least one event to pop
        if not events:
            return None

        cdef OrderInitialized init = self._serializer.deserialize(events.pop(0))
        cdef Order order = OrderUnpacker.from_init_c(init)

        cdef bytes event_bytes
        for event_bytes in events:
            order.apply(self._serializer.deserialize(event_bytes))

        return order

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

        cdef list events = self._redis.lrange(
            name=self._key_positions + position_id.to_str(),
            start=0,
            end=-1,
        )

        # Check there is at least one event to pop
        if not events:
            return None

        cdef OrderFilled initial_fill = self._serializer.deserialize(events.pop(0))
        cdef Instrument instrument = self.load_instrument(initial_fill.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot load position: "
                f"no instrument found for {initial_fill.instrument_id}",
            )
            return

        cdef Position position = Position(instrument, initial_fill)

        cdef bytes event_bytes
        for event_bytes in events:
            position.apply(self._serializer.deserialize(event_bytes))

        return position

    cpdef dict load_strategy(self, StrategyId strategy_id):
        """
        Load the state for the given strategy.

        Parameters
        ----------
        strategy_id : StrategyId
            The ID of the strategy state dictionary to load.

        Returns
        -------
        dict[str, bytes]

        """
        Condition.not_none(strategy_id, "strategy_id")

        cdef dict user_state = self._redis.hgetall(
            name=self._key_strategies + strategy_id.to_str() + ":State",
        )
        return {k.decode('utf-8'): v for k, v in user_state.items()}

    cpdef void delete_strategy(self, StrategyId strategy_id) except *:
        """
        Delete the given strategy from the database.

        Parameters
        ----------
        strategy_id : StrategyId
            The ID of the strategy state dictionary to delete.

        """
        Condition.not_none(strategy_id, "strategy_id")

        self._redis.delete(self._key_strategies + strategy_id.to_str())

        self._log.info(f"Deleted {repr(strategy_id)}.")

    cpdef void add_currency(self, Currency currency) except *:
        """
        Add the given currency to the database.

        Parameters
        ----------
        currency : Currency
            The currency to add.

        """
        Condition.not_none(currency, "currency")

        cdef dict currency_map = {
            "precision": currency.precision,
            "iso4217": currency.iso4217,
            "name": currency.name,
            "currency_type": CurrencyTypeParser.to_str(currency.currency_type)
        }

        # Command pipeline
        pipe = self._redis.pipeline()
        for key, value in currency_map.items():
            pipe.hset(name=self._key_currencies + currency.code, key=key, value=value)
        pipe.execute()

        self._log.debug(f"Added currency {currency.code}.")

    cpdef void add_instrument(self, Instrument instrument) except *:
        """
        Add the given instrument to the database.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        Condition.not_none(instrument, "instrument")

        cdef str key = self._key_instruments + instrument.id.to_str()
        self._redis.set(name=key, value=self._serializer.serialize(instrument))

        self._log.debug(f"Added instrument {instrument.id}.")

    cpdef void add_account(self, Account account) except *:
        """
        Add the given account to the database.

        Parameters
        ----------
        account : Account
            The account to add.

        """
        Condition.not_none(account, "account")

        # Command pipeline
        pipe = self._redis.pipeline()
        pipe.rpush(self._key_accounts + account.id.to_str(), self._serializer.serialize(account.last_event_c()))
        cdef list reply = pipe.execute()

        # Check data integrity of reply
        if len(reply) > 1:  # Reply = The length of the list after the push operation
            self._log.error(
                f"The {repr(account.id)} already existed and was appended to.",
            )

        self._log.debug(f"Added {account}).")

    cpdef void add_order(self, Order order) except *:
        """
        Add the given order to the database.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        Condition.not_none(order, "order")

        cdef bytes last_event = self._serializer.serialize(order.last_event_c())
        cdef int reply = self._redis.rpush(self._key_orders + order.client_order_id.to_str(), last_event)

        # Check data integrity of reply
        if reply > 1:  # Reply = The length of the list after the push operation
            self._log.warning(
                f"The {repr(order.client_order_id)} already existed and was appended to.",
            )

        self._log.debug(f"Added Order(id={order.client_order_id.to_str()}).")

    cpdef void add_position(self, Position position) except *:
        """
        Add the given position associated with the given strategy ID.

        Parameters
        ----------
        position : Position
            The position to add.

        """
        Condition.not_none(position, "position")

        cdef bytes last_event = self._serializer.serialize(position.last_event_c())
        cdef int reply = self._redis.rpush(self._key_positions + position.id.to_str(), last_event)

        # Check data integrity of reply
        if reply > 1:  # Reply = The length of the list after the push operation
            self._log.warning(
                f"The {repr(position.id)} already existed and was appended to.",
            )

        self._log.debug(f"Added Position(id={position.id.to_str()}).")

    cpdef void add_submit_order_command(self, SubmitOrder command) except *:
        """
        Add the given command to the database.

        Parameters
        ----------
        command : SubmitOrder
            The command to add.

        """
        Condition.not_none(command, "command")

        cdef str key = f"{self._key_commands}submit_order:{command.order.client_order_id.value}"
        cdef bytes command_bytes = self._serializer.serialize(command)
        cdef int reply = self._redis.set(key, command_bytes)

        # Check data integrity of reply
        if reply > 1:  # Reply = The length of the list after the push operation
            self._log.warning(
                f"The {repr(command)} already existed.",
            )

        self._log.debug(f"Added {command}.")

    cpdef void update_strategy(self, Strategy strategy) except *:
        """
        Update the given strategy state in the database.

        Parameters
        ----------
        strategy : Strategy
            The strategy to update.

        """
        Condition.not_none(strategy, "strategy")

        cdef dict state = strategy.save()  # Extract state dictionary from strategy

        # Command pipeline
        pipe = self._redis.pipeline()
        for key, value in state.items():
            pipe.hset(
                name=self._key_strategies + strategy.id.value + ":State",
                key=key,
                value=value,
            )
            self._log.debug(f"Saving {strategy.id} state {{ {key}: {value} }}")
        pipe.execute()

        self._log.debug(f"Saved strategy state for {strategy.id.value}.")

    cpdef void update_account(self, Account account) except *:
        """
        Update the given account in the database.

        Parameters
        ----------
        account : The account to update (from last event).

        """
        Condition.not_none(account, "account")

        cdef bytes serialized_event = self._serializer.serialize(account.last_event_c())
        self._redis.rpush(self._key_accounts + account.id.to_str(), serialized_event)

        self._log.debug(f"Updated {account}.")

    cpdef void update_order(self, Order order) except *:
        """
        Update the given order in the database.

        Parameters
        ----------
        order : Order
            The order to update (from last event).

        """
        Condition.not_none(order, "order")

        cdef bytes serialized_event = self._serializer.serialize(order.last_event_c())
        cdef int reply = self._redis.rpush(self._key_orders + order.client_order_id.to_str(), serialized_event)

        # Check data integrity of reply
        if reply == 1:  # Reply = The length of the list after the push operation
            self._log.error(f"The updated Order(id={order.client_order_id.to_str()}) did not already exist.")

        self._log.debug(f"Updated {order}.")

    cpdef void update_position(self, Position position) except *:
        """
        Update the given position in the database.

        Parameters
        ----------
        position : Position
            The position to update (from last event).

        """
        Condition.not_none(position, "position")

        cdef bytes serialized_event = self._serializer.serialize(position.last_event_c())
        cdef int reply = self._redis.rpush(self._key_positions + position.id.to_str(), serialized_event)

        self._log.debug(f"Updated {position}.")
