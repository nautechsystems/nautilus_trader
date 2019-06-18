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
from typing import List, Dict, Callable

from inv_trader.common.clock cimport TestTimer
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.core.precondition cimport Precondition
from inv_trader.model.identifiers cimport Label, GUID
from inv_trader.model.events cimport TimeEvent

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)


cdef class Clock:
    """
    The abstract base class for all clocks. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the Clock class.
        """
        self._log = None
        self._event_handler = None
        self.is_logger_registered = False
        self.is_handler_registered = False

    cpdef void register_logger(self, LoggerAdapter logger):
        """
        Register the given handler with the clock to receive all generated
        time events.
        """
        self._log = logger
        self.is_logger_registered = True

    cpdef void register_handler(self, handler: Callable):
        """
        Register the given handler with the clock to receive all generated
        time events.
        """
        self._event_handler = handler
        self.is_handler_registered = True

    cpdef datetime time_now(self):
        """
        Return the current UTC datetime of the clock.
        
        :return: datetime.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef timedelta get_delta(self, datetime time):
        """
        Return the timedelta from the given time.
        
        :return: timedelta.
        """
        return self.time_now() - time

    cpdef set_time_alert(self, Label label, datetime alert_time):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef set_timer(self, Label label, timedelta interval, datetime start_time=None, datetime stop_time=None):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef list get_time_alert_labels(self):
        """
        Return the time alert labels held by the clock.
        
        :return: List[Label].
        """
        return list(self._time_alerts.keys())

    cpdef list get_timer_labels(self):
        """
        Return the timer labels held by the clock.
        
        :return: List[Label].
        """
        return list(self._timers.keys())

    cpdef cancel_time_alert(self, Label label):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef cancel_timer(self, Label label):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef cancel_all_time_alerts(self):
        """
        Cancel all time alerts inside the clock.
        """
        for label in self._time_alerts.copy().keys():  # Copy to avoid resize during iteration
            self.cancel_time_alert(label)

    cpdef cancel_all_timers(self):
        """
        Cancel all timers inside the clock.
        """
        for label in self._timers.copy().keys():  # Copy to avoid resize during iteration
            self.cancel_timer(label)


cdef class LiveClock(Clock):
    """
    Provides a clock for live trading. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the LiveClock class.
        """
        super().__init__()
        self._time_alerts = {}  # type: Dict[Label, Timer]
        self._timers = {}       # type: Dict[Label, Timer]

    cpdef datetime time_now(self):
        """
        Return the current UTC datetime of the clock.
        
        :return: datetime.
        """
        return datetime.now(timezone.utc)

    cpdef set_time_alert(self, Label label, datetime alert_time):
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, the clocks handler is passed the TimeEvent containing the
        alerts unique label.

        Note: The timer thread will begin immediately.

        :param label: The label for the alert (must be unique).
        :param alert_time: The time for the alert.
        :raises ValueError: If the label is not unique for this clock.
        :raises ValueError: If the alert_time is not > than the clocks current time.
        """
        Precondition.true(alert_time > self.time_now(), 'alert_time > time_now()')
        Precondition.not_in(label, self._time_alerts, 'label', 'time_alerts')

        timer = Timer(
            interval=(alert_time - self.time_now()).total_seconds(),
            function=self._raise_time_event,
            args=[label, alert_time])

        timer.start()
        self._time_alerts[label] = timer

        if self.is_logger_registered:
            self._log.info(f"Set TimeAlert('{label.value}') for {alert_time}")

    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time=None,
            datetime stop_time=None):
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
        :raises ValueError: If the label is not unique.
        :raises ValueError: If the handler is not of type Callable.
        :raises ValueError: If the start_time is not None and not >= the current time (UTC).
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
        self._timers[label] = timer

        cdef str start_time_msg = ''
        cdef str stop_time_msg = ''

        if self.is_logger_registered:
            if start_time is not None:
                start_time_msg = f', starting at {start_time}'
            if stop_time is not None:
                stop_time_msg = f', stopping at {stop_time}'
            self._log.info(f"Set Timer('{label.value}') with interval {interval}{start_time_msg}{stop_time_msg}.")

    cpdef cancel_time_alert(self, Label label):
        """
        Cancel the time alert corresponding to the given label.

        :param label: The label for the alert to cancel.
        :raises ValueError: If the label is not found in the internal time alerts.
        """
        Precondition.is_in(label, self._time_alerts, 'label', 'timers')

        self._time_alerts[label].cancel()
        del self._time_alerts[label]

        if self.is_logger_registered:
            self._log.info(f"Cancelled TimeAlert('{label.value}').")

    cpdef cancel_timer(self, Label label):
        """
        Cancel the timer corresponding to the given unique label.

        :param label: The label for the timer to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        Precondition.is_in(label, self._timers, 'label', 'timers')

        self._timers[label].cancel()
        del self._timers[label]

        if self.is_logger_registered:
            self._log.info(f"Cancelled Timer('{label.value}').")

    cpdef void _raise_time_event(self, Label label, datetime alert_time):
        """
        Create a new TimeEvent and pass it to the clocks event handler.
        """
        self._event_handler(TimeEvent(label, GUID(uuid4()), alert_time))

        if label in self._timers:
            del self._timers[label]

    cpdef void _repeating_timer(
            self,
            Label label,
            datetime alert_time,
            timedelta interval,
            datetime stop_time):
        """
        Create a new TimeEvent and pass it to the clocks event handler.
        Then start a timer for the next time event if applicable.
        """
        self._event_handler(TimeEvent(label, GUID(uuid4()), alert_time))

        if stop_time is not None and alert_time + interval > stop_time:
            self._timers[label].cancel()
            del self._timers[label]
            return

        cdef datetime next_alert_time = alert_time + interval
        cdef float delay = (next_alert_time - self.time_now()).total_seconds()
        timer = Timer(
            interval=delay,
            function=self._repeating_timer,
            args=[label, next_alert_time, interval, stop_time])
        timer.start()
        self._timers[label] = timer


cdef class TestTimer:
    """
    Provides a fake timer for backtesting and unit testing.
    """

    def __init__(self,
                 Label label,
                 timedelta interval,
                 datetime start,
                 datetime stop=None):
        """
        Initializes a new instance of the TestTimer class.

        :param label: The label for the timer.
        :param interval: The timedelta interval for the timer.
        :param start: The start UTC datetime for the timer.
        :param stop: The stop UTC datetime for the timer.
        """
        self.label = label
        self.interval = interval
        self.start = start
        self.stop = stop
        self.next_alert = start + interval
        self.expired = False

    cpdef list advance(self, datetime time):
        """
        Return a list of time events in chronological order by advancing the 
        test timer forward to the given time.

        :param time: The time to advance the test timer to.
        :return: List[TimeEvent].
        """
        cdef list time_events = []  # type: List[TimeEvent]

        while time >= self.next_alert and self.expired is False:
            time_events.append(TimeEvent(self.label, GUID(uuid4()), self.next_alert))
            self.next_alert += self.interval
            if self.stop is not None and self.next_alert > self.stop:
                self.expired = True

        return time_events


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
        self._time_alerts = {}  # type: Dict[Label, datetime]
        self._timers = {}       # type: Dict[Label, TestTimer]

    cpdef void set_time(self, datetime time):
        """
        Set the clocks UTC datetime to the given time.
        
        :param time: The time to set to.
        """
        self._time = time

    cpdef datetime time_now(self):
        """
        Return the current UTC datetime of the clock.

        :return: datetime.
        """
        return self._time

    cpdef dict iterate_time(self, datetime to_time):
        """
        Iterates the clocks time to the given datetime.
        
        :param to_time: The datetime to iterate the test clock to.
        :return: List[TimeEvent].
        """
        # Preconditions commented out for performance reasons (assumes backtest implementation is correct)
        # Precondition.true(time.tzinfo == self.timezone, 'time.tzinfo == self.timezone')
        # Precondition.true(time > self.time_now(), 'time > self.time_now()')

        cdef dict time_events = {}  # type: Dict[TimeEvent, Callable]
        cdef Label label

        # Iterate time alerts
        cdef datetime alert_time
        for label, alert_time in self._time_alerts.copy().items():
            if to_time >= alert_time:
                time_events[TimeEvent(label, GUID(uuid4()), alert_time)] = self._event_handler
                del self._time_alerts[label]  # Remove triggered time alert

        # Iterate timers
        cdef TestTimer timer
        for label, timer in self._timers.copy().items():
            for timer_event in timer.advance(to_time):
                time_events[timer_event] = self._event_handler
            if timer.expired:
                del self._timers[label]  # Remove expired timer

        # Set the clock time to the given to_time
        self._time = to_time

        return dict(sorted(time_events.items()))

    cpdef set_time_alert(self, Label label, datetime alert_time):
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, on_event() is passed the TimeEvent containing the
        alerts unique label.

        Note: The timer thread will begin immediately.

        :param label: The label for the alert (must be unique).
        :param alert_time: The datetime for the alert.
        :raises ValueError: If the label is not unique for this strategy.
        :raises ValueError: If the alert_time is not > than the clocks current time.
        """
        Precondition.true(alert_time > self.time_now(), 'alert_time > time_now()')
        Precondition.not_in(label, self._time_alerts, 'label', 'time_alerts')

        self._time_alerts[label] = alert_time

        if self.is_logger_registered:
            self._log.info(f"Set TimeAlert('{label.value}') for {alert_time}")

    cpdef set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time=None,
            datetime stop_time=None):
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
        else:
            start_time = self.time_now()
        if stop_time is not None:
            Precondition.true(stop_time > start_time, 'stop_time > start_time')
            Precondition.true(start_time + interval <= stop_time,
                              'start_time + interval <= stop_time')

        cdef TestTimer timer = TestTimer(
            label,
            interval,
            start_time,
            stop_time)

        self._timers[label] = timer

        cdef str start_time_msg = ''
        cdef str stop_time_msg = ''

        if self.is_logger_registered:
            if start_time is not None:
                start_time_msg = f', starting at {start_time}'
            if stop_time is not None:
                stop_time_msg = f', stopping at {stop_time}'
            self._log.info(f"Set Timer('{label.value}') with interval {interval}{start_time_msg}{stop_time_msg}.")

    cpdef cancel_time_alert(self, Label label):
        """
        Cancel the time alert corresponding to the given label.

        :param label: The label for the alert to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        Precondition.is_in(label, self._time_alerts, 'label', 'time_alerts')

        del self._time_alerts[label]

        if self.is_logger_registered:
            self._log.info(f"Cancelled TimeAlert('{label.value}').")

    cpdef cancel_timer(self, Label label):
        """
        Cancel the timer corresponding to the given unique label.

        :param label: The label for the timer to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        Precondition.is_in(label, self._timers, 'label', 'timers')

        del self._timers[label]

        if self.is_logger_registered:
            self._log.info(f"Cancelled Timer('{label.value}').")
