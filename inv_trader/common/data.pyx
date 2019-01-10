#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime
from typing import List, Dict, Callable

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.model.objects cimport Symbol, BarType, Instrument
from inv_trader.strategy cimport TradeStrategy

cdef str UTF8 = 'utf-8'

cdef class DataClient:
    """
    The abstract base class for all data clients.
    """

    def __init__(self,
                 Clock clock=LiveClock(),
                 Logger logger=None):
        """
        Initializes a new instance of the DataClient class.

        :param clock: The internal clock.
        :param logger: The internal logger.
        """
        self._clock = clock
        if logger is None:
            self._log = LoggerAdapter(f"DataClient")
        else:
            self._log = LoggerAdapter(f"DataClient", logger)

        self._subscriptions_bars = []   # type: List[str]
        self._subscriptions_ticks = []  # type: List[str]
        self._instruments = {}          # type: Dict[Symbol, Instrument]
        self._bar_handlers = {}         # type: Dict[BarType, List[Callable]]
        self._tick_handlers = {}        # type: Dict[Symbol, List[Callable]]

        self._log.info("Initialized.")

    cpdef list symbols(self):
        """
        :return: All instrument symbols held by the data client.
        """
        return list(self._instruments).copy()

    cpdef list instruments(self):
        """
        :return: All instruments held by the data client.
        """
        return list(self._instruments.values()).copy()

    cpdef list subscriptions_ticks(self):
        """
        :return: The list of tick channels subscribed to.
        """
        return self._subscriptions_ticks

    cpdef list subscriptions_bars(self):
        """
        :return: The list of bar channels subscribed to.
        """
        return self._subscriptions_bars

    cpdef void connect(self):
        """
        Connect to the data service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the data client.")

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the data client.")

    cpdef void update_all_instruments(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the data client.")

    cpdef void update_instrument(self, symbol: Symbol):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the data client.")

    cpdef Instrument get_instrument(self, Symbol symbol):
        """
        Get the instrument corresponding to the given symbol.

        :param symbol: The symbol of the instrument to get.
        :return: The instrument (if found)
        :raises KeyError: If the instrument is not found.
        """
        if symbol not in self._instruments:
            raise KeyError(f"Cannot find instrument for {symbol}.")

        return self._instruments[symbol]

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Registers the given trade strategy with the data client.

        :param strategy: The strategy to register.
        :raise ValueError: If the strategy does not inherit from TradeStrategy.
        """
        strategy._register_data_client(self)

        self._log.info(f"Registered strategy {strategy} with the data client.")

    cpdef void historical_bars(
            self,
            BarType bar_type,
            int quantity,
            handler: Callable):
        """
        Download the historical bars for the given parameters from the data
        service, then pass them to the callable bar handler.

        Note: A log warnings are given if the downloaded bars quantity does not
        equal the requested quantity.

        :param bar_type: The historical bar type to download.
        :param quantity: The number of historical bars to download (can be None, will download all).
        :param handler: The bar handler to pass the bars to.
        :raises ValueError: If the quantity is not None and not positive (> 0).
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the data client.")

    cpdef void historical_bars_from(
            self,
            BarType bar_type,
            datetime from_datetime,
            handler: Callable):
        """
        Download the historical bars for the given parameters from the data
        service, then pass them to the callable bar handler.

        Note: A log warning is given if the downloaded bars first timestamp is
        greater than the requested datetime.

        :param bar_type: The historical bar type to download.
        :param from_datetime: The datetime from which the historical bars should be downloaded.
        :param handler: The handler to pass the bars to.
        :raises ValueError: If the from_datetime is not less than datetime.utcnow().
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the data client.")

    cdef void _subscribe_bars(
            self,
            BarType bar_type,
            handler: Callable):
        """
        Subscribe to live bar data for the given bar parameters.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        Precondition.type_or_none(handler, Callable, 'handler')

        if bar_type not in self._bar_handlers:
            self._bar_handlers[bar_type] = []  # type: List[Callable]

        if handler is not None and handler not in self._bar_handlers[bar_type]:
            self._bar_handlers[bar_type].append(handler)
            self._log.debug(f"Added bar handler {handler}.")

    cdef void _unsubscribe_bars(
            self,
            BarType bar_type,
            handler: Callable):
        """
        Unsubscribes from live bar data for the given symbol and venue.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        """
        Precondition.type_or_none(handler, Callable, 'handler')

        if bar_type not in self._bar_handlers:
            self._log.warning(f"Cannot unsubscribe bars (no handlers for {bar_type}).")
            return

        if handler is not None:
            if handler in self._bar_handlers[bar_type]:
                self._bar_handlers[bar_type].remove(handler)
                self._log.debug(f"Removed handler {handler} from bar handlers.")
            else:
                self._log.warning(f"Cannot remove handler {handler} from bar handlers (not found).")

    cdef void _subscribe_ticks(
            self,
            Symbol symbol,
            handler: Callable):
        """
        Subscribe to live tick data for the given symbol and venue.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        Precondition.type_or_none(handler, Callable, 'handler')

        if symbol not in self._tick_handlers:
            self._tick_handlers[symbol] = []  # type: List[Callable]

        if handler is not None and handler not in self._tick_handlers:
            self._tick_handlers[symbol].append(handler)
            self._log.debug(f"Added tick {handler}.")

    cdef void _unsubscribe_ticks(
            self,
            Symbol symbol,
            handler: Callable):
        """
        Unsubscribes from live tick data for the given symbol and venue.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        :raises ValueError: If the symbol is not a valid string.
        """
        Precondition.type_or_none(handler, Callable, 'handler')

        if symbol not in self._tick_handlers:
            self._log.warning(f"Cannot unsubscribe ticks (no handlers for {symbol}).")
            return

        if handler is not None:
            if handler in self._tick_handlers[symbol]:
                self._tick_handlers[symbol].remove(handler)
                self._log.debug(f"Removed handler {handler} from tick handlers.")
            else:
                self._log.warning(f"Cannot remove handler {handler} from tick handlers (not found).")

    cdef void _reset(self):
        """
        Resets the DataClient by clearing all stateful internal values. 
        """
        self._subscriptions_bars = []   # type: List[str]
        self._subscriptions_ticks = []  # type: List[str]
        self._instruments = {}          # type: Dict[Symbol, Instrument]
        self._bar_handlers = {}         # type: Dict[BarType, List[Callable]]
        self._tick_handlers = {}        # type: Dict[Symbol, List[Callable]]

        self._log.info("Initialized.")
