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
from libc.stdint cimport int64_t

from nautilus_trader.common.timer cimport LoopTimer
from nautilus_trader.common.timer cimport TestTimer
from nautilus_trader.common.timer cimport ThreadTimer
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport nanos_to_secs
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
from nautilus_trader.core.time cimport unix_timestamp
from nautilus_trader.core.time cimport unix_timestamp_ns

from nautilus_trader.core.datetime import timedelta_to_nanos


cdef class Clock:
    """
    The abstract base class for all clocks.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``Clock`` class.
        """
        self._uuid_factory = UUIDFactory()
        self._timers = {}    # type: dict[str, Timer]
        self._handlers = {}  # type: dict[str, callable]
        self._stack = None
        self._default_handler = None
        self.is_test_clock = False
        self.is_default_handler_registered = False

        self.timer_count = 0
        self.next_event_name = None
        self.next_event_time = None
        self.next_event_time_ns = 0

    cpdef double timestamp(self) except *:
        """
        Return the current UNIX time in seconds.

        Returns
        -------
        double

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int64_t timestamp_ns(self) except *:
        """
        Return the current UNIX time in nanoseconds.

        Returns
        -------
        int64

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef datetime utc_now(self):
        """
        Return the current time (UTC).

        Returns
        -------
        datetime
            The current tz-aware UTC time of the clock.

        """
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef datetime local_now(self, tzinfo tz=None):
        """
        Return the current datetime of the clock in the given local timezone.

        Parameters
        ----------
        tz : tzinfo, optional
            The local timezone (if None the system local timezone is assumed for
            the target timezone).

        Returns
        -------
        datetime
            tz-aware in local timezone.

        """
        return self.utc_now().astimezone(tz)

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

        return self.utc_now() - time

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
        Condition.true(alert_time >= self.utc_now(), "alert_time was < self.utc_now()")
        Condition.callable(handler, "handler")

        cdef int64_t alert_time_ns = dt_to_unix_nanos(alert_time)
        cdef int64_t now_ns = self.timestamp_ns()

        cdef Timer timer = self._create_timer(
            name=name,
            callback=handler,
            interval_ns=alert_time_ns - now_ns,
            start_time_ns=now_ns,
            stop_time_ns=alert_time_ns,
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
        Set a timer to run.

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
        cdef datetime now = self.utc_now()  # Call here for greater accuracy

        Condition.valid_string(name, "name")
        Condition.not_none(interval, "interval")
        if handler is None:
            handler = self._default_handler
        Condition.not_in(name, self._timers, "name", "timers")
        Condition.not_in(name, self._handlers, "name", "timers")
        Condition.true(interval.total_seconds() > 0, f"interval was {interval.total_seconds()}")
        Condition.callable(handler, "handler")

        if start_time is None:
            start_time = now
        if stop_time is not None:
            Condition.true(stop_time > now, "stop_time was < now")
            Condition.true(start_time + interval <= stop_time, "start_time + interval was > stop_time")

        cdef int64_t interval_ns = timedelta_to_nanos(interval)
        cdef int64_t start_time_ns = dt_to_unix_nanos(start_time)
        cdef int64_t stop_time_ns = dt_to_unix_nanos(stop_time) if stop_time else 0

        cdef Timer timer = self._create_timer(
            name=name,
            interval_ns=interval_ns,
            callback=handler,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
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
        already been canceled).

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
        int64_t interval_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cdef void _add_timer(self, Timer timer, handler: callable) except *:
        self._timers[timer.name] = timer
        self._handlers[timer.name] = handler
        self._update_stack()
        self._update_timing()

    cdef void _remove_timer(self, Timer timer) except *:
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
    cdef void _update_timing(self) except *:
        if self.timer_count == 0:
            self.next_event_time_ns = 0
            return
        elif self.timer_count == 1:
            self.next_event_time_ns = self._stack[0].next_time_ns
            return

        cdef int64_t next_time_ns = self._stack[0].next_time_ns
        cdef int64_t observed_ns
        cdef int i
        for i in range(self.timer_count - 1):
            observed_ns = self._stack[i + 1].next_time_ns
            if observed_ns < next_time_ns:
                next_time_ns = observed_ns

        self.next_event_time_ns = next_time_ns


cdef class TestClock(Clock):
    """
    Provides a monotonic clock for backtesting and unit testing.
    """
    __test__ = False

    def __init__(self, int64_t initial_ns=0):
        """
        Initialize a new instance of the ``TestClock`` class.

        Parameters
        ----------
        initial_ns : int64
            The initial UNIX time for the clock (nanos).

        """
        super().__init__()

        self._time_ns = initial_ns
        self.is_test_clock = True

    cpdef datetime utc_now(self):
        """
        Return the current time (UTC).

        Returns
        -------
        datetime
            The current tz-aware UTC time of the clock.

        """
        return nanos_to_unix_dt(nanos=self._time_ns)

    cpdef double timestamp(self) except *:
        """
        Return the current UNIX time in seconds.

        Returns
        -------
        double

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        return nanos_to_secs(self._time_ns)

    cpdef int64_t timestamp_ns(self) except *:
        """
        Return the current UNIX time in nanoseconds (ns).

        Returns
        -------
        int64

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        return self._time_ns

    cpdef void set_time(self, int64_t to_time_ns) except *:
        """
        Set the clocks datetime to the given time (UTC).

        Parameters
        ----------
        to_time_ns : int64
            The UNIX time (nanos) to set.

        """
        self._time_ns = to_time_ns

    cpdef list advance_time(self, int64_t to_time_ns):
        """
        Advance the clocks time to the given `datetime`.

        Parameters
        ----------
        to_time_ns : int64
            The UNIX time (nanos) advance the clock to.

        Returns
        -------
        list[TimeEvent]
            Sorted chronologically.

        Raises
        ------
        ValueError
            If to_time is < the clocks current time.

        """
        # Ensure monotonic
        Condition.true(to_time_ns >= self._time_ns, "to_time_ns was < self._time_ns")

        cdef list event_handlers = []

        if self.timer_count == 0 or to_time_ns < self.next_event_time_ns:
            self._time_ns = to_time_ns
            return event_handlers  # No timer events to iterate

        # Iterate timer events
        cdef TestTimer timer
        cdef TimeEvent event
        for timer in self._stack:
            for event in timer.advance(to_time_ns=to_time_ns):
                event_handlers.append(TimeEventHandler(event, timer.callback))

        # Remove expired timers
        for timer in self._stack:
            if timer.is_expired:
                self._remove_timer(timer)

        self._update_timing()
        self._time_ns = to_time_ns
        return sorted(event_handlers)

    cdef Timer _create_timer(
        self,
        str name,
        callback: callable,
        int64_t interval_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns,
    ):
        return TestTimer(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )


cdef class LiveClock(Clock):
    """
    Provides a clock for live trading. All times are timezone aware UTC.
    """

    def __init__(self, loop=None):
        """
        Initialize a new instance of the ``LiveClock`` class.

        If loop is None then threads will be used for timers.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the clocks timers.

        """
        super().__init__()

        self._loop = loop
        self._utc = pytz.utc

    cpdef double timestamp(self) except *:
        """
        Return the current UNIX time in seconds from the system clock.

        Returns
        -------
        double

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        return unix_timestamp()

    cpdef int64_t timestamp_ns(self) except *:
        """
        Return the current UNIX time in nanoseconds (ns) from the system clock.

        Returns
        -------
        int64

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        return unix_timestamp_ns()

    cpdef datetime utc_now(self):
        """
        Return the current time (UTC).

        Returns
        -------
        datetime
            The current tz-aware UTC time of the clock.

        """
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
        int64_t interval_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns,
    ):
        if self._loop is not None:
            return LoopTimer(
                loop=self._loop,
                name=name,
                callback=self._raise_time_event,
                interval_ns=interval_ns,
                now_ns=self.timestamp_ns(),  # Timestamp now here for accuracy
                start_time_ns=start_time_ns,
                stop_time_ns=stop_time_ns,
            )
        else:
            return ThreadTimer(
                name=name,
                callback=self._raise_time_event,
                interval_ns=interval_ns,
                now_ns=self.timestamp_ns(),  # Timestamp now here for accuracy
                start_time_ns=start_time_ns,
                stop_time_ns=stop_time_ns,
            )

    cpdef void _raise_time_event(self, LiveTimer timer) except *:
        cdef int64_t timestamp_ns = self.timestamp_ns()
        cdef TimeEvent event = timer.pop_event(
            event_id=self._uuid_factory.generate(),
            timestamp_ns=timestamp_ns,
        )

        timer.iterate_next_time(self.timestamp_ns())
        self._handle_time_event(event)

        if timer.is_expired:
            self._remove_timer(timer)
        else:  # Continue timing
            timer.repeat(now_ns=self.timestamp_ns())
            self._update_timing()

    cdef void _handle_time_event(self, TimeEvent event) except *:
        handler = self._handlers.get(event.name)
        if handler is not None:
            handler(event)
