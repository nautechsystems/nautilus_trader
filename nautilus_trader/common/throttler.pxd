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

from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.common.timer cimport TimeEvent


cdef class Throttler:
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef int _limit
    cdef timedelta _interval
    cdef int64_t _interval_ns
    cdef Queue _buffer
    cdef object _timestamps
    cdef str _timer_name
    cdef object _output

    cdef readonly str name
    """The name of the throttler.\n\n:returns: `str`"""
    cdef readonly bint is_buffering
    """If the throttler is currently buffering items.\n\n:returns: `bool`"""

    cpdef double utilization(self) except *
    cpdef void send(self, item) except *
    cdef int64_t _next_delta(self) except *
    cdef void _send_item(self, item) except *
    cpdef void _process(self, TimeEvent event) except *
