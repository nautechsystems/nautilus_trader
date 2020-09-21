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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.backtest.clock cimport Clock
from nautilus_trader.backtest.uuid cimport TestUUIDFactory
from nautilus_trader.common.clock cimport Timer
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.core.correctness cimport Condition

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
_UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc)


cdef class TestTimer(Timer):
    """
    Provides a fake timer for backtesting and unit testing.
    """
    __test__ = False

    def __init__(self,
                 str name not None,
                 callback not None,
                 timedelta interval not None,
                 datetime start_time not None,
                 datetime stop_time=None):
        """
        Initialize a new instance of the TestTimer class.

        :param name: The name for the timer.
        :param interval: The time interval for the timer (not negative).
        :param start_time: The stop datetime for the timer (UTC).
        :param stop_time: The optional stop datetime for the timer (UTC) (if None then timer repeats).
        """
        Condition.valid_string(name, "name")
        super().__init__(name, callback, interval, start_time, stop_time)

        self._uuid_factory = TestUUIDFactory()

    cpdef list advance(self, datetime to_time):
        """
        Return a list of time events by advancing the test timer forward to
        the given time. A time event is appended for each time a next event is
        <= the given to_time.

        :param to_time: The time to advance the test timer to.
        :return List[TimeEvent].
        """
        Condition.not_none(to_time, "to_time")

        cdef list events = []  # type: [TimeEvent]
        while not self.expired and to_time >= self.next_time:
            events.append(self.pop_event(self._uuid_factory.generate()))
            self.iterate_next_time(self.next_time)

        return events

    cpdef Event pop_next_event(self):
        """
        Return the next time event for this timer.

        :return TimeEvent.
        :raises ValueError: If the next event timestamp is not equal to the at_time.
        """
        cdef TimeEvent event = self.pop_event(self._uuid_factory.generate())
        self.iterate_next_time(self.next_time)

        return event

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self.expired = True


cdef class TestClock(Clock):
    """
    Provides a clock for backtesting and unit testing.
    """
    __test__ = False

    def __init__(self, datetime initial_time not None=_UNIX_EPOCH):
        """
        Initialize a new instance of the TestClock class.

        :param initial_time: The initial time for the clock.
        """
        super().__init__(TestUUIDFactory())

        self._time = initial_time
        self.is_test_clock = True

    cpdef datetime utc_now(self):
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
        Condition.not_none(to_time, "to_time")

        self._time = to_time

    cpdef list advance_time(self, datetime to_time):
        """
        Iterates the clocks time to the given datetime.

        :param to_time: The datetime to iterate the test clock to.
        """
        Condition.not_none(to_time, "to_time")

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

    cdef Timer _get_timer(
            self,
            str name,
            callback,
            timedelta interval,
            datetime now,
            datetime start_time,
            datetime stop_time):
        return TestTimer(
            name=name,
            callback=callback,
            interval=interval,
            start_time=start_time,
            stop_time=stop_time)
