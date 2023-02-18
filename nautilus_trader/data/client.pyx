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

from typing import Optional

from cpython.datetime cimport datetime

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class DataClient(Component):
    """
    The base class for all data clients.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    clock : Clock
        The clock for the client.
    logger : Logger
        The logger for the client.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        Venue venue: Optional[Venue] = None,
        dict config = None,
    ):
        if config is None:
            config = {}
        super().__init__(
            clock=clock,
            logger=logger,
            component_id=client_id,
            component_name=config.get("name", f"DataClient-{client_id}"),
            msgbus=msgbus,
            config=config,
        )

        self._cache = cache

        self.venue = venue

        # Subscriptions
        self._subscriptions_generic: set[DataType] = set()

        self.is_connected = False

    def __repr__(self) -> str:
        return f"{type(self).__name__}-{self.id.value}"

    cpdef void _set_connected(self, bint value=True) except *:
        """
        Setter for Python implementations to change the readonly property.

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
        """
        Subscribe to data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.

        """
        self._log.error(
            f"Cannot subscribe to {data_type}: not implemented. "
            f"You can implement by overriding the `subscribe` method for this client.",
        )

    cpdef void unsubscribe(self, DataType data_type) except *:
        """
        Unsubscribe from data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.

        """
        self._log.error(
            f"Cannot unsubscribe from {data_type}: not implemented. "
            f"You can implement by overriding the `unsubscribe` method for this client.",
        )

    cpdef void _add_subscription(self, DataType data_type) except *:
        Condition.not_none(data_type, "data_type")

        self._subscriptions_generic.add(data_type)

    cpdef void _remove_subscription(self, DataType data_type) except *:
        Condition.not_none(data_type, "data_type")

        self._subscriptions_generic.discard(data_type)

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID4 correlation_id) except *:
        """
        Request data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.
        correlation_id : UUID4
            The correlation ID for the response.

        """
        self._log.error(
            f"Cannot request {data_type}: not implemented. "
            f"You can implement by overriding the `request` method for this client.",
        )

# -- PYTHON WRAPPERS ------------------------------------------------------------------------------

    def _handle_data_py(self, Data data):
        self._handle_data(data)

    def _handle_data_response_py(self, DataType data_type, data, UUID4 correlation_id):
        self._handle_data_response(data_type, data, correlation_id)

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_data(self, Data data) except *:
        self._msgbus.send(endpoint="DataEngine.process", msg=data)

    cpdef void _handle_data_response(self, DataType data_type, data, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=self.venue,
            data_type=data_type,
            data=data,
            correlation_id=correlation_id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)


cdef class MarketDataClient(DataClient):
    """
    The base class for all market data clients.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    logger : Logger
        The logger for the client.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        Venue venue: Optional[Venue] = None,
        dict config = None,
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
        self._subscriptions_venue_status_update = set()       # type: set[Venue]
        self._subscriptions_instrument_status_update = set()  # type: set[InstrumentId]
        self._subscriptions_instrument_close = set()          # type: set[InstrumentId]
        self._subscriptions_instrument = set()                # type: set[InstrumentId]

        # Tasks
        self._update_instruments_task = None

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_generic_data(self):
        """
        Return the generic data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        return sorted(list(self._subscriptions_generic))

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

    cpdef list subscribed_venue_status_updates(self):
        """
        Return the status update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_venue_status_update))

    cpdef list subscribed_instrument_status_updates(self):
        """
        Return the status update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_instrument_status_update))

    cpdef list subscribed_instrument_close(self):
        """
        Return the instrument closes subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscriptions_instrument_close))

    cpdef void subscribe(self, DataType data_type) except *:
        """
        Subscribe to data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.

        """
        self._log.error(
            f"Cannot subscribe to {data_type}: not implemented. "
            f"You can implement by overriding the `subscribe` method for this client.",
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_instruments(self) except *:
        """
        Subscribe to all `Instrument` data.

        """
        self._log.error(
            f"Cannot subscribe to all `Instrument` data: not implemented. "
            f"You can implement by overriding the `subscribe_instruments` method for this client.",
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Subscribe to the `Instrument` with the given instrument ID.

        """
        self._log.error(
            f"Cannot subscribe to `Instrument` data for {instrument_id}: not implemented. "
            f"You can implement by overriding the `subscribe_instrument` method for this client.",
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_order_book_deltas(self, InstrumentId instrument_id, BookType book_type, int depth = 0, dict kwargs = None) except *:
        """
        Subscribe to `OrderBookDeltas` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        depth : int, optional, default None
            The maximum depth for the subscription.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `OrderBookDeltas` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_order_book_deltas` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_order_book_snapshots(self, InstrumentId instrument_id, BookType book_type, int depth = 0, dict kwargs = None) except *:
        """
        Subscribe to `OrderBookSnapshot` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
            The order book level.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `OrderBookSnapshot` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_order_book_snapshots` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_ticker(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `Ticker` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The ticker instrument to subscribe to.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `Ticker` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_ticker` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `QuoteTick` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_quote_ticks` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `TradeTick` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_trade_ticks` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_venue_status_updates(self, Venue venue) except *:
        """
        Subscribe to `InstrumentStatusUpdate` data for the venue.

        Parameters
        ----------
        venue : Venue
            The venue to subscribe to.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `VenueStatusUpdate` data for {venue}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_venue_status_updates` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `InstrumentStatusUpdates` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `InstrumentStatusUpdates` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_instrument_status_updates` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_instrument_close(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `InstrumentClose` updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `InstrumentClose` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_instrument_close` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        self._log.error(  # pragma: no cover
            f"Cannot subscribe to `Bar` data for {bar_type}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `subscribe_bars` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe(self, DataType data_type) except *:
        """
        Unsubscribe from data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.

        """
        self._log.error(
            f"Cannot unsubscribe from {data_type}: not implemented. "
            f"You can implement by overriding the `unsubscribe` method for this client.",
        )

    cpdef void unsubscribe_instruments(self) except *:
        """
        Unsubscribe from all `Instrument` data.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from all `Instrument` data: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_instruments` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `Instrument` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_instrument` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBookDeltas` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `OrderBookDeltas` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_order_book_deltas` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_order_book_snapshots(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBookSnapshot` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `OrderBookSnapshot` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_order_book_snapshots` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_ticker(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `Ticker` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The ticker instrument to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `Ticker` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_ticker` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `QuoteTick` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_quote_ticks` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `TradeTick` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_trade_ticks` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `Bar` data for {bar_type}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_bars` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_venue_status_updates(self, Venue venue) except *:
        """
        Unsubscribe from `InstrumentStatusUpdate` data for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `VenueStatusUpdates` data for {venue}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_venue_status_updates` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `InstrumentStatusUpdate` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument status updates to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `InstrumentStatusUpdates` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_instrument_status_updates` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_instrument_close(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `InstrumentClose` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        self._log.error(  # pragma: no cover
            f"Cannot unsubscribe from `InstrumentClose` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `unsubscribe_instrument_close` method for this client.",  # pragma: no cover
        )
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void _add_subscription(self, DataType data_type) except *:
        Condition.not_none(data_type, "data_type")

        self._subscriptions_generic.add(data_type)

    cpdef void _add_subscription_instrument(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument.add(instrument_id)

    cpdef void _add_subscription_order_book_deltas(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_order_book_delta.add(instrument_id)

    cpdef void _add_subscription_order_book_snapshots(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_order_book_snapshot.add(instrument_id)

    cpdef void _add_subscription_ticker(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_ticker.add(instrument_id)

    cpdef void _add_subscription_quote_ticks(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_quote_tick.add(instrument_id)

    cpdef void _add_subscription_trade_ticks(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_trade_tick.add(instrument_id)

    cpdef void _add_subscription_bars(self, BarType bar_type) except *:
        Condition.not_none(bar_type, "bar_type")

        self._subscriptions_bar.add(bar_type)

    cpdef void _add_subscription_venue_status_updates(self, Venue venue) except *:
        Condition.not_none(venue, "venue")

        self._subscriptions_venue_status_update.add(venue)

    cpdef void _add_subscription_instrument_status_updates(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument_status_update.add(instrument_id)

    cpdef void _add_subscription_instrument_close(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument_close.add(instrument_id)

    cpdef void _remove_subscription(self, DataType data_type) except *:
        Condition.not_none(data_type, "data_type")

        self._subscriptions_generic.discard(data_type)

    cpdef void _remove_subscription_instrument(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument.discard(instrument_id)

    cpdef void _remove_subscription_order_book_deltas(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_order_book_delta.discard(instrument_id)

    cpdef void _remove_subscription_order_book_snapshots(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_order_book_snapshot.discard(instrument_id)

    cpdef void _remove_subscription_ticker(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_ticker.discard(instrument_id)

    cpdef void _remove_subscription_quote_ticks(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_quote_tick.discard(instrument_id)

    cpdef void _remove_subscription_trade_ticks(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_trade_tick.discard(instrument_id)

    cpdef void _remove_subscription_bars(self, BarType bar_type) except *:
        Condition.not_none(bar_type, "bar_type")

        self._subscriptions_bar.discard(bar_type)

    cpdef void _remove_subscription_venue_status_updates(self, Venue venue) except *:
        Condition.not_none(venue, "venue")

        self._subscriptions_venue_status_update.discard(venue)

    cpdef void _remove_subscription_instrument_status_updates(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument_status_update.discard(instrument_id)

    cpdef void _remove_subscription_instrument_close(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")

        self._subscriptions_instrument_close.discard(instrument_id)

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request_instrument(self, InstrumentId instrument_id, UUID4 correlation_id) except *:
        """
        Request `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the request.
        correlation_id : UUID4
            The correlation ID for the request.

        """
        self._log.error(  # pragma: no cover
            f"Cannot request `Instrument` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `request_instrument` method for this client.",  # pragma: no cover  # noqa
        )

    cpdef void request_instruments(self, Venue venue, UUID4 correlation_id) except *:
        """
        Request all `Instrument` data for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue for the request.
        correlation_id : UUID4
            The correlation ID for the request.

        """
        self._log.error(  # pragma: no cover
            f"Cannot request all `Instrument` data: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `request_instruments` method for this client.",  # pragma: no cover  # noqa
        )

    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        int limit,
        UUID4 correlation_id,
        datetime from_datetime = None,
        datetime to_datetime = None,
    ) except *:
        """
        Request historical `QuoteTick` data.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID4
            The correlation ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If ``None`` then will default
            to the current datetime.

        """
        self._log.error(  # pragma: no cover
            f"Cannot request `QuoteTick` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `request_quote_ticks` method for this client.",  # pragma: no cover  # noqa
        )

    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        int limit,
        UUID4 correlation_id,
        datetime from_datetime = None,
        datetime to_datetime = None,
    ) except *:
        """
        Request historical `TradeTick` data.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID4
            The correlation ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If ``None`` then will default
            to the current datetime.

        """
        self._log.error(  # pragma: no cover
            f"Cannot request `TradeTick` data for {instrument_id}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `request_trade_ticks` method for this client.",  # pragma: no cover  # noqa
        )

    cpdef void request_bars(
        self,
        BarType bar_type,
        int limit,
        UUID4 correlation_id,
        datetime from_datetime = None,
        datetime to_datetime = None,
    ) except *:
        """
        Request historical `Bar` data.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        limit : int
            The limit for the number of returned bars.
        correlation_id : UUID4
            The correlation ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If ``None`` then will default
            to the current datetime.

        """
        self._log.error(  # pragma: no cover
            f"Cannot request `Bar` data for {bar_type}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `request_bars` method for this client.",  # pragma: no cover  # noqa
        )

# -- PYTHON WRAPPERS ------------------------------------------------------------------------------

    # Convenient Python wrappers for the data handlers. Often Python methods
    # involving threads or the event loop don't work with `cpdef` methods.

    def _handle_data_py(self, Data data):
        self._handle_data(data)

    def _handle_instrument_py(self, Instrument instrument, UUID4 correlation_id):
        self._handle_instrument(instrument, correlation_id)

    def _handle_instruments_py(self, Venue venue, list instruments, UUID4 correlation_id):
        self._handle_instruments(venue, instruments, correlation_id)

    def _handle_quote_ticks_py(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id):
        self._handle_quote_ticks(instrument_id, ticks, correlation_id)

    def _handle_trade_ticks_py(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id):
        self._handle_trade_ticks(instrument_id, ticks, correlation_id)

    def _handle_bars_py(self, BarType bar_type, list bars, Bar partial, UUID4 correlation_id):
        self._handle_bars(bar_type, bars, partial, correlation_id)

    def _handle_data_response_py(self, DataType data_type, data, UUID4 correlation_id):
        self._handle_data_response(data_type, data, correlation_id)

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_data(self, Data data) except *:
        self._msgbus.send(endpoint="DataEngine.process", msg=data)

    cpdef void _handle_instrument(self, Instrument instrument, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=instrument.venue,
            data_type=DataType(Instrument, metadata={"instrument_id": instrument.id}),
            data=instrument,
            correlation_id=correlation_id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)

    cpdef void _handle_instruments(self, Venue venue, list instruments, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=venue,
            data_type=DataType(Instrument, metadata={"venue": venue}),
            data=instruments,
            correlation_id=correlation_id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)

    cpdef void _handle_quote_ticks(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=instrument_id.venue,
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            data=ticks,
            correlation_id=correlation_id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)

    cpdef void _handle_trade_ticks(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=instrument_id.venue,
            data_type=DataType(TradeTick, metadata={"instrument_id": instrument_id}),
            data=ticks,
            correlation_id=correlation_id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)

    cpdef void _handle_bars(self, BarType bar_type, list bars, Bar partial, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=bar_type.instrument_id.venue,
            data_type=DataType(Bar, metadata={"bar_type": bar_type, "Partial": partial}),
            data=bars,
            correlation_id=correlation_id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)

    cpdef void _handle_data_response(self, DataType data_type, data, UUID4 correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            venue=self.venue,
            data_type=data_type,
            data=data,
            correlation_id=correlation_id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="DataEngine.response", msg=response)
