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
from threading import Timer
from typing import List, Dict, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.clock cimport TestTimer
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.identifiers cimport Label
from nautilus_trader.model.events cimport TimeEvent

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)


cdef class Clock:
    """
    The base class for all clocks. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the Clock class.
        """
        self._log = None
        self._event_handler = None
        self._timers = {}
        self._event_times = {}
        self.event_times = []
        self.next_event_time = None
        self.has_event_times = False
        self.is_logger_registered = False
        self.is_handler_registered = False

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

    cpdef void set_time_alert(self, Label label, datetime alert_time) except *:
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, the clocks handler is passed the TimeEvent containing the
        alerts unique label.

        Note: For LiveClock the timer thread will begin immediately.

        :param label: The label for the alert (must be unique).
        :param alert_time: The time for the alert.
        :raises ConditionFailed: If the label is not unique for this clock.
        :raises ConditionFailed: If the alert_time is not > than the clocks current time.
        """
        Condition.not_in(label, self._timers, 'label', 'timers')
        Condition.true(alert_time > self.time_now(), 'alert_time > time_now()')

        timer = self._get_timer(label=label, event_time=alert_time)
        self._add_timer(label, timer, alert_time)

        if self.is_logger_registered:
            self._log.info(f"Set Timer('{label.value}') with alert for {alert_time}.")

    cpdef void set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time=None,
            datetime stop_time=None) except *:
        """
        Set a timer with the given interval (timedelta). The timer will run from
        the start time (optionally until the stop time). When the interval is
        reached and the strategy is running, the on_event() is passed the
        TimeEvent containing the timers unique label.

        Note: For LiveClock the timer thread will begin immediately.

        :param label: The label for the timer (must be unique).
        :param interval: The time delta interval for the timer.
        :param start_time: The start time for the timer (optional can be None - then starts immediately).
        :param stop_time: The stop time for the timer (optional can be None - then repeats indefinitely).
        :raises ConditionFailed: If the label is not unique.
        :raises ConditionFailed: If the interval is not positive (> 0).
        :raises ConditionFailed: If the start_time is not None and not >= the current time (UTC).
        :raises ConditionFailed: If the stop_time is not None and not > than the start_time (UTC).
        :raises ConditionFailed: If the stop_time is not None and start_time plus interval is greater
        than the stop_time.
        """
        Condition.not_in(label, self._timers, 'label', 'timers')
        Condition.true(interval.total_seconds() > 0, 'interval > 0')

        if start_time is not None:
            Condition.true(start_time >= self.time_now(), 'start_time >= time_now()')
        else:
            start_time = self.time_now()
        if stop_time is not None:
            Condition.true(stop_time > start_time, 'stop_time > start_time')
            Condition.true(start_time + interval <= stop_time, 'start_time + interval <= stop_time')

        cdef datetime event_time = start_time + interval

        timer = self._get_timer_repeating(
            label=label,
            next_event_time=event_time,
            interval=interval,
            stop_time=stop_time)

        self._add_timer(label, timer, event_time)

        cdef str start_time_msg = ''
        cdef str stop_time_msg = ''

        if self.is_logger_registered:
            if start_time is not None:
                start_time_msg = f', starting at {start_time}'
            if stop_time is not None:
                stop_time_msg = f', stopping at {stop_time}'
            self._log.info(f"Set Timer('{label.value}') with interval {interval}{start_time_msg}{stop_time_msg}.")

    cpdef void cancel_timer(self, Label label) except *:
        """
        Cancel the timer corresponding to the given unique label.

        :param label: The label for the timer to cancel.
        :raises ConditionFailed: If the label is not found in the internal timers.
        """
        Condition.is_in(label, self._timers, 'label', 'timers')

        self._timers[label].cancel()
        self._timers.pop(label, None)
        self._event_times.pop(label, None)
        self._sort_event_times()

        if self.is_logger_registered:
            self._log.info(f"Cancelled Timer('{label.value}').")

    cpdef void cancel_all_timers(self) except *:
        """
        Cancel all timers inside the clock.
        """
        for label in self._timers.copy().keys():  # Copy to avoid resize during iteration
            self.cancel_timer(label)

    cpdef void _raise_time_event(self, Label label, datetime event_time) except *:
        # Create a new TimeEvent and pass it to the clocks event handler
        self._event_handler(TimeEvent(label, GUID(uuid.uuid4()), event_time))

        if label in self._timers:
            self.cancel_timer(label)

    cpdef void _raise_time_event_repeating(
            self,
            Label label,
            datetime event_time,
            timedelta interval,
            datetime stop_time) except *:
        # Create a new TimeEvent and pass it to the clocks event handler
        # Then start a timer for the next time event if applicable
        self._event_handler(TimeEvent(label, GUID(uuid.uuid4()), event_time))

        if stop_time is not None and event_time + interval > stop_time and label in self._timers:
            self.cancel_timer(label)
            return

        timer = self._get_timer_repeating(
            label=label,
            next_event_time=event_time + interval,
            interval=interval,
            stop_time=stop_time)

        self._add_timer(label, timer, event_time)

    cdef object _get_timer(self, Label label, datetime event_time):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef object _get_timer_repeating(
            self,
            Label label,
            datetime next_event_time,
            timedelta interval,
            datetime stop_time):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _add_timer(self, Label label, timer, datetime event_time):
        self._timers[label] = timer
        self._event_times[label] = event_time
        self._sort_event_times()

    cdef void _sort_event_times(self):
        self.event_times = sorted(set(self._event_times.values()))
        if self.event_times:
            self.next_event_time = self.event_times[0]
            self.has_event_times = True
        else:
            self.next_event_time = None
            self.has_event_times = False


cdef class LiveClock(Clock):
    """
    Provides a clock for live trading. All times are timezone aware UTC.
    """

    cpdef datetime time_now(self):
        """
        Return the current UTC datetime of the clock.
        
        :return datetime.
        """
        return datetime.now(timezone.utc)

    cdef object _get_timer(self, Label label, datetime event_time):
        cdef float delay = (event_time - self.time_now()).total_seconds()
        timer = Timer(
            interval=delay,
            function=self._raise_time_event,
            args=[self, label, event_time])
        timer.daemon = True
        timer.start()

        return timer

    cdef object _get_timer_repeating(
            self,
            Label label,
            datetime next_event_time,
            timedelta interval,
            datetime stop_time):
        cdef float delay = (next_event_time - self.time_now()).total_seconds()
        timer = Timer(
            interval=delay,
            function=self._raise_time_event_repeating,
            args=[self, label, next_event_time, interval, stop_time])
        timer.daemon = True
        timer.start()

        return timer


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
        :return List[TimeEvent].
        """
        cdef list time_events = []  # type: List[TimeEvent]

        while time >= self.next_alert and self.expired is False:
            time_events.append(TimeEvent(self.label, GUID(uuid.uuid4()), self.next_alert))
            self.next_alert += self.interval
            if self.stop is not None and self.next_alert > self.stop:
                self.expired = True

        return time_events

    cpdef void cancel(self):
        pass  # No thread to cancel


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

    cpdef datetime time_now(self):
        """
        Return the current datetime of the clock (UTC).

        :return datetime.
        """
        return self._time

    cpdef void set_time(self, datetime time):
        """
        Set the clocks datetime to the given time (UTC).
        
        :param time: The time to set to.
        """
        self._time = time

    cpdef dict iterate_time(self, datetime to_time):
        """
        Iterates the clocks time to the given datetime.
        
        :param to_time: The datetime to iterate the test clock to.
        :return List[TimeEvent].
        """
        # Assumes time.tzinfo == self.timezone
        # Assumes to_time > self.time_now()

        cdef dict time_events = {}  # type: Dict[TimeEvent, Callable]

        # Iterate timers
        cdef Label label
        cdef TestTimer timer
        for label, timer in self._timers.copy().items():
            for timer_event in timer.advance(to_time):
                time_events[timer_event] = self._event_handler
            if timer.expired:
                self._timers.pop(label, None)  # Remove expired timer

        # Set the clock time to the given to_time
        self._time = to_time

        return dict(sorted(time_events.items()))

    cdef object _get_timer(self, Label label, datetime event_time):
        pass

    cdef object _get_timer_repeating(self, Label label, datetime next_event_time, timedelta interval, datetime stop_time):
        pass
