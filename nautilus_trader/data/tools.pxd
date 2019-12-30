# -------------------------------------------------------------------------------------------------
# <copyright file="tools.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Tick, Bar, DataBar


cdef class TickBuilder:
    cdef Symbol _symbol
    cdef int _decimal_precision
    cdef object _tick_data
    cdef object _bid_data
    cdef object _ask_data

    cpdef list build_ticks_all(self)
    cpdef Tick _build_tick(self, float bid, float ask, datetime timestamp)
    cpdef Tick _build_tick_from_values(self, double[:] values, datetime timestamp)


cdef class BarBuilder:
    cdef int _decimal_precision
    cdef int _volume_multiple
    cdef object _data

    cpdef list build_databars_all(self)
    cpdef list build_databars_from(self, int index=*)
    cpdef list build_databars_range(self, int start=*, int end=*)
    cpdef list build_bars_all(self)
    cpdef list build_bars_from(self, int index=*)
    cpdef list build_bars_range(self, int start=*, int end=*)
    cpdef DataBar _build_databar(self, double[:] values, datetime timestamp)
    cpdef Bar _build_bar(self, double[:] values, datetime timestamp)


cdef class IndicatorUpdater:
    cdef object _indicator
    cdef object _input_method
    cdef list _input_params
    cdef list _outputs
    cdef bint _include_self

    cpdef void update_tick(self, Tick tick) except *
    cpdef void update_bar(self, Bar bar) except *
    cpdef void update_databar(self, DataBar bar) except *
    cpdef dict build_features_ticks(self, list ticks)
    cpdef dict build_features_bars(self, list bars)
    cpdef dict build_features_databars(self, list bars)
    cdef list _get_values(self)
