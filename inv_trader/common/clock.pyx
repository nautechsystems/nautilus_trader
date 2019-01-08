#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="clock.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from datetime import timezone
from cpython.datetime cimport datetime, timedelta

from inv_trader.core.precondition cimport Precondition

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
cdef datetime UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)
cdef int MILLISECONDS_PER_SECOND = 1000


cdef class Clock:
    """
    The abstract base class for all clocks. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the Clock class.
        """
        self.timezone = timezone.utc
        self._unix_epoch = UNIX_EPOCH

    cpdef datetime time_now(self):
        """
        :return: The current time of the clock.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the data client.")

    cpdef datetime unix_epoch(self):
        """
        Unix time (also known as POSIX time or epoch time) is a system for
        describing instants in time, defined as the number of seconds that have
        elapsed since 00:00:00 Coordinated Universal Time (UTC), on Thursday,
        1 January 1970, minus the number of leap seconds which have taken place
        since then.
        
        :return: The time at the unix epoch (00:00:00 on 1/1/1970 UTC).
        """
        return self._unix_epoch

    cdef long milliseconds_since_unix_epoch(self):
        """
        :return:  Returns the number of ticks of the given time now since the Unix Epoch.
        """
        return (self.time_now() - self._unix_epoch).total_seconds() * MILLISECONDS_PER_SECOND


cdef class LiveClock(Clock):
    """
    Implements a clock for live trading.
    """

    def __init__(self):
        """
        Initializes a new instance of the LiveClock class.
        """
        super().__init__()

    cpdef datetime time_now(self):
        """
        :return: The current time of the clock.
        """
        return datetime.now(self.timezone)


cdef class TestClock(Clock):
    """
    Implements a clock for backtesting and unit testing.
    """

    def __init__(self,
                 datetime initial_time=UNIX_EPOCH,
                 timedelta time_step=timedelta(seconds=1)):
        """
        Initializes a new instance of the TestClock class.

        :param initial_time: The initial time for the clock.
        """
        super().__init__()
        self._time = initial_time
        self.time_step = time_step

    cpdef datetime time_now(self):
        """
        :return: The current time of the clock.
        """
        return self._time

    cpdef void increment_time(self):
        """
        Increment the clock by the internal time step.
        """
        self._time += self.time_step

    cpdef void set_time(self, datetime time):
        """
        Set the clocks internal time with the given time.
        
        :raises ValueError: If the given times timezone is not UTC.
        """
        Precondition.equal(time.tzinfo, self.timezone)

        self._time = time
