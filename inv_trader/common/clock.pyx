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
from datetime import timezone
from cpython.datetime cimport datetime, timedelta
from threading import Timer
from typing import Dict, Callable

from inv_trader.common.clock cimport TestTimer
from inv_trader.core.precondition cimport Precondition
from inv_trader.model.identifiers cimport Label, GUID
from inv_trader.model.events cimport TimeEvent

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
cdef datetime UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)
cdef int MILLISECONDS_PER_SECOND = 1000


cdef class Clock:
    """
    The abstract base class for all clocks. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the Clock class.
        """
        self.timezone = timezone.utc
        self._unix_epoch = UNIX_EPOCH

    cpdef datetime time_now(self):
        """
        :return: The current time of the clock.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef datetime unix_epoch(self):
        """
        Unix time (also known as POSIX time or epoch time) is a system for
        describing instants in time, defined as the number of seconds that have
        elapsed since 00:00:00 Coordinated Universal Time (UTC), on Thursday,
        1 January 1970, minus the number of leap seconds which have taken place
        since then.
        
        :return: The time at the unix epoch (00:00:00 on 1/1/1970 UTC).
        """
        return self._unix_epoch

    cpdef float get_elapsed(self, datetime start):
        """
        :return: The number of seconds elapsed since the given start time rounded
        to two decimal places. 
        """
        Precondition.true(start.tzinfo == self.timezone, 'time.tzinfo == self.timezone')
        Precondition.true(start <= self.time_now(), 'start >= self.time_now()')

        return (self.time_now() - start).total_seconds()

    cdef str get_datetime_tag(self):
        """
        :return: The datetime tag string for the current time. 
        """
        cdef datetime time_now = self.time_now()
        return (f'{time_now.year}'
                f'{time_now.month:02d}'
                f'{time_now.day:02d}'
                f'-'
                f'{time_now.hour:02d}'
                f'{time_now.minute:02d}'
                f'{time_now.second:02d}')

    # cdef long milliseconds_since_unix_epoch(self):
    #     """
    #     :return:  Returns the number of ticks of the given time now since the Unix Epoch.
    #     """
    #     return (self.time_now() - self._unix_epoch).total_seconds() * MILLISECONDS_PER_SECOND

    cpdef set_time_alert(
            self,
            Label label,
            datetime alert_time,
            handler: Callable):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef cancel_time_alert(self, Label label):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time,
            datetime stop_time,
            bint repeat,
            handler: Callable):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef cancel_timer(self, Label label):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef list get_labels(self):
        """
        :return: The timer labels held by the clock.
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
    Implements a clock for live trading.
    """

    def __init__(self):
        """
        Initializes a new instance of the LiveClock class.
        """
        super().__init__()
        self._timers = {}   # type: Dict[Label, (Timer, Callable)]

    cpdef datetime time_now(self):
        """
        :return: The current time of the clock.
        """
        return datetime.now(self.timezone)

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
        :raises ValueError: If the label is not unique for this strategy.
        :raises ValueError: If the alert_time is not > than the clocks current time.
        """
        Precondition.true(alert_time > self.time_now(), 'self.time_now()')
        Precondition.not_in(label, self._timers, 'label', 'timers')

        timer = Timer(
            interval=(alert_time - self.time_now()).total_seconds(),
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

    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time,
            datetime stop_time,
            bint repeat,
            handler: Callable):
        """
        Set a timer with the given interval (time delta). The timer will run from
        the start time (optionally until the stop time). When the interval is
        reached and the strategy is running, the on_event() is passed the
        TimeEvent containing the timers unique label.

        Optionally the timer can be run repeatedly whilst the strategy is running.

        Note: The timer thread will begin immediately.

        :param label: The label for the timer (must be unique).
        :param interval: The time delta interval for the timer.
        :param start_time: The start time for the timer (can be None, then starts immediately).
        :param stop_time: The stop time for the timer (can be None).
        :param repeat: The option for the timer to repeat until the strategy is stopped.
        :param handler: The handler method for the alert.
        :raises ValueError: If the label is not unique.
        :raises ValueError: If the start_time is not None and not >= the current time (UTC).
        :raises ValueError: If the stop_time is not None and repeat is False.
        :raises ValueError: If the stop_time is not None and not > than the start_time.
        :raises ValueError: If the stop_time is not None and start_time plus interval is greater
        than the stop_time.
        """
        Precondition.not_in(label, self._timers, 'label', 'timers')

        if start_time is not None:
            Precondition.true(start_time >= self.time_now(),
                              'start_time >= self.clock.time_now()')
        else:
            start_time = self.time_now()
        if stop_time is not None:
            Precondition.true(repeat, 'repeat True')
            Precondition.true(stop_time > start_time, 'stop_time > start_time')
            Precondition.true(start_time + interval <= stop_time,
                              'start_time + interval <= stop_time')

        if label in self._timers:
            raise KeyError(
                f"Cannot set timer (the label {label} was not unique for this strategy).")

        cdef datetime alert_time = start_time + interval
        cdef float delay = (alert_time - self.time_now()).total_seconds()
        if repeat:
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
        :return: The timer labels held by the clock.
        """
        return list(self._timers)

    cpdef stop_all_timers(self):
        """
        Stop all alerts and timers inside the clock.
        """
        for label, timer in self._timers.items():
            timer[0].cancel()

    cpdef void _raise_time_event(
            self,
            Label label,
            datetime alert_time):
        """
        Create a new TimeEvent and pass it to the registered handler.
        """
        self._timers[label][1](TimeEvent(label, GUID(uuid4()), alert_time))
        del self._timers[label]

    # Cannot convert below method to Python callable if cdef
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
    Implements a fake timer for backtesting and unit testing.
    """

    def __init__(self,
                 Label label,
                 timedelta interval,
                 datetime start,
                 datetime stop,
                 bint repeating,
                 handler: Callable):
        """
        Initializes a new instance of the TestTimer class.

        :param label: The label for the timer.
        """
        self.label = label
        self.start = start
        self.stop = stop
        self.interval = interval
        self.next_alert = start + interval
        self.repeating = repeating
        self.handler = handler
        self.expired = False

    cpdef void advance(self, datetime time):
        """
        Wind the timer forward.
        
        :param time: The time to wind the timer to.
        """
        while time >= self.next_alert and self.expired is False:
            self.handler(TimeEvent(self.label, GUID(uuid4()), self.next_alert))
            self.next_alert += self.interval
            if not self.repeating or self.next_alert > self.stop:
                self.expired = True


cdef class TestClock(Clock):
    """
    Implements a clock for backtesting and unit testing.
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
        :return: The current time of the clock.
        """
        return self._time

    cpdef void set_time(self, datetime time):
        """
        Set the clocks time to the given time.
        
        :param time: The time to set to.
        """
        self._time = time

    cpdef void iterate_time(self, datetime time):
        """
        Iterates the clocks time to the given time at time_step intervals.
        
        :raises ValueError: If the given times timezone is not UTC.
        :raises ValueError: If the given time is <= the clocks internal time.
        """
        Precondition.true(time.tzinfo == self.timezone, 'time.tzinfo == self.timezone')

        cdef list expired_alerts = []
        cdef list expired_timers = []

        # Time alerts
        for label, alert in self._time_alerts.items():
            if time >= alert[0]:
                alert[1](TimeEvent(label, GUID(uuid4()), alert[0]))
                expired_alerts.append(label)

        # Remove expired time alerts
        for label in expired_alerts:
            del self._time_alerts[label]

        # Timers
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
        :param alert_time: The time for the alert.
        :param handler: The handler method for the alert.
        :raises ValueError: If the label is not unique for this strategy.
        :raises ValueError: If the alert_time is not > than the clocks current time.
        """
        Precondition.true(alert_time > self.time_now(), 'self.time_now()')
        Precondition.not_in(label, self._time_alerts, 'label', 'time_alerts')

        self._time_alerts[label] = (alert_time, handler)

    cpdef cancel_time_alert(self, Label label):
        """
        Cancel the time alert corresponding to the given label.

        :param label: The label for the alert to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        Precondition.is_in(label, self._time_alerts, 'label', 'time_alerts')

        del self._time_alerts[label]

    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time,
            datetime stop_time,
            bint repeat,
            handler: Callable):
        """
        Set a timer with the given interval (time delta). The timer will run from
        the start time (optionally until the stop time). When the interval is
        reached and the strategy is running, the on_event() is passed the
        TimeEvent containing the timers unique label.

        Optionally the timer can be run repeatedly whilst the strategy is running.

        Note: The timer thread will begin immediately.

        :param label: The label for the timer (must be unique).
        :param interval: The time delta interval for the timer.
        :param start_time: The start time for the timer (can be None, then starts immediately).
        :param stop_time: The stop time for the timer (can be None).
        :param repeat: The option for the timer to repeat until the strategy is stopped.
        :param handler: The handler method for the alert.
        :raises ValueError: If the label is not unique.
        :raises ValueError: If the start_time is not None and not >= the current time (UTC).
        :raises ValueError: If the stop_time is not None and repeat is False.
        :raises ValueError: If the stop_time is not None and not > than the start_time.
        :raises ValueError: If the stop_time is not None and start_time plus interval is greater
        than the stop_time.
        """
        Precondition.not_in(label, self._timers, 'label', 'timers')

        if start_time is not None:
            Precondition.true(start_time >= self.time_now(),
                              'start_time >= self.clock.time_now()')
        else:
            start_time = self.time_now()
        if stop_time is not None:
            Precondition.true(repeat, 'repeat True')
            Precondition.true(stop_time > start_time, 'stop_time > start_time')
            Precondition.true(start_time + interval <= stop_time,
                              'start_time + interval <= stop_time')

        if label in self._timers:
            raise KeyError(
                f"Cannot set timer (the label {label} was not unique for this strategy).")

        cdef TestTimer timer = TestTimer(label,
                                         interval,
                                         start_time,
                                         stop_time,
                                         repeat,
                                         handler)

        self._timers[label] = timer

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
        :return: The timer labels held by the clock.
        """
        return list(self._time_alerts) + list(self._timers)

    cpdef stop_all_timers(self):
        """
        Clears all alerts and timers.
        """
        self._time_alerts = {}  # type: Dict[Label, tuple]
        self._timers = {}       # type: Dict[Label, Timer]
