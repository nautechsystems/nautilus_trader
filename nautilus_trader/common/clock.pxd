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
from cpython.datetime cimport tzinfo
from libc.stdint cimport int64_t

from nautilus_trader.common.timer cimport LiveTimer
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.common.timer cimport Timer
from nautilus_trader.common.uuid cimport UUIDFactory


cdef class Clock:
    cdef UUIDFactory _uuid_factory
    cdef dict _timers
    cdef dict _handlers
    cdef Timer[:] _stack
    cdef object _default_handler

    cdef readonly bint is_test_clock
    """If the clock is a `TestClock`.\n\n:returns: `bool`"""
    cdef readonly bint is_default_handler_registered
    """If the clock has a default handler registered.\n\n:returns: `bool`"""
    cdef readonly int timer_count
    """The number of timers active in the clock.\n\n:returns: `int`"""
    cdef readonly datetime next_event_time
    """The timestamp of the next time event.\n\n:returns: `datetime`"""
    cdef readonly int64_t next_event_time_ns
    """The UNIX timestamp (nanos) of the next time event.\n\n:returns: `int64`"""
    cdef readonly str next_event_name
    """The name of the next time event.\n\n:returns: `str`"""

    cpdef double timestamp(self) except *
    cpdef int64_t timestamp_ns(self) except *
    cpdef datetime utc_now(self)
    cpdef datetime local_now(self, tzinfo tz=*)
    cpdef timedelta delta(self, datetime time)
    cpdef list timer_names(self)
    cpdef Timer timer(self, str name)
    cpdef void register_default_handler(self, handler: callable) except *
    cpdef void set_time_alert(self, str name, datetime alert_time, handler=*) except *
    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time=*,
        datetime stop_time=*,
        handler: callable=*,
    ) except *
    cpdef void cancel_timer(self, str name) except *
    cpdef void cancel_timers(self) except *

    cdef Timer _create_timer(
        self,
        str name,
        callback: callable,
        int64_t interval_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns,
    )
    cdef void _add_timer(self, Timer timer, handler: callable) except *
    cdef void _remove_timer(self, Timer timer) except *
    cdef void _update_stack(self) except *
    cdef void _update_timing(self) except *


cdef class TestClock(Clock):
    cdef int64_t _time_ns
    cdef dict _pending_events

    cpdef void set_time(self, int64_t to_time_ns) except *
    cpdef list advance_time(self, int64_t to_time_ns)


cdef class LiveClock(Clock):
    cdef object _loop
    cdef tzinfo _utc

    cpdef void _raise_time_event(self, LiveTimer timer) except *

    cdef void _handle_time_event(self, TimeEvent event) except *
