# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.common cimport LiveClock_API
from nautilus_trader.core.rust.common cimport TestClock_API
from nautilus_trader.core.rust.common cimport TimeEvent_t
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.uuid cimport UUID4


cdef class Clock:
    cpdef double timestamp(self)
    cpdef uint64_t timestamp_ms(self)
    cpdef uint64_t timestamp_ns(self)
    cpdef datetime utc_now(self)
    cpdef datetime local_now(self, tzinfo tz=*)
    cpdef uint64_t next_time_ns(self, str name)
    cpdef void register_default_handler(self, handler: Callable[[TimeEvent], None])
    cpdef void set_time_alert(
        self,
        str name,
        datetime alert_time,
        callback: Callable[[TimeEvent], None]=*,
    )
    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Callable[[TimeEvent], None]=*,
    )
    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time=*,
        datetime stop_time=*,
        callback: Callable[[TimeEvent], None]=*,
    )
    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Callable[[TimeEvent], None]=*,
    )
    cpdef void cancel_timer(self, str name)
    cpdef void cancel_timers(self)


cdef class TestClock(Clock):
    cdef TestClock_API _mem

    cpdef void set_time(self, uint64_t to_time_ns)
    cdef CVec advance_time_c(self, uint64_t to_time_ns, bint set_time=*)
    cpdef list advance_time(self, uint64_t to_time_ns, bint set_time=*)


cdef class LiveClock(Clock):
    cdef LiveClock_API _mem
    cdef object _default_handler
    cdef dict _handlers

    cdef object _loop
    cdef int _timer_count
    cdef dict _timers
    cdef LiveTimer[:] _stack
    cdef tzinfo _utc
    cdef uint64_t _next_event_time_ns

    cpdef void _raise_time_event(self, LiveTimer timer)

    cdef void _handle_time_event(self, TimeEvent event)
    cdef void _add_timer(self, LiveTimer timer, handler: Callable[[TimeEvent], None])
    cdef void _remove_timer(self, LiveTimer timer)
    cdef void _update_stack(self)
    cdef void _update_timing(self)
    cdef LiveTimer _create_timer(
        self,
        str name,
        callback: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
    )


cdef class TimeEvent(Event):
    cdef TimeEvent_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef TimeEvent from_mem_c(TimeEvent_t raw)


cdef class TimeEventHandler:
    cdef object _handler
    cdef readonly TimeEvent event
    """The handlers event.\n\n:returns: `TimeEvent`"""

    cpdef void handle(self)


cdef class LiveTimer:
    cdef object _internal

    cdef readonly str name
    """The timers name using for hashing.\n\n:returns: `str`"""
    cdef readonly object callback
    """The timers callback function.\n\n:returns: `object`"""
    cdef readonly uint64_t interval_ns
    """The timers set interval.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t start_time_ns
    """The timers set start time.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t next_time_ns
    """The timers next alert timestamp.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t stop_time_ns
    """The timers set stop time (if set).\n\n:returns: `uint64_t`"""
    cdef readonly bint is_expired
    """If the timer is expired.\n\n:returns: `bool`"""

    cpdef TimeEvent pop_event(self, UUID4 event_id, uint64_t ts_init)
    cpdef void iterate_next_time(self, uint64_t to_time_ns)
    cpdef void cancel(self)
    cpdef void repeat(self, uint64_t ts_now)
    cdef object _start_timer(self, uint64_t ts_now)


cdef class ThreadTimer(LiveTimer):
    pass


cdef class LoopTimer(LiveTimer):
    cdef object _loop
