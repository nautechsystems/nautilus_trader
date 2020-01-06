# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import cython

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.common.handlers cimport BarHandler
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.identifiers cimport Symbol, Venue
from nautilus_trader.model.objects cimport Tick, BarType, Bar, Instrument
from nautilus_trader.model.events cimport TimeEvent
from nautilus_trader.data.market cimport BarBuilder
from nautilus_trader.trade.strategy cimport TradingStrategy


cdef class DataClient:
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef dict _bar_aggregators
    cdef dict _tick_handlers
    cdef dict _bar_handlers
    cdef dict _instrument_handlers
    cdef dict _instruments

    cdef readonly Venue venue

# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #
    cpdef void connect(self)
    cpdef void disconnect(self)
    cpdef void reset(self)
    cpdef void dispose(self)
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
# ------------------------------------------------------------------------------------------------ #

    cpdef datetime time_now(self)
    cpdef list subscribed_ticks(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_instruments(self)
    cpdef list instrument_symbols(self)
    cpdef void register_strategy(self, TradingStrategy strategy)
    cpdef dict get_instruments(self)
    cpdef Instrument get_instrument(self, Symbol symbol)

    cdef void _self_generate_bars(self, BarType bar_type, handler)
    cdef void _add_tick_handler(self, Symbol symbol, handler)
    cdef void _add_bar_handler(self, BarType bar_type, handler)
    cdef void _add_instrument_handler(self, Symbol symbol, handler)
    cdef void _remove_tick_handler(self, Symbol symbol, handler)
    cdef void _remove_bar_handler(self, BarType bar_type, handler)
    cdef void _remove_instrument_handler(self, Symbol symbol, handler)
    cpdef void _handle_tick(self, Tick tick)
    cpdef void _handle_bar(self, BarType bar_type, Bar bar)
    cpdef void _handle_instrument(self, Instrument instrument)
    cpdef void _handle_instruments(self, list instruments)
    cpdef void _reset(self)


cdef class BarAggregator:
    cdef LoggerAdapter _log
    cdef DataClient _client
    cdef BarHandler _handler
    cdef BarBuilder _builder

    cdef readonly BarType bar_type

    cpdef void update(self, Tick tick) except *
    cpdef void _handle_bar(self, Bar bar)


cdef class TickBarAggregator(BarAggregator):
    cdef int step


cdef class TimeBarAggregator(BarAggregator):
    cdef Clock _clock

    cdef readonly timedelta interval
    cdef readonly datetime next_close

    cpdef void _build_event(self, TimeEvent event)
    cdef timedelta _get_interval(self)
    cdef datetime _get_start_time(self)
    cdef void _set_build_timer(self)
