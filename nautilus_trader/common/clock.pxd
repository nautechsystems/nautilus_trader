# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Callable

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from cpython.datetime cimport tzinfo
from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.common.timer cimport LiveTimer
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.rust.common cimport CTestClock


cdef class Clock:
    cdef dict _handlers
    cdef object _default_handler

    cpdef double timestamp(self) except *
    cpdef uint64_t timestamp_ms(self) except *
    cpdef uint64_t timestamp_ns(self) except *
    cpdef datetime utc_now(self)
    cpdef datetime local_now(self, tzinfo tz=*)
    cpdef uint64_t next_time_ns(self, str name) except *
    cpdef void register_default_handler(self, handler: Callable[[TimeEvent], None]) except *
    cpdef void set_time_alert(
        self,
        str name,
        datetime alert_time,
        callback: Callable[[TimeEvent], None]=*,
    ) except *
    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Callable[[TimeEvent], None]=*,
    ) except *
    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time=*,
        datetime stop_time=*,
        callback: Callable[[TimeEvent], None]=*,
    ) except *
    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Callable[[TimeEvent], None]=*,
    ) except *
    cpdef void cancel_timer(self, str name) except *
    cpdef void cancel_timers(self) except *


cdef class TestClock(Clock):
    cdef CTestClock _mem

    cpdef void set_time(self, uint64_t to_time_ns) except *
    cpdef list advance_time(self, uint64_t to_time_ns, bint set_time=*)


cdef class LiveClock(Clock):
    cdef object _loop
    cdef double _offset_secs
    cdef int64_t _offset_ms
    cdef int64_t _offset_ns
    cdef int _timer_count
    cdef dict _timers
    cdef LiveTimer[:] _stack
    cdef tzinfo _utc
    cdef uint64_t _next_event_time_ns

    cpdef void set_offset(self, int64_t offset_ns) except *
    cpdef void _raise_time_event(self, LiveTimer timer) except *

    cdef void _handle_time_event(self, TimeEvent event) except *
    cdef void _add_timer(self, LiveTimer timer, handler: Callable[[TimeEvent], None]) except *
    cdef void _remove_timer(self, LiveTimer timer) except *
    cdef void _update_stack(self) except *
    cdef void _update_timing(self) except *
    cdef LiveTimer _create_timer(
        self,
        str name,
        callback: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
    )
