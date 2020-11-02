# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
The `DataEngine` is the central component of the entire data stack for the platform.

Its primary responsibility is to orchestrate interactions between the individual
`DataClient` instances, and the rest of the platform. This includes consumers
subscribing to specific data types, as well as hydrating a `DataCache` layer
which presents a read-only facade for consumers.

Alternative implementations can be written on top which just need to override
the engines `execute` and `process` methods.
"""

import cython

from cpython.datetime cimport datetime

from nautilus_trader.common.c_enums.component_trigger cimport ComponentTrigger
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.commands cimport Connect
from nautilus_trader.common.commands cimport Disconnect
from nautilus_trader.common.commands cimport RequestData
from nautilus_trader.common.commands cimport Subscribe
from nautilus_trader.common.commands cimport Unsubscribe
from nautilus_trader.common.component cimport ComponentFSMFactory
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.constants cimport *  # str constants
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.data.aggregation cimport BarAggregator
from nautilus_trader.data.aggregation cimport TickBarAggregator
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.wrappers cimport BarData
from nautilus_trader.data.wrappers cimport BarDataBlock
from nautilus_trader.data.wrappers cimport InstrumentDataBlock
from nautilus_trader.data.wrappers cimport QuoteTickDataBlock
from nautilus_trader.data.wrappers cimport TradeTickDataBlock
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataEngine:
    """
    Provides a high-performance data engine for managing many `DataClient`
    instances, for the asynchronous ingest of data.
    """

    def __init__(
            self,
            Portfolio portfolio not None,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
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
        uuid_factory : UUIDFactory
            The UUID factory for the component.
        logger : Logger
            The logger for the component.
        config : dict, option
            The configuration options.

        """
        if config is None:
            config = {}

        # Core components
        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(type(self).__name__, logger)
        self._fsm = ComponentFSMFactory.create()

        self._use_previous_close = config.get('use_previous_close', True)
        self._clients = {}              # type: {Venue, DataClient}

        # Handlers
        self._instrument_handlers = {}  # type: {Symbol, [callable]}
        self._quote_tick_handlers = {}  # type: {Symbol, [callable]}
        self._trade_tick_handlers = {}  # type: {Symbol, [callable]}
        self._bar_handlers = {}         # type: {BarType, [callable]}

        # Aggregators
        self._bar_aggregators = {}      # type: {BarType, BarAggregator}

        # Public components
        self.portfolio = portfolio
        self.cache = DataCache(logger)

        # Counters
        self.command_count = 0
        self.data_count = 0

        self._log.info("Initialized.")
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
    def subscribed_quote_ticks(self):
        """
        Return the quote tick symbols subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return sorted(list(self._quote_tick_handlers.keys()))

    @property
    def subscribed_trade_ticks(self):
        """
        Return the trade tick symbols subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return sorted(list(self._trade_tick_handlers.keys()))

    @property
    def subscribed_bars(self):
        """
        Return the bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        return sorted(list(self._bar_handlers.keys()))

    @property
    def subscribed_instruments(self):
        """
        Return the instruments subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return sorted(list(self._instrument_handlers.keys()))

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
        Condition.not_in(client.venue, self._clients, "client", "_clients")

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

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void execute(self, Command command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : Command
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, object data) except *:
        """
        Process the given data.

        Parameters
        ----------
        data : object
            The data to process.

        """
        Condition.not_none(data, "data")

        self._handle_data(data)

    cpdef void reset(self) except *:
        """
        Reset the class to its initial state.

        All stateful values are reset to their initial value.
        """
        try:
            self._fsm.trigger(ComponentTrigger.RESET)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            return

        self._log.info(f"state={self._fsm.state_string()}...")

        cdef DataClient client
        for client in self._clients.values():
            client.reset()

        self.cache.reset()
        self._instrument_handlers.clear()
        self._quote_tick_handlers.clear()
        self._trade_tick_handlers.clear()
        self._bar_aggregators.clear()
        self._bar_handlers.clear()
        self._clock.cancel_all_timers()
        self.command_count = 0
        self.data_count = 0

        self._fsm.trigger(ComponentTrigger.RESET)  # State changes to initialized
        self._log.info(f"state={self._fsm.state_string()}.")

    cpdef void dispose(self) except *:
        """
        Dispose the data engine

        All registered data clients are disposed.
        """
        try:
            self._fsm.trigger(ComponentTrigger.DISPOSE)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            return

        self._log.info(f"state={self._fsm.state_string()}...")

        cdef DataClient client
        for client in self._clients.values():
            client.dispose()

        self._fsm.trigger(ComponentTrigger.DISPOSED)
        self._log.info(f"state={self._fsm.state_string()}.")

    cpdef void update_instruments(self, Venue venue) except *:
        """
        Update all instruments for the given venue.
        """
        Condition.not_none(venue, "venue")
        Condition.is_in(venue, self._clients, "venue", "_clients")

        cdef RequestData request = RequestData(
            data_type=Instrument,
            options={
                VENUE: venue,
                HANDLER: self._internal_update_instruments,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self.execute(request)

    cpdef void update_instruments_all(self) except *:
        """
        Update all instruments for every venue.
        """
        cdef Venue venue
        for venue in self.registered_venues():
            self.update_instruments(venue)

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, Command command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        if isinstance(command, Connect):
            self._handle_connect(command)
        elif isinstance(command, Disconnect):
            self._handle_disconnect(command)
        elif isinstance(command, Subscribe):
            self._handle_subscribe(command)
        elif isinstance(command, Unsubscribe):
            self._handle_unsubscribe(command)
        elif isinstance(command, RequestData):
            self._handle_request(command)
        else:
            self._log.error(f"Cannot handle command ({command} is unrecognized).")

    cdef inline void _handle_connect(self, Connect command) except *:
        self._log.info("Connecting all clients...")

        cdef DataClient client
        if command.venue is not None:
            client = self._clients.get(command.venue)
            if client is None:
                self._log.error(f"Cannot execute {command} "
                                f"(venue {command.venue} not registered).")
            else:
                client.connect()
        else:
            for client in self._clients:
                client.connect()

    cdef inline void _handle_disconnect(self, Disconnect command) except *:
        self._log.info("Disconnecting all clients...")

        cdef DataClient client
        if command.venue is not None:
            client = self._clients.get(command.venue)
            if client is None:
                self._log.error(f"Cannot execute {command} "
                                f"(venue {command.venue} not registered).")
            else:
                client.disconnect()
        else:
            for client in self._clients:
                client.disconnect()

    cdef inline void _handle_subscribe(self, Subscribe command) except *:
        if command.data_type == Instrument:
            self._handle_subscribe_instrument(
                command.options.get(SYMBOL),
                command.options.get(HANDLER),
            )
        elif command.data_type == QuoteTick:
            self._handle_subscribe_quote_ticks(
                command.options.get(SYMBOL),
                command.options.get(HANDLER),
            )
        elif command.data_type == TradeTick:
            self._handle_subscribe_trade_ticks(
                command.options.get(SYMBOL),
                command.options.get(HANDLER),
            )
        elif command.data_type == Bar:
            self._handle_subscribe_bars(
                command.options.get(BAR_TYPE),
                command.options.get(HANDLER),
            )
        else:
            self._log.error(f"Cannot handle command ({command.data_type} is unrecognized).")

    cdef inline void _handle_unsubscribe(self, Unsubscribe command) except *:
        if command.data_type == Instrument:
            self._handle_unsubscribe_instrument(
                command.options.get(SYMBOL),
                command.options.get(HANDLER),
            )
        elif command.data_type == QuoteTick:
            self._handle_unsubscribe_quote_ticks(
                command.options.get(SYMBOL),
                command.options.get(HANDLER),
            )
        elif command.data_type == TradeTick:
            self._handle_unsubscribe_trade_ticks(
                command.options.get(SYMBOL),
                command.options.get(HANDLER),
            )
        elif command.data_type == Bar:
            self._handle_unsubscribe_bars(
                command.options.get(BAR_TYPE),
                command.options.get(HANDLER),
            )
        else:
            self._log.error(f"Cannot handle command ({command.data_type} is unrecognized).")

    cdef inline void _handle_request(self, RequestData command) except *:
        if command.data_type == Instrument:
            venue = command.options.get(VENUE)
            if venue is not None:
                self._handle_request_instruments(
                    venue,
                    command.options.get(CALLBACK),
                )
            else:
                self._handle_request_instrument(
                    command.options.get(SYMBOL),
                    command.options.get(CALLBACK),
                )
        elif command.data_type == QuoteTick:
            self._handle_request_quote_ticks(
                command.options.get(SYMBOL),
                command.options.get(FROM_DATETIME),
                command.options.get(TO_DATETIME),
                command.options.get(LIMIT),
                command.options.get(CALLBACK),
            )
        elif command.data_type == TradeTick:
            self._handle_request_trade_ticks(
                command.options.get(SYMBOL),
                command.options.get(FROM_DATETIME),
                command.options.get(TO_DATETIME),
                command.options.get(LIMIT),
                command.options.get(CALLBACK),
            )
        elif command.data_type == Bar:
            self._handle_request_bars(
                command.options.get(BAR_TYPE),
                command.options.get(FROM_DATETIME),
                command.options.get(TO_DATETIME),
                command.options.get(LIMIT),
                command.options.get(CALLBACK),
            )
        else:
            self._log.error(f"Cannot handle command ({command.data_type} is unrecognized).")

    cdef inline void _handle_request_instrument(self, Symbol symbol, callback: callable) except *:
        Condition.not_none(symbol, "symbol")
        Condition.callable(callback, "callback")
        Condition.is_in(symbol.venue, self._clients, "venue", "_clients")

        self._clients[symbol.venue].request_instrument(symbol, callback)

    cdef inline void _handle_request_instruments(self, Venue venue, callback: callable) except *:
        Condition.not_none(venue, "venue")
        Condition.callable(callback, "callback")
        Condition.is_in(venue, self._clients, "venue", "_clients")

        self._clients[venue].request_instruments(venue, callback)

    cdef inline void _handle_request_quote_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,  # Can be None
            datetime to_datetime,    # Can be None
            int limit,
            callback: callable,
    ) except *:
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.callable(callback, "callback")
        Condition.is_in(symbol.venue, self._clients, "venue", "_clients")

        self._clients[symbol.venue].request_quote_ticks(
            symbol,
            from_datetime,
            to_datetime,
            limit,
            callback,
        )

    cdef inline void _handle_request_trade_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable,
    ) except *:
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.callable(callback, "callback")
        Condition.is_in(symbol.venue, self._clients, "venue", "_clients")

        self._clients[symbol.venue].request_trade_ticks(
            symbol,
            from_datetime,
            to_datetime,
            limit,
            callback,
        )

    cdef inline void _handle_request_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable,
    ) except *:
        Condition.not_none(bar_type, "bar_type")
        Condition.not_negative_int(limit, "limit")
        Condition.callable(callback, "callback")
        Condition.is_in(bar_type.symbol.venue, self._clients, "venue", "_clients")

        self._clients[bar_type.symbol.venue].request_bars(
            bar_type,
            from_datetime,
            to_datetime,
            limit,
            callback,
        )

    cdef inline void _handle_subscribe_instrument(self, Symbol symbol, handler: callable) except *:
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._add_instrument_handler(symbol, handler)
        self._clients[symbol.venue].subscribe_instrument(symbol)

    cdef inline void _handle_subscribe_quote_ticks(self, Symbol symbol, handler: callable) except *:
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._add_quote_tick_handler(symbol, handler)
        self._clients[symbol.venue].subscribe_quote_ticks(symbol)

    cdef inline void _handle_subscribe_trade_ticks(self, Symbol symbol, handler: callable) except *:
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._add_trade_tick_handler(symbol, handler)
        self._clients[symbol.venue].subscribe_trade_ticks(symbol)

    cdef inline void _handle_subscribe_bars(self, BarType bar_type, handler: callable) except *:
        Condition.not_none(bar_type, "bar_type")
        Condition.callable(handler, "handler")
        Condition.is_in(bar_type.symbol.venue, self._clients, "bar_type.symbol.venue", "_clients")

        # TODO: Refactor to make this optional
        self._start_generating_bars(bar_type, handler)
        # self._clients[bar_type.symbol.venue].subscribe_bars(bar_type)
        # self._add_bar_handler(bar_type, handler)

    cdef inline void _handle_unsubscribe_instrument(self, Symbol symbol, handler: callable) except *:
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._clients[symbol.venue].unsubscribe_instrument(symbol)
        self._remove_instrument_handler(symbol, handler)

    cdef inline void _handle_unsubscribe_quote_ticks(self, Symbol symbol, handler: callable) except *:
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._clients[symbol.venue].unsubscribe_quote_ticks(symbol)
        self._remove_quote_tick_handler(symbol, handler)

    cdef inline void _handle_unsubscribe_trade_ticks(self, Symbol symbol, handler: callable) except *:
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._clients[symbol.venue].unsubscribe_trade_ticks(symbol)
        self._remove_trade_tick_handler(symbol, handler)

    cdef inline void _handle_unsubscribe_bars(self, BarType bar_type, handler: callable) except *:
        Condition.not_none(bar_type, "bar_type")
        Condition.callable(handler, "handler")
        Condition.is_in(bar_type.symbol.venue, self._clients, "bar_type.symbol.venue", "_clients")

        # TODO: Refactor to make this optional
        self._stop_generating_bars(bar_type, handler)
        # self._clients[bar_type.symbol.venue].unsubscribe_bars(bar_type)
        # self._remove_bar_handler(bar_type, handler)

# -- DATA HANDLERS -------------------------------------------------------------------------------

    cdef inline void _handle_data(self, object data) except *:
        self.data_count += 1

        if isinstance(data, QuoteTick):
            self._handle_quote_tick(data)
        elif isinstance(data, TradeTick):
            self._handle_trade_tick(data)
        elif isinstance(data, BarData):
            self._handle_bar(data.bar_type, data.bar)
        elif isinstance(data, Instrument):
            self._handle_instrument(data)
        elif isinstance(data, QuoteTickDataBlock):
            self._handle_quote_ticks(data.ticks)
        elif isinstance(data, TradeTickDataBlock):
            self._handle_trade_ticks(data.ticks)
        elif isinstance(data, BarDataBlock):
            self._handle_bars(data.bar_type, data.bars)
        elif isinstance(data, InstrumentDataBlock):
            self._handle_instruments(data.instruments)
        else:
            self._log.error(f"Cannot handle data ({data} is unrecognized).")

    cdef inline void _handle_instrument(self, Instrument instrument) except *:
        self.cache.add_instrument(instrument)

        self.portfolio.update_instrument(instrument)

        cdef list instrument_handlers = self._instrument_handlers.get(instrument.symbol)
        if instrument_handlers:
            for handler in instrument_handlers:
                handler.handle(instrument)

    cdef inline void _handle_instruments(self, list instruments: [Instrument]) except *:
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cdef inline void _handle_quote_tick(self, QuoteTick tick, bint send_to_handlers=True) except *:
        self.cache.add_quote_tick(tick)

        if not send_to_handlers:
            return

        # Send to portfolio as a priority
        self.portfolio.update_tick(tick)

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._quote_tick_handlers.get(tick.symbol)
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler(tick)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef inline void _handle_quote_ticks(self, list ticks: [QuoteTick]) except *:
        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self._log.debug(f"Received <QuoteTick[{length}]> data for {symbol}.")
        else:
            self._log.debug("Received <QuoteTick[]> data with no ticks.")

        cdef int i
        for i in range(length):
            self._handle_quote_tick(ticks[i], send_to_handlers=False)

    cdef inline void _handle_trade_tick(self, TradeTick tick, bint send_to_handlers=True) except *:
        cdef Symbol symbol = tick.symbol

        self._cache.add_trade_tick(tick)

        if not send_to_handlers:
            return

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._trade_tick_handlers.get(tick.symbol)
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler(tick)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef inline void _handle_trade_ticks(self, list ticks: [TradeTick]) except *:
        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self._log.debug(f"Received <TradeTick[{length}]> data for {symbol}.")
        else:
            self._log.debug("Received <TradeTick[]> data with no ticks.")

        cdef int i
        for i in range(length):
            self._handle_trade_tick(ticks[i], send_to_handlers=False)

    cdef inline void _handle_bar(self, BarType bar_type, Bar bar, bint send_to_handlers=True) except *:
        self.cache.add_bar(bar_type, bar)

        if not send_to_handlers:
            return

        # Send to all registered bar handlers for that bar type
        cdef list bar_handlers = self._bar_handlers.get(bar_type)
        if bar_handlers is not None:
            for handler in bar_handlers:
                handler(bar_type, bar)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef inline void _handle_bars(self, BarType bar_type, list bars: [Bar]) except *:
        cdef int length = len(bars)

        self._log.debug(f"Received <Bar[{length}]> data for {bar_type}.")

        if length > 0 and bars[0].timestamp > bars[length - 1].timestamp:
            raise RuntimeError("Cannot handle <Bar[]> data (incorrectly sorted).")

        cdef int i
        for i in range(length):
            self._handle_bar(bar_type, bars[i], send_to_handlers=False)

# -- INTERNAL --------------------------------------------------------------------------------------

    # Wrapper for _start_generating_bars to be able to pass _handle_bar as a callback
    cpdef void _py_handle_bar(self, BarType bar_type, Bar bar) except *:
        self._handle_bar(bar_type, bar)

    cdef inline void _internal_update_instruments(self, list instruments: [Instrument]) except *:
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cdef inline void _start_generating_bars(self, BarType bar_type, handler: callable) except *:
        if bar_type not in self._bar_aggregators:
            if bar_type.spec.aggregation == BarAggregation.TICK:
                aggregator = TickBarAggregator(bar_type, self._py_handle_bar, self._log.get_logger())
            elif bar_type.is_time_aggregated():
                aggregator = TimeBarAggregator(
                    bar_type=bar_type,
                    handler=self._py_handle_bar,
                    use_previous_close=self._use_previous_close,
                    clock=self._clock,
                    logger=self._log.get_logger(),
                )

                bulk_updater = BulkTimeBarUpdater(aggregator)

                if bar_type.spec.price_type == PriceType.LAST:
                    self._handle_request_quote_ticks(
                        symbol=bar_type.symbol,
                        from_datetime=aggregator.get_start_time(),
                        to_datetime=None,  # Max
                        limit=0,  # No limit
                        callback=bulk_updater.receive,
                    )
                else:
                    self._handle_request_trade_ticks(
                        symbol=bar_type.symbol,
                        from_datetime=aggregator.get_start_time(),
                        to_datetime=None,  # Max
                        limit=0,  # No limit
                        callback=bulk_updater.receive,
                    )

            # Add aggregator and subscribe to QuoteTick updates
            self._bar_aggregators[bar_type] = aggregator
            self._log.debug(f"Added {aggregator} for {bar_type} bars.")

            # Subscribe to required data
            if bar_type.spec.price_type == PriceType.LAST:
                self._handle_subscribe_trade_ticks(bar_type.symbol, aggregator.handle_trade_tick)
            else:
                self._handle_subscribe_quote_ticks(bar_type.symbol, aggregator.handle_quote_tick)

        self._add_bar_handler(bar_type, handler)  # Add handler last

    cdef inline void _stop_generating_bars(self, BarType bar_type, handler: callable) except *:
        if bar_type in self._bar_handlers:  # Remove handler first
            self._remove_bar_handler(bar_type, handler)
            if bar_type not in self._bar_handlers:  # No longer any handlers for that bar type
                aggregator = self._bar_aggregators[bar_type]
                if isinstance(aggregator, TimeBarAggregator):
                    aggregator.stop()

                # Unsubscribe from update ticks
                if bar_type.spec.price_type == PriceType.LAST:
                    self._handle_unsubscribe_trade_ticks(bar_type.symbol, aggregator.handle_trade_tick)
                else:
                    self._handle_unsubscribe_quote_ticks(bar_type.symbol, aggregator.handle_quote_tick)

                del self._bar_aggregators[bar_type]

    cdef inline void _add_quote_tick_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._quote_tick_handlers:
            # Setup handlers
            self._quote_tick_handlers[symbol] = []  # type: [callable]
            self._log.info(f"Subscribed to {symbol} <QuoteTick> data.")

        # Add handler for subscriber
        if handler not in self._quote_tick_handlers[symbol]:
            self._quote_tick_handlers[symbol].append(handler)
            self._log.debug(f"Added {handler} for {symbol} <QuoteTick> data.")
        else:
            self._log.error(f"Cannot add {handler} for {symbol} <QuoteTick> data"
                            f"(duplicate handler found).")

    cdef inline void _add_trade_tick_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._trade_tick_handlers:
            # Setup handlers
            self._trade_tick_handlers[symbol] = []  # type: [callable]
            self._log.info(f"Subscribed to {symbol} <TradeTick> data.")

        # Add handler for subscriber
        if handler not in self._trade_tick_handlers[symbol]:
            self._trade_tick_handlers[symbol].append(handler)
            self._log.debug(f"Added {handler} for {symbol} <TradeTick> data.")
        else:
            self._log.error(f"Cannot add {handler} for {symbol} <TradeTick> data"
                            f"(duplicate handler found).")

    cdef inline void _add_bar_handler(self, BarType bar_type, handler: callable) except *:
        if bar_type not in self._bar_handlers:
            # Setup handlers
            self._bar_handlers[bar_type] = []  # type: [callable]
            self._log.info(f"Subscribed to {bar_type} <Bar> data.")

        # Add handler for subscriber
        if handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(handler)
            self._log.debug(f"Added {handler} for {bar_type} <Bar> data.")
        else:
            self._log.error(f"Cannot add {handler} for {bar_type} <Bar> data "
                            f"(duplicate handler found).")

    cdef inline void _add_instrument_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._instrument_handlers:
            self._instrument_handlers[symbol] = []  # type: [callable]
            self._log.info(f"Subscribed to {symbol} <Instrument> data.")

        if handler not in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].append(handler)
            self._log.debug(f"Added {handler} for {symbol} <Instrument> data.")
        else:
            self._log.error(f"Cannot add {handler} for {symbol} <Instrument> data"
                            f"(duplicate handler found).")

    cdef inline void _remove_quote_tick_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._quote_tick_handlers:
            self._log.debug(f"Cannot remove handler for {symbol} <QuoteTick> data "
                            f"(no handlers found).")
            return

        # Remove subscribers handler
        if handler in self._quote_tick_handlers[symbol]:
            self._quote_tick_handlers[symbol].remove(handler)
            self._log.debug(f"Removed handler {handler} for {symbol} <QuoteTick> data.")
        else:
            self._log.error(f"Cannot remove {handler} for {symbol} <QuoteTick> data "
                            f"(no matching handler found).")

        if not self._quote_tick_handlers[symbol]:
            del self._quote_tick_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <QuoteTick> data.")

    cdef inline void _remove_trade_tick_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._trade_tick_handlers:
            self._log.debug(f"Cannot remove handler for {symbol} <TradeTick> data "
                            f"(no handlers found).")
            return

        # Remove subscribers handler
        if handler in self._trade_tick_handlers[symbol]:
            self._trade_tick_handlers[symbol].remove(handler)
            self._log.debug(f"Removed handler {handler} for {symbol} <TradeTick> data.")
        else:
            self._log.error(f"Cannot remove {handler} for {symbol} <TradeTick> data"
                            f"(no matching handler found).")

        if not self._trade_tick_handlers[symbol]:
            del self._trade_tick_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <TradeTick> data.")

    cdef inline void _remove_bar_handler(self, BarType bar_type, handler: callable) except *:
        if bar_type not in self._bar_handlers:
            self._log.debug(f"Cannot remove handler for {bar_type} <Bar> data "
                            f"(no handlers found).")
            return

        # Remove subscribers handler
        if handler in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].remove(handler)
            self._log.debug(f"Removed handler {handler} for {bar_type} <Bar> data.")
        else:
            self._log.debug(f"Cannot remove {handler} for {bar_type} <Bar> data"
                            f"(no matching handler found).")

        if not self._bar_handlers[bar_type]:
            del self._bar_handlers[bar_type]
            self._log.info(f"Unsubscribed from {bar_type} <Bar> data.")

    cdef inline void _remove_instrument_handler(self, Symbol symbol, handler: callable) except *:
        if symbol not in self._instrument_handlers:
            self._log.debug(f"Cannot remove handler for {symbol} <Instrument> data "
                            f"(no handlers found).")
            return

        if handler in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].remove(handler)
            self._log.debug(f"Removed handler {handler} for {symbol} <Instrument> data.")
        else:
            self._log.debug(f"Cannot remove {handler} for {symbol} <Instrument> data "
                            f"(no matching handler found).")

        if not self._instrument_handlers[symbol]:
            del self._instrument_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <Instrument> data.")

    cdef inline void _bulk_build_tick_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback,
    ) except *:
        # Bulk build tick bars
        cdef int ticks_to_order = bar_type.spec.step * limit

        cdef BulkTickBarBuilder bar_builder = BulkTickBarBuilder(
            bar_type,
            self._log.get_logger(),
            callback,
        )

        self._handle_request_quote_ticks(
            bar_type.symbol,
            from_datetime,
            to_datetime,
            ticks_to_order,
            bar_builder.receive,
        )


cdef class BulkTickBarBuilder:
    """
    Provides a temporary builder for tick bars from a bulk tick order.
    """

    def __init__(
            self,
            BarType bar_type not None,
            Logger logger not None,
            callback not None: callable,
    ):
        """
        Initialize a new instance of the `BulkTickBarBuilder` class.

        Parameters
        ----------
        bar_type : BarType
            The bar_type to build.
        logger : Logger
            The logger for the bar aggregator.
        callback : callable
            The callback to send the built bars to.

        Raises
        ------
        ValueError
            If callback is not of type callable.

        """
        Condition.callable(callback, "callback")

        self.bars = []
        self.aggregator = TickBarAggregator(bar_type, self._add_bar, logger)
        self.callback = callback

    # noinspection: Object has warned attribute
    # noinspection PyUnresolvedReferences
    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void receive(self, list ticks) except *:
        """
        Receive the bulk list of ticks and build aggregated bars.

        Then send the bar type and bars list on to the registered callback.

        Parameters
        ----------
        ticks : list[Tick]
            The ticks for aggregation.

        """
        Condition.not_none(ticks, "ticks")

        cdef int i
        if self.aggregator.bar_type.spec.price_type == PriceType.LAST:
            for i in range(len(ticks)):
                self.aggregator.handle_trade_tick(ticks[i])
        else:
            for i in range(len(ticks)):
                self.aggregator.handle_quote_tick(ticks[i])

        self.callback(self.aggregator.bar_type, self.bars)

    cpdef void _add_bar(self, BarType bar_type, Bar bar) except *:
        self.bars.append(bar)


cdef class BulkTimeBarUpdater:
    """
    Provides a temporary updater for time bars from a bulk tick order.
    """

    def __init__(self, TimeBarAggregator aggregator not None):
        """
        Initialize a new instance of the `BulkTimeBarUpdater` class.

        Parameters
        ----------
        aggregator : TimeBarAggregator
            The time bar aggregator to update.

        """
        self.aggregator = aggregator
        self.start_time = self.aggregator.next_close - self.aggregator.interval

    # noinspection: Object has warned attribute
    # noinspection PyUnresolvedReferences
    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void receive(self, list ticks) except *:
        """
        Receive the bulk list of ticks and update the aggregator.

        Parameters
        ----------
        ticks : list[Tick]
            The ticks for updating.

        """
        cdef int i
        if self.aggregator.bar_type.spec.price_type == PriceType.LAST:
            for i in range(len(ticks)):
                if ticks[i].timestamp < self.start_time:
                    continue  # Price not applicable to this bar
                self.aggregator.handle_trade_tick(ticks[i])
        else:
            for i in range(len(ticks)):
                if ticks[i].timestamp < self.start_time:
                    continue  # Price not applicable to this bar
                self.aggregator.handle_quote_tick(ticks[i])
