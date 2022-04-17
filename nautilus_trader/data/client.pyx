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

from cpython.datetime cimport datetime

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class DataClient(Component):
    """
    The abstract base class for all data clients.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    msgbus : MessageBus
        The message bus for the client.
    clock : Clock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        Venue venue,  # Can be None
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        if config is None:
            config = {}
        super().__init__(
            clock=clock,
            logger=logger,
            component_id=client_id,
            component_name=config.get("name", f"DataClient-{client_id.value}"),
            msgbus=msgbus,
            config=config,
        )

        self._cache = cache

        self.venue = venue

        # Subscriptions
        self._subscriptions_generic = set()  # type: set[DataType]

        self.is_connected = False

    def __repr__(self) -> str:
        return f"{type(self).__name__}-{self.id.value}"

    cpdef void _set_connected(self, bint value=True) except *:
        """
        Setter for pure Python implementations to change the readonly property.

        Parameters
        ----------
        value : bool
            The value to set for is_connected.

        """
        self.is_connected = value

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_generic_data(self):
        """
        Return the generic data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        return sorted(list(self._subscriptions_generic))

    cpdef void subscribe(self, DataType data_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe(self, DataType data_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void _add_subscription(self, DataType data_type) except *:
        """
        Add subscription for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.

        """
        Condition.not_none(data_type, "data_type")

        self._subscriptions_generic.add(data_type)

    cpdef void _remove_subscription(self, DataType data_type) except *:
        """
        Remove subscription for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.

        """
        Condition.not_none(data_type, "data_type")

        self._subscriptions_generic.discard(data_type)

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID4 correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- PYTHON WRAPPERS ------------------------------------------------------------------------------

    def _handle_data_py(self, Data data):
        self._handle_data(data)

    def _handle_data_response_py(self, DataType data_type, object data, UUID4 correlation_id):
        self._handle_data_response(data_type, data, correlation_id)

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_data(self, Data data) except *:
        self._msgbus.send(endpoint="DataEngine.process", msg=data)

    cpdef void _handle_data_response(self, DataType data_type, object data, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=self.venue,
            data_type=data_type,
            data=data,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)


cdef class MarketDataClient(DataClient):
    """
    The abstract base class for all market data clients.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        Venue venue,  # Can be None
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        super().__init__(
            client_id=client_id,
            venue=venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        # Subscriptions
        self._subscriptions_order_book_delta = set()          # type: set[InstrumentId]
        self._subscriptions_order_book_snapshot = set()       # type: set[InstrumentId]
        self._subscriptions_ticker = set()                    # type: set[InstrumentId]
        self._subscriptions_quote_tick = set()                # type: set[InstrumentId]
        self._subscriptions_trade_tick = set()                # type: set[InstrumentId]
        self._subscriptions_bar = set()                       # type: set[BarType]
        self._subscriptions_instrument_status_update = set()  # type: set[InstrumentId]
        self._subscriptions_instrument_close_price = set()    # type: set[InstrumentId]
        self._subscriptions_instrument = set()                # type: set[InstrumentId]

        # Tasks
        self._update_instruments_task = None

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_instruments(self):
        """
        Return the instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_instrument))

    cpdef list subscribed_order_book_deltas(self):
        """
        Return the order book delta instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_order_book_delta))

    cpdef list subscribed_order_book_snapshots(self):
        """
        Return the order book snapshot instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_order_book_snapshot))

    cpdef list subscribed_tickers(self):
        """
        Return the ticker instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_ticker))

    cpdef list subscribed_quote_ticks(self):
        """
        Return the quote tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_quote_tick))

    cpdef list subscribed_trade_ticks(self):
        """
        Return the trade tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_trade_tick))

    cpdef list subscribed_bars(self):
        """
        Return the bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        return sorted(list(self._subscriptions_bar))

    cpdef list subscribed_instrument_status_updates(self):
        """
        Return the status update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_instrument_status_update))

    cpdef list subscribed_instrument_close_prices(self):
        """
        Return the close price instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_instrument_close_price))

    cpdef void subscribe(self, DataType data_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe(self, DataType data_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_instruments(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_order_book_deltas(self, InstrumentId instrument_id, BookType book_type, int depth=0, dict kwargs=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_order_book_snapshots(self, InstrumentId instrument_id, BookType book_type, int depth=0, dict kwargs=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_ticker(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_instrument_close_prices(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_instruments(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_order_book_snapshots(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_ticker(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void unsubscribe_instrument_close_prices(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void _add_subscription_instrument(self, InstrumentId instrument_id) except *:
        """
        Add subscription for instrument updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument.add(instrument_id)

    cpdef void _add_subscription_order_book_deltas(self, InstrumentId instrument_id) except *:
        """
        Add subscription for order book deltas for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_order_book_delta.add(instrument_id)

    cpdef void _add_subscription_order_book_snapshots(self, InstrumentId instrument_id) except *:
        """
        Add subscription for order book snapshots for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_order_book_snapshot.add(instrument_id)

    cpdef void _add_subscription_ticker(self, InstrumentId instrument_id) except *:
        """
        Add subscription for ticker updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_ticker.add(instrument_id)

    cpdef void _add_subscription_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Add subscription for quote ticks for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_quote_tick.add(instrument_id)

    cpdef void _add_subscription_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Add subscription for trade ticks for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_trade_tick.add(instrument_id)

    cpdef void _add_subscription_bars(self, BarType bar_type) except *:
        """
        Add subscription for bars for the bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the subscription.

        """
        Condition.not_none(bar_type, "bar_type")

        self._subscriptions_bar.add(bar_type)

    cpdef void _add_subscription_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """
        Add subscription for instrument status updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument_status_update.add(instrument_id)

    cpdef void _add_subscription_instrument_close_prices(self, InstrumentId instrument_id) except *:
        """
        Add subscription for instrument close price updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument_close_price.add(instrument_id)

    cpdef void _remove_subscription_instrument(self, InstrumentId instrument_id) except *:
        """
        Remove subscription for instrument updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument.discard(instrument_id)

    cpdef void _remove_subscription_order_book_deltas(self, InstrumentId instrument_id) except *:
        """
        Remove subscription for order book deltas for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_order_book_delta.discard(instrument_id)

    cpdef void _remove_subscription_order_book_snapshots(self, InstrumentId instrument_id) except *:
        """
        Remove subscription for order book snapshots for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_order_book_snapshot.discard(instrument_id)

    cpdef void _remove_subscription_ticker(self, InstrumentId instrument_id) except *:
        """
        Remove subscription for ticker updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_ticker.discard(instrument_id)

    cpdef void _remove_subscription_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Remove subscription for quote ticks for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_quote_tick.discard(instrument_id)

    cpdef void _remove_subscription_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Remove subscription for trade ticks for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_trade_tick.discard(instrument_id)

    cpdef void _remove_subscription_bars(self, BarType bar_type) except *:
        """
        Remove subscription for bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the subscription.

        """
        Condition.not_none(bar_type, "bar_type")

        self._subscriptions_bar.discard(bar_type)

    cpdef void _remove_subscription_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """
        Remove subscription for instrument status updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument_status_update.discard(instrument_id)

    cpdef void _remove_subscription_instrument_close_prices(self, InstrumentId instrument_id) except *:
        """
        Remove subscription for instrument close price updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument_close_price.discard(instrument_id)

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request(self, DataType datatype, UUID4 correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void request_instrument(self, InstrumentId instrument_id, UUID4 correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID4 correlation_id,
    ) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID4 correlation_id,
    ) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID4 correlation_id,
    ) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- PYTHON WRAPPERS ------------------------------------------------------------------------------

    # Convenient pure Python wrappers for the data handlers. Often Python methods
    # involving threads or the event loop don't work with cpdef methods.

    def _handle_quote_ticks_py(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id):
        self._handle_quote_ticks(instrument_id, ticks, correlation_id)

    def _handle_trade_ticks_py(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id):
        self._handle_trade_ticks(instrument_id, ticks, correlation_id)

    def _handle_bars_py(self, BarType bar_type, list bars, Bar partial, UUID4 correlation_id):
        self._handle_bars(bar_type, bars, partial, correlation_id)

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_quote_ticks(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=self.venue,
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            data=ticks,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)

    cpdef void _handle_trade_ticks(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=self.venue,
            data_type=DataType(TradeTick, metadata={"instrument_id": instrument_id}),
            data=ticks,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)

    cpdef void _handle_bars(self, BarType bar_type, list bars, Bar partial, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=self.venue,
            data_type=DataType(Bar, metadata={"bar_type": bar_type, "Partial": partial}),
            data=bars,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)
