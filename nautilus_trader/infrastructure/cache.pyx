# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.factory cimport AccountFactory
from nautilus_trader.cache.database cimport CacheDatabase
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.enums_c cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.enums_c cimport currency_type_from_str
from nautilus_trader.model.enums_c cimport currency_type_to_str
from nautilus_trader.model.enums_c cimport order_type_to_str
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.unpacker cimport OrderUnpacker
from nautilus_trader.model.position cimport Position
from nautilus_trader.serialization.base cimport Serializer
from nautilus_trader.trading.strategy cimport Strategy


try:
    import redis
except ImportError:  # pragma: no cover
    redis = None


cdef str _UTF8 = "utf-8"
cdef str _GENERAL = "general"
cdef str _CURRENCIES = "currencies"
cdef str _INSTRUMENTS = "instruments"
cdef str _SYNTHETICS = "synthetics"
cdef str _ACCOUNTS = "accounts"
cdef str _TRADER = "trader"
cdef str _ORDERS = "orders"
cdef str _POSITIONS = "positions"
cdef str _ACTORS = "actors"
cdef str _STRATEGIES = "strategies"

cdef str _INDEX_ORDER_IDS = "index:order_ids"
cdef str _INDEX_ORDER_POSITION = "index:order_position"
cdef str _INDEX_ORDER_CLIENT = "index:order_client"
cdef str _INDEX_ORDERS = "index:orders"
cdef str _INDEX_ORDERS_OPEN = "index:orders_open"
cdef str _INDEX_ORDERS_CLOSED = "index:orders_closed"
cdef str _INDEX_ORDERS_EMULATED = "index:orders_emulated"
cdef str _INDEX_ORDERS_INFLIGHT = "index:orders_inflight"
cdef str _INDEX_POSITIONS = "index:positions"
cdef str _INDEX_POSITIONS_OPEN = "index:positions_open"
cdef str _INDEX_POSITIONS_CLOSED = "index:positions_closed"

cdef str _SNAPSHOTS_ORDERS = "snapshots:orders"
cdef str _SNAPSHOTS_POSITIONS = "snapshots:positions"
cdef str _HEARTBEAT = "health:heartbeat"


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
        super().__init__(logger, config)

        # Database keys
        self._key_trader      = f"{_TRADER}-{trader_id}"              # noqa
        self._key_general     = f"{self._key_trader}:{_GENERAL}:"     # noqa
        self._key_currencies  = f"{self._key_trader}:{_CURRENCIES}:"  # noqa
        self._key_instruments = f"{self._key_trader}:{_INSTRUMENTS}:" # noqa
        self._key_synthetics  = f"{self._key_trader}:{_SYNTHETICS}:"  # noqa
        self._key_accounts    = f"{self._key_trader}:{_ACCOUNTS}:"    # noqa
        self._key_orders      = f"{self._key_trader}:{_ORDERS}:"      # noqa
        self._key_positions   = f"{self._key_trader}:{_POSITIONS}:"   # noqa
        self._key_actors      = f"{self._key_trader}:{_ACTORS}:"      # noqa
        self._key_strategies  = f"{self._key_trader}:{_STRATEGIES}:"  # noqa

        self._key_index_order_ids = f"{self._key_trader}:{_INDEX_ORDER_IDS}:"
        self._key_index_order_position = f"{self._key_trader}:{_INDEX_ORDER_POSITION}:"
        self._key_index_order_client = f"{self._key_trader}:{_INDEX_ORDER_CLIENT}:"
        self._key_index_orders = f"{self._key_trader}:{_INDEX_ORDERS}"
        self._key_index_orders_open = f"{self._key_trader}:{_INDEX_ORDERS_OPEN}"
        self._key_index_orders_closed = f"{self._key_trader}:{_INDEX_ORDERS_CLOSED}"
        self._key_index_orders_emulated = f"{self._key_trader}:{_INDEX_ORDERS_EMULATED}"
        self._key_index_orders_inflight = f"{self._key_trader}:{_INDEX_ORDERS_INFLIGHT}"
        self._key_index_positions = f"{self._key_trader}:{_INDEX_POSITIONS}"
        self._key_index_positions_open = f"{self._key_trader}:{_INDEX_POSITIONS_OPEN}"
        self._key_index_positions_closed = f"{self._key_trader}:{_INDEX_POSITIONS_CLOSED}"

        self._key_snapshots_orders = f"{self._key_trader}:{_SNAPSHOTS_ORDERS}:"
        self._key_snapshots_positions = f"{self._key_trader}:{_SNAPSHOTS_POSITIONS}:"
        self._key_heartbeat = f"{self._key_trader}:{_HEARTBEAT}"

        # Serializers
        self._serializer = serializer

        # Redis client
        self._redis = redis.Redis(
            host=config.host,
            port=config.port or 6379,
            db=0,
            username=config.username,
            password=config.password,
            ssl=config.ssl,
        )

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void flush(self):
        """
        Flush the database which clears all data.

        """
        self._log.debug("Flushing database....")
        self._redis.flushdb()
        self._log.info("Flushed database.", LogColor.BLUE)

    cpdef dict load(self):
        """
        Load all general objects from the database.

        Returns
        -------
        dict[str, bytes]

        """
        cdef dict general = {}

        cdef list general_keys = self._redis.keys(f"{self._key_general}*")
        if not general_keys:
            return general

        cdef bytes key_bytes
        cdef bytes value_bytes
        cdef str key
        for key_bytes in general_keys:
            value_bytes = self._redis.get(name=key_bytes)
            if value_bytes is not None:
                key = key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1]
                general[key] = value_bytes

        return general

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

    cpdef dict load_synthetics(self):
        """
        Load all synthetic instruments from the database.

        Returns
        -------
        dict[InstrumentId, SyntheticInstrument]

        """
        cdef dict synthetics = {}

        cdef list synthetic_keys = self._redis.keys(f"{self._key_synthetics}*")
        if not synthetic_keys:
            return synthetics

        cdef bytes key_bytes
        cdef str key_str
        cdef InstrumentId instrument_id
        cdef SyntheticInstrument synthetic
        for key_bytes in synthetic_keys:
            key_str = key_bytes.decode(_UTF8).rsplit(':', maxsplit=1)[1]
            instrument_id = InstrumentId.from_str_c(key_str)
            synthetic = self.load_synthetic(instrument_id)

            if synthetic is not None:
                synthetics[synthetic.id] = synthetic

        return synthetics

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

    cpdef dict load_index_order_position(self):
        """
        Load the order to position index from the database.

        Returns
        -------
        dict[ClientOrderId, PositionId]

        """
        cdef dict raw_index = self._redis.hgetall(self._key_index_order_position)

        return {ClientOrderId(k.decode("utf-8")): PositionId(v.decode("utf-8")) for k, v in raw_index.items()}

    cpdef dict load_index_order_client(self):
        """
        Load the order to execution client index from the database.

        Returns
        -------
        dict[ClientOrderId, ClientId]

        """
        cdef dict raw_index = self._redis.hgetall(self._key_index_order_client)

        return {ClientOrderId(k.decode("utf-8")): ClientId(v.decode("utf-8")) for k, v in raw_index.items()}

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
            currency_type=currency_type_from_str(c_map["currency_type"].decode("utf-8")),
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

    cpdef SyntheticInstrument load_synthetic(self, InstrumentId instrument_id):
        """
        Load the synthetic instrument associated with the given synthetic instrument ID
        (if found).

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
            If `instrument_id` is not for a synthetic instrument.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(instrument_id.is_synthetic(), "instrument_id was not for a synthetic instrument")

        cdef str key = self._key_synthetics + instrument_id.to_str()
        cdef bytes synthetic_bytes = self._redis.get(name=key)
        if not synthetic_bytes:
            return None

        return self._serializer.deserialize(synthetic_bytes)

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

        cdef int event_count = 0
        cdef bytes event_bytes
        cdef OrderEvent event
        for event_bytes in events:
            event = self._serializer.deserialize(event_bytes)

            # Check event integrity
            if event in order._events:
                raise RuntimeError(f"Corrupt cache with duplicate event for order {event}")

            if event_count > 0 and isinstance(event, OrderInitialized):
                if event.order_type == OrderType.MARKET:
                    order = MarketOrder.transform(order, event.ts_init)
                elif event.order_type == OrderType.LIMIT:
                    price = Price.from_str_c(event.options["price"])
                    order = LimitOrder.transform(order, event.ts_init, price)
                else:
                    raise RuntimeError(  # pragma: no cover (design-time error)
                        f"Cannot transform order to {order_type_to_str(event.order_type)}",  # pragma: no cover (design-time error)
                    )
            else:
                order.apply(event)
            event_count += 1

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

        cdef:
            bytes event_bytes
            OrderFilled fill
        for event_bytes in events:
            event = self._serializer.deserialize(event_bytes)

            # Check event integrity
            if event in position._events:
                raise RuntimeError(f"Corrupt cache with duplicate event for position {event}")

            position.apply(event)

        return position

    cpdef dict load_actor(self, ComponentId component_id):
        """
        Load the state for the given actor.

        Parameters
        ----------
        component_id : ComponentId
            The ID of the actor state dictionary to load.

        Returns
        -------
        dict[str, bytes]

        """
        Condition.not_none(component_id, "component_id")

        cdef dict user_state = self._redis.hgetall(
            name=self._key_actors + component_id.to_str() + ":state",
        )
        return {k.decode('utf-8'): v for k, v in user_state.items()}

    cpdef void delete_actor(self, ComponentId component_id):
        """
        Delete the given actor from the database.

        Parameters
        ----------
        component_id : ComponentId
            The ID of the actor state dictionary to delete.

        """
        Condition.not_none(component_id, "component_id")

        self._redis.delete(self._key_actors + component_id.to_str() + ":state")

        self._log.info(f"Deleted {repr(component_id)}.")

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
            name=self._key_strategies + strategy_id.to_str() + ":state",
        )
        return {k.decode('utf-8'): v for k, v in user_state.items()}

    cpdef void delete_strategy(self, StrategyId strategy_id):
        """
        Delete the given strategy from the database.

        Parameters
        ----------
        strategy_id : StrategyId
            The ID of the strategy state dictionary to delete.

        """
        Condition.not_none(strategy_id, "strategy_id")

        self._redis.delete(self._key_strategies + strategy_id.to_str() + ":state")

        self._log.info(f"Deleted {repr(strategy_id)}.")

    cpdef void add(self, str key, bytes value):
        """
        Add the given general object value to the database.

        Parameters
        ----------
        key : str
            The key to write to.
        value : bytes
            The object value.

        """
        Condition.not_none(key, "key")
        Condition.not_none(value, "value")

        self._redis.set(name=self._key_general + key, value=value)
        self._log.debug(f"Added general object {key}.")

    cpdef void add_currency(self, Currency currency):
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
            "currency_type": currency_type_to_str(currency.currency_type)
        }

        # Command pipeline
        pipe = self._redis.pipeline()
        for key, value in currency_map.items():
            pipe.hset(name=self._key_currencies + currency.code, key=key, value=value)
        pipe.execute()

        self._log.debug(f"Added currency {currency.code}.")

    cpdef void add_instrument(self, Instrument instrument):
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

    cpdef void add_synthetic(self, SyntheticInstrument synthetic):
        """
        Add the given synthetic instrument to the database.

        Parameters
        ----------
        synthetic : SyntheticInstrument
            The synthetic instrument to add.

        """
        Condition.not_none(synthetic, "synthetic")

        cdef str key = self._key_synthetics + synthetic.id.value
        self._redis.set(name=key, value=self._serializer.serialize(synthetic))

        self._log.debug(f"Added synthetic instrument {synthetic.id}.")

    cpdef void add_account(self, Account account):
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

        self._log.debug(f"Added {account}.")

    cpdef void add_order(self, Order order, PositionId position_id = None, ClientId client_id = None):
        """
        Add the given order to the database.

        Parameters
        ----------
        order : Order
            The order to add.
        position_id : PositionId, optional
            The position ID to associate with this order.
        client_id : ClientId, optional
            The execution client ID to associate with this order.

        """
        Condition.not_none(order, "order")

        cdef bytes last_event = self._serializer.serialize(order.last_event_c())
        cdef int reply = self._redis.rpush(self._key_orders + order.client_order_id.to_str(), last_event)

        # Check data integrity of reply
        if reply > 1:  # Reply = The length of the list after the push operation
            # Dropped the log level to debug as this is expected for transformed orders
            self._log.debug(
                f"The {repr(order.client_order_id)} already existed and was appended to.",
            )

        self._redis.sadd(self._key_index_orders, order.client_order_id.to_str())

        if order.emulation_trigger != TriggerType.NO_TRIGGER:
            self._redis.sadd(self._key_index_orders_emulated, order.client_order_id.to_str())

        self._log.debug(f"Added {order}.")

        if position_id is not None:
            self.index_order_position(order.client_order_id, position_id)
        if client_id is not None:
            self._redis.hset(self._key_index_order_client, order.client_order_id.to_str(), client_id.to_str())
            self._log.debug(f"Indexed {order.client_order_id!r} -> {client_id!r}")

    cpdef void add_position(self, Position position):
        """
        Add the given position to the database.

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

        self._redis.sadd(self._key_index_positions, position.id.to_str())
        self._redis.sadd(self._key_index_positions_open, position.id.to_str())

        self._log.debug(f"Added {position}.")

    cpdef void index_venue_order_id(self, ClientOrderId client_order_id, VenueOrderId venue_order_id):
        """
        Add an index entry for the given `venue_order_id` to `client_order_id`.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to index.
        venue_order_id : VenueOrderId
            The venue order ID to index.

        """
        Condition.not_none(client_order_id, "client_order_id")
        Condition.not_none(venue_order_id, "venue_order_id")

        self._redis.hset(
            self._key_index_order_ids,
            client_order_id.to_str(),
            venue_order_id.to_str(),
        )

        self._log.debug(f"Indexed {client_order_id!r} -> {venue_order_id!r}")

    cpdef void index_order_position(self, ClientOrderId client_order_id, PositionId position_id):
        """
        Add an index entry for the given `client_order_id` to `position_id`.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to index.
        position_id : PositionId
            The position ID to index.

        """
        Condition.not_none(client_order_id, "client_order_id")
        Condition.not_none(position_id, "position_id")

        self._redis.hset(
            self._key_index_order_position,
            client_order_id.to_str(),
            position_id.to_str(),
        )

        self._log.debug(f"Indexed {client_order_id!r} -> {position_id!r}")

    cpdef void update_actor(self, Actor actor):
        """
        Update the given actor state in the database.

        Parameters
        ----------
        actor : Actor
            The actor to update.

        """
        Condition.not_none(actor, "actor")

        cdef dict state = actor.save()  # Extract state dictionary from strategy

        # Command pipeline
        pipe = self._redis.pipeline()
        for key, value in state.items():
            pipe.hset(
                name=self._key_actors + actor.id.value + ":state",
                key=key,
                value=value,
            )
            self._log.debug(f"Saving {actor.id} state {{ {key}: {value} }}")
        pipe.execute()

        self._log.debug(f"Saved actor state for {actor.id.value}.")

    cpdef void update_strategy(self, Strategy strategy):
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
                name=self._key_strategies + strategy.id.value + ":state",
                key=key,
                value=value,
            )
            self._log.debug(f"Saving {strategy.id} state {{ {key}: {value} }}")
        pipe.execute()

        self._log.debug(f"Saved strategy state for {strategy.id.value}.")

    cpdef void update_account(self, Account account):
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

    cpdef void update_order(self, Order order):
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

        if order.venue_order_id is not None:
            # Assumes order_id does not change
            self.index_venue_order_id(order.client_order_id, order.venue_order_id)

        # Update in-flight state
        if order.is_inflight_c():
            self._redis.sadd(self._key_index_orders_inflight, order.client_order_id.to_str())
        else:
            self._redis.srem(self._key_index_orders_inflight, order.client_order_id.to_str())

        # Update open/closed state
        if order.is_open_c():
            self._redis.srem(self._key_index_orders_closed, order.client_order_id.to_str())
            self._redis.sadd(self._key_index_orders_open, order.client_order_id.to_str())
        elif order.is_closed_c():
            self._redis.srem(self._key_index_orders_open, order.client_order_id.to_str())
            self._redis.sadd(self._key_index_orders_closed, order.client_order_id.to_str())

        # Update emulation state
        if order.emulation_trigger == TriggerType.NO_TRIGGER:
            self._redis.srem(self._key_index_orders_emulated, order.client_order_id.to_str())
        else:
            self._redis.sadd(self._key_index_orders_emulated, order.client_order_id.to_str())

        self._log.debug(f"Updated {order}.")

    cpdef void update_position(self, Position position):
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

        if position.is_open_c():
            self._redis.sadd(self._key_index_positions_open, position.id.to_str())
            self._redis.srem(self._key_index_positions_closed, position.id.to_str())
        elif position.is_closed_c():
            self._redis.sadd(self._key_index_positions_closed, position.id.to_str())
            self._redis.srem(self._key_index_positions_open, position.id.to_str())

        self._log.debug(f"Updated {position}.")

    cpdef void snapshot_order_state(self, Order order):
        """
        Snapshot the state of the given `order`.

        Parameters
        ----------
        order : Order
            The order for the state snapshot.

        """
        Condition.not_none(order, "order")

        cdef dict order_state = order.to_dict()
        cdef bytes snapshot_bytes = self._serializer.serialize(order_state)

        self._redis.rpush(self._key_snapshots_orders + order.client_order_id.to_str(), snapshot_bytes)

        self._log.debug(f"Added state snapshot {order}.")

    cpdef void snapshot_position_state(self, Position position, uint64_t ts_snapshot, Money unrealized_pnl = None):
        """
        Snapshot the state of the given `position`.

        Parameters
        ----------
        position : Position
            The position for the state snapshot.
        ts_snapshot : uint64_t
            The UNIX timestamp (nanoseconds) when the snapshot was taken.
        unrealized_pnl : Money, optional
            The unrealized PnL for the state snapshot.

        """
        Condition.not_none(position, "position")

        cdef dict position_state = position.to_dict()

        if unrealized_pnl is not None:
            position_state["unrealized_pnl"] = unrealized_pnl.to_str()

        position_state["ts_snapshot"] = ts_snapshot

        cdef bytes snapshot_bytes = self._serializer.serialize(position_state)
        self._redis.rpush(self._key_snapshots_positions + position.id.to_str(), snapshot_bytes)

        self._log.debug(f"Added state snapshot {position}.")

    cpdef void heartbeat(self, datetime timestamp):
        """
        Add a heartbeat at the given `timestamp`.

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the heartbeat.

        """
        Condition.not_none(timestamp, "timestamp")

        cdef timestamp_str = format_iso8601(timestamp)
        self._redis.set(self._key_heartbeat, timestamp_str)

        self._log.debug(f"Set last heartbeat {timestamp_str}.")
