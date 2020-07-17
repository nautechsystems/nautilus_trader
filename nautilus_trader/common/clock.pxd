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

from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.core.message cimport Event
from nautilus_trader.common.uuid cimport UUIDFactory, TestUUIDFactory
from nautilus_trader.common.logging cimport LoggerAdapter


cdef class TimeEvent(Event):
    cdef readonly str name


cdef class TimeEventHandler:
    cdef readonly TimeEvent event
    cdef object handler

    cdef void handle(self) except *


cdef class Timer:
    cdef readonly str name
    cdef readonly object callback
    cdef readonly timedelta interval
    cdef readonly datetime start_time
    cdef readonly datetime next_time
    cdef readonly datetime stop_time
    cdef readonly expired

    cpdef TimeEvent pop_event(self, UUID event_id)
    cpdef void iterate_next_time(self, datetime now) except *
    cpdef void cancel(self) except *


cdef class TestTimer(Timer):
    cdef TestUUIDFactory _uuid_factory

    cpdef list advance(self, datetime to_time)


cdef class Clock:
    cdef LoggerAdapter _log
    cdef UUIDFactory _uuid_factory
    cdef dict _timers
    cdef dict _handlers
    cdef Timer[:] _stack
    cdef object _default_handler

    cdef readonly int timer_count
    cdef readonly datetime next_event_time
    cdef readonly str next_event_name
    cdef readonly bint is_test_clock
    cdef readonly bint is_default_handler_registered

    cpdef datetime time_now(self)
    cpdef timedelta get_delta(self, datetime time)
    cpdef list get_timer_names(self)
    cpdef void register_default_handler(self, handler) except *
    cpdef void set_time_alert(self, str name, datetime alert_time, handler=*) except *
    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time=*,
        datetime stop_time=*,
        handler=*) except *
    cpdef void cancel_timer(self, str name) except *
    cpdef void cancel_all_timers(self) except *

    cdef Timer _get_timer(
        self,
        str name,
        callback,
        timedelta interval,
        datetime now,
        datetime start_time,
        datetime stop_time)
    cdef void _add_timer(self, Timer timer, handler) except *
    cdef void _remove_timer(self, Timer timer) except *
    cdef void _update_stack(self) except *
    cdef void _update_timing(self) except *


cdef class TestClock(Clock):
    cdef datetime _time
    cdef dict _pending_events

    cpdef void set_time(self, datetime to_time) except *
    cpdef list advance_time(self, datetime to_time)
