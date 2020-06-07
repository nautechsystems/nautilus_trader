# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock, Timer, TimeEvent


cdef class LiveTimer(Timer):
    cdef object _internal

    cpdef void repeat(self, datetime now) except *
    cdef object _start_timer(self, datetime now)


cdef class LiveClock(Clock):
    cpdef void _raise_time_event(self, LiveTimer timer) except *

    cdef void _handle_time_event(self, TimeEvent event) except *
