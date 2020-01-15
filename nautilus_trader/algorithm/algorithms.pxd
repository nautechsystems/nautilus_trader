# -------------------------------------------------------------------------------------------------
# <copyright file="algorithms.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.objects cimport Price, Tick, BarType, Bar
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.order cimport Order


cdef class TrailingStopSignal:
    cdef bint is_signal
    cdef Price price


cdef class TrailingStopAlgorithm:
    cdef Order order

    cdef object _calculate
    cdef object generate

    cdef TrailingStopSignal _generate_buy(self, Price update_price)
    cdef TrailingStopSignal _generate_sell(self, Price update_price)


cdef class TickTrailingStopAlgorithm(TrailingStopAlgorithm):
    cdef readonly Symbol symbol

    cpdef void update(self, Tick tick) except *
    cpdef TrailingStopSignal calculate_buy(self, Tick tick)
    cpdef TrailingStopSignal calculate_sell(self, Tick tick)


cdef class BarTrailingStopAlgorithm(TrailingStopAlgorithm):
    cdef readonly BarType bar_type

    cpdef void update(self, Bar bar) except *
    cpdef TrailingStopSignal calculate_buy(self, Bar bar)
    cpdef TrailingStopSignal calculate_sell(self, Bar bar)


cdef class BarsBackTrail(BarTrailingStopAlgorithm):
    cdef int _bars_back
    cdef float _sl_atr_multiple
    cdef list _bars
    cdef object _atr
