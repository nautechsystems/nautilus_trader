# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Callable

from nautilus_trader.common.enums import LogColor
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.persistence.catalog import ParquetDataCatalog

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport RECV
from nautilus_trader.common.component cimport REQ
from nautilus_trader.common.component cimport RES
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_MILLISECOND
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_SECOND
from nautilus_trader.core.rust.core cimport millis_to_nanos
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.uuid cimport UUID4
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
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.data cimport VenueStatus
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


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
    config : DataEngineConfig, optional
        The configuration for the instance.
    """

    def __init__(
        self,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        config: DataEngineConfig | None = None,
    ):
        if config is None:
            config = DataEngineConfig()
        Condition.type(config, DataEngineConfig, "config")
        super().__init__(
            clock=clock,
            component_id=ComponentId("DataEngine"),
            msgbus=msgbus,
            config=config,
        )

        self._cache = cache

        self._clients: dict[ClientId, DataClient] = {}
        self._routing_map: dict[Venue, DataClient] = {}
        self._default_client: DataClient | None = None
        self._catalog: ParquetDataCatalog | None = None
        self._order_book_intervals: dict[(InstrumentId, int), list[Callable[[Bar], None]]] = {}
        self._bar_aggregators: dict[BarType, BarAggregator] = {}
        self._synthetic_quote_feeds: dict[InstrumentId, list[SyntheticInstrument]] = {}
        self._synthetic_trade_feeds: dict[InstrumentId, list[SyntheticInstrument]] = {}
        self._subscribed_synthetic_quotes: list[InstrumentId] = []
        self._subscribed_synthetic_trades: list[InstrumentId] = []

        # Settings
        self.debug = config.debug
        self._time_bars_build_with_no_updates = config.time_bars_build_with_no_updates
        self._time_bars_timestamp_on_close = config.time_bars_timestamp_on_close
        self._time_bars_interval_type = config.time_bars_interval_type
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
    def registered_clients(self) -> list[ClientId]:
        """
        Return the execution clients registered with the engine.

        Returns
        -------
        list[ClientId]

        """
        return sorted(list(self._clients.keys()))

    @property
    def default_client(self) -> ClientId | None:
        """
        Return the default data client registered with the engine.

        Returns
        -------
        ClientId or ``None``

        """
        return self._default_client.id if self._default_client is not None else None

    def connect(self) -> None:
        """
        Connect the engine by calling connect on all registered clients.
        """
        self._log.info("Connecting all clients...")
        # Implement actual client connections for a live/sandbox context

    def disconnect(self) -> None:
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        self._log.info("Disconnecting all clients...")
        # Implement actual client connections for a live/sandbox context

    cpdef bint check_connected(self):
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

    cpdef bint check_disconnected(self):
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

# --REGISTRATION ----------------------------------------------------------------------------------

    def register_catalog(self, catalog: ParquetDataCatalog) -> None:
        """
        Register the given data catalog with the engine.

        Parameters
        ----------
        catalog : ParquetDataCatalog
            The data catalog to register.

        """
        Condition.not_none(catalog, "catalog")

        self._catalog = catalog

    cpdef void register_client(self, DataClient client):
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

    cpdef void register_default_client(self, DataClient client):
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

    cpdef void register_venue_routing(self, DataClient client, Venue venue):
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

    cpdef void deregister_client(self, DataClient client):
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

    cpdef list subscribed_custom_data(self):
        """
        Return the custom data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        cdef list subscriptions = []
        cdef DataClient client
        for client in self._clients.values():
            subscriptions += client.subscribed_custom_data()
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

    cpdef list subscribed_instrument_status(self):
        """
        Return the status update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []
        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_instrument_status()
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

    cpdef list subscribed_synthetic_quotes(self):
        """
        Return the synthetic instrument quote ticks subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return self._subscribed_synthetic_quotes.copy()

    cpdef list subscribed_synthetic_trades(self):
        """
        Return the synthetic instrument trade ticks subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return self._subscribed_synthetic_trades.copy()

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self):
        pass  # Optionally override in subclass

    cpdef void _on_stop(self):
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _start(self):
        cdef DataClient client
        for client in self._clients.values():
            client.start()

        self._on_start()

    cpdef void _stop(self):
        cdef DataClient client
        for client in self._clients.values():
            client.stop()

        for aggregator in self._bar_aggregators.values():
            if isinstance(aggregator, TimeBarAggregator):
                aggregator.stop()

        self._on_stop()

    cpdef void _reset(self):
        cdef DataClient client
        for client in self._clients.values():
            client.reset()

        self._order_book_intervals.clear()
        self._bar_aggregators.clear()
        self._synthetic_quote_feeds.clear()
        self._synthetic_trade_feeds.clear()
        self._subscribed_synthetic_quotes.clear()
        self._subscribed_synthetic_trades.clear()

        self._clock.cancel_timers()
        self.command_count = 0
        self.data_count = 0
        self.request_count = 0
        self.response_count = 0

    cpdef void _dispose(self):
        cdef DataClient client
        for client in self._clients.values():
            client.dispose()

        self._clock.cancel_timers()

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void execute(self, DataCommand command):
        """
        Execute the given data command.

        Parameters
        ----------
        command : DataCommand
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, Data data):
        """
        Process the given data.

        Parameters
        ----------
        data : Data
            The data to process.

        """
        Condition.not_none(data, "data")

        self._handle_data(data)

    cpdef void request(self, DataRequest request):
        """
        Handle the given request.

        Parameters
        ----------
        request : DataRequest
            The request to handle.

        """
        Condition.not_none(request, "request")

        self._handle_request(request)

    cpdef void response(self, DataResponse response):
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

    cpdef void _execute_command(self, DataCommand command):
        if self.debug:
            self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        cdef Venue venue = command.venue
        cdef DataClient client = self._clients.get(command.client_id)
        if venue is not None and venue.is_synthetic():
            # No further check as no client needed
            pass
        elif client is None:
            client = self._routing_map.get(command.venue, self._default_client)
            if client is None:
                self._log.error(
                    f"Cannot execute command: "
                    f"no data client configured for {command.venue} or `client_id` {command.client_id}, "
                    f"{command}."
                )
                return  # No client to handle command

        if isinstance(command, Subscribe):
            self._handle_subscribe(client, command)
        elif isinstance(command, Unsubscribe):
            self._handle_unsubscribe(client, command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cpdef void _handle_subscribe(self, DataClient client, Subscribe command):
        if command.data_type.type == Instrument:
            self._handle_subscribe_instrument(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == OrderBookDelta:
            self._handle_subscribe_order_book_deltas(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata,
            )
        elif command.data_type.type == OrderBook:
            self._handle_subscribe_order_book_snapshots(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata,
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
                command.data_type.metadata.get("await_partial"),
            )
        elif command.data_type.type == VenueStatus:
            self._handle_subscribe_venue_status(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == InstrumentStatus:
            self._handle_subscribe_instrument_status(
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

    cpdef void _handle_unsubscribe(self, DataClient client, Unsubscribe command):
        if command.data_type.type == Instrument:
            self._handle_unsubscribe_instrument(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == OrderBook:
            self._handle_unsubscribe_order_book_snapshots(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata,
            )
        elif command.data_type.type == OrderBookDelta:
            self._handle_unsubscribe_order_book_deltas(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata,
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

    cpdef void _handle_subscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ):
        Condition.not_none(client, "client")

        if instrument_id is None:
            client.subscribe_instruments()
            return

        if instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `Instrument` data.")
            return

        if instrument_id not in client.subscribed_instruments():
            client.subscribe_instrument(instrument_id)

    cpdef void _handle_subscribe_order_book_deltas(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        if instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `OrderBookDelta` data.")
            return

        self._setup_order_book(
            client,
            instrument_id,
            metadata,
            only_deltas=True,
            managed=metadata["managed"]
        )

    cpdef void _handle_subscribe_order_book_snapshots(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        if instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `OrderBook` data.")
            return

        cdef:
            uint64_t interval_ms = metadata["interval_ms"]
            uint64_t interval_ns
            uint64_t timestamp_ns
        key = (instrument_id, interval_ms)
        if key not in self._order_book_intervals:
            self._order_book_intervals[key] = []

            timer_name = f"OrderBook|{instrument_id}|{interval_ms}"
            interval_ns = millis_to_nanos(interval_ms)
            timestamp_ns = self._clock.timestamp_ns()
            start_time_ns = timestamp_ns - (timestamp_ns % interval_ns)

            if start_time_ns - NANOSECONDS_IN_MILLISECOND <= self._clock.timestamp_ns():
                start_time_ns += NANOSECONDS_IN_SECOND  # Add one second

            self._clock.set_timer_ns(
                name=timer_name,
                interval_ns=interval_ns,
                start_time_ns=start_time_ns,
                stop_time_ns=0,  # No stop
                callback=self._snapshot_order_book,
            )
            self._log.debug(f"Set timer {timer_name}.")

        self._setup_order_book(
            client,
            instrument_id,
            metadata,
            only_deltas=False,
            managed=metadata["managed"]
        )

    cpdef void _setup_order_book(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
        bint only_deltas,
        bint managed,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        # Create order book
        if managed and not self._cache.has_order_book(instrument_id):
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot subscribe to {instrument_id} <OrderBook> data: "
                    f"no instrument found in the cache.",
                )
                return
            order_book = OrderBook(
                instrument_id=instrument.id,
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
            if only_deltas:
                raise
            if instrument_id not in client.subscribed_order_book_snapshots():
                client.subscribe_order_book_snapshots(
                    instrument_id=instrument_id,
                    book_type=metadata["book_type"],
                    depth=metadata["depth"],
                    kwargs=metadata.get("kwargs"),
                )

        # Setup subscriptions
        cdef str topic = f"data.book.deltas.{instrument_id.venue}.{instrument_id.symbol}"

        if not self._msgbus.is_subscribed(
            topic=topic,
            handler=self._update_order_book,
        ):
            self._msgbus.subscribe(
                topic=topic,
                handler=self._update_order_book,
                priority=10,
            )

        topic = f"data.book.depth.{instrument_id.venue}.{instrument_id.symbol}"

        if not only_deltas and not self._msgbus.is_subscribed(
            topic=topic,
            handler=self._update_order_book,
        ):
            self._msgbus.subscribe(
                topic=topic,
                handler=self._update_order_book,
                priority=10,
            )

    cpdef void _handle_subscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ):
        Condition.not_none(instrument_id, "instrument_id")
        if instrument_id.is_synthetic():
            self._handle_subscribe_synthetic_quote_ticks(instrument_id)
            return
        Condition.not_none(client, "client")

        if instrument_id not in client.subscribed_quote_ticks():
            client.subscribe_quote_ticks(instrument_id)

    cpdef void _handle_subscribe_synthetic_quote_ticks(self, InstrumentId instrument_id):
        cdef SyntheticInstrument synthetic = self._cache.synthetic(instrument_id)
        if synthetic is None:
            self._log.error(
                f"Cannot subscribe to `QuoteTick` data for synthetic instrument {instrument_id}, "
                " not found."
            )
            return

        if instrument_id in self._subscribed_synthetic_quotes:
            return  # Already setup

        cdef:
            InstrumentId component_instrument_id
            list synthetics_for_feed
        for component_instrument_id in synthetic.components:
            synthetics_for_feed = self._synthetic_quote_feeds.get(component_instrument_id)
            if synthetics_for_feed is None:
                synthetics_for_feed = []
            if synthetic in synthetics_for_feed:
                continue
            synthetics_for_feed.append(synthetic)
            self._synthetic_quote_feeds[component_instrument_id] = synthetics_for_feed

        self._subscribed_synthetic_quotes.append(instrument_id)

    cpdef void _handle_subscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ):
        Condition.not_none(instrument_id, "instrument_id")
        if instrument_id.is_synthetic():
            self._handle_subscribe_synthetic_trade_ticks(instrument_id)
            return
        Condition.not_none(client, "client")

        if instrument_id not in client.subscribed_trade_ticks():
            client.subscribe_trade_ticks(instrument_id)

    cpdef void _handle_subscribe_synthetic_trade_ticks(self, InstrumentId instrument_id):
        cdef SyntheticInstrument synthetic = self._cache.synthetic(instrument_id)
        if synthetic is None:
            self._log.error(
                f"Cannot subscribe to `TradeTick` data for synthetic instrument {instrument_id}, "
                " not found."
            )
            return

        if instrument_id in self._subscribed_synthetic_trades:
            return  # Already setup

        cdef:
            InstrumentId component_instrument_id
            list synthetics_for_feed
        for component_instrument_id in synthetic.components:
            synthetics_for_feed = self._synthetic_trade_feeds.get(component_instrument_id)
            if synthetics_for_feed is None:
                synthetics_for_feed = []
            if synthetic in synthetics_for_feed:
                continue
            synthetics_for_feed.append(synthetic)
            self._synthetic_trade_feeds[component_instrument_id] = synthetics_for_feed

        self._subscribed_synthetic_trades.append(instrument_id)

    cpdef void _handle_subscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
        bint await_partial,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")

        if bar_type.is_internally_aggregated():
            # Internal aggregation
            if bar_type not in self._bar_aggregators:
                self._start_bar_aggregator(client, bar_type, await_partial)
        else:
            # External aggregation
            if bar_type.instrument_id.is_synthetic():
                self._log.error(
                    "Cannot subscribe for externally aggregated synthetic instrument bar data.",
                )
                return

            if bar_type not in client.subscribed_bars():
                client.subscribe_bars(bar_type)

    cpdef void _handle_subscribe_data(
        self,
        DataClient client,
        DataType data_type,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(data_type, "data_type")

        try:
            if data_type not in client.subscribed_custom_data():
                client.subscribe(data_type)
        except NotImplementedError:
            self._log.error(
                f"Cannot subscribe: {client.id.value} "
                f"has not implemented {data_type} subscriptions.",
            )
            return

    cpdef void _handle_subscribe_venue_status(
        self,
        MarketDataClient client,
        Venue venue,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(venue, "venue")

        if venue not in client.subscribed_venue_status():
            client.subscribe_venue_status(venue)

    cpdef void _handle_subscribe_instrument_status(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id.is_synthetic():
            self._log.error(
                "Cannot subscribe for synthetic instrument `InstrumentStatus` data.",
            )
            return

        if instrument_id not in client.subscribed_instrument_status():
            client.subscribe_instrument_status(instrument_id)

    cpdef void _handle_subscribe_instrument_close(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `InstrumentClose` data.")
            return

        if instrument_id not in client.subscribed_instrument_close():
            client.subscribe_instrument_close(instrument_id)

    cpdef void _handle_unsubscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ):
        Condition.not_none(client, "client")

        if instrument_id is None:
            if not self._msgbus.has_subscribers(f"data.instrument.{client.id.value}.*"):
                client.unsubscribe_instruments()
            return
        else:
            if instrument_id.is_synthetic():
                self._log.error("Cannot unsubscribe from synthetic instrument `Instrument` data.")
                return

            if not self._msgbus.has_subscribers(
                f"data.instrument"
                f".{instrument_id.venue}"
                f".{instrument_id.symbol}",
            ):
                client.unsubscribe_instrument(instrument_id)

    cpdef void _handle_unsubscribe_order_book_deltas(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        if instrument_id.is_synthetic():
            self._log.error("Cannot unsubscribe from synthetic instrument `OrderBookDelta` data.")
            return

        if not self._msgbus.has_subscribers(
            f"data.book.deltas"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_order_book_deltas(instrument_id)

    cpdef void _handle_unsubscribe_order_book_snapshots(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        if instrument_id.is_synthetic():
            self._log.error("Cannot unsubscribe from synthetic instrument `OrderBook` data.")
            return

        if not self._msgbus.has_subscribers(
            f"data.book.snapshots"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_order_book_snapshots(instrument_id)

    cpdef void _handle_unsubscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if not self._msgbus.has_subscribers(
            f"data.quotes"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_quote_ticks(instrument_id)

    cpdef void _handle_unsubscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if not self._msgbus.has_subscribers(
            f"data.trades"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            client.unsubscribe_trade_ticks(instrument_id)

    cpdef void _handle_unsubscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")

        if self._msgbus.has_subscribers(f"data.bars.{bar_type}"):
            return

        if bar_type.is_internally_aggregated():
            # Internal aggregation
            if bar_type in self._bar_aggregators:
                self._stop_bar_aggregator(client, bar_type)
        else:
            # External aggregation
            if bar_type in client.subscribed_bars():
                client.unsubscribe_bars(bar_type)

    cpdef void _handle_unsubscribe_data(
        self,
        DataClient client,
        DataType data_type,
    ):
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

    cpdef void _handle_request(self, DataRequest request):
        if self.debug:
            self._log.debug(f"{RECV}{REQ} {request}.", LogColor.MAGENTA)
        self.request_count += 1

        # Query data catalog
        if self._catalog:
            # For now we'll just query the catalog if its present (as very likely this is a backtest)
            self._query_catalog(request)
            return

        # Query data client
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
                client.request_instruments(
                    request.data_type.metadata.get("venue"),
                    request.id,
                    request.data_type.metadata.get("start"),
                    request.data_type.metadata.get("end"),
                )
            else:
                client.request_instrument(
                    instrument_id,
                    request.id,
                    request.data_type.metadata.get("start"),
                    request.data_type.metadata.get("end"),
                )
        elif request.data_type.type == QuoteTick:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_quote_ticks(
                request.data_type.metadata.get("instrument_id"),
                request.data_type.metadata.get("limit", 0),
                request.id,
                request.data_type.metadata.get("start"),
                request.data_type.metadata.get("end"),
            )
        elif request.data_type.type == TradeTick:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_trade_ticks(
                request.data_type.metadata.get("instrument_id"),
                request.data_type.metadata.get("limit", 0),
                request.id,
                request.data_type.metadata.get("start"),
                request.data_type.metadata.get("end"),
            )
        elif request.data_type.type == Bar:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_bars(
                request.data_type.metadata.get("bar_type"),
                request.data_type.metadata.get("limit", 0),
                request.id,
                request.data_type.metadata.get("start"),
                request.data_type.metadata.get("end"),
            )
        else:
            try:
                client.request(request.data_type, request.id)
            except NotImplementedError:
                self._log.error(f"Cannot handle request: unrecognized data type {request.data_type}.")

    cpdef void _query_catalog(self, DataRequest request):
        cdef datetime start = request.data_type.metadata.get("start")
        cdef datetime end = request.data_type.metadata.get("end")

        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef uint64_t ts_start = dt_to_unix_nanos(start) if start is not None else 0
        cdef uint64_t ts_end = dt_to_unix_nanos(end) if end is not None else ts_now

        # Validate request time range
        Condition.true(ts_start <= ts_end, f"{ts_start=} was greater than {ts_end=}")

        if end is not None and ts_end > ts_now:
            self._log.warning(
                "Cannot request data beyond current time. "
                f"Truncating `end` to current UNIX nanoseconds {unix_nanos_to_dt(ts_now)}.",
            )
            ts_end = ts_now

        if request.data_type.type == Instrument:
            instrument_id = request.data_type.metadata.get("instrument_id")
            if instrument_id is None:
                data = self._catalog.instruments()
            else:
                data = self._catalog.instruments(instrument_ids=[str(instrument_id)])
        elif request.data_type.type == QuoteTick:
            data = self._catalog.quote_ticks(
                instrument_ids=[str(request.data_type.metadata.get("instrument_id"))],
                start=ts_start,
                end=ts_end,
            )
        elif request.data_type.type == TradeTick:
            data = self._catalog.trade_ticks(
                instrument_ids=[str(request.data_type.metadata.get("instrument_id"))],
                start=ts_start,
                end=ts_end,
            )
        elif request.data_type.type == Bar:
            bar_type = request.data_type.metadata.get("bar_type")
            if bar_type is None:
                self._log.error("No bar type provided for bars request.")
                return
            data = self._catalog.bars(
                instrument_ids=[str(bar_type.instrument_id)],
                bar_type=str(bar_type),
                start=ts_start,
                end=ts_end,
            )
        elif request.data_type.type == InstrumentClose:
            data = self._catalog.instrument_closes(
                instrument_ids=[str(request.data_type.metadata.get("instrument_id"))],
                start=ts_start,
                end=ts_end,
            )
        else:
            data = self._catalog.custom_data(
                cls=request.data_type.type,
                metadata=request.data_type.metadata,
                start=ts_start,
                end=ts_end,
            )

        # Validation data is not from the future
        if data and data[-1].ts_init > ts_now:
            raise RuntimeError(
                "Invalid response: Historical data from the future: "
                f"data[-1].ts_init={data[-1].ts_init}, {ts_now=}",
            )

        response = DataResponse(
            client_id=request.client_id,
            venue=request.venue,
            data_type=request.data_type,
            data=data,
            correlation_id=request.id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        self._handle_response(response)

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_data(self, Data data):
        self.data_count += 1

        if isinstance(data, OrderBookDelta):
            self._handle_order_book_delta(data)
        elif isinstance(data, OrderBookDeltas):
            self._handle_order_book_deltas(data)
        elif isinstance(data, OrderBookDepth10):
            self._handle_order_book_depth(data)
        elif isinstance(data, QuoteTick):
            self._handle_quote_tick(data)
        elif isinstance(data, TradeTick):
            self._handle_trade_tick(data)
        elif isinstance(data, Bar):
            self._handle_bar(data)
        elif isinstance(data, Instrument):
            self._handle_instrument(data)
        elif isinstance(data, VenueStatus):
            self._handle_venue_status(data)
        elif isinstance(data, InstrumentStatus):
            self._handle_instrument_status(data)
        elif isinstance(data, InstrumentClose):
            self._handle_close_price(data)
        elif isinstance(data, CustomData):
            self._handle_custom_data(data)
        else:
            self._log.error(f"Cannot handle data: unrecognized type {type(data)} {data}.")

    cpdef void _handle_instrument(self, Instrument instrument):
        self._cache.add_instrument(instrument)
        self._msgbus.publish_c(
            topic=f"data.instrument"
                  f".{instrument.id.venue}"
                  f".{instrument.id.symbol}",
            msg=instrument,
        )

    cpdef void _handle_order_book_delta(self, OrderBookDelta delta):
        cdef OrderBookDeltas deltas = OrderBookDeltas(
            instrument_id=delta.instrument_id,
            deltas=[delta]
        )
        self._msgbus.publish_c(
            topic=f"data.book.deltas"
                  f".{deltas.instrument_id.venue}"
                  f".{deltas.instrument_id.symbol}",
            msg=deltas,
        )

    cpdef void _handle_order_book_deltas(self, OrderBookDeltas deltas):
        self._msgbus.publish_c(
            topic=f"data.book.deltas"
                  f".{deltas.instrument_id.venue}"
                  f".{deltas.instrument_id.symbol}",
            msg=deltas,
        )

    cpdef void _handle_order_book_depth(self, OrderBookDepth10 depth):
        self._msgbus.publish_c(
            topic=f"data.book.depth"
                  f".{depth.instrument_id.venue}"
                  f".{depth.instrument_id.symbol}",
            msg=depth,
        )

    cpdef void _handle_quote_tick(self, QuoteTick tick):
        self._cache.add_quote_tick(tick)

        # Handle synthetics update
        cdef list synthetics = self._synthetic_quote_feeds.get(tick.instrument_id)
        if synthetics is not None:
            self._update_synthetics_with_quote(synthetics, tick)

        self._msgbus.publish_c(
            topic=f"data.quotes"
                  f".{tick.instrument_id.venue}"
                  f".{tick.instrument_id.symbol}",
            msg=tick,
        )

    cpdef void _handle_trade_tick(self, TradeTick tick):
        self._cache.add_trade_tick(tick)

        # Handle synthetics update
        cdef list synthetics = self._synthetic_trade_feeds.get(tick.instrument_id)
        if synthetics is not None:
            self._update_synthetics_with_trade(synthetics, tick)

        self._msgbus.publish_c(
            topic=f"data.trades"
                  f".{tick.instrument_id.venue}"
                  f".{tick.instrument_id.symbol}",
            msg=tick,
        )

    cpdef void _handle_bar(self, Bar bar):
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
                    elif bar.ts_event > last_bar.ts_event:
                        # Bar is latest, consider as new bar
                        self._cache.add_bar(bar)
                    else:
                        self._log.warning(
                            f"Bar revision {bar} was not at last bar `ts_event` {last_bar.ts_event}.",
                        )
                        return  # Revision SHOULD be at `last_bar.ts_event`

        if not bar.is_revision:
            self._cache.add_bar(bar)

        self._msgbus.publish_c(topic=f"data.bars.{bar_type}", msg=bar)

    cpdef void _handle_venue_status(self, VenueStatus data):
        self._msgbus.publish_c(topic=f"data.status.{data.venue}", msg=data)

    cpdef void _handle_instrument_status(self, InstrumentStatus data):
        self._msgbus.publish_c(topic=f"data.status.{data.instrument_id.venue}.{data.instrument_id.symbol}", msg=data)

    cpdef void _handle_close_price(self, InstrumentClose data):
        self._msgbus.publish_c(topic=f"data.venue.close_price.{data.instrument_id}", msg=data)

    cpdef void _handle_custom_data(self, CustomData data):
        self._msgbus.publish_c(topic=f"data.{data.data_type.topic}", msg=data.data)

# -- RESPONSE HANDLERS ----------------------------------------------------------------------------

    cpdef void _handle_response(self, DataResponse response):
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

    cpdef void _handle_instruments(self, list instruments):
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cpdef void _handle_quote_ticks(self, list ticks):
        self._cache.add_quote_ticks(ticks)

    cpdef void _handle_trade_ticks(self, list ticks):
        self._cache.add_trade_ticks(ticks)

    cpdef void _handle_bars(self, list bars, Bar partial):
        self._cache.add_bars(bars)

        cdef BarAggregator aggregator
        if partial is not None and partial.bar_type.is_internally_aggregated():
            # Update partial time bar
            aggregator = self._bar_aggregators.get(partial.bar_type)
            aggregator.set_await_partial(False)

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
    cpdef void _internal_update_instruments(self, list instruments: [Instrument]):
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cpdef void _update_order_book(self, Data data):
        cdef OrderBook order_book = self._cache.order_book(data.instrument_id)
        if order_book is None:
            # TODO: Silence error for now (book may be managed manually)
            # self._log.error(
            #     "Cannot update order book: "
            #     f"no book found for {data.instrument_id}.",
            # )
            return

        order_book.apply(data)

    cpdef void _snapshot_order_book(self, TimeEvent snap_event):
        cdef tuple[str] parts = snap_event.name.partition('|')[2].rpartition('|')
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(parts[0])
        cdef int interval_ms = int(parts[2])

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

    cpdef void _start_bar_aggregator(
        self,
        MarketDataClient client,
        BarType bar_type,
        bint await_partial,
    ):
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
                build_with_no_updates=self._time_bars_build_with_no_updates,
                timestamp_on_close=self._time_bars_timestamp_on_close,
                interval_type=self._time_bars_interval_type,
            )
        elif bar_type.spec.aggregation == BarAggregation.TICK:
            aggregator = TickBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.VOLUME:
            aggregator = VolumeBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.VALUE:
            aggregator = ValueBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
            )
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"Cannot start aggregator: "  # pragma: no cover (design-time error)
                f"BarAggregation.{bar_type.spec.aggregation_string_c()} "  # pragma: no cover (design-time error)
                f"not supported in open-source"  # pragma: no cover (design-time error)
            )

        # Set if awaiting initial partial bar
        aggregator.set_await_partial(await_partial)

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

    cpdef void _stop_bar_aggregator(self, MarketDataClient client, BarType bar_type):
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

    cpdef void _update_synthetics_with_quote(self, list synthetics, QuoteTick update):
        cdef SyntheticInstrument synthetic
        for synthetic in synthetics:
            self._update_synthetic_with_quote(synthetic, update)

    cpdef void _update_synthetic_with_quote(self, SyntheticInstrument synthetic, QuoteTick update):
        cdef list components = synthetic.components
        cdef list[double] inputs_bid = []
        cdef list[double] inputs_ask = []

        cdef:
            InstrumentId instrument_id
            QuoteTick component_quote
            Price update_bid
            Price update_ask
        for instrument_id in components:
            if instrument_id == update.instrument_id:
                update_bid = update.bid_price
                update_ask = update.ask_price
                inputs_bid.append(update_bid.as_f64_c())
                inputs_ask.append(update_ask.as_f64_c())
                continue
            component_quote = self._cache.quote_tick(instrument_id)
            if component_quote is None:
                self._log.warning(
                    f"Cannot calculate synthetic instrument {synthetic.id} price, "
                    f"no quotes for {instrument_id} yet...",
                )
                return
            update_bid = component_quote.bid_price
            update_ask = component_quote.ask_price
            inputs_bid.append(update_bid.as_f64_c())
            inputs_ask.append(update_ask.as_f64_c())

        cdef Price bid_price = synthetic.calculate(inputs_bid)
        cdef Price ask_price = synthetic.calculate(inputs_ask)
        cdef Quantity size_one = Quantity(1, 0)  # Placeholder for now

        cdef InstrumentId synthetic_instrument_id = synthetic.id
        cdef QuoteTick synthetic_quote = QuoteTick(
            synthetic_instrument_id,
            bid_price,
            ask_price,
            size_one,
            size_one,
            update.ts_event,
            self._clock.timestamp_ns(),
        )

        self._msgbus.publish_c(
            topic=f"data.quotes"
                  f".{synthetic_instrument_id.venue}"
                  f".{synthetic_instrument_id.symbol}",
            msg=synthetic_quote,
        )

    cpdef void _update_synthetics_with_trade(self, list synthetics, TradeTick update):
        cdef SyntheticInstrument synthetic
        for synthetic in synthetics:
            self._update_synthetic_with_trade(synthetic, update)

    cpdef void _update_synthetic_with_trade(self, SyntheticInstrument synthetic, TradeTick update):
        cdef list components = synthetic.components
        cdef list[double] inputs = []

        cdef:
            InstrumentId instrument_id
            TradeTick component_quote
            Price update_price
        for instrument_id in components:
            if instrument_id == update.instrument_id:
                update_price = update.price
                inputs.append(update_price.as_f64_c())
                continue
            component_trade = self._cache.trade_tick(instrument_id)
            if component_trade is None:
                self._log.warning(
                    f"Cannot calculate synthetic instrument {synthetic.id} price, "
                    f"no trades for {instrument_id} yet...",
                )
                return
            update_price = component_trade.price
            inputs.append(update_price.as_f64_c())

        cdef Price price = synthetic.calculate(inputs)
        cdef Quantity size_one = Quantity(1, 0)  # Placeholder for now

        cdef InstrumentId synthetic_instrument_id = synthetic.id
        cdef TradeTick synthetic_trade = TradeTick(
            synthetic_instrument_id,
            price,
            size_one,
            update.aggressor_side,
            update.trade_id,
            update.ts_event,
            self._clock.timestamp_ns(),
        )

        self._msgbus.publish_c(
            topic=f"data.trades"
                  f".{synthetic_instrument_id.venue}"
                  f".{synthetic_instrument_id.symbol}",
            msg=synthetic_trade,
        )
