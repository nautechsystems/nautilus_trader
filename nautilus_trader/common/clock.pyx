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
_UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)


cdef class Clock:
    """
    The base class for all clocks. All times are timezone aware UTC.
    """

    def __init__(self):
        """
        Initializes a new instance of the Clock class.
        """
        self._log = None
        self._timers = {}
        self._handlers = {}
        self._default_handler = None

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
        Register the given handler with the clock to receive all generated
        time events.
        """
        self._log = logger
        self.is_logger_registered = True

    cpdef void register_default_handler(self, handler: Callable):
        """
        Register the given handler as the clocks default handler.
        """
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

        :param label: The label for the alert (must be unique to this clock).
        :param handler: The optional handler for the time event (must be Callable or None).
        :param alert_time: The time for the alert.
        :raises ConditionFailed: If the label is not unique for this clock.
        :raises ConditionFailed: If the handler is not of type Callable or None.
        :raises ConditionFailed: If the handler is None and no default handler is registered.
        :raises ConditionFailed: If the alert_time is not >= the clocks current time.
        """
        if handler is None:
            handler = self._default_handler
        Condition.not_in(label, self._timers, 'label', 'timers')
        Condition.not_in(label, self._handlers, 'label', 'handlers')
        Condition.type(handler, Callable, 'handler')
        Condition.true(alert_time >= self.time_now(), 'alert_time >= time_now()')

        timer = self._get_timer(label=label, event_time=alert_time)
        self._add_timer(label, timer, handler)

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
        Set a timer with the given interval (timedelta). The timer will run from
        the start time (optionally until the stop time). When the intervals are
        reached the handlers will be passed the TimeEvent containing the timers 
        unique label.

        :param label: The label for the timer (must be unique to this clock).
        :param interval: The time delta interval for the timer.
        :param handler: The handler for the time events (must be Callable or None).
        :param start_time: The start time for the timer (optional can be None - then starts immediately).
        :param stop_time: The stop time for the timer (optional can be None - then repeats indefinitely).
        :raises ConditionFailed: If the label is not unique for this clock.
        :raises ConditionFailed: If the interval is not positive (> 0).
        :raises ConditionFailed: If the start_time and stop_time are not None and start_time >= stop_time.
        :raises ConditionFailed: If the start_time is not None and start_time + interval > the current time (UTC).
        :raises ConditionFailed: If the stop_time is not None and not > than the start_time (UTC).
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

        if start_time is None:
            start_time = self.time_now()
        if stop_time is not None:
            Condition.true(start_time < stop_time, 'start_time < stop_time')
            Condition.true(start_time + interval <= stop_time, 'start_time + interval <= stop_time')

        cdef datetime next_time = start_time + interval
        Condition.true(next_time >= self.time_now(), 'event_time >= time_now')

        timer = self._get_timer_repeating(
            label=label,
            interval=interval,
            next_time=next_time,
            stop_time=stop_time)
        self._add_timer(label, timer, handler)

        cdef str start_time_msg
        cdef str stop_time_msg
        if self.is_logger_registered:
            if start_time is not None:
                start_time_msg = f', starting at {start_time}'
            else:
                start_time_msg = ''
            if stop_time is not None:
                stop_time_msg = f', stopping at {stop_time}'
            else:
                stop_time_msg = ''
            self._log.info(f"Set Timer('{label.value}') with interval {interval}{start_time_msg}{stop_time_msg}.")

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
                self._log.info(f"Cancelled Timer('{label.value}').")

        self._handlers.pop(label, None)

    cpdef void cancel_all_timers(self) except *:
        """
        Cancel all timers inside the clock.
        """
        cdef Label label
        for label in self.get_timer_labels():
            self.cancel_timer(label)

    cdef object _get_timer(self, Label label, datetime event_time):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef object _get_timer_repeating(
            self,
            Label label,
            timedelta interval,
            datetime next_time,
            datetime stop_time):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _add_timer(self, Label label, timer, handler) except *:
        self._timers[label] = timer
        self._handlers[label] = handler

    cdef void _remove_timer(self, Label label) except *:
        self._timers.pop(label, None)
        self._handlers.pop(label, None)


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
            args=[label, event_time])
        timer.daemon = True
        timer.start()

        return timer

    cdef object _get_timer_repeating(
            self,
            Label label,
            timedelta interval,
            datetime next_time,
            datetime stop_time):
        cdef float delay = (next_time - self.time_now()).total_seconds()
        timer = Timer(
            interval=delay,
            function=self._raise_time_event_repeating,
            args=[label, interval, next_time, stop_time])
        timer.daemon = True
        timer.start()

        return timer

    cpdef void _raise_time_event(self, Label label, datetime event_time) except *:
        cdef TimeEvent event = TimeEvent(label, GUID(uuid.uuid4()), event_time)
        self._handle_time_event(event)
        self._remove_timer(label)

    cpdef void _raise_time_event_repeating(
            self,
            Label label,
            timedelta interval,
            datetime next_time,
            datetime stop_time) except *:
        cdef TimeEvent event = TimeEvent(label, GUID(uuid.uuid4()), next_time)
        self._handle_time_event(event)

        if stop_time is not None and next_time + interval > stop_time:
            self._remove_timer(label)  # Timer now expired
            return

        # Continue timing
        timer = self._get_timer_repeating(
            label=label,
            next_time=next_time + interval,
            interval=interval,
            stop_time=stop_time)

        self._timers[label] = timer

    cdef void _handle_time_event(self, TimeEvent event) except *:
        handler = self._handlers.get(event.label)
        if handler:
            handler(event)


cdef class TestTimer:
    """
    Provides a fake timer for backtesting and unit testing.
    """

    def __init__(self,
                 Label label,
                 timedelta interval,
                 datetime next_time,
                 datetime stop_time=None):
        """
        Initializes a new instance of the TestTimer class.
        :param label: The label for the timer.
        :param interval: The timedelta interval for the timer.
        :param next_time: The start UTC datetime for the timer.
        :param stop_time: The stop UTC datetime for the timer.
        """
        # Condition: assumes interval not negative.
        self.label = label
        self.interval = interval
        self.next_time = next_time
        self.stop_time = stop_time
        self.expired = False

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
            print(self.next_time)
            time_events.append(TimeEvent(self.label, GUID(uuid.uuid4()), self.next_time))
            self.next_time += self.interval
            if self.stop_time is not None and self.next_time > self.stop_time:
                self.expired = True

        return time_events

    cpdef void cancel(self):
        """
        Cancels the timer (the timer will not generate an event).
        """
        self.expired = True


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

    cpdef dict advance_time(self, datetime to_time):
        """
        Iterates the clocks time to the given datetime.
        
        :param to_time: The datetime to iterate the test clock to.
        :return Dict[TimeEvent].
        """
        # Condition: assumes time.tzinfo == self.timezone
        # Condition: assumes to_time > self.time_now()
        cdef dict events = {}  # type: Dict[TimeEvent, Callable]

        # Iterate timers
        cdef Label label
        cdef TestTimer timer
        cdef TimeEvent event
        for label, timer in self._timers.copy().items():
            for event in timer.advance(to_time):
                handler = self._handlers.get(label)
                if handler:
                    events[event] = handler
            if timer.expired:
                self._remove_timer(label)  # Removes expired timer

        self._time = to_time

        return dict(sorted(events.items()))

    cdef object _get_timer(self, Label label, datetime event_time):
        return TestTimer(
            label=label,
            interval=event_time - self.time_now(),
            next_time=event_time,
            stop_time=event_time)

    cdef object _get_timer_repeating(
            self,
            Label label,
            timedelta interval,
            datetime next_time,
            datetime stop_time):
        return TestTimer(
            label=label,
            interval=interval,
            next_time=next_time,
            stop_time=stop_time)
