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

from libc.stdint cimport uint64_t

from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.common cimport TimeEvent_t
from nautilus_trader.core.uuid cimport UUID4


cdef class TimeEvent(Event):
    cdef TimeEvent_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef TimeEvent from_mem_c(TimeEvent_t raw)


cdef class TimeEventHandler:
    cdef object _handler
    cdef readonly TimeEvent event
    """The handlers event.\n\n:returns: `TimeEvent`"""

    cpdef void handle(self) except *


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
    cpdef void iterate_next_time(self, uint64_t to_time_ns) except *
    cpdef void cancel(self) except *
    cpdef void repeat(self, uint64_t now_ns) except *
    cdef object _start_timer(self, uint64_t now_ns)


cdef class ThreadTimer(LiveTimer):
    pass


cdef class LoopTimer(LiveTimer):
    cdef object _loop
