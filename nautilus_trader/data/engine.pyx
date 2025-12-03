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

from typing import Any
from typing import Callable
from typing import Generator

from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import min_date
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.persistence.catalog import ParquetDataCatalog

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport RECV
from nautilus_trader.common.component cimport REQ
from nautilus_trader.common.component cimport RES
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TestClock
from nautilus_trader.common.data_topics cimport TopicCache
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_MILLISECOND
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_SECOND
from nautilus_trader.core.rust.core cimport millis_to_nanos
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.aggregation cimport BarAggregator
from nautilus_trader.data.aggregation cimport RenkoBarAggregator
from nautilus_trader.data.aggregation cimport TickBarAggregator
from nautilus_trader.data.aggregation cimport TickImbalanceBarAggregator
from nautilus_trader.data.aggregation cimport TickRunsBarAggregator
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.aggregation cimport ValueBarAggregator
from nautilus_trader.data.aggregation cimport ValueImbalanceBarAggregator
from nautilus_trader.data.aggregation cimport ValueRunsBarAggregator
from nautilus_trader.data.aggregation cimport VolumeBarAggregator
from nautilus_trader.data.aggregation cimport VolumeImbalanceBarAggregator
from nautilus_trader.data.aggregation cimport VolumeRunsBarAggregator
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.engine cimport SnapshotInfo
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport RequestBars
from nautilus_trader.data.messages cimport RequestData
from nautilus_trader.data.messages cimport RequestInstrument
from nautilus_trader.data.messages cimport RequestInstruments
from nautilus_trader.data.messages cimport RequestJoin
from nautilus_trader.data.messages cimport RequestOrderBookDepth
from nautilus_trader.data.messages cimport RequestOrderBookSnapshot
from nautilus_trader.data.messages cimport RequestQuoteTicks
from nautilus_trader.data.messages cimport RequestTradeTicks
from nautilus_trader.data.messages cimport SubscribeBars
from nautilus_trader.data.messages cimport SubscribeData
from nautilus_trader.data.messages cimport SubscribeFundingRates
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
from nautilus_trader.data.messages cimport UnsubscribeFundingRates
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
from nautilus_trader.model.data cimport FundingRateUpdate
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport MarkPriceUpdate
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.data cimport bar_aggregation_not_implemented_message
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
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

        self._request_group_parent_request: dict[UUID4, RequestData] = {}
        self._request_group_n_components: dict[UUID4, int] = {}
        self._request_group_parent_request_id: dict[UUID4, UUID4] = {}
        self._request_group_responses: dict[UUID4, list] = {}
        self._long_request_generator: dict[UUID4, object] = {}
        self._requests: dict[UUID4, RequestData] = {}
        self._parent_long_request_id: dict[UUID4, UUID4] = {}
        self._parent_join_request_id: dict[UUID4, UUID4] = {}

        self._topic_cache = TopicCache()

        # Configuration
        self.debug = config.debug
        self._time_bars_interval_type = config.time_bars_interval_type
        self._time_bars_timestamp_on_close = config.time_bars_timestamp_on_close
        self._time_bars_skip_first_non_full_bar = config.time_bars_skip_first_non_full_bar
        self._time_bars_build_with_no_updates = config.time_bars_build_with_no_updates
        self._time_bars_origin_offset = config.time_bars_origin_offset or {}
        self._time_bars_build_delay = config.time_bars_build_delay
        self._validate_data_sequence = config.validate_data_sequence
        self._buffer_deltas = config.buffer_deltas
        self._emit_quotes_from_book = config.emit_quotes_from_book
        self._emit_quotes_from_book_depths = config.emit_quotes_from_book_depths

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
        self._msgbus.register(endpoint="DataEngine.process_historical", handler=self.process_historical)

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

    @property
    def routing_map(self) -> dict[Venue, DataClient]:
        """
        Return the default data client registered with the engine.

        Returns
        -------
        ClientId or ``None``

        """
        return self._routing_map

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

    cpdef set[ClientId] get_external_client_ids(self):
        """
        Returns the configured external client order IDs.

        Returns
        -------
        set[ClientId]

        """
        return self._external_clients.copy()

    cpdef bint _is_backtest_client(self, DataClient client):
        """
        Check if we're in a backtest context by looking for the default backtest client.

        Returns
        -------
        bool
            True if in backtest context, else False.

        """
        # Avoid importing `BacktestMarketDataClient` from the `backtest` subpackage at
        # module import time – doing so creates a circular import between
        # `nautilus_trader.data` and `nautilus_trader.backtest`.
        from nautilus_trader.backtest.data_client import BacktestMarketDataClient

        return isinstance(client, BacktestMarketDataClient)

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

    cpdef bint is_live_mode(self):
        cdef ClientId backtest_client_id = ClientId("backtest_default_client")

        return backtest_client_id not in self._clients

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_custom_data(self):
        """
        Return the custom data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        cdef:
            list subscriptions = []
            DataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_index_prices()

        return subscriptions

    cpdef list subscribed_funding_rates(self):
        """
        Return the funding rate update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef:
            list subscriptions = []
            MarketDataClient client
        for client in [c for c in self._clients.values() if isinstance(c, MarketDataClient)]:
            subscriptions += client.subscribed_funding_rates()

        return subscriptions

    cpdef list subscribed_bars(self):
        """
        Return the bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        cdef:
            list subscriptions = []
            MarketDataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
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
        cdef:
            list subscriptions = []
            MarketDataClient client
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
                aggregator.stop_timer()

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

        self._request_group_parent_request.clear()
        self._request_group_n_components.clear()
        self._request_group_parent_request_id.clear()
        self._request_group_responses.clear()
        self._long_request_generator.clear()
        self._requests.clear()
        self._parent_long_request_id.clear()
        self._parent_join_request_id.clear()

        self._topic_cache.clear_cache()

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

    cpdef void process(self, Data data, bint historical = False):
        """
        Process the given data.

        Parameters
        ----------
        data : Data
            The data to process.

        """
        Condition.not_none(data, "data")

        self._handle_data(data, historical)

    cpdef void process_historical(self, Data data):
        """
        Process historical data.

        Parameters
        ----------
        data : Data
            The historical data to process.
        """
        self._handle_data(data, historical=True)

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
                f"Skipping data command for external client {command.client_id}: {command}",
                LogColor.MAGENTA,
            )
            return

        # In a backtest context, we never want to subscribe to live data
        cdef:
            ClientId backtest_client_id = ClientId("backtest_default_client")
            DataClient client
        if backtest_client_id in self._clients:
            client = self._clients[backtest_client_id]
        else:
            client = self._clients.get(command.client_id)

        cdef Venue venue = command.venue
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
            self._handle_subscribe_order_book(client, command)
        elif isinstance(command, SubscribeQuoteTicks):
            self._handle_subscribe_quote_ticks(client, command)
        elif isinstance(command, SubscribeTradeTicks):
            self._handle_subscribe_trade_ticks(client, command)
        elif isinstance(command, SubscribeMarkPrices):
            self._handle_subscribe_mark_prices(client, command)
        elif isinstance(command, SubscribeIndexPrices):
            self._handle_subscribe_index_prices(client, command)
        elif isinstance(command, SubscribeFundingRates):
            self._handle_subscribe_funding_rates(client, command)
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
            self._handle_unsubscribe_order_book(client, command)
        elif isinstance(command, UnsubscribeQuoteTicks):
            self._handle_unsubscribe_quote_ticks(client, command)
        elif isinstance(command, UnsubscribeTradeTicks):
            self._handle_unsubscribe_trade_ticks(client, command)
        elif isinstance(command, UnsubscribeMarkPrices):
            self._handle_unsubscribe_mark_prices(client, command)
        elif isinstance(command, UnsubscribeIndexPrices):
            self._handle_unsubscribe_index_prices(client, command)
        elif isinstance(command, UnsubscribeFundingRates):
            self._handle_unsubscribe_funding_rates(client, command)
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

    cpdef void _handle_subscribe_order_book(self, MarketDataClient client, SubscribeOrderBook command):
        Condition.not_none(client, "client")

        if command.instrument_id.is_synthetic():
            self._log.error(f"Cannot subscribe for synthetic instrument `{command.data_type.type}` data")
            return

        if command.data_type.type == OrderBookDelta:
            if command.instrument_id not in client.subscribed_order_book_deltas():
                client.subscribe_order_book_deltas(command)
        elif command.data_type.type == OrderBookDepth10:
            if command.instrument_id not in client.subscribed_order_book_snapshots():
                client.subscribe_order_book_depth(command)
        else:  # pragma: no cover (design-time error)
            raise TypeError(f"Invalid book data type, was {command.data_type}")

        cdef:
            str topic
            uint64_t interval_ns
            uint64_t timestamp_ns
            SnapshotInfo snap_info
            tuple[InstrumentId, int] key

        if command.interval_ms > 0:
            key = (command.instrument_id, command.interval_ms)

            if key not in self._order_book_intervals:
                self._order_book_intervals[key] = []

                timer_name = f"OrderBook|{command.instrument_id}|{command.interval_ms}"
                interval_ns = millis_to_nanos(command.interval_ms)
                timestamp_ns = self._clock.timestamp_ns()
                start_time_ns = timestamp_ns - (timestamp_ns % interval_ns)

                topic = self._topic_cache.get_snapshots_topic(command.instrument_id, command.interval_ms)

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

        if command.managed:
            self._setup_order_book(client, command)

    cpdef void _setup_order_book(self, MarketDataClient client, SubscribeOrderBook command):
        cdef Instrument instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            self._log.warning(
                f"No instrument found for {command.instrument_id} on order book data subscription"
            )

        cdef:
            list[Instrument] instruments
            str root

        # Create order book(s)
        if command.instrument_id.symbol.is_composite():
            root = command.instrument_id.symbol.root()
            instruments = self._cache.instruments(venue=command.instrument_id.venue, underlying=root)

            for instrument in instruments:
                self._create_new_book(instrument.id, command.book_type)
        else:
            self._create_new_book(command.instrument_id, command.book_type)

        cdef str topic = self._topic_cache.get_book_topic(command.data_type.type, command.instrument_id)
        if not self._msgbus.is_subscribed(
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

        if "start_ns" not in command.params:
            last_timestamp: datetime | None = self._catalog_last_timestamp(QuoteTick, str(command.instrument_id))[0]
            command.params["start_ns"] = last_timestamp.value + 1 if last_timestamp else None

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

        if "start_ns" not in command.params:
            last_timestamp: datetime | None = self._catalog_last_timestamp(TradeTick, str(command.instrument_id))[0]
            command.params["start_ns"] = last_timestamp.value + 1 if last_timestamp else None

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

    cpdef void _handle_subscribe_funding_rates(self, MarketDataClient client, SubscribeFundingRates command):
        Condition.not_none(client, "client")
        Condition.not_none(command.instrument_id, "instrument_id")

        if command.instrument_id not in client.subscribed_funding_rates():
            client.subscribe_funding_rates(command)

    cpdef void _handle_subscribe_bars(self, MarketDataClient client, SubscribeBars command):
        Condition.not_none(client, "client")

        if command.bar_type.is_internally_aggregated():
            # Internal aggregation
            bar_type_standard = command.bar_type.standard()

            if bar_type_standard not in self._bar_aggregators:
                # Aggregator doesn't exist, create and start it
                self._start_bar_aggregator(client, command)
            elif not self._bar_aggregators[bar_type_standard].is_running:
                # Aggregator exists but not running, start it
                self._start_bar_aggregator(client, command)
            else:
                # Aggregator exists and is running
                self._log.warning(f"Aggregator for {bar_type_standard} is currently in use, subscription can't be started.")
        else:
            # External aggregation
            if command.bar_type.instrument_id.is_synthetic():
                self._log.error(
                    "Cannot subscribe for externally aggregated synthetic instrument bar data",
                )
                return

            if "start_ns" not in command.params:
                last_timestamp: datetime | None = self._catalog_last_timestamp(Bar, str(command.bar_type))[0]
                command.params["start_ns"] = last_timestamp.value + 1 if last_timestamp else None

            if command.bar_type not in client.subscribed_bars():
                client.subscribe_bars(command)

    cpdef void _handle_subscribe_data(self, DataClient client, SubscribeData command):
        Condition.not_none(client, "client")

        try:
            if command.data_type not in client.subscribed_custom_data():
                if "start_ns" not in command.params:
                    last_timestamp: datetime | None = self._catalog_last_timestamp(command.data_type.type, str(command.instrument_id))[0]
                    command.params["start_ns"] = last_timestamp.value + 1 if last_timestamp else None

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

    cpdef void _handle_unsubscribe_order_book(self, MarketDataClient client, UnsubscribeOrderBook command):
        Condition.not_none(client, "client")

        if command.instrument_id.is_synthetic():
            self._log.error(f"Cannot unsubscribe from synthetic instrument `{command.data_type.type}` data")
            return

        # If the internal order book is the last subscriber on this topic, remove it
        cdef str topic = self._topic_cache.get_book_topic(command.data_type.type, command.instrument_id)
        cdef int num_subscribers = len(self._msgbus.subscriptions(pattern=topic))
        cdef bint is_internal_book_subscriber = self._msgbus.is_subscribed(
            topic=topic,
            handler=self._update_order_book,
        )

        if num_subscribers == 1 and is_internal_book_subscriber:
            self._msgbus.unsubscribe(
                topic=topic,
                handler=self._update_order_book,
            )

        # If no more subscribers to the client-backed topic, unsubscribe the client
        if not self._msgbus.has_subscribers(topic):
            if command.data_type.type == OrderBookDelta:
                if command.instrument_id in client.subscribed_order_book_deltas():
                    client.unsubscribe_order_book_deltas(command)
            elif command.data_type.type == OrderBookDepth10:
                if command.instrument_id in client.subscribed_order_book_snapshots():
                    client.unsubscribe_order_book_depth(command)
            else:
                if command.instrument_id in client.subscribed_order_book_snapshots():
                    client.unsubscribe_order_book_snapshots(command)

        # Cancel any snapshot timers for this instrument that no longer have subscribers
        cdef:
            tuple[InstrumentId, int] key
            list[tuple[InstrumentId, int]] keys_to_remove = []
            str timer_name
            str snapshots_topic
        for key in list(self._order_book_intervals.keys()):
            if key[0] != command.instrument_id:
                continue

            snapshots_topic = self._topic_cache.get_snapshots_topic(key[0], key[1])

            if not self._msgbus.has_subscribers(snapshots_topic):
                timer_name = f"OrderBook|{key[0]}|{key[1]}"
                self._clock.cancel_timer(timer_name)
                keys_to_remove.append(key)

                if timer_name in self._snapshot_info:
                    del self._snapshot_info[timer_name]

        for key in keys_to_remove:
            del self._order_book_intervals[key]

    cpdef void _handle_unsubscribe_quote_ticks(self, MarketDataClient client, UnsubscribeQuoteTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            self._topic_cache.get_quotes_topic(command.instrument_id),
        ):
            if command.instrument_id in client.subscribed_quote_ticks():
                client.unsubscribe_quote_ticks(command)

    cpdef void _handle_unsubscribe_trade_ticks(self, MarketDataClient client, UnsubscribeTradeTicks command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            self._topic_cache.get_trades_topic(command.instrument_id),
        ):
            if command.instrument_id in client.subscribed_trade_ticks():
                client.unsubscribe_trade_ticks(command)

    cpdef void _handle_unsubscribe_mark_prices(self, MarketDataClient client, UnsubscribeMarkPrices command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            self._topic_cache.get_mark_prices_topic(command.instrument_id),
        ):
            if command.instrument_id in client.subscribed_mark_prices():
                client.unsubscribe_mark_prices(command)

    cpdef void _handle_unsubscribe_index_prices(self, MarketDataClient client, UnsubscribeIndexPrices command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            self._topic_cache.get_index_prices_topic(command.instrument_id),
        ):
            if command.instrument_id in client.subscribed_index_prices():
                client.unsubscribe_index_prices(command)

    cpdef void _handle_unsubscribe_funding_rates(self, MarketDataClient client, UnsubscribeFundingRates command):
        Condition.not_none(client, "client")

        if not self._msgbus.has_subscribers(
            self._topic_cache.get_funding_rates_topic(command.instrument_id),
        ):
            if command.instrument_id in client.subscribed_funding_rates():
                client.unsubscribe_funding_rates(command)

    cpdef void _handle_unsubscribe_bars(self, MarketDataClient client, UnsubscribeBars command):
        Condition.not_none(client, "client")

        if self._msgbus.has_subscribers(self._topic_cache.get_bars_topic(command.bar_type.standard())):
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

        # Only unsubscribe if currently subscribed
        if command.instrument_id in client.subscribed_instrument_close():
            client.unsubscribe_instrument_close(command)

# -- REQUEST HANDLERS -----------------------------------------------------------------------------

    cpdef void _handle_request(self, RequestData request):
        if self.debug:
            self._log.debug(f"{RECV}{REQ} {request}", LogColor.MAGENTA)

        self.request_count += 1

        if request.params.get("join_request", False):
            self._requests[request.id] = request
            return

        # Get data client
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

        if ("time_range_generator" in request.params
                and not (isinstance(request, RequestJoin) and request.correlation_id is None)):
            self._handle_long_request(client, request)
            return

        if isinstance(request, RequestJoin):
            self._handle_request_join(request)

        if request.params.get("bar_types"):
            self._init_historical_aggregators(request)

        if isinstance(request, RequestInstruments):
            self._handle_request_instruments(client, request)
        elif isinstance(request, RequestInstrument):
            self._handle_request_instrument(client, request)
        elif isinstance(request, RequestOrderBookSnapshot):
            self._handle_request_order_book_snapshot(client, request)
        elif isinstance(request, RequestOrderBookDepth):
            self._handle_request_order_book_depth(client, request)
        elif isinstance(request, RequestQuoteTicks):
            self._handle_request_quote_ticks(client, request)
        elif isinstance(request, RequestTradeTicks):
            self._handle_request_trade_ticks(client, request)
        elif isinstance(request, RequestBars):
            self._handle_request_bars(client, request)
        else:
            self._handle_request_data(client, request)

    cpdef void _handle_request_instruments(self, DataClient client, RequestInstruments request):
        update_catalog = request.params.get("update_catalog", False)
        force_instrument_update = request.params.get("force_instrument_update", False)

        if self._catalogs and not update_catalog and not force_instrument_update:
            self._query_catalog(request)
            return

        if client is None:
            self._log_request_warning(request)
            return  # No client to handle request

        client.request_instruments(request)

    cpdef void _handle_request_instrument(self, DataClient client, RequestInstrument request):
        last_timestamp = self._catalog_last_timestamp(Instrument, str(request.instrument_id))[0]
        force_instrument_update = request.params.get("force_instrument_update", False)

        if last_timestamp and not force_instrument_update:
            self._query_catalog(request)
            return

        if client is None:
            self._log_request_warning(request)
            return  # No client to handle request

        client.request_instrument(request)

    cpdef void _handle_request_order_book_snapshot(self, DataClient client, RequestOrderBookSnapshot request):
        if client is None:
            self._log_request_warning(request)
            return  # No client to handle request

        client.request_order_book_snapshot(request)

    cpdef void _handle_request_order_book_depth(self, DataClient client, RequestOrderBookDepth request):
        self._handle_date_range_request(client, request)

    cpdef void _handle_request_quote_ticks(self, DataClient client, RequestQuoteTicks request):
        self._handle_date_range_request(client, request)

    cpdef void _handle_request_trade_ticks(self, DataClient client, RequestTradeTicks request):
        self._handle_date_range_request(client, request)

    cpdef void _handle_request_bars(self, DataClient client, RequestBars request):
        self._handle_date_range_request(client, request)

    cpdef void _handle_request_data(self, DataClient client, RequestData request):
        self._handle_date_range_request(client, request)

    cpdef void _handle_date_range_request(self, DataClient client, RequestData request):
        cdef DataClient used_client = client
        if self._is_backtest_client(used_client):
            used_client = None

        # Capping dates to the now datetime
        start, end = self._bound_dates(request)
        cdef datetime now = self._clock.utc_now()

        if start > end:
            self._log.error(f"Cannot handle request: incompatible request dates for {request}")
            return

        cdef list query_interval = [(start.value, end.value)]
        cdef list missing_intervals = query_interval
        cdef bint has_catalog_data = False

        if isinstance(request, RequestBars):
            identifier = request.bar_type
        else:
            identifier = request.instrument_id

        # We assume each symbol is only in one catalog
        for catalog in self._catalogs.values():
            missing_intervals = catalog.get_missing_intervals_for_request(
                start.value,
                end.value,
                request.data_type.type,
                identifier,
            )
            has_catalog_data = missing_intervals != query_interval

            if has_catalog_data:
                break

        skip_catalog_data = request.params.get("skip_catalog_data", False)
        n_requests = (len(missing_intervals) if used_client else 0) + (1 if has_catalog_data and not skip_catalog_data else 0)
        request.params["identifier"] = identifier # Allows to update catalog file names when no data is returned

        # From here the parent request is split into subrequests
        if n_requests == 0:
            self._new_request_group(request, 1)
            self._request_group_parent_request_id[request.id] = request.id
            response = DataResponse(
                client_id=request.client_id,
                venue=request.venue,
                data_type=request.data_type,
                data=[],
                correlation_id=request.id,
                response_id=UUID4(),
                start=request.start,
                end=request.end,
                ts_init=self._clock.timestamp_ns(),
                params=request.params,
            )
            self._handle_response(response)
            return

        self._new_request_group(request, n_requests)

        # Catalog query
        if has_catalog_data and not skip_catalog_data:
            new_request = request.with_dates(start, end, now.value)
            self._request_group_parent_request_id[new_request.id] = new_request.correlation_id
            self._query_catalog(new_request)

        # Client requests
        if len(missing_intervals) > 0 and used_client:
            for request_start, request_end in missing_intervals:
                new_request = request.with_dates(time_object_to_dt(request_start), time_object_to_dt(request_end), now.value)
                self._request_group_parent_request_id[new_request.id] = new_request.correlation_id
                self._date_range_client_request(used_client, new_request)

    cpdef void _date_range_client_request(self, DataClient client, RequestData request):
        if isinstance(request, RequestBars):
            client.request_bars(request)
        elif isinstance(request, RequestQuoteTicks):
            client.request_quote_ticks(request)
        elif isinstance(request, RequestTradeTicks):
            client.request_trade_ticks(request)
        elif isinstance(request, RequestOrderBookDepth):
            client.request_order_book_depth(request)
        else:
            try:
                client.request(request)
            except:
                self._log.error(f"Cannot handle request: unrecognized data type {request.data_type}, {request}")

    def _log_request_warning(self, RequestData request):
        self._log.warning(f"Cannot handle request: no client registered for '{request.client_id}', {request}")

    cpdef void _query_catalog(self, RequestData request):
        cdef datetime start = request.start
        cdef datetime end = request.end
        cdef bint query_past_data = request.params.get("subscription_name") is None

        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef uint64_t ts_start = dt_to_unix_nanos(start) if start is not None else 0
        cdef uint64_t ts_end = dt_to_unix_nanos(end) if end is not None else ts_now

        # Validate request time range
        Condition.is_true(ts_start <= ts_end, f"{ts_start=} was greater than {ts_end=}")

        if end is not None and ts_end > ts_now and query_past_data:
            self._log.warning(
                "Cannot request data beyond current time. "
                f"Truncating `end` to current UNIX nanoseconds {unix_nanos_to_dt(ts_now)}",
            )
            ts_end = ts_now

        data = []

        # We assume each symbol is only in one catalog
        for catalog in self._catalogs.values():
            if isinstance(request, RequestInstruments):
                # We only use ts_end if end is passed as request argument
                data += catalog.instruments(
                    start=ts_start,
                    end=(ts_end if end is not None else None),
                )
            elif isinstance(request, RequestInstrument):
                # We only use ts_end if end is passed as request argument
                data = catalog.instruments(
                    instrument_ids=[str(request.instrument_id)],
                    start=ts_start,
                    end=(ts_end if end is not None else None),
                )
            elif isinstance(request, RequestQuoteTicks):
                data = catalog.quote_ticks(
                    instrument_ids=[str(request.instrument_id)],
                    start=ts_start,
                    end=ts_end,
                )
            elif isinstance(request, RequestTradeTicks):
                data = catalog.trade_ticks(
                    instrument_ids=[str(request.instrument_id)],
                    start=ts_start,
                    end=ts_end,
                )
            elif isinstance(request, RequestBars):
                bar_type = request.bar_type
                if bar_type is None:
                    self._log.error("No bar type provided for bars request")
                    return

                data = catalog.bars(
                    instrument_ids=[str(bar_type.instrument_id)],
                    bar_types=[str(bar_type)],
                    start=ts_start,
                    end=ts_end,
                )
            elif isinstance(request, RequestOrderBookDepth):
                data = catalog.order_book_depth10(
                    instrument_ids=[str(request.instrument_id)],
                    start=ts_start,
                    end=ts_end,
                )
            elif type(request) is RequestData:
                data = catalog.custom_data(
                    cls=request.data_type.type,
                    instrument_ids=[str(request.instrument_id)] if request.instrument_id else None,
                    metadata=request.data_type.metadata,
                    start=ts_start,
                    end=ts_end,
                )

            if data and not isinstance(request, RequestInstruments):
                break

        # Validate data is not from the future
        if data and data[-1].ts_init > ts_now and query_past_data:
            raise RuntimeError(
                "Invalid response: Historical data from the future: "
                f"data[-1].ts_init={data[-1].ts_init}, {ts_now=}",
            )

        if isinstance(request, RequestInstrument):
            if len(data) == 0:
                self._log.error(f"Cannot find instrument for {request.instrument_id}")
                return

        if isinstance(request, RequestInstruments) or isinstance(request, RequestInstrument):
            only_last = request.params.get("only_last", True)

            if only_last:
                # Retains only the latest instrument record per instrument_id, based on the most recent ts_init
                last_instrument = {}

                for instrument in data:
                    if instrument.id not in last_instrument:
                        last_instrument[instrument.id] = instrument
                    elif instrument.ts_init > last_instrument[instrument.id].ts_init:
                        last_instrument[instrument.id] = instrument

                data = list(last_instrument.values())

        params = request.params.copy()
        params["update_catalog"] = False

        response = DataResponse(
            client_id=request.client_id,
            venue=request.venue,
            data_type=request.data_type,
            data=data,
            correlation_id=request.id,
            response_id=UUID4(),
            start=request.start,
            end=request.end,
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._handle_response(response)

    cpdef void _handle_long_request(self, DataClient client, RequestData request):
        start, end = self._bound_dates(request)
        request.start, request.end = start, end
        self._requests[request.id] = request

        time_range_generator = get_time_range_generator(
            request.params.get("time_range_generator", "")
        )(request)
        self._long_request_generator[request.id] = time_range_generator

        self._update_long_request_data(request.id, is_first_call=True)

    cpdef void _update_long_request_data(
        self,
        UUID4 parent_request_id,
        bint data_received = False,
        bint is_first_call = False
    ):
        time_range_generator = self._long_request_generator.get(parent_request_id)
        if time_range_generator is None:
            self._log.error(f"No time range generator found for request {parent_request_id}")
            return

        cdef RequestData parent_request = self._requests.get(parent_request_id)
        if parent_request is None:
            self._log.error(f"No parent request found for {parent_request_id}")
            return

        # Get next time range from generator
        cdef:
            uint64_t request_start_ns
            uint64_t request_end_ns
        try:
            if is_first_call:
                request_start_ns, request_end_ns = next(time_range_generator)
            else:
                request_start_ns, request_end_ns = time_range_generator.send(data_received)
        except StopIteration:
            # No more intervals, send final empty response
            self._finalize_long_request(parent_request_id)
            return

        if parent_request.end is not None and request_start_ns > parent_request.end.value:
            self._finalize_long_request(parent_request_id)
            return

        request_end_ns = min(request_end_ns, parent_request.end.value)

        # Create a sub-request for this interval
        cdef datetime now = self._clock.utc_now()
        cdef RequestData new_request = parent_request.with_dates(
            unix_nanos_to_dt(request_start_ns),
            unix_nanos_to_dt(request_end_ns),
            now.value,
            self._handle_long_request_response
        )

        # We remove time_range_generator from params to avoid an infinite recursion
        new_request.params.pop("time_range_generator", None)

        # Send the sub-request through the message bus to properly register the callback
        self._parent_long_request_id[new_request.id] = parent_request_id
        self._msgbus.request(endpoint="DataEngine.request", request=new_request)

    cpdef void _handle_long_request_response(self, DataResponse response):
        cdef:
            UUID4 sub_request_id = response.correlation_id
            UUID4 parent_request_id = self._parent_long_request_id.pop(sub_request_id, None)
        if parent_request_id is None:
            self._log.error(f"No parent request ID found for sub-request {sub_request_id}")
            return

        # Storing information about the data count received in the parent request's params
        cdef int data_count = response.params.get("data_count", 0)
        cdef RequestData parent_request = self._requests.get(parent_request_id)
        parent_request.params["data_count"] = parent_request.params.get("data_count", 0) + data_count

        # Process the next interval with feedback on if data was received
        cdef bint data_received = data_count > 0
        self._update_long_request_data(parent_request_id, data_received=data_received)

    cpdef void _finalize_long_request(self, UUID4 parent_request_id):
        cdef RequestData parent_request = self._requests.pop(parent_request_id, None)
        if parent_request is None:
            self._log.error(f"Cannot finalize long request: no parent request found for {parent_request_id}")
            return

        # Close the generator
        time_range_generator = self._long_request_generator.pop(parent_request_id, None)
        if time_range_generator is not None:
            try:
                time_range_generator.close()
            except (StopIteration, GeneratorExit):
                pass

        # Send response for the original request to trigger its callback
        response = DataResponse(
            client_id=parent_request.client_id,
            venue=parent_request.venue,
            data_type=parent_request.data_type,
            data=[],
            correlation_id=parent_request_id,
            response_id=UUID4(),
            start=parent_request.start,
            end=parent_request.end,
            ts_init=self._clock.timestamp_ns(),
            params=parent_request.params,
        )
        self._msgbus.response(response)

    cpdef void _handle_request_join(self, RequestJoin request):
        if not request.correlation_id:
            self._requests[request.id] = request
            start, end = self._bound_dates(request)
            new_request = request.with_dates(start, end, self._clock.timestamp_ns(), self._finalize_request_join)
            self._parent_join_request_id[new_request.id] = request.id
            self._msgbus.request(endpoint="DataEngine.request", request=new_request)
            return

        self._new_request_group(request, len(request.request_ids))

        for request_id in request.request_ids:
            joined_request = self._requests.get(request_id)
            new_request = joined_request.with_dates(request.start, request.end, self._clock.timestamp_ns())
            new_request.params["join_request"] = False
            self._request_group_parent_request_id[new_request.id] = request.id
            self._msgbus.request(endpoint="DataEngine.request", request=new_request)

    cpdef void _finalize_request_join(self, DataResponse response):
        parent_request_id = self._parent_join_request_id.pop(response.correlation_id, None)
        if not parent_request_id:
            self._log.error(f"parent_request_id for {response.correlation_id=} not found.")
            return

        parent_request = self._requests.pop(parent_request_id, None)
        if not parent_request:
            self._log.error(f"parent_request for {parent_request_id=} not found.")
            return

        # We send responses for the joined requests and the joining request to trigger callbacks
        for request_id in parent_request.request_ids:
            joined_request = self._requests.pop(request_id, None)
            if not joined_request:
                self._log.error(f"joined_request for {request_id=} not found.")
                continue

            response = DataResponse(
                client_id=joined_request.client_id,
                venue=joined_request.venue,
                data_type=joined_request.data_type,
                data=[],
                correlation_id=joined_request.id,
                response_id=UUID4(),
                start=None,
                end=None,
                ts_init=self._clock.timestamp_ns(),
                params=joined_request.params,
            )
            self._msgbus.response(response)

        response = DataResponse(
            client_id=parent_request.client_id,
            venue=parent_request.venue,
            data_type=parent_request.data_type,
            data=[],
            correlation_id=parent_request.id,
            response_id=UUID4(),
            start=parent_request.start,
            end=parent_request.end,
            ts_init=self._clock.timestamp_ns(),
            params=parent_request.params,
        )
        self._msgbus.response(response)

    cpdef tuple _bound_dates(self, RequestData request):
        # Capping dates to the now datetime
        cdef bint query_past_data = request.params.get("subscription_name") is None
        cdef datetime now = self._clock.utc_now()

        cdef datetime start = request.start if request.start is not None else time_object_to_dt(0)
        cdef datetime end = request.end if request.end is not None else now

        if query_past_data:
            start = min_date(start, now)
            end = min_date(end, now)

        return start, end

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_data(self, Data data, bint historical = False):
        self.data_count += 1

        if isinstance(data, OrderBookDelta):
            self._handle_order_book_delta(data, historical)
        elif isinstance(data, OrderBookDeltas):
            self._handle_order_book_deltas(data, historical)
        elif isinstance(data, OrderBookDepth10):
            self._handle_order_book_depth(data, historical)
        elif isinstance(data, QuoteTick):
            self._handle_quote_tick(data, historical)
        elif isinstance(data, TradeTick):
            self._handle_trade_tick(data, historical)
        elif isinstance(data, MarkPriceUpdate):
            self._handle_mark_price(data, historical)
        elif isinstance(data, IndexPriceUpdate):
            self._handle_index_price(data, historical)
        elif isinstance(data, FundingRateUpdate):
            self._handle_funding_rate(data, historical)
        elif isinstance(data, Bar):
            self._handle_bar(data, historical)
        elif isinstance(data, Instrument):
            self._handle_instrument(data, historical)
        elif isinstance(data, InstrumentStatus):
            self._handle_instrument_status(data, historical)
        elif isinstance(data, InstrumentClose):
            self._handle_close_price(data, historical)
        elif isinstance(data, CustomData):
            self._handle_custom_data(data, historical)
        else:
            self._log.error(f"Cannot handle data: unrecognized type {type(data)} {data}")

    cpdef void _handle_instrument(
        self,
        Instrument instrument,
        bint historical = False,
        dict params = None,
    ):
        self._cache.add_instrument(instrument)

        if params is None:
            params = {}

        instrument_properties = params.get("instrument_properties")
        update_catalog = params.get("update_catalog", False)
        force_update_catalog = params.get("force_update_catalog", False)
        modified_instrument = self._modify_instrument_properties(instrument, instrument_properties)

        if update_catalog:
            self._update_catalog(
                [modified_instrument],
                Instrument,
                instrument.id,
                is_instrument=True,
                force_update_catalog=force_update_catalog,
            )

        self._msgbus.publish_c(
            topic=self._topic_cache.get_instrument_topic(modified_instrument.id, historical),
            msg=modified_instrument,
        )

    cpdef Instrument _modify_instrument_properties(self, Instrument instrument, dict instrument_properties):
        if instrument_properties is None:
            return instrument

        instrument_dict = type(instrument).to_dict(instrument)
        instrument_dict.update(instrument_properties)

        return type(instrument).from_dict(instrument_dict)

    cpdef void _handle_order_book_delta(
        self,
        OrderBookDelta delta,
        bint historical = False
    ):
        cdef:
            OrderBookDeltas deltas = None
            list[OrderBookDelta] buffer_deltas = None
            bint is_last_delta = False
            InstrumentId instrument_id = delta.instrument_id
        if self._buffer_deltas:
            buffer_deltas = self._buffered_deltas_map.get(instrument_id)
            if buffer_deltas is None:
                buffer_deltas = []
                self._buffered_deltas_map[instrument_id] = buffer_deltas

            buffer_deltas.append(delta)

            is_last_delta = delta.flags == RecordFlag.F_LAST
            if is_last_delta:
                deltas = OrderBookDeltas(
                    instrument_id=instrument_id,
                    deltas=buffer_deltas
                )
                self._msgbus.publish_c(
                    topic=self._topic_cache.get_deltas_topic(instrument_id, historical),
                    msg=deltas,
                )
                buffer_deltas.clear()
        else:
            deltas = OrderBookDeltas(
                instrument_id=instrument_id,
                deltas=[delta]
            )
            self._msgbus.publish_c(
                topic=self._topic_cache.get_deltas_topic(instrument_id, historical),
                msg=deltas,
            )

    cpdef void _handle_order_book_deltas(self, OrderBookDeltas deltas, bint historical = False):
        cdef:
            OrderBookDeltas deltas_to_publish = None
            list[OrderBookDelta] buffer_deltas = None
            bint is_last_delta = False
            InstrumentId instrument_id = deltas.instrument_id
        if self._buffer_deltas:
            buffer_deltas = self._buffered_deltas_map.get(instrument_id)
            if buffer_deltas is None:
                buffer_deltas = []
                self._buffered_deltas_map[instrument_id] = buffer_deltas

            for delta in deltas.deltas:
                buffer_deltas.append(delta)

                is_last_delta = delta.flags == RecordFlag.F_LAST
                if is_last_delta:
                    deltas_to_publish = OrderBookDeltas(
                        instrument_id=instrument_id,
                        deltas=buffer_deltas,
                    )
                    self._msgbus.publish_c(
                        topic=self._topic_cache.get_deltas_topic(instrument_id, historical),
                        msg=deltas_to_publish,
                    )
                    buffer_deltas.clear()
        else:
            self._msgbus.publish_c(
                topic=self._topic_cache.get_deltas_topic(instrument_id, historical),
                msg=deltas,
            )

    cpdef void _handle_order_book_depth(self, OrderBookDepth10 depth, bint historical = False):
        # Publish the depth data
        self._msgbus.publish_c(
            topic=self._topic_cache.get_depth_topic(depth.instrument_id, historical),
            msg=depth,
        )

        cdef:
            QuoteTick quote_tick
            QuoteTick last_quote
        if self._emit_quotes_from_book_depths:
            quote_tick = depth.to_quote_tick()
            if quote_tick is not None:
                # Check if top of book has changed
                last_quote = self._cache.quote_tick(depth.instrument_id)
                if last_quote is None or (
                    quote_tick.bid_price != last_quote.bid_price or
                    quote_tick.ask_price != last_quote.ask_price or
                    quote_tick.bid_size != last_quote.bid_size or
                    quote_tick.ask_size != last_quote.ask_size
                ):
                    self._handle_quote_tick(quote_tick)

    cpdef void _handle_quote_tick(self, QuoteTick tick, bint historical = False):
        self._cache.add_quote_tick(tick)

        # Handle synthetics update
        cdef:
            InstrumentId instrument_id = tick.instrument_id
            list synthetics = self._synthetic_quote_feeds.get(instrument_id)
        if synthetics is not None:
            self._update_synthetics_with_quote(synthetics, tick)

        self._msgbus.publish_c(
            topic=self._topic_cache.get_quotes_topic(instrument_id, historical),
            msg=tick,
        )

    cpdef void _handle_trade_tick(self, TradeTick tick, bint historical = False):
        self._cache.add_trade_tick(tick)

        # Handle synthetics update
        cdef:
            InstrumentId instrument_id = tick.instrument_id
            list synthetics = self._synthetic_trade_feeds.get(instrument_id)
        if synthetics is not None:
            self._update_synthetics_with_trade(synthetics, tick)

        self._msgbus.publish_c(
            topic=self._topic_cache.get_trades_topic(instrument_id, historical),
            msg=tick,
        )

    cpdef void _handle_mark_price(self, MarkPriceUpdate mark_price, bint historical = False):
        self._cache.add_mark_price(mark_price)

        self._msgbus.publish_c(
            topic=self._topic_cache.get_mark_prices_topic(mark_price.instrument_id, historical),
            msg=mark_price,
        )

    cpdef void _handle_index_price(self, IndexPriceUpdate index_price, bint historical = False):
        self._cache.add_index_price(index_price)

        self._msgbus.publish_c(
            topic=self._topic_cache.get_index_prices_topic(index_price.instrument_id, historical),
            msg=index_price,
        )

    cpdef void _handle_funding_rate(self, FundingRateUpdate funding_rate, bint historical = False):
        self._cache.add_funding_rate(funding_rate)

        self._msgbus.publish_c(
            topic=self._topic_cache.get_funding_rates_topic(funding_rate.instrument_id, historical),
            msg=funding_rate,
        )

    cpdef void _handle_bar(self, Bar bar, bint historical = False):
        cdef:
            BarType bar_type = bar.bar_type
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

        self._msgbus.publish_c(topic=self._topic_cache.get_bars_topic(bar_type, historical), msg=bar)

    cpdef void _handle_instrument_status(self, InstrumentStatus data, bint historical = False):
        self._msgbus.publish_c(topic=self._topic_cache.get_status_topic(data.instrument_id, historical), msg=data)

    cpdef void _handle_close_price(self, InstrumentClose data, bint historical = False):
        self._msgbus.publish_c(topic=self._topic_cache.get_close_prices_topic(data.instrument_id, historical), msg=data)

    cpdef void _handle_custom_data(self, CustomData data, bint historical = False):
        cdef InstrumentId instrument_id = getattr(data.data, "instrument_id", None)
        cdef str topic = self._topic_cache.get_custom_data_topic(data.data_type, instrument_id, historical)
        self._msgbus.publish_c(topic=topic, msg=data.data)

# -- RESPONSE HANDLERS ----------------------------------------------------------------------------

    cpdef void _handle_response(self, DataResponse response):
        if self.debug:
            self._log.debug(f"{RECV}{RES} {response}", LogColor.MAGENTA)

        self.response_count += 1

        # We may need to join responses from a catalog and a client
        grouped_response = None
        if response.data_type.type == Instrument:
            grouped_response = response
        else:
            grouped_response = self._handle_request_group(response)

        if grouped_response is None:
            return

        # When a request group is part of another request group (RequestJoin case)
        if self._request_group_parent_request_id.get(grouped_response.correlation_id):
            self._handle_response(grouped_response)
            return

        cdef:
            bint query_past_data = response.params.get("subscription_name") is None
            Data data
        if query_past_data or grouped_response.data_type.type == Instrument:
            if grouped_response.data_type.type == Instrument:
                for data in grouped_response.data:
                    self._handle_instrument(data, params=grouped_response.params)

                grouped_response.data = []
            else:
                for data in grouped_response.data:
                    self.process_historical(data)

                if grouped_response.params.get("bar_types"):
                    self._handle_aggregated_bars(grouped_response)

                # We store the amount of data received to be used for long requests
                grouped_response.params["data_count"] = len(grouped_response.data)
                grouped_response.data = []

        self._msgbus.response(grouped_response)

    cpdef void _new_request_group(self, RequestData request, int n_components):
        # The parent request is stored so the grouped response can use its information
        self._request_group_n_components[request.id] = n_components
        self._request_group_parent_request[request.id] = request
        self._request_group_responses[request.id] = []

    cpdef DataResponse _handle_request_group(self, DataResponse response):
        # Closure is not allowed in cpdef functions so we call a cdef function
        return self._handle_request_group_aux(response)

    cdef DataResponse _handle_request_group_aux(self, DataResponse response):
        correlation_id = response.correlation_id

        # Look for parent request id using the mapping
        parent_request_id = self._request_group_parent_request_id.get(correlation_id)
        if parent_request_id is not None:
            self._request_group_parent_request_id.pop(correlation_id, None)

        if parent_request_id not in self._request_group_responses:
            self._log.error(f"_handle_request_group_aux: correlation_id {correlation_id} not found "
                            f"in _request_group_responses. Available keys: {list(self._request_group_responses.keys())}")
            return None

        self._request_group_responses[parent_request_id].append(response)

        if len(self._request_group_responses[parent_request_id]) != self._request_group_n_components[parent_request_id]:
            return None

        cdef list responses = self._request_group_responses[parent_request_id]
        cdef list data_result = []

        for response in responses:
            self._check_bounds(response)

            update_catalog = response.params.get("update_catalog", False)
            if update_catalog:
                start = response.start.value if response.start is not None else None
                end = response.end.value if response.end is not None else None
                identifier = response.params.get("identifier")
                self._update_catalog(
                    response.data,
                    response.data_type.type,
                    identifier,
                    start,
                    end
                )

            data_result += response.data

        data_result.sort(key=lambda x: x.ts_init)

        # Use the parent request to ensure the correct response parameters are returned to the caller.
        parent_request = self._request_group_parent_request[parent_request_id]
        response.data = data_result
        response.start = parent_request.start
        response.end = parent_request.end
        response.correlation_id = parent_request_id
        response.id = UUID4()
        response.params = parent_request.params

        del self._request_group_n_components[parent_request_id]
        del self._request_group_parent_request[parent_request_id]
        del self._request_group_responses[parent_request_id]

        return response

    cpdef void _check_bounds(self, DataResponse response):
        cdef int data_len = len(response.data)
        if data_len == 0:
            return

        cdef:
            uint64_t start = response.start.value if response.start is not None else 0
            cdef int first_index = 0
        if start:
            for i in range(data_len):
                if response.data[i].ts_init >= start:
                    first_index = i
                    break

        cdef:
            uint64_t end = response.end.value if response.end is not None else 0
            int last_index = data_len - 1
        if end:
            for i in range(data_len-1, -1, -1):
                if response.data[i].ts_init <= end:
                    last_index = i
                    break

        if first_index <= last_index:
            response.data = response.data[first_index:last_index + 1]

    def _update_catalog(
        self,
        data: list,
        data_cls: type,
        identifier: object,
        start: int | None = None,
        end: int | None = None,
        is_instrument: bool = False,
        force_update_catalog: bool = False,
    ) -> None:
        # Works with InstrumentId or BarType
        used_catalog = self._catalog_last_timestamp(data_cls, identifier)[1]

        # We don't want to write in the catalog several times the same instrument
        if used_catalog and is_instrument and not force_update_catalog:
            return

        # If more than one catalog exists, use the first declared one as default
        if used_catalog is None and len(self._catalogs) > 0:
            used_catalog = list(self._catalogs.values())[0]

        if used_catalog is None:
            self._log.warning("No catalog available for appending data.")
            return

        if len(data) == 0 and data_cls and start and end:
            # identifier can be None for custom data
            used_catalog.extend_file_name(data_cls, identifier, start, end)
        else:
            used_catalog.write_data(data, start, end)

    cpdef tuple[datetime, object] _catalog_last_timestamp(
        self,
        type data_cls,
        identifier = str | None,
    ):
        # We assume each symbol is only in one catalog
        for catalog in self._catalogs.values():
            last_timestamp = catalog.query_last_timestamp(data_cls, identifier)
            if last_timestamp:
                return last_timestamp, catalog

        return None, None

    # -- INTERNAL -------------------------------------------------------------------------------------

    # Python wrapper to enable callbacks
    cpdef void _internal_update_instruments(self, list instruments):
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cpdef void _update_order_book(self, Data data):
        cdef OrderBook order_book = self._cache.order_book(data.instrument_id)
        if order_book is None:
            return

        order_book.apply(data)

        cdef:
            QuoteTick quote_tick
            QuoteTick last_quote
        if self._emit_quotes_from_book:
            quote_tick = order_book.to_quote_tick()
            if quote_tick is not None:
                # Check if top of book has changed
                last_quote = self._cache.quote_tick(data.instrument_id)
                if last_quote is None or (
                    quote_tick.bid_price != last_quote.bid_price or
                    quote_tick.ask_price != last_quote.ask_price or
                    quote_tick.bid_size != last_quote.bid_size or
                    quote_tick.ask_size != last_quote.ask_size
                ):
                    self._handle_quote_tick(quote_tick)

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

    cpdef void _update_synthetics_with_quote(self, list synthetics, QuoteTick update):
        cdef SyntheticInstrument synthetic
        for synthetic in synthetics:
            self._update_synthetic_with_quote(synthetic, update)

    cpdef void _update_synthetic_with_quote(self, SyntheticInstrument synthetic, QuoteTick update):
        cdef:
            list components = synthetic.components
            list[double] inputs_bid = []
            list[double] inputs_ask = []
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
            topic=self._topic_cache.get_quotes_topic(synthetic_instrument_id),
            msg=synthetic_quote,
        )

    cpdef void _update_synthetics_with_trade(self, list synthetics, TradeTick update):
        cdef SyntheticInstrument synthetic
        for synthetic in synthetics:
            self._update_synthetic_with_trade(synthetic, update)

    cpdef void _update_synthetic_with_trade(self, SyntheticInstrument synthetic, TradeTick update):
        cdef:
            list components = synthetic.components
            list[double] inputs = []
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
            topic=self._topic_cache.get_trades_topic(synthetic_instrument_id),
            msg=synthetic_trade,
        )

    # -- INTERNAL - Bar Aggregators -------------------------------------------------------------------

    cpdef void _init_historical_aggregators(self, RequestData request):
        bar_types = request.params.get("bar_types", ())
        for bar_type in bar_types:
            self._create_bar_aggregator(bar_type, request.params)
            aggregator = self._bar_aggregators.get(bar_type.standard())

            # No need to setup again an already existing aggregator (kept with update_subscriptions) in historical_mode
            if aggregator and not aggregator.historical_mode:
                self._setup_bar_aggregator(bar_type, historical=True)

    cpdef void _start_bar_aggregator(self, MarketDataClient client, SubscribeBars command):
        self._create_bar_aggregator(command.bar_type, command.params)
        self._setup_bar_aggregator(command.bar_type)
        self._subscribe_bar_aggregator(client, command)

    cpdef BarAggregator _create_bar_aggregator(self, BarType bar_type, dict params):
        aggregated_bar_type = bar_type.standard()
        if aggregated_bar_type in self._bar_aggregators:
            self._log.debug(f"BarAggregator for {aggregated_bar_type} already exists.")
            return

        instrument = self._cache.instrument(bar_type.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot start bar aggregation: "
                f"no instrument found for {bar_type.instrument_id}",
            )
            return

        if bar_type.spec.is_time_aggregated():
            time_bars_origin_offset = self._time_bars_origin_offset.get(bar_type.spec.aggregation) or params.get("time_bars_origin_offset")
            aggregator = TimeBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
                clock=self._clock,
                interval_type=self._time_bars_interval_type,
                timestamp_on_close=self._time_bars_timestamp_on_close,
                skip_first_non_full_bar=self._time_bars_skip_first_non_full_bar,
                build_with_no_updates=self._time_bars_build_with_no_updates,
                time_bars_origin_offset=time_bars_origin_offset,
                bar_build_delay=self._time_bars_build_delay,
            )
        elif bar_type.spec.aggregation == BarAggregation.TICK:
            aggregator = TickBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.TICK_IMBALANCE:
            aggregator = TickImbalanceBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.TICK_RUNS:
            aggregator = TickRunsBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.VOLUME:
            aggregator = VolumeBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.VOLUME_IMBALANCE:
            aggregator = VolumeImbalanceBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.VOLUME_RUNS:
            aggregator = VolumeRunsBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.VALUE:
            aggregator = ValueBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.VALUE_IMBALANCE:
            aggregator = ValueImbalanceBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.VALUE_RUNS:
            aggregator = ValueRunsBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        elif bar_type.spec.aggregation == BarAggregation.RENKO:
            aggregator = RenkoBarAggregator(
                instrument=instrument,
                bar_type=aggregated_bar_type,
                handler=self.process,
            )
        else:
            raise NotImplementedError(  # pragma: no cover (design-time error)
                bar_aggregation_not_implemented_message(bar_type.spec.aggregation)
            )

        self._bar_aggregators[aggregated_bar_type] = aggregator

    cpdef void _setup_bar_aggregator(
            self,
            BarType bar_type,
            bint historical = False,
    ):
        aggregator = self._bar_aggregators.get(bar_type.standard())
        if aggregator is None:
            self._log.error(f"Cannot setup bar aggregator: no aggregator found for {bar_type}")
            return

        if historical:
            aggregator.set_historical_mode(historical, self.process_historical)
        else:
            if aggregator.historical_mode:
                # When switching from historical to live mode we unsubscribe from a historical topic
                self._dispose_bar_aggregator(bar_type, historical=True)

            aggregator.set_historical_mode(historical, self.process)

        # Subscribe aggregator to message bus to receive underlying data
        if bar_type.is_composite():
            self._msgbus.subscribe(
                topic=self._topic_cache.get_bars_topic(bar_type.composite(), historical),
                handler=aggregator.handle_bar,
            )
        elif bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.subscribe(
                topic=self._topic_cache.get_trades_topic(bar_type.instrument_id, historical),
                handler=aggregator.handle_trade_tick,
                priority=5,
            )
        else:
            self._msgbus.subscribe(
                topic=self._topic_cache.get_quotes_topic(bar_type.instrument_id, historical),
                handler=aggregator.handle_quote_tick,
                priority=5,
            )

        if isinstance(aggregator, TimeBarAggregator):
            if historical:
                # Each aggregator gets its own independent clock
                test_clock = TestClock()
                aggregator.set_clock(test_clock)
            else:
                aggregator.set_clock(self._clock)
                aggregator.start_timer()

        aggregator.set_running(True)

    cpdef void _subscribe_bar_aggregator(self, MarketDataClient client, SubscribeBars command):
        # Subscribe to required market data
        if command.bar_type.is_composite():
            composite_bar_type = command.bar_type.composite()
            if composite_bar_type.is_externally_aggregated():
                subscribe = SubscribeBars(
                    bar_type=composite_bar_type,
                    client_id=command.client_id,
                    venue=command.venue,
                    command_id=UUID4(),
                    ts_init=command.ts_init,
                    params=command.params,
                    correlation_id=command.id,
                )
                self._handle_subscribe_bars(client, subscribe)
        elif command.bar_type.spec.price_type == PriceType.LAST:
            subscribe = SubscribeTradeTicks(
                instrument_id=command.bar_type.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=UUID4(),
                ts_init=command.ts_init,
                params=command.params,
                correlation_id=command.id,
            )
            self._handle_subscribe_trade_ticks(client, subscribe)
        else:
            subscribe = SubscribeQuoteTicks(
                instrument_id=command.bar_type.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=UUID4(),
                ts_init=command.ts_init,
                params=command.params,
                correlation_id=command.id,
            )
            self._handle_subscribe_quote_ticks(client, subscribe)

    cpdef void _handle_aggregated_bars(self, DataResponse response):
        # 1. Create aggregators (in _handle_request)
        # 2. Process underlying data through aggregators (in _handle_response)
        # 3. Handle aggregator lifecycle based on update_subscriptions
        update_subscriptions = response.params.get("update_subscriptions", False)

        bar_types = response.params.get("bar_types", ())
        for bar_type in bar_types:
            aggregator = self._bar_aggregators.get(bar_type.standard())
            if not aggregator:
                continue

            # Setting aggregator.is_running to False allows to start a live subscription
            # allowing the aggregator to still aggregate historical data if update_subscriptions is True
            aggregator.set_running(False)

            if not update_subscriptions:
                self._dispose_bar_aggregator(bar_type, historical=True)
                self._bar_aggregators.pop(bar_type.standard(), None)

    cpdef void _stop_bar_aggregator(self, MarketDataClient client, UnsubscribeBars command):
        aggregator = self._bar_aggregators.get(command.bar_type.standard())
        if aggregator is None:
            self._log.warning(
                f"Cannot stop bar aggregator: "
                f"no aggregator to stop for {command.bar_type}",
            )
            return

        if isinstance(aggregator, TimeBarAggregator):
            aggregator.stop_timer()

        self._dispose_bar_aggregator(command.bar_type)
        self._unsubscribe_aggregator(client, command)

        del self._bar_aggregators[command.bar_type.standard()]

    cpdef void _dispose_bar_aggregator(self, BarType bar_type, bint historical = False):
        aggregator = self._bar_aggregators.get(bar_type.standard())
        if aggregator is None:
            self._log.error(f"Cannot dispose bar aggregator: no aggregator found for {bar_type}")
            return

        if bar_type.is_composite():
            self._msgbus.unsubscribe(
                topic=self._topic_cache.get_bars_topic(bar_type.composite(), historical),
                handler=aggregator.handle_bar,
            )
        elif bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.unsubscribe(
                topic=self._topic_cache.get_trades_topic(bar_type.instrument_id, historical),
                handler=aggregator.handle_trade_tick,
            )
        else:
            self._msgbus.unsubscribe(
                topic=self._topic_cache.get_quotes_topic(bar_type.instrument_id, historical=historical),
                handler=aggregator.handle_quote_tick,
            )

    cpdef void _unsubscribe_aggregator(self, MarketDataClient client, UnsubscribeBars command):
        # Unsubscribe from market data updates
        if command.bar_type.is_composite():
            composite_bar_type = command.bar_type.composite()
            if composite_bar_type.is_externally_aggregated():
                unsubscribe = UnsubscribeBars(
                    bar_type=composite_bar_type,
                    client_id=command.client_id,
                    venue=command.venue,
                    command_id=UUID4(),
                    ts_init=command.ts_init,
                    params=command.params,
                    correlation_id=command.id,
                )
                self._handle_unsubscribe_bars(client, unsubscribe)
        elif command.bar_type.spec.price_type == PriceType.LAST:
            unsubscribe = UnsubscribeTradeTicks(
                instrument_id=command.bar_type.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=UUID4(),
                ts_init=command.ts_init,
                params=command.params,
                correlation_id=command.id,
            )
            self._handle_unsubscribe_trade_ticks(client, unsubscribe)
        else:
            unsubscribe = UnsubscribeQuoteTicks(
                instrument_id=command.bar_type.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=UUID4(),
                ts_init=command.ts_init,
                params=command.params,
                correlation_id=command.id,
            )
            self._handle_unsubscribe_quote_ticks(client, unsubscribe)

TimeRangeGenerator = Callable[[int, dict[str, Any]], Generator[int, bool, None]]


cdef dict[str, TimeRangeGenerator] TIME_RANGE_GENERATORS = {}


def register_time_range_generator(name: str, function: TimeRangeGenerator):
    TIME_RANGE_GENERATORS[name] = function


def get_time_range_generator(name: str):
    return TIME_RANGE_GENERATORS.get(name, default_time_range_generator)


def default_time_range_generator(RequestData request):
    """
    Generator that yields (request_start_ns, request_end_ns) tuples for subrequests.

    This generator handles the duration logic and receives data_received feedback via .send().
    """
    cdef uint64_t prev_request_end_ns = request.start.value
    cdef uint64_t last_end_ns = request.end.value
    cdef uint64_t duration_ns = 0

    point_data = request.params.get("point_data", False)
    durations_seconds = request.params.get("durations_seconds", [None])

    iteration_index = 0
    while True:
        for duration_seconds in durations_seconds:
            # Possibility to use durations of various lengths to take into account weekends or market breaks
            # First iteration for [a, a + duration], then ]a + duration, a + 2 * duration], etc.
            # When point_data we do a query for [request_start_ns, request_start_ns] only
            offset = 1 if iteration_index > 0 and not point_data else 0
            request_start_ns = prev_request_end_ns + offset

            if request_start_ns > last_end_ns:
                return

            if duration_seconds is not None:
                duration_ns = duration_seconds * NANOSECONDS_IN_SECOND
                request_end_ns = min(request_start_ns + duration_ns - offset, last_end_ns)
            else:
                request_end_ns = last_end_ns

            prev_request_end_ns = request_end_ns

            if point_data:
                request_end_ns = request_start_ns

            # Yield the time range and wait for feedback
            data_received = yield (request_start_ns, request_end_ns)
            iteration_index += 1

            # When requesting a single point we exit the generator directly
            if duration_ns == 0:
                return

            # If we receive a success signal, break from the duration loop
            if data_received:
                break
        else:
            # If we complete the for loop without breaking (no success) we exit the generator
            return
