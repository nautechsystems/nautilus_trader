# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.c_enums.venue cimport Venue
from nautilus_trader.model.objects cimport Symbol, Tick, BarType, Bar, Instrument
from nautilus_trader.trade.strategy cimport TradeStrategy


cdef class DataClient:
    """
    The base class for all data clients.
    """
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef dict _tick_handlers
    cdef dict _bar_handlers
    cdef dict _instrument_handlers
    cdef dict _instruments

    cdef readonly Venue venue

    cpdef datetime time_now(self)
    cpdef list subscribed_ticks(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_instruments(self)
    cpdef list instrument_symbols(self)

    cpdef void connect(self)
    cpdef void disconnect(self)
    cpdef void reset(self)
    cpdef void dispose(self)

    cpdef void register_strategy(self, TradeStrategy strategy)
    cpdef void request_ticks(self, Symbol symbol, datetime from_datetime, datetime to_datetime, callback)
    cpdef void request_bars(self, BarType bar_type, datetime from_datetime, datetime to_datetime, callback)
    cpdef void request_instrument(self, Symbol symbol, callback)
    cpdef void request_instruments(self, callback)
    cpdef void subscribe_ticks(self, Symbol symbol, handler)
    cpdef void subscribe_bars(self, BarType bar_type, handler)
    cpdef void subscribe_instrument(self, Symbol symbol, handler)
    cpdef void unsubscribe_ticks(self, Symbol symbol, handler)
    cpdef void unsubscribe_bars(self, BarType bar_type, handler)
    cpdef void unsubscribe_instrument(self, Symbol symbol, handler)
    cpdef void update_instruments(self)
    cpdef dict get_all_instruments(self)
    cpdef Instrument get_instrument(self, Symbol symbol)

    cdef void _add_tick_handler(self, Symbol symbol, handler)
    cdef void _add_bar_handler(self, BarType bar_type, handler)
    cdef void _add_instrument_handler(self, Symbol symbol, handler)
    cdef void _remove_tick_handler(self, Symbol symbol, handler)
    cdef void _remove_bar_handler(self, BarType bar_type, handler)
    cdef void _remove_instrument_handler(self, Symbol symbol, handler)
    cdef void _handle_tick(self, Tick tick)
    cdef void _handle_bar(self, BarType bar_type, Bar bar)
    cdef void _handle_instrument(self, Instrument instrument)
    cdef void _reset(self)
