# -------------------------------------------------------------------------------------------------
# <copyright file="clock.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.common.guid cimport GuidFactory, TestGuidFactory
from nautilus_trader.model.identifiers cimport Label
from nautilus_trader.model.events cimport TimeEvent


cdef class TimeEventHandler:
    cdef readonly TimeEvent event
    cdef object handler

    cdef void handle(self) except *


cdef class Timer:
    cdef readonly Label label
    cdef readonly object callback
    cdef readonly timedelta interval
    cdef readonly datetime start_time
    cdef readonly datetime next_time
    cdef readonly datetime stop_time
    cdef readonly expired

    cpdef TimeEvent iterate_event(self, GUID event_id, datetime now)
    cpdef void cancel(self) except *


cdef class TestTimer(Timer):
    cdef TestGuidFactory _guid_factory

    cpdef list advance(self, datetime to_time)


cdef class LiveTimer(Timer):
    cdef object _internal

    cpdef void repeat(self, datetime now) except *
    cdef object _start_timer(self, datetime now)


cdef class Clock:
    cdef LoggerAdapter _log
    cdef GuidFactory _guid_factory
    cdef dict _timers
    cdef dict _handlers
    cdef Timer[:] _stack
    cdef object _default_handler

    cdef readonly int timer_count
    cdef readonly datetime next_event_time
    cdef readonly Label next_event_label
    cdef readonly bint is_test_clock
    cdef readonly bint is_logger_registered
    cdef readonly bint is_default_handler_registered

    cpdef datetime time_now(self)
    cpdef timedelta get_delta(self, datetime time)
    cpdef list get_timer_labels(self)
    cpdef void register_logger(self, LoggerAdapter logger) except *
    cpdef void register_default_handler(self, handler) except *
    cpdef void set_time_alert(self, Label label, datetime alert_time, handler=*) except *
    cpdef void set_timer(self, Label label, timedelta interval, datetime start_time=*, datetime stop_time=*, handler=*) except *
    cpdef void cancel_timer(self, Label label) except *
    cpdef void cancel_all_timers(self) except *

    cdef object _get_timer(self, Label label, callback, timedelta interval, datetime now, datetime start_time, datetime stop_time)
    cdef void _add_timer(self, Timer timer, handler) except *
    cdef void _remove_timer(self, Timer timer) except *
    cdef void _update_stack(self) except *
    cdef void _update_timing(self) except *


cdef class TestClock(Clock):
    cdef datetime _time
    cdef dict _pending_events

    cpdef void set_time(self, datetime to_time) except *
    cpdef list advance_time(self, datetime to_time)


cdef class LiveClock(Clock):
    cpdef void _raise_time_event(self, LiveTimer timer) except *

    cdef void _handle_time_event(self, TimeEvent event) except *
