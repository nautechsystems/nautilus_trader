# -------------------------------------------------------------------------------------------------
# <copyright file="clock.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport date, datetime, timedelta

from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.identifiers cimport Label


cdef class Clock:
    cdef LoggerAdapter _log
    cdef object _event_handler
    cdef dict _timers
    cdef dict _event_times

    cdef readonly list event_times
    cdef readonly bint is_logger_registered
    cdef readonly bint is_handler_registered

    cpdef datetime time_now(self)
    cpdef timedelta get_delta(self, datetime time)
    cpdef list get_timer_labels(self)
    cpdef void register_logger(self, LoggerAdapter logger)
    cpdef void register_handler(self, handler)
    cpdef void set_time_alert(self, Label label, datetime alert_time) except *
    cpdef void set_timer(self, Label label, timedelta interval, datetime start_time=*, datetime stop_time=*) except *
    cpdef void cancel_timer(self, Label label) except *
    cpdef void cancel_all_timers(self) except *
    cpdef void _raise_time_event(self, Label label, datetime alert_time) except *
    cpdef void _repeating_timer(self, Label label, datetime alert_time, timedelta interval, datetime stop_time) except *
    cdef object _get_timer(self, Label label, datetime event_time)
    cdef object _get_timer_repeating(self, Label label, datetime next_event_time, timedelta interval, datetime stop_time)
    cdef void _add_timer(self, Label label, timer, datetime event_time)
    cdef void _sort_event_times(self)


cdef class LiveClock(Clock):
    pass


cdef class TestTimer:
    cdef readonly Label label
    cdef readonly timedelta interval
    cdef readonly datetime start
    cdef readonly datetime stop
    cdef readonly datetime next_alert
    cdef readonly bint expired

    cpdef list advance(self, datetime time)
    cpdef void cancel(self)


cdef class TestClock(Clock):
    cdef datetime _time

    cpdef void set_time(self, datetime time)
    cpdef dict iterate_time(self, datetime time)
