# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import pytz
import numpy as np
from cpython.datetime cimport datetime, timedelta

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.common.clock cimport TestTimer
from nautilus_trader.common.guid cimport TestGuidFactory
from nautilus_trader.common.logging cimport LoggerAdapter

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
_UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, pytz.utc)


cdef class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """

    def __init__(self,
                 Label label not None,
                 GUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initializes a new instance of the TimeEvent class.

        :param event_id: The event label.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        super().__init__(event_id, event_timestamp)
        self.label = label

    def __eq__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp == other.timestamp

    def __ne__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp != other.timestamp

    def __lt__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp < other.timestamp

    def __le__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp <= other.timestamp

    def __gt__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp > other.timestamp

    def __ge__(self, TimeEvent other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp >= other.timestamp

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"label={self.label}, "
                f"timestamp={format_iso8601(self.timestamp)})")

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class TimeEventHandler:
    """
    Represents a bundled event and handler.
    """

    def __init__(self, TimeEvent event not None, handler not None):
        self.event = event
        self.handler = handler

    cdef void handle(self) except *:
        self.handler(self.event)

    def __eq__(self, TimeEventHandler other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.event.timestamp == other.event.timestamp

    def __ne__(self, TimeEventHandler other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.event.timestamp != other.event.timestamp

    def __lt__(self, TimeEventHandler other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.event.timestamp < other.event.timestamp

    def __le__(self, TimeEventHandler other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.event.timestamp <= other.event.timestamp

    def __gt__(self, TimeEventHandler other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.event.timestamp > other.event.timestamp

    def __ge__(self, TimeEventHandler other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """


cdef class Timer:
    """
    The base class for all timers.
    """

    def __init__(self,
                 Label label not None,
                 callback not None,
                 timedelta interval not None,
                 datetime start_time not None,
                 datetime stop_time=None):
        """
        Initializes a new instance of the Timer class.

        :param label: The label for the timer.
        :param callback: The function to call at the next time.
        :param interval: The time interval for the timer (not negative).
        :param start_time: The start datetime for the timer (UTC).
        :param stop_time: The optional stop datetime for the timer (UTC) (if None then timer repeats).
        """
        Condition.callable(callback, 'function')
        Condition.positive(interval.total_seconds(), 'interval')
        if stop_time:
            Condition.true(start_time + interval <= stop_time, 'start_time + interval <= stop_time')

        self.label = label
        self.callback = callback
        self.interval = interval
        self.start_time = start_time
        self.next_time = start_time + interval
        self.stop_time = stop_time
        self.expired = False

    cpdef TimeEvent pop_event(self, GUID event_id):
        """
        Returns a generated time event with the given identifier.

        :param event_id: The identifier for the time event.
        """
        Condition.not_none(event_id, 'event_id')

        return TimeEvent(self.label, event_id, self.next_time)

    cpdef void iterate_next_time(self, datetime now) except *:
        """
        Iterates the timers next time and checks if the timer is now expired.

        :param now: The datetime now (UTC).
        """
        Condition.not_none(now, 'now')

        self.next_time += self.interval
        if self.stop_time and now >= self.stop_time:
            self.expired = True

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not raise an event).
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.label.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"Timer("
                f"label={self.label}, "
                f"interval={self.interval}, "
                f"start_time={self.start_time}, "
                f"next_time={self.next_time}, "
                f"stop_time={self.stop_time})")

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__str__} object at {id(self)}>"


cdef class TestTimer(Timer):
    """
    Provides a fake timer for backtesting and unit testing.
    """
    __test__ = False

    def __init__(self,
                 Label label not None,
                 callback not None,
                 timedelta interval not None,
                 datetime start_time not None,
                 datetime stop_time=None):
        """
        Initializes a new instance of the TestTimer class.

        :param label: The label for the timer.
        :param interval: The time interval for the timer (not negative).
        :param start_time: The stop datetime for the timer (UTC).
        :param stop_time: The optional stop datetime for the timer (UTC) (if None then timer repeats).
        """
        super().__init__(label, callback, interval, start_time, stop_time)

        self._guid_factory = TestGuidFactory()

    cpdef list advance(self, datetime to_time):
        """
        Return a list of time events by advancing the test timer forward to
        the given time. A time event is appended for each time a next event is
        <= the given to_time.

        :param to_time: The time to advance the test timer to.
        :return List[TimeEvent].
        """
        Condition.not_none(to_time, 'to_time')

        cdef list time_events = []  # type: [TimeEvent]
        while not self.expired and to_time >= self.next_time:
            time_events.append(self.pop_event(self._guid_factory.generate()))
            self.iterate_next_time(self.next_time)

        return time_events

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self.expired = True


cdef class Clock:
    """
    The base class for all clocks. All times are timezone aware UTC.
    """

    def __init__(self, GuidFactory guid_factory not None):
        """
        Initializes a new instance of the Clock class.

        :param guid_factory: The guid factory for producing time events.
        """
        self._log = None
        self._guid_factory = guid_factory
        self._timers = {}    # type: {Label, Timer}
        self._handlers = {}  # type: {Label, callable}
        self._stack = None
        self._default_handler = None

        self.timer_count = 0
        self.next_event_time = None
        self.next_event_label = None
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
        Condition.not_none(time, 'time')

        return self.time_now() - time

    cpdef list get_timer_labels(self):
        """
        Return the timer labels held by the clock.
        
        :return List[Label].
        """
        return list(self._timers.keys())

    cpdef void register_logger(self, LoggerAdapter logger) except *:
        """
        Register the given logger with the clock.
        
        :param logger: The logger to register.
        """
        Condition.not_none(logger, 'logger')

        self._log = logger
        self.is_logger_registered = True

    cpdef void register_default_handler(self, handler: callable) except *:
        """
        Register the given handler as the clocks default handler.
        
        :param handler: The handler to register (must be Callable).
        :raises TypeError: If the handler is not of type callable.
        """
        Condition.callable(handler, 'handler')

        self._default_handler = handler
        self.is_default_handler_registered = True

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
        :param handler: The optional handler to receive time events (must be Callable).
        :raises ValueError: If the label is not unique for this clock.
        :raises ValueError: If the alert_time is not >= the clocks current time.
        :raises TypeError: If the handler is not of type Callable or None.
        :raises ValueError: If the handler is None and no default handler is registered.
        """
        Condition.not_none(label, 'label')
        Condition.not_none(alert_time, 'alert_time')
        if handler is None:
            handler = self._default_handler
        Condition.not_in(label, self._timers, 'label', 'timers')
        Condition.not_in(label, self._handlers, 'label', 'timers')
        cdef datetime now = self.time_now()
        Condition.true(alert_time >= now, 'alert_time >= time_now()')
        Condition.callable(handler, 'handler')

        cdef Timer timer = self._get_timer(
            label=label,
            callback=handler,
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
        :param handler: The optional handler to receive time events (must be Callable or None).
        :raises ValueError: If the label is not unique for this clock.
        :raises ValueError: If the interval is not positive (> 0).
        :raises ValueError: If the stop_time is not None and stop_time < time_now.
        :raises ValueError: If the stop_time is not None and start_time + interval > stop_time.
        :raises TypeError: If the handler is not of type Callable or None.
        :raises ValueError: If the handler is None and no default handler is registered.
        """
        Condition.not_none(label, 'label')
        Condition.not_none(interval, 'interval')
        if handler is None:
            handler = self._default_handler
        Condition.not_in(label, self._timers, 'label', 'timers')
        Condition.not_in(label, self._handlers, 'label', 'timers')
        Condition.true(interval.total_seconds() > 0, 'interval positive')
        Condition.callable(handler, 'handler')
        cdef datetime now = self.time_now()
        if start_time is None:
            start_time = now
        if stop_time is not None:
            Condition.true(stop_time > now, 'stop_time > now')
            Condition.true(start_time + interval <= stop_time, 'start_time + interval <= stop_time')

        cdef Timer timer = self._get_timer(
            label=label,
            interval=interval,
            callback=handler,
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
        Condition.not_none(label, 'label')

        cdef Timer timer = self._timers.pop(label, None)
        if timer is None:
            if self.is_logger_registered:
                self._log.warning(f"Cannot cancel timer (no timer found with label '{label.value}').")
        else:
            timer.cancel()
            if self.is_logger_registered:
                self._log.info(f"Cancelled Timer(label={timer.label.value}).")
            self._handlers.pop(label, None)
            self._remove_timer(timer)

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
            callback,
            timedelta interval,
            datetime now,
            datetime start_time,
            datetime stop_time):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef void _add_timer(self, Timer timer, handler) except *:
        self._timers[timer.label] = timer
        self._handlers[timer.label] = handler
        self._update_stack()
        self._update_timing()

    cdef void _remove_timer(self, Timer timer) except *:
        self._timers.pop(timer.label, None)
        self._handlers.pop(timer.label, None)
        self._update_stack()
        self._update_timing()

    cdef void _update_stack(self) except *:
        self.timer_count = len(self._timers)

        if self.timer_count > 0:
            self._stack = np.asarray(list(self._timers.values()))
        else:
            self._stack = None

    cdef void _update_timing(self) except *:
        if self.timer_count == 0:
            self.next_event_time = None
            return
        elif self.timer_count == 1:
            self.next_event_time = self._stack[0].next_time
            return

        cdef datetime next_time = self._stack[0].next_time
        cdef datetime observed
        cdef int i
        for i in range(self.timer_count - 1):
            observed = self._stack[i + 1].next_time
            if observed < next_time:
                next_time = observed

        self.next_event_time = next_time


cdef class TestClock(Clock):
    """
    Provides a clock for backtesting and unit testing.
    """
    __test__ = False

    def __init__(self, datetime initial_time not None=_UNIX_EPOCH):
        """
        Initializes a new instance of the TestClock class.

        :param initial_time: The initial time for the clock.
        """
        super().__init__(TestGuidFactory())

        self._time = initial_time
        self.is_test_clock = True

    cpdef datetime time_now(self):
        """
        Return the current datetime of the clock (UTC).

        :return datetime.
        """
        return self._time

    cpdef void set_time(self, datetime to_time) except *:
        """
        Set the clocks datetime to the given time (UTC).
        
        :param to_time: The time to set to.
        """
        Condition.not_none(to_time, 'to_time')

        self._time = to_time

    cpdef list advance_time(self, datetime to_time):
        """
        Iterates the clocks time to the given datetime.

        :param to_time: The datetime to iterate the test clock to.
        """
        Condition.not_none(to_time, 'to_time')

        cdef list events = []

        if self.timer_count == 0 or to_time < self.next_event_time:
            self._time = to_time
            return events  # No timer events to iterate

        # Iterate timer events
        cdef TestTimer timer
        cdef TimeEvent event
        for timer in self._stack:
            for event in timer.advance(to_time):
                events.append(TimeEventHandler(event, timer.callback))

        # Remove expired timers
        for timer in self._stack:
            if timer.expired:
                self._remove_timer(timer)

        self._update_timing()
        self._time = to_time
        return events

    cdef object _get_timer(
            self,
            Label label,
            callback,
            timedelta interval,
            datetime now,
            datetime start_time,
            datetime stop_time):
        return TestTimer(
            label=label,
            callback=callback,
            interval=interval,
            start_time=start_time,
            stop_time=stop_time)
