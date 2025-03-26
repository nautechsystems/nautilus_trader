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
from nautilus_trader.common.enums import UpdateCatalogMode
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import max_date
from nautilus_trader.core.datetime import min_date
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.catalog.types import CatalogWriteMode

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport RECV
from nautilus_trader.common.component cimport REQ
from nautilus_trader.common.component cimport RES
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport MessageBus
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
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport RequestBars
from nautilus_trader.data.messages cimport RequestData
from nautilus_trader.data.messages cimport RequestInstrument
from nautilus_trader.data.messages cimport RequestInstruments
from nautilus_trader.data.messages cimport RequestOrderBookSnapshot
from nautilus_trader.data.messages cimport RequestQuoteTicks
from nautilus_trader.data.messages cimport RequestTradeTicks
from nautilus_trader.data.messages cimport SubscribeBars
from nautilus_trader.data.messages cimport SubscribeData
from nautilus_trader.data.messages cimport SubscribeIndexPrices
from nautilus_trader.data.messages cimport SubscribeInstrument
from nautilus_trader.data.messages cimport SubscribeInstrumentClose
from nautilus_trader.data.messages cimport SubscribeInstruments
from nautilus_trader.data.messages cimport SubscribeInstrumentStatus
from nautilus_trader.data.messages cimport SubscribeMarkPrices
from nautilus_trader.data.messages cimport SubscribeOrderBook
from nautilus_trader.data.messages cimport SubscribeQuoteTicks
from nautilus_trader.data.messages cimport SubscribeTradeTicks
from nautilus_trader.data.messages cimport UnsubscribeBars
from nautilus_trader.data.messages cimport UnsubscribeData
from nautilus_trader.data.messages cimport UnsubscribeIndexPrices
from nautilus_trader.data.messages cimport UnsubscribeInstrument
from nautilus_trader.data.messages cimport UnsubscribeInstrumentClose
from nautilus_trader.data.messages cimport UnsubscribeInstruments
from nautilus_trader.data.messages cimport UnsubscribeInstrumentStatus
from nautilus_trader.data.messages cimport UnsubscribeMarkPrices
from nautilus_trader.data.messages cimport UnsubscribeOrderBook
from nautilus_trader.data.messages cimport UnsubscribeQuoteTicks
from nautilus_trader.data.messages cimport UnsubscribeTradeTicks
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport CustomData
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport MarkPriceUpdate
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

        # Configuration
        self.debug = config.debug
        self._time_bars_interval_type = config.time_bars_interval_type
        self._time_bars_timestamp_on_close = config.time_bars_timestamp_on_close
        self._time_bars_skip_first_non_full_bar = config.time_bars_skip_first_non_full_bar
        self._time_bars_build_with_no_updates = config.time_bars_build_with_no_updates
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

    cpdef list subscribed_mark_prices(self):
        """
        Return the mark price update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []

        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_mark_prices()

        return subscriptions

    cpdef list subscribed_index_prices(self):
        """
        Return the index price update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list subscriptions = []

        cdef MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_index_prices()

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
            if client.is_running:
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

    cpdef void request(self, RequestData request):
        """
        Handle the given request.

        Parameters
        ----------
        request : RequestData
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

        if isinstance(command, SubscribeData):
            self._handle_subscribe(client, command)
        elif isinstance(command, UnsubscribeData):
            self._handle_unsubscribe(client, command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}")

    cpdef void _handle_subscribe(self, DataClient client, SubscribeData command):
        if isinstance(command, SubscribeInstruments):
            self._handle_subscribe_instruments(client, command)
        elif isinstance(command, SubscribeInstrument):
            self._handle_subscribe_instrument(client, command)
        elif isinstance(command, SubscribeOrderBook):
            if command.data_type.type == OrderBookDelta:
                self._handle_subscribe_order_book_deltas(client, command)
            else:
                self._handle_subscribe_order_book_snapshots(client, command)
        elif isinstance(command, SubscribeQuoteTicks):
            self._handle_subscribe_quote_ticks(client, command)
        elif isinstance(command, SubscribeTradeTicks):
            self._handle_subscribe_trade_ticks(client, command)
        elif isinstance(command, SubscribeMarkPrices):
            self._handle_subscribe_mark_prices(client, command)
        elif isinstance(command, SubscribeIndexPrices):
            self._handle_subscribe_index_prices(client, command)
        elif isinstance(command, SubscribeBars):
            self._handle_subscribe_bars(client, command)
        elif isinstance(command, SubscribeInstrumentStatus):
            self._handle_subscribe_instrument_status(client, command)
        elif isinstance(command, SubscribeInstrumentClose):
            self._handle_subscribe_instrument_close(client, command)
        else:
            self._handle_subscribe_data(client, command)

    cpdef void _handle_unsubscribe(self, DataClient client, UnsubscribeData command):
        if isinstance(command, UnsubscribeInstruments):
            self._handle_unsubscribe_instruments(client, command)
        elif isinstance(command, UnsubscribeInstrument):
            self._handle_unsubscribe_instrument(client, command)
        elif isinstance(command, UnsubscribeOrderBook):
            if command.data_type.type == OrderBookDelta:
                self._handle_unsubscribe_order_book_deltas(client, command)
            else:
                self._handle_unsubscribe_order_book_snapshots(client, command)
        elif isinstance(command, UnsubscribeQuoteTicks):
            self._handle_unsubscribe_quote_ticks(client, command)
        elif isinstance(command, UnsubscribeTradeTicks):
            self._handle_unsubscribe_trade_ticks(client, command)
        elif isinstance(command, UnsubscribeMarkPrices):
            self._handle_unsubscribe_mark_prices(client, command)
        elif isinstance(command, UnsubscribeIndexPrices):
            self._handle_unsubscribe_index_prices(client, command)
        elif isinstance(command, UnsubscribeBars):
            self._handle_unsubscribe_bars(client, command)
        elif isinstance(command, UnsubscribeInstrumentStatus):
            self._handle_unsubscribe_instrument_status(client, command)
        elif isinstance(command, UnsubscribeInstrumentClose):
            self._handle_unsubscribe_instrument_status(client, command)
        else:
            self._handle_unsubscribe_data(client, command)

    cpdef void _handle_subscribe_instruments(self, MarketDataClient client, SubscribeInstruments command):
        Condition.not_none(client, "client")

        client.subscribe_instruments(command)

    cpdef void _handle_subscribe_instrument(self, MarketDataClient client, SubscribeInstrument command):
        Condition.not_none(client, "client")

        if command.instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `Instrument` data")
            return

        if command.instrument_id not in client.subscribed_instruments():
            client.subscribe_instrument(command)

    cpdef void _handle_subscribe_order_book_deltas(self, MarketDataClient client, SubscribeOrderBook command):
        Condition.not_none(client, "client")
        Condition.not_none(command.instrument_id, "instrument_id")
        Condition.not_none(command.params, "params")

        if command.instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `OrderBookDelta` data")
            return

        self._setup_order_book(client, command)

    cpdef void _handle_subscribe_order_book_snapshots(self, MarketDataClient client, SubscribeOrderBook command):
        Condition.not_none(client, "client")
        Condition.not_none(command.instrument_id, "instrument_id")
        Condition.positive_int(command.interval_ms, "interval_ms")
        Condition.not_none(command.params, "params")

        if command.instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `OrderBook` data")
            return

        cdef:
            uint64_t interval_ns
            uint64_t timestamp_ns
            SnapshotInfo snap_info
        key = (command.instrument_id, command.interval_ms)
        if key not in self._order_book_intervals:
            self._order_book_intervals[key] = []

            timer_name = f"OrderBook|{command.instrument_id}|{command.interval_ms}"
            interval_ns = millis_to_nanos(command.interval_ms)
            timestamp_ns = self._clock.timestamp_ns()
            start_time_ns = timestamp_ns - (timestamp_ns % interval_ns)

            topic = f"data.book.snapshots.{command.instrument_id.venue}.{command.instrument_id.symbol}.{command.interval_ms}"

            # Cache snapshot event info
            snap_info = SnapshotInfo.__new__(SnapshotInfo)
            snap_info.instrument_id = command.instrument_id
            snap_info.venue = command.instrument_id.venue
            snap_info.is_composite = command.instrument_id.symbol.is_composite()
            snap_info.root = command.instrument_id.symbol.root()
            snap_info.topic = topic
            snap_info.interval_ms = command.interval_ms

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

        self._setup_order_book(client, command)

    cpdef void _setup_order_book(self, MarketDataClient client, SubscribeOrderBook command):
        Condition.not_none(client, "client")
        Condition.not_none(command.instrument_id, "instrument_id")
        Condition.not_none(command.params, "params")

        cdef bint only_deltas = command.data_type.type == OrderBookDelta
        cdef Instrument instrument = self._cache.instrument(command.instrument_id)

        if instrument is None:
            self._log.warning(
                f"No instrument found for {command.instrument_id} on order book data subscription"
            )

        cdef:
            list[Instrument] instruments
            str root
        if command.managed:
            # Create order book(s)
            if command.instrument_id.symbol.is_composite():
                root = command.instrument_id.symbol.root()
                instruments = self._cache.instruments(venue=command.instrument_id.venue, underlying=root)

                for instrument in instruments:
                    self._create_new_book(instrument.id, command.book_type)
            else:
                self._create_new_book(command.instrument_id, command.book_type)

        # Always re-subscribe to override previous settings
        try:
            if command.instrument_id not in client.subscribed_order_book_deltas():
                client.subscribe_order_book_deltas(command)
        except NotImplementedError:
            if only_deltas:
                raise

            if command.instrument_id not in client.subscribed_order_book_snapshots():
                client.subscribe_order_book_snapshots(command)

        # Set up subscriptions
        cdef str topic = f"data.book.deltas.{command.instrument_id.venue}.{command.instrument_id.symbol.topic()}"

        if not self._msgbus.is_subscribed(
            topic=topic,
            handler=self._update_order_book,
        ):
            self._msgbus.subscribe(
                topic=topic,
                handler=self._update_order_book,
                priority=10,
            )

        topic = f"data.book.depth.{command.instrument_id.venue}.{command.instrument_id.symbol.topic()}"

        if not only_deltas and not self._msgbus.is_subscribed(
            topic=topic,
            handler=self._update_order_book,
        ):
            self._msgbus.subscribe(
                topic=topic,
                handler=self._update_order_book,
                priority=10,
            )

    cpdef void _create_new_book(self, InstrumentId instrument_id, BookType book_type):
        order_book = OrderBook(
            instrument_id=instrument_id,
            book_type=book_type,
        )

        self._cache.add_order_book(order_book)
        self._log.debug(f"Created {type(order_book).__name__} for {instrument_id}")

    cpdef void _handle_subscribe_quote_ticks(self, MarketDataClient client, SubscribeQuoteTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")
        if command.instrument_id.is_synthetic():
            self._handle_subscribe_synthetic_quote_ticks(command.instrument_id)
            return

        Condition.not_none(client, "client")

        if "start" not in command.params:
            last_timestamp: datetime | None = self._catalogs_timestamp_bound(
                data_cls=QuoteTick,
                instrument_id=command.instrument_id,
                is_last=True,
            )[0]

            # Time in nanoseconds from pd.Timestamp
            command.params["start"] = last_timestamp.value + 1 if last_timestamp else None

        if command.instrument_id not in client.subscribed_quote_ticks():
            client.subscribe_quote_ticks(command)

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

    cpdef void _handle_subscribe_trade_ticks(self, MarketDataClient client, SubscribeTradeTicks command):
        if command.instrument_id.is_synthetic():
            self._handle_subscribe_synthetic_trade_ticks(command.instrument_id)
            return
        Condition.not_none(client, "client")

        if "start" not in command.params:
            last_timestamp: datetime | None = self._catalogs_timestamp_bound(
                data_cls=TradeTick,
                instrument_id=command.instrument_id,
                is_last=True,
            )[0]

            # Time in nanoseconds from pd.Timestamp
            command.params["start"] = last_timestamp.value + 1 if last_timestamp else None

        if command.instrument_id not in client.subscribed_trade_ticks():
            client.subscribe_trade_ticks(command)

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

    cpdef void _handle_subscribe_mark_prices(self, MarketDataClient client, SubscribeMarkPrices command):
        Condition.not_none(client, "client")
        Condition.not_none(command.instrument_id, "instrument_id")

        if command.instrument_id not in client.subscribed_mark_prices():
            client.subscribe_mark_prices(command)

    cpdef void _handle_subscribe_index_prices(self, MarketDataClient client, SubscribeIndexPrices command):
        Condition.not_none(client, "client")
        Condition.not_none(command.instrument_id, "instrument_id")

        if command.instrument_id not in client.subscribed_index_prices():
            client.subscribe_index_prices(command)

    cpdef void _handle_subscribe_bars(self, MarketDataClient client, SubscribeBars command):
        Condition.not_none(client, "client")

        if command.bar_type.is_internally_aggregated():
            # Internal aggregation
            if command.bar_type.standard() not in self._bar_aggregators or not self._bar_aggregators[command.bar_type.standard()].is_running:
                self._start_bar_aggregator(client, command)
        else:
            # External aggregation
            if command.bar_type.instrument_id.is_synthetic():
                self._log.error(
                    "Cannot subscribe for externally aggregated synthetic instrument bar data",
                )
                return

            if "start" not in command.params:
                last_timestamp: datetime | None = self._catalogs_timestamp_bound(
                    data_cls=Bar,
                    bar_type=command.bar_type,
                    is_last=True,
                )[0]

                # Time in nanoseconds from pd.Timestamp
                command.params["start"] = last_timestamp.value + 1 if last_timestamp else None

            if command.bar_type not in client.subscribed_bars():
                client.subscribe_bars(command)

    cpdef void _handle_subscribe_data(self, DataClient client, SubscribeData command):
        Condition.not_none(client, "client")

        try:
            if command.data_type not in client.subscribed_custom_data():
                if "start" not in command.params:
                    last_timestamp: datetime | None = self._catalogs_timestamp_bound(data_cls=command.data_type.type, is_last=True)[0]
                    command.params["start"] = last_timestamp.value + 1 if last_timestamp else None

                client.subscribe(command)
        except NotImplementedError:
            self._log.error(
                f"Cannot subscribe: {client.id.value} "
                f"has not implemented {command.data_type} subscriptions",
            )
            return

    cpdef void _handle_subscribe_instrument_status(self, MarketDataClient client, SubscribeInstrumentStatus command):
        Condition.not_none(client, "client")

        if command.instrument_id.is_synthetic():
            self._log.error(
                "Cannot subscribe for synthetic instrument `InstrumentStatus` data",
            )
            return

        if command.instrument_id not in client.subscribed_instrument_status():
            client.subscribe_instrument_status(command)

    cpdef void _handle_subscribe_instrument_close(self, MarketDataClient client, SubscribeInstrumentClose command):
        Condition.not_none(client, "client")

        if command.instrument_id.is_synthetic():
            self._log.error("Cannot subscribe for synthetic instrument `InstrumentClose` data")
            return

        if command.instrument_id not in client.subscribed_instrument_close():
            client.subscribe_instrument_close(command)

    cpdef void _handle_unsubscribe_instruments(self, MarketDataClient client, UnsubscribeInstruments command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(f"data.instrument.{client.id.value}.*"):
            if client.subscribed_instruments():
                client.unsubscribe_instruments(command)

    cpdef void _handle_unsubscribe_instrument(self, MarketDataClient client, UnsubscribeInstrument command):
        Condition.not_none(client, "client")

        if command.instrument_id.is_synthetic():
            self._log.error("Cannot unsubscribe from synthetic instrument `Instrument` data")
            return

        if not self._msgbus.has_subscribers(
            f"data.instrument"
            f".{command.instrument_id.venue}"
            f".{command.instrument_id.symbol}",
        ):
            if command.instrument_id in client.subscribed_instruments():
                client.unsubscribe_instrument(command)

    cpdef void _handle_unsubscribe_order_book_deltas(self, MarketDataClient client, UnsubscribeOrderBook command):
        Condition.not_none(client, "client")
        Condition.not_none(command.params, "params")

        if command.instrument_id.is_synthetic():
            self._log.error("Cannot unsubscribe from synthetic instrument `OrderBookDelta` data")
            return

        cdef str topic = f"data.book.deltas.{command.instrument_id.venue}.{command.instrument_id.symbol.topic()}"
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
            if command.instrument_id in client.subscribed_order_book_deltas():
                client.unsubscribe_order_book_deltas(command)

    cpdef void _handle_unsubscribe_order_book_snapshots(self, MarketDataClient client, UnsubscribeOrderBook command):
        Condition.not_none(client, "client")
        Condition.not_none(command.params, "params")

        if command.instrument_id.is_synthetic():
            self._log.error("Cannot unsubscribe from synthetic instrument `OrderBook` data")
            return

        # Set up topics
        cdef str deltas_topic = f"data.book.deltas.{command.instrument_id.venue}.{command.instrument_id.symbol.topic()}"
        cdef str depth_topic = f"data.book.depth.{command.instrument_id.venue}.{command.instrument_id.symbol.topic()}"
        cdef str snapshots_topic = f"data.book.snapshots.{command.instrument_id.venue}.{command.instrument_id.symbol.topic()}"

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
            if command.instrument_id in client.subscribed_order_book_deltas():
                client.unsubscribe_order_book_deltas(command)

        if not self._msgbus.has_subscribers(snapshots_topic):
            if command.instrument_id in client.subscribed_order_book_snapshots():
                client.unsubscribe_order_book_snapshots(command)

    cpdef void _handle_unsubscribe_quote_ticks(self, MarketDataClient client, UnsubscribeQuoteTicks command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            f"data.quotes"
            f".{command.instrument_id.venue}"
            f".{command.instrument_id.symbol}",
        ):
            if command.instrument_id in client.subscribed_quote_ticks():
                client.unsubscribe_quote_ticks(command)

    cpdef void _handle_unsubscribe_trade_ticks(self, MarketDataClient client, UnsubscribeTradeTicks command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            f"data.trades"
            f".{command.instrument_id.venue}"
            f".{command.instrument_id.symbol}",
        ):
            if command.instrument_id in client.subscribed_trade_ticks():
                client.unsubscribe_trade_ticks(command)

    cpdef void _handle_unsubscribe_mark_prices(self, MarketDataClient client, UnsubscribeMarkPrices command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            f"data.mark_prices"
            f".{command.instrument_id.venue}"
            f".{command.instrument_id.symbol}",
        ):
            if command.instrument_id in client.subscribed_mark_prices():
                client.unsubscribe_mark_prices(command)

    cpdef void _handle_unsubscribe_index_prices(self, MarketDataClient client, UnsubscribeIndexPrices command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            f"data.index_prices"
            f".{command.instrument_id.venue}"
            f".{command.instrument_id.symbol}",
        ):
            if command.instrument_id in client.subscribed_index_prices():
                client.unsubscribe_index_prices(command)

    cpdef void _handle_unsubscribe_bars(self, MarketDataClient client, UnsubscribeBars command):
        Condition.not_none(client, "client")

        if self._msgbus.has_subscribers(f"data.bars.{command.bar_type.standard()}"):
            return

        if command.bar_type.is_internally_aggregated():
            # Internal aggregation
            if command.bar_type.standard() in self._bar_aggregators:
                self._stop_bar_aggregator(client, command)
        else:
            # External aggregation
            if command.bar_type in client.subscribed_bars():
                client.unsubscribe_bars(command)

    cpdef void _handle_unsubscribe_data(self, DataClient client, UnsubscribeData command):
        Condition.not_none(client, "client")

        try:
            if not self._msgbus.has_subscribers(f"data.{command.data_type}"):
                if command.data_type in client.subscribed_custom_data():
                    client.unsubscribe(command)
        except NotImplementedError:
            self._log.error(
                f"Cannot unsubscribe: {client.id.value} "
                f"has not implemented data type {command.data_type} subscriptions",
            )
            return

    cpdef void _handle_unsubscribe_instrument_status(self, MarketDataClient client, UnsubscribeInstrumentStatus command):
        Condition.not_none(client, "client")

        if command.instrument_id.is_synthetic():
            self._log.error(
                "Cannot unsubscribe for synthetic instrument `InstrumentStatus` data",
            )
            return

        if command.instrument_id in client.subscribed_instrument_status():
            client.unsubscribe_instrument_status(command)

    cpdef void _handle_unsubscribe_instrument_close(self, MarketDataClient client, UnsubscribeInstrumentClose command):
        Condition.not_none(client, "client")

        if command.instrument_id.is_synthetic():
            self._log.error("Cannot unsubscribe for synthetic instrument `InstrumentClose` data")
            return

        if command.instrument_id in client.subscribed_instrument_close():
            client.subscribe_instrument_close(command)

# -- REQUEST HANDLERS -----------------------------------------------------------------------------

    cpdef tuple[datetime, object] _catalogs_timestamp_bound(
        self,
        type data_cls,
        InstrumentId instrument_id = None,
        BarType bar_type = None,
        str ts_column = "ts_init",
        bint is_last = True,
    ):
        cdef datetime timestamp_bound = None
        cdef datetime prev_timestamp_bound = None
        timestamp_bound_catalog = None

        for catalog in self._catalogs.values():
            prev_timestamp_bound = timestamp_bound

            timestamp_bound = (max_date if is_last else min_date)(
                timestamp_bound,
                catalog.query_timestamp_bound(data_cls, instrument_id, bar_type, ts_column, is_last)
            )

            if (timestamp_bound is not None and
                (prev_timestamp_bound is None or
                 (is_last and timestamp_bound > prev_timestamp_bound)
                  or (timestamp_bound < prev_timestamp_bound))):
                timestamp_bound_catalog = catalog

        return timestamp_bound, timestamp_bound_catalog

    cpdef void _handle_request(self, RequestData request):
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
            Condition.is_true(isinstance(client, DataClient), "client was not a DataClient")

        request.start = time_object_to_dt(request.start)
        request.end = time_object_to_dt(request.end)

        if isinstance(request, RequestInstruments):
            self._handle_request_instruments(client, request)
        elif isinstance(request, RequestInstrument):
            self._handle_request_instrument(client, request)
        elif isinstance(request, RequestOrderBookSnapshot):
            self._handle_request_order_book_snapshot(client, request)
        elif isinstance(request, RequestQuoteTicks):
            self._handle_request_quote_ticks(client, request)
        elif isinstance(request, RequestTradeTicks):
            self._handle_request_trade_ticks(client, request)
        elif isinstance(request, RequestBars):
            self._handle_request_bars(client, request)
        else:
            self._handle_request_data(client, request)

    def _log_request_error(self, RequestData request):
        self._log.error(f"Cannot handle request: no client registered "
                        f"for '{request.client_id}', {request}")

    cpdef void _handle_request_instruments(self, DataClient client, RequestInstruments request):
        update_catalog_mode = request.params.get("update_catalog_mode", None)

        if self._catalogs and update_catalog_mode is None:
            self._query_catalog(request)
            return

        if client is None:
            self._log_request_error(request)
            return  # No client to handle request

        client.request_instruments(request)

    cpdef void _handle_request_instrument(self, DataClient client, RequestInstrument request):
        last_timestamp = self._catalogs_timestamp_bound(
            data_cls=Instrument,
            instrument_id=request.instrument_id,
            is_last=True,
        )[0]

        if last_timestamp:
            self._query_catalog(request)
            return

        if client is None:
            self._log_request_error(request)
            return  # No client to handle request

        client.request_instrument(request)

    cpdef void _handle_request_order_book_snapshot(self, DataClient client, RequestOrderBookSnapshot request):
        if client is None:
            self._log_request_error(request)
            return  # No client to handle request

        client.request_order_book_snapshot(request)

    cpdef void _date_range_client_request(self, DataClient client, RequestData request):
        if client is None:
            self._log_request_error(request)
            return # No client to handle request

        if isinstance(request, RequestBars):
            client.request_bars(request)
        elif isinstance(request, RequestQuoteTicks):
            client.request_quote_ticks(request)
        elif isinstance(request, RequestTradeTicks):
            client.request_trade_ticks(request)
        else:
            try:
                client.request(request)
            except:
                self._log.error(f"Cannot handle request: unrecognized data type {request.data_type}, {request}")

    def _convert_update_catalog_mode(self, update_catalog_mode: UpdateCatalogMode, catalog_write_mode: CatalogWriteMode) -> CatalogWriteMode | None:
        if update_catalog_mode is None:
            return None
        elif update_catalog_mode is UpdateCatalogMode.MODIFY:
            return catalog_write_mode
        elif update_catalog_mode is UpdateCatalogMode.OVERWRITE:
            return CatalogWriteMode.OVERWRITE
        elif update_catalog_mode is UpdateCatalogMode.NEWFILE:
            return CatalogWriteMode.NEWFILE

    cpdef void _handle_date_range_request(
        self,
        DataClient client,
        RequestData request,
        datetime start_catalog,
        datetime end_catalog,
    ):
        # No catalog to use
        if start_catalog is None:
            self._date_range_client_request(client, request)
            return

        # Capping dates to the now datetime
        cdef datetime now = self._clock.utc_now()
        cdef datetime used_start_catalog = min_date(start_catalog, now)
        cdef datetime used_end_catalog = min_date(end_catalog, now)
        cdef datetime used_start_request = min_date(request.start, now) if request.start is not None else time_object_to_dt(0)
        cdef datetime used_end_request = min_date(request.end, now) if request.start is not None else now

        if used_start_request > used_end_request:
            self._log.error(f"Cannot handle request: incompatible request dates for {request}")
            return

        # If the request dates are fully outside the catalog dates
        if used_end_request < used_start_catalog:
            self._date_range_client_request(client, request)
            return

        if used_start_request > used_end_catalog:
            self._date_range_client_request(client, request)
            return

        # From here the request dates have an intersection with the catalog
        # Number of requests for the request group that will wait for all requests to be completed.
        # One request at least for the catalog
        n_requests = 1

        if used_start_request < used_start_catalog and client is not None:
            n_requests += 1

        if used_end_request > used_end_catalog and client is not None:
            n_requests += 1

        self._new_query_group(request.id, n_requests)

        # Client query before the catalog
        if used_start_request < used_start_catalog and client is not None:
            new_request = request.with_dates(used_start_request, used_start_catalog)
            new_request.params["update_catalog_mode"] = self._convert_update_catalog_mode(
                new_request.params.get("update_catalog_mode", None),
                CatalogWriteMode.PREPEND
            )
            self._date_range_client_request(client, new_request)

        # Catalog query
        new_request = request.with_dates(max_date(used_start_request, used_start_catalog), min_date(used_end_request, used_end_catalog))
        new_request.params["update_catalog_mode"] = self._convert_update_catalog_mode(
            new_request.params.get("update_catalog_mode", None),
            None
        )
        self._query_catalog(new_request)

        # Client query after the catalog
        if used_end_request > used_end_catalog and client is not None:
            new_request = request.with_dates(used_end_catalog, used_end_request)
            new_request.params["update_catalog_mode"] = self._convert_update_catalog_mode(
                new_request.params.get("update_catalog_mode", None),
                CatalogWriteMode.APPEND
            )
            self._date_range_client_request(client, new_request)

    cpdef void _handle_request_quote_ticks(self, DataClient client, RequestQuoteTicks request):
        start_catalog = self._catalogs_timestamp_bound(
            data_cls=QuoteTick,
            instrument_id=request.instrument_id,
            is_last=False,
        )[0]
        end_catalog = self._catalogs_timestamp_bound(
            data_cls=QuoteTick,
            instrument_id=request.instrument_id,
            is_last=True,
        )[0]

        self._handle_date_range_request(
            client,
            request,
            start_catalog,
            end_catalog,
        )

    cpdef void _handle_request_trade_ticks(self, DataClient client, RequestTradeTicks request):
        start_catalog = self._catalogs_timestamp_bound(
            data_cls=TradeTick,
            instrument_id=request.instrument_id,
            is_last=False,
        )[0]
        end_catalog = self._catalogs_timestamp_bound(
            data_cls=TradeTick,
            instrument_id=request.instrument_id,
            is_last=True,
        )[0]

        self._handle_date_range_request(
            client,
            request,
            start_catalog,
            end_catalog,
        )

    cpdef void _handle_request_bars(self, DataClient client, RequestBars request):
        start_catalog = self._catalogs_timestamp_bound(
            data_cls=Bar,
            bar_type=request.bar_type,
            is_last=False,
        )[0]
        end_catalog = self._catalogs_timestamp_bound(
            data_cls=Bar,
            bar_type=request.bar_type,
            is_last=True,
        )[0]

        self._handle_date_range_request(
            client,
            request,
            start_catalog,
            end_catalog,
        )

    cpdef void _handle_request_data(self, DataClient client, RequestData request):
        start_catalog = self._catalogs_timestamp_bound(
            data_cls=request.data_type.type,
            is_last=False,
        )[0]
        end_catalog = self._catalogs_timestamp_bound(
            data_cls=request.data_type.type,
            is_last=True,
        )[0]

        self._handle_date_range_request(
            client,
            request,
            start_catalog,
            end_catalog,
        )

    cpdef void _query_catalog(self, RequestData request):
        cdef datetime start = request.start
        cdef datetime end = request.end

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

        data = []

        if isinstance(request, RequestInstruments):
            for catalog in self._catalogs.values():
                data += catalog.instruments()
        elif isinstance(request, RequestInstrument):
            for catalog in self._catalogs.values():
                data += catalog.instruments(instrument_ids=[str(request.instrument_id)])
        elif isinstance(request, RequestQuoteTicks):
            for catalog in self._catalogs.values():
                data += catalog.quote_ticks(
                    instrument_ids=[str(request.instrument_id)],
                    start=ts_start,
                    end=ts_end,
                )
        elif isinstance(request, RequestTradeTicks):
            for catalog in self._catalogs.values():
                data += catalog.trade_ticks(
                    instrument_ids=[str(request.instrument_id)],
                    start=ts_start,
                    end=ts_end,
                )
        elif isinstance(request, RequestBars):
            bar_type = request.bar_type
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
        elif isinstance(request, RequestData):
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

        params = request.params.copy()
        params["update_catalog_mode"] = None

        response = DataResponse(
            client_id=request.client_id,
            venue=request.venue,
            data_type=request.data_type,
            data=data,
            correlation_id=request.id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
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
        elif isinstance(data, MarkPriceUpdate):
            self._handle_mark_price(data)
        elif isinstance(data, IndexPriceUpdate):
            self._handle_index_price(data)
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

    cpdef void _handle_instrument(self, Instrument instrument, update_catalog_mode: CatalogWriteMode | None = None):
        self._cache.add_instrument(instrument)

        if update_catalog_mode is not None:
            self._update_catalog([instrument], update_catalog_mode, is_instrument=True)

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

    cpdef void _handle_mark_price(self, MarkPriceUpdate mark_price):
        self._cache.add_mark_price(mark_price)

        self._msgbus.publish_c(
            topic=f"data.mark_prices"
                  f".{mark_price.instrument_id.venue}"
                  f".{mark_price.instrument_id.symbol}",
            msg=mark_price,
        )

    cpdef void _handle_index_price(self, IndexPriceUpdate index_price):
        self._cache.add_index_price(index_price)

        self._msgbus.publish_c(
            topic=f"data.index_prices"
                  f".{index_price.instrument_id.venue}"
                  f".{index_price.instrument_id.symbol}",
            msg=index_price,
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

        # We may need to join responses from a catalog and a client
        response_2 = self._handle_query_group(response)

        if response_2 is None:
            return

        if response_2.data_type.type == Instrument:
            update_catalog_mode = response_2.params.get("update_catalog_mode", None)

            if isinstance(response_2.data, list):
                self._handle_instruments(response_2.data, update_catalog_mode)
            else:
                self._handle_instrument(response_2.data, update_catalog_mode)
        elif response_2.data_type.type == QuoteTick:
            if response_2.params.get("bars_market_data_type"):
                response_2.data = self._handle_aggregated_bars(response_2.data, response_2.params)
                response_2.data_type = DataType(Bar)
            else:
                self._handle_quote_ticks(response_2.data)
        elif response_2.data_type.type == TradeTick:
            if response_2.params.get("bars_market_data_type"):
                response_2.data = self._handle_aggregated_bars(response_2.data, response_2.params)
                response_2.data_type = DataType(Bar)
            else:
                self._handle_trade_ticks(response_2.data)
        elif response_2.data_type.type == Bar:
            if response_2.params.get("bars_market_data_type"):
                response_2.data = self._handle_aggregated_bars(response_2.data, response_2.params)
            else:
                self._handle_bars(response_2.data, response_2.data_type.metadata.get("partial"))

        self._msgbus.response(response_2)

    cpdef void _new_query_group(self, UUID4 correlation_id, int n_components):
        self._query_group_n_components[correlation_id] = n_components

    cpdef DataResponse _handle_query_group(self, DataResponse response):
        # Closure is not allowed in cpdef functions so we call a cdef function
        return self._handle_query_group_aux(response)

    cdef DataResponse _handle_query_group_aux(self, DataResponse response):
        if response.data_type.type is Instrument:
            return response

        correlation_id = response.correlation_id

        if correlation_id not in self._query_group_n_components or self._query_group_n_components[correlation_id] == 1:
            update_catalog_mode = response.params.get("update_catalog_mode", None)

            if update_catalog_mode is not None:
                self._update_catalog(response.data, update_catalog_mode)

            self._query_group_n_components.pop(correlation_id, None)

            return response

        if correlation_id not in self._query_group_components:
            self._query_group_components[correlation_id] = []

        self._query_group_components[correlation_id].append(response)
        if len(self._query_group_components[correlation_id]) != self._query_group_n_components[correlation_id]:
            return None

        components = []
        for component in self._query_group_components[correlation_id]:
            if len(component.data) > 0:
                components.append(component)

        components = sorted(components, key=lambda response: response.data[0].ts_init)
        result = []
        last_timestamp = None
        for component in components:
            first_non_duplicate_index = 0

            if last_timestamp is not None:
                for i in range(len(component.data)):
                    if component.data[i].ts_init > last_timestamp:
                        first_non_duplicate_index = i
                        break

            last_timestamp = component.data[-1].ts_init

            update_catalog_mode = component.params.get("update_catalog_mode", None)
            if update_catalog_mode is not None:
                self._update_catalog(component.data[first_non_duplicate_index:], update_catalog_mode)

            result += component.data[first_non_duplicate_index:]

        del self._query_group_n_components[correlation_id]
        del self._query_group_components[correlation_id]

        response.data = result

        return response

    cpdef void _update_catalog(self, list ticks, update_catalog_mode: CatalogWriteMode, bint is_instrument = False):
        if len(ticks) == 0:
            return

        if type(ticks[0]) is Bar:
            timestamp_bound_catalog = self._catalogs_timestamp_bound(
                data_cls=Bar,
                bar_type=ticks[0].bar_type,
                is_last=(update_catalog_mode is not CatalogWriteMode.PREPEND),
            )[1]
        else:
            timestamp_bound_catalog = self._catalogs_timestamp_bound(
                data_cls=type(ticks[0]),
                instrument_id=ticks[0].instrument_id,
                is_last=(update_catalog_mode is not CatalogWriteMode.PREPEND),
            )[1]

        # We don't want to write in the catalog several times the same instrument
        if timestamp_bound_catalog and is_instrument:
            return

        if timestamp_bound_catalog is None and len(self._catalogs) > 0:
            # If more than one catalog exists, the first declared one is the default one
            last_timestamp_catalog = list(self._catalogs.values())[0]

        if timestamp_bound_catalog is not None:
            timestamp_bound_catalog.write_data(ticks, mode=update_catalog_mode)
        else:
            self._log.warning("No catalog available for appending data.")

    cpdef void _handle_instruments(self, list instruments, update_catalog_mode: CatalogWriteMode | None = None):
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument, update_catalog_mode)

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

            if aggregator is not None:
                aggregator.set_await_partial(False)
                self._log.debug(f"Applying partial bar {partial} for {partial.bar_type}")
                aggregator.set_partial(partial)
            else:
                if self._fsm.state == ComponentState.RUNNING:
                    # Only log this error if the component is running, because
                    # there may have been an immediate stop called after start
                    # - with the partial bar being for a now removed aggregator.
                    self._log.error("No aggregator for partial bar update")

    cpdef dict _handle_aggregated_bars(self, list ticks, dict params):
        # Closure is not allowed in cpdef functions so we call a cdef function
        return self._handle_aggregated_bars_aux(ticks, params)

    cdef dict _handle_aggregated_bars_aux(self, list ticks, dict params):
        result = {}

        if len(ticks) == 0:
            self._log.warning("_handle_aggregated_bars: No data to aggregate")
            return result

        bars_result = {}

        if params["include_external_data"]:
            if params["bars_market_data_type"] == "quote_ticks":
                self._cache.add_quote_ticks(ticks)
                result["quote_ticks"] = ticks
            elif params["bars_market_data_type"] == "trade_ticks":
                self._cache.add_trade_ticks(ticks)
                result["trade_ticks"] = ticks
            elif params["bars_market_data_type"] == "bars":
                self._cache.add_bars(ticks)

        if params["bars_market_data_type"] == "bars":
            bars_result[params["bar_type"]] = ticks

        for bar_type in params["bar_types"]:
            if params["update_subscriptions"] and bar_type.standard() in self._bar_aggregators:
                aggregator = self._bar_aggregators[bar_type.standard()]
            else:
                instrument = self._cache.instrument(params["bar_type"].instrument_id)

                if instrument is None:
                    self._log.error(
                        f"Cannot start bar aggregation: "
                        f"no instrument found for {bar_type.instrument_id}",
                    )
                    continue

                aggregator = self._create_bar_aggregator(instrument, bar_type)

                if params["update_subscriptions"]:
                    self._bar_aggregators[bar_type.standard()] = aggregator

            aggregated_bars = []
            handler = lambda bar: aggregated_bars.append(bar)

            if params["bars_market_data_type"] == "quote_ticks" and not bar_type.is_composite():
                aggregator.start_batch_update(handler, ticks[0].ts_event)

                for tick in ticks:
                    aggregator.handle_quote_tick(tick)
            elif params["bars_market_data_type"] == "trade_ticks" and not bar_type.is_composite():
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

        if not params["include_external_data"] and params["bars_market_data_type"] == "bars":
            del bars_result[params["bar_type"]]

        # We need a second final dict as a we can't delete keys in a loop
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

    cpdef object _create_bar_aggregator(self, Instrument instrument, BarType bar_type):
        if bar_type.spec.is_time_aggregated():
            aggregator = TimeBarAggregator(
                instrument=instrument,
                bar_type=bar_type,
                handler=self.process,
                clock=self._clock,
                interval_type=self._time_bars_interval_type,
                timestamp_on_close=self._time_bars_timestamp_on_close,
                skip_first_non_full_bar=self._time_bars_skip_first_non_full_bar,
                build_with_no_updates=self._time_bars_build_with_no_updates,
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

        return aggregator

    cpdef void _start_bar_aggregator(self, MarketDataClient client, SubscribeBars command):
        cdef Instrument instrument = self._cache.instrument(command.bar_type.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot start bar aggregation: "
                f"no instrument found for {command.bar_type.instrument_id}",
            )
            return

        # An aggregator may already have been created with actor.request_aggregated_bars and _handle_aggregated_bars
        aggregator = self._bar_aggregators.get(command.bar_type.standard())

        if aggregator is None:
            aggregator = self._create_bar_aggregator(instrument, command.bar_type)

        # Set if awaiting initial partial bar
        aggregator.set_await_partial(command.await_partial)

        # Add aggregator
        self._bar_aggregators[command.bar_type.standard()] = aggregator
        self._log.debug(f"Added {aggregator} for {command.bar_type} bars")

        # Subscribe to required data
        if command.bar_type.is_composite():
            composite_bar_type = command.bar_type.composite()

            self._msgbus.subscribe(
                topic=f"data.bars.{composite_bar_type}",
                handler=aggregator.handle_bar,
            )
            subscribe = SubscribeBars(
                bar_type=composite_bar_type,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                await_partial=command.await_partial,
                params=command.params
            )
            self._handle_subscribe_bars(client, subscribe)
        elif command.bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.subscribe(
                topic=f"data.trades"
                      f".{command.bar_type.instrument_id.venue}"
                      f".{command.bar_type.instrument_id.symbol}",
                handler=aggregator.handle_trade_tick,
                priority=5,
            )
            subscribe = SubscribeTradeTicks(
                instrument_id=command.bar_type.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params
            )
            self._handle_subscribe_trade_ticks(client, subscribe)
        else:
            self._msgbus.subscribe(
                topic=f"data.quotes"
                      f".{command.bar_type.instrument_id.venue}"
                      f".{command.bar_type.instrument_id.symbol}",
                handler=aggregator.handle_quote_tick,
                priority=5,
            )
            subscribe = SubscribeQuoteTicks(
                instrument_id=command.bar_type.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params
            )
            self._handle_subscribe_quote_ticks(client, subscribe)

        aggregator.is_running = True

    cpdef void _stop_bar_aggregator(self, MarketDataClient client, UnsubscribeBars command):
        cdef aggregator = self._bar_aggregators.get(command.bar_type.standard())

        if aggregator is None:
            self._log.warning(
                f"Cannot stop bar aggregator: "
                f"no aggregator to stop for {command.bar_type}",
            )
            return

        if isinstance(aggregator, TimeBarAggregator):
            aggregator.stop()

        # Unsubscribe from market data updates
        if command.bar_type.is_composite():
            composite_bar_type = command.bar_type.composite()

            self._msgbus.unsubscribe(
                topic=f"data.bars.{composite_bar_type}",
                handler=aggregator.handle_bar,
            )
            unsubscribe = UnsubscribeBars(
                bar_type=composite_bar_type,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params
            )
            self._handle_unsubscribe_bars(client, unsubscribe)
        elif command.bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.unsubscribe(
                topic=f"data.trades"
                      f".{command.bar_type.instrument_id.venue}"
                      f".{command.bar_type.instrument_id.symbol}",
                handler=aggregator.handle_trade_tick,
            )
            unsubscribe = UnsubscribeTradeTicks(
                instrument_id=command.bar_type.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params
            )
            self._handle_unsubscribe_trade_ticks(client, unsubscribe)
        else:
            self._msgbus.unsubscribe(
                topic=f"data.quotes"
                      f".{command.bar_type.instrument_id.venue}"
                      f".{command.bar_type.instrument_id.symbol}",
                handler=aggregator.handle_quote_tick,
            )
            unsubscribe = UnsubscribeQuoteTicks(
                instrument_id=command.bar_type.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params
            )
            self._handle_unsubscribe_quote_ticks(client, unsubscribe)

        # Remove from aggregators
        del self._bar_aggregators[command.bar_type.standard()]

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
