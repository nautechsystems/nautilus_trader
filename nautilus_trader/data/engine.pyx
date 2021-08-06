# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

Beneath it sits a `DataCache` which presents a read-only facade for consumers.
The engine employs a simple fan-in fan-out messaging pattern to execute
`DataCommand` type messages, and process `DataResponse` messages or market data
objects.

Alternative implementations can be written on top of the generic engine - which
just need to override the `execute`, `process`, `send` and `receive` methods.
"""

from cpython.datetime cimport timedelta
from typing import Callable

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport REQ
from nautilus_trader.common.logging cimport RES
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.aggregation cimport BarAggregator
from nautilus_trader.data.aggregation cimport BulkTimeBarUpdater
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
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.venue cimport InstrumentClosePrice
from nautilus_trader.model.data.venue cimport InstrumentStatusUpdate
from nautilus_trader.model.data.venue cimport StatusUpdate
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.msgbus.message_bus cimport MessageBus


cdef class DataEngine(Component):
    """
    Provides a high-performance data engine for managing many `DataClient`
    instances, for the asynchronous ingest of data.
    """

    def __init__(
        self,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``DataEngine`` class.

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
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        super().__init__(
            clock=clock,
            logger=logger,
            component_id=ComponentId("DataEngine"),
        )

        self._msgbus = msgbus
        self._cache = cache

        self._use_previous_close = config.get("use_previous_close", True)
        self._clients = {}               # type: dict[ClientId, DataClient]
        self._order_book_intervals = {}  # type: dict[(InstrumentId, int), list[Callable[[Bar], None]]]
        self._bar_aggregators = {}       # type: dict[BarType, BarAggregator]

        # Counters
        self.command_count = 0
        self.data_count = 0
        self.request_count = 0
        self.response_count = 0

        self._log.info(f"use_previous_close={self._use_previous_close}")

        # Register endpoints
        self._msgbus.register(endpoint="DataEngine.execute", handler=self.execute)
        self._msgbus.register(endpoint="DataEngine.process", handler=self.process)
        self._msgbus.register(endpoint="DataEngine.request", handler=self.request)
        self._msgbus.register(endpoint="DataEngine.response", handler=self.response)

    @property
    def registered_clients(self):
        """
        The data clients registered with the data engine.

        Returns
        -------
        list[ClientId]

        """
        return sorted(list(self._clients.keys()))

    @property
    def subscribed_generic_data(self):
        """
        The generic data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        return None  # TODO

    @property
    def subscribed_instruments(self):
        """
        The instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return []  # TODO

    @property
    def subscribed_order_book_deltas(self):
        """
        The order books data (diffs) subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return []  # TODO

    @property
    def subscribed_order_book_snapshots(self):
        """
        The order books subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list interval_instruments = [k[0] for k in self._order_book_intervals.keys()]
        return []  # TODO

    @property
    def subscribed_quote_ticks(self):
        """
        The quote tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return []  # TODO

    @property
    def subscribed_trade_ticks(self):
        """
        The trade tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return []  # TODO

    @property
    def subscribed_bars(self):
        """
        The bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        return []  # TODO

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

# --REGISTRATION -----------------------------------------------------------------------------------

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
            If client is already registered.

        """
        Condition.not_none(client, "client")
        Condition.not_in(client.id, self._clients, "client", "self._clients")

        self._clients[client.id] = client

        self._log.info(f"Registered {client}.")

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

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *:
        pass  # Optionally override in subclass

    cpdef void _on_stop(self) except *:
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        cdef DataClient client
        for client in self._clients.values():
            client.connect()

        self._on_start()

    cpdef void _stop(self) except *:
        cdef DataClient client
        for client in self._clients.values():
            client.disconnect()

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

# -- COMMANDS --------------------------------------------------------------------------------------

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

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef void _execute_command(self, DataCommand command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        cdef DataClient client = self._clients.get(command.client_id)
        if client is None:
            self._log.error(
                f"Cannot handle command: "
                f"no client registered for '{command.client_id}' in {self.registered_clients}, "
                f" {command}.")
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
        elif command.data_type.type == OrderBook:
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
        elif command.data_type.type == InstrumentStatusUpdate:
            self._handle_subscribe_instrument_status_updates(
                client,
                command.data_type.metadata.get("instrument_id"),
            )
        elif command.data_type.type == InstrumentClosePrice:
            self._handle_subscribe_instrument_close_prices(
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
        elif command.data_type.type == OrderBook:
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
        else:
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

        # Always re-subscribe to override previous settings
        client.subscribe_order_book_deltas(
            instrument_id=instrument_id,
            level=metadata["level"],
            kwargs=metadata.get("kwargs"),
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
            timer_name = f"OrderBookSnapshot-{instrument_id}-{interval_ms}"
            self._clock.set_timer(
                name=timer_name,
                interval=timedelta(milliseconds=interval_ms),
                start_time=start_time,
                stop_time=None,
                handler=self._snapshot_order_book,
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
                level=metadata["level"],
            )

            self._cache.add_order_book(order_book)
            self._log.debug(f"Created {type(order_book).__name__}.")

        # Always re-subscribe to override previous settings
        try:
            client.subscribe_order_book_deltas(
                instrument_id=instrument_id,
                level=metadata.get("level"),
                kwargs=metadata.get("kwargs"),
            )
        except NotImplementedError:
            client.subscribe_order_book_snapshots(
                instrument_id=instrument_id,
                level=metadata.get("level"),
                depth=metadata.get("depth"),
                kwargs=metadata.get("kwargs"),
            )

        self._msgbus.subscribe(
            topic=f"data.book.deltas"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self._maintain_order_book,
            priority=10,
        )

    cdef void _handle_subscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        client.subscribe_quote_ticks(instrument_id)

    cdef void _handle_subscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        client.subscribe_trade_ticks(instrument_id)

    cdef void _handle_subscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")

        if bar_type.is_internal_aggregation and bar_type not in self._bar_aggregators:
            # Internal aggregation
            self._start_bar_aggregator(client, bar_type)
        else:
            # External aggregation
            client.subscribe_bars(bar_type)

    cdef void _handle_subscribe_data(
        self,
        DataClient client,
        DataType data_type,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(data_type, "data_type")

        try:
            client.subscribe(data_type)
        except NotImplementedError:
            self._log.error(
                f"Cannot subscribe: {client.id.value} "
                f"has not implemented data type {data_type} subscriptions.",
            )
            return

    cdef void _handle_subscribe_instrument_status_updates(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        client.subscribe_instrument_status_updates(instrument_id)

    cdef void _handle_subscribe_instrument_close_prices(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        client.subscribe_instrument_status_updates(instrument_id)

    cdef void _handle_unsubscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")

        if instrument_id is None:
            client.unsubscribe_instruments()
        else:
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

        client.unsubscribe_order_book_snapshots(instrument_id)

    cdef void _handle_unsubscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        client.unsubscribe_quote_ticks(instrument_id)

    cdef void _handle_unsubscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")

        client.unsubscribe_trade_ticks(instrument_id)

    cdef void _handle_unsubscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")

        if bar_type.is_internal_aggregation and bar_type in self._bar_aggregators:
            # Internal aggregation
            self._stop_bar_aggregator(client, bar_type)
        else:
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
            client.unsubscribe(data_type)
        except NotImplementedError:
            self._log.error(
                f"Cannot unsubscribe: {client.id.value} "
                f"has not implemented data type {data_type} subscriptions.",
            )
            return

# -- REQUEST HANDLERS ------------------------------------------------------------------------------

    cdef void _handle_request(self, DataRequest request) except *:
        self._log.debug(f"{RECV}{REQ} {request}.")
        self.request_count += 1

        cdef DataClient client = self._clients.get(request.client_id)
        if client is None:
            self._log.error(f"Cannot handle request: "
                            f"no client registered for '{request.client_id}', {request}.")
            return  # No client to handle request

        if request.data_type.type == QuoteTick:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_quote_ticks(
                request.data_type.metadata.get("instrument_id"),
                request.data_type.metadata.get("from_datetime"),
                request.data_type.metadata.get("to_datetime"),
                request.data_type.metadata.get("limit", 0),
                request.id,
            )
        elif request.data_type.type == TradeTick:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_trade_ticks(
                request.data_type.metadata.get("instrument_id"),
                request.data_type.metadata.get("from_datetime"),
                request.data_type.metadata.get("to_datetime"),
                request.data_type.metadata.get("limit", 0),
                request.id,
            )
        elif request.data_type.type == Bar:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_bars(
                request.data_type.metadata.get("bar_type"),
                request.data_type.metadata.get("from_datetime"),
                request.data_type.metadata.get("to_datetime"),
                request.data_type.metadata.get("limit", 0),
                request.id,
            )
        else:
            try:
                client.request(request.data_type, request.id)
            except NotImplementedError:
                self._log.error(f"Cannot handle request: unrecognized data type {request.data_type}.")

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef void _handle_data(self, Data data) except *:
        self.data_count += 1

        if isinstance(data, QuoteTick):
            self._handle_quote_tick(data)
        elif isinstance(data, TradeTick):
            self._handle_trade_tick(data)
        elif isinstance(data, OrderBookData):
            self._handle_order_book_data(data)
        elif isinstance(data, Bar):
            self._handle_bar(data)
        elif isinstance(data, Instrument):
            self._handle_instrument(data)
        elif isinstance(data, StatusUpdate):
            self._handle_status_update(data)
        elif isinstance(data, InstrumentClosePrice):
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
        self._cache.add_bar(bar)

        self._msgbus.publish_c(topic=f"data.bars.{bar.type}", msg=bar)

    cdef void _handle_status_update(self, StatusUpdate data) except *:
        self._msgbus.publish_c(topic=f"data.venue.status", msg=data)

    cdef void _handle_close_price(self, InstrumentClosePrice data) except *:
        self._msgbus.publish_c(topic=f"data.venue.close_price.{data.instrument_id}", msg=data)

    cdef void _handle_generic_data(self, GenericData data) except *:
        self._msgbus.publish_c(topic=f"data.{data.data_type}", msg=data)

# -- RESPONSE HANDLERS -----------------------------------------------------------------------------

    cdef void _handle_response(self, DataResponse response) except *:
        self._log.debug(f"{RECV}{RES} {response}.")
        self.response_count += 1

        if response.data_type.type == Instrument:
            self._handle_instruments(response.data)
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
        if partial is not None:
            # Update partial time bar
            aggregator = self._bar_aggregators.get(partial.type)
            if aggregator:
                self._log.debug(f"Applying partial bar {partial} for {partial.type}.")
                aggregator.set_partial(partial)
            else:
                if self._fsm.state == ComponentState.RUNNING:
                    # Only log this error if the component is running, because
                    # there may have been an immediate stop called after start
                    # - with the partial bar being for a now removed aggregator.
                    self._log.error("No aggregator for partial bar update.")

# -- INTERNAL --------------------------------------------------------------------------------------

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
                f"Cannot maintain book: no book found for {data.instrument_id}."
            )
            return

        order_book.apply(data)

    cpdef void _snapshot_order_book(self, TimeEvent snap_event) except *:
        cdef tuple pieces = snap_event.name.partition('-')[2].partition('-')
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(pieces[0])
        cdef int interval_ms = int(pieces[2])

        cdef OrderBook order_book = self._cache.order_book(instrument_id)
        if order_book:
            self._msgbus.publish_c(
                topic=f"data.book.snapshots"
                      f".{instrument_id.venue}"
                      f".{instrument_id.symbol}"
                      f".{interval_ms}",
                msg=order_book,
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
                use_previous_close=self._use_previous_close,
                clock=self._clock,
                logger=self._log.get_logger(),
            )

            self._hydrate_aggregator(client, aggregator, bar_type)
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
            raise RuntimeError(
                f"Cannot start aggregator, "
                f"BarAggregation.{BarAggregationParser.to_str(bar_type.spec.aggregation)} "
                f"not currently supported in this version"
            )

        # Add aggregator
        self._bar_aggregators[bar_type] = aggregator
        self._log.debug(f"Added {aggregator} for {bar_type} bars.")

        # Subscribe to required data
        instrument_id = bar_type.instrument_id
        if bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.subscribe(
                topic=f"data.trades"
                      f".{instrument_id.venue}"
                      f".{instrument_id.symbol}",
                handler=aggregator.handle_trade_tick,
                priority=5,
            )
            self._handle_subscribe_trade_ticks(client, bar_type.instrument_id)
        else:
            self._msgbus.subscribe(
                topic=f"data.quotes"
                      f".{instrument_id.venue}"
                      f".{instrument_id.symbol}",
                handler=aggregator.handle_quote_tick,
                priority=5,
            )
            self._handle_subscribe_quote_ticks(client, bar_type.instrument_id)

    cdef void _hydrate_aggregator(
        self,
        MarketDataClient client,
        TimeBarAggregator aggregator,
        BarType bar_type,
    ) except *:
        data_type = type(TradeTick) if bar_type.spec.price_type == PriceType.LAST else QuoteTick

        if data_type == type(TradeTick) and "request_trade_ticks" in client.unavailable_methods():
            return
        elif data_type == type(QuoteTick) and "request_quote_ticks" in client.unavailable_methods():
            return

        # Update aggregator with latest data
        bulk_updater = BulkTimeBarUpdater(aggregator)

        metadata = {
            "instrument_id": bar_type.instrument_id,
            "from_datetime": aggregator.get_start_time(),
            "to_datetime": None,
        }

        request = DataRequest(
            client_id=ClientId(bar_type.instrument_id.venue.value),
            data_type=DataType(data_type, metadata),
            callback=bulk_updater.receive,
            request_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        # Send request directly to handler as we're already inside engine
        self._handle_request(request)

    cdef void _stop_bar_aggregator(self, MarketDataClient client, BarType bar_type) except *:
        cdef aggregator = self._bar_aggregators.get(bar_type)
        if aggregator is None:
            self._log.warning(f"No bar aggregator to stop for {bar_type}")
            return

        if isinstance(aggregator, TimeBarAggregator):
            aggregator.stop()

        # Unsubscribe from update ticks
        instrument_id = bar_type.instrument_id
        if bar_type.spec.price_type == PriceType.LAST:
            self._msgbus.unsubscribe(
                topic=f"data.trades"
                      f".{instrument_id.venue}"
                      f".{instrument_id.symbol}",
                handler=aggregator.handle_trade_tick,
            )
            self._handle_unsubscribe_trade_ticks(client, bar_type.instrument_id)
        else:
            self._msgbus.unsubscribe(
                topic=f"data.quotes"
                      f".{instrument_id.venue}"
                      f".{instrument_id.symbol}",
                handler=aggregator.handle_quote_tick,
            )
            self._handle_unsubscribe_quote_ticks(client, bar_type.instrument_id)

        # Remove from aggregators
        del self._bar_aggregators[bar_type]
