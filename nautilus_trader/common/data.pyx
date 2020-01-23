# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from typing import List, Dict, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.identifiers cimport Symbol, Venue
from nautilus_trader.model.objects cimport Tick, BarType, Bar, Instrument
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.handlers cimport TickHandler, BarHandler, InstrumentHandler
from nautilus_trader.common.market cimport BarAggregator, TickBarAggregator, TimeBarAggregator
from nautilus_trader.trade.strategy cimport TradingStrategy


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
                 Clock clock not None,
                 GuidFactory guid_factory not None,
                 Logger logger not None):
        """
        Initializes a new instance of the DataClient class.

        :param clock: The clock for the component.
        :param guid_factory: The GUID factory for the component.
        :param logger: The logger for the component.
        """
        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._bar_aggregators = {}      # type: Dict[BarType, BarAggregator]
        self._tick_handlers = {}        # type: Dict[Symbol, List[TickHandler]]
        self._bar_handlers = {}         # type: Dict[BarType, List[BarHandler]]
        self._instrument_handlers = {}  # type: Dict[Symbol, List[InstrumentHandler]]
        self._instruments = {}          # type: Dict[Symbol, Instrument]

        self._log.info("Initialized.")

# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #
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

    cpdef void request_ticks(self, Symbol symbol, datetime from_datetime, datetime to_datetime, callback: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_bars(self, BarType bar_type, datetime from_datetime, datetime to_datetime, callback: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_instrument(self, Symbol symbol, callback: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void request_instruments(self, Venue venue, callback: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void subscribe_instrument(self, Symbol symbol, handler: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: Callable) except *:
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

    cdef void _self_generate_bars(self, BarType bar_type, handler) except *:
        if bar_type not in self._bar_aggregators:
            if bar_type.specification.structure == BarStructure.TICK:
                aggregator = TickBarAggregator(bar_type, self._handle_bar, self._log.get_logger())
            elif bar_type.specification.structure in _TIME_BARS:
                aggregator = TimeBarAggregator(bar_type,  self._handle_bar, self._clock, self._log.get_logger())
            self._bar_aggregators[bar_type] = aggregator
            self.subscribe_ticks(bar_type.symbol, aggregator.update)

        self._add_bar_handler(bar_type, handler)

    cdef void _add_tick_handler(self, Symbol symbol, handler: Callable) except *:
        """
        Subscribe to tick data for the given symbol and handler.
        """
        if symbol not in self._tick_handlers:
            self._tick_handlers[symbol] = []  # type: List[TickHandler]
            self._log.info(f"Subscribed to {symbol} tick data.")

        cdef TickHandler tick_handler = TickHandler(handler)
        if tick_handler not in self._tick_handlers[symbol]:
            self._tick_handlers[symbol].append(tick_handler)
            self._log.debug(f"Added {tick_handler} for {symbol} tick data.")
        else:
            self._log.error(f"Cannot add {tick_handler} (duplicate handler found).")

    cdef void _add_bar_handler(self, BarType bar_type, handler: Callable) except *:
        """
        Subscribe to bar data for the given bar type and handler.
        """
        if bar_type not in self._bar_handlers:
            self._bar_handlers[bar_type] = []  # type: List[BarHandler]
            self._log.info(f"Subscribed to {bar_type} bar data.")

        cdef BarHandler bar_handler = BarHandler(handler)
        if bar_handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(bar_handler)
            self._log.debug(f"Added {bar_handler} for {bar_type} bar data.")
        else:
            self._log.error(f"Cannot add {bar_handler} (duplicate handler found).")

    cdef void _add_instrument_handler(self, Symbol symbol, handler: Callable) except *:
        """
        Subscribe to instrument data for the given symbol and handler.
        """
        if symbol not in self._instrument_handlers:
            self._instrument_handlers[symbol] = []  # type: List[InstrumentHandler]
            self._log.info(f"Subscribed to {symbol} instrument data.")

        cdef InstrumentHandler instrument_handler = InstrumentHandler(handler)
        if instrument_handler not in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].append(instrument_handler)
            self._log.debug(f"Added {instrument_handler} for {symbol} instruments.")
        else:
            self._log.error(f"Cannot add {instrument_handler} (duplicate handler found).")

    cdef void _remove_tick_handler(self, Symbol symbol, handler: Callable) except *:
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

    cdef void _remove_bar_handler(self, BarType bar_type, handler: Callable) except *:
        """
        Unsubscribe from bar data for the given bar type and handler.
        """
        if bar_type not in self._bar_handlers:
            self._log.error(f"Cannot remove handler (no handlers for {bar_type}).")
            return

        cdef BarHandler bar_handler = BarHandler(handler)
        if bar_handler in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].remove(bar_handler)
            self._log.debug(f"Removed handler {bar_handler} for {bar_type}.")
        else:
            self._log.error(f"Cannot remove {bar_handler} (no matching handler found).")

        if not self._bar_handlers[bar_type]:
            del self._bar_handlers[bar_type]
            self._log.info(f"Unsubscribed from {bar_type} bar data.")

    cdef void _remove_instrument_handler(self, Symbol symbol, handler: Callable) except *:
        """
        Unsubscribe from tick data for the given symbol and handler.
        """
        if symbol not in self._instrument_handlers:
            self._log.error(f"Cannot remove handler (no handlers for {symbol}).")
            return

        cdef InstrumentHandler instrument_handler = InstrumentHandler(handler)
        if instrument_handler in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].remove(instrument_handler)
            self._log.debug(f"Removed handler {instrument_handler} for {symbol}.")
        else:
            self._log.error(f"Cannot remove {instrument_handler} (no matching handler found).")

        if not self._instrument_handlers[symbol]:
            del self._instrument_handlers[symbol]
            self._log.info(f"Unsubscribed from {symbol} instrument data.")

    cpdef void _handle_tick(self, Tick tick) except *:
        # Handle the given tick by sending it to all tick handlers for that symbol
        cdef list tick_handlers = self._tick_handlers.get(tick.symbol, None)
        cdef TickHandler handler
        if tick_handlers:
            for handler in tick_handlers:
                handler.handle(tick)

    cpdef void _handle_bar(self, BarType bar_type, Bar bar) except *:
        # Handle the given bar by sending it to all bar handlers for that bar type
        cdef list bar_handlers = self._bar_handlers.get(bar_type, None)
        cdef BarHandler handler
        if bar_handlers:
            for handler in bar_handlers:
                handler.handle(bar_type, bar)

    cpdef void _handle_instrument(self, Instrument instrument) except *:
        # Handle the given instrument by sending it to all instrument handlers for that symbol
        self._instruments[instrument.symbol] = instrument
        self._log.info(f"Updated instrument {instrument.symbol}")

        cdef list instrument_handlers = self._instrument_handlers.get(instrument.symbol, None)
        cdef InstrumentHandler handler
        if instrument_handlers:
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
        self._bar_aggregators.clear()
        self._tick_handlers.clear()
        self._bar_handlers.clear()
        self._instrument_handlers.clear()
        self._instruments.clear()

        self._log.debug("Reset.")
