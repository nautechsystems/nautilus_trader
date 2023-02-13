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

"""
The `DataEngine` is the central component of the entire data stack.

The data engines primary responsibility is to orchestrate interactions between
the `DataClient` instances, and the rest of the platform. This includes sending
requests to, and receiving responses from, data endpoints via its registered
data clients.

The engine employs a simple fan-in fan-out messaging pattern to execute
`DataCommand` type messages, and process `DataResponse` messages or market data
objects.

Alternative implementations can be written on top of the generic engine - which
just need to override the `execute`, `process`, `send` and `receive` methods.
"""

from typing import Callable, Optional

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import DataEngineConfig

from cpython.datetime cimport timedelta

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.enums_c cimport ComponentState
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport REQ
from nautilus_trader.common.logging cimport RES
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.data.aggregation cimport BarAggregator
from nautilus_trader.data.aggregation cimport TickBarAggregator
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.aggregation cimport ValueBarAggregator
from nautilus_trader.data.aggregation cimport VolumeBarAggregator
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport Subscribe
from nautilus_trader.data.messages cimport Unsubscribe
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.venue cimport InstrumentClose
from nautilus_trader.model.data.venue cimport InstrumentStatusUpdate
from nautilus_trader.model.data.venue cimport VenueStatusUpdate
from nautilus_trader.model.enums_c cimport BarAggregation
from nautilus_trader.model.enums_c cimport PriceType
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.model.orderbook.data cimport OrderBookSnapshot
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class DataEngine(Component):
    """
    Provides a high-performance data engine for managing many `DataClient`
    instances, for the asynchronous ingest of data.

    Parameters
    ----------
    msgbus : MessageBus
        The message bus for the engine.
    cache : Cache
        The cache for the engine.
    clock : Clock
        The clock for the engine.
    logger : Logger
        The logger for the engine.
    config : DataEngineConfig, optional
        The configuration for the instance.
    """

    def __init__(
        self,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        config: Optional[DataEngineConfig] = None,
    ):
        if config is None:
            config = DataEngineConfig()
        Condition.type(config, DataEngineConfig, "config")
        super().__init__(
            clock=clock,
            logger=logger,
            component_id=ComponentId("DataEngine"),
            msgbus=msgbus,
            config=config.dict(),
        )

        self._cache = cache

        self._clients: dict[ClientId, DataClient] = {}
        self._routing_map: dict[Venue, DataClient] = {}
        self._default_client: Optional[DataClient] = None
        self._order_book_intervals: dict[(InstrumentId, int), list[Callable[[Bar], None]]] = {}
        self._bar_aggregators: dict[BarType, BarAggregator] = {}

        # Settings
        self.debug = config.debug
        self._time_bars_build_with_no_updates = config.time_bars_build_with_no_updates
        self._time_bars_timestamp_on_close = config.time_bars_timestamp_on_close
        self._validate_data_sequence = config.validate_data_sequence

        # Counters
        self.command_count = 0
        self.data_count = 0
        self.request_count = 0
        self.response_count = 0

        # Register endpoints
        self._msgbus.register(endpoint="DataEngine.execute", handler=self.execute)
        self._msgbus.register(endpoint="DataEngine.process", handler=self.process)
        self._msgbus.register(endpoint="DataEngine.request", handler=self.request)
        self._msgbus.register(endpoint="DataEngine.response", handler=self.response)

    @property
    def registered_clients(self):
        """
        Return the execution clients registered with the engine.

        Returns
        -------
        list[ClientId]

        """
        return sorted(list(self._clients.keys()))

    @property
    def default_client(self):
        """
        Return the default data client registered with the engine.

        Returns
        -------
        Optional[ClientId]

        """
        return self._default_client.id if self._default_client is not None else None

# --REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, DataClient client) except *:
        """
        Register the given data client with the data engine.

        Parameters
        ----------
        client : DataClient
            The client to register.

        Raises
        ------
        ValueError
            If `client` is already registered.

        """
        Condition.not_none(client, "client")
        Condition.not_in(client.id, self._clients, "client", "_clients")

        self._clients[client.id] = client

        routing_log = ""
        if client.venue is None:
            if self._default_client is None:
                self._default_client = client
                routing_log = " for default routing"
        else:
            self._routing_map[client.venue] = client

        self._log.info(f"Registered {client}{routing_log}.")

    cpdef void register_default_client(self, DataClient client) except *:
        """
        Register the given client as the default routing client (when a specific
        venue routing cannot be found).

        Any existing default routing client will be overwritten.

        Parameters
        ----------
        client : DataClient
            The client to register.

        """
        Condition.not_none(client, "client")

        self._default_client = client

        self._log.info(f"Registered {client} for default routing.")

    cpdef void register_venue_routing(self, DataClient client, Venue venue) except *:
        """
        Register the given client to route orders to the given venue.

        Any existing client in the routing map for the given venue will be
        overwritten.

        Parameters
        ----------
        venue : Venue
            The venue to route orders to.
        client : ExecutionClient
            The client for the venue routing.

        """
        Condition.not_none(client, "client")
        Condition.not_none(venue, "venue")

        if client.id not in self._clients:
            self._clients[client.id] = client

        self._routing_map[venue] = client

        self._log.info(f"Registered ExecutionClient-{client} for routing to {venue}.")

    cpdef void deregister_client(self, DataClient client) except *:
        """
        Deregister the given data client from the data engine.

        Parameters
        ----------
        client : DataClient
            The data client to deregister.

        """
        Condition.not_none(client, "client")
        Condition.is_in(client.id, self._clients, "client.id", "self._clients")

        del self._clients[client.id]
        self._log.info(f"Deregistered {client}.")

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_generic_data(self):
        """
        Return the generic data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        cdef list subscriptions = []
        cdef DataClient client
        for client in self._clients.values():
            subscriptions += client.subscribed_generic_data()
        return subscriptions

    cpdef list subscribed_instruments(self):
        """
        Return the instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_instruments()
        return subscriptions

    cpdef list subscribed_order_book_deltas(self):
        """
        Return the order book delta instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_order_book_deltas()
        return subscriptions

    cpdef list subscribed_order_book_snapshots(self):
        """
        Return the order book snapshot instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_order_book_snapshots()
        return subscriptions

    cpdef list subscribed_tickers(self):
        """
        Return the ticker instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_tickers()
        return subscriptions

    cpdef list subscribed_quote_ticks(self):
        """
        Return the quote tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_quote_ticks()
        return subscriptions

    cpdef list subscribed_trade_ticks(self):
        """
        Return the trade tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_trade_ticks()
        return subscriptions

    cpdef list subscribed_bars(self):
        """
        Return the bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_bars()
        return subscriptions + list(self._bar_aggregators.keys())

    cpdef list subscribed_instrument_status_updates(self):
        """
        Return the status update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_instrument_status_updates()
        return subscriptions

    cpdef list subscribed_instrument_close(self):
        """
        Return the close price instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_instrument_close()
        return subscriptions

    cpdef bint check_connected(self) except *:
        """
        Check all of the engines clients are connected.

        Returns
        -------
        bool
            True if all clients connected, else False.

        """
        cdef DataClient client
        for client in self._clients.values():
            if not client.is_connected:
                return False
        return True

    cpdef bint check_disconnected(self) except *:
        """
        Check all of the engines clients are disconnected.

        Returns
        -------
        bool
            True if all clients disconnected, else False.

        """
        cdef DataClient client
        for client in self._clients.values():
            if client.is_connected:
                return False
        return True

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self) except *:
        pass  # Optionally override in subclass

    cpdef void _on_stop(self) except *:
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _start(self) except *:
        cdef DataClient client
        for client in self._clients.values():
            client.start()

        self._on_start()

    cpdef void _stop(self) except *:
        cdef DataClient client
        for client in self._clients.values():
            client.stop()

        for aggregator in self._bar_aggregators.values():
            if isinstance(aggregator, TimeBarAggregator):
                aggregator.stop()

        self._on_stop()

    cpdef void _reset(self) except *:
        cdef DataClient client
        for client in self._clients.values():
            client.reset()

        self._order_book_intervals.clear()
        self._bar_aggregators.clear()

        self._clock.cancel_timers()
        self.command_count = 0
        self.data_count = 0
        self.request_count = 0
        self.response_count = 0

    cpdef void _dispose(self) except *:
        cdef DataClient client
        for client in self._clients.values():
            client.dispose()

        self._clock.cancel_timers()

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void execute(self, DataCommand command) except *:
        """
        Execute the given data command.

        Parameters
        ----------
        command : DataCommand
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, Data data) except *:
        """
        Process the given data.

        Parameters
        ----------
        data : Data
            The data to process.

        """
        Condition.not_none(data, "data")

        self._handle_data(data)

    cpdef void request(self, DataRequest request) except *:
        """
        Handle the given request.

        Parameters
        ----------
        request : DataRequest
            The request to handle.

        """
        Condition.not_none(request, "request")

        self._handle_request(request)

    cpdef void response(self, DataResponse response) except *:
        """
        Handle the given response.

        Parameters
        ----------
        response : DataResponse
            The response to handle.

        """
        Condition.not_none(response, "response")

        self._handle_response(response)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cdef void _execute_command(self, DataCommand command) except *:
        if self.debug:
            self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        cdef DataClient client = self._clients.get(command.client_id)
        if client is None:
            client = self._routing_map.get(command.venue, self._default_client)
            if client is None:
                self._log.error(
                    f"Cannot execute command: "
                    f"No data client configured for {command.client_id}, {command}."
                )
                return  # No client to handle command

        if isinstance(command, Subscribe):
            self._handle_subscribe(client, command)
        elif isinstance(command, Unsubscribe):
            self._handle_unsubscribe(client, command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cdef void _handle_subscribe(self, DataClient client, Subscribe command) except *:
        if command.data_type.type == Instrument:
            self._handle_subscribe_instrument(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == OrderBookSnapshot:
            self._handle_subscribe_order_book_snapshots(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata,
            )
        elif command.data_type.type == OrderBookData:
            self._handle_subscribe_order_book_deltas(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata,
            )
        elif command.data_type.type == Ticker:
            self._handle_subscribe_ticker(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == QuoteTick:
            self._handle_subscribe_quote_ticks(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == TradeTick:
            self._handle_subscribe_trade_ticks(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == Bar:
            self._handle_subscribe_bars(
                client,
                command.data_type.metadata.get("bar_type"),
            )
        elif command.data_type.type == VenueStatusUpdate:
            self._handle_subscribe_venue_status_updates(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == InstrumentStatusUpdate:
            self._handle_subscribe_instrument_status_updates(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == InstrumentClose:
            self._handle_subscribe_instrument_close(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        else:
            self._handle_subscribe_data(client, command.data_type)

    cdef void _handle_unsubscribe(self, DataClient client, Unsubscribe command) except *:
        if command.data_type.type == Instrument:
            self._handle_unsubscribe_instrument(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == OrderBookSnapshot:
            self._handle_unsubscribe_order_book_snapshots(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata,
            )
        elif command.data_type.type == OrderBookData:
            self._handle_unsubscribe_order_book_deltas(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata,
            )
        elif command.data_type.type == Ticker:
            self._handle_unsubscribe_ticker(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == QuoteTick:
            self._handle_unsubscribe_quote_ticks(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == TradeTick:
            self._handle_unsubscribe_trade_ticks(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == Bar:
            self._handle_unsubscribe_bars(
                client,
                command.data_type.metadata.get("bar_type"),
            )
        else:
            self._handle_unsubscribe_data(client, command.data_type)

    cdef void _handle_subscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")

        if instrument_id is None:
            client.subscribe_instruments()
            return

        if instrument_id not in client.subscribed_instruments():
            client.subscribe_instrument(instrument_id)

    cdef void _handle_subscribe_order_book_deltas(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        # Create order book
        if not self._cache.has_order_book(instrument_id):
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot subscribe to {instrument_id} <OrderBook> data: "
                    f"no instrument found in the cache.",
                )
                return
            order_book = OrderBook.create(
                instrument=instrument,
                book_type=metadata["book_type"],
            )

            self._cache.add_order_book(order_book)
            self._log.debug(f"Created {type(order_book).__name__}.")

        # Always re-subscribe to override previous settings
        client.subscribe_order_book_deltas(
            instrument_id=instrument_id,
            book_type=metadata["book_type"],
            depth=metadata["depth"],
            kwargs=metadata.get("kwargs"),
        )

        cdef str topic = f"data.book.deltas.{instrument_id.venue}.{instrument_id.symbol}"

        if self._msgbus.is_subscribed(
            topic=topic,
            handler=self._maintain_order_book,
        ):
            return  # Already subscribed

        self._msgbus.subscribe(
            topic=topic,
            handler=self._maintain_order_book,
            priority=10,
        )

    cdef void _handle_subscribe_order_book_snapshots(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        cdef int interval_ms = metadata["interval_ms"]
        key = (instrument_id, interval_ms)
        if key not in self._order_book_intervals:
            self._order_book_intervals[key] = []
            now = self._clock.utc_now()
            start_time = now - timedelta(milliseconds=int((now.second * 1000) % interval_ms), microseconds=now.microsecond)
            timer_name = f"OrderBookSnapshot_{instrument_id}_{interval_ms}"
            self._clock.set_timer(
                name=timer_name,
                interval=timedelta(milliseconds=interval_ms),
                start_time=start_time,
                stop_time=None,
                callback=self._snapshot_order_book,
            )
            self._log.debug(f"Set timer {timer_name}.")

        # Create order book
        if not self._cache.has_order_book(instrument_id):
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot subscribe to {instrument_id} <OrderBook> data: "
                    f"no instrument found in the cache.",
                )
                return
            order_book = OrderBook.create(
                instrument=instrument,
                book_type=metadata["book_type"],
            )

            self._cache.add_order_book(order_book)
            self._log.debug(f"Created {type(order_book).__name__}.")

        # Always re-subscribe to override previous settings
        try:
            if instrument_id not in client.subscribed_order_book_deltas():
                client.subscribe_order_book_deltas(
                    instrument_id=instrument_id,
                    book_type=metadata["book_type"],
                    depth=metadata["depth"],
                    kwargs=metadata.get("kwargs"),
                )
        except NotImplementedError:
            if instrument_id not in client.subscribed_order_book_snapshots():
                client.subscribe_order_book_snapshots(
                    instrument_id=instrument_id,
                    book_type=metadata["book_type"],
                    depth=metadata["depth"],
                    kwargs=metadata.get("kwargs"),
                )

        cdef str topic = f"data.book.deltas.{instrument_id.venue}.{instrument_id.symbol}"

        if self._msgbus.is_subscribed(
            topic=topic,
            handler=self._maintain_order_book,
        ):
            return  # Already subscribed

        self._msgbus.subscribe(
            topic=f"data.book.deltas"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self._maintain_order_book,
            priority=10,
        )

    cdef void _handle_subscribe_ticker(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in client.subscribed_tickers():
            client.subscribe_ticker(instrument_id)

    cdef void _handle_subscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in client.subscribed_quote_ticks():
            client.subscribe_quote_ticks(instrument_id)

    cdef void _handle_subscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in client.subscribed_trade_ticks():
            client.subscribe_trade_ticks(instrument_id)

    cdef void _handle_subscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")

        if bar_type.is_internally_aggregated() and bar_type not in self._bar_aggregators:
            # Internal aggregation
            self._start_bar_aggregator(client, bar_type)
        else:
            # External aggregation
            if bar_type not in client.subscribed_bars():
                client.subscribe_bars(bar_type)

    cdef void _handle_subscribe_data(
        self,
        DataClient client,
        DataType data_type,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(data_type, "data_type")

        try:
            if data_type not in client.subscribed_generic_data():
                client.subscribe(data_type)
        except NotImplementedError:
            self._log.error(
                f"Cannot subscribe: {client.id.value} "
                f"has not implemented {data_type} subscriptions.",
            )
            return

    cdef void _handle_subscribe_venue_status_updates(
        self,
        MarketDataClient client,
        Venue venue,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(venue, "venue")

        if venue not in client.subscribed_venue_status_updates():
            client.subscribe_venue_status_updates(venue)

    cdef void _handle_subscribe_instrument_status_updates(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in client.subscribed_instrument_status_updates():
            client.subscribe_instrument_status_updates(instrument_id)

    cdef void _handle_subscribe_instrument_close(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in client.subscribed_instrument_close():
            client.subscribe_instrument_close(instrument_id)

    cdef void _handle_unsubscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")

        if instrument_id is None:
            if not self._msgbus.has_subscribers(f"data.instrument.{client.id.value}.*"):
                client.unsubscribe_instruments()
            return
        else:
            if not self._msgbus.has_subscribers(
                f"data.instrument"
                f".{instrument_id.venue}"
                f".{instrument_id.symbol}",
            ):
                client.unsubscribe_instrument(instrument_id)

    cdef void _handle_unsubscribe_order_book_deltas(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        if not self._msgbus.has_subscribers(
            f"data.book.deltas"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_order_book_deltas(instrument_id)

    cdef void _handle_unsubscribe_order_book_snapshots(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        if not self._msgbus.has_subscribers(
            f"data.book.snapshots"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_order_book_snapshots(instrument_id)

    cdef void _handle_unsubscribe_ticker(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if not self._msgbus.has_subscribers(
            f"data.tickers"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_ticker(instrument_id)

    cdef void _handle_unsubscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if not self._msgbus.has_subscribers(
            f"data.quotes"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_quote_ticks(instrument_id)

    cdef void _handle_unsubscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if not self._msgbus.has_subscribers(
            f"data.trades"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_trade_ticks(instrument_id)

    cdef void _handle_unsubscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")

        if bar_type.is_internally_aggregated() and bar_type in self._bar_aggregators:
            # Internal aggregation
            self._stop_bar_aggregator(client, bar_type)
        else:
            if not self._msgbus.has_subscribers(f"data.bars.{bar_type}"):
                # External aggregation
                client.unsubscribe_bars(bar_type)

    cdef void _handle_unsubscribe_data(
        self,
        DataClient client,
        DataType data_type,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(data_type, "data_type")

        try:
            if not self._msgbus.has_subscribers(f"data.{data_type}"):
                client.unsubscribe(data_type)
        except NotImplementedError:
            self._log.error(
                f"Cannot unsubscribe: {client.id.value} "
                f"has not implemented data type {data_type} subscriptions.",
            )
            return

# -- REQUEST HANDLERS -----------------------------------------------------------------------------

    cdef void _handle_request(self, DataRequest request) except *:
        if self.debug:
            self._log.debug(f"{RECV}{REQ} {request}.", LogColor.MAGENTA)
        self.request_count += 1

        cdef DataClient client = self._clients.get(request.client_id)
        if client is None:
            client = self._routing_map.get(
                request.venue,
                self._default_client,
            )
            if client is None:
                self._log.error(
                    f"Cannot handle request: "
                    f"no client registered for '{request.client_id}', {request}.")
                return  # No client to handle request

        if request.data_type.type == Instrument:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            instrument_id = request.data_type.metadata.get("instrument_id")
            if instrument_id is None:
                client.request_instruments(request.data_type.metadata.get("venue"), request.id)
            else:
                client.request_instrument(instrument_id, request.id)
        elif request.data_type.type == QuoteTick:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_quote_ticks(
                request.data_type.metadata.get("instrument_id"),
                request.data_type.metadata.get("limit", 0),
                request.id,
                request.data_type.metadata.get("from_datetime"),
                request.data_type.metadata.get("to_datetime"),
            )
        elif request.data_type.type == TradeTick:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_trade_ticks(
                request.data_type.metadata.get("instrument_id"),
                request.data_type.metadata.get("limit", 0),
                request.id,
                request.data_type.metadata.get("from_datetime"),
                request.data_type.metadata.get("to_datetime"),
            )
        elif request.data_type.type == Bar:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_bars(
                request.data_type.metadata.get("bar_type"),
                request.data_type.metadata.get("limit", 0),
                request.id,
                request.data_type.metadata.get("from_datetime"),
                request.data_type.metadata.get("to_datetime"),
            )
        else:
            try:
                client.request(request.data_type, request.id)
            except NotImplementedError:
                self._log.error(f"Cannot handle request: unrecognized data type {request.data_type}.")

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_data(self, Data data) except *:
        self.data_count += 1

        if isinstance(data, OrderBookData):
            self._handle_order_book_data(data)
        elif isinstance(data, Ticker):
            self._handle_ticker(data)
        elif isinstance(data, QuoteTick):
            self._handle_quote_tick(data)
        elif isinstance(data, TradeTick):
            self._handle_trade_tick(data)
        elif isinstance(data, Bar):
            self._handle_bar(data)
        elif isinstance(data, Instrument):
            self._handle_instrument(data)
        elif isinstance(data, VenueStatusUpdate):
            self._handle_venue_status_update(data)
        elif isinstance(data, InstrumentStatusUpdate):
            self._handle_instrument_status_update(data)
        elif isinstance(data, InstrumentClose):
            self._handle_close_price(data)
        elif isinstance(data, GenericData):
            self._handle_generic_data(data)
        else:
            self._log.error(f"Cannot handle data: unrecognized type {type(data)} {data}.")

    cdef void _handle_instrument(self, Instrument instrument) except *:
        self._cache.add_instrument(instrument)
        self._msgbus.publish_c(
            topic=f"data.instrument"
                  f".{instrument.id.venue}"
                  f".{instrument.id.symbol}",
            msg=instrument,
        )

    cdef void _handle_order_book_data(self, OrderBookData data) except *:
        self._msgbus.publish_c(
            topic=f"data.book.deltas"
                  f".{data.instrument_id.venue}"
                  f".{data.instrument_id.symbol}",
            msg=data,
        )

    cdef void _handle_ticker(self, Ticker ticker) except *:
        self._cache.add_ticker(ticker)
        self._msgbus.publish_c(
            topic=f"data.tickers"
                  f".{ticker.instrument_id.venue}"
                  f".{ticker.instrument_id.symbol}",
            msg=ticker,
        )

    cdef void _handle_quote_tick(self, QuoteTick tick) except *:
        self._cache.add_quote_tick(tick)
        self._msgbus.publish_c(
            topic=f"data.quotes"
                  f".{tick.instrument_id.venue}"
                  f".{tick.instrument_id.symbol}",
            msg=tick,
        )

    cdef void _handle_trade_tick(self, TradeTick tick) except *:
        self._cache.add_trade_tick(tick)
        self._msgbus.publish_c(
            topic=f"data.trades"
                  f".{tick.instrument_id.venue}"
                  f".{tick.instrument_id.symbol}",
            msg=tick,
        )

    cdef void _handle_bar(self, Bar bar) except *:
        cdef BarType bar_type = bar.bar_type

        cdef:
            Bar cached_bar
            Bar last_bar
            list bars
            int i
        if self._validate_data_sequence:
            last_bar = self._cache.bar(bar_type)
            if last_bar is not None:
                if bar.ts_event < last_bar.ts_event:
                    self._log.warning(
                        f"Bar {bar} was prior to last bar `ts_event` {last_bar.ts_event}.",
                    )
                    return  # `bar` is out of sequence
                if bar.ts_init < last_bar.ts_init:
                    self._log.warning(
                        f"Bar {bar} was prior to last bar `ts_init` {last_bar.ts_init}.",
                    )
                    return  # `bar` is out of sequence
                if bar.is_revision:
                    if bar.ts_event == last_bar.ts_event:
                        # Replace `last_bar`, previously cached bar will fall out of scope
                        self._cache._bars.get(bar_type)[0] = bar  # noqa
                    else:
                        self._log.warning(
                            f"Bar revision {bar} was not at last bar `ts_event` {last_bar.ts_event}.",
                        )
                        return  # Revision SHOULD be at `last_bar.ts_event`

        if not bar.is_revision:
            self._cache.add_bar(bar)

        self._msgbus.publish_c(topic=f"data.bars.{bar_type}", msg=bar)

    cdef void _handle_venue_status_update(self, VenueStatusUpdate data) except *:
        self._msgbus.publish_c(topic=f"data.status.{data.venue}", msg=data)

    cdef void _handle_instrument_status_update(self, InstrumentStatusUpdate data) except *:
        self._msgbus.publish_c(topic=f"data.status.{data.instrument_id.venue}.{data.instrument_id.symbol}", msg=data)

    cdef void _handle_close_price(self, InstrumentClose data) except *:
        self._msgbus.publish_c(topic=f"data.venue.close_price.{data.instrument_id}", msg=data)

    cdef void _handle_generic_data(self, GenericData data) except *:
        self._msgbus.publish_c(topic=f"data.{data.data_type.topic}", msg=data.data)

# -- RESPONSE HANDLERS ----------------------------------------------------------------------------

    cdef void _handle_response(self, DataResponse response) except *:
        if self.debug:
            self._log.debug(f"{RECV}{RES} {response}.", LogColor.MAGENTA)
        self.response_count += 1

        if response.data_type.type == Instrument:
            if isinstance(response.data, list):
                self._handle_instruments(response.data)
            else:
                self._handle_instrument(response.data)
        elif response.data_type.type == QuoteTick:
            self._handle_quote_ticks(response.data)
        elif response.data_type.type == TradeTick:
            self._handle_trade_ticks(response.data)
        elif response.data_type.type == Bar:
            self._handle_bars(response.data, response.data_type.metadata.get("Partial"))

        self._msgbus.response(response)

    cdef void _handle_instruments(self, list instruments) except *:
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cdef void _handle_quote_ticks(self, list ticks) except *:
        self._cache.add_quote_ticks(ticks)

    cdef void _handle_trade_ticks(self, list ticks) except *:
        self._cache.add_trade_ticks(ticks)

    cdef void _handle_bars(self, list bars, Bar partial) except *:
        self._cache.add_bars(bars)

        cdef TimeBarAggregator aggregator
        if partial is not None and partial.bar_type.is_internally_aggregated():
            # Update partial time bar
            aggregator = self._bar_aggregators.get(partial.bar_type)
            if aggregator:
                self._log.debug(f"Applying partial bar {partial} for {partial.bar_type}.")
                aggregator.set_partial(partial)
            else:
                if self._fsm.state == ComponentState.RUNNING:
                    # Only log this error if the component is running, because
                    # there may have been an immediate stop called after start
                    # - with the partial bar being for a now removed aggregator.
                    self._log.error("No aggregator for partial bar update.")

# -- INTERNAL -------------------------------------------------------------------------------------

    # Python wrapper to enable callbacks
    cpdef void _internal_update_instruments(self, list instruments: [Instrument]) except *:
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cpdef void _maintain_order_book(self, OrderBookData data) except *:
        cdef OrderBook order_book = self._cache.order_book(data.instrument_id)
        if order_book is None:
            self._log.error(
                f"Cannot maintain order book: "
                f"no book found for {data.instrument_id}.",
            )
            return

        order_book.apply(data)

    cpdef void _snapshot_order_book(self, TimeEvent snap_event) except *:
        cdef tuple pieces = snap_event.name.partition('_')[2].partition('_')
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(pieces[0])
        cdef int interval_ms = int(pieces[2])

        cdef OrderBook order_book = self._cache.order_book(instrument_id)
        if order_book:
            if order_book.ts_last == 0:
                self._log.debug("OrderBook not yet updated, skipping snapshot.")
                return

            self._msgbus.publish_c(
                topic=f"data.book.snapshots"
                      f".{instrument_id.venue}"
                      f".{instrument_id.symbol}"
                      f".{interval_ms}",
                msg=order_book,
            )

        else:
            self._log.error(
                f"Cannot snapshot orderbook: "
                f"no order book found, {snap_event}.",
            )

    cdef void _start_bar_aggregator(self, MarketDataClient client, BarType bar_type) except *:
        cdef Instrument instrument = self._cache.instrument(bar_type.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot start bar aggregation: "
                f"no instrument found for {bar_type.instrument_id}.",
            )

        if bar_type.spec.is_time_aggregated():
            # Create aggregator
            aggregator = TimeBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
                clock=self._clock,
                logger=self._log.get_logger(),
                build_with_no_updates=self._time_bars_build_with_no_updates,
                timestamp_on_close=self._time_bars_timestamp_on_close,
            )
        elif bar_type.spec.aggregation == BarAggregation.TICK:
            aggregator = TickBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
                logger=self._log.get_logger(),
            )
        elif bar_type.spec.aggregation == BarAggregation.VOLUME:
            aggregator = VolumeBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
                logger=self._log.get_logger(),
            )
        elif bar_type.spec.aggregation == BarAggregation.VALUE:
            aggregator = ValueBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
                logger=self._log.get_logger(),
            )
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"Cannot start aggregator: "  # pragma: no cover (design-time error)
                f"BarAggregation.{bar_type.spec.aggregation_string_c()} "  # pragma: no cover (design-time error)
                f"not supported in open-source"  # pragma: no cover (design-time error)
            )

        # Add aggregator
        self._bar_aggregators[bar_type] = aggregator
        self._log.debug(f"Added {aggregator} for {bar_type} bars.")

        # Subscribe to required data
        if bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.subscribe(
                topic=f"data.trades"
                      f".{bar_type.instrument_id.venue}"
                      f".{bar_type.instrument_id.symbol}",
                handler=aggregator.handle_trade_tick,
                priority=5,
            )
            self._handle_subscribe_trade_ticks(client, bar_type.instrument_id)
        else:
            self._msgbus.subscribe(
                topic=f"data.quotes"
                      f".{bar_type.instrument_id.venue}"
                      f".{bar_type.instrument_id.symbol}",
                handler=aggregator.handle_quote_tick,
                priority=5,
            )
            self._handle_subscribe_quote_ticks(client, bar_type.instrument_id)

    cdef void _stop_bar_aggregator(self, MarketDataClient client, BarType bar_type) except *:
        cdef aggregator = self._bar_aggregators.get(bar_type)
        if aggregator is None:
            self._log.warning(
                f"Cannot stop bar aggregator: "
                f"no aggregator to stop for {bar_type}",
            )
            return

        if isinstance(aggregator, TimeBarAggregator):
            aggregator.stop()

        # Unsubscribe from update ticks
        if bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.unsubscribe(
                topic=f"data.trades"
                      f".{bar_type.instrument_id.venue}"
                      f".{bar_type.instrument_id.symbol}",
                handler=aggregator.handle_trade_tick,
            )
            self._handle_unsubscribe_trade_ticks(client, bar_type.instrument_id)
        else:
            self._msgbus.unsubscribe(
                topic=f"data.quotes"
                      f".{bar_type.instrument_id.venue}"
                      f".{bar_type.instrument_id.symbol}",
                handler=aggregator.handle_quote_tick,
            )
            self._handle_unsubscribe_quote_ticks(client, bar_type.instrument_id)

        # Remove from aggregators
        del self._bar_aggregators[bar_type]
