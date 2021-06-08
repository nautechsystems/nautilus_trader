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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport REQ
from nautilus_trader.common.logging cimport RES
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.aggregation cimport BarAggregator
from nautilus_trader.data.aggregation cimport BulkTickBarBuilder
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
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.book cimport OrderBookData
from nautilus_trader.model.orderbook.book cimport OrderBookDeltas
from nautilus_trader.model.orderbook.book cimport OrderBookSnapshot
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataEngine(Component):
    """
    Provides a high-performance data engine for managing many `DataClient`
    instances, for the asynchronous ingest of data.
    """

    def __init__(
        self,
        Portfolio portfolio not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``DataEngine`` class.

        Parameters
        ----------
        portfolio : int
            The portfolio to register.
        cache : Cache
            The cache for the engine.
        clock : Clock
            The clock for the engine.
        logger : Logger
            The logger for the component.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        super().__init__(clock, logger, name="DataEngine")

        if config:
            self._log.info(f"Config: {config}.")

        self._use_previous_close = config.get("use_previous_close", True)
        self._clients = {}                    # type: dict[ClientId, DataClient]
        self._correlation_index = {}          # type: dict[UUID, callable]

        # Handlers
        self._instrument_handlers = {}        # type: dict[InstrumentId, list[callable]]
        self._order_book_handlers = {}        # type: dict[InstrumentId, list[callable]]
        self._order_book_delta_handlers = {}  # type: dict[InstrumentId, list[callable]]
        self._quote_tick_handlers = {}        # type: dict[InstrumentId, list[callable]]
        self._trade_tick_handlers = {}        # type: dict[InstrumentId, list[callable]]
        self._bar_handlers = {}               # type: dict[BarType, list[callable]]
        self._data_handlers = {}              # type: dict[DataType, list[callable]]

        # Aggregators
        self._bar_aggregators = {}            # type: dict[BarType, BarAggregator]

        # OrderBook indexes
        self._order_book_intervals = {}       # type: dict[(InstrumentId, int), list[callable]]

        # Public components
        self.portfolio = portfolio
        self.cache = cache

        # Counters
        self.command_count = 0
        self.data_count = 0
        self.request_count = 0
        self.response_count = 0

        self._log.info(f"use_previous_close={self._use_previous_close}")

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
    def subscribed_data_types(self):
        """
        The custom data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        return sorted(list(self._data_handlers.keys()))

    @property
    def subscribed_instruments(self):
        """
        The instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._instrument_handlers.keys()))

    @property
    def subscribed_order_books(self):
        """
        The order books subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        cdef list interval_instruments = [k[0] for k in self._order_book_intervals.keys()]
        return sorted(list(self._order_book_handlers.keys()) + interval_instruments)

    @property
    def subscribed_order_book_data(self):
        """
        The order books data (diffs) subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._order_book_delta_handlers.keys()))

    @property
    def subscribed_quote_ticks(self):
        """
        The quote tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._quote_tick_handlers.keys()))

    @property
    def subscribed_trade_ticks(self):
        """
        The trade tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._trade_tick_handlers.keys()))

    @property
    def subscribed_bars(self):
        """
        The bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        return sorted(list(self._bar_handlers.keys()))

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

    cpdef void register_strategy(self, TradingStrategy strategy) except *:
        """
        Register the given trading strategy with the data engine.

        Parameters
        ----------
        strategy : TradingStrategy
            The strategy to register.

        """
        Condition.not_none(strategy, "strategy")

        strategy.register_data_engine(self)

        self._log.info(f"Registered {strategy}.")

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

        self.cache.reset()

        self._correlation_index.clear()
        self._instrument_handlers.clear()
        self._order_book_handlers.clear()
        self._quote_tick_handlers.clear()
        self._trade_tick_handlers.clear()
        self._bar_handlers.clear()
        self._data_handlers.clear()
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

    cpdef void send(self, DataRequest request) except *:
        """
        Handle the given request.

        Parameters
        ----------
        request : DataRequest
            The request to handle.

        """
        Condition.not_none(request, "request")

        self._handle_request(request)

    cpdef void receive(self, DataResponse response) except *:
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
            self._log.error(f"Cannot handle command: "
                            f"(no client registered for '{command.client_id}' in {self.registered_clients})"
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
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.handler,
            )
        elif command.data_type.type == OrderBook:
            self._handle_subscribe_order_book(
                client,
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.data_type.metadata,
                command.handler,
            )
        elif command.data_type.type == OrderBookData:
            self._handle_subscribe_order_book_deltas(
                client,
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.data_type.metadata,
                command.handler,
            )
        elif command.data_type.type == QuoteTick:
            self._handle_subscribe_quote_ticks(
                client,
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.handler,
            )
        elif command.data_type.type == TradeTick:
            self._handle_subscribe_trade_ticks(
                client,
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.handler,
            )
        elif command.data_type.type == Bar:
            self._handle_subscribe_bars(
                client,
                command.data_type.metadata.get(BAR_TYPE),
                command.handler,
            )
        else:
            self._handle_subscribe_data(
                client,
                command.data_type,
                command.handler,
            )

    cdef void _handle_unsubscribe(self, DataClient client, Unsubscribe command) except *:
        if command.data_type.type == Instrument:
            self._handle_unsubscribe_instrument(
                client,
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.handler,
            )
        elif command.data_type.type == OrderBook:
            self._handle_unsubscribe_order_book(
                client,
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.data_type.metadata,
                command.handler,
            )
        elif command.data_type.type == QuoteTick:
            self._handle_unsubscribe_quote_ticks(
                client,
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.handler,
            )
        elif command.data_type.type == TradeTick:
            self._handle_unsubscribe_trade_ticks(
                client,
                command.data_type.metadata.get(INSTRUMENT_ID),
                command.handler,
            )
        elif command.data_type.type == Bar:
            self._handle_unsubscribe_bars(
                client,
                command.data_type.metadata.get(BAR_TYPE),
                command.handler,
            )
        else:
            self._handle_unsubscribe_data(
                client,
                command.data_type,
                command.handler,
            )

    cdef void _handle_subscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable(handler, "handler")

        if instrument_id not in self._instrument_handlers:
            self._instrument_handlers[instrument_id] = []  # type: list[callable]
            client.subscribe_instrument(instrument_id)
            self._log.info(f"Subscribed to {instrument_id} <Instrument> data.")

        # Add handler for subscriber
        if handler not in self._instrument_handlers[instrument_id]:
            self._instrument_handlers[instrument_id].append(handler)
            self._log.debug(f"Added handler {handler} for {instrument_id} <Instrument> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {instrument_id} <Instrument> data.")

    cdef void _handle_subscribe_order_book(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")
        Condition.callable(handler, "handler")

        cdef int interval = metadata[INTERVAL]
        if interval > 0:
            # Subscribe to interval snapshots
            key = (instrument_id, interval)
            if key not in self._order_book_intervals:
                self._order_book_intervals[key] = []
                now = self._clock.utc_now()
                start_time = now - timedelta(seconds=now.second % interval, microseconds=now.microsecond)
                timer_name = f"OrderBookSnapshot-{instrument_id}-{interval}"
                self._clock.set_timer(
                    name=timer_name,
                    interval=timedelta(seconds=interval),
                    start_time=start_time,
                    stop_time=None,
                    handler=self._snapshot_order_book,
                )
                self._log.debug(f"Set timer {timer_name}.")

            # Add handler for subscriber
            self._order_book_intervals[key].append(handler)
            self._log.info(f"Subscribed to {instrument_id} <OrderBook> "
                           f"{interval} second intervals data.")
        else:
            # Subscribe to stream
            if instrument_id not in self._order_book_handlers:
                # Setup handlers
                self._order_book_handlers[instrument_id] = []  # type: list[callable]
                self._log.info(f"Subscribed to {instrument_id} <OrderBook> data.")

            # Add handler for subscriber
            if handler not in self._order_book_handlers[instrument_id]:
                self._order_book_handlers[instrument_id].append(handler)
                self._log.debug(f"Added {handler} for {instrument_id} <OrderBook> data.")
            else:
                self._log.warning(f"Handler {handler} already subscribed to {instrument_id} <OrderBook> data.")

        # Create order book
        if not self.cache.has_order_book(instrument_id):
            instrument = self.cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(f"Cannot subscribe to {instrument_id} <OrderBook> data: "
                                f"no instrument found in cache.")
                return
            order_book = OrderBook.create(
                instrument=instrument,
                level=metadata[LEVEL],
            )

            self.cache.add_order_book(order_book)

        # Always re-subscribe to override previous settings
        client.subscribe_order_book(
            instrument_id=instrument_id,
            level=metadata.get(LEVEL),
            depth=metadata.get(DEPTH),
            kwargs=metadata.get(KWARGS),
        )

    cdef void _handle_subscribe_order_book_deltas(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")
        Condition.callable(handler, "handler")

        # Subscribe to stream
        if instrument_id not in self._order_book_delta_handlers:
            # Setup handlers
            self._order_book_delta_handlers[instrument_id] = []  # type: list[callable]
            self._log.info(f"Subscribed to {instrument_id} <OrderBookDeltas> data.")

        # Add handler for subscriber
        if handler not in self._order_book_delta_handlers[instrument_id]:
            self._order_book_delta_handlers[instrument_id].append(handler)
            self._log.debug(f"Added {handler} for {instrument_id} <OrderBookDeltas> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to "
                              f"{instrument_id} <OrderBookDeltas> data.")

        # Create order book
        if not self.cache.has_order_book(instrument_id):
            instrument = self.cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(f"Cannot subscribe to {instrument_id} <OrderBookDeltas> data: "
                                f"no instrument found in cache.")
                return
            order_book = OrderBook.create(
                instrument=instrument,
                level=metadata[LEVEL],
            )

            self.cache.add_order_book(order_book)

        # Always re-subscribe to override previous settings
        client.subscribe_order_book_deltas(
            instrument_id=instrument_id,
            level=metadata[LEVEL],
            kwargs=metadata.get(KWARGS),
        )

    cdef void _handle_subscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable(handler, "handler")

        if instrument_id not in self._quote_tick_handlers:
            # Setup handlers
            self._quote_tick_handlers[instrument_id] = []  # type: list[callable]
            client.subscribe_quote_ticks(instrument_id)
            self._log.info(f"Subscribed to {instrument_id} <QuoteTick> data.")

        # Add handler for subscriber
        if handler not in self._quote_tick_handlers[instrument_id]:
            self._quote_tick_handlers[instrument_id].append(handler)
            self._log.debug(f"Added {handler} for {instrument_id} <QuoteTick> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {instrument_id} <QuoteTick> data.")

    cdef void _handle_subscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable(handler, "handler")

        if instrument_id not in self._trade_tick_handlers:
            # Setup handlers
            self._trade_tick_handlers[instrument_id] = []  # type: list[callable]
            client.subscribe_trade_ticks(instrument_id)
            self._log.info(f"Subscribed to {instrument_id} <TradeTick> data.")

        # Add handler for subscriber
        if handler not in self._trade_tick_handlers[instrument_id]:
            self._trade_tick_handlers[instrument_id].append(handler)
            self._log.debug(f"Added {handler} for {instrument_id} <TradeTick> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {instrument_id} <TradeTick> data.")

    cdef void _handle_subscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")
        Condition.callable(handler, "handler")

        if bar_type not in self._bar_handlers:
            # Setup handlers
            self._bar_handlers[bar_type] = []  # type: list[callable]
            if bar_type.is_internal_aggregation:
                if bar_type not in self._bar_aggregators:
                    # Aggregation not started
                    self._start_bar_aggregator(client, bar_type)
            else:
                # External aggregation
                client.subscribe_bars(bar_type)
            self._log.info(f"Subscribed to {bar_type} <Bar> data.")

        # Add handler for subscriber
        if handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(handler)
            self._log.debug(f"Added {handler} for {bar_type} <Bar> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {bar_type} <Bar> data.")

    cdef void _handle_subscribe_data(
        self,
        DataClient client,
        DataType data_type,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(data_type, "data_type")
        Condition.callable(handler, "handler")

        if data_type not in self._data_handlers:
            # Setup handlers
            try:
                client.subscribe(data_type)
            except NotImplementedError:
                self._log.error(f"Cannot subscribe: {client.id.value} "
                                f"has not implemented data type {data_type} subscriptions.")
                return
            self._data_handlers[data_type] = []  # type: list[callable]
            self._log.info(f"Subscribed to {data_type} data.")

        # Add handler for subscriber
        if handler not in self._data_handlers[data_type]:
            self._data_handlers[data_type].append(handler)
            self._log.debug(f"Added {handler} for {data_type} data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {data_type} data.")

    cdef void _handle_unsubscribe_instrument(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable(handler, "handler")

        if instrument_id not in self._instrument_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {instrument_id} <Instrument> data.")
            return

        # Remove subscribers handler
        if handler in self._instrument_handlers[instrument_id]:
            self._instrument_handlers[instrument_id].remove(handler)
            self._log.debug(f"Removed handler {handler} for {instrument_id} <Instrument> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {instrument_id} <Instrument> data.")

        if not self._instrument_handlers[instrument_id]:
            # No more handlers for instrument_id
            del self._instrument_handlers[instrument_id]
            client.unsubscribe_instrument(instrument_id)
            self._log.info(f"Unsubscribed from {instrument_id} <Instrument> data.")

    cdef void _handle_unsubscribe_order_book(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        dict metadata,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(metadata, "metadata")
        Condition.callable(handler, "handler")

        cdef int interval = metadata.get(INTERVAL)
        if interval > 0:
            # Remove interval subscribers handler
            key = (instrument_id, interval)
            handlers = self._order_book_intervals.get(key)
            if not handlers:
                self._log.warning(f"No order book snapshot handlers for {instrument_id}"
                                  f"at {interval} second intervals.")
                return

            if handler not in handlers:
                self._log.warning(f"Handler {handler} not subscribed to {instrument_id} "
                                  f"<OrderBook> data at {interval} second intervals.")
                return

            handlers.remove(handler)
            if not handlers:
                timer_name = f"OrderBookSnapshot-{instrument_id}-{interval}"
                self._clock.cancel_timer(timer_name)
                self._log.debug(f"Cancelled timer {timer_name}.")
                del self._order_book_intervals[key]
                self._log.info(f"Unsubscribed from {instrument_id} <OrderBook> "
                               f"{interval} second intervals data.")
            return

        if instrument_id not in self._order_book_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {instrument_id} <OrderBook> data.")
            return

        # Remove subscribers handler
        if handler in self._order_book_handlers[instrument_id]:
            self._order_book_handlers[instrument_id].remove(handler)
            self._log.debug(f"Removed handler {handler} for {instrument_id} <OrderBook> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {instrument_id} <OrderBook> data.")

        if not self._order_book_handlers[instrument_id]:
            # No more handlers for instrument_id
            del self._order_book_handlers[instrument_id]
            client.unsubscribe_order_book(instrument_id)
            self._log.info(f"Unsubscribed from {instrument_id} <OrderBook> data.")

    cdef void _handle_unsubscribe_quote_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable(handler, "handler")

        if instrument_id not in self._quote_tick_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {instrument_id} <QuoteTick> data.")
            return

        # Remove subscribers handler
        if handler in self._quote_tick_handlers[instrument_id]:
            self._quote_tick_handlers[instrument_id].remove(handler)
            self._log.debug(f"Removed handler {handler} for {instrument_id} <QuoteTick> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {instrument_id} <QuoteTick> data.")

        if not self._quote_tick_handlers[instrument_id]:
            # No more handlers for instrument_id
            del self._quote_tick_handlers[instrument_id]
            client.unsubscribe_quote_ticks(instrument_id)
            self._log.info(f"Unsubscribed from {instrument_id} <QuoteTick> data.")

    cdef void _handle_unsubscribe_trade_ticks(
        self,
        MarketDataClient client,
        InstrumentId instrument_id,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable(handler, "handler")

        if instrument_id not in self._trade_tick_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {instrument_id} <TradeTick> data.")
            return

        # Remove subscribers handler
        if handler in self._trade_tick_handlers[instrument_id]:
            self._trade_tick_handlers[instrument_id].remove(handler)
            self._log.debug(f"Removed handler {handler} for {instrument_id} <TradeTick> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {instrument_id} <TradeTick> data.")

        if not self._trade_tick_handlers[instrument_id]:
            # No more handlers for instrument_id
            del self._trade_tick_handlers[instrument_id]
            client.unsubscribe_trade_ticks(instrument_id)
            self._log.info(f"Unsubscribed from {instrument_id} <TradeTick> data.")

    cdef void _handle_unsubscribe_bars(
        self,
        MarketDataClient client,
        BarType bar_type,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(bar_type, "bar_type")
        Condition.callable(handler, "handler")

        if bar_type not in self._bar_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {bar_type} <Bar> data.")
            return

        # Remove subscribers handler
        if handler in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].remove(handler)
            self._log.debug(f"Removed handler {handler} for {bar_type} <Bar> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {bar_type} <Bar> data.")

        if not self._bar_handlers[bar_type]:
            # No more handlers for bar type
            del self._bar_handlers[bar_type]
            if bar_type.is_internal_aggregation:
                self._stop_bar_aggregator(client, bar_type)
            else:
                client.unsubscribe_bars(bar_type)
            self._log.info(f"Unsubscribed from {bar_type} <Bar> data.")

    cdef void _handle_unsubscribe_data(
        self,
        DataClient client,
        DataType data_type,
        handler: callable,
    ) except *:
        Condition.not_none(client, "client")
        Condition.not_none(data_type, "data_type")
        Condition.callable(handler, "handler")

        if data_type not in self._data_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {data_type} data.")
            return

        # Remove subscribers handler
        if handler in self._data_handlers[data_type]:
            self._data_handlers[data_type].remove(handler)
            self._log.debug(f"Removed handler {handler} for {data_type} data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {data_type} data.")

        if not self._data_handlers[data_type]:
            # No more handlers for data type
            del self._data_handlers[data_type]
            client.unsubscribe(data_type)
            self._log.info(f"Unsubscribed from {data_type} data.")

# -- REQUEST HANDLERS ------------------------------------------------------------------------------

    cdef void _handle_request(self, DataRequest request) except *:
        self._log.debug(f"{RECV}{REQ} {request}.")
        self.request_count += 1

        cdef DataClient client = self._clients.get(request.client_id)
        if client is None:
            self._log.error(f"Cannot handle request: "
                            f"no client registered for '{request.client_id}', {request}.")
            return  # No client to handle request

        if request.id in self._correlation_index:
            self._log.error(f"Cannot handle request: "
                            f"duplicate identifier {request.id} found in correlation index.")
            return  # Do not handle duplicates

        self._correlation_index[request.id] = request.callback

        if request.data_type.type == Instrument:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            instrument_id = request.data_type.metadata.get(INSTRUMENT_ID)
            if instrument_id:
                client.request_instrument(instrument_id, request.id)
            else:
                client.request_instruments(request.id)
        elif request.data_type.type == QuoteTick:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_quote_ticks(
                request.data_type.metadata.get(INSTRUMENT_ID),
                request.data_type.metadata.get(FROM_DATETIME),
                request.data_type.metadata.get(TO_DATETIME),
                request.data_type.metadata.get(LIMIT, 0),
                request.id,
            )
        elif request.data_type.type == TradeTick:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_trade_ticks(
                request.data_type.metadata.get(INSTRUMENT_ID),
                request.data_type.metadata.get(FROM_DATETIME),
                request.data_type.metadata.get(TO_DATETIME),
                request.data_type.metadata.get(LIMIT, 0),
                request.id,
            )
        elif request.data_type.type == Bar:
            Condition.true(isinstance(client, MarketDataClient), "client was not a MarketDataClient")
            client.request_bars(
                request.data_type.metadata.get(BAR_TYPE),
                request.data_type.metadata.get(FROM_DATETIME),
                request.data_type.metadata.get(TO_DATETIME),
                request.data_type.metadata.get(LIMIT, 0),
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
        elif isinstance(data, OrderBookDeltas):
            self._handle_order_book_deltas(data)
        elif isinstance(data, OrderBookSnapshot):
            self._handle_order_book_snapshot(data)
        elif isinstance(data, Bar):
            self._handle_bar(data)
        elif isinstance(data, Instrument):
            self._handle_instrument(data)
        elif isinstance(data, GenericData):
            self._handle_generic_data(data)
        else:
            self._log.error(f"Cannot handle data: unrecognized type {type(data)} {data}.")

    cdef void _handle_instrument(self, Instrument instrument) except *:
        self.cache.add_instrument(instrument)

        cdef list instrument_handlers = self._instrument_handlers.get(instrument.id, [])
        for handler in instrument_handlers:
            handler(instrument)

    cdef void _handle_quote_tick(self, QuoteTick tick) except *:
        self.cache.add_quote_tick(tick)

        # Send to portfolio as a priority
        self.portfolio.update_tick(tick)

        # Send to all registered tick handlers for that instrument_id
        cdef list tick_handlers = self._quote_tick_handlers.get(tick.instrument_id, [])
        for handler in tick_handlers:
            handler(tick)

    cdef void _handle_trade_tick(self, TradeTick tick) except *:
        self.cache.add_trade_tick(tick)

        # Send to all registered tick handlers for that instrument_id
        cdef list tick_handlers = self._trade_tick_handlers.get(tick.instrument_id, [])
        for handler in tick_handlers:
            handler(tick)

    cdef void _handle_order_book_deltas(self, OrderBookDeltas deltas) except *:
        cdef InstrumentId instrument_id = deltas.instrument_id
        cdef OrderBook order_book = self.cache.order_book(instrument_id)
        if order_book is None:
            self._log.error(f"Cannot apply `OrderBookDeltas`: "
                            f"no book found for {deltas.instrument_id}.")
            return

        order_book.apply_deltas(deltas)

        # Send to all registered order book handlers for that instrument_id
        cdef list order_book_handlers = self._order_book_handlers.get(instrument_id, [])
        for orderbook_handler in order_book_handlers:
            orderbook_handler(order_book)

        # Send to all registered order book delta handlers for that instrument_id
        cdef list order_book_delta_handlers = self._order_book_delta_handlers.get(instrument_id, [])
        for orderbook_delta_handler in order_book_delta_handlers:
            orderbook_delta_handler(deltas)

    cdef void _handle_order_book_snapshot(self, OrderBookSnapshot snapshot) except *:
        cdef InstrumentId instrument_id = snapshot.instrument_id
        cdef OrderBook order_book = self.cache.order_book(instrument_id)
        if order_book is None:
            self._log.error(f"Cannot apply `OrderBookSnapshot`: "
                            f"no book found for {snapshot.instrument_id}.")
            return

        order_book.apply_snapshot(snapshot)

        # Send to all registered order book handlers for that instrument_id
        cdef list order_book_handlers = self._order_book_handlers.get(instrument_id, [])
        for handler in order_book_handlers:
            handler(order_book)

        # Send to all registered order book delta handlers for that instrument_id
        cdef list order_book_delta_handlers = self._order_book_delta_handlers.get(instrument_id, [])
        for orderbook_delta_handler in order_book_delta_handlers:
            orderbook_delta_handler(snapshot)

    cdef void _handle_bar(self, Bar bar) except *:
        self.cache.add_bar(bar)

        # Send to all registered bar handlers for that bar type
        cdef list bar_handlers = self._bar_handlers.get(bar.type, [])
        for handler in bar_handlers:
            handler(bar)

    cdef void _handle_generic_data(self, GenericData data) except *:
        # Send to all registered data handlers for that data type
        cdef list handlers = self._data_handlers.get(data.data_type, [])
        for handler in handlers:
            handler(data)

# -- RESPONSE HANDLERS -----------------------------------------------------------------------------

    cdef void _handle_response(self, DataResponse response) except *:
        self._log.debug(f"{RECV}{RES} {response}.")
        self.response_count += 1

        if response.data_type.type == Instrument:
            self._handle_instruments(response.data, response.correlation_id)
        elif response.data_type.type == QuoteTick:
            self._handle_quote_ticks(response.data, response.correlation_id)
        elif response.data_type.type == TradeTick:
            self._handle_trade_ticks(response.data, response.correlation_id)
        elif response.data_type.type == Bar:
            self._handle_bars(
                response.data,
                response.data_type.metadata.get("Partial"),
                response.correlation_id,
            )
        else:
            callback = self._correlation_index.pop(response.correlation_id, None)
            if callback is None:
                self._log.error(f"Callback not found for correlation_id {response.correlation_id}.")
                return

            callback(response.data)

    cdef void _handle_instruments(self, list instruments, UUID correlation_id) except *:
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

        cdef callback = self._correlation_index.pop(correlation_id, None)
        if callback is None:
            self._log.error(f"Callback not found for correlation_id {correlation_id}.")
            return

        if callback == self._internal_update_instruments:
            return  # Already updated

        callback(instruments)

    cdef void _handle_quote_ticks(self, list ticks, UUID correlation_id) except *:
        self.cache.add_quote_ticks(ticks)

        cdef callback = self._correlation_index.pop(correlation_id, None)
        if callback is None:
            self._log.error(f"Callback not found for correlation_id {correlation_id}.")
            return

        callback(ticks)

    cdef void _handle_trade_ticks(self, list ticks, UUID correlation_id) except *:
        self.cache.add_trade_ticks(ticks)

        cdef callback = self._correlation_index.pop(correlation_id, None)
        if callback is None:
            self._log.error(f"Callback not found for correlation_id {correlation_id}.")
            return

        callback(ticks)

    cdef void _handle_bars(self, list bars, Bar partial, UUID correlation_id) except *:
        self.cache.add_bars(bars)

        cdef callback = self._correlation_index.pop(correlation_id, None)
        if callback is None:
            self._log.error(f"Callback not found for correlation_id {correlation_id}.")
            return

        cdef TimeBarAggregator aggregator
        if partial is not None:
            # Update partial time bar
            aggregator = self._bar_aggregators.get(partial.type)
            if aggregator:
                self._log.debug(f"Applying partial bar {partial} for {partial.type}.")
                aggregator.set_partial(partial)
            else:
                if self._fsm.state == ComponentState.RUNNING:
                    # Only log this error if the component is running as here
                    # there may have been an immediate stop after start with the
                    # partial bar being for a now removed aggregator.
                    self._log.error("No aggregator for partial bar update.")

        callback(bars)

# -- INTERNAL --------------------------------------------------------------------------------------

    # Python wrapper to enable callbacks
    cpdef void _internal_update_instruments(self, list instruments: [Instrument]) except *:
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cpdef void _snapshot_order_book(self, TimeEvent snap_event) except *:
        cdef tuple pieces = snap_event.name.partition('-')[2].partition('-')
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(pieces[0])
        cdef int interval = int(pieces[2])
        cdef list handlers = self._order_book_intervals.get((instrument_id, interval))
        if handlers is None:
            self._log.error("No handlers")
            return

        cdef OrderBook order_book = self.cache.order_book(instrument_id)
        if order_book:
            for handler in handlers:
                handler(order_book)

    cdef void _start_bar_aggregator(self, MarketDataClient client, BarType bar_type) except *:
        if bar_type.spec.is_time_aggregated():
            # Create aggregator
            aggregator = TimeBarAggregator(
                bar_type=bar_type,
                handler=self.process,
                use_previous_close=self._use_previous_close,
                clock=self._clock,
                logger=self._log.get_logger(),
            )

            self._hydrate_aggregator(client, aggregator, bar_type)
        elif bar_type.spec.aggregation == BarAggregation.TICK:
            aggregator = TickBarAggregator(
                bar_type=bar_type,
                handler=self.process,
                logger=self._log.get_logger(),
            )
        elif bar_type.spec.aggregation == BarAggregation.VOLUME:
            aggregator = VolumeBarAggregator(
                bar_type=bar_type,
                handler=self.process,
                logger=self._log.get_logger(),
            )
        elif bar_type.spec.aggregation == BarAggregation.VALUE:
            aggregator = ValueBarAggregator(
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
        if bar_type.spec.price_type == PriceType.LAST:
            self._handle_subscribe_trade_ticks(client, bar_type.instrument_id, aggregator.handle_trade_tick)
        else:
            self._handle_subscribe_quote_ticks(client, bar_type.instrument_id, aggregator.handle_quote_tick)

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
            INSTRUMENT_ID: bar_type.instrument_id,
            FROM_DATETIME: aggregator.get_start_time(),
            TO_DATETIME: None,
        }

        # noinspection bulk_updater.receive
        # noinspection PyUnresolvedReferences
        request = DataRequest(
            client_id=ClientId(bar_type.instrument_id.venue.value),
            data_type=DataType(data_type, metadata),
            callback=bulk_updater.receive,
            request_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
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
        if bar_type.spec.price_type == PriceType.LAST:
            self._handle_unsubscribe_trade_ticks(client, bar_type.instrument_id, aggregator.handle_trade_tick)
        else:
            self._handle_unsubscribe_quote_ticks(client, bar_type.instrument_id, aggregator.handle_quote_tick)

        # Remove from aggregators
        del self._bar_aggregators[bar_type]

    cdef void _bulk_build_tick_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback: callable,
    ) except *:
        # Bulk build tick bars
        cdef int ticks_to_order = bar_type.spec.step * limit

        cdef BulkTickBarBuilder bar_builder = BulkTickBarBuilder(
            bar_type,
            self._log.get_logger(),
            callback,
        )

        # noinspection bar_builder.receive
        # noinspection PyUnresolvedReferences
        self._handle_request_quote_ticks(
            bar_type.instrument_id,
            from_datetime,
            to_datetime,
            ticks_to_order,
            bar_builder.receive,
        )
