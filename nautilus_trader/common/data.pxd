# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport date, datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.identifiers cimport Symbol, Venue
from nautilus_trader.model.objects cimport Tick, BarType, Bar, Instrument
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataClient:
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef dict _bar_aggregators
    cdef dict _tick_handlers
    cdef dict _bar_handlers
    cdef dict _instrument_handlers
    cdef dict _instruments

# -- ABSTRACT METHODS ------------------------------------------------------------------------------
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void request_ticks(
        self,
        Symbol symbol,
        date from_date,
        date to_date,
        int limit,
        callback) except *
    cpdef void request_bars(
        self,
        BarType bar_type,
        date from_date,
        date to_date,
        int limit,
        callback) except *
    cpdef void request_instrument(self, Symbol symbol, callback) except *
    cpdef void request_instruments(self, Venue venue, callback) except *
    cpdef void subscribe_ticks(self, Symbol symbol, handler) except *
    cpdef void subscribe_bars(self, BarType bar_type, handler) except *
    cpdef void subscribe_instrument(self, Symbol symbol, handler) except *
    cpdef void unsubscribe_ticks(self, Symbol symbol, handler) except *
    cpdef void unsubscribe_bars(self, BarType bar_type, handler) except *
    cpdef void unsubscribe_instrument(self, Symbol symbol, handler) except *
    cpdef void update_instruments(self, Venue venue) except *
# ------------------------------------------------------------------------------------------------ #

    cpdef datetime time_now(self)
    cpdef list subscribed_ticks(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_instruments(self)
    cpdef list instrument_symbols(self)
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef dict get_instruments(self)
    cpdef Instrument get_instrument(self, Symbol symbol)

    cdef void _self_generate_bars(self, BarType bar_type, handler) except *
    cdef void _add_tick_handler(self, Symbol symbol, handler) except *
    cdef void _add_bar_handler(self, BarType bar_type, handler) except *
    cdef void _add_instrument_handler(self, Symbol symbol, handler) except *
    cdef void _remove_tick_handler(self, Symbol symbol, handler) except *
    cdef void _remove_bar_handler(self, BarType bar_type, handler) except *
    cdef void _remove_instrument_handler(self, Symbol symbol, handler) except *
    cpdef void _bulk_build_tick_bars(
            self,
            BarType bar_type,
            date from_date,
            date to_date,
            int limit,
            callback) except *
    cpdef void _handle_tick(self, Tick tick) except *
    cpdef void _handle_bar(self, BarType bar_type, Bar bar) except *
    cpdef void _handle_instrument(self, Instrument instrument) except *
    cpdef void _handle_instruments(self, list instruments) except *
    cpdef void _reset(self) except *
