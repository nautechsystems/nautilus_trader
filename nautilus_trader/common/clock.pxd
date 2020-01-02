# -------------------------------------------------------------------------------------------------
# <copyright file="clock.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.identifiers cimport Label
from nautilus_trader.model.events cimport TimeEvent


cdef class Clock:
    cdef LoggerAdapter _log
    cdef dict _timers
    cdef dict _handlers
    cdef object _default_handler

    cdef readonly bint is_logger_registered
    cdef readonly bint is_default_handler_registered

    cpdef datetime time_now(self)
    cpdef timedelta get_delta(self, datetime time)
    cpdef list get_timer_labels(self)
    cpdef void register_logger(self, LoggerAdapter logger)
    cpdef void register_default_handler(self, handler)
    cpdef void set_time_alert(self, Label label, datetime alert_time, handler=*) except *
    cpdef void set_timer(self, Label label, timedelta interval, datetime start_time=*, datetime stop_time=*, handler=*) except *
    cpdef void cancel_timer(self, Label label) except *
    cpdef void cancel_all_timers(self) except *

    cdef object _get_timer(self, Label label, datetime event_time)
    cdef object _get_timer_repeating(self, Label label, timedelta interval, datetime next_time, datetime stop_time)
    cdef void _add_timer(self, Label label, timer, handler) except *
    cdef void _remove_timer(self, Label label) except *


cdef class LiveClock(Clock):
    cpdef void _raise_time_event(self, Label label, datetime alert_time) except *
    cpdef void _raise_time_event_repeating(self, Label label, timedelta interval, datetime next_time, datetime stop_time) except *

    cdef void _handle_time_event(self, TimeEvent event) except *


cdef class TestTimer:
    cdef readonly Label label
    cdef readonly timedelta interval
    cdef readonly datetime start
    cdef readonly datetime stop
    cdef readonly datetime next_event
    cdef readonly expired

    cpdef list advance(self, datetime to_time)
    cpdef void cancel(self)


cdef class TestClock(Clock):
    cdef datetime _time

    cpdef void set_time(self, datetime to_time)
    cpdef dict advance_time(self, datetime to_time)
