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

Its primary responsibility is to orchestrate interactions between the individual
`DataClient` instances, and the rest of the platform. This includes consumers
subscribing to specific data types, as well as hydrating a `DataCache` layer
which presents a read-only facade for consumers.

The engine employs a simple fan-in fan-out messaging pattern to receive data
from the `DataClient` instances, and sending those to the registered
handlers.

Alternative implementations can be written on top which just need to override
the engines `execute`, `process`, `send` and `receive` methods.
"""

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.messages cimport Connect
from nautilus_trader.common.messages cimport Disconnect
from nautilus_trader.common.messages cimport DataRequest
from nautilus_trader.common.messages cimport DataResponse
from nautilus_trader.common.messages cimport Subscribe
from nautilus_trader.common.messages cimport Unsubscribe
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport RES
from nautilus_trader.common.logging cimport REQ
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.aggregation cimport BarAggregator
from nautilus_trader.data.aggregation cimport TickBarAggregator
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.aggregation cimport ValueBarAggregator
from nautilus_trader.data.aggregation cimport VolumeBarAggregator
from nautilus_trader.data.aggregation cimport BulkTickBarBuilder
from nautilus_trader.data.aggregation cimport BulkTimeBarUpdater
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarData
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.strategy cimport TradingStrategy
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class DataEngine(Component):
    """
    Provides a high-performance data engine for managing many `DataClient`
    instances, for the asynchronous ingest of data.
    """

    def __init__(
        self,
        Portfolio portfolio not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `DataEngine` class.

        Parameters
        ----------
        portfolio : int
            The portfolio to register.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        super().__init__(clock, logger, name="DataEngine")

        self._use_previous_close = config.get("use_previous_close", True)
        self._clients = {}              # type: dict[Venue, DataClient]
        self._correlation_index = {}    # type: dict[UUID, callable]

        # Handlers
        self._instrument_handlers = {}  # type: dict[Symbol, list[callable]]
        self._quote_tick_handlers = {}  # type: dict[Symbol, list[callable]]
        self._trade_tick_handlers = {}  # type: dict[Symbol, list[callable]]
        self._bar_handlers = {}         # type: dict[BarType, list[callable]]

        # Aggregators
        self._bar_aggregators = {}      # type: dict[BarType, BarAggregator]

        # Public components
        self.portfolio = portfolio
        self.cache = DataCache(logger)

        # Counters
        self.command_count = 0
        self.data_count = 0
        self.request_count = 0
        self.response_count = 0

        self._log.info(f"use_previous_close={self._use_previous_close}")

    @property
    def registered_venues(self):
        """
        The venues registered with the data engine.

        Returns
        -------
        list[Venue]

        """
        return sorted(list(self._clients.keys()))

    @property
    def subscribed_instruments(self):
        """
        The instruments subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return sorted(list(self._instrument_handlers.keys()))

    @property
    def subscribed_quote_ticks(self):
        """
        The quote tick symbols subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return sorted(list(self._quote_tick_handlers.keys()))

    @property
    def subscribed_trade_ticks(self):
        """
        The trade tick symbols subscribed to.

        Returns
        -------
        list[Symbol]

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

    cpdef bint check_initialized(self) except *:
        """
        Check the engine is initialized.

        Returns
        -------
        bool
            True if all data clients initialized, else False.

        """
        cdef DataClient client
        for client in self._clients.values():
            if not client.initialized:
                return False
        return True

    cpdef bint check_disconnected(self) except *:
        """
        Check all clients are disconnected.

        Returns
        -------
        bool
            True if all data clients disconnected, else False.

        """
        cdef DataClient client
        for client in self._clients.values():
            if client.is_connected():
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
        Condition.not_in(client.venue, self._clients, "client", "self._clients")

        self._clients[client.venue] = client

        self._log.info(f"Registered {client}.")

    cpdef void register_strategy(self, TradingStrategy strategy) except *:
        """
        Register the given trade strategy with the data client.

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
        Condition.is_in(client.venue, self._clients, "client.venue", "self._clients")

        del self._clients[client.venue]
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

        self.update_instruments_all()
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
        self._quote_tick_handlers.clear()
        self._trade_tick_handlers.clear()
        self._bar_handlers.clear()
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

    cpdef void execute(self, VenueCommand command) except *:
        """
        Execute the given command for a specified venue.

        Parameters
        ----------
        command : VenueCommand
            The venue to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, data) except *:
        """
        Process the given data.

        Parameters
        ----------
        data : object
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

    cpdef void update_instruments(self, Venue venue) except *:
        """
        Update all instruments for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue for the update.

        """
        Condition.not_none(venue, "venue")
        Condition.is_in(venue, self._clients, "venue", "self._clients")

        cdef DataRequest request = DataRequest(
            venue=venue,
            data_type=Instrument,
            metadata={},
            callback=self._internal_update_instruments,
            request_id=self._uuid_factory.generate(),
            request_timestamp=self._clock.utc_now(),
        )

        # Send to external API entry as this method could be called at any time
        self.send(request)

    cpdef void update_instruments_all(self) except *:
        """
        Update all instruments for every venue.
        """
        cdef Venue venue
        for venue in self.registered_venues:
            self.update_instruments(venue)

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, VenueCommand command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        cdef DataClient client = self._clients.get(command.venue)
        if client is None:
            self._log.error(f"Cannot handle command "
                            f"(no client registered for {command.venue}) {command}.")
            return  # No client to handle command

        if isinstance(command, Connect):
            client.connect()
        elif isinstance(command, Disconnect):
            client.disconnect()
        elif isinstance(command, Subscribe):
            self._handle_subscribe(client, command)
        elif isinstance(command, Unsubscribe):
            self._handle_unsubscribe(client, command)
        else:
            self._log.error(f"Cannot handle unrecognized command {command}.")

    cdef inline void _handle_subscribe(self, DataClient client, Subscribe command) except *:
        if command.data_type == Instrument:
            self._handle_subscribe_instrument(
                client,
                command.metadata.get(SYMBOL),
                command.handler,
            )
        elif command.data_type == QuoteTick:
            self._handle_subscribe_quote_ticks(
                client,
                command.metadata.get(SYMBOL),
                command.handler,
            )
        elif command.data_type == TradeTick:
            self._handle_subscribe_trade_ticks(
                client,
                command.metadata.get(SYMBOL),
                command.handler,
            )
        elif command.data_type == Bar:
            self._handle_subscribe_bars(
                client,
                command.metadata.get(BAR_TYPE),
                command.handler,
            )
        else:
            self._log.error(f"Cannot subscribe to unrecognized data type {command.data_type}.")

    cdef inline void _handle_unsubscribe(self, DataClient client, Unsubscribe command) except *:
        if command.data_type == Instrument:
            self._handle_unsubscribe_instrument(
                client,
                command.metadata.get(SYMBOL),
                command.handler,
            )
        elif command.data_type == QuoteTick:
            self._handle_unsubscribe_quote_ticks(
                client,
                command.metadata.get(SYMBOL),
                command.handler,
            )
        elif command.data_type == TradeTick:
            self._handle_unsubscribe_trade_ticks(
                client,
                command.metadata.get(SYMBOL),
                command.handler,
            )
        elif command.data_type == Bar:
            self._handle_unsubscribe_bars(
                client,
                command.metadata.get(BAR_TYPE),
                command.handler,
            )
        else:
            self._log.error(f"Cannot unsubscribe from unrecognized data type {command.data_type}.")

    cdef inline void _handle_subscribe_instrument(
        self,
        DataClient client,
        Symbol symbol,
        handler: callable,
    ) except *:
        # client already checked
        # validate message data
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")

        self._add_instrument_handler(symbol, handler)
        client.subscribe_instrument(symbol)

    cdef inline void _handle_subscribe_quote_ticks(
        self,
        DataClient client,
        Symbol symbol,
        handler: callable,
    ) except *:
        # client already checked
        # validate message data
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")

        self._add_quote_tick_handler(symbol, handler)
        client.subscribe_quote_ticks(symbol)

    cdef inline void _handle_subscribe_trade_ticks(
        self,
        DataClient client,
        Symbol symbol,
        handler: callable,
    ) except *:
        # client already checked
        # validate message data
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")

        self._add_trade_tick_handler(symbol, handler)
        client.subscribe_trade_ticks(symbol)

    cdef inline void _handle_subscribe_bars(
        self,
        DataClient client,
        BarType bar_type,
        handler: callable,
    ) except *:
        # client already checked
        # validate message data
        Condition.not_none(bar_type, "bar_type")
        Condition.callable(handler, "handler")

        self._add_bar_handler(bar_type, handler)

        if bar_type.is_internal_aggregation:
            if bar_type not in self._bar_aggregators:
                # Aggregation not started
                self._start_bar_aggregator(client, bar_type)
        else:
            # External aggregation
            client.subscribe_bars(bar_type)

    cdef inline void _handle_unsubscribe_instrument(
        self,
        DataClient client,
        Symbol symbol,
        handler: callable,
    ) except *:
        # client already checked
        # validate message data
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")

        client.unsubscribe_trade_ticks(symbol)
        self._remove_instrument_handler(symbol, handler)

    cdef inline void _handle_unsubscribe_quote_ticks(
        self,
        DataClient client,
        Symbol symbol,
        handler: callable,
    ) except *:
        # client already checked
        # validate message data
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")

        client.unsubscribe_quote_ticks(symbol)
        self._remove_quote_tick_handler(symbol, handler)

    cdef inline void _handle_unsubscribe_trade_ticks(
        self,
        DataClient client,
        Symbol symbol,
        handler: callable,
    ) except *:
        # client already checked
        # validate message data
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")

        client.unsubscribe_trade_ticks(symbol)
        self._remove_trade_tick_handler(symbol, handler)

    cdef inline void _handle_unsubscribe_bars(
        self,
        DataClient client,
        BarType bar_type,
        handler: callable,
    ) except *:
        # client already checked
        # validate message data
        Condition.not_none(bar_type, "bar_type")
        Condition.callable(handler, "handler")

        if bar_type.is_internal_aggregation:
            # Internal aggregation
            self._remove_bar_handler(bar_type, handler)
            if bar_type not in self._bar_handlers:
                self._stop_bar_aggregator(client, bar_type)
        else:
            # External aggregation
            client.unsubscribe_bars(bar_type)
            self._remove_bar_handler(bar_type, handler)

# -- REQUEST HANDLERS ------------------------------------------------------------------------------

    cdef inline void _handle_request(self, DataRequest request) except *:
        self._log.debug(f"{RECV}{REQ} {request}.")
        self.request_count += 1

        cdef DataClient client = self._clients.get(request.venue)
        if client is None:
            self._log.error(f"Cannot handle request "
                            f"(no client registered for {request.venue}) {request}.")
            return  # No client to handle request

        if request.id in self._correlation_index:
            self._log.error(f"Cannot handle request "
                            f"(duplicate identifier {request.id} found in correlation index).")
            return  # Do not handle duplicates

        self._correlation_index[request.id] = request.callback

        if request.data_type == Instrument:
            symbol = request.metadata.get(SYMBOL)
            if symbol:
                client.request_instrument(symbol, request.id)
            else:
                client.request_instruments(request.id)
        elif request.data_type == QuoteTick:
            client.request_quote_ticks(
                request.metadata.get(SYMBOL),
                request.metadata.get(FROM_DATETIME),
                request.metadata.get(TO_DATETIME),
                request.metadata.get(LIMIT, 0),
                request.id,
            )
        elif request.data_type == TradeTick:
            client.request_trade_ticks(
                request.metadata.get(SYMBOL),
                request.metadata.get(FROM_DATETIME),
                request.metadata.get(TO_DATETIME),
                request.metadata.get(LIMIT, 0),
                request.id,
            )
        elif request.data_type == Bar:
            client.request_bars(
                request.metadata.get(BAR_TYPE),
                request.metadata.get(FROM_DATETIME),
                request.metadata.get(TO_DATETIME),
                request.metadata.get(LIMIT, 0),
                request.id,
            )
        else:
            self._log.error(f"Cannot handle request "
                            f"(data type {request.data_type} is unrecognized).")

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef inline void _handle_data(self, data) except *:
        # Not logging every data item received
        self.data_count += 1

        if isinstance(data, QuoteTick):
            self._handle_quote_tick(data)
        elif isinstance(data, TradeTick):
            self._handle_trade_tick(data)
        elif isinstance(data, BarData):
            self._handle_bar(data.bar_type, data.bar)
        elif isinstance(data, Instrument):
            self._handle_instrument(data)
        else:
            self._log.error(f"Cannot handle unrecognized data of type {type(data)}, {data}.")

    cdef inline void _handle_instrument(self, Instrument instrument) except *:
        self.cache.add_instrument(instrument)

        cdef list instrument_handlers = self._instrument_handlers.get(instrument.symbol)
        if instrument_handlers:
            for handler in instrument_handlers:
                handler(instrument)

    cdef inline void _handle_quote_tick(self, QuoteTick tick) except *:
        self.cache.add_quote_tick(tick)

        # Send to portfolio as a priority
        self.portfolio.update_tick(tick)

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._quote_tick_handlers.get(tick.symbol)
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler(tick)

    cdef inline void _handle_trade_tick(self, TradeTick tick) except *:
        self.cache.add_trade_tick(tick)

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._trade_tick_handlers.get(tick.symbol)
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler(tick)

    cdef inline void _handle_bar(self, BarType bar_type, Bar bar) except *:
        self.cache.add_bar(bar_type, bar)

        # Send to all registered bar handlers for that bar type
        cdef list bar_handlers = self._bar_handlers.get(bar_type)
        if bar_handlers is not None:
            for handler in bar_handlers:
                handler(bar_type, bar)

# -- RESPONSE HANDLERS -----------------------------------------------------------------------------

    cdef inline void _handle_response(self, DataResponse response) except *:
        self._log.debug(f"{RECV}{RES} {response}.")
        self.response_count += 1

        if response.data_type == Instrument:
            self._handle_instruments(response.data, response.correlation_id)
        elif response.data_type == QuoteTick:
            self._handle_quote_ticks(response.data, response.correlation_id)
        elif response.data_type == TradeTick:
            self._handle_trade_ticks(response.data, response.correlation_id)
        elif response.data_type == Bar:
            self._handle_bars(
                response.metadata.get(BAR_TYPE),
                response.data,
                response.metadata.get("Partial"),
                response.correlation_id,
            )
        else:
            self._log.error(f"Cannot handle data (data_type {response.data_type} is unrecognized).")

    cdef inline void _handle_instruments(self, list instruments, UUID correlation_id) except *:
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

    cdef inline void _handle_quote_ticks(self, list ticks, UUID correlation_id) except *:
        self.cache.add_quote_ticks(ticks)

        cdef callback = self._correlation_index.pop(correlation_id, None)
        if callback is None:
            self._log.error(f"Callback not found for correlation_id {correlation_id}.")
            return

        callback(ticks)

    cdef inline void _handle_trade_ticks(self, list ticks, UUID correlation_id) except *:
        self.cache.add_trade_ticks(ticks)

        cdef callback = self._correlation_index.pop(correlation_id, None)
        if callback is None:
            self._log.error(f"Callback not found for correlation_id {correlation_id}.")
            return

        callback(ticks)

    cdef inline void _handle_bars(self, BarType bar_type, list bars, Bar partial, UUID correlation_id) except *:
        self.cache.add_bars(bar_type, bars)

        cdef callback = self._correlation_index.pop(correlation_id, None)
        if callback is None:
            self._log.error(f"Callback not found for correlation_id {correlation_id}.")
            return

        cdef TimeBarAggregator
        if partial is not None:
            # Update partial time bar
            aggregator = self._bar_aggregators.get(bar_type)
            if aggregator:
                self._log.debug(f"Applying partial bar {partial} for {bar_type}.")
                aggregator.set_partial(partial)
            else:
                self._log.error("No aggregator for partial bar update.")

        callback(bar_type, bars)

# -- INTERNAL --------------------------------------------------------------------------------------

    # Python wrapper to enable callbacks
    cpdef void _internal_update_instruments(self, list instruments: [Instrument]) except *:
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cdef inline void _start_bar_aggregator(self, DataClient client, BarType bar_type) except *:
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
            raise RuntimeError(f"Cannot start aggregator, "
                               f"BarAggregation.{BarAggregationParser.to_str(bar_type.spec.aggregation)} "
                               f"not currently supported in this version")

        # Add aggregator
        self._bar_aggregators[bar_type] = aggregator
        self._log.debug(f"Added {aggregator} for {bar_type} bars.")

        # Subscribe to required data
        if bar_type.spec.price_type == PriceType.LAST:
            self._handle_subscribe_trade_ticks(client, bar_type.symbol, aggregator.handle_trade_tick)
        else:
            self._handle_subscribe_quote_ticks(client, bar_type.symbol, aggregator.handle_quote_tick)

    cdef inline void _hydrate_aggregator(
        self,
        DataClient client,
        TimeBarAggregator aggregator,
        BarType bar_type,
    ) except *:
        data_type = TradeTick if bar_type.spec.price_type == PriceType.LAST else QuoteTick

        if data_type == TradeTick and "request_trade_ticks" in client.unavailable_methods():
            return
        elif data_type == QuoteTick and "request_quote_ticks" in client.unavailable_methods():
            return

        # Update aggregator with latest data
        bulk_updater = BulkTimeBarUpdater(aggregator)

        # noinspection bulk_updater.receive
        # noinspection PyUnresolvedReferences
        request = DataRequest(
            venue=bar_type.symbol.venue,
            data_type=data_type,
            metadata={
                SYMBOL: bar_type.symbol,
                FROM_DATETIME: aggregator.get_start_time(),
                TO_DATETIME: None,
            },
            callback=bulk_updater.receive,
            request_id=self._uuid_factory.generate(),
            request_timestamp=self._clock.utc_now(),
        )

        # Send request directly to handler as we're already inside engine
        self._handle_request(request)

    cdef inline void _stop_bar_aggregator(self, DataClient client, BarType bar_type) except *:
        cdef aggregator = self._bar_aggregators.get(bar_type)
        if aggregator is None:
            self._log.warning(f"No bar aggregator to stop for {bar_type}")
            return

        if isinstance(aggregator, TimeBarAggregator):
            aggregator.stop()

        # Unsubscribe from update ticks
        if bar_type.spec.price_type == PriceType.LAST:
            self._handle_unsubscribe_trade_ticks(client, bar_type.symbol, aggregator.handle_trade_tick)
        else:
            self._handle_unsubscribe_quote_ticks(client, bar_type.symbol, aggregator.handle_quote_tick)

        # Remove from aggregators
        del self._bar_aggregators[bar_type]

    cdef inline void _bulk_build_tick_bars(
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
            bar_type.symbol,
            from_datetime,
            to_datetime,
            ticks_to_order,
            bar_builder.receive,
        )

# -- HANDLERS --------------------------------------------------------------------------------------

    cdef inline void _add_instrument_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._instrument_handlers:
            self._instrument_handlers[symbol] = []  # type: list[callable]
            self._log.info(f"Subscribed to {symbol} <Instrument> data.")

        # Add handler for subscriber
        if handler not in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].append(handler)
            self._log.debug(f"Added handler {handler} for {symbol} <Instrument> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {symbol} <Instrument> data.")

    cdef inline void _add_quote_tick_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._quote_tick_handlers:
            # Setup handlers
            self._quote_tick_handlers[symbol] = []  # type: list[callable]
            self._log.info(f"Subscribed to {symbol} <QuoteTick> data.")

        # Add handler for subscriber
        if handler not in self._quote_tick_handlers[symbol]:
            self._quote_tick_handlers[symbol].append(handler)
            self._log.debug(f"Added {handler} for {symbol} <QuoteTick> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {symbol} <QuoteTick> data.")

    cdef inline void _add_trade_tick_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._trade_tick_handlers:
            # Setup handlers
            self._trade_tick_handlers[symbol] = []  # type: list[callable]
            self._log.info(f"Subscribed to {symbol} <TradeTick> data.")

        # Add handler for subscriber
        if handler not in self._trade_tick_handlers[symbol]:
            self._trade_tick_handlers[symbol].append(handler)
            self._log.debug(f"Added {handler} for {symbol} <TradeTick> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {symbol} <TradeTick> data.")

    cdef inline void _add_bar_handler(self, BarType bar_type, handler: callable) except *:
        if bar_type not in self._bar_handlers:
            # Setup handlers
            self._bar_handlers[bar_type] = []  # type: list[callable]
            self._log.info(f"Subscribed to {bar_type} <Bar> data.")

        # Add handler for subscriber
        if handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(handler)
            self._log.debug(f"Added {handler} for {bar_type} <Bar> data.")
        else:
            self._log.warning(f"Handler {handler} already subscribed to {bar_type} <Bar> data.")

    cdef inline void _remove_instrument_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._instrument_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {symbol} <Instrument> data.")
            return

        # Remove subscribers handler
        if handler in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].remove(handler)
            self._log.debug(f"Removed handler {handler} for {symbol} <Instrument> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {symbol} <Instrument> data.")

        if not self._instrument_handlers[symbol]:  # No more handlers for symbol
            del self._instrument_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <Instrument> data.")

    cdef inline void _remove_quote_tick_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._quote_tick_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {symbol} <QuoteTick> data.")
            return

        # Remove subscribers handler
        if handler in self._quote_tick_handlers[symbol]:
            self._quote_tick_handlers[symbol].remove(handler)
            self._log.debug(f"Removed handler {handler} for {symbol} <QuoteTick> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {symbol} <QuoteTick> data.")

        if not self._quote_tick_handlers[symbol]:  # No more handlers for symbol
            del self._quote_tick_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <QuoteTick> data.")

    cdef inline void _remove_trade_tick_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._trade_tick_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {symbol} <TradeTick> data.")
            return

        # Remove subscribers handler
        if handler in self._trade_tick_handlers[symbol]:
            self._trade_tick_handlers[symbol].remove(handler)
            self._log.debug(f"Removed handler {handler} for {symbol} <TradeTick> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {symbol} <TradeTick> data.")

        if not self._trade_tick_handlers[symbol]:  # No more handlers for symbol
            del self._trade_tick_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <TradeTick> data.")

    cdef inline void _remove_bar_handler(self, BarType bar_type, handler: callable) except *:
        if bar_type not in self._bar_handlers:
            self._log.warning(f"Handler {handler} not subscribed to {bar_type} <Bar> data.")
            return

        # Remove subscribers handler
        if handler in self._bar_handlers[bar_type]:  # No more handlers for bar type
            self._bar_handlers[bar_type].remove(handler)
            self._log.debug(f"Removed handler {handler} for {bar_type} <Bar> data.")
        else:
            self._log.warning(f"Handler {handler} not subscribed to {bar_type} <Bar> data.")

        if not self._bar_handlers[bar_type]:
            del self._bar_handlers[bar_type]
            self._log.info(f"Unsubscribed from {bar_type} <Bar> data.")
