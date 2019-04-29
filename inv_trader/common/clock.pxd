#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="clock.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime, timedelta

from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.model.identifiers cimport Label


cdef class Clock:
    """
    The abstract base class for all clocks. All times are tz-aware UTC.
    """
    cdef LoggerAdapter _log
    cdef object _event_handler
    cdef dict _time_alerts
    cdef dict _timers

    cdef readonly is_logger_registered
    cdef readonly is_handler_registered

    cpdef void register_logger(self, LoggerAdapter logger)
    cpdef void register_handler(self, handler)
    cpdef datetime time_now(self)
    cpdef timedelta get_delta(self, datetime time)
    cpdef set_time_alert(self, Label label, datetime alert_time)
    cpdef set_timer(self, Label label, timedelta interval, datetime start_time=*, datetime stop_time=*)
    cpdef list get_time_alert_labels(self)
    cpdef list get_timer_labels(self)
    cpdef cancel_time_alert(self, Label label)
    cpdef cancel_timer(self, Label label)
    cpdef cancel_all_time_alerts(self)
    cpdef cancel_all_timers(self)


cdef class LiveClock(Clock):
    """
    Provides a clock for live trading. All times are tz-aware UTC.
    """
    cpdef void _raise_time_event(self, Label label, datetime alert_time)
    cpdef void _repeating_timer(self, Label label, datetime alert_time, timedelta interval, datetime stop_time)


cdef class TestTimer:
    """
    Provides a fake timer for backtesting and unit testing.
    """
    cdef readonly Label label
    cdef readonly timedelta interval
    cdef readonly datetime start
    cdef readonly datetime stop
    cdef readonly datetime next_alert
    cdef readonly bint expired

    cpdef list advance(self, datetime time)


cdef class TestClock(Clock):
    """
    Provides a clock for backtesting and unit testing.
    """
    cdef datetime _time

    cpdef void set_time(self, datetime time)
    cpdef dict iterate_time(self, datetime time)
