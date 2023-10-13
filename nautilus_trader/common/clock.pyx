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
import pytz

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from cpython.datetime cimport tzinfo
from cpython.object cimport PyCallable_Check
from cpython.object cimport PyObject
from libc.stdint cimport uint64_t
from libc.stdio cimport printf

from nautilus_trader.common.timer cimport LoopTimer
from nautilus_trader.common.timer cimport ThreadTimer
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport maybe_dt_to_unix_nanos
from nautilus_trader.core.rust.common cimport TimeEventHandler_t
from nautilus_trader.core.rust.common cimport live_clock_drop
from nautilus_trader.core.rust.common cimport live_clock_new
from nautilus_trader.core.rust.common cimport live_clock_timestamp
from nautilus_trader.core.rust.common cimport live_clock_timestamp_ms
from nautilus_trader.core.rust.common cimport live_clock_timestamp_ns
from nautilus_trader.core.rust.common cimport live_clock_timestamp_us
from nautilus_trader.core.rust.common cimport test_clock_advance_time
from nautilus_trader.core.rust.common cimport test_clock_cancel_timer
from nautilus_trader.core.rust.common cimport test_clock_cancel_timers
from nautilus_trader.core.rust.common cimport test_clock_drop
from nautilus_trader.core.rust.common cimport test_clock_new
from nautilus_trader.core.rust.common cimport test_clock_next_time_ns
from nautilus_trader.core.rust.common cimport test_clock_register_default_handler
from nautilus_trader.core.rust.common cimport test_clock_set_time
from nautilus_trader.core.rust.common cimport test_clock_set_time_alert_ns
from nautilus_trader.core.rust.common cimport test_clock_set_timer_ns
from nautilus_trader.core.rust.common cimport test_clock_timer_count
from nautilus_trader.core.rust.common cimport test_clock_timer_names
from nautilus_trader.core.rust.common cimport test_clock_timestamp
from nautilus_trader.core.rust.common cimport test_clock_timestamp_ms
from nautilus_trader.core.rust.common cimport test_clock_timestamp_ns
from nautilus_trader.core.rust.common cimport test_clock_timestamp_us
from nautilus_trader.core.rust.common cimport vec_time_event_handlers_drop
from nautilus_trader.core.rust.core cimport CVec
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

    cpdef double timestamp(self):
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

    cpdef uint64_t timestamp_ms(self):
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

    cpdef uint64_t timestamp_ns(self):
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
        return pd.Timestamp(self.timestamp_ns(), tz=pytz.utc)

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

    cpdef void register_default_handler(self, handler: Callable[[TimeEvent], None]):
        """
        Register the given handler as the clocks default handler.

        handler : Callable[[TimeEvent], None]
            The handler to register.

        Raises
        ------
        TypeError
            If `handler` is not of type `Callable`.

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef uint64_t next_time_ns(self, str name):
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
    ):
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
            If `name` is not a valid string.
        KeyError
            If `name` is not unique for this clock.
        TypeError
            If `handler` is not of type `Callable` or ``None``.
        ValueError
            If `handler` is ``None`` and no default handler is registered.

        Warnings
        --------
        If `alert_time` is in the past or at current time, then an immediate
        time event will be generated (rather than being invalid and failing a condition check).

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
    ):
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
            If `name` is not a valid string.
        ValueError
            If `name` is not unique for this clock.
        TypeError
            If `callback` is not of type `Callable` or ``None``.
        ValueError
            If `callback` is ``None`` and no default handler is registered.

        Warnings
        --------
        If `alert_time_ns` is in the past or at current time, then an immediate
        time event will be generated (rather than being invalid and failing a condition check).

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time = None,
        datetime stop_time = None,
        callback: Optional[Callable[[TimeEvent], None]] = None,
    ):
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
            If `name` is not a valid string.
        KeyError
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
    ):
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
            If `name` is not a valid string.
        KeyError
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

    cpdef void cancel_timer(self, str name):
        """
        Cancel the timer corresponding to the given label.

        Parameters
        ----------
        name : str
            The name for the timer to cancel.

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` is not an active timer name for this clock.

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void cancel_timers(self):
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
        self._mem = test_clock_new()

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            test_clock_drop(self._mem)

    @property
    def timer_names(self) -> list[str]:
        return sorted(<list>test_clock_timer_names(&self._mem))

    @property
    def timer_count(self) -> int:
        return test_clock_timer_count(&self._mem)

    cpdef double timestamp(self):
        return test_clock_timestamp(&self._mem)

    cpdef uint64_t timestamp_ms(self):
        return test_clock_timestamp_ms(&self._mem)

    cpdef uint64_t timestamp_ns(self):
        return test_clock_timestamp_ns(&self._mem)

    cpdef void register_default_handler(self, callback: Callable[[TimeEvent], None]):
        Condition.callable(callback, "callback")

        test_clock_register_default_handler(&self._mem, <PyObject *>callback)

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Optional[Callable[[TimeEvent], None]] = None,
    ):
        Condition.valid_string(name, "name")
        Condition.not_in(name, self.timer_names, "name", "self.timer_names")

        test_clock_set_time_alert_ns(
            &self._mem,
            pystr_to_cstr(name),
            alert_time_ns,
            <PyObject *>callback,
        )

    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Optional[Callable[[TimeEvent], None]] = None,
    ):
        Condition.valid_string(name, "name")
        Condition.not_in(name, self.timer_names, "name", "self.timer_names")
        Condition.positive_int(interval_ns, "interval_ns")

        cdef uint64_t ts_now = self.timestamp_ns()

        if start_time_ns == 0:
            start_time_ns = ts_now
        if stop_time_ns:
            Condition.true(stop_time_ns > ts_now, "`stop_time_ns` was < `ts_now`")
            Condition.true(start_time_ns + interval_ns <= stop_time_ns, "`start_time_ns` + `interval_ns` was > `stop_time_ns`")

        test_clock_set_timer_ns(
            &self._mem,
            pystr_to_cstr(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            <PyObject *>callback,
        )

    cpdef uint64_t next_time_ns(self, str name):
        Condition.valid_string(name, "name")
        return test_clock_next_time_ns(&self._mem, pystr_to_cstr(name))

    cpdef void cancel_timer(self, str name):
        Condition.valid_string(name, "name")
        Condition.is_in(name, self.timer_names, "name", "self.timer_names")

        test_clock_cancel_timer(&self._mem, pystr_to_cstr(name))

    cpdef void cancel_timers(self):
        test_clock_cancel_timers(&self._mem)

    cpdef void set_time(self, uint64_t to_time_ns):
        """
        Set the clocks datetime to the given time (UTC).

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX time (nanoseconds) to set.

        """
        test_clock_set_time(&self._mem, to_time_ns)

    cdef CVec advance_time_c(self, uint64_t to_time_ns, bint set_time=True):
        Condition.true(to_time_ns >= test_clock_timestamp_ns(&self._mem), "to_time_ns was < time_ns (not monotonic)")

        return <CVec>test_clock_advance_time(&self._mem, to_time_ns, set_time)

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
        cdef CVec raw_handler_vec = self.advance_time_c(to_time_ns, set_time)
        cdef TimeEventHandler_t* raw_handlers = <TimeEventHandler_t*>raw_handler_vec.ptr
        cdef list event_handlers = []

        cdef:
            uint64_t i
            object callback
            TimeEvent event
            TimeEventHandler_t raw_handler
            TimeEventHandler event_handler
        for i in range(raw_handler_vec.len):
            raw_handler = <TimeEventHandler_t>raw_handlers[i]
            event = TimeEvent.from_mem_c(raw_handler.event)

            # Cast raw `PyObject *` to a `PyObject`
            callback = <object>raw_handler.callback_ptr

            event_handler = TimeEventHandler(event, callback)
            event_handlers.append(event_handler)

        vec_time_event_handlers_drop(raw_handler_vec)

        return event_handlers


cdef class LiveClock(Clock):
    """
    Provides a monotonic clock for live trading.

    All times are tz-aware UTC.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the clocks timers.
    """

    def __init__(self, loop: Optional[asyncio.AbstractEventLoop] = None):
        self._mem = live_clock_new()
        self._default_handler = None
        self._handlers: dict[str, Callable[[TimeEvent], None]] = {}

        self._loop = loop
        self._timers: dict[str, LiveTimer] = {}
        self._stack = np.ascontiguousarray([], dtype=LiveTimer)

        self._timer_count = 0
        self._next_event_time_ns = 0

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            live_clock_drop(self._mem)

    @property
    def timer_names(self) -> list[str]:
        return list(self._timers.keys())

    @property
    def timer_count(self) -> int:
        return self._timer_count

    cpdef double timestamp(self):
        return live_clock_timestamp(&self._mem)

    cpdef uint64_t timestamp_ms(self):
        return live_clock_timestamp_ms(&self._mem)

    cpdef uint64_t timestamp_ns(self):
        return live_clock_timestamp_ns(&self._mem)

    cpdef void register_default_handler(self, callback: Callable[[TimeEvent], None]):
        Condition.callable(callback, "callback")

        self._default_handler = callback

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Optional[Callable[[TimeEvent], None]] = None,
    ):
        Condition.valid_string(name, "name")
        Condition.not_in(name, self.timer_names, "name", "self.timer_names")
        if callback is None:
            callback = self._default_handler

        cdef uint64_t ts_now = self.timestamp_ns()

        cdef LiveTimer timer = self._create_timer(
            name=name,
            callback=callback,
            interval_ns=alert_time_ns - ts_now,
            start_time_ns=ts_now,
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
    ):
        Condition.not_in(name, self.timer_names, "name", "self.timer_names")

        cdef uint64_t ts_now = self.timestamp_ns()  # Call here for greater accuracy

        Condition.valid_string(name, "name")
        if callback is None:
            callback = self._default_handler

        Condition.not_in(name, self._timers, "name", "_timers")
        Condition.not_in(name, self._handlers, "name", "_handlers")
        Condition.true(interval_ns > 0, f"interval was {interval_ns}")
        Condition.callable(callback, "callback")

        if start_time_ns == 0:
            start_time_ns = ts_now
        if stop_time_ns:
            Condition.true(stop_time_ns > ts_now, "stop_time was < ts_now")
            Condition.true(start_time_ns + interval_ns <= stop_time_ns, "start_time + interval was > stop_time")

        cdef LiveTimer timer = self._create_timer(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )
        self._add_timer(timer, callback)

    cdef void _add_timer(self, LiveTimer timer, handler: Callable[[TimeEvent], None]):
        self._timers[timer.name] = timer
        self._handlers[timer.name] = handler
        self._update_stack()
        self._update_timing()

    cdef void _remove_timer(self, LiveTimer timer):
        self._timers.pop(timer.name, None)
        self._handlers.pop(timer.name, None)
        self._update_stack()
        self._update_timing()

    cdef void _update_stack(self):
        self._timer_count = len(self._timers)

        if self._timer_count > 0:
            # The call to `np.ascontiguousarray` here looks inefficient, its
            # only called when a timer is added or removed. This then allows the
            # construction of an efficient Timer[:] memoryview.
            timers = list(self._timers.values())
            self._stack = np.ascontiguousarray(timers, dtype=LiveTimer)
        else:
            self._stack = None

    cpdef uint64_t next_time_ns(self, str name):
        return self._timers[name].next_time_ns

    cpdef void cancel_timer(self, str name):
        Condition.valid_string(name, "name")
        Condition.is_in(name, self.timer_names, "name", "self.timer_names")

        cdef LiveTimer timer = self._timers.pop(name, None)
        if not timer:
            # No timer with given name
            return

        timer.cancel()
        self._handlers.pop(name, None)
        self._remove_timer(timer)

    cpdef void cancel_timers(self):
        cdef str name
        for name in self.timer_names:
            # Using a list of timer names from the property and passing this
            # to cancel_timer() handles the clean removal of both the handler
            # and timer.
            self.cancel_timer(name)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef void _update_timing(self):
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
                ts_now=self.timestamp_ns(),  # Timestamp here for accuracy
                start_time_ns=start_time_ns,
                stop_time_ns=stop_time_ns,
            )
        else:
            return ThreadTimer(
                name=name,
                callback=self._raise_time_event,
                interval_ns=interval_ns,
                ts_now=self.timestamp_ns(),  # Timestamp here for accuracy
                start_time_ns=start_time_ns,
                stop_time_ns=stop_time_ns,
            )

    cpdef void _raise_time_event(self, LiveTimer timer):
        cdef uint64_t now = self.timestamp_ns()
        cdef TimeEvent event = timer.pop_event(
            event_id=UUID4(),
            ts_init=now,
        )

        if now < timer.next_time_ns:
            timer.iterate_next_time(timer.next_time_ns)
        else:
            timer.iterate_next_time(now)

        self._handle_time_event(event)

        if timer.is_expired:
            self._remove_timer(timer)
        else:  # Continue timing
            timer.repeat(ts_now=self.timestamp_ns())
            self._update_timing()

    cdef void _handle_time_event(self, TimeEvent event):
        handler = self._handlers.get(event.name)
        if handler is not None:
            handler(event)
