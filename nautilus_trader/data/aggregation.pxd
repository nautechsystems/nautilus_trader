# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class BarBuilder:
    cdef BarType _bar_type

    cdef readonly bint use_previous_close
    """If the builder is using the previous close for aggregation.\n\n:returns: `bool`"""
    cdef readonly bint initialized
    """If the builder is initialized.\n\n:returns: `bool`"""
    cdef readonly int64_t last_timestamp_ns
    """The builders last update UNIX timestamp (nanoseconds).\n\n:returns: `int64`"""
    cdef readonly int count
    """The builders current update count.\n\n:returns: `int`"""

    cdef bint _partial_set
    cdef Price _last_close
    cdef Price _open
    cdef Price _high
    cdef Price _low
    cdef Price _close
    cdef object volume

    cpdef void set_partial(self, Bar partial_bar) except *
    cpdef void update(self, Price price, Quantity size, int64_t timestamp_ns) except *
    cpdef void reset(self) except *
    cpdef Bar build_now(self)
    cpdef Bar build(self, int64_t timestamp_ns)


cdef class BarAggregator:
    cdef LoggerAdapter _log
    cdef BarBuilder _builder
    cdef object _handler

    cdef readonly BarType bar_type
    """The aggregators bar type.\n\n:returns: `BarType`"""

    cpdef void handle_quote_tick(self, QuoteTick tick) except *
    cpdef void handle_trade_tick(self, TradeTick tick) except *
    cdef void _apply_update(self, Price price, Quantity size, int64_t timestamp_ns) except *
    cdef void _build_now_and_send(self) except *
    cdef void _build_and_send(self, int64_t timestamp_ns) except *


cdef class TickBarAggregator(BarAggregator):
    pass


cdef class VolumeBarAggregator(BarAggregator):
    pass


cdef class ValueBarAggregator(BarAggregator):
    cdef object _cum_value

    cpdef object get_cumulative_value(self)


cdef class TimeBarAggregator(BarAggregator):
    cdef Clock _clock
    cdef bint _build_on_next_tick
    cdef int64_t _stored_close_ns

    cdef readonly timedelta interval
    """The aggregators time interval.\n\n:returns: `timedelta`"""
    cdef readonly int64_t interval_ns
    """The aggregators time interval.\n\n:returns: `int64`"""
    cdef readonly int64_t next_close_ns
    """The aggregators next closing time.\n\n:returns: `int64`"""

    cpdef datetime get_start_time(self)
    cpdef void set_partial(self, Bar partial_bar) except *
    cpdef void stop(self) except *
    cdef timedelta _get_interval(self)
    cdef int64_t _get_interval_ns(self)
    cpdef void _set_build_timer(self) except *
    cpdef void _build_bar(self, int64_t timestamp_ns) except *
    cpdef void _build_event(self, TimeEvent event) except *


cdef class BulkTickBarBuilder:
    cdef TickBarAggregator aggregator
    cdef object callback
    cdef list bars


cdef class BulkTimeBarUpdater:
    cdef TimeBarAggregator aggregator
    cdef int64_t start_time_ns
