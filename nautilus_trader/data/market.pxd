# -------------------------------------------------------------------------------------------------
# <copyright file="market.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price, Tick, Bar, DataBar, BarSpecification


cdef class TickDataWrangler:
    cdef Symbol _symbol
    cdef int _precision
    cdef object _tick_data
    cdef object _bid_data
    cdef object _ask_data

    cpdef list build_ticks_all(self)
    cpdef Tick _build_tick(self, float bid, float ask, datetime timestamp)
    cpdef Tick _build_tick_from_values(self, double[:] values, datetime timestamp)


cdef class BarDataWrangler:
    cdef int _precision
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


cdef class BarBuilder:
    cdef readonly BarSpecification bar_spec
    cdef readonly datetime last_update
    cdef readonly int count

    cdef Price _open
    cdef Price _high
    cdef Price _low
    cdef Price _close
    cdef long _volume
    cdef bint _use_previous_close

    cpdef void update(self, Tick tick)
    cpdef Bar build(self, datetime close_time=*)
    cdef void _reset(self)
    cdef Price _get_price(self, Tick tick)
    cdef int _get_volume(self, Tick tick)
