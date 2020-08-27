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
from cpython.datetime cimport datetime, timedelta

from nautilus_trader.core.message cimport Event
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.core.datetime cimport format_iso8601

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
_UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc)


cdef class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """

    def __init__(self,
                 str name not None,
                 UUID event_id not None,
                 datetime event_timestamp not None):
        """
        Initialize a new instance of the TimeEvent class.

        :param event_id: The event label.
        :param event_id: The event identifier.
        :param event_timestamp: The event timestamp.
        """
        # Precondition: name checked in Timer
        super().__init__(event_id, event_timestamp)

        self.name = name

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
                f"name={self.name}, "
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
                 str name not None,
                 callback not None,
                 timedelta interval not None,
                 datetime start_time not None,
                 datetime stop_time=None):
        """
        Initialize a new instance of the Timer class.

        :param name: The name for the timer.
        :param callback: The function to call at the next time.
        :param interval: The time interval for the timer (not negative).
        :param start_time: The start datetime for the timer (UTC).
        :param stop_time: The optional stop datetime for the timer (UTC) (if None then timer repeats).
        """
        Condition.valid_string(name, "name")
        Condition.callable(callback, "function")
        Condition.positive(interval.total_seconds(), "interval")
        if stop_time:
            Condition.true(start_time + interval <= stop_time, "start_time + interval <= stop_time")

        self.name = name
        self.callback = callback
        self.interval = interval
        self.start_time = start_time
        self.next_time = start_time + interval
        self.stop_time = stop_time
        self.expired = False

    cpdef TimeEvent pop_event(self, UUID event_id):
        """
        Returns a generated time event with the given identifier.

        :param event_id: The identifier for the time event.
        """
        Condition.not_none(event_id, "event_id")

        return TimeEvent(self.name, event_id, self.next_time)

    cpdef void iterate_next_time(self, datetime now) except *:
        """
        Iterates the timers next time and checks if the timer is now expired.

        :param now: The datetime now (UTC).
        """
        Condition.not_none(now, "now")

        self.next_time += self.interval
        if self.stop_time and now >= self.stop_time:
            self.expired = True

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not raise an event).
        """
        raise NotImplementedError("method must be implemented in the subclass")

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.name)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return (f"Timer("
                f"name={self.name}, "
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
