#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="clock.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from uuid import uuid4
from cpython.datetime cimport datetime, timedelta
from datetime import timezone
from threading import Timer
from typing import Dict, Callable

from inv_trader.common.clock cimport TestTimer
from inv_trader.core.precondition cimport Precondition
from inv_trader.model.identifiers cimport Label, GUID
from inv_trader.model.events cimport TimeEvent

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
cdef datetime UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)


cdef class Clock:
    """
    The abstract base class for all clocks. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the Clock class.
        """
        self._unix_epoch = UNIX_EPOCH

    cpdef datetime time_now(self):
        """
        Return the current UTC datetime of the clock.
        
        :return: datetime.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef datetime unix_epoch(self):
        """
        Return Unix time (also known as POSIX time or epoch time) is a system for
        describing instants in time, defined as the number of seconds that have
        elapsed since 00:00:00 Coordinated Universal Time (UTC), on Thursday,
        1 January 1970, minus the number of leap seconds which have taken place
        since then.
        
        :return: datetime.
        """
        return self._unix_epoch

    cpdef timedelta get_delta(self, datetime time):
        """
        Return the timedelta from the given time.
        
        :return: timedelta.
        """
        return self.time_now() - time

    cpdef set_time_alert(
            self,
            Label label,
            datetime alert_time,
            handler: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef cancel_time_alert(self, Label label):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time,
            datetime stop_time,
            handler: Callable):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef cancel_timer(self, Label label):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef list get_labels(self):
        """
        Return the timer labels held by the clock
        
        :return: List[Label].
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef stop_all_timers(self):
        """
        Stop all alerts and timers inside the clock.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class LiveClock(Clock):
    """
    Provides a clock for live trading. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the LiveClock class.
        """
        super().__init__()
        self._timers = {}   # type: Dict[Label, (Timer, Callable)]

    cpdef datetime time_now(self):
        """
        Return the current UTC datetime of the clock.
        
        :return: datetime.
        """
        return datetime.now(timezone.utc)

    cpdef set_time_alert(
            self,
            Label label,
            datetime alert_time,
            handler: Callable):
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, on_event() is passed the TimeEvent containing the
        alerts unique label.

        Note: The timer thread will begin immediately.

        :param label: The label for the alert (must be unique).
        :param alert_time: The time for the alert.
        :param handler: The handler method for the alert.
        :raises ValueError: If the label is not unique for this clock.
        :raises ValueError: If the alert_time is not > than the clocks current time.
        """
        Precondition.true(alert_time > self.time_now(), 'alert_time > time_now()')
        Precondition.not_in(label, self._timers, 'label', 'timers')

        timer = Timer(
            interval=(alert_time - self.time_now()).total_seconds(),
            function=self._raise_time_event,
            args=[label, alert_time])

        timer.start()
        self._timers[label] = (timer, handler)

    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time,
            datetime stop_time,
            handler: Callable):
        """
        Set a timer with the given interval (timedelta). The timer will run from
        the start time (optionally until the stop time). When the interval is
        reached and the strategy is running, the on_event() is passed the
        TimeEvent containing the timers unique label.

        Note: The timer thread will begin immediately.

        :param label: The label for the timer (must be unique).
        :param interval: The time delta interval for the timer.
        :param start_time: The start time for the timer (optional can be None - then starts immediately).
        :param stop_time: The stop time for the timer (optional can be None - then repeats indefinitely).
        :param handler: The handler method for the alert.
        :raises ValueError: If the label is not unique.
        :raises ValueError: If the handler is not of type Callable.
        :raises ValueError: If the start_time is not None and not >= the current time (UTC).
        :raises ValueError: If the stop_time is not None and not > than the start_time.
        :raises ValueError: If the stop_time is not None and start_time plus interval is greater
        than the stop_time.
        """
        Precondition.not_in(label, self._timers, 'label', 'timers')
        Precondition.type(handler, Callable, 'handler')

        if start_time is not None:
            Precondition.true(start_time >= self.time_now(),
                              'start_time >= self.clock.time_now()')
        else:
            start_time = self.time_now()
        if stop_time is not None:
            Precondition.true(stop_time > start_time, 'stop_time > start_time')
            Precondition.true(start_time + interval <= stop_time,
                              'start_time + interval <= stop_time')

        cdef datetime alert_time = start_time + interval
        cdef float delay = (alert_time - self.time_now()).total_seconds()
        if stop_time is None:
            timer = Timer(
                interval=delay,
                function=self._repeating_timer,
                args=[label, alert_time, interval, stop_time])
        else:
            timer = Timer(
                interval=delay,
                function=self._raise_time_event,
                args=[label, alert_time])

        timer.start()
        self._timers[label] = (timer, handler)

    cpdef cancel_time_alert(self, Label label):
        """
        Cancel the time alert corresponding to the given label.

        :param label: The label for the alert to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        Precondition.is_in(label, self._timers, 'label', 'timers')

        self._timers[label][0].cancel()
        del self._timers[label]

    cpdef cancel_timer(self, Label label):
        """
        Cancel the timer corresponding to the given unique label.

        :param label: The label for the timer to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        Precondition.is_in(label, self._timers, 'label', 'timers')

        self._timers[label][0].cancel()
        del self._timers[label]

    cpdef list get_labels(self):
        """
        Return the timer labels held by the clock
        
        :return: List[Label].
        """
        return list(self._timers)

    cpdef stop_all_timers(self):
        """
        Stops all alerts and timers inside the clock.
        """
        for label, timer in self._timers.items():
            timer[0].cancel()

    cpdef void _raise_time_event(self, Label label, datetime alert_time):
        """
        Create a new TimeEvent and pass it to the registered handler.
        """
        self._timers[label][1](TimeEvent(label, GUID(uuid4()), alert_time))
        del self._timers[label]

    cpdef void _repeating_timer(
            self,
            Label label,
            datetime alert_time,
            timedelta interval,
            datetime stop_time):
        """
        Create a new TimeEvent and pass it into _update_events().
        Then start a timer for the next time event.
        """
        self._timers[label][1](TimeEvent(label, GUID(uuid4()), alert_time))

        if stop_time is not None and alert_time + interval >= stop_time:
            self._timers[label][0].cancel()
            del self._timers[label]
            return

        cdef datetime next_alert_time = alert_time + interval
        cdef float delay = (next_alert_time - self.time_now()).total_seconds()
        timer = Timer(
            interval=delay,
            function=self._repeating_timer,
            args=[label, next_alert_time, interval, stop_time])
        timer.start()
        self._timers[label] = (timer, self._timers[label][1])


cdef class TestTimer:
    """
    Provides a fake timer for backtesting and unit testing.
    """

    def __init__(self,
                 Label label,
                 timedelta interval,
                 datetime start,
                 datetime stop,
                 handler: Callable):
        """
        Initializes a new instance of the TestTimer class.

        :param label: The label for the timer.
        :param interval: The timedelta interval for the timer.
        :param start: The start UTC datetime for the timer.
        :param stop: The stop UTC datetime for the timer.
        :param handler: The handler to call when time events are raised.
        """
        self.label = label
        self.interval = interval
        self.start = start
        self.stop = stop
        self.next_alert = start + interval
        self.handler = handler
        self.expired = False

    cpdef void advance(self, datetime time):
        """
        Advance the timer forward to the given time.
        
        :param time: The time to advance the timer to.
        """
        while time >= self.next_alert and self.expired is False:
            self.handler(TimeEvent(self.label, GUID(uuid4()), self.next_alert))
            self.next_alert += self.interval
            if self.stop is not None and self.next_alert > self.stop:
                self.expired = True


cdef class TestClock(Clock):
    """
    Provides a clock for backtesting and unit testing.
    """

    def __init__(self, datetime initial_time=UNIX_EPOCH):
        """
        Initializes a new instance of the TestClock class.

        :param initial_time: The initial time for the clock.
        """
        super().__init__()
        self._time = initial_time
        self._time_alerts = {}  # type: Dict[Label, tuple]
        self._timers = {}       # type: Dict[Label, Timer]

    cpdef datetime time_now(self):
        """
        Return the current UTC datetime of the clock.

        :return: datetime.
        """
        return self._time

    cpdef void set_time(self, datetime time):
        """
        Set the clocks UTC datetime to the given time.
        
        :param time: The time to set to.
        """
        self._time = time

    cpdef void iterate_time(self, datetime time):
        """
        Iterates the clocks time to the given time at time_step intervals.
        
        :param time: The datetime to iterate the strategies clock to.
        """
        # Preconditions commented out for performance reasons (assumes backtest implementation is correct)
        # Precondition.true(time.tzinfo == self.timezone, 'time.tzinfo == self.timezone')
        # Precondition.true(time > self.time_now(), 'time > self.time_now()')

        cdef list expired_alerts = []
        cdef list expired_timers = []
        cdef Label label

        # Iterate time alerts
        for label, alert in self._time_alerts.items():
            if time >= alert[0]:
                alert[1](TimeEvent(label, GUID(uuid4()), alert[0]))
                expired_alerts.append(label)

        # Remove expired time alerts
        for label in expired_alerts:
            del self._time_alerts[label]

        # Iterate timers
        for label, timer in self._timers.items():
            timer.advance(time)
            if timer.expired:
                expired_timers.append(label)

        # Remove expired timers
        for label in expired_timers:
            del self._timers[label]

        self._time = time

    cpdef set_time_alert(
            self,
            Label label,
            datetime alert_time,
            handler: Callable):
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, on_event() is passed the TimeEvent containing the
        alerts unique label.

        Note: The timer thread will begin immediately.

        :param label: The label for the alert (must be unique).
        :param alert_time: The datetime for the alert.
        :param handler: The handler method for the alert.
        :raises ValueError: If the label is not unique for this strategy.
        :raises ValueError: If the alert_time is not > than the clocks current time.
        """
        Precondition.true(alert_time > self.time_now(), 'alert_time > time_now()')
        Precondition.not_in(label, self._time_alerts, 'label', 'time_alerts')

        self._time_alerts[label] = (alert_time, handler)

    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time,
            datetime stop_time,
            handler: Callable):
        """
        Set a timer with the given interval (timedelta). The timer will run from
        the start time (optionally until the stop time). When the interval is
        reached and the strategy is running, the on_event() is passed the
        TimeEvent containing the timers unique label.

        Note: The timer thread will begin immediately.

        :param label: The label for the timer (must be unique).
        :param interval: The interval timedelta  for the timer.
        :param start_time: The start datetime for the timer (optional can be None - then starts immediately).
        :param stop_time: The stop datetime for the timer (optional can be None - then will run indefinitely).
        :param handler: The handler method for the alert.
        :raises ValueError: If the label is not unique.
        :raises ValueError: If the start_time is not None and not >= the current time (UTC).
        :raises ValueError: If the stop_time is not None and not > than the start_time.
        :raises ValueError: If the stop_time is not None and start_time plus interval is greater
        than the stop_time.
        """
        Precondition.not_in(label, self._timers, 'label', 'timers')

        if start_time is not None:
            Precondition.true(start_time >= self.time_now(),
                              'start_time >= self.clock.time_now()')
        if stop_time is not None:
            Precondition.true(stop_time > start_time, 'stop_time > start_time')
            Precondition.true(start_time + interval <= stop_time,
                              'start_time + interval <= stop_time')

        cdef TestTimer timer = TestTimer(
            label,
            interval,
            start_time if start_time is not None else self.time_now(),
            stop_time,
            handler)

        self._timers[label] = timer

    cpdef cancel_time_alert(self, Label label):
        """
        Cancel the time alert corresponding to the given label.

        :param label: The label for the alert to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        Precondition.is_in(label, self._time_alerts, 'label', 'time_alerts')

        del self._time_alerts[label]

    cpdef cancel_timer(self, Label label):
        """
        Cancel the timer corresponding to the given unique label.

        :param label: The label for the timer to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        Precondition.is_in(label, self._timers, 'label', 'timers')

        del self._timers[label]

    cpdef list get_labels(self):
        """
        Return the timer labels held by the clock.
        
        :return: List[Label].
        """
        return list(self._time_alerts) + list(self._timers)

    cpdef stop_all_timers(self):
        """
        Stops and clears all alerts and timers.
        """
        self._time_alerts = {}  # type: Dict[Label, tuple]
        self._timers = {}       # type: Dict[Label, Timer]
