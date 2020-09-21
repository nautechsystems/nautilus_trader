# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from threading import Timer as TimerThread

import pytz

from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.live.factories cimport LiveUUIDFactory


cdef class LiveTimer(Timer):
    """
    Provides a timer for live trading.
    """

    def __init__(self,
                 str name not None,
                 callback not None,
                 timedelta interval not None,
                 datetime now not None,
                 datetime start_time not None,
                 datetime stop_time=None):
        """
        Initialize a new instance of the LiveTimer class.

        Parameters
        ----------
        name : str
            The name for the timer.
        callback : callable
            The function to call at the next time.
        interval : timedelta
            The time interval for the timer.
        now : datetime
            The datetime now (UTC).
        start_time : datetime
            The start datetime for the timer (UTC).
        stop_time : datetime, optional
            The stop datetime for the timer (UTC) (if None then timer repeats).

        Raises
        ------
        TypeError
            If callback is not of type callable.

        """
        Condition.valid_string(name, "name")
        super().__init__(name, callback, interval, start_time, stop_time)

        self._internal = self._start_timer(now)

    cpdef void repeat(self, datetime now) except *:
        """
        Continue the timer.

        Parameters
        ----------
        now : datetime
            The current time to base the repeat from.

        """
        Condition.not_none(now, "now")

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
        Initialize a new instance of the LiveClock class.
        """
        super().__init__(LiveUUIDFactory())  # Instantiation inside constructor?

    cpdef datetime utc_now(self):
        """
        Return the current datetime of the clock (UTC).

        :return datetime.
        """
        # From the pytz docs https://pythonhosted.org/pytz/
        # -------------------------------------------------
        # Unfortunately using the tzinfo argument of the standard datetime
        # constructors ‘’does not work’’ with pytz for many timezones.
        # It is safe for timezones without daylight saving transitions though,
        # such as UTC. The preferred way of dealing with times is to always work
        # in UTC, converting to localtime only when generating output to be read
        # by humans.
        return datetime.now(tz=pytz.utc)

    cdef Timer _get_timer(
            self,
            str name,
            callback,
            timedelta interval,
            datetime now,
            datetime start_time,
            datetime stop_time):
        return LiveTimer(
            name=name,
            callback=self._raise_time_event,
            interval=interval,
            now=now,
            start_time=start_time,
            stop_time=stop_time)

    cpdef void _raise_time_event(self, LiveTimer timer) except *:
        cdef datetime now = self.utc_now()
        cdef TimeEvent event = timer.pop_event(self._uuid_factory.generate())
        timer.iterate_next_time(now)
        self._handle_time_event(event)

        if timer.expired:
            self._remove_timer(timer)
        else:  # Continue timing
            timer.repeat(now)
            self._update_timing()

    cdef void _handle_time_event(self, TimeEvent event) except *:
        handler = self._handlers.get(event.name)
        if handler:
            handler(event)
