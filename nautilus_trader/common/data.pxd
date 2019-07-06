#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.objects cimport Symbol, Tick, BarType, Bar, Instrument
from nautilus_trader.trade.strategy cimport TradeStrategy


cdef class DataClient:
    """
    The base class for all data clients.
    """
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef dict _instruments
    cdef dict _tick_handlers
    cdef dict _bar_handlers

    cpdef datetime time_now(self)
    cpdef list symbols(self)
    cpdef list instruments(self)
    cpdef list subscribed_ticks(self)
    cpdef list subscribed_bars(self)

    cpdef void connect(self)
    cpdef void disconnect(self)
    cpdef void update_all_instruments(self)
    cpdef void update_instrument(self, Symbol symbol)
    cpdef dict get_all_instruments(self)
    cpdef Instrument get_instrument(self, Symbol symbol)
    cpdef void register_strategy(self, TradeStrategy strategy)
    cpdef void historical_bars(self, BarType bar_type, int quantity, handler)
    cpdef void historical_bars_from(self, BarType bar_type, datetime from_datetime, handler)
    cpdef void subscribe_ticks(self, Symbol symbol, handler)
    cpdef void unsubscribe_ticks(self, Symbol symbol, handler)
    cpdef void subscribe_bars(self, BarType bar_type, handler)
    cpdef void unsubscribe_bars(self, BarType bar_type, handler)

    cdef void _subscribe_ticks(self, Symbol symbol, handler)
    cdef void _unsubscribe_ticks(self, Symbol symbol, handler)
    cdef void _subscribe_bars(self, BarType bar_type, handler)
    cdef void _unsubscribe_bars(self, BarType bar_type, handler)
    cdef void _handle_tick(self, Tick tick)
    cdef void _handle_bar(self, BarType bar_type, Bar bar)
    cdef void _reset(self)
