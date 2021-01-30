# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import cython
import numpy as np
import pytz

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from cpython.datetime cimport tzinfo

from nautilus_trader.common.timer cimport TestTimer
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.common.timer cimport ThreadTimer
from nautilus_trader.common.timer cimport LoopTimer
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport UNIX_EPOCH
from nautilus_trader.core.time cimport unix_time


cdef class Clock:
    """
    The abstract base class for all clocks.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(self):
        """
        Initialize a new instance of the `Clock` class.
        """
        self._uuid_factory = UUIDFactory()
        self._timers = {}    # type: dict[str, Timer]
        self._handlers = {}  # type: dict[str, callable]
        self._stack = None
        self._default_handler = None
        self.is_test_clock = False
        self.is_default_handler_registered = False

        self.timer_count = 0
        self.next_event_time = None
        self.next_event_name = None

    cpdef datetime utc_now(self):
        """Abstract method (implement in subclass)."""
        # As the method implies, this should return a tz-aware UTC datetime
        raise NotImplementedError("method must be implemented in the subclass")

    cdef datetime utc_now_c(self):
        """Abstract method (implement in subclass)."""
        # As the method implies, this should return a tz-aware UTC datetime
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef datetime local_now(self, tzinfo tz):
        """
        Return the current datetime of the clock in the given local timezone.

        Parameters
        ----------
        tz : tzinfo
            The local timezone.

        Returns
        -------
        datetime
            tz-aware as local timezone.

        """
        return self.utc_now_c().astimezone(tz)

    cpdef timedelta delta(self, datetime time):
        """
        Return the timedelta from the current time to the given time.

        Parameters
        ----------
        time : datetime
            The datum time.

        Returns
        -------
        timedelta
            The time difference.

        """
        Condition.not_none(time, "time")

        return self.utc_now_c() - time

    cpdef double unix_time(self):
        """
        Return the current Unix time in seconds from the system clock.

        Returns
        -------
        double

        """
        return unix_time()

    cpdef list timer_names(self):
        """
        The timer names held by the clock.

        Returns
        -------
        list[str]

        """
        return list(self._timers.keys())

    cpdef Timer timer(self, str name):
        """
        Find a particular timer.

        Parameters
        ----------
        name : str
            The name of the timer.

        Returns
        -------
        Timer or None
            The timer with the given name (if found).

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        Condition.valid_string(name, "name")

        return self._timers.get(name)

    cpdef void register_default_handler(self, handler: callable) except *:
        """
        Register the given handler as the clocks default handler.

        handler : callable
            The handler to register.

        Raises
        ------
        TypeError
            If handler is not of type callable.

        """
        Condition.not_none(handler, "handler")

        self._default_handler = handler
        self.is_default_handler_registered = True

    cpdef void set_time_alert(
        self,
        str name,
        datetime alert_time,
        handler: callable=None,
    ) except *:
        """
        Set a time alert for the given time.

        When the time is reached the handler will be passed the `TimeEvent`
        containing the timers unique name. If no handler is passed then the
        default handler (if registered) will receive the `TimeEvent`.

        Parameters
        ----------
        name : str
            The name for the alert (must be unique for this clock).
        alert_time : datetime
            The time for the alert.
        handler : callable, optional
            The handler to receive time events.

        Raises
        ------
        ValueError
            If name is not unique for this clock.
        ValueError
            If alert_time is not >= the clocks current time.
        TypeError
            If handler is not of type callable or None.
        ValueError
            If handler is None and no default handler is registered.

        """
        Condition.not_none(name, "name")
        Condition.not_none(alert_time, "alert_time")
        if handler is None:
            handler = self._default_handler
        Condition.not_in(name, self._timers, "name", "timers")
        Condition.not_in(name, self._handlers, "name", "timers")
        cdef datetime now = self.utc_now_c()
        Condition.true(alert_time >= now, "alert_time >= time_now()")
        Condition.callable(handler, "handler")

        cdef Timer timer = self._create_timer(
            name=name,
            callback=handler,
            interval=alert_time - now,
            now=now,
            start_time=now,
            stop_time=alert_time,
        )

        self._add_timer(timer, handler)

    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time=None,
        datetime stop_time=None,
        handler: callable=None,
    ) except *:
        """
        Set a timer with the given interval.

        The timer will run from the start time (optionally until the stop time).
        When the intervals are reached the handlers will be passed the
        `TimeEvent` containing the timers unique name. If no handler is passed
        then the default handler (if registered) will receive the `TimeEvent`.

        Parameters
        ----------
        name : str
            The name for the timer (must be unique for this clock).
        interval : timedelta
            The time interval for the timer.
        start_time : datetime, optional
            The start time for the timer (if None then starts immediately).
        stop_time : datetime, optional
            The stop time for the timer (if None then repeats indefinitely).
        handler : callable, optional
            The handler to receive time events.

        Raises
        ------
        ValueError
            If name is not unique for this clock.
        ValueError
            If interval is not positive (> 0).
        ValueError
            If stop_time is not None and stop_time < time_now.
        ValueError
            If stop_time is not None and start_time + interval > stop_time.
        TypeError
            If handler is not of type callable or None.
        ValueError
            If handler is None and no default handler is registered.

        """
        Condition.valid_string(name, "name")
        Condition.not_none(interval, "interval")
        if handler is None:
            handler = self._default_handler
        Condition.not_in(name, self._timers, "name", "timers")
        Condition.not_in(name, self._handlers, "name", "timers")
        Condition.true(interval.total_seconds() > 0, "interval positive")
        Condition.callable(handler, "handler")

        cdef datetime now = self.utc_now_c()
        if start_time is None:
            start_time = now
        if stop_time is not None:
            Condition.true(stop_time > now, "stop_time > now")
            Condition.true(start_time + interval <= stop_time, "start_time + interval <= stop_time")

        cdef Timer timer = self._create_timer(
            name=name,
            interval=interval,
            callback=handler,
            now=now,
            start_time=start_time,
            stop_time=stop_time,
        )
        self._add_timer(timer, handler)

    cpdef void cancel_timer(self, str name) except *:
        """
        Cancel the timer corresponding to the given label.

        Parameters
        ----------
        name : str
            The name for the timer to cancel.

        Notes
        -----
        Logs a warning if a timer with the given name is not found (it may have
        already been cancelled).

        """
        Condition.valid_string(name, "name")

        cdef Timer timer = self._timers.pop(name, None)
        if not timer:
            # No timer with given name
            return

        timer.cancel()
        self._handlers.pop(name, None)
        self._remove_timer(timer)

    cpdef void cancel_timers(self) except *:
        """
        Cancel all timers.
        """
        cdef str name
        for name in self.timer_names():
            # Using a list of timer names from the property and passing this
            # to cancel_timer() handles the clean removal of both the handler
            # and timer.
            self.cancel_timer(name)

    cdef Timer _create_timer(
        self,
        str name,
        callback: callable,
        timedelta interval,
        datetime now,
        datetime start_time,
        datetime stop_time,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cdef inline void _add_timer(self, Timer timer, handler: callable) except *:
        self._timers[timer.name] = timer
        self._handlers[timer.name] = handler
        self._update_stack()
        self._update_timing()

    cdef inline void _remove_timer(self, Timer timer) except *:
        self._timers.pop(timer.name, None)
        self._handlers.pop(timer.name, None)
        self._update_stack()
        self._update_timing()

    cdef void _update_stack(self) except *:
        self.timer_count = len(self._timers)

        if self.timer_count > 0:
            # The call to np.asarray here looks inefficient, however its only
            # called when a timer is added or removed. This then allows the
            # construction of an efficient Timer[:] memoryview.
            self._stack = np.asarray(list(self._timers.values()))
        else:
            self._stack = None

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef inline void _update_timing(self) except *:
        if self.timer_count == 0:
            self.next_event_time = None
            return
        elif self.timer_count == 1:
            self.next_event_time = self._stack[0].next_time
            return

        cdef datetime next_time = self._stack[0].next_time
        cdef datetime observed
        cdef int i
        for i in range(self.timer_count - 1):
            observed = self._stack[i + 1].next_time
            if observed < next_time:
                next_time = observed

        self.next_event_time = next_time


cdef class TestClock(Clock):
    """
    Provides a monotonic clock for backtesting and unit testing.
    """
    __test__ = False

    def __init__(self, datetime initial_time not None=UNIX_EPOCH):
        """
        Initialize a new instance of the `TestClock` class.

        Parameters
        ----------
        initial_time : datetime
            The initial time for the clock.

        """
        super().__init__()

        self._time = initial_time
        self.is_test_clock = True

    cpdef datetime utc_now(self):
        """
        Returns
        -------
        datetime
            The current tz-aware UTC time of the clock.

        """
        return self._time

    cdef datetime utc_now_c(self):
        return self._time

    cpdef void set_time(self, datetime to_time) except *:
        """
        Set the clocks datetime to the given time (UTC).

        Parameters
        ----------
        to_time : datetime
            The time to set.

        """
        Condition.not_none(to_time, "to_time")

        self._time = to_time

    cpdef list advance_time(self, datetime to_time):
        """
        Advance the clocks time to the given `datetime`.

        Parameters
        ----------
        to_time : datetime
            The datetime to advance the clock to.

        Returns
        -------
        list[TimeEvent]
            Sorted chronologically.

        Raises
        ------
        ValueError
            If to_time is < the clocks current time.

        """
        Condition.not_none(to_time, "to_time")
        Condition.true(to_time >= self._time, "to_time >= self._time")  # Ensure monotonic

        cdef list event_handlers = []

        if self.timer_count == 0 or to_time < self.next_event_time:
            self._time = to_time
            return event_handlers  # No timer events to iterate

        # Iterate timer events
        cdef TestTimer timer
        cdef TimeEvent event
        for timer in self._stack:
            for event in timer.advance(to_time):
                event_handlers.append(TimeEventHandler(event, timer.callback))

        # Remove expired timers
        for timer in self._stack:
            if timer.expired:
                self._remove_timer(timer)

        self._update_timing()
        self._time = to_time
        return sorted(event_handlers)

    cdef Timer _create_timer(
        self,
        str name,
        callback: callable,
        timedelta interval,
        datetime now,
        datetime start_time,
        datetime stop_time,
    ):
        return TestTimer(
            name=name,
            callback=callback,
            interval=interval,
            start_time=start_time,
            stop_time=stop_time,
        )


cdef class LiveClock(Clock):
    """
    Provides a clock for live trading. All times are timezone aware UTC.
    """

    def __init__(self, loop=None):
        """
        Initialize a new instance of the `LiveClock` class.

        If loop is None then threads will be used for timers.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the clocks timers.

        """
        super().__init__()
        self._loop = loop
        self._utc = pytz.utc

    cpdef datetime utc_now(self):
        """
        Returns
        -------
        datetime
            The current tz-aware UTC time of the clock.

        """
        return self.utc_now_c()

    cdef datetime utc_now_c(self):
        # Regarding the below call to pytz.utc
        # From the pytz docs https://pythonhosted.org/pytz/
        # -------------------------------------------------
        # Unfortunately using the tzinfo argument of the standard datetime
        # constructors ‘’does not work’’ with pytz for many timezones.
        # It is safe for timezones without daylight saving transitions though,
        # such as UTC. The preferred way of dealing with times is to always work
        # in UTC, converting to localtime only when generating output to be read
        # by humans.
        return datetime.now(tz=self._utc)

    cdef Timer _create_timer(
        self,
        str name,
        callback: callable,
        timedelta interval,
        datetime now,
        datetime start_time,
        datetime stop_time,
    ):
        if self._loop is not None:
            return LoopTimer(
                loop=self._loop,
                name=name,
                callback=self._raise_time_event,
                interval=interval,
                now=now,
                start_time=start_time,
                stop_time=stop_time,
            )
        else:
            return ThreadTimer(
                name=name,
                callback=self._raise_time_event,
                interval=interval,
                now=now,
                start_time=start_time,
                stop_time=stop_time,
            )

    cpdef void _raise_time_event(self, LiveTimer timer) except *:
        cdef datetime now = self.utc_now_c()
        cdef TimeEvent event = timer.pop_event(self._uuid_factory.generate())
        timer.iterate_next_time(now)
        self._handle_time_event(event)

        if timer.expired:
            self._remove_timer(timer)
        else:  # Continue timing
            timer.repeat(now)
            self._update_timing()

    cdef inline void _handle_time_event(self, TimeEvent event) except *:
        handler = self._handlers.get(event.name)
        if handler is not None:
            handler(event)
