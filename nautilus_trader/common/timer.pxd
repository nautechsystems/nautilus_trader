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

from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID


cdef class TimeEvent(Event):
    cdef readonly str name
    """The time events unique name.\n\n:returns: `str`"""


cdef class TimeEventHandler:
    cdef object _handler
    cdef readonly TimeEvent event
    """The handlers event.\n\n:returns: `TimeEvent`"""

    cdef void handle(self) except *


cdef class Timer:
    cdef readonly str name
    """The timers name using for hashing.\n\n:returns: `str`"""
    cdef readonly object callback
    """The timers callback function.\n\n:returns: `object`"""
    cdef readonly timedelta interval
    """The timers set interval.\n\n:returns: `timedelta`"""
    cdef readonly datetime start_time
    """The timers set start time.\n\n:returns: `datetime`"""
    cdef readonly datetime next_time
    """The timers next alert timestamp.\n\n:returns: `datetime`"""
    cdef readonly datetime stop_time
    """The timers set stop time (if set).\n\n:returns: `datetime`"""
    cdef readonly bint expired
    """If the timer is expired.\n\n:returns: `bool`"""

    cpdef TimeEvent pop_event(self, UUID event_id)
    cpdef void iterate_next_time(self, datetime now) except *
    cpdef void cancel(self) except *


cdef class TestTimer(Timer):
    cdef UUIDFactory _uuid_factory

    cpdef Event pop_next_event(self)
    cpdef list advance(self, datetime to_time)


cdef class LiveTimer(Timer):
    cdef object _internal

    cpdef void repeat(self, datetime now) except *
    cdef object _start_timer(self, datetime now)
