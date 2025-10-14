# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
import copy
import socket
import sys
import traceback
from collections import deque
from typing import Any
from typing import Callable

import cython
import msgspec
import numpy as np
import pandas as pd
import pyarrow
import pytz

from nautilus_trader.common.config import InvalidConfiguration
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.rust.common import ComponentState as PyComponentState

cimport numpy as np
from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from cpython.datetime cimport tzinfo
from cpython.object cimport PyCallable_Check
from cpython.object cimport PyObject
from cpython.pycapsule cimport PyCapsule_GetPointer
from libc.stdint cimport int64_t
from libc.stdint cimport uint32_t
from libc.stdint cimport uint64_t
from libc.stdio cimport printf

from nautilus_trader.common.messages cimport ComponentStateChanged
from nautilus_trader.common.messages cimport ShutdownSystem
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport maybe_dt_to_unix_nanos
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.common cimport ComponentTrigger
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport LogGuard_API
from nautilus_trader.core.rust.common cimport LogLevel
from nautilus_trader.core.rust.common cimport TimeEventHandler_t
from nautilus_trader.core.rust.common cimport component_state_from_cstr
from nautilus_trader.core.rust.common cimport component_state_to_cstr
from nautilus_trader.core.rust.common cimport component_trigger_from_cstr
from nautilus_trader.core.rust.common cimport component_trigger_to_cstr
from nautilus_trader.core.rust.common cimport is_matching_ffi
from nautilus_trader.core.rust.common cimport live_clock_cancel_timer
from nautilus_trader.core.rust.common cimport live_clock_drop
from nautilus_trader.core.rust.common cimport live_clock_new
from nautilus_trader.core.rust.common cimport live_clock_next_time
from nautilus_trader.core.rust.common cimport live_clock_register_default_handler
from nautilus_trader.core.rust.common cimport live_clock_set_time_alert
from nautilus_trader.core.rust.common cimport live_clock_set_timer
from nautilus_trader.core.rust.common cimport live_clock_timer_count
from nautilus_trader.core.rust.common cimport live_clock_timer_names
from nautilus_trader.core.rust.common cimport live_clock_timestamp
from nautilus_trader.core.rust.common cimport live_clock_timestamp_ms
from nautilus_trader.core.rust.common cimport live_clock_timestamp_ns
from nautilus_trader.core.rust.common cimport live_clock_timestamp_us
from nautilus_trader.core.rust.common cimport log_color_from_cstr
from nautilus_trader.core.rust.common cimport log_color_to_cstr
from nautilus_trader.core.rust.common cimport log_level_from_cstr
from nautilus_trader.core.rust.common cimport log_level_to_cstr
from nautilus_trader.core.rust.common cimport logger_drop
from nautilus_trader.core.rust.common cimport logger_flush
from nautilus_trader.core.rust.common cimport logger_log
from nautilus_trader.core.rust.common cimport logging_clock_set_realtime_mode
from nautilus_trader.core.rust.common cimport logging_clock_set_static_mode
from nautilus_trader.core.rust.common cimport logging_clock_set_static_time
from nautilus_trader.core.rust.common cimport logging_init
from nautilus_trader.core.rust.common cimport logging_is_colored
from nautilus_trader.core.rust.common cimport logging_is_initialized
from nautilus_trader.core.rust.common cimport logging_log_header
from nautilus_trader.core.rust.common cimport logging_log_sysinfo
from nautilus_trader.core.rust.common cimport logging_shutdown
from nautilus_trader.core.rust.common cimport test_clock_advance_time
from nautilus_trader.core.rust.common cimport test_clock_cancel_timer
from nautilus_trader.core.rust.common cimport test_clock_cancel_timers
from nautilus_trader.core.rust.common cimport test_clock_drop
from nautilus_trader.core.rust.common cimport test_clock_new
from nautilus_trader.core.rust.common cimport test_clock_next_time
from nautilus_trader.core.rust.common cimport test_clock_register_default_handler
from nautilus_trader.core.rust.common cimport test_clock_set_time
from nautilus_trader.core.rust.common cimport test_clock_set_time_alert
from nautilus_trader.core.rust.common cimport test_clock_set_timer
from nautilus_trader.core.rust.common cimport test_clock_timer_count
from nautilus_trader.core.rust.common cimport test_clock_timer_names
from nautilus_trader.core.rust.common cimport test_clock_timestamp
from nautilus_trader.core.rust.common cimport test_clock_timestamp_ms
from nautilus_trader.core.rust.common cimport test_clock_timestamp_ns
from nautilus_trader.core.rust.common cimport test_clock_timestamp_us
from nautilus_trader.core.rust.common cimport time_event_new
from nautilus_trader.core.rust.common cimport time_event_to_cstr
from nautilus_trader.core.rust.common cimport vec_time_event_handlers_drop
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.core cimport nanos_to_millis
from nautilus_trader.core.rust.core cimport nanos_to_secs
from nautilus_trader.core.rust.core cimport secs_to_nanos
from nautilus_trader.core.rust.core cimport uuid4_from_cstr
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pybytes_to_cstr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.string cimport ustr_to_pystr
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport Identifier
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.serialization.base cimport _EXTERNAL_PUBLISHABLE_TYPES
from nautilus_trader.serialization.base cimport Serializer


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
        raise NotImplementedError("method `timer_names` must be implemented in the subclass")  # pragma: no cover

    @property
    def timer_count(self) -> int:
        """
        Return the count of *active* timers running in the clock.

        Returns
        -------
        int

        """
        raise NotImplementedError("method `timer_count` must be implemented in the subclass")  # pragma: no cover

    cpdef double timestamp(self):
        """
        Return the current UNIX timestamp in seconds.

        Returns
        -------
        double

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        raise NotImplementedError("method `timestamp` must be implemented in the subclass")  # pragma: no cover

    cpdef uint64_t timestamp_ms(self):
        """
        Return the current UNIX timestamp in milliseconds (ms).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        raise NotImplementedError("method `timestamp_ms` must be implemented in the subclass")  # pragma: no cover

    cpdef uint64_t timestamp_us(self):
        """
        Return the current UNIX timestamp in microseconds (μs).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        raise NotImplementedError("method `timestamp_us` must be implemented in the subclass")  # pragma: no cover

    cpdef uint64_t timestamp_ns(self):
        """
        Return the current UNIX timestamp in nanoseconds (ns).

        Returns
        -------
        uint64_t

        References
        ----------
        https://en.wikipedia.org/wiki/Unix_time

        """
        raise NotImplementedError("method `timestamp_ns` must be implemented in the subclass")  # pragma: no cover

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

        Parameters
        ----------
        handler : Callable[[TimeEvent], None]
            The handler to register.

        Raises
        ------
        TypeError
            If `handler` is not of type `Callable`.

        """
        raise NotImplementedError("method `register_default_handler` must be implemented in the subclass")  # pragma: no cover

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
        raise NotImplementedError("method `next_time_ns` must be implemented in the subclass")  # pragma: no cover

    cpdef void set_time_alert(
        self,
        str name,
        datetime alert_time,
        callback: Callable[[TimeEvent], None] = None,
        bint override = False,
        bint allow_past = True,
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
        override: bool, default False
            If override is set to True an alert with a given name can be overwritten if it exists already.
        allow_past : bool, default True
            If True, allows an `alert_time` in the past and adjusts it to the current time
            for immediate firing. If False, raises an error when the `alert_time` is in the
            past, requiring it to be in the future.

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
        if override and self.next_time_ns(name) > 0:
            self.cancel_timer(name)

        self.set_time_alert_ns(
            name=name,
            alert_time_ns=dt_to_unix_nanos(alert_time),
            callback=callback,
            allow_past=allow_past,
        )

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Callable[[TimeEvent], None] = None,
        bint allow_past = True,
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
            The UNIX timestamp (nanoseconds) for the alert.
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.
        allow_past : bool, default True
            If True, allows an `alert_time_ns` in the past and adjusts it to the current time
            for immediate firing. If False, panics when the `alert_time_ns` is in the
            past, requiring it to be in the future.

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
        raise NotImplementedError("method `set_time_alert_ns` must be implemented in the subclass")  # pragma: no cover

    cpdef void set_timer(
        self,
        str name,
        timedelta interval,
        datetime start_time = None,
        datetime stop_time = None,
        callback: Callable[[TimeEvent], None] | None = None,
        bint allow_past = True,
        bint fire_immediately = False,
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
        allow_past : bool, default True
            If True, allows timers where the next event time may be in the past.
            If False, raises an error when the next event time would be in the past.
        fire_immediately : bool, default False
            If True, the timer will fire immediately at the start time,
            then fire again after each interval. If False, the timer will
            fire after the first interval has elapsed (default behavior).

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
            allow_past=allow_past,
            fire_immediately=fire_immediately,
        )

    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Callable[[TimeEvent], None] | None = None,
        bint allow_past = True,
        bint fire_immediately = False,
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
            The start UNIX timestamp (nanoseconds) for the timer.
        stop_time_ns : uint64_t
            The stop UNIX timestamp (nanoseconds) for the timer.
        callback : Callable[[TimeEvent], None], optional
            The callback to receive time events.
        allow_past : bool, default True
            If True, allows timers where the next event time may be in the past.
            If False, raises an error when the next event time would be in the past.
        fire_immediately : bool, default False
            If True, the timer will fire immediately at the start time,
            then fire again after each interval. If False, the timer will
            fire after the first interval has elapsed (default behavior).

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
        raise NotImplementedError("method `set_timer_ns` must be implemented in the subclass")  # pragma: no cover

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
        raise NotImplementedError("method `cancel_timer` must be implemented in the subclass")  # pragma: no cover

    cpdef void cancel_timers(self):
        """
        Cancel all timers.
        """
        raise NotImplementedError("method `cancel_timers` must be implemented in the subclass")  # pragma: no cover


# Global map of clocks per kernel instance used when running a `BacktestEngine`
_COMPONENT_CLOCKS = {}


cdef list[TestClock] get_component_clocks(UUID4 instance_id):
    # Create a shallow copy of the clocks list, in case a new
    # clock is registered during iteration.
    return _COMPONENT_CLOCKS[instance_id].copy()


cpdef void register_component_clock(UUID4 instance_id, Clock clock):
    Condition.not_none(instance_id, "instance_id")
    Condition.not_none(clock, "clock")

    cdef list[Clock] clocks = _COMPONENT_CLOCKS.get(instance_id)

    if clocks is None:
        clocks = []
        _COMPONENT_CLOCKS[instance_id] = clocks

    if clock not in clocks:
        clocks.append(clock)


cpdef void deregister_component_clock(UUID4 instance_id, Clock clock):
    Condition.not_none(instance_id, "instance_id")
    Condition.not_none(clock, "clock")

    cdef list[Clock] clocks = _COMPONENT_CLOCKS.get(instance_id)

    if clocks is None:
        return

    if clock in clocks:
        clocks.remove(clock)


cpdef void remove_instance_component_clocks(UUID4 instance_id):
    Condition.not_none(instance_id, "instance_id")

    _COMPONENT_CLOCKS.pop(instance_id, None)


# Global backtest force stop flag
_FORCE_STOP = False

cpdef void set_backtest_force_stop(bint value):
    global FORCE_STOP
    FORCE_STOP = value


cpdef bint is_backtest_force_stop():
    return FORCE_STOP


cdef class TestClock(Clock):
    """
    Provides a monotonic clock for backtesting and unit testing.

    """

    __test__ = False  # Prevents pytest from collecting this as a test class

    def __init__(self):
        self._mem = test_clock_new()

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            test_clock_drop(self._mem)

    @property
    def timer_names(self) -> list[str]:
        cdef str timer_names = cstr_to_pystr(test_clock_timer_names(&self._mem))
        if not timer_names:
            return []

        # For simplicity we split a string on a reasonably unique delimiter.
        # This is a temporary solution pending the removal of Cython.
        return sorted(timer_names.split("<,>"))

    @property
    def timer_count(self) -> int:
        return test_clock_timer_count(&self._mem)

    cpdef double timestamp(self):
        return test_clock_timestamp(&self._mem)

    cpdef uint64_t timestamp_ms(self):
        return test_clock_timestamp_ms(&self._mem)

    cpdef uint64_t timestamp_us(self):
        return test_clock_timestamp_us(&self._mem)

    cpdef uint64_t timestamp_ns(self):
        return test_clock_timestamp_ns(&self._mem)

    cpdef void register_default_handler(self, callback: Callable[[TimeEvent], None]):
        Condition.callable(callback, "callback")

        test_clock_register_default_handler(&self._mem, <PyObject *>callback)

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Callable[[TimeEvent], None] | None = None,
        bint allow_past = True,
    ):
        Condition.valid_string(name, "name")
        Condition.not_in(name, self.timer_names, "name", "self.timer_names")

        # Validate allow_past logic to prevent Rust errors
        cdef uint64_t ts_now = self.timestamp_ns()

        if not allow_past:
            if alert_time_ns < ts_now:
                alert_dt = datetime.fromtimestamp(alert_time_ns / 1e9).isoformat()
                current_dt = datetime.fromtimestamp(ts_now / 1e9).isoformat()
                raise ValueError(
                    f"Timer '{name}' alert time {alert_dt} was in the past "
                    f"(current time is {current_dt})"
                )

        test_clock_set_time_alert(
            &self._mem,
            pystr_to_cstr(name),
            alert_time_ns,
            <PyObject *>callback,
            allow_past,
        )

    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Callable[[TimeEvent], None] | None = None,
        bint allow_past = True,
        bint fire_immediately = False,
    ):
        Condition.valid_string(name, "name")
        Condition.not_in(name, self.timer_names, "name", "self.timer_names")
        Condition.positive_int(interval_ns, "interval_ns")

        # Validate callback availability to prevent Rust panics
        # Note: We can't easily check if default handler is registered from Cython,
        # but we can provide a more informative error than a Rust panic
        # The existing tests in the codebase show this validation should be done

        cdef uint64_t ts_now = self.timestamp_ns()

        if start_time_ns == 0:
            start_time_ns = ts_now
        if stop_time_ns:
            Condition.is_true(stop_time_ns > ts_now, "`stop_time_ns` was < `ts_now`")
            Condition.is_true(start_time_ns + interval_ns <= stop_time_ns, "`start_time_ns` + `interval_ns` was > `stop_time_ns`")

        # Validate allow_past logic to prevent Rust errors
        cdef uint64_t next_event_time

        if not allow_past:
            if fire_immediately:
                next_event_time = start_time_ns
            else:
                next_event_time = start_time_ns + interval_ns

            if next_event_time < ts_now:
                next_dt = datetime.fromtimestamp(next_event_time / 1e9).isoformat()
                current_dt = datetime.fromtimestamp(ts_now / 1e9).isoformat()
                raise ValueError(
                    f"Timer '{name}' next event time {next_dt} would be in the past "
                    f"(current time is {current_dt})"
                )

        test_clock_set_timer(
            &self._mem,
            pystr_to_cstr(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            <PyObject *>callback,
            allow_past,
            fire_immediately,
        )

    cpdef uint64_t next_time_ns(self, str name):
        Condition.valid_string(name, "name")
        return test_clock_next_time(&self._mem, pystr_to_cstr(name))

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
            The UNIX timestamp (nanoseconds) to set.

        """
        test_clock_set_time(&self._mem, to_time_ns)

    cdef CVec advance_time_c(self, uint64_t to_time_ns, bint set_time=True):
        Condition.is_true(to_time_ns >= test_clock_timestamp_ns(&self._mem), "to_time_ns was < time_ns (not monotonic)")

        return <CVec>test_clock_advance_time(&self._mem, to_time_ns, set_time)

    cpdef list advance_time(self, uint64_t to_time_ns, bint set_time=True):
        """
        Advance the clocks time to the given `to_time_ns`.

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX timestamp (nanoseconds) to advance the clock to.
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
            PyObject *raw_callback
        for i in range(raw_handler_vec.len):
            raw_handler = <TimeEventHandler_t>raw_handlers[i]
            event = TimeEvent.from_mem_c(raw_handler.event)

            # Cast raw `PyObject *` to a `PyObject`
            raw_callback = <PyObject *>raw_handler.callback_ptr
            callback = <object>raw_callback

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

    def __init__(self):
        self._mem = live_clock_new()

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            live_clock_drop(self._mem)

    @property
    def timer_names(self) -> list[str]:
        cdef str timer_names = cstr_to_pystr(live_clock_timer_names(&self._mem))
        if not timer_names:
            return []

        # For simplicity we split a string on a reasonably unique delimiter.
        # This is a temporary solution pending the removal of Cython.
        return sorted(timer_names.split("<,>"))

    @property
    def timer_count(self) -> int:
        return live_clock_timer_count(&self._mem)

    cpdef double timestamp(self):
        return live_clock_timestamp(&self._mem)

    cpdef uint64_t timestamp_ms(self):
        return live_clock_timestamp_ms(&self._mem)

    cpdef uint64_t timestamp_us(self):
        return live_clock_timestamp_us(&self._mem)

    cpdef uint64_t timestamp_ns(self):
        return live_clock_timestamp_ns(&self._mem)

    cpdef void register_default_handler(self, callback: Callable[[TimeEvent], None]):
        Condition.callable(callback, "callback")

        callback = create_pyo3_conversion_wrapper(callback)

        live_clock_register_default_handler(&self._mem, <PyObject *>callback)

    cpdef void set_time_alert_ns(
        self,
        str name,
        uint64_t alert_time_ns,
        callback: Callable[[TimeEvent], None] | None = None,
        bint allow_past = True,
    ):
        Condition.valid_string(name, "name")
        Condition.not_in(name, self.timer_names, "name", "self.timer_names")

        # Validate allow_past logic to prevent Rust errors
        cdef uint64_t ts_now = self.timestamp_ns()

        if not allow_past:
            if alert_time_ns < ts_now:
                alert_dt = datetime.fromtimestamp(alert_time_ns / 1e9).isoformat()
                current_dt = datetime.fromtimestamp(ts_now / 1e9).isoformat()
                raise ValueError(
                    f"Timer '{name}' alert time {alert_dt} was in the past "
                    f"(current time is {current_dt})"
                )

        if callback is not None:
            callback = create_pyo3_conversion_wrapper(callback)

        live_clock_set_time_alert(
            &self._mem,
            pystr_to_cstr(name),
            alert_time_ns,
            <PyObject *>callback,
            allow_past,
        )

    cpdef void set_timer_ns(
        self,
        str name,
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns,
        callback: Callable[[TimeEvent], None] | None = None,
        bint allow_past = True,
        bint fire_immediately = False,
    ):
        Condition.valid_string(name, "name")
        Condition.not_in(name, self.timer_names, "name", "self.timer_names")
        Condition.positive_int(interval_ns, "interval_ns")

        # Validate callback availability to prevent Rust panics
        # For LiveClock, we need either a callback or a default handler
        # Since we can't easily check default handler from Cython, we need some validation
        if callback is None:
            # If no callback provided, we rely on default handler being set
            # This will be validated by Rust, but we can't prevent the panic here
            pass

        # Validate allow_past logic to prevent Rust errors
        cdef uint64_t ts_now = self.timestamp_ns()
        cdef uint64_t next_event_time

        if not allow_past:
            if start_time_ns != 0:  # Only validate if start_time is explicitly set
                if fire_immediately:
                    next_event_time = start_time_ns
                else:
                    next_event_time = start_time_ns + interval_ns

                if next_event_time < ts_now:
                    from datetime import datetime
                    next_dt = datetime.fromtimestamp(next_event_time / 1e9).isoformat()
                    current_dt = datetime.fromtimestamp(ts_now / 1e9).isoformat()
                    raise ValueError(
                        f"Timer '{name}' next event time {next_dt} would be in the past "
                        f"(current time is {current_dt})"
                    )

        if callback is not None:
            callback = create_pyo3_conversion_wrapper(callback)

        live_clock_set_timer(
            &self._mem,
            pystr_to_cstr(name),
            interval_ns,
            start_time_ns,
            stop_time_ns,
            <PyObject *>callback,
            allow_past,
            fire_immediately,
        )

    cpdef uint64_t next_time_ns(self, str name):
        Condition.valid_string(name, "name")
        return live_clock_next_time(&self._mem, pystr_to_cstr(name))

    cpdef void cancel_timer(self, str name):
        Condition.valid_string(name, "name")
        Condition.is_in(name, self.timer_names, "name", "self.timer_names")

        live_clock_cancel_timer(&self._mem, pystr_to_cstr(name))

    cpdef void cancel_timers(self):
        cdef str name
        for name in self.timer_names:
            # Using a list of timer names from the property and passing this
            # to cancel_timer() handles the clean removal of both the handler
            # and timer.
            self.cancel_timer(name)


def create_pyo3_conversion_wrapper(callback) -> Callable:
    def wrapper(capsule):
        callback(capsule_to_time_event(capsule))

    return wrapper


cdef class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.

    Parameters
    ----------
    name : str
        The event name.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the time event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        str name not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        # Precondition: `name` validated in Rust
        self._mem = time_event_new(
            pystr_to_cstr(name),
            event_id._mem,
            ts_event,
            ts_init,
        )

    def __getstate__(self):
        return (
            self.to_str(),
            self.id.value,
            self.ts_event,
            self.ts_init,
        )

    def __setstate__(self, state):
        self._mem = time_event_new(
            pystr_to_cstr(state[0]),
            uuid4_from_cstr(pystr_to_cstr(state[1])),
            self.ts_event,
            self.ts_init,
        )

    cdef str to_str(self):
        return ustr_to_pystr(self._mem.name)

    def __eq__(self, TimeEvent other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return cstr_to_pystr(time_event_to_cstr(&self._mem))

    @property
    def name(self) -> str:
        """
        Return the name of the time event.

        Returns
        -------
        str

        """
        return ustr_to_pystr(self._mem.name)

    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        cdef UUID4 uuid4 = UUID4.__new__(UUID4)
        uuid4._mem = self._mem.event_id
        return uuid4

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        return self._mem.ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._mem.ts_init

    @staticmethod
    cdef TimeEvent from_mem_c(TimeEvent_t mem):
        cdef TimeEvent event = TimeEvent.__new__(TimeEvent)
        event._mem = mem
        return event


cdef inline TimeEvent capsule_to_time_event(capsule):
    cdef TimeEvent_t* ptr = <TimeEvent_t*>PyCapsule_GetPointer(capsule, NULL)
    cdef TimeEvent event = TimeEvent.__new__(TimeEvent)
    event._mem = ptr[0]
    return event


cdef class TimeEventHandler:
    """
    Represents a time event with its associated handler.

    Parameters
    ----------
    event : TimeEvent
        The time event to handle
    handler : Callable[[TimeEvent], None]
        The handler to call.

    """

    def __init__(
        self,
        TimeEvent event not None,
        handler not None: Callable[[TimeEvent], None],
    ) -> None:
        self.event = event
        self._handler = handler

    cpdef void handle(self):
        """
        Call the handler with the contained time event.
        """
        self._handler(self.event)

    def __eq__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event == other.event.ts_event

    def __lt__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event < other.event.ts_event

    def __le__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event <= other.event.ts_event

    def __gt__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event > other.event.ts_event

    def __ge__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event >= other.event.ts_event

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"event={repr(self.event)})"
        )


RECV = "<--"
SENT = "-->"
CMD = "[CMD]"
EVT = "[EVT]"
DOC = "[DOC]"
RPT = "[RPT]"
REQ = "[REQ]"
RES = "[RES]"


cdef void set_logging_clock_realtime_mode():
    logging_clock_set_realtime_mode()


cdef void set_logging_clock_static_mode():
    logging_clock_set_static_mode()


cdef void set_logging_clock_static_time(uint64_t time_ns):
    logging_clock_set_static_time(time_ns)


cpdef LogColor log_color_from_str(str value):
    return log_color_from_cstr(pystr_to_cstr(value))


cpdef str log_color_to_str(LogColor value):
    return cstr_to_pystr(log_color_to_cstr(value))


cpdef LogLevel log_level_from_str(str value):
    return log_level_from_cstr(pystr_to_cstr(value))


cpdef str log_level_to_str(LogLevel value):
    return cstr_to_pystr(log_level_to_cstr(value))


cdef class LogGuard:
    """
    Provides a `LogGuard` which serves as a token to signal the initialization
    of the logging subsystem. It also ensures that the global logger is flushed
    of any buffered records when the instance is destroyed.
    """

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            logger_drop(self._mem)


cpdef LogGuard init_logging(
    TraderId trader_id = None,
    str machine_id = None,
    UUID4 instance_id = None,
    LogLevel level_stdout = LogLevel.INFO,
    LogLevel level_file = LogLevel.OFF,
    str directory = None,
    str file_name = None,
    str file_format = None,
    dict component_levels: dict[ComponentId, LogLevel] = None,
    bint log_components_only = False,
    bint colors = True,
    bint bypass = False,
    bint print_config = False,
    uint64_t max_file_size = 0,
    uint32_t max_backup_count = 5,
):
    """
    Initialize the logging subsystem.

    Provides an interface into the logging subsystem implemented in Rust.

    This function should only be called once per process, at the beginning of the application
    run. Subsequent calls will raise a `RuntimeError`, as there can only be one `LogGuard`
    per initialized system.

    Parameters
    ----------
    trader_id : TraderId, optional
        The trader ID for the logger.
    machine_id : str, optional
        The machine ID.
    instance_id : UUID4, optional
        The instance ID.
    level_stdout : LogLevel, default ``INFO``
        The minimum log level to write to stdout.
    level_file : LogLevel, default ``OFF``
        The minimum log level to write to a file.
    directory : str, optional
        The path to the log file directory.
        If ``None`` then will write to the current working directory.
    file_name : str, optional
        The custom log file name (will use a '.log' suffix for plain text or '.json' for JSON).
        If ``None`` will not log to a file (unless `file_auto` is True).
    file_format : str { 'JSON' }, optional
        The log file format. If ``None`` (default) then will log in plain text.
        If set to 'JSON' then logs will be in JSON format.
    component_levels : dict[ComponentId, LogLevel]
        The additional per component log level filters, where keys are component
        IDs (e.g. actor/strategy IDs) and values are log levels.
    log_components_only : bool, default False
        If only components with explicit component-level filters should be logged.
        When enabled, only log messages from components that have been explicitly
        configured in `log_component_levels` will be output.
    colors : bool, default True
        If ANSI codes should be used to produce colored log lines.
    bypass : bool, default False
        If the output for the core logging subsystem is bypassed (useful for logging tests).
    print_config : bool, default False
        If the core logging configuration should be printed to stdout on initialization.
    max_file_size : uint64_t, default 0
        The maximum size of log files in bytes before rotation occurs.
        If set to 0, file rotation is disabled.
    max_backup_count : uint32_t, default 5
        The maximum number of backup log files to keep when rotating.

    Returns
    -------
    LogGuard

    Raises
    ------
    RuntimeError
        If the logging subsystem has already been initialized.

    """
    if trader_id is None:
        trader_id = TraderId("TRADER-000")
    if machine_id is None:
        machine_id = socket.gethostname()
    if instance_id is None:
        instance_id = UUID4()

    if logging_is_initialized():
        raise RuntimeError("Logging subsystem already initialized")

    cdef LogGuard_API log_guard_api = logging_init(
        trader_id._mem,
        instance_id._mem,
        level_stdout,
        level_file,
        pystr_to_cstr(directory) if directory else NULL,
        pystr_to_cstr(file_name) if file_name else NULL,
        pystr_to_cstr(file_format) if file_format else NULL,
        pybytes_to_cstr(msgspec.json.encode(component_levels)) if component_levels else NULL,
        colors,
        bypass,
        print_config,
        log_components_only,
        max_file_size,
        max_backup_count,
    )

    cdef LogGuard log_guard = LogGuard.__new__(LogGuard)
    log_guard._mem = log_guard_api
    return log_guard


LOGGING_PYO3 = False


cpdef bint is_logging_initialized():
    if LOGGING_PYO3:
        return True
    return <bint>logging_is_initialized()


cpdef bint is_logging_pyo3():
    return LOGGING_PYO3


cpdef void set_logging_pyo3(bint value):
    global LOGGING_PYO3
    LOGGING_PYO3 = value


cpdef void flush_logger():
    logger_flush()


cdef class Logger:
    """
    Provides a logger adapter into the logging subsystem.

    Parameters
    ----------
    name : str
        The name of the logger. This will appear within each log line.

    """

    def __init__(self, str name not None) -> None:
        Condition.valid_string(name, "name")

        self._name = name  # Reference to `name` needs to be kept alive
        self._name_ptr = pystr_to_cstr(self._name)

    @property
    def name(self) -> str:
        """
        Return the name of the logger.

        Returns
        -------
        str

        """
        return self._name

    cpdef void debug(
        self,
        str message,
        LogColor color = LogColor.NORMAL,
    ):
        """
        Log the given DEBUG level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        if LOGGING_PYO3:
            nautilus_pyo3.logger_log(
                nautilus_pyo3.LogLevel.DEBUG,
                nautilus_pyo3.LogColor(log_color_to_str(color)),
                self._name,
                message,
            )
            return

        if not logging_is_initialized():
            return

        logger_log(
            LogLevel.DEBUG,
            color,
            self._name_ptr,
            pystr_to_cstr(message) if message is not None else NULL,
        )

    cpdef void info(
        self, str message,
        LogColor color = LogColor.NORMAL,
    ):
        """
        Log the given INFO level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        if LOGGING_PYO3:
            nautilus_pyo3.logger_log(
                nautilus_pyo3.LogLevel.INFO,
                nautilus_pyo3.LogColor(log_color_to_str(color)),
                self._name,
                message,
            )
            return

        if not logging_is_initialized():
            return

        logger_log(
            LogLevel.INFO,
            color,
            self._name_ptr,
            pystr_to_cstr(message) if message is not None else NULL,
        )

    cpdef void warning(
        self,
        str message,
        LogColor color = LogColor.YELLOW,
    ):
        """
        Log the given WARNING level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        if LOGGING_PYO3:
            nautilus_pyo3.logger_log(
                nautilus_pyo3.LogLevel.WARNING,
                nautilus_pyo3.LogColor(log_color_to_str(color)),
                self._name,
                message,
            )
            return

        if not logging_is_initialized():
            return

        logger_log(
            LogLevel.WARNING,
            color,
            self._name_ptr,
            pystr_to_cstr(message) if message is not None else NULL,
        )

    cpdef void error(
        self,
        str message,
        LogColor color = LogColor.RED,
    ):
        """
        Log the given ERROR level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        if LOGGING_PYO3:
            nautilus_pyo3.logger_log(
                nautilus_pyo3.LogLevel.ERROR,
                nautilus_pyo3.LogColor(log_color_to_str(color)),
                self._name,
                message,
            )
            return

        if not logging_is_initialized():
            return

        logger_log(
            LogLevel.ERROR,
            color,
            self._name_ptr,
            pystr_to_cstr(message) if message is not None else NULL,
        )

    cpdef void exception(
        self,
        str message,
        ex,
    ):
        """
        Log the given exception including stack trace information.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        ex : Exception
            The exception to log.

        """
        Condition.not_none(ex, "ex")

        cdef str ex_string = f"{type(ex).__name__}({ex})"
        ex_type, ex_value, ex_traceback = sys.exc_info()
        stack_trace = traceback.format_exception(ex_type, ex_value, ex_traceback)

        cdef str stack_trace_lines = ""
        cdef str line
        for line in stack_trace[:len(stack_trace) - 1]:
            stack_trace_lines += line

        self.error(f"{message}\n{ex_string}\n{stack_trace_lines}")


cpdef void log_header(
    TraderId trader_id,
    str machine_id,
    UUID4 instance_id,
    str component,
):
    logging_log_header(
        trader_id._mem,
        pystr_to_cstr(machine_id),
        instance_id._mem,
        pystr_to_cstr(component),
    )


cpdef void log_sysinfo(str component):
    logging_log_sysinfo(pystr_to_cstr(component))



cpdef ComponentState component_state_from_str(str value):
    return component_state_from_cstr(pystr_to_cstr(value))


cpdef str component_state_to_str(ComponentState value):
    return cstr_to_pystr(component_state_to_cstr(value))


cpdef ComponentTrigger component_trigger_from_str(str value):
    return component_trigger_from_cstr(pystr_to_cstr(value))


cpdef str component_trigger_to_str(ComponentTrigger value):
    return cstr_to_pystr(component_trigger_to_cstr(value))


cdef dict[tuple[ComponentState, ComponentTrigger], ComponentState] _COMPONENT_STATE_TABLE = {
    (ComponentState.PRE_INITIALIZED, ComponentTrigger.INITIALIZE): ComponentState.READY,
    (ComponentState.READY, ComponentTrigger.RESET): ComponentState.RESETTING,  # Transitional state
    (ComponentState.READY, ComponentTrigger.START): ComponentState.STARTING,  # Transitional state
    (ComponentState.READY, ComponentTrigger.DISPOSE): ComponentState.DISPOSING,  # Transitional state
    (ComponentState.RESETTING, ComponentTrigger.RESET_COMPLETED): ComponentState.READY,
    (ComponentState.STARTING, ComponentTrigger.START_COMPLETED): ComponentState.RUNNING,
    (ComponentState.STARTING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.STARTING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.DEGRADE): ComponentState.DEGRADING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.RESUMING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.RESUMING, ComponentTrigger.RESUME_COMPLETED): ComponentState.RUNNING,
    (ComponentState.RESUMING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.STOPPING, ComponentTrigger.STOP_COMPLETED): ComponentState.STOPPED,
    (ComponentState.STOPPING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.RESET): ComponentState.RESETTING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.RESUME): ComponentState.RESUMING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.DISPOSE): ComponentState.DISPOSING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.DEGRADING, ComponentTrigger.DEGRADE_COMPLETED): ComponentState.DEGRADED,
    (ComponentState.DEGRADED, ComponentTrigger.RESUME): ComponentState.RESUMING,  # Transitional state
    (ComponentState.DEGRADED, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.DEGRADED, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transition state
    (ComponentState.DISPOSING, ComponentTrigger.DISPOSE_COMPLETED): ComponentState.DISPOSED,  # Terminal state
    (ComponentState.FAULTING, ComponentTrigger.FAULT_COMPLETED): ComponentState.FAULTED,  # Terminal state
}

cdef class ComponentFSMFactory:
    """
    Provides a generic component Finite-State Machine.
    """

    @staticmethod
    def get_state_transition_table() -> dict:
        """
        The default state transition table.

        Returns
        -------
        dict[int, int]
            C Enums.

        """
        return _COMPONENT_STATE_TABLE.copy()

    @staticmethod
    cdef create():
        """
        Create a new generic component FSM.

        Returns
        -------
        FiniteStateMachine

        """
        return FiniteStateMachine(
            state_transition_table=ComponentFSMFactory.get_state_transition_table(),
            initial_state=ComponentState.PRE_INITIALIZED,
            trigger_parser=component_trigger_to_str,
            state_parser=component_state_to_str,
        )


cdef class Component:
    """
    The base class for all system components.

    A component is not considered initialized until a message bus is registered
    (this either happens when one is passed to the constructor, or when
    registered with a trader).

    Thus, if the component does not receive a message bus through the constructor,
    then it will be in a ``PRE_INITIALIZED`` state, otherwise if one is passed
    then it will be in an ``INITIALIZED`` state.

    Parameters
    ----------
    clock : Clock
        The clock for the component.
    trader_id : TraderId, optional
        The trader ID associated with the component.
    component_id : Identifier, optional
        The component ID. If ``None`` is passed then the identifier will be
        taken from `type(self).__name__`.
    component_name : str, optional
        The custom component name.
    msgbus : MessageBus, optional
        The message bus for the component (required before initialized).
    config : NautilusConfig, optional
        The configuration for the component.

    Raises
    ------
    ValueError
        If `component_name` is not a valid string.
    TypeError
        If `config` is not of type `NautilusConfig`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        Clock clock not None,
        TraderId trader_id = None,
        Identifier component_id = None,
        str component_name = None,
        MessageBus msgbus = None,
        config: NautilusConfig | None = None,
    ):
        if component_id is None:
            component_id = ComponentId(type(self).__name__)
        if component_name is None:
            component_name = component_id.value
        Condition.valid_string(component_name, "component_name")
        Condition.type_or_none(config, NautilusConfig, "config")

        self.trader_id = msgbus.trader_id if msgbus is not None else None
        self.id = component_id
        self.type = type(self)

        self._clock = clock
        self._log = Logger(name=component_name)
        self._msgbus = msgbus
        self._fsm = ComponentFSMFactory.create()
        self._config = config.json_primitives() if config is not None else {}

        if self._msgbus is not None:
            self._initialize()

    def __eq__(self, Component other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __str__(self) -> str:
        return self.id.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.id.to_str()})"

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the components class.

        Returns
        -------
        str

        References
        ----------
        https://www.python.org/dev/peps/pep-3155/

        """
        return cls.__module__ + ':' + cls.__qualname__

    @property
    def state(self) -> ComponentState:
        """
        Return the components current state.

        Returns
        -------
        ComponentState

        """
        return PyComponentState(self._fsm.state)

    @property
    def is_initialized(self) -> bool:
        """
        Return whether the component has been initialized (component.state >= ``INITIALIZED``).

        Returns
        -------
        bool

        """
        return self._fsm.state >= ComponentState.READY

    @property
    def is_running(self) -> bool:
        """
        Return whether the current component state is ``RUNNING``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.RUNNING

    @property
    def is_stopped(self) -> bool:
        """
        Return whether the current component state is ``STOPPED``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.STOPPED

    @property
    def is_disposed(self) -> bool:
        """
        Return whether the current component state is ``DISPOSED``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.DISPOSED

    @property
    def is_degraded(self) -> bool:
        """
        Return whether the current component state is ``DEGRADED``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.DEGRADED

    @property
    def is_faulted(self) -> bool:
        """
        Return whether the current component state is ``FAULTED``.

        Returns
        -------
        bool

        """
        return self._fsm.state == ComponentState.FAULTED

    cdef void _change_clock(self, Clock clock):
        Condition.not_none(clock, "clock")

        self._clock = clock

    cdef void _change_msgbus(self, MessageBus msgbus):
        # As an additional system wiring check: if a message bus is being added
        # here, then there should not be an existing trader ID or message bus.
        Condition.not_none(msgbus, "msgbus")
        Condition.none(self.trader_id, "self.trader_id")
        Condition.none(self._msgbus, "self._msgbus")

        self.trader_id = msgbus.trader_id
        self._msgbus = msgbus
        self._initialize()

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _start(self):
        # Optionally override in subclass
        pass

    cpdef void _stop(self):
        # Optionally override in subclass
        pass

    cpdef void _resume(self):
        # Optionally override in subclass
        pass

    cpdef void _reset(self):
        # Optionally override in subclass
        pass

    cpdef void _dispose(self):
        # Cancel all active timers to prevent post-disposal execution
        if self._clock is not None:
            self._clock.cancel_timers()
        # Optionally override in subclass

    cpdef void _degrade(self):
        # Optionally override in subclass
        pass

    cpdef void _fault(self):
        # Optionally override in subclass
        pass

# -- COMMANDS -------------------------------------------------------------------------------------

    cdef void _initialize(self):
        # This is a protected method dependent on registration of a message bus
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.INITIALIZE,  # -> INITIALIZED
                is_transitory=False,
                action=None,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on initialize", e)
            raise

    cpdef void start(self):
        """
        Start the component.

        While executing `on_start()` any exception will be logged and reraised, then the component
        will remain in a ``STARTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.START,  # -> STARTING
                is_transitory=True,
                action=self._start,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on START", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.START_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void stop(self):
        """
        Stop the component.

        While executing `on_stop()` any exception will be logged and reraised, then the component
        will remain in a ``STOPPING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.STOP,  # -> STOPPING
                is_transitory=True,
                action=self._stop,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on STOP", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.STOP_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void resume(self):
        """
        Resume the component.

        While executing `on_resume()` any exception will be logged and reraised, then the component
        will remain in a ``RESUMING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.RESUME,  # -> RESUMING
                is_transitory=True,
                action=self._resume,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on RESUME", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.RESUME_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void reset(self):
        """
        Reset the component.

        All stateful fields are reset to their initial value.

        While executing `on_reset()` any exception will be logged and reraised, then the component
        will remain in a ``RESETTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.RESET,  # -> RESETTING
                is_transitory=True,
                action=self._reset,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on RESET", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.RESET_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void dispose(self):
        """
        Dispose of the component.

        While executing `on_dispose()` any exception will be logged and reraised, then the component
        will remain in a ``DISPOSING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.DISPOSE,  # -> DISPOSING
                is_transitory=True,
                action=self._dispose,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on DISPOSE", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.DISPOSE_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void degrade(self):
        """
        Degrade the component.

        While executing `on_degrade()` any exception will be logged and reraised, then the component
        will remain in a ``DEGRADING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.DEGRADE,  # -> DEGRADING
                is_transitory=True,
                action=self._degrade,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on DEGRADE", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.DEGRADE_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void fault(self):
        """
        Fault the component.

        Calling this method multiple times has the same effect as calling it once (it is idempotent).
        Once called, it cannot be reversed, and no other methods should be called on this instance.

        While executing `on_fault()` any exception will be logged and reraised, then the component
        will remain in a ``FAULTING`` state.

        Warnings
        --------
        Do not override.

        If the component is not in a valid state from which to execute this method,
        then the component state will not change, and an error will be logged.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.FAULT,  # -> FAULTING
                is_transitory=True,
                action=self._fault,
            )
        except Exception as e:
            self._log.exception(f"{repr(self)}: Error on FAULT", e)
            raise  # Halt state transition

        self._trigger_fsm(
            trigger=ComponentTrigger.FAULT_COMPLETED,
            is_transitory=False,
            action=None,
        )

    cpdef void shutdown_system(self, str reason = None):
        """
        Initiate a system-wide shutdown by generating and publishing a `ShutdownSystem` command.

        The command is handled by the system's `NautilusKernel`, which will invoke either `stop` (synchronously)
        or `stop_async` (asynchronously) depending on the execution context and the presence of an active event loop.

        Parameters
        ----------
        reason : str, optional
            The reason for issuing the shutdown command.

        """
        cdef ShutdownSystem command = ShutdownSystem(
            trader_id=self.trader_id,
            component_id=self.id,
            reason=reason,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        self._msgbus.publish("commands.system.shutdown", command)

# --------------------------------------------------------------------------------------------------

    cdef void _trigger_fsm(
        self,
        ComponentTrigger trigger,
        bint is_transitory,
        action: Callable[[None], None] | None = None,
    ):
        try:
            self._fsm.trigger(trigger)
        except InvalidStateTrigger as e:
            self._log.error(f"{repr(e)} state {self._fsm.state_string_c()}")
            return  # Guards against invalid state

        if is_transitory:
            self._log.debug(f"{self._fsm.state_string_c()}")
        else:
            self._log.info(f"{self._fsm.state_string_c()}")

        if action is not None:
            action()

        if self._fsm == ComponentState.PRE_INITIALIZED:
            return  # Cannot publish event

        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef ComponentStateChanged event = ComponentStateChanged(
            trader_id=self.trader_id,
            component_id=self.id,
            component_type=self.type.__name__,
            state=self._fsm.state,
            config=self._config,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

        self._msgbus.publish(
            topic=f"events.system.{self.id}",
            msg=event,
        )


cdef class MessageBus:
    """
    Provides a generic message bus to facilitate various messaging patterns.

    The bus provides both a producer and consumer API for Pub/Sub, Req/Rep, as
    well as direct point-to-point messaging to registered endpoints.

    Pub/Sub wildcard patterns for hierarchical topics are possible:
     - `*` asterisk represents one or more characters in a pattern.
     - `?` question mark represents a single character in a pattern.

    Given a topic and pattern potentially containing wildcard characters, i.e.
    `*` and `?`, where `?` can match any single character in the topic, and `*`
    can match any number of characters including zero characters.

    The asterisk in a wildcard matches any character zero or more times. For
    example, `comp*` matches anything beginning with `comp` which means `comp`,
    `complete`, and `computer` are all matched.

    A question mark matches a single character once. For example, `c?mp` matches
    `camp` and `comp`. The question mark can also be used more than once.
    For example, `c??p` would match both of the above examples and `coop`.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the message bus.
    clock : Clock
        The clock for the message bus.
    name : str, optional
        The custom name for the message bus.
    serializer : Serializer, optional
        The serializer for database operations.
    database : nautilus_pyo3.RedisMessageBusDatabase, optional
        The backing database for the message bus.
    config : MessageBusConfig, optional
        The configuration for the message bus.

    Raises
    ------
    ValueError
        If `name` is not ``None`` and not a valid string.

    Warnings
    --------
    This message bus is not thread-safe and must be called from the same thread
    as the event loop.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        Clock clock,
        UUID4 instance_id = None,
        str name = None,
        Serializer serializer = None,
        database: nautilus_pyo3.RedisMessageBusDatabase | None = None,
        config: Any | None = None,
    ) -> None:
        # Temporary fix for import error
        from nautilus_trader.common.config import MessageBusConfig

        if instance_id is None:
            instance_id = UUID4()
        if name is None:
            name = type(self).__name__
        Condition.valid_string(name, "name")
        if config is None:
            config = MessageBusConfig()
        Condition.type(config, MessageBusConfig, "config")

        self.trader_id = trader_id
        self.serializer = serializer
        self.has_backing = database is not None

        self._clock = clock
        self._log = Logger(name)
        self._database = database
        self._listeners = []

        # Validate configuration
        if config.buffer_interval_ms and config.buffer_interval_ms > 1000:
            self._log.warning(
                f"High `buffer_interval_ms` at {config.buffer_interval_ms}, "
                "recommended range is [10, 1000] milliseconds",
            )

        # Configuration
        self._log.info(f"{config.database=}", LogColor.BLUE)
        self._log.info(f"{config.encoding=}", LogColor.BLUE)
        self._log.info(f"{config.timestamps_as_iso8601=}", LogColor.BLUE)
        self._log.info(f"{config.buffer_interval_ms=}", LogColor.BLUE)
        self._log.info(f"{config.autotrim_mins=}", LogColor.BLUE)
        self._log.info(f"{config.use_trader_prefix=}", LogColor.BLUE)
        self._log.info(f"{config.use_trader_id=}", LogColor.BLUE)
        self._log.info(f"{config.use_instance_id=}", LogColor.BLUE)
        self._log.info(f"{config.streams_prefix=}", LogColor.BLUE)
        self._log.info(f"{config.types_filter=}", LogColor.BLUE)

        # Copy and clear `types_filter` before passing down to the core MessageBus
        cdef list types_filter = copy.copy(config.types_filter)
        if config.types_filter is not None:
            config.types_filter.clear()

        self._endpoints: dict[str, Callable[[Any], None]] = {}
        self._patterns: dict[str, Subscription[:]] = {}
        self._subscriptions: dict[Subscription, list[str]] = {}
        self._correlation_index: dict[UUID4, Callable[[Any], None]] = {}
        self._publishable_types = tuple(_EXTERNAL_PUBLISHABLE_TYPES)
        if types_filter is not None:
            self._publishable_types = tuple(o for o in _EXTERNAL_PUBLISHABLE_TYPES if o not in types_filter)
        self._streaming_types = set()
        self._resolved = False

        # Counters
        self.sent_count = 0
        self.req_count = 0
        self.res_count = 0
        self.pub_count = 0

    cpdef list endpoints(self):
        """
        Return all endpoint addresses registered with the message bus.

        Returns
        -------
        list[str]

        """
        return list(self._endpoints.keys())

    cpdef list topics(self):
        """
        Return all topics with active subscribers.

        Returns
        -------
        list[str]

        """
        return sorted(set([s.topic for s in self._subscriptions.keys()]))

    cpdef list subscriptions(self, str pattern = None):
        """
        Return all subscriptions matching the given topic `pattern`.

        Parameters
        ----------
        pattern : str, optional
            The topic pattern filter. May include wildcard characters `*` and `?`.
            If ``None`` then query is for **all** topics.

        Returns
        -------
        list[Subscription]

        """
        if pattern is None:
            pattern = "*"  # Wildcard
        Condition.valid_string(pattern, "pattern")

        return [s for s in self._subscriptions if is_matching(s.topic, pattern)]

    cpdef set streaming_types(self):
        """
        Return all types registered for external streaming -> internal publishing.

        Returns
        -------
        set[type]

        """
        return self._streaming_types.copy()

    cpdef bint has_subscribers(self, str pattern = None):
        """
        If the message bus has subscribers for the give topic `pattern`.

        Parameters
        ----------
        pattern : str, optional
            The topic filter. May include wildcard characters `*` and `?`.
            If ``None`` then query is for **all** topics.

        Returns
        -------
        bool

        """
        return len(self.subscriptions(pattern)) > 0

    cpdef bint is_subscribed(self, str topic, handler: Callable[[Any], None]):
        """
        Return if topic and handler is subscribed to the message bus.

        Does not consider any previous `priority`.

        Parameters
        ----------
        topic : str
            The topic of the subscription.
        handler : Callable[[Any], None]
            The handler of the subscription.

        Returns
        -------
        bool

        """
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")

        # Create subscription
        cdef Subscription sub = Subscription(
            topic=topic,
            handler=handler,
        )

        return sub in self._subscriptions

    cpdef bint is_pending_request(self, UUID4 request_id):
        """
        Return if the given `request_id` is still pending a response.

        Parameters
        ----------
        request_id : UUID4
            The request ID to check (to match the correlation_id).

        Returns
        -------
        bool

        """
        Condition.not_none(request_id, "request_id")

        return request_id in self._correlation_index

    cpdef bint is_streaming_type(self, type cls):
        """
        Return whether the given type has been registered for external message streaming.

        Returns
        -------
        bool
            True if registered, else False.

        """
        return cls in self._streaming_types

    cpdef void dispose(self):
        """
        Dispose of the message bus which will close the internal channel and thread.

        """
        self._log.debug("Closing message bus")

        if self._database is not None:
            self._database.close()

        self._log.info("Closed message bus")

    cpdef void register(self, str endpoint, handler: Callable[[Any], None]):
        """
        Register the given `handler` to receive messages at the `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to register.
        handler : Callable[[Any], None]
            The handler for the registration.

        Raises
        ------
        ValueError
            If `endpoint` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.
        KeyError
            If `endpoint` already registered.

        """
        Condition.valid_string(endpoint, "endpoint")
        Condition.callable(handler, "handler")
        Condition.not_in(endpoint, self._endpoints, "endpoint", "_endpoints")

        self._endpoints[endpoint] = handler

        self._log.debug(f"Added endpoint '{endpoint}' {handler}")

    cpdef void deregister(self, str endpoint, handler: Callable[[Any], None]):
        """
        Deregister the given `handler` from the `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to deregister.
        handler : Callable[[Any], None]
            The handler to deregister.

        Raises
        ------
        ValueError
            If `endpoint` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.
        KeyError
            If `endpoint` is not registered.
        ValueError
            If `handler` is not registered at the endpoint.

        """
        Condition.valid_string(endpoint, "endpoint")
        Condition.callable(handler, "handler")
        Condition.is_in(endpoint, self._endpoints, "endpoint", "self._endpoints")
        Condition.equal(handler, self._endpoints[endpoint], "handler", "self._endpoints[endpoint]")

        del self._endpoints[endpoint]

        self._log.debug(f"Removed endpoint '{endpoint}' {handler}")

    cpdef void add_streaming_type(self, type cls):
        """
        Register the given type for external->internal message bus streaming.

        Parameters
        ----------
        type : cls
            The type to add for streaming.

        """
        Condition.not_none(cls, "cls")

        self._streaming_types.add(cls)

        self._log.debug(f"Added streaming type {cls}")

    cpdef void add_listener(self, listener: nautilus_pyo3.MessageBusListener):
        """
        Adds the given listener to the message bus.

        Parameters
        ----------
        listener : nautilus_pyo3.MessageBusListener
            The listener to add.

        """
        self._listeners.append(listener)

    cpdef void send(self, str endpoint, msg: Any):
        """
        Send the given message to the given `endpoint` address.

        Parameters
        ----------
        endpoint : str
            The endpoint address to send the message to.
        msg : object
            The message to send.

        """
        Condition.not_none(endpoint, "endpoint")
        Condition.not_none(msg, "msg")

        handler = self._endpoints.get(endpoint)
        if handler is None:
            self._log.error(
                f"Cannot send message: no endpoint registered at '{endpoint}'",
            )
            return  # Cannot send

        handler(msg)
        self.sent_count += 1

    cpdef void request(self, str endpoint, Request request):
        """
        Handle the given `request`.

        Will log an error if the correlation ID already exists.

        Parameters
        ----------
        endpoint : str
            The endpoint address to send the request to.
        request : Request
            The request to handle.

        """
        Condition.not_none(endpoint, "endpoint")
        Condition.not_none(request, "request")

        if request.id in self._correlation_index:
            self._log.error(
                f"Cannot handle request: "
                f"duplicate ID {request.id} found in correlation index",
            )
            return  # Do not handle duplicates

        self._correlation_index[request.id] = request.callback

        handler = self._endpoints.get(endpoint)
        if handler is None:
            self._log.error(
                f"Cannot handle request: no endpoint registered at '{endpoint}'",
            )
            return  # Cannot handle

        handler(request)
        self.req_count += 1

    cpdef void response(self, Response response):
        """
        Handle the given `response`.

        Will log an error if the correlation ID is not found.

        Parameters
        ----------
        response : Response
            The response to handle

        """
        Condition.not_none(response, "response")

        callback = self._correlation_index.pop(response.correlation_id, None)
        if callback is None:
            self._log.error(
                f"Cannot handle response: "
                f"callback not found for correlation_id {response.correlation_id}",
            )
            return  # Cannot handle

        callback(response)
        self.res_count += 1

    cpdef void subscribe(
        self,
        str topic,
        handler: Callable[[Any], None],
        int priority = 0,
    ):
        """
        Subscribe to the given message `topic` with the given callback `handler`.

        Parameters
        ----------
        topic : str
            The topic for the subscription. May include wildcard characters
            `*` and `?`.
        handler : Callable[[Any], None]
            The handler for the subscription.
        priority : int, optional
            The priority for the subscription. Determines the ordering of
            handlers receiving messages being processed, higher priority
            handlers will receive messages prior to lower priority handlers.

        Raises
        ------
        ValueError
            If `topic` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.

        Warnings
        --------
        Assigning priority handling is an advanced feature which *shouldn't
        normally be needed by most users*. **Only assign a higher priority to the
        subscription if you are certain of what you're doing**. If an inappropriate
        priority is assigned then the handler may receive messages before core
        system components have been able to process necessary calculations and
        produce potential side effects for logically sound behavior.

        """
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")

        # Create subscription
        cdef Subscription sub = Subscription(
            topic=topic,
            handler=handler,
            priority=priority,
        )

        # Check if already exists
        if sub in self._subscriptions:
            self._log.debug(f"{sub} already exists")
            return

        cdef list matches = []
        cdef list patterns = list(self._patterns.keys())

        cdef str pattern
        cdef list subs
        for pattern in patterns:
            if is_matching(topic, pattern):
                subs = list(self._patterns[pattern])
                subs.append(sub)
                subs = sorted(subs, reverse=True)
                self._patterns[pattern] = np.ascontiguousarray(subs, dtype=Subscription)
                matches.append(pattern)

        self._subscriptions[sub] = sorted(matches)

        self._resolved = False

        self._log.debug(f"Added {sub}")

    cpdef void unsubscribe(self, str topic, handler: Callable[[Any], None]):
        """
        Unsubscribe the given callback `handler` from the given message `topic`.

        Parameters
        ----------
        topic : str, optional
            The topic to unsubscribe from. May include wildcard characters `*`
            and `?`.
        handler : Callable[[Any], None]
            The handler for the subscription.

        Raises
        ------
        ValueError
            If `topic` is not a valid string.
        ValueError
            If `handler` is not of type `Callable`.

        """
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")

        cdef Subscription sub = Subscription(topic=topic, handler=handler)

        cdef list patterns = self._subscriptions.get(sub)

        # Check if exists
        if patterns is None:
            self._log.warning(f"{sub} not found")
            return

        cdef str pattern
        for pattern in patterns:
            subs = list(self._patterns[pattern])
            subs.remove(sub)
            subs = sorted(subs, reverse=True)
            self._patterns[pattern] = np.ascontiguousarray(subs, dtype=Subscription)

        del self._subscriptions[sub]

        self._resolved = False

        self._log.debug(f"Removed {sub}")

    cpdef void publish(self, str topic, msg: Any, bint external_pub = True):
        """
        Publish the given message for the given `topic`.

        Subscription handlers will receive the message in priority order
        (highest first).

        Parameters
        ----------
        topic : str
            The topic to publish on.
        msg : object
            The message to publish.
        external_pub : bool, default True
            If the message should also be published externally.

        """
        self.publish_c(topic, msg, external_pub)

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef void publish_c(self, str topic, msg: Any, bint external_pub = True):
        Condition.not_none(topic, "topic")
        Condition.not_none(msg, "msg")

        # Get all subscriptions matching topic pattern
        # Note: cannot use truthiness on array
        cdef Subscription[:] subs = self._patterns.get(topic)
        if subs is None or (not self._resolved and len(subs) == 0):
            # Add the topic pattern and get matching subscribers
            subs = self._resolve_subscriptions(topic)
            self._resolved = True

        # Send message to all matched subscribers
        cdef:
            int i
            Subscription sub
        for i in range(len(subs)):
            sub = subs[i]
            sub.handler(msg)

        # Publish externally (if configured)
        cdef bytes payload_bytes = None
        if isinstance(msg, self._publishable_types):
            if external_pub and self._database is not None and not self._database.is_closed():
                if isinstance(msg, bytes):
                    payload_bytes = msg
                else:
                    payload_bytes = self.serializer.serialize(msg)

                self._database.publish(
                    topic,
                    payload_bytes,
                )

            for listener in self._listeners:
                if listener.is_closed():
                    continue

                if payload_bytes is None:
                    if isinstance(msg, bytes):
                        payload_bytes = msg
                    else:
                        payload_bytes = self.serializer.serialize(msg)

                listener.publish(topic, payload_bytes)

        self.pub_count += 1

    cdef Subscription[:] _resolve_subscriptions(self, str topic):
        cdef list subs_list = []
        cdef Subscription existing_sub
        # Copy to handle subscription changes on iteration
        for existing_sub in self._subscriptions.copy():
            if is_matching(topic, existing_sub.topic):
                subs_list.append(existing_sub)

        subs_list = sorted(subs_list, reverse=True)
        cdef Subscription[:] subs_array = np.ascontiguousarray(subs_list, dtype=Subscription)
        self._patterns[topic] = subs_array

        cdef list matches
        for sub in subs_array:
            matches = self._subscriptions.get(sub, [])
            if topic not in matches:
                matches.append(topic)
            self._subscriptions[sub] = sorted(matches)

        return subs_array


cdef inline bint is_matching(str topic, str pattern):
    return is_matching_ffi(pystr_to_cstr(topic), pystr_to_cstr(pattern))


# Python wrapper for test access
def is_matching_py(str topic, str pattern) -> bool:
    return is_matching(topic, pattern)


cdef class Subscription:
    """
    Represents a subscription to a particular topic.

    This is an internal class intended to be used by the message bus to organize
    topics and their subscribers.

    Parameters
    ----------
    topic : str
        The topic for the subscription. May include wildcard characters `*` and `?`.
    handler : Callable[[Message], None]
        The handler for the subscription.
    priority : int
        The priority for the subscription.

    Raises
    ------
    ValueError
        If `topic` is not a valid string.
    ValueError
        If `handler` is not of type `Callable`.
    ValueError
        If `priority` is negative (< 0).

    Notes
    -----
    The subscription equality is determined by the topic and handler,
    priority is not considered (and could change).
    """

    def __init__(
        self,
        str topic,
        handler not None: Callable[[Any], None],
        int priority=0,
    ):
        Condition.valid_string(topic, "topic")
        Condition.callable(handler, "handler")
        Condition.not_negative_int(priority, "priority")

        self.topic = topic
        self.handler = handler
        self.priority = priority

    def __eq__(self, Subscription other) -> bool:
        return self.topic == other.topic and self.handler == other.handler

    def __lt__(self, Subscription other) -> bool:
        return self.priority < other.priority

    def __le__(self, Subscription other) -> bool:
        return self.priority <= other.priority

    def __gt__(self, Subscription other) -> bool:
        return self.priority > other.priority

    def __ge__(self, Subscription other) -> bool:
        return self.priority >= other.priority

    def __hash__(self) -> int:
        # Convert handler to string to avoid builtin_function_or_method hashing issues
        return hash((self.topic, str(self.handler)))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"topic={self.topic}, "
            f"handler={self.handler}, "
            f"priority={self.priority})"
        )


cdef class Throttler:
    """
    Provides a generic throttler which can either buffer or drop messages.

    Will throttle messages to the given maximum limit-interval rate.
    If an `output_drop` handler is provided, then will drop messages which
    would exceed the rate limit. Otherwise will buffer messages until within
    the rate limit, then send.

    Parameters
    ----------
    name : str
        The unique name of the throttler.
    limit : int
        The limit setting for the throttling.
    interval : timedelta
        The interval setting for the throttling.
    clock : Clock
        The clock for the throttler.
    output_send : Callable[[Any], None]
        The output handler to send messages from the throttler.
    output_drop : Callable[[Any], None], optional
        The output handler to drop messages from the throttler.
        If ``None`` then messages will be buffered.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    ValueError
        If `limit` is not positive (> 0).
    ValueError
        If `interval` is not positive (> 0).
    ValueError
        If `output_send` is not of type `Callable`.
    ValueError
        If `output_drop` is not of type `Callable` or ``None``.

    Warnings
    --------
    This throttler is not thread-safe and must be called from the same thread as
    the event loop.

    The internal buffer queue is unbounded and so a bounded queue should be
    upstream.

    """

    def __init__(
        self,
        str name,
        int limit,
        timedelta interval not None,
        Clock clock not None,
        output_send not None: Callable[[Any], None],
        output_drop: Callable[[Any], None] | None = None,
    ) -> None:
        Condition.valid_string(name, "name")
        Condition.positive_int(limit, "limit")
        Condition.positive(interval.total_seconds(), "interval.total_seconds()")
        Condition.callable(output_send, "output_send")
        Condition.callable_or_none(output_drop, "output_drop")

        self._clock = clock
        self._log = Logger(name=f"Throttler-{name}")
        self._interval_ns = secs_to_nanos(interval.total_seconds())
        self._buffer = deque()
        self._timer_name = f"{name}|DEQUE"
        self._timestamps = deque(maxlen=limit)
        self._output_send = output_send
        self._output_drop = output_drop
        self._warm = False  # If throttler has sent at least limit number of msgs

        self.name = name
        self.limit = limit
        self.interval = interval
        self.is_limiting = False
        self.recv_count = 0
        self.sent_count = 0

        self._log.info("READY")

    @property
    def qsize(self) -> int:
        """
        Return the qsize of the internal buffer.

        Returns
        -------
        int

        """
        return len(self._buffer)

    cpdef void reset(self):
        """
        Reset the state of the throttler.

        """
        self._buffer.clear()
        self._warm = False
        self.recv_count = 0
        self.sent_count = 0
        self.is_limiting = False

    cpdef double used(self):
        """
        Return the percentage of maximum rate currently used.

        Returns
        -------
        double
            [0, 1.0].

        """
        if not self._warm:
            if self.sent_count < 2:
                return 0

        cdef int64_t diff = self._clock.timestamp_ns() - self._timestamps[-1]
        diff = max_uint64(0, self._interval_ns - diff)
        cdef double used = <double>diff / <double>self._interval_ns

        if not self._warm:
            used *= <double>self.sent_count / <double>self.limit

        return used

    cpdef void send(self, msg):
        """
        Send the given message through the throttler.

        Parameters
        ----------
        msg : object
            The message to send.

        """
        self.recv_count += 1

        # Throttling is active
        if self.is_limiting:
            self._limit_msg(msg)
            return

        # Check msg rate
        cdef int64_t delta_next = self._delta_next()
        if delta_next <= 0:
            self._send_msg(msg)
        else:
            # Start throttling
            self._limit_msg(msg)

    cdef int64_t _delta_next(self):
        if not self._warm:
            if self.sent_count < self.limit:
                return 0
            self._warm = True

        cdef int64_t diff = self._clock.timestamp_ns() - self._timestamps[-1]
        return self._interval_ns - diff

    cdef void _limit_msg(self, msg):
        if self._output_drop is None:
            # Buffer
            self._buffer.appendleft(msg)
            timer_target = self._process
            self._log.warning(f"Buffering {msg}")
        else:
            # Drop
            self._output_drop(msg)
            timer_target = self._resume
            self._log.warning(f"Dropped {msg}")

        if not self.is_limiting:
            self._set_timer(timer_target)
            self.is_limiting = True

    cdef void _set_timer(self, handler: Callable[[TimeEvent], None]):
        # Cancel any existing timer
        if self._timer_name in self._clock.timer_names:
            self._clock.cancel_timer(self._timer_name)

        self._clock.set_time_alert_ns(
            name=self._timer_name,
            alert_time_ns=self._clock.timestamp_ns() + self._delta_next(),
            callback=handler,
        )

    cpdef void _process(self, TimeEvent event):
        # Send next msg on buffer
        msg = self._buffer.pop()
        self._send_msg(msg)

        # Send remaining messages if within rate
        cdef int64_t delta_next
        while self._buffer:
            delta_next = self._delta_next()
            msg = self._buffer.pop()
            if delta_next <= 0:
                self._send_msg(msg)
            else:
                self._set_timer(self._process)
                return

        # No longer throttling
        self.is_limiting = False

    cpdef void _resume(self, TimeEvent event):
        self.is_limiting = False

    cdef void _send_msg(self, msg):
        self._timestamps.appendleft(self._clock.timestamp_ns())
        self._output_send(msg)
        self.sent_count += 1


cdef inline uint64_t max_uint64(uint64_t a, uint64_t b):
    if a > b:
        return a
    else:
        return b
