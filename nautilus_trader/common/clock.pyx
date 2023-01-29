# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
from typing import Callable, Optional

import cython
import numpy as np
import pandas as pd

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from cpython.datetime cimport tzinfo
from libc.stdint cimport uint64_t

from nautilus_trader.common.timer cimport LoopTimer
from nautilus_trader.common.timer cimport ThreadTimer
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport maybe_dt_to_unix_nanos
from nautilus_trader.core.rust.common cimport Vec_TimeEvent
from nautilus_trader.core.rust.common cimport test_clock_advance_time
from nautilus_trader.core.rust.common cimport test_clock_cancel_timer
from nautilus_trader.core.rust.common cimport test_clock_cancel_timers
from nautilus_trader.core.rust.common cimport test_clock_free
from nautilus_trader.core.rust.common cimport test_clock_new
from nautilus_trader.core.rust.common cimport test_clock_next_time_ns
from nautilus_trader.core.rust.common cimport test_clock_set_time
from nautilus_trader.core.rust.common cimport test_clock_set_time_alert_ns
from nautilus_trader.core.rust.common cimport test_clock_set_timer_ns
from nautilus_trader.core.rust.common cimport test_clock_time_ns
from nautilus_trader.core.rust.common cimport test_clock_timer_count
from nautilus_trader.core.rust.common cimport test_clock_timer_names
from nautilus_trader.core.rust.common cimport vec_time_events_drop
from nautilus_trader.core.rust.core cimport nanos_to_millis
from nautilus_trader.core.rust.core cimport nanos_to_secs
from nautilus_trader.core.rust.core cimport unix_timestamp
from nautilus_trader.core.rust.core cimport unix_timestamp_ms
from nautilus_trader.core.rust.core cimport unix_timestamp_ns
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.uuid cimport UUID4


cdef class Clock:
    """
    The base class for all clocks.

    Notes
    -----
    An *active* timer is one which has not expired.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        self._handlers: dict[str, Callable[[TimeEvent], None]] = {}
        self._default_handler = None

    @property
    def timer_names(self) -> list[str]:
        """
        Return the names of *active* timers running in the clock.

        Returns
        -------
        list[str]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    @property
    def timer_count(self) -> int:
        """
        Return the count of *active* timers running in the clock.

        Returns
        -------
        int

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

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
        return pd.Timestamp(self.timestamp_ns(), tz="UTC")

    cpdef datetime local_now(self, tzinfo tz = None):
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

    cpdef uint64_t next_time_ns(self, str name) except *:
        """
        Find a particular timer.

        Parameters
        ----------
        name : str
            The name of the timer.

        Returns
        -------
        uint64_t

        Raises
        ------
        ValueError
            If `name` is not a valid string.

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void set_time_alert(
        self,
        str name,
        datetime alert_time,
        callback: Callable[[TimeEvent], None] = None,
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
        self.set_time_alert_ns(
            name=name,
            alert_time_ns=dt_to_unix_nanos(alert_time),
            callback=callback,
        )

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Callable[[TimeEvent], None] = None,
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
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time = None,
        datetime stop_time = None,
        callback: Optional[Callable[[TimeEvent], None]] = None,
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
        self.set_timer_ns(
            name=name,
            interval_ns=pd.Timedelta(interval).value,
            start_time_ns=maybe_dt_to_unix_nanos(start_time) or 0,
            stop_time_ns=maybe_dt_to_unix_nanos(stop_time) or 0,
            callback=callback,
        )

    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Optional[Callable[[TimeEvent], None]] = None,
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
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

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
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void cancel_timers(self) except *:
        """
        Cancel all timers.
        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover


cdef class TestClock(Clock):
    """
    Provides a monotonic clock for backtesting and unit testing.

    """

    __test__ = False  # Required so pytest does not consider this a test class

    def __init__(self):
        super().__init__()

        self._mem = test_clock_new()

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            test_clock_free(self._mem)

    @property
    def timer_names(self) -> list[str]:
        return sorted(<list>test_clock_timer_names(&self._mem))

    @property
    def timer_count(self) -> int:
        return test_clock_timer_count(&self._mem)

    cpdef double timestamp(self) except *:
        return nanos_to_secs(test_clock_time_ns(&self._mem))

    cpdef uint64_t timestamp_ms(self) except *:
        return nanos_to_millis(test_clock_time_ns(&self._mem))

    cpdef uint64_t timestamp_ns(self) except *:
        return test_clock_time_ns(&self._mem)

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Optional[Callable[[TimeEvent], None]] = None,
    ) except *:
        Condition.not_none(name, "name")
        if callback is None:
            callback = self._default_handler

        self._handlers[name] = callback

        test_clock_set_time_alert_ns(&self._mem, pystr_to_cstr(name), alert_time_ns)

    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Optional[Callable[[TimeEvent], None]] = None,
    ) except *:
        if callback is None:
            callback = self._default_handler

        self._handlers[name] = callback

        cdef uint64_t now_ns = self.timestamp_ns()

        if start_time_ns == 0:
            start_time_ns = now_ns
        if stop_time_ns:
            Condition.true(stop_time_ns > now_ns, "stop_time was < now")
            Condition.true(start_time_ns + interval_ns <= stop_time_ns, "start_time + interval was > stop_time")

        test_clock_set_timer_ns(
            &self._mem,
            pystr_to_cstr(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
        )

    cpdef uint64_t next_time_ns(self, str name) except *:
        return test_clock_next_time_ns(&self._mem, pystr_to_cstr(name))

    cpdef void cancel_timer(self, str name) except *:
        test_clock_cancel_timer(&self._mem, pystr_to_cstr(name))

    cpdef void cancel_timers(self) except *:
        test_clock_cancel_timers(&self._mem)

    cpdef void set_time(self, uint64_t to_time_ns) except *:
        """
        Set the clocks datetime to the given time (UTC).

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX time (nanoseconds) to set.

        """
        test_clock_set_time(&self._mem, to_time_ns)

    cpdef list advance_time(self, uint64_t to_time_ns, bint set_time=True):
        """
        Advance the clocks time to the given `to_time_ns`.

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX time (nanoseconds) to advance the clock to.
        set_time : bool
            If the clock should also be set to the given `to_time_ns`.

        Returns
        -------
        list[TimeEventHandler]
            Sorted chronologically.

        Raises
        ------
        ValueError
            If `to_time_ns` is < the clocks current time.

        """
        # Ensure monotonic
        Condition.true(to_time_ns >= test_clock_time_ns(&self._mem), "to_time_ns was < time_ns")

        cdef Vec_TimeEvent raw_events = test_clock_advance_time(&self._mem, to_time_ns, set_time)
        cdef list event_handlers = []

        cdef:
            cdef uint64_t i
            TimeEvent event
            TimeEventHandler event_handler
        for i in range(raw_events.len):
            # For now, we hold the Python callables on the Python side and match
            # by timer name. In another iteration this will all be moved to Rust
            # along with the live timer impls.
            event = TimeEvent.from_mem_c(raw_events.ptr[i])
            event_handler = TimeEventHandler(event, self._handlers[event.name])
            event_handlers.append(event_handler)

        vec_time_events_drop(raw_events)

        return sorted(event_handlers)


cdef class LiveClock(Clock):
    """
    Provides a monotonic clock for live trading. All times are timezone aware UTC.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the clocks timers.
    """

    def __init__(self, loop: Optional[asyncio.AbstractEventLoop] = None):
        super().__init__()

        self._loop = loop
        self._timers: dict[str, LiveTimer] = {}
        self._stack = np.ascontiguousarray([], dtype=LiveTimer)

        self._offset_secs = 0.0
        self._offset_ms = 0
        self._offset_ns = 0
        self._timer_count = 0
        self._next_event_time_ns = 0

    @property
    def timer_names(self) -> list[str]:
        return list(self._timers.keys())

    @property
    def timer_count(self) -> int:
        return self._timer_count

    cpdef void set_offset(self, int64_t offset_ns) except *:
        """
        Set the offset (nanoseconds) for the clock.

        The `offset` will then be *added* to all subsequent timestamps.

        Warnings
        --------
        It shouldn't be necessary for a user to call this method.

        """
        self._offset_ns = offset_ns
        self._offset_ms = nanos_to_millis(offset_ns)
        self._offset_secs = nanos_to_secs(offset_ns)

    cpdef double timestamp(self) except *:
        return unix_timestamp() + self._offset_secs

    cpdef uint64_t timestamp_ms(self) except *:
        return unix_timestamp_ms() + self._offset_ms

    cpdef uint64_t timestamp_ns(self) except *:
        return unix_timestamp_ns() + self._offset_ns

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Optional[Callable[[TimeEvent], None]] = None,
    ) except *:
        Condition.not_none(name, "name")
        if callback is None:
            callback = self._default_handler

        cdef uint64_t now_ns = self.timestamp_ns()

        cdef LiveTimer timer = self._create_timer(
            name=name,
            callback=callback,
            interval_ns=alert_time_ns - now_ns,
            start_time_ns=now_ns,
            stop_time_ns=alert_time_ns,
        )
        self._add_timer(timer, callback)

    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Optional[Callable[[TimeEvent], None]] = None,
    ) except *:
        cdef uint64_t now_ns = self.timestamp_ns()  # Call here for greater accuracy

        Condition.valid_string(name, "name")
        if callback is None:
            callback = self._default_handler

        Condition.not_in(name, self._timers, "name", "_timers")
        Condition.not_in(name, self._handlers, "name", "_handlers")
        Condition.true(interval_ns > 0, f"interval was {interval_ns}")
        Condition.callable(callback, "callback")

        if start_time_ns == 0:
            start_time_ns = now_ns
        if stop_time_ns:
            Condition.true(stop_time_ns > now_ns, "stop_time was < now")
            Condition.true(start_time_ns + interval_ns <= stop_time_ns, "start_time + interval was > stop_time")

        cdef LiveTimer timer = self._create_timer(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )
        self._add_timer(timer, callback)

    cdef void _add_timer(self, LiveTimer timer, handler: Callable[[TimeEvent], None]) except *:
        self._timers[timer.name] = timer
        self._handlers[timer.name] = handler
        self._update_stack()
        self._update_timing()

    cdef void _remove_timer(self, LiveTimer timer) except *:
        self._timers.pop(timer.name, None)
        self._handlers.pop(timer.name, None)
        self._update_stack()
        self._update_timing()

    cdef void _update_stack(self) except *:
        self._timer_count = len(self._timers)

        if self._timer_count > 0:
            # The call to `np.ascontiguousarray` here looks inefficient, its
            # only called when a timer is added or removed. This then allows the
            # construction of an efficient Timer[:] memoryview.
            timers = list(self._timers.values())
            self._stack = np.ascontiguousarray(timers, dtype=LiveTimer)
        else:
            self._stack = None

    cpdef uint64_t next_time_ns(self, str name) except *:
        return self._timers[name].next_time_ns

    cpdef void cancel_timer(self, str name) except *:
        Condition.valid_string(name, "name")

        cdef LiveTimer timer = self._timers.pop(name, None)
        if not timer:
            # No timer with given name
            return

        timer.cancel()
        self._handlers.pop(name, None)
        self._remove_timer(timer)

    cpdef void cancel_timers(self) except *:
        cdef str name
        for name in self.timer_names:
            # Using a list of timer names from the property and passing this
            # to cancel_timer() handles the clean removal of both the handler
            # and timer.
            self.cancel_timer(name)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef void _update_timing(self) except *:
        if self._timer_count == 0:
            self._next_event_time_ns = 0
            return

        cdef LiveTimer first_timer = self._stack[0]
        if self._timer_count == 1:
            self._next_event_time_ns = first_timer.next_time_ns
            return

        cdef uint64_t next_time_ns = first_timer.next_time_ns
        cdef:
            int i
            LiveTimer timer
            uint64_t observed_ns
        for i in range(self._timer_count - 1):
            timer = self._stack[i + 1]
            observed_ns = timer.next_time_ns
            if observed_ns < next_time_ns:
                next_time_ns = observed_ns

        self._next_event_time_ns = next_time_ns

    cdef LiveTimer _create_timer(
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
