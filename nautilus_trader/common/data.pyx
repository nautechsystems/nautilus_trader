# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from typing import Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.typed_collections cimport TypedList, ConcurrentDictionary
from nautilus_trader.model.objects cimport Venue, Symbol, Tick, BarType, Bar, Instrument
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.handlers cimport TickHandler, BarHandler, InstrumentHandler
from nautilus_trader.trade.strategy cimport TradeStrategy


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
        self._tick_handlers = ConcurrentDictionary(Symbol, TypedList)
        self._bar_handlers = ConcurrentDictionary(BarType, TypedList)
        self._instrument_handlers = ConcurrentDictionary(Symbol, TypedList)
        self._instruments = ConcurrentDictionary(Symbol, Instrument)

        self._log.info("Initialized.")

    cpdef datetime time_now(self):
        """
        Return the current time of the data client.
        
        :return: datetime.
        """
        return self._clock.time_now()

    cpdef list subscribed_ticks(self):
        """
        Return the list of tick symbols subscribed to.
        
        :return: List[Symbol].
        """
        return list(self._tick_handlers.keys())

    cpdef list subscribed_bars(self):
        """
        Return the list of bar types subscribed to.
        
        :return: List[BarType].
        """
        return list(self._bar_handlers.keys())

    cpdef list subscribed_instruments(self):
        """
        Return the list of instruments subscribed to.
        
        :return: List[Symbol].
        """
        return list(self._instrument_handlers.keys())

    cpdef list instrument_symbols(self):
        """
        Return all instrument symbols held by the data client.
        
        :return: List[Symbol].
        """
        return list(self._instruments).copy()

    cpdef void connect(self):
        """
        Connect to the data service.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self):
        """
        Disconnect from the data service.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void reset(self):
        """
        Reset the data client.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void dispose(self):
        """
        Dispose of the data client.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Register the given trade strategy with the data client.

        :param strategy: The strategy to register.
        """
        strategy.register_data_client(self)

        self._log.debug(f"Registered {strategy}.")

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

    cpdef dict get_instruments_all(self):
        """
        Return a dictionary of all instruments held by the data client.
        
        :return: Dict[Symbol, Instrument].
        """
        return self._instruments.copy()

    cpdef Instrument get_instrument(self, Symbol symbol):
        """
        Return the instrument corresponding to the given symbol.

        :param symbol: The symbol of the instrument to return.
        :return: Instrument (if found)
        :raises ValueError: If the instrument is not found.
        """
        Condition.true(symbol in self._instruments, 'symbol in instruments')

        return self._instruments[symbol]

    cdef void _add_tick_handler(self, Symbol symbol, handler: Callable):
        # Subscribe to tick data for the given symbol and handler
        Condition.type(handler, Callable, 'handler')

        if symbol not in self._tick_handlers:
            self._tick_handlers[symbol] = TypedList(TickHandler)

        cdef TickHandler tick_handler = TickHandler(handler)
        if tick_handler not in self._tick_handlers[symbol]:
            self._tick_handlers[symbol].append(tick_handler)
            self._log.debug(f"Added {tick_handler} for {symbol} ticks.")
            self._log.info(f"Subscribed to {symbol} ticks.")
        else:
            self._log.error(f"Cannot add {tick_handler} (duplicate handler found).")

    cdef void _add_bar_handler(self, BarType bar_type, handler: Callable):
        # Subscribe to bar data for the given bar type and handler.
        Condition.type(handler, Callable, 'handler')

        if bar_type not in self._bar_handlers:
            self._bar_handlers[bar_type] = TypedList(BarHandler)

        cdef BarHandler bar_handler = BarHandler(handler)
        if bar_handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(bar_handler)
            self._log.debug(f"Added {bar_handler} for {bar_type} bars.")
            self._log.info(f"Subscribed to {bar_type} bars.")
        else:
            self._log.error(f"Cannot add {bar_handler} (duplicate handler found).")

    cdef void _add_instrument_handler(self, Symbol symbol, handler: Callable):
        # Subscribe to tick data for the given symbol and handler
        Condition.type(handler, Callable, 'handler')

        if symbol not in self._instrument_handlers:
            self._instrument_handlers[symbol] = TypedList(InstrumentHandler)

        cdef InstrumentHandler instrument_handler = InstrumentHandler(handler)
        if instrument_handler not in self._instrument_handlers[symbol]:
            self._instrument_handlers[symbol].append(instrument_handler)
            self._log.debug(f"Added {instrument_handler} for {symbol} instruments.")
            self._log.info(f"Subscribed to {symbol} instrument updates.")
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
        # Unsubscribe from bar data for the given bar type and handler.
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
        self._log.info(f"Updated instrument for {instrument.symbol}.")

        cdef InstrumentHandler handler
        if instrument.symbol in self._instrument_handlers:
            for handler in self._instrument_handlers[instrument.symbol]:
                handler.handle(instrument)

    cdef void _handle_instruments(self, list instruments):
        # Handle all instruments individually
        for instrument in instruments:
            self._handle_instrument(instrument)

    cdef void _reset(self):
        # Reset the data client by returning all stateful internal values to their initial value
        self._tick_handlers = ConcurrentDictionary(Symbol, TypedList)
        self._bar_handlers = ConcurrentDictionary(BarType, TypedList)
        self._instrument_handlers = ConcurrentDictionary(Symbol, TypedList)
        self._instruments = ConcurrentDictionary(Symbol, Instrument)

        self._log.debug("Reset.")
