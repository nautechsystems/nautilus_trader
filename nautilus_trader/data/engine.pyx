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

import cython

from cpython.datetime cimport datetime

from typing import List

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.handlers cimport BarHandler
from nautilus_trader.common.handlers cimport InstrumentHandler
from nautilus_trader.common.handlers cimport QuoteTickHandler
from nautilus_trader.common.handlers cimport TradeTickHandler
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.aggregation cimport BarAggregator
from nautilus_trader.data.aggregation cimport TickBarAggregator
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataEngine:
    """
    Provides a generic data engine for managing many data clients.
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
        Initialize a new instance of the DataEngine class.

        Parameters
        ----------
        portfolio : int
            The portfolio to register to receive quote ticks.
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

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._portfolio = portfolio
        self._clients = {}              # type: {Venue, DataClient}

        self._use_previous_close = config.get('use_previous_close', True)

        self.cache = DataCache(logger)

        # Handlers
        self._instrument_handlers = {}  # type: {Symbol, [InstrumentHandler]}
        self._quote_tick_handlers = {}  # type: {Symbol, [QuoteTickHandler]}
        self._trade_tick_handlers = {}  # type: {Symbol, [TradeTickHandler]}
        self._bar_aggregators = {}      # type: {BarType, BarAggregator}

        # Aggregators
        self._bar_handlers = {}         # type: {BarType, [BarHandler]}

        self._log.info("Initialized.")
        self._log.info(f"use_previous_close={self._use_previous_close}")

    cpdef void connect(self) except *:
        """
        Connect all data clients.
        """
        self._log.info("Connecting all clients...")

        cdef DataClient client
        for client in self._clients:
            client.connect()

    cpdef void disconnect(self) except *:
        """
        Disconnect all data clients.
        """
        self._log.info("Disconnecting all clients...")

        cdef DataClient client
        for client in self._clients:
            client.disconnect()

    cpdef void reset(self) except *:
        """
        Reset all data clients.
        """
        self._log.info("Resetting all clients...")

        cdef DataClient client
        for client in self._clients:
            client.reset()

        self.cache.reset()
        self._instrument_handlers.clear()
        self._quote_tick_handlers.clear()
        self._trade_tick_handlers.clear()
        self._bar_aggregators.clear()
        self._bar_handlers.clear()

    cpdef void dispose(self) except *:
        """
        Dispose all data clients.
        """
        self._log.info("Disposing all clients...")

        cdef DataClient client
        for client in self._clients:
            client.dispose()

    cpdef void update_instruments(self, Venue venue) except *:
        """
        Update all instruments for the given venue.
        """
        Condition.not_none(venue, "venue")
        Condition.is_in(venue, self._clients, "venue", "_clients")

        self._clients[venue].request_instruments(self._internal_update_instruments)

    cpdef void update_instruments_all(self) except *:
        """
        Update all instruments for every venue.
        """
        cdef DataClient client
        for client in self._clients.values():
            client.request_instruments(self._internal_update_instruments)

    cpdef void request_instrument(self, Symbol symbol, callback: callable) except *:
        """
        Request the latest instrument data for the given symbol.

        The response will be sent to the given callback.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the request.
        callback : Callable
            The callback for the instrument.

        Raises
        ------
        ValueError
            If callback is not of type Callable.
        ValueError
            If a data client has not been registered for the symbol.venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(callback, "callback")
        Condition.is_in(symbol.venue, self._clients, "venue", "_clients")

        self._clients[symbol.venue].request_instrument(symbol, callback)

    cpdef void request_instruments(self, Venue venue, callback: callable) except *:
        """
        Request the latest instruments data for the given venue.

        The response will be sent to the given callback.

        Parameters
        ----------
        venue : Venue
            The venue for the request.
        callback : Callable
            The callback for the instruments.

        Raises
        ------
        ValueError
            If callback is not of type Callable.
        ValueError
            If a data client has not been registered for the venue.

        """
        Condition.not_none(venue, "venue")
        Condition.callable(callback, "callback")
        Condition.is_in(venue, self._clients, "venue", "_clients")

        self._clients[venue].request_instruments(venue, callback)

    cpdef void request_quote_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,  # Can be None
            datetime to_datetime,    # Can be None
            int limit,
            callback: callable,
    ) except *:
        """
        Request historical quote ticks for the given parameters.

        The response will be sent to the given callback.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the request.
        from_datetime : datetime
            The start of the request range.
        to_datetime : datetime
            The end of the request range.
        limit : int
            The limit for the number of ticks.
        callback : Callable
            The callback for the ticks.

        Raises
        ------
        ValueError
            If limit is not positive (> 0).
        ValueError
            If callback is not of type Callable.
        ValueError
            If a data client has not been registered for the symbol.venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.positive_int(limit, "limit")
        Condition.callable(callback, "callback")
        Condition.is_in(symbol.venue, self._clients, "venue", "_clients")

        self._clients[symbol.venue].request_quote_ticks(
            symbol,
            from_datetime,
            to_datetime,
            limit,
            callback,
        )

    cpdef void request_trade_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable,
    ) except *:
        """
        Request historical trade ticks for the given parameters.

        The response will be sent to the given callback.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the request.
        from_datetime : datetime
            The start of the request range.
        to_datetime : datetime
            The end of the request range.
        limit : int
            The limit for the number of ticks.
        callback : Callable
            The callback for the ticks.

        Raises
        ------
        ValueError
            If limit is not positive (> 0).
        ValueError
            If callback is not of type Callable.
        ValueError
            If a data client has not been registered for the symbol.venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.positive_int(limit, "limit")
        Condition.callable(callback, "callback")
        Condition.is_in(symbol.venue, self._clients, "venue", "_clients")

        self._clients[symbol.venue].request_trade_ticks(
            symbol,
            from_datetime,
            to_datetime,
            limit,
            callback,
        )

    cpdef void request_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable,
    ) except *:
        """
        Request historical bars for the given parameters.

        The response will be sent to the given callback.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        from_datetime : datetime
            The start of the request range.
        to_datetime : datetime
            The end of the request range.
        limit : int
            The limit for the number of bars.
        callback : Callable
            The callback for the bars.

        Raises
        ------
        ValueError
            If limit is not positive (> 0).
        ValueError
            If callback is not of type Callable.
        ValueError
            If a data client has not been registered for the bar_type.symbol.venue.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.positive_int(limit, "limit")
        Condition.callable(callback, "callback")
        Condition.is_in(bar_type.symbol.venue, self._clients, "venue", "_clients")

        self._clients[bar_type.symbol.venue].request_bars(
            bar_type,
            from_datetime,
            to_datetime,
            limit,
            callback,
        )

    cpdef void subscribe_instrument(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to instrument updates for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The instrument symbol to subscribe to.
        handler : Callable
            The handler to receive the instruments.

        Raises
        ------
        ValueError
            If handler is not of type Callable.
        ValueError
            If a data client has not been registered for the symbol.venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._add_instrument_handler(symbol, handler)
        self._clients[symbol.venue].subscribe_instrument(symbol)

    cpdef void subscribe_quote_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to a stream of quote ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.
        handler : Callable
            The handler to receive the tick stream.

        Raises
        ------
        ValueError
            If handler is not of type Callable.
        ValueError
            If a data client has not been registered for the symbol.venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._add_quote_tick_handler(symbol, handler)
        self._clients[symbol.venue].subscribe_quote_ticks(symbol)

    cpdef void subscribe_trade_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to a stream of trade ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.
        handler : Callable
            The handler to receive the tick stream.

        Raises
        ------
        ValueError
            If handler is not of type Callable.
        ValueError
            If a data client has not been registered for the symbol.venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._add_trade_tick_handler(symbol, handler)
        self._clients[symbol.venue].subscribe_trade_ticks(symbol)

    cpdef void subscribe_bars(self, BarType bar_type, handler: callable) except *:
        """
        Subscribe to a stream of bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.
        handler : Callable
            The handler to receive the bar stream.

        Raises
        ------
        ValueError
            If handler is not of type Callable.
        ValueError
            If a data client has not been registered for the bar_type.symbol.venue.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.callable(handler, "handler")
        Condition.is_in(bar_type.symbol.venue, self._clients, "bar_type.symbol.venue", "_clients")

        self._add_bar_handler(bar_type, handler)
        self._clients[bar_type.symbol.venue].subscribe_bars(bar_type)

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from the stream of trade ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The instrument symbol to unsubscribe from.
        handler : Callable
            The handler which was receiving the instrument.

        Raises
        ------
        ValueError
            If handler is not of type Callable.
        ValueError
            If a data client has not been registered for the venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._clients[symbol.venue].unsubscribe_instrument(symbol)
        self._remove_instrument_handler(symbol, handler)

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from the stream of quote ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.
        handler : Callable
            The handler which was receiving the ticks.

        Raises
        ------
        ValueError
            If handler is not of type Callable.
        ValueError
            If a data client has not been registered for the symbol.venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._clients[symbol.venue].unsubscribe_quote_ticks(symbol)
        self._remove_quote_tick_handler(symbol, handler)

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from the stream of trade ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.
        handler : Callable
            The handler which was receiving the ticks.

        Raises
        ------
        ValueError
            If handler is not of type Callable.
        ValueError
            If a data client has not been registered for the symbol.venue.

        """
        Condition.not_none(symbol, "symbol")
        Condition.callable(handler, "handler")
        Condition.is_in(symbol.venue, self._clients, "symbol.venue", "_clients")

        self._clients[symbol.venue].unsubscribe_trade_ticks(symbol)
        self._remove_trade_tick_handler(symbol, handler)

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: callable) except *:
        """
        Unsubscribe from the stream of trade ticks for the given symbol.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.
        handler : Callable
            The handler which was receiving the bars.

        Raises
        ------
        ValueError
            If handler is not of type Callable.
        ValueError
            If a data client has not been registered for the bar_type.symbol.venue.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.callable(handler, "handler")
        Condition.is_in(bar_type.symbol.venue, self._clients, "bar_type.symbol.venue", "_clients")

        self._clients[bar_type.symbol.venue].unsubscribe_bars(bar_type)
        self._remove_bar_handler(bar_type, handler)

# --REGISTRATION METHODS ---------------------------------------------------------------------------

    cpdef void register_data_client(self, DataClient client) except *:
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

    cpdef list registered_venues(self):
        """
        Return the venues registered with the data engine.

        Returns
        -------
        List[Venue]

        """
        return list(self._clients.keys())

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef list subscribed_quote_ticks(self):
        """
        Return the quote tick symbols subscribed to.

        Returns
        -------
        List[Symbol]

        """
        return list(self._quote_tick_handlers.keys())

    cpdef list subscribed_trade_ticks(self):
        """
        Return the trade tick symbols subscribed to.

        Returns
        -------
        List[Symbol]

        """
        return list(self._trade_tick_handlers.keys())

    cpdef list subscribed_bars(self):
        """
        Return the bar types subscribed to.

        Returns
        -------
        List[BarType]

        """
        return list(self._bar_handlers.keys())

    cpdef list subscribed_instruments(self):
        """
        Return the instruments subscribed to.

        Returns
        -------
        List[Symbol]

        """
        return list(self._instrument_handlers.keys())

# -- HANDLER METHODS -------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument) except *:
        """
        Handle the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The received instrument to handle.

        """
        self.cache.add_instrument(instrument)

        self._portfolio.update_instrument(instrument)

        cdef list instrument_handlers = self._instrument_handlers.get(instrument.symbol)
        cdef InstrumentHandler handler
        if instrument_handlers:
            for handler in instrument_handlers:
                handler.handle(instrument)

    cpdef void handle_instruments(self, list instruments) except *:
        """
        Handle the given instruments by handling each instrument individually.

        Parameters
        ----------
        instruments : Instrument
            The received instruments to handle.

        """
        cdef Instrument instrument
        for instrument in instruments:
            self.handle_instrument(instrument)

    cpdef void handle_quote_tick(self, QuoteTick tick, bint send_to_handlers=True) except *:
        """
        Handle the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick to handle.
        send_to_handlers : bool
            If the tick should be sent to any registered handlers.

        """
        Condition.not_none(tick, "tick")

        self.cache.add_quote_tick(tick)

        if not send_to_handlers:
            return

        # Send to portfolio as a priority
        self._portfolio.update_tick(tick)

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._quote_tick_handlers.get(tick.symbol)
        cdef QuoteTickHandler handler
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler.handle(tick)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_quote_ticks(self, list ticks) except *:
        """
        Handle the given ticks by handling each tick individually.

        Parameters
        ----------
        ticks : List[QuoteTick]
            The received ticks to handle.

        """
        Condition.not_none(ticks, "tick")

        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self._log.debug(f"Received <QuoteTick[{length}]> data for {symbol}.")
        else:
            self._log.debug("Received <QuoteTick[]> data with no ticks.")

        cdef int i
        for i in range(length):
            self.handle_quote_tick(ticks[i], send_to_handlers=False)

    cpdef void handle_trade_tick(self, TradeTick tick, bint send_to_handlers=True) except *:
        """
        Handle the given tick.

        Parameters
        ----------
        tick : TradeTick
            The received tick to handle.
        send_to_handlers : bool
            If the tick should be sent to any registered handlers.

        """
        Condition.not_none(tick, "tick")

        cdef Symbol symbol = tick.symbol

        self.cache.add_trade_tick(tick)

        if not send_to_handlers:
            return

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._trade_tick_handlers.get(tick.symbol)
        cdef TradeTickHandler handler
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler.handle(tick)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_trade_ticks(self, list ticks) except *:
        """
        Handle the given ticks by handling each tick individually.

        Parameters
        ----------
        ticks : List[TradeTick]
            The received ticks to handle.

        """
        Condition.not_none(ticks, "ticks")

        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self._log.debug(f"Received <TradeTick[{length}]> data for {symbol}.")
        else:
            self._log.debug("Received <TradeTick[]> data with no ticks.")

        cdef int i
        for i in range(length):
            self.handle_trade_tick(ticks[i], send_to_handlers=False)

    cpdef void handle_bar(self, BarType bar_type, Bar bar, bint send_to_handlers=True) except *:
        """
        Handle the given bar type and bar.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the received bar.
        bar : Bar
            The received bar to handle.
        send_to_handlers : bool
            If the bar should be sent to any registered handlers.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bar, "bar")

        self.cache.add_bar(bar_type, bar)

        if not send_to_handlers:
            return

        # Send to all registered bar handlers for that bar type
        cdef list bar_handlers = self._bar_handlers.get(bar_type)
        cdef BarHandler handler
        if bar_handlers is not None:
            for handler in bar_handlers:
                handler.handle(bar_type, bar)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_bars(self, BarType bar_type, list bars: List[Bar]) except *:
        """
        Handle the given bar type and bars by handling each bar individually.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the received bars.
        bars : List[Bar]
            The received bars to handle.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)

        self._log.debug(f"Received <Bar[{length}]> data for {bar_type}.")

        if length > 0 and bars[0].timestamp > bars[length - 1].timestamp:
            raise RuntimeError("Cannot handle <Bar[]> data (incorrectly sorted).")

        cdef int i
        for i in range(length):
            self.handle_bar(bar_type, bars[i], send_to_handlers=False)

# ------------------------------------------------------------------------------------------------ #

    cdef void _internal_update_instruments(self, list instruments) except *:
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self.handle_instrument(instrument)

    cdef void _start_generating_bars(self, BarType bar_type, handler: callable) except *:
        if bar_type not in self._bar_aggregators:
            if bar_type.spec.aggregation == BarAggregation.TICK:
                aggregator = TickBarAggregator(bar_type, self.handle_bar, self._log.get_logger())
            elif bar_type.is_time_aggregated():
                aggregator = TimeBarAggregator(
                    bar_type=bar_type,
                    handler=self.handle_bar,
                    use_previous_close=self._use_previous_close,
                    clock=self._clock,
                    logger=self._log.get_logger(),
                )

                bulk_updater = BulkTimeBarUpdater(aggregator)

                self.request_quote_ticks(
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
                self.subscribe_trade_ticks(bar_type.symbol, aggregator.handle_trade_tick)
            else:
                self.subscribe_quote_ticks(bar_type.symbol, aggregator.handle_quote_tick)

        self._add_bar_handler(bar_type, handler)  # Add handler last

    cdef void _stop_generating_bars(self, BarType bar_type, handler: callable) except *:
        if bar_type in self._bar_handlers:  # Remove handler first
            self._remove_bar_handler(bar_type, handler)
            if bar_type not in self._bar_handlers:  # No longer any handlers for that bar type
                aggregator = self._bar_aggregators[bar_type]
                if isinstance(aggregator, TimeBarAggregator):
                    aggregator.stop()

                # Unsubscribe from update ticks
                if bar_type.spec.price_type == PriceType.LAST:
                    self.unsubscribe_trade_ticks(bar_type.symbol, aggregator.handle_trade_tick)
                else:
                    self.unsubscribe_quote_ticks(bar_type.symbol, aggregator.handle_quote_tick)

                del self._bar_aggregators[bar_type]

    cdef void _add_quote_tick_handler(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to QuoteTick data for the given symbol and handler.
        """
        if symbol not in self._quote_tick_handlers:
            # Setup handlers
            self._quote_tick_handlers[symbol] = []                        # type: [QuoteTickHandler]
            self._log.info(f"Subscribed to {symbol} <QuoteTick> data.")

        # Add handler for subscriber
        tick_handler = QuoteTickHandler(handler)
        if tick_handler not in self._quote_tick_handlers[symbol]:
            self._quote_tick_handlers[symbol].append(tick_handler)
            self._log.debug(f"Added {tick_handler} for {symbol} <QuoteTick> data.")
        else:
            self._log.error(f"Cannot add {tick_handler} for {symbol} <QuoteTick> data"
                            f"(duplicate handler found).")

    cdef void _add_trade_tick_handler(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to <TradeTick> data for the given symbol and handler.
        """
        if symbol not in self._trade_tick_handlers:
            # Setup handlers
            self._trade_tick_handlers[symbol] = []                        # type: [TradeTickHandler]
            self._log.info(f"Subscribed to {symbol} <TradeTick> data.")

        # Add handler for subscriber
        tick_handler = TradeTickHandler(handler)
        if tick_handler not in self._trade_tick_handlers[symbol]:
            self._trade_tick_handlers[symbol].append(tick_handler)
            self._log.debug(f"Added {tick_handler} for {symbol} <TradeTick> data.")
        else:
            self._log.error(f"Cannot add {tick_handler} for {symbol} <TradeTick> data"
                            f"(duplicate handler found).")

    cdef void _add_bar_handler(self, BarType bar_type, handler: callable) except *:
        """
        Subscribe to bar data for the given bar type and handler.
        """
        if bar_type not in self._bar_handlers:
            # Setup handlers
            self._bar_handlers[bar_type] = []                        # type: [BarHandler]
            self._log.info(f"Subscribed to {bar_type} <Bar> data.")

        # Add handler for subscriber
        bar_handler = BarHandler(handler)
        if bar_handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(bar_handler)
            self._log.debug(f"Added {bar_handler} for {bar_type} <Bar> data.")
        else:
            self._log.error(f"Cannot add {bar_handler} for {bar_type} <Bar> data "
                            f"(duplicate handler found).")

    cdef void _add_instrument_handler(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to instrument data for the given symbol and handler.
        """
        if symbol not in self._instrument_handlers:
            self._instrument_handlers[symbol] = []  # type: [InstrumentHandler]
            self._log.info(f"Subscribed to {symbol} <Instrument> data.")

        instrument_handler = InstrumentHandler(handler)
        if instrument_handler not in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].append(instrument_handler)
            self._log.debug(f"Added {instrument_handler} for {symbol} <Instrument> data.")
        else:
            self._log.error(f"Cannot add {instrument_handler} for {symbol} <Instrument> data"
                            f"(duplicate handler found).")

    cdef void _remove_quote_tick_handler(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from tick data for the given symbol and handler.
        """
        if symbol not in self._quote_tick_handlers:
            self._log.debug(f"Cannot remove handler for {symbol} <QuoteTick> data "
                            f"(no handlers found).")
            return

        # Remove subscribers handler
        tick_handler = QuoteTickHandler(handler)
        if tick_handler in self._quote_tick_handlers[symbol]:
            self._quote_tick_handlers[symbol].remove(tick_handler)
            self._log.debug(f"Removed handler {tick_handler} for {symbol} <QuoteTick> data.")
        else:
            self._log.error(f"Cannot remove {tick_handler} for {symbol} <QuoteTick> data "
                            f"(no matching handler found).")

        if not self._quote_tick_handlers[symbol]:
            del self._quote_tick_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <QuoteTick> data.")

    cdef void _remove_trade_tick_handler(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from tick data for the given symbol and handler.
        """
        if symbol not in self._trade_tick_handlers:
            self._log.debug(f"Cannot remove handler for {symbol} <TradeTick> data "
                            f"(no handlers found).")
            return

        # Remove subscribers handler
        tick_handler = TradeTickHandler(handler)
        if tick_handler in self._trade_tick_handlers[symbol]:
            self._trade_tick_handlers[symbol].remove(tick_handler)
            self._log.debug(f"Removed handler {tick_handler} for {symbol} <TradeTick> data.")
        else:
            self._log.error(f"Cannot remove {tick_handler} for {symbol} <TradeTick> data"
                            f"(no matching handler found).")

        if not self._trade_tick_handlers[symbol]:
            del self._trade_tick_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <TradeTick> data.")

    cdef void _remove_bar_handler(self, BarType bar_type, handler: callable) except *:
        """
        Unsubscribe from bar data for the given bar type and handler.
        """
        if bar_type not in self._bar_handlers:
            self._log.debug(f"Cannot remove handler for {bar_type} <Bar> data "
                            f"(no handlers found).")
            return

        # Remove subscribers handler
        cdef BarHandler bar_handler = BarHandler(handler)
        if bar_handler in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].remove(bar_handler)
            self._log.debug(f"Removed handler {bar_handler} for {bar_type} <Bar> data.")
        else:
            self._log.debug(f"Cannot remove {bar_handler} for {bar_type} <Bar> data"
                            f"(no matching handler found).")

        if not self._bar_handlers[bar_type]:
            del self._bar_handlers[bar_type]
            self._log.info(f"Unsubscribed from {bar_type} <Bar> data.")

    cdef void _remove_instrument_handler(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from tick data for the given symbol and handler.
        """
        if symbol not in self._instrument_handlers:
            self._log.debug(f"Cannot remove handler for {symbol} <Instrument> data "
                            f"(no handlers found).")
            return

        cdef InstrumentHandler instrument_handler = InstrumentHandler(handler)
        if instrument_handler in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].remove(instrument_handler)
            self._log.debug(f"Removed handler {instrument_handler} for {symbol} <Instrument> data.")
        else:
            self._log.debug(f"Cannot remove {instrument_handler} for {symbol} <Instrument> data "
                            f"(no matching handler found).")

        if not self._instrument_handlers[symbol]:
            del self._instrument_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} <Instrument> data.")

    cdef void _bulk_build_tick_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback
    ) except *:
        # Bulk build tick bars
        cdef int ticks_to_order = bar_type.spec.step * limit

        cdef BulkTickBarBuilder bar_builder = BulkTickBarBuilder(
            bar_type,
            self._log.get_logger(),
            callback
        )

        self.request_quote_ticks(
            bar_type.symbol,
            from_datetime,
            to_datetime,
            ticks_to_order,
            bar_builder.receive
        )

    cdef void _reset(self) except *:
        # Reset the class to its initial state
        self._clock.cancel_all_timers()
        self._instrument_handlers.clear()
        self._quote_tick_handlers.clear()
        self._bar_aggregators.clear()
        self._bar_handlers.clear()

        self._log.debug("Reset.")


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
        Initialize a new instance of the BulkTickBarBuilder class.

        Parameters
        ----------
        bar_type : BarType
            The bar_type to build.
        logger : Logger
            The logger for the bar aggregator.
        callback : Callable
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

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void receive(self, list ticks) except *:
        """
        Receives the bulk list of ticks and builds aggregated tick
        bars. Then sends the bar type and bars list on to the registered callback.

        Parameters
        ----------
        ticks : List[Tick]
            The bulk ticks for aggregation into tick bars.

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
        Initialize a new instance of the BulkTimeBarUpdater class.

        Parameters
        ----------
        aggregator : TimeBarAggregator
            The time bar aggregator to update.

        """
        self.aggregator = aggregator
        self.start_time = self.aggregator.next_close - self.aggregator.interval

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void receive(self, list ticks) except *:
        """
        Receives the bulk list of ticks and updates the aggregator.

        Parameters
        ----------
        ticks : List[Tick]
            The bulk ticks for updating the aggregator.

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
