#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

cdef class DataClient:
    cdef list _subscriptions_bars
    cdef list _subscriptions_ticks
    cdef object _instruments
    cdef object _bar_handlers
    cdef object _tick_handlers

    cdef readonly object log

    cpdef list symbols(self)

    cpdef list instruments(self)

    cpdef list subscriptions_ticks(self)

    cpdef list subscriptions_bars(self)

    cpdef void connect(self)

    cpdef void disconnect(self)

    cpdef void update_all_instruments(self)

    cpdef void update_instrument(self, symbol)

    cpdef object get_instrument(self, symbol)

    cpdef void register_strategy(self, strategy)

    cpdef void historical_bars(self, bar_type, int quantity, handler)

    cpdef void historical_bars_from(self, bar_type, from_datetime, handler)

    cdef void _subscribe_bars(self, bar_type, handler)

    cdef void _unsubscribe_bars(self, bar_type, handler)

    cdef void _subscribe_ticks(self, symbol, handler)

    cdef void _unsubscribe_ticks(self, symbol, handler)

    cdef void _reset(self)
