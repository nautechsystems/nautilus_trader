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
from nautilus_trader.core.datetime import max_date
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.persistence.catalog import ParquetDataCatalog

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport RECV
from nautilus_trader.common.component cimport REQ
from nautilus_trader.common.component cimport RES
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TestClock
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_MILLISECOND
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_SECOND
from nautilus_trader.core.rust.core cimport millis_to_nanos
from nautilus_trader.core.rust.model cimport BookType
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
    ) -> None:
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
        self._external_clients: set[ClientId] = set()
        self._catalogs: dict[str, ParquetDataCatalog] = {}
        self._order_book_intervals: dict[tuple[InstrumentId, int], list[Callable[[OrderBook], None]]] = {}
        self._bar_aggregators: dict[BarType, BarAggregator] = {}
        self._synthetic_quote_feeds: dict[InstrumentId, list[SyntheticInstrument]] = {}
        self._synthetic_trade_feeds: dict[InstrumentId, list[SyntheticInstrument]] = {}
        self._subscribed_synthetic_quotes: list[InstrumentId] = []
        self._subscribed_synthetic_trades: list[InstrumentId] = []
        self._buffered_deltas_map: dict[InstrumentId, list[OrderBookDelta]] = {}
        self._snapshot_info: dict[str, SnapshotInfo] = {}
        self._query_group_n_components: dict[UUID4, int] = {}
        self._query_group_components: dict[UUID4, list] = {}

        # Settings
        self.debug = config.debug
        self._time_bars_build_with_no_updates = config.time_bars_build_with_no_updates
        self._time_bars_timestamp_on_close = config.time_bars_timestamp_on_close
        self._time_bars_interval_type = config.time_bars_interval_type
        self._time_bars_origins = config.time_bars_origins or {}
        self._validate_data_sequence = config.validate_data_sequence
        self._buffer_deltas = config.buffer_deltas

        if config.external_clients:
            self._external_clients = set(config.external_clients)

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

    def register_catalog(self, catalog: ParquetDataCatalog, name: str = "catalog_0") -> None:
        """
        Register the given data catalog with the engine.

        Parameters
        ----------
        catalog : ParquetDataCatalog
            The data catalog to register.
        name : str, default 'catalog_0'
            The name of the catalog to register.

        """
        Condition.not_none(catalog, "catalog")

        self._catalogs[name] = catalog

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

        self._log.info(f"Registered {client}{routing_log}")

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

        self._log.info(f"Registered {client} for default routing")

    cpdef void register_venue_routing(self, DataClient client, Venue venue):
        """
        Register the given client to route messages to the given venue.

        Any existing client in the routing map for the given venue will be
        overwritten.

        Parameters
        ----------
        venue : Venue
            The venue to route messages to.
        client : DataClient
            The client for the venue routing.

        """
        Condition.not_none(client, "client")
        Condition.not_none(venue, "venue")

        if client.id not in self._clients:
            self._clients[client.id] = client

        self._routing_map[venue] = client

        self._log.info(f"Registered DataClient-{client} for routing to {venue}")

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
        self._log.info(f"Deregistered {client}")

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
        Return the synthetic instrument quotes subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return self._subscribed_synthetic_quotes.copy()

    cpdef list subscribed_synthetic_trades(self):
        """
        Return the synthetic instrument trades subscribed to.

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
        for client in self._clients.values():
            client.start()

        self._on_start()

    cpdef void _stop(self):
        for client in self._clients.values():
            if client.is_running:
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
        self._buffered_deltas_map.clear()
        self._snapshot_info.clear()

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

    cpdef void stop_clients(self):
        """
        Stop the registered clients.
        """
        for client in self._clients.values():
            client.stop()

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
            self._log.debug(f"{RECV}{CMD} {command}", LogColor.MAGENTA)
        self.command_count += 1

        if command.client_id in self._external_clients:
            self._msgbus.add_streaming_type(command.data_type.type)
            self._log.debug(
                f"{command.client_id} declared as external client - disregarding subscription command",
            )
            return

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
                    f"{command}"
                )
                return  # No client to handle command

        if isinstance(command, Subscribe):
            self._handle_subscribe(client, command)
        elif isinstance(command, Unsubscribe):
            self._handle_unsubscribe(client, command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}")

    cpdef void _handle_subscribe(self, DataClient client, Subscribe command):
        if command.data_type.type == Instrument:
            self._handle_subscribe_instrument(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == OrderBookDelta:
            self._handle_subscribe_order_book_deltas(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata.get("book_type"),
                command.data_type.metadata.get("depth", 0),
                command.params.get("managed", True),
                command.params,
            )
        elif command.data_type.type == OrderBook:
            self._handle_subscribe_order_book(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.data_type.metadata.get("book_type"),
                command.data_type.metadata.get("depth", 0),
                command.params.get("interval_ms", 1_000),  # TODO: Temporary default
                command.params.get("managed", True),
                command.params,
            )
        elif command.data_type.type == QuoteTick:
            self._handle_subscribe_quote_ticks(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == TradeTick:
            self._handle_subscribe_trade_ticks(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == Bar:
            self._handle_subscribe_bars(
                client,
                command.data_type.metadata.get("bar_type"),
                command.params.get("await_partial"),
                command.params,
            )
        elif command.data_type.type == InstrumentStatus:
            self._handle_subscribe_instrument_status(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == InstrumentClose:
            self._handle_subscribe_instrument_close(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        else:
            self._handle_subscribe_data(client, command.data_type)

    cpdef void _handle_unsubscribe(self, DataClient client, Unsubscribe command):
        if command.data_type.type == Instrument:
            self._handle_unsubscribe_instrument(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == OrderBook:
            self._handle_unsubscribe_order_book(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == OrderBookDelta:
            self._handle_unsubscribe_order_book_deltas(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == QuoteTick:
            self._handle_unsubscribe_quote_ticks(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == TradeTick:
            self._handle_unsubscribe_trade_ticks(
                client,
                command.data_type.metadata.get("instrument_id"),
                command.params,
            )
        elif command.data_type.type == Bar:
            self._handle_unsubscribe_bars(
                client,
                command.data_type.metadata.get("bar_type"),
                command.params,
            )
        else:
            self._handle_unsubscribe_data(client, command.data_type)

    cpdef void _handle_subscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict params,
    ):
        Condition.not_none(client, "client")

        if instrument_id is None:
            client.subscribe_instruments()
            return

        if instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `Instrument` data")
            return

        if instrument_id not in client.subscribed_instruments():
            client.subscribe_instrument(instrument_id, params)

    cpdef void _handle_subscribe_order_book_deltas(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        BookType book_type,
        uint64_t depth,
        bint managed,
        dict params,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(params, "params")

        if instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `OrderBookDelta` data")
            return

        self._setup_order_book(
            client,
            instrument_id,
            book_type=book_type,
            depth=depth,
            only_deltas=True,
            managed=managed,
            metadata=params,
        )

    cpdef void _handle_subscribe_order_book(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        BookType book_type,
        uint64_t depth,
        uint64_t interval_ms,
        bint managed,
        dict params,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.positive_int(interval_ms, "interval_ms")
        Condition.not_none(params, "params")

        if instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `OrderBook` data")
            return


        cdef:
            uint64_t interval_ns
            uint64_t timestamp_ns
            SnapshotInfo snap_info
        key = (instrument_id, interval_ms)
        if key not in self._order_book_intervals:
            self._order_book_intervals[key] = []

            timer_name = f"OrderBook|{instrument_id}|{interval_ms}"
            interval_ns = millis_to_nanos(interval_ms)
            timestamp_ns = self._clock.timestamp_ns()
            start_time_ns = timestamp_ns - (timestamp_ns % interval_ns)

            topic = f"data.book.snapshots.{instrument_id.venue}.{instrument_id.symbol}.{interval_ms}"

            # Cache snapshot event info
            snap_info = SnapshotInfo.__new__(SnapshotInfo)
            snap_info.instrument_id = instrument_id
            snap_info.venue = instrument_id.venue
            snap_info.is_composite = instrument_id.symbol.is_composite()
            snap_info.root = instrument_id.symbol.root()
            snap_info.topic = topic
            snap_info.interval_ms = interval_ms

            self._snapshot_info[timer_name] = snap_info

            if start_time_ns - NANOSECONDS_IN_MILLISECOND <= self._clock.timestamp_ns():
                start_time_ns += NANOSECONDS_IN_SECOND  # Add one second

            self._clock.set_timer_ns(
                name=timer_name,
                interval_ns=interval_ns,
                start_time_ns=start_time_ns,
                stop_time_ns=0,  # No stop
                callback=self._snapshot_order_book,
            )
            self._log.debug(f"Set timer {timer_name}")

        self._setup_order_book(
            client,
            instrument_id,
            book_type=book_type,
            depth=depth,
            only_deltas=False,
            managed=managed,
            metadata=params,
        )

    cpdef void _setup_order_book(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        BookType book_type,
        uint64_t depth,
        bint only_deltas,
        bint managed,
        dict params,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(params, "params")

        cdef:
            list[Instrument] instruments
            Instrument instrument
            str root
        if managed:
            # Create order book(s)
            if instrument_id.symbol.is_composite():
                root = instrument_id.symbol.root()
                instruments = self._cache.instruments(venue=instrument_id.venue, underlying=root)
                for instrument in instruments:
                    self._create_new_book(instrument, book_type)
            else:
                instrument = self._cache.instrument(instrument_id)
                self._create_new_book(instrument, book_type)

        # Always re-subscribe to override previous settings
        try:
            if instrument_id not in client.subscribed_order_book_deltas():
                client.subscribe_order_book_deltas(
                    instrument_id=instrument_id,
                    book_type=book_type,
                    depth=depth,
                    metadata=params,
                )
        except NotImplementedError:
            if only_deltas:
                raise
            if instrument_id not in client.subscribed_order_book_snapshots():
                client.subscribe_order_book_snapshots(
                    instrument_id=instrument_id,
                    book_type=book_type,
                    depth=depth,
                    metadata=params,
                )

        # Set up subscriptions
        cdef str topic = f"data.book.deltas.{instrument_id.venue}.{instrument_id.symbol.topic()}"

        if not self._msgbus.is_subscribed(
            topic=topic,
            handler=self._update_order_book,
        ):
            self._msgbus.subscribe(
                topic=topic,
                handler=self._update_order_book,
                priority=10,
            )

        topic = f"data.book.depth.{instrument_id.venue}.{instrument_id.symbol.topic()}"

        if not only_deltas and not self._msgbus.is_subscribed(
            topic=topic,
            handler=self._update_order_book,
        ):
            self._msgbus.subscribe(
                topic=topic,
                handler=self._update_order_book,
                priority=10,
            )

    cpdef void _create_new_book(self, Instrument instrument, BookType book_type):
        if instrument is None:
            self._log.error(
                f"Cannot subscribe to {instrument.id} <OrderBook> data: "
                f"no instrument found in the cache",
            )
            return
        order_book = OrderBook(
            instrument_id=instrument.id,
            book_type=book_type,
        )

        self._cache.add_order_book(order_book)
        self._log.debug(f"Created {type(order_book).__name__} for {instrument.id}")

    cpdef void _handle_subscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(instrument_id, "instrument_id")
        if instrument_id.is_synthetic():
            self._handle_subscribe_synthetic_quote_ticks(instrument_id)
            return
        Condition.not_none(client, "client")

        if instrument_id not in client.subscribed_quote_ticks():
            client.subscribe_quote_ticks(instrument_id, metadata)

    cpdef void _handle_subscribe_synthetic_quote_ticks(self, InstrumentId instrument_id):
        cdef SyntheticInstrument synthetic = self._cache.synthetic(instrument_id)
        if synthetic is None:
            self._log.error(
                f"Cannot subscribe to `QuoteTick` data for synthetic instrument {instrument_id}, "
                " not found"
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
        dict metadata
    ):
        Condition.not_none(instrument_id, "instrument_id")
        if instrument_id.is_synthetic():
            self._handle_subscribe_synthetic_trade_ticks(instrument_id)
            return
        Condition.not_none(client, "client")

        if instrument_id not in client.subscribed_trade_ticks():
            client.subscribe_trade_ticks(instrument_id, metadata)

    cpdef void _handle_subscribe_synthetic_trade_ticks(self, InstrumentId instrument_id):
        cdef SyntheticInstrument synthetic = self._cache.synthetic(instrument_id)
        if synthetic is None:
            self._log.error(
                f"Cannot subscribe to `TradeTick` data for synthetic instrument {instrument_id}, "
                " not found"
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
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")

        if bar_type.is_internally_aggregated():
            # Internal aggregation
            if bar_type.standard() not in self._bar_aggregators:
                self._start_bar_aggregator(client, bar_type, await_partial, metadata)
        else:
            # External aggregation
            if bar_type.instrument_id.is_synthetic():
                self._log.error(
                    "Cannot subscribe for externally aggregated synthetic instrument bar data",
                )
                return

            if bar_type not in client.subscribed_bars():
                client.subscribe_bars(bar_type, metadata)

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
                f"has not implemented {data_type} subscriptions",
            )
            return

    cpdef void _handle_subscribe_instrument_status(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id.is_synthetic():
            self._log.error(
                "Cannot subscribe for synthetic instrument `InstrumentStatus` data",
            )
            return

        if instrument_id not in client.subscribed_instrument_status():
            client.subscribe_instrument_status(instrument_id, metadata)

    cpdef void _handle_subscribe_instrument_close(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `InstrumentClose` data")
            return

        if instrument_id not in client.subscribed_instrument_close():
            client.subscribe_instrument_close(instrument_id, metadata)

    cpdef void _handle_unsubscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")

        if instrument_id is None:
            if not self._msgbus.has_subscribers(f"data.instrument.{client.id.value}.*"):
                if client.subscribed_instruments():
                    client.unsubscribe_instruments(metadata)
            return
        else:
            if instrument_id.is_synthetic():
                self._log.error("Cannot unsubscribe from synthetic instrument `Instrument` data")
                return

            if not self._msgbus.has_subscribers(
                f"data.instrument"
                f".{instrument_id.venue}"
                f".{instrument_id.symbol}",
            ):
                if instrument_id in client.subscribed_instruments():
                    client.unsubscribe_instrument(instrument_id, metadata)

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
            self._log.error("Cannot unsubscribe from synthetic instrument `OrderBookDelta` data")
            return

        cdef str topic = f"data.book.deltas.{instrument_id.venue}.{instrument_id.symbol.topic()}"

        cdef int num_subscribers = len(self._msgbus.subscriptions(pattern=topic))
        cdef bint is_internal_book_subscriber = self._msgbus.is_subscribed(
            topic=topic,
            handler=self._update_order_book,
        )

        # Remove the subscription for the internal order book if it is the last subscription
        if num_subscribers == 1 and is_internal_book_subscriber:
            self._msgbus.unsubscribe(
                topic=topic,
                handler=self._update_order_book,
            )

        if not self._msgbus.has_subscribers(topic):
            if instrument_id in client.subscribed_order_book_deltas():
                client.unsubscribe_order_book_deltas(instrument_id, metadata)

    cpdef void _handle_unsubscribe_order_book(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")

        if instrument_id.is_synthetic():
            self._log.error("Cannot unsubscribe from synthetic instrument `OrderBook` data")
            return

        # Set up topics
        cdef str deltas_topic = f"data.book.deltas.{instrument_id.venue}.{instrument_id.symbol.topic()}"
        cdef str depth_topic = f"data.book.depth.{instrument_id.venue}.{instrument_id.symbol.topic()}"
        cdef str snapshots_topic = f"data.book.snapshots.{instrument_id.venue}.{instrument_id.symbol.topic()}"

        # Check the deltas and the depth subscription
        cdef list[str] topics = [deltas_topic, depth_topic]

        cdef int num_subscribers = 0
        cdef bint is_internal_book_subscriber = False

        for topic in topics:
            num_subscribers = len(self._msgbus.subscriptions(pattern=topic))
            is_internal_book_subscriber = self._msgbus.is_subscribed(
                topic=topic,
                handler=self._update_order_book,
            )

            # Remove the subscription for the internal order book if it is the last subscription
            if num_subscribers == 1 and is_internal_book_subscriber:
                self._msgbus.unsubscribe(
                    topic=topic,
                    handler=self._update_order_book,
                )

        if not self._msgbus.has_subscribers(deltas_topic):
            if instrument_id in client.subscribed_order_book_deltas():
                client.unsubscribe_order_book_deltas(instrument_id, metadata)

        if not self._msgbus.has_subscribers(snapshots_topic):
            if instrument_id in client.subscribed_order_book_snapshots():
                client.unsubscribe_order_book_snapshots(instrument_id, metadata)

    cpdef void _handle_unsubscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if not self._msgbus.has_subscribers(
            f"data.quotes"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            if instrument_id in client.subscribed_quote_ticks():
                client.unsubscribe_quote_ticks(instrument_id, metadata)

    cpdef void _handle_unsubscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        if not self._msgbus.has_subscribers(
            f"data.trades"
            f".{instrument_id.venue}"
            f".{instrument_id.symbol}",
        ):
            if instrument_id in client.subscribed_trade_ticks():
                client.unsubscribe_trade_ticks(instrument_id, metadata)

    cpdef void _handle_unsubscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
        dict metadata,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")

        if self._msgbus.has_subscribers(f"data.bars.{bar_type.standard()}"):
            return

        if bar_type.is_internally_aggregated():
            # Internal aggregation
            if bar_type.standard() in self._bar_aggregators:
                self._stop_bar_aggregator(client, bar_type, metadata)
        else:
            # External aggregation
            if bar_type in client.subscribed_bars():
                client.unsubscribe_bars(bar_type, metadata)

    cpdef void _handle_unsubscribe_data(
        self,
        DataClient client,
        DataType data_type,
    ):
        Condition.not_none(client, "client")
        Condition.not_none(data_type, "data_type")

        try:
            if not self._msgbus.has_subscribers(f"data.{data_type}"):
                if data_type in client.subscribed_custom_data():
                    client.unsubscribe(data_type)
        except NotImplementedError:
            self._log.error(
                f"Cannot unsubscribe: {client.id.value} "
                f"has not implemented data type {data_type} subscriptions",
            )
            return

# -- REQUEST HANDLERS -----------------------------------------------------------------------------

    cpdef tuple[datetime, object] _catalogs_last_timestamp(
        self,
        type data_cls,
        InstrumentId instrument_id = None,
        BarType bar_type = None,
        str ts_column = "ts_init",
    ):
        cdef datetime last_timestamp = None
        cdef datetime prev_last_timestamp = None

        last_timestamp_catalog = None

        for catalog in self._catalogs.values():
            prev_last_timestamp = last_timestamp
            last_timestamp = max_date(
                last_timestamp,
                catalog.query_last_timestamp(data_cls, instrument_id, bar_type, ts_column)
            )

            if last_timestamp is not None and (prev_last_timestamp is None or last_timestamp > prev_last_timestamp):
                last_timestamp_catalog = catalog

        return last_timestamp, last_timestamp_catalog

    cpdef void _handle_request(self, DataRequest request):
        if self.debug:
            self._log.debug(f"{RECV}{REQ} {request}", LogColor.MAGENTA)

        self.request_count += 1

        # Query data client
        cdef DataClient client = self._clients.get(request.client_id)

        if client is None:
            client = self._routing_map.get(
                request.venue,
                self._default_client,
            )

        if client is not None:
            Condition.is_true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")

        cdef dict[str, object] metadata = request.data_type.metadata
        cdef str bars_market_data_type = metadata.get("bars_market_data_type", "")

        cdef datetime now = self._clock.utc_now()
        cdef datetime start = time_object_to_dt(metadata.get("start"))  # Can be None
        cdef datetime end = time_object_to_dt(metadata.get("end"))  # Can be None

        if request.data_type.type == Instrument:
            instrument_id = request.data_type.metadata.get("instrument_id")
            if instrument_id is None:
                self._handle_request_instruments(request, client, start, end, metadata)
            else:
                self._handle_request_instrument(request, client, instrument_id, start, end, metadata)
        elif request.data_type.type == OrderBookDeltas:
            self._handle_request_order_book_deltas(request, client, metadata)
        elif request.data_type.type == QuoteTick or bars_market_data_type == "quote_ticks":
            self._handle_request_quote_ticks(request, client, start, end, now, metadata)
        elif request.data_type.type == TradeTick or bars_market_data_type == "trade_ticks":
            self._handle_request_trade_ticks(request, client, start, end, now, metadata)
        elif request.data_type.type == Bar or bars_market_data_type == "bars":
            self._handle_request_bars(request, client, start, end, now, metadata)
        else:
            self._handle_request_data(request, client, start, end, now)

    cpdef void _handle_request_instruments(self, DataRequest request, DataClient client, datetime start, datetime end, dict metadata):
        cdef bint update_catalog = metadata.get("update_catalog", False)

        if self._catalogs and not update_catalog:
            self._query_catalog(request)
            return

        if client is None:
            self._log.error(
                f"Cannot handle request: "
                f"no client registered for '{request.client_id}', {request}")
            return  # No client to handle request

        client.request_instruments(
            request.data_type.metadata.get("venue"),
            request.id,
            start,
            end,
            metadata,
        )

    cpdef void _handle_request_instrument(self, DataRequest request, DataClient client, InstrumentId instrument_id, datetime start, datetime end, dict metadata):
        last_timestamp, _ = self._catalogs_last_timestamp(
            Instrument,
            instrument_id,
        )

        if last_timestamp:
            self._query_catalog(request)
            return

        if client is None:
            self._log.error(
                f"Cannot handle request: "
                f"no client registered for '{request.client_id}', {request}")
            return  # No client to handle request

        client.request_instrument(
            instrument_id,
            request.id,
            start,
            end,
            metadata,
        )

    cpdef void _handle_request_order_book_deltas(self, DataRequest request, DataClient client, dict metadata):
        instrument_id = request.data_type.metadata.get("instrument_id")

        if client is None:
            self._log.error(
                f"Cannot handle request: "
                f"no client registered for '{request.client_id}', {request}")
            return  # No client to handle request

        client.request_order_book_snapshot(
            instrument_id,
            request.data_type.metadata.get("limit", 0),
            request.id,
            metadata,
        )

    cpdef void _handle_request_quote_ticks(self, DataRequest request, DataClient client, datetime start, datetime end, datetime now, dict metadata):
        instrument_id = request.data_type.metadata.get("instrument_id")

        last_timestamp, _ = self._catalogs_last_timestamp(
            QuoteTick,
            instrument_id,
        )

        if last_timestamp:
            if (now <= last_timestamp) or (end and end <= last_timestamp):
                self._query_catalog(request)
                return

        if client is None:
            self._log.error(
                f"Cannot handle request: "
                f"no client registered for '{request.client_id}', {request}")
            return  # No client to handle request

        if last_timestamp and start and start <= last_timestamp:
            self._new_query_group(request.id, 2)
            self._query_catalog(request)

        client_start = max_date(start, last_timestamp)
        client.request_quote_ticks(
            instrument_id,
            request.data_type.metadata.get("limit", 0),
            request.id,
            client_start,
            end,
            metadata,
        )

    cpdef void _handle_request_trade_ticks(self, DataRequest request, DataClient client, datetime start, datetime end, datetime now, dict metadata):
        instrument_id = request.data_type.metadata.get("instrument_id")

        last_timestamp, _ = self._catalogs_last_timestamp(
            TradeTick,
            instrument_id,
        )

        if last_timestamp:
            if (now <= last_timestamp) or (end and end <= last_timestamp):
                self._query_catalog(request)
                return

        if client is None:
            self._log.error(
                f"Cannot handle request: "
                f"no client registered for '{request.client_id}', {request}")
            return  # No client to handle request

        if last_timestamp and start and start <= last_timestamp:
            self._new_query_group(request.id, 2)
            self._query_catalog(request)

        client_start = max_date(start, last_timestamp)
        client.request_trade_ticks(
            instrument_id,
            request.data_type.metadata.get("limit", 0),
            request.id,
            client_start,
            end,
            metadata,
        )

    cpdef void _handle_request_bars(self, DataRequest request, DataClient client, datetime start, datetime end, datetime now, dict metadata):
        bar_type = request.data_type.metadata.get("bar_type")

        last_timestamp, _ = self._catalogs_last_timestamp(
            Bar,
            bar_type=bar_type,
        )

        if last_timestamp:
            if (now <= last_timestamp) or (end and end <= last_timestamp):
                self._query_catalog(request)
                return

        if client is None:
            self._log.error(
                f"Cannot handle request: "
                f"no client registered for '{request.client_id}', {request}")
            return  # No client to handle request

        if last_timestamp and start and start <= last_timestamp:
            self._new_query_group(request.id, 2)
            self._query_catalog(request)

        client_start = max_date(start, last_timestamp)
        client.request_bars(
            bar_type,
            request.data_type.metadata.get("limit", 0),
            request.id,
            client_start,
            end,
            metadata,
        )

    cpdef void _handle_request_data(
        self,
        DataRequest request,
        DataClient client,
        datetime start,
        datetime end,
        datetime now,
    ):
        last_timestamp, _ = self._catalogs_last_timestamp(
            request.data_type.type,
        )

        if last_timestamp:
            if (now <= last_timestamp) or (end and end <= last_timestamp):
                self._query_catalog(request)
                return

        if client is None:
            self._log.error(
                f"Cannot handle request: "
                f"no client registered for '{request.client_id}', {request}")
            return  # No client to handle request

        if last_timestamp and start and start <= last_timestamp:
            self._new_query_group(request.id, 2)
            self._query_catalog(request)

        try:
            client.request(request.data_type, request.id)
        except NotImplementedError:
            self._log.error(f"Cannot handle request: unrecognized data type {request.data_type}")

    cpdef void _query_catalog(self, DataRequest request):
        cdef datetime start = request.data_type.metadata.get("start")
        cdef datetime end = request.data_type.metadata.get("end")

        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef uint64_t ts_start = dt_to_unix_nanos(start) if start is not None else 0
        cdef uint64_t ts_end = dt_to_unix_nanos(end) if end is not None else ts_now

        # Validate request time range
        Condition.is_true(ts_start <= ts_end, f"{ts_start=} was greater than {ts_end=}")

        if end is not None and ts_end > ts_now:
            self._log.warning(
                "Cannot request data beyond current time. "
                f"Truncating `end` to current UNIX nanoseconds {unix_nanos_to_dt(ts_now)}",
            )
            ts_end = ts_now

        bars_market_data_type = request.data_type.metadata.get("bars_market_data_type", "")
        data = []

        if request.data_type.type == Instrument:
            instrument_id = request.data_type.metadata.get("instrument_id")
            if instrument_id is None:
                for catalog in self._catalogs.values():
                    data += catalog.instruments()
            else:
                for catalog in self._catalogs.values():
                    data += catalog.instruments(instrument_ids=[str(instrument_id)])
        elif request.data_type.type == QuoteTick or bars_market_data_type == "quote_ticks":
            for catalog in self._catalogs.values():
                data += catalog.quote_ticks(
                    instrument_ids=[str(request.data_type.metadata.get("instrument_id"))],
                    start=ts_start,
                    end=ts_end,
                )
        elif request.data_type.type == TradeTick or bars_market_data_type == "trade_ticks":
            for catalog in self._catalogs.values():
                data += catalog.trade_ticks(
                    instrument_ids=[str(request.data_type.metadata.get("instrument_id"))],
                    start=ts_start,
                    end=ts_end,
                )
        elif request.data_type.type == Bar or bars_market_data_type == "bars":
            bar_type = request.data_type.metadata.get("bar_type")
            if bar_type is None:
                self._log.error("No bar type provided for bars request")
                return

            for catalog in self._catalogs.values():
                data += catalog.bars(
                    instrument_ids=[str(bar_type.instrument_id)],
                    bar_type=str(bar_type),
                    start=ts_start,
                    end=ts_end,
                )
        elif request.data_type.type == InstrumentClose:
            for catalog in self._catalogs.values():
                data += catalog.instrument_closes(
                    instrument_ids=[str(request.data_type.metadata.get("instrument_id"))],
                    start=ts_start,
                    end=ts_end,
                )
        else:
            for catalog in self._catalogs.values():
                data += catalog.custom_data(
                    cls=request.data_type.type,
                    metadata=request.data_type.metadata,
                    start=ts_start,
                    end=ts_end,
                )

        # Validate data is not from the future
        if data and data[-1].ts_init > ts_now:
            raise RuntimeError(
                "Invalid response: Historical data from the future: "
                f"data[-1].ts_init={data[-1].ts_init}, {ts_now=}",
            )

        metadata = request.data_type.metadata.copy()
        metadata["update_catalog"] = False
        data_type = DataType(request.data_type.type, metadata)

        response = DataResponse(
            client_id=request.client_id,
            venue=request.venue,
            data_type=data_type,
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
        elif isinstance(data, InstrumentStatus):
            self._handle_instrument_status(data)
        elif isinstance(data, InstrumentClose):
            self._handle_close_price(data)
        elif isinstance(data, CustomData):
            self._handle_custom_data(data)
        else:
            self._log.error(f"Cannot handle data: unrecognized type {type(data)} {data}")

    cpdef void _handle_instrument(self, Instrument instrument, bint update_catalog = False):
        self._cache.add_instrument(instrument)

        if update_catalog:
            self._update_catalog([instrument], is_instrument=True)

        self._msgbus.publish_c(
            topic=f"data.instrument"
                  f".{instrument.id.venue}"
                  f".{instrument.id.symbol}",
            msg=instrument,
        )

    cpdef void _handle_order_book_delta(self, OrderBookDelta delta):
        cdef OrderBookDeltas deltas = None
        cdef list[OrderBookDelta] buffer_deltas = None
        cdef bint is_last_delta = False

        if self._buffer_deltas:
            buffer_deltas = self._buffered_deltas_map.get(delta.instrument_id)

            if buffer_deltas is None:
                buffer_deltas = []
                self._buffered_deltas_map[delta.instrument_id] = buffer_deltas

            buffer_deltas.append(delta)
            is_last_delta = delta.flags == RecordFlag.F_LAST

            if is_last_delta:
                deltas = OrderBookDeltas(
                    instrument_id=delta.instrument_id,
                    deltas=buffer_deltas
                )
                self._msgbus.publish_c(
                    topic=f"data.book.deltas"
                        f".{deltas.instrument_id.venue}"
                        f".{deltas.instrument_id.symbol}",
                    msg=deltas,
                )
                buffer_deltas.clear()
        else:
            deltas = OrderBookDeltas(
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
        cdef OrderBookDeltas deltas_to_publish = None
        cdef list[OrderBookDelta] buffer_deltas = None
        cdef bint is_last_delta = False

        if self._buffer_deltas:
            buffer_deltas = self._buffered_deltas_map.get(deltas.instrument_id)

            if buffer_deltas is None:
                buffer_deltas = []
                self._buffered_deltas_map[deltas.instrument_id] = buffer_deltas

            for delta in deltas.deltas:
                buffer_deltas.append(delta)
                is_last_delta = delta.flags == RecordFlag.F_LAST

                if is_last_delta:
                    deltas_to_publish = OrderBookDeltas(
                        instrument_id=deltas.instrument_id,
                        deltas=buffer_deltas,
                    )
                    self._msgbus.publish_c(
                        topic=f"data.book.deltas"
                            f".{deltas.instrument_id.venue}"
                            f".{deltas.instrument_id.symbol}",
                        msg=deltas_to_publish,
                    )
                    buffer_deltas.clear()
        else:
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
                        f"Bar {bar} was prior to last bar `ts_event` {last_bar.ts_event}",
                    )
                    return  # `bar` is out of sequence
                if bar.ts_init < last_bar.ts_init:
                    self._log.warning(
                        f"Bar {bar} was prior to last bar `ts_init` {last_bar.ts_init}",
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
                            f"Bar revision {bar} was not at last bar `ts_event` {last_bar.ts_event}",
                        )
                        return  # Revision SHOULD be at `last_bar.ts_event`

        if not bar.is_revision:
            self._cache.add_bar(bar)

        self._msgbus.publish_c(topic=f"data.bars.{bar_type}", msg=bar)

    cpdef void _handle_instrument_status(self, InstrumentStatus data):
        self._msgbus.publish_c(topic=f"data.status.{data.instrument_id.venue}.{data.instrument_id.symbol}", msg=data)

    cpdef void _handle_close_price(self, InstrumentClose data):
        self._msgbus.publish_c(topic=f"data.venue.close_price.{data.instrument_id}", msg=data)

    cpdef void _handle_custom_data(self, CustomData data):
        self._msgbus.publish_c(topic=f"data.{data.data_type.topic}", msg=data.data)

# -- RESPONSE HANDLERS ----------------------------------------------------------------------------

    cpdef void _handle_response(self, DataResponse response):
        if self.debug:
            self._log.debug(f"{RECV}{RES} {response}", LogColor.MAGENTA)

        self.response_count += 1

        correlation_id = response.correlation_id
        update_catalog = False

        if response.data_type.metadata is not None:
            update_catalog = response.data_type.metadata.get("update_catalog", False)

        if type(response.data) is list:
            response_data = response.data
        else:
            #for request_instrument case
            response_data = [response.data]

        if update_catalog and response.data_type.type != Instrument:
            # for instruments we want to handle each instrument individually
            self._update_catalog(response_data)

        response_data = self._handle_query_group(correlation_id, response_data)

        if response_data is None:
            return

        if response.data_type.type != Instrument:
            response.data = response_data

        if response.data_type.type == Instrument:
            if isinstance(response.data, list):
                self._handle_instruments(response.data, update_catalog)
            else:
                self._handle_instrument(response.data, update_catalog)
        elif response.data_type.type == QuoteTick:
            self._handle_quote_ticks(response.data)
        elif response.data_type.type == TradeTick:
            self._handle_trade_ticks(response.data)
        elif response.data_type.type == Bar:
            if response.data_type.metadata.get("bars_market_data_type"):
                response.data = self._handle_aggregated_bars(response.data, response.data_type.metadata)
            else:
                self._handle_bars(response.data, response.data_type.metadata.get("Partial"))

        self._msgbus.response(response)

    cpdef void _update_catalog(self, list ticks, bint is_instrument = False):
        if len(ticks) == 0:
            return

        if type(ticks[0]) is Bar:
            last_timestamp, last_timestamp_catalog = self._catalogs_last_timestamp(Bar, bar_type=ticks[0].bar_type)
        else:
            last_timestamp, last_timestamp_catalog = self._catalogs_last_timestamp(type(ticks[0]), ticks[0].instrument_id)

        # We don't want to write in the catalog several times the same instrument
        if last_timestamp_catalog and is_instrument:
            return

        if last_timestamp_catalog is None and len(self._catalogs) > 0:
            last_timestamp_catalog = self._catalogs[0]

        if last_timestamp_catalog is not None:
            last_timestamp_catalog.write_data(ticks, mode="append")
        else:
            self._log.warning("No catalog available for appending data.")

    cpdef void _new_query_group(self, UUID4 correlation_id, int n_components):
        self._query_group_n_components[correlation_id] = n_components

    cpdef object _handle_query_group(self, UUID4 correlation_id, list ticks):
        # closure is not allowed in cpdef functions so we call a cdef function
        return self._handle_query_group_aux(correlation_id, ticks)

    cdef object _handle_query_group_aux(self, UUID4 correlation_id, list ticks):
        # return None or a list of ticks
        if correlation_id not in self._query_group_n_components:
            return ticks

        if self._query_group_n_components[correlation_id] == 1:
            del self._query_group_n_components[correlation_id]
            return ticks

        if correlation_id not in self._query_group_components:
            self._query_group_components[correlation_id] = []

        self._query_group_components[correlation_id].append(ticks)

        if len(self._query_group_components[correlation_id]) != self._query_group_n_components[correlation_id]:
            return None

        components = []

        for component in self._query_group_components[correlation_id]:
            if len(component) > 0:
                components.append(component)

        components = sorted(components, key=lambda l: l[0].ts_init)
        result = components[0]
        last_timestamp = result[-1].ts_init

        if len(components) > 1:
            for component in components[1:]:
                first_index = 0

                for i in range(len(component)):
                    if component[i].ts_init > last_timestamp:
                        first_index = i
                        last_timestamp = component[-1].ts_init
                        break

                result += component[first_index:]

        del self._query_group_n_components[correlation_id]
        del self._query_group_components[correlation_id]

        return result

    cpdef void _handle_instruments(self, list instruments, bint update_catalog = False):
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument, update_catalog)

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
                self._log.debug(f"Applying partial bar {partial} for {partial.bar_type}")
                aggregator.set_partial(partial)
            else:
                if self._fsm.state == ComponentState.RUNNING:
                    # Only log this error if the component is running, because
                    # there may have been an immediate stop called after start
                    # - with the partial bar being for a now removed aggregator.
                    self._log.error("No aggregator for partial bar update")

    cpdef dict _handle_aggregated_bars(self, list ticks, dict metadata):
        # closure is not allowed in cpdef functions so we call a cdef function
        return self._handle_aggregated_bars_aux(ticks, metadata)

    cdef dict _handle_aggregated_bars_aux(self, list ticks, dict metadata):
        result = {}

        if len(ticks) == 0:
            self._log.warning("_handle_aggregated_bars: No data to aggregate")
            return result

        bars_result = {}

        if metadata["include_external_data"]:
            if metadata["bars_market_data_type"] == "quote_ticks":
                self._cache.add_quote_ticks(ticks)
                result["quote_ticks"] = ticks
            elif metadata["bars_market_data_type"] == "trade_ticks":
                self._cache.add_trade_ticks(ticks)
                result["trade_ticks"] = ticks
            elif metadata["bars_market_data_type"] == "bars":
                self._cache.add_bars(ticks)

        if metadata["bars_market_data_type"] == "bars":
            bars_result[metadata["bar_type"]] = ticks

        for bar_type in metadata["bar_types"]:
            aggregated_bars = []
            handler = lambda bar: aggregated_bars.append(bar)
            aggregator = None

            if metadata["update_existing_subscriptions"] and bar_type.standard() in self._bar_aggregators:
                aggregator = self._bar_aggregators.get(bar_type.standard())
            else:
                instrument = self._cache.instrument(metadata["instrument_id"])
                if instrument is None:
                    self._log.error(
                        f"Cannot start bar aggregation: "
                        f"no instrument found for {bar_type.instrument_id}",
                    )

                # Create aggregator
                if bar_type.spec.is_time_aggregated():
                    test_clock = TestClock()
                    aggregator = TimeBarAggregator(
                        instrument=instrument,
                        bar_type=bar_type,
                        handler=handler,
                        clock=test_clock,
                        build_with_no_updates=self._time_bars_build_with_no_updates,
                        timestamp_on_close=self._time_bars_timestamp_on_close,
                        interval_type=self._time_bars_interval_type,
                    )
                elif bar_type.spec.aggregation == BarAggregation.TICK:
                    aggregator = TickBarAggregator(
                        instrument=instrument,
                        bar_type=bar_type,
                        handler=handler,
                    )
                elif bar_type.spec.aggregation == BarAggregation.VOLUME:
                    aggregator = VolumeBarAggregator(
                        instrument=instrument,
                        bar_type=bar_type,
                        handler=handler,
                    )
                elif bar_type.spec.aggregation == BarAggregation.VALUE:
                    aggregator = ValueBarAggregator(
                        instrument=instrument,
                        bar_type=bar_type,
                        handler=handler,
                    )

            if metadata["bars_market_data_type"] == "quote_ticks" and not bar_type.is_composite():
                aggregator.start_batch_update(handler, ticks[0].ts_event)

                for tick in ticks:
                    aggregator.handle_quote_tick(tick)
            elif metadata["bars_market_data_type"] == "trade_ticks" and not bar_type.is_composite():
                aggregator.start_batch_update(handler, ticks[0].ts_event)

                for tick in ticks:
                    aggregator.handle_trade_tick(tick)
            else:
                input_bars = bars_result[bar_type.composite()]

                if len(input_bars) > 0:
                    aggregator.start_batch_update(handler, input_bars[0].ts_init)

                    for bar in input_bars:
                        aggregator.handle_bar(bar)

            aggregator.stop_batch_update()
            bars_result[bar_type.standard()] = aggregated_bars

        if not metadata["include_external_data"] and metadata["bars_market_data_type"] == "bars":
            del bars_result[metadata["bar_type"]]

        # we need a second final dict as a we can't delete keys in a loop
        result["bars"] = {}

        for bar_type in bars_result:
            if len(bars_result[bar_type]) > 0:
                result["bars"][bar_type] = bars_result[bar_type]

        return result

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
            return

        order_book.apply(data)

    cpdef void _snapshot_order_book(self, TimeEvent snap_event):
        if self.debug:
            self._log.debug(f"Received snapshot event for {snap_event}", LogColor.MAGENTA)

        cdef SnapshotInfo snap_info = self._snapshot_info.get(snap_event.name)
        if snap_info is None:
            self._log.error(f"No `SnapshotInfo` found for snapshot event {snap_event}")
            return

        cdef:
            list[Instrument] instruments
            Instrument instrument
        if snap_info.is_composite:
            instruments = self._cache.instruments(venue=snap_info.venue, underlying=snap_info.root)
            for instrument in instruments:
                self._publish_order_book(instrument.id, snap_info.topic)
        else:
            self._publish_order_book(snap_info.instrument_id, snap_info.topic)

    cpdef void _publish_order_book(self, InstrumentId instrument_id, str topic):
        cdef OrderBook order_book = self._cache.order_book(instrument_id)
        if order_book is None:
            self._log.error(
                f"Cannot snapshot orderbook: "
                f"no order book found, {instrument_id}",
            )
            return

        if order_book.ts_last == 0:
            self._log.debug("OrderBook not yet updated, skipping snapshot")
            return

        self._msgbus.publish_c(
            topic=topic,
            msg=order_book,
        )

    cpdef void _start_bar_aggregator(
        self,
        MarketDataClient client,
        BarType bar_type,
        bint await_partial,
        dict metadata,
    ):
        cdef Instrument instrument = self._cache.instrument(bar_type.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot start bar aggregation: "
                f"no instrument found for {bar_type.instrument_id}",
            )

        # Create aggregator
        if bar_type.spec.is_time_aggregated():
            aggregator = TimeBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
                clock=self._clock,
                build_with_no_updates=self._time_bars_build_with_no_updates,
                timestamp_on_close=self._time_bars_timestamp_on_close,
                interval_type=self._time_bars_interval_type,
                time_bars_origin=self._time_bars_origins.get(bar_type.spec.aggregation),
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
        self._bar_aggregators[bar_type.standard()] = aggregator
        self._log.debug(f"Added {aggregator} for {bar_type} bars")

        # Subscribe to required data
        if bar_type.is_composite():
            composite_bar_type = bar_type.composite()

            self._msgbus.subscribe(
                topic=f"data.bars.{composite_bar_type}",
                handler=aggregator.handle_bar,
            )
            self._handle_subscribe_bars(client, composite_bar_type, False, metadata)
        elif bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.subscribe(
                topic=f"data.trades"
                      f".{bar_type.instrument_id.venue}"
                      f".{bar_type.instrument_id.symbol}",
                handler=aggregator.handle_trade_tick,
                priority=5,
            )
            self._handle_subscribe_trade_ticks(client, bar_type.instrument_id, metadata)
        else:
            self._msgbus.subscribe(
                topic=f"data.quotes"
                      f".{bar_type.instrument_id.venue}"
                      f".{bar_type.instrument_id.symbol}",
                handler=aggregator.handle_quote_tick,
                priority=5,
            )
            self._handle_subscribe_quote_ticks(client, bar_type.instrument_id, metadata)

    cpdef void _stop_bar_aggregator(self, MarketDataClient client, BarType bar_type, dict metadata):
        cdef aggregator = self._bar_aggregators.get(bar_type.standard())
        if aggregator is None:
            self._log.warning(
                f"Cannot stop bar aggregator: "
                f"no aggregator to stop for {bar_type}",
            )
            return

        if isinstance(aggregator, TimeBarAggregator):
            aggregator.stop()

        # Unsubscribe from market data updates
        if bar_type.is_composite():
            composite_bar_type = bar_type.composite()

            self._msgbus.unsubscribe(
                topic=f"data.bars.{composite_bar_type}",
                handler=aggregator.handle_bar,
            )
            self._handle_unsubscribe_bars(client, composite_bar_type, metadata)
        elif bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.unsubscribe(
                topic=f"data.trades"
                      f".{bar_type.instrument_id.venue}"
                      f".{bar_type.instrument_id.symbol}",
                handler=aggregator.handle_trade_tick,
            )
            self._handle_unsubscribe_trade_ticks(client, bar_type.instrument_id, metadata)
        else:
            self._msgbus.unsubscribe(
                topic=f"data.quotes"
                      f".{bar_type.instrument_id.venue}"
                      f".{bar_type.instrument_id.symbol}",
                handler=aggregator.handle_quote_tick,
            )
            self._handle_unsubscribe_quote_ticks(client, bar_type.instrument_id, metadata)

        # Remove from aggregators
        del self._bar_aggregators[bar_type.standard()]

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
                    f"no quotes for {instrument_id} yet",
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
                    f"no trades for {instrument_id} yet",
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
