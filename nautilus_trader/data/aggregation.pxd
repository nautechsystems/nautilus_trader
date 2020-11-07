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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarData
from nautilus_trader.model.bar cimport BarSpecification
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.objects cimport Decimal
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class BarBuilder:
    cdef readonly BarSpecification bar_spec
    """The builders bar specification.\n\n:returns: `BarSpecification`"""
    cdef readonly bint use_previous_close
    """If the builder is using the previous close for aggregation.\n\n:returns: `bool`"""
    cdef readonly bint initialized
    """If the builder is initialized.\n\n:returns: `bool`"""
    cdef readonly datetime last_timestamp
    """The builders last update timestamp.\n\n:returns: `datetime`"""
    cdef readonly int count
    """The builders current update count.\n\n:returns: `int`"""

    cdef Price _last_close
    cdef Price _open
    cdef Price _high
    cdef Price _low
    cdef Price _close
    cdef Decimal volume

    cpdef void update(self, Price price, Decimal size, datetime timestamp) except *
    cpdef void reset(self) except *
    cpdef Bar build(self, datetime close_time=*)


cdef class BarAggregator:
    cdef LoggerAdapter _log
    cdef BarBuilder _builder
    cdef object _handler

    cdef readonly BarType bar_type
    """The aggregators bar type.\n\n:returns: `BarType`"""

    cpdef void handle_quote_tick(self, QuoteTick tick) except *
    cpdef void handle_trade_tick(self, TradeTick tick) except *
    cdef void _apply_update(self, Price price, Quantity size, datetime timestamp) except *
    cdef inline void _build_and_send(self, datetime close=*) except *


cdef class TickBarAggregator(BarAggregator):
    cdef int step


cdef class VolumeBarAggregator(BarAggregator):
    cdef int step


cdef class TimeBarAggregator(BarAggregator):
    cdef Clock _clock

    cdef readonly timedelta interval
    """The aggregators time interval.\n\n:returns: `timedelta`"""
    cdef readonly datetime next_close
    """The aggregators next closing time.\n\n:returns: `datetime`"""

    cpdef datetime get_start_time(self)
    cpdef void stop(self) except *
    cdef timedelta _get_interval(self)
    cpdef void _set_build_timer(self) except *
    cpdef void _build_bar(self, datetime at_time) except *
    cpdef void _build_event(self, TimeEvent event) except *


cdef class BulkTickBarBuilder:
    cdef TickBarAggregator aggregator
    cdef object callback
    cdef list bars

    cpdef void _add_bar(self, BarData data) except *


cdef class BulkTimeBarUpdater:
    cdef TimeBarAggregator aggregator
    cdef datetime start_time
