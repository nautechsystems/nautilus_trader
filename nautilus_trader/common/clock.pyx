# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Callable

import cython
import numpy as np
import pandas as pd

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from cpython.datetime cimport tzinfo
from libc.stdint cimport uint64_t

from nautilus_trader.common.timer cimport LoopTimer
from nautilus_trader.common.timer cimport TestTimer
from nautilus_trader.common.timer cimport ThreadTimer
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_millis
from nautilus_trader.core.datetime cimport nanos_to_secs
from nautilus_trader.core.rust.core cimport unix_timestamp
from nautilus_trader.core.rust.core cimport unix_timestamp_ms
from nautilus_trader.core.rust.core cimport unix_timestamp_ns
from nautilus_trader.core.uuid cimport UUID4


cdef class Clock:
    """
    The abstract base class for all clocks.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        self._timers = {}    # type: dict[str, Timer]
        self._handlers = {}  # type: dict[str, Callable[[TimeEvent], None]]
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
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef uint64_t timestamp_ms(self) except *:
        """
        Return the current UNIX time in milliseconds (ms).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef uint64_t timestamp_ns(self) except *:
        """
        Return the current UNIX time in nanoseconds (ns).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef datetime utc_now(self):
        """
        Return the current time (UTC).

        Returns
        -------
        datetime
            The current tz-aware UTC time of the clock.

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

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
        Timer or ``None``
            The timer with the given name (if found).

        Raises
        ------
        ValueError
            If `name` is not a valid string.

        """
        Condition.valid_string(name, "name")

        return self._timers.get(name)

    cpdef void register_default_handler(self, handler: Callable[[TimeEvent], None]) except *:
        """
        Register the given handler as the clocks default handler.

        handler : Callable[[TimeEvent], None]
            The handler to register.

        Raises
        ------
        TypeError
            If `handler` is not of type `Callable`.

        """
        Condition.callable(handler, "handler")

        self._default_handler = handler
        self.is_default_handler_registered = True

    cpdef void set_time_alert(
        self,
        str name,
        datetime alert_time,
        callback: Callable[[TimeEvent], None]=None,
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
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.

        Raises
        ------
        ValueError
            If `name` is not unique for this clock.
        ValueError
            If `alert_time` is not >= the clocks current time.
        TypeError
            If `handler` is not of type `Callable` or ``None``.
        ValueError
            If `handler` is ``None`` and no default handler is registered.

        """
        Condition.not_none(name, "name")
        Condition.not_none(alert_time, "alert_time")
        if callback is None:
            callback = self._default_handler
        Condition.not_in(name, self._timers, "name", "_timers")
        Condition.not_in(name, self._handlers, "name", "_handlers")
        Condition.true(alert_time >= self.utc_now(), "alert_time was < self.utc_now()")
        Condition.callable(callback, "callback")

        cdef uint64_t alert_time_ns = int(pd.Timestamp(alert_time).to_datetime64())
        cdef uint64_t now_ns = self.timestamp_ns()

        cdef Timer timer = self._create_timer(
            name=name,
            callback=callback,
            interval_ns=alert_time_ns - now_ns,
            start_time_ns=now_ns,
            stop_time_ns=alert_time_ns,
        )
        self._add_timer(timer, callback)

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Callable[[TimeEvent], None]=None,
    ) except *:
        """
        Set a time alert for the given time.

        When the time is reached the handler will be passed the `TimeEvent`
        containing the timers unique name. If no callback is passed then the
        default handler (if registered) will receive the `TimeEvent`.

        Parameters
        ----------
        name : str
            The name for the alert (must be unique for this clock).
        alert_time_ns : uint64_t
            The UNIX time (nanoseconds) for the alert.
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.

        Raises
        ------
        ValueError
            If `name` is not unique for this clock.
        ValueError
            If `alert_time` is not >= the clocks current time.
        TypeError
            If `callback` is not of type `Callable` or ``None``.
        ValueError
            If `callback` is ``None`` and no default handler is registered.

        """
        Condition.not_none(name, "name")
        if callback is None:
            callback = self._default_handler

        cdef uint64_t now_ns = self.timestamp_ns()

        cdef Timer timer = self._create_timer(
            name=name,
            callback=callback,
            interval_ns=alert_time_ns - now_ns,
            start_time_ns=now_ns,
            stop_time_ns=alert_time_ns,
        )
        self._add_timer(timer, callback)

    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time=None,
        datetime stop_time=None,
        callback: Callable[[TimeEvent], None]=None,
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
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.

        Raises
        ------
        ValueError
            If `name` is not unique for this clock.
        ValueError
            If `interval` is not positive (> 0).
        ValueError
            If `stop_time` is not ``None`` and `stop_time` < time now.
        ValueError
            If `stop_time` is not ``None`` and `start_time` + `interval` > `stop_time`.
        TypeError
            If `handler` is not of type `Callable` or ``None``.
        ValueError
            If `handler` is ``None`` and no default handler is registered.

        """
        cdef datetime now = self.utc_now()  # Call here for greater accuracy

        Condition.valid_string(name, "name")
        Condition.not_none(interval, "interval")
        if callback is None:
            callback = self._default_handler
        Condition.not_in(name, self._timers, "name", "_timers")
        Condition.not_in(name, self._handlers, "name", "_handlers")
        Condition.true(interval.total_seconds() > 0, f"interval was {interval.total_seconds()}")
        Condition.callable(callback, "callback")

        if start_time is None:
            start_time = now
        if stop_time is not None:
            Condition.true(stop_time > now, "stop_time was < now")
            Condition.true(start_time + interval <= stop_time, "start_time + interval was > stop_time")

        cdef uint64_t interval_ns = int(pd.Timedelta(interval).to_timedelta64())
        cdef uint64_t start_time_ns = int(pd.Timestamp(start_time).to_datetime64())
        cdef uint64_t stop_time_ns = int(pd.Timestamp(stop_time).to_datetime64()) if stop_time else 0

        cdef Timer timer = self._create_timer(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )
        self._add_timer(timer, callback)

    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Callable[[TimeEvent], None]=None,
    ) except *:
        """
        Set a timer to run.

        The timer will run from the start time until the stop time.
        When the intervals are reached the handlers will be passed the
        `TimeEvent` containing the timers unique name. If no handler is passed
        then the default handler (if registered) will receive the `TimeEvent`.

        Parameters
        ----------
        name : str
            The name for the timer (must be unique for this clock).
        interval_ns : uint64_t
            The time interval (nanoseconds) for the timer.
        start_time_ns : uint64_t
            The start UNIX time (nanoseconds) for the timer.
        stop_time_ns : uint64_t
            The stop UNIX time (nanoseconds) for the timer.
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.

        Raises
        ------
        ValueError
            If `name` is not unique for this clock.
        ValueError
            If `interval` is not positive (> 0).
        ValueError
            If `stop_time` is not ``None`` and `stop_time` < time now.
        ValueError
            If `stop_time` is not ``None`` and `start_time` + interval > `stop_time`.
        TypeError
            If `callback` is not of type `Callable` or ``None``.
        ValueError
            If `callback` is ``None`` and no default handler is registered.

        """
        Condition.valid_string(name, "name")
        if callback is None:
            callback = self._default_handler
        Condition.positive(interval_ns, f"interval_ns")
        Condition.callable(callback, "callback")
        Condition.true(start_time_ns + interval_ns <= stop_time_ns, "start_time_ns + interval_ns was > stop_time_ns")

        cdef Timer timer = self._create_timer(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )
        self._add_timer(timer, callback)

    cdef Timer _create_timer(
        self,
        str name,
        callback: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cdef void _add_timer(self, Timer timer, handler: Callable[[TimeEvent], None]) except *:
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
            timers = list(self._timers.values())
            self._stack = np.ascontiguousarray(timers, dtype=Timer)
        else:
            self._stack = None

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

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef void _update_timing(self) except *:
        if self.timer_count == 0:
            self.next_event_time_ns = 0
            return
        elif self.timer_count == 1:
            self.next_event_time_ns = self._stack[0].next_time_ns
            return

        cdef uint64_t next_time_ns = self._stack[0].next_time_ns
        cdef uint64_t observed_ns
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

    def __init__(self):
        super().__init__()

        self._time_ns = 0
        self.is_test_clock = True

    cpdef datetime utc_now(self):
        """
        Return the current time (UTC).

        Returns
        -------
        pd.Timestamp
            The current tz-aware UTC time of the clock.

        """
        return pd.Timestamp(self._time_ns, tz="UTC")

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

    cpdef uint64_t timestamp_ms(self) except *:
        """
        Return the current UNIX time in milliseconds (ms).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        return nanos_to_millis(self._time_ns)

    cpdef uint64_t timestamp_ns(self) except *:
        """
        Return the current UNIX time in nanoseconds (ns).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        return self._time_ns

    cpdef void set_time(self, uint64_t to_time_ns) except *:
        """
        Set the clocks datetime to the given time (UTC).

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX time (nanoseconds) to set.

        """
        self._time_ns = to_time_ns

    cpdef list advance_time(self, uint64_t to_time_ns):
        """
        Advance the clocks time to the given `datetime`.

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX time (nanoseconds) advance the clock to.

        Returns
        -------
        list[TimeEvent]
            Sorted chronologically.

        Raises
        ------
        ValueError
            If `to_time` is < the clocks current time.

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
        callback: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
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
    Provides a monotonic clock for live trading. All times are timezone aware UTC.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the clocks timers.
    """

    def __init__(self, loop=None):
        super().__init__()

        self._loop = loop

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

    cpdef uint64_t timestamp_ms(self) except *:
        """
        Return the current UNIX time in milliseconds (ms).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        return unix_timestamp_ms()

    cpdef uint64_t timestamp_ns(self) except *:
        """
        Return the current UNIX time in nanoseconds (ns) from the system clock.

        Returns
        -------
        uint64_t

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
        pd.Timestamp
            The current tz-aware UTC time of the clock.

        """
        return pd.Timestamp.utcnow()

    cdef Timer _create_timer(
        self,
        str name,
        callback: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
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
        cdef TimeEvent event = timer.pop_event(
            event_id=UUID4(),
            ts_init=self.timestamp_ns(),
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
