# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport date, datetime, timedelta
from datetime import timezone
from threading import Timer as TimerThread

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport Label
from nautilus_trader.common.clock cimport TimeEvent
from nautilus_trader.live.guid cimport LiveGuidFactory

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
_UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)


cdef class LiveTimer(Timer):
    """
    Provides a timer for live trading.
    """

    def __init__(self,
                 Label label not None,
                 callback not None,
                 timedelta interval not None,
                 datetime now not None,
                 datetime start_time not None,
                 datetime stop_time=None):
        """
        Initializes a new instance of the LiveTimer class.

        :param label: The label for the timer.
        :param callback: The function to call at the next time.
        :param interval: The time interval for the timer.
        :param now: The datetime now (UTC).
        :param start_time: The start datetime for the timer (UTC).
        :param stop_time: The optional stop datetime for the timer (UTC) (if None then timer repeats).
        :raises TypeError: If the function is not of type callable.
        """
        super().__init__(label, callback, interval, start_time, stop_time)

        self._internal = self._start_timer(now)

    cpdef void repeat(self, datetime now) except *:
        """
        Continue the timer.
        """
        Condition.not_none(now, 'now')

        self._internal = self._start_timer(now)

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self._internal.cancel()

    cdef object _start_timer(self, datetime now):
        timer = TimerThread(
            interval=(self.next_time - now).total_seconds(),
            function=self.callback,
            args=[self])
        timer.daemon = True
        timer.start()

        return timer


cdef class LiveClock(Clock):
    """
    Provides a clock for live trading. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the LiveClock class.
        """
        super().__init__(LiveGuidFactory())

    cpdef date date_now(self):
        """
        Return the current date of the clock (UTC).
        
        :return date.
        """
        return datetime.now(timezone.utc).date()

    cpdef datetime time_now(self):
        """
        Return the current datetime of the clock (UTC).
        
        :return datetime.
        """
        return datetime.now(timezone.utc)

    cdef object _get_timer(
            self,
            Label label,
            callback,
            timedelta interval,
            datetime now,
            datetime start_time,
            datetime stop_time):
        return LiveTimer(
            label=label,
            callback=self._raise_time_event,
            interval=interval,
            now=now,
            start_time=start_time,
            stop_time=stop_time)

    cpdef void _raise_time_event(self, LiveTimer timer) except *:
        cdef datetime now = self.time_now()
        cdef TimeEvent event = timer.pop_event(self._guid_factory.generate())
        timer.iterate_next_time(now)
        self._handle_time_event(event)

        if timer.expired:
            self._remove_timer(timer)
        else:  # Continue timing
            timer.repeat(now)
            self._update_timing()

    cdef void _handle_time_event(self, TimeEvent event) except *:
        handler = self._handlers.get(event.label)
        if handler:
            handler(event)
