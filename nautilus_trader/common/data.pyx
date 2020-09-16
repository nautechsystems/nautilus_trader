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

# cython: boundscheck=False
# cython: wraparound=False

from cpython.datetime cimport datetime

from collections import deque

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.handlers cimport BarHandler
from nautilus_trader.common.handlers cimport InstrumentHandler
from nautilus_trader.common.handlers cimport QuoteTickHandler
from nautilus_trader.common.handlers cimport TradeTickHandler
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.market cimport BarAggregator
from nautilus_trader.common.market cimport TickBarAggregator
from nautilus_trader.common.market cimport TimeBarAggregator
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef list _TIME_BARS = [
    BarAggregation.SECOND,
    BarAggregation.MINUTE,
    BarAggregation.HOUR,
    BarAggregation.DAY,
]


cdef class DataClient:
    """
    The base class for all data clients.
    """

    def __init__(self,
                 int tick_capacity,
                 int bar_capacity,
                 bint use_previous_close,
                 Clock clock not None,
                 UUIDFactory uuid_factory not None,
                 Logger logger not None):
        """
        Initialize a new instance of the DataClient class.

        :param tick_capacity: The length for the internal ticks deque (> 0).
        :param bar_capacity: The length for the internal bars deque (> 0).
        :param use_previous_close: If bar aggregators should use the previous close.
        :param clock: The clock for the component.
        :param uuid_factory: The UUID factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the tick_capacity is not positive (> 0).
        :raises ValueError: If the bar_capacity is not positive (> 0).
        """
        Condition.positive_int(tick_capacity, "tick_capacity")
        Condition.positive_int(bar_capacity, "bar_capacity")

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter("DataClient", logger)
        self._exchange_calculator = ExchangeRateCalculator()
        self._quote_ticks = {}          # type: {Symbol, [QuoteTick]}
        self._trade_ticks = {}          # type: {Symbol, [TradeTick]}
        self._quote_tick_handlers = {}  # type: {Symbol, [QuoteTickHandler]}
        self._trade_tick_handlers = {}  # type: {Symbol, [TradeTickHandler]}
        self._bars = {}                 # type: {BarType, [Bar]}
        self._bar_aggregators = {}      # type: {BarType, BarAggregator}
        self._bar_handlers = {}         # type: {BarType, [BarHandler]}
        self._instrument_handlers = {}  # type: {Symbol, [InstrumentHandler]}
        self._instruments = {}          # type: {Symbol, Instrument}

        self.tick_capacity = tick_capacity
        self.bar_capacity = bar_capacity
        self.use_previous_close = True

        self._log.info("Initialized.")


# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #

    cpdef void connect(self) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void disconnect(self) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void dispose(self) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback: callable) except *:  # noqa (E125)
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_trade_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable) except *:  # noqa (E125)
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback: callable) except *:  # noqa (E125)
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_instrument(self, Symbol symbol, callback: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_instruments(self, Venue venue, callback: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_quote_ticks(self, Symbol symbol, handler: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_trade_ticks(self, Symbol symbol, handler: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_bars(self, BarType bar_type, handler: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_instrument(self, Symbol symbol, handler: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol, handler: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol, handler: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: callable) except *:
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_instruments(self, Venue venue) except *:
        raise NotImplementedError("method must be implemented in the subclass")


# --REGISTRATION METHODS ------------------------------------------------------------------------- #

    cpdef void register_strategy(self, TradingStrategy strategy) except *:
        """
        Register the given trade strategy with the data client.

        :param strategy: The strategy to register.
        """
        Condition.not_none(strategy, "strategy")

        strategy.register_data_client(self)

        self._log.info(f"Registered {strategy}.")


# -- HANDLER METHODS ----------------------------------------------------------------------------- #

    cpdef void handle_quote_tick(self, QuoteTick tick, bint send_to_handlers=True) except *:
        cdef Symbol symbol = tick.symbol

        # Update ticks and spreads
        ticks = self._quote_ticks.get(symbol)

        if ticks is None:
            # The symbol was not registered
            ticks = deque(maxlen=self.tick_capacity)
            self._quote_ticks[symbol] = ticks

        cdef int ticks_length = len(ticks)
        if ticks_length > 0 and tick.timestamp <= ticks[0].timestamp:
            if ticks_length < self.tick_capacity and tick.timestamp > ticks[ticks_length - 1].timestamp:
                ticks.append(tick)
            return  # Tick previously handled

        ticks.appendleft(tick)

        if not send_to_handlers:
            return

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._quote_tick_handlers.get(symbol)
        cdef QuoteTickHandler handler
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler.handle(tick)

    cpdef void handle_quote_ticks(self, list ticks) except *:
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
        cdef Symbol symbol = tick.symbol

        # Update ticks
        ticks = self._trade_ticks.get(symbol)

        if ticks is None:
            # The symbol was not registered
            ticks = deque(maxlen=self.tick_capacity)
            self._trade_ticks[symbol] = ticks

        cdef int ticks_length = len(ticks)
        if ticks_length > 0 and tick.timestamp <= ticks[0].timestamp:
            if ticks_length < self.tick_capacity and tick.timestamp > ticks[ticks_length - 1].timestamp:
                ticks.append(tick)
            return  # Tick previously handled

        ticks.appendleft(tick)

        if not send_to_handlers:
            return

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._trade_tick_handlers.get(symbol)
        cdef TradeTickHandler handler
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler.handle(tick)

    cpdef void handle_trade_ticks(self, list ticks) except *:
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
        # Update ticks
        bars = self._bars.get(bar_type)

        if bars is None:
            # The bar type was not registered
            bars = deque(maxlen=self.bar_capacity)
            self._bars[bar_type] = bars

        cdef int bars_length = len(bars)
        if bars_length > 0 and bar.timestamp <= bars[0].timestamp:
            if bars_length < self.bar_capacity and bar.timestamp > bars[bars_length - 1].timestamp:
                bars.append(bar)
            return  # Bar previously handled

        bars.appendleft(bar)

        if not send_to_handlers:
            return

        # Send to all registered bar handlers for that bar type
        cdef list bar_handlers = self._bar_handlers.get(bar_type)
        cdef BarHandler handler
        if bar_handlers is not None:
            for handler in bar_handlers:
                handler.handle(bar_type, bar)

    cpdef void handle_bars(self, BarType bar_type, list bars) except *:
        """
        System method. Handle the given bar type and bars by handling
        each bar individually.
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

    cpdef void handle_instrument(self, Instrument instrument) except *:
        # Handle the given instrument by sending it to all instrument handlers for that symbol
        self._instruments[instrument.symbol] = instrument
        self._log.info(f"Updated instrument {instrument.symbol}")

        cdef list instrument_handlers = self._instrument_handlers.get(instrument.symbol)
        cdef InstrumentHandler handler
        if instrument_handlers is not None:
            for handler in instrument_handlers:
                handler.handle(instrument)

    cpdef void handle_instruments(self, list instruments) except *:
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self.handle_instrument(instrument)


# -- QUERY METHODS ------------------------------------------------------------------------------- #

    cpdef list subscribed_quote_ticks(self):
        """
        Return the list of quote tick symbols subscribed to.

        :return List[Symbol].
        """
        return list(self._quote_tick_handlers.keys())

    cpdef list subscribed_trade_ticks(self):
        """
        Return the list of trade tick symbols subscribed to.

        :return List[Symbol].
        """
        return list(self._trade_tick_handlers.keys())

    cpdef list subscribed_bars(self):
        """
        Return the list of bar types subscribed to.

        :return List[BarType].
        """
        return list(self._bar_handlers.keys())

    cpdef list subscribed_instruments(self):
        """
        Return the list of instruments subscribed to.

        :return List[Symbol].
        """
        return list(self._instrument_handlers.keys())

    cpdef list instrument_symbols(self):
        """
        Return all instrument symbols held by the data client.

        :return List[Symbol].
        """
        return list(self._instruments.keys())

    cpdef dict get_instruments(self):
        """
        Return a dictionary of all instruments for the given venue.

        :return Dict[Symbol, Instrument].
        """
        return {instrument.symbol: instrument for instrument in self._instruments}

    cpdef Instrument get_instrument(self, Symbol symbol):
        """
        Return the instrument corresponding to the given symbol (if found).

        :param symbol: The symbol of the instrument to return.
        :raises ValueError: If the instrument is not found.
        :return Instrument.
        """
        Condition.is_in(symbol, self._instruments, "symbol", "instruments")

        return self._instruments[symbol]

    cpdef bint has_quote_ticks(self, Symbol symbol):
        """
        Return a value indicating whether the data client has quote ticks for
        the given symbol.

        :param symbol: The symbol for the ticks.
        :return bool.
        """
        Condition.not_none(symbol, "symbol")

        return symbol in self._quote_ticks and len(self._quote_ticks[symbol]) > 0

    cpdef bint has_trade_ticks(self, Symbol symbol):
        """
        Return a value indicating whether the data client has trade ticks for
        the given symbol.

        :param symbol: The symbol for the ticks.
        :return bool.
        """
        Condition.not_none(symbol, "symbol")

        return symbol in self._trade_ticks and len(self._trade_ticks[symbol]) > 0

    cpdef bint has_bars(self, BarType bar_type):
        """
        Return a value indicating whether the data client has bars for the given
        bar type.

        :param bar_type: The bar type for the bars.
        :return bool.
        """
        Condition.not_none(bar_type, "bar_type")

        return bar_type in self._bars and len(self._bars[bar_type]) > 0

    cpdef int quote_tick_count(self, Symbol symbol):
        """
        Return the count of quote ticks for the given symbol.

        :param symbol: The symbol for the ticks.
        :return int.
        """
        Condition.not_none(symbol, "symbol")

        return len(self._quote_ticks[symbol]) if symbol in self._quote_ticks else 0

    cpdef int trade_tick_count(self, Symbol symbol):
        """
        Return the count of trade ticks for the given symbol.

        :param symbol: The symbol for the ticks.
        :return int.
        """
        Condition.not_none(symbol, "symbol")

        return len(self._trade_ticks[symbol]) if symbol in self._trade_ticks else 0

    cpdef int bar_count(self, BarType bar_type):
        """
        Return the count of bars for the given bar type.

        :param bar_type: The bar type to count.
        :return int.
        """
        Condition.not_none(bar_type, "bar_type")

        return len(self._bars[bar_type]) if bar_type in self._bars else 0

    cpdef list quote_ticks(self, Symbol symbol):
        """
        Return the quote ticks for the given symbol (returns a shallow copy of the internal deque).

        :param symbol: The symbol for the ticks to get.
        :return List[QuoteTick].
        """
        Condition.not_none(symbol, "symbol")
        Condition.is_in(symbol, self._quote_ticks, "symbol", "ticks")

        return list(self._quote_ticks[symbol])

    cpdef list trade_ticks(self, Symbol symbol):
        """
        Return the trade ticks for the given symbol (returns a shallow copy of the internal deque).

        :param symbol: The symbol for the ticks to get.
        :return List[TradeTick].
        """
        Condition.not_none(symbol, "symbol")
        Condition.is_in(symbol, self._trade_ticks, "symbol", "ticks")

        return list(self._trade_ticks[symbol])

    cpdef list bars(self, BarType bar_type):
        """
        Return the bars for the given bar type (returns a shallow copy of the internal deque).

        :param bar_type: The bar type to get.
        :return List[Bar].
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.is_in(bar_type, self._bars, "bar_type", "bars")

        return list(self._bars[bar_type])

    cpdef QuoteTick quote_tick(self, Symbol symbol, int index=0):
        """
        Return the quote tick for the given symbol at the given index or last if no index specified.

        :param symbol: The symbol for the tick to get.
        :param index: The optional index for the tick to get.
        :return QuoteTick.
        :raises ValueError: If the data clients quote ticks does not contain the symbol.
        :raises IndexError: If the tick index is out of range.
        """
        Condition.not_none(symbol, "symbol")
        Condition.is_in(symbol, self._quote_ticks, "symbol", "ticks")

        return self._quote_ticks[symbol][index]

    cpdef TradeTick trade_tick(self, Symbol symbol, int index=0):
        """
        Return the trade tick for the given symbol at the given index or last if no index specified.

        :param symbol: The symbol for the tick to get.
        :param index: The optional index for the tick to get.
        :return TradeTick.
        :raises ValueError: If the data clients trade ticks does not contain the symbol.
        :raises IndexError: If the tick index is out of range.
        """
        Condition.not_none(symbol, "symbol")
        Condition.is_in(symbol, self._trade_ticks, "symbol", "ticks")

        return self._trade_ticks[symbol][index]

    cpdef Bar bar(self, BarType bar_type, int index=0):
        """
        Return the bar for the given bar type at the given index or last if no index specified.

        :param bar_type: The bar type to get.
        :param index: The optional index for the bar to get.
        :return Bar.
        :raises ValueError: If the data clients bars does not contain the bar type.
        :raises IndexError: If the bar index is out of range.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.is_in(bar_type, self._bars, "bar_type", "bars")

        return self._bars[bar_type][index]

    cpdef double get_exchange_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            PriceType price_type=PriceType.MID):
        """
        Return the calculated exchange rate for the given currencies.

        :param from_currency: The currency to convert from.
        :param to_currency: The currency to convert to.
        :param price_type: The price type for the exchange rate (default=MID).
        :return float.
        :raises ValueError: If the price_type is UNDEFINED or LAST.
        """
        cdef Symbol symbol
        cdef dict bid_rates = {symbol.code: ticks[0].bid.as_double() for symbol, ticks in self._quote_ticks.items() if len(ticks) > 0}
        cdef dict ask_rates = {symbol.code: ticks[0].ask.as_double() for symbol, ticks in self._quote_ticks.items() if len(ticks) > 0}

        return self._exchange_calculator.get_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type,
            bid_rates=bid_rates,
            ask_rates=ask_rates)

    cdef void _start_generating_bars(self, BarType bar_type, handler: callable) except *:
        if bar_type not in self._bar_aggregators:
            if bar_type.spec.aggregation == BarAggregation.TICK:
                aggregator = TickBarAggregator(bar_type, self.handle_bar, self._log.get_logger())
            elif bar_type.spec.aggregation in _TIME_BARS:
                aggregator = TimeBarAggregator(
                    bar_type=bar_type,
                    handler=self.handle_bar,
                    use_previous_close=self.use_previous_close,
                    clock=self._clock,
                    logger=self._log.get_logger())

                bulk_updater = BulkTimeBarUpdater(aggregator)

                self.request_quote_ticks(
                    symbol=bar_type.symbol,
                    from_datetime=aggregator.get_start_time(),
                    to_datetime=None,  # Max
                    limit=0,  # No limit
                    callback=bulk_updater.receive)

            # Add aggregator and subscribe to QuoteTick updates
            self._bar_aggregators[bar_type] = aggregator
            self._log.debug(f"Added {aggregator} for {bar_type} bars.")

            self.subscribe_quote_ticks(bar_type.symbol, aggregator.update)

        self._add_bar_handler(bar_type, handler)  # Add handler last

    cdef void _stop_generating_bars(self, BarType bar_type, handler: callable) except *:
        if bar_type in self._bar_handlers:  # Remove handler first
            self._remove_bar_handler(bar_type, handler)
            if bar_type not in self._bar_handlers:  # No longer any handlers for that bar type
                aggregator = self._bar_aggregators[bar_type]
                if isinstance(aggregator, TimeBarAggregator):
                    aggregator.stop()

                # Unsubscribe from update ticks
                self.unsubscribe_quote_ticks(bar_type.symbol, aggregator.update)
                del self._bar_aggregators[bar_type]

    cdef void _add_quote_tick_handler(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to QuoteTick data for the given symbol and handler.
        """
        if symbol not in self._quote_tick_handlers:
            # Setup handlers
            self._quote_ticks[symbol] = deque(maxlen=self.tick_capacity)  # type: [QuoteTick]
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
            self._trade_ticks[symbol] = deque(maxlen=self.tick_capacity)  # type: [TradeTick]
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
            self._bars[bar_type] = deque(maxlen=self.tick_capacity)  # type: [Bar]
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
            callback) except *:
        # Bulk build tick bars
        cdef int ticks_to_order = bar_type.spec.step * limit

        cdef BulkTickBarBuilder bar_builder = BulkTickBarBuilder(
            bar_type,
            self._log.get_logger(),
            callback)

        self.request_quote_ticks(
            bar_type.symbol,
            from_datetime,
            to_datetime,
            ticks_to_order,
            bar_builder.receive)

    cdef void _reset(self) except *:
        # Reset the class to its initial state
        self._clock.cancel_all_timers()
        self._quote_ticks.clear()
        self._quote_tick_handlers.clear()
        self._bar_aggregators.clear()
        self._bar_handlers.clear()
        self._instrument_handlers.clear()
        self._instruments.clear()

        self._log.debug("Reset.")

# ------------------------------------------------------------------------------------------------ #


cdef class BulkTickBarBuilder:
    """
    Provides a temporary builder for tick bars from a bulk tick order.
    """
    cdef TickBarAggregator aggregator
    cdef object callback
    cdef list bars

    def __init__(self,
                 BarType bar_type not None,
                 Logger logger not None,
                 callback not None):
        """
        Initialize a new instance of the BulkTickBarBuilder class.

        :param bar_type: The bar_type to build.
        :param logger: The logger for the bar aggregator.
        :param callback: The callback to send the built bars to.
        :raises ValueError: If the callback is not type callable.
        """
        Condition.callable(callback, "callback")

        self.bars = []
        self.aggregator = TickBarAggregator(bar_type, self._add_bar, logger)
        self.callback = callback

    cpdef void receive(self, list ticks) except *:
        """
        Receives the bulk list of ticks and builds aggregated tick
        bars. Then sends the bar type and bars list on to the registered callback.

        :param ticks: The bulk ticks for aggregation into tick bars.
        """
        cdef int i
        for i in range(len(ticks)):
            self.aggregator.update(ticks[i])

        self.callback(self.aggregator.bar_type, self.bars)

    cpdef void _add_bar(self, BarType bar_type, Bar bar) except *:
        self.bars.append(bar)


cdef class BulkTimeBarUpdater:
    """
    Provides a temporary updater for time bars from a bulk tick order.
    """
    cdef TimeBarAggregator aggregator
    cdef datetime start_time

    def __init__(self, TimeBarAggregator aggregator not None):
        """
        Initialize a new instance of the BulkTimeBarUpdater class.

        :param aggregator: The time bar aggregator to update.
        """
        self.aggregator = aggregator
        self.start_time = self.aggregator.next_close - self.aggregator.interval

    cpdef void receive(self, list ticks) except *:
        """
        Receives the bulk list of ticks and updates the aggregator.

        :param ticks: The bulk ticks for updating the aggregator.
        """
        cdef int i
        for i in range(len(ticks)):
            if ticks[i].timestamp < self.start_time:
                continue  # Price not applicable to this bar

            self.aggregator.update(ticks[i])
