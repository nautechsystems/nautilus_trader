# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.common.clock cimport Clock, TimeEvent
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.handlers cimport BarHandler
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.objects cimport Price, Volume, Tick, BarType, BarSpecification, Bar, Instrument


cdef class TickDataWrangler:
    cdef object _data_ticks
    cdef dict _data_bars_ask
    cdef dict _data_bars_bid

    cdef readonly Instrument instrument
    cdef readonly tick_data
    cdef readonly BarStructure resolution

    cpdef void build(self, int symbol_indexer)
    cpdef Tick _build_tick_from_values_with_sizes(self, double[:] values, datetime timestamp)
    cpdef Tick _build_tick_from_values(self, double[:] values, datetime timestamp)


cdef class BarDataWrangler:
    cdef int _precision
    cdef int _volume_multiple
    cdef object _data

    cpdef list build_bars_all(self)
    cpdef list build_bars_from(self, int index=*)
    cpdef list build_bars_range(self, int start=*, int end=*)
    cpdef Bar _build_bar(self, double[:] values, datetime timestamp)


cdef class IndicatorUpdater:
    cdef Indicator _indicator
    cdef object _input_method
    cdef list _input_params
    cdef list _outputs
    cdef bint _include_self

    cpdef void update_tick(self, Tick tick) except *
    cpdef void update_bar(self, Bar bar) except *
    cpdef dict build_features_ticks(self, list ticks)
    cpdef dict build_features_bars(self, list bars)
    cdef list _get_values(self)


cdef class BarBuilder:
    cdef readonly BarSpecification bar_spec
    cdef readonly datetime last_update
    cdef readonly int count

    cdef Price _last_open
    cdef Price _open
    cdef Price _high
    cdef Price _low
    cdef Price _close
    cdef Volume _volume
    cdef bint _use_previous_close

    cpdef void update(self, Tick tick) except *
    cpdef Bar build(self, datetime close_time=*)
    cdef void _reset(self) except *
    cdef Price _get_price(self, Tick tick)
    cdef Volume _get_volume(self, Tick tick)


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
