# -------------------------------------------------------------------------------------------------
# <copyright file="market.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.common.handlers cimport BarHandler
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price, Tick, BarType, BarSpecification, Bar, DataBar
from nautilus_trader.model.events cimport TimeEvent


cdef class TickDataWrangler:
    cdef Symbol _symbol
    cdef int _precision
    cdef object _data_ticks
    cdef dict _data_bars_ask
    cdef dict _data_bars_bid

    cdef readonly tick_data
    cdef readonly BarStructure resolution

    cpdef void build(self, int symbol_indexer)
    cpdef Tick _build_tick_from_values_with_sizes(self, double[:] values, datetime timestamp)
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

    cpdef void update(self, Tick tick) except *
    cpdef Bar build(self, datetime close_time=*)
    cdef void _reset(self) except *
    cdef Price _get_price(self, Tick tick)
    cdef int _get_volume(self, Tick tick)


cdef class BarAggregator:
    cdef LoggerAdapter _log
    cdef DataClient _client
    cdef BarHandler _handler
    cdef BarBuilder _builder

    cdef readonly BarType bar_type

    cpdef void update(self, Tick tick) except *
    cpdef void _handle_bar(self, Bar bar) except *


cdef class TickBarAggregator(BarAggregator):
    cdef int step


cdef class TimeBarAggregator(BarAggregator):
    cdef Clock _clock

    cdef readonly timedelta interval
    cdef readonly datetime next_close

    cpdef void _build_event(self, TimeEvent event) except *
    cdef timedelta _get_interval(self)
    cdef datetime _get_start_time(self)
    cdef void _set_build_timer(self) except *