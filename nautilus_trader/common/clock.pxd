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

from nautilus_trader.common.logging cimport LoggerAdapter
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
    cdef readonly datetime next_event_time
    cdef readonly str next_event_name
    cdef readonly bint is_test_clock
    cdef readonly bint is_default_handler_registered

    cpdef datetime time_now(self)
    cpdef timedelta get_delta(self, datetime time)
    cpdef Timer get_timer(self, str name)
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
