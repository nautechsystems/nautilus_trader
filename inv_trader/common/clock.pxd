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
from inv_trader.model.identifiers cimport Label


cdef class Clock:
    """
    The abstract base class for all clocks.
    """
    cdef readonly object timezone
    cdef datetime _unix_epoch
    cdef dict _timers

    cpdef datetime time_now(self)
    cpdef datetime unix_epoch(self)
    cpdef float get_elapsed(self, datetime start)
    cdef str get_datetime_tag(self)
    cpdef set_time_alert(
            self,
            Label label,
            datetime alert_time,
            handler)
    cpdef cancel_time_alert(self, Label label)
    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time,
            datetime stop_time,
            bint repeat,
            handler)
    cpdef cancel_timer(self, Label label)
    cpdef list get_labels(self)
    cpdef stop_all_timers(self)


cdef class LiveClock(Clock):
    """
    Implements a clock for live trading.
    """
    cpdef void _raise_time_event(
            self,
            Label label,
            datetime alert_time)
    cpdef void _repeating_timer(
            self,
            Label label,
            datetime alert_time,
            timedelta interval,
            datetime stop_time)


cdef class TestTimer:
    """
    Implements a fake timer for backtesting and unit testing.
    """
    cdef readonly Label label
    cdef readonly datetime start
    cdef readonly datetime stop
    cdef readonly timedelta interval
    cdef readonly datetime next_alert
    cdef readonly object handler
    cdef readonly bint repeating
    cdef readonly bint expired

    cpdef void advance(self, datetime time)


cdef class TestClock(Clock):
    """
    Implements a clock for backtesting and unit testing.
    """
    cdef readonly timedelta time_step
    cdef datetime _time
    cdef dict _time_alerts

    cpdef void set_time(self, datetime time)
    cpdef void iterate_time(self, datetime time)
