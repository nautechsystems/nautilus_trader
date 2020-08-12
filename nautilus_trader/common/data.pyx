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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport fast_mean_iterated
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.identifiers cimport Symbol, Venue
from nautilus_trader.model.objects cimport Tick, BarType, Bar, Instrument
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.common.logging cimport Logger, LoggerAdapter
from nautilus_trader.common.handlers cimport TickHandler, BarHandler, InstrumentHandler
from nautilus_trader.common.market cimport BarAggregator, TickBarAggregator, TimeBarAggregator
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef list _TIME_BARS = [
    BarStructure.SECOND,
    BarStructure.MINUTE,
    BarStructure.HOUR,
    BarStructure.DAY,
]


cdef class DataClient:
    """
    The base class for all data clients.
    """

    def __init__(self,
                 int tick_capacity,
                 Clock clock not None,
                 UUIDFactory uuid_factory not None,
                 Logger logger not None):
        """
        Initializes a new instance of the DataClient class.

        :param tick_capacity: The length for the internal bars deque (> 0).
        :param clock: The clock for the component.
        :param uuid_factory: The UUID factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the tick_capacity is not positive (> 0).
        """
        Condition.positive_int(tick_capacity, 'tick_capacity')

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._ticks = {}                # type: {Symbol, Tick}
        self._tick_handlers = {}        # type: {Symbol, [TickHandler]}
        self._spreads = {}              # type: {Symbol, [float]}
        self._spreads_average = {}      # type: {Symbol, float}
        self._bar_aggregators = {}      # type: {BarType, BarAggregator}
        self._bar_handlers = {}         # type: {BarType, [BarHandler]}
        self._instrument_handlers = {}  # type: {Symbol, [InstrumentHandler]}
        self._instruments = {}          # type: {Symbol, Instrument}
        self._exchange_calculator = ExchangeRateCalculator()

        self.tick_capacity = tick_capacity
        self.use_previous_close = True

        self._log.info("Initialized.")

# -- ABSTRACT METHODS ------------------------------------------------------------------------------
    cpdef void connect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void reset(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void dispose(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback: callable) except *:  # noqa (E125)
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback: callable) except *:  # noqa (E125)
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_instrument(self, Symbol symbol, callback: callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_instruments(self, Venue venue, callback: callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_ticks(self, Symbol symbol, handler: callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_bars(self, BarType bar_type, handler: callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_instrument(self, Symbol symbol, handler: callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void update_instruments(self, Venue venue) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")
# ------------------------------------------------------------------------------------------------ #

    cpdef datetime time_now(self):
        """
        Return the current time of the data client.
        
        :return datetime.
        """
        return self._clock.time_now()

    cpdef list subscribed_ticks(self):
        """
        Return the list of tick symbols subscribed to.
        
        :return List[Symbol].
        """
        return list(self._tick_handlers.keys())

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

    cpdef void register_strategy(self, TradingStrategy strategy) except *:
        """
        Register the given trade strategy with the data client.

        :param strategy: The strategy to register.
        """
        Condition.not_none(strategy, 'strategy')

        strategy.register_data_client(self)

        self._log.debug(f"Registered {strategy}.")

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
        Condition.is_in(symbol, self._instruments, 'symbol', 'instruments')

        return self._instruments[symbol]

    cpdef bint has_ticks(self, Symbol symbol):
        """
        Return a value indicating whether the data client has ticks for the given symbol.
        
        :param symbol: The symbol of the ticks.
        :return bool.
        """
        Condition.not_none(symbol, 'symbol')

        return symbol in self._ticks and len(self._ticks[symbol]) > 0

    cpdef double spread(self, Symbol symbol):
        """
        Return the current spread for the given symbol.
        
        :param symbol: The symbol for the spread to get.
        :return float.
        :raises ValueError: If the data clients ticks does not contain the symbol.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.is_in(symbol, self._spreads, 'symbol', 'spreads')

        return self._spreads[symbol][0]

    cpdef double spread_average(self, Symbol symbol):
        """
        Return the average spread of the ticks from the given symbol.
        
        :param symbol: The symbol for the average spread to get.
        :return float.
        :raises ValueError: If the data clients ticks does not contain the symbol.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.is_in(symbol, self._spreads_average, 'symbol', 'spreads_average')

        return self._spreads_average.get(symbol, 0.0)

    cpdef double get_exchange_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            PriceType price_type=PriceType.MID):
        """
        Return the calculated exchange rate for the given currencies.

        :param from_currency: The currency to convert from.
        :param to_currency: The currency to convert to.
        :param price_type: The quote type for the exchange rate (default=MID).
        :return float.
        :raises ValueError: If the quote type is LAST.
        """
        cdef Symbol symbol
        cdef dict bid_rates = {symbol.code: ticks[0].bid.as_double() for symbol, ticks in self._ticks.items() if len(ticks) > 0}
        cdef dict ask_rates = {symbol.code: ticks[0].ask.as_double() for symbol, ticks in self._ticks.items() if len(ticks) > 0}

        return self._exchange_calculator.get_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type,
            bid_rates=bid_rates,
            ask_rates=ask_rates)

    cpdef void set_use_previous_close(self, bint value) except *:
        """
        Set the value of the use_previous_close flag. Determines whether bar
        aggregators will use the close of the previous bar as the open of the
        next bar.
        
        :param value: The value to set.
        """
        self.use_previous_close = value

    cdef void _start_generating_bars(self, BarType bar_type, handler: callable) except *:
        cdef BulkTimeBarUpdater bulk_updater
        if bar_type not in self._bar_aggregators:
            if bar_type.specification.structure == BarStructure.TICK:
                aggregator = TickBarAggregator(bar_type, self._handle_bar, self._log.get_logger())
            elif bar_type.specification.structure in _TIME_BARS:
                aggregator = TimeBarAggregator(
                    bar_type=bar_type,
                    handler=self._handle_bar,
                    use_previous_close=self.use_previous_close,
                    clock=self._clock,
                    logger=self._log.get_logger())

                bulk_updater = BulkTimeBarUpdater(aggregator)

                self.request_ticks(
                    symbol=bar_type.symbol,
                    from_datetime=aggregator.get_start_time(),
                    to_datetime=None,  # Max
                    limit=0,  # No limit
                    callback=bulk_updater.receive)

            self._bar_aggregators[bar_type] = aggregator
            self.subscribe_ticks(bar_type.symbol, aggregator.update)

        self._add_bar_handler(bar_type, handler)  # Add handler last

    cdef void _stop_generating_bars(self, BarType bar_type, handler: callable) except *:
        if bar_type in self._bar_handlers:  # Remove handler first
            self._remove_bar_handler(bar_type, handler)
            if bar_type not in self._bar_handlers:  # No longer any handlers for that bar type
                aggregator = self._bar_aggregators[bar_type]
                if isinstance(aggregator, TimeBarAggregator):
                    aggregator.stop()
                self.unsubscribe_ticks(bar_type.symbol, aggregator.update)
                del self._bar_aggregators[bar_type]

    cdef void _add_tick_handler(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to tick data for the given symbol and handler.
        """
        if symbol not in self._tick_handlers:
            self._ticks[symbol] = deque(maxlen=self.tick_capacity)
            self._tick_handlers[symbol] = []  # type: [TickHandler]
            self._spreads[symbol] = deque(maxlen=self.tick_capacity)
            self._log.info(f"Subscribed to {symbol} tick data.")

        cdef TickHandler tick_handler = TickHandler(handler)
        if tick_handler not in self._tick_handlers[symbol]:
            self._tick_handlers[symbol].append(tick_handler)
            self._log.debug(f"Added {tick_handler} for {symbol} tick data.")
        else:
            self._log.error(f"Cannot add {tick_handler} for {symbol} tick data"
                            f"(duplicate handler found).")

    cdef void _add_bar_handler(self, BarType bar_type, handler: callable) except *:
        """
        Subscribe to bar data for the given bar type and handler.
        """
        if bar_type not in self._bar_handlers:
            self._bar_handlers[bar_type] = []  # type: [BarHandler]
            self._log.info(f"Subscribed to {bar_type} bar data.")

        cdef BarHandler bar_handler = BarHandler(handler)
        if bar_handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(bar_handler)
            self._log.debug(f"Added {bar_handler} for {bar_type} bar data.")
        else:
            self._log.error(f"Cannot add {bar_handler} for {bar_type} bar data "
                            f"(duplicate handler found).")

    cdef void _add_instrument_handler(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to instrument data for the given symbol and handler.
        """
        if symbol not in self._instrument_handlers:
            self._instrument_handlers[symbol] = []  # type: [InstrumentHandler]
            self._log.info(f"Subscribed to {symbol} instrument data.")

        cdef InstrumentHandler instrument_handler = InstrumentHandler(handler)
        if instrument_handler not in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].append(instrument_handler)
            self._log.debug(f"Added {instrument_handler} for {symbol} instruments.")
        else:
            self._log.error(f"Cannot add {instrument_handler} for {symbol} instruments"
                            f"(duplicate handler found).")

    cdef void _remove_tick_handler(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from tick data for the given symbol and handler.
        """
        if symbol not in self._tick_handlers:
            self._log.debug(f"Cannot remove handler (no handlers for {symbol}).")
            return

        cdef TickHandler tick_handler = TickHandler(handler)
        if tick_handler in self._tick_handlers[symbol]:
            self._tick_handlers[symbol].remove(tick_handler)
            self._log.debug(f"Removed handler {tick_handler} for {symbol}.")
        else:
            self._log.error(f"Cannot remove {tick_handler} (no matching handler found).")

        if not self._tick_handlers[symbol]:
            del self._tick_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} tick data.")

    cdef void _remove_bar_handler(self, BarType bar_type, handler: callable) except *:
        """
        Unsubscribe from bar data for the given bar type and handler.
        """
        if bar_type not in self._bar_handlers:
            self._log.debug(f"Cannot remove handler (no handlers for {bar_type}).")
            return

        cdef BarHandler bar_handler = BarHandler(handler)
        if bar_handler in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].remove(bar_handler)
            self._log.debug(f"Removed handler {bar_handler} for {bar_type}.")
        else:
            self._log.debug(f"Cannot remove {bar_handler} (no matching handler found).")

        if not self._bar_handlers[bar_type]:
            del self._bar_handlers[bar_type]
            self._log.info(f"Unsubscribed from {bar_type} bar data.")

    cdef void _remove_instrument_handler(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from tick data for the given symbol and handler.
        """
        if symbol not in self._instrument_handlers:
            self._log.debug(f"Cannot remove handler (no handlers for {symbol}).")
            return

        cdef InstrumentHandler instrument_handler = InstrumentHandler(handler)
        if instrument_handler in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].remove(instrument_handler)
            self._log.debug(f"Removed handler {instrument_handler} for {symbol}.")
        else:
            self._log.debug(f"Cannot remove {instrument_handler} (no matching handler found).")

        if not self._instrument_handlers[symbol]:
            del self._instrument_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} instrument data.")

    cpdef void _bulk_build_tick_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback) except *:
        # Bulk build tick bars
        cdef int ticks_to_order = bar_type.specification.step * limit

        cdef BulkTickBarBuilder bar_builder = BulkTickBarBuilder(
            bar_type,
            self._log.get_logger(),
            callback)

        self.request_ticks(
            bar_type.symbol,
            from_datetime,
            to_datetime,
            ticks_to_order,
            bar_builder.receive)

    cpdef void _handle_tick(self, Tick tick) except *:
        cdef Symbol symbol = tick.symbol
        cdef double spread = tick.ask.as_double() - tick.bid.as_double()

        # Update ticks and spreads
        ticks = self._ticks.get(symbol)
        spreads = self._spreads.get(symbol)
        if ticks is None:
            # Symbol was not registered
            ticks = deque(maxlen=self.tick_capacity)
            spreads = deque(maxlen=self.tick_capacity)
            self._ticks[symbol] = ticks
            self._spreads[symbol] = spreads
            self._log.warning(f"Received {repr(tick)} when symbol not registered. "
                              f"Handling is now setup.")

        ticks.appendleft(tick)
        spreads.appendleft(spread)

        # Update average spread
        cdef double average_spread = self._spreads_average.get(symbol, -1)
        if average_spread == -1:
            average_spread = spread

        cdef double new_average_spread = fast_mean_iterated(
            values=list(spreads),
            next_value=spread,
            current_value=average_spread,
            expected_length=self.tick_capacity,
            drop_left=False)

        self._spreads_average[symbol] = new_average_spread

        # Send to all registered tick handlers for that symbol
        cdef list tick_handlers = self._tick_handlers.get(tick.symbol)
        cdef TickHandler handler
        if tick_handlers is not None:
            for handler in tick_handlers:
                handler.handle(tick)

    cpdef void _handle_bar(self, BarType bar_type, Bar bar) except *:
        # Handle the given bar by sending it to all bar handlers for that bar type
        cdef list bar_handlers = self._bar_handlers.get(bar_type)
        cdef BarHandler handler
        if bar_handlers is not None:
            for handler in bar_handlers:
                handler.handle(bar_type, bar)

    cpdef void _handle_instrument(self, Instrument instrument) except *:
        # Handle the given instrument by sending it to all instrument handlers for that symbol
        self._instruments[instrument.symbol] = instrument
        self._log.info(f"Updated instrument {instrument.symbol}")

        cdef list instrument_handlers = self._instrument_handlers.get(instrument.symbol)
        cdef InstrumentHandler handler
        if instrument_handlers is not None:
            for handler in instrument_handlers:
                handler.handle(instrument)

    cpdef void _handle_instruments(self, list instruments) except *:
        # Handle all instruments individually
        cdef Instrument instrument
        for instrument in instruments:
            self._handle_instrument(instrument)

    cpdef void _reset(self) except *:
        # Reset the class to its initial state
        self._clock.cancel_all_timers()
        self._ticks.clear()
        self._tick_handlers.clear()
        self._spreads.clear()
        self._spreads_average.clear()
        self._bar_aggregators.clear()
        self._bar_handlers.clear()
        self._instrument_handlers.clear()
        self._instruments.clear()

        self._log.debug("Reset.")


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
        Initializes a new instance of the BulkTickBarBuilder class.

        :param bar_type: The bar_type to build.
        :param logger: The logger for the bar aggregator.
        :param callback: The callback to send the built bars to.
        :raises ValueError: If the callback is not type callable.
        """
        Condition.callable(callback, 'callback')

        self.bars = []
        self.aggregator = TickBarAggregator(bar_type, self._add_bar, logger)
        self.callback = callback

    cpdef void receive(self, list ticks) except *:
        """
        Accepts for delivery the bulk list of ticks and builds aggregated tick
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

    def __init__(self, TimeBarAggregator aggregator):
        """
        Initializes a new instance of the BulkTimeBarUpdater class.

        :param aggregator: The time bar aggregator to update.
        """
        Condition.not_none(aggregator, 'aggregator')

        self.aggregator = aggregator
        self.start_time = self.aggregator.next_close - self.aggregator.interval

    cpdef void receive(self, list ticks) except *:
        """
        Accepts for delivery the bulk list of ticks and updates the aggregator.
        
        :param ticks: The bulk ticks for updating the aggregator.
        """
        cdef int i
        for i in range(len(ticks)):
            if ticks[i].timestamp < self.start_time:
                continue

            self.aggregator.update(ticks[i])
