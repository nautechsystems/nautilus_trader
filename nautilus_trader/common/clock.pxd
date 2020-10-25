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
from cpython.datetime cimport tzinfo

from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.timer cimport LiveTimer
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.common.timer cimport Timer
from nautilus_trader.common.uuid cimport UUIDFactory


cdef class Clock:
    cdef LoggerAdapter _log
    cdef UUIDFactory _uuid_factory
    cdef dict _timers
    cdef dict _handlers
    cdef Timer[:] _stack
    cdef object _default_handler

    cdef readonly int timer_count
    """
    Returns
    -------
    int
        The number of timers active in the clock.

    """
    cdef readonly datetime next_event_time
    """
    Returns
    -------
    str
        The name of the next time event.
    """
    cdef readonly str next_event_name

    cdef readonly bint is_test_clock
    """
    Return a value indicating whether the clock is a `TestClock`.

    Returns
    -------
    bool
        True if clock is of type `TestClock`, else False.

    """

    cdef readonly bint is_default_handler_registered
    """
    Return a value indicating whether the clock has a default handler registered.

    Returns
    -------
    bool
        True if default handler registered, else False.

    """

    cpdef datetime utc_now(self)
    cpdef datetime local_now(self, tzinfo tz)
    cpdef timedelta delta(self, datetime time)
    cpdef Timer timer(self, str name)
    cpdef void register_default_handler(self, handler) except *
    cpdef void set_time_alert(self, str name, datetime alert_time, handler=*) except *
    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time=*,
        datetime stop_time=*,
        handler=*,
    ) except *
    cpdef void cancel_timer(self, str name) except *
    cpdef void cancel_all_timers(self) except *

    cdef Timer _create_timer(
        self,
        str name,
        callback,
        timedelta interval,
        datetime now,
        datetime start_time,
        datetime stop_time,
    )
    cdef inline void _add_timer(self, Timer timer, handler) except *
    cdef inline void _remove_timer(self, Timer timer) except *
    cdef inline void _update_stack(self) except *
    cdef inline void _update_timing(self) except *


cdef class TestClock(Clock):
    cdef datetime _time
    cdef dict _pending_events

    cpdef void set_time(self, datetime to_time) except *
    cpdef list advance_time(self, datetime to_time)


cdef class LiveClock(Clock):
    cpdef void _raise_time_event(self, LiveTimer timer) except *

    cdef inline void _handle_time_event(self, TimeEvent event) except *
