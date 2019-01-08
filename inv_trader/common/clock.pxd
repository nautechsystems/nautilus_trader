#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="clock.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime, timedelta


cdef class Clock:
    """
    The abstract base class for all clocks.
    """
    cdef object _timezone
    cdef datetime _unix_epoch

    cpdef object get_timezone(self)
    cpdef datetime time_now(self)
    cpdef datetime unix_epoch(self)
    cdef long milliseconds_since_unix_epoch(self)


cdef class LiveClock(Clock):
    """
    Implements a clock for live trading.
    """
    pass


cdef class TestClock(Clock):
    """
    Implements a clock for backtesting and unit testing.
    """
    cdef datetime _time
    cdef readonly timedelta time_step

    cpdef void increment_time(self)
    cpdef void set_time(self, datetime time)