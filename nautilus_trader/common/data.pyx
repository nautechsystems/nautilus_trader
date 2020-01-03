# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta
from typing import List, Dict, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure, bar_structure_to_string
from nautilus_trader.model.identifiers cimport Symbol, Venue, Label
from nautilus_trader.model.objects cimport Tick, BarType, Bar, Instrument
from nautilus_trader.model.events cimport TimeEvent
from nautilus_trader.data.market cimport BarBuilder
from nautilus_trader.common.clock cimport Clock, LiveClock, TestClock
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.handlers cimport TickHandler, BarHandler, InstrumentHandler
from nautilus_trader.trade.strategy cimport TradingStrategy


cdef class DataClient:
    """
    The base class for all data clients.
    """

    def __init__(self,
                 Venue venue,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the DataClient class.

        :param venue: The venue for the data client.
        :param clock: The clock for the component.
        :param guid_factory: The GUID factory for the component.
        :param logger: The logger for the component.
        """
        self.venue = venue
        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._tick_handlers = {}        # type: Dict[Symbol, List[TickHandler]]
        self._bar_handlers = {}         # type: Dict[BarType, List[BarHandler]]
        self._instrument_handlers = {}  # type: Dict[Symbol, List[InstrumentHandler]]
        self._instruments = {}          # type: Dict[Symbol, Instrument]

        self._log.info("Initialized.")

# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #
    cpdef void connect(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void reset(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void dispose(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_ticks(self, Symbol symbol, datetime from_datetime, datetime to_datetime, callback: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_bars(self, BarType bar_type, datetime from_datetime, datetime to_datetime, callback: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_instrument(self, Symbol symbol, callback: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_instruments(self, callback: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_instrument(self, Symbol symbol, handler: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void update_instruments(self):
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

    cpdef void register_strategy(self, TradingStrategy strategy):
        """
        Register the given trade strategy with the data client.

        :param strategy: The strategy to register.
        """
        strategy.register_data_client(self)

        self._log.debug(f"Registered {strategy}.")

    cpdef dict get_instruments(self):
        """
        Return a dictionary of all instruments held by the data client.
        
        :return Dict[Symbol, Instrument].
        """
        return self._instruments.copy()

    cpdef Instrument get_instrument(self, Symbol symbol):
        """
        Return the instrument corresponding to the given symbol.

        :param symbol: The symbol of the instrument to return.
        :return Instrument (if found)
        :raises ConditionFailed: If the instrument is not found.
        """
        Condition.is_in(symbol, self._instruments, 'symbol', 'instruments')

        return self._instruments[symbol]

    cdef void _add_tick_handler(self, Symbol symbol, handler: Callable):
        # Subscribe to tick data for the given symbol and handler
        Condition.type(handler, Callable, 'handler')

        if symbol not in self._tick_handlers:
            self._tick_handlers[symbol] = []  # type: List[TickHandler]

        cdef TickHandler tick_handler = TickHandler(handler)
        if tick_handler not in self._tick_handlers[symbol]:
            self._tick_handlers[symbol].append(tick_handler)
            self._log.debug(f"Added {tick_handler} for {symbol} tick data.")
            self._log.info(f"Subscribed to {symbol} tick data.")
        else:
            self._log.error(f"Cannot add {tick_handler} (duplicate handler found).")

    cdef void _add_bar_handler(self, BarType bar_type, handler: Callable):
        # Subscribe to bar data for the given bar type and handler
        Condition.type(handler, Callable, 'handler')

        if bar_type not in self._bar_handlers:
            self._bar_handlers[bar_type] = []  # type: List[BarHandler]

        cdef BarHandler bar_handler = BarHandler(handler)
        if bar_handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(bar_handler)
            self._log.debug(f"Added {bar_handler} for {bar_type} bar data.")
            self._log.info(f"Subscribed to {bar_type} bar data.")
        else:
            self._log.error(f"Cannot add {bar_handler} (duplicate handler found).")

    cdef void _add_instrument_handler(self, Symbol symbol, handler: Callable):
        # Subscribe to tick data for the given symbol and handler
        Condition.type(handler, Callable, 'handler')

        if symbol not in self._instrument_handlers:
            self._instrument_handlers[symbol] = []  # type: List[InstrumentHandler]

        cdef InstrumentHandler instrument_handler = InstrumentHandler(handler)
        if instrument_handler not in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].append(instrument_handler)
            self._log.debug(f"Added {instrument_handler} for {symbol} instruments.")
            self._log.info(f"Subscribed to {symbol} instrument data.")
        else:
            self._log.error(f"Cannot add {instrument_handler} (duplicate handler found).")

    cdef void _remove_tick_handler(self, Symbol symbol, handler: Callable):
        # Unsubscribe from tick data for the given symbol and handler
        Condition.type(handler, Callable, 'handler')

        if symbol not in self._tick_handlers:
            self._log.error(f"Cannot remove handler (no handlers for {symbol}).")
            return

        cdef TickHandler tick_handler = TickHandler(handler)
        if tick_handler in self._tick_handlers[symbol]:
            self._tick_handlers[symbol].remove(tick_handler)
            self._log.debug(f"Removed handler {tick_handler} for {symbol}.")
        else:
            self._log.error(f"Cannot remove {tick_handler} (no matching handler found).")

        if len(self._tick_handlers[symbol]) == 0:
            del self._tick_handlers[symbol]

    cdef void _remove_bar_handler(self, BarType bar_type, handler: Callable):
        # Unsubscribe from bar data for the given bar type and handler
        Condition.type(handler, Callable, 'handler')

        if bar_type not in self._bar_handlers:
            self._log.error(f"Cannot remove handler (no handlers for {bar_type}).")
            return

        cdef BarHandler bar_handler = BarHandler(handler)
        if bar_handler in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].remove(bar_handler)
            self._log.debug(f"Removed handler {bar_handler} for {bar_type}.")
        else:
            self._log.error(f"Cannot remove {bar_handler} (no matching handler found).")

        if len(self._bar_handlers[bar_type]) == 0:
            del self._bar_handlers[bar_type]

    cdef void _remove_instrument_handler(self, Symbol symbol, handler: Callable):
        # Unsubscribe from tick data for the given symbol and handler
        Condition.type(handler, Callable, 'handler')

        if symbol not in self._instrument_handlers:
            self._log.error(f"Cannot remove handler (no handlers for {symbol}).")
            return

        cdef InstrumentHandler instrument_handler = InstrumentHandler(handler)
        if instrument_handler in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].remove(instrument_handler)
            self._log.debug(f"Removed handler {instrument_handler} for {symbol}.")
        else:
            self._log.error(f"Cannot remove {instrument_handler} (no matching handler found).")

        if len(self._instrument_handlers[symbol]) == 0:
            del self._instrument_handlers[symbol]

    cdef void _handle_tick(self, Tick tick):
        # Handle the given tick by sending it to all tick handlers for that symbol
        cdef TickHandler handler
        if tick.symbol in self._tick_handlers:
            for handler in self._tick_handlers[tick.symbol]:
                handler.handle(tick)

    cdef void _handle_bar(self, BarType bar_type, Bar bar):
        # Handle the given bar by sending it to all bar handlers for that bar type
        cdef BarHandler handler
        if bar_type in self._bar_handlers:
            for handler in self._bar_handlers[bar_type]:
                handler.handle(bar_type, bar)

    cdef void _handle_instrument(self, Instrument instrument):
        # Handle the given instrument by sending it to all instrument handlers for that symbol
        if instrument.symbol in self._instruments:
            # Remove instrument key if already exists
            del self._instruments[instrument.symbol]
        self._instruments[instrument.symbol] = instrument
        self._log.info(f"Updated instrument {instrument.symbol}")

        cdef InstrumentHandler handler
        if instrument.symbol in self._instrument_handlers:
            for handler in self._instrument_handlers[instrument.symbol]:
                handler.handle(instrument)

    cdef void _handle_instruments(self, list instruments):
        # Handle all instruments individually
        for instrument in instruments:
            self._handle_instrument(instrument)

    cdef void _reset(self):
        # Reset the class to its initial state
        self._tick_handlers.clear()
        self._bar_handlers.clear()
        self._instrument_handlers.clear()
        self._instruments.clear()

        self._log.debug("Reset.")


cdef class BarAggregator:
    """
    Provides a means of aggregating built bars to the registered handler.
    """

    def __init__(self,
                 BarType bar_type,
                 handler,
                 Logger logger,
                 bint use_previous_close):
        """
        Initializes a new instance of the BarAggregator class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The handler to receive built bars (must be Callable).
        :param logger: The logger for the aggregator.
        :param use_previous_close: If the previous close price should be the open price of a new bar.
        :raises: ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self.bar_type = bar_type
        self._handler = handler
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._builder = BarBuilder(
            bar_spec=self.bar_type.specification,
            use_previous_close=use_previous_close)

    cpdef void update(self, Tick tick, long volume=1):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _handle_bar(self, Bar bar):
        self._log.debug(f"Built {self.bar_type} bar.")
        self._handler(bar)


cdef class TickBarAggregator(BarAggregator):
    """
    Provides a means of building tick bars from ticks.
    """

    def __init__(self,
                 BarType bar_type,
                 handler,
                 Logger logger):
        """
        Initializes a new instance of the TickBarBuilder class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The handler builder (must be Callable).
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        super().__init__(bar_type=bar_type,
                         handler=handler,
                         logger=logger,
                         use_previous_close=False)

        self.step = bar_type.specification.step

    cpdef void update(self, Tick tick, long volume=1):
        """
        Update the builder with the given tick.

        :param tick: The tick for the update.
        :param volume: The market volume for the update.
        """
        self._builder.update(tick, volume)

        if self._builder.count == self.step:
            self._handle_bar(self._builder.build())


cdef class TimeBarAggregator(BarAggregator):
    """
    Provides a means of building time bars from ticks with an internal timer.
    """
    def __init__(self,
                 BarType bar_type,
                 handler,
                 Clock clock,
                 Logger logger):
        """
        Initializes a new instance of the TickBarBuilder class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The handler builder (must be Callable).
        :param clock: If the clock for the aggregator.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        super().__init__(bar_type=bar_type,
                         handler=handler,
                         logger=logger,
                         use_previous_close=True)

        self._clock = clock
        self.interval = self._get_interval()
        self._set_build_timer(self.bar_type.specification.structure, self.interval)

    cpdef void update(self, Tick tick, long volume=1):
        """
        Update the builder with the given tick.

        :param tick: The tick for the update.
        :param volume: The market volume for the update.
        """
        self._builder.update(tick, volume)

        cdef dict events
        cdef TimeEvent event
        if self._clock.is_test_clock:
            if self._clock.has_timers and tick.timestamp >= self._clock.next_event_time:
                events = self._clock.advance_time(tick.timestamp)
                for event, handler in sorted(events.items()):
                    handler(event)

    cpdef void _build_event(self, TimeEvent event):
        self._handle_bar(self._builder.build(event.timestamp))

    cdef timedelta _get_interval(self):
        if self.bar_type.specification.structure == BarStructure.SECOND:
            return timedelta(seconds=(1 * self.bar_type.specification.step))
        elif self.bar_type.specification.structure == BarStructure.MINUTE:
            return timedelta(minutes=(1 * self.bar_type.specification.step))
        elif self.bar_type.specification.structure == BarStructure.HOUR:
            return timedelta(hours=(1 * self.bar_type.specification.step))
        elif self.bar_type.specification.structure == BarStructure.DAY:
            return timedelta(days=(1 * self.bar_type.specification.step))
        else:
            raise ValueError(f"The BarStructure {bar_structure_to_string(self.bar_type.specification.structure)} is not supported.")

    cdef datetime _get_start_time(self, BarStructure structure):
        cdef datetime now = self._clock.time_now()
        cdef datetime start
        if structure == BarStructure.SECOND:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                minute=now.minute,
                second=now.second,
                tzinfo=now.tzinfo
            )
        elif structure == BarStructure.MINUTE:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                minute=now.minute,
                tzinfo=now.tzinfo
            )
        elif structure == BarStructure.HOUR:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                tzinfo=now.tzinfo
            )
        elif structure == BarStructure.DAY:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
            )
        else:
            raise ValueError(f"The BarStructure {bar_structure_to_string(structure)} is not supported.")

    cdef void _set_build_timer(self, BarStructure structure, timedelta interval):
        cdef datetime start_time = self._get_start_time(self.bar_type.specification.structure)
        self._clock.set_timer(
            label=Label(str(self.bar_type)),
            interval=interval,
            start_time=start_time,
            stop_time=None,
            handler=self._build_event)
