# -------------------------------------------------------------------------------------------------
# <copyright file="clock.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid

from cpython.datetime cimport datetime, timedelta
from datetime import timezone
from threading import Timer as TimerThread
from typing import List, Dict, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.clock cimport TestTimer
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.identifiers cimport Label
from nautilus_trader.model.events cimport TimeEvent

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
_UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)


cdef class Timer:
    """
    The base class for all timers.
    """

    def __init__(self,
                 Label label,
                 timedelta interval,
                 datetime start_time,
                 datetime stop_time):
        """
        Initializes a new instance of the Timer class.

        :param label: The label for the timer.
        :param interval: The time interval for the timer (not negative).
        :param start_time: The start datetime for the timer (UTC).
        :param stop_time: The stop datetime for the timer (UTC).
        """
        Condition.positive(interval.total_seconds(), 'interval')
        if stop_time:
            Condition.true(start_time + interval <= stop_time, 'start_time + interval <= stop_time')

        self.label = label
        self.interval = interval
        self.start_time = start_time
        self.next_time = start_time + interval
        self.stop_time = stop_time
        self.expired = False

    cpdef TimeEvent iterate_event(self, datetime now):
        """
        Sets the next time and checks if expired.
        """
        cdef TimeEvent event = TimeEvent(self.label, GUID(uuid.uuid4()), self.next_time)

        self.next_time += self.interval
        if self.stop_time and now >= self.stop_time:
            self.expired = True

        return event

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not raise an event).
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    def __hash__(self) -> int:
        """"
        Return a hash representation of this object.

        :return int.
        """
        return hash(self.label.value)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return (f"Timer("
                f"label={self.label.value}, "
                f"interval={self.interval}, "
                f"start_time={self.start_time}, "
                f"next_time={self.next_time}, "
                f"stop_time={self.stop_time})")

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__str__} object at {id(self)}>"


cdef class TestTimer(Timer):
    """
    Provides a fake timer for backtesting and unit testing.
    """

    def __init__(self,
                 Label label,
                 timedelta interval,
                 datetime start_time,
                 datetime stop_time=None):
        """
        Initializes a new instance of the TestTimer class.

        :param label: The label for the timer.
        :param interval: The time interval for the timer (not negative).
        :param start_time: The stop datetime for the timer (UTC).
        :param stop_time: The optional stop datetime for the timer (UTC).
        """
        # Condition: interval checked in base class
        # Condition: stop_time checked in base class

        super().__init__(label, interval, start_time, stop_time)

    cpdef list advance(self, datetime to_time):
        """
        Return a list of time events by advancing the test timer forward to 
        the given time. A time event is appended for each time a next event is
        <= the given to_time.

        :param to_time: The time to advance the test timer to.
        :return List[TimeEvent].
        """
        cdef list time_events = []  # type: List[TimeEvent]
        while not self.expired and to_time >= self.next_time:
            time_events.append(self.iterate_event(self.next_time))

        return time_events

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self.expired = True


cdef class LiveTimer(Timer):
    """
    Provides a timer for live trading.
    """

    def __init__(self,
                 Label label,
                 function,
                 timedelta interval,
                 datetime now,
                 datetime start_time,
                 datetime stop_time=None):
        """
        Initializes a new instance of the LiveTimer class.

        :param label: The label for the timer.
        :param function: The function to call at the next time.
        :param interval: The time interval for the timer.
        :param now: The datetime now (UTC).
        :param start_time: The start datetime for the timer (UTC).
        :param stop_time: The stop datetime for the timer (UTC).
        """
        # Condition: interval checked in base class
        # Condition: stop_time checked in base class

        super().__init__(label, interval, start_time, stop_time)

        self._function = function
        self._internal = self._start_timer(now)

    cpdef void repeat(self, datetime now) except *:
        """
        Continue the timer.
        """
        self._internal = self._start_timer(now)

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self._internal.cancel()

    cdef object _start_timer(self, datetime now):
        timer = TimerThread(
            interval=(self.next_time - now).total_seconds(),
            function=self._function,
            args=[self])
        timer.daemon = True
        timer.start()

        return timer


cdef class Clock:
    """
    The base class for all clocks. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the Clock class.
        """
        self._log = None
        self._timers = {}    # type: Dict[Label, Timer]
        self._handlers = {}  # type: Dict[Label, Callable]
        self._default_handler = None

        self.next_event_time = None
        self.has_timers = False
        self.is_logger_registered = False
        self.is_default_handler_registered = False

    cpdef datetime time_now(self):
        """
        Return the current datetime of the clock (UTC).
        
        :return datetime.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef timedelta get_delta(self, datetime time):
        """
        Return the timedelta from the given time.
        
        :return timedelta.
        """
        return self.time_now() - time

    cpdef list get_timer_labels(self):
        """
        Return the timer labels held by the clock.
        
        :return List[Label].
        """
        return list(self._timers.keys())

    cpdef void register_logger(self, LoggerAdapter logger):
        """
        Register the given logger with the clock.
        
        :param logger: The logger to register.
        """
        self._log = logger
        self.is_logger_registered = True

    cpdef void register_default_handler(self, handler: Callable) except *:
        """
        Register the given handler as the clocks default handler.
        
        :param handler: The handler to register (must be Callable).
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._default_handler = handler
        self.is_default_handler_registered = True
        if self.is_logger_registered:
            self._log.debug(f"Registered default handler {handler}.")

    cpdef void set_time_alert(
            self,
            Label label,
            datetime alert_time,
            handler=None) except *:
        """
        Set a time alert for the given time. When the time is reached the 
        handler will be passed the TimeEvent containing the timers unique label.

        :param label: The label for the alert (must be unique for this clock).
        :param alert_time: The time for the alert.
        :param handler: The optional handler to receive time events (if None then must be Callable).
        :raises ConditionFailed: If the label is not unique for this clock.
        :raises ConditionFailed: If the alert_time is not >= the clocks current time.
        :raises ConditionFailed: If the handler is not of type Callable or None.
        :raises ConditionFailed: If the handler is None and no default handler is registered.
        """
        if handler is None:
            handler = self._default_handler
        Condition.not_in(label, self._timers, 'label', 'timers')
        Condition.not_in(label, self._handlers, 'label', 'handlers')
        cdef datetime now = self.time_now()
        Condition.true(alert_time >= now, 'alert_time >= time_now()')
        Condition.type(handler, Callable, 'handler')

        cdef Timer timer = self._get_timer(
            label=label,
            interval=alert_time - now,
            now=now,
            start_time=now,
            stop_time=alert_time)
        self._add_timer(timer, handler)

        if self.is_logger_registered:
            self._log.info(f"Set Timer('{label.value}') with alert for {alert_time}.")

    cpdef void set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time=None,
            datetime stop_time=None,
            handler=None) except *:
        """
        Set a timer with the given interval. The timer will run from the start 
        time (optionally until the stop time). When the intervals are reached the 
        handlers will be passed the TimeEvent containing the timers unique label.

        :param label: The label for the timer (must be unique for this clock).
        :param interval: The time interval for the timer.
        :param start_time: The optional start time for the timer (if None then starts immediately).
        :param stop_time: The optional stop time for the timer (if None then repeats indefinitely).
        :param handler: The optional handler to receive time events (if None then must be Callable).
        :raises ConditionFailed: If the label is not unique for this clock.
        :raises ConditionFailed: If the interval is not positive (> 0).
        :raises ConditionFailed: If the stop_time is not None and stop_time < time_now.
        :raises ConditionFailed: If the stop_time is not None and start_time + interval > stop_time.
        :raises ConditionFailed: If the handler is not of type Callable or None.
        :raises ConditionFailed: If the handler is None and no default handler is registered.
        """
        if handler is None:
            handler = self._default_handler
        Condition.not_in(label, self._timers, 'label', 'timers')
        Condition.not_in(label, self._handlers, 'label', 'handlers')
        Condition.true(interval.total_seconds() > 0, 'interval positive')
        Condition.type(handler, Callable, 'handler')
        cdef datetime now = self.time_now()
        if start_time is None:
            start_time = now
        if stop_time is not None:
            Condition.true(stop_time > now, 'stop_time > now')
            Condition.true(start_time + interval <= stop_time, 'start_time + interval <= stop_time')

        cdef Timer timer = self._get_timer(
            label=label,
            interval=interval,
            now=now,
            start_time=start_time,
            stop_time=stop_time)
        self._add_timer(timer, handler)

        if self.is_logger_registered:
            self._log.info(f"Started {timer}.")

    cpdef void cancel_timer(self, Label label) except *:
        """
        Cancel the timer corresponding to the given label.

        :param label: The label for the timer to cancel.
        """
        timer = self._timers.pop(label, None)
        if timer is None:
            if self.is_logger_registered:
                self._log.warning(f"Cannot cancel timer (no timer found with label '{label.value}').")
        else:
            timer.cancel()
            if self.is_logger_registered:
                self._log.info(f"Cancelled Timer(label={timer.label.value}).")

        self._handlers.pop(label, None)
        self._update_timing()

    cpdef void cancel_all_timers(self) except *:
        """
        Cancel all timers inside the clock.
        """
        cdef Label label
        for label in self.get_timer_labels():
            self.cancel_timer(label)

    cdef object _get_timer(
            self,
            Label label,
            timedelta interval,
            datetime now,
            datetime start_time,
            datetime stop_time):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _add_timer(self, Timer timer, handler) except *:
        self._timers[timer.label] = timer
        self._handlers[timer.label] = handler
        self._update_timing()

    cdef void _remove_timer(self, Timer timer) except *:
        self._timers.pop(timer.label, None)
        self._handlers.pop(timer.label, None)
        self._update_timing()

    cdef void _update_timing(self) except *:
        if len(self._timers) == 0:
            self.has_timers = False
            self.next_event_time = None
        else:
            self.has_timers = True
            self.next_event_time = sorted(timer.next_time for timer in self._timers.values())[0]


cdef class TestClock(Clock):
    """
    Provides a clock for backtesting and unit testing.
    """

    def __init__(self, datetime initial_time=_UNIX_EPOCH):
        """
        Initializes a new instance of the TestClock class.

        :param initial_time: The initial time for the clock.
        """
        super().__init__()
        self._time = initial_time
        self._pending_events = {}
        self.is_test_clock = True

    cpdef datetime time_now(self):
        """
        Return the current datetime of the clock (UTC).

        :return datetime.
        """
        return self._time

    cpdef void set_time(self, datetime to_time):
        """
        Set the clocks datetime to the given time (UTC).
        
        :param to_time: The time to set to.
        """
        self._time = to_time

    cpdef void advance_time(self, datetime to_time):
        """
        Iterates the clocks time to the given datetime.
        
        :param to_time: The datetime to iterate the test clock to.
        :return Dict[TimeEvent].
        """
        assert to_time >= self.time_now()

        self._pending_events = {}  # type: Dict[TimeEvent, Callable]

        if not self.has_timers or to_time < self.next_event_time:
            return # No timer events to iterate

        # Iterate timers
        cdef TestTimer timer
        cdef TimeEvent event
        for timer in self._timers.copy().values():  # To avoid resize during loop
            for event in timer.advance(to_time):
                self._pending_events[event] = self._handlers[timer.label]
            if timer.expired:
                self._remove_timer(timer)

        self._update_timing()
        self._time = to_time
        self._pending_events = dict(sorted(self._pending_events.items()))

    cpdef dict get_pending_events(self):
        return self._pending_events

    cdef object _get_timer(
            self,
            Label label,
            timedelta interval,
            datetime now,
            datetime start_time,
            datetime stop_time):
        return TestTimer(
            label=label,
            interval=interval,
            start_time=start_time,
            stop_time=stop_time)


cdef class LiveClock(Clock):
    """
    Provides a clock for live trading. All times are timezone aware UTC.
    """

    cpdef datetime time_now(self):
        """
        Return the current datetime of the clock (UTC).
        
        :return datetime.
        """
        return datetime.now(timezone.utc)

    cdef object _get_timer(
            self,
            Label label,
            timedelta interval,
            datetime now,
            datetime start_time,
            datetime stop_time):
        return LiveTimer(
            label=label,
            function=self._raise_time_event,
            interval=interval,
            now=now,
            start_time=start_time,
            stop_time=stop_time)

    cpdef void _raise_time_event(self, LiveTimer timer) except *:
        cdef datetime now = self.time_now()
        self._handle_time_event(timer.iterate_event(now))

        if timer.expired:
            self._remove_timer(timer)
        else:  # Continue timing
            timer.repeat(now)
            self._update_timing()

    cdef void _handle_time_event(self, TimeEvent event) except *:
        handler = self._handlers.get(event.label)
        if handler:
            handler(event)
