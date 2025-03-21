# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.cache.transformers import transform_account_from_pyo3
from nautilus_trader.cache.transformers import transform_currency_from_pyo3
from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.cache.transformers import transform_order_from_pyo3
from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.core import nautilus_pyo3

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.factory cimport AccountFactory
from nautilus_trader.cache.facade cimport CacheDatabaseFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.functions cimport currency_type_from_str
from nautilus_trader.model.functions cimport currency_type_to_str
from nautilus_trader.model.functions cimport order_type_to_str
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
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.unpacker cimport OrderUnpacker
from nautilus_trader.model.position cimport Position
from nautilus_trader.serialization.base cimport Serializer
from nautilus_trader.trading.strategy cimport Strategy


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


cdef class CacheDatabaseAdapter(CacheDatabaseFacade):
    """
    Provides a generic cache database adapter.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the adapter.
    instance_id : UUID4
        The instance ID for the adapter.
    serializer : Serializer
        The serializer for database operations.
    config : CacheConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `CacheConfig`.

    Warnings
    --------
    Redis can only accurately store int64 types to 17 digits of precision.
    Therefore nanosecond timestamp int64's with 19 digits will lose 2 digits of
    precision when persisted. One way to solve this is to ensure the serializer
    converts timestamp int64's to strings on the way into Redis, and converts
    timestamp strings back to int64's on the way out. One way to achieve this is
    to set the `timestamps_as_str` flag to true for the `MsgSpecSerializer`, as
    per the default implementations for both `TradingNode` and `BacktestEngine`.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        UUID4 instance_id not None,
        Serializer serializer not None,
        config: CacheConfig | None = None,
    ) -> None:
        if config is None:
            config = CacheConfig()
        Condition.type(config, CacheConfig, "config")
        super().__init__(config)

        # Validate configuration
        if config.buffer_interval_ms and config.buffer_interval_ms > 1000:
            self._log.warning(
                f"High `buffer_interval_ms` at {config.buffer_interval_ms}, "
                "recommended range is [10, 1000] milliseconds",
            )

        # Configuration
        self._log.info(f"{config.database=}", LogColor.BLUE)
        self._log.info(f"{config.encoding=}", LogColor.BLUE)
        self._log.info(f"{config.timestamps_as_iso8601=}", LogColor.BLUE)
        self._log.info(f"{config.buffer_interval_ms=}", LogColor.BLUE)
        self._log.info(f"{config.flush_on_start=}", LogColor.BLUE)
        self._log.info(f"{config.use_trader_prefix=}", LogColor.BLUE)
        self._log.info(f"{config.use_instance_id=}", LogColor.BLUE)

        self._serializer = serializer

        self._backing = nautilus_pyo3.RedisCacheDatabase(
            trader_id=nautilus_pyo3.TraderId(trader_id.value),
            instance_id=nautilus_pyo3.UUID4.from_str(instance_id.value),
            config_json=msgspec.json.encode(config, enc_hook=msgspec_encoding_hook),
        )

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void close(self):
        """
        Close the backing database adapter.

        """
        self._log.debug("Closing cache database adapter")
        self._backing.close()
        self._log.info("Closed cache database adapter")

    cpdef void flush(self):
        """
        Flush the database which clears all data.

        """
        self._log.debug("Flushing cache database")
        self._backing.flushdb()
        self._log.info("Flushed cache database", LogColor.BLUE)

    cpdef list[str] keys(self, str pattern = "*"):
        """
        Return all keys in the database matching the given `pattern`.

        Parameters
        ----------
        pattern : str, default '*'
            The glob-style pattern to match against the keys in the database.

        Returns
        -------
        list[str]

        Raises
        ------
        ValueError
            If `pattern` is not a valid string.

        Warnings
        --------
        Using the default '*' pattern string can have serious performance implications and
        can take a long time to execute if many keys exist in the database. This operation
        can lead to high memory and CPU usage, and should be used with caution, especially
        in production environments.

        """
        Condition.valid_string(pattern, "pattern")

        return self._backing.keys(pattern)

    cpdef dict load_all(self):
        """
        Load all cache data from the database.

        Returns
        -------
        dict[str, dict]
            A dictionary containing all cache data organized by category.

        """
        cdef dict raw_data = self._backing.load_all()
        cdef dict result = {}

        cdef dict currencies_dict = raw_data.get("currencies", {})
        cdef dict instruments_dict = raw_data.get("instruments", {})
        cdef dict synthetics_dict = raw_data.get("synthetics", {})
        cdef dict accounts_dict = raw_data.get("accounts", {})
        cdef dict orders_dict = raw_data.get("orders", {})
        cdef dict positions_dict = raw_data.get("positions", {})

        result["currencies"] = {
            key: transform_currency_from_pyo3(value) for key, value in currencies_dict.items()
        }
        result["instruments"] = {
            key: transform_instrument_from_pyo3(value) for key, value in instruments_dict.items()
        }
        result["synthetics"] = synthetics_dict
        result["accounts"] = {
            key: transform_account_from_pyo3(value) for key, value in accounts_dict.items()
        }
        result["orders"] = {
            key: transform_order_from_pyo3(value) for key, value in orders_dict.items()
        }
        result["positions"] = positions_dict

        return result

    cpdef dict load(self):
        """
        Load all general objects from the database.

        Returns
        -------
        dict[str, bytes]

        """
        cdef dict general = {}

        cdef list general_keys = self._backing.keys(f"{_GENERAL}:*")
        if not general_keys:
            return general

        cdef:
            str key
            list result
            bytes value_bytes
        for key in general_keys:
            key = key.split(':', maxsplit=1)[1]
            result = self._backing.read(key)
            value_bytes = result[0]
            if value_bytes is not None:
                key = key.split(':', maxsplit=1)[1]
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

        cdef list currency_keys = self._backing.keys(f"{_CURRENCIES}*")
        if not currency_keys:
            return currencies

        cdef:
            str key
            str currency_code
            Currency currency
        for key in currency_keys:
            currency_code = key.rsplit(':', maxsplit=1)[1]
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

        cdef list instrument_keys = self._backing.keys(f"{_INSTRUMENTS}*")
        if not instrument_keys:
            return instruments

        cdef:
            str key
            InstrumentId instrument_id
            Instrument instrument
        for key in instrument_keys:
            instrument_id = InstrumentId.from_str_c(key.rsplit(':', maxsplit=1)[1])
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

        cdef list synthetic_keys = self._backing.keys(f"{_SYNTHETICS}*")
        if not synthetic_keys:
            return synthetics

        cdef:
            str key
            InstrumentId instrument_id
            SyntheticInstrument synthetic
        for key in synthetic_keys:
            instrument_id = InstrumentId.from_str_c(key.rsplit(':', maxsplit=1)[1])
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

        cdef list account_keys = self._backing.keys(f"{_ACCOUNTS}*")
        if not account_keys:
            return accounts

        cdef:
            str key
            str account_str
            AccountId account_id
            Account account
        for key in account_keys:
            account_id = AccountId(key.rsplit(':', maxsplit=1)[1])
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

        cdef list order_keys = self._backing.keys(f"{_ORDERS}*")
        if not order_keys:
            return orders

        cdef:
            str key
            ClientOrderId client_order_id
            Order order
        for key in order_keys:
            client_order_id = ClientOrderId(key.rsplit(':', maxsplit=1)[1])
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

        cdef list position_keys = self._backing.keys(f"{_POSITIONS}*")
        if not position_keys:
            return positions

        cdef:
            str key
            PositionId position_id
            Position position
        for key in position_keys:
            position_id = PositionId(key.rsplit(':', maxsplit=1)[1])
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
        cdef list result = self._backing.read(_INDEX_ORDER_POSITION)
        if not result:
            return {}

        cdef dict raw_index = msgspec.json.decode(result[0])
        return {ClientOrderId(k): PositionId(v) for k, v in raw_index.items()}

    cpdef dict load_index_order_client(self):
        """
        Load the order to execution client index from the database.

        Returns
        -------
        dict[ClientOrderId, ClientId]

        """
        cdef list result = self._backing.read(_INDEX_ORDER_CLIENT)
        if not result:
            return {}

        cdef dict raw_index = msgspec.json.decode(result[0])
        return {ClientOrderId(k): ClientId(v) for k, v in raw_index.items()}

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

        cdef str key = f"{_CURRENCIES}:{code}"
        cdef list result = self._backing.read(key)

        if not result:
            return None

        cdef dict c_map = self._serializer.deserialize(result[0])

        return Currency(
            code=code,
            precision=int(c_map["precision"]),
            iso4217=int(c_map["iso4217"]),
            name=c_map["name"],
            currency_type=currency_type_from_str(c_map["currency_type"]),
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

        cdef str key = f"{_INSTRUMENTS}:{instrument_id.to_str()}"
        cdef list result = self._backing.read(key)
        if not result:
            return None

        cdef bytes instrument_bytes = result[0]

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
        Condition.is_true(instrument_id.is_synthetic(), "instrument_id was not for a synthetic instrument")

        cdef str key = f"{_SYNTHETICS}:{instrument_id.to_str()}"
        cdef list result = self._backing.read(key)
        if not result:
            return None

        cdef bytes synthetic_bytes = result[0]

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

        cdef str key = f"{_ACCOUNTS}:{account_id.to_str()}"
        cdef list result = self._backing.read(key)
        if not result:
            return None

        cdef bytes initial_event = result.pop(0)
        cdef Account account = AccountFactory.create_c(self._serializer.deserialize(initial_event))

        cdef bytes event
        for event in result:
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

        cdef str key = f"{_ORDERS}:{client_order_id.to_str()}"
        cdef list result = self._backing.read(key)

        # Check there is at least one event to pop
        if not result:
            return None

        cdef OrderInitialized init = self._serializer.deserialize(result.pop(0))
        cdef Order order = OrderUnpacker.from_init_c(init)

        cdef int event_count = 0
        cdef bytes event_bytes
        cdef OrderEvent event
        for event_bytes in result:
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

        cdef str key = f"{_POSITIONS}:{position_id.to_str()}"
        cdef list result = self._backing.read(key)

        # Check there is at least one event to pop
        if not result:
            return None

        cdef OrderFilled initial_fill = self._serializer.deserialize(result.pop(0))
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
        for event_bytes in result:
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
        dict[str, Any]

        """
        Condition.not_none(component_id, "component_id")

        cdef str key = f"{_ACTORS}:{component_id.to_str()}:state"
        cdef list result = self._backing.read(key)
        if not result:
            return {}

        return self._serializer.deserialize(result[0])

    cpdef void delete_actor(self, ComponentId component_id):
        """
        Delete the given actor from the database.

        Parameters
        ----------
        component_id : ComponentId
            The ID of the actor state dictionary to delete.

        """
        Condition.not_none(component_id, "component_id")

        cdef str key = f"{_ACTORS}:{component_id.to_str()}:state"
        self._backing.delete(key)

        self._log.info(f"Deleted {repr(component_id)}")

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

        cdef str key = f"{_STRATEGIES}:{strategy_id.to_str()}:state"
        cdef list result = self._backing.read(key)
        if not result:
            return {}

        return self._serializer.deserialize(result[0])

    cpdef void delete_strategy(self, StrategyId strategy_id):
        """
        Delete the given strategy from the database.

        Parameters
        ----------
        strategy_id : StrategyId
            The ID of the strategy state dictionary to delete.

        """
        Condition.not_none(strategy_id, "strategy_id")

        cdef str key = f"{_STRATEGIES}:{strategy_id.to_str()}:state"
        self._backing.delete(key)

        self._log.info(f"Deleted {repr(strategy_id)}")

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

        self._backing.insert(f"{_GENERAL}:{key}", [value])
        self._log.debug(f"Added general object {key}")

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

        cdef key = f"{_CURRENCIES}:{currency.code}"
        cdef list payload = [self._serializer.serialize(currency_map)]
        self._backing.insert(key, payload)

        self._log.debug(f"Added currency {currency.code}")

    cpdef void add_instrument(self, Instrument instrument):
        """
        Add the given instrument to the database.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        Condition.not_none(instrument, "instrument")

        cdef str key = f"{_INSTRUMENTS}:{instrument.id.to_str()}"
        cdef list payload = [self._serializer.serialize(instrument)]
        self._backing.insert(key, payload)

        self._log.debug(f"Added instrument {instrument.id}")

    cpdef void add_synthetic(self, SyntheticInstrument synthetic):
        """
        Add the given synthetic instrument to the database.

        Parameters
        ----------
        synthetic : SyntheticInstrument
            The synthetic instrument to add.

        """
        Condition.not_none(synthetic, "synthetic")

        cdef str key = f"{_SYNTHETICS}:{synthetic.id.value}"
        cdef list payload = [self._serializer.serialize(synthetic)]
        self._backing.insert(key, payload)

        self._log.debug(f"Added synthetic instrument {synthetic.id}")

    cpdef void add_account(self, Account account):
        """
        Add the given account to the database.

        Parameters
        ----------
        account : Account
            The account to add.

        """
        Condition.not_none(account, "account")

        cdef str key = f"{_ACCOUNTS}:{account.id.value}"
        cdef list payload = [self._serializer.serialize(account.last_event_c())]
        self._backing.insert(key, payload)

        self._log.debug(f"Added {account}")

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

        cdef client_order_id_str = order.client_order_id.to_str()
        cdef str key = f"{_ORDERS}:{client_order_id_str}"
        cdef list payload = [self._serializer.serialize(order.last_event_c())]
        self._backing.insert(key, payload)

        cdef bytes client_order_id_bytes = client_order_id_str.encode()
        payload = [client_order_id_bytes]
        self._backing.insert(_INDEX_ORDERS, payload)

        if order.emulation_trigger != TriggerType.NO_TRIGGER:
            self._backing.insert(_INDEX_ORDERS_EMULATED, payload)

        self._log.debug(f"Added {order}")

        if position_id is not None:
            self.index_order_position(order.client_order_id, position_id)
        if client_id is not None:
            payload = [client_order_id_bytes, client_id.to_str().encode()]
            self._backing.insert(_INDEX_ORDER_CLIENT, payload)
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

        cdef str position_id_str = position.id.to_str()
        cdef str key = f"{_POSITIONS}:{position_id_str}"
        cdef list payload = [self._serializer.serialize(position.last_event_c())]
        self._backing.insert(key, payload)

        cdef bytes position_id_bytes = position_id_str.encode()
        self._backing.insert(_INDEX_POSITIONS, [position_id_bytes])
        self._backing.insert(_INDEX_POSITIONS_OPEN, [position_id_bytes])

        self._log.debug(f"Added {position}")

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

        cdef list payload = [client_order_id.to_str().encode(), venue_order_id.to_str().encode()]
        self._backing.insert(_INDEX_ORDER_IDS, payload)

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

        cdef list payload = [client_order_id.to_str().encode(), position_id.to_str().encode()]
        self._backing.insert(_INDEX_ORDER_POSITION, payload)

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

        cdef key = f"{_ACTORS}:{actor.id.value}:state"
        cdef list payload = [self._serializer.serialize(state)]
        self._backing.insert(key, payload)

        self._log.debug(f"Saved actor state for {actor.id.value}")

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

        cdef key = f"{_STRATEGIES}:{strategy.id.value}:state"
        cdef list payload = [self._serializer.serialize(state)]
        self._backing.insert(key, payload)

        self._log.debug(f"Saved strategy state for {strategy.id.value}")

    cpdef void update_account(self, Account account):
        """
        Update the given account in the database.

        Parameters
        ----------
        account : The account to update (from last event).

        """
        Condition.not_none(account, "account")

        cdef str key = f"{_ACCOUNTS}:{account.id.to_str()}"
        cdef list payload = [self._serializer.serialize(account.last_event_c())]
        self._backing.update(key, payload)

        self._log.debug(f"Updated {account}")

    cpdef void update_order(self, Order order):
        """
        Update the given order in the database.

        Parameters
        ----------
        order : Order
            The order to update (from last event).

        """
        Condition.not_none(order, "order")

        cdef str client_order_id_str = order.client_order_id.to_str()
        cdef str key = f"{_ORDERS}:{client_order_id_str}"
        cdef list payload = [self._serializer.serialize(order.last_event_c())]
        self._backing.update(key, payload)

        if order.venue_order_id is not None:
            # Assumes order_id does not change
            self.index_venue_order_id(order.client_order_id, order.venue_order_id)

        payload = [client_order_id_str.encode()]

        # Update in-flight state
        if order.is_inflight_c():
            self._backing.insert(_INDEX_ORDERS_INFLIGHT, payload)
        else:
            self._backing.delete(_INDEX_ORDERS_INFLIGHT, payload)

        # Update open/closed state
        if order.is_open_c():
            self._backing.delete(_INDEX_ORDERS_CLOSED, payload)
            self._backing.insert(_INDEX_ORDERS_OPEN, payload)
        elif order.is_closed_c():
            self._backing.delete(_INDEX_ORDERS_OPEN, payload)
            self._backing.insert(_INDEX_ORDERS_CLOSED, payload)

        # Update emulation state
        if order.emulation_trigger == TriggerType.NO_TRIGGER:
            self._backing.delete(_INDEX_ORDERS_EMULATED, payload)
        else:
            self._backing.insert(_INDEX_ORDERS_EMULATED, payload)

        self._log.debug(f"Updated {order}")

    cpdef void update_position(self, Position position):
        """
        Update the given position in the database.

        Parameters
        ----------
        position : Position
            The position to update (from last event).

        """
        Condition.not_none(position, "position")

        cdef str position_id_str = position.id.to_str()
        cdef str key = f"{_POSITIONS}:{position.id.to_str()}"
        cdef list payload = [self._serializer.serialize(position.last_event_c())]
        self._backing.update(key, payload)

        if position.is_open_c():
            self._backing.insert(_INDEX_POSITIONS_OPEN, payload)
            self._backing.delete(_INDEX_POSITIONS_CLOSED, payload)
        elif position.is_closed_c():
            self._backing.insert(_INDEX_POSITIONS_CLOSED, payload)
            self._backing.delete(_INDEX_POSITIONS_OPEN, payload)

        self._log.debug(f"Updated {position}")

    cpdef void snapshot_order_state(self, Order order):
        """
        Snapshot the state of the given `order`.

        Parameters
        ----------
        order : Order
            The order for the state snapshot.

        """
        Condition.not_none(order, "order")

        cdef str key = f"{_SNAPSHOTS_ORDERS}:{order.client_order_id.to_str()}"
        cdef list payload = [self._serializer.serialize(order.to_dict())]
        self._backing.insert(key, payload)

        self._log.debug(f"Added state snapshot {order}")

    cpdef void snapshot_position_state(
        self,
        Position position,
        uint64_t ts_snapshot,
        Money unrealized_pnl = None,
    ):
        """
        Snapshot the state of the given `position`.

        Parameters
        ----------
        position : Position
            The position for the state snapshot.
        ts_snapshot : uint64_t
            UNIX timestamp (nanoseconds) when the snapshot was taken.
        unrealized_pnl : Money, optional
            The unrealized PnL for the state snapshot.

        """
        Condition.not_none(position, "position")

        cdef dict position_state = position.to_dict()

        if unrealized_pnl is not None:
            position_state["unrealized_pnl"] = str(unrealized_pnl)

        position_state["ts_snapshot"] = ts_snapshot

        cdef str key = f"{_SNAPSHOTS_POSITIONS}:{position.id.to_str()}"
        cdef list payload = [self._serializer.serialize(position_state)]
        self._backing.insert(key, payload)

        self._log.debug(f"Added state snapshot {position}")

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
        self._backing.insert(_HEARTBEAT, [timestamp_str.encode()])

        self._log.debug(f"Set last heartbeat {timestamp_str}")
